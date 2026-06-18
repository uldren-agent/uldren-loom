#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/inference-smoke.sh [--cli] [--config] [--no-test]

Runs rerunnable inference smoke checks against the shared Hugging Face cache.

Options:
  --cli        Also run CLI download, list, and doctor checks.
  --config     Also probe instance and vector namespace config commands.
  --no-test    Skip activation tests and only run selected CLI checks.
  -h, --help   Show this help.

Environment:
  HF_HOME              Optional Hugging Face cache root.
  HF_TOKEN             Optional token for gated or private models.
  CARGO_TARGET_DIR     Optional Cargo target directory.
  LOOM_SMOKE_CLI       Optional path to a prebuilt loom binary.
USAGE
}

run_cli=0
run_config=0
run_tests=1

while [ "$#" -gt 0 ]; do
  case "$1" in
    --cli)
      run_cli=1
      ;;
    --config)
      run_config=1
      run_cli=1
      ;;
    --no-test)
      run_tests=0
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
target_dir="${CARGO_TARGET_DIR:-/tmp/loom-inference-smoke-target}"
loom_bin="${LOOM_SMOKE_CLI:-}"

cargo_env=(env "CARGO_TARGET_DIR=$target_dir")
cargo_features="candle-cpu,native-hf"

cd "$repo_root"

run_activation_test() {
  local test_name="$1"
  "${cargo_env[@]}" cargo test \
    -p uldren-loom-inference \
    --features "$cargo_features" \
    --test network_smoke \
    "$test_name" \
    -- \
    --ignored \
    --nocapture
}

run_loom() {
  if [ -n "$loom_bin" ]; then
    "$loom_bin" "$@"
  else
    "${cargo_env[@]}" cargo run \
      -p uldren-loom-cli \
      --no-default-features \
      --features "nfs,inference-native-hf" \
      -- "$@"
  fi
}

run_cli_embedding() {
  run_loom inference model download \
    sentence-transformers/all-MiniLM-L6-v2 \
    config.json model.safetensors tokenizer.json tokenizer_config.json special_tokens_map.json \
    --kind text-embedding \
    --runtime candle-safetensors \
    --foreground

  run_loom inference model list \
    --kind text-embedding \
    --format json
}

run_cli_llm() {
  run_loom inference model download \
    Qwen/Qwen2.5-0.5B-Instruct \
    config.json generation_config.json model.safetensors tokenizer.json tokenizer_config.json vocab.json merges.txt \
    --kind llm \
    --runtime candle-safetensors \
    --foreground

  run_loom inference model list \
    --kind llm \
    --format json
}

run_cli_config() {
  if run_loom inference instance show smoke-embedding --format json >/dev/null 2>&1; then
    run_loom inference instance update smoke-embedding --preset fast
  else
    run_loom inference instance create smoke-embedding \
      --model sentence-transformers/all-MiniLM-L6-v2 \
      --kind text-embedding \
      --runtime candle-safetensors \
      --preset fast
  fi

  run_loom vector namespace configure /tmp/loom-inference-smoke.loom smoke \
    --embedding-instance smoke-embedding

  if run_loom inference instance show smoke-llm --format json >/dev/null 2>&1; then
    run_loom inference instance update smoke-llm --preset fast --set max_tokens=8
  else
    run_loom inference instance create smoke-llm \
      --model Qwen/Qwen2.5-0.5B-Instruct \
      --kind llm \
      --runtime candle-safetensors \
      --preset fast \
      --set max_tokens=8
  fi
}

if [ "$run_tests" = "1" ]; then
  run_activation_test downloaded_embedding_model_activates_with_candle
  run_activation_test downloaded_qwen2_model_completes_with_candle
fi

if [ "$run_cli" = "1" ]; then
  run_cli_embedding
  run_cli_llm
  if [ "$run_config" = "1" ]; then
    run_cli_config
  fi
  run_loom doctor --format json
fi
