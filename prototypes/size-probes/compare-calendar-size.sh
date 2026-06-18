#!/usr/bin/env bash
# THROWAWAY: measure the binary footprint of the calendar (0037) parse/recurrence candidates against
# the empty baseline. Release + stripped + thin-LTO so the delta reflects code/data size, not debug
# info. Each feature links exactly one path; `ical_rrule` links both (the facet's real footprint).
#
#   ./compare-calendar-size.sh
#
# Observed (aarch64-linux, stripped release): baseline ~0.29 MiB, +icalendar ~+48 KiB,
# +rrule ~+3.0 MiB (chrono-tz embeds the full IANA tz database), +both ~+2.9 MiB. `civil_time` (the
# `time` crate, the substrate for a hand-built RRULE engine) measures == baseline, i.e. ~0 marginal
# dependency footprint. The rrule tz database dominates and is the figure that motivates building our
# own RRULE engine over `time` + each resource's embedded VTIMEZONE instead of linking chrono-tz.
set -euo pipefail
cd "$(dirname "$0")"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/sizeprobe}"
for feat in baseline civil_time ical rrule ical_rrule; do
  if cargo build --release --no-default-features --features "$feat" >/dev/null 2>&1; then
    sz=$(stat -c %s "$CARGO_TARGET_DIR/release/probe")
    printf "%-12s %10d bytes  %6.2f MiB\n" "$feat" "$sz" "$(echo "scale=2; $sz/1048576" | bc)"
  else
    echo "$feat FAILED (bump version or adjust its block in main.rs)"
  fi
  rm -f "$CARGO_TARGET_DIR/release/probe"
done
