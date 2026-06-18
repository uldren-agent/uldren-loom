# uldren-loom-store

The persistent single-file (`.loom`) object store for Uldren Loom: the on-disk `ObjectStore`
backend. Native-only (`std::fs`); loom-core stays wasm-clean and the browser uses the
in-memory store (the `loom-hnsw` pattern). No third-party dependencies (CRC-32C is inline).

`FileStore` implements `loom_core::ObjectStore` (`put`/`get`/`has`/`len`), so it plugs into
`Loom::new(store)` like `MemoryStore`, and passes the same `loom-conformance` vectors. Beyond the
trait it offers `put_batch`: an atomic multi-object commit (one superblock swap, all-or-nothing)
for bulk writers such as import and sync-bundle application.

## Crash-consistency model

A `.loom` file is: two 4 KiB superblock slots (A@0, B@4096) followed by an append-only data
region holding object records and object-index B-tree nodes. The superblock carries a monotonic
`generation`, a `file_logical_end`, the engine-state root, and the object-index B-tree root
(`index_root`), all protected by a CRC-32C; on open, the valid (CRC-ok) slot with the highest
generation wins.

Every mutation (`put`, `put_batch`, `set_reference_root`) goes through one commit path: append all
of the transaction's object records, CoW-insert each `(digest -> offset)` into the index B-tree
(appending fresh path nodes), `fsync`, write the alternate superblock slot (generation+1, new
`logical_end` + `index_root` + reference), `fsync`. The superblock fsync is the commit point. A
batch of N objects therefore commits in a single swap (one generation bump), atomic and far cheaper
than N swaps. Therefore:

- a crash before the superblock fsync leaves the prior superblock authoritative, and everything
  appended beyond the prior `logical_end` (record + new B-tree nodes) is ignored on recovery;
- a crash during the superblock write tears only that slot (its CRC fails); the other slot, at
  `generation-1`, is intact and recovers the prior committed state, including its prior `index_root`
  (CoW never overwrites the old nodes, so the prior tree is fully intact);
- a valid superblock never references unwritten data (data is fsync'd first);
- if the file is shorter than the chosen superblock's `logical_end`, a committed generation was
  truncated away: a clean `CORRUPT` error (no silent fall back to an older generation).

On open, the in-memory `digest -> offset` index is rebuilt by walking the CoW B-tree from
`index_root`: `O(index)` node reads, not an `O(file)` payload scan, and no per-object
rehash. Per-object integrity is still enforced on every `get` by hashing plaintext bytes under the
store's identity profile; each B-tree node and object record additionally carries a CRC-32C.

## What is implemented

A full `Loom` survives restart, the conformance vectors pass, and crash-consistency holds. The store
provides: a crash-consistent object store, reference-root and content-map persistence, the on-disk
CoW B-tree object index, atomic batched commits (`put_batch`, single commit path), and compaction
(`compact`).

**Compaction (`compact`).** Because the index B-tree is append-only copy-on-write, every
`put` leaves the superseded root-to-leaf path as dead nodes. `compact` reclaims them: it rewrites the
live objects into a fresh file with a single bulk-built index (each node written exactly once; a
per-key rebuild would reproduce the churn), then atomically replaces the file via `rename`
(fsync'd; a crash before the rename leaves the original intact and discards the temp). It retains
all stored objects, while `compact_retaining(&live)` drops engine-unreachable objects
(superseded engine-state blobs, abandoned commits) given a live set; `gc_loom` computes that set via
`loom_core::Loom::live_object_set` (reachable closure from refs + tags + the current reference root) and
calls it.

**Write-ahead journal.** A redo log of the committed root-set backs the two-slot superblock: each
commit fsyncs its record into a ring slot, and recovery scans the ring for the newest valid record and
adopts it when it is ahead of the superblock checkpoint. Transaction atomicity comes from the two-slot
superblock: a batch appends all its bytes beyond the committed `logical_end`, then a single swap makes
them visible atomically (a crash before the swap discards the whole batch).

**Per-object compression frames.** Each record carries a frame id: `0x00` identity,
`0x01` DEFLATE (`miniz_oxide`), `0x02` LZ4 (`lz4_flex`); both pure-Rust and `wasm32`-clean. `put`
attempts the store's default codec (`set_default_codec`; default DEFLATE) but only above ~1 KiB and only
if it shrinks the payload, else stores identity; `get` inverts the frame and re-verifies the
plaintext under the store's identity profile. Frames sit below the content-address boundary
(`get`/`put` speak plaintext), so digests, dedup, and sync are frame-independent and two peers may
compress the same object differently. AEAD encryption frames (`0x10`-`0x12`) seal an inner compression
frame, with the digest still over the plaintext, so an encrypted Loom shares object identity with a
plaintext one when both stores use the same identity profile.

The superblock layout: `generation` occupies `[12,20)` and `digest_algo` is at offset 20.

Licensed under BUSL-1.1, which embeds the engine (see the repo `LICENSE`).
