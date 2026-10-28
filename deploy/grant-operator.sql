\set ON_ERROR_STOP on
BEGIN;
SELECT set_config('opentrack.provision_operator_role', :'operator_role', true),
       set_config('opentrack.provision_operator_schema', :'operator_schema', true);
DO $$
DECLARE
 role_name text := current_setting('opentrack.provision_operator_role');
 schema_name text := current_setting('opentrack.provision_operator_schema');
 candidate record;
 issue_function regprocedure;
 revoke_function regprocedure;
 disable_function regprocedure;
 enable_function regprocedure;
 consumer_status_function regprocedure;
 consumer_disposition_function regprocedure;
 consumer_enabled_function regprocedure;
 cleanup_resume_function regprocedure;
 admission_limits_function regprocedure;
 admission_status_function regprocedure;
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
  RAISE EXCEPTION 'Operator must be a separate restricted login without ownership, inherited roles, or elevated attributes';
 END IF;
 issue_function := to_regprocedure(format('%I.operator_issue_recovery(uuid,uuid,text,integer,text)',schema_name));
 revoke_function := to_regprocedure(format('%I.operator_revoke_recovery(uuid,text)',schema_name));
 disable_function := to_regprocedure(format('%I.operator_disable_account(uuid,text)',schema_name));
 enable_function := to_regprocedure(format('%I.operator_enable_account(uuid,text)',schema_name));
 consumer_status_function := to_regprocedure(format('%I.operator_consumer_status(uuid)',schema_name));
 consumer_disposition_function := to_regprocedure(format('%I.operator_consumer_disposition(uuid,uuid,text,text)',schema_name));
 consumer_enabled_function := to_regprocedure(format('%I.operator_consumer_set_enabled(uuid,boolean,text)',schema_name));
 cleanup_resume_function := to_regprocedure(format('%I.operator_resume_assignment_cleanup(uuid,uuid,text)',schema_name));
 admission_limits_function := to_regprocedure(format('%I.operator_event_admission_limits(uuid,bigint,bigint,text)',schema_name));
 admission_status_function := to_regprocedure(format('%I.operator_event_admission_status(uuid)',schema_name));
 IF issue_function IS NULL OR revoke_function IS NULL OR disable_function IS NULL OR enable_function IS NULL
 OR consumer_status_function IS NULL OR consumer_disposition_function IS NULL OR consumer_enabled_function IS NULL OR cleanup_resume_function IS NULL OR admission_limits_function IS NULL OR admission_status_function IS NULL THEN
  RAISE EXCEPTION 'Apply all operator migrations before granting operator capabilities';
 END IF;
 IF EXISTS(SELECT 1 FROM pg_proc WHERE oid IN(issue_function,revoke_function,disable_function,enable_function,consumer_status_function,consumer_disposition_function,consumer_enabled_function,cleanup_resume_function,admission_limits_function,admission_status_function) AND NOT prosecdef)
    OR EXISTS(SELECT 1 FROM pg_proc p CROSS JOIN LATERAL aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) acl WHERE p.oid IN(issue_function,revoke_function,disable_function,enable_function,consumer_status_function,consumer_disposition_function,consumer_enabled_function,cleanup_resume_function,admission_limits_function,admission_status_function) AND acl.grantee=0 AND acl.privilege_type='EXECUTE') THEN
  RAISE EXCEPTION 'Operator capability functions must be SECURITY DEFINER and deny PUBLIC execution';
 END IF;
 EXECUTE format('REVOKE CREATE ON SCHEMA %I FROM %I',schema_name,role_name);
 EXECUTE format('REVOKE ALL ON ALL TABLES IN SCHEMA %I FROM %I',schema_name,role_name);
 FOR relation IN SELECT c.oid::regclass AS name,string_agg(quote_ident(a.attname),',' ORDER BY a.attnum) AS columns
 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace JOIN pg_attribute a ON a.attrelid=c.oid
 WHERE n.nspname=schema_name AND c.relkind IN('r','p') AND a.attnum>0 AND NOT a.attisdropped GROUP BY c.oid
 LOOP
  EXECUTE format('REVOKE SELECT(%2$s),INSERT(%2$s),UPDATE(%2$s),REFERENCES(%2$s) ON %1$s FROM %3$I',relation.name,relation.columns,role_name);
 END LOOP;
 EXECUTE format('REVOKE ALL ON ALL SEQUENCES IN SCHEMA %I FROM %I',schema_name,role_name);
 EXECUTE format('REVOKE ALL ON ALL FUNCTIONS IN SCHEMA %I FROM %I',schema_name,role_name);
 EXECUTE format('GRANT CONNECT ON DATABASE %I TO %I',current_database(),role_name);
 EXECUTE format('GRANT USAGE ON SCHEMA %I TO %I',schema_name,role_name);
 EXECUTE format('GRANT SELECT(version,success,checksum) ON %I._sqlx_migrations TO %I',schema_name,role_name);
 EXECUTE format('GRANT EXECUTE ON FUNCTION %s,%s,%s,%s,%s,%s,%s,%s,%s,%s TO %I',issue_function,revoke_function,disable_function,enable_function,consumer_status_function,consumer_disposition_function,consumer_enabled_function,cleanup_resume_function,admission_limits_function,admission_status_function,role_name);
 IF to_regclass(format('%I.planning_issue_facts',schema_name)) IS NOT NULL THEN
  issue_function:=to_regprocedure(format('%I.operator_planning_rebuild(uuid,text)',schema_name));
  IF issue_function IS NULL OR NOT (SELECT prosecdef FROM pg_proc WHERE oid=issue_function)
  OR EXISTS(SELECT 1 FROM pg_proc p CROSS JOIN LATERAL aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a WHERE p.oid=issue_function AND a.grantee=0 AND a.privilege_type='EXECUTE') THEN
   RAISE EXCEPTION 'Planning repair capability must deny PUBLIC execution';
  END IF;
  EXECUTE format('GRANT EXECUTE ON FUNCTION %s TO %I',issue_function,role_name);
 END IF;
END $$;
COMMIT;
