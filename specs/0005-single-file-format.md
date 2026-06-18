# 0005 - Single-File Container Format (`.loom`)

**Status:** Source-backed current format. **Version:** 0.1.0. **Normative.**

This document specifies the source-backed byte-level format for the `SingleFile` provider
(`crates/loom-store::FileStore`, 0004 §3.1). All multi-byte integers are little-endian unless stated.
Offsets and lengths are in bytes. `uvarint` denotes unsigned LEB128. Target storage-format
extensions are tracked by 0005a and are not part of this format until promoted.

> **Current implementation:** `FileStore` is a page engine. A `.loom` file has two 4096-byte
> superblock slots, one 4096-byte journal-ring slot, then a 4096-byte page array. Object records are
> stored on slab pages or large-record page runs. A copy-on-write B-tree maps digest bytes to record
> locators. A region-table page points at the object-index root, free-page-map root, and conservative
> maintenance-state root. The engine-state
> root is a digest in the superblock and journal record. Every committed write fsyncs referenced pages,
> then fsyncs a journal `COMMIT` record; that journal fsync is the commit point. Superblocks are
> periodic checkpoints, not the per-transaction commit point.

## 1. Design goals

- **Self-contained and portable.** Copying one `.loom` file copies objects, workspace state, and
  history.
- **Portable hosted configuration target.** Certificate bundles in 0052 extend this portability goal to
  hosted TLS material by copying operator-provided certificate bytes into the `.loom` file instead of
  leaving durable hosted listener records dependent on host filesystem paths.
- **Crash-consistent.** A power loss leaves a valid prior committed generation or the new committed
  generation, never a torn root set.
- **Page-addressed.** Data after the fixed header is a 4096-byte page array addressed by `PageId`.
- **Content-addressed.** Object identity is the store-profile hash of plaintext canonical bytes.
- **Transform-friendly.** Compression and encryption are storage frames below the digest boundary.
- **Profile-aware.** The store records one identity-profile digest algorithm, BLAKE3 or SHA-256 today.

## 2. File layout

```
offset  size       region
0       4096       superblock slot A
4096    4096       superblock slot B
8192    4096       journal ring slot
12288   N * 4096   page array
```

Constants:

| Name | Value | Source meaning |
| --- | --- | --- |
| `MAGIC` | `LOOMFS\x00\x01` | file magic |
| `SLOT_SIZE` | 4096 | superblock slot size |
| `JOURNAL_OFFSET` | 8192 | start of journal ring |
| `RING_SLOTS` | 32 | commit records retained in the ring |
| `CHECKPOINT_INTERVAL` | 16 | commits between superblock checkpoints |
| `DATA_START` | 12288 | start of page array |
| `PAGE_SIZE` | 4096 | page size |
| `SEGMENT_BYTES` | 64 MiB | logical segment size for GC accounting |
| `PAGES_PER_SEGMENT` | `SEGMENT_BYTES / PAGE_SIZE` | pages per logical segment |

The current journal ring occupies one 4096-byte slot. Each journal record is 67 bytes, so 32 records
fit in the ring.

## 3. Superblock

Two superblock slots are written alternately for checkpoints. On open, both slots are decoded and the
valid slot with the highest generation is the checkpoint baseline. Recovery then overlays any newer
valid journal `COMMIT` records.

Superblock layout:

```
off   size   field
0     8      magic = "LOOMFS\x00\x01"
8     2      format_major = 1
10    2      format_minor = 0
12    8      generation
20    1      digest_algo
21    8      page_count
29    1      region_table_present
30    8      region_table_page_id, valid iff region_table_present != 0
38    1      reference_present
39    32     reference_digest, valid iff reference_present == 1
71    1      control_present
72    32     control_digest, valid iff control_present == 1
104   1      encryption_present
105   2      encryption_meta_len, valid iff encryption_present == 1, little-endian u16
107   var    encryption_meta bytes, length `encryption_meta_len`
...          zero-reserved bytes through offset 4091
4092  4      crc32c over bytes [0,4092)
```

Rules:

- `format_major` must be supported. Unknown major versions are unreadable.
- `digest_algo` must decode to a supported `Algo`.
- A present `region_table_page_id` must be below `page_count`.
- `reference_digest` is the engine-state root object digest, if present.
- `control_digest` is the durable local control-plane root object digest, if present.
- `encryption_meta` is encoded key metadata from 0009. The only supported metadata schema is the
  source-tagged multi-wrap descriptor `LKM1 || 2 || suite || u16{n} || entries`. It is stored under
  the superblock CRC, carried forward into every checkpoint, and updated immediately on rekey.
- Reserved bytes are zero in source-generated stores.

Fresh stores write an initial checkpoint superblock and start with an empty page array.

## 4. Region table

The region table is a CRC-protected blob stored at the page id referenced by the superblock or journal
record. Its encoded length is 48 bytes and it fits in one page.

```
off  size  field
0    1     magic = 0xB6
1    8     page_size
9    1     index_root_present
10   8     index_root_page_id, valid iff present
18   1     freemap_root_present
19   8     freemap_root_page_id, valid iff present
27   1     maintenance_root_present
28   8     maintenance_root_page_id, valid iff present
36   8     open_segment
44   4     crc32c over bytes [0,44)
```

The region table carries three roots:

- `index_root`: root page of the object-location B-tree.
- `freemap_root`: root page of the free-page map.
- `maintenance_root`: root page of conservative maintenance metadata.

The engine-state root is not in the region table. It is a digest and is stored directly in the
superblock and journal record.

## 5. Object records

The logical object record format is:

```
field          encoding
rec_magic      u8 = 0xB0
digest         32 bytes
frame_id       u8
plain_len      uvarint
stored_len     uvarint
stored         `stored_len` bytes
crc32c         u32 over all prior bytes in this record
```

The digest is over plaintext canonical bytes under the store's `digest_algo`. On read, the store
checks the record CRC, decrypts if needed, decompresses if needed, checks `plain_len`, and verifies
`Digest::hash(store_algo, plaintext) == requested_digest`.

### 5.1 Storage frames

| Frame id | Meaning |
| --- | --- |
| `0x00` | identity |
| `0x01` | DEFLATE via `miniz_oxide` |
| `0x02` | LZ4 via `lz4_flex` |
| `0x10` | identity then AEAD |
| `0x11` | DEFLATE then AEAD |
| `0x12` | LZ4 then AEAD |

Compression is attempted only when the payload is at least 1024 bytes. If compression does not shrink
the payload, the record is stored as identity. AEAD frames store `suite_id || nonce || ciphertext ||
tag`, with associated data binding the frame format version, frame id, suite id, digest, `plain_len`,
and `stored_len`.

Frames are storage policy only. They never affect object identity, sync identity, or conformance
vectors.

## 6. Record pages

Framed records are placed into the page array in one of two page-level formats.

### 6.1 Slab page

Small framed records, currently records whose framed length is at most `PAGE_SIZE / 4`, are packed into
a shared slab page.

```
off   size        field
0     1           magic = 0xB5
1     2           slot_count
3     4*n         slot directory: n entries of { offset u16, len u16 }
...               packed framed records
4092  4           crc32c over bytes [0,4092)
```

The object index stores the slab page locator plus an intra-page slot number.

### 6.2 Large-record page run

Large framed records are stored in their own contiguous page run.

```
off       size   field
0         1      magic = 0xB6
1         8      framed_record_len
9         len    framed record
9+len     4      crc32c over bytes [0,9+len)
...              zero padding to a page boundary
```

The object index stores the first page locator and slot `0`.

## 7. Object index

The object-location index maps 32 digest bytes to a `RecordLoc`:

```
RecordLoc = segment_id uvarint, page_index uvarint, slot uvarint
global_page = segment_id * PAGES_PER_SEGMENT + page_index
```

The index is a copy-on-write B-tree. One node occupies one page:

```
off      size      field
0        1         magic = 0xB7
1        1         flags, bit 0 means leaf
2        2         entry_count
4        var       entries: entry_count * { key[32], RecordLoc }
...      var       internal nodes only: (entry_count + 1) child PageId u64 values
4092     4         crc32c over bytes [0,4092)
```

Nodes are immutable after commit. Insert or delete operations write fresh nodes along the changed
path and free superseded pages into the transaction allocator. The committed `index_root` is published
through the region table.

## 8. Free-page map and allocation

The free-page map persists reusable page runs:

```
field      encoding
magic      u8 = 0xB4
count      u32
runs       count * { start u64, len u64, freed_gen u64 }
crc32c     u32 over prior bytes
```

Allocation order:

1. Reuse pages freed earlier in the same transaction if those pages were extended within that same
   transaction.
2. Reuse prior-generation free runs only after `freed_gen + REUSE_SAFE_WINDOW <= txn_gen`.
3. Extend the page array.

`REUSE_SAFE_WINDOW` is the maximum of `RING_SLOTS` and `2 * CHECKPOINT_INTERVAL`. This keeps pages
reachable by any recoverable generation from being overwritten too early.

At commit, the prior free-page map and prior region-table page are freed, the new map is written, a
new maintenance-state page is written, a new region table is written, and a trailing free run at the
top of the page array may be truncated.

## 8.1 Maintenance-state page

The maintenance-state page is a CRC-protected advisory record rooted from the region table. It records
the physical allocation state and candidate maintenance debt produced by ordinary commits. It is not a
reachability proof and cannot authorize deletion without an independent mark phase.

```
field                       encoding
magic                       u8 = 0xB7
version                     u8 = 2
flags                       u8, bit 0 means segment-list overflow
generation                  u64
object_count                u64
physical_page_count         u64
reusable_free_pages         u64
candidate_dead_pages        u64
last_validated_mark_epoch   u64
touched_segment_count       u16
candidate_segment_count     u16
touched_segments            sorted u64 list
candidate_segments          sorted u64 list
crc32c                      u32 over prior bytes
```

Rules:

- Missing `maintenance_root` means no maintenance state is present, as in legacy stores.
- Bad magic, version, flags, CRC, bounds, or unsorted segment lists make the store corrupt on open.
- `object_count` is the number of committed live object records in the object index for this
  generation. Store administration and bounded object-index opens read this count from the
  maintenance root instead of reconstructing it by walking the entire index.
- `candidate_dead_pages` is the current reusable free-page count. It is a maintenance-prioritization
  hint, not a liveness result.
- `touched_segments` and `candidate_segments` are bounded sorted sets. If either set overflows, the
  overflow flag is set and consumers must treat the record as incomplete.
- `last_validated_mark_epoch` advances only when a completed Loom-engine reachability mark epoch is
  committed together with its control-plane cursor record. Incomplete epochs never authorize
  reclamation.

The active reachability mark epoch is a durable-local control-plane record under
`maintenance/v1/reachability-mark/active`. The record stores the epoch id, base maintenance
generation, captured reference root, captured control-plane fingerprint, captured derived-artifact
roots, the marked set, the pending object queue, pending stream roots, and the completion flag. The
control-plane fingerprint is computed from the durable-local control map excluding the active
mark-epoch record itself, so epoch progress writes do not invalidate their own cursor. The engine
supplies traversal semantics for workspace refs, tags, working trees, open inodes,
content-addressed blobs, streams, and facet roots. The store only persists the cursor and validates
completion. If the reference root, control-plane fingerprint, or derived-artifact roots changed
since the epoch began, completion fails with `CONFLICT` and `last_validated_mark_epoch` does not
advance.

Store-maintenance policy and run state are durable-local control-plane records:

- `maintenance/v1/policy` stores minimum candidate-dead and reusable-free page thresholds, interval
  and backoff milliseconds, bounded segment and page budgets, whole-file compaction policy, tail
  trim policy, and tail-compaction policy. Source-backed defaults allow local maintenance to run
  with bounded budgets and keep whole-file compaction disabled unless policy explicitly permits it.
  Durable policy writes validate the encoded policy before mutation. Zero interval, zero backoff,
  zero max segments, and zero max pages are rejected without replacing the previous policy.
- `maintenance/v1/run-state` stores the last run time, next eligible time, last skip reason, and last
  error. This state is diagnostic and scheduler-facing. It does not authorize reclamation.

Deleted content is reclaimable only after no current durable live root references it. Current live
roots include workspace refs and tags, persisted working-tree and staging roots, stream roots, facet
roots, open-handle or transaction roots captured by the engine, maintenance mark-epoch captured
roots, derived-artifact roots, and configured retention roots when present. Retention is represented
by these explicit roots rather than per-object lifecycle state.

`FileStore::store_maintenance_report` is the source-backed status projection. It joins maintenance
state, durable policy, run state, and the active mark epoch into the bytes and reasons shown by
operator surfaces. It reports physical bytes, marked-live bytes, candidate reclaimable bytes,
reusable free bytes, mark epoch identity and completion, last validated mark epoch, eligibility, and
the reason when maintenance is not eligible.

## 8.2 Derived-artifact lifecycle (shared contract)

The embedded derived-artifact store is the single, source-backed lifecycle every facet uses for
rebuildable native indexes and analytical accelerators (full-text indexes, vector accelerators,
columnar projections, graph property/spatial indexes, dataframe materializations, and PIM derived
indexes). It is implemented by `crates/loom-store/src/derived.rs`, which is the source of truth for
this section; facet specs reference this contract instead of restating their own rebuild lifecycle.

**Canonical config vs materialization.** A facet's index *declaration* (analyzers, indexed keys,
projection shape) is canonical Loom state: it is committed to the versioned object DAG, synced, and
part of identity. The *materialization* (the physical index/accelerator bytes) is a derived artifact:
it lives in the durable-local control plane, is excluded from identity, is never synced, and is
rebuilt on demand. Only the declaration participates in the content address; changing it forces a
rebuild.

**Keys and records.** A derived artifact is addressed by a `DerivedArtifactKey`
(`workspace`, `facet`, `collection`, `artifact`) under the `derived/artifact/v1/` control-plane
prefix; in-flight rebuild records live under `derived/rebuild/v1/`. A stored `DerivedArtifactRecord`
carries its `stamp`, `payload_digest`, and `payload_len`.

**Source-anchor stamp.** Freshness is decided by a `DerivedArtifactStamp`
(`source_digest`, `engine_version`, `format_version`). `source_digest` is the content address of the
canonical source the artifact derives from; `engine_version` is the building engine's version;
`format_version` is the artifact's on-disk format identifier. An artifact is fresh only when its
stored stamp equals the expected stamp; any change to committed source, engine version, or format
version makes it stale before it is trusted.

**States.** The lifecycle states are `Missing`, `Stale`, `Rebuilding { run_id }`, `Ready`,
`Failed { message }`, and `Unsupported { message }`. `Unsupported` (for example an accelerator whose
engine feature is not compiled in) is distinct from a transient `Failed`.

**Rebuild and coalescing.** Beginning a rebuild returns `AlreadyReady` (a fresh artifact already
exists), `Started { run_id }`, or `Coalesced { run_id }` (an equivalent rebuild for the same stamp is
already running, so concurrent requests fold into one run). A rebuild then either finishes (storing
the payload and stamp) or fails (recording a message). Rebuild writes are serialized by the
`FileStore` single-writer guard (0036 §8).

**Serve-read policy.** A read serves the payload only when the artifact is `Ready` and stamp-fresh;
on `Missing`, `Stale`, `Rebuilding`, `Failed`, or `Unsupported` the caller falls back to the
authoritative path (for example vector search falls back to exact scan) or returns a typed
degraded/unavailable result. `DerivedArtifactStatus::serving_policy` is the source-backed shared
projection: `Missing`, `Stale`, `Rebuilding`, and `Failed` serve the authoritative source and report
the derived capability as `degraded` with an explicit reason code, while `Unsupported` serves the
authoritative source and reports the derived capability as `unsupported` with stable error
`UNSUPPORTED`. Derived artifacts are never authoritative, never part of the content address, and
never synced: `push`/`pull`/`clone` move the object DAG only, so a receiver rebuilds locally.

## 9. Journal and recovery

The journal ring stores root-set commit records. Each record is:

```
off  size  field
0    4     magic = "JRNL"
4    1     kind: 0 PREPARE, 1 COMMIT, 2 ABORT, 3 CHECKPOINT
5    8     generation
13   8     page_count
21   1     region_table_present
22   8     region_table_page_id
30   1     reference_present
31   32    reference_digest
63   4     crc32c over bytes [0,63)
```

Current source writes `COMMIT` records for durable commits. The other kinds are reserved by the
decoder but are not the source-backed transaction protocol.

Commit sequence:

1. Write object record pages and copy-on-write index pages.
2. Write the free-page map, if present.
3. Write the maintenance-state page.
4. Write the region-table page.
5. Fsync referenced pages.
6. Write a journal `COMMIT` record into slot `generation % RING_SLOTS`.
7. Fsync the journal record. This is the commit point.
8. Optionally shrink trailing free pages.
9. Every `CHECKPOINT_INTERVAL` generations, write the current root set to one superblock slot and
   fsync it.

Open sequence:

1. Read both superblock slots.
2. Select the valid slot with the highest generation.
3. Scan the journal ring for valid records.
4. Adopt the newest valid `COMMIT` root set ahead of the selected superblock.
5. If recovery adopted a journal record that is ahead of the checkpoint, write a checkpoint slot.
6. Load the region table, free-page map, maintenance state, and reference root.
7. Resolve object reads and existence checks through bounded object-index B-tree lookups. A process
   may cache validated object locators and B-tree node pages, but caches are bounded and are not part
   of the durable format. Maintenance operations that require a whole-index view materialize the index
   explicitly.

If the file is shorter than `DATA_START + page_count * PAGE_SIZE` for the selected generation, open
fails as corrupt rather than reading past EOF.

## 10. Engine-state persistence

`Loom::save_state` serializes the workspace registry, branch/tag state, working trees, and
content-address map into independently rooted section objects under one canonical Tree root.
`FileStore::set_reference_root` stores that section Tree root as the superblock and journal
`reference_digest`. `save_loom` writes the exported state root and then updates the store's reference
root. Current stores require this root to decode as a canonical Tree whose entries are the named
engine-state sections; a monolithic Blob engine-state root is not a supported current format.

This means refs are not a standalone B-tree in the current format. They are engine state inside a
content-addressed object graph anchored by `reference_digest`. The current root is split by state
section so store open can move toward independently loadable roots without introducing sidecars.

The `01-content` section is promoted from a monolithic section Blob into a structured Tree. Each
entry name is the content digest string and the entry target is the stored object digest for that
content. This keeps the top-level engine manifest stable while letting the largest fixed-width map
share structure independently from unrelated engine-state sections. Other current sections remain
section Blob objects until source-backed growth data justifies promotion to a typed root.

Metadata-only readers can load only the `00-registry` section from the engine-state section Tree and
retain the other section handles for later materialization. This path is used for local MCP workspace
list/get reads and local CLI workspace-list reads so registry-only inventory does not decode the
content map, working tree state, staging index, merge state, open-file table, or protected-ref
section. Read-only file and VCS operations on a registry-only engine fail closed until the full state
is materialized. Mutable file and VCS operations materialize the retained section handles before
reading or changing mutable state. Local client metadata sessions use the same retained-handle model:
workspace metadata reads stay registry-only, while ordinary local client operations materialize before
executing. The normal full open path still materializes all sections before returning the engine.

The next engine-state promotions are ranked by growth risk and by whether multiple sections can
share one canonical representation. The `02-work` and `07-index` sections are promoted together
because they store the same workspace/path to staged-entry shape.

| Section | Current root | Growth driver | Target root | Direction |
|---|---|---|---|---|
| `02-work` | Structured staged-entry path-map Tree | Workspace count times path count, including structured facet roots stored as staged entries | Structured staged-entry path-map Tree keyed by workspace and path | Source-backed current representation. Blob roots for this section are rejected. |
| `07-index` | Structured staged-entry path-map Tree | Workspace count times staged path count; duplicates the `02-work` path-to-staged-entry shape | Structured staged-entry path-map Tree keyed by workspace and path | Source-backed current representation. Blob roots for this section are rejected. |
| `03-dirs` | Blob section payload | Workspace count times explicit directory count | Typed path-set root keyed by workspace and directory path | Promote after the staged-entry map codec lands; it is simpler but lower impact. |
| `06-merge-state` | Blob section payload | Number of conflicted workspaces, conflict paths, and pre-merge path snapshots | Bounded operational root or reuse the staged-entry map codec for embedded pre-merge maps | Keep behind work/index promotion because large merge records depend on the same staged-entry shape. |
| `08-open-files` | Blob section payload | Active inode and handle count in stateless per-operation reopen workflows | Bounded operational root | Keep operational and validate limits; do not make it the first structured promotion. |
| `09-protected-refs` | Blob section payload | Number of protected branch/tag policies | Compact typed table or bounded Blob | Low growth risk; promote for uniformity after path-heavy sections. |
| `04-compression` and `05-consumer-offsets` | Blob section payload | Workspace override count; queue consumer count | Bounded Blob or small typed table | Lower priority than path-heavy VCS sections. |

## 10.1 Local open-amplification instrumentation

`FileStore` exposes local `StoreIoStats` counters for store-runtime diagnostics. These counters are
not encoded into the `.loom` file and are not part of the public wire ABI. They report bounded
locator-cache occupancy, locator-cache hits and misses, B-tree page-cache occupancy, B-tree page-cache
hits and misses, B-tree pages read from the backing, sparse lookup count, materialized-index lookup
count, and whether open had to materialize the object index to recover legacy maintenance metadata.

The counters exist to make cold-open and repeated-read costs testable. They must not be used as
durable correctness state.

## 11. Compaction, GC, and rekey

- `compact` rewrites all stored objects into a dense replacement `.loom` and atomically renames it
  over the original file on native platforms.
- `compact_retaining` performs whole-file compaction while dropping objects outside a caller-supplied
  live set.
- `ensure_compaction_capacity` preflights whole-file compaction against the store directory. On Unix
  platforms it reports available temporary bytes with `statvfs` and fails with `RESOURCE_EXHAUSTED`
  before rewriting when available space is below the current physical store size. On platforms without
  a source-backed free-space signal, it reports unknown available bytes and lets the atomic temp
  rewrite surface any IO failure.
- `gc_segments` reclaims dead pages in place by segment live ratio.
- `gc_validated_segments` reclaims dead pages in place only from maintenance-candidate segments after
  a completed validated mark epoch is present. Each run is bounded by segment and page budgets.
- `gc_loom` asks the engine for the live set, then calls `compact_retaining`.
- `rekey` updates encoded encryption metadata and forces an immediate superblock checkpoint.
- `rekey_reseal` rewrites object frames under new encryption metadata through the compaction path.

These operations are direct native single-writer operations: the caller holds a writable `FileStore`
handle, so the file advisory lock, store mutexes, journal commit path, and forced checkpoint path are
the coordination guard. They are not public held locks. Hosted or daemon-mediated maintenance must use
the 0036 §8 runtime lease model before it admits concurrent hosted writers.

`gc_validated_segments` is the source-backed bounded in-place maintenance entry point. It refuses to
run if no active mark epoch exists, if the active mark epoch is incomplete, or if
`last_validated_mark_epoch` has not advanced to that epoch. It captures immutable reclaim evidence
from the current store generation, reference root, control root, object-index root, control-plane
fingerprint, and derived-artifact roots, then performs the broad index scan, sparse-segment
selection, and survivor payload reads outside the writer-critical section. The broad index scan walks
the captured copy-on-write object-index root from disk instead of cloning the in-memory index while
holding the store mutex; snapshot pages are read under short per-page file locks. Immediately before
mutation it enters the store serialization boundary, recomputes the evidence, and commits the bounded
reclaim transaction only if the evidence is unchanged and the completed epoch still matches the
current reference root, control-plane fingerprint, and derived-artifact roots. A mismatch clears the
stale active epoch and returns `CONFLICT`, so a later mark cycle must prove the new root set before
any bounded reclamation can run. When validation passes, it uses the epoch's retained digest set,
keeps the current reference root, current control root, and derived-artifact payload roots alive, and
selects only sparse segments listed in the maintenance candidate set. `max_segments` and `max_pages`
bound a cycle. Freed pages return to the free-page map for later reuse; this path does not perform a
whole-file replacement and does not require an idle window.

The daemon owns automatic local store-maintenance scheduling. The daemon periodically reads the
durable maintenance report, respects the durable policy interval and backoff, records skip and
failure reasons in run state, and opens a daemon-authorized writable store only when the report is
eligible. A run advances or resumes the reachability mark epoch first. When the policy does not allow
whole-file compaction, the daemon calls `gc_validated_segments` only after completion is validated.
When the policy explicitly allows whole-file compaction and the material-debt thresholds are met, the
daemon preflights temporary capacity and then calls `gc_loom`, which uses `compact_retaining` and
atomic same-directory replacement to physically reclaim file size while preserving live data.
`loom daemon maintenance run` performs one pass explicitly. `loom daemon maintenance status` and
`loom doctor store <store>` report the same maintenance projection without mutating the store. Stateless
operation does not compact on shutdown, and automatic whole-file compaction remains disabled unless a
durable policy explicitly allows it.

## 11.1 Controlled pre-release normalization

Pre-release stores are normalized with an explicit destination-copy protocol, not by editing the
active file in place. The source-backed CLI surface is:

```
loom store copy <source.loom> <destination.loom> --with compacted --format json --report-file <report.json>
```

Add `--with fips` when the target also changes identity profile. Add `--dry-run` to produce the same
plan and report shape without writing the destination.

The protocol is:

1. Pick a destination path that does not already exist. The unchanged source file is the backup until
   the operator deliberately swaps paths outside Loom.
2. Run `store copy --dry-run --format json --report-file <report.json>` and review the source
   identity profile, target identity profile, selected modifiers, workspace count, encryption state,
   omitted items, warnings, and disabled listener import counts.
3. Quiesce external writers before any operator-controlled replacement. The copy command itself opens
   the source read-only and writes only the new destination; the active source is never rewritten by
   this protocol.
4. Run `store copy` without `--dry-run`. Profile-preserving copies use a byte copy followed by
   optional `gc_loom` compaction. Profile-changing copies rebuild each workspace through the
   profile-migration path and reject dirty workspaces before writing the destination.
5. Validate the destination with the current binary before any path swap: run
   `loom store preflight-replacement <destination.loom> <coordination-workspace>`, which opens the
   store read-only, reads store maintenance status, lists workspaces, lists lanes, lists tickets, and
   reports an explicit blocking error if the active binary cannot read the candidate format. Operators
   may also run `doctor store` and any additional application-specific coordination reads the
   deployment depends on.
6. Replace the active store only after the destination validates. Keep the source backup until the
   replacement has been opened by the active binary and the application-level reads have passed.
7. Remove obsolete compatibility readers or temporary migration helpers only after all known
   pre-release stores covered by them have been normalized and validated. Compatibility code must not
   linger as an undocumented open path.

Interrupted migration safety follows from destination-copy semantics: a failed or interrupted copy may
leave an incomplete destination, but it does not mutate the source. Operators discard the incomplete
destination and rerun the protocol from the source backup.

The matrix store normalization precedent is this protocol applied to `matrix/matrix.loom`: dry-run the
candidate copy, create a compacted destination, validate it with the same binary that will serve it,
then perform any repository path replacement as an explicit operator action. The path
`matrix/matrix.loom` is a precedent, not a hardcoded migration target.

## 12. Versioning and forward compatibility

- Unknown `format_major` is unreadable.
- `format_minor` is currently written as `0`.
- Unknown digest algorithms are unreadable because the store cannot address objects under an unknown
  profile.
- The identity-profile digest algorithm is immutable after creation.
- The current format does not use a `flags` bitfield, pack-split sibling files, a standalone ref
  B-tree, a generated binary schema, or externally visible capability metadata.

## 13. Target format extensions

The following ideas are not source-backed in the current format and must not be treated as
implemented. They are tracked in 0005a so they do not block this current byte-layout contract:
pack-split sibling files, standalone ref or reflog regions, signed commit or tag enforcement flags,
pin-retention records, generated binary schemas, public capability metadata embedded in the file,
remote-provider storage regions, and additional identity-profile fields beyond the stored digest
algorithm plus engine-state objects.

## Resolved decisions

1. **Superblock size.** The source-backed superblock slot size is fixed at 4096 bytes.
2. **Generation ordering.** The valid root set with the highest generation wins, after CRC and
   structural checks.
3. **Commit point.** The journal `COMMIT` fsync is the commit point. Superblocks are periodic
   checkpoints.
4. **Compression and encryption.** Compression and encryption are storage frames below the digest
   boundary. Source stores plaintext identity, optional compression, and optional AEAD sealing.
5. **Object index placement.** The object index is a page-resident copy-on-write B-tree rooted through
   the region table.
6. **Engine refs.** Current refs and workspace metadata are exported engine state anchored by a
   reference digest, not a standalone ref store region.
7. **Pack-split.** Pack-split remains target work. The current flagship format is one `.loom` file.
8. **Read-only behavior.** Read-only opens are lock-free. Writable native opens take an exclusive
   advisory lock for the handle lifetime.

## Change log

- 2026-06-28 (task 229, 217f): Decomposed `crates/loom-store/src/lib.rs` with **no format or behavior
  change** - it went from 2,288 lines to 1,311. The `FileStore` struct, the open/control/crypto/commit
  half of its impl, the `ObjectStore` impl, and the `open_loom*` constructors stay in `lib.rs`. Four
  modules moved out: `record_io.rs` (on-disk record/transaction/control-map serialization -
  `encode_record`/`decode_record`/`write_record_pages`/`finish_txn`/`TxnRoots`/control-map + lock-fence
  codecs), `superblock.rs` (the `Superblock` header struct + read/write/checkpoint impl), `compact.rs`
  (the compaction/GC half of the impl - `compact`/`compact_retaining`/`gc_segments`/`rekey_reseal`/
  `compact_inner` - as a second inherent `impl FileStore`), and `backing.rs` (the `BackingIo`
  block-device trait + `MaybeSend` + the `std::fs::File` / `MemoryBacking` impls). `record_io`/
  `superblock` are `pub(crate)` glob-re-exported; `backing` is `pub` re-exported (it is the crate's
  public I/O API); `compact` is an inherent impl (auto-attaches). `TxnRoots`/`Superblock` fields read by
  the commit paths are `pub(crate)`. Verified lossless: the moved bodies are byte-identical to the
  originals.
