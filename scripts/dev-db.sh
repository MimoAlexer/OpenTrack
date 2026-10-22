#!/usr/bin/env bash
# PostgreSQL development cluster. Never modifies system packages or services.
set -Eeuo pipefail
umask 077

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
STATE="${OPENTRACK_DEV_STATE:-$ROOT/work/postgres}"
PORT="${OPENTRACK_DEV_DB_PORT:-55432}"
BIN="${OPENTRACK_PG_BIN:-$STATE/runtime/usr/lib/postgresql/18/bin}"
DATA="$STATE/data"
SOCKET="$STATE/socket"
ENV_FILE="$STATE/dev.env"
MULTIARCH="$(dpkg-architecture -qDEB_HOST_MULTIARCH 2>/dev/null || gcc -dumpmachine)"
export LD_LIBRARY_PATH="$STATE/runtime/usr/lib/$MULTIARCH${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

fail() { printf '%s\n' "$*" >&2; exit 1; }
require_binary() { [[ -x "$BIN/$1" ]] || fail "Missing $1. Run scripts/dev-db.sh install or set OPENTRACK_PG_BIN to PostgreSQL 18 binaries."; }
load_env() { [[ -r "$ENV_FILE" ]] || fail 'Run scripts/dev-db.sh setup first.'; source "$ENV_FILE"; }
admin_sql() { "$BIN/psql" -X -v ON_ERROR_STOP=1 -h "$SOCKET" -p "$PORT" -U "$(id -un)" -d postgres "$@"; }
libpq_env() {
  # Keep connection passwords out of process arguments and ordinary output.
  source <(python3 - "$1" <<'PY'
import os, shlex, sys
from urllib.parse import unquote, urlsplit
url = urlsplit(os.environ[sys.argv[1]])
for key, value in dict(PGHOST=url.hostname, PGPORT=str(url.port), PGUSER=unquote(url.username), PGPASSWORD=unquote(url.password), PGDATABASE=url.path[1:]).items():
    print('export ' + key + '=' + shlex.quote(value))
PY
  )
}

install_runtime() {
  if [[ -x "$BIN/postgres" ]]; then "$BIN/postgres" --version; return; fi
  command -v apt-get >/dev/null || fail 'Automatic local extraction requires apt-get; install PostgreSQL 18 separately and set OPENTRACK_PG_BIN.'
  command -v dpkg-deb >/dev/null || fail 'dpkg-deb is required for local package extraction.'
  mkdir -p "$STATE/packages" "$STATE/runtime"
  # apt validates package hashes against the configured repository metadata.
  (cd "$STATE/packages" && apt-get download postgresql-18 postgresql-client-18 libpq5)
  for package in "$STATE/packages"/*.deb; do dpkg-deb -x "$package" "$STATE/runtime"; done
  "$BIN/postgres" --version
  "$BIN/psql" --version
}

initialize() {
  require_binary initdb
  [[ "$PORT" =~ ^[0-9]+$ ]] && (( PORT >= 1024 && PORT <= 65535 )) || fail 'Development database port must be between 1024 and 65535.'
  [[ "$STATE" != *"'"* && "$STATE" != *$'\n'* ]] || fail 'Development state path must not contain quotes or newlines.'
  [[ ${#SOCKET} -lt 90 ]] || fail 'Development socket path is too long; set OPENTRACK_DEV_STATE to a shorter private directory.'
  mkdir -p "$STATE" "$SOCKET"
  chmod 700 "$STATE" "$SOCKET"
  if [[ ! -f "$ENV_FILE" ]]; then
    # Generated values are hexadecimal and are written only to owner-readable files.
    python3 - "$ENV_FILE" "$STATE" "$PORT" <<'PY'
import os, secrets, shlex, sys
path, state, port = sys.argv[1:]
values = {}
for suffix, role, database in [('', 'opentrack', 'opentrack'), ('MIGRATION_', 'opentrack_migrator', 'opentrack'), ('TEST_', 'opentrack_test', 'opentrack_test'), ('TEST_MIGRATION_', 'opentrack_test_migrator', 'opentrack_test')]:
    password = secrets.token_hex(32)
    values['OPENTRACK_' + suffix + 'DATABASE_URL'] = f'postgres://{role}:{password}@127.0.0.1:{port}/{database}'
    with open(os.path.join(state, suffix.lower() + 'database-url'), 'x') as f:
        f.write(values['OPENTRACK_' + suffix + 'DATABASE_URL'] + '\n')
token = secrets.token_hex(32)
with open(os.path.join(state, 'setup-token'), 'x') as f:
    f.write(token + '\n')
values.update(OPENTRACK_SETUP_TOKEN=token, OPENTRACK_ENVIRONMENT='development', OPENTRACK_PUBLIC_URL='http://127.0.0.1:8080', OPENTRACK_LISTEN='127.0.0.1:8080')
with open(path, 'x') as f:
    for key, value in values.items():
        f.write('export ' + key + '=' + shlex.quote(value) + '\n')
PY
  fi
  python3 "$ROOT/scripts/dev-operator-credentials.py" "$ENV_FILE" "$STATE" "$PORT"
  python3 "$ROOT/scripts/dev-worker-credentials.py" "$ENV_FILE" "$STATE" "$PORT"
  if [[ ! -f "$DATA/PG_VERSION" ]]; then
    "$BIN/initdb" -D "$DATA" --username="$(id -un)" --auth-local=peer --auth-host=scram-sha-256 --encoding=UTF8 --locale=C.UTF-8
    cat >> "$DATA/postgresql.conf" <<EOF

# OpenTrack isolated development cluster; never exposed on a public interface.
listen_addresses = '127.0.0.1'
port = $PORT
unix_socket_directories = '$SOCKET'
unix_socket_permissions = 0700
password_encryption = 'scram-sha-256'
max_connections = 60
shared_buffers = '128MB'
jit = off
EOF
  fi
  [[ "$(cat "$DATA/PG_VERSION")" = 18 ]] || fail 'Existing cluster is not PostgreSQL 18; migration requires an explicit backup and upgrade.'
}

start() {
  require_binary pg_ctl
  [[ -f "$DATA/PG_VERSION" ]] || fail 'Run scripts/dev-db.sh setup first.'
  if "$BIN/pg_ctl" -D "$DATA" status >/dev/null 2>&1; then
    printf 'OpenTrack PostgreSQL is already running on port %s.\n' "$PORT"
  else
    "$BIN/pg_ctl" -D "$DATA" -l "$STATE/server.log" -w -t 30 start
  fi
}

roles() {
  load_env
  # SQL is piped, not placed in command-line arguments, logs or tracked files.
  python3 - <<'PY' | admin_sql -q
import os
from urllib.parse import urlsplit
for suffix in ['', 'MIGRATION_', 'TEST_', 'TEST_MIGRATION_', 'OPERATOR_', 'TEST_OPERATOR_', 'WORKER_', 'TEST_WORKER_']:
    url = urlsplit(os.environ['OPENTRACK_' + suffix + 'DATABASE_URL'])
    role, password = url.username, url.password
    print(f"SELECT 'CREATE ROLE {role} LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS' WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname='{role}')\\gexec")
    print(f"ALTER ROLE {role} WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS PASSWORD '{password}';")
for database in ['opentrack', 'opentrack_test']:
    owner = database + '_migrator'
    operator = database + '_operator'
    worker = database + '_worker'
    print(f"SELECT 'CREATE DATABASE {database} OWNER {owner}' WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname='{database}')\\gexec")
    print(f"REVOKE ALL ON DATABASE {database} FROM PUBLIC;")
    print(f"GRANT CONNECT ON DATABASE {database} TO {database};")
    print(f"GRANT CONNECT ON DATABASE {database} TO {operator};")
    print(f"GRANT CONNECT ON DATABASE {database} TO {worker};")
    print(f"\\connect {database}")
    print('REVOKE ALL ON SCHEMA public FROM PUBLIC;')
    print(f'GRANT USAGE, CREATE ON SCHEMA public TO {owner};')
    print(f'GRANT USAGE ON SCHEMA public TO {database};')
    print(f'GRANT USAGE ON SCHEMA public TO {operator};')
    print(f'GRANT USAGE ON SCHEMA public TO {worker};')
    print(f'REVOKE CREATE ON SCHEMA public FROM {worker};')
    print(f'REVOKE CREATE ON SCHEMA public FROM {operator};')
    print(f'REVOKE ALL ON ALL TABLES IN SCHEMA public FROM {operator};')
    print(f'REVOKE ALL ON ALL SEQUENCES IN SCHEMA public FROM {operator};')
    print(f'GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO {database};')
    print(f'GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO {database};')
    for command in [
        f'REVOKE INSERT,UPDATE,DELETE ON public.workspace_stream_heads FROM {database}',
        f'GRANT INSERT(workspace_id),UPDATE(last_seq) ON public.workspace_stream_heads TO {database}',
    ]:
        print("SELECT '" + command + "' WHERE EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid=to_regclass('public.workspace_stream_heads') AND attname='retained_event_bytes' AND NOT attisdropped)\\gexec")
    print(f"SELECT 'REVOKE UPDATE, DELETE, TRUNCATE ON public.audit_entries FROM {database}' WHERE to_regclass('public.audit_entries') IS NOT NULL\\gexec")
    print(f"SELECT 'REVOKE UPDATE, DELETE, TRUNCATE ON public.account_security_events FROM {database}' WHERE to_regclass('public.account_security_events') IS NOT NULL\\gexec")
    print(f"SELECT 'REVOKE UPDATE, DELETE, TRUNCATE ON public.document_revisions FROM {database}' WHERE to_regclass('public.document_revisions') IS NOT NULL\\gexec")
    print(f"SELECT 'REVOKE DELETE, TRUNCATE ON public.issue_templates FROM {database}' WHERE to_regclass('public.issue_templates') IS NOT NULL\\gexec")
    print(f"SELECT 'REVOKE UPDATE, DELETE, TRUNCATE ON public.issue_template_sources FROM {database}' WHERE to_regclass('public.issue_template_sources') IS NOT NULL\\gexec")
    for command in [
        f'REVOKE ALL ON public.jobs,public.job_attempts,public.worker_workspace_registry FROM {database}',
        f'GRANT SELECT,INSERT ON public.jobs TO {database}',
        f'GRANT UPDATE(requested_by,state,retry_count,error_code,available_at,finished_at,version,updated_at) ON public.jobs TO {database}',
        f'GRANT SELECT,INSERT ON public.worker_workspace_registry TO {database}',
    ]:
        print("SELECT '" + command + "' WHERE to_regclass('public.jobs') IS NOT NULL\\gexec")
    print(f"SELECT 'REVOKE UPDATE, DELETE, TRUNCATE ON public.document_attachments,public.document_attachment_revisions FROM {database}' WHERE to_regclass('public.document_attachments') IS NOT NULL\\gexec")
    for table in ['assignment_reconciliations', 'consumer_registrations', 'consumer_checkpoints', 'consumer_receipts', 'consumer_dead_letters', 'consumer_alerts', 'event_retention_state', 'consumer_operations', 'document_retention_operations', 'attachment_storage_operations']:
        print(f"SELECT 'REVOKE ALL ON public.{table} FROM {database}' WHERE to_regclass('public.{table}') IS NOT NULL\\gexec")
    print(f'ALTER DEFAULT PRIVILEGES FOR ROLE {owner} IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO {database};')
    print(f'ALTER DEFAULT PRIVILEGES FOR ROLE {owner} IN SCHEMA public GRANT USAGE, SELECT ON SEQUENCES TO {database};')
    print(f'ALTER ROLE {database} SET statement_timeout = \'3s\';')
    print(f'ALTER ROLE {database} SET lock_timeout = \'1s\';')
    print(f'ALTER ROLE {database} SET idle_in_transaction_session_timeout = \'5s\';')
    print('\\connect postgres')
PY
  printf 'Development and test roles configured. Credentials: %s (private; do not commit).\n' "$ENV_FILE"
}

check() {
  load_env
  require_binary psql
  for variable in OPENTRACK_DATABASE_URL OPENTRACK_TEST_DATABASE_URL OPENTRACK_OPERATOR_DATABASE_URL OPENTRACK_TEST_OPERATOR_DATABASE_URL OPENTRACK_WORKER_DATABASE_URL OPENTRACK_TEST_WORKER_DATABASE_URL; do
    libpq_env "$variable"
    "$BIN/psql" -X -v ON_ERROR_STOP=1 -Atc "SELECT current_database(), current_user, current_setting('server_version'), rolsuper, rolbypassrls, rolcreatedb, rolcreaterole FROM pg_roles WHERE rolname = current_user;"
    "$BIN/psql" -X -v ON_ERROR_STOP=1 -Atc "SELECT NOT has_schema_privilege(current_user, 'public', 'CREATE') AS cannot_create_tables, NOT pg_has_role(current_user, datdba, 'MEMBER') AS not_database_owner FROM pg_database WHERE datname = current_database();"
  done
}

grant_operator() (
  load_env
  require_binary psql
  if [[ "${1:-development}" = test ]]; then
    libpq_env OPENTRACK_TEST_MIGRATION_DATABASE_URL
    export OPENTRACK_OPERATOR_ROLE=opentrack_test_operator
  else
    libpq_env OPENTRACK_MIGRATION_DATABASE_URL
    export OPENTRACK_OPERATOR_ROLE=opentrack_operator
  fi
  OPENTRACK_OPERATOR_SCHEMA=public OPENTRACK_MIGRATION_DATABASE_URL_FILE= PSQL="$BIN/psql" "$ROOT/deploy/grant-operator.sh"
)

worker_roles() {
  python3 "$ROOT/scripts/dev-worker-credentials.py" "$ENV_FILE" "$STATE" "$PORT"
  load_env
  python3 - <<'WORKER_PY' | admin_sql -q
import os
from urllib.parse import urlsplit
for name in ['OPENTRACK_WORKER_DATABASE_URL','OPENTRACK_TEST_WORKER_DATABASE_URL']:
    url=urlsplit(os.environ[name]); role=url.username; database=url.path[1:]
    # Only a missing worker role is created. Existing passwords are not altered.
    command=f"CREATE ROLE {role} LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS PASSWORD '{url.password}'"
    print("SELECT '"+command.replace("'","''")+f"' WHERE NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='{role}')\\gexec")
    print(f"DO $$ BEGIN IF EXISTS(SELECT 1 FROM pg_roles r WHERE rolname='{role}' AND (rolsuper OR rolbypassrls OR rolcreatedb OR rolcreaterole OR rolinherit OR rolreplication OR NOT rolcanlogin OR EXISTS(SELECT 1 FROM pg_auth_members WHERE member=r.oid OR roleid=r.oid))) THEN RAISE EXCEPTION 'Existing worker role is unsafe'; END IF; END $$;")
    print(f"GRANT CONNECT ON DATABASE {database} TO {role};")
    print(f"\\connect {database}")
    print(f"GRANT USAGE ON SCHEMA public TO {role};")
    print('\\connect postgres')
WORKER_PY
  for variable in OPENTRACK_WORKER_DATABASE_URL OPENTRACK_TEST_WORKER_DATABASE_URL; do
    libpq_env "$variable"
    "$BIN/psql" -X -q -w -v ON_ERROR_STOP=1 -Atc "SELECT rolcanlogin AND NOT(rolsuper OR rolbypassrls OR rolcreatedb OR rolcreaterole OR rolinherit OR rolreplication) AND NOT EXISTS(SELECT 1 FROM pg_auth_members WHERE member=r.oid OR roleid=r.oid) FROM pg_roles r WHERE rolname=current_user" | while read -r valid; do [[ "$valid" = t ]] || fail 'Existing worker role is unsafe; no credentials changed.'; done
  done
  printf '%s\n' 'Restricted worker identities are ready; existing credentials preserved.'
}

grant_worker() (
  if [[ "${1:-}" != current ]]; then load_env; fi
  require_binary psql
  if [[ "${1:-development}" = test ]]; then
    libpq_env OPENTRACK_TEST_MIGRATION_DATABASE_URL
    export OPENTRACK_WORKER_ROLE=opentrack_test_worker
  else
    libpq_env OPENTRACK_MIGRATION_DATABASE_URL
    export OPENTRACK_WORKER_ROLE="${OPENTRACK_WORKER_ROLE:-opentrack_worker}"
  fi
  OPENTRACK_WORKER_SCHEMA=public OPENTRACK_MIGRATION_DATABASE_URL_FILE= PSQL="$BIN/psql" "$ROOT/deploy/grant-worker.sh"
)

case "${1:-help}" in
  install) install_runtime ;;
  setup) install_runtime; initialize; start; roles; check ;;
  start) start ;;
  stop) require_binary pg_ctl; "$BIN/pg_ctl" -D "$DATA" -w -t 30 stop -m fast ;;
  status) require_binary pg_ctl; "$BIN/pg_ctl" -D "$DATA" status ;;
  check) check ;;
  env-path) printf '%s\n' "$ENV_FILE" ;;
  psql) load_env; require_binary psql; shift; libpq_env OPENTRACK_DATABASE_URL; exec "$BIN/psql" -X "$@" ;;
  psql-test) load_env; require_binary psql; shift; libpq_env OPENTRACK_TEST_DATABASE_URL; exec "$BIN/psql" -X "$@" ;;
  psql-operator) load_env; require_binary psql; shift; libpq_env OPENTRACK_OPERATOR_DATABASE_URL; exec "$BIN/psql" -X "$@" ;;
  psql-test-operator) load_env; require_binary psql; shift; libpq_env OPENTRACK_TEST_OPERATOR_DATABASE_URL; exec "$BIN/psql" -X "$@" ;;
  grant-operator) grant_operator ;;
  grant-worker) grant_worker ;;
  grant-worker-current) grant_worker current ;;
  worker-setup) worker_roles ;;
  grant-test-worker) grant_worker test ;;
  grant-test-operator) grant_operator test ;;
  migrate|migrate-test)
    load_env
    if [[ "$1" = migrate-test ]]; then export OPENTRACK_MIGRATION_DATABASE_URL="$OPENTRACK_TEST_MIGRATION_DATABASE_URL"; fi
    cd "$ROOT"
    cargo run -p opentrack-server -- migrate
    if [[ "$1" = migrate-test ]]; then grant_operator test; grant_worker test; else grant_operator; grant_worker; fi
    ;;
  test)
    load_env
    export OPENTRACK_DATABASE_URL="$OPENTRACK_TEST_DATABASE_URL"
    export OPENTRACK_MIGRATION_DATABASE_URL="$OPENTRACK_TEST_MIGRATION_DATABASE_URL"
    export OPENTRACK_OPERATOR_DATABASE_URL="$OPENTRACK_TEST_OPERATOR_DATABASE_URL"
    cd "$ROOT"
    cargo run -p opentrack-server -- migrate
    grant_operator test
    grant_worker test
    shift
    exec cargo test --workspace "$@"
    ;;
  help|--help|-h) printf '%s\n' 'Usage: scripts/dev-db.sh {setup|install|start|stop|status|check|env-path|psql|psql-test|psql-operator|psql-test-operator|migrate|migrate-test|grant-operator|grant-test-operator|worker-setup|grant-worker|grant-test-worker|test}' 'setup downloads/extracts PostgreSQL locally and creates isolated development/test databases.' 'source "$(scripts/dev-db.sh env-path)" to load private development environment values.' ;;
  *) fail "Unknown command: $1" ;;
esac
