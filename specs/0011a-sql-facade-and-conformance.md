# 0011a - SQL Facade and Conformance

**Status:** Draft target extension. **Version:** 0.1.0. **Normative target.**

This document owns SQL work that is beyond the current source-backed 0011 boundary but still belongs
to the local/generated SQL facade. 0011 is complete for the implemented tabular substrate, GlueSQL
frontend, CLI, C ABI, selected bindings, current result payloads, direct table/history readers, and the
current curated MCP SQL tools. Hosted SQL protocols are owned by 0008. PostgreSQL-wire and MySQL-wire
foreign adapters are owned by 0011b.

## Current source boundary

Current source implements structured table trees, GlueSQL sessions and batches, SQL result payloads,
row iterators, direct table readers, historical direct readers, index scans, table blame, row-level
table diff, schema-aware table diff records, alter-table through GlueSQL, batch commit, selected
binding projections, and the authorization split where `sql_query` is read-only and
`sql_exec`/batches are mutation-capable. The MCP host exposes the folded current SQL tool subset:
`sql_exec`, `sql_query`, `sql_commit`, `sql_read_table`, `sql_index_scan`, `sql_diff`, `sql_blame`, and
`sql_list_databases`. It deliberately excludes SQL session/batch lifecycle tools, historical reader
tools, schema-aware table diff tools, and async forms.

Current source also exposes feature-backed Rust compute access through `StateAccess` when
`loom-compute` is built with `sql-state-access`. `sql_query_cbor` authorizes `Sql` read access for the
database target, returns canonical SQL result CBOR, and rejects statements that dirty the store.
`sql_exec_cbor` authorizes `Sql` write access for the database target, runs the statement batch, and
persists changed SQL state. `crates/loom-conformance` executes this behavior as `sql-state-access`.
The same feature backs the guest execution host ABI: `sql_query` and `sql_exec` are reachable through
public `exec_cbor` surfaces in the CLI, hosted adapters, C ABI backed bindings, Node, Python, and WASM.

Current row iterators, async tasks, and result views are local binding helpers, not hosted served
handles. `loom_sql_query` opens an in-process `LoomIter` after the SQL read authorization check and
then yields already-produced row bytes. `LoomTask` defers a local operation until poll and returns one
owned result buffer. `LoomResultView` decodes a canonical result buffer that the caller already holds.
None of these handles may be exposed by hosted SQL protocols as a long-lived remote capability without
the served-handle authorization contract in 0008.

Current SQL error mapping is stable and source-backed. `LoomSqlStore::run` maps GlueSQL failures to
SQL-specific `Code` variants: `SQL_SYNTAX`, `SQL_CONSTRAINT_VIOLATION`, `SQL_TABLE_NOT_FOUND`,
`SQL_TYPE_MISMATCH`, and `SQL_EXECUTION_FAILED`. The C ABI propagates the code and `sql:` diagnostic
message through `loom_last_error`, and bindings preserve the `Code` verbatim. The conformance crate
executes the `sql-errors` behavioral suite for these five categories.

Current source does not provide:

- (P1) a generated language-neutral SQL facade across every binding;
- (P1) hosted REST, JSON-RPC, gRPC, and full MCP session/batch SQL projection, owned by 0008;
- (P1) arbitrary SQL query-at-revision syntax or API beyond the source-backed historical direct readers;
- (P0) conformance for cross-binding SQL transactions, row streaming, and hosted protocols;
- (P0) served SQL result-handle authorization and revocation behavior for any hosted streaming or
  resumable result surface, owned by 0008 and 0027/0028;
- (P2) a portable normative SQL subset independent of GlueSQL behavior;
- (P2) analytical accelerator projection.

## Target facade track

- (P0) Define one workspace selector shape for SQL across IDL, C ABI, bindings, and hosted protocols.
- (P0) Pin canonical result payloads and row-stream ownership semantics before widening generated
  bindings.
- (P0) Preserve the stable SQL error-code taxonomy across every generated binding and hosted protocol.
- (P1) Promote table list, describe, create, drop, alter, exec, query, query-one, transaction, diff,
  merge, blame, and historical reader operations.
- (P1) Add binding parity for every promoted operation.
- (P1) Add conformance for sessions, batches, row iterators, direct readers, result views, and binding
  parity.

## Stable SQL error taxonomy

The source-backed taxonomy is a Loom-owned classifier over GlueSQL 0.19 typed errors, never a
message-string parser:

- `SQL_SYNTAX`: top-level parser failures.
- `SQL_TABLE_NOT_FOUND`: typed missing-table variants from fetch, insert, execute, alter-table, index,
  and alter paths.
- `SQL_CONSTRAINT_VIOLATION`: duplicate primary/unique keys, missing referenced foreign-key values,
  not-null values, and referential drop/delete blockers.
- `SQL_TYPE_MISMATCH`: typed value/evaluation failures where the value does not fit the requested SQL
  type.
- `SQL_EXECUTION_FAILED`: any SQL failure outside the narrower categories.

The mapping is intentionally conservative. When GlueSQL exposes a typed error that is not confidently
part of a narrower category, Loom returns `SQL_EXECUTION_FAILED` rather than guessing from prose.

Remaining work:

- (P0) Add cross-binding runtime fixtures that assert the same SQL failure maps to the same code in every
  promoted binding.
- (P1) Re-audit the classifier whenever the pinned GlueSQL version changes.

## Target hosted-projection track

- (P1) Define REST, JSON-RPC, gRPC, and full MCP SQL methods in 0008 after the IDL facade is stable.
- (P0) Reject multi-statement hosted transactions unless the implementation can provide real atomicity.
- (P0) Bind every hosted SQL result handle to the authenticated principal, workspace, database, table or
  query resource scope, creation operation, and expiry, then re-check authorization before each
  streamed chunk, poll, cancellation, or result retrieval.
- (P1) Define transaction lifetime, cancellation, reconnect, and cleanup behavior.
- (P2) Add conformance for hosted row streaming, transaction failure, authorization, and protocol error
  mapping.

Foreign SQL wire adapters are split to 0011b and stay target-only.

## Target historical-query track

- Source-backed: historical direct readers read tables and index scans from a commit without changing
  the working tree.
- Source-backed: schema-aware table diff records report schema changes separately from row changes.
- (P1) Define arbitrary SQL query-at-revision syntax or API over committed SQL state.
- (P1) Add conformance for historical direct readers across create, alter, drop, insert, update,
  delete, and merge.

## Sequencing

1. (P0) Keep 0011 source-backed and do not claim full public SQL certification before 0011a is
   implemented.
2. (P0) Pin result payload, row-stream, and error behavior before generated binding work.
3. (P1) Promote IDL/C ABI/binding parity for the language-neutral SQL facade.
4. (P1) Add hosted projections only after 0008 auth and 0026-0028 authorization are ready.
5. (P2) Add foreign wire adapters only through 0011b after the core SQL facade and conformance suite
   pass.

## Resolved decisions

1. **Public wire name is `sql`.** `Db` may remain an internal or IDL alias, but REST, JSON-RPC, MCP,
   and capability names use `sql`.
2. **Dialect honesty lives in 0011b.** A PostgreSQL-wire or MySQL-wire adapter is wire-compatible with
   that protocol, not a claim that Loom accepts the full PostgreSQL or MySQL dialect.
3. **No fake transactions.** Hosted `BEGIN`/`COMMIT` must be rejected until the implementation can
   provide atomicity.

## Active SQL cross-binding certification owner gate

Completion state: active implementation owner. Current SQL substrate behavior, selected binding
fixtures, direct table and history readers, stable SQL error mapping, and selected MCP SQL tools are
source-backed. Uniform cross-binding runtime certification remains implementation work.

Decision Points: none.

| Gate | Source-backed evidence | Remaining implementation work | Disposition |
| --- | --- | --- | --- |
| Cross-binding SQL runtime fixtures | Node, Python, iOS, C++, JVM, Android, React Native, and WASM have uneven SQL fixture coverage through their current runtime suites. | Normalize promoted SQL fixture coverage by binding family for transactions, row streaming, direct history readers, schema-aware table diffs, SQL error parity, workflow execution, and unsupported or degraded states. | Target P0. |
| Row-stream and iterator ownership | C ABI row iterators and binding helpers are local handles over already-produced row bytes. | Certify ownership, finalization, read-only rejection, equality, and error behavior per promoted binding family without treating local iterators as hosted served handles. | Target P0. |
| SQL error parity | The current GlueSQL error classifier maps failures to stable Loom SQL `Code` variants. | Add cross-binding runtime fixtures that assert the same SQL failure maps to the same code in every promoted binding. | Target P0. |
| Hosted SQL boundary | Hosted REST, JSON-RPC, gRPC, full MCP SQL projection, served SQL result handles, and hosted row streaming are target work owned by 0008 plus security specs. | Keep hosted session, batch, stream, and handle behavior out of local cross-binding certification except as explicit dependencies and reporting states. | Target outside this local certification gate. |
