# 0031a - Blind Replica Sync Labels and Protocol

**Status:** Draft target. **Version:** 0.1.0-target.
**Parent:** 0031 End-to-End Encrypted Sync.
**Capability:** `e2e-sync`.

This sub-spec splits the blind-replica label and hosted-sync work out of 0031. It is not implemented
today. The current source implements whole-Loom at-rest encryption, local object verification,
direct workspace sync, and offline bundles, but it does not implement hosted E2E remotes, opaque label
derivation, selective encrypted pull, or protocol/binding projection for `e2e-sync`.

## Target Contract

A blind replica stores encrypted frames and negotiates sync by keyed opaque labels. The host never
receives plaintext object bytes or raw plaintext digests. The client keeps ordinary Loom identity
locally: object digests remain plaintext content addresses, and every pulled frame is decrypted and
verified against that plaintext identity before it is accepted.

The v1 label profile is:

```text
sync_label_key = HKDF-SHA-256(DEK,
                              info = "uldren-loom/e2e-sync-label-key/v1")

remote_label = HMAC-SHA-256(sync_label_key,
                            "uldren-loom/e2e-sync-label/v1" ||
                            plaintext_digest ||
                            object_kind)
```

`object_kind` is a closed discriminator. The initial required discriminators are object frame,
workspace label, branch ref label, tag ref label, bundle/session label, and authorization scope label.
The remote label is exactly 32 bytes unless a later conformance-backed revision changes the profile.

## Required Work

- (P0) Pin the label profile with canonical vectors for at least two digest profiles, two object
  kinds, one empty payload object, one non-empty payload object, one workspace label, and one ref
  label.
- (P0) Add negative vectors proving a blind host cannot use a raw plaintext digest as a remote label,
  cannot distinguish equal plaintext across different Loom DEKs, and cannot validate a known-content
  hash without the label key.
- (P0) Implement client-side label derivation in core behind an explicit `e2e-sync` module or facade.
  The API must accept parsed plaintext digests and object-kind discriminators, not arbitrary strings.
- (P0) Implement decrypt-then-verify on pull: a client must reject any frame whose plaintext digest
  does not match the identity expected from the label relationship.
- (P0) Define hosted have/want negotiation over remote labels in the 0006a/0008 sync protocol work.
  The host may compare labels inside one Loom but must not receive plaintext digest values.
- (P0) Define served authorization inputs after 0027: workspace/ref authorization is evaluated over
  labels and capability scopes without exposing content.
- (P1) Implement selective encrypted pull as a workspace/ref transfer filter. It must not create a
  per-workspace encryption boundary.
- (P1) Add executable conformance for label stability, cross-Loom unlinkability, known-digest
  non-disclosure, wrong-key behavior, tamper detection, selective pull, and binding/protocol parity.
- (P2) Add metadata-hardening extensions only as separately advertised options: padded frames,
  encrypted ref names, and DAG obfuscation.

## Non-Goals

- This sub-spec does not change local object identity. Plaintext digests remain the local content
  addresses.
- This sub-spec does not introduce convergent encryption or cross-Loom ciphertext deduplication.
- This sub-spec does not define server-side compute for keyless remotes. A keyless remote is storage,
  authorization, and relay only.
- This sub-spec does not replace 0034 key-source work. It consumes an unlocked DEK/session and the
  provider model 0034 defines.

## Sequencing

1. (P0) Pin the label profile and vectors first.
2. (P0) Implement the core derivation and verification helpers with conformance.
3. (P0) Add hosted sync negotiation after 0006a and 0008 settle live remotes.
4. (P0) Add authorization projection after 0027 defines the served policy enforcement point.
5. (P1) Add selective encrypted pull and binding/protocol parity.
6. (P2) Add metadata-hardening options only when a deployment requires them.

## Active blind-replica label protocol owner gate

Completion state: active implementation owner. Whole-Loom at-rest encryption, local object
verification, direct workspace sync, and offline bundles are source-backed elsewhere. Opaque label
derivation, vectors, hosted have/want negotiation, authorization, ABI and binding projection,
capability reporting, and conformance remain implementation work.

Decision Points: none.

| Gate | Source-backed evidence | Remaining implementation work | Disposition |
| --- | --- | --- | --- |
| Label profile and vectors | This spec defines the target HKDF/HMAC label profile and object-kind discriminator model. | Pin canonical positive and negative vectors for digest profiles, object kinds, empty and non-empty payloads, workspace labels, ref labels, wrong keys, and raw-digest non-disclosure. | Target P0. |
| Core derivation and verification | Current source verifies local encrypted objects after decrypting them. | Implement explicit blind-label derivation and decrypt-then-verify helpers over parsed plaintext digests and closed object-kind discriminators. | Target P0. |
| Hosted have/want negotiation | Direct sync and offline bundles are source-backed; hosted blind remotes are not. | Define hosted have/want negotiation over remote labels through 0006a and 0008 without exposing plaintext digests to the host. | Target P0. |
| Authorization and projection | Parent security specs own workspace/ref authorization and served policy enforcement. | Define label-scoped authorization inputs, ABI/binding/protocol projection, capability reporting, denied response behavior, and executable conformance. | Target P0. |
