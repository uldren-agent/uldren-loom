# 0050 - Embedding providers

**Status:** Partial, provider seam and curated Candle CPU activation source-backed; broader runtime acquisition is owned by 0062 ·
**Version:** 0.2.0-draft · **Optional capability `providers.embedding`.**
**Depends on:** 0017 (vector facet - the embedding consumer), 0015 (programs may embed under
capability), 0032 (native/web parity), 0007 (bindings). **Relates to:** 0051 (LLM/chat providers),
LEANN "multi-provider support".

An **embedding provider** turns text into vectors, behind a small trait so the `vector` facet never
binds to a specific model, vendor, or runtime. This is LEANN's "multi-provider support" as a
first-class Loom concern: the vector facet's recompute path (0017 §RD5) and any program (0015) that
needs an embedding go through a provider, and the provider is **swappable** (local in-process or remote
HTTP) without touching the data model.

This spec is text-only. 0068 owns image, audio, video, PDF, and multimodal embedding source pipelines
because they need media decoding, chunking, frame/window selection, source provenance, and
model-specific metadata before they become ordinary vectors in 0017.

> **Scope split.** This document covers **embeddings only**. LLM / chat completion providers
> (`ChatProvider`, streaming, the ReAct agent, derived-view summarization) live in **0051**. They have
> a different shape (token streaming, tools, backpressure) and a different consumer, and mixing them
> here muddied both. The two share only the "swappable behind a trait, endpoint/secret via env" posture.

## 1. Why a separate spec

Embeddings are the **load-bearing** provider kind: the vector facet depends on them, and they carry
real **dependency and platform** weight (an in-process inference runtime that compiles to wasm, Apple
GPU concerns) plus a **determinism contract** (§5) that the vector facet's recompute model relies on.
That is enough surface to live on its own rather than buried inside 0017.

## 2. The embedding trait

```idl
interface EmbeddingProvider {
  model_id(): string                       // stable id, e.g. "candle:all-MiniLM-L6-v2@<rev>"
  dimension(): u32                          // fixed output width
  embed(texts: List<string>): Future<List<Vector>>   // batched
}
```

`model_id` is part of a `vector` workspace's identity (§4); `dimension` must match the workspace's
fixed width (0017 §RD1).

Current source backs the thin provider seam through `loom-inference`, re-exported by
`loom-core::inference`. The handle reports `UNSUPPORTED` when no provider is installed, validates
batch result count and vector dimensions, and exposes model id, dimension, and optional weights digest
to vector source-aware writes. 0062 owns concrete runtime acquisition, runtime packaging, provider
activation, and doctor compatibility. The source tree has Candle CPU text embedding activation for
curated safetensors models. MLX, hosted HTTP, Core ML, Ollama, CUDA, and llama.cpp are represented in
0062 by runtime flags, adapter contracts, doctor checks, or explicit bridge-missing behavior where
provider execution is not promoted.

## 3. Implementations (all behind the trait)

These are **alternative** implementations a deployment picks from; they are not layered on each other.

- **In-process `candle` - the portable runtime, used everywhere.** A pure-Rust inference framework that
  runs on **CPU on every target** (Linux/CI, macOS, **iOS**, and the browser via `wasm32` CPU+SIMD128;
  `tokenizers` via `unstable_wasm`), with **GPU via Metal on macOS and CUDA on Linux**. It is the
  default because it is the only *cross-platform* runtime - one provider, one model id, every platform.
  **Caveat (measured, June 2026):** candle has **no Metal backend on iOS** - on iOS it is **CPU-only**
  (small/quantized models only). GPU-accelerated embedding *on iOS* therefore needs MLX (next bullet).
- **Remote HTTP - OpenAI-compatible (`/v1/embeddings`).** A single client (base URL + bearer token)
  covers OpenAI, Jina, Ollama, LM Studio, vLLM, Gemini, etc. Pure `reqwest` + JSON, wasm-safe (fetch),
  the simplest provider, and the way the browser can embed without shipping a model.
- **MLX - optional Apple runtime through 0062.** Because candle is CPU-only on iOS, Apple GPU or ANE
  acceleration for on-device embedding needs an Apple runtime path. The current 0062 direction is not
  an iOS-binding-only `mlx-rs` dependency. It is an optional Apple runtime profile in `loom-inference`
  that loads a refreshable native MLX C bundle through a Loom-owned adapter ABI. Non-Apple builds and
  the default Loom binary must run without MLX headers, libraries, or Apple frameworks. Using a
  different backend on Apple devices remains safe by construction because recompute vectors are local
  and never synced (§5).

## 4. Configuration & selection

- **Per-workspace embedding model: one hard MUST (local), the rest SHOULD.** A `vector` workspace
  **records** its `model_id` + `dimension` + weights digest as metadata. The only hard requirement is
  **local and automatic**: within a single index, the query MUST be embedded by the same model that
  produced that index's vectors (else you compare points from different spaces) - and since a peer uses
  one embedder for its own index, this holds for free. Across peers it is a **SHOULD, not a MUST**:
  recompute vectors are **never synced** (§5), so two peers on *different* models each search their own
  local index correctly; the only consequence of a mismatch is they may return *different rankings*,
  never corruption or a broken sync. The recorded `model_id`/digest exist so a peer can **detect and
  warn** on a mismatch (and `dimension` must still agree if vectors are ever stored+synced) - they do
  **not** gate reads or sync. There is no hard dependency on any particular embedding model.
- **Endpoints & secrets** are environment/flag config, never stored in the data: `LOOM_EMBED_*` (base
  URL, token, model). Tokens never enter the object store or sync.
- Selection is explicit (no ambient default that silently changes results); the chosen provider is
  reported in `capabilities()`. The backend, such as candle CPU, candle Metal, MLX, or Core ML when it
  exists, is a local choice that does not change the workspace identity because backend output is never
  synced (§5).
- **Model acquisition (weights).** 0062 owns concrete acquisition and runtime package policy. The
  current native enterprise path is `hf-hub` with default features disabled and `tokio` plus
  `rustls-tls`, using the shared Hugging Face cache. Candle consumes safetensors and tokenizer/config
  files for curated encoder models. MLX compatibility is runtime-specific and MUST be proven per
  curated model; do not assume every candle safetensors embedding model is also MLX-compatible.
  `candle-core` and `candle-transformers` do not depend on `hf-hub`; they load weights from bytes or
  files supplied by the acquisition layer. Acquisition is separate from inference, and it is
  platform-split (§6 / 0032):
    - **Native:** follow 0062. Use shared-cache `hf-hub` acquisition for the release target rather than
      treating direct HTTP GET as an equal native implementation path.
    - **Browser (`wasm32`):** **`hf-hub` cannot run** (no `std::fs`, no native sockets - its whole API
      is filesystem-path-based). The browser MUST fetch the safetensors/tokenizer/config via the
      platform `fetch()` (candle's own browser examples do exactly this), hold the bytes in memory (or
      cache in **OPFS/IndexedDB**), and hand the bytes to candle (which compiles to `wasm32`). No
      `hf-hub`, no disk cache.
  Recorded metadata: the `model_id` + weights digest let a peer **detect/​warn** on a model mismatch
  (SHOULD, per §4) - not a synced-identity gate. A workspace MAY instead **carry its weights inside the
  Loom** - a content-addressed `cas` "models" space, dedup'd + synced - for offline, self-contained,
  browser-friendly reproducibility (this also sidesteps the browser-fetch/CORS path entirely). A
  dedicated *external* HuggingFace org/Space is only worth standing up if/when we publish our **own
  canonical int8-quantized** weights for the store-and-sync path (§5).

## 5. Determinism contract - GPU output is local, never synced

This is the spine of the whole design, and it dissolves the float-nondeterminism worry.

The vector facet's default mode is **recompute** (0017 §RD5): what a Loom stores and **syncs is the
text**, not the vectors. Each peer recomputes its own embeddings and builds its own index **locally**,
and search runs against that local index. Per-peer indexes are **never compared byte-for-byte across
peers**, so they do **not** need to be bit-identical.

That matters because FP matmul is non-associative and backends reduce in different orders, so
candle CPU, candle Metal, MLX, and future Core ML paths can differ in the low bits for the same
model and input. Given the
recompute model, that is a non-issue:

- **Recompute mode (default): GPU output is local and ephemeral.** A peer MAY embed on whatever is
  fastest locally, such as candle Metal on a Mac, MLX on Apple hardware, candle CPU on a server, or a
  future Core ML provider, because those
  vectors live only in that peer's local, rebuildable index (like the HNSW graph, 0017 §RD6) and are
  **never written into the synced object store**. Different peers on different backends is fine.
- **Store-and-sync mode (opt-in): use the canonical path.** Only when a workspace opts to **store and
  sync the vectors themselves** (0017 §RD5) does bit-reproducibility matter. Those vectors MUST come
  from **one canonical deterministic path** - a pinned **CPU** embedder, or **int8-quantized** output
  that is exactly reproducible regardless of backend. `model_id` pins model+revision; the canonical path
  pins the arithmetic.

So: GPU-capable local providers accelerate local work; the synced source of truth is either text
(recompute) or canonical-path vectors (store). Same reconciliation discipline as the HNSW accelerator
(0017 §RD6) and the platform-parity float caveat (0032 §4.7): a deterministic path defines the
contract; faster/looser paths stay local or are canonicalized.

## 6. Parity (0032)

- **Remote HTTP:** identical on native and web (just fetch).
- **candle in-process:** CPU on every target including iOS and the browser; GPU via Metal on macOS and
  CUDA on Linux. iOS is CPU-only. Behaviour is uniform under the §5 rule (local output, never synced).
- **MLX:** optional Apple-only runtime profile owned by 0062. It is absent on non-Apple targets and
  optional even on Apple hosts, so its absence changes performance and supported local models, not
  synced data semantics.
- **Core ML:** future Apple built-in runtime path owned by 0062 after MLX dynamic loading is
  source-backed. It is distinct from MLX and candle Metal.
- **Weight acquisition is a hard parity divergence (0032).** Native release acquisition uses the 0062
  shared-cache `hf-hub` path; the **browser cannot use `hf-hub` at all** and MUST fetch weights via `fetch()`
  into memory/OPFS (or carry them in a `cas` "models" space). Inference (candle) is uniform; only *how
  the weights arrive* differs - record it as a §2.3-style degradation, not a silent gap.

## 7. Weight acquisition background (`hf-hub`, candle, browser)

The findings below explain why acquisition is separate from the embedding provider contract. 0062 owns
the current native downloader choice, dependency profile, cache behavior, and runtime packaging.

- **candle does NOT depend on `hf-hub`.** Both `candle-core` and `candle-transformers` show **0**
  `hf-hub` entries in their dependency trees (verified). candle loads a model from **in-memory bytes /
  a safetensors buffer you hand it**; `hf-hub` is pulled only by candle's *example binaries*. So
  **acquisition is a separate concern from inference**, and choosing how to fetch weights does not
  touch the inference dependency.
- **`hf-hub` is native-only - it cannot run in a browser.** Its entire API is **filesystem-path
  based**: `Cache::new(path: PathBuf)`, `CacheRepo::get(..) -> Option<PathBuf>`,
  `ApiRepo::download(..) -> PathBuf` - it downloads into a `std::fs` cache and returns file paths (34
  `std::fs`/path calls in `sync.rs` alone), over native HTTP (`ureq`, or `reqwest`+`tokio`), with
  essentially no `wasm` awareness. `wasm32` (browser) has **neither a filesystem nor native sockets**,
  so `hf-hub` is out there by construction.
- **`hf-hub` default features are heavy** (when used natively): the default pulls a blocking client
  **and** an async path, and **two TLS stacks** (`native-tls`/openssl **and** rustls) - on the order of
  ~150 transitive crates. If we use it, take `default-features = false` and select **one** client +
  rustls only.
- **Native acquisition target:** 0062 selects `hf-hub` with default features disabled and
  `tokio,rustls-tls` for the release path. Direct Hub file GETs remain useful for browser fetch and
  diagnostics, but they are not the equal native implementation target.
- **Browser acquisition (required path):** since `hf-hub` cannot run, the browser MUST fetch the
  `config.json` / `tokenizer.json` / `model.safetensors` via the platform `fetch()`, hold the bytes in
  memory (or cache in **OPFS/IndexedDB**), and hand the bytes to candle (which compiles to `wasm32`).
  candle's own browser examples already do exactly this. No `hf-hub`, no disk cache.
- **Decision recap (§4):** default = fetch + **record the weights digest** as workspace metadata for
  detect-and-warn (SHOULD-match, not a sync gate); opt-in = carry weights inside the Loom in a
  content-addressed `cas` "models" space (which also sidesteps the browser fetch/CORS path entirely).
- **Determinism is NOT a hard dependency (§4/§5).** Matching the embedding model across peers is a
  **SHOULD**, not a MUST: recompute vectors are never synced, so a mismatch only changes rankings.

## Open questions

1. **Provider trait location. [RESOLVED - decision (a), source-backed.]**
   - **Context.** Every facet (vector recompute, document/graph enrichment, programs) needs to reach an
     `EmbeddingProvider`, but the candle runtime is a heavy dependency and `loom-core` must stay
     wasm-clean and dependency-light.
   - **Example.** If the candle implementation lived in `loom-core`, every consumer of the core - the
     wasm binding included - would drag in the inference runtime even when it only ever calls a remote
     HTTP embedder.
   - **Options.** (a) a thin `EmbeddingProvider` trait in `loom-core`, implementations in a separate
     optional `loom-providers` crate (the loom-sql / loom-hnsw promotion pattern); (b) trait and all
     implementations in `loom-core` behind cargo features; (c) the whole thing in a separate crate,
     `loom-core` unaware of providers.
   - **Decision.** (a), updated by 0062 crate split - the trait is available through
     `loom-inference` and re-exported by core, while concrete candle, MLX, hosted HTTP, and browser
     acquisition code stays outside the core engine.
2. **Weight distribution. [RESOLVED - decision (c) + (a) opt-in.]**
   - **Context.** A recompute index is only reproducible if every peer uses the same model weights, but
     weights are large and the provider may fetch them out of band.
   - **Example.** Two peers both say `model_id = "candle:all-MiniLM-L6-v2@<rev>"`, but one silently has
     a different quantization of the weights - their recomputed indexes diverge despite matching ids.
   - **Options.** (a) store weights *in the Loom* (content-addressed, synced, dedup'd) so a `.loom`
     carries its own embedder; (b) always fetch from the provider and trust `model_id`; (c) fetch but
     record the weights' digest in the workspace identity so a mismatch is detectable.
   - **Decision (owner-confirmed, normative).** **(c): fetch from HF Hub and record the weights' digest
     as `vector` workspace metadata** (alongside `model_id` + `dimension`). It is **recorded for
     detect-and-warn, not as a hard identity/sync gate**: because recompute vectors are never synced
     (§5), a model/weights mismatch between peers is a SHOULD-warning that may change *rankings*, not a
     MUST that blocks reads or sync (§4). **(a) is the opt-in:** a workspace MAY instead carry its
     weights inside the Loom (a content-addressed `cas` "models" space, dedup'd + synced) for offline,
     self-contained reproducibility - which also avoids the browser's no-`hf-hub` fetch path. See §4
     "Model acquisition".
3. **Embedding batch sizing across the ABI.**
   - **Context.** `embed(texts)` is batched, but the batch that is optimal in-process differs from what
     is sane to marshal across the C ABI (0007) or a remote call.
   - **Example.** A program embeds 100k chunks; passing them as one ABI call balloons a single buffer,
     while one-call-per-text destroys throughput.
   - **Options.** (a) a fixed internal batch the provider chunks to, hiding it from callers; (b) a
     caller-supplied batch hint; (c) a streaming `embed` that yields as batches complete.
   - **Recommendation.** (a) with a sensible default, plus (b) as an override - keeps the common path
     simple while letting large jobs tune; streaming (c) is deferred to the 0051 streaming work.
