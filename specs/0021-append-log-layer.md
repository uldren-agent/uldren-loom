# 0021 - Append-Log Layer

**Status:** Partial, public queue facade, consumer offsets, and local ACL hooks source-backed. **Version:** 0.1.0.
**Capability:** `queue`.

This spec defines the append-log and queue facet: versioned ordered streams of opaque entries.
Current source implements the Rust substrate in `loom-core::log` over the structured stream storage
owned by 0021a, plus a language-neutral public queue facade for append, get, range, len, and
consumer offsets through IDL, C ABI, supported bindings, and executable conformance. Dequeue, hosted
wire surface, replay or resequence tooling, observed stream anchors, and served-write authorization
remain target work. The local Rust engine checks queue read/write/advance permissions when ACL is
active.

0021c owns the first-class Kafka-compatible served surface over this queue substrate. Kafka topics map
to queue collections inside one workspace, while producer epochs, group generations, and transaction
state belong to the 0036a coordination substrate.

MQTT and NATS/JetStream are target first-class compatibility surface candidates over the
queue/eventing family. They are not native queue transports, and each requires an owning design before
implementation.

Every operation is scoped to one workspace's queue facet. Cross-workspace queue writes are out of
contract and must fail with `CROSS_WORKSPACE` once a public facade exposes them.

## 1. Current Implementation

`loom-core::log` implements:

- `Stream::new`;
- `len` and `is_empty`;
- `append(entry)`;
- `get(seq)`;
- `range(lo, hi)`;
- `iter()`;
- canonical `encode` and `decode`;
- `put_stream(loom, ns, name, stream)`;
- `get_stream(loom, ns, name)`;
- structured stream root staging, append, get, range, and len through `loom-core::vcs`.

Each append returns the zero-based sequence number. `get` returns absence for out-of-range sequence
numbers. `range(lo, hi)` is half-open and clamps both bounds to the stream length. Entry values are
opaque byte strings.

Source also exposes a minimal queue facade:

- `idl/loom.idl` defines `Queue.append`, `Queue.get`, `Queue.range`, and `Queue.len`;
- the C ABI exposes `loom_queue_append`, `loom_queue_get`, `loom_queue_range`, and `loom_queue_len`;
- Node exposes `queueAppend`, `queueGet`, `queueRange`, and `queueLen`;
- Python exposes `queue_append`, `queue_get`, `queue_range`, and `queue_len`;
- the C++ wrapper exposes queue append, get, len, and raw-CBOR range helpers;
- wasm exposes queue append, get, range, and len on the OPFS SQL store;
- iOS/Swift, JVM, Android, and React Native expose queue append, get, range, and len through their
  binding conventions. React Native projects queue sequence and length values as unsigned 64-bit
  decimal strings because the JS bridge cannot safely carry all `u64` values as numbers.
- the CLI exposes `loom queue append|get|range|len|position|read|advance|reset`.

Source also exposes the 0021b consumer-offset facade:

- `idl/loom.idl` defines `QueueConsumers.consumer_position`, `consumer_read`, `consumer_advance`,
  and `consumer_reset`;
- the C ABI exposes `loom_queue_consumer_position`, `loom_queue_consumer_read`,
  `loom_queue_consumer_advance`, and `loom_queue_consumer_reset`;
- Node, Python, C++, wasm, iOS/Swift, JVM, Android, and React Native expose matching consumer-offset
  helpers; React Native uses unsigned 64-bit decimal strings for positions and `next_seq` arguments;
- consumer offsets persist with local engine state, but are outside stream roots, commits, clone,
  push, bundle export/import, and ordinary sync.

MCP data tools project the local queue and consumer-offset facades through `queue_append`, `queue_get`,
`queue_range`, `queue_len`, `queue_list_streams`, `queue_consumer_position`,
`queue_consumer_read`, `queue_consumer_advance`, and `queue_consumer_reset`. There is no source-backed
non-MCP hosted queue protocol, dequeue operation, observed stream-anchor validation, or replicated
consumer-group profile today.

## 2. Current Storage Shape

The queue facet path is:

```text
/.loom/facets/queue/<name>
```

`put_stream` creates the queue facet directory and stages a structured stream root at that path through
the workspace working tree. `get_stream` reads the structured stream root and decodes the entry records.
Workspace commit, branch, checkout, bundle sync, and clone see the stream as committed content under
the queue facet.

The current committed stream uses a dedicated stream entry kind and a stream root with metadata and a
sequence-keyed entry map. It does not contain consumer-offset, replay, compaction, or merge-metadata
roots. Consumer offsets are operational metadata outside the committed stream tree.

## 3. Current Encoding

The storage encoding is owned by 0021a: stream metadata is canonical CBOR, entry-map keys are
`u64_be(seq)`, and each entry record stores the payload content digest and payload length. Entry
payloads are opaque byte strings stored through ordinary Loom content storage. Current source does not
encode consumer offsets, producer ids, timestamps, delivery state, compaction metadata, or conflict
metadata.

## 4. Current Versioning and Merge Behavior

Current streams version with the workspace because they are written into the workspace working tree. A
commit snapshots the stream root with every other staged workspace path. `checkout_commit` and
`checkout_branch` restore the stream root with the rest of the workspace tree.

Current source implements authority-local consumer offsets outside stream roots and commits. Checkout,
branch movement, clone, push, bundle export/import, and ordinary sync do not move consumer progress.

Current source does not implement stream-specific replay or resequence tooling. If two branches edit
the same stream path differently, the current merge machinery treats it as a normal same-path conflict
unless a promoted queue-specific reconciliation helper exists. Sync follows
`CONFLICT-RESOLUTION-MATRIX.md`: branch/ref divergence uses the S1 fast-forward boundary, and
deterministic replay or resequence is explicit target work.

## 5. Current Conformance

`loom-core::log` has unit tests for:

- append sequence assignment;
- point get;
- clamped half-open ranges;
- canonical encode/decode;
- commit and checkout versioning;
- pinned small and multi-leaf structured stream roots;
- structural sharing after one append;
- clone and bundle import for small and chunked stream payloads.
- missing consumer offset as sequence 0;
- bounded consumer read without advance;
- persisted consumer advance across engine-state reopen;
- rejection of backward or past-tail advance;
- explicit reset for replay;
- checkout, clone, and bundle exclusion for consumer offsets;
- invalid consumer ids and stream names.

`loom-conformance` includes executable `run_queue_behavior` and `run_consumer_behavior`, and aggregate
memory-store certification reports `queue` and `queue-consumer` as executable behavior suites. Current
C ABI tests cover append/get/range/len, workspace selection by UUID, absent get, invalid stream names,
consumer position/read/advance/reset, local reopen persistence, and invalid consumer ids. Node and
Python runtime tests include matching queue assertions. Wasm, iOS/Swift, JVM, Android, and React
Native are covered by binding-suite checks and compile or syntax gates.

## 6. Target Contract

The current minimal public queue facade provides:

- append;
- get by sequence;
- half-open range by sequence;
- length;
- consumer position;
- consumer read without advance;
- monotonic consumer advance;
- administrative consumer reset.

The remaining target queue facade work is:

- optional dequeue semantics;
- explicit replay or resequence tooling for divergent histories;
- observed stream anchors for compare-and-advance;
- explicit replicated consumer-group profile, if a deployment needs cross-authority progress sync.

Before full promotion, the remaining facade needs:

- hosted protocol methods in 0008;
- stable error mapping through `loom_core::error::Code`;
- access-control review for served appends and consumer offset updates;
- clear file-projection behavior for `/.loom/facets/queue/...`;
- concurrent append behavior aligned with `CONFLICT-RESOLUTION-MATRIX.md`.

## 7. Target Storage Contract

The enterprise storage target is the structured stream value owned by 0021a. At this layer, the
required shape is:

| Role | Target encoding | Status |
| --- | --- | --- |
| Entry log | Prolly map keyed by `u64_be(seq)` | Implemented in 0021a |
| Stream metadata | Canonical metadata for length and component roots | Implemented in 0021a |
| Consumer offsets | Operational metadata outside the committed stream tree | Implemented in 0021b |
| Replay metadata | Optional records for explicit reconciliation | Target |
| Stream root | Reachable root referencing stream component roots | Implemented in 0021a |

The sequence allocation rule, initial canonical byte layout, append algorithm, range algorithm, and
conformance requirements are specified in 0021a. Consumer-offset storage and sync boundaries are
specified in 0021b. Replay or resequence tooling remains target work in this spec until its behavior is
promoted.

## 8. Relationship to Other Facets

- **Ledger:** a ledger is the tamper-evident cousin of an append log. Queue keeps ordering and consumer
  semantics separate from hash-chain guarantees.
- **Time-series:** time-series is append-oriented but keyed by timestamp rather than sequence.
- **Compute:** `loom-compute` has a queue capability tag, but queue state access from programs is
  target work until 0015 defines and implements it.
- **Kafka:** 0021c defines the first-class Kafka-compatible served surface. Kafka is a presentation
  over queue collections plus 0036a authority-local coordination state, not a transport under `queue`.
- **MQTT and NATS/JetStream:** MQTT and NATS/JetStream are target first-class compatibility surface
  candidates over queue/eventing primitives. Their topic/session/QoS or subject/consumer/retention
  semantics must not be added to the native queue facade.

## 9. Non-Goals and Limits

- Current source is not a broker.
- Current source provides authority-local at-least-once consumer offsets, not a replicated broker.
- Current source does not provide replay or resequence tooling.
- Current source does not validate observed stream anchors.

## 10. Unfinished Work

| Order | Parent | Work item | Status | Exit criteria |
| --- | --- | --- | --- | --- |
| T1 | RD5 | Spec/source reconciliation | Complete local | Current implementation text distinguishes the implemented local queue and consumer-offset facades plus CLI and MCP data tools from hosted broker work. |
| T2 | RD5 | Queue CLI projection | Complete local | `loom queue ...` commands expose append, get, range, len, and consumer-offset operations with byte-stable output forms. |
| T3 | RD5 | Hosted queue wire projection | Partial source-backed | Non-MCP REST, JSON-RPC, and native gRPC adapters expose append, get, range, and len with ACL behavior and stable errors. Native gRPC protocol conformance proves append, get, range, and len through `hosted-queue-grpc`. Served REST and JSON-RPC protocol conformance prove the same minimal facade through `hosted-queue-rest` and `hosted-queue-jsonrpc`. Consumer-offset hosted gRPC, pagination, observed anchors, and consumer-offset protocol conformance remain target work. |
| T4 | RD5 | Observed stream anchors | Target | Compare-and-advance anchors prevent stale served appends or consumer updates where hosted clients require optimistic concurrency. |
| T5 | RD6 | Replay, resequence, and replicated consumer groups | Target | Divergent stream reconciliation and optional replicated consumer progress are explicit, deterministic, and covered by conformance. |
| T6 | RD9 | Kafka compatibility surface | Target | 0021c defines workspace-scoped Kafka serving with topics as queue collections, partition metadata, offset commits, producer epochs, transactions, and single-node capability reporting. |

## 11. Resolved Decisions

- **RD1 - Current storage.** Current source stores each named stream as a structured stream root under
  the workspace queue facet.
- **RD2 - Current sequence.** Sequence numbers are zero-based array positions assigned by append.
- **RD3 - Current payload.** Stream entries are opaque bytes.
- **RD4 - Consumer offsets.** Consumer offsets are operational metadata outside current stream content.
- **RD5 - Public facade status.** The minimal public `queue` facade for append, get, range, and len is
  source-backed across the local projection surfaces. Consumer offsets are source-backed by 0021b.
  `dequeue` is target work.
- **RD6 - Merge boundary.** Concurrent append reconciliation is explicit target work. Sync does not
  silently resequence divergent stream histories.
- **RD7 - Structured storage ownership.** 0021a owns the implemented storage format that avoids
  rewriting the full logical stream value without changing append-log semantics.
- **RD8 - Consumer offset ownership.** 0021b owns authority-local consumer progress outside the
  committed stream tree. Ordinary Loom sync does not transfer consumer offsets by default.
- **RD9 - Kafka surface ownership.** Kafka compatibility is first-class served surface work owned by
  0021c. Native queue remains the source of truth; Kafka-specific coordination state is authority-local
  operational metadata through 0036a.
- **RD10 - Eventing compatibility candidates.** MQTT and NATS/JetStream are first-class target
  compatibility surface candidates, not `queue` transports. They need owning designs before build.
