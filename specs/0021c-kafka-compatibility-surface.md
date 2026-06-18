# 0021c - Kafka Compatibility Surface

**Status:** Partial, record-batch produce/fetch/offset commit, idempotent sequence validation, and
bounded transaction control source-backed. **Version:** 0.1.0.
**Depends on:** 0021, 0021a, 0021b, 0036a, 0008.
**Owns:** The first-class `kafka` served surface over Loom queue collections.

This spec defines the Kafka-compatible served surface for Loom. It does not redefine the native queue
facet. The native source of truth remains the versioned queue collection and its structured stream
storage; Kafka compatibility is a hosted presentation over that substrate plus authority-local
coordination state.

## Current Implementation

The current tree source-backs the authenticated topic-lifecycle, record-batch, and bounded
producer/transaction-control slice:

- `kafka-protocol` 0.17.0 is the selected Kafka codec dependency with default compression disabled
  and broker/client protocol features enabled.
- `loom serve configure <store> kafka <workspace> --bind <addr>` is admitted as durable listener
  intent through the served registry.
- `loom daemon` opens enabled `kafka/tcp` listeners.
- The hosted listener handles ApiVersions, SaslHandshake, SaslAuthenticate, Metadata, CreateTopics,
  DeleteTopics, Produce, Fetch, OffsetCommit, InitProducerId, AddPartitionsToTxn, AddOffsetsToTxn,
  EndTxn, and TxnOffsetCommit requests.
- ApiVersions reports only those currently implemented APIs.
- SASL PLAIN authenticates against hosted passphrase principals and app credential tokens through the
  hosted kernel before any queue-backed topic operation can expose or mutate state.
- Metadata returns workspace-scoped, single-broker topic metadata for authenticated clients. Requested
  topics resolve through durable Kafka topic metadata records and expose the source-backed partition,
  leader, leader epoch, replica, ISR, and topic UUID fields.
- CreateTopics creates a queue stream for the requested topic when the authenticated principal can
  write to the workspace, then persists Kafka topic metadata under the queue facet. Validate-only
  requests check the request without mutating queue state.
- DeleteTopics removes the queue stream and the Kafka topic metadata record when the authenticated
  principal can read and write the workspace.
- Topic metadata records are versioned and include the Kafka-visible topic name, stable topic UUID,
  creation time, metadata version, and partition metadata. The current runtime source-backs one
  partition per topic.
- Kafka topic metadata versions are allocated from a shared durable 0036a metadata-version high-water
  record scoped to the Kafka workspace, so future group metadata can use the same workspace-local
  allocation source.
- Produce decodes Kafka record-batch v2 bytes after hosted authentication and authorization,
  normalizes Kafka-visible offsets to the assigned queue offsets, stores one encoded record batch per
  record, and returns the assigned base offset.
- Fetch reads queue entries from the requested offset, concatenates stored record-batch bytes up to
  the requested byte bound, and returns the topic high-watermark.
- OffsetCommit validates the committed offset against the partition high-watermark and persists the
  group/partition progress through queue consumer offsets.
- InitProducerId allocates durable producer ids and epochs for transactional ids and fences stale
  producer epochs.
- AddPartitionsToTxn validates the current transactional producer epoch and records active
  transaction topic/partition participants under queue-facet Kafka metadata.
- AddOffsetsToTxn validates the current transactional producer epoch and records active transaction
  group participants.
- EndTxn validates the current transactional producer epoch and records commit or abort terminal
  transaction state.
- TxnOffsetCommit now buffers valid pending offsets into the active transaction and applies them only
  on EndTxn commit. EndTxn abort leaves those pending offsets unapplied.
- Non-transactional idempotent Produce validates producer id, producer epoch, per-partition sequence
  continuity, and exact duplicate retries before append.
- Transactional Produce validates producer id, producer epoch, active transaction membership, and
  per-partition sequence continuity before append. Read-committed Fetch hides active and aborted
  transactional produced records, reports last-stable-offset from active transaction ranges, and
  reports aborted transaction markers for aborted ranges. Read-uncommitted Fetch returns appended
  transactional records.
- Conformance inventory rows distinguish supported Kafka ApiVersions/auth/topic lifecycle behavior,
  supported record-batch produce/fetch/offset-commit behavior, supported offset normalization,
  supported producer-id/epoch fencing, supported bounded transaction control, degraded single-node
  single-partition metadata, supported shared durable metadata-version allocation, supported
  idempotent sequence validation, supported transactional offset visibility, supported
  transactional produced-record visibility, target groups, and unsupported multi-broker replication,
  ISR, and election behavior.

The implementation does not yet source-back multi-partition topics, older message-set versions,
consumer-group membership or rebalance, stale generation checks, AddPartitionsToTxn v4/v5
multi-transaction batches, transaction timeout enforcement, or generalized durable 0036a high-water
persistence beyond the Kafka workspace metadata-version allocator. Those remain Queue 2 subtasks and
must not be claimed as implemented capability.

## 1. Command Shape

Operator form:

```text
loom serve configure <store> kafka <workspace> --bind 127.0.0.1:9092
```

Direct compatibility form:

```text
loom serve <store> kafka <workspace> --bind 127.0.0.1:9092
```

The `kafka` surface has one selector: the Loom workspace. Kafka topics are resolved inside that
workspace as queue collections. The listener does not bind one topic or partition.

The only admitted transport for the first surface contract is `tcp`. The transport is implicit because
Kafka has one obvious client-facing carrier in the current target.

## 2. Storage And Selector Mapping

| Kafka concept | Loom mapping |
| --- | --- |
| Listener | One served listener scoped to one Loom workspace. |
| Topic | One queue collection inside that workspace. |
| Partition | Partition metadata under the topic collection. |
| Record | Queue entry payload plus Kafka record metadata. |
| Offset | Monotonic partition-local sequence. |
| Consumer group | Authority-local operational group state. |
| Committed consumer offset | Authority-local operational progress outside the queue stream root. |
| Producer id and epoch | 0036a producer coordinator record. |
| Transactional id | 0036a transaction coordinator record scoped to the Kafka workspace. |

The base queue collection path remains owned by 0021 and 0021a. Kafka metadata that affects client
behavior but is not queue content, such as producer epochs, group generations, and transaction state,
belongs to 0036a authority-local operational metadata.

Kafka record headers, keys, timestamps, and attributes are presentation metadata. The first
implementation may store them in Kafka-specific record envelopes inside queue payload bytes, or in a
parallel metadata map, but it must preserve deterministic readback for Kafka clients and must not
change the native queue append/get/range/len contract.

The current implementation parses Kafka record-batch v2 bytes, rejects invalid opaque bytes, rewrites
visible offsets to the assigned queue offsets, and stores one encoded batch per record. It preserves
keys, values, headers, timestamps, and batch metadata that the selected codec exposes. The workspace
dependency enables Kafka gzip, Snappy, LZ4, and Zstandard record compression.

The current implementation accepts non-transactional idempotent record batches after validating the
producer id, producer epoch, per-partition sequence continuity, and exact duplicate retry shape. It
also accepts transactional record batches when the request has a transactional id, the producer epoch
is current, the partition is registered in the active transaction, and the sequence is contiguous.

## 3. Topic And Partition Model

Topic names map to queue collection names after validation. A topic name must be a non-empty UTF-8
string accepted by both the Kafka profile and Loom collection naming rules. Names that would escape
the queue facet path, collide with reserved Loom names, or normalize ambiguously must be rejected.

Each topic has explicit partition metadata. The enterprise target is multi-partition from the first
contract, even if the first implementation only creates one partition by default.

Required topic metadata:

| Field | Meaning |
| --- | --- |
| `topic` | Kafka-visible topic name. |
| `topic_id` | Stable internal id for metadata references. |
| `partitions` | Ordered partition ids starting at 0. |
| `created_at_ms` | Authority-local creation time for metadata and audit. |
| `metadata_version` | Monotonic version from the Kafka workspace-scoped 0036a metadata-version high-water allocator. |

Required partition metadata:

| Field | Meaning |
| --- | --- |
| `partition` | Zero-based partition id. |
| `next_offset` | Next offset allocated by 0036a for this partition. |
| `leader_epoch` | Single-node epoch for client metadata and fencing. |
| `high_watermark` | Highest committed offset visible to fetch. |

The first implementation may use one broker and one replica. It must still expose topic and partition
metadata in a way that can grow to a distributed backend without changing stored queue content.

## 4. Produce And Fetch Semantics

Produce:

- authenticates and authorizes before mutation;
- validates topic, partition, producer id, and producer epoch when provided;
- allocates offsets monotonically per `(workspace, topic, partition)`;
- appends records in offset order;
- returns assigned offsets to the client;
- fails stale producer epochs before appending.

Fetch:

- authenticates and authorizes before read;
- reads from one or more topic partitions by offset;
- returns a bounded batch;
- does not advance committed consumer progress;
- reports high-watermark and end-offset values from partition metadata.

The Kafka surface must preserve at-least-once behavior. A fetch never advances a consumer-group offset.
Offset commits are explicit.

## 5. Consumer Groups

Consumer-group state is authority-local operational metadata. It is not stored inside the versioned
queue collection and is not transferred by ordinary Loom sync.

Required consumer-group state:

| Field | Meaning |
| --- | --- |
| `group_id` | Kafka-visible group id. |
| `generation` | Monotonic group generation from 0036a. |
| `members` | Active members and assigned topic partitions. |
| `committed_offsets` | Per-topic-partition committed offsets. |
| `metadata_version` | Monotonic version for group metadata. |

The first implementation may use a single-node assignment strategy. It must not claim full clustered
rebalance compatibility. A stale generation or stale member epoch must fail rather than silently moving
committed offsets.

## 6. Producer Epochs And Idempotent Produce

Producer state is owned by 0036a.

The current implementation source-backs producer id allocation, stale producer epoch fencing,
per-producer per-partition sequence validation, and exact duplicate retry recognition for
non-transactional idempotent Produce. Transactional produced records use the same source-backed
producer epoch and sequence validation before append.

If idempotent produce is not implemented in the first runtime slice, capability reporting must mark it
unsupported. The wire response must be a clear unsupported or invalid-request response, not silent
downgrade to non-idempotent behavior when the client requested idempotence.

## 7. Transactions

Transaction state is owned by 0036a.

The current implementation source-backs transactional id registration through InitProducerId, active
transaction participant records for AddPartitionsToTxn and AddOffsetsToTxn, producer epoch fencing,
pending offset records through TxnOffsetCommit, and EndTxn terminal commit or abort state. On commit,
pending transactional offsets are applied; on abort, they are discarded. Transactional produced
records append to the queue log with a durable transaction-produced range attached to the active
transaction record. Read-committed Fetch hides active and aborted ranges, read-uncommitted Fetch sees
the appended bytes, committed transaction ranges become visible after EndTxn commit, and aborted
transaction ranges remain hidden from read-committed Fetch.

The surface must not claim clustered exactly-once producer transaction support. The current
implementation is a bounded single-node visibility contract over durable Loom state.

## 8. Authorization And Auditing

The Kafka surface uses the hosted kernel and PEP. Authorization is never transport-local.

Minimum authorization mapping:

| Kafka operation class | Required Loom right |
| --- | --- |
| Topic metadata read | workspace or queue read. |
| Produce | queue write on the topic collection. |
| Fetch | queue read on the topic collection. |
| Offset commit | queue advance on the topic collection or group scope. |
| Topic create/delete | administrative or collection-management right. |
| Transaction control | write plus coordination authority for the transactional scope. |

Auditing must distinguish:

- authentication failure;
- authorization denial;
- topic create/delete;
- produce batch;
- fetch batch when configured for read audit;
- offset commit;
- transaction begin, commit, abort, and timeout when implemented;
- unsupported feature requests.

## 9. Capability Reporting

The `kafka` listener must report capability rows separately from native `queue`.

Required capability dimensions:

| Capability | Initial status |
| --- | --- |
| ApiVersions and SASL PLAIN auth | supported |
| single-node broker metadata | supported |
| topic create/list/delete | supported |
| durable topic metadata | supported |
| single-node single-partition metadata | degraded |
| multi-partition topics | target |
| record-batch produce/fetch/offset commit | supported |
| normalized record-batch offsets | supported |
| compressed record batches | supported |
| consumer groups | target |
| shared durable metadata-version allocation | supported |
| producer id and epoch fencing | supported |
| bounded transaction control | supported |
| idempotent produce sequence validation | supported |
| transactional offset atomic visibility | supported |
| transactional produced-record visibility | supported |
| multi-broker replication | unsupported |
| ISR and broker election | unsupported |
| clustered rebalancing | unsupported until distributed coordination exists |

## 10. Runtime Support Boundary

Admission of a `kafka/tcp` served listener is durable intent. It is not proof of daemon runtime support.

Runtime support requires:

- a Kafka protocol codec or parser;
- a daemon opener for `kafka/tcp`;
- hosted kernel integration for store open/save, auth, PEP, audit, and stable errors;
- a 0036a coordinator instance;
- topic and partition metadata persistence;
- guarded or real-client transcript tests where practical;
- conformance and capability report rows.

## 11. Non-Goals

- no Kafka cluster replacement claim in the single-node implementation;
- no multi-broker replication or ISR model without a distributed coordinator backend;
- no silent fallback from requested idempotent or transactional behavior to ordinary append;
- no queue storage mutation from 0036a;
- no ordinary Loom sync of Kafka consumer-group progress, producer state, or transaction state.

## 12. Resolved Decisions

- **RD1 - First-class surface.** Kafka compatibility is the `kafka` served surface, not
  `queue --transport kafka`.
- **RD2 - Workspace-scoped listener.** The listener binds one Loom workspace. Kafka topics map to queue
  collections inside that workspace.
- **RD3 - Multi-partition target.** The contract reserves topic partition metadata from the start. A
  one-partition first runtime does not freeze a one-partition public model.
- **RD4 - Coordination ownership.** Producer epochs, group generations, sequencers, and transaction
  state belong to 0036a `loom-coordination`.
- **RD5 - Honest single-node support.** Single-node Kafka compatibility is valuable, but
  multi-broker cluster behavior remains unsupported until a distributed coordination backend exists.
