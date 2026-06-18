#!/usr/bin/env bash
#
# THROWAWAY: footprint of candidate L1 (guard) and L2 (derivation) engines,
# each grounded against an empty baseline probe. Builds the `probe` binary once per engine feature
# (release, stripped via the crate's release profile) and reports size and delta over baseline.
#
# Tolerant: a probe that fails to build is reported FAILED and the rest continue. Portable to old
# bash (macOS bash 3.2): no associative arrays, no process substitution, ASCII only.
#
# Usage:   bash compare-l1-l2-size.sh
# Safe to delete when done.

set -o pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
cd "$here" || exit 1

bin="target/release/probe"

mib() { awk -v b="$1" 'BEGIN { printf "%.2f MiB", b / 1048576 }'; }

# Build one probe; print its stripped-binary byte size, or "FAILED".
build_size() {
  rm -f "$bin" 2>/dev/null
  if cargo build --release --no-default-features --features "$1" --quiet >/dev/null 2>&1 && [ -f "$bin" ]; then
    wc -c < "$bin" | tr -d ' '
  else
    echo "FAILED"
  fi
}

printf '==> building baseline\n' >&2; base="$(build_size baseline)"
printf '==> building cel\n'      >&2; cel="$(build_size cel)"
printf '==> building regorus\n'  >&2; rego="$(build_size regorus)"
printf '==> building ascent\n'   >&2; asc="$(build_size ascent)"
printf '==> building cozo\n'     >&2; cozo="$(build_size cozo)"

row() { # $1 label  $2 size-or-FAILED
  if [ "$2" = "FAILED" ]; then
    printf '  %-12s %14s\n' "$1" "FAILED"
  elif [ "$1" = "baseline" ] || [ "$base" = "FAILED" ]; then
    printf '  %-12s %14s   %s\n' "$1" "$(mib "$2")" "(baseline)"
  else
    d=$(( $2 - base ))
    printf '  %-12s %14s   %s\n' "$1" "$(mib "$2")" "$(awk -v x="$d" 'BEGIN{printf "%+.2f MiB", x/1048576}')"
  fi
}

echo
echo "size-probes (release, stripped)   size            vs baseline"
echo "---------------------------------------------------------------"
row baseline "$base"
row cel      "$cel"
row regorus  "$rego"
row ascent   "$asc"
row cozo     "$cozo"
echo
echo "L1: cel (CEL) vs regorus (Rego).  L2: ascent (compile-time macro, expect ~baseline) vs cozo"
echo "(full DB, heavy; may need a C++ toolchain to build).  Pick ONE per layer."
