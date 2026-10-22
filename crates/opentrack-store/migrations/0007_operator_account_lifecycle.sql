-- Installation account authority stays outside the serving credential. Existing
-- memberships, authorship and password hashes survive disable/enable. The new
-- capabilities receive explicit grants only after this migration succeeds.
ALTER TABLE audit_entries ALTER COLUMN actor_id DROP NOT NULL;
ALTER TABLE audit_entries ADD COLUMN actor_kind text NOT NULL DEFAULT 'account';
ALTER TABLE audit_entries ADD COLUMN operator_identity text;
-- Historical account correlations remain the audit row ID. An operator command
-- can affect several workspaces, whose distinct audit IDs share this correlation.
ALTER TABLE audit_entries ADD COLUMN correlation_id uuid;
ALTER TABLE audit_entries ADD CONSTRAINT audit_actor_shape CHECK(
 (actor_kind='account' AND actor_id IS NOT NULL AND operator_identity IS NULL AND correlation_id IS NULL)
 OR (actor_kind='local_operator' AND actor_id IS NULL AND operator_identity IS NOT NULL
     AND length(operator_identity)>0 AND correlation_id IS NOT NULL)
);
ALTER TABLE workspace_events ALTER COLUMN actor_id DROP NOT NULL;
ALTER TABLE workspace_events ADD COLUMN actor_kind text NOT NULL DEFAULT 'account';
ALTER TABLE workspace_events ADD CONSTRAINT workspace_event_actor_shape CHECK(
 (actor_kind='account' AND actor_id IS NOT NULL)
 OR (actor_kind='local_operator' AND actor_id IS NULL AND event_type='access.changed'
     AND team_id IS NULL AND entity_id=workspace_id)
);

ALTER TABLE account_security_events DROP CONSTRAINT account_security_event_shape;
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
 OR
 (action IN ('account.disabled','account.enabled','account.disable_noop','account.enable_noop')
  AND actor_kind='local_operator' AND session_id IS NULL AND related_session_id IS NULL AND recovery_id IS NULL
  AND operator_identity IS NOT NULL AND length(operator_identity)>0
  AND reason IS NOT NULL AND length(btrim(reason)) BETWEEN 1 AND 500 AND reason !~ '[[:cntrl:]]')
);

DO $migration$
DECLARE app_schema text := current_schema(); migration_owner text := current_user;
BEGIN
 -- FORCE RLS remains enabled. Only the migration owner used by the definer
 -- helpers can inspect other organization memberships and workspace metadata.
 -- No private team/content table gains a policy or an operator table grant.
 EXECUTE format('CREATE POLICY migration_owner_metadata ON %I.workspaces FOR SELECT TO %I USING(true)',app_schema,migration_owner);
 EXECUTE format('CREATE POLICY migration_owner_membership_check ON %I.organization_members FOR SELECT TO %I USING(true)',app_schema,migration_owner);

 EXECUTE format($definition$
 CREATE FUNCTION %1$I.account_disabled_authority() RETURNS trigger
 LANGUAGE plpgsql SET search_path=pg_catalog,%1$I,pg_temp AS $body$
 BEGIN
  IF ((TG_OP='INSERT' AND NEW.disabled_at IS NOT NULL)
      OR (TG_OP='UPDATE' AND NEW.disabled_at IS DISTINCT FROM OLD.disabled_at))
     AND current_user<>pg_get_userbyid((SELECT relowner FROM pg_class WHERE oid=TG_RELID)) THEN
   RAISE EXCEPTION 'Global account state requires local operator authority' USING ERRCODE='42501';
  END IF;
  RETURN NEW;
 END $body$;
 $definition$,app_schema);

 EXECUTE format($definition$
 CREATE FUNCTION %1$I.workspace_operator_actor_authority() RETURNS trigger
 LANGUAGE plpgsql SET search_path=pg_catalog,%1$I,pg_temp AS $body$
 BEGIN
  IF NEW.actor_kind='local_operator'
     AND current_user<>pg_get_userbyid((SELECT relowner FROM pg_class WHERE oid=TG_RELID)) THEN
   RAISE EXCEPTION 'Operator attribution requires local operator authority' USING ERRCODE='42501';
  END IF;
  RETURN NEW;
 END $body$;
 $definition$,app_schema);

 EXECUTE format($definition$
 CREATE FUNCTION %1$I.assert_workspace_enabled_owner(p_workspace uuid,p_exclude uuid)
 RETURNS void LANGUAGE plpgsql SECURITY DEFINER
 SET search_path=pg_catalog,%1$I,pg_temp AS $body$
 DECLARE previous_workspace text := current_setting('opentrack.workspace_id',true);
 BEGIN
  PERFORM set_config('opentrack.workspace_id',p_workspace::text,true);
  PERFORM 1 FROM %1$I.workspace_auth_guards g WHERE g.workspace_id=p_workspace FOR UPDATE;
  IF EXISTS(SELECT 1 FROM %1$I.workspaces w WHERE w.id=p_workspace)
     AND NOT EXISTS(SELECT 1 FROM %1$I.workspace_members m JOIN %1$I.users u ON u.id=m.user_id
       WHERE m.workspace_id=p_workspace AND m.active AND m.role='owner' AND u.disabled_at IS NULL
       AND (p_exclude IS NULL OR m.user_id<>p_exclude)) THEN
   RAISE EXCEPTION 'A workspace must retain an active enabled owner'
    USING ERRCODE='23514',CONSTRAINT='enabled_workspace_owner_required';
  END IF;
  PERFORM set_config('opentrack.workspace_id',coalesce(previous_workspace,''),true);
 END $body$;
 $definition$,app_schema);

 EXECUTE format($definition$
 CREATE FUNCTION %1$I.assert_organization_enabled_owner(p_organization uuid,p_exclude uuid)
 RETURNS void LANGUAGE plpgsql SECURITY DEFINER
 SET search_path=pg_catalog,%1$I,pg_temp AS $body$
 BEGIN
  PERFORM 1 FROM %1$I.organizations o WHERE o.id=p_organization FOR UPDATE;
  IF EXISTS(SELECT 1 FROM %1$I.organizations o WHERE o.id=p_organization)
     AND NOT EXISTS(SELECT 1 FROM %1$I.organization_members m JOIN %1$I.users u ON u.id=m.user_id
       WHERE m.organization_id=p_organization AND m.role='owner' AND u.disabled_at IS NULL
       AND (p_exclude IS NULL OR m.user_id<>p_exclude)) THEN
   RAISE EXCEPTION 'An organization must retain an enabled owner'
    USING ERRCODE='23514',CONSTRAINT='enabled_organization_owner_required';
  END IF;
 END $body$;
 $definition$,app_schema);

 -- Deferred membership checks must use their own row's scope, not the final
 -- workspace GUC of a transaction that changed more than one workspace.
 EXECUTE format($definition$
 CREATE OR REPLACE FUNCTION %1$I.enforce_workspace_owner() RETURNS trigger
 LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,%1$I,pg_temp AS $body$
 DECLARE target uuid;
 BEGIN
  FOR target IN SELECT DISTINCT v FROM unnest(ARRAY[
    CASE WHEN TG_OP<>'INSERT' THEN OLD.workspace_id END,
    CASE WHEN TG_OP<>'DELETE' THEN NEW.workspace_id END]) v WHERE v IS NOT NULL ORDER BY v
  LOOP
   PERFORM %1$I.assert_workspace_enabled_owner(target,NULL);
  END LOOP;
  RETURN NULL;
 END $body$;
 $definition$,app_schema);

 EXECUTE format($definition$
 CREATE FUNCTION %1$I.enforce_organization_owner() RETURNS trigger
 LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,%1$I,pg_temp AS $body$
 DECLARE target uuid;
 BEGIN
  FOR target IN SELECT DISTINCT v FROM unnest(ARRAY[
    CASE WHEN TG_OP<>'INSERT' THEN OLD.organization_id END,
    CASE WHEN TG_OP<>'DELETE' THEN NEW.organization_id END]) v WHERE v IS NOT NULL ORDER BY v
  LOOP
   PERFORM %1$I.assert_organization_enabled_owner(target,NULL);
  END LOOP;
  RETURN NULL;
 END $body$;
 $definition$,app_schema);

 EXECUTE format($definition$
 CREATE FUNCTION %1$I.enforce_disabled_account_owners() RETURNS trigger
 LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,%1$I,pg_temp AS $body$
 DECLARE target uuid; previous_user text := current_setting('opentrack.user_id',true);
 BEGIN
  IF NEW.disabled_at IS NULL OR NEW.disabled_at IS NOT DISTINCT FROM OLD.disabled_at THEN
   RETURN NULL;
  END IF;
  PERFORM set_config('opentrack.user_id',NEW.id::text,true);
  FOR target IN SELECT m.organization_id FROM %1$I.organization_members m
    WHERE m.user_id=NEW.id AND m.role='owner' ORDER BY m.organization_id
  LOOP
   PERFORM %1$I.assert_organization_enabled_owner(target,NULL);
  END LOOP;
  FOR target IN SELECT m.workspace_id FROM %1$I.workspace_members m
    WHERE m.user_id=NEW.id AND m.active AND m.role='owner' ORDER BY m.workspace_id
  LOOP
   PERFORM %1$I.assert_workspace_enabled_owner(target,NULL);
  END LOOP;
  PERFORM set_config('opentrack.user_id',coalesce(previous_user,''),true);
  RETURN NULL;
 END $body$;
 $definition$,app_schema);

 EXECUTE format($definition$
 CREATE FUNCTION %1$I.operator_change_account_state(p_user_id uuid,p_disabled boolean,p_reason text)
 RETURNS TABLE(user_id uuid,disabled boolean,changed boolean,disabled_at timestamptz,
   correlation_id uuid,revoked_sessions bigint,revoked_recovery_codes bigint,revoked_invitations bigint)
 LANGUAGE plpgsql SET search_path=pg_catalog,%1$I,pg_temp AS $body$
 DECLARE
  previous_user text := current_setting('opentrack.user_id',true);
  previous_workspace text := current_setting('opentrack.workspace_id',true);
  prior_disabled_at timestamptz; next_disabled_at timestamptz; did_change boolean;
  correlation uuid := gen_random_uuid(); scope_id uuid; workspaces uuid[];
  session_count bigint := 0; recovery_count bigint := 0; invitation_count bigint := 0;
  affected bigint; next_epoch bigint; next_seq bigint; event_action text;
 BEGIN
  IF p_user_id IS NULL OR p_disabled IS NULL OR p_reason IS NULL
     OR length(btrim(p_reason)) NOT BETWEEN 1 AND 500 OR p_reason ~ '[[:cntrl:]]' THEN
   RAISE EXCEPTION 'Invalid account lifecycle request' USING ERRCODE='22023';
  END IF;
  PERFORM set_config('opentrack.user_id',p_user_id::text,true);
  PERFORM set_config('opentrack.workspace_id','',true);
  PERFORM set_config('lock_timeout','1000',true);
  SELECT u.disabled_at INTO prior_disabled_at FROM %1$I.users u WHERE u.id=p_user_id FOR NO KEY UPDATE;
  IF NOT FOUND THEN
   RAISE EXCEPTION 'Account is unavailable' USING ERRCODE='P0002';
  END IF;
  did_change := (prior_disabled_at IS NOT NULL) IS DISTINCT FROM p_disabled;
  next_disabled_at := prior_disabled_at;
  IF did_change THEN
   -- Target account guard makes this scope set stable against grants, workspace
   -- creation and recipient acceptance. Do not lock co-owner users afterwards.
   PERFORM o.id FROM %1$I.organizations o JOIN %1$I.organization_members m ON m.organization_id=o.id
    WHERE m.user_id=p_user_id ORDER BY o.id FOR UPDATE OF o;
   SELECT array_agg(m.workspace_id ORDER BY m.workspace_id) INTO workspaces
    FROM %1$I.workspace_members m WHERE m.user_id=p_user_id;
   FOREACH scope_id IN ARRAY coalesce(workspaces,ARRAY[]::uuid[]) LOOP
    PERFORM set_config('opentrack.workspace_id',scope_id::text,true);
    PERFORM 1 FROM %1$I.workspace_auth_guards g WHERE g.workspace_id=scope_id FOR UPDATE;
   END LOOP;
   IF p_disabled THEN
    FOR scope_id IN SELECT m.organization_id FROM %1$I.organization_members m
      WHERE m.user_id=p_user_id AND m.role='owner' ORDER BY m.organization_id
    LOOP
     PERFORM %1$I.assert_organization_enabled_owner(scope_id,p_user_id);
    END LOOP;
    FOREACH scope_id IN ARRAY coalesce(workspaces,ARRAY[]::uuid[]) LOOP
     PERFORM set_config('opentrack.workspace_id',scope_id::text,true);
     IF EXISTS(SELECT 1 FROM %1$I.workspace_members m WHERE m.workspace_id=scope_id
       AND m.user_id=p_user_id AND m.active AND m.role='owner') THEN
      PERFORM %1$I.assert_workspace_enabled_owner(scope_id,p_user_id);
     END IF;
    END LOOP;
    next_disabled_at := clock_timestamp();
   ELSE
    next_disabled_at := NULL;
   END IF;
   UPDATE %1$I.users u SET disabled_at=next_disabled_at WHERE u.id=p_user_id;
   -- Also fence a legacy disabled account: before 0007, disabled_at could be
   -- changed without cleanup. Enabling must never revive its old credentials.
   WITH revoked AS (
     UPDATE %1$I.sessions s SET revoked_at=clock_timestamp()
      WHERE s.user_id=p_user_id AND s.revoked_at IS NULL RETURNING s.id
    ), audited AS (
     INSERT INTO %1$I.account_security_events(user_id,session_id,action,correlation_id)
      SELECT p_user_id,r.id,'session.revoked',correlation FROM revoked r RETURNING id
    ) SELECT (SELECT count(*) FROM revoked),(SELECT count(*) FROM audited)
       INTO session_count,affected;
   IF session_count<>affected THEN
    RAISE EXCEPTION 'Required session audit was not appended' USING ERRCODE='P0001';
   END IF;
   UPDATE %1$I.recovery_tokens r SET revoked_at=clock_timestamp()
    WHERE r.user_id=p_user_id AND r.used_at IS NULL AND r.revoked_at IS NULL;
   GET DIAGNOSTICS recovery_count=ROW_COUNT;
  END IF;
  event_action := CASE WHEN did_change THEN
    CASE WHEN p_disabled THEN 'account.disabled' ELSE 'account.enabled' END
   ELSE CASE WHEN p_disabled THEN 'account.disable_noop' ELSE 'account.enable_noop' END END;
  INSERT INTO %1$I.account_security_events(user_id,action,actor_kind,operator_identity,reason,correlation_id)
   VALUES(p_user_id,event_action,'local_operator',session_user,btrim(p_reason),correlation);
  GET DIAGNOSTICS affected=ROW_COUNT;
  IF affected<>1 THEN
   RAISE EXCEPTION 'Required operator audit was not appended' USING ERRCODE='P0001';
  END IF;
  IF did_change THEN
   FOREACH scope_id IN ARRAY coalesce(workspaces,ARRAY[]::uuid[]) LOOP
    PERFORM set_config('opentrack.workspace_id',scope_id::text,true);
    UPDATE %1$I.invitations i SET revoked_at=clock_timestamp(),version=i.version+1
     WHERE i.workspace_id=scope_id AND i.invited_by=p_user_id
      AND i.accepted_at IS NULL AND i.revoked_at IS NULL;
    GET DIAGNOSTICS affected=ROW_COUNT;
    invitation_count := invitation_count+affected;
    UPDATE %1$I.workspace_auth_guards g SET epoch=g.epoch+1
     WHERE g.workspace_id=scope_id RETURNING g.epoch INTO next_epoch;
    IF NOT FOUND THEN
     RAISE EXCEPTION 'Required workspace guard is missing' USING ERRCODE='P0001';
    END IF;
    INSERT INTO %1$I.audit_entries(id,workspace_id,actor_id,actor_kind,operator_identity,action,target_id,correlation_id)
     VALUES(gen_random_uuid(),scope_id,NULL,'local_operator',session_user,event_action,p_user_id,correlation);
    GET DIAGNOSTICS affected=ROW_COUNT;
    IF affected<>1 THEN
     RAISE EXCEPTION 'Required workspace audit was not appended' USING ERRCODE='P0001';
    END IF;
   END LOOP;
   -- Complete entity/epoch/audit effects in every scope before taking any stream
   -- head. Multi-workspace commands use the same global lock order as ordinary
   -- writes, with stream heads last and all workspace orders deterministic.
   FOREACH scope_id IN ARRAY coalesce(workspaces,ARRAY[]::uuid[]) LOOP
    PERFORM set_config('opentrack.workspace_id',scope_id::text,true);
    SELECT g.epoch INTO next_epoch FROM %1$I.workspace_auth_guards g WHERE g.workspace_id=scope_id;
    UPDATE %1$I.workspace_stream_heads h SET last_seq=h.last_seq+1
     WHERE h.workspace_id=scope_id RETURNING h.last_seq INTO next_seq;
    IF NOT FOUND THEN
     RAISE EXCEPTION 'Required workspace stream head is missing' USING ERRCODE='P0001';
    END IF;
    INSERT INTO %1$I.workspace_events(workspace_id,seq,event_id,event_type,team_id,entity_id,
      entity_version,actor_id,actor_kind,mutation_id)
     VALUES(scope_id,next_seq,gen_random_uuid(),'access.changed',NULL,scope_id,
      next_epoch,NULL,'local_operator',correlation);
    GET DIAGNOSTICS affected=ROW_COUNT;
    IF affected<>1 THEN
     RAISE EXCEPTION 'Required workspace event was not appended' USING ERRCODE='P0001';
    END IF;
   END LOOP;
  END IF;
  PERFORM set_config('opentrack.user_id',coalesce(previous_user,''),true);
  PERFORM set_config('opentrack.workspace_id',coalesce(previous_workspace,''),true);
  RETURN QUERY SELECT p_user_id,p_disabled,did_change,next_disabled_at,correlation,
    session_count,recovery_count,invitation_count;
 END $body$;
 $definition$,app_schema);

 EXECUTE format($definition$
 CREATE FUNCTION %1$I.operator_disable_account(p_user_id uuid,p_reason text)
 RETURNS TABLE(user_id uuid,disabled boolean,changed boolean,disabled_at timestamptz,
   correlation_id uuid,revoked_sessions bigint,revoked_recovery_codes bigint,revoked_invitations bigint)
 LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,%1$I,pg_temp AS $body$
  SELECT * FROM %1$I.operator_change_account_state(p_user_id,true,p_reason)
 $body$;
 $definition$,app_schema);
 EXECUTE format($definition$
 CREATE FUNCTION %1$I.operator_enable_account(p_user_id uuid,p_reason text)
 RETURNS TABLE(user_id uuid,disabled boolean,changed boolean,disabled_at timestamptz,
   correlation_id uuid,revoked_sessions bigint,revoked_recovery_codes bigint,revoked_invitations bigint)
 LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,%1$I,pg_temp AS $body$
  SELECT * FROM %1$I.operator_change_account_state(p_user_id,false,p_reason)
 $body$;
 $definition$,app_schema);
END $migration$;

CREATE TRIGGER account_disabled_authority BEFORE INSERT OR UPDATE ON users
 FOR EACH ROW EXECUTE FUNCTION account_disabled_authority();
CREATE TRIGGER audit_operator_actor BEFORE INSERT OR UPDATE ON audit_entries
 FOR EACH ROW EXECUTE FUNCTION workspace_operator_actor_authority();
CREATE TRIGGER event_operator_actor BEFORE INSERT OR UPDATE ON workspace_events
 FOR EACH ROW EXECUTE FUNCTION workspace_operator_actor_authority();
CREATE CONSTRAINT TRIGGER organization_retains_enabled_owner AFTER INSERT OR UPDATE OR DELETE ON organization_members
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION enforce_organization_owner();
CREATE CONSTRAINT TRIGGER disabled_account_retains_owners AFTER UPDATE ON users
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION enforce_disabled_account_owners();

REVOKE ALL ON FUNCTION operator_disable_account(uuid,text) FROM PUBLIC;
REVOKE ALL ON FUNCTION operator_enable_account(uuid,text) FROM PUBLIC;
REVOKE ALL ON FUNCTION operator_change_account_state(uuid,boolean,text) FROM PUBLIC;
REVOKE ALL ON FUNCTION assert_workspace_enabled_owner(uuid,uuid) FROM PUBLIC;
REVOKE ALL ON FUNCTION assert_organization_enabled_owner(uuid,uuid) FROM PUBLIC;
REVOKE ALL ON FUNCTION enforce_workspace_owner() FROM PUBLIC;
REVOKE ALL ON FUNCTION enforce_organization_owner() FROM PUBLIC;
REVOKE ALL ON FUNCTION enforce_disabled_account_owners() FROM PUBLIC;
REVOKE ALL ON FUNCTION account_disabled_authority() FROM PUBLIC;
REVOKE ALL ON FUNCTION workspace_operator_actor_authority() FROM PUBLIC;

-- Refuse pre-existing stranded ownership rather than selecting a new owner or
-- silently enabling an account. Repair deliberately with the migration operator.
DO $validate$
DECLARE scope_id uuid;
BEGIN
 FOR scope_id IN SELECT id FROM organizations ORDER BY id LOOP
  PERFORM assert_organization_enabled_owner(scope_id,NULL);
 END LOOP;
 FOR scope_id IN SELECT id FROM workspaces ORDER BY id LOOP
  PERFORM assert_workspace_enabled_owner(scope_id,NULL);
 END LOOP;
END $validate$;
