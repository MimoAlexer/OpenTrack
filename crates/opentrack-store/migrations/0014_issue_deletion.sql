-- Tombstones retain stable identity and key aliases. There is no purge here.
CREATE INDEX issues_deleted_listing ON issues(workspace_id,deleted_at DESC,id DESC)
 WHERE deleted_at IS NOT NULL;
CREATE INDEX issues_deleted_team ON issues(workspace_id,team_id,deleted_at DESC,id DESC)
 WHERE deleted_at IS NOT NULL;

CREATE FUNCTION issue_deletion_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 -- Status-only worker updates must not read any graph or account columns/tables.
 IF TG_OP='UPDATE' AND (NEW.workspace_id,NEW.team_id,NEW.parent_id,NEW.deleted_at)
  IS NOT DISTINCT FROM (OLD.workspace_id,OLD.team_id,OLD.parent_id,OLD.deleted_at) THEN RETURN NEW; END IF;
 PERFORM 1 FROM teams WHERE workspace_id=NEW.workspace_id AND id=NEW.team_id FOR UPDATE;
 IF NEW.parent_id IS NOT NULL AND (NEW.deleted_at IS NOT NULL OR EXISTS(
  SELECT 1 FROM issues WHERE workspace_id=NEW.workspace_id AND id=NEW.parent_id AND deleted_at IS NOT NULL)) THEN
  RAISE EXCEPTION 'Deleted issues cannot acquire hierarchy links' USING ERRCODE='23514',CONSTRAINT='issue_deleted_graph';
 END IF;
 IF TG_OP='UPDATE' AND NEW.deleted_at IS DISTINCT FROM OLD.deleted_at THEN
  IF OLD.deleted_at IS NOT NULL AND NEW.deleted_at IS NULL AND clock_timestamp()>=OLD.deleted_at+interval '30 days' THEN
   RAISE EXCEPTION 'The restore period has ended' USING ERRCODE='23514',CONSTRAINT='issue_restore_expired';
  END IF;
  IF OLD.deleted_at IS NOT NULL AND NEW.deleted_at IS NOT NULL AND current_user<>(SELECT pg_get_userbyid(relowner) FROM pg_class WHERE oid=TG_RELID) THEN
   RAISE EXCEPTION 'Deletion time is immutable' USING ERRCODE='23514',CONSTRAINT='issue_deletion_immutable';
  END IF;
  IF (OLD.deleted_at IS NULL OR NEW.deleted_at IS NULL) AND (NEW.parent_id IS NOT NULL OR EXISTS(
   SELECT 1 FROM issues WHERE workspace_id=NEW.workspace_id AND parent_id=NEW.id) OR EXISTS(
   SELECT 1 FROM issue_relations WHERE workspace_id=NEW.workspace_id AND (source_id=NEW.id OR target_id=NEW.id))) THEN
   RAISE EXCEPTION 'Detach issue connections before changing deletion state' USING ERRCODE='23514',CONSTRAINT='issue_deleted_graph';
  END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER issues_deletion_guard BEFORE INSERT OR UPDATE ON issues FOR EACH ROW EXECUTE FUNCTION issue_deletion_guard();

CREATE FUNCTION issue_deleted_relation_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 PERFORM 1 FROM teams WHERE workspace_id=NEW.workspace_id AND id=NEW.team_id FOR UPDATE;
 IF EXISTS(SELECT 1 FROM issues WHERE workspace_id=NEW.workspace_id AND id IN(NEW.source_id,NEW.target_id) AND deleted_at IS NOT NULL) THEN
  RAISE EXCEPTION 'Deleted issues cannot acquire relationships' USING ERRCODE='23514',CONSTRAINT='issue_deleted_graph';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER issue_relations_deleted_guard BEFORE INSERT OR UPDATE ON issue_relations FOR EACH ROW EXECUTE FUNCTION issue_deleted_relation_guard();
