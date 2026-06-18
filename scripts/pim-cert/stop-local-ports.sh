#!/usr/bin/env sh
set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
LOOM_BIN="${LOOM_BIN:-$ROOT_DIR/scripts/pim-cert/bin/loom}"
OUT_DIR="${PIM_CERT_OUT_DIR:-$ROOT_DIR/scripts/pim-cert/out}"
ENV_FILE="$OUT_DIR/env.sh"

if [ ! -r "$ENV_FILE" ]; then
  echo "missing $ENV_FILE"
  exit 1
fi

. "$ENV_FILE"

if [ ! -x "$LOOM_BIN" ]; then
  echo "loom binary not found at $LOOM_BIN"
  echo "run: scripts/pim-cert/build-local-loom.sh"
  exit 1
fi

STATUS_JSON=$("$LOOM_BIN" daemon status "$PIM_CERT_STORE" --json 2>/dev/null || true)
if printf '%s\n' "$STATUS_JSON" | grep -q '"state":"RUNNING"'; then
  "$LOOM_BIN" --auth-principal "$PIM_CERT_PRINCIPAL_ID" --auth-key-source "file:$PIM_CERT_PASS_FILE" daemon stop "$PIM_CERT_STORE" --force --wait 5000
else
  echo "local daemon is not running"
fi
