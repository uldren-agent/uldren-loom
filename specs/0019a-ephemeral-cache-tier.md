# 0019a - Ephemeral Cache Tier (KV)

**Status:** Draft. **Version:** 0.1.7. **Capability:** `kv` (tier modifier; no new capability).

This child spec extends the key-value facet (0019) with an **ephemeral, non-versioned, non-synced
cache tier** - the Memcached-shaped use case. It is expressed as a *durability tier* applied to a KV
map through configuration, not as a new facet. The same key/value/range surface of 0019 applies; only
the storage tier, lifetime, and eviction behavior change.

## Current implementation

Current source implements the core in-process cache object and the Rust public KV facade tier selector.
`loom_core::kv::EphemeralKvMap` provides non-versioned, non-synced typed-key storage with `put`, `get`,
`delete`, half-open `range`, TTL, idle TTL, and lazy expiry. `Loom::configure_kv_map`,
`kv_put_configured`, `kv_get_configured`, `kv_delete_configured`, and `kv_range_configured` route a
named map through either the default versioned tier or the runtime-only ephemeral tier. Read-through
and write-through are source-backed; write-through writes the backing versioned map synchronously
through the normal KV path, then populates the cache. Source tests and the `kv-ephemeral` conformance
runner prove expiry, configured routing, read-through, write-through, and that configured cache entries
do not become versioned unless write-through is enabled. Capacity bounds (`max_entries`, `max_bytes`)
and the eviction policy (`none`, `lru`, `lfu`, `random`) with the `on_evict` action (`drop` or
`write_through`) are source-backed in `EphemeralKvMap`: a configured map sheds entries per its policy
when a bound is hit, the durable config (including the new bounds) is persisted as a committed reserved
file under the KV facet root so it versions and syncs with the workspace (a clone reconstructs the same
cache shape), the `kv-ephemeral` conformance runner proves LRU eviction under a capacity bound, and a
source unit test proves a configured map's tier/eviction/capacity survive a clone+checkout into a fresh
coordinator. IDL, C ABI, CLI, C++,
and Swift expose the durable map tier selector through the `ManagementKv` `set_config` / `get_config`
operations. This config surface is public but administrative: it changes durability and sync behavior
for a named map, so hosted projection requires explicit ACL and tool-policy work before promotion. MCP
data operations use the configured KV facade: persistent MCP hosts preserve runtime-only cache state
for pure ephemeral maps, while stateless MCP per-request hosts reject pure ephemeral map data
operations instead of pretending entries survive between calls. Daemon-attached MCP hosts route pure
ephemeral KV put/get/delete/list/range through the CLI daemon runtime, so two attached MCP hosts share
one volatile cache authority for the same store. Source does not yet provide projection of the new
capacity/eviction knobs through the binding `set_config` surface, hosted wire projection, write-behind
queue, or hot-restart persistence.

Projection caveat: a stateless binding that reopens the Loom for every operation cannot expose pure
ephemeral maps as ordinary put/get/delete calls, because runtime-only entries would disappear between
calls. Stateless projections expose durable map configuration and versioned or write-through behavior,
but pure ephemeral map operations require a stateful session, an embedded Loom instance, or a hosted
coordinator that keeps the runtime cache alive for the caller's authority. The source-backed C ABI
handle-session bridge carries authenticated principal context across per-call opens; it does not by
itself keep an in-memory ephemeral map alive.

## 1. Motivation

The versioned KV facet (0019) is the wrong tool for cache-shaped data: session blobs, rate-limit
counters, fan-out caches, derived lookups, idempotency keys, short-lived coordination scratch. Those
want volatility, TTL, eviction under memory pressure, and *no* history - the opposite of a committed,
content-addressed, syncable map. Memcached maps onto KV by **shape** (typed key → opaque bytes,
get/put/delete/range) but not by **lifecycle**. This spec adds the missing lifecycle as a tier rather
than forking a second facet, so applications use one KV surface and choose durability per map.

This tier uses the same authority-local control-plane store as the lock register of 0036 §3:
per-coordinator, outside the versioned Merkle DAG, never synced. Cache entries use the volatile class by
default. The v1 lock/session hot-restart decision in 0036 does not make cache entries durable data.
Any optional hot-restart cache persistence remains local to the same coordinator and must not be
confused with versioned or synced KV content.

## 2. Tiers

A KV map declares a `tier` at creation:

| Tier | Versioned? | Synced? | Survives restart? | Use |
| --- | --- | --- | --- | --- |
| `versioned` (default, = 0019) | yes | yes (0006) | yes | source-of-truth data |
| `ephemeral` | **no** | **no** | **no** (see §5) | cache, sessions, counters, scratch |

`ephemeral` maps live in per-engine runtime state, not the working tree. The **map configuration** (tier,
TTL, capacity bounds, eviction policy) is a committed reserved file under the KV facet root, so it
versions and syncs with the workspace - a clone or pull reconstructs the same cache *shape* on the new
coordinator - while the cache *entries* themselves stay runtime-only. Ephemeral entries are invisible to
`commit`/`branch`/`merge`/`diff`/`checkout`, never appear in a bundle, and are never pushed or pulled.
A workspace rebuilt by pulling its versioned content comes up with **empty** ephemeral maps - by design
(§5).

## 3. Per-entry lifetime and eviction

`ephemeral` maps add the following to the 0019 surface:

```idl
kv.put(ns, name, key, value, opts: { ttl: Option<Duration>, idle_ttl: Option<Duration> })
// ttl: absolute expiry from write; idle_ttl: expiry after last access (Hazelcast max-idle analog)
```

Map-level configuration:

- **`default_ttl`** - applied when a `put` omits `ttl`.
- **`eviction`** - `none` | `lru` | `lfu` | `random` | `fifo` | `ttl_priority`, applied when a capacity
  bound is hit. `fifo` evicts the oldest-written entry; `ttl_priority` evicts the soonest-to-expire
  entry (entries without an expiry are evicted last). `none` leaves the bounds advisory (no eviction).
- **`max_entries`** / **`max_bytes`** - capacity bounds that trigger eviction. `max_bytes` accounts each
  entry as encoded key bytes + value bytes + a fixed per-entry overhead, tracked with an O(1) running
  counter so the bound check is constant-time.
- **`on_evict`** - `drop` (default) | `write_through` (§4).

Expiry and eviction are best-effort and lazy-plus-background: a read of an expired entry MUST behave
as `NOT_FOUND`; reclamation MAY lag. Eviction order is implementation-defined within the named policy
and is **not** a conformance-pinned object graph (there is no versioned graph for this tier).

## 4. Backing a versioned map (MapStore analog)

An `ephemeral` map MAY be configured with a `backing` reference to a `versioned` KV map in the same
workspace, giving Hazelcast `MapStore`-style flows:

- **read-through** - a miss in the cache loads from the backing versioned map and populates the cache.
- **write-through** - a `put` writes the backing versioned map synchronously, then the cache.
- **write-around** - a `put` writes the backing versioned map synchronously and does **not** populate
  the cache (it invalidates any stale entry); for write-heavy keys that are not re-read soon.
- **write-behind** - a `put` updates the cache immediately and buffers the backing-map mutation in a
  bounded, coalescing dirty queue (last write per key wins), flushed later; a crash before flush loses
  the un-flushed delta (acceptable for cache, stated loudly here).

Write modes are a precedence ladder: `write_around` > `write_behind` > `write_through` > cache-only.
Write-through, write-around, and write-behind write the backing map through the normal versioned path,
so backed writes participate in history and sync; the cache layer itself never does. Optimistic CAS
(0036 §2, `expected_version`) is the recommended way to make backed writes safe under concurrency.

### 4.1 Write-behind queue, back-pressure, and flush drive-points (source-backed)

The write-behind dirty queue is drained by an explicit drive-point, `Loom::flush_pending(ns, name,
max)` (and `flush_all_pending(ns)` for a whole workspace), which writes the coalesced mutations to the
backing map in key order. `loom-core` exposes the drive-points and stays single-threaded and
runtime-free; the **host** owns the flush cadence (a daemon flush task woken on write, a checkpoint, or
graceful shutdown - see §5 and 0019b host wiring). A stateless host that cannot keep the queue alive
between calls must not advertise write-behind and downgrades it to write-through.

A soft **high-water mark** - `flush_high_water_pct` of a capacity bound (`max_entries`/`max_bytes`;
`None` means only the hard bound, i.e. 100%) - governs back-pressure once the cache crosses it:

- **`block`** (default) - drain the whole queue synchronously before the write returns: a hard memory
  bound at the cost of unbounded write latency. Never silently loses buffered writes.
- **`pressure`** - reject the write with `LOCKED` while saturated so the caller backs off and retries:
  a hard bound with no latency penalty for accepted writes.
- **`assisted`** - flush one bounded batch (`flush_batch`) then let the write proceed: bounded latency
  and a soft bound that may briefly exceed the high-water mark under burst.

The hard capacity bound is always enforced by eviction (§3) independent of back-pressure; an evicted
entry's buffered write-behind mutation still flushes (the dirty queue is separate from cache entries),
so eviction never drops an un-flushed delta. `Loom::clear_ephemeral_cache` / `drop_ephemeral_caches`
invalidate a cache whose backing working tree changed under it (checkout/merge); callers wanting
durability `flush_pending` first. The host wiring of these (checkout/merge invalidation, daemon flush+GC
cadence, graceful-shutdown drain) is the next slice.

## 5. Durability and rebuild semantics (normative)

- **Volatile by default.** `ephemeral` state is per-coordinator runtime state. It does not survive a
  process restart unless an implementation offers an explicit local-persistence option (a hot-restart
  store), and even then it is restored only by the **same coordinator**, never transferred to another.
- **Lost on pull-rebuild, intentionally.** Reconstructing a local Loom by pulling workspaces yields
  empty ephemeral maps, because that is a *new coordinator*; inheriting another coordinator's cache or
  leases would be incorrect (cf. 0036 §3, §5 on locks).
- **Never synced (entries).** `push`/`pull`/`clone`/bundle never carry ephemeral cache *entries*. The
  map *configuration* (tier, TTL, capacity bounds, eviction policy) is the exception: it is a committed
  reserved file and travels with the workspace, so a clone or pull reconstructs the same cache shape
  (with empty entries).

Durable data belongs in the `versioned` tier (optionally fronted by an `ephemeral` cache via §4).

### 5.1 Coordinator lifecycle (source-backed core; host cadence pending)

- **Invalidate on working-tree replacement.** A `checkout` (commit or branch), a fast-forward or clean
  merge, an entered merge-conflict state, and a merge abort all replace the workspace working tree, so
  the engine drops that workspace's ephemeral caches - entries and buffered write-behind deltas. A read
  after the tree changed therefore reflects the new tree (no stale cache hit). Uncommitted write-behind
  deltas are dropped here, consistent with the volatility contract; a host wanting them persisted
  `flush_pending` first.
- **GC sweep drive-point.** `Loom::sweep_expired(ns, name, now)` / `sweep_all_expired(ns, now)` reclaim
  expired entries proactively, returning the count. Reads still expire lazily (§3); the sweep bounds
  memory on a quiet cache. `loom-core` exposes the drive-point and stays single-threaded; the host owns
  the cadence.
- **Stateless downgrade.** `KvMapConfig::for_stateless()` maps write-behind to synchronous write-through
  so a stateless per-request host - which cannot keep the runtime queue alive between calls - never
  buffers a delta it would then lose. A stateful host (embedded or daemon) uses the config unchanged.
- **Host cadence.** The CLI daemon consumes the GC drive-point: it calls `sweep_all_expired` for a
  workspace on each access (`with_pure_ephemeral_kv`), so a long-lived daemon bounds memory between
  reads. The daemon hosts **pure-ephemeral** maps only (it rejects `read_through`/`write_through`), so
  there is no dirty queue to flush and no durable state to drain - its caches are volatile and dropped
  when the process exits, satisfying the shutdown contract by construction. Write-behind flush is driven
  by the **embedded** host through `flush_pending`/`flush_all_pending` (the drive-points landed in the
  lifecycle-core slice); a future daemon that hosts backed maps would call `flush_all_pending` on
  graceful shutdown. `loom-core` is wasm/stateless and stays lazy-only.

## 6. Concurrency

The 0003 §9 model is unchanged. Within one coordinator, `ephemeral` puts are last-writer-wins at
operation granularity; `expected_version` CAS (0036 §2) is available for atomic counters and
read-modify-write. Cross-coordinator coordination is out of scope - an ephemeral map is local to its
coordinator and is not a coordination channel.

### 6.1 Conditional mutation and comparison anchors

The cache tier consumes 0003 section 9.1 without making volatile entries a versioned or synchronized
resource. The anchor for an entry mutation is the local coordinator's current entry generation. The
atomic scope is one entry and its expiry or eviction decision at that coordinator. Cache entry mutations
consume `any`, `absent`, `exact`, and `generation`; `operation_anchor` is not a cache-entry condition.
Backed writes use the backing KV map's anchor and atomic scope, including its batch rules, rather than
the cached copy.

Cache entries do not merge. A failed comparison, expiry, eviction, working-tree replacement, or cache
clear invalidates the stale entry and cannot make an old token succeed. The cache exposes no canonical
entity tag. A presentation may expose an opaque local generation or adapt a protocol CAS token, but
Memcached and Redis do not define cache semantics. Authorization and redacted audit follow 0009; errors
and safe compare outcomes follow 0003 section 8.

## 7. Non-goals

- No versioning, diff, merge, blame, or `as_of` for `ephemeral` maps.
- No sync, bundle, or clone of ephemeral state.
- No cross-coordinator visibility or coherence (it is a local cache, not a distributed cache; a
  distributed cache would be a hosted-provider feature layered on the §4 backing map).
- No conformance-pinned eviction order.
- No Redis or Memcached compatibility protocol definition. 0019b owns those served presentations.

## 8. Resolved decisions

- (P1) `ephemeral` is a per-map tier. Per-key overlays on a versioned map are not part of v1.
- (P1, source-backed) Read-through, write-through, write-around, and write-behind are the source-backed
  backing modes, with a coalescing dirty queue, `block`/`pressure`/`assisted` back-pressure at a
  configurable soft high-water mark, and explicit `flush_pending` drive-points. The host owns the flush
  cadence; the engine stays single-threaded.
- (P2) Local cache hot-restart persistence is deferred. This is separate from the 0036 v1
  hot-restart requirement for lock/session daemon registers.
- (P2) A hosted coherent distributed cache is deferred to hosted-provider work.

## 9. Implementation slices

- (P0) Source-backed in Rust: public per-map `ephemeral` configuration is implemented in the KV facade
  and executable behavior covers TTL, idle TTL, lazy expiry, configured routing, read-through,
  write-through, and runtime-only cache state.
- (P1) Project the public tier selector through IDL, C ABI, CLI, bindings, and hosted protocols. IDL,
  C ABI, CLI, C++, and Swift local projection are source-backed for durable configuration.
- (P1, source-backed) MCP data operations honor configured KV tiers: persistent hosts preserve pure
  ephemeral entries, while stateless per-request hosts return `UNSUPPORTED` for pure ephemeral data
  operations.
- (P1, source-backed) Daemon-attached MCP hosts share pure ephemeral KV map state through the CLI daemon
  runtime. The daemon loads the persisted identity/ACL control state and applies the engine PEP for each
  request session before serving the runtime-only map.
- (P1, source-backed) Capacity bounds (`max_entries`, `max_bytes`) and the eviction policy
  (`none`/`lru`/`lfu`/`random`) with `on_evict` (`drop`/`write_through`) are implemented in
  `EphemeralKvMap` and persisted in the durable config; eviction order stays out of content-addressed
  conformance (the runner asserts the capacity bound holds and LRU sheds the least-recently-used key).
  The full config surface (capacity, eviction, `on_evict`, write modes, back-pressure, high-water,
  flush-batch) is projected through the C ABI `loom_management_kv_set_config`, the IDL `KvMapConfig`
  struct, the C header, the C++ engine wrapper, and the Swift facade; `get_config` renders every field
  as JSON. The remaining language wrappers (Node/Python/JVM/Android/RN/WASM) do not yet expose KV map
  config at all - a pre-existing gap tracked separately from this tier.
- (P1, source-backed) Write modes (through/around/behind), back-pressure (`block`/`pressure`/`assisted`)
  at a soft high-water mark, the coalescing dirty queue with `flush_pending`/`flush_all_pending`, the GC
  `sweep_expired`/`sweep_all_expired` drive-points, working-tree-replacement cache invalidation
  (checkout/merge), and the `for_stateless` write-behind->write-through downgrade are implemented in
  `loom-core` and exercised by the `kv-ephemeral` conformance runner plus source unit tests. The full
  config surface is projected through the C ABI, IDL, C header, C++, and Swift; the daemon flush/GC
  cadence and graceful-shutdown drain that consume the drive-points are the remaining host step.
- (P2) Add cache-entry hot-restart persistence, a distributed hosted cache, and capacity-policy tuning
  only after the P1 tier has operational metrics. The 0036 daemon may hot-restart lock/session
  registers without making pure-ephemeral KV entries durable.

## Change log

### 0.1.7

Daemon host wiring for the GC drive-point. The CLI daemon now calls `sweep_all_expired` for a workspace
on each KV access (`with_pure_ephemeral_kv`), so a long-lived daemon reclaims expired entries between
reads instead of growing until the next read of each key. §5.1 records the scope reality: the daemon
hosts pure-ephemeral (volatile, no-backing) maps only, so there is no write-behind queue to flush and
no durable state to drain on shutdown - its caches drop when the process exits, satisfying the shutdown
contract by construction. Write-behind flush remains driven by the embedded host via
`flush_pending`/`flush_all_pending`; a future daemon that hosts backed maps would flush on graceful
shutdown. This completes the host-cadence work for the current daemon architecture.

### 0.1.6

The full cache config surface is projected through the C ABI, IDL, header, C++, and Swift.
`loom_management_kv_set_config` gains nine arguments - `max_entries`, `max_bytes`, `eviction`,
`on_evict`, `write_behind`, `write_around`, `back_pressure`, `flush_high_water_pct`, `flush_batch` -
with stable enum tags (eviction 0..5, on_evict 0..1, back_pressure 0..2), 0 = unbounded for the `u64`
bounds, and a negative `flush_high_water_pct` meaning "only the hard bound". `get_config`'s JSON now
renders every field (capacity, eviction/on_evict/back_pressure as names, write-mode flags, high-water,
flush-batch). The IDL `KvMapConfig` struct gains the matching fields plus `EvictionPolicy`, `OnEvict`,
and `BackPressure` enums; the C header, the C++ `management_kv_set_config`, and the Swift
`managementKvSetConfig` wrappers carry the new (defaulted) parameters. The FFI test exercises and
asserts the full round-trip. The remaining language wrappers (Node/Python/JVM/Android/RN/WASM) still do
not expose KV map config - a pre-existing gap, not part of this tier. Daemon flush/GC cadence and
graceful-shutdown drain remain the one open host slice.

### 0.1.5

Conformance for the full cache surface and spec finalization. The `kv-ephemeral` executable runner now
exercises write-behind buffer-then-`flush_pending`, `pressure` back-pressure rejection (`LOCKED`) at the
high-water mark, write-around (backing written, cache not populated), the GC `sweep_expired` drive-point
(reclaim + idempotent), and checkout cache invalidation; the `EPHEMERAL_KV_SCENARIOS` table gains the
matching scenario rows. §8/§9 are updated: write modes (through/around/behind), back-pressure, the dirty
queue with flush drive-points, the GC sweep, checkout/merge invalidation, and the `for_stateless`
downgrade are all source-backed in `loom-core`. Remaining: the daemon flush/GC cadence + graceful-
shutdown drain (host) and the binding `set_config` projection of the new knobs.

### 0.1.4

Coordinator lifecycle for the cache is source-backed (new §5.1). Working-tree replacement -
`checkout_commit`/`checkout_branch`, fast-forward and clean merge (via checkout), an entered
merge-conflict state, and `merge_abort` - now drops the workspace's ephemeral caches (entries + buffered
write-behind deltas) so post-change reads reflect the new tree. New GC drive-points
`Loom::sweep_expired`/`sweep_all_expired` and `EphemeralKvMap::sweep_expired` proactively reclaim expired
entries (reads still expire lazily). `KvMapConfig::for_stateless()` downgrades write-behind to
write-through for per-request hosts that cannot keep the runtime queue alive. Unit tests cover proactive
sweep, the stateless downgrade, and checkout cache invalidation. The host cadence that consumes these
drive-points (daemon flush task woken on write, GC sweep on a timer, graceful-shutdown drain with
`SHUTTING_DOWN`) is the remaining slice; `loom-core` stays single-threaded and lazy-only.

### 0.1.3

Write modes and write-behind back-pressure are source-backed. `KvMapConfig` gains `write_behind`,
`write_around`, `back_pressure` (`block`/`pressure`/`assisted`), `flush_high_water_pct`, and
`flush_batch`; the write mode is a precedence ladder (write_around > write_behind > write_through >
cache-only). `EphemeralKvMap` gains a coalescing dirty queue (`mark_dirty_put`/`mark_dirty_delete`,
`take_flush_batch`, `pending_len`/`has_pending`, `over_high_water`). New `Loom` drive-points
`flush_pending`/`flush_all_pending`/`pending_flush_count` drain the queue to the backing map in key
order, and `clear_ephemeral_cache`/`drop_ephemeral_caches` invalidate caches whose backing tree changed.
`kv_put_configured`/`kv_delete_configured` route through the write mode and apply back-pressure at the
soft high-water mark: `block` drains synchronously, `pressure` rejects with `LOCKED` while saturated,
`assisted` flushes one bounded batch. Eviction stays independent of back-pressure and never drops an
un-flushed delta (the dirty queue is separate from cache entries). Unit tests cover dirty coalescing and
ordered draining, the high-water predicate, and write-behind buffer/flush, block-drain, assisted-backlog,
pressure-reject, and write-around. The config wire format grows from 9 to 14 fields. Host wiring of the
flush cadence, checkout/merge invalidation, graceful-shutdown drain, and the stateless write-behind ->
write-through downgrade is the next slice; the binding `set_config` projection of the new knobs follows.

### 0.1.2

The KV map configuration is now **versioned and synced** with its workspace. Previously the durable
config round-tripped only through per-coordinator engine state (`export_state`/`import_state`); it is now
persisted as a committed reserved file under the KV facet root (`.loom/facets/kv/.config/{collection}`),
so it travels through `commit`/`clone`/`pull`/bundle and a clone reconstructs the same cache *shape*
(tier, TTL, capacity bounds, eviction policy) on a fresh coordinator. The cache *entries* remain
runtime-only and unsynced, so §5's "never synced" rule now reads "never synced (entries)" with the
config as the stated exception. `configure_kv_map` reserves the `.config` collection name (rejected as a
user map name) and writes/removes the reserved config file; `kv_map_config` reads it back from the
working tree. The in-memory `kv_map_configs` engine-state map and its serde are removed. New unit test
`configured_kv_config_syncs_via_clone` proves tier/eviction/capacity survive a clone + checkout into a
fresh coordinator. Rationale: the cache's *durability contract* (what it is) must be reproducible
wherever the loom is loaded; only the volatile contents (what it holds) are local.

### 0.1.1

Eviction policy set extended to `fifo` (oldest-written) and `ttl_priority` (soonest-to-expire), joining
`none`/`lru`/`lfu`/`random`, so both read-heavy (LRU/LFU) and write-heavy (FIFO) workloads are served.
`max_bytes` accounting now counts encoded key + value + a fixed per-entry overhead via an O(1) running
byte counter (was an O(n) value-bytes-only sum), so the bound approximates real footprint and the check
is constant-time. New unit tests cover FIFO, TTL-priority, and the byte counter across insert/replace/
delete. The full composable cache surface (cache layer over the versioned tier, write modes
through/around/behind with back-pressure, versioned/synced config, daemon flush+GC, graceful shutdown)
is designed and sequenced; this entry covers the eviction-set + accounting slice.

### 0.1.0

Capacity bounds and eviction are now source-backed in the ephemeral KV tier. `KvMapConfig` carries
`max_entries`, `max_bytes`, `eviction` (`none`/`lru`/`lfu`/`random`), and `on_evict`
(`drop`/`write_through`); `EphemeralKvMap` enforces the bounds on put, evicting per policy (LRU by last
access, LFU by access count, Random by a rotating cursor; `none` leaves bounds advisory) and reclaiming
expired entries first. `on_evict = write_through` flushes evicted entries to the backing versioned map.
The new config fields round-trip through `export_state`/`import_state`, and the `kv-ephemeral`
conformance runner covers LRU eviction under a capacity bound. Eviction order within a policy is
implementation-defined and deliberately not conformance-pinned. The binding `set_config` projection of
the new knobs, write-behind, and cache-entry local persistence remain follow-on work.
