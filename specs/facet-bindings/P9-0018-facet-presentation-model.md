# P9-0018 - Facet Base Layers And Presentation Surfaces

**Series:** P9 binding plan companion
**Version:** 0.1.0-draft
**Status:** Draft design direction, owner-resolved
**Last updated:** 2026-07-02
**Reads first:** `P9-0001-binding-architecture.md`, `P9-0002-projection-conventions.md`,
`REFERENCE-IMPLEMENTATIONS.md`, `REFERENCE-CLIENTS-AND-FORMATS.md`

This document captures the resolved design direction from the Queue 2 facet-binding design review. It
exists so the design does not remain only in chat while the facet-binding specs are reconciled.

## 1. Resolved Direction

Each data facet has a Loom-native base layer. Tier-2 adapters and compatibility protocols are
presentations over that base layer. A presentation must not become the storage model, identity model,
authorization model, or source of truth.

The base layer owns:

- canonical data model;
- content identity and deterministic encoding profile;
- workspace and facet identity;
- sync, merge, and conflict semantics;
- ACL, PEP, audit, and stable errors;
- source data for derived artifacts.

A presentation owns:

- client-visible protocol shape;
- compatibility grammar or query profile;
- request and response mapping;
- protocol-specific capability reporting;
- unsupported behavior for gaps;
- conformance traffic for the target client ecosystem.

## 2. REST Root Correction

The hosted model serves one `.loom` store per daemon/listener. Therefore canonical hosted REST roots use
the workspace path directly:

```text
/v1/workspaces/{workspace_id}/...
```

The older multi-store gateway shape carried an outer store id segment. That shape is stale for local
hosted listeners and must not be copied into per-store REST roots:

```text
/v1/looms/{loom_id}/workspaces/{workspace_id}/...
```

If a future cloud gateway serves many Loom stores behind one account or workspace route, that gateway can
add an outer route segment. The per-store hosted API should not carry a redundant loom id.

## 3. Recommended Design Rule

Every facet-binding spec should separate these concepts explicitly:

| Concept | Meaning |
| --- | --- |
| Base facet | Canonical Loom data model, identity, merge/sync behavior, ACL, audit |
| Native projection | Loom REST, JSON-RPC, gRPC, and MCP methods |
| Presentation | Compatibility surface shaped like an external ecosystem |
| Transport | Encoding, framing, session, or RPC carrier that preserves the same surface semantics |
| Profile | Named compatibility dialect inside a surface when selectors and options still belong to that surface |
| Physical or interchange format | Files or byte formats such as Parquet, Arrow, CAR, JSONL, GraphML |
| Engine | Internal execution, indexing, or storage implementation; never public serve syntax by itself |
| Derived artifact | Rebuildable index, cache, or accelerator; never the source of truth |

This rule prevents a Tier-2 protocol from distorting the core contract. It also prevents local lifecycle
mechanics such as derived-artifact rebuild states from becoming premature public commands.

Served listener grammar uses this stricter rule:

- a surface owns product or domain semantics, selectors, lifecycle, capability reporting, and
  protocol-specific options;
- a transport owns only encoding, framing, session, or RPC mechanics for that surface;
- a profile owns a compatibility dialect only when the selector model and public options still belong
  to the same surface;
- an engine is an implementation detail and must not become a served surface only because the runtime
  uses it.

Examples:

| Correct classification | Example |
| --- | --- |
| First-class compatibility surface | `redis`, `memcached`, `s3`, `oci`, `kafka`, `influx`, `prometheus` |
| Transport | `rest`, `json_rpc`, `grpc`, `resp`, `text`, `ndjson`, `arrow_flight` |
| Profile | `vector --profile qdrant` if the native surface still owns selectors and lifecycle |
| Engine | Tantivy, Polars, HNSW, GlueSQL |

## 4. Facets That Should Use The Pattern

The pattern applies broadly. KV is not an exception: Redis, Memcached, and etcd-class surfaces are
presentations over the base KV family. Redis and Memcached are dedicated served compatibility surfaces,
not transports under `kv`.

| Facet | Base Loom layer | Candidate presentations | Why it fits |
| --- | --- | --- | --- |
| `columnar` | Versioned analytical datasets over a Loom manifest and deterministic Parquet-backed segments | Native columnar API, Arrow Flight result transport, Parquet import/export, DuckDB-like analytical SQL, Snowflake-like warehouse SQL, Spark-like batch surface, BigQuery-like job/query surface | Analytical data has multiple dominant client ecosystems. The base layer should own schema, segments, identity, and versioning while presentations expose familiar query and transport shapes. Arrow Flight is a transport when it only carries columnar batches; warehouse-style ecosystems are presentations that need separate compatibility contracts. |
| `dataframe` | Versioned transformation plans and schema workflows over CSV, JSON/NDJSON, Arrow, Parquet, SQL, files/CAS, and columnar inputs | Native dataframe API, hosted dataframe surface, Polars-backed native execution, Arrow Flight result transfer, materialization into columnar | Data preparation and transformation are broader than durable columnar storage. The dataframe facet owns logical frames and transformations, while columnar owns committed analytical datasets. |
| `sql` | Versioned relational or tabular workspace | Native SQL, first-class PostgreSQL-compatible surface, first-class MySQL-compatible surface, SQLite-like import/export, JDBC/ODBC-adjacent bridge later | SQL clients expect wire compatibility, but Loom must retain versioned storage, branch/diff/merge, ACL, and stable errors. PostgreSQL and MySQL compatibility have product-specific session, dialect, metadata, and option semantics; they should not be modeled as generic SQL transports once promoted. |
| `search` | Loom search collection over source documents and derived indexes | Native FTS API, OpenSearch-compatible route set with aggregations, Elasticsearch-compatible profile where possible, Lucene-query-string profile later if justified | OpenSearch-compatible clients are valuable, but canonical documents and derived index lifecycle remain Loom-owned. The served native full-text surface is `fts`; top-level `search` is store-wide discovery, not a served alias. |
| `vector` | Loom vector set with exact deterministic search and optional derived accelerators | Native vector API, generic hosted profile, Qdrant-shaped REST/gRPC profile, Pinecone-shaped REST profile, PostgreSQL/pgvector projection through the PostgreSQL surface, Milvus-like or Weaviate-like profile later if justified | Vector APIs are fragmented. Loom should expose useful ecosystem profiles without making one vendor API the storage model. Compatibility serving uses explicit profiles when selectors and options remain vector-owned. PostgreSQL/pgvector belongs with the PostgreSQL compatibility surface. |
| `document` | Loom document collection keyed by id, with native indexes/query | Native document API, native document indexes/query, deferred first-class MongoDB-compatible surface, deferred Couchbase-compatible surface, JSON/JSONL/BSON import/export | Document stores split between id-keyed document access, query/index behavior, and product-specific wire ecosystems. Native document indexes/query are source-backed; MongoDB and Couchbase remain valuable P3 compatibility candidates that need product-scope design before build. CouchDB serving is cut from the current roadmap. |
| `kv` | Loom typed ordered key-value map | Native KV API, dedicated Redis-compatible served surface, dedicated Memcached-compatible served surface, first-class etcd-compatible surface if promoted, Couchbase KV behavior only through the Couchbase design if document semantics are not needed | KV has multiple mature client ecosystems. Redis is a storage-capable data-structure server, Memcached is a cache service, etcd is ordered-control-plane KV, and Couchbase overlaps KV and document. |
| `files` | Loom file tree and working tree | Native tree API, FUSE, archive import/export, and S3-backed internal object-key materialization where the first-class S3 surface stores path-like objects | Files are naturally consumed as paths, mounts, object APIs, and archives. The base tree must not be tied to any one consumer, and S3 compatibility is a first-class served surface rather than a transport under `files`. Archive import/export uses `tar.zstd` as the canonical format and `tar`, `tar.gz`, and `zip` as compatibility formats. |
| `queue` | Loom append log or queue collection | Native log API, first-class Kafka-compatible surface, first-class MQTT-compatible candidate, first-class NATS/JetStream candidate, AMQP candidate if justified | Queue ecosystems differ in protocol and delivery semantics. Kafka, MQTT, NATS/JetStream, and AMQP own product-specific selectors, lifecycle, and options; they should not be modeled as generic queue transports once promoted. |
| `time-series` | Loom timestamped series | Native series API, first-class Influx-compatible surface, first-class Prometheus-compatible surface, first-class Grafana datasource surface, first-class OTLP metrics candidate | Time-series systems differ in tags, query language, ingestion protocol, and client lifecycle. The base point model must support selected presentations through canonical labels/tags, typed fields, timestamp precision, and retention/rollup policy. |
| `graph` | Loom property graph | Native traversal API, structured graph storage, bounded GQL-aligned openCypher profile, GraphML/GEXF import/export | Graph owns Loom-native storage, traversal, query IR, resource bounds, and result values. Neo4j compatibility is a first-class `neo4j` surface because official drivers expect product semantics beyond generic graph framing. Gremlin is cut from active scope. |
| `neo4j` | Neo4j-compatible graph product surface | Bolt-compatible TCP, Neo4j-shaped records/errors, official-driver transcript conformance, catalog/procedure compatibility shims, HTTP/Query API candidates | Neo4j has official drivers and APIs that create product-level expectations. The `neo4j` surface maps to native graph IR and storage but owns sessions, handshakes, driver behavior, and unsupported-feature reporting. |
| `cas` | Loom workspace-scoped reachable blob set | Native CAS API, first-class OCI distribution surface internals, CAR import/export, and S3 digest/object storage internals where appropriate | Content-addressed data can mimic registry and content-exchange ecosystems while preserving Loom digest semantics. S3 and OCI are first-class served compatibility surfaces, not `cas` transports. IPFS and Kubo are excluded from the current roadmap. |
| `ledger` | Loom append-only verifiable log | Native ledger API, segment-native structured storage, signed checkpoints, derived proof artifacts, and transparency-log/checkpoint profile after proof semantics land | Ledger presentations vary by proof model. The active Ledger direction is Loom-native segment-native storage, canonical head metadata, canonical segment indexes, principal-signed checkpointing, retained/pruned range semantics, and derived proof artifacts. Transparency-log behavior may remain a ledger profile after inclusion proofs, consistency proofs, witness policy, disclosure policy, and conformance are specified. Product-clone ledger database compatibility is not an active target. |
| `vcs` | Loom commit/ref/history model | Native VCS REST/gRPC/MCP, Loom bundle import/export, custom Loom GUI protocol | Git remote compatibility remains excluded. The pattern still applies, but the Tier-2 foreign adapter set is intentionally limited. |

## 4.1 Served Registry Cleanup Direction

The served registry cleanup applies the surface, transport, profile, and engine rule across rows that
still admit product-shaped names as transports.

| Area | Before | After |
| --- | --- | --- |
| SQL PostgreSQL | `sql --transport pg_wire` | first-class `postgres --transport tcp` surface; native `sql` keeps Loom SQL REST, JSON-RPC, and gRPC |
| SQL MySQL | `sql --transport mysql_wire` | first-class `mysql --transport tcp` surface; native `sql` keeps Loom SQL REST, JSON-RPC, and gRPC |
| KV etcd | `kv --transport etcd_grpc` | first-class `etcd` surface if promoted |
| Couchbase | `kv --transport couchbase_kv` or `document --transport couchbase_document` | P3 integrated first-class `couchbase` surface only after KV, document, query, and analytics boundaries are designed; native document indexes/query are already source-backed foundation |
| MongoDB | `document --transport mongodb_wire` | P3 first-class `mongodb` surface now that native document indexes/query are source-backed, with product-wire scope and compatibility matrix required before build |
| CouchDB | `document --transport couchdb_rest` | cut from the current roadmap; reconsider only if revision trees, conflicts, `_changes`, and replication become strategic |
| PostgreSQL/pgvector | Native vector has no pgvector listener transport | pgvector-style access under the `postgres` compatibility surface; native vector profiles remain under `vector` |
| Graph Bolt | legacy `graph` transport candidate | first-class `neo4j/tcp` target surface |
| Graph Gremlin | legacy `graph` profile or transport candidate | cut from current roadmap |
| Ledger product clone | product-specific ledger database transport | cut from active design; Ledger focuses on Loom-native structured storage, signing, checkpoints, replay, retention, and derived proof artifacts |
| Transparency log | `ledger --transport transparency_log` | ledger profile after proof, witness, disclosure, and conformance semantics are promoted |
| PIM standards access | `calendar --transport caldav`, `contacts --transport carddav`, `mail --transport imap`, `mail --transport jmap` | keep under owning PIM surfaces because these standards expose the same domain semantics |
| SMTP | `mail --transport smtp` | keep only for bounded mailbox-adjacent compatibility; real submission, relay, delivery, and outbound policy require a PIM-owned design |

### 4.2 Queue Eventing Compatibility Direction

Kafka, MQTT, and NATS are first-class compatibility surface candidates over the queue/eventing family,
not transports under the native `queue` surface.

The distinction is visible in command shape:

```text
loom serve configure app.loom queue work events --transport rest --bind 127.0.0.1:8080
loom serve configure app.loom mqtt work --bind 127.0.0.1:1883
loom serve configure app.loom nats work --bind 127.0.0.1:4222
```

The native `queue` surface owns append, get, range, len, and consumer-offset operations over Loom
collections. MQTT owns subscriptions, topic filters, session state, QoS, retained messages, will
messages, and broker lifecycle. NATS owns subjects, queue groups, request/reply, core pub/sub, and
JetStream stream, consumer, acknowledgement, retention, and replay semantics if JetStream is promoted.
Those options are not universal queue options, so placing them behind `queue --transport ...` would
make the base queue surface carry product-specific behavior.

Task 360e records MQTT as a first-class target candidate. Task 360f records NATS/JetStream as a
first-class target candidate with an explicit follow-up design split between NATS core and JetStream.
Neither task claims implementation.

## 5. Redis, Memcached, And Couchbase Placement

The reference docs should keep these placements explicit during facet-binding reconciliation.

| Reference system | Primary Loom facet | Secondary facets | Rationale | Rows to update |
| --- | --- | --- | --- | --- |
| Redis | `redis` served compatibility surface over the KV family | `queue` for streams, `delivery` or runtime fanout for pub/sub, `compute` for functions if promoted | Redis clients expect one Redis-like port with a Redis command space. The dedicated `redis` surface owns that compatibility contract while using KV/cache/queue/runtime internals as needed. | `REFERENCE-IMPLEMENTATIONS.md` master `kv` and compatibility rows; `REFERENCE-CLIENTS-AND-FORMATS.md` KV and Redis client rows; `P9-0007-kv-binding.md`; `0019b-redis-memcached-presentations.md`. |
| Memcached | `memcached` served compatibility surface over cache semantics | none by default | Memcached is a network cache service. The public token and protocol name should be `memcached`; volatile cache lifecycle is the default, while durable or backed variants require explicit operator policy. | `REFERENCE-IMPLEMENTATIONS.md` master `kv` and compatibility rows; `REFERENCE-CLIENTS-AND-FORMATS.md` KV and Memcached client rows; `P9-0007-kv-binding.md`; `0019b-redis-memcached-presentations.md`. |
| Couchbase | `document` | `kv`, `sql`, `columnar` depending on presentation | Couchbase combines key-value access, JSON document storage, query services, and analytics. The closest primary Loom facet is `document` because the stored unit is a document. KV-style Couchbase access belongs under KV presentations; SQL++/N1QL-like querying requires document and SQL planning; analytics overlaps columnar only through explicitly materialized analytical datasets. Native document indexes/query are source-backed. The remaining P3 work is integrated service, bucket/scope/collection, subdocument, query, analytics, authentication, transaction, durability, error, and client-conformance design. | `REFERENCE-IMPLEMENTATIONS.md` document detail row, plus a cross-reference in the KV detail row; `REFERENCE-CLIENTS-AND-FORMATS.md` document and KV client rows; `P9-0008-document-binding.md` Tier-2 section; `P9-0007-kv-binding.md` notes. |

## 5.1 S3, OCI, And CAR Placement

The object/blob/archive family resolves into first-class compatibility surfaces and interchange
formats instead of treating every ecosystem shape as a transport under `files` or `cas`.

| Reference system or format | Primary Loom surface | Backing facets | Rationale | Rows to update |
| --- | --- | --- | --- | --- |
| S3 | `s3` served compatibility surface | `files`, `cas`, and `.loom/facets/s3/buckets/{bucketname}` state | S3 clients expect a bucket/object service endpoint. A service-scoped S3 listener selects a Loom workspace as the authority scope and resolves buckets from virtual-host-style Host headers, CNAME-style Host headers, or path-style compatibility fallback. A bucket-scoped listener binds one bucket to one endpoint, so the request path is the object key root. Daemon-opened `s3/rest` is source-backed for bucket create/list/delete, object put/get/head/delete, metadata headers, byte ranges, conditional writes, opaque S3-safe version IDs, S3-compatible ETags, basic multipart upload, hosted auth/PEP, SigV4 app credential verification, configured unauthenticated public-read ACLs, guarded AWS CLI create/put/get transcript coverage, and conformance rows. Direct TLS for `s3/rest` uses the shared hosted TLS path when configured. Bucket lifecycle, versioning, policies, public-access configuration, metadata, multipart state, and conditional writes belong to the S3 surface, while file/CAS primitives remain internals. S3 version IDs are opaque S3-safe tokens over deterministic internal version identity, not raw Loom commit IDs. S3 ETags are representation/version ETags, not Loom canonical digests. | `0008-wire-protocols.md`; `P9-0003-files-binding.md`; S3 child spec when created. |
| OCI Distribution | `oci` served compatibility surface | `cas`, stable repository-id metadata, manifest/tag metadata, referrer indexes, and durable upload-session state | OCI is repository and digest native. Daemon-opened `oci/rest` is source-backed for public slash-separated repository names, stable internal repository ids plus display metadata, API check, manifest GET/HEAD/PUT/DELETE, blob GET/HEAD/DELETE, monolithic upload, durable chunked upload, upload status/cancel, cross-repository mount, tags list, bounded catalog, referrers, strict SHA-256 digest verification, hosted auth/PEP, OCI and Docker v2 schema media-type admission, and schema v1 plus unknown dangerous media-type rejection. Direct TLS for `oci/rest` uses the shared hosted TLS path when configured. Blob delete removes repository reachability metadata; physical CAS byte deletion requires separate reachability-proven GC. | `0008-wire-protocols.md`; `P9-0005-cas-binding.md`; OCI child spec when created. |
| CAR | Import/export command and interchange format | `cas` and workspace object graph | CAR is an exchange format, not a daemon listener. `loom interchange export-car` and `loom interchange import-car` are source-backed for deterministic workspace graph export/import through a CARv1-shaped length-delimited stream. The single root block is a Loom CAR manifest; object blocks use CIDv1 raw-codec multihash CIDs derived from Loom BLAKE3 or SHA-256 digests. Import validates every block CID before reconstructing the workspace bundle. IPFS and Kubo compatibility remain excluded from active scope. | `P9-0005-cas-binding.md`; CAR child spec when created. |
| Archive | Import/export command and interchange format | `files`, `cas`, and workspace object graph | Archive import/export is a compatibility and preservation path over a versioned file tree. Source-backed Rust and CLI import/export use `tar.zstd` as the canonical performant format; `tar`, `tar.gz`, and `zip` are compatibility formats. Single-file gzip is import-only compatibility input. Path-safety, deterministic ordering, metadata handling, and file-tree mapping belong to the archive profile rather than the S3 or OCI surfaces. | `0012-interchange.md`; `P9-0003-files-binding.md`; archive child spec when created. |

IPFS gateway compatibility and Kubo RPC compatibility are excluded from the current roadmap. Their value is
lower than OCI, S3, and CAR for the current roadmap.

## 6. Search Presentation Direction

The OpenSearch-compatible presentation is resolved as a meaningful search compatibility profile backed
by a route-by-route compatibility matrix. OpenSearch is the primary target, while Elasticsearch
compatibility is tracked as a selectable comparison target.

Target OpenSearch-compatible scope:

- service/info and health endpoints;
- index create/delete and mapping introspection;
- document CRUD;
- bulk indexing;
- `_bulk` NDJSON;
- `_msearch` NDJSON;
- aliases and `is_write_index`;
- multi-index and wildcard expansion inside the served workspace;
- OpenSearch-style refresh;
- search over the Query DSL target matrix;
- full aggregation target matrix;
- full analyzer target matrix;
- read-only auth/capability security shims over Loom hosted auth and ACL.

The target Query DSL matrix includes at least:

- `bool`;
- `match`;
- `multi_match`;
- `match_phrase`;
- `term`;
- `terms`;
- `range`;
- `exists`;
- `prefix`;
- pagination;
- sort;
- `_source` selection.

The target aggregation matrix includes at least:

- `terms`;
- `histogram`;
- `date_histogram`;
- `min`;
- `max`;
- `avg`;
- `sum`;
- `value_count`;
- `stats` if the underlying field type mapping is clear;
- additional OpenSearch aggregations as explicit supported, unsupported, approximate, or native-only
  rows.

Out of v1 scope unless explicitly promoted:

- scripting;
- custom ingest pipelines;
- dashboard saved-object APIs;
- percolator;
- nested joins;
- cluster-management mutation APIs;
- OpenSearch security admin mutation APIs.

The native served syntax uses the Loom `fts` surface and generic transport labels:

```text
loom serve configure <store> fts <workspace> <collection> --transport rest --bind 127.0.0.1:9200
loom serve configure <store> fts <workspace> <collection> --transport ndjson --bind 127.0.0.1:9200
```

OpenSearch compatibility is not exposed as an `opensearch` served surface. The public served surface
remains `fts`, with OpenSearch-shaped behavior selected by the route set and generic `rest` or `ndjson`
transport. This preserves `search` for whole-Loom discovery and avoids adding a product-named
surface where the base full-text collection semantics still own selectors and lifecycle.

Each OpenSearch index maps to one Loom search collection. Since the listener is scoped to a workspace,
multi-index and wildcard expansion do not cross workspace boundaries.

## 7. Columnar Presentation Direction

The base `columnar` facet should become Arrow/Parquet-capable. It should not collapse into any one
external product shape.

| Presentation | Target behavior |
| --- | --- |
| Native columnar | Loom-native dataset create, append/import, scan, aggregate, compact, and inspect operations. |
| Arrow/Parquet | Arrow as runtime/wire batch model; Parquet as first-class storage/import/export where deterministic identity can be pinned. |
| DuckDB-like | Embedded/local analytical SQL feel over Arrow/Parquet datasets without using the DuckDB name as a public Loom surface. |
| Snowflake-like | Hosted warehouse-style query and metadata behavior where datasets, schemas, auth, and result sets feel service-oriented. |
| Spark-like | Batch-oriented scan/write contracts over partitioned datasets. Dataframe-specific transformations are owned by 0045. |
| BigQuery-like | Job/query/result API shape for serverless analytical workflows, if the hosted runtime can support it honestly. |

The migration question is not product migration. Because Loom is unreleased, current row-encoded
development stores and fixtures may be broken and regenerated when the promoted manifest and segment
profile lands.

## 7.1 DataFrame Presentation Direction

The base `dataframe` facet is a first-class transformation surface, not a thin presentation of
`columnar`.

| Presentation | Target behavior |
| --- | --- |
| Native dataframe | Create logical frames, bind sources, infer schema, transform, preview, collect, export, and materialize. |
| Polars-backed native | Use Polars as the default native execution layer while keeping Loom plans and outputs as identity. |
| Hosted dataframe | `loom serve <surface>` surface for frame management and result transfer where client value is clear. |
| Arrow Flight | High-volume result transfer after 0023 pins Arrow batch semantics. |
| Columnar materialization | Commit cleaned or transformed outputs as versioned `columnar` datasets. |

Current source backs the base logical-plan and source-binding substrate in `loom_core::dataframe`,
deterministic CSV, JSON, and NDJSON loading, files/CAS/columnar inputs where the source contract is
available, a portable deterministic executor subset, CLI commands, IDL, C ABI, generated C headers,
MCP tools, hosted `dataframe/rest` management routes, and shared local conformance for
collect/materialization/versioning behavior.

Remaining dataframe presentation work includes Arrow IPC, Parquet, and SQL-result adapters, the native
Polars executor, language-specific wrapper ergonomics, Arrow Flight or similar result transfer,
JSON-RPC or generic gRPC only if client value is proven, and capability reporting that distinguishes
portable subset, native Polars, hosted REST, and unsupported transports.

DataFusion is eliminated from the v1 plan. It can be reconsidered only by a later design decision with a
specific role, platform profile, and conformance strategy.

## 7.2 Analytical Presentation Grouping

SQL wire, pgvector-style vector access, DuckDB-like analytical SQL, columnar query access, dataframe
SQL-result inputs, and hosted result transfer belong to one design family. They should be implemented
as coordinated presentation layers over separate Loom base facets, not as isolated protocol islands.

| Concern | Owning base facet | Presentation role | Boundary |
| --- | --- | --- | --- |
| OLTP-style relational state | `sql` | Native SQL facade and future PostgreSQL-wire or MySQL-wire adapters | The wire adapter accepts client protocol traffic but does not change Loom SQL identity, ACL, stable errors, or versioning. |
| Analytical datasets | `columnar` | Native columnar, Arrow IPC, Parquet, Arrow Flight, Flight SQL or ADBC-adjacent access, and DuckDB-like analytical SQL | Columnar owns committed dataset identity and segment policy. External analytical presentations read or write through the columnar contract. |
| Transformation workflows | `dataframe` | Native dataframe, Polars-backed native execution, SQL-result inputs, Arrow Flight result transfer, and materialization | Dataframe owns logical plans, source bindings, previews, exports, lineage, and materialization into columnar, files, or CAS. |
| Vector similarity through SQL clients | `vector` plus `sql` | pgvector-like operators over SQL once SQL wire exists | pgvector is a SQL presentation. It is not a `vector/rest` or `vector/grpc` profile. |
| Hosted analytical results | `sql`, `columnar`, and `dataframe` | Result handles, Arrow batches, Flight SQL or ADBC-adjacent access where useful | Hosted result handles must be principal-bound, authorization-checked, expiring resources owned by the hosted kernel. |

The first implementation path should be:

1. Keep native Loom SQL, columnar, dataframe, and vector contracts separate.
2. Promote PostgreSQL-wire first if a SQL foreign wire adapter is built, because the existing SQL wire
   adapter spec already ranks it ahead of MySQL-wire.
3. Treat pgvector-style operators as a SQL presentation over Loom vector collections, gated on SQL
   wire and explicit vector mapping.
4. Build DuckDB-like local analytical SQL as a Loom presentation over columnar datasets and dataframe
   materialization, without embedding DuckDB or naming the public Loom surface after DuckDB.
5. Use Arrow batches as the common high-volume result shape once the 0023 Arrow profile is pinned.
6. Keep Snowflake-like, Spark-like, and BigQuery-like presentations behind the same base contracts so
   they reuse schema, auth, result transport, and conformance work instead of creating independent
   data models.

This grouping is the design input for analytical presentation work. New SQL, columnar, dataframe, and
vector presentations should not add a foreign listener in isolation. They should select the
client-facing profile and then implement the shared analytical result, capability, and conformance
pieces needed by that profile.

## 8. Facet-Binding Exit Criteria

Facet-binding reconciliation is complete only when the design directions above become actionable spec
and queue work:

1. Shared P9 architecture and convention docs use `/v1/workspaces/{workspace_id}/...` for hosted
   per-store REST roots.
2. P9 docs distinguish base facets, native projections, presentations, physical/interchange formats, and
   derived artifacts.
3. Reference implementation and client rows explicitly place Redis, Memcached, and Couchbase.
4. Per-facet P9 docs reflect the base-plus-presentation model, not a single Tier-2 surface as the model.
5. Columnar records Arrow/Parquet as required foundation and the DuckDB-like, Snowflake-like, Spark-like,
   and BigQuery-like presentation directions.
6. Search records OpenSearch core plus aggregations as the target presentation.
7. Queue 2 breaks the design directions into dependency-ordered implementation tasks and coalesces
   cross-binding updates so related bindings are updated in batches.
8. Remaining stale claims, especially designed-only status and no-port status where source now proves
   otherwise, are removed or explicitly marked as historical.
