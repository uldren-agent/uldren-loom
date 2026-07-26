# 0071 - Mutable Overlay Substrate

**Status:** Draft target. **Capability:** `mutable-overlay`. **Normative target.**

Loom keeps immutable committed history in content-addressed Merkle form. That remains the right model
for VCS commits, sync, verification, retained history, reproducible exports, audit roots, and signed
checkpoints. It is not the right physical write path for high-churn current operational state.

The target architecture makes Loom a two-plane system:

| Plane | Purpose | Storage behavior |
| --- | --- | --- |
| Hot mutable plane | Tickets, lanes, pages, documents, indexes, cursors, offsets, and operational facets. | MVCC current-state records, WAL-backed recovery, page reuse, configurable durability, and transactional secondary indexes. |
| Immutable versioned plane | VCS commits, snapshots, exports, sync, audit roots, signed history, and retained immutable artifacts. | CAS/Merkle roots produced at explicit checkpoint, commit, export, sync, or promotion boundaries. |

The core rule is:

```text
Normal writes update hot mutable state.
VCS commits snapshot selected hot state into immutable CAS/Merkle roots.
```

CAS is identity and history infrastructure. It must not be the default write path for operational
state. Treating every small current-state update as a mini commit creates write amplification,
lock contention, stale record buildup, and file growth that is disproportionate to user data.

## 1. Problem Statement

Current high-churn writes can rewrite and retain large aggregate engine-state roots. A small lane,
ticket, project-setting, document-head, staging, or working-tree update can force a full engine-state
save path. Repeating that pattern makes store growth proportional to write count and aggregate state
size rather than proportional to changed records.

The expected enterprise behavior is database-like current-state behavior:

- updating one current ticket many times updates one current logical record, subject to configured
  audit and retention;
- updating one current document many times updates one current logical head, subject to configured
  versioning and retention;
- creating new current records grows the store roughly in proportion to new useful state plus bounded
  metadata, not megabytes of stale pages for kilobytes of user payload;
- committing, checkpointing, exporting, or syncing intentionally promotes selected current state into
  immutable versioned history.

The mutable overlay provides current-state behavior without abandoning immutable CAS history.

## 2. Current Source-Backed State

The current implementation already contains pieces of the target but does not yet implement the full
architecture.

| Area | Current source-backed state | Target status |
| --- | --- | --- |
| MCP write facade | `StoreAccess::write` in `crates/loom-mcp/src/lib.rs` opens a writable store generation for individual mutations. | Move to shared mutable transaction API with short commit publication. |
| Engine-state saves | `Loom::save_state` in `crates/loom-core/src/vcs/state.rs` exports aggregate in-memory state. | Remove aggregate state saves from hot operational writes. |
| Store transaction finalization | `finish_txn` in `crates/loom-store/src/record_io.rs` writes metadata pages and fsyncs the committed root set. | Keep strict finalization for strict durability and promotion boundaries; add policy-driven group commit for hot mutable writes. |
| Mutable overlay records | `put_mutable_overlay_value`, `commit_mutable_overlay_records`, and related helpers in `crates/loom-store/src/lib.rs` provide a current-record path. | Expand into full MVCC mutable substrate with transaction batching, durability modes, and diagnostics. |
| Documents | `crates/loom-core/src/document.rs` contains overlay current-head work and checkpoint reads for document current state. | Move document heads and mutable metadata fully to the hot plane; retain bodies and promoted versions through CAS policy. |
| Tickets and lanes | `crates/loom-tickets/src/workflow_current.rs`, `crates/loom-tickets/src/service.rs`, `crates/loom-lanes/src/lib.rs`, and `crates/loom-mcp/src/writes.rs` contain current-record work. | Move workflow state fully to hot mutable tables and indexes. |
| Page-class attribution | `page_class_attribution` in `crates/loom-store/src/lib.rs` and `store attribution` in `crates/loom-cli/src/main.rs` attribute physical bytes by page class. | Use this as required evidence for storage-growth work. |

Accepted Matrix work has proven the overwrite path can be bounded for `scripts/loop/loop.loom`.
Random new-item creation has bounded regression coverage and manual probe attribution, but final
performance acceptance still depends on the manual report thresholds and compacted-copy byte
semantics defined below.

## 3. Performance Goals

Loom must be designed for multi-threaded, concurrent, hosted and local use.

| Goal | Requirement |
| --- | --- |
| Concurrent reads | Snapshot reads do not block hot writes. |
| Concurrent writes | Writers can prepare changes concurrently and only serialize the narrow commit publication step. |
| Bounded hot updates | Rewriting the same logical record many times does not grow physical storage linearly. |
| Reasonable new-record growth | Creating new logical records grows in proportion to useful data plus bounded metadata. |
| Configurable durability | Strict power-loss durability is available, but not forced on every operational write. |
| Explicit versioning | VCS commits, sync, export, and retained snapshots are explicit promotion boundaries. |
| Shared implementation | Tickets, lanes, pages, documents, and other facets use one mutable substrate, not one-off storage paths. |
| Observable storage | Diagnostics attribute physical bytes, live bytes, reusable bytes, stale pages, and unknown gaps. |

## 4. Performance Pillars

| Area | Recommendation | Why it matters |
| --- | --- | --- |
| Hot mutable substrate | Make this the primary path for operational state. | Avoid mini commits and CAS tree churn. |
| Configurable durability | Support `strict`, `normal`, `relaxed`, and `ephemeral`. | Avoid fsync bottlenecks for every small change. |
| MVCC reads | Readers see stable snapshots without blocking writers. | Required for concurrent agents, MCP, hosted APIs, CLI, and dashboards. |
| Group commit | Batch many logical commits into one fsync. | Core database technique for throughput. |
| Multi-facet transactions | One transaction can update ticket, page, document, and lane together. | Avoid six store commits for one logical workflow. |
| Page reuse | Reuse mutable pages quickly and safely. | Prevent file growth and stale page buildup. |
| Background checkpointing | Move WAL and overlay state into compact current pages asynchronously. | Keeps hot writes fast while controlling size. |
| Explicit VCS promotion | CAS/Merkle snapshots occur only at commit, checkpoint, export, and sync boundaries. | Preserves VCS semantics without taxing every write. |
| Derived artifact policy | Search, vector, dataframe indexes, projections, and caches are rebuildable unless explicitly retained. | Avoid storing derived churn as history. |
| Observability | Page-class attribution, write amplification metrics, and lock contention metrics are required. | Lets operators find bottlenecks from source-backed data. |

## 5. Durability Policy

Durability is a first-class store and transaction policy. Correctness remains strict. The policy only
controls when acknowledged transactions are guaranteed to survive process exit, OS crash, or power loss.

| Mode | Acknowledgement meaning | Process crash | OS crash or power loss | Intended use |
| --- | --- | --- | --- | --- |
| `strict` | Transaction is acknowledged only after the commit record or equivalent WAL commit is fsynced. | Recovered. | Recovered if storage honors fsync. | Ledger, audit, signing, explicit VCS commit, sync checkpoints, critical metadata. |
| `normal` | Transaction is appended to the WAL or mutable commit queue and fsync is grouped or periodic. | Recovered after process restart. | May lose the latest acknowledged window, but the store must not corrupt. | Default for tickets, lanes, pages, documents, PIM, KV, queue offsets, and normal operational state. |
| `relaxed` | Transaction may rely on OS flushing and can be reconstructed or tolerated if lost. | Best effort recovery. | Recent acknowledged writes may be lost. | Rebuildable search/vector indexes, derived projections, caches with backing data. |
| `ephemeral` | No durable recovery guarantee. | May be lost. | May be lost. | Sessions, temporary cache, runtime observations, volatile health state. |

The default store policy for user-facing operational facets is `normal`. `strict` is opt-in for
facets, operations, or store profiles that require per-transaction power-loss durability. VCS commit,
sync export, ledger append, audit checkpoint, and signed checkpoint operations force `strict`
durability unless a spec explicitly says otherwise.

### 5.1 Correctness guarantees that do not weaken

Every durability mode except `ephemeral` MUST preserve:

- atomic transaction boundaries;
- no torn logical state;
- no table/index mismatch after recovery;
- deterministic compare-token conflict behavior;
- idempotent retry behavior;
- checksummed or authenticated WAL/record frames;
- snapshot read consistency;
- recovery to either the prior committed state or a complete later committed state.

`normal` may lose the most recent acknowledged transactions after OS crash or power loss. It must not
recover a corrupted partial transaction. `relaxed` may lose rebuildable state but must not corrupt
canonical state. `ephemeral` is excluded from durable recovery.

### 5.2 Policy selection

Durability can be chosen at three levels, with narrower scope overriding broader scope:

1. store default policy;
2. facet or domain policy;
3. operation-level policy.

Examples:

- a store defaults to `normal`;
- the ledger facet uses `strict`;
- the search index uses `relaxed`;
- a ticket update uses `normal`;
- a `loom vcs commit` over current ticket state uses `strict` for the promotion boundary;
- a session cache uses `ephemeral`.

## 6. Concurrency Model

The target concurrency shape is:

```text
many readers + many prepared writers + one narrow commit publisher
```

Readers open MVCC snapshots by overlay generation and immutable base root. A reader does not block
writers unless it pins a generation that blocks reclamation. Writers prepare write sets concurrently,
validate owner tokens, and publish through a narrow serialized commit step.

The substrate must support:

- snapshot reads over overlay generation plus immutable base root;
- per-key or per-range write conflict detection by owner token;
- transaction-local read-your-writes;
- idempotency-key deduplication;
- group commit for fsync batching;
- bounded writer wait time under normal load;
- metrics for write-lock wait, commit batch size, fsync latency, and pinned-reader blockers.

The implementation may keep one serialized publisher for a store. It must not hold a coarse write lock
around full Loom object hydration, business logic, hosted request handling, or long-running scans.

## 7. Target Model

Reads compose two layers:

```text
logical Loom view
  hot mutable overlay: current records, tombstones, mutable heads, offsets, indexes, and deltas
  immutable base: committed CAS/Merkle roots and retained history
```

Lookup order is deterministic:

1. If the overlay has a current value for the logical key, return that value.
2. If the overlay has a tombstone for the logical key, return not found.
3. Otherwise, read the immutable base at the selected base root.

The overlay stores only changed logical records and metadata. It does not store a full snapshot of the
workspace, facet, or Loom.

### 7.1 MVCC snapshot handle contract

A snapshot handle is the read-side MVCC primitive. It is the contract that readers, promotion, and
reclamation all agree on. It is grounded in the current `OverlaySnapshot` and `OverlayCheckpoint` types
in `crates/loom-core/src/mutable_overlay.rs` and the store overlay publication path in
`crates/loom-store/src/lib.rs`.

**Identity.** A snapshot handle is identified by the pair `(overlay generation G, immutable base root
B)`. `G` is the store-local monotonic overlay generation (`OverlaySnapshot::generation`); `B` is the
immutable base root the overlay composes over (the store reference/CAS root selected when the handle is
opened, per section 13). Two handles with the same `(G, B)` observe byte-identical logical state. `G`
alone is not a complete snapshot identity: the same overlay generation over a different base root is a
different logical view.

**Composite-read semantics.** A read through a handle is deterministic (section 7 lookup order) and
visibility-filtered by `G`: the newest overlay entry for the key whose `entry.generation <= G` wins;
a `value` returns the payload, a `tombstone` returns not-found, and absence falls through to a base
read at `B`. This is exactly `OverlaySnapshot::read_composite`. A writer's own uncommitted generation
is visible to that writer (read-your-writes); other readers do not observe it until it is published.

**Pin lifecycle.** Opening a handle captures `G` and pins the overlay entry log that backs it (the
handle holds a reference to the shared entry storage, so entries visible at `G` remain readable even as
newer generations supersede them). A handle is *pinned* for its lifetime and *released* when dropped.
While a handle is pinned, reclamation MUST NOT free any overlay entry or physical page that is still
visible at that handle's `G`; the oldest live pin defines the reclamation frontier. Releasing the last
handle at or below a generation lets reclamation advance past it. Checkpoints (`OverlayCheckpoint`) are
long-lived pins used for promotion.

**Stale-reader behavior.** Writers advance `G` monotonically (`MutableOverlay::write` increments the
generation per entry). A held handle keeps observing its original `G` regardless of later writes, so a
long reader sees a stable, non-torn view rather than a moving target; it is never blocked by writers.
A stale handle only affects reclamation (it holds the frontier back while pinned), never write
progress. Operations that require current freshness detect staleness explicitly:
`MutableOverlay::validate_checkpoint` returns `Conflict` when a checkpoint's generation no longer
matches the live generation.

**Reader/writer concurrency.** The shape is many readers + many prepared writers + one narrow
serialized commit publisher (section 6). Readers never block writers and writers never block readers; a
pin blocks only reclamation, not writes. Write conflicts are detected optimistically by content-
addressed owner token (`put_value`/`put_tombstone` with an expected owner token; mismatch returns
`Conflict`), never by a coarse read lock.

**Checkpoint interactions.** An `OverlayCheckpoint` binds `(generation, the set of current keys at that
generation, snapshot)` and is the promotion handle of section 13. Promotion/commit reads the pinned
generation through composite reads while live writes advance to later generations; a checkpoint that is
stale for a freshness-requiring owner is rejected, while an explicitly requested older retained
generation is allowed. Reclamation preserves read equivalence for every live pin and checkpoint
(section 14).

**Diagnostics.** Handle state is observable: number of live checkpoint/pin references, the pinned
generation holding the reclamation frontier, reclaimable overlay pages, and blocked-reclamation reasons
(`MutableOverlayHealth`).

**Implementation-readiness notes (design stage; owned by the MVCC snapshot storage API task, queue
task 82).** The current `MutableOverlay` has no explicit pin registry or reference count:
`MutableOverlayHealth::live_checkpoint_references`, `reclaimable_overlay_pages`, and
`blocked_reclamation_reasons` are placeholders (`0`/empty). Implementing this contract requires a pin
registry that tracks the oldest live `G` so reclamation honors the frontier, and binding an explicit
base root `B` into the handle (today `read_composite` takes a base-read closure and the base is
implicit). These are the net-new pieces the snapshot storage API and reclamation tickets must add;
the read semantics, generation monotonicity, owner-token conflict detection, and composite-read order
already exist in source.

## 8. Logical Key Model

An overlay entry is addressed by one canonical logical key:

```text
overlay/<scope-kind>/<scope-id>/<domain>/<collection>/<record-kind>/<record-id>
```

Key segments are length-delimited byte strings, not slash-split filesystem paths. The text form above
is only a diagnostic rendering.

| Segment | Examples |
| --- | --- |
| `scope-kind` | `store`, `workspace`, `served-listener`, `session`, `derived` |
| `scope-id` | store-global zero id, workspace id, listener id, session id, derived-artifact key |
| `domain` | `vcs`, `document`, `tickets`, `lanes`, `pages`, `chat`, `queue`, `workgraph`, `control`, `derived` |
| `collection` | facet collection, project id, stream id, branch name, empty singleton segment |
| `record-kind` | `working-entry`, `stage-entry`, `document-head`, `ticket`, `comment`, `relation`, `project-setting`, `lane-order`, `consumer-offset`, `operation-cursor`, `derived-status`, `runtime-observation` |
| `record-id` | owner-defined identity inside the record family |

Keys MUST be canonical:

- every textual segment follows the owner facet's identity normalization rules;
- raw ids are encoded as canonical bytes, not display strings;
- there is exactly one logical key for one current record;
- duplicate canonical encodings for the same owner target are rejected;
- diagnostic renderings escape arbitrary bytes without changing canonical bytes.

## 9. Entry Record Model

Each overlay entry stores:

| Field | Meaning |
| --- | --- |
| `schema` | Overlay entry schema name and version. |
| `key` | Canonical logical key bytes. |
| `base_root` | Immutable base root or store generation the entry was derived from. |
| `generation` | Monotonic overlay generation that wrote the entry. |
| `owner_token` | Compare token for the owner target after the write. |
| `kind` | `value`, `tombstone`, `redirect`, or `ephemeral`. |
| `payload_ref` | Inline bytes or a digest reference to object storage for larger payloads. |
| `retention_class` | `current-only`, `audit-retained`, `history-retained`, or `ephemeral`. |
| `expires_at_ms` | Optional expiry for runtime observations and reclaimable tombstones. |
| `idempotency_key` | Optional caller or operation key used to deduplicate retried writes. |
| `operation_ref` | Optional retained operation-log record that explains the current value. |
| `durability` | Effective durability mode for the transaction that wrote this entry. |

`value` is the current record. `tombstone` masks the immutable base and any older overlay value.
`redirect` points to an immutable promoted root or retained record when a current value has been
materialized. `ephemeral` is durable only if the owner explicitly says runtime observations survive
reopen.

## 10. Page, WAL, and Index Layout

The mutable substrate uses Loom's single-file store as its durability substrate, but it must behave
like a database storage engine for hot state.

Target components:

```text
mutable substrate
  WAL or append queue: transaction intent and recovery records
  logical-key index: key -> latest entry
  current-state root: B-tree root containing only current entries
  owner-token index: owner target -> latest compare token
  MVCC generation index: generation -> visible root set
  retention index: retention class + expiry -> logical key
  checkpoint index: checkpoint id -> base root + generation + retained roots
  reclaim index: obsolete page run -> blocker reason
```

Large payloads stay as content-addressed object records or facet-owned payloads. Overlay pages store
entry envelopes, small scalar payloads, and pointers. The ordinary exact-read path must not depend on
retention or reclaim indexes.

Cold open must hydrate only the current logical overlay. Retained history, idempotency records,
owner-token records, secondary indexes, checkpoint metadata, audit retention records, and other
control records must live behind distinct persisted roots or a current-state root that lets open
traverse current entries without scanning every historical/control record. A store without the
current-state root may be handled only by a controlled migration path; normal open must not keep a
permanent scan fallback.

Small records must not waste a full durable transaction per tiny payload. The implementation must
support one or more of:

- transaction batching across a logical workflow;
- cross-transaction small-record arenas;
- group commit packing;
- mutable page-local updates with WAL protection;
- background checkpoint compaction of mutable record pages.

## 11. Transactions and Compare Tokens

An overlay transaction has one atomic write set:

```text
read immutable base + read overlay generation + compare owner tokens
write entries + update indexes + publish next visible generation
```

Preconditions:

- store generation: the writer opened an acceptable overlay generation;
- owner token: the owner target still has the expected current token;
- idempotency key: a retry with the same operation key returns the already-published result when the
  write set is identical.

Compare tokens are owner tokens, not storage page ids. Tokens are derived from owner target, prior
token, logical key, payload digest, tombstone state, and operation reference.

Conflict results are deterministic:

- stale store generation with no owner-token change may retry automatically;
- stale owner token returns the owner surface's conflict code;
- duplicate idempotency key with different payload returns `CONFLICT`;
- malformed or unknown token returns `INVALID_ARGUMENT`.

## 12. Multi-Facet Transactions

A logical workflow can update multiple facets and projections in one transaction. This is required for
performance and correctness.

Examples:

| Workflow | Single logical transaction writes |
| --- | --- |
| Agent closeout | Ticket status/comment, lane status summary, workgraph observation, metrics counter. |
| Page import | Page record, document body pointer, source import checkpoint, search invalidation marker. |
| Ticket creation with assignment | Ticket, rank, project counters, lane membership, audit operation. |
| Hosted request | Native facet mutation, audit record, capability/metrics observation, response cursor. |

Each workflow publishes one atomic visible generation unless the caller explicitly asks for separate
transaction boundaries. Adapters must use the shared transaction API. CLI, MCP, hosted, local client,
remote client, and bindings must not each hand-roll persistence semantics.

## 12.1 Shared Multi-Facet Transaction API

This subsection defines the shared API surface and error semantics that every adapter uses to commit
one logical workflow mutation. It is the single entry point that replaces the per-mutation writable
generation opened today by the MCP write facade (`StoreAccess::write` in `crates/loom-mcp/src/lib.rs`)
and the per-facade owner-token, idempotency, and durability handling scattered across
`crates/loom-mcp/src/writes.rs`. It composes the existing single-generation commit primitives
(`FileStore::commit_txn` and the overlay `finish_txn` publish path in `crates/loom-store/src/lib.rs`)
and the overlay entry model (`OverlayKey`, `OverlayOwnerToken`, `OverlayEntryKind`,
`OverlayDurabilityPolicy` in `crates/loom-core/src/mutable_overlay.rs`). It is a design target; section
21 tracks implementation state.

### Write set

A transaction is one ordered write set plus transaction-level policy. Each write names its facet, its
logical target as an `OverlayKey` (a logical key, never a storage page id), and an operation over the
overlay entry model. A write is either a value put or a tombstone delete, matching the entry kinds the
store already publishes.

```rust
pub struct WorkflowTransaction {
    pub workspace: WorkspaceId,
    pub actor: Principal,
    pub writes: Vec<FacetWrite>,
    pub durability: OverlayDurabilityPolicy,
    pub boundary: AtomicityBoundary,
    pub idempotency: Option<IdempotencyKey>,
}

pub struct FacetWrite {
    pub facet: FacetKind,
    pub target: OverlayKey,
    pub op: FacetWriteOp,
    pub expected: Option<CompareToken>,
    pub audit: Option<AuditIntent>,
    pub side_effects: FacetSideEffects,
}

pub enum FacetWriteOp {
    Put { payload: Vec<u8> },
    Delete,
}
```

`FacetSideEffects` carries the operation-log, revision-index, and reference-index intents the facet
would otherwise perform for the mutation, so the shared committer applies them inside the same
generation rather than letting a facet write them out of band. `AuditIntent` preserves audited-write
variants so a batching change cannot silently downgrade an audited write to a non-audited one.

Revision indexes are mutable current metadata even when the operations they index are retained
history. A facet must not persist its current revision index by staging
`.loom/substrate/revisions/<scope>.lri` as a VCS file during a daemon request. The current index must
use a shared mutable-overlay key qualified by canonical workspace identity and owner scope, and must
commit atomically with the owning current-state mutation and its retained operation record. A point
read or compare-token lookup must access only that key. Explicit promotion may project the index into
an immutable snapshot, but ordinary hot writes do not depend on a VCS save boundary.

Retained history must be physically append-addressed. Appending one operation or revision must not
decode, encode, or rewrite the complete prior operation log or revision index. Ordered segment
metadata, current-revision pointers, checkpoint indexes, and retention watermarks may be mutable, but
their update cost must remain bounded independently of total retained history. Page-chunked mutable
records are the fallback for growing current values because they can reuse fragmented free pages;
they do not satisfy this append-addressed requirement for operation or revision history by
themselves.

The shared FileStore retained-history contract stores each record under an owner-qualified sequence
address and replaces one bounded head record in the same workflow transaction. A stale expected
sequence rejects the transaction without publishing owner writes, history records, or the new head.
Page operation records and substrate entity revisions use this contract. Existing monolithic values
convert to retained records on their next successful owner write; the obsolete aggregate current
record is removed in that same transaction.

Owner-state publication point-reads only the overlay addresses it replaces and copy-on-write updates
the existing overlay B-tree root. It must not load every overlay entry, free the complete tree, or
rebuild the index from an empty root to append one retained record. Segment GC sweeps the active tail
segment in place: unreachable index entries are removed and dead-only pages are freed, while live
records stay where they are. Closed sparse segments may be evacuated. This separation prevents a
small active store from repeatedly relocating its complete live set whenever maintenance metadata
creates a few newly unreachable objects.

Large immutable object records use CRC-protected page chains so they can consume fragmented reusable
pages. GC and tail compaction enumerate the physical pages in a chain instead of assuming a
contiguous run. Tail compaction treats every object sharing a selected slab page as one physical
relocation unit, because freeing a slab after relocating only a subset would leave dangling object
index locators.

Physical append storage is not sufficient by itself. The writer resolves the affected entity's
latest revision and checkpoint uniqueness through bounded current point indexes. The manifest marks
when the point index is complete. An older incomplete manifest triggers one atomic backfill from
retained history; after that boundary, an unknown entity is resolved without reading retained
history. Full `RevisionIndex` reconstruction remains an explicit history-read operation.

A daemon-restart probe exposed the failure mode this rule prevents. The first page update and publish
persisted the overlay-backed page workspace, but the staged revision-index writes did not advance the
durable index. After daemon restart, a second update advanced the page to revision 2 and publish then
compared it with revision-index state 0, returning `CONFLICT: profile entity revision does not match
expected revision`. The compare check was correct; the out-of-band persistence location was not.
Pages, tickets, chat, drive, lifecycle, interchange, MCP, hosted, CLI, and conformance revision-index
paths must be audited and moved through one shared substrate contract.

The source audit assigns ownership as follows:

| Owner | Current metadata | Retained history | Read or maintenance surface |
| --- | --- | --- | --- |
| Pages | One mutable revision index per page workspace. Page workspace state, operation log, and revision-index write share one workflow transaction. | `PageOperationLog` and published `PageRevision` bodies remain retained owner data. | MCP and hosted page history read the shared current index. |
| Tickets | One mutable revision index per ticket workspace. Project, ticket, comment, and relation workflow-current writes are accumulated and published with indexed objects, reference root, profile control state, operation record, delivery notification, revision rows, field-reference indexes, and unresolved-reference candidates. | `TicketOperationLog` remains the retained ticket history. | Ticket history and projection code consume owner records rather than a staged revision-index file. Native create, field-changing update, and delete own field-reference derivation; CLI, MCP, hosted, client, and C ABI facades do not repeat it. |
| Chat | One mutable revision index per chat workspace. Staged channel streams, reference root, and message revision rows share one workflow transaction. | Channel streams retain chat operations. | Chat history readers resolve current revision rows through the shared substrate. |
| Drive | One mutable revision index per drive workspace. Profile state, operation log, metadata indexes, ACL changes, audit records, reference state, and revision rows share one workflow transaction. | Drive operation and version records remain retained owner data. | MCP delegates drive metadata mutations to native `loom-drive` operations and reads the shared current index. |
| Lifecycle | One mutable revision index per lifecycle workspace. Definitions, instances, surfaces, operation logs, snapshots, trigger state, reference root, and revision rows share one workflow transaction. | Lifecycle operation logs and snapshots remain retained owner data. | Lifecycle readers use the shared current index. |
| Meetings interchange | One mutable revision index per imported meetings workspace. Imported payload files, snapshot, checkpoint, audit rows, reference root, and revision rows share one workflow transaction. | Source payloads, import runs, and meeting snapshots remain retained owner data. | CLI rebuild and MCP meeting-review paths read and write the shared index. Duplicated MCP review mutations must delegate to the atomic owner API. |
| MCP and hosted | No independently owned revision index. | No duplicate history store. | These are authorized read/write facades over the owning facet index. |
| CLI | No independently owned revision index. | No duplicate history store. | Revision backfill reads and updates the shared owner index. |
| Protocol conformance | No persisted owner state. | Canonical fixtures remain test evidence. | Conformance checks load the shared owner index. |

`loom_substrate::versioning` owns the workspace-qualified current key, compare-token construction,
transactional write, and point snapshot refresh. Providers that cannot commit the complete workflow
transaction fail with `UNSUPPORTED`; no partial fallback is allowed. Runtime writers do not stage or
read `.loom/substrate/revisions/<scope>.lri`. The old path remains only as the stable ACL resource
identity used to authorize revision-history access.

The shared workflow transaction can publish mutable overlay records, content-addressed objects,
reference-root changes, control records, audit records, and revision-index writes through one
superblock commit. Pages, ticket creation, chat, drive, lifecycle, meetings import, and meetings
review use that boundary. Reopen tests cover the migrated owner paths, the daemon hot-write probe
passes across restarts, and a rejected combined transaction leaves the prior generation visible
after reopen. Document mutations prepare declared-index and reference-projection state before one
workflow transaction publishes the collection head, changed records, secondary indexes,
content-addressed engine objects, and engine-state root. Providers without workflow transactions fail
closed.

MCP composite transactions execute against an isolated planning engine seeded from the live
immutable state, mutable overlay, and authorization context. The planner validates every operation,
coalesces repeated document changes into one final collection-head and record write set, and publishes
the planned objects, engine-state root, and mutable writes in one workflow transaction. A failed plan
does not reach the durable store. A successful mixed CAS, document, graph, and view transaction and a
rejected document transaction both retain the expected state after reopen. Composite transactions are
single-workspace because the transaction generation and owner state have one canonical workspace.
MCP drive
mutations delegate to native `loom-drive` operations. Drive share mutations include the share index,
ACL snapshot, audit record, operation log, and revision row in the same publication and update the
in-memory ACL only after that publication succeeds. Meetings promotions prepare ticket, lifecycle,
decision-ledger, or reference-artifact target state and publish it with the source promotion record.
Ordinary ticket update, comment, and relation paths accumulate workflow-current projection writes,
coalesce repeated writes to one target, and publish them with canonical ticket state. Ticket expected
roots accept either the current profile root or the addressed ticket-current root, preserving profile
and entity compare semantics. Ticket field-reference indexing and unresolved-reference candidate
enqueueing execute inside the native ticket owner transaction. Ticket-key resolution uses the
already-open owner profile before reference state is staged, avoiding a read through a stale profile
control root. Facades may wake a runtime reconciler after a successful mutation, but they do not
derive or persist ticket reference state.

Page publication prepares its published-reference index before the workflow transaction. Canonical
`Body` text, explicit block references, page state, operation data, reference objects and controls,
and the revision-index row publish through one owner transaction. No post-commit page-reference
mutation remains.

Legacy staged `.lri` records require a controlled one-time conversion. Conversion validates the
workspace-qualified destination, writes equivalent canonical revision bytes, reopens and verifies the
destination, and only then removes the staged source. Restart with an already equivalent destination
resumes source cleanup; a divergent destination fails without changing either side. There is no
runtime compatibility reader or shipped migration command. The temporary Matrix conversion utility
was removed after the development-store record was validated after reopen.

### Compare tokens

A `CompareToken` is an owner token as defined in section 11, derived from owner target, prior token,
logical key, payload digest, tombstone state, and operation reference. It is not a storage page id.
Each write may carry an expected token for optimistic concurrency; the transaction may also carry an
expected store generation. All compare-token checks are evaluated against the one read snapshot the
transaction opened, so a stale token on any single write aborts the whole transaction before publish.

```rust
pub struct CompareToken(pub OverlayOwnerToken);
```

### Idempotency keys

A transaction carries at most one `IdempotencyKey`. A retry with the same key and an identical write
set returns the already-published commit result. A retry with the same key and a different write set
returns `CONFLICT`, matching the overlay idempotency contract already implemented for single-key writes
(`put_mutable_overlay_value_idempotent`). Per-write operation references still feed compare-token
derivation, but the transaction key governs replay of the whole set.

### Durability policy

The transaction default is an `OverlayDurabilityPolicy` value (`strict`, `normal`, `relaxed`, or
`ephemeral`). When facets in the write set carry differing policy overrides, the commit resolves to the
strictest policy touched, so a strict facet in the set forces a strict commit boundary. This matches
the facet-override resolution the store durability policy already defines. The resolved policy selects
the commit acknowledgement and fsync behavior: strict makes the journal commit fsync the commit point,
normal allows grouped or deferred fsync, and relaxed and ephemeral follow their weaker contracts.

### Atomicity boundary

The default boundary publishes the entire write set as exactly one visible overlay generation through a
single publish, so a reader never observes a partial workflow. All indexes the write set touches
(owner-token, retention, reclaim, operation log, revision, and reference indexes, plus any reference or
control root) advance within that one generation.

```rust
pub enum AtomicityBoundary {
    Single,   // default: one visible generation for the whole write set
    Separate, // caller explicitly opts into per-write boundaries
}
```

`Separate` exists only for callers that deliberately want independent boundaries and is discouraged;
the default is `Single`.

### Commit result

```rust
pub struct CommitReceipt {
    pub generation: OverlayGeneration,
    pub root_after: Digest,
    pub writes: Vec<WriteOutcome>,
    pub replayed: bool,
}

pub struct WriteOutcome {
    pub facet: FacetKind,
    pub target: OverlayKey,
    pub owner_token: OverlayOwnerToken,
    pub change: OverlayEntryKind,
}
```

`root_after` and `generation` identify the single published generation. Each `WriteOutcome` returns the
new owner token a later writer must present as its compare token. `replayed` is true when the receipt
was returned from an idempotent replay rather than a fresh commit.

### Error mapping

Errors are deterministic and extend the section 11 conflict results:

- stale store generation with no owner-token change is retryable: the caller re-prepares against a
  fresh snapshot;
- a stale owner token on any write returns that facet surface's conflict code and aborts the whole
  transaction;
- a duplicate idempotency key with a different write set returns `CONFLICT`;
- a malformed or unknown token returns `INVALID_ARGUMENT`;
- an authorization failure on any write returns that write's `PERMISSION_DENIED`, and the policy
  enforcement point runs per write before the batched commit rather than sharing one authorization
  across writes that were authorized separately;
- a durability policy that cannot be honored for the set, such as an ephemeral write mixed into a set
  that resolves to strict, returns `INVALID_ARGUMENT`;
- an unknown facet or an operation a facet does not support returns `UNSUPPORTED`.

Any error before publish leaves zero visible effect. This is the same all-or-nothing guarantee the
store commit path already provides, where a crash before the journal commit fsync discards the whole
batch.

### Avoiding facet-specific persistence shortcuts

- Every adapter routes each logical workflow mutation through this API. No adapter opens its own
  writable store generation per mutation or reimplements owner-token, idempotency, or durability logic.
- Each facet contributes a builder that turns its part of the workflow into overlay entries and
  side-effect intents; the shared committer composes the builders into one write set and is the only
  code that calls the store commit path. Facets never commit on their own.

```rust
pub trait WorkflowCommitter {
    fn commit(&self, txn: WorkflowTransaction) -> Result<CommitReceipt>;
}

pub trait FacetWriteBuilder {
    fn facet(&self) -> FacetKind;
    fn prepare(&self, snapshot: &OverlayReadSnapshot) -> Result<Vec<FacetWrite>>;
}
```

- A facet joins the API only after it satisfies the section 19 coalesced-transaction preservation
  checklist: operation logs, audit records, revision indexes, reference indexes, compare tokens,
  idempotency, ACL and policy enforcement, public and generated surface shape, regression coverage, and
  probe honesty must all be preserved across the coalesced boundary.

The existing atomic ticket-comment-plus-lane-summary closeout write (`write_lanes_closeout` composed
through `write_lanes_mutation` in `crates/loom-mcp/src/writes.rs`) is the concrete precedent this API
generalizes: it already commits two facet surfaces in one store mutation, and the shared API extends
that shape to arbitrary facet sets under one policy, one compare-token check pass, one idempotency key,
and one published generation.

## 13. Generations, Checkpoints, and Promotion

An overlay generation is a store-local monotonic integer plus the visible root set for hot mutable
state. A checkpoint id is a stable handle over:

- immutable base root;
- overlay generation;
- selected owner scopes included in the checkpoint;
- retained operation roots needed to explain current state;
- retention horizon and pruning blockers.

Checkpoint ids are not public commit ids. VCS commit, sync export, merge preparation, retained
history snapshot, and explicit user checkpoint consume a checkpoint and promote selected overlay
entries into immutable CAS/Merkle roots.

Live writes move to later generations while promotion reads the pinned generation. Promotion rejects a
stale checkpoint when the owner scope requires current freshness, and allows historical promotion when
the owner explicitly requested an older retained generation.

## 14. Tombstones and Retention

Tombstones are overlay entries. They are required for deletes because the immutable base may still
contain an older value.

| Retention class | Reclamation rule |
| --- | --- |
| `current-only` | May be reclaimed after all pinned checkpoints older than the tombstone are released and the base no longer exposes the deleted record through composite reads. |
| `audit-retained` | Remains explainable through operation logs until the audit horizon allows compaction. |
| `history-retained` | Promoted into immutable retained history. |
| `ephemeral` | Expires at the owner-defined deadline and is never exported as workspace history. |

Reclamation preserves read equivalence for every retained checkpoint. Store doctor reports distinguish
semantic reachability from physical reusability.

### 14.1 Superseded current-record reclaim eligibility

A superseded mutable current-record page is eligible for allocator reuse only when every logical view
that could observe that page has moved past it. Eligibility is evaluated per logical key and per
superseded page run before any page is returned to the free map.

The minimum reclaim contract is:

1. The logical-key current index no longer points at the superseded record. If the latest visible
   generation for the key is the superseded generation or older, the page is still semantically live.
2. No pinned MVCC snapshot can read the superseded record. A snapshot whose generation is greater
   than or equal to the superseded generation and less than the superseding generation blocks
   reclamation.
3. No retained history checkpoint can read the superseded record. Retained history uses the same
   generation-window rule as a pinned MVCC snapshot.
4. Audit-retained entries remain blocked until their operation log and audit horizon allow compaction.
5. A tombstone remains blocked while it is required to hide a value still visible from the immutable
   base through composite reads.
6. The durable-generation floor must be at or beyond the superseding generation. Group-commit,
   relaxed-durability, and recovery windows cannot reclaim a record whose replacement may still be
   rolled back by recovery.
7. Strict promotion, sync, export, ledger, and audit boundaries pin their selected checkpoint
   generation until the consumer finishes or rejects the checkpoint.

The corresponding blocker names are `current-root-visible`, `pinned-snapshot`,
`retained-history`, `audit-retention`, `tombstone-retention`, `durable-generation-window`, and
`strict-promotion-boundary`. Reclaim planning may expose multiple blockers for one page run, and it
must not collapse them into a generic "not reclaimable" state because operators need to know whether
storage growth is caused by readers, retention policy, recovery safety, or promotion consumers.

Until the full MVCC snapshot-handle API lands, implementation fixtures use the generation-window
contract above as the minimum source-backed snapshot model. Allocator reuse must not assume that a
missing snapshot API means there are no pinned readers.

## 15. Facet Scope

The default rule is: hot mutable unless identity or retained history is the primary product semantic.

| Facet or surface | Hot mutable shape | VCS or CAS shape | Default durability | Notes |
| --- | --- | --- | --- | --- |
| Tickets | Tables for ticket, fields, comments, relations, ranks, project settings, and history heads. | Snapshot project or ticket collection when explicitly promoted. | `normal` | Definitely hot mutable. |
| Lanes | Coordination rows plus lane-ticket rank table. | Usually excluded from VCS unless explicitly included. | `normal` | Operational coordination state. |
| Pages | Draft table, published table, metadata, relations, import checkpoints. | Snapshot published pages, optionally drafts. | `normal` | Similar to Notion or Confluence current state. |
| Documents | Current heads, content refs, metadata, text/binary state. | Snapshot selected heads and content refs. | `normal` | Content blobs can still be CAS; heads are mutable. |
| Mail and PIM | Message records, folders, flags, calendar objects, contacts. | Snapshot mailbox/calendar state by policy. | `normal` | Flags and read state must not be mini commits. |
| KV and cache facades | Mutable keyspace with TTL and index metadata. | Optional checkpoint/export. | `normal` for durable KV, `ephemeral` for cache. | Redis-like storage semantics belong here or in a facade over this substrate. |
| Queue and stream | Append log plus consumer offsets. | Checkpoint stream segments by retention policy. | `normal` | Consumer offsets are mutable operational state. |
| Metrics, logs, traces | Time-partitioned append segments, rollups, and indexes. | Snapshot/export partitions by policy. | `normal` or `relaxed` for derived rollups. | High ingest, not VCS per point. |
| Search, FTS, vector | Mutable indexes and derived artifacts. | Rebuildable or checkpointed derived state. | `relaxed` | Do not CAS-commit every index mutation. |
| SQL, columnar, dataframe | Mutable tables, catalogs, column chunks, transaction log. | Snapshot table versions at commit boundaries. | `normal` | Requires MVCC. |
| Graph | Node and edge tables plus adjacency indexes. | Snapshot graph roots. | `normal` | Traversal indexes are mutable or derived. |
| Ledger and audit | Append-only log with signatures and transparency roots. | CAS/Merkle roots are central. | `strict` | One of the few CAS-native domains. |
| OCI, S3, CAR, artifact transfer | Object manifests, blob refs, bucket/object metadata. | Immutable object payloads and manifests. | `strict` for manifest publication, `normal` for mutable bucket metadata. | Split metadata from immutable payload identity. |
| Runtime sessions and observations | Session state, health checks, listener status, lock telemetry. | Audit config changes only. | `ephemeral` or `relaxed` | Runtime state is not workspace history. |

### 15.1 Durability and retention default classification

This subsection adds the retention-class and retention-default dimension on top of the durability
defaults in the section 15 table. It does not restate the hot-mutable and VCS/CAS shapes from section
15, and it cross-references the promotion targets in section 16. The retention class of each facet is
drawn from these five values: `operational-current` (hot mutable working state), `immutable-promotion`
(CAS or VCS identity fixed at an explicit boundary), `rebuildable-derived` (local artifacts outside
commit identity), `ephemeral-runtime` (volatile state that is never workspace history), and
`strict-audit` (append-only ledger, audit, or export boundaries). A facet may list a primary class and
a secondary class when it owns more than one kind of state. Default durability values match section 15
exactly.

| Facet | Retention class (primary; secondary) | Default durability | Retention default | Source anchor |
| --- | --- | --- | --- | --- |
| Tickets | operational-current; strict-audit | `normal` | Ticket, comment, relation, board, lane, and active-assignment current records are mutable tables and delete through overlay tombstones; the ticket operation log is appended and its revision index is retained, so workflow history and audit are kept by default. | `crates/loom-tickets/src/workflow_current.rs` (`WorkflowCurrentRecordKind`, `operation_root`); `crates/loom-tickets/src/indexed.rs` (`append_operation`, `put_tombstone`, `put_mutable_overlay_tombstone`); `crates/loom-tickets/src/model.rs` (`PROFILE_OPERATION_LOG_SCHEMA` = `loom.studio.tickets.operation-log.v1`, `TicketOperationLog`). |
| Lanes | operational-current | `normal` | Coordination rows and the lane-ticket rank table are current-only operational state; no version history and no VCS inclusion are kept by default, and ad-hoc ticket-level coordination fields are rejected at the coordination boundary. | `crates/loom-lanes/src/lib.rs` (`LANE_COLLECTION`, coordination-boundary rejection at `lane_decode_rejects_ad_hoc_coordination_fields`); consumes `loom_tickets::workflow_lane_current_record`. |
| Pages | operational-current; immutable-promotion | `normal` | Draft and published tables are mutable current state; the page operation log is appended and the revision index is retained, so edit history is kept by default; published pages are snapshotted only on explicit promotion. | `crates/loom-pages/src/lib.rs` (`save_workspace_and_append_operation`, `update_page_operation_revision_index`, `PageOperationLog`, `PageRevision`, `RevisionIndex`); `loom_substrate::versioning::revision_index_path`. |
| Documents | operational-current; immutable-promotion | `normal` | Collection heads and per-id live records are mutable; large bodies are stored as content-addressed chunks that dedup and version through the engine; the default tombstone retention policy is `no-retained-tombstones.v1`, so deletes are not retained unless a collection selects `retain-tombstones.v1`. | `crates/loom-core/src/document.rs` (`DOCUMENT_RETENTION_POLICY_NONE` default at manifest read, `DOCUMENT_RETENTION_POLICY_RETAIN`, `DocumentBodyRef::Chunked`, `DocumentTombstoneRecord`, `DOCUMENT_CURRENT_HEAD_KIND`). |
| Mail and PIM | operational-current | `normal` | Message records and mailbox metadata are mutable; flags live in a separate versioned sub-tree so flag churn diffs independently and is squash-bounded by the default flag retention policy of a 30-day detailed delta window and 10000 detailed deltas; calendar and contact records use a content-address ETag with a commit sync-token, and deletes are current-state mutations. | `crates/loom-core/src/mail.rs` (`FLAGS_DIR`, `FLAG_RETENTION_POLICY_FILE`, `DEFAULT_FLAG_DELTA_WINDOW_MS`, `DEFAULT_MAX_DETAILED_FLAG_DELTAS`, `MailFlagRetentionPolicy::default`); `crates/loom-core/src/calendar.rs`; `crates/loom-core/src/contacts.rs`. |
| KV and cache facades | operational-current; ephemeral-runtime | `normal` for durable KV, `ephemeral` for cache | The durable keyspace is a mutable versioned map whose anchors retain deleted-key generations; the ephemeral tier carries per-entry TTL and idle-TTL with an eviction policy and no default TTL, so cache entries expire and are never workspace history. The canonical cache lifecycle facet is planned. | `crates/loom-core/src/kv.rs` (`KvTier::Ephemeral`, `EphemeralPutOptions`, `EvictionPolicy`, anchor deleted-key generation comment); cache facet is a planned facet in `specs/_FACET_PRIMITIVES.md` (Approved Planned Facets). |
| Queue and stream | operational-current; ephemeral-runtime | `normal` | The append log is retained down to the retained low-water mark, and the shared change-set gap vocabulary (`retained`, `planned_prune`, `gap`) governs replayability; consumer offsets are operational metadata stored outside the committed stream tree and are excluded from commits, clone, push, and sync (authority-local by default). | `specs/0021b-queue-consumer-offsets.md` (RD1 offset storage, offsets excluded from commits/sync); `crates/loom-core/src/change_set.rs` (`retained_low_water_mark`, `require_not_before_low_water`, `GapState`). |
| Metrics, logs, traces | rebuildable-derived; operational-current | `normal`, `relaxed` for derived rollups | Raw time-series points are retained until an explicit destructive prune; the default time-series policy declares no automatic prune horizon; rollups are derived views materialized over points and carry rebuild and window status. Logs and traces have core record and query scaffolding but their retention defaults are not yet source-backed (see gaps). | `crates/loom-core/src/timeseries.rs` (`TimeSeriesPolicy::default`, `ts_prune_before`); `crates/loom-core/src/metrics.rs` (`MetricRollupProfile`, `MetricRollupMaintenanceResult`, `MetricMaterializedRollup`); `crates/loom-core/src/logs.rs`, `crates/loom-core/src/traces.rs`. |
| Search, FTS, vector | rebuildable-derived | `relaxed` | Indexes are derived artifacts stamped against the committed source digest; a mapping remap rebuilds the derived index from source; indexes are not CAS-committed per mutation and carry no independent retention, since a full resync rebuilds them from the facet source. | `crates/loom-core/src/search.rs` (`search_source_digest`, remap rebuild comment, `is_write_index`); engines `crates/loom-tantivy`, `crates/loom-vector`, `crates/loom-hnsw`; shared derived-artifact rebuild contract in `specs/_FACET_PRIMITIVES.md`. |
| SQL, columnar, dataframe | operational-current; immutable-promotion | `normal` | Tables, catalogs, and column segments are mutable and versioned through the engine with MVCC; table and columnar versions are snapshotted at commit boundaries; the dataframe facet stores only Loom-readable plan and source-binding records, and engine-native execution state is not identity and is ephemeral. | `crates/loom-core/src/tabular.rs` (versioned-table facade, committed table slots); `crates/loom-core/src/columnar.rs` (append-oriented segments, `columnar_source_digest`); `crates/loom-core/src/dataframe.rs` (plan state only, engine-native state is not identity). |
| Graph | operational-current; immutable-promotion | `normal` | Node and edge state plus adjacency live in a root Tree of component prolly maps that versions and syncs through the engine; graph roots are snapshotted on commit; traversal indexes are derived or mutable and no dangling edges are retained. | `crates/loom-core/src/graph.rs` (root Tree with component prolly maps, `upsert_node`, `remove_node` cascade rules). |
| Ledger and audit | strict-audit | `strict` | The chain is append-only and tamper-evident; entries are chained and never silently reordered or interleaved, signed checkpoints and inclusion and consistency proofs are retained, and declared retention ranges never drop the append-only log by a durability change. | `crates/loom-core/src/ledger.rs` (`ledger_append`, `LEDGER_SIGNED_CHECKPOINT_V1`, `LEDGER_INCLUSION_PROOF_V1`, `LEDGER_CONSISTENCY_PROOF_V1`, append-only chain header comment, retention-range validation). |
| OCI, S3, CAR, artifact transfer | immutable-promotion; operational-current | `strict` for manifest publication, `normal` for mutable bucket metadata | Object payloads and manifests are immutable content-addressed blobs that dedup and are held by GC reachability with no derived index; bucket and object metadata are mutable current state served over the hosted routers. | `crates/loom-core/src/cas.rs` (`cas_put`, content-address dedup, GC reachability, no derived index); `crates/loom-hosted/src/serve.rs` (`OciRestState`, `S3RestState`, `oci_rest_router`, `s3_rest_router`). |
| Runtime sessions and observations | ephemeral-runtime; strict-audit | `ephemeral` or `relaxed` | Session state, health, listener status, and lock telemetry are held in the owner-only daemon runtime directory, not in the committed store, and are never workspace history; only configuration changes are audited. | `crates/loom-store/src/daemon.rs` (`runtime_dir`, `DaemonStatus`, `OwnerRuntimeDirectory`, owner-match checks); section 16 Control plane row. |

Cross-cutting retention rules:

- Audit and operation logs are strict and are never dropped by a durability change. A relaxed or
  ephemeral durability selection on a facet lowers write-path guarantees for hot mutable records only;
  it never drops the ticket, page, ledger, or audit operation logs, and it never removes a retained
  low-water mark or a signed checkpoint.
- Derived artifacts are rebuildable and carry no independent retention unless an owning spec explicitly
  retains them. Search, FTS, vector, metric rollups, and traversal indexes are rebuilt from a stamped
  source digest after checkout, sync, or invalidation rather than being preserved as history.
- Ephemeral runtime state is never workspace history. Cache entries, consumer offsets, daemon session
  and health telemetry, and engine-native execution state stay outside commit identity, clone, push,
  and sync, and only their configuration changes are auditable.

Derived artifact policy integration:

| Artifact family | Canonical source state | Hot current state | Derived payload default | Retained metadata | Must not become immutable history by accident | Source anchors |
| --- | --- | --- | --- | --- | --- | --- |
| Search and FTS | Mapping, alias configuration, and document map. | Source digest, engine version, rebuild status, stale and failure state, invalidation marker. | `relaxed`; rebuild from source after remap, checkout, sync, or engine-version change. | Mapping, alias target and `is_write_index`, source digest, format version, engine version, rebuild status. | Tantivy segment files, embedding projection bytes, scorer caches, highlight caches, and query-result caches. | `crates/loom-core/src/search.rs` (`SearchCollection`, `SearchAliasTarget::is_write_index`, `search_source_digest`, `search_remap`); `crates/loom-store/src/derived.rs` (`SEARCH_TANTIVY_KIND`, `SEARCH_EMBEDDING_KIND`, `search_tantivy_artifact_stamp`). |
| Vector accelerators | Vector manifest, id-keyed vectors, source text, embedding-model declaration, and declared metadata-index keys. | Source digest, accelerator policy, build status, stale and failure state, and declared metadata-index markers. | `relaxed`; exact search remains the source-backed contract, while ANN and PQ accelerators are rebuildable. | Vector dimension, metric, metadata-index declarations, source digest, accelerator family, format version, engine version. | HNSW graphs, PQ codebooks, ANN scratch buffers, candidate caches, and native accelerator memory maps. | `crates/loom-core/src/vector.rs` (`encode_manifest`, `vector_source_digest`, metadata-index marker paths, `build_pq_index`, `search_with_policy_auto`); `crates/loom-vector/src/lib.rs` (`VectorSet::entries`, `PqIndex`); `crates/loom-hnsw/src/lib.rs` (`HnswIndex` derived rebuildable accelerator); `crates/loom-store/src/derived.rs` (`VECTOR_PQ_KIND`, `VECTOR_HNSW_KIND`). |
| Dataframe and columnar projections | Dataframe logical plan and source bindings; columnar manifests and durable columnar segment payloads when a materialization target explicitly selects columnar, files, or CAS. | Plan digest, source digest set, materialization id, engine version, rebuild status, stale and failure state. | `relaxed` for dataframe materialization records and columnar Arrow projection artifacts; `normal` for explicitly materialized columnar, files, or CAS outputs because those become user-selected source state. | Plan digest, source binding digests, materialization policy, destination, format, artifact family, format version, engine version. | Polars execution plans, Arrow projection caches, preview batches, temporary scan buffers, statistics caches, and native engine state. | `crates/loom-core/src/dataframe.rs` (`dataframe_source_digests`, `dataframe_materialize_auto`, plan-state header); `crates/loom-core/src/columnar.rs` (`stage_columnar_reserved`, `columnar_source_digest`); `crates/loom-store/src/derived.rs` (`DATAFRAME_MATERIALIZATION_KIND`, `COLUMNAR_ARROW_KIND`). |
| Read projections and presentation caches | Native ticket, lane, page, document, drive, and meeting records plus retained operation or revision indexes where those indexes are the source-backed audit or history surface. | Projection selection, display profile, freshness token, source digest set, and stale/failure status for expensive projections. | `relaxed` for rebuildable projection outputs and presentation caches; retained revision and operation indexes keep their owning facet durability. | Projection profile config, selected projection kind, source digest set, revision index root where owned by the facet, and operation-log root where owned by the facet. | Rendered ticket/list JSON, board-card caches, page/document preview caches, drive OS projection hydration cache, meeting vector projection payloads, and app presentation bundles unless explicitly promoted by owner policy. | `crates/loom-tickets/src/service.rs` (`ticket_projection_selection`, `update_ticket_revision_index`, `derived_ticket_key`); `crates/loom-pages/src/lib.rs` (`update_page_operation_revision_index`, `update_revision_index`); `crates/loom-drive/src/lib.rs` (`plan_os_projection_worker`, `update_file_revision_index`); `crates/loom-mcp/src/writes.rs` (`studio_reindex_job`, `meetings_vector_projection_job`, `current_workspace_source_digests`). |

Unresolved gaps (default not yet source-backed; no default is invented here):

- Logs facet retention default. `crates/loom-core/src/logs.rs` provides record and query scaffolding,
  but the logs facet is a planned facet in `specs/_FACET_PRIMITIVES.md` and no source-backed prune,
  legal-hold, or retention-class default exists yet.
- Traces facet retention default. `crates/loom-core/src/traces.rs` provides span scaffolding, but the
  traces facet is planned and has no source-backed retention or sampling-evidence prune default.
- Metrics facet automatic prune horizon. Time-series prune is explicit through `ts_prune_before` and
  the default policy declares none, but the planned metrics facet cardinality and retention policy has
  no source-backed automatic default.
- Cache facet lifecycle default. The `kv` ephemeral tier primitives are source-backed, but the
  canonical cache facet capacity, accounting, and eviction default policy is planned, not source-backed.
- Shared automatic retention compaction. `specs/_FACET_PRIMITIVES.md` records that automatic retention
  compaction and shared retention policy remain target work, so the cross-facet automatic-compaction
  default is not source-backed.

## 16. Promotion Bridge by Consumer Group

| Consumer group | Overlay current records | Promotion target |
| --- | --- | --- |
| VCS working state | Working entries, explicit dirs, staging entries, merge metadata, protected-ref current state. | Tree objects, commits, ref updates, retained merge records. |
| Document heads | Collection head, per-id live record, tombstone, selected body pointer. | Document collection root, retained document version, body and chunk CAS references. |
| Workflow state | Ticket, comment, relation, project setting, lane order, board projection, active assignment, workgraph latest state. | Retained ticket and workgraph operation logs, audit roots, optional project checkpoint roots. |
| Cursors and offsets | Queue offsets, chat cursors, delivery positions, operation-change cursors, revision-head pointers. | Retained cursor snapshots only when owner policy says they are history or sync material. |
| Derived state | Selected artifact root, rebuild status, freshness token, invalidation marker. | Derived-artifact catalog entry or no promotion when rebuildable payloads stay local. |
| Control plane | Listener health, runtime observations, lock telemetry, capability availability. | Audit records for configuration changes; volatile observations are not promoted. |

Promotion does not back-propagate adapter state into native facet identity. Hosted facades, MCP views,
SQL wire adapters, and board projections consume native overlay records or derived projections; they
do not define independent current-state truth.

### 16.1 VCS/CAS promotion bridge semantics

The VCS/CAS promotion bridge is the only path that turns selected hot current records into immutable
workspace history. Ordinary hot writes publish overlay entries and owner tokens only; they do not
create VCS commits, branch movements, tree objects, or public commit ids.

Promotion starts from an `OverlayCheckpoint` or equivalent store snapshot handle identified by
immutable base root plus overlay generation. The checkpoint selection is owned by the promoting
consumer:

- `loom vcs commit` and `commit --staged` require current freshness for the workspace VCS owner scope.
  If the selected checkpoint is stale for that scope, promotion fails with a compare-token or stale
  checkpoint conflict and the caller retries from a fresh snapshot.
- sync, export, ledger, audit, and explicit retained-history checkpoints may select an older retained
  generation when the owner requested historical state. That selection pins the generation until the
  consumer either completes or rejects the checkpoint.
- background mutable-overlay page checkpointing is storage compaction, not history promotion. It may
  rewrite compactable current records into denser pages and free old pages, but it must not mint
  commit ids, change VCS refs, or change the logical current state.

Owner scopes are explicit. A promotion request names the workspace and the owner domains it includes.
The default VCS scope includes VCS working entries, explicit directories, staged entries, merge
metadata, protected-ref current state, and document current heads that are materialized into the
workspace tree. Workflow records, lane coordination state, cursors, offsets, derived artifacts, and
runtime observations are excluded unless the owner policy names a promotion target for that domain.

The bridge materializes included current records into the immutable plane by owner-domain projection:

- VCS working entries and explicit directories become Tree entries, then Commit objects and ref
  updates.
- Document collection heads and live records become document collection roots, retained document
  versions, and body or chunk CAS references according to the document retention policy.
- Workflow records become retained operation logs, audit roots, or optional project checkpoint roots.
  They do not become branch-visible files by default.
- Derived artifacts promote only their catalog entry or freshness token when retained by policy.
  Rebuildable payloads stay local.
- Control-plane runtime observations are never workspace history. Configuration changes are auditable,
  but listener health, lock telemetry, and capability availability remain operational state.

Strict boundaries apply for promotion consumers. VCS commit, sync export, ledger append, audit
checkpoint, and signed checkpoint operations force `strict` durability. Reclamation treats the
selected promotion generation as pinned by a `strict-promotion-boundary` blocker until the consumer
finishes. Normal, relaxed, or ephemeral hot writes cannot weaken the audit, ledger, sync, or VCS
promotion boundary.

Namespace behavior remains VCS-owned. Overlay keys are length-delimited logical keys, not filesystem
paths, and diagnostic renderings do not participate in VCS namespace checks. Only promoted VCS tree
paths are checked for leaf-or-parent collisions before commit. This prevents document, ticket, lane,
MCP, hosted, or board projection keys from leaking into ordinary hot writes as VCS path semantics,
while still preserving the VCS preflight guarantee for materialized commit trees.

## 17. Observability and Acceptance Metrics

Storage work is not complete without source-backed diagnostics.

Required metrics and diagnostics:

- physical bytes;
- useful live bytes;
- reusable free bytes;
- stale record slab pages;
- stale record large pages;
- object record pages;
- mutable overlay record pages;
- object index tree pages;
- mutable overlay tree pages;
- free-map pages;
- region table pages;
- maintenance pages;
- journal and checkpoint overhead;
- unknown/unclassified pages;
- group commit batch size;
- fsync latency;
- write-lock wait time;
- reader snapshot pins;
- transaction count per logical workflow.

Acceptance for random new-item writes is not "compaction can fix it." The online file size must be
reasonable relative to useful data plus bounded metadata. A 13x to 14x ratio between online size and
compacted useful state for tiny item bundles is not an acceptable target.

Small records created by one transaction are slab-packed. Records replacing existing addresses use
independently reclaimable pages, and a shared slab is reclaimed only when every resident live slot is
superseded in the same transaction. This preserves neighboring records while removing the dedicated
4 KiB page cost per new record. A fresh eight-bundle random probe followed by a second eight-bundle
batch measured 483,328 bytes of marginal growth for the second batch, down from 1,720,320 bytes before
packing. Cross-transaction reuse of a partially occupied slab remains a separate page-format design
problem because in-place append would violate crash atomicity and copy-on-write append would change
every resident locator.

### CLI and daemon latency attribution

`scripts/loop-speed-probe.sh` runs a fixed number of ticket, page, document, and lane mutations. It
records every command's wall time, every iteration's total time and store size, rolling throughput,
first- and last-quartile latency, and per-operation averages. It writes CSV artifacts so growth and
latency can be correlated without parsing terminal output.

A fresh 20-iteration run exposed two phases. Iterations one through seven took 2.49 to 3.37 seconds
while newly freed pages remained inside the 32-generation recovery window. Once reuse became legal,
iteration time fell to 700 milliseconds, then climbed to 930 milliseconds by iteration 20. A second
20-iteration run over the same store climbed from 1.07 to 1.39 seconds after its maintenance-startup
outlier. Later runs reached 1.45 to 1.89 seconds as retained history increased.

The source-backed causes are:

- `FileStore::open` calls `load_mutable_overlay_from_storage`, which walks the complete overlay
  B-tree and reads every record. Current entries, retained-history entries, owner tokens, secondary
  indexes, and idempotency records share that tree, so open cost grows with retained history even
  though only current entries are imported into the in-memory overlay.
- CLI facet commands call `cli_open_loom`, creating and hydrating a new `FileStore` and `Loom` for
  each process. A running daemon authorizes the file session but does not execute ticket, page,
  document, or lane operations through its already-open engine. Store diagnostics consequently
  reported zero group-commit batches for the entire probe.
- Daemon maintenance starts with an immediately eligible reconciliation check. Eligible maintenance
  competes with foreground commands for the store and produced first-iteration latency between 4.77
  and 8.13 seconds in warmed runs. The accompanying file shrink confirms that these were maintenance
  outliers rather than ordinary mutation cost.
- `mutable_overlay_record_payload` previously loaded the complete B-tree for a logical point lookup,
  and workflow transactions exported complete in-memory overlay history for rollback and again to
  find newly written records. Point B-tree reads, current-entry map lookups, and copy-on-write rollback
  snapshots now remove those avoidable scans. End-to-end latency still grows because process-open
  hydration remains dominant.

The target architecture separates current mutable records from retained/control records at the
persisted index-root level, allowing current-state hydration and point reads to scale with current
state rather than history. When a local daemon is available, CLI operations use the shared generated
client/dispatch contract against the daemon's persistent engine instead of adding facet-specific
daemon commands. Maintenance performs bounded background steps and yields between budgets so it does
not impose multi-second foreground latency.

### 17.1 Performance report schema

The manual performance report is a JSON object with this required shape:

```text
{
  "command": string,
  "iterations": unsigned integer,
  "artifacts": {
    "json": path string,
    "summary": path string
  },
  "scenarios": [
    {
      "name": string,
      "status": "completed" | "skipped" | "failed",
      "skip_reason": string | null,
      "operations": unsigned integer,
      "elapsed_ms": number,
      "operations_per_second": number,
      "p50_latency_ms": number | null,
      "p95_latency_ms": number | null,
      "p99_latency_ms": number | null,
      "transaction_count": unsigned integer,
      "write_lock_wait_ms": number | null,
      "storage": {
        "physical_bytes": unsigned integer,
        "useful_live_bytes": unsigned integer,
        "live_bytes": unsigned integer,
        "reusable_free_bytes": unsigned integer,
        "reclaimable_bytes": unsigned integer,
        "metadata_bytes": unsigned integer,
        "compacted_bytes": unsigned integer,
        "overlay_current_records": unsigned integer,
        "overlay_obsolete_records": unsigned integer,
        "overlay_obsolete_pages": unsigned integer,
        "retained_checkpoint_blockers": unsigned integer
      } | null,
      "stale_page_classes": [
        {
          "class": string,
          "pages": unsigned integer,
          "bytes": unsigned integer
        }
      ],
      "growth_domains": [
        {
          "domain": string,
          "current_records": unsigned integer,
          "obsolete_records": unsigned integer,
          "payload_bytes": unsigned integer
        }
      ],
      "extra": object<string, string>
    }
  ]
}
```

For storage-growth scenarios, `storage.compacted_bytes` means the physical byte size of an equivalent
compacted copy of the scenario store. It is not a count of bytes relocated by tail compaction. If a
scenario cannot create a compacted copy, it must set `status` to `failed` or put a clear
`compacted_bytes_unavailable` explanation in `extra`; it must not silently emit zero as if compaction
had produced an empty store.

The report MUST include at least these completed scenario names before a storage-performance change
can claim 0071 performance acceptance:

| Scenario | Required purpose |
| --- | --- |
| `hot_mutable_overwrite` | Repeated writes to one logical current-record key. |
| `random_new_ticket_lane_page_document_bundles` | New ticket, lane, page, and document current-record keys per bundle. |
| `concurrent_readers_and_writer` | Snapshot readers opening while a writer mutates current records. |
| `vcs_promotion` | Explicit promotion from mutable document current state into immutable VCS history. |

Durability scenarios for `strict`, `normal`, `relaxed`, and `ephemeral` may remain `skipped` only
until those durability modes are source-backed in the store contract. Once configurable durability
exists, skipped durability scenarios are a failing report.

### 17.2 Acceptance thresholds

The first accepted thresholds are intentionally simple and source-backed. They are failure gates, not
benchmark targets. A better implementation may tighten them after it records a representative baseline
artifact and updates this section in the same change.

| Scenario | Required threshold |
| --- | --- |
| Hot overwrite | `storage.overlay_current_records == 1`; `operations >= iterations`; `transaction_count <= operations + 2`; `storage.physical_bytes <= max(storage.compacted_bytes * 3, storage.compacted_bytes + 512 KiB)`; `storage.reusable_free_bytes + storage.reclaimable_bytes <= storage.physical_bytes / 3`; `p99_latency_ms` is present and finite when at least 20 latencies are sampled. |
| Random new item bundles | `storage.overlay_current_records == operations`; growth domains include `tickets`, `lanes`, `pages`, and `documents`; each of those domains has `obsolete_records == 0`; `storage.physical_bytes <= max(storage.compacted_bytes * 4, storage.compacted_bytes + 1 MiB)`; `storage.reusable_free_bytes + storage.reclaimable_bytes <= storage.physical_bytes / 4`; stale page classes must not dominate the report. |
| Concurrent readers and writer | `extra.reader_failed_opens == "0"`; `operations >= iterations`; `p99_latency_ms` is present and finite when at least 20 writer latencies are sampled. |
| VCS promotion | `extra.commit` is present; `extra.promotion_latency_ms` is present and finite; promoted document bytes are non-zero; the scenario does not increase `overlay_obsolete_records` during promotion. |

For this table, "stale page classes must not dominate" means the sum of stale record, stale tree,
stale region table, stale maintenance, stale free-map, reusable free, and unreferenced unclassified
classes reported in `stale_page_classes` is less than or equal to 25 percent of
`storage.physical_bytes`. Tail-free pages are excluded from this stale-page sum because they are
immediately shrinkable and should be judged through `reusable_free_bytes`, tail-trim eligibility, and
the compacted-copy size.

The ratio thresholds compare online size to compacted-copy size because random new logical items have
legitimate payload and index growth. They are not allowed to pass by deleting semantic side effects:
operation logs, audit rows, revision indexes, reference indexes, compare-token checks, idempotency,
ACL/PEP enforcement, and public output shape still have to pass the semantic-preservation checklist.

## 18. Manual Performance Test Suite

Performance validation is required, but it must not make the default developer gate slow. The default
test path remains unit-sized. Persistence volume tests, daemon-backed write loops, concurrent client
tests, durability-mode probes, and growth attribution runs belong in a manual performance target.

The repository SHOULD provide a manual command such as:

```sh
just test-performance
```

The performance suite MUST:

- create fresh temporary stores unless a test explicitly exercises migration;
- report elapsed time, operation count, operations per second, and p50, p95, and p99 latency where
  timing is meaningful;
- report physical bytes, useful live bytes, reusable free bytes, stale page classes, metadata bytes,
  transaction count, and compacted bytes where storage growth is meaningful;
- include overwrite loops for hot mutable records;
- include random-new-item loops for tickets, lanes, pages, and documents;
- include concurrent reader and writer scenarios;
- include durability-mode scenarios for `strict`, `normal`, `relaxed`, and `ephemeral`;
- include VCS promotion scenarios that verify immutable snapshots are explicit and do not collide with
  hot mutable paths;
- emit machine-readable JSON artifacts and a human-readable summary;
- run outside `just ci` unless a bounded unit-sized subset is explicitly split into the default gate.

The suite is an engineering instrument, not a marketing benchmark. Its purpose is to detect
amplification regressions early and explain where bytes and time go before broad facet migration
continues.

## 19. Review And Acceptance Gates

Large storage-engine work must pass explicit design and code-review gates. A queue row is not complete
merely because the code compiles or a probe improves. The implementation must preserve the semantics
of the facet being optimized.

Before implementation starts for each major substrate area, the queue must include a source-backed
design review gate for:

- durability policy and `normal` group-commit behavior;
- MVCC snapshots, pinned readers, and reclamation blockers;
- shared multi-facet transaction semantics and error behavior;
- VCS/CAS promotion boundaries;
- controlled migration and replacement of development stores.

After implementation, review must verify semantic preservation. Performance changes MUST NOT remove,
skip, or weaken:

- operation logs;
- audit records;
- revision indexes;
- reference indexes;
- compare-token checks;
- idempotency behavior;
- ACL/PEP enforcement;
- generated, ABI, binding, hosted, local-client, remote-client, CLI, and MCP surfaces when the
  observable contract changes.

The final closure gate for this spec must prove both performance and correctness: the store is smaller
and faster for hot mutable workloads, and every retained public behavior remains source-backed.

### 19.1 Semantic-preservation checklist

Use this checklist during code review of any 0071 performance change (transaction batching, write
coalescing, page or record layout changes, mutation-boundary reduction). It is concrete on purpose so
an owner-agent can run it against a diff. The rule for the whole checklist: a performance change may
write the same logical records in fewer transactions, but it must never write fewer logical records.

Motivating regression (MX-420). A transaction-amplification fix in `crates/loom-pages/src/lib.rs`
reduced writes by dropping Page facet side effects instead of only batching them: `update_page` and
`publish_page` stopped emitting their `page.updated` / `page.published` operation records, stopped
calling `update_page_operation_revision_index`, and stopped updating published reference indexes,
while still saving the workspace snapshot. The store looked smaller and the probe improved, but the
audit and revision surfaces silently regressed. The accepted fix batched those writes into fewer
transactions while preserving every record, and added the focused regression
`page_update_and_publish_keep_operation_revision_and_reference_side_effects`. Every item below exists
because that class of regression is easy to introduce and hard to see in a probe.

For each write path the change touches, verify against `git show HEAD:<file>` (diff the old behavior,
do not just read the new code):

1. Operation logs. The same operation records are still appended. For every logical mutation, the
   `append_operation` / operation-record write that HEAD performed still runs. Count operation records
   per logical op before and after; the count must not drop. Anchor: MX-420 `update_page` /
   `publish_page` operation records.
2. Audit records. Audited write variants are preserved. Calls such as `control_set_audited*` and any
   audit-append still fire; a batching change must not silently switch an audited write to a
   non-audited one.
3. Revision indexes. Every `update_*_revision_index` call HEAD made for a mutated entity still runs
   (for example the page operation revision index). Do not drop a revision-index update because it is
   an extra write.
4. Reference indexes. Reference-index updates are preserved (for example page published references,
   substrate reference roots). A coalesced transaction must still leave the reference index consistent
   with the committed state.
5. Compare tokens and optimistic concurrency. Expected-root / compare-token checks still gate the
   write. Batching must not move a mutation past its concurrency check or reuse a stale token across
   the coalesced boundary.
6. Idempotency. Replay and idempotent behavior is unchanged: applying the same operation twice yields
   the same result or the same conflict record as HEAD, not a divergent state from the new batching.
7. ACL/PEP enforcement. The `authorize_*` check still precedes the batched write on the same
   namespace/domain/right. Batching must not move a write outside its authorization boundary or share
   one authorization across writes that HEAD authorized separately.
8. Public and generated surfaces. Hosted, local-client, remote-client, CLI, MCP, generated IDL/ABI,
   and language-binding outputs are shape-identical to HEAD unless the change is an explicit, recorded
   contract change. Diff a representative response; a performance change is not license to alter an
   observable contract.
9. Regression coverage. A batching or coalescing change that touches side-effectful writes must add a
   focused regression asserting the side effects still occur after the change (operation-log entry
   present, revision-index row present, reference-index updated), following the MX-420 example.
10. Probe honesty. A smaller store or improved probe is necessary but not sufficient. Confirm the size
    win comes from fewer transactions or reclaimed pages, not from fewer retained records; pair any
    probe number with the checks above before accepting.
11. Request and restart boundaries. Run at least two mutations of the same entity through separate
    daemon requests, stop and restart the daemon between passes, and verify that current state,
    retained operations, and the revision index agree after reopen.

If the checklist surfaces a semantic surface that no current review step protects, record it as a
follow-up ticket rather than expanding this prose.

## 20. Migration Requirements

The final architecture must include controlled migration for existing development stores.

Migration must:

- preserve data reachable through current public surfaces;
- preserve or explicitly convert retained audit and operation history;
- move hot current heads into the mutable substrate;
- remove obsolete temporary migration tools from committed source after development migration;
- produce a freshness watermark before replacement;
- preflight replacement against active-store freshness;
- verify VCS commit no longer sees namespace collisions from legacy facet projections;
- report backup and cleanup artifacts.

Because Loom is not released, source code should converge to one current schema rather than carrying
permanent legacy upgraders unless the owner explicitly preserves a compatibility contract.

### 20.1 Controlled development-store replacement contract

Development-store migration is a controlled copy, preflight, replacement, and rollback workflow. It is
not an automatic open-time upgrader and it is not a permanent legacy compatibility surface. The
operator must produce a candidate store, verify it against the still-active store, replace the active
store only at a quiet boundary, and retain rollback artifacts until the replacement has been accepted.

The source-backed CLI surface is:

```text
loom store copy <src> <dst> [--with compacted] [--with fips] --format json --report-file <report>
loom store preflight-replacement <candidate> <workspace> --live-store <active> --candidate-report <report> --format json
```

If the candidate is intentionally older than the live store, the replacement preflight is allowed only
with both:

```text
--force-owner-approval <approval-text> --backup-store <backup>
```

The backup must exist and preserve the newer live state that the replacement would discard. Owner
approval text is evidence, not a substitute for the backup.

The copy report is the freshness contract. Its `freshness_watermark` records:

- `created_at_ms`;
- `source_reference_root`;
- `source_control_root`;
- one entry per workspace containing `workspace_id`, `workspace_name`, and
  `latest_ticket_operation`.

Replacement preflight compares the candidate report with the active store. It must block replacement
when:

- the report destination does not match the candidate path;
- the report source does not match `--live-store`;
- the active store reference root or control root advanced after the candidate watermark;
- a workspace no longer resolves;
- ticket operation history contains a newer sequence than the candidate watermark;
- the candidate cannot open, list workspaces, list lanes, list tickets, or read maintenance state;
- VCS namespace preflight detects a legacy document/file projection collision in the workspace that
  would make promotion or commit fail.

Clean replacement requires:

1. Stop writers or otherwise establish a quiet boundary for the active store.
2. Create the candidate with `loom store copy`; use `--with compacted` when the goal is cleanup, and
   `--with fips` only for an explicit identity-profile migration.
3. Save the JSON copy report as the migration artifact.
4. Run replacement preflight against the live store and candidate report.
5. If preflight reports stale live mutations, recopy from the active store. Force is reserved for an
   explicit owner decision plus a backup of the advanced live store.
6. Replace the active store path only after preflight returns `ok`.
7. Keep the old active store, the candidate report, the preflight report, and any force backup until
   the owner accepts the replacement.
8. Remove one-off migration tooling from committed source after development stores have moved.

Rollback expectation: if replacement validation fails after activation, stop writers, restore the old
active store path or the force backup, and re-run replacement preflight before attempting another
activation. The replacement tool must not delete rollback artifacts.

No lingering temporary tools: development-only repair scripts, ad hoc Matrix-store mutation helpers,
and one-off projection rewrites must not remain in source after the migration window. Durable
operator-facing behavior belongs in `loom store copy` and `loom store preflight-replacement`, or in a
later owner-approved public contract.

## 21. Implementation State

| Area | State |
| --- | --- |
| Mutable overlay current-record path | Implemented for the migrated operational owners; broader facet adoption remains tracked separately. |
| Bounded overwrite growth for loop probe | Implemented and accepted for the current probe. |
| Page-class attribution | Implemented and accepted as the diagnostic base. |
| Random new-item write amplification | New records are packed within transaction batches and shared pages are reclaimed only when all live slots are superseded. The measured second eight-bundle batch grew by 483,328 bytes instead of 1,720,320 bytes and remained within the section 17.2 bounds. Crash-safe reuse of partially occupied slabs across transaction boundaries remains open. |
| Durability policy type contract | Implemented and accepted for `strict`, `normal`, `relaxed`, and `ephemeral` names and validation semantics. |
| Configurable durability behavior | Implemented with store and facet policy resolution plus CLI and MCP settings surfaces. |
| WAL or group commit durability policy | Implemented for normal-durability workflow transactions with recovery coverage and diagnostics. |
| MVCC read snapshots across all surfaces | Implemented for the migrated document, ticket, lane, and page readers; broader facet adoption remains tracked separately. |
| Multi-facet transaction API | The local engine and FileStore adapter implement owner state, secondary indexes, compare tokens, and idempotency. Document current records, declared indexes, references, and engine state co-publish. MCP composite operations validate in an isolated planning engine and publish one durable single-workspace transaction; rejected plans leave durable state unchanged. |
| VCS promotion bridge | Implemented for selected overlay checkpoints, namespace-safe projection, and strict promotion consumers. |
| Append-addressed retained history | Physical retained-history records, bounded heads, page operation append, revision append, legacy conversion, fragmented immutable record chains, GC, and tail compaction support are implemented. |
| Bounded revision append reads | Implemented for page and ticket writers with latest-revision and checkpoint point records plus one-time incomplete-manifest backfill. |
| Steady-state physical slack | Implemented. Owner-state publication uses overlay point updates, active-segment GC sweeps in place, free-run ages remain stable across adjacency coalescing, and reclaimable/free diagnostic overlap is counted once. Two consecutive 30-second daemon probes reopened successfully; the second retained seven additional ticket/page histories while physical size grew from 2,166,784 to 2,203,648 bytes and reusable interior space fell from 524,288 to 266,240 bytes. |
| Full migration to final architecture | Incomplete. |
| Other facet adoption | Design target only. |
| Manual performance suite | Implemented with JSON and summary artifacts, compacted-copy measurement, durability scenarios, and concurrent reader/writer coverage. |
| Review and acceptance gates | Implemented for durability, MVCC, transactions, promotion, migration, and semantic preservation; final queue closure review remains open. |

## 22. Closure Criteria

This spec is complete only when:

- hot mutable current state is the default path for operational facets;
- durability policy is configurable and source-backed;
- strict promotion boundaries exist for VCS, audit, ledger, sync, and export;
- random-new-item growth is bounded by source-backed page attribution;
- CLI, MCP, hosted, local client, remote client, and bindings use shared persistence semantics;
- migration guidance for development stores is source-backed and temporary tools are removed;
- `just test-performance` or an equivalent documented manual command covers storage growth,
  concurrency, latency, durability modes, and VCS promotion;
- design review gates and final closure review prove semantic preservation across affected facets and
  public surfaces;
- tests cover overwrite loops, random new-item loops, durability policy behavior, crash recovery,
  transaction batching, and VCS promotion.

Decision Points: none.
