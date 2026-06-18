# 0023 - Columnar Layer

**Status:** Partial, current columnar substrate and public facade source-backed. **Version:** 0.1.0.
**Capability:** `columnar`.

This spec defines the columnar facet: versioned append-oriented typed datasets for scan-heavy use.
Current source implements the reusable Rust columnar model in `loom-columnar`. That crate owns the
structured canonical segment manifest, profile-aware segment digests, deterministic committed segment
statistics, portable select and aggregate semantics, the columnar executor seam, and Arrow IPC plus
Parquet import/export behind its `arrow` feature. `loom-core::columnar` consumes and re-exports those
contracts while retaining workspace, ACL, store, `FacetKind::Columnar`, and public facade integration.
The language-neutral public facade, C ABI and C header projection, all eight local binding projections,
StateAccess select support, local MCP data-tool projection, and executable facade conformance remain
source-backed through core integration. The source-backed public surface also includes aggregate,
compact, inspect, and source-digest projections through Rust, CLI, C ABI/header, IDL, and MCP. The
compatibility `columnar-arrow` core feature enables `loom-columnar/arrow`; native CLI import/export
continues to use the core re-export. Direct scalar types use native Arrow arrays. Extended Loom values
use exact canonical-cell binary carriers plus Loom field metadata so they round trip without changing
committed storage identity. Local wrapper parity for aggregate, compact, inspect, and source-digest
operations is source-backed. Hosted REST, JSON-RPC, and native gRPC are source-backed for the current
facade. Hosted REST and JSON-RPC protocol conformance covers create, append, scan, columns, rows,
compact, inspect, source-digest, select, and aggregate. Hosted REST and native gRPC binary Arrow IPC
and Parquet import/export are source-backed behind `loom-hosted/columnar-arrow`. Prepared REST Arrow
IPC result handles are source-backed as principal-bound, session-bound, one-shot
`/_loom/results/{handle}` routes that reauthenticate and rerun PEP before returning the binary body.
Arrow Flight or Flight SQL data-plane transfer, durable
Parquet segment storage, and segment-level merge tooling remain target work. Dataframe transformations
and Polars-backed dataframe execution are owned by 0045.

Every operation is scoped to one workspace's columnar facet. Cross-workspace columnar writes are out
of contract and must fail with `CROSS_WORKSPACE` once a public facade exposes them.

## 1. Current Implementation

`loom-columnar` implements:

- `ColumnarSet::new(columns, target_segment_rows)`;
- `columns`;
- `rows`;
- `is_empty`;
- `segment_count`;
- `target_segment_rows`;
- `manifest()`;
- `manifest_with_algo(algo)`;
- `append_row(row)`;
- `scan()`;
- `compact()`;
- canonical `encode`, `encode_with_algo`, `decode`, and `decode_with_algo`;
- `ColumnarExecutor`.

`loom-core::columnar` integrates that reusable model with the Loom kernel and implements:

- `put_columnar(loom, ns, name, dataset)`;
- `get_columnar(loom, ns, name)`;
- `columnar_create(loom, ns, name, columns, target_segment_rows)`;
- `columnar_append(loom, ns, name, row)`;
- `columnar_scan(loom, ns, name)`;
- `columnar_columns(loom, ns, name)`;
- `columnar_rows(loom, ns, name)`;
- `columnar_compact(loom, ns, name)`;
- `columnar_inspect(loom, ns, name)`;
- `columnar_source_digest(loom, ns, name)`;
- `columnar_select(loom, ns, name, columns, filter)`;
- `columnar_select_auto(loom, ns, name, columns, filter, exec)`.
- `columnar_aggregate(loom, ns, name, aggregates, filter)`;
- `columnar_aggregate_auto(loom, ns, name, aggregates, filter, exec)`.

When built with the `arrow` feature, `loom-columnar` implements:

- `columnar_to_arrow_ipc(dataset)`;
- `columnar_from_arrow_ipc(bytes, target_segment_rows)`;
- `columnar_to_parquet(dataset)`;
- `columnar_from_parquet(bytes, target_segment_rows)`.

When built with the compatibility `columnar-arrow` feature, `loom-core::columnar_arrow` re-exports the
same functions from `loom-columnar`.

Columns are named and typed with `tabular::ColumnType`. Rows are `tabular::Value` arrays. `append_row`
validates arity and type, returning `INVALID_ARGUMENT` for invalid rows. Empty column lists are
rejected. `Null` is accepted by the tabular value/type rules.

The current Rust value stores rows in ordered segments. Committed canonical identity is a structured
columnar root containing manifest metadata plus durable segment payload references. The manifest
contains the schema, target segment size, statistics policy, compression policy, ordered segment
records, profile-aware segment digests, and deterministic segment statistics. Segment payloads are
native Loom Canonical CBOR row batches addressed from the structured root. Segment boundaries are
identity-affecting. `compact` preserves logical row order but may change canonical bytes when it
changes segment layout.

The public `Columnar` facade is source-backed through the IDL, C ABI, C header, CLI, Node, Python,
C++, Swift, JVM, Android, React Native, and WASM for the original create/append/scan/columns/rows/select
subset. Aggregate, compact, inspect, and source-digest are source-backed through Rust, CLI, C ABI/header,
IDL, MCP, and all local language wrappers. Arrow IPC and Parquet import/export are source-backed in the
opt-in native Rust `loom-columnar` feature, core compatibility feature, and native CLI for the promoted
type profile.
Bool, signed and unsigned integer widths through 64-bit, f32/f64, UTF-8 text, bytes, date, time, and
timestamp use native Arrow arrays. Decimal, 128-bit integers, UUID, inet, interval, point, list, and map
use exact canonical-cell binary carriers with `loom.column_type` field metadata because their Loom
semantics are per-value or heterogeneous in ways Arrow native logical arrays do not represent directly.
MCP remains a canonical CBOR control/data-tool surface and does not expose large binary Arrow or Parquet
transfer in the current contract. The capability registry reports `columnar-arrow-ipc` and
`columnar-parquet` as source-backed, with support set by the build feature. Hosted native REST,
JSON-RPC, and gRPC are source-backed for the current columnar facade, and hosted REST plus native gRPC
expose binary Arrow IPC and Parquet import/export when built with `loom-hosted/columnar-arrow`.
Prepared REST Arrow IPC result handles are source-backed for route-level analytical transfer. There is
no source-backed language-wrapper Arrow/Parquet binary transfer, Arrow Flight or Flight SQL data-plane
transfer, Polars integration, durable Parquet segment storage, or segment-level merge helper today.

## 2. Current Storage Shape

The columnar facet path is:

```text
/.loom/facets/columnar/<name>
```

`put_columnar` creates the columnar facet directory and stages a dedicated structured columnar root at
that path through the workspace working tree. `get_columnar` reads that root, loads its `manifest`
blob, loads each durable segment payload from the `segments` tree by digest, and validates it using the
store's identity-profile digest algorithm. Workspace commit, branch, checkout, bundle sync, and clone
see the dataset as a structured committed root under the columnar facet.

The current committed dataset is a structured manifest plus durable native-CBOR segment payloads. It
does not commit Arrow metadata, Parquet metadata, projection indexes, or merge-metadata roots. Arrow
IPC and Parquet bytes are derived interchange projections, not committed storage identity.

## 3. Current Encoding

`ColumnarSet::encode` writes a Loom Canonical CBOR array:

1. format version `2`;
2. columns as `[name, type_tag]` pairs;
3. target rows per segment;
4. statistics policy;
5. compression policy;
6. ordered segment records.

Each structured-root manifest segment record stores ordinal, row start, row count, encoding tag,
segment digest, and deterministic statistics. Native segment rows use the tabular cell-value codec and
are stored as durable segment payloads under the root's `segments` tree. Segment digests use the store
identity profile for committed datasets and BLAKE3 for default in-memory encoding. Current committed
source does not encode Arrow schema metadata, Parquet page metadata, projection indexes, or conflict
metadata. The native Arrow and Parquet projections attach Loom column type tags as Arrow field
metadata. Direct scalar values use native Arrow arrays; extended Loom values use canonical-cell binary
carriers, so all promoted column types round trip without changing committed Loom bytes.

## 4. Current Versioning and Merge Behavior

Current columnar datasets version with the workspace because they are written into the workspace
working tree. A commit snapshots the structured columnar root with every other staged workspace path.
`checkout_commit` and `checkout_branch` restore the `StagedEntry::Columnar` root with the rest of the
workspace tree.

Current source does not implement segment-level merge. If two branches edit the same dataset root
differently, the current merge machinery treats it as a normal same-path conflict unless a promoted
columnar-specific merge helper exists. Sync follows `CONFLICT-RESOLUTION-MATRIX.md`: branch/ref
divergence uses the S1 fast-forward boundary, and segment-level S2 merge is explicit target work.

Current columnar diff is object/path-level through the workspace tree. Whole-segment diff over the
structured segment manifest is target work.

## 5. Current Conformance

`loom-columnar` has unit tests for:

- segment rolling by target row count;
- append-order scans;
- explicit compaction preserving logical row order;
- segment-layout identity;
- corrupt segment digest and segment ordering rejection;
- row arity and type validation;
- empty schema rejection;
- canonical encode/decode;
- portable select and aggregate behavior;
- Arrow IPC and Parquet import/export for the promoted type profile;
- exact canonical-cell binary carriers for extended Loom logical values;
- deterministic Parquet bytes for that profile.

`loom-core::columnar` has unit tests for:

- profile-aware committed segment digests through the store integration;
- aggregate, inspect, source-digest, and compact facade behavior;
- storage facade create, append, scan, validation, and conflict behavior;
- ACL collection scope enforcement;
- injected executor reconciliation;
- commit and checkout versioning.

`loom-conformance` contains columnar behavior scenarios and an executable public columnar facade runner.
The runner exercises create, append, scan, select, validation, versioning, and clone behavior against
the in-memory store. `run_columnar_manifest_vectors` pins default-profile and SHA-256-profile columnar
manifest canonical bytes and includes a negative tamper vector.

## 6. Target Contract

The target public columnar facade should provide:

- create or open dataset;
- append row batch or segment;
- scan with projection;
- optional predicate pushdown;
- aggregation;
- explicit compaction;
- inspect and source digest;
- whole-segment diff;
- explicit segment-level merge tooling;
- Arrow IPC and Parquet import/export for supported type profiles;
- Parquet-backed durable segments if promoted;
- Arrow runtime, Flight transfer, or Flight SQL if promoted.

Before hosted and full enterprise promotion, the remaining facade needs:

- hosted protocol methods in 0008;
- stable error mapping through `loom_core::error::Code`;
- access-control review for served columnar writes;
- clear file-projection behavior for `/.loom/facets/columnar/...`;
- segment collision behavior aligned with `CONFLICT-RESOLUTION-MATRIX.md`.

## 7. Target Storage Contract

The enterprise storage target is a structured columnar value, not one serialized dataset value. The base
contract is a Loom canonical manifest, not raw Parquet or Arrow alone. A promoted storage format
should use existing object types and deterministic encodings:

| Role | Target encoding | Status |
| --- | --- | --- |
| Segment blobs | Durable native-CBOR row payloads referenced by the manifest; deterministic Parquet remains interchange | Source-backed |
| Runtime batches | Arrow RecordBatch and IPC-compatible values for promoted direct and extended type profiles | Partial source-backed |
| Manifest | Canonical record listing schema, codec, ordered segment digests, row ranges, and metadata | Source-backed |
| Statistics | Small deterministic committed profile plus richer derived metadata | Partial |
| Analytical view | Derived engine state | Target, not identity |
| Dataset root | Dedicated structured committed root containing `manifest` and `segments` entries | Source-backed |

The canonical manifest layout, native segment row codec, durable segment reference layout, statistics
identity policy, compression policy, compaction behavior, and default plus SHA-256 profile bytes are
pinned by implementation and conformance. The Arrow/Parquet promoted type profile and uncompressed
deterministic Parquet writer profile are source-backed in the opt-in native core feature and native
CLI. Parquet-backed durable segment storage remains a separate storage decision. If those choices
change canonical bytes, update conformance vectors with the implementation.

## 8. Engine Guidance

- The default source remains pure Rust and `wasm32`-clean.
- Arrow and Parquet are source-backed only behind the native `columnar-arrow` feature for the promoted
  interchange profile.
- Polars is not a current source-backed columnar contract.
- If a native OLAP engine is promoted, the storage contract must remain readable without that engine.
- In-memory analytical state is derived and rebuilt from committed columnar source data.
- DataFusion is eliminated from the v1 columnar and dataframe plan unless a later spec reintroduces it
  with a concrete role, platform profile, and conformance strategy.

## 9. Relationship to Other Facets

- **Tabular substrate / structured-table readers:** columnar reuses the generic structured-table
  readers in `loom-core::tabular` (read-table, index-scan, blame, diff), which address a table by its
  reserved path built from collection + table (0011 section 2.1). They were generalized off SQL's
  hard-coded database path for exactly this reuse; columnar supplies its own reserved table paths.
- **SQL:** columnar complements the row store. It should not replace row-level OLTP behavior from 0011.
- **Time-series:** time-series rollups may use columnar datasets after both specs pin stable public
  contracts.
- **Interchange:** Arrow IPC and Parquet import/export are implemented in 0023 for the native promoted
  type profile. Broader file interchange and dataframe ingestion semantics are split with 0012 and
  0045.
- **DataFrame:** dataframe transformations over CSV, JSON/NDJSON, Arrow, Parquet, SQL, files/CAS, and
  columnar inputs are owned by 0045. Materialized durable analytical outputs may be committed as
  columnar datasets.

## 10. Non-Goals and Limits

- Default builds are not Arrow or Parquet implementations.
- The native `columnar-arrow` feature is an interchange projection, not a replacement for Loom's
  canonical storage identity.
- Current source is not a distributed analytical query engine.
- Current source provides durable native-CBOR segment payloads, not Parquet segment files.
- Current source does not provide segment-level merge.
- Current source does not provide predicate pushdown or native acceleration.
- Current source does not provide dataframe transformation workflows.

## 11. Resolved Decisions

- **RD1 - Current storage.** Current source stores each named columnar dataset as a dedicated
  structured committed root under the workspace columnar facet.
- **RD2 - Current schema.** Current columns are named `tabular::ColumnType` values.
- **RD3 - Current rows.** Rows are appended and scanned in append order.
- **RD4 - Segment boundary.** Current segment boundaries are identity-affecting. Compaction preserves
  logical row order but changes canonical bytes when it changes the segment layout.
- **RD5 - Public facade status.** The workspace-scoped public `columnar` facade (create, append, scan,
  columns, row count, and the StateAccess predicate select) is source-backed across the engine, the
  C ABI, the IDL, the C header, and all eight bindings, with an executable facade conformance suite.
- **RD6 - Merge boundary.** Segment-level merge is explicit target work. Sync does not silently merge
  divergent columnar histories.
- **RD7 - Target base model.** The current columnar base is a Loom canonical manifest over
  deterministic native segment bytes. Arrow IPC and Parquet are native interchange projections for the
  promoted type profile. Parquet durable segment storage remains a separate target storage decision.
  Arrow remains the runtime, IPC, and Flight batch model.
- **RD8 - Type profile.** The source-backed interchange profile covers direct scalar native Arrow
  mappings plus exact canonical-cell binary carriers for extended Loom logical values. The extended
  carriers preserve Loom semantics first; later optimized Arrow-native representations may be promoted
  as additional profile rows only when they preserve the same round-trip contract.
- **RD9 - Unreleased format transition.** Current row-encoded columnar blobs are not a legacy contract.
  The promoted Arrow/Parquet-capable implementation may break current development datasets and
  regenerate fixtures.
- **RD10 - Statistics identity.** The manifest commits a small deterministic statistics profile,
  including row count, null count, and min/max for supported comparable types, while keeping richer
  engine metadata derived.
- **RD11 - Client-first wire posture.** Parquet import/export, Arrow runtime batches, Arrow Flight, and
  Flight SQL or ADBC-adjacent analytical access are higher-priority than generic REST, JSON-RPC, or
  native gRPC data-plane symmetry. REST, JSON-RPC, and native gRPC remain useful management/native
  projections, but they do not replace analytical client data-plane compatibility.
- **RD12 - Presentation order.** DuckDB-like local analytical SQL is the P0 presentation after the base
  storage profile. Snowflake-like hosted warehouse, Spark-like batch/dataframe, BigQuery-like job/query,
  and direct DuckDB integration remain P1 or later targets.
- **RD13 - Dataframe boundary.** Polars is the default native execution layer for the `dataframe` facet
  described by 0045, not a columnar storage identity dependency.

## 11.1 Unfinished Work

| Order | Parent | Work item | Status | Exit criteria |
| --- | --- | --- | --- | --- |
| T1 | RD5 | Spec/source reconciliation | Complete local | Current implementation and conformance text describe the implemented public columnar facade instead of stale target-only language. |
| T2 | RD5 | CLI columnar projection | Complete local | `loom columnar ...` commands expose create, append, scan, columns, rows, and select with canonical CBOR forms. |
| T3 | RD5 | MCP columnar data projection | Complete local | MCP tools expose create, append, scan, columns, rows, and select with registered schemas and canonical CBOR payload tests. |
| T4 | RD5, RD11 | Non-MCP hosted columnar wire projection | Partial source-backed | Native hosted REST, JSON-RPC, and gRPC expose create, append, scan, columns, rows, compact, inspect, source-digest, select, and aggregate with hosted auth and route coverage. REST and JSON-RPC protocol conformance proves the current native route set. Hosted REST and native gRPC binary Arrow IPC and Parquet import/export are source-backed behind `columnar-arrow`, with default builds returning stable `UNSUPPORTED`. Prepared REST Arrow IPC result handles are source-backed as principal-bound, session-bound, one-shot reads with PEP rechecks. Arrow Flight and Flight SQL or ADBC-adjacent access are compatibility/profile work under 310/320, not closure-blocking storage promotion debt for the current native-CBOR root. |
| T5 | RD5 | Aggregation public surface | Complete local | Portable aggregate request/response, C ABI, IDL, CLI, MCP, local language wrappers, hosted REST/JSON-RPC, and tests are implemented. |
| T6 | RD13 | Dataframe-native Polars integration | Target | Polars-backed dataframe execution is designed and implemented under 0045 without adding Polars to `loom-core` or making Polars-native state canonical. |
| T7 | RD6, RD7, RD10 | Parquet and Arrow storage promotion | Partial source-backed | Arrow IPC and deterministic Parquet import/export are implemented for the promoted type profile behind the native `columnar-arrow` feature, native CLI, hosted REST, and native hosted gRPC binary transfer routes, with feature-aware capability rows. MCP binary transfer is not part of this contract. Durable native-CBOR segment payloads are source-backed. Durable Parquet segment storage, Arrow schema metadata in the canonical manifest, projection indexes, compaction policy, and migration and conformance vectors are closure-blocking promotion debt before promoting a Parquet-backed or Arrow-schema-backed storage profile. |
| T8 | RD6 | Segment-level merge | Target | Whole-segment diff and merge behavior are implemented under the conflict matrix. Segment-level merge is closure-blocking promotion debt for structured storage promotion, because divergent committed segment histories must not be treated as opaque whole-dataset conflicts after promotion. |

## 12. Native query executor and aggregation (target design, 243b / 0023-agg)

This section is the complete, self-contained design for the Polars-backed native executor and the
columnar aggregation surface. It is written so a separate, build-capable session can implement it
without further discussion. `polars` is a heavy native dependency that cannot be compiled or run in the
spec-authoring sandbox, so the portable pieces (AST, portable aggregator, conformance vectors) are
implementable and verifiable anywhere, while the `loom-polars` accelerator must be built and verified on
a native toolchain.

### 12.1 Decisions (resolved)

- **Crate placement (resolved):** the native executor lives in a new standalone crate `loom-polars`
  that depends on `loom-core` + `polars`, and plugs in through the existing `ColumnarExecutor` trait at
  the call site. `loom-core` stays `wasm32`-clean; the default workspace build does not link `polars`.
  This mirrors `loom-hnsw` / `VectorAccelerator`. Do **not** add `polars` as a feature on an existing
  crate.
- **Scope (resolved, Q5=B):** full enterprise scope - both select acceleration **and** group-by /
  aggregation. Aggregation is a new public surface (structured request + facade + C ABI + IDL + 8
  bindings), not just a faster `select`.
- **Reconciliation contract (resolved, Q6=A):** a **portable Rust aggregator in `loom-core` is the
  normative source of truth**; `loom-polars` is a reconciled accelerator that MUST produce byte-identical
  results in identical order. This preserves wasm parity (aggregation works with no native engine) and
  keeps the "switch engines, identical result" guarantee of the seam. The predicate/filter is evaluated
  in our own Rust over the frame (never pushed into Polars), so ordering and the typed `Value` total
  order are guaranteed identical; Polars is used for vectorized scan/projection/aggregation kernels only,
  always re-reconciled to the portable contract.
- **Surface (resolved, Q7=A):** aggregation is expressed as a **structured CBOR aggregate request (AST)**
  on the columnar facet, distinct from the SQL facet. SQL (0011) stays the ad-hoc query surface;
  the columnar aggregate AST is the programmatic, ABI-stable analytical surface for scan-heavy datasets.
- **Function set (resolved, Q8=C):** the fullest set, landed in determinism-pinned tiers (see 12.4).
- **Determinism (resolved, Q10=A):** every aggregate function's algorithm is pinned in this spec and
  covered by a golden conformance suite; both the portable and Polars engines are verified against the
  same vectors. A function is not "done" until its determinism is pinned and vectored.
- **Auto-switch (resolved):** mirror `vindex::DEFAULT_EXACT_THRESHOLD` - run the portable path at or
  below a row threshold and the native executor above it; both return identical results, so the switch
  is invisible except in speed. Per-call DataFrame materialization is the initial mechanism (a
  `ColumnarSet` is `Vec<Vec<Value>>` with no persisted Arrow).
- **Cached derived artifact (resolved):** cached Arrow projections live in the shared embedded
  derived-artifact store and follow the canonical lifecycle contract in 0005 section 8.2 (implemented
  by `loom-store::derived`) - the same contract the vector ANN index (0017 section 5) and the search
  Tantivy index (0033 N.6) bind to. The registered columnar artifact is `arrow`, with format version
  `columnar-arrow-ipc-v1`. The projection's `source_digest` is taken over the committed structured
  columnar root, while engine version and format version stamp the local payload. The generic
  normative properties (identity-excluded, copy-carried/sync-rebuilt, stamp-based stale-before-trust,
  serve-read policy) are defined there and not restated here. `loom-store` source-backs the
  durable-local artifact record, stale detection, rebuild reporting, failure reporting, unsupported
  reporting, and compaction retention for columnar Arrow projections; actual Arrow projection writers
  and daemon rebuild orchestration remain target work.

### 12.2 The executor trait (current) and its extension

The source-backed seam today:

```rust
pub trait ColumnarExecutor {
    fn select(&self, set: &ColumnarSet, columns: &[&str],
              filter: Option<(&str, CmpOp, &Value)>) -> Result<Vec<Vec<Value>>>;
}
pub fn columnar_select_auto(loom, ns, name, columns, filter, exec: Option<&dyn ColumnarExecutor>)
    -> Result<Vec<Vec<Value>>>;
```

Extend the trait with an `aggregate` method and add a parallel `columnar_aggregate_auto` facade:

```rust
pub trait ColumnarExecutor {
    fn select(&self, set, columns, filter) -> Result<Vec<Vec<Value>>>;
    fn aggregate(&self, set: &ColumnarSet, req: &AggregateRequest) -> Result<AggregateResponse>;
}
// Portable normative implementation lives in loom-core (no Polars):
pub fn columnar_aggregate(loom, ns, name, req: &AggregateRequest) -> Result<AggregateResponse>;
// Auto: portable below threshold or when exec is None, native above.
pub fn columnar_aggregate_auto(loom, ns, name, req, exec: Option<&dyn ColumnarExecutor>, threshold)
    -> Result<AggregateResponse>;
```

### 12.3 Aggregate request / response model (the AST)

```
AggregateRequest {
  group_by: Vec<String>,            // grouping columns, in output key order; empty = whole-dataset agg
  aggs: Vec<AggSpec>,               // output aggregate columns, in order
  filter: Option<(String, CmpOp, Value)>,  // pre-aggregation predicate (same shape as select)
  having: Option<(usize, CmpOp, Value)>,   // post-aggregation predicate over an agg output index
  order_by: Vec<(usize, bool)>,     // (output column index, ascending) - applied after having
  limit: u32, offset: u32,          // 0 = unbounded
}
AggSpec { func: AggFunc, column: Option<String>, alias: String }  // column None only for Count(*)
AggregateResponse { reduced: bool, columns: Vec<String>, rows: Vec<Vec<Value>> }
```

Canonical CBOR wire form (ABI/bindings):
- request `= [ [group_by_text...], [aggspec...], filter_opt, having_opt, [order_by...], limit, offset ]`
  where `aggspec = [func_tag, column_opt_text, alias_text]`,
  `filter_opt = null | [col, op, value_cell]`,
  `having_opt = null | [out_index, op, value_cell]`,
  `order_by item = [out_index, ascending_bool]`.
- response `= [ reduced_bool, [columns_text...], [ row=[cell...] ... ] ]` (same cell codec as `select`).

Grouping key order: rows are grouped by the tuple of `group_by` column values under the typed `Value`
total order; output rows are emitted in ascending group-key order (then `order_by` reorders if present).
`reduced` is `true` whenever produced by the portable path (parity with `select`/`search`).

### 12.4 Aggregate functions and pinned determinism (Q8=C, Q10=A)

Land in two tiers; each function's algorithm is normative and golden-vectored.

Tier 1 (exact / integer-safe - implement first):
- `count` (rows in group), `count(*)` (column None), `count_distinct` (exact, via the typed
  `Value` total order set), `min`, `max`, `sum_int` (i128 accumulator; overflow is an error),
  `first`, `last` (group-input order), `group_concat(sep)` (group-input order, explicit separator).

Tier 2 (float-dependent - pin the math):
- `sum` / `mean` over floats: **ordered left-to-right reduction in row/index order** (no reordered or
  tree summation) so the portable and Polars paths agree bit-for-bit; document that Polars must be
  driven in a matching ordered reduction or its result re-derived.
- `var` / `std`: **sample** (denominator `n-1`) by default; population variants are named `var_pop` /
  `std_pop`. Single-element groups: sample var/std = `Null`.
- `median` / `quantile(q)`: **type-7 linear interpolation** on the sorted group (the R/NumPy default).
- Null handling: aggregates skip `Null` inputs (SQL semantics); `count(*)` counts rows including nulls;
  `count(col)` counts non-null; an all-null/empty group yields `Null` for `sum`/`mean`/`min`/`max` and
  `0` for `count`.
- Decimal: `sum`/`mean` over `Decimal` accumulate in exact decimal (mantissa/scale), never via float.

### 12.5 Value <-> Polars dtype mapping

Map native where a faithful Polars dtype exists (`Int*`/`UInt*` -> integer, `F32`/`Float` -> float,
`Text` -> Utf8, `Bool` -> Boolean, `Date`/`Time`/`Timestamp` -> temporal). Types with no faithful Polars
dtype (`Decimal{mantissa,scale}`, `Uuid`, `Inet`, `Point`, `I128`/`U128`) cross as their canonical-CBOR
cell **bytes** (Polars `Binary`) and are excluded from native aggregation kernels - any aggregate over a
byte-fallback column falls back to the portable aggregator for that column. Float bit-patterns
(`NaN`, `-0.0`, infinities) follow the cell codec (raw bits), and the portable path is authoritative for
their ordering.

### 12.6 Implementation slices (for the separate session)

- **agg-A (portable, sandbox-verifiable):** add `AggregateRequest`/`AggregateResponse`/`AggSpec`/`AggFunc`
  + CBOR codec + the portable Rust aggregator for Tier 1 functions on the columnar facet, plus
  `columnar_aggregate` / `columnar_aggregate_auto` and the `ColumnarExecutor::aggregate` trait method
  (default uses the portable path). Golden conformance vectors for Tier 1.
- **agg-B:** Tier 2 float-dependent functions with the pinned reductions + golden vectors.
- **agg-C (native, Mac-only):** `loom-polars` crate implementing `ColumnarExecutor` (select + aggregate),
  reconciled to the agg-A/B vectors, with the row-count auto-switch threshold. Predicate evaluated in
  Rust; Polars for kernels only.
- **agg-D:** project the aggregate request/response across the C ABI (`loom_columnar_aggregate_cbor`),
  IDL (`Columnar.aggregate`), C header, and all 8 bindings, mirroring the existing columnar wrappers.
- **agg-E:** spec - flip RD/Non-Goals lines as each tier lands; record the change log.

### 12.7 Conformance

A golden-vector suite is mandatory: each vector is `(dataset, AggregateRequest) -> expected response
bytes`, run against **both** the portable aggregator and (when built) the `loom-polars` executor; the two
MUST match the pinned bytes. Cover: empty/all-null groups, single-element groups (var/std null),
float ordered-sum determinism, decimal exactness, `count` vs `count(*)` vs `count(col)`, `group_concat`
ordering, `having`/`order_by`/`limit`/`offset`, and byte-fallback columns forcing the portable path.

## Change log

### 0.1.0

The columnar facet's workspace-scoped public facade is now source-backed end to end, mirroring the KV
and CAS facets: dataset create, row append, ordered scan, column schema, row count, and the StateAccess
predicate select cross the C ABI (`loom_columnar_*`), the IDL `Columnar` interface, the C header, and
the Node, Python, C++, iOS, JVM, Android, React Native, and WASM bindings. A column schema crosses as
canonical CBOR `[name, type_tag]`, a row as a CBOR cell array, and the select filter as
`[column, op, value_cell]`. The MCP `columnar_select` tool also accepts the 0061 JSON predicate root for
the subset that lowers to that existing single-column comparison filter through
`loom-substrate::predicate`; unsupported full-grammar nodes fail closed until the columnar evaluator
executes the full predicate tree. An executable behavioral suite exercises the facade against the
in-memory store, so the `columnar` capability is reported executable. The
`ColumnarExecutor` seam is source-backed: `columnar_select_auto` runs the portable `ColumnarSet::select`
when no executor is injected and delegates to a native executor otherwise, with a reconciliation contract
(identical rows in identical order, so the switch is invisible except in speed), mirroring the vector
`VectorAccelerator` seam. The Polars-backed native executor that plugs into this seam remains deferred
behind a feature gate in a separate crate and is not part of the source-backed contract.
Hosted native gRPC now serves `loom.hosted.v1.Columnar` for the current facade and feature-gated Arrow
IPC and Parquet transfer methods over the same canonical CBOR codecs used by local and binding
projections.
