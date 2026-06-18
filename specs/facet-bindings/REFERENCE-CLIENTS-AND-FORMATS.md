# Reverse Mapping  -  File Formats & GUI Clients per Facet

**Series:** P9 companion (informative)
**Version:** 0.1.0-draft - **Status:** Draft companion, reconciled with current hosted surface map.
**Last updated:** 2026-07-13
**Companion to:** [`REFERENCE-IMPLEMENTATIONS.md`](./REFERENCE-IMPLEMENTATIONS.md) (which lists the
*engines*), [`P9-0001-binding-architecture.md`](./P9-0001-binding-architecture.md) (the bindings these
clients would use).

`REFERENCE-IMPLEMENTATIONS.md` maps each facet to the *databases/engines* in its category. This doc maps
each facet **in reverse**, to the two ways a human actually touches that shape of data:

- **At rest  -  a file.** A spreadsheet *is* an `.xlsx`; a SQLite database *is* a `.db`. The file is the
  facet's data serialized into a portable artifact. (Table A.)
- **Interactively  -  a GUI/app.** A user edits a spreadsheet in **Google Sheets/Excel**, browses a SQL
  database in **Sequel Ace/DBeaver**, a graph in **Neo4j Browser**. The GUI is not a file  -  it is a client
  that **connects to a live backend over a protocol**. (Table B.)

The key insight for Loom: **Table B's "connects via" column is exactly the Tier-2 foreign adapter from the
P9 plan.** A `.loom` `sql` workspace becomes reachable by Sequel Ace because of the PostgreSQL or MySQL
wire adapters (`P9-0006`); by Grafana because of the time-series HTTP datasource adapter (`P9-0010`);
and so on. The file formats in Table A are the facet's import/export/at-rest form (Loom interchange,
spec 0012).

---

## Table A  -  File formats (the at-rest / interchange view)

One row per facet; the first column lists the representative file format(s) that serialize that workspace.

| File format(s) | Workspace (facet) | Format standard | Loom interchange (0012)? | Notes |
| --- | --- | --- | --- | --- |
| **`.xlsx`, `.csv`, `.sql` dump, SQLite `.db`** | `sql` / tabular | OOXML (xlsx), CSV (RFC 4180), SQLite file format | **Yes**  -  `import_table` / `export_table` | **SQLite `.db` = a whole relational DB in one file**  -  the closest single-file analog to a `sql` workspace (just as `.xlsx` <-> Google Sheets) |
| **`.parquet`, `.arrow`/`.feather`, `.orc`** | `columnar` | Apache Parquet / Arrow / ORC | Partial | Arrow IPC and Parquet import/export are source-backed behind the columnar-arrow feature; durable Parquet segment storage remains target |
| **any file; `.tar`, `.zip`, OCI layer** | `files` | POSIX / tar / zip / OCI | **Yes**  -  `import_fs` / `checkout` | the facet *is* files; an archive about a directory snapshot |
| **`.loom` bundle; git `.bundle`** | `vcs` | Loom bundle (0006 section8); git bundle | **Yes**  -  Loom bundle export/import (git import excluded, 0012) | a whole repo + history in one portable file |
| **`.car`; git `.pack`; OCI image tar** | `cas` | CARv1, git packfile, OCI | Partial | CAR import/export and OCI Distribution serving are source-backed subsets; IPFS/Kubo compatibility is cut from the current roadmap |
| **`.json`, `.jsonl`/NDJSON, `.bson`** | `document` | JSON (RFC 8259), BSON |  -  | one doc per line (JSONL) is the natural dump |
| **`.json`/`.csv` export; Redis `.rdb`; RocksDB `.sst`** | `kv` | RDB (de facto); SST (engine-internal) |  -  | no standard interchange; `.rdb`/`.sst` are engine-internal |
| **line protocol `.lp`, `.csv`; InfluxDB `.tsm`** | `time-series` | Influx line protocol (de facto); TSM (engine) |  -  | `.lp`/`.csv` portable; `.tsm` engine-internal |
| **`.jsonl` export; Kafka segment `.log`+`.index`** | `queue` | none portable; Kafka log (engine) |  -  | no standard interchange file; export as JSONL |
| **`.jsonl` of entries + signed checkpoint** | `ledger` | none |  -  | no standard ledger file; export = entries plus checkpoint/head |
| **embeddings as `.parquet`/`.npy`/`.jsonl`; FAISS `.index`, hnswlib `.bin`** | `vector` | none (engine-specific indexes) |  -  | vectors are portable as parquet/npy; the **index** is non-portable/derived |
| **source docs `.jsonl`; Lucene/Tantivy index dir** | `search` | none (index directory is engine-specific) |  -  | index is derived/non-portable; ship the source documents and mapping |
| **`.graphml`, `.gexf`, GraphSON, `.cypher`, node/edge `.csv`** | `graph` | GraphML, GEXF, GraphSON (de facto) |  -  | several interchange formats exist |

**Observations.** Only `files`, `vcs`, and `sql`/tabular have a real Loom interchange path today (0012);
the rest would need import/export work. The facets with a clean **"whole dataset in one file"** analog
(`sql`->SQLite/.xlsx, `vcs`->.loom bundle, `cas`->.car, `columnar`->.parquet) are the ones where the
file-as-workspace mental model is tightest; `kv`/`queue`/`ledger`/`vector`/`search` have **no standard
interchange file** (their on-disk forms are engine-internal or derived).

---

## Table B  -  GUI / application clients (the interactive view)

One row per facet; the first column lists representative GUI/desktop/web tools. **"Connects via"** names
the protocol the tool speaks  -  i.e. the Tier-2 adapter a `.loom` would need to serve it.

| GUI / application | Workspace (facet) | Platform | Connects via (-> P9 Tier-2) | Notes |
| --- | --- | --- | --- | --- |
| Sequel Ace, TablePlus, DBeaver, DataGrip, pgAdmin, Beekeeper | `sql` | mac / cross / win | **PostgreSQL wire / MySQL wire** (P9-0006) | live DB clients reach a `.loom` through first-class `postgres/tcp` and `mysql/tcp` compatibility surfaces |
| Excel, Google Sheets, Apple Numbers | `sql` / tabular | desktop / web | **file** (`.xlsx`/`.csv`) - not live | spreadsheet UIs are file-based, not live DB clients |
| MongoDB Compass, Studio 3T, NoSQLBooster | `document` | cross | **MongoDB wire** (P9-0008, deferred) | native document indexes/query are source-backed; MongoDB product compatibility remains P3/spec-owned |
| Couchbase Web Console / Capella UI | `document` primary; `kv` / `sql` / `columnar` overlap | web | **Couchbase-style document, KV, query, and analytics services** (P9-0008 and P9-0007 planning) | primary placement is `document`; KV/query/analytics presentations are cross-facet |
| Tableau, Power BI, Apache Superset, Tad, DuckDB UI | `columnar` / `sql` | desktop / web | **Arrow Flight / Parquet / SQL** (P9-0009) | Arrow IPC and Parquet import/export are source-backed; Arrow Flight, Flight SQL, and warehouse-style profiles remain target |
| Grafana, Chronograf, InfluxDB UI, Prometheus UI | `time-series` | web | **HTTP API / line protocol** (P9-0010) | Influx, Prometheus, Grafana, and OTLP HTTP-compatible served surfaces have source-backed subsets |
| AKHQ, Conduktor, Offset Explorer, Redpanda Console | `queue` | desktop / web | **Kafka protocol** (P9-0011) | Kafka TCP has a bounded source-backed subset; RabbitMQ and AMQP remain separate target candidates |
| RedisInsight, Memcached clients/admin tools, etcdkeeper | `kv` | desktop / web | **RESP / Memcached protocol / etcd gRPC** (P9-0007) | Redis and Memcached are first-class compatibility surfaces; etcd remains a separate compatibility candidate |
| Qdrant Web UI, Weaviate console, Attu (Milvus), Pinecone console | `vector` | web | **vendor REST/gRPC** (P9-0012) | vector compatibility uses explicit profiles when selector semantics remain vector-owned |
| Kibana, OpenSearch Dashboards, Elasticvue | `search` | web | **OpenSearch REST/NDJSON over `fts`** (P9-0013) | `search` is store-wide discovery; collection-local full-text serving uses `fts` |
| Neo4j Browser/Bloom, Memgraph Lab, Arrows.app, Cytoscape, Gephi | `graph` | web / desktop | **Neo4j Bolt / bounded openCypher / GraphML** (P9-0014) | Neo4j is first-class `neo4j/tcp`; Gremlin is cut from the current roadmap |
| OCI registry UIs (Harbor, Docker Desktop) | `cas` | desktop / web | **OCI Distribution** (P9-0005) | OCI serving is source-backed for the bounded registry subset; IPFS/Kubo is cut |
| Ledger verification and transparency tooling | `ledger` | cross / CLI | **native ledger gRPC/REST/JSON-RPC profiles** (P9-0015) | ledger focuses on Loom-native structured storage, signed checkpoints, and derived proofs, not immudb compatibility |
| Finder, Explorer, Nautilus; Cyberduck, Transmit | `files` | mac / win / linux | **FUSE mount** (P9-0017) or **S3** (P9-0003) | Cyberduck/Transmit speak S3; FUSE remains a mount path, not a served facet |
| GitHub Desktop, Sourcetree, GitKraken, Tower, Fork | `vcs` | cross | git - **excluded** (P9-0004) | git GUIs cannot attach to a `.loom`; a native Loom GUI would be needed |

**Observations.**

- **Most GUI ecosystems are unlocked by exactly one Tier-2 adapter** - so the value of building each
  adapter is legible: PostgreSQL wire unlocks the entire SQL-GUI ecosystem; the
  time-series HTTP adapter unlocks Grafana, the single most valuable client in that space.
- **Two facets break the pattern.** `vcs`: git GUIs cannot connect (git interop is excluded, 0012) - Loom
  needs its *own* version-control UI. Spreadsheets (`sql`/tabular): Excel/Sheets are **file-based**, so the
  path is import/export (Table A), not a live adapter.
- **This table is a Tier-2 prioritization aid.** Adapters whose GUI ecosystem is large and mature
  (SQL-wire, Grafana/TSDB, Kafka UIs, ES/Kibana) are high-value. Fragmented ecosystems such as KV should
  be modeled as presentation families, not as exceptions or reasons to leave the facet underdesigned.
