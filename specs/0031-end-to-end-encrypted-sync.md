# 0031 - End-to-End Encrypted Sync (zero-knowledge replica)

**Status:** Target, with current source-backed boundary documented. **Version:** 0.1.0-target.
**Optional capability:** `e2e-sync`.

**Depends on:** 0009 (encryption at rest §3, the AEAD/suite scheme §3.1, random keying §3.2, end-to-end
sync §3.4, what-is-hidden §3.3), 0002 (digests are computed over plaintext, 0002 §3.8, which is what
lets sealing leave addressing intact), 0006 (synchronization; have/want negotiation), 0005 (single-file
frames), 0007 (the `wasm32` browser binding), 0027 (the host authorizes access by label while blind to
content). **Relates to:** 0026 (the user whose credential derives the key is a principal), 0014
(selective workspace pull). **Promoted from:** `PRINCIPALS-AND-ACCESS-CONTROL-LANDSCAPE.md` §3.1, which
this document supersedes as the normative source.

This document specifies confidentiality of a Loom from the host that stores it: a client seals content
so an untrusted or honest-but-curious server can store, relay, and synchronize the data without being
able to read it, while the client retains full read/write access. It is the Signal/iCloud/Tarsnap
pattern adapted to Loom's content-addressed model. It is gated by the `e2e-sync` capability; absence
means a synced host can read plaintext (the trusted-host model, 0027 §6). Encryption itself is
**whole-Loom** (0009 §3.1, ADR-0007); this capability selects the **sync relationship to a remote** -
whether that remote holds the key - and is distinct from both authorization (0027) and local at-rest
encryption (0009 §3).

## Current implementation

The current Rust workspace implements the storage crypto substrate but not blind-replica hosted sync.
Source-backed behavior today includes:

- whole-Loom encrypted `.loom` stores;
- per-object AEAD frames whose plaintext digests stay unchanged;
- passphrase and raw-KEK create/open paths across the core store, CLI, C ABI, and selected bindings;
- `encryption_meta` decoding and encoding for the source-tagged multi-wrap schema
  `LKM1 || 2 || suite || u16{n} || entries`, while create and cheap rekey replace the current wrap set
  with one passphrase or raw-KEK wrap;
- multi-wrap add/remove, duplicate-wrap rejection, CLI and C ABI wrap management, and selected
  binding projections for adding/removing passphrase and raw-KEK wraps;
- cheap rekey and full reseal;
- stable `E2E_LOCKED` and `E2E_KEY_INVALID` error codes;
- direct workspace sync and offline bundle sync over local, verified objects.

The source does not yet implement a hosted blind-replica protocol, keyed opaque remote labels,
label-key derivation, selective encrypted pull, remote authorization by label, metadata-hardening
transports, E2E sync ABI or binding APIs, or E2E sync conformance.

## 1. Goals & non-goals

**Goals.** (G1) A host stores and syncs a Loom it cannot read: it holds ciphertext frames identified by
keyed opaque labels, not raw plaintext digests. (G2) The client retains full access, deriving keys from
its own credential, which never reaches the host. (G3) Sync still works: have/want negotiation runs on
opaque labels (0006); under random keying a keyless host cannot dedup ciphertext across independently
keyed Looms, which is the accepted cost (0009 §3.2). (G4) The host still authorizes (0027) by
workspace and ref label while blind to content, so confidentiality and authorization compose. (G5) The
client runs in the browser: the crypto compiles to `wasm32` (0007 §7).

**Non-goals.** (N1) Not server-side compute on a host that cannot read the Loom. A keyless host cannot
run SQL, vector search, or programs (0015); it is sync-and-storage only, and compute happens
client-side or on a keyed remote (§7). (N2) Not metadata hiding by default. Sealing hides content, not
object sizes, counts, the DAG shape, or ref names (0009 §3.3); hiding those needs the heavier opt-ins
of §8. (N3) Not a replacement for at-rest encryption (0009 §3), which protects local bytes on disk; the
two compose. (N4) Not per-workspace encryption: encryption is **whole-Loom** (0009 §3.1, ADR-0007).
This capability governs the sync relationship; a client MAY pull a **subset of workspaces** for
bandwidth (§7), but that is a transfer choice, not an encryption boundary - every object carries the
same whole-Loom key.

## 2. The model

The enabling fact is that Loom addresses every object by the digest of its **plaintext** (0002 §3.8).
Sealing therefore does not change local object identity. A blind remote, however, does not receive raw
plaintext digests. It receives keyed opaque labels derived from those digests.

1. The client computes each object's digest over plaintext, exactly as today.
2. The client seals each object's payload with the whole-Loom AEAD suite (XChaCha20-Poly1305 default,
   0009 §3.1) under a per-object content key derived from the data-encryption key (DEK). The local
   frame binds pre-decrypt metadata such as the suite id, plaintext digest, frame id, and lengths into
   associated data so a frame cannot be relocated or have its suite substituted.
3. For blind sync, the client derives a **sync label key** from the DEK with HKDF-SHA-256:

   ```text
   sync_label_key = HKDF-SHA-256(DEK,
                                 info = "uldren-loom/e2e-sync-label-key/v1")
   ```

   It then computes each remote label with that key:

   ```text
   remote_label = HMAC-SHA-256(sync_label_key,
                               "uldren-loom/e2e-sync-label/v1" ||
                               plaintext_digest ||
                               object_kind)
   ```

   `object_kind` is a closed protocol discriminator such as object frame, ref label, or workspace
   label. The host stores `remote_label -> ciphertext frame` and never receives the plaintext digest.
4. On pull, the client decrypts and **verifies on decrypt** (the recovered plaintext MUST hash to the
   plaintext digest that produced the remote label, 0009 §4); the host cannot verify integrity and is
   not trusted to (§6).

The host can run have/want reconciliation on remote labels (0006 §4) but can neither read content,
derive plaintext digests, test known-content hashes, integrity-verify frames, nor compare equality
across Looms that use different label keys. Equality within one Loom remains visible where sync needs
stable labels.

## 3. Key management

The DEK never reaches the host. It is wrapped (envelope encryption, 0009 §3.1) by a key the user
controls, and only the **wrapped** DEK is stored, on the host or in the Loom metadata, useless without
the user's secret:

- **Derivation.** The unwrapping key is derived client-side from the user's credential: a passphrase
  via Argon2id (0009 §3.1), or a secret held by a passkey / WebAuthn / the OS or browser keystore. The
  password and the unwrapping key never leave the client.
- **Unwrap.** On login the client fetches the wrapped DEK and unwraps it locally (in the wasm sandbox
  for a browser client), then seals on write and opens on read.
- **Rotation.** Rotating the user credential or the DEK re-wraps the DEK (cheap) without re-encrypting
  objects, because per-object keys derive from the DEK and addresses are over plaintext (0009 §3.1,
  the `rekey` interaction); re-encrypting objects under new content keys is a separate offline pass.

This is the established server-stores-an-encrypted-key-blob pattern (iCloud, 1Password, Signal):
recovery and multi-device access reduce to giving another device the user's secret, never the host.

## 4. The blind replica (host role)

A host serving an E2E Loom runs in a **blind replica** mode:

- it stores and relays ciphertext frames addressed by keyed opaque labels;
- it runs sync negotiation (0006) on labels, with no cross-Loom ciphertext dedup under random keying
  (0009 §3.2);
- it enforces access control (0027) at the granularity of workspaces and refs, by label, so it can
  decide *who may sync which workspace* without reading content;
- it MUST NOT be relied on for integrity verification (it cannot recompute a plaintext digest); the
  receiving client verifies (§2, 0009 §4).

So the host still does real work: durable long-term storage, authorization by label, and relay between
a user's devices and tabs. It simply never sees plaintext.

## 5. Delivery flavors

A client reaches an E2E Loom in one of two shapes (the browser case, where a wasm binary in several
tabs connects to a hosted Loom over TLS, is the worked example):

- **Flavor A - stream from the host.** The host is authoritative; the client opens objects on demand,
  the host streams the sealed frames, the client decrypts and seals writes back, holding only a
  working-set cache. Cost: a round trip and a decrypt per access, online-only.
- **Flavor B - local replica, sync on demand (RECOMMENDED).** The client holds a local replica (in
  IndexedDB/OPFS via the wasm binding for a browser), fully usable offline, and syncs sealed frames to
  the host on demand. The host shrinks to durable storage, authorization, and relay between the user's
  tabs and devices.

Flavor B is recommended because it is what Loom already is (local-first data that syncs, README use
case 2) plus sealing, so it reuses the sync engine (0006) rather than inventing a remote object
protocol; the client stays fast and offline-capable; and multiple tabs reconcile through a shared
local replica or independently through the host. Both flavors keep the key client-side and hand the
host only ciphertext, so Flavor A trades the local-first benefits for nothing in confidentiality.

## 6. Threat model

The adversary is A-server (0009 §1): an honest-but-curious or compromised host holding the Loom.
Protected: object content (confidentiality), via client-held keys and per-object AEAD. Also protected
in the blind-sync protocol: raw plaintext digests, known-content hash tests, and cross-Loom object
equality. Detected: tampering, because the client verifies every object against its plaintext digest on
decrypt (0009 §4), so a host that alters or substitutes a frame is caught. Not protected by base
sealing alone: object sizes, counts, the DAG shape within one Loom, and ref names the host still
observes (0009 §3.3); §8 lists the heavier opt-ins that reduce this leakage. Transport confidentiality
and authentication are TLS/mTLS (0009 §1, 0008 §5); E2E sealing is orthogonal to and composes with the
transport.

## 7. Key-holding topologies and the compute fork

Encryption is whole-Loom; what `e2e-sync` selects is the **sync relationship to a remote**, decided by
**who holds the key**:

- An **opaque remote (zero-knowledge)** does not hold the key: it stores and relays ciphertext by
  keyed opaque label, runs have/want on labels, and authorizes by label, but cannot read, verify,
  compute on the data, or derive raw plaintext digests. It is safe redundant storage / backup; all
  compute on it happens client-side.
- A **keyed remote** holds the key (it was given the credential): it is a fully accessible replica that
  can read, verify, and compute (SQL, vector search, programs, 0015), defended by TLS, at-rest
  encryption (0009 §3), and authorization (0027) like any local Loom.
- A **selective mount** (e.g. a browser) holds the key and pulls a **minimal set of workspaces** to
  avoid downloading the whole Loom, then syncs on demand. Selective pull is a **bandwidth** choice
  enabled by per-object sealing (0009 §3.1); it is **not** an encryption boundary.

So the compute-vs-confidentiality fork is a property of **key possession**, not of which workspace: a
keyless remote is blind storage, a keyed remote computes. A deployment that needs both a zero-knowledge
backup and server-side compute uses two relationships (an opaque remote for backup, a keyed remote for
compute), not a mix of encrypted and plaintext workspaces inside one Loom (ADR-0007).

## 8. Metadata hardening (opt-in, heavier)

Sealing leaves metadata observable (§6). A deployment that must also reduce metadata leakage adds, at
cost, the 0009 §10 menu items: `padded-objects` (pad frame sizes to buckets so sizes do not fingerprint
content), `encrypted-refnames` (so the host sees opaque ref labels), and `obfuscated-dag` (so commit
structure is hidden). These are independent capabilities, each with a performance and dedup cost, and
are out of scope for the base `e2e-sync` capability.

## 9. Interface sketch

Illustrative only (non-normative). E2E is configured for the Loom at creation or via an explicit
conversion, and a remote is opened either in blind-replica (keyless) or keyed mode.

```idl
interface E2E {
  // Mark a Loom end-to-end encrypted; derives a DEK and stores it wrapped under the user key.
  enable_e2e(loom: LoomHandle, key: KeySpec): void          // KeySpec = passphrase | keystore | passkey
  // Unlock for the session: derive the unwrap key client-side and unwrap the DEK locally.
  unlock(loom: LoomHandle, secret: Secret): Session
  rotate_key(loom: LoomHandle, new_key: KeySpec): void       // re-wrap the DEK; no object re-encryption
  // Pull only a subset of workspaces for bandwidth (a transfer choice, not an encryption boundary, §7).
  pull(loom: LoomHandle, workspaces: NsSelector[]): SyncReport
}
// A host advertises a blind-replica role (it stores frames and authorizes by label, never decrypts).
```

The E2E-specific errors are already registered as stable core codes: `E2E_LOCKED` (an operation needs
an unlocked DEK), `E2E_KEY_INVALID` (the secret did not unwrap the DEK), and `INTEGRITY_FAILURE`
(existing, 0009 §4) on a post-decrypt digest mismatch.

## 10. Interaction with existing specs

- **0009** - this is the promotion of end-to-end-encrypted sync (0009 §3.4) into a named capability;
  it reuses the AEAD/suite/envelope scheme (§3.1), random keying (§3.2), and the integrity rule (§4),
  and inherits the metadata-leakage statement (§3.3). Encryption is whole-Loom (§3.1), not per
  workspace.
- **0002** - relies on digests over plaintext (§3.8) so sealing does not change addressing.
- **0006** - blind sync negotiates over keyed opaque labels derived from plaintext digests; the
  receiver verifies; whether a derived result syncs is unchanged (rebuild on receiver, 0013 RD4).
- **0027** - the host authorizes by workspace and ref label while blind to content; authorization and
  confidentiality are independent doors that compose (0027 §6).
- **0007** - the client crypto compiles to `wasm32` (RustCrypto: `chacha20poly1305`, `aes-gcm`,
  `argon2`), so a browser binary is a full client.
- **0014** - a client MAY selectively pull a subset of workspaces for bandwidth (§7); this is a
  transfer choice, not a per-workspace encryption boundary.

## 11. Resolved decisions

1. **Whole-Loom encryption:** encryption remains whole-Loom. Selective pull is a bandwidth choice, not
   a per-workspace encryption boundary. Baked into N4 and §7.
2. **Random keying only:** keying is random only for the whole Loom (0009 RD3). Convergent keying is
   not offered, including per workspace. Baked into §1 and §2.
3. **Blind-remote labels:** blind remotes receive keyed opaque labels, not raw plaintext digests. The
   client keeps plaintext digest addressing locally and verifies every decrypted object against it.
   Baked into §2 and §6.
4. **Key recovery:** optional recovery uses user-controlled escrow, such as an additional passphrase or
   hardware-backed recovery key wrapping the DEK. Host-held recovery is rejected because it defeats the
   zero-knowledge remote threat model. Baked into §3.
5. **Multi-tab and multi-device key sharing:** v1 devices derive the unwrap key independently from the
   user credential or host-supplied KEK. Device-to-device key handoff is a separate enrollment
   capability, not required for v1. Baked into §3 and §5.

## 12. Unfinished work

- (P0) Define and implement the blind-replica label profile tracked by
  `0031a-blind-replica-sync.md`: HKDF-SHA-256 sync label-key derivation, HMAC-SHA-256 remote labels,
  label domains, label length, object-kind discriminators, and conformance vectors.
- (P0) Implement blind-replica hosted sync and remote have/want negotiation over keyed opaque labels.
- (P0) Project E2E sync through IDL, C ABI, bindings, wire protocols, capability reporting, and
  executable conformance before any host claims the `e2e-sync` capability.
- (P0) Add remote authorization by workspace/ref label after 0027 defines the served policy
  enforcement point.
- (P1) Implement selective encrypted pull as a transfer policy over workspace selectors, not as an
  encryption boundary.
- (P1) Extend user-controlled recovery/provider acquisition after 0034 completes provider-specific key
  sources; multi-wrap add/remove is source-backed, but provider acquisition and remaining binding
  parity are still target work.
- (P1) Add conformance for locked behavior, wrong keys, tamper detection, opaque-label stability,
  known-digest non-disclosure, cross-Loom unlinkability, selective pull, and binding/protocol parity.
- (P2) Add metadata-hardening conformance only for options the host advertises, such as padding,
  encrypted ref names, and DAG obfuscation.

## 13. Sources

- End-to-end sync, AEAD/suite/envelope, random keying, what-is-hidden: `specs/0009-security-and-capabilities.md`
  §3.1-§3.4, §3.3, §4, §10.
- Digests over plaintext: `specs/0002-data-model.md` §3.8. Sync negotiation: `specs/0006-synchronization.md` §4.
- Host authorization by label: `specs/0027-access-control.md` §6. Browser/wasm baseline:
  `specs/0007-bindings.md` §7.
- Whole-Loom encryption + topologies decision: `specs/adr/adr-0007-whole-loom-encryption.md`.
- Design note and the browser worked example: `PRINCIPALS-AND-ACCESS-CONTROL-LANDSCAPE.md` §3.1, §3.1.1.
