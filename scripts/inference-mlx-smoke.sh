#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  scripts/inference-mlx-smoke.sh

Purpose:
  Build and run a manual MLX C smoke test against the staged Loom MLX bundle.

Environment:
  MLX_C_SOURCE              Local mlx-c source checkout.
  LOOM_MLX_BUNDLE_DIR       Staged MLX runtime bundle.
  LOOM_MLX_SMOKE_BUILD_DIR  Build directory for the smoke binary.

The smoke links against libmlxc.dylib, libmlx.dylib, and libjaccl.dylib from
the staged bundle, then runs a small MLX array operation and verifies the
result. It does not require the future libloom_mlx_adapter.dylib.
USAGE
}

fail() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"

case "${1:-}" in
  -h | --help | help)
    usage
    exit 0
    ;;
  "") ;;
  *) usage >&2 && fail "unknown argument: $1" ;;
esac

host_os="$(uname -s)"
host_arch="$(uname -m)"
if [ "$host_os" != "Darwin" ]; then
  fail "MLX smoke must run on macOS, not $host_os"
fi
case "$host_arch" in
  arm64 | aarch64) ;;
  *) fail "MLX smoke requires Apple silicon, not $host_arch" ;;
esac

if ! command -v cc >/dev/null 2>&1; then
  fail "cc is required"
fi

target_triple="$(rustc -vV 2>/dev/null | awk '/^host:/ { print $2 }')"
if [ -z "$target_triple" ]; then
  target_triple="aarch64-apple-darwin"
fi

source_dir="${MLX_C_SOURCE:-$repo_root/target/inference/mlx/src/mlx-c}"
bundle_dir="${LOOM_MLX_BUNDLE_DIR:-$repo_root/crates/loom-inference/native/mlx/$target_triple}"
build_dir="${LOOM_MLX_SMOKE_BUILD_DIR:-$repo_root/target/inference/mlx/smoke}"
smoke_src="$script_dir/inference-mlx-smoke.c"
smoke_bin="$build_dir/inference-mlx-smoke"

[ -f "$source_dir/mlx/c/mlx.h" ] || fail "missing mlx-c headers at $source_dir"
[ -f "$bundle_dir/libmlxc.dylib" ] || fail "missing $bundle_dir/libmlxc.dylib"
[ -f "$bundle_dir/libmlx.dylib" ] || fail "missing $bundle_dir/libmlx.dylib"
[ -f "$bundle_dir/libjaccl.dylib" ] || fail "missing $bundle_dir/libjaccl.dylib"
[ -f "$bundle_dir/mlx.metallib" ] || fail "missing $bundle_dir/mlx.metallib"

mkdir -p "$build_dir"
cc -std=c11 \
  "$smoke_src" \
  -I "$source_dir" \
  -L "$bundle_dir" \
  -Wl,-rpath,"$bundle_dir" \
  -lmlxc \
  -lmlx \
  -ljaccl \
  -o "$smoke_bin"

printf 'mlx_smoke_binary=%s\n' "$smoke_bin"
DYLD_LIBRARY_PATH="$bundle_dir:${DYLD_LIBRARY_PATH:-}" "$smoke_bin"
