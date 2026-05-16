#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VENV_DIR="${DEXTER_CHATTERBOX_VENV:-$ROOT_DIR/.venv-chatterbox}"

if [[ ! -x "$VENV_DIR/bin/python" ]]; then
  echo "Missing Chatterbox venv at $VENV_DIR. Run scripts/setup-chatterbox.sh first." >&2
  exit 1
fi

cd "$ROOT_DIR"
exec "$VENV_DIR/bin/python" "$ROOT_DIR/scripts/chatterbox-openai-server.py" "$@"
