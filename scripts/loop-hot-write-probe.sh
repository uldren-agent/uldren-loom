#!/usr/bin/env bash
set -euo pipefail

minutes="${1:-2}"
if ! [[ "$minutes" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
  echo "usage: $0 [minutes]" >&2
  exit 2
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
loom_bin="${LOOM_BIN:-$script_dir/loop/loom}"
store="${LOOP_STORE:-loop.loom}"
daemon_transport="${LOOP_DAEMON_TRANSPORT:-native}"
workspace="${LOOP_WORKSPACE:-loop}"
project_id="loop"
ticket_id="LOOP-1"
space_id="loop-space"
page_id="loop-page"
document_collection="loop-documents"
document_id="loop-document"
lane_id="loop-lane"

if [[ ! -x "$loom_bin" ]]; then
  echo "loom binary is not executable: $loom_bin" >&2
  echo "copy the Loom binary to $script_dir/loop/loom or set LOOM_BIN=/path/to/loom" >&2
  exit 1
fi

duration_seconds="$(awk -v minutes="$minutes" 'BEGIN { printf "%d", minutes * 60 }')"
if [[ "$duration_seconds" -le 0 ]]; then
  echo "duration must be greater than zero minutes" >&2
  exit 2
fi

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/loom-loop-probe.XXXXXX")"
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

write_text_file() {
  local path="$1"
  local body="$2"
  printf '%s\n' "$body" > "$path"
}

run_iteration_operation() {
  local iteration="$1"
  local operation="$2"
  shift 2
  if ! "$@" >/dev/null; then
    echo "operation failed: $operation (iteration $iteration)" >&2
    return 1
  fi
}

if [[ ! -f "$store" ]]; then
  echo "initializing $store"
  "$loom_bin" store init "$store"
  "$loom_bin" workspace create "$store" "$workspace" --facet document
else
  echo "using existing $store"
fi

project_json="$("$loom_bin" tickets project-settings-get "$store" "$workspace" "$project_id" --format json 2>/dev/null || true)"
if [[ -z "$project_json" || "$project_json" == "null" ]]; then
  "$loom_bin" tickets project-create "$store" "$workspace" "$project_id" LOOP "Loop Probe" --format text
fi
ticket_json="$("$loom_bin" tickets get "$store" "$workspace" "$ticket_id" --format json 2>/dev/null || true)"
if [[ -z "$ticket_json" || "$ticket_json" == "null" ]]; then
  "$loom_bin" tickets create "$store" "$workspace" task \
    --project-id "$project_id" \
    --title "Loop probe ticket" \
    --description "Initial loop probe ticket." \
    --priority P1 \
    --format text
fi
space_json="$("$loom_bin" pages space-get "$store" "$workspace" "$space_id" --format json 2>/dev/null || true)"
if [[ -z "$space_json" || "$space_json" == "null" ]]; then
  "$loom_bin" pages space-create "$store" "$workspace" "$space_id" "Loop Probe Space" --format text
fi
page_json="$("$loom_bin" pages get "$store" "$workspace" "$page_id" --format json 2>/dev/null || true)"
if [[ -z "$page_json" || "$page_json" == "null" ]]; then
  "$loom_bin" pages create "$store" "$workspace" "$page_id" "$space_id" "Loop Probe Page" --format text
fi
if ! "$loom_bin" document get-text "$store" "$workspace" "$document_collection" "$document_id" >/dev/null 2>&1; then
  write_text_file "$tmpdir/document.txt" "initial loop document"
  "$loom_bin" document put-text "$store" "$workspace" "$document_collection" "$document_id" "$tmpdir/document.txt"
fi
lane_json="$("$loom_bin" lanes get "$store" "$workspace" "$lane_id" --format json 2>/dev/null || true)"
if [[ -z "$lane_json" || "$lane_json" == "null" ]]; then
  "$loom_bin" lanes create "$store" "$workspace" "$lane_id" "$lane_id" \
    --kind assignment \
    --owner-principal "loop:agent" \
    --title "Loop Probe Lane" \
    --description "Initial loop probe lane." \
    --lane-status ready \
    --status-report "initialized" \
    --reviewer-feedback "" \
    --ticket "$ticket_id" \
    --updated-by "loop:agent" \
    --format text
fi

daemon_status="$("$loom_bin" daemon status "$store" --json 2>/dev/null || true)"
if [[ "$daemon_status" == *'"state":"RUNNING"'* ]]; then
  echo "daemon already running for $store"
else
  echo "starting daemon for $store"
  "$loom_bin" daemon start "$store" --transport "$daemon_transport"
  daemon_started=1
fi

start_epoch="$(date +%s)"
end_epoch="$((start_epoch + duration_seconds))"
iteration=0

while [[ "$(date +%s)" -lt "$end_epoch" ]]; do
  iteration="$((iteration + 1))"
  now_ms="$(date +%s000)"
  body="loop iteration $iteration at $now_ms"
  write_text_file "$tmpdir/document.txt" "$body"
  write_text_file "$tmpdir/page.txt" "# Loop Probe Page

$body"

  run_iteration_operation "$iteration" ticket_update \
    "$loom_bin" tickets update "$store" "$workspace" "$ticket_id" \
    --title "Loop probe ticket $iteration" \
    --description "$body" \
    --status in_progress \
    --priority P1 \
    --format text

  run_iteration_operation "$iteration" page_update \
    "$loom_bin" pages update "$store" "$workspace" "$page_id" "$tmpdir/page.txt" --format text
  run_iteration_operation "$iteration" page_publish \
    "$loom_bin" pages publish "$store" "$workspace" "$page_id" --format text

  run_iteration_operation "$iteration" document_put \
    "$loom_bin" document put-text "$store" "$workspace" "$document_collection" "$document_id" "$tmpdir/document.txt"

  run_iteration_operation "$iteration" lane_update \
    "$loom_bin" lanes update "$store" "$workspace" "$lane_id" \
    --lane-status working \
    --status-report "$body" \
    --reviewer-feedback "loop feedback $iteration" \
    --updated-by "loop:agent" \
    --format text

  if (( iteration % 25 == 0 )); then
    echo "iterations=$iteration elapsed=$(( $(date +%s) - start_epoch ))s"
  fi
done

echo "completed iterations=$iteration duration=${duration_seconds}s store=$store"
echo "workload_semantics=current_overwrite:document,lane retained_history:ticket,page"
if [[ "$daemon_started" -eq 1 ]]; then
  "$loom_bin" daemon stop "$store"
  daemon_started=0
fi
"$loom_bin" store stat "$store"
"$loom_bin" store attribution "$store" "$workspace" \
  --max-objects 0 \
  --examples 0 \
  --format text
