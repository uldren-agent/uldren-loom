# 0008 - Wire Protocols

**Status:** Target, with current source-backed boundaries documented. **Version:** 0.1.0-target.

Loom's protocol contract projects the interface from 0003 over hosted transports and local adapters.
The enterprise target is one interface, many protocol shapes: REST, JSON-RPC 2.0, gRPC, MCP, FUSE,
and sync transport frames use the same method, type, error, identity, and authorization model.

## Current implementation

The Rust workspace now implements the first hosted substrate slices, but not the full hosted protocol
suite. Source backs a shared hosted request kernel in `crates/loom-hosted`, hosted REST, JSON-RPC,
and gRPC CAS adapter methods, REST/JSON-RPC hosted Archive and CAR adapter methods, REST/JSON-RPC
hosted tree and workspace-history listeners, served PIM auth
gates, daemon-opened files and VCS REST and JSON-RPC listeners, daemon-opened SQL REST, SQL JSON-RPC, and unary SQL gRPC listeners, daemon-opened
KV/document/queue/time-series REST and JSON-RPC listeners, daemon-opened graph/ledger/vector/search
REST and JSON-RPC listeners, hosted `substrate_changes` REST and JSON-RPC adapter methods, and
hosted chat REST and JSON-RPC adapter methods for the current
message/thread/reaction/emoji-registry/event/cursor/presence vertical with shared revision projection
for message create/edit routes, daemon-opened hosted chat REST and JSON-RPC listeners for the current
chat adapter surface, protocol conformance for served Chat and Drive typed write routes that project
shared revision rows, CAS REST/JSON-RPC/gRPC put, get, missing get, has, list, and delete, and
daemon-opened CAS REST, CAS JSON-RPC, CAS gRPC,
admin REST, and
admin JSON-RPC listeners from durable `loom serve` configuration. The workspace does not yet implement
complete REST, JSON-RPC, or gRPC listener coverage, OpenAPI documents, complete protobuf schemas,
JSON-RPC manifests, or full hosted protocol conformance. It
also implements a local MCP host and local filesystem projections; those are source-backed local
adapters, not proof that the hosted protocol suite is complete.

The source-backed local surfaces today are:

- `idl/loom.idl`, a manually maintained source-backed interface definition for the current C ABI and
  direct binding surface. It is not yet the generator source for the C ABI, bindings, or wire schemas.
- `include/loom.h` and `crates/loom-ffi`, the cbindgen-generated C ABI over current Rust surfaces.
- Language bindings that call Rust or the C ABI directly rather than generated hosted protocols.
- Loom Canonical CBOR result payloads used by ABI and binding data paths.
- Source-backed local facade projections for workspace lifecycle, key-source operations, CAS, queue,
  queue consumer offsets, SQL sessions and batches, diagnostics/result views, direct table readers,
  and workspace history operations where present in `idl/loom.idl`, `include/loom.h`, and bindings.
- `loom-core::sync::bundle_export` and `loom-core::sync::bundle_import`, which implement offline
  Loom-to-Loom bundles. They are not live network negotiation, resumable sync, or a hosted protocol.
- `loom_core::identity`, `loom_core::acl`, and `crates/loom-hosted`, which now provide core
  principal/session state, ACL evaluation, promoted local engine PEP hooks, and the first hosted
  passphrase-authenticated request kernel plus app-specific API-key authentication for compatibility
  profiles. The local MCP host projects launch-time
  passphrase-authenticated principal context through `LocalOpenAuth` for daemon-backed per-request
  stdio and Streamable HTTP serving. The hosted kernel projects the same identity/PEP path into tested
  REST, JSON-RPC, and gRPC-shaped adapters. Hosted HTTP auth accepts passphrase headers, one
  API-key credential header from `x-loom-api-key`, `x-api-key`, `api-key`, `x-pinecone-api-key`, or
  `Authorization: Bearer <token>`, or one already-verified external assertion carried as
  `x-loom-external-kind`, `x-loom-external-issuer`, `x-loom-external-subject`, and optional
  `x-loom-external-material-digest`. Hosted gRPC accepts the same passphrase, API-key, and verified
  external assertion metadata. Tokens and verified assertions resolve through the 0026 identity store
  before PEP. The 0026 identity store also source-backs generic external-challenge lifecycle state for
  proof verifiers. Hosted HTTP and gRPC reject direct external proof material through a fail-closed
  unsupported path until a real provider verifier is linked. Direct OAuth/OIDC/SAML token validation,
  certificate proof verification, passkey/WebAuthn proof verification, full listener coverage, and
  cross-protocol auth conformance remain target work.
- `crates/loom-mcp`, which implements the local MCP host over stdio and Streamable HTTP with curated
  tools, resources, prompts, output schemas, elided workspace/collection scoping, completion,
  progress/cancel mechanics, daemon attach, launch-time local passphrase principal authentication, and
  management-surface exclusion. It is not a substitute for the REST/JSON-RPC/gRPC hosted protocol
  adapters.
- `loom-vfs` plus the FUSE and NFS projection commands, which expose local filesystem views over
  source-backed workspace paths. They are not hosted wire protocols.
- Local CLI commands that exercise ABI/core behavior. They are not protocol adapters.

Everything after this section is the target protocol contract unless it explicitly cites current
source behavior.

## 1. Principles

- **One contract, many shapes.** REST is resource-oriented, JSON-RPC is method-oriented, gRPC is
  binary and streaming, MCP exposes tool-shaped operations, and FUSE projects selected workspace
  trees as a filesystem. They must share method semantics, stable errors, identity rules, and
  authorization behavior.
- **Generation is the target.** IDL-driven generation becomes required only after 0003 is reconciled
  with the C ABI, bindings, and promoted facades. Until then, generated artifacts are not claimed.
- **UUIDs are canonical resource identity.** A hosted listener selects one `.loom` store. REST paths use
  workspace UUIDs inside that store. Names, filenames, aliases, and deployment labels are lookup metadata,
  not canonical identity.
- **Content addressing is transport-friendly.** Immutable objects are cacheable by digest. Transport
  validators such as HTTP ETags validate transferred representations, not necessarily canonical Loom
  object bytes.
- **Streaming is first-class.** Listings, tree walks, logs, large file I/O, and sync use native
  streaming for each protocol.
- **Transport carries identity; the engine enforces authorization.** Served protocols attach a
  principal context to each operation. Engine policy enforcement, not adapter-specific filtering, is
  the security boundary. Served write promotion depends on 0026-0028.

## 2. IDL and Generated Artifacts

The target IDL is compiled to:

- `loom.proto`, for gRPC services and messages.
- `loom.openapi.yaml`, for REST routes and JSON schemas.
- `loom.jsonrpc.json`, for JSON-RPC method names, parameters, results, and streaming shapes.
- C ABI and per-language binding types, after 0003 and 0007 define a generated surface.

Current `idl/loom.idl` is useful as the source-backed local interface anchor, but source comments in
that file correctly describe generation and hosted protocol schemas as target work. CI must fail on
stale generated artifacts only after the generators and generated outputs exist. Per RD10 the IDL is
the contract for the programming libraries (C ABI and bindings), not the literal shape of the served
protocols: MCP, REST, and gRPC are each hand-written and curated for their audience and guarded by a
mandatory drift/coverage conformance layer (coverage of every facet operation plus golden-contract
schema stability). A true IDL-codegen track is an optional future consolidation, not a prerequisite.

The `db` facade from 0011 and data-facet facades from 0016-0024 project over hosted protocols only
after each facade is promoted through IDL, ABI, bindings, wire schemas, authorization, and
conformance. CAS and queue have source-backed local IDL/C ABI/binding surfaces today. Hosted CAS has
source-backed REST, JSON-RPC, and unary gRPC adapter methods plus daemon-opened CAS REST, CAS
JSON-RPC, and CAS gRPC listeners. Protocol conformance proves CAS REST, JSON-RPC, and gRPC put,
get, missing get, has, list, and delete. Hosted SQL has source-backed REST, JSON-RPC, and unary gRPC
query and exec listeners. Hosted Queue gRPC protocol conformance proves append, get, range, and len.
Hosted Time-series gRPC protocol conformance proves put, get, latest, and server-streaming range.
Hosted KV, document, queue, and time-series have source-backed listener-bound REST
and JSON-RPC local-facade adapters plus daemon-opened listeners for their core methods. Hosted graph,
ledger, vector, and search have source-backed listener-bound REST and JSON-RPC native subsets plus
daemon-opened listeners for their core methods. Hosted calendar has a bounded source-backed
`calendar/caldav` listener for WebDAV discovery, collection/resource read/write/delete, conditional
writes, `calendar-multiget`, `calendar-query`, commit-backed `sync-collection`, and direct TLS over
`.ics`.
Hosted contacts has a bounded source-backed `contacts/carddav` listener for WebDAV discovery,
address-book/resource read/write/delete, conditional writes, address-book properties, resource
properties, `addressbook-multiget`, `addressbook-query`, and commit-backed `sync-collection` with
tombstones, vCard 3.0 input/output, bounded vCard 4.0 output, and direct TLS over `.vcf`.
Hosted mail has a source-backed `mail/imap` listener subset for LOGIN, AUTHENTICATE PLAIN, CAPABILITY,
LIST, CREATE, DELETE, SELECT/EXAMINE, STATUS, FETCH/UID FETCH, STORE/UID STORE, SEARCH/UID SEARCH,
COPY/UID COPY, MOVE/UID MOVE, APPEND, EXPUNGE, CLOSE, UNSELECT, ENABLE, WORKSPACE, IDLE, NOOP, LOGOUT,
RFC 6154 role-mailbox discovery, Apple Notes compatibility, direct rustls IMAPS, and mutable flag-state
retention/compaction evidence over the hosted kernel. Hosted mail also has a bounded source-backed
`mail/jmap` listener for JMAP session discovery, `Mailbox/*`, `Thread/*`, `Email/*`, `Identity/*`,
upload/download URLs, deterministic state tokens, direct TLS, and explicit unsupported push over the
hosted kernel. Setup-only `mail/smtp` compatibility is source-backed for authenticated STARTTLS account
setup without relay, delivery, or mail-facet mutation. Dynamic runtime listener updates, full lifecycle conformance,
richer canonical REST resource shapes, hosted queue consumer-offset projection, additional gRPC
services, full per-facet method coverage, full CalDAV/CardDAV, full IMAP/JMAP conformance, durable
certification administration, Redis and Memcached compatibility protocols, and other compatibility
protocols remain target work. The `exec` facade from
0015 has source-backed REST, JSON-RPC, and gRPC served listeners that preserve
`loom.exec.request.v1` and `loom.exec.result.v1` bytes rather than defining a second wire envelope.

Hosted write promotion has a fixed preflight gate: identity and ACL must be publicly projected, the
engine PEP must cover the promoted facade, and protocol conformance must prove the transport attaches
the resolved principal context rather than reimplementing policy in the adapter. Current source has
the local identity/ACL substrate, selected PEP hooks, and served `exec` projection. Other served write
surfaces promote only when their owning facade and protocol conformance prove the same principal
attachment and PEP path.

The FUSE shape in this spec is the *hosted/network* filesystem projection (served to remote principals
with the authorization rules in section 9), which remains target work. It is distinct from the *local*
filesystem projection - mounting a workspace working tree as a folder on the same machine over FUSE or
NFSv3 - which is specified in 0003c and is source-backed today (`loom-vfs` portable layer with the
`loom-vfs-fuse` / `loom-vfs-nfs` backends and the CLI `loom mount fuse` / `loom mount nfs`
subcommands). A hosted FUSE endpoint, when built, reuses the same `loom-vfs` projection semantics over
a network transport:
the shared operation policy matrix, hidden implementation-private `.loom` storage, declared facet
projection handlers, and canonical `ProjectionMetadata` envelope.

## 3. REST Projection

### 3.1 Resource model

A hosted listener serves one `.loom` store at a time. The listener selects the Loom store; canonical
resource paths select a workspace with `{workspace_id}`, the workspace UUID assigned at creation. A
service may expose alias lookup routes, but canonical resources use workspace UUIDs.

| Resource | Path | Notes |
| --- | --- | --- |
| Store root | `/v1` | The served `.loom` store selected by the listener |
| Workspace | `/v1/workspaces/{workspace_id}` | Named bucket with one canonical root |
| File or directory | `/v1/workspaces/{workspace_id}/tree/{path}` | Path-addressed workspace tree |
| Facet path | `/v1/workspaces/{workspace_id}/tree/.loom/facets/{facet}/{path}` | Target file-style projection for facets that explicitly opt in |
| Object | `/v1/objects/{digest}` | Loom-scoped content address |
| Ref | `/v1/workspaces/{workspace_id}/refs/{name}` | Mutable workspace-history pointer |
| Commit | `/v1/workspaces/{workspace_id}/commits/{digest}` | Convenience over objects |
| Diff | `/v1/workspaces/{workspace_id}/diff?from=...&to=...` | Computed comparison |
| Sync | `/v1/sync/*` | Transport endpoints |

Unless marked Loom-scoped, routes below are relative to
`/v1/workspaces/{workspace_id}`.

Direct core APIs may read the reserved `/.loom/` and `/.loom/facets/` directories and may not write them
through ordinary user file operations. Hosted filesystem-style projections use the 0003c/0014a virtual
projection rule instead: implementation-private `.loom` storage is hidden, and only completed facets
that explicitly opt in publish file-style projection paths. Each opted-in facet defines its own readable
and writable paths, metadata behavior, and validation model.

### 3.2 Filesystem to HTTP verbs

| 0003 method | HTTP |
| --- | --- |
| `fs_read_file` | `GET /tree/{path}` with `Accept: application/octet-stream` |
| `fs.stat` | `HEAD /tree/{path}` or `GET /tree/{path}?stat=1` |
| `fs.list_directory` | `GET /tree/{path}?list=1` as paged JSON or NDJSON |
| `fs.create_file` | `POST /tree/{path}` with `If-None-Match: *` |
| `fs_write_file` | `PUT /tree/{path}` for full replacement |
| `fs_append_file` | `PATCH /tree/{path}` with append semantics |
| `fs.delete_file` and `fs.remove_directory` | `DELETE /tree/{path}` with `?recursive=1` for directories |
| `fs.create_directory` | `PUT /tree/{path}/` with `?recursive=1` when needed |
| `fs.move` and `fs.copy` | `POST /tree/{path}:move` or `POST /tree/{path}:copy` |
| byte-range read | `GET /tree/{path}` with `Range`, returning `206 Partial Content` |

### 3.3 Workspace history to HTTP

The public method namespace remains `vcs.*` because 0003 uses that facade name for workspace history.
It does not imply a dedicated `vcs` facet or typed workspace.

| 0003 method | HTTP |
| --- | --- |
| `vcs_commit` | `POST /commits` returning `201` and `Location: /commits/{digest}` |
| `vcs_log` | `GET /commits?ref=...&limit=...` as paged JSON or NDJSON |
| `vcs.branch_create` | `PUT /refs/branch/{name}` with `{ at }` |
| `vcs_checkout` | `POST :checkout` with `{ target, ... }` |
| `vcs_merge` | `POST :merge` with `{ source, strategy }`, returning `409` for conflicts |
| `vcs_rebase`, `vcs_squash`, `vcs_cherry_pick`, `vcs_revert` | `POST :{op}` |
| `vcs.update_ref` | `PUT /refs/{name}` with `If-Match: "{expected_old_digest}"` |
| `vcs.read_object` | `GET /v1/objects/{digest}` |

### 3.4 Caching, conditional requests, and idempotency

- `GET /objects/{digest}` returns `Loom-Object-Digest: "{digest}"`. The Loom object digest is the
  plaintext canonical object identity.
- HTTP `ETag` validates the selected HTTP representation. A canonical identity response may use a
  strong ETag derived from the Loom object digest. Compressed, encrypted, or transformed responses use
  representation-specific strong ETags and include `Vary: Accept-Encoding` when content negotiation
  selects a representation.
- Canonical object bytes and other stable immutable representations may use
  `Cache-Control: public, max-age=31536000, immutable`.
- `GET /tree/{path}` returns an ETag for the current file content. `If-None-Match` returns `304` when
  the content is unchanged.
- Ref updates use `If-Match: "{old}"`, which maps to compare-and-swap. A mismatch returns HTTP `412`
  and stable code `CAS_MISMATCH`.
- Mutating POST requests accept `Idempotency-Key`. Keys are scoped per `(store_id, principal_id)`,
  retained for a bounded TTL, and bound to a request fingerprint. A matching replay returns the
  original result, a mismatching fingerprint returns `422` with `INVALID_ARGUMENT`, and an in-flight
  duplicate returns `409` with `CONFLICT`.
- Pagination uses `?cursor=` plus `Link: rel="next"`. Large listings may stream
  `application/x-ndjson`.

### 3.4a Conditional-mutation projection

REST, JSON-RPC, gRPC, WebDAV, CalDAV, CardDAV, Redis, Memcached, and S3 projections consume the
native comparison contract from 0003 section 9.1. Headers, request fields, revision strings, ETags,
CAS values, and product transaction tokens are projection-owned syntax. None establishes a universal
Loom token format or changes the owning native primitive's anchor, condition set, atomic read point,
or merge boundary.

For every conditional write, the hosted kernel resolves and authorizes the principal before calling the
owning native primitive. It preserves the primitive's redacted result for audit under 0009, then maps
the stable 0003 section 8 outcome to the protocol response. HTTP `412`, RESP conditional replies,
JSON-RPC errors, and gRPC statuses are mappings, not alternate native outcomes. A failed comparison
must have no partial write and must not reveal the protected current value, raw opaque token, or private
topology.

S3-compatible object-key and version state may consume this projection for conditional object
operations. Immutable CAS blob upload remains digest-addressed and idempotent: it is not the
conditional-mutation primitive, even when an S3-compatible facade uses object ETags or version ids.

### 3.5 Hosted PIM protocol projections

Hosted calendar, contacts, and mail protocols are owned by this hosted-protocol spec, not by the local
facet completion gates in 0037, 0038, or 0039. The local PIM facets and their MCP projection may be
source-backed before the hosted protocol servers exist.

Required hosted protocol projections:

- **CalDAV** for the calendar facet: hosted RFC 4791/WebDAV projection over the structured calendar
  records from 0037. The first source-backed profile supports `.well-known/caldav`, OPTIONS,
  PROPFIND, MKCALENDAR, GET, PUT, DELETE, conditional writes, `calendar-multiget`, `calendar-query`,
  and commit-backed `sync-collection` with tombstones over `.ics` resources through daemon-opened
  `calendar/caldav` listener records. Direct TLS is source-backed through hosted certificate-bundle
  listener records and rustls startup. Apple Calendar, Apple Reminders, Thunderbird, and DAVx5 are
  owner-verified against the local certification harness. Scheduling, free-busy, delegated sharing,
  durable certification administration, and full CalDAV conformance remain target work.
- **CardDAV** for the contacts facet: hosted RFC 6352/WebDAV projection over the structured contact
  records from 0038. The first source-backed profile supports `.well-known/carddav`, OPTIONS,
  PROPFIND, MKCOL, GET, PUT, DELETE, conditional writes, address-book properties, resource properties,
  `addressbook-multiget`, `addressbook-query`, and commit-backed `sync-collection` with tombstones over
  `.vcf` resources through daemon-opened `contacts/carddav` listener records. The hosted projection
  advertises and serves `text/vcard` versions `3.0` and `4.0`, defaulting CardDAV `address-data` to
  vCard 3.0 unless vCard 4.0 is requested. CardDAV PUT accepts vCard 3.0 input and preserves grouped,
  parameterized RFC 2426 properties that do not have typed Loom fields. Direct TLS is source-backed
  through hosted certificate-bundle listener records and rustls startup. Apple Contacts, Thunderbird,
  and DAVx5 are owner-verified against the local certification harness. Full CardDAV ACL authoring,
  partial `address-data`, Apple-label display semantics, delegated sharing, durable certification
  administration, and broader ACL-aware serving remain target work.
- **IMAP** for the mail facet: hosted mailbox/message projection over the immutable message body,
  structured index, and flag subtrees from 0039. The current source-backed subset covers login,
  SASL PLAIN authentication with initial-response and continuation flows, mailbox list/select/status,
  mailbox create/delete, durable per-mailbox numeric UID mapping, stable `UIDVALIDITY`, stable
  `UIDNEXT`, durable `SUBSCRIBE`/`UNSUBSCRIBE`/`LSUB` metadata, message fetch, UID fetch, flag
  mutation, fetch-attribute validation, STORE `.SILENT`, bounded common search criteria, copy/move
  between mailboxes, APPEND with synchronizing literals, EXPUNGE, CLOSE, and direct rustls IMAPS serving
  from hosted TLS certificate/key policy. The adapter also supports WORKSPACE and IDLE completion. Full
  RFC 6154 role-mailbox discovery, Apple Notes compatibility, and mutable flag-state
  retention/compaction rows are source-backed. Apple Mail and Thunderbird are owner-verified against the
  local certification harness. Full RFC 9051 conformance, recursive SEARCH operators, date/comparator
  search extensions, non-synchronizing literal upload handling, full MIME/BODYSTRUCTURE fidelity,
  durable certification administration, and broader reference-client automation remain target work. The
  current adapter explicitly rejects non-synchronizing APPEND literals with tested protocol responses.
- **JMAP** for the mail facet: target JSON mail sync and web/mobile application surface. It remains
  useful after IMAP because it is a better Loom-native application API, but it is separate from the
  standards-based IMAP mailbox read and management target. The first source-backed profile supports
  `/.well-known/jmap`, `/jmap/session`, `/jmap`, `/jmap/api`, `Mailbox/get`, `Mailbox/set`,
  `Email/query`, `Email/get`, and `Email/set` update/destroy through daemon-opened `mail/jmap`
  listener records. Blob upload/download, `Email/import`, `Email/set` create from uploaded blobs,
  stable `Identity/get`, deterministic account/query/session state tokens, `Email/changes`, and
  `Email/queryChanges` are source-backed. Direct TLS is source-backed for the current JMAP routes
  through hosted certificate-bundle listener records and rustls startup. JMAP push is explicitly
  unsupported until backed by 0035 durable delivery semantics. Queue 7 JMAP closure is owner-accepted
  green without external client/tool evidence because no such tool is available in the certification
  environment; source-backed hosted RFC 8620/8621 transcripts are the evidence for this slice. Full JMAP
  conformance, native JMAP client/tool certification, JMAP contacts, and JMAP calendars remain target
  surfaces unless explicitly promoted; CardDAV and CalDAV remain the Queue 7 standards surfaces for
  contacts and calendars.
- **SMTP** is not part of the base mail facet. Loom exposes a setup-only hosted `mail/smtp`
  compatibility listener for the PIM certification harness because some reference clients require an
  outgoing server during account setup. It accepts the fixture credentials, requires `STARTTLS`, accepts
  submitted `DATA` only for setup and send-probe compatibility, and does not relay, deliver, or promote
  real mail submission into the base facet. The listener must not advertise optional SMTP extensions
  unless the behavior is source-backed and recorded in the hosted capability rows.

The conformance report records hosted PIM feature evidence as supported, degraded, target, or
unsupported rows. It is source-backed for the bounded IMAP, CalDAV, CardDAV, and JMAP profiles, but it
does not claim full standards conformance. It also records the source-backed PIM hook registration,
canonical envelope, event-emission, and execution-policy planning rows, plus the mail mutable-state
policy/version/delta and merge/audit/compaction/retained-gap rows. Queue 7 owner verification closes the
Apple/Thunderbird/DAVx5 CalDAV/CardDAV/IMAP evidence and owner-accepted JMAP executable-transcript
evidence for the bounded profile. Durable transcript storage, admin-visible certification status, hook
administration, and operator controls remain owned by 0065.

These projections are gated on the hosted stack rather than the local facet stack: TLS and deployment
identity, 0026 principal authentication, 0027/0028 policy enforcement, served write authority, stable
protocol error mapping, protocol conformance, and durable-delivery/change-feed semantics where the
protocol needs reconnect, sync, or push behavior.

The first hosted PIM completion pass is owner-only. Calendar, contacts, and mail projections bind the
served principal to the authenticated owner context and do not expose delegated calendars, address
books, shared mailboxes, free/busy lookup, or service-principal sharing. Mailbox sharing is out of
scope until explicitly requested and must not be advertised by the current hosted PIM capability rows.
Calendar and contacts sharing remain separate target-scope questions.

#### PIM hosted completion target

The full PIM hosted target requires a shared WebDAV substrate before CalDAV and CardDAV diverge. That
substrate owns request classification, percent decoding for inbound paths, percent encoding for response
`href` values, bounded XML parsing, multistatus generation, stable WebDAV error bodies, ETag
preconditions, `sync-token` parsing, invalid-token responses, hosted auth, PEP checks, request-size
limits, audit, and store-save behavior.

CalDAV and CardDAV provide only domain-specific collection metadata, resource parsing and serialization,
query predicates, and projected body properties. `sync-token` values are opaque to clients and include
collection scope plus a Loom commit digest. Unknown, expired, or compacted tokens fail with a stable
resync-required response rather than silently returning a full listing.

Direct TLS for CalDAV, CardDAV, and JMAP uses the shared hosted-listener and certificate policy. PIM
adapters must not grow protocol-specific TLS stacks. Capability rows must distinguish direct TLS support
per listener from bounded router support.

Hosted PIM certification evidence feeds 0010a reports and 0065 `admin.certification.profile` records.
The Queue 7 release gate is owner-verified Apple plus cross-platform client coverage for CalDAV,
CardDAV, and IMAP, plus owner-accepted JMAP RFC 8620/8621 executable hosted transcript evidence. Native
JMAP client or tool certification, durable transcript storage, and admin review remain 0065 target work.
Direct TLS is already source-backed for current JMAP routes.

#### Shared PIM RFC implementation gate

Queue 7 treats this shared gate as a prerequisite for CalDAV, CardDAV, IMAP, JMAP, and reference-client
certification. Facet-specific RFCs live in 0037, 0038, and 0039; this table owns transport, discovery,
HTTP, TLS, auth, URI, and WebDAV behavior that must remain DRY across hosted PIM adapters.

The grouped shared row is `pim/rfc-gate/shared-http-uri-webdav-basic-auth` in
`crates/loom-conformance`. Individual shared RFC rows are promoted only after their RFC requirements
are checked against source, tests, and capability evidence. RFC 3986 is source-backed by
`pim/rfc-gate/uri-rfc3986-percent-encoding`: RFC 3986 sections 2.1, 2.2, 2.4, 3.3, 6.2.2.1,
6.2.2.2, 6.2.2.3, and 7 require percent triplets, reserved slash delimiter handling, parse-before-
decode behavior, one-pass decoding, uppercase percent-encoding normalization, dot-segment discipline,
and protection against encoded delimiters. Loom implements that in `crates/loom-hosted-pim/src/dav.rs`
with `webdav_segments`, `webdav_segment`, `percent_decode_path`, `webdav_resource_path`,
`webdav_href_resource_path`, and `url_segment`. Tests cover encoded collection/resource names,
canonical response `href` encoding, encoded dot-segment rejection, encoded slash rejection, incomplete
percent-triplet rejection, invalid hex rejection, and invalid UTF-8 rejection. RFC 4918 is
source-backed by `pim/rfc-gate/webdav-rfc4918-base-methods`: RFC 4918 sections 8.2, 8.3, 8.6, 9.1,
10.2, 11.1, 12.1, 13, 14.16, 14.22, 17, and 18 require XML request/response discipline, URL handling,
ETag guidance, PROPFIND, Depth handling, 207 Multi-Status, propstat response shape, XML extensibility,
and DAV compliance discovery by OPTIONS. Loom implements that in `crates/loom-hosted-pim/src/dav.rs`
with `caldav_dispatch`, `carddav_dispatch`, `webdav_depth`, `webdav_propfind_request`,
`webdav_check_preconditions`, `webdav_write_error_response`, `webdav_multistatus`,
`caldav_propfind`, and `carddav_propfind`. Tests cover OPTIONS DAV headers, PROPFIND Depth `0` and
`1`, explicit Depth `infinity` rejection, XML multistatus bodies, `propstat` resource rows, resource
ETags, stale `If-Match`/`If-None-Match` failures, and bounded XML validation. The remaining grouped
shared row is backed by `crates/loom-hosted-pim/src/dav.rs` tests for well-known CalDAV/CardDAV
redirects, Basic-auth DAV listeners, sync-token properties, and CalendarServer `getctag`
compatibility properties. RFC 5397 is source-backed by
`pim/rfc-gate/webdav-rfc5397-current-user-principal`: RFC 5397 section 3 requires
`DAV:current-user-principal` to be computed per request and identify the currently authenticated
user's principal resource with a single `DAV:href`. Loom implements that in `hosted_dav_auth`,
`hosted_principal_name`, `caldav_home_response`, `caldav_principal_response`,
`carddav_home_response`, and `carddav_principal_response`. Tests cover authenticated principal
discovery on CalDAV and CardDAV roots and prove path-supplied principal names do not replace the
authenticated principal in `current-user-principal`, `principal-URL`, or `owner`.
RFC 5689 is explicitly unsupported by `pim/rfc-gate/webdav-rfc5689-extended-mkcol`: RFC 5689 sections
3, 3.1, 3.2, 3.3, and 4 define XML-bodied Extended MKCOL, `extended-mkcol` OPTIONS discovery, property
setting, valid resource-type preconditions, and MKCALENDAR replacement behavior. Loom does not
advertise `extended-mkcol`; CardDAV plain `MKCOL` accepts only an empty request body, and XML-bodied
Extended MKCOL requests are rejected before collection creation. CalDAV collection creation remains
the RFC 4791 `MKCALENDAR` path, and CardDAV address-book creation remains the plain RFC 4918 `MKCOL`
path.
RFC 5785 and its successor RFC 8615 are source-backed by
`pim/rfc-gate/well-known-rfc5785-caldav-carddav` and
`pim/rfc-gate/well-known-rfc8615-caldav-carddav`: RFC 5785 sections 1.1 and 3 define the
`/.well-known/` path prefix for HTTP(S) discovery, and RFC 8615 sections 3 and 4 update that rule for
current HTTP(S) well-known URI handling and security scoping. Loom implements that in
`caldav_well_known`, `carddav_well_known`, `caldav_router_with_policy_and_cache`,
`carddav_router_with_policy_and_cache`, and `dav_router_with_policy`. Tests cover
`/.well-known/caldav`, `/.well-known/caldav/`, `/.well-known/carddav`, and
`/.well-known/carddav/`, all redirecting to the trailing-slash service roots. Loom advertises only the
CalDAV and CardDAV well-known routes in this queue.
RFC 6578 is source-backed by `pim/rfc-gate/webdav-rfc6578-sync-collection`: RFC 6578 sections 3.1,
3.2, 3.3, 3.4, 3.5.1, 3.5.2, 4, and 6 define collection synchronization by tokens, supported
`sync-collection` reports, Depth behavior, initial full synchronization, changed and removed member
rows, `DAV:sync-token`, and XML element shape. Loom implements that in `caldav_sync_collection`,
`carddav_sync_collection`, `caldav_sync_resources`, `carddav_sync_resources`,
`caldav_sync_present_row`, `carddav_sync_present_row`, `caldav_collection_sync_token`,
`carddav_book_sync_token`, `caldav_sync_token_digest`, and `carddav_sync_token_digest`. Tests cover
initial sync, matching-token no-op sync, changed-resource rows, tombstone rows, collection
`sync-token` properties, and malformed or stale token errors for both CalDAV and CardDAV.
RFC 6764 is degraded by `pim/rfc-gate/service-discovery-rfc6764`: RFC 6764 sections 3, 4, 5, 6, 7,
8, 9.1, and 9.2 define CalDAV/CardDAV SRV labels, TXT `path` records, well-known redirects,
bootstrap PROPFIND to `DAV:current-user-principal`, principal discovery, home-set discovery,
certificate verification posture, and IANA service/well-known registrations. Loom source-backs the
HTTP discovery profile only: well-known redirects, authenticated `current-user-principal`,
`principal-URL`, CalDAV `calendar-home-set`, CardDAV `addressbook-home-set`, and owner-only service
roots. DNS SRV, DNS TXT `path`, DNS-SD lookup behavior, and SRV-started certificate-name verification
remain target rows under `pim/rfc-gate/shared-dns-srv-dns-sd-discovery` and
`pim/rfc-gate/shared-http-over-tls-service-identity`.
RFC 7617 is degraded by `pim/rfc-gate/http-basic-rfc7617`: RFC 7617 sections 2, 2.1, 2.2, 3, and 4
define the Basic scheme name, required realm challenge, optional UTF-8 charset advisory, Base64
`user-id:password` credentials, control-character rejection, credential reuse scope, and TLS security
warning. Loom source-backs the DAV Basic parser, UTF-8 credentials, malformed Base64 rejection,
missing-colon rejection, empty credential rejection, control-character rejection, case-insensitive
scheme matching, exact `WWW-Authenticate: Basic realm="Uldren Loom DAV", charset="UTF-8"` challenge,
username-to-principal resolution by ID or display name, the hosted PIM compatibility profile for
one-pass percent-decoded email usernames (`example%40domain`), same-domain local-part usernames
qualified by the trusted request host (`example` plus `Host: domain`), no double decoding, no
credential logging, and a bounded positive-auth cache whose key stores only a password digest.
Request-level enforcement that Basic auth arrives only over TLS remains degraded until
listener/request metadata can prove or reject plaintext Basic at the DAV router boundary;
deployment-level TLS posture remains recorded under
`pim/rfc-gate/shared-http-over-tls-service-identity`.
RFC 8996 is source-backed by `pim/rfc-gate/tls-rfc8996-modern-versions`: RFC 8996 sections 4 and 5
say TLS 1.0 and TLS 1.1 MUST NOT be used. Hosted PIM direct TLS uses the shared `HostedTlsConfig`,
which builds `rustls::ServerConfig::builder()` after installing the aws-lc provider. The pinned
`rustls` 0.23.41 server builder uses `DEFAULT_VERSIONS`, and that set contains only TLS 1.3 and TLS
1.2. The hosted drift test asserts the default version set rejects TLS 1.0 and TLS 1.1. Certificate
name, trust-anchor, and deployment service-identity proof remain under
`pim/rfc-gate/shared-http-over-tls-service-identity`; this row covers only the obsolete protocol
version ban.
RFC 9110 is source-backed by `pim/rfc-gate/http-semantics-rfc9110-bounded-profile` for the bounded
hosted PIM origin-server profile: DAV method dispatch, `GET`, `HEAD`, `PUT`, `DELETE`, `OPTIONS`,
405 `Allow`, 401 `WWW-Authenticate`, 201/204 write and delete success mapping, 412 ETag precondition
failure mapping, strong ETag parsing for `If-Match` and `If-None-Match`, and representation
`Content-Type` for `.ics`, `.vcf`, XML multistatus, and JSON error bodies. General-purpose HTTP
caching, range requests, content negotiation, proxy semantics, trailers, redirects beyond well-known
service discovery, and broad field extension behavior are outside the bounded PIM adapter profile
unless a later owning task adds explicit source-backed rows.
RFC 9112 is source-backed by `pim/rfc-gate/http1-rfc9112-shared-stack`: hosted HTTP/1.1 message
syntax, parsing, body framing, connection persistence, chunk handling, and malformed-message behavior
are delegated to pinned `hyper` 1.10.1 through `hyper-util` 0.1.20's auto server builder, with the
`http1` and `server` features enabled. Loom-owned HTTP/1.1 posture is the listener wiring, request
body limit handoff to `axum::body::to_bytes`, header-read timeout, session timeout, graceful shutdown,
and raw TCP transcript tests that verify HTTP/1.1 status lines, response bodies, and slow or expired
connection closure. PIM adapters must not grow protocol-specific HTTP/1.1 parsers.
`pim/rfc-gate/shared-http-over-tls-service-identity` remains target: RFC 9110 sections 4.3.3 and
4.3.4 require HTTPS authority to rest on a trusted certificate chain and service identity matching the
URI origin. Loom source-backs hosted certificate-bundle storage, direct rustls listener startup, local
CA generation for `uldrentest.com`, and local CA installation instructions for manual certification.
It does not yet source-back durable listener-level certificate-name policy, trust-anchor inventory,
client-side verification transcripts, or admin-visible service-identity pass/fail evidence. Those
remain 0065-owned deployment posture and certification records.
`pim/rfc-gate/shared-dns-srv-dns-sd-discovery` remains target: hosted PIM discovery is currently
HTTP-only. Loom source-backs well-known redirects, principal discovery, and home-set discovery, but it
does not implement DNS SRV lookup, DNS TXT `path` lookup, DNS-SD lookup, or SRV-started certificate
name verification. Those behaviors remain deployment-specific target work until resolver code,
configuration records, negative tests, and reference-client transcripts exist.

| RFC or standard | Gate for hosted PIM | Acceptance rule |
| --- | --- | --- |
| RFC 3986 | URI syntax and percent encoding | Inbound resource paths are decoded once, invalid encodings fail deterministically, encoded slash traversal is rejected, and response `href` values are encoded canonically. |
| RFC 4918 | WebDAV base methods and multistatus | OPTIONS, PROPFIND, REPORT response shape, Depth `0`/`1`, explicit Depth `infinity` rejection, ETags, precondition status codes, and XML multistatus output are shared by CalDAV and CardDAV. |
| RFC 5397 | `current-user-principal` discovery | DAV principal discovery is source-backed for owner-only hosted PIM and never fabricates a principal outside the authenticated identity. |
| RFC 5689 | Extended MKCOL | Extended MKCOL is unadvertised in the owner-only profile. Collection creation is source-backed by `MKCALENDAR` for CalDAV and plain `MKCOL` for CardDAV only. |
| RFC 5785 | Well-known URI registry | `/.well-known/caldav` and `/.well-known/carddav` are the only well-known PIM routes advertised by this queue, and both redirect to trailing-slash service roots. |
| RFC 8615 | Well-known URI successor | RFC 5785 is obsolete. The same CalDAV/CardDAV well-known routes are source-backed under the current well-known URI RFC, with no additional PIM routes advertised. |
| RFC 6578 | WebDAV sync collection | `sync-collection` support is advertised only when commit-backed sync tokens, no-op sync, tombstones, and invalid-token errors are source-backed. |
| RFC 6764 | CalDAV/CardDAV service discovery | Degraded. Well-known redirects, principal discovery, and home-set discovery are source-backed. DNS SRV, DNS TXT `path`, DNS-SD discovery, and SRV-started certificate-name verification remain target rows unless explicitly configured and tested. |
| RFC 7617 | HTTP Basic authentication | Degraded. Basic syntax, challenge, UTF-8 credential parse, malformed credential rejection, username-to-principal resolution, no credential logging, and bounded digest-keyed positive cache are source-backed. Request-level plaintext Basic rejection remains target with the shared TLS service-identity row. |
| RFC 8996 | TLS 1.0 and TLS 1.1 deprecation | Source-backed for direct hosted TLS protocol versions through rustls 0.23.41 defaults: TLS 1.3 and TLS 1.2 only. Service identity and trust-anchor proof remain target under the shared HTTPS row. |
| RFC 9110 | HTTP semantics | Source-backed for the bounded hosted PIM origin-server profile: DAV method dispatch, GET/HEAD, PUT/DELETE, OPTIONS, 405 Allow, 401 challenge, write/delete status mapping, ETag preconditions, and representation content types. General HTTP caching, range, proxy, trailer, and content-negotiation behavior are outside this adapter profile unless added by later rows. |
| RFC 9112 | HTTP/1.1 message syntax | Source-backed through the shared hyper/hyper-util server stack with HTTP/1 enabled, hosted header/session timeouts, raw HTTP/1.1 transcript tests, and no per-facet HTTP parser. |
| HTTP over TLS and service identity | HTTPS certificate verification | Target. Certificate bundles, direct TLS startup, local CA generation, and local trust instructions are source-backed, but durable certificate-name policy, trust-anchor inventory, client-side verification transcripts, and admin-visible pass/fail evidence remain 0065-owned target work. |
| DNS SRV and DNS-SD | Optional service discovery | SRV and DNS-SD remain target or deployment-specific rows unless Queue 7 adds source-backed lookup and transcript evidence. |

### 3.6 Hosted SQL protocol projections

Hosted SQL protocol projection is target work owned by this hosted-protocol spec. The local SQL facade
and binding projection are owned by 0011 and 0011a; PostgreSQL-wire and MySQL-wire foreign adapters are
owned by 0011b.

Required hosted SQL projections:

- (P1) REST SQL methods for direct table readers, read-only queries, mutation-capable exec, commits,
  and schema-aware table diffs after identity and PEP coverage are stable.
- (P1) JSON-RPC SQL methods with the same stable `Code` error mapping as the local C ABI and bindings.
- (P1) gRPC SQL methods for service-to-service integrations and streamed row results.
- (P1) Full MCP SQL session and batch tools only after session lifetime, cancellation, reconnect, and
  cleanup behavior are defined.

Hosted SQL writes are gated on TLS and deployment identity, 0026 principal authentication, 0027/0028
policy enforcement, session/daemon lifecycle, durable cleanup for abandoned work, protocol conformance,
and stable error mapping. Multi-statement hosted transactions must be rejected unless the implementation
can provide real atomicity and cleanup for disconnect, timeout, and crash cases.

## 4. JSON-RPC 2.0 and gRPC Projections

### 4.1 JSON-RPC 2.0

Method names mirror the promoted IDL methods one-to-one, such as `fs.readFile`, `vcs_commit`, and
`sync.push`. The `vcs.*` methods are workspace-history calls, not a `vcs` facet projection. Params
are IDL structs as JSON and results are IDL result types.

Batch requests are supported. Streaming uses JSON-RPC notifications over a persistent WebSocket, with
server-pushed `*.next` and `*.end` events keyed by subscription id, or chunked NDJSON where WebSocket
is not available. JSON-RPC is the recommended projection for scripting, editor integrations, and
tooling integrations.

### 4.2 gRPC

`loom.proto` defines a `Loom` service with unary RPCs for point operations, server-streaming RPCs for
list, walk, log, and file reads, client-streaming RPCs for large writes, and bidirectional RPCs for
sync. gRPC is the preferred service-to-service and high-throughput sync projection because it has
binary framing, multiplexing, backpressure, and a mature streaming model.

## 5. Authentication and Authorization

Identity is defined in 0026 and authorization in 0027/0028. A transport authenticates the connection
or request, resolves it to a `PrincipalId`, and attaches that principal context to each engine call.
The transport does not decide whether an operation is allowed; it delegates to the engine policy
enforcement point before state is touched.

- **REST and JSON-RPC over HTTP:** `Authorization: Bearer <token>` for a Loom session or OIDC-style
  access token, or mTLS client certificates for service identities.
- **gRPC:** Per-RPC credentials in metadata and/or channel mTLS.
- **WebSocket:** Credentials are presented in the upgrade request and re-established on reconnect.
- **Owner mode:** A non-authenticated Loom has no principal store. Hosted owner-mode writes must be
  deployment-confined. Public multi-user services enable authenticated mode before exposing writes.
- **Out-of-scope masking:** Unauthorized access outside the caller's granted scope is masked as
  `NOT_FOUND`, so existence cannot be probed. `PERMISSION_DENIED` is used when the caller can see the
  resource but the specific action is denied.

Authentication failure, including missing or invalid credentials where authentication is required,
returns `AUTHENTICATION_FAILED` and HTTP `401`. Authorization failure returns `PERMISSION_DENIED` or
is masked as `NOT_FOUND` as above.

## 6. Error Mapping

The stable `loom_core::error::Code` registry is source-backed. Protocol mappings below are the target
wire projection. Every response body includes the stable machine code so clients branch on `code`,
not on HTTP status, JSON-RPC number, gRPC status, or human text.

| Stable `Code` | HTTP | JSON-RPC error code | gRPC status |
| --- | --- | --- | --- |
| `NOT_FOUND` | 404 | -32004 | `NOT_FOUND` |
| `ALREADY_EXISTS` | 409 | -32009 | `ALREADY_EXISTS` |
| `CORRUPT_OBJECT` | 422 | -32030 | `DATA_LOSS` |
| `INTEGRITY_FAILURE` | 422 | -32031 | `DATA_LOSS` |
| `UNSUPPORTED` | 501 | -32601 | `UNIMPLEMENTED` |
| `INVALID_ARGUMENT` | 400 or 422 | -32602 | `INVALID_ARGUMENT` |
| `IO` | 500 | -32603 | `INTERNAL` |
| `INTERNAL` | 500 | -32603 | `INTERNAL` |
| `CROSS_WORKSPACE` | 422 | -32602 | `INVALID_ARGUMENT` |
| `CAS_MISMATCH` | 412 | -32017 | `ABORTED` |
| `NOT_FAST_FORWARD` | 409 | -32018 | `ABORTED` |
| `DIMENSION_MISMATCH` | 422 | -32602 | `INVALID_ARGUMENT` |
| `PERMISSION_DENIED` | 403 | -32001 | `PERMISSION_DENIED` |
| out-of-scope `PERMISSION_DENIED` masked as `NOT_FOUND` | 404 | -32004 | `NOT_FOUND` |
| `AUTHENTICATION_FAILED` | 401 | -32001 | `UNAUTHENTICATED` |
| `IDENTITY_NO_ROOT_CREDENTIAL` | 409 | -32024 | `FAILED_PRECONDITION` |
| `TRIGGER_NOT_FOUND` | 404 | -32025 | `NOT_FOUND` |
| `TRIGGER_DENIED` | 403 | -32026 | `PERMISSION_DENIED` |
| `CURSOR_INVALID` | 410 | -32027 | `FAILED_PRECONDITION` |
| `E2E_LOCKED` | 423 | -32028 | `FAILED_PRECONDITION` |
| `E2E_KEY_INVALID` | 403 | -32029 | `PERMISSION_DENIED` |
| `CONFLICT` | 409 | -32040 | `ABORTED` |

Standard JSON-RPC reserved codes, including `-32700`, `-32600`, `-32601`, and `-32602`, are honored.
Loom-specific implementation-defined codes use `-32000..-32099`. `INVALID_ARGUMENT` uses HTTP `400`
for malformed syntax and HTTP `422` for well-formed but semantically invalid requests.

The HTTP and JSON response body carries `{ code, message, path?, details? }`. gRPC errors use the
canonical status in the table and include structured details in `google.rpc.Status.details`: an
`Any`-packed `LoomError` and `google.rpc.ErrorInfo` whose `reason` is the stable Loom code.
`Status.message` is for developers and must not be parsed for program behavior.

```proto
message LoomError {
  LoomCode code = 1;
  string message = 2;
  optional string path = 3;
  map<string, string> details = 4;
}
```

## 7. Served Adapters and Write Authority

All served write surfaces share the same authorization rule: resolve a principal first, then call the
engine with that principal context. Adapter-level hiding of write operations is an ergonomic filter,
not the security boundary.

| Surface | Write authority rule |
| --- | --- |
| REST, JSON-RPC, and gRPC | Mutating routes require an authenticated principal in authenticated mode. Owner-mode writes are deployment-confined and must not be exposed as public multi-user service writes. |
| MCP stdio | A local stdio session runs as the opening owner in owner mode or as a resolved principal in authenticated mode. Write tools may be omitted from `tools/list` when unauthorized, but each call still reaches the engine policy check. |
| MCP HTTP/SSE | Uses the same transport authentication as REST or WebSocket. Remote MCP never grants ambient writes merely because a tool exists. |
| FUSE read-only commit mount | Always read-only. Mutating filesystem operations return read-only filesystem errors before reaching the engine. |
| FUSE live-tip mount | Mutating filesystem operations run as the mounting principal, or as owner only in local owner mode. Authorization failures map to POSIX permission errors. |
| Sync push or bundle import | Creating workspaces, importing objects, or advancing refs requires write and ref-advance authority for the affected workspace and ref. Pull and fetch require read authority. |

This table also applies to SQL-wire, S3, GraphQL, and other adapter ideas after they are promoted:
syntax changes, but principal resolution and authorization do not.

Served branch and tag mutation must use the public ref surface only. Public ref names reject `HEAD`,
`refs/...`, slash-separated names, dot-prefixed names, trailing dots, `..`, backslashes, and control
characters before any ref is created or advanced. Hosted protocols must not expose arbitrary raw-ref
mutation until a separate raw-ref policy exists. Fast-forward destination updates require `Advance`;
non-fast-forward rewrites require an explicit administrative path and must not be smuggled through
ordinary write routes. Tags are governance-affecting refs: create, delete, and rename require `Admin`.
When protected-ref policy is promoted, every served write that would create, advance, merge into,
delete, or rename a protected branch or tag must evaluate the grant first and then the protected-ref
policy before publishing the ref.

Current Streamable HTTP MCP is intentionally read-only: `http_service` forces write tools out of the
registered tool surface. Stdio MCP can expose write tools, but every call still runs through the engine
PEP. Remote HTTP writes remain target work until transport identity, hosted authorization, protected
ref policy, and protocol conformance are promoted together.

### 7.1 Durable served-listener configuration

Served listener configuration is Loom state, not a sidecar file. The command line selects the store and
the process action; the store records which served protocols should reopen when the daemon starts. A
restart of the Loom daemon therefore reloads the configured listeners from the `.loom` file, validates
them, and opens only those listeners the current executable supports.

Current source backs the first durable listener-intent subset: `FileStore` persists served listener
records under the durable-local control root, and `loom serve configure <store> <surface> ...`
validates the store, surface, selector shape, transport, bind address, TLS/auth/exposure policy,
network access policy reference, request/idle/session limits, and audit mode, writes the record, and
appends an audit event.
`loom serve list|enable|disable|remove <store> ...` are also source-backed and audited. The direct
compatibility form `loom serve <store> <surface> ...` configures a listener with the same validation
path, but the `configure` form is the documented operator form. The source-backed record currently
contains id, schema version, surface, selector list, transport, bind address, enabled flag, TLS mode
and certificate-bundle reference, auth mode, route scope, exposure mode, network access policy
reference, request/idle/session limits, audit mode, and last-modified audit sequence. Records without
an explicit supported schema version are rejected. Daemon startup reloads enabled
`cas/rest`, `cas/json_rpc`, `cas/grpc`, `files/rest`, `files/json_rpc`, `vcs/rest`,
`vcs/json_rpc`, `admin/rest`, `admin/json_rpc`, `sql/rest`, `sql/json_rpc`, `sql/grpc`,
`chat/rest`, `chat/json_rpc`, and selected data-facet REST/JSON-RPC listener records, opens hosted HTTP or tonic/prost gRPC listeners as appropriate, audits runtime open and close, applies configured
request-size, idle-timeout, and session-timeout limits to HTTP surfaces, supports direct rustls TLS for
the current TLS-enabled hosted HTTP, CalDAV, CardDAV, IMAP, and JMAP listener runtimes with stored
certificate-bundle PEM certificate/key loading and optional PEM trust-bundle loading for
client-certificate verification, enforces `owner-or-passphrase` and `passphrase` HTTP auth modes,
reconciles enabled/disabled/removed/modified listener records while the daemon is running, and rejects
unsupported direct-TLS/auth/exposure combinations, invalid TLS material, or FIPS-profile stores from
the current non-FIPS hosted runtime with audited rejection. gRPC direct TLS, MCP listener runtime TLS,
remaining gRPC services, product TCP compatibility listener TLS, time-series compatibility HTTP TLS,
and full protocol conformance are still target work.
Admin REST and admin JSON-RPC are source-backed for capability reporting, listener, identity, ACL,
protected-ref, and audit management under global Admin authorization.

The portable network admission contract is 0066 network access. `loom network-access list|set|audit|remove`
manages reusable ordered policies with default allow or deny behavior, CIDR rules, trusted proxy CIDRs,
and mTLS certificate criteria. `loom serve configure` attaches a policy with
`--network-access <policy-name>`. Referenced policy removal is rejected, listener startup fails closed
when a referenced policy is missing or incompatible with TLS client-certificate verification, and the
daemon runtime identity includes the policy digest so affected listeners restart when the policy
changes.
The current gRPC acceptor evaluates only the direct peer address. It does not provide forwarded-proxy
headers or verified peer-certificate input to the network-access evaluator, so those HTTP capabilities
must not be claimed for gRPC listeners.

The portable TLS contract is 0052 certificate bundles. `loom serve configure`
uses `--tls-certificate-bundle <name>` and no longer accepts `--tls-mode`, `--tls-cert-ref`,
`--tls-key-ref`, or `--trust-bundle-ref`. Bundle bytes are copied into the `.loom` file, direct hosted TLS loads from those
stored bytes, removal is guarded by computed served-listener and hosted-project references, and daemon
restart validates the bundle digest used by each enabled listener.

The serving command is action-first for operator management. Transport alone is not enough:
`--rest`, `--json-rpc`, and `--grpc` describe HOW traffic is carried, but not WHAT Loom surface is
exposed. The public operator shape is:

```text
loom serve configure <store> <surface> [selector...] --bind <addr> [--transport <transport>] [policy flags]
loom network-access list <store>
loom network-access set <store> <name> --default-action deny --allow-source <cidr>
loom network-access audit <store> <name>
loom network-access remove <store> <name>
loom serve list <store>
loom serve enable <store> <listener-id>
loom serve disable <store> <listener-id>
loom serve remove <store> <listener-id>
```

Examples:

```text
loom serve configure app.loom cas work --transport rest --bind 127.0.0.1:8001
loom serve configure app.loom sql work main --transport json-rpc --bind 127.0.0.1:8002
loom serve configure app.loom redis work default --bind 127.0.0.1:6379 --persistence versioned
loom serve configure app.loom memcached work sessions --bind 127.0.0.1:11211
loom serve configure app.loom kafka work --bind 127.0.0.1:9092
loom serve configure app.loom influx work --bind 127.0.0.1:8086
loom serve configure app.loom prometheus work --bind 127.0.0.1:9090
loom serve configure app.loom vector work embeddings --transport rest --profile qdrant --bind 127.0.0.1:6333
loom serve configure app.loom admin --transport rest --bind 127.0.0.1:8003
loom serve configure app.loom mcp --bind 127.0.0.1:8004
loom network-access set app.loom office --default-action deny --allow-source 203.0.113.0/24
loom serve configure app.loom admin --transport rest --bind 0.0.0.0:8003 --network-access office
```

`loom doctor` reports network-access policy health, reference counts, missing references, and
deny-by-default policies with no allow path. Enabled public binds on `0.0.0.0`, `::`, or concrete
non-loopback addresses should be paired with a network access policy; doctor reports these as warnings
so local development listeners on loopback remain lightweight.

Each served surface declares its allowed transports and whether it has a default transport. A default
is allowed only when the surface has one obvious transport. Otherwise `--transport` is required. The
control-plane projection is named `admin`.

Served listener grammar separates surfaces, transports, profiles, and engines:

- a surface owns product or domain semantics, selectors, lifecycle, capability reporting, and
  protocol-specific options;
- a transport owns encoding, framing, session, or RPC mechanics for the same surface semantics;
- a profile owns a compatibility dialect only when the selector model and public options still belong
  to the same surface;
- an engine is an internal implementation detail and is not a served surface by itself.

Redis, Memcached, S3, OCI, Kafka, Influx, Prometheus, Grafana, and OTLP-class compatibility are
surfaces because their clients expect protocol-specific command spaces, selectors, lifecycle, or
policy options. `rest`, `json_rpc`, `grpc`, `resp`, `text`, `ndjson`, and Arrow Flight-style batch
carriers are transports when they preserve the owning surface semantics. Polars, Tantivy, HNSW, and
GlueSQL are engines.

The served-listener registry admits these surface records. Admission means the durable listener intent
can be configured and audited. Runtime support still depends on a daemon opener for the selected
surface and transport.

| Surface id | Selectors | Route scope | Default transport | Admitted transports | Runtime status |
| --- | --- | --- | --- | --- | --- |
| `admin` | none | whole store | `rest` | `rest`, `json_rpc` | source-backed daemon opener |
| `mcp` | none | whole store | `mcp_http` | `mcp_http` | target daemon opener |
| `cas` | workspace | workspace | `rest` | `rest`, `json_rpc`, `grpc` | `rest`, `json_rpc`, and `grpc` source-backed daemon openers; OCI, S3, and CAR are separate compatibility or interchange surfaces over CAS/files primitives rather than transports under `cas` |
| `s3` | workspace, optional bucket | workspace collection | `rest` | `rest` | First-class S3-compatible served surface. Daemon-opened `rest` is source-backed for one-selector service endpoints scoped to one workspace, two-selector bucket-scoped endpoints where the request path is the object key root, path-style fallback, virtual-host bucket selection, bucket create/list/delete, object put/get/head/delete, metadata headers, byte ranges, conditional writes, opaque S3-safe version IDs, S3-compatible ETags separate from Loom digests, basic multipart upload, hosted auth/PEP, SigV4 app credential verification, configured unauthenticated public-read ACLs, guarded AWS CLI create/put/get transcript coverage, and conformance report rows. Direct TLS for `s3/rest` uses the shared hosted TLS path when configured |
| `oci` | workspace | workspace collection | `rest` | `rest` | First-class OCI Distribution compatible served surface. Daemon-opened `rest` is source-backed for public slash-separated repository names, stable internal repository ids plus display metadata, OCI and Docker v2 schema media-type admission, schema v1 and unknown dangerous media-type rejection, manifest PUT/GET/HEAD/DELETE, blob GET/HEAD/DELETE, monolithic upload, durable chunked upload, upload status/cancel, cross-repository mount, tags list, bounded catalog, referrers, strict SHA-256 digest verification, hosted auth/PEP, and conformance report rows. Direct TLS for `oci/rest` uses the shared hosted TLS path when configured |
| `files` | workspace | workspace | none | `rest`, `json_rpc`, `grpc` | `rest`, `json_rpc`, and native `grpc` source-backed daemon openers for read, write, stat, list, mkdir, and delete. Native gRPC list is server-streaming. Append, range, handle, symlink, move, copy, archive import/export, generated protobuf artifacts, and S3-backed projection internals remain target. Archive import/export targets canonical `tar.zstd` plus compatibility `tar`, `tar.gz`, and `zip` formats |
| `web` | workspace | workspace | `rest` | `rest` | Source-backed daemon opener for static Webish HTTP over namespace files: `GET /` serves `index.html`, `GET /path/` serves `path/index.html`, `GET /path` tries the exact file then `path.html`, `HEAD` omits the body, common content types are emitted, and hidden `.loom` paths are not served. Hosted listener-backed route-table dispatch is source-backed for static routes. `loom serve route list/set/remove` persists audited static route-table configuration for stored `web/rest` listeners; the daemon loads the persisted table at listener startup; hosted admin REST and JSON-RPC expose equivalent static route management. Templates, hooks, full Webish operation-envelope writes, cache workers, TLS certificate providers, MCP Webish management, and broader Webish conformance vectors remain target |
| `vcs` | workspace | workspace | none | `rest`, `json_rpc`, `grpc` | `rest`, `json_rpc`, and native `grpc` source-backed daemon openers for commit, commit-staged, log, branch, checkout, status, stage, stage-all, unstage, merge, and structural diff. Native gRPC log is server-streaming and diff returns structural `LMDIFF` CBOR bytes. Merge-resolution routes, tag/restore/replay, update-ref, read-object, generated schemas, and full conformance remain target |
| `sql` | workspace, database | workspace collection | none | `rest`, `json_rpc`, `grpc` | Native Loom SQL surface. `rest`, `json_rpc`, and unary `grpc` are source-backed daemon openers. PostgreSQL and MySQL product protocols are first-class `postgres/tcp` and `mysql/tcp` surfaces because their handshakes, catalogs, sessions, dialects, and option sets are not native SQL transport mechanics. |
| `postgres` | workspace, database | workspace collection | `tcp` | `tcp` | Source-backed daemon opener over the PostgreSQL wire implementation. `tokio-postgres` covers simple query, parameterless extended query, bounded prepared-statement Bind/Execute parameter rewriting over `loom-sql` metadata, auth denial, stable transaction-boundary rejection with SQLSTATE `0A000`, bounded pgvector-style exact search, and bounded columnar analytical queries through `columnar.<dataset>`; guarded local `psql` over libpq covers create, insert, select, `\dt`, `\d`, and `\d+`. PostgreSQL SSLRequest negotiation upgrades through the shared hosted TLS configuration path before authentication, and raw loopback transcript coverage asserts TLS startup plus cleartext-password auth over the upgraded stream. JDBC, Node, Python, and BI-tool transcript rows are tracked as guarded target coverage until their local driver harnesses are source-backed. COPY and broader catalog compatibility remain target. |
| `mysql` | workspace, database | workspace collection | `tcp` | `tcp` | Source-backed daemon opener over the MySQL wire implementation for cleartext passphrase auth, app-credential auth, native-password auth, MySQL `CLIENT_SSL` negotiation over the shared hosted TLS configuration path, `COM_INIT_DB`, `COM_QUERY`, `COM_PING`, `COM_STMT_PREPARE`, `COM_STMT_EXECUTE`, `COM_STMT_CLOSE`, `COM_STMT_RESET`, one-result simple query execution, text resultsets, binary prepared-statement resultsets, bounded `SHOW` and information-schema metadata shims, transaction-boundary rejection, raw loopback protocol transcripts, and guarded local MySQL 8.4 CLI transcript evidence. Guarded optional Connector/J, Node `mysql2`, and Python PyMySQL/mysqlclient transcript harnesses run against the real listener when local tooling is installed and stay target evidence when unavailable. `caching_sha2_password`, direct TLS, richer binary type metadata, and mandatory checked-in language-driver transcript profiles remain target. |
| `kv` | workspace, collection | workspace collection | none | `rest`, `json_rpc`, `grpc` | `rest`, `json_rpc`, and native `grpc` source-backed daemon openers for put, get, delete, list, and range over canonical-CBOR keys and raw value bytes. Native gRPC range is server-streaming. Collection discovery and broader protocol conformance remain target. `etcd` is promoted to a first-class surface rather than a KV transport. Couchbase KV behavior belongs only to the deferred P3 Couchbase integrated-surface design |
| `etcd` | workspace, collection | workspace collection | `tcp` | `tcp` | Source-backed first-class etcd-compatible served surface over Loom KV collections. Daemon-opened `tcp` serves gRPC method paths for `etcdserverpb.KV` `Range`, `Put`, `DeleteRange`, selected `Txn` compare/apply behavior, `Compact`, `etcdserverpb.Lease` `LeaseGrant`, `LeaseRevoke`, and `LeaseKeepAliveOnce`, and bounded `etcdserverpb.Watch` replay from the durable event log. It uses durable sidecar metadata for revisions, compacted revision, per-key create/mod/version counters, lease-owned keys, and replayable events while preserving raw native KV values. Hosted conformance rows distinguish supported, degraded, target, and unsupported etcd behavior. Cluster, auth, and maintenance services are registered with stable `UNIMPLEMENTED` responses. Live Watch tailing, member, cluster, auth, maintenance, and quorum-administration implementations remain target |
| `redis` | workspace, keyspace | workspace collection | `resp` | `resp` | Dedicated Redis-compatible served surface over the Redis substrate from 0019b; daemon-opened RESP is source-backed for AUTH, PING, strings, TTL, hashes, sets, lists, sorted sets, durable reload, queue-backed stream commands `XADD`, `XLEN`, `XRANGE`, `XREVRANGE`, `XREAD`, and `XDEL`, and runtime-only pub/sub commands `SUBSCRIBE`, `UNSUBSCRIBE`, `PSUBSCRIBE`, `PUNSUBSCRIBE`, `PUBLISH`, and bounded `PUBSUB` introspection. Stream consumer groups, stream trimming, blocking stream reads, and broader option coverage retain explicit unsupported boundaries |
| `memcached` | workspace, cache | workspace collection | `text` | `text` | Dedicated Memcached-compatible served surface over cache semantics from 0019b; daemon-opened text protocol is source-backed for version, get, gets, gat, gats, set, add, replace, append, prepend, incr, decr, cas, touch, delete, flush_all, verbosity, and stats over volatile state by default. Explicit `loom serve configure ... memcached ... --mode versioned|read-through|write-through|write-around|write-behind` selects a durable or backed cache profile mapped to the 0019a KV tier config |
| `document` | workspace, collection | workspace collection | none | `rest`, `json_rpc`, `grpc` | `rest`, `json_rpc`, and native `grpc` source-backed daemon openers for document put-text, get-text, put-binary, get-binary, delete, list-binary, index create/drop/list/status/rebuild, find, and native query. Native gRPC list-binary and find are server-streaming. Collection discovery, generated protobuf artifacts, and broader protocol conformance remain target. MongoDB and Couchbase are P3/spec-owned first-class compatibility candidates now that native document indexes/query are source-backed. CouchDB serving is cut from the current roadmap |
| `tickets` | workspace | workspace | none | `rest`, `json_rpc` | Daemon-opened REST and JSON-RPC routes are source-backed for the current promoted ticket vertical: project create, project re-key, project settings, ticket create/update/delete, ticket field update, ticket get by UUID or derived key, relation set/remove, and ticket operation history. The listener collection is the ticket workspace id. Routes preserve `expected_root`, stale-root `CONFLICT`, UUID ticket identity, derived key resolution, external identity uniqueness, operation-log history, revision rows, internal retired-prefix redirects after rekey, and text-field reference indexing through the shared `loom-tickets` helper used by MCP and hosted writes |
| `spaces` | workspace | workspace | none | `rest`, `json_rpc` | Daemon-opened REST and JSON-RPC routes are source-backed for space create, list, and get over the shared `loom-pages` component. The listener collection is the Pages workspace id. Routes preserve `expected_root`, stale-root `CONFLICT`, operation-log writes, and the profile root returned by the page workspace snapshot |
| `pages` | workspace | workspace | none | `rest`, `json_rpc` | Daemon-opened REST and JSON-RPC routes are source-backed for page create, draft update, publish, get, and history over the shared `loom-pages` component. The listener collection is the Pages workspace id. Routes preserve `expected_root`, stale-root `CONFLICT`, draft status, published revision history, rendered body text, render issues, block-ref read-through, text reference indexing, block-ref reference indexing, and revision-index updates on publish |
| `structures` | workspace | workspace | none | `rest`, `json_rpc` | Daemon-opened REST and JSON-RPC routes are source-backed for structure create, get, add-node, update-node, bind, move-node, and link-node over the shared `loom-pages` component. The listener collection is the Pages workspace id. Routes preserve `expected_root`, stale-root `CONFLICT`, structure graph projection, node and edge summaries, and graph collection naming. Structure decomposition to tickets remains MCP-only until hosted route tests cover the cross-profile rollback behavior |
| `drive` | workspace | workspace | none | `rest`, `json_rpc` | Durable listener admission is source-backed: `loom serve <store> drive <workspace> --transport rest|json-rpc` validates and persists the listener intent. The Drive profile is the workspace Drive and has no second profile selector. Daemon-opened REST and JSON-RPC routes are source-backed for the current hosted Drive read/write vertical plus share and retention metadata management. Share-to-ACL projection, manual share-expiry application, manual retention application, daemon-scheduled policy application for registered Drive policy targets, local OS projection primitives, and served dehydrate, hydrate, worker-plan, and OS-write routes are source-backed. OS-native placeholder hooks, worker scheduling, and platform hydration/eviction adapters remain target |
| `meetings` | workspace | workspace | none | `rest`, `json_rpc` | Daemon-opened REST and JSON-RPC routes are source-backed for product-shaped meeting list/get/search reads, projection-output reads, deterministic projection-output apply, materialized output readback, extraction review, annotation accept/reject, vocabulary propose/accept/reject, and entity-merge writes over stored Meetings snapshots. The Meetings profile is the workspace Meetings profile; there is no nested organization selector. Apply writes document, files, graph, search, SQL/dataframe, and ledger projections. SQL/dataframe outputs persist into SQL database `meetings/{workspace}` table `meetings_projection_outputs`; vector outputs persist projection-output-level Studio embedding jobs with durable `no_engine` state until built-in embedding inference is configured. When the daemon can resolve a configured text-embedding binding for the workspace, served apply also drains vector outputs into physical vector records. The materialized output readback reports concrete document/file/graph/FTS/SQL-dataframe/ledger artifacts plus physical vector records and vector job records. CLI `loom meetings list` and `loom meetings get` read stored snapshots; CLI/MCP/hosted `meetings search` delegates to the store-wide search contract scoped to materialized meeting projection text. CLI `loom studio reindex` can drain Meetings vector outputs into physical vector records when a text-embedding instance is bound. Ledger appends avoid duplicates by projection output id. Meetings import execution over hosted protocols, assistant answer generation, and `verify-apps` visual coverage remain target |
| `queue` | workspace, collection | workspace collection | none | `rest`, `json_rpc`, `grpc` | `rest`, `json_rpc`, and native `grpc` source-backed daemon openers for append, get, range, and len. Native gRPC protocol conformance proves append, get, range, and len. Consumer-offset gRPC, broader REST/JSON-RPC protocol conformance, and observed-anchor validation remain target. Target cleanup promotes Kafka, MQTT, NATS/JetStream, and AMQP-class ecosystems to separate compatibility surface candidates rather than queue transports |
| `kafka` | workspace | workspace | `tcp` | `tcp` | First-class Kafka-compatible served surface defined by 0021c over Loom queue collections. Daemon-opened `tcp` is source-backed for ApiVersions, SASL PLAIN auth through hosted principals and app credentials, authenticated Metadata, CreateTopics, DeleteTopics, durable topic metadata records, shared durable workspace-scoped metadata-version allocation, stable topic UUIDs, Kafka record-batch Produce including gzip, Snappy, LZ4, and Zstandard compression, Fetch with normalized visible offsets, OffsetCommit over queue consumer progress, invalid opaque batch rejection, producer id allocation, producer epoch fencing, non-transactional and transactional idempotent produce sequence validation, exact duplicate retry recognition, bounded transaction-control records for AddPartitionsToTxn, AddOffsetsToTxn, and EndTxn, transactional offset commits that apply on EndTxn commit and are discarded on abort, transactional produced-record visibility for read-committed and read-uncommitted Fetch, and conformance capability rows. Older message-set versions, consumer-group membership and rebalance, AddPartitionsToTxn v4/v5 multi-transaction batches, transaction timeout enforcement, and multi-partition topics remain target; multi-broker replication, ISR, and broker election are unsupported |
| `mqtt` | workspace | workspace | `tcp` | `tcp` | Target first-class MQTT-compatible served surface candidate over queue/eventing primitives. MQTT is not a `queue --transport mqtt` option because QoS levels, retained messages, sessions, subscriptions, topic filters, will messages, and broker lifecycle are product/domain semantics rather than generic queue framing. Candidate command shape is `loom serve configure <store> mqtt <workspace> --bind 127.0.0.1:1883`; implementation requires an owning MQTT design before build |
| `nats` | workspace | workspace | `tcp` | `tcp` | Target first-class NATS-compatible served surface candidate over queue/eventing primitives, with JetStream as an explicit subprofile or separate promoted surface after design. NATS is not a `queue --transport nats` option because subjects, queue groups, request/reply, durable consumers, streams, retention, acknowledgements, and JetStream lifecycle are product/domain semantics rather than generic queue framing. Candidate command shape is `loom serve configure <store> nats <workspace> --bind 127.0.0.1:4222`; NATS core versus JetStream scope requires an owning design before build |
| `time-series` | workspace, series | workspace collection | none | `rest`, `json_rpc`, `grpc` | `rest`, `json_rpc`, and native `grpc` source-backed daemon openers for byte-facade point operations, structured point put/range, policy, rollup materialization, rollup range, and explicit prune. Native gRPC protocol conformance proves put, get, latest, and server-streaming range. Generated protobuf artifacts, REST/JSON-RPC protocol conformance, broader compatibility conformance, and collection discovery remain target. Influx, Prometheus, Grafana, and OTLP-class ecosystems are separate compatibility surfaces rather than time-series transports |
| `influx` | workspace | workspace | `http` | `http` | Source-backed first-class Influx-compatible served surface over canonical time-series points. Daemon-opened HTTP accepts line protocol at `/api/v2/write` and `/write`; bucket or db selects the time-series collection; measurement, tags, fields, and precision-scaled timestamps map to structured points. Direct TLS, InfluxQL, Flux, and query API compatibility remain target |
| `prometheus` | workspace | workspace | `http` | `http` | Source-backed first-class Prometheus-compatible served surface over canonical time-series points. Daemon-opened HTTP accepts Snappy-compressed Prometheus remote-write protobuf at `/api/v1/write`, maps `__name__` and labels to structured points in the workspace Prometheus collection, and exposes simple selector `query` and `query_range` JSON responses. Direct TLS, full PromQL, remote read, and broader capability reporting remain target |
| `grafana` | workspace, optional collection | workspace collection | `http` | `http` | Source-backed first-class Grafana datasource surface over canonical structured time-series query behavior. Daemon-opened HTTP provides health, search, and query routes with Grafana-style datapoints for exact metric selectors. Direct TLS, full Grafana plugin lifecycle, annotations, variables, and broader query semantics remain target |
| `otlp` | workspace | workspace | none | `grpc`, `http` | Source-backed first-class OTLP HTTP metrics ingestion surface for JSON gauge and sum datapoints at `/v1/metrics`; resource and datapoint attributes map to tags in the workspace OTLP collection. Direct TLS, OTLP protobuf, gRPC, logs, traces, histograms, exemplars, and full partial-success semantics remain target |
| `columnar` | workspace, dataset | workspace collection | none | `rest`, `json_rpc`, `grpc`, `arrow_flight`, `parquet`, `duckdb_like`, `snowflake_like`, `spark_like`, `bigquery_like` | `rest`, `json_rpc`, and native `grpc` source-backed daemon openers for native columnar create, append, compact, inspect, source-digest, scan, columns, rows, select, and aggregate. Native gRPC uses the shared columnar canonical-CBOR request/response codecs for schema, row, projection, filter, aggregate, result rows, values, inspect, and digest payloads. REST and native gRPC binary Arrow IPC and Parquet import/export are source-backed behind `columnar-arrow`, with stable `UNSUPPORTED` in builds without that feature. REST prepared Arrow IPC result handles are principal-bound, session-bound, one-shot, and PEP-rechecked. Arrow Flight, Flight SQL, ADBC-adjacent, and warehouse-style profiles remain target. Target cleanup keeps Arrow Flight and Parquet as transport or interchange formats, while warehouse-style ecosystems require profile or first-class surface decisions before promotion |
| `vector` | workspace, collection | workspace collection | none | `rest`, `json_rpc`, `grpc` | `rest` and `json_rpc` source-backed daemon openers; `grpc` and compatibility profiles target. Compatibility serving uses generic `rest` or `grpc` with explicit `--profile`; no default profile is implied. PostgreSQL-wire owns the bounded pgvector-style SQL presentation rather than treating it as a vector transport |
| `fts` | workspace, collection | workspace collection | none | `rest`, `json_rpc`, `grpc`, `ndjson` | Canonical served surface for the `search` capability's full-text collection/index API. `rest`, `json_rpc`, and native `grpc` are source-backed daemon openers for create, index, get, delete, ids, remap, and query. Native gRPC ids are server-streaming and use typed mapping, document, and query messages. `ndjson` is source-backed for the OpenSearch-compatible bulk route set. `search` is not a served-surface alias. Generated protobuf artifacts and broader conformance remain target |
| `graph` | workspace, graph | workspace collection | none | `rest`, `json_rpc`, `grpc` | `rest` and `json_rpc` source-backed daemon openers for native graph CRUD, traversal, bounded query, explain-query, and capability reporting. Native `grpc` is source-backed for node upsert/get, edge upsert, server-streaming neighbors and reachability, bounded openCypher query, explain-query, and capability reporting over canonical CBOR graph payloads. Native graph owns Loom graph semantics and the bounded GQL-aligned openCypher profile over native graph IR. Core semantic graph diff/merge has canonical wire CBOR projection, but no hosted methods yet. Generated protobuf artifacts, full CRUD parity, cursoring, mutation-plan projection, broader hosted conformance, and hosted graph diff/merge methods remain target. Bolt is not a generic graph transport. Gremlin is cut from the current roadmap |
| `neo4j` | workspace, graph | workspace collection | `tcp` | `tcp` | First-class Neo4j-compatible served surface over the native graph substrate. The compatibility matrix, native graph capability rows, durable listener admission, daemon runtime opening, and bounded Bolt 5.1 read subset are source-backed. Current `neo4j/tcp` negotiates Bolt 5.1, authenticates through hosted principals or app credentials, handles lifecycle messages, lowers bounded read `RUN` queries with scalar parameters into native graph query IR, streams `PULL` records, and projects scalar, node, relationship, and path values into Neo4j-shaped records. Write-query execution, explicit transaction semantics, catalog/procedure compatibility shims, and official-driver transcript conformance remain target work. It must not claim full Neo4j until matrix rows and official-driver transcripts prove the supported subset |
| `ledger` | workspace, collection | workspace collection | none | `rest`, `json_rpc`, `grpc`, `transparency_log` | `rest` and `json_rpc` source-backed daemon openers for append, get, head, len, and verify. Native `grpc` is source-backed for append, get, server-streaming range, head, len, verify, collection listing, checkpoint payload, checkpoint-signature verification, proof tree, inclusion proof, and consistency proof. Ledger compatibility work focuses on the Loom-native structured ledger, signed checkpoints, retention metadata, explicit replay, and derived proof artifacts. REST/JSON-RPC range and proof parity, generated protobuf artifacts, hosted conformance, witness publication, physical pruning, and transparency-log behavior remain target. Product-clone ledger database compatibility is not an active target |
| `calendar` | workspace | workspace | `caldav` | `rest`, `json_rpc`, `caldav` | bounded `caldav` source-backed daemon opener for `.well-known/caldav`, OPTIONS, PROPFIND, MKCALENDAR, GET, PUT, DELETE, conditional writes, `calendar-multiget`, `calendar-query`, commit-backed `sync-collection` with tombstones over `.ics`, and direct TLS through certificate-bundle listener records; REST, JSON-RPC, full conformance, and reference-client validation remain target |
| `contacts` | workspace | workspace | `carddav` | `rest`, `json_rpc`, `carddav` | bounded `carddav` source-backed daemon opener for `.well-known/carddav`, OPTIONS, PROPFIND, MKCOL, GET, PUT, DELETE, conditional writes, address-book properties, resource properties, `addressbook-multiget`, `addressbook-query`, vCard 3.0 input/output with registered-property preservation, vCard 4.0 `address-data`, commit-backed `sync-collection` with tombstones over `.vcf`, and direct TLS through certificate-bundle listener records; REST, JSON-RPC, full conformance, Apple-label semantics, and reference-client validation remain target |
| `mail` | workspace | workspace | none | `rest`, `json_rpc`, `imap`, `jmap` | `imap` source-backed daemon opener for login, SASL PLAIN authentication, mailbox list/create/delete/select/status, durable per-mailbox numeric UID mapping, stable `UIDVALIDITY`, stable `UIDNEXT`, durable `SUBSCRIBE`/`UNSUBSCRIBE`/`LSUB` metadata, message fetch, UID fetch, flag mutation, fetch-attribute validation, STORE `.SILENT`, bounded common search criteria, copy/move, APPEND with synchronizing literals, EXPUNGE, CLOSE, WORKSPACE, IDLE completion, and direct rustls IMAPS serving; `jmap` source-backed daemon opener for session discovery, Mailbox get/set, Email query/get, Email set update/destroy, blob upload/download, Email import/create from uploaded blobs, stable Identity get, deterministic state tokens, Email changes/queryChanges, direct TLS for current JMAP routes, and explicit unsupported push; REST, JSON-RPC, full RFC 9051 conformance, recursive SEARCH operators, date/comparator search extensions, non-synchronizing literal handling, full JMAP conformance, and reference-client validation remain target |

The registry accepts `timeseries` and `time_series` as CLI aliases for the canonical `time-series`
surface id. The full-text served surface is `fts`; `search` is the store-wide CLI and MCP discovery
surface, not a served-surface alias. Transport flags accept hyphenated spellings such as
`json-rpc` and `pg-wire`, then store normalized underscore ids in the durable listener record.

Redis and Memcached are compatibility surfaces, not transports under `kv`. `redis` uses the `resp`
transport and owns one Redis-like command space with explicit persistence policy. `memcached` uses the
`text` transport and owns the Memcached text protocol cache profile. The base `kv` surface remains the
Loom-native typed ordered KV API.

The remaining registry cleanup audit is:

| Area | Current admitted shape | Target classification | Follow-up owner |
| --- | --- | --- | --- |
| SQL PostgreSQL | `sql` with `pg_wire` transport | First-class `postgres` surface with `tcp` transport; PostgreSQL wire, catalog, session, auth, prepared statements, COPY, and pgvector-style extension behavior belong to that surface | Source-backed registry/runtime promotion; remaining compatibility work is owned by SQL wire specs |
| SQL MySQL | `sql` with `mysql_wire` transport | First-class `mysql` surface with `tcp` transport; MySQL handshake, auth plugins, schema selection, metadata, prepared statements, and dialect behavior belong to that surface | Source-backed registry/runtime promotion; remaining compatibility work is owned by SQL wire specs |
| KV etcd | promoted from stale `kv` with `etcd_grpc` transport | First-class `etcd` surface with `tcp` transport; revisions, leases, compare/swap transactions, watches, and member APIs are product semantics. KV/Lease gRPC methods are source-backed; Watch and member/cluster APIs remain target | P9-0007 |
| Couchbase | stale `kv` or `document` compatibility transport candidates | P3 first-class `couchbase` surface only after an integrated KV, document, query, and analytics design is approved; native document indexes/query are source-backed foundation, not the full Couchbase product surface | P9-0007/P9-0008/P9-0018 |
| MongoDB | stale `document` with `mongodb_wire` transport candidate | P3 first-class `mongodb` surface now that native document indexes/query are source-backed; query/update grammar, indexes beyond the native subset, collections, wire metadata, sessions, and client lifecycle are product semantics | P9-0008 |
| CouchDB | stale `document` with `couchdb_rest` transport candidate | Cut from the current roadmap; reconsider only if revision trees, conflicts, `_changes`, and replication become strategic | P9-0008 |
| Vector pgvector | Native vector has no pgvector listener transport | PostgreSQL/pgvector projection under the `postgres` surface; native vector remains `vector` with profiles such as Qdrant or Pinecone | Source-backed bounded PostgreSQL-wire presentation; remaining PostgreSQL compatibility is owned by SQL wire work |
| Graph Bolt | legacy native-graph transport candidate | First-class `neo4j` surface with `tcp` transport; Bolt handshake, sessions, driver compatibility, results, errors, transactions, and catalog behavior are Neo4j product semantics, not generic graph framing | P9-0014 |
| Graph Gremlin | legacy native-graph profile or transport candidate | Cut from the current roadmap; reopen only through a separate owner-approved design session | P9-0014 |
| Ledger transparency logs | `ledger` with `transparency_log` transport | Native Ledger structured storage, principal-signed checkpoints, retention metadata, explicit replay, and derived proof artifacts first; transparency-log can remain a ledger profile after proof, witness, disclosure, and conformance semantics land. Product-clone ledger database compatibility is cut from active design | P9-0015 |
| PIM standards protocols | `calendar/caldav`, `contacts/carddav`, `mail/imap`, `mail/jmap` | Keep under owning PIM surfaces because they are standards access protocols for the same domain semantics | Queue 7/PIM specs |
| SMTP | `mail/smtp` target transport | Keep only for bounded mailbox-adjacent compatibility; real submission, relay, delivery, and outbound policy require a PIM-owned design and may become a first-class submission surface | Queue 7/PIM specs |

Compatibility profile flags are separate from transports. For vector serving, `--profile qdrant` and
`--profile pinecone` select the vendor-shaped request and response profile over a generic `rest` or
`grpc` transport, while `--profile generic` selects the Loom-native hosted profile. A vector
compatibility listener without an explicit profile is invalid.

Every registry entry inherits the v2 served-listener policy fields: TLS mode and material references,
authentication mode, exposure mode, request-size limit, idle timeout, session timeout, audit mode, and
last-modified audit sequence. Authorization is never transport-local. The route scope only declares the
largest resource boundary a listener can expose; the adapter still resolves a principal and delegates
read/write decisions to the owning facet PEP before data access.

The audit classes are common across the registry. Management writes emit `serve.listener.configure`,
`serve.listener.enable`, `serve.listener.disable`, and `serve.listener.remove`; listing emits
`serve.listener.list`; daemon runtime transitions emit `serve.listener.open`, `serve.listener.close`,
or `serve.listener.reject`. Facet-specific hosted adapters add auth failure, authorization denial, and
facet read/write audit events where the owning facet spec requires them.

An admitted but unsupported surface is durable listener intent, not proof of runtime support. Current
daemon startup opens only supported records and leaves unsupported target records closed until their
owning task adds a daemon opener. Invalid TLS/auth/exposure/FIPS combinations are rejected and audited
before any bind attempt.

Local `loom mcp` remains the locally runnable MCP host. Hosted MCP is a served surface owned by the
hosted listener system, not by the local stdio command. The hosted command spelling is
`loom serve configure <store> mcp`.

The durable served-listener record includes:

- served surface, such as `cas`, `sql`, `kv`, `redis`, `memcached`, `admin`, `mcp`, or a future
  surface id;
- surface selector, such as workspace, collection, database, or whole-store scope;
- transport: `rest`, `json_rpc`, `mcp_http`, `resp`, `text`, reserved `grpc`, or a future transport id;
- bind address and port, constrained by the daemon's deployment profile;
- enabled flag;
- TLS mode plus certificate, key, and trust-bundle references;
- authentication mode;
- route scope, such as whole Loom, workspace-bound, or workspace-and-collection-bound;
- read/write exposure mode;
- network access policy reference;
- request-size, idle-timeout, and session-timeout settings;
- audit mode;
- last-modified audit sequence.

Target fields still to promote:

- accepted credential proof verifiers beyond passphrase and app-specific API keys;
- rate-limit settings;

The store path itself is not stored in this record. The active store is selected by the command that
starts or attaches to the daemon. A copied `.loom` file carries its listener intent as policy state,
but a daemon may refuse to bind externally until a local operator explicitly confirms the deployment
profile on that host.

Changing a served listener through `loom serve` is an audited control-plane operation. Source-backed
management events cover configure, list, enable, disable, and remove. Runtime listener open and close
are source-backed for current daemon-opened CAS and admin listeners. A served listener must not be
opened if authenticated mode requires credentials that the listener cannot verify, if TLS is required
but not configured, or if the policy would expose unauthenticated owner-mode writes outside a
deployment-confined loopback profile.

A Loom marked FIPS-required must not be served through a non-FIPS TLS or provider path. This rule
applies to every hosted protocol and is specified by 0060.

### 7.1.1 Admin JSON-RPC methods

The source-backed `admin/json_rpc` listener uses `POST /jsonrpc` and JSON-RPC 2.0 envelopes. The
method result is the same JSON object returned by the equivalent Admin REST route, wrapped as the
JSON-RPC `result`. Loom failures return JSON-RPC application errors with Loom `Code` in
`error.data.loom_code` and `error.data.loom_code_number`.

| Method | Params | Result shape |
| --- | --- | --- |
| `admin.capabilities` | optional `{ "detailed": boolean }` | capability matrix JSON, detailed by default |
| `admin.listeners.list` | `{}` | `{ "listeners": [...] }` |
| `admin.listeners.enable` | `{ "id": string }` | served-listener record |
| `admin.listeners.disable` | `{ "id": string }` | served-listener record |
| `admin.listeners.remove` | `{ "id": string }` | `{ "seq": integer, "id": string }` |
| `admin.audit.list` | `{}` | `{ "records": [...] }` |
| `admin.audit.export` | `{}` | `{ "records": [...] }` plus an audit event for the export |
| `admin.audit.config.get` | `{}` | `{ "retention_days": integer, "legal_hold": boolean }` |
| `admin.audit.config.set` | optional `{ "retention_days": integer, "legal_hold": boolean }` | `{ "seq": integer, "config": { ... } }` |
| `admin.audit.prune` | `{ "through_seq": integer }` | `{ "pruned": integer, "checkpoint_seq": integer or null, "checkpoint_hash": string or null, "audit_seq": integer }` |
| `admin.identity.list` | `{}` | `{ "principals": [...], "roles": [...], "app_credentials": [...], "external_credentials": [...] }` |
| `admin.identity.add` | `{ "id": string optional, "name": string, "kind": "root" or "user" or "service" }` | `{ "seq": integer, "principal": { ... } }` |
| `admin.identity.passphrase.set` | `{ "principal": string, "passphrase": string }` | `{ "seq": integer, "principal": string }` |
| `admin.identity.app_credentials.create` | `{ "principal": string, "label": string }` | `{ "seq": integer, "credential": { "id": string, "principal": string, "label": string, "enabled": boolean }, "secret": string }` |
| `admin.identity.app_credentials.revoke` | `{ "credential": string }` | `{ "seq": integer, "credential": { "id": string, "principal": string, "label": string, "enabled": boolean } }` |
| `admin.identity.external_credentials.create` | `{ "principal": string, "kind": "public-key" or "mtls-certificate" or "passkey" or "oidc-subject" or "saml-subject", "label": string, "issuer": string, "subject": string, "material_digest": string optional }` | `{ "seq": integer, "credential": { "id": string, "principal": string, "kind": string, "label": string, "issuer": string, "subject": string, "material_digest": string or null, "enabled": boolean } }` |
| `admin.identity.external_credentials.revoke` | `{ "credential": string }` | `{ "seq": integer, "credential": { "id": string, "principal": string, "kind": string, "label": string, "issuer": string, "subject": string, "material_digest": string or null, "enabled": boolean } }` |
| `admin.identity.remove` | `{ "principal": string }` | `{ "seq": integer, "principal": string }` |
| `admin.identity.roles.assign` | `{ "principal": string, "role": string }` | `{ "seq": integer, "target": string }` |
| `admin.identity.roles.revoke` | `{ "principal": string, "role": string }` | `{ "seq": integer or null, "removed": boolean, "target": string }` |
| `admin.acl.list` | `{}` | `{ "grants": [...] }` |
| `admin.acl.grant` | ACL grant object | `{ "granted": true, "grant": { ... } }` |
| `admin.acl.revoke` | ACL grant object | `{ "removed": boolean, "grant": { ... } }` |
| `admin.protected_refs.list` | `{ "workspace": string }` | `{ "policies": [...] }` |
| `admin.protected_refs.get` | `{ "workspace": string, "ref_name": string }` | protected-ref policy object or `null` |
| `admin.protected_refs.set` | protected-ref policy object with `workspace` and `ref_name` | protected-ref policy object |
| `admin.protected_refs.remove` | `{ "workspace": string, "ref_name": string }` | `{ "removed": boolean }` |

### 7.2 Hosted implementation libraries

The hosted implementation is split by responsibility:

- shared request authorization, stable Loom errors, and store open/save logic live in
  `crates/loom-hosted`;
- current `crates/loom-hosted` source backs REST, JSON-RPC, and unary tonic/prost gRPC CAS adapter
  methods over that shared kernel, including auth and ACL failure behavior;
- REST and JSON-RPC over HTTP use the existing tokio plus axum/hyper/tower stack already pulled by the
  MCP Streamable HTTP feature;
- JSON-RPC is a Loom-owned strict JSON-RPC 2.0 dispatcher over axum for v1 unless a later probe proves
  `jsonrpsee` adds enough server features to justify the extra dependency;
- gRPC uses tonic/prost for the source-backed CAS listener. Additional gRPC services, streaming row
  results, sync streams, and promoted protobuf conformance remain P1 because they bring a separate
  binary contract and HTTP/2 service stack;
- Direct TLS uses rustls with an explicitly selected provider profile rather than accidental default
  features. The FIPS-capable release profile selects rustls `fips` and AWS-LC-FIPS explicitly;
  default rustls is not a FIPS compliance claim. A deployment reverse proxy is still allowed, but
  production remote listeners require a trusted TLS boundary either way.

This keeps the hosted implementation DRY: REST and JSON-RPC share the same HTTP server and hosted
kernel, while gRPC reuses the same adapter logic behind a promoted protobuf surface.

## 8. Sync Over the Wire

Current source implements direct in-process sync helpers and offline bundles. The target hosted sync
protocol maps those semantics to transports without changing object identity or ref advancement
rules.

- **gRPC `Sync` bidirectional stream:** Preferred for hosted sync. Frames include `Hello`,
  `Have`/`Ack` rounds, object frames, and ref updates. The transport supplies backpressure and
  resumability.
- **HTTP:** `POST /sync/negotiate` exchanges `Hello` plus have/want state, `POST /sync/fetch` streams
  objects, and `POST /sync/push` advances refs. Resumption uses a bounded sync session id plus a
  cursor or byte range over the object stream.
- **WebSocket:** The gRPC-style frame sequence is tunneled as binary messages for browser-hosted
  clients.
- **Bundles:** `GET /sync/bundle?refs=...` streams an offline bundle and `POST /sync/bundle` imports
  one. This is the hosted projection of the current `Bundle` concept, but the hosted endpoints do not
  exist yet.

Live hosted sync must remain consistent with 0006: matching identity profiles, verified object
ingest, no silent cross-profile rehashing, and atomic ref advancement after required objects are
present.

## 9. MCP Serving Surface

The Model Context Protocol (MCP, spec revision 2025-06-18) is served by a native host crate
`loom-mcp` (tokio, over the `rmcp` SDK). It is a host process, not a language binding or a
wasm-embedded surface. MCP is richer than tool calls: it defines three server primitives (tools,
resources, prompts), client elicitation, and cross-cutting utilities (progress, cancellation,
completion, pagination, notifications). Roots, sampling, and logging are deprecated upstream and are
dropped from the Loom MCP surface.
The Loom MCP surface served now is the following.

| MCP primitive | Loom projection |
| --- | --- |
| Tools | Facet operations as `facet.method`; read and write calls both pass through the engine PEP. |
| Resources + templates | `loom://<workspace>/<path>` reads: files, CAS blobs by content address, and calendar/contacts/mail bodies via the source-backed `loom-vfs` codecs; the content address is the resource version/ETag. |
| Resource subscriptions | `resources/subscribe` plus `notifications/resources/updated`, source-backed today by per-session ETag polling over readable resources. Loom-native data changes use 0030 `DataChange` payloads with domain-owned `DomainChange` records; MCP lifecycle notifications keep lifecycle payloads. |
| Roots | Dropped. MCP roots are deprecated upstream; Loom scoping is launch configuration plus workspace/collection elision. |
| Elicitation | Source-backed for destructive-write confirmation. Unlock, principal selection, ACL approval, and merge-conflict choices are target uses. |
| Completion | Source-backed for workspace prefix completion. Collection, principal, ref/tag, path, SQL, and RRULE completion are target work. |
| Pagination | Source-backed offset-cursor paging for MCP list operations. Prolly-position cursors for large data ranges are target work. |
| Prompts | Source-backed as curated prompt templates. Prompt execution is client-driven and never bypasses tool/resource PEP. |
| Progress / cancellation | Source-backed as tool-call start/finish progress and cancellation before or during the handler boundary. Long-operation progress and resumable cancellation are target work. |
| `*/list_changed` notifications | Source-backed for MCP resource-list fingerprinting tied to the current MCP resource surface and for session-bound active lifecycle tool-surface changes. Workspace/domain change-feed backed notifications are target work through 0030 `DataChange`. |

Deferred MCP primitives (recorded so the requirement is captured, not implemented as empty surfaces):

- **Tasks (experimental).** Durable execution wrappers for deferred result retrieval and status of
  long or multi-step requests. The MCP spec marks this experimental; Loom would back task state with
  the ledger (0018), append-log/queue (0021), and durable delivery (0035). Gated on the MCP Tasks
  primitive stabilizing in the protocol before it is served.

### 9.1 Tool naming and area grouping

Tools are named `<area>.<verb>` in snake_case. The area is the lowercase facet/subsystem name; the
verb is the IDL method, with a leading token stripped only when it exactly equals the area (so
`sql_exec` becomes `sql_exec`, while `tag_create`
under the `vcs` area stays `vcs_tag_create`). The `LoomHandle` parameter is implicit - the host
supplies it from its `StoreAccess` strategy (a per-request open or a persistent handle; section 9.9),
not necessarily a single held handle; `FacetKind facet`, `workspace`, `principal`, and the remaining
arguments become the tool's JSON input schema. This is the agent-curated projection of RD10,
not a 1:1 IDL emission: binding-ergonomic interfaces are folded in or dropped, as noted below.
Workspace discovery is exposed as a read-only MCP tool. Workspace lifecycle and `ManagementKv` map
configuration are control-plane management surfaces, not ordinary MCP data tools, and are not listed
here.

| Area | IDL interface | Tools |
| --- | --- | --- |
| `store` | Store | `store_version`, `store_capabilities`, `store_capabilities_json`, `store_blob_digest`, `store_maintenance_status`, `store_maintenance_policy_set`, `store_maintenance_run` |
| `workspace` | Workspaces | `workspace_list` |
| `vcs` | VersionControl | `vcs_commit`, `vcs_branch`, `vcs_checkout`, `vcs_head_branch`, `vcs_log`, `vcs_merge`, `vcs_merge_in_progress`, `vcs_merge_conflicts`, `vcs_merge_resolve`, `vcs_merge_abort`, `vcs_merge_continue`, `vcs_status`, `vcs_stage`, `vcs_stage_all`, `vcs_unstage`, `vcs_commit_staged`, `vcs_tag_create`, `vcs_tag_list`, `vcs_tag_target`, `vcs_tag_delete`, `vcs_tag_rename`, `vcs_restore_file`, `vcs_restore_path`, `vcs_cherry_pick`, `vcs_revert`, `vcs_rebase`, `vcs_squash`, `vcs_diff`, `vcs_blame` |
| `watch` | Watch | `watch_subscribe`, `watch_poll` |
| `fs` | FileSystem | `fs_write_file`, `fs_read_file`, `fs_append_file`, `fs_remove_file`, `fs_create_directory`, `fs_remove_directory`, `fs_read_at`, `fs_stat`, `fs_list_directory`, `fs_write_at`, `fs_truncate`, `fs_symlink`, `fs_read_link` |
| `apps` | FileSystem curated app storage | `apps_list`, `apps_show`, `apps_read_file`, `apps_create`, `apps_write_file`, `apps_remove_file`, `apps_call_tool` (app-only) |
| `ask` | Document curated ask storage | `ask_questions`, `ask_answers`, `ask_record` (app-only) |
| `cas` | Cas | `cas_put`, `cas_get`, `cas_has`, `cas_delete`, `cas_list` |
| `graph` | Graph plus local query facade | `graph_upsert_node`, `graph_get_node`, `graph_remove_node`, `graph_upsert_edge`, `graph_get_edge`, `graph_remove_edge`, `graph_neighbors`, `graph_out_edges`, `graph_in_edges`, `graph_reachable`, `graph_shortest_path`, `graph_query`, `graph_explain_query` |
| `vector` | Vector | `vector_create`, `vector_upsert`, `vector_upsert_source`, `vector_get`, `vector_source_text`, `vector_embedding_model`, `vector_ids`, `vector_metadata_index_keys`, `vector_create_metadata_index`, `vector_drop_metadata_index`, `vector_delete`, `vector_search`, `vector_search_policy` |
| `columnar` | Columnar | `columnar_create`, `columnar_append`, `columnar_scan`, `columnar_columns`, `columnar_rows`, `columnar_select`, `columnar_aggregate`, `columnar_compact`, `columnar_inspect`, `columnar_source_digest` |
| `dataframe` | Dataframe | `dataframe_create`, `dataframe_collect`, `dataframe_preview`, `dataframe_materialize`, `dataframe_plan_digest`, `dataframe_source_digests` |
| `fts` | Full-text collection management | `fts_create`, `fts_index`, `fts_get`, `fts_delete`, `fts_ids`, `fts_remap`, `fts_query`, `fts_source_digest`, `fts_status` |
| `import` | Store import substrate | `import_submit_batch`, `import_execute_batch` |
| `search` | Store-wide search | `search` |
| `substrate` | Shared operation substrate | `substrate_changes`, `substrate_refs`, `substrate_alias_bind`, `substrate_alias_release`, `substrate_alias_resolve`, `substrate_alias_list`, `substrate_reference_status`, `substrate_reference_reconcile`, `substrate_history`, `substrate_revision_latest`, `substrate_revision_at`, `substrate_revision_as_of_root`, `substrate_checkpoint_before`, `substrate_transact`, `substrate_view_define`, `substrate_view_get`, `substrate_view_list`, `substrate_write_admission_policy_get`, `substrate_write_admission_policy_set` |
| `workgraph` | Workgraph lifecycle substrate | `workgraph_changes`, `workgraph_fact_put`, `workgraph_metrics` |
| `tickets` | Store-backed Studio profile | `tickets_project_create`, `tickets_project_rekey`, `tickets_project_settings_get`, `tickets_project_settings_set`, `tickets_projects`, `tickets_relations`, `tickets_fields`, `tickets_field_put`, `tickets_field_retire`, `tickets_create`, `tickets_update`, `tickets_delete`, `tickets_board_create`, `tickets_board_update`, `tickets_board_delete`, `tickets_board_configure_columns`, `tickets_board_move_card`, `tickets_get`, `tickets_list`, `tickets_board_get`, `tickets_board_list`, `tickets_history`, `tickets_relation_set`, `tickets_relation_remove`, `tickets_comments`, `tickets_comment_add`, `tickets_comment_update`, `tickets_comment_delete` |
| `lanes` | Store-backed Lane coordination state | `lanes_create`, `lanes_get`, `lanes_list`, `lanes_update`, `lanes_ticket_add`, `lanes_ticket_remove`, `lanes_ticket_transfer`, `lanes_delete` |
| `meetings` | Store-backed Studio profile | `meetings_list`, `meetings_get`, `meetings_search`, `meetings_projection_outputs`, `meetings_extraction_review`, `meetings_accept_annotation`, `meetings_reject_annotation`, `meetings_propose_vocabulary`, `meetings_accept_vocabulary`, `meetings_reject_vocabulary`, `meetings_add_entity_merge`, `meetings_add_promotion`, `meetings_promote_task_to_ticket`, `meetings_promote_decision_to_decision_log`, `meetings_promote_question_to_lifecycle`, `meetings_promote_artifact_to_reference_artifact`, `meetings_promote_reference_to_reference_artifact`, `meetings_import_snapshot` |
| `studio` | Store-backed Studio maintenance | `studio_reindex` |
| `redmine` | Store-backed Redmine import profile | `redmine_import_snapshot` |
| `spaces` | Store-backed Studio profile | `spaces_create`, `spaces_get`, `spaces_list` |
| `pages` | Store-backed Studio profile | `pages_create`, `pages_update`, `pages_publish`, `pages_get`, `pages_list`, `pages_history` |
| `lifecycles` | Store-backed Studio profile | `lifecycles_define`, `lifecycles_define_standard`, `lifecycles_definitions`, `lifecycles_definition`, `lifecycles_instantiate`, `lifecycles_instances`, `lifecycles_instance`, `lifecycles_active_set`, `lifecycles_active_clear`, `lifecycles_snapshot_plan`, `lifecycles_current_surface`, `lifecycles_transition`, `lifecycles_snapshots`, `lifecycles_snapshot`, `lifecycles_snapshot_content`, `lifecycles_operation_log` |
| `chat` | Store-backed Studio profile | `chat_channels`, `chat_create_channel`, `chat_rename_channel`, `chat_fetch_events`, `chat_messages`, `chat_cursor`, `chat_presence`, `chat_post_message`, `chat_edit_message`, `chat_redact_message`, `chat_emoji_list`, `chat_emoji_register`, `chat_emoji_unregister`, `chat_add_reaction`, `chat_remove_reaction`, `chat_create_thread`, `chat_create_task`, `chat_claim_task`, `chat_complete_task`, `chat_invoke_agent`, `chat_agent_reply`, `chat_request_handoff`, `chat_update_cursor`, `chat_set_presence` |
| `drive` | Store-backed Studio profile | `drive_list`, `drive_stat`, `drive_read`, `drive_list_versions`, `drive_create_folder`, `drive_create_upload`, `drive_upload_chunk`, `drive_commit_upload`, `drive_rename`, `drive_move`, `drive_delete`, `drive_list_conflicts`, `drive_resolve_conflict`, `drive_list_shares`, `drive_grant_share`, `drive_revoke_share`, `drive_apply_share_expiry`, `drive_list_retention`, `drive_pin_retention`, `drive_unpin_retention`, `drive_apply_retention`, `drive_acquire_lease`, `drive_refresh_lease`, `drive_release_lease`, `drive_break_lease` |
| `structures` | Store-backed Studio profile | `structures_create`, `structures_get`, `structures_list`, `structures_add_node`, `structures_update_node`, `structures_move_node`, `structures_link_node`, `structures_bind`, `structures_decompose_to_tickets` |
| `kv` | Kv | `kv_put`, `kv_get`, `kv_delete`, `kv_list`, `kv_range`, `kv_list_collections` |
| `document` | Document | `document_put_text`, `document_get_text`, `document_put_binary`, `document_get_binary`, `document_delete`, `document_list_binary`, `document_list_collections`, `document_query`, `document_replace_text` |
| `timeseries` | TimeSeries | `timeseries_put`, `timeseries_get`, `timeseries_range`, `timeseries_latest`, `timeseries_list_collections` |
| `metrics` | Metrics | `metrics_put_descriptor`, `metrics_get_descriptor`, `metrics_put_observation`, `metrics_query` |
| `logs` | Logs | `logs_put_record`, `logs_get_record`, `logs_query` |
| `traces` | Traces | `traces_put_span`, `traces_get_span`, `traces_trace_spans`, `traces_query` |
| `ledger` | Ledger | `ledger_append`, `ledger_get`, `ledger_head`, `ledger_len`, `ledger_verify`, `ledger_list_collections` |
| `queue` | Queue, QueueConsumers | `queue_append`, `queue_get`, `queue_range`, `queue_len`, `queue_list_streams`, `queue_consumer_position`, `queue_consumer_read`, `queue_consumer_advance`, `queue_consumer_reset` |
| `calendar` | Calendar | `calendar_create_collection`, `calendar_get_collection`, `calendar_list_collections`, `calendar_delete_collection`, `calendar_put_entry`, `calendar_put_ics`, `calendar_get_entry`, `calendar_delete_entry`, `calendar_list_entries`, `calendar_range`, `calendar_search`, `calendar_to_ics` |
| `contacts` | Contacts | `contacts_create_book`, `contacts_get_book`, `contacts_list_books`, `contacts_delete_book`, `contacts_put_entry`, `contacts_put_vcard`, `contacts_get_entry`, `contacts_delete_entry`, `contacts_list_entries`, `contacts_search`, `contacts_to_vcard` |
| `mail` | Mail | `mail_create_mailbox`, `mail_get_mailbox`, `mail_list_mailboxes`, `mail_delete_mailbox`, `mail_ingest_message`, `mail_get_message`, `mail_to_eml`, `mail_delete_message`, `mail_list_messages`, `mail_get_flags`, `mail_set_flags`, `mail_search` |
| `sql` | Sql | `sql_exec`, `sql_query` (read-only), `sql_commit`, `sql_read_table`, `sql_read_table_at`, `sql_index_scan`, `sql_index_scan_at`, `sql_diff`, `sql_table_diff`, `sql_blame`, `sql_list_databases` |

MCP `tools/list` returns the complete visible model-facing inventory with callable schemas. It does
not expose a separate tool-metadata search API. For model-facing response control, collection-shaped
tools use a default page of 500 items with optional `limit` and `offset` controls where ordering is
stable. Byte-heavy tools that return opaque payloads use `max_output_bytes` when exposed by the tool,
and the default delivered payload budget is 4 MiB. `resources/read` applies the same 4 MiB default to
delivered text or base64 content. `fs_list_directory` keeps the canonical CBOR
`loom.fs.dir-listing.v1` byte-array result and pages it by decoding, slicing, and re-encoding that
shape.

`watch` transport methods outside the MCP tool set are source-backed where implemented: `GET /watch:stream`
SSE, JSON-RPC `watch.stream`, and gRPC server-streaming `Watch::Stream` share the same cursor and
authorization semantics with `watch_subscribe`/`watch_poll`.

Hosted `substrate_changes` is a shared adapter over the same authorization and cursor rules, not a
separate watch implementation. Current source backs the shared adapter and the REST/JSON-RPC method
wrappers; gRPC remains target until the protobuf service is promoted. It projects the 0061
tagged-union change model without forking cursor semantics:

- `kind: "data"` events come from the existing hosted watch path and may include the 0003d LMDIFF
  envelope for non-root commits.
- `kind: "operation"` events come from profile operation logs and use `oplog:` cursors. Current
  source-backed profile cursors cover tickets, pages, and chat.
- REST exposes one adapter method for the canonical route under the workspace,
  `GET /substrate/changes?cursor=...&max=...`.
- JSON-RPC exposes one adapter method, `substrate_changes`, with `{ workspace, cursor, max }`.
- gRPC remains target until the protobuf service is promoted, but it must reuse the same adapter
  result shape.
- The hosted adapter must reject workspace-mismatched cursors with `CURSOR_INVALID`, enforce the same
  VCS read permission used by MCP, cap `max` with the hosted watch limit, and preserve the same
  `next` cursor string across transports.
- REST and JSON-RPC listener implementations must call the shared adapter rather than each transport
  rebuilding data-event or operation-event projection independently.

Hosted reference reconciliation status is likewise one shared, read-only adapter rather than a
worker-control protocol. REST and JSON-RPC expose aggregate `pending`, `resolved`, `failed`, active
target count, next due time, and unsupported-target count through the same table-scope read
authorization used by local status. Candidate aliases, evidence, retry internals, and resolver
control remain unavailable through this surface. In-process hosted conformance covers both adapter
projections; physical listener routes remain target work.

Hosted Drive has source-backed in-process REST and JSON-RPC adapter methods for the same read-only
surface exposed through MCP: `drive_list`, `drive_stat`, `drive_read`, and `drive_list_versions`.
Stdio MCP, REST, and JSON-RPC additionally source-back the first Drive write vertical:
`drive_create_folder`, `drive_create_upload`, `drive_upload_chunk`, `drive_commit_upload`,
`drive_rename`, `drive_move`, `drive_delete`, `drive_list_conflicts`, and
`drive_resolve_conflict`, all routed through the engine policy enforcement point with structured
output schemas. Stale-base new-file upload commits materialize visible conflict-copy entries;
stale-base replacement upload commits fail closed. MCP also source-backs product-native Drive lease
lifecycle tools, `drive_acquire_lease`, `drive_refresh_lease`, `drive_release_lease`, and
`drive_break_lease`, over the shared 0036 daemon lock token shape when the host is attached to a
daemon session. Lease token results and write-admission inputs use the structured fence shape
`authority`, `epoch`, and `sequence`; the attached daemon maps its local sequence as `0:0:sequence`.
Break-lease is admin-gated and removes all current holders for the Drive target without resetting
fence counters. The attached MCP lease lifecycle appends Drive operation-log records for
`lock.acquired`, `lock.refreshed`, `lock.released`, and `lock.broken`. MCP Drive write tools
additionally expose the reusable `write_admission` envelope; when present, the host validates the
live daemon token and applies the fence before mutating Drive state. The MCP substrate policy
tools `substrate_write_admission_policy_get` and `substrate_write_admission_policy_set` source-back
generic mandatory/advisory policy management, and MCP Drive writes reject missing admission when the
Drive surface scope is marked mandatory or when target-specific mandatory rows exist that the
admission-free call cannot prove it avoids. Stale file deletes are source-backed as held-delete
conflict records across MCP, REST, and JSON-RPC; the edited file remains visible until
`drive_resolve_conflict` with `keep_conflict` applies the delete. MCP stale folder deletes create
descendant survivor conflict records and leave the ancestor chain plus edited descendants visible;
resolving every survivor conflict from the same folder delete as `keep_conflict` prunes the deleted
folder root across MCP, REST, and JSON-RPC. Durable hosted listener admission for `drive/rest` and
`drive/json_rpc` is source-backed through the served-listener registry, and daemon-opened Drive REST
and JSON-RPC listeners route the current hosted Drive read/write vertical. Lease lifecycle operations
remain attached-daemon MCP tools, not REST/JSON-RPC listener routes. Hosted REST and JSON-RPC also
source-back share-to-ACL projection and retention management routes equivalent to the MCP admin
tools, including `drive_apply_share_expiry` and `drive_apply_retention`. Hosted REST and JSON-RPC
Drive committed-upload routes maintain shared `drive:file:{file_id}` revision rows through the
generic 0061 profile transaction helper; adapter tests and the in-process hosted protocol
conformance suite assert the typed route result and the reserved revision projection together.
Drive refresh and release paths that observe expired daemon locks append `lock.expired` operation
records. The daemon applies due share-expiry and retention policy for registered Drive policy targets
as an audited service principal in authenticated mode. `loom-hosted` source-backs local OS projection
primitives and daemon-opened REST/JSON-RPC routes for dehydrated marker rendering, hydrate-on-read,
marker-byte write rejection, and generic hydration/eviction worker planning. OS-native placeholder
hooks, worker scheduling, and platform hydration/eviction adapters remain target work.

Stdio MCP source-backs Meetings profile tools over stored
`MeetingsProfileSnapshot` control records: `meetings_projection_outputs`,
`meetings_extraction_review`. MCP also source-backs write-side review tools over the same snapshot:
`meetings_accept_annotation`, `meetings_reject_annotation`, `meetings_propose_vocabulary`,
`meetings_accept_vocabulary`, `meetings_reject_vocabulary`, and `meetings_add_entity_merge`. These
tools derive canonical projection-output and review records and preserve exact canonical CBOR hex
where canonical records are returned. `loom-hosted` source-backs matching REST and JSON-RPC adapter
methods over the same stored snapshots for projection, review, and review writes. The served
`meetings` surface is source-backed for REST and JSON-RPC as
`loom serve <store> meetings <workspace> --transport rest|json_rpc`, exposing
projection-output, apply-projection-output, materialized-output, and extraction-review routes. The
apply route physically materializes deterministic document, files, graph, search, SQL/dataframe, and
ledger outputs. SQL/dataframe outputs persist into SQL database `meetings/{workspace}` table
`meetings_projection_outputs`. Vector outputs persist projection-output-level Studio embedding jobs
with durable `no_engine` state until built-in embedding inference is configured. CLI
`loom studio reindex` can drain Meetings vector outputs into physical vector records when a
text-embedding instance is bound. A served Meetings listener also drains those vector outputs during
apply when the daemon can resolve a configured text-embedding binding for the workspace. Ledger
appends avoid duplicates by deterministic projection output id. The materialized-output route reads
back the concrete document/file/graph/FTS/SQL-dataframe/ledger artifacts, physical vector records, and
durable vector job records produced by apply. The `uldren-loom-protocol-conformance`
crate certifies those served routes in-process against a real store-backed snapshot.
Product-shaped Meetings list/get/search/materialized-output surfaces are source-backed. Hosted
assistant answer generation remains target work.

The served `tickets` surface is source-backed for REST and JSON-RPC as
`loom serve <store> tickets <workspace> --transport rest|json_rpc`. It exposes project create,
project re-key, project settings get/set, ticket create/update/delete, ticket get, relation
set/remove, and ticket history over the same `loom-tickets` component used by the MCP ticket vertical. The listener
collection is the ticket workspace id. The route tests exercise both transports against a real store
and verify derived keys, `expected_root`, re-key, settings, get, update, delete, and history. Ticket create
and update-field routes also maintain text-field reference edges and unresolved ticket-key candidates
through the shared `loom-tickets` helper used by MCP and hosted writes.

The served `lifecycle` surface is source-backed for REST and JSON-RPC as
`loom serve <store> lifecycle <workspace> --transport rest|json_rpc`. It exposes standard definition
creation, custom canonical-definition creation, definition reads, instance creation, instance reads,
snapshot-plan reads, current-stage surface reads, guarded transitions, snapshot reads, and
operation-log reads over the same `loom-lifecycle` component used by the MCP lifecycle vertical. The
listener collection is the Studio workspace profile id. Route tests exercise both transports against
a real store and verify definition creation, instance creation, attested transition to `draft`,
current surface reads, and `lifecycle.transitioned` operation-log records. Session-bound active
lifecycle surfacing, `tools/list_changed` recomputation, prompt registration, and stored snapshot
content readback are source-backed in the MCP lifecycle vertical. Generic trigger execution is owned
by 0015/0029; durable keeper and public trigger facade promotion are not hosted transport work.

There are two layers, and they must not be conflated:

- **Workspace-level history** is one history per workspace (a commit is a cross-facet snapshot). In
  core, `log`/`branch`/`diff`/`status` take `workspace`, not a facet. So version control is a single
  `vcs` area scoped by `workspace` (no per-facet duplication). `vcs_diff` is the structural
  cross-facet `LMDIFF` envelope between two commits, and `vcs_blame` is the path-level "which commit
  last set this entry".
- **Table-model operations** apply only to facets whose storage is a table: `read_table`,
  `index_scan`, the row-level diff (`diff_table`), and the row-level blame (`blame_table`). They are
  not usable by `kv`, `document`, `cas`, and the like. They are bound to each concrete table facet:
  `sql_read_table`, `sql_index_scan`, `sql_diff`, `sql_blame` now. Columnar exposes its promoted
  facade tools directly; row-level columnar history readers remain target work. There is no generic
  `table` area.

Names are plain verbs in every area (`kv_get`, `calendar_range`, `sql_diff`, `vcs_diff`); the area
prefix carries the layer and the tool description and parameters carry the rest, which reads most
naturally to an agent browsing `tools/list`. `vcs_diff` answers "which structural units changed in
the workspace" and `sql_diff` answers "which rows changed inside this table"; same verb, different
area, no `row_` qualifier needed.

**Rule for `FacetKind`-parameterized IDL methods.** If the operation is uniform across every facet
(version control), keep one area (`vcs`) scoped by workspace and do not duplicate it per facet. If the
operation is specific to a storage model that only some facets have, bind it to each concrete facet
area instead of a confusing generic area. SQL exposes table-model history readers today; columnar
exposes its promoted dataset facade today, while columnar row-history readers remain target work.

Per RD12 this organization is applied across the board, not only on the MCP surface: the generic
`TableReaders` IDL interface is dissolved, its readers and row diff/blame move onto the concrete `Sql`
interface with the `FacetKind` parameter dropped, its async `log`/`merge` move next to
`VersionControl`, and `VersionControl` gains workspace-level structural `diff` using the public
`LMDIFF` envelope plus a workspace-level `blame`. The C ABI, headers, all eight bindings, conformance,
and the drift test are updated to match. Columnar has its own promoted facade; table-history reader
projection for columnar is separate target work.

Interfaces deliberately not projected as tools (folded into the host or returned natively, per RD10):
Store session lifecycle (`open`/`open_keyed`/`open_with_kek`/`create`/`close`) is the host's launch
configuration, not a tool; KeySource (`key_add_wrap_*`, `key_remove_wrap`) is sensitive key/wrap
administration and is not an agent-callable tool at all - unlocking an encrypted store is handled once
via elicitation at session start (section 9.5), and wrap management stays a CLI/admin operation;
Diagnostics (`result_to_json`, `last_error`) is unnecessary because MCP returns structured JSON
content and JSON-RPC errors natively; ResultViews (cell/row decoding) is the same native-JSON concern;
FileHandle (stateful descriptor ops) is replaced by the path-based `fs.*` tools plus
`fs_read_at`/`fs_write_at`; Tasks async plumbing (`*_async`, `task_poll`/`task_result`) and served
result handles are not exposed as standalone tools. Current MCP returns results inline in structured
tool content. The shared hosted result-handle authorization substrate is source-backed, and the first
concrete served route is source-backed for prepared columnar Arrow IPC exports through
`/_loom/results/{handle}`. Resumable MCP task/result routes remain target work tied to the
experimental MCP Tasks primitive.

When served result handles are promoted, they are not reusable bearer tokens. A handle is valid only
for the authenticated principal and session family that created it, the operation that created it, and
the workspace/domain/resource scopes recorded at creation. Every `poll`, `next`, `read`, `result`,
`cancel`, or `close` request must authenticate the caller, reject a different principal, and rerun the
engine PEP against the current ACL state before returning another chunk or result. If the caller's
grant was revoked, the next handle operation fails with `PERMISSION_DENIED`. If the caller is no longer
authenticated, it fails with `AUTHENTICATION_FAILED`. If the handle expired, was closed, was already
consumed, or belongs to a different daemon instance, it fails as absent rather than leaking the prior
operation. For streamed or paged results whose next chunk could include mixed resources, the server
may narrow future chunks to rows/items still covered by the caller's current grants; if it cannot prove
the next chunk remains authorized, it must fail closed instead of returning stale material. Local C ABI
`LoomIter`, `LoomTask`, and `LoomResultView` handles are in-process binding helpers over already
authorized operations; they are not served handles and do not create a remote authorization exception.
Current source backs this rule with `ServedResultHandles`, which stores operation, session, scopes,
expiry, and route resource, reauthenticates each handle operation, reruns PEP over recorded scopes,
hides wrong-session and closed handles as not found, and removes one-shot result handles after
authorized read.

### 9.2 Policy enforcement for the whole MCP surface

The entire MCP surface goes through the engine policy enforcement point (PEP), not only tool calls:
`tools/call`, `resources/read`, `resources/subscribe`, `prompts/get`, `completion/complete`, and any
scope-elided operation each resolve a principal and pass the PEP before any state is read or written.
There is no read fast-path and no resource/prompt/completion fast-path (this strengthens RD9, which is
now explicit that reads and every other primitive are gated too).

The MCP host does not implement its own authorization; it rides the engine PEP, whose model is:

- No `IdentityStore` installed: the engine behaves as it does today - a passwordless owner with full
  read and write. A default-created loom is in this state, so both stdio and Streamable HTTP serve
  immediately with no authentication handshake.
- `IdentityStore` installed, unauthenticated root mode: ACL is bypassed until auth is enforced (the
  migration window while an operator configures identities).
- `IdentityStore` installed and auth enforced: every operation goes through `AclStore::authorize`
  against the resolved principal.

Current local MCP source resolves the principal from launch configuration: `loom mcp` accepts the
global `--auth-principal` plus `--auth-key-source` arguments, attaches to the local daemon, stores the
resolved passphrase-backed principal in `LocalOpenAuth`, and rebinds that principal on every
per-request store open. Missing or invalid launch principal authentication fails as
`AUTHENTICATION_FAILED` before authorization. A valid principal without a covering grant reaches the
engine PEP and fails as authorization. Establishing OAuth 2.1 as the HTTP authentication mechanism -
which resolves the principal that `AclStore::authorize` then checks - is separate work for remote HTTP
deployments (see RD11). Adapter-side omission of a tool from `tools/list` (or a resource from
`resources/list`) when the caller lacks rights is an ergonomic filter only; the call still reaches the
PEP and is refused there.

### 9.3 Resources, subscriptions, and list-changed notifications

Resources are read-addressed context under the `loom://<workspace>/<path>` scheme: workspace files,
CAS blobs by content address, and calendar/contacts/mail bodies materialized through the source-backed
`loom-vfs` codecs (`.ics`/`.vcf`/`.eml`). `resources/templates/list` advertises parameterized URIs
(for example `loom://{workspace}/calendar/{principal}/{collection}/{uid}.ics`). The resource version
and ETag are the content address, so a client can cache and conditionally re-read. `resources/read`
runs through the PEP exactly like a read tool.

Subscriptions use `resources/subscribe` plus `notifications/resources/updated`. Current source stores
subscribed URIs per MCP session, recomputes readable resource ETags on a short poll interval, and
emits an update when the ETag changes. MCP App resource subscriptions also keep a 0030 watch cursor for
the app workspace and record changed app resources into server-lifetime delivery streams with ack,
replay, redelivery, and retention. The Loom pull-watch baseline is source-backed separately through
read-only `watch_subscribe` and `watch_poll` tools. The target design is to replace the subscription
polling drive point with Loom-native 0030 `DataChange` payloads for branch heads, queue streams,
tables, PIM collections, and other watchable domains after each domain defines its `DomainChange`
records. `notifications/resources/list_changed` is source-backed at the MCP level by comparing the
visible `resources/list` URI set per session; workspace/domain change-feed backed list refreshes
remain target work through 0030. Daemon shutdown, attached-session loss, cancellation, progress, and
transport close remain lifecycle payloads owned by MCP and hosted protocols, not `DomainChange`
records unless an owning data domain explicitly records them as workspace data.

### 9.4 Roots (dropped)

DROPPED from the Loom MCP surface. Roots is deprecated upstream (MCP 2026-07-28 RC, SEP-2577) and its
replacement is tool parameters / resource URIs / server configuration. Loom already scopes every tool
by an explicit `workspace` argument (the handle pattern), which is exactly that replacement, so there
is nothing to build here. Scope is enforced at the PEP on the `workspace` argument, not via `roots`.

### 9.5 Elicitation

`elicitation/create` lets the server request structured input from the user mid-operation, each with a
JSON schema for the requested fields. Current source uses elicitation only for destructive-write
confirmation before the operation reaches the engine. Interactive unlock passphrase entry (a 0034 key
source, never an environment variable), principal selection when more than one identity is available,
ACL grant approval, SQL migration confirmation, and merge-conflict choices that feed
`vcs_merge_resolve` are target uses. Elicitation is paired with the PEP: a write the policy would allow
but that is flagged destructive triggers an elicitation round before commit.

### 9.6 Completion and pagination

`completion/complete` is source-backed for workspace-name prefix completion, PEP-gated through
`workspace_list`. Collection/book/mailbox ids, principal ids, ref and tag names, SQL names, RRULE
fields, and tree paths are target completions. MCP list pagination is source-backed with opaque
offset cursors over tools/resources/prompts. Prolly-position cursors for large data ranges are target
work. Both surfaces run through the PEP so completions never reveal out-of-scope names.

### 9.7 Utilities: progress, cancellation

(MCP logging is deprecated upstream - MCP 2026-07-28 RC, SEP-2577 - so it is dropped here; server
observability goes to stderr / OpenTelemetry per 0030, not the MCP `logging/*` methods.) Progress is
source-backed today as a start/finish notification around each tool call when the client supplies a
progress token. Cancellation is source-backed at the handler boundary: a cancellation request can
abort before the tool runs or race the handler and discard the late result. Long-operation progress
for clone, sync, reconcile passes, bundle import, vector builds, and large scans is target work, as is
operation-internal cooperative cancellation.

### 9.8 Prompts by area

Prompts are curated, reusable workflow templates (`prompts/list`, `prompts/get`) that orchestrate the
tools of an area. The planned set per area:

| Area | Prompt | Purpose and tools orchestrated |
| --- | --- | --- |
| calendar | `calendar_summarize_period` | Summarize events and todos in a date range (`calendar_range`). |
| calendar | `calendar_find_conflicts` | Detect overlapping or double-booked events (`calendar_range` + reasoning). |
| calendar | `calendar_schedule_event` | Propose and create an event around existing commitments and recurrence (`calendar_range`, `calendar_put_entry`, elicitation confirm). |
| calendar | `calendar_agenda` | Build a next-N-days agenda (`calendar_range`, `calendar_list_entries`). |
| contacts | `contacts_find` | Natural-language contact lookup (`contacts_search`). |
| contacts | `contacts_deduplicate` | Find and merge duplicate cards (`contacts_list_entries`, `contacts_put_entry`/`delete_entry` with confirm). |
| contacts | `contacts_enrich` | Fill missing fields on a card from context (`contacts_get_entry`, `contacts_put_entry`). |
| mail | `mail_triage` | Classify and prioritize unread mail and propose flags (`mail_list_messages`, `mail.get_body`, `mail_set_flags` with confirm). |
| mail | `mail_summarize_thread` | Summarize a conversation (`mail_search`/`list_messages`, `mail.get_body`). |
| mail | `mail_draft_reply` | Draft a reply to a message (`mail.get_body`; optional future Loom inference provider, not MCP sampling). |
| mail | `mail_find` | Natural-language search across mailboxes (`mail_search`). |
| vcs | `vcs_summarize_changes` | Summarize the diff between two refs (`vcs_status`, `table.table_diff`). |
| vcs | `vcs_explain_conflict` | Explain a merge conflict and propose a resolution (`vcs_merge_in_progress`, `vcs_status`, `vcs_merge_resolve` with confirm). |
| vcs | `vcs_blame` | Attribute changes to a file or table (`table.table_blame`). |
| vcs | `vcs_release_notes` | Generate notes from the log between two tags (`vcs_log`). |
| fs | `fs_summarize_tree` | Summarize the documents under a directory (`fs_read_file`, listing). |
| fs | `fs_find` | Locate files by name or content (listing, `fs_read_file`). |
| sql | `sql_ask` | Answer a natural-language question over a database (schema introspection, `sql_query`). |
| sql | `sql_schema_overview` | Describe tables and columns (`sql_query` over schema). |
| timeseries | `timeseries_trend` | Summarize a metric's trend over a window (`timeseries_range`, `timeseries_latest`). |
| ledger | `ledger_audit` | Verify the chain and summarize entries (`ledger_verify`, `ledger_get`). |
| queue | `queue_inspect` | Summarize backlog and consumer lag (`queue_len`, `queue_consumer_position`, `queue_range`). |
| document | `document_summarize_collection` | Summarize a document collection (`document_list_binary`, `document_get_binary`). |
| lifecycle | `lifecycle_feature_ideate` | Frame a feature idea and identify subject scope (`lifecycles_instantiate`, `pages_create`). |
| lifecycle | `lifecycle_feature_draft` | Draft the feature plan and refine page content (`pages_update`, `lifecycles_current_surface`). |
| lifecycle | `lifecycle_feature_structure` | Decompose feature scope into structure and tickets (`pages_update`, `tickets_create`). |
| lifecycle | `lifecycle_feature_ready` | Check readiness and prepare a frozen scope snapshot (`lifecycles_snapshot_plan`, `lifecycles_transition`). |
| lifecycle | `lifecycle_feature_build` | Drive build work from tickets and published pages (`tickets_update`, `pages_publish`). |
| lifecycle | `lifecycle_feature_done` | Close a completed feature against its frozen scope (`lifecycles_snapshot_content`, `lifecycles_transition`). |
| lifecycle | `lifecycle_bug_triage` | Triage a bug and identify impacted tickets or pages (`tickets_update`, `pages_create`). |
| lifecycle | `lifecycle_bug_reproduce` | Capture reproduction evidence for a bug lifecycle (`pages_create`, `lifecycles_transition`). |
| lifecycle | `lifecycle_bug_fix` | Coordinate bug fix work across tickets and notes (`tickets_update`, `pages_update`). |
| lifecycle | `lifecycle_bug_verify` | Verify bug resolution before closing scope (`lifecycles_snapshot_plan`, `lifecycles_transition`). |
| lifecycle | `lifecycle_bug_done` | Close a bug lifecycle with final evidence (`pages_publish`, `lifecycles_transition`). |
| lifecycle | `lifecycle_incident_triage` | Triage an incident and open the response scope (`tickets_create`, `chat_post_message`). |
| lifecycle | `lifecycle_incident_mitigate` | Coordinate mitigation work and team updates (`tickets_update`, `chat_post_message`). |
| lifecycle | `lifecycle_incident_resolve` | Resolve an incident and collect closure evidence (`tickets_update`, `lifecycles_transition`). |
| lifecycle | `lifecycle_incident_review` | Prepare incident review material and action items (`pages_publish`, `lifecycles_transition`). |
| lifecycle | `lifecycle_design_ideate` | Frame a design topic and initial alternatives (`pages_create`, `lifecycles_instantiate`). |
| lifecycle | `lifecycle_design_draft` | Draft a design proposal for review (`pages_update`, `lifecycles_current_surface`). |
| lifecycle | `lifecycle_design_review` | Review design tradeoffs and unresolved questions (`pages_publish`, `lifecycles_transition`). |
| lifecycle | `lifecycle_design_accepted` | Finalize an accepted design decision (`pages_publish`, `lifecycles_snapshot_plan`). |
| lifecycle | `lifecycle_archive` | Summarize an archived lifecycle instance (`lifecycles_instance`, `lifecycles_operation_log`). |
| apps | `apps_author` | Create or update a Loom MCP App (`apps_list`, `apps_create`, `apps_write_file`, `apps_show`, `resources/read`). |
| apps | `apps_inspect` | Inspect app candidates and MCP resource visibility (`apps_list`, `apps_show`, `resources/list`, `resources/read`). |
| store | `store_inventory` | Overview of the loom: workspaces, facets, and capabilities (host inventory plus `store_capabilities`). |

### 9.9 Store access modes

The host does not assume it holds a long-lived open loom; that is true only in one deployment. There
are two access modes and the host abstracts over both with a `StoreAccess` strategy that tools,
resources, and prompts are written against (never against a captured handle):

- **Per-request open (default, stateless).** Each request opens the loom, runs, saves if it mutated,
  and closes - the same model as the path-based language bindings and the CLI. This is the natural mode
  for a local stdio deployment launched per use. It gives a fresh consistent snapshot per request and
  the simplest correctness, at the cost of an open per request.
- **Persistent handle (server mode).** A long-lived process opens the loom once and serves many
  requests and clients, typically over Streamable HTTP on a port or URL. It avoids the per-request
  open cost and enables in-memory change watches, but it requires the single-writer discipline and the
  0036 locking layer, and coordination with any FUSE/NFS mount over the same `.loom`.

Implementation impact of supporting both:

- Encrypted stores cannot re-derive a key from nothing on each reopen, and environment variables are
  never a key source. The passphrase or KEK is obtained once - from launch configuration or a one-time
  elicitation at session start (section 9.5) - and held in host process memory for subsequent reopens.
- Subscriptions (section 9.3) must not depend on an in-memory watch, because per-request mode has no
  long-lived state between calls. Change detection is therefore sync-token / ETag based: the host
  stores the last-seen token per subscription, and on the next interaction (or a server-mode watch)
  reopens and compares. The calendar changes-since machinery already works this way, so the same code
  path serves both modes.
- Concurrency differs by mode: per-request stdio is effectively a serialized single client, while
  server mode must serialize writers and coordinate with 0036.
- The PEP/identity resolution (section 9.2) is loaded with the handle in server mode and resolved per
  open in per-request mode; section 9.2 holds either way.

Authority follows RD11: a default passwordless loom serves both transports with full read and write in
owner mode through the PEP; OAuth 2.1 and authenticated-principal resolution are separate, later work
triggered when the loom's auth model is active. The served tool, resource, and prompt schemas are
hand-written and agent-curated per RD10, not generated from `idl/loom.idl`.

### 9.10 Scoped invocation and parameter elision

The host is launched scoped to a target, and the depth of that scope determines how much an assistant
has to reason about on every call. Each value in the addressing chain is a different concern with a
different natural binding time: the loom file is a connection identity (like a database DSN), the
workspace is the isolation boundary (cross-workspace queries are intentionally unsupported, so the
workspace is effectively the connection scope), and the collection is a per-call query parameter. The
CLI binds the first two at launch and leaves the collection to the call, with an optional third level
for the fully-scoped case:

- `loom mcp <file> <workspace> <collection>` - collection-scoped (deepest).
- `loom mcp <file> <workspace>` - workspace-scoped (the recommended default).
- `loom mcp <file>` - file-scoped; an optional `default_workspace` in launch config makes it behave
  like the workspace-scoped level for a single-workspace loom.

There is no zero-argument `loom mcp`: the file is a connection, never something the assistant discovers
by roaming the filesystem (that would break the bounded-host model and the PEP scoping). The file
always comes from launch configuration.

The assistant can discover the values that remain unbound: workspaces are enumerable (section 9.3
resources, plus `workspace` argument completion in section 9.6), and collections are enumerable per
facet (a list-collections read). It does not, and must not, discover the file.

Each bound positional drives two elision mechanisms over the served surface, applied as a
post-construction pass over the registered tools (the same pass that sets tool metadata):

- **Parameter elision.** The bound value is removed from every tool's input JSON-schema, from
  completion candidates, and from the `loom://` resource URIs (which are re-rooted at the bound
  prefix). The host injects the bound value server-side in `call_tool` before dispatch, so the engine
  facade still receives the full `workspace`/collection it expects and the PEP (section 9.2) is
  unchanged. A scoped tool is therefore strictly narrower at the wire, never a different code path.
- **Collection-scope elision.** Binding a workspace does not filter tools, prompts, or resources by
  facet. Binding a collection removes the collection-axis argument from collection-addressed tools and
  drops collection discovery tools, because the collection name is already supplied by launch config.

The collection axis is one concept with per-facet names. The second addressing level is:

| Facet | Collection axis | Notes |
| --- | --- | --- |
| `cas` | (none) | workspace-scoped; item is the content `digest` |
| `fs` | (none) | workspace-scoped; item is `path` |
| `vcs` | (none) | workspace-scoped; operates on the workspace history/tree |
| `kv` | `collection` | renamed from `name` |
| `document` | `collection` | renamed from `name` |
| `time-series` | `collection` | renamed from `name` |
| `ledger` | `collection` | renamed from `name` |
| `queue` | `stream` | domain term kept |
| `sql` | `db` (database) | three-level: workspace -> db -> table -> row |
| `calendar` | `collection` | `principal` is implicit (see below) |
| `contacts` | `book` | `principal` is implicit |
| `mail` | `mailbox` | `principal` is implicit |

Rules at the collection level:

- A bound collection is applied to every facet in the workspace that has a collection of that name; it
  is not rejected when more than one facet matches (the assistant sees the union of those facets'
  collection-scoped tools). Facets with no collection (`cas`, `fs`, `vcs`) stay visible and
  workspace-scoped at the collection level.
- `principal` is never an assistant-supplied argument for `calendar`/`contacts`/`mail`. The host binds
  it to the calling principal (the owner in passwordless mode), the PEP denies access to any other
  principal's subtree (`.loom/facets/<facet>/{principal}/*`), and the parameter is elided from those
  tools. With `principal` implicit, those facets are single-collection-axis like the rest. Sharing a
  calendar/inbox/address book across principals is an open question deferred to P2+ (see 0037, 0038,
  0039).

Net effect: at the recommended default the assistant never sees or reasons about `workspace`; at the
collection level a tool such as `kv_get` is reduced to "give me a key". Narrowing the surface this way
is a reliability multiplier for assistants, and it matches how MCP servers are configured in practice
(one entry per scoped workspace). Multiple scoped configurations over the same file are expected and
are how a user maps out the combinations they want.

## Resolved Decisions

- **RD1 - Store and workspace addressing.** A hosted listener serves one `.loom` store. Canonical REST
  paths use `/v1/workspaces/{workspace_id}/...`, where `{workspace_id}` is the workspace UUID assigned at
  creation. Filenames, deployment aliases, and workspace names are mutable lookup metadata, not
  canonical path identity.
- **RD2 - Object ETag under transforms.** `Loom-Object-Digest` is always the plaintext canonical
  object digest. HTTP `ETag` validates the selected wire representation, so transformed responses use
  representation-specific strong ETags and `Vary: Accept-Encoding` when content negotiation selects a
  representation.
- **RD3 - Idempotency-Key retention.** Scope per `(store_id, principal_id)`, retain for a bounded TTL,
  and bind each key to a request fingerprint. A matching replay returns the original result, a
  mismatching fingerprint returns `422` with `INVALID_ARGUMENT`, and an in-flight duplicate returns
  `409` with `CONFLICT`.
- **RD4 - Unauthenticated vs. unauthorized.** Missing or invalid credentials return `401` with
  `AUTHENTICATION_FAILED`. Unauthorized access outside the caller's scope returns `404` masked as
  `NOT_FOUND`, so existence cannot be probed. `403` with `PERMISSION_DENIED` is used only where the
  caller can already see the resource but the specific action is denied.
- **RD5 - Client-aborted HTTP status.** Keep `499` for client-aborted requests as an informational
  operator convention. `408` remains a real timeout and must not be reused for cancellation.
- **RD6 - Sync resumption.** Use bounded session TTL with a restart-from-cursor fallback. Expiry
  surfaces as `NOT_FOUND` on the session. Re-fetch is safe because objects are content-addressed.
- **RD7 - Object-stream encoding.** Default to length-prefixed binary on `/sync/fetch`. Serve NDJSON
  only on explicit `Accept` to avoid base64 inflation on the bulk-transfer hot path.
- **RD8 - gRPC error-detail typing.** gRPC errors use the fixed Loom-code to canonical-status mapping
  in this spec and include `LoomError` plus `google.rpc.ErrorInfo` in `Status.details`.
- **RD9 - Served write authority.** REST, JSON-RPC, gRPC, MCP, FUSE, sync push, and later protocol
  adapters attach a principal context and delegate authorization to the engine policy enforcement
  point. Adapter-side hiding of write commands is an ergonomic filter, not the security boundary.

- **RD10 - Served-protocol projection strategy.** `idl/loom.idl` is the contract for the programming
  libraries (C ABI and language bindings); it is not required to be the literal shape of the
  agent-facing or human-facing served protocols. All served protocols (MCP, REST, gRPC) are
  **hand-written and curated for their audience**, and every one of them is guarded by a **mandatory
  drift/coverage conformance layer** that is the real anti-drift boundary: it asserts (1) coverage -
  every facet operation reachable through the IDL is reachable through each served protocol - and (2)
  schema stability - each protocol's request/response shapes match a checked-in golden contract, so an
  unintended change fails CI. Rationale for choosing hand-written-plus-enforcement over a codegen
  prerequisite: the conformance layer is lighter and more flexible than an IDL-to-protocol compiler,
  it lets each surface be shaped for its audience (agent-curated MCP tools and prompts, resource-
  oriented REST, streaming gRPC) instead of a lowest-common-denominator emission, and it removes the
  redo risk just as effectively because the golden contract pins every surface. A true IDL-codegen
  track remains an **optional future consolidation** (it could later subsume the hand-written bindings
  and protocols), but it is explicitly **not a prerequisite** for REST or gRPC. Language bindings keep
  their existing hand-written-plus-drift-test approach under the same principle.

- **RD11 - MCP transport and authentication scope.** Every tool call (read and write) passes through
  the engine PEP; there is no unauthenticated bypass and no read fast-path (see section 9.2). A
  default-created loom has no principal store and a passwordless owner, so the PEP resolves the caller
  to that owner and grants full read and write with no authentication handshake; both stdio and
  Streamable HTTP/SSE serve immediately in that mode, and the operator is responsible for
  network-confining an unauthenticated HTTP endpoint. Current local MCP can also be launched with a
  passphrase-authenticated principal through the global CLI auth flags; that principal is rebound on
  each daemon-backed per-request open before the PEP runs. OAuth 2.1, remote bearer/session
  credentials, and cross-protocol hosted principal resolution remain separate target work for remote
  HTTP deployments. The auth model, once active, is enforced for every call.

- **RD12 - Facet-scoped table operations (across the board).** Version control is workspace-level (one
  history per workspace; a commit is a cross-facet snapshot), and table-model operations
  (`read_table`, `index_scan`, row diff, row blame) apply only to facets whose storage is a table.
  This organization is applied consistently to the library surface, not just the MCP surface. The
  generic `TableReaders` IDL interface is dissolved: its readers and row diff/blame move onto the
  concrete table-facet interfaces (`Sql` now; a `Columnar` interface when 0023 is promoted) with the
  `FacetKind` parameter dropped; its async `log`/`merge` move next to `VersionControl`. `VersionControl`
  gains a workspace-level structural `diff` (the public `LMDIFF` envelope) and a new workspace-level
  `blame` (path-level "which commit last set this entry", the
  symmetric counterpart of the row-level `blame_table`). The C ABI, headers, all eight bindings,
  conformance, and the binding drift test are updated to match. Tool/method names are plain verbs in
  each area (`sql_diff`, `vcs_diff`, `sql_blame`, `vcs_blame`); the area scopes the layer and the
  description carries the detail - no `row_` qualifier.

## Unfinished Work

- Keep `idl/loom.idl`, `include/loom.h`, bindings, and promoted facades reconciled before generating
  hosted protocol artifacts.
- Build the served-protocol drift/coverage conformance layer (per RD10): coverage of every facet
  operation plus golden-contract schema stability, applied to MCP, REST, and gRPC. This is the
  anti-drift boundary in place of an IDL-codegen prerequisite.
- Implement the `loom-mcp` host (hand-written, agent-curated per RD10): tools (section 9.1), resources
  and templates, subscriptions, elicitation, completion, pagination, prompts (section 9.8), and the
  progress/cancellation utilities, over stdio and Streamable HTTP, with every call through the PEP per
  RD11. Roots, sampling, and logging are dropped because they are deprecated upstream. Tasks remain
  target work per section 9.
- Implement hosted REST, JSON-RPC, gRPC, FUSE, and sync endpoints (hand-written, behind the same
  drift/coverage layer).
- Implement hosted CalDAV, CardDAV, IMAP, and JMAP projections only after TLS/deployment identity,
  0026-0028 principal/authz, served write authority, protocol error mapping, and protocol conformance
  are ready.
- Optionally, later, stand up an IDL-codegen track to consolidate the hand-written bindings and
  protocols. Not a prerequisite for any served protocol (RD10).
- Add protocol conformance for stable errors, auth, streaming, sync, idempotency, caching, and binding
  parity.
- Promote served write paths only after 0026-0028 define principal context and policy enforcement.
- Keep bundle endpoints aligned with the source-backed `Bundle` format and 0006 sync rules.

## Change log

- 2026-06-29 (MCP scope-honesty pass): Reconciled section 9 with current source and 0043. Roots are
  dropped, not served; subscriptions/list-changed are source-backed only as MCP resource mechanics with
  per-session ETag polling; completion is workspace-prefix only; progress/cancellation are tool-call
  boundary mechanics; served result handles remain target work tied to MCP Tasks; capability and
  conformance resources remain target work.

- 2026-06-28 (task 227, 217d): Decomposed the loom-cli binary with **no behavior change** - `main.rs`
  went from 3,699 lines to 739. `main()`, the `Cli` parser, the `run()` dispatch, and the key-source
  helpers stay in `main.rs`; the rest moved into five modules: `cli` (the clap `Command` + sub-command
  enums), `daemon_cmd` (`run_daemon`/`run_mcp`/`run_lock` + the daemon runtime internals), `management_cmd`
  (`run_management` + workspace/identity/acl/kv handlers), `table_cmd` (`run_table` + blame/diff output),
  and `helpers` (the ~50 shared utilities + the test suite). Every moved item is `pub(crate)` and each
  module is `pub(crate) use`-re-exported from the crate root, so `run()` and the modules reach everything
  via crate-root paths + `use super::*`. Verified lossless (line multiset unchanged) with no cross-module
  private symbols, fields, or methods left dangling.

- 2026-06-28 (task 224, 217a): Decomposed the loom-ffi C-ABI crate with **no contract change** - the
  same 207 `extern "C"` symbols are exported, just relocated. `lib.rs` went from 12,933 lines to 949
  (the crate spine: version/blob/string helpers, the error contract + result rendering, `derive_sql_ns_id`,
  streaming iterators `LoomIter`/`loom_sql_query`, the cooperative task machinery `LoomTask`/`spawn_task`/
  `loom_task_*`, and the async forms of the direct readers). Extracted, each lossless (byte-identical
  bodies): `macros.rs` (the shared `handle_ref!`/`handle_mut!`/`arg_str!`, now crate-wide via
  `#[macro_use]` instead of mid-file textual scope); `sql_session.rs` (`LoomSqlSession` + `LoomSqlBatch`
  sync verbs); `direct.rs` (the `LoomSession` handle, version-control verbs, table inspection, and the
  shared engine-helper layer the per-facet modules delegate to); plus `result_render.rs` (the
  `LoomValue`/`LoomResultView` + `loom_result_*` decode surface, renamed from `result_view` to avoid the
  clash with `loom_sql::result_view`) and the 13 per-facet modules. Each module uses `use super::*` to
  pull crate-root types/helpers; the ~60 helpers shared across modules (engine `*_ns` ops,
  `open_h_read`/`open_h_write`, `resolve_workspace_arg`, `random_workspace_id`, `json_string`,
  `passphrase_arg`/`kek_arg`, `exec_session`/`load_store_read`, ...) are `pub(crate)` and re-imported at
  the crate root so descendants see them via `use super::*`. The ABI/header (`include/loom.h`) and every
  binding are untouched.

- 2026-06-28 (task 223): CLI exposure of the HTTP transport (resolves the follow-on noted in task 221).
  `loom mcp <store>` now takes `--http <addr>` to serve over Streamable HTTP (mounted at `POST /mcp`)
  instead of stdio, and `--stateless` to select MCP stateless mode (POST-only; no subscription push or
  progress streams) under `--http`. The HTTP path runs on a multi-threaded, IO-enabled tokio runtime;
  stdio keeps the current-thread runtime. loom-cli's `mcp` feature now pulls `uldren-loom-mcp/http`
  (axum + streamable-http) and the tokio `net` / `rt-multi-thread` features. (loom-cli `main.rs`.)

- 2026-06-28 (task 221): Stateless Streamable HTTP transport. `http_service`/`serve_http` take a
  `stateful` flag; `false` sets rmcp's `StreamableHttpServerConfig.stateful_mode = false` - MCP
  stateless mode: POST-only (no GET/DELETE), a fresh `LoomServer` built per request via the service
  factory, no session and no SSE resume. This is the load-balanced / serverless fan-out shape: any node
  serves any request with no shared in-process state. It composes with `StoreAccess::PerRequest` (open /
  run / save-if-mutated / close) and the local coordinator daemon (`loom-store::daemon`:
  `session_attach`, `lock_acquire`/`lock_refresh`/`lock_release`, lease/fencing tokens), which supplies
  the cross-process single-writer discipline that a stateless multi-node deployment needs in place of a
  held handle. Degradation in stateless mode (documented, by construction): no subscription push and no
  progress/cancellation streams (both need a live session), and no resumable session id. Each request
  still completes its tool/resource/prompt call normally through the engine PEP. `mcp-host` stays
  source-backed; no wire/IDL change. CLI exposure of the HTTP transport (stateful or stateless) remains
  follow-on (the `loom mcp` command currently wires stdio only). (loom-mcp `server.rs`.)

- 2026-06-28 (task 220): HTTP subscription push. Resource subscriptions now push
  `notifications/resources/updated` identically over stdio and Streamable HTTP. The poll loop
  (`subscription_poll_loop`) is spawned from the transport-agnostic `ServerHandler::on_initialized`
  hook using the session `Peer`, so each session - a stdio pipe or an HTTP SSE channel - gets its own
  loop; the previous manual spawn in `serve_stdio` was removed. The loop self-terminates on
  `Peer::is_transport_closed`, so short-lived HTTP sessions do not leak the task. No wire/IDL change;
  `mcp-host` stays source-backed. (loom-mcp `server.rs`.)

- 2026-06-28 (task 216 build): Propagated the `name` -> `collection` rename for the kv/document/
  time-series/ledger facets out of loom-core (task 206) into the full external surface. The C ABI
  (loom-ffi `loom_kv_*`/`loom_doc_*`/`loom_ts_*`/`loom_ledger_*` plus `loom_management_kv_*`),
  `include/loom.h` (+ the byte-identical iOS copy), and `idl/loom.idl` (interfaces Kv, ManagementKv,
  Document, TimeSeries, Ledger) now name the collection-axis argument `collection`. All eight language
  bindings followed: Node (napi `src/lib.rs` + `index.d.ts`), Python (pyo3 `src/lib.rs` + `__init__.pyi`),
  WASM (`src/lib.rs`), C++ (`loom.hpp`), iOS/Swift (`Loom.swift`), JVM (`Loom.java`), Android (Kotlin
  commonMain expect + jvm/android actuals + native externals, JNI C entrypoints), and React Native (TS
  `index.ts` + `NativeUldrenLoom.ts`, ObjC `UldrenLoom.mm`, Android JNI `UldrenLoom.cpp` + Kotlin
  module). Domain-specific axis names are unchanged: SQL keeps `db` (3-level workspace->db->table),
  Queue keeps `stream`, Contacts `book`, Mail `mailbox`, Calendar already used `collection`. The
  workspace selector (`workspace`, a UUID or unique name), `workspace_create`, ACL grant/revoke, and
  tag names are untouched -- those `name` tokens denote workspaces/tags, not collections. The wasm-pack
  `pkg/` glue and napi `index.d.ts` are regenerated by `just test-bindings`. Verified via `just test-bindings`.

- 2026-06-27 (P-mcp-planning): Added section 9 (MCP Serving Surface) recording the served MCP primitives
  (tools, resources + templates, subscriptions, roots, elicitation, completion, pagination, prompts,
  logging/progress/cancellation, list-changed notifications) over the `loom-mcp` host (rmcp/tokio),
  with Sampling (gated on 0040 GraphRAG / 0015 program facet) and Tasks (experimental) explicitly
  deferred. Added RD10 (served-protocol projection strategy: MCP hand-written and agent-curated; REST
  and gRPC gated on a true IDL-codegen prerequisite track to avoid a hand-write-then-regenerate redo;
  bindings consolidated by that same track) and RD11 (a passwordless default loom serves stdio and
  Streamable HTTP in owner mode with full read/write now; OAuth 2.1 enforcement is a phased follow-on
  triggered when a loom configures authentication). Updated section 2 and Unfinished Work accordingly.

- 2026-06-27 (P-mcp-planning revision 2): Expanded section 9 with 9.1 (tool naming `<area>.<verb>` and
  the full IDL-to-tool area mapping, plus the interfaces deliberately not projected), 9.2 (every tool
  - read and write - through the engine PEP; passwordless default loom needs no auth; OAuth 2.1 is
  separate auth-model-triggered work), 9.3 (resources, subscriptions, list-changed), 9.4 (roots as a
  PEP scoping input), 9.5 (elicitation: unlock passphrase, destructive-write confirm, principal
  select, merge resolve), 9.6 (completion + cursor pagination over prolly ranges), 9.7 (logging,
  progress, cancellation), and 9.8 (curated prompts per area). Revised RD10: MCP, REST, and gRPC are
  all hand-written and curated, guarded by a mandatory drift/coverage conformance layer (coverage +
  golden-contract schema stability); a true IDL-codegen track is an optional future consolidation, not
  a prerequisite. Refined RD11: all tool calls pass the PEP with no read fast-path; OAuth 2.1 is
  separate work triggered when the auth model is active. Updated section 2, Unfinished Work, and the
  1000-Deferred entries accordingly.

- 2026-06-27 (P-mcp-planning revision 3): Broadened section 9.2 - the whole MCP surface (tools,
  resources read/subscribe, prompts, completion, roots-scoped ops) goes through the engine PEP, and
  aligned it to the actual PEP model (no IdentityStore = passwordless owner; IdentityStore installed +
  unauthenticated root mode bypasses ACL until auth is enforced; enforced auth goes through
  AclStore::authorize). Removed the `key` area from the tool surface (KeySource is sensitive
  key/wrap admin, not agent-callable; unlock is one-time elicitation at session start). Folded the
  TableReaders direct/history readers into the `sql` area (sql.read_table/index_scan/blame/diff) and
  mapped its async log/merge to vcs.*, an MCP-surface grouping per RD10 that leaves the IDL/bindings
  unchanged (Sql + TableReaders interfaces stay; TableReaders stays generic over FacetKind). Added
  section 9.9 (Store access modes): per-request open (default, stateless, the binding/CLI model) vs a
  persistent handle (server mode), with the StoreAccess abstraction, key material held in host memory
  (never env), sync-token-based subscriptions that work statelessly, and 0036 locking only in server
  mode.

- 2026-06-27 (P-mcp-planning revision 4): Regrouped the TableReaders methods in section 9.1 by what each
  is. `read_table`/`index_scan` are tabular-storage-model readers (need a schema; `index_scan`
  needs a declared secondary index) and are NOT usable by kv/document/cas; they get a distinct
  `table` area, not `sql`. `table_blame`/`table_diff` move to `vcs_blame`/`vcs_diff` as
  facet-dispatched history ops (row-granular for tabular, tree/file-level for files), mirroring how
  `vcs_merge` dispatches by facet; `log`/`merge` were already in `vcs` (TableReaders only had
  their async forms, which are not separate tools). Noted the core/IDL cleanup this depends on: the
  generic commit diff (Loom::diff) exists but is not in the IDL (only table_diff is), and `blame`
  exists only as row-level blame_table today, so file-level blame is future work behind the same
  facet-dispatched vcs.blame. Reverted the earlier fold of table readers under `sql`.

- 2026-06-27 (P-mcp-planning revision 5): Simplified and corrected the table/vcs grouping after checking
  core. Version control is workspace-level (core log/branch/diff/status take `workspace`, not a
  facet; a commit is a cross-facet snapshot), so `vcs` is one workspace-scoped area with no per-facet
  duplication and `vcs_diff` is now the structural cross-facet diff envelope. The two "diffs" are
  different layers, not versions of one thing: `vcs_diff` = which structural units changed in the
  workspace; `sql.row_diff` = which rows changed inside a table (diff_table -> Vec<RowDiff>). Removed
  the generic `table` area and the earlier `vcs_blame`. Established the rule for
  FacetKind-parameterized methods: uniform-across-all-facets ops stay in one workspace-scoped area
  (vcs); storage-model-specific ops (table readers, row diff, row blame) bind to each concrete table
  facet (sql.read_table/index_scan/row_diff/row_blame; columnar.* when 0023 is promoted). index_scan
  needs a schema + declared index and is not usable by kv/document/cas. Only core/IDL cleanup needed:
  project the existing workspace-level Loom::diff into the IDL (#195); diff_table/blame_table already
  exist.

- 2026-06-27 (P-mcp-planning revision 6): Added RD12 - facet-scoped table operations applied across the
  board (IDL/C ABI/8 bindings), not just the MCP surface: dissolve the generic TableReaders interface;
  move read_table/index_scan/diff/blame onto the concrete Sql interface (FacetKind dropped; Columnar
  later); move async log/merge next to VersionControl; add workspace-level VersionControl.diff
  (Loom::diff) and a new VersionControl.blame; update conformance + drift. Finalized plain-verb naming
  (sql.diff/sql.blame, vcs.diff/vcs.blame; no row_ qualifier) - the area scopes the layer and the tool
  description carries the rest. Added vcs.blame to the vcs area (the symmetric workspace-level
  counterpart of the row-level blame). Tasks: #195 (core diff/blame), #196 (across-the-board
  IDL/ABI/binding restructure), both prerequisites of S2 (#181).

- 2026-06-28 (P-mcp S1): Scaffolded crates/loom-mcp (workspace member). The published rmcp is 0.16 (not
  1.8); pinned rmcp 0.16 + tokio behind a default-off `server` feature so the engine surface builds
  without an async runtime. Implemented StoreAccess (per-request open + persistent handle, section
  9.9; key held in memory, never env) and the LoomMcp facade (version + check_open through the PEP);
  4 unit tests green. The `server` feature adds the rmcp stdio host (ServerHandler + tool_router +
  tool_handler with a store_version tool). The user-facing launch surface is `loom mcp`; there is no
  standalone binary target. PEP readiness confirmed:
  loom_core::identity + loom_core::acl + PEP hooks exist; passwordless default = owner full access.

- 2026-06-28 (P-mcp S2): Added crates/loom-mcp/src/tools.rs, the curated tool surface as a static
  `TOOL_SURFACE` (ToolSpec: name, area, idl_interface, idl_method, ToolKind read/write) across the 13
  live areas of section 9.1, with workspace lifecycle held back as management/control-plane surface
  and the columnar area held back until 0023 is promoted.
  Dropped the proposed `mail_to_eml`: the mail facet stores raw RFC 5322, so `mail_get_message`
  already returns the .eml bytes - unlike calendar.to_ics/contacts.to_vcard, which serialize stored
  structured records, there is nothing for to_eml to do. Encoded the deliberate folds as
  `EXCLUDED` (Store lifecycle, SQL sessions/auth/batches, the `*_async` forms) and `FULLY_FOLDED`
  (KeySource, Workspaces, ManagementKv, FileHandle, Diagnostics, Tasks, ResultViews). Built the RD10 drift + coverage layer as
  tests: source `TOOL_SURFACE` names equal the section 9.1 table (drift); every tool's idl_method is a
  real method on its interface and every projected interface's methods minus EXCLUDED equal the
  projected tools (coverage), parsing idl/loom.idl directly so a new IDL method that is neither
  projected nor excluded fails the build; plus read/write partition and unique-name checks. 9 unit
  tests green, clippy clean, `--features server` still builds. The read_tools()/write_tools()
  partition feeds S3/S4.

- 2026-06-28 (P-mcp S3): Added crates/loom-mcp/src/reads.rs, the read-tool engine facade. Every read
  tool in section 9.1 is a method routed through `StoreAccess::read`; because each core read fn opens
  with `loom.authorize(ns, facet, AclRight::Read)`, the policy enforcement point is crossed on every
  call with no read fast-path (section 9.2). Covers store (capabilities, blob_digest), workspace
  (list, get), cas, document, timeseries, ledger, kv, fs, mail (get_message structured + to_eml raw),
  queue (get/range/len/consumer_position/consumer_read), vcs (log/status/merge_in_progress/
  merge_conflicts/tag_list/tag_target/diff/blame), and the sql table-model readers
  (read_table/index_scan/blame/diff via loom-sql result_cbor). Returns native Rust values so the layer
  is unit-testable on the default build (20 mcp tests, clippy clean); the rmcp `#[tool]` wire
  registration is tracked separately since its `Parameters`/schemars/JSON plumbing is shared with the
  S4 write tools. Also locked the mail decision: core/IDL/ABI `get_body` -> `to_eml` (it already
  returned the full raw `.eml`; `get_message` stays the structured record), propagated through core,
  C ABI, loom.h, loom-vfs, conformance, and specs 0008/0039; the 8-binding rename is task #199.

- 2026-06-28 (task 183): Added crates/loom-mcp/src/writes.rs, the complete write-tool engine facade
  routed through `StoreAccess::write` (open/mutate/save), so every call crosses the PEP with write
  authority - each core write fn opens with `loom.authorize(ns, facet, AclRight::Write)` (commit/advance
  verbs also take `AclRight::Advance`). Covers every section 9.1 write tool: cas (put/delete), kv
  (put/delete), document (put/delete), timeseries (put), ledger (append), queue (append/
  consumer_advance/consumer_reset), fs (write_file/append_file/remove_file/write_at/truncate/symlink),
  vcs (commit/branch/checkout/merge/merge_resolve/merge_abort/merge_continue/stage/stage_all/unstage/
  commit_staged/tag_create/tag_delete/tag_rename/restore_file/restore_path/cherry_pick/revert/rebase/
  squash), workspace (create/rename/delete), calendar (create_collection/delete_collection/put_entry/
  delete_entry), contacts (create_book/delete_book/put_entry/delete_entry), mail (create_mailbox/
  delete_mailbox/ingest_message/delete_message/set_flags), and sql (exec/commit). `sql_exec`/`sql_commit`
  replicate the C ABI per-op session (a lock-free read snapshot runs statements; the write lock is taken
  to flush only when dirty; a transaction must open and close within one exec) and require per-request
  store access. 28 mcp tests pass (writes persist and read back through a fresh per-request open), clippy
  clean. The rmcp `#[tool]` registration of reads+writes onto the wire is task #200.

- 2026-06-28 (MCP spec staleness audit, verified against modelcontextprotocol.io): the planning here
  targeted spec 2025-06-18 / rmcp 0.16; the current line is 2025-11-25 with a 2026-07-28 release
  candidate (final 2026-07-28). Material changes for this track: (1) Roots, Sampling, and Logging are
  DEPRECATED (SEP-2577) and are DROPPED from the Loom MCP surface rather than built: roots (9.4 dropped;
  the per-tool `workspace` argument is its replacement; task 186 deleted), sampling (removed from the
  client-primitive plan), logging (task 189 narrowed to progress + cancellation; observability goes to
  stderr / OpenTelemetry per 0030). (2) Elicitation (task 187) stays on the model the linked rmcp SDK
  implements; the future Multi Round-Trip replacement is not actionable against rmcp 0.16 and is not
  tracked here. (3) Stateless core (SEP-2575/2567): the
  `initialize` handshake and `Mcp-Session-Id` are gone; client info/version/capabilities ride in
  `_meta` per request, `server/discover` fetches capabilities, Streamable HTTP requires `Mcp-Method`/
  `Mcp-Name` headers and list/read carry `ttlMs`/`cacheScope` - S11/190 must target the stateless
  transport. (4) Tasks graduates to an extension and `tasks/list` is removed; MCP Apps and an
  Extensions framework are now first-class. (5) Tool schemas move to full JSON Schema 2020-12; the
  missing-resource error becomes `-32602`. rmcp 0.16 implements the pre-stateless model, so the current
  build is valid today; these notes keep the remaining slices off deprecated primitives.

- 2026-06-28 (task 200 follow-up, tool metadata): enriched every registered tool in server.rs via
  `enrich_metadata` (driven by `tools::TOOL_SURFACE`): a short `title` set identically on `Tool.title`
  and `annotations.title` (e.g. "SQL: read table"), `read_only_hint` from the surface kind,
  `open_world_hint=false` (closed local engine) everywhere, and on write tools `destructive_hint` /
  `idempotent_hint` from a verb classifier; plus `_meta.category` (KV, SQL, Calendar, ...). Two tests
  assert the metadata is populated and self-consistent. Workspace lifecycle and KV map config remain
  excluded pending a deliberate MCP management-tool projection.

- 2026-06-28 (task 191): Added crates/loom-mcp/src/prompts.rs, the curated prompt surface as a static
  `PROMPT_SURFACE` (PromptSpec: name, area, summary) of the 24 section 9.8 prompts, with a drift test
  asserting the source list equals the section 9.8 table (parallels the tool surface in task 181). The
  rmcp `#[prompt]` wire registration (24 `#[prompt]` handlers returning templated `GetPromptResult`,
  plus a `#[prompt_handler]` alongside the existing `#[tool_handler]`) is the wiring follow-on, mirroring
  how task 200 wired the tool surface from 181.

- 2026-06-28 (task 184): Resources + templates. Added crates/loom-mcp/src/resources.rs - the `loom://`
  URI scheme parser and the template catalog (`loom://{workspace}/files/{path}`, `.../cas/{digest}`,
  and the calendar/contacts/mail `.ics`/`.vcf`/`.eml` bodies), plus a dependency-free base64 encoder for
  blob contents. server.rs implements the three rmcp `ServerHandler` resource methods:
  `list_resource_templates` (the catalog), `list_resources` (one entry per workspace), and
  `read_resource` (parse the URI, dispatch to the read facade through the PEP, return text/blob contents
  with the content address as the `_meta.version`/ETag). Added a `get_info` advertising the tools,
  prompts, and resources capabilities. 40 mcp tests pass (URI parse matrix, base64 vectors, and a
  seeded read_target round-trip for files + CAS incl. not-found), clippy clean on default/server/http.
  Subscriptions + `*/list_changed` are task 185.

- 2026-06-28 (task 185): Resource subscriptions + list-changed. `get_info` now advertises
  `resources.subscribe` and `resources.listChanged` (via `.enable_resources_subscribe()` /
  `.enable_resources_list_changed()`). server.rs adds a `subscriptions: Arc<Mutex<HashMap<uri,
  Option<etag>>>>` registry and the `subscribe`/`unsubscribe` `ServerHandler` methods (record/forget a
  URI with its last-seen content address). Change detection is ETag-based: `resource_etag(uri)` resolves
  the current content address through the read facade (so the poll still crosses the PEP),
  `compute_changed()` returns the URIs whose stored ETag differs from current (and seeds the ETag on a
  fresh `None` subscription so the first poll reports it once), and `emit_resource_updates(peer)` sends
  one `notifications/resources/updated` per changed URI and stores the new ETag. stdio's `serve_stdio`
  spawns `subscription_poll_loop(peer)` (periodic `compute_changed` -> emit) and aborts it after the
  peer's run future completes. A `subscription_change_detection` unit test seeds a CAS blob, subscribes
  with `None`, asserts the URI is reported once then goes quiet, and asserts a stale stored ETag is
  re-detected. mcp tests pass under `--features http`, clippy clean. Spec-ideal event-driven push (0029)
  and HTTP-transport push (the session manager owns the peers) are noted as follow-ons; stdio is the
  delivered path.

- 2026-07-01 (queue 3 task 160): App-aware resource list-changed. The same poll loop now fingerprints
  the visible `resources/list` URI set and emits `notifications/resources/list_changed` when valid MCP
  app resources are added or removed. Invalid app candidates remain visible through `apps_list` but do
  not enter the standard resource list and do not trigger resource-list membership changes.

- 2026-07-02 (queue 3 task 230): Binary-sourced internal VCS app. `loom-mcp` now advertises
  `ui://.../mcp/apps/internal/vcs` from the binary asset provider for each workspace. The app is
  inspectable through `apps_show`, `apps_read_file`, and `resources/read`, but mutation tools still use
  the single-segment user-app validator and cannot write the reserved internal hierarchy.

- 2026-06-28 (task 188): Completion + pagination. server.rs implements the `complete` `ServerHandler`
  method (capability advertised via `.enable_completions()`): `complete_argument` resolves the
  `workspace` argument - the one completable slot shared by the prompt surface and every `loom://`
  resource template - against live workspace names through the read facade (PEP-gated), prefix-filtered
  by the partial value and capped at `CompletionInfo::MAX_VALUES`; other argument names have no
  enumerable domain and return empty. Pagination: a generic `paginate<T>(items, cursor)` helper walks a
  list in `PAGE_SIZE` (100) windows with an opaque decimal-index cursor (out-of-range / unparsable
  cursors -> invalid-params), now wired into `list_resources` and `list_resource_templates` (tools and
  prompts come from the bounded static surface via the rmcp routers). Two unit tests: `paginate_cursors`
  (three-page walk + bad-cursor rejection) and `complete_workspace_prefix` (seeded workspaces,
  prefix filter, empty for non-workspace args). mcp tests pass under `--features http`, clippy clean.

- 2026-06-28 (task 189): Utilities - progress + cancellation (logging dropped: deprecated upstream).
  The `#[tool_handler]` macro is now hand-rolled in server.rs (`call_tool`/`list_tools`/`get_tool`) so
  both utilities apply uniformly at the single dispatch chokepoint. Progress: if the client put a
  `progressToken` in the request `_meta`, a `0/1` start and (on success) a `1/1` completion
  `notifications/progress` bracket the call (`progress_param` helper). Cancellation: the dispatch is
  raced against the request's `RequestContext::ct` (which rmcp cancels on `notifications/cancelled`); a
  request already cancelled at the boundary returns the JSON-RPC -32800 "request cancelled" error
  (`cancelled_error` helper). The engine runs each tool body synchronously and quickly, so the dispatch
  boundary - not mid-body preemption - is the meaningful cancellation point (the framework already
  unblocks the client and discards the late result). `list_tools` also paginates now (the ~116-tool
  surface spans two `PAGE_SIZE` pages). Unit test `progress_and_cancellation_helpers` checks the -32800
  code and the 0/1 + 1/1 progress params. clippy clean (collapsed the success+token if-let chain).

- 2026-06-28 (task 187): Elicitation - destructive-write confirmation (0008 section 9.5). Enabled rmcp
  `schemars` + `elicitation` features (the latter pulls `url`; MIT/Apache-2.0/Unicode-3.0 tree, all in
  the deny allow-list - `cargo deny check licenses` clean). At the `call_tool` chokepoint, a tool whose
  annotation carries `destructive_hint=true` (`is_destructive_tool`) is gated by `confirm_destructive`,
  which sends a `DestructiveConfirm { confirm: bool }` form elicitation (`rmcp::elicit_safe!`) via
  `Peer::elicit` before the tool runs: an explicit decline/cancel or a `confirm=false` reply aborts with
  a clean invalid-params refusal (`declined_error`), while a client that does not support elicitation
  proceeds (owner-mode default - no one to ask, and a passwordless loom resolves the caller to the
  owner). Elicitation is a client capability, so `get_info` is unchanged. The other 9.5 flows (unlock
  passphrase, principal select, merge resolve) reuse the same `Peer::elicit` mechanism when those auth
  /merge models are active. Unit test `destructive_tool_recognition` checks destructive vs read/
  idempotent classification and the decline error code. mcp tests + clippy clean under `--features http`.

- 2026-06-28 (task 192): Behavioral conformance + drift. Two server.rs tests close the loop on the
  served surface. `capabilities_advertise_section_9` is a capability-drift guard: it asserts `get_info`
  advertises exactly the section-9 primitives - tools, prompts, completions, and resources with both
  `subscribe` and `listChanged` true. `behavioral_conformance_read_path` seeds a CAS workspace + blob
  then drives the served read path through the same methods the resource/completion handlers delegate to
  (the wire transport itself is covered by task 190's HTTP smoke test and rmcp's own conformance):
  `list_workspace_resources` surfaces `loom://blobs/`, `read_target` returns the blob's base64 contents
  with `resource_etag` equal to the content address, `complete_argument` resolves the `workspace` slot,
  `paginate` yields a non-empty template page, and `is_destructive_tool` recognizes the destructive
  elicitation target. These join the existing surface-drift tests (section 9.1 tools, 9.8 prompts, the
  registered-tool/prompt equality checks, and the IDL-coverage test). mcp tests + clippy clean on
  default/server/http.

- 2026-06-28 (task 215a build): Removed the `facet` selector from whole-workspace operations across
  the C ABI surface (RD12c). Workspace names are unique (0014), so version-control and file operations
  now resolve a workspace by name/UUID alone: `idl/loom.idl` drops `FacetKind facet` from all 44 affected
  methods, `include/loom.h` drops the 43 `const char *facet` params, and `loom-ffi`'s `resolve_ns` is
  name/UUID-only with every helper and exported fn updated. `FacetKind` is retained only where the facet
  is the subject, not a selector: `workspace_create` (creates a workspace with a facet) and ACL
  grant/revoke (facet-scoped grants). loom-ffi tests pass; clippy clean. The 8 language bindings (215b)
  are the remaining surface.

- 2026-06-28 (task 215b build): Propagated the `facet`-less whole-workspace selector into the 8
  language bindings for the two whole-workspace history ops the bindings expose, `vcs_blame` and
  `vcs_diff`. C++ (`loom.hpp`), iOS/Swift (`Loom.swift`), and React Native (TS `index.ts` /
  `NativeUldrenLoom.ts`, ObjC `UldrenLoom.mm`, Android JNI `UldrenLoom.cpp` + Kotlin module) drop the
  leading `facet` argument and its C-call site. Node (napi `src/lib.rs` + checked-in `index.d.ts`),
  Python (pyo3 `src/lib.rs` + `__init__.pyi`), and WASM (`src/lib.rs`) drop the `facet` parameter and
  resolve the workspace by name/UUID via `resolve_workspace_arg` instead of `resolve_typed_ns`. JVM
  (`Loom.java`) drops `facet` from the `LOOM_VCS_BLAME`/`LOOM_VCS_DIFF` downcall descriptors (one fewer
  `ADDRESS`), from all public `vcsBlame*`/`vcsDiff*` overloads, and from the `invokeExact` call sites.
  Android Kotlin drops `facet` from the `commonMain` expect, the `jvmMain`/`androidMain` actuals and
  their `nativeVcsBlame`/`nativeVcsDiff` externals, and the JNI C entrypoints. The wasm-pack `pkg/`
  glue is a build artifact and is regenerated by `just test-bindings`. Task 215 complete; verify via
  `just test-bindings`.

- 2026-06-28 (tasks 212-214 build, corrected 2026-06-29): Finished MCP scoping + added collection
  discovery. Workspace scoping elides and injects the workspace argument but does not filter tool,
  prompt, or resource areas by facet. Collection scoping elides and injects the collection-axis
  argument and drops collection discovery tools. Re-rooting: resources and resource templates are
  scoped to the bound workspace and collection, resource read/subscribe rejects out-of-scope URIs, and
  `complete` suppresses `workspace` completion when bound. List-collections discovery: a generic
  core `Loom::list_collections(ns, facet)` enumerates the first
  path segment under a facet's reserved directory (serves kv/document/time-series/ledger/queue and sql
  databases; per-principal facets keep their own listers), a `read_collections` facade method, six new
  read tools (`kv_list_collections`, `document_list_collections`, `timeseries_list_collections`,
  `ledger_list_collections`, `queue_list_streams`, `sql_list_databases`) with matching IDL methods and
  section 9.1 rows so the drift/coverage tests stay balanced.

- 2026-06-28 (tasks 206-209 build): Implemented the collection rename + scoping. (206) Renamed the
  collection parameter `name`->`collection` in the core facade (kv/document/time-series/ledger,
  handling the document.rs value-param collision) and the MCP agent-facing tool schema; argument order
  is workspace-then-collection. (207) `sql_read_table`/`index_scan`/`blame`/`diff` now take
  `{workspace, db, table}` and build the reserved path `.loom/facets/sql/{db}/tables/{table}` via a
  `sql_table_path(db, table)` helper (no hard-coded `main`; the generic core `read_table` stays
  path-based for columnar reuse). (208) Principal auto-binding: per-principal areas (calendar/contacts/
  mail) elide `principal` and the host injects the caller ("owner" in passwordless mode), so the agent
  can never address another principal's subtree. (209) Scoping/elision: a `Binding { workspace?,
  collection?, principal }` (CLI `loom mcp <file> [<workspace> [<collection>]]`) elides bound params
  from tool schemas (`apply_binding`) and re-injects them at `call_tool` (`inject_binding`), so the PEP
  is unchanged; `serve_stdio`/`http_service`/`serve_http` take the binding. The scoping pass now covers
  tool schema elision, tool-call injection, collection discovery removal when collection-bound,
  completion suppression for bound workspace slots, resource-template re-rooting, resource
  read/subscribe scope validation, and collection discovery. Deferred to the backlog: the
  FFI/IDL/header/binding parameter rename and the `vcs.rs`->`tabular.rs` move.

- 2026-06-28 (scoping design): Added section 9.10 (Scoped invocation and parameter elision). Records
  the three launch scopes (`loom mcp <file>` / `<file> <workspace>` / `<file> <workspace> <collection>`;
  no zero-arg form - the file is a connection, never assistant-discovered), the recommended
  workspace-scoped default, and the optional `default_workspace`. Defines the two elision mechanisms
  (parameter elision with server-side injection at `call_tool` so the PEP is unchanged), the
  per-facet collection-axis table, the
  collection-level rules (apply-to-all-matching not reject; `cas`/`fs`/`vcs` stay workspace-scoped),
  and principal auto-binding for calendar/contacts/mail (caller principal, cross-principal denied,
  parameter elided; cross-principal sharing deferred to P2+). The implementation is recorded in the
  later collection-rename, SQL db-path, principal-binding, and scoping entries.

- 2026-06-28 (task 193): Capability entry + docs/change-logs. Registered the `mcp-host` capability
  (owning spec 0008) in the engine capability registry: `loom_core::capability` gains
  `cap("mcp-host", "0008", SourceBacked, false)` and 0010 section 5 gains the matching
  `| mcp-host | 1/1 | 0008 | source-backed |` row (the loom-core drift test keeps the const catalog and
  the table identical). Proof status is source-backed (the host API and its behavioral tests exist in
  loom-mcp, but the shared `certify_memory_store` suite does not yet run an MCP backend); supported is
  false at the engine layer because loom-core does not serve MCP. loom-mcp owns the capability and
  contributes it: `loom_mcp::MCP_HOST_CAPABILITY` + `served_capabilities(base)` overlay it supported
  (the capability-contribution pattern), with a unit test asserting the engine declares it unsupported
  and the host overlay flips it. This change log plus the section-9 task entries (180-185, 187-192,
  200-201) are the docs record; the staleness audit and the dropped/deprecated notes (roots, sampling,
  logging, multi-round-trip) remain recorded above.

- 2026-06-28 (task 190): Transports. stdio was already served (`serve_stdio`, task 180); added the
  Streamable HTTP transport (owner mode) behind a new `http` cargo feature: `http_service(Arc<LoomMcp>)`
  builds rmcp's tower `StreamableHttpService` (LocalSessionManager, default config) with a per-session
  `LoomServer` factory over the shared facade, and `serve_http(mcp, addr)` mounts it at `POST /mcp` via
  axum. Owner mode = passwordless; every call still passes the engine PEP. The `http` feature pulls axum
  + rmcp `transport-streamable-http-server` + tokio net/rt-multi-thread, all off by default so the
  default and `server` builds are unaffected (and axum stays out of the default dependency graph for
  cargo-deny). A smoke test constructs the service; 35 mcp tests pass under `--features http`, clippy
  clean on default/server/http. Note: this is the pre-stateless (rmcp 0.16) Streamable HTTP with
  sessions; the 2026-07-28 stateless transport (no session id, `Mcp-Method`/`Mcp-Name` headers) is a
  future migration noted in the staleness audit.

- 2026-06-28 (task 201): Wired the prompt surface onto rmcp - a `prompt_router` field + a
  `#[prompt_router]` impl with one `#[prompt]` per `PROMPT_SURFACE` entry returning a templated
  `Vec<PromptMessage>` (User message orchestrating the area's tools), plus `#[prompt_handler]`
  alongside `#[tool_handler]` on the ServerHandler impl. `enrich_prompts` sets each prompt's
  description (from the surface summary), title, and `_meta.category`. A drift test asserts the
  registered prompt set equals `PROMPT_SURFACE`; 34 mcp tests pass under `--features server`,
  clippy clean. Also corrected the tool `idempotent_hint` classifier (creates, renames, checkout,
  symlink removed - they error or differ on re-run; reads stay unset per the MCP "meaningful only
  when not read-only" rule).

- 2026-06-28 (task 200): Wired the full tool surface onto rmcp in crates/loom-mcp/src/server.rs - every
  `TOOL_SURFACE` entry is an `#[tool]` (dotted name, read tools tagged `read_only_hint`) that
  deserializes `Parameters<T>` (serde + schemars) and returns `Json<serde_json::Value>` structured
  content, calling the read/write facade so each `tools/call` crosses the engine PEP. Result encodings:
  scalars/strings/lists serialized directly; raw bytes and canonical-CBOR readers (kv/sql/document/
  timeseries/calendar/contacts entries) as byte arrays; mail records, vcs status, merge/replay outcomes,
  and occurrences as JSON objects. While wiring, completed the read facade gaps S3 had left
  (document_list_binary, timeseries.range, the calendar/contacts read sets, and mail get_mailbox/list_mailboxes/
  list_messages/get_flags/search), and fixed a wasm `sql_diff` regression (dropped `ns` arg restored).
  A drift test asserts the registered rmcp tool set equals `TOOL_SURFACE` exactly (it does); 29 mcp tests
  pass under `--features server`, clippy clean on both default and server builds. Added serde (always) +
  schemars (server) deps.
