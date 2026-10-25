-- Offline owner migration; preserve every existing logical issue field.
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM pg_class WHERE oid IN('workspaces'::regclass,'teams'::regclass,'issues'::regclass) AND pg_get_userbyid(relowner)<>current_user) THEN
  RAISE EXCEPTION 'Issue ordering migration requires the table-owning role' USING ERRCODE='42501';
 END IF;
END $$;
LOCK TABLE workspaces,teams,issues IN ACCESS EXCLUSIVE MODE;
ALTER TABLE teams ADD COLUMN ordering_version bigint NOT NULL DEFAULT 1 CHECK(ordering_version>0);
ALTER TABLE issues ADD COLUMN rank numeric(38,18);
ALTER TABLE workspaces NO FORCE ROW LEVEL SECURITY;
DO $$
DECLARE w uuid;expected bigint;affected bigint;previous_workspace text:=current_setting('opentrack.workspace_id',true);
BEGIN
 FOR w IN SELECT id FROM workspaces ORDER BY id LOOP
  PERFORM set_config('opentrack.workspace_id',w::text,true);
  SELECT count(*) INTO expected FROM issues WHERE workspace_id=w;
  WITH ordered AS(
   SELECT id,row_number() OVER(PARTITION BY team_id,status_id ORDER BY created_at DESC,id DESC)::numeric*1024 AS rank
   FROM issues WHERE workspace_id=w
  ) UPDATE issues i SET rank=o.rank FROM ordered o WHERE i.workspace_id=w AND i.id=o.id;
  GET DIAGNOSTICS affected=ROW_COUNT;
  IF affected<>expected OR EXISTS(SELECT 1 FROM issues WHERE workspace_id=w AND rank IS NULL) THEN
   RAISE EXCEPTION 'Issue order backfill did not preserve its complete scope';
  END IF;
 END LOOP;
 PERFORM set_config('opentrack.workspace_id',coalesce(previous_workspace,''),true);
END $$;
ALTER TABLE workspaces FORCE ROW LEVEL SECURITY;
SET CONSTRAINTS ALL IMMEDIATE;
ALTER TABLE issues ALTER COLUMN rank SET NOT NULL,
 ADD CONSTRAINT issue_rank_finite CHECK(rank NOT IN('NaN'::numeric,'Infinity'::numeric,'-Infinity'::numeric));
CREATE INDEX issues_manual_order ON issues(workspace_id,team_id,status_id,rank,id)
 WHERE archived_at IS NULL AND deleted_at IS NULL;

ALTER TABLE jobs DROP CONSTRAINT jobs_kind_check;
ALTER TABLE jobs ADD CONSTRAINT jobs_kind_check
 CHECK(kind IN('workflow.status_replace','assignment.reconcile','issues.order_normalize'));
ALTER TABLE jobs DROP CONSTRAINT jobs_requester_shape;
ALTER TABLE jobs ADD CONSTRAINT jobs_requester_shape
 CHECK((kind IN('workflow.status_replace','issues.order_normalize') AND requested_by IS NOT NULL)
  OR (kind='assignment.reconcile' AND requested_by IS NULL));
CREATE TABLE issue_order_normalizations (
 workspace_id uuid NOT NULL,id uuid NOT NULL,team_id uuid NOT NULL,status_id uuid NOT NULL,
 original_requested_by uuid NOT NULL REFERENCES users(id),
 phase text NOT NULL DEFAULT 'staging' CHECK(phase IN('staging','applying','completed')),
 staged_count bigint NOT NULL DEFAULT 0 CHECK(staged_count>=0),
 applied_count bigint NOT NULL DEFAULT 0 CHECK(applied_count>=0 AND applied_count<=staged_count),
 total_count bigint CHECK(total_count>=0),after_rank numeric(38,18),after_issue_id uuid,
 PRIMARY KEY(workspace_id,id),UNIQUE(workspace_id,team_id,status_id,id),
 FOREIGN KEY(workspace_id,id) REFERENCES jobs(workspace_id,id),
 FOREIGN KEY(workspace_id,team_id,status_id) REFERENCES workflow_statuses(workspace_id,team_id,id),
 CHECK((after_rank IS NULL)=(after_issue_id IS NULL)),
 CHECK(after_rank IS NULL OR after_rank NOT IN('NaN'::numeric,'Infinity'::numeric,'-Infinity'::numeric)),
 CHECK((phase='staging')=(total_count IS NULL)),
 CHECK(total_count IS NULL OR total_count=staged_count),
 CHECK(phase<>'completed' OR applied_count=total_count)
);
CREATE TABLE issue_order_guards (
 workspace_id uuid NOT NULL,team_id uuid NOT NULL,status_id uuid NOT NULL,job_id uuid NOT NULL,
 PRIMARY KEY(workspace_id,team_id,status_id),
 FOREIGN KEY(workspace_id,team_id,status_id,job_id)
  REFERENCES issue_order_normalizations(workspace_id,team_id,status_id,id)
);
CREATE TABLE issue_order_staging (
 workspace_id uuid NOT NULL,job_id uuid NOT NULL,ordinal bigint NOT NULL CHECK(ordinal>0),
 issue_id uuid NOT NULL,original_rank numeric(38,18) NOT NULL,
 PRIMARY KEY(workspace_id,job_id,ordinal),UNIQUE(workspace_id,job_id,issue_id),
 FOREIGN KEY(workspace_id,job_id) REFERENCES issue_order_normalizations(workspace_id,id),
 FOREIGN KEY(workspace_id,issue_id) REFERENCES issues(workspace_id,id),
 CHECK(original_rank NOT IN('NaN'::numeric,'Infinity'::numeric,'-Infinity'::numeric))
);
DO $$ DECLARE tab text;r text;BEGIN
 FOREACH tab IN ARRAY ARRAY['issue_order_normalizations','issue_order_guards','issue_order_staging'] LOOP
  EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',tab);
  EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',tab);
  EXECUTE format('CREATE POLICY tenant_scope ON %I USING(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid) WITH CHECK(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid)',tab);
 END LOOP;
 FOR r IN SELECT DISTINCT grantee FROM information_schema.role_table_grants
  WHERE table_schema=current_schema() AND table_name='issue_order_staging' AND grantee<>current_user LOOP
  EXECUTE format('REVOKE ALL ON issue_order_staging FROM %I',r);
  EXECUTE format('REVOKE UPDATE,DELETE,TRUNCATE ON issue_order_normalizations,issue_order_guards FROM %I',r);
 END LOOP;
END $$;

CREATE FUNCTION issue_order_scope_guard() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE w uuid;teams uuid[];statuses uuid[];n integer;
BEGIN
 IF TG_OP='UPDATE' AND (NEW.team_id,NEW.status_id,NEW.rank,NEW.archived_at,NEW.deleted_at)
  IS NOT DISTINCT FROM (OLD.team_id,OLD.status_id,OLD.rank,OLD.archived_at,OLD.deleted_at) THEN RETURN NEW;END IF;
 -- Physical maintenance is reachable only through owner-controlled capabilities.
 IF current_user=pg_get_userbyid((SELECT relowner FROM pg_class WHERE oid=TG_RELID)) THEN
  RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
 END IF;
 IF TG_OP='INSERT' THEN w:=NEW.workspace_id;teams:=ARRAY[NEW.team_id];statuses:=ARRAY[NEW.status_id];
 ELSIF TG_OP='DELETE' THEN w:=OLD.workspace_id;teams:=ARRAY[OLD.team_id];statuses:=ARRAY[OLD.status_id];
 ELSE w:=NEW.workspace_id;teams:=ARRAY[OLD.team_id,NEW.team_id];statuses:=ARRAY[OLD.status_id,NEW.status_id];END IF;
 FOR n IN 1..cardinality(teams) LOOP
  IF EXISTS(SELECT 1 FROM issue_order_guards WHERE workspace_id=w AND team_id=teams[n] AND status_id=statuses[n]) THEN
   RAISE EXCEPTION 'Issue group order maintenance is active' USING ERRCODE='23514',CONSTRAINT='issue_order_maintenance';
  END IF;
 END LOOP;
 RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
END $$;
CREATE TRIGGER issues_order_scope_guard BEFORE INSERT OR UPDATE OR DELETE ON issues
 FOR EACH ROW EXECUTE FUNCTION issue_order_scope_guard();

CREATE FUNCTION issue_order_operation_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 PERFORM 1 FROM teams WHERE workspace_id=NEW.workspace_id AND id=NEW.team_id FOR UPDATE;
 IF NOT EXISTS(SELECT 1 FROM jobs WHERE workspace_id=NEW.workspace_id AND id=NEW.id
  AND kind='issues.order_normalize' AND requested_by=NEW.original_requested_by AND state='queued') THEN
  RAISE EXCEPTION 'Issue order maintenance requires its queued typed job' USING ERRCODE='23514';
 END IF;
 IF EXISTS(SELECT 1 FROM workflow_statuses WHERE workspace_id=NEW.workspace_id AND team_id=NEW.team_id
  AND retiring_at IS NOT NULL AND (id=NEW.status_id OR replacement_status_id=NEW.status_id)) THEN
  RAISE EXCEPTION 'Workflow replacement prevents issue order maintenance' USING ERRCODE='23514',CONSTRAINT='issue_order_workflow';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER issue_order_operation_guard BEFORE INSERT ON issue_order_normalizations
 FOR EACH ROW EXECUTE FUNCTION issue_order_operation_guard();

CREATE FUNCTION workflow_issue_order_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF NEW.retiring_at IS NOT NULL AND (TG_OP='INSERT' OR
  (NEW.retiring_at,NEW.replacement_status_id) IS DISTINCT FROM (OLD.retiring_at,OLD.replacement_status_id)) THEN
  PERFORM 1 FROM teams WHERE workspace_id=NEW.workspace_id AND id=NEW.team_id FOR UPDATE;
  IF EXISTS(SELECT 1 FROM issue_order_guards WHERE workspace_id=NEW.workspace_id AND team_id=NEW.team_id
   AND status_id IN(NEW.id,NEW.replacement_status_id)) THEN
   RAISE EXCEPTION 'Issue order maintenance prevents workflow replacement' USING ERRCODE='23514',CONSTRAINT='issue_order_workflow';
  END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER workflow_issue_order_guard BEFORE INSERT OR UPDATE ON workflow_statuses
 FOR EACH ROW EXECUTE FUNCTION workflow_issue_order_guard();

CREATE FUNCTION issue_order_rebalance_inline(w uuid,t uuid,s uuid) RETURNS boolean
 LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE n bigint;expected bigint;changed bigint;
BEGIN
 IF w IS DISTINCT FROM nullif(current_setting('opentrack.workspace_id',true),'')::uuid
  OR NOT EXISTS(SELECT 1 FROM workspace_members m JOIN users u ON u.id=m.user_id
   JOIN workspaces workspace ON workspace.id=m.workspace_id
   JOIN teams team ON team.workspace_id=m.workspace_id AND team.id=t
   WHERE m.workspace_id=w AND m.user_id=nullif(current_setting('opentrack.user_id',true),'')::uuid
    AND m.active AND m.role<>'viewer' AND u.disabled_at IS NULL
    AND workspace.archived_at IS NULL AND team.archived_at IS NULL
    AND (NOT team.private OR EXISTS(SELECT 1 FROM team_members tm
     WHERE tm.workspace_id=w AND tm.team_id=t AND tm.user_id=m.user_id))) THEN
  RAISE EXCEPTION 'Issue order normalization is unavailable' USING ERRCODE='42501';
 END IF;
 PERFORM 1 FROM teams WHERE workspace_id=w AND id=t FOR UPDATE;
 IF NOT EXISTS(SELECT 1 FROM workflow_statuses WHERE workspace_id=w AND team_id=t AND id=s
  AND archived_at IS NULL AND retiring_at IS NULL)
  OR EXISTS(SELECT 1 FROM issue_order_guards WHERE workspace_id=w AND team_id=t AND status_id=s)
  OR EXISTS(SELECT 1 FROM workflow_statuses WHERE workspace_id=w AND team_id=t AND retiring_at IS NOT NULL
   AND (id=s OR replacement_status_id=s)) THEN
  RAISE EXCEPTION 'Issue group maintenance is unavailable' USING ERRCODE='23514',CONSTRAINT='issue_order_maintenance';
 END IF;
 SELECT count(*) INTO n FROM(SELECT id FROM issues WHERE workspace_id=w AND team_id=t AND status_id=s
  AND archived_at IS NULL AND deleted_at IS NULL ORDER BY rank,id LIMIT 501) probe;
 IF n>500 THEN RETURN false;END IF;
 WITH ordered AS MATERIALIZED(SELECT id,rank,row_number() OVER(ORDER BY rank,id)::numeric*1024 AS next_rank
  FROM issues WHERE workspace_id=w AND team_id=t AND status_id=s AND archived_at IS NULL AND deleted_at IS NULL),
 changed_rows AS(UPDATE issues i SET rank=o.next_rank FROM ordered o
  WHERE i.workspace_id=w AND i.id=o.id AND i.rank IS DISTINCT FROM o.next_rank RETURNING i.id)
 SELECT (SELECT count(*) FROM ordered WHERE rank IS DISTINCT FROM next_rank),count(*) INTO expected,changed FROM changed_rows;
 IF expected<>changed THEN RAISE EXCEPTION 'Required inline order normalization did not apply';END IF;
 RETURN true;
END $$;

CREATE FUNCTION issue_rank_version_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF NEW.rank IS DISTINCT FROM OLD.rank
 AND current_user<>pg_get_userbyid((SELECT relowner FROM pg_class WHERE oid=TG_RELID))
 AND NEW.version<>OLD.version+1 THEN
  RAISE EXCEPTION 'A logical rank change requires its issue version' USING ERRCODE='23514',CONSTRAINT='issue_rank_version';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER issue_rank_version_guard BEFORE UPDATE ON issues FOR EACH ROW EXECUTE FUNCTION issue_rank_version_guard();

CREATE FUNCTION issue_ordering_changed() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE w uuid;scopes uuid[];t uuid;prior bigint;advanced bigint;
BEGIN
 IF TG_RELID<>'issues'::regclass THEN RAISE EXCEPTION 'Unexpected ordering source' USING ERRCODE='42501';END IF;
 IF TG_OP='UPDATE' THEN
  IF NEW.workspace_id<>OLD.workspace_id THEN RAISE EXCEPTION 'Issue workspace is immutable' USING ERRCODE='23514';END IF;
  IF (NEW.team_id,NEW.status_id,NEW.rank,NEW.version,NEW.archived_at,NEW.deleted_at)
   IS NOT DISTINCT FROM (OLD.team_id,OLD.status_id,OLD.rank,OLD.version,OLD.archived_at,OLD.deleted_at) THEN RETURN NEW;END IF;
  w:=NEW.workspace_id;scopes:=ARRAY[OLD.team_id,NEW.team_id];
 ELSIF TG_OP='INSERT' THEN w:=NEW.workspace_id;scopes:=ARRAY[NEW.team_id];
 ELSIF TG_OP='DELETE' THEN w:=OLD.workspace_id;scopes:=ARRAY[OLD.team_id];
 ELSE RAISE EXCEPTION 'Unexpected ordering operation' USING ERRCODE='42501';END IF;
 IF w IS DISTINCT FROM nullif(current_setting('opentrack.workspace_id',true),'')::uuid THEN
  RAISE EXCEPTION 'Ordering generation requires its workspace context' USING ERRCODE='42501';
 END IF;
 FOR t IN SELECT DISTINCT value FROM unnest(scopes) value ORDER BY value LOOP
  SELECT ordering_version INTO prior FROM teams WHERE workspace_id=w AND id=t FOR UPDATE;
  IF NOT FOUND THEN RAISE EXCEPTION 'Required ordering team is unavailable';END IF;
  UPDATE teams SET ordering_version=prior+1 WHERE workspace_id=w AND id=t RETURNING ordering_version INTO advanced;
  IF advanced IS DISTINCT FROM prior+1 THEN RAISE EXCEPTION 'Required ordering generation did not advance';END IF;
 END LOOP;
 RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
END $$;
CREATE TRIGGER issues_ordering_changed AFTER INSERT OR UPDATE OR DELETE ON issues FOR EACH ROW EXECUTE FUNCTION issue_ordering_changed();
DO $$ DECLARE p regprocedure;BEGIN
 FOREACH p IN ARRAY ARRAY['issue_rank_version_guard()'::regprocedure,'issue_ordering_changed()'::regprocedure,
  'issue_order_scope_guard()'::regprocedure,'issue_order_operation_guard()'::regprocedure,'workflow_issue_order_guard()'::regprocedure,
  'issue_order_rebalance_inline(uuid,uuid,uuid)'::regprocedure] LOOP
  EXECUTE format('ALTER FUNCTION %s SET search_path=pg_catalog,%I,pg_temp',p,current_schema());
  EXECUTE format('REVOKE ALL ON FUNCTION %s FROM PUBLIC',p);
 END LOOP;
END $$;
DO $$ DECLARE r text;BEGIN
 FOR r IN SELECT DISTINCT grantee FROM information_schema.role_table_grants
  WHERE table_schema=current_schema() AND table_name='issue_order_normalizations' AND privilege_type='INSERT'
   AND grantee<>current_user LOOP
  EXECUTE format('GRANT EXECUTE ON FUNCTION issue_order_rebalance_inline(uuid,uuid,uuid) TO %I',r);
 END LOOP;
END $$;
