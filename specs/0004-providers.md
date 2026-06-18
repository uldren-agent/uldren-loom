# 0004 - Providers (Backends)

**Status:** Complete for current source-backed provider boundary; hosted facade split. **Version:**
0.1.0-draft. **Normative.**

A **Provider** is a backend that hosts the storage and mutable engine state needed by a Loom. The
interface (0003) and data model (0002) are identical across providers; this document defines the
source-backed object-store contract (§2.1), the source-backed `FileStore` and `BackingIo` backends
(§3), and the cross-cutting **capability** mechanism (§4). The broader hosted or generated Provider
facade is target work in 0004a.
Note (ADR-0002): the `LocalShim` from the original sketch is **dropped**, and the **`Database`
backend is likewise dropped and no longer supported** (§3). Moving data *in and out* of Loom -
importing a database table, checking a commit tree out to the filesystem, building a commit from a
filesystem tree - is **not** a provider; it is the **Interchange** layer, specified separately in
**0012**.

> **Current implementation:** the Rust trait named `ObjectStore` is intentionally lean:
> `put`/`put_hint`/`get`/`has`, size helpers, and `digest_algo`. It computes object addresses itself,
> so a caller cannot pass a mismatched digest through that trait. Workspace refs, branch CAS, working
> trees, sync, and exported engine state live above the trait in `Loom` and `Registry`; persistent
> state is anchored into `FileStore` through a single `reference_root`. `crates/loom-store::FileStore`
> implements the persistent object-store path, read-only opens, single-writer locking, compression,
> encryption-at-rest, identity-profile digest algorithms, compaction, and segment GC. Existing
> conformance covers memory stores, `FileStore`, BLAKE3 vectors, and SHA-256 profile vectors. The full
> provider facade is target work in 0004a unless source exports it.

## 1. Provider model (informative overview)

```
        Loom (0003 interface)
              │  depends on ↓
        ┌─────────────────────────────┐
        │  ObjectStore + engine state  │
        └─────────────────────────────┘
                ▲                         ▲
            FileStore                 target hosted provider facade (0004a)
```

The engine is written against `ObjectStore` for immutable object bytes and owns higher-level mutable
state itself. The target hosted provider facade keeps the same separation of semantics from storage
while adding service-grade lifecycle, capability, ref, and remote surfaces in 0004a.

## 2. Provider contract

### 2.1 Source-backed `ObjectStore` contract

The checked-in Rust provider boundary is `loom_core::provider::ObjectStore`:

```rust
trait ObjectStore {
  put(canonical: &[u8]) -> Digest
  put_hint(canonical: &[u8], hint: CompressionHint) -> Digest
  get(digest: &Digest) -> Option<Vec<u8>>
  has(digest: &Digest) -> bool
  len() -> usize
  is_empty() -> bool
  digest_algo() -> Algo
}
```

Source-backed requirements:

- **R1 - Address computation on write.** `put` and `put_hint` compute the digest from the supplied
  bytes under the store's identity-profile digest algorithm. The caller does not provide the digest.
- **R2 - Idempotent writes.** Writing bytes that already exist is a no-op returning the same digest.
- **R3 - Profile honesty.** `digest_algo()` returns the algorithm every object address in this store
  uses. The default is BLAKE3; `FileStore` can be created with SHA-256 for the FIPS profile.
- **R4 - Compression is storage policy.** `CompressionHint` may influence the stored frame, but never
  the digest.
- **R5 - Missing reads are explicit.** `get` returns `None` for a missing digest and an error only for
  real I/O, integrity, encryption, or internal failures.
- **R6 - Higher layers own refs and canonical shape.** The low-level trait stores bytes by digest. The
  object model and VCS layers create canonical object bytes and manage workspace refs, branches, tags,
  working trees, and exported engine state.

**Source anchors:** `crates/loom-core/src/provider.rs` defines this trait. `MemoryStore` implements it
as the deterministic in-memory reference. `FileStore` implements it over the `.loom` page engine and
passes the shared conformance vectors, including profiled BLAKE3 and SHA-256 object-model vectors.

### 2.2 Target hosted provider facade

The target enterprise Provider interface combines the source-backed `ObjectStore` with lifecycle,
capability, ref, transaction, and remote-service responsibilities. That interface is specified in
0004a so it can mature with 0008 hosted protocols, 0010 capability reporting, and 0026-0028
authorization without blocking this source-backed provider contract.

0004a retains these target requirements: digest verification for APIs that accept caller-supplied
digests, idempotent object writes, atomic durable ref compare-and-swap, reachability-safe deletion,
capability honesty, and identity-profile exposure for sync negotiation.

## 3. The backends

Each backend is given a name, a one-line essence, a storage layout, its capability profile, and
its caveats. Detailed byte formats for the single-file backend are in 0005; sync is in 0006.

> **Removed: `LocalShim`.** The original sketch included a provider that ran the interface on top of
> a normal filesystem + normal git. It is **dropped** (ADR-0002): maintaining a faithful FS+git
> backend alongside the native object model is high-cost and tangential to the goal (the `.loom`
> file and its features). The legitimate need it served - getting data *in and out* of Loom - is
> met by the **Interchange** layer (**0012**), which is a set of import/export operations, not a
> storage backend.

### 3.1 Backend 1 - `SingleFile` (an entire versioned filesystem inside one file)

**Essence:** A complete Loom packed inside **one** regular file (`*.loom`). No directory sprawl; the
whole repository, including history and exported engine state, is one portable artifact. This is the
flagship backend and the canonical home of Loom's own Merkle implementation.

**Layout:** Defined byte-for-byte in **0005**. In brief:
- Two superblock slots and a journal ring form the crash-recovery commit point.
- Object records live in fixed-size pages, using packed slabs for small records and page runs for
  larger records.
- The digest-to-record locator index is a copy-on-write B-tree rooted from the superblock.
- The committed engine-state root is a single `reference_root`; `Loom::save_state` stores the
  workspace registry, branches, tags, working trees, and content map as independently rooted state
  sections under a regular Tree object.
- A persisted free-page map supports reuse after the crash-safe recovery window.
- Optional per-object compression and encryption-at-rest are self-describing frame properties.
- The store's identity-profile digest algorithm is stored in the superblock and is immutable after
  creation.

**Working tree:** virtual. Files are represented in the engine working tree and materialized by the
facades, not by a host directory backend. Read-only opens are lock-free. Native writable opens take a
single-writer advisory lock; non-file backings use their host coordination.

**Source-backed capability profile:** object `random-read`, object `random-write`, page-engine GC,
native compaction, compression, encryption-at-rest, atomic store commits, native single-writer
locking with concurrent read-only opens, bundle import/export through `loom-core::sync`, BLAKE3 and
SHA-256 identity profiles, caller-supplied backing I/O for browser or in-memory hosts, and executable
conformance vectors for the current object-store contract.

**Target capability profile:** public capability reporting, reflog, pinning, commit-graph
acceleration, signing, pack-split sibling files, remote provider projection, and generated provider
metadata are 0004a or owning-spec work unless source exposes them.

**Caveats:** A single huge file can be less convenient for some OS tools than a directory. Pack-split
remains target work, not current source behavior.

> Use case: "ship a whole versioned dataset/site/repo as a single encrypted file"; embedded and
> mobile apps; reproducible artifacts; the unit of the file-to-file sync demo (§3.2).

### 3.2 Backend 2 - `Sync` - synchronization between any two Looms

**Essence:** Not a storage backend but a **relation** between two Looms: reconcile and transfer
objects + refs so a destination gains the history a source has (`push`/`pull`/`fetch`/`clone`).
Because every backend presents the same content-addressed model (0001 A1), sync is uniform across any
promoted supported pair: file-to-file, local-to-remote, browser-to-remote, and so on.

**Specified in 0006.** Source currently implements direct in-process sync between Loom values and
bundle export/import. A remote-provider adapter that wraps a Loom over transport (0008) remains 0004a
target work. Thus "remote Loom" is a target provider shape; "synchronization" is the algorithm
operating between a local provider and a peer provider, or between two local Looms.

**Canonical demo:** `SingleFile` to `SingleFile`. Two `.loom` files synchronize by exchanging
have/want sets and transferring only missing objects (0006 §4), with full integrity verification on
receipt (R1).

**Capability profile:** current direct sync requires matching identity-profile digest algorithms and
transfers reachable objects. Full capability negotiation remains target work in 0006/0010.

> **Removed: `Database` backend.** An objects-and-refs-in-SQL/KV backend was previously sketched here
> (SQLite/Postgres, or RocksDB/redb, hosting object and ref storage for hosted/multi-client service). It is
> **dropped and no longer supported** [owner-decided]: `SingleFile` (§3.1) is the canonical store, and
> `Sync` (§3.2) plus the wire protocols (0008) cover the hosted/remote case, so a second full storage
> backend is not worth maintaining. (Unrelated: the `sql` *facet* (0011) - GlueSQL tables stored
> *inside* a Loom - is data in the object model, not a backend that hosts the object model.)

### 3.3 Capabilities layer (cross-cutting, not a standalone store)

The capability spectrum from the project brief - *encryption at rest, compression, security, and
"anything we can superimpose"* - is **not** a separate storage backend. It is a cross-cutting layer
over provider and engine surfaces. Implemented items are source-backed in `FileStore` and
`loom-core`; target items are specified in **0009** and related specs. Listed here for the map:

- **Encryption at rest** - implemented in `FileStore` frames with wrapped data-encryption keys.
  Hardware/KMS unlock providers and end-to-end encrypted sync are target work (0009).
- **Compression** - implemented as per-object frame policy. The digest is over plaintext bytes, so
  compression never changes identity.
- **Integrity and signing** - Merkle verification is implemented by digest-addressed reads and
  canonical object validation. Signed commits/tags and transparency logs are target work.
- **Access control** - principal-aware authorization is target work in 0026-0028.
- **Lifecycle** - compaction, page-engine GC, rekey, and store recovery are implemented. Retention
  holds, redaction policy, and audit logs are target work.
- **Advanced** - sparse/partial checkout, shallow history, transparent LFS, CRDT multi-writer,
  FUSE/NFS mounts, and time-travel reads are target work.

The target hosted provider facade may expose these as composable interceptors. Source today
implements compression and encryption inside `FileStore` rather than through a generic
`EncryptingProvider` wrapper.

### 3.4 Backend 3 - browser backing (`BackingIo` / OPFS)

**Essence:** The browser-resident persistent object store, for the `wasm32` build where `std::fs`
(and therefore `SingleFile`) is unavailable. It hosts the same content-addressed object model as
`SingleFile`, persisted in the browser's own durable storage so a Loom survives a page reload.

**Current implementation:** `FileStore` runs over a pluggable `BackingIo`. The wasm binding includes
an OPFS `FileSystemSyncAccessHandle` implementation of that backing and exposes an OPFS SQL session,
queue helpers, encryption open/create paths, read-only snapshots, and an OPFS conformance digest
helper. This is still the `FileStore` page-engine format over a browser file handle, not a separate
IndexedDB object-store backend.

**What it shares with `SingleFile`:** the `ObjectStore` contract (§2.1), the entire
`loom-core` engine (object model, refs, vcs, facets, prolly trees), and the pure-Rust,
`wasm32`-clean per-object frame codecs (compression; 0005 §4.3). The Digest, canonical form, and
identity profile are identical, so a `BrowserStore` Loom and a `SingleFile` Loom sync object-for-
object (0006) with no translation.

**What differs:** native file open/lock/compaction helpers are `cfg`-gated off for `wasm32`.
The browser path supplies a backing handle to `FileStore::with_backing` and uses in-place segment GC
rather than native atomic-rename compaction. OPFS sync-access handles require a Web Worker and are
exclusive per file. IndexedDB remains target fallback work, not a current backend.

**Source-backed capability profile:** OPFS inherits the `FileStore` object model, frame codecs,
identity-profile digest algorithms, encryption frame support, reference-root persistence, page-engine
recovery, and in-place segment GC. The wasm binding source-backs SQL and queue operations over this
backing, plus digest conformance. Native-file locking and compaction remain native-only.

**Target capability profile:** IndexedDB fallback, Web Locks coordination, browser-specific
capability reporting, signing, reflog, commit-graph, pins, and hosted sync projection are target work.

**Caveats:** storage is origin-scoped and subject to browser eviction under storage pressure unless
the origin requests persistent storage (`navigator.storage.persist()`); OPFS sync access handles
require a Web Worker; quota is finite and per-origin. None of these affect object identity or sync.

## 4. Capabilities (normative mechanism)

```idl
struct CapabilitySet { caps: Map<string, CapabilityInfo> }
struct CapabilityInfo { supported: bool; version: u32; params: Map<string,bytes> }
```

- The capability **name space** is defined centrally in 0010 §4 (a registry). Names are stable
  strings (`encryption-at-rest`, `signing`, `prolly-trees`, `sql`, `watch`, …).
- A caller queries `loom.meta.capabilities()` and MUST handle `UNSUPPORTED` for anything not
  advertised. Sync negotiates the **intersection** of two peers' capabilities (0006 §3).
- Capabilities are **versioned**: an incompatible change to a capability bumps its `version`;
  peers negotiate the highest common version (0010 §4).

**Current implementation:** a source-backed `capabilities()` facade now exists. `loom_core::capability`
holds the canonical registry (the 0010 §5 catalog: each capability's name, version pair, owning spec,
and proof status) and a runtime `supported` flag; `Loom::capabilities()` returns a `CapabilitySet` that
can be queried (`get`/`supports`), negotiated between peers (`negotiate`, for 0006 sync), and overlaid
(`with_supported`). The static catalog is a `const` table kept in lock-step with 0010 §5 by a drift
test. `loom-core` asserts `supported` only for the capabilities it implements; capabilities owned by
downstream crates (`single-file-store`, `compression`, `encryption-at-rest`, `rekey` in `loom-store`;
`sql` in `loom-sql`) and all `target` capabilities are reported unsupported by core and overlaid by the
assembling layer (the capability-contribution pattern), so `loom-core` never depends on its dependents.
0010 §5 owns the registry's contents. The C ABI / IDL / binding projection of this facade is the
remaining cross-language surfacing.

## 5. Choosing a backend (informative)

| Need                                                                          | Backend                                        |
| ----------------------------------------------------------------------------- | ---------------------------------------------- |
| One portable, encryptable, copyable artifact; embedded apps                   | `SingleFile` (§3.1)                            |
| Hosted/multi-client service; remote access over the network                   | remote Loom over the wire protocols (0008)     |
| Move history between any of the above                                         | `Sync` (§3.2, 0006)                            |
| Encryption and compression in current source                                  | `FileStore` frame layer (§3.1, §3.3)           |
| Signing, ACLs, E2E sync, and policy over any store                            | Target capability layer (§3.3, 0009)           |
| Import a DB table; check a commit out to disk; commit a directory             | **Interchange** (not a backend - see **0012**) |

All promoted provider combinations must interoperate by construction (0001 A1). Current source proves
this for in-memory and single-file object stores, direct local sync, and bundle sync under matching
identity profiles.

## Resolved decisions

The Database-backend questions (external-blobs integrity, `path_index` staleness) are **dropped**
along with that backend (§3). The remaining capability/provider questions resolve as follows.

1. **Rust boundary stays lean.** `ObjectStore` remains the source-backed byte store. The broader
   Provider interface is target work for generated, remote, or hosted surfaces, not a reason to grow
   the core trait prematurely.
2. **Hosted Provider facade is split out.** Remote object/ref APIs, authenticated ref CAS, hosted
   capability reports, remote GC, pins, reflogs, and generated Provider projection live in 0004a until
   0008 hosted protocols, 0010 capability reports, and 0026-0028 authorization are ready.
3. **Compression and encryption order.** Stored frames compress plaintext first and encrypt the
   compressed frame when encryption is enabled. Digests remain over plaintext bytes, so storage policy
   never changes identity.
4. **Digest verification shape.** The current source-backed API computes the digest from bytes and
   returns it. Target provider APIs that accept caller-supplied digests must recompute and reject
   mismatches.
5. **Deletion refusal.** Pinned-object retention is target work. When implemented, refusing to delete
   a pinned object should report a retained result rather than treating retention as an I/O failure.
6. **Pack-split.** Pack-split is target work. The default flagship store remains one `.loom` file.
7. **Capability negotiation.** At-rest storage features such as compression and encryption are not
   sync blockers. Wire-relevant capabilities are negotiated by 0006/0010 once the generated
   capability surface exists.
8. **Identity profile.** The base store defines the identity profile. Storage transforms do not change
   digests, and sync rejects identity-profile mismatches.
