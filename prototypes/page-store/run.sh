#!/usr/bin/env bash
# D-1 page-size analysis: runs the standalone page-layout model at each candidate PAGE_SIZE (via
# LOOM_PAGE_SIZE), then prints a comparison table plus a recommendation.
#
#   ./run.sh                 # default sweep: 1024 2048 4096 8192 16384 32768
#   ./run.sh 4096 8192       # custom sizes
set -euo pipefail
cd "$(dirname "$0")"

# Default sweep starts at 4096: the index B-tree uses a fixed order-64 node (~3 KB), so pages below
# 4 KiB cannot hold one ("btree node exceeds one page"). 4 KiB is also the OS page floor.
sizes=("$@")
if [ "${#sizes[@]}" -eq 0 ]; then
  sizes=(4096 8192 16384)
fi

echo "benchmarking ${#sizes[@]} page size(s), 40000 objects each; production loom-store remains fixed at 4096 B..." >&2
rows=""
for sz in "${sizes[@]}"; do
  echo "  building + running at ${sz} B ..." >&2
  row=$(LOOM_PAGE_SIZE="$sz" cargo run --release --quiet)
  rows+="${row}"$'\n'
done

printf '\n%-12s %14s %14s %12s\n' "page_size" "put_obj/s" "get_obj/s" "bytes/obj"
printf '%s' "$rows" | awk -F'\t' 'NF==4 { printf "%-12s %14s %14s %12s\n", $1, $2, $3, $4 }'

printf '%s' "$rows" | awk -F'\t' '
  NF==4 {
    if (best_bpo == "" || $4 < best_bpo) { best_bpo = $4; best_bpo_sz = $1 }
    if ($2+0 > best_put+0)               { best_put = $2; best_put_sz = $1 }
    if ($3+0 > best_get+0)               { best_get = $3; best_get_sz = $1 }
  }
  END {
    printf "\nanalysis:\n"
    printf "  smallest file:  %s B page -> %s bytes/object\n", best_bpo_sz, best_bpo
    printf "  fastest writes: %s B page -> %s obj/s\n", best_put_sz, best_put
    printf "  fastest reads:  %s B page -> %s obj/s\n", best_get_sz, best_get
    if (best_bpo_sz == best_put_sz && best_put_sz == best_get_sz)
      printf "  recommendation: %s B - best on all three axes.\n", best_bpo_sz
    else
      printf "  recommendation: trade space (%s) vs write (%s) vs read (%s); smaller pages favor small objects.\n", best_bpo_sz, best_put_sz, best_get_sz
  }'
