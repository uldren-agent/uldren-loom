# 0027 - Access Control (workspace-level authorization)

**Status:** Partial, core evaluator source-backed. **Version:** 0.1.0-target.
**Optional capability:** `acl`; promotes the access-control target model from 0009.

**Depends on:** 0026 (principals and identity; the subjects of grants), 0015 (the capability grant
grammar, 0015 §6.1, and current `crates/loom-compute/src/capability.rs` plus `manifest.rs`),
0014 (workspaces; the coarsest resource axis), 0009 (security boundary and stable errors), 0009a
(future audit and authenticity surfaces), 0003 (interface, the `Code` enum), 0008 (transport projection). **Relates to:** 0029
(a trigger's authority is resolved here at fire time), 0028 (fine-grained access control extends this
below the workspace), 0027a (operation-by-operation authorization matrix). **Promoted from:**
`PRINCIPALS-AND-ACCESS-CONTROL-LANDSCAPE.md`, which this
document supersedes as the normative source for workspace-level authorization.

This document specifies authorization for a shared Loom at the workspace level: a stored grant model
that the engine consults on every operation, evaluated at one policy enforcement point. It is the
minimum that makes a shared Loom usable (an admin or recovery principal grants accounts read or write
over workspaces), and it is built so the finer scoping of 0028 drops in without changing the evaluation
rule. It is gated by the `acl` capability; absence means unauthenticated root-mode single-user access
(0026 §4).

## Current implementation

Current source implements the core ACL evaluator in `loom_core::acl` and persists direct grants through
the `FileStore` durable-local control root. `AclStore` supports principal and everyone subjects,
workspace and authorization-domain wildcards, `Read`, `Write`, `Advance`, `Merge`, `Execute`, and `Admin` rights,
explicit allow and deny grants, unauthenticated root-mode bypass, authenticated default-deny,
deny-precedence, and `Admin` as an all-rights grant. Denial returns `PERMISSION_DENIED`.
`FileStore::save_acl_store` and `acl_store` persist and restore direct grants outside workspace
history. `Loom` carries an optional identity store, a session id, and an ACL store. Promoted local
engine operations call the evaluator before mutating or reading: file path operations, configured KV
map/key operations, CAS digest/list operations, queue stream and consumer operations, SQL query/exec,
direct SQL table/history/index readers and mutators, document, ledger, time-series, graph, vector,
columnar, search, calendar, contacts, mail, workspace-history branch/tag/merge operations, sync clone,
push, bundle import/export, and cross-facet diff. Internal facet storage uses reserved file access and
does not require callers to hold separate `files` rights for a non-files facet. The C ABI `LoomSession`
bridge reloads persisted identity and ACL state on each per-call open, rebinds the authenticated
session into the identity store, and then calls the same engine authorization path. `loom-ffi` exposes
direct grant management through
`loom_acl_list_json`, `loom_acl_grant`, and `loom_acl_revoke`; the CLI exposes `acl list/grant/revoke`
and gates workspace lifecycle management with global `Admin` once authenticated mode is active. C++
`Loom` and Swift `Loom.open` project session and direct ACL management calls. The `acl` behavior runner
proves default-deny, deny-precedence, role-subject evaluation, and selected engine PEP hooks; FFI and
CLI tests prove a persisted ACL denies without authentication and permits after passphrase
authentication. Source supports direct principal grants, everyone grants, role grants encoded as
`role:<uuid>` subjects, role expansion in the single engine PEP, deterministic `ref_glob` and
length-prefixed scope-list storage, bounded scope/grant validation, and typed prefix matching for
`ref`, `collection`, `path`, `key`, `table`, and `exec` scopes. Source-backed fine-grained local hooks
now cover file paths, configured KV key prefixes, SQL table prefixes, VCS branch/tag ref names, and
collection-owned facet resources. The local MCP host reuses the same model for daemon-backed serving:
launch-time passphrase auth binds a principal, missing or invalid launch auth fails before the PEP, and
valid principals are authorized by the engine for every tool/resource operation. MCP `tools/list`
derives regular read/write tool visibility from the current bound principal and live ACL state,
advertises tool-list change notifications, and rejects hidden stale tool calls before router dispatch;
argument-scoped authorization remains in the engine PEP. CLI, C ABI, Node, Python, and C++ ACL
grant/revoke can create and remove scoped grants with a ref glob and repeated typed prefix scopes;
remaining binding and hosted grant management projections still create broad `All`-scope grants or do
not expose scoped inputs yet. Successful local ACL grant/revoke
mutations through the C ABI and CLI append durable-local audit records with the acting principal and
redacted grant target. Core source now implements durable protected-ref policy records, exact-ref
lookup, evaluation after ACL authorization and before ref publication, `PERMISSION_DENIED` mapping for
unmet protected-ref policy, and executable conformance for fast-forward-only, fail-closed
signature/review, and retention/governance lock behavior. `crates/loom-hosted` now source-backs the
first hosted PEP kernel: hosted requests attach the 0026 principal to a request session, read and write
opens call the engine PEP, authorization failure maps to `PERMISSION_DENIED`, and REST, JSON-RPC, and
gRPC-shaped hosted tree-adapter tests prove shared auth failure, permission denial, stable error
mapping, and daemon-authorized writes. The protocol-conformance runner additionally proves CAS
authentication failure and default-deny at REST, JSON-RPC, and gRPC boundaries, followed by a persisted
CAS read/write grant, authorized access, and immediate revoke denial. JSON-RPC carries its canonical
error object under HTTP 200; REST and gRPC use their respective canonical failure statuses. Served SQL, calendar, contacts, and mail auth gates also use
the hosted kernel in the executable hosted protocol matrix. Source also backs `admin/rest` and
`admin/json_rpc` operations for listener listing, listener enable/disable/remove, identity
list/add/set-passphrase/remove/role assign/role revoke, ACL grant/revoke/list, protected-ref
set/list/get/remove, and audit list/export/config/prune under global Admin authorization, with direct
hosted-admin adapter and daemon-listener tests for the identity route families. Scoped grant and
protected-ref management projection is source-backed for CLI, IDL, C ABI, Node, Python, C++,
Swift/iOS, JVM, Android, React Native, and WASM. The hosted result-handle authorization substrate is
source-backed for future served routes: handles are principal-bound, session-bound, scope-bound, and
expiring, and every operation reauthenticates and reruns the PEP before observing data. Watch
authorization is source-backed for the portable pull baseline: ref read is required, explicit
unauthorized files path-prefix selectors fail closed, and broad file watches omit unauthorized file path
changes. `crates/loom-compute` now source-backs the Rust `exec` PEP and multi-facet `StateAccess` hooks:
execution requires the principal's `Execute` grant and the program manifest grant to both cover the
requested facet operation, and denied guest operations fail closed with `PERMISSION_DENIED`. IDL, C
ABI, Node, Python, C++, Swift/iOS, JVM, Android, React Native, and WASM projection are source-backed through canonical
`loom.exec.request.v1` and `loom.exec.result.v1` CBOR. Hosted REST, JSON-RPC, and gRPC adapters plus
served listeners call the same hosted auth/write path and execution boundary. `loom-delivery`
now enforces queue collection authorization before durable delivery enqueue, replay, and ack. Source does not yet implement complete hosted
listener surface coverage, served protected-ref
enforcement across all ref-publication write paths, hosted MCP auth/session routes, row/column predicate
policy, full trigger keeper/facade authorization, automatic hosted audit retention, or
full binding/protocol ACL conformance. The full operation matrix for
completing those hooks is tracked by 0027a.

`AclDomain` is source-backed as the ACL resource axis in core evaluation, durable encoding, canonical
wire encoding, IDL and generated remote surfaces, CLI, C ABI, hosted administration, MCP discovery,
and principal binding signatures. Native facet domains preserve the stable `FacetKind` tags;
`tickets`, `pages`, `chat`, `lifecycle`, and `meetings` use distinct tags. Tickets and Lanes share the
`tickets` domain, Drive uses `files`, and Pages, Chat, Lifecycle, and Meetings use their named domains.
Executable MCP coverage proves that a Tickets grant reveals Tickets and Lanes without revealing
Files, Pages, or Chat. MCP Workgraph fact writes and board-read observation now authorize the
referenced task through a `tickets` key-prefix resource before appending a Workgraph operation
record. Shared operation-change records now preserve `target_entity_id` from the operation envelope
for Tickets, Chat, Pages, Lifecycle, and Workgraph logs, and the MCP substrate-change summary exposes
that target. Workgraph cursor reads filter returned events by the caller's task-key read grant and
advance the cursor across scanned records. Workgraph expansion still must authorize every referenced
non-ticket target in addition to the task topology. Substrate inventory and dispatch still need
operation-specific domain metadata for authorization decisions, binding runtime matrices still need
to execute the renamed inputs, and existing pre-release stores still require one controlled migration
to the final codec.
The final ACL snapshot uses the non-versioned `LACL` discriminator; it does not reuse any historical
`LAC1` through `LAC4` discriminator with a different field layout. The durable-local control key is
the final `acl` key rather than a versioned key.

## Remaining target work

- (P0) Finish authorization-domain promotion. The core type, durable and wire codecs, IDL, generated
  remote surfaces, CLI, C ABI, hosted administration, principal binding signatures, and direct
  Tickets/Lanes, Pages, Chat, Lifecycle, Meetings, Drive, selected MCP Workgraph task-key checks,
  operation-change target metadata, and Workgraph cursor filtering are source-backed. Complete
  Workgraph expansion for referenced non-ticket targets, operation-specific Substrate discovery and
  dispatch, binding runtime conformance, and a controlled pre-release migration of every affected
  store. Remove the temporary migration path after all stores use the single final shape; do not
  retain a legacy decoder or a permanent migration command.

- (P0) Remaining hosted protocol PEP projection. 0008 owns the wire shapes, but this spec owns the
  authorization rule: every hosted REST, JSON-RPC, gRPC, served SQL, PIM, and hosted MCP read/write
  must attach a 0026 principal and call the same engine PEP before observing or mutating Loom state.
  The shared hosted kernel and matrix are source-backed; `admin/rest` and `admin/json_rpc` listener,
  identity, ACL, protected-ref, and audit management routes are source-backed; full runtime listener
  coverage and all promoted facet-specific served write paths remain target.
- (P0) Served result-handle route projection. The shared hosted result-handle authorization substrate
  is source-backed. Future `poll`, `next`, `read`, `result`, `cancel`, or `close` routes must use it
  rather than creating bearer handles.
- (P0) Protected-ref remaining public management and served enforcement. Core source validates public
  ref names, gates local branch/tag operations, persists protected-ref records, enforces
  fast-forward-only, fails closed for signature/review requirements, enforces retention/governance tag
  locks, maps policy failure to `PERMISSION_DENIED`, and has executable conformance. CLI, IDL, C ABI,
  Node, Python, C++, Swift/iOS, JVM, Android, React Native, WASM, admin REST, and admin JSON-RPC
  protected-ref set/list/get/remove are source-backed. Served ref-publication enforcement and
  conformance-report inventory remain target work with 0009a.
- (P0) Protocol ACL conformance. 0010a and 0025 own the runners, but 0027 must supply the expected
  behavior for default-deny, deny-precedence, revocation, omitted reads, served write rejection,
  protected-ref composition, and cross-protocol consistency.
- (P0) Complete scoped grant management projection. The CLI, IDL, C ABI, Node, Python, C++,
  Swift/iOS, JVM, Android, React Native, WASM, admin REST, and admin JSON-RPC now expose ref glob plus
  typed prefix scopes. Non-admin hosted wire-management routes and conformance/reporting still need
  matching evidence where they claim ACL management.
- (P1) Public audit read/export and retention. Successful local management mutations, selected daemon
  lifecycle/session/pin events, served listener configuration and runtime CAS/admin open/close/reject
  events, hosted auth failures, hosted authorization denials, and `admin/rest` audit
  list/export/config/prune are audited. Redaction policy, automatic legal-hold-aware retention
  compaction, and complete event coverage across every promoted management surface remain target work.
- (P1) Cross-projection compute and trigger authorization. The Rust 0015 `exec` facade, `ExecContext`,
  multi-facet Rust `StateAccess` hooks, IDL projection, C ABI projection, Node, Python, C++, Swift/iOS, JVM,
  Android, React Native, WASM, and hosted REST/JSON-RPC/gRPC adapters and served listeners are source-backed.
  Trigger `run_as_context` is source-backed; full trigger keeper/facade authorization remains target until 0029 exposes stable public
  resource descriptors. Watch
  authorization is source-backed for the portable pull files baseline; non-file watch domains narrow as
  their owning event contracts promote.

## 1. Goals & non-goals

**Goals.** (G1) A stored, engine-evaluated grant model (the database model: grants are state the engine
consults, not bearer tokens the client carries). (G2) Grants of read, write, and ref-advance over a
workspace to a principal (0026), the `GRANT ... ON db.* TO user` equivalent. (G3) Roles as grant
bundles. (G4) One policy enforcement point in the engine, so the rules hold for every caller. (G5) An
evaluation rule (allow with deny-precedence) fixed now so it never has to change retroactively.

**Non-goals.** (N1) Not sub-workspace scoping; per-ref, per-path, per-facet narrowing and the
`merge`/`exec` rights are 0028 (the grammar here already carries the axes, but workspace-level grants
wildcard them). (N2) Not confidentiality from the host; authorization protects the live interface, not
the at-rest bytes or an operator holding the file and key (§6, and 0009 §3). (N3) Not token issuance;
stateless-token sessions are deferred (0028, decision 13.4). (N4) Not identity; who a principal is and
how it authenticates is 0026.

## 2. The grant model

Grants live alongside principals in the bucket-2 store-global control region (0001 §6.1, 0026 §5.2):
store-scoped engine state that syncs with the whole `.loom` file but is outside per-workspace version
control, so a `checkout`/`revert` can never roll back a revocation. It is single-authority and
fast-forward-replicated (0026 §5.2.1); the live grant set is a bounded snapshot and the change history
is the append-only audit log.

Authorization is a stored set of grants the engine evaluates per operation. The grant reuses the
compute layer's capability grammar (0015 §6.1: facet plus mode plus scopes) and adds the version-control
axes needed by workspace history and protected refs (a workspace selector and a ref glob), so that
programs (0015) and principals share one resource algebra rather than two parallel ones. The axes read
coarsest-to-finest:
**workspace -> authorization domain -> mode (rights) -> scopes**, with `scopes` last because it is the most specific and
may be a list.

```idl
struct Grant {
  effect:    Effect,                          // Allow | Deny   (§3)
  subject:   PrincipalId | RoleId,            // who the grant is for (0026)
  workspace: NsSelector | AllOfType(NsType) | All,   // which tree(s); the coarsest axis
  ref_glob:  string,                          // which branches/tags within it (default "*")
  domain:    AclDomain | AllDomains,           // native facets plus product authorization domains
  rights:    Set<Right>,                      // the mode axis (§2.1)
  scopes:    Scope | List<Scope>,             // one scope OR a list; applies if ANY listed scope matches (§2.2)
}

type Scope = All | Prefix(bytes)              // path/key prefix (0015 §6.1); All at workspace level
enum Effect { Allow, Deny }
enum Right { Read, Write, Advance, Merge, Admin, Exec }
```

A grant's `scopes` is therefore **one scope or a list of scopes**: a list lets a single grant cover
several prefixes under the same workspace/domain/mode (e.g. KV `Read` over `session:*` and
`user:profile:*`) without minting a grant per prefix. The grant matches if the resource matches **any**
scope in the list (union for `Allow`, and likewise for `Deny` - a `Deny` whose list contains a matching
prefix denies, §3).

The list is encoded as a **length-prefixed array** - a `count`, then each prefix as `length || bytes` -
consistent with the canonical object encoding (0002 §3.7). There is **no in-band delimiter**: a
`Prefix` is arbitrary bytes, so any separator character could occur inside a prefix, making a delimited
form both ambiguous and a forge-an-extra-scope injection vector. A textual surface such as a grant CLI
or config uses a repeated flag or a structured array, never a delimited string, for the same reason.

At the workspace level (this document) `ref_glob`, `scope`, and `domain` are wildcards: a grant reads
"read/write this whole workspace." 0028 narrows them. The current compute crate has `Capability`,
`Mode`, `Scope`, `Grant`, and `GrantSet` types for program manifests. This document extends that
vocabulary into the target principal grant type by widening `Mode` into the `Right` set and adding
`workspace`, `ref_glob`, `subject`, and `effect`, so the compute and principal models converge on one
resource algebra.

### 2.1 Authorization domains

`FacetKind` identifies a native data model, storage contract, and workspace behavior. `AclDomain`
identifies the policy boundary whose rights a caller must hold. Every native facet has a same-named
authorization domain, but a product service does not become a facet merely because it needs
independent policy.

| Public surface | Authorization domain | Reason |
| --- | --- | --- |
| Native facet operations | Same-named facet domain | Preserve the owning data model's policy boundary. |
| Tickets and Lanes | `tickets` | Workflow, comments, project policy, and coordination must not grant VCS history access. |
| Pages and Structures | `pages` | Knowledge content and structure must not inherit VCS administration. |
| Chat | `chat` | Channels, messages, membership, and moderation form an independent boundary. |
| Lifecycle | `lifecycle` | Definitions and execution controls must not borrow VCS rights. |
| Meetings | `meetings` | Meeting records, annotations, and extraction have independent visibility. |
| Drive | `files` | Drive is a files presentation and preserves path-scoped file authorization. |
| Workgraph | `tickets` plus target domains | Ticket topology requires ticket access; expanded content requires its owning domain. |
| Substrate | Operation target | Substrate is a cross-facet projection, not an authority domain. |

Unknown MCP areas must not fall through to global-admin visibility merely because they lack a mapping.
Each tool declares an authorization domain or an explicit global-management policy, and discovery and
invocation use the same declaration.

### 2.2 Rights

| Right | Permits |
| ----- | ------- |
| `Read` | read content and history of the workspace |
| `Write` | create commits / write content in the workspace |
| `Advance` | move a branch ref forward (push, fast-forward) |
| `Merge` | promote into a protected ref (a merge a protected-ref policy gates, §5) |
| `Admin` | manage principals, grants, and roles; reassign a trigger's `run_as` (0029 §8) |
| `Exec` | invoke the compute facade (`exec`, 0015) against the workspace |

`Read`/`Write`/`Advance` are the workspace-level minimum (G2). `Merge`, `Admin`, and `Exec` are defined
here so the version-control and compute operations are first-class permissions; `Merge` and the
sub-workspace use of `scopes`/`facet` are exercised by 0028.

### 2.3 Grant size is bounded

Because `scopes` may be a list (§2), a grant is variable-size; and the PEP (§4) evaluates the
applicable grant set on **every** operation, so an unbounded grant is both an interop and a
denial-of-service hazard. Grant size is therefore **bounded** (normative): at most **64 scopes** per
grant, each scope prefix at most **1 KiB**, and at most **256 grants** per principal or role - and the
same per program manifest (0015 §6.1). A grant or grant set that exceeds a bound is **rejected** at
`grant`/manifest-load time with `INVALID_ARGUMENT`, never silently truncated. The conformance suite
pins these limits so every implementation agrees on what a valid grant is.

## 3. Evaluation: allow with deny-precedence

Effective permission for `(principal, operation, resource)` is computed from the principal's direct
grants and the grants of its roles (0026 §6), as follows:

0. If the Loom is still in unauthenticated root mode (0026 §4), the operation is allowed as the initial
   root principal and ACL evaluation is bypassed.
1. If any applicable grant has `effect = Deny`, the request is **denied** (`PERMISSION_DENIED`).
2. Otherwise, if any applicable grant has `effect = Allow` covering the requested right and resource,
   the request is **allowed**.
3. Otherwise the request is **denied** (default-deny).

A grant is *applicable* when its `subject` is the principal or one of its roles, its `workspace`,
`ref_glob`, `scopes`, and `facet` cover the resource, and its `rights` include the requested right.

**Deny-precedence is adopted from the start** (decision 13.3), even though a workspace-level deployment
may issue only `Allow` grants. The reason is data safety: an access model must not be re-interpreted
retroactively, so the rule that an explicit `Deny` always wins is fixed now, before 0028 makes deny
grants common ("read-write this workspace except the `secrets/` prefix"). Default-deny (rule 3)
applies once authentication is enforced; a principal with no applicable grant can do nothing, matching
0026 §2.

## 4. The policy enforcement point

A single **policy enforcement point (PEP)** sits at the engine boundary. Every facade call
(`fs_write_file`, `vcs_merge`, `db.insert`, `exec.apply`, the `watch` subscription of 0030, ...) passes
through `authorize(principal, operation, resource)` before it touches state; a denial returns
`PERMISSION_DENIED` and no effect occurs. Because the check is in the engine:

- **Embedded / in-process:** a caller that opens an authenticated Loom and supplies a principal context
  is checked in-process; there is no external gatekeeper, the engine itself refuses. This is what makes
  authorization "in Loom," not in the surrounding environment.
- **Served (0008):** the transport authenticates the connection (0026 §5) and attaches the resolved
  principal id to each request, then calls the same engine. The transport carries identity; it does not
  define policy.

Conditional grants (a grant that should apply only under a predicate, for example a time-of-day or an
attribute test) are target work expressed as a **CEL** predicate evaluated read-only at the PEP. CEL is
the selected target L1 guard language in 0015, so conditional authorization should reuse that language
when the guard layer is promoted; a failing or unsatisfiable predicate fails closed.

### 4.1 History and diff authorization

History operations (`vcs_log`, `vcs_show`, `vcs_diff`, and related status surfaces)
are authorized as `vcs_*` operations over workspace history. A caller without the required VCS/history
permission cannot run the command and receives `PERMISSION_DENIED` before any commit message, parent id,
changed-unit id, or aggregate is revealed. A caller with that permission can see the commit metadata
for the operation, including messages.

After the VCS/history gate passes, unit payload details are still filtered by the owning facet's read
policy. A unit the viewer can read is returned fully qualified, with its field-level sub-diff (0003d).
A unit the viewer cannot read is omitted entirely, or, where a coarser grant justifies showing that
something changed, rolled up to an opaque count at a level the viewer is allowed to see - never exposing
the principal segment, collection name, or field value.

The stored commit and stored structural diff use fully qualified unit ids. The presented result is
derived per viewer by intersecting those ids with the VCS/history permission and the viewer's owning
facet grants. Roll-up labels are a PEP presentation choice, not a coarsening of committed state. The
operation-by-operation matrix is tracked by 0027a.

### 4.2 Commit is a Write; commit visibility is independent of authorship

Producing a commit is a write and is gated by `Write` on the units it touches (a commit spanning several
principals' partitions needs `Write` on each touched partition, or `Admin` on the enclosing scope). The
commit object records the committing principal as provenance and the full unit set. Who may later *read*
that commit's diff/log is governed entirely by §4.1 (the viewer's `Read` grants), independently of who
authored it: authorship does not grant any reader extra visibility, and a reader authorized for a unit
sees it regardless of which principal committed it.

### 4.3 Served result handles

Served result handles are target work for hosted protocols, not the local C ABI helper handles. A
served handle is an authorization continuation, so it must carry enough policy context to re-check
access before every later operation on the handle: principal, session family or auth epoch, issuing
operation, workspace, facet, concrete resource scopes, creation time, expiry, and consumption state.
The handle is not transferable across principals or across unrelated sessions.

Every served handle operation (`poll`, `next`, `read`, `result`, `cancel`, `close`, or an equivalent
protocol verb) first authenticates the caller and then runs the PEP against the current grant set. If
authentication is absent or stale, it fails with `AUTHENTICATION_FAILED`. If the caller is still
authenticated but no longer has the required grant, it fails with `PERMISSION_DENIED`. A closed,
expired, consumed, unknown, or wrong-daemon handle is reported as absent and must not reveal the
original operation or resource.

For results that can span several resources, the server may continue by narrowing future chunks to
items the caller can still read. If the server cannot prove the next chunk is within current grants, it
fails closed. This makes revocation effective for future result delivery without requiring a completed
result buffer to be retracted from a caller who already received it.

## 5. Protected refs

A protected-ref policy is attached to a ref (not to a principal) and composes with grants at the PEP: a
ref MAY require fast-forward-only, signed commits (0009a), a minimum number of reviews, or specific
signer keys. A `merge`/`advance` that a grant would otherwise allow is still refused if the
ref's protection policy is unmet. Protected-ref policy is host or versioned configuration; this
document fixes only that it composes with, and is evaluated after, the grant check.

The source-backed boundary today covers core local evaluation: public branch and tag names reject raw
or reserved ref spellings, branch/tag operations authorize through `ref_glob`, tag mutation requires
`Admin`, non-fast-forward local rewrites require `Admin`, durable protected-ref policy records persist
in engine state, exact ref lookup is evaluated after ACL authorization, and policy failure maps to
`PERMISSION_DENIED`. CLI, IDL, C ABI, Node, Python, and C++ can set, list, read, and remove those
policy records. Signature and review requirements fail closed until signed payload and review records
are promoted by 0009a. Therefore no hosted write surface may claim protected-ref support until hosted
publication paths load and enforce the same policy records.

The target evaluation order for a protected ref is:

1. Authenticate the caller and bind the principal to the request.
2. Authorize the operation through the normal PEP, including workspace, facet, `ref_glob`, right, and
   scope checks.
3. Load the protected-ref policy for the exact branch or tag.
4. Evaluate fast-forward-only, signature, review, retention, and governance requirements.
5. Publish the ref only after all reachable objects are verified and every policy condition passes.

Failure at step 1 returns `AUTHENTICATION_FAILED`; failure at step 2 returns `PERMISSION_DENIED`;
failure at a protected-ref policy step returns the stable policy-specific code once 0009a promotes it,
or `PERMISSION_DENIED` until a narrower code exists. The ref remains unchanged on every failure.

## 6. Trust boundary (stated honestly)

Authorization protects the **live interface**, not the **at-rest bytes**. A principal restricted to
read-only cannot write through Loom, but anyone holding the file and its encryption key can read
everything, exactly as a database administrator with filesystem access can read data files regardless
of `GRANT`. This is the intended guarantee: authorization constrains authenticated access through the
engine's checked surface, and a deployment that relies on it (a shared service) is the one that does
not hand out `loom-core` or the raw file. Confidentiality of the at-rest bytes is a separate door
(whole-file encryption, 0009 §3); confidentiality from an untrusted host is a third
(`PRINCIPALS-AND-ACCESS-CONTROL-LANDSCAPE.md` §3.1, the `e2e-sync` capability). Authorization is none
of these and does not claim to be.

## 7. Cross-workspace operations

History operations never cross workspaces (`CROSS_WORKSPACE`, 0014 §7), so they need authorization only
on their single workspace. A read that legitimately spans workspaces MUST hold a
`Read` grant on **every** workspace it touches (decision 13.5); a `Deny` on any touched workspace
blocks the whole read (deny-precedence, §3). This composes with the existing grammar and needs no new
right.

A **program** (0015) is not special here: it runs as a principal (its `run_as`, 0029 §8) and is bound
by that principal's grants and the subset its manifest declares (0015 §6.1, the shared algebra of §2).
A program may therefore touch several workspaces - e.g. read workspace `B` and write workspace `C` - by
holding a `Read` grant on `B` and a `Write` grant on `C`; each individual operation still targets a
single workspace (no operation spans workspaces, so `CROSS_WORKSPACE` is unaffected). Cross-workspace
data flow is thus an **authorization** outcome - which grants the program's principal holds, enforced
at the PEP - not something the engine forbids outright. (With whole-Loom encryption, 0009 §3, every
workspace a program reaches is inside the same encryption boundary, so this flow does not cross a
confidentiality boundary either.)

## 8. Interface (`Acl` facade)

Uses principals from the `Identity` facade in 0026 §7. Illustrative IDL only (non-normative):

```idl
interface Acl {
  grant(g: Grant): void                         // requires Admin
  revoke(g: Grant): void                         // requires Admin
  create_role(name: string, grants: List<Grant>): RoleId
  set_role_grants(role: RoleId, grants: List<Grant>): void
  // Inspect effective permission (read-only; useful for tooling and tests).
  effective(principal: PrincipalId, resource: ResourceRef): Set<Right>
}
```

The error for a denied operation is the already-registered stable code `PERMISSION_DENIED` (0003 §8).
Path and ref scoping use glob matching (`globset`, MIT OR Apache-2.0).

## 9. Interaction with existing specs

- **0026** - principals and roles are the subjects of grants; `Admin` and recovery authority are the
  management identities.
- **0015** - the grant grammar is the compute capability grammar (0015 §6.1) widened with the
  version-control axes; CEL (0015 §6) evaluates conditional grants.
- **0029** - a trigger's authority is its `run_as` principal's grants, resolved at fire time and
  fail-closed; `reassign` requires `Admin` (0029 §8).
- **0027a** - maps each public operation family to its authorization decision and tracks remaining PEP
  hook work.
- **0030** - a `watch` subscription is a read; it passes the PEP and is narrowed to the authorized
  subset (0030 §9).
- **0009** - provides the security boundary and stable error-code context; authorization remains a
  live-interface policy, not at-rest confidentiality.
- **0009a** - owns future audit, signature, trust-set, and governance records that compose with ACL
  decisions.
- **0028** - extends this below the workspace (ref/path/facet scoping, `Merge`/`Exec` in anger, deny
  grants in common use) without changing the evaluation rule (§3).

## 10. Security

The adversary is an authenticated client doing more than it should: a read-only account writing, a
tenant reading another tenant's workspace, an automation account merging to a protected ref. The PEP
(§4) is the single chokepoint, default-deny (§3) is the safe baseline, deny-precedence makes explicit
prohibitions irreversible by a later allow. Audit and governance records are owned by 0009a after the
required substrates are promoted. The boundary this does not cross is the at-rest and untrusted-host
threats (§6), which are encryption's concern. Conditional predicates and feed narrowing fail closed.

## 11. Resolved decisions

1. **Grant storage and evaluation cost.** V1 evaluates authorization against the stored grant set on
   each call so revocation is immediate. A later implementation may add a per-principal effective
   permission cache keyed by a grant-version stamp, but cache invalidation must preserve immediate
   revocation semantics.
2. **Default grants for a new principal.** A new principal receives no default grants. Default-deny is
   the safe baseline; deployments that want a baseline explicitly assign a role at creation.
3. **Unauthenticated root-mode bypass.** Before a root credential is set or a second principal is
   added, the initial root principal acts without authentication and ACL evaluation is bypassed. Once
   authentication is enforced, there is no implicit owner fallback and default-deny applies.

## 12. Sources

- Design note and decisions (13.3, 13.5): `PRINCIPALS-AND-ACCESS-CONTROL-LANDSCAPE.md` §6, §7, §13.
- Capability grant grammar: `specs/0015-execution-and-logic.md` §6.1;
  `crates/loom-compute/src/capability.rs` and `crates/loom-compute/src/manifest.rs`. CEL guard:
  target work in 0015.
- Security boundary and governance split: `specs/0009-security-and-capabilities.md` and
  `specs/0009a-governance-and-authenticity.md`.
- Workspace isolation and cross-workspace reads: `specs/0014-workspaces.md` §7.
