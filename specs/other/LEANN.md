# LEANN — technical analysis & uldren-loom planning

**Purpose:** understand how LEANN achieves its storage/performance results, judge what is portable
to a pure-Rust, content-addressed, versioned, `wasm32`-capable store, and turn its feature set into a
prioritized plan for uldren-loom's vector facet (0017) and adjacent facets.

**Sources**

- The cloned repo at `/Users/nxkavian/Drive/Source/InFlow/LEANN` (read directly — `README.md`,
  `packages/leann-core/src/leann/`, the three backends, `apps/`, `packages/leann-mcp`, `docs/`).
- The paper *LEANN: A Low-Storage Vector Index*, **arXiv:2506.08276v2**.
- DeepWiki overview (`https://deepwiki.com/yichuan-w/LEANN`), used only to corroborate.

**Source caveats (kept honest):** the arXiv HTML fetch was truncated at the start of §7's results
table, so Section 7.2's full main-results tables and appendices were not read directly; Table 1 (the
motivating numbers) and Sections 1–6 (all algorithms) were. The repo's HNSW backend exposes the
graph-prune + recompute path in Python but the modified-FAISS C++ (where the PQ two-level search
lives) was not read, so a couple of points below are marked *paper-asserted, repo-plausible*. Python
floor is 3.10+.

---

## 1. What LEANN is

LEANN ("Low-storage Embedding-based ANN") is a local-first vector index + RAG toolkit that achieves
up to a ~97% storage reduction (paper: "up to 50×") versus a conventional vector database **at no
downstream-accuracy loss**, by **not storing the dense embeddings at all** and instead **recomputing
them on demand at query time** with the same encoder used at build. It is built on a vendored,
modified FAISS; it stores only a *high-degree-preserving pruned proximity graph* (CSR format) plus a
small PQ table plus the raw passages, and uses a *two-level (hybrid-distance) search with dynamic
batching* and a persistent embedding-recompute server to keep latency acceptable. Around that core it
ships a declarative Python API (`LeannBuilder`/`LeannSearcher`/`LeannChat`), a CLI
(`leann build/search/ask/watch/list/remove`), an MCP server for Claude Code, a ReAct agent,
multi-provider embedding/LLM backends, and ~15 ready-made RAG apps.

---

## 2. How the gains are achieved (the core)

The storage win is structural: **eliminate the largest cost (stored embeddings) and shrink the
second-largest (graph metadata).** The paper's motivating numbers (§1, Table 1 — 76 GB RedPajama-Wiki
text, Natural Questions, Qwen3-4B generator, RTX 4090):

| Metric | BM25 | HNSW | PQ | **LEANN** |
|---|---|---|---|---|
| Downstream accuracy (%) | 18.3 | 25.5 | 17.9 | **25.5** |
| Storage (GB) | 59 | 188 | 20 | **4** |
| — index metadata | – | 15 | 15 | **2** |
| — vectors | – | 173 | 5 | **2** |
| End-to-end latency (s) | 21.36 | 20.95 | 25.45 | **23.34** |
| — search | 0.03 | 0.05 | 4.53 | **2.48** |
| — generation | 21.33 | 20.90 | 20.92 | 20.86 |

The framing argument: in RAG, **generation dominates end-to-end latency** (~20.9 s vs. ms–s for
search), so trading a couple of extra seconds of search for a ~47× storage cut (188 → 4 GB) at
identical accuracy is a good deal. README per-corpus figures: 60M Wiki chunks 6 GB vs 201 GB (97%);
DPR 2.1M 324 MB vs 3.8 GB (91%); chat 400K 64 MB vs 1.8 GB (97%); email 780K 79 MB vs 2.4 GB (97%).

### The five techniques (and Rust portability)

**(a) Don't store embeddings; store the pruned graph + PQ table + raw text.**
At build, FAISS makes a normal `IndexHNSWFlat`; LEANN then physically **strips the vector storage
section** from the serialized file (`convert_to_csr.py`: `prune_hnsw_embeddings_inplace`, writes a
`NULL` storage FourCC) leaving graph-only. What remains: CSR adjacency lists (`O(N·|D|)` u32
neighbor ids), a PQ codebook (~100× smaller than FP32), and outside the index a `passages.jsonl`
(text + metadata) with a pickled `{id: offset}` map, plus `meta.json` and `ids.txt`.
*Portable to Rust? **Yes.*** Plain format work: CSR in `Vec<u32>` + offsets; a passage store (for us,
the content-addressed blob store); a PQ codebook (k-means + asymmetric distance — straightforward).
No FAISS needed to *store* the graph; we define our own format.

**(b) Selective recomputation during traversal.**
Best-first search on a proximity graph touches only `O(log N)` nodes, so only *visited* nodes need
embeddings. LEANN recomputes those on the fly: the C++ search, when it needs a node's exact distance,
calls a **ZMQ embedding server** that maps node→passage id, seeks the text, embeds it, and returns
L2/MIPS distances. Fast enough because: only visited nodes; the two-level search restricts exact
recompute further; dynamic batching; and the model server is persistent/warm.
*Portable to Rust? **With work** — this is the biggest lift.* Needs (1) an **in-process embedding
runtime** (`candle` pure-Rust, or `ort`/ONNX, or `ggml` bindings) and (2) a recompute scheduler that
collects visited node ids, fetches text from the store, embeds, returns distances. In Rust the ZMQ
server, daemon, registry, and file locks all **disappear** — replace with a resident `trait Embedder`
behind an `Arc`. The hard constraint is **wasm32**: running a 100M-param encoder in the browser is
feasible (candle/ort-wasm) but slow; a small encoder (MiniLM-class) or a host-provided embedder is
the realistic browser path.

**(c) Two-level search with hybrid distance (paper §4.1, Algorithm 2).**
Pure "PQ-search then re-rank" fails at LEANN's compression (PQ error misroutes traversal). Instead
LEANN keeps an **exact queue** (drives traversal on recomputed exact distances) and an **approximate
queue** (all explored nodes, cheap PQ distance); each step it recomputes exactly only the **top-α%**
of the approximate frontier. Repo knobs: `prune_ratio`/`pruning_strategy ∈ {global,local,proportional}`.
*Portable to Rust? **Yes** — pure algorithm: two heaps + a PQ distance + top-α selection. The most
important part to port for recall/latency.*

**(d) Dynamic batching (paper §4.2).**
Relaxes the strict best-first dependency to accumulate candidates across steps until a batch size
(~64), then recomputes them together for GPU efficiency. Knob: `batch_size`.
*Portable to Rust? **Yes**; partly free (candle/ort batch inference). Less critical on CPU/wasm but
still amortizes tokenizer/forward overhead.*

**(e) High-degree-preserving graph pruning (paper §5, Algorithm 3).**
Node degree/visit frequency is highly skewed; a few "hub" nodes carry navigability. LEANN keeps the
**top β% (~2%) highest-degree nodes** at full degree M and caps the rest at m = M/5, while still
allowing bidirectional links up to M to newly inserted nodes — preserving recall at a fraction of the
metadata. Solved as a storage-budget-constrained minimization of total recomputations.
*Portable to Rust? **Yes** — degree bookkeeping over our adjacency lists. Needs an HNSW *builder*
first (reuse `hnswlib-rs`/`instant-distance`/`hora`, or reimplement) then apply the prune pass.*

Also relevant: **storage-constrained sharded build** (paper §6 — k-means → 2-nearest-centroid assign
→ per-shard build → merge; bounds *peak* build storage) and **incremental update + soft delete**
(paper §6/App. B — cheap insert, batched adds, mark-inactive deletes). Both *portable with work*; both
deferrable for a v1 that builds in memory.

---

## 3. Backend deep-dive (HNSW vs DiskANN vs IVF, as LEANN uses them)

| | **HNSW** (default) | **DiskANN** | **IVF** |
|---|---|---|---|
| Underlying | modified FAISS `IndexHNSWFlat` + CSR + `set_zmq_port` | MS DiskANN (Vamana) via native `diskannpy` + PQ | FAISS `IndexIVFFlat` + `DirectMap.Hashtable` |
| For | most datasets, **max storage savings** | larger-than-memory / scale; PQ traversal + rerank | in-place **incremental add/remove** |
| Storage | smallest (CSR graph + PQ; embeddings stripped) | PQ-compressed, disk-resident, partitioned | **largest** — stores full FP32 vectors |
| Carries the recompute trick? | **Yes (primary)** | **Yes** (`is_recompute=True` → partition + own recompute server) | **No** — searches stored vectors; only embeds the *query* |
| Incremental | add-only (non-compact) | none (rebuild) | full add/remove |

GPU variants `flashlib`/`flashlib_ivf` exist (CUDA-only). **Takeaway:** the storage story is the
HNSW recompute path; IVF is for incremental updates; DiskANN is the scale path and the heaviest
native dependency.

---

## 4. Categorized feature list (LEANN's own wording)

**Data sources ("RAG on Everything!")** — documents (`.pdf/.txt/.md`, `apps.document_rag`),
multimodal PDF (ColQwen/ColPali, `apps.colqwen_rag`), Apple Mail (`email_rag`), Chrome history
(`browser_rag`), WeChat (`wechat_rag`), iMessage (`imessage_rag`), ChatGPT/Claude exports
(`chatgpt_rag`/`claude_rag`), live data via MCP — Slack (`slack_rag`), Twitter bookmarks
(`twitter_rag`); plus code (`code_rag`, AST-aware). CLI: `index-{browser,email,calendar,imessage,wechat,chatgpt,claude}`.

**Multi-provider support** — embeddings: `sentence-transformers` (default `facebook/contriever`),
`openai`, `ollama`, `mlx` (Apple), `gemini`; OpenAI-compatible base-URL covers LM Studio/vLLM/llama.cpp/
SGLang/LiteLLM/OpenRouter/Groq/DeepSeek/Jina. LLMs: Ollama, HF, OpenAI, Anthropic, Gemini, MiniMax,
Novita, Simulated.

**Backends** — HNSW (default), DiskANN, IVF, + GPU FlashLib.

**Search/RAG** — metadata filtering (`== != < <= > >= in not_in contains starts_with ends_with
is_true is_false`), grep search (`use_grep`), hybrid dense+BM25 (`vector_weight`, SQLite FTS5),
two-level search, dynamic batching, high-degree-preserving pruning; params `top_k`, `complexity/efSearch`,
`beam_width`, `prune_ratio`, `pruning_strategy`, `batch_size`, `recompute/no-recompute`.

**Incremental/sync** — `FileSynchronizer` (Merkle/SHA-256), `leann watch`, idempotent `leann build`.

**Agent/MCP** — MCP server (`leann_list`, `leann_search`; AST chunking; zero-config; `leann_mcp`
shells the CLI); ReAct agent (`leann_search`, `web_search` via Serper, `visit_page` via Jina).

**CLI** — `build, search, ask, watch, list, remove, migrate, rebuild, warmup, daemon{start,stop,status},
react, serve, index-*`.

**Use cases** — personal knowledge management, code search for AI assistants (MCP), private local
RAG, large-scale doc search (60M on a laptop), cross-source retrieval; vision: a unified personal
knowledge layer with always-on indexing and agent memory.

---

## 5. Examples breakdown (to model later)

**Quick example: CLI** (README "Quick Start" / "Usage Examples") — the build→search→ask pipeline:
```bash
leann build my-docs --docs ./your_documents       # idempotent / incremental
leann search my-docs "machine learning concepts"   # semantic search
leann ask my-docs --interactive                     # chat
leann watch my-docs ; leann list ; leann remove my-docs
```
Python: `LeannBuilder(backend_name="hnsw").add_text(...).build_index(path)` →
`LeannSearcher(path).search(q, top_k)` → `LeannChat(path, llm_config={...}).ask(q)`.
*Capability exercised:* end-to-end RAG with on-demand recompute; default HNSW, default encoder
`facebook/contriever`, `--graph-degree 32`, `--complexity 64`, `--recompute/--no-recompute`,
`--compact/--no-compact`.

**1. Multiple data-source support** (README "RAG on Everything!" / "Personal Data Manager"):
```bash
python -m apps.document_rag --data-dir "~/Documents/Papers" --chunk-size 1024
python -m apps.document_rag --data-dir "./docs" --file-types .md .py
python -m apps.document_rag --enable-code-chunking --data-dir "./my_project"
python -m apps.code_rag --repo-dir "./my_codebase" --query "How does authentication work?"
```
*Capability:* pluggable readers → chunking (incl. AST) → build → query, shared flags. Inline storage
exemplars: email 780K→78 MB, browser 38K→6 MB, WeChat 400K→64 MB.

**4. Multi-provider support** (README "Supported LLM & Embedding Providers via OpenAI compatibility"):
```bash
export OPENAI_BASE_URL="http://localhost:1234/v1"     # LM Studio; :11434/v1 Ollama; :8000/v1 vLLM
leann build my-index --docs ./docs \
  --embedding-mode openai --embedding-model jina-embeddings-v3 \
  --embedding-api-base https://api.jina.ai/v1 --embedding-api-key $JINA_API_KEY
```
*Capability:* swap embedder/LLM by flag/env; `--thinking-budget low/medium/high` for reasoning models.

**3. Backend layer** (README "Architecture & How It Works → Backends"): HNSW "(default): maximum
storage savings through full recomputation"; DiskANN "PQ-based graph traversal with real-time
reranking for the best speed-accuracy trade-off." Flag `--backend {hnsw,diskann}`.

**DiskANN** — needs `--extra diskann` + native deps (libomp, boost, protobuf, zeromq, MKL/oneAPI),
macOS 13.3+; writes vectors to `.bin`, calls native `diskannpy.build_disk_float_index`, auto-partitions
for recompute; PQ sized ~1/10 of embeddings.

**Claude Code / MCP**:
```bash
uv tool install leann-core --with leann
claude mcp add --scope user leann-server -- leann_mcp
leann build my-project --docs $(git ls-files)
```

---

## 6. Dependency / runtime reality → Rust equivalents

| LEANN dep | Role | Rust equivalent |
|---|---|---|
| Python 3.10+ | orchestration, CLI, apps | replaced wholesale by our Rust core |
| modified FAISS (C++/SWIG) | HNSW build + CSR search + IVF; the two-level recompute search | no drop-in; `hnswlib-rs`/`instant-distance`/`hora` for HNSW + our own CSR-prune + two-level search |
| DiskANN `diskannpy` (C++; MKL/boost/libaio) | large-scale disk index | no mature pure-Rust Vamana; **out of scope** for a wasm/pure-Rust v1 |
| ZeroMQ + msgpack | searcher↔embedder IPC | **eliminate** — in-process `trait Embedder` |
| sentence-transformers / PyTorch | local embedding inference | **the critical port**: `candle` (pure Rust, Metal/CUDA, some wasm), `ort` (ONNX), `tokenizers` (Rust). wasm = small encoder or host import |
| MLX | Apple-Silicon embeddings | `candle` Metal |
| OpenAI/Ollama/Gemini SDKs | remote embed/LLM | `reqwest` + serde (trivial) |
| SQLite FTS5 | BM25 hybrid | `tantivy` (query wasm-OK; indexing native-only - 0033) or `rusqlite` FTS5 |
| tiktoken | token truncation | `tiktoken-rs` |
| tree-sitter / LlamaIndex readers | AST chunking + doc loaders | `tree-sitter` (Rust), `lopdf`/`pdf-extract`; per-loader work |
| psutil/tqdm/requests | sizing/progress/HTTP | `sysinfo`/`indicatif`/`reqwest` |

**Bottom line:** the *format* (CSR graph + PQ + content-addressed passages + metadata) and the
*algorithms* (best-first traversal, two-level hybrid-distance recompute, dynamic batching, high-degree
pruning) port cleanly to pure Rust and are the parts worth porting. The two real dependencies are an
**in-process embedding runtime** (our wasm32 binding constraint) and an **HNSW graph builder**.
Everything LEANN built for process isolation (ZMQ, daemon, registry, locks) is a Python artifact we
drop.

---

## 6b. Prototype: measured storage vs recompute (`prototypes/vector-tradeoff`)

To validate "recompute-instead-of-store" and that it is **complementary** to our content-addressed
dedup, a std-only Rust prototype (`prototypes/vector-tradeoff/`) measures three storage/query
strategies over a synthetic corpus. The "embedding" is a deterministic, tunable workload (measured at
**7.9 µs/embedding** for D=384), *not* a real encoder — it exists only to give recompute a realistic
per-call cost so latencies compare fairly. Real-encoder costs are larger, which only sharpens the
conclusions.

Main grid (D=384, top-10, graph degree 32, visited ≈ 16·log₂N):

| N | A. store-FP32 + exact | B. recompute-all + exact | C. recompute-visited (LEANN) |
|---|---|---|---|
| 1,000 | 1.86 MB / 0.36 ms | 0.40 MB / 7.9 ms | 0.52 MB / 1.23 ms |
| 10,000 | 18.6 MB / 3.59 ms | 3.95 MB / 77.8 ms | 5.17 MB / 1.65 ms |
| 100,000 | 186 MB / 35.8 ms | 39.7 MB / 784 ms | 51.9 MB / **2.09 ms** |

Dedup demonstration (N=100k, 30% exact duplicates), vs the naive store-FP32 baseline (186 MB):

| scheme | size | vs naive |
|---|---|---|
| store-FP32, no dedup | 186.1 MB | 1.00× |
| store-FP32 + content-addressed dedup | 131.2 MB | 0.71× (−29.5%) |
| recompute-C (drop vectors) + dedup | **37.1 MB** | **0.199× (−80.1%, 5× smaller)** |

**What the numbers say.** (1) For **native / large N**, the LEANN trick (mode C: drop vectors, keep a
pruned graph, recompute only the ~266 visited nodes) cuts storage ~72% *and* is **faster** than the
exact FP32 scan (2.1 ms vs 35.8 ms at 100k) because it touches a logarithmic slice rather than all N.
(2) The naive "recompute everything" (mode B) is the trap — 784 ms/query at 100k — so the *graph* is
the real trick, not recompute alone. (3) For **browser / small N** (≤ a few thousand), exact recompute
is ~8 ms/query — interactive — so the browser can skip the graph entirely and stay exact and smallest.
(4) **Dedup and drop-vectors stack** (29.5% + 72.1% → 80.1%) because they attack orthogonal redundancy
— dedup shrinks the unique-item count, recompute shrinks the per-item cost. This confirms LEANN's trick
and our content addressing are **complementary, not the same**: we get both savings together.

> Caveat: the model's recompute cost is synthetic; a real encoder (tens of ms/embedding on CPU) makes
> mode B far worse and mode C's "few hundred recomputes" the deciding factor — i.e. the graph-pruned
> recompute is essential, and an in-process embedding runtime (0017/§7) is the gating dependency.

## 7. How this maps onto uldren-loom

A crucial framing difference: **LEANN's storage trick and ours are complementary, not the same.**
LEANN saves space by *not storing embeddings and recomputing them*; uldren-loom already saves space by
*content-addressed structural sharing + versioning* (BLAKE3 Merkle store, 0002). The big new idea
LEANN offers us is **recompute-instead-of-store for the embedding vectors**, layered on top of our
existing dedup — and it fits our model unusually well, because our compute facet (0015) already gives
us a sandboxed, deterministic, on-demand execution path, and our vector facet (0017) already treats
the ANN index as a derived, rebuildable, non-synced artifact.

### What we already support (or have specced)
- **Content-addressed, versioned, deduplicated storage** (0002) — our structural-sharing analogue of
  LEANN's storage goal, plus branch/merge/diff/sync that LEANN lacks.
- **Sync** (0006, built P7) — LEANN has only local Merkle change-detection; we have real
  clone/push/pull + bundles.
- **Workspaces** (0014) incl. a `vector` type, and `program` for executable logic.
- **Vector facet surface** (0017): `upsert`/`search`, fixed dimension+metric, metadata **pre-filter**,
  exact search, **derived/rebuildable/non-synced index** — already aligned with LEANN's "index is a
  rebuildable cache."
- **On-demand deterministic compute** (0015, built files-facet engine) — the substrate a
  recompute-on-search scheme would run on.
- **MCP server** (planned, 0013, `rmcp`) and a **CLI** (`loom-cli`) — same surfaces LEANN exposes.
- **Merkle change detection** — we *are* a Merkle store, so `leann watch`-style incrementality is free.

### What we can add easily (low risk, mostly pure-Rust glue)
- **CSR pruned-graph storage format** as Loom objects (blobs); fits the object model directly.
- **Exact (flat) vector search** — already the v1 engine decision (ADR-0008); identical native/web.
- **Metadata pre-filter** in `search` — already a resolved 0017 decision.
- **Hybrid dense + BM25** via `tantivy` (pure-Rust; query is wasm-OK, but **indexing is native-only** -
  tantivy's writer is multi-threaded; 0033 / 0032 §4.8). Not Arrow-based (its own Lucene-style format).
- **AST-aware chunking** via `tree-sitter` (Rust bindings) and basic doc loaders.
- **Grep/exact-text search** over passages.
- **Remote embedding/LLM providers** (OpenAI/Ollama/Gemini) via `reqwest` — trivial, and wasm-safe
  (HTTP), so even the browser can embed by calling a provider.
- **Incremental add/remove** — we already have versioned add + the IVF-style remove pattern.

### What needs discussion & decisions
- **Recompute-instead-of-store for embeddings (the headline idea).** Adopting it means a `vector`
  workspace stores text + pruned graph + PQ but *not* the FP32 vectors, recomputing visited nodes via
  an embedder during search. Huge storage win; costs query CPU and **requires an in-process embedding
  runtime** — which is exactly the kind of heavy/wasm-sensitive dependency we just gated for OLAP and
  ANN. Decision needed: do we (a) store embeddings (simple, bigger), (b) recompute (LEANN-style,
  smaller, needs embedder), or (c) make it a per-workspace choice? My lean: **(c)** — store by
  default; offer a `recompute` mode behind the embedder feature, native-first, with the browser using
  a host-provided or remote embedder.
- **Embedding-runtime dependency** (`candle` vs `ort` vs host/remote) — a real ADR, same shape as
  ADR-0008. wasm is the deciding constraint (large encoders are slow in-browser).
- **Two-level hybrid-distance search + PQ table** — worth it for recall-at-compression, but only
  meaningful *with* an HNSW accelerator, which is itself deferred (ADR-0008). Sequence after exact
  search ships.
- **High-degree-preserving pruning + HNSW builder** — depends on choosing/porting a wasm-capable HNSW
  (the open item from ADR-0008).
- **Determinism of recomputed float embeddings across native/web** — recompute must yield the same
  vectors on both platforms or search diverges; ties into the parity rule we just set.

### What we can't easily add (or should not, now)
- **DiskANN backend** — C++ Vamana, MKL/boost/libaio, no pure-Rust equivalent, not wasm — out of
  scope.
- **GPU FlashLib backends** — CUDA-only.
- **In-browser large-encoder inference at LEANN's quality** — feasible but slow; needs a small encoder
  or host offload.
- **MLX path** — Apple-specific; `candle` Metal covers the need without a separate dependency.
- **The ZMQ/daemon/registry machinery** — deliberately *not* ported; in-process embedder replaces it.

---

## 8. Feature table (ROI vs difficulty)

ROI = value to uldren-loom's local-first/AI-memory story. Difficulty = engineering cost in our
pure-Rust + wasm32 context. Verdict = recommended disposition.

**In code?** legend: ✅ built & tested · 🟡 partial / prototype · ⬜ decided, not built · ❌ out of scope.

| Feature | Category | Decision | In code? | ROI | Difficulty | Verdict |
|---|---|---|---|---|---|---|
| Exact (flat) vector search | Search | the cross-platform contract (ADR-0008) | ⬜ not built (next) | High | Low | **Do (v1)** |
| Metadata pre-filter | Search | resolved (0017 §RD2) | 🟡 `Predicate` pre-filter built in `tabular`; vector pending | High | Low | **Do (v1)** |
| CSR pruned-graph format | Storage | ships with HNSW | ⬜ not built | Med | Low | **Do (with HNSW)** |
| Hybrid dense + BM25 | Search | via `tantivy` (query wasm-OK; indexing native-only, 0033) | ⬜ not built | High | Low–Med | **Do soon** |
| AST-aware chunking | Ingest | `tree-sitter` | ⬜ not built | Med | Low–Med | **Do soon** |
| Data-source loaders | Ingest | per-source readers | ⬜ not built (`data-sources/` download script added) | Med | Med | **Incremental** |
| Remote embed/LLM providers | Providers | spec'd in 0050 | 🟡 ReAct prototype calls one via `reqwest` | High | Low | **Do** |
| Incremental update + soft delete | Index | deferred/IVF-style indexing | 🟡 `tabular` upsert/delete + vcs commit/branch built | Med | Med | **Do** |
| Recompute-instead-of-store embeddings | Storage | **recompute-by-default, per-ns config** (0017 §RD5) | ⬜ not built | High | High | **Do (needs runtime)** |
| In-process embedding runtime | Runtime | **candle (wasm-confirmed)**, 0050 | ⬜ not built | High | High | **Do (ADR + 0050)** |
| Two-level hybrid-distance search + PQ | Search | **adopted** (0017 §RD6) | ⬜ not built | Med | Med–High | **After HNSW** |
| High-degree-preserving pruning | Index | **our own CSR pass** (0017 §RD7) | ⬜ not built | Med | Med | **With HNSW** |
| Dynamic batching | Runtime | batch recompute ~64 | ⬜ not built | Low–Med | Low | **After recompute** |
| HNSW accelerator | Index | **adopted**: `hnsw_rs` native, browser via gated fork (ADR-0008) | ⬜ not built | High | Med–High | **Above threshold** |
| Threshold auto-select (exact↔HNSW) | Search | resolved (0017 §RD4) | ⬜ not built | High | Low | **Do (with HNSW)** |
| Sharded storage-bounded build | Build | k-means + shard + merge | ⬜ not built | Low | Med–High | **Later** |
| MCP server | Surface | planned (0013, `rmcp`) | ⬜ not built | High | Low–Med | **Do** |
| ReAct agent | Surface | tokio + reqwest loop | 🟡 prototype (`prototypes/react-agent`) | Med | Med | **Prototype done** |
| MLX Apple-Silicon accel | Runtime | native-gated, default-off | ⬜ not built (modeled in bench) | Low–Med | Med | **Optional native** |
| DiskANN backend | Index | out of scope (C++/MKL) | ❌ | Low | Very High | **No** |
| GPU FlashLib backends | Index | out of scope (CUDA) | ❌ | Low | Very High | **No** |
| In-browser large-encoder inference | Runtime | out of scope now (slow in wasm) | ❌ | Med | Very High | **No (now)** |

**Recommended order:** the v1 vector facet is well-scoped and offline-buildable now — **exact search +
metadata pre-filter + the threshold switch + CSR format**. The two dependency commitments are the
**embedding runtime (candle, confirmed wasm-capable)** behind 0050, and **recompute-vs-store** (decided:
recompute-by-default). HNSW (`hnsw_rs` native; browser via the gated fork) + two-level search + PQ +
our pruning pass form the above-threshold block. DiskANN/GPU stay out of scope; MLX is an optional
native fast-path.

## 9. Captured feature ideas

- **Browser-extension "scrape a site into a vector DB."** Ship the `wasm32` build as a browser plugin
  so a user, while browsing, can capture the current page (or a whole site) directly into a local
  `vector` (or `files`/`document`) workspace — a private, on-device "time machine for the web" that
  indexes as you read, with zero cloud. This is a **web-only** capability in the parity model (0032):
  it has no native analogue (it lives in the browser's page context) and pairs naturally with exact
  search (browser-scale corpora) + remote/host embeddings. It is the browser counterpart to LEANN's
  `browser_rag` app, but live and incremental rather than a one-shot export. Also captured in
  `AI-CAPABILITIES-LANDSCAPE.md`.

## 10. Three-strategy benchmark (exact vs HNSW vs LEANN)

The enhanced `prototypes/vector-tradeoff` (binary `strategies`) implements a **real minimal HNSW** in
pure Rust and compares it against exact scan and the LEANN recompute path, with a measured recall@10.
Headline numbers (D=384, single-threaded; measured up to 50k, extrapolated to 6.5M):

| N | EXACT query | HNSW query | HNSW speedup | HNSW recall@10 |
|---|---|---|---|---|
| 1,000 | 0.28 ms | 0.18 ms | 1.6× | 0.982 |
| 5,000 | 1.43 ms | 0.19 ms | 7.6× | 0.997 |
| 20,000 | 5.69 ms | 0.15 ms | 38× | 1.000 |
| 50,000 | 14.2 ms | 0.18 ms | 77× | 0.992 |
| **6.5M (≈10 GB)** | **~1.9 s** (extrap.) | **~0.37 ms** | **~5000×** | — |

Scenario winners: **(S1) mobile / small N** — exact is fine (sub-ms, recall 1.0), HNSW already 7.6× at
5k. **(S2) 10 GB** — HNSW decisive (1.9 s vs 0.37 ms): this is the evidence HNSW is *required* at scale.
**(S3) 1 GB 80/20 read/write** — HNSW (reads dominate; 3212 vs 7 ops/s). **(S4) write-heavy** — exact /
append-only wins (165k writes/s vs HNSW 2.4k inserts/s), i.e. defer indexing (snapshot + flat delta).
**MLX (modeled 8–20×)** shrinks the recompute/scan *constant* but exact@6.5M is still ~0.09 s vs HNSW
0.37 ms — MLX changes the constant, HNSW changes the exponent. This is the evidence base for the
threshold auto-select (0017 §RD4) and HNSW adoption (ADR-0008).

## 11. Decisions resolved this round (and where they live)

- **Are we "better than LEANN"?** No — **complementary.** LEANN's win is recompute-instead-of-store;
  ours is content-addressed dedup + versioning. They stack (29.5% + 72.1% → ~80%, §6b). We adopt
  LEANN's trick *on top of* our dedup.
- **Embeddings: recompute-by-default, per-workspace configurable; storing is the opt-in.** Confirmed
  sync-friendly (recompute is the *smaller* payload). 0017 §RD5.
- **Embedding runtime: candle** (confirmed wasm32-capable — CPU+SIMD128, `tokenizers` via
  `unstable_wasm`; model weights are the cost). Speccd in **0050**. 0017 §RD5 / 0032 §4.7.
- **HNSW adopted** as the opt-in accelerator with **two-level search + PQ table**; **high-degree
  pruning is our own pure-Rust CSR pass** (browser-compatible by construction). 0017 §RD6/§RD7.
- **Threshold auto-select**: exact below a per-workspace count, HNSW above. 0017 §RD4.
- **Backends (Section 3 answer):** build **exact** (contract) + **HNSW** (`hnsw_rs` native, browser
  fork) + an **IVF-style deferred-index** path for write-heavy (the snapshot+flat-delta model);
  **DiskANN and GPU are out of scope**.
- **Multi-provider (Section 4):** yes, its own spec — **0050** (embedding + LLM provider abstraction).
- **Incremental re-index concern (Section 4):** we do **not** rebuild the whole index per small
  change. Adds/removes land in an unindexed **delta** (exact-scanned for correctness) and fold into
  the index on a threshold / in the background (LEANN soft-delete + batched-add). A change every 10 s
  costs a cheap append + an occasional background rebuild, not a full re-index. 0017 §RD3.
- **MLX:** an optional, default-off, cfg-gated **native macOS** fast-path (~3× embeddings, ~4–10×
  matmul); never wasm or Linux-CI. ADR-worthy when Apple-Silicon users are a priority.
- **VCS-behavior:** every data-layer spec now documents how commit/checkout/branch/merge/sync act on
  its data and what is derived/rebuilt (0017 §7b is the template; 0000-index records the convention).
