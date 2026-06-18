#!/usr/bin/env bash
#
# THROWAWAY: footprint + dependency-surface of candidate cron-expression parsers for the
# events/triggers exploration. Each parser is grounded
# against an empty baseline probe. Builds the `probe` binary once per parser feature (release,
# stripped via the crate's release profile) and reports stripped size, delta over baseline, and
# transitive crate count.
#
# These parsers are HOST-SIDE (they run in the scheduler/daemon, never in the deterministic
# sandbox), so binary size is informative but rarely decisive at these magnitudes; the dependency
# surface, cron dialect, and maintenance recency usually matter more.
#
# Tolerant: a probe that fails to build is reported FAILED and the rest continue. Portable to old
# bash (macOS bash 3.2): no associative arrays, no process substitution, ASCII only.
#
# Usage:   bash compare-cron-size.sh
# Safe to delete when done.

set -o pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
cd "$here" || exit 1

bin="target/release/probe"

mib() { awk -v b="$1" 'BEGIN { printf "%.2f MiB", b / 1048576 }'; }
kib() { awk -v b="$1" 'BEGIN { printf "%.1f KiB", b / 1024 }'; }

# Build one probe; print its stripped-binary byte size, or "FAILED".
build_size() {
  rm -f "$bin" 2>/dev/null
  if cargo build --release --no-default-features --features "$1" --quiet >/dev/null 2>&1 && [ -f "$bin" ]; then
    wc -c < "$bin" | tr -d ' '
  else
    echo "FAILED"
  fi
}

# Count transitive normal-dependency crates for a feature (excludes the probe crate itself).
dep_count() {
  cargo tree --no-default-features --features "$1" -e normal --prefix none 2>/dev/null \
    | sed '1d' | sort -u | grep -c .
}

printf '==> building baseline\n' >&2; base="$(build_size baseline)"
printf '==> building cron\n'     >&2; cron="$(build_size cron)";       cron_d="$(dep_count cron)"
printf '==> building croner\n'   >&2; croner="$(build_size croner)";   croner_d="$(dep_count croner)"
printf '==> building saffron\n'  >&2; saffron="$(build_size saffron)"; saffron_d="$(dep_count saffron)"

row() { # $1 label  $2 size-or-FAILED  $3 dep-count
  if [ "$2" = "FAILED" ]; then
    printf '  %-10s %14s\n' "$1" "FAILED"
  elif [ "$1" = "baseline" ] || [ "$base" = "FAILED" ]; then
    printf '  %-10s %14s   %-12s  %s\n' "$1" "$(kib "$2")" "(baseline)" "${3:-0} deps"
  else
    d=$(( $2 - base ))
    printf '  %-10s %14s   %-12s  %s\n' "$1" "$(kib "$2")" \
      "$(awk -v x="$d" 'BEGIN{printf "%+.1f KiB", x/1024}')" "${3:-?} deps"
  fi
}

echo
echo "cron-parser probes (release, stripped)   size           vs baseline    deps (normal, transitive)"
echo "------------------------------------------------------------------------------------------------"
row baseline "$base"     0
row cron     "$cron"     "$cron_d"
row croner   "$croner"   "$croner_d"
row saffron  "$saffron"  "$saffron_d"
echo
echo "All three are host-side parsers over chrono. Pick ONE. Dialect/maintenance usually decide,"
echo "not size: cron 0.16 (sec+year, no L/W/#), croner 3.0 (sec+year, L/#/W, actively maintained),"
echo "saffron 0.1 (Vixie 5-field + L/W, minimal deps, but stale and license-file needs a deny.toml clarify)."
