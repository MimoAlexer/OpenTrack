\set ON_ERROR_STOP on
BEGIN;
SELECT set_config('opentrack.provision_worker_role', :'worker_role', true),
       set_config('opentrack.provision_worker_schema', :'worker_schema', true);
DO $$
DECLARE
 role_name text:=current_setting('opentrack.provision_worker_role');
 schema_name text:=current_setting('opentrack.provision_worker_schema');
 candidate record;
 discovery regprocedure;
 context_lock regprocedure;
 relation record;
BEGIN
 SELECT * INTO candidate FROM pg_roles WHERE rolname=role_name;
 IF NOT FOUND OR NOT candidate.rolcanlogin OR candidate.rolsuper OR candidate.rolbypassrls
 OR candidate.rolcreatedb OR candidate.rolcreaterole OR candidate.rolinherit OR candidate.rolreplication
 OR EXISTS(SELECT 1 FROM pg_auth_members WHERE member=candidate.oid OR roleid=candidate.oid)
 OR EXISTS(SELECT 1 FROM pg_database WHERE datname=current_database() AND datdba=candidate.oid)
 OR EXISTS(SELECT 1 FROM pg_namespace WHERE nspname=schema_name AND nspowner=candidate.oid)
 OR EXISTS(SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=schema_name AND c.relowner=candidate.oid)
 OR EXISTS(SELECT 1 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname=schema_name AND p.proowner=candidate.oid) THEN
  RAISE EXCEPTION 'Worker must be an isolated restricted login without ownership, memberships or elevated attributes';
 END IF;
 discovery:=to_regprocedure(format('%I.worker_ready_workspaces(uuid,integer)',schema_name));
 context_lock:=to_regprocedure(format('%I.worker_lock_context(uuid,uuid)',schema_name));
 IF discovery IS NULL OR context_lock IS NULL OR to_regclass(format('%I.workflow_replacements',schema_name)) IS NULL THEN
  RAISE EXCEPTION 'Apply worker and workflow migrations before granting worker capabilities';
 END IF;
 IF EXISTS(SELECT 1 FROM pg_proc WHERE oid IN(discovery,context_lock) AND NOT prosecdef)
 OR EXISTS(SELECT 1 FROM pg_proc p CROSS JOIN LATERAL aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) acl WHERE p.oid IN(discovery,context_lock) AND acl.grantee=0 AND acl.privilege_type='EXECUTE') THEN
  RAISE EXCEPTION 'Worker functions must be SECURITY DEFINER and deny PUBLIC execution';
 END IF;
 EXECUTE format('REVOKE ALL ON ALL TABLES IN SCHEMA %I FROM %I',schema_name,role_name);
 FOR relation IN SELECT c.oid::regclass AS name,string_agg(quote_ident(a.attname),',' ORDER BY a.attnum) AS columns
 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace JOIN pg_attribute a ON a.attrelid=c.oid
 WHERE n.nspname=schema_name AND c.relkind IN('r','p') AND a.attnum>0 AND NOT a.attisdropped GROUP BY c.oid
 LOOP
  EXECUTE format('REVOKE SELECT(%2$s),INSERT(%2$s),UPDATE(%2$s),REFERENCES(%2$s) ON %1$s FROM %3$I',relation.name,relation.columns,role_name);
 END LOOP;
 EXECUTE format('REVOKE ALL ON ALL SEQUENCES IN SCHEMA %I FROM %I',schema_name,role_name);
 EXECUTE format('REVOKE ALL ON ALL FUNCTIONS IN SCHEMA %I FROM %I',schema_name,role_name);
 EXECUTE format('REVOKE CREATE ON SCHEMA %I FROM %I',schema_name,role_name);
 EXECUTE format('GRANT CONNECT ON DATABASE %I TO %I',current_database(),role_name);
 EXECUTE format('GRANT USAGE ON SCHEMA %I TO %I',schema_name,role_name);
 EXECUTE format('GRANT EXECUTE ON FUNCTION %s,%s TO %I',discovery,context_lock,role_name);
 EXECUTE format('GRANT SELECT,UPDATE(state,version,available_at,lease_owner,lease_token,lease_expires_at,attempt_count,retry_count,error_code,updated_at,finished_at) ON %I.jobs TO %I',schema_name,role_name);
 EXECUTE format('GRANT SELECT,INSERT,UPDATE(lease_token,ended_at,outcome) ON %I.job_attempts TO %I',schema_name,role_name);
 EXECUTE format('GRANT SELECT(id,archived_at) ON %I.workspaces TO %I',schema_name,role_name);
 EXECUTE format('GRANT SELECT(workspace_id,user_id,role,active) ON %I.workspace_members TO %I',schema_name,role_name);
 EXECUTE format('GRANT SELECT(id,workspace_id,private,archived_at,workflow_version),UPDATE(workflow_version) ON %I.teams TO %I',schema_name,role_name);
 EXECUTE format('GRANT SELECT(workspace_id,team_id,user_id,role) ON %I.team_members TO %I',schema_name,role_name);
 EXECUTE format('GRANT SELECT,UPDATE(archived_at,retiring_at,replacement_status_id,replacement_job_id,is_default) ON %I.workflow_statuses TO %I',schema_name,role_name);
 EXECUTE format('GRANT SELECT(id,workspace_id,team_id,status_id,version,archived_at,deleted_at),UPDATE(status_id,version,updated_at) ON %I.issues TO %I',schema_name,role_name);
 EXECUTE format('GRANT SELECT,UPDATE(processed_count,remaining_count) ON %I.workflow_replacements TO %I',schema_name,role_name);
 EXECUTE format('GRANT SELECT,UPDATE(last_seq) ON %I.workspace_stream_heads TO %I',schema_name,role_name);
 EXECUTE format('GRANT INSERT ON %I.activity_entries,%I.audit_entries,%I.workspace_events TO %I',schema_name,schema_name,schema_name,role_name);
 IF to_regclass(format('%I.consumer_receipts',schema_name)) IS NOT NULL THEN
  FOR discovery IN SELECT p.oid::regprocedure FROM pg_proc p WHERE p.pronamespace=schema_name::regnamespace AND p.proname IN('worker_lock_maintenance_context','worker_assignee_eligible','worker_clear_assignment','worker_capture_events','worker_retain_events') LOOP
   IF NOT (SELECT prosecdef FROM pg_proc WHERE oid=discovery) OR EXISTS(SELECT 1 FROM pg_proc p CROSS JOIN LATERAL aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) acl WHERE p.oid=discovery AND acl.grantee=0 AND acl.privilege_type='EXECUTE') THEN
    RAISE EXCEPTION 'Consumer capabilities must be fixed-path definer functions without PUBLIC execution';
   END IF;
   EXECUTE format('GRANT EXECUTE ON FUNCTION %s TO %I',discovery,role_name);
  END LOOP;
  EXECUTE format('GRANT SELECT(assignee_id) ON %I.issues TO %I',schema_name,role_name);
  EXECUTE format('GRANT SELECT,UPDATE(after_team_id,after_issue_id,scanned_count,cleared_count) ON %I.assignment_reconciliations TO %I',schema_name,role_name);
 END IF;
 IF to_regclass(format('%I.documents',schema_name)) IS NOT NULL THEN
  discovery:=to_regprocedure(format('%I.worker_prune_document_revisions(uuid,integer)',schema_name));
  IF discovery IS NULL OR NOT EXISTS(SELECT 1 FROM pg_proc p WHERE p.oid=discovery AND p.prosecdef AND format('search_path=pg_catalog, %I, pg_temp',schema_name)=ANY(p.proconfig))
  OR EXISTS(SELECT 1 FROM pg_proc p CROSS JOIN LATERAL aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) acl WHERE p.oid=discovery AND acl.grantee=0 AND acl.privilege_type='EXECUTE') THEN
   RAISE EXCEPTION 'Revision retention requires a fixed-path definer function without PUBLIC execution';
  END IF;
  EXECUTE format('GRANT EXECUTE ON FUNCTION %s TO %I',discovery,role_name);
 END IF;
 IF to_regclass(format('%I.attachments',schema_name)) IS NOT NULL THEN
  FOR discovery IN SELECT signature::regprocedure FROM unnest(ARRAY[format('%I.worker_claim_attachment_gc(uuid,integer)',schema_name),format('%I.worker_ack_attachment_gc(uuid,uuid,bigint,uuid)',schema_name)]) signature LOOP
   IF NOT EXISTS(SELECT 1 FROM pg_proc p WHERE p.oid=discovery AND p.prosecdef AND format('search_path=pg_catalog, %I, pg_temp',schema_name)=ANY(p.proconfig))
   OR EXISTS(SELECT 1 FROM pg_proc p CROSS JOIN LATERAL aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) acl WHERE p.oid=discovery AND acl.grantee=0 AND acl.privilege_type='EXECUTE') THEN
    RAISE EXCEPTION 'Attachment cleanup requires fixed-path definer functions without PUBLIC execution';
   END IF;
   EXECUTE format('GRANT EXECUTE ON FUNCTION %s TO %I',discovery,role_name);
  END LOOP;
 END IF;
END $$;
COMMIT;
