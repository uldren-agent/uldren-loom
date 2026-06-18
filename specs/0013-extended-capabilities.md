# 0013 - Extended Capabilities Catalog

**Status:** Complete as a source-backed catalog. **Version:** 0.1.0.

This catalog records the promoted facet taxonomy and the optional adapter and consumer ideas that sit
beside the core Loom interface. It is not an implementation gate by itself. A data facet becomes
gating only through its owning spec and implementation plan entry, and an adapter or consumer becomes
gating only through a focused follow-up spec.

## 1. Current And Target Taxonomy

The current workspace facet tags are source-backed by `loom_core::workspace::FacetKind` where the
source-backed substrate column is populated. Target-only rows identify promoted design work without
claiming source support.

| Facet tag | Source-backed substrate | Owning spec | Current public contract status |
| --- | --- | --- | --- |
| `files` | `loom-core::fs`, `loom-core::vcs`, `loom-vfs` | 0003, 0003a, 0003c, 0014 | Source-backed whole-file, byte-range, handle, symlink create/read-link, directory, and local filesystem projection surface; symlink following and hosted projection remain target. |
| `sql` | `loom-core::tabular`, `loom-sql` | 0011 | Implemented tabular and GlueSQL surfaces; target generated SQL facade and hosted certification are tracked by 0011a. |
| `kv` | `loom-core::kv` | 0019 | Source-backed workspace-scoped public facade and executable conformance; structured prolly-map storage, CAS, and ephemeral cache tier remain target. |
| `document` | `loom-core::document` | 0020 | Source-backed workspace-scoped public facade and executable conformance; secondary indexes, structured document-map storage, and hosted projection remain target. |
| `vector` | `loom-core::vector`, `loom-core::vindex`, optional `loom-hnsw` | 0017 | Implemented exact substrate and native accelerator; public facade, structured storage, and merge remain target. |
| `graph` | `loom-core::graph` | 0016 | Implemented Rust substrate; target query/facade work remains incomplete. |
| `columnar` | `loom-core::columnar` | 0023 | Implemented Rust substrate; public facade, Arrow/Parquet projection, and scan engine remain target. |
| `dataframe` | - | 0045 | Target first-class transformation facet over CSV, JSON/NDJSON, Arrow, Parquet, SQL, files/CAS, and columnar inputs; Polars is the default native execution layer where supported. |
| `queue` | `loom-core::log` | 0021 | Source-backed structured append-log facade and authority-local consumer offsets with executable conformance; hosted projection and dequeue semantics remain target work. |
| `time-series` | `loom-core::timeseries` | 0022 | Source-backed workspace-scoped public facade and executable conformance; structured point storage, retention, and rollups remain target. |
| `cas` | `loom-core::cas` | 0024 | Source-backed workspace-scoped public facade, IDL/C ABI, selected bindings, and executable conformance; hosted projection and remaining binding breadth remain target. |
| `ledger` | `loom-core::ledger` | 0018 | Source-backed hash-chain facade and executable conformance; signing, transparency, retention, hosted projection, and remaining binding breadth remain target. |
| `calendar` | `loom-core::calendar` | 0037 | Source-backed local calendar/task-list facet and executable conformance; hosted CalDAV, auth-bound principals, and richer projection policy remain target. |
| `contacts` | `loom-core::contacts` | 0038 | Source-backed local contacts facet and executable conformance; hosted CardDAV, auth-bound principals, and richer projection policy remain target. |
| `mail` | `loom-core::mail` | 0039 | Source-backed local mail facet and executable conformance; MCP, hosted IMAP, filters, and auth-bound principals remain target. |
| `program` | `loom-compute` | 0015 | Implemented capability and execution substrate; target public `exec`/policy surface remains incomplete. |
| `lock` | - | 0036 | Target ephemeral control-plane facet: leased, reentrant, fenced exclusive/shared/semaphore locks. No substrate yet; lock state and fence counter are non-versioned and never synced. |

The workspace path convention is owned by 0014:

```text
/
/.loom
/.loom/facets/<facet>/...
```

User files occupy `/`. Non-files facets occupy `/.loom/facets/<facet>/...`. Source-backed generic
file projection writes are currently limited to the files facet; broader facet path read/write
interoperability is tracked by 0014a.

## 2. Current Implementation Boundaries

The following statements are source-backed today:

- `loom-core` exports substrate modules for CAS, columnar, document, graph, KV, ledger, append-log,
  SQL/tabular, time-series, vector, filesystem, workspace, sync, and VCS.
- Most non-SQL data facets currently store one canonical value or one structured root per named dataset
  through the workspace working tree. They version, branch, checkout, and sync as workspace entries, but
  several are now promoted as workspace-scoped public facades with executable conformance; each owning
  spec lists the remaining hosted, binding, storage, and merge work.
- `loom-core::vector` defines deterministic exact search as the portable vector contract.
  `loom-hnsw` is a native, rebuildable accelerator that must reconcile returned scores to the exact
  vector contract.
- `loom-core::ledger` pins store-profile-aware chain hashes and conformance vectors for the default
  and FIPS digest profiles.
- `loom-compute` defines a program capability vocabulary over files, SQL, key-value, document, graph,
  vector, columnar, dataframe, append-log, time-series, CAS, and ledger. Its promoted public execution
  surface is owned by 0015, not by this catalog.

The following are target work, not current source-backed public contracts:

- generated IDL, C ABI, language binding, and wire projections for every facet not already promoted by
  its owning spec;
- row/key/object-level merge behavior for non-SQL facets unless the owning spec and source prove it;
- served write policy, principal context, and access-control enforcement for adapters;
- REST, GraphQL, S3, Postgres-wire, MCP, and hosted filesystem adapters;
- RDF/SPARQL as a promoted facet;
- Cozo or another unified graph/vector/Datalog engine as a committed storage engine.

## 3. Promotion Rule

0013 can list a candidate, but it cannot make that candidate normative. Promotion requires:

- an owning spec or subspec;
- a source-backed data model;
- workspace and facet path behavior;
- merge and conflict behavior aligned with `CONFLICT-RESOLUTION-MATRIX.md`;
- stable error mapping through `loom_core::error::Code`;
- conformance vectors or executable behavior runners;
- IDL, C ABI, binding, and wire projection decisions where the surface is public;
- principal and access-control review for any served write path.

This rule prevents a catalog entry from being mistaken for an implemented enterprise contract.

## 4. Data-Facet Status

| Facet | Current storage shape | Current merge status | Target owner |
| --- | --- | --- | --- |
| Graph | One canonical graph value under the graph facet. | Same graph value conflicts are not promoted as node/edge merge. | 0016 |
| Vector | One canonical vector set plus optional rebuildable native HNSW accelerator. | Same vector-id merge remains target tooling. | 0017 |
| Ledger | One canonical hash-chained ledger value. | Append-only branch policy, signing, transparency, and retention remain target work. | 0018 |
| KV | One canonical typed key-value map. | Same-key merge and structured prolly-map storage remain target tooling. | 0019 |
| Document | One canonical id-keyed document collection. | Per-document merge, secondary indexes, and structured storage remain target tooling. | 0020 |
| Append-log / queue | Structured stream root with sequence-keyed entries and authority-local consumer offsets. | Observed stream anchors, dequeue semantics, hosted projection, and multi-writer ordering remain target facade work. | 0021, 0021a, 0021b |
| Time-series | One canonical timestamp-keyed series. | Same-timestamp merge, structured storage, retention, and rollups remain target work. | 0022 |
| Columnar | One canonical typed append-oriented dataset. | Same-segment edits, Arrow/Parquet projection, and scan engine remain target work. | 0023 |
| DataFrame | Target Loom-readable logical transformation plan over files/CAS, SQL, columnar, Arrow, Parquet, CSV, and JSON inputs. | Plan merge, schema-inference conflict handling, and materialized-output policy remain target work. | 0045 |
| CAS | Workspace-scoped reachable digest set. | Immutable blob union is the natural target merge; hosted projection and remaining binding breadth remain target work. | 0024 |
| Calendar | Structured records per principal and collection. | Hosted CalDAV, auth-bound principals, lifecycle hooks, and richer projection policy remain target work. | 0037 |
| Contacts | Structured records per principal and address book. | Hosted CardDAV, auth-bound principals, lifecycle hooks, and richer projection policy remain target work. | 0038 |
| Mail | Structured message index plus CAS body and versioned flags. | MCP, hosted IMAP, filters, auth-bound principals, and retention policy remain target work. | 0039 |

## 5. Adapter and Consumer Catalog

These ideas remain advisory until promoted:

| Candidate | Category | Target dependency | Notes |
| --- | --- | --- | --- |
| REST / JSON-RPC / gRPC | Wire adapter | 0008 | Protocol contracts need generated schemas, auth, and conformance. |
| GraphQL | Wire adapter | 0008 plus facade specs | Thin schema over promoted facades, not a core primitive. |
| S3-compatible endpoint | Foreign protocol adapter | 0008 plus 0026-0028 | Useful for files/CAS, but served write authority must be explicit. |
| Postgres-wire endpoint | Foreign protocol adapter | 0008 plus 0011 | Depends on portable SQL subset and transaction semantics. |
| MCP server | Agent adapter | 0008 plus 0009 and 0026-0028 | Should default read-only; writes require explicit capability grants. |
| Local filesystem mount | Consumer | 0003, 0003c, 0014, 0014a | Source-backed FUSE/NFS projection over `loom-vfs`; non-files facet projection depends on each owner. |
| CLI / REPL | Consumer | 0003, 0007, owning facades | CLI exists today, but REPL is not a promoted requirement. |

Git remote interoperability is closed out of scope. Foreign data import uses filesystem snapshots,
tables, or a separately promoted interchange contract, not a live git remote.

## 6. Resolved Decisions

- **RD1 - Catalog authority.** 0013 owns the facet taxonomy catalog, but implementation gates live in
  the owning specs and `IMPLEMENTATION-PLAN.md`.
- **RD2 - Facet tags.** The current source-backed facet tags are `files`, `sql`, `kv`, `document`,
  `vector`, `graph`, `columnar`, `queue`, `time-series`, `cas`, `ledger`, `calendar`, `contacts`,
  `mail`, and `program`. The target `dataframe` tag is promoted by 0045 only after its substrate and
  public projections are implemented.
- **RD3 - Relational naming.** The storage facet tag remains `sql`; the target language-neutral facade
  may be named `Db` in 0011 and public APIs. New facets should avoid this split.
- **RD4 - Derived indexes.** Derived indexes and accelerators are rebuilt from source state unless the
  owning spec explicitly promotes a persisted, content-addressed form.
- **RD5 - Adapter safety.** Served adapters must wait for principal and access-control decisions before
  public write paths are finalized.
- **RD6 - RDF/SPARQL.** RDF/SPARQL is not a promoted v1 facet. It is deferred until a concrete use case
  justifies a separate spec.
- **RD7 - Unified graph/vector engines.** Cozo or another unified graph/vector/Datalog engine can be
  evaluated later, but the current contract keeps graph and vector as separate facets.
- **RD8 - Sequencing.** The dependency table, not 0013's historical suggestions, controls review order.
