# ADR-0007 - Whole-Loom encryption at rest, and end-to-end sync topologies

**Status:** Accepted · **Date:** 2026-06-19 (revises the 2026-06-16 "E2E as a per-namespace capability" decision) · **Deciders:** Nas
**Related:** 0031 (end-to-end encrypted sync), 0009 §3 (encryption at rest; AEAD/suite scheme §3.1;
keying §3.2; sync topologies §3.4), 0002 §3.8 (digests over plaintext), 0027 §6 (the trust boundary),
ADR-0005 (separation of concerns), 0015 (programs).

## Context

A `.loom` may need to be unreadable off-disk, and a hosted deployment may need a remote that stores a
user's data without being able to read it (the Signal/iCloud/Tarsnap model). An earlier draft of this
ADR made end-to-end encryption a **per-namespace** capability, so a deployment could mix private
(encrypted) and server-computable (plaintext) namespaces inside one Loom. On review that granularity
is **rejected**: a per-namespace plaintext region is an in-Loom exfiltration target. Because a
`program` (0015) lives in its own namespace and may be **granted read/write on other namespaces**, a
malicious program planted in a plaintext namespace A could read an encrypted namespace B and write the
cleartext into a plaintext namespace C. Whole-Loom encryption removes that entire class of in-Loom
plaintext target, and matches the originally stated owner model (whole-file). This ADR records the
corrected shape.

## Decisions

1. **Encryption at rest is whole-Loom, chosen at creation.** A `.loom` is created either unencrypted or
   encrypted; when encrypted, *every* namespace and object is sealed uniformly under one key hierarchy
   (0009 §3.1). There is **no** per-namespace encryption selection. (Supersedes the prior decision.)

2. **"Whole-Loom" is a uniform policy, not a monolithic ciphertext.** Objects are still sealed
   **individually** under per-object keys - which is exactly what keeps content addressing, dedup,
   sync, and selective-namespace pull working (Decision 6). Addresses are over plaintext (0002 §3.8),
   so sealing changes no address.

3. **Suite-agile AEAD, random keying.** Each frame carries a 1-byte AEAD suite id (XChaCha20-Poly1305
   `0x01` default, AES-256-GCM `0x02`, reserved for AES-256-GCM-SIV / a PQ AEAD), bound into associated
   data. Random keying is the only mode; convergent keying is not offered (confirmation-of-file attack
   plus cross-client reference-count bookkeeping). The default suite is chosen at creation and switched
   later by an in-place `rekey` that re-seals under stable addresses. (0009 §3.1-§3.2, RD1-RD6.)

4. **Three independent doors remain (ADR-0005 #8).** Authorization (0027) protects the live interface;
   whole-Loom encryption (0009 §3) protects the at-rest bytes; `e2e-sync` (0031) protects content from
   a remote host. They compose; conflating them was the first-pass error.

5. **`e2e-sync` is a sync topology decided by who holds the key - not an encryption granularity.** A
   whole-Loom-encrypted local Loom can sync its sealed objects to a remote in three shapes:
   - **Opaque remote (zero-knowledge):** the remote lacks the key; it stores/relays ciphertext by
     opaque label and runs have/want on labels, but cannot read or integrity-verify content. Safe
     redundant storage / backup. The receiving client verifies on decrypt.
   - **Keyed remote:** the remote holds the key; it is a fully accessible replica that can read,
     verify, and compute on the data.
   - **Selective mount (e.g. a browser):** a third host mounts the remote with the decryption
     credential and selectively pulls a minimal set of namespaces (to avoid downloading the whole
     Loom), then syncs on demand.

6. **Selective namespace pull is a bandwidth optimization, not a confidentiality boundary.** Per-object
   sealing lets a client materialize only the namespaces it wants; every object carries the same
   whole-Loom key regardless of which namespaces are pulled.

7. **A host can compute on a Loom iff it holds the key.** A keyed remote computes (SQL, vector,
   programs); an opaque remote is blind storage with all compute client-side. This is the whole-Loom
   form of the old "compute fork" - now a property of key possession, not of which namespace.

## Alternatives considered and rejected

- **Per-namespace encryption selection (the prior decision)** - rejected: a plaintext namespace is an
  in-Loom exfiltration target for a granted program (Context), and a mixed-trust surface is harder to
  reason about than a single whole-Loom bit. The hosted "private + computable in one Loom" case is
  served instead by two Looms (one encrypted, one not), or by a keyed remote.
- **Convergent keying** - rejected: enables a confirmation-of-file attack and forces cross-client
  reference-count bookkeeping on the host (0009 §3.2).
- **Host-side decryption / host holds keys for the zero-knowledge case** - rejected: defeats the threat
  model (the host is the adversary, 0031 §6).
- **Streaming-from-host as the default client** - rejected in favour of local-first (0031 §5).

## Consequences

- **Positive:** one trust bit per Loom; no in-Loom plaintext exfiltration target; the three sync
  topologies are clean; content addressing makes sealing nearly free; a suite-agile frame makes a
  FIPS/PQ cipher change a transparent `rekey`, not a format break.
- **Negative / accepted:** a Loom that needs server-side compute on a remote that must *not* read it
  cannot have both in one Loom - split into an encrypted Loom (opaque remote) and an unencrypted/keyed
  Loom. Metadata (sizes, counts, DAG, ref names) still leaks unless the heavier opt-ins are added
  (0031 §8). A lost client secret means lost data unless escrow is configured (0031 OQ2). Some
  enterprise procurement flags any client-side-key scheme; accepted.
- **Follow-on:** 0009 §3 / 0005 §4.3 carry the suite id and whole-Loom keying (RD1-RD7); the `loom` CLI
  gains `init --encrypt [--suite ...]` and a `rekey` command (surface reserved until the at-rest layer
  lands, milestone 8); the `Code` enum keeps `E2E_LOCKED`/`E2E_KEY_INVALID` (0003 §8); `e2e-sync` stays
  registered (0010 §4) with per-namespace selection removed from its parameters.

## Open

- **Program cross-namespace scope (0015 §4).** 0015 currently says cross-namespace effects are "out of
  scope for v1," which contradicts the program model assumed here (a program in its own namespace with
  granted read/write on others). Resolving that wording is tracked separately; whole-Loom encryption
  makes it not a confidentiality issue, but it remains an access-control (0027/0028) decision.
- Key recovery/escrow and multi-device key sharing remain the open questions in 0031.
