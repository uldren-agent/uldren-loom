# 0066 - Network Access

**Status:** Partial, source-backed policy and listener enforcement; transport coverage is mixed.
**Version:** 0.1.0-target.
**Capability:** `network-access`.

**Depends on:** 0005 (single-file format), 0008 (hosted protocols), 0026 (principals and identity),
0027 (access control), 0028 (fine-grained authorization), 0030 (observability), 0036 (locking and
coordination), 0043 (MCP serving surface), 0060 (FIPS distribution and compliance).
**Relates to:** 0003 (core interface), 0009 (security and capabilities), 0010 (canonical reports),
0025 (conformance), 0035 (durable delivery), 0042 (collections).

This spec defines reusable network admission policy for every TCP listener that Loom opens from
durable served-listener configuration. A network access policy controls which clients may establish a
served connection before the request reaches the hosted protocol surface. It is not a replacement for
identity, ACL, or fine-grained authorization. It is the first network gate in front of those checks.

The public CLI command is `network-access`. The stored object is a `NetworkAccessPolicyRecord`.
Served listeners reference a policy by name through `loom serve configure --network-access <name>`.

## 1. Terminology

Loom uses **network access policy** for the reusable object because the contract is broader than an IP
set and narrower than a full packet firewall. The policy applies at listener accept time for Loom-owned
ports.

Reference terminology:

- AWS VPC security groups are stateful resource-level controls with inbound and outbound rules.
- AWS network ACLs are subnet-level allow or deny rule lists.
- AWS WAF IP sets are reusable collections of IP addresses and ranges referenced by web ACL rules.
- AWS Network Firewall uses rule groups attached to firewall policies.
- Firewall products commonly use rules, rulesets, policies, aliases, address groups, allow, pass,
  deny, block, reject, and default deny.

Loom deliberately avoids naming the command `ip-access`: IP and CIDR rules are required, but the
long-term contract also includes mutual TLS client identity and trusted proxy source handling.

## 2. Contract Boundary

Network access policy is evaluated before protocol authentication and before any Loom state read or
write. The result is one of:

- `allow`: continue to the hosted protocol implementation.
- `deny`: close or reject the connection without running request handlers.

The policy is transport-neutral. It applies to HTTP, JSON-RPC, gRPC, IMAP, JMAP, WebDAV-style PIM
surfaces, and any future TCP listener started from `serve configure`.

The policy is listener-local at runtime and reusable in storage. One named policy can be referenced by
many listeners. Removing a referenced policy is rejected, matching the certificate-bundle reference
discipline.

Direct ad hoc commands that bind a port without durable served-listener configuration are outside this
contract unless their command surface explicitly accepts a `--network-access` option and resolves the
same policy record.

## 3. Policy Model

A network access policy has:

```text
network access policy:
  name
  schema_version
  description optional
  default_action   allow | deny
  rules            ordered list of network access rules
  created_audit_seq optional
  updated_audit_seq optional
```

A rule has:

```text
network access rule:
  id
  action           allow | deny
  source_cidr      optional IP CIDR
  trusted_proxy_cidr optional IP CIDR
  require_mtls     optional boolean
  client_cert_subject optional exact string
  client_cert_san  optional exact string
  client_cert_issuer optional exact string
  description      optional
```

Rules are evaluated in order. The first rule whose criteria all match decides the connection. If no
rule matches, `default_action` decides. This is the firewall and network ACL model and gives operators
explicit ordering for exceptions.

The engine stores normalized rule criteria:

- exact IPv4 addresses normalize to `/32`;
- exact IPv6 addresses normalize to `/128`;
- CIDR prefixes normalize to canonical network addresses;
- invalid host bits are rejected rather than silently rewritten;
- IPv4 and IPv6 rules are never compared across families;
- duplicate rule ids inside a policy are rejected.

## 4. Match Inputs

The evaluator receives:

```text
network access context:
  peer_addr        socket peer address
  listener_addr    local listener address
  transport        served transport id
  surface          served surface id
  tls_peer_cert    optional verified peer certificate summary
  forwarded_for    optional parsed forwarded client chain
```

The default source address is `peer_addr.ip`.

Forwarded client addresses are used only when a rule or policy enables trusted proxy handling and the
socket peer matches `trusted_proxy_cidr`. Untrusted `X-Forwarded-For`, `Forwarded`, or protocol
specific proxy headers are ignored. Trusted proxy parsing is conservative: malformed chains fail
closed for rules that require forwarded client identity and otherwise fall back to the socket peer.

## 5. Mutual TLS Criteria

mTLS support is part of this contract. A rule can require a verified peer certificate and can match
its subject, issuer, or subject alternative name. The peer certificate is usable only when the served
listener's TLS configuration performs client certificate verification through a configured trust
bundle.

Policy validation rejects impossible combinations:

- a referenced policy that requires mTLS cannot be attached to a listener with TLS mode `off`;
- a referenced policy that requires mTLS cannot be attached to a direct TLS listener whose certificate
  bundle has no trust bundle;
- mTLS criteria without `require_mtls` imply `require_mtls = true`;
- mTLS criteria on transports that do not currently support direct TLS are rejected by the daemon
  before startup.

The policy stores only match criteria and safe certificate metadata. It never stores private key
material.

## 6. Served Listener Integration

`ServedListenerRecord` gains a network-access reference:

```text
network_access_policy_ref optional string
```

`loom serve configure` accepts:

```text
--network-access <policy-name>
```

The CLI validates that the referenced policy exists before saving the listener. `serve list` includes
the policy reference. The listener target string used in audit records includes the policy name and a
policy fingerprint so daemon restarts and audit review can see the effective network gate.

Changes to a referenced policy must restart affected daemon listeners. The daemon runtime identity
therefore includes the policy fingerprint, similar to TLS bundle fingerprinting.

## 7. CLI Surface

The top-level command is:

```text
loom network-access list <store>
loom network-access set <store> <name> [options]
loom network-access audit <store> <name>
loom network-access remove <store> <name>
```

`set` replaces the named policy atomically and appends an audit record. The command accepts repeated
rule flags and emits the normalized policy as JSON with the audit sequence.

Recommended flag shape:

```text
--description <text>
--default-action allow|deny
--allow-source <cidr-or-ip>
--deny-source <cidr-or-ip>
--allow-mtls-subject <subject>
--deny-mtls-subject <subject>
--allow-mtls-san <san>
--deny-mtls-san <san>
--allow-mtls-issuer <issuer>
--deny-mtls-issuer <issuer>
--trusted-proxy <cidr-or-ip>
```

For complex ordered rules, the CLI also accepts an input file:

```text
--rules <json-file>
```

The JSON rule file is the lossless administrative interface. Convenience flags append rules in the
order provided by the CLI parser.

All outputs are JSON and include enough safe metadata for automation:

- policy name;
- schema version;
- normalized rules;
- default action;
- fingerprint;
- reference count;
- served listener references;
- created and updated audit sequence numbers.

## 8. Runtime Enforcement

Enforcement occurs before protocol handlers:

- HTTP, JSON-RPC, WebDAV-style PIM, and TLS variants evaluate in the shared HTTP accept path before a
  connection is spawned.
- gRPC evaluates through a filtered incoming stream before tonic receives the connection.
- IMAP evaluates in the plain and TLS accept loops before session tasks are spawned.
- JMAP and future HTTP-derived surfaces inherit the HTTP path.

Denied connections are audit-relevant security events. The daemon may rate-limit denied-connection
audit records to avoid log amplification, but it must retain aggregate counters in observability
state.

The evaluator is deterministic and allocation-conscious. CIDR rules compile to family-specific integer
ranges when a listener starts. Runtime matching does not parse CIDR strings.

## 9. Doctor Health

`loom doctor` reports network-access health for a store:

- `network_access_policy_health ok` for valid policies with reference counts;
- `network_access_policy_health unhealthy` for invalid policy records;
- warning for enabled public binds without a network access policy;
- warning for enabled deny-by-default policies with no allow path;
- warning for mTLS-required policies attached to listeners without client trust;
- warning for trusted proxy rules without trusted proxy CIDRs;
- warning for referenced policy changes that require listener restart when runtime drift can be
  detected;
- reference counts and listener ids for each policy.

Public bind detection treats `0.0.0.0`, `::`, and non-loopback concrete addresses as externally
reachable unless a platform-specific probe proves otherwise. Loopback binds do not require a warning.

## 10. Audit and Observability

Management actions append audit records:

```text
network_access.policy.list
network_access.policy.set
network_access.policy.remove
network_access.policy.remove.denied
network_access.policy.audit
```

Daemon events append or aggregate:

```text
network_access.connection.deny
network_access.policy.runtime_drift
```

Audit targets include policy name, listener id, surface, transport, bind address, rule id when
available, action, and policy fingerprint. Denied connection targets must avoid storing untrusted
header values verbatim. Header-derived addresses are reported only after trusted proxy validation.

Observability counters include allowed and denied connection counts by listener, policy, rule id, and
source family.

## 11. Security Properties

Network access policy must fail closed when:

- a referenced policy is missing;
- a policy record is corrupt;
- a policy requires mTLS but no verified peer certificate is available;
- trusted proxy parsing is required but malformed;
- a daemon cannot compile a policy into runtime match structures.

Network access policy must not:

- bypass identity or ACL checks;
- trust forwarded-client headers from untrusted peers;
- expose denied existence of Loom resources;
- store private key material;
- depend on DNS, reverse DNS, ASN, geolocation, or remote reputation lookups for admission decisions.

DNS names, ASN, country, and reputation feeds are intentionally excluded from the core contract. They
can be projected through an external gateway, but they are not deterministic enough for Loom's
content-addressed contract.

## 12. Conformance

Conformance must cover:

- exact IPv4 and IPv6 normalization;
- CIDR host-bit rejection;
- first-match ordering;
- default allow and default deny behavior;
- mTLS-required success and failure;
- trusted proxy accepted and ignored paths;
- missing referenced policy fail-closed behavior;
- reference-safe policy removal;
- daemon restart when a referenced policy fingerprint changes;
- doctor warnings for public bind without policy and mTLS misconfiguration.

### Current source-backed boundary

The current matrix distinguishes evaluator and listener behavior rather than treating the
transport-neutral policy model as proof of equal listener support.

| Boundary | Current status | Evidence |
| --- | --- | --- |
| REST and JSON-RPC HTTP listeners | Source-backed for direct-peer allow and deny, trusted-proxy header handling, and verified-peer-certificate input when direct TLS terminates at the hosted listener. | `crates/loom-hosted-core/src/http.rs`; `crates/loom-protocol-conformance/src/lib.rs` network-access matrix. |
| gRPC listeners | Source-backed for direct-peer allow and deny before tonic receives an accepted stream, plus trusted-proxy request metadata before generated gRPC handlers run. Verified mTLS certificate input remains unpromoted because direct TLS gRPC listeners are rejected by the daemon. | `crates/loom-hosted/src/grpc.rs` `network_access_grpc_incoming`; `crates/loom-hosted/src/grpc.rs` `grpc_network_access_allows_request`; `crates/loom-protocol-conformance/src/lib.rs` network-access matrix. |
| Policy-drift restart | Source-backed and executable: the daemon restarts an affected listener when its referenced policy fingerprint changes. | `crates/loom-cli/src/daemon_cmd.rs` `daemon_restarts_listener_when_network_access_policy_changes`. |
| Denied audit evidence | Source-backed for the evaluator and its sanitized listener and policy identity. | `crates/loom-hosted-core/src/network_access.rs`; `crates/loom-protocol-conformance/src/lib.rs` network-access matrix. |

Remaining target work is verified mTLS input for gRPC if direct TLS gRPC listeners are promoted, plus
live listener coverage certification for every future promoted served surface.

Decision Points: none.
