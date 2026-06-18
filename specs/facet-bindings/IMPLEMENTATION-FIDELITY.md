# Loom Data Facets - Implementation Fidelity vs Reference Software (Phase 2)

**Spec series:** Loom Core Specification (LCS) - companion (informative)
**Version:** 0.1.0-draft
**Status:** Draft companion, reconciled with current hosted surface map.
**Last updated:** 2026-07-13

This is the companion to [`REFERENCE-IMPLEMENTATIONS.md`](./REFERENCE-IMPLEMENTATIONS.md) (Phase 1). Phase 1
mapped each data facet to the industry software that defines its category and the standards/ports those
references expose. This document asks the harder question for each facet:

> **Is our implementation bespoke, or does it faithfully carry a subset of the reference's interface,
> grammar, or algorithm, and where does it diverge?**

It is grounded in the actual source and the layer specs, not on intent. Graph, ledger, search, columnar,
SQL, time-series, queue, CAS, files, and other facets now have bounded hosted or compatibility subsets
where the owning specs say so. Full product compatibility remains target unless executable evidence
backs the specific row.

## How to read the verdicts

Fidelity is not one axis. A facet can clone a reference's *behavior* while sharing none of its *wire
format*. We score three separate things:

1. **Interface / grammar fidelity** - does a caller talk to Loom the way they'd talk to the reference
   (same query language, same protocol, same API shape), or is it a brand-new Rust API?
2. **Model / semantic fidelity** - does the data model and its operations match the reference's
   semantics (and where does Loom give *more* - history, branch/merge - or *less* - no indexes, no
   concurrency)?
3. **Exposure fidelity** - can anything actually *connect*, and which rows are bounded source-backed
   subsets rather than full compatibility claims?

Verdict vocabulary used in the table:

- **Real-grammar subset** - Loom adopts a genuine third-party grammar/parser, exposing a subset.
- **Faithful semantics, bespoke format** - behavior tracks the reference closely; on-disk format, API,
  and protocol are all Loom's own (no interop).
- **Faithful algorithm/engine, bespoke API** - the core algorithm or engine is the industry one; the
  surface around it is Loom's.
- **Thin/bespoke abstraction** - a deliberately minimal Loom-native model that shares the reference's
  *shape* but little of its surface area.

### Risk flags

Separately from the verdict (which describes *what kind* of fidelity a facet has), each facet carries a
flag for *how much a reader should worry* about the gap between what it promises, its name, its
reference, and its own normative spec, and what actually exists:

- **Green** - faithful and sound; remaining gaps are deliberate, such as chosen non-interop, or cosmetic.
- **Yellow** - sound but thin or partial; the divergence is honestly scoped or explicitly deferred and
  is low-risk.
- **Orange** - the name/spec promises materially more than the build delivers, **or the build diverges
  from the facet's own normative model**; a reader could be misled and real rework remains.
- **Red** - a correctness, security, or architecture concern, or a build that actively contradicts a
  core invariant.

**There are no red flags today**  -  the substrate (content addressing, canonical encodings, determinism,
git-style merge, the exact-vector contract) is sound everywhere it is built. There is exactly **one
orange**, `columnar`, flagged for a specific, verified reason in the historical audit. Current 0023 has
since been reconciled around a Loom canonical manifest plus Arrow/Parquet interchange projections; durable
Parquet segment storage remains target. Everything else is green or yellow. Flags are an analytical judgment offered for the owner to confirm, not a
conformance result.

---

## Verdict summary

| Facet | Flag | Built? | Interface/grammar fidelity | Reference faithfully tracked | Primary divergence from the reference |
| --- | --- | --- | --- | --- | --- |
| `sql` | Yellow | partial | **Real-grammar subset** | GlueSQL dialect of SQL, plus bounded PostgreSQL/MySQL wire subsets | PostgreSQL/MySQL compatibility is source-backed only for bounded rows; prepared statements, COPY, broader catalogs, TLS, and full dialect parity remain target |
| `vcs` | Green | yes | **Faithful semantics, bespoke format** | Git's DAG model and merge semantics | BLAKE3 plus bespoke canonical objects, not git's format/pack/wire; deliberately non-interoperable with git |
| `files` | Yellow | yes | **Faithful semantics, bespoke API** | POSIX path/stat/mode model | Not the POSIX C/syscall API; FUSE remains separate; hosted files subset is bounded |
| `vector` | Green | yes | **Faithful algorithm, bespoke API** | Exact kNN plus HNSW/product quantization | Qdrant/Pinecone profiles remain explicit profile work; pgvector belongs under PostgreSQL |
| `cas` | Green | yes | **Thin/bespoke abstraction** | Content addressing, OCI/S3/CAR family where promoted | IPFS/Kubo is cut; S3 and OCI are first-class compatibility surfaces, not CAS transports |
| `kv` | Green | yes | **Thin/bespoke abstraction** | Ordered KV plus Redis/Memcached compatibility surfaces | Redis and Memcached are dedicated compatibility surfaces; etcd remains a separate candidate |
| `document` | Yellow | yes | **Native document collection with indexes/query** | The id-to-document shape of MongoDB and Couchbase foundations | MongoDB and Couchbase remain P3/spec-owned product compatibility candidates; CouchDB serving is cut |
| `columnar` | Yellow | yes | **Loom manifest plus Arrow/Parquet interchange** | The columnar/segmented model, Arrow IPC, Parquet import/export | Durable Parquet segment storage, Flight, warehouse profiles, and segment merge remain target |
| `time-series` | Yellow | yes | **Thin/bespoke abstraction plus product ingestion subsets** | Time-series point model with Influx/Prometheus/Grafana/OTLP subsets | Full query languages, remote read, and broad capability reporting remain target |
| `queue` | Yellow | yes | **Append log plus Kafka compatibility subset** | A durable ordered log and bounded Kafka TCP behavior | Consumer-group rebalance, multi-partition topics, and broader eventing products remain target |
| `graph` | Yellow | yes | **Native property graph with bounded openCypher/GQL** | Property graph, native query IR, and bounded Neo4j Bolt subset | Full Neo4j, graph merge, broader conformance, and Gremlin remain target or cut as specified |
| `ledger` | Yellow | yes | **Native append-only ledger with proof/checkpoint subset** | Loom-native structured ledger, signed checkpoints, derived proofs | Transparency witness publication, physical pruning, and product-clone ledger DB compatibility remain target or cut |
| `search` | Yellow | yes | **Portable FTS plus Tantivy/OpenSearch bounded subset** | Tantivy-derived native engine, BM25, OpenSearch REST/NDJSON subset | Full analyzer execution, nested/pipeline aggregations, and broader client certification remain target |

The single highest-fidelity *interface* is `sql` (it rides a real parser); the single highest-fidelity
*behavior* is `vcs` (it reproduces git's merge semantics, including crisscross resolution). Everything
else is a Loom-native abstraction that borrows the reference's shape. Several bounded hosted surfaces
are reachable today, but none should be described as full product compatibility without row-specific
conformance.

---

## Per-facet deep analysis

### `sql` - a faithful subset of a real SQL grammar (not bespoke)

This is the facet your example asked about, and the answer is clear: **Loom does not invent a SQL
grammar.** `crates/loom-sql` embeds GlueSQL, whose front end is `sqlparser-rs`, the
same parser used widely across the Rust data ecosystem. So the grammar Loom accepts is a genuine SQL
dialect (a subset of ISO/IEC 9075), and GlueSQL's planner/executor genuinely supports `SELECT` with
`WHERE`, `JOIN`, `GROUP BY`, `DISTINCT`, and aggregate functions.

Where the fidelity narrows is **below the parser**, in Loom's storage adapter (`LoomSqlStore`). GlueSQL
delegates all persistence to a `Store`/`StoreMut` trait pair, and Loom implements only part of it:

- **Implemented:** `insert_schema`, `delete_schema`, `append_data` (keyless/auto-id tables), `insert_data`
  (keyed upsert), ALTER through full table re-stage, direct table reads, table diff/blame, result views,
  and selected hosted SQL plus PostgreSQL/MySQL wire subsets.
- **Defaulted / not wired:** `Index`/`IndexMut` (no secondary indexes), `AlterTable` (no `ALTER`),
  `Transaction` (autocommit only for the Loom store adapter), broader prepared statements, COPY,
  full PostgreSQL/MySQL catalog compatibility, and complete hosted SQL certification.
  custom store like ours), `CustomFunction*`, and metadata introspection.

So the honest statement is: **Loom carries a faithful subset of a real SQL dialect, validated today at
`CREATE / INSERT / SELECT / DELETE`, on top of bespoke versioned storage.** `SELECT`'s richness
(filtering, joins, aggregates) is whatever GlueSQL's planner does over a full table scan of our rows;
the write/DDL surface is the limiting factor, not the query language. What Loom *adds* over any reference
SQL engine is orthogonal: each table is a content-addressed `tabular::Table`, the whole database is
snapshotted into a `sql/<db>` workspace path, and the database is therefore commit/branch/checkout/
merge-able. No MySQL or PostgreSQL does that. Loom now exposes bounded PostgreSQL and MySQL wire
subsets, but it must not claim full PostgreSQL or MySQL compatibility without row-specific transcripts.

### `vcs` - git semantics without git's bytes

`vcs` is the most behaviorally faithful facet. `loom-core::vcs` implements the git mental model directly:
a commit DAG (`Commit { tree, parents, author, timestamp_ms, message, meta }`), branches as
compare-and-swap refs, first-parent `log`, path-level `diff` (Added/Modified/Deleted), and a real
**three-way merge** with fast-forward detection and a deterministic conflict list. It goes further than
naive merges by computing a **virtual merge base for crisscross histories** (an ORT-style recursive fold),
which is exactly what modern git does to avoid spurious conflicts.

But the *fidelity is in the semantics, not the format*. Objects are BLAKE3-addressed and serialized with
Loom's own canonical `[type:u8][len:uvarint][body]` codec - not git's zlib-compressed, SHA-1/SHA-256
objects, not packfiles, not the smart-HTTP/`git://` wire protocol. The project also rules git
interoperability **permanently out of scope** (0012). So: a git user would feel at home conceptually, but
no `git` client can clone a Loom, and Loom stores nothing git could read. Divergences from git proper:
no cherry-pick/interactive rebase yet, symlinks exist as an `EntryKind` but have no operations, and a
`files`-typed workspace is intentionally **linear** (rejects branch/merge).

### `files` - a POSIX-shaped model, not the POSIX API

`fs` reproduces the POSIX *model*: hierarchical paths, `stat` with a `kind`/`size`/`mode` (u32, e.g.
`0o100644`), explicit directories, `move`/`copy`/`walk`, and normalized paths. That is faithful to how
POSIX filesystems behave. It is **not** the POSIX C API or the syscall ABI. Hosted file operations are
bounded Loom protocol operations, not POSIX syscalls; broader handle and symlink projection remains target
(the kind exists; the ops don't). Directories are first-class and non-implicit (writing into a missing
parent fails), which is a deliberate, slightly stricter choice than some filesystems.

The decisive gap is exposure: the reference way to "connect" to a filesystem is to **mount it on a path**
via the kernel VFS or FUSE. Loom's FUSE mount is specified but unbuilt (P9). Until then the filesystem is
reachable only as an in-process Rust API, so none of the OS-level tooling (a `cd` into the mount, `ls`,
`cat`) applies. Conformance is anchored to POSIX behavior in spec 0025.

### `vector` - high algorithmic fidelity, bounded protocol fidelity

`vector` tracks the *algorithms* of the reference vector databases faithfully. The contract is **exact
top-k** (recall 1.0, byte-identical on native and `wasm32`), with the standard distance metrics
(`Cosine | L2 | Dot`, normalized so higher = more similar). Acceleration uses the real industry
algorithms - HNSW and product quantization - but as derived, never-stored, never-synced indexes that must
*reconcile to the exact result's scores and order*. That is a stronger correctness guarantee than most
vector DBs, which serve approximate results directly.

The divergences are in surface and richness, not correctness. There is no Qdrant REST/gRPC API, no
pgvector-style exact search is source-backed through the PostgreSQL compatibility surface, not as a
native vector transport. Native vector compatibility profiles remain explicit profile work. Net: if you
care about what answers come back, fidelity is high; if you care about product-specific client behavior,
the claim must stay profile-specific.

### `cas` - content addressing plus selected object/blob/archive presentations

Conceptually faithful to the content-addressed-store idea shared by Git objects and registry blobs:
`cas_put` returns a digest, `cas_get` verifies on read, and blobs deduplicate globally. Loom now also has
bounded S3, OCI, and CAR projections where the owning specs state them. IPFS/Kubo compatibility is cut
from the current roadmap. S3 and OCI are first-class compatibility surfaces over CAS/files primitives,
not generic CAS transports.

### `kv` - an ordered, typed map with dedicated Redis and Memcached presentations

`kv` is a `BTreeMap<Value, Vec<u8>>` with a deterministic **total order over typed keys** (Null < Bool <
Int < Float < Text < Bytes, with type-specific ordering), exposing `put/get/delete/iter/range` (half-open
ranges). The ordered-range capability makes it resemble an embedded ordered store (RocksDB/LevelDB, or
etcd's sorted keyspace) more than Redis. Redis and Memcached are dedicated served compatibility surfaces
over the KV/cache family rather than transports under native `kv`. The typed-key total order remains a
distinctive Loom decision.

### `document` - native document collection with indexes/query and deferred product ports

The native document base now includes document CRUD, indexes/query, hosted REST/JSON-RPC/gRPC subsets,
and source-backed native query behavior where the owning specs state it. MongoDB and Couchbase remain
P3 product compatibility candidates over that native base. CouchDB serving is cut from the current
roadmap because revision-tree and replication semantics are not a current strategic target.

### `columnar` - manifest-backed, not yet Arrow/Parquet-compatible

The built `columnar.rs` now stores a versioned canonical manifest with identity-affecting native
segments, profile-aware segment digests, deterministic segment statistics, `scan`, `select`,
`aggregate`, `compact`, `inspect`, and `source_digest`. This fixes the earlier gap where segment
boundaries were flattened out of identity.

The remaining **orange** flag is interoperability, not manifest identity. The source still does not
emit Arrow buffers or Parquet files, so there is no reference-ecosystem interop yet: no DuckDB read, no
Arrow Flight, no Parquet readers, no predicate pushdown, and no columnar compression profile. The fix is
still concrete: add deterministic Arrow/Parquet import/export and segment support, then layer Polars as
the native dataframe executor per 0045 rather than making Polars canonical columnar state.

### `time-series` - native timestamped series plus selected ecosystem presentations

`timeseries.rs` is `BTreeMap<i64 ms, Vec<u8>>` per series, with `put/get/range/latest/iter`. Faithful to
the *shape* of a TSDB point stream, but missing the things that define InfluxDB/Prometheus: **no tags/
labels/dimensions**, no line protocol or remote-write ingestion, no PromQL/InfluxQL/Flux query language,
and no built-in aggregation - rollups/downsampling are explicitly derived views (rebuilt, not stored,
resolved decision in 0022). Opaque values. It's a clean primitive, not a competitor to a TSDB's query
engine.

### `queue` - one durable ordered log plus bounded Kafka compatibility

`log.rs` is an append-only `Vec<Vec<u8>>` with monotonic positional `seq`, `append/get/range/iter`. This
is faithful to a single Kafka partition or a simple commit log. A bounded Kafka TCP compatibility subset
is now source-backed over the queue/coordinated metadata substrate, but full Kafka cluster, rebalance,
and multi-partition behavior remains target or unsupported as specified.

### `graph` - faithful property-graph model with bounded openCypher/GQL and Neo4j subsets

The data model is a faithful property graph with structured storage, typed graph values, property-index
declarations, native graph query IR, and a bounded GQL-aligned openCypher profile. Loom also has a
bounded `neo4j/tcp` Bolt subset with official-driver transcript evidence. Gremlin is cut from the
current roadmap. Full Neo4j compatibility, graph-specific merge, generated protobuf artifacts, and
broader hosted conformance remain target.

### `ledger` - native append-only ledger with checkpoint and proof subsets

The active direction is Loom-native structured ledger storage, canonical head metadata, sequence-keyed
entries, retained/pruned range metadata, principal-signed checkpoints, and derived proof artifacts.
Native hosted gRPC exposes a bounded range/checkpoint/proof subset. The current linear chain should not
be described as transparency-log-grade by itself. Witness publication, physical pruning, retention
scheduling, transparency-log behavior, and product-clone ledger DB compatibility remain target or cut as
specified.

### `search` - portable FTS plus Tantivy/OpenSearch bounded subsets

The portable source collection is source-backed, and the optional Tantivy-backed engine is source-backed
for the current BM25 and deterministic parity subset. Collection-local full-text management is `fts`;
top-level `search` is store-wide discovery. A bounded OpenSearch-compatible REST/NDJSON profile is
source-backed over `fts`, including aliases, multi-index search, bulk, msearch, selected query DSL rows,
selected aggregations, refresh no-op behavior, and read-only security shims. Full analyzer execution,
nested and pipeline aggregations, generated schemas, and broader client certification remain target.

---

## The exposure gap

Every reference in Phase 1 is something a person or tool connects to over a wire or a mount. Loom now has
source-backed hosted listener management, local MCP, many native hosted REST/JSON-RPC/gRPC subsets, and
selected compatibility surfaces. The remaining exposure gap is not "no ports exist"; it is that full
product compatibility, generated schemas, and broad reference-client certification are still row-by-row
target work.

Two consequences for this analysis:

1. **Interface fidelity is row-specific.** A surface can have a source-backed handshake, query subset,
   or import/export path without being fully compatible with the reference product.
2. **The faithful-binding target is explicit per facet.** "Tracking the reference" means opening the
   surface in the table below and proving each advertised row.

| Facet | What a faithful binding would expose (the P9 target) | Realistic near-term bridge |
| --- | --- | --- |
| `sql` | PostgreSQL- and MySQL-wire endpoints with full catalog, parameter, transaction, and driver behavior | Bounded `postgres/tcp` and `mysql/tcp` subsets |
| `files` | FUSE mount on a path plus hosted file protocol parity | Hosted files subset and archive import/export |
| `vcs` | Native Loom VCS protocol and sync bundles; git remote remains excluded | Native `loom` CLI verbs, sync bundles, hosted VCS subset |
| `kv` | RESP, Memcached, and etcd-class compatibility where promoted | Redis and Memcached dedicated surfaces; etcd candidate |
| `document` | Native document indexes/query plus MongoDB/Couchbase if promoted | Native document query/index subset |
| `vector` | Qdrant/Pinecone-style profiles and pgvector through PostgreSQL | Native vector plus pgvector-style PostgreSQL exact search |
| `columnar` | Arrow Flight, Parquet, Flight SQL, and analytical profile evidence | Arrow IPC/Parquet import/export and prepared result handles |
| `time-series` | Influx, Prometheus, Grafana, OTLP, and query API compatibility | Bounded HTTP ingestion/query subsets |
| `queue` | Kafka, MQTT, NATS/JetStream, AMQP where promoted | Bounded Kafka TCP subset |
| `search` | OpenSearch-compatible REST/NDJSON over `fts` | Bounded `fts/rest` and `fts/ndjson` subset |
| `graph` | Bounded openCypher/GQL and Neo4j driver compatibility | Native graph gRPC and bounded `neo4j/tcp` subset |
| `ledger` | Native ledger checkpoint/proof profiles and optional transparency witness behavior | Native ledger gRPC proof/checkpoint subset |

This table is the seed for the P9 binding work: [`P9-0001-binding-architecture.md`](./P9-0001-binding-architecture.md)
turns each row into a concrete two-tier binding plan (native IDL projection plus optional foreign adapter)
with a per-facet matrix and open questions.

---

## Conformance anchors (spec 0025)

Loom's behavioral-conformance approach is itself a fidelity statement: each facet's BDD scenarios are
**anchored to the established tool** so Loom gives the same assurances. `files` is anchored to POSIX
filesystem behavior; `vcs` to git; `cas` runs today; `sql`, `vector`, `graph`, `ledger`, and `search`
have scenario stubs pending their builds. This is the mechanism by which "faithful semantics" is meant to
be proven rather than asserted, and the gate that should accompany each binding when a facet
to the wire.

---

## Bottom line

- **Bespoke vs faithful:** Only `sql` adopts an external grammar (a real SQL subset via GlueSQL). `vcs`
  and `vector` are faithful to a reference's semantics or algorithms respectively, behind Loom-native
  APIs. The other facets are Loom-native bases with selected compatibility presentations layered over
  them.
- **The unifying value-add** across every facet is the substrate, not the surface: content addressing,
  versioning, branch/merge, and sync over each data shape. That is what no reference offers, and it is the
  reason the interfaces are bespoke.
- **The unifying gap** is full compatibility proof: bounded hosted surfaces exist, but full product
  compatibility requires route-by-route, method-by-method, and client-by-client conformance evidence.
