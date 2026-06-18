# 0040 - Versioned GraphRAG

**Status:** Draft, target. **Version:** 0.1.0.
**Capability:** `graphrag`.

This spec defines GraphRAG over Loom: knowledge-graph construction, storage, and retrieval for LLM/agent
workloads. It is **not a new storage facet**. It is a capability that *composes* three existing facets -
graph (0016), vector (0017), and columnar/Parquet (0023) - over the versioned engine, plus a thin
`GraphRAG` facade and ingestion path. Its distinguishing properties come from being built on a
content-addressed, versioned store, which the current GraphRAG field (Microsoft GraphRAG, LazyGraphRAG,
temporal-graph RAG) does not have.

## 1. Shape

`graphrag` is a facade and ingestion layer, not storage:

- **Entities and relationships** are graph (0016) nodes and edges, keyed by stable caller ids.
- **Node embeddings are co-located** with node properties (RD-colocate) so a query runs structural
  traversal (the GraphBLAS-style sparse-matrix engine, 0016) and vector similarity (0017) in one
  cache-friendly pass, rather than treating vectors as a bolted-on secondary index.
- **Bulk ingestion is Parquet** (0023): entities/relationships/text-units/embeddings load via Parquet
  `COPY`-style import - the exact artifact shape Microsoft GraphRAG already emits - and the mounted
  `.loom` can expose those tables as `.parquet` for zero-friction push from a Python pipeline.
- **Community summaries and reports** are documents (0020) / columnar rows, versioned like everything
  else.

So GraphRAG state lives in bucket 1 (versioned workspace content) across those facets; `graphrag` adds
the API and the ingestion/maintenance logic.

## 2. What being versioned buys (the differentiators)

These are native here because the store is content-addressed and versioned; they are open problems
elsewhere.

- **Versioned.** The knowledge graph is workspace content: every ingestion is a commit, every state is
  addressable.
- **Auditable / reproducible retrieval.** A retrieval can be pinned to the exact commit of the KG it ran
  against, so an answer is replayable with full provenance - the goal SAT-Graph RAG reaches for
  ("deterministic, auditable retrieval with provenance chains"), free here from content addressing.
- **Temporal / time-travel.** Query the KG *as of* any commit; model bitemporal edges (valid-time vs
  commit-time). This is the temporal-graph-RAG frontier (TG-RAG); native here because history is the
  store.
- **Incremental.** When new documents are ingested, the commit diff (0003b) reports exactly which
  nodes/edges/communities changed, so only affected community summaries and embeddings are recomputed -
  incremental maintenance instead of Microsoft GraphRAG's full rebuild. The diff is the enabler.
- **Branchable.** Run a new extraction prompt or model on a branch, evaluate it, and merge only if it is
  better - experiment without polluting main.
- **Lazy.** Support LazyGraphRAG-style deferral of community summarization to query time (indexing cost
  approaching plain vector RAG) as well as eager precomputation; co-located vectors + sparse-matrix
  traversal make lazy local search cheap.

## 3. Facade (target IDL, illustrative)

```idl
interface GraphRAG {
    // Ingest a Microsoft-GraphRAG-shaped Parquet bundle (entities, relationships, text units,
    // embeddings) into the workspace KG; returns the ingestion commit.
    Digest ingest_parquet(LoomHandle handle, string workspace, string path);
    // Retrieval pinned to a commit (default HEAD) for reproducibility; returns ranked context.
    bytes retrieve(LoomHandle handle, string workspace, string query, RetrieveOptions opts);
    // Incremental maintenance: recompute only what the diff between two commits changed.
    void reindex_incremental(LoomHandle handle, string workspace, Digest from, Digest to);
}
```

`RetrieveOptions` selects mode (lazy local / global / DRIFT-style hybrid), an `as_of` commit, and
graph-vs-vector weighting.

## 4. Dependencies and gating

Depends on 0016 (graph engine + structured node/edge storage + commit diff), 0017 (vector, exact +
opt-in ANN), 0023 (columnar/Parquet), 0003b (diff contract), and the determinism spine (ANN/GPU results
are local-only, never an authoritative sync source). The embedding provider is 0050. It does not gate
those facets; it is built after them.

## 5. Resolved decisions

- **RD1 - Composition, not a facet.** `graphrag` composes graph/vector/columnar; it adds no new storage
  class.
- **RD2 - Co-located embeddings.** Node embeddings live with node properties for single-pass
  structural + similarity retrieval.
- **RD3 - Parquet ingestion is first-class.** The bulk path is Parquet (the Microsoft GraphRAG artifact
  shape); the mount exposes Parquet tables for push-ingestion.
- **RD4 - Versioned-native features.** Auditable/reproducible, temporal/time-travel, incremental (via
  0003b diff), branchable, and lazy modes are in scope precisely because the store is versioned.

## 6. Sources

- LazyGraphRAG (Microsoft Research): cost approaching vector RAG, query-time summarization.
- VersionRAG (arXiv 2510.08109): standard RAG and GraphRAG degrade on evolving/versioned documents.
- RAG Meets Temporal Graphs / TG-RAG (arXiv 2510.13590); SAT-Graph RAG (deterministic, auditable
  retrieval with provenance).
- FalkorDB (GraphBLAS sparse-matrix graph for GraphRAG); Microsoft GraphRAG (Parquet artifacts); Kuzu
  (Parquet bulk import, Cypher).
