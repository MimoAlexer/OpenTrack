#!/usr/bin/env bash
# The same verification entry point is used locally and by CI.
set -Eeuo pipefail
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

declare -A test_overrides
for name in OPENTRACK_TEST_DATABASE_URL OPENTRACK_TEST_MIGRATION_DATABASE_URL OPENTRACK_TEST_OPERATOR_DATABASE_URL OPENTRACK_TEST_WORKER_DATABASE_URL; do
  if [[ -v "$name" ]]; then test_overrides["$name"]="${!name}"; fi
done
if [[ -z "${OPENTRACK_TEST_DATABASE_URL:-}" || -z "${OPENTRACK_TEST_MIGRATION_DATABASE_URL:-}" || -z "${OPENTRACK_TEST_OPERATOR_DATABASE_URL:-}" || -z "${OPENTRACK_TEST_WORKER_DATABASE_URL:-}" ]]; then
  env_file="$(scripts/dev-db.sh env-path)"
  [[ -r "$env_file" ]] || { printf '%s\n' 'Run scripts/dev-db.sh setup or provide the separate runtime, migration, operator, and worker OPENTRACK_TEST_*DATABASE_URL values.' >&2; exit 1; }
  source "$env_file"
fi
for name in "${!test_overrides[@]}"; do export "$name=${test_overrides[$name]}"; done
# Fail before migration if accidentally pointed at a development/production DB.
python3 - <<'PY'
import os
from urllib.parse import urlsplit
runtime = urlsplit(os.environ['OPENTRACK_TEST_DATABASE_URL'])
owner = urlsplit(os.environ['OPENTRACK_TEST_MIGRATION_DATABASE_URL'])
operator = urlsplit(os.environ['OPENTRACK_TEST_OPERATOR_DATABASE_URL'])
worker = urlsplit(os.environ['OPENTRACK_TEST_WORKER_DATABASE_URL'])
assert all(url.path.endswith('_test') for url in [runtime, owner, operator, worker]), 'Verification requires isolated *_test databases'
assert len({(url.hostname, url.port, url.path) for url in [runtime, owner, operator, worker]}) == 1, 'Test URLs must target the same database'
assert len({url.username for url in [runtime, owner, operator, worker]}) == 4, 'Runtime, migration, operator, and worker identities must differ'
PY
export OPENTRACK_DATABASE_URL="$OPENTRACK_TEST_DATABASE_URL"
export OPENTRACK_MIGRATION_DATABASE_URL="$OPENTRACK_TEST_MIGRATION_DATABASE_URL"
export OPENTRACK_OPERATOR_DATABASE_URL="$OPENTRACK_TEST_OPERATOR_DATABASE_URL"
export OPENTRACK_ENVIRONMENT=test
export OPENTRACK_PUBLIC_URL=http://127.0.0.1:18970
export OPENTRACK_LISTEN=127.0.0.1:18970

python3 scripts/test-dev-operator-credentials.py
python3 scripts/test-dev-worker-credentials.py
python3 scripts/test-deploy-secrets.py
python3 scripts/test-dev-supervisor.py
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo run --locked -p opentrack-server -- migrate
cargo test --workspace --locked "$@"
npm --prefix scripts/contracts ci --no-audit --no-fund
scripts/generate-contracts.sh --check
npm --prefix web ci --no-audit --no-fund
npm --prefix web test
npm --prefix web run build
cargo test --locked -p opentrack-api --test built_router_contract -- --ignored
printf '%s\n' 'Formatting, lint, real PostgreSQL Rust contracts, frontend tests, production frontend build, and built-asset HTTP delivery passed.'
