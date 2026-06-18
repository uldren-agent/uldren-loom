# 0019 - Key-Value Layer

**Status:** Partial, current KV substrate and public facade source-backed. **Version:** 0.1.0.
**Capability:** `kv`.

This spec defines the key-value facet: versioned maps from typed, order-preserving keys to opaque byte
values. Current source implements the Rust substrate in `loom-core::kv`, the workspace-scoped public
facade (`kv_put`/`kv_get`/`kv_delete`/`kv_list`/`kv_range`), the language-neutral IDL shape, the C ABI
and C header projection, all eight language bindings, C ABI tests, hosted REST/JSON-RPC/gRPC data
facades, a structured prolly-map root over content-addressed value components, and executable facade
and storage behavior runners in `loom-conformance`. Hosted KV management, semantic per-key merge
tooling, and compare-and-set remain target work.

Every operation is scoped to one workspace's KV facet. Cross-workspace KV writes are out of contract
and must fail with `CROSS_WORKSPACE` once a public facade exposes them.

An ephemeral, non-versioned, non-synced **cache tier** over this same key/value/range surface -
TTL, idle-TTL, eviction, capacity bounds, and optional write-through/-behind to a versioned map
(the Memcached-shaped use case) - is specified as a tier modifier in **0019a**. It is the same
control-plane storage class as the lock register in 0036 section 3, not a branch of the versioned tree.
Redis and Memcached compatibility are specified in **0019b** as dedicated served presentations, not as
transports under the base `kv` facade.

## 1. Current Implementation

`loom-core::kv` implements:

- `KvMap::new`;
- `len` and `is_empty`;
- `put(key, value)`;
- `get(key)`;
- `delete(key)`;
- `iter()`;
- `range(lo, hi)`;
- canonical `encode` and `decode`;
- `replace_kv_map(loom, ns, name, map)`;
- `get_kv(loom, ns, name)`.

Keys are `tabular::Value` values, which already carry a deterministic total order shared with the
tabular layer. Opaque byte keys use the `Bytes` value variant. Values are opaque byte strings.

`iter` returns entries in key order. `range(lo, hi)` is half-open: `lo <= key < hi`. `put` replaces an
existing value at the same key inside the in-memory map. `delete` returns whether the key existed.

The workspace-scoped public facade adds `kv_put`, `kv_get`, `kv_delete`, `kv_list`, and `kv_range`,
each selecting a workspace by UUID or name (`kv_put` ensures the `kv` facet) and operating on a named
map. An absent key or map reads as absent (`None`/empty), not an error. Keys cross the public boundary
as one Loom Canonical CBOR typed cell each (the SQL cell codec, via `key_to_cbor`/`key_from_cbor`);
`kv_list`/`kv_range` return the canonical CBOR array of `[key, value]` pairs in key order. Storage uses
the tabular order-preserving key encoding under the prolly tree so raw byte order matches
`tabular::Value` order.

`kv_put` and `kv_delete` are key-scoped mutations. Their exact tokens are isolated to the selected
key, so a mutation to another key does not invalidate them. `replace_kv_map` is a separate atomic
whole-collection replacement operation. It advances every retained entry anchor and invalidates every
exact token in that collection. There is no ambiguous whole-map `put_kv` compatibility alias.

The source-backed public projection currently includes:

- IDL `interface Kv` with `put`, `get`, `delete`, `list`, and `range`;
- C ABI `loom_kv_put`, `loom_kv_get`, `loom_kv_delete`, `loom_kv_list_cbor`, and `loom_kv_range_cbor`;
- C header declarations for those functions;
- CLI `loom kv put|get|delete|list|range`;
- bindings in Node, Python, C++, Swift/iOS, JVM, Android, React Native, and WASM;
- MCP data tools `kv_put`, `kv_get`, `kv_delete`, `kv_list`, `kv_range`, and
  `kv_list_collections`, routed through the engine PEP and covered by MCP surface/schema tests.

Hosted KV REST, JSON-RPC, and native gRPC are source-backed for the current data facade. The protocol
conformance crate certifies the current REST and JSON-RPC put, get, delete, list, and range route set.
Hosted KV management projection, Redis and Memcached compatibility, collection discovery, broader
protocol conformance, and compare-and-set remain target work.

## 2. Current Storage Shape

The KV facet path is:

```text
/.loom/facets/kv/<name>
```

`replace_kv_map` creates the KV facet directory and writes a structured map root at that path through
the workspace working tree. The root points at content-addressed value components under:

```text
/.loom/facets/kv/.values/<hex(collection)>/<hex(value-digest)>
```

`get_kv` reads the root, verifies each component's declared digest and length, and reconstructs the
logical ordered map. Workspace commit, branch, checkout, bundle sync, and clone see the KV root and
value components as ordinary committed content under the KV facet.

The current committed KV map is a structured prolly map. It does not contain secondary-index or
semantic merge-metadata roots. It also does not use a dedicated KV object type.

## 3. Current Encoding

The durable storage root writes:

```text
["loom.kv.prolly-map-root.v1", digest-algo-code, entries-root, anchors-root]
```

`entries-root` and `anchors-root` are optional 32-byte prolly root digests. Both prolly trees are keyed
by the tabular order-preserving encoding of one typed key. Each storage entry value is:

```text
[key, value-digest, value-length]
```

Each anchor entry value is:

```text
[key, anchor]
```

`key` uses the tabular cell-value codec. `value-digest` is the 32-byte content digest of the value
using the workspace store's digest profile. `value-length` is the byte length of the component. The
root retains anchors for live and deleted keys so exact tokens remain monotonic across delete and
reinsert cycles. On decode, key-codec mismatches, corrupt roots, missing components, digest
mismatches, and length mismatches fail before the logical map is returned.

The current writer emits only `loom.kv.prolly-map-root.v1`. Storage roots with other schema tags are
rejected. Logical `KvMap::decode` still accepts whole-map bytes for API payload compatibility, but
persisted collection storage is the prolly root only.

The logical map export remains `KvMap::encode`, which writes `[2, entries, anchors]`, where both
`entries` and `anchors` are arrays sorted by typed key. Each logical entry is:

```text
[key, value]
```

`key` uses the tabular cell-value codec. `value` is a byte string. Current source does not encode a
declared key type, secondary indexes, or conflict metadata.

## 4. Current Versioning and Merge Behavior

Current KV maps version with the workspace because they are written into the workspace working tree.
A commit snapshots the KV blob with every other staged workspace path. `checkout_commit` and
`checkout_branch` restore the KV blob with the rest of the workspace tree.

Current source does not implement semantic per-key merge. `Loom::diff_commits` exposes key-level
structural diff for KV collections: each added, removed, or changed key is emitted as a `kv` unit
with unit kind `key`, the public canonical typed-cell key bytes from `kv::key_to_cbor`, and before
and after content digests for the value bytes. The promoted prolly root lets this classification be
computed from ordered key records rather than treating the collection root as an opaque same-path
blob. A promoted KV-specific merge helper is still required before sync may merge divergent key
histories. Sync follows `CONFLICT-RESOLUTION-MATRIX.md`: branch/ref divergence uses the S1
fast-forward boundary, and per-key S2 merge is explicit target work. This is not closure-blocking for
current source because the VCS merge path does not silently merge divergent KV roots; unresolved
same-path collection edits remain conflicts until an explicit semantic KV merge primitive is added.

Mutating one value creates or updates its content-addressed value component and rewrites the changed
prolly leaf and ancestor path while digest-equal subtrees remain shared.

## 5. Current Conformance

`loom-core::kv` has unit tests for:

- typed key ordering;
- point get;
- delete;
- half-open range scans;
- canonical encode/decode;
- structured prolly root bytes and value components;
- key-map structural sharing after a one-key update;
- malformed value component rejection;
- logical whole-map payload compatibility;
- commit and checkout versioning.

`loom-conformance` contains both KV behavior scenarios, the executable `run_kv_facade_behavior`
runner, and canonical KV map vectors that assert the prolly root header, storage cardinality,
logical canonical bytes, logical whole-map payload compatibility, commit/checkout versioning, and
clone reachability.
Those checks are wired into `certify_memory_store` as the `kv` executable suite.

## 6. Target Contract

The target public KV facade should provide:

- get;
- put;
- delete;
- ordered scan;
- explicit range bounds;
- optional predicate filtering over typed keys. Opaque value bytes are not inspected by the KV
  predicate surface; declared value-schema filtering would be a separate schema-owning extension;
- key-level diff;
- explicit per-key merge tooling;
- optional compare-and-set behavior if required by served write paths.

Before hosted management and full enterprise promotion, the remaining facade needs:

- hosted management protocol methods in 0008;
- stable error mapping through `loom_core::error::Code`;
- access-control review for served KV writes;
- clear file-projection behavior for `/.loom/facets/kv/...`;
- same-key collision behavior aligned with `CONFLICT-RESOLUTION-MATRIX.md`.

### 6.1 Conditional mutation and comparison anchors

The KV facade consumes the conditional-mutation contract owned by 0003 section 9.1. For a single-key
mutation, the comparison anchor is the current canonical state of that key in the owning map, or an
owner-issued opaque revision for that state. An ordered multi-key compare-and-apply operation has one
declared map revision and key set as its atomic scope; it is a 0003 section 6 batch transaction, not a
sequence of independent conditional writes.

Single-key puts and deletes consume `any`, `absent`, `exact`, and `generation`. A KV operation does not
consume `operation_anchor` unless a promoted coordination facade explicitly protects that operation.
KV does not define a universal serialization for an `exact` value or generation: a native or hosted
facade may expose an opaque client token, but Redis, Memcached, or etcd compatibility tokens are facade
adapters and do not define the KV contract.

Conditional mutation does not merge same-key values. A satisfied condition applies one replacement or
delete atomically; a failed condition leaves the map unchanged. Per-key merge remains the explicit
target tooling in this spec, with its own conflict policy. Authorization, redacted audit evidence, and
the `ALREADY_EXISTS`, `CONFLICT`, `CAS_MISMATCH`, and `FENCING_STALE` outcomes are inherited from 0009
and 0003 section 8. A caller never receives the protected current value merely because its comparison
failed.

## 7. Target Storage Contract

The current storage slice is a structured prolly KV root, not one whole map blob. The enterprise
target continues from that root toward large-value chunking and semantic merge metadata, using
existing object types and deterministic encodings:

| Role | Target encoding | Status |
| --- | --- | --- |
| KV map | Structured root over content-addressed value components | Source-backed as migration input |
| Range-partitioned KV map | Prolly tree keyed by encoded `tabular::Value` | Source-backed |
| Large values | ChunkLists referenced from map values | Target |
| Tombstone metadata | Optional records for merge or history views | Target |
| Map root | Root manifest referencing map component roots and metadata | Source-backed for the prolly root slice; fuller tree metadata remains target |

The exact typed-key byte encoding for the prolly tree is source-backed by the tabular key encoding.
Value chunking threshold, tombstone policy, and conflict metadata must be pinned before those storage
features become identity-affecting. If those choices change canonical bytes, update conformance
vectors with the implementation.

## 7a. Public Key Encoding Decision

**Decision.** Keys are handled with two encodings at two layers, not one: a typed cell at the public
boundary (call it form A) and an order-preserving byte string at the storage layer (form B). This is
"A and B," not "A or B." Form A and form B are source-backed.

- **Public boundary (form A, current).** A key crosses the IDL, C ABI, and every binding as one Loom
  Canonical CBOR typed cell (the same cell codec the SQL result path uses), via `key_to_cbor` /
  `key_from_cbor`. Comparison and range ordering come from the `tabular::Value` total order evaluated in
  the engine, so `Int(2)` precedes `Int(10)` (numeric, not lexicographic). No byte-comparable key form
  is exposed.
- **Storage layer (form B, source-backed).** The prolly root uses an order-preserving byte encoding of
  each key so that a plain `memcmp` of the encoded bytes reproduces the `tabular::Value` order. It does
  not change form A.

**Rationale.** The systems with typed, range-capable keys converge on this two-layer split rather than
a single encoding:

- **MongoDB** is the direct precedent. Its public/query layer is typed BSON with a defined cross-type
  sort order (form A); its WiredTiger index layer encodes keys as **KeyString**, an order-preserving
  byte serialization so `memcmp` reproduces BSON order (form B). It runs both, at different layers.
  Cassandra clustering keys and FoundationDB's Tuple layer follow the same pattern.
- **Hazelcast** keys are serialized objects used for identity/partitioning (hash, not order); sorted and
  range access is a separate concern over typed `Comparable` attributes - effectively form A at the
  query surface, with no exposed byte-ordered key form.
- **Redis** and **Memcached** never face this choice: keys are opaque byte strings. Memcached has no
  ordering or range queries at all; Redis orders only inside sorted sets, by raw `memcmp` over member
  bytes. They have form B only because they never had typed keys to begin with - the opposite end of
  the design space from a typed, ordered KV facet.

The reason B exists at all is that a byte-ordered tree must compare keys by their encoded bytes, so the
encoding has to agree with the logical type order. Loom keeps form A at the API and form B under the
storage root, matching MongoDB's own trigger for KeyString while avoiding a public wire commitment to
the storage encoding.

## 8. Relationship to Other Facets

- **Tabular:** KV keys use `tabular::Value` ordering. The KV facet is the bare ordered map; relational
  schema, SQL, joins, and secondary indexes belong to the SQL facet.
- **Document:** document collections may use KV-like storage later, but document IDs, payloads, and
  indexes are owned by 0020.
- **Compute:** `loom-compute` has a `Kv` capability tag, but KV state access from programs is target
  work until 0015 defines and implements it.

## 9. Non-Goals and Limits

- Current source is not a Redis or Memcached compatibility service.
- Current source does not provide secondary indexes.
- Current source does not provide per-key merge.
- Current source does not provide compare-and-set.

## 10. Unfinished Work

| Order | Parent | Work item | Status | Exit criteria |
| --- | --- | --- | --- | --- |
| T1 | RD5 | Spec/source reconciliation | Complete local | Current implementation text distinguishes the implemented local facade, CLI, and MCP data projection from hosted and storage work. |
| T2 | RD5 | KV data CLI projection | Complete local | `loom kv ...` commands expose put, get, delete, list, and range with canonical CBOR key and result forms. |
| T3 | RD5 | Hosted KV wire projection | Source-backed REST/JSON-RPC/gRPC | Non-MCP REST, JSON-RPC, and native gRPC adapters expose put, get, delete, list, range, ACL behavior, and stable errors. Protocol conformance certifies the REST and JSON-RPC operation slice. Collection discovery and broader protocol conformance remain target work. |
| T4 | RD5 | Hosted KV management projection | Target | Configured map tier management from 0019a has a served policy and projection outside local management CLI and attached MCP runtime paths. |
| T5 | RD6 | Structured KV storage and per-key semantics | Partial source-backed | Structured prolly-map storage over content-addressed value components, ordered point/range reads, `diff_commits` key-unit classification, and identity-pinned conformance vectors are source-backed. Semantic per-key merge and compare-and-set remain target work. |
| T6 | 0061 | KV predicate semantics | Complete MCP | `kv_range` accepts the 0061 JSON predicate root over the typed `key` path only. Opaque value bytes remain outside predicate semantics. |
| T7 | 0019b | Redis and Memcached presentations | Target | Dedicated `redis` and `memcached` served surfaces implement their compatibility protocols without making either protocol the base KV model. |

## 11. Resolved Decisions

- **RD1 - Current storage.** Current source stores each named KV map as a structured prolly root under
  the workspace KV facet, with values stored as content-addressed components below `.values`.
- **RD2 - Current keys.** Keys are `tabular::Value` values with deterministic total ordering.
- **RD3 - Opaque keys.** Opaque byte keys use `Value::Bytes`; a separate byte-only key type is not
  needed.
- **RD4 - Range semantics.** Current range scans are half-open: `lo <= key < hi`.
- **RD5 - Public facade status.** The workspace-scoped public `kv` facade (put/get/delete/list/range)
  is source-backed across IDL, C ABI, C header, CLI, and all eight bindings, with an executable facade
  conformance runner. Hosted REST/JSON-RPC/gRPC data facades are source-backed, with REST and
  JSON-RPC operation conformance for put, get, delete, list, and range. Hosted KV management,
  collection discovery, broader protocol conformance, and compare-and-set remain target work.
- **RD6 - Merge boundary.** Key-level change classification is source-backed through
  `Loom::diff_commits`, which emits KV unit changes keyed by canonical typed-cell bytes. Semantic
  per-key merge is explicit target work. Sync does not silently merge divergent KV histories.
- **RD7 - Public key encoding (two layers, A and B).** Keys use a typed cell at the public boundary
  (form A, source-backed: one Loom Canonical CBOR cell, order from `tabular::Value`) **and** an
  order-preserving byte encoding at the storage layer (form B). This
  is "A and B" at different layers, not "A or B"; it matches MongoDB's typed-BSON API plus KeyString
  storage. Full rationale and cross-system precedent are recorded in section 7a.
- **RD8 - Collections (0042).** A KV **map** is the facet's collection (0042, invariant 0001 A7): keys
  live in a named map, addressed `kv.<map>.<key>`, and a map is the unit of ACL scope (0027) and of
  projection. Redis and Memcached are not `kv` transports; 0019b defines them as dedicated served
  compatibility surfaces that may use KV or cache internals where appropriate. The map is one flat
  level; KV does not nest collections. The facade's unit operations take the map segment ahead of the
  key. The canonical parameter name for that map segment is `collection` (0042 section 5.1), replacing
  the legacy `name`.

## Change log

### KV public facade across the stack (0007, 0010, 0024 pattern)

The KV facet's workspace-scoped public facade is now source-backed end to end, mirroring the CAS and
queue promotions:

- **Core.** `loom-core::kv` adds `kv_put`/`kv_get`/`kv_delete`/`kv_list`/`kv_range` (workspace + named
  map, absent reads as empty/None) plus `key_to_cbor`/`key_from_cbor` for the typed-cell key wire form.
- **IDL / C ABI / header.** `interface Kv` in `idl/loom.idl`; `loom_kv_put`/`loom_kv_get`/
  `loom_kv_delete`/`loom_kv_list_cbor`/`loom_kv_range_cbor` in `loom-ffi` (with a C ABI round-trip
  test) and `include/loom.h`.
- **Bindings.** Node, Python, C++, Swift/iOS, JVM, Android (JNI + KMP), React Native (TurboModule), and
  WASM. Keys/values cross as bytes (typed-cell CBOR keys); `list`/`range` return canonical CBOR pairs.
- **Conformance + capability.** `run_kv_facade_behavior` (put/get/replace/delete, typed half-open
  range, commit/checkout versioning, clone reachability) is wired into `certify_memory_store` under the
  `kv` executable suite; the `kv` capability flips from `scenario` to `executable` in the source
  registry and 0010 section 5.

Key encoding follows RD7 and the section 7a decision record: typed CBOR cell at the boundary (form A)
plus an order-preserving byte encoding at the storage layer (form B) - "A and B" at two layers, the
MongoDB typed-BSON-plus-KeyString pattern, not "A or B".
