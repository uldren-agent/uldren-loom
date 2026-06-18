# CP-0001 - Control-Plane Reference Landscape

**Series:** Control-plane bindings (informative companion)
**Version:** 0.1.0-draft - **Status:** Draft - **Last updated:** 2026-06-18
**Reads first:** [`CP-0000-index.md`](./CP-0000-index.md). Companion to the per-capability binding docs
(`CP-0002`...`CP-0006`); the control-plane analog of
[`../facet-bindings/REFERENCE-IMPLEMENTATIONS.md`](../facet-bindings/REFERENCE-IMPLEMENTATIONS.md).

For each control-plane capability: the **reference implementations** (OSS / commercial/managed), the
**governing standard(s)**, **how clients connect**, and **Loom's posture** vs that landscape (the fidelity
note folded in, per CP-RD4). Scope is the five control-plane capabilities; the AI providers' landscape lives
in [`../ai/AI-0001-providers-binding.md`](../ai/AI-0001-providers-binding.md) §6 (they were broken out,
CP-RD3).

## Master map

| Capability | Reference impls (OSS / commercial) | Governing standard(s) | How clients connect |
| --- | --- | --- | --- |
| `exec` | Wasmtime/wasmi/WasmEdge, OPA, eBPF, PL/pgSQL · AWS Lambda, Cloudflare Workers, Deno Deploy | WebAssembly Core (W3C), WASI; CloudEvents (trigger) | invoke API (HTTP/gRPC) or embedded runtime |
| `watch` | Debezium, Postgres logical replication, Mongo change streams, Kafka Connect · DynamoDB Streams, Fivetran, Airbyte | CloudEvents (envelope), SSE (transport); CDC (pattern) | subscribe/stream: gRPC stream, SSE, Kafka topic, polling cursor |
| `trigger` | cron/systemd-timers, Airflow, Temporal, Argo Events, DB triggers · EventBridge, Step Functions, GitHub Actions, Zapier | cron syntax; CloudEvents; webhooks (de facto) | schedule-registration API; webhook endpoints; gRPC (Temporal 7233) |
| `identity` | Keycloak, Ory (Kratos/Hydra), Zitadel, Authentik, OpenLDAP, Dex · Auth0, Okta, AWS Cognito, Entra ID | OIDC, OAuth 2.0 (RFC 6749), SAML 2.0, SCIM (RFC 7644), LDAP, WebAuthn | OIDC/OAuth2 flows (HTTPS); LDAP 389/636; SAML POST/redirect |
| `acl` | OPA/Rego, OpenFGA, Casbin, Oso, Cedar, Ory Keto · Amazon Verified Permissions, Auth0 FGA, Styra, Permit.io | XACML (OASIS), NIST RBAC, Zanzibar (paper); langs: Rego/Cedar/Polar | decision API (OPA HTTP 8181; OpenFGA 8080/gRPC 8081) or embedded lib |

## Per-capability detail

### `exec` - programmable logic over the store (binding: CP-0002)

| Dimension | Detail |
| --- | --- |
| Reference impls - OSS | [Wasmtime](https://wasmtime.dev/), [wasmi](https://github.com/wasmi-labs/wasmi), [WasmEdge](https://wasmedge.org/), [OPA](https://www.openpolicyagent.org/) (policy exec), eBPF, PostgreSQL PL/pgSQL stored procedures |
| Reference impls - commercial/managed | AWS Lambda, Cloudflare Workers, Deno Deploy, Supabase Edge Functions, Fastly Compute |
| Governing standard(s) | [WebAssembly Core (W3C)](https://www.w3.org/TR/wasm-core-2/) + [WASI](https://wasi.dev/); event triggering via [CloudEvents](https://cloudevents.io/). **No standard** for "content-addressed program + capability manifest." |
| How clients connect | A function-invoke API (HTTP/gRPC) or an embedded runtime; FaaS platforms add their own deploy/invoke APIs |
| **Loom posture** | Loom-native: programs are **content-addressed Blobs** with a **capability manifest**, run on a **throwaway branch** with a **diff before merge** (`dry_run`/`apply`, 0015). No reference combines content-addressed code + capability-from-manifest + branch/diff/merge, so `exec` is bespoke, MCP-first, no faithful foreign adapter (CP-0002 §6). |

### `watch` - change feed / observability (binding: CP-0003)

| Dimension | Detail |
| --- | --- |
| Reference impls - OSS | [Debezium](https://debezium.io/), Postgres [logical replication](https://www.postgresql.org/docs/current/logical-replication.html), [MongoDB change streams](https://www.mongodb.com/docs/manual/changeStreams/), [Kafka Connect](https://kafka.apache.org/documentation/#connect), Maxwell |
| Reference impls - commercial/managed | AWS DynamoDB Streams, Confluent CDC connectors, Fivetran, Airbyte |
| Governing standard(s) | **No single standard.** [CloudEvents (CNCF)](https://cloudevents.io/) for the event envelope; [Server-Sent Events](https://html.spec.whatwg.org/multipage/server-sent-events.html) for browser transport; CDC is a *pattern*, not a spec |
| How clients connect | Subscribe/stream: a gRPC stream, an SSE feed, a Kafka topic, or a polling cursor/offset |
| **Loom posture** | A **commit-DAG-ordered, resumable-by-cursor** feed scoped by ref/path/facet (`subscribe`/`poll`/`stream`, 0030). Maps cleanly to CDC (cursor <-> offset); the fidelity ceiling is payload richness. Element-level before/after images exist only where a facet's diff provides them (CP-0003 §6/OQ-W2). |

### `trigger` - reactive automation & scheduling (binding: CP-0006)

| Dimension | Detail |
| --- | --- |
| Reference impls - OSS | cron / systemd timers, [Apache Airflow](https://airflow.apache.org/), [Temporal](https://temporal.io/), [Argo Events](https://argoproj.github.io/argo-events/), [n8n](https://n8n.io/), DB triggers |
| Reference impls - commercial/managed | AWS EventBridge / Step Functions, GitHub Actions, Zapier, Google Cloud Scheduler, Temporal Cloud |
| Governing standard(s) | cron expression syntax (Vixie/POSIX; Loom uses [croner](https://github.com/hexagon/croner)); [CloudEvents](https://cloudevents.io/); webhooks (de facto, no formal spec) |
| How clients connect | A schedule/binding-registration API; inbound/outbound webhook endpoints; gRPC (Temporal frontend 7233) |
| **Loom posture** | Durable **cron/change to stored `exec` program under capability**, on the Loom backend with **no external job queue** (ADR-0006); time is a **seeded input** to idempotent re-fire. It is durable cron/change-to-exec, **not** a DAG workflow orchestrator like Airflow/Temporal (CP-0006 §6). |

### `identity` - authentication & principals (binding: CP-0004)

| Dimension | Detail |
| --- | --- |
| Reference impls - OSS | [Keycloak](https://www.keycloak.org/), [Ory](https://www.ory.sh/) (Kratos/Hydra), [Zitadel](https://zitadel.com/), [Authentik](https://goauthentik.io/), OpenLDAP, [Dex](https://dexidp.io/) |
| Reference impls - commercial/managed | Auth0, Okta, AWS Cognito, Microsoft Entra ID, Google Identity Platform |
| Governing standard(s) | [OpenID Connect](https://openid.net/connect/), [OAuth 2.0 (RFC 6749)](https://www.rfc-editor.org/rfc/rfc6749), SAML 2.0 (OASIS), [SCIM (RFC 7644)](https://www.rfc-editor.org/rfc/rfc7644), LDAP (RFC 4511), [WebAuthn/FIDO2 (W3C)](https://www.w3.org/TR/webauthn-2/) |
| How clients connect | OIDC/OAuth2 browser+token flows (HTTPS); LDAP on 389/636; SAML POST/redirect; Keycloak HTTP 8080 / 8443 |
| **Loom posture** | An internal **principal store** (user/service/root) with password (Argon2id) + Ed25519 credentials first, certificate deferred; owner-vs-authenticated mode + root bootstrap (0026). External IdPs (OIDC/LDAP/SAML) are a **federation Tier-2** that verifies and maps to a Loom principal (CP-0004 §6). |

### `acl` - authorization & policy (binding: CP-0005)

| Dimension | Detail |
| --- | --- |
| Reference impls - OSS | [OPA/Rego](https://www.openpolicyagent.org/), [OpenFGA](https://openfga.dev/) (Zanzibar ReBAC), [Casbin](https://casbin.org/), [Oso](https://www.osohq.com/), [Cedar](https://www.cedarpolicy.com/), [Ory Keto](https://www.ory.sh/keto/) |
| Reference impls - commercial/managed | Amazon Verified Permissions (Cedar), Auth0 FGA, Styra DAS (OPA), Permit.io, Aserto |
| Governing standard(s) | **No single standard.** XACML 3.0 (OASIS), [NIST RBAC](https://csrc.nist.gov/projects/role-based-access-control), [Google Zanzibar](https://research.google/pubs/pub48190/) (paper); policy languages Rego, Cedar, Polar |
| How clients connect | A decision API ([OPA HTTP 8181](https://www.openpolicyagent.org/docs/security), [OpenFGA HTTP 8080 / gRPC 8081](https://github.com/openfga/openfga)) or an embedded library (Casbin, Cedar, Oso) |
| **Loom posture** | Grant-based + roles + **deny-precedence + default-deny + CEL conditions**, enforced by an **in-engine PEP** (0027) with fine-grained scopes (0028). Foreign engines (OPA/OpenFGA) interop by **exporting** Loom grants, not by delegating the decision outward (CP-0005 §6 / CP-RD-A1). |

## Cross-cutting observation

The control-plane references split into two shapes Loom relates to differently:

- **Federate-able (identity):** mature open standards (OIDC/SAML/LDAP) - Loom should *adopt* them as a Tier-2
  federation layer rather than reinvent enterprise auth.
- **Loom-native-by-design (exec, acl, trigger, watch):** no single standard, and Loom's value (content
  addressing, versioning, branch/merge, in-engine enforcement, durable backend) is *why* these are bespoke.
  Foreign adapters here are **interop/export**, not authority hand-off - the opposite of the data facets,
  where Tier-2 (pg-wire, S3) is about *being* the reference protocol.
