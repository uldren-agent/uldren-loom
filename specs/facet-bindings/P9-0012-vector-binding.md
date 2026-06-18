# P9-0012 - `vector` Binding

**Series:** P9 binding plan (normative-track sub-series; Draft)
**Version:** 0.1.0-draft | **Status:** Draft | **Last updated:** 2026-06-18
**Reads first:** [`P9-0002-projection-conventions.md`](./P9-0002-projection-conventions.md), facade spec
**0017 section 4** (Vector), ADR-0008, [`IMPLEMENTATION-FIDELITY.md`](./IMPLEMENTATION-FIDELITY.md) (exact
contract + real HNSW).

## 1. Facade surface (0017 section 4 `Vector`)

Current local facade: `create(name, dim, metric)`, `upsert(name, id, embedding, metadata)`,
`get(name, id) -> Embedding?`, `ids(name, prefix?) -> List<string>`, `delete(name, id) -> bool`,
and `search(name, query, k, filter) -> List<Hit>`. Embeddings cross local bindings as little-endian
`f32` bytes; metadata crosses as canonical CBOR `text -> cell`; the recursive filter is `All`,
equality, or AND. Codes use the core set, including `NOT_FOUND`, `CONFLICT`, `INVALID_ARGUMENT`, and
`DIMENSION_MISMATCH`.

**Source alignment:** 0017 and the current build use the `loom-vector` exact portable contract with
metadata pre-filtering and fixed dim+metric per set. Id listing is a sorted string-prefix scan across
the engine, IDL, C ABI, C header, CLI, and local bindings. The engine stores each set as a manifest plus
per-id entry files and metadata equality index markers. Metadata-index declaration is projected
through the CLI, C ABI, IDL, C header, Node, Python, C++, iOS, JVM, Android, React Native, and WASM
surfaces. Hosted REST and JSON-RPC now expose the native create/upsert/get/search subset over the
shared hosted kernel and daemon-opened listeners; hosted ids/delete/index/source/model routes remain
target work. Rust callers can choose exact-only or approximate-above-threshold acceleration with
`AcceleratorPolicy`. CLI and local binding callers can choose exact search or built-in PQ search above
an explicit threshold; hosted accelerator policy projection remains target work.
Source-aware upsert, source text reads, and embedding model profile reads are projected through the
CLI, C ABI, IDL, C header, Node, Python, C++, iOS, JVM, Android, React Native, and WASM. Checked-in
binding evidence covers the source/model path for C++, Swift, JVM, Android, and React Native. React
Native connected-device execution remains binding-certification work.

## 1.1 Binding Boundary

The base layer is the Loom vector set with exact deterministic search, fixed metric and dimension, typed
metadata, and optional source text/model profile. Native projections expose create, upsert, get, ids,
delete, search, source reads, metadata-index controls, and explicit search policy where source-backed.
Qdrant-shaped, Pinecone-shaped, pgvector-like, Milvus-like, and Weaviate-like surfaces are
presentations. NPY, Parquet, and JSONL vector dumps are interchange. HNSW/PQ indexes, source-digest
stamps, and candidate caches are derived artifacts and are never the synchronized source of truth.

## 1.2 CLI

The local CLI exposes `loom vector create`, `upsert`, `get`, `ids`, `index-keys`, `create-index`,
`drop-index`, `delete`, and `search`. Binary vectors and query embeddings cross as little-endian
`f32` files or stdin. Metadata and filters cross as canonical CBOR files. `get` writes canonical CBOR
`[vector_bytes, metadata]`; `ids`, `index-keys`, and `search` print text by default and can write
canonical CBOR with `--out`.

## 2. Tier-1 REST

Current source-backed hosted REST is the native action-shaped subset used by the served listener:
`/vector:create`, `/vector:upsert`, `/vector:get`, and `/vector:search`. The resource-shaped routes
below remain the target REST profile.

Facet-root `/v1/workspaces/{workspace_id}/vector-sets/{name}`:

| Facade method | HTTP |
| --- | --- |
| `create` | `PUT /vector-sets/{name} {dim, metric}` |
| `upsert` | `PUT /vector-sets/{name}/vectors/{id} {embedding, metadata}` |
| `get` | `GET /vector-sets/{name}/vectors/{id}` -> `Embedding?` |
| `delete` | `DELETE /vector-sets/{name}/vectors/{id}` -> `{found}` |
| `ids` | `GET /vector-sets/{name}/vectors?prefix={prefix}&list=1` -> NDJSON ids |
| `search` | `POST /vector-sets/{name}:search {query, k, filter}` -> NDJSON `Stream<Hit>` |

## 3. Tier-1 JSON-RPC / 4. gRPC

Current hosted JSON-RPC is source-backed for `vector.create`, `vector.upsert`, `vector.get`, and
`vector.search`. The full target JSON-RPC surface is 1:1
(`vector.create/upsert/get/ids/delete/search`). gRPC remains target: `Search` and `Ids`
server-streaming; `Create`, `Upsert`, `Get`, and `Delete` unary.

## 5. Tier-1 MCP

- **Read tools:** `vector.search`, `vector.get`, `vector.ids`.
- **Write tools (token-gated):** `vector.create`, `vector.upsert`, `vector.delete`.

## 6. Tier-2 foreign adapters - vector compatibility profiles

Vector database APIs have no single durable standard. Loom therefore exposes vendor-shaped profiles
over the `vector` surface rather than making one vendor API the base model. The served grammar uses
generic transports and requires an explicit compatibility profile:

```text
loom serve configure <store> vector <workspace> <collection> --transport rest --profile qdrant --bind 127.0.0.1:6333
loom serve configure <store> vector <workspace> <collection> --transport grpc --profile qdrant --bind 127.0.0.1:6334
loom serve configure <store> vector <workspace> <collection> --transport rest --profile pinecone --bind 127.0.0.1:8080
loom serve configure <store> vector <workspace> <collection> --transport rest --profile generic --bind 127.0.0.1:8081
```

There is no default profile for `vector` compatibility serving. Native Loom `vector/rest` remains a
Loom-shaped API. A compatibility listener must say whether it is `qdrant`, `pinecone`, `generic`, or a
later approved profile. This avoids a REST endpoint accidentally claiming Qdrant or Pinecone behavior.

Target profile matrix:

| Profile | Transport | Mapping | Initial target | Not the base contract |
| --- | --- | --- | --- | --- |
| `generic` | `rest`, later `grpc` | Loom-native collection, vector id, vector values, typed payload, exact search | Stable native hosted API over the base vector model | No vendor response envelope or vendor operational knobs |
| `qdrant` | `rest`, `grpc` | One Qdrant collection maps to one Loom vector presentation collection. Qdrant named vectors map to Loom vector sets under that presentation collection; the unnamed vector maps to the primary set. Points map to vector ids plus shared payload metadata. | Collections, points, upsert, get, search/query, scroll, count, payload/filter operations, and read-only collection info shaped for Qdrant clients | Qdrant optimizer, shard, replication, and cluster-management knobs are compatibility metadata or unsupported rows unless backed by Loom behavior |
| `pinecone` | `rest` | One Pinecone index view maps to one served Loom vector presentation collection. Pinecone workspaces remain profile-local partitions inside the served collection, not Loom workspaces, because the listener already selects a Loom workspace. | Upsert, fetch, query, delete, list, describe-index-shaped capability, metadata filters, and API-key authentication using 0026 app-specific credentials | Integrated embedding and serverless operational behavior are provider/profile features, not vector-set identity |
| `pgvector` | SQL presentation | Vector columns and operators project through the SQL facet when SQL wire work exists | `<->`, `<=>`, and related operator compatibility over SQL/query surfaces | Not a `vector/rest` or `vector/grpc` listener profile |
| `milvus` | `grpc` later | Milvus collection/partition semantics map through a future profile matrix | Deferred until Qdrant and Pinecone prove the common adapter substrate | Not implied by Qdrant gRPC |
| `weaviate` | `rest`, `grpc`, GraphQL later | Weaviate class/object/vector semantics need their own profile matrix | Deferred until document/object overlap is designed | Not implied by generic vector REST |

Compatibility rules:

- Loom hosted auth, ACL, PEP, stable errors, audit, request limits, and store save behavior remain
  authoritative for every profile.
- API-key-style vendor headers resolve through 0026 app-specific hosted credentials. A profile must not
  add its own credential store.
- Exact search is the default result contract. Approximate HNSW/PQ use is allowed only when the served
  listener policy explicitly permits approximate recall and the capability report marks the behavior.
- Payload metadata is a typed cell map, and the base predicate model is source-backed for equality,
  inequality, range, set-membership, existence, AND, OR, and NOT predicates. Qdrant and Pinecone JSON
  request-shape translation onto that model is source-backed in the hosted HTTP compatibility layer.
  Qdrant protobuf-shaped filter translation is source-backed in the hosted unary gRPC compatibility
  subset.
- Qdrant and Pinecone source-text or integrated-embedding flows store source text only when listener
  policy maps an input field to Loom source text. Actual embedding provider execution remains owned by
  0050; if no provider is configured, the profile returns a stable provider-not-configured or
  unsupported error.
- Operational fields such as HNSW parameters, optimizer settings, replicas, shards, and pod/serverless
  placement are accepted only as harmless compatibility metadata unless a Loom equivalent is enforced.
- Capability reporting must expose supported, unsupported, approximate, degraded, and native-only rows
  plus a conformance transcript using real clients where available.

Current source-backed status: Qdrant REST compatibility is implemented for collection create/info,
point upsert/get/delete, search/query, scroll, count, payloads, JSON filters, stable unsupported-filter
errors, profile-scoped primary and named vector mappings, hosted auth/PEP, request limits, and daemon
profile handoff. Qdrant gRPC compatibility is implemented as a hosted unary protobuf-shaped subset for
collection create/info, point upsert/get/delete, search/query, scroll, count, protobuf-shaped filters,
API-key metadata, hosted auth/PEP, request limits, stable errors, and daemon profile handoff.
Pinecone REST compatibility is implemented for index-specific describe stats, vector upsert, fetch,
query, delete, list, workspaces, metadata filters, API-key headers, exact-only capability reporting,
stable unsupported integrated-embedding and sparse-vector errors, hosted auth/PEP, request limits,
stable errors, and daemon profile handoff. pgvector-style SQL presentation is source-backed through
PostgreSQL-wire for bounded exact-search queries shaped as `SELECT id, embedding <op> '[..]' AS
distance FROM <vector-set> ORDER BY embedding <op> '[..]' LIMIT n`, with `<->` requiring L2 sets, `<=>`
requiring cosine sets, and `<#>` requiring dot-product sets. Milvus and Weaviate compatibility
listeners remain target work. Generated external-client Qdrant and Pinecone transcripts remain
conformance work.
The hosted protocol feature matrix inventories the current vector compatibility status: Qdrant REST,
Qdrant unary gRPC, and Pinecone REST are supported rows; hosted approximate accelerator policy,
integrated embedding, and sparse vector request shapes are unsupported rows unless a later explicit
listener/provider policy is implemented; generated Qdrant/Pinecone client transcripts and
Milvus/Weaviate listeners are target rows, while pgvector is a SQL-presentation row rather than a
vector listener row.
The source-backed vector implementation is the Loom base vector set, typed metadata, predicate
filtering, exact search, PQ accelerator helpers, native `loom-hnsw`, explicit served-vector profile
admission, durable listener profile storage, 0026 app-specific API-key authentication for vendor-style
headers, Qdrant/Pinecone JSON filter translation, Qdrant protobuf-shaped filter translation,
deterministic Qdrant/Pinecone presentation mapping, Qdrant REST and unary gRPC compatibility,
Pinecone REST compatibility, and the native hosted REST/JSON-RPC subset described above.

## 7. Errors / parity / concurrency

- **Errors:** `NOT_FOUND`, `CONFLICT`, `INVALID_ARGUMENT`, `DIMENSION_MISMATCH` (0017/0010 section 4)
  plus the core set.
- **Parity (0032):** exact search portable (native + `wasm32`, byte-identical); HNSW accelerator
  native-only; the ANN index is derived, never stored/synced.
- **Concurrency:** single writer; per-set dim/metric immutable after creation.

## 8. Open Questions

### OQ-VE1 - Reconcile 0017 facade text with the build (resolved)

- **Decision.** 0017 now matches the built exact-contract + metadata-filter model. Id-prefix scan is
  exposed as `ids`, not as the search filter.

### OQ-VE2 - Metadata-filter richness (resolved locally)

- **Decision.** The local source now supports typed metadata predicates for equality, inequality,
  range, set-membership, existence, AND, OR, and NOT. Qdrant JSON filters, Pinecone JSON filters, and
  Qdrant protobuf-shaped filters translate onto this model. Pinecone REST execution coverage proves
  the Pinecone JSON filter path, and Qdrant REST/gRPC coverage proves the Qdrant paths. The hosted
  protocol feature matrix carries capability rows for supported, unsupported, and target vector
  compatibility behavior. Remaining compatibility work covers generated external-client transcripts
  and future profiles. Exact, ordered search remains the invariant for exact-mode listeners.
