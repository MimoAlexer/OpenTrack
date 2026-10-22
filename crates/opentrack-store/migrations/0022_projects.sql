-- Projects are single-team aggregates. Rich narratives use the existing safe
-- document schema subset, versioned with project metadata for this increment.
CREATE TABLE projects (
 id uuid PRIMARY KEY,workspace_id uuid NOT NULL,team_id uuid NOT NULL,
 name text NOT NULL CHECK(length(name) BETWEEN 1 AND 160 AND name=btrim(name) AND name !~ '[[:cntrl:]]'),
 summary text NOT NULL DEFAULT '' CHECK(length(summary)<=500 AND summary=btrim(summary) AND summary !~ '[[:cntrl:]]'),
 narrative_schema_version integer NOT NULL CHECK(narrative_schema_version>0),narrative_body jsonb NOT NULL,narrative_plain_text text NOT NULL,
 lead_id uuid,start_date date,target_date date,
 state text NOT NULL DEFAULT 'planned' CHECK(state IN('planned','active','paused','completed','canceled')),
 health text NOT NULL DEFAULT 'on_track' CHECK(health IN('on_track','at_risk','off_track')),
 version bigint NOT NULL DEFAULT 1 CHECK(version>0),archived_at timestamptz,
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 CHECK(start_date IS NULL OR target_date IS NULL OR start_date<=target_date),
 UNIQUE(workspace_id,id),UNIQUE(workspace_id,team_id,id),
 FOREIGN KEY(workspace_id,team_id) REFERENCES teams(workspace_id,id),
 FOREIGN KEY(workspace_id,lead_id) REFERENCES workspace_members(workspace_id,user_id)
);
CREATE INDEX projects_directory ON projects(workspace_id,team_id,lower(name),id);
CREATE TABLE project_issues (
 workspace_id uuid NOT NULL,team_id uuid NOT NULL,project_id uuid NOT NULL,issue_id uuid NOT NULL,
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(workspace_id,issue_id),
 FOREIGN KEY(workspace_id,team_id,project_id) REFERENCES projects(workspace_id,team_id,id),
 FOREIGN KEY(workspace_id,team_id,issue_id) REFERENCES issues(workspace_id,team_id,id) ON DELETE CASCADE
);
CREATE INDEX project_issues_scope ON project_issues(workspace_id,project_id,issue_id);
CREATE TABLE project_updates (
 id uuid PRIMARY KEY,workspace_id uuid NOT NULL,team_id uuid NOT NULL,project_id uuid NOT NULL,
 author_id uuid NOT NULL,body text NOT NULL CHECK(length(body) BETWEEN 1 AND 8000 AND body=btrim(body)),
 state text NOT NULL CHECK(state IN('planned','active','paused','completed','canceled')),
 health text NOT NULL CHECK(health IN('on_track','at_risk','off_track')),
 completed bigint NOT NULL CHECK(completed>=0),eligible bigint NOT NULL CHECK(eligible>=completed),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),UNIQUE(workspace_id,id),
 FOREIGN KEY(workspace_id,team_id,project_id) REFERENCES projects(workspace_id,team_id,id),
 FOREIGN KEY(workspace_id,author_id) REFERENCES workspace_members(workspace_id,user_id)
);
CREATE INDEX project_updates_feed ON project_updates(workspace_id,project_id,created_at DESC,id DESC);
DO $$ DECLARE t text;BEGIN
 FOREACH t IN ARRAY ARRAY['projects','project_issues','project_updates'] LOOP
  EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',t);
  EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',t);
  EXECUTE format('CREATE POLICY tenant_scope ON %I USING(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid) WITH CHECK(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid)',t);
 END LOOP;
END $$;
CREATE FUNCTION project_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF (NEW.id,NEW.workspace_id,NEW.team_id,NEW.created_at) IS DISTINCT FROM(OLD.id,OLD.workspace_id,OLD.team_id,OLD.created_at) OR NEW.version<>OLD.version+1 THEN
  RAISE EXCEPTION 'Project identity is immutable and version must advance once' USING ERRCODE='23514';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER project_identity BEFORE UPDATE ON projects FOR EACH ROW EXECUTE FUNCTION project_guard();
CREATE TRIGGER projects_no_delete BEFORE DELETE ON projects FOR EACH ROW EXECUTE FUNCTION audit_append_only();
CREATE TRIGGER projects_no_truncate BEFORE TRUNCATE ON projects FOR EACH STATEMENT EXECUTE FUNCTION audit_append_only();
CREATE TRIGGER project_updates_immutable BEFORE UPDATE OR DELETE ON project_updates FOR EACH ROW EXECUTE FUNCTION audit_append_only();
CREATE TRIGGER project_updates_no_truncate BEFORE TRUNCATE ON project_updates FOR EACH STATEMENT EXECUTE FUNCTION audit_append_only();
DO $$ DECLARE r record;BEGIN
 EXECUTE format('ALTER FUNCTION project_guard() SET search_path=pg_catalog,%I,pg_temp',current_schema());
 REVOKE ALL ON FUNCTION project_guard() FROM PUBLIC;
 FOR r IN SELECT DISTINCT grantee FROM information_schema.role_table_grants WHERE table_schema=current_schema() AND table_name='projects' AND grantee<>current_user LOOP
  EXECUTE format('REVOKE DELETE,TRUNCATE ON projects FROM %I',r.grantee);
  EXECUTE format('REVOKE UPDATE,DELETE,TRUNCATE ON project_updates FROM %I',r.grantee);
 END LOOP;
END $$;
