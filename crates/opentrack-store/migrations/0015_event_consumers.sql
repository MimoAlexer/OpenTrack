-- One exercised durable consumer: access changes -> assignment reconciliation.
-- No browser replay/SSE or generic external delivery framework is introduced.
ALTER TABLE workspace_events ADD COLUMN schema_version integer NOT NULL DEFAULT 1 CHECK(schema_version>0);
ALTER TABLE jobs DROP CONSTRAINT jobs_kind_check;
ALTER TABLE jobs ALTER COLUMN requested_by DROP NOT NULL;
ALTER TABLE jobs ADD CONSTRAINT jobs_kind_check CHECK(kind IN('workflow.status_replace','assignment.reconcile'));
ALTER TABLE jobs ADD CONSTRAINT jobs_requester_shape CHECK((kind='workflow.status_replace' AND requested_by IS NOT NULL) OR (kind='assignment.reconcile' AND requested_by IS NULL));
CREATE TABLE assignment_reconciliations (
 workspace_id uuid NOT NULL,id uuid NOT NULL,source_seq bigint NOT NULL CHECK(source_seq>=0),
 after_team_id uuid,after_issue_id uuid,scanned_count bigint NOT NULL DEFAULT 0 CHECK(scanned_count>=0),
 cleared_count bigint NOT NULL DEFAULT 0 CHECK(cleared_count>=0),
 PRIMARY KEY(workspace_id,id),FOREIGN KEY(workspace_id,id) REFERENCES jobs(workspace_id,id),
 CHECK((after_team_id IS NULL)=(after_issue_id IS NULL))
);
CREATE TABLE consumer_registrations (
 workspace_id uuid NOT NULL REFERENCES workspaces(id),consumer_name text NOT NULL,consumer_version integer NOT NULL CHECK(consumer_version>0),
 required boolean NOT NULL DEFAULT true,enabled boolean NOT NULL DEFAULT true,registered_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(workspace_id,consumer_name,consumer_version)
);
CREATE TABLE consumer_checkpoints (
 workspace_id uuid NOT NULL,consumer_name text NOT NULL,consumer_version integer NOT NULL,
 last_seq bigint NOT NULL DEFAULT 0 CHECK(last_seq>=0),backfill_job_id uuid,updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(workspace_id,consumer_name,consumer_version),
 FOREIGN KEY(workspace_id,consumer_name,consumer_version) REFERENCES consumer_registrations,
 FOREIGN KEY(workspace_id,backfill_job_id) REFERENCES jobs(workspace_id,id)
);
CREATE TABLE consumer_receipts (
 workspace_id uuid NOT NULL,consumer_name text NOT NULL,consumer_version integer NOT NULL,event_id uuid NOT NULL,seq bigint NOT NULL CHECK(seq>0),
 state text NOT NULL CHECK(state IN('ignored','captured','reconciled')),job_id uuid,created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(workspace_id,consumer_name,consumer_version,event_id),UNIQUE(workspace_id,consumer_name,consumer_version,seq),
 FOREIGN KEY(workspace_id,consumer_name,consumer_version) REFERENCES consumer_registrations,
 FOREIGN KEY(workspace_id,job_id) REFERENCES jobs(workspace_id,id),CHECK((state='ignored')=(job_id IS NULL))
);
-- Recovery metadata deliberately has no FK to the replay rows which can expire.
CREATE TABLE consumer_dead_letters (
 workspace_id uuid NOT NULL,id uuid NOT NULL,consumer_name text NOT NULL,consumer_version integer NOT NULL,
 seq bigint NOT NULL CHECK(seq>=0),event_id uuid,event_type text,schema_version integer,
 attempts integer NOT NULL CHECK(attempts>0),state text NOT NULL CHECK(state IN('retry_wait','blocked','resolved')),
 error_code text NOT NULL CHECK(error_code IN('effect_failed','unsupported_event_schema')),
 available_at timestamptz NOT NULL,created_at timestamptz NOT NULL DEFAULT clock_timestamp(),resolved_at timestamptz,
 disposition text CHECK(disposition IN('retry','reconcile')),reason text,
 PRIMARY KEY(workspace_id,id),UNIQUE(workspace_id,consumer_name,consumer_version,seq),
 FOREIGN KEY(workspace_id,consumer_name,consumer_version) REFERENCES consumer_registrations
);
CREATE TABLE consumer_alerts (
 workspace_id uuid NOT NULL,dead_letter_id uuid NOT NULL,created_at timestamptz NOT NULL DEFAULT clock_timestamp(),resolved_at timestamptz,
 PRIMARY KEY(workspace_id,dead_letter_id),FOREIGN KEY(workspace_id,dead_letter_id) REFERENCES consumer_dead_letters(workspace_id,id)
);
CREATE TABLE event_retention_state (
 workspace_id uuid PRIMARY KEY REFERENCES workspaces(id),purged_through_seq bigint NOT NULL DEFAULT 0 CHECK(purged_through_seq>=0)
);
CREATE TABLE consumer_operations (
 workspace_id uuid NOT NULL REFERENCES workspaces(id),id uuid NOT NULL,action text NOT NULL,
 through_seq bigint NOT NULL,affected_count bigint NOT NULL CHECK(affected_count>=0),reason text,operator_identity text,created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(workspace_id,id)
);
CREATE INDEX assignment_scan ON issues(workspace_id,team_id,id) WHERE assignee_id IS NOT NULL AND archived_at IS NULL AND deleted_at IS NULL;
CREATE INDEX consumer_pending_letters ON consumer_dead_letters(workspace_id,seq) WHERE state<>'resolved';
CREATE INDEX assignment_pending_jobs ON jobs(workspace_id,id) WHERE kind='assignment.reconcile' AND state<>'succeeded';
CREATE INDEX events_retention ON workspace_events(workspace_id,seq,occurred_at);
DO $$ DECLARE tab text; BEGIN
 FOREACH tab IN ARRAY ARRAY['assignment_reconciliations','consumer_registrations','consumer_checkpoints','consumer_receipts','consumer_dead_letters','consumer_alerts','event_retention_state','consumer_operations'] LOOP
  EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',tab);
  EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',tab);
  EXECUTE format('CREATE POLICY tenant_scope ON %I USING (workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid) WITH CHECK (workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid)',tab);
 END LOOP;
END $$;
-- Internal helpers are never executable by PUBLIC or the serving login.
CREATE FUNCTION consumer_required(affected bigint) RETURNS void LANGUAGE plpgsql AS $$ BEGIN
 IF affected<>1 THEN RAISE EXCEPTION 'Required consumer effect missing' USING ERRCODE='P0001'; END IF;
END $$;
CREATE FUNCTION register_assignment_consumer(w uuid) RETURNS void LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE saved text:=current_setting('opentrack.workspace_id',true); n bigint;
BEGIN
 PERFORM set_config('opentrack.workspace_id',w::text,true);
 INSERT INTO consumer_registrations(workspace_id,consumer_name,consumer_version) VALUES(w,'assignment_cleanup',1); GET DIAGNOSTICS n=ROW_COUNT; PERFORM consumer_required(n);
 INSERT INTO consumer_checkpoints(workspace_id,consumer_name,consumer_version) VALUES(w,'assignment_cleanup',1); GET DIAGNOSTICS n=ROW_COUNT; PERFORM consumer_required(n);
 INSERT INTO event_retention_state(workspace_id) VALUES(w); GET DIAGNOSTICS n=ROW_COUNT; PERFORM consumer_required(n);
 INSERT INTO worker_workspace_registry(workspace_id) VALUES(w) ON CONFLICT DO NOTHING;
 IF NOT EXISTS(SELECT 1 FROM worker_workspace_registry WHERE workspace_id=w) THEN RAISE EXCEPTION 'Required discovery registration missing'; END IF;
 PERFORM set_config('opentrack.workspace_id',coalesce(saved,''),true);
END $$;
CREATE FUNCTION register_workspace_consumers() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER AS $$ BEGIN
 PERFORM register_assignment_consumer(NEW.workspace_id); RETURN NEW;
END $$;
CREATE TRIGGER workspace_consumer_registration AFTER INSERT ON workspace_stream_heads FOR EACH ROW EXECUTE FUNCTION register_workspace_consumers();
DO $$ DECLARE w uuid; BEGIN
 FOR w IN SELECT id FROM workspaces LOOP PERFORM register_assignment_consumer(w); END LOOP;
END $$;
CREATE FUNCTION worker_lock_maintenance_context(w uuid) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER AS $$ BEGIN
 PERFORM set_config('opentrack.user_id','',true),set_config('opentrack.workspace_id',w::text,true),set_config('statement_timeout','3000',true),set_config('lock_timeout','1000',true),set_config('idle_in_transaction_session_timeout','5000',true);
 PERFORM 1 FROM workspace_auth_guards WHERE workspace_id=w FOR SHARE;
 RETURN FOUND;
END $$;
CREATE FUNCTION worker_assignee_eligible(w uuid,t uuid,u uuid) RETURNS boolean LANGUAGE sql SECURITY DEFINER AS $$
 SELECT w=nullif(current_setting('opentrack.workspace_id',true),'')::uuid AND EXISTS(
 SELECT 1 FROM workspace_members m JOIN users a ON a.id=m.user_id JOIN teams team ON team.workspace_id=m.workspace_id AND team.id=t
 WHERE m.workspace_id=w AND m.user_id=u AND m.active AND m.role<>'viewer' AND a.disabled_at IS NULL
 AND (NOT team.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=w AND tm.team_id=t AND tm.user_id=u)))
$$;
CREATE FUNCTION worker_clear_assignment(w uuid,t uuid,i uuid,prior_assignee uuid,j uuid,owner_id uuid,fence bigint) RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE v bigint;
BEGIN
 IF w IS DISTINCT FROM nullif(current_setting('opentrack.workspace_id',true),'')::uuid
 OR NOT EXISTS(SELECT 1 FROM jobs WHERE workspace_id=w AND id=j AND kind='assignment.reconcile' AND state='running' AND lease_owner=owner_id AND lease_token=fence AND lease_expires_at>clock_timestamp())
 OR worker_assignee_eligible(w,t,prior_assignee) THEN RAISE EXCEPTION 'Maintenance fence or eligibility changed' USING ERRCODE='40001';END IF;
 UPDATE issues SET assignee_id=NULL,version=version+1,updated_at=clock_timestamp()
 WHERE workspace_id=w AND team_id=t AND id=i AND assignee_id=prior_assignee AND archived_at IS NULL AND deleted_at IS NULL
 AND EXISTS(SELECT 1 FROM workflow_statuses s WHERE s.workspace_id=w AND s.team_id=t AND s.id=issues.status_id AND s.category NOT IN('completed','canceled'))
 AND EXISTS(SELECT 1 FROM teams team JOIN workspaces ws ON ws.id=team.workspace_id WHERE team.workspace_id=w AND team.id=t AND team.archived_at IS NULL AND ws.archived_at IS NULL)
 RETURNING version INTO v;
 IF NOT FOUND THEN RAISE EXCEPTION 'Required assignment clear missing';END IF;RETURN v;
END $$;
CREATE FUNCTION enqueue_assignment_reconciliation(w uuid,s bigint,dedupe text) RETURNS uuid LANGUAGE plpgsql AS $$
DECLARE j uuid:=gen_random_uuid(); n bigint;
BEGIN
 INSERT INTO jobs(workspace_id,id,kind,requested_by,dedupe_key) VALUES(w,j,'assignment.reconcile',NULL,dedupe);
 GET DIAGNOSTICS n=ROW_COUNT; PERFORM consumer_required(n);
 INSERT INTO assignment_reconciliations(workspace_id,id,source_seq) VALUES(w,j,s);
 GET DIAGNOSTICS n=ROW_COUNT; PERFORM consumer_required(n); RETURN j;
END $$;
-- A system effect is not attributed to a disabled/removed human or local operator.
ALTER TABLE activity_entries ALTER COLUMN actor_id DROP NOT NULL;
ALTER TABLE activity_entries ADD COLUMN actor_kind text NOT NULL DEFAULT 'account';
ALTER TABLE activity_entries ADD COLUMN system_job_id uuid;
ALTER TABLE audit_entries ADD COLUMN system_job_id uuid;
ALTER TABLE workspace_events ADD COLUMN system_job_id uuid;
ALTER TABLE activity_entries ADD CONSTRAINT activity_actor_shape CHECK((actor_kind='account' AND actor_id IS NOT NULL AND system_job_id IS NULL) OR (actor_kind='system' AND actor_id IS NULL AND system_job_id IS NOT NULL AND event_kind='issue.updated'));
ALTER TABLE activity_entries ADD FOREIGN KEY(workspace_id,system_job_id) REFERENCES jobs(workspace_id,id);
ALTER TABLE audit_entries DROP CONSTRAINT audit_actor_shape;
ALTER TABLE audit_entries ADD CONSTRAINT audit_actor_shape CHECK(
 (actor_kind='account' AND actor_id IS NOT NULL AND operator_identity IS NULL AND correlation_id IS NULL AND system_job_id IS NULL)
 OR (actor_kind='local_operator' AND actor_id IS NULL AND operator_identity IS NOT NULL AND length(operator_identity)>0 AND correlation_id IS NOT NULL AND system_job_id IS NULL)
 OR (actor_kind='system' AND actor_id IS NULL AND operator_identity IS NULL AND correlation_id=system_job_id AND system_job_id IS NOT NULL AND action='issue.assignment_cleared'));
ALTER TABLE audit_entries ADD FOREIGN KEY(workspace_id,system_job_id) REFERENCES jobs(workspace_id,id);
ALTER TABLE workspace_events DROP CONSTRAINT workspace_event_actor_shape;
ALTER TABLE workspace_events ADD CONSTRAINT workspace_event_actor_shape CHECK(
 (actor_kind='account' AND actor_id IS NOT NULL AND system_job_id IS NULL)
 OR (actor_kind='local_operator' AND actor_id IS NULL AND event_type='access.changed' AND team_id IS NULL AND entity_id=workspace_id AND system_job_id IS NULL)
 OR (actor_kind='system' AND actor_id IS NULL AND event_type='issue.updated' AND system_job_id IS NOT NULL AND mutation_id=system_job_id));
ALTER TABLE workspace_events ADD FOREIGN KEY(workspace_id,system_job_id) REFERENCES jobs(workspace_id,id);
CREATE FUNCTION system_effect_authority() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF NEW.actor_kind='system' THEN
  IF current_user<>pg_get_userbyid((SELECT relowner FROM pg_class WHERE oid=TG_RELID))
   AND NOT has_function_privilege(current_user,'worker_lock_maintenance_context(uuid)','EXECUTE') THEN
   RAISE EXCEPTION 'System attribution requires maintenance authority' USING ERRCODE='42501';
  END IF;
  IF NOT EXISTS(SELECT 1 FROM jobs WHERE workspace_id=NEW.workspace_id AND id=NEW.system_job_id AND kind='assignment.reconcile' AND state='running' AND lease_expires_at>clock_timestamp()) THEN
   RAISE EXCEPTION 'System effect requires a live maintenance job' USING ERRCODE='23514';
  END IF;
 END IF; RETURN NEW;
END $$;
CREATE TRIGGER activity_system_authority BEFORE INSERT ON activity_entries FOR EACH ROW EXECUTE FUNCTION system_effect_authority();
CREATE TRIGGER audit_system_authority BEFORE INSERT ON audit_entries FOR EACH ROW EXECUTE FUNCTION system_effect_authority();
CREATE TRIGGER event_system_authority BEFORE INSERT ON workspace_events FOR EACH ROW EXECUTE FUNCTION system_effect_authority();
CREATE FUNCTION maintenance_job_authority() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF NEW.kind='assignment.reconcile' AND current_user<>pg_get_userbyid((SELECT relowner FROM pg_class WHERE oid=TG_RELID))
 AND NOT has_function_privilege(current_user,'worker_lock_maintenance_context(uuid)','EXECUTE') THEN
  RAISE EXCEPTION 'Maintenance work requires worker authority' USING ERRCODE='42501';
 END IF; RETURN NEW;
END $$;
CREATE TRIGGER maintenance_job_authority BEFORE INSERT OR UPDATE ON jobs FOR EACH ROW EXECUTE FUNCTION maintenance_job_authority();
-- On capture failure the inner subtransaction rolls back all partial intent.
-- Retry/dead-letter metadata is then committed without advancing its checkpoint.
CREATE FUNCTION worker_capture_events(w uuid,batch_limit integer)
RETURNS TABLE(processed bigint,captured bigint,backfilled boolean,dead_lettered boolean) LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE cp record;e record;dl record;head bigint;j uuid;n bigint;next_seq bigint;failed boolean;code text;dl_id uuid;attempt integer;prior_processed bigint;prior_captured bigint;prior_backfilled boolean;prior_seq bigint;receipt record;relevant boolean;
BEGIN
 processed:=0;captured:=0;backfilled:=false;dead_lettered:=false;
 IF batch_limit<1 OR batch_limit>100 THEN RAISE EXCEPTION 'Invalid consumer batch bound' USING ERRCODE='22023'; END IF;
 IF NOT worker_lock_maintenance_context(w) THEN RETURN NEXT;RETURN; END IF;
 PERFORM 1 FROM consumer_registrations WHERE workspace_id=w AND consumer_name='assignment_cleanup' AND consumer_version=1 AND enabled FOR SHARE;
 IF NOT FOUND THEN RETURN NEXT;RETURN; END IF;
 SELECT * INTO STRICT cp FROM consumer_checkpoints WHERE workspace_id=w AND consumer_name='assignment_cleanup' AND consumer_version=1 FOR UPDATE;
 FOR e IN SELECT * FROM (SELECT 0::bigint AS seq,NULL::uuid AS event_id,'backfill'::text AS event_type,1 AS schema_version,NULL::uuid AS team_id,w AS entity_id,0::bigint AS entity_version,'system'::text AS actor_kind WHERE cp.backfill_job_id IS NULL
 UNION ALL SELECT ev.seq,ev.event_id,ev.event_type,ev.schema_version,ev.team_id,ev.entity_id,ev.entity_version,ev.actor_kind FROM workspace_events ev WHERE ev.workspace_id=w AND ev.seq>cp.last_seq AND cp.backfill_job_id IS NOT NULL) candidates ORDER BY seq LIMIT batch_limit
 LOOP
  next_seq:=e.seq;SELECT * INTO dl FROM consumer_dead_letters WHERE workspace_id=w AND consumer_name='assignment_cleanup' AND consumer_version=1 AND seq=next_seq FOR UPDATE;
  IF FOUND AND (dl.state='blocked' OR (dl.state='retry_wait' AND dl.available_at>clock_timestamp())) THEN EXIT; END IF;
  failed:=false;code:='effect_failed';j:=NULL;prior_processed:=processed;prior_captured:=captured;prior_backfilled:=backfilled;prior_seq:=cp.last_seq;
  BEGIN
   IF e.seq=0 THEN
    SELECT last_seq INTO STRICT head FROM workspace_stream_heads WHERE workspace_id=w;
    j:=enqueue_assignment_reconciliation(w,head,'assignment:backfill:v1');
    UPDATE consumer_checkpoints SET last_seq=head,backfill_job_id=j,updated_at=clock_timestamp() WHERE workspace_id=w AND consumer_name='assignment_cleanup' AND consumer_version=1;
    GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);backfilled:=true;captured:=captured+1;
   ELSE
    IF e.seq<>cp.last_seq+1 THEN RAISE EXCEPTION 'Consumer sequence gap'; END IF;
    relevant:=e.actor_kind<>'system' AND e.event_type IN('access.changed','workflow.replacement_progress','workflow.replacement_completed');
    IF relevant AND (e.schema_version<>1 OR (e.event_type='access.changed' AND (e.entity_id<>w OR e.team_id IS NOT NULL OR e.entity_version<1))) THEN code:='unsupported_event_schema';RAISE EXCEPTION 'Unsupported consumer schema';END IF;
    SELECT * INTO receipt FROM consumer_receipts WHERE workspace_id=w AND consumer_name='assignment_cleanup' AND consumer_version=1 AND event_id=e.event_id;
    IF FOUND THEN
     IF receipt.seq<>e.seq OR (relevant AND receipt.state='ignored') OR (receipt.job_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM assignment_reconciliations ar JOIN jobs job ON job.workspace_id=ar.workspace_id AND job.id=ar.id WHERE ar.workspace_id=w AND ar.id=receipt.job_id AND ar.source_seq=e.seq AND job.kind='assignment.reconcile')) THEN RAISE EXCEPTION 'Inconsistent consumer receipt';END IF;
    ELSE
     IF relevant THEN j:=enqueue_assignment_reconciliation(w,e.seq,'assignment:event:'||e.event_id::text);END IF;
     INSERT INTO consumer_receipts(workspace_id,consumer_name,consumer_version,event_id,seq,state,job_id) VALUES(w,'assignment_cleanup',1,e.event_id,e.seq,CASE WHEN j IS NULL THEN 'ignored' ELSE 'captured' END,j);
     GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
    END IF;
    UPDATE consumer_checkpoints SET last_seq=e.seq,updated_at=clock_timestamp() WHERE workspace_id=w AND consumer_name='assignment_cleanup' AND consumer_version=1 AND last_seq=cp.last_seq;
    GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
    cp.last_seq:=e.seq;processed:=processed+1;IF j IS NOT NULL THEN captured:=captured+1;END IF;
   END IF;
   IF dl.id IS NOT NULL THEN
    UPDATE consumer_dead_letters SET state='resolved',resolved_at=clock_timestamp() WHERE workspace_id=w AND id=dl.id;
    GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
    IF EXISTS(SELECT 1 FROM consumer_alerts WHERE workspace_id=w AND dead_letter_id=dl.id) THEN
     UPDATE consumer_alerts SET resolved_at=clock_timestamp() WHERE workspace_id=w AND dead_letter_id=dl.id;GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
    END IF;
   END IF;
  EXCEPTION WHEN query_canceled OR OTHERS THEN failed:=true;
  END;
  IF failed THEN
   processed:=prior_processed;captured:=prior_captured;backfilled:=prior_backfilled;cp.last_seq:=prior_seq;
   INSERT INTO consumer_dead_letters(workspace_id,id,consumer_name,consumer_version,seq,event_id,event_type,schema_version,attempts,state,error_code,available_at)
    VALUES(w,gen_random_uuid(),'assignment_cleanup',1,next_seq,e.event_id,e.event_type,e.schema_version,1,'retry_wait',code,clock_timestamp()+interval '2 seconds')
    ON CONFLICT(workspace_id,consumer_name,consumer_version,seq) DO UPDATE SET attempts=consumer_dead_letters.attempts+1,state=CASE WHEN consumer_dead_letters.attempts+1>=5 THEN 'blocked' ELSE 'retry_wait' END,error_code=excluded.error_code,available_at=clock_timestamp()+make_interval(secs=>least(900,power(2,least(consumer_dead_letters.attempts+1,9))))
    RETURNING id,attempts INTO dl_id,attempt;
   IF NOT FOUND THEN RAISE EXCEPTION 'Required consumer failure record missing'; END IF;
   IF attempt>=5 THEN
    INSERT INTO consumer_alerts(workspace_id,dead_letter_id) VALUES(w,dl_id) ON CONFLICT DO NOTHING;
    IF NOT EXISTS(SELECT 1 FROM consumer_alerts WHERE workspace_id=w AND dead_letter_id=dl_id) THEN RAISE EXCEPTION 'Required consumer alert missing'; END IF;
    dead_lettered:=true;
   END IF;EXIT;
  END IF;
  IF backfilled THEN EXIT;END IF;
 END LOOP;RETURN NEXT;
END $$;
CREATE FUNCTION worker_retain_events(w uuid,batch_limit integer)
RETURNS TABLE(deleted bigint,purged_through_seq bigint) LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE watermark bigint;floor bigint;candidate bigint;n bigint;expected bigint;
BEGIN
 deleted:=0;purged_through_seq:=0;
 IF batch_limit<1 OR batch_limit>1000 THEN RAISE EXCEPTION 'Invalid retention batch bound' USING ERRCODE='22023';END IF;
 IF NOT worker_lock_maintenance_context(w) THEN RETURN NEXT;RETURN;END IF;
 PERFORM 1 FROM consumer_registrations WHERE workspace_id=w ORDER BY consumer_name,consumer_version FOR SHARE;
 PERFORM 1 FROM consumer_checkpoints WHERE workspace_id=w ORDER BY consumer_name,consumer_version FOR SHARE;
 SELECT s.purged_through_seq INTO STRICT floor FROM event_retention_state s WHERE s.workspace_id=w FOR UPDATE;
 purged_through_seq:=floor;
 IF NOT EXISTS(SELECT 1 FROM consumer_registrations WHERE workspace_id=w AND consumer_name='assignment_cleanup' AND consumer_version=1 AND required)
 OR EXISTS(SELECT 1 FROM consumer_registrations r LEFT JOIN consumer_checkpoints c USING(workspace_id,consumer_name,consumer_version) WHERE r.workspace_id=w AND r.required AND (c.workspace_id IS NULL OR c.backfill_job_id IS NULL)) THEN RETURN NEXT;RETURN;END IF;
 SELECT min(c.last_seq) INTO watermark FROM consumer_registrations r JOIN consumer_checkpoints c USING(workspace_id,consumer_name,consumer_version) WHERE r.workspace_id=w AND r.required;
 SELECT max(seq),count(*) INTO candidate,expected FROM (SELECT ev.seq FROM workspace_events ev WHERE ev.workspace_id=w AND ev.seq>floor AND ev.seq<=watermark AND ev.occurred_at<clock_timestamp()-interval '7 days' AND ev.seq<coalesce((SELECT min(recent.seq) FROM workspace_events recent WHERE recent.workspace_id=w AND recent.seq>floor AND recent.occurred_at>=clock_timestamp()-interval '7 days'),9223372036854775807) ORDER BY ev.seq LIMIT batch_limit) old;
 IF candidate IS NULL THEN RETURN NEXT;RETURN;END IF;
 DELETE FROM workspace_events WHERE workspace_id=w AND seq>floor AND seq<=candidate;GET DIAGNOSTICS deleted=ROW_COUNT;
 IF deleted<>expected THEN RAISE EXCEPTION 'Required retention removal missing';END IF;
 UPDATE event_retention_state SET purged_through_seq=candidate WHERE workspace_id=w;GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
 INSERT INTO consumer_operations(workspace_id,id,action,through_seq,affected_count) VALUES(w,gen_random_uuid(),'events.retained',candidate,deleted);GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
 purged_through_seq:=candidate;RETURN NEXT;
END $$;
-- Operator commands have no direct table grant and require a bounded reason.
CREATE FUNCTION consumer_operator_audit(w uuid,target uuid,action_name text,reason_text text) RETURNS uuid LANGUAGE plpgsql AS $$
DECLARE correlation uuid:=gen_random_uuid();n bigint;
BEGIN
 IF reason_text IS NULL OR length(btrim(reason_text)) NOT BETWEEN 1 AND 500 OR reason_text~'[[:cntrl:]]' THEN RAISE EXCEPTION 'A bounded reason is required' USING ERRCODE='22023';END IF;
 INSERT INTO audit_entries(id,workspace_id,actor_kind,operator_identity,action,target_id,correlation_id) VALUES(correlation,w,'local_operator',session_user,action_name,target,correlation);GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
 INSERT INTO consumer_operations(workspace_id,id,action,through_seq,affected_count,reason,operator_identity) VALUES(w,correlation,action_name,0,1,btrim(reason_text),session_user);GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
 RETURN correlation;
END $$;
CREATE FUNCTION operator_lock_consumer_context(w uuid) RETURNS boolean LANGUAGE plpgsql AS $$ BEGIN
 PERFORM set_config('opentrack.user_id','',true),set_config('opentrack.workspace_id',w::text,true),set_config('statement_timeout','3000',true),set_config('lock_timeout','1000',true),set_config('idle_in_transaction_session_timeout','5000',true);
 PERFORM 1 FROM workspace_auth_guards WHERE workspace_id=w FOR UPDATE;RETURN FOUND;
END $$;
CREATE FUNCTION operator_consumer_disposition(w uuid,letter uuid,action_name text,reason_text text) RETURNS uuid LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE dl record;cp record;j uuid;correlation uuid;n bigint;
BEGIN
 IF NOT operator_lock_consumer_context(w) THEN RAISE EXCEPTION 'Workspace unavailable' USING ERRCODE='P0002';END IF;
 PERFORM 1 FROM consumer_registrations WHERE workspace_id=w AND consumer_name='assignment_cleanup' AND consumer_version=1 FOR SHARE;
 SELECT * INTO STRICT cp FROM consumer_checkpoints WHERE workspace_id=w AND consumer_name='assignment_cleanup' AND consumer_version=1 FOR UPDATE;
 SELECT * INTO dl FROM consumer_dead_letters WHERE workspace_id=w AND id=letter AND state IN('blocked','retry_wait') FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Dead letter unavailable' USING ERRCODE='P0002';END IF;
 IF action_name='retry' THEN
  UPDATE consumer_dead_letters SET attempts=1,state='retry_wait',available_at=clock_timestamp(),disposition='retry',reason=btrim(reason_text) WHERE workspace_id=w AND id=letter;GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
 ELSIF action_name='reconcile' AND dl.seq>0 AND dl.seq=cp.last_seq+1 THEN
  j:=enqueue_assignment_reconciliation(w,dl.seq,'assignment:disposition:'||dl.id::text);
  INSERT INTO consumer_receipts(workspace_id,consumer_name,consumer_version,event_id,seq,state,job_id) VALUES(w,'assignment_cleanup',1,dl.event_id,dl.seq,'reconciled',j);GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
  UPDATE consumer_checkpoints SET last_seq=dl.seq,updated_at=clock_timestamp() WHERE workspace_id=w AND consumer_name='assignment_cleanup' AND consumer_version=1;GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
  UPDATE consumer_dead_letters SET state='resolved',resolved_at=clock_timestamp(),disposition='reconcile',reason=btrim(reason_text) WHERE workspace_id=w AND id=letter;GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
  IF EXISTS(SELECT 1 FROM consumer_alerts WHERE workspace_id=w AND dead_letter_id=letter) THEN
   UPDATE consumer_alerts SET resolved_at=clock_timestamp() WHERE workspace_id=w AND dead_letter_id=letter;GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
  END IF;
 ELSE RAISE EXCEPTION 'Choose retry or a current-event reconciliation' USING ERRCODE='22023';END IF;
 correlation:=consumer_operator_audit(w,letter,'consumer.'||action_name,reason_text);RETURN correlation;
END $$;
CREATE FUNCTION operator_consumer_set_enabled(w uuid,is_enabled boolean,reason_text text) RETURNS uuid LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE last bigint;head bigint;n bigint;
BEGIN
 IF NOT operator_lock_consumer_context(w) THEN RAISE EXCEPTION 'Workspace unavailable' USING ERRCODE='P0002';END IF;
 PERFORM 1 FROM consumer_registrations WHERE workspace_id=w AND consumer_name='assignment_cleanup' AND consumer_version=1 FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Consumer unavailable' USING ERRCODE='P0002';END IF;
 SELECT last_seq INTO STRICT last FROM consumer_checkpoints WHERE workspace_id=w AND consumer_name='assignment_cleanup' AND consumer_version=1 FOR UPDATE;
 IF NOT is_enabled THEN
  SELECT last_seq INTO STRICT head FROM workspace_stream_heads WHERE workspace_id=w;
  IF last<>head OR EXISTS(SELECT 1 FROM consumer_checkpoints WHERE workspace_id=w AND backfill_job_id IS NULL) OR EXISTS(SELECT 1 FROM jobs WHERE workspace_id=w AND kind='assignment.reconcile' AND state<>'succeeded') THEN RAISE EXCEPTION 'Drain consumer and maintenance jobs before disabling' USING ERRCODE='23514',CONSTRAINT='consumer_must_be_drained';END IF;
 END IF;
 UPDATE consumer_registrations SET enabled=is_enabled WHERE workspace_id=w AND consumer_name='assignment_cleanup' AND consumer_version=1;GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
 RETURN consumer_operator_audit(w,w,CASE WHEN is_enabled THEN 'consumer.enabled' ELSE 'consumer.disabled' END,reason_text);
END $$;
CREATE FUNCTION operator_resume_assignment_cleanup(w uuid,j uuid,reason_text text) RETURNS uuid LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE n bigint;
BEGIN
 IF NOT operator_lock_consumer_context(w) THEN RAISE EXCEPTION 'Workspace unavailable' USING ERRCODE='P0002';END IF;
 UPDATE jobs SET state='queued',retry_count=0,error_code=NULL,available_at=clock_timestamp(),finished_at=NULL,version=version+1,updated_at=clock_timestamp() WHERE workspace_id=w AND id=j AND kind='assignment.reconcile' AND state IN('dead_letter','blocked','retry_wait');GET DIAGNOSTICS n=ROW_COUNT;
 IF n<>1 THEN RAISE EXCEPTION 'Maintenance job unavailable for resume' USING ERRCODE='P0002';END IF;
 RETURN consumer_operator_audit(w,j,'assignment.cleanup_resumed',reason_text);
END $$;
CREATE FUNCTION operator_consumer_status(w uuid) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE result jsonb;
BEGIN
 IF NOT worker_lock_maintenance_context(w) THEN RAISE EXCEPTION 'Workspace unavailable' USING ERRCODE='P0002';END IF;
 SELECT jsonb_build_object('workspace_id',w,'head_seq',h.last_seq::text,'purged_through_seq',r.purged_through_seq::text,
 'registrations',coalesce((SELECT jsonb_agg(jsonb_build_object('consumer_name',cr.consumer_name,'consumer_version',cr.consumer_version,'required',cr.required,'enabled',cr.enabled,'last_seq',cp.last_seq::text,'backfill_job_id',cp.backfill_job_id,'lag',(h.last_seq-cp.last_seq)::text)) FROM consumer_registrations cr LEFT JOIN consumer_checkpoints cp USING(workspace_id,consumer_name,consumer_version) WHERE cr.workspace_id=w),'[]'::jsonb),
 'dead_letters',coalesce((SELECT jsonb_agg(to_jsonb(d)) FROM (SELECT id,seq::text,event_type,schema_version,attempts,state,error_code,created_at,available_at FROM consumer_dead_letters WHERE workspace_id=w AND state<>'resolved' ORDER BY seq LIMIT 50) d),'[]'::jsonb),
 'failed_jobs',coalesce((SELECT jsonb_agg(to_jsonb(j)) FROM (SELECT id,state,error_code,retry_count,created_at FROM jobs WHERE workspace_id=w AND kind='assignment.reconcile' AND state IN('dead_letter','blocked','retry_wait') ORDER BY created_at,id LIMIT 50) j),'[]'::jsonb),
 'oldest_pending_at',(SELECT min(e.occurred_at) FROM workspace_events e WHERE e.workspace_id=w AND e.seq>(SELECT min(cp.last_seq) FROM consumer_checkpoints cp WHERE cp.workspace_id=w)))
 INTO result FROM workspace_stream_heads h JOIN event_retention_state r ON r.workspace_id=h.workspace_id WHERE h.workspace_id=w;
 IF result IS NULL THEN RAISE EXCEPTION 'Consumer state unavailable' USING ERRCODE='P0002';END IF;RETURN result;
END $$;
-- Freeze every helper's path. No ambient schema or temporary object can redirect it.
DO $$ DECLARE f regprocedure;grantee_name text;tab text; BEGIN
 FOR f IN SELECT p.oid::regprocedure FROM pg_proc p WHERE p.pronamespace=current_schema()::regnamespace AND p.proname IN('consumer_required','register_assignment_consumer','register_workspace_consumers','worker_lock_maintenance_context','worker_assignee_eligible','worker_clear_assignment','enqueue_assignment_reconciliation','system_effect_authority','maintenance_job_authority','worker_capture_events','worker_retain_events','consumer_operator_audit','operator_consumer_disposition','operator_consumer_set_enabled','operator_resume_assignment_cleanup','operator_consumer_status','operator_lock_consumer_context') LOOP
  EXECUTE format('ALTER FUNCTION %s SET search_path TO pg_catalog,%I,pg_temp',f,current_schema());EXECUTE format('REVOKE ALL ON FUNCTION %s FROM PUBLIC',f);
 END LOOP;
 FOR grantee_name IN SELECT DISTINCT r.rolname FROM pg_class c CROSS JOIN LATERAL aclexplode(c.relacl) a JOIN pg_roles r ON r.oid=a.grantee WHERE c.oid='consumer_receipts'::regclass AND a.grantee<>c.relowner LOOP
  FOREACH tab IN ARRAY ARRAY['assignment_reconciliations','consumer_registrations','consumer_checkpoints','consumer_receipts','consumer_dead_letters','consumer_alerts','event_retention_state','consumer_operations'] LOOP EXECUTE format('REVOKE ALL ON %I FROM %I',tab,grantee_name);END LOOP;
 END LOOP;
END $$;
