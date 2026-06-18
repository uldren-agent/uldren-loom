#!/usr/bin/env bash
#
# Focused footprint report for external auth verifier candidates.
#
# Usage:   bash compare-auth-size.sh

set -o pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
cd "$here" || exit 1

bin="target/release/probe"

kib() { awk -v b="$1" 'BEGIN { printf "%.1f KiB", b / 1024 }'; }

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

print_row() {
  label="$1"
  feature="$2"
  point="$3"
  size="$(build_size "$feature")"
  deps="$(dep_count "$feature")"

  if [ "$size" = "FAILED" ]; then
    printf '  %-28s %-28s %14s   %-12s  %-7s  %s\n' "$label" "$feature" "FAILED" "" "" "$point"
  elif [ "$feature" = "baseline" ] || [ "$base" = "FAILED" ]; then
    printf '  %-28s %-28s %14s   %-12s  %-7s  %s\n' "$label" "$feature" "$(kib "$size")" "(baseline)" "${deps} deps" "$point"
  else
    delta=$(( size - base ))
    printf '  %-28s %-28s %14s   %-12s  %-7s  %s\n' "$label" "$feature" "$(kib "$size")" "$(awk -v x="$delta" 'BEGIN{printf "%+.1f KiB", x/1024}')" "${deps} deps" "$point"
  fi
}

base="$(build_size baseline)"

echo
echo "external auth verifier candidates"
echo "--------------------------------------------------------------------------------------------------------------------------------"
printf '  %-28s %-28s %14s   %-12s  %-7s  %s\n' "probe" "feature" "size" "vs baseline" "deps" "point"
echo "--------------------------------------------------------------------------------------------------------------------------------"
print_row baseline baseline "empty binary control"
print_row oidc-openidconnect-default oidc_openidconnect_default "full OIDC crate plus default HTTP"
print_row oidc-openidconnect-core oidc_openidconnect_core "full OIDC crate without HTTP client"
print_row oidc-id-token-verifier oidc_id_token_verifier "OIDC config/type probe"
print_row oidc-jsonwebtoken-default oidc_jsonwebtoken_default "JWT primitive with default crypto"
print_row oidc-jsonwebtoken-aws oidc_jsonwebtoken_aws "JWT primitive with aws-lc-rs"
print_row saml-alpha saml_alpha "new SAML type/replay probe"
print_row saml-rustauth saml_rustauth "SAML config/type probe"
print_row saml-rustauth-signed saml_rustauth_signed "SAML signed type path"
print_row saml-opensaml saml_opensaml "SAML protocol with XML crypto"
print_row saml-opensaml-protocol saml_opensaml_protocol "SAML protocol without XML crypto"
print_row webauthn-rp webauthn_rp "permissive RP option probe"
print_row webauthn-caden webauthn_caden "permissive RP verifier candidate"
print_row webauthn-passkey webauthn_passkey "client/authenticator ecosystem reference"
print_row webauthn-rs webauthn_rs "mature RP crate, license screened"
print_row x509-parser-verify-ring x509_parser_verify_ring "X.509 parser with ring verify"
print_row x509-parser-verify-aws x509_parser_verify_aws "X.509 parser with aws-lc-rs verify"
print_row x509-verify-default x509_verify_default "RustCrypto X.509 type probe"
print_row public-key-ring public_key_ring "ring Ed25519 verification"
print_row public-key-ed25519-dalek public_key_ed25519_dalek "pure Rust Ed25519 verification"
print_row public-key-p256 public_key_p256 "pure Rust P-256 verification"
print_row public-key-rsa public_key_rsa "pure Rust RSA type probe"
