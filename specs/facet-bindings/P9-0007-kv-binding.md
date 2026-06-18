# P9-0007 - `kv` Binding

**Series:** P9 binding plan (normative-track sub-series; Draft)
**Version:** 0.1.0-draft. **Status:** Draft. **Last updated:** 2026-07-02
**Reads first:** [`P9-0002-projection-conventions.md`](./P9-0002-projection-conventions.md), facade spec
**0019 section 4** (the `Kv` facade), [`IMPLEMENTATION-FIDELITY.md`](./IMPLEMENTATION-FIDELITY.md) (kv is closer
to an ordered RocksDB/etcd store than to Redis).

## 1. Facade surface (0019 section 4 `Kv`)

`get(key: Value) -> Option<bytes>`, `put(key: Value, value: bytes)`, `delete(key: Value)`,
`range(lo: Value, hi: Value) -> Stream<Pair{key, value}>` (ordered by typed key).

0019 and current source now use the same key model: typed `Value` keys with half-open
`range(lo, hi)` scans. Opaque byte keys use `Value::Bytes`. Public wire encodings for `Value` keys are
target work and must use the same scalar value contract as the table/result surfaces.

### 1.1 Binding Boundary

The base layer is the typed ordered Loom KV map. Native projections expose get, put, delete, and range
with stable typed-key encoding once pinned. Redis and Memcached are dedicated served compatibility
surfaces defined by 0019b, not `kv` transports. `etcd` is a first-class served surface. Couchbase KV
behavior is no longer a KV transport target; it belongs only to the deferred P3 Couchbase integrated
surface. The native document index/query prerequisite is source-backed, but the document, KV, query,
analytics, service, and client-conformance contract remains to be designed. RDB, SST, snapshots, and protocol
dumps are interchange or engine-internal formats, not canonical Loom storage. Cache eviction
structures, write-behind queues, and compatibility cursors are derived or operational state unless a
control-plane spec promotes them.

## 2. Tier-1 REST

Current source has a listener-bound hosted REST facade for configured `kv/rest` listeners. The
listener selectors bind `{workspace, collection}`, and the current routes are `POST /kv:put`,
`POST /kv:get`, `POST /kv:delete`, `POST /kv:list`, and `POST /kv:range` with JSON bodies. Typed
keys use the source-backed canonical CBOR key encoding carried as `key_hex`, `lo_key_hex`, and
`hi_key_hex`; byte values are carried as hex. This is source-backed by `crates/loom-hosted/src/data.rs`,
`crates/loom-hosted/src/serve.rs`, and the daemon listener test in `crates/loom-cli/src/daemon_cmd.rs`.

The resource-oriented target shape remains:

Facet-root `/v1/workspaces/{workspace_id}/kv`:

| Facade method | HTTP |
| --- | --- |
| `get` | `POST /kv:get` with typed key body -> `200` + value; `404` if absent |
| `put` | `POST /kv:put` with typed key and value body; optional conditional write headers |
| `delete` | `POST /kv:delete` with typed key body |
| `range` | `POST /kv:range` with typed `lo` and `hi` bounds -> NDJSON `Stream<Pair>` |

A value read MAY carry `ETag: "{value-content-digest}"` for conditional GET, but a key-to-value binding is mutable,
so it is not `immutable`-cacheable like a `cas` object.

## 3. Tier-1 JSON-RPC

Current source has `kv.get`, `kv.put`, `kv.delete`, `kv.list`, and `kv.range` over `kv/json_rpc`
listeners. Results use hex-encoded keys and values. Streaming range responses, collection discovery,
and generated JSON-RPC schema artifacts remain target work.

## 4. Tier-1 gRPC

Current source has native Loom `kv/grpc` listener support through `loom.hosted.v1.Kv`.

- `Put`, `Get`, `Delete`, and `List` are unary.
- `Range` is server-streaming.
- Keys are canonical-CBOR bytes and values are raw bytes, matching the native KV data contract rather
  than an external string-key profile.

Collection discovery, generated protobuf artifacts, and broader protocol conformance remain target
work.

## 5. Tier-1 MCP

- **Read tools (always on):** `kv.get`, `kv.range`.
- **Write tools (token-gated, P9-0002 section 5):** `kv.put`, `kv.delete`.

## 6. Tier-2 foreign adapter

`kv` has multiple presentation classes. The base Loom model remains a typed ordered key-value map; no
single external protocol becomes the storage model.

- **Redis-compatible surface.** Redis is a dedicated `redis` served surface, not a `kv` transport. The
  Redis adapter owns one Redis-like command space, a type-aware Redis substrate, explicit persistence
  modes, logical expiry, and live-view counts. It may use KV, cache, queue, delivery, or runtime fanout
  internals where appropriate, but clients should not need to connect to separate KV/cache/queue Redis
  ports to get Redis semantics. See 0019b.
- **Memcached-compatible surface.** Memcached is a dedicated `memcached` served surface, not a `kv`
  transport. Its default lifecycle is volatile cache semantics over the 0019a cache tier. CAS-token,
  expiry, eviction, and slab behavior are compatibility semantics, not Loom identity or storage
  semantics. Use `Memcached` for the project/protocol name and `memcached` for the served token.
- **etcd v3 gRPC-style profile.** `etcd` is a dedicated served surface, not a `kv` transport. The
  source-backed single-authority profile maps gRPC `Range`, `Put`, `DeleteRange`, selected `Txn`
  compare/apply behavior, `Compact`, `LeaseGrant`, `LeaseKeepAliveOnce`, and `LeaseRevoke` to Loom
  ordered ranges plus durable sidecar metadata for revisions, compacted revision, per-key
  create/mod/version counters, lease-owned keys, and replayable watch events. Bounded Watch replay is
  source-backed; cluster, auth, and maintenance services are registered with stable `UNIMPLEMENTED`
  responses. Live Watch tailing, member, cluster, auth, maintenance, and quorum-administration
  implementations remain target work.
- **Couchbase KV-style profile.** Treat direct key-value access as a Couchbase integrated-surface concern,
  not as a `kv` transport. Couchbase as a product is primarily covered by the document binding and remains
  P3/spec-owned. Native document indexes/query are source-backed; the unresolved work is the integrated KV,
  document, SQL++ query, analytics, service, authentication, and client-conformance contract.

All compatibility presentations are capability-gated and native-only. Default ports follow the selected
presentation (`redis` 6379, `memcached` 11211, etcd 2379).

## 7. Errors / parity / concurrency

- **Errors:** reuse 0008 section 6 (`NOT_FOUND` on a missing key). No new codes.
- **Parity (0032):** fully portable (no engine dependency, 0019 section 5); KV compatibility servers are
  native-only.
- **Concurrency:** single writer; the same-key cross-peer write-collision policy is **unresolved**
  (deferred to `CONFLICT-RESOLUTION-MATRIX.md`) - the binding inherits whatever that decides.

## 8. Resolved Decisions and Open Questions

### RD-K1 - Typed keys and half-open ranges

0019 follows the source-backed model: typed `Value` keys and half-open `range(lo, hi)` scans. Opaque
byte keys are represented as `Value::Bytes`. Public wire and binding projections must define a stable
typed-key encoding rather than silently reducing KV to byte-prefix scans.

### RD-K2 - KV has multiple presentation classes

The resolved direction is not to choose one protocol as the KV model. Redis and Memcached are dedicated
served compatibility surfaces tracked in 0019b. `etcd` is a first-class served surface. Couchbase KV
behavior is not a KV transport target; it belongs to the deferred P3 Couchbase integrated-surface design.
Implementation order belongs in Queue 2 and should be chosen by compatibility value and conformance cost,
not by making one reference protocol canonical.

### RD-K3 - etcd promotion

`etcd` is a first-class served surface. Its initial profile is one Loom authority, not a simulated
cluster. Current source backs `loom serve configure app.loom etcd <workspace> <collection> --bind
127.0.0.1:2379` with `tcp` as the default transport and gRPC method paths for `etcdserverpb.KV`
`Range`, `Put`, `DeleteRange`, `Txn`, `Compact`, and `etcdserverpb.Lease` `LeaseGrant`,
`LeaseRevoke`, `LeaseKeepAliveOnce`, and bounded `etcdserverpb.Watch` replay. The implementation stores
etcd revision, compacted revision, lease metadata, and replayable events in reserved sidecar KV
collections so the served collection still exposes raw values to native Loom KV. Cluster, auth, and
maintenance services are registered with stable `UNIMPLEMENTED` responses. Hosted conformance rows
distinguish the supported KV/Lease/Compact/bounded-Watch profile from degraded single-authority
behavior, target live Watch tailing and member-class APIs, and unsupported multi-member Raft quorum
behavior. Live Watch tailing, member, peer-replication, cluster, auth, maintenance, and
quorum-administration implementations remain target work until distributed coordination backend support
exists.
