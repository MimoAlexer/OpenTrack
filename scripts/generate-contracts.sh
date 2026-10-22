#!/usr/bin/env bash
set -Eeuo pipefail
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
[[ ${1:-} = '' || ${1:-} = --check ]] || { printf '%s\n' 'Usage: scripts/generate-contracts.sh [--check]' >&2; exit 2; }
[[ -f scripts/contracts/node_modules/openapi-typescript/bin/cli.js ]] || { printf '%s\n' 'Install generator dependencies with npm --prefix scripts/contracts ci.' >&2; exit 1; }
mkdir -p work docs/api web/src/generated
contract_tmp="$(mktemp -d "$ROOT/work/contract.XXXXXX")"
trap 'rm -rf -- "$contract_tmp"' EXIT
cargo run --quiet --locked -p opentrack-api --example export_openapi > "$contract_tmp/openapi.json"
node scripts/contracts/node_modules/openapi-typescript/bin/cli.js "$contract_tmp/openapi.json" -o "$contract_tmp/api.d.ts"
if [[ ${1:-} = --check ]]; then
  diff -u docs/api/openapi.json "$contract_tmp/openapi.json"
  diff -u web/src/generated/api.d.ts "$contract_tmp/api.d.ts"
  printf '%s\n' 'Committed OpenAPI and TypeScript contract match the Rust handlers and DTOs.'
else
  cp "$contract_tmp/openapi.json" docs/api/openapi.json
  cp "$contract_tmp/api.d.ts" web/src/generated/api.d.ts
  printf '%s\n' 'Updated docs/api/openapi.json and web/src/generated/api.d.ts.'
fi
