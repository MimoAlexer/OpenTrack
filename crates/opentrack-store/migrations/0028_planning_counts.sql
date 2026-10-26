-- Current per-issue planning facts; memberships and authorization remain source-owned.
ALTER TABLE jobs DROP CONSTRAINT jobs_kind_check;
ALTER TABLE jobs ADD CONSTRAINT jobs_kind_check CHECK(kind IN('workflow.status_replace','assignment.reconcile','issues.order_normalize','planning.backfill'));
ALTER TABLE jobs DROP CONSTRAINT jobs_requester_shape;
ALTER TABLE jobs ADD CONSTRAINT jobs_requester_shape CHECK(
 (kind IN('workflow.status_replace','issues.order_normalize') AND requested_by IS NOT NULL)
 OR (kind IN('assignment.reconcile','planning.backfill') AND requested_by IS NULL));
ALTER TABLE consumer_receipts DROP CONSTRAINT consumer_receipts_state_check;
ALTER TABLE consumer_receipts ADD CONSTRAINT consumer_receipts_state_check CHECK(state IN('ignored','captured','reconciled','applied'));
ALTER TABLE consumer_receipts DROP CONSTRAINT consumer_receipts_check;
ALTER TABLE consumer_receipts ADD CONSTRAINT consumer_receipts_check CHECK((state IN('ignored','applied'))=(job_id IS NULL));
CREATE OR REPLACE FUNCTION maintenance_job_authority() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF NEW.kind IN('assignment.reconcile','planning.backfill') AND current_user<>pg_get_userbyid((SELECT relowner FROM pg_class WHERE oid=TG_RELID))
 AND NOT has_function_privilege(current_user,'worker_lock_maintenance_context(uuid)','EXECUTE') THEN
  RAISE EXCEPTION 'Maintenance work requires worker authority' USING ERRCODE='42501';
 END IF;RETURN NEW;
END $$;
CREATE TABLE planning_issue_facts (
 workspace_id uuid NOT NULL,consumer_version integer NOT NULL DEFAULT 1 CHECK(consumer_version=1),
 issue_id uuid NOT NULL,issue_version bigint NOT NULL CHECK(issue_version>0),
 category text NOT NULL CHECK(category IN('backlog','unstarted','started','completed','canceled')),
 estimate integer CHECK(estimate BETWEEN 0 AND 1000),excluded boolean NOT NULL,
 calculated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(workspace_id,consumer_version,issue_id),
 FOREIGN KEY(workspace_id,issue_id) REFERENCES issues(workspace_id,id) ON DELETE CASCADE
);
CREATE TABLE planning_backfills (
 workspace_id uuid NOT NULL,job_id uuid NOT NULL,consumer_version integer NOT NULL DEFAULT 1 CHECK(consumer_version=1),
 target_seq bigint NOT NULL CHECK(target_seq>=0),last_issue_id uuid,
 PRIMARY KEY(workspace_id,job_id),FOREIGN KEY(workspace_id,job_id) REFERENCES jobs(workspace_id,id)
);
DO $$ DECLARE tab text;r text;BEGIN
 FOREACH tab IN ARRAY ARRAY['planning_issue_facts','planning_backfills'] LOOP
  EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',tab);
  EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',tab);
  EXECUTE format('CREATE POLICY tenant_scope ON %I USING(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid) WITH CHECK(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid)',tab);
  FOR r IN SELECT grantee FROM information_schema.role_table_grants WHERE table_schema=current_schema() AND table_name=tab AND grantee<>current_user LOOP
   EXECUTE format('REVOKE ALL ON %I FROM %I',tab,r);
   IF tab='planning_issue_facts' THEN EXECUTE format('GRANT SELECT ON %I TO %I',tab,r);END IF;
  END LOOP;
 END LOOP;
END $$;
CREATE FUNCTION register_planning_counts_consumer(w uuid) RETURNS void LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE saved text:=current_setting('opentrack.workspace_id',true);n bigint;
BEGIN
 PERFORM set_config('opentrack.workspace_id',w::text,true);
 INSERT INTO consumer_registrations(workspace_id,consumer_name,consumer_version) VALUES(w,'planning_counts',1);
 GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
 INSERT INTO consumer_checkpoints(workspace_id,consumer_name,consumer_version) VALUES(w,'planning_counts',1);
 GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
 INSERT INTO worker_workspace_registry(workspace_id) VALUES(w) ON CONFLICT DO NOTHING;
 IF NOT EXISTS(SELECT 1 FROM worker_workspace_registry WHERE workspace_id=w) THEN RAISE EXCEPTION 'Required planning discovery registration missing';END IF;
 PERFORM set_config('opentrack.workspace_id',coalesce(saved,''),true);
END $$;
CREATE OR REPLACE FUNCTION register_workspace_consumers() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER AS $$ BEGIN
 PERFORM register_assignment_consumer(NEW.workspace_id);
 PERFORM register_planning_counts_consumer(NEW.workspace_id);
 RETURN NEW;
END $$;
ALTER TABLE workspaces NO FORCE ROW LEVEL SECURITY;
DO $$ DECLARE w uuid;BEGIN
 FOR w IN SELECT id FROM workspaces ORDER BY id LOOP PERFORM register_planning_counts_consumer(w);END LOOP;
END $$;
ALTER TABLE workspaces FORCE ROW LEVEL SECURITY;

-- Equal-version encounters deliberately preserve the original calculation time.
CREATE FUNCTION planning_refresh_issue(w uuid,i uuid) RETURNS void LANGUAGE plpgsql AS $$
DECLARE source record;n bigint;
BEGIN
 SELECT issue.version,status.category,issue.estimate,(issue.archived_at IS NOT NULL OR issue.deleted_at IS NOT NULL) AS excluded
 INTO source FROM issues issue JOIN workflow_statuses status ON status.workspace_id=issue.workspace_id AND status.team_id=issue.team_id AND status.id=issue.status_id
 WHERE issue.workspace_id=w AND issue.id=i;
 IF FOUND THEN
  INSERT INTO planning_issue_facts(workspace_id,consumer_version,issue_id,issue_version,category,estimate,excluded)
  VALUES(w,1,i,source.version,source.category,source.estimate,source.excluded)
  ON CONFLICT(workspace_id,consumer_version,issue_id) DO UPDATE SET issue_version=excluded.issue_version,category=excluded.category,estimate=excluded.estimate,excluded=excluded.excluded,calculated_at=clock_timestamp()
  WHERE planning_issue_facts.issue_version<excluded.issue_version;
  IF NOT EXISTS(SELECT 1 FROM planning_issue_facts WHERE workspace_id=w AND consumer_version=1 AND issue_id=i AND issue_version>=source.version) THEN
   RAISE EXCEPTION 'Required planning fact missing';
  END IF;
 ELSE
  DELETE FROM planning_issue_facts WHERE workspace_id=w AND consumer_version=1 AND issue_id=i;
  IF EXISTS(SELECT 1 FROM planning_issue_facts WHERE workspace_id=w AND consumer_version=1 AND issue_id=i) THEN RAISE EXCEPTION 'Required planning removal missing';END IF;
 END IF;
END $$;
CREATE FUNCTION enqueue_planning_backfill(w uuid,h bigint) RETURNS uuid LANGUAGE plpgsql AS $$
DECLARE j uuid:=gen_random_uuid();n bigint;
BEGIN
 INSERT INTO jobs(workspace_id,id,kind,requested_by,dedupe_key) VALUES(w,j,'planning.backfill',NULL,'planning:backfill:v1:'||j::text);
 GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
 INSERT INTO planning_backfills(workspace_id,job_id,target_seq) VALUES(w,j,h);
 GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
 UPDATE consumer_checkpoints SET backfill_job_id=j,updated_at=clock_timestamp() WHERE workspace_id=w AND consumer_name='planning_counts' AND consumer_version=1;
 GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
 RETURN j;
END $$;
CREATE FUNCTION worker_apply_planning_events(w uuid,batch_limit integer)
RETURNS TABLE(processed bigint,captured bigint,backfilled boolean,dead_lettered boolean) LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE cp record;e record;dl record;receipt record;head bigint;j uuid;n bigint;relevant boolean;failed boolean;code text;prior_seq bigint;letter_id uuid;attempt integer;
BEGIN
 processed:=0;captured:=0;backfilled:=false;dead_lettered:=false;
 IF batch_limit<1 OR batch_limit>100 THEN RAISE EXCEPTION 'Invalid planning event batch bound' USING ERRCODE='22023';END IF;
 IF NOT worker_lock_maintenance_context(w) THEN RETURN NEXT;RETURN;END IF;
 PERFORM 1 FROM consumer_registrations WHERE workspace_id=w AND consumer_name='planning_counts' AND consumer_version=1 AND required AND enabled FOR SHARE;
 IF NOT FOUND THEN RETURN NEXT;RETURN;END IF;
 SELECT * INTO STRICT cp FROM consumer_checkpoints WHERE workspace_id=w AND consumer_name='planning_counts' AND consumer_version=1 FOR UPDATE;
 IF cp.backfill_job_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM jobs WHERE workspace_id=w AND id=cp.backfill_job_id AND kind='planning.backfill' AND state='succeeded') THEN RETURN NEXT;RETURN;END IF;
 FOR e IN SELECT * FROM (
  SELECT 0::bigint AS seq,NULL::uuid AS event_id,'backfill'::text AS event_type,1 AS schema_version,w AS entity_id,0::bigint AS entity_version WHERE cp.backfill_job_id IS NULL
  UNION ALL SELECT ev.seq,ev.event_id,ev.event_type,ev.schema_version,ev.entity_id,ev.entity_version FROM workspace_events ev WHERE ev.workspace_id=w AND ev.seq>cp.last_seq AND cp.backfill_job_id IS NOT NULL
 ) pending ORDER BY seq LIMIT batch_limit LOOP
  SELECT * INTO dl FROM consumer_dead_letters WHERE workspace_id=w AND consumer_name='planning_counts' AND consumer_version=1 AND seq=e.seq FOR UPDATE;
  IF FOUND AND (dl.state='blocked' OR (dl.state='retry_wait' AND dl.available_at>clock_timestamp())) THEN EXIT;END IF;
  failed:=false;relevant:=false;code:='effect_failed';prior_seq:=cp.last_seq;
  BEGIN
   IF e.seq=0 THEN
    SELECT last_seq INTO STRICT head FROM workspace_stream_heads WHERE workspace_id=w;
    j:=enqueue_planning_backfill(w,head);
   ELSE
   IF e.seq<>cp.last_seq+1 THEN RAISE EXCEPTION 'Planning event sequence gap';END IF;
   relevant:=starts_with(e.event_type,'issue.');
   IF relevant AND (e.schema_version<>1 OR e.entity_version<1) THEN code:='unsupported_event_schema';RAISE EXCEPTION 'Unsupported planning event schema';END IF;
   SELECT * INTO receipt FROM consumer_receipts WHERE workspace_id=w AND consumer_name='planning_counts' AND consumer_version=1 AND event_id=e.event_id;
   IF FOUND AND (receipt.seq<>e.seq OR receipt.state<>CASE WHEN relevant THEN 'applied' ELSE 'ignored' END OR receipt.job_id IS NOT NULL) THEN RAISE EXCEPTION 'Inconsistent planning receipt';END IF;
   IF relevant THEN PERFORM planning_refresh_issue(w,e.entity_id);END IF;
   IF receipt.event_id IS NULL THEN
    INSERT INTO consumer_receipts(workspace_id,consumer_name,consumer_version,event_id,seq,state) VALUES(w,'planning_counts',1,e.event_id,e.seq,CASE WHEN relevant THEN 'applied' ELSE 'ignored' END);
    GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
   END IF;
   UPDATE consumer_checkpoints SET last_seq=e.seq,updated_at=clock_timestamp() WHERE workspace_id=w AND consumer_name='planning_counts' AND consumer_version=1 AND last_seq=cp.last_seq;
   GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
   END IF;
   IF dl.id IS NOT NULL AND dl.state<>'resolved' THEN
    UPDATE consumer_dead_letters SET state='resolved',resolved_at=clock_timestamp() WHERE workspace_id=w AND id=dl.id;
    GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
    IF EXISTS(SELECT 1 FROM consumer_alerts WHERE workspace_id=w AND dead_letter_id=dl.id) THEN
     UPDATE consumer_alerts SET resolved_at=clock_timestamp() WHERE workspace_id=w AND dead_letter_id=dl.id;GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
    END IF;
   END IF;
   IF e.seq>0 THEN cp.last_seq:=e.seq;END IF;
  EXCEPTION WHEN query_canceled OR OTHERS THEN failed:=true;
  END;
  IF failed THEN
   cp.last_seq:=prior_seq;
   INSERT INTO consumer_dead_letters(workspace_id,id,consumer_name,consumer_version,seq,event_id,event_type,schema_version,attempts,state,error_code,available_at)
   VALUES(w,gen_random_uuid(),'planning_counts',1,e.seq,e.event_id,e.event_type,e.schema_version,1,'retry_wait',code,clock_timestamp()+interval '2 seconds')
   ON CONFLICT(workspace_id,consumer_name,consumer_version,seq) DO UPDATE SET attempts=consumer_dead_letters.attempts+1,state=CASE WHEN consumer_dead_letters.attempts+1>=5 THEN 'blocked' ELSE 'retry_wait' END,error_code=excluded.error_code,available_at=clock_timestamp()+make_interval(secs=>least(900,power(2,least(consumer_dead_letters.attempts+1,9))))
   RETURNING id,attempts INTO letter_id,attempt;
   IF NOT FOUND THEN RAISE EXCEPTION 'Required planning failure record missing';END IF;
   IF attempt>=5 THEN
    INSERT INTO consumer_alerts(workspace_id,dead_letter_id) VALUES(w,letter_id) ON CONFLICT DO NOTHING;
    IF NOT EXISTS(SELECT 1 FROM consumer_alerts WHERE workspace_id=w AND dead_letter_id=letter_id) THEN RAISE EXCEPTION 'Required planning alert missing';END IF;
    dead_lettered:=true;
   END IF;
   EXIT;
  END IF;
  IF e.seq=0 THEN backfilled:=true;captured:=captured+1;EXIT;END IF;
  processed:=processed+1;IF relevant THEN captured:=captured+1;END IF;
 END LOOP;
 RETURN NEXT;
END $$;
CREATE FUNCTION worker_planning_backfill_batch(w uuid,j uuid,owner_id uuid,fence bigint,batch_limit integer) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE progress record;i uuid;last_id uuid;n bigint;expected bigint;
BEGIN
 IF batch_limit<1 OR batch_limit>100 THEN RAISE EXCEPTION 'Invalid planning backfill bound' USING ERRCODE='22023';END IF;
 IF w IS DISTINCT FROM nullif(current_setting('opentrack.workspace_id',true),'')::uuid THEN RAISE EXCEPTION 'Planning scope changed' USING ERRCODE='40001';END IF;
 PERFORM 1 FROM jobs WHERE workspace_id=w AND id=j AND kind='planning.backfill' AND state='running' AND lease_owner=owner_id AND lease_token=fence AND lease_expires_at>clock_timestamp() FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Planning backfill lease changed' USING ERRCODE='40001';END IF;
 PERFORM 1 FROM consumer_checkpoints WHERE workspace_id=w AND consumer_name='planning_counts' AND consumer_version=1 AND backfill_job_id=j FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Planning backfill superseded' USING ERRCODE='40001';END IF;
 SELECT * INTO STRICT progress FROM planning_backfills WHERE workspace_id=w AND job_id=j FOR UPDATE;
 last_id:=progress.last_issue_id;
 FOR i IN SELECT id FROM issues WHERE workspace_id=w AND (last_id IS NULL OR id>last_id) ORDER BY id LIMIT batch_limit LOOP
  PERFORM planning_refresh_issue(w,i);last_id:=i;
 END LOOP;
 UPDATE planning_backfills SET last_issue_id=last_id WHERE workspace_id=w AND job_id=j;
 GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
 IF NOT EXISTS(SELECT 1 FROM jobs WHERE workspace_id=w AND id=j AND lease_owner=owner_id AND lease_token=fence AND lease_expires_at>clock_timestamp()) THEN RAISE EXCEPTION 'Planning backfill lease expired' USING ERRCODE='40001';END IF;
 IF EXISTS(SELECT 1 FROM issues WHERE workspace_id=w AND (last_id IS NULL OR id>last_id)) THEN RETURN false;END IF;
 UPDATE consumer_checkpoints SET last_seq=progress.target_seq,updated_at=clock_timestamp() WHERE workspace_id=w AND consumer_name='planning_counts' AND consumer_version=1 AND backfill_job_id=j AND last_seq<=progress.target_seq;
 GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
 SELECT count(*) INTO expected FROM consumer_dead_letters WHERE workspace_id=w AND consumer_name='planning_counts' AND consumer_version=1 AND seq<=progress.target_seq AND state<>'resolved';
 UPDATE consumer_dead_letters SET state='resolved',resolved_at=clock_timestamp(),disposition='reconcile' WHERE workspace_id=w AND consumer_name='planning_counts' AND consumer_version=1 AND seq<=progress.target_seq AND state<>'resolved';
 GET DIAGNOSTICS n=ROW_COUNT;IF n<>expected THEN RAISE EXCEPTION 'Required planning repair disposition missing';END IF;
 SELECT count(*) INTO expected FROM consumer_alerts a JOIN consumer_dead_letters d ON d.workspace_id=a.workspace_id AND d.id=a.dead_letter_id WHERE d.workspace_id=w AND d.consumer_name='planning_counts' AND d.seq<=progress.target_seq AND a.resolved_at IS NULL;
 UPDATE consumer_alerts a SET resolved_at=clock_timestamp() FROM consumer_dead_letters d WHERE d.workspace_id=a.workspace_id AND d.id=a.dead_letter_id AND d.workspace_id=w AND d.consumer_name='planning_counts' AND d.seq<=progress.target_seq AND a.resolved_at IS NULL;
 GET DIAGNOSTICS n=ROW_COUNT;IF n<>expected THEN RAISE EXCEPTION 'Required planning repair alert resolution missing';END IF;
 RETURN true;
END $$;
CREATE FUNCTION operator_planning_rebuild(w uuid,reason_text text) RETURNS uuid LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE cp record;head bigint;j uuid;n bigint;
BEGIN
 IF NOT operator_lock_consumer_context(w) THEN RAISE EXCEPTION 'Workspace unavailable' USING ERRCODE='P0002';END IF;
 PERFORM 1 FROM consumer_registrations WHERE workspace_id=w AND consumer_name='planning_counts' AND consumer_version=1 AND required AND enabled FOR SHARE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Required planning registration unavailable';END IF;
 SELECT * INTO STRICT cp FROM consumer_checkpoints WHERE workspace_id=w AND consumer_name='planning_counts' AND consumer_version=1 FOR UPDATE;
 IF cp.backfill_job_id IS NOT NULL THEN
  UPDATE job_attempts SET ended_at=clock_timestamp(),outcome='blocked' WHERE workspace_id=w AND job_id=cp.backfill_job_id AND ended_at IS NULL;
  UPDATE jobs SET state='blocked',error_code='effect_failed',lease_owner=NULL,lease_expires_at=NULL,lease_token=lease_token+1,finished_at=clock_timestamp(),updated_at=clock_timestamp(),version=version+1 WHERE workspace_id=w AND id=cp.backfill_job_id AND state<>'succeeded';
 END IF;
 SELECT last_seq INTO STRICT head FROM workspace_stream_heads WHERE workspace_id=w;
 j:=enqueue_planning_backfill(w,head);
 RETURN consumer_operator_audit(w,j,'planning.rebuild',reason_text);
END $$;
CREATE OR REPLACE FUNCTION operator_consumer_status(w uuid) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE result jsonb;
BEGIN
 IF NOT worker_lock_maintenance_context(w) THEN RAISE EXCEPTION 'Workspace unavailable' USING ERRCODE='P0002';END IF;
 SELECT jsonb_build_object('workspace_id',w,'head_seq',h.last_seq::text,'purged_through_seq',r.purged_through_seq::text,
 'registrations',coalesce((SELECT jsonb_agg(jsonb_build_object('consumer_name',cr.consumer_name,'consumer_version',cr.consumer_version,'required',cr.required,'enabled',cr.enabled,'last_seq',cp.last_seq::text,'backfill_job_id',cp.backfill_job_id,'lag',(h.last_seq-cp.last_seq)::text)) FROM consumer_registrations cr LEFT JOIN consumer_checkpoints cp USING(workspace_id,consumer_name,consumer_version) WHERE cr.workspace_id=w),'[]'::jsonb),
 'dead_letters',coalesce((SELECT jsonb_agg(to_jsonb(d)) FROM (SELECT id,consumer_name,consumer_version,seq::text,event_type,schema_version,attempts,state,error_code,created_at,available_at FROM consumer_dead_letters WHERE workspace_id=w AND state<>'resolved' ORDER BY seq LIMIT 50) d),'[]'::jsonb),
 'failed_jobs',coalesce((SELECT jsonb_agg(to_jsonb(j)) FROM (SELECT id,kind,state,error_code,retry_count,created_at FROM jobs WHERE workspace_id=w AND kind IN('assignment.reconcile','planning.backfill') AND state IN('dead_letter','blocked','retry_wait') ORDER BY created_at,id LIMIT 50) j),'[]'::jsonb),
 'oldest_pending_at',(SELECT min(e.occurred_at) FROM workspace_events e WHERE e.workspace_id=w AND e.seq>(SELECT min(cp.last_seq) FROM consumer_checkpoints cp WHERE cp.workspace_id=w)))
 INTO result FROM workspace_stream_heads h JOIN event_retention_state r ON r.workspace_id=h.workspace_id WHERE h.workspace_id=w;
 IF result IS NULL THEN RAISE EXCEPTION 'Consumer state unavailable' USING ERRCODE='P0002';END IF;RETURN result;
END $$;
DO $$ DECLARE f regprocedure;BEGIN
 FOR f IN SELECT p.oid::regprocedure FROM pg_proc p WHERE p.pronamespace=current_schema()::regnamespace AND p.proname IN('maintenance_job_authority','register_planning_counts_consumer','register_workspace_consumers','planning_refresh_issue','enqueue_planning_backfill','worker_apply_planning_events','worker_planning_backfill_batch','operator_planning_rebuild','operator_consumer_status') LOOP
  EXECUTE format('ALTER FUNCTION %s SET search_path TO pg_catalog,%I,pg_temp',f,current_schema());
  EXECUTE format('REVOKE ALL ON FUNCTION %s FROM PUBLIC',f);
 END LOOP;
END $$;
