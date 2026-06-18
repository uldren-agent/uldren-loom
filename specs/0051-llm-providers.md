# 0051 - LLM / chat providers

**Status:** Partial, provider target with small local LLM support decided by 0062 ·
**Version:** 0.1.0-draft · **Optional capability `providers.llm`.**
**Depends on:** 0050 (embedding providers - shared posture), 0008 (wire protocols - streaming),
0015 (programs may call an LLM under capability), 0007 (bindings). **Relates to:** LEANN
"multi-provider support", the ReAct agent prototype.

A **chat provider** turns a prompt (a sequence of messages) into a completion, behind a small trait so
chat, the ReAct agent, and derived-view summarization never bind to a specific model or vendor. Split
out of 0050 because completions have a different shape than embeddings - **token streaming, tool calls,
and backpressure** - a different consumer, and a different (looser) determinism posture.

> **Scope & status.** This capability began exploratory, but 0062 now decides that both `llm` and
> `text-embedding` model kinds are first-class for acquisition, diagnosis, configuration, and at least
> one release-smoked local runtime. The source tree has a Candle CPU small-LLM activation path for a
> curated Qwen2 model. Large local LLM hosting remains outside the lightweight core target and can be a
> Loom Desktop or managed deployment concern. The remote OpenAI-compatible path (§3) remains valid.

## 1. Why separate from 0050

Embeddings are a pure function `text -> fixed-width vector` whose output feeds the vector index and is
subject to the recompute determinism contract (0050 §5). Completions are an open-ended **stream** of
tokens, optionally interleaved with **tool calls**, are inherently non-deterministic (sampling), and
are **never** stored as synced index bytes. Different shape, different consumer, different rules - so a
separate document.

## 2. The chat trait

```idl
interface ChatProvider {                    // OpenAI-compatible chat/completions shape
  model_id(): string
  complete(messages: List<Message>, opts: ChatOptions): Future<Completion>
  stream(messages: List<Message>, opts: ChatOptions): Stream<Delta>   // token/tool deltas
}

record Message  { role: "system"|"user"|"assistant"|"tool"; content: string; tool_calls?: List<ToolCall> }
record ChatOptions { temperature?: f32; max_tokens?: u32; tools?: List<ToolSpec>; stop?: List<string> }
```

`stream` projects over 0008 (SSE / gRPC stream); `complete` is the buffered convenience over it.

## 3. Implementations (all behind the trait)

- **Remote HTTP - OpenAI-compatible (`/v1/chat/completions`) - the primary path.** A single client
  (base URL + bearer token) covers OpenAI, **Ollama, LM Studio, vLLM, llama.cpp, SGLang, LiteLLM,
  OpenRouter, Groq, DeepSeek, Gemini**. Pure `reqwest` + JSON/SSE, wasm-safe (fetch + `EventSource`).
  This is what the ReAct prototype already uses (token-auth to a local LM Studio).
- **In-process `candle` (optional, native).** A local LLM (e.g. a small quantized Llama/Phi) for
  offline/no-server use, behind the same trait. Heavier and slower than remote; opt-in. On Apple
  hardware, 0062 owns optional MLX execution and later Core ML support as separate runtime profiles.
  These remain local choices because completions are never synced.

## 3.1 Local model sourcing (where the in-process model comes from)

The in-process (`candle` / Apple) path needs a *model*; Loom supports sourcing it several ways, selected by
capability detection plus explicit config and reported in `capabilities()`. In rough precedence:

- **(a) Apple on-device models (iOS / macOS).** Use Apple's on-device foundation models where
  available through the future 0062 Core ML runtime path. This is OS-managed, private, **zero
  download**, and keeps model bytes out of Loom.
- **(b) Existing host Hugging Face cache (no download).** Detect models already present in the host HF cache
  (`$HF_HOME` / `~/.cache/huggingface/hub`) and load them in-process. Use what the user already has;
  **never download arbitrary models implicitly.**
- **(c) Small-model fetch via `reqwest` (browser-capable).** On demand, download a **small/quantized** model
  from the HF Hub with `reqwest` (wasm-safe fetch), bounded by a size cap so it can run in-process or even
  inside a **browser** (`wasm32`) build.
- **(d) Full download queue (host, large models).** A managed, resumable background **download queue** for
  large models to the host filesystem (progress, integrity, dedup against the cache), for native/desktop
  where disk and RAM permit - the path most aligned with a Loom Desktop app.

All four feed the same `ChatProvider` trait (§2); the remote HTTP path (§3) remains the default everywhere
and is unaffected. Downloaded model bytes live on the host/cache, **never in the object store or sync** (like
endpoints/keys, §4). Embeddings (0050) SHOULD reuse the same sourcing ladder for its integrated model.

## 4. Configuration & secrets

Endpoints/keys are environment/flag config, never stored in data: `LOOM_LLM_*` (base URL, token,
model). Tokens never enter the object store or sync. Selection is explicit and reported in
`capabilities()`.

## 5. Streaming & backpressure (0008)

`stream` yields `Delta`s (text chunks and/or tool-call fragments) as an async iterator across the ABI
(0007 §2.3): `loom_iter_next` on the native side, `AsyncIterable`/`EventSource` in bindings. The
consumer drives the pace; the provider MUST honor cancellation (drop = stop the upstream request) so a
caller that stops reading does not leak an open completion.

## 6. Parity (0032)

- **Remote HTTP:** identical on native and web (fetch + SSE). The recommended default everywhere.
- **In-process candle LLM:** native only in practice (model size); the browser uses the remote path.

## Open questions

1. **Determinism expectation for completions.**
   - **Context.** Unlike embeddings, completions are sampled and vary run to run; no part of the system
     stores them as synced bytes, but callers (tests, derived views) may want repeatability.
   - **Example.** A derived-view trigger summarizes a document via the LLM; re-running it yields a
     slightly different summary, so the derived artifact's digest changes even though nothing upstream did.
   - **Options.** (a) treat completions as inherently non-deterministic; derived artifacts that embed LLM
     output are *not* content-stable and are recomputed, not synced; (b) require `temperature = 0` +
     pinned model for any completion whose output is persisted; (c) persist the prompt + model_id and
     treat the output as a cache, recomputable but not authoritative.
   - **Recommendation.** (c) - store the prompt+model as the stable input and treat the completion as a
     recomputable cache, so a persisted summary is reproducible-enough and never a sync source of truth.
2. **Tool-call schema.**
   - **Context.** The ReAct agent and programs (0015) need a tool-calling contract, but vendors differ
     (OpenAI `tools`, others `functions`, others text protocols).
   - **Example.** A program written against OpenAI `tools` calls a provider that only speaks the older
     `functions` shape, and the tool call silently fails to parse.
   - **Options.** (a) normalize to the OpenAI `tools` shape at the trait boundary and adapt per vendor;
     (b) expose raw vendor shapes and make callers branch; (c) define a Loom-native tool schema and map
     both ways.
   - **Recommendation.** (a) - normalize to the most widely-supported shape at the boundary, matching the
     embedding side's "one canonical shape, adapt per vendor" posture.
3. **Async shape across the ABI.**
   - **Context.** 0007 §2.2 offers callback and poll async forms; streaming completions must pick one
     per binding for deterministic cancellation tests (mirrors 0007 OQ4).
   - **Example.** A binding that exposes both forms needs two cancellation paths for one stream.
   - **Options.** (a) callback/iterator form everywhere except wasm, which uses the poll form; (b) both
     forms in every binding; (c) poll everywhere.
   - **Recommendation.** (a) - consistent with 0007's recommended async resolution.
4. **Is a local LLM first-class in the data platform? [RESOLVED by 0062 DP3.]**
   - **Context.** Embeddings (0050) are intended first-class, but a first-class *LLM* in the core (vs a Loom
     Desktop app) is undecided; model size is the crux. (Owner still deciding; recorded here.)
   - **Example.** A 70B model cannot run in-process in a browser or on a phone, but a 1-3B quantized model
     can - so "LLM in the platform" is really a question of *which size class*.
   - **Options.** (a) no first-class LLM in core, remote HTTP (§3) only, large/local LLMs are a Loom
     Desktop concern; (b) **small models only** first-class in core (sourcing §3.1 a-c), large models
     deferred to Desktop (§3.1 d); (c) all sizes first-class in core via the download queue (§3.1 d).
   - **Decision.** (b) with 0062 coverage: small local LLMs are first-class for the curated tier,
     including download, doctor, instance configuration, and at least one source-backed local runtime.
     Large-model hosting remains a Desktop or managed deployment concern.
5. **Sourcing precedence & the small/large size threshold.**
   - **Context.** §3.1 lists four sources; the selection rule and the size cap that routes (c) small-fetch
     vs (d) download-queue need defining.
   - **Options.** (a) fixed precedence (Apple → cache → small-fetch → queue) with a built-in size cap; (b)
     explicit per-deployment config only; (c) capability-detected with an overridable default cap.
   - **Recommendation.** (c) - detect what's available, apply a sane configurable default cap to route (c)
     vs (d), and report the resolved source in `capabilities()`.
