#!/bin/bash

export LOOM_LLM_BASE_URL="http://localhost:1234/v1"
export LOOM_LLM_TOKEN="sk-lm-cL6kSL9o:OwhBkvVACocQnZbNRJqf"
export LOOM_LLM_MODEL="google/gemma-4-31b-qat"
cargo run --bin react_tools -- "What is 47 * 19, and what is the Loom spec?"