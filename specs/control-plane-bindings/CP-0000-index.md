# CP-0000 - Control-Plane Bindings: Index, Architecture & Plan

**Series:** Control-plane bindings (normative-track sub-series; Draft)
**Version:** 0.1.0-draft - **Status:** Draft - foundation/scoping doc
**Last updated:** 2026-06-18
**Reads first:** [`../facet-bindings/P9-0001-binding-architecture.md`](../facet-bindings/P9-0001-binding-architecture.md)
and [`../facet-bindings/P9-0002-projection-conventions.md`](../facet-bindings/P9-0002-projection-conventions.md)
(the conventions this sub-series inherits), specs **0015** (exec), **0026/0027/0028** (identity/acl),
**0029** (triggers), **0030** (observability/watch), **0050/0051** (providers).

This is the **control-plane** analog of `facet-bindings/` (the P9 data-facet binding plan). It plans how
Loom's capabilities that are *operations on / around* the store - rather than data **shapes** - are exposed
over the wire and to AI agents. It was opened by the 2026-06-18 review
([`facet-bindings/P9-0000` §2b](../facet-bindings/P9-0000-index.md)), which noted two of these (`exec` over
MCP, `watch` as a streaming change-subscription) are arguably higher agent-value than several data adapters.

> **Why this folder isn't called "P13".** These capabilities span **three** build phases - P11 (`exec`,
> 0015), P12 (`identity`/`acl`, 0026-0028), and P13 (`trigger`/`watch`, 0029/0030) - plus the provider
> specs (0050/0051). A single phase number undersells the scope, so the folder is `control-plane-bindings`
> and docs use the **`CP-`** prefix. Same status posture as P9: normative-track **Draft**,
> promoted per decision; design-ahead, not a build queue.

## 1. Scope - the control-plane capabilities

| Capability | Facade (spec) | Phase | Build state | Binding character | Reference landscape |
| --- | --- | --- | --- | --- | --- |
| `exec` / `program` | `Exec` and Program lifecycle (0015) | P11 | **partial** (`engine=wasm` raw `exec_cbor` source-backed; Program lifecycle and `engine=cel` target) | stored program authoring plus request + **streamed logs**; the agent "store logic" and "run logic" tools | WASM runtimes, CEL/OPA policy engines, AWS Lambda / CF Workers, SQL stored procs |
| `watch` | `Watch` (0030) | P13 | spec-only | **streaming subscription** (cursor `poll`/`stream`); the agent change-feed | Debezium/CDC, Postgres logical replication, Mongo change streams, Kafka Connect |
| `trigger` | `Triggers` (0029) | P13 | prototype (keeper) | CRUD + `fire_now` + `history`; reactive automation | cron, Airflow, GitHub Actions, webhooks, DB triggers, Temporal |
| `identity` | principals (0026) | P12 | spec-only | principal store + authn; **foundation for all auth** | OIDC/OAuth2, Keycloak, Auth0, LDAP/AD |
| `acl` | `Acl` (0027/0028) | P12 | **partial** (grant grammar in `loom-compute`) | grant/role management; **foundation for all writes** | RBAC, ABAC, Google Zanzibar, OpenFGA, OPA/Rego, AWS IAM |

> **AI providers broken out to `ai/`.** The AI provider facades - `providers.embedding` (0050) and
> `providers.llm` (0051) - are **not** control-plane (they are external-service facades, not store control).
> They move to a separate `ai/` folder (prefix `AI-`) and are written in **Batch D**. Listed here only for
> the cross-reference. (0050 = integrated/downloadable embedding model, intended first-class; 0051 = LLM,
> small-models-leaning, with multi-source model loading per the enhanced 0051 §3.1.)

**Already covered / out of scope here.** `sync` (0006) is **already wire-bound** by spec 0008 §7 (the
Shuttle transport over gRPC/HTTP/WebSocket/bundle) - no new binding doc, just that pointer. `e2e-sync`
(0031) is a **storage transform over sync**, not a binding surface. Both are noted for completeness.

## 2. How control-plane bindings differ from facet bindings

These inherit `facet-bindings/P9-0002` (IDL to REST/JSON-RPC/gRPC/MCP, error mapping 0008 §6, auth transport
0008 §5) but with four deltas:

1. **Operation-oriented resource roots** use capability names such as `/exec`, `/watch`, `/triggers`,
   `/identity`, `/acl`, `/embeddings`, and `/chat`. A workspace selector is a parameter where relevant,
   such as `exec.dry_run(ns, ...)` or `watch.subscribe(sel, ...)`.
2. **Streaming-first.** `watch` (change feed) and `exec` (logs) are inherently streamed - gRPC server
   streams, SSE/WebSocket, and MCP streaming - not request/response. `watch` is the marquee case: a
   resumable cursor (`subscribe`/`poll`/`stream`, `CURSOR_INVALID` on GC).
3. **"Foreign protocol" (Tier-2) means different references.** `identity` maps to OIDC/SAML/LDAP;
   `acl` maps to OPA/Rego or Zanzibar/OpenFGA; `trigger` maps to cron/webhooks/DB-triggers; `watch`
   maps to CDC (Debezium-style). The AI `providers`, now in the `ai/` folder, use an OpenAI-compatible
   HTTP API.
4. **Auth is foundational, not peer.** `identity` + `acl` *are* the security model every other binding,
   including all of `facet-bindings/` and the MCP write-gating (P9-0016 §3), depends on. They are
   prerequisites, so although they sit in the middle of the agent-value ranking, they gate everything that
   writes.

Per-facet **fidelity** and **reference landscape** are folded into each CP doc (lighter than P9's separate
`REFERENCE-IMPLEMENTATIONS`/`IMPLEMENTATION-FIDELITY` docs); split them out only if they grow.

## 3. Files & sections to create (the plan)

| Doc | Scope | Batch |
| --- | --- | --- |
| `CP-0000-index.md` | this scoping/architecture/plan doc | A (done) |
| `CP-0001-reference-landscape.md` | reference impls + standards + connection methods + Loom posture per capability | A (done) |
| `CP-0002-exec-binding.md` | `program` lifecycle plus `exec` (put CEL/WASM, dry_run/apply; streamed logs; MCP store/run-logic tools) | **B** |
| `CP-0003-watch-binding.md` | `watch` (cursor subscribe/poll/stream; the agent change-feed; CDC Tier-2) | **B** |
| `CP-0004-identity-binding.md` | `identity` (principal store, authn; OIDC/LDAP Tier-2) | C |
| `CP-0005-acl-binding.md` | `acl` (grant/role mgmt; OPA/Zanzibar reference) | C |
| `CP-0006-trigger-binding.md` | `trigger` (binding CRUD, fire_now, history; cron/webhook Tier-2) | D |
| `ai/AI-0000` + `ai/AI-0001-providers-binding.md` *(new `ai/` folder)* | `providers.embedding` + `providers.llm` live outside CP; OpenAI-compatible Tier-2 + local model sourcing (0051 §3.1) | D |

(`CP-0001` is the **reference landscape** doc; per-capability *fidelity* notes stay folded
into each binding doc's "Loom posture" row.)

Each CP doc follows the `facet-bindings/P9-0002` §11 template with the §2 deltas: **facade surface** (cite
spec), **Tier-1** REST/JSON-RPC/gRPC/MCP (note streaming), **Tier-2** foreign adapter (the control-plane
reference), **auth / errors / parity / concurrency**, and **resolved decisions or open questions**.

## 4. Batch sequence

- **Batch A (this doc):** scope, architecture deltas, file/section plan.
- **Batch B - `program` + `exec` + `watch`:** the agent-relevant gap the review flagged; highest
  near-term value because agents need to store interpreted CEL or uploaded WASM programs before running
  them, and then observe changes through watch.
- **Batch C - `identity` + `acl`:** the auth foundation; gates writes everywhere (incl. `facet-bindings`).
- **Batch D - `trigger` (CP) + AI `providers` (in `ai/`):** reactive automation, plus the embedding/LLM
  provider surface broken out into the `ai/` folder per CP-RD3.

Same design-ahead caveat as P9: these depend on their facades existing (P11/P12/P13). `exec` read paths
and `watch` are the closest to a near-term MCP slice.

## 5. Resolved Decisions

- **CP-RD1 - folder/prefix.** Use `control-plane-bindings` and `CP-` because the set spans
  P11/P12/P13.
- **CP-RD2 - identity and acl document split.** Keep identity and ACL in two docs, cross-linked, because
  0026 owns authentication and principals while 0027/0028 own authorization.
- **CP-RD3 - providers location.** AI providers are external-service facades, not store control.
  `providers.embedding` (0050) and `providers.llm` (0051) live in the separate `ai/` folder.
- **CP-RD4 - landscape and fidelity.** The reference landscape is its own doc,
  [`CP-0001-reference-landscape.md`](./CP-0001-reference-landscape.md). Per-capability fidelity stays
  folded into each binding doc's "Loom posture" row.
