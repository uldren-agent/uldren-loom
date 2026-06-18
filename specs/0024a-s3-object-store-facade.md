# 0024a - S3 Object-Store Facade

**Status:** Partial source-backed; multipart completeness, versioning, and richer ACL are target.
**Version:** 0.1.0. **Surface:** `s3` (0008 §wire-surfaces).

This sub-spec owns the S3-compatible object-store facade. S3 is a compatibility surface over the
native CAS (0024) and files primitives, not a native storage contract: bucket/object identity,
authorization, and durability are defined by Loom, and the S3 protocol is projected on top. The
conditional-mutation boundary for the mutable object-key namespace is defined in 0024 §6.1; the
served surface row and transport admission are defined in 0008. This spec pins the supported surface,
the target completions (multipart, versioning, ACL), and the stable unsupported non-goals so clients
get precise behavior and future sessions do not re-derive the boundary.

Every operation is scoped to one workspace's object namespace. Cross-workspace object access is out
of contract and fails with `CROSS_WORKSPACE`.

## 1. Current source-backed surface

The daemon-opened `s3/rest` listener is source-backed for:

- **Addressing:** one-selector service endpoints scoped to one workspace; two-selector
  bucket-scoped endpoints where the request path is the object-key root; path-style fallback and
  virtual-host bucket selection.
- **Bucket operations:** create, list, delete.
- **Object operations:** PUT, GET, HEAD, DELETE, including user metadata headers and byte-range
  reads.
- **Conditional writes:** `If-Match` / `If-None-Match` preconditions mapped to the 0024 §6.1
  mutable-object conditional-mutation boundary (a `CasMismatch` on failed precondition; see the
  compare-token contract).
- **Version identifiers:** opaque, S3-safe version IDs (distinct from Loom content digests).
- **ETags:** S3-compatible ETags computed and presented separately from Loom digests.
- **Multipart upload:** the basic create / upload-part / complete path.
- **Security:** hosted auth / policy-enforcement-point integration, SigV4 app-credential
  verification, configured unauthenticated public-read ACLs, and the shared hosted TLS path for
  direct `s3/rest` TLS when configured.

Source anchors: `crates/loom-hosted/src/serve.rs` — `S3RestState` (L210), `s3_rest_router` (L465),
`serve_s3_rest` (L2031); `crates/loom-cli/src/daemon_cmd.rs` — `start_s3_listener` (L1621);
`crates/loom-conformance/src/lib.rs` — S3 surface row (L5809, status Supported) and the guarded
`s3-compatible-bucket-object-service` AWS-CLI transcript suite (L10619). Served-surface definition:
`specs/0008-wire-protocols.md` `s3` row. Object-key conditional mutation: `specs/0024-cas-layer.md`
§6.1.

## 2. Target contract

The following are in-scope and required for the facade to be marketed as broadly S3-compatible; each
is a sequenced implementation ticket (see §5). They must not redefine Loom object identity or
authorization.

### 2.1 Multipart upload completeness

- List multipart uploads (`ListMultipartUploads`) and list parts (`ListParts`).
- `AbortMultipartUpload` with durable cleanup of staged parts.
- `UploadPartCopy` (server-side part copy) and `CopyObject`.
- Part-number and part-size validation with S3-stable error codes
  (`EntityTooSmall`, `InvalidPart`, `InvalidPartOrder`, `NoSuchUpload`).
- Deterministic multipart completion ETag (the S3 `-N` multipart ETag form), distinct from the
  Loom digest.

### 2.2 Object versioning

- Bucket versioning state (`PutBucketVersioning` / `GetBucketVersioning`): `Unversioned`,
  `Enabled`, `Suspended`.
- Version-aware GET/HEAD/DELETE (`versionId` query), delete markers, and `ListObjectVersions`.
- Mapping of S3 version IDs onto Loom's versioned object history so a version ID resolves to a
  committed object state; version IDs remain opaque and S3-safe.
- Interaction of versioning with conditional writes and multipart completion pinned explicitly.

### 2.3 ACL behavior

- `PutObjectAcl` / `GetObjectAcl` and `PutBucketAcl` / `GetBucketAcl` for the supported canned ACLs
  (`private`, `public-read`) plus explicit grant grammar for the supported subset.
- Deterministic mapping of ACL state onto Loom authorization (ACLs constrain, never widen beyond,
  the Loom policy for the principal).
- Stable errors for unsupported grantees / permissions rather than silent acceptance.

### 2.4 Conformance

- AWS client transcript conformance beyond the current guarded create/put/get: multipart lifecycle,
  versioned read/delete, and ACL round-trip transcripts.
- Conformance-report rows distinguishing supported, degraded, target, and unsupported for every
  operation in §1–§2.
- Negative/transcript coverage for the stable unsupported boundary (§3).

## 3. Stable unsupported non-goals

These are intentionally **not** implemented and must return stable, documented S3 error responses
(not silent partial behavior). They are recorded here so clients get precise failure and future
sessions do not treat them as gaps:

- **Object lifecycle** (`PutBucketLifecycleConfiguration`, expiration/transition rules).
- **Cross-region / bucket replication.**
- **Full IAM** (bucket policies, STS, assume-role, condition keys) beyond the supported SigV4
  app-credential + canned/public-read ACL subset.
- **Server-side encryption negotiation (SSE-KMS/SSE-C headers)** as an S3-managed keyring — Loom
  container encryption is orthogonal and not exposed through S3 SSE headers.
- **Bucket-level features** outside the object-store core: website hosting, CORS configuration,
  logging, notifications, request payer, object lock / legal hold, inventory, analytics, tagging as
  a policy surface.
- **Storage classes** beyond a single default class.

Each returns the closest stable S3 error (for example `NotImplemented` or the specific
`*Configuration`-absent error) with a conformance row marking it UNSUPPORTED.

## 4. Invariants

- S3 identity artifacts (ETags, version IDs, upload IDs) are facade-owned and opaque; they are never
  Loom content digests and never leak Loom-internal identity.
- Authorization always flows through the hosted PEP; ACLs may only narrow access relative to Loom
  policy for the principal.
- Object bytes are stored through the native CAS/files primitives; the S3 facade adds naming,
  versioning metadata, and protocol projection only.
- Conditional writes use the single compare-before-write contract (0024 §6.1; shared entity-tag /
  compare-token model), so lost-update semantics match the other facades.

## 5. Implementation task breakdown (sequenced)

Recommended child tickets under MX-124 (facade compatibility). Each is a bounded implementation slice
with its own conformance evidence; ordering reflects dependency and client value.

1. **S1 — Multipart completeness.** `ListMultipartUploads`, `ListParts`, `AbortMultipartUpload`,
   `UploadPartCopy`/`CopyObject`, part validation errors, multipart-`-N` ETag. Depends on the
   existing basic multipart path. (Highest client value; no versioning dependency.)
2. **S2 — Object versioning.** Bucket versioning state, version-aware GET/HEAD/DELETE, delete
   markers, `ListObjectVersions`, S3-version-ID ↔ Loom-history mapping. Depends on S1 for
   multipart-under-versioning interaction.
3. **S3 — ACL behavior.** Object/bucket ACL get/put for the supported subset, ACL→Loom-policy
   mapping, stable unsupported-grantee errors. Independent of S1/S2 (can run parallel to S2).
4. **S4 — AWS transcript conformance + report rows.** Guarded AWS-CLI/SDK transcripts for multipart
   lifecycle, versioned read/delete, and ACL round-trip; add supported/degraded/target/unsupported
   rows for every §1–§3 operation. Depends on S1–S3.
5. **S5 — Unsupported-boundary hardening.** Ensure every §3 non-goal returns its stable S3 error
   with a negative conformance vector. Can run parallel to S4.

## 6. Change log

- 0.1.0 — Initial dedicated S3 object-store facade spec: pins the source-backed surface, target
  completions (multipart, versioning, ACL), stable unsupported non-goals, invariants, and the
  sequenced implementation breakdown. Extracted from the `s3` row of 0008 and the object-key
  conditional-mutation boundary of 0024 §6.1 (filed by MX-124 / MX-196).
