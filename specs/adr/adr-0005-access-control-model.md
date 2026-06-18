# ADR-0005 - Principals and access-control model

**Status:** Accepted · **Date:** 2026-06-16 · **Deciders:** Nas
**Related:** 0026 (principals and identity), 0027 (access control), 0028 (fine-grained access
control), 0009 §6 (the originating sketch), 0015 §6 (the capability grant grammar this reuses).

## Context

Loom is embeddable and single-actor: whoever opens the file has full access. To run behind a shared
service (0008, IMPLEMENTATION-PLAN.md P9) it needs authorization. 0009 §6 sketched capability tokens
and path-scoped grants but stopped short of a model. The target the owner set is the database-server
model: connect, a root account creates users, root grants each user read or write over things, at the
namespace level at minimum. Several design forks fell out of refining that target, and they are
cross-cutting enough to record once here rather than only inside the three specs.

## Decisions

1. **Identity is keys-to-principals-to-roles.** A principal (user, service, or root) is the unit that
   authenticates and is authorized; a principal carries one or more credentials and zero or more
   roles; a role is a named grant bundle. Stable `PrincipalId` (UUIDv4) is the reference other specs
   use (a grant subject, a trigger `run_as`); the human `name` is a mutable label, never an identity
   reference. (0026.)

2. **Authentication is pluggable and separate from authorization.** Authn resolves a connection to a
   principal id and produces only an identity, never permissions. Password (Argon2id) and Ed25519
   public key ship first; certificate/secp256k1 is deferred behind the same trait. Both shipped
   methods reuse primitives the project already commits to (0009 §3.1, §5). (0026 §5.)

3. **Authorization lives in the engine, at one policy enforcement point.** Every facade call passes
   through `authorize(principal, operation, resource)` in `loom-core` before it touches state, so the
   rule holds identically for an embedded caller, the CLI, and the served API; the transport only
   carries the authenticated principal id. Authz is not built into the surrounding environment.
   (0027 §4.)

4. **Grants are stored state the engine consults, not bearer tokens the client carries.** The source
   of truth is a stored principal-and-grant model inside the Loom (the database model: users and
   grants live in the database). Tokens are at most a later, optional session/delegation layer, never
   the foundation. (0027 §2.)

5. **One grant grammar for programs and principals.** A principal grant reuses the compute layer's
   capability grammar (facet + scope + mode, 0015 §6.1, `prototypes/loom-compute/src/access.rs`) and
   adds the version-control axes (namespace, ref-glob) and the `Right` set (Read, Write, Advance,
   Merge, Admin, Exec). A program is constrained code; a principal is a constrained actor; same
   algebra, one model in the codebase. (0027 §2, 0028 §2.)

6. **Allow with deny-precedence, fixed from the start.** Effective permission is the union of allows
   with any explicit deny winning, plus default-deny. This is adopted before deny grants are common
   (0027 issues none at the namespace level) specifically so the evaluation rule never changes
   retroactively, which would be unsafe for an access model. A broad deny beats a narrow allow;
   specificity does not override a deny. (0027 §3, 0028 §3.)

7. **Owner mode by default; authenticated mode is an explicit, bootstrapped flip.** A new `.loom` is
   single-owner with no credentials (the embedded case). Setting a root credential and enabling
   authenticated mode is one journaled transaction, refused without a usable root credential.
   Reverting is root-only and audited. (0026 §4.)

8. **The trust boundary is named honestly.** Authorization protects authenticated access through the
   engine's checked surface, not the at-rest bytes, and not an operator holding the file and key, just
   as a database `GRANT` constrains clients of the server and not someone with shell access to the
   data directory. Confidentiality of the bytes is whole-file encryption (0009 §3); confidentiality
   from an untrusted host is `e2e-sync` (ADR-0007, 0031). Three doors, locked separately. (0027 §6.)

9. **Namespace-level first, fine-grained as a superset.** Namespace-level grants are the minimum
   viable shared Loom (0027); ref/path/facet scoping and `Merge`/`Exec` in anger are 0028 on the same
   grammar, so the finer model drops in without changing the evaluation rule.

## Alternatives considered and rejected

- **Bearer tokens (biscuit/macaroons) as the foundation** - rejected for v1: the database model is
  stored-grant-first; tokens are a delegation/transport convenience layered on later, not the source
  of truth (Decision #4). Biscuit remains the candidate if a stateless-token API is ever wanted (0028
  §5).
- **Authorization in the environment around Loom** (a gateway/proxy enforcing access) - rejected: it
  would not hold for embedded or CLI callers and splits the policy from the engine (Decision #3).
- **Allow-only semantics, adding deny later** - rejected: changing the evaluation rule after grants
  exist is unsafe; deny-precedence is cheap to fix now (Decision #6).
- **Coupling authorization to encryption** (the first-pass "stronger option") - rejected: authn,
  authz, and encryption are three independent concerns (Decision #2, #8).

## Consequences

- **Positive:** one access model shared by code and principals; a familiar database mental model; the
  safe evaluation rule fixed early; embedded use unaffected (owner mode is the default and costs
  nothing). The PEP is the single chokepoint to audit.
- **Negative / accepted:** the honest trust boundary (Decision #8) means authz alone does not defend
  against a host or operator that holds the bytes; deployments needing that compose encryption
  (0009 §3) or `e2e-sync` (0031). A principal linking `loom-core` directly can construct its own root
  context, so the guarantee is for deployments that do not hand out the engine or the file.
- **Follow-on:** the `Code` enum gains `AUTHENTICATION_FAILED` and `IDENTITY_NO_ROOT_CREDENTIAL`
  (0003 §8); the `identity`/`acl`/`acl-fine` capabilities are registered (0010 §4); a grant-evaluator
  prototype exists (`prototypes/authz/`). Build sequencing is P12 (IMPLEMENTATION-PLAN.md).

## Open

None blocking. Remaining choices (the system-namespace inspection surface, multiple-root vs an admin
role, token coupling if tokens land) are the open questions in 0026/0027/0028 and do not change this
model.
