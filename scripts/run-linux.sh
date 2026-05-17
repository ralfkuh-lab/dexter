#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Falls die ALSA-Libs aus dem lokalen Dep-Cache statt vom System genutzt wurden.
DEXTER_ALSA="${DEXTER_ALSA:-$HOME/.cache/dexter-deps/alsa/root}"
export LD_LIBRARY_PATH="$DEXTER_ALSA/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

exec "$ROOT_DIR/src-tauri/target/debug/voice-assistant"
