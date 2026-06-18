# 0037 - Calendar Layer

**Status:** Draft, with the local facet, CLI, MCP projection, VFS overlay, compute/WASM host calls,
bindings, bounded hosted CalDAV profile, RFC gate, Queue 7 client evidence, and Queue 10 local
hardening evidence source-backed.
**Version:** 0.1.0.
**Capability:** `calendar`.

This spec defines the `calendar` facet: per-principal collections of calendar events (VEVENT) and task
lists (VTODO), stored as **structured records** (the source of truth), with iCalendar (RFC 5545) text,
the mounted `.ics` files, and the hosted CalDAV (RFC 4791) projection all **constructed on demand** from
those records. The structured records are what the agent, search, filters, and commit diff operate on;
iCalendar/CalDAV is an output format, not the storage (RD1).

The local facet is source-backed today in `loom-core::calendar`, backed by reusable calendar record and
projection contracts in `loom-pim`, plus the IDL/C ABI/bindings, CLI, `loom-vfs` facet overlay,
executable conformance, and the `loom-mcp` curated tools/resources/prompts. The first bounded hosted
CalDAV profile is source-backed for owner-only discovery, collection properties, resource
read/write/delete, conditional writes, `calendar-multiget`, `calendar-query`, commit-backed
`sync-collection`, direct TLS listener startup, and the bounded RFC rows recorded below. Scheduling,
free-busy, delegated sharing, durable certification administration, hosted hook administration, and
broader ACL-aware hosted-server policy remain target work. Every operation is scoped to one
workspace's calendar facet. Cross-workspace calendar writes are out of contract and must fail with
`CROSS_WORKSPACE`.

The first hosted calendar completion pass is owner-only. Delegated calendars, shared calendars,
free/busy lookup, scheduling inbox/outbox behavior, and service-principal sharing remain deferred to a
later 0026/0027 policy slice and are not advertised by the current CalDAV capability rows.

Compute automation follows the 0015/0041 D+C decision. Calendar automation first enters compute through
lifecycle events such as `on_event_added`, `on_event_updated`, `on_event_cancelled`, and
`on_occurrence_due`. Direct Rust `StateAccess` is source-backed as domain-shaped collection metadata,
entry CRUD, list, and search operations, and guest WASM uses the same domain-shaped host calls. The
0029 trigger-fire bridge can execute a calendar program and append a fire record. Hook registration,
event emission, and execution-policy planning are source-backed in 0041. Occurrence-range host calls and
hook administration remain target work. Calendar programs must not
receive raw reserved-record CRUD as the contract.

## 1. Model

A `calendar` facet holds, per **principal**, one or more **collections**. A collection is either a
calendar (events) or a task list (todos), distinguished by its CalDAV `supported-component-set`
(`VEVENT` vs `VTODO`; `VJOURNAL` is optional and out of scope for v1). Within a collection, each
**entry** is one iCalendar object identified by its `UID`, comprising a master component plus any
recurrence overrides (same `UID`, distinct `RECURRENCE-ID`), exactly as CalDAV models a calendar
resource (one resource per `UID`).

- A principal is the owning identity (see 0026). Until 0026 is source-backed, a principal is an opaque,
  caller-supplied string label; when 0026 lands, that label binds to a real principal id and access is
  gated by 0027.
- **Principal is the caller, not a query argument.** Projections (the MCP host, 0008 section 9.10; a
  hosted CalDAV server) bind the principal to the calling identity - the owner in a passwordless loom -
  and never accept it as an agent-supplied parameter. A principal may read and write only its own
  subtree (`.loom/facets/calendar/{principal}/*`); the policy enforcement point denies any other
  principal's subtree. Reading or browsing another principal's collections is therefore disallowed.
  **Cross-principal sharing** (delegating or sharing a calendar/task list to another principal,
  CalDAV-style sharing and free/busy) is an open question deferred to **P2+**: it needs the 0026/0027
  identity and ACL model and a sharing/ACL record design, and is intentionally not solved here.
- A collection is identified today by a caller-supplied, validated path segment. Its human display
  name, description, color, time-zone hint, and `supported-component-set` live in metadata. A
  server-assigned UUID path segment is the target hosted-server shape when collection ids need to be
  stable across display-name changes and aliases (RD8).
- An entry is a **structured record** (typed fields below), not a stored text blob. Its `{UID}.ics`
  filesystem appearance and its CalDAV `GET` body are serialized from the record on demand.

## 2. Storage (structured source of truth)

The authoritative store is a **structured, typed record** per entry, held as committed workspace content
in the facet's reserved tree. iCalendar text, the `.ics` file under the mount, and the CalDAV resource
body are **projections** serialized from the record when a client or the filesystem asks for them
(section 6, RD1). There is no second "canonical bytes" copy: the record is the truth and the text is
generated.

This is the model production CalDAV servers use (they parse iCalendar into a database and re-serialize on
`GET`); RFC 4791/6352 define the wire protocol and resource model, not server storage, so this is fully
compliant. It is also what an agent-first system needs: search, filters (0041 hooks), and commit diff
operate on the structured record, never on text.

### 2.1 Lossless round-trip and ETag

- **Extension bag.** The record models the full RFC 5545 field set *plus* a typed bag of unknown/`X-`
  /vendor properties and parameters, so anything a client sends that the model does not name is retained
  and re-emitted. Round-trip conformance asserts parse -> store -> serialize -> parse is semantically
  identical (RD1a).
- **ETag** is the content address of the canonical structured record (not of any serialized text), so it
  is stable across re-serializations and changes iff the record changes - the one contract CalDAV
  clients depend on.
- **User-agent-aware serialization.** The projection layer emits the dialect a client expects (for
  example vCard 3.0 vs 4.0 for `contacts`, Apple `X-APPLE-*` calendar props, `VTIMEZONE` emission) keyed
  on User-Agent. This quirk handling lives entirely at projection, never in storage (RD1b).

### 2.2 Canonical layout

Reserved facet paths (written only through the privileged facet path):

```text
/.loom/facets/calendar/<principal>/<collection>/<uid>        # the structured entry record (canonical CBOR)
/.loom/facets/calendar/<principal>/<collection>/.collection  # collection metadata (canonical CBOR)
```

Records version, branch, and sync as ordinary committed workspace content, keyed by `<uid>`. Source has
per-entry helper diffs for calendar states; the public cross-facet `diff_commits` contract is tracked
by 0003d. The `.ics` text is materialized for the filesystem projection (section 3) and CalDAV `GET`,
not stored.

### 2.3 Entry record and search index

The entry record holds: `uid`, component type (`VEVENT`/`VTODO`), `dtstart`/`dtend` (with the originating
time zone), `summary`, `status`, `rrule` (parsed; see RD10), recurrence overrides, and the extension bag.
`VEVENT` records require `DTSTART`; `VTODO` records may omit `DTSTART` and preserve task scheduling
properties such as `DUE` through the extension bag unless a later schema pass promotes them to typed fields.
`DESCRIPTION`, `SEQUENCE`, and newer iCalendar properties are preserved through the extension bag in the
bounded owner-only profile unless a later schema pass promotes them to typed fields. Current source serves `range` and `search` directly from the structured records. A
derived, rebuildable search index is target work for larger hosted deployments; it would materialize
query columns such as normalized `dtstart_utc`/`dtend_utc` for half-open time-range scans plus
`summary`/`description` and component/property filters (RD9):

```text
calendar_entries(principal, collection, uid, recurrence_id, component, dtstart_utc, dtend_utc,
                 summary, status, all_day, rrule, last_modified)
```

The index is derived state rebuildable from the records; conformance vectors are over the structured
records, not the index. Durable local calendar indexes use `loom-store::derived` records under
`derived-index:<index-name>` with format version `calendar-derived-index-v1`, source-digest,
engine-version, stale, rebuild, failed, and unsupported reporting. The source digest is supplied by
the index builder and must cover the canonical calendar records plus the index profile.

### 2.3 ETag and sync-token

- **ETag** of an entry is its Loom content address over the canonical resource bytes (`algo:hex`), so
  it is stable and collision-resistant by construction (RD3).
- **sync-token** of a collection maps to a Loom commit digest, so the CalDAV `sync-collection` REPORT
  becomes a commit-to-commit diff over the collection subtree (RD5).

### 2.4 Conditional mutation and comparison anchors

Calendar entry mutation consumes the contract owned by 0003 section 9.1. The comparison anchor is the
current canonical entry record for a `(principal, collection, uid)` resource. Its content address is
the calendar ETag, but the owner may provide an opaque native comparison token as well. The atomic scope
is one entry write or delete. Collection creation consumes `absent` at the collection resource; grouped
calendar changes use the 0003 section 6 batch transaction boundary.

Entry writes consume `any`, `absent`, and `exact`; collection metadata mutations may consume
`generation`. Calendar does not consume `operation_anchor` for ordinary resource edits. The sync-token
is a collection synchronization cursor, not an entry comparison anchor. Conditional mutation does not
merge recurrence rules, overrides, or extension-bag fields: a stale replacement fails, and any future
calendar merge must be an explicit operation with its own policy.

CalDAV `If-Match` and `If-None-Match` are projections of this native contract, not its definition.
Authorization and redacted audit evidence inherit 0009; compare failures use the 0003 section 8 error
contract and must not reveal protected calendar content.

## 3. Filesystem projection

The source-backed local VFS overlay (0003c) projects to top-level mount roots:

```text
calendar/{principal}/{calendar}/{uid}.ics
```

Reads expose each entry as its canonical `.ics` text. Writes through the projection (e.g. dropping an
`.ics` file in via FUSE/NFS) are accepted when the bytes parse as a valid single-`UID` `VCALENDAR`, go
through the same validated parse-and-store path as the facade, and otherwise fail with
`INVALID_ARGUMENT`.
The reserved-path policy of 0014a is separate: `/.loom/` and `/.loom/facets/` are readable and
non-writable, and the `calendar` facet has not opted into a user-facing `/.loom/facets/calendar/...`
projection. Collection metadata is not projected as a user-writable file in v1.

## 4. Public facade

The local facade is source-backed in core and projected through the current local CLI, ABI, and binding
surfaces. The MCP host exposes the same calendar surface as curated tools, `.ics` resources, and
calendar prompts; the MCP server binding elides `principal` from agent-visible tool schemas and injects
the bound principal server-side. The bounded hosted CalDAV projection is source-backed for discovery,
collection/resource CRUD, conditional writes, `calendar-multiget`, `calendar-query`, and commit-backed
`sync-collection` with per-UID tombstones. Direct TLS is source-backed through hosted
certificate-bundle listener records. Full conformance and reference-client validation remain target
work.

```text
interface Calendar {
    // Collection management.
    void create_collection(LoomHandle handle, string workspace, string principal, string collection,
                           bytes metadata);
    optional bytes get_collection(LoomHandle handle, string workspace, string principal,
                                  string collection);
    bytes list_collections(LoomHandle handle, string workspace, string principal);
    bool delete_collection(LoomHandle handle, string workspace, string principal, string collection);

    // Entry CRUD. `entry` is canonical CBOR for the structured record; put returns the new ETag.
    Digest put_entry(LoomHandle handle, string workspace, string principal, string collection,
                     bytes entry);
    optional bytes get_entry(LoomHandle handle, string workspace, string principal, string collection,
                             string uid);
    bool delete_entry(LoomHandle handle, string workspace, string principal, string collection,
                      string uid);
    bytes list_entries(LoomHandle handle, string workspace, string principal, string collection);

    // Search. Half-open local date-time range [from, to); returns canonical CBOR of occurrence rows.
    // Text search is currently over summary, with component filtering.
    bytes range(LoomHandle handle, string workspace, string principal, string collection,
                string from, string to);
    bytes search(LoomHandle handle, string workspace, string principal, string collection,
                 string component, string text);

    // Projection.
    optional bytes to_ics(LoomHandle handle, string workspace, string principal, string collection,
                          string uid);
}
```

An absent entry/collection reads as absent (no value / empty list), not an error. `put_entry` validates
and stores the structured record in the workspace working tree. `put_ics` exists in core as the
validated write-in path used by the VFS overlay; a public IDL method for `put_ics` can be added when a
binding needs text-first writes outside the mount path. `changes_since` and derived-index maintenance
remain target work.

## 5. Bindings

The facade projects across all binding families exactly as the other local facets do: C ABI
(`loom_cal_*`), C header, then Node, Python, C++, Swift/iOS, JVM, Android, React Native, and WASM.
Structured records and canonical CBOR result payloads cross as byte arrays; ETags cross as `algo:hex`
strings. The current binding surface is collection management, entry CRUD, range/search, and `to_ics`;
sync-token/change-feed APIs remain target work with 0003d and hosted CalDAV.

## 6. Hosted CalDAV projection (target)

A hosted projection serves CalDAV (RFC 4791, over WebDAV/RFC 4918, over HTTP) on a configurable port so
standard clients (Apple Calendar, Thunderbird, DAVx5, Fantastical) connect directly. This is target
work gated on 0008 (hosted protocols), 0026 (principals/authentication), and 0027 (access control).

Required surface:

- **Discovery:** `/.well-known/caldav` redirect to the CalDAV root; principal discovery
  (`current-user-principal`), calendar-home-set, and per-principal collection enumeration.
- **Methods:** `OPTIONS` (advertise `DAV: 1, 2, 3, calendar-access`), `PROPFIND` (collection and
  resource properties incl. `getetag`, `getcontenttype`, `displayname`, `supported-calendar-component-set`,
  `calendar-color`), `REPORT` (`calendar-query` with `time-range`/`comp-filter`, `calendar-multiget`,
  `sync-collection`), `GET`/`PUT`/`DELETE` of `.ics` resources, `MKCALENDAR`, and collection `DELETE`.
- **Concurrency:** `ETag`/`If-Match`/`If-None-Match` map to the content-address ETag; a mismatched
  precondition is `412 Precondition Failed`.
- **Authentication:** authenticated connections per 0026 (the principal in the URL space must match the
  authenticated principal unless 0027 grants delegated access); transport auth and TLS are deployment
  concerns of 0008. CalDAV clients present username + password (L3 in 0026 §5.4); a biometric/passkey
  principal connects via an app-specific credential (0026 §5.5), the same hosted-auth model SQL and
  other endpoints use - nothing here is CalDAV-specific.
- **Sync:** `sync-collection` uses the sync-token = commit mapping (section 2.3); `getctag` (collection
  tag) maps to the collection subtree head.
- **Apple Reminders:** account setup exposes task-list behavior through the same CalDAV/iCalendar
  `VTODO` surface. Reference-client certification must verify Apple Reminders can discover, read, create,
  edit, complete, and sync `VTODO` resources before the hosted calendar target closes.
- **Scheduling:** iTIP/iMIP (invitations, free/busy by email) is explicitly out of scope for v1
  (RD6); `calendar-query` free-busy may be added later.

## 7. Dependencies and gating

- **Local facet (sections 1-5)** depends on 0003 (reserved-path writes), 0014/0014a (workspaces and
  facet interoperability), 0010/0025 (conformance), and 0003c for the mount projection. It is
  source-backed now using an opaque principal string.
- **Filesystem projection (section 3)** depends on 0003c.
- **Principal binding and ACL** depend on 0026 and 0027; until then the principal is an opaque label
  and access control is out of contract.
- **Hosted CalDAV (section 6)** depends on 0008, 0026, 0027, and optionally 0035 (push). It is target
  work and does not gate the local facet.

## 8. Resolved decisions

- **RD1 - Structured source of truth; iCalendar is a projection.** The authoritative store is a
  structured, typed entry record (committed workspace content, key = UID); iCalendar text, the `.ics`
  file, and the CalDAV body are serialized from it on demand. This supersedes the earlier
  "canonical-bytes-authoritative" position: storing the wire text would make querying, filtering, and
  per-entry commit diff operate on messy text, which is wrong for an agent-first system. CalDAV/CardDAV
  define the wire protocol, not storage, so projecting on demand is compliant.
- **RD1a - Lossless via an extension bag + record-derived ETag.** The record carries a typed bag for
  unknown/`X-`/vendor properties so round-trip is lossless; the ETag is the content address of the
  canonical record (stable across re-serialization, changes iff the record changes). Round-trip
  conformance asserts parse/store/serialize/parse semantic identity.
- **RD1b - Quirks at projection.** Client dialect handling (vCard 3.0 vs 4.0, Apple `X-APPLE-*`,
  `VTIMEZONE` emission) is User-Agent-keyed at the serialization layer, never in storage.
- **RD2 - Calendars and task lists in one facet.** VEVENT calendars and VTODO task lists are the same
  facet, distinguished per collection by `supported-calendar-component-set`, matching CalDAV.
- **RD3 - ETag is the content address.** An entry's ETag is its Loom content address over the canonical
  bytes.
- **RD4 - Recurrence stored, expanded at query.** A recurring entry is stored as its master plus
  override components under one `UID` (one resource). The index records the `RRULE` and the master
  bounds; `time-range` query expands occurrences at query time within the requested window rather than
  materializing every instance. (Bounded-expansion limits are an implementation detail.)
- **RD5 - sync-token is a commit.** Target CalDAV `sync-collection` uses a Loom commit digest as the
  sync-token and maps to 0003d's commit-to-commit diff over the collection subtree. The current local
  facade does not expose `changes_since`.
- **RD6 - No scheduling in v1.** iTIP/iMIP scheduling (email invitations, attendee replies) is out of
  scope for v1; the facet stores and serves resources but does not send or process invitations.
- **RD7 - One resource per UID.** A calendar resource is one `VCALENDAR` for one `UID` (master plus
  recurrence overrides), per CalDAV; multi-`UID` resources are rejected with `INVALID_ARGUMENT`.
- **RD8 - Collection identity target is a UUID.** Source currently accepts a caller-supplied validated
  collection segment. Hosted/server-managed collections should use a server-assigned UUID path segment
  with display name as metadata, so duplicate display names do not collide and renames do not relocate
  resources.
- **RD9 - Search target is property-filtered, not substring-only.** Current source supports component
  filtering, summary substring search, and recurrence-aware range queries by scanning structured
  records. The hosted target adds a derived index for the CalDAV `comp-filter`/`prop-filter`/`time-range`
  model.
- **RD10 - Recurrence via an owned engine, not the `rrule` crate.** Time-range expansion uses a
  Loom-owned RFC 5545 RRULE/RDATE/EXDATE engine built over the lean `time` crate (proleptic-Gregorian
  wall-clock math), with UTC offsets resolved from each resource's embedded `VTIMEZONE` rather than a
  global timezone database. The same engine computes `VTIMEZONE` STANDARD/DAYLIGHT transitions (which
  are themselves RRULEs), so one component serves both event recurrence and tz resolution. Footprint
  probe (`prototypes/size-probes/compare-calendar-size.sh`, release+stripped): baseline ~0.29 MiB; the
  `time` substrate measures `== baseline` (~0 added); `icalendar` +48 KiB; the `rrule` crate +3.0 MiB
  because `chrono-tz` embeds the full IANA database. The owned engine removes that 3 MiB from every
  binding (notably wasm/mobile), behaves identically on all platforms, and is self-consistent (the
  resource is self-describing). Expansion is a pure deterministic function of
  (rule, dtstart, vtimezone, window), certified by vectors vendored from the RFC 5545 examples and the
  public python-dateutil/libical RRULE corpora. `icalendar` is retained for parse/serialize (+48 KiB is
  not worth reimplementing an RFC 5545 tokenizer).

## Dependency footprint (probe)

`icalendar` (parse/build) and `rrule` (RECURRENCE expansion) are the two candidate dependencies. Their
release+stripped binary cost, measured by `prototypes/size-probes` (features `ical`, `rrule`,
`ical_rrule` against `baseline`):

| Probe | Size | Delta vs baseline |
| --- | --- | --- |
| baseline | ~0.29 MiB | - |
| `icalendar` | ~0.34 MiB | ~+48 KiB |
| `rrule` | ~3.12 MiB | ~+3.0 MiB (chrono-tz IANA database) |
| `icalendar` + `rrule` | ~3.16 MiB | ~+2.9 MiB |

Implication: adopt `icalendar` freely; treat `rrule`'s timezone-database footprint as a binding-slice
prerequisite per RD10 (slim chrono-tz or gate rrule out of wasm) rather than shipping a 3 MiB tz
database into the browser bundle.

## 9. Implementation plan (ordered slices)

Priority bands (uniform across calendar/contacts/mail): **P0 = the local facet end to end - core,
IDL/C ABI/conformance, CLI, language bindings, and the filesystem mount projection**; **P1 = the MCP
surface** (agent-first tool calls over the same facade); **P2 = the hosted CalDAV wire projection**
(gated on the auth tier). Filesystem projection is P0 because the mount is the primary agent and human
surface and adds no new auth dependency; the hosted wire server waits on 0008/0026/0027.

0. **(P0) Owned RRULE + VTIMEZONE engine (sub-slice 1a, prerequisite).** A `loom-rrule` engine (or
   `loom-core::rrule` module) over the `time` crate: parse and expand RFC 5545 `RRULE`/`RDATE`/`EXDATE`
   (full FREQ set, INTERVAL, COUNT/UNTIL, the BYxxx expand/limit matrix incl. `BYSETPOS`, ordinal
   `BYDAY`, `BYWEEKNO` ISO weeks, `WKST`) over a bounded window; resolve UTC offsets from a parsed
   `VTIMEZONE` (reusing the same expander for STANDARD/DAYLIGHT transitions). No `chrono-tz`. Conformance
   vectors vendored from RFC 5545 examples and the python-dateutil/libical RRULE corpora. The calendar
   facet and its tz resolver depend on this.
1. **(P0) Core facet over structured records.** `loom-core::calendar`: collection create/list/delete and
   entry put/get/delete/list over reserved paths `.loom/facets/calendar/<principal>/<collection>/...`,
   storing the typed entry record (iCalendar validated on parse, single `UID`) with `.collection`
   metadata. Add `FacetKind::Calendar`. Unit tests; absent-reads-as-empty.
2. **(P0) Range/search over structured records.** Implement `range` and `search` directly from stored
   records. A derived SQL index is target optimization for hosted scale, not the local source-backed
   contract.
3. **(P0/P1) ETag plus helper diff.** ETag = content address of the record. `diff_entries` is
   source-backed as a helper over two collection states; public `changes_since` is target work through
   0003d/hosted CalDAV.
4. **(P0) IDL + C ABI + header + executable facade conformance.** `interface Calendar`, `loom_cal_*`,
   header, `run_calendar_facade_behavior` (collection lifecycle, entry CRUD, range, recurrence-at-query,
   helper diff, clone reachability); flip the `calendar` capability to executable in the registry and
   0010 section 5.
5. **(P0) Bindings.** Project `loom_cal_*` across Node, Python, C++, Swift/iOS, JVM, Android, React
   Native, and WASM (one pass).
6. **(P0) Filesystem projection.** Serialize entries as `.ics` on demand under the mount; validated
   write-in (parse to record on write). The mount is the primary local surface, so it ships with the core
   facet rather than after it.
7. **(P1) MCP surface.** Expose the facade as MCP tools (collection + entry CRUD, range, search, and
   later change feeds) for agent-first use over the same structured records. No new auth dependency
   beyond the local facet.
8. **(P2) Hosted CalDAV projection.** The bounded source-backed profile serves `.well-known/caldav`,
   OPTIONS, PROPFIND, MKCALENDAR, GET, PUT, DELETE, ETag preconditions, `calendar-multiget`,
   `calendar-query`, commit-backed `sync-collection` with tombstones, direct TLS through hosted
   certificate-bundle listener records, bounded RFC-gate rows, and owner-verified reference-client
   evidence. Scheduling, free-busy, delegated sharing, durable certification administration, and
   broader ACL-aware hosted-server policy remain target work.

Slices 0-8 plus the local CLI projection are source-backed for the bounded profile described above.

## Change log

### Slice 0 (P0) - owned RRULE + VTIMEZONE engine: source-backed

The `loom-rrule` workspace crate (lib `loom_rrule`) implements the recurrence prerequisite end to end
over the lean `time` crate (no `chrono-tz`):

- `RRule::parse` + `RRule::expand(dtstart, from, to)` cover the full RFC 5545 `FREQ` set
  (SECONDLY/MINUTELY/HOURLY/DAILY/WEEKLY/MONTHLY/YEARLY) with `INTERVAL`, `COUNT`, `UNTIL`, `WKST`, and
  the complete BY-rule set - `BYSECOND`/`BYMINUTE`/`BYHOUR`/`BYDAY` (ordinals incl. negative)/
  `BYMONTHDAY` (neg)/`BYYEARDAY` (neg)/`BYWEEKNO` (ISO)/`BYMONTH`/`BYSETPOS` - applied per the RFC 5545
  expand/limit matrix over a bounded window. `BYSETPOS` selects over the full period set before the
  `dtstart` lower bound, so the first partial period's set-positions are correct.
- `RecurrenceSet` composes one-or-more `RRULE` with `RDATE` and `EXDATE`, with `DTSTART` always the first
  instance and `EXDATE` removing last (RD10).
- `VTimeZone::parse` + `offset_at_utc` + `to_utc` resolve UTC offsets from the resource's own
  `VTIMEZONE`, reusing the same expander to enumerate STANDARD/DAYLIGHT transitions - one recurrence
  engine for both event recurrence and tz resolution, no embedded zone database (RD10).
- Tests: 20 in-crate unit tests plus 10 conformance vectors vendored from RFC 5545 section 3.8.5.3 and
  the python-dateutil/libical corpora (`tests/rfc5545_vectors.rs`), green under
  `RUSTFLAGS="-D warnings"`. `deny.toml` carries the `uldren-loom-rrule` BUSL exception; the footprint
  probe (RD10) confirms the `time` substrate adds ~0 over baseline.

Remaining for the calendar facet proper: slices 4-6 (IDL/ABI/conformance, bindings, and `.ics` mount
projection).

### Slices 1-3 (P0) - core structured-record facet and range/search: source-backed

`loom-core::calendar` implements the local facet over structured records (RD1), with `FacetKind::Calendar`
added (and registered in `as_str`/`from_str`, the compression policy, the capability registry as
`calendar` = source-backed, and the 0010 section 5 table, kept in lock-step by the drift test):

- **Storage (slice 1).** `CalendarEntry` is the typed source of truth (uid, component, summary, dtstart,
  dtend, tzid, rrule, rdate, exdate, status, plus an `extra` extension bag for lossless round-trip, RD1a),
  encoded as canonical CBOR; the ETag is the content address of those bytes (RD3). Entries live at
  `calendar/<principal>/<collection>/<uid>` (0042 `principal > collection`, one resource per UID, RD7;
  the UID is hex-encoded into the path segment). `CollectionMeta` carries the display name and
  `component_set` (RD2). Verbs: `create_collection`/`get_collection`/`list_collections`/
  `delete_collection` and `put_entry`/`get_entry`/`delete_entry`/`list_entries`; a put into a missing
  collection is `NOT_FOUND` (CalDAV requires MKCALENDAR first), an empty/invalid UID or DTSTART is
  `INVALID_ARGUMENT`. Entries version/branch/checkout with commits like any reserved content.
- **Range and search (slice 2).** `range(from, to)` expands every entry's `RecurrenceSet` via `loom-rrule`
  within the half-open window, ordered by start then UID (RD4); `search` filters by component and a
  case-insensitive summary substring (the property-filtered model of RD9). Both are derived on demand from
  the records (no materialized index), so they are always consistent with stored state.
- **Helper diff (slice 3).** `diff_entries(old, new)` reports per-UID `Added`/`Updated`/`Removed` with
  the new ETag, the structured form needed by future CalDAV `sync-collection` work (RD5). Public
  commit-token `changes_since` remains target through 0003d. ETag = record content address throughout.
- Tests: 8 in-crate tests (record round-trip, collection lifecycle, entry CRUD with ETag-changes-on-edit,
  missing-collection NOT_FOUND, commit versioning, recurrence range with EXDATE, search, diff), green
  under `RUSTFLAGS="-D warnings"`; the capability drift test and workspace tests pass with the new facet.

### Slice 4 (P0) - IDL + executable conformance: source-backed (capability now executable)

- **IDL.** `interface Calendar` is in `idl/loom.idl` (the local interface source for the C ABI and
  language bindings; hosted CalDAV remains target under 0008):
  `create_collection`/`get_collection`/`list_collections`/`delete_collection`,
  `put_entry`/`get_entry`/`delete_entry`/`list_entries`, `range`, `search`, and `to_ics`. Entry records and
  collection metadata cross as Loom Canonical CBOR; `put_entry` returns the `Digest` ETag.
- **Executable conformance.** `run_calendar_facade_behavior` (loom-conformance) exercises the full facade
  against `MemoryStore` plus a clone destination: MKCALENDAR-before-PUT (`NOT_FOUND`), entry CRUD with an
  ETag that changes on edit, EXDATE-aware recurrence `range`, `search`, the `.ics` projection round-trip,
  `diff_entries`, commit/checkout versioning, and clone reachability. It is wired into
  `certify_memory_store`, listed in `BEHAVIOR_SUITES` (with a declarative `CALENDAR_SCENARIOS` table) and
  `EXECUTABLE_BEHAVIOR_SUITES`, and the aggregate certification test passes (34 conformance tests green).
- **Capability flipped to executable.** The `calendar` capability is now `executable` in both the source
  registry and the 0010 section 5 table (kept in lock-step by the drift test).
- Slice 5 (the eight language bindings over `loom_cal_*`) is the remaining binding pass; it builds on this
  IDL and is run through the per-binding toolchain recipes.

### Slice 5 (P0) - bindings: source-backed (C ABI verified; language bindings pending toolchain build)

Done as one pass across the calendar/contacts/mail trio. The C ABI (`loom_cal_*`, 11 functions) is in
`crates/loom-ffi` with `ensure_cal_ns`, records crossing as their CBOR (`CalendarEntry::encode`/`decode`),
list/search returns as a canonical-CBOR `Array(Bytes)`, `range` as `Array([uid, "YYYYMMDDTHHMMSS"])`, and
`create_collection` taking `display_name` + a comma-separated component string; both C headers
(`include/loom.h` and the iOS copy) regenerated via cbindgen and in sync; a
`calendar_contacts_mail_round_trip_over_the_c_abi` test added (loom-ffi: 36 tests green under
`-D warnings`). The eight language bindings (Node, Python, C++, Swift/iOS, JVM, Android, React Native,
WASM) project the same surface, mirroring the existing kv/document wrappers; the cargo-excluded bindings
build through their own toolchains (`just test-bindings`). Note: the Node/Python/WASM bindings call `loom-core`
directly (not the C ABI), so they each gained a `loom-codec` dep and the list-encoding helpers.

### Slice 6 (partial, P0) - iCalendar (.ics) projection codec: source-backed

The projection codec - the substance of the filesystem (and later CalDAV) surface - is implemented and
tested in `loom-core::calendar`:

- `CalendarEntry::to_ics` serializes a record to a one-component `VCALENDAR` (RFC 5545: TEXT escaping,
  `DTSTART;TZID=` parameters, 75-octet line folding); `CalendarEntry::from_ics` parses it back, routing
  unknown / `X-` properties into the `extra` bag so the round-trip is lossless (RD1a). Facet helpers
  `entry_ics` (serialize the stored record on demand) and `put_ics` (parse-and-store, the validated
  write-in path) bracket it. Tests cover a semantic round-trip with escaping/TZID/RRULE/EXDATE/extras, a
  `VTODO` with long-line folding, and a facet-level put/get-ics.
### Slice 6b (P0) - VFS facet overlay (model A): mechanism source-backed (shared with 0038/0039)

The mount overlay is implemented as the portable module `loom-vfs::overlay`, layering facet behaviour on
the `calendar/`, `contacts/`, and `mail/` mount roots (one mechanism for all three). **Model A** (the
owner decision): a facet-extension file (`.ics`/`.vcf`/`.eml`) under `<root>/<principal>/<collection>/`
is the *projection* of a structured record (record = source of truth, RD1; raw wire bytes not stored);
any other file (a `.jpg`, a sidecar) is an ordinary working-tree file, stored verbatim - **no denylist**.

Owner-confirmed semantics: **(D1)** a write-in leaves the change **unstaged** in the working tree; only
an explicit `vcs` commit persists it. **(D2)** a facet-extension file that fails to parse is
**quarantined** - the raw bytes stay as an ordinary file at that path (so it is still "there") and
processing metadata records the error; no record is created; a later valid re-upload of the same name
supersedes the quarantine. **(D3)** any file is allowed. **(D4)** per-file processing metadata is exposed
as **xattrs** (`user.loom.status`/`error`/`etag`); no synthetic `/_status` or `/_errors` views. **(D5)**
bulk/batch ingestion is out of scope here (later only).

The module provides `classify` (path -> facet file or not), `ingest` (parse-and-store on flush, or
quarantine; a missing collection is propagated as `NOT_FOUND`, never quarantined), `project`
(serialize the record on read, else fall through to an ordinary read of a quarantined/arbitrary file),
`processing`/`Processing::xattrs` (the metadata surface), and `list_projected` (record names to merge
into a directory listing). Mail uses the filename stem as the message uid; calendar/contacts take the
authoritative uid from the parsed content (the dropped name need not match, CalDAV-style). Ingestion is
**synchronous** on write-in (the FUSE/NFS write path blocks); there is no async queue. 8 unit tests cover
the happy path (valid file -> projected record), quarantine (+ xattr error + raw kept), fix-supersedes-
quarantine, missing-collection NOT_FOUND, commit/checkout durability across restart, and the mail
stem-as-uid path; green under `RUSTFLAGS="-D warnings"`.

- **Projection integration (done).** The overlay is wired into `loom-vfs::Projection`: a write persists
  as an ordinary file immediately (no data loss on any backend); `flush_overlay(ino)` parses that file
  into a record (removing the raw file) or quarantines it; `read`/`getattr`/`lookup` project a record as
  a regular file; `readdir` merges `list_projected` with the ordinary entries; `mkdir` of a
  `<root>/<principal>/<collection>` directory also creates the backing facet collection; `getxattr`/
  `listxattr` expose the processing metadata. 11 loom-vfs tests (8 overlay + 3 Projection end-to-end:
  ingest+project, quarantine+xattr, arbitrary-file passthrough), green under `-D warnings`.
- **FUSE backend (done).** `loom-vfs-fuse` drives ingestion on `flush`/`fsync` (calling `flush_overlay`)
  and serves `getxattr`/`listxattr` from the processing metadata; reads/readdir/lookup are overlay-aware
  via the shared `Projection`. Builds under `-D warnings` (a real mount is not exercisable in the test
  sandbox).
- **NFS backend + the close-signal problem (investigated).** NFSv3 is stateless (no open/close); the
  only completion signal is the COMMIT RPC. The `nfsserve` crate's `NFSFileSystem` trait exposes **no**
  `commit`/`fsync`/`flush` method, and its WRITE handler advertises `committed: FILE_SYNC`, so clients
  treat every write as durable and never send COMMIT. So routing parse-on-close from NFS (option D) is
  **not viable without forking nfsserve** on two counts; rejected. NFS still gets the full read side
  (projection, readdir merge, getattr) through the shared `Projection`, and a dropped facet file persists
  as a raw working-tree file (no data loss) until reconciled.
- **Reconcile (the close-signal-free finalize, generic over all three facets).** `loom-vfs::overlay`
  provides `pending_facet_files` (raw facet files with no record yet, across `calendar/`/`contacts/`/
  `mail/`), `reconcile` (pass B - ingest all pending now; for an explicit command or a 0029 trigger),
  and `reconcile_quiescent` (pass C - ingest only files whose content was stable since the previous tick,
  so a file still being written is never finalized mid-write; the caller holds the cross-tick state and a
  timer drives it). FUSE still finalizes immediately on flush; reconcile is how NFS (and any
  close-signal-free backend) finalizes. 22 loom-vfs tests green (incl. reconcile-now, quiescent-waits,
  and quiescent-skips-a-changing-file).
- **Remaining (later).** Wire the FUSE/NFS backends or the CLI/0029 keeper to call `reconcile`/
  `reconcile_quiescent` on a cadence; a live management-portal projection over the processing metadata +
  the 0030 watch feed (the monitoring surface for bulk ingest).

### Slice 7 (P1) - MCP projection: source-backed

### Codec library adoption - `icalendar` (supersedes the hand-rolled .ics codec)

The `.ics` projection codec now uses the **`icalendar`** crate (0.17, `default-features=false`,
`features=["parser"]`) rather than a hand-rolled serializer/parser: `to_ics` builds via the high-level
`Calendar`/`Event`/`Todo` builder; `from_ics` parses via `icalendar::parser::read_calendar` (robust RFC
5545 - folding, escaping, parameters, multi-component). The structured record stays the source of truth
and recurrence stays the owned `loom-rrule` engine (icalendar's `recurrence` feature and `chrono-tz` are
OFF; it pulls a no-tz `chrono`, already present via gluesql, ~+48 KiB). `icalendar` injects a generated
`DTSTAMP` on build; it is transport metadata regenerated on each serialize and is ignored on parse (not
routed into the `extra` bag), like `VERSION`/`PRODID`. Round-trip is semantic (`from_ics(to_ics) == e`),
not byte-exact, since the library owns the wire format. Licensed MIT/Apache, deny.toml-clean. 11 calendar
tests + the conformance runner green.

`crates/loom-mcp` projects calendar through curated tools, resources, and prompts. The tool surface
includes `calendar_create_collection`, `calendar_get_collection`, `calendar_list_collections`,
`calendar_delete_collection`, `calendar_put_entry`, `calendar_get_entry`, `calendar_delete_entry`,
`calendar_list_entries`, `calendar_range`, `calendar_search`, and `calendar_to_ics`. The MCP resource
surface serves `.ics` bodies from `loom://.../calendar/...`, and the prompt surface includes the calendar
workflow prompts. The server binding removes `principal` from agent-visible schemas for calendar tools
and injects the bound principal before dispatch.

The first hosted CalDAV runtime is source-backed in `crates/loom-hosted/src/serve.rs` and
`crates/loom-cli/src/daemon_cmd.rs`: durable `calendar/caldav` listener records open a bounded WebDAV
profile for `.well-known/caldav`, OPTIONS, PROPFIND, MKCALENDAR, GET, PUT, DELETE, conditional writes,
`calendar-multiget`, `calendar-query`, and commit-backed `sync-collection` over the existing `.ics`
codec and hosted kernel auth/PEP. Focused tests cover hosted router behavior and daemon-opened listener
startup, including direct TLS over hosted certificate-bundle listener records. Queue 7 reference-client
validation is owner-verified for Apple Calendar and Reminders, Thunderbird, and DAVx5 against the local
certification harness; durable redacted transcript storage and admin-visible certification status remain
0065 target work. Full CalDAV remains target work for scheduling, free-busy, delegated sharing,
ACL-aware serving, and lifecycle-hook administration.

### Reusable PIM component extraction - source-backed

`loom-pim` now owns the calendar local record contracts and projection helpers: `CalendarEntry`,
`CollectionMeta`, `Component`, canonical CBOR encode/decode, `.ics` parse/serialize through
`icalendar`, and recurrence expansion through `loom-rrule`. `loom-core::calendar` consumes and
re-exports those contracts while retaining workspace, ACL, reserved-path storage, collection CRUD,
entry CRUD, ETag calculation, search, range facade wiring, and `FacetKind::Calendar` integration.
Existing callers keep the `loom_core::calendar::*` surface. Component-level record/projection tests
live in `loom-pim`; executable facade conformance stays in `loom-conformance` because it proves engine
storage, versioning, clone reachability, and workspace integration.

## Hosted CalDAV Projection

Hosted CalDAV completion is owned jointly by this spec and 0008. The bounded owner-only profile is
source-backed for the rows listed here; the deferred full profile remains target work.

Source-backed bounded profile:

- consume the shared 0008 WebDAV substrate for path escaping, XML parsing, multistatus output,
  conditional writes, sync-token behavior, hosted auth, PEP, request limits, audit, and store-save;
- return DAV collection and principal discovery properties plus CalDAV `calendar-home-set`,
  `calendar-user-address-set`, `supported-calendar-component-set`, `supported-calendar-data`, `getctag`,
  and `sync-token`;
- implement `calendar-multiget` with per-resource `200` and `404` rows;
- implement a deterministic `calendar-query` subset over structured records: `VEVENT` and `VTODO`,
  optional `time-range`, optional UID text match, optional summary text match, `getetag`, and
  `calendar-data`;
- evaluate `time-range` through bounded recurrence expansion over the structured recurrence model;
- apply `If-Match` and `If-None-Match` to PUT and DELETE using the structured-record digest as the ETag;
- keep `sync-collection` commit-digest tokens and per-UID diffs source-backed, including tombstones and
  stable invalid-token errors;
- keep iCalendar dialect handling as projection logic, with User-Agent hooks allowed only when client
  transcripts prove the need;

Deferred full-profile rows:

- keep scheduling inbox/outbox, iTIP/iMIP send and receive, free-busy query, delegated sharing, and
  broader ACL-aware serving as explicit deferred rows unless promoted by a later 0026/0027 policy slice;
- keep durable certification profile selection, transcript storage, certification status projection,
  and admin review in 0065 rather than in this facet spec.

### CalDAV RFC implementation gate

Queue 7 cannot certify reference clients until this gate is either source-backed or explicitly recorded
as unsupported, degraded, target, or deferred in the hosted capability report. Shared HTTP, TLS, URI,
discovery, Basic auth, and WebDAV mechanics are owned by 0008. This table owns calendar-specific
protocol and data behavior. The hosted capability inventory splits this gate into
`calendar/rfc-gate/caldav-rfc4791-bounded-access-profile`,
`calendar/rfc-gate/icalendar-rfc5545-bounded-profile`,
`calendar/rfc-gate/caldav-rfc4791-rfc5545-bounded-profile`,
`calendar/rfc-gate/itip-rfc5546`,
`calendar/rfc-gate/imip-rfc6047`,
`calendar/rfc-gate/caldav-scheduling-rfc6638`,
`calendar/rfc-gate/scheduling-itip-imip-freebusy`,
`calendar/rfc-gate/non-gregorian-recurrence-rfc7529`,
`calendar/rfc-gate/non-gregorian-recurrence`,
`calendar/rfc-gate/timezone-reference-rfc7809`,
`calendar/rfc-gate/availability-rfc7953`,
`calendar/rfc-gate/timezone-reference-and-availability`, and
`calendar/rfc-gate/rfc7986-extra-properties`.

| RFC | Area | Acceptance rule |
| --- | --- | --- |
| RFC 4791 | CalDAV access | Supported for the bounded owner-only profile: advertised CalDAV collection and principal discovery properties, `current-user-principal`, `principal-URL`, `owner`, `current-user-privilege-set`, `calendar-home-set`, `calendar-user-address-set`, `supported-calendar-component-set`, `supported-calendar-data`, `calendar-multiget`, `calendar-query`, `MKCALENDAR` with creation-body `displayname` and component-set parsing, resource GET/PUT/DELETE, GET and REPORT ETags, sync tokens, and status mapping have source-backed tests. Scheduling inbox/outbox properties, free-busy query, COPY, and MOVE are not advertised by this profile. |
| RFC 5545 | iCalendar | Supported for the bounded single-resource owner-only profile: `VCALENDAR` with one `VEVENT` or `VTODO`, required `UID`, required `DTSTART` for `VEVENT`, optional `DTSTART` for `VTODO`, optional `DTEND`, `SUMMARY`, `STATUS`, `TZID` parameters on date-time values, RFC 5545 TEXT escaping, 75-octet line folding and unfolding, `RRULE`, `RDATE`, `EXDATE`, bounded recurrence expansion, and extension preservation are source-backed by `loom-pim`, `loom-rrule`, local facade, and hosted CalDAV tests. `DESCRIPTION`, `SEQUENCE`, `DUE`, and `X-` properties are preserved through the extension bag. `VJOURNAL`, `VFREEBUSY`, `VALARM`, full `VTIMEZONE` service behavior, scheduling `METHOD` flows, binary attachment handling, and typed schema fields for the wider property registry are not claimed by this row. |
| RFC 5546 | iTIP | Target, not advertised in Queue 7. The hosted CalDAV profile does not advertise scheduling inbox/outbox properties, auto-schedule properties, iTIP `METHOD` flows, or organizer/attendee scheduling state. Any future promotion must add scheduling inbox/outbox behavior, `PUBLISH`/`REQUEST`/`REPLY`/`ADD`/`CANCEL`/`REFRESH`/`COUNTER`/`DECLINECOUNTER` method handling, identity checks, sequence handling, recurrence-instance scheduling behavior, status replies, and transcript evidence. |
| RFC 6047 | iMIP | Target, not advertised. Email-based scheduling remains deferred until real mail submission and delivery semantics exist. The setup-only hosted SMTP compatibility listener in 0039 accepts probe `DATA` but does not relay, deliver, mutate the mail facet, parse `text/calendar` MIME parts, verify `Content-Type` `method` parameters against iCalendar `METHOD`, authenticate organizer/attendee authority, or process iTIP messages. |
| RFC 6638 | CalDAV scheduling | Target, not advertised. The bounded owner-only profile may expose `calendar-user-address-set` for principal discovery, but it does not advertise the `calendar-auto-schedule` DAV feature, scheduling inbox/outbox URLs, scheduling inbox/outbox collections, scheduling privileges, schedule tags, schedule response bodies, free-busy requests, or organizer/attendee implicit scheduling flows. Any promotion requires a later 0026/0027 policy slice plus source-backed scheduling delivery, identity, conflict, and transcript evidence. |
| RFC 7529 | Non-Gregorian recurrence | Unsupported in the bounded profile. `RSCALE`, `SKIP`, leap-month `BYMONTH` values, and `supported-rscale-set` are not advertised, parsed, or expanded. Unknown non-`RRULE` properties remain preserved through the extension bag, but RFC 7529 recurrence rules fail as unsupported rather than being interpreted as Gregorian recurrence. |
| RFC 7809 | Time zones by reference | Target, not advertised. The hosted CalDAV profile does not advertise `calendar-no-timezone`, `timezone-service-set`, `calendar-timezone-id`, `timezone-id`, or `CalDAV-Timezones` behavior. Timezone-by-reference promotion requires a hosted timezone distribution service, policy for known and unknown time zones, `VTIMEZONE` inclusion or suppression behavior, request-header handling, `calendar-query` timezone-id support, and reference tests. Inline `TZID` preservation and structured recurrence remain the Queue 7 path. |
| RFC 7953 | Calendar availability | Target, not advertised. The bounded profile has no `VAVAILABILITY` or `AVAILABLE` record model, does not include those components in `supported-calendar-component-set`, does not advertise `free-busy-query`, and does not evaluate availability during free-busy. Promotion requires a typed availability schema, recurrence-aware availability evaluation, free-busy integration, CalDAV scheduling policy, and reference tests. |
| RFC 7986 | New iCalendar properties | Degraded. Component-level RFC 7986 property values such as `COLOR`, `IMAGE`, and `CONFERENCE` are preserved through the extension bag for `VEVENT` and `VTODO`. Top-level `VCALENDAR` properties (`NAME`, calendar-level `DESCRIPTION`, calendar-level `UID`, `LAST-MODIFIED`, `URL`, `CATEGORIES`, `REFRESH-INTERVAL`, `SOURCE`, calendar-level `COLOR`, calendar-level `IMAGE`), property parameters (`DISPLAY`, `EMAIL`, `FEATURE`, `LABEL`), and typed schema fields remain target until the record model carries them explicitly. |

Reference-client certification for the bounded owner-only target is Apple Calendar or iOS Calendar,
Apple Reminders for `VTODO`, Thunderbird, and DAVx5. Queue 7 task 90.2 is owner-verified green against
the local certification harness. The conformance crate records the durable requirement and redacted
transcript fixture shape through `PIM_CERTIFICATION_CLIENT_REQUIREMENTS`,
`PIM_TRANSCRIPT_INVENTORY`, and `QUEUE7_PIM_CAPABILITY_REPORT_FIXTURES`; 0065 owns the later admin
interface and durable certification record. Fantastical remains advisory unless promoted later.

### Queue 7 closure evidence

| Evidence area | Disposition | Source-backed references |
| --- | --- | --- |
| Bounded CalDAV access | Supported. Discovery, principal/home-set properties, collection properties, resource GET/PUT/DELETE, `MKCALENDAR`, `calendar-multiget`, `calendar-query`, ETags, and sync tokens are source-backed. Scheduling, free-busy, COPY, and MOVE are not advertised. | `crates/loom-hosted/src/serve.rs`; `crates/loom-conformance/src/lib.rs` rows `calendar/rfc-gate/caldav-rfc4791-bounded-access-profile` and `calendar/rfc-gate/caldav-rfc4791-rfc5545-bounded-profile`; Queue 7 task 30.5.1. |
| Bounded iCalendar projection | Supported for one `VEVENT` or `VTODO` per resource, required UID, event `DTSTART`, optional todo `DTSTART`, optional `DTEND`, `SUMMARY`, `STATUS`, `TZID`, TEXT escaping, line folding, recurrence, bounded expansion, and extension preservation. Wider property registry typing remains target or degraded as listed in the RFC gate. | `crates/loom-pim/src/calendar.rs`; `crates/loom-rrule/src/lib.rs`; `crates/loom-conformance/src/lib.rs` row `calendar/rfc-gate/icalendar-rfc5545-bounded-profile`; Queue 7 task 30.5.2. |
| Direct TLS and hosted listener shape | Supported for daemon-opened `calendar/caldav` listener records, certificate-bundle loading, and shared-port DAV coalescing through 0008. Durable service identity and trust-anchor administration remain 0065 target work. | `crates/loom-cli/src/daemon_cmd.rs`; `crates/loom-conformance/src/lib.rs` rows `calendar/caldav/direct-tls`, `pim/rfc-gate/tls-rfc8996-modern-versions`, and `pim/rfc-gate/shared-http-over-tls-service-identity`; Queue 7 task 45. |
| Lifecycle hooks | Source-backed for registration records, canonical event envelopes, matching, calendar event emission, and execution-policy planning. Hosted hook administration and occurrence-range hook calls remain target work. | `crates/loom-core/src/hooks.rs`; `crates/loom-conformance/src/lib.rs` rows `pim/hooks/registration-envelope-event-emission` and `pim/hooks/execution-policy-planning`; Queue 7 tasks 70.1 through 70.3. |
| Owner-only access profile | Supported for the first hosted completion pass. Delegated calendars, shared task lists, service-principal sharing, and ACL-aware multi-principal serving are deferred to 0026/0027 policy work. | `specs/0008-wire-protocols.md`; `crates/loom-conformance/src/lib.rs` row `pim/access/owner-only-profile`; Queue 7 task 85. |
| Reference clients | Owner-verified for Apple Calendar, Apple Reminders, Thunderbird, and DAVx5 against the local certification harness. This is client evidence, not a substitute for the RFC rows above. Durable transcript capture and review are 0065 target work. | `_QUEUE7.md` tasks 90.2 and 90.2.1; `crates/loom-conformance/src/lib.rs` `PIM_CERTIFICATION_CLIENT_REQUIREMENTS` and `PIM_TRANSCRIPT_INVENTORY`; `scripts/pim-cert/README.md`. |
| Deferred standards | RFC 5546 iTIP, RFC 6047 iMIP, RFC 6638 CalDAV scheduling, RFC 7809 time zones by reference, and RFC 7953 availability are target and not advertised. RFC 7529 non-Gregorian recurrence is unsupported. RFC 7986 is degraded to component-property preservation. | `crates/loom-conformance/src/lib.rs` calendar RFC-gate rows; Queue 7 tasks 30.5.3 through 30.5.9. |

### Queue 10 local hardening evidence

Queue 10 closed the local non-binding hardening pass for calendar without moving the unfinished
binding parity, binding runtime, or coverage-reporting rows below. Evidence:

| Surface | Queue 10 evidence | Source-backed references |
| --- | --- | --- |
| MCP | Calendar tool schemas elide agent-supplied principal, server-side binding overwrites any scoped argument, malformed PIM arguments are rejected, calendar resource reads reject malformed or unauthorized targets, and calendar prompts stay in the registered prompt inventory. | `crates/loom-mcp/src/server/tests.rs` tests `registered_prompts_equal_the_surface`, `pim_binding_injects_and_overwrites_agent_scope`, `pim_arguments_reject_missing_or_malformed_scope_fields`, and `binding_scopes_resource_templates_and_reads`; `_QUEUE10.md` tasks 20 and 60. |
| Compute/WASM | Direct `StateAccess` and guest WASM calls cover calendar capability denial, missing collection mapping, malformed guest record bytes, and the existing positive domain-shaped calendar host-call round trip. | `crates/loom-compute/src/state_access.rs` test `pim_state_access_denies_modes_facets_and_missing_collections`; `crates/loom-compute/src/engine_wasmi.rs` tests `pim_host_abi_rejects_malformed_records_and_denied_grants` and `pim_domain_records_round_trip_through_host_abi`; `_QUEUE10.md` task 30. |
| VFS | The `.ics` overlay covers missing collection handling without quarantine and projected-record unlink through the structured calendar record path. | `crates/loom-vfs/src/overlay.rs` tests `missing_collection_is_not_quarantined` and `overlay_unlinks_projected_facet_record`; `_QUEUE10.md` task 40. |
| Local CLI and C ABI | Local smoke tests cover missing calendar collections through the CLI and missing or malformed calendar inputs through the C ABI path. | `crates/loom-cli/src/helpers.rs` test `pim_cli_reports_missing_containers`; `crates/loom-ffi/src/tests.rs` test `calendar_contacts_mail_round_trip_over_the_c_abi`; `_QUEUE10.md` task 50. |

### Unfinished binding and coverage items

These unfinished spec items are not Queue 10 scope.

| Priority | Item | Status | Owning follow-up |
| --- | --- | --- | --- |
| P1 | Per-binding calendar parity matrix for Node, Python, C++, Swift/iOS, JVM, Android, React Native, WASM, C ABI, and IDL shapes across local calendar operations, `.ics` projection, capability errors, and hosted-adjacent client expectations. | Unfinished | P9 binding specs or a later binding certification queue. |
| P1 | Binding runtime tests for positive, negative, and boundary calendar cases in each language binding. | Unfinished | P9 binding specs or a later binding certification queue. |
| P2 | Coverage reporting by calendar surface and binding family, distinct from Rust line coverage. | Unfinished | 0010a conformance reporting or a later binding certification queue. |
