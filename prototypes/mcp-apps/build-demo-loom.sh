#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  printf 'usage: %s <output.loom> [namespace]\n' "$0" >&2
  exit 2
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
OUTPUT_LOOM="$1"
NAMESPACE="${2:-apps}"

cargo run --quiet --manifest-path "$REPO_ROOT/Cargo.toml" -p uldren-loom-mcp --example import_mcp_apps -- "$OUTPUT_LOOM" "$SCRIPT_DIR/apps" "$NAMESPACE"
