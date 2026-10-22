#!/usr/bin/env bash
# Build and run the real local app; do not kill or replace existing listeners.
set -Eeuo pipefail
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
declare -A overrides
for name in OPENTRACK_DATABASE_URL OPENTRACK_MIGRATION_DATABASE_URL OPENTRACK_SETUP_TOKEN OPENTRACK_ENVIRONMENT OPENTRACK_PUBLIC_URL OPENTRACK_LISTEN OPENTRACK_CONFIG OPENTRACK_WORKER_DATABASE_URL OPENTRACK_WORKER_DATABASE_URL_FILE; do
  if [[ -v "$name" ]]; then overrides["$name"]="${!name}"; fi
done
# Durable local files are deliberately separate from source and static assets.
if [[ ! -v OPENTRACK__STORAGE__LOCAL_ROOT && ! -v OPENTRACK_CONFIG ]]; then
  export OPENTRACK__STORAGE__LOCAL_ROOT="${XDG_DATA_HOME:-$HOME/.local/share}/opentrack-server/blobs"
  (umask 077; mkdir -p -- "$OPENTRACK__STORAGE__LOCAL_ROOT")
  chmod 700 -- "$OPENTRACK__STORAGE__LOCAL_ROOT"
fi
scripts/dev-db.sh setup
source "$(scripts/dev-db.sh env-path)"
export OPENTRACK_LISTEN=127.0.0.1:18970
for name in "${!overrides[@]}"; do export "$name=${overrides[$name]}"; done
if [[ ! -v 'overrides[OPENTRACK_PUBLIC_URL]' ]]; then export OPENTRACK_PUBLIC_URL="http://$OPENTRACK_LISTEN"; fi
[[ "$OPENTRACK_ENVIRONMENT" = development || "$OPENTRACK_ENVIRONMENT" = test ]] || { printf '%s\n' 'The development launcher requires development or test mode.' >&2; exit 1; }
python3 - <<'PY'
import os, socket
listen = os.environ['OPENTRACK_LISTEN']
host, port = listen.rsplit(':', 1)
if host != '127.0.0.1':
    raise SystemExit('Development launcher binds only to loopback 127.0.0.1.')
with socket.socket() as probe:
    # Like the real listener, permit a restart while old connections expire.
    # An active listener still prevents this bind; no process is stopped.
    probe.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    try:
        probe.bind((host, int(port)))
    except OSError:
        raise SystemExit(f'{listen} is already in use. Choose OPENTRACK_LISTEN and OPENTRACK_PUBLIC_URL explicitly; no process was stopped.')
PY
# Derive only the worker role identifier; passwords remain in environment/files.
if [[ -v 'overrides[OPENTRACK_WORKER_DATABASE_URL_FILE]' || -v OPENTRACK__WORKER__DATABASE_URL_FILE ]]; then unset OPENTRACK_WORKER_DATABASE_URL; fi
grant_exports="$(python3 - <<'PY_ROLE'
import os,re,shlex,stat,tomllib
from pathlib import Path
from urllib.parse import urlsplit

def private_file(value):
    path=Path(value); meta=path.lstat()
    assert path.is_absolute() and stat.S_ISREG(meta.st_mode) and meta.st_uid==os.getuid() and stat.S_IMODE(meta.st_mode) in (0o400,0o600)
    assert meta.st_size<=65536
    return path.read_text().strip()
try:
    config={}
    if os.environ.get('OPENTRACK_CONFIG'):
        with open(os.environ['OPENTRACK_CONFIG'],'rb') as source:config=tomllib.load(source)
    def secret(section,key,alias):
        file=config.get(section,{}).get(key+'_file')
        file=os.environ.get(alias+'_FILE',file)
        file=os.environ.get('OPENTRACK__'+section.upper()+'__'+key.upper()+'_FILE',file)
        literal=os.environ.get(alias)
        literal=os.environ.get('OPENTRACK__'+section.upper()+'__'+key.upper(),literal)
        assert not (literal is not None and file is not None)
        return literal if literal is not None else private_file(file)
    worker=urlsplit(secret('worker','database_url','OPENTRACK_WORKER_DATABASE_URL'))
    migration_value=secret('database','migration_url','OPENTRACK_MIGRATION_DATABASE_URL')
    migration=urlsplit(migration_value)
    assert worker.scheme in ('postgres','postgresql') and migration.scheme in ('postgres','postgresql')
    assert (worker.hostname,worker.port,worker.path)==(migration.hostname,migration.port,migration.path)
    assert re.fullmatch(r'[a-zA-Z_][a-zA-Z0-9_]*',worker.username or '')
    assert worker.username!=migration.username
except (AssertionError,KeyError,OSError,ValueError,TypeError,tomllib.TOMLDecodeError):
    raise SystemExit('Worker and migration credentials must resolve to the same local database with separate simple role names and private files; no values printed.')
print('export OPENTRACK_WORKER_ROLE='+shlex.quote(worker.username))
print('OPENTRACK_WORKER_GRANT_DATABASE_URL='+shlex.quote(migration_value))
PY_ROLE
)"
# The producer above emits only shell-quoted assignments; never interpolate raw URL text as code.
eval "$grant_exports"
unset grant_exports
cargo run --locked -p opentrack-server -- migrate
OPENTRACK_MIGRATION_DATABASE_URL="$OPENTRACK_WORKER_GRANT_DATABASE_URL" scripts/dev-db.sh grant-worker-current
unset OPENTRACK_WORKER_GRANT_DATABASE_URL
npm --prefix web ci --no-audit --no-fund
npm --prefix web run build
cargo build --locked -p opentrack-server
printf 'Starting OpenTrack API at %s and its isolated worker\n' "$OPENTRACK_PUBLIC_URL"
api_pid=
worker_pid=
cleanup() {
  trap - EXIT INT TERM
  # These PIDs were created by this shell; no listener lookup or unrelated process is stopped.
  while read -r child; do
    if [[ "$child" = "$api_pid" || "$child" = "$worker_pid" ]]; then kill -TERM "$child" 2>/dev/null || true; fi
  done < <(jobs -pr)
  for child in "$api_pid" "$worker_pid"; do
    [[ -n "$child" ]] && wait "$child" 2>/dev/null || true
  done
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
(
  unset OPENTRACK_MIGRATION_DATABASE_URL OPENTRACK_MIGRATION_DATABASE_URL_FILE OPENTRACK_OPERATOR_DATABASE_URL OPENTRACK_OPERATOR_DATABASE_URL_FILE
  unset OPENTRACK_WORKER_DATABASE_URL OPENTRACK_WORKER_DATABASE_URL_FILE OPENTRACK_TEST_DATABASE_URL OPENTRACK_TEST_MIGRATION_DATABASE_URL OPENTRACK_TEST_OPERATOR_DATABASE_URL OPENTRACK_TEST_WORKER_DATABASE_URL
  unset OPENTRACK__OPERATOR__DATABASE_URL OPENTRACK__OPERATOR__DATABASE_URL_FILE OPENTRACK__WORKER__DATABASE_URL OPENTRACK__WORKER__DATABASE_URL_FILE
  unset OPENTRACK__DATABASE__MIGRATION_URL OPENTRACK__DATABASE__MIGRATION_URL_FILE
  exec target/debug/opentrack-server serve
) &
api_pid=$!
(
  unset OPENTRACK_DATABASE_URL OPENTRACK_DATABASE_URL_FILE OPENTRACK_MIGRATION_DATABASE_URL OPENTRACK_MIGRATION_DATABASE_URL_FILE OPENTRACK_OPERATOR_DATABASE_URL OPENTRACK_OPERATOR_DATABASE_URL_FILE OPENTRACK_SETUP_TOKEN OPENTRACK_SETUP_TOKEN_FILE
  unset OPENTRACK_TEST_DATABASE_URL OPENTRACK_TEST_MIGRATION_DATABASE_URL OPENTRACK_TEST_OPERATOR_DATABASE_URL OPENTRACK_TEST_WORKER_DATABASE_URL
  unset OPENTRACK__DATABASE__URL OPENTRACK__DATABASE__URL_FILE OPENTRACK__DATABASE__MIGRATION_URL OPENTRACK__DATABASE__MIGRATION_URL_FILE
  unset OPENTRACK__OPERATOR__DATABASE_URL OPENTRACK__OPERATOR__DATABASE_URL_FILE OPENTRACK__AUTH__SETUP_TOKEN OPENTRACK__AUTH__SETUP_TOKEN_FILE
  exec target/debug/opentrack-server worker
) &
worker_pid=$!
status=0
wait -n "$api_pid" "$worker_pid" || status=$?
exit "$status"
