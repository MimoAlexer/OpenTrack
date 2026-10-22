#!/bin/sh
# The PostgreSQL image runs this only for an empty database volume.
set -eu
export OPENTRACK_RUNTIME_PASSWORD_FILE=/run/secrets/runtime-password
export OPENTRACK_MIGRATION_PASSWORD_FILE=/run/secrets/migration-password
export OPENTRACK_OPERATOR_PASSWORD_FILE=/run/secrets/operator-password
export OPENTRACK_WORKER_PASSWORD_FILE=/run/secrets/worker-password
exec /bin/sh /opt/opentrack/init-db.sh
