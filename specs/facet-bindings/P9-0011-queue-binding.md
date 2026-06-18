# P9-0011 - `queue` Binding

**Series:** P9 binding plan (normative-track sub-series; draft)
**Version:** 0.1.0-draft. **Status:** Draft. **Last updated:** 2026-07-02.
**Reads first:** `P9-0002-projection-conventions.md`, 0021, 0021a, 0021b, 0021c, and
`IMPLEMENTATION-FIDELITY.md`.

## 1. Facade Surface

Current source-backed minimal queue facade:

```text
append(workspace, stream, entry: bytes) -> u64
get(workspace, stream, seq: u64) -> optional bytes
range(workspace, stream, lo: u64, hi: u64) -> list<bytes>
len(workspace, stream) -> u64
```

Source-backed consumer-offset surface is owned by 0021b:

```text
consumer_position(workspace, stream, consumer_id) -> u64
consumer_read(workspace, stream, consumer_id, max) -> list<bytes>
consumer_advance(workspace, stream, consumer_id, next_seq)
consumer_reset(workspace, stream, consumer_id, next_seq)
```

`dequeue(stream, consumer)` remains target work and must not be advertised until a safe convenience
contract is defined over explicit read and advance.

## 2. Build Status

Source-backed today:

- IDL, C ABI, Node, Python, C++, wasm, iOS/Swift, JVM, Android, and React Native expose append, get,
  range, and len. React Native projects sequence and length values as unsigned 64-bit decimal strings;
- IDL, C ABI, Node, Python, C++, wasm, iOS/Swift, JVM, Android, and React Native expose consumer
  position, read, advance, and reset. React Native projects positions and `next_seq` arguments as
  unsigned 64-bit decimal strings;
- `loom-conformance` has executable `run_queue_behavior` and `run_consumer_behavior`;
- structured queue storage is implemented by 0021a;
- stream names are validated by the public bindings;
- consumer offsets persist locally and are not transferred by ordinary sync.
- hosted `queue/rest` and `queue/json_rpc` listeners expose append, get, range, and len through the
  hosted data adapter and daemon-opened served listener runtime.

Target:

- hosted projection for 0021b consumer-offset APIs;
- hosted gRPC projection for append, get, range, and len;
- observed stream anchors for compare-and-advance;
- `dequeue` only after explicit read and explicit advance semantics are source-backed.

### 2.1 Binding Boundary

The base layer is a Loom append log with explicit consumer offset metadata. Native projections expose
append, get, range, len, consumer position, consumer read, consumer advance, and consumer reset.
Kafka, MQTT, AMQP, and NATS/JetStream-like surfaces are presentations and must not redefine canonical
ordering or sync semantics. JSONL exports and broker log files are interchange or reference formats.
Consumer-group coordination, delivery cursors, broker sessions, and compatibility offsets are
operational state unless explicitly made syncable.

0021c owns the Kafka-compatible served surface. This binding spec records projection posture only.

## 3. REST Projection

Current source has a listener-bound hosted REST facade for configured `queue/rest` listeners. The
listener selectors bind `{workspace, stream}`, and the current routes are `POST /queue:append`,
`POST /queue:get`, `POST /queue:len`, and `POST /queue:range` with JSON bodies. Entry bytes are
carried as `payload_hex`.

The resource-oriented target shape remains:

Facet-root:

```text
/v1/workspaces/{workspace_id}/streams
```

| Facade method | HTTP |
| --- | --- |
| `append` | `POST /streams/{stream}/entries` with bytes body -> `201` plus `{ "seq": u64 }` |
| `get` | `GET /streams/{stream}/entries/{seq}` -> bytes or `404` |
| `range` | `GET /streams/{stream}/entries?lo={lo}&hi={hi}` -> JSON or binary batch profile |
| `len` | `GET /streams/{stream}/length` -> `{ "len": u64 }` |
| `consumer_position` | target hosted projection |
| `consumer_read` | target hosted projection |
| `consumer_advance` | target hosted projection |
| `consumer_reset` | target hosted projection |

`range` over an immutable sequence range is cacheable when the response identifies the stream root or
commit used for the read.

## 4. JSON-RPC Projection

Current source-backed methods over `queue/json_rpc` listeners:

```text
queue.append
queue.get
queue.range
queue.len
```

Consumer methods remain target for hosted projection:

```text
queue.consumerPosition
queue.consumerRead
queue.consumerAdvance
queue.consumerReset
```

`queue.dequeue` is not part of the v1 projection unless it is later defined as a safe convenience over
explicit `consumer_read` plus `consumer_advance`.

## 5. gRPC Projection

Current source has native Loom `queue/grpc` listener support through `loom.hosted.v1.Queue`.

Unary source-backed methods:

- `Append`;
- `Get`;
- `Range`;
- `Len`.

Consumer offset methods should project as unary methods, but they remain target work. A long-lived
tail/follow stream should use 0035 durable delivery rather than inventing queue-specific reconnect
semantics.

## 6. MCP Projection

Read tools:

- `queue.get`;
- `queue.range`;
- `queue.len`.

Write tools:

- `queue.append`;
- `queue.consumerAdvance`;
- `queue.consumerReset`, with admin authorization for backward movement.

Tools that mutate queue content or consumer progress require write or advance authorization once
0027/0028 policy exists.

## 7. Kafka-Compatible Surface

Kafka compatibility is a first-class served surface, not a transport under `queue`. 0021c is the
owning contract for workspace-scoped Kafka serving, topic and partition metadata, producer epochs,
consumer groups, offset commits, transaction state, and capability reporting.

Command shape:

```text
loom serve configure <store> kafka <workspace> --bind 127.0.0.1:9092
```

Projection requirements:

| Requirement | Owner |
| --- | --- |
| Native queue append/get/range/len remains unchanged | 0021 |
| Consumer offset storage remains authority-local | 0021b |
| Kafka topic, partition, producer, group, and transaction mapping | 0021c |
| Producer epochs, group generations, sequencers, and transactions | 0036a |
| Served listener admission and runtime posture | 0008 |
| Hosted auth, PEP, stable errors, and audit | 0008, 0026, 0027, 0028 |

Single-node compatibility is valuable and should be implemented honestly. The surface must not report
multi-broker replication, ISR, broker election, or clustered rebalance compatibility until a
distributed coordination backend is source-backed.

## 8. MQTT And NATS/JetStream Candidate Surfaces

MQTT and NATS/JetStream compatibility are target first-class served surface candidates, not transports
under `queue`.

Candidate command shapes:

```text
loom serve configure <store> mqtt <workspace> --bind 127.0.0.1:1883
loom serve configure <store> nats <workspace> --bind 127.0.0.1:4222
```

MQTT must have an owning design before implementation. The design must resolve at least:

- topic filter mapping to Loom collections or virtual topic spaces;
- QoS 0, QoS 1, and QoS 2 support level;
- retained messages and session persistence;
- will messages, clean-session behavior, and authentication;
- whether MQTT state is durable Loom state, authority-local operational state, or explicitly volatile.

NATS and JetStream must have an owning design before implementation. The design must resolve at least:

- whether NATS core and JetStream are one `nats` surface with profiles or separate first-class
  surfaces;
- subject mapping and wildcard behavior;
- queue groups, request/reply, and fanout semantics;
- JetStream stream and durable-consumer lifecycle;
- acknowledgement, replay, retention, and exactly-once or at-least-once guarantees;
- which state is durable Loom state and which state is authority-local operational metadata.

These options are product/domain semantics. They are intentionally not added to the native `queue`
surface or modeled as generic `--transport` options.

## 9. Errors, Parity, and Concurrency

- Invalid stream or consumer ids map to `INVALID_ARGUMENT`.
- Missing workspace, stream, or entry maps to `NOT_FOUND`.
- Stale observed stream anchors map to a conflict-style error.
- Served append and offset advance require authorization once 0027/0028 are source-backed.
- Queue storage is portable across native and web targets.
- The Kafka-compatible surface is a hosted/native projection and not the source of truth.
- Kafka stale producer epochs, stale transaction epochs, stale consumer-group generations, and stale
  fences map to stable conflict or fencing-style errors selected by 0036a and 0008.
- Cross-peer concurrent append behavior follows 0021 and `CONFLICT-RESOLUTION-MATRIX.md`.
- Consumer offsets are authority-local operational metadata by default and are not transferred by
  ordinary Loom sync.

## 10. Resolved Decisions

- **RD1 - Minimal facade.** Append, get, range, and len are the source-backed public queue surface.
- **RD2 - Offset storage.** Consumer offsets are stored as operational metadata outside the committed
  stream tree, per 0021b.
- **RD3 - Advance model.** Reads do not advance offsets. Progress advances only through explicit
  `consumer_advance` or administrative reset.
- **RD4 - Sync default.** Ordinary Loom sync does not transfer consumer offsets.
- **RD5 - Delivery guarantee.** Queue consumers are at-least-once by default. Duplicate delivery is
  acceptable; silent loss is not.
- **RD6 - Kafka surface.** Kafka compatibility is first-class served surface work owned by 0021c, not
  a `queue` transport. It uses 0036a for producer epochs, sequencing, group generations, and
  transaction state.
- **RD7 - MQTT and NATS/JetStream candidates.** MQTT and NATS/JetStream are first-class compatibility
  surface candidates over queue/eventing primitives, not native queue transports. They require
  dedicated owning designs before implementation.
