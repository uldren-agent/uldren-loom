#!/usr/bin/env bash
#
# Grouped footprint and dependency-surface report for every built-in size probe in this crate.
# Each row builds the `probe` binary with exactly one feature enabled, then reports stripped release
# size, delta from the empty baseline, and normal transitive dependency count.
#
# Usage:   bash compare-all-size.sh

set -o pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
cd "$here" || exit 1

bin="target/release/probe"

kib() { awk -v b="$1" 'BEGIN { printf "%.1f KiB", b / 1024 }'; }
mib() { awk -v b="$1" 'BEGIN { printf "%.2f MiB", b / 1048576 }'; }

size_value() {
  label="$1"
  if [ "$label" = "FAILED" ]; then
    echo "FAILED"
  else
    kib "$label"
  fi
}

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

build_probe() {
  label="$1"
  feature="$2"
  #printf '==> building %-22s (%s)\n' "$label" "$feature"
  size="$(build_size "$feature")"
  deps="$(dep_count "$feature")"
}

print_header() {
  echo
  echo "$1"
  echo "--------------------------------------------------------------------------------------------------------------------------"
  printf '  %-22s %-22s %14s   %-12s  %-7s  %s\n' "probe" "feature" "size" "vs baseline" "deps" "point"
  echo "--------------------------------------------------------------------------------------------------------------------------"
}

print_row() {
  label="$1"
  feature="$2"
  point="$3"
  size="$4"
  deps="$5"

  if [ "$size" = "FAILED" ]; then
    printf '  %-22s %-22s %14s   %-12s  %-7s  %s\n' "$label" "$feature" "FAILED" "" "" "$point"
  elif [ "$base" = "FAILED" ] || [ "$feature" = "$base_feature" ]; then
    printf '  %-22s %-22s %14s   %-12s  %-7s  %s\n' "$label" "$feature" "$(kib "$size")" "(baseline)" "${deps} deps" "$point"
  else
    delta=$(( size - base ))
    printf '  %-22s %-22s %14s   %-12s  %-7s  %s\n' "$label" "$feature" "$(kib "$size")" "$(awk -v x="$delta" 'BEGIN{printf "%+.1f KiB", x/1024}')" "${deps} deps" "$point"
  fi
}

run_row() {
  label="$1"
  feature="$2"
  point="$3"
  build_probe "$label" "$feature"
  print_row "$label" "$feature" "$point" "$size" "$deps"
}

build_probe baseline baseline
base="$size"
base_deps="$deps"
base_feature="baseline"

echo
echo "size-probes summary"
echo "baseline: $(size_value "$base") (${base_deps} deps)"
if [ "$base" != "FAILED" ]; then
  echo "baseline exact bytes: $base ($(mib "$base"))"
fi

print_header "L1 guard and L2 derivation engines"
print_row baseline baseline "empty binary control" "$base" "$base_deps"
run_row cel cel "CEL expression engine"
run_row regorus regorus "Rego policy engine"
run_row ascent ascent "compile-time Datalog macro"
run_row cozo cozo "full Datalog database"

print_header "Cron parser candidates"
print_row baseline baseline "empty binary control" "$base" "$base_deps"
run_row cron cron "cron crate over chrono"
run_row croner croner "richer cron dialect over chrono"
run_row saffron saffron "small Vixie-style parser"

print_header "Calendar parse and recurrence candidates"
print_row baseline baseline "empty binary control" "$base" "$base_deps"
run_row civil-time civil_time "civil date-time math substrate"
run_row ical ical "iCalendar parse/build"
run_row rrule rrule "RRULE expansion"
run_row ical-rrule ical_rrule "calendar parse plus recurrence"

print_header "Loom Templates parser/runtime candidates"
print_row baseline baseline "empty binary control" "$base" "$base_deps"
run_row minijinja-default minijinja_default "MiniJinja published defaults"
run_row minijinja-minimal minijinja_minimal "explicit Loom wrapper profile"
run_row tera tera_template "Jinja/Django-family engine"
run_row upon upon_template "smaller Jinja-like subset"
run_row handlebars handlebars_template "negative control, not Jinja syntax"

print_header "Transport and TLS candidates"
print_row baseline baseline "empty binary control" "$base" "$base_deps"
run_row rustls-default rustls_default "rustls default provider"
run_row rustls-ring rustls_ring "rustls with ring provider"
run_row rustls-aws rustls_aws "rustls with aws-lc-rs provider"
run_row rustls-aws-fips rustls_aws_fips "rustls FIPS provider profile"
run_row x509-parser x509_parser "X.509 display parser"
run_row x509-cert x509_cert "RustCrypto X.509 parser"
run_row rcgen-self-signed rcgen_self_signed "X.509 certificate generator"
run_row rcgen-ed25519 rcgen_ed25519 "Ed25519 X.509 certificate generator"
run_row grpc-tonic grpc_tonic "tonic plus prost client surface"

print_header "IPFS and Tor candidates"
print_row baseline baseline "empty binary control" "$base" "$base_deps"
run_row rust-ipfs-node ipfs_rust_node "embeddable node: DHT, Bitswap, pubsub"
run_row arti-onion tor_arti_onion "embedded Tor routing plus onion-service server"

print_header "External auth verifier candidates"
print_row baseline baseline "empty binary control" "$base" "$base_deps"
run_row oidc-openidconnect-default oidc_openidconnect_default "full OIDC crate plus default HTTP"
run_row oidc-openidconnect-core oidc_openidconnect_core "full OIDC crate without HTTP client"
run_row oidc-id-token-verifier oidc_id_token_verifier "OIDC config/type probe"
run_row oidc-jsonwebtoken-default oidc_jsonwebtoken_default "JWT primitive with default crypto"
run_row oidc-jsonwebtoken-aws oidc_jsonwebtoken_aws "JWT primitive with aws-lc-rs"
run_row saml-alpha saml_alpha "new SAML type/replay probe"
run_row saml-rustauth saml_rustauth "SAML config/type probe"
run_row saml-rustauth-signed saml_rustauth_signed "SAML signed type path"
run_row saml-opensaml saml_opensaml "SAML protocol with XML crypto"
run_row saml-opensaml-protocol saml_opensaml_protocol "SAML protocol without XML crypto"
run_row webauthn-rp webauthn_rp "permissive RP option probe"
run_row webauthn-caden webauthn_caden "permissive RP verifier candidate"
run_row webauthn-passkey webauthn_passkey "client/authenticator ecosystem reference"
run_row webauthn-rs webauthn_rs "mature RP crate, license screened"
run_row x509-parser-verify-ring x509_parser_verify_ring "X.509 parser with ring verify"
run_row x509-parser-verify-aws x509_parser_verify_aws "X.509 parser with aws-lc-rs verify"
run_row x509-verify-default x509_verify_default "RustCrypto X.509 type probe"
run_row public-key-ring public_key_ring "ring Ed25519 verification"
run_row public-key-ed25519-dalek public_key_ed25519_dalek "pure Rust Ed25519 verification"
run_row public-key-p256 public_key_p256 "pure Rust P-256 verification"
run_row public-key-rsa public_key_rsa "pure Rust RSA type probe"

print_header "Inference model download candidates"
print_row baseline baseline "empty binary control" "$base" "$base_deps"
run_row hf-hub-default hf_hub_default "official client, published defaults"
run_row hf-hub-tokio-rustls hf_hub_tokio_rustls "official client, async rustls only"
run_row hf-hub-ureq hf_hub_ureq "official client, blocking ureq"
run_row reqwest-rustls-blocking reqwest_rustls_blocking "direct GET client, rustls blocking"

build_probe tokio-runtime tokio_runtime
base="$size"
base_deps="$deps"
base_feature="tokio_runtime"

print_header "Inference model download candidates over Tokio baseline"
print_row tokio-runtime tokio_runtime "tokio runtime control" "$base" "$base_deps"
run_row hf-hub-default+tokio tokio_runtime,hf_hub_default "official client, published defaults"
run_row hf-hub-tokio-rustls+tokio tokio_runtime,hf_hub_tokio_rustls "official client, async rustls only"
run_row hf-hub-ureq+tokio tokio_runtime,hf_hub_ureq "official client, blocking ureq"
run_row reqwest-rustls-blocking+tokio tokio_runtime,reqwest_rustls_blocking "direct GET client, rustls blocking"

build_probe loom-tls-baseline loom_tls_baseline
base="$size"
base_deps="$deps"
base_feature="loom_tls_baseline"

print_header "Inference model download candidates over Loom TLS baseline"
print_row loom-tls-baseline loom_tls_baseline "tokio plus rustls aws-lc control" "$base" "$base_deps"
run_row hf-hub-default+loom-tls loom_tls_baseline,hf_hub_default "official client, published defaults"
run_row hf-hub-tokio-rustls+loom-tls loom_tls_baseline,hf_hub_tokio_rustls "official client, async rustls only"
run_row hf-hub-ureq+loom-tls loom_tls_baseline,hf_hub_ureq "official client, blocking ureq"
run_row reqwest-rustls-blocking+loom-tls loom_tls_baseline,reqwest_rustls_blocking "direct GET client, rustls blocking"

build_probe inference-http-baseline inference_http_baseline
base="$size"
base_deps="$deps"
base_feature="inference_http_baseline"

print_header "Inference runtime/client candidates over Tokio reqwest rustls baseline"
print_row inference-http-baseline inference_http_baseline "tokio plus reqwest plus rustls control" "$base" "$base_deps"
run_row genai inference_http_baseline,genai_rustls "multi-provider client, rustls"
run_row ollama-rs inference_http_baseline,ollama_rs_rustls "Ollama HTTP client, rustls"
run_row ollama-rs-stream inference_http_baseline,ollama_rs_stream "Ollama HTTP client with streaming"
run_row llmfit-core inference_http_baseline,llmfit_core "hardware and model-fit library"
run_row llama-cpp-2-common inference_http_baseline,llama_cpp_2_common "llama.cpp bindings, common CPU profile"
run_row llama-cpp-2-metal inference_http_baseline,llama_cpp_2_metal "llama.cpp bindings, Metal profile"
run_row mistralrs-default inference_http_baseline,mistralrs_default "full local inference runtime"
run_row apple-mlx inference_http_baseline,apple_mlx "Apple MLX C API bindings"

build_probe baseline baseline
base="$size"
base_deps="$deps"
base_feature="baseline"

print_header "Hardware probing candidates"
print_row baseline baseline "empty binary control" "$base" "$base_deps"
run_row sysinfo hardware_sysinfo "cross-platform process/system info"
run_row systemstat hardware_systemstat "cross-platform system statistics"
run_row sys-info hardware_sys_info "small system info wrapper"
