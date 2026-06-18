# P9-0015 - `ledger` Binding

**Series:** P9 binding plan (normative-track sub-series; Draft)
**Version:** 0.1.0-draft. **Status:** Draft, source-backed local facade and hosted native subset.
**Last updated:** 2026-07-02
**Reads first:** [`P9-0002-projection-conventions.md`](./P9-0002-projection-conventions.md), facade spec
**0018** (Ledger; BLAKE3 hash chain; optional signing 0009a), fidelity doc (tamper-evident, not a Merkle
transparency log).

## 1. Facade surface (0018 section 4 `Ledger`)

The source-backed local facade stores one tamper-evident hash chain per collection. Operations are
`append(collection, payload) -> u64`, `get(collection, seq) -> Option<bytes>`,
`head(collection) -> Option<Digest>`, `len(collection) -> u64`, `verify(collection)`, and
`list_collections()`.

`verify` reports success by returning normally and reports corruption through the stable
`INTEGRITY_FAILURE` code. Merkle inclusion/consistency proofs and signed checkpoint serving are target
presentation work.

### 1.1 Binding Boundary

The base layer is the Loom linear hash-chained ledger using the store identity profile. Native
projections expose append, get, head, len, verify, and collection listing. The target base layer is a
segment-native Loom ledger with immutable sequence-range segments, a canonical segment index,
canonical head metadata, range scan, explicit replay, retention metadata, signed checkpoints, and
derived proof artifacts. A sequence-map-only intermediate format is not approved. Transparency-log-
style checkpoint APIs are presentations after proof semantics are promoted. Entry exports and signed
checkpoint files are interchange. Merkle accumulators, proof indexes, witness records, and signature
materializations are derived artifacts unless the ledger spec promotes a selected proof commitment to
canonical proof data. Product-clone ledger database compatibility is not an active target.

Current source also includes canonical model encoders and decoders for the target manifest,
immutable segment, segment index, head metadata, and append-mode values. Public append, get, head,
len, and verify stage and load ledger collections through a segment-native root Tree containing
manifest, head, segment-index, and immutable segment content roots. The current writer emits one
segment per collection snapshot; the canonical root shape can add additional immutable segments
without changing the public storage contract. Local range scan and retained/pruned range semantics
are source-backed in core and conformance. Core also exposes explicit draft versus authoritative
append mode; authoritative append requires the current branch to be protected fast-forward-only.
Hosted native gRPC projection for range scan, checkpoint payload, checkpoint-signature verification,
proof tree, inclusion proof, and consistency proof is source-backed over the shared hosted kernel.
Public projection of authoritative append mode beyond core, REST/JSON-RPC range and proof routes,
physical pruning, witness publication, and transparency presentation remain target work.

## 2. Tier-1 REST

Current source-backed hosted REST is the native action-shaped subset used by the served listener:
`/ledger:append`, `/ledger:get`, `/ledger:head`, `/ledger:len`, and `/ledger:verify`. The
resource-shaped routes below remain the target REST profile.

Facet-root `/v1/workspaces/{workspace_id}/ledger`:

| Facade method | HTTP |
| --- | --- |
| `append` | `POST /ledger/entries {payload}` -> `201` + `{seq, entry_hash}` |
| `get` | `GET /ledger/entries/{seq}` -> `Entry` |
| `head` | `GET /ledger/head` -> optional digest |
| `len` | `GET /ledger/length` -> `{len}` |
| `verify` | `POST /ledger:verify` -> success or `INTEGRITY_FAILURE` |
| `list_collections` | `GET /ledger?list=1` -> NDJSON names |

Entries are immutable once appended, so point reads can use digest-backed validators. Hosted range scan is
target work. The scan contract must preserve the source-backed local distinction between absent
entries and explicitly pruned or retained-gap ranges.

## 3-4. JSON-RPC / gRPC

Current hosted JSON-RPC is source-backed for `ledger.append`, `ledger.get`, `ledger.head`,
`ledger.len`, and `ledger.verify`. The full target JSON-RPC surface is 1:1
(`ledger.append/get/head/len/verify/list_collections/range/checkpoint/proof`).

Native hosted gRPC is source-backed for:

- unary `Append`, `Get`, `Head`, `Len`, `Verify`, and `ListCollections`;
- server-streaming `Range`, including retained/pruned/legal-hold range state strings and entry hashes;
- unary `CheckpointPayload` and `VerifyCheckpointSignatures`;
- unary `ProofTree`, `InclusionProof`, and `ConsistencyProof` returning canonical CBOR proof bytes.

Generated protobuf artifacts, REST/JSON-RPC parity for range and proofs, witness publication,
transparency presentation, physical pruning, and broader hosted conformance remain target work.

## 5. Tier-1 MCP

- **Read tools:** `ledger.get`, `ledger.head`, `ledger.len`, `ledger.verify`, `ledger.list_collections`.
- **Write tool (token-gated):** `ledger.append`. Append-only: there is no update/delete tool by design.

## 6. Tier-2 native checkpoint and transparency presentation

Ledger focuses on Loom-native verifiability first, not compatibility with a separate ledger database
product. The target presentation starts with signed checkpoints over canonical ledger heads and only
exposes transparency-log-style behavior after inclusion and consistency proof semantics, witness
policy, disclosure policy, and conformance are specified. Loom's current chain is linear and
tamper-evident, not a Merkle transparency tree, so it offers no transparency-log-grade inclusion or
consistency proofs today.
Single-writer append remains the base concurrency model. Divergence is reconciled by explicit replay
onto a new head, not automatic merge.

## 7. Errors / parity / concurrency

- **Errors:** current source uses stable core codes, including `INTEGRITY_FAILURE` for broken chains.
- **Parity (0032):** the source-backed linear chain is portable and has no engine dependency. Native
  checkpoint and proof serving stays target until structured storage and principal signing land.
- **Concurrency:** single writer, appends serialized via ref CAS (0018 section 2.3); merge is fast-forward-only
  (a hash chain cannot be order-independently merged).

## 8. Open Questions

### OQ-LE1 - Expose Merkle inclusion/consistency proofs? (resolved target direction)

- **Context.** 0018 ships a linear chain and lists a Merkle accumulator as optional future work; a faithful
  transparency-log presentation needs inclusion and consistency proofs the linear chain cannot give.
- **Decision.** Keep the source-backed linear chain as the current base layer. Promote structured
  ledger storage first, then add signed checkpoints through the shared principal signing substrate.
  The approved structured-storage target is segment-native: immutable sequence-range segments,
  canonical head metadata, and a canonical segment index. Merkle accumulators and proof indexes are
  derived artifacts unless a later storage-promotion decision makes a selected proof commitment part
  of canonical identity.
- **Signing direction.** Checkpoints use a closed signature-suite registry rooted in Loom principals.
  Ed25519 and OpenPGP/PGP are designed as suite or provider options; Ed25519 is the first
  implementation target unless source review finds a blocker. OpenPGP/PGP is not a Loom identity
  root.
- **Retention direction.** Retention metadata, prune planning, and explicit retained or pruned range
  errors are part of the target contract before physical pruning is implemented.
