# 0067b - Remote MCP decision packet (tasks 395-399)

Decision packet for the five remote-MCP follow-ups under task **370**. **No implementation yet** - each
task needs an owner contract decision before coding. All five are children/follow-ups of 370, startable
independently, and each is currently handled by a precise in-method (or gate) rejection over remote so
behavior is safe and honest today.

Shared change flow for any IDL-changing task below (so "files expected to change" stays consistent):

1. Edit the IDL source `idl/loom.idl`.
2. Regenerate (never hand-edit these): `cargo run -p uldren-loom-remote-codegen` →
   `crates/loom-remote-protocol/src/generated_api.rs`, `.../generated.rs`,
   `crates/loom-remote-client/src/generated_client.rs`, `crates/loom-hosted-core/src/generated_dispatch.rs`.
3. Implement the engine op on `LocalLoomClient` in `crates/loom-client/src/local.rs` (+ the thin generated
   binding in `crates/loom-client/src/service.rs`).
4. Wire the MCP host: `crates/loom-mcp/src/{reads,writes}.rs` (replace the interim reject with a forward),
   `crates/loom-mcp/src/lib.rs` (`RemoteMcpBackend` trait method; retire the reject helper),
   `crates/loom-cli/src/remote.rs` (`McpRemoteBackend` impl), `crates/loom-mcp/src/server/tests.rs`
   (`GateTestBackend` stub).
5. Docs: `_QUEUE11.md` (task row + 370 row), `specs/0067-remote-loom-protocol.md`,
   `specs/0067a-mcp-remote-inventory.md`.
6. Tests: focused loom-mcp unit tests + the live omnibus
   `crates/loom-cli/src/remote.rs::live_tests::mcp_kv_round_trip_through_remote_backend`.

---

## 395 - document/graph reference-index writes

**DECISION (owner) + IMPLEMENTED; live-verified owner-side (2026-07-13).** Shared home = **`loom-reference`** (not
loom-core; no new crate; server dispatch does not depend on loom-mcp). Supplemental **`*_indexed`** IDL
methods (`Document.put_binary_indexed`/`delete_indexed`/`replace_text_indexed` (+`struct DocumentReplaceTextResult`),
`Graph.upsert_edge_indexed`/`remove_edge_indexed`); explicit binary document methods stay overlay-free for non-MCP clients;
the indexed methods are host-internal (EXCLUDED, not tools). The combined engine-write + overlay was
relocated into `loom-reference` (called by both the local MCP host and the server dispatch - no
duplication; loom-mcp's copies removed, its reconcile/alias paths now call the loom-reference versions).
`LocalLoomClient` inherents + `service.rs` bridges run the combined fn inside one `with_session`+`save_loom`
unit (atomic; a mid-overlay failure does not persist the primary write). `RemoteMcpBackend`/`McpRemoteBackend`
forward the five; `replace_text_indexed` crosses the wire via `loom_wire::document::replace_text_result_*`.
The MCP document/graph write tools call the indexed path locally AND over remote; the reject helper is
removed. Verified: loom-mcp `remote_ref_index_writes_forward` + `surface_covers_projected_idl_interfaces`;
loom-reference + loom-wire unit tests; whole-graph compile; codegen `--check` (311 methods); clippy clean.
Live parity (primary facet + byte-identical reference index, local vs remote) asserted in the omnibus test
and LIVE-VERIFIED owner-side (GREEN, 2026-07-13).

**Question.** How should the MCP-host substrate reference-index overlay be preserved when
`document.put_binary`/`delete`/`replace_text` and `graph.upsert_edge`/`remove_edge` run over remote - and where
should the shared overlay live?

**Context.** Locally these MCP writes run the engine op **and then** a host-side substrate ref-index
overlay (alias/ref index) in the same call. The generated IDL methods run only the engine op, so a raw
remote forward would leave the ref index stale. They reject over remote today.

**Source references.**
- `crates/loom-mcp/src/writes.rs:1722` `write_document_put` (and `write_document_delete`,
  `write_document_replace_text` nearby) and `:1353`/`:1375` `write_graph_upsert_edge`/`write_graph_remove_edge`
  - each early-returns `crate::remote_ref_index_write_unsupported(op)` when a remote backend is present.
- Overlay: `crates/loom-mcp/src/substrate_refs.rs` - `update_document_refs`, `update_graph_edge_refs`,
  `remove_graph_edge_refs`.
- Reject helper: `crates/loom-mcp/src/lib.rs:820` `remote_ref_index_write_unsupported`.

**Options.**
- **(A) Combined server-side IDL methods** (e.g. `Document.put_binary_indexed`/`delete_indexed`/
  `replace_text_indexed`, `Graph.upsert_edge_indexed`/`remove_edge_indexed`) that run engine write +
  overlay in one op; relocate the overlay into a crate both the MCP host and server dispatch call.
- **(B)** Keep the overlay MCP-host-only and reject document/graph edge writes over remote permanently
  (reads + node writes stay remote).
- **(C)** Two round-trips from the host (engine write, then a separate ref-index write method) - rejected
  as non-atomic (a crash between the two leaves the index inconsistent).

**Recommendation.** (A). Sub-decision still required and deliberately **not** pre-decided here: the
overlay's relocation target (loom-core vs a new shared crate) is a layering call that needs its own
short source-backed design when 395 starts - do not assume loom-core.

**Consequence of deferring.** Document and graph remain read-mostly over remote; edge/document mutations
stay local-only. 370 cannot claim full Document/Graph remote **write** parity.

**Files expected to change.** `idl/loom.idl` (5 combined methods) → regen (4 generated files);
`crates/loom-client/src/local.rs` (+`service.rs`); the relocated overlay module (loom-mcp
`substrate_refs.rs` → shared crate, TBD) called by both host and `generated_dispatch.rs`;
`crates/loom-mcp/src/writes.rs` + `lib.rs` (retire `remote_ref_index_write_unsupported` for these);
`crates/loom-cli/src/remote.rs`; `crates/loom-mcp/src/server/tests.rs`; docs; tests.

**Acceptance.** IDL+codegen+dispatch+client stubs updated; overlay shared, not duplicated; local MCP
output byte-for-byte unchanged; a live remote write updates BOTH the primary object and the ref-index
entry (asserted, not just non-error); existing families still pass; fmt + clippy clean.

---

## 396 - timestamped VCS writes + `sql_commit`

**DECISION (owner): option (A) - add `timestamp_ms` params to the existing methods (`merge` already has
`cell_level`). Split by return type: 396a IMPLEMENTED, 396b IMPLEMENTED.** 396a wired the six
digest-returning timestamped writes - `commit`, `commit_staged`, `tag_create`, `merge_continue`, `squash`,
`sql_commit` - end-to-end (IDL `timestamp_ms` param, codegen, `LocalLoomClient` threads the caller
timestamp instead of `now_ms()`, MCP forwards; `sql_commit` reuses `vcs_commit`); live-verified owner-side (2026-07-13)
(asserts remote==local commit digest at a fixed `timestamp_ms`). 396b wired the richer-return replay/merge
writes `merge`/`cherry_pick`/`revert`/`rebase`: they gained `timestamp_ms` and now forward over remote -
the host decodes the canonical `MergeResult`/`ReplayOutcome` wire back into the engine
`MergeOutcome`/`ReplayOutcome` via the round-trip-tested `loom_wire::vcs` codecs. The decode is a lossless
bijection (`MergeResult{commit,fast_forwarded,conflicts}` <-> `{UpToDate,FastForward,Merged,Conflicts}`;
`ReplayOutcome{kind,tip,paths}` <-> `{Replayed,Clean,Conflicts,Empty}`), so no new contract decision was
needed - this was a divergent-but-lossless bridge, not a stop-and-ask. Unit-verified
`remote_vcs_timestamped_writes_forward` (renamed; asserts all four forward+decode); live-verified owner-side (2026-07-13).
The interim `remote_vcs_timestamp_write_unsupported` reject helper is removed (no callers remain).

**Question.** How should a caller-supplied `timestamp_ms` cross the IDL for content-addressed commit-like
writes: add a `timestamp_ms` parameter to the existing methods, or add parallel `*_at` methods? And should
`merge`'s `cell_level` flag be exposed for MCP parity?

**Context.** The MCP write methods take a caller `timestamp_ms`; the IDL methods have none and the server
stamps `now_ms()`. Because commit/tag/replay objects are content-addressed and include the timestamp, a
remote forward changes the digest and silently drops the caller timestamp. Scope: `commit`,
`commit_staged`, `tag_create`, `merge`, `merge_continue`, `cherry_pick`, `revert`, `rebase`, `squash`,
and **`sql_commit`** (a VCS commit over the SQL-facet workspace - same root cause, folded in).

**Source references.**
- `crates/loom-mcp/src/writes.rs::write_vcs_commit(ws, author, message, timestamp_ms)` →
  `loom.commit(ns, author, message, timestamp_ms)`; `write_sql_commit(ws, author, message, timestamp_ms)`
  → `loom.commit(...)`.
- IDL (no timestamp): `crates/loom-remote-protocol/src/generated_api.rs:375`
  `VersionControl::commit(handle, workspace, author, message)`; `:1945` `Sql::sql_commit(session, message, author)`.
- Server stamps server time: `crates/loom-client/src/local.rs` `now_ms()` (VCS commit path; and `:5066`
  for `sql_commit`).
- Reject helpers: `crate::remote_vcs_timestamp_write_unsupported`, `crate::remote_sql_commit_unsupported`.

**Options.**
- **(A)** Add a `timestamp_ms` parameter to each existing method (one method per op).
- **(B)** Add new `*_at` methods carrying `timestamp_ms`, leaving current methods (server-time) intact.

**Recommendation.** (A) - add `timestamp_ms` to the existing methods (no parallel family), and expose
`merge cell_level` since the MCP tool already accepts it. Rationale: the MCP surface always supplies a
timestamp, so a server-time-only variant has no MCP caller.

**Consequence of deferring.** All timestamped VCS writes and `sql_commit` stay local-only over remote;
370 cannot claim full VCS remote write parity, and the SQL tranche stays partial (`sql_exec` only).

**Files expected to change.** `idl/loom.idl` (10 methods gain `timestamp_ms`; `merge` gains `cell_level`)
→ regen; `crates/loom-client/src/local.rs` (+`service.rs`) to thread the caller timestamp instead of
`now_ms()`; `crates/loom-mcp/src/writes.rs` + `lib.rs` (retire the two reject helpers for these);
`crates/loom-cli/src/remote.rs`; `crates/loom-mcp/src/server/tests.rs`; docs; tests.

**Acceptance.** Local and remote MCP produce **byte-identical** commit/tag/replay digests (and
`sql_commit` digest) for identical timestamped inputs; a live remote commit at a fixed `timestamp_ms`
yields the same digest as the local MCP commit; local behavior unchanged; fmt + clippy clean.

---

## 397 - `document_query` composite

**DECISION (owner): option 2 - host-assembles-over-primitives. IMPLEMENTED + unit-verified + LIVE-VERIFIED
owner-side (GREEN, 2026-07-13).** Did NOT relocate the MCP-specific `DocumentQueryResult` into loom-core, did NOT add a
combined IDL method, and kept `Document.query_json` unchanged. The one missing primitive was greenlit and
added: `string Store.digest_algo()` (stable name via `Algo::as_str`; new `Algo::from_name` parser shared
with `Digest::parse`; served by `LocalLoomClient::store_digest_algo` reading the superblock through a
lock-free `FileStore::open_read`; in `EXCLUDED`, not a tool). `read_document_query` now gathers the algo,
the full `Collection` (`document_list_binary`), and candidate ids (`query_json`/`find_json`/list) over remote,
then runs the SAME extracted `document_query_assemble` the local path runs, so per-item digests use the
store's REAL algorithm (no Blake3 assumption). New `RemoteMcpBackend` methods
`document_query_json`/`document_find_json`/`store_digest_algo` + `McpRemoteBackend` impls forward the
generated `Document::query_json`/`Document::find_json`/`Store::digest_algo`; the
`remote_host_composite_unsupported` reject helper is removed. Unit-verified
`remote_document_query_composite_forwards` + `algo_name_round_trips`; codegen `--check` in sync (306
methods); clippy clean. Live parity (remote==local document_query incl. per-item digest) asserted in the
omnibus test and LIVE-VERIFIED owner-side (GREEN, 2026-07-13).

**Context.** The MCP `document_query` read (`reads.rs::read_document_query`, ~2001-2109) is a host composite:
`document_list_binary` + candidate ids (predicate `doc_query` / index `doc_find` / list-all) + projection assembly +
per-item `Digest::hash(store().digest_algo(), doc)` + `id_prefix`/`cursor` pagination - not the single
generated `Document.query_json` method its `idl_method` nominally names.

**Option-2 feasibility (source-backed).** Reproducible over remote in the host:
- The collection is already available over remote - `document_list_binary` -> `Collection::decode`
  (reads.rs 3757). `Collection` (loom-core `document.rs` 21-85) holds `id -> doc bytes`, giving the
  list-all candidate ids (`iter`), doc bytes for `get`, `len`, `include_document`, and the JSON
  `projections` via `loom_core::doc_extract_index_value` on those bytes.
- `id_prefix`, `cursor`, paging, and `next_cursor` are pure host logic on the ids.
- Predicate-branch candidate ids (`loom_core::doc_query`) and index-branch candidate ids
  (`loom_core::doc_find`) are reproducible by wiring backend forwards to the existing generated
  `Document::query_json` / `Document::find_json` (client bindings present:
  `generated_client.rs` 4117 `find_json`, 4142 `query_json`) and parsing the returned ids - the
  `query_json`/`find_json` reuse the owner authorized under option 2.

**BLOCKER - one missing primitive.** Each item's `digest` is `Digest::hash(loom.store().digest_algo(), doc)`
(reads.rs 2020 sets `algo = loom.store().digest_algo()`; 2098 hashes RAW doc bytes). No shipped remote read
exposes the store's digest algorithm:
- `store_version` returns the static `loom_core::VERSION` (lib.rs 1288) - no algo.
- `store_capabilities` is the capability registry (lib.rs `served_capabilities` 52) - no algo.
- `store_blob_digest` is `Object::Blob(data).digest()` (reads.rs 998) = hardcoded `Algo::Blake3`
  (object.rs 310 `digest()` → 304 `digest_with(Algo::Blake3)` → hash of `canonical()`), i.e. the WRONG algo
  source AND the WRONG input (canonical object framing, not raw bytes).
- `Collection` carries no per-doc digest (document.rs 21-85).
Assuming Blake3 would be a silent parity loss for a `FileStore::create_with_profile(path, Algo::Sha256)`
store. Missing source op to expose: `loom_core::ObjectStore::digest_algo()` (loom-store `lib.rs` 3423).

**Recommended minimal fix (a primitive, not a combined method - consistent with option 2).** Add
`string digest_algo()` to IDL interface `Store`; wire codegen + the `service.rs` bridge (returns the
`loom.store().digest_algo()` name) + a `RemoteMcpBackend::store_digest_algo` forward. The host parses the
name → `loom_types::Algo` and computes `Digest::hash(algo, doc)` locally over the Collection bytes. This
adds one read-only primitive and reshapes nothing existing.

**Waiting On Decision.** Greenlight adding the `Store.digest_algo()` primitive (or name an alternative
source-backed way to obtain the store algorithm over remote). Once greenlit, 397 is implementable end to end
with no further contract questions.

**Files expected to change (once greenlit).** `idl/loom.idl` (`Store.digest_algo()`) → regen;
`crates/loom-client/src/service.rs` bridge; `crates/loom-mcp/src/lib.rs` (backend method + `document_query`
forward), `reads.rs` (host reassembly over remote primitives), `crates/loom-cli/src/remote.rs` (backend
impls for `store_digest_algo` + `document_query_json`/`document_find_json` id lookups);
`crates/loom-mcp/src/server/tests.rs`; docs; tests.

**Acceptance.** Local and remote MCP `document_query` produce identical `DocumentQueryResult` (including
per-item digest, cursor pagination, and projections) for identical inputs; live remote test proves it;
fmt + clippy clean.

---

## 398 - `timeseries_latest` timestamp parity

**DECISION (owner): option (A) - grow the existing `TimeSeries.latest` return payload. IMPLEMENTED**
(payload now the canonical `[ts, value]` pair via `loom_core::timeseries::latest_point_to_cbor`/
`latest_point_from_cbor`; IDL signature unchanged; server bridge + MCP backend + host wired; interim
reject retired; unit-verified `remote_ts_latest_forwards`). LIVE-VERIFIED owner-side (GREEN, 2026-07-13)
via the omnibus `mcp_kv_round_trip_through_remote_backend`.

**Question.** Should `TimeSeries.latest` carry the point timestamp in its return, or should a timestamped
variant be added?

**Context.** The MCP `timeseries_latest` tool returns the latest point **with its timestamp**, but the
IDL `latest` wire form returns only the value bytes, so a remote arm cannot reconstruct the timestamped
output. `timeseries_get`/`put`/`range` are already shipped over remote.

**Source references.**
- `crates/loom-mcp/src/reads.rs:2130` `read_timeseries_latest(ws, name) -> Result<Option<TsPoint>>`
  (`TsPoint` carries the timestamp).
- IDL drops it: `crates/loom-remote-protocol/src/generated_api.rs:1735`
  `TimeSeries::latest(handle, workspace, collection) -> Option<Vec<u8>>` (value bytes only).
- Rejects over remote today (unit `remote_ts_latest_is_unsupported_with_precise_error` + live assertion).

**Options.**
- **(A)** Grow `TimeSeries.latest` to return `(timestamp, value)` (or a `TsPoint` wire encoding).
- **(B)** Add a `latest_at`/`latest_point` variant carrying the timestamp; leave `latest` as-is.

**Recommendation.** (A) - the value-only `latest` has no other consumer that benefits from dropping the
timestamp; enriching it is the smallest change. (Lowest effort of the five; P2.)

**Consequence of deferring.** `timeseries_latest` stays local-only; a small, isolated gap. No parity claim
blocker beyond this one tool.

**Files expected to change.** `idl/loom.idl` (`TimeSeries.latest` return) → regen;
`crates/loom-client/src/local.rs` (+`service.rs`); `crates/loom-mcp/src/reads.rs` + `lib.rs`;
`crates/loom-cli/src/remote.rs`; `crates/loom-mcp/src/server/tests.rs`; docs; tests.

**Acceptance.** Local and remote MCP `timeseries_latest` produce byte-identical `TsPoint` output
(value + timestamp) for identical inputs; live remote test proves it; fmt + clippy clean.

---

## 399 - `sql_query` full-result parity

**DECISION (owner): option (A) - new read-only full-result IDL method. IMPLEMENTED** (added
`Sql.sql_query_result(handle, workspace, db, sql) -> bytes`; `LocalLoomClient::sql_query_result` runs
`exec_cbor` on an eager read overlay and rejects a transaction/dirtying statement, so nothing persists;
the streaming `sql_query` is unchanged; codegen re-run; MCP `read_sql_query` forwards; interim reject
removed; `sql_query_result` added to `EXCLUDED`). Unit-verified `remote_sql_query_forwards`; LIVE-VERIFIED
owner-side (GREEN, 2026-07-13) via the omnibus `mcp_kv_round_trip_through_remote_backend`.

**Question.** Should remote `sql_query` be served by a new read-only IDL method returning the full
`exec_cbor` result (no persist), or by enriching the `sql_query` stream to carry the full
statement/label structure?

**Context.** The MCP `sql_query` tool returns the full `exec_cbor` result -
`Array([Statement::Select{labels, rows}, ...])` with column labels and per-statement structure - and
enforces read-only by refusing to persist a dirty store. The IDL `sql_query` stream yields rows only for
the first SELECT, dropping labels and the statement wrapper; the only full-`exec_cbor` IDL path
(`sql_exec`) persists dirty changes, so it cannot serve a read-only query. Rejects over remote today.

**Source references.**
- MCP output: `crates/loom-mcp/src/reads.rs::read_sql_query` → `read_sql_query_cbor` →
  `LoomSqlStore::exec_cbor` (`crates/loom-sql/src/lib.rs:1058`), with read-only guards (reject if dirty).
- IDL stream is rows-only: `crates/loom-sql/src/lib.rs:1082` `select_rows_cbor`; the dropped fields are in
  `crates/loom-result/src/result_view.rs:40` `Statement::Select { labels, rows }`.
- IDL: `crates/loom-remote-protocol/src/generated_api.rs:1938` `Sql::sql_query(session, sql) -> LoomStream`.
- Reject helper: `crate::remote_sql_query_unsupported`.

**Options.**
- **(A)** New read-only IDL method returning the full `exec_cbor` payload without persisting (e.g.
  `Sql.sql_query_result`).
- **(B)** Enrich the `sql_query` stream to carry labels + full statement structure (per-item envelope),
  then reassemble host-side into `exec_cbor`.

**Recommendation.** (A) - a dedicated read-only full-result method is simpler and matches the tool's
single-payload output; (B) re-shapes the streaming contract for one consumer. Depends on the 396 SQL
session/timestamp decisions only insofar as it also opens a `SqlSession`; otherwise independent.

**Consequence of deferring.** `sql_query` stays local-only; the SQL family is `sql_exec` + SQL-read over
remote but not `sql_query`.

**Files expected to change.** `idl/loom.idl` (new read-only full-result method) → regen;
`crates/loom-client/src/local.rs` (+`service.rs`) - read snapshot, `exec_cbor`, reject-if-dirty, no
persist; `crates/loom-mcp/src/reads.rs` + `lib.rs`; `crates/loom-cli/src/remote.rs`;
`crates/loom-mcp/src/server/tests.rs`; docs; tests.

**Acceptance.** Local and remote MCP `sql_query` produce byte-identical `exec_cbor` (labels + statement
structure) for identical inputs, and remote `sql_query` never persists a mutation (read-only preserved);
live remote test proves it; fmt + clippy clean.

---

## 370 status summary

**Shipped + live-verified over remote:** KV, CAS, Queue, Ledger, TimeSeries (`get`/`put`/`range`), Search,
Columnar, Calendar, Contacts, Mail, FileSystem, Vector; document reads (`get`/`get_range`/`list`); the VCS
clean subset (reads + non-timestamped writes); the graph clean subset (all reads incl. `query`/`explain` +
node writes); the SQL-read group (`sql_read_table`/`_at`, `sql_index_scan`/`_at`, `sql_diff`,
`sql_table_diff`, `sql_blame`, `sql_list_databases`); the Dataframe group (`create`/`collect`/`preview`/
`materialize`/`plan_digest`/`source_digests`); the Watch group (`watch_subscribe`/`watch_poll`, batch wire
now carries `parent`); and `sql_exec` (per-request `SqlSession`). Classification: `HANDLE_STREAM_METHODS`
is empty - every IDL-backed tool is Unary and is either forwarded or rejected precisely in-method
(counts derived from `TOOL_SURFACE`: unary 171 / handle-stream 0 / local-only 111). `--stateless`+remote
is rejected.

**Explicitly rejected over remote today (precise errors, with tests):** document/graph ref-index writes
(`document.put_binary`/`delete`/`replace_text`, `graph.upsert_edge`/`remove_edge`) - task 395; the 9 timestamped
VCS writes and `sql_commit` - task 396; `document_query` composite - task 397; `timeseries_latest` - task
398; `sql_query` - task 399. Plus the 111 host/composite local-only tools and 3 substrate/studio resources.

**Remaining blockers:** the five contract decisions above (395-399). No non-contract remote-MCP work
remains.

**370 completion gate:** 370 **cannot** be marked Done until 395-399 are each either (a) implemented and
live-verified, (b) cut, or (c) explicitly moved out of 370 scope by owner decision. Until then 370 stays
**In Progress**.
