# 0027a - Authorization Surface Matrix

**Status:** Draft target. **Version:** 0.1.0-draft. **Normative only after promotion.**

This sub-spec is the work queue for completing the 0027 policy enforcement point across every public
operation. 0027 defines the grant model and evaluation rule. This document maps each source-backed or
target public surface to an authorization decision so implementation work can proceed without guessing
which calls are covered.

## Current Source Boundary

Source-backed today:

- `loom_core::acl::AclStore` evaluates direct grants for principals and everyone, with allow/deny,
  default-deny in authenticated mode, unauthenticated root-mode bypass, deny-precedence, workspace and
  facet matching, and `Admin` coverage.
- `Loom` threads identity, session, and ACL state through selected engine operations.
- File operations, configured KV operations, SQL query/exec split, SQL direct table readers and
  mutators, SQL table history readers, document, ledger, time-series, calendar, contacts, mail,
  commit, checkout, branch, restore, log, path-level diff, structural diff, staging, and merge
  lifecycle operations have source-backed ACL checks.
- Tag list and target resolution require `vcs` `Read`; tag create, delete, and rename require `vcs`
  `Admin`. Branch updates that rewrite history rather than fast-forwarding require `vcs` `Admin`;
  ordinary commits, merge commits, and fast-forward branch updates continue to use their existing
  write, advance, or merge rights.
- CAS put/delete, CAS get/has/list, queue append/get/range/len, queue stream load/store, and queue
  consumer position/read/advance/reset have source-backed ACL checks in the Rust engine.
- Pull-watch subscribe and poll have source-backed ACL checks for the files baseline: ref read is
  required, explicit unauthorized files path-prefix selectors fail closed, and broad watches omit
  unauthorized file path changes.
- Direct workspace clone, fast-forward push, bundle export, and bundle import have source-backed ACL
  checks in the Rust engine. Source workspaces require read access across all workspace facets,
  destination push requires write and advance access across all destination facets, and workspace
  creation through clone or import requires global `Admin`.
- `FileStore` persists session-independent identity state and direct ACL grants in the durable-local
  control root.
- The C ABI `LoomSession` bridge authenticates a principal with a passphrase, binds that session to
  the open Loom object, reloads persisted identity and ACL state on every per-call open, and proves
  the bridge through ACL-checked operations. The C ABI, CLI, C++ `Loom`, and Swift `Loom.open` expose
  source-backed identity and direct ACL management for the local handle surface.
- Local C ABI `LoomIter`, `LoomTask`, and `LoomResultView` handles are in-process helpers over an
  operation that has already crossed the PEP. They are not hosted served handles and are not projected
  as MCP tools.
- The MCP host exposes a curated data-tool, prompt, and `loom://` resource surface over engine facade
  calls, not management workspace/identity/ACL/lock/daemon tools. A workspace-bound MCP server elides
  the workspace argument and re-roots resources; a collection-bound server also elides the
  collection-axis argument and drops collection discovery tools. Binding a workspace does not hide tool
  areas by facet.
- Result-view and diagnostics helpers are local decode or thread-local-error surfaces. They do not
  read Loom state by themselves; authorization is enforced by the operation that produced the result
  bytes or error.
- Conformance covers default-deny, deny-precedence, and selected engine PEP hooks.

Not source-backed today:

- (P0) A complete authorization matrix for every public operation exposed through core, CLI, IDL, C ABI,
  bindings, hosted protocols, and projections.
- (P0) ACL checks on authenticated hosted write/read surfaces, long-lived served result handles, and
  served sync projection. VFS, FUSE, and NFS mount auth propagation is source-backed for local
  projections; VFS conformance proves parent listings may reveal child names while denied direct
  lookup, read, and write map to `PERMISSION_DENIED` / `EACCES`. Local daemon-backed MCP principal
  authentication/session projection is source-backed; remote hosted protocol auth remains target.
  Streamable HTTP omits write tools until that hosted write contract is promoted.
- (P0) Remaining public identity and ACL management through hosted protocols and any binding surface
  not yet covered by the checked-in local projection. C ABI, CLI, Node, Python, C++, Swift, JVM,
  Android, React Native, WASM, and local MCP session propagation are source-backed for promoted local
  identity/session or ACL surfaces. Public-key credentials, hosted credentials, and wire protocol
  session projection remain target.
- (P1) Binding and hosted protocol conformance that proves permission checks are identical across
  projections.

## Authorization Principles

1. Every externally reachable operation has one owning facade name and one engine PEP decision.
2. Projection layers do not define policy. CLI, bindings, ABI, wire protocols, VFS, FUSE, NFS, and MCP
   all call an engine operation that authorizes before reading or mutating state.
3. Public method names use explicit facade prefixes: `identity_*`, `acl_*`, `lock_*`, `kv_*`, `vcs_*`,
   `cas_*`, `queue_*`, and so on.
4. Workspace history is authorized through the `vcs_*` operation family. `vcs_diff`, `vcs_log`, and
   `vcs_show` are not allowed to run unless the caller has the appropriate
   VCS/history permission for the workspace.
5. A caller granted VCS diff/log/show permission can see commit metadata for that operation, including
   commit messages. A caller without that permission receives `PERMISSION_DENIED` and sees no commit
   metadata.
6. Unit payload visibility remains owned by the relevant facet grant. A structural diff can include
   commit metadata because the VCS operation was authorized, while field values, row values, document
   bodies, mail subject/body, or other unit details still require the owning facet read grant.
7. Existence-leak behavior is explicit per operation. The default for authorization failure is
   `PERMISSION_DENIED`; security-sensitive lookup operations may mask as `NOT_FOUND` only when their
   owning spec says so and conformance pins it.
8. `FacetKind` identifies data facets. `AclDomain` identifies authorization boundaries. Every native
   facet maps to a same-named domain, while product services use explicit domains. Discovery and
   invocation consult the same domain declaration.

## Rights and Operation Families

The current source-backed `AclRight` set is `Read`, `Write`, `Advance`, `Merge`, `Execute`, and
`Admin`. The matrix below maps public operation families to those rights unless a future 0028 promotion
adds a narrower right. Workspace history uses the source-backed `vcs` facet tag for ACL grants, distinct
from the `files` facet that owns file payload reads and writes.

| Operation family | Resource | Required right | Current source hook | Notes |
| --- | --- | --- | --- | --- |
| `session_*` | handle or connection principal context | authentication proof | Source-backed IDL, C ABI, C++, Swift, CLI open context, checked-in local binding session surfaces, local MCP launch context | `loom_authenticate_passphrase` and `loom_clear_authentication` exist for `LoomSession`; C++/Swift wrap them, CLI `--auth-principal` plus `--auth-key-source` binds a per-command session, checked-in bindings route promoted local operations through authenticated session state, and `loom mcp` reuses the same launch context for daemon-backed per-request serving. Remote hosted session credentials remain target. |
| `identity_*` | store-global identity control state | `Admin` or recovery authority | Source-backed IDL, C ABI, CLI, Node, Python, C++, Swift, JVM, Android, React Native, WASM | Public local principal list/add/set-passphrase/remove exists. Built-in role records, role assignment/revocation, and admin-role recovery invariants are source-backed. Hosted credentials and generated wire projection remain target. |
| `acl_*` | store-global ACL control state | `Admin` | Source-backed IDL, C ABI, CLI, Node, Python, C++, Swift, JVM, Android, React Native, WASM | Direct grant list/grant/revoke exists for local handle surfaces. Role subjects are source-backed as `role:<uuid>`. CLI and IDL scoped grant inputs are source-backed; C ABI, binding, hosted wire scoped inputs, and cross-protocol conformance remain target. |
| `lock_*` | store-local lock coordinator key | `Admin` for global management, operation-specific right for guarded resources | Target | Core coordinator exists. Public projection and auth integration are missing. |
| `management_workspace_*` | workspace registry | `Admin` for create/rename/delete/list in the current CLI and local management projection | Partial | CLI workspace lifecycle is admin-gated once authenticated mode is active and is not exposed as ordinary MCP tools. Remaining binding and hosted workspace-list policy still need full review. |
| `fs_*` | files facet path | `Read` or `Write` | Partial | Public file read/write/remove and selected staging paths are hooked. Reserved `.loom` writes remain denied through the public file facade. |
| `vcs_commit` | workspace history | `vcs` `Write` plus `Advance` when moving the current ref | Source-backed local engine | Multi-facet commit records the workspace working tree behind the `vcs` gate. ACL-scoped touched-unit promotion remains target. |
| `vcs_log`, `vcs_show` | workspace history metadata | `vcs` `Read` | Partial | `log` is hooked. `show` and public projection breadth need review. Authorized callers see commit metadata. |
| `vcs_diff` | workspace history metadata plus facet units | `vcs` `Read`, plus facet `Read` for unit details | Source-backed local engine and selected projections | Public diff is workspace-aware structural `LMDIFF` and requires `vcs` read plus commit reachability. C ABI, MCP, Node, Python, and WASM projections call the same engine hook. Local presentation redacts unit details into facet-level roll-ups when the caller lacks the changed facet's `Read` right. Hosted projection remains target. Unauthorized callers cannot run or see commit messages. |
| `vcs_branch_*`, `vcs_tag_*`, `vcs_checkout`, `vcs_restore`, `vcs_stage_*`, `vcs_merge_*` | refs and working tree | `vcs` `Read`, `Write`, `Advance`, `Merge`, or `Admin` by operation | Source-backed local engine and selected projections | Branch, checkout, restore, stage, stage-all, merge-state reads, and merge lifecycle have source-backed hooks. Tag list/target require `vcs` `Read`; tag create/delete/rename require `vcs` `Admin`; non-fast-forward branch rewrites require `vcs` `Admin`. Public branch and tag names reject reserved/raw-ref spellings such as `HEAD`, `refs/...`, slash-separated refs, dot-prefixed names, trailing dots, `..`, backslashes, and control characters. |
| `sync_*`, bundle import/export, clone/push | workspace refs and object closure | `Read`, `Write`, `Advance`, and protected-ref policy by direction | Source-backed local engine, served projection target | Direct clone/export require source `Read` across all facets; push requires source `Read` plus destination `Write` and `Advance` across all facets and refuses non-fast-forward updates; bundle import and clone creation require destination global `Admin`. Hosted sync projection remains target. |
| `kv_*` data operations | KV map and key prefix | `Read` or `Write` | Partial | Configured KV data operations are hooked. Legacy and projection paths need full review. |
| `management_kv_*` | KV map configuration | `Admin` | Partial | Durable tier config is source-backed through local management projection. MCP and hosted management projection remain target. |
| `cas_*` | CAS digest set | `Read` or `Write` | Source-backed local Rust engine | Unknown digest lookup remains absent as `Ok(None)` after read authorization passes. Hosted mapping remains target. |
| `queue_*` | stream and consumer offsets | `Read`, `Write`, or `Advance` | Source-backed local Rust engine | Append/store require `Write`; get/range/len/load and consumer position/read require `Read`; consumer advance/reset require `Advance`. |
| `sql_*`, direct table readers, result views | SQL database/table/row | `Read`, `Write`, or `Merge` | Source-backed local engine and selected projections | `sql_query` requires `Read`, rejects mutating statements, and never persists; `sql_exec` and SQL batches require `Write`; direct table mutators require `Write`; direct table readers, table blame, and workspace-aware table diff require `Read`; table diff rejects commits not reachable from the workspace. Local row iterators, tasks, and result views decode or deliver already-produced local bytes. Hosted served result handles remain target and must bind principal/session/resource context and re-check the PEP before each future chunk or result retrieval. |
| `document_*` | document collection and document id | `Read` or `Write` | Source-backed local Rust engine | Put/delete require `Write`; get/list require `Read`. |
| `ledger_*` | ledger stream | `Read` or `Write` | Source-backed local Rust engine | Append requires `Write`; get/head/len/verify require `Read`. |
| `time_series_*` | series set, series, point range | `Read` or `Write` | Source-backed local Rust engine | Put requires `Write`; get/range/latest require `Read`. |
| `calendar_*`, `contacts_*`, `mail_*` | principal collection and unit id | `Read` or `Write` | Source-backed local Rust engine | Local facades enforce facet-level grants. Hosted serving remains target. Mail bodies use internal CAS storage without requiring a separate public `cas_*` grant. |
| `graph_*`, `vector_*`, `columnar_*`, `search_*` | owning collection and unit/segment/query | `Read` or `Write` | Source-backed local Rust engine | Local facades enforce collection-scoped grants. Derived vector and columnar accelerator reads inherit collection `Read`. Facet-specific query redaction below the collection and merge policy remain owning-spec target work. |
| `exec_*` | program plus authorized state access | `Execute`, intersected with program manifest grants | Target | Compute has program grants; principal ACL integration is target. |
| `watch_*`, durable delivery subscriptions | observed resources | `Read` | Partial | Pull-watch requires VCS/ref read, supports files-domain path-prefix authorization, rejects explicit unauthorized file prefixes with `PERMISSION_DENIED`, and omits unauthorized file path changes from broad watches. Non-file domain event authorization, durable delivery subscription authorization, and hosted push replay checks remain target. |
| `trigger_*` | trigger definition, fire execution, and run-as authority | `Admin`, `Execute`, and target-resource rights | Partial | Source-backed Rust trigger execution resolves `run_as` at fire time through `run_as_context`, enforces program grants and target-resource ACLs, fails closed, and records denied/skipped/error outcomes. Public trigger definition, reassignment, CLI/ABI/binding, and hosted authorization remain target until the trigger facade is promoted. |
| `tickets_*`, `lanes_*` | ticket projects, tickets, comments, workflow policy, and lane coordination | `tickets` `Read`, `Write`, or `Admin` | Source-backed domain mapping | Runtime and MCP discovery share the Tickets domain; full binding and import conformance remains. |
| `pages_*`, `structures_*` | page content and knowledge structures | `pages` `Read`, `Write`, or `Admin` | Source-backed domain mapping | Page access does not imply VCS history access. |
| `chat_*` | channels, membership, messages, threads, and moderation | `chat` `Read`, `Write`, `Advance`, or `Admin` | Source-backed domain mapping | Stronger membership and moderation conformance remains operation-specific. |
| `lifecycle_*` | lifecycle definitions, instances, and controls | `lifecycle` operation-specific right | Source-backed domain mapping | Runtime and MCP discovery use the Lifecycle domain. |
| `meetings_*` | meetings, sources, annotations, and extraction | `meetings` `Read`, `Write`, or `Admin` | Source-backed domain mapping | Runtime and MCP discovery use the Meetings domain. |
| `drive_*` | file hierarchy and Drive metadata | `files` with path scope | Source-backed correction | Drive is a Files projection. |
| `workgraph_*` | ticket topology plus expanded target resources | `tickets`, then each expanded target domain | Partial | MCP fact writes, board-read observation, and cursor reads authorize or filter by task as a `tickets` key-prefix resource; non-ticket target expansion remains target. |
| `substrate_*` | operation-specific native target | Target domain selected by operation | Partial | Operation-change records preserve and expose `target_entity_id`; Workgraph cursor reads consume it for filtering. Broader operation-specific dispatch and authorization filtering still need to consume it consistently. Substrate is not grantable; each operation enforces its actual target. |
| Served result handles | original operation plus returned chunk/resource | same right as original operation, rechecked on each handle operation | Target, policy defined | Handles are principal-bound, session-family-bound, operation-bound, scope-bound, expiring continuations. Revocation affects future chunks. The server may narrow future chunks, but fails closed if it cannot prove a chunk is still authorized. |
| VFS/FUSE/NFS/MCP projections | projected engine operation | same as underlying operation | Partial | MCP data tools/resources call the engine facade, exclude management tools, and source-back local launch-time principal auth for daemon-backed serving. MCP `tools/list` derives regular read/write visibility from the bound session's current ACL state and emits tool-list change notifications when the visible set changes; argument-scoped checks remain in the engine PEP. Workspace/collection scoping elides bound arguments. Streamable HTTP omits write tools until authenticated hosted writes are promoted. VFS/FUSE/NFS local builders can attach the CLI mount principal session and unlock key before projection operations run. VFS core conformance pins filesystem-style denied direct access behavior. Remote hosted projection and FUSE/NFS runtime gates remain target. |

## Implementation Slices

| Priority | Slice | Priority dependencies | User effort |
| --- | --- | --- | --- |
| 1 | (P0) Project remaining `session_*`, `identity_*`, `acl_*`, `lock_*`, `management_workspace_*`, and `management_kv_*` surfaces through IDL, C ABI, CLI, and selected bindings with conformance. CLI scoped ACL grant inputs are source-backed; C ABI and binding scoped grant inputs remain target. | Current local engine PEP hooks. | No, names are settled unless changed. |
| 2 | (P0) Add reserved-ref and served-write authorization gates before hosted writes promote. Branch/tag name validation, tag lifecycle, non-fast-forward local branch rewrites, and Streamable HTTP write-tool omission are source-backed. | Priority 1. | Later reserved-ref policy choices may be needed if arbitrary refs are promoted. |
| 3 | (P1) Add binding and hosted protocol conformance that proves authorization parity across projections. | Priorities 1-2. | Possibly for platform-specific gates. |
| 4 | (P0) Finish the source-backed `AclDomain` promotion by adding Workgraph target expansion, operation-specific Substrate metadata, binding runtime conformance, and the controlled migration of persisted grants. Core, durable and wire codecs, IDL, generated remote surfaces, CLI, C ABI, hosted administration, direct product mappings, MCP domain isolation, selected MCP Workgraph task-key checks, operation-change target metadata, and Workgraph cursor filtering are source-backed. | Controlled pre-release migration of persisted grants. | No; 0027 §2.1 settles domain ownership. |

## Relationship to 0003d

`vcs_diff` is a `vcs_*` history operation. The first authorization decision is whether the caller
may run the VCS diff and see commit metadata for the workspace. If not, the call fails before any commit
message, parent id, changed-unit id, or aggregate is revealed. If yes, the result can include commit
metadata; unit-level payload details still pass through the owning facet's read policy before display.
