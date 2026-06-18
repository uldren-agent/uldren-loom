# 0030 - Observability (observing state changes in the store)

**Status:** Source-backed and complete for all 0030-owned behavior; non-owned work is delegated to owning specs. **Version:** 0.1.0-target.
**Optional facade:** capability `watch`.

**Depends on:** 0002 (the commit DAG and refs), 0003 (core interface; change feeds, 0003 §4.6), 0014
(workspaces), 0006 (sync), 0009 (the `watch` capability menu item, 0009 §8; access control). **Relates
to:** 0029 (events and triggers; the change-trigger class is a server-side subscriber that runs a
program on each observed change), 0018/0021 (the append-log/ledger that can persist a feed), 0035
(durable delivery for hosted push transports). **Widens:** the `watch` menu item (0009 §8) and the
change-feed surface (0003 §4.6) into a first-class leg.

This document specifies the observation leg over Loom: subscribing to changes in the store and
receiving them as an ordered feed. Storage holds data, version control holds history, compute (0015)
acts on the store; observability is the counterpart that lets a caller learn that the store changed,
across every event-producing domain, so that external tools and triggers (0029) can react. It is gated
by the `watch` capability; absence yields `UNSUPPORTED`.

The framing is deliberate: 0029 is the **act** side (run a program when something happens) and 0030 is
the **observe** side (learn that something happened). A change trigger is an observability subscription
whose handler is a program; an external dashboard is a subscription whose handler is a client.

## Current implementation

The Rust workspace implements the portable pull baseline for `watch`:

- `loom_core::watch` owns the kernel-integrated watch materializer. `loom-watch` is the reusable
  component boundary that exports `WatchSelector`, `WatchCursor`, `ChangeEvent`, `WatchBatch`,
  `DomainChange`, `UnsupportedDomainDetail`, path-level `WatchPathChange`, domain-support helpers,
  and canonical `loom.watch.batch.v1` encoding for hosted protocols and conformance.
- `Loom::watch_subscribe` returns a stateless encoded cursor for one workspace branch, either from a
  supplied reachable commit or from the current branch tip.
- `Loom::watch_poll` returns deterministic first-parent commit events in forward order, advances the
  cursor, maps unreachable cursors to `CURSOR_INVALID`, includes sorted file path changes, emits
  file-domain `DomainChange` records with path keys, normalized change-kind strings, and before or
  after digests where the commit diff can derive them, and preserves non-file-only workspace revisions
  with capability-labeled unsupported-domain detail markers.
- Source-backed selector narrowing supports the files domain, file path prefixes, and normalized file
  change kinds. Non-file exact-domain selectors fail with `UNSUPPORTED` until that domain promotes a
  stable watch detail contract.
- Source-backed authorization requires read access to the watched workspace ref. File watch output is
  fail-closed: explicit unauthorized path-prefix subscriptions are denied, and broad subscriptions omit
  file path changes the caller cannot read. Unsupported-domain markers are also omitted unless the
  caller can read the marked facet. Fine-grained non-file scope semantics remain owned by 0027 and
  0028.
- `loom-mcp` projects the pull baseline as read-only `watch_subscribe` and `watch_poll` tools with
  workspace-bound schema elision, normalized change-kind selector narrowing, and the source-backed
  `DataChange` envelope with nested `DomainChange` and unsupported-domain detail records. MCP App
  resource subscriptions advance watch cursors for committed app workspace changes; daemon/session
  lifecycle remains a separate MCP lifecycle path. The MCP host overlays `watch` as supported in
  `store_capabilities`.
- `uldren-loom-ffi` projects the pull baseline through `loom_watch_subscribe`,
  `loom_watch_poll`, and `loom_watch_poll_async`. The cursor crosses as an opaque UTF-8 string, and
  poll returns canonical `loom.watch.batch.v1` CBOR.
- Local bindings project the pull baseline in Node, Python, WASM, Swift, Kotlin, JVM, C++, and
  React Native as subscribe and poll operations over the same cursor and canonical batch payload.
- `uldren-loom-hosted` projects the pull baseline through REST `POST /watch:subscribe` and
  `GET /watch?cursor=...&max=...`, JSON-RPC `watch_subscribe`, `watch_poll`, and `watch.stream`, and
  gRPC `Subscribe`, `Poll`, and `Watch::Stream`. Hosted pull uses the same authorization gates,
  source cursor, selector narrowing, `DataChange` envelope, nested `DomainChange` records,
  unsupported-domain markers, and bounded poll limit as the core baseline. JSON-facing protocols encode
  domain keys and detail bytes as lowercase hex strings; gRPC carries them as bytes.
- Hosted REST also projects `GET /watch:stream` as an SSE live stream over an existing watch cursor.
  Stream frames carry the advanced source cursor as the SSE `id`, as `source_cursor`, and as `next`.
  The source-backed stream uses bounded poll batches, configurable poll interval, and a bounded
  debounce window that coalesces available committed changes into the next delivered batch. This is a
  live source stream, not a generic 0035 durable outbox.
- Hosted REST and JSON-RPC expose `watch.materialize` as an optional feed materialization hook. It
  polls from a watch cursor, encodes the resulting batch as canonical `loom.watch.batch.v1` CBOR, and
  appends that payload to a named append-log stream while returning the append sequence and advanced
  source cursor.
- Hosted JSON-RPC and gRPC now also support stream profiles over source-backed `watch` cursors:
  `watch.stream` and `Watch::Stream`.
- `crates/loom-conformance` consumes the `loom-watch` contract crate and includes an executable
  `watch` behavior suite for history order, resume, path events, file-domain change records,
  unsupported-domain markers, file selector narrowing, empty polls, and invalid cursors.

The source does not yet implement durable replay and reconnection for hosted watch streams, or non-file
domain event detail. The generic 0035 core delivery substrate is source-backed, and hosted watch
materialization can seed an append-log stream for replay, while reconnect-safe transport ack and redelivery
remain 0035 work. Everything after this section is the full target observation contract unless it explicitly cites
the portable pull baseline.

## 1. Goals & non-goals

**Goals.** (G1) Observe state changes in one workspace as an ordered, resumable feed. (G2) Provide the
common observation spine for every event-producing domain: workspace/ref ordering, source cursors,
authorization narrowing, projection, and delivery handoff. (G3) Carry domain-owned change records for
files, KV, SQL, vector, graph, ledger, and other promoted domains without making 0030 own every domain
event vocabulary (§3). (G4) Scope a subscription narrowly (by ref, path or key prefix, domain, and
change kind, §4). (G5) Deliver deterministically and idempotently: a feed is a forward walk of the
commit DAG, so two observers from the same cursor see the same events in the same order. (G6) Provide
the substrate the change-trigger class (0029 §4) consumes.

**Non-goals.** (N1) Not engine telemetry: logs, metrics, and traces of the Loom process itself are a
separate operational concern and out of scope here; this is data-change observability. (N2) Not a
message bus with cross-workspace fan-out or exactly-once network delivery; ordering is guaranteed
within a workspace, and idempotency is achieved by the commit digest (§5), not by a delivery protocol.
(N3) Not a query layer: a feed reports *that* and *what* changed, not arbitrary historical queries
(use `vcs_diff`/`log`, 0003 §5, for that). (N4) Not a lifecycle or control-plane event bus: daemon
shutdown, attached-session loss, request drain, progress, cancellation, and transport close are owned by
0043 and 0008 unless a future operations workspace intentionally records them as data.

## 2. The observation model

A workspace's history is an append-only DAG of commits (0002 §6). A change feed is a **forward walk of
that DAG from a cursor**: given the commit a subscriber last saw, the feed yields the commits that
followed and, for each, the set of changes it introduced (computed as the commit-to-parent diff, 0003
§5). Observation is therefore reading the existing history forward, not a new data structure.

Two consequences follow and are normative:

- **Per-workspace order.** Feed order is defined only within one workspace. A caller that watches
  several workspaces opens one cursor per workspace and merges client-side if needed.
- **Determinism.** Two subscribers starting from the same cursor over the same workspace MUST receive
  the same events in the same order, because the events are a function of the commit DAG, which is
  content-addressed.
- **Resumability.** A cursor is a versioned source cursor that carries its concrete ref, selector
  scope, last delivered commit digest or null, and intra-commit index for large diffs. A subscriber
  that disconnects resumes from its cursor without loss or duplication of committed events (S4-style
  resumability, 0006 §S4).

## 3. Event taxonomy

0030 owns the common data-change envelope, not every domain event vocabulary. A `DataChange` describes
one ordered workspace transition and carries zero or more domain-owned `DomainChange` records:

```idl
struct DataChange {
  workspace: Uuid,                 // the workspace id (0014)
  ref:       string,               // branch or ref that yielded this event
  commit:    Digest,               // the commit that introduced the change
  seq:       u64,                   // monotonic per-workspace sequence (the cursor advances over this)
  changes:   List<DomainChange>,    // domain-owned records introduced by this workspace transition
  unsupported_domains: List<UnsupportedDomainDetail>,
}

struct DomainChange {
  domain:         string,           // files | sql | kv | graph | vector | document | ...
  schema_version: u32,              // version of the domain-owned detail contract
  kind:           string,           // domain-owned verb or normalized create/update/delete class
  key:            bytes,            // path, key, table+rowid, node/edge id, stream offset, ...
  before:         optional Digest,  // prior value digest, if meaningful for this domain event
  after:          optional Digest,  // new value digest, if meaningful for this domain event
  detail:         optional bytes,   // canonical domain-owned detail payload
}

struct UnsupportedDomainDetail {
  domain:     string,               // domain whose source transition was observed
  capability: string,               // capability that would enable stable detail, e.g. watch.domain.kv
}
```

The `domain` and `detail` contracts are owned by the domain's spec. For example, 0003 owns file path
change detail, 0011 owns SQL table and row detail, 0021 owns queue stream and consumer-offset detail,
and 0017 owns vector upsert/delete/index detail. 0030 only defines how those records are ordered,
filtered, authorized, carried, and delivered.

A domain that has not promoted a detailed event contract reports an `UnsupportedDomainDetail` marker
instead of a `DomainChange`. That preserves the workspace revision feed without requiring 0030 to
invent domain-specific semantics. Lifecycle and control-plane notifications are not `DomainChange`
records unless an owning spec explicitly records them as data; if they share a delivery transport,
they use a separate discriminated payload such as `Lifecycle`.

## 4. Subscriptions and selectors

A subscription is defined by a selector that bounds what it observes:

```idl
struct WatchSelector {
  workspace: string,               // workspace UUID or name
  ref:       string,               // one exact branch/tag ref for this cursor
  scope:     WatchScope,           // all, or a domain-owned canonical key prefix
  domains:   List<string>,         // empty means all authorized domains; otherwise sorted and unique
  kinds:     List<ChangeKind>,     // empty means all normalized create/update/delete classes
}

enum WatchScopeKind { All, KeyPrefix }

struct WatchScope {
  kind:       WatchScopeKind,
  prefix:     bytes,               // empty for All; otherwise a domain-owned canonical key prefix
}
```

`WatchSelector` is the type the change-trigger binding references (0029 §3, `TriggerKind::Change`), so
a trigger and an external observer scope changes the same way. A subscription observes only what its
caller is authorized to read (§9). A pull cursor is bound to one concrete `ref`; a caller that wants to
observe several refs opens one cursor per ref and merges client-side. This keeps replay deterministic
and avoids a hidden multi-ref ordering rule.

Domain filters are strings from the domain registry. The list is canonicalized by sorting and rejecting
duplicates. Kind filters use the normalized create/update/delete class (`ADDED`, `MODIFIED`,
`DELETED` in the IDL) rather than every domain-owned `DomainChange.kind` string; callers that need
domain-specific verbs filter those after delivery.

## 5. Delivery: pull and push

Both delivery shapes ride the same cursor model:

- **Pull (cursor).** The caller polls `poll(cursor, max)` and receives the next events plus an advanced
  cursor. This is the simplest, most robust mode and the one a trigger keeper (0029 §9) uses
  internally. It needs no persistent connection and is trivially resumable.
- **Push (stream).** The caller holds a stream and receives events as they commit. Watch still owns the
  source cursor and `DataChange` payload. Source-backed hosted REST SSE streams emit the advanced watch
  cursor as the stream id and payload source cursor. Hosted transports that need reconnect-safe push use
  0035 durable delivery for outbox storage, acknowledgements, replay, backpressure, and transport
  projection.

The watch cursor is a source cursor, not a 0035 delivery cursor. It is an opaque string to callers but
has a stable versioned profile so every binding can store and resume it without server-side session
state. The current source-backed profile is:

```text
loom-watch-v1|<workspace-uuid>|<ref-name-utf8-hex>|<commit-digest-or-->|<intra-commit-index>
```

That profile is valid for the current exact-ref, all-domain, commit-level pull baseline. The target
narrowed-selector profile is:

```text
loom-watch-v2.<base64url-no-pad(loom-canonical-cbor WatchCursorV2)>
```

`WatchCursorV2` carries the workspace UUID, concrete ref, canonical selector (`scope`, `domains`,
`kinds`), last delivered commit or null, and intra-commit index. It MUST carry enough selector state for
`poll(cursor, max)` to enforce the same scope without a server-side subscription row. A malformed
cursor, wrong profile, invalid selector encoding, or no-longer-reachable commit fails with
`CURSOR_INVALID`.

Delivery is at-least-once; idempotency is the observer's, made trivial because every `DataChange`
carries the `commit` digest and a per-workspace `seq`. An observer that records the last `seq` it processed
discards anything at or below it. A 0035 delivery envelope may add a transport-level delivery id around
the same payload without changing watch idempotency. A persistent feed (for audit or replay) MAY be
materialized into an append-log or ledger workspace (0021/0018), which then is itself syncable and
addressable.

Coalescing and debounce (collapsing a burst of rapid changes into one delivered event) are a
subscription option owned here rather than in 0029. A subscription MAY request a debounce window, in
which case the delivered event spans the resulting commit range.

Derived indexes and rebuildable caches are not observable source events. A watch observes the
underlying stored data change that caused a derived rebuild, not the rebuild itself.

## 6. Relationship to triggers, sync, and the event spine

- **Triggers (0029).** A change trigger is a built-in subscriber whose handler runs a program. The
  keeper subscribes with the binding's `WatchSelector`, and on each delivered event invokes `exec`
  with the event digest as a seeded input (0029 §2, §4). 0030 owns observation; 0029 owns the program
  execution.
- **Sync (0006).** A sync receiver is an observer of the sender's refs; conversely, a feed can drive
  incremental sync. Whether an observed change propagates is governed by 0006; observation itself
  transfers nothing.
- **The event spine.** When a feed must be durable, replayable, or auditable, it is materialized into
  a ledger or append-log workspace (the same spine the trigger fire log uses, 0029 §6), so observation
  history is first-class versioned data.

## 7. Interface sketch (`watch` facade)

A new optional facade, present iff the `watch` capability is advertised. Illustrative IDL only
(non-normative):

```idl
interface Watch {
  // Open a cursor at the current head (or at a given commit, to replay history forward).
  subscribe(sel: WatchSelector, from?: Digest): Cursor
  // Pull the next batch and advance.
  poll(cursor: Cursor, max: u32): { events: List<DataChange>; next: Cursor }
  // Server-streamed push over the same cursor model; callers resume by passing the latest source cursor.
  stream(sel: WatchSelector, from?: Digest): Stream<DataChange>
}
```

The error for an invalid cursor is the already-registered stable code `CURSOR_INVALID` (a cursor whose
commit is no longer reachable, for example after history rewrite or GC, 0009 §7).

## 8. Interaction with existing specs

- **0002 / 0003** - a feed is a forward walk of the commit DAG; per-event changes are commit diffs;
  this widens the change-feed surface of 0003 §4.6.
- **0009** - promotes the `watch` menu item (0009 §8) to a capability; observation is gated by read
  authorization (§9).
- **0014** - subscriptions are workspace-scoped; ordering is per workspace (N2).
- **0029** - the change-trigger class subscribes here; coalescing and debounce are defined here.
- **0006** - sync and observation are duals; neither implies the other.
- **0018 / 0021** - a durable feed materializes into a ledger or append-log workspace.
- **0035** - hosted push transports use durable delivery for outbox storage, acknowledgements,
  reconnect replay, backpressure, and transport-specific envelopes.
- **0043 / 0008** - MCP and hosted protocol lifecycle notifications, such as daemon-loss drain,
  cancellation, progress, and transport close, are separate lifecycle payloads. They are not 0030
  `DomainChange` records unless an owning data domain explicitly records them as workspace data.

## 9. Security

Observation is a read. A subscriber MUST hold a read grant (0027) over what its `WatchSelector`
covers; a subscription that names a workspace, ref, path scope, or domain the caller may not read is
refused with `PERMISSION_DENIED`, and a feed never delivers an event for a resource the subscriber may
not read (fail-closed, narrowing the feed to the authorized subset rather than erroring per event).
Because `before`/`after` are content addresses, delivering them does not leak content the subscriber
could not already fetch under its grants. Observation does not widen the threat model beyond read
access; it makes existing read access continuous.

## Resolved Decisions

1. **Cross-workspace ordering.** Feeds are per-workspace only. A caller watching several workspaces
   opens one cursor per workspace and merges client-side if it needs a combined view. Loom does not
   invent a global ordering model across workspace histories.
2. **Push transport and backpressure.** Watch defines the source cursor and event model. Durable hosted
   push, client acknowledgements, reconnect replay, and backpressure are owned by 0035. Watch streams
   degrade to pull and resume from the watch cursor.
3. **Observing derived data.** Watch observes underlying stored data only. Derived indexes,
   accelerators, and rebuildable caches do not emit source events; observers react to the stored data
   change that caused the rebuild.
4. **Domain-owned change records.** 0030 owns the observation spine and `DataChange` envelope. Domain
   specs own `DomainChange` records, including key shape, event kinds, detail schema, canonical bytes,
   and conformance vectors. Lifecycle and control-plane notifications use separate lifecycle payloads
   unless explicitly recorded as data by an owning domain.
5. **Exact-ref stateless cursors.** A watch cursor observes one concrete ref. Multi-ref observation is a
   client or hosted-projection merge of several cursors. The cursor string is versioned and carries the
   selector state needed for stateless `poll`, while 0035 delivery cursors remain transport state.

## Unfinished Work

- No unfinished work remains in the 0030-owned scope.
- Durable reconnect and replay mechanics, transport ack contracts, and host-side delivery envelopes
  remain owned by `specs/0035-durable-delivery.md`.
- Non-file `DomainChange` grammars and field-level narrowing rules remain owned by their owning domain
  specs and are tracked there.

## Sources

- The `watch` capability and change feeds: `specs/0009-security-and-capabilities.md` §8;
  `specs/0003-core-interface.md` §4.6.
- The commit DAG and diffs: `specs/0002-data-model.md` §6; `specs/0003-core-interface.md` §5.
- The reactive act side that consumes this feed: `specs/0029-events-and-triggers.md` §2, §4.
- Durable feed materialization: `specs/0018-ledger-layer.md`, `specs/0021-append-log-layer.md`.
