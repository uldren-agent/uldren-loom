#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/inference-vector-demo.sh [options]

Runs a text embedding demo through the loom CLI and vector facet.

Options:
  --query TEXT       Question to embed and search.
  --data PATH        UTF-8 TSV file of "id<TAB>text" records.
  --top-k N          Number of nearest records to print. Defaults to 3.
  --model REPO_ID    Curated text-embedding repo id.
  --instance NAME    Named text-embedding instance. Defaults to loom-vector-demo.
  --store PATH       Demo .loom path. Defaults under target/inference-vector-demo.
  --check            Build the CLI and validate command parsing without downloading.
  -h, --help         Show this help.

Environment:
  HF_HOME            Shared Hugging Face cache root used by loom-inference.
  HF_TOKEN           Optional token for gated or private models.
  CARGO_TARGET_DIR   Optional Cargo target directory.

The instance store currently follows HF_HOME. This script intentionally uses that shared cache path
instead of a script-private cache override so model records, instances, and vector bindings agree.
USAGE
}

query="Which Loom feature stores semantic search text?"
data_path=""
top_k="3"
model="sentence-transformers/all-MiniLM-L6-v2"
instance="loom-vector-demo"
store_path=""
check_only=0

while [ "$#" -gt 0 ]; do
  case "$1" in
    --query)
      query="${2:?missing value for --query}"
      shift
      ;;
    --data)
      data_path="${2:?missing value for --data}"
      shift
      ;;
    --top-k)
      top_k="${2:?missing value for --top-k}"
      shift
      ;;
    --model)
      model="${2:?missing value for --model}"
      shift
      ;;
    --instance)
      instance="${2:?missing value for --instance}"
      shift
      ;;
    --store)
      store_path="${2:?missing value for --store}"
      shift
      ;;
    --check)
      check_only=1
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
demo_dir="$target_dir/inference-vector-demo"
loom_bin="$target_dir/debug/loom"

mkdir -p "$demo_dir"

if [ -z "$store_path" ]; then
  store_path="$demo_dir/demo.loom"
fi

case "$model" in
  sentence-transformers/all-MiniLM-L6-v2|BAAI/bge-small-en-v1.5)
    model_files=(
      config.json
      model.safetensors
      special_tokens_map.json
      tokenizer.json
      tokenizer_config.json
    )
    ;;
  *)
    echo "unsupported curated text embedding model: $model" >&2
    echo "supported: sentence-transformers/all-MiniLM-L6-v2, BAAI/bge-small-en-v1.5" >&2
    exit 2
    ;;
esac

build_cli() {
  cargo build \
    -p uldren-loom-cli \
    --no-default-features \
    --features inference-native-hf,inference-candle-cpu \
    --bin loom
}

ensure_sample_data() {
  local path="$1"
  cat > "$path" <<'DATA'
vector-facet	The Loom vector facet stores embeddings, source text, metadata, and nearest-neighbor search indexes.
inference-downloads	The inference download path installs curated Hugging Face models into the shared cache and activates text embeddings.
doctor	The doctor command reports hardware, inference runtime support, model fit, and MLX bundle health by default.
instances	Named inference instances bind model artifacts, runtime choices, presets, and explicit provider settings.
unrelated	Calendar and contacts facets store personal information management records such as events, tasks, and address cards.
DATA
}

model_installed() {
  "$loom_bin" inference model show "$model" \
    --kind text-embedding \
    --runtime candle-safetensors \
    >/dev/null 2>&1
}

ensure_model() {
  if model_installed; then
    echo "using installed model: $model" >&2
    return
  fi

  echo "downloading model: $model" >&2
  "$loom_bin" inference model download "$model" \
    --kind text-embedding \
    --runtime candle-safetensors \
    --foreground \
    "${model_files[@]}"
}

ensure_instance() {
  if "$loom_bin" inference instance show "$instance" >/dev/null 2>&1; then
    echo "using existing instance: $instance" >&2
    return
  fi

  "$loom_bin" inference instance create "$instance" \
    --kind text-embedding \
    --model "$model" \
    --runtime candle-safetensors \
    --preset fast
}

load_records() {
  local path="$1"
  while IFS=$'\t' read -r id text extra || [ -n "${id:-}" ]; do
    if [ -z "${id:-}" ] && [ -z "${text:-}" ]; then
      continue
    fi
    if [ -n "${extra:-}" ] || [ -z "${id:-}" ] || [ -z "${text:-}" ]; then
      echo "invalid TSV row in $path; expected id<TAB>text" >&2
      exit 2
    fi
    "$loom_bin" vector text upsert "$store_path" \
      --namespace demo \
      notes \
      "$id" \
      --text "$text" \
      --embedding-instance "$instance" \
      --create
  done < "$path"
}

build_cli

if [ "$check_only" = "1" ]; then
  "$loom_bin" inference model list --remote --kind text-embedding >/dev/null
  "$loom_bin" vector text query --help >/dev/null
  exit 0
fi

if [ -z "$data_path" ]; then
  data_path="$demo_dir/sample.tsv"
  ensure_sample_data "$data_path"
fi

rm -f "$store_path"
"$loom_bin" store init "$store_path" >/dev/null
ensure_model
ensure_instance
"$loom_bin" vector namespace configure "$store_path" demo --embedding-instance "$instance" >/dev/null
load_records "$data_path"

"$loom_bin" vector text query "$store_path" \
  --namespace demo \
  notes \
  --query "$query" \
  --top-k "$top_k"
