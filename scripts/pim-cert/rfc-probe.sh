#!/usr/bin/env sh
set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
OUT_DIR="${PIM_CERT_OUT_DIR:-$ROOT_DIR/scripts/pim-cert/out}"
ENV_FILE="$OUT_DIR/env.sh"

if [ ! -r "$ENV_FILE" ]; then
  echo "missing $ENV_FILE"
  echo "run scripts/pim-cert/seed.sh first"
  exit 1
fi

. "$ENV_FILE"

HOST="${PIM_CERT_HOST:-uldrentest.com}"
DAV_PORT="${PIM_CERT_DAV_PORT:-10443}"
IMAP_PORT="${PIM_CERT_IMAP_PORT:-10993}"
JMAP_PORT="${PIM_CERT_JMAP_PORT:-18444}"
SMTP_PORT="${PIM_CERT_SMTP_PORT:-1587}"
RESULT_DIR="$OUT_DIR/manual-results"
RESULT_FILE="${PIM_CERT_RFC_PROBE_OUT:-$RESULT_DIR/live-rfc-probes.json}"
TMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/pim-rfc-probes.XXXXXX")
FIRST_ROW=1
FAILURES=0

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

mkdir -p "$RESULT_DIR"

json_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

begin_report() {
  timestamp=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
  {
    printf '{\n'
    printf '  "schema": "uldren-loom-pim-live-rfc-probes-v1",\n'
    printf '  "generated_at": "%s",\n' "$(json_escape "$timestamp")"
    printf '  "host": "%s",\n' "$(json_escape "$HOST")"
    printf '  "ports": {"dav": %s, "imap": %s, "jmap": %s, "smtp": %s},\n' "$DAV_PORT" "$IMAP_PORT" "$JMAP_PORT" "$SMTP_PORT"
    printf '  "account": "%s",\n' "$(json_escape "$PIM_CERT_ACCOUNT")"
    printf '  "probes": [\n'
  } >"$RESULT_FILE"
}

append_probe() {
  name="$1"
  surface="$2"
  protocol="$3"
  status="$4"
  evidence="$5"
  detail="$6"
  if [ "$FIRST_ROW" -eq 0 ]; then
    printf ',\n' >>"$RESULT_FILE"
  fi
  FIRST_ROW=0
  {
    printf '    {'
    printf '"name": "%s", ' "$(json_escape "$name")"
    printf '"surface": "%s", ' "$(json_escape "$surface")"
    printf '"protocol": "%s", ' "$(json_escape "$protocol")"
    printf '"status": "%s", ' "$(json_escape "$status")"
    printf '"evidence": "%s", ' "$(json_escape "$evidence")"
    printf '"detail": "%s"' "$(json_escape "$detail")"
    printf '}'
  } >>"$RESULT_FILE"
  if [ "$status" != "passed" ]; then
    FAILURES=$((FAILURES + 1))
  fi
}

finish_report() {
  {
    printf '\n  ],\n'
    printf '  "summary": {"failures": %s}\n' "$FAILURES"
    printf '}\n'
  } >>"$RESULT_FILE"
}

curl_xml() {
  method="$1"
  url="$2"
  body="$3"
  out="$4"
  curl -sk --noproxy '*' \
    -o "$out" \
    -w '%{http_code}' \
    -u "$PIM_CERT_ACCOUNT:$PIM_CERT_PASSWORD" \
    -X "$method" \
    -H 'Content-Type: application/xml' \
    --data "$body" \
    "$url" 2>"$out.err" || printf '000'
}

curl_jmap() {
  method="$1"
  url="$2"
  body="$3"
  out="$4"
  curl -sk --noproxy '*' \
    -o "$out" \
    -w '%{http_code}' \
    -X "$method" \
    -H "x-loom-principal: $PIM_CERT_PRINCIPAL_ID" \
    -H "x-loom-passphrase: $PIM_CERT_PASSWORD" \
    -H 'Content-Type: application/json' \
    --data "$body" \
    "$url" 2>"$out.err" || printf '000'
}

check_contains() {
  file="$1"
  pattern="$2"
  grep -Fq "$pattern" "$file"
}

begin_report

propfind_body='<D:propfind xmlns:D="DAV:"><D:prop><D:resourcetype/><D:displayname/></D:prop></D:propfind>'
caldav_root="$TMP_DIR/caldav-root.xml"
caldav_root_status=$(curl_xml PROPFIND "https://$HOST:$DAV_PORT/caldav/" "$propfind_body" "$caldav_root")
if [ "$caldav_root_status" = "207" ] && check_contains "$caldav_root" "/caldav/personal/"; then
  append_probe "caldav-root-discovery" "calendar" "caldav" "passed" "RFC 4791 service discovery, WebDAV PROPFIND" "root PROPFIND exposes personal calendar"
else
  append_probe "caldav-root-discovery" "calendar" "caldav" "failed" "RFC 4791 service discovery, WebDAV PROPFIND" "expected HTTP 207 and /caldav/personal/"
fi

caldav_query_body='<C:calendar-query xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav"><D:prop><D:getetag/><C:calendar-data/></D:prop><C:filter><C:comp-filter name="VCALENDAR"><C:comp-filter name="VEVENT"/></C:comp-filter></C:filter></C:calendar-query>'
caldav_query="$TMP_DIR/caldav-query.xml"
caldav_query_status=$(curl_xml REPORT "https://$HOST:$DAV_PORT/caldav/personal/" "$caldav_query_body" "$caldav_query")
if [ "$caldav_query_status" = "207" ] &&
  check_contains "$caldav_query" "PIM cert kickoff" &&
  check_contains "$caldav_query" "Reference client review" &&
  check_contains "$caldav_query" "Protocol transcript signoff"; then
  append_probe "caldav-calendar-query-events" "calendar" "caldav" "passed" "RFC 4791 calendar-query and RFC 5545 VEVENT" "calendar-query returns the three seeded VEVENT resources"
else
  append_probe "caldav-calendar-query-events" "calendar" "caldav" "failed" "RFC 4791 calendar-query and RFC 5545 VEVENT" "expected three seeded event summaries"
fi

carddav_root="$TMP_DIR/carddav-root.xml"
carddav_root_status=$(curl_xml PROPFIND "https://$HOST:$DAV_PORT/carddav/" "$propfind_body" "$carddav_root")
if [ "$carddav_root_status" = "207" ] && check_contains "$carddav_root" "/carddav/personal/"; then
  append_probe "carddav-root-discovery" "contacts" "carddav" "passed" "RFC 6352 service discovery, WebDAV PROPFIND" "root PROPFIND exposes personal address book"
else
  append_probe "carddav-root-discovery" "contacts" "carddav" "failed" "RFC 6352 service discovery, WebDAV PROPFIND" "expected HTTP 207 and /carddav/personal/"
fi

carddav_query_body='<CARD:addressbook-query xmlns:D="DAV:" xmlns:CARD="urn:ietf:params:xml:ns:carddav"><D:prop><D:getetag/><CARD:address-data/></D:prop><CARD:filter><CARD:prop-filter name="FN"/></CARD:filter></CARD:addressbook-query>'
carddav_query="$TMP_DIR/carddav-query.xml"
carddav_query_status=$(curl_xml REPORT "https://$HOST:$DAV_PORT/carddav/personal/" "$carddav_query_body" "$carddav_query")
if [ "$carddav_query_status" = "207" ] &&
  check_contains "$carddav_query" "Ada Lovelace" &&
  check_contains "$carddav_query" "Grace Hopper" &&
  check_contains "$carddav_query" "Katherine Johnson"; then
  append_probe "carddav-addressbook-query-contacts" "contacts" "carddav" "passed" "RFC 6352 addressbook-query and RFC 6350 vCard" "addressbook-query returns the three seeded vCards"
else
  append_probe "carddav-addressbook-query-contacts" "contacts" "carddav" "failed" "RFC 6352 addressbook-query and RFC 6350 vCard" "expected three seeded contact names"
fi

imap_out="$TMP_DIR/imap.txt"
imap_auth=$(printf '\000%s\000%s' "$PIM_CERT_ACCOUNT" "$PIM_CERT_PASSWORD" | base64 | tr -d '\n')
if printf 'a1 AUTHENTICATE PLAIN %s\r\na2 SELECT INBOX\r\na3 FETCH 1:* (FLAGS UID RFC822.SIZE ENVELOPE)\r\na4 FETCH 1:* (FLAGS UID RFC822.SIZE BODY[])\r\na5 LOGOUT\r\n' "$imap_auth" |
  openssl s_client -quiet -connect "$HOST:$IMAP_PORT" -servername "$HOST" >"$imap_out" 2>&1; then
  if check_contains "$imap_out" "* 3 EXISTS" &&
    check_contains "$imap_out" "PIM cert message one" &&
    check_contains "$imap_out" "PIM cert message two" &&
    check_contains "$imap_out" "PIM cert message three"; then
    append_probe "imap-rfc9051-inbox-fetch" "mail" "imap" "passed" "RFC 9051 SELECT and FETCH, RFC 5322 message projection" "IMAPS exposes the three seeded inbox messages"
  else
    append_probe "imap-rfc9051-inbox-fetch" "mail" "imap" "failed" "RFC 9051 SELECT and FETCH, RFC 5322 message projection" "expected 3 EXISTS and three seeded subjects"
  fi
else
  append_probe "imap-rfc9051-inbox-fetch" "mail" "imap" "failed" "RFC 9051 SELECT and FETCH, RFC 5322 message projection" "openssl IMAPS probe failed"
fi

jmap_session="$TMP_DIR/jmap-session.json"
jmap_session_status=$(curl_jmap GET "https://$HOST:$JMAP_PORT/jmap/session" '{}' "$jmap_session")
if [ "$jmap_session_status" = "200" ] &&
  check_contains "$jmap_session" "urn:ietf:params:jmap:core" &&
  check_contains "$jmap_session" "urn:ietf:params:jmap:mail"; then
  append_probe "jmap-session-discovery" "mail" "jmap" "passed" "RFC 8620 session resource" "session advertises JMAP core and mail capabilities"
else
  append_probe "jmap-session-discovery" "mail" "jmap" "failed" "RFC 8620 session resource" "expected JMAP core and mail capabilities"
fi

jmap_api_body='{"using":["urn:ietf:params:jmap:core","urn:ietf:params:jmap:mail"],"methodCalls":[["Mailbox/get",{},"a"],["Email/query",{"filter":{"inMailbox":"inbox"}},"b"],["Email/get",{"ids":["inbox/msg-1","inbox/msg-2","inbox/msg-3"]},"c"]]}'
jmap_api="$TMP_DIR/jmap-api.json"
jmap_api_status=$(curl_jmap POST "https://$HOST:$JMAP_PORT/jmap/api" "$jmap_api_body" "$jmap_api")
if [ "$jmap_api_status" = "200" ] &&
  check_contains "$jmap_api" "PIM cert message one" &&
  check_contains "$jmap_api" "PIM cert message two" &&
  check_contains "$jmap_api" "PIM cert message three"; then
  append_probe "jmap-mail-query-get" "mail" "jmap" "passed" "RFC 8620, RFC 8621, RFC 9404 bounded mail methods" "JMAP returns the three seeded mail messages"
else
  append_probe "jmap-mail-query-get" "mail" "jmap" "failed" "RFC 8620, RFC 8621, RFC 9404 bounded mail methods" "expected three seeded JMAP email subjects"
fi

smtp_out="$TMP_DIR/smtp.txt"
smtp_auth=$(printf '\000%s\000%s' "$PIM_CERT_ACCOUNT" "$PIM_CERT_PASSWORD" | base64 | tr -d '\n')
if printf 'EHLO client.example\r\nAUTH PLAIN %s\r\nMAIL FROM:<%s>\r\nRCPT TO:<%s>\r\nDATA\r\nSubject: Setup Probe\r\n\r\nsetup probe\r\n.\r\nQUIT\r\n' "$smtp_auth" "$PIM_CERT_ACCOUNT" "$PIM_CERT_ACCOUNT" |
  openssl s_client -quiet -starttls smtp -connect "$HOST:$SMTP_PORT" -servername "$HOST" >"$smtp_out" 2>&1; then
  if check_contains "$smtp_out" "235 Authentication successful" &&
    check_contains "$smtp_out" "250 Message accepted for setup compatibility"; then
    append_probe "smtp-setup-auth-session" "mail" "smtp" "passed" "RFC 5321 STARTTLS setup dialogue and AUTH PLAIN compatibility" "STARTTLS SMTP setup listener authenticates and accepts DATA without delivery"
  else
    append_probe "smtp-setup-auth-session" "mail" "smtp" "failed" "RFC 5321 STARTTLS setup dialogue and AUTH PLAIN compatibility" "expected authenticated setup DATA acceptance"
  fi
else
  append_probe "smtp-setup-auth-session" "mail" "smtp" "failed" "RFC 5321 STARTTLS setup dialogue and AUTH PLAIN compatibility" "openssl SMTP probe failed"
fi

finish_report
echo "wrote $RESULT_FILE"
if [ "$FAILURES" -ne 0 ]; then
  echo "rfc probes failed: $FAILURES"
  exit 1
fi
echo "rfc probes passed"
