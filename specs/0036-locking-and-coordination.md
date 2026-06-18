# 0036 - Locking and Coordination

**Status:** Draft, with local/native coordination source-backed and public hosted projection remaining.
**Version:** 0.1.1. **Capability:** `lock`.

This spec defines runtime mutual-exclusion and coordination primitives for Loom: leased, reentrant,
**fenced** locks scoped to a single workspace and a single coordinator, plus the optimistic
compare-and-set primitives that should be preferred over them. It is a *control-plane* facet: lock
state is ephemeral runtime state and is **never** part of the versioned, content-addressed object
graph (§3).

0036a defines the reusable `loom-coordination` substrate used by hosted compatibility surfaces that
need authority epochs, sequencers, producer epochs, transaction records, or future clustered
coordination. 0036 remains the public lock surface; 0036a owns the internal reusable authority
boundary.

## Current implementation

Current source implements the embedded core substrate, not the public `lock` facade. `loom-store`
persists a durable-local control root outside the versioned workspace object graph, with CRUD and
prefix-scan helpers over deterministic key/value bytes. `FileStore::lock_coordinator` and
`save_lock_coordinator` bridge `LockCoordinator` fence counters and applied high-water marks into that
control root so fences do not regress after reopen. `loom-core::lock` implements an in-process
`LockCoordinator` with exclusive, shared, and semaphore modes, reentrancy, leases, monotonic fencing,
stale-fence rejection, and restoreable durable-local fence high-waters. `loom-core::sync` exposes
`push_branch_locked` as the first internal user: it takes an exclusive destination-branch lock before
running the existing fast-forward push. The CLI exposes the top-level manual daemon lifecycle
(`loom daemon start|stop|restart|status`) for a local coordinator process keyed by stable `.loom`
file identity on Unix and Windows, with canonical path fallback only on other platforms. The local
control channel is loopback-only and provides readiness/status/stop probes. The daemon runtime uses
per-store-identity address, pid, and lock files; duplicate starts for the same store identity are
idempotent after a verified daemon handshake, while different store identities route to different
runtime files and may have independent daemons. The status handshake carries the daemon protocol
version, transport, pid, canonical store path, and store identity. The shared runtime directory is
created as a non-symlink directory with private Unix permissions where that check is available.
`loom doctor daemon <store>` diagnoses runtime files, runtime artifact presence and size, runtime-directory
privacy, loopback bind capability, parsed daemon endpoint, host-daemon reachability from the current
environment, and live daemon status. `loom-store::daemon` source-backs the transport contract model
and routes client requests through a typed endpoint abstraction. Daemon startup selects secure native
IPC by default where available: Unix socket serving with peer-credential owner checks on supported
Unix-family targets, and Windows named-pipe serving with an owner-only security descriptor on Windows.
TCP loopback requires explicit selection, remains the degraded portable fallback, and reports its
`degraded_loopback` security profile in the endpoint envelope, `doctor daemon`, text status output, and
status JSON. `doctor daemon` reports every daemon transport capability with supported, degraded, or
unsupported status, endpoint security profile, and reason strings. The
daemon process owns an in-memory session
set, pin register, and restored `LockCoordinator`; CLI `daemon session attach|detach`, `daemon pin
add|remove`, and top-level `lock acquire|refresh|release` exercise those daemon-backed surfaces.
The daemon also source-backs `lock-apply-fence`, which validates a live held token and records the
applied fence high-water before a protected write is admitted.
Manual pins are permanent until removed. FUSE and NFS mount commands use leased pins and refresh them
while the mount command is alive, so a crashed mount process stops refreshing and the daemon prunes the
expired pin on later status/stop handling. `daemon stop` refuses while a live pin remains.
`loom-store::daemon` owns the reusable native daemon client protocol: per-store runtime path
derivation, status parsing, request dispatch, session attach/detach, pin add/remove, lock
acquire/refresh/release/apply-fence request construction, stale daemon address classification, and
the lock-mode wire vocabulary. Missing or stale daemon address files surface as `NOT_FOUND`, so attached clients can
distinguish "no coordinator is running" from a reachable daemon rejecting an operation. The CLI,
`loom-ffi`, and `loom-mcp` use that shared client. `loom-ffi` exposes path-oriented daemon status,
session attach/detach, pin add/remove, and lock acquire/refresh/release C ABI calls; token-returning
lock calls return JSON rather than the daemon's internal tab-separated wire line. The public lock
token projection is the structured `Fence` shape, with `authority:u32`, `epoch:u32`, and
`sequence:u64`; the embedded daemon maps its current local sequence as `0:0:sequence`. The C ABI
refresh/release calls accept the canonical low and high `u64` limbs, IDL and remote generated APIs
carry the structured fields or limbs, and Node, Python, C++, and JVM host-native wrappers expose
structured token parsers. `loom-mcp` stdio host startup attaches to an already-running per-store
daemon and detaches on host drop, so multiple MCP server processes share the same daemon session
register instead of each owning independent volatile state. Attached MCP access revalidates that
daemon session before per-request reads, writes, SQL access, runtime KV requests, and
daemon-authorized opens. A stopped daemon or restarted daemon with an empty session register makes
the old attached host fail closed instead of continuing as an independent writer. MCP `tools/list`
derives regular tool visibility from current ACL state, advertises `tools.list_changed`, and rejects
hidden stale tool calls before router dispatch; the engine PEP remains authoritative for
argument-scoped enforcement on every call. Native writable
`FileStore::open` rejects direct opens when a verified CLI daemon is running for the store; read-only
snapshots remain allowed, and daemon internals use an explicit daemon-authorized open path.
`loom-conformance` includes an executable `lock` behavior runner for lock modes, lease expiry, and
fencing, plus portable `lock-fence` vectors for embedded and external authority packing. Current
ordinary persisted operations are not routed through daemon sessions unless they need shared runtime
state; they stay direct and are guarded by the daemon writable-open exclusion and native
single-writer store guard. Source does not yet expose daemon/lock client wrappers in closed-filesystem
or browser bindings, implement public hosted lock protocol, blocking hosted acquire, hosted wait
queues, lock authorization integration, hosted lock schemas, or public `lock` conformance through
bindings/protocols. IDL names the target `Daemon` and `Locks` interfaces; the source-backed
implementation today is shared native daemon client, C ABI daemon client operations, Node/Python/C++/
JVM daemon and lock binding wrappers, host-native typed structured lock tokens,
explicit acquire/try-acquire/refresh/release helpers, scoped cleanup helpers for Node/Python/C++/JVM,
CLI daemon runtime, CLI lock commands, direct writable-open daemon exclusion, MCP host attach, MCP
daemon-authorized writes, MCP stateless rejection of pure-ephemeral KV data operations, and
daemon-hosted pure-ephemeral KV for attached MCP hosts. `uldren-loom-conformance` serializes
`local_coordination` evidence rows that distinguish embedded coordinator behavior, CLI daemon runtime,
host-native lock clients, MCP attached-session liveness, degraded TCP loopback, unsupported hosted lock
protocol, unsupported mobile/browser daemon-lock clients, and unsupported native transports on
nonmatching platforms. Daemon launch itself is a CLI/runtime concern, not a general binding contract.

The same authority-local control-plane store is also used by 0019a's ephemeral cache tier. The shared
store has two durability classes: volatile registers that are live coordinator state and may be
restored only through the v1 same-authority hot-restart path, and durable local high-water records that
survive restart but never sync. Lock leases use the volatile class; fencing high-waters use the
durable-local class.

This spec composes with the concurrency model already defined in 0003 §9 (filesystem vs.
workspace-history locking), the ref compare-and-swap of 0003 §5.10, batches (0003 §6), the
single-writer journal lock (0005 §6.4), principals and identity (0026), grants and access control
(0015 §6, 0027, 0028), synchronization (0006), and durable-delivery leases (0035).

## 1. Motivation and posture

A lock has meaning only at a **linearization point** - a single authority that totally orders
acquire and release. Loom is local-first, content-addressed, branchable, and asynchronously
synced; that is structurally an AP system. The design therefore takes the same explicit split that
Hazelcast takes between its AP partition-level `IMap` locks and its linearizable CP-Subsystem
`FencedLock`/`ISemaphore`/`IAtomicLong`:

- **Within one coordinator** - today, one `.loom` file's single-writer journal (0005 §6.4); a hosted
  authority is designed-for but **not built** (RD3, §11) - locks are linearizable and safe.
- **Across branches, or across asynchronously-syncing replicas, there is no safe mutual exclusion.**
  This is a CAP consequence, not a missing feature. In that setting "locks" are at most advisory and
  MUST be reconciled at merge; an implementation MUST NOT present cross-replica mutual exclusion as a
  guarantee.

Locks are the deliberate, fenced, leased *exception*. The substrate is already an optimistic-
concurrency engine, and §2 is the default that removes most demand for held locks.

## 2. Prefer optimistic concurrency (the default)

The ref CAS of 0003 §5.10 (`update_ref(name, new, expected_old)`), the commit-time CAS validation of
batches (0003 §6), and version-expected facet writes are Loom's `compareAndSet`. Most "I need a lock"
needs are really "I need a compare-and-set." This spec therefore REQUIRES, as a precondition for the
held-lock layer, that mutating facets expose **version-expected mutation**:

```idl
kv.put(ns, name, key, value, opts: { expected_version: Option<Version> })  // CAS_MISMATCH on stale
// and the analogous shape for sql rows, document, graph, ... where a facet defines a per-element version
```

This mirrors the Hazelcast `EntryProcessor` idea: atomic in-place mutation with no held lock.
Pessimistic locks (§4-§6) are reserved for cases optimistic CAS cannot serve: serializing **external
side effects**, and coarse multi-step critical sections that do not fit one batch.

## 3. Lock state is ephemeral, not versioned (normative)

Lock records and, critically, the monotonic **fencing counter** (§5) MUST NOT be written into the
versioned Merkle object graph or any branch's working tree. They are not facet content; they are
control-plane runtime state held by the coordinator.

Reasons this is mandatory:

- **A lock "held on branch A" must not appear held on branch B.** Versioned lock state would make
  mutual exclusion a property that forks, merges, and syncs - which is meaningless and unsafe.
- **A fencing token in the versioned tree would not be monotonic.** Branching, rebasing, and merging
  reorder and duplicate committed state; a fence derived from committed history could move backward
  or fork, defeating its only purpose (§5).
- **Sync must never carry lock leases.** Like the delivery leases of 0035, leases are ephemeral and
  reconstructable after a restart; they are not stream/graph content and are never pushed or pulled.

Where it lives instead - two distinct control-plane storage classes, both per workspace, both
outside the object DAG, **neither ever synced**:

1. **Held-lock register (volatile, with v1 hot-restart).** Who holds what right now, lease deadlines,
   reentrancy, waiter queues, sessions, and wait state. Pure runtime state. The v1 local daemon MUST
   offer an explicit hot-restart path that restores this register only for the same verified store
   identity, same coordinator identity, and same local authority epoch. Hot-restart state remains local,
   never syncs, and MUST be discarded when the store identity, coordinator epoch, protocol profile,
   principal/session proof, or durable fence high-water check does not match. A Loom rebuilt by pulling
   workspaces still comes up with this register **empty** - intentionally, because a rebuilt replica is
   a *new coordinator* and inheriting another coordinator's live leases would be unsafe (§5,
   restart-safety).
2. **Fence high-water (durable, but local).** The monotonic fence counter and per-key high-water
   marks (§5). This MUST be durable at the authority so it never regresses across a restart, yet it
   MUST NOT be synced, because a fence is meaningful only relative to its issuing authority.

Durability of the fence therefore lives wherever the linearization authority lives: the single-file
backend keeps it in a journal side-region governed by the §6.4 single-writer lock; a future hosted
authority keeps it in its own durable store (and local replicas never hold the authoritative fence
at all). Locks remain **per workspace** - that scoping is correct and retained - but the register is
a sibling control plane, not a branch of the data tree.

> NOTE: The one durable artifact a lock *may* leave in versioned history is an ordinary audit/event
> record ("principal P held key K, fence F, t0-t1") emitted through 0029/0030 *after the fact*. That
> is history about a lock, not the lock.

## 3.1 Coordinator runtime and detached lifecycle

The public lock facade requires an authority process that owns volatile runtime state. In embedded
native mode that authority is a **local Loom coordinator runtime** keyed by stable `.loom` file
identity where available. Sessions attach to the coordinator; they do not own the lock register
directly.

Lifecycle policy:

- **Manual lifecycle is the default.** Operators start and stop the coordinator explicitly. The CLI
  source-backs `loom daemon start|stop|restart|status`. General bindings do not launch the CLI daemon;
  embedded hosts manage their own process lifecycle. This avoids guessing whether a caller wanted locks,
  cache state, or sessions to outlive the first process.
- **Routing is by stable store identity.** On Unix-family platforms, starting the daemon through two
  hard links to the same store resolves to one runtime identity by device/inode. Windows uses the
  by-handle volume serial number and file index. Platforms without a durable file identity fall back to
  canonical path. Starting daemons for two different store
  identities creates two independent coordinators. A daemon status response is accepted only when its
  protocol, transport, and store identity match the caller's expectation.
- **Duplicate start is idempotent.** A second `start` for the same running daemon reports the existing
  daemon. A concurrent `start` waits for the first starter's lock file to produce a verified daemon
  handshake before it treats stale files as cleanup candidates.
- **Agent/MCP hosts attach, not own state.** A stdio MCP server may be long-running, but it is still an
  integration process. Multiple MCP servers opened over the same `.loom` MUST attach to the per-store
  daemon for locks, pure-ephemeral KV, delivery leases, and other volatile runtime state. They MUST NOT
  each keep independent authoritative coordinator state for the same store.
- **Auto-start is opt-in.** A binding or CLI command MAY request "start if absent" when opening a
  session, but that policy is explicit. Auto-start is useful for application bindings and interactive
  tools, not for silent one-shot commands.
- **Detach policy is explicit.** After the last session disconnects, the coordinator follows its
  configured policy: stop immediately, stay alive for a grace window, or stay alive until manually
  stopped. A first session MAY set the policy when it starts the coordinator.
- **Mounts pin the coordinator.** A FUSE or NFS mount starts or attaches to the coordinator and holds
  a leased pin for the mount lifetime. `loom daemon stop` fails while the leased pin is live; unmounting
  releases the pin, and a crashed mount process stops refreshing it so the daemon can prune it later.
  Manual permanent pins require explicit removal or `loom daemon stop --force`. Daemon status and
  doctor output report total, permanent, leased, and per-pin state.
- **True stateless calls cannot take locks.** A call that cannot attach to a coordinator MUST reject
  public lock operations rather than create a handle-local lock register. Handle-local locks would let
  two processes acquire the same key and are therefore out of contract.
- **Host reachability is environment-local.** The local daemon address is a loopback endpoint. Sandboxes,
  containers, remote dev shells, and restricted agents attach only when that endpoint is reachable from
  their own network namespace and store identity matches the address envelope. Otherwise they reject
  lock and stateful runtime operations instead of assuming a visible store path implies a reachable host
  daemon.

Binding projection:

- The CLI owns the full local daemon lifecycle: start, stop, restart, status, doctor, pins, sessions,
  and lock commands.
- Native desktop/server bindings MAY expose daemon status, session attach/detach, pins, and lock
  acquire/refresh/release where they are explicitly promoted. Node, Python, C++, and JVM source-back
  these client calls today. Node, Python, C++, and JVM also source-back typed lock-token helpers and
  scoped cleanup helpers. They do not launch the CLI daemon.
- Embedded and closed-filesystem bindings, such as Android, iOS, and browser WASM, are host-managed:
  the application initializes Loom, opens/authenticates its store, responds to platform lifecycle
  events, logs out, and deinitializes. The application process is the coordinator for that closed
  environment; it does not spawn or control the CLI daemon.
- Desktop/server bindings that open a native `.loom` directly MUST respect the native writer lock and
  MUST reject unsafe direct access when a CLI daemon owns the store for stateful guarantees. Source now
  rejects direct writable native opens while a verified daemon is running; race closure around daemon
  startup and already-open writers remains target work.
- WASM bindings can provide coordinator semantics only within the platform's lifetime model, such as a
  Worker or SharedWorker. Cross-tab or long-lived coordination SHOULD use a hosted coordinator.

### 3.2 Known unresolved coordinator corner cases

These are not optional polish. They are the concurrency and operating-environment cases that must be
closed before the daemon/lock surface can be called release-complete:

- (P0) **Non-Unix file-identity parity.** Source backs stable daemon routing by device/inode on
  Unix-family platforms and by Windows volume serial number plus file index on Windows.
  Canonical-path fallback remains only for platforms that do not expose a durable file identity through
  the current implementation.
- (P0) **Runtime directory ownership.** Source backs a private, non-symlink Unix runtime directory
  check before address, pid, or lock files are trusted, and daemon clients reject non-regular or
  symlink address files before connecting. Same-user spoofing remains a concern only for explicit TCP
  loopback fallback. Secure native runtime paths use Unix peer-credential owner checks or a Windows
  owner-only named-pipe descriptor. Non-Unix privacy parity remains target work only for platforms that
  lack those native transport profiles.
- (P0) **Stale starter recovery.** Source backs four recovery edges: duplicate `start`
  waits for a verified handshake before cleaning stale files, shared daemon clients classify missing
  or stale address files as `NOT_FOUND` rather than protocol success, FUSE/NFS mount pins are leased
  and refreshed by the mounting process, and `loom daemon stop --force` is an explicit operator
  override for permanent pins. `loom doctor daemon <store>` reports the address, pid, starter-lock artifact
  state, and pin breakdown so operators can see whether stale files or manual pins are present. A
  heavily paused starter can still look stale and must be handled by the verified-handshake cleanup
  path rather than by trusting a pid file.
- (P0) **Multi-MCP split-brain.** MCP stdio processes attach to the daemon at startup, and
  daemon-attached MCP writes use daemon-authorized store opens instead of conflicting with the
  daemon-owned-store guard. Stateless MCP hosts reject pure-ephemeral KV data operations rather than
  creating per-call cache illusions. Daemon-attached MCP hosts route pure-ephemeral KV
  put/get/delete/list/range through the daemon runtime, and daemon-side KV requests load persisted
  identity/ACL control state before crossing the engine PEP. Current MCP source does not expose
  delivery leases or held file handles as stateful guarantees. Source backs request-scoped daemon
  session liveness checks for attached MCP access; a stale attached session fails closed. Any future MCP
  surface that depends on shared runtime state must route that state through the daemon or through a
  hosted coordinator before promotion.
- (P0) **Hosted projection disconnect after coordinator loss.** `daemon stop` stops the daemon process
  and removes runtime files. Daemon-owned hosted HTTP listeners stop accepting, gracefully drain active
  Hyper connections, and wait for connection tasks; hosted gRPC uses the tonic shutdown future; hosted
  IMAP stops accepting, waits for connection tasks for a bounded grace window, and aborts leftover
  connection tasks. External attached `loom mcp` stdio and HTTP hosts monitor their daemon session,
  reject new requests after daemon loss, wait for active MCP requests for a bounded grace window, and
  then close their transports. The CLI stop policy is source-backed: graceful drain is the default,
  `--wait <ms>` selects the graceful wait, `--hard` skips graceful waiting, and `--force` remains the
  explicit live-pin override. Daemon stop audit and response output include force, hard, wait, pin,
  listener, and timed-out listener counts.
- (P0) **Daemon-owned-store exclusion race closure.** Source backs the main guard: when a verified
  daemon is active for a store, non-daemon direct writable opens fail fast with a stable conflict error,
  while read-only snapshots and daemon-authorized opens remain allowed. Daemon startup holds the native
  writer lock until the verified daemon handshake is established, so an existing direct writer blocks
  startup and a new direct writer cannot slip in during launch. Remaining work is hostile same-user
  daemon impersonation defense under the runtime-directory threat model.
- (P1) **Secure IPC transport promotion.** TCP loopback works across macOS, Linux, Windows, Docker,
  and many agent hosts, but it is a degraded portable fallback because it does not authenticate hostile
  same-user peers. The transport contract model records TCP loopback, Unix socket, and Windows
  named-pipe capability states plus endpoint security profile. The daemon client request path uses a
  typed endpoint abstraction. Supported Unix-family builds bind a runtime-directory Unix socket,
  parse and connect Unix socket envelopes, and validate peer credentials against the store owner before
  request handling. Windows builds bind an owner-only named pipe, parse and connect named-pipe
  envelopes, and advertise the owner-only named-pipe security profile. Daemon startup selects those
  secure native transports by default where available, while explicit `--transport tcp` selects the
  degraded loopback fallback. Transport-profile evidence is source-backed through endpoint envelopes,
  `doctor daemon`, text status output, `daemon status --json`, and daemon transport capability tests.
- (P1) **Encrypted-store daemon credentials.** The daemon can restore persisted fence high-waters from
  unencrypted control state. If future control-plane state needs encrypted store access, the daemon
  must receive an explicit session credential or delegate that operation back to an authenticated
  session. It must not silently cache user passphrases because the daemon may outlive the initiating
  client.
- (P1) **Hot-restart of volatile registers.** v1 includes local hot-restart for held locks, sessions,
  pins, waiters, and related runtime registers. Restore is allowed only after the daemon verifies the
  same store identity, same coordinator identity, same local authority epoch, matching protocol
  profile, and durable fence high-waters that are at least as high as the restored volatile records.
  Any mismatch drops the volatile snapshot and keeps only durable fence high-waters. Hot-restart does
  not make locks syncable, does not survive pull-rebuild as live state, and does not weaken fencing:
  stale holders remain fenced out by the durable high-water rule even when the volatile register is not
  restored.

## 4. The `lock` facet

A workspace that advertises the `lock` capability exposes named locks keyed by an order-preserving
key (same key domain as KV, 0019). A lock record (held in the §3 register) is:

```idl
LockRecord {
  key:           Value,                    // typed key, workspace-scoped
  owner:         (PrincipalId, SessionId), // 0026 identity; ownership like Hazelcast thread-ownership
  mode:          Exclusive | Shared | Semaphore { permits, capacity },
  fence:         Fence,                    // monotonic fencing token (§5)
  lease_deadline: Instant,                 // auto-release; like Hazelcast lock(key, leaseTime)
  reentrancy:    u32,                      // reentrant for the same owner
}
```

Semantics:

- **Reentrant** for the same `(principal, session)` owner; a re-acquire increments `reentrancy` and
  returns the same fence. Release decrements; the lock frees at zero.
- **Leased.** Every acquire carries a lease; the lock auto-releases at `lease_deadline` so a dead or
  partitioned holder cannot deadlock the workspace. Holders renew with `refresh`. This is the dead-
  holder defense and it is mandatory.
- **Mode lattice.** Shared locks coexist with shared locks; an exclusive request waits for or is
  rejected against any holder; semaphore holders coexist until their summed permits reach capacity.
- **Non-blocking and bounded-blocking forms.** `try_lock` returns immediately; `lock(timeout)` waits
  up to a bound then raises `LOCKED`. There is no unbounded blocking acquire in the served surface.

Surface sketch (target IDL, not yet promoted):

```idl
lock.acquire(ns, key, opts: { mode, lease, timeout }): Future<LockToken>   // LockToken { fence, lease_deadline }
lock.try_acquire(ns, key, opts): Future<Option<LockToken>>
lock.refresh(ns, key, token): Future<LockToken>                            // extend lease
lock.release(ns, key, token): Future<void>                                 // LOCK_NOT_HELD / FENCING_STALE
```

## 5. Fencing tokens (normative)

This is the central safety property, taken from Hazelcast `FencedLock.lockAndGetFence()` and the
standard distributed-locking critique: a leased lock alone is unsafe, because a holder can pause past
its lease, a second holder can acquire, and the zombie can then write.

The coordinator maintains a **strictly monotonic per-workspace fence counter** in the §3 register
(never in versioned history). Each successful exclusive acquire returns the next value as the
`fence`. Every write on a path protected by a lock MUST present a `fence`, and the facet write path
MUST reject any write whose `fence` is below the highest fence already applied to that key, with
`FENCING_STALE`. A stale (expired-lease) holder is thereby fenced out even if it believes it still
holds the lock.

The fence counter is the same kind of object as a FlakeId / monotonic generation; it MAY be sourced
from the journal's monotonic write sequence, but it is *not* the commit DAG sequence (which is not
monotonic across branches, §3). The long-term public fence shape is a structured `u128` value with
authority/epoch/sequence room for hosted and clustered authorities. The embedded local coordinator may
store and issue a compact `u64` sequence internally while the public hosted and cross-authority
contract treats fences as `u128`. Public APIs SHOULD expose the structured fence as an opaque numeric
or byte-string token rather than as a plain mutex-like integer.

**Restart and rebuild safety (normative).** The fence counter MUST NOT regress. Across a coordinator
*restart* it is restored from the durable fence high-water (§3), so a restarted coordinator never
re-issues a fence a stale holder may still carry. Across a *pull-rebuild* it does not need restoring:
a rebuilt Loom is a new authority, and in the embedded single-writer model the §6.4 lock guarantees
no other coordinator is live for the workspace, so starting the fence fresh is safe. A future hosted
authority keeps the fence on the server; local rebuilds never touch it.

## 6. Enforcement modes

- **Advisory (default).** A lock excludes only other lock-takers; raw facet writes ignore it. This is
  Hazelcast `IMap.lock` semantics and is the cheaper, more common case.
- **Enforced.** When a key is locked in enforced mode, the facet write path requires a valid `fence`
  (§5) and rejects unfenced or stale writes. Enforced mode is what makes a held lock actually protect
  a key against careless writers.

A facet declares per-key/range/table granularity (KV key, SQL row/range/table, document id); the
register keys lock records accordingly.

## 7. Programs and locks (revised - no in-program contention)

Programs (0015) MUST stay deterministic and replayable (0015 §10): same program + base + inputs ⇒
same state root. **A program blocking on a contended lock is therefore not allowed** - acquisition
order would depend on wall-clock races, making the run non-deterministic and non-replayable. The
design reflects this:

- **Gated run-on-a-branch (0015 §8) needs no pessimistic locks.** Fork, run, adopt-by-merge is
  *already* optimistic concurrency; contention surfaces at the adopting merge as `CAS_MISMATCH` /
  `CONFLICT`. This is the recommended mode for anything contended.
- **For direct/served execution, the runtime - not the program body - owns coordination.** The
  scheduler MAY take a lock *around* a whole run (so two runs that touch the same key serialize), but
  the program does not contend mid-execution. If a program is permitted any lock primitive at all, it
  is the **non-blocking `try_lock` only**, and the outcome must be treated as a *declared input* to
  the run so the execution stays replayable; a blocking acquire grant is out of contract.
- Any lock a runtime holds for a run is leased and **force-released on completion, budget exhaustion,
  or crash** (§4 lease), bounded by the run budget.

Net: drop the earlier idea of a general `Mode::Lock` that lets a program block. Coordination for
exec is the scheduler's job (run-level serialization) or is expressed optimistically (run-on-branch).

## 8. Coordination use sites (where Loom itself should take a lock)

The same primitive serves Loom's own internals, but not every internal mutation needs a separate
runtime lease. Current source uses two guard classes:

- **Runtime lock primitive** - a daemon or embedded `LockCoordinator` lease/fence for operations whose
  correctness depends on a named, externally visible critical section.
- **Native single-writer store guard** - the exclusive writable `FileStore` handle plus the per-handle
  `FileStore` mutexes and crash-safe journal/checkpoint path. This is sufficient for one process-local
  maintenance or storage mutation because a second native writer cannot open the same `.loom`, and a
  verified daemon-owned store rejects direct writable opens.

Current use-site status:

| Use site | Current guard | Status |
| --- | --- | --- |
| Sync destination | Runtime lock primitive | Source-backed for `push_branch_locked`, which takes an exclusive destination-branch lock before fast-forward publication. Pull/live-sync promotion must use the same destination lock. |
| Workspace-history operations | Native single-writer store guard, plus VCS per-workspace state | Current `commit`, `merge`, `branch`, `checkout`, restore, tag, and ref mutations run through one writable `Loom<FileStore>` and `save_loom`. A future hosted coordinator can narrow these to named workspace-history leases when concurrent hosted writers are promoted. |
| Maintenance and GC/compaction | Native single-writer store guard | `gc_loom`, `compact`, `compact_retaining`, and `gc_segments` require a mutable writable `FileStore` handle. They remain direct single-writer operations, not public held locks. |
| Schema/DDL | Native single-writer store guard and SQL transaction overlay | SQL create/alter/catalog mutations run inside the writable SQL facade and persist through `save_loom`; concurrent native DDL is excluded by the writer handle. Hosted SQL DDL must add a named DDL lease before concurrent hosted writers are promoted. |
| Key rotation / unlock | Native single-writer store guard for mutation; handle-local unlock state for reads | `rekey` and `rekey_reseal` require a writable unlocked store handle. Unlock itself is handle-local and is not a shared critical section. Hosted or daemon-mediated key rotation must remain a privileged single-writer operation. |
| Snapshot/checkpoint | Native single-writer store guard | `save_loom`, journal commits, superblock checkpoints, and forced rekey checkpoints are serialized by the writable store handle and internal store mutexes. |
| Derived-artifact rebuilds | Native single-writer store guard for embedded artifact writes | Embedded derived-artifact metadata and payloads live in the durable-local control root and are retained by compaction. Concrete accelerator rebuild scheduling remains target work; when multiple builders are promoted, they need a named builder lease. |
| Triggers/derivations (0029) | Target runtime lock primitive | Single-fire and single-builder guarantees are target work. |
| Queue consumers (0021b) | Target lease primitive | A single-consumer claim should be modeled as a lease over a partition and unified with delivery leases (0035). |

## 9. Bindings (application layer)

One `locks` facade projected through the C ABI to all bindings (0007). The blessed default is the
**scoped** form, so release cannot be forgotten:

```text
with_lock(ns, key, lease, fn)   // RAII guard / context manager / try-with-resources / using
```

The source-backed host-native binding layer exposes explicit acquire, try-acquire, refresh, and
release-by-token helpers, plus scoped helpers where the language has a natural cleanup form: Node
`withLock`, Python context manager, C++ RAII guard, and JVM `AutoCloseable` guard for
try-with-resources. The JVM binding uses a fenced `LockToken` shape and does not drop fence semantics
behind a plain mutex abstraction. A future Java `java.util.concurrent.Lock` adapter MAY be added only
as an advisory convenience over the fenced API. The optimistic family
(`AtomicLong`/`AtomicReference`/`Semaphore`/`CountDownLatch`-style helpers built on §2 CAS) SHOULD
also be exposed, since it is cheaper and safer than held locks for most app needs.

## 10. Error taxonomy additions

The lock error codes are source-backed in `loom_core::error::Code`, the daemon error parser, and the
current lock/conformance paths. Binding and wire projections that claim lock behavior MUST preserve
these codes verbatim:

| Code | Meaning |
| --- | --- |
| `LOCKED` | The lock is held by another owner; a bounded acquire timed out. |
| `LOCK_LEASE_EXPIRED` | The caller's lease elapsed before the operation. |
| `FENCING_STALE` | A fenced write presented a token below the applied high-water mark (§5). |
| `LOCK_NOT_HELD` | Release/refresh for a lock the caller does not own. |

## 11. Resolved decisions

- **RD1 - Lock mode lattice (enterprise).** The contract defines one mode lattice from day one:
  exclusive (writer), shared (reader), and an N-permit semaphore (exclusive = 1 permit, shared = N).
  The IDL, per-mode fence rules, error codes, and the semaphore primitive are frozen now even though
  the first implementation MAY ship exclusive-only behind that surface; adding a mode to the acquire
  signature later would be a breaking ABI/wire change and is disallowed.
- **RD2 - Enforced by default for durable keys (enterprise).** Enforcement (§6) is a per-key/per-facet
  policy wired into access control (0027/0028) and defaults **on** for durable facets; advisory is an
  explicit opt-out for app-level coordination over non-durable state. The fence check is O(1) (one
  `u64` compare against the per-key high-water already in the register), so enforcement carries no
  meaningful write-path cost.
- **RD3 - Coordinator seam now; clustering later (not yet built).** The facade is defined against a
  single `Coordinator` (linearization-authority) abstraction so a future hosted/Raft CP backend can
  be added without changing the surface or the fence semantics. **Loom is not a clusterable server
  today.** The only implemented authority is the embedded single-writer coordinator of one `.loom`
  file (0005 §6.4). A multi-member CP subsystem is explicitly future work - designed-for, not built;
  nothing in this spec claims current clustered operation.
- **RD4 - Fence source = authority-owned monotonic generator (enterprise).** The fence is a durable,
  authority-owned, strictly monotonic value issued in **preallocated ranges** so acquisition need not
  fsync per call. The embedded authority may use a local `u64` sequence internally, but the long-term
  public fence contract is a structured `u128` FlakeId-shaped value so hosted, multi-authority, and
  epoch-bearing coordinators do not need a later wire break. A future hosted CP authority maps its
  consensus log position or monotonic allocation sequence into that structured fence. The fence is
  never the commit DAG sequence and never a wall clock. The generator SHOULD also be exposed as a
  first-class FlakeId primitive.
- **RD5 - Manual detached coordinator lifecycle by default.** Public locks, pure-ephemeral KV, and
  long-lived sessions attach to a local coordinator runtime. Manual start/stop/status is the default
  lifecycle; auto-start and post-disconnect grace are explicit policies. Mounts pin the coordinator,
  and true stateless calls reject locks.
- **RD6 - CLI daemon, embedded host lifecycle.** The `loom` CLI is the full-featured detached daemon
  runtime. General language bindings do not spawn or manage that daemon by default. Closed-filesystem
  environments, including mobile apps and browser WASM, embed Loom and let the host application manage
  initialization, authentication, pause/resume, logout, and shutdown.
- **RD7 - Local hot-restart is in v1.** The local daemon must persist enough volatile-register snapshot
  state to restore sessions, pins, waiters, and held locks across a controlled daemon restart when the
  same store identity, coordinator identity, local authority epoch, protocol profile, and durable fence
  high-water checks match. This is an operational continuity feature, not a sync feature. It must fail
  closed by discarding the volatile snapshot on any identity, epoch, authorization, or high-water
  mismatch.
- **RD8 - Public fence width is `u128`.** The long-term enterprise surface uses a structured `u128`
  fence token. The lower 64 bits can carry the embedded local sequence for current single-file stores;
  upper bits are reserved for authority, epoch, and hosted allocation profile. This avoids freezing a
  local-only `u64` into hosted APIs and bindings, and it keeps the future hosted CP authority from
  needing a breaking migration.

### Remaining open questions

Decision Points: none.

## 12. Implementation slices

- (P0, source-backed) Persistent fence bridge: `LockCoordinator::fence_counters` and
  `applied_fences` serialize through `FileStore::control_*`, so embedded fences survive real `.loom`
  reopen.
- (P0, source-backed) Lock behavior conformance covers exclusive/shared/semaphore compatibility,
  expiry, reentrancy, live-token fenced-write admission, stale-fence rejection, and sync destination
  serialization.
- (P0, source-backed for manual lifecycle) Local coordinator runtime lifecycle includes daemon
  start/stop/restart/status, doctor, session attach/detach, pins,
  lock acquire/refresh/release/apply-fence, explicit manual lifecycle, MCP attached-session
  liveness, stateful pure-ephemeral KV routing, and stateless pure-ephemeral rejection.
- (P0, source-backed) Public fence tokens use the structured `u128` contract. `loom-types` defines
  `Fence` as `authority:u32`, `epoch:u32`, and `sequence:u64` with canonical packing and low/high
  limb conversion. The embedded coordinator maps its durable local sequence as `0:0:sequence`;
  `loom-wire`, IDL, C ABI refresh/release calls, remote generated APIs, hosted dispatch, MCP lease
  tools, and the Node/Python/C++/JVM host-native wrappers preserve the structured token. The
  `lock-fence` vectors cover embedded and external authority packing. The generic coordination
  `FenceToken(u128)`, text authority identity, and `u64` authority epoch contracts remain separate
  and must not be narrowed, hashed, truncated, or repacked by the public lock projection.
- (P0, blocked) Add v1 hot-restart for volatile daemon registers: sessions, pins, waiters, held
  locks, and related runtime records. Restore must be same-store, same-coordinator,
  same-authority-epoch, protocol-compatible, authorization-valid, and fence-high-water-safe;
  otherwise the daemon discards the volatile snapshot and starts with only durable high-waters.
  Blocker: the hot-restart contract needs a dedicated pre-implementation assessment before source
  changes so the snapshot owner, local persistence shape, coordinator identity, authority epoch,
  authorization proof, waiter semantics, and mismatch behavior are pinned in this owning spec and
  aligned with 0036a. Queue 12 closed its row 80 implementation slice in favor of the broader
  implementation-plan assessment.
- (P1, partially source-backed) Binding projection exposes source-backed daemon client operations and
  lock operations through the C ABI plus Node, Python, C++, and JVM host-native wrappers. Lock
  refresh/release use canonical low/high limbs at the ABI boundary, and returned tokens expose
  authority, epoch, and sequence. Do not duplicate the daemon wire protocol in individual bindings.
  Daemon start/stop/restart remains CLI-owned unless a platform deliberately defines an embedded host
  lifecycle outside the CLI daemon. Closed-filesystem and browser daemon/lock wrappers remain target.
- (P1) Expose the public `lock` facade through hosted protocols after daemon projection,
  authorization checks, hosted wait queues, and conformance runners land.
- (P1) Expose the same scoped/RAII binding helpers in future generated bindings and hosted bindings.
  Source-backed Node, Python, C++, and JVM host-native bindings already expose scoped cleanup helpers,
  explicit acquire, try-acquire through `wait_ms = 0`, refresh, and release-by-token over the daemon
  client/C ABI path.
- (P2) Add hosted CP authority support after the embedded coordinator contract is source-backed end to
  end.

## 12. Non-goals

- No cross-workspace locks (0014 forbids cross-workspace operations).
- No mutual exclusion across asynchronously-syncing replicas or across branches (§1).
- No versioned/syncable lock state of any kind (§3).
- No unbounded blocking acquire in the served surface, and no blocking acquire inside programs (§7).
