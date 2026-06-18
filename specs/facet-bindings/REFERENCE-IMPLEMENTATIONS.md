# Loom Data Facets  -  Reference Implementations & Interface Landscape

**Spec series:** Loom Core Specification (LCS) - companion (informative)
**Version:** 0.1.0-draft
**Status:** Draft companion, reconciled with current hosted surface map.
**Last updated:** 2026-07-13

This is an informative companion to the spec series. It maps each of Loom's **data-storage facets**
to the established software the wider industry already uses for that shape of data, so that when we
design a facet's interface, bindings, and wire protocol we are deliberately tracking (or deliberately
diverging from) a known reference rather than inventing in a vacuum.

It is built in two phases:

- **Phase 1 (this document, below).** For every data facet: the popular open-source and commercial
  **reference implementations**; the **interface standard / grammar** that governs the category (with a
  canonical link); the **client SDKs / connection tooling** a caller needs (and whether that capability
  is preinstalled, needs a package, or needs an OS-level driver); and the **connection method** (port,
  mount path, protocol).
- **Phase 2 - [`IMPLEMENTATION-FIDELITY.md`](./IMPLEMENTATION-FIDELITY.md).** A deep per-facet comparison
  of *our* implementation against the chosen reference, for example whether SQL is bespoke or a
  faithful subset of a real SQL grammar, with a risk flag per facet and the exposure gap that follows
  from hosted surfaces that are source-backed only for bounded subsets.

Scope is the **data-storage facets** only: `cas`, `files`, `vcs`, `kv`, `document`, `sql`, `columnar`,
`time-series`, `queue`, `ledger`, `graph`, `vector`, `search`. The control-plane capabilities
(`sync`, `exec`, `identity`, `acl`, `trigger`, `watch`, `e2e-sync`, embedding/LLM providers) are out of
scope here.

> **Conformance note.** This document is **informative** and carries no conformance weight. The normative
> definition of each facet lives in its numbered spec (cross-referenced per section).

---

## Cross-cutting reality check: how Loom is reachable today

This frames every "connection method" cell below, and is the central caveat for Phase 2.

Every reference implementation in this document is reached over a wire, a TCP port speaking a protocol
(MySQL on 3306, OpenSearch on 9200, Neo4j Bolt on 7687), a gRPC endpoint, an HTTP API, or a kernel mount
point. Loom now has source-backed hosted listener management and bounded served surfaces for many
facets, but these are not blanket compatibility claims.

- `loom serve` owns durable hosted listener management. `loom daemon` owns local coordination.
- Native hosted REST/JSON-RPC/gRPC subsets exist for core data facets where the owning specs say so.
- Product-shaped compatibility surfaces such as `postgres`, `mysql`, `redis`, `memcached`, `kafka`,
  `neo4j`, `s3`, `oci`, `influx`, `prometheus`, `grafana`, and `fts` have bounded source-backed rows.
- Full compatibility, generated schemas, reference-client certification, and broad protocol
  conformance remain target unless the owning spec records executable evidence.

---

## Master map: facet -> reference software

Quick orientation. "Loom today" is the local build state (`built`, `built local`, `partial`, or
`spec-only`). The detail tables and links follow below. Connection ports listed are the **reference
software's** defaults, not Loom's.

| #  | Facet         | Spec | Loom facade - today        | Reference impls (OSS - commercial)                          | Interface standard                          | Reference connection (default)             |
| -- | ------------- | ---- | -------------------------- | ----------------------------------------------------------- | ------------------------------------------- | ------------------------------------------ |
| 1  | `cas`         | 0024 | `cas` built; OCI/CAR bounded subsets | Git objects, Perkeep, restic, OCI, S3, R2 | OCI image-spec, CAR, content-addressing patterns | content fetched by hash; OCI HTTP registry |
| 2  | `files`       | 0003 section 4 / 0005 | `fs` built; hosted files subset | ext4, Btrfs, ZFS, libfuse, EFS, FSx, Filestore | POSIX IEEE 1003.1; FUSE | mount on a path; no port |
| 3  | `vcs`         | 0003 section 5 | `vcs` built; hosted VCS subset | Git, Mercurial, SVN, Fossil, Pijul, GitHub, GitLab | Git pack/wire protocol (de facto) | SSH 22 / HTTPS 443 / `git://` 9418 |
| 4  | `kv`          | 0019 | `kv` - **built**           | Redis/Valkey, Memcached, etcd, RocksDB, LevelDB - DynamoDB, Workers KV | RESP; Memcache protocol; etcd gRPC (per-engine) | Redis TCP 6379; Memcached 11211; etcd 2379 |
| 5  | `document`    | 0020 | `document` - **built**     | MongoDB, CouchDB, RavenDB, ArangoDB, Couchbase - Atlas, DocumentDB | MongoDB wire protocol + BSON; Couchbase-style document/query services | MongoDB TCP 27017; Couchbase 11210/8091 |
| 6  | `sql`         | 0011 | `sql` partial; `postgres/tcp` and `mysql/tcp` bounded subsets | PostgreSQL, MySQL/MariaDB, SQLite, GlueSQL, Oracle, Aurora | ISO/IEC 9075 (SQL:2023) | PostgreSQL 5432; MySQL 3306 |
| 7  | `columnar`    | 0023 | `columnar` built; Arrow/Parquet import/export bounded subset | DuckDB, ClickHouse, Parquet/Arrow, Polars, Snowflake, BigQuery | Apache Arrow + Parquet format spec | DuckDB in-process; ClickHouse 9000/8123 |
| 8  | `time-series` | 0022 | `time-series` - **built**  | InfluxDB, TimescaleDB, Prometheus, QuestDB - Timestream     | InfluxDB Line Protocol; PromQL (de facto)   | InfluxDB 8086; Prometheus 9090             |
| 9  | `queue`       | 0021 | `queue` - **built**        | Kafka, RabbitMQ, NATS, Redpanda, Pulsar - Confluent, MSK    | Kafka protocol; AMQP 0-9-1 / 1.0 (ISO 19464) | Kafka 9092; AMQP 5672; NATS 4222          |
| 10 | `ledger`      | 0018 | `ledger` built; native hosted gRPC proof subset | Hyperledger Fabric, Trillian, Rekor, Azure SQL ledger | RFC 6962 / RFC 9162 transparency-log references | Fabric peer 7051; Trillian log server 8090 |
| 11 | `graph`       | 0016 | `graph` built; native gRPC and bounded `neo4j/tcp` subset | Neo4j, JanusGraph, ArangoDB, Dgraph, Memgraph, Aura, Neptune | ISO/IEC 39075 GQL; openCypher; Neo4j Bolt | Neo4j Bolt 7687 |
| 12 | `vector`      | 0017 | `vector` - **built**       | Qdrant, Milvus, Weaviate, pgvector, FAISS - Pinecone, Zilliz | none (de facto per-product; HNSW algo)    | Qdrant 6333/6334; Milvus 19530             |
| 13 | `search`      | 0033 | `search` built; `fts` hosted/OpenSearch-compatible bounded subset | OpenSearch, Elasticsearch, Solr, Tantivy, Meilisearch, Elastic Cloud, Algolia | none (Query DSL / Lucene; BM25) | OpenSearch/ES HTTP 9200; Solr 8983 |

---

## Per-facet detail

Each facet below carries the four requested dimensions plus a one-line "Loom today" pointer (the full
comparison is Phase 2). Ports are the **reference software's** defaults.

### 1. `cas`  -  content-addressed blob store (spec 0024)

| Dimension | Detail |
| --- | --- |
| Reference impls - OSS | Git object store, [Perkeep](https://perkeep.org/), [restic](https://restic.net/), [casync/desync](https://github.com/systemd/casync), [ostree](https://ostreedev.github.io/ostree/), OCI registries |
| Reference impls - commercial/managed | AWS S3, Cloudflare R2, managed OCI registries |
| Interface standard / grammar | No formal CAS standard. De facto references include Git object storage, OCI image-spec, CAR, and S3-compatible object APIs where content identity or checksums matter. |
| Client SDKs / connection tooling (+ install) | OCI clients and registry tools, S3 clients, Git plumbing via `libgit2` / `git hash-object`. IPFS/Kubo compatibility is cut from the current roadmap. |
| Connection method (port/path/protocol) | No universal CAS port. OCI uses HTTP registry endpoints; S3-compatible clients use HTTP; Git object store is local content-addressed files under `.git/objects`. |
| **Loom today** | `cas` facade built in `loom-core::cas`; bounded OCI Distribution, S3, CAR import/export, and hosted CAS protocol subsets are source-backed where owning specs state them. |

### 2. `files` - filesystem (spec 0003 section 4, container 0005)

| Dimension | Detail |
| --- | --- |
| Reference impls - OSS | [ext4](https://docs.kernel.org/filesystems/ext4/), [Btrfs](https://btrfs.readthedocs.io/), [XFS](https://xfs.wiki.kernel.org/), [OpenZFS](https://openzfs.org/), [libfuse](https://github.com/libfuse/libfuse) |
| Reference impls - commercial/managed | AWS EFS, Amazon FSx, Google Filestore, Azure Files |
| Interface standard / grammar | POSIX IEEE Std 1003.1 and FUSE-style mount behavior. |
| Client SDKs / connection tooling (+ install) | Mounting uses the OS VFS or FUSE-style user-space filesystem support. |
| Connection method (port/path/protocol) | Mount onto a path. Network filesystems layer a protocol on top, such as NFS TCP 2049 or SMB 445. |
| **Loom today** | `fs` facade built; hosted files REST/JSON-RPC/gRPC subsets and archive import/export are source-backed where owning specs state them. FUSE remains a mount path, not a served facet. |

### 3. `vcs`  -  version control (spec 0003 section5)

| Dimension | Detail |
| --- | --- |
| Reference impls  -  OSS | [Git](https://git-scm.com/), [Mercurial](https://www.mercurial-scm.org/), [Subversion](https://subversion.apache.org/), [Fossil](https://fossil-scm.org/), [Pijul](https://pijul.org/) |
| Reference impls  -  commercial/managed | GitHub, GitLab, Bitbucket, Azure Repos |
| Interface standard / grammar | **No formal cross-vendor standard.** De facto: the **[Git pack/wire protocol](https://git-scm.com/docs/gitprotocol-pack)** ([v2](https://git-scm.com/docs/gitprotocol-v2), [smart HTTP](https://git-scm.com/docs/gitprotocol-http)). |
| Client SDKs / connection tooling (+ install) | `git` CLI (**install**); `libgit2` + bindings (`git2-rs`, pygit2, NodeGit) or JGit (Java)  -  **install/link** |
| Connection method (port/path/protocol) | Git over **SSH (TCP 22)**, **HTTPS (443, smart-HTTP)**, the unauthenticated [`git://` daemon](https://git-scm.com/docs/git-daemon) on TCP **9418**, or a local `.git` repo on a filesystem path |
| **Loom today** | `vcs` facade **built** in `loom-core::vcs` (commit/branch/checkout/diff/log + 3-way merge/rebase/squash/cherry-pick; deterministic conflict rule). Sync engine core built (`loom-core::sync`); CLI verbs + network transport remain. |

### 4. `kv`  -  key-value store (spec 0019)

| Dimension | Detail |
| --- | --- |
| Reference impls  -  OSS | [Redis](https://redis.io/) / [Valkey](https://valkey.io/), [etcd](https://etcd.io/), [RocksDB](https://rocksdb.org/), [LevelDB](https://github.com/google/leveldb), [Memcached](https://memcached.org/) |
| Reference impls  -  commercial/managed | AWS DynamoDB, Cloudflare Workers KV, Google Cloud Bigtable, Azure Cosmos DB (Table API), Couchbase KV service |
| Interface standard / grammar | **No single standard**  -  per-engine: Redis **[RESP](https://redis.io/docs/latest/develop/reference/protocol-spec/)**; Memcached text/binary protocol; etcd **[gRPC API v3](https://etcd.io/docs/latest/learning/api/)** |
| Client SDKs / connection tooling (+ install) | `redis-cli` + clients (`redis-py`, node-redis, Jedis/Lettuce)  -  **install**; Memcached clients and CLIs  -  **install**; etcd `etcdctl` / gRPC clients  -  **install**; RocksDB/LevelDB are **embedded libraries** linked in-process (no server) |
| Connection method (port/path/protocol) | Redis: TCP **6379** (RESP, TLS variant common); etcd: **2379** client / 2380 peer (gRPC over HTTP/2); Memcached: **11211**; embedded engines open a local on-disk path (no port) |
| **Loom today** | `kv` facade **built** in `loom-core::kv` (typed `Value` map over an ordered `BTreeMap`, range queries, canonical codec). In-process only. Redis, Memcache, etcd-class, and Couchbase-KV compatibility are presentations over this base layer, not separate storage models. |

### 5. `document`  -  JSON/document database (spec 0020)

| Dimension | Detail |
| --- | --- |
| Reference impls  -  OSS | [MongoDB](https://www.mongodb.com/) (SSPL), [Apache CouchDB](https://couchdb.apache.org/), [RavenDB](https://ravendb.net/), [ArangoDB](https://arangodb.com/), Couchbase Community Edition, RethinkDB |
| Reference impls  -  commercial/managed | MongoDB Atlas, Amazon DocumentDB, Azure Cosmos DB (MongoDB API), Couchbase Server / Capella |
| Interface standard / grammar | **No ISO standard.** De facto: the **[MongoDB Wire Protocol](https://www.mongodb.com/docs/manual/reference/mongodb-wire-protocol/)** (OP_MSG) + the [query language](https://www.mongodb.com/docs/manual/crud/), over **[BSON](https://bsonspec.org/)**; Couchbase-style document access combines KV operations, JSON document storage, SQL++/N1QL-like query, and analytics services. |
| Client SDKs / connection tooling (+ install) | Official MongoDB drivers per language (PyMongo, node `mongodb`, Java, Go, Rust, ...) + `mongosh` shell  -  **install** the driver/shell; connect via `mongodb://` / `mongodb+srv://` URIs. Couchbase clients and CLI tools are separate install targets if that presentation is promoted. |
| Connection method (port/path/protocol) | MongoDB TCP **27017** over the binary MongoDB Wire Protocol; Couchbase data/query/admin services conventionally use **11210** and HTTP ports such as **8091**. |
| **Loom today** | `document` facade **built** in `loom-core::document` (id-keyed collections of opaque document bytes, BTreeMap order, canonical codec). Secondary indexes deferred; schema-on-read. In-process only. Couchbase is primarily a document presentation target, with explicit KV, SQL, and columnar overlap. |

### 6. `sql`  -  relational / SQL database (spec 0011)

| Dimension | Detail |
| --- | --- |
| Reference impls  -  OSS | [PostgreSQL](https://www.postgresql.org/), [MySQL](https://www.mysql.com/) / [MariaDB](https://mariadb.org/), [SQLite](https://sqlite.org/), [GlueSQL](https://github.com/gluesql/gluesql), [CockroachDB](https://www.cockroachlabs.com/) |
| Reference impls  -  commercial/managed | Oracle Database, Microsoft SQL Server, Amazon Aurora / RDS, Google Cloud SQL |
| Interface standard / grammar | **[ISO/IEC 9075 (SQL:2023)](https://www.iso.org/standard/76584.html)**  -  Part 2 Foundation. Open grammar sources: PostgreSQL Bison **[`gram.y`](https://github.com/postgres/postgres/blob/master/src/backend/parser/gram.y)**; MySQL `sql/sql_yacc.yy` ([mysql-server](https://github.com/mysql/mysql-server)) |
| Client SDKs / connection tooling (+ install) | JDBC / ODBC drivers + per-language clients (psycopg/asyncpg, mysql-connector, node-postgres) + CLIs (`psql`, `mysql`)  -  **install**. SQLite / GlueSQL are **embedded** (link the library, no server). |
| Connection method (port/path/protocol) | PostgreSQL TCP **5432** ([F/B wire protocol](https://www.postgresql.org/docs/current/protocol.html)); MySQL/MariaDB TCP **3306** (X Protocol [33060](https://dev.mysql.com/doc/mysql-port-reference/en/mysql-port-reference-tables.html)); SQLite is in-process file access (no port) |
| **Loom today** | `sql` facade **partial**: `loom-core::tabular` (typed schema, PK-indexed rows, metadata pre-filter scan, canonical codec) + `loom-sql` adapting **GlueSQL** (CREATE/INSERT/SELECT/DELETE, snapshot persistence, commit/branch/checkout). Polars analytical path & row-level prolly diff/merge deferred (ADR-0008). In-process only. |

### 7. `columnar`  -  analytical / OLAP columnar storage (spec 0023)

| Dimension | Detail |
| --- | --- |
| Reference impls  -  OSS | [DuckDB](https://duckdb.org/), [ClickHouse](https://clickhouse.com/), [Apache Parquet](https://parquet.apache.org/) / [Arrow](https://arrow.apache.org/), [Polars](https://pola.rs/), [Apache Druid](https://druid.apache.org/) |
| Reference impls  -  commercial/managed | Snowflake, Google BigQuery, Databricks (Delta Lake / Photon), Amazon Redshift |
| Interface standard / grammar | **[Apache Arrow columnar format](https://arrow.apache.org/docs/format/Columnar.html)** (in-memory) + **[Apache Parquet format](https://github.com/apache/parquet-format)** (on-disk); transport via **[Arrow Flight RPC](https://arrow.apache.org/docs/format/Flight.html)** |
| Client SDKs / connection tooling (+ install) | DuckDB **embedded** (in-process; install the lib/CLI, no server); Parquet/Arrow readers (PyArrow, parquet-java); Arrow Flight clients over gRPC; ClickHouse via `clickhouse-client`/HTTP/JDBC  -  **install**; Polars is an in-process DataFrame lib |
| Connection method (port/path/protocol) | DuckDB **in-process** (file or memory, no port); ClickHouse TCP **9000** (native) / **8123** (HTTP); Arrow Flight over gRPC (conventional **8815**, not IANA-registered); Parquet/Arrow files read directly from filesystem/object store |
| **Loom today** | `columnar` facade **built** in `loom-core::columnar` (typed columns, append-only rows, target-size segment rolling, canonical codec). Polars native accelerator deferred (ADR-0008). In-process only. |

### 8. `time-series`  -  time-series database (spec 0022)

| Dimension | Detail |
| --- | --- |
| Reference impls  -  OSS | [InfluxDB](https://www.influxdata.com/), [TimescaleDB](https://www.timescale.com/), [Prometheus](https://prometheus.io/), [QuestDB](https://questdb.io/), [VictoriaMetrics](https://victoriametrics.com/) |
| Reference impls  -  commercial/managed | InfluxDB Cloud, Amazon Timestream, Grafana Cloud (Mimir), Timescale Cloud |
| Interface standard / grammar | **No cross-vendor ISO standard.** De facto: InfluxDB **[Line Protocol](https://docs.influxdata.com/influxdb/v2/reference/syntax/line-protocol/)** (write) + **[InfluxQL](https://docs.influxdata.com/influxdb/v1/query_language/)** / [Flux](https://docs.influxdata.com/flux/v0/); Prometheus **[PromQL](https://prometheus.io/docs/prometheus/latest/querying/basics/)** |
| Client SDKs / connection tooling (+ install) | InfluxDB client libs + `influx` CLI / any HTTP writer of Line Protocol (**install** client); Prometheus is scraped + queried over HTTP (plain HTTP client); TimescaleDB uses **standard PostgreSQL drivers** (it is a PG extension) |
| Connection method (port/path/protocol) | InfluxDB HTTP API TCP **8086**; Prometheus HTTP/UI/API TCP **9090**; TimescaleDB over PostgreSQL wire on TCP **5432** |
| **Loom today** | `time-series` facade **built** in `loom-core::timeseries` (points keyed by i64 ms-timestamp, range queries, latest-point). Rollups specified as derived views. In-process only. |

### 9. `queue`  -  append-only log / message queue (spec 0021)

| Dimension | Detail |
| --- | --- |
| Reference impls  -  OSS | [Apache Kafka](https://kafka.apache.org/), [RabbitMQ](https://www.rabbitmq.com/), [NATS / JetStream](https://nats.io/), [Redpanda](https://redpanda.com/), [Apache Pulsar](https://pulsar.apache.org/) |
| Reference impls  -  commercial/managed | Confluent Cloud, Amazon MSK, Amazon Kinesis Data Streams, Redpanda Cloud |
| Interface standard / grammar | **[Kafka binary protocol](https://kafka.apache.org/protocol)** (de facto); **[AMQP 0-9-1](https://www.rabbitmq.com/amqp-0-9-1-protocol)**; **AMQP 1.0** is the OASIS / **ISO/IEC 19464** standard ([amqp.org](https://www.amqp.org/)); [NATS protocol](https://docs.nats.io/reference/reference-protocols/nats-protocol) (text-based) |
| Client SDKs / connection tooling (+ install) | Kafka: Java client + `librdkafka`-based clients (`confluent-kafka`)  -  **install**; RabbitMQ: AMQP clients (`pika`, `amqplib`)  -  **install**; NATS: `nats.go`/`nats.py`/`nats.js`  -  **install**; CLIs `kafka-console-*`, `nats` |
| Connection method (port/path/protocol) | Kafka TCP **9092** (SASL_SSL often 9093); AMQP TCP **5672** (5671 TLS); NATS TCP **4222**; Pulsar binary TCP **6650** (HTTP admin 8080) |
| **Loom today** | `queue` facade **built** in `loom-core::log` (append-only entries keyed by monotonic `seq`, range-by-seq, canonical codec). Single-writer-per-stream model. In-process only. |

### 10. `ledger` - append-only verifiable log (spec 0018)

| Dimension | Detail |
| --- | --- |
| Reference impls - OSS | [Hyperledger Fabric](https://www.hyperledger.org/projects/fabric), [Google Trillian](https://github.com/google/trillian), [Sigstore Rekor](https://docs.sigstore.dev/logging/overview/) |
| Reference impls - commercial/managed | Azure SQL Database ledger, managed Hyperledger Fabric offerings |
| Interface standard / grammar | No single ledger-DB standard. Transparency-log references include RFC 6962 and RFC 9162, but Loom's active direction is native structured ledger storage, signed checkpoints, and derived proof artifacts. |
| Client SDKs / connection tooling (+ install) | Fabric Gateway SDKs, Trillian generated gRPC clients, Rekor tooling. immudb compatibility is not an active target. |
| Connection method (port/path/protocol) | Fabric peer gRPC 7051, Trillian log server gRPC 8090, native Loom ledger REST/JSON-RPC/gRPC profiles. |
| **Loom today** | `ledger` facade built with native hosted REST/JSON-RPC append/get/head/len/verify and native gRPC range/checkpoint/proof subset. Product-clone ledger database compatibility is not active scope. |

### 11. `graph` - property-graph database (spec 0016)

| Dimension | Detail |
| --- | --- |
| Reference impls  -  OSS | [Neo4j Community](https://neo4j.com/), [JanusGraph](https://janusgraph.org/), [ArangoDB](https://arangodb.com/), [Dgraph](https://dgraph.io/), [Memgraph](https://memgraph.com/), [Apache TinkerPop](https://tinkerpop.apache.org/) |
| Reference impls  -  commercial/managed | Neo4j Aura, Amazon Neptune, ArangoGraph, TigerGraph Cloud |
| Interface standard / grammar | ISO/IEC 39075:2024 GQL, openCypher grammar, and Neo4j Bolt for product compatibility. Gremlin is cut from the current roadmap. |
| Client SDKs / connection tooling (+ install) | Neo4j Bolt drivers (Python/Java/JS/Go/.NET), `cypher-shell`, Neo4j Browser/Bloom. |
| Connection method (port/path/protocol) | Neo4j Bolt TCP 7687; Neo4j HTTP/Browser 7474/7473. |
| **Loom today** | `graph` facade, structured storage, bounded openCypher/GQL query profile, native hosted REST/JSON-RPC/gRPC subsets, and bounded `neo4j/tcp` Bolt subset are source-backed. Full Neo4j compatibility remains target. |

### 12. `vector`  -  vector / embedding database with ANN (spec 0017)

| Dimension | Detail |
| --- | --- |
| Reference impls  -  OSS | [Qdrant](https://qdrant.tech/), [Milvus](https://milvus.io/), [Weaviate](https://weaviate.io/), [pgvector](https://github.com/pgvector/pgvector), [FAISS](https://github.com/facebookresearch/faiss), [hnswlib](https://github.com/nmslib/hnswlib) |
| Reference impls  -  commercial/managed | Pinecone, Weaviate Cloud, Qdrant Cloud, Zilliz Cloud (managed Milvus) |
| Interface standard / grammar | **No formal standard** - de facto per-product APIs: [Qdrant REST + gRPC](https://api.qdrant.tech/), Pinecone REST, [Milvus gRPC](https://milvus.io/docs), [Weaviate REST/GraphQL/gRPC](https://weaviate.io/developers/weaviate/api); pgvector exposes vectors through **SQL** with `<->`/`<=>`/`<#>` operators. Algorithm reference: **[HNSW](https://arxiv.org/abs/1603.09320)**. |
| Client SDKs / connection tooling (+ install) | `qdrant-client`, Pinecone SDKs, `pymilvus`, `weaviate-client` (REST/gRPC) - **install**; pgvector is a **Postgres extension** (`CREATE EXTENSION vector;`, then any PG driver); FAISS / hnswlib are **in-process** libraries (`pip install`) |
| Connection method (port/path/protocol) | Qdrant REST **6333** / gRPC **6334**; Pinecone HTTPS REST; Milvus gRPC **19530** (health 9091); Weaviate HTTP **8080** / gRPC 50051; pgvector via PostgreSQL **5432**; FAISS / hnswlib in-process (no port) |
| **Loom today** | `vector` facade **built**: `loom-core::vector` (deterministic exact top-k, id-keyed vectors + metadata pre-filter, fixed dim/metric, canonical codec) + `loom-hnsw` (native-only HNSW accelerator behind a trait, reconciled to exact results, never synced; auto-switch at corpus > 4096). In-process only. |

### 13. `search`  -  full-text search engine (spec 0033)

| Dimension | Detail |
| --- | --- |
| Reference impls  -  OSS | [OpenSearch](https://opensearch.org/), [Elasticsearch](https://www.elastic.co/elasticsearch), [Apache Solr](https://solr.apache.org/), [Tantivy](https://github.com/quickwit-oss/tantivy), [Meilisearch](https://www.meilisearch.com/), [Typesense](https://typesense.org/) |
| Reference impls  -  commercial/managed | Elastic Cloud, Amazon OpenSearch Service, Algolia, Meilisearch Cloud |
| Interface standard / grammar | **No ISO standard.** De facto: [Elasticsearch Query DSL](https://www.elastic.co/docs/reference/query-languages/querydsl) / [OpenSearch Query DSL](https://docs.opensearch.org/latest/query-dsl/) (JSON); [Apache Lucene query parser](https://lucene.apache.org/core/9_0_0/queryparser/org/apache/lucene/queryparser/classic/package-summary.html) syntax; ranking reference **[Okapi BM25](https://en.wikipedia.org/wiki/Okapi_BM25)** |
| Client SDKs / connection tooling (+ install) | Primarily HTTP/REST (usable with plain `curl`); official clients `elasticsearch` / `opensearch-py`, `pysolr`, `meilisearch`, `typesense`  -  **install**; Tantivy is an **in-process Rust library** (or `tantivy-py`) |
| Connection method (port/path/protocol) | OpenSearch / Elasticsearch HTTP REST **9200** (transport 9300); Solr HTTP **8983**; Meilisearch **7700**; Typesense **8108**; Tantivy in-process (no port) |
| **Loom today** | `search` facade is built; collection-local full-text management is `loom fts` and served `fts`. Bounded OpenSearch-compatible REST/NDJSON, native hosted REST/JSON-RPC/gRPC subsets, optional Tantivy-derived payload rebuild/readiness, portable aggregations, aliases, and multi-index search are source-backed where owning specs state them. Full analyzer execution, nested/pipeline aggregations, and broader client certification remain target. |

---

## Companion documents

This landscape feeds two companions:

- **Phase 2  -  [`IMPLEMENTATION-FIDELITY.md`](./IMPLEMENTATION-FIDELITY.md).** The deep per-facet comparison
  of Loom's implementation against each reference above: interface/grammar fidelity (bespoke vs faithful
  subset), the semantic give-and-take of the versioned substrate, a GreenYellowOrangeRed risk flag per facet, the
  exposure gap analyzed per facet, and the conformance anchors (POSIX for `files`, git for `vcs`, ...).
- **Binding plan  -  [`P9-0001-binding-architecture.md`](./P9-0001-binding-architecture.md).** The P9 plan
  for actually exposing these facets over the wire  -  the IDL->REST/JSON-RPC/gRPC projection of spec 0008,
  the MCP server, FUSE, and the optional foreign-protocol adapters  -  with a per-facet binding matrix and
  open questions.
