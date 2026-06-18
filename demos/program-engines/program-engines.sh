#!/usr/bin/env bash
set -euo pipefail

demo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
store="${1:-${demo_dir}/program.loom}"

cargo run --offline --manifest-path "${demo_dir}/Cargo.toml" -- build "${store}"
cargo run --offline --manifest-path "${demo_dir}/Cargo.toml" -- call-wasm "${store}"
cargo run --offline --manifest-path "${demo_dir}/Cargo.toml" -- call-template "${store}"
cargo run --offline --manifest-path "${demo_dir}/Cargo.toml" -- call-cel "${store}"
cargo run --offline --manifest-path "${demo_dir}/Cargo.toml" -- list "${store}"
