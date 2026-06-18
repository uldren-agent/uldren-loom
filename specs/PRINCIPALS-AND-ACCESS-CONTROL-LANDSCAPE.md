# Principals & Access Control - Landscape and Options

**Status:** Exploratory / research note. **Version:** 0.1.0-draft. **Normative?** No.
**Promoted to:** 0026 (Principals & Identity), 0027 (Access Control, workspace-level), and 0028
(Fine-grained Access Control), which are now the normative sources for those parts; this note is
retained as the research input and remains the home of the end-to-end-encryption analysis (§3.1), which
is a separate future `e2e-sync` capability, not part of the access-control series.
**Relates to:** 0009 (security and capabilities, especially section 5 signing and section 6 access
control), 0015 (the `StateAccess` capability model and the integrated CEL guard), 0014 (workspaces),
0003 (core interface), 0008 (wire protocols), 0005 (single-file format).

> **Partially superseded (2026-06-19).** Where this note discusses *convergent keying* or
> *per-workspace encryption selection*, it is out of date: encryption at rest is now **whole-Loom** with
> **random keying only** (convergent dropped), and `e2e-sync` is a whole-Loom sync topology decided by
> who holds the key, not a per-workspace choice. The normative sources are 0009 §3, 0031, and ADR-0007.
> The rest of this note (the three-door model and the landscape analysis) still stands as research input.

Working data-gathering doc. It refines the question "how does a shared Loom limit what a person can
access" into a model close to how a database server manages users and grants, separates the concerns
that were tangled in the first pass, and scopes the Rust libraries. Nothing here is committed. House
conventions apply: no em-dashes, no emoji, every claim reads as current fact.

## 1. Why this exists, and the three-spec split

Loom is embeddable and, today, single-actor: whoever opens the file or calls the API has full access.
The moment a Loom sits behind a shared service (a REST API, per 0008 and IMPLEMENTATION-PLAN.md P9),
"who may do what" becomes a real question. 0009 section 6 sketches an answer (capability tokens,
protected refs, path-scoped grants) but stops short of a model an implementer can build.

The target, stated plainly by the owner, is the database-server model: connect to a server, a root
user creates accounts, and root grants each account read or write access to things, at the workspace
level at minimum and ideally finer. Authentication is not limited to passwords; public keys and
signed certificates should work too.

That target decomposes into three pieces with a strict dependency order, and each is a spec on its
own:

1. **Principals and identity (the foundation).** What an actor is, how it proves who it is, where
   identities are stored. Nothing can be authorized until there is something to authorize.
2. **Authorization, workspace-level (the minimum viable shared Loom).** An on/off auth mode, the root
   bootstrap, accounts, and grants of read or write at the workspace level. This is the spec that
   makes a shared Loom usable.
3. **Fine-grained access control (the enhancement).** Grants below the workspace: by branch/ref, by
   path or subtree, by facet, and by operation class. Reuses the grammar of the compute layer.

This note covers all three together because they share one model; the eventual specs separate them so
the minimum slice can ship without waiting on the granular one.

## 2. The target model, mapped to a database server

| Database-server concept | Loom analogue | Where it already exists |
| ----------------------- | ------------- | ----------------------- |
| The server you connect to | a Loom instance opened behind a service | 0008 wire, P9 server |
| `mysql.user` and friends (users live in the database) | principals stored in a reserved system workspace inside the Loom | new; uses 0014 workspaces |
| Authentication plugins (password, PAM, key) | a pluggable authenticator resolving a credential to a principal | new; uses 0009 section 5 keys |
| `GRANT SELECT/INSERT ON db.* TO user` | a grant of read/write over a workspace (and finer) to a principal | extends 0009 section 6 |
| Roles | named grant bundles assigned to principals | new |
| The grant tables are themselves stored in the database | the principal and grant store is itself versioned Loom content | uses 0014, 0002 |

The key structural decision this mapping forces, and the first piece of guidance: **in the database
model, grants are stored state that the server consults, not bearer tokens the client carries.** MySQL
keeps its users and grants in tables inside the database and evaluates them on every statement. That
is the opposite emphasis from 0009 section 6, which leads with macaroon-style bearer tokens. Both have
a place, but for this target the source of truth is a **stored principal-and-grant model inside the
Loom**, evaluated by the engine. Tokens become a derived, optional convenience for stateless API
sessions and for delegation (section 9), layered on top of the stored model rather than replacing it.

A refinement the owner added: **a user-visible "users table" is not a requirement.** The principal and
grant store is an internal structure; it can be projected as a virtual `users` table for inspection,
but nothing forces that surface. What matters is the **enforcement boundary**: authentication becomes
real at the connection. If a Loom is started so that it exposes, for example, a MySQL-compatible port
over TLS, then a client authenticates at connect time and every statement on that connection is
checked against the stored grants. So "where principals live" is an implementation detail (section
5.2); "where authentication is enforced" is the connection or transport surface (0008), and that is
the part a deployment actually configures.

## 3. Three orthogonal concerns (the correction to the first pass)

The first-pass framing offered two "options" that conflated layers. The owner's pushback is correct,
and the clean model separates three concerns that vary independently:

- **Authentication (authn): who are you?** Resolve an incoming connection to a principal id, or
  reject it. Pluggable across methods (section 5.3). Says nothing about permissions.
- **Authorization (authz): what may you do?** Given a principal id and a requested operation on a
  resource, allow or deny. This is the grant model (section 6). Says nothing about how you proved who
  you are.
- **Encryption at rest: can the stored bytes be read off disk?** A whole-file gate, set by a
  passphrase or certificate when the store is created (0009 section 3). Says nothing about principals
  or operations.

The first pass got two things wrong, now corrected:

- It described authz as something built "into the environment" around Loom. That is rejected. **Authz
  belongs inside Loom**, evaluated by the engine, so the same rules hold whether the Loom is reached
  through the REST API, a future gRPC surface, the CLI, or a direct embedding. The server (P9) only
  transports a request and carries the authenticated principal id; it does not own the policy. Section
  7 covers how an in-process engine enforces this without an external system.
- It tied the stronger option to encryption, implying authz and confidentiality were one choice. They
  are not. **Encryption is a separate, whole-file concern.** The owner's stated model is exactly
  right: opening a brand-new store, you may set one passphrase or certificate to encrypt the whole
  file, with no per-workspace or finer encryption. That envelope (0009 section 3.1, a master key
  wrapping a data-encryption key) gates whether the bytes decrypt at all; it is orthogonal to which
  principal may read which workspace once the file is open.

The honest consequence of separating them, which the spec must state (section 8): authz protects the
**API**, not the **disk**. A principal restricted to read-only cannot write through Loom, but anyone
who holds the file and its encryption key can read everything, exactly as a database administrator
with filesystem access can read a database's data files regardless of `GRANT`. Whole-file encryption
defends the at-rest bytes; authz defends the live interface. Two doors, two locks.

### 3.1 End-to-end encryption to an untrusted host (the Signal / iCloud question)

> **Promoted to 0031** (End-to-End Encrypted Sync, capability `e2e-sync`), which is now the normative
> source. This section is retained as the analysis that motivated it.

The owner asked a sharp follow-on: if a third party hosts a Loom on an open port over TLS, and a local
Loom syncs to it, how does the client encrypt so the host cannot read the data while the client still
has full access? And what does SQLite actually encrypt, the whole file or the tables?

On SQLite first, because it sets the right expectation. Public-domain SQLite has no built-in
encryption. The encryption add-ons (the commercial SEE, and the open SQLCipher) encrypt the **whole
database file at the page level**: every page, including the schema page, is encrypted and decrypted
in transit to and from disk. There is no "encrypt one table" mode; it is all pages or none. That is
the same shape as Loom's whole-file at-rest envelope (section 3), and it defends the same door: bytes
on disk.

The Signal/iCloud question is a different door, and Loom already specifies the mechanism for it: 0009
section 3.4, end-to-end-encrypted sync. The shape:

- The client computes every object's digest over its **plaintext** (0002 section 3.8 already addresses
  objects by plaintext digest), so content addressing and dedup keep working after encryption.
- The client seals each object with an AEAD under a per-object key derived from a data-encryption key,
  which is itself wrapped by a master key the client holds (from a passphrase via Argon2id, or from a
  certificate or keystore). The master key never leaves the client.
- The client pushes **opaque ciphertext frames labeled by the plaintext digest**. The host stores
  `digest -> frame`, runs have/want negotiation on the opaque labels (so sync still works), and can
  dedup only under convergent keying (0009 section 3.2). It cannot read content and cannot verify
  integrity; it trusts the labels.
- The client pulls frames and verifies on decrypt (the digest must match the recovered plaintext).

So "how does the client set up the certificate" is: configure the master key material on the local
Loom, and run the remote as a ciphertext-only replica that never receives key material. TLS protects
the transport; the envelope protects the content from the host itself. This is the restic / Tarsnap /
Mega pattern, and Loom is unusually well-suited to it because addresses are computed over plaintext
already, so sealing does not disturb addressing.

How hard: moderate, and low in novelty. The pieces are the encryption-at-rest capability (per-object
AEAD plus envelope keys, 0009 section 3.1, buildable from permissive RustCrypto crates such as
`chacha20poly1305`, `aes-gcm`, and `argon2`), client-side key management, and a "blind" replica mode
for the remote provider that stores and relays frames by opaque label without decrypting or verifying.
The careful parts are integrity (the receiving client must verify, since the host cannot), key
rotation, and metadata leakage: even with content sealed, the host still sees object sizes, counts, the
DAG shape, and ref names (0009 section 3.3) unless heavier opt-ins (padding, encrypted ref names) are
added.

The fork that matters for a hosted "World Computer", and the one to decide deliberately: **a host can
either compute on your data or be unable to read it, not both.** If the host runs SQL, vector search,
or programs (0015) on a workspace, it needs that workspace in plaintext, so you trust the host and rely
on TLS, at-rest encryption, and authz (the two doors above). If the workspace is end-to-end encrypted,
the host is reduced to a sync and storage replica and all compute happens client-side. You can mix this
per workspace: keep private workspaces end-to-end encrypted (dumb storage) and keep computable
workspaces in plaintext on the trusted host. That selective mix is the one case that genuinely wants
**per-workspace or per-subtree keys** (0009 open question 4), which refines the earlier
"whole-file only" stance: whole-file keying is right for local at-rest, but selective end-to-end
confidentiality to an untrusted host wants finer keys. Worth naming as its own future capability
(`e2e-sync` or zero-knowledge replica), separate from both authz and the local at-rest envelope.

#### 3.1.1 Worked example: a centralized host and a browser client

The concrete case the owner posed: a central server hosts the workspaces assigned to a user; the user
opens three browser tabs, each loads the Loom wasm binary and connects to the hosted workspaces over
TLS; the host must be unable to read workspace data, yet the logged-in browser must read and write
freely. The mechanism above applies; what makes it work is where the key lives.

The workspace's data-encryption key never reaches the host. The host stores only a **wrapped** copy of
it (a key blob encrypted under a key the user controls), which is useless without the user's secret.
On login, the browser derives the unwrapping key client-side (Argon2id over the user's password, or a
secret held by a passkey / WebAuthn / the browser keystore), fetches the wrapped key, and unwraps it
inside the wasm sandbox. From then on the browser seals before every push and verifies after every
pull; the host sees only ciphertext frames labeled by plaintext digests and never the key, the
password, or the plaintext. This is the iCloud / 1Password / Signal pattern: the server keeps an
encrypted key blob, the client derives the unwrap key from the user's credential. TLS protects the
connection; the envelope protects the content from the host itself.

Two flavors, as the owner framed them:

- **Flavor A (stream from the host).** The host is authoritative; the browser opens objects on demand,
  the host streams the sealed frames, the browser decrypts in wasm and seals writes back. The browser
  holds only a working-set cache. Cost: a round trip and a decrypt per access, online-only, and the key
  must still be present in every tab.
- **Flavor B (local replica, sync on demand).** The browser holds a local replica in
  IndexedDB/OPFS via the wasm binding, fully usable offline, and syncs sealed frames to the host on
  demand. The host's job shrinks to durable long-term storage, authentication and authorization of who
  may sync which workspace, and relaying sealed frames between the user's tabs and devices.

**Flavor B is the recommended fit**, because it is what Loom already is (local-first data that syncs,
README use case 2) plus E2E sealing, so it reuses the sync engine (0006) rather than inventing a
remote object protocol; the browser stays fast and offline-capable; and the three tabs reconcile
through a shared local replica (a shared worker over OPFS) or independently through the host. Flavor A
gives up the local-first benefits and pays per-access latency for no gain in confidentiality, since
both flavors keep the key client-side and hand the host only ciphertext.

The clean separation this example makes vivid: the host still does useful work in the encrypted case,
but only on **labels and refs**, never content. Authorization (which principal may sync which
workspace, 0027) operates on workspace ids and refs; confidentiality (the host cannot read bytes)
operates on content. The host authorizes by label and stores blind. The wasm crypto is not a barrier:
the RustCrypto primitives (`chacha20poly1305`, `aes-gcm`, `argon2`) compile to `wasm32`, which is
Loom's browser baseline already (0007 §7).

## 4. Two operating modes, and the bootstrap

The "root by default until I turn auth on" behavior the owner described is the embedded-vs-server
distinction that SQLite and MySQL embody. Model it as a single mode flag on the Loom, recorded in the
registry or superblock (0005 section 3, 0014 section 3):

- **Owner mode (default).** No principals, no authentication, full access. This is today's behavior
  and the embedded single-user case. Opening the file is the authorization. A `.loom` created and used
  locally never has to think about users.
- **Authenticated mode.** Every connection must authenticate to a principal, and every operation is
  checked against that principal's grants. The implicit owner is replaced by explicit principals and a
  bootstrap/recovery authority.

The transition is the bootstrap, and it is a small state machine:

```
create new .loom
   -> owner mode: caller is the implicit owner, no credentials needed   (use freely)
   -> caller sets a bootstrap/recovery credential (password / public key / certificate)
   -> caller enables authenticated mode  (atomic: recovery credential must exist first)
   -> recovery authority adds principals
   -> recovery authority grants each principal read/write at workspace level (or finer, spec 3)
   -> subsequent connections authenticate; owner-mode access is gone
```

Two safety rules the spec must pin: enabling authenticated mode without a usable recovery credential is
refused (otherwise the store locks itself out), and the mode flip is a single journaled transaction
(0005 section 6) so a crash cannot leave a half-open store. Returning to owner mode is an explicit,
audited operation authorized by an enabled recovery/admin principal, or it is simply disallowed in a
hardened profile.

## 5. Concept 0: principals and identity

### 5.1 What a principal is

A principal is an actor that can be authenticated and authorized: a human account, an automation
account (a trigger keeper, see EVENTS-TRIGGERS-LANDSCAPE.md), or a service. The minimum record:

```
Principal {
  id:          PrincipalId,        // stable, e.g. a UUIDv4 like a workspace id (0014)
  name:        string,             // human label, unique, mutable (e.g. "root", "analytics-ro")
  credentials: Vec<Credential>,    // one or more, see 5.3; multiple methods per principal
  roles:       Vec<RoleId>,        // named grant bundles (section 6)
  enabled:     bool,
  created_at, meta,
}
```

This is the keys-to-principals-to-roles shape: an identity can carry one or more credentials (a
password and a public key), and roles bundle grants so a principal's effective permissions are the
union of its direct grants and its roles' grants.

### 5.2 Where principals live

The principal store and the grant store are themselves Loom content, in a reserved system workspace
(call it `system` or `_auth`), mirroring how programs live in a `program` workspace (0015 section 7.1)
and schedules in a `trigger` workspace (EVENTS-TRIGGERS-LANDSCAPE.md section 4). Benefits: the store
travels with the `.loom` file, it is journaled and crash-consistent like everything else, and its
change history is auditable. Unlike a normal workspace it is not a merge target and is reachable only
through the auth facade, never through `fs`/`db`/etc. Writes to it require recovery/admin authority.
(0014 registry auditability is the relevant precedent: a versioned system workspace gives the "who
changed a grant and when" history that pure mutable metadata would not.)

### 5.3 Authentication is pluggable

Authn resolves a presented credential to a principal id. Each method is a verifier behind one trait,
so methods are added without touching authz:

| Method | How it proves identity | Stored per principal | Rust primitive | License |
| ------ | ---------------------- | -------------------- | -------------- | ------- |
| Username + password | client sends password; server checks against a salted hash | an Argon2id verifier string | `argon2` (RustCrypto) | MIT OR Apache-2.0 |
| Public key (ssh-style) | server issues a nonce; client signs it; server verifies | the principal's public key | `ed25519-dalek`; `ssh-key` for OpenSSH key formats | BSD-3-Clause; MIT OR Apache-2.0 |
| Signed certificate / address (EVM-style) | client signs a challenge; server recovers the signer address or validates a cert chain | the principal's address or certificate | `k256` (secp256k1, `ecrecover`-style); an X.509 crate for cert chains | MIT OR Apache-2.0 |

All three reduce to one interface: `authenticate(connection_credential) -> Result<PrincipalId>`. The
Ed25519 and Argon2id primitives are already in the security story (0009 section 5 signs commits with
Ed25519; 0009 section 3.1 derives keys with Argon2id), so two of the three methods reuse machinery the
project already commits to. The EVM-style path (prove control of a secp256k1 address by signing a
challenge, recover and compare) is the same `ecrecover` pattern smart-contract wallets use to prove
identity, and `k256` provides it in pure Rust.

A subtle but important point for the spec: authn produces only an identity. It never produces
permissions. A freshly authenticated principal with no grants can do nothing, exactly as a freshly
created MySQL user can connect but touch no database.

## 6. The authorization model: one grammar for code and people

The compute layer already defines a fine-grained capability grammar for what a *program* may touch:
manifest grants, each a triple of facet plus scope plus mode (0015 section 6.1, implemented in
`crates/loom-compute/src/capability.rs` and encoded by `crates/loom-compute/src/manifest.rs`). A
principal's grant is the same idea for a *person*. Using one grammar for both is the central design
economy of this note: a program is constrained code and a principal is a constrained actor, and they
share a resource algebra.

A grant adds the version-control axes that 0009 section 6 names (ref-glob, path-glob, rights) to the
compute layer's facet-plus-scope-plus-mode:

```
Grant {
  workspace: NsSelector | AllOfType(NsType) | All,   // which tree(s); the coarsest axis
  ref_glob:  Glob,            // which branches/tags within it (default "*")
  scope:     All | Prefix(bytes),   // path prefix (files), key/table prefix, etc. (0015 section 6.1)
  facet:     Facet,           // files | sql | kv | graph | vector | ... (0015 section 6.1)
  rights:    BitSet<Right>,   // read, write, advance, merge, admin, exec
}
```

`Right` extends 0009 section 6's `{read, write, advance, admin}` with `merge` (promote into a
protected ref) and `exec` (invoke the compute facade), so the version-control and compute operations
are first-class permissions rather than special cases.

Granularity is a dial, and it is exactly the two-spec boundary:

- **Workspace-level (spec 2, the minimum).** A grant whose `ref_glob`, `scope`, and `facet` are all
  wildcards is "read/write this whole workspace," which is the `GRANT ... ON db.* TO user` equivalent
  and the owner's stated minimum. Most real policies start and stay here.
- **Fine-grained (spec 3, the enhancement).** Narrow any axis: read-only on `branch/main` but
  read-write on `branch/dev`; write only under the path prefix `reports/`; the `vector` facet but not
  `files`; `advance` but not `merge`. The grammar already supports it; spec 3 defines the matching
  rules and the deny-precedence semantics.

Roles bundle grants (`Role { id, name, grants }`) and a principal references roles plus optional
direct grants. Effective permission is the union of allows **with explicit denies taking precedence**,
adopted from the start rather than deferred (decision 13.3): a `deny` grant always wins over an
`allow`, so "read-write this workspace except the `secrets/` prefix" is expressible the moment finer
scoping lands, and the evaluation rule never has to change underneath existing grants. The reasoning is
the data-safety one the owner raised: an access model cannot be re-interpreted retroactively without
risk, so the safe move is to define deny-precedence once, even though the workspace-level spec may not
yet issue deny grants. Protected refs (0009 section 6: fast-forward-only, require signed commits,
minimum reviews) are policy attached to a ref rather than to a principal, and compose with grants at
the enforcement point.

## 7. Where enforcement lives: one policy enforcement point, inside Loom

A single Policy Enforcement Point (PEP) sits at the engine boundary: every facade call
(`fs_write_file`, `vcs_merge`, `db.insert`, `exec.apply`, ...) passes through a check
`authorize(principal, operation, resource) -> Allow | Deny(PERMISSION_DENIED)` before it touches
state. Because the check is in the engine, it holds for every caller:

- **Embedded / in-process.** A program that opens an authenticated Loom and provides a principal
  context is checked in-process. This is what makes authz "in Loom, not the environment": there is no
  external gatekeeper, the engine itself refuses.
- **Served (REST/gRPC, P9).** The server authenticates the connection (section 5.3), attaches the
  resolved principal id to the request, and calls the same engine with that context. The transport
  carries identity; it does not define policy. 0008 section 5 already anticipates projecting auth to
  transports.

A new error code `PERMISSION_DENIED` joins the stable `Code` enum (0003 section 8; the enum is a
stable contract per AGENTS.md, so this is an additive variant). Path and ref scoping use glob matching
(`globset`, MIT OR Apache-2.0).

This raises a real boundary question the spec must answer: the embeddable `loom-core` is, by design,
the thing that holds all the bytes. If authz lives in core, a caller linking `loom-core` directly and
constructing its own "root" principal context can bypass the check, just as anything with the raw file
and key can. So the enforced guarantee is precisely "authenticated access through the engine's checked
surface," and the deployment that wants it (the shared service) is the one that does not hand out
`loom-core` or the file. This is the same trust boundary as a database: `GRANT` constrains clients of
the server, not someone with shell access to the data directory. Naming this boundary honestly is part
of the spec, not a flaw to paper over.

## 8. Threat model, and the two doors

The owner asked to see the threat-model choice laid out. With the three concerns separated (section 3),
it is no longer an either/or; it is two independent doors a deployment locks separately:

- **Door 1, the live interface (authz).** Defends against an authenticated client doing more than it
  should: a read-only analyst writing, a tenant reading another tenant's workspace, an automation
  account merging to a protected ref. Enforced by the PEP (section 7). The host is trusted; it sees
  plaintext.
- **Door 2, the at-rest bytes (whole-file encryption).** Defends against someone reading or stealing
  the `.loom` file off disk. Enforced by the master-key envelope set at creation (0009 section 3.1).
  The host that serves the file must hold the key, so this door does not defend against a compromised
  host, only against a stolen file or a snooped disk.

What neither door alone gives you is confidentiality of a subtree *from the operator of a shared
server*. That requires the heavier per-workspace or per-subtree key separation discussed in 0009 open
question 4 (folding the workspace id into key derivation so a server holding one workspace's key cannot
read another). That is a third, separable capability, explicitly out of scope for the database-style
model the owner wants, and worth naming as a non-goal in spec 2 so it is a deliberate later choice
rather than an assumed property.

## 9. Library landscape for authorization

The stored-grant model (section 6) is first-party: the grammar extends the compute layer's manifest
grant vocabulary, and evaluating a grant against a request is a small matching function, not a
dependency.
The optional pieces are where libraries help.

| Library | Role in this design | License | `deny.toml` | wasm32 | Note |
| ------- | ------------------- | ------- | ----------- | ------ | ---- |
| **(first-party grant evaluator)** | the stored MySQL-style model: principals, roles, grants, the PEP | n/a | n/a | yes | the spine; extends the manifest-grant grammar in `crates/loom-compute/src/capability.rs` |
| **cel-interpreter** (CEL) | target conditional policy predicates (ABAC-style), reusing the selected compute guard language when promoted | Apache-2.0 | allowed | yes | Target work in 0015; conditional rules run read-only and fail closed |
| **biscuit-auth** | optional derived capability tokens for stateless API sessions and offline delegation/attenuation | Apache-2.0 | allowed | yes (official biscuit-wasm) | realizes 0009 section 6's "macaroon-style attenuable token"; carries its own Datalog, distinct from the CEL guard path; a holder mints a strictly-narrower token offline. Not used in v1 (decision 13.4) |
| **cedar-policy** | optional policy overlay for richer RBAC/ABAC and protected-ref rules | Apache-2.0 | allowed | yes | purpose-built authz language, has a formal-verification story; use where central policy beats per-principal grants |
| **regorus** (Rego) | alternative policy engine to CEL | Apache-2.0 | allowed | yes | only if Rego is preferred over the selected CEL target; otherwise redundant |
| **casbin** | alternative ready-made ACL/RBAC/ABAC model engine | Apache-2.0 | allowed | yes | viable, but config-model oriented; prefer the first-party grammar so code and principals share one model |
| **macaroon** | (not recommended) bearer tokens | MIT | allowed | - | unmaintained and self-described as not production-ready; biscuit supersedes it |

Guidance: build the workspace-level spec on the **first-party stored grant evaluator** (it extends the
compute layer's manifest-grant grammar and is a small matching function, not a dependency) plus the
authn primitives of section 5.3. Where a grant needs a condition rather than a flat allow (for example
"this role may write only outside business hours" or an attribute test), express it as a **CEL**
predicate once the 0015 guard layer is promoted. That keeps one predicate language across compute and
access control. Tokens (**biscuit**) and a heavier policy overlay (**cedar**) are deferred and not part
of v1 (decisions 13.3-13.4).

## 10. Pushback and guidance, summarized

1. **Lead with stored grants, not tokens.** The database model you want keeps users and grants as
   server-side state and consults them per operation. That is the source of truth. 0009 section 6's
   bearer-token emphasis is the delegation/transport layer, useful but secondary; do not build the
   foundation on it.
2. **Authn, authz, and encryption are three layers.** Your instinct is right. Keep whole-file
   encryption (one passphrase or certificate at creation) entirely separate from who-can-do-what.
   Authn resolves identity, authz resolves permission, encryption resolves at-rest readability.
3. **Authz lives in the engine.** Not in the environment. One PEP in `loom-core`, so the rules hold
   for the REST API, the CLI, and a direct embedding alike. The server only carries the authenticated
   principal id.
4. **One grammar for programs and people.** Reuse the compute layer's facet-plus-scope-plus-mode
   grants (already prototyped) and add the version-control axes (ref-glob, path-glob, and the `merge`
   and `exec` rights). A program is constrained code; a principal is a constrained actor; same algebra.
5. **Workspace-level first.** It is the minimum viable shared Loom and matches "at minimum at the
   workspace level." Ship it as spec 2; make finer scoping spec 3 on the same grammar.
6. **State the trust boundary honestly.** Authz protects authenticated access through the engine, not
   a person who holds the file and key. That is the database guarantee, and it is the right one to
   promise; naming it is required, not optional.
7. **Owner mode by default.** A new `.loom` is single-owner with no credentials, like SQLite. Enabling
   authenticated mode (after setting a recovery credential, atomically) turns it into the MySQL-style
   server. That single mode flag is a clean first spec boundary.

## 11. Sketches

Illustrative IDL for the auth facade (non-normative; consistent with 0003 and the database model):

```idl
interface Auth {
  // Bootstrap (owner mode -> authenticated mode); recovery/admin-only thereafter.
  set_root_credential(cred: CredentialSpec): void
  enable_authentication(): void              // refused unless a recovery credential exists
  // Principal management (recovery authority or a principal with `admin`).
  create_principal(name: string, creds: List<CredentialSpec>): PrincipalId
  add_credential(id: PrincipalId, cred: CredentialSpec): void
  disable_principal(id: PrincipalId): void
  // Grants and roles (the GRANT/REVOKE surface).
  grant(id: PrincipalId, grant: Grant): void
  revoke(id: PrincipalId, grant: Grant): void
  create_role(name: string, grants: List<Grant>): RoleId
  assign_role(id: PrincipalId, role: RoleId): void
  // Session.
  authenticate(cred: Credential): Session      // resolves to a principal; may mint a token (spec 3+)
}

struct Grant { workspace: NsSelectorOrWildcard; ref_glob: string; scope: Scope; facet: Facet; rights: RightSet }
enum Right { Read, Write, Advance, Merge, Admin, Exec }
```

The current compute crate has `Capability`, `Mode`, `Scope`, `Grant`, and `GrantSet` in
`crates/loom-compute/src/capability.rs` and canonical manifest encoding in
`crates/loom-compute/src/manifest.rs`; spec 3 extends that vocabulary with principal subjects,
workspace selectors, ref globs, effects, and the wider `Right` set above, so the compute and principal
models converge on one resource algebra rather than two parallel ones.

## 12. The three specs (promoted)

All three are now numbered specs; 0029 (events) and 0030 (observability) are also created. For the
record, in dependency order:

| Spec | Title | Status | Scope |
| ---- | ----- | ------ | ----- |
| 0026 | Principals & Identity | Draft, capability `identity` | what a principal is (user/service/root); the internal principal store; pluggable authentication (password + Ed25519 first, certificate/secp256k1 deferred, decision 13.1); the owner-vs-authenticated mode and the bootstrap/recovery authority (removable under the no-lockout invariant, reversible and audited, decision 13.2) |
| 0027 | Access Control (workspace-level) | Draft, capability `acl`, promotes 0009 section 6 | the PEP; the stored grant model with allow-and-deny-precedence (decision 13.3); read/write/advance grants at the workspace level; roles; target CEL predicates; cross-workspace reads require read on each workspace (decision 13.5); `PERMISSION_DENIED`; the honest trust boundary and the encryption non-goal |
| 0028 | Fine-grained Access Control | Draft, capability `acl-fine` | grants below the workspace (ref-glob, path/scope prefix, facet, `merge`/`exec` rights); matching and deny-precedence; tokens (biscuit) only if a stateless-token API is wanted (deferred, decision 13.4) |

0026 lands before 0027; 0028 builds on 0027. All three are capabilities (0010 section 4), advertised
and conformance-profiled, consistent with how 0009 capabilities are registered. Each reuses the
compute layer's grant grammar so there is one access model in the codebase, not two. The `e2e-sync`
zero-knowledge-replica capability (section 3.1) is separate from this series and not part of 0028.

## 13. Resolved decisions

The owner has decided 13.1 through 13.6; the choices are recorded here and folded into the numbered
specs. Specs 0026-0028 are now the normative sources.

1. **Authn method set for v1 - decided (a).** Ship password (Argon2id) plus Ed25519 public key first;
   defer the secp256k1/certificate path. Both shipped methods reuse primitives the project already
   commits to (0009 sections 3.1 and 5), and the deferred path is additive behind the same trait
   (section 5.3) with no model change.
2. **Reversibility of authenticated mode - decided (b), refined by 0026.** Reverting authenticated
   mode to owner mode is allowed, but only as an audited operation authorized by an enabled recovery or
   admin principal. The lockout-and-downgrade surface this opens is mitigated by requiring recovery
   authorization and logging the change (the audit log, 0009 section 7); the bootstrap safety rules of
   section 4 still apply in reverse.
3. **Deny semantics - decided (b).** Adopt allow-with-deny-precedence from the start, not allow-only.
   The reasoning is data safety: an access model should not be re-interpreted retroactively, so the
   evaluation rule (explicit deny wins) is fixed now even though the workspace-level spec may not yet
   issue deny grants. Folded into section 6.
4. **Tokens - decided: none in v1.** The v1 design uses the first-party stored grant evaluator. Biscuit
   tokens are not used; the token-coupling question is therefore moot for v1 and revisited only if a
   stateless-token API layer is added later, at which point option (a) (re-check the token's subset
   against the live stored model on use, for immediate revocation) is the starting recommendation.
5. **Authorization for cross-workspace reads - decided (a).** A cross-workspace read requires a read
   grant on every workspace it touches. This composes with the existing grammar and needs no new right;
   it also matches the deny-precedence model (a deny on any touched workspace blocks the read).

6. **System-workspace exposure surface - decided (a).** Management is through the `Auth` facade in v1.
   A CLI, web binding, server API, or SDK may surface principals and grants by calling the facade. A
   later implementation may add a read-only `users`/`grants` view for inspection. A writable virtual
   table is out of v1 because it would create two write paths into one audited internal store.

## 14. Sources

- Access-control sketch and capability tokens: `specs/0009-security-and-capabilities.md` section 6;
  signing and identity primitives: section 5; encryption envelope: section 3.1; key-derivation
  per-workspace question: open question 4.
- Capability grant grammar (facet + scope + mode): `specs/0015-execution-and-logic.md` section 6.1;
  `crates/loom-compute/src/capability.rs`; `crates/loom-compute/src/manifest.rs`.
- Workspaces and the system-workspace precedent: `specs/0014-workspaces.md` (registry, open question 7).
- Wire/transport projection of auth: `specs/0008-wire-protocols.md` section 5.
- argon2 (MIT OR Apache-2.0): https://github.com/RustCrypto/password-hashes. ed25519-dalek (BSD-3-Clause): https://crates.io/crates/ed25519-dalek. ssh-key (MIT OR Apache-2.0): https://github.com/RustCrypto/SSH.
- biscuit-auth (Apache-2.0): https://crates.io/crates/biscuit-auth ; https://github.com/biscuit-auth/biscuit. cedar-policy (Apache-2.0): https://github.com/cedar-policy/cedar. casbin (Apache-2.0): https://github.com/casbin/casbin-rs.
