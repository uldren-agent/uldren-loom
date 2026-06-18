#!/bin/bash
set -euo pipefail

export LOOM_LLM_BASE_URL="http://localhost:1234"
export LOOM_LLM_TOKEN="sk-lm-cL6kSL9o:OwhBkvVACocQnZbNRJqf"
export LOOM_LLM_MODEL="google/gemma-4-31b-qat"
export LOOM_LLM_API="rest-v1"
export LOOM_LLM_TIMEOUT_SECS="60"

if [ "$#" -eq 0 ]; then
  cargo run -- --judge lm-studio --analysis --limit 100 --script-evidence include
elif [ "$1" = "generate" ]; then
  shift
  node scripts/generate-dataset.mjs "$@"
elif [ "$1" = "full" ]; then
  shift
  node scripts/generate-dataset.mjs --seed "${SHELL_GUARD_DATASET_SEED:-20260621}"
  cargo run -- --judge lm-studio --analysis --script-evidence include --quiet "$@"
elif [ "$1" = "smoke" ]; then
  shift
  cargo run -- --judge lm-studio --analysis --limit 100 --script-evidence include "$@"
else
  cargo run -- "$@"
fi
