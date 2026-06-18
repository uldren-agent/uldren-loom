#!/usr/bin/env sh
set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
OUT_DIR="${PIM_CERT_OUT_DIR:-$ROOT_DIR/scripts/pim-cert/out}"
CERT_DIR="$OUT_DIR/certs"
HOSTNAME="${PIM_CERT_HOSTNAME:-uldrentest.com}"

if [ "${RESET:-0}" = "1" ]; then
  rm -rf "$CERT_DIR"
fi

if [ -e "$CERT_DIR/ca.cert.pem" ] || [ -e "$CERT_DIR/server.cert.pem" ]; then
  echo "certificate material already exists in $CERT_DIR"
  echo "set RESET=1 to replace it"
  exit 1
fi

command -v openssl >/dev/null 2>&1 || {
  echo "openssl is required"
  exit 1
}

mkdir -p "$CERT_DIR"

openssl req \
  -x509 \
  -newkey rsa:3072 \
  -nodes \
  -days 825 \
  -subj "/CN=Uldren Loom Local Test Root CA" \
  -keyout "$CERT_DIR/ca.key.pem" \
  -out "$CERT_DIR/ca.cert.pem"

cat > "$CERT_DIR/server.cnf" <<EOF
[req]
distinguished_name = dn
req_extensions = v3_req
prompt = no

[dn]
CN = $HOSTNAME

[v3_req]
basicConstraints = CA:FALSE
keyUsage = digitalSignature,keyEncipherment
extendedKeyUsage = serverAuth
subjectAltName = @alt_names

[alt_names]
DNS.1 = $HOSTNAME
DNS.2 = localhost
IP.1 = 127.0.0.1
IP.2 = 10.0.2.2
EOF

openssl req \
  -new \
  -newkey rsa:2048 \
  -nodes \
  -keyout "$CERT_DIR/server.key.pem" \
  -out "$CERT_DIR/server.csr.pem" \
  -config "$CERT_DIR/server.cnf"

openssl x509 \
  -req \
  -in "$CERT_DIR/server.csr.pem" \
  -CA "$CERT_DIR/ca.cert.pem" \
  -CAkey "$CERT_DIR/ca.key.pem" \
  -CAcreateserial \
  -days 397 \
  -sha256 \
  -extensions v3_req \
  -extfile "$CERT_DIR/server.cnf" \
  -out "$CERT_DIR/server.cert.pem"

cat "$CERT_DIR/server.cert.pem" "$CERT_DIR/ca.cert.pem" > "$CERT_DIR/server.chain.pem"

echo "created local CA and server certificate in $CERT_DIR"
echo "install CA on macOS:"
echo "sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain $CERT_DIR/ca.cert.pem"
