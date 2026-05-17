#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# ALSA aus dem lokalen Dep-Cache — wird von `cpal` (Mikrofon) gebraucht.
DEXTER_ALSA="${DEXTER_ALSA:-$HOME/.cache/dexter-deps/alsa/root}"

export PKG_CONFIG_PATH="$DEXTER_ALSA/usr/lib/x86_64-linux-gnu/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export CFLAGS="-I$DEXTER_ALSA/usr/include ${CFLAGS:-}"
export LIBRARY_PATH="$DEXTER_ALSA/usr/lib/x86_64-linux-gnu${LIBRARY_PATH:+:$LIBRARY_PATH}"
export LD_LIBRARY_PATH="$DEXTER_ALSA/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

cd "$ROOT_DIR"
npm run tauri -- dev
