#!/usr/bin/env python3
"""Prepare isolated CI roles; requires explicit PostgreSQL admin environment.

No production credentials are used. A generated runtime credential and a separate
migration-owner and capability-only operator credentials are exported through GITHUB_ENV or a private env file.
"""
import os
from pathlib import Path
import re
import secrets
import subprocess
import sys

prefix = os.environ.get("OPENTRACK_CI_PREFIX", "opentrack_ci")
if not re.fullmatch(r"[a-z][a-z0-9_]{1,40}", prefix):
    sys.exit("CI role prefix must contain 2-41 lowercase ASCII letters, digits or underscores.")
runtime = prefix
migrator = prefix + "_migrator"
operator = prefix + "_operator"
worker = prefix + "_worker"
database = prefix + "_test"
runtime_password = secrets.token_hex(32)
migration_password = secrets.token_hex(32)
operator_password = secrets.token_hex(32)
worker_password = secrets.token_hex(32)
host = os.environ.get("OPENTRACK_CI_TCP_HOST", "127.0.0.1")
port = os.environ.get("PGPORT", "5432")
if host not in {"127.0.0.1", "localhost"} or not port.isdigit():
    sys.exit("CI bootstrap accepts only a loopback PostgreSQL service.")
destination = os.environ.get("GITHUB_ENV") or os.environ.get("OPENTRACK_CI_ENV_FILE")
if not destination:
    sys.exit("Set GITHUB_ENV or OPENTRACK_CI_ENV_FILE before creating CI credentials.")
statements = []
for role, password in [(runtime, runtime_password), (migrator, migration_password), (operator, operator_password), (worker, worker_password)]:
    statements += [
        f"SELECT 'CREATE ROLE {role} LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS' WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname='{role}')\\gexec",
        f"ALTER ROLE {role} WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS PASSWORD '{password}';",
    ]
statements += [
    f"SELECT 'CREATE DATABASE {database} OWNER {migrator}' WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname='{database}')\\gexec",
    f"ALTER DATABASE {database} OWNER TO {migrator};",
    f"REVOKE ALL ON DATABASE {database} FROM PUBLIC;",
    f"GRANT CONNECT ON DATABASE {database} TO {runtime};",
    f"GRANT CONNECT ON DATABASE {database} TO {operator};",
    f"GRANT CONNECT ON DATABASE {database} TO {worker};",
    f"\\connect {database}",
    "REVOKE ALL ON SCHEMA public FROM PUBLIC;",
    f"GRANT USAGE, CREATE ON SCHEMA public TO {migrator};",
    f"GRANT USAGE ON SCHEMA public TO {runtime};",
    f"GRANT USAGE ON SCHEMA public TO {operator};",
    f"GRANT USAGE ON SCHEMA public TO {worker};",
    f"REVOKE CREATE ON SCHEMA public FROM {worker};",
    f"REVOKE CREATE ON SCHEMA public FROM {operator};",
    f"REVOKE ALL ON ALL TABLES IN SCHEMA public FROM {operator};",
    f"REVOKE ALL ON ALL SEQUENCES IN SCHEMA public FROM {operator};",
    f"GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO {runtime};",
    f"GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO {runtime};",
    f"SELECT 'REVOKE INSERT,UPDATE,DELETE ON public.workspace_stream_heads FROM {runtime}' WHERE EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid=to_regclass('public.workspace_stream_heads') AND attname='retained_event_bytes' AND NOT attisdropped)\\gexec",
    f"SELECT 'GRANT INSERT(workspace_id),UPDATE(last_seq) ON public.workspace_stream_heads TO {runtime}' WHERE EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid=to_regclass('public.workspace_stream_heads') AND attname='retained_event_bytes' AND NOT attisdropped)\\gexec",
    f"SELECT 'REVOKE UPDATE, DELETE, TRUNCATE ON public.audit_entries FROM {runtime}' WHERE to_regclass('public.audit_entries') IS NOT NULL\\gexec",
    f"SELECT 'REVOKE UPDATE, DELETE, TRUNCATE ON public.account_security_events FROM {runtime}' WHERE to_regclass('public.account_security_events') IS NOT NULL\\gexec",
    f"ALTER DEFAULT PRIVILEGES FOR ROLE {migrator} IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO {runtime};",
    f"ALTER DEFAULT PRIVILEGES FOR ROLE {migrator} IN SCHEMA public GRANT USAGE, SELECT ON SEQUENCES TO {runtime};",
    f"ALTER ROLE {runtime} SET statement_timeout='3s';",
    f"ALTER ROLE {runtime} SET lock_timeout='1s';",
    f"ALTER ROLE {runtime} SET idle_in_transaction_session_timeout='5s';",
]
for command in [
    f'REVOKE ALL ON public.issue_order_staging FROM {runtime}',
    f'REVOKE UPDATE,DELETE,TRUNCATE ON public.issue_order_normalizations,public.issue_order_guards FROM {runtime}',
]:
    statements.append("SELECT '" + command + "' WHERE to_regclass('public.issue_order_staging') IS NOT NULL\\gexec")
for command in [
    f'REVOKE ALL ON public.jobs,public.job_attempts,public.worker_workspace_registry FROM {runtime}',
    f'GRANT SELECT,INSERT ON public.jobs TO {runtime}',
    f'GRANT UPDATE(requested_by,state,retry_count,error_code,available_at,finished_at,version,updated_at) ON public.jobs TO {runtime}',
    f'GRANT SELECT,INSERT ON public.worker_workspace_registry TO {runtime}',
]:
    statements.append("SELECT '" + command + "' WHERE to_regclass('public.jobs') IS NOT NULL\\gexec")
statements.append(f"SELECT 'REVOKE ALL ON public.planning_backfills FROM {runtime}; REVOKE INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER ON public.planning_issue_facts FROM {runtime}' WHERE to_regclass('public.planning_issue_facts') IS NOT NULL\\gexec")
result = subprocess.run(
    ["psql", "-X", "-q", "-v", "ON_ERROR_STOP=1", "--dbname", "postgres"],
    input="\n".join(statements), text=True, capture_output=True, check=False,
)
if result.returncode:
    detail = result.stderr.replace(runtime_password, "[redacted]").replace(migration_password, "[redacted]").replace(operator_password, "[redacted]").replace(worker_password, "[redacted]")
    sys.exit("CI database initialization failed: " + detail)
values = {
    "OPENTRACK_TEST_WORKER_DATABASE_URL": f"postgres://{worker}:{worker_password}@{host}:{port}/{database}",
    "OPENTRACK_TEST_DATABASE_URL": f"postgres://{runtime}:{runtime_password}@{host}:{port}/{database}",
    "OPENTRACK_TEST_MIGRATION_DATABASE_URL": f"postgres://{migrator}:{migration_password}@{host}:{port}/{database}",
    "OPENTRACK_TEST_OPERATOR_DATABASE_URL": f"postgres://{operator}:{operator_password}@{host}:{port}/{database}",
}
os.umask(0o077)
path = Path(destination)
path.parent.mkdir(parents=True, exist_ok=True)
with path.open("a") as output:
    for key, value in values.items():
        if os.environ.get("GITHUB_ACTIONS") == "true":
            print("::add-mask::" + value)
        output.write(key + "=" + value + "\n")
if not os.environ.get("GITHUB_ENV"):
    path.chmod(0o600)
print(f"Prepared {database} with distinct restricted runtime, migration, operator, and worker identities.")
