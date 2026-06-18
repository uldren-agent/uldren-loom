# 0009 - Security and Capabilities

**Status:** Complete for current source-backed storage security; governance expansion is split.
**Version:** 0.1.0. **Normative for implemented at-rest security mechanisms.**

This spec separates the security mechanisms implemented today from the enterprise security contract
that still depends on principals, access control, served write paths, and policy specs.

Current source implements whole-Loom encrypted `.loom` stores, compression frames, digest verification,
identity profiles, passphrase and raw-KEK unlock, cheap rekey, full reseal, multi-wrap add/remove,
duplicate-wrap detection, CLI projection, C ABI projection, and selected binding projection. Source
does not yet implement generic authorization, principal-aware policy, signed history, transparency
logs, redaction workflow, retention policy, hardware unlock providers, key-service unlock providers,
or E2E blind sync.

## 1. Threat Model

Assets:

- Object content confidentiality.
- Object, ref, and history integrity.
- Principal identity and authorization state.
- Availability and recoverability of stored data.

Adversaries considered:

- **A-disk:** can read or modify the storage substrate at rest, such as a stolen `.loom` file or a
  compromised disk. Current mitigation is encryption at rest plus digest and AEAD verification.
- **A-transport:** can observe or modify network traffic. Current direct and bundle sync are local
  APIs, so hosted transport security remains a 0008 target.
- **A-server:** can host data while being honest-but-curious or compromised. Current source does not
  implement zero-knowledge hosted sync; E2E blind sync remains target work.
- **A-principal:** presents a stale, revoked, forged or under-scoped identity. Current source has stable
  auth-related error codes but not principal enforcement.
- **A-program:** runs untrusted or AI-authored logic. The implemented compute crate has capability
  models, but served execution and workspace-aware grants remain gated by 0015 and 0026-0028.

Out of scope for the core contract: host side-channel resistance, OS isolation, malware detection,
tenant isolation outside Loom, and compliance controls that depend on external deployment policy.

## 2. Current Implementation

### 2.1 Storage transforms

The store frame layer applies transforms below the content-address boundary. Object addresses are over
plaintext canonical bytes, so compression and encryption do not change object identity.

- Frame `0x00` stores identity bytes.
- Frame `0x01` stores DEFLATE bytes.
- Frame `0x02` stores LZ4 bytes.
- Frames `0x10`, `0x11`, and `0x12` are AEAD-sealed forms of identity, DEFLATE, and LZ4 payloads.

The write path may fall back to identity framing for small or incompressible payloads. The decode path
inverts framing, checks the plaintext length, and verifies `Digest::hash(store_profile, payload) ==
digest` before returning bytes.

### 2.2 Whole-Loom encryption

Encryption at rest is whole-Loom. A `.loom` file is created either unencrypted or encrypted, and when
encrypted the policy covers every workspace and every object under one key hierarchy. The implementation
does not support per-workspace encryption selection.

Objects are still sealed one record at a time. Each sealed frame stores:

```text
suite_id || nonce || ciphertext || tag
```

The implemented AEAD suite registry is closed:

- `0x01`: XChaCha20-Poly1305, default profile path.
- `0x02`: AES-256-GCM, FIPS-oriented profile path.
- `0x03` through `0xff`: reserved and rejected by current source.

Associated data binds:

- object-frame domain tag,
- frame-format version,
- on-disk frame id,
- AEAD suite id,
- plaintext digest,
- plaintext length,
- stored length.

Object type and workspace id are not bound in v1 because the store record header does not carry them
before decryption. That is an intentional current-source limit, not an enterprise security property.

### 2.3 Key hierarchy

The key layer owns KDFs, DEK wrap and unwrap, CEK derivation, AEAD seal and unseal, and the
`encryption_meta` codec. It does not perform I/O or randomness generation.

The hierarchy is:

- A caller credential yields a KEK.
- The KEK wraps one store DEK.
- The DEK derives a per-object CEK from the object digest and suite.
- The CEK seals the object frame payload.

Implemented credential sources:

- Passphrase, stretched under the profile KDF.
- Raw 256-bit KEK supplied by the host.

Reserved on-disk wrap-source codes exist for keystore, secure enclave, passkey, KMS, and raw KEK
sources. TPM and HSM provider-class splits are target work in 0034. The source does not implement
provider acquisition for keystore, secure enclave, passkey, KMS, TPM, or HSM yet.

### 2.4 Identity profiles

The store identity profile is chosen at creation and is immutable for that store. It selects the object
digest algorithm. Changing it is not a rekey; it is a migration that rewrites object addresses into a
new Loom.

Implemented profiles:

- `default`: BLAKE3-256 object identity, XChaCha20-Poly1305 object AEAD by default, keyed-BLAKE3 CEK
  derivation, Argon2id passphrase KDF, and XChaCha20-Poly1305 DEK wrap.
- `fips`: SHA-256 object identity, AES-256-GCM object AEAD by default, HKDF-SHA-256 CEK derivation,
  PBKDF2-HMAC-SHA-256 passphrase KDF, and AES-256-GCM DEK wrap.

Both profiles use 32-byte object digests. Canonical object bytes are profile-independent; only the
digest layer differs.

### 2.5 `encryption_meta`

The page-engine superblock stores:

- one digest-profile byte,
- an encryption-present byte,
- a two-byte `encryption_meta` length,
- encoded `encryption_meta` in the CRC-covered reserved superblock span.

`encryption_meta` records the active object suite and one or more DEK wrap entries. The only supported
metadata schema is the source-tagged multi-wrap descriptor `LKM1 || 2 || suite || u16{n} || entries`.
Current create and rekey paths write one wrap entry; add-wrap and remove-wrap paths can maintain
multiple active wraps for the same DEK. Duplicate wrap credentials are rejected. Older
single-passphrase metadata is rejected as corrupt metadata.

### 2.6 Rekey and reseal

Two implemented operations have different security and operational meaning:

- Cheap rekey unwraps the existing DEK and re-wraps that same DEK under a new credential. Object frames
  are not rewritten and the active AEAD suite does not change.
- Reseal generates a fresh DEK, optionally changes the active AEAD suite, rewrites surviving objects, and
  records new `encryption_meta`. Plaintext digests stay unchanged, so object identity and refs stay
  stable.

Reseal requires an encrypted, unlocked native-file store. A locked store returns `E2E_LOCKED`; an
unencrypted store returns `UNSUPPORTED`.

### 2.7 API projection

Current CLI projection:

- `loom init --encrypt`
- `loom init --identity-profile default|fips`
- `loom init --fips`
- `loom init --suite xchacha20-poly1305|aes-256-gcm`
- `--key-source prompt|file:<path>|fd:0|raw-kek:file:<path>|raw-kek:fd:0`
- `--new-key-source ...`
- `loom rekey`
- `loom rekey --reseal`

Current C ABI projection includes passphrase and raw-KEK variants for store creation, store open, SQL
open, and SQL batch open. The public error enum carries:

- `PERMISSION_DENIED`
- `AUTHENTICATION_FAILED`
- `IDENTITY_NO_ROOT_CREDENTIAL`
- `E2E_LOCKED`
- `E2E_KEY_INVALID`
- `CONFLICT`

Only `E2E_LOCKED` and `E2E_KEY_INVALID` are fully exercised by the current encryption path. The other
security codes are stable contract placeholders until principal and policy enforcement land.

## 3. Current Integrity Contract

Every current object read is verified against the store identity profile before bytes leave the store.
AEAD authentication, decompression, plaintext length checking, and content-address verification are all
part of the read path.

The v1 enterprise contract keeps verification mandatory. Sampling is not a v1 security relaxation. If a
deployment later wants sampled verification for performance, that has to be a separate certified profile
with a named storage-medium assumption and visible capability downgrade.

## 4. Current Sync and Identity Profile Contract

Direct sync and bundle sync require matching identity profiles. A cross-profile transfer is rejected
loudly; it is never silently rehashed.

Current bundle sync uses the v4 bundle shape. The bundle carries the source digest profile, workspace id,
workspace name, facets, refs, tags, and reachable objects. Import into a Loom with a different identity
profile fails. Rehash transfer is a separate migration operation that is not implemented by sync.
`loom store copy --with fips` is the current explicit CLI migration path for committed unencrypted
workspace history; the remaining migration-policy surface is tracked in 0060.

## 5. Target Enterprise Security Contract

The following surfaces are target work. A spec or API must not claim them as implemented until source,
tests, ABI or bindings where relevant, and conformance prove the behavior.

### 5.1 Principals and access control

Principal identity, login, root credential lifecycle, scoped grants, workspace-level ACLs, fine-grained
facet grants, deny precedence, policy evaluation, and administrative override rules belong in 0026,
0027, and 0028. Served write paths in 0008 must not be promoted until those specs define the
authenticated principal context and enforcement points.

The root principal is not a permanent undeletable account. The principal model must support bootstrap,
rotation, recovery, transfer, and removal semantics explicitly before it gates server writes.

#### Conditional-mutation authorization and audit

The shared conditional-mutation contract is owned by 0003 section 9.1. A facet or hosted adapter
MUST resolve the authenticated principal and evaluate authorization and policy for the requested
mutation before evaluating its compare condition. A denied caller receives the normal authorized
denial shape and does not learn a protected current token, value, target existence, configuration, or
runtime detail through a compare result.

Every compare attempt must produce audit evidence for success and failure. The record must include
the principal or service identity, authorized target and scope, operation kind, compare kind, outcome,
stable error or owner-defined disposition, policy decision, correlation identifier, and observation
time. A successful record may include a safe fingerprint of the resulting anchor. A failure may
include a safe fingerprint of the presented token only when that supports correlation without
disclosure. Audit records must not retain raw credentials, raw opaque tokens, protected current
values, or implementation-private topology.

0003 determines `ALREADY_EXISTS`, `CONFLICT`, `CAS_MISMATCH`, and the lock or fencing error that
describes the compare outcome. A protocol projection may map that result to its own precondition or
conflict response, but it must not introduce a second authorization decision or reveal a value that
the native result would redact.

### 5.2 Signing and transparency

Commit signatures, signed ref advances, signature-suite registries, trust sets, and transparency or audit
logs are target work in 0009a. The enterprise shape is:

- signatures use a closed suite id that is part of the signed payload,
- certificate-chain formats are not identity roots for v1,
- trust is anchored in Loom principals,
- transparency and audit records use a ledger-style append-only facet once 0018 is promoted.

An external witness can be an optional deployment hardening layer, but the base contract must not rely on
an undefined third-party operator.

### 5.3 Lifecycle and governance

Retention, legal hold, redaction, deletion proofs, audit feeds, and policy-controlled garbage collection
are target work in 0009a. Immutable distributed history means redaction cannot guarantee deletion from
already copied peers. The enforceable v1 rule is narrower: ordinary sync must not resurrect redacted
objects after an administrative rewrite. The conflict matrix owns that merge-policy decision.

Conditional-mutation audit evidence is governed by the same retention, legal-hold, redaction, and
evidence-export policy as other security records. Retaining audit evidence does not retain a raw
compare token or protected prior value. Redaction of an underlying target does not permit ordinary
sync or a later audit projection to reintroduce protected compare material.

### 5.4 E2E blind sync

Zero-knowledge remote sync is not implemented. The target shape must define:

- who holds keys,
- whether the host indexes opaque labels or verified objects,
- how clients verify received data,
- what metadata remains visible,
- how revocation and rotation work,
- how blind sync interacts with digest-profile migration and signing.

### 5.5 Unlock providers

OS keychains, Secure Enclave, TPM, passkeys, KMS, and HSM are target provider acquisition work in 0034.
The current metadata shape reserves source-tagged wrap entries, and source creates passphrase and raw-KEK
entries. Multiple active wraps are source-backed for passphrase and raw-KEK credentials.

### 5.6 Capability advertisement

Security capability names must not be advertised as implemented from a planning list. Capability
reporting must distinguish:

- executable, proved by conformance or source tests;
- source-backed, implemented but not yet in shared conformance;
- scenario, documented test data only;
- target, desired but not implemented;
- deprecated, intentionally removed from the contract.

## 6. Digest Migration

Digest migration rewrites every object under a different identity profile into a new Loom, produces an
`old_digest -> new_digest` translation table, and re-points refs. It is not a rekey and it is not sync
behavior. Current source backs `loom store copy --with fips` for committed unencrypted workspace
history, including object, content-address, prolly-node, and queue stream-record rewrites, audit
retention/legal-hold policy import, and disabled served-listener intent import. Encrypted source
migration and formal migration reports remain target work in 0060.

Migration is incompatible with blind E2E sync or signed histories unless the migration contract defines
how ciphertext labels, signatures, transparency records, and audit records are re-established.

## 7. Conformance Requirements

Current source-backed coverage includes:

- KDF determinism and salt sensitivity.
- DEK wrap and unwrap.
- wrong credential failures.
- locked encrypted-store access.
- `encryption_meta` round trips.
- sealed-object round trips for both suites and all inner codecs.
- tamper, wrong-key, and rebound-metadata detection.
- no plaintext leak in encrypted object records.
- compaction reseal.
- corrupted metadata and ciphertext rejection.
- cheap rekey.
- full reseal.
- FIPS profile stores.
- byte-pinned sealed-frame vectors.
- profile mismatch rejection for bundles.
- multi-wrap add/remove behavior.
- duplicate-wrap rejection.

Target conformance still needs:

- shared security capability reports,
- cross-binding encrypted store vectors,
- protocol auth and policy vectors,
- principal lifecycle vectors,
- ACL and fine-grained grant vectors,
- signing and transparency vectors,
- retention and redaction vectors,
- E2E blind-sync vectors,
- provider-acquisition vectors for OS keychains, secure enclave, TPM, passkeys, KMS, and HSM.

## 8. Resolved Decisions

- **RD1 - Encryption scope.** Encryption at rest is whole-Loom. A `.loom` file is encrypted or
  unencrypted at creation. There is no per-workspace encryption selection in v1.
- **RD2 - AEAD suite agility.** Every encrypted frame carries a one-byte AEAD suite id from a closed
  registry, and that id is bound into associated data.
- **RD3 - Random keying only.** Current v1 derives per-object CEKs from the random DEK and object digest.
  Convergent keying is not offered.
- **RD4 - Per-object sealing.** Object records are sealed individually, not as one monolithic file
  ciphertext.
- **RD5 - Rekey split.** Cheap rekey changes the credential wrap only. Reseal rotates object key material
  and can change the AEAD suite while preserving plaintext object addresses.
- **RD6 - AEAD frame ids.** AEAD frames occupy `0x10` through `0x12`, mirroring identity, DEFLATE, and
  LZ4 inner payloads.
- **RD7 - Key-wrap agility.** `encryption_meta` records the DEK-wrap algorithm separately from the object
  AEAD suite.
- **RD7a - Multi-wrap source boundary.** Multiple active wraps are source-backed for passphrase and
  raw-KEK credentials. Provider acquisition remains 0034 target work.
- **RD8 - Verification is mandatory.** Current and v1 target behavior verifies every read against the
  store identity profile. Sampling is not a v1 security relaxation.
- **RD9 - Signature-suite agility.** Target signatures carry a closed, signed-over suite id. Free-form
  algorithm negotiation is out of contract.
- **RD10 - Transparency and ref authenticity.** Target authenticity uses Loom principals. X.509 and PGP
  certificate chains are not v1 identity roots. Transparency and audit use a promoted ledger-style facet.
- **RD11 - Redaction and sync.** Distributed copies can retain redacted data. Ordinary sync must not
  resurrect redacted objects after an administrative rewrite; the conflict matrix owns the merge rule.

## 9. Sensitive Topic Note

This document defines defensive mechanisms for protecting user data: encryption, integrity, access
control, audit, and lifecycle policy. It is not a guide to attacking systems.
