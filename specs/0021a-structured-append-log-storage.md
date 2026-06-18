# 0021a - Structured Append-Log Storage

**Status:** Implemented storage sub-spec, with target consumer offsets deferred. **Version:** 0.1.0.
**Owns:** The internal storage format needed to make 0021 appends scale without rewriting a whole
stream value.

## 1. Purpose

0021 defines the append-log and queue facet behavior. Current source stores one named stream as a
structured stream root under `/.loom/facets/queue/<name>`, with entry records in a sequence-keyed
prolly map and payload bytes stored through ordinary Loom content storage.

This sub-spec owns that storage format so the public queue facade can remain focused on user-visible
queue semantics. The behavior remains:

- `append(stream, entry)` returns the next zero-based sequence number;
- `get(seq)` returns the entry at that sequence number or absence;
- `range(lo, hi)` is half-open and ordered by sequence number;
- `len(stream)` returns the number of entries;
- (P1) consumer-offset behavior remains separate target work until 0021 promotes it;
- (P3) `dequeue` behavior remains separate target work until 0021 promotes it.

## 2. Current Implementation

Current source implements the structured storage format in `loom-core`:

- `Object::EntryKind::Stream` identifies stream roots in workspace trees;
- `Loom::stream_append` assigns `seq = length`, stores the payload through content storage, inserts an
  entry record into a sequence-keyed prolly map, writes new metadata, and stages a new stream root;
- `Loom::stream_get`, `Loom::stream_range`, and `Loom::stream_len` read by sequence without decoding
  unrelated payloads;
- `loom_core::log::put_stream`, `get_stream`, `append`, `get`, `range`, and `len` delegate to the
  structured storage path;
- clone, bundle export/import, reachability, and GC include the stream root, metadata blob, entry-map
  nodes, payload blobs, and chunk lists;
- ingest rebuilds the derived content-address index for imported `ChunkList` objects before chunked
  stream payloads are read.

Consumer offsets, dequeue semantics, replay or resequence tooling, hosted delivery, wire projection,
bindings, and public queue facade conformance remain target work in 0021 and 0035.

Target priorities:

- (P0) public queue facade conformance;
- (P1) consumer offsets;
- (P2) hosted delivery;
- (P2) wire projection;
- (P2) binding projection;
- (P3) dequeue semantics;
- (P3) replay or resequence tooling.

## 3. Storage Goals

The storage format provides:

- append cost proportional to the new entry plus the touched index path, not total stream size;
- range reads by sequence number without decoding the whole stream;
- deterministic object identity across peers;
- normal workspace commit, checkout, clone, bundle, sync, and GC reachability;
- no automatic divergent-append reconciliation beyond the conflict policy in 0021 and
  `CONFLICT-RESOLUTION-MATRIX.md`;
- no change to public queue semantics.

## 4. Layout

A structured stream is represented by a stream root, an entry map, and optional consumer-offset map.

| Component | Encoding | Required |
| --- | --- | --- |
| Stream root | `Tree` object with stable entry names | Yes |
| Stream metadata | Canonical CBOR blob | Yes |
| Entries | Prolly map keyed by sequence number | Yes, absent only for an empty stream |
| Consumer offsets | Prolly map keyed by consumer id | Target, absent until consumer offsets are implemented |
| Payload content | Existing file-content storage, including chunking for large payloads | Yes |

The stream root is an `Object::Tree` with these entries:

| Entry name | Entry kind | Target |
| --- | --- | --- |
| `meta` | `Blob` | Content address of the stream metadata bytes |
| `entries` | `ProllyMap` | Entry-map root, omitted only for an empty stream |
| `consumers` | `ProllyMap` | Consumer-offset root, omitted until consumer offsets exist |

The workspace tree points at the stream root with `EntryKind::Stream` rather than reusing
`EntryKind::Table` or a plain directory entry. This keeps stream reachability explicit while still
reusing existing object types.

Reusing a plain file blob for the root is insufficient unless reachability walks the root, entry map,
consumer-offset map, and payload content.

## 5. Canonical Keys

Entry-map keys are exactly 8 bytes:

```text
u64_be(seq)
```

Big-endian encoding is required so byte ordering matches numeric sequence ordering in the prolly map.
Sequence numbers start at `0`. The next assigned sequence is `length`.

Consumer-offset keys, once implemented, are:

```text
utf8(consumer_id)
```

`consumer_id` must be non-empty UTF-8 without `/` or NUL. Consumer-offset ordering is lexical byte
ordering of the UTF-8 form.

## 6. Canonical Values

Stream metadata is Loom Canonical CBOR:

```text
[1, length, entries_root_or_null, consumers_root_or_null]
```

- `1` is the structured stream metadata version.
- `length` is a `u64`.
- `entries_root_or_null` is the entry-map root digest, or null when length is `0`.
- `consumers_root_or_null` is the consumer-offset map root digest, or null until consumer offsets are
  implemented.

Each entry-map value is Loom Canonical CBOR:

```text
[1, payload_digest, payload_len]
```

- `1` is the entry-record version.
- `payload_digest` is the file-content digest of the entry bytes under the workspace identity profile.
- `payload_len` is the entry byte length as `u64`.

Payload bytes are stored through the same content path as ordinary files. Payloads at or below the
file chunking threshold may be one Blob; larger payloads use the existing ChunkList and chunk Blob
layout. The stream implementation must retain the content-address to storage-root mapping needed for
reachability, clone, bundle, and GC.

Each consumer-offset value, once implemented, is Loom Canonical CBOR:

```text
[1, next_seq]
```

`next_seq` is the next sequence number the consumer should read.

## 7. Append Algorithm

Given a stream root and entry bytes:

1. Load stream metadata.
2. Assign `seq = length`.
3. Store the entry bytes through the file-content storage path and record `(payload_digest, payload_len)`.
4. Encode the entry-map key as `u64_be(seq)`.
5. Insert the entry record into the prolly map.
6. Write updated metadata with `length + 1` and the new entry-map root.
7. Write a new stream root with `meta`, `entries`, and optional `consumers` entries.
8. Stage the new stream root in the workspace working tree.

The append must not decode or re-encode prior entry payloads. The expected write set for one small
append is the payload object, one entry-map leaf, the prolly spine, the metadata blob, the stream root,
and the workspace tree/commit objects that already change for any workspace write.

## 8. Range Read Algorithm

`range(lo, hi)` opens a prolly range from `u64_be(lo)` to `u64_be(hi)`, exclusive of `hi`, and yields
entries in ascending key order. Each entry record is decoded and its payload bytes are loaded by
`payload_digest`.

The implementation must reject a decoded entry record whose key sequence is greater than or equal to
the stream `length`, whose payload length does not match loaded bytes, or whose payload content digest
does not match `payload_digest`.

## 9. Versioning and Merge Behavior

Structured storage changes physical storage only. It does not change append-log semantics.

Concurrent appends from divergent branch heads are still branch/ref conflicts unless a later explicit
replay or resequence operation is invoked. Sync must not silently interleave two divergent stream
histories.

The sequence allocation rule is single-writer at the stream head: the next sequence is the committed
stream length observed by the writer. A stale writer must fail through the same branch/ref conflict
surface used by 0006 and 0021.

## 10. Migration Boundary

The project has no stable release. The current v1 storage contract is the structured stream format
defined here. The earlier single-value stream encoding is not a v1 compatibility requirement.

Any implementation that changes canonical stream bytes, object reachability, or conformance vectors
must update 0010 and 0025 coverage in the same change.

## 11. Conformance Requirements

Source-backed coverage exists for:

- fixed small and multi-leaf stream root digests;
- structural sharing when appending one entry to a multi-leaf stream;
- append, point get, range, len, commit, checkout, clone, and bundle behavior;
- clone, bundle export/import, and reachability for entry-map nodes and payload content;
- large chunked stream payloads across clone and bundle import.

The remaining conformance work belongs to the public queue facade in 0021 and durable delivery in
0035:

- (P0) public queue facade conformance;
- (P2) durable delivery conformance;
- (P3) divergent append replay or resequence conformance.

Divergent append histories remain unresolved without explicit replay or resequence tooling.

## 12. Resolved Decisions

- **RD1 - Sequence key.** Entry-map keys are `u64` big-endian bytes.
- **RD2 - Payload storage.** Entry payloads use existing file-content storage and chunking, not inline
  prolly values.
- **RD3 - Append semantics.** Structured storage does not change public append, range, len, or conflict
  semantics.
- **RD4 - Consumer offsets.** Consumer offsets are part of the target root shape but are not required for
  the first structured-storage implementation.
- **RD5 - Compatibility.** No stable backwards-compatibility requirement exists before v1 release.
- **RD6 - Workspace entry kind.** Structured streams use a stream-specific workspace entry kind rather
  than masquerading as table, file, or directory entries.
