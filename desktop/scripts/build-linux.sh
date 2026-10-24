#!/usr/bin/env bash
# Build the shared application and package an installable Linux desktop client.
set -Eeuo pipefail
DESKTOP="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT="$(cd "$DESKTOP/.." && pwd)"
OVERLAY="${OPENTRACK_DESKTOP_BUILD_ROOT:-$ROOT/work/desktop-build}"
python3 "$DESKTOP/scripts/prepare-linux-sysroot.py" --directory "$OVERLAY"
export PKG_CONFIG_PATH="$OVERLAY/sysroot/usr/lib/x86_64-linux-gnu/pkgconfig:$OVERLAY/sysroot/usr/share/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
# Match the native requirements selected by the locked Rust dependencies.
pkg-config --print-errors --cflags --libs \
  'glib-2.0 >= 2.70' 'gobject-2.0 >= 2.70' 'gio-2.0 >= 2.70' \
  'gdk-3.0 >= 3.22' 'gtk+-3.0 >= 3.24' 'gdk-pixbuf-2.0 >= 2.36.8' \
  'webkit2gtk-4.1 >= 2.40' 'javascriptcoregtk-4.1 >= 2.38' 'libsoup-3.0 >= 3.0' \
  'dbus-1 >= 1.6'
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
