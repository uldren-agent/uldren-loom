# uldren-loom-conformance

Canonical conformance vectors and a generic runner that every Uldren Loom backend, in every
language, must reproduce. The vectors pin the data model - digests and the canonical object
encoding - so behavior stays identical across the polyglot bindings.

`certify_memory_store()` is the aggregate entry point: it runs the canonical vectors, including the
`exec-manifest` positive and negative byte vectors, the `substrate-model` vector runner for the
source-backed operation-substrate primitives, and the `meetings-profile` vector runner for Meeting
Memory snapshots, projection effects, redaction invalidation, extraction review, and evidence
ordering, plus the
executable behavioral suites (`cas`, `cas-facade`, `workspace`, `sync`, `queue`, `queue-consumer`,
`delivery`, `vcs-diff`, `lock`, `identity`, `acl`, `kv`, `kv-ephemeral`, `document`, `time-series`,
`ledger`, `graph`, `vector`, `columnar`, `dataframe`, `search`, `calendar`, `contacts`, `mail`,
`pim-trigger`, `inference`, `providers.embedding`, `sql-errors`, `sql-history`, `merge-conflict`,
`staging`, `file-ops`, `file-handle`, `symlink`, `tags`, `restore`, `replay`, `squash`,
`protected-ref`, `exec`, and `sql-state-access`) against fresh in-memory backends and returns a typed
`ConformanceSummary`.
Declarative-only scenario suites are listed in the summary as inventory, never reported as passed.
The `acl` runner covers default-deny, deny-precedence, selected engine PEP hooks, role-grant
expansion, immediate role revocation, and ref/path scoped grants. The `sync` runner also proves
authenticated push refuses missing source read and destination advance grants.
The `exec` runner covers gated/direct/batched execution plus promoted multi-facet `StateAccess`
operations for files, CAS, document, queue, time-series, ledger, graph, columnar, search, vector, and
dataframe; SQL and PIM execution have dedicated runners.

`NETWORK_ACCESS_VECTORS` records data-only hosted admission cases for CIDR first-match behavior,
default deny, trusted proxy headers, malformed `Forwarded` handling, and mTLS SAN matching. The vectors
are intentionally not part of `run_all_vectors()` because network-access storage and runtime
enforcement live above the core conformance crate boundary.

## Binding conformance inventory

`BINDING_CONFORMANCE_INVENTORY` records every binding-adjacent surface and the strongest checked-in
evidence behind it, sorted into four `BindingTier`s so the reporting stays factual:

- `ExecutableCore` - source-backed suites `cargo test --workspace` runs in-tree on every CI run (`just
  ci`): the canonical vectors, the C ABI (`crates/loom-ffi`), and the result codec fixture. These are
  the binding-inventory surfaces reported as CI-gated. The hosted auth/ACL matrix is also inventoried
  here because it is an in-tree Rust suite under `crates/loom-hosted`; `report_memory_store()` records
  it as skipped rather than claiming it ran in the memory-store certification.
- `BindingRuntimeSuite` - checked-in binding runtime/smoke suites that exist today (`node`, `python`,
  `ios`, `cpp`, `jvm`, `android`, `react-native`, `wasm`) but run only through their own toolchain
  recipes (`just node`/`just python`/`just ios`/`just cpp`/`just jvm`/`just android`/
  `just react-native-android`/`just wasm`), not through `just ci`.
- `ImplementedNotGated` - implemented binding surfaces with a build recipe but no checked-in runtime
  test.
- `TargetOnly` - surfaces specified in `specs/0007-bindings.md` but not implemented today (generated IDL
  bindings, distribution packaging, cross-binding interop, full per-language conformance execution).

A surface is only reported as CI-gated when it is `ExecutableCore`. Tests assert the inventory partitions
cleanly, every non-target surface cites a checked-in file that actually exists on disk, and every
checked-in runtime suite records the promoted surfaces it exercises.
Auth-related labels are explicit: runtime coverage distinguishes identity/ACL administration,
role-ACL management, session authentication, authenticated SQL sessions, and authenticated ordinary
facet operations. These labels are evidence inventory for the checked-in binding suites; they do not
certify hosted protocol auth. Hosted auth/ACL has its own executable evidence row covering auth
failure, permission denial, stable error mapping, security audit, CAS REST serving, hosted admin REST,
hosted admin JSON-RPC, and FIPS listener rejection.

Hosted protocols have separate evidence rows in the `HOSTED_PROTOCOL_FEATURES` matrix. The matrix
records supported bounded profiles for IMAP, CalDAV, CardDAV, JMAP, Qdrant REST, Qdrant unary gRPC,
and Pinecone REST; degraded behavior such as IMAP IDLE completion without push delivery; target
standards or client-transcript work such as CalDAV/CardDAV REPORT, JMAP changes/upload support, and
generated vector-client transcripts; and explicitly unsupported behavior such as durable IMAP
subscriptions, non-synchronizing APPEND literals, direct TLS gaps for current HTTP PIM listeners, SMTP
submission, hosted vector approximate-accelerator policy, and vector integrated-embedding or sparse
request shapes where no configured provider/profile exists.

## Serialized conformance report

`report_memory_store()` builds a `ConformanceReport` over the current source-backed boundary and
`ConformanceReport::to_json()` serializes it to stable machine-readable JSON (no serde dependency). It
records the implementation name/version, an optional source revision injected at compile time via
`LOOM_SOURCE_REVISION` (`None` when unset or blank), the identity profiles exercised, the vector and
behavior suites with a `passed`/`failed`/`skipped`/`inventory`/`target` `ReportStatus`, the linked
runtime provider profile, the binding evidence tiers plus checked-in runtime coverage, and hosted
protocol feature rows with `supported`/`degraded`/`target`/`unsupported` status.

CI (`.github/workflows/ci.yml`) and the release workflow (`.github/workflows/release-plz.yml`) set
`LOOM_SOURCE_REVISION` to the commit SHA (`${{ github.sha }}`) so report builds carry the real revision.
For a local build, set it explicitly, for example `LOOM_SOURCE_REVISION=$(git rev-parse HEAD) cargo test
-p uldren-loom-conformance`.
The report serializes proof only: it never claims hosted protocol, full binding, or provider lifecycle
certification. See `specs/0010a-conformance-reporting-and-certification.md`.

Part of [Uldren Loom](https://github.com/uldrenai/uldren-loom).

## License

Business Source License 1.1 (BUSL-1.1). See the [repository](https://github.com/uldrenai/uldren-loom).
