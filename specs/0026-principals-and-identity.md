# 0026 - Principals & Identity (who acts on a shared Loom)

**Status:** Partial, core principal registry source-backed. **Version:** 0.1.0-target.
**Optional capability:** `identity`.

**Depends on:** 0001 (store-global engine state), 0005 (single-file format and journaled control
state), 0009 (key primitives and at-rest security), 0009a (future signatures and governance), 0003
(core interface, error taxonomy), 0008 (wire protocols; where
authentication is enforced). **Required by:** 0027 (access control evaluates grants against the principals defined here),
0029 (a trigger's `run_as` is a principal defined here). **Promoted from:**
`PRINCIPALS-AND-ACCESS-CONTROL-LANDSCAPE.md` (Concept 0), which this document supersedes as the
normative source.

This document specifies identity for a shared Loom: what a principal is, how it proves who it is, where
identities live, and the lifecycle that turns a single-owner store into an authenticated, multi-user
one. It defines no permissions; what a principal MAY do is 0027. It is gated by the `identity`
capability; absence means the Loom runs in unauthenticated root mode (§4) and these methods yield
`UNSUPPORTED`.

## Current implementation

Current source implements the core principal registry in `loom_core::identity` and persists it through
the `FileStore` durable-local control root. `IdentityStore` creates an initial root principal with no
passphrase, treats that as unauthenticated root mode, switches to authenticated mode when a passphrase
is set or a second principal is added, verifies local passphrases with Argon2id, records
session-to-principal context, persists five built-in roles (`admin`, `reader`, `writer`, `operator`,
`service`), records principal role membership, permits root removal only after a credentialed admin
replacement exists, rejects recovery lockout with `IDENTITY_NO_ROOT_CREDENTIAL`, and encodes a
session-independent durable snapshot. Each principal has an immutable UUID identity, a current
lowercase handle, a mutable display name, and a durable handle registry. A handle rename retains the
previous handle as a reserved alias resolving to the same UUID. The explicit-handle identity snapshot
format is the sole supported format; pre-handle development formats are rejected rather than inferred
or migrated. The CLI, IDL, C ABI, Node, Python, C++, Swift, JVM, and WASM expose explicit
handle creation or rename management and project handles in identity-list output. Android and React
Native currently derive the initial handle from the submitted display name and do not yet expose
explicit handle creation or rename management. Source also backs two non-passphrase credential records.
Principal-owned app-specific API-key credentials let an admin create a credential, receive the secret
once, list only credential metadata, revoke by credential id, and authenticate hosted requests through
that token without a profile-local credential store. External credentials record the shared enterprise
provider profile for public-key, mTLS certificate, passkey, OIDC subject, and SAML subject bindings;
the profile carries or references verifier material through its issuer, subject, and optional material
digest. Core also stores generic external-credential challenges with nonce, issue time, expiry,
single-consume semantics, expiry pruning, and revoke cleanup so challenge-based verifiers do not
create provider-specific identity stores. Core verifies already-validated provider assertions against
external credential records. Hosted HTTP and gRPC metadata can now carry those verified external
assertions for all five kinds and bind them through the same principal/session path as passphrase and
app-specific credentials. Hosted direct proof verification is source-backed in standard builds for
public-key, mTLS, passkey/WebAuthn, OIDC, and SAML proof envelopes. FIPS builds source-back
public-key verification through AWS-LC and mTLS binding to rustls-verified peer certificates. FIPS
builds intentionally fail closed for direct OIDC, SAML, and passkey proof verification; those
credential kinds enter a FIPS deployment through already-verified external assertions from a compliant
identity provider or gateway, not through Loom-local non-provider verifier crates.
`FileStore::save_identity_store` and `identity_store` persist and restore the identity snapshot outside
workspace history; sessions are intentionally not persisted.
`loom-ffi` exposes `LoomSession` authentication (`loom_authenticate_passphrase`,
`loom_clear_authentication`) and source-backed principal lifecycle management
(`loom_identity_list_json`, `loom_identity_add_principal`, `loom_identity_set_passphrase`, and
`loom_identity_remove_principal`) plus role assignment and revocation (`loom_identity_assign_role`,
`loom_identity_revoke_role`), external credential management (`loom_identity_create_external_credential`,
`loom_identity_revoke_external_credential`), and public-key lifecycle management
(`loom_identity_add_public_key`, `loom_identity_revoke_public_key`). Per-call FFI opens reload the
persisted identity snapshot and rebind the authenticated session before running engine operations.
The CLI initializes root identity and ACL control state, exposes
`identity list/add/set-passphrase/remove/assign-role/revoke-role`, `identity public-key
add/list/revoke`, and binds `--auth-principal` plus `--auth-key-source` into every opened `Loom`. The
checked-in C++, Swift, Node, Python, JVM, Android, React Native, and WASM bindings expose session
authentication plus local identity management, including role assignment and revocation where the
platform exposes management methods, external credential list/create/revoke metadata projection, and
public-key add/revoke lifecycle projection. The `identity` behavior runner proves bootstrap,
authentication failure and success, admin-replacement root removal, recovery lockout rejection, and
session behavior; CLI and FFI tests prove the public management path. Successful local identity
management mutations through the C ABI and CLI append durable-local audit records that include the
acting principal and redacted target, without recording passphrases. The local MCP host uses the CLI
launch context:
`loom mcp <store> ... --auth-principal <uuid> --auth-key-source <source>` resolves a local passphrase
once, stores the principal/session context in `LocalOpenAuth`, and reattaches it on each daemon-backed
per-request open before any MCP tool or resource reaches the engine. Missing served principal auth
fails as `AUTHENTICATION_FAILED`; an authenticated principal with insufficient grants reaches the PEP
and fails as authorization. `crates/loom-hosted` now source-backs the first hosted authentication
kernel: hosted requests carry principal, passphrase, app-specific credential, verified external
assertion, direct proof, and session context; local opens attach that context before the engine sees a
read or write; missing or invalid auth maps to `AUTHENTICATION_FAILED`; and hosted REST, JSON-RPC, and
gRPC-shaped tree-adapter tests prove the same principal behavior across the shared kernel. Served SQL,
calendar, contacts, and mail auth gates also use that kernel in the hosted protocol matrix. Source also backs `admin/rest` and `admin/json_rpc`
listeners for listener management, identity list/add/set-passphrase/remove/role-assign/role-revoke,
ACL management, protected-ref management, and audit list/export/config/prune under global Admin
authorization, with direct hosted-admin adapter and daemon-listener tests for the identity route
families, including app-specific API-key and external credential list/create/revoke routes. Hosted MCP
is explicitly bounded today: local `loom mcp` launch auth is source-backed, and durable
`loom serve configure <store> mcp` records intent, but the daemon does not start hosted MCP
auth/session routes yet. Source also backs the first store-global authority-state substrate inside
`IdentityStore`: current authority mode, authority principal, monotonic generation, optional
authority head digest, canonical COSE-style handoff record storage and payload validation, and
forced-detach state all encode and decode with the durable identity snapshot. Local identity list JSON,
the C ABI identity list JSON, and `idl/loom.idl` now project the authority state, handoff list, and
forced-detach record. The CLI and hosted admin REST/JSON-RPC surfaces also source-back audited
authority force-detach operations that record the local authority fork reason and generation. Source
also backs durable principal-bound public verification keys in the identity snapshot, `ES256`
authority handoff verification against those keys, rejection of mismatched key ownership or invalid
signatures, identity-list projection of trusted public keys through local JSON, C ABI JSON, hosted
admin JSON, promoted bindings, and IDL snapshot shapes, and public-key lifecycle management through
local CLI, hosted admin REST/JSON-RPC, C ABI, IDL, and promoted binding surfaces. Source also backs
deterministic authority witness records with publication digests, local CLI plus hosted admin
REST/JSON-RPC witness projection, and a strict fast-forward authority replication primitive that
rejects stale sources, same-generation forks, detached forks, and non-continuous signed handoff
chains. The CLI also source-backs an authenticated manual authority pull operation that opens a source
store, fast-forwards the destination authority snapshot through that primitive, persists it, audits it,
and returns the resulting witness report. Source now backs durable authority replication policy
records with pull-on-start, interval, deterministic jitter, backoff, and witness-report settings; the
daemon reconciler reads those policies, applies strict fast-forward authority pulls, records success or
failure state, and audits each scheduled pull or failure. Source does not yet
implement complete hosted MCP auth management routes, real network auth profiles for all configured
listeners, cross-protocol auth certification beyond the current hosted matrix, or control-region fork
reconciliation.

### Remote-surface boundary: authority administration is local-store only (by design)

The authority detach/witness/replication administration commands — `identity force-detach-authority`,
`identity authority-witness`, `identity replicate-authority`, `identity configure-authority-replication`,
`identity list-authority-replication`, and `identity remove-authority-replication` — are **local-store
administration by design and are not part of the remote `LoomClient` surface.** They operate directly on
a store file's authority-state substrate and its durable replication-policy records through the
`FileStore` authority APIs; `idl/loom.idl` deliberately projects only the authority *types* (state,
handoff, forced-detach), not authority *administration methods*. This is a settled boundary, not an open
future-IDL question: a remote locator for any of these commands is rejected with one stable
local-admin-boundary error rather than being forwarded. (Recorded per the 2026-07-14 owner decision;
Queue 11 task 530 tracks the stable error + rejection tests. Ordinary identity mutations — principal,
role, external-credential, public-key, and app-credential lifecycle — remain in scope for the remote
surface; see Queue 11 tasks 510/520.)

## Remaining target work

- (P0) Remaining hosted protocol authentication projection. 0008 owns the transport mechanics, but
  this spec owns the identity contract: every hosted REST, JSON-RPC, gRPC, served SQL, PIM, and hosted
  MCP request must resolve its presented credential to the same principal/session model used by local
  Loom sessions before any operation reaches 0027. The shared passphrase-backed kernel and hosted
  matrix are source-backed; `admin/rest` and `admin/json_rpc` listener, identity, ACL,
  protected-ref, and audit management routes are source-backed; hosted MCP auth routes and full
  listener coverage remain target.
- (P0) Cross-protocol auth conformance inputs. 0010a and 0025 own certification, but this spec must
  define the principal/session facts those runners assert: successful auth binds one principal id,
  missing or stale auth fails as `AUTHENTICATION_FAILED`, and the same principal behaves consistently
  across local CLI, C ABI, bindings, MCP, the hosted kernel, and future protocol listeners.
- (P1) Additional credential-family verifiers. Passphrase verification, app-specific hosted API-key
  credentials, durable external credential records, external credential management projection across
  C ABI, IDL, C++, Swift, Node, Python, JVM, Android, React Native, and WASM, the core
  verified-assertion binder, generic challenge lifecycle, material-reference storage, hosted
  HTTP/gRPC verified-assertion metadata, and hosted direct-proof verification for public-key, mTLS,
  passkey/WebAuthn, OIDC, and SAML are source-backed in standard builds. FIPS public-key verification
  and FIPS mTLS peer-certificate binding are source-backed. FIPS direct OIDC, SAML, and passkey proof
  verification is explicitly unsupported until Loom has a certified provider-backed verifier profile;
  FIPS deployments use hosted verified-assertion metadata for those credential families.
  Cross-protocol conformance remains target work.
- (P1) Generated IDL identity projection. C ABI, CLI, C++, Swift, Node, Python, JVM, Android, React
  Native, and WASM have local source-backed identity/session coverage for the promoted surfaces.
  Generated bindings from `idl/loom.idl` remain target.
- (P1) Authority replication and handoff. Store-global identity state persists locally, and the
  authority-state substrate records authority mode, generation, head digest, canonical COSE-style
  handoff records with payload binding, forced-detach state, local identity-list projection, C ABI
  identity-list projection, hosted admin identity projection, IDL snapshot shape, audited CLI
  forced-detach operation, audited hosted admin REST/JSON-RPC force-detach operation, durable
  principal public verification keys, public-key lifecycle management across local, hosted, ABI, IDL,
  and promoted binding surfaces, and `ES256` signature verification for ordinary authority handoff
  application. Deterministic witness records, CLI/hosted witness projection, and strict fast-forward
  replication of signed authority snapshots are source-backed. The CLI also source-backs manual
  authority pull from a source store into a destination store with atomic identity persistence,
  auditing, and witness-report output. Durable authority replication policy records and daemon
  scheduling are source-backed for pull-on-start, periodic interval, deterministic jitter, backoff,
  success/failure state, and audit recording. Control-region fork reconciliation remains target.
- (P1) Shared principal signing substrate. Public-key lifecycle, ES256 authority handoff
  verification, Ed25519 principal-signature verification, closed signature-suite ids, and
  purpose-bound payload bytes are source-backed. The generic payload binds suite, purpose,
  principal, key id, and payload bytes; verification rejects wrong-purpose signatures, wrong-owner
  keys, disabled or missing keys, and suite/key mismatches. Fresh challenge verification, delegated
  agent and service-principal signing policy, key rotation semantics, revocation effects on
  historical signatures, key lifecycle audit, OpenPGP/PGP provider behavior, and private-key storage
  in a protected key-provider or control-plane layer remain target work. OpenPGP/PGP may be a suite
  or provider, but Loom principals remain the trust anchor.

## 1. Goals & non-goals

**Goals.** (G1) A stable notion of a principal (a human account, a service account, or a
bootstrap/recovery authority) that can be authenticated. (G2) Authentication decoupled from
authorization and from encryption (the three concerns are independent, §2). (G3) Pluggable
authentication methods, so password, public key, and certificate-based proof share one interface. (G4)
A clean lifecycle from an unauthenticated single-owner store to an authenticated multi-user one, with a
safe bootstrap and a no-lockout recovery invariant.

**Non-goals.** (N1) No permission model here; grants, roles-as-permission-bundles, and enforcement are
0027. (N2) No encryption coupling; whole-file encryption at rest (0009 §3) and end-to-end sync
confidentiality are orthogonal capabilities (§2, and `PRINCIPALS-AND-ACCESS-CONTROL-LANDSCAPE.md`
§3.1). (N3) Not an external identity provider; federation (OIDC, SAML) is a later transport concern,
not core identity. (N4) Authentication produces only an identity, never permissions (§5.3).

## 2. Three orthogonal concerns

Identity is one of three concerns that vary independently and MUST NOT be conflated:

- **Authentication (this document):** resolve a connection to a principal id, or reject it.
- **Authorization (0027):** given a principal id and a requested operation on a resource, allow or
  deny.
- **Encryption at rest (0009 §3):** whether the stored bytes can be read off disk, gated by a
  whole-file key set at creation.

A principal with no grants can authenticate and do nothing (authentication succeeded, authorization
denies everything), exactly as a freshly created database user can connect but touch no schema.

## 3. The principal

A **principal** is an actor that can be authenticated and authorized.

```idl
struct Principal {
  id:          PrincipalId,        // a UUIDv4 (0014 §2), stable for the life of the principal
  name:        string,             // human label, unique within the Loom, mutable
  kind:        PrincipalKind,      // User | Service | Root
  credentials: List<Credential>,   // zero or more; multiple methods may coexist (§5.3)
  roles:       List<RoleId>,       // role references; roles bundle grants (0027)
  enabled:     bool,
  created_at:  Timestamp,
  meta:        Map<string, bytes>,
}

enum PrincipalKind { User, Service, Root }
```

- **User** is a human account. **Root** is a bootstrap/recovery authority (§4), not an account that
  must remain in the store indefinitely. A root principal can be disabled or deleted once another enabled
  principal with a usable credential can perform `Admin` and recovery operations. **Service** is a
  non-human account for automation: a trigger keeper, a sync agent, or any standing program runs as a
  service principal (0029 §8). A service principal is created by an administrator and carries roles
  like any other principal; it is the durable identity a binding is reassigned onto (0029 §8) so
  automation does not depend on an individual.
- A principal MAY hold several credentials at once (for example a password and a public key); any one
  authenticates it (§5.3).
- `id` is stable and is the value other documents reference (a grant's subject in 0027, a binding's
  `run_as` in 0029). `name` is a mutable label and MUST NOT be used as an identity reference.

## 4. Operating modes and the bootstrap

A Loom is in one of two modes, recorded in the registry/superblock (0005 §3, 0014 §3):

- **Unauthenticated root mode (default).** Every Loom has an initial root principal with no password.
  Opening the store resolves to that root principal without prompting or enforcing authentication.
  Action logs record that principal as the actor, but access control is bypassed until authentication is
  enabled. This is the embedded single-user case and the behavior of a Loom with no `identity`
  capability in use.
- **Authenticated mode.** Every connection MUST authenticate to a principal, and every operation is
  checked against that principal's grants (0027).

The transition is the bootstrap:

```
create new .loom
  -> unauthenticated root mode: root principal exists, no credential required
  -> caller sets a root credential or creates a second principal
  -> authenticated mode becomes enforced
  -> bootstrap authority creates principals and assigns roles (0027)
  -> subsequent connections authenticate; implicit owner access ends
```

Normative bootstrap rules:

- A Loom starts with exactly one enabled root principal and no credential. That state is not a
  passwordless account exposed to the network; it is the local unauthenticated mode.
- Authentication MUST become enforced when a root credential is set or when a second user or service
  principal is added. From that point forward, the store cannot silently fall back to unauthenticated
  root access.
- Enabling authenticated mode without at least one usable root/admin/recovery credential MUST be
  refused (`IDENTITY_NO_ROOT_CREDENTIAL`), so a store cannot lock itself out.
- The mode change MUST be a single journaled transaction (0005 §6); a crash leaves the store in
  exactly one mode.
- Disabling or deleting a root principal is permitted only if the transaction leaves at least one
  enabled principal with a usable credential and `Admin`/recovery authority. This no-lockout invariant
  applies to credential removal, principal deletion, principal disablement, role removal, grant
  revocation, and authenticated-mode changes.
- Reverting authenticated mode to owner mode is permitted but MUST be an audited operation authorized
  by an enabled `Admin`/recovery principal. Audit and governance storage are defined by 0009a after
  the required substrates exist. The change is subject to the same no-lockout invariant in reverse
  (decision 13.2). An implementation MAY refuse reversion entirely under a hardened profile.

## 5. Authentication

### 5.1 Model

Authentication resolves a presented credential to a principal id:
`authenticate(Credential) -> Result<PrincipalId>`. It is enforced at the connection boundary: when a
Loom is exposed over a transport (0008), the connection authenticates once and the resolved principal
id is attached to every request on it. An embedded caller supplies a principal context directly. Where
the principal store physically lives (§6) is independent of where authentication is enforced.

### 5.2 Where principals live

The principal and ACL state is a **store-global control region** - bucket 2 in the 0001 §6.1 state-class
model: store-scoped engine state that travels with the `.loom` file under whole-file sync but is **not**
a versioned workspace. This supersedes any earlier framing of a "reserved internal workspace": placing
identity/policy in a workspace would subject it to per-workspace `checkout`/`branch`/`revert`, and a
revert of a revocation is a security footgun. The control region is therefore deliberately outside
workspace version control (it cannot be reached by any workspace VCS operation), while still being
internal Loom content that is persisted, carried by clone/bundle/whole-file sync, and auditable.

The region holds two parts that are managed only through the `Identity` facade (§7) and `Acl` (0027),
never by writing internal rows: the **live state** (current principals, their credentials, roles, and
grants - bounded by how many exist now) and an append-only **audit log** of policy events. Writes
require an enabled recovery principal or the `admin` right (0027). An implementation MAY project a
read-only `users`/`grants` view for inspection, but the region is internal structure, not a writable
schema.

### 5.2.1 Authority, replication, and history

Because the control region is store-global rather than a branchable tree, it has a single-writer model
(confirmed decision; see also 0027):

- **Single authority.** Exactly one loom is the policy authority at a time; the store creator (root) is
  the initial authority. The control region is a single-writer append-only chain with a `head` (chain
  hash) and a monotonic `generation`. Other replicas hold read-only mirrors.
- **Fast-forward replication.** Sync compares the mirror's `head`/`generation` to the authority's: a
  mirror behind the authority fast-forwards (pulls); a mirror that is not a strict ancestor is treated
  as tampered/corrupt and rejected. Mirrors never push policy, so there is no symmetric policy merge to
  reconcile (this avoids CRDT-merging security state). Current source backs the local fast-forward
  rule at the identity-store level: stale sources, equal-generation snapshot drift, detached forks, and
  broken handoff chains are rejected. Current CLI source backs manual destination pulls from another
  store and writes the accepted snapshot through the audited identity persistence path. Current daemon
  source also backs durable pull policies with pull-on-start, interval, deterministic jitter, backoff,
  success/failure state, and audited scheduled reconciliation.
- **Authority handoff.** Authority moves only by an explicit, audited, signed handoff: the current
  authority (or root) issues a signed `authority = X` record and X imports the live snapshot plus the
  (optionally checkpoint-compacted) log. Current source validates the canonical handoff record envelope,
  binds it to the exact handoff payload, and verifies `ES256` signatures against stored
  principal-bound public-key material. Root MAY force-designate a new authority for the
  lost-authority case.
- **Forced detach.** A replica MAY force itself to detach from an unreachable authority and become its
  own authority, but only with an explicit warning that it may no longer sync policy with the former
  authority (a fork of the control region). This is the deliberate escape hatch for "the server is
  gone"; it is audited.
- **History GC.** The live state is a compact snapshot bounded by the current principal/grant count, so
  principal churn never floods what syncs or what the enforcement point reads. Only the audit log grows;
  it is retained on the authority with a retention window and periodic signed **checkpoints** ("state as
  of generation N") so the tail can be truncated without breaking the integrity chain.
- **Witness publication.** Current source exports deterministic authority witness records with a
  snapshot digest, latest-handoff digest, canonical record bytes, and record digest. Local CLI and
  hosted admin REST/JSON-RPC expose that record for publication to an external witness log.

### 5.3 Pluggable methods

Each method is a verifier behind one interface, so methods are added without touching authorization.
For v1, password and app-specific API-key verification are source-backed. External credential records
reserve the shared provider profile for public-key, certificate, passkey, OIDC, and SAML verifiers.
Generic challenge lifecycle is also source-backed. Hosted direct proof verification is source-backed
for public-key, mTLS, passkey, OIDC, and SAML in standard builds. FIPS builds currently source-back
public-key proof verification and mTLS binding. Direct OIDC, SAML, and passkey proof verification is
not a FIPS surface; those methods use verified external assertions from a compliant provider or
gateway until a certified Loom provider profile exists.

| Method | Proof | Stored per principal | Primitive (permissive) | Source-backed verifier |
| ------ | ----- | -------------------- | ---------------------- | ---------------------- |
| Password | client sends a password; server checks a salted hash | an Argon2id verifier string | `argon2` (MIT OR Apache-2.0) | Yes |
| App-specific credential | hosted client sends a revocable token or password-class secret | credential id, label, enabled flag, and secret verifier | existing KDF/hash primitives | Yes |
| Public key | server issues a nonce; client signs it; server verifies | external credential record plus public-key material digest and generic challenge | `ring` in standard builds; AWS-LC in FIPS builds | Yes |
| mTLS certificate | TLS layer validates a client certificate chain; Loom maps the verified certificate subject | external credential record plus certificate or trust material digest | `rustls` plus trusted peer-certificate capture | Yes |
| Passkey/WebAuthn | browser or platform authenticator signs a challenge | external credential record plus WebAuthn credential id or material digest and generic challenge | `caden` candidate behind the hosted verifier | Standard build only |
| OIDC/SAML subject | token or assertion is verified against issuer metadata and mapped to subject | external credential record plus issuer, subject, metadata digest, and optional state/nonce challenge | OIDC/SAML verifier crates or gateway integration | Standard build only |

Password (Argon2id) and Ed25519 reuse the same cryptographic families already used or planned by the
storage security and authenticity specs, so v1 adds no new cryptographic dependency class. A principal
MAY carry more than one credential; any one authenticates it.

Recommended verifier library selection:

| Credential family | Recommendation | Boundary |
| --- | --- | --- |
| OIDC | Use `openidconnect` with default features disabled for protocol-level OIDC validation. `jsonwebtoken` is only a JWT primitive fallback, not a full OIDC verifier. | Standard build direct verifier; FIPS deployments use already-verified external assertions unless a certified provider-backed profile is added. |
| SAML | Use `opensaml` as the SAML protocol and XML-signature candidate. Track newer SAML crates only after MSRV, maturity, and license fit are proven. | Standard build direct verifier; FIPS deployments use already-verified external assertions unless a certified provider-backed profile is added. |
| Passkey/WebAuthn | Prefer `caden` for permissive licensing and smaller footprint. Keep `webauthn_rp` as an alternate verifier candidate. | Standard build direct verifier; browser and platform authenticators still bind through Loom's challenge and external credential records. |
| mTLS certificate identity | Reuse rustls client-certificate verification for hosted TLS, then bind the verified peer leaf to the external credential record. Use `x509-parser` with a FIPS-compatible verification path only when direct certificate chain verification is needed outside rustls. | Standard and FIPS builds; FIPS binding relies on rustls-verified peer certificate material. |
| Public-key challenge | Use AWS-LC-backed verification for FIPS profiles and `ring` for standard profiles. Pure Rust signature crates are useful reference probes but are not the FIPS served path. | Standard and FIPS builds with provider-specific enforcement. |

### 5.4 The three authentication layers

Authentication is not one thing; three distinct layers each "verify" something different, and a given
credential is only usable at some of them. Conflating them is a category error.

- **L1 - whole-file decryption** (0009/0034). Acquiring the key that decrypts the `.loom` bytes. The
  "verifiers" here are *key sources* (passphrase to KEK via KDF; raw KEK released or derived from a
  file/fd, OS keystore, secure enclave, WebAuthn-PRF passkey, KMS, or HSM). Multi-wrap (0009/0034)
  already lets many independent unlock paths open the same DEK - this is the file-layer multi-unlock
  mechanism.
- **L2 - principal authentication** (this spec). Proving *which principal* is acting so 0027 can apply
  its grants. Source-backed verifiers are the §5.3 password path, already-verified external
  assertions, public-key proof, mTLS peer-certificate binding, and standard-build passkey, OIDC, and
  SAML direct proofs, with generic challenge state available for challenge-bound verifiers. FIPS OIDC,
  SAML, and passkey direct proof verification remains target work. L2 only has teeth where access is
  *mediated*: at any served boundary always, and at the file level only when L1 encryption is on - on a
  plaintext file, raw-file access is root-equivalent (0001 §6.1 trust boundary; 5.1c), so L2 alone is
  theatre there.
- **L3 - hosted protocol authentication** (0008). The remote protocol's own scheme. Mainstream clients
  for CalDAV (0037), and likewise SQL and other hosted endpoints, present **username + password** over
  TLS; they do not do challenge-response, client certificates, or biometrics. L3 therefore consumes a
  password-class verifier and resolves it to a principal.

Verifier-to-layer applicability:

| Verifier | L1 file decryption | L2 principal auth | L3 hosted (SQL, CalDAV, ...) |
| --- | --- | --- | --- |
| passphrase / password (Argon2id) | yes (KDF to KEK) | yes | yes |
| Ed25519 keypair (in OS keystore) | yes (release/derive KEK) | yes (sign nonce) | no |
| biometric to secure enclave | yes (gates the key/KEK) | yes (gates the key) | no |
| WebAuthn-PRF passkey | yes (PRF to KEK) | yes (PRF to principal key) | no |
| KMS / HSM | yes (KEK) | possible (sign) | no |
| app-specific credential | no | n/a | yes |

### 5.5 Credential overlap and app-specific credentials

A principal carries a **set** of credentials, and each entry point selects which it accepts. Two
consequences follow from §5.4:

- **One biometric, both local layers.** A single enrolled secure-enclave key or WebAuthn-PRF passkey
  can serve both L1 and L2. The clean construction is a **PRF split**: one passkey yields two derived
  secrets - one contributes the file KEK (L1), the other is the principal's auth key (L2) - rather than
  reusing one raw key for two purposes (which would couple the layers). Multi-user multi-unlock falls
  out of existing mechanisms: file-layer multi-wrap gives each user an independent DEK wrap and each
  user has their own principal key, so every user enrols once and coexists, with no shared secret.
- **App-specific credentials for L3.** A biometric/passkey principal cannot present that to a CalDAV or
  SQL client, which needs a transmittable shared secret. A principal MAY mint **any number of
  app-specific credentials** - scoped, individually named, and individually revocable password-class
  secrets that resolve to the principal at L3 only (the Apple/Google "app password" pattern). These are
  the L3 credential model generally, not specific to CalDAV; SQL and other hosted protocols use the same
  mechanism. Vendor-compatible HTTP profiles may present the same stored credential as an API key
  header, for example a Pinecone-style `Api-Key` header or a bearer token accepted by a hosted vector
  profile. The credential still resolves to a Loom principal before 0027 policy enforcement, and the
  profile does not own a separate credential database. Revoking one app-specific credential does not
  affect the principal's primary L1/L2 credentials or its other app-specific credentials.

## 6. Roles

A **role** is a named bundle of grants (the grant type and its evaluation are defined in 0027). A
principal references roles, and its effective permission is computed in 0027 from its roles plus any
direct grants. Identity owns the principal-to-role assignment; authorization owns what a role permits.

```idl
struct Role { id: RoleId; name: string; }   // the grants a role carries are defined in 0027
```

A service principal (§3) is the typical holder of a role created for automation; an administrator
creates the service principal, assigns it a role, and reassigns a trigger's `run_as` onto it (0029
§8).

## 7. Interface sketch (`Identity` facade)

Present iff the `identity` capability is advertised. Illustrative IDL only (non-normative); the
authorization facade is specified in 0027.

```idl
interface Identity {
  // Bootstrap (§4); recovery/admin-only thereafter.
  set_root_credential(cred: CredentialSpec): void
  enable_authentication(): void               // refused unless a recovery credential exists
  disable_authentication(): void              // recovery/admin-only, audited (decision 13.2)
  // Principal management (recovery authority or the `admin` right, 0027).
  create_principal(name: string, kind: PrincipalKind, creds: List<CredentialSpec>): PrincipalId
  add_credential(id: PrincipalId, cred: CredentialSpec): void
  remove_credential(id: PrincipalId, cred_id: CredentialId): void
  // App-specific credentials (§5.5): any number, scoped to hosted (L3) use, individually revocable.
  create_app_credential(id: PrincipalId, label: string): AppCredential   // returns the secret once
  revoke_app_credential(id: PrincipalId, cred_id: CredentialId): void
  // External provider credentials (§5.3): durable records for already-verified provider assertions.
  create_external_credential(id: PrincipalId, spec: ExternalCredentialSpec): ExternalCredential
  revoke_external_credential(cred_id: CredentialId): ExternalCredential
  // Control-region authority (§5.2.1).
  transfer_authority(to: PrincipalId): void    // signed, audited handoff
  force_detach_authority(): void               // escape hatch; warns it may no longer sync policy
  disable_principal(id: PrincipalId): void
  delete_principal(id: PrincipalId): void
  assign_role(id: PrincipalId, role: RoleId): void
  // Session.
  authenticate(cred: Credential): Session      // resolves a principal id; permissions are 0027
}
```

New error codes (register in 0003 §8 / 0010 §4): `IDENTITY_NO_ROOT_CREDENTIAL` (enabling auth without
a bootstrap/recovery credential), `AUTHENTICATION_FAILED` (a credential does not resolve to a
principal). Permission failures are `PERMISSION_DENIED` (0027).

## 8. Interaction with existing specs

- **0027** - evaluates grants for the principals defined here; a recovery principal and the `admin`
  right are the management authorities for both documents.
- **0029** - a trigger's `run_as` is a principal id; a service principal is the durable identity for
  automation, reassignable by an administrator (0029 §8).
- **0009** - reuses storage security and key primitives without coupling identity to at-rest
  encryption.
- **0009a** - owns future signature, audit, retention, and governance surfaces that refer to
  principals.
- **0008** - authentication is enforced at the connection/transport boundary (L3, §5.4); a transport
  carries the resolved principal id, it does not define identity. Hosted SQL, CalDAV (0037), and other
  endpoints resolve a password-class or app-specific credential (§5.5) to a principal.
- **0001** - the principal/ACL state is bucket 2 (store-global engine state) per §6.1, not a versioned
  workspace; `id` values are UUIDv4.
- **0034** - L1 key sources (§5.4) and an L2/biometric credential can share one enrolled enclave/passkey
  via a PRF split (§5.5); 0034 owns the key-acquisition side.

## 9. Security

The adversary is an unauthenticated or under-privileged client (the access-control threat model, 0027
§Security, and 0009 §1). Identity's contribution is to resolve every connection to a principal so that
authorization has a subject. Authentication failures fail closed (no principal, no access). Credential
material at rest (Argon2id password verifiers, public keys, app-specific credential hashes) lives in the
bucket-2 control region (§5.2) and is covered by whole-file encryption at rest where enabled (0009 §3);
password verifiers are Argon2id, not recoverable plaintext. Identity does not defend the at-rest bytes
or the operator-with-the-file threat; that is encryption's door (0009 §3, and
`PRINCIPALS-AND-ACCESS-CONTROL-LANDSCAPE.md` §3, §3.1).

## 10. Resolved decisions

1. **System-workspace exposure surface.** Principal and grant management goes through the `Identity`
   facade in v1. A CLI, web binding, server API, or SDK may surface principals and grants by calling
   facade methods. A later implementation may add read-only `users`/`grants` views for inspection, but
   a writable virtual table is out of v1 because it would create two write paths into one audited
   internal store.
2. **Root lifecycle and administrators.** Root is a bootstrap/recovery authority, not a fixed
   permanent account. A root principal may be disabled or deleted once another enabled principal with a
   usable credential can perform `Admin` and recovery operations. Larger deployments grant `Admin` to
   additional user or service principals; the invariant is no lockout, not preserving a root row.
3. **Bootstrap mode.** Every Loom starts with an initial root principal that has no password. That is
   unauthenticated root mode: action logs can name the root principal, but authentication and ACL
   enforcement do not begin until a root credential is set or a second principal is added.
4. **State class.** Principal/ACL state is bucket 2 (0001 §6.1) - store-global engine state carried by
   whole-file sync, deliberately outside per-workspace version control so a `checkout`/`revert` can
   never roll back a revocation. It supersedes the earlier reserved-workspace framing (§5.2).
5. **Single authority + fast-forward replication.** One loom is the policy authority (root initially);
   the control region is a single-writer append-only chain with `head`/`generation`; mirrors
   fast-forward and never push policy, so there is no policy merge (§5.2.1).
6. **Authority handoff and forced detach.** Authority moves only by signed, audited handoff; current
   source validates the canonical record envelope, payload binding, and `ES256` signature against
   stored principal-bound public-key material. Root may force-designate. A replica may force-detach
   from an unreachable authority with an explicit warning that policy may no longer sync (a
   control-region fork) - the "server is gone" escape hatch (§5.2.1).
7. **History GC.** Live state is a bounded snapshot (principal churn never floods it); the audit log
   grows on the authority and is bounded by retention plus signed checkpoints (§5.2.1).
8. **Three authentication layers.** L1 file decryption (0009/0034), L2 principal auth (this spec), L3
   hosted protocol auth (0008). A credential applies only at certain layers; L2 has teeth only where
   access is mediated or the file is encrypted (§5.4).
9. **Multi-verifier principals and app-specific credentials.** A principal carries a set of credentials;
   it MAY mint any number of scoped, individually revocable app-specific credentials for L3 (SQL,
   CalDAV, and other hosted endpoints alike). A biometric/passkey MAY back both L1 and L2 via a PRF
   split (§5.5).

## 11. Sources

- Design note: `PRINCIPALS-AND-ACCESS-CONTROL-LANDSCAPE.md` (Concept 0; decisions 13.1, 13.2).
- Key and KDF primitives: `specs/0009-security-and-capabilities.md` §5, §3.1; audit log §7.
- Workspace and registry precedent: `specs/0014-workspaces.md` §2, §3.
- argon2 (MIT OR Apache-2.0): https://github.com/RustCrypto/password-hashes. ed25519-dalek
  (BSD-3-Clause): https://crates.io/crates/ed25519-dalek. ssh-key (MIT OR Apache-2.0):
  https://github.com/RustCrypto/SSH.
