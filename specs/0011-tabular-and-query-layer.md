# 0011 - Tabular and Query Layer

**Status:** Complete for current source-backed SQL/table boundary; target facade expansion is split.
**Version:** 0.1.0.
**Capability:** `sql` and `tabular`.

This spec defines Loom's versioned relational layer. Current source implements a typed tabular
substrate, a GlueSQL frontend, CLI commands, C ABI surfaces, selected binding projections, and the
current curated MCP SQL tool projection. The fuller generated language-neutral SQL facade, REST/JSON-RPC/
gRPC and foreign-wire hosted projections, full `as_of` facade, schema-change diff envelope, normative
SQL subset, SQL-specific stable error codes, and analytical accelerator are split into 0011a.

Every operation is scoped to one workspace's SQL facet. Cross-workspace table operations are out of
contract and must fail with the stable cross-workspace error once the public facade exposes them.

## 1. Current Implementation

### 1.1 Tabular substrate

`loom-core::tabular` is the dependency-light, wasm-clean table substrate. It implements:

- typed schemas with primary keys and declared secondary indexes,
- rich scalar values using primitive carriers,
- canonical schema, cell, row, and table encodings,
- order-preserving key encoding for primary keys and indexes,
- row maps stored as prolly trees,
- table trees with `schema`, optional `rows`, and optional `index/<name>` entries,
- row-level diff,
- row-level three-way merge,
- opt-in cell-level merge,
- row blame,
- unique-index validation,
- index rebuild after merge,
- lazy row cursors for larger-than-RAM native reads.

The public Rust engine stages tables as `EntryKind::Table` working-tree entries. A table entry points to
a table `Tree` whose `schema` child is a Blob, `rows` is a prolly-map root when non-empty, and each
secondary index is an `index/<name>` prolly-map root when non-empty. Commit, checkout, merge, diff, sync,
reachability, and GC therefore see tables as first-class object graph members rather than opaque whole
database blobs.

`Table::encode` and `Table::decode` still exist as a canonical whole-table byte form for result payloads,
tests, and compact in-process uses. The source-backed committed form is the structured table tree.

### 1.2 SQL frontend

`crates/loom-sql` implements GlueSQL over the tabular substrate. It provides:

- `LoomSqlStore`,
- GlueSQL `Store` and `StoreMut` implementations,
- GlueSQL index, transaction, alter-table, and planner trait implementations,
- projection from GlueSQL schemas, keys, rows, and values to tabular schemas and rows,
- one synthetic `__key` primary-key column per SQL table,
- one typed tabular column per SQL column,
- schemaless table projection through a `__map` column,
- durable single-column identifier indexes,
- SQL `CREATE INDEX` and `DROP INDEX` projection to tabular secondary indexes,
- `ALTER TABLE` through GlueSQL with a full table re-stage,
- canonical-CBOR result payloads,
- JSON rendering as a debug projection from canonical CBOR,
- a deterministic SQL conformance commit digest,
- feature-backed Rust compute `StateAccess` operations for `sql_exec_cbor` and read-only
  `sql_query_cbor`,
- result payload conformance vectors.

The native SQL store uses a lazy base snapshot plus a transaction overlay. Base rows are read from the
durable table prolly maps on demand. The overlay holds only session changes and tombstones. Persisting a
session writes a catalog and stages one structured tabular table per SQL table under the workspace's SQL
facet.

Wasm and other backends that cannot provide a `Send` row stream can use eager loading. That path is
RAM-bound by design.

### 1.3 Current CLI surface

The `loom` CLI exposes:

- `loom sql <store> --workspace <UUID|name> --db <name> "<sql>"`,
- `loom table blame ...`,
- `loom table diff ...`,
- workspace commit, checkout, branch, and merge commands that include staged table entries,
- `loom merge --cell-level` for opt-in cell-level table reconciliation.

SQL commands run against a workspace's SQL facet and stage results into the workspace working tree.
The user commits that workspace through the normal workspace-history facade.

### 1.4 Current C ABI surface

The C ABI exposes:

- SQL sessions: open, keyed open, raw-KEK open, exec, commit, close,
- SQL batches: begin, keyed begin, raw-KEK begin, exec, commit, commit with VCS commit, abort, close,
- first-`SELECT` row iterators: `loom_sql_query`, `loom_iter_next`, `loom_iter_free`,
- row decoding: `loom_row_open`,
- result view decoding: columns, rows, cells, counts, strings, maps, diff rows, merge outcomes,
- table readers: read table, index scan, blame, diff,
- async task wrappers for selected SQL and table operations.

Result payloads and iterator rows use the shared canonical tabular cell codec.

### 1.5 Current bindings

Bindings wrap parts of the C ABI. Source-backed examples include C++, Python, Android/Kotlin,
React Native, Node, iOS/Swift, and WASM SQL helpers. The binding surface is not yet generated from
`idl/loom.idl`, and not every binding has identical SQL session, batch, row-stream, table reader, and
task coverage.

## 2. Current Data Model

Tables live inside a workspace's SQL facet. The current path shape used by source is:

```text
.loom/facets/sql/<db>/catalog
.loom/facets/sql/<db>/tables/<table>
```

The catalog stores GlueSQL schemas and keyless append counters. Each table path is a structured table
entry in the workspace working tree.

There is no `/.loom/db/...` addressing model. There is also no separate typed workspace for SQL. A
workspace can contain user files at `/` and SQL tables under the SQL facet at the same time.

### 2.1 Structured-table readers: addressing and module home

The structured-table readers (read-table, index-scan, blame, diff) address a table by **database and
table name** - `(workspace, db, table)` - and construct the reserved path
`.loom/facets/sql/<db>/tables/<table>` internally. Callers and wire projections never pass a pre-built
reserved path, and the database is never defaulted to a hard-coded `main`: it is an explicit parameter,
the SQL collection level (0042 RD2, the `db` collection axis). The same readers serve the columnar
facet (0023) over its own reserved table paths, which is why they are generic over the table path.

These readers belong to the `loom-core::tabular` substrate (the table type and its row/diff/blame
operations), not to the version-control module: they are moved out of `vcs.rs` into `tabular.rs`. They
remain SQL operations at the wire surface (0008 `sql.*`), and the move is purely about where the
generic table-reading code lives so columnar can reuse it without depending on VCS internals.

## 3. Row, Key, and Index Encoding

Primary keys and secondary-index keys use deterministic order-preserving encodings from
`loom-core::tabular`. Rows encode as canonical arrays of cell values. Text is UTF-8. The source-backed
default ordering is the stable binary/code-point ordering implied by the encoded value order. Locale or
Unicode-version-aware collations are not implemented.

Large SQL values currently project through the tabular value model. A separate large-cell externalization
contract is target work; do not claim that oversized SQL cells are stored as file-style chunk lists unless
source implements that projection.

## 4. Transactions, Commits, and Merge

SQL statement execution mutates a `LoomSqlStore`. `persist` stages changed SQL state into a Loom
workspace. A workspace commit records the staged table trees with any other staged workspace data.
The current public authorization split is explicit: `sql_query` is the read path, requires `Sql Read`,
rejects mutating statements, and never persists; `sql_exec` and SQL batches are the mutation-capable
paths and require `Sql Write`.

Current transaction behavior:

- In-process GlueSQL transactions snapshot the SQL store overlay.
- `COMMIT` promotes the overlay state inside the SQL store.
- `ROLLBACK` restores the overlay snapshot.
- C ABI batches hold the `.loom` writer open across statements and are the current cross-call atomic SQL
  transaction surface.
- Per-operation SQL sessions reject a dangling open transaction at the end of a call and require a batch
  for cross-call transactions.

Current merge behavior:

- Normal workspace merge auto-merges table entries at row granularity when schemas match and rows do not
  have same-row divergent edits.
- `merge_cell_level` is an explicit opt-in that can reconcile different-column edits to the same row.
- Schema divergence, file/table kind changes, unique-index collisions, and same-row conflicts remain
  unresolved path conflicts.
- Sync still follows the conflict matrix: divergent branch/ref advancement is not resolved by implicit
  table merge. Explicit merge creates a resolution commit.

## 5. Current Query Behavior

GlueSQL is the only implemented SQL engine. Its parser, planner, executor, and dialect define the
current executable SQL surface.

The current planner applies GlueSQL primary-key, index, and join planning. Indexed equality and range
predicates can route to durable tabular secondary-index trees. Missing index roots and unsupported index
forms fall back to correct row scans.

The C ABI row iterator yields selected rows one at a time, but the current SQL result is computed before
those rows are yielded. The iterator ABI is a stable projection shape, not proof of a fully lazy SQL
executor.

The current MCP host exposes the source-backed SQL tool subset `sql_exec`, `sql_query`, `sql_commit`,
`sql_read_table`, `sql_index_scan`, `sql_diff`, `sql_blame`, and `sql_list_databases`. SQL session and
batch lifecycle APIs are intentionally folded out of MCP tools; hosted long-lived transaction semantics
remain target work in 0011a.

## 6. Current Conformance

Source-backed conformance covers:

- table identity vectors,
- unique secondary-index table identity,
- rich scalar table identity,
- backend data-model vector certification,
- deterministic SQL conformance commit digest,
- canonical SQL result payload vectors,
- FFI result-view decoding tests,
- SQL session, batch, iterator, table-read, index-scan, blame, and diff tests,
- executable `sql-state-access` conformance over the private Rust execution surface.

This does not yet equal a complete public `Db` conformance suite. The binding and protocol certification
surface remains target work.

## 7. Target Contract

The target language-neutral SQL facade is tracked by 0011a. It is a public interface over a workspace's
SQL facet and should include:

- create, drop, alter, list, and describe table operations,
- exec and query operations with parameter binding,
- streamed query rows,
- query-one convenience,
- transaction begin, commit, and rollback,
- row diff,
- row merge,
- row blame,
- `as_of(rev)` historical query views.

Before promotion, 0011a needs:

- an IDL shape in `idl/loom.idl`,
- generated C ABI and binding projections or a documented non-generated exception,
- protocol methods in 0008,
- stable result payload and row-stream conformance,
- stable transaction semantics across in-process, C ABI, binding, and wire calls,
- stable SQL error mapping,
- schema-change diff records,
- `as_of` query behavior,
- parameter binding behavior,
- binding parity coverage.

## 8. Non-Goals and Limits

- Loom SQL is a versioned operational database surface, not a columnar warehouse.
- Polars or any other analytical accelerator is not implemented in current source.
- DataFusion is not used.
- Distributed query execution is not part of the core contract.
- Locale-sensitive collation is not implemented.
- Lazy per-row schema migration is not implemented.
- SQL execution failures map to stable SQL-specific `loom_core::error::Code` variants:
  `SQL_SYNTAX`, `SQL_CONSTRAINT_VIOLATION`, `SQL_TABLE_NOT_FOUND`, `SQL_TYPE_MISMATCH`, and
  `SQL_EXECUTION_FAILED`. The mapping is source-backed by `LoomSqlStore::run`; errors keep the `sql:`
  human-readable message prefix for diagnostics, but callers must branch on the stable code.

## 9. Resolved Decisions

- **RD1 - SQL facet addressing.** Tables live under the workspace SQL facet. `/.loom/db/...` is not part
  of the v1 contract.
- **RD1a - Collections: database > table (0042).** The SQL facet's collection model is `database > table`
  (0042, invariant 0001 A7), matching enterprise SQL engines (MySQL/Postgres database, Cassandra
  keyspace): the database is the collection, the table is its sub-collection, rows are units. The unit
  address and commit summary are `sql.<db>.<table>` (e.g. `sql.sales.orders`), not a bare `sql.<table>`;
  the database segment is always present (a default database name covers the single-database case) so
  there is no later flat-to-grouped migration. The database/table is the unit of ACL scope (0027) and of
  projection (a database is a connection's default schema).
- **RD2 - Row-level default merge.** Row-level table merge is the default explicit merge behavior.
  Cell-level merge is opt-in.
- **RD3 - Sync conflict boundary.** Sync does not run implicit table merges to resolve branch/ref
  divergence. Explicit merge creates a resolution commit.
- **RD4 - GlueSQL executable surface.** Current source uses GlueSQL as the executable SQL engine.
  A normative portable SQL subset remains target work.
- **RD5 - Structured table commit.** Committed SQL tables are structured table trees with separate schema,
  row-map, and index roots.
- **RD6 - Secondary indexes.** Implemented single-column identifier secondary indexes are
  content-addressed, committed, synced, and rebuilt or updated with the row map.
- **RD7 - Schema evolution.** Current source re-stages affected tables on schema or index changes.
  Lazy per-row schema migration is target work. Local DDL/catalog mutation is coordinated by the
  writable `FileStore` single-writer guard described in 0036 §8; hosted SQL DDL must add a named DDL
  lease before concurrent hosted writers are promoted.
- **RD8 - Historical query.** Current source has commit checkout and row diff between commits, but not a
  promoted `as_of` query facade.

## Change log

- 2026-06-28 (task 228, 217e): Decomposed `crates/loom-core/src/tabular.rs` with **no behavior change**
  - it went from 3,373 lines to 877. The table model (`ColumnType`/`Value`/`Schema`/`Table`/`Predicate`/
  `RowCursor`), the versioned-table facade (`put_table`/`get_table`/`list_tables`/`drop_table`), and the
  `mod merge`/`mod index` wiring stay in `tabular.rs`. Three concerns moved out: `tabular/codec.rs` (the
  value/cell/key serialization + the key-byte cursor `Cur`; `encode_cell`/`encode_cells`/`cell_value`/
  `cell_from` stay `pub` and re-export via glob, while `Cur`/`as_u8`/`as_usize`/`encode_key_value`/
  `encode_pk_values`/`encode_row`/`decode_row` are `pub(crate)` and re-exported for tabular.rs + the
  index/merge submodules + tests; `Cur`'s `new`/`u8`/`take`/`uvarint` and `pos` are `pub(crate)` for the
  index skip-scanner); `tabular/engine.rs` (the inherent `impl Loom<S>` table surface - stage/insert/
  delete, read/scan, blame/diff, list_collections - which attaches crate-wide so needs no re-export);
  and `tabular/tests.rs` (the unit tests). Verified lossless: the codec/engine/test bodies are
  byte-identical to the originals.
