#!/usr/bin/env sh
set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
CARGO_BIN="${CARGO:-cargo}"
OUT_DIR="${PIM_CERT_OUT_DIR:-$ROOT_DIR/scripts/pim-cert/out}"
TARGET_DIR="${PIM_CERT_BUILD_TARGET_DIR:-$OUT_DIR/build-target}"
BIN_DIR="$ROOT_DIR/scripts/pim-cert/bin"
LOOM_BIN="$BIN_DIR/loom"
TMP_BIN="$BIN_DIR/loom.tmp.$$"

cleanup() {
  rm -f "$TMP_BIN"
}
trap cleanup EXIT

mkdir -p "$BIN_DIR" "$TARGET_DIR"

CARGO_TARGET_DIR="$TARGET_DIR" "$CARGO_BIN" build -p uldren-loom-cli --no-default-features --features serve
cp "$TARGET_DIR/debug/loom" "$TMP_BIN"
chmod 755 "$TMP_BIN"
mv "$TMP_BIN" "$LOOM_BIN"

"$LOOM_BIN" version
echo "copied loom to $LOOM_BIN"
