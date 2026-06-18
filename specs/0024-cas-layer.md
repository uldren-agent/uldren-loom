# 0024 - Content-Addressed Store Layer

**Status:** Partial, current CAS substrate and public projection source-backed. **Version:** 0.1.0.
**Capability:** `cas`.

This spec defines the CAS facet: workspace-scoped reachable sets of immutable blobs addressed by
content digest. Current source implements the Rust substrate in `loom-core::cas`, local ACL checks, the
language-neutral IDL shape, the C ABI and C header projection, all eight binding wrappers, MCP data
tools, C ABI tests, and executable provider-level and workspace-facade behavior runners in
`loom-conformance`. Hosted native REST/JSON-RPC/gRPC operation surfaces and REST/JSON-RPC/gRPC
operation protocol conformance are source-backed. Bounded hosted invalid-digest and post-delete
absence behavior is source-backed. Retention/GC policy, served handles, integrity-failure transport
cases, cache headers, and ACL privacy masking remain target work.

Every operation is scoped to one workspace's CAS facet. Cross-workspace CAS writes are out of contract
and must fail with `CROSS_WORKSPACE` once a public facade exposes them.

## 1. Current Implementation

`loom-core::cas` implements:

- `cas_put(loom, ns, bytes)`;
- `cas_get(loom, ns, digest)`;
- `cas_has(loom, ns, digest)`;
- `cas_delete(loom, ns, digest)`;
- `cas_list(loom, ns)`.

`cas_put` checks CAS write permission when ACL is active, computes the content address, creates the CAS
facet directory, writes bytes at the digest path, and returns the digest. Putting identical bytes is
idempotent because the digest and path are the same.

`cas_get`, `cas_has`, and `cas_list` check CAS read permission when ACL is active. `cas_get` reads by
digest and verifies that the stored bytes hash back to the requested digest. It returns `Ok(None)` when
the digest is absent and `INTEGRITY_FAILURE` when the stored bytes do not match the requested digest.
`cas_has` checks the current workspace working tree. `cas_list` enumerates valid digest paths reachable
in the current workspace working tree, sorted by digest hex. It is not a global provider inventory.

The source-backed public projection currently includes:

- IDL `interface Cas` with `put`, `get`, `has`, `delete`, and `list`;
- C ABI `loom_cas_put`, `loom_cas_get`, `loom_cas_has`, `loom_cas_delete`, and `loom_cas_list_json`;
- C header declarations for those functions;
- CLI `loom cas put|get|has|delete|list`;
- binding wrappers in Node, Python, C++, Swift/iOS, JVM, Android, React Native, and WASM
  (`casPut`/`casGet`/`casHas`/`casDelete`/`casList`-style names per binding);
- MCP data tools `cas_put`, `cas_get`, `cas_has`, `cas_delete`, and `cas_list`, routed through the
  engine PEP and covered by MCP surface/schema tests;
- the `cas` capability advertised through the source-backed registry (0010 section 5).

Hosted native REST/JSON-RPC/gRPC operation surfaces are source-backed for CAS, and REST/JSON-RPC/gRPC
operation protocol conformance proves put, get, missing get, has, list, delete, invalid digest, and
post-delete absence. Retention/GC policy, served handles, integrity-failure transport cases, cache
headers, and ACL privacy masking remain target work.
`cas_delete` checks CAS write permission when ACL is active, then unlinks a blob from the current
workspace working tree (it does not eagerly erase bytes); storage reclamation is the engine GC's
delete-on-last-reference behavior.

## 2. Current Storage Shape

The CAS facet path is:

```text
/.loom/facets/cas/<digest-hex>
```

The workspace tree is the current reachable-digest manifest. Workspace commit, branch, checkout,
bundle sync, and clone see CAS blobs as ordinary committed content under the CAS facet. Provider
storage may deduplicate identical bytes by content address, but reachability is workspace-scoped.

The current CAS facet does not contain a separate manifest object, global object index, named blob
index, retention metadata, or transport metadata.

## 3. Current Digest and Integrity Behavior

`cas_put` content-addresses bytes under the store's identity profile
(`content_address_with(loom.store().digest_algo(), bytes)`): a default/BLAKE3 store yields BLAKE3
addresses, a FIPS/SHA-256 store yields SHA-256 addresses. Current source names CAS paths with the
digest hex returned by that content-addressing path. `cas_get` recomputes the content address of loaded
bytes under the same profile and compares it to the requested digest before returning data; `cas_list`
reconstructs digests under the store profile. The address algorithm is fixed per store at creation.

Absent digest behavior is `Ok(None)` in the embedded Rust API. Current hosted CAS operation
conformance pins REST missing `GET` as `NOT_FOUND`, JSON-RPC missing `cas.get` as a null bytes result,
and gRPC missing `get` as `found=false`. Privacy masking for unauthorized or out-of-scope digests
remains owned by 0008 and 0026-0028.

## 4. Current Versioning and Merge Behavior

Current CAS blobs version with the workspace because they are written into the workspace working tree.
A commit snapshots the reachable digest set with every other staged workspace path. `checkout_commit`
and `checkout_branch` restore the reachable digest set with the rest of the workspace tree.

CAS blobs are immutable. Merging independent additions to the CAS facet is conceptually a set union,
but no public CAS merge facade is promoted today. Current sync and branch divergence still follow
`CONFLICT-RESOLUTION-MATRIX.md` at the branch/ref boundary.

There is no derived index to rebuild after checkout, clone, or sync.

## 5. Current Conformance

`loom-core::cas` has unit tests for:

- put/get/has round trip;
- idempotent put and deduped list behavior;
- absent digest lookup;
- commit and checkout reachability;
- empty blob storage.

The C ABI has source-backed tests for round trip, absence, invalid digest handling, workspace
selection by UUID, and JSON list projection.

`loom-conformance::behavior::run_cas_behavior` is executable provider-level behavior coverage for
put, get, idempotent put, and absent digest behavior. It runs against `ObjectStore` directly, not the
workspace CAS helper facade.

`loom-conformance::behavior::run_cas_facade_behavior` is executable workspace-facade coverage over the
public `cas_put`/`cas_get`/`cas_has`/`cas_list` helpers: put-get-has round trip, idempotent put with
dedup, absent-digest behavior, commit-then-checkout reachability per workspace, and reachable-set
preservation across `clone_workspace` and an offline `bundle` export/import round-trip. It is wired into
`certify_memory_store`. Digest-profile behavior (a FIPS/SHA-256 store addresses CAS blobs with SHA-256
and round-trips) is certified by `cas_facade_honors_sha256_profile` in `loom-store` over a real
`FileStore` (the in-memory runner is BLAKE3 only). Integrity verification on read is unit-tested in
`loom-core::cas`.

Treat `run_cas_behavior`, `run_cas_facade_behavior`, the digest-profile test, the C ABI tests, and the
hosted REST/JSON-RPC/gRPC operation suites as current executable coverage. Broader hosted transport
absence, privacy masking, served-handle behavior, and retention policy remain target certification.

## 6. Target Contract

The target public CAS facade should provide:

- put;
- get;
- has;
- list;
- workspace-scoped reachability;
- explicit transport absence mapping;
- optional retention and GC policy integration if promoted.

Before full promotion, the target facade needs:

- hosted protocol methods in 0008;
- stable error mapping through `loom_core::error::Code`;
- access-control review for served CAS writes;
- clear file-projection behavior for `/.loom/facets/cas/...`;
- digest-profile behavior aligned with 0002 and 0009.

### 6.1 Conditional mutation boundary

CAS `put` is immutable and digest-addressed. Repeating a put for the same canonical bytes is idempotent
because it addresses the same blob; it is not a conditional mutation and does not consume the 0003
section 9.1 condition set. CAS reads, presence checks, and deletion of a workspace reachability entry
likewise do not make a blob mutable by digest.

An S3-compatible or other named-object facade may use conditional mutation for its mutable object-key,
version, or metadata state. That facade consumes the owner-defined anchor and maps product ETags,
version ids, and precondition syntax without treating them as Loom-wide token formats. The hosted kernel
authorizes before invoking that owner, preserves 0009 redaction and audit requirements, and maps the
stable 0003 section 8 result after the native operation. Conditional failure never changes immutable
blob bytes or exposes protected current object state. The S3 facade that consumes this boundary is
specified in 0024a (S3 object-store facade).

## 7. Target Storage Contract

The current storage shape is close to the target for the workspace-scoped CAS helper. Promotion still
needs pinned public contracts for:

| Role | Target encoding | Status |
| --- | --- | --- |
| Blob path | `/.loom/facets/cas/<digest-hex>` | Source-backed helper |
| Reachability | Workspace tree entries under CAS facet | Source-backed helper |
| Global provider inventory | Not exposed by CAS facade | Target rule |
| Transport envelope | Digest and bytes mapping per 0008 | Target |
| Retention metadata | Optional policy-controlled records | Target |

If digest encoding, identity profiles, or public transport envelopes change observable bytes, update
conformance vectors with the implementation.

## 8. Relationship to Other Facets

- **Files:** files add names, modes, and tree structure over content-addressed bytes.
- **KV and document:** pair CAS with another facet when callers need human names or metadata indexes.
- **Security:** integrity verification follows the content-addressing and identity-profile rules from
  0002 and 0009.
- **Compute:** `loom-compute` has a CAS capability tag, but CAS state access from programs is target
  work until 0015 defines and implements it.

## 9. Non-Goals and Limits

- Current source is not a global provider inventory.
- Current source does not provide mutable named blobs.
- Current hosted source defines bounded transport absence and invalid-digest mapping for the promoted
  operation surfaces.

## 10. Unfinished Work

| Order | Parent | Work item | Status | Exit criteria |
| --- | --- | --- | --- | --- |
| T1 | RD6 | Spec/source reconciliation | Complete local | Current implementation, binding, conformance, and decision text describe the implemented CAS facade, CLI, and MCP data projection instead of stale partial-parity language. |
| T2 | RD6 | CAS data CLI projection | Complete local | `loom cas ...` commands expose put, get, has, delete, and list with digest-profile-aware output and absence behavior. |
| T3 | RD6 | Hosted CAS wire projection | Partial source-backed | Non-MCP REST, JSON-RPC, and gRPC adapters expose put, get, has, delete, and list with source-backed operation surfaces. REST/JSON-RPC protocol conformance proves put, get, missing get, has, list, delete, invalid digest, and post-delete absence through `hosted-cas-rest-jsonrpc`; gRPC protocol conformance proves the same operation set through `hosted-cas-grpc`. Broader served-handle/privacy behavior remains target. |
| T4 | Section 3 | Transport absence mapping | Partial source-backed | Hosted CAS missing-get, post-delete absence, and invalid-digest behavior are source-backed for the current REST, JSON-RPC, and gRPC operation surfaces. Remaining work: integrity-failure, privacy masking, cache header, and ACL-driven not-found versus permission-denied behavior without changing embedded Rust absence semantics. |
| T5 | Section 6 | Retention and GC policy facade | Target | Retention, pinning, and reclamation policy are explicit and do not weaken content-addressed immutability. |

## 11. Resolved Decisions

- **RD1 - Current storage.** Current source stores each CAS blob under its digest path in the workspace
  CAS facet.
- **RD2 - Current reachability.** The workspace tree is the reachable-digest manifest.
- **RD3 - Current absence.** The embedded Rust API returns `Ok(None)` for an absent digest. Current
  hosted operation conformance maps missing and post-delete get as REST `NOT_FOUND`, JSON-RPC null
  bytes, and gRPC `found=false`; ACL privacy masking remains target work.
- **RD3a - Current invalid digest mapping.** Current hosted operation conformance maps malformed
  digest input as REST HTTP 400 `INVALID_ARGUMENT`, JSON-RPC HTTP 400 `INVALID_ARGUMENT`, and gRPC
  `InvalidArgument`.
- **RD4 - Current integrity.** `cas_get` verifies bytes against the requested digest before returning.
- **RD5 - Current conformance.** Provider-level CAS behavior has an executable conformance runner, and
  C ABI CAS has source-backed tests.
- **RD6 - Public facade status.** IDL, C ABI, C header, CLI, all eight bindings, MCP data tools,
  hosted native REST/JSON-RPC/gRPC operation surfaces, REST/JSON-RPC/gRPC operation protocol
  conformance, bounded hosted invalid-digest and post-delete absence behavior, and executable
  workspace-facade conformance are source-backed; retention/GC policy, served handles,
  integrity-failure transport cases, cache headers, and ACL privacy masking remain target work.

## Change log

### CAS non-Node binding parity (0007)

The CAS facade (`cas_put` / `cas_get` / `cas_has` / `cas_list_json` over the existing C ABI) is now
projected to more bindings, narrowing RD6's "non-Node binding parity" gap:

- Source-backed: Node, Python, C++, Swift/iOS (existing), plus **JVM** (FFM downcalls) and **Android**
  (KMP `expect`/`actual` over new JNI `Java_..._nativeCas*` shims). The CAS surface is passphrase-based
  (and, where the binding already carries it, `kek`); the digest, byte payload, and workspace selector
  match the C ABI exactly.
- Pending: React Native (TurboModule) and WASM (direct over the OPFS loom).

### CAS React Native + WASM parity (0007)

The two remaining bindings now project the CAS facade, closing RD6's "non-Node binding parity" gap
across every shipped binding:

- **React Native** (TurboModule): `casPut` / `casGet` / `casHas` / `casList` added to the codegen Spec
  (`NativeUldrenLoom.ts`), the public TS wrappers (`index.ts`, `casList` parses the JSON array), the
  iOS Obj-C++ module (`UldrenLoom.mm`, async `dispatch_async` over `loom_cas_*` with the shared
  `openStore` opener), the Android Kotlin TurboModule (`UldrenLoomModule.kt`, `executor`-dispatched
  promises), and the Android JNI shims (`UldrenLoom.cpp`, `Java_..._nativeCas*` over `openStoreKeyed`).
  Bytes cross as 0-255 number arrays; `casHas` returns a boolean; `casGet` resolves null on absence.
- **WASM** (`bindings/wasm`): `cas_put` / `cas_get` / `cas_has` / `cas_list` added as methods on the
  `LoomSql` session handle (which already holds an open `Loom<FileStore>` over OPFS and exposes
  workspace/queue ops), calling the `loom_core::cas` functions directly with an `ensure_cas_ns` helper
  that mirrors the FFI's workspace/`cas`-facet ensure. `cas_get` returns `Option<Uint8Array>`;
  `cas_list` returns a JS string array.

RD6 now reads: IDL, C ABI, C header, and all bindings (Node, Python, C++, Swift/iOS, JVM, Android,
React Native, WASM), hosted native REST/JSON-RPC/gRPC operation surfaces, and REST/JSON-RPC protocol
conformance are source-backed; transport absence mapping and retention/GC policy remain target work.

### Workspace-facade conformance + digest-profile fix (0010, 0025; plan items 2-3)

Closing the workspace-facade dimension of the CAS P0 work (plan slices 2-3):

- **Digest-profile fix (correctness).** `cas_put`/`cas_get`/`cas_list` previously hardcoded BLAKE3
  (`content_address` and `Digest::from_blake3_bytes`), so a FIPS/SHA-256 store emitted non-compliant
  BLAKE3 content addresses. They now content-address under the store's identity profile
  (`content_address_with(loom.store().digest_algo(), ..)`, `Digest::of(algo, ..)`). The address
  algorithm is fixed per store, so per-store reconstruction in `cas_list` is well-defined.
- **Executable workspace-facade conformance.** `run_cas_facade_behavior` was extended from a
  single-`Loom` runner to a source+destination runner and now also proves CAS reachability survives
  `clone_workspace` and an offline `bundle` export/import round-trip; it stays wired into
  `certify_memory_store` under the existing `cas-facade` executable suite (no scenario-table churn).
- **Digest-profile conformance.** `cas_facade_honors_sha256_profile` in `loom-store` certifies the
  SHA-256 path over a real `FileStore` (the in-memory runner is BLAKE3 only).

This promotes 0024 plan item 3 (workspace reachability, commit/checkout versioning, bundle sync,
digest-profile, absence, integrity) to executable coverage. Hosted transport absence/error mapping
(item 4) stays blocked on 0008 and 0026-0028.

### CAS delete (forward unreference)

CAS previously had no forward delete: a blob could leave the reachable set only by checking out an
earlier commit that did not contain it, with the bytes reclaimed by GC. `cas_delete(loom, ns, digest)`
adds the missing operation - it unlinks the digest from the workspace's current working tree (making it
unreachable going forward) and returns whether it was present; removing an absent digest is a no-op.
This preserves CAS immutability: it drops a reference rather than mutating content, the bytes are
reclaimed by GC only once no commit, branch, or other workspace still references them, and checking out
an earlier commit that held the blob restores it byte-for-byte.

Implementation: a privileged `Loom::remove_file_reserved` twin (the reserved-path counterpart of
`write_file_reserved`) unlinks under `.loom/facets/cas/`; `loom-core::cas::cas_delete` uses it. The
operation is projected across IDL (`Cas.delete`), the C ABI (`loom_cas_delete`, with a round-trip test),
the C header, and all eight bindings (`casDelete`), and is covered by the executable
`run_cas_facade_behavior` runner (delete reports presence, is idempotent, and checkout restores the
dropped blob) plus a `loom-core::cas` unit test. A retention/GC-policy facade (scheduled or
policy-driven reclamation) remains target work.
