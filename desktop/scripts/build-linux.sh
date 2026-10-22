#!/usr/bin/env bash
# Build the shared application and package an installable Linux desktop client.
set -Eeuo pipefail
DESKTOP="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT="$(cd "$DESKTOP/.." && pwd)"
OVERLAY="${OPENTRACK_DESKTOP_BUILD_ROOT:-$ROOT/work/desktop-build}"
python3 "$DESKTOP/scripts/prepare-linux-sysroot.py" --directory "$OVERLAY"
export PKG_CONFIG_PATH="$OVERLAY/sysroot/usr/lib/x86_64-linux-gnu/pkgconfig:$OVERLAY/sysroot/usr/share/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
# This directory belongs to the packager, independently of the live web server.
FRONTEND="$(mktemp -d "$OVERLAY/frontend.XXXXXX")"
CONFIG="$(mktemp "$OVERLAY/bundle.XXXXXX.toml")"
cleanup() { rm -rf -- "$FRONTEND"; rm -f -- "$CONFIG"; }
trap cleanup EXIT
npm --prefix "$ROOT/web" ci --no-audit --no-fund
npm --prefix "$ROOT/web" run build -- --outDir "$FRONTEND"
npm --prefix "$DESKTOP" ci --no-audit --no-fund
python3 - "$FRONTEND" "$CONFIG" <<'PY'
import json,sys
from pathlib import Path
Path(sys.argv[2]).write_text('[build]\nfrontendDist = '+json.dumps(str(Path(sys.argv[1]).resolve()))+'\n')
PY
cd "$DESKTOP"
node node_modules/@tauri-apps/cli/tauri.js build --ci --bundles deb --config "$CONFIG" -- --locked -j 2 "$@"
