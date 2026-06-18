# Announcing Uldren Loom: the first Versioned GraphRAG engine

> Mock announcement (non-normative). A marketing-voice preview of the capability the 0016/0017/0023/0040
> specs define. Not a conformance document.

Every GraphRAG system today builds a knowledge graph, answers a question, and forgets how it got there.
Rebuild the graph and yesterday's answer is gone, unauditable, unreproducible. Loom changes that.

Loom is a content-addressed, versioned store. Your knowledge graph is not a throwaway index sitting next
to the database - it *is* committed, versioned data. That one fact turns GraphRAG from a black box into
something you can audit, rewind, branch, and update incrementally.

## What makes it different

- **Versioned.** Your knowledge graph is committed state. Every ingestion is a commit; every state of
  the graph is addressable and permanent.
- **Auditable.** Pin any answer to the exact commit of the graph it was retrieved from. Reproduce it
  byte-for-byte, months later, with a full provenance chain. No more "the model said so, we think."
- **Temporal.** Ask the graph what it knew last Tuesday. Time-travel queries and valid-time edges are
  native, because history is the storage, not a bolt-on.
- **Incremental.** Ingest new documents and Loom diffs the graph to recompute only what changed - the
  affected entities, communities, and summaries. No more multi-hour full re-indexing on every update.
- **Branchable.** Try a new extraction model or prompt on a branch. Compare it to main. Merge it only if
  it is actually better. Experiment without risk.
- **Lazy.** Skip expensive up-front summarization entirely and resolve at query time, at indexing cost
  approaching plain vector RAG - or precompute eagerly. Your call, same engine.
- **Co-located.** Node embeddings live inside the graph, so a single query walks relationships and
  filters by vector similarity in one pass. Structure and semantics, together, fast.
- **Portable.** The same engine runs native (with an optional GraphBLAS accelerator) and in the browser
  via WebAssembly, with identical results.
- **Open by ingestion.** Push your entities, relationships, and embeddings as Parquet - the same format
  Microsoft GraphRAG already emits - straight from a Python pipeline. Mount the store and drop Parquet
  files in. Zero-copy Arrow back out to Pandas and PyTorch.

## Why it matters

Enterprises cannot ship answers they cannot audit, reproduce, or update affordably. A knowledge graph
that is versioned, diffable, and branchable is not a feature checklist - it is the difference between a
demo and a system of record. Loom is GraphRAG you can put your name on.

Auditable. Temporal. Incremental. Branchable. Versioned. Lazy. Co-located. Portable.

This is GraphRAG, finally built on something that remembers.
