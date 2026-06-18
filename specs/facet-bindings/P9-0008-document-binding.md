# P9-0008 - `document` Binding

**Series:** P9 binding plan (normative-track sub-series; Draft)
**Version:** 0.1.0-draft. **Status:** Draft. **Last updated:** 2026-07-11
**Reads first:** [`P9-0002-projection-conventions.md`](./P9-0002-projection-conventions.md), facade spec
**0020 section 4** (the `Document` facade), [`IMPLEMENTATION-FIDELITY.md`](./IMPLEMENTATION-FIDELITY.md)
(document is today a keyed opaque-byte store, not a query database).

## 1. Facade surface (0020 section 4 `Document`)

`put(id: string, doc: bytes)`, `get(id: string) -> Option<bytes>`, `delete(id: string)`,
`ids() -> Stream<string>`, `find(field, value) -> Stream<string>` (ids, **by declared index only**,
not arbitrary query, 0020 section 5).

**Build status:** `put`/`get`/`delete`/`ids`, declared single-field JSON scalar indexes, indexed
`find`, and the native query JSON surface are source-backed across the native projections. Storage is
the structured canonical document root (0020 section 2): the index catalog persists canonical
`DocumentIndexDeclaration` records with a load-bearing/verified `index_catalog_root`, while the public
binding surface still accepts and returns the reduced `{name, path, unique}` index shape - full
`DocumentIndexDeclaration` projection to IDL/C ABI/bindings is target work. Collection listing
(`document.list_collections` / MCP `document_list_collections`) hides reserved implementation roots
(`.maps`/`.bodies`/`.indexes`/`.index-data`). MongoDB and Couchbase compatibility remain deferred
product presentations over that base layer.

### 1.1 Binding Boundary

The base layer is Loom's id-keyed document collection plus declared native indexes/query. Native
projections expose document CRUD, listing, indexed find, and bounded query. MongoDB-like and
Couchbase-style surfaces are deferred presentations over that base layer. JSON, JSONL, BSON, and
document dumps are interchange formats. Secondary indexes, query planner state, and adapter cursors
are derived artifacts unless their identity is explicitly pinned by the document spec. CouchDB-style
serving is cut from the active Queue 2 build plan.

## 2. Tier-1 REST

Current source has a listener-bound hosted REST facade for configured `document/rest` listeners. The
listener selectors bind `{workspace, collection}`, and the current routes cover `POST /documents:put`,
`POST /documents:get`, `POST /documents:delete`, `POST /documents:list`, index
create/drop/list/status/rebuild, indexed find, and native query with JSON bodies. Document ids are
strings and document bytes are carried as `document_hex`.

The resource-oriented target shape remains:

Facet-root `/v1/workspaces/{workspace_id}/documents`:

| Facade method | HTTP |
| --- | --- |
| `get` | `GET /documents/{id}` -> `200` + doc; `404`/`NOT_FOUND` if absent |
| `put` | `PUT /documents/{id}` (body = doc) |
| `delete` | `DELETE /documents/{id}` |
| `ids` | `GET /documents?list=1` -> NDJSON `Stream<string>` (ids) |
| `find` *(needs a declared index)* | `GET /documents:find?field=<field>&value=<value>` -> NDJSON ids; `NOT_FOUND` or a future index-specific code if the field is not indexed |

A doc read MAY carry `ETag: "{doc-content-digest}"` for conditional GET.

## 3. Tier-1 JSON-RPC

Current source has `document.get`, `document.put`, `document.delete`, `document.list`,
`document.index_create`, `document.index_drop`, `document.index_list`, `document.index_status`,
`document.index_rebuild`, `document.find`, and `document.query` over `document/json_rpc` listeners.
The target `document.ids` naming, streaming ids, generated JSON-RPC schema artifacts, and product
compatibility schema families remain target work.

## 4. Tier-1 gRPC

Current source has `loom.hosted.v1.Document` over configured `document/grpc` listeners. `Put`, `Get`,
`Delete`, `IndexCreate`, `IndexDrop`, `IndexRebuild`, `IndexList`, `IndexStatus`, and `Query` are
unary. `List` and `Find` are server-streaming with bounded response batches. `Query` currently uses
the native source-backed document query JSON contract as the request and response payload. Generated
protobuf artifacts, collection discovery, and broader hosted conformance remain target work.

## 5. Tier-1 MCP

- **Read tools (always on):** `document.get`, `document.ids`, `document.find`.
- **Write tools (token-gated, P9-0002 section 5):** `document.put`, `document.delete`.

## 6. Tier-2 foreign adapter

The document facet has multiple presentation classes. The base Loom model remains id-keyed document
storage plus declared native indexes/query.

The first major reference is **MongoDB** (wire protocol + query language), but a faithful MongoDB-wire
adapter needs a query/index engine Loom does not have (opaque docs; `find` is declared-index-only). So:

- **Source-backed native base:** declared index metadata, field-path extraction, typed scalar
  normalization, maintained secondary index storage, rebuild and readiness status, a bounded query AST,
  equality/range/boolean predicates, exclusive id cursor tokens, selected scalar projection, and native
  REST, JSON-RPC, MCP, CLI, binding, and conformance projections are source-backed.
- **Deferred / P3:** a **MongoDB** compatibility surface (so `mongosh` and MongoDB drivers attach) is
  now spec-owned work over that native base. It is not an active Queue 2 build item. The remaining work is
  BSON type and canonicalization fidelity, wire `hello` and session lifecycle, database and collection
  catalogs, authenticated command dispatch, CRUD command lowering, Mongo cursor batches, product index
  commands, update and array operators, aggregation, change streams, transactions, and protocol/client
  conformance. The implemented native query subset must not be presented as MongoDB compatibility.
- **Deferred / P3:** a **Couchbase** compatibility surface belongs primarily here because the stored unit is
  a document, but the product shape spans several facets. Direct key access maps to the `kv` boundary,
  document CRUD and indexes map to `document`, SQL++/N1QL-like query requires a document and SQL planning
  decision, and analytics maps to `columnar` only through explicitly materialized analytical datasets. The
  source-backed native document index/query base is necessary but insufficient. Service endpoints, buckets,
  scopes, collections, subdocument semantics, query, analytics, authentication, transactions, durability,
  error mapping, and client conformance require an integrated surface design before any port is promised.
- **Cut from active Queue 2:** CouchDB-style serving is not part of the current build plan. It may be
  reconsidered only if revision trees, conflicts, `_changes`, and replication semantics become a strategic
  document-replication goal.

### 6.1 Reusable Document Primitives

| Primitive | MongoDB benefit | Couchbase benefit | Native document index/query benefit | Priority |
| --- | --- | --- | --- | --- |
| Declared index catalog | Required for `createIndexes` and query planning | Required for query services | Makes `document.find` a real indexed operation | P1 |
| Field-path extraction from JSON or canonical document payloads | Required for predicate and projection fields | Required for document queries and subdocument access | Lets native queries address nested fields without scanning opaque bytes blindly | P1 |
| Typed scalar normalization | Required for stable BSON-like comparison classes | Required for stable query comparison | Gives deterministic equality/range keys across strings, numbers, bools, nulls, and time-like values | P1 |
| Maintained secondary index storage | Required for useful `find` | Required for useful query service behavior | Avoids collection scans and gives sublinear indexed lookup | P1 |
| Index rebuild and readiness status | Required for operational correctness | Required for service readiness | Lets operators and tests distinguish usable, rebuilding, failed, and stale indexes | P1 |
| Query AST | Required for a bounded MongoDB subset | Required for bounded Couchbase-style query | Gives native REST, JSON-RPC, MCP, CLI, and bindings one shared query contract | P1 |
| Equality, range, and boolean predicates | Required for useful first query subset | Required for useful first query subset | Covers practical native filters without implementing a product query language | P1 |
| Cursor/page token model | Required for driver cursors | Required for paged service results | Avoids unstable offset-only pagination over changing collections | P2 |
| Projection | Required for efficient result shaping | Required for query services | Lets native callers return selected fields without full document transfer | P2 |
| Unique, compound, sparse, partial, and multikey indexes | Useful for compatibility fidelity | Useful for compatibility fidelity | Useful after the single-field index model is source-backed | P2/P3 |
| Text, geo, aggregation, wire sessions, BSON command codec, SQL++/N1QL, and analytics services | Product-specific | Product-specific | Not native document index/query v1; use FTS, SQL, dataframe, or columnar where they own the semantics | P3 or deferred |

## 7. Errors / parity / concurrency

- **Errors:** current source uses the stable core set, with `NOT_FOUND` for absent documents. Index-specific
  errors are target work and must be added to the stable `Code` enum before they are promised.
- **Parity (0032):** portable for opaque-byte storage; GraphQL/Mongo-wire servers are native-only.
- **Concurrency:** single writer; same-id cross-peer collision policy is unresolved
  (`CONFLICT-RESOLUTION-MATRIX.md`).

## 8. Resolved Direction And Open Questions

### RD-D1 - Native document indexes/query first

- **Resolution.** Build native document indexes/query before any MongoDB or Couchbase compatibility
  implementation.
- **Source-backed slice.** The native exact-match index layer is source-backed: single-field dotted JSON
  scalar index declarations, deterministic scalar extraction, backfill, incremental put/delete
  maintenance, uniqueness checks, readiness/status, rebuild, drop cleanup, core `document.find`,
  hosted REST/JSON-RPC, MCP indexed `document_query`, and CLI index/find commands.
- **Native query slice.** The shared native query AST is source-backed in core, hosted REST/JSON-RPC,
  MCP, CLI, C ABI/IDL/header, C++, iOS, JVM, Android KMP, React Native, Node, Python, and WASM for
  equality/range comparisons, `and`/`or`, exclusive id cursors, selected scalar projections, and
  optional document bytes.
- **Conformance slice.** The executable document behavior runner now covers native exact indexed
  lookup, JSON query parsing/result formatting, cursor pagination, and projection. Core document tests
  cover index maintenance and cursor semantics.
- **Reason.** MongoDB and Couchbase both need maintained indexes, field extraction, query planning,
  cursors, and readiness semantics. Those primitives also make the native `document` facet useful, while
  product wire protocols would be misleading without them.
- **Remaining scope.** MongoDB and Couchbase move to spec-owned P3 work now that native index/query
  evidence is source-backed. CouchDB is cut from the active queue.

### OQ-D1 - How are secondary indexes declared over the wire? (resolved)

- **Context.** `find` depends on declared indexes (0020 section 5). The implemented native contract uses
  explicit index create/drop/list/status/rebuild methods.
- **Example.** A client wants `GET /documents:find?field=email&value=x`; the implemented native contract
  requires first declaring an `email` index through the explicit index management surface.
- **Options.** (a) add an explicit `create_index(field, opts)` / `drop_index` to the 0020 facade and a
  `PUT /documents/_indexes/{field}` route; (b) derive indexes from a workspace-level mapping fixed at
  creation (like `search` 0033 mappings); (c) push document-field querying onto the `sql` facet (0011) via
  projection and don't index in `document` at all (0020 section 6 hints at this).
- **Resolution.** (a) explicit index management. This keeps `document` self-contained and makes `find`
  real, while richer relational queries can still project through SQL or another query layer.

### OQ-D2 - MongoDB compatibility: spec-owned before any wire implementation (resolved)

- **Context.** The prerequisite native index/query substrate is now source-backed, but it is deliberately
  smaller than the MongoDB product contract. A MongoDB port would add BSON fidelity, command envelopes,
  sessions, product cursors, update operators, collection and database administration, aggregation, and
  client conformance that the native document API neither needs nor currently promises.
- **Example.** Native `document.query` can execute a bounded typed range predicate over a declared index.
  A MongoDB client expects `find`, `getMore`, `update`, index administration, BSON values, wire `hello`,
  cursor batches, stable command errors, and documented feature negotiation to work together.
- **Options.** (a) make MongoDB spec-owned P3 work with a separately reviewed compatibility matrix before
  any listener is enabled; (b) ship a partial MongoDB port over the native query subset; (c) remove MongoDB
  compatibility from the roadmap permanently.
- **Resolution.** (a). The native prerequisite is complete enough to design against, not enough to claim a
  MongoDB port. MongoDB remains a planned first-class `mongodb` surface with its own product semantics,
  capability matrix, and conformance work. No partial listener or driver-facing compatibility claim is
  permitted before that design is approved.

### 8.1 MongoDB P3 Compatibility Scope

| Product primitive | Native document contribution | MongoDB-specific work still required |
| --- | --- | --- |
| Documents and collections | Id-keyed document collections and bounded JSON query | BSON encoding, BSON type fidelity, database and collection catalog behavior |
| Query predicates and projection | Typed scalar equality/range/boolean predicates, projection, and cursor tokens | Mongo filter grammar, `$` operators, sort, collation, array semantics, cursor batches, and `getMore` |
| Secondary indexes | Declared single-field scalar indexes, rebuild, readiness, and uniqueness | `createIndexes`, compound, sparse, partial, multikey, text, geo, index metadata, and planner semantics |
| Mutation | Atomic document put/delete at the native boundary | Insert/update/delete command grammar, update modifiers, replacement rules, retryable writes, and write concern behavior |
| Sessions and security | Hosted kernel principal authentication and PEP | Mongo wire handshake, SASL/auth profile mapping, session and transaction behavior, command error envelopes |
| Change and analytical features | No native Mongo product equivalent | Aggregation pipeline, change streams, replication semantics, sharding, and administrative command scope |

The earliest MongoDB implementation task must begin with a client-facing compatibility matrix that names
the selected MongoDB version, wire and authentication profile, command families, BSON constraints, error
mapping, unsupported behavior, and transcript conformance clients. It must reuse native document indexes
and query primitives through a dedicated adapter, not duplicate them in a MongoDB-only store.
