# 0009a - Governance and Authenticity

**Status:** Draft target extension. **Version:** 0.1.0. **Normative target.**

This document owns security governance and authenticity work split out of 0009. It does not change the
current at-rest security boundary: 0009 is complete for source-backed whole-Loom encryption, integrity,
identity profiles, rekey, reseal, and current key-wrap behavior.

## Current source boundary

Current source provides:

- whole-Loom at-rest encryption;
- digest and AEAD verification on object reads;
- default and FIPS identity profiles;
- passphrase and raw-KEK unlock;
- cheap rekey and full reseal;
- multi-wrap add/remove and duplicate-wrap rejection for passphrase and raw-KEK credentials;
- stable error-code placeholders for authentication, authorization, and E2E locked/key failures;
- durable local audit records for successful identity, ACL, workspace, and management KV
  configuration changes;
- durable audit configuration with 365-day default retention and legal-hold flag;
- Admin-only CLI audit read/config/prune commands;
- sequence/hash-addressed audit records with checkpointed pruning for retained-suffix verification;
- daemon start, stop, session attach, session detach, pin add, and pin remove audit events;
- audited `loom serve` durable served-listener configuration writes;
- hosted Admin REST and JSON-RPC audit list, export, config read/write, and prune operations;
- hosted authentication-failure and authorization-denial audit events;
- daemon-opened CAS/admin listener open, close, reject, enable, disable, and configuration events;
- core durable protected-ref policy records keyed by exact `branch/name` or `tag/name`;
- core protected-ref evaluation after ACL authorization and before ref publication;
- protected-ref set/list/get/remove projection through CLI, IDL, C ABI, Node, Python, and C++;
- conformance coverage for fast-forward-only denial, fail-closed signature/review requirements, and
  retention/governance tag locks.

Current source does not provide:

- (P1) commit signatures;
- (P1) signed ref advances;
- (P1) trust-set management;
- (P2) transparency logs or external witnessing;
- (P1) complete audit feeds beyond the source-backed local control-plane, daemon, served-listener,
  hosted auth-denial, and hosted admin-audit subset;
- (P1) automatic retention compaction and policy-controlled garbage collection;
- (P1) legal-hold enforcement outside the audit prune command;
- (P1) redaction workflow;
- (P1) deletion proofs;

Principal identity and ACL work belongs to 0026-0028. Key-provider acquisition belongs to 0034. E2E
blind sync belongs to 0031.

## Target authenticity track

Commit and ref authenticity should be promoted only after principals exist:

- (P0) finish protected-ref policy management through Swift/iOS, JVM, Android, React Native, WASM,
  hosted protocols, and conformance-report inventory;
- (P0) enforce protected-ref policy in every served branch/tag publication path;
- (P0) define signed payload bytes for commits, tags, and ref advances;
- (P0) use a closed signature-suite registry;
- (P0) bind the signature-suite id into the signed payload;
- (P0) anchor trust in Loom principals, not X.509 or PGP roots;
- (P0) define a shared principal signing substrate for commit, ref, ledger checkpoint, authority,
  witness, and challenge payloads;
- (P1) define key rotation and revocation effects on historical signatures;
- (P1) add verification APIs, CLI, ABI, bindings, and conformance;
- (P2) add external witnessing only as optional deployment hardening.

The first shared principal signing substrate slice is source-backed in `loom-core`: public signing
keys bind to Loom principals, suite identifiers are closed, Ed25519 and ES256 verification are
supported, and generic purpose-bound payload bytes bind suite, purpose, principal, key id, and
payload. Verification rejects wrong-purpose signatures, wrong-principal keys, disabled or missing
keys, and suite/key mismatches. Fresh challenge verification, delegated agent and service-principal
signing policy, key rotation semantics, revocation effects on historical signatures, signing-key
audit, OpenPGP/PGP provider behavior, and protected key-provider storage remain target work.
OpenPGP/PGP may be one provider or suite, but Loom principals remain the trust anchor. Private keys
generated or stored by Loom belong in a protected key-provider or control-plane layer, not normal
workspace or facet content.

## Target transparency and audit track

Current source implements a first local audit spine in `loom-store`: successful local control-plane
mutations append hash-chained records under the durable-local control root. The v1 source-backed
record contains sequence, actor principal when one is resolved, action name, optional redacted target,
previous entry hash, and entry hash under the store identity profile. Identity and ACL management
through the C ABI and CLI call the audited save helpers; CLI workspace management and management KV
configuration append audit records after their state mutation succeeds. The local CLI exposes
Admin-only `audit list`, `audit view`, `audit config show`, `audit config set`, and `audit compact`.
The durable audit config defaults to 365 retention days and legal hold disabled. Legal hold blocks
manual audit compaction. Pruning removes old records only after storing a checkpoint for the removed
prefix, so the retained suffix still verifies. Daemon start, stop, session attach, session detach,
pin add, and pin remove append local audit records. `loom serve` configuration writes append local
audit records. Admin REST and JSON-RPC expose audit list, audit export, audit config read/write, and
audit prune through the hosted kernel under global Admin authorization, and audit those reads or
mutations.
Ordinary data-plane writes such as SQL row writes, KV puts, file writes, queue appends, and document
updates are not logged by default.

Transparency and audit should use promoted Loom substrates:

- (P1) promote the local audit record schema into the public ABI, binding, protocol, and conformance
  contract;
- (P1) add ABI, binding, non-admin hosted protocol, and conformance surfaces for audit
  read/config/prune/export beyond the source-backed CLI and Admin REST/JSON-RPC surfaces;
- (P1) require `Admin` for audit export in v1. A future dedicated `audit.read` or `audit.export` right
  may narrow that without granting full administration;
- (P0) prevent audit records from granting authority by themselves;
- (P1) automate retention compaction from the durable retention setting;
- (P1) define sync behavior for audit records;
- (P1) define privacy boundaries for audit payloads;
- (P1) define atomicity semantics for audit records that are appended after workspace-state commits;
- (P1) define hosted audit behavior for served writes, served listener open/close/remove operations,
  failed authentication, and denied authorization;
- (P2) define optional witness publication.

The v1 audit event classes are control-plane and security-plane events, not every data-plane write.
Required audited event classes are:

- identity and role management;
- ACL grant and revoke, including scoped grant management;
- workspace lifecycle management;
- management configuration writes, including KV tier configuration and served-listener configuration;
- daemon lifecycle actions such as start, stop, restart, pin, unpin, session attach, session detach,
  and doctor actions that change durable state;
- served protocol listener open, close, enable, disable, and bind or TLS configuration changes;
- failed authentication where authentication is required;
- denied authorization for privileged or management operations;
- audit read, export, prune, and configuration changes.

High-volume ordinary data operations are not logged by default because logging every SQL row write,
KV put, file write, queue append, or document update would flood the audit log and distort workload
cost. A future per-facet data-write audit policy may record summaries for regulated deployments, but it
must be explicit, bounded, and scoped.

## Target lifecycle governance track

Retention, legal hold, redaction, and deletion proofs must respect content-addressed history:

- (P0) define delete refusal behavior under retention or legal hold;
- (P0) define redaction rewrite semantics and stable error mapping;
- (P0) define why already-copied peers cannot be forced to forget data;
- (P0) define ordinary-sync behavior so redacted objects are not silently resurrected after an
  administrative rewrite;
- (P1) define deletion proof records;
- (P1) define policy-controlled GC;
- (P1) add conformance for retention, redaction, deletion proof, sync interaction, and policy errors.

## Sequencing

1. (P0) Implement principals and ACLs first through 0026-0028.
2. (P0) Expose protected-ref policy management and served-write enforcement before any hosted write
   surface claims protected-ref support.
3. (P1) Promote ledger-style audit storage through 0018 before audit and transparency claims.
4. (P1) Define signature payload bytes and trust-set records.
5. (P1) Define retention, legal hold, redaction, deletion proof, and GC policy records.
6. (P1) Add API, CLI, ABI, binding, protocol, and conformance coverage for each promoted surface.

## Resolved decisions

1. **At-rest encryption is not authenticity.** Encryption protects stored bytes; signatures and trusted
   ref advances are separate target work.
2. **Trust anchors are Loom principals.** Certificate chains and PGP webs are not v1 identity roots.
3. **Redaction is bounded.** Loom can prevent ordinary sync from resurrecting administratively rewritten
   data, but it cannot force already-copied peers to forget bytes.
4. **Protected refs compose with ACL.** A grant can make a ref operation eligible, but a protected-ref
   policy can still refuse publication. Policy never grants authority by itself.
5. **Principal signing is shared infrastructure.** Ledger checkpoints, authority handoffs, ref
   signatures, and future witness payloads use the same principal-rooted signing model instead of
   each facet inventing a private identity root.
