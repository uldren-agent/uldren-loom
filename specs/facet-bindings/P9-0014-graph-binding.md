# P9-0014 - `graph` Binding

**Series:** P9 binding plan (normative-track sub-series; Draft)
**Version:** 0.1.0-draft. **Status:** Draft, source-backed local facade and hosted native subset.
**Last updated:** 2026-07-02
**Reads first:** [`P9-0002-projection-conventions.md`](./P9-0002-projection-conventions.md), facade spec
**0016** (Graph; property graph; engine-specific query), fidelity doc.

## 1. Facade surface (0016 section 4 `Graph`)

The source-backed local facade stores one canonical property graph per name. Nodes are keyed by id.
Edges are keyed by id and carry `src`, `dst`, `label`, and properties. Source-backed operations are:
`upsert_node`, `get_node`, `remove_node`, `upsert_edge`, `get_edge`, `remove_edge`, `neighbors`,
`out_edges`, `in_edges`, `reachable`, and `shortest_path`. Core graph storage and native graph query
IR now also support canonical multi-label nodes, scan-based node and directed-edge pattern matching,
label and property predicates, explicit projections, grouped count aggregation, deterministic
ordering, skip, row limits, typed node/edge/scalar result values, and a bounded openCypher/GQL read
parser for `MATCH`, label/property patterns, comparison predicates with `AND`/`OR`/`NOT`, `RETURN`,
`ORDER BY`, `SKIP`, grouped `count`, `LIMIT`, deterministic regex predicates, fixed chained paths, bounded variable-length
all-simple paths, bounded `shortestPath`, path values, `length(path)`, `id`, `type`, `startNode`,
and `endNode`. Local CLI, generated remote protocol and CLI remote facade calls, hosted native REST,
hosted native JSON-RPC, MCP, Node, Python, and WASM now project native graph query and explain-query
over the shared source-backed subset, using shared canonical CBOR result encoders for binary
projections. A native mutation plan is source-backed for create
node/edge, merge node/edge, set/remove node/edge properties, delete edge, delete node, and detach
delete node through the core facade and hosted action-shaped REST/JSON-RPC projection. Bounded
openCypher/GQL mutation text lowering is source-backed for `CREATE`, `MERGE`, `SET`, `REMOVE`,
`DELETE`, and `DETACH DELETE` through an explicit Loom identity envelope.

Full GQL, full openCypher, Neo4j/Bolt compatibility, and engine-specific text query execution are
presentation targets over the base graph layer. Gremlin is cut from the active roadmap.

### 1.1 Binding Boundary

The base layer is the Loom property graph: node ids, edge ids, node labels, edge labels, typed properties, and
deterministic traversal. Native projections expose node and edge CRUD plus neighbors, in/out edges,
reachable, and shortest path. The approved storage target is an existing `Tree` root with component
`ProllyMap` entries for node map, edge map, forward adjacency, reverse adjacency, and derived
property-index materializations. Node and edge keys are current string ids. Adjacency keys are
length-prefixed compound bytes. The property index catalog is canonical; physical index
materializations and visualization layouts are derived artifacts. Neo4j/Bolt, openCypher/GQL, and
graph file import/export are presentations or interchange over that native substrate. Gremlin is not
active scope.

## 2. Tier-1 REST

Current source-backed hosted REST is the native action-shaped subset used by the served listener:
`/graph:upsert-node`, `/graph:get-node`, `/graph:upsert-edge`, `/graph:neighbors`,
`/graph:reachable`, `/graph:query`, `/graph:explain-query`, and `/capabilities`. The resource-shaped
routes below remain the target REST profile.

Facet-root `/v1/workspaces/{workspace_id}/graph`:

| Facade method | HTTP |
| --- | --- |
| nodes | `PUT /graph/{name}/nodes/{id}`; `GET /graph/{name}/nodes/{id}`; `DELETE /graph/{name}/nodes/{id}?cascade=1` |
| edges | `PUT /graph/{name}/edges/{id} {src,dst,label,props}`; `GET /graph/{name}/edges/{id}`; `DELETE /graph/{name}/edges/{id}` |
| traversal | `GET /graph/{name}/nodes/{id}/out`; `/in`; `/neighbors`; `/reachable`; `/shortest-path` |

## 3-4. JSON-RPC / gRPC

Current hosted JSON-RPC is source-backed for `graph.upsert_node`, `graph.get_node`,
`graph.upsert_edge`, `graph.neighbors`, `graph.query`, `graph.explain_query`,
`graph.explainQuery`, and `graph.capabilities`. Full JSON-RPC coverage remains target.

Native hosted gRPC is source-backed for:

- unary `UpsertNode`, `GetNode`, and `UpsertEdge` using canonical CBOR property bags;
- server-streaming `Neighbors` and `Reachable`;
- unary `Query` and `ExplainQuery` returning shared canonical CBOR result and explain payloads;
- unary `Capabilities` for the native graph profile.

Generated protobuf artifacts, full graph CRUD parity, cursoring, mutation-plan projection, hosted
conformance, and graph-specific merge remain target work.

## 5. Tier-1 MCP

- **Read tools:** `graph.get_node`, `graph.get_edge`, `graph.neighbors`, `graph.out_edges`,
  `graph.in_edges`, `graph.reachable`, `graph.shortest_path`, `graph_query`,
  `graph_explain_query`.
- **Write tools (token-gated):** `graph.upsert_node`, `graph.remove_node`, `graph.upsert_edge`,
  `graph.remove_edge`.

## 6. Tier-2 foreign adapter - openCypher / GQL / Neo4j

The portable traversal API (neighbors/out/in) maps cleanly to a foreign graph endpoint, but the
declarative query surface is only partially source-backed. A full GQL/openCypher profile or
Neo4j-compatible served surface is gated on adopting the native graph query IR, profile grammar,
result model, conformance, and resource limits. Fidelity ceiling: without that standard grammar and
Neo4j session contract, a Neo4j client can use only a Loom-native traversal projection, not the
expected Neo4j product surface. Decide via OQ-GR1 and OQ-GR3. Gremlin is cut from active scope.

## 7. Errors / parity / concurrency

- **Errors:** current source uses the stable core set. Graph-specific codes remain target work.
- **Parity (0032):** traversal API is portable; a query grammar engine and Tier-2 server are
  native-dependent target work.
- **Concurrency:** single writer; node/edge-level 3-way merge (0016).

## 8. Open Questions

### OQ-GR1 - Standard graph query grammar and compatibility order (resolved)

- **Resolution.** First promote structured canonical graph storage and a shared native graph query and
  traversal IR. Then implement a bounded GQL-aligned openCypher contract over that substrate. Neo4j is
  a first-class target served surface, not a generic `graph` transport, because official Neo4j
  drivers and APIs create product-level expectations around Bolt sessions, result records,
  transactions, errors, catalog/procedure behavior, and client compatibility. Gremlin is cut from the
  active graph roadmap and requires a separate owner-approved design session to reopen.
- **Boundary.** The initial GQL/openCypher subset belongs to the `graph` semantic surface. It is not a
  full Neo4j claim. `loom serve configure <store> neo4j <workspace> <graph> --bind
  127.0.0.1:7687` is the source-backed durable listener-admission shape for Neo4j-compatible
  clients. Daemon runtime opening and the bounded Bolt 5.1 session skeleton are source-backed.
- **Reason.** The structured graph root gives Loom a versioned substrate, but it does not by itself
  promise the query planning, property indexing, mutation, and merge behavior expected by foreign graph
  clients. A shared native IR lets GQL/openCypher and Neo4j compatibility consume canonical storage,
  authorization, resource budgets, and stable errors without making a protocol handshake define graph
  identity.

### OQ-GR2 - Structured graph storage root (resolved)

- **Resolution.** Use an existing `Tree` root with component `ProllyMap` entries, typed canonical
  property values, canonical property-index catalog declarations, derived index materializations, and
  node/edge-level three-way merge.
- **Boundary.** This is a storage promotion, not a Neo4j, Bolt, or graph-engine compatibility claim.
  Query and protocol surfaces remain gated by the approved 0016 sequence. Gremlin is not active scope.
- **Reason.** Existing `Tree` and `ProllyMap` reachability, sync, diff, and structural-sharing
  mechanics give Loom the durable versioned graph substrate before foreign query semantics are exposed.

### OQ-GR3 - Native query IR and openCypher/GQL scope (resolved)

- **Resolution.** The native query contract is a versioned Loom graph query IR. The first text profile
  is a bounded GQL-aligned openCypher profile over that IR. The approved roadmap includes canonical
  multi-label nodes, read patterns, grouped aggregation, `CREATE`, `MERGE`, `SET`, `REMOVE`,
  `DELETE`, `DETACH DELETE`, fixed paths, bounded variable paths, shortest path, all-simple-path
  behavior, regex predicates, list/map values, scalar and path functions, FTS-backed full-text
  integration, geospatial posture, property-index materialization, explain/readiness behavior, stable
  errors, auth, and hard resource limits. Core property-index declarations, transient
  materialization, readiness/stale reporting, ready node-property equality selection, explain output,
  and graph predicates over FTS hit-id projections are source-backed; public projection and text
  grammar remain separate.
- **Boundary.** The openCypher profile returns openCypher-style graph values: nodes include id, labels,
  and properties; relationships include id, label, endpoints, and properties; paths include ordered
  nodes and relationships; scalar expressions return typed scalar values. The native IR stores result
  shape explicitly so CLI, hosted protocols, bindings, MCP, and future Neo4j compatibility share one typed result
  contract. This is not a full Neo4j product claim.
- **Reason.** A broad tracked roadmap avoids burying hard features such as `MERGE` and all-simple-path
  expansion while still keeping implementation safe. Expensive features are split into capability
  families with resource limits, index/readiness posture, and conformance gates instead of being
  hidden behind a vague query-engine bucket.
- **Source-backed baseline.** Core graph now implements canonical multi-label nodes, a native query IR
  executor for scan-based node and directed-edge patterns, label predicates, property comparisons,
  explicit projections, grouped count aggregation, deterministic ordering, skip, row limits, typed
  node/edge/path/scalar/list/map result values, fixed chained paths, bounded variable-length all-simple paths,
  bounded `shortestPath`, deterministic regex predicates, `length(path)`, `id`, `type`, `startNode`,
  `endNode`, `labels`, `keys`, `properties`, `nodes`, `relationships`, core property-index
  explain/readiness, ready node-property equality selection, atomic mutation plans including `MERGE`,
  and a bounded openCypher/GQL read parser for
  `MATCH`, label/property patterns, comparison and regex predicates with `AND`/`OR`/`NOT`, `RETURN`,
  `ORDER BY`, `SKIP`, grouped `count`, `LIMIT`, `CREATE`, `MERGE`, `SET`, `REMOVE`, `DELETE`, and
  `DETACH DELETE`. Mutation text lowering requires an explicit Loom identity envelope. `MERGE` uses
  that identity envelope as its uniqueness contract and rejects mismatched existing node or edge
  shape with deterministic conflict errors. This baseline is not a full openCypher parser, Bolt
  session contract, or Neo4j compatibility claim. Gremlin is cut from active scope.

### OQ-GR4 - Geospatial graph posture (resolved)

- **Resolution.** Native geospatial graph support is a Loom typed-value contract with closed CRS
  profiles and derived spatial indexes. It is not WKT, WKB, Extended WKB, or GeoJSON stored as opaque
  graph source truth. The target value family follows Simple Features geometry shape: point, line
  string, polygon, multi-point, multi-line string, multi-polygon, and geometry collection. The first
  implementation should prioritize point values, bounding boxes, and distance predicates while keeping
  the value family extensible without changing graph roots.
- **CRS boundary.** Required CRS profiles are `crs84_2d`, `crs84_3d`, `cartesian_2d`, and
  `cartesian_3d`. Geographic coordinates use longitude, latitude, and optional height in meters.
  Arbitrary EPSG/SRID, projection transforms, datum transforms, and PROJ-backed behavior are target
  work and must not be accepted silently.
- **Index boundary.** Spatial indexes are derived artifacts. Canonical declarations name node or edge
  geo properties. Materializations store entity ids and canonical bounding boxes, are source-stamped,
  and report `NotBuilt`, `Stale`, `Ready`, and failure state when the shared derived-artifact lifecycle
  exists. Query planning uses the spatial index as a candidate filter and performs exact predicate
  recheck before returning rows.
- **Presentation boundary.** A bounded openCypher/GQL profile may expose point construction, distance,
  and bounding-box predicates first. GeoJSON, WKT, WKB, and SQL-like `ST_*` names are interchange or
  compatibility surfaces over native graph semantics. They do not define native graph identity.
- **Current source-backed subset.** Point geometry values are implemented as Loom typed graph values
  with closed CRS profiles: `crs84_2d`, `crs84_3d`, `cartesian_2d`, and `cartesian_3d`. The value
  layer validates finite coordinates, CRS84 longitude and latitude bounds, and 2D/3D dimensionality.
  It encodes through the reserved canonical CBOR tag `loom.graph.geometry.v1` and rejects ordinary
  graph lists that would collide with that tag. The shared value projection covers core graph storage,
  `loom-wire`, compute, CLI, MCP, hosted JSON, Neo4j-shaped read records, and local Node, Python, and
  WASM Rust binding layers. Core graph query supports point distance and inclusive x/y bounding-box
  predicates over those point values, with missing properties, non-point values, and CRS mismatches
  evaluating as non-matches. Spatial index declarations are canonical, materializations are transient
  source-stamped artifacts, readiness and stale state are visible through explain/readiness output,
  and ready spatial indexes narrow node and edge point-property predicates before exact predicate
  recheck. Line/polygon geometry families, shared derived-artifact lifecycle orchestration, and
  persistent rebuild scheduling remain target work.

### OQ-GR5 - Neo4j and Gremlin compatibility classification (resolved)

- **Resolution.** Neo4j compatibility is a first-class target served surface, not a `graph` transport.
  Gremlin is cut from the active roadmap. Reopening Gremlin requires a separate design session that
  justifies the client value, execution boundary, bytecode/text subset, step whitelist, resource
  policy, and conformance strategy.
- **Operator shape.** The target Neo4j listener shape is:

```text
loom serve configure app.loom neo4j main people --bind 127.0.0.1:7687
```

- **Reason.** Neo4j documents officially supported application libraries for Python, JavaScript,
  Java, .NET, Go, GraphQL, Node.js, Spring Data, and OGM, plus HTTP and Query APIs for executing
  Cypher over HTTP. Neo4j also documents Bolt as the application protocol for database queries,
  including protocol compatibility and Neo4j drivers. That makes Neo4j a product compatibility
  surface with driver and session expectations, not a generic binary transport under native `graph`.
- **Boundary.** `neo4j/tcp` must own Bolt handshake, sessions, driver expectations, Neo4j-shaped
  records, errors, transactions where supported, catalog/procedure compatibility shims, and transcript
  conformance. It must report unsupported behavior explicitly and must not claim full Neo4j until a
  compatibility matrix proves the supported subset. The native `graph` surface continues to own Loom
  graph semantics, REST/JSON-RPC/gRPC projections, and the bounded GQL-aligned openCypher profile over
  native graph IR.
- **Compatibility matrix.**

| Concern | Target posture | Current capability row |
| --- | --- | --- |
| Surface admission | `loom serve configure <store> neo4j <workspace> <graph> --bind 127.0.0.1:7687` records durable listener intent through the served registry. Daemon runtime opening is source-backed for the bounded Bolt read subset. | `source_backed_read_subset` |
| Official driver targets | Certify at least Neo4j Python and JavaScript drivers first, then widen only with transcript evidence. Current guarded transcripts cover the official Python and JavaScript drivers over the bounded read/write subset. | `source_backed_python_javascript_subset` |
| Bolt handshake and version negotiation | Negotiate one approved Bolt version and reject unsupported versions deterministically. Current source negotiates Bolt 5.1, accepts compatible range offers by selecting the concrete supported version, and declines unsupported versions. | `source_backed_skeleton` |
| Bolt message framing | Decode and encode driver-visible messages without leaking Loom-native internals. Current source handles chunked PackStream framing for the bounded lifecycle subset. | `source_backed_skeleton` |
| Authentication | Reuse hosted principals and app credentials; do not create a separate Neo4j identity root. Current source maps Bolt basic auth into hosted passphrase or app credential authentication and Graph read authorization. | `source_backed_skeleton` |
| Session lifecycle | Support driver open, run, pull, reset, close, and stable unsupported responses for unimplemented messages. Current source handles `HELLO`, `LOGON`, `LOGOFF`, `RESET`, `GOODBYE`, `TELEMETRY`, bounded read/write `RUN`, and bounded `PULL`; unsupported transaction messages fail deterministically. | `source_backed_read_write_subset` |
| Read queries | Lower supported read Cypher into native graph query IR with resource limits and PEP checks. Current source lowers bounded read `RUN` text through the native graph query parser and hosted graph kernel. | `source_backed_read_subset` |
| Write queries | Lower supported `CREATE`, `MERGE`, `SET`, `REMOVE`, `DELETE`, and `DETACH DELETE` into authorized native mutation plans where dialect parity is proven. Current source lowers auto-commit `CREATE` and `MERGE` patterns with deterministic Loom graph identity into native mutation plans; `SET`, `REMOVE`, `DELETE`, and `DETACH DELETE` remain target. | `source_backed_create_merge_subset` |
| Parameters | Support parameter binding for the approved Cypher subset before driver certification. Current source supports scalar literal parameters for the bounded read subset and the bounded `CREATE`/`MERGE` write subset. | `source_backed_scalar_subset` |
| Result records | Return Neo4j-shaped nodes, relationships, paths, records, summaries, and type mappings over native graph values. Current source returns `RECORD` messages with scalar, node, relationship, and path values for the bounded read subset, with official Python and JavaScript driver transcript coverage for relationship/path reads. | `source_backed_read_subset` |
| Error mapping | Map Loom parse, authorization, conflict, resource-limit, and unsupported errors into stable driver-visible errors. Current source maps parse, auth, permission, missing, conflict, resource, and unsupported errors into Neo4j-style error codes for the bounded read subset. | `source_backed_limited` |
| Transactions | Start with auto-commit and a limited explicit transaction profile; multi-database and cluster routing are unsupported. Current source supports auto-commit bounded reads and `CREATE`/`MERGE` writes through `RUN` plus `PULL`; explicit transaction messages remain unsupported. | `source_backed_auto_commit_subset` |
| Bookmarks and routing | Explicitly unsupported until clustered coordination is designed. | `unsupported` |
| Catalog and procedures | Provide limited shims required by certified drivers and common tools. | `target_limited` |
| Full Cypher and full Neo4j | Not claimed. Expand only when matrix rows and transcripts prove behavior. | `unsupported` |

- **Source anchors.** Neo4j application libraries and APIs:
  <https://neo4j.com/docs/getting-started/languages-guides/>. Neo4j Bolt protocol:
  <https://neo4j.com/docs/bolt/current/>. Neo4j HTTP API:
  <https://neo4j.com/docs/http-api/current/>. Neo4j Query API:
  <https://neo4j.com/docs/query-api/current/>.
