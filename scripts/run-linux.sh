#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

DEXTER_ALSA="${DEXTER_ALSA:-$HOME/.cache/dexter-deps/alsa/root}"
DEXTER_CLANG="${DEXTER_CLANG:-$HOME/.cache/dexter-deps/clang/root}"
DEXTER_CMAKE="${DEXTER_CMAKE:-$HOME/.cache/dexter-deps/cmake/root}"

export LD_LIBRARY_PATH="$DEXTER_ALSA/usr/lib/x86_64-linux-gnu:$DEXTER_CLANG/usr/lib/llvm-18/lib:$DEXTER_CLANG/usr/lib/x86_64-linux-gnu:$DEXTER_CMAKE/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

exec "$ROOT_DIR/src-tauri/target/debug/voice-assistant"
