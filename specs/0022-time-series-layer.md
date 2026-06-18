# 0022 - Time-Series Layer

**Status:** Partial, current time-series substrate and public facade source-backed. **Version:** 0.1.0.
**Capability:** `time-series`.

This spec defines the time-series facet: versioned ordered points keyed by timestamp. Current source
implements the Rust substrate in `loom-core::timeseries`, including a reachable structured point tree
with canonical measurement, tags, typed fields, and nanosecond timestamps, plus the workspace-scoped public facade
(`ts_put`/`ts_get`/`ts_range`/`ts_latest`), the language-neutral IDL shape, the C ABI and C header
projection, all eight language bindings, C ABI tests, and an executable facade behavior runner in
`loom-conformance`. Hosted REST/JSON-RPC exposes the native time-series surface, and first-class
Influx, Prometheus, Grafana, and OTLP metrics HTTP surfaces now map into the structured point model.
Native hosted gRPC now exposes the same core time-series operations. Point-level merge tooling, full PromQL/remote-read compatibility, full Grafana plugin
coverage, OTLP protobuf/gRPC ingestion, and automatic retention compaction remain target work. Points
are keyed by an `i64` timestamp; `ts_range` is half-open `[from, to)` and returns the canonical CBOR
array of `[ts, value]` pairs; an absent timestamp or series reads as absent.

Every operation is scoped to one workspace's time-series facet. Cross-workspace time-series writes
are out of contract and must fail with `CROSS_WORKSPACE` once a public facade exposes them.

## 1. Current Implementation

`loom-core::timeseries` implements:

- `Series::new`;
- `len` and `is_empty`;
- `put(ts, value)`;
- `get(ts)`;
- `range(from, to)`;
- `latest()`;
- `iter()`;
- canonical `encode` and `decode`;
- `put_series(loom, ns, name, series)`;
- `get_series(loom, ns, name)`;
- structured points keyed by measurement, sorted tags, nanosecond timestamp, and field;
- collection policy with query visibility horizon and declared rollups;
- rollup materialization into reachable derived point trees;
- explicit raw point pruning before a cutoff.

Timestamps are signed 64-bit integers. Values are opaque byte strings. A repeated timestamp replaces
the existing value at that timestamp inside one in-memory series. `range(from, to)` is half-open:
`from <= ts < to`. `latest` returns the highest timestamp.

The public `TimeSeries` facade is source-backed through the IDL, C ABI, C header, CLI, Node, Python,
C++, Swift, JVM, Android, React Native, WASM, and MCP data tools. Hosted native REST, JSON-RPC, and
gRPC are source-backed for byte-facade points, structured points, policy, rollup materialization,
rollup range, and explicit prune. There is no source-backed point-level merge helper today.

## 2. Current Storage Shape

The time-series facet path is:

```text
/.loom/facets/time-series/<name>
```

`put_series` creates the time-series facet directory and stages the collection as a structured
time-series root (a Tree of metadata, a reachable point-field prolly root, and rollup roots) through
the workspace working tree; byte-facade points are stored as points inside that structured root, not
as a single series blob. `get_series` reads the points back from the structured point tree and
decodes them. Workspace commit, branch, checkout, bundle sync, and clone see the series as ordinary
committed content under the time-series facet.

Structured collections use a dedicated time-series Tree entry with metadata, a reachable raw
point-field prolly root, and a nested rollups Tree of reachable derived prolly roots. The current
source persists query visibility and rollup declarations in metadata. Tombstone and point-level merge
metadata are not implemented.

## 3. Current Encoding

`Series::encode` writes a Loom Canonical CBOR array of points in timestamp order. Each entry is:

```text
[timestamp, value]
```

The byte-facade API encodes and decodes `Series` values for callers, but current storage does not
persist that value as a series blob. Structured collection metadata is version 2 and encodes query
visibility plus rollup declarations. Version 1 metadata is not a supported current storage schema.

## 4. Current Versioning and Merge Behavior

Current series values version with the workspace because they are written into the workspace working
tree. A commit snapshots the structured time-series root with every other staged workspace path.
`checkout_commit` and `checkout_branch` restore the time-series root with the rest of the workspace
tree.

Current source does not implement point-level merge. If two branches edit the same time-series
collection differently, the current merge machinery treats it as a normal same-path conflict unless a promoted
time-series-specific merge helper exists. Sync follows `CONFLICT-RESOLUTION-MATRIX.md`: branch/ref
divergence uses the S1 fast-forward boundary, and point-level S2 merge is explicit target work.

Rollup declarations, materialized derived point trees, query visibility, and explicit raw pruning are
source-backed. Automatic retention compaction, daemon-scheduled rollup rebuild, and point-level merge
remain target work.

## 5. Current Conformance

`loom-core::timeseries` has unit tests for:

- timestamp ordering;
- half-open range queries;
- latest point lookup;
- repeated timestamp replacement inside one writer;
- canonical encode/decode;
- commit and checkout versioning.

`loom-conformance` contains time-series behavior scenarios and an executable public time-series facade
runner. The runner exercises put, get, half-open range, latest, structured points, query visibility,
rollup materialization, explicit raw pruning, commit and checkout versioning, and clone reachability
against the in-memory store.

## 6. Target Contract

The target public time-series facade should provide:

- append or put point;
- get by timestamp;
- range query;
- latest;
- explicit timestamp unit;
- optional multi-series keying;
- rollup declaration and rebuild;
- retention policy;
- point-level diff;
- explicit point-level merge tooling.

Before hosted and full enterprise promotion, the remaining facade needs:

- hosted protocol methods in 0008;
- stable error mapping through `loom_core::error::Code`;
- access-control review for served time-series writes;
- clear file-projection behavior for `/.loom/facets/time-series/...`;
- same-point collision behavior aligned with `CONFLICT-RESOLUTION-MATRIX.md`.

## 7. Target Storage Contract

Time-series storage is a structured time-series value, not one whole series blob. This structured root
is source-backed today and uses existing object types and deterministic encodings:

| Role | Target encoding | Status |
| --- | --- | --- |
| Point tree | Prolly tree keyed by measurement, sorted tags, timestamp, and field | Source-backed |
| Rollup views | Derived point trees under the collection root | Source-backed |
| Retention metadata | Query visibility horizon and rollup declarations | Source-backed partial |
| Series root | Tree referencing point roots, rollup roots, and metadata | Source-backed |

The timestamp unit (signed Unix nanoseconds), series-key encoding (`[measurement, sorted_tags,
timestamp_ns, field]`), duplicate-point policy (last-write-wins per that identity), metadata schema
(`[version=2, query_start_ns|null, [[name, resolution_ns, aggregation]...]]`), and rollup naming are
pinned by canonical and negative conformance vectors in `loom-core::timeseries` tests
(`time_series_metadata_canonical_vectors`, `time_series_point_key_canonical_vector`,
`time_series_negative_decode_vectors`; MX-280). Retention-metadata completeness and a retained-point
tombstone policy remain partial/target and are tracked separately. If any pinned choice changes
canonical bytes, update these vectors with the implementation.

### Resolved structured-point contract

The promoted collection root is a typed time-series Tree entry containing metadata and a reachable
`ProllyMap` root. The committed-tree reachability walk must descend through it, so point-tree nodes
participate in commit, clone, bundle, sync, and garbage collection.

Point-field identity is canonical CBOR `[measurement, sorted_tags, timestamp_ns, field]`. Tags are
ordered `[name, value]` text pairs, timestamps are signed Unix nanoseconds, and fields are typed
canonical values. A duplicate identity replaces only that field. The byte-series facade maps to a
collection-local default measurement, no tags, and a `value` field.

Current-format posture: there is one storage story. All time-series source data lives in the single
structured root's point tree. The byte API (`put_series`/`get_series`/`ts_put`/`ts_get`) is a
compatibility facade that writes and reads points under the reserved `_loom_legacy` measurement (no
tags, `value` field) via the same `ts_put_point`/`ts_range_points` path; current source never writes
or reads a separate series blob, and there is no blob read fallback. The structured root is the only
persisted time-series format.

Query visibility, derived rollups, and destructive history pruning are separate controls. A default
live-query horizon does not alter committed raw points; rollups are rebuildable derived views; physical
history pruning requires an explicit separately-audited operation.

## 8. Relationship to Other Facets

- **Queue:** time-series is append-oriented, but point lookup is ordered by timestamp rather than
  append sequence.
- **Columnar:** rollups may be represented by columnar datasets after 0023 promotes a stable storage
  contract.
- **Compute:** rollup and retention policy should be compute-triggered target work, not hidden
  behavior in the raw series substrate.

## 9. Non-Goals and Limits

- Current source is not a high-ingest TSDB.
- Current source does not provide automatic retention compaction.
- Current source does not provide daemon-scheduled rollup rebuild.
- Current source does not provide point-level merge.
- Current source does not provide timestamp-unit metadata.

## 10. Unfinished Work

| Order | Parent | Work item | Status | Exit criteria |
| --- | --- | --- | --- | --- |
| T1 | RD6 | Spec/source reconciliation | Complete local | Current implementation, conformance, and decision text describe the implemented local time-series facade, CLI, and MCP data projection instead of stale target-only language. |
| T2 | RD6 | Time-series data CLI projection | Complete local | `loom time-series ...` and the `timeseries` alias expose put, get, range, and latest with byte-stable output forms. |
| T3 | RD6 | Hosted time-series wire projection | Source-backed partial | REST and JSON-RPC expose byte-facade points, structured points, policy, rollup materialization, rollup range, and explicit prune. Native gRPC protocol conformance proves put, get, latest, and server-streaming range through `hosted-timeseries-grpc`. Served REST and JSON-RPC protocol conformance prove put, get, latest, and range through `hosted-timeseries-rest` and `hosted-timeseries-jsonrpc`. Generated-client, policy, rollup, retention, and broader compatibility protocol conformance remain target. |
| T4 | RD5 | Rollup and retention public surface | Source-backed partial | Rollup declarations, materialization, retained aggregate history, query visibility, and explicit raw prune are source-backed. Automatic compaction and scheduled rebuild remain target. |
| T5 | RD7 | Structured point storage and point-level merge | Source-backed partial | Point-tree storage is source-backed; timestamp-unit metadata, point-level diff, and point-level merge remain target. |
| T6 | RD12 | Time-series compatibility surfaces | Source-backed partial | Influx line protocol write, Prometheus remote write plus simple selector query/query_range, Grafana datasource health/search/query, and OTLP HTTP JSON metrics ingestion are source-backed. Full PromQL, Prometheus remote read, full Grafana plugin behavior, and OTLP protobuf/gRPC remain target. |

## 11. Resolved Decisions

- **RD1 - Current storage.** Current source stores each named series as a structured time-series root
  (metadata, a reachable point-field prolly root, and rollup roots) under the workspace time-series
  facet, not as one canonical series blob.
- **RD2 - Current key.** Points are keyed by signed 64-bit timestamp.
- **RD3 - Current duplicate policy.** Repeated timestamp writes replace the existing point inside one
  writer.
- **RD4 - Range semantics.** Current range queries are half-open: `from <= ts < to`.
- **RD5 - Rollup boundary.** Rollups are target derived views, not current committed source of truth.
- **RD6 - Public facade status.** The workspace-scoped public `time-series` facade
  (put/get/range/latest, `i64` timestamps, half-open range) is source-backed across IDL, C ABI, C
  header, CLI, all eight bindings, MCP data tools, hosted REST/JSON-RPC/gRPC operation surfaces, and
  native gRPC protocol conformance, with an executable facade conformance runner.
- **RD7 - Merge boundary.** Point-level merge is explicit target work. Sync does not silently merge
  divergent time-series histories.
- **RD8 - Target boundary.** Automatic rollup rebuilding, automatic retention compaction,
  generated-client protocol conformance, broader compatibility protocol conformance, and
  point-level merge remain target work.
- **RD9 - Structured root.** Time-series uses a typed Tree entry with a reachable `ProllyMap` root.
- **RD10 - Point identity.** Measurement, sorted tags, nanosecond timestamp, and field identify one
  field value; duplicate writes replace that value.
- **RD11 - Retention boundary.** Query visibility, derived rollups, and destructive history pruning
  are separate controls.
- **RD12 - Compatibility surfaces.** Influx, Prometheus, Grafana, and OTLP are first-class served
  surfaces over canonical structured time-series points, not `time-series` transports.

## Change log

### Collection parameter rename (0042 section 5.1)

The series-set collection segment's canonical parameter name is `collection`, replacing the legacy
`name`. Concept and address position are unchanged (0042); this aligns the time-series facade with the
cross-facet collection term. Implementation is the follow-on full-stack rename pass.

### Time-series public facade (engine slice; 0007, 0010)

The time-series facade is source-backed end to end (engine portion of the Document + Time-series +
Ledger batch): `loom-core::timeseries` adds `ts_put`/`ts_get`/`ts_range`/`ts_latest` (`i64` timestamp,
half-open `[from, to)`, absent reads as empty/None); projected to IDL `interface TimeSeries`, C ABI
`loom_ts_*` (with a round-trip test; `loom_ts_latest` returns the newest timestamp and bytes), the C
header, and all eight bindings; covered by the executable `run_timeseries_facade_behavior` runner
(put/get, half-open range, latest, commit/checkout versioning, clone reachability) wired into
`certify_memory_store`, with the `time-series` capability flipped from `scenario` to `executable`
(registry + 0010 section 5). Rollups and retention remain target work.

- 2026-06-27 (P-bindings): Time-series facade (`ts_put`/`ts_get`/`ts_range`/`ts_latest`) now has
  full language-binding parity across all eight families (Node, Python, WASM, C++, iOS/Swift, JVM,
  Android JNI+Kotlin, React Native). The i64 timestamp crosses the React Native bridge as a decimal
  string for 64-bit safety; `ts_latest` resolves `{ ts, value }`. Verified via `just test-bindings`.
