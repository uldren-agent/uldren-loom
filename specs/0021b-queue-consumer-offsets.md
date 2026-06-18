# 0021b - Queue Consumer Offsets

**Status:** Implemented sub-spec, with observed-anchor validation still target. **Version:** 0.1.0.
**Owns:** Durable consumer progress for the 0021 queue facade.

## 1. Purpose

0021 now has a source-backed minimal queue facade for append, get, range, and len. This sub-spec owns
the next queue slice: durable consumer offsets and the safe building blocks for later `dequeue`.

Queue entries and consumer progress are separate contracts:

- queue entries are committed workspace content under the queue facet;
- consumer offsets are operational metadata for a specific consumer group or worker;
- ordinary branch, checkout, merge, and sync operations must not silently move consumer progress.

## 2. Current Implementation

Source implements consumer offsets without the observed stream anchors. Offsets are operational
metadata in the local Loom store, keyed by `(workspace_id, stream, consumer_id)`, with a missing offset
reading as `next_seq = 0`.

- `loom-core::log` and `loom-core::vcs` implement structured streams and the consumer-offset store;
- core exposes `consumer_position`, `consumer_read`, `consumer_advance`, and `consumer_reset`;
- IDL, C ABI, Node, Python, C++, wasm, iOS/Swift, JVM, Android, React Native, and conformance expose
  the consumer-offset operations; React Native carries positions and `next_seq` arguments as unsigned
  64-bit decimal strings;
- `consumer_read` reads from the stored `next_seq` and does not advance; `consumer_advance` is monotonic;
  `consumer_reset` may move backward;
- offsets persist with the local engine state but are excluded from commits, stream roots, clone, push,
  bundle export/import, and ordinary sync;
- checkout and branch movement do not mutate stored consumer progress.

The `observed_stream_root` and `observed_commit` compare-and-advance anchors in sections 3 and 5 are
target work and are not part of the current API. They will be added with the anchor-validation
`CONFLICT` mapping in a later change.

## 3. Model

A consumer offset records the next sequence a named consumer should read from one stream.

```text
QueueConsumerOffset {
  workspace_id: Uuid,
  stream: string,
  consumer_id: string,
  next_seq: u64,
  observed_stream_root: optional Digest,
  observed_commit: optional Digest,
  updated_at_ms: u64
}
```

`next_seq` is the next entry to deliver to that consumer. A missing offset is equivalent to `next_seq =
0`.

`observed_stream_root` and `observed_commit` are validation anchors. They let the engine detect that a
consumer is advancing against a different stream view than the one it read. These anchors are not
authority by themselves; they are safety evidence for compare-and-advance.

## 4. Storage Decision

Consumer offsets are stored in a separate operational metadata store, outside the committed workspace
tree and outside the structured stream root.

Required properties:

- persisted with the local Loom store;
- keyed by `(workspace_id, stream, consumer_id)`;
- not affected by checkout, branch creation, merge, or tag movement;
- not transferred by ordinary Loom sync;
- optional export/import only through explicit operational-backup tooling;
- updateable without creating queue-content commits.

This avoids two failure modes:

- committed offsets can roll backward on checkout or branch movement;
- synced offsets can silently skip messages when one replica advances farther than another.

## 5. API Shape

Source-backed interface:

```text
interface QueueConsumers {
  consumer_position(workspace: string, stream: string, consumer_id: string): u64
  consumer_read(workspace: string, stream: string, consumer_id: string, max: u32): list<bytes>
  consumer_advance(workspace: string, stream: string, consumer_id: string, next_seq: u64): void
  consumer_reset(workspace: string, stream: string, consumer_id: string, next_seq: u64): void
}
```

`consumer_read` does not advance progress. It reads from the current stored `next_seq` and returns a
bounded batch. The caller processes the entries and then calls `consumer_advance`.

`consumer_advance` is monotonic by default. It rejects attempts to move `next_seq` backward. Backward
movement requires `consumer_reset`, which is an explicit administrative operation.

The target compare-and-advance extension will add observed stream anchors. Until that is implemented,
callers advance by explicit `next_seq` only.

## 6. Delivery Semantics

The target guarantee is at-least-once delivery.

Allowed:

- a consumer may see the same entry again after a crash;
- a consumer may reread entries after restoring from backup;
- a consumer may explicitly reset progress to replay.

Not allowed:

- automatic skip of unacknowledged entries;
- advancing offset during read;
- implicit progress movement caused by checkout, merge, branch, or ordinary sync.

At-least-once delivery makes duplicate processing possible. Clients that perform side effects should
use idempotency keys derived from `(workspace_id, stream, seq)` or an application payload id.

## 7. Sync Boundary

Consumer offsets are authority-local by default. Ordinary Loom sync transfers queue content, refs, and
objects according to 0006, but it does not transfer queue consumer offsets.

If a deployment needs replicated consumer-group offsets, that is a later explicit profile. It must
define:

- (P0) which authority owns the consumer group;
- (P0) whether max, min, conflict, or lease-based resolution is used;
- (P0) how message loss is prevented;
- (P0) how stale consumers are fenced;
- (P0) how conformance proves crash, restart, and divergent-replica behavior.

Until that profile exists, consumers on different authorities may double-deliver entries after sync.
That is preferred over silently skipping entries.

## 8. Validation Rules

Implementations must validate:

- (P0) `consumer_id` is non-empty UTF-8 without `/`, NUL, or control characters;
- (P0) `stream` follows the public queue stream-name rules from 0021;
- (P0) `next_seq` is not greater than the current stream length unless an explicit tail-position operation
  permits it;
- (P0) `consumer_advance` does not move backward;
- (P1) optional observed stream anchors, when supplied, match the stream view that produced the read or
  fail with a stable conflict-style error.

The exact stable error-code mapping must be selected before implementation. Existing likely mappings:

- (P0) invalid ids and sequence overflows: `INVALID_ARGUMENT`;
- (P0) unknown workspace or stream: `NOT_FOUND`;
- (P1) stale observed stream anchors: `CONFLICT`;
- (P0) source-backed local Rust behavior: unauthorized consumer position/read/advance/reset once ACL is
  active returns `PERMISSION_DENIED`; binding and hosted protocol parity remain target.

## 9. Relationship to 0035 Durable Delivery

Queue consumer offsets are data-facet operational metadata. 0035 delivery acknowledgements are
transport/control-plane progress records. They are similar, but they are not the same API:

- 0021b tracks application consumers reading queue streams;
- 0035 tracks subscribers receiving hosted delivery envelopes;
- both use explicit ack/advance and at-least-once semantics;
- neither should be moved by ordinary branch checkout.

Implementation may share an internal progress-record helper later, but the public contracts remain
separate.

## 10. Conformance Requirements

Implemented conformance covers:

- missing consumer offset reads as `next_seq = 0`;
- `consumer_read` returns a bounded batch from `next_seq` and does not advance progress;
- `consumer_advance` persists progress;
- crash or reopen preserves progress in the local store;
- duplicate delivery occurs after read-without-advance;
- backward advance is rejected;
- explicit reset can move backward when authorized;
- checkout and branch movement do not mutate stored consumer progress;
- ordinary sync does not transfer consumer progress;
- invalid consumer ids and stream names are rejected;
- ABI, binding, and conformance projections preserve the same semantics.

Remaining target conformance:

- (P1) stale observed stream anchors are rejected once anchor validation is implemented.

## 11. Resolved Decisions

- **RD1 - Offset storage.** Consumer offsets are operational metadata outside the committed stream tree.
- **RD2 - Advance model.** Reads do not advance offsets. Progress advances only through explicit
  `consumer_advance` or administrative reset.
- **RD3 - Delivery guarantee.** The queue consumer contract is at-least-once, not exactly-once.
- **RD4 - Sync default.** Ordinary Loom sync does not transfer consumer offsets.
- **RD5 - Replay.** Duplicate delivery is acceptable; silent message loss is not.
