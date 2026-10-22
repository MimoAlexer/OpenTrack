#!/usr/bin/env bash
# Run desktop checks separately from the server's GUI-independent test suite.
set -Eeuo pipefail
DESKTOP="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT="$(cd "$DESKTOP/.." && pwd)"
MODE=full
if [ "$#" -gt 0 ]; then
  if [ "$#" -ne 1 ] || [ "$1" != --core ]; then
    echo 'Usage: desktop/scripts/check.sh [--core]' >&2
    exit 2
  fi
  MODE=core
fi
CLIPPY_FEATURES=()
if [ "$MODE" = core ]; then
  # The optimized vendor regression still needs GLib/GObject/GIO headers and
  # runtime libraries; the Tauri shell and WebKit are outside this CI mode.
  pkg-config --print-errors --exists glib-2.0 gobject-2.0 gio-2.0
  CLIPPY_FEATURES=(--no-default-features)
else
  OVERLAY="${OPENTRACK_DESKTOP_BUILD_ROOT:-$ROOT/work/desktop-build}"
  python3 "$DESKTOP/scripts/prepare-linux-sysroot.py" --directory "$OVERLAY"
  export PKG_CONFIG_PATH="$OVERLAY/sysroot/usr/lib/x86_64-linux-gnu/pkgconfig:$OVERLAY/sysroot/usr/share/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
fi
python3 "$DESKTOP/scripts/check-vendor.py"
cargo fmt --manifest-path "$DESKTOP/Cargo.toml" --check
cargo clippy --manifest-path "$DESKTOP/Cargo.toml" --all-targets "${CLIPPY_FEATURES[@]}" --locked -j 2 -- -D warnings
cargo test --manifest-path "$DESKTOP/Cargo.toml" --no-default-features --locked -j 2
