# P9-0006 - `sql` Binding

**Series:** P9 binding plan (normative-track sub-series; Draft)
**Version:** 0.1.0-draft - **Status:** Draft - **Last updated:** 2026-06-18
**Reads first:** [`P9-0002-projection-conventions.md`](./P9-0002-projection-conventions.md), facade spec
**0011 section 3** (the `Db` facade), **0013 section B** (pg-wire/MySQL adapters), **0008 section 2** (the `db` facade "projects
the same way"), ADR-0008 (engine posture), [`IMPLEMENTATION-FIDELITY.md`](./IMPLEMENTATION-FIDELITY.md)
(the SQL fidelity finding).

This is the richest Tier-2 case: a **pg-wire** server lets `psql`/JDBC connect to a `.loom` unmodified.
It is also the one facet with a **name split** (P9-0001 notes the convention): the facet/workspace/
capability and wire surface are `sql`; `Db` remains an in-engine or IDL facade alias when needed.

## 0. Binding Boundary

The base layer is Loom's versioned tabular and SQL state. Native projections expose SQL sessions,
queries, table metadata, diffs, and merge behavior through Loom protocols and MCP. pg-wire and MySQL-wire
are presentations over that base layer, not the storage model. SQL dumps, SQLite files, and spreadsheet
exports are interchange. Query plans, indexes, result views, and OLAP accelerators are derived artifacts
unless a spec explicitly promotes them to canonical data.

## 1. Facade surface (0011 section 3 `Db`)

DDL: `create_table(ddl)`, `drop_table(name)`, `alter_table(ddl)`, `list_tables()`, `describe(table)`.
DML/query: `exec(sql, params) -> ExecResult`, `query(sql, params) -> Stream<Row>`, `query_one(...)`.
Transactions: `begin() -> DbTxn` (commit/rollback; maps to 0003 section 6 Batch). Version-control over data:
`diff_rows(table, from, to) -> Stream<RowChange>`, `merge_table(table, source, opts) -> MergeResult`,
`blame(table, pk)`, `as_of(rev) -> Db` (time-travel).

**Build status (loom-sql / GlueSQL 0.19 over the tabular substrate):** the source-backed boundary
includes GlueSQL DDL/DML/query execution, `ALTER TABLE` through GlueSQL with full table re-stage, C ABI
sessions, C ABI batches, direct table read, index scan, table blame, table diff, result views, selected
binding projections, hosted `sql/rest` plus `sql/json_rpc` listener verticals for `sql.query` and
`sql.exec`, and the first feature-gated PostgreSQL-wire simple-query listener slice. The `postgres/tcp`
slice uses PostgreSQL startup plus simple query traffic through `pgwire`, maps PostgreSQL user to a
Loom principal UUID, validates the presented password as that principal's Loom passphrase, executes
through the hosted SQL kernel, rejects transaction-boundary commands as unsupported, and has an
executable `tokio-postgres` transcript for simple query, parameterless extended query, auth denial,
transaction-boundary rejection, and parameter-binding rejection. A guarded local `psql` transcript now
proves create, insert, select, `\dt`, `\d`, and `\d+` through the first PostgreSQL catalog shims. The
promoted generated SQL facade, hosted SQL gRPC, PostgreSQL prepared-statement parameter binding,
MySQL native-password auth, MySQL prepared statement execution, full `as_of`, schema change diff
records, portable SQL subset, SQL-specific stable errors, and broader foreign wire adapters remain
target work owned by 0011a and 0011b.
PostgreSQL-wire pgvector-style exact search is source-backed for bounded `SELECT id, embedding <op>
'[..]' AS distance FROM <vector-set> ORDER BY embedding <op> '[..]' LIMIT n` queries over Loom vector
sets, with `<->`, `<=>`, and `<#>` gated by the vector set metric. PostgreSQL-wire also exposes the
first bounded analytical SQL projection over Loom columnar datasets with
`SELECT <columns>|*|count(*) FROM columnar.<dataset> [LIMIT n]`, with filters, joins, grouping,
ordering, and having clauses rejected as explicitly unsupported until the broader analytical profile is
promoted. MySQL-wire is source-backed through a dependency-free protocol profile for cleartext
passphrase authentication, app-credential authentication, `COM_INIT_DB`, `COM_QUERY`, `COM_PING`,
one-result simple query execution through the hosted SQL kernel, text resultsets, bounded `SHOW` and
information-schema metadata shims, transaction-boundary rejection, prepared-statement unsupported
errors, daemon listener dispatch, raw loopback protocol transcript evidence, and guarded local MySQL
8.4 CLI transcript evidence.

## 2. Tier-1 - REST

Facet-root `/v1/workspaces/{workspace_id}/sql`:

| Facade method | HTTP |
| --- | --- |
| `list_tables` / `describe` | `GET /sql/tables`; `GET /sql/tables/{t}` |
| `create_table` / `drop_table` / `alter_table` | `POST /sql/tables {ddl}`; `DELETE /sql/tables/{t}`; `POST /sql/tables/{t}:alter {ddl}` |
| `query` (SELECT) | Source-backed now as `POST /sql:query {sql}` on a listener bound to workspace and database, returning Loom Canonical CBOR. Target shape streams rows as NDJSON or a negotiated row stream. |
| `exec` (INSERT/UPDATE/DELETE/DDL) | Source-backed now as `POST /sql:exec {sql}` on a listener bound to workspace and database, returning Loom Canonical CBOR. Target shape returns structured `ExecResult`. |
| `begin` / txn | `POST /sql:begin` -> `{txn}`; subsequent calls carry the txn id; `:commit`/`:rollback` |
| `diff_rows` | `GET /sql/tables/{t}/diff?from=...&to=...` -> NDJSON `Stream<RowChange>` |
| `merge_table` | `POST /sql/tables/{t}:merge {source, opts}` -> `200`/`409` |
| `blame` / `as_of` | `GET /sql/tables/{t}/blame?pk=...`; any read with `?rev=...` |

Only `?rev`-pinned reads are immutable enough for a strong `ETag` (a live `query` is not cacheable);
otherwise no caching deltas from P9-0002 section 2.

## 3. Tier-1 - JSON-RPC

Source-backed now: `sql.exec` and `sql.query` on a listener bound to workspace and database, returning
Loom Canonical CBOR as `cbor_hex`. Target 1:1 surface: `sql.createTable`, `sql.exec`, `sql.query`
(streamed), `sql.queryOne`, `sql.begin`, `sql.diffRows` (streamed), `sql.mergeTable`, `sql.blame`,
`sql.asOf` (P9-0002 section 3).

## 4. Tier-1 - gRPC

`Query`, `DiffRows`, `Blame` **server-streaming**; `Exec`, `CreateTable`, `DropTable`, `AlterTable`,
`QueryOne`, `MergeTable` unary; transactions as a short **bidi** session (begin, statements, commit/
rollback) so `DbTxn` lifetime maps to a stream (0008 section 4.2).

## 5. Tier-1 - MCP

- **Read tools (always on):** `sql.query` (**SELECT-only**; the server MUST reject non-SELECT here),
  `sql.describe`, `sql.listTables`, `sql.diffRows`, `sql.blame`.
- **Write tools (token-gated, P9-0002 section 5):** `sql.exec` (DML/DDL), `sql.createTable`, `sql.dropTable`,
  `sql.alterTable`, `sql.mergeTable`, `sql.begin`. A token grants write on the `sql` workspace.

## 6. Tier-2 - foreign adapter - pg-wire / MySQL-wire

The headline "it just works" adapter (0013 section B): stand up a **PostgreSQL wire** server (`pgwire`, default)
and/or a **MySQL wire** server over the `Db` facade, so `psql`, JDBC/ODBC, and any Postgres/MySQL
client can connect to a `.loom` after the advertised subset is proven against those clients.

- **Concept mapping.** Wire `Query`/`Parse`+`Bind`+`Execute` maps to `query`/`exec` with `params`; result
  rows map to `Stream<Row>`; `BEGIN`/`COMMIT` maps to `begin`/commit (see OQ-S3); auth handshake maps to
  capability token (P9-0002 section 6). Default ports: **5432** (pg) / **3306** (MySQL).
- **Fidelity ceiling (ties to the fidelity doc).** Only the **GlueSQL dialect/subset** is parseable, not
  full PostgreSQL or MySQL; `pg_catalog`/`information_schema` introspection is partial; hosted
  transaction semantics are target work in 0011a; no server-side prepared-statement plan cache beyond
  GlueSQL; the advertised dialect won't match a real PG/MySQL in edge cases (OQ-S2). Capability-gated;
  native-only server.
- **Current PostgreSQL-wire slice.** Source now backs startup, passphrase authentication, simple query,
  parameterless extended query, text-shaped result rows, command completion tags, bounded row
  descriptions for row-returning extended statements, transaction-boundary rejection, PostgreSQL `$1`
  parameter-binding rejection, and daemon listener dispatch for `postgres/tcp`. A standard
  `tokio-postgres` client transcript proves login, create, insert, simple select, parameterless
  extended select, parameterless extended insert, authentication denial, transaction rejection, and
  parameter-binding rejection through the actual socket listener. A guarded local `psql` transcript
  proves create, insert, select, `\dt`, `\d`, and `\d+` through catalog responses backed by Loom
  `SHOW TABLES` and `SHOW COLUMNS`. Prepared-statement parameter binding, COPY, broader catalog
  metadata, binary result typing, direct TLS, MySQL native-password auth, MySQL prepared statement
  execution, and broader `psql`/libpq/JDBC/Node/Python/MySQL transcript conformance remain target
  work. The `mysql/tcp` MySQL-wire profile is source-backed for cleartext passphrase auth, app-credential auth,
  `COM_INIT_DB`, `COM_QUERY`, `COM_PING`, text resultsets, bounded metadata shims, simple one-result
  query execution, stable prepared-command rejection, raw loopback protocol evidence, and guarded
  local MySQL 8.4 CLI transcript evidence.

### 6.1 Analytical presentation grouping

The foreign SQL adapter should not be implemented as a standalone listener that only proves a simple
query transcript. It is the entry point for a wider analytical client ecosystem:

| Adjacent surface | SQL-facing role | Required boundary |
| --- | --- | --- |
| `columnar` | Queryable analytical datasets and Arrow/Parquet-oriented result transfer. | Columnar remains the dataset identity owner. SQL sees a presentation or query view. |
| `dataframe` | SQL result sets can become dataframe sources, and dataframe materialization can produce SQL-queryable columnar datasets. | Dataframe owns transformation plans, lineage, and execution profile. |
| `vector` | pgvector-like operators may expose Loom vector collections through SQL clients. | Vector search identity, metrics, dimensions, and metadata filters stay owned by the vector facet. |
| hosted results | Source-backed in part: prepared columnar Arrow IPC exports can be read through principal-bound, session-bound, one-shot `/_loom/results/{handle}` routes. Target: SQL-owned result handles, driver-facing row streams, Flight SQL, or other profiled analytical batches. | Hosted handles must be principal-bound, authorization-checked, revocable or expiring resources. |

PostgreSQL-wire and MySQL-wire are first-class SQL compatibility surfaces. PostgreSQL-wire remains the
preferred first protocol for deeper analytical expansion unless a concrete client target requires
MySQL-wire first. DuckDB-like local analytical SQL is a separate presentation over columnar/dataframe
behavior; Loom should not embed DuckDB or expose `duckdb` as the public surface name for that path.
The first pgvector-style SQL presentation is source-backed as a bounded exact-search query shape over
Loom vector sets, not as a `vector` listener profile. The first columnar SQL presentation is
source-backed as a bounded `columnar.<dataset>` query shape over Loom columnar datasets, not as an
embedded DuckDB engine or public `duckdb` surface.

## 7. Errors / parity / concurrency

- **Errors:** reuse 0008 section 6; SQL parse/constraint failures map to `INVALID_ARGUMENT` (`400`/`-32602`),
  constraint/PK conflicts to `CONFLICT`/`ALREADY_EXISTS`, missing table to `NOT_FOUND`, unsupported
  statement (e.g. `UPDATE`/`ALTER` pre-wiring) to `UNSUPPORTED` (`501`).
- **Parity (0032):** GlueSQL is pure-Rust so `query`/`exec` are portable (incl. `wasm32`); the Polars OLAP
  fast-path is native-gated (ADR-0008); the `postgres/tcp` and `mysql/tcp` servers are native-only.
- **Concurrency:** single serialized writer (P9-0002 section 10); hosted transaction semantics are target work
  in 0011a.

## 8. Resolved Decisions

### P9-RD-S1 - Wire name

- **Decision.** Use `sql` for REST, JSON-RPC, MCP, capability names, and generated wire projection.
  Retain `Db` only as an in-engine or IDL facade alias where useful.

## 9. Open Questions

### OQ-S2 - Which SQL dialect does the pg-wire adapter advertise?

- **Context.** A `pgwire` client assumes PostgreSQL semantics, but the engine is GlueSQL (a different
  subset/dialect). Advertising "PostgreSQL 16" while parsing GlueSQL will surprise clients on unsupported
  syntax.
- **Example.** A JDBC app sends a PG-specific `INSERT ... ON CONFLICT` or `::` cast; GlueSQL rejects it,
  but the client believed it was talking to Postgres.
- **Options.** (a) advertise a low PG version and document "GlueSQL subset"; (b) translate/shim common PG
  idioms into GlueSQL where feasible; (c) only claim wire compatibility, not dialect compatibility, and
  return `UNSUPPORTED` with a clear message.
- **Recommendation.** (c) wire-compatible, dialect-honest - accept the PG/MySQL wire protocol but document
  that the SQL dialect is GlueSQL's subset and return `UNSUPPORTED` on unparseable syntax; revisit (b)
  shims only for high-frequency idioms once usage data exists.

### OQ-S3 - Transactions over hosted wire protocols

- **Context.** C ABI batches exist for current source-backed cross-call SQL work, but hosted SQL
  transaction lifetime, reconnect, cancellation, and cleanup are target work in 0011a. Wire clients send
  `BEGIN`/`COMMIT`.
- **Example.** `psql` opens `BEGIN; INSERT...; INSERT...; COMMIT;` expecting atomicity; today each statement
  autocommits, so a mid-transaction failure leaves partial state.
- **Options.** (a) reject `BEGIN` with `UNSUPPORTED` until real transactions land; (b) accept `BEGIN`/
  `COMMIT` as no-ops (single-statement autocommit), risking silent non-atomicity; (c) implement
  transactions by running the statement batch on a throwaway **branch** and merging on `COMMIT` (reusing
  the run-on-a-branch gate, 0015/P4).
- **Recommendation.** (a) now, (c) later - reject hosted multi-statement transactions with a clear
  `UNSUPPORTED` until they are real, then implement (c), which fits Loom's branch/merge substrate
  naturally; never (b), which violates the atomicity clients assume.
