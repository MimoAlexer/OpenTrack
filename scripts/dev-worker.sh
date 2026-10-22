#!/usr/bin/env bash
# Foreground worker; start after scripts/dev-db.sh migrate has applied its grants.
set -Eeuo pipefail
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
declare -A overrides
for name in OPENTRACK_WORKER_DATABASE_URL OPENTRACK_WORKER_DATABASE_URL_FILE OPENTRACK_CONFIG; do
  if [[ -v "$name" ]]; then overrides["$name"]="${!name}"; fi
done
source "$(scripts/dev-db.sh env-path)"
for name in "${!overrides[@]}"; do export "$name=${overrides[$name]}"; done
if [[ -n "${OPENTRACK_WORKER_DATABASE_URL_FILE:-}" ]]; then unset OPENTRACK_WORKER_DATABASE_URL; fi
# Only the worker credential enters this process. The config loader applies its normal precedence.
unset OPENTRACK_DATABASE_URL OPENTRACK_MIGRATION_DATABASE_URL OPENTRACK_OPERATOR_DATABASE_URL OPENTRACK_SETUP_TOKEN
unset OPENTRACK_TEST_DATABASE_URL OPENTRACK_TEST_MIGRATION_DATABASE_URL OPENTRACK_TEST_OPERATOR_DATABASE_URL OPENTRACK_TEST_WORKER_DATABASE_URL
unset OPENTRACK_DATABASE_URL_FILE OPENTRACK_MIGRATION_DATABASE_URL_FILE OPENTRACK_OPERATOR_DATABASE_URL_FILE OPENTRACK_SETUP_TOKEN_FILE
unset OPENTRACK__DATABASE__URL OPENTRACK__DATABASE__URL_FILE OPENTRACK__DATABASE__MIGRATION_URL OPENTRACK__DATABASE__MIGRATION_URL_FILE
unset OPENTRACK__OPERATOR__DATABASE_URL OPENTRACK__OPERATOR__DATABASE_URL_FILE OPENTRACK__AUTH__SETUP_TOKEN OPENTRACK__AUTH__SETUP_TOKEN_FILE
cargo build --locked -p opentrack-server
exec target/debug/opentrack-server worker "$@"
