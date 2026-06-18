# CP-0003 - `watch` Binding

**Series:** Control-plane bindings (normative-track sub-series; Draft)
**Version:** 0.1.0-draft. **Status:** Draft. **Last updated:** 2026-06-25
**Reads first:** [`CP-0000-index.md`](./CP-0000-index.md),
[`../facet-bindings/P9-0002-projection-conventions.md`](../facet-bindings/P9-0002-projection-conventions.md),
facade spec **0030** (`watch`), **0008** streaming transport, **0029** triggers, and **0035** durable
delivery.

`watch` is the target control-plane surface for observing state changes. It is read-class: a caller
subscribes to changes it is authorized to read and resumes from a cursor after interruption.

## 1. Current Source Boundary

Current source implements the portable pull baseline plus hosted watch stream and materialization:

- `loom_core::watch` exposes `WatchSelector`, `WatchCursor`, `ChangeEvent`, `DomainChange`,
  `UnsupportedDomainDetail`, and `WatchBatch`.
- `Loom::watch_subscribe` creates a stateless cursor for one workspace branch.
- `Loom::watch_poll` returns bounded first-parent commit events in forward order, advances the cursor,
  emits sorted file path changes, emits file-domain `DomainChange` records with path keys and
  before/after digests where derivable, preserves non-file-only workspace revisions with
  capability-labeled unsupported-domain detail markers, and maps unreachable cursors to
  `CURSOR_INVALID`.
- The portable pull baseline supports files-domain, path-prefix, and normalized change-kind selector
  narrowing over the source-backed file diff substrate. Non-file exact-domain selectors fail with
  `UNSUPPORTED` until that domain promotes a stable watch detail contract.
- The portable pull baseline requires VCS/ref read access before subscribe or poll. Explicit
  unauthorized file path-prefix selectors fail with `PERMISSION_DENIED`; broad file watches omit
  unauthorized path changes and omit unsupported-domain markers for facets the caller cannot read.
- `loom-mcp` exposes read-only `watch.subscribe` and `watch.poll` tools with workspace-bound schema
  elision, normalized change-kind selector narrowing, output schemas for `DataChange` payloads, and
  host capability overlay. MCP App resource wakeups advance source watch cursors for committed
  workspace changes; daemon/session lifecycle remains a separate MCP lifecycle payload.
- The C ABI exposes `loom_watch_subscribe`, `loom_watch_poll`, and `loom_watch_poll_async`.
  Subscribe returns an opaque UTF-8 cursor string. Poll returns canonical `loom.watch.batch.v1` CBOR.
- Local bindings expose the pull baseline in Node, Python, WASM, Swift, Kotlin, JVM, C++, and
  React Native.
- Hosted REST, JSON-RPC, and gRPC expose source-backed pull subscribe and poll operations over the
  same authorization gates, source cursor, selector narrowing, `DataChange` envelope, nested
  `DomainChange` records, unsupported-domain markers, and bounded poll limit. REST and JSON-RPC encode
  domain keys and detail bytes as lowercase hex strings; gRPC carries them as bytes.
- Hosted REST exposes source-backed SSE streaming with `GET /watch:stream` over an existing watch
  cursor. Each SSE frame carries the advanced source cursor as both the SSE `id` and payload
  `source_cursor`, supports bounded poll batches, and supports a bounded debounce window for
  deterministic coalescing.
- Hosted JSON-RPC `watch.stream` and gRPC server-streaming `Watch::Stream` are source-backed and share
  the same cursor validation, selector narrowing, batch limit, interval, and debounce semantics.
- Hosted REST and JSON-RPC expose `watch.materialize` as an optional append-log materialization hook.
  The hook stores canonical `loom.watch.batch.v1` CBOR and returns the append sequence and advanced
  source cursor.
- `crates/loom-conformance` runs an executable `watch` behavior suite.

Generic 0035 durable delivery integration and non-file domain event detail are still target work.
Table-history readers and row diffs are adjacent source-backed substrate for later domain-specific
event detail.

## 2. Target Facade Surface

Public shape:

```text
subscribe(sel: WatchSelector, from?: Digest) -> Cursor
poll(cursor: Cursor, max: u32) -> { events: List<DataChange>, next: Cursor }
stream(sel: WatchSelector, from?: Digest) -> Stream<DataChange>
```

`WatchSelector` scopes by workspace, exact ref, domain, path or key prefix, and normalized change kind.
One cursor observes one concrete ref. A caller that wants multiple refs opens multiple cursors and
merges client-side or through a hosted projection. The cursor is a stateless source position over
reachable history and includes enough canonical selector state for `poll(cursor, max)` to enforce the
same scope without a server-side subscription row. If that position is no longer reachable after GC or
history rewrite, the operation fails with `CURSOR_INVALID`.

The source-backed cursor profile is
`loom-watch-v1|<workspace-uuid>|<ref-name-utf8-hex>|<commit-digest-or-->|<intra-commit-index>` and is
valid for the current exact-ref, all-domain, commit-level pull baseline. The target narrowed-selector
profile is `loom-watch-v2.<base64url-no-pad(loom-canonical-cbor WatchCursorV2)>`, where
`WatchCursorV2` carries workspace UUID, concrete ref, canonical selector, last delivered commit or
null, and intra-commit index.

The public payload is a 0030 `DataChange`: a workspace revision envelope with ordered, domain-owned
`DomainChange` records. Commit-level `DataChange` events are the baseline because they are always
derivable from history. File-domain `DomainChange` records are source-backed for path keys and
before/after digests. Other detailed `DomainChange` records are opt-in per domain after the owning
spec provides a stable diff shape, key grammar, detail schema, and conformance vectors. Until then, a
source can report capability-labeled unsupported-domain detail markers without inventing a
domain-specific `DomainChange` shape. Lifecycle and control-plane notifications, such as daemon-loss
drain or transport cancellation, are not `DomainChange` records unless an owning domain explicitly
records them as workspace data.

## 3. Tier-1 REST

Source-backed workspace-bound REST root:

| Method | HTTP |
| --- | --- |
| `subscribe` | `POST /watch:subscribe` with `{ branch?, from?, facet?, path_prefix?, change_kinds? }` returning `{ cursor }` |
| `poll` | `GET /watch?cursor=...&max=...` returning `{ events, next }` |
| `stream` | `GET /watch:stream?cursor=...&max=...&interval_ms=...&debounce_ms=...` as SSE, with the advanced source cursor in the SSE `id` and payload |
| `materialize` | `POST /watch:materialize` with `{ cursor, max, stream }` returning `{ stream, seq, source_cursor, events, payload_schema }` |

`CURSOR_INVALID` maps through 0008's stable error table.

## 4. Tier-1 JSON-RPC and gRPC

Source-backed JSON-RPC methods: `watch.subscribe`, `watch.poll`, `watch.stream`, and
`watch.materialize`.

Source-backed gRPC methods: `Subscribe`, `Poll`, and streaming `Stream`.

## 5. Tier-1 MCP

- **Read tools:** `watch.subscribe` and `watch.poll` are source-backed with `DataChange`,
  `DomainChange`, and unsupported-domain output schemas.
- **Authorization:** observing is not write-class, but selectors must be narrowed to the caller's
  readable workspace, ref, domain, and path/key scopes. A watch stream must never emit an event outside
  the caller's grants.

## 6. Tier-2 Foreign Adapter

Change-data-capture and webhook adapters are target work. A CDC-style adapter can map `DataChange` and
domain-owned `DomainChange` records to CDC records and the cursor to an offset. Webhooks can wrap
`stream` delivery. Both depend on the
source-backed watch facade, 0008 transport rules, 0026-0028 authorization, and 0035 durable delivery
for reconnect-safe push.

## 7. Errors, Parity, and Concurrency

- **Errors:** `CURSOR_INVALID` is source-backed in the stable `Code` enum and used by pull polling.
  Other watch-specific errors are target work.
- **Parity:** pull polling is source-backed as the portable baseline across core, C ABI, local
  bindings, MCP, REST, JSON-RPC, and gRPC. REST SSE streaming, JSON-RPC `watch.stream`, gRPC
  `Stream`, and append-log materialization are source-backed hosted profiles.
- **Concurrency:** watch is read-class. Delivery is at-least-once; consumers deduplicate using cursor,
  commit, sequence, or delivery envelope identity.

## 8. Resolved Decisions

### CP-RD-W1 - Cursor model

- **Decision.** Use stateless source cursors as the default cursor model. A cursor observes one concrete
  workspace ref, carries the canonical selector state needed for poll, and advances over commit plus
  intra-commit index. Durable named delivery cursors belong to 0035 or an explicit consumer feature.

### CP-RD-W2 - Event granularity

- **Decision.** `DataChange` workspace revision envelopes are the baseline. Domain-owned
  `DomainChange` records are capability-reported opt-ins after each owning domain defines a stable diff
  shape. 0030 owns the watch spine and envelope, not every domain event vocabulary.
