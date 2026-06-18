# 0028 - Fine-grained Access Control (grants below the workspace)

**Status:** Partial, local fine-grained PEP hooks source-backed. **Version:** 0.1.0-target.
**Optional capability:** `acl-fine`; extends 0027.

**Depends on:** 0027 (the grant model, the evaluation rule, and the policy enforcement point), 0026
(principals and roles), 0015 (the capability grant grammar 0015 §6.1 whose scope/facet axes this
narrows), 0014 (workspaces), 0009a (future protected-ref authenticity and audit records).
**Relates to:** 0029 (a trigger's `run_as` authority is evaluated at this granularity), 0030 (a `watch`
subscription is narrowed to the authorized fine-grained subset). **Promoted from:**
`PRINCIPALS-AND-ACCESS-CONTROL-LANDSCAPE.md` §12, which this document supersedes as the normative
source for sub-workspace authorization.

This document extends workspace-level access control (0027) below the workspace: grants scoped to a
branch or tag, to a path or key prefix, to a facet, and grants of the `Merge` and `Exec` rights. It
changes none of 0027's machinery; it uses the axes the 0027 grant already carries and the
deny-precedence rule 0027 already fixed. It is gated by the `acl-fine` capability; absence means
authorization is workspace-level only (0027).

## Current implementation

The current Rust workspace implements the core `acl-fine` grant encoding, matcher, and local engine PEP
rollout for promoted data operations. `loom_core::acl` stores `ref_glob` and a length-prefixed list of
scopes on each grant, encodes them in the durable ACL codec, preserves older broad-scope ACL stores on
decode, enforces the 0027 bounds of 64 scopes per grant, 1 KiB per prefix, and 256 grants per subject,
and matches typed prefix scopes for `ref`, `collection`, `path`, `key`, `table`, and `exec` resources
with deny precedence. Concrete local engine hooks now pass path scopes for file operations, key scopes
for configured KV data operations, table scopes for SQL table/history/index operations, ref globs for
branch and tag operations, and collection scopes for CAS, queue, document, ledger, time-series, graph,
vector, columnar, search, calendar, contacts, and mail operations. Internal facet storage uses reserved
file access, so a caller with a non-files collection grant is not forced to also hold a `files` path
grant. CLI ACL management can now create and remove scoped grants with `--ref-glob` and repeated
`--scope KIND:PREFIX` flags, and list output includes stored `ref_glob` and `scopes` for inspection.
IDL, C ABI, Node, Python, C++, Swift/iOS, JVM, Android, React Native, and WASM management surfaces now
expose the same structured ref glob and typed prefix scope inputs. `crates/loom-hosted` source-backs
the first hosted fine-grained enforcement kernel through shared request sessions, engine PEP calls,
and REST, JSON-RPC, and gRPC-shaped adapter tests; served SQL, calendar, contacts, and mail auth gates
also share that path. Admin REST and admin JSON-RPC can now manage scoped ACL grants through the same
structured input model. `crates/loom-compute` now source-backs Rust `Exec` evaluation for multi-facet
`StateAccess`: the principal must hold `Execute` over the matching facet and typed `exec` scope, and the
manifest must independently permit the same operation. IDL, C ABI, Node, Python, C++, Swift/iOS, JVM,
Android, React Native, and WASM
projection now expose execution through canonical `loom.exec.request.v1` and `loom.exec.result.v1`
CBOR. Hosted REST, JSON-RPC, and gRPC `exec` adapters and served listeners call the same hosted auth/write
path and execution boundary. `loom-delivery` enforces queue collection scope before durable
delivery enqueue, replay, and ack. There is no row or column predicate policy, attenuable token session,
complete hosted listener coverage, full trigger keeper/facade authorization,
non-admin hosted wire-management projection, or conformance/reporting evidence today. Row or column
predicates, conditional CEL policy, and attenuable token sessions are split into 0028a.

## Remaining target work

- (P0) Scoped grant management projection. The engine can encode and evaluate scoped grants, and the
  CLI, IDL, C ABI, Node, Python, C++, Swift/iOS, JVM, Android, React Native, WASM, admin REST, and
  admin JSON-RPC expose typed scope inputs. Non-admin hosted wire-management routes and
  conformance/reporting evidence remain target where a projection claims ACL management.
- (P0) Hosted fine-grained enforcement. Hosted protocols must carry enough resource identity for the
  engine PEP to evaluate workspace, facet, ref, and scope consistently with local operations. The
  shared hosted kernel and admin REST grant management are source-backed; complete listener coverage
  and non-admin hosted management routes remain target.
- (P0) Fine-grained ACL conformance. 0010a and 0025 own reporting and runners, but this spec owns the
  expected cases: ref-glob matching, multiple prefix matching, broad-deny-over-narrow-allow, facet
  gating, collection scoping, and binding/protocol parity.
- (P1) Cross-projection `Exec`, watch, and trigger integration. Rust multi-facet `Exec` checks plus IDL,
  C ABI, Node, Python, C++, Swift/iOS, JVM, Android, React Native, and WASM `exec` projection are
  source-backed in 0015, along with hosted REST/JSON-RPC/gRPC adapters and served listeners. Non-file
  watch narrowing and full trigger keeper/facade authorization need stable resource descriptors from
  0029 and 0030.
- (P1) Protected-ref composition. Core protected-ref policy records and local branch/tag enforcement
  are source-backed. Served protocols must still compose `Merge` with those 0027 protected-ref records
  and 0009a governance records before claiming protected-ref support.

## 1. Goals & non-goals

**Goals.** (G1) Narrow any axis of a grant below the whole workspace: by ref glob, by path/key scope
prefix, by facet. (G2) Make `Merge` (promote into a protected ref) and `Exec` (invoke the compute
facade) usable rights, not just reserved ones. (G3) Define matching precisely and keep 0027's
deny-precedence so an exclusion ("everything except `secrets/`") is expressible and safe. (G4) Keep one
grant type shared with the compute layer (0015 §6.1), so principals and programs scope the same way.

**Non-goals.** (N1) Not a new evaluation rule; allow-with-deny-precedence and default-deny are 0027 §3,
unchanged. (N2) Not confidentiality; hiding content from an untrusted host is the separate `e2e-sync`
capability (`PRINCIPALS-AND-ACCESS-CONTROL-LANDSCAPE.md` §3.1), not an access-control concern. (N3) Not
row-level or column-level predicates, conditional CEL policy, or attenuable sessions; those are 0028a.
(N4) Not identity (0026) and not workspace-level grants (0027).

## 2. Fine-grained axes

A grant is the 0027 `Grant` type; this document gives meaning to its axes below the workspace. At the
workspace level (0027) `ref_glob`, `scope`, and `facet` are wildcards; here each may narrow.

| Axis | Workspace-level (0027) | Fine-grained (this document) |
| ---- | ---------------------- | ---------------------------- |
| `ref_glob` | `*` (all branches and tags) | a glob over ref names, e.g. `branch/dev`, `branch/feature-*`, `tag/release-*` |
| `scopes` | `All` | one `Prefix(bytes)` **or a list of them** (0015 §6.1 / 0027 §2), each facet-specific: a path prefix for files, a key prefix for kv, a table-name prefix for relational, a series prefix for time-series, and so on; the grant applies if the resource matches any listed prefix |
| `facet` | `AllFacets` | a single facet (`files`, `sql`, `kv`, `graph`, `vector`, ...) |
| `rights` | `Read`/`Write`/`Advance` | adds `Merge` (§4) and `Exec` (§4) in common use |

Scope semantics are prefix-based and facet-defined - one prefix or a list of prefixes (0027 §2) -
aligned with the compute layer's manifest-grant scoping (0015 §6.1), so a grant to a principal and a
manifest grant to a program confine resources the same way.
Ref globs and path globs use glob matching (`globset`, MIT OR Apache-2.0), as 0027 §8.

## 3. Matching and precedence

A grant is *applicable* to a request `(principal, right, resource)` when all hold: its `subject` is the
principal or one of its roles (0026); its `workspace` covers the resource's workspace; its `ref_glob`
matches the resource's ref; **any prefix in** its `scopes` is a prefix of (or `All` over) the resource's
path/key; its `facet` matches (or is `AllFacets`); and its `rights` include the requested right.

Effective permission is then exactly 0027 §3, unchanged:

1. Any applicable `Deny` denies the request.
2. Otherwise any applicable `Allow` covering the right allows it.
3. Otherwise default-deny.

The rule that matters at fine grain, and the reason deny-precedence was fixed in 0027 ahead of need: a
broad `Deny` beats a narrow `Allow`. **Specificity does not override a deny.** This is deliberate and
is the data-safe choice: an exclusion cannot be defeated by adding a more specific allow underneath it.
A deployment that wants "allow the narrow thing despite the broad deny" must express it as the absence
of the broad deny, not as a more specific allow.

### 3.1 Worked examples

| Intent | Grants |
| ------ | ------ |
| Read-write the workspace except the `secrets/` path | `Allow(ns, scopes=All, RW)` plus `Deny(ns, scopes=Prefix("secrets/"), RW)` |
| Read-only on `main`, read-write on `dev` | `Allow(ns, ref="branch/dev", RW)` plus `Allow(ns, ref="branch/main", R)` |
| May query the vector facet but not read files | `Allow(ns, facet=vector, R)` (no files grant; default-deny covers files) |
| May run programs against `reports/` but not write directly | `Allow(ns, scopes=Prefix("reports/"), Exec)` (no `Write` grant) |
| May advance `dev` but only merge to `main` through review | `Allow(ns, ref="branch/dev", Advance)` plus `Allow(ns, ref="branch/main", Merge)`, with `main` a protected ref (§4) |

## 4. The `Merge` and `Exec` rights at fine grain

- **`Merge`** authorizes promoting a candidate change into a ref. It composes with the protected-ref
  policy (0027 §5): a `Merge` grant is necessary but not sufficient when the target ref also requires
  fast-forward-only, signed commits, or a minimum number of reviews; all must be satisfied.
  Separating `Advance` (move a branch forward) from `Merge` (promote into a protected ref) lets a
  principal push freely to a working branch while merges to a protected ref stay gated.
  Source currently provides the `Merge` right, `ref_glob` matching, branch/tag ref scopes, tag-admin
  gates, non-fast-forward admin gates, durable protected-ref policy records, and core local
  protected-ref evaluation. CLI, IDL, C ABI, Node, Python, and C++ can manage protected-ref policies.
  A served protocol must not treat `Merge` as sufficient for a protected ref until it loads and
  enforces the same 0027 §5 policy records.
- **`Exec`** authorizes invoking the compute facade (0015) against a workspace or a scoped subtree. It
  gates *whether a principal may run a program there*; what the program may touch once running is still
  bounded by the program's manifest grants (0015 §6), so a fire is confined by the intersection of the
  principal's `Exec`-scoped authority and the program's manifest (the same two-layer rule a trigger
  obeys, 0029 §8). An `Exec` grant scoped to a path prefix lets a principal run programs only against
  that subtree.

## 5. Deferred policy extensions

Row-level predicates, column-level predicates, conditional CEL policy, and attenuable token sessions
are not part of this dependency gate. They are tracked by 0028a so 0028 can stay focused on
source-backed grant axes: ref glob, prefix, facet, `Merge`, and `Exec`.

## 6. Interaction with existing specs

- **0027** - reuses the grant type, the PEP, deny-precedence, and `PERMISSION_DENIED`; this document
  only gives the sub-workspace axes meaning and activates `Merge`/`Exec`.
- **0015** - scope/facet semantics align with manifest-grant scoping (0015 §6.1); `Exec` gates
  invocation while the program manifest still bounds the program; the target CEL guard layer evaluates
  any conditional grant once promoted.
- **0029** - a trigger's `run_as` authority is evaluated at this granularity, and `Exec` plus a scoped
  grant is how a binding is confined to a subtree.
- **0030** - a `watch` subscription is narrowed to the fine-grained authorized subset (0030 §9).
- **0009a** - protected-ref authenticity, audit, and governance records compose with `Merge` once
  those target surfaces are promoted.
- **e2e-sync** - confidentiality from an untrusted host is a separate capability
  (`PRINCIPALS-AND-ACCESS-CONTROL-LANDSCAPE.md` §3.1), not part of this document.

## 7. Security

Fine-grained scoping is the point of embedding Loom with untrusted or multi-tenant callers: path,
ref, and facet confinement plus deny-precedence let a deployment express least privilege and hard
exclusions. The safety properties are inherited from 0027 (single PEP, default-deny, audited grants)
and sharpened here by the rule that a broad deny always wins (§3), so an exclusion cannot be eroded by
a more specific allow. Conditional grants and feed narrowing fail closed. This document does not change
the trust boundary of 0027 §6: authorization protects the live interface, not the at-rest bytes or an
untrusted host (which are encryption's and `e2e-sync`'s concerns).

## 8. Resolved decisions

1. **Predicate scoping beyond prefixes.** V1 fine-grained access control stops at prefix scopes. Row,
   column, conditional CEL, and record predicates are target work in 0028a. A dedicated row-policy
   facet is deferred as heavier than the current contract needs.
2. **Most-specific-wins as an option.** There is one evaluation rule: deny always wins. A per-Loom
   switch between deny-precedence and most-specific-wins is out of v1 because it makes the same grant
   set mean different things in different stores.
3. **Token lifetime and attenuation depth.** Tokens are out of v1 and tracked by 0028a. If a
   stateless-token API is later specified, token lifetime and attenuation depth are decided there, and
   every token use must still be checked against live stored grants so revocation is immediate.

## 9. Sources

- Workspace-level model this extends: `specs/0027-access-control.md` §2, §3, §5.
- Capability scope/facet grammar: `specs/0015-execution-and-logic.md` §6.1;
  `crates/loom-compute/src/capability.rs` and `crates/loom-compute/src/manifest.rs`.
- Governance and authenticity: `specs/0009a-governance-and-authenticity.md`.
- Predicate and token extensions: `specs/0028a-acl-policy-extensions.md`.
- Design note and decisions (13.3, 13.4): `PRINCIPALS-AND-ACCESS-CONTROL-LANDSCAPE.md` §6, §12, §13.
