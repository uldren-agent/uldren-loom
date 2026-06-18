# DESKTOP.md - Companion desktop & mobile app (Loom-as-core)

**Status:** Exploratory planning note. **Normative?** No. **Relates to:** the Loom specs (0001-0033),
`LEANN.md`, and the `bindings/` (node, wasm, react-native, ios, android, jvm, cpp).

This captures a design conversation about an **application** - a desktop app and a mobile app - that
**bundles Loom as its embedded data/memory core** and stitches together retrieval, local models,
voice, ingestion, and document generation around it.

**Scope boundary (read first).** Nothing here proposes changes to Loom's spec or pushes these
features into the Loom core. Loom stays what it is: a content-addressed, versioned store with the
filesystem / VCS / SQL / vector / search / KV / document / graph / ledger facets, a C ABI, sync, and
the model-providers facet (0050) for embedding/LLM provider abstraction. Everything below - ingestion
parsers, the agent/ReAct loop, voice, document renderers, the UI, third-party model hosts - lives in
the **app layer on top of Loom**. The recurring discipline is to keep app concerns out of the core
and let Loom do storage, versioning, sync, search, and vectors.

## 1. What the app is

A local-first knowledge and assistant app, NotebookLM-shaped, where:

- Loom is the embedded core (memory, versioned corpus, vector + full-text retrieval, sync between the
  user's own devices).
- A local LLM host (LM Studio by assumption) provides generation; a local or remote embedder provides
  vectors.
- The app adds ingestion (turn any source into clean, chunked text), retrieval-augmented chat, voice
  in/out, and document generation (PDF / DOCX / XLSX / PPTX).
- The same Loom core ships on desktop and mobile through Loom's existing bindings.

Loom is the part that is durable, versioned, and synced. The app is the part that perceives
(ingestion, voice), reasons (the LLM + ReAct loop), and produces (answers, generated files).

## 2. Architecture at a glance

```
+-------------------------------------------------------------+
|  UI layer                                                   |
|   desktop: Tauri 2 / native;  mobile: React Native (TurboModule) |
+-------------------------------------------------------------+
|  App / orchestration layer                                  |
|   agent loop (ReAct), tools, MCP client/host, voice pipeline|
+-------------------------------------------------------------+
|  Model / runtime layer                                      |
|   LLM host (LM Studio, OpenAI-compatible)                   |
|   embedder (candle/Metal native, remote/host fallback)      |
|   STT / TTS services                                        |
+-------------------------------------------------------------+
|  Loom core  (Rust, via C ABI binding)                       |
|   fs/vcs/sql/kv/document/vector/search/graph/ledger + sync  |
+-------------------------------------------------------------+
```

Loom is reached through its C ABI (`libuldren_loom`); the UI talks to it via the per-platform binding
(`react-native` TurboModule on mobile, a native/Tauri binding on desktop, `wasm` for any web surface).

## 3. The reference experience: NotebookLM-shaped

NotebookLM is the model to emulate. It is a source-grounded RAG assistant (Gemini under the hood) with
three functional parts, mapped here to app pieces and to where Loom fits:

| NotebookLM part | What it does | Where it lives in this app |
| --- | --- | --- |
| **Sources** | ingest PDFs/Docs/slides/URLs/YouTube/text; build the grounding corpus | app ingestion pipeline writes documents + embeddings into **Loom** (`document` + `vector` + `search`) |
| **Chat** | grounded Q&A with citations back to source spans | app ReAct loop: retrieve from Loom, generate with the LLM host, cite |
| **Studio** | generate Audio/Video Overviews, Mind Maps, Slides, Reports, Quizzes | app generation tools (voice + document renderers), outputs stored back in Loom |

The 2026 NotebookLM redesign is a three-column Sources / Chat / Studio layout. Under the hood it is the
standard RAG loop: chunk, embed, store, retrieve top-k, generate with citations. The app reproduces
that loop with Loom as the corpus + retrieval store (and a better one, since Loom versions the corpus).

## 4. Retrieval and RAG: Loom as the core, MCP as the universal adapter

The clean integration is to expose Loom as an **MCP server** with a retrieval tool (a `search` / vector
query over its workspaces - the `loom mcp` surface the repo already plans via `rmcp`). One
implementation then plugs into every MCP host.

| Host | How Loom plugs in |
| --- | --- |
| **LM Studio** | (a) built-in "Chat with Documents" RAG (attach .pdf/.docx/.txt; it chunks + embeds + retrieves) - shallow, per-chat. (b) **MCP** - LM Studio added MCP host support in the 0.4.x line (0.4.12 added MCP OAuth). Loom-as-MCP-server is the real path: the local model calls Loom's retrieve tool. |
| **Claude Desktop** | the canonical MCP host - cleanest fit. Add the Loom MCP server to its config and Claude calls Loom's retrieval tools directly. Local-RAG-over-MCP servers for Claude Desktop already exist (including Rust ones). |
| **Gemini** | no consumer-app MCP plug-in today. Paths: **Gemini CLI** supports MCP (`~/.gemini/settings.json` `mcpServers`) - same Loom server works. Programmatic: **File Search API**, **Vertex AI RAG Engine**, or function-calling that queries Loom. |

Net: build one Loom MCP retrieval server and cover LM Studio, Claude Desktop, and Gemini CLI with zero
per-host rework; only the consumer Gemini app needs the API path instead. LM Studio is also an
OpenAI-compatible server, so a custom app can call it for generation and Loom for retrieval directly.

## 5. Local model hosting and on-device embeddings

**LLM host.** LM Studio serves an OpenAI-compatible endpoint and hosts the LLM; it does no audio, so
voice services are separate (Section 6).

**Embeddings on a Mac with MLX / GPU.** The pure-Rust path that matches Loom's posture is **candle**
(HuggingFace's Rust ML framework) with its **Metal backend**, which runs the embedding model on the
Mac GPU. Pieces:

- **Download:** `hf-hub` pulls weights (`safetensors`) and `tokenizer.json` into a local cache; the
  `tokenizers` crate tokenizes; candle loads safetensors natively. Good models: `bge-small/base`,
  `gte`, `nomic-embed-text`, `e5`.
- **Run:** candle forward pass on Metal, mean-pool/CLS, L2-normalize, `f32` vector, upsert into a Loom
  `vector` workspace (0017).
- **One-call wrappers:** `fastembed-rs` (downloads models to a cache, runs locally; BGE/SPLADE/BGE-M3,
  Nomic-v2/Qwen3 behind a candle backend) or `embed_anything` (candle and ONNX backends, multimodal).
  Note: fastembed's ONNX path uses `ort` (a C++ runtime) - native-only, not wasm.
- **MLX specifically:** `mlx-rs` (Rust bindings to Apple's MLX) exists and is maturing; `metal-candle`
  claims large speedups. MLX is Apple-only and not pure Rust, so it would be a native-Mac accelerator
  behind a feature flag - the same shape as Loom's other gated native engines. `LEANN.md` concludes
  candle's Metal backend already covers the Mac-GPU need without taking MLX on as a separate
  dependency. **Decision (0050 §3): MLX is not built or linked at all** - candle+Metal is the runtime
  on Apple hardware; MLX survives only as a deferred memo scoped to the `ios` binding if ever revived.

**Determinism caveat (this is why the providers facet has a determinism contract).** GPU/Metal float
math is not bit-identical to CPU or wasm, so the same text embedded on the Mac GPU vs a wasm peer can
differ slightly and perturb nearest-neighbor order. Pin the model id + revision + backend per
workspace and treat cross-platform embedding parity as a hard constraint (the 0032 §4.7 concern for the
recompute path).

In Loom terms: the providers facet exposes an `Embedder` trait; the Mac build registers a
`CandleMetalEmbedder`; the wasm/default build falls back to a remote or host embedder. Same trait,
platform-selected implementation.

## 6. Voice: STT and TTS

LM Studio handles only the LLM, so STT and TTS are separate services. The pipeline:

```
mic -> VAD -> STT -> LLM (LM Studio) -> TTS -> speaker
```

Components and 2026 options:

- **Audio I/O + endpointing:** `sounddevice`/PortAudio for capture; **Silero VAD** (or webrtcvad) to
  detect speech start/stop.
- **STT:** **faster-whisper** (Whisper on CTranslate2 - the practical default); NVIDIA
  **Parakeet/Canary** for top accuracy; **Vosk/Kaldi** for tiny offline footprints.
- **TTS:** **Piper** (fastest, CPU-only, real-time on a Raspberry Pi); **Kokoro** (82M params, higher
  quality, still light); **Fish Speech / CosyVoice2 / IndexTTS-2** for expressive voices or cloning.
- **Wake word (optional):** openWakeWord or Porcupine.
- **Orchestration:** **Pipecat** is the current standard framework for real-time voice agents
  (turn-taking, streaming, barge-in).
- **Tidy shortcut:** **Speaches** is an OpenAI-API-compatible audio server bundling faster-whisper +
  Piper/Kokoro. Since LM Studio is also OpenAI-compatible, the whole stack speaks one API shape.

Engineer for latency: stream partial STT, stream LLM tokens, stream TTS chunks rather than waiting for
full turns.

## 7. Ingestion: replicating NotebookLM's source handling

The goal is many formats turned into clean, chunked text. High-fidelity tools are Python:

- **Docling** (IBM) - strongest self-hostable; layout-aware, tables, structured output + chunking;
  closest to NotebookLM quality.
- **MarkItDown** (Microsoft) - everything to Markdown, dead simple, text-first.
- **Unstructured** - a document ETL platform producing typed elements (Title, Table, NarrativeText)
  plus connectors.
- **Marker**, and hosted **LlamaParse** as alternatives.

Rust-native pieces if staying in-process:

- **PDF text:** `pdfium-render` (Chromium pdfium, best quality, native), `pdf-extract`/`lopdf`; OCR for
  scans via `tesseract` bindings (`rusty-tesseract`/`leptess`).
- **Office read:** `calamine` (XLSX), `docx-rs` (DOCX).
- **HTML:** `scraper` + a readability port; **Markdown:** `pulldown-cmark`/`comrak`.
- **Chunking:** **`text-splitter`** - the key Rust crate, tokenizer-aware semantic/recursive chunking.
- **Code:** `tree-sitter` for AST-aware chunking (called out in `LEANN.md`).
- **Audio/video sources (YouTube, like NotebookLM):** Whisper/faster-whisper transcription (Section 6).
- **`embed_anything`** covers ingestion + embedding together (PDFs, sites, images, audio to vectors).

The full loop: ingest (Docling or the Rust readers) -> `text-splitter` chunks -> embed (candle/Metal,
Section 5) -> store in a Loom `vector` workspace + index in a `search` workspace (0033) for hybrid
dense + BM25 retrieval -> grounded answer -> optional document-generation tool for the output artifact.

## 8. Document generation (ReAct outputs)

Generating files (PDF / DOCX / XLSX / PPTX) is an app tool the agent calls. Rust-first options:

- **XLSX:** `rust_xlsxwriter` - the gold standard (a Rust rewrite by the Python XlsxWriter author).
- **DOCX:** `docx-rs`.
- **PDF:** **Typst used as a library** is the best programmatic route (Rust, markup to typeset PDF);
  `printpdf`/`genpdf`/`lopdf` for lower-level control; HTML-to-PDF via headless Chromium if HTML is
  already rendered.
- **PPTX:** the honest gap - no strong native Rust authoring crate. Options: a template engine like
  **Carbone** (`carbone-sdk-rust`: fill DOCX/PPTX/XLSX/HTML templates from JSON), or shell out to
  Python **python-pptx**. `office2pdf` (pure-Rust, Typst-powered) converts DOCX/XLSX/PPTX to PDF but
  does not author PPTX.

ReAct mechanics: the agent calls `render_document(format, spec)`. Keep the model emitting a
**structured intermediate** (Markdown or JSON) and let a deterministic renderer produce the bytes; do
not have the LLM emit binary. The result becomes a Loom CAS blob / fs entry (0024 / 0003).

Reality check: the most mature authoring (especially PPTX) and the best ingestion (Docling) are Python.
That means a native-only sidecar, not the wasm path. Staying pure-Rust (`embed_anything` +
`text-splitter` + the format readers/writers) keeps wasm portability but gives up fidelity on messy
PDFs and real PPTX. Decide this trade per feature; it is the same native-accelerator-vs-portable-core
tension Loom already prices in.

## 9. Packaging and platform strategy

**How the app bundles Loom.** Loom's Rust core compiles to a native library and is reached over the C
ABI. The repo already carries the bindings: `react-native` (TurboModule -> `@uldrenai/loom-react-native`),
`wasm`, `ios`, `android`, `jvm`, `cpp`, `node`.

- **Mobile (React Native).** Compile Loom to a native lib for iOS/Android, expose it via the C ABI, and
  wrap it as the React Native TurboModule. The RN app builds its own UI in JS/TSX and calls into Loom
  for storage/sync/search/vector. Precise framing: you do not compile Loom "into" React Native; you
  compile Loom to a native lib and bind it into the RN app.
- **Desktop.** Tauri 2 (Rust backend + web UI) or a native shell, calling the same C ABI.

**Rust UI options** if an all-Rust UI is preferred over JS:

- **Dioxus** - closest "React for Rust": RSX (JSX-like), components, hooks, hot reload; web/desktop/
  mobile (iOS baked in, Android still experimental and config-heavy).
- **Tauri 2** - Rust backend, UI in web tech (React/Vue/Svelte), now with iOS/Android support; the
  bridge from web knowledge.
- **Slint** - declarative UI with its own `.slint` markup language; strongest on desktop/embedded,
  mobile improving.
- Yew/Leptos are JSX-ish but web/wasm only; egui/iced run on mobile but do not feel native.

Clarification: Rust text/HTML **templating** crates (`askama`, `tera`, `maud`, `minijinja`) render
strings/HTML, not mobile UIs. For app UI it is Dioxus/Slint/Tauri.

**Precedent.** Well-known apps use the same "Rust core, native UI" shape: **1Password** (headless Rust
core, thin per-platform UI), **Signal** (`libsignal` in Rust across Android/iOS/Desktop), **Mullvad
VPN** (Rust client; Rust WireGuard engine on Android), **Firefox mobile** (shared Rust components).

**Dynamic loading / plugins.** Rust crates are statically linked by default; Rust has no stable ABI, so
runtime loading only works across a **C ABI** boundary: build a `cdylib`, expose
`#[no_mangle] extern "C"` functions, load with `libloading`. `abi_stable`/`stabby` make Rust-to-Rust
plugins safer with their own ABI discipline. This is exactly what `loom-ffi` already is (a `cdylib` +
`staticlib`, the one crate allowed `unsafe`), so any plugin surface for the app follows the same
C-ABI-boundary rule.

## 10. Device acceleration notes

- **Mac:** candle Metal backend gives GPU acceleration for embeddings (and small models). Pure Rust,
  native-only acceleration behind a feature; CPU/remote elsewhere.
- **iPhone:** the phone has an Apple GPU (Metal-programmable, same API as macOS) plus a Neural Engine
  (ANE). candle's **Metal backend on iOS is weak/unsupported today** (documented limitation; reported
  command-buffer assertion failures). What works reliably is candle on the **CPU** via
  `aarch64-apple-ios`. To use the GPU/ANE, route through **CoreML** (`candle-coreml`, or `ort` with the
  CoreML execution provider) or **MLX** (`mlx-rs`/`mlx-swift`); convert the model to CoreML as needed.
- For on-device embedding on iPhone: candle-CPU is the portable fallback (fine for small models);
  CoreML or MLX is the accelerator that reaches GPU/ANE. Same native-accelerator-behind-a-trait pattern
  as the providers facet, with the same float-determinism caveat (ANE/GPU vs CPU vs wasm).

## 11. The Loom boundary (what stays in the core vs the app)

| Concern | Loom core | App layer |
| --- | --- | --- |
| Durable, versioned, content-addressed storage | yes (0002-0005) | uses it |
| Branch / merge / diff / time-travel | yes (0003) | uses it |
| Sync between the user's devices | yes (0006, 0031) | triggers it |
| Vector store + nearest-neighbor | yes (0017) | populates it |
| Full-text search (BM25) | yes (0033) | populates/queries it |
| Embedding/LLM provider abstraction + determinism | yes (0050) | selects model/provider |
| Exposing Loom over MCP | yes (`loom mcp`, planned) | configures hosts |
| Ingestion (parse PDFs/HTML/audio, chunk) | no | yes |
| Agent / ReAct loop, tool calling | no | yes |
| Voice (STT/TTS) | no | yes |
| Document generation (PDF/DOCX/XLSX/PPTX) | no | yes |
| LLM hosting (LM Studio etc.) | no | yes (external) |
| UI (desktop/mobile) | no | yes |

The point of the table: the app is where the moving, model-dependent, format-heavy work lives, and it
treats Loom as a stable embedded core rather than absorbing those concerns into it.

## 12. Open questions

1. **Embedder placement and parity.** candle/Metal native vs remote/host vs an on-device iPhone path
   (CoreML/MLX) - and how strict cross-device vector parity must be given float nondeterminism. Pinning
   model + revision + backend per workspace is the lever.
2. **Ingestion fidelity vs portability.** Python Docling/Unstructured (native sidecar, best quality)
   vs Rust-native (`embed_anything`/`text-splitter`, wasm-portable, lower fidelity on messy inputs).
   Likely per-source-type choice.
3. **PPTX authoring.** Accept PDF-only output (Typst/office2pdf), use a template engine (Carbone), or
   take a Python python-pptx sidecar for real .pptx.
4. **UI stack.** React Native (reuse JS ecosystem, the existing TurboModule binding) vs all-Rust
   (Dioxus/Slint/Tauri) for a single-language stack. Desktop and mobile could differ.
5. **MCP host vs client role.** The app can be an MCP host (calling Loom + other tools) and/or expose
   Loom as an MCP server for external hosts (Claude Desktop, LM Studio, Gemini CLI). Decide which roles
   ship first.

## 13. Running the LLM in Rust (an all-Rust alternative to the host)

§5 takes the LLM host as external (LM Studio, OpenAI-compatible). A separate question is whether the
generation side could itself be pure Rust - the same posture Loom already takes for embeddings. The
pieces exist:

**Model download.** `hf-hub` (already used for embedder weights in §5) is HuggingFace's Rust Hub client
and pulls *any* repo file - `safetensors`, GGUF, `config.json`, `tokenizer.json` - with local caching,
in a blocking (ureq) or async (tokio/reqwest) flavor. It is the same downloader candle and `tokenizers`
use, so model acquisition is a solved, in-Rust step.

**Running the model: candle, with caveats.** candle runs an architecture only if it is implemented in
`candle-transformers` - which today covers Llama (incl. 3.x), Mistral, Mixtral, Qwen (incl. Qwen3 +
MoE), Phi, Gemma, Falcon, StableLM, plus Whisper, Stable Diffusion/FLUX, and BERT. So "downloadable
from the Hub" != "runnable in candle"; new or exotic architectures lag until someone ports them. For
the long tail, **mistral.rs** (a Rust-native engine built *on* candle) advertises 40+ model families
(text, multimodal, speech, image, embeddings) and tends to add new ones faster - the better choice if
the app wants broad coverage without doing the porting work itself.

**GGUF vs MLX in candle.**

- **GGUF - first-class.** candle implements the llama.cpp quantized types and reads GGUF directly
  (`candle_core::quantized::gguf_file`), on CPU, CUDA, or Metal. Its Metal qmatmul kernels are reported
  competitive with llama.cpp and ahead of MLX on prompt throughput. This is the same quantized-model
  story LM Studio's GGUF engine offers.
- **MLX - not a candle target.** MLX is Apple's separate array framework with its own model format;
  candle does not load MLX models. Running MLX-format weights in Rust means binding to Apple's MLX via
  `mlx-rs`, *not* candle - and the project has already decided (§5, §10; 0050 §3) not to take MLX on as
  a dependency, with candle+Metal covering the Mac-GPU need.

**Rust equivalents to LM Studio's engines.**

| LM Studio engine | Rust equivalent |
| --- | --- |
| **Metal llama.cpp** (GGUF on the Apple GPU) | Pure-Rust: candle's **Metal backend**, or **mistral.rs** (Metal-optimized, built on candle). Or bind straight to llama.cpp: **`llama-cpp-2`** (low-level, tracks the C API), **`llama_cpp`** / **`llm_client`** (higher-level wrappers). |
| **LM Studio MLX** (MLX runtime on Apple Silicon) | **`mlx-rs`** (most active Rust bindings to Apple's MLX, via the official mlx-c API); also `mlx-rust`, `apple-mlx`. A binding layer only - there is no pure-Rust MLX *serving* stack as packaged as LM Studio's. Consistent with the §5/§10 decision, this stays a deferred memo, not a built dependency. |

**Chat rendering: Harmony is already Rust.** LM Studio renders gpt-oss chats with OpenAI's **harmony**
format. The reference implementation is itself written in Rust - the `openai-harmony` crate does the
rendering and parsing (channels for chain-of-thought, tool-call preambles, instruction hierarchy), and
the Python package is just PyO3 bindings over that Rust core. So this piece is not merely available in
Rust, it is *natively* Rust: add `openai-harmony` and the app gets the exact chat-format handling LM
Studio uses, with the heavy lifting in Rust by design.

**Net.** `hf-hub` (download) + candle or **mistral.rs** (run, incl. GGUF on Metal) + **openai-harmony**
(chat rendering) is an LM-Studio-shaped GGUF stack entirely in Rust - consistent with Loom's "native
accelerator behind a trait" posture and reusing the `hf-hub`, candle, and Metal pieces §5 already pulls
in. The one genuine gap is MLX: that path means binding to Apple's MLX through `mlx-rs` rather than
staying in the candle ecosystem, and the project has already chosen not to.

## 14. References

Products and components discussed:

- NotebookLM: [What's new - Video Overviews & Studio (Google)](https://blog.google/innovation-and-ai/models-and-research/google-labs/notebooklm-video-overviews-studio-upgrades/), [2026 update](https://bibigpt.co/en/features/notebooklm-2026-update-explained)
- LM Studio: [Chat with Documents (RAG)](https://lmstudio.ai/docs/app/basics/rag), [MCP servers](https://mcpmarket.com/businesses/lm-studio)
- Gemini RAG: [RAG & grounding on Vertex AI](https://cloud.google.com/blog/products/ai-machine-learning/rag-and-grounding-on-vertex-ai), [MCP with Gemini CLI](https://geminicli.com/docs/tools/mcp-server/)
- Claude Desktop RAG over MCP: [Local RAG with Rust + MCP](https://medium.com/@ksaritek/local-rag-with-rust-and-mcp-private-document-search-for-claude-desktop-6fccb37c024e)
- Embeddings (Rust): [candle](https://github.com/huggingface/candle), [fastembed-rs](https://github.com/anush008/fastembed-rs), [embed_anything](https://docs.rs/embed_anything/latest/embed_anything/), [metal-candle](https://lib.rs/crates/metal-candle), [candle-coreml](https://crates.io/crates/candle-coreml)
- LLM inference (Rust): [hf-hub](https://github.com/huggingface/hf-hub), [mistral.rs](https://github.com/EricLBuehler/mistral.rs), [candle GGUF (`gguf_file`)](https://docs.rs/candle-core/latest/candle_core/quantized/gguf_file/index.html), [candle Metal qmatmul kernels (PR #2615)](https://github.com/huggingface/candle/pull/2615), [llama-cpp-2 / Rust LLM ecosystem](https://hackmd.io/@Hamze/Hy5LiRV1gg), [mlx-rs](https://crates.io/crates/mlx-rs), [openai/harmony](https://github.com/openai/harmony) ([Rust docs](https://github.com/openai/harmony/blob/main/docs/rust.md), [openai_harmony on docs.rs](https://docs.rs/openai-harmony/latest/openai_harmony/))
- STT/TTS: [Best open-source STT 2026](https://www.gladia.io/blog/best-open-source-speech-to-text-models), [Best open-source TTS 2026](https://www.bentoml.com/blog/exploring-the-world-of-open-source-text-to-speech-models), [faster-whisper](https://github.com/SYSTRAN/faster-whisper), [Piper](https://github.com/rhasspy/piper), [Speaches](https://github.com/speaches-ai/speaches)
- Document generation (Rust): [rust_xlsxwriter](https://github.com/jmcnamara/rust_xlsxwriter), [office2pdf](https://github.com/developer0hye/office2pdf), [Carbone SDK (Rust)](https://lib.rs/crates/carbone-sdk-rust)
- Ingestion: [Best PDF parser for RAG 2026 (Docling/Marker/MarkItDown/Unstructured)](https://blazedocs.io/blog/best-pdf-parser-for-rag), [Docling for RAG](https://thenewstack.io/from-unstructured-data-to-rag-ready-with-docling/), [Production RAG in pure Rust](https://rust-dd.com/post/our-first-production-ready-rag-dev-journey-in-pure-rust)
- Mobile/UI (Rust): [Dioxus](https://dioxuslabs.com/), [Tauri 2.0](https://v2.tauri.app/), [Phi-3 on iOS with candle](https://www.strathweb.com/2024/05/running-microsoft-phi-3-model-in-an-ios-app-with-rust/)
- Rust in production (precedent): [1Password](https://serokell.io/blog/rust-in-production-1password), [Signal libsignal](https://github.com/signalapp/libsignal), [Mullvad Rust engine](https://www.techradar.com/vpn/vpn-services/mullvad-vpn-boosts-wireguard-speeds-and-stability-with-new-rust-based-engine)
