-- Materialization and publication have independent readiness on the same guard.
ALTER TABLE notification_ready_workspaces ADD COLUMN capture_at timestamptz;
CREATE INDEX notification_capture_due ON notification_ready_workspaces(workspace_id,capture_at) WHERE capture_at IS NOT NULL;
CREATE FUNCTION notification_recompute_capture(w uuid) RETURNS void LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE due timestamptz;actual timestamptz;BEGIN
 IF w IS DISTINCT FROM nullif(current_setting('opentrack.workspace_id',true),'')::uuid THEN RAISE EXCEPTION 'Capture readiness requires the current workspace' USING ERRCODE='42501';END IF;
 INSERT INTO notification_ready_workspaces(workspace_id) VALUES(w) ON CONFLICT DO NOTHING;
 PERFORM workspace_id FROM notification_ready_workspaces WHERE workspace_id=w FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Required notification capture guard missing';END IF;
 -- No source/head locks follow the guard. Every query uses a fresh statement
 -- snapshot, so concurrent event append/checkpoint catch-up cannot lose a wake.
 SELECT CASE WHEN r.required AND r.enabled AND (c.backfill_job_id IS NULL OR c.last_seq<h.last_seq) AND coalesce(d.state,'')<>'blocked'
  THEN CASE WHEN d.state='retry_wait' THEN d.available_at ELSE '-infinity'::timestamptz END END INTO due
 FROM consumer_registrations r JOIN consumer_checkpoints c USING(workspace_id,consumer_name,consumer_version)
 JOIN workspace_stream_heads h USING(workspace_id)
 LEFT JOIN consumer_dead_letters d ON d.workspace_id=c.workspace_id AND d.consumer_name=c.consumer_name AND d.consumer_version=c.consumer_version AND d.seq=CASE WHEN c.backfill_job_id IS NULL THEN 0 ELSE c.last_seq+1 END
 WHERE r.workspace_id=w AND r.consumer_name='notifications' AND r.consumer_version=1;
 UPDATE notification_ready_workspaces SET capture_at=due WHERE workspace_id=w RETURNING capture_at INTO actual;
 IF NOT FOUND OR actual IS DISTINCT FROM due THEN RAISE EXCEPTION 'Required notification capture hint missing';END IF;
END $$;
CREATE FUNCTION notification_refresh_capture() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE w uuid;BEGIN
 IF TG_OP='UPDATE' AND NEW.workspace_id IS DISTINCT FROM OLD.workspace_id THEN RAISE EXCEPTION 'Capture workspace identity is immutable' USING ERRCODE='23514';END IF;
 IF TG_TABLE_NAME<>'workspace_events' THEN
  IF TG_OP='DELETE' THEN
   IF OLD.consumer_name<>'notifications' OR OLD.consumer_version<>1 THEN RETURN NULL;END IF;
  ELSE
   IF NEW.consumer_name<>'notifications' OR NEW.consumer_version<>1 THEN RETURN NULL;END IF;
  END IF;
 END IF;
 w:=CASE WHEN TG_OP='DELETE' THEN OLD.workspace_id ELSE NEW.workspace_id END;
 PERFORM notification_recompute_capture(w);RETURN NULL;
END $$;
CREATE TRIGGER notification_capture_event AFTER INSERT ON workspace_events FOR EACH ROW EXECUTE FUNCTION notification_refresh_capture();
CREATE TRIGGER notification_capture_checkpoint AFTER INSERT OR UPDATE OR DELETE ON consumer_checkpoints FOR EACH ROW EXECUTE FUNCTION notification_refresh_capture();
CREATE TRIGGER notification_capture_registration AFTER INSERT OR UPDATE OR DELETE ON consumer_registrations FOR EACH ROW EXECUTE FUNCTION notification_refresh_capture();
CREATE TRIGGER notification_capture_failure AFTER INSERT OR UPDATE OR DELETE ON consumer_dead_letters FOR EACH ROW EXECUTE FUNCTION notification_refresh_capture();
CREATE FUNCTION worker_ready_notification_capture_workspace(after_workspace uuid) RETURNS uuid LANGUAGE sql SECURITY DEFINER AS $$
 SELECT workspace_id FROM notification_ready_workspaces WHERE capture_at<=statement_timestamp() AND (after_workspace IS NULL OR workspace_id>after_workspace) ORDER BY workspace_id LIMIT 1
$$;
DO $$ DECLARE f regprocedure;r text;w uuid;previous text;BEGIN
 FOR f IN SELECT p.oid::regprocedure FROM pg_proc p WHERE p.pronamespace=current_schema()::regnamespace AND p.proname IN('notification_recompute_capture','notification_refresh_capture','worker_ready_notification_capture_workspace') LOOP
  EXECUTE format('ALTER FUNCTION %s SET search_path=pg_catalog,%I,pg_temp',f,current_schema());EXECUTE format('REVOKE ALL ON FUNCTION %s FROM PUBLIC',f);
  FOR r IN SELECT rolname FROM pg_roles WHERE rolname<>current_user LOOP EXECUTE format('REVOKE ALL ON FUNCTION %s FROM %I',f,r);END LOOP;
 END LOOP;
 previous:=current_setting('opentrack.workspace_id',true);
 FOR w IN SELECT workspace_id FROM worker_workspace_registry LOOP
  PERFORM set_config('opentrack.workspace_id',w::text,true);PERFORM notification_recompute_capture(w);
 END LOOP;
 PERFORM set_config('opentrack.workspace_id',coalesce(previous,''),true);
END $$;
