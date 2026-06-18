# 0035 - Durable Delivery

**Status:** Partial implementation, core storage and replay substrate source-backed. **Version:** 0.1.0.
**Capability:** `delivery`.

This spec defines Loom's generic durable delivery component: a server-side delivery outbox and
acknowledgement model for streaming messages whose clients may disconnect and reconnect. It is not a
data facet and not a broker facade. It is infrastructure used by hosted protocols, watch streams,
exec streams, trigger notifications, MCP streams, and any future WebSocket or SSE push surface that
must not lose messages during ordinary connection interruption.

## 1. Current Implementation

Current source has a generic core delivery substrate in `loom_core::delivery`, backed by the 0021a
structured append-log stream storage and ordinary Loom content storage. The reusable `loom-delivery`
crate owns the delivery envelope, canonical envelope codec, replay message shapes, produce request
contract, and component-level delivery tests over `loom-types`. `loom-core` consumes those contracts
for CAS-backed payload storage, queue-backed stream sequencing, subscriber acknowledgement mutation,
authorization checks, and executable engine conformance. It is not yet exposed as a generic wire
transport, ABI surface, binding surface, or hosted push service.

The source-backed generic substrate includes:

- caller-supplied stream ids;
- monotonic per-stream sequences;
- stable idempotency ids derived from canonical envelope identity fields;
- canonical envelopes encoded by `loom-delivery` with producer, subject, payload digest, payload
  length, creation time, optional expiry, and optional producer-owned source cursor;
- CAS-backed payload storage;
- subscriber ack records keyed by stream id and subscriber id through queue consumer offsets;
- replay from explicit sequence or from the subscriber ack cursor;
- redelivery of unacked messages with the same message id;
- authorization before enqueue, replay, and ack through the queue collection PEP;
- executable conformance for redelivery, ack advancement, payload/cursor round-trip, and authorization,
  with reusable envelope codec coverage in `loom-delivery`.

Current source also has a server-lifetime MCP App notification baseline in `loom-mcp`. It is used
behind MCP App resource subscriptions and is separate from the generic core delivery substrate until
hosted transport projection is promoted.

Source-backed adjacent pieces are:

- 0021 append-log streams, which can store ordered opaque entries;
- 0030 watch source-backed pull semantics, which provide workspace change cursors for App wakeups;
- 0008 hosted protocol targets for WebSocket, SSE, JSON-RPC notifications, gRPC streams, and MCP;
- 0026-0028 principal and access-control target specs.

There is no current generic retry scheduler, backpressure policy, retention compactor, wire projection,
ABI, binding projection, hosted transport projection, or process-restart hosted delivery service.

The unfinished generic delivery target remains owned here, not by MCP Apps. The remaining work is:

- retention-gap detection and compaction policy;
- backpressure policy and lease handling;
- hosted WebSocket, SSE, JSON-RPC, gRPC, and MCP transport projections;
- shared conformance for retention gaps, backpressure, process restart, and cross-transport parity.

## 2. Goals and Non-Goals

Goals:

- (P1) preserve messages across transient WebSocket, SSE, or stream interruptions;
- (P1) let clients resume from the last acknowledged message;
- (P0) provide at-least-once delivery with idempotent message ids;
- (P1) support backpressure and bounded retention;
- (P2) provide one shared substrate for multiple producers and transports;
- (P0) enforce authorization before messages enter or leave a delivery stream.

Non-goals:

- (P0) not a Kafka, AMQP, NATS, or general broker replacement;
- (P0) not a cross-tenant pub/sub fabric;
- (P0) not exactly-once execution;
- (P0) not a replacement for 0021 queue consumer offsets;
- (P0) not a replacement for 0030 watch cursors;
- (P0) not a way to bypass ACL by storing authorized data for later unauthorized replay.

## 3. Concepts

| Term | Meaning |
| --- | --- |
| Producer | A Loom subsystem that emits delivery messages, such as watch, exec, trigger, or sync. |
| Delivery stream | A named ordered outbox scoped to a Loom, workspace, principal, session class, or protocol endpoint. |
| Subscriber | A client principal or service that can receive messages from a delivery stream. |
| Delivery cursor | The next delivery sequence number a subscriber should receive. |
| Ack | A durable record that the subscriber processed messages up to a cursor. |
| Lease | A bounded server-side claim that a connection is actively delivering from a stream. |
| Envelope | The canonical wrapper around a message payload. |

The component separates source state from delivery state. A 0030 `DataChange` source cursor still
belongs to 0030. A delivery cursor only records whether a specific subscriber has received the outbound
message that carried that payload. Domain-specific change semantics remain with the producer's owning
spec; 0035 only wraps payloads for transport, retention, replay, acknowledgement, and backpressure.

## 4. Delivery Envelope

Every delivered message is wrapped in a canonical envelope:

```text
DeliveryEnvelope {
  stream_id: string,
  seq: u64,
  id: Digest,
  producer: string,
  subject: string,
  payload_digest: Digest,
  payload_len: u64,
  created_at_ms: u64,
  expires_at_ms: Option<u64>,
  source_cursor: Option<bytes>,
}
```

`seq` is monotonic within `stream_id`. `id` is the digest of the canonical envelope excluding
transport-only fields. `payload_digest` addresses the message payload in Loom content storage.
`source_cursor` is the producer-owned resume position when one exists, such as a 0030 watch cursor.
The payload may be a 0030 `DataChange`, a trigger notification, an exec stream item, or a
control-plane lifecycle payload. 0035 does not require those payloads to share one source event shape.

Clients must treat `id` as an idempotency key. Receiving the same `id` after reconnect is permitted
and must not cause duplicate side effects in a well-behaved client.

## 5. Storage Model

The source-backed core storage model is:

- (P1) delivery streams use the 0021a structured append-log substrate;
- (P0) envelope records are append-log entries keyed by delivery sequence;
- (P0) payload bytes use ordinary Loom content storage;
- (P0) subscriber acknowledgements are small records keyed by `(stream_id, subscriber_id)`;
- (P2) leases are ephemeral and may be reconstructed after process restart from the durable ack record.

Acknowledgements are not part of the delivery stream content. They are mutable subscriber progress
records. A stream may be shared by many subscribers, each with its own ack cursor.

## 6. Delivery Flow

### 6.1 Produce

1. Producer builds or references a payload.
2. Delivery policy selects `stream_id` and eligible subscribers.
3. Access control checks that each subscriber may receive the payload.
4. The envelope is appended to the delivery stream.
5. The append returns the assigned delivery sequence.

Authorization is evaluated before enqueue. If grants change later, replay must also re-check
authorization before sending any retained message.

### 6.2 Connect and Resume

1. Client authenticates through 0008 transport rules.
2. Client subscribes with `stream_id` and either an explicit cursor or `resume_from_ack`.
3. Server loads the durable ack cursor for the subscriber when `resume_from_ack` is requested.
4. Server streams messages from that cursor.
5. Client acknowledges the highest processed sequence.
6. Server durably advances the subscriber ack.

A reconnect resumes from the durable ack cursor. The server may redeliver unacknowledged messages.

### 6.3 Backpressure

Each stream has bounded pending delivery per subscriber. When a subscriber is slow, the server may:

- (P1) stop reading from the producer;
- (P1) stop writing to the transport until ack progress resumes;
- (P1) close the connection with a retryable backpressure error;
- (P2) expire old messages according to retention policy.

The server must never drop unexpired messages silently. If retention expires before a subscriber
acknowledges, resume fails with `CURSOR_INVALID` or a delivery-specific retained-gap error once that
code exists.

The MCP App notification baseline defaults to 24 hours, 10,000 events per stream, and 64 MiB per
stream. Hosts can raise those values, for example to 7 days and 100,000 events per stream, when the
deployment needs enterprise-length reconnect windows and has matching storage budgets.

## 7. Delivery Guarantees

The v1 target guarantee is **durable at-least-once delivery within the retention window**.

| Guarantee | Target |
| --- | --- |
| Ordering | Per delivery stream by `seq`. |
| Redelivery | Allowed after reconnect until ack advances. |
| Exactly once | Not provided. Clients use message `id` for idempotency. |
| Cross-stream order | Not provided. Clients merge streams if needed. |
| Retention | Bounded by stream policy. |
| Authorization | Checked before enqueue and before replay. |
| Disconnect behavior | Resume from durable ack, with possible redelivery. |

## 8. Transport Projection

Durable delivery is transport-neutral.

| Transport | Projection |
| --- | --- |
| WebSocket | Bidirectional subscribe, message, ack, ping, and close frames. |
| SSE | Server-pushed messages; ack uses a separate HTTP request or next reconnect. |
| JSON-RPC WebSocket | `delivery.subscribe`, `delivery.ack`, and `delivery.message` notifications. |
| gRPC | Server-streaming receive with unary or bidirectional ack. |
| MCP | Tool or resource subscriptions use the same cursor and ack model. |

Transport-specific reconnect tokens are not authority. Authentication and authorization are still
provided by 0008 and 0026-0028.

## 9. Interface Sketch

Illustrative IDL:

```text
interface Delivery {
  subscribe(stream_id: string, cursor: Option<u64>): Stream<DeliveryEnvelope>
  ack(stream_id: string, subscriber_id: string, seq: u64): Future<void>
  get_ack(stream_id: string, subscriber_id: string): Future<u64>
  replay(stream_id: string, from: u64, limit: u32): Future<List<DeliveryEnvelope>>
}
```

This interface is a host/control-plane surface. It is not a user data facet. Public exposure may be
limited to hosted server builds and bindings that manage long-lived streams.

## 10. Relationship to Other Specs

- **0008 Wire Protocols:** owns HTTP, WebSocket, JSON-RPC, gRPC, and MCP transport mapping. 0035 owns
  durable delivery semantics used by those transports.
- **0021 Append-Log:** provides the storage substrate for ordered delivery streams.
- **0021a Structured Append-Log Storage:** provides the scalable append and range storage shape needed
  for high-volume delivery outboxes.
- **0026-0028 Identity and ACL:** define subscriber identity and authorization checks.
- **0029 Events and Triggers:** may emit trigger fire notifications through delivery streams.
- **0030 Observability:** produces `DataChange` watch events; 0035 can deliver them reliably over push
  transports without owning their source cursor or domain semantics.
- **0032 Platform Parity:** reports whether a platform supports durable delivery or only pull polling.

## 11. Security and Privacy

Durable delivery can retain sensitive payloads after a client disconnects. Implementations must:

- (P0) scope delivery streams to the minimum principal and audience;
- (P0) re-check authorization before replay;
- (P0) retain payloads only within explicit retention windows;
- (P0) avoid storing bearer tokens, session cookies, or raw credentials in envelopes;
- (P1) support deletion of subscriber ack state and retained messages according to retention policy;
- (P0) ensure a reconnect token cannot replay another principal's stream.

For end-to-end encrypted sync or opaque hosted topologies, delivery payloads must remain encrypted to
the host if the producer's source contract requires that.

## 12. Conformance Requirements

Before marking 0035 fully implemented:

- (P0) a subscriber that disconnects before ack receives the message again after reconnect;
- (P0) a subscriber that acks through `seq = N` resumes at `N + 1`;
- (P0) messages are delivered in sequence order within a stream;
- (P0) duplicate delivery preserves the same message `id`;
- (P0) authorization denial prevents enqueue and replay;
- (P1) retention expiry produces a stable resume failure;
- (P1) slow subscribers trigger backpressure without silent message loss;
- (P2) WebSocket and at least one non-WebSocket transport share the same delivery semantics;
- (P1) conformance covers process restart if the host claims durable delivery across restart.

The first five requirements are source-backed by `run_delivery_behavior`; the remaining requirements
stay target until hosted transport projection and retention/backpressure policy are implemented.

## 13. Resolved Decisions

- **RD1 - Shared component.** Durable delivery is a generic control-plane component, not a data facet.
- **RD2 - Storage substrate.** Delivery streams use 0021/0021a append-log storage.
- **RD3 - Delivery guarantee.** The v1 target is durable at-least-once delivery within retention.
- **RD4 - Ack model.** Acks are per subscriber and durable. Unacked messages may be redelivered.
- **RD5 - Authorization.** Authorization is checked before enqueue and before replay.
- **RD6 - Transport neutrality.** WebSocket is one projection, not the source of truth.
