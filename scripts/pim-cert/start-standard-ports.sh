#!/usr/bin/env sh
set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
LOOM_BIN="${LOOM_BIN:-$ROOT_DIR/scripts/pim-cert/bin/loom}"
OUT_DIR="${PIM_CERT_OUT_DIR:-$ROOT_DIR/scripts/pim-cert/out}"
ENV_FILE="$OUT_DIR/env.sh"
SMTP_PORT="${PIM_CERT_SMTP_PORT:-587}"

if [ ! -r "$ENV_FILE" ]; then
  echo "missing $ENV_FILE"
  echo "run scripts/pim-cert/seed.sh and scripts/pim-cert/configure-listeners.sh first"
  exit 1
fi

. "$ENV_FILE"

if [ ! -x "$LOOM_BIN" ]; then
  echo "loom binary not found at $LOOM_BIN"
  echo "run: scripts/pim-cert/build-local-loom.sh"
  exit 1
fi

check_port_free() {
  port="$1"
  if ! command -v lsof >/dev/null 2>&1; then
    return 0
  fi
  owner=$(lsof -nP -iTCP:"$port" -sTCP:LISTEN 2>/dev/null || true)
  if [ -n "$owner" ]; then
    echo "port $port is already in use"
    printf '%s\n' "$owner"
    exit 1
  fi
}

wait_for_store_writer_release() {
  if ! command -v lsof >/dev/null 2>&1; then
    return 0
  fi
  attempt=0
  while [ "$attempt" -lt 50 ]; do
    owner=$(lsof -nP "$PIM_CERT_STORE" 2>/dev/null || true)
    if [ -z "$owner" ]; then
      return 0
    fi
    attempt=$((attempt + 1))
    sleep 0.1
  done
  echo "store is still open by another process"
  printf '%s\n' "$owner"
  exit 1
}

wait_for_daemon_stopped() {
  attempt=0
  while [ "$attempt" -lt 50 ]; do
    status_json=$("$LOOM_BIN" daemon status "$PIM_CERT_STORE" --json 2>/dev/null || true)
    if ! printf '%s\n' "$status_json" | grep -q '"state":"RUNNING"'; then
      wait_for_store_writer_release
      return 0
    fi
    attempt=$((attempt + 1))
    sleep 0.1
  done
  echo "daemon did not stop before standard port reconfiguration"
  printf '%s\n' "$status_json"
  exit 1
}

configure_with_retry() {
  attempt=0
  while [ "$attempt" -lt 20 ]; do
    output=$("$@" 2>&1) && {
      if [ "${PIM_CERT_VERBOSE:-0}" = "1" ]; then
        printf '%s\n' "$output"
      fi
      return 0
    }
    case "$output" in
      *"loom is open for writing by another process"*)
        attempt=$((attempt + 1))
        sleep 0.25
        ;;
      *)
        printf '%s\n' "$output"
        return 1
        ;;
    esac
  done
  printf '%s\n' "$output"
  return 1
}

if [ "${PIM_CERT_STANDARD_ROOT:-0}" != "1" ]; then
  STATUS_JSON=$("$LOOM_BIN" daemon status "$PIM_CERT_STORE" --json 2>/dev/null || true)
  if printf '%s\n' "$STATUS_JSON" | grep -q '"state":"RUNNING"'; then
    "$LOOM_BIN" --auth-principal "$PIM_CERT_PRINCIPAL_ID" --auth-key-source "file:$PIM_CERT_PASS_FILE" daemon stop "$PIM_CERT_STORE" --force --wait 10000
    wait_for_daemon_stopped
  else
    wait_for_store_writer_release
  fi
  if [ "${PIM_CERT_VERBOSE:-0}" = "1" ]; then
    configure_with_retry "$ROOT_DIR/scripts/pim-cert/configure-listeners.sh"
  else
    configure_with_retry "$ROOT_DIR/scripts/pim-cert/configure-listeners.sh" >/dev/null
  fi
fi

check_port_free 443
check_port_free 993
check_port_free 8444
check_port_free "$SMTP_PORT"

if [ "$(id -u)" != "0" ]; then
  echo "starting daemon on privileged ports with sudo"
  exec sudo -E env PIM_CERT_STANDARD_ROOT=1 "$0"
fi

"$LOOM_BIN" daemon restart "$PIM_CERT_STORE" --transport native
"$LOOM_BIN" daemon status "$PIM_CERT_STORE" --json
