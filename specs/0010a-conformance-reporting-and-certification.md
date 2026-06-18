# 0010a - Conformance Reporting and Certification

**Status:** Draft target extension. **Version:** 0.1.0. **Normative target.**

This document owns the enterprise reporting and certification work split out of 0010. It does not
change the current proof boundary: 0010 remains complete for the source-backed vectors, executable
behavior runners, and binding inventory that exist today.

## Current source boundary

Current source provides:

- canonical vector runners through `run_all_vectors`;
- aggregate memory-store certification through `certify_memory_store`;
- executable behavior runners for CAS, CAS facade, workspace, sync, queue, queue-consumer, delivery, lock,
  identity, ACL, KV, ephemeral KV, document, time-series, ledger, calendar, contacts, mail,
  SQL error codes, SQL historical readers and schema-aware table diff, merge-conflict, staging,
  file-ops, file-handle, symlink, tags, restore, replay, squash, and capability registry behavior;
- binding evidence tiers through `BINDING_CONFORMANCE_INVENTORY`;
- a Linux CI-gated C++ runtime suite that builds the release FFI library and runs
  `loom_cpp_runtime` through CTest. The serialized Rust certification report records it as skipped
  only because that report does not execute the binding toolchain itself;
- promoted-surface coverage labels for each checked-in binding runtime suite;
- source-backed runtime provider/profile reporting in native bindings through canonical CBOR;
- hosted protocol feature evidence through `HOSTED_PROTOCOL_FEATURES`, with supported, degraded,
  target, and unsupported status rows. Watch delivery is reported with `stream` and durable transport
  rows only through the owned 0008/0035 projection surfaces; replay-safe transport claims remain target
  until those slices are source-backed end-to-end. FTS rows distinguish native REST/JSON-RPC support,
  the bounded OpenSearch-compatible REST profile, supported exact bucket and metric aggregations, supported
  `match_all` and analyzer-boundary rows, supported aliases/multi-index/wildcard expansion, supported
  read-only security shims, unsupported security mutation APIs, native graph and ledger REST/JSON-RPC/gRPC
  boundaries, and target OpenSearch compatibility rows that remain unimplemented.
- a focused network-access conformance matrix for direct-peer allow and deny, trusted-proxy handling,
  missing-mTLS rejection, and denied-audit evidence. The matrix records gRPC's current direct-peer-only
  admission boundary instead of claiming HTTP proxy or mTLS parity.
- a source-backed derived-Tantivy lifecycle test that pins missing, rebuilding, ready, stale, failed,
  and unsupported artifact statuses against the source digest and engine-version stamp.
- local coordination feature evidence through `LOCAL_COORDINATION_FEATURES`, with supported,
  degraded, and unsupported rows for embedded locks, CLI daemon runtime, daemon transports,
  host-native lock clients, MCP attached-client authority checks, mobile/browser daemon-lock absence,
  and hosted lock protocol absence;
- canonical `lock-fence` vectors for structured embedded and external-authority fence packing,
  including ordered low/high limb reconstruction;
- hosted CAS REST and JSON-RPC operation conformance for put, get, missing get, has, list, and
  delete, invalid digest, and post-delete absence through the `hosted-cas-rest-jsonrpc` suite;
- hosted CAS gRPC operation conformance for put, get, missing get, has, list, and delete through
  the `hosted-cas-grpc` suite, including invalid digest and post-delete absence;
- hosted Queue gRPC operation conformance for append, get, range, and len through the
  `hosted-queue-grpc` suite;
- hosted Queue REST and JSON-RPC served-route operation conformance for append, get, range, and len
  through the `hosted-queue-rest` and `hosted-queue-jsonrpc` suites;
- hosted Time-series gRPC operation conformance for put, get, latest, and server-streaming range
  through the `hosted-timeseries-grpc` suite;
- hosted Time-series REST and JSON-RPC served-route operation conformance for put, get, latest, and
  range through the `hosted-timeseries-rest` and `hosted-timeseries-jsonrpc` suites;
- hosted Ledger REST and JSON-RPC served-route operation conformance for append, get, head, len, and
  verify through the `hosted-ledger-rest` and `hosted-ledger-jsonrpc` suites;
- hosted native FTS REST and JSON-RPC served-route operation conformance for create, index, get,
  query, ids, remap, and delete through the `hosted-fts-rest` and `hosted-fts-jsonrpc` suites;
- hosted Graph REST and JSON-RPC served-route operation conformance for the bounded native graph
  route set through the `hosted-graph-rest` and `hosted-graph-jsonrpc` suites;
- hosted Vector REST and JSON-RPC served-route operation conformance for create, upsert, get, and
  search through the `hosted-vector-rest` and `hosted-vector-jsonrpc` suites;
- hosted Columnar REST and JSON-RPC served-route operation conformance for create, append, scan,
  columns, rows, compact, inspect, source-digest, select, and aggregate through the
  `hosted-columnar-rest` and `hosted-columnar-jsonrpc` suites;
- hosted KV REST and JSON-RPC served-route operation conformance for put, get, delete, list, and
  range through the `hosted-kv-rest` and `hosted-kv-jsonrpc` suites;
- cross-surface capability matrix evidence through `CAPABILITY_MATRIX`, with local, MCP, hosted Tier-1,
  hosted Tier-2, binding, provider, supported, degraded, target, unsupported, transport, and profile
  rows;
- release, package, browser, device, and provider certification evidence through
  `RELEASE_CERTIFICATION_INVENTORY`, including release-material scripts, binding package material
  capture, browser-worker OPFS evidence, device runtime fixtures, provider profile reporting, and
  explicit target or unsupported release boundaries;
- a serialized conformance report over that boundary through `ConformanceReport`,
  `report_memory_store`, and `ConformanceReport::to_json`, with the `passed`/`failed`/`skipped`/
  `inventory`/`target` status taxonomy in `ReportStatus` and hosted protocol
  `supported`/`degraded`/`target`/`unsupported` rows;
- provider/profile evidence in `ConformanceReport.runtime_profile`, sourced from the linked
  `loom_core::runtime_profile` report.
- a source-backed PIM certification profile and transcript inventory fixture through
  `ConformanceReport.certification_profile` and `ConformanceReport.transcript_inventory`. The current
  profile is the owner-only enterprise gate aligned with 0065. It records required Apple,
  Thunderbird, DAVx5, and JMAP executable transcript targets, plus transcript redaction and retention
  policy. Queue 7 owner verification has accepted Apple, Thunderbird, and DAVx5 CalDAV/CardDAV/IMAP
  evidence for the bounded owner-only profile, and owner-accepted JMAP closure relies on the hosted RFC
  8620/8621 executable router transcript because no external JMAP client/tool is available in the
  certification environment. Durable transcript storage, admin review, native JMAP client/tool
  certification, and profile selection remain 0065 target work.

The current `acl` executable runner proves local engine authorization for default-deny,
deny-precedence, selected PEP hooks, role-grant expansion, immediate role revocation, ref-scoped
grants, prefix-scoped grants, and broad-resource isolation. The current `sync` executable runner also
proves authenticated push fails without source read and destination advance rights, then succeeds
after source read plus destination write and advance grants exist. This is source-backed local auth
coverage. Hosted protocol auth certification is currently bounded to the CAS REST/JSON-RPC/gRPC
auth/ACL matrix; served-handle re-checks, ordinary read/list omission, protected-write rejection, and
additional promoted facets remain target certification.

Current source does not provide:

- (P1) full hosted protocol conformance beyond the hosted PIM feature evidence matrix, the
  source-backed CAS REST/JSON-RPC operation suite, the source-backed CAS gRPC operation suite, the
  source-backed Queue REST/JSON-RPC/gRPC operation suites, the source-backed Time-series
  REST/JSON-RPC/gRPC operation suites, the source-backed Ledger REST/JSON-RPC operation suites, and
  the source-backed native FTS REST/JSON-RPC operation suites, the source-backed Graph
  REST/JSON-RPC operation suites, the source-backed Vector REST/JSON-RPC operation suites, the
  source-backed Columnar REST/JSON-RPC operation suites, and the source-backed KV REST/JSON-RPC
  operation suites;
- (P0) generated capability reports;
- (P2) crash or fuzz dashboards;
- (P1) full per-binding runtime certification;
- (P2) cross-binding interoperability certification.

### Report schema (source-backed)

`report_memory_store` serializes only what `certify_memory_store` proves. Canonical vector suites,
including `lock-fence`, and the executable behavior runners report `passed`; declarative-only behavior
suites report `inventory`. Binding
surfaces carry their `BINDING_CONFORMANCE_INVENTORY` tier: the canonical-vectors core suite reports
`passed`; the C ABI and result-codec core suites and the binding runtime suites report `skipped` with a
reason, because this certification does not execute them; implemented-but-ungated bindings report
`inventory`; target surfaces report `target`. Binding runtime suite rows also serialize the promoted
surface labels that their checked-in tests exercise, so a skipped Node/Python/iOS/C++/JVM/Android/WASM
runtime suite is still auditable for scope. Auth-related runtime labels distinguish
identity/ACL-administration evidence, role-ACL management evidence, session-auth evidence,
authenticated SQL-session evidence, and authenticated ordinary facet-operation evidence. These labels
are binding evidence inventory; they are not hosted auth certification. The report serializes hosted
protocol feature evidence for supported, degraded, target, and unsupported behavior, but it does not
claim full hosted protocol, full binding, provider lifecycle, or reference-client certification.
The report also serializes local coordination evidence in `local_coordination`. That section
distinguishes embedded lock coordinator behavior, CLI daemon runtime, degraded TCP loopback, supported
host-native lock clients, MCP attached-session liveness checks, unsupported hosted lock protocol, and
unsupported mobile/browser daemon-lock clients. It is evidence inventory, not a promotion of the
target public `lock` capability in the static registry.
The report also serializes `capability_matrix`. That section gives release tooling and reviewers a
flat status matrix for local, MCP, hosted Tier-1, hosted Tier-2, binding, and provider planes. It
records surface, transport, optional profile, supported/degraded/target/unsupported status, and
source-backed evidence paths. It is an inventory and drift guard, not a generated capability resource.
The report also serializes the linked runtime provider profile: binary channel, runtime policy,
default identity profile, crypto provider, TLS provider, FIPS capability, and FIPS TLS claim. This is
provider evidence for the artifact that produced the report; it is not a packaging, SBOM, or external
certification claim.
The report also serializes `release_certification`. That section records source-backed release-material,
binding-package-material, browser-runtime, device-runtime, and provider-profile evidence. It distinguishes
source-backed evidence capture from target registry publishing, artifact signing, install validation,
native binding FIPS publication, and unsupported WASM native-FIPS claims.
The report also serializes the PIM certification profile under the 0065
`admin.certification.profile` key and a redacted transcript inventory. The profile uses
`pim-owner-only-enterprise-v1` and redaction policy `pim-owner-only-redacted-transcripts-v1`. Redaction
removes credentials, cookies, tokens, secrets, private keys, and raw PIM payload bodies while retaining
method, path template, status, capability, ETag/sync/state token presence, UID shape, and error code
evidence. This is fixture shape and required-client inventory, not completed client certification.
Queue 7 closes the bounded PIM reference-client gate by owner verification for Apple,
Thunderbird, and DAVx5 CalDAV/CardDAV/IMAP evidence plus owner-accepted JMAP executable transcript
evidence. The serialized report still treats the PIM transcript inventory as durable fixture shape
until 0065 stores reviewed transcripts and certification decisions.

`ConformanceReport.source_revision` is injected at compile time from the optional `LOOM_SOURCE_REVISION`
environment variable (a build may set it to a git revision or release tag) and normalized by
`resolve_source_revision`, which trims the value and keeps `None` when it is unset or blank, so the report
never serializes a fabricated revision. CI (`.github/workflows/ci.yml`) and the release workflow
(`.github/workflows/release-plz.yml`) set `LOOM_SOURCE_REVISION` to the commit SHA (`${{ github.sha }}`)
at the workflow level, so conformance and release-certification builds carry the real revision; cargo
fingerprints `option_env!` variables, so a changed SHA recompiles only the affected crate. Builds that do
not set the variable continue to report `null`.

## Target report model

A conformance report should be machine-readable and stable enough for CI, release artifacts, hosted
provider claims, and customer audits.

Each report entry records:

- (P0) implementation name and version;
- (P0) source revision or release tag;
- (P0) provider, binding, protocol, or hosted surface under test;
- (P0) identity profile;
- (P0) capability name and version pair;
- (P0) vector suites run;
- (P0) behavior runners run;
- (P0) skipped suites and reasons;
- (P0) binding runtime coverage labels for skipped checked-in runtime suites;
- (P0) result status;
- (P1) platform, target triple, and toolchain version where platform behavior matters.

The report must distinguish:

- (P0) `passed`: executable suite ran and passed;
- (P0) `failed`: executable suite ran and failed;
- (P0) `skipped`: executable suite exists but did not run, with a reason;
- (P0) `inventory`: declarative scenario exists but no executable runner exists;
- (P0) `target`: planned capability has no source-backed suite.

## Target certification tracks

### Provider lifecycle

Provider certification should cover:

- (P0) canonical vector suites for every claimed identity profile;
- (P0) store reopen behavior;
- (P0) crash recovery and corruption fixtures;
- (P1) GC and compaction;
- (P0) encrypted open, wrong credential, rekey, reseal, and multi-wrap behavior where supported;
- (P1) browser and native backing equivalence where a provider claims platform parity.

### Binding runtime

Binding certification should cover every shipped binding through a checked-in runtime suite:

- (P0) workspace lifecycle;
- (P0) identity and ACL administration where exposed;
- (P0) session authentication and authenticated ordinary operations where exposed;
- (P0) SQL result vectors and SQL error-code behavior;
- (P0) authenticated SQL sessions where exposed;
- (P1) queue and consumer offsets where exposed;
- (P1) CAS where exposed;
- (P0) key-source wrappers where exposed;
- (P1) direct table and history readers where exposed;
- (P0) error-code preservation;
- (P0) ownership and freeing rules;
- (P2) cross-binding open/read interoperability.

Bindings without a runtime suite remain `ImplementedNotGated` even when they compile.

### Hosted protocols

Protocol certification starts only after 0008 promotes a source-backed hosted surface. It should cover:

- (P0) feature evidence rows for supported, degraded, target, and unsupported behavior;
- (P0) RFC implementation-gate rows for promoted standards-backed surfaces;
- (P1) REST, JSON-RPC, gRPC, WebSocket, MCP, or other promoted transports;
- (P0) stable error mapping;
- (P0) idempotency and retry behavior;
- (P0) auth propagation after 0026-0028;
- (P1) streaming backpressure and durable delivery where a transport claims it;
- (P1) sync negotiation, resumability, and remote-ref behavior after 0006a is implemented.

The PIM certification profile is source-backed as a report fixture before durable transcript storage is
implemented. It requires Apple Calendar or iOS Calendar, Apple Contacts or iOS Contacts, Apple Mail,
Thunderbird for CalDAV/CardDAV/IMAP, DAVx5 for CalDAV/CardDAV, and RFC 8620/8621 executable JMAP
transcripts. Each transcript row must cite the redaction profile, surface, protocol, client, status,
reason, and evidence path. The profile also records the PIM RFC implementation gate from 0008, 0037,
0038, and 0039 so client certification cannot replace standards evidence. Queue 7 owner verification
accepted the Apple, Thunderbird, and DAVx5 CalDAV/CardDAV/IMAP runs; the current JMAP transcript row is
passed by the hosted router executable transcript and accepted without external JMAP tooling for this
queue. Later 0065 certification tasks can replace fixture target rows with passed, failed, degraded,
unsupported, or skipped evidence without changing the report shape.

### Reusable transcript evidence schema

Hosted and facade compatibility profiles use one transcript evidence schema. PIM transcript inventory
rows are the current source-backed seed, and other profiles must map to the same fields before their
evidence is promoted from prose to a capability or release gate.

Each transcript evidence row MUST include:

| Field | Requirement |
| --- | --- |
| `name` | Stable row identifier, unique within the report. |
| `surface` | Native or facade surface, such as `mail`, `postgres`, `s3`, `search`, or `vector`. |
| `protocol` | Wire or projection protocol, such as `imap`, `jmap`, `tcp`, `rest`, `json-rpc`, `grpc`, `webdav`, or a product profile name. |
| `transport` | Listener or execution transport used by the transcript, including loopback TCP, direct TLS, HTTP, JSON-RPC, gRPC, MCP, CLI, or in-process executable transcript. |
| `profile` | Declared compatibility profile or bounded subset being certified. A row without a declared profile cannot justify a broad compatibility claim. |
| `client` | External client, official SDK, protocol tool, fixture runner, or generated client used to produce the transcript. |
| `client_kind` | One of `real-client`, `official-sdk`, `protocol-tool`, `fixture-runner`, `generated-client`, or `in-process-runner`. |
| `owner_scope` | Principal and authority scope used for the run, such as owner-only, delegated, service-principal, anonymous-denied, or policy-denied. |
| `status` | One of `passed`, `failed`, `degraded`, `unsupported`, `skipped`, or `target`. The status has the same meaning as the certification status labels serialized by the conformance report. |
| `reason` | Required non-empty explanation. For non-passed rows it names the stable unsupported, degraded, skipped, target, or failure boundary. |
| `stable_error` | Required when `status` is `unsupported` or `failed`, and when a degraded path exposes an error. It records the public code, protocol status, and stable reason string. |
| `request_fixture` | Redacted request transcript, deterministic fixture path, or executable probe identifier. |
| `response_fixture` | Redacted response transcript, deterministic fixture path, or normalized result summary. |
| `redaction_profile` | Redaction policy name. It must identify sensitive headers, credentials, bodies, principal identifiers, and tokens that are removed or retained. |
| `evidence` | Source path or accepted spec path backing the row. A supported row needs executable source or fixture evidence, not only prose. |
| `replayability` | One of `deterministic`, `guarded-optional-tool`, `requires-local-listener`, `requires-external-client`, or `inventory-only`. |

A capability or release report MAY project fewer fields for compact output, but the source inventory
MUST retain all required fields. A projected `supported` compatibility-profile row MUST cite matching
transcript evidence or a narrower executable conformance row. Rows for unsupported behavior MUST keep
the stable public error and reason visible; a protocol-level success response MUST NOT mask unsupported
behavior. Missing external tools are `skipped` or `target`, not `passed`.

The reusable schema is intentionally profile-centered. Product headers, SDK retries, HTTP status codes,
or protocol command names do not define native semantics by themselves; they are adapter evidence for
the declared profile. The owning facade must still cite the native or shared substrate contract for
state mutation, authorization, persistence, and capability reporting.

MCP Apps protocol certification is a dedicated hosted-protocol profile. It must cover resource-backed
app discovery, `resources/read` metadata, valid-app-only resource listing, invalid-candidate reporting
through the Loom authoring tools, dynamic launcher tools with `_meta.ui.resourceUri`, workspace-bound
and unbound app name flattening, `ServerCapabilities.extensions` negotiation for
`io.modelcontextprotocol/ui`, app subscription wakeups, resource and tool `list_changed`
notifications, and the split between standalone app loading and tool-result visualization.

The host-owned MCP Apps iframe JSON-RPC bridge is a separate compatibility claim. A Loom MCP server can
certify the resource, tool metadata, extension advertisement, template rendering, and delivery behavior
it emits. A host that claims iframe bridge support needs a host protocol suite that proves the iframe
shell, bridge injection, JavaScript lifecycle, and bridge error semantics.

### Capability reports

Generated capability reports should be derived from source-backed declarations, not prose. A report may
claim `executable` only when the capability has source-backed API and a shared runner.
Local coordination rows are separate from the target `lock` capability row because embedded locks,
CLI daemon behavior, host-native clients, and MCP attach behavior are narrower than a fully promoted
hosted or cross-platform public lock facade.

### Capability-state vectors and transition matrix

The two-axis `CapabilityRecord` in 0010 section 5.1 requires a dedicated conformance family. The
family is target work until the record codec and scoped contributors exist. It MUST use the real
registry, feature gates, listener runtime, policy-enforcement point, and derived-artifact lifecycle;
mocked availability flags do not prove the capability contract.

| Fixture or runner | Owner | Required proof |
| --- | --- | --- |
| `capability-record-canonical-v1` | `loom-conformance` plus `loom-codec` | Canonical CBOR and lossless JSON vectors for every required field, selector ordering, omitted optionals, version pairs, evidence, limits, and probe freshness. |
| `capability-proof-status-v1` | `loom-conformance` plus `loom-core` | One declaration for each of `executable`, `source-backed`, `scenario`, `target`, and `deprecated`; runtime observation cannot change proof status. |
| `capability-operational-state-v1` | `loom-conformance` plus real contributors | One record for each of `supported`, `degraded`, `disabled`, `unavailable`, `denied`, `unsupported`, and `target`, including the stable error rule. |
| `capability-record-negative-v1` | `loom-conformance` | Reject duplicate capability and scope selectors, unknown required enum values, non-supported records without a reason code, required-error states without a stable error, degraded records without a degradation boundary, and undeclared profile claims. |
| `capability-transition-v1` | `loom-conformance`, hosted runtime, and optional engines | Prove build-feature absence and restoration, configuration enable and disable, runtime dependency loss and recovery, listener bind failure and recovery, policy deny and allow, index rebuild degradation and recovery, and probe freshness expiry. |
| `capability-projection-v1` | C ABI, IDL, bindings, CLI, hosted, and MCP owners | Decode and preserve the same state, reason code, stable error, scope, and evidence across all public projections. |
| `capability-hosted-transcript-v1` | hosted REST, JSON-RPC, gRPC, MCP, and facade owners | Prove target error mapping, retry behavior, no protocol-success masking, and policy-safe diagnostic redaction using real listeners and principals. |

The fixture matrix MUST cover the following state and error combinations:

| Operational state | Required case |
| --- | --- |
| `supported` | A declared native profile is compiled, enabled, runtime-ready, allowed, and within limits. |
| `unsupported` | An undeclared capability or profile reports `profile_unsupported` and `UNSUPPORTED`. |
| `degraded` | A declared fallback executes with an explicit equivalence, freshness, or exactness boundary. |
| `denied` | A visible resource returns `PERMISSION_DENIED`; an out-of-scope resource is masked as `NOT_FOUND`; neither reveals global runtime or profile diagnostics. |
| `disabled` | A declared compiled listener or feature is intentionally inactive and status inspection remains possible. |
| `unavailable` with `feature_not_compiled` | A configuration parsed by a build lacking the required feature reports `feature_not_compiled` and `UNSUPPORTED`. |
| `unavailable` with `runtime_dependency_absent` | A compiled profile with an absent real dependency reports `runtime_dependency_absent` and target `UNAVAILABLE`. |
| `unavailable` | Real bind, health, maintenance, recovery, or stale-probe failure reports target `UNAVAILABLE` unless an exact existing Code, such as `INDEX_NOT_READY` or `RESOURCE_EXHAUSTED`, is primary. |
| `target` | A capability declared in the target design with no source-backed surface reports `not_source_backed` and `UNSUPPORTED`. |
| `unavailable` with `listener_bind_failed` | A declared listener that fails to bind reports `listener_bind_failed` and target `UNAVAILABLE`. |
| `unavailable` with `capacity_exhausted` | A capacity-bounded surface at its limit reports `capacity_exhausted` and `RESOURCE_EXHAUSTED`. |
| `unavailable` with `recovery_in_progress` | A surface recovering from failure reports `recovery_in_progress` and target `UNAVAILABLE`. |
| `unavailable` with `probe_stale` | An expired freshness probe reports `probe_stale` and target `UNAVAILABLE`, not `supported`. |

The transition runner records the initial and terminal record, change trigger, source evidence, and
whether retry is safe. It MUST prove that a transition never changes `proof_status`. A degraded result
may be reported successful only when the caller selected or accepted its declared profile. A stale
probe is unavailable rather than supported.

Capability reports remain evidence, not promotion. `capability-proof-status-v1` can report a target
or scenario declaration, but no runner may call that declaration executable until its source API and
shared suite are both present.

## Sequencing

1. (P0) Define the report schema after 0010 and 0025 agree on executable suite names.
2. (P0) Add provider lifecycle reports after 0005, 0009, and 0034 security behavior has shared
   conformance.
3. (P1) Add binding runtime reports after 0007 selects v1 runtime gates for each mandatory platform.
4. (P1) Expand hosted protocol reports beyond the current PIM feature evidence matrix after 0008 has
   source-backed generated schemas and additional protocol adapters.
5. (P0) Expand principal-aware and ACL-aware certification after each additional 0026-0028 surface
   promotes beyond the current local executable runner boundary.

## Resolved decisions

1. **Reports do not promote capabilities.** Reports serialize proof. They do not turn target or
   scenario-only surfaces into implemented behavior.
2. **Runtime suites are required for binding certification.** Compile checks prove buildability, not
   runtime correctness.
3. **Hosted protocol conformance waits for source.** Protocol adapters cannot be certified before 0008
   has an implemented hosted surface.
4. **Skipped suites are explicit.** A skipped suite must include a reason and cannot be counted as
   passed.
