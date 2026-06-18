# 0011b - SQL Foreign Wire Adapters

**Status:** Target extension. **Version:** 0.1.0. **Normative target.**

This sub-spec owns PostgreSQL-wire and MySQL-wire adapter planning for Loom SQL. It is deliberately
split out of 0011a so the core SQL facade can advance without being blocked by database-driver
compatibility.

## Current Source Boundary

Current source implements the first PostgreSQL-wire listener slices behind the `loom-hosted/pg-wire`
feature and the CLI `serve` feature. The source-backed slices use the `pgwire` crate's server API for
PostgreSQL startup, simple query, and extended query messages, validate cleartext password startup as `user =
principal UUID` plus `password = Loom principal passphrase`, store the resulting `HostedAuth` in the
per-connection session, and executes simple query plus parameterless extended query traffic through
`HostedKernel::sql().exec_cbor`.
Daemon-opened `postgres/tcp` listeners are source-backed for non-TLS owner-or-passphrase policy.
Executable client evidence uses the standard `tokio-postgres` client against a loopback pg-wire
listener to prove login, create, insert, simple select, parameterless extended select, parameterless
extended insert, bounded parameterized Bind/Execute, authentication denial, transaction-boundary
rejection, pgvector-style exact search, and columnar analytical projection. PostgreSQL SSLRequest
direct TLS is source-backed through a raw loopback transcript over the hosted TLS path. A guarded
local `psql` transcript, representing the installed libpq client path, proves create, insert, select,
`\dt`, `\d`, and `\d+` by answering the required `pg_class`, `pg_workspace`, `pg_attribute`,
row-policy, extended-statistics, publication, and inheritance catalog query shapes from Loom
`SHOW TABLES` and `SHOW COLUMNS`. JDBC, Node, Python, and BI-tool transcript entries are recorded in
the conformance matrix as guarded target rows until the corresponding local driver harnesses and
checked-in invocation profiles are source-backed.

Current source implements the MySQL-wire listener profile behind the `loom-hosted/mysql-wire` feature
and the CLI `serve` feature. It uses a dependency-free MySQL protocol implementation for server
handshake v10, per-connection random salts, the MySQL `mysql_native_password` plugin for app
credentials, the MySQL cleartext password plugin as an explicit fallback, `COM_INIT_DB`, `COM_QUERY`,
`COM_PING`, `COM_STMT_PREPARE`, `COM_STMT_EXECUTE`, `COM_STMT_CLOSE`, `COM_STMT_RESET`, text
resultsets, binary prepared-statement resultsets, OK packets, error packets, bounded `SHOW TABLES`,
`SHOW FULL TABLES`, `DESCRIBE`/`SHOW COLUMNS`, `SHOW VARIABLES`, and `information_schema.tables`/
`information_schema.columns` shims. It validates `user = principal UUID` plus the presented Loom
passphrase through the cleartext fallback, or a `loom_app_...` app credential through either
`mysql_native_password` challenge-response verification or the cleartext fallback, through
`HostedKernel`, stores the resulting `HostedAuth` per connection, executes one simple query result
through `HostedKernel::sql().exec_cbor`, maps hosted errors to MySQL error packets, rejects
transaction-boundary commands as unsupported, and daemon-opens `mysql/tcp` for non-TLS
owner-or-passphrase policy. Executable evidence uses raw loopback protocol transcripts for handshake,
passphrase auth, native-password app-credential auth, cleartext app-credential auth, create, insert,
select, metadata, ping, and prepared-statement execution, plus a guarded local MySQL 8.4 CLI
transcript for create, insert, select, table listing, describe, and information-schema metadata.
Guarded optional client harnesses are source-backed for Connector/J, Node `mysql2`, and Python
PyMySQL/mysqlclient. They execute against the real loopback MySQL-wire listener when the corresponding
local driver tooling is installed, and skip without claiming support when the tooling is unavailable.

The source-backed PostgreSQL-wire slice is deliberately dialect-honest:

- it supports simple query and parameterless extended query execution over the existing GlueSQL-backed
  Loom SQL subset;
- it maps Loom hosted errors to PostgreSQL SQLSTATE classes without replacing stable Loom `Code`
  values;
- it rejects `BEGIN`, `COMMIT`, `ROLLBACK`, and `START TRANSACTION` with unsupported behavior rather
  than pretending hosted transactions are atomic;
- PostgreSQL-wire pins transaction-boundary rejection to SQLSTATE `0A000` with the stable message
  `PostgreSQL-wire transactions require engine atomicity and are not supported by this facade` for
  `BEGIN`, `COMMIT`, `ROLLBACK`, `START TRANSACTION`, savepoint-family commands, and multi-statement
  attempts that start with a transaction boundary;
- `loom-sql` source-backs a bounded prepared-statement parameter type inference helper for schema-backed
  `INSERT ... VALUES`, `UPDATE ... SET`, column comparison predicates, and explicit PostgreSQL casts.
  Unknown parameters remain explicit so PostgreSQL-wire can advertise unknown metadata without guessing;
- PostgreSQL-wire source-backs bounded Bind/Execute parameter rewriting for text parameters, NULL
  parameters, and binary boolean, integer, float, and UTF-8 text parameters over the metadata helper.
  Statement Describe keeps parameter metadata and result-field shape aligned for the supported
  prepared-statement subset;
- it returns result columns as text in the first profile while richer PostgreSQL type metadata remains
  target work.

Current source does not implement PostgreSQL COPY, broad PostgreSQL catalog emulation beyond the
`psql` table-description profile, SQL result handles, JDBC/Node/Python driver transcripts, or BI-tool
transcript conformance. Current source does not implement MySQL `caching_sha2_password`, direct TLS,
unguarded checked-in driver dependencies for Connector/J, Node `mysql2`, or Python MySQL clients, or
a richer MySQL binary type-metadata profile beyond the bounded scalar parameter and row result
encoding used for prepared statements.
Principal-bound served Arrow IPC result handles are source-backed on the columnar REST surface, but
SQL-owned result handles and SQL driver result-handle integration remain target work.
PostgreSQL-wire now exposes the first bounded analytical SQL projection over Loom
columnar datasets with `SELECT <columns>|*|count(*) FROM columnar.<dataset> [LIMIT n]`, and returns
stable unsupported behavior for filters, joins, grouping, ordering, and having clauses until the
broader analytical SQL profile owns those semantics. PostgreSQL-wire pgvector-style exact search is
source-backed for the bounded
`SELECT id, embedding <op> '[..]' AS distance FROM <vector-set> ORDER BY embedding <op> '[..]' LIMIT n`
shape over Loom vector sets. The supported operators are `<->` for L2 sets, `<=>` for cosine sets,
and `<#>` for dot-product sets; metric mismatches return unsupported behavior.

## Adapter Contract

### Served-surface ownership

`sql` is the native Loom SQL surface and owns only Loom-native REST, JSON-RPC, and gRPC contracts.
`postgres` and `mysql` are first-class served surfaces, each with the single `tcp` transport. The
operator forms are:

```text
loom serve configure app.loom postgres <workspace> <database> --bind 127.0.0.1:5432
loom serve configure app.loom mysql <workspace> <database> --bind 127.0.0.1:3306
```

Each surface dispatches into the shared hosted SQL kernel. This preserves Loom principal resolution,
authorization, stable errors, audit behavior, and durable store writes while keeping product-specific
wire semantics out of `sql --transport ...`.

Foreign wire adapters are protocol adapters, not dialect promises:

- (P0) A PostgreSQL-wire adapter may accept PostgreSQL client protocol handshakes, but it must not
  claim PostgreSQL dialect compatibility unless the SQL parser, metadata, transactions, errors,
  prepared statements, and type system actually satisfy that driver contract.
- (P0) A MySQL-wire adapter may accept MySQL client protocol handshakes, but it must not claim MySQL
  dialect compatibility unless the same driver-facing contract is satisfied for MySQL clients.
- (P0) Stable Loom error `Code` values must remain the machine contract. Adapter-specific status codes
  are projections over the Loom code, not a replacement.
- (P0) Authentication must resolve to a Loom principal before any query, metadata lookup, or mutation.
- (P0) Authorization must be enforced by the engine PEP for every SQL operation and metadata surface.
- (P1) Transaction semantics must be honest. Multi-statement transactions are rejected unless the
  implementation can provide real atomicity and cleanup for disconnect, timeout, and crash cases.
- (P1) Metadata introspection must be explicitly scoped to the promoted Loom SQL subset. Driver
  compatibility tables, catalogs, and information-schema responses must not fabricate unsupported
  capabilities.

## Analytical Presentation Grouping

SQL foreign wire adapters sit inside the broader analytical presentation family. They must be planned
with columnar query access, dataframe SQL-result inputs, pgvector-style vector operators, and hosted
result transfer, because the same client ecosystems expect those pieces to work together.

| Integration | Target behavior | Boundary |
| --- | --- | --- |
| Columnar datasets | Source-backed now: PostgreSQL-wire can query committed columnar datasets with `SELECT <columns>|*|count(*) FROM columnar.<dataset> [LIMIT n]`. Target: richer analytical query planning and data-plane transfer. | Columnar remains the canonical owner of committed analytical dataset identity and segment policy. |
| Dataframe inputs and outputs | SQL results may feed dataframe source bindings, and dataframe materialization may write queryable columnar outputs. | Dataframe owns transformation plans and lineage; SQL does not own dataframe execution state. |
| pgvector-style access | Source-backed PostgreSQL-wire exact-search operators expose Loom vector collections. | pgvector is a SQL presentation over the vector facet, not a separate vector listener profile. |
| Hosted result transfer | Source-backed in part: prepared columnar Arrow IPC exports can be read through principal-bound, session-bound, one-shot `/_loom/results/{handle}` routes. Target: SQL-owned result handles, Flight SQL, or another profiled data-plane. | Long-lived result handles must be principal-bound, authorization-checked, expiring hosted resources. |

The first foreign SQL wire implementation should prove one shared adapter spine before widening to more
protocols. PostgreSQL-wire remains the preferred first proof point because the surrounding ecosystem
also gives the clearest path to pgvector-style compatibility and BI tooling transcripts. MySQL-wire is
a later adapter over the same spine unless a specific client requirement outranks the PostgreSQL path.

## Implementation Slices

1. (P0) Source-backed: implement PostgreSQL-wire startup, owner-or-passphrase authentication, simple
   query execution, transaction-boundary rejection, daemon listener dispatch, adapter-local tests, and
   a `tokio-postgres` simple-query transcript for login, mutation, read, auth denial, and transaction
   rejection.
2. (P0) Add broader conformance fixtures for metadata, authorization denial by ACL scope, disconnect
   cleanup, and additional real PostgreSQL clients. Source-backed now: guarded `psql` over libpq.
   Target: JDBC, Node, Python, and BI-tool driver transcript harnesses where toolchains are available.
3. (P0) Define the adapter capability report: supported protocol, dialect subset, transaction mode,
   prepared-statement mode, metadata mode, type-metadata mode, and authentication mode.
4. (P1) Source-backed: add parameterless extended query, bounded row-description behavior, explicit
   unsupported prepared-statement parameter binding, and `tokio-postgres` transcript evidence.
5. (P1) Add PostgreSQL catalog and metadata introspection only for the promoted Loom SQL subset, and
   replace describe-time row-description probing with scoped catalog/type metadata. Source-backed now:
   guarded `psql` create, insert, select, `\dt`, `\d`, and `\d+` transcript coverage.
6. (P1) Source-backed: add bounded pgvector-style exact-search SQL presentation mapping over Loom
   vector sets through PostgreSQL-wire for `<->`, `<=>`, and `<#>` with metric-match enforcement.
7. (P1) Source-backed in part: add the first bounded columnar analytical SQL profile through
   PostgreSQL-wire for projection, wildcard selection, count, and limit. Remaining target work is the
   broader DuckDB-like local analytical SQL profile, SQL-owned result handles, and Flight SQL or
   ADBC-adjacent analytical transfer profiles through the columnar/dataframe grouping recorded in
   P9-0018.
8. (P2) Source-backed: add MySQL-wire startup, cleartext passphrase authentication, native-password
   and cleartext app-credential authentication, simple one-result query execution,
   transaction-boundary rejection, prepared statement execution, daemon listener dispatch, common
   metadata shims, raw loopback protocol transcript evidence, guarded local MySQL 8.4 CLI transcript
   evidence, and guarded optional Connector/J, Node `mysql2`, and Python PyMySQL/mysqlclient transcript
   harnesses. Remaining target work is `caching_sha2_password`, direct TLS, richer binary type
   metadata, and checked-in driver profiles that make the guarded JDBC/Node/Python breadth mandatory.
9. (P2) Source-backed in part: add driver-compatibility matrix rows for `tokio-postgres`, `psql` over
   libpq, SSLRequest direct TLS, and target guarded rows for JDBC, Node, Python, and common BI tools.

## Relationship To Other Specs

- 0011 owns the source-backed SQL/tabular substrate.
- 0011a owns generated/local SQL facade parity, historical readers, schema-aware table diffs, and
  binding/runtime SQL conformance.
- 0008 owns hosted REST, JSON-RPC, gRPC, MCP, authentication, transport security, and served SQL
  projection.
- 0026 through 0028 own principal resolution and policy enforcement.
- 0035 owns durable delivery for any streaming or reconnect behavior.

## Resolved Decisions

1. **Adapters are dialect-honest.** Wire compatibility is not SQL dialect compatibility.
2. **Core SQL comes first.** Foreign wire adapters stay dialect-honest and build on the source-backed
   local SQL facade, authorization model, and conformance suite.
3. **PostgreSQL-wire precedes MySQL-wire.** The first PostgreSQL-wire slices prove one shared spine
   before widening to another protocol family.
