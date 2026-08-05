#!/usr/bin/env bash
set -euo pipefail

iterations="${1:-8}"
if ! [[ "$iterations" =~ ^[0-9]+$ ]] || [[ "$iterations" -le 0 ]]; then
  echo "usage: $0 [iterations]" >&2
  exit 2
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
loom_bin="${LOOM_BIN:-$script_dir/loop/loom}"
store="${LOOP_STORE:-random.loom}"
daemon_transport="${LOOP_DAEMON_TRANSPORT:-native}"
workspace="${LOOP_WORKSPACE:-random}"
project_id="random"
space_id="random-space"
document_collection="random-documents"

if [[ ! -x "$loom_bin" ]]; then
  echo "loom binary is not executable: $loom_bin" >&2
  echo "copy the Loom binary to $script_dir/loop/loom or set LOOM_BIN=/path/to/loom" >&2
  exit 1
fi

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/loom-random-probe.XXXXXX")"
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

file_size() {
  if [[ -f "$store" ]]; then
    stat -f "%z" "$store"
  else
    printf '0'
  fi
}

before_bytes="$(file_size)"

if [[ ! -f "$store" ]]; then
  echo "initializing $store"
  "$loom_bin" store init "$store"
  "$loom_bin" workspace create "$store" "$workspace" --facet document
  "$loom_bin" tickets project-create "$store" "$workspace" "$project_id" RND "Random Probe" --format text
  "$loom_bin" pages space-create "$store" "$workspace" "$space_id" "Random Probe Space" --format text
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

payload_bytes=0

for (( iteration = 1; iteration <= iterations; iteration++ )); do
  nonce="$(uuidgen | tr '[:upper:]' '[:lower:]')"
  page_id="random-page-$iteration-$nonce"
  document_id="random-document-$iteration-$nonce"
  lane_id="random-lane-$iteration-$nonce"
  body="random iteration $iteration nonce $nonce at $(date +%s000)"

  write_text_file "$tmpdir/document.txt" "$body"
  write_text_file "$tmpdir/page.txt" "# Random Probe Page $iteration

$body"

  payload_bytes="$((payload_bytes + ${#body}))"
  payload_bytes="$((payload_bytes + $(wc -c < "$tmpdir/document.txt")))"
  payload_bytes="$((payload_bytes + $(wc -c < "$tmpdir/page.txt")))"

  ticket_json="$("$loom_bin" tickets create "$store" "$workspace" task \
    --project-id "$project_id" \
    --title "Random probe ticket $iteration" \
    --description "$body" \
    --priority P1 \
    --format json)"
  ticket_id="$(printf '%s\n' "$ticket_json" | sed -n 's/.*"primary_key"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"
  if [[ -z "$ticket_id" ]]; then
    echo "could not read created ticket id from tickets create output" >&2
    exit 1
  fi

  "$loom_bin" pages create "$store" "$workspace" "$page_id" "$space_id" "Random Probe Page $iteration" --format text >/dev/null
  "$loom_bin" pages update "$store" "$workspace" "$page_id" "$tmpdir/page.txt" --format text >/dev/null
  "$loom_bin" pages publish "$store" "$workspace" "$page_id" --format text >/dev/null

  "$loom_bin" document put-text "$store" "$workspace" "$document_collection" "$document_id" "$tmpdir/document.txt" >/dev/null

  "$loom_bin" lanes create "$store" "$workspace" "$lane_id" "$lane_id" \
    --kind assignment \
    --owner-principal "random:agent" \
    --title "Random Probe Lane $iteration" \
    --description "$body" \
    --lane-status ready \
    --status-report "$body" \
    --reviewer-feedback "" \
    --ticket "$ticket_id" \
    --updated-by "random:agent" \
    --format text >/dev/null
done

after_bytes="$(file_size)"
growth_bytes="$((after_bytes - before_bytes))"

echo "completed iterations=$iterations store=$store"
echo "before_bytes=$before_bytes"
echo "after_bytes=$after_bytes"
echo "growth_bytes=$growth_bytes"
echo "approx_payload_bytes=$payload_bytes"
if [[ "$daemon_started" -eq 1 ]]; then
  "$loom_bin" daemon stop "$store"
  daemon_started=0
fi
"$loom_bin" store stat "$store"
"$loom_bin" store attribution "$store" "$workspace" --format text --examples 6
