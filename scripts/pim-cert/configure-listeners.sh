#!/usr/bin/env sh
set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
LOOM_BIN="${LOOM_BIN:-$ROOT_DIR/scripts/pim-cert/bin/loom}"
OUT_DIR="${PIM_CERT_OUT_DIR:-$ROOT_DIR/scripts/pim-cert/out}"
ENV_FILE="$OUT_DIR/env.sh"
CERT_DIR="$OUT_DIR/certs"
BUNDLE_NAME="${PIM_CERT_BUNDLE_NAME:-pim-cert-local}"

if [ ! -r "$ENV_FILE" ]; then
  echo "missing $ENV_FILE"
  echo "run scripts/pim-cert/seed.sh first"
  exit 1
fi

. "$ENV_FILE"

CALENDAR_NAMESPACE="${PIM_CERT_CALENDAR_NAMESPACE:-37373737-3737-3737-3737-373737373737}"
CONTACTS_NAMESPACE="${PIM_CERT_CONTACTS_NAMESPACE:-38383838-3838-3838-3838-383838383838}"
MAIL_NAMESPACE="${PIM_CERT_MAIL_NAMESPACE:-39393939-3939-3939-3939-393939393939}"
DAV_BIND="${PIM_CERT_DAV_BIND:-0.0.0.0:443}"
CALDAV_BIND="${PIM_CERT_CALDAV_BIND:-$DAV_BIND}"
CARDDAV_BIND="${PIM_CERT_CARDDAV_BIND:-$DAV_BIND}"
IMAP_BIND="${PIM_CERT_IMAP_BIND:-0.0.0.0:993}"
JMAP_BIND="${PIM_CERT_JMAP_BIND:-0.0.0.0:8444}"
SMTP_BIND="${PIM_CERT_SMTP_BIND:-0.0.0.0:587}"

if [ ! -x "$LOOM_BIN" ]; then
  echo "loom binary not found at $LOOM_BIN"
  echo "run: scripts/pim-cert/build-local-loom.sh"
  exit 1
fi

if [ ! -r "$CERT_DIR/server.chain.pem" ] || [ ! -r "$CERT_DIR/server.key.pem" ] || [ ! -r "$CERT_DIR/ca.cert.pem" ]; then
  echo "missing certificate material in $CERT_DIR"
  echo "run scripts/pim-cert/generate-ca.sh first"
  exit 1
fi

"$LOOM_BIN" --auth-principal "$PIM_CERT_PRINCIPAL_ID" --auth-key-source "file:$PIM_CERT_PASS_FILE" certificate import "$PIM_CERT_STORE" "$BUNDLE_NAME" --cert-chain "$CERT_DIR/server.chain.pem" --private-key "$CERT_DIR/server.key.pem" --trust-bundle "$CERT_DIR/ca.cert.pem" --force

LISTENERS_JSON=$("$LOOM_BIN" --auth-principal "$PIM_CERT_PRINCIPAL_ID" --auth-key-source "file:$PIM_CERT_PASS_FILE" serve list "$PIM_CERT_STORE")
if printf '%s\n' "$LISTENERS_JSON" | grep -Eq '"selectors":\["(calendar|contacts|mail|37373737-3737-3737-3737-373737373737|38383838-3838-3838-3838-383838383838|39393939-3939-3939-3939-393939393939)"\]'; then
  if ! command -v python3 >/dev/null 2>&1; then
    echo "python3 is required to remove stale PIM listener records"
    exit 1
  fi
  STALE_IDS=$(
    printf '%s\n' "$LISTENERS_JSON" |
      python3 -c 'import json,sys
data=json.load(sys.stdin)
targets = {
    ("calendar", "caldav", ("calendar",)),
    ("calendar", "caldav", ("37373737-3737-3737-3737-373737373737",)),
    ("contacts", "carddav", ("contacts",)),
    ("contacts", "carddav", ("38383838-3838-3838-3838-383838383838",)),
    ("mail", "imap", ("mail",)),
    ("mail", "imap", ("39393939-3939-3939-3939-393939393939",)),
    ("mail", "jmap", ("mail",)),
    ("mail", "jmap", ("39393939-3939-3939-3939-393939393939",)),
    ("mail", "smtp", ("mail",)),
    ("mail", "smtp", ("39393939-3939-3939-3939-393939393939",)),
}
for record in data.get("listeners", []):
    key = (record.get("surface"), record.get("transport"), tuple(record.get("selectors", [])))
    if key in targets:
        print(record["id"])'
  )
  for id in $STALE_IDS; do
    "$LOOM_BIN" --auth-principal "$PIM_CERT_PRINCIPAL_ID" --auth-key-source "file:$PIM_CERT_PASS_FILE" serve remove "$PIM_CERT_STORE" "$id"
  done
fi

"$LOOM_BIN" --auth-principal "$PIM_CERT_PRINCIPAL_ID" --auth-key-source "file:$PIM_CERT_PASS_FILE" serve configure "$PIM_CERT_STORE" calendar "$CALENDAR_NAMESPACE" --bind "$CALDAV_BIND" --transport caldav --tls-certificate-bundle "$BUNDLE_NAME" --auth-mode passphrase --exposure read-write --audit-mode all
"$LOOM_BIN" --auth-principal "$PIM_CERT_PRINCIPAL_ID" --auth-key-source "file:$PIM_CERT_PASS_FILE" serve configure "$PIM_CERT_STORE" contacts "$CONTACTS_NAMESPACE" --bind "$CARDDAV_BIND" --transport carddav --tls-certificate-bundle "$BUNDLE_NAME" --auth-mode passphrase --exposure read-write --audit-mode all
"$LOOM_BIN" --auth-principal "$PIM_CERT_PRINCIPAL_ID" --auth-key-source "file:$PIM_CERT_PASS_FILE" serve configure "$PIM_CERT_STORE" mail "$MAIL_NAMESPACE" --bind "$IMAP_BIND" --transport imap --tls-certificate-bundle "$BUNDLE_NAME" --auth-mode passphrase --exposure read-write --audit-mode all
"$LOOM_BIN" --auth-principal "$PIM_CERT_PRINCIPAL_ID" --auth-key-source "file:$PIM_CERT_PASS_FILE" serve configure "$PIM_CERT_STORE" mail "$MAIL_NAMESPACE" --bind "$JMAP_BIND" --transport jmap --tls-certificate-bundle "$BUNDLE_NAME" --auth-mode passphrase --exposure read-write --audit-mode all
"$LOOM_BIN" --auth-principal "$PIM_CERT_PRINCIPAL_ID" --auth-key-source "file:$PIM_CERT_PASS_FILE" serve configure "$PIM_CERT_STORE" mail "$MAIL_NAMESPACE" --bind "$SMTP_BIND" --transport smtp --tls-certificate-bundle "$BUNDLE_NAME" --tls-mode starttls --auth-mode passphrase --exposure read-write --audit-mode all

"$LOOM_BIN" --auth-principal "$PIM_CERT_PRINCIPAL_ID" --auth-key-source "file:$PIM_CERT_PASS_FILE" serve list "$PIM_CERT_STORE"
