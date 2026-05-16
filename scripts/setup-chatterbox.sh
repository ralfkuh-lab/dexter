#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VENV_DIR="${DEXTER_CHATTERBOX_VENV:-$ROOT_DIR/.venv-chatterbox}"

if [[ ! -x "$VENV_DIR/bin/python" ]]; then
  uv venv "$VENV_DIR" --python 3.12
fi
uv pip install --python "$VENV_DIR/bin/python" "setuptools<81" chatterbox-tts fastapi "uvicorn[standard]"

echo "Chatterbox environment ready: $VENV_DIR"
