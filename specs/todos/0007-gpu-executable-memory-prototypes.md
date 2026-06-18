# GPU executable memory prototypes

## Idea

Explore a ladder of Loom memory execution designs, from ordinary CPU-side retrieval through
GPU-resident retrieval snapshots and eventually model-integrated memory layers.

The motivating question: can Loom memory be compiled into something executable on GPU so the model
does not depend only on chat context or CPU-side RAG? The answer is yes in several progressively more
ambitious forms, but each form has different engineering cost, debuggability, model dependence, and
training requirements.

This document defines prototype tracks for each layer so the project can test them separately.

## Sources checked

- `0006-loom-memory-controller.md` defines the controller as the layer that classifies intent,
  resolves entities, plans retrieval, packs context, mediates tool calls, and writes durable memory
  back to Loom.
  Source: `specs/todos/0006-loom-memory-controller.md:1`.
- Candle is a Rust ML framework with GPU support. Its README shows changing a tensor device from
  `Device::Cpu` to `Device::new_cuda(0)` for GPU execution, and lists CUDA backend support.
  Source: `https://github.com/huggingface/candle`.
- Unsloth docs describe Unsloth as an open-source framework for running and training models locally,
  and Unsloth Studio as a no-code local UI for training, running, and exporting open models.
  Source: `https://unsloth.ai/docs`,
  `https://unsloth.ai/docs/new/studio`.
- Unsloth Studio docs state that Studio can run GGUF and safetensor models locally, train 500+
  models, auto-create datasets from files, monitor training, export to GGUF or safetensors, compare
  models, and work offline locally.
  Source: `https://unsloth.ai/docs/new/studio`.
- vLLM docs describe fast LLM serving with PagedAttention, continuous batching, prefix caching,
  CUDA/HIP graphs, optimized attention kernels, tool calling, OpenAI-compatible serving, multi-LoRA
  support, and embedding or retrieval models.
  Source: `https://docs.vllm.ai/`.
- FAISS GPU docs state that some useful indexes are implemented on GPU, GPU indexes can accept host
  or device pointers for `add()` and `search()`, CPU/GPU index conversion is supported, and GPU
  indexes can accelerate CPU-index-compatible workloads.
  Source: `https://github.com/facebookresearch/faiss/wiki/Faiss-on-the-GPU`.
- RAPIDS cuGraph docs describe cuGraph as graph algorithms integrated into the RAPIDS ecosystem, with
  support for cuDF/Pandas DataFrames or CuPy/SciPy sparse matrices, a NetworkX backend, and
  GPU-accelerated graph analytics.
  Source: `https://docs.rapids.ai/api/cugraph/stable/`.
- RETRO retrieves chunks from a large corpus and uses chunked cross-attention to condition generation
  on retrieved data.
  Source: `https://arxiv.org/abs/2112.04426`.
- Memorizing Transformers add approximate kNN lookup over a non-differentiable memory of key-value
  pairs from internal representations of past inputs.
  Source: `https://arxiv.org/abs/2203.08913`.
- kNN-LM interpolates a pretrained neural language model with a k-nearest-neighbor model over
  embedding-space nearest neighbors from a text collection.
  Source: `https://arxiv.org/abs/1911.00172`.
- LoRA freezes pretrained model weights and injects trainable low-rank matrices, reducing trainable
  parameters and GPU memory requirements for downstream adaptation.
  Source: `https://arxiv.org/abs/2106.09685`.
- QLoRA fine-tunes a frozen 4-bit quantized pretrained model through LoRA adapters and reports
  65B-parameter fine-tuning on a single 48 GB GPU.
  Source: `https://arxiv.org/abs/2305.14314`.

## Design ladder

The proposal should be tested as a ladder. Each rung should be useful on its own.

| Track | Name | Model changes | GPU use | Best for |
| --- | --- | --- | --- | --- |
| 0 | CPU memory controller | none | optional model inference only | correctness baseline |
| 1 | GPU vector snapshot | none | vector search | fast semantic recall |
| 2 | GPU graph snapshot | none | graph traversal and ranking | relationship-heavy recall |
| 3 | GPU hybrid snapshot | none | vector plus graph plus filters | ranked subgraph retrieval |
| 4 | KV cache snapshot | runtime support | attention cache reuse | stable project manuals and profiles |
| 5 | Retrieval cross-attention | model architecture change | memory keys and values inside forward pass | research memory layer |
| 6 | LoRA or adapter memory | adapter training | weights execute on GPU | behavior and extraction skill |

Track 0 is mandatory. Tracks 1 through 3 are practical engineering. Tracks 4 through 6 are research
or runtime-integration tracks.

## Track 0: CPU memory controller baseline

This is the control group.

Architecture:

```text
User turn
-> memory controller on CPU
-> Loom files, documents, vector, graph, search
-> evidence pack
-> model runtime
```

Prototype goal:

- Prove retrieval semantics before moving anything to GPU.
- Establish evaluation questions and expected answers.
- Record selected evidence and policy decisions.
- Define the memory snapshot schema used by later tracks.

Tools:

- Loom core.
- Candle, llama.cpp, vLLM, or hosted model for inference.
- Existing Loom vector or graph implementations.
- Simple JSON evidence pack.

Success criteria:

- Correct answer with cited evidence.
- Trace explains why each memory item was selected.
- Whole-source, graph-first, vector-first, and search-first routing work.
- Writeback proposals are policy-gated.

## Track 1: GPU vector snapshot

Compile Loom vector memory into a GPU-resident vector index.

Architecture:

```text
Loom commit
-> memory compiler
-> embeddings matrix + id table + metadata columns
-> GPU vector index
-> top-k memory IDs
-> CPU fetches source spans from Loom
-> evidence pack
```

Compiled artifact:

```text
memory-vector.snapshot/
  manifest.json
  vectors.f16
  ids.u64
  source_refs.arrow
  metadata.arrow
  faiss.index
```

Manifest fields:

- Loom commit digest.
- Workspace IDs.
- Embedding model.
- Dimension.
- Metric.
- vector compiler version.
- source record count.
- created time.

Prototype tools:

- FAISS GPU for first NVIDIA prototype.
- Candle tensors for small flat exact search in Rust.
- cuVS or RAFT later if RAPIDS integration becomes desirable.
- vLLM embedding models or Candle embedding models for query embeddings.

Questions to test:

- Can GPU top-k produce the same IDs as CPU exact search for small corpora?
- How much latency is saved for 100k, 1M, and 10M vectors?
- How expensive is CPU source fetch after GPU ID retrieval?
- Which metadata filters must live on GPU versus CPU?

Risks:

- GPU memory pressure.
- Index rebuild cost.
- Approximate search may violate Loom's exact-search contract unless reconciled.
- IDs returned by GPU are not enough without source spans and citations.

Recommended first prototype:

- Flat exact cosine or dot-product search over 10k to 100k vectors.
- Keep all IDs and metadata on CPU.
- Return top-k IDs and compare against CPU Loom vector search.
- Add FAISS GPU after the flat prototype is correct.

## Track 2: GPU graph snapshot

Compile Loom graph memory into GPU-friendly graph arrays.

Architecture:

```text
Loom graph workspace
-> node compaction
-> edge type dictionary
-> CSR adjacency arrays
-> GPU graph kernels
-> ranked paths or neighborhoods
-> CPU fetches evidence spans
```

Compiled artifact:

```text
memory-graph.snapshot/
  manifest.json
  node_ids.u64
  node_kinds.u16
  edge_indptr.u64
  edge_indices.u64
  edge_types.u16
  edge_scores.f32
  node_metadata.arrow
  edge_source_refs.arrow
```

Prototype tools:

- RAPIDS cuGraph for Python-first experiments.
- nx-cugraph for quick NetworkX-compatible experiments.
- Custom CUDA or Triton kernels later for relation-constrained traversal.
- CPU fallback with petgraph or existing Loom graph substrate.

Queries to prototype:

- Ego network around one entity.
- Relation-constrained expansion, such as `Idea -> EVOLVES_TO -> Decision`.
- Timeline extraction by event time.
- Top-k paths between two entities.
- Personalized PageRank-like ranking over a topic subgraph.
- Evidence-backed neighborhood retrieval.

Risks:

- Graph schemas drift.
- Not every graph query maps cleanly to GPU kernels.
- Dynamic updates can make snapshots stale.
- Multi-hop retrieval can overfetch without tight relation and time filters.

Recommended first prototype:

- Export accepted graph-ready annotations from `0002`, `0004`, or `0005` into a CSR graph.
- Run CPU and GPU degree, neighborhood, and path queries.
- Return only node and edge IDs, then fetch evidence from Loom on CPU.

## Track 3: GPU hybrid memory snapshot

Combine vector retrieval, graph traversal, temporal filters, and status filters in one snapshot.

Architecture:

```text
Query text
-> query embedding
-> GPU vector top-k seed nodes
-> GPU graph expansion around seeds
-> filters by relation, time, confidence, status
-> ranked memory subgraph
-> CPU source fetch
-> evidence pack
```

Compiled artifact:

```text
memory-hybrid.snapshot/
  manifest.json
  vectors.f16
  vector_to_node.u64
  graph_csr/
  metadata_columns.arrow
  source_refs.arrow
  ranking_config.json
```

Scoring model:

```text
score =
  semantic_similarity
  + graph_proximity_weight
  + relation_priority
  + recency_or_valid_time_weight
  + confidence_weight
  + accepted_status_boost
  - sensitivity_penalty
```

Prototype tools:

- FAISS GPU plus cuGraph for Python prototype.
- RAPIDS cuDF for metadata filters.
- Candle for Rust-only vector scoring prototype.
- Arrow for snapshot metadata.

Questions to test:

- Does hybrid retrieval beat vector-only retrieval for evolution and provenance questions?
- Does graph expansion recover important evidence missed by semantic top-k?
- How should weights be learned or configured?
- Can the controller explain the hybrid ranking?

Risks:

- Ranking becomes opaque.
- GPU integration can hide provenance if not designed carefully.
- Hybrid snapshots may be harder to update incrementally.

Recommended first prototype:

- Offline Python prototype with FAISS GPU and cuGraph.
- Fixed scoring formula.
- Compare against Track 0 evidence selections on known questions.

## Track 4: KV cache memory snapshots

Compile stable source text into model-native attention cache blocks.

Idea:

```text
Source memory text
-> tokenize
-> run prefill once
-> store layer-wise key and value tensors
-> later attach selected cache blocks during inference
```

This treats memory as precomputed transformer state rather than retrieved text.

Potential artifact:

```text
memory-kv.snapshot/
  manifest.json
  tokenizer.json
  model_id
  layer_count
  position_policy.json
  blocks/
    block_0001.k.f16
    block_0001.v.f16
  source_refs.arrow
```

Potential tools:

- vLLM as a reference for PagedAttention, prefix caching, KV cache management, and serving.
- llama.cpp KV cache internals for local GGUF experiments.
- Candle custom runtime for controlled Rust experiments.

Good fit:

- Stable project manuals.
- User profile.
- Routing files.
- Frequently used instructions.
- Canonical domain glossary.

Poor fit:

- Frequently changing memory.
- Large graph traversal.
- Facts requiring precise deletion or redaction.
- Cross-model portability.

Hard problems:

- KV cache is model-specific and layer-specific.
- Position encoding and cache splicing are tricky.
- Cached memory can be large.
- Deletion and redaction require invalidating derived cache blocks.
- The model may still attend to irrelevant cached content unless retrieval selects blocks carefully.

Recommended first prototype:

- Pick one model and runtime.
- Precompute KV for one stable routing document.
- Compare latency and answer quality against including the document as text.
- Do not use for arbitrary user memory until deletion, provenance, and selection are solved.

## Track 5: Retrieval cross-attention memory layer

Add an explicit memory layer to the model forward pass.

Concept:

```text
h_t = hidden state
q = Wq h_t
neighbors = top_k(q, memory_keys)
m = attention(q, memory_keys[neighbors], memory_values[neighbors])
h_t2 = h_t + gate(m)
```

Loom populates the memory bank:

- Source spans.
- Graph nodes.
- Graph edges.
- Accepted facts.
- Tasks.
- Decisions.
- Entity aliases.
- Summaries.

Compiled artifact:

```text
memory-layer.snapshot/
  manifest.json
  memory_keys.f16
  memory_values.f16
  memory_ids.u64
  relation_types.u16
  source_refs.arrow
  projection_weights.safetensors
```

Related research:

- RETRO uses retrieval plus chunked cross-attention over retrieved chunks.
- Memorizing Transformers use approximate kNN over key-value memories from internal representations.
- kNN-LM interpolates language model predictions with nearest-neighbor retrieval in embedding space.

Prototype tools:

- PyTorch first for architecture experiments.
- Triton kernels later for custom top-k and memory attention.
- Unsloth or Hugging Face training stack for fine-tuning adapters around the memory layer.
- Candle only after the architecture stabilizes.

Training requirement:

The base model will not automatically know how to use arbitrary injected memory vectors. This track
likely needs at least adapter tuning and possibly model architecture changes.

Evaluation:

- Does the model use memory without copying irrelevant facts?
- Can it cite the memory IDs that influenced output?
- Does memory injection improve answers over context-packed RAG?
- Can memory be updated without retraining?

Risks:

- Model-specific architecture work.
- Harder safety and provenance.
- Memory vectors can become uninterpretable.
- Training data quality dominates.

Recommended first prototype:

- Tiny model, tiny memory bank, synthetic tasks.
- Teach lookup of a fact table or graph relation through a memory layer.
- Require the model to output memory IDs used.

## Track 6: LoRA or adapter memory

Compile durable behavior or domain skill into adapter weights.

This is not ideal for exact mutable memory, but it is useful for:

- Extraction behavior.
- Domain-specific tagging.
- Tone and formatting.
- Tool-use discipline.
- Query-planning behavior.
- Memory-controller policy imitation.

Artifact:

```text
memory-adapter.snapshot/
  manifest.json
  base_model
  adapter.safetensors
  training_dataset.jsonl
  eval_report.json
  source_refs.arrow
```

Prototype tools:

- Unsloth Studio for local no-code training, data recipes, observability, and export.
- Unsloth Core for scriptable training.
- Hugging Face PEFT for LoRA baselines.
- QLoRA for memory-constrained fine-tuning.
- vLLM multi-LoRA for serving multiple adapters on one base model.
- Candle LoRA ecosystem for Rust-native experiments after the training recipe is known.

Good fit:

- "Extract guided-capture annotations exactly in our schema."
- "Plan Loom retrieval before answering."
- "Prefer source-backed claims."
- "Use finance tagging nomenclature."
- "Generate graph-ready relation proposals."

Poor fit:

- "Remember Sarah's new title."
- "Delete this private fact."
- "Cite the exact meeting span."
- "Use the latest project state."

Recommended first prototype:

- Train an adapter to emit the annotation schema from `specs/studio/MEETINGS.md`, `0004`, and
  `0005`.
- Compare base model versus adapter on extraction precision, schema validity, and source-span
  grounding.
- Do not train factual memories into the adapter.

## Snapshot compiler

All GPU and model-integrated tracks need a memory compiler.

Inputs:

- Loom commit digest.
- Workspace selectors.
- Extraction schema version.
- Embedding model.
- Relation schema.
- Sensitivity policy.
- Snapshot target: vector, graph, hybrid, KV, memory layer, or adapter.

Outputs:

- Snapshot directory.
- Manifest.
- Binary arrays.
- Metadata tables.
- Source reference table.
- Evaluation report.
- Rebuild instructions.

Manifest fields:

- `snapshot_id`.
- `loom_commit`.
- `workspaces`.
- `compiler_version`.
- `snapshot_kind`.
- `created_at`.
- `model_id` where model-specific.
- `tokenizer_id` where token-specific.
- `embedding_model`.
- `graph_schema_version`.
- `source_record_count`.
- `redaction_policy`.
- `hashes`.

Invariant:

The snapshot is derived. Loom remains the source of truth. If a source is deleted or redacted, every
dependent snapshot must be invalidated or rebuilt.

## Tooling map

| Tool | Role | Best tracks | Notes |
| --- | --- | --- | --- |
| Candle | Rust model inference and custom tensor prototypes | 0, 1, 4, later 5 | Good for Rust-native control and local inference |
| llama.cpp | GGUF inference and local KV experiments | 0, 4 | Useful local runtime; KV internals are model/runtime-specific |
| vLLM | High-throughput serving, PagedAttention, prefix caching, LoRA serving | 0, 4, 6 | Strong serving baseline and OpenAI-compatible interface |
| FAISS GPU | GPU vector search | 1, 3 | Fast way to test GPU vector snapshots |
| RAPIDS cuGraph | GPU graph algorithms | 2, 3 | Good Python-first path for graph retrieval prototypes |
| cuDF / Arrow | GPU and columnar metadata | 2, 3 | Useful for metadata filters and snapshot tables |
| PyTorch | Research model architecture work | 5, 6 | Best first environment for memory-layer experiments |
| Triton | Custom GPU kernels | 3, 5 | Use after Python prototypes reveal bottlenecks |
| Unsloth Studio | Local no-code training, datasets, export, model comparison | 6 | Useful for adapter and extractor training prototypes |
| Unsloth Core | Scripted efficient fine-tuning | 6 | Better for reproducible training pipelines |
| Hugging Face PEFT | LoRA baseline | 6 | Standard ecosystem baseline |
| MLX | Apple Silicon training or inference path | 6 later | Track once Unsloth Studio MLX training lands |

## Prototype order

1. CPU baseline from `0006`.
2. GPU vector snapshot.
3. GPU graph snapshot.
4. GPU hybrid snapshot.
5. Adapter-trained extractor.
6. KV cache snapshot.
7. Retrieval cross-attention memory layer.

Reasoning:

- Tracks 1 through 3 do not require model changes.
- Track 6 teaches useful behavior but should not store mutable facts.
- Track 4 is runtime-specific and should wait until retrieval semantics are stable.
- Track 5 is research-heavy and should be last.

## Evaluation plan

Use the same question set across tracks:

- Exact meeting summary.
- Fuzzy idea recall.
- First-seen timeline.
- Decision provenance.
- Task ownership.
- Contradiction lookup.
- Relationship path.
- Sensitive-data denial.
- Redaction invalidation.

Metrics:

- Answer accuracy.
- Evidence recall.
- Evidence precision.
- Citation correctness.
- Latency.
- GPU memory.
- Snapshot build time.
- Snapshot size.
- Rebuild cost after one source change.
- Deletion and redaction correctness.
- Cross-model portability.

## Uldren Desktop prototype UX

The Desktop app should expose these tracks as experimental memory engines:

- `CPU controller`.
- `GPU vector`.
- `GPU graph`.
- `GPU hybrid`.
- `KV cache`.
- `Adapter`.
- `Memory layer`.

For each engine, show:

- Snapshot source commit.
- Build status.
- GPU memory estimate.
- Query latency.
- Evidence selected.
- Differences from CPU baseline.
- Rebuild required warnings.
- Redaction invalidation warnings.

The user should be able to run the same query across engines and compare answers, evidence, and cost.

## Open questions

- Should the first GPU snapshot be implemented in Rust with Candle tensors or Python with FAISS GPU?
- Should Loom's vector exactness guarantee require GPU approximate results to be reconciled on CPU?
- What is the smallest graph query set worth compiling to GPU?
- Should source text ever live on GPU, or only IDs, embeddings, graph arrays, and metadata?
- How should snapshots be encrypted when source Loom files are encrypted?
- How should a redaction propagate to derived GPU snapshots and adapter training datasets?
- Can KV cache snapshots be made portable enough to justify first-class support?
- Which model family should be used for the memory-layer research prototype?
- Should Unsloth Studio be used only for adapter training, or also for dataset generation from Loom
  exports?
