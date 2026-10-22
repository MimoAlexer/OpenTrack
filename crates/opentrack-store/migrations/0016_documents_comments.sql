-- Typed document ownership; issue description/body_version are derived projections,
-- never document-ID backlinks. Historical audiences remain immutable after moves.
ALTER TABLE issues ADD COLUMN body_version bigint NOT NULL DEFAULT 0 CHECK(body_version>=0);
ALTER TABLE audit_entries ADD COLUMN metadata jsonb NOT NULL DEFAULT '{}'::jsonb CHECK(jsonb_typeof(metadata)='object');
CREATE TABLE comments (
 id uuid PRIMARY KEY,workspace_id uuid NOT NULL,issue_id uuid NOT NULL,origin_team_id uuid NOT NULL,
 author_id uuid NOT NULL REFERENCES users(id),version bigint NOT NULL DEFAULT 1 CHECK(version>0),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),edited_at timestamptz,deleted_at timestamptz,
 UNIQUE(workspace_id,id),FOREIGN KEY(workspace_id,issue_id) REFERENCES issues(workspace_id,id),
 FOREIGN KEY(workspace_id,origin_team_id) REFERENCES teams(workspace_id,id)
);
CREATE INDEX comments_issue_page ON comments(workspace_id,issue_id,created_at DESC,id DESC);
CREATE TABLE documents (
 id uuid PRIMARY KEY,workspace_id uuid NOT NULL,issue_id uuid,comment_id uuid,
 schema_version integer NOT NULL CHECK(schema_version>0),body jsonb NOT NULL CHECK(jsonb_typeof(body)='object'),plain_text text NOT NULL,
 version bigint NOT NULL DEFAULT 1 CHECK(version>0),updated_by_user_id uuid NOT NULL REFERENCES users(id),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),deleted_at timestamptz,
 CHECK(num_nonnulls(issue_id,comment_id)=1),UNIQUE(workspace_id,id),
 FOREIGN KEY(workspace_id,issue_id) REFERENCES issues(workspace_id,id),FOREIGN KEY(workspace_id,comment_id) REFERENCES comments(workspace_id,id)
);
CREATE UNIQUE INDEX document_issue_owner ON documents(workspace_id,issue_id) WHERE issue_id IS NOT NULL;
CREATE UNIQUE INDEX document_comment_owner ON documents(workspace_id,comment_id) WHERE comment_id IS NOT NULL;
CREATE TABLE document_revisions (
 workspace_id uuid NOT NULL,document_id uuid NOT NULL,revision bigint NOT NULL CHECK(revision>0),
 origin_team_id uuid NOT NULL,schema_version integer NOT NULL,body jsonb NOT NULL,plain_text text NOT NULL,
 author_id uuid NOT NULL REFERENCES users(id),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(workspace_id,document_id,revision),FOREIGN KEY(workspace_id,document_id) REFERENCES documents(workspace_id,id),
 FOREIGN KEY(workspace_id,origin_team_id) REFERENCES teams(workspace_id,id)
);
CREATE INDEX document_revision_retention ON document_revisions(workspace_id,created_at,document_id,revision);
CREATE TABLE document_retention_operations (
 workspace_id uuid NOT NULL REFERENCES workspaces(id),id uuid NOT NULL,deleted_count bigint NOT NULL CHECK(deleted_count>0),
 through_created_at timestamptz NOT NULL,created_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(workspace_id,id)
);
DO $$ DECLARE tab text; BEGIN
 FOREACH tab IN ARRAY ARRAY['comments','documents','document_revisions','document_retention_operations'] LOOP
  EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',tab);
  EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',tab);
  EXECUTE format('CREATE POLICY tenant_scope ON %I USING(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid) WITH CHECK(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid)',tab);
 END LOOP;
END $$;
CREATE FUNCTION document_identity_guard() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF (NEW.id,NEW.workspace_id,NEW.issue_id,NEW.comment_id,NEW.created_at) IS DISTINCT FROM (OLD.id,OLD.workspace_id,OLD.issue_id,OLD.comment_id,OLD.created_at) THEN
  RAISE EXCEPTION 'Document ownership is immutable' USING ERRCODE='23514'; END IF;
 IF NEW.version<>OLD.version+1 THEN RAISE EXCEPTION 'Document writes require the next version' USING ERRCODE='23514'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER documents_identity BEFORE UPDATE ON documents FOR EACH ROW EXECUTE FUNCTION document_identity_guard();
CREATE FUNCTION comment_identity_guard() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF (NEW.id,NEW.workspace_id,NEW.issue_id,NEW.origin_team_id,NEW.author_id,NEW.created_at) IS DISTINCT FROM (OLD.id,OLD.workspace_id,OLD.issue_id,OLD.origin_team_id,OLD.author_id,OLD.created_at) OR NEW.version<>OLD.version+1 OR OLD.deleted_at IS NOT NULL THEN
  RAISE EXCEPTION 'Comment identity and tombstones are immutable' USING ERRCODE='23514'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER comments_identity BEFORE UPDATE ON comments FOR EACH ROW EXECUTE FUNCTION comment_identity_guard();
CREATE FUNCTION document_effects() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE t uuid;n bigint;
BEGIN
 IF NEW.issue_id IS NOT NULL THEN
  SELECT team_id INTO STRICT t FROM issues WHERE workspace_id=NEW.workspace_id AND id=NEW.issue_id;
 ELSE
  SELECT i.team_id INTO STRICT t FROM comments c JOIN issues i ON i.workspace_id=c.workspace_id AND i.id=c.issue_id WHERE c.workspace_id=NEW.workspace_id AND c.id=NEW.comment_id;
 END IF;
 INSERT INTO document_revisions(workspace_id,document_id,revision,origin_team_id,schema_version,body,plain_text,author_id,created_at)
 VALUES(NEW.workspace_id,NEW.id,NEW.version,t,NEW.schema_version,NEW.body,NEW.plain_text,NEW.updated_by_user_id,NEW.updated_at);
 GET DIAGNOSTICS n=ROW_COUNT;IF n<>1 THEN RAISE EXCEPTION 'Required document revision missing';END IF;
 IF NEW.issue_id IS NOT NULL THEN
  UPDATE issues SET description=NEW.plain_text,body_version=NEW.version WHERE workspace_id=NEW.workspace_id AND id=NEW.issue_id;
  GET DIAGNOSTICS n=ROW_COUNT;IF n<>1 THEN RAISE EXCEPTION 'Required document projection missing';END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER documents_effects AFTER INSERT OR UPDATE ON documents FOR EACH ROW EXECUTE FUNCTION document_effects();
-- Inserts from old import/fixture paths also receive their required body. New App
-- creation supplies validated JSON in a transaction-local value, never SQL text.
CREATE FUNCTION issue_initial_document() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE b jsonb;n bigint;
BEGIN
 b:=nullif(current_setting('opentrack.initial_document',true),'')::jsonb;
 IF b IS NULL THEN b:=jsonb_build_object('type','doc','content',jsonb_build_array(jsonb_build_object('type','paragraph','content',CASE WHEN NEW.description='' THEN '[]'::jsonb ELSE jsonb_build_array(jsonb_build_object('type','text','text',NEW.description)) END)));END IF;
 INSERT INTO documents(id,workspace_id,issue_id,schema_version,body,plain_text,updated_by_user_id)
 VALUES(gen_random_uuid(),NEW.workspace_id,NEW.id,1,b,NEW.description,NEW.created_by_user_id);
 GET DIAGNOSTICS n=ROW_COUNT;IF n<>1 THEN RAISE EXCEPTION 'Required issue document missing';END IF;
 RETURN NEW;
END $$;
-- Lossless grandfathering: valid legacy text can serialize beyond the new 256KiB
-- JSON write limit. Never truncate or reject previously accepted content.
DO $$ DECLARE w uuid;n bigint;expected bigint; BEGIN
 FOR w IN SELECT id FROM workspaces LOOP
  PERFORM set_config('opentrack.workspace_id',w::text,true);
  SELECT count(*) INTO expected FROM issues WHERE workspace_id=w;
  INSERT INTO documents(id,workspace_id,issue_id,schema_version,body,plain_text,updated_by_user_id,created_at,updated_at)
  SELECT gen_random_uuid(),workspace_id,id,1,jsonb_build_object('type','doc','content',jsonb_build_array(jsonb_build_object('type','paragraph','content',CASE WHEN description='' THEN '[]'::jsonb ELSE jsonb_build_array(jsonb_build_object('type','text','text',description)) END))),description,created_by_user_id,created_at,created_at FROM issues WHERE workspace_id=w;
  GET DIAGNOSTICS n=ROW_COUNT;IF n<>expected THEN RAISE EXCEPTION 'Document backfill incomplete';END IF;
 END LOOP;
 PERFORM set_config('opentrack.workspace_id','',true);
END $$;
CREATE TRIGGER issue_initial_document AFTER INSERT ON issues FOR EACH ROW EXECUTE FUNCTION issue_initial_document();
CREATE FUNCTION document_required_owner() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF TG_TABLE_NAME='comments' THEN
  IF NOT EXISTS(SELECT 1 FROM documents WHERE workspace_id=NEW.workspace_id AND comment_id=NEW.id) THEN RAISE EXCEPTION 'Comment requires a document' USING ERRCODE='23514';END IF;
 ELSE
  IF NOT EXISTS(SELECT 1 FROM documents WHERE workspace_id=NEW.workspace_id AND issue_id=NEW.id AND plain_text=NEW.description AND version=NEW.body_version) THEN RAISE EXCEPTION 'Issue document projection is inconsistent' USING ERRCODE='23514';END IF;
 END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER comment_document_required AFTER INSERT ON comments DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION document_required_owner();
-- Compare final row state, not a stale trigger NEW from before the projection.
CREATE FUNCTION issue_document_consistent() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF TG_OP='UPDATE' AND (NEW.description,NEW.body_version) IS NOT DISTINCT FROM (OLD.description,OLD.body_version) THEN RETURN NULL;END IF;
 IF NOT EXISTS(SELECT 1 FROM issues i JOIN documents d ON d.workspace_id=i.workspace_id AND d.issue_id=i.id WHERE i.workspace_id=NEW.workspace_id AND i.id=NEW.id AND i.description=d.plain_text AND i.body_version=d.version) THEN
  RAISE EXCEPTION 'Issue document projection is inconsistent' USING ERRCODE='23514';END IF;RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER issue_document_consistent AFTER INSERT OR UPDATE ON issues DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION issue_document_consistent();
CREATE FUNCTION document_history_immutable() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF TG_OP='DELETE' AND current_user=pg_get_userbyid((SELECT relowner FROM pg_class WHERE oid=TG_RELID)) THEN RETURN OLD;END IF;
 RAISE EXCEPTION 'Document history is append only' USING ERRCODE='42501';
END $$;
CREATE TRIGGER document_revisions_immutable BEFORE UPDATE OR DELETE ON document_revisions FOR EACH ROW EXECUTE FUNCTION document_history_immutable();
CREATE TRIGGER document_revisions_no_truncate BEFORE TRUNCATE ON document_revisions FOR EACH STATEMENT EXECUTE FUNCTION document_history_immutable();
CREATE TRIGGER document_retention_immutable BEFORE UPDATE OR DELETE ON document_retention_operations FOR EACH ROW EXECUTE FUNCTION audit_append_only();
CREATE TRIGGER document_retention_no_truncate BEFORE TRUNCATE ON document_retention_operations FOR EACH STATEMENT EXECUTE FUNCTION audit_append_only();
CREATE FUNCTION worker_prune_document_revisions(w uuid,batch_limit integer) RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE n bigint;expected bigint;through_time timestamptz;
BEGIN
 IF batch_limit NOT BETWEEN 1 AND 1000 THEN RAISE EXCEPTION 'Invalid revision retention bound' USING ERRCODE='22023';END IF;
 PERFORM set_config('opentrack.workspace_id',w::text,true),set_config('opentrack.user_id','',true);
 PERFORM 1 FROM workspace_auth_guards WHERE workspace_id=w FOR SHARE;
 IF NOT FOUND THEN RETURN 0;END IF;
 WITH candidates AS MATERIALIZED(SELECT r.document_id,r.revision,r.created_at FROM document_revisions r JOIN documents d ON d.workspace_id=r.workspace_id AND d.id=r.document_id WHERE r.workspace_id=w AND r.created_at<clock_timestamp()-interval '90 days' AND r.revision<d.version ORDER BY r.created_at,r.document_id,r.revision LIMIT batch_limit FOR UPDATE OF r SKIP LOCKED),
 removed AS(DELETE FROM document_revisions r USING candidates c WHERE r.workspace_id=w AND r.document_id=c.document_id AND r.revision=c.revision RETURNING r.created_at)
 SELECT (SELECT count(*) FROM candidates),count(*),max(created_at) INTO expected,n,through_time FROM removed;
 IF n<>expected THEN RAISE EXCEPTION 'Required revision removal missing';END IF;
 IF n>0 THEN
  INSERT INTO document_retention_operations(workspace_id,id,deleted_count,through_created_at) VALUES(w,gen_random_uuid(),n,through_time);
  GET DIAGNOSTICS expected=ROW_COUNT;IF expected<>1 THEN RAISE EXCEPTION 'Required retention audit missing';END IF;
 END IF;RETURN n;
END $$;
DO $$ DECLARE p record;r record;BEGIN
 FOR p IN SELECT oid::regprocedure AS signature FROM pg_proc WHERE pronamespace=current_schema()::regnamespace AND proname IN('document_identity_guard','comment_identity_guard','document_effects','issue_initial_document','document_required_owner','issue_document_consistent','document_history_immutable','worker_prune_document_revisions') LOOP
  EXECUTE format('ALTER FUNCTION %s SET search_path=pg_catalog,%I,pg_temp',p.signature,current_schema());
  EXECUTE format('REVOKE ALL ON FUNCTION %s FROM PUBLIC',p.signature);
 END LOOP;
 FOR r IN SELECT DISTINCT grantee FROM information_schema.role_table_grants WHERE table_schema=current_schema() AND table_name IN('documents','document_revisions','document_retention_operations') AND grantee<>current_user LOOP
  EXECUTE format('REVOKE UPDATE,DELETE,TRUNCATE ON document_revisions FROM %I',r.grantee);
  EXECUTE format('REVOKE ALL ON document_retention_operations FROM %I',r.grantee);
 END LOOP;
END $$;
CREATE TRIGGER documents_no_hard_delete BEFORE DELETE ON documents FOR EACH ROW EXECUTE FUNCTION audit_append_only();
CREATE TRIGGER documents_no_truncate BEFORE TRUNCATE ON documents FOR EACH STATEMENT EXECUTE FUNCTION audit_append_only();
CREATE TRIGGER comments_no_hard_delete BEFORE DELETE ON comments FOR EACH ROW EXECUTE FUNCTION audit_append_only();
CREATE TRIGGER comments_no_truncate BEFORE TRUNCATE ON comments FOR EACH STATEMENT EXECUTE FUNCTION audit_append_only();
CREATE FUNCTION document_revision_source() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM documents d WHERE d.workspace_id=NEW.workspace_id AND d.id=NEW.document_id AND d.version=NEW.revision AND d.schema_version=NEW.schema_version AND d.body=NEW.body AND d.plain_text=NEW.plain_text AND d.updated_by_user_id=NEW.author_id AND d.updated_at=NEW.created_at) THEN
  RAISE EXCEPTION 'A revision must match its current document' USING ERRCODE='23514';END IF;RETURN NEW;
END $$;
CREATE TRIGGER document_revision_source BEFORE INSERT ON document_revisions FOR EACH ROW EXECUTE FUNCTION document_revision_source();
REVOKE ALL ON FUNCTION document_revision_source() FROM PUBLIC;

DO $$ BEGIN
 EXECUTE format('ALTER FUNCTION document_revision_source() SET search_path=pg_catalog,%I,pg_temp',current_schema());
END $$;
