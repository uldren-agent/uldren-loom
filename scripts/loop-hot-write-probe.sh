#!/usr/bin/env bash
set -euo pipefail

minutes="${1:-2}"
if ! [[ "$minutes" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
  echo "usage: $0 [minutes]" >&2
  exit 2
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
loom_bin="${LOOM_BIN:-$repo_root/target/debug/loom}"
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
  echo "set LOOM_BIN=/path/to/loom or build ./target/debug/loom" >&2
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

if [[ ! -f "$store" ]]; then
  echo "initializing $store"
  "$loom_bin" store init "$store"
  "$loom_bin" workspace create "$store" "$workspace" --facet document
  "$loom_bin" tickets project-create "$store" "$workspace" "$project_id" LOOP "Loop Probe" --format text
  "$loom_bin" tickets create "$store" "$workspace" task \
    --project-id "$project_id" \
    --title "Loop probe ticket" \
    --description "Initial loop probe ticket." \
    --priority P1 \
    --format text
  "$loom_bin" pages space-create "$store" "$workspace" "$space_id" "Loop Probe Space" --format text
  "$loom_bin" pages create "$store" "$workspace" "$page_id" "$space_id" "Loop Probe Page" --format text
  write_text_file "$tmpdir/document.txt" "initial loop document"
  "$loom_bin" document put-text "$store" "$workspace" "$document_collection" "$document_id" "$tmpdir/document.txt"
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
else
  echo "using existing $store"
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

  "$loom_bin" tickets update "$store" "$workspace" "$ticket_id" \
    --title "Loop probe ticket $iteration" \
    --description "$body" \
    --status in_progress \
    --priority P1 \
    --format text >/dev/null

  "$loom_bin" pages update "$store" "$workspace" "$page_id" "$tmpdir/page.txt" --format text >/dev/null
  "$loom_bin" pages publish "$store" "$workspace" "$page_id" --format text >/dev/null

  "$loom_bin" document put-text "$store" "$workspace" "$document_collection" "$document_id" "$tmpdir/document.txt" >/dev/null

  "$loom_bin" lanes update "$store" "$workspace" "$lane_id" \
    --lane-status working \
    --status-report "$body" \
    --reviewer-feedback "loop feedback $iteration" \
    --updated-by "loop:agent" \
    --format text >/dev/null

  if (( iteration % 25 == 0 )); then
    echo "iterations=$iteration elapsed=$(( $(date +%s) - start_epoch ))s"
  fi
done

echo "completed iterations=$iteration duration=${duration_seconds}s store=$store"
echo "workload_semantics=current_overwrite:document,lane retained_history:ticket,page"
"$loom_bin" store stat "$store"
"$loom_bin" store attribution "$store" "$workspace" \
  --max-objects 0 \
  --examples 0 \
  --format text
