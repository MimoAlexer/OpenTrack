-- Durable, scoped status-replacement work. API intent and worker capability are separate.
CREATE TABLE jobs (
 workspace_id uuid NOT NULL REFERENCES workspaces(id),
 id uuid NOT NULL,
 kind text NOT NULL CHECK(kind='workflow.status_replace'),
 requested_by uuid NOT NULL REFERENCES users(id),
 state text NOT NULL DEFAULT 'queued' CHECK(state IN('queued','running','retry_wait','blocked','dead_letter','succeeded')),
 version bigint NOT NULL DEFAULT 1 CHECK(version>0),
 available_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 lease_owner uuid,
 lease_token bigint NOT NULL DEFAULT 0 CHECK(lease_token>=0),
 lease_expires_at timestamptz,
 attempt_count integer NOT NULL DEFAULT 0 CHECK(attempt_count>=0),
 retry_count integer NOT NULL DEFAULT 0 CHECK(retry_count>=0),
 max_attempts integer NOT NULL DEFAULT 5 CHECK(max_attempts BETWEEN 1 AND 10),
 error_code text CHECK(error_code IN('access_lost','parent_archived','target_unavailable','dependency_unavailable','attempt_abandoned','effect_failed','attempt_limit')),
 dedupe_key text NOT NULL CHECK(length(dedupe_key) BETWEEN 1 AND 200),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 finished_at timestamptz,
 PRIMARY KEY(workspace_id,id),
 UNIQUE(workspace_id,kind,dedupe_key),
 CHECK((state='running')=(lease_owner IS NOT NULL AND lease_expires_at IS NOT NULL)),
 CHECK(state='running' OR (lease_owner IS NULL AND lease_expires_at IS NULL)),
 CHECK(state NOT IN('succeeded','dead_letter','blocked') OR finished_at IS NOT NULL)
);
CREATE INDEX jobs_ready ON jobs(workspace_id,available_at,id) WHERE state IN('queued','retry_wait');
CREATE INDEX jobs_expired_lease ON jobs(workspace_id,lease_expires_at,id) WHERE state='running';
CREATE TABLE job_attempts (
 workspace_id uuid NOT NULL,
 job_id uuid NOT NULL,
 attempt integer NOT NULL CHECK(attempt>0),
 lease_token bigint NOT NULL CHECK(lease_token>0),
 started_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 ended_at timestamptz,
 outcome text CHECK(outcome IN('succeeded','retry_wait','blocked','dead_letter','abandoned')),
 PRIMARY KEY(workspace_id,job_id,attempt),
 FOREIGN KEY(workspace_id,job_id) REFERENCES jobs(workspace_id,id),
 CHECK((ended_at IS NULL)=(outcome IS NULL))
);
-- A metadata-only registry avoids granting an unscoped SELECT on jobs or content.
-- Entries persist; workers scan it with a bounded keyset cursor and ignore idle scopes.
CREATE TABLE worker_workspace_registry (
 workspace_id uuid PRIMARY KEY REFERENCES workspaces(id)
);
DO $$ DECLARE tab text; BEGIN
 FOREACH tab IN ARRAY ARRAY['jobs','job_attempts','worker_workspace_registry'] LOOP
  EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',tab);
  EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',tab);
  EXECUTE format('CREATE POLICY tenant_scope ON %I USING (workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid) WITH CHECK (workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid)',tab);
 END LOOP;
 EXECUTE format('CREATE POLICY registry_discovery_owner ON worker_workspace_registry FOR SELECT USING (current_user=%L)',current_user);
END $$;
CREATE FUNCTION worker_ready_workspaces(after_workspace uuid, page_limit integer)
RETURNS TABLE(workspace_id uuid) LANGUAGE sql SECURITY DEFINER
AS $$ SELECT r.workspace_id FROM worker_workspace_registry r
 WHERE (after_workspace IS NULL OR r.workspace_id>after_workspace)
 ORDER BY r.workspace_id LIMIT greatest(1,least(page_limit,100)) $$;
-- FOR SHARE requires UPDATE privilege. Expose only the locks, never grant a worker
-- UPDATE on users/auth guards or access to password/session/recovery columns.
CREATE FUNCTION worker_lock_context(actor uuid, workspace uuid)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE enabled boolean;
BEGIN
 PERFORM set_config('opentrack.user_id',actor::text,true),
         set_config('opentrack.workspace_id',workspace::text,true),
         set_config('statement_timeout','3000',true),
         set_config('lock_timeout','1000',true),
         set_config('idle_in_transaction_session_timeout','5000',true);
 SELECT disabled_at IS NULL INTO enabled FROM users WHERE id=actor FOR SHARE;
 PERFORM 1 FROM workspace_auth_guards WHERE workspace_id=workspace FOR SHARE;
 RETURN coalesce(enabled,false);
END $$;
DO $$ DECLARE f regprocedure; BEGIN
 FOREACH f IN ARRAY ARRAY['worker_ready_workspaces(uuid,integer)'::regprocedure,'worker_lock_context(uuid,uuid)'::regprocedure] LOOP
  EXECUTE format('ALTER FUNCTION %s SET search_path TO pg_catalog,%I,pg_temp',f,current_schema());
  EXECUTE format('REVOKE ALL ON FUNCTION %s FROM PUBLIC',f);
 END LOOP;
END $$;
-- Default API table grants must not implicitly authorize lease administration.
DO $$ DECLARE grantee_name text; BEGIN
 FOR grantee_name IN
 SELECT DISTINCT r.rolname FROM pg_class c CROSS JOIN LATERAL aclexplode(c.relacl) a
 JOIN pg_roles r ON r.oid=a.grantee WHERE c.oid='jobs'::regclass AND a.grantee<>c.relowner
 LOOP
  EXECUTE format('REVOKE ALL ON jobs,job_attempts,worker_workspace_registry FROM %I',grantee_name);
  EXECUTE format('GRANT SELECT,INSERT ON jobs TO %I',grantee_name);
  EXECUTE format('GRANT UPDATE(requested_by,state,retry_count,error_code,available_at,finished_at,version,updated_at) ON jobs TO %I',grantee_name);
  EXECUTE format('GRANT SELECT,INSERT ON worker_workspace_registry TO %I',grantee_name);
 END LOOP;
END $$;
