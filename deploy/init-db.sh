#!/bin/sh
set -eu
runtime_password=$(cat "${OPENTRACK_RUNTIME_PASSWORD_FILE:-/run/secrets/runtime_password}")
migration_password=$(cat "${OPENTRACK_MIGRATION_PASSWORD_FILE:-/run/secrets/migration_password}")
operator_password=$(cat "${OPENTRACK_OPERATOR_PASSWORD_FILE:-/run/secrets/operator_password}")
worker_password=$(cat "${OPENTRACK_WORKER_PASSWORD_FILE:-/run/secrets/worker_password}")
case "$runtime_password$migration_password$operator_password$worker_password" in
  *[!0-9a-f]*|'') echo 'Database passwords must be generated hexadecimal secrets.' >&2; exit 1 ;;
esac
[ "${#runtime_password}" -eq 64 ] && [ "${#migration_password}" -eq 64 ] && [ "${#operator_password}" -eq 64 ] && [ "${#worker_password}" -eq 64 ] || { echo 'Invalid database secret length.' >&2; exit 1; }

# Values are constrained to hexadecimal above; secrets flow only through stdin.
if ! psql -X -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname postgres >/dev/null 2>&1 <<SQL
SET log_statement = 'none';
SET log_min_error_statement = 'panic';
CREATE ROLE opentrack_migrator LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS PASSWORD '$migration_password';
CREATE ROLE opentrack LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS PASSWORD '$runtime_password';
CREATE ROLE opentrack_operator LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS PASSWORD '$operator_password';
CREATE ROLE opentrack_worker LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS PASSWORD '$worker_password';
CREATE DATABASE opentrack OWNER opentrack_migrator;
REVOKE ALL ON DATABASE opentrack FROM PUBLIC;
GRANT CONNECT ON DATABASE opentrack TO opentrack;
GRANT CONNECT ON DATABASE opentrack TO opentrack_operator;
GRANT CONNECT ON DATABASE opentrack TO opentrack_worker;
\connect opentrack
REVOKE ALL ON SCHEMA public FROM PUBLIC;
GRANT USAGE, CREATE ON SCHEMA public TO opentrack_migrator;
GRANT USAGE ON SCHEMA public TO opentrack;
GRANT USAGE ON SCHEMA public TO opentrack_operator;
GRANT USAGE ON SCHEMA public TO opentrack_worker;
ALTER DEFAULT PRIVILEGES FOR ROLE opentrack_migrator IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO opentrack;
ALTER DEFAULT PRIVILEGES FOR ROLE opentrack_migrator IN SCHEMA public GRANT USAGE, SELECT ON SEQUENCES TO opentrack;
ALTER ROLE opentrack SET statement_timeout = '3s';
ALTER ROLE opentrack SET lock_timeout = '1s';
ALTER ROLE opentrack SET idle_in_transaction_session_timeout = '5s';
SQL
then
  echo 'Initial database provisioning failed; preserve the database and secret volumes for explicit reconciliation. No credential values printed.' >&2
  exit 1
fi
printf '%s\n' 'Initial database and separate restricted roles provisioned.'
