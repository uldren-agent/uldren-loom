# P9-0000 - Binding Sub-Series Index And Build Priority

**Series:** P9 binding plan (normative-track sub-series; Draft)
**Version:** 0.1.0-draft - **Status:** Draft companion, reconciled with current hosted surface map.
**Last updated:** 2026-07-13

Index for the **P9 binding plan** - the docs that take Loom's facets from in-process Rust APIs to
wire-reachable surfaces (REST/JSON-RPC/gRPC + MCP) and consumers (FUSE), per phase **P9** of
`IMPLEMENTATION-PLAN.md`. Start at `P9-0001` (architecture) and `P9-0002` (shared conventions); each
`P9-00NN-<facet>-binding.md` states only its deltas.

## 1. Build priority - quickest win -> largest lift

Ordered by **implementation effort to ship a useful binding**, given each facet's current build state and
its blocking findings. Effort: **S** small - **M** moderate - **L** large. "Blockers" reference
`SPEC-RECONCILIATION-FINDINGS.md`.

| # | Doc | Scope | Facet state | Tier-1 | Tier-2 (effort) | Blockers |
| --- | --- | --- | --- | --- | --- | --- |
| - | `P9-0001` | binding architecture | foundation | - | - | - |
| - | `P9-0002` | shared projection conventions | foundation | - | - | - |
| 1 | `P9-0016` | **MCP server** (`loom mcp`, read-only) | built path | **S** | n/a | cheapest first win; agent leverage |
| 2 | `P9-0005` | `cas` | built plus bounded OCI/CAR/S3 subsets | **S** | OCI/S3/CAR (M) | broader conformance and capability rows |
| 3 | `P9-0004` | `vcs` | built | **S** | none (git excluded) | verify cherry_pick, F11 |
| 4 | `P9-0003` | `files` (Tier-1) | built | **S** | S3 (M) | - |
| 5 | `P9-0007` | `kv` | built | **S-M** | etcd/RESP (M) | F3 (key model) |
| 6 | `P9-0010` | `time-series` | built | **S-M** | line-protocol (M, partial) | F4 (range bound) |
| 7 | `P9-0012` | `vector` | built | **M** | Qdrant (M) | F2 (facade text) |
| 8 | `P9-0011` | `queue` | built | **M** | Kafka (M, partial) | F9 (offsets/substrate) |
| 9 | `P9-0017` | **FUSE mount** | built path (RO) | **M-L** | n/a | F11 (RW needs append/offset/symlink) |
| 10 | `P9-0008` | `document` | built (no index) | **M** | REST now / Mongo-wire later | F8 (indexes/`find`) |
| 11 | `P9-0006` | `sql` | partial | **M** | pg-wire/MySQL (L) | F6 (name), F10 (facade methods), OQ-S3 (txn) |
| 12 | `P9-0009` | `columnar` | built plus Arrow/Parquet interchange subset | **M-L** | Arrow Flight/Parquet/analytical SQL profiles | durable segment profile, Flight, and broader client evidence |
| 13 | `P9-0015` | `ledger` | built plus native hosted gRPC proof subset | **M** | native ledger proof/checkpoint profile | witness, transparency, physical retention, and broader proof parity |
| 14 | `P9-0013` | `search` | built plus bounded hosted `fts` profile | **M-L** | OpenSearch REST/NDJSON over `fts` | full analyzer execution, nested/pipeline aggregations, broader certification |
| 15 | `P9-0014` | `graph` | built plus native graph gRPC and bounded Neo4j subset | **M-L** | bounded openCypher/GQL and `neo4j/tcp` | full Neo4j compatibility, graph merge, broader conformance |

**Reading of the order.** This table is now a companion prioritization aid, not the active build queue.
Many hosted and compatibility subsets have landed since the original P9 drafting pass. Use the owning
facet specs and `specs/IMPLEMENTATION-PLAN.md` for current implementation gates.

> **Cross-cutting prerequisite:** promoted hosted write surfaces depend on public principal/access-control
> projection and engine PEP coverage. Local facades and local MCP tools may already be source-backed
> without proving hosted listener readiness.

## 2. Decisions snapshot (all resolved)

OQ1 full Tier-2 ambition - OQ2 hybrid layout - OQ3 all facets in batches - OQ4 workspace **path segment**
(`/v1/workspaces/{workspace_id}/...`) - OQ5 normative-track, promoted per decision; MCP **read-only by default**, writes
token-gated - OQ6 FUSE **both modes** (commit = read-only; tip = read-write or read-only by mount option).
Per-facet OQs carry recommendations in their docs; none blocks drafting.

## 2b. Recommendations & caveats (from the 2026-06-18 review)

- **This is a hosted and compatibility build queue, not a claim that local facades are absent.** Many local
  facades and the local MCP host are now source-backed. Hosted write promotion still depends on the
  public principal/access-control path and engine PEP coverage. Read section 1's priority table as the sequence
  for promoted protocol and compatibility work, not as a denial of local source state.
- **Triage Tier-2; do not build all 13 at once.** Full Tier-2 ambition (OQ1) is the right *plan* but a poor
  *build order*. Prioritize adapters that unlock large, mature client ecosystems - per
  [`REFERENCE-CLIENTS-AND-FORMATS.md`](./REFERENCE-CLIENTS-AND-FORMATS.md) that is **pg-wire (SQL GUIs),
  the time-series HTTP surface (Grafana), Kafka UIs, and OpenSearch (Kibana)**. Fragmented ecosystems
  such as KV should become explicit presentation families, not underdesigned exceptions.
- **Stand up executable conformance before trusting "done."** Capability and release-certification
  reports are source-backed, but every promoted protocol row still needs executable evidence before it
  can become a full compatibility claim.
- **Control-plane facets are out of scope here.** `exec`, `watch`, `trigger`, `identity`, `acl`, and the
  embedding/LLM providers need their own binding plan - and two (`exec` over MCP, `watch` as a streaming
  change-subscription) are arguably higher agent-value than several data adapters. That work lives in
  [`../control-plane-bindings/CP-0000-index.md`](../control-plane-bindings/CP-0000-index.md).

## 3. Companion documents

- [`REFERENCE-IMPLEMENTATIONS.md`](./REFERENCE-IMPLEMENTATIONS.md) - Phase 1: the reference landscape per
  facet (impls, standards, ports).
- [`IMPLEMENTATION-FIDELITY.md`](./IMPLEMENTATION-FIDELITY.md) - Phase 2: fidelity vs reference plus the
  Green, Yellow, Orange, and Red flags and the exposure gap this sub-series closes.
- [`P9-0018-facet-presentation-model.md`](./P9-0018-facet-presentation-model.md) - resolved design
  direction for base facets, native projections, presentation surfaces, physical formats, and derived
  artifacts.
- [`SPEC-RECONCILIATION-FINDINGS.md`](./SPEC-RECONCILIATION-FINDINGS.md) - **temporary** backlog of the 11
  spec/build mismatches the binding work surfaced (run in a session, then delete). **F1** (columnar) is the
  highest-priority build item. Current active gates live in `specs/IMPLEMENTATION-PLAN.md`.

## 4. Status legend

`built` = facet has working local code. `built local` = core plus at least one local projection is
source-backed. `partial` = some facade methods are wired. `bounded subset` = a hosted or compatibility
profile has executable evidence for specific rows only. Current completion state is owned by the
numbered specs and `specs/IMPLEMENTATION-PLAN.md`.
