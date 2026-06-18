# AI-0001 — `providers` Binding (embedding + LLM)

**Series:** AI provider bindings (normative-track sub-series; Draft)
**Version:** 0.1.0-draft · **Status:** Draft · **Last updated:** 2026-06-18
**Reads first:** [`AI-0000-index.md`](./AI-0000-index.md),
[`../facet-bindings/P9-0002-projection-conventions.md`](../facet-bindings/P9-0002-projection-conventions.md),
facade specs **0050** (`EmbeddingProvider`) and **0051** (`ChatProvider`, incl. the enhanced §3.1 model
sourcing).

Both provider facets share a trait shape and an **OpenAI-compatible** wire form, so one binding doc covers
them. Embeddings are deterministic and feed the `vector` facet; completions are streamed and
non-deterministic (0051 §1).

## 1. Facade surfaces

- **`EmbeddingProvider` (0050):** `model_id() → string`, `dimension() → u32`, `embed(texts: List<string>) →
  List<Vector>` (batched). `model_id`/`dimension` are part of a `vector` workspace's identity (0050 §4 /
  0017 §RD1).
- **`ChatProvider` (0051):** `model_id() → string`, `complete(messages, opts) → Completion`, `stream(messages,
  opts) → Stream<Delta>` (token/tool deltas). OpenAI-compatible message/options shape (0051 §2).

**Build state:** partial - the core `EmbeddingProvider` seam exists for embeddings, while in-process
`candle`, OpenAI-compatible HTTP, MLX, provider binding projection, and the local model sourcing ladder
(0051 §3.1: Apple on-device, existing HF cache, `reqwest` small-fetch, host download queue) remain target
integration work.

## 2. Tier-1 · REST (OpenAI-compatible shape)

| Method | HTTP |
| --- | --- |
| `embed` | `POST /embeddings {input}` → `{data: [{embedding}]}` (OpenAI `/v1/embeddings` shape) |
| `complete` | `POST /chat/completions {messages, …}` → `Completion` |
| `stream` | `POST /chat/completions {…, stream: true}` → **SSE** `Delta`s (OpenAI streaming shape) |
| `model_id` / `dimension` | `GET /models` / `GET /embeddings/model` (capability/identity) |

Using the OpenAI route shapes makes Tier-1 and Tier-2 (§6) nearly the same surface.

## 3–4. JSON-RPC / gRPC

JSON-RPC: `ai.embed`, `ai.complete`, `ai.stream` (`*.next`/`*.end` for deltas). gRPC: `Embed` unary
(batched), `Complete` unary, `Stream` server-streaming (deltas, with cancellation = stop upstream, 0051 §5).

## 5. Tier-1 · MCP

- **`ai.embed`, `ai.complete`, `ai.stream`** are the agent's access to embedding/LLM through Loom. They do
  **not mutate stored data**, so they are **read-class** — but they **consume compute/cost**, so they are
  gated by a **provider/compute capability + budget** rather than a data-write token (CP-style budget, cf.
  `exec` CP-0002 budgets). This lets an operator cap an agent's spend.

## 6. Tier-2 · foreign adapter — OpenAI-compatible (both directions) + local sourcing

- **Consume (primary, exists):** a single `reqwest` client (base URL + bearer) speaks OpenAI-compatible
  `/v1/*` to OpenAI, Ollama, LM Studio, vLLM, llama.cpp, Groq, DeepSeek, Gemini, OpenRouter (0051 §3) —
  wasm-safe (fetch + `EventSource`).
- **Serve (optional, high-value):** Loom **exposes** the OpenAI-compatible `/v1/embeddings` +
  `/v1/chat/completions` endpoints (§2), so existing **OpenAI SDK clients** point at a `.loom` and get its
  configured provider. (Scope question AI-OQ1.)
- **Local model sourcing (0051 §3.1):** Apple on-device (iOS/macOS), existing HF cache (no download),
  `reqwest` small-model fetch (browser-capable), and a host download queue for large models.
- **Fidelity ceiling:** the OpenAI **subset** Loom serves (embeddings + chat completions, streaming, tool
  calls normalized per 0051 OQ2) — not the full OpenAI API (no fine-tuning, assistants, batch, etc.).

## 7. Errors / parity / concurrency / determinism

- **Errors:** provider/transport errors map to the core taxonomy (0008 §6); a missing/oversized local model
  → `UNSUPPORTED`/`QUOTA_EXCEEDED`.
- **Parity (0032):** remote HTTP is identical native/web (0051 §6); in-process `candle` is native in
  practice (large models), with small models reachable in the browser via the `reqwest` small-fetch path
  (0051 §3.1c). Apple on-device is `ios`/macOS only.
- **Determinism:** embeddings obey the recompute determinism contract (0050 §5) and feed the synced
  `vector` index; **completions are never a sync source of truth** (0051 §1) — a persisted summary stores
  prompt + `model_id` as the stable input and treats the output as a recomputable cache (0051 OQ1).
- **Secrets:** endpoints/keys are env/flag config, never stored or synced (0051 §4); downloaded model bytes
  live on host/cache, never in the object store.

## 8. Open Questions

### AI-OQ1 — Does Loom *serve* an OpenAI-compatible endpoint, or only *consume* one? (open)

- **Context.** §6 notes Loom can both consume upstream providers and expose its own OpenAI-compatible
  endpoint. Serving turns a `.loom` into a drop-in `/v1` backend for any OpenAI SDK client; consuming is the
  already-built path.
- **Options.** (a) **consume only** (current state); (b) **also serve** `/v1/embeddings` + `/v1/chat/
  completions` over the configured provider (high "it just works" value, esp. with a local model); (c)
  serve only for embeddings (deterministic, feeds `vector`), not chat.
- **Recommendation.** (b) also serve — it is cheap given the routes already match (§2) and makes a `.loom`
  with an integrated embedding model (0050, first-class) usable by any OpenAI-SDK client; gate it behind the
  provider capability + budget (§5).

### AI-OQ2 — Split embedding and LLM into separate binding docs? (open)

- **Context.** They share a doc now; the LLM side (model sourcing 0051 §3.1, the first-class-LLM question
  0051 OQ4) may grow.
- **Recommendation.** Keep one doc until the LLM sourcing/first-class work is decided; split into `AI-0002`
  if it does (CP-OQ4-style fold/split call).
