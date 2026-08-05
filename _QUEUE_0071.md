# Queue 0071 - Performant Mutable Overlay Substrate

This is the active working queue for the 0071 mutable overlay substrate delivery track. It is separate
from `specs/IMPLEMENTATION-PLAN.md`; do not use this file as a historical changelog.

## Goal

Deliver the 0071 target architecture: a performant hot mutable substrate with configurable durability
modes, MVCC reads, multi-facet transactions, bounded storage growth, explicit VCS/CAS promotion, and
source-backed diagnostics.

Queue type: Implementation

## Definition Of Done

Queue 0071 is complete when operational facets use the shared mutable substrate for hot current state,
durability modes are implemented and configurable, random-new-item write amplification is reduced to a
reasonable source-backed bound, VCS/CAS promotion is explicit, and completion evidence is satisfied.

## Completion State

Current state: In Progress

Current cursor: recovery gates REC-1 through REC-5 in `_MESSEDUP.md`
Next task: restore a validated CLI, daemon, and MCP candidate before resuming transferred Task 190a
or Task 193 parity work.

Decision Points: none.

## No Buried Work Rule

Before every status update, pause point, handoff, or final control-return message, audit the response
for future-tense work, prevention work, risks, blockers, follow-ups, or "should do next" statements.

If the response mentions work that is not already represented in the queue, do one of these before
handing off:

- Add it to the Active Focus Window and Ordered Task List when it is in current scope.
- Add it to Missed Or Hidden Work Found when it needs user vetting before promotion.
- Add it to Decision Points when user choice blocks the next action.
- Move it to another queue or planning document when it belongs outside this queue.

Do not describe future work only in chat. If it matters enough to mention, it must be represented in
the queue before control returns to the user.

## Decision Log

| Date | Decision | Rationale | Source |
| --- | --- | --- | --- |
| 2026-07-22 | Loom uses a two-plane model: hot mutable current state plus explicit immutable CAS/VCS promotion. | Operational state must be performant and concurrent; CAS/Merkle history remains essential but must not be the default write path. | User decision and `specs/0071-mutable-overlay-substrate.md`. |
| 2026-07-22 | Durability modes are first-class: `strict`, `normal`, `relaxed`, and `ephemeral`. | Enterprise correctness requires configurable durability instead of forcing strict fsync on every hot mutation. | User decision and `specs/0071-mutable-overlay-substrate.md`. |
| 2026-07-22 | Page-class attribution is required before accepting storage-growth fixes. | Random new-item growth was dominated by stale record pages and reusable free pages, not user payload bytes. | MX-419 review evidence. |
| 2026-08-03 | Retain the 0071 two-plane, canonical-root, COW B-tree, generation-fenced reclamation, persistent-daemon, and explicit-promotion architecture. | The target remains suitable for a single-file embedded database; the implementation problem is fragmented publication authority and incremental remediation, not the two-plane model itself. | Owner architecture review and `specs/0071-mutable-overlay-substrate.md`. |
| 2026-08-03 | Consolidate every ordinary foreground mutation behind one typed prepared-publication authority before final performance acceptance. | Caller audits currently discover bypasses that should instead be impossible by construction. The prepared authority must own reservation, reusable runs, control update, reclamation lease, and commit consumption. | Tasks 199 through 199d. |

## Architecture Direction And Stop Conditions

The 0071 destination remains authoritative. Implementation work must converge on one database-style
transaction kernel rather than accumulating caller-specific reclamation, persistence, or recovery
mechanisms.

The following components remain target architecture:

- immutable CAS/VCS base plus a hot mutable overlay;
- typed current, retained, token, index, and control roots published through one canonical root set;
- copy-on-write B-tree snapshots with generation and reader fences;
- one persistent daemon execution authority shared by CLI and MCP;
- explicit VCS/CAS promotion rather than automatic versioning of every hot mutation;
- configurable durability, bounded maintenance, complete page attribution, and controlled migration.

Stop implementation and return to architecture review if closing storage growth requires any of:

- another persisted queue, cursor family, or commit path outside the canonical transaction kernel;
- caller-specific reclamation policy for tickets, lanes, pages, documents, or another ordinary facet;
- increasing an acceptance ceiling to fit observed bad growth;
- ordinary mutation code bypassing the prepared-publication authority;
- complete-state scans during ordinary point mutation or cold-open current-state hydration;
- unknown or unattributed physical pages;
- reclamation that cannot converge after the fixed recovery window;
- weakening rollback, reopen, reader fencing, compare-token, idempotency, audit, or retained-history semantics.

Internal structural budgets are necessary diagnostics but are not final 0071 acceptance. Task 193
must still enforce the specification's product-level online-to-compacted size ratios, stale-page
limits, stable throughput, semantic-preservation checklist, and copied/live recovery proof.

## Source Authority Order

When sources disagree, resolve them in this order unless the user says otherwise:

1. Current repo source.
2. User decisions in the active thread.
3. `specs/0071-mutable-overlay-substrate.md`.
4. Matrix tickets linked to this queue.
5. Generated artifacts.
6. Agent inference.

## Assumptions

| Assumption | Why acceptable | Revisit trigger |
| --- | --- | --- |
| `normal` durability can be the default for operational state. | This matches the user-approved design and common database practice: preserve correctness while batching durability. | Owner selects strict-by-default store semantics. |
| Random new-item write amplification should be fixed before broader facet rollout. | The problem is source-backed and blocks confidence in the mutable substrate. | MX-421 adds bounded regression and attribution guidance; final acceptance still depends on applying the MX-423 thresholds. |

## Current Source-Backed State

| Claim | Source |
| --- | --- |
| 0071 defines the target mutable overlay substrate and now includes durability modes, performance pillars, facet scope, and closure criteria. | `specs/0071-mutable-overlay-substrate.md` |
| Store transaction finalization currently has fsync-heavy strict publication behavior. | `crates/loom-store/src/record_io.rs` |
| Page-class attribution can identify stale record pages, reusable free pages, current record pages, metadata pages, and unknown gaps. | `crates/loom-store/src/lib.rs`, `crates/loom-cli/src/main.rs` |
| The random new-item probe creates new ticket, page, document, and lane bundles. | `scripts/loop-random-write-probe.sh` |
| The overwrite loop probe verifies repeated writes to the same logical records. | `scripts/loop-hot-write-probe.sh` |

## Scope Boundary

Queue 0071 includes:

- 0071 spec and task queue maintenance.
- Page-class attribution and storage-growth diagnostics.
- Random new-item write amplification reduction.
- Durability policy implementation.
- Group commit and WAL policy work.
- MVCC read snapshots.
- Shared multi-facet transaction API.
- Tickets, lanes, pages, and documents migration to shared hot mutable substrate.
- Explicit VCS/CAS promotion bridge.
- Source-backed tests and diagnostic probes.

Queue 0071 does not own:

- Full implementation of every future facet listed in 0071.
- Product-specific hosted compatibility surfaces unless they are required to prove shared persistence semantics.
- Release packaging.
- Long-tail derived index features beyond durability and rebuildability policy.

## Priority Definitions

- P0: Blocks the queue goal.
- P1: Required for a correct, durable result.
- P2: Valuable follow-up that must be completed or explicitly re-homed before this queue closes.
- P3: Long-tail, low-to-medium ROI, or distant feature work that can be deferred with little to no consequence.

## Lift Scale

- 1: Trivial.
- 2-3: Small and clear.
- 4-5: Moderate and bounded.
- 6-8: Large or ambiguous; try to split before starting.
- 9+: Too large for one task; split before starting.

## Research Notes

| Topic | Finding | Source |
| --- | --- | --- |
| SQLite durability | WAL mode can trade per-transaction sync for performance while preserving consistency under documented modes. | SQLite PRAGMA synchronous documentation. |
| InnoDB durability | Log flush policy is configurable; strict every-transaction flush is not the only enterprise mode. | MySQL InnoDB durability documentation. |
| Loom random growth | Eight random item bundles grew much larger online than compacted useful state. | Manual probe and MX-419 evidence. |

## Completion Evidence

| Evidence | Required? | Result | Notes |
| --- | --- | --- | --- |
| `cargo fmt --all --check` or narrower package formatting for touched crates. | Yes | Done for the 110 remediation | `just ci` passed formatting after the workspace-qualified revision-index remediation. |
| Final `just ci` batch gate for tasks 110w through 110z. | Yes | Done | After coherent-snapshot remediation, the one final batch gate passed formatting, workspace Clippy, default tests, focused hosted-PIM tests, and `cargo deny check`. |
| Manual `just test-performance` or equivalent target. | Yes | Done for MX-422 | Stays outside `just ci` and reports storage, time, latency, concurrency, JSON, and summary artifacts. |
| Design review gates for durability, MVCC, transaction API, promotion, and migration. | Yes | Done | Tasks 71, 81, 91, 121, and 149 are accepted with source-backed design-gate evidence. |
| Semantic preservation review gates after implementation batches. | Yes | Pending | Must prove performance work did not drop operation logs, audit, indexes, PEP, or public contracts. |
| Focused store tests for durability policy types. | Yes | Done for MX-425 | Proves `strict`, `normal`, `relaxed`, and `ephemeral` policy names, parsing, validation, and source-readable semantics. |
| Focused store tests for configurable durability behavior. | Yes | Done | Task 66 is accepted, and the final 110r `just ci` gate passed strict, normal, relaxed, ephemeral, recovery, idempotency, and group-commit coverage. |
| Focused store tests for MVCC snapshot reads. | Yes | Done for MX-443 | Proves document, ticket, lane, and page readers use provider snapshot routing. |
| Published-page live-session and reopen interruption tests. | Yes | Done for task 110u | `interrupted_publish_keeps_revision_and_references_at_prior_state` proves the page revision, reference index, projected graph, and revision index all remain at the prior state before the live engine is dropped and again after reopen. |
| Bounded transaction-planning instrumentation. | Yes | Done for tasks 110v through 110z | Core instrumentation counts full engine exports, imports, bounded plans, selected paths, rewritten sections, and unrelated rewrites. Page publication and MCP `substrate_transact` success/rejection tests prove no full engine export/import or unrelated section rewrite. `page_publish_excludes_unpublished_live_reference_state` and `substrate_transact_excludes_unpublished_live_reference_state` prove that unpublished live reference-index and graph mutations do not enter a transaction planned from the provider snapshot. The core bounded-planner tests prove read-only planning inputs can be loaded but cannot be changed. |
| Matrix revision-index migration equivalence. | Yes | Done for task 110q | `specs/0071-matrix-revision-migration-evidence.json` records read-only equality for workspace and scope `baa779e9-685e-4fb1-95cb-03ddb65eb030`: 2,880 revisions, 2,880 checkpoints, 1,728,568 canonical bytes, and digest `blake3:e58ea8c85d000dd692ac206bf20e2d8ac7932f17206652b839dea406742322eb` on both sides. The temporary verifier was deleted after success. |
| Random new-item probe with page-class attribution. | Yes | Done for MX-421 | Bounded default regression and manual probe attribution exist; final performance closure still needs threshold application. |
| Overwrite hot-loop probe. | Yes | Done for MX-418 | Keep as regression. |
| VCS promotion test. | Yes | Done | Tasks 122 through 127 are accepted; the final 110r core suite passed promotion, namespace-collision, migration-readiness, audit, ledger, sync, and export coverage. |
| Matrix development-store migration validation. | Yes | Done | The active replacement passed candidate freshness, readability, namespace, maintenance, ticket, lane, project-settings, VCS-cleanliness, and live MCP ticket/lane read-write checks. The legacy revision-index deletion is committed at `blake3:1f497d700be353a695509d5cacd488e3dff42d3ac27eb0bc9e78fd38521a41d4`; the workspace-qualified index retains 2,880 revisions and checkpoints. The owner accepted MX-489. |

## Task 152 Execution Evidence

Task 152 is complete. Matrix MCP and other Matrix writers remained stopped throughout the quiet
replacement window. `lsof matrix/matrix.loom` reported no open writer before candidate creation and
was checked again as an activation guard.

| Step | Command or artifact | Result |
| --- | --- | --- |
| Create candidate | `./matrix/loom store copy matrix/matrix.loom matrix/matrix.task152.candidate.loom --with compacted --format json --report-file matrix/matrix.task152.copy-report.json` | Passed. Candidate has 22,081 objects and 780,599,296 physical bytes. Source freshness recorded control root `blake3:482e8c0155a0722d1a958b680a393b37103a92332b1def7b80d9111f9a0bbc54`, reference root `blake3:3421791ae5296762cd666ad89de3b99de2df8451e7e8083ad1e87ad66fa1b3c2`, and ticket sequence 3,092. |
| Preflight | `./matrix/loom store preflight-replacement matrix/matrix.task152.candidate.loom matrix2 --live-store matrix/matrix.loom --candidate-report matrix/matrix.task152.copy-report.json --format json` | Passed with `ok=true` and `safe_to_replace=true`. Readability, freshness, 491 tickets, 7 lanes, maintenance, and namespace checks passed. Output saved to `matrix/matrix.task152.preflight-report.json`. |
| Dry run | `./matrix/loom store replace matrix/matrix.loom matrix/matrix.task152.candidate.loom matrix2 --candidate-report matrix/matrix.task152.copy-report.json --backup-store matrix/matrix.task152.rollback.loom --report-file matrix/matrix.task152.dry-run-report.json --dry-run --format json` | Passed without modifying the active path. |
| Activation | `test -z "$(lsof matrix/matrix.loom 2>/dev/null)" && ./matrix/loom store replace matrix/matrix.loom matrix/matrix.task152.candidate.loom matrix2 --candidate-report matrix/matrix.task152.copy-report.json --backup-store matrix/matrix.task152.rollback.loom --report-file matrix/matrix.task152.activation-report.json --format json` | Passed. The prior active store remains at `matrix/matrix.task152.rollback.loom`; the candidate is active at `matrix/matrix.loom`. |
| Active reads | `store stat`, `tickets list`, `lanes list --detailed`, `tickets project-settings-get --include-contracts`, and `doctor store` against `matrix/matrix.loom` | Passed. Tickets, lanes, project settings and contracts, maintenance state, workspace identity, and references are readable. |
| VCS cleanliness | Temporary read-only verifier calling `Loom::status` and `vcs_namespace_preflight` | Failed: `staged=0`, `unstaged=1`, `untracked=0`, `conflicts=0`. The only change is deletion of `.loom/substrate/revisions/baa779e9-685e-4fb1-95cb-03ddb65eb030.lri`. The verifier was deleted immediately after use and is not retained in source. |
| Artifact identity | `shasum -a 256 matrix/matrix.loom matrix/matrix.task152.candidate.loom matrix/matrix.task152.rollback.loom` | Active and candidate SHA-256 are `5d641008531023079496366c777f96c0c7818bf1138fd3529e8f2e610ab92411`; rollback SHA-256 is `48e679cf8c3c2ea2658cfa58250c14e70b3c5d37a7e466ba9022d632699e84f3`. |
| Migration baseline | `./matrix/loom vcs commit matrix/matrix.loom matrix2 --author loom-migration --message "Migrate revision index to mutable substrate"` | Passed with commit `blake3:1f497d700be353a695509d5cacd488e3dff42d3ac27eb0bc9e78fd38521a41d4`. |
| Post-baseline cleanliness and revision check | Temporary read-only verifier calling `Loom::status`, `vcs_namespace_preflight`, and `load_current_revision_index` | Passed: `staged=0`, `unstaged=0`, `untracked=0`, `conflicts=0`; 2,880 revisions; 2,880 checkpoints; 1,728,568 canonical bytes; digest `blake3:e58ea8c85d000dd692ac206bf20e2d8ac7932f17206652b839dea406742322eb`; legacy path absent. The verifier was deleted immediately after use. |
| Post-baseline candidate | `./matrix/loom store copy matrix/matrix.loom matrix/matrix.task152.postbaseline.candidate.loom --with compacted --format json --report-file matrix/matrix.task152.postbaseline.copy-report.json` | Passed. The source is 780,775,424 bytes and the compacted validation copy is 780,263,424 bytes. |
| Post-baseline replacement validation | `./matrix/loom store preflight-replacement matrix/matrix.task152.postbaseline.candidate.loom matrix2 --live-store matrix/matrix.loom --candidate-report matrix/matrix.task152.postbaseline.copy-report.json --format json` | Passed with `ok=true`, `safe_to_replace=true`, matching live freshness, 491 tickets, 7 lanes, clean namespace checks, and healthy maintenance state. Output is `matrix/matrix.task152.postbaseline.preflight-report.json`. |
| Post-baseline active reads | `store stat`, `tickets list`, `lanes list --detailed`, `tickets project-settings-get --include-contracts`, and `doctor store` against `matrix/matrix.loom` | Passed. Active branch tip is the migration baseline commit; tickets, lanes, project contracts, maintenance, references, and the workspace-qualified revision index remain readable. |
| Ticket evidence | `tickets comment-add ... MX-489 ... --comment-id task152-migration-baseline --comment-type progress --evidence ...` | Passed as ticket operation sequence 3,093 and profile root `blake3:8c471f38a90927526e94db92cab17eda9990491260338a41df2529fc1ab7697d`. |
| Live MCP reads | Matrix MCP `tickets_get` for MX-489 and `lanes_get` for Lane 3 | Passed. The activated store returned the ticket, comments, dependency relation, Lane membership, project fields, and status aggregation. |
| Live MCP atomic ticket and lane write | Matrix MCP `lanes_closeout` for MX-489 and Lane 3 | Passed. Ticket comment operation sequence 3,094 recorded `closeout_evidence`; Lane 3 persisted the Task 152 status report. |
| Live MCP ticket update | Matrix MCP `tickets_update` with `target_status=waiting_for_review` and structured review-request evidence | Passed as operation sequence 3,095 and profile root `blake3:c1d8ba1c512e20e1f960163a2928cb79626c6366ea8de4deaefbb3ceb788da18`. |
| Live MCP lane update and read-back | Matrix MCP `lanes_update`, followed by `tickets_get` and `lanes_get` | Passed. MX-489 reads as `waiting_for_review`; Lane 3 reports one waiting-for-review ticket, two ready tickets, and no status warnings. |

Source anchors for the controlled replacement contract are
`specs/0071-mutable-overlay-substrate.md:1126`,
`specs/0071-mutable-overlay-substrate.md:1135`, and
`specs/0071-mutable-overlay-substrate.md:1184`. Replacement and preflight implementation anchors are
`crates/loom-cli/src/main.rs:13377`, `crates/loom-cli/src/main.rs:13527`, and
`crates/loom-cli/src/main.rs:13827`.

## Ordered Task List

Current cursor: 190
Next task: Implement MX-495 for Task 190 using the accepted MX-494 generated CLI client selector.

Status values: Not Started, In Progress, Blocked, Waiting On Decision, Done, Cut.
Evidence types: Source, Test, Review, Artifact, User Decision, External.

| Order | Status | Priority | Lift | Task | Owning specs | Depends on | Output | Verification | User input needed |
| --- | --- | --- | ---: | --- | --- | --- | --- | --- | --- |
| 10 | Done | P0 | 3 | Rewrite 0071 around the two-plane architecture, durability modes, performance pillars, facet scope, and closure criteria. | `specs/0071-mutable-overlay-substrate.md` | None | Updated 0071 spec. | Review plus `git diff --check`. | No. |
| 20 | Done | P0 | 3 | Create this queue from `_QUEUE_TEMPLATE.md` and seed it with source-backed tasks. | `_QUEUE_0071.md` | 10 | Queue file. | Review plus `git diff --check`. | No. |
| 30 | Done | P0 | 5 | Add page-class store attribution for internal growth diagnosis. | 0071 | 10 | `loom store attribution` includes page classes. | MX-419 accepted. | No. |
| 40 | Done | P0 | 7 | Reduce transaction amplification for random new-item writes. Matrix ticket: MX-420. | 0071 | 30 | Storage write-path change that materially reduces stale record pages and reusable free pages for random new-item bundles. | MX-420 accepted after source review and focused page regression. | No. |
| 45 | Done | P0 | 5 | Add manual performance test harness and `just test-performance` target. Matrix ticket: MX-422. | 0071, `justfile` | 30 | Manual target that runs storage-growth, timing, latency, and concurrency probes outside `just ci`. | MX-422 accepted after source review and harness type-check. | No. |
| 47 | Done | P1 | 3 | Materialize the split 0071 queue rows as Matrix tickets when they become assignable. | 0071, Matrix tickets | 40, 45 | Concrete tickets with dependency relations, lane placement, and source-backed acceptance text. | Remaining 0071 tickets through MX-491 are materialized with dependency relations and lane placement. | No. |
| 50 | Done | P0 | 5 | Add random-new-item storage growth regression and probe guidance. Matrix ticket: MX-421. | 0071 | 30, 40, 45 | Bounded default regression plus larger manual performance probe. | MX-421 accepted. | No. |
| 52 | Done | P0 | 4 | Define performance acceptance thresholds and report schema for hot overwrite and random-new-item probes. Matrix ticket: MX-423. | 0071 | 45 | JSON schema and documented thresholds for physical bytes, compacted bytes, page classes, latency, and operation counts. | `specs/0071-mutable-overlay-substrate.md` schema and threshold review. | No. |
| 54 | Done | P0 | 4 | Add semantic-preservation acceptance checklist for storage performance work. Matrix ticket: MX-424. | 0071 | 40, 45 | Checklist requiring review of operation logs, audit records, revision indexes, reference indexes, compare tokens, idempotency, PEP, and public surfaces. | Delivered as specs/0071-mutable-overlay-substrate.md section 19.1 Semantic-preservation checklist (MX-424); source-anchored to the MX-420 regression. | No. |
| 60 | Done | P0 | 4 | Add durability policy types for `strict`, `normal`, `relaxed`, and `ephemeral`. Matrix ticket: MX-425. | 0071 | 10 | Core/store policy types with validation and source-documented semantics. | MX-425 accepted. | No. |
| 62 | Done | P0 | 5 | Persist store-level and facet-level durability policy configuration. Matrix ticket: MX-426. | 0071 | 60 | Policy storage with effective-policy resolution. | MX-426 accepted after source review and store policy tests. | No. |
| 64 | Done | P0 | 5 | Expose durability policy configuration through CLI and MCP without adding one-off policy surfaces. Matrix ticket: MX-427. | 0071 | 62 | Shared project/store settings commands or existing settings surfaces updated. | MX-427 accepted after source review and focused CLI/MCP checks. | No. |
| 66 | Done | P0 | 5 | Add durability-mode behavior tests for strict, normal, relaxed, and ephemeral writes. Matrix ticket: MX-428. | 0071 | 62 | Focused tests proving persistence and recovery expectations per mode. | MX-428 accepted after durability, idempotency, and no-default-features checks. | No. |
| 68 | Done | P0 | 5 | Add durable owner-token and idempotency-key indexes for overlay transactions. Matrix ticket: MX-429. | 0071 | 62 | Per-owner compare-token lookup and retry deduplication that survive reopen. | MX-429 accepted after source review and concurrent idempotency/publication tests. | No. |
| 70 | Done | P0 | 5 | Design the `normal` durability write path around WAL or group commit without weakening strict promotion boundaries. Matrix ticket: MX-430. | 0071 | 60, 62 | Source-backed design note in 0071 or queue comment plus implementation-ready acceptance criteria. | MX-430 accepted after review against `record_io.rs`, current group commit code, and 0071. | No. |
| 71 | Done | P0 | 4 | Run durability and group-commit design review gate. Matrix ticket: MX-431. | 0071 | 60, 62, 70 | Accepted design review proving durability semantics, crash windows, and strict boundaries are implementation-ready. | MX-431 accepted after source-backed gate review and focused durability policy checks. | No. |
| 72 | Done | P0 | 4 | Add hot mutable commit-queue data structures for `normal` durability. Matrix ticket: MX-432. | 0071 | 71 | Queue records eligible hot mutable transactions without publishing torn state. | MX-432 accepted after source review and focused queue tests. | No. |
| 73 | Done | P0 | 5 | Implement the `normal` durability group-commit publisher and fsync policy. Matrix ticket: MX-433. | 0071 | 72 | Publisher drains eligible queued commits and batches fsync while preserving strict boundaries. | MX-433 accepted after source re-review and focused public-contention batching test. | No. |
| 74 | Done | P0 | 5 | Add crash and reopen recovery tests for grouped hot mutable writes. Matrix ticket: MX-434. | 0071 | 73 | Tests that prove committed current state survives reopen and incomplete batches do not publish torn state. | MX-434 accepted after source review and focused recovery tests. | No. |
| 76 | Done | P1 | 4 | Report group commit batch size, fsync latency, and write-lock wait time in diagnostics. Matrix ticket: MX-435. | 0071 | 73 | Store status or doctor visibility for performance bottlenecks. | MX-435 accepted after source review, focused store test, CLI check, MCP feature check, fmt check, and diff check. | No. |
| 78 | Done | P0 | 4 | Define mutable-page reclaim eligibility for superseded current records. Matrix ticket: MX-436. | 0071 | 68, 73 | Rules for reclaim blockers: pinned snapshots, retained history, audit retention, and current index visibility. | MX-436 accepted after source review and focused eligibility tests. | No. |
| 79 | Done | P0 | 5 | Reuse eligible mutable pages through allocator and page-class accounting. Matrix ticket: MX-437. | 0071 | 78, 82, 83 | Reusable pages return to the allocator and attribution reports current, stale, and reusable classes accurately. | Accepted after source review, focused reclaim/page-class tests, no-default-features check, and `git diff --check`. | No. |
| 80 | Done | P0 | 5 | Define MVCC snapshot handle semantics for overlay generation plus immutable base root. Matrix ticket: MX-440. | 0071 | 60 | Snapshot API contract, pin lifecycle, and stale-reader behavior. | MX-440 accepted after source-backed spec review. | No. |
| 81 | Done | P0 | 4 | Run MVCC and reclamation design review gate. Matrix ticket: MX-441. | 0071 | 78, 80 | Accepted design review proving snapshot pinning, stale-reader behavior, tombstone retention, and reclaim blockers are implementation-ready. | MX-441 accepted after source-backed design-gate review. | No. |
| 82 | Done | P0 | 4 | Add MVCC snapshot storage API over overlay generation and immutable base root. Matrix ticket: MX-442. | 0071 | 81 | Snapshot handles can be opened, pinned, inspected, and released. | Accepted after source review and `cargo test -p uldren-loom-store mvcc_snapshot_ --no-default-features`. | No. |
| 83 | Done | P0 | 5 | Route mutable overlay read paths through MVCC snapshots. Matrix ticket: MX-443. | 0071 | 82 | Readers see a stable generation while writers continue. | Accepted after remediation routed production document, lane, ticket, and page reads through the provider snapshot route and focused tests passed. | No. |
| 84 | Done | P1 | 4 | Add pinned-reader and snapshot diagnostics. Matrix ticket: MX-444. | 0071 | 82, 83 | Diagnostics for active snapshot count, oldest pinned generation, and retention pressure. | Accepted after source review, `cargo test -p uldren-loom-store store_maintenance_report_surfaces_mvcc_snapshot_diagnostics`, and `cargo build -p uldren-loom-cli`. | No. |
| 86 | Done | P0 | 5 | Implement tombstone retention and reclamation rules for overlay entries. Matrix ticket: MX-447. | 0071 | 83 | Tombstones preserve composite reads until checkpoints and base exposure allow reclamation. | Accepted after source review, three focused tombstone/reclaim tests, `cargo build -p uldren-loom-store`, and `git diff --check`. | No. |
| 88 | Done | P0 | 4 | Add mutable-overlay checkpoint planner. Matrix ticket: MX-448. | 0071 | 79, 83, 86 | Planner identifies compactable current records, tombstones, blockers, and pinned generations. | Accepted after source review, focused planner tests, no-default-features check, and `git diff --check`. | No. |
| 89 | Done | P0 | 5 | Add background checkpoint compaction writer for mutable overlay pages. Matrix ticket: MX-450. | 0071 | 88 | Checkpoint path materializes compact current pages while live reads and writes continue. | MX-450 accepted after source review, checkpoint-writer/planner/store checks, and code review. | No. |
| 90 | Done | P0 | 5 | Define shared multi-facet transaction API and error semantics. Matrix ticket: MX-449. | 0071 | 60, 68, 73, 83 | API for one logical workflow mutation across tickets, lanes, pages, documents, and future facets. | Accepted after source-backed spec review and stable error-code correction to `INVALID_ARGUMENT`. | No. |
| 91 | Done | P0 | 4 | Run shared transaction API design review gate. Matrix ticket: MX-451. | 0071 | 90 | Accepted design review proving write-set, compare-token, idempotency, durability, and error semantics are implementation-ready. | Matrix ticket accepted; queue reconciled from source-of-truth ticket state. | No. |
| 92 | Done | P0 | 4 | Add shared transaction API types for write sets, compare tokens, idempotency keys, durability policy, and commit results. Matrix ticket: MX-453. | 0071 | 91 | Public internal API contract with error semantics and no facet-specific persistence shortcuts. | Matrix ticket accepted; queue reconciled from source-of-truth ticket state. | No. |
| 93 | Done | P0 | 5 | Implement shared transaction API in the local engine and store adapter. Matrix ticket: MX-454. | 0071 | 92 | Concrete transaction boundary that applies write sets atomically through the mutable substrate. | Matrix ticket accepted; queue reconciled from source-of-truth ticket state. | No. |
| 96 | Done | P0 | 5 | Add transactional secondary-index primitives for mutable current-state tables. Matrix ticket: MX-455. | 0071 | 93 | Index updates commit atomically with current records and recover consistently. | MX-455 accepted after closeout evidence and acceptance evidence on the Matrix ticket. | No. |
| 98 | Done | P1 | 4 | Define current-state index adoption plan for tickets, lanes, pages, and documents. Matrix ticket: MX-456. | 0071 | 96 | Source-backed map of lookup paths that must move to transactional secondary indexes. | MX-456 accepted after feedback remediation, closeout evidence, and acceptance evidence on the Matrix ticket. | No. |
| 100 | Done | P0 | 5 | Move tickets and lanes fully onto shared mutable transaction APIs and transactional indexes. Matrix ticket: MX-457. | 0071 | 93, 96, 98 | Workflow state uses shared substrate without custom persistence semantics. | MX-457 accepted after source review, stale-index remediation, compile-break remediation, and focused ticket/lane tests. | No. |
| 110 | Done | P0 | 5 | Move pages and documents fully onto shared mutable transaction APIs and transactional indexes. Matrix ticket: MX-458. | 0071 | 93, 96, 98 | Page/document current heads use shared substrate without dropping operation-log, revision, reference, or text/binary semantics. | Coherent-snapshot adversarial tests, the affected package batch, generation-conflict tests, formatting, `git diff --check`, and the final `just ci` gate pass. | No. |
| 110a | Done | P0 | 3 | Audit every `RevisionIndex` reader and writer and classify the index as mutable current metadata, retained operation history, or a read projection. | 0071 | 110 | Source-backed ownership map for pages, tickets, chat, drive, lifecycle, interchange, MCP, hosted, CLI, and conformance paths. | Every runtime call now supplies the canonical workspace and scope; owning facets retain direct reopen proof while facades read the shared owner index. | No. |
| 110b | Done | P0 | 4 | Add shared mutable-overlay revision-index keys and transactional read/write helpers to `loom-substrate`. | 0071 | 93, 96, 110a | One reusable revision-index persistence contract with compare tokens, durability, and atomic transaction participation. | The key includes canonical workspace bytes and scope, FileStore supplies point entry and owner-token reads, and transaction receipts synchronize only their written entries. | No. |
| 110c | Done | P0 | 4 | Move page operation and published-page revision indexes into the shared transactional revision-index substrate. | 0071 | 110b | Page state, operation record, reference state, and revision-index updates survive daemon requests and restarts as one consistent mutation. | `crates/loom-pages/src/lib.rs` prepares reference/index/graph state in an isolated planning engine, commits it with page operation and revision state, and imports planned live state only after success. The focused interruption and reopen tests pass. | No. |
| 110d | Done | P0 | 4 | Migrate remaining facet and facade revision-index writers and readers to the shared substrate. | 0071 | 110a, 110b | Tickets, chat, drive, lifecycle, interchange, MCP, hosted, CLI, and conformance stop depending on staged VCS files for hot revision indexes. | Runtime callers use the workspace-qualified shared index; direct reopen tests pass for tickets, chat, drive, lifecycle, and meetings interchange. | No. |
| 110e | Done | P0 | 3 | Add daemon restart and crash-boundary revision-index conformance tests. | 0071 | 110c, 110d | Repeated update/publish operations remain consistent across request boundaries, daemon restart, interruption, and reopen for every migrated owner. | The page interruption test now checks live state before drop and again after reopen; direct reopen, workspace isolation, point access, unsupported-provider, daemon, relevant package, and full `just ci` coverage pass. | No. |
| 110f | Done | P0 | 5 | Extend the shared store transaction so owner control, table, stream, file, reference, audit, and revision-index state can co-commit. | 0071 | 93, 110b, 110d | One store transaction publishes owner state, retained operation data, audit records, and revision metadata without facet-specific two-phase windows. | `WorkflowOwnerState` carries objects, reference updates, control writes, and audit writes; `FileStore` publishes them with overlay records through one superblock commit. | No. |
| 110g | Done | P0 | 4 | Move tickets, chat, drive, lifecycle, and meetings onto the cross-storage transaction boundary and add interruption/reopen proof. | 0071 | 110f | Every owner exposes either the complete old mutation or complete new mutation after interruption, never current state with a stale revision index. | Tickets, chat, drive, lifecycle, meetings, and meetings promotions use the combined owner transaction. Reopen, rejected-transaction, full ticket, and focused facade tests pass. | No. |
| 110h | Done | P0 | 5 | Remove duplicated MCP drive-upload and meetings-review persistence sequences by routing them through atomic owner operations. | 0071 | 110g, 112 | MCP drive upload and meetings review cannot publish owner controls or staged engine state separately from their revision-index row. | MCP drive upload delegates to `loom-drive`; eleven meetings review and promotion writers use one snapshot, audit, reference, and revision transaction. Focused MCP tests pass. | No. |
| 110i | Done | P0 | 5 | Move all remaining drive metadata mutations onto one native atomic snapshot, operation-log, ACL/audit, and revision transaction, then make MCP delegate to those operations. | 0071 | 110h | Folder, rename, move, delete, conflict, share, retention, lease-recording, and upload-session mutations use native drive operations; committed metadata changes expose one old or new owner state after interruption. | Native drive mutations publish profile, operation log, indexes, ACL changes, audits, reference state, and revision rows through the shared owner transaction. MCP delegates every drive metadata mutation to `loom-drive`. Native reopen and focused MCP drive suites pass. | No. |
| 110j | Done | P0 | 4 | Define and implement the cross-facet transaction boundary for meetings promotions into tickets, lifecycle, decisions, and references. | 0071 | 110f, 110h | A promotion never exposes a created target without its source promotion record, or vice versa. | Ticket and lifecycle creation expose prepared owner state, reference artifacts expose prepared control writes, and ledger append remains staged engine state. MCP combines each target with the meetings snapshot, audit, reference root, and promotion revision in one transaction. Four focused promotion suites and native lifecycle/reference tests pass. | No. |
| 110k | Done | P0 | 5 | Co-commit ordinary ticket workflow-current projections with ticket indexed tables, profile controls, operation records, delivery notifications, and revision rows. | 0071 | 110f, 110j | Every ticket mutation exposes a complete old or new ticket owner state; no workflow-current projection can lead or lag its canonical ticket record. | `IndexedTicketProfile` accumulates and coalesces current-record puts/deletes for its finish transaction. Ticket writes no longer bypass canonical history through an overlay-only return, delivery notifications are staged before owner publication, and all 101 ticket tests pass. | No. |
| 110l | Done | P1 | 4 | Move ticket reference-index updates and unresolved-reference enqueueing from CLI, MCP, client, hosted, and C ABI facade sequences into the native ticket owner transaction. | 0071 | 110k | Ticket field-reference state is derived once by the owner and cannot lag a successful ticket mutation because a facade omitted or separately persisted the update. | Native create, field-changing update, and delete stage reference-index and unresolved-candidate state before owner publication. Ticket-key resolution uses the open owner profile instead of reopening staged rows against an older control root. Duplicate mutations are removed from CLI, MCP, client, hosted, and C ABI paths. A native reopen test proves reference index and pending candidate durability without facade synchronization. | No. |
| 110m | Done | P0 | 4 | Qualify every mutable revision-index key and API by canonical workspace identity. | 0071 | 110a, 110b | Identical scope IDs in different workspaces map to independent current revision indexes, and all callers provide the owning workspace. | Source audit and same-store, same-scope, two-workspace isolation test pass. | No. |
| 110n | Done | P0 | 5 | Add efficient provider point-read and owner-token APIs and remove full-overlay reconstruction from revision-index access. | 0071 | 68, 83, 110m | Revision-index reads and writes access one overlay key without exporting, cloning, or importing all current overlay entries. | FileStore point APIs and instrumentation prove revision access does not enumerate the overlay; receipt synchronization is bounded by the transaction write set. | No. |
| 110o | Done | P0 | 4 | Make revision-index transaction capability fail closed instead of silently dropping owner state or additional writes. | 0071 | 93, 110n | A provider either applies the complete workflow transaction atomically or returns a stable unsupported error without publishing partial state. | Unsupported-provider test uses non-empty owner state and multiple writes and proves no partial publication. | No. |
| 110p | Done | P0 | 5 | Co-commit published-page reference state with page state, operation data, and revision metadata. | 0071 | 110f, 110m, 110o | Page publication exposes either the complete prior state or complete new page and reference state, with no post-commit reference mutation. | `prepare_page_published_refs` at `crates/loom-pages/src/lib.rs:1577` opens one `WorkflowPlanningSnapshot` and obtains both the planner and reference index through `bounded_planner_with_reference_index` at `crates/loom-reference/src/lib.rs:574`. Page interruption and adversarial unpublished-live-state tests pass live and after reopen. | No. |
| 110q | Done | P0 | 5 | Perform a controlled migration of legacy staged `.lri` revision indexes into the one canonical workspace-qualified mutable layout. | 0071 | 110m, 110n | Existing development stores preserve revision history without a permanent compatibility reader or shipped migration utility. | Read-only verification compared `matrix.before-compacted-replacement.loom` with `matrix.loom` for workspace/scope `baa779e9-685e-4fb1-95cb-03ddb65eb030`. Both canonical indexes contain 2,880 revisions and checkpoints, occupy 1,728,568 bytes, and have digest `blake3:e58ea8c85d000dd692ac206bf20e2d8ac7932f17206652b839dea406742322eb`. Evidence is retained in `specs/0071-matrix-revision-migration-evidence.json`; the temporary verifier was deleted. | No. |
| 110r | Remediation Required | P0 | 4 | Complete revision-index isolation, provider, scaling, migration, interruption, daemon-restart, and reopen verification. | 0071 | 110m, 110n, 110o, 110p, 110q, 110s, 110t, 110u, 110v, 110w, 110x, 110y, 110z, 110aa | The 110 batch has direct proof for every corrected invariant and no completion claim relies on reopen-only evidence for crash atomicity. | The earlier completion claim missed full-engine planning in native ticket create/update and full-state delivery planning. Task 110aa owns the source-backed remediation and must pass before this aggregate verification can return to Done. | No. |
| 110s | Done | P0 | 5 | Co-commit document current records, declared index state, reference state, and the engine-state root through one workflow transaction. | 0071 | 93, 96, 110o | A document mutation exposes either the complete old document/index/reference state or the complete new state; no FileStore path silently falls back to independent writes. | Core document tests prove fail-closed provider behavior; FileStore reopen proves document and declared-index state recover together; reference reopen proves document, reverse references, and graph projection recover together. | No. |
| 110t | Done | P0 | 5 | Replace `substrate_transact` engine-snapshot rollback with a planned shared durable transaction. | 0071 | 93, 110s | Every supported composite operation is validated and published as one durable transaction; a rejected operation leaves both engine state and mutable-overlay state unchanged. | `substrate_transact` uses the shared bounded planner for CAS, document, graph, view, and reference mutations. It publishes explicit deltas from one provider snapshot, applies only receipt-target synchronization, and preserves success, rejection, mixed-operation, and reopen behavior without full-engine or complete-overlay cloning. | No. |
| 110u | Done | P0 | 4 | Make published-page reference preparation rollback-safe in the live engine before durable commit. | 0071 | 110p | A failed precommit leaves page state, reference index, projected graph edges, and revision metadata unchanged both in the current engine instance and after reopen. | `interrupted_publish_keeps_revision_and_references_at_prior_state` checks page revision 1, only the prior reference target, only the prior projected graph edge, and revision-index revision 1 before dropping the engine and after reopen. | No. |
| 110v | Done | P0 | 5 | Remove complete-overlay enumeration from `substrate_transact` planning and page transaction synchronization. | 0071 | 83, 93, 110n, 110t | Composite and page transactions plan against a stable provider snapshot or copy-on-write view and synchronize only their bounded write set. | FileStore counts overlay enumerations. Page interruption/success and MCP transaction success/rejection tests assert no counter change; `MutableOverlay::fork_from_snapshot` reads unchanged keys from an immutable parent and isolates delta writes; production page and MCP paths contain no `mutable_overlay_entries` or `mutable_overlay_current_entries` call. | No. |
| 110w | Done | P0 | 4 | Define a shared bounded engine-state planning contract for atomic owner mutations. | 0071 | 90, 93, 110u, 110v | Planning reads only the required immutable sections and owner records, accumulates explicit deltas, and returns owner objects, controls, reference-root changes, and post-commit live-state updates without serializing the complete Loom engine. | `WorkflowPlanningSnapshot`, `BoundedMutationPlan`, `EnginePlanningScope`, `BoundedEnginePlanner`, and `EngineStateDelta` form one reusable contract. Instrumentation reports full exports/imports, selected paths, rewritten sections, and unrelated rewrites. | No. |
| 110x | Done | P0 | 5 | Move page reference and graph preparation from full-engine forks to bounded owner-state delta planning. | 0071 | 110p, 110w | Publishing one page performs work proportional to that page's reference changes and affected graph records, while preserving live-session and reopen atomicity. | `page_publish_excludes_unpublished_live_reference_state` at `crates/loom-pages/src/lib.rs:2435` injects live-only index and graph state, publishes from the pinned durable snapshot, and proves the mutation absent in the live engine and after reopen. It also preserves the no-export, no-import, no-overlay-enumeration, and unrelated-rewrite assertions. | No. |
| 110y | Done | P0 | 5 | Move MCP `substrate_transact` from full-engine forks to shared bounded multi-owner delta planning. | 0071 | 110t, 110w | Composite transactions validate and publish explicit CAS, document, graph, view, and reference deltas without cloning total engine state. | `substrate_transact` at `crates/loom-mcp/src/writes.rs:5098` initializes reference state through the bounded planner. `substrate_transact_excludes_unpublished_live_reference_state` at `crates/loom-mcp/src/server/tests.rs:1677` proves live-only reference and graph mutations are excluded live and after reopen while preserving bounded-planning instrumentation. | No. |
| 110z | Done | P0 | 3 | Bind planning reads, compare tokens, and expected generation to one coherent provider snapshot. | 0071 | 83, 110w, 110x, 110y | A planner cannot combine an in-memory snapshot from one generation with a store generation from another; stale planning fails with `CONFLICT` before publication. | Read-only selectors at `crates/loom-core/src/vcs.rs:93` separate planning inputs from writable paths. `bounded_reference_rebuild_reads_pinned_sources_without_emitting_source_writes` proves missing-index rebuilds use pinned document sources without source deltas. Store tests `workflow_planning_snapshot_binds_reads_tokens_and_generation` and `concurrent_workflow_plans_publish_only_one_generation` prove stale and competing publications fail with `CONFLICT`. | No. |
| 110aa | In Progress | P0 | 4 | Move native ticket create/update and durable delivery planning onto the shared bounded engine-state contract. | 0071 | 110k, 110w, 110z | Ticket mutation and its delivery notification publish one coherent bounded owner transaction without a full engine export/import; non-FileStore providers retain the established supported fallback; live and reopened ticket, history, revision, reference, and delivery state remain complete. | Native ticket create/update now use `BoundedEnginePlanner`; durable delivery produces a bounded queue delta on coherent-snapshot providers; operation-local objects are published without `save_state_objects`; focused mutation, interruption, reopen, and delivery tests pass. The storage-amplification endpoint remains red, so final aggregate and performance acceptance remain open. | No. |
| 112 | Done | P0 | 5 | Route CLI and MCP hot-state mutations through migrated facet transaction APIs. Matrix ticket: MX-459. | 0071 | 100, 110 | CLI and MCP stop owning separate persistence semantics for tickets, lanes, pages, and documents. | MX-459 accepted after source review and focused MCP/CLI checks. | No. |
| 114 | Done | P0 | 5 | Route hosted, local-client, and remote-client hot-state mutations through migrated facet transaction APIs. Matrix ticket: MX-460. | 0071 | 100, 110, 112 | Hosted and client surfaces share the same transaction semantics as CLI and MCP. | Accepted after source-backed arbiter review. | No. |
| 116 | Done | P1 | 5 | Update IDL, C ABI, and language bindings only where shared transaction semantics change observable contracts. Matrix ticket: MX-478. | 0071 | 112, 114 | Generated and binding surfaces stay aligned with any new compare-token, durability, or transaction fields. | Accepted after focused public-surface review. | No. |
| 120 | Done | P0 | 5 | Define VCS/CAS promotion bridge semantics for overlay checkpoints. Matrix ticket: MX-466. | 0071 | 82, 93 | Promotion contract for which hot current records become immutable roots and which remain operational. | MX-466 accepted after source-backed design/spec review. | No. |
| 121 | Done | P0 | 4 | Run VCS/CAS promotion design review gate. Matrix ticket: MX-467. | 0071 | 120 | Accepted design review proving checkpoint selection, strict promotion boundaries, and namespace behavior are implementation-ready. | MX-467 accepted after source-backed design gate review. | No. |
| 122 | Done | P0 | 4 | Implement promotion checkpoint reader and overlay-entry selection. Matrix ticket: MX-479. | 0071 | 100, 110, 121 | Promotion reads a pinned generation and selects exactly the owner scopes requested. | Accepted after focused checkpoint-selection review. | No. |
| 123 | Done | P0 | 5 | Project selected overlay records into VCS/CAS tree objects without namespace collisions. Matrix ticket: MX-480. | 0071 | 122 | VCS commit snapshots selected overlay state into immutable roots. | Accepted after focused core promotion and checkpoint checks. | No. |
| 124 | Done | P0 | 5 | Add promotion tests for namespace-collision repair and no-uncommitted-state migration preflight. Matrix ticket: MX-481. | 0071 | 123 | Tests covering document/file projection collisions and clean commit readiness. | Accepted after focused VCS collision repair and migration-readiness tests. | No. |
| 126 | Done | P1 | 4 | Enforce strict promotion boundaries for audit and ledger consumers. Matrix ticket: MX-483. | 0071 | 120, 123 | Audit and ledger consumers use explicit strict checkpoints rather than hot-write mini commits. | Accepted after source-backed evidence review and focused audit/ledger checks. | No. |
| 127 | Done | P1 | 4 | Enforce strict promotion boundaries for sync and export consumers. Matrix ticket: MX-484. | 0071 | 120, 123 | Sync and export consumers use explicit strict checkpoints and do not publish hot mutable state accidentally. | Accepted after focused export and sync checks. | No. |
| 130 | Done | P1 | 4 | Add operation/facet durability defaults and policy override surfaces. Matrix ticket: MX-445. | 0071 | 64 | CLI/MCP/store policy visibility and validation. | Accepted after source review and focused store, CLI, and MCP policy tests. | No. |
| 140 | Done | P1 | 4 | Add lock contention, group commit, fsync latency, and pinned-reader diagnostics. Matrix ticket: MX-452. | 0071 | 76, 84 | Doctor/maintenance metrics. | Matrix ticket accepted; queue reconciled from source-of-truth ticket state. | No. |
| 142 | Done | P0 | 5 | Implement MVCC reader pin tracking and reclamation blockers. Matrix ticket: MX-475. | 0071 | 82, 83, 84, 86 | Readers pin stable overlay generations and reclamation reports active blockers. | MX-475 accepted after source review and focused MVCC/reclaim tests. | No. |
| 144 | Done | P0 | 4 | Implement compacted-copy byte measurement for performance reports. Matrix ticket: MX-477. | 0071 | 45, 52 | `storage.compacted_bytes` measures an equivalent compacted copy and never silently reports zero. | MX-477 accepted after source review, example test, four-iteration performance run, and JSON artifact check. | No. |
| 146 | Done | P0 | 4 | Add concurrent reader and writer performance scenario. Matrix ticket: MX-476. | 0071 | 142, 144 | Manual performance suite proves stable readers while a writer mutates current records and reports reader failures plus latency. | Accepted after focused source-backed review and small manual performance probe. | No. |
| 147 | Done | P0 | 4 | Add normal workflow transaction group-commit regression coverage. Matrix ticket: MX-473. | 0071 | 112, 114 | Normal workflow transactions batch under contention, survive reopen, and replay idempotently. | MX-473 accepted after source review and focused workflow transaction tests. | No. |
| 153 | Done | P0 | 4 | Add durability-mode scenarios to manual performance suite. Matrix ticket: MX-474. | 0071 | 147 | Strict, normal, relaxed, and ephemeral scenarios are source-backed and no longer silently skipped. | Accepted after focused source-backed review and small manual performance artifact run. | No. |
| 148 | Done | P1 | 4 | Define controlled migration preflight and replacement contract for development stores. Matrix ticket: MX-485. | 0071 | 122, 124 | Preflight checklist for freshness watermark, backup, replacement, and no lingering temporary tools. | Accepted after source-backed spec and CLI preflight review. | No. |
| 149 | Done | P1 | 4 | Run controlled migration design review gate. Matrix ticket: MX-486. | 0071 | 148 | Accepted design review proving migration preflight, backup, replacement, cleanup, and rollback expectations are implementation-ready. | Accepted after source-backed design-gate review. | No. |
| 150 | Done | P1 | 4 | Implement migration preflight for development stores. Matrix ticket: MX-487. | 0071 | 100, 110, 123, 124, 149 | Preflight reports freshness watermark, backup plan, active-store freshness, and legacy collision risks. | Accepted after focused CLI report-field test and non-default CLI compile check. | No. |
| 151 | Done | P1 | 5 | Implement migration execution and verification for development stores. Matrix ticket: MX-488. | 0071 | 150 | Migration performs backup, replacement, cleanup, and post-migration surface checks with no lingering temporary tools. | Accepted after source-backed CLI activation review and focused CLI checks. | No. |
| 152 | Done | P1 | 5 | Validate the real Matrix development store migration against the final mutable substrate shape. Matrix ticket: MX-489. | 0071 | 151 | Source-backed migration report proving Matrix data remains readable, uncommitted state is clean, and Matrix MCP serves the migrated store correctly. | MX-489 accepted after the migration deletion was committed, cleanliness and the 2,880-entry revision index were proven, post-baseline replacement validation passed, and live MCP ticket/lane read-write smoke tests passed. Rollback artifacts remain retained. | No. |
| 160 | Done | P1 | 5 | Classify facet defaults for durability and retention across tickets, lanes, pages, documents, PIM, KV, queue, metrics, search, vector, SQL, graph, ledger, OCI, S3, and runtime state. Matrix ticket: MX-446. | 0071 | 60 | Source-backed default policy table and owner docs. | Accepted after source-backed spec review and `git diff --check -- specs/0071-mutable-overlay-substrate.md`. | No. |
| 170 | Done | P2 | 4 | Add derived artifact policy integration for search, vector, dataframe, and projections. Matrix ticket: MX-482. | 0071 | 60, 93 | Rebuildable derived state defaults to `relaxed` unless retained. | Accepted after focused derived-artifact policy review. | No. |
| 178 | Blocked | P0 | 5 | Run final semantic-preservation and performance closure review for 0071. Matrix ticket MX-490; transferred recovery id MU-19. | 0071 | 50, 52, 54, 74, 79, 84, 89, 112, 114, 116, 124, 126, 127, 152, 160, 170, 181, 182, 183, 184, 185, 188, 190, 192, 193, 198 | Closure review proving all spec criteria are source-backed and no optimized facet lost required semantics. | Blocked on remaining performance architecture tasks. Existing MU-19 evidence remains under `specs/messedup/evidence/`. | No. |
| 180 | Blocked | P2 | 4 | Update implementation plan once 0071 architecture changes are source-backed. Matrix ticket MX-491; transferred recovery id MU-20. | `specs/IMPLEMENTATION-PLAN.md` | 60, 73, 93, 178 | Implementation plan reflects current progress and unfinished work. | Blocked on Task 178. Existing MU-20 evidence remains under `specs/messedup/evidence/`. | No. |
| 181 | Done | P0 | 4 | Eliminate repeated-current-write page leakage and fragmented large-record amplification. | 0071 | 40, 45, 110 | Superseded current records become reusable without stale pages, owner transactions coalesce duplicate addresses, and growing mutable records reuse fragmented pages. | Planning pins are released before generation-checked commit; overlay generation is no longer compared with file generation; owner-state records are coalesced; mutable blobs use CRC-protected page chains. Two consecutive 30-second probes left zero stale pages and grew from 3,866,624 to 4,079,616 bytes while retaining seven additional ticket/page histories. Store, ticket, and page suites pass. | No. |
| 182 | Done | P0 | 4 | Replace monolithic retained operation logs and revision indexes with append-addressed segmented storage. | 0071 | 181 | Ticket/page history append cost and bytes written are proportional to the new records, while ordered history, revision lookup, checkpoints, retention, and reopen behavior remain unchanged. | Completed through 182a and 182b. Retained records append atomically, current heads stay bounded, and page/ticket writers use latest-revision and checkpoint point records rather than reconstructing history. | No. |
| 182a | Done | P0 | 4 | Implement atomic append-addressed retained records and fragmented immutable-record allocation. | 182 | 181 | Page operations and revision records append independently, retained heads remain bounded, legacy aggregates convert on write, and large immutable records reuse fragmented pages without corrupting GC or tail compaction. | FileStore retained-history records, page operation logs, and revision history are append-addressed. The full store suite passed 261 tests. A shared-slab tail-compaction defect found by the reopen probe was fixed and regression tested. After the 32-generation recovery window, a five-iteration probe grew by 184,320 bytes while retaining five new ticket/page history steps. | No. |
| 182b | Done | P0 | 4 | Add bounded latest-revision and checkpoint point indexes for append writers. | 182 | 182a | Ticket and page append paths read only the affected entity's latest revision and checkpoint uniqueness metadata; they do not reconstruct complete retained history before appending. Full history reads, ordered checkpoints, compare conflicts, legacy conversion, and reopen behavior remain unchanged. | The shared append batch writes latest entity revisions and checkpoint identity points atomically with retained records. An incomplete older manifest triggers one point-index backfill; complete manifests make an unknown-entity lookup without reading retained history. Substrate, page, and all 102 ticket tests pass. | No. |
| 183 | Done | P0 | 4 | Bound steady-state physical slack created by mixed current and retained writes. | 0071 | 182 | After the crash-recovery reuse window, physical file growth remains within a documented bounded factor of newly retained logical bytes; reusable interior pages do not continue accumulating linearly, and reopen, journal recovery, GC, and tail compaction remain correct. | Owner-state publication now point-updates the existing overlay B-tree instead of loading, freeing, and rebuilding the complete index. Active GC segments are swept in place instead of repeatedly relocating every live object, adjacent free runs retain independent recovery generations, and diagnostics count overlapping reclaimable/free pages once. Two consecutive 30-second daemon probes reopened cleanly; the second retained seven additional ticket/page histories while physical size grew by 36,864 bytes and reusable interior space fell by 258,048 bytes. | No. |
| 184 | Done | P0 | 4 | Pack new small mutable-overlay records without losing independent replacement reclamation. | 0071 | 183 | New records share slab pages, replacement records remain independently reclaimable, and replacing one shared-slab record cannot free neighboring live records. | The second eight-bundle random-write batch grew by 483,328 bytes instead of the prior 1,720,320 bytes. Physical size was 2,220,032 bytes with 430,080 reusable bytes, within the documented random-write bounds. The 264-test store suite and three-test mutable-overlay growth suite pass. | No. |
| 185 | Remediation Required | P1 | 5 | Design and implement crash-safe packing of small mutable records across transaction boundaries. Matrix design ticket: MX-504. | 0071 | 184, 187 | Separate small transactions can reuse partially occupied mutable record pages without in-place torn writes, shared-page corruption, complete-overlay scans, or a permanent 4 KiB live-page floor per transaction. | One typed transaction-local batch authority now packs eligible records from multiple root families into shared immutable slabs and accepts both canonical locator codecs. Focused production-path tests prove mixed-family sharing, distinct slots, reopen, idempotency-family routing, and replacement-neighbor preservation. Task 185 remains open because `transaction_delta_pack.rs` is orphaned and uncompilable, and no production authority yet owns cross-transaction physical slab membership, advisory candidates, exact revalidation, bounded consolidation, or atomic semantic plus advisory publication. Remove the orphan and implement that current-format authority before restoring Done. | No. |
| 186 | Done | P0 | 4 | Add command-level speed diagnostics and remove avoidable full-overlay scans from point reads and transaction bookkeeping. | 0071 | 184 | A repeatable probe attributes latency by command and store size; point reads use B-tree lookup; workflow rollback and current-record encoding do not export complete overlay history. | `loop-speed-probe.sh` produced three comparable CSV runs. The 264-test store suite, three-test growth suite, focused workflow tests, shell syntax check, formatting check, and diff check pass. | No. |
| 187 | Done | P0 | 4 | Design persisted index-root separation for current mutable state and retained/control records. Matrix design ticket: MX-505. | 0071 | 186 | Store open and current-state hydration scale with live current records rather than retained history, while point reads, GC, checkpoints, recovery, and crash atomicity retain one coherent root set. | MX-505 is accepted with one canonical post-migration format: `current_record_root` is direct in the RegionTable; a fixed 4096-byte CRC-protected catalog names family-owned address-to-RecordLoc roots plus the rebuildable delta-pack candidate index; no global overlay locator remains. | No. |
| 188 | In Progress | P0 | 5 | Implement current-state and retained/control index-root separation with bounded hydration. | 0071 | 187 | Fresh and migrated stores open without reading retained payloads; current, retained, token, index, and idempotency records remain atomically published and independently diagnosable. | T188-1 design evidence is recorded in `specs/0071-mutable-overlay-substrate.md`: exact canonical RegionTable and 4096-byte root-catalog layouts, stable family ids, flags, omission-only absence semantics, validation rejection rules for absent entry roots and present zero page ids, unknown-family behavior, corrected vectors, and the typed `RootFamily` registry source for routing, validation, attribution, and GC. Stop before T188-2 codec implementation. | No. |
| 189 | Done | P0 | 4 | Design local daemon execution through the shared generated client and dispatch contract. Matrix tickets: MX-492, MX-497, MX-498. | 0071 | 186 | CLI and MCP operations share a generated, typed execution boundary without facet-specific daemon RPC drift or independent persistent engines. | MX-492 established the canonical boundary; MX-497 audited the MCP surface; MX-498 was accepted after compile-time generated IDL target enforcement, principal-bound server execution, and cross-route coherence tests. | No. |
| 190 | In Progress | P0 | 5 | Route ticket, lane, page, document, and remaining CLI operations through persistent local daemon execution. Matrix tickets: MX-493, MX-494, MX-495, MX-496; transferred recovery parent MU-17g. | 0071 | 189, 190a | Repeated CLI operations avoid process-local store hydration and participate in daemon serialization, group commit, metrics, and concurrent read/write behavior. | MX-493, MX-494, and MX-495 are accepted. MU-6 and MU-7 are recorded accepted in the recovery archive. Task 190 remains open on the transferred exhaustive ownership proof 190a and final recovery coherence evidence. | No. |
| 190a | Remediation Required | P1 | 4 | Complete the exhaustive post-migration CLI ownership audit transferred from MU-17g-g. | 0071 | 189, MX-493, MX-494, MX-495, MX-496 | Every executable production leaf is classified exactly once by an explicit typed generated-owner tuple or a source-anchored exception tuple. | Remove source-proximity inference and make family guards plus the aggregate audit consume the same typed full-path tuples. Preserve the 466-leaf grammar, 382 generated and 83 exception totals, disjoint-union proof, and negative validation tests. Evidence remains `specs/messedup/evidence/MU-17g-g.md`. | No. |
| 191 | Done | P1 | 4 | Design and prove resumable bounded non-blocking daemon maintenance scheduling. Matrix ticket: MX-503. | 0071 | 186, 189 | Mark, index traversal, reclaim, trim, and compaction work execute in bounded resumable steps with yielding, cancellation, restart, observable debt, and foreground progress. | MX-503 accepted after source review proved durable traversal cursors, evidence invalidation, scheduler counters, exact reachability, store-backed foreground operations during a deterministic yielded worker boundary, slice-boundary cancellation, runtime reconstruction, and exact convergence. | No. |
| 192 | Done | P1 | 5 | Implement bounded maintenance steps and foreground latency controls. Matrix ticket: MX-500. | 0071 | 190, 191 | Maintenance no longer causes multi-second startup or eligibility spikes and still converges under sustained writes. | MX-500 accepted after source and artifact review proved bounded repeated maintenance slices, foreground latency percentiles, progress/yield/overrun counters, cancellation/restart behavior, and convergence classified by the store maintenance eligibility policy with residual debt reasons. | No. |
| 193 | Blocked | P0 | 3 | Run final overwrite, random-write, cross-surface parity, and sustained performance scaling evidence. Matrix ticket MX-501; transferred recovery id MU-18. | 0071 | 185, 188, 190, 192, 193p, 198, 199d | Multiple consecutive probe batches demonstrate bounded physical growth and stable throughput as retained history and current-record populations increase. | Task 192 is done. Tasks 188, 190, 193p, 198, and 199d remain incomplete. Existing MU-18 evidence remains under `specs/messedup/evidence/`. | No. |
| 193c1 | Done | P1 | 4 | Preserve analytical and core JSON parity transferred from MU-17h-c1. | 0071 | MU-17h-a | Vector workspace, FTS status, and every Pages JSON selector have field-preserving direct-local, daemon-local, and hosted-remote parity; no-selector and process-local exceptions are source-enforced. | Accepted evidence: `specs/messedup/evidence/MU-17h-c1.md`. | No. |
| 193c2 | Done | P1 | 4 | Preserve security, management, and inference JSON parity transferred from MU-17h-c2. | 0071 | MU-17h-a | Generated store-backed Inference instance JSON selectors have three-target parity; no-selector and external-runtime exceptions are source-enforced. | Accepted evidence: `specs/messedup/evidence/MU-17h-c2.md`. | No. |
| 193d1 | Planned | P1 | 4 | Add stable error and absent or empty-result parity for Graph, Columnar, and Dataframe without weakening exact binary-output assertions. Transferred from MU-17h-d1. | 0071 | 190a | Reuse the existing analytical report and preserve semantic errors and empty results across all three targets. | Focused three-target parity and source inventory guard. | No. |
| 193d2 | Planned | P1 | 4 | Add stable error and absent or empty-result parity for Calendar, Contacts, and Mail. Transferred from MU-17h-d2. | 0071 | 190a | Reuse the existing PIM report across all three targets. | Focused three-target parity and source inventory guard. | No. |
| 193d3 | Planned | P1 | 4 | Add stable error and absent or empty-result parity for Program, Metrics, Logs, and Traces. Transferred from MU-17h-d3. | 0071 | 190a | Reuse the existing D5 report across all three targets. | Focused three-target parity and source inventory guard. | No. |
| 193e | Planned | P1 | 4 | Add empty SQL results and hosted Meetings pre-write absence, list, get, and source-read parity. Transferred from MU-17h-e. | 0071 | 190a | Extend the existing E1 fixture without changing production semantics. | Focused three-target parity. | No. |
| 193f | Planned | P1 | 4 | Add Studio command coverage, representative VCS error or absence, and complete Inference default, JSON, and not-found parity. Transferred from MU-17h-f. | 0071 | 190a | Extend the existing F3 fixture with field-preserving normalization only. | Focused three-target parity. | No. |
| 193g | Planned | P1 | 4 | Compare complete ordered Audit output and add representative Certificate and NetworkAccess failure parity. Transferred from MU-17h-g. | 0071 | 190a | Preserve semantic output without broad filtering. | Focused three-target parity. | No. |
| 193h | Planned | P1 | 4 | Replace overbroad Store-stat normalization and add passphrase and raw-KEK rekey parity. Transferred from MU-17h-h. | 0071 | 190a | Normalize only source-proven physical nondeterminism and preserve rekey semantics. | Focused D4 parity. | No. |
| 193i | Planned | P1 | 3 | Add blocked or invalid daemon StoreAdmin maintenance and Serve not-found parity. Transferred from MU-17h-i. | 0071 | 190a | Preserve semantic counters and stable errors. | Focused F4 parity. | No. |
| 193j | Planned | P1 | 4 | Add successful Exec run/apply parity and canonical Interchange CBOR comparison. Transferred from MU-17h-j. | 0071 | 190a | Normalize only embedded dynamic values. | Focused F2 parity. | No. |
| 193k | Planned | P1 | 4 | Add QueueConsumers empty state, post-delete Workspace empty list, absent Page or Space, and empty SQL parity. Transferred from MU-17h-k. | 0071 | 190a | Reuse existing foundational fixtures. | Focused three-target parity. | No. |
| 193p | Planned | P1 | 3 | Complete the transferred MU-17h default-text, JSON, success, error, absence, and empty-state parity parent. | 0071 | 190a, 193c1, 193c2, 193d1, 193d2, 193d3, 193e, 193f, 193g, 193h, 193i, 193j, 193k | Every migrated family has source-inventoried three-target presentation parity with only source-proven nondeterminism normalized. | Skeptical source review plus focused family reports; existing accepted MU-17h-a and MU-17h-b1 through b3 evidence remains in the recovery archive. | No. |
| 194 | In Progress | P0 | 6 | Close captured-free foreground publication and its deterministic growth gate. | 0071 | 52, 54, 195 | Preserve accepted mixed-age cursor and measured-demand proofs; implement the approved bounded allocator-owned metadata bootstrap reserve with a durable descriptor beside canonical roots; exclude reserve pages from the free map and captured-free set; atomically consume and refill through `finish_txn`; reject over-capacity demand before mutation; and use one durable monotonic minimum-recoverable-generation authority so validated-GC pages are not quarantined for a second fixed window. Prove fragmented-map oscillation, interruption, reopen, reader-fence, no leak, and final ticket/page growth behavior. | The canonical RegionTable now carries the horizon without changing the fixed journal-record size. Every successful root-set publication advances it atomically, old zero-valued pages decode, and foreground, maintenance, mark-epoch, GC, and migration reuse paths require it together with reader and epoch fences. Focused canonical-vector, reopen, torn-commit, lease, and allocator tests pass. After bounded ticket/delivery planning, ticket-create still measures 4,456,448 online bytes for 851,968 compacted bytes against a 3,407,872 ceiling. Task 185 physical advisory and bounded specialized consolidation remain the blocker. Complete the crash-boundary matrix and all workload ceilings before closure. | Yes. |
| 195 | Done | P0 | 6 | Close bounded copy-on-write B-tree batch mutation. | 0071 | 52 | Preserve accepted decoder, codec, structural, path-locality, duplicate, and size proofs; implement one immutable prepared delete/upsert delta and consuming apply operation; preserve exact allocator accounting and untouched identities; reject stale source before mutation; and prove a real transaction failure leaves live and reopened authoritative state unchanged. | Source review confirmed one prepared topology authority, consuming apply with pinned identity validation, shared mutation adapters, and live plus reopened rollback proof. Evidence is preserved in `MU-17j-l-b-review.md` and `MU-17j-l-b1.md`. | No. |
| 196 | Done | P0 | 6 | Close all multi-record publication callers. | 0071 | 195 | Preserve accepted segment-GC and tail-compaction migration; prove maintenance rollback; migrate audit-retention family updates; prove ordering, replacement, reopen, accounting, and rollback; and audit source until no production multi-record publisher uses repeated one-key insertion. | Source review confirmed maintenance rollback boundaries, audit-retention delete-before-batch-put ordering, live and reopened authority preservation, and no production repeated one-key insertion loop. Evidence is preserved in `MU-17j-l-c-review.md`, `MU-17j-l-c1.md`, and `MU-17j-l-c.md`. | No. |
| 197 | Planned | P0 | 3 | Pass the cross-facet ticket/page physical-growth gate after batch publication. | 0071 | 185, 194, 196 | At each measured endpoint, ticket and page create online bytes are at most `max(4 * compacted_bytes, compacted_bytes + 1 MiB)`; overwrite online bytes are at most `max(3 * compacted_bytes, compacted_bytes + 512 KiB)`; reusable plus independently reclaimable bytes and stale acceptance bytes remain disjoint and within their specified shares. The gate does not infer acceptance from a three-window plateau because B-tree depth and live cardinality can change between windows. | Run ticket create first; only after it passes run ticket overwrite, page create, and page overwrite, followed by the dedicated recipe. After the recovery-horizon correction, the current ticket-create endpoint is 4,509,696 online bytes for 770,048 compacted bytes, still exceeding the 3,080,192-byte ceiling. | No. |
| 198 | Planned | P0 | 3 | Run bounded hot, random, and mixed-workload attribution after object-index remediation. | 0071 | 197 | Physical growth follows retained payload and index depth, no other page class absorbs the removed amplification, and throughput does not degrade with store size. | Bounded probe artifacts plus skeptical source review; no acceptance based only on shell-script completion. | No. |
| 199 | Planned | P0 | 3 | Establish one typed foreground-publication authority that makes reclamation planning mandatory for ordinary mutations. | 0071 | 198 | Ordinary mutation code cannot reach final transaction publication without a prepared plan that owns the reusable runs, control update, reservation, and reclamation lease through commit. | Design review proves the authority boundary, ownership invariants, specialized-path separation, and migration sequence before implementation. | No. |
| 199a | Planned | P0 | 4 | Extract the foreground publication planner and prepared-publication types into a dedicated store module without changing durable behavior. | 199 | 199 | `PreparedForegroundPublication` privately owns its lease, reservation, allocator-visible runs, and control update; construction and consumption are explicit and single-use. | Focused compile and invariant tests; no caller migration in this task. | No. |
| 199b | Planned | P0 | 4 | Route every ordinary foreground mutation owner through the prepared-publication commit authority. | 199 | 199a | Workflow owner state, current and tombstone records, control and audit updates, and ordinary object/reference/control transactions cannot bypass planning or release the lease before commit. | Source inventory and compile-time API visibility prove no ordinary caller can invoke low-level publication directly. | No. |
| 199c | Planned | P0 | 3 | Separate maintenance, compaction, checkpoint, migration, and reachability publication behind explicit specialized authorities. | 199 | 199b | Specialized operations retain their required reclamation semantics without sharing or bypassing the ordinary foreground authority accidentally. | Exhaustive caller classification is represented by typed entry points and fails compilation when an unclassified caller is added. | No. |
| 199d | Planned | P0 | 4 | Add compile-time routing guards and structural page-budget tests for every foreground mutation shape. | 199 | 199c | Ticket, lane, page, document, tombstone, workflow, and non-ticket mutations have complete page attribution, absolute structural ceilings, failure rollback, reopen, and recovery-fence proof. Root-family batching rejects non-locator codecs and duplicate cross-family addresses before writes, preserves empty-family roots, packs eligible mixed-family records into distinct slots, preserves neighboring slab records on replacement, and rolls every family back together on failure. Recovery-floor tests cover every publication and first-reuse crash boundary plus reader and epoch fences. | Fast page-write invariant tests plus the manual storage-amplification integration gate; exact endpoint compacted-size ratios and disjoint reclaimable/stale shares replace the invalid multi-window slope rule. No acceptance ceiling may be derived from observed bad growth. | No. |

## Missed Or Hidden Work Found

- Item: The current attribution command is now good enough for MX-419, but new-item write
  amplification remains unresolved and is the next blocker.
  Moved to: Task 40.
  Reason: Blocks the queue goal.
  Date: 2026-07-22.

- Item: Durability policy was not part of the original 0071 draft.
  Moved to: Tasks 60, 62, 64, 66, 70, 72, 74, and 130.
  Reason: User approved durability modes as first-class target architecture.
  Date: 2026-07-22.

- Item: Manual performance integration tests were implied but not explicitly represented.
  Moved to: Task 45.
  Reason: Performance work needs early measurement without slowing default test runs.
  Date: 2026-07-22.

- Item: The split 0071 rows now exist in the file queue, but only the active early work is
  materialized as Matrix tickets.
  Moved to: Task 47.
  Reason: The queue file preserves scope now; Matrix tickets should be created in dependency order
  as tasks become assignable to avoid stale or misleading ticket bodies.
  Date: 2026-07-22.

- Item: Page reuse, tombstone retention, background checkpoint compaction, owner-token indexes,
  idempotency indexes, transactional secondary indexes, and non-VCS strict promotion boundaries were
  specified in 0071 but only implied by broad tasks.
  Moved to: Tasks 68, 78, 79, 86, 88, 89, 96, 98, 126, and 127.
  Reason: These are closure requirements and should not be hidden inside broader implementation
  tickets.
  Date: 2026-07-22.

- Item: Several queue rows had lift 5 or 6 but still represented multiple implementation concerns.
  Moved to: Tasks 72, 73, 78, 79, 82, 83, 88, 89, 92, 93, 112, 114, 116, 122, 123, 150, and 151.
  Reason: Future Matrix tickets need executable scope with one primary output and one review shape.
  Date: 2026-07-22.

- Item: Document current records, declared document-index state, and reference projections are still
  published in separate persistence steps.
  Moved to: Task 110s.
  Reason: Task 110 cannot close while a successful document mutation can expose a new current record
  with stale index or reference state.
  Date: 2026-07-24.

- Item: `substrate_transact` restores only `Loom::export_state` after an operation fails, but mutable
  overlay writes are already durable in `FileStore` and survive that in-memory rollback.
  Moved to: Task 110t.
  Reason: The composite tool advertises atomic rollback and has conformance coverage for it; accepting
  a partial durable write would violate the shared transaction contract.
  Date: 2026-07-24.

- Item: Workflow planning pins, mixed generation domains, and contiguous mutable-record allocation
  caused repeated current writes to strand pages or extend the file despite reusable free space.
  Moved to: Task 181.
  Reason: These shared store defects amplified every mutable facet and were not facet-specific churn.
  Date: 2026-07-24.

- Item: Retained ticket/page operation logs and revision indexes were encoded and rewritten as
  growing monolithic aggregates. Physical append storage is now implemented, but page and ticket
  writers still reconstruct full revision history to calculate the next entity revision.
  Moved to: Tasks 182a and 182b.
  Reason: Task 182 cannot close until both physical bytes written and in-memory append work are
  bounded by the new records.
  Date: 2026-07-24.

- Item: Append-addressed history initially left owner-state publication rebuilding the complete
  overlay B-tree, while active-segment GC repeatedly relocated all live objects and free-run
  coalescing refreshed otherwise reusable pages.
  Moved to: Task 183.
  Reason: Point updates, active-segment in-place sweep, generation-preserving free-run coalescing,
  and overlap-correct diagnostics now bound the physical cost. A second 30-second probe retained
  seven ticket/page history steps while adding 36,864 physical bytes.
  Date: 2026-07-25.

- Item: Random new-item transactions allocated dedicated 4 KiB pages for nearly every small mutable
  record, and the first correction could not reclaim a shared page safely when only one resident
  record was replaced.
  Moved to: Tasks 184 and 185.
  Reason: Task 184 packs records created by one transaction and reclaims shared slabs only when every
  live slot is superseded together, reducing measured marginal growth by 72 percent without neighbor
  corruption. Task 185 tracks the remaining crash-safe cross-transaction packing design instead of
  hiding the per-transaction page floor in final closure.
  Date: 2026-07-25.

- Item: Repeated CLI writes slow as retained history grows even after write amplification is bounded.
  Moved to: Tasks 186 through 193.
  Reason: Every command reopens and hydrates the store; current and retained records share one
  persisted B-tree; CLI commands do not execute through the daemon's persistent engine; and
  immediately eligible maintenance causes multi-second foreground outliers. Local point-read and
  transaction-history scans are removed, while the root split, shared daemon execution, and bounded
  maintenance scheduler remain explicit architecture work.
  Date: 2026-07-25.

- Item: The queue had implementation work but not enough explicit design-review and semantic-preservation
  gates.
  Moved to: Tasks 54, 71, 81, 91, 121, 149, and 178.
  Reason: Performance work must not regress operation logs, indexes, audit, PEP, or public surfaces.
  Date: 2026-07-22.

- Item: Queue rows and accepted Matrix tickets claim several durability and MVCC substrate tasks are
  complete, while `specs/0071-mutable-overlay-substrate.md` section 21 still marks configurable
  durability behavior, WAL/group commit behavior, and MVCC completeness as incomplete. Newly created
  durability and MVCC tickets must be reconciled against older accepted tickets MX-425 through
  MX-435 and MX-440 through MX-447 before they are used as implementation tasks.
  Moved to: Task 47 and MX-472 blocker.
  Reason: Prevent duplicate implementation or accepting stale queue/spec status as source truth.
  Date: 2026-07-23.

## Risk Register

| Risk | Impact | Mitigation | Status |
| --- | --- | --- | --- |
| Optimizing random writes before attribution is accepted. | Could optimize the wrong layer. | MX-419 is a dependency for MX-420 and was accepted before MX-420 proceeds. | Mitigated |
| Keeping strict fsync as the default for all hot writes. | Throughput remains poor under agents and hosted workloads. | Implement first-class durability modes and group commit. | Open |
| Building per-facet storage paths. | Drift, duplicated bugs, and inconsistent durability semantics. | Shared multi-facet transaction API. | Open |
| VCS semantics leaking into hot operational writes. | Mini commits return through another path. | Explicit promotion bridge and tests. | Open |
| Broad tests become too slow. | Engineering flow slows down. | Follow AGENTS.md test sizing and use diagnostic targets for stress tests. | Open |

## Implementation Batch Map

| Batch | Tasks | Purpose |
| --- | --- | --- |
| Spec and queue | 10, 20 | Lock the target architecture and task map. |
| Storage diagnostics | 30 | Make growth explainable. |
| Immediate growth fix | 40, 45, 47, 50, 52, 54 | Reduce random-new-item amplification and keep it tested without semantic regressions. |
| Durability substrate | 60, 62, 64, 66, 70, 71, 72, 73, 74, 76, 130 | Make durability policy configurable and performant. |
| Concurrency substrate | 68, 78, 79, 80, 81, 82, 83, 84, 86, 88, 89, 90, 91, 92, 93, 96, 98, 140 | Support concurrent reads and writes through shared APIs and indexes. |
| Facet migration | 100, 110, 112, 114, 116, 160, 170 | Move current state to the shared substrate and route public surfaces through it. |
| VCS bridge and migration | 120, 121, 122, 123, 124, 126, 127, 148, 149, 150, 151, 152 | Preserve explicit versioned history and migrate development stores. |
| Retained-history scaling | 181, 182, 183 | Remove current-record leakage, make retained history physically append-addressed, and bound residual physical slack. |
| Cross-transaction packing and root separation | 185, 187, 188 | Remove the remaining per-transaction page floor and separate current-state hydration from retained/control history. |
| Shared execution boundary | 189, 190 | Route CLI and MCP through the generated local/remote service contract and persistent daemon execution. |
| Bounded maintenance | 191, 192 | Make maintenance resumable, cancellable, convergent, and nonblocking for foreground work. |
| Typed publication authority | 199, 199a, 199b, 199c, 199d | Make ordinary transaction planning mandatory by construction and keep specialized publication responsibilities explicit. |
| Final performance evidence | 193 | Prove sustained throughput and bounded growth after all architecture batches land. |
| Closure and planning sync | 178, 180 | Prove 0071 achieved, then update implementation plan. |

## Blocked Task Protocol

Blocked tasks must include:

- Blocking condition.
- Attempted resolution.
- Decision needed, if any.
- Next unblock action.

## Queue Closure Rules

Do not close this queue until:

- Every task is Done, Cut with rationale, or moved to another queue or planning document.
- Missed Or Hidden Work Found is empty, promoted, cut with rationale, or moved.
- Decision Points are resolved, cut with rationale, or moved.
- Completion Evidence is satisfied.
- Final Handoff is complete.

Do not reorder, reprioritize, or cut tasks without recording the reason. Ask the user before changing
P0/P1 priority unless it is a blocker carve-out.

## Final Handoff

Complete this section before closing the queue.

- Summary:
- Completed tasks:
- Cut or deferred tasks and where they moved:
- Decisions resolved:
