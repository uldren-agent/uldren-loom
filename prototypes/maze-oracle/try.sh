#!/bin/bash
set -euo pipefail

export LOOM_LLM_BASE_URL="http://localhost:1234"
export LOOM_LLM_TOKEN="sk-lm-cL6kSL9o:OwhBkvVACocQnZbNRJqf"
export LOOM_LLM_MODEL="google/gemma-4-31b-qat"
export LOOM_LLM_API="rest-v1"
export LOOM_LLM_TIMEOUT_SECS="120"

if [ "$#" -eq 0 ]; then
  #cargo run -- --oracle lm-studio --analysis --sizes 7,11,21 --path-limit 12 --retries 0
  #cargo run -- --oracle lm-studio --analysis --sizes 2,3,5,7 --path-limit 6 --retries 0
  #cargo run -- --oracle lm-studio --analysis --sizes 2,3,5,7 --path-limit 6 --visibility full --exit-known --retries 0
  cargo run -- --oracle lm-studio --execution guarded --analysis --sizes 2,3,5,7 --path-limit 1 --visibility full --exit-known --max-calls 20 --retries 0
else
  cargo run -- "$@"
fi
