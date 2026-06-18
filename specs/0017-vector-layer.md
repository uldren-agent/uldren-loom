# 0017 - Vector Layer

**Status:** Local promotion complete; hosted/provider/certification targets remain. **Version:** 0.1.0.
**Capability:** `vector`.

This spec defines the vector facet: versioned embedding sets with deterministic exact search as the
portable contract and rebuildable accelerators as optional derived state. Current source implements
the reusable vector model and accelerator contracts in `loom-vector`, the engine storage facade in
`loom-core::vector`, a compatibility accelerator re-export in `loom-core::vindex`, a native HNSW
accelerator in `loom-hnsw` over `loom-vector`, the language-neutral local `Vector` facade, structured
manifest-plus-entry storage, vector-entry structural diffs, same-id vector-entry merge, explicit Rust
accelerator policy, maintained metadata equality indexes, the C ABI, the C header, the CLI projection,
all eight local binding projections for the base vector facade, broad local binding projection for
metadata-index declarations, local exact/PQ accelerator policy projection, local MCP data-tool
projection with output schemas and facade-boundary coverage, and executable facade conformance.
Source-text sidecar storage, embedding model profile tracking, Rust core provider-backed text upsert,
and source/model projection through the CLI, C ABI, C header, IDL, Node, Python, C++, Swift, JVM,
Android, React Native, and WASM are source-backed at the local API layer; source/model projection
through local MCP is also source-backed. First-class CLI commands that embed user text through a
configured inference instance and perform text query are source-backed through 0062. C++, Swift, JVM, Android,
and React Native have local smoke or build evidence for the
ergonomic source/model wrapper surface, including Swift source/model runtime coverage. Hosted REST and
JSON-RPC native projection and protocol conformance are source-backed for vector create, upsert, get,
and search through the shared hosted kernel and daemon-opened listeners. Concrete embedding
providers, hosted source/model routes, hosted accelerator policy projection beyond exact-only
compatibility reporting, hosted gRPC for the native Loom vector API, and recall-target accelerator
policy remain target work.
Qdrant/Pinecone JSON filter translation and deterministic presentation mapping are source-backed in the hosted HTTP
compatibility substrate. Qdrant REST compatibility is source-backed for collection create/info, point
upsert/get/delete, search/query, scroll, count, payloads, JSON filters, hosted auth/PEP, stable
errors, and daemon profile handoff. Qdrant gRPC compatibility is source-backed as a unary
protobuf-shaped subset for collection create/info, point upsert/get/delete, search/query, scroll,
count, protobuf-shaped filters, hosted API-key metadata, hosted auth/PEP, stable errors, and daemon
profile handoff. Pinecone REST compatibility is source-backed for index-specific describe stats,
vector upsert/fetch/query/delete/list, workspaces, metadata filters, API-key headers, exact-only
capability reporting, stable unsupported integrated-embedding/sparse-vector errors, hosted auth/PEP,
stable errors, and daemon profile handoff. Generated external-client transcripts remain conformance
target work. The conformance report inventories Qdrant REST, Qdrant unary gRPC, Pinecone REST,
exact-only behavior, unsupported approximate hosted acceleration, unsupported integrated-embedding and
sparse-vector request shapes, target generated-client transcripts, and target
Milvus/Weaviate/pgvector listeners from source-backed evidence.

Every operation is scoped to one workspace's vector facet. Cross-workspace vector writes are out of
contract and must fail with `CROSS_WORKSPACE`.

## 1. Current Implementation

`loom-vector` implements:

- `VectorSet::new(dim, metric)`;
- `dim`, `metric`, `len`, and `is_empty`;
- `upsert(id, vector, metadata)`;
- `get(id)`;
- `remove(id)`;
- `entries()`;
- `search(query, k, filter)`;
- `Metric`;
- `MetaFilter`;
- `Hit`;
- `VectorEntry`;
- `VectorAccelerator`;
- `AcceleratorPolicy`;
- `DEFAULT_EXACT_THRESHOLD`;
- `search_with_policy`;
- `search_auto`;
- `PqIndex`;
- `prune_csr`.

`loom-core::vector` integrates that reusable model with the Loom kernel and implements:

- `put_vector_set(loom, ns, name, set)`;
- `get_vector_set(loom, ns, name)`;
- `vector_create(loom, ns, name, dim, metric)`;
- `vector_upsert(loom, ns, name, id, vector, metadata)`;
- `vector_get(loom, ns, name, id)`;
- `vector_ids(loom, ns, name, prefix)`;
- `vector_metadata_index_keys(loom, ns, name)`;
- `vector_create_metadata_index(loom, ns, name, key)`;
- `vector_drop_metadata_index(loom, ns, name, key)`;
- `vector_delete(loom, ns, name, id)`;
- `vector_search(loom, ns, name, query, k, filter)`;
- `vector_source_text(loom, ns, name, id)`;
- `vector_embedding_model(loom, ns, name)`;
- `vector_upsert_with_source(loom, ns, name, id, vector, metadata, source_text, model)`;
- `vector_upsert_text(loom, ns, name, id, source_text, metadata, embeddings)`;
- `vector_search_with_policy(loom, ns, name, query, k, filter, accel, policy, ef)`.

Vector ids are caller-supplied strings. A vector set has an immutable dimension and metric. Supported
metrics are cosine, L2, and dot. `upsert` and `search` reject wrong-width vectors or queries with
`DIMENSION_MISMATCH`. Metadata is a sorted map from string keys to `tabular::Value` values.

Exact search is source-backed. It evaluates the metadata filter before scoring, scores with the set's
metric, returns higher scores first, and breaks score ties by ascending vector id. The current filter
model supports `All`, `Eq`, `Ne`, `Lt`, `Le`, `Gt`, `Ge`, `In`, `Exists`, `And`, `Or`, and `Not`.
Range comparisons are deterministic and type-strict: a range predicate compares only metadata values
with the same cell variant as the predicate value.

Metadata equality indexes are source-backed in the Rust core and projected through the CLI, C ABI,
IDL, C header, Node, Python, C++, iOS, JVM, Android, React Native, and WASM bindings. Declared keys
are stored in the vector manifest. For each indexed metadata key, the engine maintains versioned
marker files keyed by metadata value and vector id. `vector_search` uses those markers to narrow
equality, set-membership, and conjunction filters before loading entries, then validates the full
filter and scores through the exact contract. The marker-file shape is chosen over detached
prolly-root files because the current vector storage is ordinary workspace content; every index marker
remains reachable through commit, clone, sync, and GC.

The local public facade is source-backed across the IDL, C ABI, C header, CLI, local MCP, and local
bindings. Hosted REST and JSON-RPC expose the native collection create, upsert, get, and search subset
over the same hosted auth/PEP path used by the other promoted data facets. Vector-entry structural
diffs are source-backed through the workspace VCS diff envelope. Same-id vector-entry merge is
source-backed through the shared VCS merge/replay seam. Hosted vector compatibility JSON filter
translation, deterministic Qdrant/Pinecone presentation mapping, and Qdrant REST compatibility are
source-backed. Qdrant REST currently covers collection create/info, point upsert/get/delete,
search/query, scroll, count, payloads, and JSON filters over profile-scoped vector-set paths. Qdrant
gRPC currently covers the matching unary collection and point subset, protobuf-shaped filters, and
vendor-style API-key metadata over the same profile-scoped vector-set paths. Pinecone REST currently
covers index-specific describe stats, vector upsert/fetch/query/delete/list, workspaces, metadata
filters, exact-only capability reporting, and stable unsupported integrated-embedding/sparse-vector
errors over profile-scoped vector-set paths. The hosted protocol feature matrix records the current
Qdrant/Pinecone supported, unsupported, and target compatibility rows from source-backed evidence.
Hosted native vector delete, id listing, metadata index
management, source/model routes, native Loom vector gRPC, broad hosted accelerator policy projection,
and external-client compatibility transcripts are not source-backed yet.

Source-text storage is source-backed in the Rust core, CLI, local MCP, C ABI, C header, IDL, Node,
Python, C++, Swift, JVM, Android, React Native, and WASM. `vector_upsert_with_source` stores an
already-computed vector plus UTF-8 source text. `vector_upsert_text` embeds source text through the
0050 provider seam and stores the resulting vector plus source text for Rust callers. Raw vector
upsert clears old source text for that id so source-aware reads never claim stale recompute input.
The C ABI and dynamic local bindings expose the embedding model profile as canonical CBOR
`[1, model_id, dimension, weights_digest]`. Product CLI commands now resolve a named 0062 text
embedding instance, call `vector_upsert_text`, embed query text, and return source-text hits.

The current vector facet has one storage type: caller-supplied fixed-width `f32` vectors with
metadata. Source text is a sidecar helper for recompute and provenance. Images, audio, video, PDFs,
and other documents are not distinct vector storage types today. 0068 owns the non-text source
pipelines that turn media into one or more vectors, source records, chunks, and metadata before those
vectors enter this facet.

## 2. Current Storage Shape

The vector facet root is:

```text
/.loom/facets/vector/<name>
```

Current source stores each vector set as a directory:

```text
/.loom/facets/vector/<name>/.manifest
/.loom/facets/vector/<name>/.embedding
/.loom/facets/vector/<name>/entries/<hex(cbor_text_id)>
/.loom/facets/vector/<name>/sources/<hex(cbor_text_id)>
/.loom/facets/vector/<name>/indexes/<hex(cbor_text_key)>/<hex(cbor_cell_value)>/<hex(cbor_text_id)>
```

The manifest is `[2, dim, metric, metadata_index_keys]`. Each entry file is `[vector_bits,
metadata]`, keyed by the hex encoding of the Loom Canonical CBOR text form of the vector id.
Metadata index markers are empty files under the indexed key and encoded metadata value. This keeps
arbitrary string ids and metadata keys, including values containing `/`, out of the path grammar while
preserving deterministic path order. Create, upsert, get, id listing, delete, and indexed filtered
search use this structured shape directly. Full-set loads remain available for accelerator builds and
compatibility helpers.

Source text is stored as UTF-8 bytes under `sources/<hex(cbor_text_id)>`. The optional `.embedding`
profile is `[1, model_id, dimension, weights_digest]`, with an empty weights digest string when the
provider does not report one. This profile is detect-and-warn metadata for source-aware writes; it is
not a secret store and does not include provider endpoint or token configuration.

Workspace commit, branch, checkout, bundle sync, and clone see the vector set as ordinary committed
content under the vector facet. The current committed vector set contains source-of-truth vector
entries, versioned source text, optional embedding model metadata, and versioned metadata-index
markers. It does not contain derived accelerator roots or a dedicated vector object type.

## 3. Current Encoding

The manifest is the Loom Canonical CBOR array `[2, dim, metric, metadata_index_keys]`. Each entry
file is the Loom Canonical CBOR array `[vector_bits, metadata]`.

The local vector filter CBOR contract is recursive: `[0]` all, `[1,key,value]` equality, `[2,a,b]`
AND, `[3,a,b]` OR, `[4,a]` NOT, `[5,key]` exists, `[6,key,value]` not-equal, `[7,key,value]` less-than,
`[8,key,value]` less-or-equal, `[9,key,value]` greater-than, `[10,key,value]` greater-or-equal, and
`[11,key,[values...]]` set membership. Empty filter bytes mean `All`.

`vector_bits` is the little-endian IEEE-754 bit pattern of each `f32` component. Metadata values use
the tabular cell-value codec. Metadata index keys are stored as a canonical CBOR text array. Metadata
index value path segments are the hex encoding of the canonical tabular cell value. Source text is
UTF-8 file content. The embedding model profile is canonical CBOR. Current source does not encode
accelerator configuration, quantization state, or query policy.

## 4. Current Accelerator Behavior

`loom-vector` implements derived accelerator helpers:

- `VectorAccelerator`, the trait used by accelerators;
- `AcceleratorPolicy`, with `ExactAlways` and `ApproximateAbove { threshold }`;
- `DEFAULT_EXACT_THRESHOLD`;
- `search_with_policy`, which makes the exact-vs-approximate choice explicit;
- `search_auto`, which uses exact search unless an accelerator is provided and the set size exceeds
  the threshold, preserved as a compact internal helper over `AcceleratorPolicy::ApproximateAbove`;
- `PqIndex`, a pure-Rust product-quantization accelerator that stores derived copies of ids,
  vectors, metadata, and PQ codes;
- `prune_csr`, a deterministic high-degree-preserving CSR pruning helper.

`loom-core::vindex` re-exports the `loom-vector` accelerator contracts for existing core callers.

`loom-hnsw` implements a native `HnswIndex` that satisfies `VectorAccelerator`. It builds a derived
HNSW graph from a `VectorSet`, applies the same metadata filter, and rescans returned candidates with
the exact metric before sorting. The HNSW graph is approximate and rebuildable. Returned hits carry
exact scores in deterministic order, but candidate recall can differ from exact search. It is not
committed, synced, or part of canonical identity.

The implemented accelerator policy is source-backed for Rust callers, the CLI, local MCP, C ABI, IDL,
C header, and all eight local bindings. Rust callers can inject any `VectorAccelerator`; CLI, MCP, and
binding callers can choose exact search or built-in PQ search above an explicit threshold. Non-MCP
hosted protocols do not project accelerator policy yet.

## 5. Current Versioning and Merge Behavior

Current vector sets version with the workspace because they are written into the workspace working
tree. A commit snapshots the vector-set manifest and per-id entry files with every other staged
workspace path. `checkout_commit` and `checkout_branch` restore those paths with the rest of the
workspace tree.

Disjoint vector ids merge through ordinary path-level VCS merge because each id has a separate entry
file. Same-id vector entries use a semantic 3-way merge in the shared VCS merge/replay seam. Vector
components merge bit-exactly: if one side leaves the vector bytes unchanged from the base, the other
side's vector bytes are taken; if both sides change the vector bytes differently, the entry conflicts.
Metadata merges per key: unchanged-vs-changed is accepted, disjoint key edits are accepted, and
same-key divergent edits conflict. Delete-vs-edit remains a normal same-path conflict. Sync follows
`CONFLICT-RESOLUTION-MATRIX.md`: branch/ref divergence uses the S1 fast-forward boundary, and same-id
semantic merge supplies the local S2 merge helper for vector entries.

Derived accelerators are never versioned or synced. After checkout, clone, or sync, an accelerator
must be rebuilt or bypassed with exact search.

Embedded derived-artifact store (partial source-backed). The shared derived-artifact lifecycle -
keys, source-anchor stamp (`source_digest`/`engine_version`/`format_version`), states, rebuild
coalescing, stale-before-trust, and serve-read policy - is defined canonically in 0005 §8.2 and
implemented by `loom-store::derived`; this section does not restate it. Vector-specific binding: the
HNSW/PQ accelerator MAY persist its materialization as a derived artifact in that store rather than
rebuilding in memory each process, while its declaration (metric, dimension, metadata-index keys)
stays canonical in the vector manifest (the config-vs-materialization split of 0005 §8.2).
`FileStore` exposes typed PQ and HNSW artifact keys, source-digest stamps, status reads, rebuild
begin/finish/fail transitions, unsupported markers, and the shared serving policy projection. Concrete
accelerator rebuild scheduling remains target work; when multiple builders are promoted they need a
named builder lease rather than an ad-hoc per-process guard (see 0005 §8.2 rebuild coalescing and the
`FileStore` single-writer guard in 0036 §8).

Accelerator selection policy. The Rust API exposes `AcceleratorPolicy::ExactAlways` and
`AcceleratorPolicy::ApproximateAbove { threshold }`, with a default threshold of
`DEFAULT_EXACT_THRESHOLD`. `vector_search_with_policy` and `search_with_policy` make approximate
search an explicit choice for Rust callers. `vector_search_with_pq_policy` projects the same
exact-or-approximate threshold choice through the built-in PQ accelerator for local non-Rust
surfaces. `search_auto` remains as a compatibility helper over `ApproximateAbove`.
`ApproximateWithRecallTarget(r)` remains target work because it needs measured accelerator
conformance and fallback rules.

## 6. Current Conformance

`loom-vector`, `loom-core::vector`, and `loom-hnsw` have unit tests for:

- dimension mismatch rejection;
- deterministic exact top-k order;
- metadata pre-filter behavior;
- commit and checkout versioning;
- structured manifest-plus-entry storage;
- maintained metadata equality indexes;
- vector-entry structural diff;
- same-id vector-entry merge;
- explicit accelerator policy;
- source-text storage and provider-backed text upsert;
- threshold dispatch to exact or accelerator search;
- PQ exact rescoring and metadata filtering;
- CSR pruning;
- HNSW exact rescoring and metadata filtering.

`loom-conformance` contains vector behavior scenarios and an executable vector facade runner. The
runner exercises create, upsert, get, id listing with string-prefix filtering, delete, exact
nearest-neighbor search, dimension mismatch, commit/checkout versioning, and clone reachability
against the in-memory store. Binding runtime suites are tracked separately from the executable core
conformance gate. Hosted REST and JSON-RPC protocol conformance covers the bounded native create,
upsert, get, and search route set.

## 7. Target Contract

The current public vector facade provides create, upsert, get, id listing with optional string-prefix
filtering, delete, exact top-k search, and metadata pre-filtered search. Target additions are:

- get dimension and metric;
- hosted projection for accelerator policy;
- recall-target accelerator policy;
- hosted vector compatibility runtime behavior for admitted profiles: `qdrant`, `pinecone`,
  `generic`, and later approved profiles, with no default compatibility profile;
- richer vector-set diff views beyond the VCS structural envelope;
- richer same-id vector merge reporting beyond path conflicts;
- concrete embedding provider runtimes.

Remaining promotion work for the hosted and enterprise boundary:

- remaining hosted protocol methods in 0008;
- generic served-transport runtime behavior for compatibility profiles beyond source-backed Qdrant
  REST, Qdrant unary gRPC, and Pinecone REST;
- client-backed conformance;
- access-control review for served vector writes;
- clear file-projection behavior for `/.loom/facets/vector/...`;
- vector-id collision behavior aligned with `CONFLICT-RESOLUTION-MATRIX.md`.

## 8. Storage Contract

Current source uses a structured vector value, not one whole vector-set blob. It uses existing object
types and deterministic encodings:

| Role | Target encoding | Status |
| --- | --- | --- |
| Vector map | Per-id entry files keyed by encoded vector id | Source-backed |
| Metadata indexes | Optional marker files keyed by encoded metadata key, value, and vector id | Source-backed; local MCP and local bindings projected |
| Source text map | Per-id UTF-8 files keyed by encoded vector id | Source-backed in core, CLI, local MCP, C ABI, C header, IDL, Node, Python, C++, Swift, JVM, Android, React Native, and WASM |
| Embedding model profile | `[1, model_id, dimension, weights_digest]` | Source-backed in core, CLI, local MCP, C ABI, C header, IDL, Node, Python, C++, Swift, JVM, Android, React Native, and WASM |
| Vector set root | Directory tree with manifest, entries, and optional metadata index markers | Source-backed |
| Accelerator index | Derived rebuildable side artifact | Store foundation source-backed; concrete accelerator persistence target, not identity |

The exact vector-id path encoding, metadata value model, and canonical `f32` byte rules are
source-backed by the current implementation. Metadata index projection through non-MCP hosted surfaces,
source-text projection through non-MCP hosted surfaces, hosted accelerator policy projection, concrete
embedding provider runtimes, recall-target accelerator policy, and quantized-byte persistence rules
remain target work. If those choices change canonical bytes, update conformance vectors with the
implementation.

## 9. Query and Engine Guidance

- Exact search is the portable baseline.
- Accelerator search must either return exact-equivalent results or clearly advertise approximate
  recall behavior at the public boundary.
- A derived accelerator must be rebuildable from the committed vector source of truth.
- Accelerator state is not committed, synced, or used as canonical identity.
- Recompute-by-default embeddings remain target work until concrete embedding providers and derived
  local index rebuild policy are implemented.

## 10. Relationship to Other Facets

- **Graph:** graph nodes or edges may reference vector ids, but vector storage and search remain in the
  vector facet.
- **Document:** document chunks may reference vector ids, but document indexing remains separate until
  0020 promotes the document facade.
- **Compute:** `loom-compute` has a `Vector` capability tag, but vector state access from programs is
  target work until 0015 defines and implements it.
- **Sync:** vector source data syncs as workspace content. Derived accelerators rebuild on the
  receiver.

## 11. Non-Goals and Limits

- Current source is not a full vector database service.
- Current source does not provide a concrete embedding model runtime.
- Current source does not commit or sync accelerators.

## 12. Resolved Decisions

- **RD1 - Current storage.** Current source stores each named vector set as a manifest plus one entry
  file per vector id under the workspace vector facet. There is no legacy vector-set storage contract.
- **RD2 - Current identity.** Caller-supplied string vector ids are the current identity keys.
- **RD3 - Dimension and metric.** Dimension and metric are fixed for a vector set; mismatches return
  `DIMENSION_MISMATCH`.
- **RD4 - Exact baseline.** Deterministic exact search is the portable source-backed baseline.
- **RD5 - Metadata filtering.** Current source applies metadata predicates before scoring.
- **RD6 - Accelerator boundary.** PQ and HNSW indexes are derived and rebuildable; they are not
  committed or synced. Rust callers choose `ExactAlways` or `ApproximateAbove { threshold }`
  explicitly and can inject an accelerator. CLI, C ABI, IDL, C header, and local binding callers can
  choose exact search or built-in PQ search above an explicit threshold. Target: an accelerator MAY
  be cached in the shared embedded derived-artifact store inside the loom container
  (store foundation source-backed; concrete writer target; identity-excluded, copy-carried but
  sync-rebuilt, version/digest stamped; section 5), and `ApproximateWithRecallTarget(r)` may be added
  after measured accelerator conformance exists.
- **RD7 - Public facade status.** The workspace-scoped public `vector` facade (create, upsert, get,
  id listing, delete, exact filtered top-k search) is source-backed across the engine, the C ABI, the
  IDL, the C header, CLI, and all eight bindings, with an executable facade conformance suite.
- **RD8 - Merge boundary.** Disjoint ids merge through path-level VCS merge. Same-id vector entries
  merge bit-exact vector bytes and per-key metadata when edits are disjoint, and conflict when both
  sides change the same vector bytes or metadata key differently. Sync does not silently merge
  divergent branch histories.
- **RD9 - Metadata index storage.** Rust core metadata equality indexes are source-backed as ordinary
  versioned marker files under the vector set, not detached prolly roots. This keeps index candidates
  reachable through the current file-based vector storage. Local MCP projection of metadata index
  declaration is source-backed; non-MCP hosted projection remains target work.
- **RD10 - Source-text storage.** Source text is stored as per-id UTF-8 sidecar files under the vector
  set. Source-aware writes can record the 0050 embedding model profile. Raw vector writes clear source
  text for that id to avoid stale recompute claims.
- **RD11 - Source/model projection status.** Source-aware vector writes, source reads, and embedding
  model profile reads are projected through the CLI, local MCP, C ABI, C header, IDL, Node, Python,
  C++, Swift, JVM, Android, React Native, and WASM. C++, Swift, JVM, Android, and React Native have
  local smoke or build evidence. React Native connected-device execution remains binding-certification
  work.
- **RD12 - Compatibility profile boundary.** Qdrant, Pinecone, pgvector, Milvus, Weaviate, and similar
  vector database shapes are compatibility profiles over the base Loom vector set, not the canonical
  data model. Hosted vector compatibility uses generic `rest` or `grpc` transports plus an explicit
  `--profile`; there is no default compatibility profile. Exact search remains the default result
  contract, and approximate accelerator behavior is public only when explicitly selected and
  capability-reported.
- **RD13 - Predicate filter model.** The source-backed vector filter model includes equality,
  inequality, range, set-membership, existence, AND, OR, and NOT predicates over typed metadata cells.
  Exact search evaluates the full predicate before scoring. Metadata indexes are optimization hints
  only; equality and set-membership can narrow candidates, while every candidate is still checked
  against the full predicate before scores are returned.

## 13. Unfinished Work

| Order | Parent | Work item | Status | Exit criteria |
| --- | --- | --- | --- | --- |
| T14 | RD11 | C++ vector source/model ergonomic wrapper | Complete local | C++ wrapper exposes source-aware upsert, source read, and model-profile read over the C ABI; `just cpp` passed. |
| T15 | RD11 | Swift vector source/model ergonomic wrapper | Complete local | Swift wrapper exposes source-aware upsert, source read, and model-profile read over the C ABI; `swift test --disable-sandbox --scratch-path /private/tmp/loom-swift-scratch` passed with source/model runtime coverage. |
| T16 | RD11 | JVM vector source/model ergonomic wrapper | Complete local | JVM binding exposes source-aware upsert, source read, and model-profile read; `just jvm` passed with source/model runtime smoke coverage. |
| T17 | RD11 | Android vector source/model ergonomic wrapper | Complete local | Android/Kotlin binding exposes source-aware upsert, source read, and model-profile read through JNI; `just android` passed. |
| T18 | RD11 | React Native vector source/model ergonomic wrapper | Complete local | TurboModule exposes source-aware upsert, source read, and model-profile read on Android and iOS. `just react-native-android` passed with the checked-in Gradle wrapper and host Android test APK assembly. |
| T19 | T14-T18 | Binding-specific runtime tests | Complete local | C++, Swift, JVM, Android, and React Native have targeted source text, model profile, and stale-source clearing coverage in checked-in tests or host smoke tests. React Native connected-device execution remains separate from APK assembly. |
| T20 | T19 | Binding inventory and conformance evidence | Complete local | `loom-conformance` binding inventory records the promoted vector source/model surface for C++, Swift, JVM, Android, and React Native without claiming CI-gated binding execution. |
| T21 | T20 | Binding host-environment verification | Partial certification | Swift host tests passed locally. React Native Android test APK assembly passed through `just react-native-android`; connected-device instrumentation remains certification work. |
| T22 | RD6 | MCP vector data projection | Complete local | MCP tools expose create, upsert, source-aware upsert, get, source text, embedding model, ids, metadata-index keys, metadata-index create/drop, delete, exact search, and policy search with registered schemas and canonical CBOR payload tests. |
| T27 | T22 | Native hosted vector projection | Partial | REST and JSON-RPC hosted protocols expose create, upsert, get, and search with hosted auth/PEP, daemon-opened listener tests, and executable protocol conformance. Remaining work: delete, ids, metadata-index management, source/model routes, gRPC, broader method conformance, vector compatibility profiles, and capability reporting. |
| T28 | T22 | Non-MCP hosted accelerator policy projection | Target | REST, JSON-RPC, and gRPC hosted protocols expose explicit exact/PQ threshold policy or record why it remains local-only. |
| T23 | RD6 | Recall-target accelerator policy | Target | Measured accelerator conformance defines fallback rules before `ApproximateWithRecallTarget(r)` is public. |
| T24 | RD10 | Concrete embedding provider runtimes | Target | At least one non-test provider implements 0050 embedding generation with deterministic model identity reporting. |
| T25 | RD10 | Non-MCP hosted source/model projection | Target | REST, JSON-RPC, and gRPC hosted wire protocols expose source-aware writes, source reads, and model-profile reads with ACL coverage. |
| T29 | RD12 | Vector compatibility profile contract | Complete current profile | Served-vector admission uses generic transports plus explicit profiles; source-backed profile rows cover Qdrant, Pinecone, pgvector, Milvus, Weaviate, supported/unsupported behavior, auth, filters, accelerator policy, source text, and conformance inventory. |
| T30 | T29 | Qdrant and Pinecone served compatibility profiles | Complete current profile | Qdrant REST maps collections, primary and named vectors, points, payloads, JSON filters, search/query, scroll, count, deletes, exact-only capability reporting, and unsupported sparse/integrated-embedding request shapes to profile-scoped Loom vector sets through the hosted kernel and daemon profile handoff. Qdrant unary gRPC maps collection create/info, point upsert/get/delete, search/query, scroll, count, payloads, protobuf-shaped filters, and API-key metadata through the same hosted kernel and daemon profile handoff. Pinecone REST maps index-specific describe stats, upsert, fetch, query, delete, list, workspaces, metadata filters, API-key headers, exact-only capability reporting, and unsupported integrated-embedding/sparse-vector errors through the same hosted kernel and daemon profile handoff. Remaining target work: generated external-client transcripts, hosted provider-backed source/model routes, hosted approximate accelerator policy, and Milvus/Weaviate/pgvector listeners. |
| T31 | RD13 | Base vector predicate model v2 | Complete local | Rust core, CLI CBOR filter parser, C ABI CBOR filter parser, and IDL now support `All`, `Eq`, `Ne`, `Lt`, `Le`, `Gt`, `Ge`, `In`, `Exists`, `And`, `Or`, and `Not`; focused tests prove deterministic exact-search filtering. Hosted Qdrant/Pinecone translation remains T30/T32 scope. |
| T32 | T31 | Vendor filter translation and conformance | Complete current profile | Qdrant JSON filters, Pinecone JSON filters, and Qdrant protobuf-shaped filters translate into the source-backed predicate model with stable unsupported-filter errors. Qdrant REST, Qdrant unary gRPC, and Pinecone REST execution coverage prove the supported filter paths. The conformance report inventories supported, unsupported, and target vector compatibility rows; generated external-client transcripts remain target certification work. |
| T26 | Hidden | MCP SQL history projection gate drift | Complete local | Hidden drift found while running `just ci`: SQL history methods in the IDL lacked MCP tool projection. `sql_read_table_at`, `sql_index_scan_at`, and `sql_table_diff` are now projected with server output schemas and spec-table coverage; `just ci` passed. |
| T27 | T21 | Local promotion close record | Complete local | 0017 local source/model promotion is closed for CLI, C ABI, C header, IDL, Node, Python, C++, Swift, JVM, Android, React Native, and WASM. Remaining work is hosted/provider/recall/device-certification scope. |

## Change log

### 0.1.0

The vector facet's workspace-scoped public facade is now source-backed for set creation, vector
upsert/get/delete, id listing, structured per-id storage, vector-entry structural diff, same-id
vector-entry merge, explicit accelerator policy, and exact filtered top-k search across the engine,
C ABI (`loom_vector_*`), IDL `Vector` interface, C header, CLI, local MCP, and all eight local bindings.
Create/upsert/get/delete/search also cross the Node, Python, C++, iOS, JVM, Android, React Native,
and WASM bindings. Metadata equality indexes are source-backed, with non-MCP hosted projection still target
work. Source text, embedding model profile tracking, and provider-backed text upsert are
source-backed in core; source-aware vector writes, source reads, and embedding model profile reads
are projected through the CLI, local MCP, C ABI, C header, IDL, Node, Python, C++, Swift, JVM,
Android, React Native, and WASM. C++, Swift, JVM, Android, and React Native have local smoke or build evidence.
React Native connected-device execution remains binding-certification work.
Embeddings cross as little-endian `f32` bytes, metadata as a canonical CBOR `text -> cell` map, vector
ids as a CBOR text array, and search results as `[id, score_cell]`. The recursive metadata filter
(all / equality / and) crosses as canonical CBOR. An executable behavioral suite exercises the facade
against the in-memory store, so the `vector` capability is reported executable. The derived PQ and
HNSW accelerators remain rebuildable and are not part of the synced contract.
