# Driveish - Shared File Organization

**Status:** Target design. **Version:** 0.1.0-target.
**Capability:** `drive`.

This document defines a Dropbox or Google Drive style shared file organization on top of Loom. It is an
Studio application profile, not a replacement for the core filesystem facade, synchronization, access
control, or end-to-end encrypted sync specs.

Driveish exists because a shared Studio folder is not just a Loom file repository. A repository exposes
history, branches, commits, and explicit merges. A drive product exposes files and folders that many
people and agents edit concurrently, with background sync, conflict copies, upload progress, folder
sharing, previews, comments, retention, and user-facing recovery.

## 1. Contract Boundaries

The design builds on these contracts:

- `0003-core-interface.md` defines the target filesystem facade over workspace working trees.
- `P9-0003-files-binding.md` records the files binding and notes that working-tree writes serialize
  through the single-writer engine.
- `0006-synchronization.md` defines current sync as movement of immutable content-addressed objects plus
  mutable workspace refs. Current branch sync is fast-forward only.
- `0031-end-to-end-encrypted-sync.md` defines the blind-replica topology where a local client holds keys
  and the remote stores ciphertext by opaque labels.
- `SLACKISH.md` defines the Studio pattern used here: local-first replicas, blind cloud by default,
  keyed workers where content-aware compute is required, MCP resources, and durable callbacks.
- `0061.md` defines the shared operation substrate: envelope, sequencer, durable cursors, order
  tokens, conflict records, annotations, entity versioning, projections/views, and cross-facet
  search. Where this document's local envelope or sequencer text differs, 0061 supersedes it
  (see §20).
- `SURFACES.md` defines the human experience layer (MCP Apps, elicitation flows, visualizations)
  rendered over this profile's projections.

This document depends on those boundaries. It does not make raw current branch sync sufficient for
multi-device concurrent editing of a shared folder.

## 2. Product Model

A Driveish organization exposes a tree of folders and files:

```text
drive
  roots
  folders
  files
  versions
  shares
  comments
  previews
  locks
  cursors
  conflicts
  trash
  audit
  retention
```

Users and agents interact with the drive as a normal folder. They should not need to understand branch
tips, commit parents, bundle imports, or object graphs during ordinary use.

The product contract is:

- edits appear in a shared folder after sync;
- file history is recoverable;
- concurrent edits do not silently overwrite each other;
- conflicts are visible and resolvable;
- large files upload and download incrementally;
- folder sharing and access revocation are enforceable;
- a blind cloud can store and relay encrypted content without reading it;
- keyed workers are required for search, previews, virus scanning, DLP, and server-side document merge.

## 3. Cloud and Encryption Model

The same cloud topologies from Slackish apply.

```text
Private drive:
  Local Loom replicas hold keys and full folder state.
  Loom Cloud is blind storage, sync, notification, and opaque lease coordination.
  Content-aware search and previews happen locally or in a tenant-controlled keyed worker.

Managed enterprise drive:
  A keyed service runs inside the tenant trust boundary.
  It can index, preview, scan, classify, and perform server-side merge helpers.

Hybrid drive:
  Blind Loom Cloud stores canonical encrypted file history.
  Selected folders or files are mirrored to keyed compute replicas for approved workloads.
```

A blind remote can sequence file operations and coordinate opaque leases, but it cannot inspect file
contents, compute diffs, render previews, scan malware, or classify documents.

## 4. Storage Layer

Driveish uses Loom content-addressed objects for file bytes and Merkle-backed indexes for folder state.

```text
drive root
  folder tree root
  file metadata index
  version index
  share index
  lock index
  conflict index
  trash index
  audit log root
```

File bytes are stored as content-addressed blobs or chunk lists. Large files must be chunked so a small
range change does not rewrite the whole file. Folder and metadata state must be stored as structured
indexes, not one giant directory blob.

Current source backs the reusable Drive service boundary in `loom-drive`, built on the Drive model
layer in `loom-substrate::drive`: canonical folder indexes, verbatim names with NFC plus case-fold
collision keys, chunk manifests carrying the pinned 512 KiB minimum, 1 MiB average, and 4 MiB maximum
policy, file version indexes, profile snapshots, share indexes, and retention live-root pin indexes.
`loom-core::chunk` uses the same pinned thresholds for file and stream chunking, and stores content
at or below the minimum chunk size as a single blob. `loom-hosted` re-exports the reusable service
boundary for hosted REST and JSON-RPC adapters. The `loom` CLI source-backs the reusable Drive
service boundary for folder/file reads, version/conflict/share/retention reads, upload sessions,
folder create, rename, move, delete, share and retention admin writes, and conflict resolution:
`loom drive list`, `stat`, `read`, `list-versions`, `list-conflicts`, `list-shares`,
`grant-share`, `revoke-share`, `apply-share-expiry`, `list-retention`, `pin-retention`,
`unpin-retention`, `apply-retention`, `create-folder`, `create-upload`, `upload-chunk`,
`commit-upload`, `rename`, `move`, `delete`, and `resolve-conflict`. Attached-daemon lease commands
and OS hydration/dehydration worker controls remain MCP/host-owned rather than generic CLI parity.
`loom-mcp` and `loom-hosted` source-back the first read-only Drive tool and adapter slice over stored
`DriveProfileSnapshot` records:
`drive_list`, `drive_stat`, `drive_read`, and `drive_list_versions`. These surfaces decode the
profile snapshot at
`profile/drive/v1/{workspace_id}/snapshot`, gate access through the current Studio-profile VCS read
permission, and read file bytes through the CAS facade so content digests are verified on every read.
`loom-mcp`, `loom-hosted` REST, and `loom-hosted` JSON-RPC also source-back the first write
vertical: `drive_create_folder`, `drive_create_upload`, `drive_upload_chunk`,
`drive_commit_upload`, `drive_resolve_conflict`, `drive_rename`, `drive_move`, `drive_delete`,
and `drive_list_conflicts`.
These operations require `expected_root` for ordinary writes, persist upload sessions under
`profile/drive/v1/{workspace_id}/uploads/{upload_id}`, append operation records under
`profile/drive/v1/{workspace_id}/operations`, and persist conflict records under
`profile/drive/v1/{workspace_id}/conflicts`. The current write slice materializes duplicate-name upload
collisions and stale-base new-file upload collisions as visible conflict-copy entries, exposes
conflicts through `drive_list_conflicts`, resolves them through `drive_resolve_conflict`, and
deletes completed upload-session control records after successful commit.
MCP source-backs product-native Drive lease lifecycle tools, `drive_acquire_lease`,
`drive_refresh_lease`, `drive_release_lease`, and `drive_break_lease`, when the MCP host is attached
to the shared 0036 daemon session. MCP Drive write tools also accept the reusable `write_admission`
envelope with a Drive target and fence; attached hosts validate the live daemon token and apply the
fence before the write. Generic substrate write-admission policy storage is source-backed through
`substrate_write_admission_policy_get` and `substrate_write_admission_policy_set`; Drive is the first
consumer, and MCP Drive writes reject missing admission when the Drive surface scope is marked
mandatory or when target-specific mandatory rows exist that the admission-free call cannot prove it
avoids. Stale deletes whose `expected_root` no longer matches the current Drive root are held as
open conflict records instead of silently deleting the current winner. Source-backed MCP, REST, and
JSON-RPC file deletes leave the edited file visible, and `drive_resolve_conflict` with
`keep_conflict` applies the held delete. MCP folder deletes create one held-delete conflict for each
surviving descendant and leave the ancestor chain plus edited descendants visible; once every
survivor conflict for the same folder delete resolves as `keep_conflict`, MCP, REST, and JSON-RPC
prune the deleted folder root. Durable hosted listener admission for `drive/rest` and
`drive/json_rpc` is source-backed through `loom serve`, and daemon-opened Drive REST and JSON-RPC
listeners route the current hosted read/write vertical. Drive lease lifecycle operations are MCP
attached-daemon tools, not REST/JSON-RPC listener routes. Drive lease refresh and release paths
append `lock.expired` operation-log records when the attached daemon reports `LOCK_LEASE_EXPIRED`. MCP
source-backs admin-gated share and retention metadata tools: `drive_list_shares`,
`drive_grant_share`, `drive_revoke_share`, `drive_apply_share_expiry`, `drive_list_retention`,
`drive_pin_retention`, `drive_unpin_retention`, and `drive_apply_retention`. Hosted REST and
JSON-RPC source-back equivalent share and retention metadata adapter methods and daemon-opened
routes. Share grants project into scoped ACL grants for the Drive target; manual share-expiry
application removes due grants, revokes projected rights, and publishes `share.expired`. Manual
retention application removes expired non-legal pins and publishes `retention.applied`. Current
source backs a durable Drive policy registry at `profile/drive/v1/registry`; `loom serve drive`
registers a policy target there, and the daemon uses the registry to run scheduled share-expiry and
retention application as an audited daemon service principal in authenticated mode. `loom-hosted`
also source-backs local OS projection primitives: `dehydrate_file_for_os` materializes canonical
marker bytes from the latest verified file version, `hydrate_file_for_os` converts marker bytes back
to verified CAS content, and `write_file_from_os` routes ordinary local writes through the same
upload-session commit path while rejecting marker bytes as content. Hosted REST and JSON-RPC
source-back equivalent daemon-opened routes for dehydrate, hydrate, worker-plan, and OS-write
operations with byte payloads exposed as hex.
`loom-mcp` source-backs built-in Drive MCP Apps for Drive Browser, Drive Preview, Drive Sharing,
Drive Conflicts, and Drive Retention. These apps render from the `loom.drive` template binding,
receive source-backed folder, file-byte, version, conflict, share, retention, and lease-tool data,
and support folder deep links at `ui://{workspace}/mcp/apps/drive-browser/folder/{folder_id}` plus
file-preview deep links at `ui://{workspace}/mcp/apps/drive-preview/file/{file_id}`. The app bridge
prepares promoted Drive tool calls such as `drive_create_folder` and `drive_create_upload` through
`apps_call_tool`. OS placeholder and hydration controls remain target app work until MCP exposes
app-callable hydrate, dehydrate, or worker-plan tools.
MCP, hosted REST, and hosted JSON-RPC `drive_commit_upload` maintain shared substrate revision rows
for `drive:file:{file_id}` file-content revisions through the generic profile transaction helper,
making committed Drive content visible through `substrate_history`. Folder metadata revisions remain
target work for the generic profile transaction rollout rather than a Drive-only revision path.

Required indexes:

- path to node id;
- node id to metadata;
- file id to version chain;
- folder id to children;
- principal id to shared resources;
- content digest to file versions;
- upload session id to staged chunks;
- conflict id to candidate versions;
- trash entry id to deleted node.

The user-facing path is mutable metadata. The stable identity of a file is its `file_id`, not its path.
Rename and move operations change folder entries without changing file identity.

### 4.1 Folder Index and Canonical Path Normalization

Owner decision 2026-07-04 (resolves §17.3): **verbatim names with a folded collision key.**

- The authoritative structure is the per-folder children map: `folder_id → { fold_key → entry }`,
  entry = `(name_verbatim, node_id, kind)`. The full-path → node_id index is a rebuildable
  projection (0061 §8) over the children maps; a tree walk is the correctness fallback.
- `fold_key` = NFC + Unicode simple case fold of `name_verbatim`. Folding is a **collision policy
  only**: identity, display, and payload digests use the verbatim bytes, consistent with the 0061
  §9.1 verbatim-storage / fold-at-read Unicode policy. Imports round-trip names byte-exact.
- Sibling uniqueness is enforced over `fold_key`: concurrent creations whose names fold equal are
  the same-name case in the §7.4 matrix (deduplicate on equal content digest, conflict copy
  otherwise). `café` (NFC) and `café` (NFD) are one name, not two invisible siblings.
- Canonical path = `/`-joined verbatim names from the drive root. `/` and NUL are forbidden in
  names. Names that cannot materialize onto a target OS filesystem (Windows-reserved names,
  trailing dots/spaces) are legal drive names; the materialization view escapes them and records
  the mapping, a fidelity concern rather than a model restriction.
- Folder listings render sorted by `(fold_key, verbatim bytes)`. Folders do not use 0061 §5 order
  tokens: name order is the drive product convention.
- Paths remain 0061 §4 aliases: move and rename rewrite directory entries, never `file_id`.

### 4.2 Chunking Policy

Owner decision 2026-07-04 (resolves §17.4): **content-defined chunking with pinned parameters.**

- FastCDC-class content-defined chunking with pinned parameters: min 512 KiB, target average
  1 MiB, max 4 MiB. Mask/normalization constants are pinned in conformance vectors; the
  parameters are identity-affecting, since independent clients must chunk identical plaintext
  identically.
- Files at or below the minimum chunk size are stored as a single blob; the version record
  references the blob digest directly, with no manifest.
- Larger files publish a content-addressed **manifest**: the ordered list of
  `(chunk_digest, size)`. The version record carries both the manifest digest and the whole-file
  content digest (integrity check and dedup key).
- Chunking runs on the key-holder over plaintext; a blind cloud sees opaque chunk labels only.
  Dedup operates on plaintext digests within a key domain; convergent encryption is explicitly
  not required.
- `files.patch_range` re-chunks from the first affected chunk boundary until CDC boundaries
  resynchronize; unaffected chunks are reused by digest, so a small range edit publishes a new
  manifest referencing mostly existing chunks. Editing one range never rewrites unrelated files
  (§16).
- Upload sessions (§8) stage chunks in CAS by digest; commit atomically publishes the manifest as
  `file.upload_committed`. Per-media-type chunkers, if ever needed, arrive as new manifest
  versions without changing this contract.

## 5. Operation Log

The source of truth for shared-folder coordination is an operation log, not a raw branch push.

Operation kinds:

```text
file.created
file.content_replaced
file.chunk_uploaded
file.upload_committed
file.renamed
file.moved
file.deleted
file.restored
folder.created
folder.renamed
folder.moved
folder.deleted
share.granted
share.revoked
share.expired
lock.acquired
lock.released
conflict.created
conflict.resolved
comment.added
preview.derived
retention.applied
```

Each operation carries the **0061 §2 canonical envelope**. The workspace is the Drive profile scope.
`target_path` is a profile payload field, not an envelope field. Agent-authored operations carry the
0061 §2 agent identity block.

The visible folder tree is a projection over operations. The projection may materialize to ordinary Loom
filesystem paths for compatibility, but the operation log is the collaboration contract.

## 6. Multi-Replica Coordination

Five laptops, each with a local Loom and an AI assistant, cannot safely coordinate shared folder edits by
independently editing the same workspace branch and relying on ordinary current sync. Current sync moves
objects and fast-forwards refs. It rejects divergent branch tips instead of choosing or merging a winner.

Driveish therefore needs explicit coordination above raw branch sync.

### 6.1 Blind Central Operation Sequencer

The recommended enterprise default is a central operation sequencer that can be blind to file
plaintext. This is the **0061 §3 sequencer contract** instantiated for the drive scope. The flow
below is illustrative, not a second protocol; blind, keyed-worker, and per-actor-log topologies
are deployment modes of that one contract (supersession recorded 2026-07-04).

Flow:

1. A local client writes file chunks and operation payloads into its local Loom.
2. The local Loom submits opaque object labels, an idempotency key, a base folder root, and an operation
   envelope to Loom Cloud.
3. Loom Cloud verifies authorization by label, assigns the next drive sequence, persists the opaque
   operation, updates the drive root with compare-and-swap, and emits wakeups.
4. Other local Looms pull the operation and objects, decrypt locally, verify digests, update their folder
   projection, and advance cursors.

This is a central mediator for ordering and delivery, but not a plaintext file server. It can safely
sequence opaque encrypted operations as long as it can authorize principals and validate protocol shape.

Properties:

- total order for drive operations;
- simple user experience;
- durable replay by sequence;
- zero-knowledge compatible;
- no hosted content search, preview, DLP, or document merge unless a keyed worker is added.

### 6.2 Per-Actor Operation Logs

An alternative is to give each device or assistant its own operation log:

```text
drive/actors/{actor_id}/operations
```

Clients pull all authorized actor logs and deterministically derive a folder view. The merge function
must define:

- operation identity and idempotency;
- causal parents or vector-clock metadata;
- deterministic rename and move conflict rules;
- concurrent delete versus edit rules;
- directory child ordering and case-normalization behavior;
- conflict-copy naming;
- trash and restore semantics;
- compaction rules that preserve version history and audit proofs.

Actor logs are appropriate for decentralized or offline-heavy drives. They are harder for users because
folder order and conflict state can be provisional until logs converge.

### 6.3 Keyed Central Drive Server

A keyed central drive server is operationally closest to Google Drive. It reads content, validates file
types, performs server-side document merge, indexes search, renders previews, scans malware, runs DLP,
and broadcasts updates.

This is compatible with Loom, but it is not zero-knowledge. It is appropriate when the tenant chooses
hosted compute inside a trust boundary.

### 6.4 Decision

Driveish should support the blind central operation sequencer as the default shared-folder coordination
model. Actor logs remain a target topology for decentralized or offline-heavy deployments. A keyed
central drive server is a deployment option, not a requirement.

Raw current branch sync alone is not a sufficient coordination protocol for multiple local assistants
editing the same shared folder.

## 7. Concurrent Editing Semantics

Driveish must handle concurrent writes by file type and capability, not with one universal merge rule.

### 7.1 Binary Files

For ordinary binary files, concurrent replace creates a conflict copy.

Example:

```text
Laptop A edits Budget.xlsx from version v10 and commits v11.
Laptop B edits Budget.xlsx from version v10 and commits v12.
The drive projection keeps one version as the visible file and creates a conflict entry for the other.
```

Conflict names are stable, user-facing, and deterministic per §7.5:

```text
Budget (conflicted copy of Dana, 2026-06-23).xlsx
```

The conflict record stores both candidate versions, their base version, authors, timestamps, and
resolution state.

### 7.2 Text Files

For plain text files, the system may offer a three-way merge helper when both edits share a base version.
Automatic merge is allowed only when the merge is deterministic and conflict-free. Otherwise it creates a
conflict copy or a merge-conflict document.

### 7.3 Structured Documents

Documents with known collaborative formats may use operation-based or CRDT-style editing if a promoted
document facet defines canonical operations. Without a promoted document operation model, Driveish treats
the file as binary and uses conflict copies.

### 7.4 Folders and Metadata: Pinned Merge Matrix

Owner decision 2026-07-04 (resolves §17.6): the matrix below is pinned, using the JIRAISH §7.2
conflict classes (merge disjoint, sequence-and-record, conflict copy, reject) over the 0061 §6
conflict record. **Edit wins over delete.** Every row is a conformance vector. "Concurrent" means
two operations sharing a `base_folder_root` (or base entity version); unconditioned writes (no
base) are last-write-wins by definition. This is the import escape, as in JIRAISH.

| Concurrent pair (same base) | Outcome | Class |
| --- | --- | --- |
| create different names, same folder | both apply | merge |
| create fold-equal names (§4.1), same content digest | deduplicate to one entry | merge |
| create fold-equal names, different content | first-sequenced keeps the name; other becomes a conflict copy (§7.5) | conflict copy |
| rename same node to different names | first-sequenced name is visible; conflict record with `field_or_region = name` | sequence-and-record |
| move + content edit on same file | both apply (disjoint) | merge |
| move same node to different folders | first-sequenced wins; conflict record | sequence-and-record |
| delete file + concurrent content edit | **edit wins**: delete is sequenced but held; the file stays visible with an open conflict record; resolution may re-apply the delete | conflict record (open) |
| delete folder + concurrent create/edit of a descendant | delete held for the surviving subtree: nodes that received concurrent operations survive with their ancestor chain resurrected; one conflict record per survivor | conflict record (open) |
| folder move creating a cycle (A→B while B→A) | second-sequenced move rejected, rejection rule `path_cycle` | reject |

`path_cycle` extends the JIRAISH §7.3 rejection-rule enum per the shared-enum rule; no parallel
enum. Held deletes resolve like any 0061 §6 record: an explicit resolution operation either
confirms the delete or dismisses it, and a later uncontested delete supersedes the record.

### 7.5 Conflict-Copy Naming

Owner decision 2026-07-04 (resolves §17.5): the conflict-copy name is a **pure deterministic
function of the 0061 §6 conflict record**. Every replica derives the same name from the total
order with no coordination.

- Winner/loser: the operation sequenced first from the shared base advanced the head and wins the
  visible entry; the later stale-base operation is the loser and materializes as the conflict
  copy.
- Name: `{stem} (conflicted copy of {actor_display}, {YYYY-MM-DD}){ext}`. Date is the UTC date
  of the losing operation's `timestamp_ms`; `actor_display` is the loser's principal display
  alias *at sequence time*, captured in the conflict record so later principal renames do not
  change historical names.
- Collision: append ` - {n}` before the extension, smallest free `n ≥ 2` under the §4.1 fold-key
  rule, evaluated against the folder projection at the losing operation's sequence. Deterministic
  because the projection is a deterministic function of the total order.
- The conflict copy is **derived projection state**, not a synthesized operation (same rule as
  JIRAISH rejection records): it appears in the folder projection as a read-only entry carrying
  `conflict_id` until resolved. `drive_resolve_conflict` with `keep-both` materializes it as a
  real file (`file.created`, new `file_id`, `conflict_id` provenance, named by this function);
  `keep-current` discards it (content remains in version history); `keep-conflict` makes the
  losing version the head via `files.restore_revision` semantics.

The §7.1 example name (`conflict from Dana's MacBook`) is superseded by this format; device names
are not principal identity.

Current source backs the deterministic conflict-copy naming function and the model-level §7.4 merge
matrix evaluator in `loom-substrate::drive`: create collisions, equal-content deduplication,
different-content conflict copies, rename conflicts, move conflicts, delete/edit conflicts,
descendant edits under folder deletes, and `path_cycle` rejection. MCP, REST, and JSON-RPC
source-back stale-base new-file upload conflict materialization, `drive_list_conflicts`, and
`drive_resolve_conflict` with `keep_current`, `keep_conflict`, and `keep_both` policy. MCP
source-backs Drive lease acquire, refresh, release, and break over the shared 0036 daemon lock token
shape.
Generic substrate write-admission policy storage can mark Drive scopes mandatory, and MCP Drive
writes reject missing admission for mandatory Drive scopes or target-specific mandatory rows.
Stale file deletes are source-backed as held-delete conflict records across MCP, REST, and JSON-RPC;
the current file remains visible until `keep_conflict` applies the delete. MCP stale folder deletes
source-back descendant survivor conflict records and leave the ancestor chain visible. Resolving every
survivor conflict from the same folder delete as `keep_conflict` prunes the deleted folder root across
MCP, REST, and JSON-RPC. Durable hosted listener admission for `drive/rest` and `drive/json_rpc` is
source-backed, and daemon-opened Drive REST and JSON-RPC listeners route the current hosted read/write
vertical. Lease lifecycle operations remain MCP attached-daemon tools. Refresh and release calls that
observe expired daemon locks append `lock.expired` records. Sharing and retention management are
source-backed for MCP, hosted REST, hosted JSON-RPC, and daemon-scheduled policy application for
registered Drive policy targets. `loom-hosted` source-backs
the local OS projection primitives for dehydrated marker rendering, hydrate-on-read, and marker-byte
write rejection. OS-native placeholder hooks and background hydration/eviction workers remain target
work.

### 7.6 Editor Leases

Owner decision 2026-07-04 (resolves §17.7): **one lease model, advisory by default, per-scope
policy escalation to mandatory.** Leases are profile policy over 0036 fenced leased locks
(control-plane state outside the versioned graph), per 0061 §17.

- `leases.acquire(file_id | folder_id)` takes a TTL-bounded fenced lease as the resolved
  principal. Lease state is surfaced in projections ("Dana is editing") and MCP resources.
- **Advisory mode (default):** the lease is an intent signal only. All writes still sequence;
  the §7 conflict machinery remains the safety net.
- **Mandatory mode (scope policy):** the sequencer rejects content-write operations on the leased
  node from non-holders with rejection rule `lease_held` (extends the JIRAISH §7.3 enum).
  Metadata reads, comments, and shares are unaffected. Offline edits made while another actor
  held a mandatory lease surface as conflict copies (§7.5) on reconnect, never silent loss.
- In both modes: leases auto-expire at TTL; owner/admin break is an audited operation; an expired
  or broken lease never blocks writes, and a stale fencing token cannot write (0036 semantics).
  Source-backed lease acquire, refresh, release, break, and observed expiration operations appear in
  the drive log as `lock.acquired`, `lock.refreshed`, `lock.released`, `lock.broken`, and
  `lock.expired`. Background deadline sweeps remain target work.
- Mode changes are scope-policy operations, audited like §7.1 operating-mode changes (0061 §7.1).

Current source backs the product-native MCP lease lifecycle for attached hosts:
`drive_acquire_lease`, `drive_refresh_lease`, `drive_release_lease`, and `drive_break_lease`
validate the Drive target, require an attached daemon session, and use the shared 0036 lock
coordinator request shape with deterministic keys. Acquire, refresh, and release authorize VCS write
access; break authorizes VCS admin access and removes all current holders for the Drive target without
resetting fence counters.
`drive/{workspace}/{file|folder}/{target_id}`. Attached MCP write tools accept a shared
`write_admission` envelope `{ target_kind, target_id, fence }`; when present, the host validates the
same deterministic Drive lock key through the 0036 daemon's live-token fence-admission path before
mutating Drive state. The source-backed lifecycle is advisory by default today, but generic substrate
write-admission policies can mark a Drive surface scope mandatory, and MCP Drive writes then reject
missing admission before mutation. Target-specific mandatory rows also fail closed when admission is
omitted, because the host cannot prove the unfenced write avoids those rows. The attached MCP lease
lifecycle appends Drive operation-log records for `lock.acquired`, `lock.refreshed`,
`lock.released`, `lock.broken`, and `lock.expired` when refresh or release observes an expired token.
Background expiration sweeps and OS-native hydration/eviction workers remain target work.

## 8. Upload, Download, and Sync

Large files require resumable upload sessions.

Upload flow:

1. `drive_create_upload` allocates an upload session for a target path or file id.
2. The client uploads chunks into Loom CAS.
3. The client commits an upload manifest listing chunk digests and sizes.
4. The operation sequencer accepts `file.upload_committed`.
5. Other replicas fetch only missing chunks.

Current source backs steps 1 through 3 for MCP, REST, and JSON-RPC Drive tools and records step 4 as
a durable Drive operation-log record. Stale-base new-file commits materialize duplicate-name
conflict copies; stale-base replacement commits fail closed. MCP deletes completed upload-session
control records after a successful commit, so a completed upload id cannot be committed again.
Multi-replica transfer, partial hydration worker integration, and mandatory lease-aware upload
admission remain target work.

Downloads use range reads and content-addressed chunk fetch. Sync transfers only missing objects and
metadata roots. Interrupted sessions resume from known chunk digests and drive sequence cursors.

Partial sync is required:

- selected folders;
- recent files;
- dehydrated file markers for offline-disabled content (§8.1);
- on-demand chunk hydration;
- background eviction of local content that remains safely stored in the remote.

### 8.1 Dehydrated-File Markers

Owner decision 2026-07-04 (resolves §17.8): **a pinned canonical-CBOR marker format.**

- A dehydrated file keeps its full metadata row locally (file id, size, digests, manifest ref);
  what changes is the materialized OS-filesystem view, which renders a small marker file instead of
  content: a pinned magic header plus canonical CBOR `(format_version, file_id, size,
  content_digest, loom:// file URI)`. Marker bytes are conformance-vectored.
- Hydration state is **per-replica local kv, never a sequenced operation**. Dehydrating a file
  on one laptop is not a drive event. The eviction worker (§11) flips content to a marker after
  verifying remote replication; opening a marker (or an explicit hydrate call) triggers the
  hydration worker.
- Guard against marker reflux: sync and upload paths detect the magic header and the size/digest
  mismatch with the metadata row, and must never commit marker bytes as file content. A backup tool
  that copies markers elsewhere copies markers, not data by design.
- OS-native placeholder integration (Windows CFAPI, macOS FSKit/File Provider) is a per-platform
  layer over the same local state; it replaces the visible marker, not the contract.

Current source backs the pinned marker bytes in `loom-substrate::drive`, conformance-vector
round-trips, MCP/REST/JSON-RPC upload rejection for marker bytes, and the `loom-hosted` local OS
projection primitives. `dehydrate_file_for_os` emits marker bytes without changing canonical Drive
content, `hydrate_file_for_os` converts marker bytes back into verified file content, and
`write_file_from_os` rejects marker bytes before creating an upload. `plan_os_projection_worker`
source-backs a generic worker decision layer over local materialization state: pinned dehydrated
files plan hydration, safely replicated unpinned hydrated files plan eviction, unsafe local files are
kept, and unknown local state is skipped. Focused `loom-drive` tests prove marker round trips,
marker-upload rejection, and hydrate/evict/keep/skip worker planning. Hosted REST and JSON-RPC routes
expose those four operations over daemon-opened Drive listeners. OS-native placeholder integration,
background worker scheduling, and platform-specific hydration/eviction adapters remain target work.

## 9. Sharing and Permissions

Driveish sharing applies to folders, files, comments, links, and derived artifacts.

Grant scopes:

```text
viewer
commenter
editor
owner
agent-reader
agent-editor
```

Sharing operations are durable operations in the drive log. Revocation must prevent new reads and writes,
but cannot erase content already synced to a device. Enterprise deployments that need stronger revocation
must use per-folder or per-file encryption keys and rotate keys on revocation.

Blind Loom Cloud can enforce authorization by opaque labels and share metadata it is allowed to see. If
share names, paths, or principals are confidential, they must be encrypted or represented by opaque labels
too.

Current source backs canonical `DriveShareIndex` records at
`profile/drive/v1/{workspace_id}/shares`. Each share grant records a target kind, target id, principal,
role, grant actor, grant time, and optional expiry. MCP, hosted REST, and hosted JSON-RPC expose
admin-gated share list/grant/revoke/expiry operations. Grants project into scoped ACL grants over
the Drive target, and revocation removes the projected ACL grant. Manual share-expiry application
removes due grants, revokes projected rights, and publishes `share.expired`. The daemon applies due
share-expiry policy for registered Drive policy targets as the same sequenced operation. External-link
policy and key rotation on revocation remain target work.

## 10. Trash, Retention, and GC

User delete moves a file or folder to trash by default. It does not immediately remove content objects.

Hard deletion is a policy operation:

- remove live references from folder projections;
- remove or expire search and preview indexes;
- retire content encryption keys when crypto-shredding is allowed;
- mark old roots outside the retention live set;
- let garbage collection reclaim unreachable chunks and metadata after the retention window.

Legal hold overrides hard deletion. Folder retention, file retention, shared-link expiry, and account
deprovisioning are separate policy inputs.

### 10.1 Retention Live-Root Set

Owner decision 2026-07-04 (resolves §17.9): **an explicit live-root pin set as data.** This is
the pattern JIRAISH §18.9 should copy.

- Each scope maintains a retention index enumerating **pins**: the current folder root, each
  trash-entry subtree root, legal-hold pins, and revision-retention pins (the 0061 §9
  checkpoint / §9.1 epoch roots retained by the collection's history policy).
- **Live** = reachable from any pin. The GC worker reclaims chunks, manifests, and metadata nodes
  unreachable from the pin set after the policy window. Dedup-shared chunks survive as long as
  any pin reaches them.
- Trash expiry is a sequenced `retention.applied` operation that removes the trash pin; legal
  hold adds a pin that no other policy can remove (hold release is its own audited operation).
  Hard delete = pin removal **plus** content-key retirement: key retirement (crypto-shred) is the
  immediate-effect mechanism; GC is space reclamation that follows eventually. Operation facts
  and version numbers persist per 0061 §9.
- The pin set is inspectable data: "why does this still exist" and access-review exports read it
  directly (ADOPTION §1.2). Retention workers mutate pins only through sequenced operations.

Current source backs canonical `DriveRetentionIndex` records at
`profile/drive/v1/{workspace_id}/retention`. Each pin records a pin kind, retained root, optional target
entity, actor, creation time, and optional expiry; legal-hold pins are validated as non-expiring.
MCP, hosted REST, and hosted JSON-RPC expose admin-gated pin list/add/remove operations plus manual
retention application. Manual application removes expired non-legal pins and publishes a sequenced
`retention.applied` operation. The daemon applies due retention pins for registered Drive policy
targets as the same sequenced operation. Key retirement, GC integration, access-review exports, and
OS-native hydration/eviction worker integration remains target work. Current source backs the generic
worker planning policy and hosted route exposure, but not a platform event loop or native placeholder
API binding.

## 11. Background Workers

Required workers:

- **Sync worker:** resumes transfers, verifies roots, applies operations, and backfills missing chunks.
- **Hydration worker:** fetches content for files marked offline-available.
- **Eviction worker:** removes local chunks that are not pinned and are safely replicated.
- **Index worker:** maintains path, metadata, search, comment, and recent-file indexes.
- **Preview worker:** renders thumbnails and text extraction when keys are available.
- **Security worker:** performs malware scanning, DLP, and file-type validation where keys are available.
- **Retention worker:** applies trash expiry, legal hold, key retirement, and deletion policy.
- **GC worker:** reclaims unreachable objects after policy permits it.
- **Notification worker:** converts drive root advancement into MCP and WebSocket wakeups.

Keyless workers can operate only on labels, sizes, encrypted frames, and visible metadata. Content-aware
workers require keys and must run as auditable principals.

### 11.1 Archive Preview and Extract

Owner decision 2026-07-04 (resolves the ADOPTION §1.3 archive-containers question for Driveish):
**in scope as a keyed-worker feature: preview plus extract, no pack.**

- The preview worker lists zip/gz/tgz/tar entries (name, size, entry digest where cheap to
  compute) as a derived preview artifact, like any other preview. Content-aware, therefore
  keyed-worker only; blind deployments simply lack archive previews.
- `files.extract(archive_file_id, target_folder_id, entries?)` materializes selected or all
  entries as **ordinary sequenced file operations** (`folder.created`/`file.created` with
  extraction provenance referencing the source archive's file id and version). Extraction is
  uploads with provenance. There is no special storage model, and the §7.4 matrix governs name
  collisions in the target.
- Archive *creation* (pack) is out of scope. Import-side container ingestion remains owned by the
  0012 import framework (ADOPTION §1.3); this feature is user-facing drive ergonomics, not the
  importer.

## 12. MCP as the Primary Protocol

Expose drive state as MCP resources:

```text
loom://{workspace}/drive
loom://{workspace}/drive/path/{path}
loom://{workspace}/drive/file/{file_id}
loom://{workspace}/drive/folder/{folder_id}
loom://{workspace}/drive/versions/{file_id}
loom://{workspace}/drive/conflicts
loom://{workspace}/drive/shares
```

Expose operations through MCP tools:

```text
drive_list
drive_stat
drive_read
drive_create_upload
drive_upload_chunk
drive_commit_upload
drive_create_folder
drive_move
drive_rename
drive.copy
drive_delete
drive.restore
drive_list_versions
drive.restore_version
drive_list_conflicts
drive_resolve_conflict
drive_list_shares
drive_grant_share
drive_revoke_share
drive_apply_share_expiry
drive_list_retention
drive_pin_retention
drive_unpin_retention
drive_apply_retention
drive.comment
drive.search
drive.update_cursor
```

All write tools execute as the resolved principal and are checked by the policy enforcement point.

## 13. Agent Callbacks and Subscriptions

Agents subscribe to folders, files, conflict queues, upload queues, or share events.

Example subscription:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "resources/subscribe",
  "params": {
    "uri": "loom://organization/acme/drive/main/folder/product-specs"
  }
}
```

When the folder projection changes, the MCP server emits a resource update notification. The agent then
fetches operations from its durable drive cursor:

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/call",
  "params": {
    "name": "drive_list",
    "arguments": {
      "folder_id": "product-specs",
      "after_sequence": 19342
    }
  }
}
```

The notification is a wakeup, not the source of truth. Durable delivery uses drive sequence cursors.

## 14. Elicitation

MCP elicitation is used when an agent or server needs structured input before proceeding.

Use elicitation for:

- resolving a conflict copy;
- approving a share with an external principal;
- choosing whether delete or edit wins;
- selecting a retention class;
- approving agent access to a sensitive folder;
- confirming whether to publish a generated file;
- asking which folder should receive an uploaded artifact.

Example:

```json
{
  "jsonrpc": "2.0",
  "id": 7,
  "method": "elicitation/create",
  "params": {
    "message": "Resolve the concurrent edits to Budget.xlsx.",
    "requestedSchema": {
      "type": "object",
      "properties": {
        "choice": {
          "type": "string",
          "enum": ["keep-current", "keep-conflict", "keep-both"]
        }
      },
      "required": ["choice"]
    }
  }
}
```

Elicitation responses become durable operations when they affect drive state.

## 15. WebSocket Secondary Transport

WebSocket can be offered for filesystem UI fanout and upload progress. It must preserve the same
semantics as MCP:

- authenticated principal context;
- authorized folder and file subscriptions;
- durable drive sequence cursors;
- idempotent writes;
- replay after reconnect;
- no stronger write authority than MCP tools.

## 16. Performance Requirements

The design must meet these requirements:

- editing one large file range does not rewrite unrelated files or folders;
- uploading one chunk does not require publishing a visible file version;
- finalizing an upload is atomic;
- listing a large folder is paginated and index-backed;
- moving a folder is metadata-only when contents are unchanged;
- renaming a file does not change file identity;
- sync transfers only missing objects and metadata nodes;
- conflict detection uses base version and operation identity;
- local folder views converge after replay;
- blind cloud mode remains usable for sync, lease coordination, and wakeup without content access.

## 17. Open Design Decisions

Status after the 2026-07-04 design session. Numbering is retained so external references stay
valid:

1. ~~Canonical operation envelope and payload encoding~~ - **0061-owned** (§2; see §5).
2. ~~Drive operation sequencer protocol and replay guarantees~~ - **0061-owned** (§3; see §6.1).
3. ~~Folder index structure and canonical path normalization~~ - **resolved**, §4.1.
4. ~~Chunking policy for large files and range updates~~ - **resolved**, §4.2.
5. ~~Conflict-copy naming algorithm~~ - **resolved**, §7.5.
6. ~~Merge policy matrix for folder metadata operations~~ - **resolved**, §7.4.
7. ~~Editor lease semantics~~ - **resolved**, §7.6.
8. ~~Dehydrated-file marker format~~ - **resolved**, §8.1.
9. ~~Retention live-root algorithm~~ - **resolved**, §10.1.
10. The conformance vector set for upload, move, conflict, share, delete, restore, and cursor
    replay - **open**: cursor replay is 0061-owned (§3/0035); the profile rows (§7.4 matrix,
    §7.5 naming, §4.1 folding, §4.2 chunk parameters, §8.1 marker bytes, §21 import equivalence)
    remain uniquely Driveish and unowned by any queue.

## 18. Recommended Shape

All contracts in this document are long-term decisions; sequencing is implementation staging that
never changes a contract (no-v1 principle). The recommended shape is:

- local-first Loom replicas hold organization keys and can see the full drive;
- Loom Cloud is a blind sync, operation sequencing, lease coordination, and notification replica by
  default;
- a separate keyed compute deployment is used when hosted search, previews, scanning, DLP, or server-side
  document merge are required;
- folder state is an operation-log projection, not raw current branch sync;
- binary concurrent edits create conflict copies;
- text merge and structured document merge are helpers, not assumed storage guarantees;
- MCP resources and subscriptions are the primary agent callback mechanism;
- WebSocket mirrors the same event and cursor contract for clients that need upload progress and low
  latency folder updates;
- deletion is trash by default, with hard deletion handled by retention, key retirement, and GC.

## 19. Example Tool Surface (illustrative only - not a design decision)

Names, grouping, and parameters are examples to make assistant ergonomics concrete, not designed
contracts. Underscore-flattened per MCP; capability `drive`.

| Category | Tool | Description |
| --- | --- | --- |
| Folders | `folders.create` | Create a folder |
| Folders | `folders.list` | Paginated, index-backed folder listing |
| Folders | `folders.move` | Move a folder; identity unchanged, path is an alias |
| Files | `files.upload` | Chunked content-addressed upload |
| Files | `files.download` | Fetch file bytes by digest |
| Files | `files.patch_range` | Range update per chunking policy (§17.4) |
| Files | `files.move` | Move a file within the tree |
| Files | `files.copy` | Copy; new identity, shared content addresses |
| Files | `files.get_metadata` | Fetch file metadata, scan status, retention class |
| Files | `files.history` | Revision index rows for a file (0061 §9) |
| Files | `files.restore_revision` | Make a prior revision the head (new operation) |
| Files | `files.trash` | Trash-by-default delete |
| Files | `files.restore` | Restore from trash |
| Files | `files.delete_hard` | Policy-gated hard delete via retention/key retirement |
| Files | `files.extract` | Extract archive entries as sequenced file operations (§11.1, keyed only) |
| Sharing | `shares.create` | Grant access to folder/file |
| Sharing | `shares.revoke` | Revoke a grant (cannot recall already-synced content) |
| Sharing | `links.create` | Create a share link per external-share policy |
| Annotations | `comments.add` | File comment (0061 §7) |
| Annotations | `comments.redact` | Redact; audit fact persists |
| Coordination | `leases.acquire` | Exclusive write-intent lease (0036-backed, §17.7) |
| Coordination | `leases.release` | Release a lease |
| Discovery | `drive.search` | Domain search over names/content where keys permit |
| Cursors | `cursors.update` | Advance the principal's durable cursor |

## 20. Unfinished Tasks (pushed back from Queue 8)

`specs/0061.md` owns the shared substrate: operation envelope, sequencer protocol, durable
cursors, conflict records, annotation subsystem (file comments per 0061 §7), entity versioning,
and view/projection machinery. The 2026-07-04 design session resolved the uniquely-Driveish open
decisions (§17.3 through §17.9 to §4.1, §4.2, §7.4 through §7.6, §8.1, §10.1) and pinned the import mapping
(§21) and the archive-worker decision (§11.1). Remaining, unowned by any queue:

- Remaining profile conformance vectors beyond the source-backed `drive-profile` runner: dehydration
  marker bytes and import cross-format equivalence. Current source already covers chunk manifests,
  folder indexes, file version indexes, profile snapshots, fold keys, conflict-copy naming, and
  selected merge-matrix outcomes.
- Importer *implementation* for §21 (mapping is pinned; the build rides the 0012 framework;
  build-focus tiers per ADOPTION §1.3).
- Operation-kind alignment pass: §5 kinds predate 0061 §7 (`comment.added` vs `annotation.*`)
  and §7.6 (`lock.*` vs lease vocabulary); align kind names with the substrate during the
  loom-substrate profile module work.
- Projection layouts for the required §4 indexes (facet choices), analogous to JIRAISH §18.8.
- **Import (requirement, ADOPTION §1.3):** the Drive/SharePoint-export → Driveish-operations
  mapping table (trees, files, revisions where exportable, shares → grants, comments, users →
  principals), path alias preservation, coexistence bridge semantics, and the per-run fidelity
  report.

## 21. Drive/SharePoint Import Mapping (requirement, ADOPTION §1.3)

Mapping pinned 2026-07-04, following the JIRAISH §25 pattern: history synthesizes **backdated
unconditional operations** carrying `import_provenance` (source system, source id, source
timestamp, import run id). Unconditional writes are lww by definition (§7.4), so import never
trips conflict machinery. Sources are file/export bundles and MCP-assisted normalized batches.
Takeout-style exports may carry files only; MCP-assisted input may include richer revisions,
permissions, comments, and SharePoint/OneDrive metadata when an assistant connector can observe
them.

Current source backs the reusable Drive importer in `loom-interchange-io`, the CLI
`loom interchange import-drive` path, and the generic 0012 import-execution batch path used by MCP
assisted imports. The broad fixture at `specs/studio/fixtures/drive/` is derived from Google Drive
API and Microsoft Graph DriveItem documentation: Google Drive `files`
(`https://developers.google.com/workspace/drive/api/reference/rest/v3/files`), Google Drive
`permissions` (`https://developers.google.com/workspace/drive/api/reference/rest/v3/permissions`),
Google Drive `comments` (`https://developers.google.com/workspace/drive/api/reference/rest/v3/comments`),
Google Drive `revisions` (`https://developers.google.com/workspace/drive/api/reference/rest/v3/revisions`),
and Microsoft Graph `driveItem`
(`https://learn.microsoft.com/en-us/graph/api/resources/driveitem?view=graph-rest-1.0`).

The importer accepts normalized Drive or SharePoint snapshot JSON, creates folders through the
reusable Drive service, writes inline text, inline hex bytes, local `content_path` file bytes, and
import-execution sidecar payload bytes through the Drive upload/commit service path, replaces changed
existing files, skips unchanged files idempotently, records Drive revisions for committed uploads,
and emits the shared 0012 import report. Google Drive Takeout or direct API archive parsing and
SharePoint export parsing remain target work; the source-backed input is the normalized snapshot that
CLI scripts, MCP-assisted import, or connector sessions can produce.

Source-backed coverage matrix:

| Source construct or field | Current source-backed handling |
| --- | --- |
| Normalized Drive/SharePoint snapshot | 1:1 accepted as the fixture and generic execution payload format. |
| Folder `id`, `parent_id`, `name` | Imported as Drive folder identity and placement. |
| File `id`, `parent_id`, `name` | Imported as Drive file identity and placement. |
| File `text` | Imported as current file bytes. |
| File `content_hex` | Decoded and imported as current file bytes. |
| File `content_path` | Direct import reads from the snapshot directory. Generic execution-batch import materializes sidecar payloads by safe relative path before import. |
| Changed existing file | Replaced through the Drive upload/commit path. |
| Unchanged existing file | Skipped idempotently. |
| Google Drive parents beyond `parent_id` | Unsupported with fidelity issue; multi-parent lowering to shortcuts remains target work. |
| Google Drive permissions, comments, revisions, labels, restrictions, owners, capabilities, web links, checksums, MIME metadata, drive id, trash state | Unsupported with fidelity issue. |
| Google Drive shortcuts and remote items | Imported only as placeholder current file bytes when supplied; shortcut directory-entry semantics remain target work and emit fidelity issues. |
| SharePoint `sharepointIds`, `retentionLabel`, `listItem`, `@microsoft.graph.downloadUrl`, versions, permissions, thumbnails | Unsupported with fidelity issue. |
| Direct permission/share mapping, comments, shortcut entries, multi-parent lowering, identity mapping, native Google export conversion, SharePoint library/package semantics | Target work. |

### 21.1 Mapping Table

| Source construct | Driveish target | Notes |
| --- | --- | --- |
| Drive folder / SP library + folder | `folder.created`, verbatim names | an SP document library maps to a top-level folder or its own drive root, per import config |
| File head content | `file.created` + chunked upload + `file.upload_committed` (§4.2) | chunk manifests computed at import |
| Revisions (Drive revisions API / SP versions) | version chain: one backdated `file.content_replaced` per exportable revision | **where exportable**: Takeout and some SP exports are head-only → head-only chain, fidelity-reported |
| Google-native docs (Docs/Sheets/Slides) | exported-format bytes (docx/xlsx/pptx or markdown per config), `content_type` recorded (0061 §9) | native format has no byte export; the conversion itself is a fidelity entry |
| Permissions (Drive permissions / SP role assignments) | `share.granted` → §9 grants: read→`viewer`, comment→`commenter`, write/contribute/edit→`editor`, owner/full-control→`owner` | only **direct** grants import; inherited access re-derives from the folder tree. Uninheritable SP fine-grained roles degrade to nearest scope, fidelity-reported |
| Anyone-with-link / anonymous links | `links.create` per external-share policy, flagged for admin review | imported disabled when policy forbids external links |
| Comments + replies + resolved state | 0061 §7 annotations (`annotation.added`/`resolved`), entity anchors; Docs anchored ranges → range anchors only where a §9.1-typed body exists, else entity anchor + quoted text | quoted text preserved for degraded anchors |
| Users and groups | principals via SCIM/directory mapping; unmapped users become **inactive placeholder principals** (JIRAISH §25 rule, SCIM-mergeable); groups become principal groups | actor attribution on backdated ops uses the mapped/placeholder principal |
| Shortcuts (Drive) / legacy multi-parent | `shortcut` directory-entry kind targeting the stable `file_id` | entry kinds are `file \| folder \| shortcut`; multi-parent files get one primary path + shortcuts, fidelity-reported |
| Same-name siblings (Drive allows them; §4.1 does not) | first keeps the name; later fold-equal siblings get deterministic ` (n)` suffixes | every renamed sibling is a fidelity entry |
| Trashed items | trash entries with original path | where the export includes trash |
| SP metadata columns / Drive appProperties | opaque metadata map on file metadata | not typed fields; fidelity-reported as opaque |
| Source item ids and URLs | 0061 §4 aliases + redirect map | existing links keep resolving (ADOPTION §1.3 alias preservation) |

### 21.2 Coexistence Bridge

An imported drive scope runs in **`mirror(source)` operating mode (0061 §7.1)**: read-only to
local writers, fed by assistant-assisted normalized observations, replaying source changes as
further backdated unconditional operations.
`drive.cutover` is the mode change to `read_write`; it stops the bridge and records the
last-synced source baseline in the scope's audit log. Per-folder migration is scoping the mirror,
not a different mechanism.

### 21.3 Fidelity Report

Every run emits: counts of mapped/degraded/dropped per construct class; head-only files (missing
revisions); native-doc conversions and their target formats; permission degradations; unmapped
principals represented as placeholders; same-name sibling renames and OS-unmaterializable names (§4.1); anchored
comments degraded to entity anchors; shortcuts synthesized from multi-parents; opaque metadata
carried. The current normalized importer emits shared fidelity issues for present permissions,
comments, historical revisions, metadata, and shortcut targets until those lowerings are implemented.
The report is itself a drive file with a stable schema, per ADOPTION §1.3.
