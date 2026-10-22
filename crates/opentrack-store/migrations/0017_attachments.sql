CREATE TABLE workspace_storage_usage (
 workspace_id uuid PRIMARY KEY REFERENCES workspaces(id),charged_bytes bigint NOT NULL DEFAULT 0 CHECK(charged_bytes>=0)
);
CREATE TABLE attachments (
 id uuid PRIMARY KEY,workspace_id uuid NOT NULL,upload_team_id uuid NOT NULL,uploader_id uuid NOT NULL REFERENCES users(id),
 reserved_document_id uuid,storage_key uuid NOT NULL UNIQUE,original_name text NOT NULL CHECK(octet_length(original_name) BETWEEN 1 AND 255),
 size_bytes bigint NOT NULL CHECK(size_bytes>0 AND size_bytes<=104857600),sha256 text NOT NULL CHECK(sha256~'^[0-9a-f]{64}$'),mime_hint text,detected_mime text,
 state text NOT NULL DEFAULT 'pending' CHECK(state IN('pending','quarantined','ready','rejected','deleted')),
 scan_status text NOT NULL DEFAULT 'pending' CHECK(scan_status IN('pending','clean','not_scanned','infected','unavailable')),
 antivirus_required boolean NOT NULL,version bigint NOT NULL DEFAULT 1 CHECK(version>0),generation bigint NOT NULL DEFAULT 0 CHECK(generation>=0),
 lease_token uuid,lease_expires_at timestamptz,gc_token uuid,gc_expires_at timestamptz,
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),expires_at timestamptz NOT NULL,deleted_at timestamptz,blob_deleted_at timestamptz,
 UNIQUE(workspace_id,id),FOREIGN KEY(workspace_id,upload_team_id) REFERENCES teams(workspace_id,id),
 FOREIGN KEY(workspace_id,reserved_document_id) REFERENCES documents(workspace_id,id),
 CHECK((lease_token IS NULL)=(lease_expires_at IS NULL)),CHECK((gc_token IS NULL)=(gc_expires_at IS NULL)),
 CHECK(state<>'ready' OR (detected_mime IS NOT NULL AND scan_status IN('clean','not_scanned') AND (NOT antivirus_required OR scan_status='clean'))),
 CHECK(state<>'quarantined' OR detected_mime IS NOT NULL),
 CHECK(state NOT IN('ready','rejected','deleted') OR lease_token IS NULL),
 CHECK(state<>'deleted' OR deleted_at IS NOT NULL),
 CHECK(blob_deleted_at IS NULL OR state IN('rejected','deleted'))
);
CREATE INDEX attachments_draft_page ON attachments(workspace_id,upload_team_id,uploader_id,created_at DESC,id DESC);
CREATE INDEX attachments_cleanup ON attachments(workspace_id,expires_at,id) WHERE blob_deleted_at IS NULL;
CREATE TABLE document_attachments (
 workspace_id uuid NOT NULL,document_id uuid NOT NULL,attachment_id uuid NOT NULL,linked_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(workspace_id,attachment_id),UNIQUE(workspace_id,document_id,attachment_id),
 FOREIGN KEY(workspace_id,document_id) REFERENCES documents(workspace_id,id),
 FOREIGN KEY(workspace_id,attachment_id) REFERENCES attachments(workspace_id,id)
);
CREATE TABLE document_attachment_revisions (
 workspace_id uuid NOT NULL,document_id uuid NOT NULL,revision bigint NOT NULL,attachment_id uuid NOT NULL,
 PRIMARY KEY(workspace_id,document_id,revision,attachment_id),
 FOREIGN KEY(workspace_id,document_id,revision) REFERENCES document_revisions(workspace_id,document_id,revision) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
 FOREIGN KEY(workspace_id,document_id,attachment_id) REFERENCES document_attachments(workspace_id,document_id,attachment_id)
);
CREATE INDEX document_attachment_retained ON document_attachment_revisions(workspace_id,attachment_id,document_id,revision);
CREATE TABLE attachment_storage_operations (
 workspace_id uuid NOT NULL REFERENCES workspaces(id),id uuid NOT NULL,attachment_id uuid NOT NULL,generation bigint NOT NULL,
 action text NOT NULL CHECK(action IN('gc.claimed','gc.removed')),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(workspace_id,id),FOREIGN KEY(workspace_id,attachment_id) REFERENCES attachments(workspace_id,id)
);
DO $$ DECLARE tab text; BEGIN
 FOREACH tab IN ARRAY ARRAY['workspace_storage_usage','attachments','document_attachments','document_attachment_revisions','attachment_storage_operations'] LOOP
  EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',tab);
  EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',tab);
  EXECUTE format('CREATE POLICY tenant_scope ON %I USING(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid) WITH CHECK(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid)',tab);
 END LOOP;
END $$;
CREATE FUNCTION attachment_identity_guard() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF (NEW.id,NEW.workspace_id,NEW.upload_team_id,NEW.uploader_id,NEW.reserved_document_id,NEW.storage_key,NEW.original_name,NEW.size_bytes,NEW.sha256,NEW.mime_hint,NEW.created_at,NEW.expires_at)
 IS DISTINCT FROM (OLD.id,OLD.workspace_id,OLD.upload_team_id,OLD.uploader_id,OLD.reserved_document_id,OLD.storage_key,OLD.original_name,OLD.size_bytes,OLD.sha256,OLD.mime_hint,OLD.created_at,OLD.expires_at)
 OR NEW.version<>OLD.version+1 OR NEW.generation<OLD.generation OR (OLD.antivirus_required AND NOT NEW.antivirus_required)
 OR (OLD.state='ready' AND NEW.state NOT IN('ready','deleted'))
 OR (OLD.state='pending' AND NEW.state NOT IN('pending','quarantined','rejected','deleted'))
 OR (OLD.state='quarantined' AND NEW.state NOT IN('quarantined','ready','rejected','deleted'))
 OR (OLD.state='rejected' AND NEW.state NOT IN('rejected','deleted'))
 OR (OLD.state='deleted' AND NEW.state<>'deleted')
 OR (OLD.blob_deleted_at IS NOT NULL AND NEW.blob_deleted_at IS DISTINCT FROM OLD.blob_deleted_at)
 OR (OLD.detected_mime IS NOT NULL AND NEW.detected_mime IS DISTINCT FROM OLD.detected_mime)
 THEN RAISE EXCEPTION 'Attachment identity or terminal state is immutable' USING ERRCODE='23514';END IF;
 IF NEW.blob_deleted_at IS DISTINCT FROM OLD.blob_deleted_at AND current_user<>pg_get_userbyid((SELECT relowner FROM pg_class WHERE oid=TG_RELID)) THEN
  RAISE EXCEPTION 'Physical removal requires the cleanup capability' USING ERRCODE='42501';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER attachments_identity BEFORE UPDATE ON attachments FOR EACH ROW EXECUTE FUNCTION attachment_identity_guard();
CREATE TRIGGER attachments_no_hard_delete BEFORE DELETE ON attachments FOR EACH ROW EXECUTE FUNCTION audit_append_only();
CREATE TRIGGER attachments_no_truncate BEFORE TRUNCATE ON attachments FOR EACH STATEMENT EXECUTE FUNCTION audit_append_only();
CREATE TRIGGER document_attachment_immutable BEFORE UPDATE OR DELETE ON document_attachments FOR EACH ROW EXECUTE FUNCTION audit_append_only();
CREATE TRIGGER document_attachment_no_truncate BEFORE TRUNCATE ON document_attachments FOR EACH STATEMENT EXECUTE FUNCTION audit_append_only();
CREATE TRIGGER document_attachment_revision_immutable BEFORE UPDATE OR DELETE ON document_attachment_revisions FOR EACH ROW EXECUTE FUNCTION document_history_immutable();
CREATE TRIGGER document_attachment_revision_no_truncate BEFORE TRUNCATE ON document_attachment_revisions FOR EACH STATEMENT EXECUTE FUNCTION audit_append_only();
CREATE TRIGGER attachment_storage_immutable BEFORE UPDATE OR DELETE ON attachment_storage_operations FOR EACH ROW EXECUTE FUNCTION audit_append_only();
CREATE TRIGGER attachment_storage_no_truncate BEFORE TRUNCATE ON attachment_storage_operations FOR EACH STATEMENT EXECUTE FUNCTION audit_append_only();

CREATE FUNCTION document_attachment_sync() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE ids uuid[];aid uuid;item attachments%ROWTYPE;bound_document uuid;team uuid;n bigint;
BEGIN
 WITH RECURSIVE nodes(value) AS(SELECT NEW.body UNION ALL SELECT child FROM nodes CROSS JOIN LATERAL jsonb_array_elements(CASE WHEN jsonb_typeof(value->'content')='array' THEN value->'content' ELSE '[]'::jsonb END) child)
 SELECT array_agg((value->'attrs'->>'id')::uuid ORDER BY (value->'attrs'->>'id')::uuid) INTO ids FROM nodes WHERE value->>'type'='attachment';
 IF coalesce(cardinality(ids),0)=0 THEN RETURN NEW;END IF;
 IF cardinality(ids)>100 OR cardinality(ids)<>(SELECT count(DISTINCT x) FROM unnest(ids) x) THEN RAISE EXCEPTION 'Invalid attachment reference count' USING ERRCODE='23514';END IF;
 SELECT i.team_id INTO STRICT team FROM issues i WHERE i.workspace_id=NEW.workspace_id AND i.id=coalesce(NEW.issue_id,(SELECT issue_id FROM comments WHERE workspace_id=NEW.workspace_id AND id=NEW.comment_id));
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
CREATE TRIGGER documents_attachment_sync AFTER INSERT OR UPDATE ON documents FOR EACH ROW EXECUTE FUNCTION document_attachment_sync();

CREATE FUNCTION worker_claim_attachment_gc(w uuid,batch_limit integer)
RETURNS TABLE(id uuid,storage_key uuid,generation bigint,lease_token uuid,lease_expires_at timestamptz)
LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE item attachments%ROWTYPE;token uuid;expires timestamptz;n bigint;
BEGIN
 IF batch_limit NOT BETWEEN 1 AND 100 THEN RAISE EXCEPTION 'Invalid cleanup bound' USING ERRCODE='22023';END IF;
 PERFORM set_config('opentrack.workspace_id',w::text,true),set_config('opentrack.user_id','',true);
 PERFORM 1 FROM workspace_auth_guards WHERE workspace_id=w FOR SHARE;IF NOT FOUND THEN RETURN;END IF;
 FOR item IN SELECT a.* FROM attachments a WHERE a.workspace_id=w AND a.blob_deleted_at IS NULL AND (a.gc_expires_at IS NULL OR a.gc_expires_at<=clock_timestamp()) AND
 (a.state IN('rejected','deleted') OR (a.expires_at<=clock_timestamp() AND (a.state IN('pending','quarantined') OR NOT EXISTS(
   SELECT 1 FROM document_attachment_revisions ar JOIN documents d ON d.workspace_id=ar.workspace_id AND d.id=ar.document_id JOIN document_revisions r ON r.workspace_id=ar.workspace_id AND r.document_id=ar.document_id AND r.revision=ar.revision
   WHERE ar.workspace_id=w AND ar.attachment_id=a.id AND (r.revision=d.version OR r.created_at>clock_timestamp()-interval '90 days')))))
 ORDER BY a.expires_at,a.id LIMIT batch_limit FOR UPDATE OF a SKIP LOCKED LOOP
  token:=gen_random_uuid();expires:=clock_timestamp()+interval '300 seconds';
  UPDATE attachments a SET state='deleted',deleted_at=coalesce(a.deleted_at,clock_timestamp()),version=a.version+1,generation=a.generation+1,lease_token=NULL,lease_expires_at=NULL,gc_token=token,gc_expires_at=expires WHERE a.workspace_id=w AND a.id=item.id;
  GET DIAGNOSTICS n=ROW_COUNT;IF n<>1 THEN RAISE EXCEPTION 'Required cleanup fence missing';END IF;
  INSERT INTO attachment_storage_operations(workspace_id,id,attachment_id,generation,action) VALUES(w,gen_random_uuid(),item.id,item.generation+1,'gc.claimed');
  GET DIAGNOSTICS n=ROW_COUNT;IF n<>1 THEN RAISE EXCEPTION 'Required cleanup audit missing';END IF;
  id:=item.id;storage_key:=item.storage_key;generation:=item.generation+1;lease_token:=token;lease_expires_at:=expires;RETURN NEXT;
 END LOOP;
END $$;
CREATE FUNCTION worker_ack_attachment_gc(w uuid,attachment uuid,fence bigint,token uuid) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE item attachments%ROWTYPE;n bigint;
BEGIN
 PERFORM set_config('opentrack.workspace_id',w::text,true),set_config('opentrack.user_id','',true);
 PERFORM 1 FROM workspace_auth_guards WHERE workspace_id=w FOR SHARE;IF NOT FOUND THEN RETURN false;END IF;
 PERFORM 1 FROM workspace_storage_usage WHERE workspace_id=w FOR UPDATE;
 SELECT * INTO item FROM attachments WHERE workspace_id=w AND id=attachment FOR UPDATE;
 IF NOT FOUND OR item.state<>'deleted' OR item.blob_deleted_at IS NOT NULL OR item.generation<>fence OR item.gc_token IS DISTINCT FROM token OR item.gc_expires_at<=clock_timestamp() THEN RETURN false;END IF;
 UPDATE workspace_storage_usage SET charged_bytes=charged_bytes-item.size_bytes WHERE workspace_id=w AND charged_bytes>=item.size_bytes;
 GET DIAGNOSTICS n=ROW_COUNT;IF n<>1 THEN RAISE EXCEPTION 'Required quota release missing';END IF;
 UPDATE attachments SET blob_deleted_at=clock_timestamp(),version=version+1,gc_token=NULL,gc_expires_at=NULL WHERE workspace_id=w AND id=attachment;
 GET DIAGNOSTICS n=ROW_COUNT;IF n<>1 THEN RAISE EXCEPTION 'Required cleanup completion missing';END IF;
 INSERT INTO attachment_storage_operations(workspace_id,id,attachment_id,generation,action) VALUES(w,gen_random_uuid(),attachment,fence,'gc.removed');
 GET DIAGNOSTICS n=ROW_COUNT;IF n<>1 THEN RAISE EXCEPTION 'Required cleanup audit missing';END IF;
 RETURN true;
END $$;
DO $$ DECLARE p record;r record;BEGIN
 FOR p IN SELECT oid::regprocedure AS signature FROM pg_proc WHERE pronamespace=current_schema()::regnamespace AND proname IN('attachment_identity_guard','document_attachment_sync','worker_claim_attachment_gc','worker_ack_attachment_gc') LOOP
  EXECUTE format('ALTER FUNCTION %s SET search_path=pg_catalog,%I,pg_temp',p.signature,current_schema());
  EXECUTE format('REVOKE ALL ON FUNCTION %s FROM PUBLIC',p.signature);
 END LOOP;
 FOR r IN SELECT DISTINCT grantee FROM information_schema.role_table_grants WHERE table_schema=current_schema() AND table_name='attachments' AND grantee<>current_user LOOP
  EXECUTE format('REVOKE ALL ON attachment_storage_operations FROM %I',r.grantee);
  EXECUTE format('REVOKE UPDATE,DELETE,TRUNCATE ON document_attachments,document_attachment_revisions FROM %I',r.grantee);
 END LOOP;
END $$;
