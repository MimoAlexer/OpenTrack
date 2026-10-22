-- Recovery authority belongs to a separately provisioned local database login.
-- This migration grants no operator role automatically. Provisioning grants only
-- the two functions below, never table privileges or membership in their owner.
CREATE TABLE recovery_tokens (
 id uuid PRIMARY KEY,
 user_id uuid NOT NULL REFERENCES users(id),
 purpose text NOT NULL DEFAULT 'password_reset' CHECK(purpose='password_reset'),
 token_hash text NOT NULL UNIQUE CHECK(token_hash ~ '^[0-9a-f]{64}$'),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 expires_at timestamptz NOT NULL,
 used_at timestamptz,
 revoked_at timestamptz,
 CHECK(expires_at>created_at),
 CHECK(used_at IS NULL OR revoked_at IS NULL)
);
CREATE UNIQUE INDEX recovery_one_pending ON recovery_tokens(user_id)
 WHERE used_at IS NULL AND revoked_at IS NULL;
CREATE INDEX recovery_expiry ON recovery_tokens(expires_at);
ALTER TABLE recovery_tokens ENABLE ROW LEVEL SECURITY;
ALTER TABLE recovery_tokens FORCE ROW LEVEL SECURITY;
CREATE POLICY own_recovery ON recovery_tokens
 USING(user_id=nullif(current_setting('opentrack.user_id',true),'')::uuid)
 WITH CHECK(user_id=nullif(current_setting('opentrack.user_id',true),'')::uuid);

ALTER TABLE account_security_events ALTER COLUMN session_id DROP NOT NULL;
ALTER TABLE account_security_events DROP CONSTRAINT account_security_events_action_check;
ALTER TABLE account_security_events ADD COLUMN actor_kind text NOT NULL DEFAULT 'account';
ALTER TABLE account_security_events ADD COLUMN recovery_id uuid;
ALTER TABLE account_security_events ADD COLUMN operator_identity text;
ALTER TABLE account_security_events ADD COLUMN reason text;
ALTER TABLE account_security_events ADD COLUMN correlation_id uuid NOT NULL DEFAULT gen_random_uuid();
ALTER TABLE account_security_events ADD CONSTRAINT account_security_event_shape CHECK(
 (action IN ('session.created','session.revoked','session.logged_out','session.login_rotated','session.invitation_rotated')
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
);

-- Freeze paths to the actual migration schema, including isolated test schemas.
-- Only migration-owned identifiers enter format(); no caller-controlled SQL is
-- ever executed. All table references are schema-qualified, pg_temp is last.
DO $migration$
DECLARE app_schema text := current_schema();
BEGIN
 EXECUTE format($definition$
 CREATE OR REPLACE FUNCTION %1$I.account_security_session_owner() RETURNS trigger
 LANGUAGE plpgsql SET search_path=pg_catalog,%1$I,pg_temp AS $body$
 BEGIN
  IF NEW.actor_kind='local_operator' AND
   current_user<>pg_get_userbyid((SELECT relowner FROM pg_class WHERE oid=TG_RELID)) THEN
   RAISE EXCEPTION 'Operator audit requires local operator authority' USING ERRCODE='42501';
  END IF;
  IF (NEW.session_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM %1$I.sessions WHERE user_id=NEW.user_id AND id=NEW.session_id))
   OR (NEW.related_session_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM %1$I.sessions WHERE user_id=NEW.user_id AND id=NEW.related_session_id))
   OR (NEW.recovery_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM %1$I.recovery_tokens WHERE user_id=NEW.user_id AND id=NEW.recovery_id)) THEN
   RAISE EXCEPTION 'Security event reference must belong to its account' USING ERRCODE='23503';
  END IF;
  RETURN NEW;
 END $body$;
 $definition$, app_schema);

 -- A runtime role cannot manufacture a code by inserting a hash directly or
 -- replace/revive a stored bearer. Consumption/revocation remain app mutations.
 EXECUTE format($definition$
 CREATE FUNCTION %1$I.recovery_token_authority() RETURNS trigger
 LANGUAGE plpgsql SET search_path=pg_catalog,%1$I,pg_temp AS $body$
 BEGIN
  IF current_user=pg_get_userbyid((SELECT relowner FROM pg_class WHERE oid=TG_RELID)) THEN
   RETURN NEW;
  END IF;
  IF TG_OP='INSERT' THEN
   RAISE EXCEPTION 'Recovery issuance requires local operator authority' USING ERRCODE='42501';
  END IF;
  IF (NEW.id,NEW.user_id,NEW.purpose,NEW.token_hash,NEW.created_at,NEW.expires_at)
      IS DISTINCT FROM (OLD.id,OLD.user_id,OLD.purpose,OLD.token_hash,OLD.created_at,OLD.expires_at)
   OR (OLD.used_at IS NOT NULL AND NEW.used_at IS DISTINCT FROM OLD.used_at)
   OR (OLD.revoked_at IS NOT NULL AND NEW.revoked_at IS DISTINCT FROM OLD.revoked_at) THEN
   RAISE EXCEPTION 'Recovery credentials and terminal state are immutable' USING ERRCODE='42501';
  END IF;
  RETURN NEW;
 END $body$;
 $definition$, app_schema);

 EXECUTE format($definition$
 CREATE FUNCTION %1$I.operator_issue_recovery(
  p_user_id uuid,p_receipt_id uuid,p_token_hash text,p_ttl_minutes integer,p_reason text
 ) RETURNS timestamptz LANGUAGE plpgsql SECURITY DEFINER
 SET search_path=pg_catalog,%1$I,pg_temp AS $body$
 DECLARE expiry timestamptz;
 BEGIN
  IF p_user_id IS NULL OR p_receipt_id IS NULL OR p_token_hash IS NULL OR p_token_hash !~ '^[0-9a-f]{64}$'
   OR p_ttl_minutes IS NULL OR p_ttl_minutes NOT BETWEEN 5 AND 60
   OR p_reason IS NULL OR length(btrim(p_reason)) NOT BETWEEN 1 AND 500 OR p_reason ~ '[[:cntrl:]]' THEN
   RAISE EXCEPTION 'Invalid recovery request' USING ERRCODE='22023';
  END IF;
  PERFORM set_config('opentrack.user_id',p_user_id::text,true);
  PERFORM set_config('opentrack.workspace_id','',true);
  PERFORM set_config('lock_timeout','1000',true);
  PERFORM id FROM %1$I.users WHERE id=p_user_id AND disabled_at IS NULL FOR NO KEY UPDATE;
  IF NOT FOUND THEN
   RAISE EXCEPTION 'Recovery target is unavailable' USING ERRCODE='P0002';
  END IF;
  -- A repeated receipt fails rather than silently reactivating a delivered code.
  -- Any failure rolls back the preceding invalidation and mandatory audit.
  UPDATE %1$I.recovery_tokens SET revoked_at=clock_timestamp()
   WHERE user_id=p_user_id AND used_at IS NULL AND revoked_at IS NULL;
  expiry := clock_timestamp()+make_interval(mins=>p_ttl_minutes);
  INSERT INTO %1$I.recovery_tokens(id,user_id,token_hash,expires_at)
   VALUES(p_receipt_id,p_user_id,p_token_hash,expiry);
  INSERT INTO %1$I.account_security_events(user_id,action,actor_kind,recovery_id,operator_identity,reason,correlation_id)
   VALUES(p_user_id,'recovery.issued','local_operator',p_receipt_id,session_user,btrim(p_reason),p_receipt_id);
  RETURN expiry;
 END $body$;
 $definition$, app_schema);

 EXECUTE format($definition$
 CREATE FUNCTION %1$I.operator_revoke_recovery(p_user_id uuid,p_reason text)
 RETURNS integer LANGUAGE plpgsql SECURITY DEFINER
 SET search_path=pg_catalog,%1$I,pg_temp AS $body$
 DECLARE affected integer; receipt uuid;
 BEGIN
  IF p_user_id IS NULL OR p_reason IS NULL OR length(btrim(p_reason)) NOT BETWEEN 1 AND 500 OR p_reason ~ '[[:cntrl:]]' THEN
   RAISE EXCEPTION 'Invalid recovery request' USING ERRCODE='22023';
  END IF;
  PERFORM set_config('opentrack.user_id',p_user_id::text,true);
  PERFORM set_config('opentrack.workspace_id','',true);
  PERFORM set_config('lock_timeout','1000',true);
  PERFORM id FROM %1$I.users WHERE id=p_user_id FOR NO KEY UPDATE;
  IF NOT FOUND THEN
   RAISE EXCEPTION 'Recovery target is unavailable' USING ERRCODE='P0002';
  END IF;
  UPDATE %1$I.recovery_tokens SET revoked_at=clock_timestamp()
   WHERE user_id=p_user_id AND used_at IS NULL AND revoked_at IS NULL RETURNING id INTO receipt;
  GET DIAGNOSTICS affected=ROW_COUNT;
  -- Even an idempotent zero-row operator action records its identity and reason.
  INSERT INTO %1$I.account_security_events(user_id,action,actor_kind,recovery_id,operator_identity,reason)
   VALUES(p_user_id,'recovery.revoked','local_operator',receipt,session_user,btrim(p_reason));
  RETURN affected;
 END $body$;
 $definition$, app_schema);
END $migration$;
CREATE TRIGGER recovery_token_authority BEFORE INSERT OR UPDATE ON recovery_tokens
 FOR EACH ROW EXECUTE FUNCTION recovery_token_authority();
REVOKE ALL ON FUNCTION operator_issue_recovery(uuid,uuid,text,integer,text) FROM PUBLIC;
REVOKE ALL ON FUNCTION operator_revoke_recovery(uuid,text) FROM PUBLIC;

-- Recovery audit stores immutable public receipt IDs without a foreign key, so
-- future retention can erase used/expired hashes without deleting audit history.
-- No retention worker is introduced here; old hashes are not bearer credentials.
