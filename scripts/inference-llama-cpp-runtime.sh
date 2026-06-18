#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  scripts/inference-llama-cpp-runtime.sh check
  scripts/inference-llama-cpp-runtime.sh refresh
  scripts/inference-llama-cpp-runtime.sh stage
  scripts/inference-llama-cpp-runtime.sh build-shared
  scripts/inference-llama-cpp-runtime.sh clean-build

Purpose:
  Build or stage the optional llama.cpp native runtime bundle used by Loom.

Modes:
  check         Validate host, source or prefix, and report what can be staged.
  refresh       Clone or refresh the llama.cpp source checkout.
  stage         Copy a shared llama.cpp runtime bundle into crates/loom-inference/native/llama-cpp.
  build-shared  Build llama.cpp from source with shared libraries enabled, then stage it.
  clean-build   Refresh source, remove the managed build/install dirs, rebuild, and stage.

Environment:
  LLAMA_CPP_PREFIX       Installed llama.cpp prefix. Defaults to target/inference/llama-cpp/install.
  LLAMA_CPP_REPO         Git URL used when LLAMA_CPP_SOURCE is missing.
  LLAMA_CPP_REF          Git ref to check out. Defaults to master.
  LLAMA_CPP_SOURCE       Local llama.cpp source checkout. Defaults under target/inference/llama-cpp/src.
  LOOM_LLAMA_CPP_OUT     Bundle output directory.
  LOOM_LLAMA_CPP_BUILD_DIR
                          Build directory. Defaults to target/inference/llama-cpp.

Notes:
  stage requires shared libllama, libggml, libggml-base, and libggml-cpu runtime libraries.
  The bundle is optional and ignored by git. Loom loads it through the loom-native boundary.
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

if ! command -v cmake >/dev/null 2>&1; then
  fail "cmake is required"
fi
if ! command -v c++ >/dev/null 2>&1; then
  fail "a C++ compiler is required"
fi

host_os="$(uname -s)"
case "$host_os" in
  Darwin) dylib_glob="*.dylib" ;;
  Linux) dylib_glob="*.so" ;;
  MINGW* | MSYS* | CYGWIN*) dylib_glob="*.dll" ;;
  *) fail "unsupported llama.cpp bundle host OS: $host_os" ;;
esac

target_triple="$(rustc -vV 2>/dev/null | awk '/^host:/ { print $2 }')"
if [ -z "$target_triple" ]; then
  case "$host_os" in
    Darwin) target_triple="aarch64-apple-darwin" ;;
    Linux) target_triple="$(uname -m)-unknown-linux-gnu" ;;
    *) target_triple="unknown" ;;
  esac
fi

build_dir="${LOOM_LLAMA_CPP_BUILD_DIR:-$repo_root/target/inference/llama-cpp}"
prefix="${LLAMA_CPP_PREFIX:-$build_dir/install}"
repo_url="${LLAMA_CPP_REPO:-https://github.com/ggml-org/llama.cpp.git}"
repo_ref="${LLAMA_CPP_REF:-master}"
source_dir="${LLAMA_CPP_SOURCE:-$build_dir/src/llama.cpp}"
out_dir="${LOOM_LLAMA_CPP_OUT:-$repo_root/crates/loom-inference/native/llama-cpp/$target_triple}"
managed_source=0
if [ -z "${LLAMA_CPP_SOURCE:-}" ]; then
  managed_source=1
fi

runtime_lib_names() {
  case "$host_os" in
    Darwin)
      printf '%s\n' libllama.dylib libggml.dylib libggml-base.dylib libggml-cpu.dylib
      ;;
    Linux)
      printf '%s\n' libllama.so libggml.so libggml-base.so libggml-cpu.so
      ;;
    *)
      printf '%s\n' llama.dll ggml.dll ggml-base.dll ggml-cpu.dll
      ;;
  esac
}

report_prefix() {
  info "LLAMA_CPP_PREFIX=$prefix"
  if [ -d "$prefix" ]; then
    du -sh "$prefix" 2>/dev/null || true
  else
    info "prefix status: missing"
  fi
  while IFS= read -r name; do
    if [ -d "$prefix" ]; then
      find "$prefix" -type f -name "$name" -print 2>/dev/null | while IFS= read -r path; do
        ls -lh "$path"
      done
    fi
  done < <(runtime_lib_names)
  info "LLAMA_CPP_SOURCE=$source_dir"
  if [ -d "$source_dir/.git" ]; then
    git -C "$source_dir" rev-parse --short HEAD 2>/dev/null | awk '{ print "source git head="$1 }'
    git -C "$source_dir" status --short 2>/dev/null | sed 's/^/source status: /'
  elif [ -d "$source_dir" ]; then
    info "source status: directory exists but is not a git checkout"
  else
    info "source status: missing"
  fi
}

find_runtime_lib() {
  local name="$1"
  find "$prefix" -type f -name "$name" -print -quit 2>/dev/null || true
}

require_shared_bundle() {
  while IFS= read -r name; do
    path="$(find_runtime_lib "$name")"
    [ -n "$path" ] || fail "missing $name under $prefix"
  done < <(runtime_lib_names)
}

stage_bundle() {
  require_shared_bundle
  mkdir -p "$out_dir"
  while IFS= read -r name; do
    path="$(find_runtime_lib "$name")"
    cp -f "$path" "$out_dir/$name"
  done < <(runtime_lib_names)

  find "$prefix" -type f -name "$dylib_glob" -print 2>/dev/null | while IFS= read -r path; do
    name="$(basename "$path")"
    if [ ! -f "$out_dir/$name" ]; then
      cp -f "$path" "$out_dir/$name"
    fi
  done

  {
    printf 'LOOM_LLAMA_CPP_BUNDLE_DIR=%s\n' "$out_dir"
    printf 'LOOM_LLAMA_CPP_LIBRARY_PATH=%s\n' "$out_dir"
  } > "$out_dir/bundle.env"

  {
    printf 'target=%s\n' "$target_triple"
    printf 'source_prefix=%s\n' "$prefix"
    printf 'bundle_dir=%s\n' "$out_dir"
    printf 'checksums=checksums.sha256\n'
    printf '\nfiles:\n'
    ls -lh "$out_dir"
    for lib in "$out_dir"/*; do
      if [ -f "$lib" ]; then
        case "$host_os" in
          Darwin)
            printf '\n%s linkage:\n' "$(basename "$lib")"
            otool -L "$lib" 2>/dev/null || true
            ;;
          Linux)
            printf '\n%s linkage:\n' "$(basename "$lib")"
            ldd "$lib" 2>/dev/null || true
            ;;
        esac
      fi
    done
  } > "$out_dir/manifest.txt"

  : > "$out_dir/checksums.sha256"
  find "$out_dir" -maxdepth 1 -type f ! -name 'checksums.sha256' -print | sort | while IFS= read -r file; do
    printf '%s  %s\n' "$(hash_file "$file")" "$(basename "$file")" >> "$out_dir/checksums.sha256"
  done

  info "staged llama.cpp runtime bundle at $out_dir"
}

clean_managed_build() {
  case "$build_dir" in
    "$repo_root"/target/inference/llama-cpp | /tmp/* | /private/tmp/*) ;;
    *)
      fail "refusing to clean unmanaged build dir: $build_dir"
      ;;
  esac
  rm -rf "$build_dir/build" "$build_dir/install"
}

refresh_source() {
  if ! command -v git >/dev/null 2>&1; then
    fail "git is required to refresh llama.cpp"
  fi
  mkdir -p "$(dirname "$source_dir")"
  if [ -d "$source_dir/.git" ]; then
    info "refreshing llama.cpp source at $source_dir"
    git -C "$source_dir" fetch --tags --prune origin
  elif [ -e "$source_dir" ]; then
    fail "LLAMA_CPP_SOURCE exists but is not a git checkout: $source_dir"
  else
    info "cloning llama.cpp from $repo_url into $source_dir"
    git clone "$repo_url" "$source_dir"
    git -C "$source_dir" fetch --tags --prune origin
  fi

  if git -C "$source_dir" show-ref --verify --quiet "refs/remotes/origin/$repo_ref"; then
    git -C "$source_dir" checkout -B "$repo_ref" "origin/$repo_ref"
  else
    git -C "$source_dir" checkout "$repo_ref"
  fi
  git -C "$source_dir" rev-parse --short HEAD | awk '{ print "llama.cpp source head="$1 }'
}

ensure_source() {
  [ -f "$source_dir/CMakeLists.txt" ] || fail "LLAMA_CPP_SOURCE does not contain CMakeLists.txt: $source_dir"
}

build_shared() {
  if [ "$managed_source" -eq 1 ] && [ ! -f "$source_dir/CMakeLists.txt" ]; then
    refresh_source
  fi
  ensure_source

  cmake -S "$source_dir" \
    -B "$build_dir/build" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX="$prefix" \
    -DBUILD_SHARED_LIBS=ON \
    -DLLAMA_BUILD_TESTS=OFF \
    -DLLAMA_BUILD_EXAMPLES=OFF \
    -DLLAMA_BUILD_SERVER=OFF \
    -DLLAMA_BUILD_TOOLS=OFF \
    -DLLAMA_BUILD_APP=OFF \
    -DLLAMA_CURL=OFF
  cmake --build "$build_dir/build" --config Release --parallel
  cmake --install "$build_dir/build" --config Release
  stage_bundle
}

case "$mode" in
  check)
    report_prefix
    ;;
  refresh)
    refresh_source
    ;;
  stage)
    stage_bundle
    ;;
  build-shared)
    build_shared
    ;;
  clean-build)
    refresh_source
    clean_managed_build
    build_shared
    ;;
esac
