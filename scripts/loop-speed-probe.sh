#!/usr/bin/env bash
set -euo pipefail

iterations="${1:-40}"
if ! [[ "$iterations" =~ ^[0-9]+$ ]] || [[ "$iterations" -lt 4 ]]; then
  echo "usage: $0 [iterations>=4]" >&2
  exit 2
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
loom_bin="${LOOM_BIN:-$script_dir/loop/loom}"
store="${LOOP_STORE:-speed.loom}"
workspace="${LOOP_WORKSPACE:-speed}"
project_id="speed"
ticket_id="SPD-1"
space_id="speed-space"
page_id="speed-page"
document_collection="speed-documents"
document_id="speed-document"
lane_id="speed-lane"

if [[ ! -x "$loom_bin" ]]; then
  echo "loom binary is not executable: $loom_bin" >&2
  echo "copy the Loom binary to $script_dir/loop/loom or set LOOM_BIN=/path/to/loom" >&2
  exit 1
fi

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/loom-speed-probe.XXXXXX")"
daemon_started=0
cleanup() {
  local status=$?
  trap - EXIT INT TERM
  if [[ "$daemon_started" -eq 1 ]]; then
    "$loom_bin" daemon stop "$store" --force >/dev/null 2>&1 || true
  fi
  rm -rf "$tmpdir"
  exit "$status"
}
trap cleanup EXIT INT TERM

file_size() {
  stat -f "%z" "$store"
}

measure() {
  local iteration="$1"
  local operation="$2"
  shift 2
  local timing="$tmpdir/time"
  local started
  local finished
  started="$(date +%s)"
  if ! /usr/bin/time -p -o "$timing" "$@" >/dev/null; then
    echo "operation failed: $operation (iteration $iteration)" >&2
    return 1
  fi
  finished="$(date +%s)"
  local elapsed_ms
  elapsed_ms="$(awk '$1 == "real" { printf "%.0f", $2 * 1000 }' "$timing")"
  printf '%s,%s,%s,%s,%s\n' \
    "$iteration" "$operation" "$elapsed_ms" "$started" "$finished" >> "$tmpdir/operations.csv"
  printf '%s' "$elapsed_ms"
}

if [[ ! -f "$store" ]]; then
  echo "initializing $store"
  "$loom_bin" store init "$store" >/dev/null
  "$loom_bin" workspace create "$store" "$workspace" --facet document >/dev/null
else
  echo "using existing $store"
fi

project_json="$("$loom_bin" tickets project-settings-get "$store" "$workspace" "$project_id" --format json 2>/dev/null || true)"
if [[ -z "$project_json" || "$project_json" == "null" ]]; then
  "$loom_bin" tickets project-create "$store" "$workspace" "$project_id" SPD "Speed Probe" --format text >/dev/null
fi
ticket_json="$("$loom_bin" tickets get "$store" "$workspace" "$ticket_id" --format json 2>/dev/null || true)"
if [[ -z "$ticket_json" || "$ticket_json" == "null" ]]; then
  "$loom_bin" tickets create "$store" "$workspace" task \
    --project-id "$project_id" \
    --title "Speed probe ticket" \
    --description "Initial speed probe ticket." \
    --priority P1 \
    --format text >/dev/null
fi
space_json="$("$loom_bin" pages space-get "$store" "$workspace" "$space_id" --format json 2>/dev/null || true)"
if [[ -z "$space_json" || "$space_json" == "null" ]]; then
  "$loom_bin" pages space-create "$store" "$workspace" "$space_id" "Speed Probe Space" --format text >/dev/null
fi
page_json="$("$loom_bin" pages get "$store" "$workspace" "$page_id" --format json 2>/dev/null || true)"
if [[ -z "$page_json" || "$page_json" == "null" ]]; then
  "$loom_bin" pages create "$store" "$workspace" "$page_id" "$space_id" "Speed Probe Page" --format text >/dev/null
fi
if ! "$loom_bin" document get-text "$store" "$workspace" "$document_collection" "$document_id" >/dev/null 2>&1; then
  printf '%s\n' "initial speed document" > "$tmpdir/document.txt"
  "$loom_bin" document put-text "$store" "$workspace" "$document_collection" "$document_id" "$tmpdir/document.txt" >/dev/null
fi
lane_json="$("$loom_bin" lanes get "$store" "$workspace" "$lane_id" --format json 2>/dev/null || true)"
if [[ -z "$lane_json" || "$lane_json" == "null" ]]; then
  "$loom_bin" lanes create "$store" "$workspace" "$lane_id" "$lane_id" \
    --kind assignment \
    --owner-principal "speed:agent" \
    --title "Speed Probe Lane" \
    --description "Initial speed probe lane." \
    --lane-status ready \
    --status-report "initialized" \
    --reviewer-feedback "" \
    --ticket "$ticket_id" \
    --updated-by "speed:agent" \
    --format text >/dev/null
fi

daemon_status="$("$loom_bin" daemon status "$store" --json 2>/dev/null || true)"
if [[ "$daemon_status" != *'"state":"RUNNING"'* ]]; then
  "$loom_bin" daemon start "$store" --transport native >/dev/null
  daemon_started=1
fi

printf '%s\n' "iteration,operation,elapsed_ms,started_epoch,finished_epoch" > "$tmpdir/operations.csv"
printf '%s\n' "iteration,total_ms,store_bytes" > "$tmpdir/iterations.csv"

probe_started="$(date +%s)"
for (( iteration = 1; iteration <= iterations; iteration++ )); do
  body="speed iteration $iteration at $(date +%s000)"
  printf '%s\n' "$body" > "$tmpdir/document.txt"
  printf '# Speed Probe Page\n\n%s\n' "$body" > "$tmpdir/page.txt"

  total_ms=0
  elapsed="$(measure "$iteration" ticket_update \
    "$loom_bin" tickets update "$store" "$workspace" "$ticket_id" \
    --title "Speed probe ticket $iteration" \
    --description "$body" \
    --status in_progress \
    --priority P1 \
    --format text)"
  total_ms="$((total_ms + elapsed))"

  elapsed="$(measure "$iteration" page_update \
    "$loom_bin" pages update "$store" "$workspace" "$page_id" "$tmpdir/page.txt" --format text)"
  total_ms="$((total_ms + elapsed))"

  elapsed="$(measure "$iteration" page_publish \
    "$loom_bin" pages publish "$store" "$workspace" "$page_id" --format text)"
  total_ms="$((total_ms + elapsed))"

  elapsed="$(measure "$iteration" document_put \
    "$loom_bin" document put-text "$store" "$workspace" "$document_collection" "$document_id" "$tmpdir/document.txt")"
  total_ms="$((total_ms + elapsed))"

  elapsed="$(measure "$iteration" lane_update \
    "$loom_bin" lanes update "$store" "$workspace" "$lane_id" \
    --lane-status working \
    --status-report "$body" \
    --reviewer-feedback "speed feedback $iteration" \
    --updated-by "speed:agent" \
    --format text)"
  total_ms="$((total_ms + elapsed))"

  bytes="$(file_size)"
  printf '%s,%s,%s\n' "$iteration" "$total_ms" "$bytes" >> "$tmpdir/iterations.csv"
  if (( iteration == 1 || iteration % 5 == 0 || iteration == iterations )); then
    elapsed_seconds="$(( $(date +%s) - probe_started ))"
    rate="$(awk -v n="$iteration" -v seconds="$elapsed_seconds" \
      'BEGIN { if (seconds == 0) print "n/a"; else printf "%.3f", n / seconds }')"
    printf 'iteration=%d total_ms=%d store_bytes=%d cumulative_iterations_per_second=%s\n' \
      "$iteration" "$total_ms" "$bytes" "$rate"
  fi
done

quarter="$((iterations / 4))"
awk -F, -v last_start="$((iterations - quarter + 1))" '
  NR == 1 { next }
  $1 <= '"$quarter"' { first_sum += $2; first_count++ }
  $1 >= last_start { last_sum += $2; last_count++ }
  {
    all_sum += $2
    all_count++
    if ($2 > max) max = $2
  }
  END {
    first = first_sum / first_count
    last = last_sum / last_count
    printf "iteration_summary average_ms=%.1f max_ms=%d first_quartile_ms=%.1f last_quartile_ms=%.1f slowdown_ratio=%.2f\n",
      all_sum / all_count, max, first, last, last / first
  }
' "$tmpdir/iterations.csv"

awk -F, '
  NR == 1 { next }
  {
    sum[$2] += $3
    count[$2]++
    if ($3 > max[$2]) max[$2] = $3
  }
  END {
    for (operation in sum) {
      printf "operation_summary operation=%s average_ms=%.1f max_ms=%d\n",
        operation, sum[operation] / count[operation], max[operation]
    }
  }
' "$tmpdir/operations.csv" | sort

output_prefix="${LOOP_OUTPUT_PREFIX:-${store}.speed}"
cp "$tmpdir/operations.csv" "${output_prefix}.operations.csv"
cp "$tmpdir/iterations.csv" "${output_prefix}.iterations.csv"
echo "artifacts=${output_prefix}.operations.csv,${output_prefix}.iterations.csv"
if [[ "$daemon_started" -eq 1 ]]; then
  "$loom_bin" daemon stop "$store"
  daemon_started=0
fi
"$loom_bin" store stat "$store"
"$loom_bin" store attribution "$store" "$workspace" --max-objects 0 --examples 0 --format text
