# 0052 - Certificate Bundles

**Status:** Draft target. **Version:** 0.1.0.
**Capability:** `certificate-bundles`.

**Depends on:** 0005 (single-file store), 0008 (hosted protocols), 0009 (encryption at rest), 0026
(principals and identity), 0027 (access control), 0030 (observability), 0034 (key sources), and 0036
(daemon coordination).

This spec defines portable, named TLS certificate bundles stored inside a `.loom` file. Certificate
bundles replace hosted listener TLS references to host filesystem PEM paths. Copying a `.loom` file
therefore copies the hosted TLS material and durable hosted listener intent together.

## Completion state

The certificate-bundle slice is source-backed for storage, CLI management, served-listener bundle
references, daemon bundle loading, remove guards, daemon restart reconciliation, non-secret reference
reporting, doctor health reporting, and evidence mapping. Remaining target work is limited to future
compatibility migration if a released store profile or external compatibility tool introduces a durable
path-reference listener record, plus hosted-protocol work outside certificate lookup and restart
validation.

Decision Points: none.

Resolved design directions:

- Certificate bundles are store-global control-plane records in `FileStore`, not workspace facets.
- The canonical operator command is `loom certificate`.
- Private-key import into an unencrypted `.loom` fails with a reason unless the operator passes
  `--force`.
- `loom serve` uses `--tls-certificate-bundle <name>` for hosted TLS. Legacy TLS path flags are
  removed.
- Reference counts are computed from durable served listener and hosted-project records.

## Source-backed state

| Claim | Source |
| --- | --- |
| Served listener records currently store TLS mode plus an optional certificate-bundle reference. | `crates/loom-store/src/lib.rs` |
| `loom serve` currently accepts `--tls-certificate-bundle` and validates the referenced bundle exists before saving. | `crates/loom-cli/src/cli.rs`, `crates/loom-cli/src/serve_cmd.rs` |
| Daemon startup currently resolves direct TLS material from certificate bundles and passes stored PEM bytes to rustls helpers. | `crates/loom-cli/src/daemon_cmd.rs`, `crates/loom-hosted/src/serve.rs` |
| Certificate removal computes served-listener references and denies removal while a listener still references the bundle. | `crates/loom-cli/src/certificate_cmd.rs` |
| Daemon reconciliation fingerprints active certificate-bundle material and restarts listeners when the referenced bundle bytes change. | `crates/loom-cli/src/daemon_cmd.rs` |
| Certificate list and audit output report computed served-listener references without printing raw PEM or private-key bytes. | `crates/loom-cli/src/certificate_cmd.rs` |
| `loom doctor store <store>` and `loom doctor all <store>` report invalid certificate bundles through the same parser used by certificate audit. | `crates/loom-cli/src/cli.rs`, `crates/loom-cli/src/daemon_cmd.rs`, `crates/loom-cli/src/certificate_cmd.rs` |
| X.509 display parsing uses `x509-parser`; generated self-signed bundles use `rcgen`. | `crates/loom-cli/src/certificate_cmd.rs` |
| No legacy served-listener TLS path-reference shape remains in source. | `crates/loom-store/src/lib.rs`, `crates/loom-cli/src/cli.rs` |
| The single-file format goal is self-contained portability. | `specs/0005-single-file-format.md` |

## Model

A certificate bundle is a named, store-global control-plane object. It is not a workspace, not a
workspace facet, and not part of workspace history. It is durable local store state that travels with
the `.loom` file and is addressed by a stable operator-selected name.

The first bundle profile is `tls-server-direct`. It contains:

- `name`: a stable bundle name.
- `profile`: `tls-server-direct`.
- `server_cert_chain_pem`: one or more PEM encoded X.509 certificates served to clients.
- `private_key_pem`: one PEM encoded private key matching the server certificate.
- `trust_bundle_pem`: optional PEM encoded certificate authorities used for client certificate
  verification.
- `created_audit_seq` and `updated_audit_seq`: audit sequence references.
- `server_cert_chain_digest`, `private_key_digest`, and `trust_bundle_digest`: store-profile digests
  of the imported bytes for audit and restart validation.
- Parsed non-secret summary fields cached or recomputed for audit display.

The private key is sensitive material. Implementations MUST NOT print private-key bytes, raw PEM
blocks, or decrypted key material in CLI output, audit output, logs, errors, MCP resources, hosted
responses, or conformance reports.

## Name profile

Bundle names are UTF-8 strings from 1 to 128 bytes. The portable profile SHOULD accept only ASCII
letters, ASCII digits, `.`, `_`, and `-`, with no leading `.` and no path separators. Names are
case-sensitive.

An `import` or `generate` for an existing name fails unless a later replace operation is explicitly
specified. The first source-backed surface has no rename operation; operators create a new bundle and
move references.

## Storage

Certificate bundles live in `FileStore` control-plane state alongside served listener records and audit
state. They are encoded with an explicit magic, schema version, name, profile, payload lengths, and
digest fields. Decode rejects trailing bytes, unknown mandatory fields, invalid names, missing server
certificate chains, missing private keys, and empty trust bundles.

The payload bytes are stored inside the `.loom` file. A copied `.loom` file has enough information to
restart hosted listeners configured with certificate bundles, subject to unlock requirements for
encrypted stores.

If the store is encrypted, bundle payload bytes are protected by the same store encryption profile as
other stored bytes. If the store is not encrypted and the import contains a private key, import fails
with an error that names the risk and recommends creating or migrating to an encrypted store. Passing
`--force` permits the import and records an audit event that the private key was stored in an
unencrypted store by explicit operator override.

## CLI

The canonical command family is:

```text
loom certificate list <store>
loom certificate import <store> <name> --cert-chain <file> --private-key <file> [--trust-bundle <file>] [--force]
loom certificate export <store> <name> [--cert-chain <file>] [--private-key <file>] [--trust-bundle <file>] [--force]
loom certificate generate self-signed <store> <name> [--dns <name>] [--ip <addr>] [--cn <name>] [--days <days>] [--algorithm <p256|p384|ed25519>] [--force]
loom certificate remove <store> <name>
loom certificate audit <store> <name>
```

`import` copies the referenced files into the `.loom` file. It validates that the certificate chain
parses, the private key parses, the key matches the leaf certificate through the same rustls
certificate/key loading path used by hosted TLS, and the optional trust bundle parses and contains at
least one certificate.

`export` copies stored certificate material back to files. Exporting a private key requires `--force`
and writes the private-key output with owner-only permissions on platforms that support them.

`generate self-signed` creates a new certificate bundle in the `.loom` file. The first generator
supports self-signed TLS server certificates with DNS and IP subject alternative names and signing
algorithms `p256`, `p384`, and `ed25519`.

`list` displays names, profiles, creation or update audit sequence, safe certificate summary, and
computed reference count. It does not display raw certificate or key bytes.

`audit` displays safe parsed certificate information:

- subject common name when present;
- issuer common name when present;
- not-before and not-after timestamps;
- expiration and expiring-soon health status;
- certificate count in the server chain;
- trust-bundle certificate count;
- stored digest fields;
- computed references to served listeners and hosted projects after hosted listener references land.

`loom doctor store <store>` consumes the same safe parser and reports certificate-bundle health. The first health
checks are invalid certificate material, expired leaf certificates, and leaf certificates expiring
within 30 days. Doctor output MUST identify the bundle name and non-secret
certificate summary only; it MUST NOT print private-key bytes or raw PEM payloads.

`remove` fails when computed references are non-empty. The error identifies the referencing listener or
hosted project ids. Removing an unreferenced bundle deletes its payload and record and appends an audit
event.

## Hosted listener integration

`loom serve configure` uses one hosted TLS flag:

```text
loom serve configure <store> <surface> [selector...] --bind <addr> --tls-certificate-bundle <name>
```

The bundle implies direct TLS. `--tls-mode`, `--tls-cert-ref`, `--tls-key-ref`, and
`--trust-bundle-ref` are not accepted. A listener configured with a missing bundle is invalid and is
not saved.

Served listener records carry a single optional certificate-bundle name. Daemon startup resolves the
name to in-store bytes, rebuilds the rustls server configuration from those bytes, and records the
bundle digests used for restart validation. If bundle parsing fails, the listener is rejected
fail-closed and the rejection is audited.

When a daemon restarts, every enabled hosted listener with a certificate bundle MUST restart with the
same bundle name and current bundle digests. If the bundle record changed since the prior runtime, the
daemon opens the listener with the new bundle bytes and emits deterministic close/open audit events. If
the bundle is missing or invalid, the daemon rejects the listener and leaves it closed.

## Reference inventory

Reference counts are computed, not stored. The reference inventory scans:

- served listener records;
- hosted project records if they are distinct from served listener records;
- future durable hosted binding records that carry certificate-bundle names.

The computed inventory is used by `list`, `audit`, `remove`, daemon validation, and conformance.
Mutable counters are not part of the bundle record because they can drift from durable references.

## Unreleased path-reference handling

The certificate-bundle surface replaces the unreleased served-listener TLS path-reference shape.
Source no longer carries `--tls-cert-ref`, `--tls-key-ref`, `--trust-bundle-ref`, or a persisted path
reference record shape. Operators import PEM files with `loom certificate import`, then configure
listeners with `loom serve ... --tls-certificate-bundle <name>`.

A future migration command is required only if a released store profile or external compatibility tool
introduces a durable path-reference record that this source tree can read. Such a command MUST audit
each imported bundle and each served listener rewrite.

## Security and audit requirements

Certificate-bundle actions require global Admin authorization. Audit events include:

- bundle import;
- bundle import with unencrypted-store `--force`;
- bundle export;
- bundle export private key with `--force`;
- bundle generate self-signed;
- bundle generate self-signed with unencrypted-store `--force`;
- bundle remove;
- bundle remove denied due to references;
- bundle audit;
- served listener configured with bundle;
- daemon listener open with bundle digest;
- daemon listener reject due to missing or invalid bundle.

Audit targets include the bundle name and non-secret digest fields. Audit targets MUST NOT include
private-key bytes or raw PEM payloads.

## Conformance

The source-backed gate for this spec is covered by tests and source evidence for:

- import, export, generate, list, audit, and remove behavior;
- invalid certificate, invalid private key, mismatched key, and empty trust-bundle rejection;
- unencrypted private-key import rejection and `--force` override audit;
- safe audit output redacts raw private-key and PEM payloads;
- computed reference counts block removal;
- `loom serve configure` accepts bundle names and rejects legacy TLS path flags;
- daemon TLS load uses in-store bytes, not filesystem certificate paths;
- daemon restart validation records expected bundle digests;
- copying a `.loom` file preserves enough certificate-bundle state to restart the same hosted listener
  after unlock;
- `loom doctor` reports invalid, expired, and soon-expiring bundles without exposing private keys or
  raw PEM payloads.

Source-backed evidence:

| Requirement | Evidence |
| --- | --- |
| MCP initialize identity reports Loom, not rmcp. | `crates/loom-mcp/src/server.rs` sets `Implementation::new("loom", env!("CARGO_PKG_VERSION"))` and title `Loom MCP`; `crates/loom-mcp/src/server/tests.rs` contains `capabilities_advertise_host_primitives`. |
| Certificate bundles persist inside the store and are audited. | `crates/loom-store/src/tests.rs::certificate_bundle_persists_and_is_audited_with_force_for_unencrypted_store`. |
| Encrypted stores accept certificate bundles without unencrypted-store override. | `crates/loom-store/src/tests.rs::encrypted_store_accepts_certificate_bundle_without_force`. |
| CLI generate, export, import, audit, and remove behavior exists. | `crates/loom-cli/src/certificate_cmd.rs::certificate_generate_export_import_audit_and_remove`. |
| Mismatched certificate and private key are rejected. | `crates/loom-cli/src/certificate_cmd.rs::certificate_material_rejects_mismatched_key`. |
| Remove guards use computed served-listener references and report them. | `crates/loom-cli/src/certificate_cmd.rs::certificate_remove_rejects_served_listener_reference`. |
| Daemon direct TLS loads stored bundle bytes. | `crates/loom-cli/src/daemon_cmd.rs::daemon_opens_direct_tls_cas_rest_listener`. |
| Daemon restart reconciliation reacts to bundle material changes. | `crates/loom-cli/src/daemon_cmd.rs::daemon_restarts_listener_when_certificate_bundle_changes`. |
| Invalid trust bundles fail closed during daemon listener startup. | `crates/loom-cli/src/daemon_cmd.rs::daemon_rejects_invalid_trust_bundle_listener`. |
| Doctor reports invalid certificate-bundle material through the safe parser. | `crates/loom-cli/src/certificate_cmd.rs::certificate_doctor_reports_invalid_bundle_material`. |

Verification commands should be batched because these targets compile high-fanout crates:

```text
cargo test -j 1 -p uldren-loom-cli certificate_
cargo test -j 1 -p uldren-loom-cli daemon_opens_direct_tls_cas_rest_listener
cargo test -j 1 -p uldren-loom-cli daemon_rejects_invalid_trust_bundle_listener
cargo test -j 1 -p uldren-loom-store certificate_bundle
cargo test -j 1 -p uldren-loom-mcp --features server capabilities_advertise_host_primitives
```
