#!/usr/bin/env bash
# data-sources/download.sh - fetch public, open datasets that map to Loom workspace types,
# for demos and example imports. Verified URLs (mid-2026); see README.md for licenses/sizes.
#
# Usage:
#   ./download.sh list                 # show every dataset (name, workspace, size, license)
#   ./download.sh <name>               # download one dataset into ./downloads/
#   ./download.sh small                # download all "small" (<100 MB) datasets (a quick demo set)
#   ./download.sh all                  # download everything EXCEPT the huge ones (asks per >1 GB item)
#
# Nothing is downloaded unless you ask. Huge datasets (MS MARCO 1 GB, Wikidata 154 GB) are gated
# behind an explicit confirmation. Files land in data-sources/downloads/<name>/.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUT="$HERE/downloads"

# name | workspace | size | tier(small|large|huge) | license | url
# tier: small <100MB, large 100MB-2GB, huge >2GB. "git" downloads via clone.
read -r -d '' MANIFEST <<'EOF' || true
enwik8|files|36MB|small|Public domain (CC-BY-SA text)|http://mattmahoney.net/dc/enwik8.zip
flask|vcs|~60MB|git|BSD-3-Clause|https://github.com/pallets/flask.git
movielens|sql|978KB|small|GroupLens (free, non-commercial)|https://files.grouplens.org/datasets/movielens/ml-latest-small.zip
census|sql|1.8MB|small|US Gov public domain|https://www2.census.gov/programs-surveys/popest/datasets/2020-2024/counties/totals/co-est2024-alldata.csv
words|kv|5MB|small|MIT|https://norvig.com/ngrams/count_1w.txt
amazon-reviews|document|327MB|large|CC-BY-NC 4.0|https://huggingface.co/datasets/McAuley-Lab/Amazon-Reviews-2023/resolve/main/raw/review_categories/All_Beauty.jsonl
msmarco|vector|1.04GB|large|MS MARCO research license|https://msmarco.z22.web.core.windows.net/msmarcoranking/collection.tar.gz
snap-facebook|graph|218KB|small|BSD-style (research)|https://snap.stanford.edu/data/facebook_combined.txt.gz
osm-rhode-island|graph|52MB|small|ODbL 1.0|https://download.geofabrik.de/north-america/us/rhode-island-latest.osm.pbf
nyc-taxi|columnar|59MB|small|NYC TLC public|https://d37ci6vzurychx.cloudfront.net/trip-data/yellow_tripdata_2025-01.parquet
gharchive|queue|74MB|small|Public (GH Archive)|https://data.gharchive.org/2024-01-01-0.json.gz
noaa-ghcn|time-series|~1GB|large|US Gov public domain|https://www.ncei.noaa.gov/pub/data/ghcn/daily/by_year/2024.csv.gz
enwik9|cas|322MB|large|Public domain (CC-BY-SA text)|http://mattmahoney.net/dc/enwik9.zip
wikidata|document|154GB|huge|CC0|https://dumps.wikimedia.org/wikidatawiki/entities/latest-all.json.gz
EOF

usage() {
  cat <<'USAGE'
data-sources/download.sh - fetch public datasets that map to Loom workspace types.

Usage:
  ./download.sh                 show this help
  ./download.sh help            show this help
  ./download.sh list            list every dataset (name, workspace, size, tier, license)
  ./download.sh <name>          download one dataset (e.g. ./download.sh census)
  ./download.sh small           download all "small" (<100 MB) datasets - the quick demo set
  ./download.sh all             download everything except the huge (>2 GB) datasets

Notes:
  - Files land in data-sources/downloads/<name>/. Nothing downloads unless you ask.
  - "huge" datasets (e.g. wikidata, 154 GB) prompt for confirmation first.
  - Run `./download.sh list` to see dataset names to pass to `<name>`.
USAGE
}

row() { grep "^$1|" <<<"$MANIFEST" || true; }

list() {
  printf '%-18s %-12s %-8s %-6s %s\n' NAME WORKSPACE SIZE TIER LICENSE
  printf '%-18s %-12s %-8s %-6s %s\n' ------ --------- ---- ---- -------
  while IFS='|' read -r name ns size tier lic _url; do
    [ -z "$name" ] && continue
    printf '%-18s %-12s %-8s %-6s %s\n' "$name" "$ns" "$size" "$tier" "$lic"
  done <<<"$MANIFEST"
}

fetch_one() {
  local name="$1" r ns size tier lic url
  r="$(row "$name")"; [ -z "$r" ] && { echo "unknown dataset: $name (try: ./download.sh list)"; return 1; }
  IFS='|' read -r _ ns size tier lic url <<<"$r"
  local dest="$OUT/$name"; mkdir -p "$dest"
  echo ">> $name  [$ns]  $size  ($lic)"
  if [ "$tier" = "git" ]; then
    if [ -d "$dest/.git" ]; then git -C "$dest" pull --ff-only; else git clone --depth 1 "$url" "$dest"; fi
    return 0
  fi
  if [ "$tier" = "huge" ]; then
    read -r -p "   $name is $size. Download anyway? [y/N] " ans
    [[ "$ans" =~ ^[Yy]$ ]] || { echo "   skipped"; return 0; }
  fi
  local file="$dest/$(basename "$url")"
  if command -v curl >/dev/null; then curl -fL --retry 3 -o "$file" "$url"
  else wget -O "$file" "$url"; fi
  echo "   -> $file"
}

cmd="${1:-help}"
case "$cmd" in
  help|-h|--help) usage ;;
  list) list ;;
  small) while IFS='|' read -r name _ _ tier _ _; do [ "$tier" = small ] && fetch_one "$name"; done <<<"$MANIFEST" ;;
  all)   while IFS='|' read -r name _ _ tier _ _; do [ -n "$name" ] && [ "$tier" != huge ] && fetch_one "$name"; done <<<"$MANIFEST" ;;
  *)     fetch_one "$cmd" ;;
esac
