# P9-0009 - `columnar` Binding

**Series:** P9 binding plan (normative-track sub-series; Draft)
**Version:** 0.1.0-draft. **Status:** Draft. **Last updated:** 2026-06-25
**Reads first:** [`P9-0002-projection-conventions.md`](./P9-0002-projection-conventions.md), facade spec
**0023** (Columnar), ADR-0008, [`IMPLEMENTATION-FIDELITY.md`](./IMPLEMENTATION-FIDELITY.md).

## 1. Current Source Boundary

`loom-core::columnar` stores each named dataset as one canonical manifest blob at
`/.loom/facets/columnar/<name>` through the workspace working tree.

The source-backed model is:

- named columns using `tabular::ColumnType`;
- rows as `tabular::Value` arrays;
- row arity and type validation;
- append-order scans;
- segment rolling as an identity-affecting ordered manifest layout;
- compaction that preserves logical row order and may change canonical identity;
- canonical profile-aware manifest encode/decode;
- deterministic committed segment statistics;
- commit, checkout, clone, bundle, and sync through ordinary workspace file contents;
- IDL, CLI, C ABI/header, and local MCP projections for create, append, compact, inspect,
  source-digest, scan, columns, rows, select, and aggregate.

Hosted native REST, JSON-RPC, and gRPC listeners are source-backed for create, append, scan, columns,
rows, compact, inspect, source-digest, select, and aggregate. Native gRPC serves
`loom.hosted.v1.Columnar` over daemon-opened `columnar/grpc` listeners. It uses the shared columnar
canonical-CBOR request/response codecs for schema, row, projection, filter, aggregate, result rows,
values, inspect, and digest payloads. Hosted REST and native gRPC binary Arrow IPC and Parquet
import/export are source-backed behind `loom-hosted/columnar-arrow`, with default builds returning
stable `UNSUPPORTED`. Prepared REST Arrow IPC result handles are source-backed as principal-bound,
session-bound, one-shot `/_loom/results/{handle}` reads with PEP rechecks. There is no source-backed
Arrow Flight or Flight SQL data-plane transfer, separate Parquet segment file storage, segment-level
merge, generated protobuf artifact, or public hosted conformance runner today.
Polars-backed dataframe execution is owned by 0045.

### 1.1 Binding Boundary

The current base layer is a versioned Loom columnar dataset encoded as a Loom canonical manifest over
deterministic native segment bytes. Parquet is the next durable persistent segment codec. Arrow is the
runtime, IPC, and Flight batch model. Native projections expose create, append, scan, columns, rows,
and select. Arrow Flight, Parquet import/export, and warehouse/job-style surfaces are presentations or
interchange. Dataframe transformation workflows and Polars-backed execution are owned by 0045.

## 2. Target Facade Surface

The source-backed local facade is smaller than the long-term Arrow/Parquet target:

```text
create(dataset: string, columns: List<Column>, target_segment_rows: u64)
append(dataset: string, row: Row)
compact(dataset: string)
inspect(dataset: string) -> Inspect
source_digest(dataset: string) -> Digest
scan(dataset: string) -> List<Row>
columns(dataset: string) -> List<Column>
rows(dataset: string) -> u64
select(dataset: string, columns: List<string>, filter: Predicate) -> List<Row>
aggregate(dataset: string, aggregates: List<Aggregate>, filter: Predicate) -> List<Value>
```

Predicate pushdown, hosted Flight-style data-plane projection, generated protobuf artifacts, and
engine-specific aggregate query strings remain target extensions until the Arrow/Parquet profile is
pinned.

Use the stable core error set until a columnar-specific `Code` is added. Do not claim
`DATASET_NOT_FOUND` as implemented unless the stable `Code` enum grows that variant.

## 3. Tier-1 REST

Facet-root `/v1/workspaces/{workspace_id}/columnar`:

| Facade method | HTTP |
| --- | --- |
| `create` | `PUT /columnar/{dataset}` with schema body |
| `append` | `POST /columnar/{dataset}/rows` |
| `compact` | `POST /columnar/{dataset}:compact` |
| `inspect` | `GET /columnar/{dataset}` |
| `source_digest` | `GET /columnar/{dataset}/source-digest` |
| `scan` | `GET /columnar/{dataset}/rows` |
| `columns` | `GET /columnar/{dataset}/columns` |
| `rows` | `GET /columnar/{dataset}/length` |
| `select` | `POST /columnar/{dataset}:select` |
| `aggregate` | `POST /columnar/{dataset}:aggregate` |
| `export_arrow_ipc` / `import_arrow_ipc` | `GET` / `PUT /columnar/{dataset}/arrow-ipc` |
| `export_parquet` / `import_parquet` | `GET` / `PUT /columnar/{dataset}/parquet` |

The REST contract is source-backed as a management and convenience surface. Feature-gated Arrow IPC
and Parquet binary REST bodies are source-backed. Prepared Arrow IPC result handles are source-backed
for route-level hosted analytical transfer. Arrow Flight, Flight SQL, or ADBC-adjacent data-plane
access remain target work unless explicitly deferred to a later hosted data-plane task.

## 4. Tier-1 JSON-RPC

Source-backed methods: `columnar.create`, `columnar.append`, `columnar.compact`,
`columnar.inspect`, `columnar.source_digest`, `columnar.scan`, `columnar.columns`,
`columnar.rows`, `columnar.select`, and `columnar.aggregate`.

## 5. Tier-1 gRPC

Source-backed native gRPC serves `loom.hosted.v1.Columnar` for:

- `Create`, `Append`, `Compact`;
- `Inspect`, `SourceDigest`, `Scan`, `Columns`, `Rows`;
- `Select`, `Aggregate`;
- `ExportArrowIpc`, `ImportArrowIpc`, `ExportParquet`, `ImportParquet`.

The service is listener-scoped to one workspace and dataset. Schema, rows, projection lists, filters,
aggregates, result rows, scalar result values, inspect reports, and source digests cross as the same
canonical CBOR used by the local, remote, C ABI, CLI, and MCP projections. Arrow IPC and Parquet
transfer methods return stable `UNSUPPORTED` unless the `columnar-arrow` feature is enabled.

Server-streaming scans/selects, generated protobuf artifacts, Arrow Flight, Flight SQL, and
ADBC-adjacent access remain target data-plane work.

## 6. Tier-1 MCP

- **Read tools:** `columnar.scan`, `columnar.columns`, `columnar.rows`, `columnar.inspect`,
  `columnar.source_digest`, `columnar.select`, `columnar.aggregate`.
- **Write tools:** `columnar.create`, `columnar.append`, `columnar.compact`, token-gated per P9-0002
  section 5.

## 7. Tier-2 Foreign Adapter

Arrow Flight remains the preferred enterprise data-plane target. Arrow IPC and Parquet import/export
are source-backed for native CLI and feature-gated hosted REST, not MCP. Further promotion requires:

- scalar mapping for every supported `tabular::Value`;
- null semantics;
- schema metadata profile;
- segment sizing and compression policy;
- statistics identity policy for any Arrow/Parquet-only statistics;
- deterministic import/export vectors;
- platform parity decisions for native and wasm builds.
- a decision that generic REST, JSON-RPC, or gRPC is management-only unless client demand proves a
  data-plane use.

### 7.1 Analytical presentation grouping

Columnar is the durable analytical dataset owner inside the wider SQL/vector/dataframe analytical
family. Its presentation work should be batched with SQL wire and dataframe result-transfer work where
possible, because the same clients often expect all three behaviors.

| Presentation | Columnar role | Boundary |
| --- | --- | --- |
| DuckDB-like local analytical SQL | Query columnar datasets with an embedded/local analytical feel. | Loom must not expose DuckDB as the public surface name or make DuckDB the storage engine. |
| PostgreSQL-wire plus pgvector | Serve SQL clients that may query columnar views and vector operators. | SQL wire owns the client protocol. Columnar owns analytical dataset identity. Vector owns similarity semantics. |
| Arrow Flight or Flight SQL | Move high-volume result batches to analytical clients. | Arrow batches are result/data-plane transfer. Canonical columnar identity remains the Loom manifest and segment profile. |
| Dataframe materialization | Receive transformed outputs from dataframe plans as committed datasets. | Dataframe owns plans and lineage before materialization. Columnar owns the committed output dataset. |
| Snowflake-like, Spark-like, BigQuery-like | Later warehouse, batch, or job-style presentations over the same analytical base. | These profiles must reuse columnar schema, auth, result transport, and conformance work instead of creating new base models. |

Analytical presentation work should reuse this grouping. A standalone columnar listener is not enough
to close the analytical client ecosystem gap unless it also pins the result transport and
compatibility evidence expected by that client family.

## 8. Errors, Parity, and Concurrency

- **Errors:** current source uses the stable core error set. A columnar-specific code is target work.
- **Parity:** the current Rust substrate is portable. Native OLAP acceleration is target-only.
- **Concurrency:** current storage is one dataset blob, so same-path edits conflict at the workspace
  tree boundary. Segment-level merge is target work and follows `CONFLICT-RESOLUTION-MATRIX.md`.

## 9. Resolved Decisions

- **RD1 - Current storage.** Current source stores a versioned canonical manifest with embedded native
  segment records, not Arrow or Parquet bytes.
- **RD2 - Public facade status.** The local Rust, IDL, C ABI/header, CLI, MCP, hosted REST,
  hosted JSON-RPC, and native hosted gRPC facade is source-backed for create, append, compact,
  inspect, source-digest, scan, columns, rows, select, and aggregate. REST and native gRPC
  Arrow/Parquet import/export are source-backed behind `columnar-arrow`. Language-wrapper parity,
  generated protobuf artifacts, Arrow Flight, and broader hosted conformance remain target work.
- **RD3 - Interop priority.** Arrow/Parquet remains the enterprise target, but it must be implemented
  and conformance-pinned before bindings advertise compatibility.
- **RD4 - Base model.** The current columnar base is a Loom canonical manifest over deterministic
  native segment bytes. Parquet is the next durable segment codec. Arrow is the runtime, IPC, and
  Flight batch model.
- **RD5 - Type profile.** The target type system is a required portable profile plus optional extended
  Arrow types, designed so full Arrow coverage can be promoted later.
- **RD6 - Unreleased format transition.** The old row-encoded blobs are not a legacy contract and have
  been replaced by the v2 manifest format.
- **RD7 - Statistics identity.** A small deterministic statistics profile is committed in the v2
  manifest; richer engine-specific metadata is derived.
- **RD8 - Client-first wire posture.** Parquet import/export, Arrow runtime batches, Arrow Flight, and
  Flight SQL or ADBC-adjacent access lead the data-plane design. Generic REST, JSON-RPC, and native
  gRPC are management/native projections. They are not substitutes for analytical client data-plane
  compatibility.
- **RD9 - Presentation order.** DuckDB-like local analytical SQL is the P0 presentation after the base
  profile; Snowflake-like, Spark-like, and BigQuery-like presentations are P1.
- **RD10 - Engine boundary.** DataFusion is eliminated from the v1 plan. Polars is the default native
  execution layer for the 0045 dataframe facet, not a columnar storage dependency.

## 10. Open Questions

### OQ-CO1 - Aggregate query dialect over the wire

- **Context.** An aggregate string couples the public API to an engine dialect unless Loom defines a
  portable grammar.
- **Example.** A native Polars-backed query might fail on wasm if only the Rust substrate is available.
- **Options.** (a) define a small portable aggregate grammar; (b) pass through engine-specific strings
  and report the dialect in capabilities; (c) expose scan only and let clients aggregate.
- **Recommendation.** (a) for a later P1 facade extension, with capability-gated native extensions.
