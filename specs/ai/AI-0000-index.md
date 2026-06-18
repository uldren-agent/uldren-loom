# AI-0000 — AI Provider Bindings: Index

**Series:** AI provider bindings (normative-track sub-series; Draft)
**Version:** 0.1.0-draft · **Status:** Draft — folder index
**Last updated:** 2026-06-18
**Reads first:** [`../control-plane-bindings/CP-0000-index.md`](../control-plane-bindings/CP-0000-index.md)
(where these were broken out), [`../facet-bindings/P9-0002-projection-conventions.md`](../facet-bindings/P9-0002-projection-conventions.md)
(inherited conventions), specs **0050** (embedding providers), **0051** (LLM/chat providers).

This folder holds the bindings for Loom's **AI provider** facades. They were **broken out of
control-plane** (CP-OQ3): a provider is an *external-service facade* (turn text → vectors, turn a prompt →
completion), not store control — keeping them separate keeps `control-plane-bindings/` focused on
governing the store.

## Scope & direction (owner, 2026-06-18)

| Facet | Facade (spec) | Direction |
| --- | --- | --- |
| `providers.embedding` | `EmbeddingProvider` (0050) | **Intended first-class** — Loom ships/uses an integrated, downloadable embedding model so the data platform fully functions on its own (feeds the `vector` facet). |
| `providers.llm` | `ChatProvider` (0051) | **Undecided / small-models-leaning** — a first-class core LLM is unsettled; small in-process models via the multi-source ladder (0051 §3.1: Apple on-device · existing HF cache · `reqwest` small-fetch · host download queue), large models likely a **Loom Desktop** concern. |

## Files

| Doc | Scope | Batch |
| --- | --- | --- |
| `AI-0000-index.md` | this index | D |
| `AI-0001-providers-binding.md` | `providers.embedding` + `providers.llm` bindings (Tier-1 + OpenAI-compatible Tier-2 + local model sourcing) | D |

(Embedding and LLM are kept in one binding doc for now since they share the provider-trait + OpenAI-
compatible shape; split into `AI-0002` if the LLM side grows — see AI-0001 §8.)

## Conventions

Inherits `facet-bindings/P9-0002` (IDL → REST/JSON-RPC/gRPC/MCP, error mapping, auth transport). The
provider deltas: streaming completions (SSE/gRPC stream), an **OpenAI-compatible** wire shape on both the
consume and (optionally) serve sides, and **local model sourcing** (0051 §3.1) as the cross-cutting concern.
