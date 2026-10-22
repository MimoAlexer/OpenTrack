-- Current definitions only: an issue's immutable source version/digest is
-- retained independently; editing does not accumulate unbounded rich snapshots.
CREATE TABLE issue_templates (
 id uuid PRIMARY KEY,workspace_id uuid NOT NULL,team_id uuid NOT NULL,
 name text NOT NULL CHECK(length(name) BETWEEN 1 AND 100 AND name=btrim(name) AND name !~ '[[:cntrl:]]'),
 title text NOT NULL CHECK(length(title)<=256 AND title=btrim(title) AND title !~ '[[:cntrl:]]'),
 default_status_id uuid,default_priority smallint NOT NULL CHECK(default_priority BETWEEN 0 AND 4),
 default_label_ids uuid[] NOT NULL DEFAULT '{}' CHECK(cardinality(default_label_ids)<=50),
 document_schema_version integer NOT NULL CHECK(document_schema_version>0),document_body jsonb NOT NULL,
 document_plain_text text NOT NULL,definition_sha256 text NOT NULL CHECK(definition_sha256~'^[0-9a-f]{64}$'),
 version bigint NOT NULL DEFAULT 1 CHECK(version>0),archived_at timestamptz,
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 UNIQUE(workspace_id,id),UNIQUE(workspace_id,team_id,id),
 FOREIGN KEY(workspace_id,team_id) REFERENCES teams(workspace_id,id),
 FOREIGN KEY(workspace_id,team_id,default_status_id) REFERENCES workflow_statuses(workspace_id,team_id,id)
);
CREATE UNIQUE INDEX issue_template_normalized_name ON issue_templates(workspace_id,team_id,lower(regexp_replace(name,'[[:space:]]+',' ','g')));
CREATE INDEX issue_templates_directory ON issue_templates(workspace_id,team_id,lower(name),id);
CREATE TABLE issue_template_sources (
 workspace_id uuid NOT NULL,issue_id uuid NOT NULL,source_team_id uuid NOT NULL,template_id uuid NOT NULL,
 template_version bigint NOT NULL CHECK(template_version>0),definition_sha256 text NOT NULL CHECK(definition_sha256~'^[0-9a-f]{64}$'),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(workspace_id,issue_id),
 FOREIGN KEY(workspace_id,issue_id) REFERENCES issues(workspace_id,id),
 FOREIGN KEY(workspace_id,source_team_id,template_id) REFERENCES issue_templates(workspace_id,team_id,id)
);
DO $$ DECLARE t text;BEGIN
 FOREACH t IN ARRAY ARRAY['issue_templates','issue_template_sources'] LOOP
  EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',t);
  EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',t);
  EXECUTE format('CREATE POLICY tenant_scope ON %I USING(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid) WITH CHECK(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid)',t);
 END LOOP;
END $$;
CREATE FUNCTION issue_template_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 PERFORM 1 FROM teams WHERE workspace_id=NEW.workspace_id AND id=NEW.team_id FOR UPDATE;
 IF TG_OP='INSERT' THEN
  IF (SELECT count(*) FROM issue_templates WHERE workspace_id=NEW.workspace_id AND team_id=NEW.team_id)>=100 THEN
   RAISE EXCEPTION 'This team already has 100 templates' USING ERRCODE='23514',CONSTRAINT='issue_template_capacity';
  END IF;
 ELSIF (NEW.id,NEW.workspace_id,NEW.team_id,NEW.created_at) IS DISTINCT FROM(OLD.id,OLD.workspace_id,OLD.team_id,OLD.created_at) OR NEW.version<>OLD.version+1 THEN
  RAISE EXCEPTION 'Template identity is immutable and version must advance once' USING ERRCODE='23514';
 END IF;
 IF EXISTS(SELECT 1 FROM unnest(NEW.default_label_ids) label_id LEFT JOIN labels l ON l.workspace_id=NEW.workspace_id AND l.id=label_id WHERE l.id IS NULL OR (l.team_id IS NOT NULL AND l.team_id<>NEW.team_id))
 OR cardinality(NEW.default_label_ids)<>(SELECT count(DISTINCT id) FROM unnest(NEW.default_label_ids) id) THEN
  RAISE EXCEPTION 'Template labels must have compatible scope and distinct IDs' USING ERRCODE='23514';
 END IF;
 RETURN NEW;
END $$;
CREATE FUNCTION issue_template_source_guard() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE current_template issue_templates%ROWTYPE;issue_team uuid;
BEGIN
 PERFORM 1 FROM teams WHERE workspace_id=NEW.workspace_id AND id=NEW.source_team_id FOR UPDATE;
 SELECT team_id INTO issue_team FROM issues WHERE workspace_id=NEW.workspace_id AND id=NEW.issue_id;
 SELECT * INTO current_template FROM issue_templates WHERE workspace_id=NEW.workspace_id AND id=NEW.template_id;
 IF NOT FOUND OR issue_team IS DISTINCT FROM NEW.source_team_id OR current_template.team_id<>NEW.source_team_id OR current_template.version<>NEW.template_version OR current_template.definition_sha256<>NEW.definition_sha256 OR current_template.archived_at IS NOT NULL THEN
  RAISE EXCEPTION 'Template source must match its current creation revision' USING ERRCODE='23514';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER issue_templates_guard BEFORE INSERT OR UPDATE ON issue_templates FOR EACH ROW EXECUTE FUNCTION issue_template_guard();
CREATE TRIGGER issue_templates_no_delete BEFORE DELETE ON issue_templates FOR EACH ROW EXECUTE FUNCTION audit_append_only();
CREATE TRIGGER issue_templates_no_truncate BEFORE TRUNCATE ON issue_templates FOR EACH STATEMENT EXECUTE FUNCTION audit_append_only();
CREATE TRIGGER issue_template_source_valid BEFORE INSERT ON issue_template_sources FOR EACH ROW EXECUTE FUNCTION issue_template_source_guard();
CREATE TRIGGER issue_template_source_immutable BEFORE UPDATE OR DELETE ON issue_template_sources FOR EACH ROW EXECUTE FUNCTION audit_append_only();
CREATE TRIGGER issue_template_source_no_truncate BEFORE TRUNCATE ON issue_template_sources FOR EACH STATEMENT EXECUTE FUNCTION audit_append_only();
DO $$ DECLARE p regprocedure;r record;BEGIN
 FOREACH p IN ARRAY ARRAY['issue_template_guard()'::regprocedure,'issue_template_source_guard()'::regprocedure] LOOP
  EXECUTE format('ALTER FUNCTION %s SET search_path=pg_catalog,%I,pg_temp',p,current_schema());
  EXECUTE format('REVOKE ALL ON FUNCTION %s FROM PUBLIC',p);
 END LOOP;
 FOR r IN SELECT DISTINCT grantee FROM information_schema.role_table_grants WHERE table_schema=current_schema() AND table_name='issue_templates' AND grantee<>current_user LOOP
  EXECUTE format('REVOKE DELETE,TRUNCATE ON issue_templates FROM %I',r.grantee);
  EXECUTE format('REVOKE UPDATE,DELETE,TRUNCATE ON issue_template_sources FROM %I',r.grantee);
 END LOOP;
END $$;
