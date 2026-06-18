# 0019b - Redis And Memcached Presentations

**Status:** Draft, partially source-backed. **Version:** 0.1.0. **Capabilities:** `redis`, `memcached`.

This child spec defines the Redis and Memcached compatibility presentations related to the key-value
family. These are served compatibility surfaces, not the base Loom `kv` facade and not transports under
`kv`.

## 1. Scope

The public served surfaces are:

```text
loom serve configure <store> redis <workspace> <keyspace> --bind <addr> [--persistence <mode>]
loom serve configure <store> memcached <workspace> <cache> --bind <addr>
```

`redis` is a holistic Redis-compatible surface. A Redis client connects to one Redis-like port and sees
one Redis command space. The adapter may use Loom KV, cache, queue, delivery, or runtime fanout
internally, but those backing choices are not projected as separate Redis ports.

`memcached` is the canonical Memcached compatibility surface. Use `Memcached` for the project/protocol
name and `memcached` for the served surface token. Avoid `memcache` as the canonical name.

## 2. Non-Goals

- Redis is not exposed as `kv --transport redis`.
- Memcached is not exposed as `kv --transport memcached`.
- Redis command families are not permanently reduced to strings because the first build slice starts
  with strings and TTL.
- Cache semantics do not remove Redis data structures. Cache only defines persistence and restart
  behavior.
- Whole-hash, whole-list, whole-set, whole-sorted-set, or whole-stream rewrites are not acceptable as
  the target enterprise implementation for Redis structures.

## 3. Redis Surface

Redis supports a broad command and data-structure model. The Loom Redis presentation must preserve that
shape even when the first shipped subset is smaller.

| Redis family | Backing direction | Initial status |
| --- | --- | --- |
| Strings | Redis string value records | First build slice |
| Key TTL and expiry | Redis key metadata plus expiry index | First build slice |
| Hashes | Field-level map keyed by `(redis-key, field)` | First structured slice |
| Sets | Member map keyed by `(redis-key, member)` | First structured slice |
| Lists | Sequence structure with stable head and tail operations | Second structured slice |
| Sorted sets | Dual index: member to score and score/member ordering | Second structured slice |
| Streams | Queue/log-backed stream entries | Explicit unsupported boundary until queue integration |
| Pub/sub | Runtime session fanout, not durable storage | Explicit unsupported boundary until fanout integration |
| Lua/functions | Compute-backed execution after security review | Deferred |

The first Redis build may return explicit Redis-shaped unsupported errors for unimplemented command
families. It must not store a complex Redis value as one opaque blob when the command family requires
sublinear mutation or counts.

## 4. Redis Persistence Modes

`redis` has an explicit persistence mode:

| Mode | Meaning |
| --- | --- |
| `versioned` | Redis keyspace state and TTL metadata are durable Loom state. Values survive restart and participate in Loom history according to the Redis substrate contract. |
| `ephemeral` | Redis keyspace state is runtime-only. Values do not survive restart. TTL, idle behavior, memory limits, and eviction are runtime policies. |
| `backed-cache` | Runtime cache behavior with configured read-through, write-through, write-around, or write-behind to durable Loom state where the backing mode is supported. |

The default mode is `versioned` for `redis` unless a later operator profile requires a different
default. Redis is a storage-capable system, so it must not be treated as cache-only.

## 5. Logical Expiry

Durable Redis expiry is logical first:

- Expiry metadata is stored with the Redis key catalog.
- Reads, scans, and counts evaluate expiry before returning results.
- Physical cleanup may lag behind logical expiry.
- A sweep may remove expired entries later, but externally visible answers must not count expired keys
  or expired members.
- Expiry itself does not require a daemon to create a commit at the expiration instant.

Examples:

| Command class | Required live-view behavior |
| --- | --- |
| `GET` | Expired keys read as absent. |
| `DBSIZE` | Counts only non-expired keys. |
| `HLEN`, `SCARD`, `LLEN`, `ZCARD` | If the top-level key is expired, return the Redis-compatible empty/absent result. Otherwise count only live structure state. |
| `SCAN` and family scans | Return only live keys or live members. |
| `TTL`, `PTTL` | Report remaining time from durable or runtime metadata. |
| `PERSIST` | Remove the key expiry metadata. |

Idle TTL is a cache/runtime policy. Durable Redis MAY support idle-like behavior later only if the
mutation semantics are explicit, because a read that refreshes idle time is a write.

## 6. Redis Substrate

The Redis substrate is a type-aware internal model:

| Component | Purpose |
| --- | --- |
| Key catalog | Maps Redis key bytes to type, expiry metadata, logical revision, and backing roots. |
| Expiry index | Orders expiring keys for efficient sweep and live-view filtering. |
| String records | Store string payloads, with large-value chunking when needed. |
| Hash records | Store fields independently so `HSET`, `HGET`, `HLEN`, and `HDEL` do not rewrite a whole hash. |
| Set records | Store members independently so `SADD`, `SREM`, `SCARD`, and membership checks are sublinear. |
| List records | Store sequence nodes or indexed chunks so push, pop, and range do not rewrite a whole list. |
| Sorted-set records | Maintain member lookup and score/member ordering. |
| Stream records | Use queue/log semantics once the stream family is promoted. |
| Runtime fanout | Serves pub/sub without writing messages into durable storage by accident. |

The initial implementation should introduce this model even if only strings and TTL commands are
enabled. That prevents the Redis surface from becoming a thin byte-KV adapter that cannot grow into
enterprise Redis compatibility.

## 7. Memcached Surface

Memcached is a cache compatibility surface:

```text
loom serve configure <store> memcached <workspace> <cache> --bind <addr>
```

The first profile targets the Memcached text protocol and cache semantics:

| Command family | Target behavior |
| --- | --- |
| `get`, `gets`, `gat`, `gats` | Read live runtime entries, return CAS tokens where supported, and update expiry for get-and-touch requests. |
| `set`, `add`, `replace`, `append`, `prepend` | Write runtime entries with flags, expiry, and bytes, or concatenate bytes onto existing entries. |
| `incr`, `decr` | Update decimal unsigned 64-bit counter values in place; missing and non-numeric entries return stable Memcached errors. |
| `delete`, `touch` | Delete or update expiry. |
| `cas` | Compare runtime CAS token before replacing. |
| `flush_all`, `verbosity`, `stats`, `version` | Return bounded compatibility metadata and management responses. |

Memcached entries are volatile by default. Durable and backed Memcached profiles are selected only by
explicit operator policy with `loom serve configure <store> memcached <workspace> <cache> --mode
...`. `versioned` stores entries in the versioned KV tier. `read-through`, `write-through`,
`write-around`, and `write-behind` select the matching 0019a backed-cache mode for the named cache.

## 8. Relationship To Base KV And Cache

`kv` remains the Loom-native typed ordered KV facade. `cache` or the 0019a ephemeral tier remains the
Loom-native cache lifecycle. Redis and Memcached use those primitives where appropriate but present
client-native compatibility surfaces.

| Public surface | Client expectation | Backing role |
| --- | --- | --- |
| `kv` | Loom typed ordered KV | Base data facade |
| `cache` | Loom cache lifecycle if promoted as a first-class facade | Cache lifecycle facade |
| `redis` | Redis-like database | Compatibility presentation over Redis substrate |
| `memcached` | Memcached cache service | Compatibility presentation over cache semantics |

### 8.1 Conditional mutation projection

Redis and Memcached consume 0003 section 9.1 through their owning Redis substrate or cache entry
operation. Redis key revisions, WATCH or transaction state, and Memcached CAS values are product
syntax and opaque facade tokens. They do not define a universal Loom token format or alter the native
comparison meaning. The precise atomic scope remains the owning native operation: for example, a
single cache entry for Memcached or the declared Redis command or transaction boundary for Redis.

The compatibility adapter resolves and authorizes the caller, invokes the owning primitive, preserves
0009 redaction and audit rules, then maps the stable result to the product response. A failed native
comparison must leave the state unchanged and cannot disclose the protected current value or raw native
anchor. Redis and Memcached may express their own conditional success or failure replies, but they do
not introduce another condition kind or merge policy.

## 9. Implementation Slices

| Slice | Status | Exit criteria |
| --- | --- | --- |
| R1 | Source-backed | Specs and served-listener validation name `redis` and `memcached` as dedicated surfaces. |
| R2 | Source-backed | Redis substrate exists with key catalog, value type tags, expiry metadata, and live-view helpers. |
| R3 | Source-backed | Daemon-opened Redis RESP profile supports AUTH, PING, strings, TTL, live-view counts, type inspection, delete, explicit unsupported errors, durable `.loom` persistence, and reload on listener restart. |
| R4 | Source-backed | Redis hashes and sets use field/member-level substrate storage and daemon-opened RESP command subsets with durable reload. |
| R5 | Source-backed | Redis lists and sorted sets use sequence and score-index storage, daemon-opened RESP command subsets, durable per-node/per-member records, and reload after listener restart. |
| R6 | Source-backed | Redis streams use append-only queue/log records for `XADD`, `XLEN`, `XRANGE`, `XREVRANGE`, `XREAD`, and `XDEL`, with stream IDs encoded as Redis `ms-seq` values. Redis pub/sub uses runtime-only fanout for `SUBSCRIBE`, `UNSUBSCRIBE`, `PSUBSCRIBE`, `PUNSUBSCRIBE`, `PUBLISH`, and bounded `PUBSUB` introspection without durable message writes. Stream consumer groups, stream trimming, blocking stream reads, and broader option coverage remain family-specific unsupported boundaries until promoted. |
| M1 | Source-backed | Daemon-opened Memcached text protocol supports get, gets, gat, gats, set, add, replace, append, prepend, incr, decr, delete, touch, flush_all, verbosity, cas, stats, and version over volatile cache state. |
| M2 | Source-backed | Memcached `--mode` selects explicit durable or backed cache profiles while volatile remains the default. |
| C1 | Source-backed | Conformance reports and raw socket transcripts distinguish supported Redis and Memcached command families from target and unsupported ones. Local guarded client evidence is not present in the current environment because `redis-cli` and `memcached` client binaries are absent. |

## 10. Resolved Decisions

- **RD1 - Redis surface.** Redis is a dedicated served compatibility surface, not a `kv` transport.
- **RD2 - Redis persistence.** Redis supports durable and volatile profiles through explicit
  persistence policy. Redis is not cache-only.
- **RD3 - Durable expiry.** Durable Redis expiry is logical first. Physical cleanup may lag, but reads,
  scans, and counts must use a live view.
- **RD4 - Redis substrate.** Redis structures require type-aware, sublinear internal storage from the
  start. Whole-structure blob rewrites are not the enterprise target.
- **RD5 - Memcached name.** `memcached` is the canonical served surface token and protocol name.
- **RD6 - Memcached lifecycle.** Memcached defaults to volatile cache semantics. Durable or backed
  variants require explicit operator policy.
