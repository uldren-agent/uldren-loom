# CP-0005 - `acl` Binding

**Series:** Control-plane bindings (normative-track sub-series; Draft)
**Version:** 0.1.0-draft - **Status:** Draft - **Last updated:** 2026-06-18
**Reads first:** [`CP-0000-index.md`](./CP-0000-index.md), [`CP-0004-identity-binding.md`](./CP-0004-identity-binding.md),
[`../facet-bindings/P9-0002-projection-conventions.md`](../facet-bindings/P9-0002-projection-conventions.md),
facade spec **0027** (access control) + **0028** (fine-grained), and **0009** (security boundary).

`acl` is the other half of the auth foundation: it is the **policy enforcement point** every write across
every binding funnels through. `identity` (CP-0004) says *who you are*; `acl` says *what you may do*.

## 1. Facade surface (0027 `Acl`; extended by 0028)

`grant(g: Grant)` / `revoke(g: Grant)` (require `Admin`), `create_role(name, grants) -> RoleId`,
`set_role_grants(role, grants)`, `effective(principal, resource) -> Set<Right>` (read-only; for tooling/
tests). 0028 adds **fine-grained** grants below the workspace: ref-glob, path/scope prefix, facet, and
`merge`/`exec` rights, with deny-precedence and default-deny. Code: `PERMISSION_DENIED`.

**Build state:** **partial** - the grant grammar (`Capability`/`Scope`/`Mode`/`Grant`/`GrantSet`) is built
in `loom-compute` (shared with the exec capability model); the PEP/evaluator and the management facade are
P12 work.

> **Naming decision:** 0027 projects as the **`Acl`** facade. 0026 projects as **`Identity`**.

## 2. Tier-1 - REST

| Method | HTTP |
| --- | --- |
| `grant` / `revoke` | `POST /acl/grants {grant}`; `DELETE /acl/grants {grant}` (Admin) |
| `create_role` / `set_role_grants` | `POST /acl/roles {name, grants}`; `PUT /acl/roles/{id}/grants` |
| `effective` | `GET /acl/effective?principal=...&resource=...` -> `Set<Right>` |

## 3-4. JSON-RPC / gRPC

JSON-RPC 1:1 (`acl.grant`, `acl.revoke`, `acl.createRole`, `acl.setRoleGrants`, `acl.effective`). gRPC
unary.

## 5. Tier-1 - MCP

- **Read:** `acl.effective` (a "can principal X do Y?" check, safe and useful for agents reasoning about their
  own permissions).
- **Write (root/admin-token-gated):** `acl.grant`, `acl.revoke`, role management - off by default for
  agents; granting permissions is the most sensitive operation in the system.

## 6. Tier-2 - foreign adapter - OPA/Rego or Zanzibar/OpenFGA

Loom's model is **grant-based with roles, deny-precedence, default-deny, and CEL conditions**, enforced by
an **in-engine PEP** (0027). Two reference shapes could interoperate:

- **Google Zanzibar / OpenFGA (relationship-based):** map Loom grants to relationship tuples; useful if an
  org already runs OpenFGA as its central authz store.
- **OPA/Rego (policy-as-code):** externalize decisions to an OPA sidecar evaluating Rego.
- **Fidelity ceiling / caution:** externalizing the decision point **changes Loom's trust model**. 0027
  deliberately keeps the PEP *in-engine* (one enforcement point, default-deny). A foreign adapter should be
  an **export/sync of grants** (so external tools can *read* Loom's policy) rather than delegating the
  *decision* outward unless the owner explicitly wants OPA as the authority. Native-only.

## 7. Errors / parity / concurrency

- **Errors:** `PERMISSION_DENIED` (0027/0010 §4) + core set; an explicit deny overrides a broader allow
  (deny-precedence).
- **Parity (0032):** the grant evaluator is pure-Rust and portable; `globset` (ref/path globs) and CEL
  (conditions) are wasm-capable. The OPA/Zanzibar adapters are native-only.
- **Concurrency:** the PEP is the single funnel all bindings call before a mutating op; grants are stored in
  the reserved internal workspace (versioned, single-writer).

## 8. Resolved Decisions

### CP-RD-A1 - In-engine PEP vs external policy engine

- **Context.** 0027 puts the PEP in-engine; full Tier-2 ambition tempts delegating decisions to OPA or
  OpenFGA, which changes who is authoritative.
- **Decision.** The in-engine PEP stays authoritative. Foreign adapters may export or mirror grants for
  interop, but they do not become the source of authorization decisions unless a later owner decision
  explicitly changes the trust model.
