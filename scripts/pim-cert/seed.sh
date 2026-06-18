#!/usr/bin/env sh
set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
LOOM_BIN="${LOOM_BIN:-$ROOT_DIR/target/debug/loom}"
CARGO_BIN="${CARGO:-cargo}"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"
OUT_DIR="${PIM_CERT_OUT_DIR:-$ROOT_DIR/scripts/pim-cert/out}"
STORE="${PIM_CERT_STORE:-$OUT_DIR/pim-cert.loom}"
FIXTURE_DIR="$OUT_DIR/fixtures"
PASS_FILE="$OUT_DIR/testpassword.txt"
ENV_FILE="$OUT_DIR/env.sh"
ACCOUNT="${PIM_CERT_ACCOUNT:-example@uldrentest.com}"
PASSWORD="${PIM_CERT_PASSWORD:-testpassword}"
CALENDAR_NAMESPACE="37373737-3737-3737-3737-373737373737"
CONTACTS_NAMESPACE="38383838-3838-3838-3838-383838383838"
MAIL_NAMESPACE="39393939-3939-3939-3939-393939393939"

if ! command -v "$CARGO_BIN" >/dev/null 2>&1; then
  echo "cargo not found"
  exit 1
fi

if [ "${RESET:-0}" = "1" ]; then
  rm -rf "$STORE" "$FIXTURE_DIR" "$PASS_FILE" "$ENV_FILE"
fi

if [ -e "$STORE" ]; then
  echo "store already exists at $STORE"
  echo "set RESET=1 to replace it"
  exit 1
fi

mkdir -p "$OUT_DIR" "$FIXTURE_DIR/mail" "$FIXTURE_DIR/calendar" "$FIXTURE_DIR/contacts"
printf '%s' "$PASSWORD" > "$PASS_FILE"
chmod 600 "$PASS_FILE"

write_crlf_file() {
  file=$1
  shift
  : > "$file"
  for line in "$@"; do
    printf '%s\r\n' "$line" >> "$file"
  done
}

cat > "$FIXTURE_DIR/calendar/event-1.ics" <<'EOF'
BEGIN:VCALENDAR
VERSION:2.0
PRODID:-//Uldren Loom//PIM Cert//EN
BEGIN:VEVENT
UID:pim-cert-event-1
DTSTAMP:20260705T120000Z
DTSTART:20260707T160000Z
DTEND:20260707T163000Z
SUMMARY:PIM cert kickoff
DESCRIPTION:First deterministic certification calendar event.
END:VEVENT
END:VCALENDAR
EOF

cat > "$FIXTURE_DIR/calendar/event-2.ics" <<'EOF'
BEGIN:VCALENDAR
VERSION:2.0
PRODID:-//Uldren Loom//PIM Cert//EN
BEGIN:VEVENT
UID:pim-cert-event-2
DTSTAMP:20260705T120000Z
DTSTART:20260708T170000Z
DTEND:20260708T180000Z
SUMMARY:Reference client review
DESCRIPTION:Second deterministic certification calendar event.
END:VEVENT
END:VCALENDAR
EOF

cat > "$FIXTURE_DIR/calendar/event-3.ics" <<'EOF'
BEGIN:VCALENDAR
VERSION:2.0
PRODID:-//Uldren Loom//PIM Cert//EN
BEGIN:VEVENT
UID:pim-cert-event-3
DTSTAMP:20260705T120000Z
DTSTART:20260709T180000Z
DTEND:20260709T183000Z
SUMMARY:Protocol transcript signoff
DESCRIPTION:Third deterministic certification calendar event.
END:VEVENT
END:VCALENDAR
EOF

cat > "$FIXTURE_DIR/contacts/contact-1.vcf" <<'EOF'
BEGIN:VCARD
VERSION:4.0
UID:pim-cert-contact-1
FN:Ada Lovelace
N:Lovelace;Ada;;;
ORG:Analytical Engines
TITLE:Architect
EMAIL;TYPE=work:ada@example.test
TEL;TYPE=work:+1555010101
END:VCARD
EOF

cat > "$FIXTURE_DIR/contacts/contact-2.vcf" <<'EOF'
BEGIN:VCARD
VERSION:4.0
UID:pim-cert-contact-2
FN:Grace Hopper
N:Hopper;Grace;;;
ORG:Compiler Bureau
TITLE:Engineer
EMAIL;TYPE=work:grace@example.test
TEL;TYPE=mobile:+1555010102
END:VCARD
EOF

cat > "$FIXTURE_DIR/contacts/contact-3.vcf" <<'EOF'
BEGIN:VCARD
VERSION:4.0
UID:pim-cert-contact-3
FN:Katherine Johnson
N:Johnson;Katherine;;;
ORG:Flight Dynamics
TITLE:Analyst
EMAIL;TYPE=work:katherine@example.test
TEL;TYPE=work:+1555010103
END:VCARD
EOF

write_crlf_file "$FIXTURE_DIR/mail/message-1.eml" \
  "From: sender1@example.test" \
  "To: example@uldrentest.com" \
  "Subject: PIM cert message one" \
  "Message-ID: <pim-cert-message-1@example.test>" \
  "Date: Tue, 07 Jul 2026 16:00:00 +0000" \
  "" \
  "This is the first deterministic certification mail message."

write_crlf_file "$FIXTURE_DIR/mail/message-2.eml" \
  "From: sender2@example.test" \
  "To: example@uldrentest.com" \
  "Subject: PIM cert message two" \
  "Message-ID: <pim-cert-message-2@example.test>" \
  "Date: Wed, 08 Jul 2026 17:00:00 +0000" \
  "" \
  "This is the second deterministic certification mail message."

write_crlf_file "$FIXTURE_DIR/mail/message-3.eml" \
  "From: sender3@example.test" \
  "To: example@uldrentest.com" \
  "Subject: PIM cert message three" \
  "Message-ID: <pim-cert-message-3@example.test>" \
  "Date: Thu, 09 Jul 2026 18:00:00 +0000" \
  "" \
  "This is the third deterministic certification mail message."

"$CARGO_BIN" build -p uldren-loom-cli --no-default-features --example pim_cert_seed
SEED_BIN="$TARGET_DIR/debug/examples/pim_cert_seed"
if [ ! -x "$SEED_BIN" ]; then
  echo "seeder binary not found at $SEED_BIN"
  exit 1
fi
PRINCIPAL_ID=$("$SEED_BIN" "$STORE" "$FIXTURE_DIR" "$ACCOUNT" "$PASSWORD")

cat > "$ENV_FILE" <<EOF
export PIM_CERT_STORE='$STORE'
export PIM_CERT_ACCOUNT='$ACCOUNT'
export PIM_CERT_PASSWORD='$PASSWORD'
export PIM_CERT_PRINCIPAL_ID='$PRINCIPAL_ID'
export PIM_CERT_PASS_FILE='$PASS_FILE'
export PIM_CERT_OUT_DIR='$OUT_DIR'
export PIM_CERT_CALENDAR_NAMESPACE='$CALENDAR_NAMESPACE'
export PIM_CERT_CONTACTS_NAMESPACE='$CONTACTS_NAMESPACE'
export PIM_CERT_MAIL_NAMESPACE='$MAIL_NAMESPACE'
EOF
chmod 600 "$ENV_FILE"

echo "seeded $STORE"
echo "principal id: $PRINCIPAL_ID"
echo "environment file: $ENV_FILE"
