-- Private scheduling metadata only; recipient outbox RLS is unchanged.
CREATE TABLE notification_ready_workspaces (
 workspace_id uuid PRIMARY KEY REFERENCES workspaces(id),available_at timestamptz
);
ALTER TABLE notification_ready_workspaces ENABLE ROW LEVEL SECURITY;
ALTER TABLE notification_ready_workspaces FORCE ROW LEVEL SECURITY;
DO $$ BEGIN
 EXECUTE format('CREATE POLICY readiness_owner_read ON notification_ready_workspaces FOR SELECT USING (current_user=%L)',current_user);
 EXECUTE format('CREATE POLICY readiness_owner_insert ON notification_ready_workspaces FOR INSERT WITH CHECK (current_user=%L AND workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid)',current_user);
 EXECUTE format('CREATE POLICY readiness_owner_update ON notification_ready_workspaces FOR UPDATE USING (current_user=%L AND workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid) WITH CHECK (workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid)',current_user);
END $$;
CREATE INDEX notification_ready_due ON notification_ready_workspaces(workspace_id,available_at) WHERE available_at IS NOT NULL;

CREATE FUNCTION notification_refresh_readiness() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE w uuid;due timestamptz;actual timestamptz;BEGIN
 IF TG_OP='UPDATE' AND NEW.workspace_id IS DISTINCT FROM OLD.workspace_id THEN RAISE EXCEPTION 'Readiness workspace identity is immutable' USING ERRCODE='23514';END IF;
 w:=CASE WHEN TG_OP='DELETE' THEN OLD.workspace_id ELSE NEW.workspace_id END;
 IF w IS DISTINCT FROM nullif(current_setting('opentrack.workspace_id',true),'')::uuid THEN RAISE EXCEPTION 'Readiness requires the current workspace' USING ERRCODE='42501';END IF;
 -- Never delete the guard: serialize before the fresh READ COMMITTED query.
 -- Outbox/source locks precede this lock; no new source locks are taken here.
 INSERT INTO notification_ready_workspaces(workspace_id) VALUES(w) ON CONFLICT DO NOTHING;
 PERFORM workspace_id FROM notification_ready_workspaces WHERE workspace_id=w FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Required notification readiness guard missing';END IF;
 SELECT available_at INTO due FROM notification_invalidations WHERE workspace_id=w AND published_seq IS NULL AND NOT blocked ORDER BY available_at,notification_id,notification_version LIMIT 1;
 UPDATE notification_ready_workspaces SET available_at=due WHERE workspace_id=w RETURNING available_at INTO actual;
 IF NOT FOUND OR actual IS DISTINCT FROM due THEN RAISE EXCEPTION 'Required notification readiness update missing';END IF;
 RETURN NULL;
END $$;
CREATE TRIGGER notification_readiness AFTER INSERT OR UPDATE OR DELETE ON notification_invalidations FOR EACH ROW EXECUTE FUNCTION notification_refresh_readiness();

CREATE FUNCTION worker_ready_notification_workspace(after_workspace uuid) RETURNS uuid LANGUAGE sql SECURITY DEFINER AS $$
 SELECT workspace_id FROM notification_ready_workspaces WHERE available_at<=statement_timestamp() AND (after_workspace IS NULL OR workspace_id>after_workspace) ORDER BY workspace_id LIMIT 1
$$;

DO $$ DECLARE f regprocedure;w uuid;r text;previous text;BEGIN
 FOR f IN SELECT p.oid::regprocedure FROM pg_proc p WHERE p.pronamespace=current_schema()::regnamespace AND p.proname IN('notification_refresh_readiness','worker_ready_notification_workspace') LOOP
  EXECUTE format('ALTER FUNCTION %s SET search_path=pg_catalog,%I,pg_temp',f,current_schema());EXECUTE format('REVOKE ALL ON FUNCTION %s FROM PUBLIC',f);
 END LOOP;
 REVOKE ALL ON notification_ready_workspaces FROM PUBLIC;
 -- Default grants and previously provisioned roles must not expose this hint.
 FOR r IN SELECT rolname FROM pg_roles WHERE rolname<>current_user LOOP
  EXECUTE format('REVOKE ALL ON notification_ready_workspaces FROM %I',r);
  EXECUTE format('REVOKE ALL ON FUNCTION notification_refresh_readiness(),worker_ready_notification_workspace(uuid) FROM %I',r);
 END LOOP;
 previous:=current_setting('opentrack.workspace_id',true);
 FOR w IN SELECT workspace_id FROM worker_workspace_registry LOOP
  PERFORM set_config('opentrack.workspace_id',w::text,true);
  INSERT INTO notification_ready_workspaces(workspace_id,available_at) SELECT w,(SELECT available_at FROM notification_invalidations WHERE workspace_id=w AND published_seq IS NULL AND NOT blocked ORDER BY available_at,notification_id,notification_version LIMIT 1);
 END LOOP;
 PERFORM set_config('opentrack.workspace_id',coalesce(previous,''),true);
END $$;
