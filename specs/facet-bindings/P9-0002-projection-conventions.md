# P9-0002 - Shared Projection Conventions for Per-Facet Bindings

**Series:** P9 binding plan (normative-track sub-series; Draft)
**Version:** 0.1.0-draft
**Status:** Draft
**Last updated:** 2026-06-18
**Reads first:** [`P9-0001-binding-architecture.md`](./P9-0001-binding-architecture.md), spec **0008**
(wire protocols), **0007** (bindings/ABI), **0009 §6** (authorization), **0014** (workspaces),
**0032** (platform parity).

Every `P9-00NN-<facet>-binding.md` doc states only its **deltas** over the rules here, so the spec-0008
projection model is written once, not thirteen times. This doc is the contract those deltas extend, plus
the **per-facet doc template** (§11). It is the hybrid layout's shared half.

## 1. The binding pipeline (Tier 1)

A facet becomes reachable by adding its **facade** (its normative spec's `interface` block) to the Loom
**IDL** (0003 §3) and letting 0008's generators project it. Per 0008 §2, the IDL compiles to a Protobuf
schema (gRPC), an OpenAPI 3.1 document (REST), a JSON-RPC manifest, and the C ABI + per-language types
(0007 §3); **CI fails if generated artifacts are stale**. So the authoritative work in each facet doc is:

1. Transcribe the facade methods from the facet's spec into IDL method signatures (the doc cites the spec
   section; it does not invent methods).
2. State the **REST**, **JSON-RPC**, **gRPC**, and **MCP** projections of those methods, as *deltas* from
   the defaults in §§2-5 below.
3. State the facet's **Tier-2** adapter, or record that none exists.

Behavior and the error set are **identical across all four native projections** (0008 §1) - a difference
in any projection is a bug, not a feature.

## 2. REST conventions (default; 0008 §3)

- **Resource root.** Each facet hangs under a canonical workspaced Loom path:
  `/v1/workspaces/{workspace_id}/{facet_root}`. The hosted listener selects one `.loom` store; the path
  selects the workspace UUID inside that store. Services may offer alias lookup routes, but canonical REST
  resources use UUIDs, matching 0008 section 3.1. Each facet doc gives its `{facet_root}` such as `tree`,
  `objects`, `sql`, `kv`, `documents`, or `vectors`.
- **Verbs.** Reads use `GET`; create-if-absent uses `POST`/`PUT` with `If-None-Match: *`; full replace
  uses `PUT`; delete uses `DELETE`; non-CRUD operations use `POST ...:{verb}` (e.g. `:search`, `:scan`,
  `:verify`, `:merge`),
  matching 0008 §3.3's `:merge`/`:checkout` style.
- **Conditional requests / caching.** Where a result is an immutable content-addressed object, return
  `ETag: "{digest}"` and `Cache-Control: public, max-age=31536000, immutable`; ref/working-tree reads
  return a content-digest ETag for cheap `304`s; ref CAS uses `If-Match: "{old-digest}"` with `412` on
  mismatch (`CAS_MISMATCH`). (0008 §3.4.) Facets without content-addressed reads (e.g. a live query) say so.
- **Streaming & pagination.** List/scan/range/log/walk results stream as `application/x-ndjson`, or page
  via `?cursor=` + `Link: rel="next"` (0008 §3.4). Large binary I/O uses `Range:` with `206` and, for the
  bulk object path, length-prefixed binary by default (0008 OQ7).
- **Idempotency.** Mutating `POST`s accept `Idempotency-Key`; replays return the original result, scoped
  per `(loom, token)` with a bounded TTL and `409` on same-key/different-body (0008 OQ3 recommendation).

## 3. JSON-RPC 2.0 conventions (default; 0008 §4.1)

Method names mirror the IDL 1:1 as `{facet}.{method}` in lowerCamelCase (e.g. `fs.readFile`,
`vector.search`, `ledger.verify`). Params are the IDL structs as JSON; results are the IDL result types.
Batch requests are supported. Streaming uses JSON-RPC **notifications** over a persistent WebSocket
(`{method}.next` / `{method}.end` keyed by a subscription id) or chunked NDJSON. This is the recommended
projection for editor/tooling/scripting embeddings.

## 4. gRPC conventions (default; 0008 §4.2)

Each facet contributes its methods to the generated `loom.proto`. Point operations are **unary**;
list/scan/range/log/walk are **server-streaming**; bulk writes are **client-streaming**; sync negotiation
is **bidirectional** (0006 §7). IDL structs (`Digest`, `Stat`, `Row`, `Hit`, etc.) become protobuf messages.
Errors travel in `Status.details` as a `LoomError` message plus a fixed Loom-code-to-canonical-gRPC-status
mapping (0008 OQ8 recommendation) so gRPC-native clients branch correctly.

## 5. MCP conventions (default; 0013 §B/§C)

- **Server.** `loom mcp` (an `rmcp` adapter) exposes the open `.loom`. Fully specified in
  `P9-0016-mcp-server.md`; each facet doc lists only its **tools/resources**.
- **Tools** are named `{facet}.{verb}`, with JSON schemas generated from the same IDL. Read-shaped methods
  become MCP **tools** and, where they name a stable entity, **resources** (e.g. an object by digest, a
  table by name).
- **Write safety (resolved, P9-0001 OQ5).** The MCP server is **read-only by default**; any
  state-mutating or ref-advancing tool is hidden unless the session presents an explicit **capability
  token** (0009 §6). No ambient writes, ever. Each facet doc marks which of its tools are write tools.

## 6. Authentication & authorization (default; 0008 §5, 0009 §6, 0027/0028)

- **Transport.** REST/JSON-RPC-over-HTTP: `Authorization: Bearer <token>` (capability or OIDC) or mTLS;
  gRPC: per-RPC bearer metadata and/or channel mTLS; WebSocket: token in the upgrade, re-auth on
  reconnect.
- **Scopes.** A token's grants map to **workspace + ref-glob + path/key prefix + facet + mode**
  (read/write/advance/merge/exec), per the fine-grained model (0009 §6; 0027/0028). Each facet doc states
  what its operations require (e.g. `search.index` needs write on the workspace; `cas.get` needs read).
- **Existence hiding.** Default posture (0008 §5 / OQ4-of-0008): `403`/`PERMISSION_DENIED` for named
  resources (refs/trees/tables) whose existence is not itself sensitive; `404`/`NOT_FOUND` for
  objects-by-digest, where the digest is a capability-like secret.

## 7. Error mapping (default; 0008 §6)

Reuse 0008 §6's normative `code` to HTTP and JSON-RPC table verbatim; bodies always carry the machine `code`.
A facet doc lists **only** the codes it adds beyond the core set, e.g. `DATASET_NOT_FOUND` (0023,
columnar), a ledger `verify`-mismatch code (0018), `UNSUPPORTED` when the capability is absent (0010 §4).
New codes must be registered in the 0003 §8 taxonomy, not minted ad hoc in a binding doc.

## 8. Workspace Addressing

Resolved by the owner and folded into 0008: each hosted listener serves one `.loom` store, and REST uses
`/v1/workspaces/{workspace_id}/...` as the canonical per-store resource root. gRPC and JSON-RPC pass
`ns: NsSelector` as a normal parameter. Facet docs treat the canonical workspace UUID path as fixed;
alias routes are service conveniences, not canonical identity.

## 9. Platform parity (default; 0032)

Each facet doc names any surface that is unavailable or degraded on `wasm32`, citing 0032's per-mismatch
section: e.g. `search` indexing (tantivy threads) is native-only; HNSW acceleration (`vector`) is
native-only; FUSE/foreign-protocol *servers* are native-only by nature. The Tier-1 **query/read** paths
are expected to be portable; a facet doc states the degraded fallback (e.g. `search` query-only with
linear membership in the browser, 0033/0032 §4.8).

## 10. Concurrency & multi-process (default; 0005 §6.4, `1000-Deferred.md` §0013)

When several surfaces (REST + MCP + a Tier-2 adapter) serve one `.loom`, they share **one engine instance
with a single serialized writer** (the deferred concurrent-adapter recommendation); a facet doc need only note any
facet-specific writer constraint (e.g. `queue`/`ledger` single-writer-per-stream via ref CAS, 0021/0018).

## 11. Per-facet doc template

Each `P9-00NN-<facet>-binding.md` uses this skeleton (omit a section only with an explicit "n/a - reason"):

```
# P9-00NN - <facet> Binding
Status / version / reads-first (the facet's facade spec + this doc)
§1 Facade surface        - the methods, transcribed from <facade spec> (cite section), with build status
§2 Tier-1 REST         - facet-root, per-method verb/path table, ETag/streaming notes (deltas from P9-0002 §2)
§3 Tier-1 JSON-RPC     - method list (deltas from §3); usually "1:1, nothing special"
§4 Tier-1 gRPC         - service/RPC list + which are streaming (deltas from §4)
§5 Tier-1 MCP          - tools/resources + which are write-tools (deltas from §5)
§6 Tier-2 foreign adapter - the reference protocol, concept mapping, fidelity ceiling, capability gate; or "none - reason"
§7 Errors / parity / concurrency - facet-specific codes (§7), wasm degradation (§9), writer constraint (§10)
§8 Resolved decisions or open questions - Context, Example, Options, Recommendation
```

The point of the template is that a reader can diff two facet docs and see exactly where they differ,
because everything common lives here.
