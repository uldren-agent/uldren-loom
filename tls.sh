#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"

BIN="$ROOT/target/debug/loom"
WORK="$ROOT/tmp/tls"
STORE="$WORK/admin.loom"
CERT="$WORK/localhost.crt"
KEY="$WORK/localhost.key"
BIND="127.0.0.1:9999"
URL="https://127.0.0.1:9999/admin/listeners"
ACTION="${1:-start}"

usage() {
  printf 'usage: ./tls.sh [start|stop]\n' >&2
}

ensure_loom() {
  if [[ ! -x "$BIN" ]] || ! "$BIN" serve --help >/dev/null 2>&1; then
    cargo build -p uldren-loom-cli --features serve
  fi
}

start_tls() {
  mkdir -p "$WORK"
  ensure_loom

  if [[ ! -f "$CERT" || ! -f "$KEY" ]]; then
    openssl req \
      -x509 \
      -newkey rsa:2048 \
      -keyout "$KEY" \
      -out "$CERT" \
      -days 365 \
      -nodes \
      -subj "/CN=localhost" \
      -addext "subjectAltName=DNS:localhost,IP:127.0.0.1" \
      -addext "basicConstraints=critical,CA:FALSE" \
      -addext "keyUsage=critical,digitalSignature,keyEncipherment" \
      -addext "extendedKeyUsage=serverAuth"
    chmod 600 "$KEY"
  fi

  if [[ ! -f "$STORE" ]]; then
    "$BIN" store init "$STORE"
  fi

  "$BIN" serve configure "$STORE" admin \
    --transport rest \
    --bind "$BIND" \
    --tls-mode direct \
    --tls-cert-ref "$CERT" \
    --tls-key-ref "$KEY" >/dev/null

  if "$BIN" daemon status "$STORE" >/dev/null 2>&1; then
    if ! "$BIN" daemon restart "$STORE" >/dev/null; then
      "$BIN" daemon doctor "$STORE" >&2 || true
      exit 1
    fi
  else
    if ! "$BIN" daemon start "$STORE" >/dev/null; then
      "$BIN" daemon doctor "$STORE" >&2 || true
      exit 1
    fi
  fi

  for _ in $(seq 1 50); do
    if BODY="$(curl --fail --silent --show-error --insecure "$URL" 2>/dev/null)"; then
      printf '%s\n' "$BODY"
      printf 'TLS admin listener is ready at %s\n' "$URL"
      exit 0
    fi
    sleep 0.2
  done

  printf 'TLS admin listener did not become ready at %s\n' "$URL" >&2
  "$BIN" daemon status "$STORE" >&2 || true
  exit 1
}

stop_tls() {
  ensure_loom
  if [[ ! -f "$STORE" ]]; then
    printf 'No TLS smoke store exists at %s\n' "$STORE"
    exit 0
  fi
  if "$BIN" daemon stop "$STORE" --force; then
    exit 0
  fi
  if "$BIN" daemon status "$STORE" | grep -q '^stopped'; then
    printf 'TLS admin listener is already stopped for %s\n' "$STORE"
    exit 0
  fi
  "$BIN" daemon doctor "$STORE" >&2 || true
  exit 1
}

case "$ACTION" in
  start)
    start_tls
    ;;
  stop)
    stop_tls
    ;;
  *)
    usage
    exit 1
    ;;
esac
