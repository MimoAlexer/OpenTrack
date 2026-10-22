-- Project-owned briefs with independent versions and retained legacy snapshots.
-- Runtime owner resolution, project creation and DTOs must change atomically.
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM pg_class WHERE oid IN('documents'::regclass,'document_revisions'::regclass,'projects'::regclass) AND pg_get_userbyid(relowner)<>current_user) THEN
  RAISE EXCEPTION 'Project document migration requires the table-owning role' USING ERRCODE='42501';
 END IF;
END $$;
-- Hold legacy writers until their exact final brief is copied and frozen.
LOCK TABLE projects IN ACCESS EXCLUSIVE MODE;
ALTER TABLE documents ADD COLUMN project_id uuid,
 ADD COLUMN migrated_initial boolean NOT NULL DEFAULT false,
 ALTER COLUMN updated_by_user_id DROP NOT NULL,
 DROP CONSTRAINT documents_check,
 ADD CONSTRAINT documents_one_owner CHECK(num_nonnulls(issue_id,comment_id,project_id)=1),
 ADD CONSTRAINT documents_project_fk FOREIGN KEY(workspace_id,project_id) REFERENCES projects(workspace_id,id),
 ADD CONSTRAINT documents_migrated_project CHECK(NOT migrated_initial OR project_id IS NOT NULL),
 ADD CONSTRAINT documents_author_required CHECK(updated_by_user_id IS NOT NULL OR (migrated_initial AND project_id IS NOT NULL AND version=1));
CREATE UNIQUE INDEX document_project_owner ON documents(workspace_id,project_id) WHERE project_id IS NOT NULL AND deleted_at IS NULL;
ALTER TABLE document_revisions ALTER COLUMN author_id DROP NOT NULL,
 ADD CONSTRAINT document_revision_author_required CHECK(author_id IS NOT NULL OR revision=1);
ALTER TABLE mention_intents ADD COLUMN project_id uuid,
 ALTER COLUMN issue_id DROP NOT NULL,
 ADD CONSTRAINT mention_intents_one_source CHECK(num_nonnulls(issue_id,project_id)=1),
 ADD CONSTRAINT mention_intents_project_fk FOREIGN KEY(workspace_id,project_id) REFERENCES projects(workspace_id,id);

CREATE OR REPLACE FUNCTION document_identity_guard() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF TG_OP='INSERT' THEN
  IF NEW.migrated_initial AND (NEW.project_id IS NULL OR NEW.version<>1 OR NEW.updated_by_user_id IS NOT NULL OR current_user<>pg_get_userbyid((SELECT relowner FROM pg_class WHERE oid=TG_RELID))) THEN
   RAISE EXCEPTION 'Only the migration owner may create an unattributed initial project snapshot' USING ERRCODE='42501';
  END IF;
 ELSE
  IF (NEW.id,NEW.workspace_id,NEW.issue_id,NEW.comment_id,NEW.project_id,NEW.created_at,NEW.migrated_initial) IS DISTINCT FROM (OLD.id,OLD.workspace_id,OLD.issue_id,OLD.comment_id,OLD.project_id,OLD.created_at,OLD.migrated_initial) THEN
   RAISE EXCEPTION 'Document ownership and migration provenance are immutable' USING ERRCODE='23514';
  END IF;
  IF NEW.version<>OLD.version+1 THEN RAISE EXCEPTION 'Document writes require the next version' USING ERRCODE='23514';END IF;
 END IF;
 IF NEW.updated_by_user_id IS NULL AND NOT (TG_OP='INSERT' AND NEW.migrated_initial AND NEW.project_id IS NOT NULL AND NEW.version=1 AND current_user=pg_get_userbyid((SELECT relowner FROM pg_class WHERE oid=TG_RELID))) THEN
  RAISE EXCEPTION 'Document writes require a real actor' USING ERRCODE='23514';
 END IF;
 RETURN NEW;
END $$;
DROP TRIGGER documents_identity ON documents;
CREATE TRIGGER documents_identity BEFORE INSERT OR UPDATE ON documents FOR EACH ROW EXECUTE FUNCTION document_identity_guard();

CREATE OR REPLACE FUNCTION document_revision_source() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF NEW.author_id IS NULL AND (NEW.revision<>1 OR current_user<>pg_get_userbyid((SELECT relowner FROM pg_class WHERE oid=TG_RELID)) OR NOT EXISTS(
  SELECT 1 FROM documents d WHERE d.workspace_id=NEW.workspace_id AND d.id=NEW.document_id AND d.project_id IS NOT NULL AND d.migrated_initial AND d.version=1 AND d.updated_by_user_id IS NULL
 )) THEN
  RAISE EXCEPTION 'Only the migration owner may record an unattributed initial project revision' USING ERRCODE='42501';
 END IF;
 IF NOT EXISTS(SELECT 1 FROM documents d WHERE d.workspace_id=NEW.workspace_id AND d.id=NEW.document_id AND d.version=NEW.revision AND d.schema_version=NEW.schema_version AND d.body=NEW.body AND d.plain_text=NEW.plain_text AND d.updated_by_user_id IS NOT DISTINCT FROM NEW.author_id AND d.updated_at=NEW.created_at
  AND (d.project_id IS NULL OR EXISTS(SELECT 1 FROM projects p WHERE p.workspace_id=d.workspace_id AND p.id=d.project_id AND p.team_id=NEW.origin_team_id))) THEN
  RAISE EXCEPTION 'A revision must match its current document' USING ERRCODE='23514';
 END IF;
 RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION document_effects() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE t uuid;n bigint;
BEGIN
 IF NEW.project_id IS NOT NULL THEN
  SELECT team_id INTO STRICT t FROM projects WHERE workspace_id=NEW.workspace_id AND id=NEW.project_id;
 ELSIF NEW.issue_id IS NOT NULL THEN
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

CREATE OR REPLACE FUNCTION document_required_owner() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE project uuid;
BEGIN
 IF TG_TABLE_NAME='projects' THEN project:=NEW.id;
 ELSIF TG_TABLE_NAME='documents' THEN
  IF NEW.project_id IS NULL THEN RETURN NULL;END IF;
  project:=NEW.project_id;
 ELSIF TG_TABLE_NAME='comments' THEN
  IF NOT EXISTS(SELECT 1 FROM documents WHERE workspace_id=NEW.workspace_id AND comment_id=NEW.id) THEN RAISE EXCEPTION 'Comment requires a document' USING ERRCODE='23514';END IF;
  RETURN NULL;
 ELSE
  IF NOT EXISTS(SELECT 1 FROM documents WHERE workspace_id=NEW.workspace_id AND issue_id=NEW.id AND plain_text=NEW.description AND version=NEW.body_version) THEN RAISE EXCEPTION 'Issue document projection is inconsistent' USING ERRCODE='23514';END IF;
  RETURN NULL;
 END IF;
 IF NOT EXISTS(SELECT 1 FROM documents WHERE workspace_id=NEW.workspace_id AND project_id=project AND deleted_at IS NULL) THEN
  RAISE EXCEPTION 'Project requires a live document' USING ERRCODE='23514';
 END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER project_document_required AFTER INSERT OR UPDATE ON projects DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION document_required_owner();
CREATE CONSTRAINT TRIGGER document_project_required AFTER INSERT OR UPDATE ON documents DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION document_required_owner();

CREATE OR REPLACE FUNCTION document_attachment_sync() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE ids uuid[];aid uuid;item attachments%ROWTYPE;bound_document uuid;team uuid;n bigint;
BEGIN
 -- Legacy snapshots preserve unknown schemas without inventing upload history.
 IF NEW.migrated_initial AND NEW.version=1 THEN RETURN NEW;END IF;
 WITH RECURSIVE nodes(value) AS(SELECT NEW.body UNION ALL SELECT child FROM nodes CROSS JOIN LATERAL jsonb_array_elements(CASE WHEN jsonb_typeof(value->'content')='array' THEN value->'content' ELSE '[]'::jsonb END) child)
 SELECT array_agg((value->'attrs'->>'id')::uuid ORDER BY (value->'attrs'->>'id')::uuid) INTO ids FROM nodes WHERE value->>'type'='attachment';
 IF coalesce(cardinality(ids),0)=0 THEN RETURN NEW;END IF;
 IF cardinality(ids)>100 OR cardinality(ids)<>(SELECT count(DISTINCT x) FROM unnest(ids) x) THEN RAISE EXCEPTION 'Invalid attachment reference count' USING ERRCODE='23514';END IF;
 IF NEW.project_id IS NOT NULL THEN
  SELECT team_id INTO STRICT team FROM projects WHERE workspace_id=NEW.workspace_id AND id=NEW.project_id;
 ELSE
  SELECT i.team_id INTO STRICT team FROM issues i WHERE i.workspace_id=NEW.workspace_id AND i.id=coalesce(NEW.issue_id,(SELECT issue_id FROM comments WHERE workspace_id=NEW.workspace_id AND id=NEW.comment_id));
 END IF;
 FOREACH aid IN ARRAY ids LOOP
  SELECT * INTO item FROM attachments WHERE workspace_id=NEW.workspace_id AND id=aid FOR UPDATE;
  IF NOT FOUND THEN RAISE EXCEPTION 'Attachment is unavailable' USING ERRCODE='23514';END IF;
  SELECT document_id INTO bound_document FROM document_attachments WHERE workspace_id=NEW.workspace_id AND attachment_id=aid;
  IF (item.state<>'ready' OR item.blob_deleted_at IS NOT NULL) AND NOT coalesce(
     (TG_OP='UPDATE' AND bound_document=NEW.id AND EXISTS(SELECT 1 FROM document_attachment_revisions ar WHERE ar.workspace_id=NEW.workspace_id AND ar.document_id=NEW.id AND ar.revision=OLD.version AND ar.attachment_id=aid)),false)
  THEN RAISE EXCEPTION 'Attachment is unavailable' USING ERRCODE='23514';END IF;
  IF bound_document IS NOT NULL THEN
   IF bound_document<>NEW.id THEN RAISE EXCEPTION 'Attachment belongs to another document' USING ERRCODE='23514';END IF;
   IF NOT EXISTS(SELECT 1 FROM document_attachment_revisions ar JOIN document_revisions r ON r.workspace_id=ar.workspace_id AND r.document_id=ar.document_id AND r.revision=ar.revision JOIN teams t ON t.workspace_id=r.workspace_id AND t.id=r.origin_team_id
      WHERE ar.workspace_id=NEW.workspace_id AND ar.document_id=NEW.id AND ar.attachment_id=aid
      AND ((TG_OP='UPDATE' AND ar.revision=OLD.version) OR (r.created_at>clock_timestamp()-interval '90 days' AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members m WHERE m.workspace_id=NEW.workspace_id AND m.team_id=t.id AND m.user_id=NEW.updated_by_user_id)))))
   THEN RAISE EXCEPTION 'Retained attachment history is unavailable' USING ERRCODE='23514';END IF;
  ELSE
   IF item.uploader_id<>NEW.updated_by_user_id OR item.expires_at<=clock_timestamp() OR
      (item.reserved_document_id IS NOT NULL AND item.reserved_document_id<>NEW.id) OR
      (item.reserved_document_id IS NULL AND item.upload_team_id<>team)
   THEN RAISE EXCEPTION 'Attachment reservation does not authorize this document' USING ERRCODE='23514';END IF;
   INSERT INTO document_attachments(workspace_id,document_id,attachment_id) VALUES(NEW.workspace_id,NEW.id,aid);
   GET DIAGNOSTICS n=ROW_COUNT;IF n<>1 THEN RAISE EXCEPTION 'Required attachment link missing';END IF;
   UPDATE attachments SET version=version+1 WHERE workspace_id=NEW.workspace_id AND id=aid;
   GET DIAGNOSTICS n=ROW_COUNT;IF n<>1 THEN RAISE EXCEPTION 'Required attachment version missing';END IF;
   INSERT INTO audit_entries(id,workspace_id,actor_id,action,target_id,metadata) VALUES(gen_random_uuid(),NEW.workspace_id,NEW.updated_by_user_id,'attachment.linked',aid,jsonb_build_object('document_id',NEW.id));
   GET DIAGNOSTICS n=ROW_COUNT;IF n<>1 THEN RAISE EXCEPTION 'Required attachment audit missing';END IF;
  END IF;
  INSERT INTO document_attachment_revisions(workspace_id,document_id,revision,attachment_id) VALUES(NEW.workspace_id,NEW.id,NEW.version,aid);
  GET DIAGNOSTICS n=ROW_COUNT;IF n<>1 THEN RAISE EXCEPTION 'Required attachment history link missing';END IF;
 END LOOP;RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION document_mention_sync() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE parent issues%ROWTYPE;team uuid;origin uuid;ids uuid[];old_ids uuid[];u uuid;node jsonb;n bigint;expected bigint;
BEGIN
 -- Migration is not an authored mention and must not create delivery sources.
 IF NEW.migrated_initial AND NEW.version=1 THEN RETURN NEW;END IF;
 IF NEW.project_id IS NOT NULL THEN
  SELECT team_id INTO STRICT team FROM projects WHERE workspace_id=NEW.workspace_id AND id=NEW.project_id;
  origin:=team;
 ELSIF NEW.issue_id IS NOT NULL THEN
  SELECT * INTO STRICT parent FROM issues WHERE workspace_id=NEW.workspace_id AND id=NEW.issue_id;
  team:=parent.team_id;origin:=team;
 ELSE
  SELECT i.* INTO STRICT parent FROM comments c JOIN issues i ON i.workspace_id=c.workspace_id AND i.id=c.issue_id WHERE c.workspace_id=NEW.workspace_id AND c.id=NEW.comment_id;
  SELECT origin_team_id INTO STRICT origin FROM comments WHERE workspace_id=NEW.workspace_id AND id=NEW.comment_id;
  team:=parent.team_id;
 END IF;
 ids:='{}';
 FOR node IN SELECT value FROM jsonb_path_query(NEW.body,'$.** ? (@.type == "userMention")') AS mentions(value) LOOP
  IF jsonb_typeof(node->'attrs') IS DISTINCT FROM 'object' THEN RAISE EXCEPTION 'Mention requires attributes' USING ERRCODE='23514';END IF;
  IF (SELECT count(*) FROM jsonb_object_keys(node->'attrs'))<>1 OR NOT (node->'attrs' ? 'id') OR jsonb_typeof(node->'attrs'->'id') IS DISTINCT FROM 'string' THEN RAISE EXCEPTION 'Mention requires an ID-only node' USING ERRCODE='23514';END IF;
  BEGIN u:=(node->'attrs'->>'id')::uuid;EXCEPTION WHEN invalid_text_representation THEN RAISE EXCEPTION 'Invalid mention user ID' USING ERRCODE='23514';END;
  IF (node->'attrs'->>'id')<>u::text THEN RAISE EXCEPTION 'Mention ID must be canonical' USING ERRCODE='23514';END IF;
  IF NOT u=ANY(ids) THEN ids:=array_append(ids,u);END IF;
 END LOOP;
 IF cardinality(ids)>50 THEN RAISE EXCEPTION 'At most 50 distinct mentions' USING ERRCODE='23514';END IF;
 SELECT coalesce(array_agg(user_id),'{}') INTO old_ids FROM document_mentions WHERE workspace_id=NEW.workspace_id AND document_id=NEW.id;
 FOREACH u IN ARRAY ids LOOP
  IF NOT u=ANY(old_ids) AND NOT mention_eligible(NEW.workspace_id,team,origin,u) THEN RAISE EXCEPTION 'A newly mentioned user cannot read this document' USING ERRCODE='23514';END IF;
 END LOOP;
 SELECT count(*) INTO expected FROM unnest(old_ids) x WHERE NOT x=ANY(ids);
 DELETE FROM document_mentions WHERE workspace_id=NEW.workspace_id AND document_id=NEW.id AND NOT user_id=ANY(ids);
 GET DIAGNOSTICS n=ROW_COUNT;IF n<>expected THEN RAISE EXCEPTION 'Required removed mention links missing';END IF;
 FOREACH u IN ARRAY ids LOOP
  IF NOT u=ANY(old_ids) THEN
   INSERT INTO document_mentions(workspace_id,document_id,user_id) VALUES(NEW.workspace_id,NEW.id,u);
   GET DIAGNOSTICS n=ROW_COUNT;IF n<>1 THEN RAISE EXCEPTION 'Required mention link missing';END IF;
   IF NEW.project_id IS NULL THEN PERFORM subscription_auto(NEW.workspace_id,parent.id,u,'mention');END IF;
   IF u<>NEW.updated_by_user_id THEN
    INSERT INTO mention_intents(workspace_id,id,issue_id,project_id,document_id,document_version,origin_team_id,recipient_id,actor_id)
    VALUES(NEW.workspace_id,gen_random_uuid(),parent.id,NEW.project_id,NEW.id,NEW.version,origin,u,NEW.updated_by_user_id);
    GET DIAGNOSTICS n=ROW_COUNT;IF n<>1 THEN RAISE EXCEPTION 'Required mention intent missing';END IF;
   END IF;
  END IF;
 END LOOP;
 IF TG_OP='INSERT' AND NEW.comment_id IS NOT NULL THEN
  INSERT INTO discussion_sources(workspace_id,id,issue_id,comment_id,document_id,document_version,origin_team_id,actor_id)
  VALUES(NEW.workspace_id,gen_random_uuid(),parent.id,NEW.comment_id,NEW.id,NEW.version,origin,NEW.updated_by_user_id);
  GET DIAGNOSTICS n=ROW_COUNT;IF n<>1 THEN RAISE EXCEPTION 'Required discussion source missing';END IF;
 END IF;
 RETURN NEW;
END $$;

DO $$ DECLARE p regprocedure;BEGIN
 FOREACH p IN ARRAY ARRAY['document_identity_guard()'::regprocedure,'document_revision_source()'::regprocedure,'document_effects()'::regprocedure,'document_required_owner()'::regprocedure,'document_attachment_sync()'::regprocedure,'document_mention_sync()'::regprocedure] LOOP
  EXECUTE format('ALTER FUNCTION %s SET search_path=pg_catalog,%I,pg_temp',p,current_schema());
  EXECUTE format('REVOKE ALL ON FUNCTION %s FROM PUBLIC',p);
 END LOOP;
END $$;

-- No historical author or editing time can be recovered from project metadata.
-- The immutable flag identifies revision 1; history reads derive that provenance.
DO $$ DECLARE w uuid;n bigint;expected bigint;captured_at timestamptz:=clock_timestamp();previous_workspace text:=current_setting('opentrack.workspace_id',true);BEGIN
 FOR w IN SELECT id FROM workspaces ORDER BY id LOOP
  PERFORM set_config('opentrack.workspace_id',w::text,true);
  SELECT count(*) INTO expected FROM projects WHERE workspace_id=w;
  INSERT INTO documents(id,workspace_id,project_id,schema_version,body,plain_text,updated_by_user_id,migrated_initial,created_at,updated_at)
  SELECT gen_random_uuid(),workspace_id,id,narrative_schema_version,narrative_body,narrative_plain_text,NULL,true,captured_at,captured_at FROM projects WHERE workspace_id=w;
  GET DIAGNOSTICS n=ROW_COUNT;IF n<>expected THEN RAISE EXCEPTION 'Project document backfill incomplete';END IF;
  SELECT count(*) INTO n FROM projects p JOIN documents d ON d.workspace_id=p.workspace_id AND d.project_id=p.id AND d.deleted_at IS NULL
   JOIN document_revisions r ON r.workspace_id=d.workspace_id AND r.document_id=d.id AND r.revision=1
   WHERE p.workspace_id=w AND d.version=1 AND d.migrated_initial AND d.updated_by_user_id IS NULL AND d.schema_version=p.narrative_schema_version AND d.body=p.narrative_body AND d.plain_text=p.narrative_plain_text
   AND d.created_at=captured_at AND d.updated_at=captured_at AND r.author_id IS NULL AND r.origin_team_id=p.team_id AND r.schema_version=d.schema_version AND r.body=d.body AND r.plain_text=d.plain_text AND r.created_at=captured_at;
  IF n<>expected THEN RAISE EXCEPTION 'Project document snapshot verification incomplete';END IF;
  -- Deferred owner checks must run while their workspace is still selected.
  SET CONSTRAINTS document_project_required IMMEDIATE;
  SET CONSTRAINTS document_project_required DEFERRED;
 END LOOP;
 PERFORM set_config('opentrack.workspace_id',coalesce(previous_workspace,''),true);
END $$;
-- Contract removal requires a supported release overlap and restore-tested backup.
-- Retain the old values as immutable snapshots; documents owns mutable briefs.
CREATE OR REPLACE FUNCTION project_guard() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF (NEW.id,NEW.workspace_id,NEW.team_id,NEW.created_at) IS DISTINCT FROM(OLD.id,OLD.workspace_id,OLD.team_id,OLD.created_at) OR NEW.version<>OLD.version+1 THEN
  RAISE EXCEPTION 'Project identity is immutable and version must advance once' USING ERRCODE='23514';
 END IF;
 IF (NEW.narrative_schema_version,NEW.narrative_body,NEW.narrative_plain_text) IS DISTINCT FROM (OLD.narrative_schema_version,OLD.narrative_body,OLD.narrative_plain_text) THEN
  RAISE EXCEPTION 'Legacy project narrative is an immutable snapshot; use the project document' USING ERRCODE='23514';
 END IF;
 RETURN NEW;
END $$;
DO $$ BEGIN
 EXECUTE format('ALTER FUNCTION project_guard() SET search_path=pg_catalog,%I,pg_temp',current_schema());
 REVOKE ALL ON FUNCTION project_guard() FROM PUBLIC;
END $$;
