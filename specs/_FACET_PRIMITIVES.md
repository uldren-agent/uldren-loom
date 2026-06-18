# Facet Primitives

This document is the design inventory for reusable Loom facet primitives. It separates the
native contract Loom should own from product-specific presentations that may depend on it.
It is not an implementation-status document. Source-backed implementation claims remain in
the owning numbered specifications, binding specifications, and conformance evidence.

## Design Rule

A facet owns durable domain primitives. A compatibility facade owns an external product contract
and composes the primitives it needs from one or more facets. A transport carries an unchanged
surface contract. An engine is an internal execution choice. An interchange format defines data
exchange rather than an application-facing service.

Build a primitive natively when it is useful without the facade, shared by multiple facades, or
necessary to avoid a facade owning storage and query semantics that another Loom surface cannot
reuse. Keep a primitive facade-specific when it is meaningful only in the external product model.

Do not create a new facet merely because a product has a wire protocol. Create one when the
product contract introduces a coherent domain with its own data model, lifecycle, policy, or
configuration semantics. A transport carries an unchanged surface contract. A facade or
presentation owns product semantics.

This means the comparison is not always "native facet versus product." A single-facet facade such
as MongoDB can be compared primarily with `document`. A composite facade such as Redis instead has
a dependency graph: cache lifecycle, byte-key records, durable streams, transient fanout, and
product command semantics each have a distinct owner.

## Primitive Scope

This inventory considers the primitives required to make a facet or facade correct, secure,
operable, and reusable. It is not limited to the primary stored value.

| Primitive class | Include in this inventory | Ownership rule |
| --- | --- | --- |
| Domain data model | Include identity, schema, indexes, mutations, queries, retention, derived views, and merge behavior when they define durable Loom data semantics. | The native facet owns the primitive when the behavior is useful independently of one compatibility facade. |
| Cross-cutting control plane | Include principal authentication, authorization, audit, quotas, lifecycle, listener configuration, retention scheduling, and policy enforcement when multiple facets need the same control behavior. | The shared security, management, delivery, coordination, or hosted substrate owns the primitive where that ownership prevents duplicate implementations. |
| Security and cryptography | Include encryption boundaries, key references, authenticated sessions, tenant isolation, secure defaults, and sensitive-data handling when the behavior affects trust or compliance. | Shared security and store identity contracts own these primitives so facades do not reimplement security differently. |
| Runtime and performance | Include batching, backpressure, concurrency, limits, cardinality controls, compaction, cache lifecycle, and recovery behavior when more than one surface depends on the same runtime guarantee. | A shared runtime primitive owns the behavior when reuse prevents product facades from creating incompatible performance semantics. |
| Wire protocol mechanics | Include HTTP, gRPC, TLS, request limits, authentication middleware, and stable error mapping only when the mechanics are shared across surfaces. | The hosted substrate or transport layer owns the mechanics because application facades should not redefine generic session plumbing. |
| Product protocol semantics | Include command grammar, handshakes, response shapes, session modes, capability reporting, and client quirks when they are part of a product compatibility promise. | The compatibility facade owns these requirements because native facets should not be shaped around one vendor protocol. |
| Engines and libraries | Record dependency choice, replaceability, and result-contract boundaries when an implementation engine materially affects behavior. | The internal engine boundary owns the implementation choice, but the native facet owns the stable data and result semantics. |

The test for promotion is practical: a capability becomes a native or shared primitive only when it
has a stable contract, at least two foreseeable consumers, clear ownership, and conformance tests
that do not depend on one vendor protocol. A facade-specific feature stays in the facade even when
it is important to that product.

## Precision Contract

Rows in this inventory should explain the problem being solved, the direction Loom should take, and
the boundary that prevents accidental duplicate implementations. A row is not precise enough when it
only says that a facet "owns" a concept without naming the failure mode, reusable behavior, or
compatibility pressure that makes the primitive necessary.

Use these reading rules:

- `Target`, `planned`, and `pending` identify design intent, not permission to implement without the
  owning specification.
- `Profile` means a declared compatibility subset with routes or commands, accepted inputs, response
  shapes, stable errors, unsupported behavior, capability reporting, and conformance evidence.
- `Policy` means a stored or configured rule with explicit enforcement points, audit behavior, and
  failure behavior.
- `Derived artifact` means rebuildable local state outside commit identity unless an owning spec later
  promotes its bytes into canonical storage. The shared derived-artifact lifecycle (keys, source-anchor
  stamp, states, rebuild coalescing, stale detection, failed/unsupported states, and serve-read policy)
  is defined canonically in 0005 §8.2 and implemented by `loom-store::derived`; the rows below reference
  that contract rather than restating per-facet rebuild lifecycle language.
- `Source-backed` means the owning spec and code already prove the behavior. It does not mean every
  surrounding facade, client transcript, or high-scale path is complete.
- Completion-state tables summarize ownership and status. The primitive tables above them must still
  describe the actual problem and direction before implementation begins.

## Canonical Facet Inventory

The authoritative facet set is `loom_types::FacetKind::ALL` and mirrors `idl/loom.idl`.

| Facet | Primary specification | Current primitive-design round |
| --- | --- | --- |
| `files` | 0003, 0003a, 0003c | Primitive placement is recorded, including native file-tree identity, mount boundaries, file protocol facades, and shared mount-session primitives. |
| `vcs` | 0003, 0003b, 0003d | Primitive placement is recorded, including native workspace history, structural diff and merge, Git-compatible exchange boundaries, and cross-facet version-control primitives. |
| `sql` | 0011, 0011a, 0011b | Primitive placement is recorded, including native relational storage and execution, PostgreSQL and MySQL compatibility boundaries, and shared database-session primitives. |
| `kv` | 0019, 0019a, 0019b | Primitive placement is recorded, including native ordered key-value maps, conditional mutation, cache separation, etcd, Redis, and Memcached boundaries. |
| `document` | 0020 | Primitive placement is recorded, including native document indexes, bounded query, MongoDB, Couchbase, and excluded CouchDB behavior. |
| `vector` | 0017 | Primitive placement is recorded, including native vector collections, Qdrant, Pinecone, acceleration policy, and inference boundaries. |
| `graph` | 0016 | Primitive placement is recorded, including structured graph storage, GQL/openCypher, Neo4j compatibility, query intermediate representation, Gremlin removal from active scope, and whole-blob promotion. |
| `columnar` | 0023 | Primitive placement is recorded, including native columnar manifests, Apache Arrow, Apache Parquet, Arrow Flight, analytical presentations, and dataframe boundaries. |
| `queue` | 0021, 0021a, 0021b, 0021c | Primitive placement is recorded, including native stream logs, delivery, coordination, Kafka, MQTT, NATS, JetStream, and Redis Streams. |
| `time-series` | 0022 | Primitive placement is recorded, including native timestamped points, retention, rollups, Influx, metrics separation, and telemetry facade boundaries. |
| `cas` | 0024 | Primitive placement is recorded, including native content-addressed storage, S3, OCI Distribution, CAR, archive, artifact transfer, IPFS, and retention policy. |
| `ledger` | 0018 | Primitive placement is recorded, including append-only chain semantics, structured log promotion, principal signing, checkpoints, proofs, witness policy, and immudb removal. |
| `program` | 0015 | Primitive placement is recorded, including WASM execution, CEL programs, constrained action envelopes, guards, derivations, statecharts, workflows, and trigger boundaries. |
| `calendar` | 0037 | Primitive placement is recorded, including native calendar records, recurrence, CalDAV, scheduling boundaries, and shared PIM primitives. |
| `contacts` | 0038 | Primitive placement is recorded, including native contact records, vCard, CardDAV, identity resolution boundaries, and shared PIM primitives. |
| `mail` | 0039 | Primitive placement is recorded, including native mailbox storage, IMAP, JMAP, SMTP setup behavior, and shared PIM primitives. |
| `search` / `fts` | 0033 | Primitive placement is recorded, including native FTS, OpenSearch, Tantivy, analyzers, aliases, aggregations, and cross-facet indexing. |
| `dataframe` | 0045 | Primitive placement is recorded, including frame plans, source bindings, portable execution, Polars acceleration, materialization, and analytical result transfer. |

## Shared Change Sets And Retained Gaps

The shared change-set primitive owns replay cursors, ordered change payloads, retained-history
boundaries, and the recovery rule used by facets that expose incremental changes. Queue, watch,
ledger, delivery, calendar, contacts, and mail may keep source-specific cursor internals, but their
public contract maps onto this primitive when they expose resumable reads.

### ChangeCursor

`ChangeCursor` is an opaque, typed resume envelope encoded as Loom Canonical CBOR with a UTF-8 text
projection. Clients store and return the text form without parsing it. Producers MAY decode it to
validate scope, position type, and version.

The cursor envelope carries:

- `scope`, a stable producer-owned string identifying the stream, collection, workspace, mailbox, or
  ref the cursor belongs to;
- `position`, either:
  - `sequence`, the next monotonic sequence to read; or
  - `commit`, a commit digest anchor plus an index within that anchor's ordered event projection.

A malformed cursor, unsupported cursor version, wrong scope, or unreachable commit anchor fails with
`CURSOR_INVALID`. A well-formed sequence cursor that predates the retained low-water mark fails with
`RETAINED_GAP`.

### ChangeSet

`ChangeSet` is the shared incremental payload. It carries:

- `scope`, matching the next cursor scope;
- `gap_state`, one of `retained`, `planned_prune`, or `gap`;
- optional `retained_low_water_mark`;
- `next_cursor`;
- ordered `items`.

Items are either item diffs or sequence records. An item diff uses `added`, `updated`, or `removed`
with a content-address ETag for present records. A sequence record uses `sequence` with a monotonic
sequence and payload bytes. Facets choose the item form that matches their durable model, but they do
not define incompatible gap-state or low-water semantics.

### Gap state and recovery

`retained` means the requested range is fully available. `planned_prune` means the range is still
available but intersects history scheduled for pruning. `gap` means the requested range is no longer
available from retained history.

The retained low-water mark is the earliest sequence position still replayable for a scope. Retention
or pruning that removes replayable history MUST advance the low-water mark atomically with the
retention metadata that caused the removal. A stale cursor below that mark MUST fail with
`RETAINED_GAP` and the universal recovery is full resync from the facet's current snapshot or head.

### Completion State And Primitive Placement

| Capability | Shared primitive owner | Facet projection | Design state |
| --- | --- | --- | --- |
| Opaque typed cursor | `_FACET_PRIMITIVES.md` owns the versioned envelope, text projection, scope check, sequence position, and commit-anchor position. | Watch may keep commit DAG cursors; queue, delivery, and Studio logs may use sequence positions; PIM facets may wrap collection-specific version anchors. | Source-backed in `loom_core::change_set` for canonical sequence and commit cursors. |
| Ordered change payload | `_FACET_PRIMITIVES.md` owns `ChangeSet`, item-diff records, sequence records, `next_cursor`, and payload ordering. | Calendar and contacts project ETag diffs; queue, ledger, delivery, and operation logs project sequence records. | Source-backed in `loom_core::change_set` for canonical encoding and validation. |
| Gap state | `_FACET_PRIMITIVES.md` owns `retained`, `planned_prune`, and `gap`. | Ledger range state maps onto the shared vocabulary; other facets project their retained-history checks onto it. | Source-backed in `loom_core::change_set`. |
| Retained low-water mark | `_FACET_PRIMITIVES.md` owns the comparison rule and `RETAINED_GAP` failure for sequence cursors below the mark. | Each facet owns storing and advancing its low-water mark with its retention metadata. | Source-backed in `loom_core::change_set`; per-facet adoption remains follow-on. |
| Full-resync recovery | `_FACET_PRIMITIVES.md` owns the universal recovery rule for `gap` and `RETAINED_GAP`. | Each facet defines its snapshot or current-head resync payload. | Shared rule is specified here; per-facet recovery projection remains follow-on. |

## Approved Planned Facets

These are approved architectural directions that are not yet members of the canonical
`FacetKind` registry. They remain explicitly separate from the inventory above until core,
IDL, bindings, and conformance add them.

| Facet | Relationship | Decision state |
| --- | --- | --- |
| `cache` | The cache facet is planned as a first-class native lifecycle facet over shared key-value-like runtime primitives. The `kv` facet owns versioned typed maps; the `cache` facet owns volatility, TTL behavior, eviction, capacity, and backing policy. | The direction is approved for design and future promotion into the canonical facet registry. |
| `metrics` | The metrics facet is planned as a native metrics facet over time-series storage. The `time-series` facet owns generic timestamped point storage and rollups; the `metrics` facet owns instrument semantics, units, temporality, resources, scopes, exemplars, staleness, and cardinality policy. | The direction is approved for design and future promotion into the canonical facet registry. |
| `logs` | The logs facet is planned as a native log facet for timestamped structured or unstructured event records, severity, attributes, resource and scope context, trace correlation, retention, and indexed retrieval. | Primitive placement is recorded below; promotion into the canonical facet registry remains future implementation work. |
| `traces` | The traces facet is planned as a native trace facet for trace and span identity, parent-child and link relationships, timing, status, events, attributes, resource and scope context, and trace-oriented retrieval. | Primitive placement is recorded below; promotion into the canonical facet registry remains future implementation work. |

## `document`

### Native

The native document facet is an identifier-keyed document collection. Its durable long-term value is
not opaque-byte create, read, update, and delete operations alone, but portable document indexing and bounded document querying that
can serve native callers, language bindings, MCP tools, CLI,
REST endpoints, JSON-RPC
endpoints, and future presentations through one contract.

The native v1 primitive set is:

| Primitive | Why Loom should own it | Priority |
| --- | --- | --- |
| Declared index catalog | Makes indexing explicit, inspectable, versionable, and controllable by the document owner. | Priority 1 because every useful document query profile depends on declared indexes. |
| Field-path extraction | Addresses nested document fields without each client or facade parsing documents differently. | Priority 1 because document filters and updates need one path interpretation. |
| Typed scalar normalization | Establishes deterministic equality and range comparison across strings, numbers, booleans, null values, and time-like values. | Priority 1 because index keys and predicates cannot be portable without canonical comparison. |
| Maintained secondary-index storage | Delivers indexed lookup without collection scans and is shared by every useful presentation. | Priority 1 because product facades should not build separate secondary-index stores. |
| Index backfill, rebuild, and readiness state | Lets operators distinguish usable, rebuilding, failed, and stale indexes. | Priority 1 because query planning must not silently use incomplete indexes. |
| Bounded query abstract syntax tree | Gives every Loom surface one stable query contract rather than embedding a vendor language in native APIs. | Priority 1 because native document query must exist before product query grammars are projected. |
| Equality, range, and boolean predicates | Covers practical document filtering while remaining testable, deterministic, and index-aware. | Priority 1 because these predicates define the first useful indexed query subset. |
| Stable cursor tokens | Provides forward pagination that does not depend on unstable offsets over mutable collections. | Priority 2 because pagination is needed after the first indexed query contract is defined. |
| Projection | Reduces transfer and gives callers controlled result shape. | Priority 2 because result shaping is reusable but not required to define index identity. |
| Unique and compound indexes | Enables data integrity and more selective access patterns after the single-field model is source-backed. | Priority 2 because these extend the first index contract without replacing it. |
| Sparse, partial, and multikey indexes | Adds advanced index behavior only after canonical semantics and write-maintenance costs are specified. | Priority 3 because these semantics are valuable but easier to misimplement without the simpler index contract. |

Native document query must not absorb FTS, geospatial, analytical aggregation, or
relational query semantics. The FTS facet owns text indexing and ranking; the
structured-query facet owns relational query; dataframe and columnar own analytical transformation
and scans. This boundary prevents incompatible index models and duplicate query engines.

### MongoDB Presentation

MongoDB is a planned low-priority presentation, not the native document contract. A faithful wire surface
would allow Mongo drivers and `mongosh` to attach through the MongoDB wire protocol and BSON
command model. It depends on the native primitives above before a useful subset can be accurately
described and offered without overstating compatibility.

| MongoDB requirement | Native primitive dependency | Placement |
| --- | --- | --- |
| Collection documents and `_id` lookup | Id-keyed document collection | Native foundation |
| `createIndexes` and index-aware `find` | Declared index catalog and maintained secondary indexes | Native foundation |
| Nested field predicates | Field-path extraction and typed scalar normalization | Native foundation |
| `$eq`, range predicates, `$and`, `$or`, `$not` | Bounded query abstract syntax tree | Native foundation |
| Driver cursors | Stable cursor token model | Native foundation |
| Field inclusion/exclusion | Projection | Native foundation |
| `$` update operators and patch behavior | Explicit document mutation model | Presentation-specific follow-on |
| Aggregation pipeline | SQL, dataframe, and columnar boundary design | Do not fold into the first native document version. |
| Text and geospatial indexes | FTS and a future geospatial domain | Do not fold these indexes into the first native document version. |
| Sessions, transactions, replica-set behavior, wire metadata | Hosted lifecycle and future coordination capabilities | MongoDB-specific, deferred |

The MongoDB adapter must not claim broad compatibility merely because it supports `_id` lookup.
It needs a declared, tested compatibility profile whose command subset, index behavior, cursor
semantics, write results, BSON types, error mapping, and unsupported commands are explicit.

### Couchbase Presentation

Couchbase is a planned low-priority integrated presentation. It is not a synonym for MongoDB and must not
be treated as a document-wire adapter. Its product model spans several Loom domains: direct key
operations, JSON documents, query services, and optional analytical access.

| Couchbase capability family | Native Loom dependency | Boundary |
| --- | --- | --- |
| Bucket, scope, and collection addressing | Explicit presentation aliases over namespaces and document collections | Presentation mapping |
| Direct key-value document access | The key-value facet and identifier-keyed document primitives | Cross-facet contract |
| Document field query | Native document indexes and bounded query abstract syntax tree | Native foundation |
| Subdocument operations | Field-path extraction plus a defined atomic mutation model | Candidate shared primitive |
| SQL++ and N1QL-like query | SQL and document-query boundary design | Product-specific language, not the first native document version. |
| Analytics service | Materialized columnar datasets and dataframe/columnar execution | Cross-facet projection |
| Cluster, replication, rebalance, service topology | Coordination and hosted deployment model | Out of current single-node scope |

Couchbase should be designed only after native document query/index work has a source-backed
contract and after key-value, SQL, and columnar owners agree on the cross-facet boundaries. It should
reuse those primitives, not create parallel index, query, or analytical stores inside a facade.

### Explicitly Outside The Current Direction

- CouchDB serving is cut from the active build direction. Revision trees, conflict visibility,
  `_changes`, and replication are an independent replication product, not a small REST addition.
- BSON is an interchange and MongoDB-presentation concern until a source-backed MongoDB
  compatibility subset requires more. It is not the native document identity format.
- Document text search belongs to the `fts` FTS facet, not a separate document
  text-index implementation.
- Document aggregation belongs to SQL, dataframe, or columnar once the requested operation is
  relational or analytical rather than an indexed document lookup.

### Primitive Gaps And Sequencing

1. Specify index declarations, supported path syntax, type normalization, and canonical index keys.
2. Build index maintenance, backfill, rebuild, readiness, and failure reporting.
3. Specify and build the native query abstract syntax tree, index-selection rules, predicates, projections, and cursors.
4. Project that native contract through core, IDL, language bindings,
   CLI, MCP tools, REST endpoints,
   JSON-RPC endpoints, hosted policy, error mapping, and
   conformance.
5. Design and implement MongoDB compatibility as an explicit, testable subset.
6. Design Couchbase as an integrated key-value, document, query, and analytics presentation after its boundaries are
   source-backed.

### Completion State And Primitive Placement

| Capability | Native `document` | MongoDB presentation | Couchbase presentation | Other owner or boundary | Design state |
| --- | --- | --- | --- | --- | --- |
| Id-keyed document storage | The native document facet stores and addresses documents by stable document identifiers. | The MongoDB presentation maps this storage to document records and `_id` behavior. | The Couchbase presentation maps this storage to document collections and direct document values. | The `kv` key-value facet may provide direct key operations where a facade needs byte-key access. | The base storage is source-backed; native query design remains incomplete. |
| Declared secondary indexes | The native document facet should own declared secondary-index definitions and maintained index state. | MongoDB `createIndexes` and useful `find` behavior require this primitive. | Couchbase document query services require this primitive. | No other facet should own document secondary-index semantics. | This is an unbuilt Priority 1 native primitive. |
| Nested field paths and scalar comparison | The native document facet should own path interpretation and scalar comparison rules. | MongoDB-style BSON predicates require this primitive. | Couchbase document and subdocument access require this primitive. | Shared document-mutation contracts may reuse the same path and comparison rules for updates. | This is an unbuilt Priority 1 native primitive. |
| Query predicates and cursor results | The native document facet should own a bounded query abstract syntax tree and stable cursor model. | MongoDB should adapt only an explicitly supported command subset. | Couchbase should adapt only an explicitly supported service subset. | The structured-query facet owns relational grammar and should not be folded into document query. | This is an unbuilt Priority 1 or Priority 2 native primitive, depending on the exact predicate and cursor feature. |
| Projection | The native document facet should own bounded result shaping. | MongoDB should adapt field inclusion and exclusion behavior to the native projection contract. | Couchbase should adapt query result shaping to the native projection contract. | No other facet should own generic document projection. | This is an unbuilt Priority 2 native primitive. |
| FTS | The native document facet should not own text analyzers, ranking, or full-text indexes. | MongoDB may delegate text search to the FTS facet where a declared compatibility profile permits it. | Couchbase may delegate text search to the FTS facet where a declared compatibility profile permits it. | The FTS facet owns analyzers, ranking, and text indexes. | This is deliberately outside the first native document version. |
| Aggregation and analytics | The native document facet should not own analytical aggregation or relational execution. | MongoDB aggregation remains a presentation-specific feature unless a future shared primitive is promoted. | Couchbase analytics remains a presentation-specific feature unless a future shared primitive is promoted. | The structured-query, dataframe, and columnar facets own relational and analytical execution. | The boundary is deferred for a later design review. |
| Wire sessions and cluster behavior | The native document facet should not own product wire sessions or cluster topology. | MongoDB-specific protocol and session behavior belongs to the MongoDB presentation. | Couchbase-specific service and cluster behavior belongs to the Couchbase presentation. | Hosted runtime and future coordination primitives own reusable session and deployment behavior. | This is deferred Priority 3 presentation work. |

## `files`

### Native

The native files facet owns versioned file-tree state: path identity, directory entries, file bytes,
file metadata, symlink-like references where admitted, atomic tree mutations, and file-level policy.
It is the source of truth for what a workspace tree contains. It does not own mount sessions,
kernel-driver mechanics, network file protocol handshakes, operating-system cache behavior, or
client discovery.

### Presentation Candidates

POSIX-like local access, FUSE, NFS,
SMB, WebDAV, and file-oriented Representational
State Transfer are compatibility presentations over the native file tree. Archive import/export is
an interchange profile rather than a live file protocol. Mounting is a top-level served or local
projection over the whole workspace or a declared namespace, not a separate facet.

### Primitive Gaps

| Primitive | Native files role | Presentation role | Current boundary |
| --- | --- | --- | --- |
| Path canonicalization and validation | The native files facet must define path normalization, reserved names, separator rules, case policy, invalid-component errors, and traversal rejection so every surface mutates the same tree. | Mounts, WebDAV, REST, and archive import/export adapt their path syntaxes to the native path contract. | The base file tree exists; the complete cross-protocol path profile remains target conformance work. |
| Directory entry map | The native files facet owns directory identity, child names, child object references, ordering, empty-directory representation, and conflict behavior. | Filesystem and network presentations list and mutate directories through this map. | Source-backed behavior exists; high-scale directory indexing and merge vectors remain target work. |
| File content identity | The native files facet owns file content references, content-addressed byte reachability, digest verification, chunking policy where promoted, and byte-preserving reads. | Mount protocols and file download routes stream bytes without redefining content identity. | File bytes are source-backed; large-file chunking and partial-write policy remain target work. |
| File metadata model | The native files facet owns portable metadata such as type, length, executable bit where admitted, modified-time policy, content type where declared, and extension metadata. | POSIX-like mounts, NFS, SMB, WebDAV, and archive formats map richer metadata where supported. | Basic metadata is source-backed; complete cross-platform metadata mapping remains target work. |
| Atomic tree mutation | The native files facet should own create, make directory, write, rename, copy, delete, and compare-before-write behavior over declared anchors. | Mount and protocol presentations translate open/write/rename/delete sequences into native atomic operations. | Create, delete, and make-directory surfaces are expected primitives; full atomic operation vectors remain target work. |
| Partial read and write policy | The native files facet needs explicit range-read, range-write, append, sparse-file, truncation, and conflict semantics before high-performance mounts can be correct. | FUSE, NFS, and SMB expose partial operations and caching expectations. | Reads are source-backed; complete partial-write and sparse-file semantics remain target work. |
| File locking and leases | The native files facet should not own the whole lock service, but it must bind file operations to the shared lock and lease substrate where file clients require advisory or mandatory locking. | Mount protocols expose lock calls, lease breaks, and stale-handle errors. | Shared lock direction exists; file-specific lock mapping remains target work. |
| Watch and notification integration | The native files facet should emit file lifecycle events through the shared watch and delivery substrate rather than define a separate notification system. | Mounts, desktop integrations, and hosted watchers consume file events. | Cross-facet lifecycle events exist in pieces; file watch projection remains target work. |
| Mount session state | The native files facet should not own mount lifecycle, mount credentials, client cache invalidation, or kernel-driver state. | FUSE, NFS, SMB, and WebDAV own session mechanics. | This remains presentation-owned work. |
| Archive interchange | The native files facet owns the file-tree semantics that archives materialize. | Archive import/export owns tar, tar with Zstandard compression, gzip-compressed tar, zip, deterministic ordering, safe paths, and metadata mapping. | Archive interchange is source-backed in the content-addressed-storage section; file-specific conformance remains target work. |

### Required Separations

| Boundary | Native files responsibility | Presentation responsibility | Reason |
| --- | --- | --- | --- |
| File tree versus mount session | Canonical paths, directories, file bytes, metadata, and atomic mutations | Kernel mounts, network sessions, client discovery, reconnect behavior, and cache invalidation | A failed or disconnected mount must not change native file identity. |
| Native paths versus host paths | Workspace-relative canonical paths and reserved-name policy | Host path syntax, drive letters, case behavior, symbolic-link interpretation, and operating-system errors | Host filesystem behavior must not leak into portable workspace identity. |
| Content identity versus streaming transport | Digest-verified file content and reachability | Byte-range transfer, protocol framing, compression, and response headers | Streaming performance must not redefine stored bytes. |
| File metadata versus protocol metadata | Portable metadata and extension bags | Network protocol attributes, operating-system extended attributes, archive headers, and client-specific fields | Unsupported metadata should be preserved only when the policy says it is meaningful. |
| File locking versus shared lock service | File operation integration and conflict reporting | Lock acquisition protocol, lease renewal, stale locks, and client callbacks | Locks must be reusable by files, database sessions, queue consumers, and hosted operations. |

### File Primitive Sequencing

| Order | Primitive | Why it comes before broader compatibility | Depends on |
| --- | --- | --- | --- |
| 1 | Path canonicalization, directory entries, and metadata profile | Every mount, archive, and hosted route needs the same path and metadata contract before it can claim compatibility. | Existing file tree storage |
| 2 | Atomic tree mutation and compare-before-write | Safe client writes require conflict behavior before protocol adapters map rename, delete, overwrite, and make-directory operations. | Path and directory contracts |
| 3 | Partial read/write, truncation, append, and sparse-file policy | High-performance mounts and large files need range semantics before caching and writeback are trustworthy. | Content identity and mutation contract |
| 4 | Lock and lease integration | Network file clients need stale-handle and lock behavior that composes with the shared coordination substrate. | Shared lock and coordination substrate |
| 5 | Mount-session profiles | FUSE, NFS, SMB, and WebDAV can then map their session behavior without owning storage identity. | File primitives and hosted listener policy |
| 6 | File watch and desktop synchronization profiles | Notifications should consume durable mutations and shared delivery rather than building separate file event state. | Lifecycle event envelope and delivery substrate |

### Completion State And Primitive Placement

| Capability group | Native `files` | Presentation placement | Design state |
| --- | --- | --- | --- |
| File tree, paths, directory entries, file content, and portable metadata | The native files facet owns these canonical workspace semantics. | FUSE, NFS, SMB, WebDAV, REST, and archive profiles adapt them. | The base native contract exists; complete conformance and metadata policy remain target work. |
| Create, make directory, write, rename, copy, delete, and compare-before-write | The native files facet should own these atomic mutation semantics. | Mounts and hosted file routes translate client operations into native mutations. | Basic operations are expected; full atomic mutation vectors remain target work. |
| Partial writes, sparse files, append, truncation, and writeback policy | The native files facet should own portable storage semantics once specified. | High-performance mount profiles consume these rules. | This remains target work. |
| Mount lifecycle, protocol sessions, client discovery, and kernel-driver behavior | The native files facet should not own these session mechanics. | FUSE, NFS, SMB, WebDAV, and local mount adapters own them. | This remains presentation-owned work. |
| File locks, leases, and watch delivery | The native files facet should integrate with shared coordination and delivery primitives rather than duplicate them. | Mounts and desktop integrations expose file-specific client behavior. | Shared substrate exists in pieces; file-specific mapping remains target work. |

## `vcs`

### Native

The native version-control facet owns workspace history across facets: commits, references, branches,
tags where admitted, structural diff, merge, blame, sync anchors, bundle movement, and reachability
of canonical roots. It is not a Git repository internally. Git compatibility is a projection over the
workspace history model, not the source of truth.

### Presentation Candidates

Git-compatible exchange, Git smart transport, bundle import/export, and review-oriented command-line
surfaces are presentations over native workspace history. A future whole-workspace search command can
consume version-control history, but it should not replace structural diff or merge.

### Primitive Gaps

| Primitive | Native version-control role | Presentation role | Current boundary |
| --- | --- | --- | --- |
| Commit object and parent graph | The native version-control facet owns commit identity, parent links, author/committer evidence where admitted, message policy, root pointers, and validation. | Git-compatible exchange maps native commits to Git-visible objects only through an explicit projection. | Native history exists; complete Git-object projection remains target compatibility work. |
| Reference namespace | The native version-control facet owns branch, tag, protected-reference, and workspace reference policy. | Git reference advertisement and push/fetch semantics adapt the reference namespace. | Protected references are source-backed; full Git reference behavior remains target work. |
| Structural diff | The native version-control facet owns unit-aware structural differences across file trees, key-value maps, documents, graphs, columnar manifests, and other canonical roots. | Git-style patch output, command-line review, and hosted review surfaces render those differences. | Basic difference behavior exists; facet-specific structural diff coverage remains target work. |
| Merge and conflict model | The native version-control facet owns merge base selection, per-facet merge dispatch, conflict identity, conflict records, and deterministic resolution rules. | Git-compatible merge and pull behavior can project compatible outcomes. | Merge exists in bounded form; high-cardinality facet merge rules remain target work. |
| Blame and provenance | The native version-control facet should own attribution over logical units rather than only line-based files. | Git blame output adapts file-line attribution where possible. | File-oriented blame exists only as a candidate; cross-facet blame remains target work. |
| Bundle and sync movement | The native version-control facet owns movement of commits, roots, reachable objects, policies, and capability metadata between Loom stores. | Git smart transport owns Git wire behavior; Loom bundle import/export owns native store exchange. | Native bundle direction exists; complete conformance remains target work. |
| Rebase, cherry-pick, revert, and patch application | The native version-control facet should own these operations as transformations over commits and facet roots. | Git-compatible commands expose familiar workflow shapes where supported. | This remains target workflow work. |
| Large derived artifact exclusion | The native version-control facet must exclude rebuildable artifacts from commit identity while preserving their configuration and rebuild status where canonical. | Search, vector, graph, dataframe, and columnar engines rebuild local artifacts after checkout or sync. | The design rule is recorded; enforcement vectors remain target work. |
| Git smart protocol | The native version-control facet should not own Git wire sessions, pack negotiation, sideband behavior, or client quirks. | The Git facade owns reference advertisement, packfile compatibility, fetch, push, and error mapping. | This remains presentation-owned work. |

### Required Separations

| Boundary | Native version-control responsibility | Presentation responsibility | Reason |
| --- | --- | --- | --- |
| Loom commit graph versus Git object database | Commit identity, parent graph, root pointers, and facet-aware reachability | Git object names, trees, blobs, commits, tags, packfiles, and protocol negotiation | Git compatibility must not redefine Loom canonical bytes. |
| Structural diff versus text patch | Logical unit differences, per-facet diff rules, and stable conflict records | Unified diff rendering, Git patch syntax, and review display | Text patches cannot represent every Loom facet correctly. |
| Merge policy versus protocol pull | Merge base, per-facet merge rules, conflict identity, and deterministic resolution | Pull/fetch/push command flow and remote negotiation | Network transport should not choose storage merge semantics. |
| Derived artifacts versus committed state | Canonical configuration and source anchors | Engine indexes, caches, acceleration payloads, and local rebuild products | Rebuildable artifacts must not pollute commit identity or sync. |
| Protected references versus client permissions | Native reference policy, authorization, audit, and rejection errors | Git push status and hosted administration response envelopes | Security policy must be enforceable outside Git. |

### Version-Control Primitive Sequencing

| Order | Primitive | Why it comes before broader compatibility | Depends on |
| --- | --- | --- | --- |
| 1 | Commit graph, reference namespace, and reachability | Every exchange, diff, and merge feature needs stable source identity first. | Existing workspace commit model |
| 2 | Facet-aware structural diff | Review and merge must understand more than files before broad multi-facet workflows are safe. | Canonical roots for each facet |
| 3 | Per-facet merge rules and conflict records | Synchronization and collaboration need deterministic conflict behavior before Git-like flows claim compatibility. | Structural diff and facet contracts |
| 4 | Bundle and native sync conformance | Native movement between Loom stores should be correct before Git compatibility is treated as the primary exchange path. | Commit graph and reachability |
| 5 | Git-compatible projection | Git can then be a client-facing compatibility layer without becoming the native repository model. | Native version-control conformance |
| 6 | Blame, rebase, cherry-pick, revert, and review workflows | Higher-level developer workflows should consume the stable diff and merge substrate. | Structural diff, merge, and reference policy |

### Completion State And Primitive Placement

| Capability group | Native `vcs` | Presentation placement | Design state |
| --- | --- | --- | --- |
| Commit graph, references, branches, protected references, reachability, and native sync | The native version-control facet owns workspace history and movement semantics. | Git-compatible exchange and bundle import/export adapt those semantics. | The native contract exists; conformance and compatibility coverage remain target work. |
| Structural diff and merge | The native version-control facet owns facet-aware logical differences, merge dispatch, and conflict records. | Git-style patch output and review surfaces render compatible subsets. | Basic behavior exists; broad facet-specific coverage remains target work. |
| Blame, provenance, rebase, cherry-pick, revert, and patch application | The native version-control facet should own these history transformations where promoted. | Git-compatible commands and hosted review tools adapt workflow presentation. | This remains target work. |
| Git reference advertisement, pack negotiation, sideband protocol, and client quirks | The native version-control facet should not own Git wire mechanics. | The Git facade owns Git protocol compatibility. | This remains presentation-owned work. |
| Derived artifact exclusion | The native version-control facet owns the rule that rebuildable artifacts are excluded from commit identity. | Engines rebuild local artifacts after checkout, sync, or daemon reconciliation. | The rule is recorded; enforcement vectors remain target work. |

## `sql`

### Native

The native structured-query facet owns relational storage and execution: schemas, tables, rows,
types, constraints, indexes, query planning, result sets, transaction boundaries, and stable errors.
It should provide a reusable relational kernel that PostgreSQL, MySQL, analytical presentations, and
native command-line or binding callers can share. It does not own product wire handshakes, product
session variables, product catalog illusion, or dialect-specific administrative behavior.

### Presentation Candidates

PostgreSQL and MySQL are first-class hosted compatibility surfaces. Other database dialects require
separate product-semantic assessment.

### Primitive Gaps

| Primitive | Native structured-query role | Presentation role | Current boundary |
| --- | --- | --- | --- |
| Catalog and schema model | The native structured-query facet owns database, schema, table, column, type, constraint, index, view where promoted, and metadata identity. | PostgreSQL and MySQL expose compatible catalog views and information schema where supported. | Base schema behavior exists; complete compatibility catalogs remain target work. |
| Row storage and indexes | The native structured-query facet owns row identity, primary keys, secondary indexes, uniqueness, null semantics, ordering, and storage layout. | Product facades adapt query and metadata behavior without owning row truth. | Row storage exists; broader index and constraint coverage remain target work. |
| Query abstract syntax tree and planner | The native structured-query facet owns parsed relational intent, deterministic planning, stable errors, cost limits, and unsupported-feature reporting. | PostgreSQL and MySQL parsers lower supported dialect subsets into the native query model. | Query execution exists; dialect coverage and explainability remain target work. |
| Transaction boundaries | The native structured-query facet owns transaction begin, commit, rollback, isolation profile, write conflict behavior, savepoints where promoted, and audit. | PostgreSQL and MySQL expose session transaction commands and client-visible errors. | Bounded transactions exist; full isolation and savepoint profiles remain target work. |
| Prepared statements and portals | The native structured-query facet should own reusable prepared-plan identity, parameter typing, invalidation, and result-shape guarantees. | PostgreSQL and MySQL map protocol-level prepare, bind, execute, and cursor concepts. | This remains target session work. |
| Result transfer and cursors | The native structured-query facet owns result schemas, row ordering, cursor anchors, pagination, and bounded memory policy. | Wire protocols, Arrow Flight SQL, and dataframe adapters transfer results. | Basic result sets exist; high-volume transfer and cursor conformance remain target work. |
| Administration and data definition | The native structured-query facet owns reusable data-definition semantics where they affect stored schema. | PostgreSQL and MySQL own product-specific commands, extensions, roles, server settings, replication commands, and unsupported behavior. | Core data definition exists; product administration remains out of native scope. |
| Product wire sessions | The native structured-query facet should not own startup packets, authentication negotiation, product session variables, wire encodings, or client quirks. | PostgreSQL and MySQL facades own those compatibility sessions. | First-class surfaces exist; broader compatibility matrices remain target work. |
| Analytical bridge | The native structured-query facet should expose stable results to dataframe and columnar without making analytical engines define relational truth. | Dataframe, columnar, warehouse-like, DuckDB-like, Spark-like, and BigQuery-like presentations consume relational results. | The direction is approved; result-handle and bulk-transfer work remains target. |

### Required Separations

| Boundary | Native structured-query responsibility | Presentation responsibility | Reason |
| --- | --- | --- | --- |
| Relational model versus product dialect | Tables, rows, types, constraints, query semantics, transactions, and errors | PostgreSQL syntax, MySQL syntax, product catalogs, extensions, and server variables | Product dialects should not fork the relational kernel. |
| Catalog truth versus compatibility views | Native schema and object metadata | PostgreSQL catalog tables, MySQL information schema, and compatibility aliases | Compatibility views can be partial without corrupting native metadata. |
| Transaction semantics versus wire session | Isolation, conflict detection, commit ordering, rollback, and audit | Startup, authentication, prepared-statement messages, session variables, and protocol state | Session mechanics should not define storage correctness. |
| Structured-query execution versus analytical execution | Relational semantics and result sets | Dataframe transformations, columnar scans, Apache Arrow transfer, and warehouse client profiles | Analytical acceleration must consume relational results without changing query truth. |
| Product administration versus Loom administration | Stored schema, authorization hooks, audit, and stable errors | Server configuration, replication, extension management, and product-specific administration commands | Loom should not pretend to be a full PostgreSQL or MySQL server unless that product profile is explicitly approved. |

### Structured-Query Primitive Sequencing

| Order | Primitive | Why it comes before broader compatibility | Depends on |
| --- | --- | --- | --- |
| 1 | Catalog, schema, row storage, and core constraints | Product clients need stable relational identity before dialect or wire behavior matters. | Existing structured-query storage |
| 2 | Query abstract syntax tree, planner limits, and stable errors | PostgreSQL and MySQL dialect subsets need one native semantic target. | Catalog and row model |
| 3 | Transactions, isolation profile, conflict detection, and audit | Database clients expect correctness under concurrent sessions before compatibility claims are meaningful. | Query and write path |
| 4 | Prepared statements, cursors, result handles, and cancellation | Real clients rely on session efficiency and bounded result behavior. | Transactions and query planner |
| 5 | PostgreSQL and MySQL compatibility matrices | Each product surface can then declare supported commands, wire behavior, catalog views, and unsupported features. | Native relational conformance |
| 6 | Analytical result transfer and dataframe/columnar bridge | High-volume analytical clients need bulk transfer after relational result identity is pinned. | Result handles and analytical primitives |

### Completion State And Primitive Placement

| Capability group | Native `sql` | Presentation placement | Design state |
| --- | --- | --- | --- |
| Schemas, tables, rows, types, constraints, indexes, query execution, and result sets | The native structured-query facet owns these reusable relational semantics. | PostgreSQL, MySQL, CLI, bindings, hosted routes, dataframe, and columnar consume them. | Native and hosted direction exists; detailed conformance remains target work. |
| Transactions, isolation, write conflicts, rollback, and audit | The native structured-query facet should own storage correctness and stable errors. | PostgreSQL and MySQL expose session transaction commands. | Bounded behavior exists; full isolation and savepoint profiles remain target work. |
| Prepared statements, cursors, result handles, cancellation, and bulk result transfer | The native structured-query facet should own reusable plan and result semantics. | PostgreSQL, MySQL, Arrow Flight SQL, dataframe, and analytical facades adapt transfer behavior. | This remains target session and transfer work. |
| Product dialect grammar, product catalogs, startup handshakes, session variables, and wire encodings | The native structured-query facet should not own product compatibility mechanics. | PostgreSQL and MySQL presentations own these product-specific semantics. | First-class wire surfaces exist; compatibility matrices remain target work. |
| Server administration, replication, extensions, cluster behavior, and product-specific role systems | The native structured-query facet should not imply full database-server equivalence. | Product facades own only explicitly approved subsets; Loom administration and principal policy remain shared control-plane concerns. | This is outside the current native structured-query scope unless separately approved. |

## Control-Plane Adjacent Shared Primitives

Control-plane surfaces are not data facets, but several of them carry reusable primitives that every
facet and facade depends on. These primitives should be specified once and projected through
CLI, bindings, hosted administration, MCP tools, and future
operator interfaces. A data facet should call into these contracts rather than embedding its own
listener registry, lock store, migration logic, or policy engine.

### Shared Primitive Placement

| Primitive | Problem it solves | Shared owner | Facet or facade relationship | Current direction |
| --- | --- | --- | --- | --- |
| Store lifecycle | Store creation, clone, import, export, encryption setup, rekey, migration, and integrity checks need one contract so every facet can survive movement and schema evolution. | Store management owns durable store lifecycle, format migration, encryption boundary, import/export policy, and integrity verification. | Every facet stores canonical roots inside the store and must declare migration behavior when its schema changes. | Core store behavior is source-backed; complete cross-facet migration and capability reporting remain target work. |
| Serve listener registry | Hosted listeners need durable configuration, capability reporting, enable/disable/remove lifecycle, and stable unsupported-feature errors without each facade inventing listener state. | The serve substrate owns durable listener records, listener reconciliation, bind policy, exposure policy, authentication mode, limits, and audit metadata. | Facades declare supported surfaces, transports, selectors, and runtime adapters. | The hosted substrate is source-backed for several surfaces; full capability reporting, direct TLS, and timeout enforcement remain target work. |
| Daemon local coordination | Long-running local work needs process lifecycle, listener reconciliation, rebuild jobs, maintenance jobs, and local coordination without making daemon own served product semantics. | The daemon owns local process lifecycle, startup reconciliation, runtime supervision, and local maintenance scheduling. | Search, vector, columnar, dataframe, hosted listeners, and future maintenance jobs register work through shared contracts. | Daemon substrate exists; broader job scheduling and capability reporting remain target work. |
| Lock and lease authority | File mounts, database sessions, queue consumers, hosted writes, and maintenance jobs need one stale-safe lock and lease model. | The lock and coordination substrate owns lock records, leases, fencing tokens, renewal, expiry, stale detection, and audit. | Files, structured query, queue, delivery, maintenance, and hosted writes consume the shared authority. | Single-node coordination direction exists; cross-facet lease integration remains target work. |
| Mount projection | Mounting should expose a selected store or namespace without pretending each facet mounts independently. | The mount surface owns local projection configuration, namespace selection, mountpoint validation, read-only mode, and client session policy. | Files supplies the dominant file-tree view; other facets may appear through controlled virtual paths only when explicitly designed. | The surface direction is resolved; full mount profile conformance remains target work. |
| Principal authentication and authorization | Hosted protocols, command-line operations, bindings, and local agents need consistent identity and policy decisions. | The principal and policy-enforcement substrate owns authentication, authorization, grants, protected references, audit, and stable denial errors. | Every facet and facade checks policy through the shared substrate instead of bypassing to lower-level storage. | Hosted authorization is source-backed in several paths; broader principal-management routes and application-specific credentials remain target work. |
| Audit and retention control | Operators need explainable changes, security evidence, retention, pruning, and export without one audit model per facet. | The audit and retention substrate owns audit records, redaction, export, prune policy, retention holds, and evidence boundaries. | Ledger, store, hosted listeners, mail, content-addressed storage, artifacts, and administration consume audit and retention primitives. | Audit exists in hosted administration; automatic retention compaction and shared policy remain target work. |
| Capability reporting | Clients and operators need to know whether a feature is configured, compiled, available, unsupported, or denied before relying on it. | Capability reporting owns feature state, stable unavailable reasons, limits, supported profiles, and unsupported-feature errors. | Facets, facades, optional engines, compile-feature runtimes, and hosted listeners publish capability records. | Capability records exist in pieces; a complete matrix remains target work. |
| Background maintenance jobs | Derived index rebuilds, compaction, retention pruning, migration, and cache refresh need durable scheduling and failure reporting. | The maintenance substrate owns job identity, leases, coalescing, retry, cancellation, status, and audit. | FTS, vector, columnar, dataframe, ledger, content-addressed storage, and PIM derived indexes consume it. | Durable-local status helpers exist in some facets; shared maintenance orchestration remains target work. |

### Required Separations

| Boundary | Shared control-plane responsibility | Data facet or facade responsibility | Reason |
| --- | --- | --- | --- |
| Store lifecycle versus facet schema | Store format, encryption, migration runner, import/export, and integrity checks | Facets declare canonical root formats, migration steps, and conformance vectors | Store movement must not require every facet to build a private migration system. |
| Serve registry versus product semantics | Listener records, bind policy, enable/disable/remove lifecycle, limits, exposure, and audit | Facades implement protocol grammar, request semantics, and capability declarations | A listener can be configured without making its facade runtime available. |
| Daemon lifecycle versus served surface | Process supervision, reconciliation, maintenance scheduling, and local coordination | `serve` owns what is exposed; facades own how requests behave | Daemon remains local coordination rather than a product hosting language. |
| Locks and leases versus data operations | Lease identity, fencing, expiry, renewal, stale detection, and audit | Facets decide which operations require locks and how conflicts are reported | Shared locks prevent duplicate correctness models across files, databases, queues, and maintenance. |
| Mount projection versus native storage | Mountpoint validation, namespace selection, read-only policy, and session handling | Files and other explicitly projected facets provide virtual tree contents | A mount is a projection of a store or namespace, not a separate storage owner. |
| Capability state versus implementation promise | Configured, compiled, available, unsupported, denied, and limit states | Facets and facades report the capabilities they actually implement | A saved configuration must not be mistaken for a running or supported feature. |

### Control-Plane Primitive Sequencing

| Order | Primitive | Why it comes before broader compatibility | Depends on |
| --- | --- | --- | --- |
| 1 | Store lifecycle and facet migration contract | Every canonical facet root depends on durable movement, migration, and integrity behavior. | Store and facet root identity |
| 2 | Principal authentication, authorization, and audit | Hosted and local operations need one security decision path before more product facades are exposed. | Principal and policy substrate |
| 3 | Serve listener registry and capability reporting | Operators need truthful configured/compiled/available state before optional runtimes and compatibility ports proliferate. | Hosted substrate and stable errors |
| 4 | Lock and lease authority | Files, database sessions, queue consumers, and maintenance jobs need stale-safe coordination before high-concurrency surfaces expand. | Coordination substrate |
| 5 | Background maintenance jobs | Derived artifacts, compaction, pruning, and migrations need shared scheduling before each facet creates private job runners. | Daemon lifecycle, locks, and audit |
| 6 | Mount projection profiles | Mounts should map onto stable files, locks, and capability contracts rather than defining those contracts themselves. | Files, locks, serve or local mount policy |

### Completion State And Primitive Placement

| Capability group | Shared owner | Facet or facade placement | Design state |
| --- | --- | --- | --- |
| Store lifecycle, import/export, encryption, rekey, migration, and integrity checks | Store management should own these durable operations. | Facets declare schemas and migration behavior. | Source-backed in pieces; full cross-facet migration matrix remains target work. |
| Durable served listener records, enable/disable/remove lifecycle, bind policy, exposure, limits, and audit | The serve substrate owns listener configuration and reconciliation. | Facades implement protocol runtimes and capability declarations. | Source-backed for selected hosted surfaces; full runtime capability reporting and direct TLS remain target work. |
| Daemon process lifecycle, reconciliation, local jobs, and maintenance supervision | The daemon owns local coordination. | Hosted listeners, derived indexes, compaction, and maintenance tasks register work. | Source-backed in bounded paths; general job orchestration remains target work. |
| Locks, leases, fencing, and stale detection | The lock and coordination substrate owns reusable authority. | Files, structured query, queue, delivery, hosted writes, and maintenance jobs consume it. | Single-node direction exists; cross-facet integration remains target work. |
| Mount projection over store or namespace | The mount surface owns projection configuration and session policy. | Files supplies the primary tree view; other facets require explicit virtual-path design. | Surface direction is recorded; full mount conformance remains target work. |
| Principal authentication, authorization, protected references, and policy enforcement | The principal and policy substrate owns security decisions. | Every facet and hosted protocol must call through it. | Hosted authorization is source-backed in several paths; identity-management and application-specific credential surfaces remain target work. |
| Audit, retention, prune, and evidence export | The audit and retention substrate owns reusable evidence policy. | Ledger, content-addressed storage, artifacts, mail, hosted administration, and store management consume it. | Audit exists in pieces; automatic retention compaction and shared policy remain target work. |
| Capability reporting and stable unavailable reasons | Capability reporting owns configured, compiled, available, unsupported, denied, and limit states. | Optional engines, compile-feature runtimes, and facades publish records. | Partial capability reporting exists; complete matrix remains target work. |

## `kv`

### Native

Ordered typed key-value collections, range access, and bounded conditional mutation are the native
domain. The public key-value contract keeps typed ordered keys distinct from byte-keyed product
facades.

### Presentation Candidates

etcd is a key-value-oriented compatibility facade. Redis, Memcached, and cache lifecycle are analyzed
as separate facade or facet sections because none is merely a key-value transport.

### Primitive Gaps

Structured prolly-map storage over content-addressed value components is source-backed in the native
KV facet. Large-value chunking, semantic per-key merge, and broader batch compare-and-apply remain to
be specified and built. Cache promotion must make the existing runtime tier a first-class contract
without changing the volatility boundary.

### Completion State And Primitive Placement

| Capability group | Native `kv` | Presentation placement | Design state |
| --- | --- | --- | --- |
| Ordered typed values and range operations | The native key-value facet owns ordered typed values, range operations, and the structured prolly root over content-addressed value components. | etcd adapts compatible range and key-value behavior; Redis and Memcached remain byte-keyed facade models. | The native base, prolly storage slice, bounded point lookup, and bounded range scan are source-backed. |
| Conditional mutation | The native key-value facet owns source-backed single-key conditional mutation and should own ordered batch compare-and-apply once that contract is promoted. | Redis, Memcached, etcd, locks, and backed caches adapt the shared conditional-mutation behavior to their product commands. | Single-key exact tokens are source-backed; ordered batch compare-and-apply remains incomplete. |
| Cache durability, expiration, eviction, and restart behavior | The native key-value facet should not own cache lifecycle semantics. | The planned `cache` facet owns lifecycle; Redis and Memcached select compatible cache policies. | This behavior moved to the approved `cache` facet direction. |
| Streams, publish-subscribe, and consumer semantics | The native key-value facet should not own stream retention, fanout, or consumer progress. | The Redis facade composes the native queue facet and delivery primitives for stream and publish-subscribe behavior. | The cross-facet facade direction is resolved. |

## `cache`

### Native

`cache` is an approved planned facet over shared key-value-like runtime primitives. It owns volatile
entries, TTL behavior, idle TTL behavior, capacity accounting, eviction, local
coordinator lifecycle, and explicit backing policies. Its entries are never versioned, merged,
synchronized, bundled, or silently made durable. Its durable configuration can travel with a workspace
so another coordinator recreates the same cache shape with empty runtime state.

### Facade Relationships

Memcached is a cache compatibility facade. Redis can select `ephemeral` or `backed-cache` behavior
while remaining a full data-structure server. Native `cache` is not a proxy protocol and does not
need a Redis or Memcached command model.

### Primitive Gaps

The underlying runtime implementation exists as a key-value tier. Promotion requires a canonical
`cache` facet contract, collection model, ACL and management semantics,
IDL, language bindings, CLI, MCP tools,
hosted policy, conformance, and a migration from the current key-value-tier-only public description.

### Completion State And Primitive Placement

| Capability | Native `cache` | `kv` relationship | Redis facade | Memcached facade | Design state |
| --- | --- | --- | --- | --- | --- |
| Volatile entries and local coordinator state | The planned cache facet owns volatile entries and local coordinator state. | The key-value facet does not version or synchronize cache entries. | Redis can expose this behavior through an explicit `ephemeral` profile. | Memcached uses this behavior as its default backing model. | The runtime primitive is source-backed; the canonical cache facet remains incomplete. |
| Time-to-live behavior, idle TTL behavior, lazy expiry, and sweeps | The planned cache facet owns expiration lifecycle semantics. | Versioned key-value maps do not acquire cache semantics. | Redis maps key-expiry commands and live views to this lifecycle. | Memcached maps item expiry and touch behavior to this lifecycle. | The primitive is source-backed; facade contracts are only partially source-backed. |
| Capacity, accounting, and eviction | The planned cache facet owns capacity tracking, accounting, and eviction behavior. | The key-value facet has no versioned-map eviction contract. | Redis can expose ephemeral cache policy where declared. | Memcached treats this as core cache behavior. | The primitive is source-backed; cache facade promotion remains incomplete. |
| Read-through, write-through, write-around, and write-behind | The planned cache facet owns backing policy and the rules for how cache operations interact with durable storage. | The key-value facet may provide the durable backing map when a cache profile declares one. | Redis supports a `backed-cache` profile only where that profile is explicitly declared. | Memcached should expose a backed profile only after that profile is explicitly designed. | The engine path is source-backed; hosted lifecycle scope remains incomplete. |
| Conditional writes | The planned cache facet uses the shared native key-value and cache conditional-mutation substrate. | The key-value facet owns durable compare-and-apply implementation. | Redis maps compatible conditional commands to the shared substrate. | Memcached maps compare-and-swap token behavior to the shared substrate. | The shared primitive is approved; implementation remains incomplete. |

## `metrics`

### Native

`metrics` is an approved planned facet over the time-series storage substrate. It owns the semantic
contract that makes telemetry metrics interoperable: metric descriptor identity, instrument type,
unit, description, monotonicity, aggregation temporality, resource identity, instrumentation scope,
attribute cardinality policy, exemplars, and stale-series behavior. `time-series` remains useful for
generic timestamped measurements that are not telemetry instruments.

The current source-backed Prometheus, Grafana, and OTLP metrics routes write and query canonical
structured time-series points. That is a valid transitional substrate, not the final semantic owner.
Promotion to a first-class `metrics` facet should preserve the existing structured point storage where
it is useful, but move telemetry meaning out of facade-specific mappings and into one native metrics
contract that every facade can share.

### Facade Relationships

Prometheus and OTLP metrics are compatibility facades over `metrics`. Influx line protocol can map
to generic `time-series` points or declared `metrics` collections. Grafana is a composite datasource
facade that queries metrics alongside other Loom domains.

### Native Promotion Rule

Promote `metrics` when Loom needs metric correctness that a generic point stream cannot express
without facade-specific duplication. The promotion should add a planned `FacetKind::Metrics`,
IDL projection, language binding projection, command-line surface, Model
Context Protocol surface, hosted native surface, conformance vectors, and a migration path from the
current OTLP and Grafana structured-point mapping.

The native `metrics` contract must own:

| Primitive | Why `metrics` owns it | `time-series` role | Facade role |
| --- | --- | --- | --- |
| Metric descriptor identity | Prevents Prometheus, OTLP, and Grafana from disagreeing about the same instrument | Stores point fields and rollups | Maps descriptor names, help text, units, and metadata |
| Instrument kind | Counter, gauge, histogram, exponential histogram, summary, and observable instruments have different validity rules | Stores timestamped values after normalization | Maps product-specific metric families and OTLP data points |
| Temporality and monotonicity | Sum/counter reset behavior and delta/cumulative interpretation affect query correctness | Stores raw and derived samples | Maps Prometheus counter/rate and OTLP aggregation temporality |
| Resource and instrumentation scope | Telemetry identity is not only labels; it includes process/service/resource and library scope | Tags can persist normalized dimensions | Maps OTLP resource/scope and Prometheus label conventions |
| Attribute cardinality policy | Prevents unbounded label explosions from becoming invisible storage failure | Enforces storage and query limits where delegated | Maps label, attribute, and datasource limits |
| Stale-series behavior | Prometheus-style staleness and OTLP absence rules affect query results | Stores raw points and retention metadata | Maps stale markers, absent series, and query response semantics |
| Distribution model | Histograms, exponential histograms, buckets, counts, sums, and exemplars need typed semantics | Stores bucket/count point sets or derived rollups | Maps Prometheus histograms and OTLP histogram encodings |
| Exemplars and trace links | Cross-signal correlation requires a stable native reference, not a facade-only field | May store timestamps and point links | Maps exemplar fields and trace/span references |
| Metric query abstract syntax tree | A reusable bounded query model avoids embedding PromQL as the native metrics language. | Time-series storage supplies point scans, range scans, rollups, and pruning. | PromQL, Grafana query models, and OTLP read paths lower into it. |

### Native Descriptor And Point Contract

The native descriptor should be the stable identity record for one metric stream family. It should not
be inferred from whichever compatibility endpoint first writes a sample.

| Descriptor field | Native requirement | Compatibility mapping |
| --- | --- | --- |
| `metric_id` | Stable Loom identifier for descriptor references and migrations | Not exposed as Prometheus label or OTLP name unless explicitly requested |
| `name` | Canonical metric name with a normalized display form and original facade name where needed | Prometheus metric name and OTLP metric name |
| `kind` | Closed enumeration: counter, gauge, histogram, exponential histogram, summary, and observable variants where promoted | Prometheus type family and OTLP point type plus monotonicity |
| `unit` | Canonical unit string plus optional source unit and conversion policy | OTLP unit and Prometheus naming/unit conventions |
| `description` | Non-identifying descriptive text | OTLP description and Prometheus help text |
| `temporality` | Closed enumeration: instant, cumulative, delta, and derived where needed | OTLP aggregation temporality; Prometheus counters are cumulative |
| `monotonic` | Required for counters and sums where rate/reset semantics matter | Prometheus counter and OTLP Sum monotonic flag |
| `resource_schema` | Declares required and optional resource attributes | OTLP Resource attributes and external labels in Prometheus-style systems |
| `scope_schema` | Declares instrumentation scope name, version, attributes, and schema URL when present | OTLP Scope and optional facade metadata |
| `attribute_schema` | Declares allowed labels/attributes, types, maximum lengths, and cardinality budget | Prometheus labels and OTLP datapoint attributes |
| `distribution_schema` | Declares explicit buckets, exponential histogram scale bounds, minimum/maximum policy, and summary quantile policy | Prometheus native/classic histograms and OTLP Histogram and ExponentialHistogram |
| `staleness_policy` | Declares lookback, stale marker handling, absent-series behavior, and query visibility | Prometheus lookback/staleness and OTLP no-recorded-value flags |
| `retention_policy` | Declares raw sample retention, rollups, destructive prune rules, and audit requirements | Prometheus retention settings, OTLP backend policy, and Grafana query horizons |

Native metric points should be normalized into typed point families rather than one generic
timestamp/value pair.

| Point family | Required native fields | Valid transformations |
| --- | --- | --- |
| Gauge | Descriptor identifier, resource identifier, scope identifier, attribute-set identifier, timestamp, numeric value, optional start timestamp, flags, and optional exemplars | Last-sample alignment and rollup into histograms only when explicitly configured |
| Counter or sum | Descriptor identifier, identity identifiers, start timestamp, timestamp, numeric value, monotonic flag, temporality, reset marker, flags, and optional exemplars | Delta-to-cumulative and cumulative-to-delta only when reset and overlap rules are satisfied |
| Histogram | Descriptor identifier, identity identifiers, start timestamp, timestamp, count, sum, optional minimum and maximum, explicit bounds, bucket counts, temporality, flags, and exemplars | Temporal/spatial aggregation when bucket schema and temporality permit it |
| Exponential histogram | Descriptor identifier, identity identifiers, start timestamp, timestamp, count, sum, optional minimum and maximum, scale, zero threshold, zero count, positive buckets, negative buckets, temporality, flags, and exemplars | Scale reduction and temporal/spatial aggregation when semantics are preserved |
| Summary | Descriptor identifier, identity identifiers, timestamp window, count, sum, quantiles, and flags | Compatibility only unless a native summary query contract is promoted |

### Native Policy Requirements

| Policy | Native rule |
| --- | --- |
| Descriptor conflict | If two writers report the same name, resource, and scope with conflicting identifying fields, Loom must preserve the conflict as a semantic error instead of silently merging incompatible streams. |
| Cardinality | Descriptor and collection policy must bound attribute keys, values, distinct series, active series, and churn. Over-budget writes fail or are sampled according to explicit policy. |
| Staleness | Query semantics must distinguish absent, stale, no-recorded-value, and zero. A default Prometheus-style lookback can be a facade policy, but native metrics must store enough state to make the behavior explicit. |
| Temporality | Cumulative and delta streams must track start timestamps, overlap, reset, and gap behavior. Transformations are allowed only when the output remains semantically equivalent. |
| Exemplars | Exemplars are optional point attachments with timestamp, value, filtered attributes, and optional trace and span identifiers. They must not become the trace source of truth. |
| Histograms | Bucket layout, exponential scale, inclusivity, min/max policy, and aggregation rules are part of descriptor compatibility, not query-time guesses. |
| Rollups | Rollups are derived views over native metric points. They must record source descriptor, attribute projection, aggregation window, temporality transform, and rebuild status. |
| Conformance | Descriptor identity bytes, conflict cases, reset/gap handling, stale behavior, histogram aggregation, exemplar round trips, and cardinality errors need canonical vectors before promotion. |

### Prometheus Compatibility Profile

Prometheus compatibility is a facade over native `metrics`, not the native query contract. Loom should
implement a bounded Prometheus HTTP API and
PromQL profile that works with common clients, Grafana,
exporters, and remote-write senders while keeping semantic ownership in the native metrics descriptor,
point, and query abstract syntax tree.

The first Prometheus profile should cover:

| Application programming interface family | Prometheus behavior to expose | Loom primitive dependency | Profile boundary |
| --- | --- | --- | --- |
| Instant query | `GET` and `POST /api/v1/query` with `query`, `time`, `timeout`, `limit`, `lookback_delta`, and Prometheus JSON envelope | Native metric query abstract syntax tree, staleness policy, point scans, and limit accounting | Return vector, scalar, string, and histogram-compatible results where supported. |
| Range query | `GET` and `POST /api/v1/query_range` with `start`, `end`, `step`, `timeout`, `limit`, and `lookback_delta` | Range planner, downsampling/rollup policy, point window scans | Enforce bounded range, step, sample, and series limits before execution |
| Series and labels | `/api/v1/series`, `/api/v1/labels`, and `/api/v1/label/<name>/values` | Descriptor catalog, attribute index, cardinality budgets | Approximate Prometheus metadata behavior is acceptable only when documented in capability output |
| Metadata and formatting | Metric metadata, `/api/v1/format_query`, and stable response envelopes | Descriptor catalog and PromQL parser | Formatting can be source-compatible without exposing a Prometheus internal abstract syntax tree as stable Loom state. |
| Exemplars | `/api/v1/query_exemplars` for time-bounded compatible exemplar reads | Native exemplar links and trace/span references | Experimental surface; advertise only after exemplar storage vectors exist |
| Remote write receiver | Prometheus remote-write 1.x HTTP receiver with Snappy-compressed Protobuf, sorted non-empty labels, float64 values, and millisecond timestamps | Descriptor ingestion, cardinality policy, temporality inference, write batching | Receiver profile only; sender, WAL shipping, and replication are separate choices |
| Scrape and target metadata | `/api/v1/targets` and scrape-pool metadata only when Loom owns configured scrape jobs | Listener config, scrape scheduler, audit, and auth | Do not fake target discovery for writes received from remote write or other facades |
| Administration and rule APIs | Delete-series, snapshots, configuration, rules, alerts, and alertmanager APIs | Retention, rule engine, alerting, lifecycle, and administration authorization | Out of first profile unless their owning primitives are promoted |

The bounded PromQL profile should be explicit:

| PromQL area | Include in first profile | Defer or reject until supported |
| --- | --- | --- |
| Selectors | Metric selectors, label matchers, range selectors, `offset`, `@`, scalar and duration literals | Ambiguous or product-internal selector extensions |
| Operators | Unary minus; arithmetic, comparison, logical/set, vector matching, `by`, `without`, and top-level aggregation where sample type rules are implemented | Experimental fill modifiers and histogram trim operators until histogram semantics are conformance-pinned |
| Core functions | `rate`, `irate`, `increase`, `delta`, `idelta`, `sum_over_time`, `avg_over_time`, `min_over_time`, `max_over_time`, `count_over_time`, `last_over_time`, `present_over_time`, `absent`, and `absent_over_time` | Forecasting, smoothing, calendar helpers, and experimental functions unless there is clear client demand |
| Histogram functions | `histogram_count`, `histogram_sum`, `histogram_avg`, and quantile support after native histogram vectors exist | Native histogram arithmetic, trim, and mixed float/histogram annotations until vectors prove compatibility |
| Staleness | Prometheus-style lookback and stale behavior mapped from native staleness policy | Implicit last-value carry beyond declared lookback |
| Result annotations | Warnings and info annotations in the Prometheus envelope for partial support, dropped histogram samples, and limit truncation | Silent lossy rewrites |
| Query safety | Parse, plan, and budget before scanning: max series, samples, range, step count, regex cost, output bytes, and wall-clock timeout | Unbounded PromQL execution |

Capability reporting must tell clients which endpoints, functions, sample types, histogram features,
remote-write versions, limits, and experimental APIs are available. This keeps Grafana and SDK users
from mistaking a bounded compatibility profile for a full Prometheus server.

### Primitive Gaps

Promotion requires a canonical metric descriptor model; typed counter, gauge, histogram, summary, and
exponential-histogram data; units and temporality; resource and scope identity; cardinality budgets;
exemplar links; stale-series rules; metrics query planning; migration from structured time-series
points; and complete conformance across core, IDL, language bindings,
CLI, MCP tools, hosted listeners, and facade profiles.

### Completion State And Primitive Placement

| Capability | Native `metrics` | `time-series` relationship | Prometheus facade | OTLP facade | Design state |
| --- | --- | --- | --- | --- | --- |
| Timestamped scalar samples | The planned metrics facet owns telemetry meaning for scalar samples. | The time-series facet stores ordered point series, structured fields, and rollups. | The Prometheus facade maps labeled samples to native metrics. | The OTLP facade maps gauge and sum points to native metrics. | The structured point substrate is source-backed; the native metrics contract remains incomplete. |
| Descriptor, instrument type, unit, monotonicity, temporality | The planned metrics facet owns descriptor identity and instrument semantics. | The time-series facet does not infer telemetry meaning from generic fields. | The Prometheus facade maps metric families, metadata, and staleness behavior. | The OTLP facade maps the OpenTelemetry metric data model. | This native primitive is approved but not fully implemented. |
| Resource, scope, and attributes | The planned metrics facet owns canonical telemetry identity. | Time-series tags can carry normalized dimensions where delegated. | The Prometheus facade maps labels under Prometheus rules. | The OTLP facade maps Resource and Scope attributes. | This native primitive is approved but not fully implemented. |
| Histograms, exponential histograms, summaries | The planned metrics facet owns typed distribution semantics. | The time-series facet may store bucket and count aggregates plus derived rollups. | The Prometheus facade maps native histogram semantics. | The OTLP facade maps OpenTelemetry histogram types. | This primitive is not built. |
| Exemplars and trace correlation | The planned metrics facet owns cross-signal references. | The time-series facet does not own trace identity. | The Prometheus facade exposes compatible exemplar reads. | The OTLP facade maps exemplars. | This primitive is not built and depends on the planned traces facet. |
| Cardinality, retention, rollups, and stale series | The planned metrics facet owns policy semantics for cardinality, retention, rollups, and stale data. | The time-series facet supplies storage, pruning, and derived-view mechanisms. | The Prometheus facade maps label and stale-marker rules. | The OTLP facade maps aggregation and retention policy. | Storage support is partial; the metrics policy contract remains incomplete. |
| Native metrics query abstract syntax tree | The planned metrics facet owns bounded metric query semantics. | The time-series facet supplies point scans, range scans, and rollups. | PromQL lowers into the native query model where compatible. | OTLP read and export paths use the native query model if promoted. | This remains target design work. |
| Prometheus API and bounded PromQL profile | The planned metrics facet should not own product grammar. | The time-series facet uses native query semantics and point scans as storage support. | The Prometheus facade owns HTTP routes, response envelopes, parser profile, and remote-write receiver mapping. | This is not an OTLP concern. | The target profile is defined; implementation remains incomplete. |
| Existing Prometheus, Grafana, and OTLP routes | The planned metrics facet should not own existing product grammar. | Current routes are backed by structured time-series points. | The Prometheus facade has a source-backed bounded profile. | The OTLP facade has a source-backed HTTP JSON gauge and sum profile. | These mappings should move to native metrics after promotion. |

## `logs`

### Native

`logs` is an approved planned facet. It owns append-oriented records with timestamp, severity,
body, attributes, resource and instrumentation-scope context, trace correlation, retention class,
structured field extraction, indexed selection, cursors, and bounded query behavior. It is not a
ledger: logs may be retained, compacted, indexed, redacted by policy, and queried without claiming
tamper-evident history semantics.

### Facade Relationships

OTLP logs and Grafana log datasource integrations are primary facade consumers.
The FTS facet can own text indexing and ranking where logs need FTS, but it
does not replace log ordering, retention, or event context.

### Native Log Primitive Profile

The native `logs` facet should use an OpenTelemetry-compatible shape where it improves
interoperability, while still owning the Loom storage, indexing, retention, and authorization
contract.

| Primitive | Native log requirement | Facade mapping |
| --- | --- | --- |
| Log identity | Stable record identifier, collection identifier, ingest sequence, event timestamp, observed timestamp, and source clock metadata | OTLP LogRecord timestamp and observed timestamp; Grafana log frames |
| Severity | Canonical severity number, original severity text, normalized display text, and severity comparison ordering | OTLP severity number/text and existing log-level formats |
| Body | Typed body value that supports strings, numbers, booleans, bytes where approved, arrays, maps, and null | OTLP AnyValue body and structured log payloads |
| Resource and scope | Shared telemetry resource and instrumentation-scope identity | OTLP resource logs and scope logs |
| Attributes | Typed attribute map with key policy, value limits, redaction class, and cardinality budget | OTLP attributes, JSON logs, and syslog structured data |
| Trace correlation | Optional trace identifier, span identifier, trace flags, and cross-signal links | OTLP log trace context, Grafana Explore links, and metric exemplars |
| Indexes | Time, severity, resource, scope, attribute, event name, trace identifier, and optional FTS projections | Grafana filters, OTLP receiver queries, and search integration |
| Retention and redaction | Retention class, legal hold, prune policy, field redaction, and audit record of destructive changes | Admin and compliance surfaces |
| Query abstract syntax tree | Bounded filter, projection, ordering, cursor, and aggregation model | Grafana logs, OTLP read profiles if promoted, CLI, MCP tools, and hosted routes |

Log ingestion should preserve source timestamps and observed timestamps separately. Query semantics
must not conflate missing severity, INFO severity, absent attributes, and empty strings.

### Primitive Gaps

The native log record, index model, retention semantics, query abstract syntax tree, full-text
boundary, trace correlation contract, redaction rules, and OTLP and Grafana
conformance vectors require a dedicated design round.

### Completion State And Primitive Placement

| Capability | Native `logs` | Facade relationship | Design state |
| --- | --- | --- | --- |
| Timestamped event records, severity, body, attributes | The planned logs facet owns timestamped event records, severity, body values, and attributes. | OTLP log records map to this native shape. | This is planned native work. |
| Resource, scope, and trace correlation | The planned logs facet owns shared telemetry context for logs. | OTLP and Grafana use this context for cross-signal navigation. | This is planned native work. |
| Full-text indexing and ranking | The planned logs facet should not own ranking-engine semantics. | The `fts` FTS facet supplies text indexes and search primitives. | This boundary still needs design. |
| Retention, redaction, legal hold, and prune audit | The planned logs facet owns log lifecycle policy. | Administration and compliance surfaces consume this lifecycle policy. | This is planned native work. |
| Bounded log query abstract syntax tree and cursors | The planned logs facet owns bounded log query semantics and cursor behavior. | Grafana, hosted routes, CLI, MCP tools, and possible OTLP read profiles adapt it. | This remains target design work. |
| Tamper evidence and audit history | The planned logs facet should not own tamper-evident audit history. | The ledger facet and audit surfaces retain tamper-evidence responsibility. | This separation is deliberate. |

## `traces`

### Native

`traces` is an approved planned facet. It owns trace and span identity, parent-child and link
relationships, start and end timestamps, status, events, attributes, resource and scope context,
sampling state, exemplars, and trace-oriented query primitives. It is not a general property graph:
the trace contract has strict lifecycle, timing, ingestion, retention, and correlation semantics.

### Facade Relationships

OTLP traces and Grafana trace datasource integrations are primary facade consumers. `graph` may
support separate analytical projection later, but cannot replace the trace storage and query model.

### Native Trace Primitive Profile

The native `traces` facet should preserve OpenTelemetry trace compatibility while keeping trace
storage, indexing, sampling evidence, and retention inside Loom-owned primitives.

| Primitive | Native trace requirement | Facade mapping |
| --- | --- | --- |
| Trace and span identity | 16-byte trace identifier, 8-byte span identifier, parent span identifier, validity checks, lowercase hexadecimal display, and store-local record identifier | OTLP span identifiers, W3C Trace Context, and Grafana trace frames |
| Span timing | Start timestamp, end timestamp, duration, observed ingest time, and clock-source metadata | OTLP spans and trace visualizations |
| Span kind and status | Internal, client, server, producer, consumer, status code, and status message | OTLP span kind/status and service maps |
| Resource and scope | Shared telemetry resource and instrumentation-scope identity | OTLP resource spans and scope spans |
| Attributes | Typed attribute map with cardinality, redaction, and index policy | OTLP attributes and vendor semantic conventions |
| Events | Ordered event list with timestamp, name, attributes, and bounds | OTLP span events and exception events |
| Links | Ordered links to span contexts with attributes and cross-trace support | OTLP links and asynchronous workflow traces |
| Sampling evidence | Sampled flag, trace state where admitted, ingestion source, and drop/count metadata | OTLP trace flags and collector behavior |
| Indexes | Trace identifier, service/resource, operation name, status, duration, attributes, time, and span relationship indexes | Grafana trace lookup, metrics exemplars, log correlation |
| Query abstract syntax tree | Trace lookup, service or operation search, duration filters, attribute filters, dependency edges, and cursor pagination | Grafana, hosted routes, CLI, MCP tools, and possible OTLP read profiles |

Trace storage should support parent-child tree reconstruction without requiring graph projection.
Graph projection is useful for broad relationship queries, but it must be derived from trace truth.

### Primitive Gaps

The native trace/span model, trace-aware indexes, ingestion completion rules, sampling evidence,
retention policy, TraceQL-like facade boundary, and cross-linking with metrics and logs require a
dedicated design round.

### Completion State And Primitive Placement

| Capability | Native `traces` | Facade relationship | Design state |
| --- | --- | --- | --- |
| Trace and span identity and topology | The planned traces facet owns trace identity, span identity, parent-child relationships, and link relationships. | OTLP spans and links map to this native shape. | This is planned native work. |
| Timing, status, events, attributes | The planned traces facet owns timing, status, events, and attributes. | Grafana trace views consume this data. | This is planned native work. |
| Resource, scope, exemplars, and log links | The planned traces facet owns shared telemetry context for traces. | The OTLP correlation model supplies inputs. | This is planned cross-facet work with metrics and logs. |
| Native trace indexes and query abstract syntax tree | The planned traces facet owns trace indexes and bounded trace query semantics. | Grafana, hosted routes, CLI, MCP tools, and possible OTLP read profiles adapt it. | This remains target design work. |
| Sampling evidence and ingestion accounting | The planned traces facet owns sampling evidence and ingestion accounting. | OTLP and collector-compatible diagnostics map to this data. | This is planned native work. |
| General graph analytics | The planned traces facet should not own general-purpose graph analytics. | The graph facet may consume an explicit trace projection. | This separation is deliberate. |

## Compatibility Facades

Compatibility facades are separately analyzed products. They do not become native facets merely
because they open a port. Their sections identify the product semantics the facade owns, the Loom
primitives it consumes, and the places where a reusable primitive should be promoted.

### `redis`

Redis is a composite facade. A client connects to one RESP port and sees one coherent Redis command
space, even though Loom routes its behavior to several domains internally. The facade owns Redis byte
keys, type tags, command grammar, RESP sessions, response shapes, errors,
command compatibility profiles, and product-level atomicity rules. It does not turn Redis Streams
into native key-value storage or publish-subscribe into durable queue delivery.

The approved target is a broad Redis profile delivered through declared compatibility profiles:

| Redis family | Primitive placement | Facade responsibility | Target state |
| --- | --- | --- | --- |
| Strings, counters, bit operations | Redis typed byte-value substrate; shared conditional mutation where applicable | RESP command and error semantics | Broad Redis profile |
| Keyspace, expiry, scan, persistence mode | `cache` lifecycle plus durable Redis key catalog | Maps TTL, expiry, persistence, live-view, and scan behavior | Broad Redis profile |
| Hashes and sets | Redis structured subrecords | Field/member command semantics and efficient counts/scans | Broad Redis profile |
| Lists and sorted sets | Redis sequence and dual ordering indexes | Push/pop/range/rank/score semantics | Broad Redis profile |
| Streams and consumer groups | `queue` retention and compaction plus shared leased work claims | `X*` commands, consumer groups, pending entries, acknowledgement, trimming | Required cross-facet integration |
| Publish-subscribe | Delivery runtime fanout | `PUBLISH`, subscribe sessions, patterns, and at-most-once behavior | Required cross-facet integration |
| JSON, Search, TimeSeries, and vector extensions | `document`, FTS, `time-series`, and `vector` only through explicit facade integrations | Namespaced module command and profile compatibility | Future explicit integrations, not implicit byte-key-value emulation |
| Lua, Functions, cluster, replication, Sentinel | Compute, hosted runtime, and future coordination only where individually designed | Product runtime and deployment semantics | Deferred |

The current source-backed facade already implements RESP authentication plus
strings, TTL behavior, hashes, sets, lists, and sorted sets through structured records. It
intentionally returns Redis-shaped unsupported errors for Streams and publish-subscribe until `queue`
and delivery integration land.

### Completion State And Primitive Placement

| Capability | Redis facade owns | Depends on | Current state | Next primitive work |
| --- | --- | --- | --- | --- |
| RESP, command parsing, response shapes, and error shapes | The Redis facade owns this product protocol behavior. | Hosted listener runtime | This behavior is source-backed. | Expand only through declared compatibility profiles. |
| Byte-key catalog, type tags, and structure metadata | The Redis facade owns this Redis-specific keyspace behavior. | Redis substrate storage | This behavior is source-backed. | Pin complete structure and operational invariants. |
| Expiry and live views | Redis command semantics | `cache` lifecycle concepts and durable Redis metadata | Source-backed for current commands | Promote canonical cache facet and shared conditional mutation |
| Hashes, sets, lists, sorted sets | Redis command semantics | Structured facade-specific subrecords and indexes | Source-backed subsets | Complete broad profile command families without whole-structure rewrites |
| Streams and consumer groups | Redis `X*` contract | `queue` retention and compaction plus shared leased work claims | Explicitly unsupported | Add queue-to-Redis facade integration |
| Publish-subscribe and pattern subscriptions | Redis session contract | Delivery runtime fanout | This behavior is explicitly unsupported. | Add delivery-to-Redis facade integration with at-most-once semantics. |
| Modules and server-side programming | Product compatibility profile | Owning facets, compute, and hosted runtime | Deferred | Design per module before advertising commands |

### `memcached`

Memcached is a cache compatibility facade, not a reduced Redis profile. It owns the Memcached text
and later Meta Text protocol contract, byte-key restrictions, client flags, item TTL interpretation,
compare-and-swap tokens, and compatibility statistics. It consumes `cache` lifecycle primitives and
should not make versioned key-value behavior visible by default.

### Completion State And Primitive Placement

| Capability | Memcached facade owns | Depends on | Current state | Next primitive work |
| --- | --- | --- | --- | --- |
| Basic text protocol and item commands | The Memcached facade owns the text protocol and item-command behavior. | Hosted listener runtime and cache state | This behavior is source-backed. | Add conformance with real clients when available. |
| Item expiry and touch | The Memcached facade owns Memcached-specific expiry interpretation. | `cache` lifecycle | This behavior is source-backed. | Project it through the canonical cache facet after promotion. |
| Compare-and-swap tokens | The Memcached facade owns token format and response behavior. | Shared conditional-mutation substrate | Local token behavior is source-backed. | Align durable and runtime conditional-mutation contracts. |
| Meta Text protocol | The Memcached facade owns the Meta Text protocol when implemented. | Cache state and extended item metadata | This behavior is not implemented. | Design and implement it as the preferred modern Memcached profile. |
| Cluster topology and distributed cache behavior | The Memcached facade should not claim multi-node cluster behavior on the current runtime. | Hosted provider and future coordination | This behavior is outside the current scope. | Do not imply multi-node Memcached behavior. |

### `etcd`

etcd is a key-value-oriented compatibility facade with an external revision, lease, transaction, and watch
contract. It is distinct from native typed key-value storage because etcd clients expect byte-key ranges, monotonic
revisions, compare-and-apply transactions, lease attachment, compaction, and replayable watches.

### Completion State And Primitive Placement

| Capability | etcd facade owns | Depends on | Current state | Next primitive work |
| --- | --- | --- | --- | --- |
| gRPC API, revision headers, and byte-key behavior | The etcd facade owns this etcd protocol behavior. | Hosted listener runtime | This behavior is source-backed. | Broaden conformance only against the declared API subset. |
| Leases, compaction, and watch event replay | The etcd facade owns this etcd compatibility behavior. | etcd adapter state | Single-node behavior is source-backed. | Evaluate extraction only if another facade needs the same semantics. |
| Compare transactions | The etcd facade owns the etcd API shape. | Shared conditional-mutation substrate | Adapter behavior is source-backed. | Align with the native bounded compare-and-apply contract. |
| Raft, peer replication, quorum, and cluster membership | The etcd facade should not claim distributed etcd cluster behavior on the current runtime. | Future coordination backend | This behavior is outside the current scope. | Never advertise cluster compatibility on the current runtime. |

### `kafka`

Kafka is a queue and coordination facade. Its committed records map to queue streams, while topic
metadata, partition sequencing, producer epochs, consumer-group generations, transaction records,
and fencing map to `loom-coordination`. The approved target is a complete single-broker profile with
multi-partition topics and normal client consumer-group behavior. It must report one broker and no
replication, in-sync replicas, controller quorum, or leader election.

### Kafka Single-Broker Profile

Kafka is valuable even with a single broker when the client-visible broker semantics are real. The
profile should remain explicit: single-node does not mean fake offsets, fake idempotence, or silent
downgrade when clients request transactions.

| Kafka concern | Product-owned behavior | Shared primitive | Current or target state |
| --- | --- | --- | --- |
| Topics and partitions | Topic names, partition identifiers, metadata versions, leader epoch, topic UUIDs, and API responses | Queue streams plus coordination sequencers | Single-partition behavior is source-backed; multi-partition behavior remains target design work. |
| Record batches | Kafka record-batch v2 validation, keys, values, headers, timestamps, compression, visible offsets | Queue append/range and payload storage | Source-backed bounded profile |
| Fetch visibility | High watermark, last stable offset, aborted transaction markers, isolation level behavior | Queue range plus transaction metadata | Source-backed bounded transaction visibility |
| Offset commits | Kafka group identifier, topic-partition offsets, metadata, and transactional offset visibility | Queue consumer offsets plus coordination transaction records | Source-backed offset commit and transactional commit path |
| Consumer groups | Join, sync, heartbeat, assignment, generation, member epochs, and stale generation errors | Coordination generations, leases, and queue offsets | This remains target design work. |
| Idempotent producers | Producer identifier, producer epoch, per-partition sequence continuity, and duplicate retry handling | Coordination producer epochs and sequence high-waters | Source-backed bounded profile |
| Transactions | Transactional identifier, participant lists, pending offsets, terminal commit or abort, and timeout | Coordination transaction records and queue metadata | Source-backed bounded control; timeout enforcement remains target work. |
| Cluster metadata | Broker identifier, controller, replicas, in-sync replicas, and leader election | Future distributed coordination | The supported profile is single broker only; multi-broker behavior is unsupported. |

### Completion State And Primitive Placement

| Capability | Kafka facade owns | Depends on | Current state | Next primitive work |
| --- | --- | --- | --- | --- |
| TCP, API versions, Simple Authentication and Security Layer profile, and metadata response | The Kafka facade owns this Kafka wire and session behavior. | Hosted listener and principal authentication | The bounded profile is source-backed. | Expand APIs only through declared compatibility versions. |
| Topics, partitions, record batches, offsets | Kafka names and record semantics | `queue` streams and coordination sequencers | Source-backed single-partition subset | Add multi-partition topic mapping and partition lifecycle |
| Consumer groups, assignment, heartbeat, and rebalance | Kafka group protocol | Coordination generations, leases, and offsets | This behavior is not implemented. | Build the complete single-broker group profile. |
| Idempotent producers and transactions | Kafka producer and transaction protocol | Coordination epochs, sequencing, and transaction records | Source-backed bounded profile | Add timeout enforcement and broader conformance |
| Retention and compacted topics | Kafka topic configuration and visible behavior | Native queue retention and key compaction | This behavior is not implemented. | Implement native queue policy, then map Kafka topic settings. |
| Multi-broker replication and controller behavior | Kafka cluster product behavior | Future consensus-backed coordination | This behavior is outside the current scope. | Start a separate cluster-design program after single-broker conformance. |

### `redis-streams`

Redis Streams is the durable stream family inside the Redis facade. It is not a separate served
surface and it is not the native queue API. A Redis client should still connect to one Redis-like
port and use `X*` commands alongside the other Redis command families. Internally, Redis Streams
should consume native queue storage, retention, and consumer-progress primitives where they match,
while preserving Redis-visible identifiers, field-value entries, pending-entry lists, blocking reads, and
consumer-group responses.

### Redis Streams Profile

Redis Streams should be implemented as a Redis command family over shared primitives, not as a second
queue implementation hidden inside the Redis facade.

| Redis Streams concern | Product-owned behavior | Shared primitive | Target boundary |
| --- | --- | --- | --- |
| Stream identifiers | Millisecond-sequence identifier grammar, `*` allocation, comparison, and response formatting | Queue sequence and optional server timestamp | Deterministic mapping must be stable before `XADD` is advertised |
| Entry shape | Field-value pairs, duplicate field behavior, RESP formatting, and stream-key lifecycle | Queue payload bytes plus Redis substrate metadata | The Redis facade envelope defines this behavior, not native queue payload semantics. |
| Range and length | `XRANGE`, `XREVRANGE`, `XLEN`, count bounds, and identifier bounds | Queue range, length, and stream-identifier index | Requires efficient identifier-to-sequence mapping |
| Blocking reads | `XREAD` blocking timeout and multi-stream response shape | Delivery wait/backpressure and queue reads | No busy-poll loops in the facade |
| Consumer groups | Group creation, pending entries, `XACK`, `XPENDING`, `XCLAIM`, `XAUTOCLAIM`, and last-delivered identifiers | Queue consumer offsets, observed anchors, coordination leases, and coordination claims | Build on shared claim model, not Redis-only locks |
| Trimming | `XTRIM MAXLEN`, `MINID`, approximate trim, and response counts | Queue retention and compaction | Must share retained-gap semantics with queue and delivery |

### Completion State And Primitive Placement

| Capability | Redis Streams facade owns | Depends on | Current state | Next primitive work |
| --- | --- | --- | --- | --- |
| Stream-key lifecycle and Redis stream identifiers | Redis keyspace integration and identifier grammar | Redis substrate and native queue append order | Explicitly unsupported in Redis | Define deterministic identifier mapping over queue sequence and timestamp inputs |
| `XADD`, `XRANGE`, `XREVRANGE`, `XLEN` | Redis command and response shapes | Native queue append/range/len and facade entry envelope | Explicitly unsupported in Redis | Add field-value entry envelope without whole-stream rewrites |
| `XREAD` and blocking reads | Redis blocking-read semantics | Queue reads plus hosted session wait/backpressure | Explicitly unsupported in Redis | Add bounded wait behavior through delivery/backpressure primitives |
| Consumer groups, pending entries, `XACK`, `XPENDING`, `XCLAIM` | Redis group, pending-entry, acknowledgement, and claim semantics | Native offsets, observed anchors, leased work claims, coordination | Explicitly unsupported in Redis | Build pending-entry state over shared claims instead of Redis-only locking |
| `XTRIM` and maxlen/minid policies | Redis trimming grammar and responses | Native queue retention and compaction | Explicitly unsupported in Redis | Specify queue retention first, then map Redis trim behavior |
| Publish-subscribe | This behavior is not part of Redis Streams. | Delivery runtime fanout | This behavior is explicitly unsupported separately. | Keep it separate from durable stream storage. |

### `mqtt`

MQTT is a broad MQTT 5 broker facade. It owns binary packet protocol, client and session identity,
topic filters, retained-message semantics, wills, QoS handshakes, subscription options, shared
subscriptions, user properties, flow control, and MQTT reason codes. It is neither a queue transport
nor a Redis-style publish-subscribe alias.

Quality of service level 0 maps to live at-most-once delivery. Quality of service level 1 and quality
of service level 2 map to durable inflight delivery records, acknowledgement state, coordination
leases, and redelivery. Persistent sessions, session expiry, and subscription state are operational
records. Retained messages are durable latest-value records by topic, with their own MQTT deletion
and expiry semantics rather than append-log retention.

### MQTT Profile

MQTT should be treated as a broker facade over delivery and coordination, with queue storage used only
where durable ordered history is actually required.

| MQTT concern | Product-owned behavior | Shared primitive | Target boundary |
| --- | --- | --- | --- |
| Session and client identity | Client identifiers, clean start, session expiry, keepalive, will delay, and reconnect behavior | Coordination leases, cache-like expiry, and hosted authentication | Operational state outside native queue content |
| Topic filters | MQTT wildcard grammar, subscription options, shared subscriptions, retained-handling flags | Delivery routing and policy | Not native queue collection naming |
| Quality of service level 0 | At-most-once live publish-subscribe | Delivery live fanout | No durable queue append occurs by default. |
| Quality of service level 1 | `PUBACK` state, redelivery with duplicate flag, and inflight windows | Delivery envelopes, acknowledgement state, and backpressure | Durable at-least-once behavior remains within session policy. |
| Quality of service level 2 | `PUBREC`, `PUBREL`, and `PUBCOMP` state machine | Coordination leases and durable delivery state | Implement only with explicit state-machine conformance. |
| Retained messages | Latest retained message by topic, expiry, deletion by empty payload | Retained-message map and policy | Separate from append-log retention |
| Flow control | Receive maximum, packet size, topic aliasing, reason codes | Hosted limits and delivery backpressure | Protocol-shaped errors, not generic queue errors |

### Completion State And Primitive Placement

| Capability | MQTT facade owns | Depends on | Current state | Next primitive work |
| --- | --- | --- | --- | --- |
| MQTT 5 packets, reason codes, topic filters, and user properties | The MQTT facade owns this MQTT protocol behavior. | Hosted TCP, authentication, and policy | This behavior is not implemented. | Build the protocol codec and compatibility profile. |
| Quality of service level 0 live publish and subscribe | The MQTT facade owns session semantics for live publish-subscribe. | Runtime delivery fanout | This behavior is not implemented. | Add live fanout projection. |
| Quality of service level 1 and quality of service level 2 inflight delivery | The MQTT facade owns the acknowledgement state machine. | Delivery envelopes, leased work claims, and coordination | This behavior is not implemented. | Build persistent inflight and redelivery model. |
| Persistent sessions, expiry, subscriptions, and wills | The MQTT facade owns lifecycle and session behavior. | Cache-like expiry, coordination, and delivery | This behavior is not implemented. | Build session and will state with audited lifecycle. |
| Retained messages | The MQTT facade owns latest-message rules. | Durable retained-message map | This behavior is not implemented. | Build retained-message storage and wildcard retrieval. |
| Shared subscriptions and flow control | The MQTT facade owns distribution and capacity behavior. | Leased claims, delivery backpressure, and coordination | This behavior is not implemented. | Build this only after single-consumer QoS conformance exists. |

### `nats`

NATS is one facade with explicit `core` and `jetstream` profiles over a shared NATS protocol, session,
authentication, authorization, subject, wildcard, queue-group, and request/reply model. Core NATS is
live at-most-once subject fanout. JetStream is persisted stream capture, replay, consumer, retention,
and acknowledgement behavior. These guarantees are deliberately different even when clients connect
to the same NATS listener.

### NATS And JetStream Profile

Core NATS and JetStream should share listener/session machinery, but they must not share one storage
semantics claim.

| NATS concern | Product-owned behavior | Shared primitive | Target boundary |
| --- | --- | --- | --- |
| Core subjects | Subject grammar, wildcard matching, inboxes, request/reply, queue groups | Delivery live fanout and hosted sessions | At-most-once, no persistence by default |
| Core no-subscriber behavior | Client-visible no responders and request timeout behavior | Delivery routing metadata | Product grammar, not queue absence |
| JetStream streams | Stream config, subjects, storage policy, max age/bytes/messages, discard policy | Queue retention, compaction, and stream metadata | Persisted stream profile over queue |
| JetStream consumers | Durable and ephemeral consumers, pull/push delivery, ack policy, redelivery, max ack pending | Queue offsets, delivery ack, coordination leases/claims | Shared claim and backpressure primitives |
| JetStream exactly-once | Duplicate windows, acknowledgement floors, and idempotent publish behavior | Coordination sequencers and queue identity | This remains target design work until the at-least-once profile is source-backed. |
| Cluster and leaf nodes | Routes, gateways, super-clusters, quorum, placement | Future distributed coordination | Out of current single-node target |

### Completion State And Primitive Placement

| Capability | NATS facade owns | Depends on | Current state | Next primitive work |
| --- | --- | --- | --- | --- |
| NATS protocol, subjects, wildcards, request-reply behavior, and queue groups | The NATS facade owns this product protocol behavior. | Hosted TCP, principal authentication, and policy | This behavior is not implemented. | Build Core NATS profile and session semantics. |
| Core live fanout and no-subscriber behavior | The NATS facade owns delivery semantics for Core NATS. | Runtime delivery fanout | This behavior is not implemented. | Map at-most-once Core NATS delivery. |
| JetStream stream capture, replay, and retention | The NATS facade owns JetStream API and management semantics. | Native queue retention and compaction | This behavior is not implemented. | Build the JetStream stream profile. |
| JetStream durable consumers, pull or push delivery, acknowledgement, and redelivery | The NATS facade owns JetStream consumer configuration. | Shared leased work claims, delivery, and coordination | This behavior is not implemented. | Build a durable consumer profile with flow control. |
| JetStream exactly-once profile | The NATS facade owns the NATS idempotency and acknowledgement contract. | Queue identity, claims, and coordination | This behavior is not implemented. | Design this only after at-least-once conformance is source-backed. |
| Cluster routes, leaf nodes, and super-clusters | The NATS facade owns distributed server behavior only if a future cluster profile is approved. | Future coordination and deployment model | This behavior is outside the current scope. | Start a separate cluster-design program. |

### `influx`

Influx is a time-series compatibility facade. It owns line protocol, bucket and database naming,
precision handling, HTTP request and error behavior, authentication profile adaptation, and declared
query-language profiles. It maps measurement, tags, fields, and timestamps into canonical structured
time-series points. It must not make InfluxQL, Flux, or a particular Influx server version the native
Loom query language.

The approved direction is explicit profiles: write compatibility first, then InfluxQL, then Flux only
after its pipeline semantics are reconciled with `dataframe` execution. This acknowledges that current
Influx ecosystems use more than one query model without claiming a false single compatibility target.

### Influx Compatibility Profile

The first Influx profile should make existing Influx clients and Telegraf-style writers useful without
turning Loom into an InfluxDB administration clone.

| Application programming interface family | Influx behavior to expose | Loom primitive dependency | Profile boundary |
| --- | --- | --- | --- |
| InfluxDB 2 write | `POST /api/v2/write` with bucket, org where accepted, precision, line-protocol body, and Influx-shaped status/error responses | Structured time-series ingestion, point type validation, request-size limits, auth mapping | Source-backed baseline; expand parser conformance and client transcript coverage |
| InfluxDB 1 write | `POST /write` with `db`, optional `rp`, precision, and line-protocol body | Database/retention-policy mapping to Loom collection and retention policy | Compatibility endpoint only; not a full v1 admin model |
| Influx Query Language query | `/query` with URL-encoded `q`, `db`, optional `rp`, JSON results, and optional CSV output where promoted | Native time-series query abstract syntax tree plus metrics query abstract syntax tree when querying declared metrics collections | This is a target bounded profile; it does not imply full InfluxDB server equivalence. |
| Schema exploration | `SHOW MEASUREMENTS`, `SHOW FIELD KEYS`, `SHOW TAG KEYS`, `SHOW TAG VALUES`, and bounded cardinality forms | Time-series collection catalog, tag/field indexes, cardinality policy | Exact cardinality requires source-backed indexes and query budgeting |
| Retention policy mapping | `db` and `rp` map to a declared Loom collection plus retention class | Native retention and explicit prune policy | Retention policy administration remains Loom-native unless an Influx admin profile is separately approved |
| Flux | Flux query endpoint and pipeline semantics | `dataframe` execution, time-series source adapters, package/function registry | Out of first profile |
| Server administration | users, databases, shards, subscriptions, continuous queries, tasks, backup/restore, and cluster behavior | Management, program, durable delivery, and coordination facets | Out of first profile unless each owner primitive is promoted |

The line protocol profile should be precise:

| Line-protocol concern | Native mapping | Required compatibility rule |
| --- | --- | --- |
| Measurement | Series family or measurement field inside the selected collection | Preserve case-sensitive escaped measurement name and reject names outside the declared profile |
| Tags | Indexed point dimensions | Preserve string keys and values, validate escaping, reject empty tag values, and apply cardinality budgets before write |
| Fields | Typed structured point fields | Support float64, signed int64, unsigned int64, UTF-8 string, and boolean values with Influx-compatible literal handling |
| Timestamp and precision | Canonical timestamp normalized to Loom time precision | Accept explicit timestamps and configured precision; when omitted, use server receive time and mark it as receive-time authored |
| Duplicate point | Same measurement, tag set, and timestamp | Influx-visible result merges field sets and new conflicts win; Loom history still records the revision that introduced the replacement |
| Request batching | Multiple line protocol points in one HTTP request | Validate per-point and per-request limits; report compatible partial or whole-request failure according to the selected profile |

The first InfluxQL profile should include read-only exploration and aggregation:

| InfluxQL area | Include in first profile | Defer or reject until supported |
| --- | --- | --- |
| Selection | `SELECT` fields, aliases, simple arithmetic, and bounded regular expressions over measurements, tag keys, and string field values | `SELECT INTO`, multi-statement mutation, and unrestricted regex scans |
| Filtering | `WHERE` over time, tags, fields, comparisons, boolean operators, and bounded regular expressions | Timezone or predicate behavior that cannot be lowered into the native query abstract syntax tree |
| Grouping and windows | `GROUP BY` tags, `GROUP BY time(...)`, `fill(null)`, `fill(none)`, `fill(previous)`, and scalar fill when safe | Continuous queries and background materialization |
| Ordering and limits | `ORDER BY time`, `LIMIT`, `OFFSET`, `SLIMIT`, and `SOFFSET` | Unbounded result sets or product-specific shard-order behavior |
| Functions | `count`, `sum`, `mean`, `min`, `max`, `first`, `last`, `spread`, `derivative`, `non_negative_derivative`, `elapsed`, and percentile only after deterministic semantics are pinned | Technical analysis and advanced functions until native query semantics exist |
| Metadata | `SHOW` statements for measurements, field keys, tag keys, tag values, and cardinality where indexes support them | User, shard, subscription, and cluster introspection |

### Completion State And Primitive Placement

| Capability | Influx facade owns | Depends on | Current state | Next primitive work |
| --- | --- | --- | --- | --- |
| Line protocol, precision, and bucket or database selection | The Influx facade owns this Influx protocol behavior. | Structured `time-series` point ingestion | This behavior is source-backed. | Expand parser conformance, duplicate-point semantics, and client validation. |
| Point storage, tags, fields, and rollups | The Influx facade should not own native point storage. | The `time-series` facet owns structured point storage. | Structured point mapping is source-backed. | Add native query, tag, field, and cardinality indexes. |
| Influx Query Language | The Influx facade owns query grammar and response compatibility. | Native time-series and metrics query abstract syntax trees | The target profile is defined. | Build parser, abstract syntax tree lowering, bounded execution, and JSON and CSV responses. |
| Flux | The Influx facade owns Flux pipeline grammar and response compatibility if promoted. | `dataframe` execution plus time-series source adapters | This behavior is not implemented. | Analyze and build it only after the dataframe boundary is source-backed. |
| Server administration, task scheduling, and cluster behavior | The Influx facade should not imply full InfluxDB server administration. | Management, program, and future coordination contracts | This behavior is outside the current scope. | Do not imply Influx server equivalence. |

### `prometheus`

Prometheus is a metrics compatibility facade. It owns Prometheus label and sample conventions,
staleness, remote-write and optional remote-read protocol profiles, Prometheus Hypertext Transfer
Protocol API responses, PromQL compatibility,
metadata exposure, and client-facing capability reporting. `metrics` owns the
semantic model and `time-series` supplies storage and rollups.

Remote write is a stateless HTTP through which a Prometheus server sends batches of labeled
samples to remote storage. Remote read is the inverse remote-storage protocol: a Prometheus server
requests raw label-matched samples for a time range, then evaluates PromQL itself. Neither protocol
alone makes Loom a Prometheus query server. A direct Grafana or API
client needs the Prometheus HTTP query API and a
declared PromQL profile.

The approved near-term target is a Prometheus API profile: remote write,
a bounded PromQL profile, query and query-range, label and metadata discovery,
staleness, histogram and exemplar support, and
explicit capability reporting. Full Prometheus-server behavior is a later analysis item, not an
implicit commitment: scraping, service discovery, recording and alerting rules, federation, WAL
management, and server orchestration are an operational product that could compose `metrics`,
`program`, delivery, and coordination without changing the metrics contract.

### Completion State And Primitive Placement

| Capability | Prometheus facade owns | Depends on | Current state | Next primitive work |
| --- | --- | --- | --- | --- |
| Remote write encoding, headers, retries, and response behavior | The Prometheus facade owns this remote-write protocol behavior. | Hosted HTTP and `metrics` ingestion | This behavior is partially source-backed. | Conform to the declared remote-write version and stale-marker rules. |
| Labels, samples, metric families, and staleness | The Prometheus facade owns Prometheus compatibility semantics. | `metrics` descriptor, cardinality, and sample model | Labels and simple samples are source-backed. | Build the metric semantic contract and staleness behavior. |
| PromQL and HTTP query API | The Prometheus facade owns PromQL grammar and Prometheus result shapes. | Metrics query abstract syntax tree and query planner | Exact selector query and query-range behavior are source-backed. | Build the declared PromQL profile. |
| Remote read | The Prometheus facade owns remote-storage request and chunk compatibility if promoted. | Metrics raw-series query | This behavior is not implemented. | Add it only as an optional remote-storage profile after PromQL API direction is established. |
| Histograms, exemplars, and metadata | The Prometheus facade owns Prometheus exposition and API behavior. | Metric distributions and trace correlation | This behavior is not implemented. | Build `metrics` and `traces` primitives. |
| Scraping, discovery, rules, alerts, federation, and write-ahead log management | The Prometheus facade owns full Prometheus server product behavior only if that product profile is approved. | Program, delivery, management, and coordination | This behavior is not implemented. | Perform separate full-server analysis before any implementation claim. |

### `grafana`

Grafana is a composite visualization and datasource facade. It does not own time-series storage and
is not defined by a generic server protocol comparable to Prometheus remote write. Its durable target
is a maintained `loom-grafana-datasource` integration with a query model, Grafana DataFrames, health
checks, configuration, variables, annotations, capability metadata, and declared facet targets.

The datasource is workspace-scoped. Each query explicitly selects an allowed Loom target such as
metrics, logs, traces, structured query, dataframe, FTS, or a supported native facet. The existing collection-bound
HTTP health, search, and query adapter is transitional compatibility only. It may remain as a backend endpoint
for the plugin, but it is not the primary long-term public contract.

### Grafana Datasource Profile

The target is a maintained `loom-grafana-datasource` plugin backed by hosted Loom APIs. The plugin
should make Grafana a client of Loom data, not a place where Loom stores dashboard truth or rewrites
native facet semantics.

| Datasource concern | Target behavior | Loom primitive dependency | Boundary |
| --- | --- | --- | --- |
| Plugin configuration | Store URL, workspace selector, authentication mode, optional default namespace, allowed target set, default limits, and feature flags | Hosted listener registry, principal authentication, PEP, and capability reporting | Grafana datasource configuration is not Loom store configuration |
| Health check | Validate listener reachability, auth, workspace access, and advertised capability profile | Hosted health/capabilities endpoint | Must not imply every facet target is queryable |
| Query target model | Each query target declares facet, collection or selector, query language/profile, projection, interval, max rows, and output frame preference | Native query ASTs and facade profile adapters | Query target grammar is plugin-owned, native semantics stay facet-owned |
| DataFrame shaping | Return valid Grafana DataFrames with same-length typed fields, time and value conventions, labels, units, and wide or long choices | Metrics, time-series, structured-query, dataframe, logs, traces, and FTS query outputs | Grafana frames are result projections, not canonical storage |
| Multi-query behavior | Execute multiple targets under shared time range, scoped variables, limits, and partial error reporting | Query planner, hosted request sessions, stable errors | Do not hide per-target authorization or capability failures |
| Variables | Support query variables for metric names, label values, collections, structured-query schemas, dataframe datasets, log attributes, trace services, and FTS indexes where available | Facet catalogs and metadata indexes | Variable queries must be budgeted because Grafana can refresh them often |
| Annotations | Support time-bounded annotation queries from ledger, logs, events, program executions, protected refs, or explicit annotation collections | Ledger/log/event/query primitives | Annotations are read projections, not a new annotation store unless one is designed |
| Explore support | Offer logs, traces, metrics, and search targets with compatible frame shapes and links where native facets support them | Metrics, logs, traces, FTS, and cross-signal references | Explore features are capability-gated by target |
| Alerting metadata | Advertise whether a query is alert-compatible, deterministic, and bounded | Native query contracts and stable error mapping | Grafana alert-rule lifecycle remains Grafana-owned |
| Capability metadata | Return plugin-visible endpoint, target, function, frame, variable, annotation, limit, and version data | Hosted capability reporting | Required before broad external documentation |

The first plugin target set should be metrics, time-series, structured query, dataframe, and full-text
search because those have
the clearest existing Grafana user workflows. Logs and traces should be designed in the same plugin
shape, but they should wait for native `logs` and `traces` primitive definitions.

### Completion State And Primitive Placement

| Capability | Grafana facade owns | Depends on | Current state | Next primitive work |
| --- | --- | --- | --- | --- |
| Datasource configuration, health, and query model | The Grafana facade owns datasource configuration, health checks, and the Grafana query model. | Hosted authentication, workspace binding, and capability reporting | A narrow HTTP health and query adapter is source-backed. | Build the maintained datasource plugin and capability endpoint. |
| DataFrames and visualization result shaping | The Grafana facade owns Grafana DataFrame and visualization result shaping. | Target facet query outputs | The current adapter returns narrow datapoints. | Implement frame conversion, labels, units, wide and long choices, and multi-query behavior. |
| Metric, time-series, structured-query, dataframe, and FTS targets | The Grafana facade owns target selection and editor behavior. | Owning facet query contracts | The current adapter is time-series only and collection-bound. | Add declared target registry, query models, and policy controls. |
| Logs and traces targets | The Grafana facade owns target selection, Explore shaping, and cross-signal links. | Native `logs`, `traces`, exemplars, and telemetry context | Native logs and traces facets are planned. | Add this after logs and traces primitive definitions. |
| Variables, annotations, and alerting capability metadata | The Grafana facade owns variables, annotations, and alerting capability metadata. | Queryable facets, metadata indexes, ledger, log, event sources, and stable query contracts | This behavior is not implemented. | Implement variable queries, annotation queries, and alert compatibility metadata. |
| Dashboard persistence and Grafana server administration | The Grafana facade should not own dashboard persistence or Grafana server administration. | Grafana itself or a separately designed management integration | This behavior is outside the current scope. | Do not make Loom a Grafana server. |

### `otlp`

OTLP is a composite telemetry facade, not a metrics-only or time-series-only
protocol. It owns OTLP protobuf and JSON encoding,
HTTP and gRPC export behavior, compression,
partial-success responses, resource and scope normalization, and signal-specific ingest validation.
Native `metrics`, `logs`, and `traces` own the stored signal contracts. A shared telemetry context
model must preserve resource, scope, attributes, and cross-signal correlation without flattening all
signals into one storage model.

### OTLP Compatibility Profile

The target OTLP receiver should be a multi-signal ingest facade over native
telemetry facets. It should not create a separate OTLP store, and it should not
require clients to know Loom-native record shapes.

| OTLP concern | Target behavior | Native dependency | Boundary |
| --- | --- | --- | --- |
| HTTP protobuf | Accept protobuf requests on `/v1/metrics`, `/v1/logs`, and `/v1/traces` with OTLP response envelopes | Hosted HTTP, protobuf schemas, metrics, logs, and traces | Preferred first full transport profile |
| HTTP JSON | Accept OTLP JSON on the same per-signal paths where advertised | JSON mapping and signal validators | Useful for debugging and constrained clients, but not the only profile |
| gRPC | Accept unary export services for metrics, logs, and traces over HTTP version 2 | Hosted gRPC substrate and protobuf schemas | Build after HTTP protobuf if shared gRPC runtime is ready |
| Compression | Support uncompressed and gzip payloads | Hosted request-size limits and decompression budget | Reject unsupported compression explicitly |
| Headers and authentication | Map OTLP headers into hosted authentication and principal policy | Principal authentication, API keys, PEP, and audit | Do not treat arbitrary OTLP headers as trusted resource identity |
| Partial success | Return signal-specific accepted/rejected counts and error messages without hiding policy failures | Per-signal validation, write batching, stable errors | No partial mutation without clear accepted/rejected accounting |
| Resource and scope | Normalize resource and instrumentation scope once, then reference them from metrics/logs/traces | Shared telemetry context catalog | Avoid duplicate per-signal resource records with divergent identity |
| Backpressure and retry | Return retryable versus non-retryable errors consistently with OTLP expectations | Hosted rate limits, durability, stable error mapping | The server does not implement client retry loops. |
| Capability reporting | Advertise supported signals, encodings, transports, compression, limits, semantic versions, and experimental profiles | Hosted capability reporting | Required before public docs claim OTLP support |
| Profiles signal | Treat OTLP profiles as separate future domain work | Future profile facet or explicit rejection | Do not accept profiles as generic blobs. |

The first fully useful OTLP milestone should be HTTP
protobuf for metrics, logs, and traces, plus gzip, authentication, partial-success accounting, and
capability reporting. HTTP JSON and Google Remote
Procedure Call should follow as transport profiles over the same signal validators and native write
path.

### Completion State And Primitive Placement

| Capability | OTLP facade owns | Depends on | Current state | Next primitive work |
| --- | --- | --- | --- | --- |
| HTTP JSON metrics export | The OTLP facade owns this export behavior. | Structured time-series ingestion | Gauge and sum export is source-backed. | Migrate the mapping to the native `metrics` contract. |
| HTTP protobuf metrics, logs, and traces export | The OTLP facade owns this export behavior. | Shared hosted HTTP, protobuf schemas, signal validators, and native facets | This behavior is not implemented. | Build the first full multi-signal OTLP receiver profile. |
| gRPC metrics, logs, and traces export | The OTLP facade owns this export behavior. | Hosted gRPC substrate and protobuf schemas | This behavior is not implemented. | Add it after the HTTP protobuf path shares validators. |
| Metric types, resources, scopes, and exemplars | The OTLP facade owns mapping and validation. | `metrics`, `logs`, and `traces` shared telemetry context | Only gauges and sums are currently supported. | Build metrics semantic model and trace links. |
| Logs export | The OTLP facade owns log encoding and partial-success behavior. | `logs` | The native primitive profile is defined. | Build the logs facet and OTLP log mapping. |
| Traces export | The OTLP facade owns trace encoding and partial-success behavior. | `traces` | The native primitive profile is defined. | Build the traces facet and OTLP trace mapping. |
| Compression, headers, authentication, backpressure, partial success, and capability reporting | The OTLP facade owns protocol-specific mapping for these behaviors. | Hosted substrate, principal authentication, limits, audit, and stable errors | Partial hosted primitives exist. | Add conformance vectors across all signals. |
| Profiles export | The OTLP facade owns profile encoding only if a profile domain is approved. | Future profile domain decision | This behavior is not implemented. | Defer until profiles stabilize and a native use case exists. |

## `vector`

### Native

The native vector facet owns versioned vector collections, fixed dimension and metric, caller identifiers,
metadata, exact nearest-neighbor search, metadata filters, maintained equality indexes, source-text
provenance, embedding model profile metadata, and explicit acceleration policy. Exact scoring and
deterministic identifier tie-breaking are the portable contract. Approximate structures are rebuildable
derived artifacts that may reduce recall but never redefine stored vectors or returned score values.

### Presentation Candidates

Qdrant and Pinecone are explicit vector compatibility facades. A hosted vector listener must select
its facade rather than silently defaulting to one product model. Qdrant Representational State
Transfer and unary gRPC, plus Pinecone REST, have
source-backed bounded profiles. `generic` remains the Loom-native vector profile. pgvector is a
PostgreSQL structured-query presentation over native vector collections. Milvus and Weaviate require
separate product-profile decisions rather than being implied by vector storage.

### Primitive Gaps

| Primitive | Native vector role | Engine or presentation role | Current boundary |
| --- | --- | --- | --- |
| Collection identity and immutable shape | The native vector facet owns collection name, fixed dimension, distance metric, and versioned manifest. | Qdrant and Pinecone map collection or index administration to this native shape. | This primitive is source-backed. |
| Vector entry and caller identifier | The native vector facet owns fixed-width float32 values, stable caller-supplied identifiers, and metadata maps. | Compatibility facades shape point and vector request objects. | Structured storage is source-backed. |
| Exact nearest-neighbor search | The native vector facet owns metric evaluation, pre-score filtering, descending score order, and identifier tie-break order. | Qdrant and Pinecone adapt result envelopes to this exact contract. | The cross-platform contract is source-backed. |
| Metadata filter abstract syntax tree | The native vector facet owns all, equality, inequality, range, membership, existence, and boolean semantics with type-strict comparison. | Qdrant JSON, Qdrant protobuf, and Pinecone filters translate to the native filter model. | This primitive is source-backed. |
| Metadata equality index | The native vector facet owns declared indexed keys, marker maintenance, equality narrowing, set narrowing, conjunction narrowing, validation, and versioning. | Clients request indexes through native or future administration surfaces. | This primitive is source-backed; broader index types remain target work. |
| Structural version-control merge | The native vector facet owns per-identifier entry structure, same-identifier three-way vector and metadata merge, and conflict behavior. | Facades expose the resulting version state. | This primitive is source-backed. |
| Source text and embedding profile | The native vector facet owns optional text sidecar data, model identifier, dimension, weights digest provenance, and source-aware reads. | Inference providers or runtimes embed text; Qdrant and Pinecone integrated embedding endpoints are product behavior. | Local provenance is source-backed; concrete hosted providers remain target work. |
| Embedding inference | The native vector facet should not own a model runtime or provider credentials. | A shared inference or provider facade turns source text into vectors. | Local configured inference flow is source-backed; hosted provider routes remain target work. |
| Explicit accelerator policy | The native vector facet owns exact-always and approximate-above threshold policy, capability reporting, and exact re-score requirements. | Product quantization and hierarchical navigable small world engines implement accelerator candidates. | Local policy is source-backed; hosted compatibility profiles report exact-only behavior today. |
| Product quantization accelerator | The native vector facet owns the derived-artifact lifecycle seam and policy contract. | The pure Rust product-quantization engine copies vectors and metadata and re-scores candidates exactly. | This optional accelerator is source-backed. |
| Hierarchical navigable small world accelerator | The native vector facet owns the derived-artifact lifecycle seam and exact output contract. | The native-only `loom-hnsw` engine builds a non-deterministic graph, filters candidates, and re-scores candidates exactly. | This optional accelerator is source-backed; candidate recall may differ. |
| Derived accelerator artifact | The native vector facet owns source, engine, and format stamps, identity exclusion, stale detection, and rebuild authority. | The store keeps local payloads and engines produce those payloads. | The durable-local store contract is source-backed; rebuild scheduling remains target work. |
| Recall-target policy | Needs measured recall corpus, quality target, fallback behavior, and operational reporting | PQ/HNSW tune candidates against it | Target native policy |
| Vector API key authentication | The native vector facet should not own credential secrets or principal lifecycle. | Shared application-password and API key security authenticates requests before hosted policy enforcement; Qdrant and Pinecone map headers. | Hosted vendor-style metadata exists; the full principal API key contract is shared security work. |
| Dense, sparse, and hybrid retrieval | This requires sparse representation, lexical scoring, fusion, reranking, and a clear FTS boundary. | Qdrant and Pinecone request options expose admitted profiles. | Dense float32 vectors are source-backed; sparse and integrated embedding requests are explicitly unsupported. |

### Required Separations

| Concern | Native ownership | Engine or presentation ownership | Reason |
| --- | --- | --- | --- |
| Exact vector contract versus ANN graph | Vectors, metric, metadata filters, exact scores, deterministic output ordering | PQ codes, HNSW graph, candidate beam, graph layout, and rebuild timing | A non-deterministic approximate graph cannot become versioned source of truth |
| Native vector identifier versus vendor collection path | Loom entry identity and collection namespace | Qdrant point/collection and Pinecone index/workspace route shapes | One native collection can support deliberately selected facade mappings |
| Metadata filter abstract syntax tree versus vendor filter JSON or protobuf | Stable predicate meaning, type rules, and index selection | Qdrant and Pinecone request syntax and error envelopes | Vendor request syntax should not define native query semantics. |
| Source-text provenance versus embedding provider | Source text, model profile evidence, and stored vector relationship | Endpoint, credential, batching, model execution, and provider responses | The vector facet must remain useful with precomputed vectors and replaceable inference |
| API key authentication versus vector authorization | Collection scope and PEP authorization | Shared principal API key validation and Qdrant or Pinecone header compatibility | Credentials are security primitives, not vector metadata. |
| Dense vectors versus lexical or sparse retrieval | Dense metric search and metadata filtering | FTS, sparse vector, hybrid fusion, and vendor-specific integrated embeddings | Avoids silently turning vector storage into a duplicate search facet. |

### Completion State And Primitive Placement

| Capability group | Native `vector` | Engine or presentation placement | Design state |
| --- | --- | --- | --- |
| Versioned collections, fixed dimensions and metrics, entry create/read/update/delete, exact search, filter abstract syntax tree, equality indexes, source text, model profile, merge, diff, ACL, and local projections | The native vector facet owns these reusable vector semantics. | Command-line commands, language bindings, MCP tools, native REST endpoints, and JSON-RPC endpoints adapt this contract. | This baseline is source-backed. |
| Explicit exact and approximate threshold policy, product quantization, hierarchical navigable small world indexing, exact re-scoring, derived artifact stamps, and stale detection | The native vector facet owns the acceleration policy contract. | Optional `loom-vector` product-quantization and native-only `loom-hnsw` engines implement the policy. | Local behavior is source-backed; recall targets and rebuild scheduling remain target work. |
| Qdrant REST and unary gRPC collection, point, query, scroll, count, and filter profile | The native vector facet should not own vendor protocol grammar. | The explicit Qdrant facade owns this compatibility behavior. | The bounded compatibility profile is source-backed. |
| Pinecone REST index, vector, workspace, and filter profile | The native vector facet should not own vendor protocol grammar. | The explicit Pinecone facade owns this compatibility behavior. | The bounded exact-only compatibility profile is source-backed. |
| Generic Loom native vector listener | The native vector facet owns the native API contract. | The explicit `generic` facade profile exposes that contract over hosted routes. | The design direction is approved; hosted native REST and JSON-RPC subsets are source-backed. |
| Hosted native delete, identifier, index, source, model, native gRPC, hosted acceleration policy, recall targets, external-client transcripts, sparse or hybrid vectors, and concrete hosted providers | The native vector facet owns reusable vector and inference semantics once specified. | Native, Qdrant, Pinecone, and future facade profiles consume these semantics. | This remains target work. |
| Milvus, Weaviate, and pgvector compatibility | The native vector facet should not own those client grammars. | Separate facade or structured-query presentation decisions own them. | Current vector work does not imply this compatibility. |

## `graph`

### Native

The native graph facet owns a versioned property graph: stable node and edge identities, directed
edge endpoints and labels, typed property values, traversal, reachability, path semantics, graph
mutation, and authorization. The current source is deterministic and stages each graph through a
structured `Tree` root with independently addressable node, edge, forward-adjacency,
reverse-adjacency, metadata, and declared property-index catalog components. The approved storage root
uses an existing `Tree` target with component `ProllyMap` entries rather than a new object kind. Node
and edge maps keep string identifiers as their simple keys. Adjacency maps use length-prefixed compound byte
keys so source, destination, label, and edge identifier ordering is canonical without separator ambiguity.

### Presentation Candidates

The approved first declarative grammar is a bounded GQL-aligned openCypher profile. The roadmap is
deliberately broad enough to include `MERGE`, all-simple-path semantics, regex, list/map values,
scalar and path functions, full-text integration, and geospatial posture as tracked work rather than
burying them. Bolt follows only after that query contract, session model, result framing, error
mapping, and client conformance are defined. This now means a first-class target `neo4j` surface,
not a `graph` transport. Gremlin is cut from active scope. Loom must not let a product protocol define
canonical graph identity.

### Primitive Gaps

| Primitive | Native graph role | Presentation role | Current boundary |
| --- | --- | --- | --- |
| Node and edge identity | The native graph facet owns caller-supplied identifiers, directed endpoints, labels, endpoint integrity, and deletion policy. | GQL/openCypher patterns and Neo4j compatibility bind variables to native identifiers. | Structured keys are source-backed. |
| Typed property model | The native graph facet owns canonical typed scalar, list, and map values, null and cardinality rules, schema metadata, and property update semantics. | Query grammars map literals and properties to the native model. | Scalar, list, and map values are source-backed. |
| Canonical node labels | The native graph facet owns zero-or-more labels per node with deterministic encoding and merge behavior. | GQL/openCypher label patterns and Neo4j compatibility consume this model. | This is source-backed in core graph storage and native query intermediate representation. |
| Structured graph root | The native graph facet owns canonical root metadata plus node, edge, forward adjacency, reverse adjacency, and property-index catalog roots. | All query and traversal paths consume this structure. | This is source-backed as a `Tree` root with component `ProllyMap` entries. |
| Forward and reverse adjacency | The native graph facet owns deterministic neighbor and directed-edge access with bounded expansion. | Pattern matching and traversal steps use this access path. | Component roots are source-backed. |
| Property indexes | The native graph facet owns declaration, canonical keys, maintenance, rebuild, readiness, and query selection for property indexes. | GQL/openCypher and Neo4j compatibility benefit from these indexes. | Canonical declarations, transient derived materialization, readiness/stale reporting, explain output, ready node-property equality selection, and durable local `property-index:<index-name>` derived-artifact records are source-backed. Public projection remains target work. |
| Native graph query and traversal intermediate representation | The native graph facet owns typed pattern, filter, projection, aggregation, mutation, cursor, result, path, and resource-bound semantics. | GQL/openCypher and Neo4j compatibility lower to this intermediate representation. | Scan-based node and directed-edge patterns, ready node-property equality index selection, fixed chained paths, bounded variable-length all-simple paths, bounded `shortestPath`, path values, path `length`, identifier function, type function, endpoint functions, list/map-returning functions, comparison and regex predicates, projections, ordering, skip, grouped count, row limits, node, edge, path, scalar, list, and map results, shared canonical CBOR query/explain result encoding, local CLI/MCP/Node/Python/WASM projection, generated remote protocol and CLI remote facade projection, hosted REST/JSON-RPC projection, and atomic mutation plans including `MERGE` are source-backed; broader behavior remains target work. |
| Deterministic traversal baseline | The native graph facet owns neighbors, inbound and outbound edges, reachability, and shortest path with bounded ordering. | Native REST, JSON-RPC, language bindings, and MCP tools expose it. | This behavior is source-backed. |
| Mutation and merge policy | Needs atomic node/edge/property operations, constraints, node/edge-level diff, and three-way merge behavior | Query facades expose declared mutation subset | Source-backed native mutation plan includes identity-envelope `MERGE`; node/edge-level three-way merge remains target |
| `MERGE` identity and uniqueness | The native graph facet owns match-or-create atomicity over explicit identity-envelope uniqueness and deterministic conflict errors. | openCypher `MERGE` lowers to this native behavior. | Source-backed for node and directed-edge patterns; broader constraint catalogs remain target work. |
| Path semantics | The native graph facet owns fixed paths, bounded variable paths, all-simple-path expansion, bounded shortest path selection, path values, deterministic ordering, and path-length projection. | GQL/openCypher path syntax and Neo4j compatibility consume this behavior. | Source-backed for fixed chained paths, bounded variable-length all-simple paths, bounded `shortestPath`, `length(path)`, and hop, fanout, candidate, row, and byte budgets. Native shortest-path traversal is source-backed through `shortest_path`. |
| Predicate and function registry | The native graph facet should own closed deterministic scalar, regular-expression, list, map, and path function semantics. | Text profiles map syntax into the registry. | Deterministic non-backtracking regex predicates, identifier function, type function, start-node function, and end-node function are source-backed; list/map values and broader functions remain target work. |
| Full-text integration | The native graph facet owns cross-facet projection and result semantics, not a second graph text index. | GQL and openCypher full-text predicates consume full-text-search-derived artifacts. | The first core bridge is source-backed through prepared FTS hit-id projections; query text grammar and public client projection remain target work. |
| Geospatial graph values | The native graph facet owns canonical Loom typed geometry values, closed CRS profiles, distance semantics, and spatial-index readiness. | Query profiles map geospatial predicates into native semantics. WKT, WKB, Extended WKB, GeoJSON, and SQL-like `ST_*` names are interchange or compatibility encodings, not source truth. | Canonical point values, CRS validation, canonical CBOR tagging, reserved-tag collision rejection, shared value-codec projections, core point distance/bbox predicates, canonical spatial-index declarations, readiness reporting, ready node/edge spatial candidate planning, and durable local `spatial-index:<index-name>` derived-artifact records are source-backed with conformance coverage. Line/polygon geometry families remain target work. |
| Query safety policy | Needs depth, fanout, candidate, row, byte, time, and memory limits | Parser/session surfaces enforce compatible request limits | Target native and hosted control primitive |
| Bounded GQL and openCypher adapter | The adapter should not own stored graph identity. | It parses declared text grammar into native intermediate representation. | Expanded read profile, regex predicates, fixed chained paths, bounded variable-length all-simple paths, bounded `shortestPath`, path values, path length function, identifier function, type function, start-node function, end-node function, and mutation text lowering through explicit Loom identity are source-backed; broader conformance remains target work. |
| Neo4j compatibility surface | The native graph facet should not own Neo4j product session grammar. | The `neo4j` first-class served surface owns Bolt handshake, sessions, records, transactions where supported, driver expectations, catalog/procedure compatibility shims, errors, and official-driver transcript conformance. | Compatibility matrix, capability rows, durable listener admission, daemon runtime opening, bounded Bolt 5.1 read `RUN` plus `PULL` execution, auto-commit `CREATE`/`MERGE` write `RUN`, and guarded official Python and JavaScript driver transcripts are source-backed. `SET`/`REMOVE`/`DELETE`, explicit transactions, catalog/procedure shims, and broader driver conformance remain target. |
| Gremlin facade | Gremlin is cut from the active graph roadmap. | Reopening requires a separate owner-approved design session for client value, execution boundary, bytecode/text subset, step whitelist, resource policy, and conformance strategy. | Cut from active Queue 2 scope. |

### Required Separations

| Concern | Native ownership | Presentation ownership | Reason |
| --- | --- | --- | --- |
| Canonical graph components versus query-engine side store | Node roots, edge roots, adjacency roots, index roots, identity, version-control visibility, and merge rules | Query planning caches and engine artifacts | A side store invisible to commit, diff, merge, and sync cannot become graph truth |
| Native graph intermediate representation versus text grammar | Patterns, filters, mutation, projection, resource bounds, and stable errors | GQL/openCypher parsing and Neo4j Cypher compatibility parsing | Several client-facing profiles can share one graph contract without translating between unrelated languages |
| Native graph semantics versus Neo4j product behavior | Graph semantics and common intermediate representation | Neo4j Bolt sessions, records, transactions, catalog/procedure shims, and driver expectations | Neo4j compatibility is a product surface over Loom graph, not the source identity of the graph |
| Graph mutation versus protocol transaction | Atomic native mutation plan and authorization | Neo4j transaction/session behavior and vendor error responses | Protocol transaction expectations must be explicit, not implied by local writes |
| Property indexes versus query dialect | Index declarations, maintenance, readiness, and selection policy | Grammar predicates and explain/profile result shapes | Index behavior must work for native, bindings, and all future facades |

### Completion State And Primitive Placement

| Capability group | Native `graph` | Presentation placement | Design state |
| --- | --- | --- | --- |
| Deterministic nodes, directed edges, typed scalar property bags, endpoint integrity, create/read/update/delete, neighbors, inbound and outbound edges, reachability, shortest path, ACL, structured graph roots, component reachability, structural sharing, and graph versioning | The native graph facet owns these graph semantics. | Command-line commands, language bindings, MCP tools, native REST endpoints, and JSON-RPC endpoints adapt them. | The current baseline is source-backed. |
| Canonical node labels | The native graph facet owns canonical node labels. | Query and client facades consume the labels. | This behavior is source-backed in core graph storage and native query intermediate representation. |
| Property indexes | The native graph facet owns declaration, canonical keys, transient rebuildable materialization, readiness/stale reporting, explain output, and query selection. | Query and client facades consume them. | The core subset is source-backed for ready node-property equality selection, and durable local property-index records now share the derived-artifact lifecycle. Public projection remains target work. |
| Node and edge diffs and semantic merge | The native graph facet should own these correctness semantics. | Query and client facades consume them. | This remains target correctness-promotion work. |
| Native graph intermediate-representation baseline | The native graph facet owns query semantics. | GQL and openCypher parser and result grammar consume it later. | Scan-based node and directed-edge patterns, ready node-property equality index selection, fixed chained paths, bounded variable-length all-simple paths, bounded `shortestPath`, comparison and regex predicates, identifier function, type function, endpoint functions, label, key, property, node-list, and relationship-list functions, projections, ordering, skip, grouped count, row limits, typed node, edge, path, scalar, list, and map results, shared canonical CBOR query/explain result encoding, local CLI/MCP/Node/Python/WASM projection, hosted REST/JSON-RPC projection, and native mutation plans including `MERGE` are source-backed. |
| Bounded GQL-aligned openCypher profile | The native graph facet owns lowered semantics through native intermediate representation. | Parser and text result grammar adapt those semantics. | Expanded read profile, regex predicates, fixed chained paths, bounded variable-length all-simple paths, bounded `shortestPath`, path length function, identifier function, type function, endpoint functions, list/map-returning functions, mutation text lowering through explicit Loom identity including `MERGE`, and deterministic mutation identity derivation for bounded Neo4j `CREATE`/`MERGE` are source-backed; broader conformance remains target work. |
| Broader scalar functions, text-grammar full-text-search predicates, geospatial predicates, public explain/readiness projection, and resource governance | The native graph facet owns the stable semantics and performance boundaries. | Query profiles consume these semantics. | Canonical list/map values, regex predicates, first scalar/path/list/map functions, core property-index explain/readiness, FTS hit-id projection predicates, canonical point geometry values, point distance/bbox predicates, node/edge spatial-index candidate planning, and graph index derived-artifact records are source-backed. Line/polygon geometry families remain target work. |
| Neo4j compatibility profile | The native graph facet should not own Bolt session grammar. | The first-class `neo4j` served surface owns that behavior. | Compatibility matrix, capability rows, durable listener admission, daemon runtime opening, bounded Bolt 5.1 read `RUN` plus `PULL` execution, auto-commit `CREATE`/`MERGE` write `RUN` execution over deterministic Loom graph identity, and official Python/JavaScript driver transcript certification are source-backed. |
| Gremlin text and bytecode profile | The native graph facet should not own a second graph language or storage model. | Gremlin is cut from active scope. | Reopen only through a separate owner-approved design session. |
| Full Neo4j product behavior, arbitrary Gremlin execution, graph cluster administration, and engine-private stores | The native graph facet should not own these product or engine behaviors. | Full Neo4j is not implied by `neo4j/tcp`; Gremlin is not active scope. | This is explicitly outside the current direction. |

## `columnar`

### Native

The native columnar facet owns durable versioned analytical datasets. Its canonical source is a
typed schema plus an ordered segment manifest with segment statistics, compression policy, target
segment size, and profile-aware digests. It is append-oriented and preserves logical row order.
Apache Arrow IPC and Apache Parquet are derived import/export projections, not the canonical identity format.

### Presentation Candidates

Apache Arrow IPC and Apache Parquet are interchange and file-format projections. Arrow Flight and Arrow Flight SQL are
binary data-plane presentations. DuckDB-like, warehouse-like, Spark-like, and BigQuery-like client
experiences are analytical presentations over columnar storage, dataframe execution, and SQL where
appropriate. They must not redefine the committed segment contract.

### Primitive Gaps

| Primitive | Native columnar role | Engine or presentation role | Current boundary |
| --- | --- | --- | --- |
| Dataset schema and typed rows | The native columnar facet owns named `ColumnType` schema, null rules, row arity validation, type validation, and canonical values. | Arrow and Parquet map supported values and carry Loom metadata for extended values. | This primitive is source-backed. |
| Ordered segment manifest | The native columnar facet owns target rows per segment, ordinal, row start, row count, encoding, policy, digest, and committed statistics. | Storage engines may read or accelerate segments. | This canonical primitive is source-backed. |
| Append and scan | The native columnar facet owns append order and deterministic scan results. | Structured-query, dataframe, and analytical clients consume rows or batches. | The portable facade is source-backed. |
| Segment compaction | The native columnar facet owns explicit compaction, logical-row preservation, and identity changes caused by layout changes. | Operators invoke compaction through command-line or administration surfaces. | This is source-backed; there is no background rewrite policy. |
| Segment statistics | The native columnar facet owns deterministic inspectable statistics tied to committed segments. | Query engines use statistics for pruning after a contract is specified. | Metadata is source-backed; predicate pushdown remains target work. |
| Portable select and aggregate | The native columnar facet owns declared projection, filter, and aggregate semantics with an executor seam. | A native accelerator may execute the same contract. | The portable contract is source-backed. |
| Native executor seam | The native columnar facet owns the `ColumnarExecutor` contract and result semantics. | A native executor accelerates scans and aggregates without changing canonical storage. | The seam is source-backed; no native online analytical processing executor is promoted. |
| Source digest and derived artifacts | The native columnar facet owns the digest of committed columnar source for acceleration and materialization stamps. | Engines cache rebuildable local artifacts. | Source digest is source-backed; no durable acceleration artifact profile exists yet. |
| Arrow IPC projection | The native columnar facet should not own Arrow identity. | Arrow import/export and binary batch representation own that file and batch behavior. | This is source-backed behind the `columnar-arrow` feature for the promoted type profile. |
| Parquet projection | The native columnar facet should not own Parquet identity. | Parquet import/export and external-tool interoperability own that behavior. | This is source-backed behind the `columnar-arrow` feature; Parquet segments are not canonical storage. |
| Durable Apache Parquet segment profile | Needs manifest-to-file mapping, row groups, statistics, compaction, garbage collection, recovery, and versioned compatibility policy | Apache Parquet-aware engines and file clients consume it | Target storage promotion |
| Partitioning, predicate pushdown, and selective read | Needs declared partition keys, pruning semantics, statistics correctness, and cost controls | Dataframe, SQL, Arrow Flight, and warehouse presentations use the plan | Target native performance work |
| Merge and concurrent append behavior | Needs segment-level conflict/merge policy, append ordering, duplicate prevention, and conformance vectors | Version-control and sync expose resulting commits | Target native version-control work |
| Arrow Flight and Arrow Flight SQL | The native columnar facet should not own request/session wire mechanics. | Flight clients transfer large Apache Arrow batches and SQL-shaped results. | This remains target hosted presentation work. |

### Required Separations

| Concern | Native ownership | Engine or presentation ownership | Reason |
| --- | --- | --- | --- |
| Canonical segment manifest versus Arrow/Parquet bytes | Schema, Loom scalar semantics, segment identity, policy, and commit history | File encoding, record-batch/page layout, and external client interoperability | External formats can evolve without changing Loom identity |
| Durable dataset versus dataframe plan | Committed analytical data and segment lifecycle | Input cleanup, schema inference, transformations, previews, and output selection | A transformation plan is not a durable analytical dataset until materialized |
| Portable select/aggregate contract versus acceleration | Semantics, errors, ordering, and authorization | Native vectorized engine implementation | Optimizations cannot silently change query results |
| Segment statistics versus query planner | Deterministic committed statistics | Pruning and cost decisions | Planner heuristics should remain rebuildable and independently testable |
| Columnar data versus analytical SQL grammar | Typed data and basic data operations | DuckDB-like and warehouse SQL client dialects | SQL product compatibility must not define columnar storage |

### Completion State And Primitive Placement

| Capability group | Native `columnar` | Engine or presentation placement | Design state |
| --- | --- | --- | --- |
| Canonical typed schema, ordered segments, append, scan, compact, inspect, source digest, select, aggregate, ACL, and versioning | The native columnar facet owns these reusable storage and query semantics. | Command-line commands, language bindings, MCP tools, REST endpoints, and JSON-RPC endpoints adapt them. | This behavior is source-backed. |
| Arrow IPC and Parquet import/export with native arrays and canonical-cell carriers for extended Loom values | The native columnar facet should not own external format identity. | Optional `columnar-arrow` feature and native or hosted binary routes own the interchange behavior. | The promoted interchange profile is source-backed. |
| Native vectorized execution, partitioning, pushdown, durable Parquet segments, segment merge, and high-scale artifact lifecycle | The native columnar facet should own reusable storage and query semantics once specified. | Engines and analytical presentations consume them. | This remains target native performance work. |
| Arrow Flight, Arrow Flight SQL, DuckDB-like, warehouse-like, Spark-like, and BigQuery-like behavior | The native columnar facet should not own client grammar or session protocol. | Analytical compatibility presentations own those behaviors. | This remains target design and conformance work. |

## `queue`

### Native

The native queue facet is a versioned append-log stream substrate, not a broker product. It owns
ordered stream entries, structured stream storage, explicit consumer-offset operations, configurable
retention, declared key compaction, replay boundaries, explicit reconciliation, and durable stream
lifecycle. `queue` owns committed stream content. `delivery` owns subscriber delivery state.
`loom-coordination` owns leases, claims, fencing, group generations, and transaction authority.

The source-backed queue surface is intentionally small: append, get, range, len, consumer position,
consumer read, consumer advance, and consumer reset. Reads do not advance progress. Consumer offsets
are authority-local operational metadata outside the committed stream tree by default.

### Queue, Delivery, And Coordination Boundary

The enterprise boundary is three cooperating primitives, not one broker-shaped `queue` monolith.

| Concern | Owner | Current source-backed state | Target behavior | Facades that consume it |
| --- | --- | --- | --- | --- |
| Committed stream content | `queue` | Structured stream root, sequence-keyed entry map, payload digests, append/get/range/len, commit/checkout/clone/bundle reachability | Retention, compaction, tombstones, replay or resequence tools, and hosted wire parity | Kafka, Redis Streams, JetStream, delivery outboxes |
| Authority-local consumer position | `queue` through 0021b | Position/read/advance/reset outside commits and ordinary sync | Observed anchors and compare-and-advance for stale progress detection | Kafka offset commits, Redis groups, JetStream consumers, native consumers |
| Delivery envelope and subscriber acknowledgment | `delivery` through 0035 | Envelope codec, idempotent message identifiers, replay, redelivery, subscriber acknowledgment through queue offsets, and authorization checks | Hosted WebSocket, Server-Sent Events, JSON-RPC, gRPC, and MCP projection, retention-gap errors, backpressure, and process-restart behavior | MQTT QoS flows, NATS Core fanout, Redis publish/subscribe, watches, triggers, and execution streams |
| Coordination authority | `loom-coordination` through 0036a | Single-node authority, fences, sequencers, producer epochs, consumer-group generations, and transaction records | General durable high-water adapter, daemon integration, and future consensus backend | Kafka, JetStream, MQTT sessions, Redis claims, SQL sessions, and locks |
| Live fanout | Delivery runtime, not native queue content | MCP App notification baseline only | Bounded live session fanout with retryable backpressure and no silent loss | MQTT QoS level 0, NATS Core, and Redis publish/subscribe |
| Product envelopes | Facade-specific adapters | Kafka stores record-batch envelopes; Redis, MQTT, and NATS are not implemented | Preserve product-visible identifiers, headers, fields, reason codes, and errors without changing queue bytes | Each product facade |
| Cluster topology | Future coordination or deployment profile | Not source-backed | Explicit distributed backend selection before cluster claims | Kafka multi-broker, NATS cluster routes, Redis cluster, MQTT clustered brokers |

### Queue Primitive Sequencing

Shared primitive work should happen in this order so product facades do not fork their own storage or
coordination models.

| Order | Primitive | Why it comes first | Depends on |
| --- | --- | --- | --- |
| 1 | Observed anchors and compare-and-advance | Higher-level acks, claims, and commits need stale-progress rejection before they can be correct under served concurrency | 0021b consumer offsets and stream root identity |
| 2 | Retention and compaction | Kafka compacted topics, Redis trimming, JetStream retention, and delivery retained-gap behavior need the same retained-gap and tombstone rules | 0021a stream storage |
| 3 | Delivery backpressure and leased subscriber delivery | MQTT QoS flows, NATS Core, Redis publish/subscribe, and hosted watches need one reconnect and slow-subscriber model | 0035 delivery and 0036 lease semantics |
| 4 | Coordination high-water persistence and lease claims | Kafka groups, Redis pending entries, JetStream consumers, and MQTT persistent sessions need durable generations, claims, and fencing | 0036a single-node coordinator |
| 5 | Product facade profiles | Once shared primitives exist, each product profile maps grammar and wire behavior without owning duplicate internals | Facade-specific protocol codecs and hosted listeners |

### Presentation Candidates

Kafka, MQTT, NATS/JetStream, and Redis Streams are first-class product facades because their consumer,
session, topic or subject, retention, and policy semantics differ materially.

### Primitive Gaps

| Primitive | Native queue role | Facade role | Current boundary |
| --- | --- | --- | --- |
| Ordered stream entry log | The native queue facet prevents each facade from inventing its own ordering model by owning sequence allocation, append order, range reads, and structured stream roots. | Kafka records, Redis stream entries, and JetStream messages adapt this order to product-visible identifiers. | Source-backed for append, get, range, and length operations. |
| Opaque payload storage | The native queue facet stores durable bytes and payload digests without interpreting product envelopes. | Kafka record batches, Redis field maps, MQTT packets, and NATS headers define their own envelope semantics. | Source-backed for queue bytes; facade envelopes stay facade-owned. |
| Stream collection naming | The native queue facet owns Loom stream identity inside a workspace or namespace so product topic names do not become canonical storage identity. | Kafka maps topics and partitions; MQTT maps topic filters; NATS maps subjects; Redis maps stream keys. | Source-backed for native stream names; facade validation is separate. |
| Consumer offsets | The native queue facet owns authority-local position, read, advance, and reset operations so reads do not silently mutate progress. | Kafka groups, Redis consumer groups, JetStream durable consumers, and MQTT inflight state add product semantics. | Source-backed for native offsets. |
| At-least-once progress | The native queue facet owns explicit read and explicit advance so duplicate delivery is acceptable and silent loss is not. | Facades map acknowledgment, commit, pending-entry, redelivery, and retry behavior. | Source-backed for native consumers. |
| Observed anchors | The native queue facet needs compare-and-advance and stale-progress protection so a consumer cannot advance from an unseen or superseded position. | Kafka stale generation checks, Redis pending-entry claims, and JetStream durable consumers can use it. | This remains target design work. |
| Retention and compaction | The native queue facet needs policies by age, entry count, byte size, discard mode, keyed compaction, tombstones, compaction watermarks, and retained-gap behavior so every facade reports pruning consistently. | Kafka compacted topics, Redis stream trimming, and JetStream retention policies map to native primitives. | This remains target design work. |
| Replay and resequence | The native queue facet needs explicit tools for divergent append histories and deterministic recovery so correction does not become an implicit facade behavior. | Facades must not silently merge or reorder client-visible logs. | This remains target design work. |
| Leased work claims and backpressure | The native queue facet should not own leased work claims or backpressure by itself because those behaviors are shared delivery and coordination concerns. | Kafka groups, MQTT QoS flows, JetStream consumers, Redis pending entries, and delivery fanout use shared coordination or delivery primitives. | This remains shared coordination and delivery target work. |
| Live fanout and publish/subscribe | The native queue facet does not commit live fanout messages into queue storage by default because live delivery can be intentionally non-durable. | MQTT QoS level 0, NATS Core, and Redis publish/subscribe own live session behavior. | This remains delivery runtime target work. |
| Transactions and producer fencing | The native queue facet should not own product transaction semantics because Kafka producer epochs and transactions are coordination records. | Kafka owns producer epochs and transactions through coordination records. | This behavior belongs to the Kafka facade and the shared coordination substrate. |
| Broker clustering and quorum | The native queue facet should not own deployment topology because cluster membership and quorum are distributed-system contracts. | Kafka, NATS, MQTT, and Redis cluster claims require separate distributed coordination design. | This is outside the single-node queue scope. |

### Queue And Stream Facade Placement

| Facade | Uses native queue for | Facade owns | Shared dependencies | State |
| --- | --- | --- | --- | --- |
| Kafka | Durable topic-partition logs, fetch offsets, and committed record storage | Kafka TCP wire protocol, API versions, topic metadata, partitions, consumer groups, producer epochs, transactions, and capability reporting | Hosted authentication and policy, `loom-coordination`, and durable metadata-version allocation | Source-backed bounded single-broker profile; multi-partition and group membership remain target work. |
| Redis Streams | Durable stream entries, retention or trim substrate, and reusable consumer progress | `X*` commands, Redis stream identifiers, field-value entry shape, consumer groups, pending entries, `XACK`, `XPENDING`, `XCLAIM`, blocking reads, and trimming responses | Redis substrate, native queue retention, leased work claims, and delivery/backpressure | Explicitly unsupported in Redis until queue integration is promoted |
| MQTT | Durable delivery only for MQTT QoS level 1 and level 2 inflight and session state where configured | MQTT version 5 packets, topic filters, retained messages, wills, QoS handshakes, sessions, shared subscriptions, reason codes, and flow control | Hosted TCP, delivery fanout, coordination, and cache-like expiry | Not implemented. |
| NATS Core | Usually does not use persisted queue storage | NATS subjects, wildcard matching, request/reply, queue groups, live at-most-once fanout, and no-subscriber behavior | Hosted TCP, delivery fanout, principal authentication, and policy | Not implemented. |
| NATS JetStream | Persisted stream capture, replay, retention, and durable consumer state | JetStream stream and consumer management APIs, acknowledgment behavior, redelivery, pull/push delivery, and flow control | Native queue retention, leased work claims, delivery, and coordination | Not implemented. |

### Completion State And Primitive Placement

| Capability | Native `queue` owns | Depends on | Current state | Next primitive work |
| --- | --- | --- | --- | --- |
| Structured stream storage | The native queue facet owns structured stream storage. | 0021a structured append-log storage | This behavior is source-backed. | Keep conformance aligned with canonical stream root and entry records. |
| Append, get, range, and length | The native queue facet owns append, get, range, and length operations. | Core queue substrate and bindings | This behavior is source-backed. | Add hosted gRPC and broader wire projections where needed. |
| Consumer position, read, advance, and reset | The native queue facet owns authority-local consumer offset operations. | 0021b authority-local offsets | This behavior is source-backed in bindings. | Add hosted consumer-offset projection and observed anchors. |
| Observed anchors and compare-and-advance | The native queue facet should own observed anchors and compare-and-advance behavior. | Stream root identity and conflict handling | This remains target design work. | Add stale-progress detection before higher-level group claims depend on it. |
| Retention and compaction | The native queue facet should own retention and compaction behavior. | Queue policy and stream index maintenance | This remains target design work. | Specify reusable retention, trim, keyed compaction, tombstone, and retained-gap semantics. |
| Replay and resequence tooling | The native queue facet should own explicit replay and resequence tooling. | Conflict and recovery model | This remains target design work. | Build explicit operator tools rather than implicit facade behavior. |
| Delivery fanout, leased work claims, and backpressure | The native queue facet should not own delivery fanout, leased claims, or backpressure by itself. | Durable delivery and coordination | The delivery substrate is source-backed; hosted fanout and backpressure remain target work. | Centralize this behavior so MQTT, NATS, Redis Streams, and Kafka groups do not duplicate it. |
| Coordination sequencers, producer epochs, group generations, and transaction records | The native queue facet should not own coordination authority. | `loom-coordination` | The single-node substrate with Kafka integration is source-backed. | Keep this reusable so facades map into it rather than embedding Kafka-only state. |
| Product wire grammars | The native queue facet should not own product wire grammars. | Compatibility facades | This is facade-specific behavior. | Keep Kafka, Redis, MQTT, and NATS capability reports separate from native queue. |

## `time-series`

### Native

Timestamped structured points, measurement and tags, typed fields, ordered ranges, rollup storage,
query visibility, explicit retention operations, and deterministic point identity are the native
domain. Generic time-series does not infer telemetry instrument semantics, resource identity, or
observability cardinality policy; those belong to `metrics`.

### Presentation Candidates

Influx is a time-series facade. Prometheus and OTLP metrics are primarily `metrics` facades over the
time-series substrate. Grafana is a composite visualization facade, not a time-series protocol.

### Primitive Gaps

Point-level diff and merge, automatic retention compaction, scheduled rollup materialization, query
indexes, and a bounded native time-series query abstract syntax tree remain target work. Metric types, telemetry
resources, scope, units, and cardinality are intentionally owned by `metrics`.

### Completion State And Primitive Placement

| Capability group | Native `time-series` | Presentation placement | Design state |
| --- | --- | --- | --- |
| Structured point identity, tags, typed fields, ranges, and rollup storage | The native time-series facet owns structured point storage semantics. | Influx maps directly to this storage; metrics facades use it as storage underneath native metric semantics. | The base is source-backed; query and index evolution remains incomplete. |
| Retention and derived rollups | The native time-series facet owns storage controls for retention and rollups. | Facades map their visible retention and query policies to those controls. | The behavior is partially source-backed; automatic operations remain incomplete. |
| Metric type, units, resource or scope, temporality, and cardinality | The native time-series facet should not infer telemetry semantics. | The planned `metrics` facet owns telemetry semantics. | This placement is approved. |
| Ingestion formats, query grammars, dashboards, and telemetry envelopes | The native time-series facet should not own all product ingestion and query contracts as one combined contract. | Influx, Prometheus, Grafana, and OTLP facades own their product semantics. | Support is partially source-backed; facade profiles are recorded in this document. |

## `cas`

### Native

The native content-addressed storage facet is workspace-scoped reachable immutable bytes addressed by the store's digest
profile. It owns content identity, digest verification, idempotent put, optional reachable-reference
deletion, sorted reachable listing, workspace reachability, and integrity checks. It does not own
human names, buckets, object keys, repository tags, upload sessions, archive framing, or foreign CID
identity.

The current source-backed native shape is put, get, has, delete, and list. `get` verifies returned
bytes against the requested digest. `list` is scoped to one workspace's reachable content-addressed
storage facet and is not a global provider inventory. Content-addressed storage bytes participate in workspace history through the workspace tree
under `/.loom/facets/cas/<digest-hex>`.

### Presentation Candidates

S3 and OCI Distribution are first-class served compatibility surfaces. Content
Addressable aRchive and archive are
interchange formats, not daemon listeners. Artifact transfer is a higher-level profile that may use
content-addressed storage, files, OCI Distribution, S3, Content Addressable
aRchive, archive, signatures, and manifest metadata, but it should not redefine the native
content-addressed storage object contract.

### Primitive Gaps

| Primitive | Native content-addressed storage role | Facade, interchange, or artifact role | Current boundary |
| --- | --- | --- | --- |
| Content identity | The native content-addressed storage facet owns digest-addressed immutable bytes under the store identity profile. | OCI Distribution verifies SHA-256 distribution digests; CAR maps Loom digests into CID version 1 raw-codec multihash CIDs; S3 entity tags are representation or version entity tags, not Loom digests. | Native behavior is source-backed; foreign identity mapping is facade-owned or interchange-owned. |
| Reachable set | The native content-addressed storage facet owns the workspace-scoped reachable digest manifest through the content-addressed storage facet path. | S3 buckets, OCI repositories, CAR roots, and archive manifests select subsets or projections. | Native behavior is source-backed. |
| Put, get, has, delete, and list | The native content-addressed storage facet owns digest-verified local operations and absence behavior. | REST, JSON-RPC, gRPC, MCP, CLI, language bindings, S3, OCI Distribution, and interchange adapters adapt errors and response shapes. | This behavior is source-backed across native projections; transport absence mapping is protocol-specific. |
| Retention, pinning, and garbage-collection policy | The native content-addressed storage facet owns this only if promoted as workspace or store policy over reachable objects. | S3 object lifecycle, OCI blob delete, CAR export roots, IPFS pins, and artifact retention map to policy records. | This remains a target shared primitive. |
| Mutable names and metadata | The native content-addressed storage facet should not own mutable names or metadata. | File paths, S3 object keys, OCI tags and manifests, artifact names, and archive entries own naming and metadata. | This behavior is facade-owned or interchange-owned. |
| Upload sessions and multipart state | The native content-addressed storage facet should not own protocol-visible upload sessions. | S3 multipart upload and OCI chunked upload own protocol-visible sessions, offsets, cancellation, and completion. | This is source-backed in S3 and OCI surfaces; it is not native content-addressed storage. |
| Conditional writes and version identifiers | The native content-addressed storage facet should not own conditional writes universally. | S3 owns conditional headers, S3-safe version identifiers, and S3-compatible entity tags. | The bounded S3 profile is source-backed. |
| Repository manifests, tags, referrers, and catalog | The native content-addressed storage facet should not own repository metadata. | OCI Distribution owns repository identity, manifest media types, tags, referrers, bounded catalog, and cross-repository mount. | The bounded OCI profile is source-backed. |
| Archive framing and path safety | The native content-addressed storage facet should not own archive file framing. | Archive import/export owns tar.zstd, tar, tar.gz, zip, deterministic ordering, safe paths, and manifest records. | This is source-backed through interchange. |
| CAR framing | The native content-addressed storage facet should not own CAR as storage identity. | CAR import/export owns roots, block order, CID validation, and workspace graph reconstruction. | CAR version 1-shaped interchange is source-backed. |
| Artifact transfer | Native content-addressed storage provides immutable byte storage. | The artifact profile owns manifest, provenance, signature, promotion, package metadata, retention, and allowed target projections. | This remains target design work. |
| IPFS foreign CID cache | The native content-addressed storage facet should not own the foreign CID cache by itself. | The IPFS facade owns foreign-CID catalog, gateway cache, pins, provider and publisher profiles, and network state. | This is target appendix direction outside active content-addressed storage scope. |

### S3 Compatibility

S3 is a bucket/object service facade over files, content-addressed storage, and S3-specific state. A service-scoped listener
selects a workspace and resolves buckets from host or path-style request data. A bucket-scoped listener
binds one bucket to one endpoint. Bucket lifecycle, public access, metadata, multipart uploads,
conditional writes, versioning, S3-safe version identifiers, and S3-compatible entity tags belong to S3, not native
content-addressed storage.

### OCI Distribution Compatibility

OCI Distribution is a repository and digest-oriented facade. It uses content-addressed storage for blob bytes but owns public repository
names, stable internal repository identifiers, manifest and tag metadata, referrer indexes, media-type
admission, upload sessions, cross-repository mount, bounded catalog behavior, and strict OCI/Docker
digest semantics. Blob delete removes repository reachability metadata; physical byte reclamation
requires reachability-proven garbage collection.

### CAR And Archive Interchange

CAR is an object-graph interchange profile. It is useful for deterministic workspace graph
import/export and future IPFS-related flows, but it is not an IPFS node and not a daemon listener.
Archive import/export is a file-tree interchange profile. `tar.zstd` is the canonical performant
archive format; `tar`, `tar.gz`, and `zip` are compatibility formats. Archive path safety,
deterministic ordering, metadata handling, and file-tree mapping belong to the archive profile.

### Artifact Transfer Profile

Artifact transfer is not identical to native content-addressed storage. Content-addressed storage can store immutable bytes, but an artifact
contract also needs identity above the byte level: names, versions, target platforms, media types,
manifests, signatures, provenance, promotion state, retention policy, and allowed export projections.
That higher-level contract should consume content-addressed storage, files, OCI Distribution, S3, CAR, archive, principal signing, and
audit primitives instead of becoming another hidden object store.

| Artifact concern | Reusable primitive dependency | Artifact-owned behavior |
| --- | --- | --- |
| Artifact manifest | Content-addressed storage bytes, file-tree paths, declared media types, and optional OCI descriptor shape | Defines stable artifact identifier, version, components, platform selectors, and human metadata so callers do not treat raw digest lists as releasable products |
| Promotion lifecycle | Version-control references, ledger/audit entries, retention policy, and signing substrate | Defines draft, candidate, released, revoked, deprecated, and mirrored states so release status is not inferred from storage reachability alone |
| Provenance | Principal identity, signing keys, execution records, source revision, build inputs, and hash evidence | Defines the attestation envelope, claim fields, signer requirements, and verification failures before mapping to SLSA-like or in-toto-like statements |
| Distribution projection | OCI Distribution, S3, CAR, archive, and direct content-addressed storage reads | Defines which artifact forms may be exported, mirrored, or served so compatibility endpoints cannot leak unapproved components |
| Retention and deletion | Reachability, legal hold, pin records, store garbage collection, and audit | Defines artifact lifecycle decisions separately from physical byte reclamation so still-reachable bytes are never falsely reported as erased |

### Retention, Pinning, And Garbage-Collection Policy Sequence

Retention must be promoted as a shared policy primitive before S3 lifecycle, OCI blob reclamation,
IPFS pins, or artifact retention depend on it. Otherwise each facade will invent its own delete,
hold, prune, and evidence model over the same immutable bytes.

The target sequence is:

1. Define policy records for pins, legal holds, lifecycle classes, expiry, prune eligibility, and
   audit evidence.
2. Define reachability computation across workspace references, content-addressed storage reachable manifests, file trees,
   S3 buckets, OCI repositories, artifact manifests, and explicitly pinned foreign-CID records.
3. Define delete semantics as reference removal plus policy evaluation. Physical reclamation happens
   only after reachability and policy permit it.
4. Define hosted and daemon maintenance leases so garbage collection does not race with active uploads, hosted writes,
   or artifact publication.
5. Add conformance vectors for retained bytes, deleted references, restored historical reachability,
   legal hold failures, and post-garbage-collection integrity reads.

### Required Separations

| Concern | Native or shared owner | Facade or interchange owner | Reason |
| --- | --- | --- | --- |
| Loom digest identity versus foreign digest identity | Native content-addressed storage and store identity profile | OCI SHA-256 digests, CAR CIDs, IPFS CIDs, and S3 entity tags map explicitly | Foreign clients must not infer Loom digest behavior from product-specific identifiers |
| Reachability versus physical storage | Workspace tree, object graph, content-addressed storage manifest, and store garbage collection | Facades remove their own references and report visible deletion | Immutable byte deletion must be policy and reachability proven |
| Buckets and repositories versus content-addressed storage workspace | S3 and OCI compatibility surfaces | Native content-addressed storage remains workspace-scoped digest set | Bucket or repository metadata would pollute the native content-addressed storage contract |
| Archive and CAR framing versus synchronized state | Interchange | Sync remains 0006 bundle movement between Loom stores | Import/export converts shape; it is not live replication or ordinary branch movement |
| Artifact identity versus raw byte identity | Artifact transfer profile | OCI Distribution, S3, archive, and CAR are projections | Releases need metadata, provenance, signatures, policy, and promotion state beyond bytes |

### Completion State And Primitive Placement

| Capability | Native `cas` owns | Depends on | Current state | Next primitive work |
| --- | --- | --- | --- | --- |
| Immutable byte identity, put/get/has/delete/list, digest verification, and workspace reachable set | The native content-addressed storage facet owns these byte-storage semantics. | Store digest profile and workspace tree | This behavior is source-backed. | Keep conformance aligned across digest profiles and hosted projections. |
| Hosted native content-addressed storage REST, JSON-RPC, and gRPC | The native content-addressed storage facet owns native method semantics only. | Hosted substrate, authentication, PEP, and stable errors | Daemon openers are source-backed. | Complete transport absence and integrity-failure mapping evidence. |
| Retention, pinning, and reachability-proven garbage-collection policy | The native content-addressed storage facet owns this only if promoted as reusable policy. | Workspace reachability, store garbage collection, policy records, and audit | This remains target work. | Specify this before S3 lifecycle, OCI delete reclamation, IPFS pins, or artifact retention depend on it. |
| S3 bucket and object compatibility | The native content-addressed storage facet should not own S3 bucket/object semantics. | `files`, content-addressed storage, S3 bucket state, hosted authentication, and PEP | The bounded `s3/rest` profile is source-backed. | Expand real software development kit transcripts, direct TLS decisions, lifecycle, and policy profile. |
| OCI Distribution compatibility | The native content-addressed storage facet should not own OCI repository semantics. | Content-addressed storage, repository metadata, upload-session state, hosted authentication, and PEP | The bounded `oci/rest` profile is source-backed. | Add broader client conformance and garbage-collection-safe blob reclamation policy. |
| CAR import/export | The native content-addressed storage facet should not own archive interchange semantics. | Content-addressed storage and workspace object graph | CAR version 1-shaped interchange is source-backed. | Add CAR version 2, broader IPLD and IPFS alignment, and canonical validation vectors if promoted. |
| Archive import/export | The native content-addressed storage facet should not own file-tree archive interchange semantics. | Files, content-addressed storage, workspace object graph, and interchange records | File-tree interchange is source-backed. | Add final IDL, C ABI, and binding projection only if archive becomes a stable public surface. |
| Artifact transfer profile | The native content-addressed storage facet should not own the artifact profile. | Content-addressed storage, files, OCI Distribution, S3, CAR, archive, signatures, provenance, and retention | This remains target work. | Define artifact manifest, signature/provenance model, promotion lifecycle, policy-controlled projection, and target conformance. |
| IPFS profiles | The native content-addressed storage facet should not own IPFS profiles. | Foreign-CID catalog, managed block cache, route layer, and optional content-addressed storage bytes | This remains appendix target direction. | Keep this decoupled from content-addressed storage until the IPFS gateway-cache profile is explicitly designed. |

## `ledger`

### Native

The current native base is a profile-bound, append-only linear hash chain. Each entry has a monotonic
sequence number, opaque payload bytes, and a chain hash calculated with the Loom store identity profile.
The native target is a structured Loom ledger, not a compatibility clone of another ledger database.
Native ledger owns append order, entry identity, head inspection, integrity verification, structured
entry-log storage, range scan, explicit replay of divergent histories, retained-gap reporting, signed
checkpoints through the principal signing substrate, and optional derived proof indexes. It does not
own consensus, foreign database sessions, public transparency, or Merkle inclusion and consistency
proofs until those proof contracts are explicitly promoted.

### Presentation Candidates

Transparency-log presentation remains a candidate profile after the native proof model is promoted.
It is not merely a transport because it depends on inclusion and consistency proofs, signed
checkpoints, witness policy, publication cadence, and disclosure rules. Product-clone ledger database
compatibility is not a target for the primitive canvas.

### Primitive Gaps

| Primitive | Native ledger value | Transparency-log value | Current state |
| --- | --- | --- | --- |
| Segment-native immutable entry log | The ledger needs immutable entry segments keyed by sequence ranges so point reads, range reads, structural sharing, and append scalability do not require decoding one whole ledger blob. Sequence keys live inside the segment contract and segment index; a sequence-map-only intermediate is not approved. | Transparency-log presentations can use the same sequence order as the source for tree leaves. | Canonical segment, segment-index, head, manifest, retention, append-mode, and signed-checkpoint encoders/decoders are source-backed. Public ledger roots stage through segment-native `Tree` roots. Multi-segment append policy and physical pruning remain target work. |
| Profile-bound chain hash | Tamper-evidence for each predecessor relationship | Insufficient by itself for public inclusion or consistency proofs | Source-backed linear chain |
| Canonical head metadata | The ledger needs canonical head metadata for latest sequence, latest segment, chain head, identity profile, append mode, protected-ref mode, retention state, latest signed checkpoint, and proof policy. | Transparency-log presentations can derive tree size, Merkle root, and checkpoint state from the native head. | Canonical head metadata encoding and root staging are source-backed. Proof policy remains target work. |
| Atomic append boundary | One authoritative sequence allocation and head advance | Append sequencing before a checkpoint | Current single-writer append; batch semantics target |
| Scan cursor and retained-gap behavior | The ledger needs bounded historical reads and explicit reporting of retained, planned-prune, pruned, and legal-hold ranges so auditors can distinguish missing data from intentionally pruned history. | Auditor and monitor catch-up depend on this behavior. | Local half-open range scans, retention-state reporting, and pruned-range errors are source-backed. Hosted projection, physical pruning, and scheduling remain target work. |
| Explicit divergence replay | The ledger needs explicit divergence replay so it can preserve fast-forward-only chain truth while allowing intentional reconciliation onto a new head. | A separate log publication decision controls whether the repaired head is externally published. | This remains target design work. |
| Principal signing substrate | Loom needs a shared principal signing substrate for principal-bound key records, closed signature-suite identifiers, purpose-bound signed payloads, key rotation, revocation, and audit. Ed25519 and OpenPGP/PGP are registry suites or providers. | Signed checkpoint authority and witness acceptance use this substrate rather than reusing unrelated keys. | Principal-bound public key verification, closed ES256 and Ed25519 suite ids, and purpose-bound payload bytes are source-backed. Challenge verification, delegated signing, protected key providers, lifecycle audit, historical revocation effects, and OpenPGP/PGP remain target work. |
| Signed checkpoint | The ledger needs signed checkpoints to prove authorship and durable attestation of a named ledger head at a sequence. | Transparency-log presentation requires signed checkpoints before public publication claims are accurate. | Canonical checkpoint payloads bind namespace, collection, profile, head, segment-index, retention, and append-mode state. Ed25519 principal-signature attachment, verification, and stale-checkpoint invalidation on append are source-backed locally. Hosted projection, witness policy, proof artifacts, and non-Ed25519 providers remain target work. |
| Derived Merkle accumulator or proof index | The ledger needs an optional derived proof artifact for inclusion and consistency proofs, while canonical entries remain the source of truth. | Transparency-log presentations require these proofs to interoperate with public verification workflows. | Local namespace and collection-bound Merkle proof trees, inclusion proofs, consistency proofs, encoders, decoders, and verifiers are source-backed. Hosted projection, witness policy, and transparency presentation remain target work. |
| Witness and publication policy | Witness and publication policy must not be treated as ordinary local append behavior because it changes external trust semantics. | Witness cosigning, publication cadence, and split-view detection belong to a transparency-log presentation policy. | This remains target presentation-policy work. |
| Retention hold and prune policy | The ledger needs retention holds and prune policy to declare what history remains addressable and how compacted prefixes remain verifiable. | Audit retention and checkpoint preservation depend on this policy. | Canonical retention range metadata and local read behavior are source-backed. Physical pruning, scheduling, policy enforcement, and hosted projection remain target work. |

### Required Separation

The following boundaries must remain explicit:

| Concern | Native `ledger` responsibility | Responsibility that requires a separate decision |
| --- | --- | --- |
| Integrity | Sequence order, profile-bound chain hashes, head verification | Public verifiability, external trust, authorship, or consensus |
| Storage | Canonical entry log and head metadata | A derived proof cache becoming source of truth |
| Branches | Fast-forward-only ordinary history and explicit replay | Automatic merge of divergent append histories |
| Signatures | Purpose-bound checkpoint payloads through the shared principal signing substrate | Reusing TLS, store-encryption, or unrelated credential keys as checkpoint keys |
| Proofs | Optional canonical or derived Merkle commitment only after contract selection | Claiming RFC 6962 or RFC 9162 proof behavior from a linear hash chain |
| Product access | Native append/get/head/scan/verify/checkpoint contract | Public transparency APIs without proof, witness, and disclosure matrices |

### Completion State And Primitive Placement

| Capability group | Native `ledger` | Presentation placement | Design state |
| --- | --- | --- | --- |
| Append-only sequence, chain hash, head, and linear verification | The native ledger facet owns append-only sequence order, predecessor chain hashes, current head state, and linear verification. | A transparency-log presentation can adapt public proofs only after proof promotion is designed. | This behavior is source-backed for the local and hosted native subset. |
| Segment-native entry log, head metadata, scan, replay, and retention policy | The native ledger facet should own canonical source semantics for immutable entry segments, segment indexes, head metadata, scanning, replay, and retention. | Product surfaces map their historical-read and retention behavior to that native contract. | Segment-native root staging, local scan, and retention-state behavior are source-backed. Replay, multi-segment append policy, hosted projection, physical pruning, and scheduling remain target work. |
| Principal signing substrate | Shared cross-Loom primitive for principal keys, signature suites, purpose-bound payloads, challenge verification, delegated signing, rotation, revocation, and audit. Loom principals remain the trust anchor | OpenPGP, COSE/JWS/Sigstore-style, KMS, HSM, and native signature providers plug into the substrate without becoming the identity root | Ed25519 and ES256 verification with purpose-bound payloads is source-backed. Provider expansion and key lifecycle policy remain target work. |
| Signed checkpoints and authority binding | Native verifiability primitive over principal signing substrate | Transparency presentation may expose compatible checkpoint forms | Local Ed25519 signed checkpoints are source-backed. Proof artifacts remain derived, and hosted/witness presentation remains target work. |
| Merkle inclusion and consistency proofs | The native ledger facet should own only derived proof artifacts; canonical entries remain source identity. | Transparency-log presentation requires these proofs for external verifier compatibility. | Local derived proof artifacts are source-backed and are not stored as source identity. Hosted projection and transparency-compatible presentation remain target work. |
| Witness publication, discovery, and split-view detection | The native ledger facet should not treat witness publication, discovery, or split-view detection as ordinary local storage. | A transparency-log presentation owns the protocol and operator semantics. | This remains target presentation design work. |
| Database transactions, state values, SQL, and product sessions | The native ledger facet should not universally own database transactions, mutable state values, SQL, or product session behavior. | These behaviors are no longer part of the ledger primitive canvas after product-clone removal. | This is outside the ledger scope. |

## `program`

### Native

Content-addressed program manifests, `engine=wasm` execution, target `engine=cel` interpreted programs,
capability-scoped state access, fuel and result bounds, canonical inputs, execution outcomes, CEL
guards, derivations, statecharts, workflows, and trigger execution are the native domain. The program
contract describes observable execution semantics and persisted program identity, not a particular WASM
runtime implementation.

### Presentation Candidates

WASM tooling, CEL authoring tools, task/workflow APIs, automation schedulers, and event-driven compute
ecosystems require separate compatibility assessment. A native or hosted runtime engine is not itself a
served surface.

### Primitive Gaps

| Primitive | Native program role | Engine or presentation role | Current boundary |
| --- | --- | --- | --- |
| Content-addressed manifest | The native program facet pins engine, body digest, entrypoint, declared grants, and execution policy so a stored program is reproducible. | Tooling imports, deploys, or inspects manifests. | This is the native source of truth. |
| Program lifecycle surface | The native program facet needs a public lifecycle for storing, inspecting, listing, and removing durable programs before execution. | Command-line commands, MCP tools, hosted protocols, and language bindings adapt the same lifecycle. | This remains target work because the public lifecycle is missing. |
| Capability grants and scope matching | The native program facet limits readable and writable facet resources before guest code runs. | Policy user interfaces and automation products project the same grants. | This is the native authorization contract. |
| Canonical inputs and outputs | The native program facet makes a declared invocation reproducible from a pinned program, canonical inputs, and allowed state. | Software development kits, CLI, MCP tools, and hosted runners serialize these values. | This is the native contract. |
| Fuel, result bytes, log bytes, and collection bounds | The native program facet enforces deterministic resource limits and predictable failure. | Runtimes map these limits to engine quotas or request limits. | This is native policy and is independent of the selected engine. |
| `engine=wasm` host ABI | The native program facet defines permitted file and state operations for uploaded WASM programs. | wasmi and Wasmtime implement the same ABI. | This is source-backed as a native contract; the runtime is replaceable. |
| `engine=cel` interpreted body | The native program facet should store inspectable CEL source as a durable agent-authored program. | Agents author CEL directly; Loom interprets it through a deterministic profile. | This remains target native primitive work; the guard evaluator exists. |
| Read-only CEL result profile | The native program facet should let a CEL program compute deterministic decisions, projections, classifications, and proposed state changes as data. | Agent tools and review workflows consume the result without mutating state. | This remains the target first CEL execution profile. |
| Constrained action envelope | The native program facet should represent requested mutations as canonical, bounded, grant-checked actions that Loom validates and applies outside the CEL evaluator. | Automation products expose safe rule actions while keeping CEL inspectable. | This remains the target mutation path after read-only CEL. |
| Action validation and application | Rechecks manifest grants, principal policy, target anchors, idempotency keys, limits, and facet-specific invariants before any mutation | `exec.apply`, trigger execution, hooks, and workflow steps can share one validator | Target shared primitive |
| Direct CEL host mutation | Direct CEL host mutation has no native role because it would bypass the action envelope and couple the language evaluator to `StateAccess` internals. | This is not a presentation feature. | Rejected as the enterprise target unless a later design overturns the boundary. |
| Portable interpreter | The native program facet needs a portable interpreter so WASM can run on `wasm32` and other constrained platforms. | wasmi is the portable implementation. | This is an internal engine choice. |
| Native JIT acceleration | Native JIT acceleration reduces native execution cost while preserving the host ABI and result semantics. | Wasmtime is selected by build feature on supported native targets. | This is an internal optional engine. |
| Guard | The native program facet provides side-effect-free CEL transition predicates under read-only scoped state. | Workflow and statechart products bind guards to transitions. | This is a source-backed native logic primitive. |
| Derivation | Defines deterministic derived values from declared inputs and state | Materialization, recompute, and inspection surfaces | Native logic primitive |
| Statechart | Defines states, events, transition selection, guard application, and actions | Workflow and automation clients visualize and operate it | Native logic primitive |
| Workflow | Defines durable orchestration state, steps, retries, and compensating behavior where promoted | Hosted task and automation products expose lifecycle controls | Native logic primitive |
| Trigger execution | The native program facet binds a declared stimulus to a program invocation with captured input. | Cron, event, webhook, and queue-facing schedulers adapt their delivery contracts. | This is a native trigger primitive; external schedules are presentations. |
| Execution audit and replay | Records enough declared identity and stimuli to explain an execution | Administrative and observability surfaces inspect it | Cross-cutting native control primitive |

### CEL Mutation Profile

The target CEL profile has two layers.

| Layer | Behavior | Why this is the enterprise boundary |
| --- | --- | --- |
| Read-only result profile | A persisted `engine=cel` program evaluates deterministic CEL source against canonical inputs and authorized read context, then returns a bounded canonical result. The result may be a decision, classification, projection, or proposed action document, but the evaluator itself performs no writes. | It gives agents an inspectable authoring path without creating a second mutation API beside `StateAccess`. It is deterministic, easy to test, and useful before mutation is promoted. |
| Constrained action envelope | A CEL program returns a canonical action envelope. Loom validates the envelope against the manifest grants, principal policy, target state anchors, idempotency keys, resource limits, and facet-specific invariants, then applies it through the same execution transaction, branch gate, or trigger/hook path used by other program engines. | Mutation stays in Loom's policy and storage layer. CEL remains an expression language, while the action validator becomes the reusable safety boundary for agents, workflows, triggers, and lifecycle hooks. |

Direct mutation from CEL host functions is not the target. The WASM engine may remain mutation-capable
through the private `StateAccess` host ABI because it is already the sandboxed general-computation
substrate. CEL's value is different: inspectable, bounded, agent-authored decision logic. Giving CEL the
same direct host calls would duplicate authorization, rollback, conformance, and per-facet mutation
semantics inside the CEL evaluator.

The constrained action envelope should be a closed, versioned native contract. It should contain at
least an action kind, target facet and scope, operation-specific payload, expected anchor or revision
where the facet supports it, idempotency key, declared effect summary, and bounded result projection.
Each action kind is validated by the owning facet before application. Unknown action kinds fail
closed. A rejected action must not partially mutate state.

### Completion State And Primitive Placement

| Capability group | Native `program` | Presentation placement | Design state |
| --- | --- | --- | --- |
| Manifest, grants, `engine=wasm` host ABI, state access, deterministic execution, and resource bounds | The native program facet owns these source semantics so every runtime executes the same declared program contract. | WASM tooling and hosted runners adapt the contract. | This is source-backed as a native substrate; external compatibility review remains pending. |
| Program lifecycle and read-only `engine=cel` execution | The native program facet should own durable program lifecycle and the first read-only CEL execution profile. | Agent authoring, MCP, command-line, hosted, and binding surfaces project it. | This remains target work. |
| Constrained CEL action envelope, validation, and application | The native program facet should own the constrained action envelope, validation, and application path so mutation stays in Loom policy and storage. | Automation, trigger, hook, and workflow surfaces expose safe rule actions. | This remains target mutation-profile work; direct CEL host mutation is rejected. |
| Portable WASM interpreter and native JIT runtime | The native program facet should not expose interpreter choice or JIT runtime choice as public semantics. | Engines implement the same host ABI and execution contract. | This engine separation is source-backed. |
| Guards, derivations, statecharts, workflows, and trigger execution | The native program facet owns logic and lifecycle semantics where these features are enabled. | Automation, workflow, and scheduler products adapt lifecycle contracts. | These features are source-backed; presentation review remains pending. |
| Schedules, event ingress, webhook delivery, and queue subscriptions | The native program facet should not own every external scheduling or event protocol. | Cron, eventing, and workflow presentations own their client contracts. | Cross-facet design remains pending. |

## PIM Shared Primitives

Calendar, contacts, and mail are separate native facets because their record models, standards
profiles, and client expectations differ. They still share several platform primitives. Those shared
primitives should not be reimplemented independently inside CalDAV,
CardDAV, IMAP, JMAP, or
SMTP compatibility code.

### Shared Primitive Placement

| Primitive | Problem it solves | Native shared role | Facet-specific role | Presentation role | Current direction |
| --- | --- | --- | --- | --- | --- |
| Principal-scoped collection ownership | Calendar collections, address books, and mailboxes all need owner scope, display metadata, authorization scope, lifecycle state, and discovery without letting one protocol define identity for all clients. | A shared collection-envelope contract should define principal ownership, stable collection identifiers, display metadata, role policy hooks, tombstone behavior, and audit metadata. | Calendar stores calendar collections, contacts stores address books, and mail stores mailboxes using their own record rules. | CalDAV, CardDAV, IMAP, and JMAP adapt discovery and collection listing. | Calendar, contacts, and mail each have source-backed collection models; the reusable shared collection envelope remains target consolidation work. |
| Canonical entity tags and conditional mutation | Standards clients need lost-update protection, but entity-tag calculation must be consistent across native and compatibility surfaces. | The shared conditional-mutation primitive defines owner-issued entity tags, compare-before-write semantics, mismatch reason shape, and stable error mapping. Native surfaces use `expected_entity_tag` for exact-match guards. | Document records are source-backed on this primitive. Calendar entries, contact records, mailbox metadata, and mutable mail state each bind the shared primitive to their own canonical bytes or state token. | WebDAV, CalDAV, CardDAV, IMAP, JMAP, and other compatibility facades expose protocol-specific conditional requests or state-change preconditions as adapters over the native entity-tag contract. | The shared type layer, document adoption, and executable conditional-mutation conformance are source-backed. Calendar and contacts remain source-backed for their bounded profiles; mail has source-backed state tokens. Broader cross-facet adoption beyond those source-backed rows remains target consolidation work. |
| Change sets and synchronization tokens | Calendar, contact, and mail clients need incremental synchronization that reports deletions and retained gaps without requiring every protocol to own history. | A shared change-set primitive should define sequence identity, tombstones, retained-gap reporting, compaction, redaction, authorization filtering, and replay bounds. | Calendar and contacts derive unique-identifier-level changes; mail derives mailbox membership and mutable-state changes. | CalDAV and CardDAV expose `sync-collection`; IMAP and JMAP expose their own synchronization responses. | Calendar and contacts have bounded source-backed sync tokens; mail has source-backed mutable-state deltas. Shared retained-gap semantics remain target work. |
| Lifecycle event envelope | Automation, triggers, delivery, audit, and user interfaces need one durable event shape for record changes instead of one incompatible event grammar per facet. | A shared event envelope should define event identity, principal, collection, record reference, operation kind, previous and new anchors, redaction class, and delivery ordering. | Calendar emits added, updated, cancelled, and scheduling-relevant events; contacts emits add, update, merge, and delete events; mail emits ingest, move, copy, flag-change, and expunge events. | Program, trigger, delivery, hosted notification, and future user-interface surfaces consume events without knowing protocol-specific wire grammars. | Basic lifecycle emission is source-backed in the individual facets; the canonical shared envelope and delivery contract remain target consolidation work. |
| Delegated sharing and role policy | Shared calendars, shared address books, and delegated mailboxes need the same authorization substrate even though their standards profiles expose different discovery shapes. | A shared sharing primitive should define roles, grants, delegation, discoverability, inherited policy, revocation, conflict behavior, and audit. | Calendar, contacts, and mail bind the role model to collection-level and record-level operations. | WebDAV ACL methods, CalDAV principals, CardDAV principals, IMAP ACL extensions, and JMAP account sharing adapt the same policy where supported. | Owner-only profiles are source-backed. Delegated sharing remains target work. |
| Derived query indexes | Calendar recurrence search, contact property lookup, and mailbox search all become too expensive if every query scans records forever. | A shared derived-index lifecycle should define index declarations, source anchors, rebuild state, failure reporting, stale behavior, and serving policy. | Calendar needs temporal and recurrence-aware indexes; contacts needs normalized name/value/group indexes; mail needs header, body, thread, flag, and attachment indexes. | CalDAV, CardDAV, IMAP, and JMAP route product queries through the native index planners. | Scan-based bounded query is source-backed; derived indexes remain target performance work. |
| Push and live change delivery | Clients need prompt updates, but live notification must not become a separate source of truth or bypass authorization. | A shared delivery primitive should define subscriptions, replay windows, acknowledgement, backpressure, privacy filtering, and reconnect behavior over the durable change-set primitive. | Calendar, contacts, and mail declare which events are observable and how much payload is exposed. | IMAP IDLE, JMAP push, WebDAV sync polling, web push, and webhooks adapt the same durable event stream. | IMAP IDLE is source-backed for mail; durable cross-facet push remains target delivery work. |
| Account discovery and capability reporting | Client setup needs clear capability information without implying full standards conformance where only bounded profiles are implemented. | A shared capability primitive should expose supported profiles, unsupported standards features, limits, authentication modes, policy requirements, and stable error behavior. | Calendar, contacts, and mail report their native and compatibility capabilities independently. | Client setup, hosted administration, CLI, bindings, and MCP tools consume the same capability records. | Bounded capability reporting exists in pieces; a single PIM capability matrix remains target work. |

### Shared Sequencing

| Order | Shared primitive | Why it comes before broader compatibility | Depends on |
| --- | --- | --- | --- |
| 1 | Canonical collection envelope and conditional mutation | Compatibility clients cannot safely write shared data until ownership, entity tags, and compare-before-write behavior are consistent. | Existing calendar, contacts, and mail record models |
| 2 | Change-set, retained-gap, and lifecycle event envelope | Synchronization, automation, and push need one durable event model before each protocol adds its own live notification behavior. | Collection envelope and conditional mutation |
| 3 | Delegated sharing and role policy | Shared calendars, address books, and mailboxes should use one authorization substrate before exposing broader standards sharing behavior. | Principal authorization and collection envelope |
| 4 | Derived query-index lifecycle | High-scale query support should use one rebuild and readiness contract before each facet adds specialized indexes. | Record identity, source anchors, and change-set semantics |
| 5 | Durable push and live delivery | Live delivery should consume durable changes and authorization-filtered events rather than creating protocol-local notification state. | Change-set, lifecycle envelope, and delivery substrate |
| 6 | Personal-information-management capability matrix | Operators and clients need truthful setup and feature reporting before broad reference-client claims are documented. | Facet-specific bounded profiles and shared policy primitives |

### Shared Required Separations

| Boundary | Shared native responsibility | Facet responsibility | Presentation responsibility | Reason |
| --- | --- | --- | --- | --- |
| Collection identity versus protocol discovery | Principal ownership, collection identifiers, lifecycle, authorization hooks, and audit metadata | Calendar, contacts, and mail define the contents and mutation rules for their own collections. | CalDAV, CardDAV, IMAP, and JMAP expose discovery shapes and client setup behavior. | Discovery responses must not define native collection identity. |
| Synchronization versus live notification | Durable changes, tombstones, retained gaps, compaction, and replay bounds | Each facet decides which mutations emit which events and which payloads are authorized. | Protocols expose polling, idle, push, or webhook behavior. | A lost live notification must be recoverable from durable synchronization state. |
| Sharing policy versus standards ACLs | Roles, grants, delegation, revocation, audit, and enforcement | Each facet binds roles to allowed operations and record visibility. | WebDAV ACLs and mail ACL extensions project the policy. | One protocol's access-control vocabulary must not become the only policy model. |
| Derived indexes versus canonical records | Index lifecycle, source anchors, readiness, stale behavior, and rebuild failure reporting | Each facet declares query-specific index shapes. | Standards queries consume index-backed plans where available. | Index payloads are accelerators and must not become source identity. |
| Account setup versus server equivalence | Capability reporting, authentication modes, limits, and stable unsupported-feature errors | Each facet reports its bounded native and compatibility subset. | Client setup and administration surfaces display supported behavior. | Successful account setup must not imply full product or standards-server compatibility. |

### Completion State And Primitive Placement

| Capability group | Shared PIM role | Calendar role | Contacts role | Mail role | Design state |
| --- | --- | --- | --- | --- | --- |
| Principal-scoped collection envelope | Should centralize owner, collection identity, lifecycle, authorization hooks, and audit metadata. | Calendar collections use it for calendars and task lists. | Address books use it for contact collections. | Mailboxes use it for mailbox hierarchy and membership. | Individual source-backed models exist; shared consolidation remains target work. |
| Conditional mutation and entity tags | Should centralize compare-before-write semantics and stable conflict errors. | Calendar entries use canonical entity tags. | Contact records use canonical entity tags. | Mail mutable state uses version tokens and deltas. | Calendar and contacts are source-backed; mail state tokens are source-backed; shared abstraction remains target work. |
| Change sets and lifecycle events | Should centralize durable change identity, tombstones, retained gaps, redaction, and event envelope shape. | Calendar emits entry and scheduling-relevant events. | Contacts emits record and merge events. | Mail emits ingest, mailbox, and flag-state events. | Per-facet behavior is partially source-backed; shared envelope remains target work. |
| Delegated sharing | Should centralize role policy, grants, revocation, discoverability, and audit. | Shared calendars and delegation consume it. | Shared address books consume it. | Delegated mailboxes consume it. | Owner-only profiles are source-backed; sharing remains target work. |
| Derived indexes | Should centralize source anchors, rebuild state, stale behavior, failure reporting, and serving policy. | Calendar needs recurrence and time-range indexes. | Contacts needs normalized lookup and group indexes. | Mail needs header, body, thread, and flag indexes. | Scan-based bounded profiles are source-backed; derived indexes remain target work. |
| Push and live delivery | Should centralize subscriptions, replay, acknowledgement, backpressure, authorization filtering, and reconnect behavior. | Calendar notifications consume lifecycle events. | Contact notifications consume lifecycle events. | IMAP IDLE and future push consume mailbox changes. | Basic mail idle is source-backed; durable cross-facet push remains target work. |

## `calendar`

### Native

The native calendar facet owns typed calendar records and collection lifecycle. A calendar entry is
not raw `.ics` text: it is one canonical record per principal, collection, and UID, with iCalendar,
filesystem, and CalDAV representations rendered from that record. A recurrence master and its
overrides are a structured relation, and occurrences are derived at query time rather than stored as
independent mutable events.

### Presentation Candidates

`iCalendar` is an interchange projection. `CalDAV` is a WebDAV-based client compatibility
presentation. Neither owns Loom's entry identity, collection identity, authorization, canonical
record encoding, or commit history. A future scheduling presentation can implement iCalendar Transport-Independent Interoperability Protocol and
CalDAV scheduling only after attendee, organizer, delivery, and availability semantics are native
and testable.

### Primitive Gaps

| Primitive | Native calendar role | Presentation role | Current boundary |
| --- | --- | --- | --- |
| Calendar collection and collection metadata | Prevents each CalDAV, filesystem, or native client from inventing a different collection boundary by centralizing principal scope, display metadata, allowed component set, ACL scope, and committed lifecycle | CalDAV maps it to a calendar collection and WebDAV properties | Source-backed structured primitive |
| UID resource identity | Prevents `.ics` filenames or protocol URLs from becoming canonical identity by storing one typed entry resource per UID with canonical bytes and a stable Loom address | iCalendar and CalDAV serialize the resource as one `.ics` object | Source-backed primitive |
| Typed event and task record | Moves event/task meaning out of raw text by normalizing `VEVENT` and `VTODO`, dates, summary, status, recurrence fields, and extension preservation into a reusable record | iCalendar maps property syntax and line rules | Source-backed bounded record profile |
| Master and recurrence override relation | Prevents detached occurrences from drifting by modeling one unique-identifier master plus `RECURRENCE-ID` overrides and exclusions as a structured relation | CalDAV query responses expose matching resources and occurrences | Source-backed primitive |
| Deterministic recurrence expansion | Gives every query surface the same occurrence set by expanding recurrence rules, recurrence dates, exclusion dates, and override data within bounded ranges | CalDAV time-range filtering consumes the result | Source-backed through `loom-rrule`; non-Gregorian rules remain unsupported |
| Canonical entity tag and conditional mutation | Prevents lost updates by deriving content-address entity tags from the canonical record and enforcing compare-before-write semantics | CalDAV maps entity tags to HTTP conditional request behavior | Source-backed bounded profile |
| Collection change set and sync token | Lets clients synchronize without WebDAV owning history by deriving unique-identifier-level tombstones, diffs, and sync tokens from committed collection changes | CalDAV `sync-collection` adapts it to protocol reports | Source-backed bounded profile |
| Range and structured search | Avoids protocol-specific scans by defining recurrence-aware time-range filtering, component filtering, unique-identifier matching, and summary matching once | CalDAV `calendar-query` maps its filter model to the native query | Source-backed scan-based implementation; derived index is target work |
| Time-zone model | Prevents time drift by storing typed local and UTC date-time values while preserving inline time-zone identifier and time-zone component evidence for round trips | iCalendar and CalDAV serialize time-zone properties | Bounded source-backed profile; timezone-by-reference service is target work |
| Attendee and organizer identity | Blocks false scheduling claims until participant, organizer, delegation, participation reply, and sequence semantics are native rather than preserved only as opaque text | iCalendar Transport-Independent Interoperability Protocol and CalDAV scheduling define external messages and client states | Target native model; extension preservation is not scheduling semantics |
| Availability and free-busy | Requires a reusable availability model so busy aggregation, visibility policy, and query authorization are consistent across scheduling, native APIs, and clients | CalDAV `free-busy-query` and scheduling use it | Target primitive |
| Shared calendars and delegation | Prevents WebDAV ACLs from being the only sharing model by defining collection roles, delegated access, conflict behavior, and audit once | CalDAV principal/discovery properties project the policy | Owner-only hosted profile is source-backed; shared/delegated profile is target work |
| Derived calendar index | Avoids high-scale recurrence scans by adding rebuildable temporal columns and selective text/property indexes while keeping records as source of truth | Hosted CalDAV query planner and native clients use it | Target performance primitive; records remain canonical source of truth |
| Lifecycle event envelope | Gives automation and delivery one stable event stream by emitting added, updated, and cancelled events after validated record changes | Program, trigger, and delivery domains consume events | Source-backed cross-facet primitive |

### Required Separations

| Concern | Native ownership | Presentation ownership | Reason |
| --- | --- | --- | --- |
| Calendar record versus `.ics` bytes | Typed canonical record, validation, identity, and extension bag | RFC 5545 text parsing, escaping, folding, and property grammar | Text serialization must not become the identity or mutation model |
| Calendar synchronization versus WebDAV | Commit history, unique-identifier-level changes, tombstones, and authorization | `sync-collection` request and XML response behavior | Other Loom surfaces can use the same commit history without WebDAV |
| Recurrence versus materialized occurrences | Rule data, overrides, bounded expansion, and deterministic ordering | CalDAV response shaping and client-specific query syntax | Persisting mutable occurrence copies would create consistency and merge problems |
| Calendar data versus scheduling delivery | Events, tasks, participants, availability, and local state | iCalendar Transport-Independent Interoperability Protocol methods, scheduling inbox/outbox, status replies, and transport delivery | Invitation delivery is an inter-principal protocol, not intrinsic event storage |
| Time-zone semantics versus a timezone service | Typed time values and inline zone preservation | Timezone distribution, lookup headers, service discovery, and reference behavior | A service policy can evolve without redefining stored events |

### Completion State And Primitive Placement

| Capability group | Native `calendar` | Presentation placement | Design state |
| --- | --- | --- | --- |
| Principal-scoped collections, typed `VEVENT` and `VTODO` entries, UID identity, canonical entity tags, create/read/update/delete, ACL, and committed history | The native calendar facet owns these reusable calendar record semantics. | iCalendar and CalDAV adapt the records. | This behavior is source-backed. |
| Recurrence masters, overrides, recurrence rules, recurrence dates, exclusion dates, and bounded range expansion | The native calendar facet owns recurrence identity and bounded expansion semantics. | CalDAV time-range query consumes the expanded results. | The bounded Gregorian profile is source-backed. |
| iCalendar parsing and projection | The native calendar facet should not treat iCalendar text as canonical data. | The RFC 5545 serialization and parsing adapter owns text syntax. | The bounded one-resource profile is source-backed. |
| Discovery, WebDAV methods, conditional requests, multiget, calendar-query, and sync-collection | The native calendar facet should not own WebDAV protocol mechanics. | The bounded owner-only CalDAV profile owns those mechanics. | This behavior is source-backed; broad reference-client conformance remains target work. |
| Derived indexes and high-scale query planning | The native calendar facet should own rebuildable data structures once they are specified. | Native and CalDAV query paths use those indexes. | This remains target performance work. |
| Scheduling, free-busy, availability, delegated sharing, and attendee lifecycle | The native calendar facet must own reusable participant, policy, and availability primitives before protocol promotion. | iCalendar Transport-Independent Interoperability Protocol, CalDAV scheduling, and later delivery adapters consume those primitives. | This remains target design and implementation work. |
| Timezone-by-reference, non-Gregorian recurrence, and broader iCalendar property typing | The native calendar facet owns these only where the data semantics become reusable. | CalDAV and iCalendar standards profiles expose capabilities. | Each item remains target work or must be explicitly unsupported in the current bounded profile. |

## `contacts`

### Native

The native contacts facet owns typed contact records in principal-scoped address books. A contact is
one canonical record per UID, not a `.vcf` byte blob. The typed record currently covers identity,
name, organization, title, email, telephone, and typed values; its extension bags preserve fields
that the bounded typed schema does not yet interpret. Canonical records, address-book history, ACLs,
entity tags, and contact lifecycle events are reusable native contracts.

### Presentation Candidates

`vCard` 3.0 and 4.0 are interchange projections. `CardDAV` is a WebDAV compatibility presentation.
Neither owns canonical contact identity, de-duplication policy, merge provenance, address-book ACLs,
or Loom commit history. Apple Contacts behavior is a reference-client compatibility profile, not a
separate identity model.

### Primitive Gaps

| Primitive | Native contacts role | Presentation role | Current boundary |
| --- | --- | --- | --- |
| Address book and metadata | Prevents CardDAV collections, native books, and future clients from disagreeing about book identity by centralizing principal scope, metadata, ACL scope, and lifecycle | CardDAV maps it to an address-book collection and properties | Source-backed structured primitive |
| UID contact identity | Prevents `.vcf` resources or client-local identifiers from becoming identity by storing one canonical contact record per UID with a stable content-address entity tag | vCard and CardDAV serialize it as one `.vcf` resource | Source-backed primitive |
| Typed core contact fields | Makes common fields queryable and mergeable by normalizing formatted and structured name, organization, title, email, telephone, and type values | vCard maps property and parameter syntax | Source-backed bounded schema |
| Extension preservation | Prevents data loss during round trips by retaining unknown, `X-`, grouped, and raw vCard 3.0 properties without pretending they are typed semantics | vCard 3.0/4.0 projection restores compatible properties | Source-backed bounded lossless path; not equivalent to typed semantics |
| Canonical entity tag and conditional mutation | Prevents lost updates by deriving entity tags from canonical contact content and enforcing write conflict checks | CardDAV maps entity tags to HTTP conditions | Source-backed bounded profile |
| Contact search | Avoids client-specific string matching by defining deterministic lookup over structured names, organization, title, email, telephone, UID, and type values | CardDAV `addressbook-query` adapts property filters | Source-backed scan-based implementation; derived index is target work |
| Address-book change set and sync token | Lets contacts synchronize without WebDAV owning history by deriving unique-identifier-level diffs, tombstones, and sync tokens from commits | CardDAV `sync-collection` adapts it to protocol reports | Source-backed bounded profile |
| Contact lifecycle envelope | Gives automation and delivery stable signals by emitting add, update, and merge events only after validated contact changes | Program, triggers, and delivery domains consume events | Source-backed cross-facet primitive |
| Identity evidence and match candidates | Avoids unsafe automatic dedupe by storing normalized values, confidence, provenance, and explicit review state before any merge | CardDAV and vCard can expose resulting records, not own resolution | Target native primitive |
| Merge operation and provenance | Makes merges reversible and explainable by recording source-to-survivor mapping, field-level resolution, audit evidence, and lifecycle emission | Clients present merge controls and conflict resolution | Target native primitive; lifecycle hook is not a merge model |
| Groups and relationships | Prevents raw vCard group syntax from becoming policy by defining contact group identity, membership, role, and relationship semantics | vCard `KIND`, `MEMBER`, and related properties adapt the model | Target native model; raw property preservation is insufficient |
| Rich contact fields | Expands typed semantics only where fields need query, merge, policy, or validation behavior, including postal addresses, dates, photos, keys, categories, and custom fields | vCard property registries carry field syntax | Target schema expansion |
| Shared books and delegated access | Prevents CardDAV ACLs from being the only sharing model by defining reusable role policy, discovery, and cross-principal authorization | CardDAV principal and ACL properties project the policy | Owner-only hosted profile is source-backed; sharing is target work |
| Derived contact indexes | Avoids high-scale scans by adding rebuildable normalized indexes for names, values, groups, and selective property lookup while records remain source of truth | Native and CardDAV query paths use them | Target performance primitive; records remain canonical |

### Required Separations

| Concern | Native ownership | Presentation ownership | Reason |
| --- | --- | --- | --- |
| Contact record versus vCard text | Typed fields, canonical record bytes, UID, extension retention, and validation | vCard 3.0 and 4.0 grammar, parameters, folding, and dialect selection | A vendor or RFC text dialect cannot define Loom identity semantics |
| Contact synchronization versus WebDAV | Commit history, unique-identifier diffs, tombstones, and authorization | CardDAV `sync-collection`, XML, Distributed Authoring and Versioning discovery, and HTTP methods | Native sync remains useful to CLI, bindings, MCP tools, and later APIs |
| Raw-property preservation versus typed meaning | Retained raw values and explicit typed fields | Projection of properties, parameters, and client labels | Preserving a value does not make it queryable, mergeable, or policy-aware |
| Duplicate detection versus merge | Candidate detection, evidence, confidence, and review state | Client user interface and automation present a decision | Automatically overwriting contact data would be destructive and unexplainable |
| Contact group versus vCard group syntax | Contact-group identity, membership, roles, and ACL | vCard `KIND:group`, `MEMBER`, and property grouping serialization | vCard grouping syntax is not a durable address-book group model |
| Address-book sharing versus CardDAV ACL | Native roles, authorization, discovery policy, and audit | WebDAV ACL methods and CardDAV principal response shapes | Shared policy must work outside one protocol and remain enforceable by the PEP |

### Completion State And Primitive Placement

| Capability group | Native `contacts` | Presentation placement | Design state |
| --- | --- | --- | --- |
| Principal-scoped books, typed contacts, UID identity, canonical entity tags, create/read/update/delete, ACL, commit history, and lifecycle events | The native contacts facet owns these reusable contact record semantics. | vCard and CardDAV adapt the records. | This behavior is source-backed. |
| Core name, organization, title, email, telephone, type values, and bounded extension preservation | The native contacts facet owns record semantics and preservation policy. | vCard 3.0 and 4.0 project supported properties. | The bounded schema is source-backed. |
| vCard parsing and projection | The native contacts facet should not treat vCard text as canonical data. | vCard 4.0 native projection plus CardDAV vCard 3.0 default and vCard 4.0 output own text projection behavior. | The bounded dialect profile is source-backed. |
| Discovery, WebDAV methods, conditional requests, multiget, property query, and sync-collection | The native contacts facet should not own WebDAV protocol mechanics. | The bounded owner-only CardDAV profile owns those mechanics. | This behavior is source-backed; full RFC conformance remains target work. |
| Identity resolution, merge provenance, groups, rich typed fields, and derived indexes | The native contacts facet should own reusable data and query semantics once specified. | CardDAV, vCard, and clients consume the result. | This remains target native work. |
| Delegated sharing, ACL authoring, Apple labels, xCard, and broader CardDAV behavior | The native contacts facet owns only reusable policy and data semantics. | CardDAV, vCard dialects, and client-specific compatibility behavior own protocol or client projection. | Each item remains target work or must be explicitly unsupported in the current bounded profile. |

## `mail`

### Native

The native mail facet owns principal-scoped mailboxes, immutable RFC 5322 message bodies, structured
message metadata, mutable mailbox state, and message lifecycle. The raw body is stored in content-addressed storage and
identified by digest. A structured record holds parsed headers and body metadata. Flags and labels
are versioned separately from immutable message metadata so frequent state changes do not rewrite
the message record or body. Mailbox-level UID mappings and subscriptions are native compatibility
state because IMAP clients require their durability.

### Presentation Candidates

`IMAP` is the primary mailbox compatibility presentation. `JMAP` is a source-backed bounded JSON
mail presentation, not merely a future possibility. `SMTP` is source-backed only as an account setup
compatibility listener: it authenticates and accepts a bounded probe without relaying, delivering,
or mutating native mail. Real message submission is a distinct delivery and policy product.

### Primitive Gaps

| Primitive | Native mail role | Presentation role | Current boundary |
| --- | --- | --- | --- |
| Mailbox and metadata | Prevents IMAP folders, JMAP mailboxes, and native mailboxes from diverging by centralizing principal scope, hierarchy metadata, ACL scope, and lifecycle | IMAP and JMAP expose mailbox lists and mailbox objects | Source-backed structured primitive |
| Immutable raw message body | Preserves content identity by storing RFC 5322 raw bytes in content-addressed storage, verifying digests, and separating body reachability from mutable mailbox state | IMAP FETCH, JMAP download, and `.eml` projection retrieve it | Source-backed primitive |
| Structured message index | Avoids reparsing raw mail for every client operation by extracting UID, selected headers, body digest, and size into stable metadata | IMAP and JMAP select and shape client response fields | Source-backed bounded metadata profile |
| Mutable flags and labels | Prevents flag churn from rewriting immutable messages by storing sorted flag state in a separate versioned subtree with explicit merge behavior | IMAP STORE and JMAP `Email/set` map client state changes | Source-backed primitive with operation-style keyword merge and observed-version replacement |
| Mutable-state change log and retention | Lets clients recover from missed mailbox changes by retaining flag version tokens, detailed deltas, redacted audit summaries, compaction, and retained-gap recovery | IMAP and JMAP synchronize the current state and expose protocol-specific change results | Source-backed primitive |
| Mailbox message membership and move/copy | Prevents copy/move from becoming client-side rewrite logic by defining atomic placement/removal, flag transfer policy, and lifecycle events | IMAP COPY/MOVE and JMAP copy/move semantics adapt the operation | Source-backed bounded primitive |
| Durable IMAP unique-identifier state | Keeps standards clients usable by persisting `UIDVALIDITY`, numeric unique-identifier assignment, `UIDNEXT`, and subscriptions without making them global Loom message identity | IMAP SELECT, FETCH, SEARCH, and subscription commands use it | Source-backed compatibility primitive |
| Search and retrieval plan | Avoids separate IMAP and JMAP search engines by defining bounded metadata, header, body/text, flag, keyword, unique-identifier-set, and size filters once | IMAP SEARCH and JMAP `Email/query` map external grammars to it | Source-backed bounded scan/query profile; broader query engine is target work |
| Message thread model | Requires canonical thread identity so references, in-reply-to resolution, merge policy, and stable grouping do not remain JMAP-only behavior | JMAP `Thread/*` and IMAP THREAD extensions expose client semantics | Current JMAP model is bounded; robust native threading is target work |
| MIME structure and attachment model | Avoids repeated MIME parsing and inconsistent attachment identity by storing parsed body-part trees, decoded representations, attachment identifiers, and selective retrieval policy | IMAP BODYSTRUCTURE/FETCH sections and JMAP body values/blobs consume it | Target native primitive; raw bytes and bounded headers are source-backed |
| Mailbox quotas and retention | Requires shared accounting and lifecycle policy so byte/message limits, legal hold, prune, and deletion behavior are not reimplemented per mail protocol | JMAP Quota and administrative controls expose policy | Target shared storage/policy primitive |
| Identity and submission policy | Separates stored mailbox data from authority to send mail by defining sender identities, envelope authorization, outbound queue, retry, DSN, signing, and audit before real submission | SMTP submission and JMAP EmailSubmission expose protocol contracts | Target delivery product; setup-only SMTP is not submission |
| Push and change delivery | Prevents each client protocol from inventing delivery state by centralizing durable subscriptions, event sequencing, fanout, acknowledgements, replay, and privacy policy | JMAP push, IMAP NOTIFY/IDLE extensions, web push, and webhooks adapt it | Basic IMAP IDLE is source-backed; durable push is target delivery work |
| Lifecycle event envelope | Gives automation and filtering one stable event stream by emitting ingest, move, and flag-change events after validated mutations | Program, triggers, filtering, and delivery domains consume them | Source-backed cross-facet primitive |

### Required Separations

| Concern | Native ownership | Presentation ownership | Reason |
| --- | --- | --- | --- |
| Raw message body versus mailbox state | Immutable RFC 5322 bytes, digest, and content-addressed storage lifecycle | IMAP and JMAP retrieval response syntax | A flag or label edit must not rewrite message bytes or break content identity |
| Structured message index versus MIME presentation | Parsed stable headers and message metadata | Full MIME tree, body part, section, and decoded-content response semantics | Client response grammars must not determine canonical message storage |
| Mutable flags versus version-control history | Current state, version token, deltas, merge rules, and compaction policy | IMAP STORE and JMAP mutation request/response behavior | High-frequency mailbox state needs bounded conflict and retention semantics |
| IMAP unique-identifier state versus Loom message UID | Canonical Loom record identity and mailbox membership | Numeric UID, `UIDVALIDITY`, subscriptions, and sequence-number behavior | IMAP identifiers are protocol compatibility state rather than global message identity |
| Mail storage versus message submission | Ingested messages, mailbox placement, flags, and lifecycle events | SMTP envelope, relay, delivery queue, DSN, signing, and sender policy | Receiving or storing mail does not imply authority to send it |
| Native change log versus client push | Durable state transitions and authorization | IMAP IDLE notifications, JMAP push, web push, and delivery endpoint contracts | Multiple clients can consume changes without sharing one push protocol |

### Completion State And Primitive Placement

| Capability group | Native `mail` | Presentation placement | Design state |
| --- | --- | --- | --- |
| Mailboxes, immutable content-addressed storage bodies, structured message records, `.eml` retrieval, ACL, and lifecycle events | The native mail facet owns these reusable mailbox and message-storage semantics. | IMAP and JMAP adapt mailbox client contracts. | This behavior is source-backed. |
| Flags, labels, version tokens, deltas, conflict behavior, audit summaries, compaction, and retained-gap recovery | The native mail facet owns mutable mailbox state. | IMAP `STORE` and JMAP state-change methods adapt it. | This behavior is source-backed. |
| Durable IMAP UID mappings, `UIDVALIDITY`, `UIDNEXT`, and subscriptions | The native mail facet owns compatibility state associated with mailbox data. | The bounded IMAP profile consumes it. | This behavior is source-backed. |
| Authentication, mailbox management, fetch, store, search, copy/move, append, expunge, close, unselect, idle, and direct encrypted IMAP | The native mail facet should not own session grammar. | The bounded owner-only IMAP profile owns session grammar. | This behavior is source-backed; full RFC 9051 behavior remains target work or explicitly unsupported. |
| Session discovery, Mailbox, Email, Thread, SearchSnippet, and Identity methods | The native mail facet should not own JMAP request grammar. | The bounded owner-only JMAP Core and Mail profile owns that request grammar. | This behavior is source-backed; the full data model and push behavior remain target work or explicitly unsupported. |
| SMTP account setup probe | The native mail facet should not own outbound delivery. | The setup-only SMTP compatibility listener owns account setup behavior. | This behavior is source-backed; there is no relay, queue, delivery, or native mail mutation. |
| MIME trees, robust threading, multi-mailbox labels, quotas, real submission, durable push, sharing, and broader client profiles | The native mail facet owns reusable storage, identity, and policy primitives once specified. | IMAP, JMAP, SMTP, and delivery protocols consume those primitives. | This remains target native and cross-facet work. |

## `search` / `fts`

### Native

The native full-text facet owns versioned collections of identifier-keyed field documents, explicit field
mappings, a portable query abstract syntax tree, deterministic reduced execution, and the contract for rebuildable
derived indexes. The command-line and served surface name is `fts`; `search` remains reserved for a future
whole-Loom discovery capability across facets. An inverted index is never commit identity or sync
payload: the canonical documents and mapping are enough to rebuild it on a capable node.

### Presentation Candidates

OpenSearch is the source-backed primary hosted compatibility profile. Elasticsearch is comparison
evidence, not an implied wire commitment. Tantivy is the optional native engine, not an external
service facade and not part of the default wasm-clean dependency path. Lucene is neither a served
target nor a separate Loom facet; a later query-string convenience dialect would not change this
ownership model.

### Primitive Gaps

| Primitive | Native FTS role | Engine or presentation role | Current boundary |
| --- | --- | --- | --- |
| Search collection and document identity | The native FTS facet owns versioned opaque document identifiers and the field-document map so OpenSearch index paths do not become canonical identity. | OpenSearch maps a collection to an index path; Tantivy maps identifiers to stored engine fields. | This is a source-backed canonical primitive. |
| Explicit field mapping | The native FTS facet owns field type, stored flag, faceted flag, and deliberate remap lifecycle so field meaning is stable across engines. | OpenSearch mapping parser and Tantivy schema adapt it. | The text and keyword profile is source-backed; richer typed fields remain target work. |
| Portable query abstract syntax tree | The native FTS facet owns match, term, phrase, range, and boolean query shape plus stable errors. | OpenSearch Query DSL maps a bounded grammar to it. | This is a source-backed reduced deterministic profile. |
| Reduced execution and capability signal | The native FTS facet owns portable scan behavior, stable hit order, and `reduced = true` reporting so constrained builds do not falsely claim engine parity. | Clients decide whether degraded behavior is acceptable. | This is a source-backed cross-platform fallback. |
| Native search-engine seam | The native FTS facet owns the `SearchEngine` contract and parity boundary. | Tantivy implements native BM25 scoring and selected deterministic query parity. | This is source-backed engine separation. |
| Derived index artifact identity | The native FTS facet owns source-digest, engine-version, format-version stamp, and lifecycle status so local engine artifacts are rebuildable and auditable. | Tantivy creates payloads; the store persists local artifacts. | This is a source-backed derived-artifact contract. |
| Derived index lifecycle | The native FTS facet needs rebuild scheduling, coalescing, readiness, failure reporting, stale detection, and serving policy. | Command-line administration and hosted listeners report status and route queries. | Durable-local status helpers are source-backed; daemon orchestration remains target work. |
| Analyzer catalog and analyzer execution | The native FTS facet needs versioned analyzer identifiers, deterministic configuration, language data policy, and compatibility reporting. | Tantivy executes analyzers; OpenSearch `_analyze` and mapping options adapt them. | The full analyzer direction is specified; current hosted custom analyzer and `_analyze` routes are unsupported. |
| Typed field semantics | The native FTS facet needs numeric, date, boolean, exact keyword, and text normalization with canonical range behavior. | Tantivy fast fields and OpenSearch mapping types adapt the native contract. | Text and keyword behavior is source-backed; richer types remain target work. |
| Ranking and hit stability | The native FTS facet owns declared score semantics, tie-break rules, pagination and cursor policy, and the reduced-versus-native distinction. | Tantivy supplies BM25 scoring; OpenSearch shapes `_score` and hit envelopes. | The BM25 match engine plus deterministic parity subset is source-backed; broader semantics remain target work. |
| Highlights and snippets | The native FTS facet needs analyzed offsets, source field selection, bounds, and escaping policy. | Tantivy and OpenSearch response formats render snippets. | This remains target native primitive work. |
| Facets and aggregation primitives | The native FTS facet needs faceted field semantics, bucket ordering, numeric aggregation types, null policy, and cardinality behavior. | OpenSearch aggregation grammar adapts calculations. | Bounded OpenSearch aggregations are source-backed; the native reusable aggregation contract remains incomplete. |
| Alias and collection-set resolution | The native FTS facet needs durable alias records, read target expansion, single write target, wildcard expansion, and authorization. | OpenSearch `_alias` and index expressions adapt the model. | This is a source-backed bounded compatibility primitive. |
| Indexing ingestion semantics | The native FTS facet needs atomic document mutation, per-item error isolation, visibility policy, and audit. | OpenSearch document routes, `_bulk`, `_msearch`, and NDJSON adapt requests. | This is a source-backed bounded immediate-visibility profile. |
| Cross-facet indexing bindings | The native FTS facet needs declared source selectors, extraction, update propagation, deletion policy, and rebuild ownership. | Document, files, mail, calendar, and other facets can feed FTS. | This remains target interoperability work; direct FTS documents are source-backed. |

### Required Separations

| Concern | Native ownership | Engine or presentation ownership | Reason |
| --- | --- | --- | --- |
| Canonical documents and mapping versus inverted-index segments | Versioned data, field contract, and remap meaning | Tantivy segment files, compression, merge timing, and local cache layout | Engine artifacts must be rebuildable and excluded from commit identity |
| Query abstract syntax tree versus analyzer implementation | Stable query intent, error codes, capability signal, and deterministic subset | Tantivy analyzer execution and OpenSearch analyzer request syntax | Analyzer internals can evolve without invalidating stored documents |
| Native query semantics versus reduced fallback | Native-authoritative analyzed-text behavior and explicit reduced status | Portable scan approximation on wasm and constrained builds | A fallback must not falsely claim BM25 or analyzer parity |
| Native aggregation semantics versus OpenSearch JSON | Bucket and metric definitions that Loom can reuse | `aggs`/`aggregations` request grammar and response envelopes | Vendor JSON should not be the only aggregation contract |
| Collection aliases versus OpenSearch index expressions | Alias identity, target set, write target, and authorization | `_aliases`, comma expressions, and wildcard route syntax | Alias behavior can serve native APIs and future non-HTTP clients |
| Search index versus whole-Loom discovery | Explicit FTS collection and query scope | Future `loom search` cross-facet orchestration | A collection index cannot silently imply global search coverage |

### Completion State And Primitive Placement

| Capability group | Native `fts` | Engine or presentation placement | Design state |
| --- | --- | --- | --- |
| Versioned identifier-keyed documents, text and keyword mapping, create/read/update/delete, remap, local query abstract syntax tree, reduced deterministic scan, and stable errors | The native FTS facet owns these reusable search semantics. | Command-line commands, language bindings, MCP tools, REST endpoints, and JSON-RPC endpoints adapt them. | This behavior is source-backed. |
| Native Tantivy BM25 match and deterministic term, phrase, range, and boolean parity subset | The native FTS facet owns the contract through the `SearchEngine` seam. | The optional `loom-tantivy` native-only engine implements this behavior. | This behavior is source-backed; the default workspace does not link Tantivy. |
| Source-digest-stamped local derived artifacts and rebuild state helpers | The native FTS facet owns local artifact identity and status contract. | Tantivy builds payloads; the store holds local bytes. | This behavior is source-backed; daemon rebuild orchestration remains target work. |
| Root information, mappings, document routes, Query DSL subset, bulk, multi-search, count, aliases, multi-index and wildcard routes, bounded aggregations, and capability report | The native FTS facet should not own OpenSearch route grammar. | The bounded OpenSearch REST and NDJSON profile owns this behavior. | This behavior is source-backed; there is no cluster, sharding, replica, or administration model. |
| Custom analyzers, `_analyze`, richer mapping types, highlighting, native faceting, full aggregation model, native-only query variants, and cross-facet maintained indexes | The native FTS facet owns reusable semantics once specified. | Tantivy and OpenSearch consume and expose them. | This remains target native and compatibility work. |
| Cluster coordination, shards, replicas, snapshots, plugin administration, and Lucene service emulation | The native FTS facet should not own these OpenSearch or Lucene product behaviors. | These behaviors are explicitly outside the single-node OpenSearch compatibility profile. | This is not planned as native FTS work. |

## `dataframe`

### Native

The native dataframe facet owns versioned logical frames: source bindings, source/version anchors,
input profiles, schema inference or override, ordered transformation plan, execution requirements,
materialization policy, and lineage. It accepts broader input than `columnar` and turns it into a
bounded transformation workflow. Polars is the default optional native executor; its internal frame
state is never Loom identity or a served surface.

### Presentation Candidates

CSV, JSON, NDJSON, Apache Arrow IPC, and Apache Parquet are input/output formats. Native REST is the current
management presentation. Arrow Flight and Arrow Flight SQL are candidate high-volume result-transfer
presentations after Apache Arrow batch conformance. A DuckDB-like analytical SQL experience means a
declared local analytical SQL compatibility profile over dataframe and columnar results, not an
embedded DuckDB engine or a Polars wire protocol.

### Primitive Gaps

| Primitive | Native dataframe role | Engine or presentation role | Current boundary |
| --- | --- | --- | --- |
| Frame plan identity | The native dataframe facet owns frame identifier, canonical CBOR plan, domain-separated digest, validation, versioning, and ACL scope. | Command-line commands, language bindings, MCP tools, and REST endpoints serialize the plan. | This is source-backed in `loom-dataframe` plus core integration. |
| Source binding | The native dataframe facet owns source alias, kind, target, input format, digest/version anchor, and sorted options so a plan can be reproduced. | Files, content-addressed storage, SQL, and columnar adapters load source data. | This is source-backed for files, content-addressed storage, columnar, and SQL results. |
| Input adapters | The native dataframe facet owns format selection, parse policy, schema inference, coercion, and failure behavior. | CSV, JSON, NDJSON, Apache Arrow, and Apache Parquet codecs implement bytes conversion. | CSV, JSON, and NDJSON are source-backed portable formats; Apache Arrow and Apache Parquet are source-backed through the optional columnar feature. |
| Schema inference and override | The native dataframe facet owns ordered typed schema and whether it is inferred or explicit. | Client user interfaces and formats display schema. | This is a source-backed primitive. |
| Logical transformation plan | The native dataframe facet owns ordered scan, select, rename, cast, filter, sort, limit, sample, join, union, aggregate, and with-column intent. | Executors compile and execute the plan. | Plan records are source-backed; some execution semantics remain target work. |
| Portable deterministic executor | The native dataframe facet owns fallback result semantics for supported operations, scalar coercion, ordering, and deterministic seeded sampling. | Native executors can delegate unsupported operations to it. | This is a source-backed baseline. |
| Optional Polars executor | The native dataframe facet owns the executor seam and explicit execution report. | `loom-polars` accelerates the supported native subset. | This is a source-backed optional engine; unsupported operations fall back rather than changing the result contract. |
| Preview and collect bounds | The native dataframe facet needs explicit result size, row, byte, timeout, pagination, and cancellation policy. | REST and Arrow Flight clients transfer or poll results. | Preview and collect behavior is source-backed; comprehensive hosted transfer control remains target work. |
| Materialization policy | The native dataframe facet owns explicit output target, source/plan/output lineage, and commit behavior. | Columnar, files, content-addressed storage, and ephemeral preview targets receive results. | This is source-backed for columnar, files, content-addressed storage, and ephemeral targets. |
| Lineage and reproducibility | The native dataframe facet owns source digests, plan digest, output digest, refresh policy for mutable sources, and audit. | Client tools inspect or reproduce runs. | Source digest and plan semantics are source-backed; the full lineage and reporting model remains target work. |
| Join, pivot, unpivot, window, and expression semantics | The native dataframe facet needs null rules, type promotion, ordering, cardinality bounds, and conformance vectors. | Portable and Polars executors implement the approved contract. | Join records exist; full execution semantics remain target work. |
| Hosted result transfer | The native dataframe facet should not own generic protocol session semantics. | Arrow Flight, Flight SQL, REST result handles, JSON-RPC, or gRPC transfer results. | Native REST management is source-backed; binary high-volume transfer remains target work. |

### Required Separations

| Concern | Native ownership | Engine or presentation ownership | Reason |
| --- | --- | --- | --- |
| Logical plan versus engine snapshot | Sources, schema, operations, materialization policy, and lineage | Polars lazy frame, optimized physical plan, caches, and runtime state | Engine upgrades must not alter Loom frame identity |
| Input formats versus dataframe identity | Input profile, parsing policy, schema and coerced values | CSV, JSON, NDJSON, Apache Arrow, and Apache Parquet bytes | One frame model can ingest multiple formats without making any format canonical |
| Transformation plan versus durable output | Reproducible intent and source anchors | Columnar, files, and content-addressed storage materialization targets | A plan is not automatically a committed result |
| Dataframe execution versus SQL compatibility | Frame transformation semantics and materialization | DuckDB-like or warehouse SQL grammar, sessions, catalogs, and result envelopes | SQL clients need a deliberate compatibility profile, not an accidental executor API |
| Native REST management versus bulk result transfer | Frame lifecycle, authorization, and bounded preview/collect operations | Arrow Flight, Arrow Flight SQL, or another explicit transfer protocol | Control-plane JSON is unsuitable for large analytical result sets |

### Analytical Primitive Sequencing

Columnar and dataframe should be developed as one analytical family while keeping ownership distinct.
The order matters because client compatibility is only useful when the underlying result identity and
transfer semantics are pinned.

| Order | Primitive or profile | Owner | Why it comes before broader client facades |
| --- | --- | --- | --- |
| 1 | Canonical columnar manifest, segment statistics, Apache Arrow IPC, and Apache Parquet interchange profile | `columnar` | Durable analytical data and external file/batch interchange need stable bytes and conformance before clients depend on them |
| 2 | Dataframe plan, source bindings, schema inference, deterministic portable execution, and materialization | `dataframe` | Messy inputs and transformations need reproducible plans before they feed durable analytical outputs |
| 3 | Optional native Polars acceleration with explicit fallback reporting | `dataframe` engine boundary | Acceleration can improve performance only after the portable semantics are the contract |
| 4 | Arrow Flight, Arrow Flight SQL, or ADBC-adjacent result transfer | Hosted analytical data-plane | Large results need binary batch transfer, result handles, schema negotiation, cancellation, and authentication checks before client tools can use Loom efficiently |
| 5 | DuckDB-like local analytical SQL, then warehouse, Spark-like, and BigQuery-like presentations | Analytical compatibility facades | Each profile must declare dialect, catalog, session, result, error, and unsupported behavior while reusing SQL, dataframe, columnar, Apache Arrow, authentication, and result-handle primitives |
| 6 | Durable Parquet segment storage, partitioning, predicate pushdown, and segment-level merge | `columnar` native performance work | These change storage and version-control behavior and need their own identity, merge, and conformance vectors |

### Completion State And Primitive Placement

| Capability group | Native `dataframe` | Engine or presentation placement | Design state |
| --- | --- | --- | --- |
| Canonical frame plans, source bindings, schema, portable parsing, loaded-batch execution, ACL, and plan or source digests | The native dataframe facet owns these reusable dataframe semantics. | Command-line commands, language bindings, MCP tools, and native REST endpoints adapt them. | This behavior is source-backed. |
| CSV, JSON, NDJSON, columnar, structured-query result, Arrow IPC, and Parquet input/output paths | The native dataframe facet owns adapter policy, not external format identity. | Codecs and optional columnar Arrow feature implement byte conversion. | This behavior is source-backed within declared format profiles. |
| Select, rename, cast, simple filter, sort, limit, seeded sample, aggregate, literal with-column, union, preview, collect, and materialization | The native dataframe facet owns semantics for the supported portable subset. | The optional Polars executor accelerates a narrower native subset. | This behavior is source-backed; unsupported native work falls back or remains target work. |
| Join, pivot, unpivot, windows, rich expressions, broad Polars coverage, full lineage reporting, and complete resource controls | The native dataframe facet should own reusable semantics once specified. | Executors and client presentations consume them. | This remains target native work. |
| REST frame management | The native dataframe facet should not own generic HTTP policy. | The hosted `dataframe/rest` surface owns the protocol profile. | The bounded profile is source-backed. |
| Arrow Flight, Arrow Flight SQL, adjacent ADBC behavior, JSON-RPC, gRPC, DuckDB-like, and warehouse-like client access | The native dataframe facet should not own client protocol or dialect behavior. | Analytical compatibility presentations own those behaviors. | This remains target design and conformance work. |

## Cross-Section Primitive Notes

### Engines, Derived Artifacts, And Proofs

Vector, graph, ledger, program, and FTS share a useful design constraint: internal engines and derived
indexes can improve execution, but they must not silently become canonical source state. The native
facet owns the stable identity and semantics; engines, proof indexes, and compatibility protocols
adapt that identity only through explicit contracts.

| Cross-section concern | Shared primitive rule | Facets affected |
| --- | --- | --- |
| Derived accelerator or index artifacts | Prevents engine caches from becoming hidden source state by storing source digest, engine version, format version, readiness, stale status, and rebuild outcome outside commit identity | `vector`, `fts`, future graph property indexes, columnar accelerators |
| Native query intermediate representation before text grammar | Prevents vendor syntax from defining Loom semantics by specifying a stable native intermediate representation, result model, errors, and resource bounds before broad product grammar claims | `graph`, `fts`, `sql`, future whole-Loom search |
| Exact or authoritative semantics versus reduced fallback | Prevents silent degradation by requiring capability reports to state whether execution is native-authoritative, reduced, approximate, stale, rebuilding, or unsupported | `vector`, `fts`, `dataframe`, `columnar` |
| Proof indexes versus source logs | Prevents audit shortcuts from becoming truth by keeping proof accelerators and witnesses outside source identity until a storage-promotion decision pins their bytes | `ledger`, audit, transparency profiles |
| Program engine versus program identity | Prevents each execution engine from creating a different program model by requiring `engine=wasm`, `engine=cel`, and future engines to share one manifest, grants, input, output, and result-envelope discipline | `program`, `exec`, triggers, hooks |
| Compatibility protocol versus native surface | Prevents protocol ports from owning storage semantics by assigning product sessions, handshakes, response envelopes, and client quirks to facades over native primitives | Qdrant, Pinecone, OpenSearch, Neo4j/Bolt, and future transparency APIs |

### Shared Sequencing Rule

The durable enterprise order is:

1. Pin native source identity and conformance vectors.
2. Add explicit local query, execution, proof, or accelerator seams.
3. Add derived artifact lifecycle, readiness, stale reporting, and rebuild policy.
4. Project the native contract through CLI, IDL, bindings, MCP tools, hosted routes, and capability reports.
5. Add product compatibility facades only after the native contract can explain what is supported,
   degraded, target, and unsupported.
6. Add richer product profiles, client transcript evidence, and operational policy once the native and
   hosted capability reports are accurate, source-backed, and specific about unsupported behavior.

## Design-Round Order

1. Establish facet, compatibility-facade, transport, engine, and interchange classification - recorded.
2. `document`, MongoDB, and Couchbase - recorded.
3. `kv`, `cache`, Redis, Memcached, and etcd - recorded.
4. `time-series`, `metrics`, logs, traces, Influx, Prometheus, Grafana, and OTLP - recorded.
5. `queue`, Kafka, MQTT, NATS/JetStream, Redis Streams, delivery, and coordination - recorded.
6. `cas` - S3, OCI Distribution, CAR, archive, and transfer primitives - recorded.
7. `columnar` and `dataframe` - storage, execution, Arrow, Parquet, and client presentation boundaries - recorded.
8. `vector`, `graph`, `ledger`, `program`, and `fts` cross-section primitive audit - recorded.
9. Files, version control, structured query, PIM, and control-plane adjacent primitives - recorded.
10. Current remaining work is implementation planning outside this file: promote the recorded primitives into owning specifications, queue items, conformance vectors, bindings, hosted surfaces, and source code as each implementation session takes scope.

## Appendix A - IPFS And Tor/Onion Analysis

This appendix records the IPFS and Tor design direction separately from the `cas` round. They share
some storage and hosted-runtime concerns, but they are not one feature: IPFS is a content-network
compatibility facade, while Tor is a reusable privacy-network route and onion-service exposure
layer for any served surface.

### Terms And Classification

| Term | Meaning in Loom | Classification | Does not mean |
| --- | --- | --- | --- |
| IPFS | The CID, IPLD, UnixFS data, retrieval, provider, pinning, and gateway client contract | First-class compatibility facade over a dedicated foreign-CID catalog and managed block cache | A synonym for Loom content-addressed storage or ordinary Files and version-control storage |
| Bitswap | IPFS P2P block exchange protocol | IPFS networking component | A complete IPFS node, routing system, or publication policy |
| CAR | CAR framing for IPLD blocks | Interchange format that content-addressed storage, artifact transfer, and IPFS designs can all use | An IPFS node or a promise to run Kubo |
| Tor | The privacy network used for anonymous outbound connections and onion-service circuits | Route option and shared network overlay | An application protocol transport or an access-control policy |
| Onion service | A Tor service published at a self-authenticating `.onion` address | Inbound listener exposure mode | A replacement for Loom principal authentication or PEP authorization |

Use `Tor` for the network and outbound route, and `onion` for an inbound onion service. The public
terms should therefore be `--route tor` and `--exposure onion`. `--exposure tor` is ambiguous: it
does not say whether Loom should publish an onion service, route an outgoing request over Tor, or
only use a local SOCKS proxy.

### Decoupled Surface Model

Tor must be usable without IPFS. IPFS may use Tor for retrieval or publication, but it is one client
of a generic route layer rather than the owner of it. The underlying application transport remains
the native protocol. For example, an IMAP listener still speaks IMAP and an S3 listener still speaks
S3 over HTTP when exposed as onion services.

```text
loom serve configure app.loom imap mail --bind 127.0.0.1:1143 --exposure onion --onion-port 993
loom serve configure app.loom s3 assets --bind 127.0.0.1:9000 --exposure onion --onion-port 443
loom sync pull app.loom origin/main --route tor
loom ipfs fetch app.loom bafy... --route tor
```

The commands are target syntax, not claims that these options are implemented. `--exposure onion`
creates an inbound onion-service projection of a configured listener. `--route tor` selects Tor for
an outbound connection created by a client or facade operation. Neither option changes the served
surface name, its application protocol, or its Loom authorization policy.

### IPFS Target Profiles

All profiles are target capability, with gateway cache first. A profile is an explicit operator
policy. Loom must never turn a cache fetch into public provider behavior or a publish operation
without an explicit profile and operator authorization.

| Profile | Reads from | Serves to | Announces or publishes | First target | Required controls |
| --- | --- | --- | --- | --- | --- |
| Gateway cache | Configured HTTP gateways | Local Loom callers and optional configured gateway response | Never announces or publishes content | This is the first target profile. | Gateway allowlist, request and byte limits, CID verification, cache TTL and capacity, and provenance audit |
| Retrieval node | IPFS peers through delegated routing, DHT discovery, and Bitswap | Local Loom callers | Never announces or publishes content | This follows gateway cache. | Peer and route policy, connection budgets, verified foreign block catalog, and cache eviction |
| Provider node | Local catalog and peers | IPFS peers | Announces approved pinned CIDs and re-provides them | This follows retrieval. | Explicit provider set, pin durability, reprovide schedule, bandwidth policy, and exposure policy |
| Publisher | Loom-selected source artifact or UnixFS projection | Local callers and, when enabled, IPFS peers | Creates approved CIDs, pins roots, and may announce them | This follows provider. | Pinned UnixFS importer profile, publication manifest, principal authorization, and idempotent jobs |
| Remote pinning and naming | Configured pinning or naming providers | Depends on remote provider | Sends explicitly authorized roots or name records | This is later target work. | Credential references, job state, audit, retry behavior, and revocation behavior |

The [IPFS lifecycle documentation](https://docs.ipfs.tech/concepts/lifecycle/) describes the
relevant distinction: content is identified by CIDs, retrieval is verified locally, and provider
announcement is a separate network action. This is why cache, retrieval, provider, and publisher
are independent Loom profiles rather than boolean switches.

### IPFS Storage And Version-Control Boundaries

The desired separation is correct, with one important correction: foreign IPFS data should not be a
second ordinary commit tree. A second standard tree would inherit normal branch, commit, diff,
merge, sync, and retention behavior, exactly what this design needs to avoid for fetched network
blocks and live peer state.

Use three explicit state classes instead.

| State class | Contents | Standard version-control treatment | Physical and API direction | Lifecycle |
| --- | --- | --- | --- | --- |
| Foreign CID cache | Content identifier, codec, multihash verification result, Loom object reference, fetch origin, fetch time, and cache accounting | Never automatically committed, branched, merged, or synced | Dedicated foreign-CID catalog over Loom object storage; controlled read-only projection under `/.loom/ipfs/cache/` is acceptable | Bounded by cache capacity, TTL, pin reachability, and explicit prune policy |
| Pins and publication manifests | Explicit pins, published roots, importer profile version, publication authority, desired provider state, and optional name references | Durable policy. A user may explicitly version a publication manifest, but fetched blocks do not become workspace content | IPFS facade metadata, with optional links from an artifact or workspace commit | Kept until unpinned or an explicit publication policy removes it |
| Network operational state | Peer identity, private network keys, routing observations, sessions, provider announcements, retry jobs, and reprovide jobs | Never standard version-control state and never sync-by-default | Local authority state owned by the IPFS runtime | Operational retention and secure local key handling |

Loom can reuse its Merkle object storage for bytes, but it cannot assume Loom's digest identity equals
an external CID. IPFS CIDs can choose codecs and multihashes that the Loom content-addressed storage profile does not use.
The foreign-CID catalog must retain the external CID and verified bytes-to-Loom-object mapping, then
enforce the CID's codec and multihash before caching or serving a block.

`/.loom/ipfs/...` is useful as a reserved, implementation-managed inspection and mount projection.
It must not make cached remote content ordinary Files data. Normal Files, version-control, mount, diff, and sync
operations must neither implicitly walk nor mutate the cache. The public contract belongs to an IPFS
facade through CLI, bindings, MCP tools, and configured gateway or peer operations. The reserved path is
for controlled inspection, not the primary data model.

### UnixFS Compatibility, IPLD, And Publication Correctness

Ordinary Loom files and directories do not automatically have IPFS-compatible UnixFS CIDs. To publish
them, Loom needs a pinned importer profile that makes these choices explicit and testable:

| Importer decision | Why it changes behavior | Required Loom policy |
| --- | --- | --- |
| Content identifier version and multihash | Changes the root CID and verification behavior | Declare and version the selected profile |
| Raw leaves or wrapped leaves | Changes file DAG shape and interoperability | Declare a stable default per publication profile |
| Chunker and chunk size | Changes file block boundaries and CID | Pin fixed or Rabin parameters exactly |
| Balanced or trickle layout | Changes directory and file DAG structure | Pin layout per profile |
| Directory names, metadata, and symbolic links | Changes UnixFS representation and can introduce unsafe host semantics | Canonical mapping plus explicit reject or preserve rules |
| CAR roots and ordering | Changes interchange verification and reproducibility | Support CAR version 1 and version 2 with canonical validation tests |

The [IPFS file-system documentation](https://docs.ipfs.tech/concepts/file-systems/) explains that
UnixFS files are chunked DAGs and that chunker and layout choices affect the resulting CID. The
[IPLD CAR specification](https://ipld.io/specs/transport/car/) defines CAR version 1 and version 2 as the
interchange framing. These are contract inputs, not incidental implementation choices.

### Rust Implementation Evidence And Size Probes

The probes are isolated from the workspace at `prototypes/size-probes/` and use stripped, thin-LTO
release binaries. They establish compile and binary-footprint evidence only. They do not establish
protocol conformance, security review, runtime throughput, or suitability as a final dependency.

| Candidate | Probe path | Linked exercise | Release size | Delta from empty control | Transitive normal dependencies | Decision use |
| --- | --- | --- | ---: | ---: | ---: | --- |
| `rust-ipfs` 0.15.0 | `compare-ipfs-size.sh` | Starts an in-memory node with default DHT, Bitswap, and pubsub behavior | 12,637.7 KiB | 12,306.6 KiB | 459 | Full-node cost evidence only. The crate's own README calls it alpha/WIP, so do not adopt solely from this result. |
| `co-libp2p-bitswap` 0.27.0 | `compare-bitswap-size.sh` | Constructs the maintained direct Bitswap behavior against a minimal asynchronous block-store adapter | 347.9 KiB | 16.8 KiB | 188 | Component-level cost evidence. Requires separate DHT, peer lifecycle, store, routing, and interop ownership. |
| `arti-client` 0.43.0 | `compare-tor-size.sh` | Creates an embedded unbootstrapped Tor client with Tokio, rustls, and onion-service-server features | 5,450.0 KiB | 5,119.0 KiB | 510 | Validates the single-binary embedded route and onion-service direction. |

The historical direct crates `libp2p-bitswap` 0.25.1 and `libp2p-bitswap-next` 0.26.4 were also
checked. Both resolve through yanked `core2` releases and cannot be freshly resolved by Cargo in
this environment. The probe deliberately uses `co-libp2p-bitswap` 0.27.0 instead, whose current
dependency graph resolved and compiled. This is evidence to assess that fork, not a final adoption
decision. `rust-ipfs` was source-checked separately and exposes its own Bitswap, Kademlia, and
pubsub composition, but its README explicitly labels the project alpha.

### Tor And Onion-Service Design

Arti is the leading embedded Rust choice because it can provide outbound Tor client routing and
onion-service support within a Loom binary. Its [project introduction](https://arti.torproject.org/)
and [FAQ](https://arti.torproject.org/FAQs/) describe client and onion-service support while noting
that it is not a Tor relay implementation. Loom does not need to operate a relay to provide the
planned route and listener exposure features.

| Concern | Required design |
| --- | --- |
| Inbound exposure | `--exposure onion` asks the hosted listener manager to publish a version 3 onion service that forwards accepted streams into the existing listener protocol stack. |
| Outbound routing | `--route tor` asks a client operation to create outbound TCP streams through an embedded Tor client. |
| External infrastructure | Allow an explicit SOCKS or external-Tor provider override for operators that require centralized Tor management. The default target remains embedded Arti for the single-binary product. |
| Keys | Onion-service identity keys are local encrypted operational state. They are never Loom principals, TLS keys, store encryption keys, normal commits, or sync payloads. |
| Authorization | Onion address secrecy and optional Tor client authorization are separate from Loom principal authentication and PEP authorization. Every request still passes the hosted kernel. |
| Listener policy | Request-size, connection, idle, session, rate, and audit policies apply equally to onion-originated traffic. |
| TLS | Preserve application-level TLS when a client protocol expects it. Tor circuit encryption does not remove a protocol's compatibility or end-to-end identity requirements. |
| Isolation | Bind outbound route isolation to a declared principal, operation, or service context. Do not reuse a Tor circuit across unrelated principal identities by default. |

The [Tor onion-service overview](https://community.torproject.org/onion-services/overview/index.html)
explains the location-hiding and self-authenticating properties of onion services. These properties
are valuable exposure controls, but they are not access control. Private onion access can later add
v3 client authorization as a second network-level gate; it must not bypass the existing principal
and authorization model.

### Deployment Options For Tor

| Option | Behavior | Example | Assessment |
| --- | --- | --- | --- |
| Embedded Arti only | Loom carries the Tor client and creates onion services itself | `loom serve configure app.loom imap mail --bind 127.0.0.1:1143 --exposure onion` | Best standalone experience, but upgrades and operational policy move with Loom releases. |
| External Tor or SOCKS only | Loom delegates routing and possibly onion-service management to a separately operated daemon | `loom sync pull app.loom origin/main --route socks5://127.0.0.1:9050` | Fits centrally managed environments, but violates the standalone default and cannot by itself create an onion service. |
| Embedded default plus external override | Loom defaults to Arti but permits a declared external provider where required | Both examples above, with an explicit provider configuration | Recommended enterprise direction. It preserves the single-binary default without rejecting managed-network deployments. |

### Priority And Crate Boundaries

Prioritize Tor before IPFS because it is a shared hosted-runtime overlay with immediate value for every
served surface and a bounded single-binary implementation path. IPFS is a larger networked data
domain that requires CID catalog, policy, retrieval, UnixFS conversion, pinning, and interoperability work.

| Order | Work | Owning boundary | Reason |
| --- | --- | --- | --- |
| 1 | Shared route and onion exposure model, encrypted local identity state, and hosted listener integration | Dedicated network-overlay crate, not `loom-hosted` or IPFS | Reusable by IMAP, S3-compatible object service, PIM, Studio, sync, and future facades |
| 2 | Embedded Arti route client and onion-service listener bridge | Same network-overlay crate plus hosted adapter | Keeps application protocol handlers independent from Tor mechanics |
| 3 | IPFS foreign-CID catalog, verified HTTP gateway cache, and CAR validation | Dedicated `loom-ipfs` facade crate with a reusable IPLD and CID substrate if later justified | Gateway cache is the first IPFS profile and does not require peer-network ownership |
| 4 | IPFS retrieval node with routing and Bitswap | `loom-ipfs` | Adds peer budgets, routing, peer lifecycle, and cache behavior after catalog correctness exists |
| 5 | Provider and publisher profiles | `loom-ipfs`, artifact links where explicitly selected | Requires pinned UnixFS profile, explicit publication authority, reprovide operations, and durable job control |

The current recommendation is an embedded-Arti default with an explicit external provider override,
and a gateway-cache-first IPFS facade. Neither introduces a new `FacetKind` yet. Tor is an overlay
that applies to all facades. IPFS should first be designed as a first-class compatibility facade
with its own storage boundaries; whether its durable pin/publication metadata belongs beside the
planned artifact facet or deserves a dedicated native substrate remains a later API decision.

### Distribution And Compile-Feature Contract

Every normal Loom distribution must support configuration-only IPFS and Tor behavior. That means it
understands the public command-line grammar, validates and persists the complete durable configuration,
returns it through CLI, bindings, MCP tools, and administration surfaces, and preserves it during store
copy, import, export, and schema migration. A configuration-only binary must not link an IPFS node,
Bitswap implementation, Arti, or a system-Tor adapter.

Full network behavior is opt-in at compilation through separate `ipfs` and `tor` Cargo features. A
server distribution can enable either feature independently or both together. `ipfs` enables the
selected IPFS runtime behind the IPFS facade. `tor` enables the shared route and onion-service
runtime behind `--route tor` and `--exposure onion`. Neither feature implies the other.

| Distribution state | Configuration operations | Activation behavior | Prohibited behavior |
| --- | --- | --- | --- |
| Default binary, neither feature | Parse, validate, save, list, inspect, import, export, and migrate IPFS and Tor configuration | `enable`, daemon reload, or an on-demand operation returns the stable compiled-capability error and identifies the missing feature | Binding a listener, bootstrapping Tor, creating an onion key, fetching an IPFS block, contacting a gateway or peer, or rewriting the saved configuration |
| `ipfs` enabled only | All configuration operations | Starts only supported IPFS profiles and operations | Treating `--route tor` or `--exposure onion` as active |
| `tor` enabled only | All configuration operations | Starts Tor-routed clients and onion-service listener projections | Treating IPFS cache, retrieval, provider, or publication profiles as active |
| `ipfs` and `tor` enabled | All configuration operations | Starts each explicitly enabled runtime; IPFS can select Tor for outbound requests only when its operation declares `--route tor` | Automatically publishing IPFS data, enabling a saved listener, or treating onion reachability as Loom authorization |

The configuration schema, stable error codes, capability reporting, and listener-record decoding
must live in dependency-light crates available to every distribution. Feature-gated runtime adapters
live behind those contracts. This keeps a `.loom` file portable between desktop, command-line, embedded, and
server distributions: a feature-disabled reader can preserve administrator intent, and a
feature-enabled server can later activate the same configuration without a translation step.

Configuration-only mode is deliberately not a best-effort runtime mode. `loom serve configure ...
--exposure onion` and a future IPFS configuration command may succeed on a normal binary because they
persist declarative desired state. `loom serve enable ...`, daemon reconciliation, or a request that
would use that state must fail before side effects when the matching compile feature is absent.
Capability output must show at least `configured`, `compiled`, `available`, and a stable reason when
unavailable. This prevents an operator from mistaking saved configuration for an active listener or
network node.

## Appendix B - Graph Query And Compatibility Direction

### Approved Sequence

The graph delivery order is explicit:

1. Promote the native graph substrate from one whole graph blob to structured canonical graph storage.
2. Define and implement a bounded GQL-aligned openCypher query contract over that native substrate.
3. Promote `neo4j` as a first-class target served surface only after the query contract, Bolt
   handshake and sessions, result framing, error mapping, and official-driver conformance define an
   accurate compatibility boundary.
4. Keep Gremlin cut from the active graph roadmap unless a later owner-approved design session
   reopens it.

This order is an enterprise correctness and performance requirement. The current graph model has
structured graph roots, deterministic node and edge create, read, update, delete, and merge
operations, traversal, bounded GQL-aligned openCypher read and mutation parsing, fixed and bounded
variable paths, deterministic regex predicates, canonical list and map graph values, a closed
function subset, and core property-index explain/readiness. Persistent derived-artifact
orchestration, public property-index projection, and broader Neo4j compatibility remain target work.

### Grammar Decision

The first declarative grammar is a bounded, GQL-aligned openCypher profile. `graph` owns the native
query semantics and canonical graph data. The initial contract must name its exact supported subset
rather than claim full GQL, full Cypher, Neo4j, or Bolt compatibility.

GQL is ISO/IEC 39075:2024. openCypher is the practical compatibility bridge because its language
resources include grammar material and a TCK, while Cypher's familiar
`MATCH`, `WHERE`, and `RETURN` shape is close to GQL. The openCypher query language is also a broadly
adopted property-graph contract. Neo4j compatibility is first-class because official Neo4j drivers and
APIs create product-level expectations. Gremlin is cut from the active roadmap.

| Query concern | Native graph substrate | GQL/openCypher adapter | Neo4j compatibility surface |
| --- | --- | --- | --- |
| Graph model | Typed nodes, directed edges, labels, typed properties, identity | Pattern variables and labels | Neo4j-shaped nodes, relationships, paths, records, and type mappings over native graph values |
| Navigation | Forward and reverse adjacency, bounded path expansion | Pattern matching and variable-length paths | Neo4j Cypher subset mapped through the same bounded native IR |
| Filtering | Typed comparisons, property lookup, null and cardinality policy | `WHERE` expressions and pattern predicates | Neo4j-compatible Cypher behavior only where the matrix proves parity |
| Results | Ordered rows, cursoring, projection, result limits | `RETURN`, grouping, aggregation, sorting | Bolt records, metadata, and driver-visible summaries where supported |
| Mutation | Authorized atomic graph mutation plan | `CREATE`, `MERGE`, `SET`, and `DELETE` | Neo4j transaction/session semantics over authorized native mutation plans where supported |
| Safety | Depth, fanout, candidate, row, byte, time, and memory budgets | Bounded declarative planner and abstract-syntax-tree validation | Driver-facing errors and unsupported-feature reports mapped from Loom limits |

The shared native query and traversal intermediate representation is the architectural boundary.
Neo4j compatibility lowers into it and adds product session behavior. Loom must not let the Bolt
handshake, driver protocol, or Neo4j catalog expectations define canonical graph identity.

### AI And Agent Posture

GQL/openCypher is the first text grammar because a declarative graph pattern is easier to generate,
validate, explain, constrain, and cost-bound from natural-language intent. An agent should produce a
typed query abstract syntax tree or a bounded GQL/openCypher string that is parsed into that abstract
syntax tree. Neo4j compatibility is for official drivers and client tools. Gremlin is cut from the
active agent and served-surface roadmap.

### Compatibility Boundaries

| Surface | Placement | Status |
| --- | --- | --- |
| Native graph traversal | `graph` REST endpoints, JSON-RPC endpoints, bindings, and MCP tools | Source-backed bounded subset |
| Bounded GQL/openCypher | Native `graph` query contract and a declared compatibility profile | Source-backed read subset, native mutation-plan substrate including `MERGE`, mutation text lowering through explicit Loom identity, deterministic mutation identity derivation for bounded Neo4j `CREATE`/`MERGE`, fixed and bounded variable paths, deterministic regex predicates, canonical list/map values, closed function subset, and core property-index explain/readiness; public property-index projection and persistent derived-artifact orchestration remain target work |
| Neo4j | First-class `neo4j/tcp` compatibility surface over native graph | Compatibility matrix, native graph capability rows, durable listener admission, daemon runtime opening, bounded Bolt 5.1 session skeleton, bounded read `RUN` plus `PULL`, bounded auto-commit `CREATE`/`MERGE` write `RUN`, result/error mapping for the bounded subset, official Python/JavaScript driver transcripts, and deterministic unsupported transaction behavior are source-backed. Catalog/procedure shims and broader driver conformance remain target |
| Gremlin | Cut from active scope | Reopen only through a separate owner-approved design session |
| Full Neo4j product behavior | Compatibility surface expands only when matrix and transcripts prove behavior | Not implied by `neo4j/tcp` admission |

## Appendix C - Whole-Blob Facet Storage Promotion

### Target Storage Contract

Whole-blob storage is a current source boundary, not the enterprise end state for high-cardinality or
frequently updated collections. The target is not a proliferation of unrelated files. Each promoted
facet has one compact, canonical root manifest that declares its format version, logical schema,
policy, and immutable component-root digests. The workspace commits that root. Component trees store
units by their logical identity and structurally share unchanged blocks across commits, branches,
clone, and sync.

| Requirement | Target behavior |
| --- | --- |
| Canonical root | A versioned root manifest references every semantic source component and declares the format, schema, policy, key codec, comparator, and applicable merge rules. |
| Unit addressing | Logical units are independently addressable: keys, documents, points, entries, nodes, edges, source records, or immutable segments. |
| Structural sharing | A mutation rewrites only changed leaves and ancestors. Digest-equal subtrees remain shared between versions. |
| Query and scan | Ordered or adjacency-aware component trees support bounded point lookups and range scans without collection-wide decode. |
| Diff and merge | Version control skips digest-equal subtrees, reports unit-level changes, and applies an explicit per-unit merge policy. |
| Derived artifacts | Query indexes, ANN structures, caches, query plans, statistics, and engine state are rebuildable. Their configuration may be canonical; their physical materialization is not. |
| Garbage collection and retention | Reachability begins at canonical roots. Retention, tombstone, checkpoint, compaction, and prune policy are explicit semantic metadata. |
| Conformance | Canonical root bytes, key codecs, ordering, negative decode cases, migration, and stable errors receive vectors before a storage promotion is declared complete. |

### Target Facet Shapes

| Facet | Current source shape | Enterprise canonical roots | Derived or operational state |
| --- | --- | --- | --- |
| `kv` | Structured prolly root over content-addressed value components | Typed-key ordered map root and map policy | Cache only |
| `document` | Structured canonical root: `DocumentCollectionManifest` referencing a document-identifier map of canonical records, content-addressed bodies (`body_ref`), and a canonical index-declaration catalog (`index_catalog_root` load-bearing/verified) | Prolly-tree document map for structural sharing/sublinear diff, retained tombstones, and chunked large bodies | Secondary-index materializations |
| `time-series` | Structured time-series root (`StagedEntry::TimeSeries`): metadata plus a point-field tree and rollup roots, versioned through the engine | Time-partitioned point tree plus retention and rollup policy | Rollups and query acceleration |
| `ledger` | Segment-native root Tree with manifest, head metadata, segment index, and current immutable segment roots | Sequence-range immutable segment roots, head metadata, checkpoint and retention policy | Proof caches, witness, and delivery state |
| `graph` | Structured graph root (`StagedEntry::Graph`): separate node map, edge map, and forward and reverse adjacency roots, versioned through the engine | Node map, edge map, forward adjacency, reverse adjacency, and declared property-index catalog | Property-index materializations and query plans |
| `search` / `fts` | Structured FTS source root that separates canonical source documents and mapping from the rebuildable engine index | Source-document map, mapping, analyzer, and index-policy declaration | Tantivy or other inverted index |
| `columnar` | Structured columnar root (`StagedEntry::Columnar`): dataset manifest plus durable native-CBOR segment payloads (durable Parquet segment references remain target) | Dataset manifest, Arrow schema, partition metadata, and immutable Parquet segment references | Statistics, result caches, and compaction jobs |
| `dataframe` | One canonical logical plan per named dataframe | The plan record remains canonical; its declared source bindings and materialization policy are part of the plan | Engine execution state uses `materialization:<materialization-id>` derived-artifact records; committed materialized outputs live in `columnar`, `files`, or `cas` |

### Existing Structured Baselines

Not every facet requires a promotion. SQL committed tables already use prolly-tree rows, even though
compact in-process table encoding remains useful for results and tests. Queue streams already use a
structured stream root plus sequence-keyed entries. Vector sets use a manifest plus one source entry
per vector identifier. Content-addressed storage is file-per-digest. Calendar, contacts, and mail are record-addressed structured
stores. These forms are evidence for the target direction, but each promoted root still needs its own
canonical format and conformance rules.

### Promotion Rule

Do not add an engine side store to avoid a canonical storage decision. A storage accelerator that is
invisible to commits, diff, merge, clone, sync, and garbage collection cannot become the source of
truth. Promote structured storage facet by facet when the identity, ordering, merge, retention, and
cross-language contract can be pinned together. This preserves the versioned Loom model while giving
enterprise workloads bounded reads, incremental writes, sublinear diffs, trustworthy merge behavior,
and operationally safe retention.

### Closure Gate For Whole-Blob Promotions

A whole-blob promotion ticket is not complete when it merely names the target root or points at an
existing structured implementation. Closure requires source-backed evidence for every gate below,
recorded on the ticket that requests the promotion. If any gate is missing, the ticket remains open
and the missing work is classified as closure-blocking promotion debt, P1 facet primitive work, or
compatibility/profile work.

| Gate | Closure evidence required | Classification when missing |
| --- | --- | --- |
| Canonical root contract | The owning spec names the root manifest, version tag, digest inputs, default values, ordering, key codec, comparator, component references, policy fields, and migration behavior. | Closure-blocking promotion debt |
| Source boundary | The owning spec separates canonical source components from derived indexes, caches, query plans, statistics, materializations, and engine state. | Closure-blocking promotion debt |
| Unit addressing | Source code or an accepted design packet shows that logical units can be addressed independently without decoding the full collection. | Closure-blocking promotion debt |
| Incremental mutation | Source code or an accepted design packet shows that a mutation rewrites only changed leaves and ancestors, with digest-equal subtree sharing preserved. | Closure-blocking promotion debt |
| Diff and merge semantics | The owning spec defines unit-level diff output, merge policy, conflict behavior, tombstones or delete markers, and stable errors. | Closure-blocking promotion debt |
| Query and scan behavior | The owning spec defines point lookup, range scan, adjacency traversal, or partition scan behavior needed by the facet, including limits and unavailable-feature reporting. | P1 facet primitive work |
| Retention and garbage collection | The owning spec defines reachability, tombstone retention, checkpoint, compaction, prune policy, and physical reclamation boundaries. | P1 facet primitive work |
| Migration and compatibility | The owning spec defines legacy decode, old-root migration, unsupported versions, and mixed-version behavior across commits, clone, sync, and bindings. | Closure-blocking promotion debt |
| Conformance proof | Canonical byte vectors, negative decode vectors, migration vectors, operation tests, diff or merge tests, and cross-language vectors where public are named and either source-backed or queued with exact ownership. | Closure-blocking promotion debt |
| Product-profile boundary | Any S3, OCI, OpenSearch, Neo4j, PostgreSQL, MySQL, Redis, Kafka, or similar facade claim is documented as a compatibility profile over the promoted native root, with unsupported behavior kept explicit. | Compatibility/profile work |

Whole-blob promotion packets must leave a short closure table on the active ticket:

| Field | Required content |
| --- | --- |
| `promotion_scope` | The facet, collection, or root being promoted. |
| `source_anchors_checked` | Files and sections used as evidence, including code anchors when source-backed behavior is claimed. |
| `closure_gates` | One row per gate with `source_backed`, `accepted_design`, `missing`, or `not_applicable`. |
| `remaining_work_classification` | Missing work grouped into closure-blocking promotion debt, P1 facet primitive work, and compatibility/profile work. |
| `decision_points` | Owner decisions in the required decision format, or `none`. |

No worker may close or accept a whole-blob promotion while any closure-blocking promotion debt remains.
Compatibility/profile work can follow after the native promotion gate is complete, but it cannot be
used as evidence that the native root is promoted. P1 facet primitive work can remain separate only
when the promotion is honestly scoped away from that primitive and the owning spec says which later
ticket owns it.

## Appendix D - Best Areas, Worst Areas, And Highest-Return Wins

This appendix summarizes the design posture that emerges from the primitive inventory. It is not a
source implementation claim. It is a planning analysis of where Loom is strongest, where the design
still carries the most risk, and which follow-on work gives the highest return on investment.

### Best Areas

| Area | Why it is strong | Why it matters |
| --- | --- | --- |
| Facet and facade separation | The document consistently separates native primitives from compatibility facades, transports, engines, and interchange formats. Redis, OpenSearch, S3, PostgreSQL, MySQL, Kafka, and similar systems are treated as compatibility surfaces over native contracts rather than as the native contracts themselves. | This is the right enterprise posture because it prevents product compatibility from distorting Loom identity, storage, authorization, and conformance. |
| Content-addressed storage | Native content-addressed storage has a clear boundary from S3, OCI Distribution, CAR, archive formats, artifact transfer, IPFS, and retention policy. | This is one of Loom's strongest conceptual cores. It gives many product surfaces a reusable byte-identity layer without making any one product protocol define Loom storage. |
| FTS | FTS has a strong split between native collections and query semantics, OpenSearch compatibility, Tantivy as an optional native engine, derived artifact identity, aliases, analyzers, and future whole-Loom search. | This is commercially useful and technically disciplined. It can serve real client demand while keeping the native FTS model independent from OpenSearch and Tantivy internals. |
| Dataframe and columnar direction | The split between dataframe plans, columnar storage, Apache Arrow, Apache Parquet, Polars acceleration, materialization, and analytical presentations is coherent. | This can become one of Loom's highest-value data surfaces if implementation keeps logical plans, durable analytical storage, and execution engines distinct. |
| PIM | Calendar, contacts, and mail now share primitives for collection identity, synchronization, entity tags, delegated sharing, derived indexes, push delivery, and capability reporting. | This makes the PIM stack more than protocol wrappers. It gives Loom a native user-data domain that can project to standards clients without making those standards own identity. |
| Control-plane shared primitives | Store lifecycle, serve listener registry, daemon coordination, locks, leases, audit, retention, capability reporting, and maintenance jobs are identified as shared infrastructure. | This is the right place to reduce duplication. If implemented centrally, facets will not each build private listener records, lock models, job runners, capability flags, or audit behavior. |
| Program and execution boundary | WASM execution, CEL programs, constrained action envelopes, guards, derivations, workflows, statecharts, and trigger boundaries are described as distinct execution primitives. | This gives agents and automation a path to persistent, inspectable programs without letting expression-language mutation bypass Loom authorization or storage policy. |
| Versioned storage model | The document repeatedly reinforces that canonical roots, source anchors, structured storage, and derived artifact exclusion are central to Loom's identity. | This protects diff, merge, clone, sync, garbage collection, and conformance from being quietly redefined by engines or external systems. |

### Weakest Areas

| Area | Problem | Risk |
| --- | --- | --- |
| Large target surface | Many areas are well designed but still target work: derived indexes, maintenance jobs, complete capability reporting, retention policy, delegated sharing, durable push, and compatibility matrices. | The roadmap can look complete while implementation remains thin. This makes it easy for future sessions to overstate readiness. |
| Shared infrastructure gaps | Locks, leases, maintenance jobs, capability reporting, retention, audit compaction, migration, and stable unavailable-feature reporting are needed by many facets but are not fully centralized. | Facets may grow duplicate private solutions if these primitives are not implemented early. That would create long-term maintenance and correctness debt. |
| Whole-blob storage debt | The whole-blob appendix identifies several facets that still need promotion from collection blobs to structured canonical roots. | High-cardinality collections, frequent updates, diff, merge, sync, and garbage collection will suffer if structured roots are delayed too long. |
| Compatibility breadth | Loom is aiming at OpenSearch, Redis, Memcached, etcd, Kafka, MQTT, NATS, JetStream, S3, OCI Distribution, PostgreSQL, MySQL, CalDAV, CardDAV, IMAP, JMAP, and more. | The risk is shallow compatibility across too many products instead of deep, reliable compatibility for the highest-value profiles. |
| Planned telemetry facets | Metrics, logs, and traces are approved planned facets, but they are not canonical members yet. | Telemetry has high return on investment, but ownership can drift if OTLP, Prometheus, Grafana, Influx, and generic time-series behavior continue to share partial implementations without native metrics/logs/traces contracts. |
| Product protocol proof | Many facades need transcript tests, compatibility matrices, stable unsupported behavior, conformance vectors, and capability reporting. | Clients may connect and then fail in surprising ways if Loom claims compatibility without precise support boundaries. |
| Cross-facet indexing | FTS, graph, mail, calendar, contacts, document, files, and future whole-Loom search all need source selectors, extraction, update propagation, deletion policy, and rebuild ownership. | Without one cross-facet indexing contract, search-like features will duplicate extraction and rebuild logic. |
| Retention and deletion semantics | Content-addressed storage, artifact transfer, mail, ledger, audit, IPFS, S3, and OCI Distribution all need policy-proven deletion and retention. | Deletion can become misleading or unsafe if visible deletion, policy retention, physical reclamation, and audit evidence are not separated. |
| Mount and file semantics | Files have a native base, but path normalization, metadata mapping, atomic mutations, partial writes, sparse files, locks, leases, and watch behavior still need complete conformance. | Mounts are user-visible and unforgiving. Incomplete semantics will show up quickly in real desktop and server workflows. |

### Easiest High-Return Wins

| Rank | Win | Estimated lift | Return on investment | Why this is a good first move |
| ---: | --- | ---: | ---: | --- |
| 1 | Build a single capability-reporting matrix across facets, facades, engines, compile features, and hosted listeners. | 3 | 10 | The document repeatedly needs configured, compiled, available, unsupported, denied, and limited states. A shared matrix improves command-line output, hosted administration, bindings, documentation, and operator trust without deep storage changes. |
| 2 | Convert this primitive inventory into owning-spec task lists. | 3 | 9 | The design decisions are now centralized. Moving them into the owning specifications and task queues prevents future sessions from rediscovering the same decisions or treating this appendix as the only source of unfinished work. |
| 3 | Centralize derived-artifact lifecycle. | 5 | 9 | FTS, vector, graph, dataframe, columnar, calendar, contacts, mail, and future whole-Loom search all need source anchors, rebuild state, stale state, failure reporting, and serving policy. One substrate prevents duplicated rebuild systems. |
| 4 | Define shared retained-gap and change-set semantics. | 4 | 8 | Calendar, contacts, mail, queue, ledger, delivery, and PIM push all need durable synchronization and replay behavior. This is a cross-facet multiplier. |
| 5 | Promote conditional mutation and entity tags as reusable primitives. | 4 | 8 | Files, calendar, contacts, mail, key-value, cache, and hosted protocols all need lost-update protection. A shared primitive gives consistent compare-before-write behavior and stable conflict errors. |
| 6 | Create compatibility matrices for the highest-value facades only. | 4 | 8 | OpenSearch, S3, PostgreSQL, MySQL, Redis, Memcached, IMAP, and JMAP should each have explicit supported, degraded, target, and unsupported behavior before broad claims are made. |
| 7 | Prioritize file path canonicalization and atomic file mutations. | 4 | 7 | Files are foundational for mounts, archives, sync, user workflows, and cross-facet projections. The native base exists, so path and mutation conformance is practical and high leverage. |
| 8 | Add shared background maintenance job primitives. | 6 | 9 | Rebuilds, compaction, pruning, retention, migration, cache refresh, and derived-index repair all need durable scheduling, leases, retry, cancellation, status, and audit. This is slightly larger work but has very high leverage. |
| 9 | Promote metrics as first-class before expanding telemetry facades further. | 6 | 8 | Prometheus, Grafana, OTLP, and Influx need native metric descriptors, temporality, staleness, resource identity, cardinality, histograms, exemplars, and retention behavior instead of living indefinitely over generic time-series points. |
| 10 | Tighten whole-blob promotion priorities. | 5 | 8 | Choose the worst blob-backed and high-cardinality facets first, then define canonical roots, ordering, merge behavior, retention, migration, and conformance. This protects performance and version-control behavior. |
| 11 | Define the principal signing substrate. | 5 | 8 | Ledger checkpoints, OCI signing, artifact provenance, program attestation, and agent actions all benefit from one principal-bound signing model. |
| 12 | Specify the cross-facet indexing contract. | 5 | 8 | FTS, graph, files, document, mail, calendar, contacts, and whole-Loom search need one extraction and rebuild model. This prevents many local indexing solutions. |

### Recommended Sequence

The next implementation and specification work should focus on shared primitives rather than another
product facade. Product facades are valuable, but their correctness depends on reusable substrate
work that appears repeatedly across the inventory.

| Order | Focus | Reason |
| ---: | --- | --- |
| 1 | Capability reporting matrix | This gives every facet, facade, optional engine, and compile feature a truthful operator-facing status model. |
| 2 | Derived-artifact lifecycle | This unblocks search, vector, graph, dataframe, columnar, PIM indexes, and future whole-Loom search. |
| 3 | Conditional mutation and entity tags | This gives files, PIM, key-value, cache, and hosted protocols one lost-update contract. |
| 4 | Change sets and retained gaps | This supports synchronization, push, queue progress, ledger scans, and mail/calendar/contact clients. |
| 5 | Background maintenance jobs | This centralizes rebuilds, compaction, retention pruning, migration, and cache refresh. |
| 6 | Highest-value compatibility matrices | This makes OpenSearch, S3, PostgreSQL, MySQL, Redis, Memcached, IMAP, and JMAP claims precise before implementation expands. |

### Strategic Read

Loom's strongest design advantage is its refusal to let product protocols define native storage.
The primitive inventory repeatedly chooses native identity first, then product compatibility second.
That is the right long-term architecture.

The main risk is execution sprawl. The document now names a large number of native facets, planned
facets, facades, engines, interchanges, and control-plane primitives. If implementation proceeds by
adding product surfaces one at a time, Loom will accumulate duplicate capability reporting, duplicate
rebuild logic, duplicate retention behavior, and duplicate session handling. The better path is to
implement the shared primitives that appear across many sections, then use them to make product
facades thinner and more honest.

The highest-return near-term work is therefore not another broad compatibility endpoint. It is the
shared capability, maintenance, mutation, synchronization, and retention substrate that lets many
facets become reliable at once.

## Appendix E - Cross-Facet Readiness And Risk Matrix

This appendix turns the primitive inventory into a relative readiness map. The scores are design
planning signals, not release claims. They help identify which areas are conceptually strong, which
areas are blocked by missing shared primitives, and which areas would be risky to market as
compatible before conformance evidence exists.

Score meanings:

- Source-backed strength estimates how much of the native behavior is described as already
  source-backed in this inventory.
- Missing primitive risk estimates how many reusable primitives still need promotion before the
  area can become enterprise-reliable.
- Compatibility proof risk estimates how risky it would be to claim compatibility without transcript
  tests, client matrices, negative tests, stable unsupported behavior, and capability reporting.
- Action priority is the recommended pressure to act soon, based on ROI, dependency value, and
  risk reduction.

| Area | Source-backed strength | Missing primitive risk | Compatibility proof risk | Action priority | Readiness read |
| --- | ---: | ---: | ---: | ---: | --- |
| `files` | 6 | 8 | 8 | 9 | The native base exists, but mounts and file clients will expose path, metadata, atomic mutation, partial write, lock, and watch gaps quickly. |
| `vcs` | 7 | 6 | 6 | 7 | Versioning is a core strength, but cross-facet structural diff, merge, and product exchange behavior must remain explicit. |
| `sql` | 7 | 6 | 8 | 8 | Native relational work is strong enough to justify PostgreSQL and MySQL focus, but client protocol proof and cursor/result behavior are still high-risk areas. |
| `kv` | 7 | 6 | 7 | 8 | Native maps are useful, but conditional mutation, leases, watch semantics, and cache separation need shared contracts before etcd and broad Redis claims grow. |
| `cache` | 5 | 7 | 7 | 8 | The planned facet is well motivated because volatility, TTL, eviction, and capacity policy should not distort durable `kv`. |
| `document` | 4 | 8 | 8 | 8 | The native target is clear, but indexes, bounded query, mutation semantics, and cursor behavior should come before MongoDB or Couchbase marketing. |
| `vector` | 8 | 5 | 7 | 7 | Native vector storage and exact search are comparatively strong; Qdrant and Pinecone need API key security, capability matrices, and client transcript proof. |
| `graph` | 7 | 7 | 8 | 8 | The native structured direction is now strong, but property-index materialization, broader grammar conformance, and Neo4j compatibility remain proof-heavy. Gremlin is cut from active scope. |
| `fts` | 8 | 6 | 8 | 9 | FTS is high-value and well separated from OpenSearch and Tantivy; analyzer, highlighting, aggregation, and cross-facet indexing need careful conformance. |
| `columnar` | 6 | 7 | 7 | 8 | Arrow and Parquet direction is coherent, but durable segment policy, compaction, high-volume transfer, and analytical client profiles remain important gaps. |
| `dataframe` | 7 | 6 | 6 | 8 | Native frame plans and portable execution are useful; high-volume transfer, materialization ownership, and analytical presentation profiles are next. |
| `queue` | 6 | 8 | 9 | 9 | Queue has strong native direction, but Kafka, MQTT, Redis Streams, NATS, claims, offsets, and delivery semantics all depend on shared coordination and delivery. |
| `time-series` | 6 | 7 | 7 | 7 | Point storage is useful, but metrics/logs/traces promotion, retention, rollups, and query semantics should be settled before expanding telemetry facades. |
| `metrics` | 4 | 8 | 8 | 9 | Metrics should be promoted before Prometheus, Grafana, OTLP, and Influx expand further over generic time-series points. |
| `logs` | 3 | 8 | 7 | 7 | Logs are approved but still need native event identity, retention, indexing, redaction, and trace correlation before OTLP or Grafana Explore claims mature. |
| `traces` | 3 | 8 | 7 | 7 | Traces need native span identity, links, events, exemplars, sampling, and retention before cross-signal telemetry can be strong. |
| `cas` | 8 | 6 | 8 | 8 | Native byte identity is strong, but S3, OCI Distribution, CAR, artifact, IPFS, retention, and deletion semantics need precise profile boundaries. |
| `ledger` | 5 | 8 | 6 | 8 | Ledger should lean into Loom-native append-only truth, signing, proofs, witness policy, and structured storage rather than product cloning. |
| `program` | 7 | 7 | 6 | 8 | WASM and CEL direction is useful for agents, but action envelopes, signing, authoring flow, and CLI/MCP/bindings projection need tight ownership. |
| `calendar` | 8 | 5 | 7 | 7 | Native record storage is strong; delegated sharing, retained gaps, push, and broader CalDAV compatibility remain the key gaps. |
| `contacts` | 8 | 5 | 7 | 7 | Native records are strong; groups, delegated sharing, derived search indexes, and CardDAV breadth remain target work. |
| `mail` | 7 | 6 | 8 | 8 | IMAP/JMAP foundations are valuable, but mailbox sync, threading, search, push, and standards client matrices need more proof. |

### Readiness Conclusions

| Conclusion | Direction |
| --- | --- |
| Strongest native foundations | `cas`, `fts`, `vector`, `calendar`, `contacts`, `sql`, `vcs`, and `program` have enough native shape to build outward if compatibility claims stay bounded. |
| Highest shared-substrate dependency | `queue`, `metrics`, `document`, `files`, `ledger`, `graph`, and `columnar` depend heavily on shared primitives before broad facade expansion is wise. |
| Highest compatibility-proof risk | `queue`, `files`, `sql`, `document`, `fts`, `cas`, and `mail` need explicit client matrices and negative tests before broad compatibility language is safe. |
| Best near-term enterprise leverage | Capability reporting, derived artifacts, conditional mutation, change sets, maintenance jobs, retention policy, and principal signing unlock multiple rows at once. |

## Appendix F - Conformance Strategy Matrix

This appendix records the proof shape needed before a primitive or facade should be called complete.
The goal is not to add tests everywhere immediately. The goal is to stop future work from declaring
compatibility based on a happy-path route or a single native call.

| Proof type | What it proves | Required for | Failure if omitted |
| --- | --- | --- | --- |
| Canonical byte vectors | Encoded identity, digest inputs, ordering, default values, and version tags are stable across languages and releases. | Native roots, structured storage promotions, manifests, program records, ledger entries, dataframe plans, columnar schemas, and content-addressed mappings. | A later implementation can change identity silently and break clone, sync, merge, bindings, or stored data. |
| Negative decode vectors | Invalid, ambiguous, non-canonical, oversized, duplicate, or forbidden encodings fail with stable errors. | Every canonical format, import format, and wire-to-native lowering path. | A decoder can accept data that another binding rejects, creating portability and security drift. |
| Operation transcript tests | Real request/response sequences match the declared protocol profile. | OpenSearch, S3, OCI Distribution, PostgreSQL, MySQL, Redis, Memcached, IMAP, JMAP, CalDAV, CardDAV, Kafka, MQTT, NATS, Prometheus, OTLP, Qdrant, and Pinecone. | A route can pass unit tests while real clients fail on handshake, envelope, error, cursor, or session behavior. |
| Capability matrix tests | `supported`, `unsupported`, `degraded`, `denied`, `disabled`, `unavailable`, and `target` states are reported consistently, with compile absence and runtime absence represented as `unavailable` plus registry-backed subcause reason codes. | Every hosted surface, engine, feature-gated runtime, binding, CLI command, and administration endpoint. | Operators cannot distinguish missing support from auth denial, runtime absence, compile absence, or temporary unavailability. |
| Policy enforcement tests | Authorization, PEP checks, API keys, app passwords, principal signing, audit, and redaction are applied before sensitive work happens. | Hosted routes, compatibility facades, background jobs, sync, push, import/export, and agent execution. | A facade may bypass shared security by calling lower-level storage directly. |
| Resource-limit tests | Bounds on request size, rows, bytes, time, fanout, cardinality, regex cost, result count, and memory are enforced with stable errors. | Query engines, analytical surfaces, search, graph traversal, vector search, telemetry, mail search, import/export, and hosted listeners. | A compatibility route can become a denial-of-service path or produce non-deterministic failures. |
| Differential client tests | A declared Loom profile is compared against known clients or reference behavior where appropriate. | Product facades with existing ecosystems such as PostgreSQL, MySQL, S3, OpenSearch, Redis, IMAP, CalDAV, CardDAV, Prometheus, and Grafana. | Loom may claim compatibility that only works for hand-built calls, not real client libraries. |
| Cross-language binding vectors | The same operation and data shape work through Rust, C ABI, IDL, CLI, MCP, hosted routes, and selected language bindings. | Native facets promoted into public APIs or stable bindings. | One surface can expose behavior that another cannot represent, creating permanent contract skew. |
| Migration vectors | Old records upgrade to current records, unsupported versions fail predictably, and rewritten records are canonical. | Served listener records, structured roots, lock records, derived-artifact metadata, ledger entries, profile configs, and any promoted storage format. | Stores can become unopenable or subtly rewritten into different logical state. |
| Rebuild and recovery tests | Derived artifacts can be detected as stale, rebuilt, failed, retried, and ignored without corrupting source truth. | FTS, vector accelerators, graph indexes, dataframe materializations, columnar statistics, calendar/contact/mail indexes, metrics rollups, and IPFS caches. | Engine state can masquerade as source data or serve stale results without disclosure. |
| Retention and deletion tests | Visible deletion, retention lock, legal hold, physical reclamation, audit evidence, and integrity reads are separated. | Content-addressed storage, S3, OCI Distribution, artifact transfer, mail, ledger, audit, IPFS, logs, metrics, and traces. | Users may believe data is gone when policy still retains it, or lose data that policy should preserve. |

### Minimum Completion Bar By Area

| Area | Minimum proof before complete |
| --- | --- |
| Native storage primitive | Canonical byte vectors, negative decode vectors, migration vectors, operation tests, and cross-language binding vectors where public. |
| Compatibility facade | Operation transcripts, capability matrix tests, stable unsupported behavior, policy enforcement tests, resource-limit tests, and at least one real-client or differential test when a mature client ecosystem exists. |
| Optional engine | Source-equivalence tests, reduced/degraded reporting, rebuild and recovery tests, compile-feature capability tests, and no source-identity dependence on engine artifacts. |
| Hosted listener | Listener configuration migration, startup reconciliation, enable/disable/remove behavior, auth denial audit, policy enforcement, request limits, stable errors, and capability reporting. |
| Import/export format | Canonical accepted profile, negative inputs, round trips, path or identity safety, size bounds, and explicit unsupported variants. |

## Appendix G - Shared-Substrate Dependency Graph

This appendix identifies primitives that should be built once and reused. These are the places where
Loom risks long-term duplication if each facet implements a private version.

| Shared substrate | Unlocks | Depends on | Direction |
| --- | --- | --- | --- |
| Capability reporting | CLI, MCP, bindings, hosted admin, serve registry, optional engines, compile features, product facades, and documentation. | Stable facet/facade registry, feature registry, runtime probes, policy denial shape, and versioned capability records. | Build one capability model that uses the 0010 states `supported`, `degraded`, `disabled`, `unavailable`, `denied`, `unsupported`, and `target`; compile absence and runtime absence are not states, but `unavailable` records with registry-backed subcause reason codes. |
| Derived-artifact lifecycle | FTS indexes, vector ANN/PQ artifacts, graph property and spatial indexes, dataframe materializations, columnar Arrow projections/statistics, metrics rollups, PIM search indexes, and IPFS cache indexes. | Source anchors, engine stamps, format stamps, rebuild jobs, stale detection, failure records, and serving policy. | Canonically defined in 0005 section 8.2 and implemented by `loom-store::derived` (source-backed today for search artifacts, vector ANN/PQ records, graph property and spatial index records, dataframe materialization records, PIM derived-index records, and columnar Arrow projection records). Treat derived bytes as rebuildable local state unless an owning spec promotes them into canonical roots. |
| Background maintenance jobs | Rebuilds, compaction, retention pruning, migration, cache refresh, rollup generation, index repair, provider reannounce, and audit compaction. | Locks, leases, durable job records, retry policy, cancellation, progress, audit, and capability reporting. | Build a shared job substrate before each facet grows a private repair loop. |
| Conditional mutation and entity tags | Files, WebDAV, CalDAV, CardDAV, IMAP/JMAP state, `kv`, `cache`, document updates, hosted resources, and S3 conditional writes. | Canonical bytes, version tokens, compare-before-write errors, conflict policy, and audit. | Promote one lost-update primitive and let product facades map their conditional headers or tokens onto it. |
| Change sets and retained gaps | Calendar sync, contact sync, mail deltas, queue consumers, ledger scans, delivery replay, watch streams, and PIM push. | Sequence identity, tombstones, authorization filtering, compaction, retained-gap errors, and replay bounds. | Define one retained-gap model so clients know when incremental sync must fall back to a full read. |
| Retention and deletion policy | Content-addressed storage, S3 lifecycle, OCI Distribution deletion, artifact transfer, mail retention, ledger retention, audit retention, logs, metrics, traces, and IPFS cache pruning. | Policy records, legal hold, visible deletion, physical reclamation, proof of deletion, proof of retention, and audit. | Separate user-visible deletion from physical removal and from policy-retained evidence. |
| Principal signing substrate | Ledger checkpoints, artifact attestations, OCI signing, program attestation, agent actions, publication manifests, and external trust proofs. | Principal key records, key generation or import policy, public key profile, private key protection, signature envelopes, revocation, and verification errors. | Build one signing substrate instead of embedding ad hoc signing into ledger or artifact features. |
| Cross-facet indexing | Whole-Loom search, FTS from files/document/mail/calendar/contacts, graph text search, logs search, and native discovery. | Source selectors, extraction profiles, deletion propagation, update events, rebuild ownership, authorization filtering, and derived artifacts. | Build extraction and rebuild ownership once, then let FTS and query facades consume the projected records. |
| Coordination and leases | Kafka groups, MQTT QoS, NATS/JetStream consumers, Redis Streams claims, background jobs, locks, served listener reconciliation, and multi-process daemon work. | Single-node coordination contract now, cluster extension point later, leases, fencing tokens, monotonic sequence, and stale-progress rejection. | Centralize the single-node contract so cluster semantics can later attach without rewriting facades. |
| Hosted request kernel | Native REST, JSON-RPC, gRPC, product facades, admin routes, API keys, app passwords, audit, request limits, and stable errors. | Store open/save kernel, principal auth, PEP, sessions, request limits, error mapping, and audit. | Facades should declare protocol behavior and call the shared kernel rather than bypassing policy. |

### Critical Path

| Order | Shared substrate | Why it comes here |
| ---: | --- | --- |
| 1 | Capability reporting | Every other substrate needs truthful visibility across CLI, bindings, MCP, hosted routes, feature flags, and optional engines. |
| 2 | Conditional mutation and entity tags | This is small enough to finish early and directly improves files, PIM, `kv`, `cache`, hosted routes, and S3-style behavior. |
| 3 | Derived-artifact lifecycle | Search, vector, graph, dataframe, columnar, PIM indexes, metrics rollups, and caches all need one rebuild and stale-state story. |
| 4 | Background maintenance jobs | Derived artifacts, compaction, pruning, migration, and cache refresh need durable work execution once lifecycle metadata exists. |
| 5 | Change sets and retained gaps | Durable sync, push, consumers, and replay become reliable after mutation and job semantics are stable. |
| 6 | Retention and deletion policy | Physical reclamation and evidence rules should attach after source identity and maintenance jobs are clear. |
| 7 | Principal signing substrate | Signing becomes more useful once ledger, artifact, program, and publication records have stable identities to sign. |
| 8 | Cross-facet indexing | Extraction and maintained indexes should build on derived artifacts, jobs, authorization, and change propagation. |

## Appendix H - Packaging And Deployment Profile Matrix

This appendix records how feature-gated engines, platform targets, and configuration-only behavior
should be evaluated. It exists to protect the single-binary goal without letting optional runtimes
crash normal distributions or blur capability reporting.

| Profile | Target use | Must include | Must avoid | Capability behavior |
| --- | --- | --- | --- | --- |
| Core CLI desktop | Local development, inspection, native data work, and small hosted listeners. | Canonical storage, CLI, IDL-visible facets, config parsing, capability reporting, and stable unsupported-feature errors. | Hard dynamic links to optional system runtimes such as FUSE libraries, Tor runtime, IPFS node runtime, heavy native engines, or platform-only libraries. | Reports configured but unavailable features separately from unsupported features, using `feature_not_compiled` or another registry-backed subcause reason code. |
| Core server | Long-running daemon, hosted surfaces, API clients, admin routes, and compatibility facades. | Hosted kernel, serve registry, admin, auth, audit, limits, capability reporting, and selected product facades. | Silent listener enablement when a configured runtime is not compiled or a runtime dependency is absent. | Fails listener startup with stable capability errors while preserving durable configuration. |
| Data-heavy server | Analytical, search, vector, telemetry, and large data workloads. | Optional engines such as Tantivy, HNSW/PQ, Polars, Arrow, Parquet, compression, and background maintenance. | Engine artifacts as source truth; engines that cannot report degraded or unavailable states. | Reports engine availability, reduced fallback, rebuild state, and result-equivalence boundaries. |
| Privacy-network server | Onion exposure and Tor-routed outbound operations. | Tor configuration parsing in all builds; Tor runtime only in a Tor-enabled build; route and exposure policy; key protection; audit. | Treating onion reachability as Loom authorization or replacing application-level TLS semantics. | Default builds preserve Tor config and report runtime activation as `unavailable` with `feature_not_compiled`; Tor builds activate explicit routes and onion exposure. |
| IPFS-capable server | Gateway cache first, then retrieval, provider, and publisher profiles. | IPFS configuration parsing in all builds; feature-gated IPFS runtime; foreign-CID catalog; gateway allowlist; cache policy; CAR validation. | Treating IPFS CIDs as Loom digests, turning fetched blocks into normal files, or announcing content without explicit profile authorization. | Default builds preserve IPFS config and report runtime activation as `unavailable` with `feature_not_compiled`; IPFS builds activate only explicitly enabled profiles. |
| Mount-enabled desktop/server | Local file access through FUSE, NFS, SMB, WebDAV, or similar projections. | Mount config, path policy, metadata mapping, atomic mutation, lock/watch integration, and platform capability checks. | Startup-time dynamic linker failures for optional mount libraries; assuming one mount protocol defines native files. | Reports mount protocol availability per platform and runtime dependency. |
| Mobile bindings | iOS, Android, and React Native client access. | Stable C ABI, bindings, deterministic storage, small dependency footprint, capability reporting, and config-only preservation where feasible. | Heavy server runtimes, long-running background jobs without platform policy, or unsupported dynamic libraries. | Surfaces supported native operations and reports hosted/runtime features as unavailable or remote-only where appropriate. |
| WASM/browser | Portable local logic, constrained data access, and embedded clients. | Deterministic codecs, source-backed portable engines, reduced execution modes, and explicit resource limits. | Native-only engines, blocking I/O assumptions, unbounded memory, or hidden network capabilities. | Reports reduced mode, engine absence, and unsupported hosted listeners explicitly. |
| CI/conformance | Contract verification across platforms and bindings. | Canonical vectors, negative tests, transcript suites, feature-matrix tests, and deterministic reduced profiles. | Tests that pass by mocking the behavior being verified. | Emits machine-readable capability evidence and skips only by declared feature or platform reason. |

### Feature-Gating Rules

| Rule | Reason |
| --- | --- |
| Every normal distribution must parse and preserve configuration for optional profiles. | Operators should be able to inspect, copy, import, export, and migrate stores even when a runtime feature is not compiled in. |
| Runtime activation must require both compiled support and explicit operator configuration. | A binary should not unexpectedly start Tor, IPFS, FUSE, or heavy engines because configuration is merely present. |
| Optional engines must not define source identity. | Engine artifacts can be rebuilt, removed, or unavailable without changing canonical Loom data. |
| Capability reports must separate the 0010 states `supported`, `degraded`, `disabled`, `unavailable`, `denied`, `unsupported`, and `target`, and must use registry-backed reason codes such as `feature_not_compiled`, `runtime_dependency_absent`, `configured_disabled`, and `policy_denied` for subcauses. | These states require different operator actions and must not collapse into a generic failure or grow surface-specific state aliases. |
| Product facades must not hide optional runtime absence behind protocol success. | A client should receive stable unsupported or unavailable behavior instead of partial silent behavior. |
| Platform profiles must be explicit in conformance output. | Desktop, server, mobile, WASM, and CI builds have different valid capability sets. |
