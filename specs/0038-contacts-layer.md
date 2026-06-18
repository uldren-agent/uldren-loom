# 0038 - Contacts Layer

**Status:** Draft, with the local facet, CLI, MCP projection, VFS overlay, compute/WASM host calls,
bindings, bounded hosted CardDAV profile, RFC gate, Queue 7 client evidence, and Queue 10 local
hardening evidence source-backed.
**Version:** 0.1.0.
**Capability:** `contacts`.

**Depends on:** 0001 (invariant A6/A7), 0002 (object model), 0042 (collections), 0003d (commit diff),
0014/0014a (workspaces and reserved-path projection), 0024 (CAS, shared digest profile), 0027 (ACL, for
the hosted tier). **Relates to:** 0037 (calendar - the storage/projection model this inherits), 0039
(mail), 0041 (lifecycle hooks - the de-dup entry point).

The `contacts` facet stores address books of people and organizations. It is the contacts sibling of the
calendar facet (0037) and shares its keystone decision: the **structured record is the source of truth**,
and vCard text, the mounted `.vcf` file, and the hosted CardDAV body are all serialized from it on demand
(RD1). The reusable record and projection contracts now live in `loom-pim`, with `loom-core::contacts`
retaining workspace, ACL, reserved-path storage, and `FacetKind::Contacts` integration. Storing the
wire bytes would make querying, de-dup, and per-contact diff operate on messy text; contacts are an
agent-first dataset, so the typed record is authoritative.

Compute automation follows the 0015/0041 D+C decision. Contacts automation first enters compute
through lifecycle events such as `on_contact_added`, `on_contact_updated`, and `on_contact_merged`.
Direct Rust `StateAccess` is source-backed as domain-shaped address-book metadata, contact CRUD, list,
and search operations, and guest WASM uses the same domain-shaped host calls. The 0029 trigger-fire
bridge can execute a contacts program and append a fire record. Hook registration, add/update event
emission, and execution-policy planning are source-backed in 0041; de-dup/merge UI and administration
remain target work. Contacts programs must not receive raw reserved-record CRUD as the contract.

The first hosted contacts completion pass is owner-only. Delegated address books, shared address books,
directory-style service-principal sharing, and cross-principal address-book discovery remain deferred to
a later 0026/0027 policy slice and are not advertised by the current CardDAV capability rows.

## 1. Model

A contact is a typed [`ContactEntry`] record keyed by `UID`, living per principal and address book
(0042 two-level collection `principal > book`) at the reserved path
`contacts/<principal>/<book>/<uid>`. The record carries the core vCard properties as typed fields - `FN`
(formatted name), `N` (structured name), `EMAIL` and `TEL` as typed-value lists (each an optional
`TYPE`), `ORG`, `TITLE` - plus an **extension bag** for unknown / `X-` / vendor properties so projection
round-trips losslessly (RD1a). The ETag is the content address of the canonical record (RD3); the book
sync-token is a commit (RD5). There is no time or recurrence dimension, so the facet is calendar minus
range/RRULE.

**Principal is the caller, not a query argument.** Projections (the MCP host, 0008 section 9.10; a
hosted CardDAV server) bind the principal to the calling identity - the owner in a passwordless loom -
and never accept it as an agent-supplied parameter. A principal may read and write only its own subtree
(`.loom/facets/contacts/{principal}/*`); the policy enforcement point denies any other principal's
subtree. **Cross-principal sharing** (sharing an address book to another principal) is an open question
deferred to **P2+**, pending the 0026/0027 identity and ACL model; it is not solved here.

## 2. Surfaces and priority

Uniform with 0037: **P0 = the local facet end to end** (core, search, IDL/conformance, CLI, language
bindings, and the `.vcf` filesystem projection); **P1 = the MCP surface** (source-backed through
curated tools/resources/prompts); **P2 = the hosted CardDAV wire projection** (gated on 0008/0026/0027).
vCard 3.0/4.0 dialect handling is negotiated by requested CardDAV `address-data` version at the
serialization layer (RD1b), never in storage. The bounded CardDAV wire profile defaults to vCard 3.0
because RFC 6352 requires CardDAV servers to support vCard 3.0 and only recommends vCard 4.0.

## 3. Facade (source-backed)

`loom-core::contacts`: `create_book`/`get_book`/`list_books`/`delete_book`;
`put_entry`/`get_entry`/`delete_entry`/`list_entries`; `search` (case-insensitive substring over name,
org, and email values - the CardDAV property-filtered model); `diff_entries` (per-UID
added/updated/removed with new ETags - the structured `sync-collection` diff); and the projection codec
`to_vcard`/`from_vcard` with the facet helpers `entry_vcard` (serialize on demand) and `put_vcard`
(parse-and-store, the validated write-in path). A `put_entry` into a book that does not exist is
`NOT_FOUND` (CardDAV requires the collection first); an empty `UID` or `FN` is `INVALID_ARGUMENT`.
Durable local contacts indexes use `loom-store::derived` records under
`derived-index:<index-name>` with format version `contacts-derived-index-v1`, source-digest,
engine-version, stale, rebuild, failed, and unsupported reporting. The source digest is supplied by
the index builder and must cover the canonical contact records plus the index profile.

The IDL `interface Contacts` (idl/loom.idl) is the local interface source for the C ABI and language
bindings. The CLI projects book CRUD, entry CRUD, search, and vCard input/output over the same facade.
MCP tools/resources/prompts are source-backed in `loom-mcp`: the tool surface includes the contacts
CRUD/search/vCard projection methods, resources expose `.vcf` bodies, and the server binding elides
`principal` from agent-visible tool schemas before injecting the bound principal server-side. The
first bounded hosted CardDAV projection is source-backed under 0008, including address-book
properties, resource properties, conditional writes, `addressbook-multiget`, and `addressbook-query`
over the structured fields, commit-backed `sync-collection` with tombstones, and direct TLS through
hosted certificate-bundle listener records. The hosted CardDAV projection advertises and serves
`text/vcard` versions `3.0` and `4.0`; vCard 3.0 is the default CardDAV wire representation and vCard
4.0 is returned when requested. vCard 3.0 writes are accepted through the same CardDAV PUT path and
preserve grouped, parameterized RFC 2426 properties that the typed contact model does not interpret.
Full CardDAV ACL authoring, partial `address-data`, Apple-label display semantics, delegated sharing,
durable certification administration, and broader ACL-aware serving remain target work. Queue 7
reference-client validation is owner-verified for Apple Contacts, Thunderbird, and DAVx5 against the
local certification harness; durable redacted transcript storage and admin-visible certification status
remain 0065 target work. Records and book metadata cross as Loom Canonical CBOR; `put_entry` returns
the `Digest` ETag.

### 3.1 Conditional mutation and comparison anchors

Contact entry mutation consumes the contract owned by 0003 section 9.1. The comparison anchor is the
current canonical record for one `(principal, book, uid)` resource. Its content address is the contact
ETag, while an owner-issued opaque token remains permitted for a native facade. The atomic scope is one
contact resource. Contact entry writes consume `any`, `absent`, and `exact`; book metadata mutations
may consume `generation`. Contacts does not consume `operation_anchor` for ordinary entry operations.

Conditional writes do not merge contact fields or extension-bag values. A stale replacement fails
without changing the record. Contact de-duplication or merge remains the explicit lifecycle-program
boundary in RD4. CardDAV preconditions are a projection of the native rule and do not define it.
Authorization and redacted audit evidence inherit 0009; comparison failures use 0003 section 8 and do
not disclose protected contact content or raw opaque tokens.

## 4. Resolved decisions

- **RD1 - Structured source of truth.** The record is authoritative; vCard/`.vcf`/CardDAV are projected
  on demand (inherited from 0037 RD1). Confirmed in this build.
- **RD1a - Lossless via an extension bag.** Unknown / `X-` properties are preserved in the record's
  `extra` bag; parse/store/serialize is semantically identity-preserving.
- **RD1b - Dialect at projection.** The canonical local projection remains vCard 4.0 through `vcard4`
  and parses the supported `vcard4` input grammar. The hosted CardDAV projection is source-backed for
  vCard 3.0 input/output and bounded vCard 4.0 output, selected by requested `address-data` version
  and defaulting to vCard 3.0. vCard 3.0 registered properties without typed Loom fields are preserved
  as grouped, parameterized raw properties. Apple `X-ABLabel` display semantics remain target behavior
  until source-backed transcripts require them.
- **RD2 - Collections are address books (0042).** `principal > book > contact`; the book is the unit of
  ACL scope and projection. One record per `UID`.
- **RD3 - ETag is the content address** of the canonical record.
- **RD4 - De-dup is a program (0041).** Contact de-duplication/merge is a lifecycle-hook program
  (`on_contact_added`/`on_contact_merged`), not bespoke facet logic.

## 5. Dependencies and gating

The local facet and CLI (P0), MCP projection (P1), and first bounded hosted CardDAV profile (P2) are
source-backed now. The hosted `contacts/carddav` runtime serves `.well-known/carddav`, OPTIONS,
PROPFIND, MKCOL, GET, PUT, and DELETE over `.vcf` resources through durable listener records and the
hosted auth/PEP path. It also serves CardDAV address-book properties, canonical resource ETags,
conditional PUT/DELETE preconditions, and `addressbook-multiget` with per-resource `200` and `404`
rows. `addressbook-query` is source-backed for formatted-name, structured-name, organization, title,
email, telephone, UID, and `TYPE` parameter filters over typed values. Sync-token behavior is
source-backed through commit-digest tokens, no-op matching sync, tombstones, and full-resync recovery
for stale client tokens.
Direct TLS is source-backed through hosted certificate-bundle listener records. vCard dialect
certification for vCard 3.0 input/output and vCard 4.0 output is source-backed for the CardDAV profile.
Full CardDAV ACL authoring, partial `address-data`, Apple-label display semantics, delegated sharing,
durable certification administration, and broader ACL-aware serving remain target work under 0008 and
0065. The shared
`.vcf` mount overlay rides the same loom-vfs synthetic-projection mechanism as 0037's `.ics` overlay
(built once across the trio).

## 6. Sources

- vCard 3.0 (RFC 2426); vCard 4.0 (RFC 6350); CardDAV (RFC 6352). Apple Contacts dialect quirks
  (informative).
- Shared model: 0037 (calendar) RD1/RD1a/RD1b/RD3; 0042 (collections); 0003d (commit diff).

## 7. Hosted CardDAV Projection

Hosted CardDAV completion is owned jointly by this spec and 0008. The bounded owner-only profile is
source-backed for the rows listed here; the deferred full profile remains target work.

Source-backed bounded profile:

- consume the shared 0008 WebDAV substrate for path escaping, XML parsing, multistatus output,
  conditional writes, sync-token behavior, hosted auth, PEP, request limits, audit, and store-save;
- return DAV collection properties plus CardDAV address-book properties, address-data support,
  `getctag`, and `sync-token`;
- implement `addressbook-multiget` with per-resource `200` and `404` rows;
- implement a deterministic `addressbook-query` subset over structured contact records: formatted name,
  organization, email values, typed value lists, requested `getetag`, and requested `address-data`;
- apply `If-Match` and `If-None-Match` to PUT and DELETE using the structured-record digest as the ETag;
- accept Apple's `_NO_ETAG_` `If-Match` compatibility token only when the target resource is absent,
  so an Apple Contacts create/update retry can converge without weakening normal ETag checks;
- implement `sync-collection` from commit-digest tokens and per-UID diffs, including tombstones and
  full-resync recovery for stale or unrecognized client tokens;
- keep vCard dialect handling as projection logic. vCard 3.0 input/output and vCard 4.0 output are
  source-backed for the CardDAV profile; Apple label semantics are added only when certification
  transcripts prove they are required;

Deferred full-profile rows:

- keep delegated address-book sharing and broader ACL-aware serving as explicit deferred rows unless
  promoted by a later 0026/0027 policy slice;
- keep WebDAV ACL authoring, COPY/MOVE, full partial `address-data`, `max-resource-size`,
  `supported-collation-set`, and Apple-label display semantics as explicit target rows unless promoted
  by source-backed tests and client transcripts;
- keep durable certification profile selection, transcript storage, certification status projection,
  and admin review in 0065 rather than in this facet spec.

### CardDAV RFC implementation gate

Queue 7 cannot certify reference clients until this gate is either source-backed or explicitly recorded
as unsupported, degraded, target, or deferred in the hosted capability report. Shared HTTP, TLS, URI,
discovery, Basic auth, and WebDAV mechanics are owned by 0008. This table owns contacts-specific
protocol and data behavior. The hosted capability inventory splits this gate into
`contacts/rfc-gate/carddav-rfc6352-bounded-access-profile`,
`contacts/rfc-gate/vcard-rfc6350-bounded-profile`,
`contacts/rfc-gate/carddav-rfc6352-rfc6350-bounded-profile`,
`contacts/rfc-gate/vcard3-dialect-conversion`, `contacts/rfc-gate/xcard-rfc6351`,
`contacts/rfc-gate/place-death-extensions`,
and `contacts/rfc-gate/parameter-caret-encoding`.

| RFC | Area | Acceptance rule |
| --- | --- | --- |
| RFC 6352 | CardDAV access | Degraded against full RFC 6352, source-backed for the bounded owner-only profile: advertised CardDAV collection and principal discovery properties, `current-user-principal`, `principal-URL`, `owner`, `current-user-privilege-set`, `addressbook-home-set`, `supported-address-data` for `text/vcard` versions `3.0` and `4.0`, `addressbook-multiget`, `addressbook-query`, resource GET/HEAD/PUT/DELETE, ETags on GET/HEAD/PUT/PROPFIND/REPORT paths, sync tokens, and status mapping have source-backed tests. Delegated sharing, WebDAV ACL authoring, Extended MKCOL request bodies, COPY/MOVE, full partial `address-data` projection, `max-resource-size`, and `supported-collation-set` remain outside the bounded Queue 7 profile unless promoted. |
| RFC 2426 | vCard 3.0 | Source-backed for the CardDAV profile: `BEGIN:VCARD`/`END:VCARD`, `VERSION:3.0`, required `FN`, `N`, and `VERSION` on output, grouped content lines, parameters, ordinary `TYPE` parameters, TEXT escaping, structured text delimiters, line unfolding/folding, default CardDAV `address-data` output, CardDAV PUT input, and preservation of RFC 2426 registered properties without typed Loom fields are covered by tests. Input tolerates missing `N` by deriving a structured-name value from `FN`, but generated vCard 3.0 includes `N`. `FN`, `N`, `ORG`, `TITLE`, `EMAIL`, `TEL`, and `UID` are mapped into typed fields; `PROFILE`, `SOURCE`, `NAME`, `NICKNAME`, `PHOTO`, `BDAY`, `ADR`, `LABEL`, `MAILER`, `TZ`, `GEO`, `ROLE`, `LOGO`, `AGENT`, `CATEGORIES`, `NOTE`, `PRODID`, `REV`, `SORT-STRING`, `SOUND`, `URL`, `CLASS`, `KEY`, and `X-` properties are preserved as grouped, parameterized vCard 3.0 properties. The canonical local projection remains RFC 6350 vCard 4.0. |
| RFC 6350 | vCard 4.0 | Supported for the bounded owner-only profile: `BEGIN:VCARD`/`END:VCARD`, `VERSION:4.0`, required `UID` and `FN`, `N`, `ORG`, `TITLE`, `EMAIL`, `TEL`, ordinary typed `TYPE` parameters, TEXT escaping, line folding/unfolding, supported `X-` extension values, and stable vCard 4.0 projection and parse behavior are covered by tests. Wider typed schema fields such as `ADR`, `PHOTO`, `BDAY`, `ANNIVERSARY`, `GENDER`, `IMPP`, `LANG`, `TZ`, `GEO`, `ROLE`, `LOGO`, `MEMBER`, `RELATED`, `CATEGORIES`, `NOTE`, `SOUND`, `URL`, `KEY`, `FBURL`, `CALADRURI`, `CALURI`, synchronization `PID` semantics, grouped properties, and non-`TYPE` parameter semantics remain outside this bounded profile unless promoted. |
| RFC 6351 | xCard | Unsupported in Queue 7. `supported-address-data` advertises only `text/vcard` versions `3.0` and `4.0` and does not advertise `application/vcard+xml` or the `urn:ietf:params:xml:ns:vcard-4.0` XML representation. XML vCard requires source-backed xCard parse/serialize tests and conversion policy before it can be advertised. |
| RFC 6474 | vCard place and death extensions | Target. The current vCard 4.0 parser rejects registered RFC 6474 properties `BIRTHPLACE`, `DEATHPLACE`, and `DEATHDATE`; they are not preserved through the extension bag. Promotion requires typed fields or explicit registered-property extension handling, parse/serialize vectors, parameter preservation for `VALUE`, `LANGUAGE`, and `CALSCALE`, and value-type consistency checks. |
| RFC 6868 | vCard parameter value encoding | Target. The bounded profile supports ordinary typed `TYPE` parameters, but it does not claim RFC 6868 caret decoding for `^n`, `^^`, or `^'`, nor caret encoding when serializing parameter values. Promotion requires source-backed parse/serialize vectors for quoted and unquoted vCard parameter values and must preserve unknown caret sequences literally as RFC 6868 requires. |

Reference-client certification for this target is Apple Contacts or iOS Contacts, Thunderbird, and
DAVx5. Queue 7 task 90.2 is owner-verified green against the local certification harness. The
conformance crate records the durable requirement and redacted transcript fixture shape through
`PIM_CERTIFICATION_CLIENT_REQUIREMENTS`, `PIM_TRANSCRIPT_INVENTORY`, and
`QUEUE7_PIM_CAPABILITY_REPORT_FIXTURES`; 0065 owns the later admin interface and durable certification
record.

### Queue 7 closure evidence

| Evidence area | Disposition | Source-backed references |
| --- | --- | --- |
| Bounded CardDAV access | Degraded against full RFC 6352, supported for the bounded owner-only profile. Discovery, principal/home-set properties, address-book properties, resource GET/HEAD/PUT/DELETE, `addressbook-multiget`, `addressbook-query`, ETags, sync tokens, and status mapping are source-backed. Delegated sharing, WebDAV ACL authoring, Extended MKCOL bodies, COPY/MOVE, full partial `address-data`, `max-resource-size`, and `supported-collation-set` are not advertised. | `crates/loom-hosted-pim/src/dav.rs`; `crates/loom-conformance/src/lib.rs` row `contacts/rfc-gate/carddav-rfc6352-bounded-access-profile`; Queue 7 task 40.4.1. |
| vCard 3.0 CardDAV dialect | Supported for CardDAV input/output and default `address-data`. Grouped content lines, parameters, ordinary `TYPE`, text escaping, line folding, typed field mapping, and raw preservation for registered properties without typed Loom fields are source-backed. The canonical local projection remains vCard 4.0. | `crates/loom-pim/src/contacts.rs`; `crates/loom-hosted-pim/src/dav.rs`; `crates/loom-conformance/src/lib.rs` row `contacts/rfc-gate/vcard3-dialect-conversion`; Queue 7 task 40.4.1.1. |
| vCard 4.0 projection | Supported for the bounded owner-only profile: `UID`, `FN`, `N`, `ORG`, `TITLE`, `EMAIL`, `TEL`, ordinary `TYPE`, TEXT escaping, line folding, supported `X-` values, and stable parse/projection behavior. Wider typed fields, grouped properties, synchronization `PID`, and non-`TYPE` parameter semantics remain outside this bounded profile. | `crates/loom-pim/src/contacts.rs`; `crates/loom-conformance/src/lib.rs` rows `contacts/rfc-gate/vcard-rfc6350-bounded-profile` and `contacts/rfc-gate/carddav-rfc6352-rfc6350-bounded-profile`; Queue 7 task 40.4.2. |
| Direct TLS and hosted listener shape | Supported for daemon-opened `contacts/carddav` listener records, certificate-bundle loading, and shared-port DAV coalescing through 0008. Durable service identity and trust-anchor administration remain 0065 target work. | `crates/loom-cli/src/daemon_cmd.rs`; `crates/loom-conformance/src/lib.rs` rows `contacts/carddav/direct-tls`, `pim/rfc-gate/tls-rfc8996-modern-versions`, and `pim/rfc-gate/shared-http-over-tls-service-identity`; Queue 7 task 45. |
| Lifecycle hooks | Source-backed for registration records, canonical event envelopes, matching, contacts add/update event emission, and execution-policy planning. De-dup/merge UI and hosted hook administration remain target work. | `crates/loom-core/src/hooks.rs`; `crates/loom-conformance/src/lib.rs` rows `pim/hooks/registration-envelope-event-emission` and `pim/hooks/execution-policy-planning`; Queue 7 tasks 70.1 through 70.3. |
| Owner-only access profile | Supported for the first hosted completion pass. Delegated address books, directory-style service-principal sharing, and cross-principal discovery are deferred to 0026/0027 policy work. | `specs/0008-wire-protocols.md`; `crates/loom-conformance/src/lib.rs` row `pim/access/owner-only-profile`; Queue 7 task 85. |
| Reference clients | Owner-verified for Apple Contacts, Thunderbird, and DAVx5 against the local certification harness. This is client evidence, not a substitute for the RFC rows above. Durable transcript capture and review are 0065 target work. | `_QUEUE7.md` task 90.2; `crates/loom-conformance/src/lib.rs` `PIM_CERTIFICATION_CLIENT_REQUIREMENTS` and `PIM_TRANSCRIPT_INVENTORY`; `scripts/pim-cert/README.md`. |
| Deferred standards and dialects | RFC 6351 xCard is unsupported. RFC 6474 place/death extensions and RFC 6868 caret-escaped parameters are target. Apple `X-ABLabel` display semantics remain target until required by source-backed transcripts. | `crates/loom-conformance/src/lib.rs` contacts RFC-gate rows; Queue 7 tasks 40.4.3 through 40.4.5. |

### Queue 10 local hardening evidence

Queue 10 closed the local non-binding hardening pass for contacts without moving the unfinished
binding parity, binding runtime, or coverage-reporting rows below. Evidence:

| Surface | Queue 10 evidence | Source-backed references |
| --- | --- | --- |
| MCP | Contacts tool schemas elide agent-supplied principal, server-side binding overwrites any scoped argument, malformed PIM arguments are rejected, contacts resource reads reject malformed or unauthorized targets, and contacts prompts stay in the registered prompt inventory. | `crates/loom-mcp/src/server/tests.rs` tests `registered_prompts_equal_the_surface`, `pim_binding_injects_and_overwrites_agent_scope`, `pim_arguments_reject_missing_or_malformed_scope_fields`, and `binding_scopes_resource_templates_and_reads`; `_QUEUE10.md` tasks 20 and 60. |
| Compute/WASM | Direct `StateAccess` and guest WASM calls cover contacts capability denial, missing book mapping, malformed guest record bytes, cross-principal isolation, and the existing positive domain-shaped contacts host-call round trip. | `crates/loom-compute/src/state_access.rs` test `pim_state_access_denies_modes_facets_and_missing_collections`; `crates/loom-compute/src/engine_wasmi.rs` tests `pim_host_abi_rejects_malformed_records_and_denied_grants` and `pim_domain_records_round_trip_through_host_abi`; `_QUEUE10.md` task 30. |
| VFS | The `.vcf` overlay covers missing book handling without quarantine, record UID update semantics after dropped-name ingestion, and projected-record delete support through the shared overlay path. | `crates/loom-vfs/src/overlay.rs` tests `missing_collection_is_not_quarantined` and `contacts_projection_updates_and_deletes_by_record_uid`; `crates/loom-vfs/src/lib.rs` test `overlay_unlinks_projected_facet_record`; `_QUEUE10.md` task 40. |
| Local CLI and C ABI | Local smoke tests cover missing contacts books through the CLI and missing contacts books through the C ABI path. | `crates/loom-cli/src/helpers.rs` test `pim_cli_reports_missing_containers`; `crates/loom-ffi/src/tests.rs` test `calendar_contacts_mail_round_trip_over_the_c_abi`; `_QUEUE10.md` task 50. |

### Unfinished binding and coverage items

These unfinished spec items are not Queue 10 scope.

| Priority | Item | Status | Owning follow-up |
| --- | --- | --- | --- |
| P1 | Per-binding contacts parity matrix for Node, Python, C++, Swift/iOS, JVM, Android, React Native, WASM, C ABI, and IDL shapes across address-book operations, vCard 3.0 and 4.0 projection, capability errors, and hosted-adjacent client expectations. | Unfinished | P9 binding specs or a later binding certification queue. |
| P1 | Binding runtime tests for positive, negative, and boundary contacts cases in each language binding. | Unfinished | P9 binding specs or a later binding certification queue. |
| P2 | Coverage reporting by contacts surface and binding family, distinct from Rust line coverage. | Unfinished | 0010a conformance reporting or a later binding certification queue. |

## Change log

### Local facet (P0): source-backed, capability executable

`loom-core::contacts` implements the facet over structured vCard records: `FacetKind::Contacts` (wired
through `as_str`/`from_str`/compression), `ContactEntry`/`TypedValue`/`BookMeta`, book + contact CRUD
over `contacts/<principal>/<book>/<uid>`, `search`, `diff_entries`, and the `to_vcard`/`from_vcard` +
`entry_vcard`/`put_vcard` projection codec (RFC 6350 TEXT escaping, `TYPE=` params, 75-octet line
folding, lossless `extra` bag). The `interface Contacts` IDL is in place. The `to_vcard`/`from_vcard`
projection codec uses the **`vcard4`** crate (0.7, RFC 6350 vCard 4.0):
`from_vcard` via `vcard4::parse`, `to_vcard` by constructing the typed `Vcard` (the builder is too lossy
for TYPE parameters, text UIDs, and `X-` extensions, so the record maps onto the public `Vcard` fields
directly), with library-injected `VERSION`/`PRODID`/`REV` ignored on parse so they never enter the
`extra` bag. Round-trip is semantic; MIT/Apache, deny.toml-clean. `run_contacts_facade_behavior`
(loom-conformance) exercises the full facade plus a clone target and is wired into `certify_memory_store`,
`BEHAVIOR_SUITES` (with `CONTACTS_SCENARIOS`), and `EXECUTABLE_BEHAVIOR_SUITES`; the aggregate
certification passes. The `contacts` capability is `executable` in the registry and the 0010 section 5
table (drift test green). Tests: 5 in-crate. The shared binding pass is done: the `loom_card_*` C ABI
(10 functions) is in loom-ffi and projected into
all eight language bindings as part of the calendar/contacts/mail trio pass (loom-ffi 36 tests green;
language bindings build via `just test-bindings`). The shared `loom-vfs` overlay projects
`contacts/<principal>/<book>/<uid>.vcf` and ingests valid dropped `.vcf` files through `put_vcard`.
The `loom-mcp` host projects contacts as curated tools, `.vcf` resources, and contact prompts with
server-side principal injection. The first bounded hosted CardDAV runtime is source-backed in
`crates/loom-hosted-pim/src/dav.rs` and daemon-opened by `contacts/carddav` listener records in
`crates/loom-cli/src/daemon_cmd.rs`. The hosted CardDAV runtime now includes address-book properties,
resource properties, conditional writes, `addressbook-multiget`, the source-backed
`addressbook-query` subset, vCard 3.0 input/output, vCard 4.0 `address-data` projection,
commit-backed `sync-collection` with tombstones, and direct TLS over hosted certificate-bundle listener
records. Stale or unrecognized CardDAV client sync tokens recover by returning the full current
address book with a current sync token so clients can refresh ETags before conditional writes.
Apple's `_NO_ETAG_` `If-Match` token is source-backed as an absent-resource-only compatibility
condition. Remaining: WebDAV ACL authoring, partial `address-data`, Apple-label display semantics,
broader hosted ACL-aware serving, durable certification administration, and hook administration.

### Reusable PIM component extraction - source-backed

`loom-pim` now owns the contacts local record contracts and projection helpers: `ContactEntry`,
`TypedValue`, `BookMeta`, canonical CBOR encode/decode, and vCard parse/serialize through `vcard4`.
`loom-core::contacts` consumes and re-exports those contracts while retaining workspace, ACL,
reserved-path storage, book CRUD, entry CRUD, ETag calculation, search, helper diffs, and
`FacetKind::Contacts` integration. Existing callers keep the `loom_core::contacts::*` surface.
Component-level record/projection tests live in `loom-pim`; executable facade conformance stays in
`loom-conformance` because it proves engine storage, versioning, clone reachability, and workspace
integration.
