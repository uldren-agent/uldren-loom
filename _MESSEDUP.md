# Recovery Work Control

This file is the compact control plane for the recovery work. Detailed submissions live in
`specs/messedup/evidence/`. The complete pre-split record is preserved at
`specs/messedup/archive/_MESSEDUP_FULL_2026-07-30.md`.

Agents must update only their assigned task evidence file. They must not append evidence,
remediation passes, or results to this control file. The arbiter owns task statuses, assignments,
reviews, and dependency changes in this file.

## Current Review Queue

| Task | Agent | Status | Evidence |
|---|---:|---|---|
| MU-17j-l-a controlled store promotion | - | In Progress | The synthetic decoder passed, and copied Uldren then Matrix promotion passed with unchanged source SHA-256, preserved semantic roots, complete legacy page attribution, strict `LRT5` reopen, and full Loom registry reopen. Remaining closure is owner-approved active replacement, deletion of every legacy decoder and temporary migration authority, strict production reopen, and the ordered amplification gate. Evidence: `MU-17j-l-a-caller-conversion.md`, `MU-17j-l-a-mark-gc.md`, and `MU-17j-l-a-promotion.md`. |

## Current Migration Gate

| Task | Agent | Status | Blocker | Evidence |
|---|---:|---|---|---|
| REC-1 | - | Blocked | MU-12 and MU-17j-l must produce a buildable candidate with canonical roots and bounded growth. | Recovery candidate report |
| REC-2 | - | Remediation Required | The canonical RegionTable now persists a monotonic minimum recoverable generation, and foreground, maintenance, mark-epoch, GC, and migration reuse paths require that horizon together with reader and epoch fences. Focused encoding, reopen, torn-commit, reader-lease, and allocator-fence tests pass. Native ticket create/update and durable delivery now use bounded engine-state planning, with focused no-full-export/import, interruption, delivery, and reopen proof. The latest first ticket-create endpoint is 4,456,448 online bytes for 851,968 compacted bytes against a 3,407,872-byte ceiling. Task 185 remains open because the new physical advisory path does not yet provide cursor-, page-, candidate-, and byte-bounded specialized consolidation through the maintenance transaction authority. Per-operation latency still has no source-derived ceiling, and ticket overwrite, page create, page overwrite, lane overwrite, and document overwrite must pass their exact endpoint gates after ticket create is fixed. | `specs/messedup/evidence/MU-17j-l-a.md`; `specs/messedup/evidence/MU-17j-l-amplification-contract-audit.md`; `specs/messedup/evidence/MU-17j-l-gc-quarantine-audit.md`; `specs/messedup/evidence/MU-17j-l-task185-reconstruction.md`; `crates/loom-store/src/page.rs`; `crates/loom-store/src/record_io.rs`; `crates/loom-store/src/mark_epoch.rs`; `crates/loom-store/src/compact.rs`; `crates/loom-mcp/tests/storage_amplification.rs`; `_QUEUE_0071.md` Tasks 110aa, 185, 194, 197, and 199d |
| REC-3 | 2 | Done | Authenticated CLI, daemon, and MCP execution proved coherent ticket, lane, page, and document reads and writes across restart. | `specs/messedup/evidence/REC-3.md` |
| REC-4 | - | Blocked | REC-1 through REC-3 must pass before copied Uldren and Matrix stores are exercised. | Copied-store validation report |
| REC-5 | - | Blocked | REC-4 must pass before live MCP activation, T188-24 cleanup, T188-25 hydration proof, and repeat validation. | Live activation and post-cleanup report |

Crash/amplification test-contract review: the diagnostic retains the required six-workload order and
checks exact reopened state for every workload. Generation, physical bytes/pages, active/live/stale
tree pages, free-map pages, and per-window latency come from store authorities. Physical trend gates
use only structural page allowances. The ignored acceptance test now fails explicitly after collecting
metrics while the latency ceiling or document-overwrite physical ceiling remains unresolved. The
checkpoint-fsync test compares the reopened result with an independently completed successor from
identical prior bytes, including exact roots, free runs, object count, page count, and object content.

## Active Assignments

| Agent | Current Task | Next Task | Operating Rule |
|---|---|---|---|
| 1 | - | Idle | Controlled promotion source audit is complete. |
| 2 | - | Idle | Skeptical caller and mark/GC reviews are accepted. |
| 3 | - | Idle | Temporary promoter assignment ended; the arbiter completed copied-store proof. |

## Ordered Incomplete Work

| Task | Parent | Status | Lift | Dependency | Outcome |
|---|---|---|---:|---|---|
| MU-12 | Task 188 | In Progress | 4 | MU-11c | Complete the canonical RegionTable and fixed root-catalog implementation tracked by the Task 188 queue. |
| T188-24a | MU-12 | Done | 3 | T188-24 inventory | All newly published RegionTable states use canonical LRT4, including object-only state; legacy v2/v3 readers remain until REC-5. |
| T188-24 | MU-12 | Blocked | 3 | MU-15, MU-7, live MCP smoke | Remove temporary source-layout migration and fallback code only after rebuilt CLI, daemon, and MCP health is proven against both activated stores; delete migration-only task-named tests and rename every retained test/helper by durable behavior. |
| T188-25 | MU-12 | Planned | 3 | T188-24 | Prove fresh and migrated stores open with bounded current-state hydration and zero retained/control payload hydration. |
| MU-17j-l-a | MU-17j-l | In Progress | 6 | MU-17j-k, MU-17j-l-b | Replace the dedicated one-page-per-free-extent value layout with the approved inline fixed-size extent value B-tree design. Preserve existing `RecordLoc` bytes and tags while adding one crate-private typed value-codec authority; pin the new extent bytes and fail-closed decoder; reuse the side-effect-free prepared mutation algorithm; switch free-map reads, planning, publication, traversal, and reclamation to inline values; remove value blobs, placeholder locators, rebinding, per-locator frees, and `dedicated_extent_value_pages`; perform one controlled offline promotion of both active stores; then remove migration-only code. The package closes only after byte vectors, mixed-depth copy-on-write, crash/reopen, disjointness, mark/GC, reader-lease, cardinality, fail-before-write, and ordered workload gates pass. Source-backed design and measured attribution are in `MU-17j-l-a.md`. |
| MU-17j-l-b | MU-17j-l | Done | 6 | MU-17j-k | Closed B-tree mutation as one package: the immutable prepared delete/upsert delta owns topology decisions, consuming apply validates pinned source identity, all mutation adapters use the shared authority, and a real workflow publication failure preserves live and reopened authoritative state. Evidence remains in `MU-17j-l-b-review.md` and `MU-17j-l-b1.md`. |
| MU-17j-l-c | MU-17j-l | Done | 6 | MU-17j-l-b | Closed publication callers as one package: segment GC and tail compaction preserve authoritative state across injected pre-publication failure; audit-retention deletes precede one shared put batch; live and reopened state is proven; and no production multi-record publisher uses repeated one-key insertion. Evidence remains in `MU-17j-l-c-review.md`, `MU-17j-l-c1.md`, and `MU-17j-l-c.md`. |
| MU-17j-l-d | MU-17j-l | Blocked | 3 | MU-17j-l-a, MU-17j-l-b, MU-17j-l-c | Run the final gate as one package: ticket create first, then ticket overwrite, page create, and page overwrite; run the relevant store suite and bounded hot/random probes only after all four pass. At each endpoint, create workloads must stay within `max(4 * compacted_bytes, compacted_bytes + 1 MiB)` and overwrite workloads within `max(3 * compacted_bytes, compacted_bytes + 512 KiB)`; reusable plus reclaimable bytes and stale acceptance bytes must be disjoint and within their specified shares. |
| MU-17j-l | MU-17j | Remediation Required | 2 | MU-17j-l-a, MU-17j-l-b, MU-17j-l-c, MU-17j-l-d | Close storage amplification only after all four packages pass; do not add implementation-step or remediation child tasks beneath this parent. |
| REC-1 | Recovery | Blocked | 3 | MU-12, T188-24a, MU-17j-l | Build one release candidate only after every new RegionTable publication is canonical and operational growth is bounded. |
| REC-2 | Recovery | Remediation Required | 4 | MU-17j-l | Prove repeated ticket, lane, page, and document writes have stable latency and exact endpoint bounded physical growth. Current first failure: ticket create is 10,866,688 online bytes for 770,048 compacted bytes against a 3,080,192-byte ceiling. |
| REC-3 | Recovery | Done | 4 | MU-12 | Authenticated CLI, daemon, and MCP share coherent ticket, lane, page, and document state without process-local staleness, including conditional conflicts and daemon restart. |
| REC-4 | Recovery | Blocked | 4 | REC-1, REC-2, REC-3 | Validate logical identity, reads, writes, reopen, and bounded growth first on a copied Uldren store and then on a copied Matrix store. |
| REC-5 | Recovery | Blocked | 4 | REC-4 | Activate Uldren and Matrix MCP, repeat live reads and writes, complete T188-24 cleanup only after proof, complete T188-25 bounded-hydration proof, rebuild, and repeat the complete smoke. |

## Transferred Work Ledger

No task below was deleted. Each task now has an explicit owning row in `_QUEUE_0071.md`; its
existing evidence remains under `specs/messedup/evidence/`.

| Former recovery task | Owning 0071 task | Preserved state |
|---|---|---|
| MU-17g | 190 | In Progress |
| MU-17g-g | 190a | Remediation Required |
| MU-17h-c1 | 193c1 | Done |
| MU-17h-c2 | 193c2 | Done |
| MU-17h-d1 | 193d1 | Planned |
| MU-17h-d2 | 193d2 | Planned |
| MU-17h-d3 | 193d3 | Planned |
| MU-17h-e | 193e | Planned |
| MU-17h-f | 193f | Planned |
| MU-17h-g | 193g | Planned |
| MU-17h-h | 193h | Planned |
| MU-17h-i | 193i | Planned |
| MU-17h-j | 193j | Planned |
| MU-17h-k | 193k | Planned |
| MU-17h | 193p | Planned |
| MU-18 | 193 | Blocked |
| MU-19 | 178 | Blocked |
| MU-20 | 180 | Blocked |

## Accepted State

Detailed accepted history remains in the archive. The following immediate dependencies are accepted:

- MU-17h-c1: every currently exposed generated or store-semantic JSON selector in Vector, FTS, and Pages has field-preserving direct-local, daemon-local, and hosted-remote parity; Workspace, Files, and Document expose no JSON selector, and the accepted process-local Vector text and FTS rebuild owners remain explicit source-backed exceptions.
- MU-17h-c2: generated store-backed Inference instance JSON selectors have field-preserving three-target parity; Identity, ACL, ProtectedRef, and Management expose no JSON selector, and external model, cache, and hardware operations remain explicit source-backed exceptions.

- MU-17g-g-b: Meetings source payload reads use the generated read-only CLI boundary and an authenticated LocalLoomClient semantic owner. Exact bytes, invalid leaves, absent payloads, authorization order, and direct-local, daemon-local, and hosted-remote behavior are proven.
- MU-17g-f, MU-17g-f5, and MU-17g-f5a: the remaining operational families are classified and closed. Locks use one injectable authority across generated, daemon, MCP, and hosted entry points; logical-session expiry and bounded idempotent credential rotation are proven.
- MU-17g-f5a-c: public CLI Locks route through generated operations; hosted dispatch prunes expired owners; failed credential-file replacement remains retryable; concurrent current and pending replays return byte-identical live successors; restart fails closed.
- MU-17g-g-d: all seven Drive read leaves use their existing generated methods through the read-only CLI boundary. Existing text and JSON presentation, raw bytes, authorization, empty results, absent errors, and three-target parity are preserved.
- MU-17h-b2: the existing E2 fixture proves representative Chat and Drive default-text success, stale compare conflict, empty output, absent output, raw bytes, JSON decoding, and three-target parity without a duplicate harness.
- MU-17j-k: engine-state import preserves hot mutable-overlay state at the core contract. Core, Pages, and all three fresh release probes pass without cross-facet lane loss or the observed first-ticket owner-token conflict.
- MU-17h-b3: Lifecycle and Refs representative default-text success, error, and empty output match across direct-local, daemon-local, and hosted-remote execution without weakening the existing JSON report.
- MU-17g-g-c: all five Chat read leaves use generated semantic owners, Chat edit forwards the optional expected entity tag without changing omission behavior, byte bodies remain intact, and direct-local, daemon-local, and hosted-remote parity passes.
- MU-17g-g-a1: the 55 zero-caller StoreClient methods, causal dead encoders, stale response adapters, and obsolete registry-read chain are removed; the corrected immutable-read guard positively enforces current generated owners while retaining active transfer and generated adapters.
- MU-17h-b1: Tickets and Lanes default-text success, conflict, deletion, filtered absence, and not-found presentation match across direct-local, daemon-local, and hosted-remote execution without sorting or semantic-field replacement.
- MU-17g-f5a-a: Locks derive ownership from authenticated sessions, use one injectable semantic authority, publish or roll back every coordinator mutation including error outcomes, and retry bounded waits at natural lease expiration.
- MU-17g-f5a-b: generated dispatch and daemon IPC share one durable Locks authority for acquire, refresh, release, administrative break, and fenced-write application; publication failure restores exact prior state and restart preserves fence high-water state without restoring runtime holders.
- MU-17h-a: the cross-target CLI audit verifies actual report invocations across accepted families, identifies missing text/JSON/error/absence coverage, flags four semantic over-normalizations, and orders remediation by shared fixture reuse.
- MU-17g-g-a: the legacy authority audit identifies 55 zero-caller StoreClient methods and stale adapters, preserves active transfer exceptions, separates the unimplemented Meetings source-read owner from 12 implemented Chat/Drive generated reads, and provides dependency-ordered cleanup.
- MU-17g-f4: Serve and Daemon leaves have exhaustive ownership and all generated state leaves have bounded cross-target parity. The serve daemon uses one shared engine for generated dispatch and pure-ephemeral KV, maintenance preserves ephemeral state without bypassing reclamation leases, and the non-serve fallback is conditionally compiled without task-specific warnings.
- MU-17g-f2: Lifecycle, Refs, Exec, and Interchange generated-capable leaves have exhaustive ownership and three-mode parity. Interchange dry runs use the read-only generated session path and preserve the exact store bytes, length, mtime, and mutable-overlay generation in all four strict fixtures.
- MU-17g-f3: Studio, VCS, and Inference have exhaustive leaf ownership. Inference instance list/get are explicit generated read contracts, Studio catalog delegates to the store-independent substrate authority, VCS reads use generated methods, and focused direct-local, daemon-local, and hosted-remote parity passes.
- MU-17j: the sustained-growth remediation parent is complete. Snapshot-at-the-beginning reachability, validated reclaim evidence, reader leases, bounded metadata publication, logical-mutation coalescing, and sustained overwrite/append acceptance are source-backed by accepted child evidence.
- MU-17j-g: logical mutation publication coalescing is complete across ticket, page, document, and lane paths with one authoritative workflow publication and no outer save.
- MU-17j-h: physical metadata amplification work is complete with path-local current-record updates, persistent extent-tree publication, bounded metadata traversal, and increasing-cardinality growth proofs.
- MU-17g-f1: all Audit, Certificate, and NetworkAccess leaves use generated semantic owners; nested Audit config arms are independently enforced, and focused cross-target security-administration parity passes.
- MU-17g-e2: all 31 Chat and Drive mutation leaves map independently to generated semantic owners, reject legacy store authority, and pass direct-local, daemon-local, and hosted-remote parity.
- MU-17j-e: sustained overwrite, append, and external-reader diagnostics pass with bounded generations, pages, maintenance residue, latency, and throughput after repairing mark-budget publication, reclaim-index cleanup, metadata evidence, page classification, free-map extent replacement, and epoch completion.
- MU-17g-e1: SQL execution and Meetings import use generated semantic owners with exhaustive source guards and direct-local, daemon-local, and hosted-remote parity.
- MU-17g-d5: all Program, Metrics, Logs, and Traces CLI leaves use generated semantic owners with exhaustive source guards and direct-local, daemon-local, and hosted-remote parity.
- MU-17j-h-c: increasing-cardinality mutation proofs pass at the accepted 24-page ceiling, with thread-local instrumentation, no full-tree enumeration, and source-attributed delayed-GC metadata ownership.
- MU-17g-d4: Management and Store generated-capable leaves preserve full durability-policy and passphrase/raw-KEK rekey semantics through typed StoreAdmin contracts across direct-local, daemon-local, and hosted-remote execution. Generated artifacts are current at 527 methods and focused parity passes.
- MU-17g-d3: Tickets and Lanes synchronize only successful CommitReceipt targets after publication. Complete mutable-overlay enumeration was removed from the mutation path, instrumented as zero in focused tests, and ordered direct-local, daemon-local, and hosted-remote parity passes.
- MU-17g-d2: all 27 Identity, ACL, and ProtectedRef leaves use generated ownership. Typed authority witness and replication policy reads are complete across IDL, wire, local client, remote client, and hosted dispatch; real authenticated hosted denial passes.
- MU-17j-h-b: persistent extent-tree publication and metadata reachability traversal remain bounded, resumable, reader-safe, and reopen-safe. Both historical v8 epoch layouts are validated as complete records and safely restart obsolete metadata traversal; the 13-test focused suite passes.
- MU-17g-d1: Files, Workspace, Document, and Pages generated-capable CLI leaves use typed generated clients. `Document.get_text` uses its canonical result contract, and ordered real-binary parity passes across direct-local, daemon-local, and hosted-remote execution without omitting mismatching operations.

- MU-17g-a: all 27 CAS, KV, Queue, QueueConsumers, TimeSeries, and Ledger CLI leaves route through typed generated clients. A shared real-binary report passed across direct-local, daemon-local, and hosted remote execution, including binary output adapters and representative absent/false behavior.
- MU-17g-b: all 57 Graph, Vector, FTS/Search, Columnar, and Dataframe leaves are reconciled. Generated-capable store operations use typed generated clients; local embedding, Tantivy rebuild, host-format exports, and unified diagnostic search remain explicit bounded runtime/read exceptions. Exhaustive source enforcement and recorded real-binary direct-local, daemon-local, and hosted-remote parity passed.
- MU-17g-c: all 35 Calendar, Contacts, and Mail leaves use typed generated clients with preserved output and missing-record behavior. Exhaustive source enforcement and recorded real-binary direct-local, daemon-local, and hosted-remote parity passed.
- MU-17j-g-c1: ticket update preparation is isolated from the live FileStore and publishes through one WorkflowTransaction. Focused proof passes at one overlay generation and two non-checkpoint fsyncs with reopen, stale-token, and injected-failure coverage.
- MU-17j-g-c2: page publication preparation is isolated from the live FileStore and publishes through one WorkflowTransaction. Focused proof passes at one overlay generation and two non-checkpoint fsyncs with reopen, stale-token, interruption, operation, revision, and reference coverage.
- MU-17j-g-c3: LocalLoomClient and persistent MCP document text/binary writes share one loom-client planning/publication authority. Focused proof passes at one generation and two non-checkpoint fsyncs with reopen, stale-token, and injected pre-commit rollback coverage.
- MU-17j-g-c and MU-17j-g-c4: representative ticket, page, lane, and document mutations are guarded by shared publication observation plus exact one-generation/two-fsync enforcement. Rejected mutations publish nothing, and reopened state remains complete.
- MU-17j-g-b1: workflow transactions and persisted idempotency receipts share canonical per-field and 2 MiB aggregate byte limits. Validation rejects over-budget requests before mutation, decoding rejects corrupt over-budget records before aggregate allocation, exact-limit receipts round-trip, one-byte-over receipts fail, and legacy receipt prefixes remain readable.
- MU-17f: the current CLI inventory reconciles all 463 accepted executable leaves exactly once across MU-17g-a through MU-17g-f, with zero missing or duplicate ownership. Runtime, diagnostic, local-context, external-cache, static, and physical-file exceptions are explicitly owned, and the single post-audit feature-gated diagnostic leaf is recorded outside the accepted inventory.
- MU-17j-h-a: canonical current-record publication performs path-local updates without full legacy-overlay traversal when canonical roots own state, preserves unrelated root-family identities, and reuses an unchanged validated root-catalog page. Independent scaling proof remained bounded at 8, 8, 8, and 10 data pages for cardinalities 1, 8, 32, and 64 and passed reopen verification.
- MU-17j-g-b2: generated local execution and persistent MCP now share canonical page update, page publish, document text put, and lane update operations. Persistent MCP uses its existing Loom handle without path reopen; per-request MCP uses LocalLoomClient. Focused parity tests prove successful responses, stale compare failures, invalid-update failures, and no fallback publication.
- MU-17j-c and MU-17j-c-c: completed reachability epochs persist identity-bound reclaim evidence, protect post-snapshot pages, resume bounded traversal, preserve reclaimed free-map entries, and keep epoch/fence state recoverable across clear-publication failure and reader-lease races. Independent focused runs passed 11 validated-GC and 11 mark-epoch tests.
- MU-17j-d and MU-17j-d-b: delivery payloads are private content owned through retained delivery envelopes rather than public CAS promotion. Focused core and hosted Kafka tests prove replay, deduplication, shared ownership, acknowledgement without release, low-water pruning, stream deletion, hosted topic deletion, reopen behavior, public CAS absence, and persisted low-water reachability traversal.
- MU-7
- MU-17j-a: reproduced and attributed hot, random, and speed probe growth. Hot writes produced a 739,045,376-byte store with about 600 MB of stale pages but only 20 KB reusable; maintenance reported `mark_epoch_stale`, an incomplete 256-object epoch, and a 10-second slice interval. Random remains an eight-iteration fixed workload and is not throughput-comparable.
- MU-17j-b: defined generation-stable snapshot-at-the-beginning reachability with a persisted page high-water mark, allocator reclaim fence, conservative post-snapshot retention, epoch-bound reclaim evidence, crash resume, prompt bounded slices, and reader-lease integration.
- MU-17j-d-a: proved ticket and lane durable-delivery notifications create public CAS payload paths with no release coupling on acknowledgement, low-water advancement, stream deletion, or topic deletion. Page and document mutations do not emit equivalent delivery payloads. Lane `operation_root` is payload metadata rather than a promoted GC root, and `FacetWrite.audit` is transaction intent rather than durable audit evidence without an explicit owner-state audit.
- MU-17j-f-b: measured and attributed page update and page publish. Each operation published two generations; the outer full-state save is redundant, and backing-file size remained stable while committed logical page span changed through free-page reuse.
- MU-17j-f-c: separated generated CLI and local MCP document overwrite behavior. Generated CLI publishes one workflow-owner-state generation; local MCP adds a redundant outer full-state save. Focused proof measured logical page span and backing-file bytes independently.
- MU-17j-f-d: separated generated CLI and local MCP lane update behavior. Generated CLI publishes one canonical workflow generation; local MCP adds a redundant outer full-state save. A reduced committed logical page span did not shrink the backing file.
- MU-17j-f-a: attributed the measured ticket update's 11 generations to nine intermediate object publications, one semantic workflow publication, and one redundant outer full-state save. The evidence identifies exact publication calls and separates required semantic state from avoidable intermediate durability boundaries.
- MU-17j-f-e: consolidated ticket, page, document, and lane publication costs across generated CLI and local MCP execution. The accepted matrix separates intermediate publications, semantic workflow commits, outer saves, fsync boundaries, measured logical and backing-file deltas, required semantic state, avoidable boundaries, and shared metadata costs. MU-17j-f is complete.
- MU-17j-c-a: persists a coherent snapshot identity containing the committed generation, traversable digest roots, page high-water mark, and reclaim-fence identity. Epoch publication uses the captured fence during allocation and activates the in-memory fence before releasing serialized writer authority. Focused persistence, reopen, concurrent-write, pre-publication failure, and post-commit recovery tests passed.
- MU-17j-c-b: bounded slices now resume exclusively from persisted epoch queues and cursors without invalidation when current reference, control, canonical, or derived roots advance. Post-snapshot pages are conservatively retained by the GC candidate classifier, and focused reopen, foreground-write, and monotonic-progress tests passed.
- MU-17j-g-a: locks WorkflowTransaction as the only logical-mutation request, CommitReceipt as the only authoritative receipt, and typed LocalLoomClient semantic methods as the shared generated CLI and local MCP path. Preparation is side-effect isolated, one successful mutation reaches finish_txn once, and no outer save follows publication.
- MU-17i
- MU-17i-a
- MU-17i-b
- MU-17i-c
- MU-17i-d
- MU-17i-e: native read-only store handles hold a shared cross-process reclamation lease. Writers reuse free pages and physical compaction proceeds only under the exclusive lease; ordinary commits remain append-only while readers are active. Store diagnostics report blocker presence, and same-process tail trimming, canonical relocation, and child-process lease tests prove the boundary. The store unit suite passed after correcting one stale diagnostics fixture, and the daemon generated-write maintenance regression passed.
- MU-7f
- MU-6h-n-b-d
- MU-6h-n-b-e
- MU-6h-n-b-g
- MU-6h-n-b-h
- MU-6h-n-b-i
- MU-6h-n-b-j
- MU-6h-n-b
- MU-6h-n-c-a
- MU-6h-n-c-h
- MU-6h-n-c-i
- MU-6i-c2
- MU-6i-c3
- MU-6i-c4
- MU-6i-c
- MU-6i-d2
- MU-6i-d3
- MU-6i-d4
- MU-6i-d5
- MU-6i-d6
- MU-6i-d7
- MU-6i-d8
- MU-6i-d9
- MU-6i-e
- MU-6i
- MU-6h-n-c-v
- MU-6h-n-c-j-c
- MU-6h-n-c-j-d
- MU-6h-n-c-j-e
- MU-15d-p
- MU-6h-n-c-c
- MU-6h-n-b-f
- MU-6h-n-b-g implementation (focused executable proof retained as MU-6h-n-b-g-v)
- MU-6h-n-d-a2-b
- MU-6h-n-d-a2-c
- MU-6h-n-d-a2-d
- MU-6h-n-d-a2
- MU-6h-n-f
- MU-6h-m
- MU-6h-n
- MU-6h
- MU-6i-a
- MU-6i-b
- MU-6i-c1
- MU-6i-d1
- MU-6j-a
- MU-6i-d
- MU-16a
- MU-6h-n-d-a
- MU-6h-n-d-b
- MU-6h-n-d-c
- MU-6h-n-d-d
- MU-6h-n-d-e
- MU-6h-n-d-f
- MU-6h-n-d-g
- MU-6h-n-d-h
- MU-6h-n-d-i
- MU-6h-n-d-j
- MU-6h-n-d
- MU-6h-n-c-g
- MU-6h-n-e
- MU-14d-c
- MU-14d-c3
- MU-14d-c4
- MU-14d-c5
- MU-15b-smoke-d
- MU-15b-a
- MU-15b-b
- MU-15b
- MU-15b-smoke-a
- MU-15b-smoke-b
- MU-15b-smoke-c-a
- MU-15b-smoke-c-a-v
- MU-15b-smoke-c-b
- MU-15b-smoke-c
- MU-15b-smoke-e
- MU-15b-smoke
- MU-15c
- MU-6h-n-c-d
- MU-6h-n-c-e
- MU-6h-n-c-f
- MU-6h-n-c-j-a
- MU-6h-n-c-j-b
- MU-15d-s
- MU-6h-n-c-j-f
- MU-6h-n-c-j-g
- MU-6h-n-c-j-h
- MU-6h-n-c-j
- MU-6h-n-c
- MU-15d
- MU-7e
- MU-7f-a
- MU-7f-b
- MU-7f-c
- MU-17e

## Decision Points

None.
