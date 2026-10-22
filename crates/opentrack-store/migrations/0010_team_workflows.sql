-- Workflow metadata and typed progress; durable lease/state authority stays in jobs.
ALTER TABLE teams ADD COLUMN workflow_version bigint NOT NULL DEFAULT 1 CHECK(workflow_version>0);
ALTER TABLE workflow_statuses
 ADD COLUMN retiring_at timestamptz,
 ADD COLUMN replacement_status_id uuid,
 ADD COLUMN replacement_job_id uuid,
 ADD COLUMN archived_at timestamptz,
 ADD CONSTRAINT status_name_bounded CHECK(length(name) BETWEEN 1 AND 100 AND name=btrim(name) AND name !~ '[[:cntrl:]]'),
 ADD CONSTRAINT status_color_allowed CHECK(color IN ('gray','slate','red','orange','amber','green','teal','blue','indigo','purple','pink')),
 ADD CONSTRAINT status_position_nonnegative CHECK(position>=0),
 ADD CONSTRAINT status_retirement_shape CHECK((retiring_at IS NULL)=(replacement_status_id IS NULL) AND (retiring_at IS NULL)=(replacement_job_id IS NULL) AND replacement_status_id IS DISTINCT FROM id),
 ADD CONSTRAINT status_default_active CHECK(NOT is_default OR (retiring_at IS NULL AND archived_at IS NULL)),
 ADD CONSTRAINT status_replacement_scope FOREIGN KEY(workspace_id,team_id,replacement_status_id) REFERENCES workflow_statuses(workspace_id,team_id,id),
 ADD CONSTRAINT status_job_scope FOREIGN KEY(workspace_id,replacement_job_id) REFERENCES jobs(workspace_id,id);
ALTER TABLE workflow_statuses DROP CONSTRAINT workflow_statuses_workspace_id_team_id_name_key;
CREATE UNIQUE INDEX status_live_name ON workflow_statuses(workspace_id,team_id,lower(name)) WHERE archived_at IS NULL;
DROP INDEX status_one_default;
CREATE UNIQUE INDEX status_one_default ON workflow_statuses(workspace_id,team_id) WHERE is_default AND archived_at IS NULL AND retiring_at IS NULL;
CREATE INDEX statuses_directory ON workflow_statuses(workspace_id,team_id,id);
CREATE INDEX issues_all_status_references ON issues(workspace_id,team_id,status_id,id);
CREATE TABLE workflow_replacements (
 workspace_id uuid NOT NULL, id uuid NOT NULL, team_id uuid NOT NULL,
 source_status_id uuid NOT NULL, replacement_status_id uuid NOT NULL,
 matched_count bigint NOT NULL CHECK(matched_count>=0),
 processed_count bigint NOT NULL DEFAULT 0 CHECK(processed_count>=0),
 remaining_count bigint NOT NULL CHECK(remaining_count>=0),
 PRIMARY KEY(workspace_id,id),
 FOREIGN KEY(workspace_id,id) REFERENCES jobs(workspace_id,id),
 FOREIGN KEY(workspace_id,team_id) REFERENCES teams(workspace_id,id),
 FOREIGN KEY(workspace_id,team_id,source_status_id) REFERENCES workflow_statuses(workspace_id,team_id,id),
 FOREIGN KEY(workspace_id,team_id,replacement_status_id) REFERENCES workflow_statuses(workspace_id,team_id,id),
 CHECK(source_status_id<>replacement_status_id)
);
CREATE INDEX workflow_replacement_targets ON workflow_replacements(workspace_id,team_id,replacement_status_id,id);
ALTER TABLE workflow_replacements ENABLE ROW LEVEL SECURITY;
ALTER TABLE workflow_replacements FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_scope ON workflow_replacements
 USING(workspace_id=nullif(current_setting('opentrack.workspace_id',true),'')::uuid)
 WITH CHECK(workspace_id=nullif(current_setting('opentrack.workspace_id',true),'')::uuid);
DO $migration$
DECLARE app_schema text := current_schema();
BEGIN
 EXECUTE format($definition$
 CREATE FUNCTION %1$I.workflow_status_guard() RETURNS trigger LANGUAGE plpgsql
 SET search_path=pg_catalog,%1$I,pg_temp AS $body$
 BEGIN
  IF TG_OP='UPDATE' AND (NEW.id,NEW.workspace_id,NEW.team_id,NEW.category) IS DISTINCT FROM (OLD.id,OLD.workspace_id,OLD.team_id,OLD.category) THEN
   RAISE EXCEPTION 'Status identity, scope and category are immutable' USING ERRCODE='23514';
  END IF;
  PERFORM 1 FROM %1$I.teams WHERE workspace_id=COALESCE(NEW.workspace_id,OLD.workspace_id) AND id=COALESCE(NEW.team_id,OLD.team_id) FOR UPDATE;
  RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
 END $body$;
 $definition$,app_schema);
 EXECUTE format($definition$
 CREATE FUNCTION %1$I.workflow_retains_default() RETURNS trigger LANGUAGE plpgsql
 SET search_path=pg_catalog,%1$I,pg_temp AS $body$
 DECLARE w uuid; t uuid; prior_workspace text:=current_setting('opentrack.workspace_id',true); defaults bigint; live bigint;
 BEGIN
  w:=COALESCE(NEW.workspace_id,OLD.workspace_id);
  IF TG_TABLE_NAME='teams' THEN t:=NEW.id; ELSE t:=COALESCE(NEW.team_id,OLD.team_id); END IF;
  PERFORM set_config('opentrack.workspace_id',w::text,true);
  PERFORM 1 FROM %1$I.teams WHERE workspace_id=w AND id=t FOR UPDATE;
  IF FOUND THEN
   SELECT count(*) FILTER(WHERE is_default AND retiring_at IS NULL),count(*) INTO defaults,live
    FROM %1$I.workflow_statuses WHERE workspace_id=w AND team_id=t AND archived_at IS NULL;
   IF defaults<>1 OR live>100 THEN
    RAISE EXCEPTION 'A team requires one active default and at most 100 live statuses' USING ERRCODE='23514';
   END IF;
  END IF;
  PERFORM set_config('opentrack.workspace_id',coalesce(prior_workspace,''),true);
  RETURN NULL;
 END $body$;
 $definition$,app_schema);
 EXECUTE format($definition$
 CREATE FUNCTION %1$I.issue_status_available() RETURNS trigger LANGUAGE plpgsql
 SET search_path=pg_catalog,%1$I,pg_temp AS $body$
 BEGIN
  IF TG_OP='INSERT' OR (NEW.workspace_id,NEW.team_id,NEW.status_id) IS DISTINCT FROM (OLD.workspace_id,OLD.team_id,OLD.status_id) THEN
   PERFORM 1 FROM %1$I.teams WHERE workspace_id=NEW.workspace_id AND id=NEW.team_id FOR UPDATE;
   IF NOT EXISTS(SELECT 1 FROM %1$I.workflow_statuses s WHERE s.workspace_id=NEW.workspace_id AND s.team_id=NEW.team_id AND s.id=NEW.status_id AND s.retiring_at IS NULL AND s.archived_at IS NULL) THEN
    RAISE EXCEPTION 'The issue status is unavailable' USING ERRCODE='23514';
   END IF;
  END IF;
  RETURN NEW;
 END $body$;
 $definition$,app_schema);
END $migration$;
CREATE TRIGGER workflow_status_lock BEFORE INSERT OR UPDATE OR DELETE ON workflow_statuses FOR EACH ROW EXECUTE FUNCTION workflow_status_guard();
CREATE CONSTRAINT TRIGGER workflow_default_required AFTER INSERT OR UPDATE OR DELETE ON workflow_statuses DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION workflow_retains_default();
CREATE CONSTRAINT TRIGGER new_team_default_required AFTER INSERT ON teams DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION workflow_retains_default();
CREATE TRIGGER issue_active_status BEFORE INSERT OR UPDATE OF status_id,team_id,workspace_id ON issues FOR EACH ROW EXECUTE FUNCTION issue_status_available();
REVOKE ALL ON FUNCTION workflow_status_guard() FROM PUBLIC;
REVOKE ALL ON FUNCTION workflow_retains_default() FROM PUBLIC;
REVOKE ALL ON FUNCTION issue_status_available() FROM PUBLIC;
-- Existing installations must already have their seeded default; fail rather than
-- invent a replacement or rename case-colliding custom rows during migration.
ALTER TABLE teams NO FORCE ROW LEVEL SECURITY;
ALTER TABLE workflow_statuses NO FORCE ROW LEVEL SECURITY;
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM teams t WHERE (SELECT count(*) FROM workflow_statuses s WHERE s.workspace_id=t.workspace_id AND s.team_id=t.id AND s.is_default AND s.retiring_at IS NULL AND s.archived_at IS NULL)<>1) THEN
  RAISE EXCEPTION 'An existing team has no unique active default';
 END IF;
END $$;
ALTER TABLE teams FORCE ROW LEVEL SECURITY;
ALTER TABLE workflow_statuses FORCE ROW LEVEL SECURITY;
