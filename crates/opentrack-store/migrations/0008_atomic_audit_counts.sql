-- Replace critical credential/account writes and mandatory audits without changing
-- signatures, privileges, lock order, lifetime, attribution or receipt semantics.
-- Existing grants survive CREATE OR REPLACE; PUBLIC execution remains forbidden.
DO $migration$
DECLARE app_schema text := current_schema();
BEGIN
 EXECUTE format($definition$
 CREATE OR REPLACE FUNCTION %1$I.operator_issue_recovery(
  p_user_id uuid,p_receipt_id uuid,p_token_hash text,p_ttl_minutes integer,p_reason text
 ) RETURNS timestamptz LANGUAGE plpgsql SECURITY DEFINER
 SET search_path=pg_catalog,%1$I,pg_temp AS $body$
 DECLARE expiry timestamptz; expected integer; affected integer;
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
  -- All recovery mutations hold this account guard. A suppressed retirement must
  -- not be confused with there being no pending code; at most one can be pending.
  SELECT count(*) INTO expected FROM %1$I.recovery_tokens
   WHERE user_id=p_user_id AND used_at IS NULL AND revoked_at IS NULL;
  UPDATE %1$I.recovery_tokens SET revoked_at=clock_timestamp()
   WHERE user_id=p_user_id AND used_at IS NULL AND revoked_at IS NULL;
  GET DIAGNOSTICS affected=ROW_COUNT;
  IF affected<>expected THEN
   RAISE EXCEPTION 'Required recovery retirement did not complete' USING ERRCODE='P0001';
  END IF;
  expiry := clock_timestamp()+make_interval(mins=>p_ttl_minutes);
  INSERT INTO %1$I.recovery_tokens(id,user_id,token_hash,expires_at)
   VALUES(p_receipt_id,p_user_id,p_token_hash,expiry);
  GET DIAGNOSTICS affected=ROW_COUNT;
  IF affected<>1 THEN
   RAISE EXCEPTION 'Required recovery credential was not created' USING ERRCODE='P0001';
  END IF;
  INSERT INTO %1$I.account_security_events(user_id,action,actor_kind,recovery_id,operator_identity,reason,correlation_id)
   VALUES(p_user_id,'recovery.issued','local_operator',p_receipt_id,session_user,btrim(p_reason),p_receipt_id);
  GET DIAGNOSTICS affected=ROW_COUNT;
  IF affected<>1 THEN
   RAISE EXCEPTION 'Required recovery issuance audit was not appended' USING ERRCODE='P0001';
  END IF;
  RETURN expiry;
 END $body$;
 $definition$,app_schema);

 EXECUTE format($definition$
 CREATE OR REPLACE FUNCTION %1$I.operator_revoke_recovery(p_user_id uuid,p_reason text)
 RETURNS integer LANGUAGE plpgsql SECURITY DEFINER
 SET search_path=pg_catalog,%1$I,pg_temp AS $body$
 DECLARE expected integer; affected integer; appended integer; receipt uuid;
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
  SELECT count(*) INTO expected FROM %1$I.recovery_tokens
   WHERE user_id=p_user_id AND used_at IS NULL AND revoked_at IS NULL;
  UPDATE %1$I.recovery_tokens SET revoked_at=clock_timestamp()
   WHERE user_id=p_user_id AND used_at IS NULL AND revoked_at IS NULL RETURNING id INTO receipt;
  GET DIAGNOSTICS affected=ROW_COUNT;
  IF affected<>expected THEN
   RAISE EXCEPTION 'Required recovery revocation did not complete' USING ERRCODE='P0001';
  END IF;
  -- A zero-candidate request still appends its mandatory operator audit.
  INSERT INTO %1$I.account_security_events(user_id,action,actor_kind,recovery_id,operator_identity,reason)
   VALUES(p_user_id,'recovery.revoked','local_operator',receipt,session_user,btrim(p_reason));
  GET DIAGNOSTICS appended=ROW_COUNT;
  IF appended<>1 THEN
   RAISE EXCEPTION 'Required recovery revocation audit was not appended' USING ERRCODE='P0001';
  END IF;
  RETURN affected;
 END $body$;
 $definition$,app_schema);

 EXECUTE format($definition$
 CREATE OR REPLACE FUNCTION %1$I.operator_change_account_state(p_user_id uuid,p_disabled boolean,p_reason text)
 RETURNS TABLE(user_id uuid,disabled boolean,changed boolean,disabled_at timestamptz,
   correlation_id uuid,revoked_sessions bigint,revoked_recovery_codes bigint,revoked_invitations bigint)
 LANGUAGE plpgsql SET search_path=pg_catalog,%1$I,pg_temp AS $body$
 DECLARE
  previous_user text := current_setting('opentrack.user_id',true);
  previous_workspace text := current_setting('opentrack.workspace_id',true);
  prior_disabled_at timestamptz; next_disabled_at timestamptz; did_change boolean;
  correlation uuid := gen_random_uuid(); scope_id uuid; workspaces uuid[];
  session_count bigint := 0; recovery_count bigint := 0; invitation_count bigint := 0;
  expected bigint; affected bigint; next_epoch bigint; next_seq bigint; event_action text;
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
   GET DIAGNOSTICS affected=ROW_COUNT;
   IF affected<>1 THEN
    RAISE EXCEPTION 'Required account state change did not complete' USING ERRCODE='P0001';
   END IF;
   -- Also fence a legacy disabled account: before 0007, disabled_at could be
   -- changed without cleanup. Enabling must never revive its old credentials.
   WITH eligible AS MATERIALIZED (
     SELECT s.id FROM %1$I.sessions s WHERE s.user_id=p_user_id AND s.revoked_at IS NULL
    ), revoked AS (
     UPDATE %1$I.sessions s SET revoked_at=clock_timestamp() FROM eligible e
      WHERE s.id=e.id RETURNING s.id
    ), audited AS (
     INSERT INTO %1$I.account_security_events(user_id,session_id,action,correlation_id)
      SELECT p_user_id,r.id,'session.revoked',correlation FROM revoked r RETURNING id
    ) SELECT (SELECT count(*) FROM eligible),(SELECT count(*) FROM revoked),(SELECT count(*) FROM audited)
       INTO expected,session_count,affected;
   IF expected<>session_count OR session_count<>affected THEN
    RAISE EXCEPTION 'Required session revocation or audit did not complete' USING ERRCODE='P0001';
   END IF;
   SELECT count(*) INTO expected FROM %1$I.recovery_tokens r
    WHERE r.user_id=p_user_id AND r.used_at IS NULL AND r.revoked_at IS NULL;
   UPDATE %1$I.recovery_tokens r SET revoked_at=clock_timestamp()
    WHERE r.user_id=p_user_id AND r.used_at IS NULL AND r.revoked_at IS NULL;
   GET DIAGNOSTICS recovery_count=ROW_COUNT;
   IF recovery_count<>expected THEN
    RAISE EXCEPTION 'Required recovery revocation did not complete' USING ERRCODE='P0001';
   END IF;
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
    SELECT count(*) INTO expected FROM %1$I.invitations i
     WHERE i.workspace_id=scope_id AND i.invited_by=p_user_id
      AND i.accepted_at IS NULL AND i.revoked_at IS NULL;
    UPDATE %1$I.invitations i SET revoked_at=clock_timestamp(),version=i.version+1
     WHERE i.workspace_id=scope_id AND i.invited_by=p_user_id
      AND i.accepted_at IS NULL AND i.revoked_at IS NULL;
    GET DIAGNOSTICS affected=ROW_COUNT;
    IF affected<>expected THEN
     RAISE EXCEPTION 'Required invitation revocation did not complete' USING ERRCODE='P0001';
    END IF;
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
END $migration$;
REVOKE ALL ON FUNCTION operator_issue_recovery(uuid,uuid,text,integer,text) FROM PUBLIC;
REVOKE ALL ON FUNCTION operator_revoke_recovery(uuid,text) FROM PUBLIC;
REVOKE ALL ON FUNCTION operator_change_account_state(uuid,boolean,text) FROM PUBLIC;
