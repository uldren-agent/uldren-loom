#!/usr/bin/env bash
#
# Focused footprint report for a full Rust IPFS node. The separately reusable Bitswap protocol has
# its own detached `compare-bitswap-size.sh` report.

set -o pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
cd "$here" || exit 1

bin="target/release/probe"

kib() { awk -v b="$1" 'BEGIN { printf "%.1f KiB", b / 1024 }'; }

build_size() {
  rm -f "$bin" 2>/dev/null
  if cargo build --release --no-default-features --features "$1" --quiet >/dev/null 2>&1 && [ -f "$bin" ]; then
    wc -c < "$bin" | tr -d ' '
  else
    echo "FAILED"
  fi
}

dep_count() {
  cargo tree --no-default-features --features "$1" -e normal --prefix none 2>/dev/null \
    | sed '1d' | sort -u | grep -c .
}

row() {
  label="$1"
  feature="$2"
  point="$3"
  size="$(build_size "$feature")"
  deps="$(dep_count "$feature")"
  if [ "$size" = "FAILED" ]; then
    printf '  %-20s %-20s %14s   %-12s  %-7s  %s\n' "$label" "$feature" "FAILED" "" "" "$point"
  elif [ "$feature" = "baseline" ]; then
    base="$size"
    printf '  %-20s %-20s %14s   %-12s  %-7s  %s\n' "$label" "$feature" "$(kib "$size")" "(baseline)" "${deps} deps" "$point"
  else
    delta=$(( size - base ))
    printf '  %-20s %-20s %14s   %-12s  %-7s  %s\n' "$label" "$feature" "$(kib "$size")" "$(awk -v x="$delta" 'BEGIN{printf "%+.1f KiB", x/1024}')" "${deps} deps" "$point"
  fi
}

echo
echo "IPFS size-probes"
echo "----------------------------------------------------------------------------------------------------------------"
printf '  %-20s %-20s %14s   %-12s  %-7s  %s\n' "probe" "feature" "size" "vs baseline" "deps" "point"
echo "----------------------------------------------------------------------------------------------------------------"
row baseline baseline "empty binary control"
row rust-ipfs-node ipfs_rust_node "embeddable node: DHT, Bitswap, pubsub"
