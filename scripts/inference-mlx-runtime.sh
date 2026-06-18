#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  scripts/inference-mlx-runtime.sh check
  scripts/inference-mlx-runtime.sh refresh
  scripts/inference-mlx-runtime.sh stage
  scripts/inference-mlx-runtime.sh build-shared
  scripts/inference-mlx-runtime.sh clean-build

Purpose:
  Build or stage the optional Apple MLX native runtime bundle used by Loom.

Modes:
  check         Validate host, source or prefix, and report what can be staged.
  refresh       Clone or refresh the mlx-c source checkout.
  stage         Copy a shared MLX C runtime bundle into crates/loom-inference/native/mlx.
  build-shared  Build MLX C from source with shared libraries enabled, then stage it.
  clean-build   Refresh source, remove the managed build/install dirs, rebuild, and stage.

Environment:
  MLX_C_PREFIX       Installed MLX C prefix. Defaults to /opt/mlx-c.
  MLX_C_REPO         Git URL used when MLX_C_SOURCE is missing.
  MLX_C_REF          Git ref to check out. Defaults to main.
  MLX_C_SOURCE       Local mlx-c source checkout. Defaults under target/inference/mlx/src.
  LOOM_MLX_OUT       Bundle output directory.
  LOOM_MLX_BUILD_DIR Build directory. Defaults to target/inference/mlx.

Notes:
  stage requires libmlx.dylib, libmlxc.dylib, and mlx.metallib under MLX_C_PREFIX.
  /opt/mlx-c installs that only contain static archives are reported but not
  repackaged into a shared runtime. Use build-shared from a matching mlx-c source
  checkout to create a refreshable dynamic bundle.
USAGE
}

fail() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

info() {
  printf '%s\n' "$*"
}

hash_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    shasum -a 256 "$1" | awk '{ print $1 }'
  fi
}

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
mode="${1:-check}"

case "$mode" in
  check | refresh | stage | build-shared | clean-build) ;;
  -h | --help | help)
    usage
    exit 0
    ;;
  *)
    usage >&2
    fail "unknown mode: $mode"
    ;;
esac

host_os="$(uname -s)"
host_arch="$(uname -m)"
if [ "$host_os" != "Darwin" ]; then
  fail "MLX runtime bundles for Loom must be built on macOS, not $host_os"
fi
case "$host_arch" in
  arm64 | aarch64) ;;
  *) fail "MLX runtime bundles require Apple silicon, not $host_arch" ;;
esac

if ! command -v cmake >/dev/null 2>&1; then
  fail "cmake is required"
fi
if ! command -v c++ >/dev/null 2>&1; then
  fail "the Xcode command line C++ compiler is required"
fi
if ! xcrun -sdk macosx metal -v >/dev/null 2>&1; then
  fail "the macOS Metal toolchain is required"
fi

target_triple="$(rustc -vV 2>/dev/null | awk '/^host:/ { print $2 }')"
if [ -z "$target_triple" ]; then
  target_triple="aarch64-apple-darwin"
fi

prefix="${MLX_C_PREFIX:-/opt/mlx-c}"
repo_url="${MLX_C_REPO:-https://github.com/ml-explore/mlx-c.git}"
repo_ref="${MLX_C_REF:-main}"
build_dir="${LOOM_MLX_BUILD_DIR:-$repo_root/target/inference/mlx}"
source_dir="${MLX_C_SOURCE:-$build_dir/src/mlx-c}"
out_dir="${LOOM_MLX_OUT:-$repo_root/crates/loom-inference/native/mlx/$target_triple}"
managed_source=0
if [ -z "${MLX_C_SOURCE:-}" ]; then
  managed_source=1
fi

report_prefix() {
  info "MLX_C_PREFIX=$prefix"
  if [ -d "$prefix" ]; then
    du -sh "$prefix" 2>/dev/null || true
  else
    info "prefix status: missing"
  fi
  for path in \
    "$prefix/lib/libmlx.dylib" \
    "$prefix/lib/libmlxc.dylib" \
    "$prefix/lib/libjaccl.dylib" \
    "$prefix/lib/libmlx.a" \
    "$prefix/lib/libmlxc.a" \
    "$prefix/lib/mlx.metallib" \
    "$prefix/share/cmake/MLX/MLXConfig.cmake" \
    "$prefix/share/cmake/MLXC/MLXCConfig.cmake"
  do
    if [ -e "$path" ]; then
      ls -lh "$path"
    fi
  done
  info "MLX_C_SOURCE=$source_dir"
  if [ -d "$source_dir/.git" ]; then
    git -C "$source_dir" rev-parse --short HEAD 2>/dev/null | awk '{ print "source git head="$1 }'
    git -C "$source_dir" status --short 2>/dev/null | sed 's/^/source status: /'
  elif [ -d "$source_dir" ]; then
    info "source status: directory exists but is not a git checkout"
  else
    info "source status: missing"
  fi
}

require_shared_bundle() {
  [ -f "$prefix/lib/libmlx.dylib" ] || fail "missing $prefix/lib/libmlx.dylib"
  [ -f "$prefix/lib/libmlxc.dylib" ] || fail "missing $prefix/lib/libmlxc.dylib"
  [ -f "$prefix/lib/mlx.metallib" ] || fail "missing $prefix/lib/mlx.metallib"
}

stage_bundle() {
  require_shared_bundle
  mkdir -p "$out_dir"
  for dylib in "$prefix"/lib/*.dylib; do
    if [ -f "$dylib" ]; then
      cp -f "$dylib" "$out_dir/$(basename "$dylib")"
    fi
  done
  cp -f "$prefix/lib/mlx.metallib" "$out_dir/mlx.metallib"

  {
    printf 'LOOM_MLX_BUNDLE_DIR=%s\n' "$out_dir"
    printf 'DYLD_LIBRARY_PATH=%s:${DYLD_LIBRARY_PATH:-}\n' "$out_dir"
  } > "$out_dir/bundle.env"

  {
    printf 'target=%s\n' "$target_triple"
    printf 'source_prefix=%s\n' "$prefix"
    printf 'bundle_dir=%s\n' "$out_dir"
    printf 'checksums=checksums.sha256\n'
    printf '\nfiles:\n'
    ls -lh "$out_dir"
    for dylib in "$out_dir"/*.dylib; do
      if [ -f "$dylib" ]; then
        printf '\n%s linkage:\n' "$(basename "$dylib")"
        otool -L "$dylib"
      fi
    done
  } > "$out_dir/manifest.txt"

  : > "$out_dir/checksums.sha256"
  find "$out_dir" -maxdepth 1 -type f ! -name 'checksums.sha256' -print | sort | while IFS= read -r file; do
    printf '%s  %s\n' "$(hash_file "$file")" "$(basename "$file")" >> "$out_dir/checksums.sha256"
  done

  info "staged MLX runtime bundle at $out_dir"
}

clean_managed_build() {
  case "$build_dir" in
    "$repo_root"/target/inference/mlx | /tmp/* | /private/tmp/*) ;;
    *)
      fail "refusing to clean unmanaged build dir: $build_dir"
      ;;
  esac
  rm -rf "$build_dir/build" "$build_dir/install"
}

refresh_source() {
  if ! command -v git >/dev/null 2>&1; then
    fail "git is required to refresh mlx-c"
  fi
  mkdir -p "$(dirname "$source_dir")"
  if [ -d "$source_dir/.git" ]; then
    info "refreshing mlx-c source at $source_dir"
    git -C "$source_dir" fetch --tags --prune origin
  elif [ -e "$source_dir" ]; then
    fail "MLX_C_SOURCE exists but is not a git checkout: $source_dir"
  else
    info "cloning mlx-c from $repo_url into $source_dir"
    git clone "$repo_url" "$source_dir"
    git -C "$source_dir" fetch --tags --prune origin
  fi

  if git -C "$source_dir" show-ref --verify --quiet "refs/remotes/origin/$repo_ref"; then
    git -C "$source_dir" checkout -B "$repo_ref" "origin/$repo_ref"
  else
    git -C "$source_dir" checkout "$repo_ref"
  fi
  git -C "$source_dir" rev-parse --short HEAD | awk '{ print "mlx-c source head="$1 }'
}

ensure_source() {
  [ -f "$source_dir/CMakeLists.txt" ] || fail "MLX_C_SOURCE does not contain CMakeLists.txt: $source_dir"
}

build_shared() {
  if [ "$managed_source" -eq 1 ] && [ ! -f "$source_dir/CMakeLists.txt" ]; then
    refresh_source
  fi
  ensure_source

  install_prefix="$build_dir/install"
  cmake -S "$source_dir" \
    -B "$build_dir/build" \
    -DCMAKE_BUILD_TYPE=Release \
    -DBUILD_SHARED_LIBS=ON \
    -DMLX_C_BUILD_EXAMPLES=OFF \
    -DCMAKE_INSTALL_PREFIX="$install_prefix"
  cmake --build "$build_dir/build" --config Release -j "$(sysctl -n hw.ncpu)"
  cmake --install "$build_dir/build" --prefix "$install_prefix"
  prefix="$install_prefix"
}

case "$mode" in
  check)
    report_prefix
    if [ -f "$prefix/lib/libmlx.dylib" ] && [ -f "$prefix/lib/libmlxc.dylib" ]; then
      info "shared bundle status: stageable"
    elif [ -f "$prefix/lib/libmlx.a" ] || [ -f "$prefix/lib/libmlxc.a" ]; then
      info "shared bundle status: static-only install, use build-shared from mlx-c source"
    else
      info "shared bundle status: not found"
    fi
    ;;
  refresh)
    refresh_source
    ;;
  stage)
    stage_bundle
    ;;
  build-shared)
    build_shared
    stage_bundle
    ;;
  clean-build)
    refresh_source
    clean_managed_build
    build_shared
    stage_bundle
    ;;
esac
