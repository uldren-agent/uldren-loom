#!/usr/bin/env bash
#
# Focused footprint report for inference runtime/client candidates. The baseline includes Tokio,
# reqwest, and the Loom rustls profile so rows show marginal candidate cost over the HTTP/TLS stack.

set -o pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
cd "$here" || exit 1

bin="target/release/probe"

kib() { awk -v b="$1" 'BEGIN { printf "%.1f KiB", b / 1024 }'; }
mib() { awk -v b="$1" 'BEGIN { printf "%.2f MiB", b / 1048576 }'; }

build_size() {
  rm -f "$bin" 2>/dev/null
  if cargo build --release --no-default-features --features "$1" --quiet >/dev/null 2>&1 && [ -f "$bin" ]; then
    wc -c < "$bin" | tr -d ' '
  else
    echo "FAILED"
  fi
}

dep_count() {
  cargo tree --no-default-features --features "$1" -e normal --prefix none 2>/dev/null \
    | sed '1d' | sort -u | grep -c .
}

row() {
  label="$1"
  feature="$2"
  point="$3"
  size="$(build_size "$feature")"
  deps="$(dep_count "$feature")"
  if [ "$size" = "FAILED" ]; then
    printf '  %-24s %-34s %14s   %-12s  %-7s  %s\n' "$label" "$feature" "FAILED" "" "" "$point"
  elif [ "$feature" = "$base_feature" ]; then
    base="$size"
    printf '  %-24s %-34s %14s   %-12s  %-7s  %s\n' "$label" "$feature" "$(kib "$size")" "(baseline)" "${deps} deps" "$point"
  else
    delta=$(( size - base ))
    printf '  %-24s %-34s %14s   %-12s  %-7s  %s\n' "$label" "$feature" "$(kib "$size")" "$(awk -v x="$delta" 'BEGIN{printf "%+.1f KiB", x/1024}')" "${deps} deps" "$point"
  fi
}

base_feature="inference_http_baseline"

echo
echo "inference runtime/client size-probes"
echo "--------------------------------------------------------------------------------------------------------------------------------"
printf '  %-24s %-34s %14s   %-12s  %-7s  %s\n' "probe" "feature" "size" "vs baseline" "deps" "point"
echo "--------------------------------------------------------------------------------------------------------------------------------"
row baseline "$base_feature" "tokio plus reqwest plus rustls control"
row genai inference_http_baseline,genai_rustls "multi-provider client, rustls"
row ollama-rs inference_http_baseline,ollama_rs_rustls "Ollama HTTP client, rustls"
row ollama-rs-stream inference_http_baseline,ollama_rs_stream "Ollama HTTP client with streaming"
row llmfit-core inference_http_baseline,llmfit_core "hardware and model-fit library"
row llama-cpp-2-common inference_http_baseline,llama_cpp_2_common "llama.cpp bindings, common CPU profile"
row llama-cpp-2-metal inference_http_baseline,llama_cpp_2_metal "llama.cpp bindings, Metal profile"
row mistralrs-default inference_http_baseline,mistralrs_default "full local inference runtime"
row apple-mlx inference_http_baseline,apple_mlx "Apple MLX C API bindings"
if [ -n "${base:-}" ] && [ "$base" != "FAILED" ]; then
  echo "baseline exact bytes: $base ($(mib "$base"))"
fi
