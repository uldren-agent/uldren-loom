# 0010 - Conformance and Versioning

**Status:** Complete for the current source-backed conformance boundary; reporting expansion is split.
**Version:** 0.1.0. **Normative.**

This document defines how Loom implementations prove compatibility. Source is authoritative: only
checked-in vectors, executable runners, public ABI tests, binding tests, and CI gates can certify a
claim. Scenario text, roadmap entries, and target capability names are planning inputs until executable
coverage exists.

Release-grade generated capability reports, hosted protocol conformance reports, and full binding
certification are target work in 0010a. The current source-backed report and capability projections
are scoped evidence inventories. They must not be read as release certification for untested hosted,
binding, listener, platform, or runtime profiles.

## Current implementation

The workspace currently provides:

- `uldren-loom-conformance`, a Rust crate with canonical vectors and generic runners over
  `ObjectStore`;
- blob canonical-byte and digest vectors for BLAKE3 and SHA-256 profiles;
- object-model vectors for Blob, ChunkList, Tree, Commit, and Tag;
- ledger-head vectors for BLAKE3 and SHA-256 profiles;
- table identity vectors through the real engine path;
- result-payload vectors for binding-level SQL result shapes;
- executable CAS behavior over `ObjectStore`;
- executable workspace behavior over fresh source and destination `Loom` values;
- executable sync behavior for direct workspace clone, fast-forward branch push, non-fast-forward
  rejection, v4 bundle export/import, closure preservation, and profile-mismatch rejection;
- executable queue behavior for append, get, range, len, clone, and bundle preservation;
- executable queue-consumer behavior for authority-local consumer offsets;
- executable identity and ACL behavior for bootstrap auth, default-deny, deny-precedence, role grants,
  immediate role revocation, ref and path scopes, selected engine PEP hooks, and authenticated sync
  push gates;
- executable public-facade behavior for KV, document, graph, vector, ledger, time-series, metrics,
  columnar, dataframe, search, calendar, contacts, and mail;
- executable inference-seam behavior;
- executable workspace-history and file behavior for merge-conflict recovery, staging, file ops, file
  handles, symlink create/read-link, tags, restore, replay, and squash;
- executable capability registry behavior;
- binding conformance inventory tiers that distinguish CI-gated core evidence, checked-in runtime
  suites, implemented-but-ungated bindings, and target-only binding work;
- scenario tables for other facets that are not yet executable certification.

`run_all_vectors` remains the canonical-vector runner. `certify_memory_store()` is the aggregate
source-backed certification runner: it runs `run_all_vectors`, then every runner named by
`behavior::EXECUTABLE_BEHAVIOR_SUITES`, plus capability registry behavior. Declarative-only behavior
suites are reported as inventory and are never counted as passed.

## 1. Certification Rule

An implementation may claim support only for behavior that is proven by executable tests.

A certification claim must name:

- the implementation under test;
- the crate, binding, provider, or protocol surface tested;
- the identity profile;
- the vector or behavior runner set;
- the exact version of the suite;
- any skipped tests and the reason they are skipped.

An implementation is conformant for a surface only when it passes every executable test tagged for
that surface and profile. A planned suite, prose scenario, or target contract is not certification.

## 2. Conformance Levels

Levels are cumulative. They describe product maturity, not file layout or marketing status.

| Level | Name | Current gate |
| --- | --- | --- |
| L0 | Object identity and storage | Provider round-trips canonical objects and passes blob plus object-model vectors for the declared identity profile. |
| L1 | Engine versioning | L0 plus executable tests for workspace refs, commits, branches, tags, checkout, merge behavior, and source-backed facet state that is versioned through the engine. |
| L2 | Synchronization | L1 plus executable tests for source-backed sync: direct workspace clone, fast-forward branch push, bundle export/import, workspace-id preservation, object verification, and non-fast-forward rejection. Live transports are not part of current L2. |
| L3 | Hardened profile | L2 plus an explicit list of security and lifecycle capabilities, each with its own executable conformance suite. There is no bare L3 claim. |

Current source does not yet publish a machine-readable conformance report assigning these levels to
every provider or binding. Until that report exists, claims should be written as specific vector or
runner results rather than blanket level claims.

## 3. Executable Suite

### 3.1 Canonical vectors

The conformance crate pins:

- `BLOB_VECTORS`;
- `run_blob_vectors`;
- `run_blob_vectors_profiled`;
- `run_object_model_vectors`;
- `run_object_model_vectors_profiled`;
- `LEDGER_HEAD_VECTORS`;
- `run_ledger_head_vectors_profiled`;
- `run_columnar_manifest_vectors`;
- `run_table_identity_vectors`;
- `run_all_vectors`;
- `CANONICAL_VECTOR_SUITES`.

These vectors certify canonical bytes, digest computation, store round trips, ledger chain heads, and
table plus columnar manifest identity for the source-backed cases they cover.

### 3.2 Aggregate certification

`certify_memory_store()` is the current aggregate runner for the in-memory engine. It returns a
`ConformanceSummary` with:

- vector suites passed;
- executable behavior suites passed;
- total scenario inventory count;
- data-only suites that have scenario inventory but no runner.

The summary is intentionally not a public machine-readable certification report. It is the current
source-backed proof summary inside the conformance crate.

### 3.3 Behavioral runners

The executable behavior runners are:

- `behavior::run_cas_behavior`;
- `behavior::run_cas_facade_behavior`;
- `behavior::run_workspace_behavior`;
- `behavior::run_sync_behavior`;
- `behavior::run_queue_behavior`;
- `behavior::run_consumer_behavior`;
- `behavior::run_diff_commits_behavior`;
- `behavior::run_lock_behavior`;
- `behavior::run_identity_behavior`;
- `behavior::run_acl_behavior`;
- `behavior::run_kv_facade_behavior`;
- `behavior::run_ephemeral_kv_behavior`;
- `behavior::run_document_facade_behavior`;
- `behavior::run_timeseries_facade_behavior`;
- `behavior::run_metrics_behavior`;
- `behavior::run_ledger_facade_behavior`;
- `behavior::run_graph_facade_behavior`;
- `behavior::run_vector_facade_behavior`;
- `behavior::run_columnar_facade_behavior`;
- `behavior::run_dataframe_facade_behavior`;
- `behavior::run_search_facade_behavior`;
- `behavior::run_calendar_facade_behavior`;
- `behavior::run_contacts_facade_behavior`;
- `behavior::run_mail_facade_behavior`;
- `behavior::run_inference_behavior`;
- `behavior::run_merge_conflict_behavior`;
- `behavior::run_staging_behavior`;
- `behavior::run_file_ops_behavior`;
- `behavior::run_file_handle_behavior`;
- `behavior::run_symlink_behavior`;
- `behavior::run_tags_behavior`;
- `behavior::run_restore_behavior`;
- `behavior::run_replay_behavior`;
- `behavior::run_squash_behavior`;
- `behavior::run_capability_behavior`.

`behavior::EXECUTABLE_BEHAVIOR_SUITES` currently lists:

- `cas`;
- `cas-facade`;
- `workspace`;
- `sync`;
- `queue`;
- `queue-consumer`;
- `vcs-diff`;
- `cas-facade`;
- `lock`;
- `identity`;
- `acl`;
- `kv`;
- `kv-ephemeral`;
- `document`;
- `time-series`;
- `metrics`;
- `ledger`;
- `graph`;
- `vector`;
- `columnar`;
- `dataframe`;
- `search`;
- `calendar`;
- `contacts`;
- `mail`;
- `inference`;
- `merge-conflict`;
- `staging`;
- `file-ops`;
- `file-handle`;
- `symlink`;
- `tags`;
- `restore`;
- `replay`;
- `squash`.

`behavior::BEHAVIOR_SUITES` also lists declarative suites whose runners are not yet wired to
source-backed APIs. Scenario tables without runners are not certification.

### 3.4 Binding and SQL result vectors

`bindings/conformance/result-vectors.json` pins cross-language SQL result payload shapes. Binding
tests consume that fixture where the binding is wired.

`loom_sql::CONFORMANCE_COMMIT` and `loom_sql::conformance_commit_digest` pin a deterministic
SQL-over-Loom commit digest.

`BINDING_CONFORMANCE_INVENTORY` records checked-in binding evidence by tier:

- `ExecutableCore`: canonical vectors, C ABI tests, and result codec evidence run through the Rust
  workspace gate;
- `BindingRuntimeSuite`: checked-in Node, Python, iOS, C++, JVM, Android, React Native, and WASM
  runtime suites that run through their own binding recipes;
- `ImplementedNotGated`: implemented binding surfaces with source and build recipes but no checked-in
  runtime suite;
- `TargetOnly`: generated IDL bindings, distribution packaging, cross-binding interop, and full
  binding certification.

Runtime coverage labels are explicit inventory, not pass/fail certification by this aggregate report.
The auth-related labels distinguish identity/ACL administration, role-ACL management, session
authentication, authenticated SQL sessions, and authenticated ordinary facet operations. Hosted
protocol auth remains target work until 0008 promotes a hosted surface with its own runner.

### 3.5 Provider coverage

The `FileStore` tests run `uldren_loom_conformance::run_all_vectors` and profile-specific blob and
object-model vectors. This proves the current provider against those vectors. It does not by itself
certify live sync, every behavior scenario, or every binding surface.

## 4. Target Suite

0010a owns the enterprise conformance expansion. Target coverage includes:

- crash-recovery and corruption fixtures for `.loom` files;
- provider lifecycle suites for compaction, GC, encrypted open, rekey, reseal, and browser backing;
- live protocol conformance after 0008 is implemented;
- cross-binding vector execution for every shipped binding;
- executable behavior runners for files, VCS, SQL, graph, vector, ledger, KV, document, time-series,
  metrics, columnar, and public facade CAS projections;
- remaining hosted authorization and principal-aware behavior after 0008 and 0026-0028 promote the
  served surfaces;
- execution determinism after 0015 is promoted from prototype to source-backed public API;
- fuzzing harnesses for canonical decoders and the single-file format;
- machine-readable conformance reports emitted by CI.

## 5. Capability Registry

Capability names are a managed workspace. A capability entry records:

- name;
- version pair, `current` and `minimum_compatible`;
- owning spec;
- proof status;
- parameters schema, when needed.

Current proof status values:

- `executable`: source-backed API and executable conformance coverage exist;
- `source-backed`: source-backed API exists, but shared conformance coverage is incomplete;
- `scenario`: scenario text exists, but no executable runner is available;
- `target`: planned contract, not source-backed;
- `deprecated`: retained only for compatibility or history.

Current registry:

| Capability | Version | Spec | Proof status |
| --- | --- | --- | --- |
| `object-store` | 1/1 | 0002, 0004 | executable |
| `identity-profile-blake3` | 1/1 | 0002 | executable |
| `identity-profile-sha256` | 1/1 | 0002, 0009 | executable |
| `blob-vectors` | 1/1 | 0002 | executable |
| `object-model-vectors` | 1/1 | 0002 | executable |
| `ledger-head-vectors` | 1/1 | 0018 | executable |
| `table-identity-vectors` | 1/1 | 0011 | executable |
| `columnar-manifest-vectors` | 1/1 | 0023 | executable |
| `cas` | 1/1 | 0024 | executable |
| `workspace` | 1/1 | 0014 | executable |
| `bundle` | 1/1 | 0006 | executable |
| `direct-workspace-clone` | 1/1 | 0006 | executable |
| `fast-forward-branch-push` | 1/1 | 0006 | executable |
| `queue` | 1/1 | 0021 | executable |
| `queue-consumer` | 1/1 | 0021b | executable |
| `lanes` | 1/1 | 0067 | source-backed |
| `single-file-store` | 1/1 | 0005 | source-backed |
| `sql` | 1/1 | 0011 | source-backed |
| `compression` | 1/1 | 0005, 0009 | source-backed |
| `encryption-at-rest` | 1/1 | 0005, 0009 | source-backed |
| `rekey` | 1/1 | 0009 | source-backed |
| `files` | 1/1 | 0003, 0014 | executable |
| `vcs` | 1/1 | 0003, 0014 | executable |
| `kv` | 1/1 | 0019 | executable |
| `document` | 1/1 | 0020 | executable |
| `graph` | 1/1 | 0016 | executable |
| `vector` | 1/1 | 0017 | executable |
| `ledger` | 1/1 | 0018 | executable |
| `time-series` | 1/1 | 0022 | executable |
| `metrics` | 1/1 | 0046 | executable |
| `logs` | 1/1 | 0047 | executable |
| `traces` | 1/1 | 0048 | executable |
| `columnar` | 1/1 | 0023 | executable |
| `columnar-arrow-ipc` | 1/1 | 0023 | source-backed |
| `columnar-parquet` | 1/1 | 0023 | source-backed |
| `search` | 1/1 | 0033 | executable |
| `calendar` | 1/1 | 0037 | executable |
| `contacts` | 1/1 | 0038 | executable |
| `mail` | 1/1 | 0039 | executable |
| `dataframe` | 1/1 | 0045 | executable |
| `dataframe-arrow-ipc` | 1/1 | 0045 | source-backed |
| `dataframe-parquet` | 1/1 | 0045 | source-backed |
| `dataframe-sql-result` | 1/1 | 0045, 0011 | source-backed |
| `dataframe-polars` | 1/1 | 0045 | source-backed |
| `mcp-host` | 1/1 | 0008 | source-backed |
| `mcp-apps` | 1/1 | 0043 | source-backed |
| `watch` | 1/1 | 0030 | executable |
| `delivery` | 1/1 | 0035 | executable |
| `optional-runtime-config` | 1/1 | 0067 | source-backed |
| `runtime-fuse-activation` | 1/1 | 0003c, P9-0017 | source-backed |
| `runtime-tor-activation` | 1/1 | _FACET_PRIMITIVES | source-backed |
| `runtime-ipfs-activation` | 1/1 | _FACET_PRIMITIVES | source-backed |
| `runtime-heavy-engine-activation` | 1/1 | _FACET_PRIMITIVES | source-backed |
| `live-sync-transport` | 1/1 | 0008 | target |
| `inference` | 1/1 | 0043 | executable |
| `providers.embedding` | 1/1 | 0050 | executable |
| `set-reconciliation` | 1/1 | 0006 | target |
| `delta-transfer` | 1/1 | 0006 | target |
| `partial-clone` | 1/1 | 0006, 0009 | target |
| `shallow-clone` | 1/1 | 0006 | target |
| `import-fs` | 1/1 | 0012 | target |
| `export-fs` | 1/1 | 0012 | target |
| `import-sql` | 1/1 | 0012 | target |
| `export-sql` | 1/1 | 0012 | target |
| `exec` | 1/1 | 0015 | target |
| `trigger` | 1/1 | 0029 | executable |
| `lock` | 1/1 | 0036 | target |
| `identity` | 1/1 | 0026 | target |
| `acl` | 1/1 | 0027 | target |
| `acl-fine` | 1/1 | 0028 | target |
| `e2e-sync` | 1/1 | 0031 | target |
| `audit` | 1/1 | 0009 | target |
| `retention` | 1/1 | 0009 | target |
| `redact` | 1/1 | 0009 | target |
| `digest-migration` | 1/1 | 0002 | target |

### 5.1 Target two-axis capability record

The capability registry declares what Loom has specified and proved. A capability report states whether that declared capability can run for a selected scope. These are independent questions and MUST remain separate fields.

Every reported capability record MUST contain `capability_id`, `current`, `minimum_compatible`, `owning_specs`, `owner_module`, `dimensions`, `proof_status`, `operational_state`, a scope selector, declared profiles, applicable limits, and evidence references. A record whose state is not `supported` MUST include `reason_code`. A record for which a request cannot proceed MUST include `stable_error`. Runtime-observed records MUST include observation time and freshness.

`proof_status` is a registry lifecycle value. It answers whether Loom has a source-backed and conformance-backed basis for a capability. Its values are `executable`, `source-backed`, `scenario`, `target`, and `deprecated` as defined above. Runtime observation, configuration, and a successful request MUST NOT promote proof status.

`operational_state` is a scoped runtime value. It answers whether the selected capability and profile can run in this binary, platform, configuration, runtime, listener, and caller context.

| State | Predicate |
| --- | --- |
| `supported` | The capability and selected profile are declared, compiled for the selected platform, explicitly enabled, runtime-ready, within limits, and permitted for the selected caller and scope. |
| `degraded` | A declared fallback can run, but its result-equivalence, freshness, exactness, ordering, or resource boundary differs from the normal profile and is disclosed in the record. |
| `disabled` | The capability is declared and compiled but intentionally inactive for the selected configuration or listener. |
| `unavailable` | The capability is declared but cannot currently run because a compiled feature, platform facility, dependency, credential, endpoint, library, service, health condition, readiness condition, bind, maintenance window, capacity bound, or recovery condition prevents use. |
| `denied` | The capability could otherwise run, but the policy-enforcement point rejects the selected caller or scope. |
| `unsupported` | The capability or selected profile is not declared for the selected surface. |
| `target` | The capability is part of the target design but is not source-backed for the selected surface. |

The states are mutually exclusive for one record scope. `reason_code` supplies the subcause without creating operational-state aliases. The initial registry MUST include `configured_disabled`, `listener_disabled`, `feature_not_compiled`, `runtime_dependency_absent`, `policy_denied`, `index_rebuilding`, and `profile_unsupported`. New reason codes require a backward-compatible 0010 specification update. Consumers MUST preserve unrecognized reason codes and use `operational_state` for control flow.

The initial registry left two non-`supported` situations without a subcause: the `target` state, and the transient conditions the `unavailable` predicate enumerates. The following codes are added as backward-compatible registry extensions and MUST be included. `not_source_backed` is the reason code for `target`, a capability declared in the target design with no source-backed surface, and maps to stable error `UNSUPPORTED`. For transient `unavailable` subcauses the registry adds `listener_bind_failed` (bind failure), `service_unavailable` (health or readiness failure), `maintenance_active` (maintenance window), `capacity_exhausted` (capacity bound), `recovery_in_progress` (recovery condition), and `probe_stale` (expired freshness probe). Each transient code maps to stable error `UNAVAILABLE`, except `capacity_exhausted`, which maps to the exact existing Code `RESOURCE_EXHAUSTED`. These codes remain subcauses only: `operational_state` is still the sole control-flow axis, `reason_code` MUST NOT alias a state, and consumers MUST continue to branch on `operational_state` and preserve unrecognized reason codes. The non-registry spellings `target_only`, `runtime_feature_not_compiled`, and `runtime_not_enabled` are deprecated aliases of `not_source_backed`, `feature_not_compiled`, and `configured_disabled` respectively; producers MUST emit the registry code and MUST NOT introduce state-name reason codes such as `degraded` or `unavailable`. Because the registry defines no generic `degraded` or `unavailable` subcause, a `degraded` or `unavailable` record MUST carry an explicit registry subcause reason code (for example `index_rebuilding`, `feature_not_compiled`, or `runtime_dependency_absent`); there is no default reason code for these two states.

`stable_error` identifies the stable Loom `Code` token returned when the requested operation cannot proceed. The token uses the SCREAMING_SNAKE_CASE spelling returned by `Code::as_str`, matching the remote protocol error contract. It is required for `unavailable`, `denied`, `unsupported`, and `target`. It is optional for `disabled` and `degraded`: a disabled capability may be queried without attempting use, and a degraded capability may execute successfully. Public projections MUST NOT expose a reason code, probe detail, or error diagnostic that reveals protected configuration, credentials, network topology, or policy information to an unauthorized caller.

The scope selector MUST identify the producer and the applicable subset of binary, platform profile, native facet or facade, optional engine, hosted listener, transport, requested compatibility profile, and request context. A principal-sensitive projection MAY omit or redact caller-specific fields, but it MUST NOT change a global availability condition into `denied` or erase a `denied` result for that caller.

The evaluator applies checks in this order:

1. Verify declaration of the capability and requested profile. A missing declaration is `unsupported`.
2. Verify compiled feature and platform support. A missing feature is `unavailable` with reason code `feature_not_compiled`.
3. Verify explicit configuration and listener enablement. An inactive declared feature is `disabled`.
4. Run required runtime probes. A missing dependency is `unavailable` with reason code `runtime_dependency_absent`; transient readiness, health, capacity, bind, or recovery failure is `unavailable`.
5. Evaluate policy for the selected caller and scope. A rejected request is `denied`.
6. Evaluate a declared fallback. A usable non-equivalent fallback is `degraded`; otherwise the capability is `unavailable`.
7. Emit `supported` only when every preceding predicate passes.

The registry contributes declarations. Facets, facades, engines, bindings, and hosted listeners contribute scoped operational observations. Aggregation MUST retain records with distinct scopes and MUST NOT collapse them to a single boolean. Queries filter records by capability identity and scope and return all matches in canonical selector order. Synchronization negotiation uses declared version pairs and proof eligibility, not transient operational state. A peer may select an operationally supported common profile only after version negotiation succeeds.

The canonical CBOR representation is an array of capability-record maps sorted by `capability_id`, then canonical scope selector bytes. Map keys are text keys and omitted optional fields are absent rather than encoded as null. JSON is a lossless projection using the lowercase tokens in this section. `CapabilitySet` encoding and decoding MUST reject duplicate `capability_id` plus scope selectors, unknown required enum values, a non-`supported` record without `reason_code`, a required-error state without `stable_error`, and a `degraded` record without a degradation boundary.

The following target examples define the required shape without claiming that their runtimes are source-backed.

```json
[
  {"capability_id":"files","current":1,"minimum_compatible":1,"owning_specs":["0003","0014"],"owner_module":"loom-core::capability","dimensions":{"facet":"files"},"proof_status":"executable","operational_state":"supported","scope":{"surface_kind":"facet","surface_id":"files","platform_profile":"core-cli-desktop"},"profiles":["native-v1"],"limits":{"max_request_bytes":1048576},"evidence":["behavior::run_file_ops_behavior"]},
  {"capability_id":"search","current":1,"minimum_compatible":1,"owning_specs":["0033"],"owner_module":"loom-core::capability","dimensions":{"engine":"fts"},"proof_status":"executable","operational_state":"degraded","reason_code":"index_rebuilding","scope":{"surface_kind":"engine","surface_id":"fts","engine":"portable-scan"},"profiles":["native-v1"],"degradation":{"fallback":"portable-scan","result_equivalence":"bounded scan only"},"evidence":["source digest and engine stamp"]},
  {"capability_id":"mcp-host","current":1,"minimum_compatible":1,"owning_specs":["0008"],"owner_module":"loom-core::capability","dimensions":{"listener":"mcp","transport":"streamable-http"},"proof_status":"source-backed","operational_state":"unavailable","reason_code":"runtime_dependency_absent","stable_error":"UNAVAILABLE","scope":{"surface_kind":"listener","surface_id":"mcp","listener_id":"local-mcp"},"profiles":["streamable-http"]},
  {"capability_id":"neo4j","current":1,"minimum_compatible":1,"owning_specs":["0016"],"owner_module":"loom-core::capability","dimensions":{"facade":"neo4j","transport":"bolt"},"proof_status":"source-backed","operational_state":"unsupported","reason_code":"profile_unsupported","stable_error":"UNSUPPORTED","scope":{"surface_kind":"facade","surface_id":"neo4j","transport":"bolt"},"profiles":["bolt-5.1-read"]}
]
```

### 5.2 Current source boundary and migration

The current source-backed `CapabilitySet` is a declaration catalog plus source-owned operational
records. Its implemented projections are the shared CLI JSON/text, MCP JSON/CBOR, hosted admin JSON,
and C ABI canonical-CBOR paths. Migration proceeds by replacing remaining name-only lookup and
overlays with scoped selector-aware queries, expanding binding projections that still rely on
follow-on work, then adding canonical, negative, transition, cross-language, platform-profile, and
hosted protocol conformance.

Adding an optional field, reason code, limit key, profile, or scope selector is a backward-compatible minor capability-record schema change when unknown values can be preserved. Changing an operational-state predicate, removing a state or reason code, changing canonical ordering, or changing a stable-error meaning is a breaking major schema change. Target and scenario declarations remain non-negotiable for peer interoperability until their proof status advances.

The `watch` capability is executable through the engine facade. The reusable contract types,
canonical batch encoding, cursor codec, file-domain record shape, and domain-support helpers live in
`loom-watch`; the aggregate conformance runner remains in `loom-conformance` because it proves
workspace history, ACL filtering, and engine materialization through `loom-core`.

The `delivery` capability is executable through the engine facade. The reusable envelope, canonical
envelope codec, replay message shape, and produce request contract live in `loom-delivery`; the
aggregate conformance runner remains in `loom-conformance` because it proves CAS-backed payload
storage, queue-backed sequence assignment, subscriber ack mutation, ACL enforcement, and replay
behavior through `loom-core`.

The `trigger` capability is executable through the shared keeper and compute substrate, but the public
facade projection remains target. `loom-triggers` is source-backed for reusable binding and fire-record
contract codecs plus croner-backed time evaluation. `loom-core` is source-backed for reserved binding
storage, fire-log append/history, and due fire planning. `loom-compute` is source-backed for
run-as-aware trigger execution, canonical stimulus inputs, overlap policy handling, and fire-record
outcome classification. `loom-conformance` proves the PIM trigger bridge and overlap behavior.

Promotion from `scenario` or `target` to `source-backed` requires source API. Promotion to
`executable` requires shared conformance coverage.

This table is source-backed at runtime: `loom_core::capability` encodes it as source-owned registries
for facets, facades, engines, transports, compile features, listeners, bindings, and policies, and
exposes the current `Loom::capabilities() -> CapabilitySet` declaration report (query, version
negotiation, and operational-state overlays).
A drift test parses this section and asserts the source table matches it exactly, so the two cannot
diverge. The catalog carries the declared contract (version pair, owning spec, proof status),
source owner, dimensions, structured reason code, stable error, and current per-build operational
state. `loom-core` asserts support for the capabilities it implements and downstream layers overlay
states for capabilities they own. The C ABI (`loom_capabilities`), the IDL (`Store.capabilities`),
and the C header are source-backed, as are the Node, Python, C++, Swift/iOS, and WASM binding
projections; the JVM, Android, and React Native binding projections are follow-on work. Embedding
capability metadata in the `.loom` file (0005a) and the hosted capability report (0004a, 0008)
remain target.

Calendar, contacts, and mail capability rows are engine-facade capabilities. Their reusable record and
wire projection contracts live in `loom-pim`; their executable conformance remains in
`loom-conformance` because it proves workspace storage, ACL scope, CAS body closure, commit/checkout,
clone reachability, and facade behavior through `loom-core`.

## Change log

### behavior.rs runner decomposition (task 231 / 217h)

`crates/loom-conformance/src/behavior.rs` went from 2,817 lines to 52 with **no behavior change** - it
now holds only the engine imports, the `Scenario` struct, and the `mod scenarios` + runner-module
wiring. The ~30 `run_*_behavior` runners moved into four per-area modules under `behavior/`: `engine`
(cas/kv/document/timeseries/ledger/ephemeral), `pim` (calendar/contacts/mail), `vcs` (merge/staging/
files/symlink/tags/restore/replay/squash/diff + the private `diff_*` helpers), and `admin` (lock/
identity/acl/workspace/sync/queue/consumer/capability); the inline test module moved to
`behavior/tests.rs`. Each module does `use super::*` (pulling `Scenario` + the engine imports); the
runners stay `pub` and are re-exported from `behavior.rs`, so `behavior::run_*` paths are unchanged.
Verified lossless (runner/test bodies byte-identical).

### Source-backed capability reporting (0004 section 4, 0010 section 5)

- **Core.** `loom_core::capability` is the single source of truth: source-owned registries mirror the
  0010 section 5 table (name, `current`/`minimum_compatible`, owning spec, proof status) and attach
  owner modules, dimensions, operational state, reason codes, and stable errors; `CapabilitySet` with
  `get`/`supports`/`iter`, `negotiate` (peer intersection for 0006 sync), `with_state`/`with_overlay`
  (the capability-contribution overlay), and `to_cbor` (canonical CBOR via `loom-codec`);
  `Loom::capabilities()`.
- **Contribution pattern.** `loom-store` and `loom-sql` each expose `provided_capabilities()`; the
  assembling layer overlays them, so `loom-core` asserts support only for what it implements and never
  depends on its dependents. `single-file-store`, `compression`, `encryption-at-rest`, `rekey` (store)
  and `sql` (loom-sql) are reported supported only when those crates are linked.
- **Drift guard.** A test parses the 0010 section 5 markdown table and asserts it matches the source
  registries exactly (names, version pairs, owning specs, proof statuses).
- **Cross-language.** C ABI `loom_capabilities` (handle-free, build-aware overlay) returning canonical
  CBOR; `idl/loom.idl` `Store.capabilities`; `include/loom.h` (+ iOS header copy). Binding projections:
  Node (`capabilities()` + `index.d.ts` + smoke test), Python (`capabilities()` + `.pyi` + decode test,
  exported via `__init__`), C++ (`loom::capabilities()`), Swift/iOS (`Loom.capabilities()`), WASM
  (`capabilities()`). JVM, Android, and React Native projections are pending.
- **Conformance.** `run_capability_behavior` is part of `certify_memory_store`; an FFI test decodes the
  buffer and asserts the overlay (sql / single-file-store supported, target capabilities not).
- **Docs.** 0004 section 4, 0004a, and 0005a updated to point at the source-backed facade; the
  remaining hosted/embedded capability surfaces stay target.

### MCP host capability (0008 task 193)

- Added `mcp-host` (owning spec 0008, proof `source-backed`) to the section 5 table and the
  `loom_core::capability` source registries (the drift guard keeps them identical). It is the
  first 0008 hosted-surface capability promoted off `target`: the loom-mcp host and its behavioral tests
  (0008 tasks 180-192) are source-backed, while the shared `certify_memory_store` suite does not yet run
  an MCP backend, so it stops at `source-backed` rather than `executable`. loom-core declares it
  unsupported (it does not serve MCP); the loom-mcp crate owns and contributes it via
  `served_capabilities()` (the `with_state` overlay), the same contribution pattern store/sql use.
- Added `mcp-apps` (owning spec 0043, proof `source-backed`) as a separate capability from base
  `mcp-host`. `loom-mcp` overlays both as supported and returns the overlaid set from MCP
  `store_capabilities`; host-level conformance vectors cover app listing, invalid candidate status,
  `resources/read` metadata, and app authoring. It remains `source-backed` rather than `executable`
  because the shared `loom-conformance` crate is engine-level and does not run MCP protocol backends.

## 6. Versioning

Three surfaces are versioned independently:

- spec series version;
- on-disk format version;
- API, ABI, and protocol version.

The current workspace crate version is pre-release. It does not yet carry the full compatibility
promise expected from a stable major version.

### 6.1 Spec series

A breaking normative change bumps the major version. A backward-compatible addition bumps the minor
version. A clarification bumps the patch version.

Before the first stable major version, specifications remain movable. Current conformance vectors
still pin source behavior and must be updated deliberately when identity-affecting behavior changes.
The current vector inventory includes `lock-fence`, which pins structured fence authority, epoch,
sequence, and ordered limb packing for the public lock contract.

### 6.2 On-disk format

The single-file store uses `format_major.format_minor`.

Current behavior:

- unknown `format_major` is unreadable;
- `format_minor` is currently written as `0`;
- unknown digest algorithms are unreadable;
- bundle format v4 is the current source-backed offline-transfer frame.

Unknown-field preservation for later minor versions is a target compatibility requirement, not a fully
exercised current conformance suite.

### 6.3 API, ABI, and protocol

The C ABI, IDL, language bindings, and wire protocols must each declare the version range they support.
Current source exposes a broad ABI and selected binding projections, but generated IDL projection,
hosted protocol version declarations, and full binding certification remain target work.

### 6.4 Capability compatibility

Each capability carries:

- `current`;
- `minimum_compatible`.

Two peers may use a capability when each peer's `current` is greater than or equal to the other peer's
`minimum_compatible`. The effective version is the lower `current`.

Experimental, target, and scenario-only capabilities carry no interoperability promise.

The capability-record schema is independently versioned. A consumer MAY ignore an optional field only
when it preserves that field for relay or reserialization. A consumer MUST reject an unknown required
field or enum token rather than guessing a less restrictive operational state.

## 7. Resolved decisions

1. **Levels are cumulative.** L0 through L3 remain the maturity ladder, but current claims should name
   executable runners until machine-readable level reports exist.
2. **No bare hardened claim.** L3 always includes an explicit list of security and lifecycle
   capabilities.
3. **Executable proof wins.** Scenario text and target contracts do not certify behavior.
4. **Aggregate memory-store certification exists.** `certify_memory_store()` runs canonical vectors
   plus the executable behavior runners listed in section 3.3.
5. **Declarative scenarios are inventory.** Data-only scenario suites are reported for visibility and
   never as passed certification.
6. **Binding evidence is tiered.** Binding-adjacent evidence is categorized as `ExecutableCore`,
   `BindingRuntimeSuite`, `ImplementedNotGated`, or `TargetOnly`.
7. **Capability promotion is explicit.** Promotion to `source-backed` requires implementation.
   Promotion to `executable` requires conformance coverage.
8. **Version pairs are the compatibility unit.** Versioned surfaces and capabilities use
   `current/minimum_compatible`, not only a single number.
