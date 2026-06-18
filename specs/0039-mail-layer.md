# 0039 - Mail Layer

**Status:** Draft, with the local facet, CLI, MCP projection, VFS overlay, compute/WASM host calls,
bindings, bounded hosted IMAP and JMAP profiles, setup-only SMTP compatibility, RFC gate, mutable-state
records, Queue 7 client evidence, and Queue 10 local hardening evidence source-backed. **Version:**
0.1.0.
**Capability:** `mail`.

**Depends on:** 0001 (invariant A6/A7), 0002 (object model), 0024 (CAS - the immutable body store), 0042
(collections), 0003d (commit diff), 0014/0014a (workspaces and reserved-path projection), 0027 (ACL, for
the hosted tier). **Relates to:** 0037 (calendar), 0038 (contacts), 0041 (lifecycle hooks - the filter
entry point), 0015 (programs - filters are programs).

The `mail` facet is email for the agent: store, index, flag, and serve messages. The reusable message
index record, mailbox metadata record, canonical CBOR codec, and RFC 5322 parsing helper now live in
`loom-pim`, with `loom-core::mail` retaining CAS body storage, flags, workspace, ACL, reserved paths,
and `FacetKind::Mail` integration. There is **no SMTP in the base facet**; the facet does not send. The
primary local surface is MCP and mount-ingestion of exported mail. The initial hosted mailbox surface is
IMAP for mailbox read and flag management.

Compute automation follows the 0015/0041 D+C decision. Mail automation first enters compute through
lifecycle events such as `on_message_ingested`, `on_flags_changed`, and `on_moved`. Direct Rust
`StateAccess` is source-backed as domain-shaped mailbox metadata, message ingest/get/list/delete, raw
`.eml` retrieval, flag get/set, and search operations, and guest WASM uses the same domain-shaped host
calls. The 0029 trigger-fire bridge can execute a mail program and append a fire record. Mail move,
flag, ingest event emission, hook registration, and execution-policy planning are source-backed in 0041.
Mail programs must not
receive raw reserved-record CRUD as the contract.

## 1. Model - what is immutable and what is not

Mail differs from calendar/contacts in one structural way: **only the message body is inherently
immutable**. The keystone split:

- **Immutable body.** The raw RFC 5322 bytes are stored once in the content-addressed store (0024),
  deduplicated and integrity-verified. The `.eml` projection is exactly those bytes - no reconstruction,
  unlike the iCalendar/vCard facets. This is the asymmetry: a sent message's bytes never change.
- **Structured index.** A [`MailMessage`] record per message carries the parsed `From`/`To`/`Subject`/
  `Date`/`Message-ID`, a header bag, the body size, and the body's CAS digest, so the facet is queryable
  without re-parsing the body.
- **Mutable flags/labels.** Flags (`\Seen`, `\Flagged`, Gmail-style labels) change constantly. They live
  in a **separate versioned sub-tree** (`flags/<uid>`) from the message index (`msg/<uid>`), so flag
  churn diffs independently of message arrivals. A source-backed mutable-state record carries the
  mailbox flag version token and detailed flag deltas; later retention compaction can squash-bound that
  delta history without rewriting message history (Decision A, confirmed).

Messages live per principal and mailbox (0042 `principal > mailbox`). The mailbox is the collection.

**Principal is the caller, not a query argument.** Projections (the MCP host, 0008 section 9.10; a
hosted IMAP/JMAP server) bind the principal to the calling identity - the owner in a passwordless loom -
and never accept it as an agent-supplied parameter. A principal may read and write only its own subtree
(`.loom/facets/mail/{principal}/*`); the policy enforcement point denies any other principal's subtree.
Mail mailbox sharing is out of scope until explicitly requested. The current hosted mail profile is
owner-only. Shared mailboxes, delegated mailboxes, send-as or submit-as service-principal behavior,
and cross-principal mailbox discovery are not advertised by the current IMAP or JMAP capability rows.

## 2. Decision A - flags storage (confirmed)

Flags are **versioned workspace content in a separate sub-tree**: they sync and are diffable (so they
belong in bucket 1, not the bucket-2 control region). The core stores flags as a sorted, deduplicated
set per message and stores mailbox-scoped flag retention policy, mutable-state version tokens, and
detailed deltas. Squash-bounding is target compaction policy over that separate sub-tree: unbounded flag
history would bloat on hot read/unread toggling, so the hosted/maintenance layer should be able to
collapse flag history older than a window while keeping recent changes for conflict resolution and
audit.

### 2.1 Conditional mutation and comparison anchors

Mail consumes the conditional-mutation contract owned by 0003 section 9.1 at its mutable-state
boundary. Immutable message bodies are content-addressed values and have no update condition. For flag
or mailbox metadata mutation, the anchor is the current mailbox mutable-state version token and, where
needed, the current per-message flag-delta state. The atomic scope is the declared mailbox-state or
single-message flag operation, not the complete message body store.

Mailbox creation consumes `absent`; ordinary state mutation consumes `any`, `exact`, and `generation`.
Mail does not consume `operation_anchor` for ordinary IMAP or JMAP flag updates. The mutable-state
record may provide an owner-issued opaque token, but IMAP UID state and JMAP account, query, and session
state tokens are facade-specific views and do not define the native comparison contract.

Operation-style updates merge only per keyword as already defined by the mutable-state record. A stale
full replacement fails or requires rebase, and conditional mutation does not silently merge any other
mail state. Authorization and redacted audit evidence inherit 0009; errors follow 0003 section 8 and
must not reveal protected prior flags or raw opaque tokens.

## 3. Surfaces and priority

Uniform with 0037: **P0 = the local facet** (core, search, IDL/conformance, CLI, bindings, and the
`.eml` mount projection - which is trivial since the body *is* the wire format); **P1 = the MCP
surface** (source-backed as the primary agent surface); **P2 = hosted mail protocols**. The first
source-backed hosted protocol is the bounded IMAP subset in `crates/loom-hosted/src/imap.rs`:
CAPABILITY, LOGIN, AUTHENTICATE PLAIN, LIST, bounded LIST-EXTENDED, CREATE, DELETE, RENAME, SELECT/EXAMINE, STATUS, stable per-mailbox
numeric UID state, durable SUBSCRIBE/UNSUBSCRIBE/LSUB state, FETCH/UID FETCH, STORE/UID STORE,
SEARCH/UID SEARCH, COPY/UID COPY, MOVE/UID MOVE, APPEND, EXPUNGE, CLOSE, WORKSPACE, IDLE, NOOP, and
LOGOUT. The hosted adapter validates requested STATUS and FETCH items, implements STORE `.SILENT`, and
supports common SEARCH criteria for flags, keywords, headers, body/text, UID sets, and size. Gmail-style
filters are programs (0015) registered on the mail lifecycle hook
`on_message_ingested` (0041), not bespoke facet logic.

## 4. Facade (source-backed)

`loom-core::mail`: `create_mailbox`/`get_mailbox`/`list_mailboxes`/`delete_mailbox`; `ingest_message`
(store body in CAS, parse headers, write index; returns the body's content address), `get_message`
(structured index record), `to_eml` (raw RFC 5322 bytes from CAS, digest-verified), `delete_message`, `list_messages`;
`get_flags`/`set_flags` over the separate flag sub-tree; `search` (case-insensitive substring over
subject and from); and `diff_messages` (per-UID added/removed - the body is immutable, so a present
message never "updates"; a re-ingest with different bytes is remove+add). An ingest into a missing
mailbox, or `set_flags` on a missing message, is `NOT_FOUND`; an empty `UID` is `INVALID_ARGUMENT`.
Durable local mail indexes use `loom-store::derived` records under `derived-index:<index-name>` with
format version `mail-derived-index-v1`, source-digest, engine-version, stale, rebuild, failed, and
unsupported reporting. The source digest is supplied by the index builder and must cover the canonical
mail message records, relevant mutable state where the index depends on it, and the index profile.

The IDL `interface Mail` (idl/loom.idl) is the local projection source for the C ABI and bindings. The
CLI projects mailbox CRUD, message ingest/read/delete/list, flags, search, and raw `.eml` output over
the same facade. MCP tools/resources/prompts are source-backed in `loom-mcp`: the tool surface includes
mailbox, message, flag, search, and raw `.eml` projection methods; resources expose `message/rfc822`
bodies; and the server binding elides `principal` from agent-visible tool schemas before injecting the
bound principal server-side. Hosted IMAP is source-backed for LOGIN, AUTHENTICATE PLAIN with initial
response or continuation, mailbox list/create/delete/rename except INBOX/select/status, bounded
LIST-EXTENDED for SPECIAL-USE, CHILDREN attributes, and RETURN STATUS for supported STATUS items, durable per-mailbox numeric UID
mapping, stable `UIDVALIDITY`, stable `UIDNEXT`, durable `SUBSCRIBE`/`UNSUBSCRIBE`/`LSUB` metadata,
message fetch, UID fetch, fetch-attribute validation, flag mutation, STORE `.SILENT`, bounded common
search criteria, copy/move between mailboxes, mailbox RENAME except INBOX, APPEND with synchronizing literals, EXPUNGE/CLOSE
deletion handling, WORKSPACE, IDLE completion, and direct rustls IMAPS serving through the hosted kernel
and daemon-opened `mail/imap` listener records. FETCH response shaping is source-backed for
metadata-only requests, full body requests, `BODY.PEEK[]`, partial `BODY.PEEK[]<offset.length>`,
`BODY.PEEK[HEADER]`, and `BODY.PEEK[TEXT]`. Hosted JMAP is source-backed for
session discovery, `Mailbox/get`, `Mailbox/set`, `Email/query`, `Email/get`, and `Email/set`
update/destroy through the hosted kernel and daemon-opened `mail/jmap` listener records. It also has
source-backed blob upload/download, bounded RFC 9404 `Blob/upload`, `Blob/get`, and `Blob/lookup`,
`Email/import`, and `Email/set` create from uploaded blobs. Current
JMAP routes expose stable `Identity/get` metadata, deterministic account/query/session state tokens,
`Email/changes`, and `Email/queryChanges`, and support direct TLS over hosted certificate-bundle
listener records. The session advertises upload/download capability but no event-source URL; JMAP push
is an explicit unsupported row until backed by 0035 durable delivery semantics. Full RFC 9051
conformance, recursive SEARCH operators, date/comparator search extensions, non-synchronizing literal
handling, and full JMAP conformance remain target work under 0008. Queue 7 reference-client validation
is owner-verified for Apple Mail and Thunderbird against the local certification harness; durable
redacted transcript storage and admin-visible certification status remain 0065 target work. The hosted
IMAP adapter explicitly
rejects non-synchronizing APPEND literals in tests, so that is a visible unsupported row rather than
hidden behavior. The hosted protocol conformance inventory reports supported rows for the bounded IMAP
profile, durable UID state, durable subscriptions, common SEARCH/STATUS/FETCH/STORE semantics, and
direct rustls IMAPS; degraded IDLE-without-push and unsupported non-synchronizing literals remain
explicit rows. The index record and flag set cross as Loom Canonical CBOR; the raw body crosses as
bytes.

## 5. Resolved decisions

- **RD1 - Body immutable in CAS; index and flags structured.** The body is the source of truth and the
  `.eml` projection is byte-identical to it; the index is derived from it. Confirmed in this build.
- **RD2 - Flags in a separate versioned sub-tree** (Decision A). Confirmed. Mail flag retention policy,
  mutable-state version tokens, and detailed deltas are source-backed; squash-bounded compaction remains
  target policy over that sub-tree.
- **RD3 - No SMTP in the base facet.** The facet stores, indexes, flags, and serves; it does not send.
  Thunderbird setup requires an outgoing server even for a read/manage-only IMAP account, so Loom
  exposes a setup-only hosted SMTP compatibility listener for the PIM certification harness. It is not
  real submission and remains outside the base-facet storage contract.
- **RD4 - Filters are programs (0041).** Gmail-style labeling/filtering is a program on
  `on_message_ingested`, run as a principal (0027), not facet-internal behavior.
- **RD5 - Primary surface is MCP + mount ingestion.** Hosted IMAP is the P2 wire projection, gated on
  the auth tier.
- **RD6 - Real SMTP is a separate PIM-owned submission presentation.** The current `mail/smtp` listener
  exists only for account-setup and bounded send-probe compatibility. Real SMTP submission, relay, and
  delivery are not a hidden extension of `mail` or a Queue 2 hosted data task. A future PIM-owned design
  must separately define authenticated submission, sender and recipient authority, message acceptance and
  persistence, queueing and retry, relay routing, delivery reports, abuse controls, TLS and DNS posture,
  audit, operational limits, and RFC/client conformance before any production SMTP listener is promised.

## 6. Dependencies and gating

The local facet and CLI (P0), MCP projection (P1), initial hosted IMAP projection (P2), bounded hosted
JMAP profile, setup-only hosted SMTP compatibility listener, and mutable-state retention records are
source-backed now. The `.eml` mount overlay rides the shared loom-vfs synthetic-projection mechanism.
Full RFC 9051 coverage beyond the bounded profile, full MIME validation, full RFC 9404 blob coverage, JMAP
push, JMAP contacts/calendars, real SMTP submission, and durable certification administration remain
hosted-protocol target work. Mailbox sharing is out of scope until explicitly requested. Filters
depend on 0015 + 0041.

## 7. Sources

- RFC 5322 (message format); IMAP4rev2 (RFC 9051); JMAP core/mail/blob (RFC 8620, RFC 8621,
  RFC 9404); SMTP setup compatibility (RFC 5321, RFC 6409, RFC 3207, RFC 8314, RFC 4954, RFC 4422,
  RFC 4616). Gmail labels/filters (informative).
- Shared model: 0024 (CAS), 0042 (collections), 0003d (commit diff), 0037 (the structured-facet pattern).

## 8. Hosted Mail Projection

Hosted mail completion is owned jointly by this spec and 0008. The bounded owner-only IMAP profile,
setup-only SMTP compatibility profile, and mutable-state records are source-backed for the rows listed
here; the deferred full profile remains target work.

Source-backed IMAP and mutable-state profile:

- source-backed durable per-mailbox numeric UID mapping, stable `UIDVALIDITY`, and stable `UIDNEXT`;
- source-backed durable `SUBSCRIBE`, `UNSUBSCRIBE`, and `LSUB` metadata scoped to principal and mailbox;
- source-backed RFC 6154 `SPECIAL-USE` role attributes for ordinary role mailboxes and Apple-visible
  role-folder discovery;
- source-backed common RFC 9051 search, fetch, store, literal, and mailbox edge-case coverage;
- non-synchronizing literal handling only if advertised;
- direct IMAPS over the shared hosted TLS policy;
- Apple Mail and Thunderbird Mail owner-verified certification evidence;
- source-backed mail flag retention policy records, mutable-state version tokens, detailed flag deltas,
  operation-style per-keyword merges, observed-version replacement conflicts, redacted audit summaries,
  compaction, and retained-gap full-resync responses.

Deferred full-profile rows:

- INBOX rename as part of full RFC 9051 promotion, with tests for standards-aligned INBOX special
  handling;
- recursive SEARCH operators, date/comparator extensions, full MIME/BODYSTRUCTURE fidelity, cleartext
  STARTTLS posture, and non-synchronizing literal acceptance remain target unless advertised;
- shared mailboxes, delegated mailboxes, send-as or submit-as service-principal behavior, and
  ACL-aware multi-principal mail serving are out of scope until explicitly requested;
- durable certification profile selection, transcript storage, certification status projection, and
  admin review remain 0065 target work.

Apple Mail account setup can offer a `Notes` toggle. That toggle is not a separate IETF Notes protocol
and is not covered by RFC 6154 SPECIAL-USE, which defines mail special-use attributes such as `\Sent`,
`\Drafts`, `\Trash`, `\Archive`, and `\Junk`, but not a standard `\Notes` attribute. Loom exposes an
ordinary subscribed IMAP `Notes` mailbox in the certification harness for observed Apple account setup
compatibility. Within that mailbox, Apple note edits are handled as a documented compatibility profile:
`APPEND Notes` carrying `X-Uniform-Type-Identifier: com.apple.mail-note` and a stable
`X-Universally-Unique-Identifier` replaces the existing message with the same note UUID instead of
allocating a second message. Full Apple Notes semantics beyond this mailbox/profile remain proprietary
client behavior unless promoted into a separate Loom notes facet.

SMTP remains outside the base facet. Thunderbird certification proved that account setup needs an
outgoing server, so Loom exposes the smallest authenticated hosted SMTP compatibility listener needed
for setup. It requires `STARTTLS`, accepts fixture credentials, and accepts submitted `DATA` only so
setup and send-probe flows can complete. It does not relay, deliver, or mutate the mail facet. Real
submission remains a separate hosted presentation unless promoted explicitly.

The durable JMAP target is:

- source-backed CAS-backed blob upload and download for message bytes, `Email/import`, and `Email/set`
  create from uploaded blobs;
- source-backed stable JMAP email identity metadata separate from IMAP UID mappings;
- source-backed deterministic account, query, and session state tokens;
- source-backed full-resync `Email/changes` and `Email/queryChanges` behavior over deterministic state
  tokens;
- `Email/set` create/import wired through the blob service;
- push is unsupported until backed by 0035 durable delivery semantics;
- direct TLS over the shared hosted TLS policy is source-backed for current JMAP routes;
- native JMAP client or conformance-tool certification remains a 0065 target. Queue 7 closes the current
  JMAP slice on source-backed hosted RFC 8620/8621 transcripts and owner acceptance because no external
  JMAP tool is available in the certification environment.

Mutable mail state is ordinary mail facet state. Current flag state is retained indefinitely. Mail flag
retention policy records, mutable-state version tokens, detailed per-message flag deltas,
operation-style per-keyword updates, observed-version replacement conflicts, and redacted audit
summaries, compaction, and retained-gap responses are source-backed in `loom-core::mail`. Concurrent
operation-style updates merge per keyword; stale full replacements fail or require rebase. Stale
incremental sync tokens that predate retained detailed history fail with a stable retained-gap response
requiring full resync.

Delegated mailboxes and shared mailbox policy are out of scope until explicitly requested.

### Queue 7 IMAP and mutable-state closure evidence

| Evidence area | Disposition | Source-backed references |
| --- | --- | --- |
| Bounded IMAP access | Degraded against full RFC 9051, source-backed for the bounded owner-only profile. Authentication, mailbox list/create/delete/rename except INBOX/select/status, bounded LIST-EXTENDED for SPECIAL-USE, CHILDREN attributes, and RETURN STATUS for supported STATUS items, durable UID mapping, UIDVALIDITY, UIDNEXT, durable subscriptions, metadata/body/partial/header/text FETCH, UID FETCH, STORE, SEARCH, COPY/MOVE, synchronizing APPEND, EXPUNGE, CLOSE, UNSELECT, ENABLE no-op, WORKSPACE, IDLE, LOGOUT, literals, flags, and direct IMAPS are source-backed. Full IMAP4rev2 advertisement, cleartext STARTTLS posture, INBOX rename special handling, full arbitrary LIST-EXTENDED, non-synchronizing literal acceptance, recursive SEARCH, full MIME/BODYSTRUCTURE, and broader reference-client automation remain target or unsupported. | `crates/loom-hosted-pim/src/imap.rs`; `crates/loom-conformance/src/lib.rs` rows `mail/imap/bounded-rfc9051-profile`, `mail/imap/durable-uid-state`, `mail/imap/durable-subscriptions`, `mail/imap/common-search-status-fetch-store`, `mail/imap/direct-rustls-imaps`, and `mail/rfc-gate/imap-rfc9051-bounded-profile`; Queue 7 task 50.5.12. |
| RFC 6154 role mailboxes | Supported for the bounded role-mailbox profile. `SPECIAL-USE` is advertised; role mailboxes emit `\Archive`, `\Drafts`, `\Junk`, `\Sent`, and `\Trash`; common extended LIST discovery forms are accepted; the certification seeder creates and subscribes role mailboxes plus ordinary `Notes`. | `crates/loom-hosted-pim/src/imap.rs`; `crates/loom-cli/examples/pim_cert_seed.rs`; Queue 7 task 50.5.32. |
| Apple Mail Notes compatibility | Source-backed as an IMAP compatibility profile, not as a separate standard notes protocol. `APPEND Notes` with Apple note headers replaces the existing message with the same note UUID. Full Apple Notes semantics remain proprietary behavior unless promoted into a separate Loom notes facet. | `crates/loom-hosted-pim/src/imap.rs`; Queue 7 task 90.3.1. |
| Setup-only SMTP disposition | Supported only for account setup compatibility. The hosted listener requires STARTTLS when configured, authenticates fixture users, accepts bounded SMTP probe DATA, and does not relay, deliver, or mutate the mail facet. Real submission remains outside the base facet and target unless promoted explicitly. | `crates/loom-hosted-pim/src/smtp.rs`; `crates/loom-conformance/src/lib.rs` rows `mail/smtp/setup-compatibility-listener`, `mail/smtp/real-submission-relay-delivery`, and SMTP RFC-gate rows; Queue 7 SMTP setup tasks in 50.5. |
| Mutable flag state | Supported. Mailbox flag retention policy, version tokens, detailed deltas, per-keyword operation merge, observed-version replacement conflicts, redacted audit summaries, compaction, and retained-gap full-resync responses are source-backed. | `crates/loom-core/src/mail.rs`; `crates/loom-conformance/src/lib.rs` rows `mail/mutable-state/flag-policy-version-deltas`, `mail/mutable-state/flag-merge-audit`, and `mail/mutable-state/flag-compaction-retained-gap`; Queue 7 tasks 80.1 through 80.3. |
| Owner-only access profile | Supported for the hosted mail profile. Shared mailboxes, delegated mailboxes, send-as or submit-as service-principal behavior, and cross-principal mailbox discovery are out of scope until explicitly requested. | `specs/0008-wire-protocols.md`; `crates/loom-conformance/src/lib.rs` row `pim/access/owner-only-profile`; Queue 7 task 85. |
| Reference clients | Owner-verified for Apple Mail and Thunderbird against the local certification harness. This is client evidence, not a substitute for the RFC rows above. Durable transcript capture and review are 0065 target work. | `_QUEUE7.md` tasks 90.3 and 90.3.1; `crates/loom-conformance/src/lib.rs` `PIM_CERTIFICATION_CLIENT_REQUIREMENTS` and `PIM_TRANSCRIPT_INVENTORY`; `scripts/pim-cert/README.md`. |

### Mail RFC implementation gate

Queue 7 cannot certify reference clients until this gate is either source-backed or explicitly recorded
as unsupported, degraded, target, or deferred in the hosted capability report. Shared HTTP, TLS, URI,
discovery, Basic auth, and listener posture are owned by 0008. This table owns mail-specific protocol
and data behavior. The hosted capability inventory splits this gate into
`mail/rfc-gate/message-rfc5322-bounded-profile`,
`mail/rfc-gate/imap-rfc9051-bounded-profile`,
`mail/rfc-gate/jmap-core-rfc8620-bounded-profile`,
`mail/rfc-gate/jmap-mail-rfc8621-bounded-profile`,
`mail/rfc-gate/jmap-rfc8620-rfc8621-bounded-profile`,
`mail/rfc-gate/jmap-blob-rfc9404`,
`mail/rfc-gate/jmap-quotas-rfc9425`,
`mail/rfc-gate/jscontact-rfc9553`,
`mail/rfc-gate/vcard-jscontact-extensions-rfc9554`,
`mail/rfc-gate/jscontact-vcard-conversion-rfc9555`,
`mail/rfc-gate/jmap-contacts-rfc9610`,
`mail/rfc-gate/jscalendar-rfc8984`,
`mail/rfc-gate/jmap-calendars-draft-ietf-jmap-calendars`,
`mail/rfc-gate/jmap-sharing-rfc9670`,
`mail/rfc-gate/web-push-rfc8030`,
`mail/rfc-gate/vapid-rfc8292`,
`mail/rfc-gate/jmap-webpush-vapid-rfc9749`,
`mail/rfc-gate/smtp-rfc5321-setup-profile`,
`mail/rfc-gate/smtp-submission-rfc6409-setup-profile`,
`mail/rfc-gate/smtp-starttls-rfc3207-setup-profile`,
`mail/rfc-gate/email-tls-rfc8314-bounded-profile`,
`mail/rfc-gate/email-submission-ops-rfc5068`,
`mail/rfc-gate/smtp-auth-rfc4954-bounded-profile`,
`mail/rfc-gate/sasl-rfc4422-bounded-profile`,
`mail/rfc-gate/sasl-plain-rfc4616-bounded-profile`,
`mail/rfc-gate/mailto-rfc6068-unsupported`,
`mail/rfc-gate/smtp-size-rfc1870-bounded-profile`,
`mail/rfc-gate/smtp-pipelining-rfc2920`,
`mail/rfc-gate/enhanced-status-codes-rfc3463`,
`mail/rfc-gate/enhanced-status-registry-rfc5248`,
`mail/rfc-gate/smtp-8bitmime-rfc6152-bounded-profile`,
`mail/rfc-gate/smtputf8-rfc6531`,
`mail/rfc-gate/internationalized-headers-rfc6532`,
`mail/rfc-gate/mime-format-rfc2045`,
`mail/rfc-gate/mime-media-types-rfc2046`,
`mail/rfc-gate/mime-encoded-words-rfc2047`,
`mail/rfc-gate/mime-conformance-rfc2049`,
`mail/rfc-gate/smtp-setup-auth-session-profile`,
`mail/rfc-gate/smtp-starttls-standard-port-transcript`, and
`mail/rfc-gate/smtp-optional-extensions-mixed-profile`.

#### JMAP RFC gate

| RFC or draft | Area | Acceptance rule |
| --- | --- | --- |
| RFC 8620 | JMAP core | Degraded against full RFC 8620 and source-backed for the bounded hosted JMAP core profile. Session discovery, capability advertisement, account state, `using` validation, request and response object validation, method calls, `Core/echo`, JSON-pointer result references, method-level errors, request-level `unknownCapability`, upload/download behavior, push capability posture with `eventSourceUrl: null`, and state tokens have executable hosted transcript coverage. Push subscriptions, event-source delivery, `Blob/copy`, full standard-method semantics beyond the advertised mail subset, localization, and broader request-level error registry coverage remain target or unsupported rows. |
| RFC 8621 | JMAP mail | Degraded against full RFC 8621 and source-backed for the bounded hosted JMAP Mail profile. The advertised mail capability is backed by executable `Mailbox/get`, `Mailbox/changes`, `Mailbox/query`, `Mailbox/queryChanges`, `Mailbox/set`, `Thread/get`, `Thread/changes`, `Email/query`, `Email/get`, `Email/changes`, `Email/queryChanges`, `Email/set` update/destroy/create, `Email/copy`, `Email/import`, `Email/parse`, `SearchSnippet/get`, `Identity/get`, `Identity/changes`, and `Identity/set` responses over the owner-only single-mailbox-per-message model. Full RFC 8621 property coverage, multi-mailbox labels, stable thread grouping beyond single-message threads, rich MIME body part parsing, search snippet generation, EmailSubmission, VacationResponse, MDN behavior, and push remain unsupported, target, or separately gated. |
| RFC 9404 | JMAP blob management | Degraded against full RFC 9404 and source-backed for the bounded owner-only profile. The `urn:ietf:params:jmap:blob` capability advertises `Blob/upload`, `Blob/get`, and `Blob/lookup` over the existing CAS blob store with account authorization, created-id blob references, bounded data-source count, size limits, text/base64/range retrieval, encoding-problem reporting, and lookup results limited to owner-visible Mailbox, Thread, and Email identifiers. Digest properties are not advertised because `supportedDigestAlgorithms` is empty. `Blob/copy` and full RFC 9404 breadth remain target or unsupported rows. |
| RFC 9425 | JMAP quotas | Degraded against full RFC 9425 and source-backed for the bounded owner-only account-octets profile. The `urn:ietf:params:jmap:quota` capability is advertised, `using` accepts it, and `Quota/get` exposes one account-scoped `octets` quota over owner-visible Mail messages with the operator hard-limit policy. `Quota/changes`, `Quota/query`, `Quota/queryChanges`, push notifications, count quotas, and domain or global quota projection remain target or unsupported. |
| RFC 9553 | JSContact | Target only. The JSContact Card data model, I-JSON representation, required `uid` and `version` handling, typed contact properties, localizations, extension rules, versioning, registry behavior, and security posture are not source-backed. JSContact is required before JMAP contacts can be promoted; CardDAV remains the Queue 7 contacts standard. |
| RFC 9554 | vCard format extensions for JSContact | Target only. The RFC 9554 updates to RFC 6350, including extended `ADR` and `N` components, `CREATED`, `GRAMGENDER`, `LANGUAGE`, `PRONOUNS`, `SOCIALPROFILE`, JSContact alignment parameters, and new address type values are not source-backed. |
| RFC 9555 | JSContact/vCard conversion | Target only. The bidirectional conversion rules between vCard and JSContact, generated JSContact identifiers, unknown property preservation, JSContact-only vCard extension properties and parameters, and conversion vectors are not source-backed. |
| RFC 9610 | JMAP contacts | Target and not advertised. The RFC 9610 `urn:ietf:params:jmap:contacts` capability, account capability object, AddressBook methods, ContactCard methods, JSContact card storage, blob media behavior, address-book membership invariants, contact filtering and sorting, sharing hooks, internationalization behavior, and `addressBookHasContents` error are not advertised until RFC 9553 and RFC 9555 dependencies are source-backed. |
| RFC 8984 | JSCalendar | Target only. The JSCalendar Event, Task, and Group data model, I-JSON representation, object identity, recurrence model, participants, alerts, localizations, time-zone objects, extension registry behavior, and security posture are not source-backed. JSCalendar is required before JMAP calendars can be promoted; CalDAV remains the Queue 7 calendar standard. |
| draft-ietf-jmap-calendars | JMAP calendars | Target and not advertised. The current tracked draft is `draft-ietf-jmap-calendars-26`, an active Internet-Draft in the RFC Editor queue. The `urn:ietf:params:jmap:calendars`, `urn:ietf:params:jmap:principals:availability`, and `urn:ietf:params:jmap:calendars:parse` capabilities, Principal availability methods, ParticipantIdentity methods, Calendar methods, CalendarEvent methods, alert behavior, event notification methods, recurrence expansion limits, scheduling errors, and push behavior are not advertised. This draft must not gate Queue 7 CalDAV certification unless JMAP calendars is promoted. |
| RFC 9670 | JMAP sharing | Out of scope and not advertised until explicitly requested. The RFC 9670 `urn:ietf:params:jmap:principals` capability, `urn:ietf:params:jmap:principals:owner` account capability, Principal methods, ShareNotification methods, subscribed shared-account filtering rules, delegated account exposure, and sharing security posture are not advertised by the owner-only mail profile. |
| RFC 8030 | Generic event delivery using HTTP Push | Deferred. Web Push subscription management, push message delivery, receipts, TTL, urgency, replacement, acknowledgment, expiration, load management, and security/privacy posture are not source-backed because JMAP push is not implemented. |
| RFC 8292 | VAPID for Web Push | Deferred. Application-server identification for push messages, VAPID key management, authentication headers, token signing, and security posture are not source-backed because Web Push delivery is not implemented. |
| RFC 9749 | JMAP Web Push VAPID | Deferred and not advertised. The RFC 9749 `urn:ietf:params:jmap:webpush-vapid` capability, application server key advertisement, authenticated push POST behavior, key rotation handling, stale subscription destruction, and push verification security behavior are not advertised until JMAP push is backed by 0035 durable delivery semantics and RFC 8030/RFC 8292 dependencies are source-backed. |

### Queue 7 JMAP and final mail closure evidence

| Evidence area | Disposition | Source-backed references |
| --- | --- | --- |
| Bounded JMAP core | Degraded against full RFC 8620, source-backed for the hosted owner-only profile. Session discovery, capability advertisement, account state, `using` validation, request and response object validation, method calls, `Core/echo`, result references, method-level errors, request-level `unknownCapability`, upload/download URLs, push capability posture with `eventSourceUrl: null`, deterministic state tokens, and direct HTTPS/TLS serving are source-backed. Push subscriptions, event-source delivery, `Blob/copy`, full standard-method breadth, localization, and broader request error coverage remain target or unsupported. | `crates/loom-hosted-pim/src/jmap.rs`; `crates/loom-conformance/src/lib.rs` rows `mail/jmap/bounded-rfc8620-rfc8621-profile`, `mail/jmap/executable-rfc8620-rfc8621-transcript`, `mail/rfc-gate/jmap-core-rfc8620-bounded-profile`, and `mail/rfc-gate/jmap-rfc8620-rfc8621-bounded-profile`; Queue 7 task 50.5.6. |
| Bounded JMAP mail | Degraded against full RFC 8621, source-backed for the advertised mail subset. `Mailbox/get`, `Mailbox/changes`, `Mailbox/query`, `Mailbox/queryChanges`, `Mailbox/set`, `Thread/get`, `Thread/changes`, `Email/query`, `Email/get`, `Email/changes`, `Email/queryChanges`, `Email/set` update/destroy/create, `Email/copy`, `Email/import`, `Email/parse`, `SearchSnippet/get`, `Identity/get`, `Identity/changes`, and `Identity/set` are covered by the owner-only profile. Full property coverage, multi-mailbox labels, rich MIME body parts, EmailSubmission, VacationResponse, MDN behavior, and push remain unsupported, target, or separately gated. | `crates/loom-hosted-pim/src/jmap.rs`; `crates/loom-conformance/src/lib.rs` rows `mail/rfc-gate/jmap-mail-rfc8621-bounded-profile` and `mail/rfc-gate/jmap-rfc8620-rfc8621-bounded-profile`; Queue 7 task 50.5.7. |
| JMAP blob posture | Degraded against full RFC 9404 and source-backed for the bounded owner-only profile. RFC 8620 upload/download URLs and RFC 9404 `Blob/upload`, `Blob/get`, and `Blob/lookup` are backed by the hosted CAS blob store with owner authorization, size limits, range reads, text/base64 data shaping, created-id blob references, and lookup limited to owner-visible Mailbox, Thread, and Email identifiers. Digest properties, `Blob/copy`, and full RFC 9404 breadth remain target or unsupported rows. | `crates/loom-hosted-pim/src/jmap.rs`; `crates/loom-conformance/src/lib.rs` row `mail/rfc-gate/jmap-blob-rfc9404`; Queue 7 task 50.5.8. |
| JMAP quota posture | Degraded against full RFC 9425 and source-backed for the bounded owner-only profile. RFC 9425 `Quota/get` projects the native account-octets primitive and hard-limit policy as one `mail-octets` quota for the authenticated mail account, with unrelated quota ids returned in `notFound`. Changes/query methods, push, count quotas, and domain or global quota visibility remain target or unsupported. | `crates/loom-hosted-pim/src/jmap.rs`; `crates/loom-core/src/mail.rs`; `crates/loom-conformance/src/lib.rs` row `mail/rfc-gate/jmap-quotas-rfc9425-bounded-profile`; Queue 7 task 50.5.8.1. |
| JMAP push and delivery dependencies | Unsupported or target. JMAP push is not advertised beyond `eventSourceUrl: null`; Web Push, VAPID, and JMAP Web Push VAPID remain deferred until 0035 durable delivery semantics exist. | `specs/0035-durable-delivery.md`; `crates/loom-conformance/src/lib.rs` rows `mail/jmap/push`, `mail/rfc-gate/web-push-rfc8030`, `mail/rfc-gate/vapid-rfc8292`, and `mail/rfc-gate/jmap-webpush-vapid-rfc9749`; Queue 7 tasks 50.5.14 through 50.5.16. |
| JMAP contacts and calendars | Target or deferred. JSContact, JSContact/vCard conversion, JMAP contacts, JSCalendar, and JMAP calendars are not advertised by the owner-only profile. CardDAV and CalDAV remain the Queue 7 standards for contacts and calendar. JMAP sharing for mail is out of scope until explicitly requested. | `crates/loom-conformance/src/lib.rs` JMAP contacts/calendar/sharing RFC-gate rows; Queue 7 tasks 50.5.9 through 50.5.13 and 50.5.16.1. |
| JMAP certification evidence | Owner-accepted green without external JMAP client or tool evidence because no such tool is available in the certification environment. Queue 7 relies on source-backed hosted router transcripts and conformance rows. Native JMAP client or tool certification remains a 0065 target. | `_QUEUE7.md` task 90.4; `crates/loom-conformance/src/lib.rs` `PIM_CERTIFICATION_CLIENT_REQUIREMENTS`, `PIM_TRANSCRIPT_INVENTORY`, and `QUEUE7_PIM_CAPABILITY_REPORT_FIXTURES`; `crates/loom-hosted-pim/src/jmap.rs`. |
| Final deferred mail rows | Real SMTP submission, relay, delivery, production submission operations, MIME validation, SMTP optional extensions not advertised by the setup profile, native JMAP client/tool certification, and durable certification administration remain target, unsupported, or deferred as recorded in this spec and 0065. Mailbox sharing is out of scope until explicitly requested. | `crates/loom-conformance/src/lib.rs` SMTP/JMAP/mail RFC-gate rows; `specs/0065-AdminInterface.md`; Queue 7 tasks 50.5 and 90.4. |

### Queue 10 local hardening evidence

Queue 10 closed the local non-binding hardening pass for mail without moving the unfinished binding
parity, binding runtime, or coverage-reporting rows below. Evidence:

| Surface | Queue 10 evidence | Source-backed references |
| --- | --- | --- |
| MCP | Mail tool schemas elide agent-supplied principal, server-side binding overwrites any scoped argument, malformed PIM arguments are rejected, mail resource reads reject malformed or unauthorized targets, `.eml` resource bodies are projected, and mail prompts stay in the registered prompt inventory. | `crates/loom-mcp/src/server/tests.rs` tests `registered_prompts_equal_the_surface`, `pim_binding_injects_and_overwrites_agent_scope`, `pim_arguments_reject_missing_or_malformed_scope_fields`, `binding_scopes_resource_templates_and_reads`, and `pim_resource_reads_project_domain_bodies`; `_QUEUE10.md` tasks 20 and 60. |
| Compute/WASM | Direct `StateAccess` and guest WASM calls cover mail capability denial, missing mailbox mapping, malformed guest record bytes, cross-principal isolation, and the existing positive domain-shaped mail host-call round trip. | `crates/loom-compute/src/state_access.rs` test `pim_state_access_denies_modes_facets_and_missing_collections`; `crates/loom-compute/src/engine_wasmi.rs` tests `pim_host_abi_rejects_malformed_records_and_denied_grants` and `pim_domain_records_round_trip_through_host_abi`; `_QUEUE10.md` task 30. |
| VFS | The `.eml` overlay covers missing mailbox handling without quarantine, filename-stem UID behavior, invalid input quarantine, and projected-record delete support through the shared overlay path. | `crates/loom-vfs/src/overlay.rs` tests `missing_collection_is_not_quarantined`, `mail_uses_the_filename_stem_as_uid`, and `overlay_quarantines_unparseable_facet_file`; `crates/loom-vfs/src/lib.rs` test `overlay_unlinks_projected_facet_record`; `_QUEUE10.md` task 40. |
| Local CLI and C ABI | Local smoke tests cover missing mailboxes through the CLI and missing mailboxes plus malformed flag payloads through the C ABI path. | `crates/loom-cli/src/helpers.rs` test `pim_cli_reports_missing_containers`; `crates/loom-ffi/src/tests.rs` test `calendar_contacts_mail_round_trip_over_the_c_abi`; `_QUEUE10.md` task 50. |

### Unfinished binding and coverage items

These unfinished spec items are not Queue 10 scope.

| Priority | Item | Status | Owning follow-up |
| --- | --- | --- | --- |
| P1 | Per-binding mail parity matrix for Node, Python, C++, Swift/iOS, JVM, Android, React Native, WASM, C ABI, and IDL shapes across mailbox operations, `.eml` projection, flags, capability errors, and hosted-adjacent IMAP/JMAP expectations. | Unfinished | P9 binding specs or a later binding certification queue. |
| P1 | Binding runtime tests for positive, negative, and boundary mail cases in each language binding. | Unfinished | P9 binding specs or a later binding certification queue. |
| P2 | Coverage reporting by mail surface and binding family, distinct from Rust line coverage. | Unfinished | 0010a conformance reporting or a later binding certification queue. |

#### SMTP setup-compatibility RFC gate

| RFC | Area | Acceptance rule |
| --- | --- | --- |
| RFC 5321 | SMTP transport | Supported for setup-only compatibility. The hosted listener speaks the bounded SMTP command/reply profile for greeting, EHLO/HELO, MAIL, RCPT, DATA, RSET, NOOP, QUIT, reply codes, CRLF framing, line length limits, DATA terminator handling, command sequencing, and stable rejection of unsupported behavior. Real relay, queueing, DNS MX routing, gatewaying, trace mutation, retry, bounce, and delivery remain unsupported. |
| RFC 5322 | Internet message format | Submitted DATA is accepted only as a setup or send-probe payload. Raw bytes may be discarded or retained as explicit evidence, but the listener must not claim delivery, MIME validation, or mail-facet mutation. |
| RFC 6409 | Message submission | Supported for setup-only compatibility. The port-587 posture is submission-shaped for reference-client account setup: the listener requires authentication before `MAIL`, accepts fixture credentials, applies the RFC 5321 setup command profile, and does not relay, queue, deliver, rewrite, sign, encrypt, bounce, or enforce production submission policy. |
| RFC 3207 | SMTP STARTTLS | Supported for the setup-only profile. SMTP listener records can use `tls.mode=starttls`; the listener advertises `STARTTLS` before authentication when TLS is configured, rejects `STARTTLS` parameters, reports `454` when TLS is unavailable, removes `STARTTLS` after TLS is active, requires a fresh SMTP phase after the handshake, and the local RFC probe records an authenticated STARTTLS setup transcript. |
| RFC 8314 | TLS for email submission and access | Degraded against full RFC 8314 and source-backed for the bounded server-side TLS posture. The hosted profile supports SMTP STARTTLS setup, implicit-TLS SMTP submission service wiring, direct IMAPS, direct HTTPS JMAP, TLS 1.2+ policy through the hosted TLS stack, and clear unsupported rows for production submission, relay, and delivery. POP, DNS SRV/TLSA publication, MUA certificate-validation behavior, client certificate authentication, Received-header TLS annotation, production submissions accountability, and full RFC 5068 operations remain target or unsupported. |
| RFC 5068 | Email submission operations | Target. Production email submission operations are not claimed by the setup-only SMTP compatibility listener. Submission authorization, abuse accountability, traceability after submission, external submission operations, Received-header submission metadata, and inter-operator accountability are not source-backed until real submission is promoted. |
| RFC 4954 | SMTP AUTH | Degraded against full RFC 4954 and source-backed for the bounded setup AUTH profile. `AUTH` is advertised for implemented mechanisms, requires STARTTLS first when TLS is configured, supports initial-response and continuation exchanges for the implemented mechanisms, maps success, credential failure, malformed exchange, cancellation, repeated AUTH, and mail-transaction AUTH to stable replies, and blocks `MAIL` before authentication. The `MAIL FROM AUTH=` parameter, SASL security-layer negotiation, SASLprep, enhanced status codes, and production trace annotations remain target or unsupported. |
| RFC 4422 | SASL framework | Degraded against full RFC 4422 and source-backed for the bounded SMTP AUTH exchange profile. The hosted SMTP layer supports mechanism negotiation through RFC 4954 `AUTH`, challenge/response framing for the implemented mechanisms, authorization identity binding for the fixture principal, cancellation, success, failure, and repeated-auth outcomes. A generic SASL framework, EXTERNAL, registry enforcement, SASLprep, proxy authorization policy, server-authentication mechanisms, and SASL security layers are not source-backed. |
| RFC 4616 | SASL PLAIN | Degraded against full RFC 4616 and source-backed for the bounded setup profile. `AUTH PLAIN` supports initial-response and continuation forms, UTF-8 credential fields, empty authorization identity derivation from authentication identity, non-empty authorization identity equality checks, credential failure, malformed exchange failure, cancellation, and fixture-principal binding. SASLprep/StringPrep normalization and a reusable SASL profile are not source-backed. |
| RFC 6068 | `mailto` URI | Unsupported and not advertised. Loom has no compose URI resolver, send surface, or `mailto` parser. Promotion requires URI parsing, percent-encoding, safe header filtering, `subject` and `body` behavior, internationalized address policy, duplicate-recipient handling, unsafe header rejection, and explicit user/agent send confirmation policy. |
| RFC 1870 | SMTP SIZE | Source-backed for the bounded setup-only profile. EHLO advertises the fixed maximum message size, `MAIL FROM` accepts one `SIZE` parameter, malformed or duplicate size declarations return `501`, over-limit declarations return `552`, DATA transfer still uses the DATA terminator rather than declared size, and actual DATA byte limits are enforced. Per-recipient resource checks, temporary storage failures, delivery queue reservation, and relay policy remain unsupported. |
| RFC 2920 | SMTP PIPELINING | Target and not advertised. The listener does not advertise `PIPELINING`. Promotion requires explicit ordered-response tests for grouped commands, DATA synchronization, multiline responses, failed RCPT groups, no input-buffer loss after failures, and no response buffering for EHLO, DATA, VRFY, EXPN, TURN, QUIT, NOOP, or unknown commands. |
| RFC 3463 | Enhanced mail system status codes | Target and not emitted. The setup-only SMTP listener emits plain SMTP reply codes only. Promotion requires valid enhanced status code syntax, registered code selection, matching class semantics, and tests for every emitted enhanced status code. |
| RFC 5248 | Enhanced mail system status code registry | Target and not emitted. No enhanced status codes are emitted today. Promotion requires registry-backed code selection, update policy, stable extension handling, and drift tests for every emitted enhanced status code. |
| RFC 6152 | 8BITMIME | Source-backed for the bounded setup-only profile. EHLO advertises `8BITMIME`, `MAIL FROM` accepts one `BODY=7BIT` or `BODY=8BITMIME` parameter, invalid or duplicate BODY parameters return `501`, DATA accepts high-bit octets while preserving line and size limits, and setup acceptance does not relay, deliver, validate MIME, or transform content. Full delivery/relay bit-preservation semantics remain unsupported because setup DATA is discarded. |
| RFC 6531 | SMTPUTF8 | Target and not advertised. The listener does not advertise `SMTPUTF8`. Promotion requires UTF-8 envelope mailbox parsing, `SMTPUTF8` MAIL parameter handling, IDNA domain policy, UTF-8 reply behavior, trace-field posture, interaction with `8BITMIME`, DSN behavior, and reference tests. |
| RFC 6532 | Internationalized email headers | Target. Internationalized header syntax is not claimed by the bounded immutable-message profile or setup-only SMTP listener. Promotion requires UTF-8 header parse/serialize vectors, MIME interaction, line folding behavior, downgrade policy, SMTPUTF8 integration, and client-visible projection tests. |
| RFC 2045 | MIME Part One: format of Internet message bodies | Target and no validation claim. Loom stores raw message bytes and parses bounded RFC 5322 headers, but it does not validate `MIME-Version`, `Content-Type`, `Content-Transfer-Encoding`, canonical encodings, quoted-printable, base64, content IDs, or MIME entity structure. |
| RFC 2046 | MIME Part Two: media types | Target and no validation claim. Loom does not validate MIME top-level media types, multipart boundary structure, message media types, text media behavior, application media behavior, or unrecognized media handling. |
| RFC 2047 | MIME Part Three: encoded words | Target and no validation claim. Loom does not claim encoded-word validation, charset decoding, placement rules, adjacent encoded-word handling, or encoded display-name projection beyond what the bounded RFC 5322 parser exposes. |
| RFC 2049 | MIME Part Five: conformance criteria and examples | Target and no validation claim. Loom does not claim MIME-conformant generation, full receive conformance, media-type interpretation, canonicalization, gateway behavior, or MIME example compatibility. |

#### IMAP and message-format RFC gate

| RFC | Area | Acceptance rule |
| --- | --- | --- |
| RFC 5322 | Internet message format | Supported for the bounded immutable-message profile: raw message bytes are stored in CAS, the structured index parses `From`, `To`, `Subject`, `Date`, `Message-ID`, folded headers, size, and a header bag, and `.eml` retrieval returns digest-verified raw bytes. MIME validation, internationalized headers, delivery semantics, and SMTP envelope behavior are handled by separate rows and are not claimed by this row. |
| RFC 9051 | IMAP4rev2 | Degraded against full IMAP4rev2 and source-backed for the bounded hosted IMAP profile. The listener does not advertise `IMAP4rev2` or `STARTTLS` until the full requirements behind those claims are source-backed. Current source-backed behavior covers authentication, mailbox list/create/delete/rename except INBOX/select/status, bounded LIST-EXTENDED for SPECIAL-USE, CHILDREN attributes, and RETURN STATUS for supported STATUS items, empty mailbox-name rejection before storage-path resolution, hosted-kernel write serialization for concurrent client sessions, durable UID mapping, UIDVALIDITY, UIDNEXT, subscriptions, metadata-only FETCH, full/peek body FETCH, partial body FETCH, header/text section FETCH, UID FETCH, STORE, SEARCH, COPY/MOVE, APPEND with synchronizing literals, Apple Notes UUID replacement within the ordinary `Notes` mailbox compatibility profile, EXPUNGE, CLOSE, UNSELECT, ENABLE no-op negotiation, WORKSPACE, IDLE, LOGOUT, literals, flags, and the bounded RFC 6154 SPECIAL-USE role subset. Missing or target full-IMAP4rev2 areas include cleartext STARTTLS and LOGINDISABLED posture, INBOX rename special handling, full arbitrary LIST-EXTENDED, non-synchronizing literal acceptance, recursive SEARCH operators, full MIME/BODYSTRUCTURE fidelity, and automated reference-client certification. |
| RFC 6154 | IMAP LIST extension for special-use mailboxes | Source-backed for the bounded role-mailbox profile. `CAPABILITY` advertises `SPECIAL-USE` and `LIST-EXTENDED`; `LIST` and `LSUB` responses emit `\Archive`, `\Drafts`, `\Junk`, `\Sent`, and `\Trash` for matching ordinary mailboxes; the parser accepts common extended `LIST "" "*" RETURN (SPECIAL-USE)`, `LIST (SPECIAL-USE) "" "*"`, and bounded `RETURN (STATUS (...))` forms for supported STATUS items; the PIM cert seeder creates and subscribes role mailboxes plus an ordinary subscribed `Notes` mailbox for Apple account setup. Loom does not claim full arbitrary LIST-EXTENDED selection-option semantics beyond this compatibility profile. |

## Change log

### Local facet (P0): source-backed, capability executable

`loom-core::mail` implements the facet: `FacetKind::Mail` (wired through `as_str`/`from_str`/
compression), `MailMessage`/`MailboxMeta`, mailbox CRUD, `ingest_message` (immutable body to CAS via
`cas_put`, RFC 5322 header parse with unfolding into a structured index under `msg/<uid>`), `get_message`/
`to_eml` (byte-exact raw .eml from CAS, digest-verified)/`delete_message`/`list_messages`, independent
`get_flags`/`set_flags` over the separate `flags/<uid>` sub-tree (sorted, deduplicated; Decision A),
mail flag retention policy records, mutable-state version tokens, detailed flag deltas,
operation-style flag updates, observed-version replacement checks, redacted audit summaries, `search`,
and `diff_messages` (added/removed). The `interface Mail` IDL is in place.
header parsing in `ingest_message` uses the **`mail-parser`** crate (0.11, full RFC 5322/MIME) rather
than a hand-rolled parser: `From`/`To` render to address strings, `Subject`, `Date` (rfc3339),
`Message-ID` (the library strips the angle brackets), and the raw header index. The immutable body still
goes to CAS byte-for-byte (mail-parser only feeds the index). MIT/Apache, deny.toml-clean.
`run_mail_facade_behavior` (loom-conformance) exercises the full facade plus a clone target (proving both
the index and the CAS body travel) and is wired into `certify_memory_store`, `BEHAVIOR_SUITES` (with
`MAIL_SCENARIOS`), and `EXECUTABLE_BEHAVIOR_SUITES`; the aggregate certification passes. The `mail`
capability is `executable` in the registry and the 0010 section 5 table (drift test green). Tests: 4
in-crate. The shared binding pass is done: the `loom_mail_*` C ABI (11 functions, including
`ingest_message`, `to_eml`, `get_flags`/`set_flags`) is in loom-ffi and projected into all eight
language bindings as part of the calendar/contacts/mail trio pass (loom-ffi 36 tests green; language
bindings build via `just test-bindings`). The shared `loom-vfs` overlay projects
`mail/<principal>/<mailbox>/<uid>.eml` as the byte-exact CAS body and ingests dropped `.eml` files
through `ingest_message`. The `loom-mcp` host projects mail as curated tools, `.eml` resources, and mail
prompts with server-side principal injection. The hosted IMAP opener is source-backed in
`crates/loom-hosted-pim/src/imap.rs` and `crates/loom-cli/src/daemon_cmd.rs`; focused tests cover protocol
login and SASL PLAIN authentication, mailbox LIST patterns including RFC 9051 section 6.3.9
`LIST "" ""` hierarchy delimiter discovery, mailbox create/delete, durable per-mailbox numeric UID
state, stable `UIDVALIDITY`, stable `UIDNEXT`, durable SUBSCRIBE/UNSUBSCRIBE/LSUB state,
synchronizing-literal append, fetch-attribute validation, STORE `.SILENT`, flag mutation, bounded
common search criteria, copy/move between mailboxes, RFC 3501 selected-state `CHECK` compatibility
while the listener advertises `IMAP4rev1`, RFC 6154 `SPECIAL-USE` role mailbox discovery, EXPUNGE/CLOSE
deletion handling, WORKSPACE, IDLE completion, clean `SELECT ""` and `STATUS ""` rejection, hosted
write serialization for multi-session clients, daemon-opened durable `mail/imap` listener startup,
redacted transcript inventory rows plus owner-verified Apple Mail and Thunderbird certification
evidence, and direct TLS IMAPS startup with a rustls client transcript. The hosted JMAP opener is source-backed in
`crates/loom-hosted-pim/src/jmap.rs` and
`crates/loom-cli/src/daemon_cmd.rs`; focused tests cover session discovery, blob upload/download,
Mailbox get/set, Identity get, Email query/get, Email set update/destroy/create, Email import,
Email changes/queryChanges, daemon-opened durable `mail/jmap` listener startup, direct TLS JMAP startup
with a rustls client transcript, and the executable RFC 8620/8621 hosted router transcript recorded
in the 0010a certification inventory. Mail mutable-state policy/version/delta records, operation merge, observed
replacement conflicts, redacted audit summaries, compaction, and retained-gap responses are
source-backed in `loom-core::mail`. Full IMAP coverage, reference-client validation, full JMAP, real
SMTP submission, and the filters-as-programs hosted/admin projection are tracked as follow-on hosted
protocol or lifecycle work, not the local-facet completion gate.
The conformance report includes hosted PIM protocol rows for the supported bounded profiles, degraded
IDLE-without-push behavior, target standards/reference-client work, setup-only hosted SMTP
compatibility, real SMTP submission gaps, and unsupported non-synchronizing literal gaps.

### Reusable PIM component extraction - source-backed

`loom-pim` now owns the mail local record contracts and projection helper for parsed message metadata:
`MailMessage`, `MailboxMeta`, canonical CBOR encode/decode, and RFC 5322 parsing through `mail-parser`.
`loom-core::mail` consumes and re-exports those contracts while retaining CAS body writes/reads, digest
verification, message and flag reserved-path storage, mailbox CRUD, message CRUD, search, helper diffs,
and `FacetKind::Mail` integration. Existing callers keep the `loom_core::mail::*` surface.
Component-level record/projection tests live in `loom-pim`; executable facade conformance stays in
`loom-conformance` because it proves engine storage, CAS body closure, versioning, clone reachability,
and workspace integration.
