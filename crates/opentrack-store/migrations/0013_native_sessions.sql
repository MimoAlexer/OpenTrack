-- Native credentials reuse account revocation/guard semantics while having a
-- distinct audience. Existing rows remain web credentials without rotation.
ALTER TABLE sessions ADD COLUMN audience text NOT NULL DEFAULT 'web';
ALTER TABLE sessions ADD COLUMN device_name text;
ALTER TABLE sessions ADD CONSTRAINT session_audience_shape CHECK(
 (audience='web' AND device_name IS NULL)
 OR (audience='desktop' AND device_name IS NOT NULL
     AND length(btrim(device_name)) BETWEEN 1 AND 80 AND device_name !~ '[[:cntrl:]]'
     AND csrf_token='')
);

-- This is global identity ingress, like sessions, not workspace-owned content.
-- Anonymous requests have no account scope until browser approval. Only the
-- runtime service accesses these rows; worker/operator receive no table grants.
CREATE TABLE native_auth_admission (
 singleton boolean PRIMARY KEY DEFAULT true CHECK(singleton)
);
INSERT INTO native_auth_admission(singleton) VALUES(true);
CREATE TABLE native_auth_requests (
 id uuid PRIMARY KEY,
 code_challenge text NOT NULL CHECK(code_challenge ~ '^[A-Za-z0-9_-]{43}$'),
 callback_uri text NOT NULL CHECK(length(callback_uri)<=256),
 state text NOT NULL CHECK(state ~ '^[0-9a-f]{64}$'),
 confirmation_code text NOT NULL CHECK(confirmation_code ~ '^[ABCDEFGHJKLMNPQRSTUVWXYZ23456789]{8}$'),
 device_name text NOT NULL CHECK(length(btrim(device_name)) BETWEEN 1 AND 80 AND device_name !~ '[[:cntrl:]]'),
 created_at timestamptz NOT NULL,
 expires_at timestamptz NOT NULL CHECK(expires_at=created_at+interval '10 minutes'),
 bound_user_id uuid REFERENCES users(id),
 previous_native_session_id uuid,
 approved_user_id uuid REFERENCES users(id),
 approving_web_session_id uuid,
 approved_at timestamptz,
 code_hash text CHECK(code_hash ~ '^[0-9a-f]{64}$'),
 code_expires_at timestamptz,
 consumed_at timestamptz,
 issued_session_id uuid,
 CHECK((bound_user_id IS NULL)=(previous_native_session_id IS NULL)),
 CHECK(bound_user_id IS NULL OR approved_user_id IS NULL OR bound_user_id=approved_user_id),
 CHECK(
  (approved_user_id IS NULL AND approving_web_session_id IS NULL AND approved_at IS NULL
   AND code_hash IS NULL AND code_expires_at IS NULL AND consumed_at IS NULL AND issued_session_id IS NULL)
  OR
  (approved_user_id IS NOT NULL AND approving_web_session_id IS NOT NULL AND approved_at IS NOT NULL
   AND approved_at>=created_at AND approved_at<expires_at
   AND code_hash IS NOT NULL AND code_expires_at IS NOT NULL
   AND code_expires_at>approved_at AND code_expires_at<=approved_at+interval '60 seconds'
   AND code_expires_at<=expires_at
   AND ((consumed_at IS NULL AND issued_session_id IS NULL)
        OR (consumed_at IS NOT NULL AND issued_session_id IS NOT NULL
            AND consumed_at>=approved_at AND consumed_at<code_expires_at)))
 )
);
CREATE INDEX native_auth_request_expiry ON native_auth_requests(expires_at,id);

ALTER TABLE account_security_events DROP CONSTRAINT account_security_event_shape;
ALTER TABLE account_security_events ADD CONSTRAINT account_security_event_shape CHECK(
 (action IN ('session.created','session.revoked','session.logged_out','session.login_rotated','session.invitation_rotated',
             'session.native_authorized','session.native_exchanged','session.native_rotated')
  AND actor_kind='account' AND session_id IS NOT NULL AND recovery_id IS NULL
  AND operator_identity IS NULL AND reason IS NULL)
 OR
 (action IN ('recovery.issued','recovery.revoked') AND actor_kind='local_operator'
  AND session_id IS NULL AND related_session_id IS NULL
  AND operator_identity IS NOT NULL AND length(operator_identity)>0
  AND reason IS NOT NULL AND length(btrim(reason)) BETWEEN 1 AND 500 AND reason !~ '[[:cntrl:]]'
  AND (action='recovery.revoked' OR recovery_id IS NOT NULL))
 OR
 (action='recovery.completed' AND actor_kind='recovery' AND recovery_id IS NOT NULL
  AND session_id IS NULL AND related_session_id IS NULL AND operator_identity IS NULL AND reason IS NULL)
 OR
 (action IN ('account.disabled','account.enabled','account.disable_noop','account.enable_noop')
  AND actor_kind='local_operator' AND session_id IS NULL AND related_session_id IS NULL AND recovery_id IS NULL
  AND operator_identity IS NOT NULL AND length(operator_identity)>0
  AND reason IS NOT NULL AND length(btrim(reason)) BETWEEN 1 AND 500 AND reason !~ '[[:cntrl:]]')
);

CREATE FUNCTION session_audience_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF (NEW.audience,NEW.device_name) IS DISTINCT FROM (OLD.audience,OLD.device_name) THEN
  RAISE EXCEPTION 'Session audience and device are immutable' USING ERRCODE='23514';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER sessions_audience_immutable BEFORE UPDATE ON sessions
 FOR EACH ROW EXECUTE FUNCTION session_audience_immutable();

CREATE FUNCTION native_auth_request_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP='DELETE' THEN
  IF OLD.expires_at>clock_timestamp() THEN
   RAISE EXCEPTION 'Only expired native requests may be removed' USING ERRCODE='23514';
  END IF;
  RETURN OLD;
 END IF;
 IF TG_OP='INSERT' THEN
  IF NEW.approved_at IS NOT NULL THEN
   RAISE EXCEPTION 'Native requests begin pending' USING ERRCODE='23514';
  END IF;
 ELSE
  IF (NEW.id,NEW.code_challenge,NEW.callback_uri,NEW.state,NEW.confirmation_code,NEW.device_name,
      NEW.created_at,NEW.expires_at,NEW.bound_user_id,NEW.previous_native_session_id)
     IS DISTINCT FROM
     (OLD.id,OLD.code_challenge,OLD.callback_uri,OLD.state,OLD.confirmation_code,OLD.device_name,
      OLD.created_at,OLD.expires_at,OLD.bound_user_id,OLD.previous_native_session_id)
     OR OLD.consumed_at IS NOT NULL
     OR (OLD.approved_at IS NULL AND (NEW.approved_at IS NULL OR NEW.consumed_at IS NOT NULL))
     OR (OLD.approved_at IS NOT NULL AND
         ((NEW.approved_user_id,NEW.approving_web_session_id,NEW.approved_at,NEW.code_hash,NEW.code_expires_at)
          IS DISTINCT FROM
          (OLD.approved_user_id,OLD.approving_web_session_id,OLD.approved_at,OLD.code_hash,OLD.code_expires_at)
          OR NEW.consumed_at IS NULL)) THEN
   RAISE EXCEPTION 'Invalid native request transition' USING ERRCODE='23514';
  END IF;
 END IF;
 IF (NEW.previous_native_session_id IS NOT NULL AND NOT EXISTS(
       SELECT 1 FROM sessions WHERE id=NEW.previous_native_session_id AND user_id=NEW.bound_user_id AND audience='desktop'))
 OR (NEW.approving_web_session_id IS NOT NULL AND NOT EXISTS(
       SELECT 1 FROM sessions WHERE id=NEW.approving_web_session_id AND user_id=NEW.approved_user_id AND audience='web'))
 OR (NEW.issued_session_id IS NOT NULL AND NOT EXISTS(
       SELECT 1 FROM sessions WHERE id=NEW.issued_session_id AND user_id=NEW.approved_user_id AND audience='desktop')) THEN
  RAISE EXCEPTION 'Native request session reference is invalid' USING ERRCODE='23503';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER native_auth_request_guard BEFORE INSERT OR UPDATE OR DELETE ON native_auth_requests
 FOR EACH ROW EXECUTE FUNCTION native_auth_request_guard();
DO $$ DECLARE f regprocedure; BEGIN
 FOREACH f IN ARRAY ARRAY['session_audience_immutable()'::regprocedure,'native_auth_request_guard()'::regprocedure] LOOP
  EXECUTE format('ALTER FUNCTION %s SET search_path TO pg_catalog,%I,pg_temp',f,current_schema());
  EXECUTE format('REVOKE ALL ON FUNCTION %s FROM PUBLIC',f);
 END LOOP;
END $$;
