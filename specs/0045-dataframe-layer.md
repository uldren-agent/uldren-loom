# 0045 - DataFrame Layer

**Status:** Draft target with source-backed local and REST substrate. **Version:** 0.1.0-draft.
**Capability:** `dataframe`.

> **Exec integration.** The dataframe facet is reachable today through its own local API, REST
> surface, and Rust `StateAccess` API. Invoking dataframe operations from inside a guest WASM program
> is the remaining exec-side surface tracked under 0015; until then, `exec` programs reach dataframe
> state only through Rust `StateAccess`, not through guest WASM host calls.

This spec defines the dataframe facet: a versioned transformation and data-preparation layer over
tabular inputs. The dataframe facet is distinct from `columnar`. `columnar` owns durable analytical
datasets and their canonical segment identity. `dataframe` owns logical frames, source bindings,
schema inference records, transformation plans, previews, materialization policy, and lineage.

In hosted syntax, `dataframe` is a `loom serve <surface> ...` surface. Polars is the default native
execution layer behind the dataframe facet where the runtime profile supports it. Polars does not own
Loom identity, storage bytes, authorization, audit, or conformance semantics.

The current source-backed substrate is split between `loom-dataframe` and `loom-core`.
`loom-dataframe` owns the reusable dataframe plan/source/schema/materialization/batch/executor model,
canonical plan encoding and validation, plan digest domain hashing, CSV/JSON/NDJSON portable parsing,
schema inference, loaded-batch portable execution, and row/scalar coercion helpers. Shared digest,
error, and tabular scalar contracts live in `loom-types`. `loom-core` consumes and re-exports those
contracts, stores dataframe logical plans as canonical Loom CBOR under the `dataframe` facet, adds
`FacetKind::Dataframe`, and reports the `dataframe` capability as executable in the current 0010
capability table. It still owns source-digest listing, ACL collection enforcement, nested frame-id
storage, source loading, materialization hooks, and SQL-result decoding. Arrow IPC and Parquet
adapters route through the extracted `loom-columnar` crate via the core `columnar-arrow`
compatibility feature.

Current source-backed execution includes deterministic CSV, JSON, and NDJSON parsing over `files` and
`cas` sources, native Loom `columnar` source loading, a portable deterministic executor for scan,
select, rename, cast, simple filters, sort, limit, seeded sample, aggregate, literal with-column, and
union, plus materialization into `columnar`, `files`, `cas`, and ephemeral preview. Arrow IPC and
Parquet input/export are source-backed behind the `columnar-arrow` feature. CAS-backed canonical SQL
result loading is source-backed for row-shaped `Rows`, `Select`, and `SelectMap` envelopes. Join
semantics, pivot, unpivot, window operations, broader native Polars operation coverage, and hosted
result transfer remain target work.

Current source-backed projection includes CLI commands for create, collect, preview, materialize,
plan-digest, and source-digests; IDL and C ABI entries over canonical plan and batch CBOR; generated C
headers including the iOS C shim header; C++, Swift, JVM, Android KMP, React Native, Node, Python,
and wasm OPFS raw-CBOR wrapper surfaces; MCP engine and server tools for the same core flow; hosted
`dataframe/rest` management routes; and shared conformance coverage for local dataframe
collect/materialization/versioning behavior. These projections route through the stable `loom-core`
re-export and C ABI surfaces rather than depending directly on `loom-dataframe`. Rich decoded
host-language dataframe objects, Arrow Flight, and broader hosted result transfer remain target work.

## 1. Problem Statement

Analytical users do not only start from Parquet-backed columnar datasets. They start from CSV, JSON,
NDJSON, Arrow, Parquet, SQL results, logs, application exports, uploaded files, and existing columnar
datasets. They expect dataframe-style transformation workflows before committing clean analytical
outputs.

Pushing all of that into `columnar` would make the columnar facet too broad. It would mix durable
storage identity with messy input handling, schema inference, data cleaning, joins, pivots, windows,
and export workflows. The dataframe facet keeps those concerns first-class without making Polars or
any other engine the storage contract.

## 2. Scope

The dataframe facet targets:

- named logical frames;
- source bindings to files, CAS blobs, columnar datasets, SQL results, and future hosted inputs;
- input adapters for CSV, JSON, NDJSON, Arrow IPC, Parquet, and columnar datasets;
- schema inference and schema override records;
- typed transformation plans;
- lazy execution;
- preview and sampling;
- collect/export operations;
- materialization into `columnar` datasets;
- lineage over source digests, plan digests, and output digests;
- hosted `dataframe` surfaces where real client value exists;
- local CLI, bindings, MCP, and conformance projections.

Excel, ORC, Avro, database connectors, cloud object-store connectors, and streaming inputs are target
extensions after the v1 adapter and execution contracts are pinned.

## 3. Non-Goals

- The dataframe facet does not make Polars-native binary state canonical.
- The dataframe facet does not replace `columnar` durable analytical storage.
- The dataframe facet does not replace `sql` for relational query semantics.
- The dataframe facet does not define a distributed compute cluster.
- The dataframe facet does not make DataFusion part of the Loom runtime.

## 4. Relationship To Other Facets

| Facet | Relationship |
| --- | --- |
| `files` | Stores raw input files such as CSV, JSON, NDJSON, Arrow IPC, and Parquet exports. |
| `cas` | Stores immutable input blobs and export blobs by digest. |
| `columnar` | Stores committed analytical outputs as versioned datasets, with Loom manifest identity and Parquet segments once 0023 is promoted. |
| `sql` | Provides relational query inputs and may query materialized dataframe outputs. |
| `time-series` | Can feed dataframe transformations and can use dataframe outputs for derived rollups after both contracts are pinned. |
| `search`, `vector`, `graph` | Can consume dataframe outputs as ingestion tables, but do not depend on dataframe for their base identity. |

The canonical pipeline is:

```text
files/cas/sql/columnar inputs -> dataframe plan -> preview/export/materialize -> columnar or files/cas output
```

## 5. Data Model

A dataframe record is a Loom-readable logical plan, not an engine snapshot. It contains:

- frame id;
- source bindings;
- source digests or version anchors;
- input adapter profile;
- schema inference result and optional schema override;
- ordered transformation plan;
- execution profile and required capabilities;
- materialization policy;
- lineage stamps;
- optional derived-artifact references.

Source bindings are immutable by digest or explicit version anchor when reproducibility is required.
Mutable inputs can be referenced only with a policy that states whether refresh is allowed.

Current source-backed record storage uses `DataframePlan`:

| Field | Current source-backed shape |
| --- | --- |
| Sources | `DataframeSourceBinding` rows with alias, source kind, target, input format, optional source digest, and sorted string options. |
| Source kinds | `files`, `cas`, `columnar`, and `sql-result`. |
| Input formats | `native`, `csv`, `json`, `ndjson`, `arrow-ipc`, and `parquet`. |
| Schema | Optional `DataframeSchema` with ordered typed columns and an `inferred` flag. |
| Operations | Ordered `DataframeOperation` records for scan, select, rename, cast, filter, sort, limit, sample, join, union, aggregate, and with-column. |
| Materialization | Optional target policy for `columnar`, `files`, `cas`, or ephemeral preview. |
| Digests | `loom-dataframe` hashes canonical plan bytes with a Loom dataframe domain separator; `loom-core::dataframe_plan_digest` authorizes and applies the current store digest algorithm. Source digests are read from source bindings. |

Shared record dependencies:

| Contract | Current source-backed owner |
| --- | --- |
| Digest and hashing profile | `loom-types::digest`, re-exported by `loom-core`. |
| Stable error codes and `LoomError` | `loom-types::error`, re-exported by `loom-core`. |
| `ColumnType`, tabular scalar `Value`, `Row`, comparison operators, and canonical cell codecs | `loom-types::tabular`, re-exported through `loom_core::tabular`. |
| Dataframe plan, source, schema, materialization, batch, executor trait, canonical plan bytes, plan digest domain hashing, CSV/JSON/NDJSON portable parsing, schema inference, row/scalar coercion, and loaded-batch portable execution | `loom-dataframe`, re-exported through `loom_core::dataframe` and top-level `loom_core` where the public core surface requires it. |
| Columnar model, segment manifest, select/aggregate executor seam, and Arrow/Parquet interchange projection | `loom-columnar`, re-exported through `loom_core::columnar` and `loom_core::columnar_arrow` where the public core surface requires it. |
| Workspace, ACL, CAS, files, columnar, and `Loom<S>` storage hooks | `loom-core`; these remain kernel integration points and are not owned by the reusable dataframe component. |

## 6. Transformation Plan

The v1 portable plan should include:

- select;
- rename;
- cast;
- filter;
- sort;
- limit;
- sample with pinned seed;
- join;
- union;
- group-by;
- aggregate;
- pivot and unpivot;
- window operations after determinism rules are pinned;
- with-column expressions over a restricted expression set.

Current source backs the operation records for all rows above except pivot, unpivot, and window.
Execution semantics, expression grammar, null rules, and ordering rules are still target work.

Every operation needs deterministic result ordering rules, null semantics, error mapping, and
conformance vectors before it is promoted.

## 7. Execution Model

Native profiles use Polars as the default dataframe executor. The executor receives Loom logical plans
and returns Loom-defined result shapes. The executor must not change Loom semantics.
The source-backed native executor lives in `loom-polars`, a reusable crate over `loom-dataframe`,
`loom-types`, and Polars. `loom-core` keeps the compatibility feature name `dataframe-polars`, but the
feature enables `loom-polars`; default `loom-core` builds do not link Polars.

The default execution policy is:

| Runtime profile | Dataframe execution posture |
| --- | --- |
| Native with Polars | Source-backed optional `loom-polars` executor for scan, select, rename, cast, sort, limit, literal with-column, and same-schema union over Int, Float, Text, and Bool batches; operations or scalar types outside that subset fall back to the portable executor with explicit native-versus-fallback execution reporting. |
| Native without Polars | Current source-backed portable executor for the deterministic subset; capability-gated unsupported for operations outside that subset. |
| `wasm32` | Unsupported or degraded for transform execution unless a portable implementation is promoted. |

DataFusion is eliminated from the dataframe and columnar v1 plan. It can be reconsidered only through a
new design decision with a concrete role, platform profile, and conformance strategy.

## 8. Storage And Materialization

Dataframe plans are versioned Loom source data. Execution caches and materialized intermediate frames
are derived artifacts unless explicitly committed as an output. Durable local dataframe
materialization records use `loom-store::derived` keys under `materialization:<materialization-id>`
with source-digest, engine-version, format-version, stale, rebuild, failed, and unsupported status
reporting. The source digest is supplied by the caller and must cover the canonical dataframe plan
digest plus the source digests or version anchors that make the materialization reproducible.

Materialization targets:

| Target | Behavior |
| --- | --- |
| `columnar` | Commit a versioned analytical dataset, using the 0023 columnar storage profile. |
| `files` | Export CSV, JSON, NDJSON, Arrow IPC, or Parquet files into a workspace path. |
| `cas` | Export immutable result blobs by digest. |
| ephemeral preview | Return bounded records without committing output. |

The dataframe facet should support ephemeral frames, but ephemeral frames are not versioned, synced, or
used as durable identity. This mirrors the broader Loom rule that derived runtime state is rebuilt from
source data unless a spec explicitly promotes it.

Shared derived-artifact lifecycle records use artifact family `materialization:` and format version
`dataframe-materialization-v1`. These records describe rebuildable local bytes only; committed
`columnar`, `files`, or `cas` outputs remain owned by their target facets after materialization.

## 9. Hosted Surface

`dataframe` is a served surface name:

```text
loom serve configure <store> dataframe <workspace> <frame> --bind <addr> --transport <transport>
```

The first hosted transports should be chosen from real client value, not from generic protocol
symmetry. Candidate transports:

| Transport | Priority | Rationale |
| --- | --- | --- |
| Native Loom REST management | P1 | Useful for creating frames, binding sources, requesting previews, and materializing outputs. |
| Arrow Flight | P1 | Useful for high-volume frame result transfer once Arrow batch semantics are pinned in 0023. |
| Flight SQL or ADBC-adjacent | P1 | Useful where dataframe output is consumed by analytical clients through SQL-shaped tooling. |
| JSON-RPC | P2 | Useful only if bindings or MCP symmetry creates clear value. |
| Generic gRPC | P2 | Useful only for generated clients after the operation model is stable. |

Hosted writes must reuse the hosted kernel for auth, PEP, stable errors, auditing, request limits, and
store save behavior.

Current source backs `dataframe/rest` for:

- `POST /dataframe:create` with `plan_cbor_hex`;
- `POST /dataframe:collect`;
- `POST /dataframe:preview`;
- `POST /dataframe:materialize`;
- `POST /dataframe:plan-digest`;
- `POST /dataframe:source-digests`.

The durable served-listener grammar accepts `dataframe <workspace> <frame> --transport rest`, and the
daemon opens the REST listener through the shared hosted data kernel.

Arrow Flight, Flight SQL or ADBC-adjacent result transfer, JSON-RPC, and generic gRPC are not
source-backed in the current implementation. The hosted protocol inventory records
`dataframe/arrow-flight-flight-sql` as the target binary result-transfer row and
`dataframe/json-rpc-grpc` as the target generated-client row. Those transports remain assigned to the
cross-protocol hosted work in 310/320 so dataframe v1 does not grow a low-value generic wire surface
before Arrow batch semantics and conformance transcripts are pinned.

## 9.1 Analytical Presentation Grouping

Dataframe belongs to the analytical presentation family, but it is not a storage substitute for
columnar and not a SQL dialect owner. It connects the messy input and transformation side of the system
to durable analytical outputs and SQL-facing client workflows.

| Flow | Dataframe responsibility | Adjacent owner |
| --- | --- | --- |
| SQL result input | Bind a canonical SQL result as a dataframe source, preserving source digest or version anchor. | SQL owns query execution, result shape, stable SQL errors, and authorization for the source query. |
| Columnar input | Load a committed columnar dataset into a dataframe plan for transformation. | Columnar owns committed dataset identity and segment profile. |
| Columnar materialization | Commit transformed output as a versioned columnar dataset. | Columnar owns the output dataset after materialization. |
| Arrow result transfer | Return large collected or previewed results through an Arrow batch-oriented transport when promoted. | 0023 owns the Arrow value profile; hosted serving owns result-handle authorization and expiry. |
| DuckDB-like analytical SQL | Supply transformed or materialized data to an analytical SQL presentation. | The analytical SQL presentation owns client grammar and query behavior, not dataframe plan identity. |
| pgvector-style workflows | Transform source data that may later feed vector sets or SQL-visible vector columns. | Vector owns metrics, dimensions, metadata filters, and pgvector-style operator semantics through SQL. |

Task 340 should treat dataframe SQL-result input and dataframe-to-columnar materialization as required
design inputs for SQL wire and analytical SQL work. It should not expose a broad generic hosted
dataframe protocol before Arrow result semantics, source-digest behavior, and conformance evidence are
pinned.

## 10. Public Surfaces

The target native facade should include:

```text
create_frame(frame, source_binding, options)
infer_schema(frame)
set_schema(frame, schema)
plan(frame, operations)
preview(frame, limit)
collect(frame)
materialize(frame, target)
export(frame, format, target)
describe(frame)
lineage(frame)
drop_frame(frame)
```

CLI, C ABI, IDL, local bindings, hosted protocols, and MCP must be updated together once the core
facade is pinned. Dataframe should not become a hidden Rust-only feature.

## 11. Execution Compliance (0015)

The dataframe facet is a program-grantable facet under the compute capability (0015). It is added to
`loom_core::FacetKind` as `Dataframe` and participates in the 0015 grant model, Rust `StateAccess`
surface, run-on-a-branch gate, determinism rules, and executable conformance. The facet axis of a
0015 program grant is `FacetKind` itself, so adding `Dataframe` to `FacetKind::ALL` makes it
grantable without a second facet enum to update.

### 11.1 Grant Model

A program declares a dataframe grant as facet plus mode plus scope, like any other facet (0015 grant
model). The scope prefix is a frame-id prefix: a prefix scope `etl/` covers frames whose id starts
`etl/`. Read mode covers schema inference, preview, collect, describe, and lineage; write mode covers
create_frame, set_schema, plan, materialize, export, and drop_frame. The facade enforces the
intersection of the principal's `Exec`-scoped ACL authority (0027/0028) and the program's manifest
dataframe grant, so a program touches a frame only when both permit it.

### 11.2 StateAccess Operations

The source-backed Rust `StateAccess` dataframe surface a program receives exposes create, put_plan,
get_plan, preview, collect, materialize, plan_digest, and source_digests. Each operation checks the
dataframe frame grant. Preview, collect, and materialize also preflight the plan's source bindings,
and materialize preflights the output target, so a dataframe grant cannot read Files, CAS, or Columnar
sources or write Files, CAS, or Columnar outputs without the corresponding facet grants. Richer public
facade verbs from section 10, such as set_schema, export, describe, lineage, and drop_frame, remain
target work until their source-backed shape and conformance are pinned.

### 11.3 Determinism, Metering, and Runtime Gating

0015 requires deterministic, metered, content-addressed, replayable execution with no ambient clock,
randomness, locale, or environment access. Only the deterministic dataframe operations (section 6:
pinned result ordering, null semantics, seeded sampling) are exposed to programs. Because native
execution is Polars-backed and native-only (section 7), dataframe `StateAccess` is capability-gated:
on a runtime without a deterministic dataframe executor (for example `wasm32`), dataframe operations
invoked under `exec` return the same unsupported or degraded status the facet reports elsewhere rather
than producing non-reproducible state. Materialized outputs are committed as real content-addressed
facet state (columnar, files, or cas) on the fork branch, so the run-on-a-branch proposed state root is
a genuine content address.

### 11.4 Execution Conformance

Dataframe execution still needs 0015 exec conformance beyond the facet-level conformance in section
12. The required runner must prove that a dataframe-writing program produces a deterministic proposed
state root, a denied dataframe write is rejected fail-closed, and an out-of-fuel program aborts before
any fork is created.

## 12. Conformance

Conformance must cover:

- CSV parsing profile;
- JSON and NDJSON parsing profile;
- Arrow IPC and Parquet import profile;
- schema inference determinism;
- schema override validation;
- transformation determinism;
- null and type coercion behavior;
- seeded sampling;
- join output ordering;
- aggregate output ordering;
- materialization into columnar;
- export bytes for pinned formats;
- capability reporting for native Polars support, unsupported runtimes, and degraded runtimes.

Current executable conformance covers a dataframe facade scenario in `uldren-loom-conformance`:
CSV-backed plan creation, deterministic collect, materialization into columnar, versioned input/output
behavior across commits, and clone preservation. Hosted HTTP test coverage exercises dataframe REST
create, collect, preview, plan digest, source digests with an explicit pinned source digest, and CAS
materialization. Source-backed unit coverage proves Arrow IPC and Parquet round-trips through the
columnar profile, direct canonical SQL result parsing, CAS-backed SQL-result source loading, native
Polars execution for the supported subset including Loom-coerced cast, literal with-column, and
same-schema union, plus explicit native-versus-portable-fallback execution reporting for unsupported
native Polars operations. Binding compile coverage currently includes Node, Python, wasm native Rust,
JVM Java, Android KMP JVM, Android host JNI C, SwiftPM, C++ header syntax, and RN Android C++ syntax.

## 13. Resolved Decisions

- **RD1 - Separate facet.** `dataframe` is a first-class facet, not only a presentation of `columnar`.
- **RD2 - Surface meaning.** `dataframe` is a `loom serve <surface> ...` surface when hosted.
- **RD3 - Engine posture.** Polars is the default native execution layer behind the dataframe facet.
- **RD4 - DataFusion.** DataFusion is eliminated from the v1 dataframe and columnar plan.
- **RD5 - Identity.** Loom-readable plans and materialized outputs own identity; Polars-native state
  does not.
- **RD6 - Columnar boundary.** `columnar` owns committed analytical datasets; `dataframe` owns
  transformation workflows over multiple input formats.
- **RD7 - Source-backed substrate.** The v1 storage substrate is Loom canonical CBOR over
  `DataframePlan`, with no Polars-native state in identity.
- **RD8 - Grantability.** `FacetKind::Dataframe` is in the stable facet set and is program-grantable
  through the 0015 facet-axis grant model.
- **RD9 - Binding parity.** Host projects, bindings, MCP, CLI, conformance, and hosted protocols are
  part of the dataframe promotion path, not optional follow-up polish.
- **RD10 - Execution compliance.** `dataframe` is program-grantable under 0015 via `FacetKind`; only
  deterministic operations are exposed to programs, dataframe `StateAccess` is capability-gated on
  runtimes without a deterministic executor, and materialized outputs commit as real content-addressed
  facet state on the run-on-a-branch fork.

## 14. Remaining Work

| Area | Remaining source-backed gap |
| --- | --- |
| Input adapters | CSV, JSON, NDJSON, native columnar, Arrow IPC, Parquet, and SQL-result loading are source-backed for the current promoted subset. Additional third-party messy-file adapters remain target work. |
| Native execution | Optional `loom-polars` acceleration is source-backed for the current native subset with explicit fallback reporting. Broader native Polars coverage for filter, seeded sample, aggregate, and supported joins remains target work, but portable execution remains the Loom-defined semantic fallback. |
| Transformation semantics | Join, pivot, unpivot, and window operations need pinned ordering, null, and type-coercion rules before promotion. |
| Public projection | CLI, IDL, C ABI, generated C headers, MCP projection, C++, Swift, JVM, Android KMP, React Native, Node, Python, and wasm OPFS expose the current raw-CBOR dataframe subset. Rich decoded host-language dataframe objects remain target ergonomics. |
| Hosted projection | REST management is source-backed. Arrow Flight, Flight SQL or ADBC-adjacent result transfer, JSON-RPC, and generic gRPC are explicit target rows assigned to 310/320 until Arrow batch semantics and client conformance are pinned. |
| Reporting | Capability reports distinguish dataframe, Arrow IPC, Parquet, SQL-result, and optional Polars support. Hosted protocol reports distinguish supported REST from target binary result transfer and generated-client transports. Dataframe materialization artifacts report stale, rebuild, failed, and unsupported status through the shared derived-artifact lifecycle; remaining work is final cross-report spec closure and native-versus-fallback run-report surfacing where callers request it. |
| Execution StateAccess | Rust `StateAccess` is source-backed for dataframe create, put_plan, get_plan, preview, collect, materialize, plan_digest, and source_digests, including source and materialization target grant preflight. WASM host ABI projection and executable conformance for the dataframe surface remain queued with the broader expanded StateAccess catch-up work. |
| Component extraction | `loom-types` and `loom-dataframe` are source-backed for shared contracts, dataframe model types, canonical plan bytes, plan digest domain hashing, CSV/JSON/NDJSON portable parsing, schema inference, loaded-batch portable execution, and row/scalar coercion. `loom-types` owns the shared tabular cell codec. `loom-columnar` owns columnar model and Arrow/Parquet interchange projection. `loom-polars` is source-backed as the optional native dataframe executor component. The `loom-core` storage/materialization boundary is source-backed around workspace, ACL, CAS, files, columnar, SQL-result decoding, and `FacetKind::Dataframe` kernel integration. |

## 15. Open Questions

### OQ-DF1 - Portable execution fallback

- **Context.** Native Polars support gives the useful dataframe behavior, but Loom still tracks
  platform parity and binding claims.
- **Example.** A wasm build may be able to store dataframe plans but not execute them.
- **Options.** (a) plan storage everywhere and execution only where Polars is available; (b) implement
  a limited portable executor; (c) block dataframe promotion until execution is available everywhere.
- **Recommendation.** (a) for v1, with explicit capability reporting.

### OQ-DF2 - First hosted transport

- **Context.** Hosted dataframe should be client-first. Generic REST and JSON-RPC should not become
  data-plane defaults unless clients need them.
- **Example.** Arrow Flight is useful for batch transfer; REST is useful for frame management.
- **Options.** (a) REST management plus Arrow Flight result transfer; (b) REST-only; (c) Arrow
  Flight-only; (d) Flight SQL or ADBC-adjacent first.
- **Recommendation.** (a) after 0023 pins Arrow batch semantics.

### OQ-DF3 - Input adapter order

- **Context.** CSV is the common messy starting point; Parquet and Arrow are the high-value analytical
  formats.
- **Example.** A user imports CSV, fixes schema, filters rows, then materializes Parquet-backed
  columnar output.
- **Options.** (a) CSV plus Parquet first; (b) CSV, JSON/NDJSON, Parquet, and Arrow together; (c)
  Parquet and Arrow first.
- **Recommendation.** (b) if implementation bandwidth allows, otherwise (a).
