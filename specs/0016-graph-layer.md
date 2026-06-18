# 0016 - Graph Layer

**Status:** Partial, current graph substrate and public facade source-backed. **Version:** 0.1.0.
**Capability:** `graph`.

This spec defines the property-graph facet. Current source implements a deterministic Rust substrate in
`loom-core::graph`: id-keyed nodes, id-keyed directed labelled edges, endpoint validation, cascade
removal, portable traversal helpers, reachability, shortest path, canonical encode/decode, staging
under a workspace's graph facet, the language-neutral public graph facade, C ABI and C header
projection, all eight local binding projections, local MCP data-tool projection, hosted REST and
JSON-RPC native projection for core traversal methods, daemon-opened hosted listeners, and executable
behavior conformance. Structured graph root staging and loading are source-backed with component
`ProllyMap` roots for nodes, edges, forward adjacency, and reverse adjacency. Canonical node labels,
bounded query IR, declared property indexes, transient derived property-index materialization,
readiness, stale reporting, ready node-property equality selection, and explain output are
source-backed. Core graph semantic diff and three-way semantic merge primitives are source-backed
for deterministic node, edge, and index-catalog records, independent node/edge edits, property
conflicts, endpoint deletion, label conflicts, and adjacency conflicts. Native hosted gRPC projection
for node upsert/get, edge upsert, server-streaming
neighbors and reachability, bounded openCypher query, explain-query, and capability reporting is
source-backed. Compatibility protocols, generated protobuf artifacts, broader hosted conformance, and
hosted graph diff/merge methods remain target work.

Every operation is scoped to one workspace's graph facet. Cross-workspace graph writes are out of
contract and must fail with `CROSS_WORKSPACE` once the public facade exposes them.

## 1. Current Implementation

`loom-core::graph` implements:

- `Graph::new`;
- `upsert_node(id, props)`;
- `upsert_node_with_labels(id, labels, props)`;
- `node(id)`;
- `graph_node(id)`;
- `node_labels(id)`;
- `set_node_labels(id, labels)`;
- `node_count` and `edge_count`;
- `remove_node(id, cascade)`;
- `upsert_edge(id, src, dst, label, props)`;
- `edge(id)`;
- `remove_edge(id)`;
- `out_edges(id)`;
- `in_edges(id)`;
- `neighbors(id)`;
- `reachable(start, max_depth, via_label)`;
- `shortest_path(from, to, via_label)`;
- `semantic_diff(base, head)`;
- `semantic_merge(base, left, right)`;
- `query(query_ir)`;
- canonical `encode` and `decode`;
- `put_graph(loom, ns, name, graph)`;
- `get_graph(loom, ns, name)`.

Node ids and edge ids are caller-supplied strings. Nodes have zero or more canonical labels and an
ordered string-to-typed-value property map. Edge properties are also ordered string-to-typed-value
maps. Supported property values are null, bool, i64 integer, finite f64 float, text, and bytes. Edges
are directed and carry a string label. `upsert_edge` requires both endpoint nodes to exist and returns
`NOT_FOUND` for missing endpoints. `remove_node(cascade=false)` returns `CONFLICT` when the node still
has incident edges; `cascade=true` removes the node and every incident edge. The property-only node
upsert preserves existing labels.

Traversal is the portable source-backed query surface:

- `out_edges(id)` returns outgoing edges in edge-id order;
- `in_edges(id)` returns incoming edges in edge-id order;
- `neighbors(id)` returns distinct adjacent node ids in sorted order.
- `reachable(start, max_depth, via_label)` returns reachable node ids in sorted order after bounded
  breadth-first traversal;
- `shortest_path(from, to, via_label)` returns the first deterministic shortest path by node id and
  edge id ordering, or absence.

The public `Graph` facade is source-backed through the IDL, C ABI, C header, CLI, Node, Python, C++,
Swift, JVM, Android, React Native, and WASM. Hosted REST and JSON-RPC protocol conformance proves
node upsert/get, edge upsert, graph mutation plans, neighbors, native graph query, native graph
explain-query, and capability reporting over the shared hosted kernel; REST also proves reachability.
Native hosted gRPC exposes node upsert/get, edge upsert, server-streaming neighbors and
reachability, native graph query, native graph explain-query, and capability reporting over the same
hosted kernel, with direct service and daemon-opened listener tests. Local CLI, generated remote
protocol and CLI remote facade calls, MCP, Node, Python, and WASM projections expose the
source-backed native graph query and explain-query subset through shared canonical CBOR result
encoders.
The source-backed native query IR executes scan-based node and directed-edge pattern matches with
label and property predicates, explicit projections, grouped count aggregation, deterministic
ordering, skip, typed node/edge/scalar result values, and row limits. A bounded openCypher/GQL read
parser is source-backed for `MATCH`, label/property patterns, comparison predicates with
`AND`/`OR`/`NOT`, deterministic regex predicates, fixed chained paths, bounded variable-length all-simple paths, path values,
`length(path)`, `id`, `type`, `startNode`, `endNode`, bounded `shortestPath`, `RETURN`, `ORDER BY`,
`SKIP`, grouped `count`, and `LIMIT` over that IR. Path
expansion has deterministic hop, fanout, candidate, row, and path-value byte budgets. List and map
graph values plus `labels`, `keys`, `properties`, `nodes`, and `relationships` are source-backed in
the native query value model. Declared node and edge property indexes, transient derived
materialization, `NotBuilt`/`Stale`/`Ready` readiness, ready node-property equality selection, and
explain output are source-backed in core. Core semantic graph diff emits deterministic node, edge,
property-index, and spatial-index records. Core semantic graph merge performs deterministic three-way
merge for independent node, edge, property, label, endpoint, and index-catalog edits and reports
structured conflicts for same node id, same edge id, endpoint deletion, label conflicts, property
conflicts, adjacency changes, and index definition conflicts. Public Rust re-exports and canonical
wire CBOR encoders expose these records for conformance and binding-visible projection. There is no
source-backed generated protobuf artifact, cursoring, or hosted graph-specific merge method today. A native mutation plan
is source-backed for create
node/edge, merge node/edge, set/remove node/edge properties, delete edge, delete node, and detach
delete node, with graph write authorization, endpoint integrity, conflict errors, atomic graph-level
execution, hosted REST/JSON-RPC action projection, and conformance coverage. Bounded openCypher/GQL
mutation text lowering is source-backed for `CREATE`, `MERGE`, `SET`, `REMOVE`, `DELETE`, and
`DETACH DELETE` through an explicit Loom identity envelope. `MERGE` uses the explicit identity
envelope as its uniqueness contract: existing nodes must already satisfy requested labels and
properties, existing edges must already satisfy endpoints, label, and properties, and mismatches are
deterministic conflicts.
The bounded Neo4j compatibility surface also source-backs auto-commit `CREATE` and `MERGE` write
queries through a deterministic Loom graph identity policy. The policy derives node ids from labels
and canonical property maps, derives relationship ids from endpoint ids, relationship label, and
canonical property maps, requires keyed node patterns and explicit relationship bindings, lowers the
result into native graph mutation plans, and runs through hosted graph authorization. Neo4j `SET`,
`REMOVE`, `DELETE`, explicit transactions, and catalog/procedure shims remain target work. Guarded
official-driver transcript tests are source-backed for the Neo4j Python and JavaScript drivers over
the bounded read/write subset.

## 2. Current Storage Shape

The graph facet path is:

```text
/.loom/facets/graph/<name>
```

`put_graph` creates the graph facet directory and stages a structured graph root at that path through
the workspace working tree. `get_graph` reads that staged root and reconstructs the graph. Workspace
commit, branch, checkout, bundle sync, clone, and garbage collection see the graph through a first-class
`Graph` tree-entry kind and the component object graph under it.

The current committed graph root is an existing `Tree` object with component entries for metadata,
property-index catalog, node map, edge map, forward adjacency, and reverse adjacency. The node, edge,
forward-adjacency, and reverse-adjacency components are `ProllyMap` roots. The property-index and
spatial-index catalogs are canonical metadata. Transient core materialization is source-backed.
Persistent graph index materializations use `loom-store::derived` keys under
`property-index:<index-name>` and `spatial-index:<index-name>` with source-digest, engine-version,
format-version, stale, rebuild, failed, and unsupported status reporting.

## 3. Current Encoding

`Graph::encode` writes a Loom Canonical CBOR array containing:

1. nodes in node-id order, each as `[id, labels, props]`;
2. edges in edge-id order, each as `[id, src, dst, label, props]`.

`labels` is a canonical array of label strings in sorted order. `props` is a canonical map from string
key to typed scalar value. Current source does not encode graph id policy metadata, declared property
indexes beyond the empty catalog, query dialect, or schema metadata.

## 4. Current Versioning and Merge Behavior

Current graph values version with the workspace because they are staged into the workspace working
tree. A commit snapshots the graph root with every other staged workspace path. `checkout_commit` and
`checkout_branch` restore the graph root with the rest of the workspace tree.

Current source implements core node-level and edge-level semantic graph diff and merge primitives on
`Graph` values. The workspace branch merge machinery still treats divergent graph roots as normal
same-path root conflicts unless a promoted graph-specific merge helper consumes those primitives.
Sync follows
`CONFLICT-RESOLUTION-MATRIX.md`: branch/ref divergence uses the S1 fast-forward boundary. Semantic
graph structural merge is classified as a P1 graph primitive rather than a whole-blob
storage-promotion blocker. The structured component roots and the core semantic merge over node and
edge maps are source-backed; workspace branch integration and public projection remain target work.

Current graph diff is object/path-level through the workspace tree unless callers use
`Graph::semantic_diff` directly. The structured component roots make node, edge, and adjacency
objects reachable and structurally shared, and explicit semantic graph diff over node and edge maps is
source-backed in core.

## 5. Current Conformance

`loom-core::graph` has unit tests for:

- id upsert behavior;
- traversal;
- missing endpoint rejection;
- cascade and non-cascade node removal;
- canonical encode/decode;
- structured graph root staging/loading;
- component reachability from commits and live object sets;
- prolly structural sharing after a one-node graph edit;
- semantic graph diff records;
- semantic graph merge success and conflict records;
- commit and checkout versioning.

`loom-conformance` contains graph behavior scenarios and an executable graph facade runner. The runner
exercises node/edge writes, traversal, reachability, shortest path, versioning, and clone behavior
against the in-memory store.

## 6. Target Contract

The target public graph facade should provide:

- node create, upsert, get, scan, and remove;
- edge create, upsert, get, scan, and remove;
- outgoing, incoming, and neighbor traversal;
- canonical multi-label nodes;
- native graph query IR and typed result values;
- optional property indexes;
- explicit graph diff;
- explicit graph merge;
- graph blame or provenance if required by the public facade;
- a declared query dialect if a text query engine is exposed.

Before hosted and full enterprise promotion, the remaining facade needs:

- hosted protocol methods in 0008;
- stable error mapping through `loom_core::error::Code`;
- access-control review for served graph writes;
- clear behavior for graph data under `/.loom/facets/graph/...`;
- node/edge collision behavior aligned with `CONFLICT-RESOLUTION-MATRIX.md`.

## 7. Target Storage Contract

The graph facet stores a structured graph value, not one whole graph blob; this structured root is
source-backed today and uses existing object types and deterministic encodings:

| Role | Encoding | Status |
| --- | --- | --- |
| Graph root | Existing `Tree` root with component entries and metadata | Source-backed |
| Metadata | Blob entry containing root format version, schema policy, key codecs, comparator ids, merge rules, and property-index catalog digest | Source-backed |
| Node map | `ProllyMap` keyed by node id UTF-8 bytes | Source-backed |
| Edge map | `ProllyMap` keyed by edge id UTF-8 bytes | Source-backed |
| Forward adjacency | `ProllyMap` keyed by length-prefixed `(src, label, dst, edge_id)` | Source-backed |
| Reverse adjacency | `ProllyMap` keyed by length-prefixed `(dst, label, src, edge_id)` | Source-backed |
| Property index catalog | Canonical metadata declaring index intent | Source-backed |
| Property index materializations | Derived `ProllyMap` structures keyed by declared property/value/id forms | Target, derived |

The target storage does not add a new graph object type. It uses the existing object model: a graph
working-tree entry points at a `Tree` root, and component roots are reachable `ProllyMap` entries.
Node and edge ids remain caller-supplied strings. Adjacency keys use length-prefixed compound byte
keys rather than delimiter strings. Property values are promoted to typed canonical values; the
current byte-string property bag is not a legacy compatibility profile because Loom is not released.
If those choices change canonical bytes, update conformance vectors with the implementation.

The property index catalog is canonical, but physical index materializations are derived artifacts.
The catalog declares what must be maintained or rebuilt; the materialized index shape can evolve so
long as it preserves the approved query semantics and readiness behavior. Durable local property
indexes are recorded through the shared derived-artifact lifecycle as
`property-index:<index-name>` records using `graph-property-index-v1`, with the source digest supplied
by the graph root and canonical index declaration profile.

The structured graph merge target is node/edge-level three-way merge. Disjoint node and edge edits
merge. Conflicting updates to the same node, same edge, endpoint integrity, property value, or index
catalog entry report structured conflicts instead of silently choosing a side.

## 8. Query Engine Guidance

The current source should not claim Cozo, petgraph, RDF, SPARQL, openCypher, GQL, Neo4j, Bolt, or
Gremlin support.
Those are target or deferred choices.

The approved delivery order is:

1. Promote structured canonical graph storage with node, edge, forward-adjacency, reverse-adjacency,
   and declared property-index roots.
2. Define a bounded GQL-aligned openCypher query contract over that native substrate.
3. Promote `neo4j` as a first-class target served surface only after the bounded query contract,
   Bolt handshake/session scope, result framing, error mapping, and official driver transcript
   conformance scope exist. The compatibility matrix, current capability rows, and durable listener
   admission are source-backed; runtime protocol behavior remains target work.
4. Cut Gremlin from the active graph roadmap. Reopen it only through a separate owner-approved design
   session.

Keep the portable traversal core independent of every query engine. A text query implementation must
advertise its exact dialect and supported subset. It must lower into a Loom native graph query and
traversal IR, not create an engine side store invisible to commits, diff, merge, sync, and garbage
collection. RDF/SPARQL remains deferred in `1000-Deferred.md` unless a concrete use case promotes it.

The native query contract is a versioned graph query IR. GQL-aligned openCypher is the first text
profile over that IR, not the native storage or execution identity. The IR must represent typed graph
patterns, node labels, relationship labels, property predicates, path expressions, projections,
aggregation, mutation plans, cursors, stable errors, authorization boundaries, and resource limits.

The approved openCypher/GQL roadmap is intentionally broad but not unbounded:

| Capability | Target posture | Implementation note |
| --- | --- | --- |
| Node labels | Canonical multi-label node model. | Implement before openCypher label patterns. |
| Read patterns | `MATCH`, `WHERE`, `RETURN`, `ORDER BY`, `SKIP`, `LIMIT`, and grouped aggregation. | Lower into native IR and consume graph component maps. |
| Mutations | `CREATE`, `MERGE`, `SET`, `REMOVE`, `DELETE`, and `DETACH DELETE`. | Enforce graph write authorization, endpoint integrity, identity-envelope uniqueness, and deterministic conflict behavior for every affected row. |
| `MERGE` | Source-backed for identity-envelope node and directed-edge patterns. | Uses explicit Loom identity as the uniqueness contract, atomic match-or-create execution, and deterministic conflict errors. |
| Paths | Fixed paths, bounded variable-length paths, shortest path, and all-simple-path semantics. | All path expansion must have depth, fanout, candidate, row, byte, memory, and time limits. |
| Result values | Follow openCypher-style graph values for the openCypher profile. | Node results include id, labels, and properties; relationship results include id, label, endpoints, and properties; path results include ordered nodes and relationships. |
| Regex predicates | Source-backed. | Uses deterministic non-backtracking regex validation and matching for text properties; non-text properties evaluate false. |
| Full-text predicates | Source-backed cross-facet core bridge. | Graph predicates can consume prepared FTS hit-id projections through the same workspace's full-text facet. Text grammar sugar remains target work. |
| Geospatial predicates | Partial. | Canonical point geo values use Loom typed geometry with closed CRS profiles, source-backed value codecs, source-backed point distance and bbox predicates, and rebuildable source-stamped spatial-index candidate filters for node and edge point properties. Full geometry families and shared derived-artifact lifecycle remain target work. |
| List and map values | Source-backed. | `GraphValue` supports canonical list and map values with recursive validation, parser literals, deterministic ordering, and source-backed cross-surface codecs. |
| Scalar and path functions | Source-backed subset. | Closed deterministic registry currently covers `id`, `type`, `startNode`, `endNode`, `length`, `labels`, `keys`, `properties`, `nodes`, and `relationships`. |
| Property indexes | Source-backed core subset. | Canonical declarations, transient rebuildable materialization, readiness/stale reporting, explain output, and ready node-property equality selection are implemented. Persistent derived-artifact orchestration and public projection remain target work. |

## 9. Relationship to Other Facets

- **Vector:** embeddings and nearest-neighbor search live in the vector facet. A graph may reference a
  vector id in node or edge properties, but vector indexing remains separate.
- **Document and SQL:** graph nodes or edges may reference document ids or table primary keys, but
  cross-facet referential integrity is target work.
- **Compute:** `loom-compute` has a `Graph` capability tag, but graph state access from programs is
  target work until 0015 defines and implements it.
- **Events and observability:** graph change events depend on 0029 and 0030.

## 10. Non-Goals and Limits

- Current source is not a distributed graph engine.
- Current source is not an RDF/SPARQL triplestore.
- Current source does not provide graph analytics over large graphs.
- Current source provides persisted forward and reverse adjacency component roots and canonical
  property-index declarations. Property-index lookup materializations are transient derived state,
  not canonical source identity.
- Current source provides core graph semantic diff and merge primitives, but not graph-specific
  workspace branch merge.

## 11. Unfinished Work

| Order | Parent | Work item | Status | Exit criteria |
| --- | --- | --- | --- | --- |
| T1 | RD6 | Spec/source reconciliation | Complete local | Current implementation, conformance, and change-log text describe the implemented public graph facade instead of stale target-only language. |
| T2 | RD6 | CLI graph projection | Complete local | `loom graph ...` commands expose the source-backed facade with deterministic CBOR input/output where needed. |
| T3 | RD6 | MCP graph data projection | Complete local | MCP tools expose node/edge upsert, get, remove, neighbors, in/out edges, reachability, and shortest path with registered schemas and canonical CBOR payload tests. |
| T4 | RD6 | Non-MCP hosted graph wire projection | Partial | REST and JSON-RPC served protocol conformance proves node upsert/get, edge upsert, mutation plans, neighbors, native query, explain-query, and capability reporting with hosted auth/PEP over the current data route. REST also proves reachability. Native gRPC exposes node upsert/get, edge upsert, server-streaming neighbors and reachability, bounded openCypher query, explain-query, and capability reporting. Remaining work: full CRUD/list parity, cursoring, generated protobuf artifacts, hosted graph diff/merge methods, and compatibility protocols. |
| T5 | RD7 | Structured graph storage | Complete local | Graphs stage and load through a structured `Tree` root with node, edge, forward-adjacency, reverse-adjacency, metadata, and property-index catalog components. |
| T6 | RD7 | Node/edge-level merge | Complete local | Core graph semantic diff emits deterministic node, edge, and index-catalog records. Core graph semantic merge resolves independent node and edge edits and reports structured conflicts for same node id, same edge id, endpoint deletion, label conflicts, property conflicts, adjacency changes, and index definition conflicts. Public Rust re-exports, canonical wire CBOR projection, and conformance vectors are source-backed. Hosted method projection and workspace branch integration remain target work. |
| T7 | RD8 | Query engine decision | Complete local | The target query path is native graph IR with a bounded GQL-aligned openCypher profile over it. |
| T8 | RD14 | Canonical multi-label node model | Complete local | Nodes encode zero or more labels canonically and query label patterns do not depend on pseudo-properties. |
| T9 | RD14 | Native graph query IR | Partial | Source-backed IR covers scan-based node and directed-edge patterns, ready node-property index selection, fixed chained paths, bounded variable-length all-simple paths, path values, path `length`, property and label predicates, explicit projections, grouped count aggregation, deterministic ordering, skip, row limits, typed node/edge/path/scalar/list/map results, canonical CBOR query/explain result encoding, generated remote protocol projection, and atomic mutation plans including `MERGE`. Remaining work: cursors, broader stable errors, broader index-backed planning, and cost controls. |
| T10 | RD14 | Bounded openCypher/GQL profile | Partial | Source-backed parser lowers `MATCH`, fixed chained paths, bounded variable-length all-simple paths, bounded `shortestPath`, path values, `length(path)`, `id`, `type`, `startNode`, `endNode`, `labels`, `keys`, `properties`, `nodes`, `relationships`, label/property patterns, comparison predicates with `AND`/`OR`/`NOT`, `RETURN`, `ORDER BY`, `SKIP`, grouped `count`, `LIMIT`, `CREATE`, `MERGE`, `SET`, `REMOVE`, `DELETE`, and `DETACH DELETE` into native IR through explicit Loom identity for mutations. Remaining work: cursoring, index-backed planning, and broader compatibility conformance. |
| T11 | RD14 | Query mutation and `MERGE` | Complete local | Native mutation plans and bounded openCypher/GQL mutation text lowering are source-backed for create node/edge, merge node/edge, set/remove node/edge properties, delete edge, delete node, and detach delete node. `MERGE` uses explicit identity-envelope uniqueness, idempotent match-or-create behavior, and deterministic conflict errors for mismatched existing node or edge shape. |
| T12 | RD14 | Path expansion and functions | Complete local | Fixed chained paths, bounded variable-length all-simple paths, bounded `shortestPath`, path values, `length(path)`, deterministic result ordering, and hop, fanout, candidate, row, and path-value byte budgets are source-backed. Native shortest-path traversal remains source-backed through `shortest_path`. |
| T13 | RD14 | Predicate and function expansion | Partial | Deterministic non-backtracking regex predicates, canonical list/map values, `id`, `type`, `startNode`, `endNode`, `labels`, `keys`, `properties`, `nodes`, `relationships`, FTS-backed prepared hit-id predicates, point distance/bbox predicates, and spatial-index candidate planning are source-backed with conformance coverage. Remaining work: broader scalar functions and openCypher text grammar for full-text predicates. |
| T14 | RD14 | Property index materialization | Complete local | Declared node and edge property indexes, transient derived materializations, `NotBuilt`/`Stale`/`Ready` readiness, ready node-property equality selection, and explain output are source-backed in core. Durable local property-index records use the shared derived-artifact lifecycle with source-digest, engine-version, format-version, stale, rebuild, failed, and unsupported status reporting. Public projection remains target work. |
| T14b | RD15 | Graph query public projection | Complete local | Local CLI, generated remote protocol and CLI remote facade calls, hosted REST/JSON-RPC, native gRPC, MCP, Node, Python, and WASM expose native graph query and explain-query over the shared source-backed query subset, with shared canonical CBOR encoders for binary projections and native hosted capability reporting. Generated protobuf artifacts and broader conformance remain target work. |
| T14c | RD9 | Neo4j compatibility matrix and admission | Complete local | Neo4j is classified as a first-class `neo4j/tcp` target surface; the matrix covers official driver targets, Bolt handshake, version negotiation, framing, auth, sessions, Cypher subset, result records, error mapping, transactions, bookmarks/routing, catalog/procedure shims, and transcript gates. Native graph hosted capabilities report the Neo4j rows as source-backed read subset, source-backed skeleton, target, limited target, or unsupported. The served registry admits durable `neo4j/tcp` listener intent and rejects stale `graph/bolt` and `graph/gremlin` transports. |
| T14d | RD9 | Neo4j bounded Bolt read/write subset | Complete local | The daemon can open `neo4j/tcp` listeners through the hosted runtime. The listener negotiates Bolt 5.1, handles chunked PackStream framing for `HELLO`, `LOGON`, `LOGOFF`, `RESET`, `GOODBYE`, and `TELEMETRY`, authenticates through hosted passphrases and app credentials, authorizes Graph access, lowers bounded read `RUN` queries with scalar parameters into native graph query IR, streams `PULL` records, and projects scalar, node, relationship, and path values into Neo4j-shaped records. It also lowers auto-commit `CREATE` and `MERGE` write `RUN` queries through deterministic Loom graph identity and native mutation plans, returning an empty pull stream for writes. Guarded official Neo4j Python and JavaScript driver transcripts cover connect, bounded write, bounded read-back, parameters, relationship/path reads, and clean shutdown. It returns stable unsupported execution errors for transaction and unsupported messages. Remaining work: `SET`/`REMOVE`/`DELETE` write execution, transaction semantics, catalog/procedure shims, and full Neo4j compatibility claims. |
| T15 | RD16 | Geospatial value and spatial-index implementation | Partial | Canonical point geo values are source-backed with `crs84_2d`, `crs84_3d`, `cartesian_2d`, and `cartesian_3d` profiles, validation, canonical CBOR vectors, reserved-tag collision rejection, shared wire codecs, compute, CLI, MCP, hosted JSON, Neo4j-shaped read projection, and Node/Python/WASM Rust binding projection. Core query support for point distance and bbox predicates is source-backed with focused tests, hosted capability reporting, and conformance coverage. Derived spatial-index declarations are canonical, materializations are transient source-stamped artifacts, readiness/stale reporting and explain projection are source-backed, ready spatial indexes narrow node and edge point-property predicates with exact predicate recheck, and durable local spatial-index records use the shared derived-artifact lifecycle. Remaining work: line/polygon geometry families and persistent derived-artifact rebuild scheduling. |

## 12. Resolved Decisions

- **RD1 - Current storage.** Current source stages each named graph as a structured `Tree` root under
  the workspace graph facet.
- **RD2 - Current identity.** Caller-supplied node ids and edge ids are the current identity keys.
- **RD3 - Endpoint integrity.** Edges require existing endpoints; dangling edges are not produced.
- **RD4 - Node removal.** `cascade=false` rejects incident edges with `CONFLICT`; `cascade=true`
  removes incident edges.
- **RD5 - Query baseline.** `neighbors`, `out_edges`, `in_edges`, `reachable`, and `shortest_path` are
  the current portable traversal surface.
- **RD6 - Public facade status.** The workspace-scoped public `graph` facade (node/edge upsert, get,
  remove, neighbors, out/in edges, reachable, shortest-path) is source-backed across the engine, the
  C ABI, the IDL, the C header, and all eight bindings, with an executable facade conformance suite.
- **RD7 - Merge boundary.** Core node/edge-level semantic merge is source-backed on `Graph` values.
  Sync does not silently merge divergent graph histories until workspace branch integration consumes
  the same primitive.
- **RD8 - Engine posture.** Query engines and RDF/SPARQL are not current source-backed contracts.
- **RD9 - Query and compatibility order.** Structured native graph storage precedes a bounded
  GQL-aligned openCypher contract. Neo4j compatibility is a first-class served-surface target, not a
  generic `graph` transport, because Neo4j owns product semantics around Bolt sessions, drivers,
  transactions, result records, errors, catalog behavior, HTTP/Query APIs, and client compatibility.
  Gremlin is cut from the active graph roadmap and requires a separate owner-approved design session
  to reopen.
- **RD10 - Structured root shape.** The promoted graph storage uses an existing `Tree` root with
  component `ProllyMap` entries for nodes, edges, forward adjacency, reverse adjacency, and derived
  property-index materializations. No new graph object type is required.
- **RD11 - Property model.** Structured graph storage promotes typed property values now. There is no
  legacy raw-bytes property profile because Loom is not released.
- **RD12 - Key codecs.** Node and edge maps use current string ids as UTF-8 byte keys. Forward and
  reverse adjacency maps use length-prefixed compound byte keys.
- **RD13 - Index and merge posture.** Property index declarations are canonical. Physical index
  materializations are derived. Structured graph merge uses node/edge-level three-way merge in core.
- **RD14 - Graph query roadmap.** The target graph query surface is a native graph IR plus a bounded
  GQL-aligned openCypher profile. The roadmap includes canonical multi-label nodes, read patterns,
  aggregation, mutations including `MERGE`, fixed and variable path semantics, all-simple-path
  behavior, regex, list/map values, scalar and path functions, FTS-backed full-text integration,
  geospatial posture, property-index materialization, explain/readiness, auth, and hard resource
  limits. The first core property-index slice is source-backed for declarations, transient
  materialization, readiness, stale reporting, ready node-property equality selection, and explain.
  The first full-text slice is source-backed through graph predicates that consume FTS hit-id
  projections without creating a second graph text index.
- **RD15 - Source-backed graph query baseline.** Native graph query IR and typed result values are
  source-backed for scan-based node and directed-edge patterns, fixed chained paths, bounded
  variable-length all-simple paths, bounded `shortestPath`, path values, `length(path)`, `id`,
  `type`, `startNode`, `endNode`, `labels`, `keys`, `properties`, `nodes`, `relationships`, label
  predicates, property comparisons, deterministic regex predicates, explicit projections, grouped
  count aggregation, deterministic ordering, skip, row limits, node/edge/path/scalar/list/map result
  values, and atomic mutation plans including `MERGE`. The mutation
  plan covers create node/edge, merge node/edge, set/remove node/edge properties, delete edge, delete
  node, and detach delete node with graph write authorization and endpoint integrity. `MERGE` uses
  explicit identity-envelope uniqueness, idempotent match-or-create behavior, and deterministic
  conflict errors when stored node or edge shape does not satisfy the pattern. A bounded
  openCypher/GQL parser is source-backed for `MATCH`, fixed chained paths, bounded variable-length
  all-simple paths, bounded `shortestPath`, path values, `length(path)`, `id`, `type`, `startNode`,
  `endNode`, label/property patterns, comparison and regex predicates with
  `AND`/`OR`/`NOT`, `RETURN`, `ORDER BY`, `SKIP`, grouped `count`, `LIMIT`, `CREATE`, `MERGE`, `SET`,
  `REMOVE`, `DELETE`, and `DETACH DELETE`. Mutation text lowering requires an explicit Loom identity
  envelope so canonical ids do not come from graph properties. This is not a full Cypher, Neo4j, or
  Bolt compatibility claim. Gremlin is not an active target.
- **RD15b - Public graph query projection.** Native graph query and explain-query are source-backed
  through local CLI commands, generated remote protocol and CLI remote facade calls, hosted native
  graph REST actions, hosted native graph JSON-RPC methods, MCP tools, local Node/Python/WASM binding
  helpers, and shared canonical CBOR result encoders. The hosted graph capability report advertises
  the bounded native profile, point geospatial predicate support, and unsupported Gremlin behavior;
  the Neo4j compatibility rows separately report the bounded Bolt read subset.
- **RD16 - Geospatial posture.** Native graph geospatial support is a Loom typed-value contract, not
  WKT, WKB, or GeoJSON stored as opaque source truth. The target canonical value family is a closed
  Simple-Features-shaped geometry model: point, line string, polygon, multi-point, multi-line string,
  multi-polygon, and geometry collection. WKT, WKB, Extended WKB, and GeoJSON are interchange or
  compatibility encodings over that value family. The first implementation should prioritize point
  values, bounding boxes, and distance predicates, but the source contract must leave room for the
  full geometry family without changing the root model.

  Coordinate policy is closed until transformation support is explicitly designed. Required CRS
  profiles are `crs84_2d`, `crs84_3d`, `cartesian_2d`, and `cartesian_3d`. `crs84_*` uses longitude,
  latitude, and optional height in meters, aligned with GeoJSON/RFC 7946's WGS84 decimal-degree
  posture. `cartesian_*` uses x, y, and optional z in caller units. Arbitrary EPSG/SRID, projection
  transforms, datum transforms, and PROJ-backed behavior are target work and must not be accepted
  silently.

  Distance semantics are deterministic and closed. Point-to-point distance is valid only for matching
  CRS profiles. `crs84_2d` distance returns meters using a fixed geodesic formula chosen and pinned by
  canonical vectors. `crs84_3d` combines the 2D geodesic distance with height difference. Cartesian
  distance is Euclidean in the coordinate unit. Different CRS profiles produce null in expression
  contexts and false in predicate contexts unless a future query profile explicitly maps that case to
  an error.

  Spatial indexes are derived artifacts, not graph source identity. Spatial index declarations are
  canonical and share the graph index readiness vocabulary: `NotBuilt`, `Stale`, `Ready`, and
  failure reporting. Durable local spatial-index records use the shared derived-artifact lifecycle as
  `spatial-index:<index-name>` records using `graph-spatial-index-v1`, with source-digest,
  engine-version, format-version, stale, rebuild, failed, and unsupported status reporting.
  Materializations store entity ids and canonical bounding boxes for indexed geo properties. Query
  planning uses the spatial index as a primary candidate filter and always performs exact predicate
  recheck before returning rows. Candidate planning must support node and edge properties and must
  remain visible to explain output.

  Public graph profiles map onto the native contract. A bounded openCypher/GQL profile may expose
  point construction, distance, and bounding-box predicates first. GeoJSON, WKT, WKB, and SQL-like
  `ST_*` names are compatibility/interchange surfaces and should not dictate native graph identity.

  The current source-backed subset is point geometry values only. `GraphValue::Geometry` stores a
  `Point` with one of the closed CRS profiles above, rejects non-finite coordinates, rejects CRS84
  coordinates outside longitude and latitude bounds, rejects dimensionality mismatches, and encodes
  through the reserved canonical CBOR tag `loom.graph.geometry.v1`. Ordinary list values using that
  tag as the first element are invalid so stored graph values cannot collide with the geometry family.
  Shared projection exists through core graph storage, `loom-wire`, compute, CLI, MCP, hosted JSON,
  Neo4j-shaped read records, and local Node, Python, and WASM Rust binding layers. Native graph query
  support is source-backed for `distance(binding.property, point(...)) <|<=|>|>= number` and
  `within_bbox(binding.property, min_x, min_y, max_x, max_y)` over point values. Distance predicates
  return false when the property is missing, not a point, or has a mismatched CRS. Bounding-box
  predicates are inclusive over x/y coordinates. This is not yet a source-backed claim for spatial
  indexes or line/polygon geometry families.

## Change log

### 0.1.0

The graph facet's workspace-scoped public facade is source-backed end to end, mirroring the KV and CAS
facets: node and edge upsert/get/remove, neighbour and directed out/in edge traversal, reachability,
and shortest path cross the C ABI (`loom_graph_*`), the IDL `Graph` interface, the C header, and the
Node, Python, C++, iOS, JVM, Android, React Native, and WASM bindings. Properties cross as canonical
CBOR `text -> typed scalar` maps and an edge as `[src, dst, label, props]`. Graphs stage through a
structured root with component `ProllyMap` entries for nodes, edges, forward adjacency, and reverse
adjacency. An executable behavioral suite exercises the facade against the in-memory store, so the
`graph` capability is reported executable.
