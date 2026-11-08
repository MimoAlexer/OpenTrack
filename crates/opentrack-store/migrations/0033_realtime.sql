-- Notifications are content-free hints. Independent API-node cursors and the
-- durable log remain authoritative when listeners disconnect or hints are lost.
CREATE FUNCTION realtime_wakeup() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 PERFORM pg_notify('opentrack_realtime','');
 RETURN NULL;
END $$;
CREATE TRIGGER realtime_wakeup AFTER INSERT ON workspace_events
 FOR EACH STATEMENT EXECUTE FUNCTION realtime_wakeup();

-- Account-owned operational leases enforce connection limits across API nodes.
-- They contain public session IDs, never bearer tokens, payloads or cursors.
CREATE TABLE realtime_stream_leases (
 id uuid PRIMARY KEY,
 workspace_id uuid NOT NULL,
 user_id uuid NOT NULL,
 session_id uuid NOT NULL,
 expires_at timestamptz NOT NULL,
 FOREIGN KEY(workspace_id,user_id) REFERENCES workspace_members(workspace_id,user_id),
 FOREIGN KEY(user_id,session_id) REFERENCES sessions(user_id,id) ON DELETE CASCADE
);
CREATE INDEX realtime_lease_account_expiry ON realtime_stream_leases(user_id,expires_at);
CREATE INDEX realtime_lease_session_expiry ON realtime_stream_leases(session_id,expires_at);
ALTER TABLE realtime_stream_leases ENABLE ROW LEVEL SECURITY;
ALTER TABLE realtime_stream_leases FORCE ROW LEVEL SECURITY;
-- Counting one's own leases spans workspaces; insertion/renewal additionally
-- requires the explicitly authorized current workspace context.
CREATE POLICY realtime_account_scope ON realtime_stream_leases
 USING(user_id=nullif(current_setting('opentrack.user_id',true),'')::uuid)
 WITH CHECK(user_id=nullif(current_setting('opentrack.user_id',true),'')::uuid
   AND workspace_id=nullif(current_setting('opentrack.workspace_id',true),'')::uuid);
DO $$ DECLARE role_name text; BEGIN
 FOR role_name IN SELECT DISTINCT grantee FROM information_schema.role_table_grants
 WHERE table_schema=current_schema() AND table_name='issues' AND grantee<>current_user LOOP
  EXECUTE format('GRANT SELECT,INSERT,DELETE ON realtime_stream_leases TO %I',role_name);
  EXECUTE format('REVOKE UPDATE ON realtime_stream_leases FROM %I',role_name);
  EXECUTE format('GRANT UPDATE(expires_at) ON realtime_stream_leases TO %I',role_name);
 END LOOP;
END $$;
