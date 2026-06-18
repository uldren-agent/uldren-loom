# P9-0005 - `cas` Binding

**Series:** P9 binding plan (normative-track sub-series; Draft)
**Version:** 0.1.0-draft. **Status:** Draft. **Last updated:** 2026-06-25
**Reads first:** [`P9-0002-projection-conventions.md`](./P9-0002-projection-conventions.md), facade spec
**0024** (CAS), **0008 section 3.1 and section 3.4** (objects and ETag), **0002 section 2 and section
3** (digests and blobs).

`cas` is the thinnest data facet: immutable bytes addressed by digest inside one workspace's reachable
CAS manifest.

## 1. Current Source Boundary

Current source backs:

- `loom-core::cas_put`, `cas_get`, `cas_has`, `cas_delete`, and `cas_list`;
- IDL `interface Cas` with `put`, `get`, `has`, `delete`, and `list`;
- C ABI `loom_cas_put`, `loom_cas_get`, `loom_cas_has`, `loom_cas_delete`, and `loom_cas_list_json`;
- C header declarations for those ABI functions;
- Node binding wrappers `casPut`, `casGet`, `casHas`, `casDelete`, and `casListJson`;
- C ABI tests for round trip, absence, invalid digest, workspace selection by UUID, and JSON list;
- provider-level executable CAS behavior in `loom-conformance`.

The current source-backed `list` is workspace-scoped. It enumerates digest paths reachable in that
workspace's `cas` facet, sorted by digest hex. It is not a global provider inventory.

CLI, local MCP tools, Python, C++, Swift/iOS, JVM, Android, React Native, wasm, hosted protocols,
capability reporting, and public facade conformance are target work unless a later source pass verifies
them.

### 1.1 Binding Boundary

The base layer is the workspace-scoped reachable digest manifest plus the Loom object store. Native
projections expose digest-verified put/get/has/delete/list operations. OCI and S3 are first-class served
compatibility surfaces that may use CAS internally; they are not transports under `cas`. CAR is an
interchange format. IPFS gateway compatibility and Kubo RPC compatibility are excluded from active
370 scope. Provider inventories, pin sets, upload sessions, and adapter caches are derived or
operational state, not the canonical CAS contract.

## 2. Facade Surface

Source-backed language-neutral shape:

```text
put(workspace: UUID | name, content: bytes) -> Digest
get(workspace: UUID | name, digest: Digest) -> Option<bytes>
has(workspace: UUID | name, digest: Digest) -> bool
delete(workspace: UUID | name, digest: Digest) -> bool
list(workspace: UUID | name) -> List<Digest>
```

`put` by workspace name ensures a CAS-facet workspace. `get` verifies that returned bytes hash to the
requested digest and reports `INTEGRITY_FAILURE` on mismatch. Invalid digest strings report
`INVALID_ARGUMENT`. Absent `get` returns `None` at the embedded/API layer.

## 3. Tier-1 REST

Facet-root: `/v1/workspaces/{workspace_id}/objects`.

| Facade method | HTTP |
| --- | --- |
| `put` | `POST /objects` with bytes body returns `201` and `Location: /objects/{digest}` |
| `get` | `GET /objects/{digest}` returns bytes plus strong digest ETag |
| `has` | `HEAD /objects/{digest}` returns `200` or `404` |
| `delete` | `DELETE /objects/{digest}` returns `{found}` |
| `list` | `GET /objects?list=1` returns workspace-scoped digest stream |

`PUT /objects/{digest}` is a target convenience and must recompute the body digest before accepting a
write.

## 4. Tier-1 JSON-RPC

Target methods: `cas.put`, `cas.get`, `cas.has`, `cas.delete`, and `cas.list`.

## 5. Tier-1 gRPC

Target methods: `PutObject`, `GetObject`, `HasObject`, `DeleteObject`, and `ListObjects`. Large-object
streaming is a protocol concern and must preserve digest verification.

## 6. Tier-1 MCP

- **Read tools:** `cas.get`, `cas.has`, `cas.list`.
- **Write tools:** `cas.put`, `cas.delete`, token-gated per P9-0002 section 5.

## 7. Tier-2 Foreign Adapter

OCI distribution, S3, and CAR are the object/blob/archive family:

- **OCI Distribution:** a first-class `oci` served surface over repositories, blobs, manifests, tags,
  referrers, strict digest mapping, bounded authorized catalog, delete semantics, cross-repository
  mount, and durable upload-session state. Daemon-opened `oci/rest` is source-backed for public
  slash-separated repository names, stable internal repository ids plus display metadata, monolithic
  upload, durable chunked upload, upload status/cancel, cross-repository mount, tags list, bounded
  catalog, referrers, strict SHA-256 digest verification, OCI and Docker v2 schema media-type
  admission, and schema v1 plus unknown dangerous media-type rejection. Direct TLS for `oci/rest` uses
  the shared hosted TLS path when configured. Blob delete removes repository reachability metadata;
  physical CAS byte deletion belongs to separate reachability-proven GC.
- **S3:** a first-class `s3` served surface whose bucket state lives under
  `.loom/facets/s3/buckets/{bucketname}` inside the selected workspace. The first daemon-opened
  `s3/rest` vertical is source-backed for service-scoped and bucket-scoped listeners, bucket
  create/list/delete, object put/get/head/delete, metadata, byte ranges, conditional writes, opaque
  S3-safe version IDs, S3-compatible ETags, basic multipart upload, hosted auth/PEP, and conformance
  rows. S3 may use CAS for object bytes, but its bucket lifecycle, public-access configuration,
  metadata, multipart, conditional writes, versioning, S3-compatible ETags, and SDK compatibility are
  not part of the base CAS contract. SigV4 app credentials, configured public access, direct TLS, and
  real SDK transcripts remain target work.
- **CAR:** an import/export command and interchange format over CAS and workspace object graphs.
  `loom interchange export-car` and `loom interchange import-car` are source-backed for deterministic
  workspace graph export/import. The stream is a CARv1-shaped length-delimited file with one Loom CAR
  manifest root block and deterministic object blocks sorted by Loom bundle order. CIDv1 raw-codec
  multihash CIDs translate Loom BLAKE3 and SHA-256 digests through their stable digest algorithm codes.
  Import validates every block CID against its object bytes before reconstructing the workspace bundle.
  IPFS and Kubo compatibility remain excluded from active scope.
- **Archive:** an import/export command and interchange format over file trees and workspace object
  graphs. `tar.zstd` is the canonical format; `tar`, `tar.gz`, and `zip` are compatibility formats.
- **IPFS/Kubo:** excluded from the current roadmap as lower value than OCI, S3, and CAR.

Any adapter must state that Loom currently uses its own digest profile and must not pretend to be
SHA-256, CID, or multihash-agile unless those profiles are implemented.

## 8. Errors, Parity, and Concurrency

- **Errors:** `INVALID_ARGUMENT` for malformed digests and `INTEGRITY_FAILURE` for content/digest
  mismatch are source-backed. Transport absence mapping is target work.
- **Parity:** Rust core, IDL, C ABI, C header, and Node are source-backed today. Other binding families
  remain target work.
- **Concurrency:** CAS `put` is idempotent by content. Branch/ref divergence still follows
  `CONFLICT-RESOLUTION-MATRIX.md`; no hosted merge facade is source-backed.

## 9. Resolved Decisions

- **RD1 - Enumeration.** `list` is part of the source-backed CAS facade, scoped to one workspace's
  reachable CAS manifest.
- **RD2 - Digest verification.** A digest-addressed write or read must verify content against the digest.
- **RD3 - Global inventory.** CAS does not expose a global provider inventory.

## 10. Open Questions

### OQ-C2 - `PUT /objects/{digest}` verification (open)

- **Context.** A client may want to upload bytes at a digest it already computed, but accepting a body
  under the wrong digest would break content addressing.
- **Example.** A client sends bytes B to `/objects/blake3:aaa` when `hash(B) = blake3:bbb`.
- **Options.** (a) recompute and reject mismatches; (b) trust the client digest; (c) only support
  `POST /objects` and let the server assign the digest.
- **Recommendation.** (a) if `PUT` is promoted. Recomputing is required for the integrity contract.
