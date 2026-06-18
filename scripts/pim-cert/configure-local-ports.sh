#!/usr/bin/env sh
set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)

PIM_CERT_DAV_BIND="${PIM_CERT_DAV_BIND:-127.0.0.1:10443}" \
PIM_CERT_IMAP_BIND="${PIM_CERT_IMAP_BIND:-127.0.0.1:10993}" \
PIM_CERT_JMAP_BIND="${PIM_CERT_JMAP_BIND:-127.0.0.1:18444}" \
PIM_CERT_SMTP_BIND="${PIM_CERT_SMTP_BIND:-127.0.0.1:1587}" \
  "$ROOT_DIR/scripts/pim-cert/configure-listeners.sh"
