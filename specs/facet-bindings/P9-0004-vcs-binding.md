# P9-0004 - `vcs` Binding

**Series:** P9 binding plan (normative-track sub-series; Draft)
**Version:** 0.1.0-draft - **Status:** Draft - **Last updated:** 2026-07-02
**Reads first:** [`P9-0002-projection-conventions.md`](./P9-0002-projection-conventions.md), facade spec
**0003 section 5** (version control) + **section 9** (per-workspace lock), **0008 section 3.3** (REST mapping already exists),
**0006** (sync), **0012 / ADR-0002** (git interop excluded).

Like `files`, the `vcs` REST projection is already normative in 0008 section 3.3, and the facade is built in
`loom-core::vcs`. Hosted `vcs/rest`, `vcs/json_rpc`, and native `vcs/grpc` now have source-backed
listener-scoped subsets for commit, commit-staged, log, branch, checkout, status, stage, stage-all,
unstage, merge, and structural diff through durable `loom serve` listener records. The one structural
fact that makes this doc short: **`vcs` has no Tier-2 foreign adapter** - the only foreign VCS protocol
is git, and git interoperability is permanently out of scope (0012). Cross-Loom replication is the
native Shuttle sync transport (0008 section 7), not a foreign protocol.

## 1. Facade surface (0003 section 5)

`commit`, `log`, `branch` (create), `checkout` (commit or branch), `diff`, three-way `merge`, `rebase`,
`squash`, `cherry_pick`, `revert`, `update_ref` (compare-and-swap), `read_object`, `tag`. All scoped
within one workspace; cross-workspace history ops return `CROSS_WORKSPACE` (0014). A `files`-typed
workspace is **linear** and rejects branch/merge (0014).

Build status: `commit`/`commit_staged`/`status`/`stage`/`stage_all`/`unstage`/`log`/`branch`/
`checkout`/`diff`/three-way `merge` (with ORT-style virtual base)/merge conflict lifecycle/tag/
restore/`cherry_pick`/`revert`/`rebase`/`squash` are built in `loom-core::vcs`; hosted REST/JSON-RPC
and native gRPC currently cover the listener-scoped subset above. Hosted merge-resolution routes,
tag/restore/replay routes, generated resource-shaped REST, update-ref, read-object, generated schemas,
and full protocol conformance remain target.

### 1.1 Binding Boundary

The base layer is the Loom commit, ref, object, and workspace-history model. Native projections expose
that model through Loom protocols and MCP. Git remote compatibility is excluded, so there is no Tier-2 VCS
wire presentation. Loom bundles are interchange. Diff materializations and GUI-oriented history views are
derived views over commits, not independent state.

## 2. Tier-1 - REST (0008 section 3.3 - already normative)

Facet-root: refs at `/v1/workspaces/{workspace_id}/refs/{name}`, commits at `/commits`, objects at
`.../objects/{digest}` (shared with `cas`, P9-0005). Mapping is 0008 section 3.3 verbatim:

| Facade method | HTTP |
| --- | --- |
| `commit` | `POST /commits {message, author, ...}` -> `201` + `Location: /commits/{digest}` |
| `log` | `GET /commits?ref=...&limit=...` (paged / NDJSON) |
| `branch` create | `PUT /refs/branch/{name} {at}` |
| `checkout` | `POST :checkout {target, ...}` |
| `merge` | `POST :merge {source, strategy}` -> `200` / `409` (conflicts) |
| `rebase`/`squash`/`cherry_pick`/`revert` | `POST :{op}` with op body |
| `update_ref` (CAS) | `PUT /refs/{name}` + `If-Match: "{expected_old_digest}"` -> `412` on mismatch |
| `read_object` | `GET /objects/{digest}` (immutable; `Cache-Control: immutable`) |

Current source-backed hosted VCS REST is listener-scoped to one workspace and implements
`POST /commits`, `GET /commits`, `PUT /refs/branch/{name}`, `POST /vcs:checkout`, `POST /vcs:diff`,
`POST /vcs:merge`, `GET /status`, `POST /stage`, `POST /stage-all`, and `POST /unstage`. The `/vcs:*`
action names are the current axum-compatible representation of the `:{op}` action routes in 0008.

## 3. Tier-1 - JSON-RPC

1:1: `vcs.commit`, `vcs.log`, `vcs.branch`, `vcs.checkout`, `vcs.merge`, `vcs.diff`, `vcs.updateRef`,
`vcs.readObject`, ... `vcs.log`/`vcs.diff` stream via `*.next`/`*.end` (P9-0002 section 3).

Current source-backed hosted JSON-RPC methods are `vcs.commit`, `vcs.log`, `vcs.branch`,
`vcs.checkout`, `vcs.status`, `vcs.stage`, `vcs.stage_all`, `vcs.unstage`, `vcs.merge`, and `vcs.diff`.
`vcs.diff` returns the structural `LMDIFF` CBOR envelope as hex. Method aliases, streaming, merge
resolution, tag/restore/replay, update-ref, read-object, and gRPC parity remain target.

## 4. Tier-1 - gRPC

`Log` and `Diff` are **server-streaming**; `Commit`, `Branch`, `Checkout`, `Merge`, `UpdateRef`,
`ReadObject` are unary (0008 section 4.2). `Merge` returns the `MergeOutcome` (FastForward / Merged / Conflicts /
UpToDate) as a typed message.

Current source-backed hosted gRPC serves `loom.hosted.v1.Vcs` over daemon-opened `vcs/grpc` listeners
for unary `Commit`, `Branch`, `Checkout`, `Diff`, `Merge`, `Status`, `Stage`, `StageAll`, and
`Unstage`, plus server-streaming `Log`. `Diff` returns structural `LMDIFF` CBOR bytes. The built subset
reuses the shared hosted kernel for metadata auth, PEP, stable error mapping, and store writes.
Merge-resolution routes, tag/restore/replay, update-ref, read-object, generated protobuf artifacts,
and broader conformance remain target.

## 5. Tier-1 - MCP

- **Read tools (always on):** `vcs.log`, `vcs.diff`, `vcs.show` (read a commit/object).
- **Write tools (token-gated, P9-0002 section 5):** `vcs.commit`, `vcs.branch`, `vcs.checkout`, `vcs.merge`,
  `vcs.updateRef`. Ref-advancing tools are the highest-risk MCP surface (an agent could move `branch/main`)
  and are hidden unless a capability token grants `advance` on the ref-glob (0009 section 6).

## 6. Tier-2 - foreign adapter

**None.** The only reference protocol is git (`git clone`/`push`), which is **permanently excluded** -
import or live remote - by ADR-0002 and 0012. Replication between Looms uses the native Shuttle protocol
over gRPC/HTTP/WebSocket or an offline bundle (0008 section 7), which is a *native* transport, not a Tier-2
foreign adapter. This is the one facet where "full Tier-2 ambition" (P9-0001 OQ1) resolves to "no
adapter," by prior decision.

## 7. Errors / parity / concurrency

- **Errors:** all in 0008 section 6 (`NOTHING_TO_COMMIT`, `CAS_MISMATCH`, `NOT_FAST_FORWARD`, `MERGE_CONFLICT`,
  `MERGE_IN_PROGRESS`, `REBASE_IN_PROGRESS`, `RANGE_NOT_LINEAR`, `NOT_MERGED`). No new codes.
- **Parity (0032):** fully portable - `vcs` is pure object-model logic.
- **Concurrency:** ref advance is a compare-and-swap under the per-workspace lock (0003 section 9); a lost update
  surfaces as `CAS_MISMATCH`/`412`.

## 8. Open Questions

### OQ-V1 - Conflict-resolution round-trip over the wire (open)

- **Context.** 0008 section 3.3 maps `merge` to `200`/`409`, returning the conflicting paths on `409`, but does
  not define how a client **submits resolutions** over the wire to complete a conflicted merge - the
  engine has a deterministic auto-resolution, but a human/agent override path is unspecified for REST/MCP.
- **Example.** A `vcs.merge` MCP call returns `MERGE_CONFLICT` on `a.txt`; the agent wants to resolve
  `a.txt` to a chosen blob and finalize, but there is no defined "resolve + continue" call.
- **Options.** (a) a `POST :merge/resolve {path -> blob/strategy}` (and an MCP `vcs.resolve`
  write-tool) that completes an in-progress merge; (b) require the client to write the resolved tree and
  `commit` a merge commit with both parents explicitly; (c) only expose the deterministic auto-merge over
  the wire and force manual resolution via a checked-out working tree (FUSE/local), not over REST/MCP.
- **Recommendation.** (a) an explicit resolve+continue call mirroring `MERGE_IN_PROGRESS` state - it keeps
  the conflicted-merge lifecycle first-class over every transport (consistent with the `MERGE_IN_PROGRESS`
  code already in 0008 section 6), where (b) loses the guard rails and (c) makes headless/agent merges impossible.
