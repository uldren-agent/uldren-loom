# 0018 - Ledger Layer

**Status:** Partial, current ledger substrate and public facade source-backed. **Version:** 0.1.0.
**Capability:** `ledger`.

This spec defines the ledger facet: an append-only, hash-chained, tamper-evident log stored as
versioned workspace content. Current source implements the Rust substrate in `loom-core::ledger`, the
workspace-scoped public facade (`ledger_append`/`ledger_get`/`ledger_head`/`ledger_len`/
`ledger_verify`), the language-neutral IDL shape, the C ABI and C header projection, all eight language
bindings, C ABI tests, hosted REST and JSON-RPC native projection for append/get/head/len/verify,
daemon-opened hosted listeners, native hosted gRPC projection for append/get/range/head/len/verify/
collection listing/checkpoint payload/checkpoint signature verification/proof tree/inclusion proof/
consistency proof, segment-native root storage for the local facade, local range and retention-state
reads, authoritative append validation, source-backed Ed25519 principal signature verification,
signed checkpoint attachment and verification, and an executable facade behavior runner in
`loom-conformance`. Local derived proof tree, inclusion proof, and consistency proof artifacts are
source-backed and remain rebuildable rather than source identity. Witness publication, transparency
presentation, REST/JSON-RPC range and proof projection, physical pruning, retention scheduling,
replay tooling, generated protobuf artifacts, hosted conformance, and broader checkpoint/proof
presentation remain target work. The chain hash uses the store's identity profile (BLAKE3 default,
SHA-256 FIPS), so the facade creates and loads ledgers under the store profile; `ledger_head` is the
chain head digest and `ledger_verify` recomputes the chain (an altered payload or broken link is
`INTEGRITY_FAILURE`).

Every operation is scoped to one workspace's ledger facet. Cross-workspace ledger writes are out of
contract and must fail with `CROSS_WORKSPACE` once a public facade exposes them.

## 1. Current Implementation

`loom-core::ledger` implements:

- `Ledger::new`;
- `Ledger::with_algo(algo)`;
- `algo`, `len`, and `is_empty`;
- `append(payload)`;
- `get(seq)`;
- `entry_hash(seq)`;
- `head()`;
- `verify()`;
- canonical `encode` and `decode`;
- `put_ledger(loom, ns, name, ledger)`;
- `get_ledger(loom, ns, name)`;
- `ledger_range(loom, ns, name, start, end)`;
- `ledger_set_retention_ranges(loom, ns, name, ranges)`;
- `ledger_append_with_mode(loom, ns, name, payload, mode)`;
- `ledger_checkpoint_payload` and `ledger_checkpoint_payload_bytes`;
- `ledger_attach_checkpoint_signature`;
- `ledger_verify_checkpoint_signatures`;
- `ledger_proof_tree`;
- `ledger_inclusion_proof`;
- `ledger_verify_inclusion_proof`;
- `ledger_consistency_proof`;
- `ledger_verify_consistency_proof`.

Each append returns the zero-based sequence number. `get` and `entry_hash` return absence for
out-of-range sequence numbers. `verify` recomputes every entry hash from genesis and returns
`INTEGRITY_FAILURE` if any stored payload or hash no longer matches the chain.

The public `Ledger` facade is source-backed through the IDL, C ABI, C header, CLI, Node, Python, C++,
Swift, JVM, Android, React Native, and WASM for append, get, head, len, and verify. Hosted REST and
JSON-RPC expose append, get, head, len, and verify over the shared hosted kernel, with daemon-opened
listener tests. Native hosted gRPC exposes append, get, range, head, len, verify, collection listing,
checkpoint payload, checkpoint-signature verification, proof tree, inclusion proof, and consistency
proof over the same hosted kernel, with direct service and daemon-opened listener tests. There is no
source-backed compatibility protocol, transparency witness, REST/JSON-RPC range and proof projection,
physical retention pruning, retention scheduler, proof-index artifact, or replay helper today.

## 2. Current Storage Shape

The ledger facet path is:

```text
/.loom/facets/ledger/<name>
```

Public ledger writes stage a dedicated `StagedEntry::Ledger` whose root is a canonical `Tree`, not an
ordinary file blob. The root contains a manifest blob, head metadata blob, segment-index blob, one or
more immutable segment blobs, optional retention metadata, and an optional signed checkpoint blob. The
VCS object projection, persisted working-tree state, sync traversal, file APIs, and watch digest logic
recognize ledger roots as their own entry kind. Workspace commit, branch, checkout, bundle sync, and
clone therefore see a structured ledger root under the ledger facet.

The current implementation builds one immutable segment for the current ledger contents. The canonical
format is segment-native, but multi-segment append policy, physical pruning, retention scheduling,
replay tooling, and derived proof artifacts remain target work. The current linear hash chain and
signed checkpoint support must not be described as transparency-log-grade proofing until derived
inclusion and consistency proof contracts are source-backed.

## 3. Current Chaining and Encoding

The chain hash is:

```text
entry_hash[i] = H_profile(entry_hash[i - 1] || payload[i])
```

The genesis previous hash is 32 zero bytes. `H_profile` is the ledger's digest algorithm:
BLAKE3-256 for the default identity profile and SHA-256 for the FIPS profile.

`Ledger::encode` writes a Loom Canonical CBOR array of entries in sequence order. Each entry is:

```text
[payload, entry_hash]
```

The encoded entry hash stores raw digest bytes. It does not carry an algorithm tag, so decoding must
use the correct store identity profile. `get_ledger` supplies the store profile automatically.

`put_ledger` rejects a ledger whose chain algorithm differs from the store's identity-profile digest
algorithm with `INVALID_ARGUMENT`.

## 4. Current Versioning and Merge Behavior

Current ledgers version with the workspace because they are written into the workspace working tree.
A commit snapshots the ledger root Tree with every other staged workspace path. `checkout_commit` and
`checkout_branch` restore the `StagedEntry::Ledger` root with the rest of the workspace tree.

Current source does not implement ledger-specific merge or replay tooling. If two branches edit the
same ledger collection differently, the current merge machinery treats it as a normal same-path conflict
unless a promoted ledger-specific reconciliation helper exists. The enterprise target remains
fast-forward-only for ordinary ledger history; true divergence is reconciled only by explicit replay
that creates new hashes. A ledger on a draft branch is a proposed or private lineage. A ledger on a
protected fast-forward ref is the authoritative lineage. Rebase or history rewrite is invalid for an
authoritative protected ledger ref.

Sync follows `CONFLICT-RESOLUTION-MATRIX.md`: branch/ref divergence uses the S1 fast-forward boundary,
and ledger replay is explicit target work.

## 5. Current Conformance

`loom-core::ledger` has unit tests for:

- append sequence assignment;
- chain verification;
- tamper detection;
- canonical encode/decode;
- commit and checkout versioning;
- FIPS SHA-256 ledger chaining;
- wrong-profile decode failure;
- `put_ledger` profile mismatch rejection.

`loom-conformance` pins default and FIPS ledger head vectors. It also contains ledger behavior
scenarios and an executable public ledger facade runner that exercises append, get, head, len, verify,
range reads, pruned-range errors, authoritative append validation, signed checkpoint attachment and
verification, derived proof tree, inclusion proof, consistency proof, stale-checkpoint invalidation on
append, versioning, and clone preservation against the in-memory store.

## 6. Target Contract

The target public ledger facade should provide:

- append;
- get by sequence;
- len;
- scan from sequence;
- verify;
- head inspection;
- explicit replay tooling for divergent histories;
- optional signed checkpoints through the shared principal signing substrate;
- optional transparency or witness publication after proof semantics are promoted;
- optional retention holds.

Before hosted and full enterprise promotion, the remaining facade needs:

- remaining hosted protocol methods in 0008;
- stable error mapping through `loom_core::error::Code`;
- access-control review for served ledger appends;
- signing, transparency, and retention policy decisions from 0009a and the principal specs;
- clear file-projection behavior for `/.loom/facets/ledger/...`;
- divergence behavior aligned with `CONFLICT-RESOLUTION-MATRIX.md`.

The local audit spine added for 0009a is ledger-style but is not the public ledger facet. It stores
control-plane audit records under the `FileStore` durable-local control root, chains entries with the
store identity-profile hash, and stays outside workspace commits, clone, bundle export, and ordinary
facet sync. The public ledger facet remains the user-visible append-only facet described here.

## 7. Target Storage Contract

The enterprise storage target is a structured ledger value, not one serialized ledger value. A promoted
storage format should be segment-native from the start. Implementations may choose small segments
while the append path matures, but the canonical format must use the same manifest, segment, head,
and index structures that survive at scale. A standalone sequence-map-only substrate is not an
approved intermediate format.

| Role | Target encoding | Status |
| --- | --- | --- |
| Ledger manifest | Canonical root record naming format version, identity profile, head metadata root, segment index root, optional checkpoint root, optional proof root, and optional retention root | Source-backed, proof policy target |
| Immutable entry segments | Immutable segment records keyed by sequence range; each entry carries sequence, payload bytes, predecessor chain hash, and entry chain hash | Source-backed, multi-segment policy target |
| Segment index | Canonical index mapping inclusive sequence ranges to immutable segment roots and segment summary metadata | Source-backed |
| Head metadata | Canonical head record for latest sequence, latest segment, chain head, identity profile, append mode, retention horizon, and latest signed checkpoint | Source-backed, proof policy target |
| Signed checkpoints | Canonical checkpoint payloads over namespace, collection, checkpoint-free head root, segment index root, chain head, retention root, append mode, and profile, signed through the shared principal signing substrate | Source-backed for Ed25519 principal verification |
| Transparency and proof artifacts | Derived inclusion and consistency artifacts rebuilt from canonical segments; witness and publication artifacts remain presentation work | Source-backed locally, not source identity |
| Retention metadata | Canonical policy and range state that distinguishes retained, planned-prune, pruned, and legal-hold ranges | Target, approved |
| Proof index | Derived Merkle tree over canonical segment entries with namespace and collection-bound leaves | Source-backed locally, not source of truth |

The exact entry payload framing, segment summary bytes, checkpoint format, signature envelope,
retention metadata, proof-index rules, and canonical byte layout must be pinned before this storage
shape becomes identity-affecting. If those choices change canonical bytes, update conformance vectors
with the implementation. Merkle accumulators, proof indexes, witness records, and query accelerators
are derived artifacts unless a later storage-promotion decision makes a selected proof commitment
part of canonical ledger identity.

## 8. Authenticity, Transparency, and Retention Guidance

- The current hash chain gives tamper evidence, not authorship.
- Signed checkpoints are source-backed for purpose-bound principal signatures over the canonical
  ledger checkpoint payload. The payload binds the checkpoint-free head root, segment index root,
  latest sequence, chain head, latest segment root, retention root, append mode, identity profile,
  namespace, and collection name. The live head and manifest then reference the checkpoint object.
  Appending or changing retention restages the ledger without carrying a stale checkpoint.
- The shared principal signing substrate should use Loom principals as trust anchors, closed
  signature-suite ids, purpose-bound signed payloads, challenge verification, delegated agent or
  service-principal signing, key rotation, key revocation, and audit. OpenPGP/PGP can be a suite or
  provider, but not Loom's identity root. Private keys generated or stored by Loom belong in a
  protected key-provider or control-plane layer, not normal workspace or facet content.
- The ledger checkpoint design uses a closed signature-suite registry. Ed25519 and OpenPGP/PGP are
  designed as suites or providers in that registry; Ed25519 verification is source-backed. OpenPGP/PGP
  is not a trust anchor and remains provider target work.
- Transparency is target work and should publish signed or witnessed heads, not raw ledger payloads by
  default. Signed checkpoints and optional witnesses make silent rewrite or split-view behavior
  detectable; they do not make the current linear chain a Merkle transparency log.
- Retention metadata and explicit retained or pruned range errors are source-backed locally. Physical
  pruning, retention scheduling, hosted projection, policy enforcement, and store-level GC integration
  remain target work.
- A Merkle accumulator can be added as a derived proof index, but the target source of truth is the
  segment-native ledger manifest and immutable segments, not a proof cache.

## 9. Relationship to Other Facets

- **Queue:** a queue is an ordered stream with consumer semantics. A ledger is an append-only
  tamper-evident chain. They may share substrate later, but they remain separate capabilities.
- **Security:** signing, transparency, retention, and principal enforcement depend on 0009a and the
  principal/access-control specs.
- **Compute:** `loom-compute` has a `Ledger` capability tag, but ledger state access from programs is
  target work until 0015 defines and implements it.

## 10. Non-Goals and Limits

- Current source is not a consensus ledger or blockchain.
- Current source is not a compatibility service or transparency service.
- Current source provides segment-native ledger roots, canonical retention metadata, local
  retained/pruned range read semantics, and signed checkpoint attachment and verification over the
  principal signing substrate.
- Current source does not provide REST/JSON-RPC range and proof projection, physical retention
  pruning, retention scheduling, witness publication, OpenPGP/PGP checkpoint signing, or
  transparency.
- Current source does not provide ledger replay tooling.
- Current source does not provide a Merkle proof index.
- Product-clone ledger database compatibility is not an active Ledger target.

## 11. Unfinished Work

| Order | Parent | Work item | Status | Exit criteria |
| --- | --- | --- | --- | --- |
| T1 | RD9 | Spec/source reconciliation | Complete local | Current implementation and conformance text describe the implemented public ledger facade instead of stale target-only language. |
| T2 | RD9 | CLI ledger projection | Complete local | `loom ledger ...` commands expose append, get, head, len, and verify with byte-stable output forms. |
| T3 | RD9 | Hosted ledger wire projection | Partial | REST and JSON-RPC served protocol conformance proves append, get, head, len, and verify with hosted auth/PEP over the current data route. Native gRPC exposes append, get, streaming range, head, len, verify, collection listing, checkpoint payload, checkpoint signature verification, proof tree, inclusion proof, and consistency proof with service and daemon listener tests. REST/JSON-RPC range and proof parity, generated protobuf artifacts, and signing control-plane routes are closure-blocking promotion debt. Witness publication, transparency, and physical retention are P1 ledger primitive work. |
| T4 | RD7 | Ledger replay tooling | Target | Divergent histories can be replayed explicitly into a new chain without silently merging incompatible hashes. Replay tooling is P1 ledger primitive work, not closure-blocking storage promotion debt for the current segment-native root. |
| T5 | Section 7 | Segment-native structured storage | Partial | Canonical model records for manifest, immutable entry segments, segment index, head metadata, append-mode values, retention metadata, signed checkpoints, and derived proof artifacts are source-backed with round-trip, negative decode, and behavior tests. Public append, explicit draft/authoritative append, get, head, len, verify, range read, signed checkpoint helpers, and proof helpers operate through segment-native roots. Multi-segment append policy, migration vectors, and conformance vectors are closure-blocking promotion debt. Physical pruning and broader hosted projection are P1 ledger primitive work. |
| T6 | Section 8 | Signing, transparency, and retention | Partial | Local retained/pruned range read semantics, Ed25519 signed checkpoint attachment and verification, local derived inclusion/consistency proofs, and native hosted gRPC checkpoint/range/proof projection are source-backed. REST/JSON-RPC checkpoint/range/proof/retention projection and policy enforcement are closure-blocking promotion debt. Witness publication, transparency, physical pruning, retention scheduling, historical revocation effects, OpenPGP/PGP provider behavior, and protected key-provider boundaries are P1 ledger primitive work. |
| T7 | Section 8 | Shared principal signing substrate | Partial | Principal-bound public keys, closed signature-suite ids, Ed25519 and ES256 verification, and purpose-bound payload bytes are source-backed. Remaining work: challenge verification, delegated signing policy, key rotation, revocation effects on historical signatures, key lifecycle audit, OpenPGP/PGP provider behavior, and protected key-provider boundaries. |

## 12. Resolved Decisions

- **RD1 - Current storage.** Current source stores each named ledger as a segment-native ledger root
  Tree under the workspace ledger facet.
- **RD2 - Current identity.** Sequence number is append order. The current canonical segment entries
  store payload and chain hash for each entry.
- **RD3 - Chain algorithm.** The ledger chain algorithm must match the store identity profile.
- **RD4 - FIPS posture.** FIPS ledgers chain with SHA-256, not BLAKE3.
- **RD5 - Tamper evidence.** The current source of truth is a per-entry hash chain.
- **RD6 - Public facade status.** The workspace-scoped public `ledger` facade is source-backed across
  the engine, C ABI, IDL, C header, CLI, and all eight bindings.
- **RD7 - Merge boundary.** Ordinary ledger history is fast-forward-only. Divergence requires explicit
  replay target tooling.
- **RD8 - Queue separation.** Ledger and queue remain separate capabilities because their guarantees
  and costs differ.
- **RD9 - Public facade status.** The workspace-scoped public `ledger` facade
  (append/get/head/len/verify) is source-backed across IDL, C ABI, C header, CLI, and all eight
  bindings, with an executable facade conformance runner. It creates and loads ledgers under the
  store's identity profile. REST and JSON-RPC operation protocol conformance proves append, get, head,
  len, and verify. Core range, retention-state, authoritative append, signed checkpoint, and
  derived proof helpers are source-backed locally. Hosted range/checkpoint/proof projection,
  transparency presentation, witness publication, and physical retention work remain target work.
- **RD10 - Native ledger target.** The active target is a structured Loom-native ledger with
  segment-native immutable entries, a canonical segment index, canonical head metadata, scan/range
  support, replay, retention metadata, signed checkpoints, and derived proof artifacts. A
  sequence-map-only intermediate format is not approved. Product-clone ledger database compatibility
  is cut from active Ledger design.
- **RD11 - Ledger on VCS.** Draft branches carry proposed or private ledger lineages. Protected
  fast-forward refs carry authoritative ledger lineages. Divergence requires explicit replay, and
  rebase or history rewrite is invalid for authoritative protected ledger refs.
- **RD12 - Authoritative append mode.** Core exposes draft versus authoritative append mode explicitly.
  Draft append remains the default public facade behavior. Authoritative append requires the current
  branch to have a protected-ref policy with `fast_forward_only = true`; ordinary VCS protected-ref
  evaluation still enforces the branch advance at commit/publication time.
- **RD13 - Checkpoint signing.** Checkpoints use a closed signature-suite registry rooted in Loom
  principals. Ed25519 verification is source-backed for purpose-bound checkpoint payloads through
  principal public keys. OpenPGP/PGP is designed as a future suite or provider option and is not a
  Loom identity root.
- **RD14 - Proof artifacts.** Checkpoint and proof contracts are specified together. Local inclusion
  and consistency artifacts are source-backed as derived Merkle proofs over canonical segment entries;
  they do not become source identity.
- **RD15 - Retention.** Canonical retention metadata and local retained, planned-prune, pruned, and
  legal-hold range status are source-backed. Pruned ranges reject local range and point reads before
  physical pruning. Physical pruning, retention scheduling, hosted projection, and policy enforcement
  remain target work.
- **RD16 - Segment model source state.** Canonical encoders and decoders for the target manifest,
  immutable segment, segment index, head metadata, and append-mode model are source-backed in
  `loom-core`. Public ledger append, get, head, len, and verify now stage ledger collections through
  a segment-native root Tree with manifest, head, segment-index, retention, and immutable segment
  content roots. Local range scan, retained/pruned range semantics, and authoritative append validation
  are source-backed. Signed checkpoint payloads, attachment, verification, and local derived
  inclusion/consistency proofs are source-backed. Hosted range/checkpoint/proof projection, witness
  publication, transparency presentation, and physical pruning remain target work.

## Change log

### Collection parameter rename (0042 section 5.1)

The log collection segment's canonical parameter name is `collection`, replacing the legacy `name`.
Concept and address position are unchanged (0042). Implementation is the follow-on full-stack rename
pass.

### Ledger public facade (engine slice; 0007, 0010)

The ledger facade is source-backed end to end (engine portion of the Document + Time-series + Ledger
batch): `loom-core::ledger` adds `ledger_append`/`ledger_get`/`ledger_head`/`ledger_len`/`ledger_verify`
(profile-aware: ledgers are created/loaded under the store's identity profile; an absent ledger is empty
and verifies trivially); projected to IDL `interface Ledger`, C ABI `loom_ledger_*` (with a round-trip
test; `verify` returns 0 when intact and `INTEGRITY_FAILURE` otherwise), the C header, and all eight
bindings; covered by the executable `run_ledger_facade_behavior` runner (append assigns 0 then 1,
profile-tagged head, verify, commit/checkout versioning, clone preserves a verifiable chain) wired into
`certify_memory_store`, with the `ledger` capability flipped from `scenario` to `executable` (registry +
0010 section 5). Later local slices added range scan, retained/pruned range semantics, authoritative
append validation, and signed checkpoint attachment and verification. Transparency proof artifacts,
hosted checkpoint/range projection, and physical retention remain target work.

- 2026-06-27 (P-bindings): Ledger facade (`ledger_append`/`ledger_get`/`ledger_head`/`ledger_len`/
  `ledger_verify`) now has full language-binding parity across all eight families (Node, Python,
  WASM, C++, iOS/Swift, JVM, Android JNI+Kotlin, React Native). The u64 sequence/length crosses the
  React Native bridge as a decimal string. Verified via `just test-bindings`.
