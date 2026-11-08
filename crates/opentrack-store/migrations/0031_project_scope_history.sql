-- Preserve actual scope changes from this upgrade onward. Existing membership
-- is not evidence of when, why, or by whom older plans were changed.
-- Freeze legacy writers while installing the head/backfill and required facts.
LOCK TABLE projects,project_issues,issues IN SHARE ROW EXCLUSIVE MODE;
CREATE TABLE project_scope_heads (
 workspace_id uuid NOT NULL, team_id uuid NOT NULL, project_id uuid NOT NULL,
 revision bigint NOT NULL DEFAULT 0 CHECK(revision>=0),
 history_started_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 legacy_history_unknown boolean NOT NULL DEFAULT false,
 PRIMARY KEY(workspace_id,project_id), UNIQUE(workspace_id,team_id,project_id),
 FOREIGN KEY(workspace_id,team_id,project_id) REFERENCES projects(workspace_id,team_id,id)
);
DO $$ DECLARE w uuid; previous_workspace text:=current_setting('opentrack.workspace_id',true); BEGIN
 FOR w IN SELECT id FROM workspaces ORDER BY id LOOP
  PERFORM set_config('opentrack.workspace_id',w::text,true);
  INSERT INTO project_scope_heads(workspace_id,team_id,project_id,legacy_history_unknown)
   SELECT workspace_id,team_id,id,true FROM projects WHERE workspace_id=w;
 END LOOP;
 PERFORM set_config('opentrack.workspace_id',coalesce(previous_workspace,''),true);
END $$;
CREATE TABLE project_scope_events (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(), workspace_id uuid NOT NULL,team_id uuid NOT NULL,project_id uuid NOT NULL,
 ordinal bigint NOT NULL CHECK(ordinal>0),issue_id uuid NOT NULL,
 action text NOT NULL CHECK(action IN('added','removed','postponed')),
 reason text NOT NULL DEFAULT '' CHECK(length(reason)<=1000), actor_id uuid REFERENCES users(id),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 UNIQUE(workspace_id,project_id,ordinal),
 FOREIGN KEY(workspace_id,team_id,project_id) REFERENCES project_scope_heads(workspace_id,team_id,project_id)
);
CREATE INDEX project_scope_events_issue ON project_scope_events(workspace_id,project_id,issue_id,ordinal DESC);
CREATE INDEX project_scope_events_reverse ON project_scope_events(workspace_id,issue_id,project_id);
CREATE TABLE project_scope_baselines (
 id uuid PRIMARY KEY,workspace_id uuid NOT NULL,team_id uuid NOT NULL,project_id uuid NOT NULL,
 ordinal bigint NOT NULL CHECK(ordinal>0), label text NOT NULL CHECK(length(label) BETWEEN 1 AND 160 AND label=btrim(label)),
 reason text NOT NULL CHECK(length(reason) BETWEEN 1 AND 1000 AND reason=btrim(reason)),author_id uuid REFERENCES users(id),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 UNIQUE(workspace_id,id), UNIQUE(workspace_id,project_id,ordinal),
 FOREIGN KEY(workspace_id,team_id,project_id) REFERENCES project_scope_heads(workspace_id,team_id,project_id)
);
CREATE TABLE project_scope_baseline_items (
 workspace_id uuid NOT NULL,baseline_id uuid NOT NULL,issue_id uuid NOT NULL,
 PRIMARY KEY(workspace_id,baseline_id,issue_id),
 FOREIGN KEY(workspace_id,baseline_id) REFERENCES project_scope_baselines(workspace_id,id)
);
CREATE INDEX project_scope_baseline_items_reverse ON project_scope_baseline_items(workspace_id,issue_id,baseline_id);
-- No FK to issues: removal/purge must not erase the historical fact. Read
-- projections must join current non-deleted issues and recheck current access.
DO $$ DECLARE t text; BEGIN
 FOREACH t IN ARRAY ARRAY['project_scope_heads','project_scope_events','project_scope_baselines','project_scope_baseline_items'] LOOP
  EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',t);
  EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',t);
  EXECUTE format('CREATE POLICY tenant_scope ON %I USING(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid) WITH CHECK(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid)',t);
 END LOOP;
 FOREACH t IN ARRAY ARRAY['project_scope_events','project_scope_baselines','project_scope_baseline_items'] LOOP
  EXECUTE format('CREATE TRIGGER scope_immutable BEFORE UPDATE OR DELETE ON %I FOR EACH ROW EXECUTE FUNCTION audit_append_only()',t);
  EXECUTE format('CREATE TRIGGER scope_no_truncate BEFORE TRUNCATE ON %I FOR EACH STATEMENT EXECUTE FUNCTION audit_append_only()',t);
 END LOOP;
END $$;
CREATE FUNCTION project_scope_initialize() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE affected bigint; BEGIN
 INSERT INTO project_scope_heads(workspace_id,team_id,project_id) VALUES(NEW.workspace_id,NEW.team_id,NEW.id);
 GET DIAGNOSTICS affected=ROW_COUNT;IF affected<>1 THEN RAISE EXCEPTION 'Required scope head missing';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER project_scope_initialize AFTER INSERT ON projects FOR EACH ROW EXECUTE FUNCTION project_scope_initialize();
CREATE FUNCTION project_scope_record(w uuid,t uuid,p uuid,i uuid,a text,r text) RETURNS void LANGUAGE plpgsql AS $$
DECLARE n bigint; affected bigint; BEGIN
 UPDATE project_scope_heads SET revision=revision+1 WHERE workspace_id=w AND team_id=t AND project_id=p RETURNING revision INTO STRICT n;
 INSERT INTO project_scope_events(workspace_id,team_id,project_id,ordinal,issue_id,action,reason,actor_id)
 VALUES(w,t,p,n,i,a,r,nullif(current_setting('opentrack.user_id',true),'')::uuid);
 GET DIAGNOSTICS affected=ROW_COUNT; IF affected<>1 THEN RAISE EXCEPTION 'Required scope history missing'; END IF;
END $$;
CREATE FUNCTION project_scope_membership() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF TG_OP='INSERT' THEN
  PERFORM project_scope_record(NEW.workspace_id,NEW.team_id,NEW.project_id,NEW.issue_id,'added',coalesce(current_setting('opentrack.scope_reason',true),''));
  RETURN NEW;
 ELSE
  PERFORM project_scope_record(OLD.workspace_id,OLD.team_id,OLD.project_id,OLD.issue_id,
   CASE WHEN current_setting('opentrack.scope_action',true)='postponed' THEN 'postponed' ELSE 'removed' END,
   coalesce(current_setting('opentrack.scope_reason',true),''));
  RETURN OLD;
 END IF;
END $$;
CREATE TRIGGER project_scope_membership AFTER INSERT OR DELETE ON project_issues FOR EACH ROW EXECUTE FUNCTION project_scope_membership();
-- Soft deletion preserves membership. It invalidates an outstanding review
-- without inventing a removal event. Restoring the issue makes its old facts
-- visible again after current authorization is rechecked.
CREATE FUNCTION project_scope_issue_visibility() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE target record; affected bigint; BEGIN
 IF (OLD.version,OLD.team_id,OLD.deleted_at) IS DISTINCT FROM (NEW.version,NEW.team_id,NEW.deleted_at) THEN
  FOR target IN SELECT project_id FROM (
   SELECT pi.project_id FROM project_issues pi WHERE pi.workspace_id=NEW.workspace_id AND pi.issue_id=NEW.id
   UNION
   SELECT e.project_id FROM project_scope_events e WHERE e.workspace_id=NEW.workspace_id AND e.issue_id=NEW.id
   UNION
   SELECT b.project_id FROM project_scope_baseline_items bi JOIN project_scope_baselines b ON b.workspace_id=bi.workspace_id AND b.id=bi.baseline_id
    WHERE bi.workspace_id=NEW.workspace_id AND bi.issue_id=NEW.id
  ) affected ORDER BY project_id LOOP
    UPDATE project_scope_heads SET revision=revision+1 WHERE workspace_id=NEW.workspace_id AND project_id=target.project_id;
    GET DIAGNOSTICS affected=ROW_COUNT;IF affected<>1 THEN RAISE EXCEPTION 'Required scope invalidation missing';END IF;
  END LOOP;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER project_scope_issue_visibility AFTER UPDATE OF version,team_id,deleted_at ON issues FOR EACH ROW EXECUTE FUNCTION project_scope_issue_visibility();
DO $$ DECLARE f text; BEGIN
 FOREACH f IN ARRAY ARRAY['project_scope_initialize()','project_scope_record(uuid,uuid,uuid,uuid,text,text)','project_scope_membership()','project_scope_issue_visibility()'] LOOP
  EXECUTE format('ALTER FUNCTION %s SET search_path=pg_catalog,%I,pg_temp',f,current_schema());
 END LOOP;
END $$;

-- Trigger-only writers retain the real row's tenant and do not expose a
-- callable history-forging interface to runtime/worker database roles.
ALTER FUNCTION project_scope_initialize() SECURITY DEFINER;
ALTER FUNCTION project_scope_membership() SECURITY DEFINER;
ALTER FUNCTION project_scope_issue_visibility() SECURITY DEFINER;
REVOKE ALL ON FUNCTION project_scope_record(uuid,uuid,uuid,uuid,text,text) FROM PUBLIC;

CREATE FUNCTION project_scope_membership_identity() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF (NEW.workspace_id,NEW.team_id,NEW.project_id,NEW.issue_id,NEW.created_at) IS DISTINCT FROM (OLD.workspace_id,OLD.team_id,OLD.project_id,OLD.issue_id,OLD.created_at) THEN
  RAISE EXCEPTION 'Project membership identity is immutable; detach and add explicitly' USING ERRCODE='23514';
 END IF; RETURN NEW;
END $$;
CREATE TRIGGER project_scope_membership_identity BEFORE UPDATE ON project_issues FOR EACH ROW EXECUTE FUNCTION project_scope_membership_identity();
DO $$ BEGIN EXECUTE format('ALTER FUNCTION project_scope_membership_identity() SET search_path=pg_catalog,%I,pg_temp',current_schema()); END $$;

CREATE FUNCTION project_scope_head_identity() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF (NEW.workspace_id,NEW.team_id,NEW.project_id,NEW.history_started_at,NEW.legacy_history_unknown) IS DISTINCT FROM (OLD.workspace_id,OLD.team_id,OLD.project_id,OLD.history_started_at,OLD.legacy_history_unknown) OR NEW.revision<>OLD.revision+1 THEN
  RAISE EXCEPTION 'Scope head identity is immutable and revision must advance once' USING ERRCODE='23514';
 END IF;RETURN NEW;
END $$;
CREATE TRIGGER project_scope_head_identity BEFORE UPDATE ON project_scope_heads FOR EACH ROW EXECUTE FUNCTION project_scope_head_identity();
CREATE TRIGGER project_scope_head_no_delete BEFORE DELETE ON project_scope_heads FOR EACH ROW EXECUTE FUNCTION audit_append_only();
CREATE TRIGGER project_scope_head_no_truncate BEFORE TRUNCATE ON project_scope_heads FOR EACH STATEMENT EXECUTE FUNCTION audit_append_only();
DO $$ DECLARE r record; BEGIN
 EXECUTE format('ALTER FUNCTION project_scope_head_identity() SET search_path=pg_catalog,%I,pg_temp',current_schema());
 FOR r IN SELECT DISTINCT grantee FROM information_schema.role_table_grants WHERE table_schema=current_schema() AND table_name='project_scope_heads' AND grantee<>current_user LOOP
  EXECUTE format('REVOKE INSERT,UPDATE,DELETE,TRUNCATE ON project_scope_heads FROM %I',r.grantee);
  EXECUTE format('GRANT UPDATE(revision) ON project_scope_heads TO %I',r.grantee);
  EXECUTE format('REVOKE INSERT,UPDATE,DELETE,TRUNCATE ON project_scope_events FROM %I',r.grantee);
 END LOOP;
END $$;
