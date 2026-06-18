# 0067a - MCP Tool/Resource/Prompt Inventory For Remote Loom (Queue 11 task 370)

## Purpose

Task 370 makes `loom mcp <STORE>` serve a remote Loom endpoint. This document is the source-backed
inventory that the implementation is built from: it classifies every MCP tool, resource, and prompt by
whether it can be served over a remote `LoomClient`, and states the intended remote behavior for each.

## Methodology (source-backed)

The MCP tool catalog is `crates/loom-mcp/src/tools.rs` `TOOL_SURFACE`. Each `ToolSpec` already records
the `idl_interface` and `idl_method: Option<&str>` it projects, plus read/write `kind`. That is the
per-tool source of truth used here:

- `idl_method = Some(m)` on a generated `LoomClient` interface -> the tool projects a real IDL method and
  can be forwarded to `RemoteLoomClient`.
- `idl_method = None` -> the tool is a host-level or composite feature with no single IDL method; it uses
  `Loom<FileStore>`/engine APIs directly through `StoreAccess::read`/`write` and has no remote projection.

Resources are `crates/loom-mcp/src/resources.rs` `RESOURCE_TEMPLATES`; prompts are
`crates/loom-mcp/src/prompts.rs` `PROMPT_SURFACE`.

The host store seam is `crates/loom-mcp/src/lib.rs` `StoreAccess` (`PerRequest { Loom<FileStore> }` /
`Persistent(Arc<Mutex<Loom<FileStore>>>)`); every tool runs as a closure over `&Loom<FileStore>` via
`StoreAccess::read`/`read_runtime`/`write`. There is no `LoomClient`/`RemoteLoomClient` seam today, so
remote MCP adds one rather than tweaking wiring.

## Summary

Tools: 356 total.

- 206 IDL-backed unary -> served over remote by forwarding to the generated `LoomClient` method, or (for
  a tool whose remote projection is not byte-lossless) rejected inside its own method with a precise
  current-behavior error. `HANDLE_STREAM_METHODS` is now empty. All counts are derived from `TOOL_SURFACE`
  by the `tools.rs` partition test.
  Shipped + live-verified so far (the per-row "Implement over remote" dispositions below are done for
  these): the KV, CAS, Queue, Ledger, TimeSeries (`get`/`put`/`range`; `latest` is now wired too - task
  398 grew the `TimeSeries.latest` payload to the `[ts, value]` pair, unit-verified, live-verified
  owner-side), Search (full-text `fts_*`), Columnar, Calendar,
  Contacts, Mail, FileSystem (`fs_*`: read/write/append/read-at/write-at/truncate/remove, symlink,
  read-link), and Vector (`vector_*`: create/upsert/upsert_source/get/source_text/embedding_model/ids/
  metadata_index_keys/create+drop_metadata_index/search/search_policy/delete - reads forward the
  server's canonical CBOR unchanged, proven byte-identical to the MCP `facet_cbor` encoders) families,
  plus document reads (`document_get_text`/`document_get_binary`, pure-engine forwards with digest
  projection over the remote store algorithm).
  Document/graph writes that maintain an MCP-host reference index
  (`document.put`/`document.delete`/`document.replace_text`, `graph.upsert_edge`/`graph.remove_edge`) now
  FORWARD over remote (task 395): the combined engine-write + `substrate_refs` overlay was relocated into
  `loom-reference` and both the local host and the remote server dispatch call it via new `*_indexed` IDL
  methods, so a remote write updates the primary facet AND the reference index; the
  `remote_ref_index_write_unsupported` reject is removed.
  Also shipped: the **VCS clean subset** - reads `log`/`status`/`diff`/`blame`/`tag_list`/`tag_target`/
  `merge_in_progress`/`merge_conflicts` (status/blame decode via new `loom_wire::vcs` `status_from_cbor`/
  `blame_rows_from_cbor`; diff is a direct `LMDIFF` forward) and the non-timestamped writes `branch`/
  `checkout`/`stage`/`stage_all`/`unstage`/`tag_delete`/`tag_rename`/`restore_file`/`restore_path`/
  `merge_resolve`/`merge_abort`. The **timestamped VCS writes** (`commit`/`commit_staged`/`tag_create`/
  `merge`/`merge_continue`/`cherry_pick`/`revert`/`rebase`/`squash`) reject over remote via
  `remote_vcs_timestamp_write_unsupported` (the caller `timestamp_ms` has no remote IDL parameter;
  server-time forwarding would change content digests - tracked as queue task 396); `log_async`/
  `merge_async` remain handle/stream-gated.
  Also shipped: the **graph clean subset** - all reads (`get_node`/`get_edge`/`neighbors`/`out_edges`/
  `in_edges`/`reachable`/`shortest_path`/`query`/`explain_query`; the MCP `facet_cbor` graph encoders are
  byte-identical to `loom_wire::graph`, and `query`/`explain_query` call `loom_wire::graph` directly, so
  reads are clean direct forwards) and node writes (`upsert_node`/`remove_node`). Graph **edge writes**
  (`upsert_edge`/`remove_edge`) now forward over remote too (task 395): the host-side substrate ref-index
  overlay was relocated to `loom-reference` and runs server-side via `Graph.upsert_edge_indexed`/
  `remove_edge_indexed`. `document_list_binary` is now shipped too.
  `document_query` is now shipped over remote too (task 397): though it is a host composite (`document_list_binary` +
  `doc_query`/`doc_find` candidate ids + projections + per-item `Digest::hash(store_algo, doc)`), the host
  reassembles it over remote primitives - `document_list_binary` (collection bytes), `query_json`/`find_json`
  (candidate ids), and the new `Store.digest_algo()` (the store algorithm) - with byte parity to a local
  read. The document/graph ref-index writes are shipped too (395, via loom-reference `*_indexed`); vcs
  timestamped writes are shipped (396a/396b). All remote-MCP families are now wired; only owner-side live
  verification remains. (FileHandle is FULLY_FOLDED - no MCP tools - so it is not a pending remote-MCP family.)
- Handle/stream is now classified at the METHOD level, not the interface level (the MCP remote
  capability reflects the actual exposed tool, not the broader IDL family). `HANDLE_STREAM_METHODS` is
  now empty: the three session/stream SQL tools are Unary in the surface too. `sql_exec` opens a
  per-request `SqlSession` inside the backend (open -> exec -> close) and forwards byte-clean `exec_cbor`;
  `sql_query` and `sql_commit` reject inside their own methods with precise, current-behavior errors -
  `sql_query` because the IDL `sql_query` stream yields rows only and drops the `exec_cbor` statement
  labels/structure (task 399), and `sql_commit` because the IDL commit carries no caller `timestamp_ms`,
  so the content-addressed digest would diverge (folded into task 396). The 16 formerly-"handle/stream"
  tools are also unary in the MCP surface:
  Watch `subscribe`/`poll` (server-side cursor), Dataframe `create`/`collect`/`preview`/`materialize`/
  `plan_digest`/`source_digests` (standard session), and the SQL read-side `sql_read_table`/`_at`,
  `sql_index_scan`/`_at`, `sql_diff`, `sql_table_diff`, `sql_blame`, `sql_list_databases` (standard
  session, path-bound reads). Counts are derived from `TOOL_SURFACE` in the `tools.rs`
  `remote_capability_partitions_the_surface` test (not hardcoded). Tranche-1 WIRING: the **SQL-read group
  (all 8 tools) is now WIRED + LIVE-VERIFIED**. Byte-parity holds because the MCP host and the server
  `LocalLoomClient` run the same engine op and encode with the same `loom_sql::result_cbor::*` /
  `lookup_cbor::values_from_cbor`, and the wire `table` argument is `sql_table_path(db, table)`, so the
  backend forwards the unary reply bytes unchanged (`sql_list_databases` decodes the wire `Array(Text)` to
  `Vec<String>` for the host's `read_collections`). The live remote MCP test seeds a two-commit SQL
  history pre-bind and asserts the remote bytes equal a local per-request read for `read_table` (head),
  `read_table_at` (c1 snapshot, distinct from head), `sql_diff`, and `sql_table_diff`, plus
  `sql_list_databases` contains the seeded db and `sql_exec` rejects. The **Dataframe group (all 6 tools)
  is now WIRED + LIVE-VERIFIED** too. The suspected `facet_cbor::dataframe_batch_cbor` vs
  `DataframeBatch::encode` divergence does NOT hold on inspection: both emit
  `[[[name,tag,nullable]],[[cells]]]` over the same `loom_codec` codec and the same shared
  `loom_types::cell_value`, and MCP `digest_strings_cbor` == server `loom_wire::digest_list_to_cbor` (both
  `Array(Text)`), so `collect`/`preview`/`source_digests` forward the reply bytes unchanged;
  `plan_digest`/`materialize` apply a lossless `Digest`->`algo:hex String` transform, and `create`
  forwards the plan write (no host-side augmentation). The live test seeds a CSV-backed frame pre-bind and
  asserts remote==local for collect/preview(1)/plan_digest/source_digests, plus a create-over-remote
  read-back and a materialize write. The **Watch group (both tools) is now WIRED + LIVE-VERIFIED** too,
  after a canonical wire-parity correction: `loom_watch::change_event_cbor` dropped `ChangeEvent.parent`,
  which the MCP `DataChangeSummary.parent` needs, so the event wire form gained a nullable `parent` and a
  symmetric `watch_batch_from_cbor`/`change_event_from_cbor` decoder (legacy no-`parent` payloads decode as
  `None`); the `Watch::poll` IDL signature is unchanged (still returns bytes). `watch_subscribe` resolves
  the workspace name->id via `Workspaces::workspace_list` (the selector wire form carries a `WorkspaceId`)
  and rebuilds the `watch_selector_to_cbor` selector; `watch_poll` reproduces the cursor/workspace guard
  and decodes the batch into the same `WatchBatchSummary`. The live test seeds a two-commit watched Files
  workspace and asserts the remote subscribe cursor and the polled batch (including each event's `parent`)
  match a local read exactly. Finally, `sql_exec` is SHIPPED + live-verified: the loom-mcp suite asserts
  the precise in-method rejections for `sql_query`/`sql_commit`, and the live flow adds a `sql_exec`
  create+insert `exec_cbor` byte-parity check against an independent local store (open -> exec -> close per
  call). The owner ran the live `mcp_kv_round_trip_through_remote_backend` flow green on a dev host (the
  constrained sandbox could not link the test binary - a verification-environment limit, not a code
  blocker). `timeseries_latest` (task 398) and `sql_query` (task 399) are now wired (owner-decided);
  `sql_query` forwards to a new read-only full-result `Sql.sql_query_result`. The only tool still
  rejecting in-method pending a contract decision is `sql_commit` (task 396).
- 149 host/composite (`idl_method = None`). After the 621-625 server-side promotions the split is: 122
  server-executed (promoted; `apps`, `ask` [`ask_questions`/`ask_record`], `drive`, `meetings`, `chat`
  durable tools, `pages`, `spaces`, `structures`, `substrate` incl. `substrate_transact`, `tickets`,
  plus `workgraph_metrics`); 1 client-driven (`ask_answers`, bounded wait polling the server single-shot);
  and 26 local-only that reject a remote locator with a precise unsupported error. Of those 26, 5 are
  permanent-local host-runtime/catalog features (`apps_call_tool`, `chat_presence`, `chat_set_presence`,
  `search`, `studio_reindex`) and 21 are store-backed families not yet assigned a
  promotion task (`lifecycles` x16, `import` x2, `workgraph_changes`/`workgraph_fact_put`,
  `redmine_import_snapshot`) - deferred, still class-3 reject (see "Deferred store-backed MCP projection
  gaps").

Resources: 8 total.

- 5 IDL-backed facet reads (`file`, `cas-blob`, `calendar-event`, `contact-card`, `mail-message`) map to
  `FileSystem.read_file`, `Cas.get`, `Calendar` entry/to_ics, `Contacts.to_vcard`, `Mail.to_eml` ->
  served over remote.
- 3 host/substrate views (`studio-status`, `substrate-view`, `substrate-refs`) -> local-only; reject
  remote.

Prompts: 7 total (`calendar_agenda`, `mail_summarize_thread`, `mail_draft_reply`, `vcs_blame`, `fs_find`,
`sql_schema_overview`, `apps_author`). All are static prompt templates with no store access -> served
identically for local and remote (no adapter needed).

## Remote behavior classes

1. Implemented over remote: IDL-backed unary tools, and the 5 IDL-backed resource reads. This now
   includes the richer-return VCS replay/merge writes (`merge`/`cherry_pick`/`revert`/`rebase` - task 396b)
   and the document/graph ref-index writes (via loom-reference `*_indexed` - task 395).
2. (Formerly) rejected in-method pending further work - now resolved: the items in class 1 above. Everything else
   IDL-backed is served under class 1: the timestamped VCS COMMITS (`commit`, `commit_staged`,
   `tag_create`, `merge_continue`, `squash`, `sql_commit`) gained a `timestamp_ms` IDL param (task 396a);
   `timeseries_latest` grew to `[ts, value]` (398); `sql_query` forwards to the new read-only
   `Sql.sql_query_result` (399); `sql_exec` opens a per-request `SqlSession`; and the SQL-read, Dataframe,
   and Watch groups forward. (396a/398/399 are unit-verified, live-verified owner-side (2026-07-13).)
3. Clear unsupported over remote: the 6 permanent-local host/composite tools, the 21 deferred store-backed
   tools, and the 3 substrate/studio resources - they have no IDL projection and no server-side promotion,
   so they return a precise "not supported against a remote store; run `loom mcp` against a local .loom"
   error. No silent behavior change.
4. Requires server-side projection: no tool in class 1/2 needs new IDL. Class 3 tools are not candidates
   for client-side reconstruction from low-level primitives. A remote Loom-backed tool becomes remote-capable
   only when the server exposes a single authoritative tool/aggregate operation and runs the shared domain
   logic beside the served `Loom<FileStore>`. The client remains a transport/session/UX shim.

## Host/composite family disposition (task 620)

Owner decision: *promote* Loom-backed host/composite families by server-side execution, and classify
non-store host features as permanent-local. This subsection is the per-family disposition of record; the
enforcement is the `remote_capability_partitions_the_surface` test in `crates/loom-mcp/src/tools.rs`,
whose counts are derived from `TOOL_SURFACE` (a tool without an `idl_method` is local-only), so the
partition stays green as each family is promoted. Until a family is promoted, its tools keep returning the
precise "not supported against a remote store" error (class 3).

Projection mechanism: the long-term remote MCP model is a thin client. The local process resolves the
remote, attaches auth/session state, sends the MCP tool/resource request, and renders the response. The
remote server runs the tool implementation beside the served `Loom<FileStore>` and owns authorization,
expected-root checks, workflow validation, reference/index maintenance, operation records, and commits.
Do not grow `RemoteMcpBackend` into low-level substrate primitives and do not reconstruct Loom-backed tool
semantics client-side. Shared domain/service crates remain the DRY implementation seam; the difference is
where they execute for a remote Loom: on the server.

Atomicity rule: remote writes must preserve the local optimistic-concurrency and single-commit guarantees.
If a local operation performs a read-modify-write with an expected root/profile root/VCS commit, the remote
projection must execute that operation server-side with the same compare-and-commit semantics, or it
remains unsupported until that aggregate operation exists. Best-effort and last-writer-wins remote
semantics are not acceptable.

| Family | Underlying facets | Disposition | Notes |
| --- | --- | --- | --- |
| `apps` | FileSystem | Promote by server-side tool execution | App scaffolds are file-tree operations, but the remote target still runs the tool server-side rather than letting a client assemble behavior from FileSystem calls. |
| `ask` | Document | Promote by server-side tool execution | Q&A records are documents; server-side execution preserves one behavior path and avoids client-side policy drift. |
| `chat` | Kv, Document, Queue | Promote by server-side aggregate/tool execution | Channels/messages/cursors compose over Kv+Document+Queue and should execute beside the store. |
| `drive` | FileSystem, Document | Promote by server-side aggregate/tool execution | File storage, metadata, sharing, retention, and leases are domain behavior, not client-side reconstruction targets. |
| `meetings` | Document, Calendar | Promote by server-side aggregate/tool execution | Meeting records, agenda state, extraction review, vocabulary, and merge decisions execute server-side. |
| `pages` | Document, reference, substrate/VCS profile state | Promote by server-side aggregate/tool execution | Source uses workspace snapshots, profile roots, authorization, operation records, and commits. |
| `spaces` | Document, reference, substrate/VCS profile state | Promote by server-side aggregate/tool execution | Source uses the same page workspace/profile-root machinery as pages. |
| `structures` | Document, reference, substrate/VCS profile state | Promote by server-side aggregate/tool execution | Structured records and ticket decomposition require server-side domain execution. |
| `substrate` (views) | Document, reference, substrate/VCS profile state | Promote by server-side aggregate/tool execution | Views and substrate writes are served-store state and must be evaluated on the server. |
| `tickets` | Document, reference, indexed tables, workflow/VCS profile state | Promote by server-side aggregate/tool execution | Ticket lifecycle must preserve workflow validation, authorization, expected-root concurrency, operation records, and indexed table updates. |
| `studio` (`studio-status`) | none (host runtime) | Permanent-local | Reports the host process/runtime, not served-store state; no engine projection. |
| `tools` (meta) | none (catalog) | Permanent-local | Lists the MCP tool catalog itself; a host feature, not a store operation. |

Task 620 is therefore a design-correction task plus a split point, not the implementation container for
every family. Follow-up implementation should be split into bounded server-side projection tasks: first the
remote MCP/tool execution transport for Loom-backed tools, then family aggregate operations and parity
fixtures. A family is Done only after unit coverage and live remote parity prove the server-side path
matches local behavior; sandbox-only `GateTestBackend` coverage is not sufficient for Done.

## Implementation plan

Default target for the task-620 follow-up work:

1. Server execution seam. Add a remote MCP tool/resource execution route to the served Loom endpoint.
   The request crosses the wire as an MCP tool/resource operation plus auth/session context; the hosted
   server resolves the tool, opens the served `Loom<FileStore>` under its normal authority, and runs the
   same domain implementation the local MCP host uses.
   **Landed (task 621, 2026-07-16):** the route is the non-generated `Mcp.call_tool` request handled by
   `RemoteRuntime::dispatch` in `crates/loom-hosted-core/src/remote.rs`, dispatched to an injected
   `McpToolExecutor` (`RemoteRuntime::set_mcp_executor`) under the single write authority with §6
   idempotency dedup and `McpToolContext { session_principal, idempotency_key, deadline_ms }`. The client
   side is `RemoteMcpBackend::execute_tool` (loom-mcp) forwarded by the CLI over `client.call("Mcp",
   "call_tool", [tool_name, args_json])`; `tools::remote_tool_route` gates promoted vs. rejected tools
   from the catalog (`SERVER_PROMOTED_TOOLS`, empty until families promote). loom-hosted-core ferries
   opaque bytes and never depends on loom-mcp. No `McpToolExecutor` is wired into `loom serve remote`
   yet, so promoted-tool requests return `UNSUPPORTED` until task 622 installs the executor.
2. Shared implementation seam. Move any reusable domain logic needed by apps, ask, chat, drive, meetings,
   pages, spaces, structures, substrate, and tickets into crates that both the local MCP host and hosted
   server can call. The shared seam is domain/service code, not a client-side reconstruction layer.
   **Landed (task 622, 2026-07-16):** the low-substrate aggregate families `apps`, `drive`, `meetings`,
   and `ask` are promoted to server-side execution via `uldren_loom_mcp::server::execute_promoted_tool`
   (a shared dispatch over the existing `LoomMcp` facade methods), dispatched by the hosted server's
   injected `ServedMcpExecutor` (`RemoteRuntime::set_mcp_executor`, wired in `loom serve remote`). `ask`
   required extracting its domain logic out of the async/`LoomServer` handler layer into a shared seam
   (`ask_begin`/`ask_poll_state`/`ask_submit`) both the local handler and the server route call; its
   bounded wait (`ask_answers`) stays client-side so the server never holds the write authority on a long
   poll. Each family has in-sandbox unit coverage; the live remote-parity gate is the fixture
   `promoted_mcp_tools_execute_server_side_with_local_parity` (`crates/loom-cli/src/remote.rs`,
   `live_tests`), which sweeps every `SERVER_PROMOTED_TOOLS` name over the wire against a served endpoint
   and asserts the `Mcp.call_tool` result equals the local `execute_promoted_tool` result. It passed
   owner-run on 2026-07-16 (loom-cli `gemm-f16` blocks a loom-cli build in the ARM sandbox), so tasks
   622-624 are live-verified; `remote_capability_partitions_the_surface` is unchanged by promotion.
   **Landed (task 623):** `chat` (22 durable tools) is promoted the same way; `chat_presence`/
   `chat_set_presence` stay host-local (in-process ephemeral presence).
   **Landed (task 624):** `spaces`/`pages`/`structures` and the substrate reads/aliases/revisions/views/
   write-admission-policy tools are promoted, each write threading `expected_root` server-side.
   `substrate_transact` is promoted with client-side binding default-fill
   (`normalize_substrate_transact_arguments`, UX normalization only) + unbound server-side execution that
   rejects any op still missing `workspace`/`collection`.
   **Landed (task 625):** the promoted `tickets` tools (project create/rekey/policy get+set,
   project settings get+set, field catalog/put/retire, ticket create/update/delete, relation set/remove, get, history)
   are promoted the same way, running workflow
   validation / `authorize_transition` / operation records / indexed-table + profile-root commit
   server-side as one compare-and-commit unit; in-sandbox unit-verified and live-parity verified (the
   fixture sweep now covers the tickets tools, GREEN owner-run 2026-07-16).
3. Capability gate. Keep `ToolSpec` as the catalog source of truth, but change promoted class-3 tools from
   local-only rejection to server-executed remote operations. Tools stay rejected over remote until their
   server route, auth semantics, atomicity behavior, and parity fixture exist.
4. Atomic writes. Server-executed writes must preserve the local expected-root/profile-root/VCS commit
   semantics. If the server cannot execute the whole aggregate in one compare-and-commit unit, the tool
   remains unsupported over remote.
5. Permanent-local boundary. `studio-status`, host runtime metadata, and static/catalog-only tool
   metadata remain local to the MCP host. They do not become served-store operations.
6. Tests. Each promoted family needs unit coverage against the server-side route plus live remote parity
   against a served endpoint. A `GateTestBackend` stub can prove routing shape, but it is not sufficient
   for Done.

Delivery order: (a) server-side MCP tool/resource execution transport; (b) low-substrate aggregate families
(`apps`, `ask`, `drive`, `meetings`, then `chat`); (c) substrate/profile-root families (`pages`, `spaces`,
`structures`, `substrate` views); (d) `tickets`; (e) live parity closure and documentation/count updates.

## Done criteria (task 370)

Not Done until: remote URL and remote alias launch paths are accepted where supported; at least the
agreed IDL-backed unary subset works against a live remote Loom endpoint; host/composite and substrate
tools reject remote with explicit errors; `--stateless`+remote still fails immediately; docs and queue
describe the implemented coverage accurately; and tests prove all of the above.

## Deferred store-backed MCP projection gaps — RESOLVED by task 660

The 621-625 split promoted `apps`, `ask`, `drive`, `meetings`, `chat`, `pages`, `spaces`, `structures`,
`substrate`, and `tickets`. **Task 660 (2026-07-17) promoted the remaining store-backed families
server-side** via the same 621-626 route (`SERVER_PROMOTED_TOOLS` + `server::execute_promoted_tool` over
the shared `LoomMcp` facade): 19 of the 21 tools are now server-executed. The **2 exceptions are
reclassified permanent-local**, not promoted: `lifecycles_active_set` and `lifecycles_active_clear`
manipulate `LoomServer::active_lifecycle` — in-process, per-client active-lifecycle selection (the same
host-runtime class as `chat_presence` / the bound workspace), not served-store state — so they run
host-local and stay unpromoted (remote callers pass explicit ids).

| Family | Tools | R/W | Disposition |
| --- | --- | --- | --- |
| lifecycles | `lifecycles_current_surface`, `lifecycles_define`, `lifecycles_define_standard`, `lifecycles_definition`, `lifecycles_definitions`, `lifecycles_instance`, `lifecycles_instances`, `lifecycles_instantiate`, `lifecycles_operation_log`, `lifecycles_snapshot`, `lifecycles_snapshot_content`, `lifecycles_snapshot_plan`, `lifecycles_snapshots`, `lifecycles_transition` (14) | read/write | **Promoted server-side (task 660)** |
| lifecycles (host-local) | `lifecycles_active_set`, `lifecycles_active_clear` (2) | write | **Permanent-local (host-runtime in-process active-lifecycle selection); not promoted** |
| import | `import_submit_batch`, `import_execute_batch` (2) | write | **Promoted server-side (task 660)** |
| workgraph | `workgraph_changes`, `workgraph_fact_put` (2) | read/write | **Promoted server-side (task 660)** (`workgraph_metrics` was already promoted) |
| redmine | `redmine_import_snapshot` (1) | write | **Promoted server-side (task 660)** |

The permanent-local host/composite tools are distinct: `apps_call_tool`, `chat_presence`,
`chat_set_presence`, `search`, and `studio_reindex` are host-runtime, ephemeral, or
catalog-only, with no served-store state to project, so they reject remote permanently by design.

## Full tool classification

| Area | Tool | IDL interface | IDL method | R/W | Classification | Remote behavior |
| --- | --- | --- | --- | --- | --- | --- |
| apps | `apps_list` | FileSystem | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| apps | `apps_show` | FileSystem | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| apps | `apps_read_file` | FileSystem | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| apps | `apps_create` | FileSystem | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| apps | `apps_write_file` | FileSystem | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| apps | `apps_remove_file` | FileSystem | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| apps | `apps_call_tool` | FileSystem | - | write | HOST/COMPOSITE (permanent-local) | Permanent-local host-runtime/catalog feature: no served-store state to project; rejects remote with an explicit local-only error |
| ask | `ask_questions` | Document | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| ask | `ask_answers` | Document | - | read | HOST/COMPOSITE (remote-capable, client-driven) | Client-side bounded wait polling the server single-shot; the server never holds the write authority |
| ask | `ask_record` | Document | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| calendar | `calendar_create_collection` | Calendar | create_collection | write | IDL unary | Implement over remote (forward to `LoomClient::Calendar::create_collection`) |
| calendar | `calendar_get_collection` | Calendar | get_collection | read | IDL unary | Implement over remote (forward to `LoomClient::Calendar::get_collection`) |
| calendar | `calendar_list_collections` | Calendar | list_collections | read | IDL unary | Implement over remote (forward to `LoomClient::Calendar::list_collections`) |
| calendar | `calendar_delete_collection` | Calendar | delete_collection | write | IDL unary | Implement over remote (forward to `LoomClient::Calendar::delete_collection`) |
| calendar | `calendar_put_entry` | Calendar | put_entry | write | IDL unary | Implement over remote (forward to `LoomClient::Calendar::put_entry`) |
| calendar | `calendar_get_entry` | Calendar | get_entry | read | IDL unary | Implement over remote (forward to `LoomClient::Calendar::get_entry`) |
| calendar | `calendar_delete_entry` | Calendar | delete_entry | write | IDL unary | Implement over remote (forward to `LoomClient::Calendar::delete_entry`) |
| calendar | `calendar_list_entries` | Calendar | list_entries | read | IDL unary | Implement over remote (forward to `LoomClient::Calendar::list_entries`) |
| calendar | `calendar_range` | Calendar | range | read | IDL unary | Implement over remote (forward to `LoomClient::Calendar::range`) |
| calendar | `calendar_search` | Calendar | search | read | IDL unary | Implement over remote (forward to `LoomClient::Calendar::search`) |
| calendar | `calendar_to_ics` | Calendar | to_ics | read | IDL unary | Implement over remote (forward to `LoomClient::Calendar::to_ics`) |
| calendar | `calendar_put_ics` | Calendar | put_ics | write | IDL unary | Implement over remote (forward to `LoomClient::Calendar::put_ics`) |
| cas | `cas_put` | Cas | put | write | IDL unary | Implement over remote (forward to `LoomClient::Cas::put`) |
| cas | `cas_get` | Cas | get | read | IDL unary | Implement over remote (forward to `LoomClient::Cas::get`) |
| cas | `cas_has` | Cas | has | read | IDL unary | Implement over remote (forward to `LoomClient::Cas::has`) |
| cas | `cas_delete` | Cas | delete | write | IDL unary | Implement over remote (forward to `LoomClient::Cas::delete`) |
| cas | `cas_list` | Cas | list | read | IDL unary | Implement over remote (forward to `LoomClient::Cas::list`) |
| chat | `chat_channels` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| chat | `chat_fetch_events` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| chat | `chat_messages` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| chat | `chat_cursor` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| chat | `chat_presence` | Store | - | read | HOST/COMPOSITE (permanent-local) | Host-runtime ephemeral presence (in-process TTL state); stays local-only |
| chat | `chat_create_channel` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| chat | `chat_rename_channel` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| chat | `chat_post_message` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| chat | `chat_edit_message` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| chat | `chat_redact_message` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| chat | `chat_emoji_list` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| chat | `chat_emoji_register` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| chat | `chat_emoji_unregister` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| chat | `chat_add_reaction` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| chat | `chat_remove_reaction` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| chat | `chat_create_thread` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| chat | `chat_create_task` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| chat | `chat_claim_task` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| chat | `chat_complete_task` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| chat | `chat_invoke_agent` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| chat | `chat_agent_reply` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| chat | `chat_request_handoff` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| chat | `chat_update_cursor` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| chat | `chat_set_presence` | Store | - | write | HOST/COMPOSITE (permanent-local) | Host-runtime ephemeral presence (in-process TTL state); stays local-only |
| columnar | `columnar_create` | Columnar | create | write | IDL unary | Implement over remote (forward to `LoomClient::Columnar::create`) |
| columnar | `columnar_append` | Columnar | append | write | IDL unary | Implement over remote (forward to `LoomClient::Columnar::append`) |
| columnar | `columnar_compact` | Columnar | compact | write | IDL unary | Implement over remote (forward to `LoomClient::Columnar::compact`) |
| columnar | `columnar_scan` | Columnar | scan | read | IDL unary | Implement over remote (forward to `LoomClient::Columnar::scan`) |
| columnar | `columnar_columns` | Columnar | columns | read | IDL unary | Implement over remote (forward to `LoomClient::Columnar::columns`) |
| columnar | `columnar_rows` | Columnar | rows | read | IDL unary | Implement over remote (forward to `LoomClient::Columnar::rows`) |
| columnar | `columnar_inspect` | Columnar | inspect | read | IDL unary | Implement over remote (forward to `LoomClient::Columnar::inspect`) |
| columnar | `columnar_source_digest` | Columnar | source_digest | read | IDL unary | Implement over remote (forward to `LoomClient::Columnar::source_digest`) |
| columnar | `columnar_select` | Columnar | select | read | IDL unary | Implement over remote (forward to `LoomClient::Columnar::select`) |
| columnar | `columnar_aggregate` | Columnar | aggregate | read | IDL unary | Implement over remote (forward to `LoomClient::Columnar::aggregate`) |
| contacts | `contacts_create_book` | Contacts | create_book | write | IDL unary | Implement over remote (forward to `LoomClient::Contacts::create_book`) |
| contacts | `contacts_get_book` | Contacts | get_book | read | IDL unary | Implement over remote (forward to `LoomClient::Contacts::get_book`) |
| contacts | `contacts_list_books` | Contacts | list_books | read | IDL unary | Implement over remote (forward to `LoomClient::Contacts::list_books`) |
| contacts | `contacts_delete_book` | Contacts | delete_book | write | IDL unary | Implement over remote (forward to `LoomClient::Contacts::delete_book`) |
| contacts | `contacts_put_entry` | Contacts | put_entry | write | IDL unary | Implement over remote (forward to `LoomClient::Contacts::put_entry`) |
| contacts | `contacts_get_entry` | Contacts | get_entry | read | IDL unary | Implement over remote (forward to `LoomClient::Contacts::get_entry`) |
| contacts | `contacts_delete_entry` | Contacts | delete_entry | write | IDL unary | Implement over remote (forward to `LoomClient::Contacts::delete_entry`) |
| contacts | `contacts_list_entries` | Contacts | list_entries | read | IDL unary | Implement over remote (forward to `LoomClient::Contacts::list_entries`) |
| contacts | `contacts_search` | Contacts | search | read | IDL unary | Implement over remote (forward to `LoomClient::Contacts::search`) |
| contacts | `contacts_to_vcard` | Contacts | to_vcard | read | IDL unary | Implement over remote (forward to `LoomClient::Contacts::to_vcard`) |
| contacts | `contacts_put_vcard` | Contacts | put_vcard | write | IDL unary | Implement over remote (forward to `LoomClient::Contacts::put_vcard`) |
| dataframe | `dataframe_create` | Dataframe | create | write | IDL unary | Shipped + live-verified (plan-write forward) |
| dataframe | `dataframe_collect` | Dataframe | collect | read | IDL unary | Shipped + live-verified (byte-parity forward) |
| dataframe | `dataframe_preview` | Dataframe | preview | read | IDL unary | Shipped + live-verified (byte-parity forward) |
| dataframe | `dataframe_materialize` | Dataframe | materialize | write | IDL unary | Shipped + live-verified (Option<Digest>->Option<String>) |
| dataframe | `dataframe_plan_digest` | Dataframe | plan_digest | read | IDL unary | Shipped + live-verified (Digest->String) |
| dataframe | `dataframe_source_digests` | Dataframe | source_digests | read | IDL unary | Shipped + live-verified (byte-parity forward) |
| document | `document_put_text` | Document | put_text | write | IDL unary + overlay | SHIPPED: writes exact UTF-8 text with optional digest guard and refreshes the ref-index overlay |
| document | `document_get_text` | Document | get_text | read | IDL unary | SHIPPED: returns UTF-8 text plus digest, invalid UTF-8 maps to `DOCUMENT_NOT_TEXT` |
| document | `document_put_binary` | Document | put_binary | write | IDL unary + overlay | SHIPPED: writes raw bytes with optional digest guard and refreshes the ref-index overlay |
| document | `document_get_binary` | Document | get_binary | read | IDL unary | SHIPPED: returns raw bytes plus digest |
| document | `document_query` | Document | query_json | read | HOST COMPOSITE | SHIPPED (task 397): host-assembled over remote primitives (`document_list_binary` + `query_json`/`find_json` + `Store.digest_algo`), byte parity to local incl. per-item digest |
| document | `document_replace_text` | Document | replace_text_indexed | write | IDL unary + overlay | SHIPPED (395): forwards to `Document::replace_text_indexed` (find/replace + ref-index overlay in loom-reference; `{replacements,digest}` via `loom_wire::document`) |
| document | `document_delete` | Document | delete | write | IDL unary + overlay | SHIPPED (395): forwards to `Document::delete_indexed` (engine delete + ref-index overlay) |
| document | `document_list_binary` | Document | list_binary | read | IDL unary | SHIPPED: forwards collection bytes from `LoomClient::Document::list` under the explicit binary MCP name |
| document | `document_list_collections` | Document | list_collections | read | IDL unary | Implement over remote (forward to `LoomClient::Document::list_collections`) |
| drive | `drive_list` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| drive | `drive_stat` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| drive | `drive_read` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| drive | `drive_list_versions` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| drive | `drive_list_conflicts` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| drive | `drive_create_folder` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| drive | `drive_create_upload` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| drive | `drive_upload_chunk` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| drive | `drive_commit_upload` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| drive | `drive_rename` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| drive | `drive_move` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| drive | `drive_delete` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| drive | `drive_resolve_conflict` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| drive | `drive_list_shares` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| drive | `drive_grant_share` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| drive | `drive_revoke_share` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| drive | `drive_apply_share_expiry` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| drive | `drive_list_retention` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| drive | `drive_pin_retention` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| drive | `drive_unpin_retention` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| drive | `drive_apply_retention` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| drive | `drive_acquire_lease` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| drive | `drive_refresh_lease` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| drive | `drive_release_lease` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| drive | `drive_break_lease` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| fs | `fs_write_file` | FileSystem | write_file | write | IDL unary | Implement over remote (forward to `LoomClient::FileSystem::write_file`) |
| fs | `fs_read_file` | FileSystem | read_file | read | IDL unary | Implement over remote (forward to `LoomClient::FileSystem::read_file`) |
| fs | `fs_append_file` | FileSystem | append_file | write | IDL unary | Implement over remote (forward to `LoomClient::FileSystem::append_file`) |
| fs | `fs_remove_file` | FileSystem | remove_file | write | IDL unary | Implement over remote (forward to `LoomClient::FileSystem::remove_file`) |
| fs | `fs_read_at` | FileSystem | read_at | read | IDL unary | Implement over remote (forward to `LoomClient::FileSystem::read_at`) |
| fs | `fs_write_at` | FileSystem | write_at | write | IDL unary | Implement over remote (forward to `LoomClient::FileSystem::write_at`) |
| fs | `fs_truncate` | FileSystem | truncate | write | IDL unary | Implement over remote (forward to `LoomClient::FileSystem::truncate`) |
| fs | `fs_symlink` | FileSystem | symlink | write | IDL unary | Implement over remote (forward to `LoomClient::FileSystem::symlink`) |
| fs | `fs_read_link` | FileSystem | read_link | read | IDL unary | Implement over remote (forward to `LoomClient::FileSystem::read_link`) |
| fs | `fs_create_directory` | FileSystem | create_directory | write | IDL unary | Implement over remote (forward to `LoomClient::FileSystem::create_directory`) |
| fs | `fs_list_directory` | FileSystem | list_directory | read | IDL unary | Implement over remote (forward to `LoomClient::FileSystem::list_directory`) |
| fs | `fs_remove_directory` | FileSystem | remove_directory | write | IDL unary | Implement over remote (forward to `LoomClient::FileSystem::remove_directory`) |
| fs | `fs_stat` | FileSystem | stat | read | IDL unary | Implement over remote (forward to `LoomClient::FileSystem::stat`) |
| fts | `fts_create` | Search | create | write | IDL unary | Implement over remote (forward to `LoomClient::Search::create`) |
| fts | `fts_index` | Search | index | write | IDL unary | Implement over remote (forward to `LoomClient::Search::index`) |
| fts | `fts_get` | Search | get | read | IDL unary | Implement over remote (forward to `LoomClient::Search::get`) |
| fts | `fts_delete` | Search | delete | write | IDL unary | Implement over remote (forward to `LoomClient::Search::delete`) |
| fts | `fts_ids` | Search | ids | read | IDL unary | Implement over remote (forward to `LoomClient::Search::ids`) |
| fts | `fts_remap` | Search | remap | write | IDL unary | Implement over remote (forward to `LoomClient::Search::remap`) |
| fts | `fts_query` | Search | query | read | IDL unary | Implement over remote (forward to `LoomClient::Search::query`) |
| fts | `fts_source_digest` | Search | source_digest | read | IDL unary | Implement over remote (forward to `LoomClient::Search::source_digest`) |
| fts | `fts_status` | Search | status | read | IDL unary | Implement over remote (forward to `LoomClient::Search::status`) |
| graph | `graph_upsert_node` | Graph | upsert_node | write | IDL unary | Implement over remote (forward to `LoomClient::Graph::upsert_node`) |
| graph | `graph_get_node` | Graph | get_node | read | IDL unary | Implement over remote (forward to `LoomClient::Graph::get_node`) |
| graph | `graph_remove_node` | Graph | remove_node | write | IDL unary | Implement over remote (forward to `LoomClient::Graph::remove_node`) |
| graph | `graph_upsert_edge` | Graph | upsert_edge_indexed | write | IDL unary + overlay | SHIPPED (395): forwards to `Graph::upsert_edge_indexed` (engine write + ref-index overlay) |
| graph | `graph_get_edge` | Graph | get_edge | read | IDL unary | Implement over remote (forward to `LoomClient::Graph::get_edge`) |
| graph | `graph_remove_edge` | Graph | remove_edge_indexed | write | IDL unary + overlay | SHIPPED (395): forwards to `Graph::remove_edge_indexed` (engine delete + ref-index overlay) |
| graph | `graph_neighbors` | Graph | neighbors | read | IDL unary | Implement over remote (forward to `LoomClient::Graph::neighbors`) |
| graph | `graph_out_edges` | Graph | out_edges | read | IDL unary | Implement over remote (forward to `LoomClient::Graph::out_edges`) |
| graph | `graph_in_edges` | Graph | in_edges | read | IDL unary | Implement over remote (forward to `LoomClient::Graph::in_edges`) |
| graph | `graph_reachable` | Graph | reachable | read | IDL unary | Implement over remote (forward to `LoomClient::Graph::reachable`) |
| graph | `graph_shortest_path` | Graph | shortest_path | read | IDL unary | Implement over remote (forward to `LoomClient::Graph::shortest_path`) |
| graph | `graph_query` | Graph | query | read | IDL unary | Implement over remote (forward to `LoomClient::Graph::query`) |
| graph | `graph_explain_query` | Graph | explain_query | read | IDL unary | Implement over remote (forward to `LoomClient::Graph::explain_query`) |
| kv | `kv_put` | Kv | put | write | IDL unary | Implement over remote (forward to `LoomClient::Kv::put`) |
| kv | `kv_get` | Kv | get | read | IDL unary | Implement over remote (forward to `LoomClient::Kv::get`) |
| kv | `kv_delete` | Kv | delete | write | IDL unary | Implement over remote (forward to `LoomClient::Kv::delete`) |
| kv | `kv_list` | Kv | list | read | IDL unary | Implement over remote (forward to `LoomClient::Kv::list`) |
| kv | `kv_range` | Kv | range | read | IDL unary | Implement over remote (forward to `LoomClient::Kv::range`) |
| kv | `kv_list_collections` | Kv | list_collections | read | IDL unary | Implement over remote (forward to `LoomClient::Kv::list_collections`) |
| ledger | `ledger_append` | Ledger | append | write | IDL unary | Implement over remote (forward to `LoomClient::Ledger::append`) |
| ledger | `ledger_get` | Ledger | get | read | IDL unary | Implement over remote (forward to `LoomClient::Ledger::get`) |
| ledger | `ledger_head` | Ledger | head | read | IDL unary | Implement over remote (forward to `LoomClient::Ledger::head`) |
| ledger | `ledger_len` | Ledger | len | read | IDL unary | Implement over remote (forward to `LoomClient::Ledger::len`) |
| ledger | `ledger_verify` | Ledger | verify | read | IDL unary | Implement over remote (forward to `LoomClient::Ledger::verify`) |
| ledger | `ledger_list_collections` | Ledger | list_collections | read | IDL unary | Implement over remote (forward to `LoomClient::Ledger::list_collections`) |
| mail | `mail_create_mailbox` | Mail | create_mailbox | write | IDL unary | Implement over remote (forward to `LoomClient::Mail::create_mailbox`) |
| mail | `mail_get_mailbox` | Mail | get_mailbox | read | IDL unary | Implement over remote (forward to `LoomClient::Mail::get_mailbox`) |
| mail | `mail_list_mailboxes` | Mail | list_mailboxes | read | IDL unary | Implement over remote (forward to `LoomClient::Mail::list_mailboxes`) |
| mail | `mail_delete_mailbox` | Mail | delete_mailbox | write | IDL unary | Implement over remote (forward to `LoomClient::Mail::delete_mailbox`) |
| mail | `mail_ingest_message` | Mail | ingest_message | write | IDL unary | Implement over remote (forward to `LoomClient::Mail::ingest_message`) |
| mail | `mail_get_message` | Mail | get_message | read | IDL unary | Implement over remote (forward to `LoomClient::Mail::get_message`) |
| mail | `mail_to_eml` | Mail | to_eml | read | IDL unary | Implement over remote (forward to `LoomClient::Mail::to_eml`) |
| mail | `mail_delete_message` | Mail | delete_message | write | IDL unary | Implement over remote (forward to `LoomClient::Mail::delete_message`) |
| mail | `mail_list_messages` | Mail | list_messages | read | IDL unary | Implement over remote (forward to `LoomClient::Mail::list_messages`) |
| mail | `mail_get_flags` | Mail | get_flags | read | IDL unary | Implement over remote (forward to `LoomClient::Mail::get_flags`) |
| mail | `mail_set_flags` | Mail | set_flags | write | IDL unary | Implement over remote (forward to `LoomClient::Mail::set_flags`) |
| mail | `mail_search` | Mail | search | read | IDL unary | Implement over remote (forward to `LoomClient::Mail::search`) |
| lanes | `lanes_create` | Lanes | create | write | IDL unary | Shipped: forwards canonical Lane CBOR through `LoomClient::Lanes::create` |
| lanes | `lanes_get` | Lanes | get | read | IDL unary | Shipped: forwards to `LoomClient::Lanes::get` and decodes canonical Lane CBOR |
| lanes | `lanes_list` | Lanes | list | read | IDL unary | Shipped: forwards to `LoomClient::Lanes::list` and decodes canonical Lane CBOR records |
| lanes | `lanes_update` | Lanes | update | write | IDL unary | Shipped: forwards to `LoomClient::Lanes::update` |
| lanes | `lanes_ticket_add` | Lanes | ticket_add | write | IDL unary | Shipped: forwards to `LoomClient::Lanes::ticket_add` |
| lanes | `lanes_ticket_remove` | Lanes | ticket_remove | write | IDL unary | Shipped: forwards to `LoomClient::Lanes::ticket_remove` |
| meetings | `meetings_list` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| meetings | `meetings_get` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| meetings | `meetings_search` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| meetings | `meetings_projection_outputs` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| meetings | `meetings_extraction_review` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| meetings | `meetings_accept_annotation` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| meetings | `meetings_reject_annotation` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| meetings | `meetings_propose_vocabulary` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| meetings | `meetings_accept_vocabulary` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| meetings | `meetings_reject_vocabulary` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| meetings | `meetings_add_entity_merge` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| meetings | `meetings_add_promotion` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| meetings | `meetings_import_snapshot` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| meetings | `meetings_promote_artifact_to_reference_artifact` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| meetings | `meetings_promote_decision_to_decision_log` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| meetings | `meetings_promote_question_to_lifecycle` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| meetings | `meetings_promote_reference_to_reference_artifact` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| meetings | `meetings_promote_task_to_ticket` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| pages | `pages_create` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| pages | `pages_update` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| pages | `pages_publish` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| pages | `pages_get` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| pages | `pages_history` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| pages | `pages_list` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| queue | `queue_append` | Queue | append | write | IDL unary | Implement over remote (forward to `LoomClient::Queue::append`) |
| queue | `queue_get` | Queue | get | read | IDL unary | Implement over remote (forward to `LoomClient::Queue::get`) |
| queue | `queue_range` | Queue | range | read | IDL unary | Implement over remote (forward to `LoomClient::Queue::range`) |
| queue | `queue_len` | Queue | len | read | IDL unary | Implement over remote (forward to `LoomClient::Queue::len`) |
| queue | `queue_list_streams` | Queue | list_streams | read | IDL unary | Implement over remote (forward to `LoomClient::Queue::list_streams`) |
| queue | `queue_consumer_position` | QueueConsumers | consumer_position | read | IDL unary | Implement over remote (forward to `LoomClient::QueueConsumers::consumer_position`) |
| queue | `queue_consumer_read` | QueueConsumers | consumer_read | read | IDL unary | Implement over remote (forward to `LoomClient::QueueConsumers::consumer_read`) |
| queue | `queue_consumer_advance` | QueueConsumers | consumer_advance | write | IDL unary | Implement over remote (forward to `LoomClient::QueueConsumers::consumer_advance`) |
| queue | `queue_consumer_reset` | QueueConsumers | consumer_reset | write | IDL unary | Implement over remote (forward to `LoomClient::QueueConsumers::consumer_reset`) |
| search | `search` | Search | - | read | HOST/COMPOSITE (permanent-local) | Permanent-local host-runtime/catalog feature: no served-store state to project; rejects remote with an explicit local-only error |
| spaces | `spaces_create` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| spaces | `spaces_get` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| spaces | `spaces_list` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| sql | `sql_exec` | Sql | sql_exec | write | IDL unary | Shipped + live-verified (per-request `SqlSession`: open -> exec -> close; byte-clean `exec_cbor`) |
| sql | `sql_query` | Sql | sql_query | read | IDL unary | Wired (task 399: new read-only `Sql.sql_query_result` returns full `exec_cbor`, no persist); unit-verified, live-verified owner-side (2026-07-13) |
| sql | `sql_commit` | Sql | sql_commit | write | IDL unary | Rejects in-method - no caller `timestamp_ms` -> digest divergence (task 396) |
| sql | `sql_read_table` | Sql | sql_read_table | read | IDL unary | Shipped + live-verified (byte-parity forward) |
| sql | `sql_read_table_at` | Sql | sql_read_table_at | read | IDL unary | Shipped + live-verified (byte-parity forward) |
| sql | `sql_index_scan` | Sql | sql_index_scan | read | IDL unary | Shipped + live-verified (byte-parity forward) |
| sql | `sql_index_scan_at` | Sql | sql_index_scan_at | read | IDL unary | Shipped + live-verified (byte-parity forward) |
| sql | `sql_diff` | Sql | sql_diff | read | IDL unary | Shipped + live-verified (byte-parity forward) |
| sql | `sql_table_diff` | Sql | sql_table_diff | read | IDL unary | Shipped + live-verified (byte-parity forward) |
| sql | `sql_blame` | Sql | sql_blame | read | IDL unary | Shipped + live-verified (byte-parity forward) |
| sql | `sql_list_databases` | Sql | sql_list_databases | read | IDL unary | Shipped + live-verified (decodes wire `Array(Text)`) |
| store | `store_version` | Store | version | read | IDL unary | Implement over remote (forward to `LoomClient::Store::version`) |
| store | `store_capabilities` | Store | capabilities | read | IDL unary | Implement over remote (forward to `LoomClient::Store::capabilities`) |
| store | `store_blob_digest` | Store | blob_digest | read | IDL unary | Implement over remote (forward to `LoomClient::Store::blob_digest`) |
| structures | `structures_create` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| structures | `structures_get` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| structures | `structures_add_node` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| structures | `structures_update_node` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| structures | `structures_move_node` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| structures | `structures_link_node` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| structures | `structures_bind` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| structures | `structures_decompose_to_tickets` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| structures | `structures_list` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| studio | `studio_reindex` | Store | - | write | HOST/COMPOSITE (permanent-local) | Permanent-local host-runtime/catalog feature: no served-store state to project; rejects remote with an explicit local-only error |
| substrate | `substrate_changes` | Search | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| substrate | `substrate_refs` | Search | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| substrate | `substrate_alias_bind` | Search | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| substrate | `substrate_alias_release` | Search | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| substrate | `substrate_alias_resolve` | Search | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| substrate | `substrate_alias_list` | Search | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| substrate | `substrate_reference_status` | Search | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| substrate | `substrate_reference_reconcile` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| substrate | `substrate_history` | Search | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| substrate | `substrate_transact` | Search | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| substrate | `substrate_view_define` | Search | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| substrate | `substrate_view_get` | Search | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| substrate | `substrate_view_list` | Search | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| substrate | `substrate_write_admission_policy_get` | Search | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| substrate | `substrate_write_admission_policy_set` | Search | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| substrate | `substrate_checkpoint_before` | Search | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| substrate | `substrate_revision_as_of_root` | Search | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| substrate | `substrate_revision_at` | Search | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| substrate | `substrate_revision_latest` | Search | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| tickets | `tickets_project_create` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| tickets | `tickets_project_rekey` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| tickets | `tickets_project_settings_get` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| tickets | `tickets_project_settings_set` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| tickets | `tickets_fields` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| tickets | `tickets_field_put` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| tickets | `tickets_field_retire` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| tickets | `tickets_create` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| tickets | `tickets_update` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| tickets | `tickets_delete` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| tickets | `tickets_relation_set` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| tickets | `tickets_relation_remove` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| tickets | `tickets_get` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| tickets | `tickets_history` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| tickets | `tickets_project_settings_get` | Store | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| tickets | `tickets_project_settings_set` | Store | - | write | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| timeseries | `timeseries_put` | TimeSeries | put | write | IDL unary | Implement over remote (forward to `LoomClient::TimeSeries::put`) |
| timeseries | `timeseries_get` | TimeSeries | get | read | IDL unary | Implement over remote (forward to `LoomClient::TimeSeries::get`) |
| timeseries | `timeseries_range` | TimeSeries | range | read | IDL unary | Implement over remote (forward to `LoomClient::TimeSeries::range`) |
| timeseries | `timeseries_latest` | TimeSeries | latest | read | IDL unary | Wired (task 398: `latest` payload grew to `[ts, value]`); unit-verified, live-verified owner-side (2026-07-13) |
| timeseries | `timeseries_list_collections` | TimeSeries | list_collections | read | IDL unary | Implement over remote (forward to `LoomClient::TimeSeries::list_collections`) |
| vcs | `vcs_commit` | VersionControl | commit | write | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::commit`) |
| vcs | `vcs_branch` | VersionControl | branch | write | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::branch`) |
| vcs | `vcs_checkout` | VersionControl | checkout | write | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::checkout`) |
| vcs | `vcs_log` | VersionControl | log | read | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::log`) |
| vcs | `vcs_merge` | VersionControl | merge | write | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::merge`) |
| vcs | `vcs_merge_in_progress` | VersionControl | merge_in_progress | read | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::merge_in_progress`) |
| vcs | `vcs_merge_conflicts` | VersionControl | merge_conflicts | read | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::merge_conflicts`) |
| vcs | `vcs_merge_resolve` | VersionControl | merge_resolve | write | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::merge_resolve`) |
| vcs | `vcs_merge_abort` | VersionControl | merge_abort | write | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::merge_abort`) |
| vcs | `vcs_merge_continue` | VersionControl | merge_continue | write | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::merge_continue`) |
| vcs | `vcs_status` | VersionControl | status | read | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::status`) |
| vcs | `vcs_stage` | VersionControl | stage | write | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::stage`) |
| vcs | `vcs_stage_all` | VersionControl | stage_all | write | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::stage_all`) |
| vcs | `vcs_unstage` | VersionControl | unstage | write | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::unstage`) |
| vcs | `vcs_commit_staged` | VersionControl | commit_staged | write | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::commit_staged`) |
| vcs | `vcs_tag_create` | VersionControl | tag_create | write | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::tag_create`) |
| vcs | `vcs_tag_list` | VersionControl | tag_list | read | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::tag_list`) |
| vcs | `vcs_tag_target` | VersionControl | tag_target | read | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::tag_target`) |
| vcs | `vcs_tag_delete` | VersionControl | tag_delete | write | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::tag_delete`) |
| vcs | `vcs_tag_rename` | VersionControl | tag_rename | write | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::tag_rename`) |
| vcs | `vcs_restore_file` | VersionControl | restore_file | write | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::restore_file`) |
| vcs | `vcs_restore_path` | VersionControl | restore_path | write | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::restore_path`) |
| vcs | `vcs_cherry_pick` | VersionControl | cherry_pick | write | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::cherry_pick`) |
| vcs | `vcs_revert` | VersionControl | revert | write | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::revert`) |
| vcs | `vcs_rebase` | VersionControl | rebase | write | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::rebase`) |
| vcs | `vcs_squash` | VersionControl | squash | write | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::squash`) |
| vcs | `vcs_diff` | VersionControl | diff | read | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::diff`) |
| vcs | `vcs_blame` | VersionControl | blame | read | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::blame`) |
| vcs | `vcs_head_branch` | VersionControl | head_branch | read | IDL unary | Implement over remote (forward to `LoomClient::VersionControl::head_branch`) |
| vector | `vector_create` | Vector | create | write | IDL unary | Implement over remote (forward to `LoomClient::Vector::create`) |
| vector | `vector_upsert` | Vector | upsert | write | IDL unary | Implement over remote (forward to `LoomClient::Vector::upsert`) |
| vector | `vector_upsert_source` | Vector | upsert_source | write | IDL unary | Implement over remote (forward to `LoomClient::Vector::upsert_source`) |
| vector | `vector_get` | Vector | get | read | IDL unary | Implement over remote (forward to `LoomClient::Vector::get`) |
| vector | `vector_source_text` | Vector | source_text | read | IDL unary | Implement over remote (forward to `LoomClient::Vector::source_text`) |
| vector | `vector_embedding_model` | Vector | embedding_model | read | IDL unary | Implement over remote (forward to `LoomClient::Vector::embedding_model`) |
| vector | `vector_ids` | Vector | ids | read | IDL unary | Implement over remote (forward to `LoomClient::Vector::ids`) |
| vector | `vector_metadata_index_keys` | Vector | metadata_index_keys | read | IDL unary | Implement over remote (forward to `LoomClient::Vector::metadata_index_keys`) |
| vector | `vector_create_metadata_index` | Vector | create_metadata_index | write | IDL unary | Implement over remote (forward to `LoomClient::Vector::create_metadata_index`) |
| vector | `vector_drop_metadata_index` | Vector | drop_metadata_index | write | IDL unary | Implement over remote (forward to `LoomClient::Vector::drop_metadata_index`) |
| vector | `vector_delete` | Vector | delete | write | IDL unary | Implement over remote (forward to `LoomClient::Vector::delete`) |
| vector | `vector_search` | Vector | search | read | IDL unary | Implement over remote (forward to `LoomClient::Vector::search`) |
| vector | `vector_search_policy` | Vector | search_policy | read | IDL unary | Implement over remote (forward to `LoomClient::Vector::search_policy`) |
| watch | `watch_subscribe` | Watch | subscribe | read | IDL unary | Shipped + live-verified (resolves ws id, builds selector wire) |
| watch | `watch_poll` | Watch | poll | read | IDL unary | Shipped + live-verified (batch wire now carries `parent`) |
| workspace | `workspace_list` | Workspaces | workspace_list | read | IDL unary | Implement over remote (forward to `LoomClient::Workspaces::workspace_list`) |
| workgraph | `workgraph_changes` | Search | - | read | HOST/COMPOSITE (server-executed) | Promoted server-side (task 660). |
| workgraph | `workgraph_fact_put` | Search | - | write | HOST/COMPOSITE (server-executed) | Promoted server-side (task 660). |
| workgraph | `workgraph_metrics` | Search | - | read | HOST/COMPOSITE (server-executed) | Execute server-side beside the served store; result byte-compatible with the local MCP path |
| traces | `traces_get_span` | Traces | get_span | read | IDL unary | Implement over remote (forward to `LoomClient::Traces::get_span`) |
| traces | `traces_put_span` | Traces | put_span | write | IDL unary | Implement over remote (forward to `LoomClient::Traces::put_span`) |
| traces | `traces_query` | Traces | query | read | IDL unary | Implement over remote (forward to `LoomClient::Traces::query`) |
| traces | `traces_trace_spans` | Traces | trace_spans | read | IDL unary | Implement over remote (forward to `LoomClient::Traces::trace_spans`) |
| redmine | `redmine_import_snapshot` | Store | - | write | HOST/COMPOSITE (server-executed) | Promoted server-side (task 660). |
| metrics | `metrics_get_descriptor` | Metrics | get_descriptor | read | IDL unary | Implement over remote (forward to `LoomClient::Metrics::get_descriptor`) |
| metrics | `metrics_put_descriptor` | Metrics | put_descriptor | write | IDL unary | Implement over remote (forward to `LoomClient::Metrics::put_descriptor`) |
| metrics | `metrics_put_observation` | Metrics | put_observation | write | IDL unary | Implement over remote (forward to `LoomClient::Metrics::put_observation`) |
| metrics | `metrics_query` | Metrics | query | read | IDL unary | Implement over remote (forward to `LoomClient::Metrics::query`) |
| logs | `logs_get_record` | Logs | get_record | read | IDL unary | Implement over remote (forward to `LoomClient::Logs::get_record`) |
| logs | `logs_put_record` | Logs | put_record | write | IDL unary | Implement over remote (forward to `LoomClient::Logs::put_record`) |
| logs | `logs_query` | Logs | query | read | IDL unary | Implement over remote (forward to `LoomClient::Logs::query`) |
| lifecycles | `lifecycles_active_clear` | Store | - | write | HOST/COMPOSITE (permanent-local) | Permanent-local: in-process active-lifecycle selection (host-runtime state); not promoted. |
| lifecycles | `lifecycles_active_set` | Store | - | write | HOST/COMPOSITE (permanent-local) | Permanent-local: in-process active-lifecycle selection (host-runtime state); not promoted. |
| lifecycles | `lifecycles_current_surface` | Store | - | read | HOST/COMPOSITE (server-executed) | Promoted server-side (task 660). |
| lifecycles | `lifecycles_define` | Store | - | write | HOST/COMPOSITE (server-executed) | Promoted server-side (task 660). |
| lifecycles | `lifecycles_define_standard` | Store | - | write | HOST/COMPOSITE (server-executed) | Promoted server-side (task 660). |
| lifecycles | `lifecycles_definition` | Store | - | read | HOST/COMPOSITE (server-executed) | Promoted server-side (task 660). |
| lifecycles | `lifecycles_definitions` | Store | - | read | HOST/COMPOSITE (server-executed) | Promoted server-side (task 660). |
| lifecycles | `lifecycles_instance` | Store | - | read | HOST/COMPOSITE (server-executed) | Promoted server-side (task 660). |
| lifecycles | `lifecycles_instances` | Store | - | read | HOST/COMPOSITE (server-executed) | Promoted server-side (task 660). |
| lifecycles | `lifecycles_instantiate` | Store | - | write | HOST/COMPOSITE (server-executed) | Promoted server-side (task 660). |
| lifecycles | `lifecycles_operation_log` | Store | - | read | HOST/COMPOSITE (server-executed) | Promoted server-side (task 660). |
| lifecycles | `lifecycles_snapshot` | Store | - | read | HOST/COMPOSITE (server-executed) | Promoted server-side (task 660). |
| lifecycles | `lifecycles_snapshot_content` | Store | - | read | HOST/COMPOSITE (server-executed) | Promoted server-side (task 660). |
| lifecycles | `lifecycles_snapshot_plan` | Store | - | read | HOST/COMPOSITE (server-executed) | Promoted server-side (task 660). |
| lifecycles | `lifecycles_snapshots` | Store | - | read | HOST/COMPOSITE (server-executed) | Promoted server-side (task 660). |
| lifecycles | `lifecycles_transition` | Store | - | write | HOST/COMPOSITE (server-executed) | Promoted server-side (task 660). |
| import | `import_execute_batch` | Store | - | write | HOST/COMPOSITE (server-executed) | Promoted server-side (task 660). |
| import | `import_submit_batch` | Store | - | write | HOST/COMPOSITE (server-executed) | Promoted server-side (task 660). |
