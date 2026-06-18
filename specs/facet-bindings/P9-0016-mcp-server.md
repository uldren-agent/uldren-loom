# P9-0016 - MCP Server (`loom mcp`)

**Series:** P9 binding plan (normative-track sub-series; Draft)
**Version:** 0.1.0-draft · **Status:** Draft · **Last updated:** 2026-06-18
**Reads first:** [`P9-0002-projection-conventions.md`](./P9-0002-projection-conventions.md) §5, **0013 §B/§C**
(`rmcp`, `loom mcp`), **0008 §7** (served write authority), **0026-0028** (principal and grant
model), the per-facet docs' §5 (tool inventories).

The cross-facet consumer that exposes an open `.loom` to AI agents as **MCP tools + resources**, so an
agent treats a `.loom` as versioned, queryable, syncable memory/workspace. It is the **cheapest first win**
(0013 sequencing): a thin `rmcp` adapter over the same interface (0003), no core changes.

## 0. Binding Boundary

MCP is a native projection and consumer over Loom facets, not a storage facet. It does not own base data
models, physical formats, or derived artifacts. Each tool delegates to the owning facet facade and engine
PEP. Remote MCP serving is a hosted promotion problem; local stdio MCP is a local adapter.

## 1. Launch & transport

- **Binary:** `loom mcp` (or `loom serve --mcp`) over the open `.loom` (0013 §C), built on **`rmcp`** (the
  official MCP Rust SDK, pluggable transport).
- **Transport:** **stdio** by default (local agent process); optional HTTP/SSE for remote, using the
  same transport authentication as 0008.
- **Engine:** one shared engine instance, **single serialized writer** (P9-0002 §10; `1000-Deferred.md` §0013).

## 2. Tool & resource model

- **Tools** are named `‹facet›.‹verb›`, schemas generated from the IDL (P9-0002 §1/§5), identical in
  behavior to the other projections.
- **Resources** expose stable readable entities (an object by digest, a file by path, a table, a commit).
- Per-facet tool inventory (read = always on; write = principal-authorized, §3), aggregated from the facet docs:

| Facet | Read tools | Write tools (principal-authorized) |
| --- | --- | --- |
| `files` | `files.read/stat/list/walk` | `files.write/create/delete/move/copy/mkdir` |
| `vcs` | `vcs.log/diff/show` | `vcs.commit/branch/checkout/merge/updateRef` |
| `cas` | `cas.get/has/list` | `cas.put/delete` |
| `sql` | `sql_query`(SELECT-only)`/describe/listTables/diffRows/blame` | `sql.exec/createTable/dropTable/alterTable/mergeTable/begin` |
| `kv` | `kv.get/scan` | `kv.put/delete` |
| `document` | `document.get/ids/find` | `document.put/delete` |
| `columnar` | `columnar.scan/columns/rows/select` | `columnar.create/append` |
| `time-series` | `timeseries.get/range/latest/list_collections` | `timeseries_put` |
| `queue` | `queue.get/range/len/consumer_position/consumer_read` | `queue.append/consumer_advance/consumer_reset` |
| `vector` | `vector.search/get/scan` | `vector.upsert/remove` |
| `search` | `search.query/get/ids` | `search.create/index/delete/remap` |
| `graph` | `graph.get_node/get_edge/neighbors/out_edges/in_edges/reachable/shortest_path` | `graph.upsert_node/remove_node/upsert_edge/remove_edge` |
| `ledger` | `ledger.get/head/len/verify/list_collections` | `ledger_append` |

This table tracks the source-backed local MCP tool names where source exists. Hosted remote MCP
promotion remains gated by the 0008 authentication path and engine PEP.

## 3. Write safety

- **No ambient remote writes.** A remote MCP transport uses the same authentication and authorization
  contract as 0008: resolve a principal first, then call the engine PEP for each operation.
- **Local stdio authority.** A local stdio session runs as the opening owner in owner mode, or as a
  resolved principal in authenticated mode. Launch-time session selection is allowed for local tooling;
  per-call tokens are not part of the target surface.
- **Tool visibility is ergonomic only.** Write and ref-advancing tools MAY be hidden from `tools/list`
  when the current principal cannot use them, but every write call is still checked by the engine. The
  highest-risk tools (`vcs.updateRef`, `sql_exec` DDL) require the relevant write/ref-advance grant on
  the target.
- **Run-as.** Operations execute as the resolved principal and are auditable through the same model as
  REST/gRPC writes.

## 4. Errors

MCP tool errors carry the stable Loom `code` (0008 §6) in the structured error payload, so an agent can
branch on `code` exactly as REST/gRPC clients do. `UNSUPPORTED` is returned for a tool whose facet
capability is absent (0010 §4); `PERMISSION_DENIED` is returned for an unauthorized write.

## 5. Parity & concurrency

- **Parity (0032):** the `rmcp` server is **native-only** (it is a server process); the underlying facet
  reads/writes have their own parity (e.g. `search_index` native-only). On `wasm32` there is no `loom mcp`
  server - agents reach a browser Loom through the JS binding directly.
- **Concurrency:** all tools share the single-writer engine (P9-0002 §10).

## 6. Resolved decisions

1. **MCP authority source.** Stdio MCP runs as the local owner in owner mode or a resolved principal in
   authenticated mode. Remote MCP uses the same transport authentication as 0008. Per-call token
   arguments are rejected for the target surface because they leak authority into tool schemas and logs.
2. **Remote transport default.** Ship stdio first. HTTP/SSE is permitted only when it reuses the 0008
   authentication path and the 0027 engine PEP. Remote MCP never grants writes merely because a tool is
   listed.
