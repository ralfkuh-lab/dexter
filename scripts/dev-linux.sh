#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

DEXTER_ALSA="${DEXTER_ALSA:-$HOME/.cache/dexter-deps/alsa/root}"
DEXTER_CLANG="${DEXTER_CLANG:-$HOME/.cache/dexter-deps/clang/root}"
DEXTER_CMAKE="${DEXTER_CMAKE:-$HOME/.cache/dexter-deps/cmake/root}"

export PKG_CONFIG_PATH="$DEXTER_ALSA/usr/lib/x86_64-linux-gnu/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export CFLAGS="-I$DEXTER_ALSA/usr/include ${CFLAGS:-}"
export LIBRARY_PATH="$DEXTER_ALSA/usr/lib/x86_64-linux-gnu${LIBRARY_PATH:+:$LIBRARY_PATH}"
export LIBCLANG_PATH="$DEXTER_CLANG/usr/lib/llvm-18/lib"
export LD_LIBRARY_PATH="$DEXTER_ALSA/usr/lib/x86_64-linux-gnu:$DEXTER_CLANG/usr/lib/llvm-18/lib:$DEXTER_CLANG/usr/lib/x86_64-linux-gnu:$DEXTER_CMAKE/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="-I$DEXTER_CLANG/usr/lib/llvm-18/lib/clang/18/include -I/usr/include -I/usr/include/x86_64-linux-gnu ${BINDGEN_EXTRA_CLANG_ARGS:-}"
export PATH="$DEXTER_CMAKE/usr/bin:$PATH"

cd "$ROOT_DIR"
npm run tauri -- dev
