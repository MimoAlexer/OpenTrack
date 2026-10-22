#!/bin/sh
# Apply only the reviewed worker capabilities after migrations; no URL in argv.
set -eu
role=${OPENTRACK_WORKER_ROLE:-opentrack_worker}
schema=${OPENTRACK_WORKER_SCHEMA:-public}
case "$role:$schema" in *[!a-zA-Z0-9_:]*|'') echo 'Use explicit simple PostgreSQL role/schema identifiers.' >&2; exit 1;; esac
if [ -n "${OPENTRACK_MIGRATION_DATABASE_URL_FILE:-}" ]; then
  file=$OPENTRACK_MIGRATION_DATABASE_URL_FILE
  case "$file" in /*) ;; *) echo 'Migration credential path must be absolute.' >&2; exit 1;; esac
  [ -f "$file" ] && [ ! -L "$file" ] && [ "$(stat -c %u "$file")" = "$(id -u)" ] || { echo 'Migration credential must be a regular service-owned file.' >&2; exit 1; }
  case "$(stat -c %a "$file")" in 400|600) ;; *) echo 'Migration credential must have mode 0400 or 0600.' >&2; exit 1;; esac
  # The generated deployment URI has simple identifiers and a 64-hex secret.
  # PGDATABASE alone does not expand a URI: split its constrained form into the
  # ordinary libpq environment, keeping the password out of process arguments.
  url=$(cat "$file")
  case "$url" in postgres://*) ;; *) echo 'Expected a generated PostgreSQL migration credential; no values printed.' >&2; exit 1;; esac
  address=${url#postgres://}
  credentials=${address%%@*}
  address=${address#*@}
  PGUSER=${credentials%%:*}
  PGPASSWORD=${credentials#*:}
  host_port=${address%%/*}
  PGDATABASE=${address#*/}
  PGHOST=${host_port%:*}
  PGPORT=${host_port##*:}
  invalid=0
  case "$PGUSER" in *[!a-zA-Z0-9_]*|'') invalid=1;; esac
  case "$PGPASSWORD" in *[!0-9a-f]*|'') invalid=1;; esac
  [ "${#PGPASSWORD}" -eq 64 ] || invalid=1
  case "$PGHOST" in *[!a-zA-Z0-9_.-]*|'') invalid=1;; esac
  case "$PGPORT" in *[!0-9]*|'') invalid=1;; esac
  case "$PGDATABASE" in *[!a-zA-Z0-9_]*|'') invalid=1;; esac
  [ "$url" = "postgres://$PGUSER:$PGPASSWORD@$PGHOST:$PGPORT/$PGDATABASE" ] || invalid=1
  if [ "$invalid" -ne 0 ] || ! [ "$PGPORT" -ge 1 ] 2>/dev/null || ! [ "$PGPORT" -le 65535 ] 2>/dev/null; then
    echo 'Expected a generated PostgreSQL migration credential; no values printed.' >&2
    exit 1
  fi
  unset url address credentials host_port
  export PGUSER PGPASSWORD PGHOST PGPORT PGDATABASE
fi
# libpq can include malformed URI fragments in diagnostics. Do not forward
# connection/query output from this credential-handling provisioning boundary.
if ! "${PSQL:-psql}" -X -q -w -v ON_ERROR_STOP=1 -v worker_role="$role" -v worker_schema="$schema" -f "$(dirname "$0")/grant-worker.sql" >/dev/null 2>&1; then
  echo 'Worker capability grant failed; verify the migration credential, restricted worker role, and applied migrations. No credential values printed.' >&2
  exit 1
fi
printf 'Restricted worker capability grants applied; no credential values printed.\n'
