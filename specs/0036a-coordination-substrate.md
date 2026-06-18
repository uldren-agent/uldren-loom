# 0036a - Coordination Substrate

**Status:** Partial, initial crate and single-node primitives source-backed. **Version:** 0.1.0.
**Depends on:** 0005, 0021, 0021b, 0036.

This spec defines the `loom-coordination` crate boundary: a reusable coordination substrate for
single-node Loom runtime features today and clustered control-plane backends in a future target. It
extends 0036 without changing the public lock and fencing contract.

## 1. Purpose

Some served compatibility surfaces need more than a mutex. Kafka is the first driver:

- producer identifiers and producer epochs;
- stale producer fencing;
- monotonic metadata versions;
- partition offset sequencing;
- consumer-group generation and member epochs;
- transaction state for idempotent produce and offset commits;
- a future path to a replicated control-plane authority.

These concerns should not live inside the Kafka adapter, the daemon, queue storage, or `loom-core`
facet code. The coordination substrate owns the reusable authority model, while each surface maps its
protocol-specific state onto that model.

## 2. Crate Boundary

The target crate is:

```text
crates/loom-coordination
```

Import name:

```text
loom_coordination
```

### 2.1 Current Implementation

Current source includes `crates/loom-coordination` with:

- typed authority ids and epochs;
- typed coordination scopes and actor ids;
- monotonic fence allocation and stale-fence rejection;
- monotonic per-scope sequence allocation;
- producer identity registration and stale producer epoch rejection;
- consumer-group generation allocation;
- transaction begin, commit, abort, stale epoch rejection, and terminal-state enforcement.

Hosted Kafka integration source-backs a narrow durable metadata-version high-water adapter under the
queue workspace's reserved coordination metadata. Kafka topic metadata uses this allocator for
workspace-scoped metadata versions so future Kafka group metadata can share the same durable sequence.
Hosted Kafka also source-backs durable producer sequence high-water records for non-transactional
and transactional idempotent Produce, pending transactional offset commit records that apply only
when EndTxn commits, and transactional produced-record offset ranges used by read-committed Fetch
visibility.

The crate is part of the workspace and imports as `loom_coordination`. Focused unit tests cover the
source-backed behavior above.

Still target:

- generalized durable high-water persistence adapter outside the current Kafka metadata-version
  integration;
- daemon integration;
- distributed consensus backend;
- capability/conformance report rows outside the crate-local unit tests.

The crate should depend only on stable Loom primitives and small permissive dependencies. It must not
depend on hosted protocol crates, Kafka protocol code, FUSE, SQL, or facet-specific adapters.

Initial dependency posture:

| Dependency class | Initial posture |
| --- | --- |
| Loom types and errors | Allowed, to share stable identifiers and error mapping. |
| Loom store internals | Avoid in the core trait layer; allowed in a concrete single-node persistence adapter only if the boundary stays narrow. |
| Async runtime | Avoid in core traits where practical; adapters may offer async wrappers. |
| Raft or consensus crate | Not selected for the first implementation. OpenRaft and `raft` are candidates for a future distributed adapter evaluation. |

## 3. Authority Model

A coordinator authority is the linearization source for one store-local runtime domain. The initial
authority is single-node. It must still expose the same concepts a distributed backend would need:

| Concept | Meaning |
| --- | --- |
| Authority id | Stable identifier for the local coordinator authority. |
| Authority epoch | Monotonic epoch that changes when authority identity or durable state is reset. |
| Fence token | Structured monotonic token compatible with 0036's `u128` public fence direction. |
| Sequencer | Monotonic allocator for metadata versions, partition offsets, and transaction log positions. |
| Lease | Expiring right to act as a producer, member, or session holder. |
| Fencing check | O(1) comparison that rejects stale actors before durable mutation. |
| Transaction record | State machine for begin, commit, abort, timeout, and participant records. |

Coordinator state is authority-local operational state. It is not versioned workspace content and does
not sync through ordinary Loom sync.

## 4. Core Traits

The crate should expose small traits that can be implemented by `SingleNodeCoordinator` first and a
future consensus-backed adapter without changing Kafka or other served surfaces.

```rust
pub trait CoordinationAuthority {
    fn authority_id(&self) -> AuthorityId;
    fn authority_epoch(&self) -> AuthorityEpoch;
    fn next_fence(&mut self, scope: CoordinationScope) -> Result<FenceToken, CoordinationError>;
    fn next_sequence(&mut self, scope: CoordinationScope) -> Result<Sequence, CoordinationError>;
    fn check_fence(
        &self,
        scope: CoordinationScope,
        actor: ActorId,
        fence: FenceToken,
    ) -> Result<(), CoordinationError>;
}

pub trait ProducerCoordinator {
    fn register_producer(
        &mut self,
        scope: CoordinationScope,
        producer: ProducerIdentity,
    ) -> Result<ProducerEpoch, CoordinationError>;
    fn fence_producer(
        &mut self,
        scope: CoordinationScope,
        producer: ProducerIdentity,
        epoch: ProducerEpoch,
    ) -> Result<(), CoordinationError>;
}

pub trait TransactionCoordinator {
    fn begin_transaction(
        &mut self,
        scope: CoordinationScope,
        transaction: TransactionIdentity,
    ) -> Result<TransactionEpoch, CoordinationError>;
    fn commit_transaction(
        &mut self,
        scope: CoordinationScope,
        transaction: TransactionIdentity,
        epoch: TransactionEpoch,
    ) -> Result<(), CoordinationError>;
    fn abort_transaction(
        &mut self,
        scope: CoordinationScope,
        transaction: TransactionIdentity,
        epoch: TransactionEpoch,
    ) -> Result<(), CoordinationError>;
}
```

The exact Rust signatures can change during implementation, but these capabilities must remain
separate from Kafka-specific request and response types.

## 5. Single-Node Coordinator

The first implementation is `SingleNodeCoordinator`.

Properties:

- one authority per opened `.loom` store or hosted daemon authority domain;
- monotonic counters restored from durable high-water records;
- producer and transaction records stored as authority-local operational metadata;
- restart refuses to restore volatile sessions unless store identity, authority id, authority epoch,
  protocol profile, and durable high-water records match;
- stale producer epochs, stale transaction epochs, and stale fence tokens fail closed;
- no claim of multi-broker replication, ISR, leader election, or distributed consensus.

This is sufficient for single-node Kafka compatibility because a single broker still needs real
producer epochs, idempotent sequence validation, consumer-group generations, and transaction records.

## 6. Kafka Mapping

Kafka should use this substrate without owning it.

| Kafka concept | Coordination substrate mapping |
| --- | --- |
| Broker id | Authority id plus Kafka listener metadata. |
| Controller epoch | Authority epoch or metadata epoch. |
| Topic metadata version | Sequencer scoped to the Kafka workspace. |
| Partition offset | Sequencer scoped to `(workspace, topic, partition)`. |
| Producer id | Producer identity record scoped to `(workspace, topic set or transactional id)`. |
| Producer epoch | Producer coordinator epoch used for fencing. |
| Consumer group generation | Sequencer scoped to `(workspace, consumer group)`. |
| Transactional id | Transaction identity scoped to the Kafka workspace. |
| Transaction state | Transaction coordinator record with participants and terminal state. |

The Kafka surface must report single-node capability honestly. It may support real Kafka clients for
produce, fetch, metadata, offsets, idempotent producer behavior, and bounded transaction behavior when
source-backed, but it must not report multi-broker cluster compatibility until a distributed
coordinator backend exists.

## 7. Relationship To 0036 Locks

0036 public locks and 0036a coordination use the same authority concepts but serve different users:

| 0036 locks | 0036a coordination substrate |
| --- | --- |
| Public lock, lease, and fence surface. | Internal reusable authority API. |
| Used directly by applications, mounts, and long-running local sessions. | Used by hosted compatibility surfaces and runtime subsystems. |
| Focused on mutual exclusion and fencing. | Adds sequencers, producer epochs, group generations, and transaction records. |

The two must share fence width, authority identity, epoch safety, restart behavior, and failure
semantics.

## 8. Conformance Requirements

The substrate is not complete until tests prove:

- fence tokens increase monotonically inside one authority;
- high-water restore prevents token regression after restart;
- authority epoch mismatch fails closed;
- stale producer epochs are rejected;
- stale transaction epochs are rejected;
- abort and commit are terminal transaction states;
- partition offset allocation is monotonic per partition;
- consumer-group generation allocation is monotonic per group;
- single-node restart preserves durable high-water and terminal transaction records;
- simulated future-backend adapters can satisfy the same trait-level behavior without Kafka-specific
  types.

## 9. Non-Goals

- no distributed Raft backend in the first implementation;
- no Kafka protocol parser or Kafka wire codec in this crate;
- no queue storage mutation in this crate;
- no public CLI surface for raw coordination internals beyond existing lock/daemon surfaces unless a
  later spec promotes one;
- no sync of live coordination state through ordinary Loom sync.

## 10. Resolved Decisions

- **RD1 - Separate crate.** Coordination is a reusable crate boundary because Kafka, future broker
  surfaces, SQL sessions, hosted transactions, and locks can share authority, fencing, sequencing, and
  transaction machinery.
- **RD2 - Single-node first.** The first implementation is a real single-node coordinator, not a fake
  cluster abstraction. It must still expose epochs, fencing, sequences, and transaction state.
- **RD3 - Consensus adapter not selected.** OpenRaft and `raft` remain candidates for a future backend
  evaluation. No Raft dependency is introduced before a distributed coordination task selects it.
- **RD4 - Kafka does not own coordination.** Kafka maps onto the substrate. Producer epochs,
  transactions, and group generations must be reusable concepts, not Kafka-private globals.
- **RD5 - Operational state stays outside versioned content.** Coordinator records are authority-local
  operational metadata, like 0036 lock state and 0021b consumer offsets. They do not become workspace
  commits and do not sync by ordinary Loom sync.
