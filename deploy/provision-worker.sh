#!/bin/sh
# Explicit upgrade step for existing volumes. Never reset an existing password.
set -eu
worker_password=$(cat "${OPENTRACK_WORKER_PASSWORD_FILE:-/run/secrets/worker_password}")
case "$worker_password" in
  *[!0-9a-f]*|'') echo 'Worker password must be a generated hexadecimal secret.' >&2; exit 1 ;;
esac
[ "${#worker_password}" -eq 64 ] || { echo 'Invalid worker secret length.' >&2; exit 1; }
PGPASSWORD=$(cat "${POSTGRES_PASSWORD_FILE:-/run/secrets/postgres_password}")
export PGPASSWORD
export PGHOST=${PGHOST:-database} PGPORT=${PGPORT:-5432} PGDATABASE=opentrack PGUSER=postgres

# Existing credentials are verified first. A mismatch must not be "fixed" by
# replacing the password of a role already used by this installation.
exists=$(psql -X -q -w -A -t -v ON_ERROR_STOP=1 -c "SELECT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='opentrack_worker')" 2>/dev/null) || {
  echo 'Could not inspect worker provisioning state; no role was changed.' >&2; exit 1;
}
if [ "$exists" = t ]; then
  PGUSER=opentrack_worker PGPASSWORD="$worker_password" psql -X -q -w -A -t -v ON_ERROR_STOP=1 -c 'SELECT 1' >/dev/null 2>&1 || {
    echo 'Existing worker role does not accept the supplied credential; no password was changed.' >&2; exit 1;
  }
fi

# The only interpolated secret is validated hexadecimal. SQL is sent via stdin,
# never argv, and errors are redacted so generated credentials cannot be echoed.
if ! psql -X -q -w -v ON_ERROR_STOP=1 >/dev/null 2>&1 <<SQL
SET log_statement = 'none';
SET log_min_error_statement = 'panic';
BEGIN;
SELECT pg_advisory_xact_lock(1869636980);
DO \$body\$
DECLARE candidate record;
BEGIN
 SELECT * INTO candidate FROM pg_roles WHERE rolname='opentrack_worker';
 IF NOT FOUND THEN
  CREATE ROLE opentrack_worker LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS PASSWORD '$worker_password';
 ELSE
  IF NOT candidate.rolcanlogin OR candidate.rolsuper OR candidate.rolbypassrls
   OR candidate.rolcreatedb OR candidate.rolcreaterole OR candidate.rolinherit OR candidate.rolreplication
   OR EXISTS(SELECT 1 FROM pg_auth_members WHERE member=candidate.oid OR roleid=candidate.oid)
   OR EXISTS(SELECT 1 FROM pg_database WHERE datdba=candidate.oid)
   OR EXISTS(SELECT 1 FROM pg_namespace WHERE nspowner=candidate.oid)
   OR EXISTS(SELECT 1 FROM pg_class WHERE relowner=candidate.oid)
   OR EXISTS(SELECT 1 FROM pg_proc WHERE proowner=candidate.oid) THEN
   RAISE EXCEPTION 'Worker role must be a separate restricted login without ownership or role memberships';
  END IF;
 END IF;
END \$body\$;
GRANT CONNECT ON DATABASE opentrack TO opentrack_worker;
GRANT USAGE ON SCHEMA public TO opentrack_worker;
COMMIT;
SQL
then
  echo 'Worker provisioning failed its role/ownership checks; no existing password was changed.' >&2; exit 1;
fi
PGUSER=opentrack_worker PGPASSWORD="$worker_password" psql -X -q -w -A -t -v ON_ERROR_STOP=1 -c 'SELECT 1' >/dev/null 2>&1 || {
  echo 'Worker login could not be verified; no existing password was replaced. Reconcile provisioning before granting capabilities.' >&2; exit 1;
}
printf 'Separate worker login verified. Apply migrations, then the explicit worker capability grant step.\n'
