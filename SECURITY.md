# Security Policy

## Reporting a vulnerability

Please report security issues **privately**. Do not open a public GitHub issue.

- Preferred: open a private advisory via **GitHub Security Advisories** ("Report a vulnerability" on the repository's Security tab).
- Or email **security@uldren.com** with details and reproduction steps.

We aim to acknowledge reports within 3 business days and to provide a remediation timeline after triage.
Coordinated disclosure is appreciated; we will credit reporters who wish to be named.

## Scope

The Rust core (`crates/loom-core`, `loom-ffi`), the CLI (`loom-cli`), and the language bindings (`bindings/*`).
Integrity of the content-addressed object model and the `.loom` container format are treated as high-severity areas.

## Supported versions

Pre-1.0: only the latest release is supported. A formal support window will be published at 1.0.
