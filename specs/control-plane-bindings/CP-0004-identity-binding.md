# CP-0004 - `identity` Binding

**Series:** Control-plane bindings (normative-track sub-series; Draft)
**Version:** 0.1.0-draft - **Status:** Draft - **Last updated:** 2026-06-18
**Reads first:** [`CP-0000-index.md`](./CP-0000-index.md),
[`../facet-bindings/P9-0002-projection-conventions.md`](../facet-bindings/P9-0002-projection-conventions.md),
facade spec **0026** (principals & identity), **0008 §5** (transport auth, where `authenticate` issues the
token everything else uses), **0027** (the authorization it feeds).

`identity` is **foundational, not a peer facet**: `authenticate` is what mints the session/token that
**every** other binding (all of `facet-bindings/`, MCP write-gating P9-0016 §3, `acl` CP-0005) checks. So
this binding underpins the whole wire surface.

## 1. Facade surface (0026 `Identity`)

Bootstrap: `set_root_credential(cred)`, `enable_authentication()` (refused without a root credential),
`disable_authentication()`. Principal management (root / `admin` right): `create_principal(name, kind,
creds) -> PrincipalId`, `add_credential`, `remove_credential`, `disable_principal`, `assign_role`. Session:
`authenticate(cred) -> Session`. `PrincipalKind` in {user, service, root}; first-class credentials are
password (Argon2id) + Ed25519 public key (certificate/secp256k1 deferred). Codes:
`IDENTITY_NO_ROOT_CREDENTIAL`, `AUTHENTICATION_FAILED`.

**Build state:** spec-only (0026 Draft, P12).

> **Naming decision.** 0026 projects as the **`Identity`** facade and 0027 projects as the **`Acl`**
> facade. This avoids the old `Auth`/`Auth` collision in generated IDL/proto surfaces.

## 2. Tier-1 - REST

| Method | HTTP |
| --- | --- |
| `authenticate` | `POST /identity/login {cred}` -> `{session token}` (the bearer used per 0008 §5) |
| `set_root_credential` / `enable`/`disable` auth | `POST /identity/root-credential`; `POST /identity:enable`; `POST /identity:disable` (root-only, audited) |
| `create_principal` / `disable_principal` | `POST /identity/principals`; `POST /identity/principals/{id}:disable` |
| `add_credential` / `remove_credential` | `POST /identity/principals/{id}/credentials`; `DELETE /identity/principals/{id}/credentials/{cred_id}` |
| `assign_role` | `POST /identity/principals/{id}/roles {role}` |

## 3-4. JSON-RPC / gRPC

JSON-RPC 1:1 (`identity.authenticate`, `identity.createPrincipal`, etc.). gRPC unary. `authenticate` is the
one method that **precedes** auth (it establishes it); everything else requires root/`admin`.

## 5. Tier-1 - MCP

- **Not a general agent surface.** Principal/credential management is high-privilege admin. Expose at most a
  read **`identity.whoami`** (the session's resolved principal) as a convenience; all mutating tools
  (`create_principal`, credential ops, `enable_authentication`) are **root/admin-token-gated** and off by
  default for agents.

## 6. Tier-2 - foreign adapter - OIDC / OAuth2 / LDAP / SAML (federation)

Rather than Loom owning every credential, **federate** to an external identity provider:

- **OIDC/OAuth2:** accept an external IdP access/ID token at the transport (0008 §5 already allows an "OIDC
  access token") and **map the verified subject to a Loom `PrincipalId`** (auto-provision a `user`
  principal or match an existing one).
- **LDAP/AD, SAML:** enterprise directory federation for `user` principals.
- **Fidelity ceiling:** 0026's pluggable-authentication trait currently specifies password + Ed25519;
  OIDC/LDAP are federation adapters layered on that trait. Loom
  still owns authorization (`acl`, 0027) regardless of who authenticates. Native-only server.

## 7. Errors / parity / concurrency

- **Errors:** `IDENTITY_NO_ROOT_CREDENTIAL`, `AUTHENTICATION_FAILED` (0026/0010 §4); `PERMISSION_DENIED`
  (0027) for unauthorized management.
- **Parity (0032):** the principal store rides the object model (portable); credential hashing (Argon2id)
  and signature verify (Ed25519) are pure-Rust. OIDC/LDAP federation servers are native-only.
- **Concurrency:** the root bootstrap (`set_root_credential` + `enable_authentication`) MUST be atomic and
  refuse without a usable root credential (0026 §4); single-writer for the principal store.

## 8. Resolved Decisions

### CP-RD-I1 - Rename to resolve the old `Auth`/`Auth` collision

- **Decision.** Rename the 0026 projection to **`Identity`** and the 0027 projection to **`Acl`**.
  Authentication and authorization remain separate surfaces.

### CP-RD-I2 - OIDC/LDAP as federation adapters

- **Context.** 0026's authn is pluggable (password + Ed25519 now); external IdPs can be modeled as new
  credential kinds or as a separate federation layer that maps subjects to principals.
- **Decision.** Treat OIDC/LDAP/SAML as federation adapters that verify external credentials and map to
  or auto-provision a Loom principal. Internal password/Ed25519 credentials remain the first source-backed
  model for service/root principals.
