-- Public session identifiers are unrelated to bearer-token hashes.
ALTER TABLE sessions ADD COLUMN id uuid NOT NULL DEFAULT gen_random_uuid();
ALTER TABLE sessions ADD CONSTRAINT sessions_id_unique UNIQUE(id);
ALTER TABLE sessions ADD CONSTRAINT sessions_user_id_unique UNIQUE(user_id,id);
CREATE INDEX sessions_active_directory ON sessions(user_id,created_at DESC,id DESC) WHERE revoked_at IS NULL;

-- Account security history has no workspace dependency and carries no credentials.
CREATE TABLE account_security_events (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
 user_id uuid NOT NULL REFERENCES users(id),
 session_id uuid NOT NULL,
 related_session_id uuid,
 action text NOT NULL CHECK(action IN ('session.created','session.revoked','session.logged_out','session.login_rotated','session.invitation_rotated')),
 created_at timestamptz NOT NULL DEFAULT now()
);
-- Audit stores immutable public IDs, not credentials or durable FKs to sessions.
-- Ownership is checked when appending. A future expired-session purge can remove
-- credential rows without deleting history or retaining bearer/CSRF data forever.
CREATE FUNCTION account_security_session_owner() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF NOT EXISTS(SELECT 1 FROM sessions WHERE user_id=NEW.user_id AND id=NEW.session_id)
  OR (NEW.related_session_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM sessions WHERE user_id=NEW.user_id AND id=NEW.related_session_id)) THEN
  RAISE EXCEPTION 'Security event session must belong to its account' USING ERRCODE='23503';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER account_security_owner BEFORE INSERT ON account_security_events
 FOR EACH ROW EXECUTE FUNCTION account_security_session_owner();
CREATE INDEX account_security_events_owner ON account_security_events(user_id,created_at DESC,id DESC);
ALTER TABLE account_security_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE account_security_events FORCE ROW LEVEL SECURITY;
CREATE POLICY own_account_security ON account_security_events
 USING(user_id=nullif(current_setting('opentrack.user_id',true),'')::uuid)
 WITH CHECK(user_id=nullif(current_setting('opentrack.user_id',true),'')::uuid);
CREATE TRIGGER account_security_no_rewrite BEFORE UPDATE OR DELETE ON account_security_events
 FOR EACH ROW EXECUTE FUNCTION audit_append_only();
CREATE TRIGGER account_security_no_truncate BEFORE TRUNCATE ON account_security_events
 FOR EACH STATEMENT EXECUTE FUNCTION audit_append_only();
DO $$ DECLARE grant_role text; BEGIN
 FOR grant_role IN SELECT DISTINCT grantee FROM information_schema.role_table_grants
  WHERE table_schema=current_schema() AND table_name='account_security_events'
  AND privilege_type IN ('UPDATE','DELETE','TRUNCATE') AND grantee<>current_user
 LOOP
  EXECUTE format('REVOKE UPDATE, DELETE, TRUNCATE ON account_security_events FROM %I',grant_role);
 END LOOP;
END $$;
