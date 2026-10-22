CREATE TABLE milestones (
 id uuid PRIMARY KEY,workspace_id uuid NOT NULL,team_id uuid NOT NULL,project_id uuid NOT NULL,
 title text NOT NULL CHECK(length(title) BETWEEN 1 AND 160 AND title=btrim(title) AND title !~ '[[:cntrl:]]'),
 target_date date,rank bigint NOT NULL CHECK(rank>0),
 state text NOT NULL DEFAULT 'planned' CHECK(state IN('planned','completed')),
 completed_at timestamptz,version bigint NOT NULL DEFAULT 1 CHECK(version>0),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 CHECK((state='completed')=(completed_at IS NOT NULL)),
 UNIQUE(workspace_id,team_id,project_id,id),
 UNIQUE(workspace_id,project_id,rank) DEFERRABLE INITIALLY DEFERRED,
 FOREIGN KEY(workspace_id,team_id,project_id) REFERENCES projects(workspace_id,team_id,id)
);
CREATE INDEX milestones_project ON milestones(workspace_id,project_id,rank,id);
ALTER TABLE project_issues ADD COLUMN milestone_id uuid;
ALTER TABLE project_issues ADD CONSTRAINT project_issue_milestone_scope
 FOREIGN KEY(workspace_id,team_id,project_id,milestone_id)
 REFERENCES milestones(workspace_id,team_id,project_id,id) ON DELETE SET NULL(milestone_id);
CREATE INDEX project_issue_milestone ON project_issues(workspace_id,project_id,milestone_id,issue_id);
ALTER TABLE milestones ENABLE ROW LEVEL SECURITY;
ALTER TABLE milestones FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_scope ON milestones
 USING(workspace_id=nullif(current_setting('opentrack.workspace_id',true),'')::uuid)
 WITH CHECK(workspace_id=nullif(current_setting('opentrack.workspace_id',true),'')::uuid);
CREATE FUNCTION milestone_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF (NEW.id,NEW.workspace_id,NEW.team_id,NEW.project_id,NEW.created_at) IS DISTINCT FROM (OLD.id,OLD.workspace_id,OLD.team_id,OLD.project_id,OLD.created_at) OR NEW.version<>OLD.version+1 THEN
  RAISE EXCEPTION 'Milestone identity is immutable and version must advance once' USING ERRCODE='23514';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER milestone_identity BEFORE UPDATE ON milestones FOR EACH ROW EXECUTE FUNCTION milestone_guard();
CREATE TRIGGER milestones_no_truncate BEFORE TRUNCATE ON milestones FOR EACH STATEMENT EXECUTE FUNCTION audit_append_only();
DO $$ BEGIN
 EXECUTE format('ALTER FUNCTION milestone_guard() SET search_path=pg_catalog,%I,pg_temp',current_schema());
 REVOKE ALL ON FUNCTION milestone_guard() FROM PUBLIC;
END $$;
