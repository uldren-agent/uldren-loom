# 0032 - Native / Web platform parity

**Status:** Partial, with current source-backed boundary documented. **Version:** 0.1.0-target.
**Normative:** parity classes and divergence playbook. **Informative:** source-backed matrix.

**Depends on:** 0001 (architecture), 0007 (bindings and wasm), and every facet spec it classifies.
**Relates to:** 0008 (hosted protocol surfaces), 0010 and 0025 (conformance), 0033 (search).

Loom has native builds and a browser-facing `wasm32` binding. This document records which behavior is
portable today, which API surfaces are actually exposed on each platform, and which platform-specific
capabilities remain target work. A row is current only when source, bindings, or conformance prove it.

## Current implementation

Current source-backed platform facts:

- `loom-core`, `loom-codec`, `loom-sql`, and most raw facet substrates are pure Rust and
  `wasm32`-clean by design.
- `loom-store` has a native file backend and a `BackingIo` abstraction used by the browser OPFS
  binding. The browser path requires an OPFS sync-access handle in a Web Worker.
- `bindings/wasm` exposes `version`, `blob_digest`, `conformance_digest`, OPFS conformance,
  OPFS-backed SQL, workspace lifecycle helpers, table/history readers, queue and queue-consumer
  helpers, CAS helpers, calendar/contacts/mail helpers, and encrypted plus raw-KEK create/open
  variants on `wasm32`. It also exposes local identity/ACL administration and session authentication
  on the live OPFS `LoomSql` object.
- Native bindings expose a broader C ABI and language wrapper surface than the wasm binding today.
- `loom-compute` uses wasmi by default and on wasm. Native builds may opt into Wasmtime through the
  `engine-wasmtime` feature, with a cross-engine test for the current files-facet host ABI.
- `loom-core::vector` exact search and `loom-core::vindex` are portable. `loom-hnsw` is a native-only
  accelerator crate with no workspace dependents.
- `loom-store::daemon` has a native daemon transport contract model and typed endpoint abstraction for
  TCP loopback, Unix sockets, and Windows named pipes. The daemon defaults to secure native IPC where
  available, using Unix sockets with peer-credential owner checks on supported Unix-family targets and
  Windows named pipes with an owner-only security descriptor on Windows. TCP loopback requires explicit
  selection, is reported as a degraded portable fallback, and remains the fallback on platforms without
  a safe native IPC runtime. `loom doctor daemon <store>`, text status output, and `loom daemon status --json`
  expose the active transport, endpoint security profile, and supported/degraded/unsupported transport
  capability evidence.
- The workspace does not currently implement hosted REST, JSON-RPC, gRPC, MCP, hosted/network FUSE,
  live sync transports, platform-specific release capability gates, a search crate, Tantivy
  integration, Polars, DataFusion, DiskANN, GPU vector backends, or an embedding runtime. Static
  capability registry reports and conformance-report evidence for runtime provider profile, hosted
  protocol feature rows, and local coordination feature rows are source-backed. (The *local* filesystem
  projection - `loom mount fuse` / `loom mount nfs` over `loom-vfs` - is implemented and source-backed; see
  0003c and the parity row below.)

## 1. Principles

- **P1 - Parity by default.** A capability present on both native and web MUST return the same results
  on both unless this document records and justifies a divergence.
- **P2 - The contract is the portable behavior.** When a native accelerator exists, the portable path
  defines correctness. The accelerator either returns identical results, reconciles to the portable
  result, or is an explicit approximate mode.
- **P3 - Exposed API parity is separate from substrate parity.** A Rust substrate can be portable even
  when the current wasm binding does not expose it. Specs and capability reports must not advertise a
  platform API until the binding surface exists.
- **P4 - Web is a subset, not a different product.** The browser build may omit capabilities such as
  server roles or native accelerators. Anything it exposes still follows P1 and P2.

## 2. Platform-divergence playbook

When a capability risks behaving differently on native and web, resolve it with one of these
mechanisms:

1. **Avoid the non-portable dependency.** Prefer Rust libraries that compile to
   `wasm32-unknown-unknown` without native system calls, C or C++ build steps, required threads,
   `mmap`, or process-global clocks.
2. **Dual-compile two implementations only with proof.** Two engines may back one capability only when
   conformance proves they produce the same observable output for the same input.
3. **Use an accelerator behind a portable contract.** A native-only accelerator may be used when the
   returned result is reconciled to the portable contract, or when the caller explicitly opted into an
   approximate mode whose differences are documented.
4. **Gracefully degrade.** If a capability cannot be made portable, web omits or reduces it. The
   capability report must mark it absent or reduced, use must return `UNSUPPORTED`, program manifests
   that require it must be rejected, and data must still sync as opaque content when the storage model
   allows it.

## 3. Current source-backed parity matrix

Parity classes:

- **Identical substrate:** same source-level behavior on native and wasm-capable builds.
- **Equivalent backend:** same file or object contract, different platform backend.
- **Dual-verified substrate:** two engines with source tests proving the same result for the current
  implemented surface.
- **Native accelerator:** optional native speed path behind a portable contract.
- **Native-only target:** not implemented as a current browser capability.
- **Target:** desired capability, not implemented as a current native or web product surface.

| Capability or surface | Native current | Web current | Class | Source-backed boundary |
| --- | --- | --- | --- | --- |
| Object model, digest, codec | Implemented | Implemented in wasm binding helpers | Identical substrate | `loom-core`, `loom-codec`, and `bindings/wasm::blob_digest` / conformance digest expose portable identity checks. |
| `.loom` storage format | Native file backend | OPFS `BackingIo` backend in wasm binding | Equivalent backend | Same page-engine store and encryption model; browser requires a Web Worker OPFS sync-access handle. |
| Workspace registry, files, VCS history | Implemented in `loom-core`, CLI, ABI, bindings | Workspace lifecycle helpers exposed through OPFS `LoomSql`; files and full history remain substrate-only | Identical substrate, partial web API | Do not claim full wasm API parity until wasm exposes the same files and workspace-history operations. |
| Direct workspace sync and bundles | Implemented in `loom-core` and CLI | Portable substrate, not a public wasm sync API | Identical substrate, partial web API | Current sync is in-process or offline bundle only. Hosted transports are target work in 0008. |
| Hosted sync and protocol transports | Not implemented | Not implemented | Target | No REST, JSON-RPC, gRPC, MCP, hosted/network FUSE, WebSocket, or fetch sync adapter exists in source. The *local* filesystem projection below is separate from these hosted transports. |
| Local filesystem projection (mount as a folder) | Source-backed: `loom-vfs-fuse` (FUSE; pure-Rust mount on Linux/BSD, macFUSE on macOS) and `loom-vfs-nfs` (NFSv3; driverless macOS/Linux) over the portable `loom-vfs` layer, with CLI `loom mount fuse` / `loom mount nfs` | Not applicable | Native source-backed, web out of scope | Portable inode/path layer `loom-vfs` is wasm-clean and unit-tested; native backends sit outside `loom-core`. The browser has no mount mechanism, so the OPFS / File System Access and WebDAV projections are eliminated (not deferred) in 0003c. See 0003c for the projection model, platform matrix, and promotion state. |
| Local daemon transport profile | Contract model and typed endpoint abstraction source-backed; secure native IPC selected by default where available; TCP loopback runtime source-backed, explicit, and degraded; Unix socket serving, endpoint/client routing, and safe peer-credential owner checks source-backed on supported Unix-family targets; Windows named-pipe serving, endpoint/client routing, owner-only descriptor binding, and Windows-target compile evidence source-backed | Not applicable | Native source-backed contract and secure runtime paths | `loom-store::daemon`, `loom doctor daemon <store>`, text status output, and `loom daemon status --json` report transport kind, capability status, endpoint security profile, and reason strings. Runtime serving falls back to TCP loopback only on platforms without a secure native IPC runtime or when the operator selects `--transport tcp`. |
| SQL and tabular substrate | Implemented through GlueSQL, core tabular storage, CLI, ABI, bindings | OPFS SQL session plus table/history readers in wasm binding | Identical substrate, partial web API | SQL behavior has source-backed vectors; web API surface is narrower than native bindings but includes direct table/history readers. |
| Queue facade and consumer offsets | Implemented in `loom-core`, C ABI, CLI/binding surfaces | OPFS `LoomSql` queue and queue-consumer helpers | Equivalent backend, partial web API | Queue append/range/len and authority-local consumer offsets are source-backed; hosted delivery and observed anchors remain target work. |
| Calendar, contacts, and mail local facets | Source-backed in `loom-core`, C ABI, bindings, conformance, and local VFS overlay | OPFS `LoomSql` exposes calendar/contacts/mail helpers; no browser mount | Equivalent backend, partial web API | Local typed records, search/list/range where implemented, CAS-backed mail bodies, and binding helpers are source-backed. Hosted CalDAV/CardDAV/IMAP/MCP, authenticated principal binding, and ACL-aware serving remain target. |
| Raw graph, vector, ledger, KV, document, time-series, columnar, dataframe, CAS substrates | Implemented as Rust substrates or public local facades per owning spec | Portable where compiled through `loom-core`; some are exposed through wasm OPFS helpers | Identical substrate, partial web API | Public facades, ABI, bindings, wire projection, and conformance are source-backed only where the owning spec says so. Dataframe exposes raw-CBOR plan/collect/preview/materialize helpers across native wrappers and wasm OPFS. Columnar native hosted REST and JSON-RPC are source-backed for the current management/query profile; dataframe REST management is source-backed; Arrow Flight, Flight SQL, and other binary analytical data-plane transports remain target work. |
| Vector exact search | Implemented | Portable through `loom-core` | Identical substrate | `VectorSet::search` is deterministic exact search with metadata filtering. |
| Vector PQ / auto index substrate | Implemented in `loom-core::vindex` | Portable through `loom-core` | Native/web portable accelerator substrate | The PQ path is wasm-clean and reconciles returned scores to exact search. |
| HNSW vector accelerator | Implemented in `loom-hnsw` | Absent | Native accelerator | `loom-hnsw` is native-only, has no workspace dependents, and re-scores returned candidates with exact `Metric::score`. |
| Compute files-facet engine | wasmi by default; Wasmtime optional with `engine-wasmtime` | wasmi | Dual-verified substrate | Current cross-engine test proves the same file set for the existing files-facet host ABI. Public `exec` remains target in 0015. |
| Whole-Loom storage encryption | Implemented | Implemented in wasm OPFS create/open paths | Equivalent backend | Passphrase and raw-KEK paths exist in wasm and native surfaces; provider-specific key sources remain target in 0034. |
| Local identity and ACL | Implemented in `loom-core`, C ABI, CLI, and checked-in native binding suites | Implemented in the OPFS `LoomSql` binding surface | Equivalent backend, partial web API | Identity/ACL administration and session authentication are source-backed locally. Hosted auth, platform capability reports, and every cross-platform runtime gate remain target work. |
| Capability and evidence reporting | Static capability registry exposed through C ABI and checked-in bindings; conformance report serializes runtime profile, hosted protocol rows, local daemon/lock coordination evidence, capability matrix rows, release certification rows, and binding package material manifests | Static capability registry and runtime profile exposed in WASM; WASM package material is compatibility evidence only | Partial source-backed reporting | `capabilities()` reports the build-linked registry. `report_memory_store().to_json()` distinguishes embedded coordinator, CLI daemon, host-native wrappers, MCP attached state, unsupported mobile/browser daemon locks, unsupported hosted locks, unsupported native transport profiles, degraded TCP loopback evidence, hosted Tier-1 and Tier-2 capability rows, browser/device runtime evidence, provider-profile evidence, and release-material evidence. Binding release-material manifests record package names, runtime-profile surfaces, FIPS claim eligibility, artifacts, and checksums. Registry publishing, artifact signing, install validation, and native binding FIPS publication remain target work. |
| Full-text search | Not implemented | Not implemented | Target | No search crate, Tantivy dependency, search facade, search ABI, binding, wire projection, or conformance exists. 0033 owns the target design. |
| MCP or served protocol roles | Not implemented | Not implemented | Target | 0008 defines target protocols, but source has no server role today. |
| Polars/DataFusion OLAP acceleration | Not implemented | Not implemented | Target | Current columnar/tabular substrates are source-backed; native OLAP accelerators are not. |
| DiskANN, GPU vector backends, embedding runtime | Not implemented | Not implemented | Target | These are not current product behavior on either platform. |

## 4. Current mismatch details

### 4.1 Native file backend and browser OPFS backend

Native `.loom` storage uses the file-backed `FileStore` path. Browser storage uses the same store
engine over a wasm `BackingIo` implementation backed by OPFS sync-access handles. This is equivalent
at the store contract level, but not identical operationally: OPFS sync access is browser-specific,
requires a Web Worker, and the wasm binding holds an open writer session differently from native
per-operation SQL sessions.

### 4.2 Binding surface mismatch

Native bindings wrap the C ABI and currently expose more of the store, workspace, encryption, SQL,
task, result, CAS, and direct handle APIs. The wasm binding exposes identity helpers, conformance
checks, OPFS-backed SQL, workspace lifecycle helpers, direct table/history readers, and queue helpers
inside the `LoomSql` surface. This means many substrates are portable without yet being public web
APIs. Completion of a parity row requires both portable source and the binding projection for that
platform.

### 4.3 Compute engine mismatch

wasmi is the default engine and the only wasm engine. Wasmtime is a native optional feature. The
current proof is scoped: source tests compare the current files-facet host ABI result, not every target
`exec` behavior described in 0015. New `StateAccess` operations, grants, guards, or multi-facet
surfaces must be added to both engines and covered by cross-engine conformance before being marked
dual-verified.

### 4.4 Vector accelerator mismatch

Exact vector search is the portable contract. `loom-hnsw` is a native-only accelerator and does not
enter the wasm dependency graph. It is acceptable because it is derived, rebuildable, and re-scores
returned candidates with exact vector scoring before returning hits. Approximate recall differences
must not be hidden from callers when HNSW is used for large corpora.

### 4.5 Hosted protocol and server role absence

Current source does not implement hosted REST, JSON-RPC, gRPC, MCP, hosted/network FUSE, live sync
transport, or browser fetch/WebSocket sync adapters. These are target protocol surfaces in 0008. Until
source lands, 0032 cannot classify native server roles versus browser client roles as implemented
behavior. This is distinct from the *local* filesystem projection (4.7), which is source-backed and
serves only a local mount, not a network role.

### 4.6 Search absence

0033 is target work. There is no `search` crate, Tantivy dependency, search workspace facade, mapping
metadata, native indexer, web fallback, ABI, binding, wire projection, or conformance. Platform parity
for search should be decided in 0033 first, then recorded here after implementation exists.

### 4.7 Local filesystem projection

A workspace working tree can be projected as a real OS filesystem (mount it as a folder). The semantics
live once in the portable `loom-vfs` layer (inode/path mapping over the `fs` facade), which is
`wasm32`-clean and unit-tested; thin native backends sit outside `loom-core`: `loom-vfs-fuse` (FUSE)
and `loom-vfs-nfs` (NFSv3), driven by the CLI `loom mount fuse` / `loom mount nfs` subcommands. This is a *local*
mount, not a hosted/network role (contrast 4.5). Platform behavior: Linux/BSD get real FUSE mounting
with no native dependency (fuser's pure-Rust mount) plus driverless NFSv3; macOS gets driverless NFSv3
and FUSE only with macFUSE installed; the web has no mount mechanism, so the OPFS / File System Access
and WebDAV projections are eliminated (not deferred). 0003c is the owning spec for the projection model,
platform matrix, errno mapping, and promotion state.

## 5. Target parity requirements

Before a new facet or capability is advertised on any platform:

1. Record the current substrate and public API status in the matrix.
2. Prove whether each implementation is identical, equivalent, dual-verified, accelerated, degraded,
   or absent.
3. Add conformance that runs on every platform claiming the capability.
4. Add reduced or absent capability reporting before exposing a platform-specific degradation.
5. Keep native accelerators behind portable contracts unless the caller explicitly opts into
   approximate behavior.
6. Ensure data created by a native-only capability still has a defined web behavior: opaque relay,
   read-only inspection, `UNSUPPORTED`, or no exposure.

### 5.1 Capability-state platform evidence

Every platform that projects `CapabilityRecord` MUST run or explicitly skip the same
`capability-record-canonical-v1`, `capability-proof-status-v1`, and
`capability-operational-state-v1` vectors. A skip names the missing compiled feature, unavailable
runtime, or unsupported platform boundary. It is not a pass.

| Platform profile | Required state evidence | Required negative or transition evidence |
| --- | --- | --- |
| Core CLI desktop | `supported`, `unsupported`, `disabled`, `denied`, `unavailable` with `feature_not_compiled`, and `target` runtime states for configured optional profiles. | Real configuration enable and disable, feature-gate comparison, policy deny and allow, canonical CLI JSON and C ABI projection. |
| Core server | The seven operational states where the selected listener, engine, and policy can produce them: `supported`, `degraded`, `disabled`, `unavailable`, `denied`, `unsupported`, and `target`. | Real listener bind failure and recovery, dependency health loss and recovery, TLS or auth admission failure, retry behavior, REST/JSON-RPC/gRPC transcript mapping, and audit/redaction verification. |
| Data-heavy server | Exact `supported`, declared `degraded`, rebuilding subcauses, stale probe subcauses, `unavailable` with `runtime_dependency_absent`, and transient `unavailable` engine states. | Derived-artifact source-equivalence, rebuild and recovery, probe freshness, `INDEX_NOT_READY`, `RESOURCE_EXHAUSTED`, and `UNAVAILABLE` distinction. |
| Mobile bindings | Native records available through the C ABI and binding decoder; hosted and heavy runtime profiles explicit as unavailable, remote-only, or unsupported. | Binding round trip for unknown reason codes, no boolean collapse, and an explicit skip reason for unavailable server runtimes. |
| WASM/browser | Portable supported, reduced or degraded, unsupported, and `unavailable` native-only states with `feature_not_compiled` or platform subcause reason codes. | Canonical record decode, reduced-mode boundary, no hidden network probe, and explicit absence of hosted listeners or native-only engines. |
| CI/conformance | Machine-readable record, proof, negative, transition, and projection evidence for every profile selected by the matrix. | No-mock validation for build, listener, runtime, policy, and derived-artifact state; every intentional skip includes platform and feature reason. |

The platform report MUST preserve `proof_status` separately from `operational_state`. It MUST preserve
unknown reason codes across bindings and relay paths, reject unknown required state tokens, and never
convert a configured-but-unrunnable optional profile into `supported`. A platform may report target
`UNAVAILABLE` before the stable Code implementation exists only as target evidence; it cannot claim a
source-backed stable error mapping until the shared error, ABI, binding, remote-protocol, and hosted
implementations pass the corresponding vectors.

## 6. Unfinished work

- (P0) Promote platform-specific release gates on top of the current source-backed capability/evidence
  reporting inventory. The current report boundary records static capability registry data, runtime
  profile, hosted rows, local daemon/lock coordination evidence, capability matrix rows, browser/device
  runtime evidence, provider-profile evidence, release-material evidence, and binding package material.
  Remaining release-gate work is the status vocabulary, negative and transition evidence, no hidden
  capability advertisement, registry publishing, artifact signing, install validation, and native
  binding FIPS publication.
- (P1) Expand wasm bindings beyond the current OPFS SQL, workspace, table/history, queue, and identity
  helpers only after the corresponding native ABI and IDL surfaces are source-backed.
- (P1) Add cross-platform conformance for every promoted binding surface, not just Rust substrates.
- (P1) Keep 0032 updated when 0008 hosted protocols, 0033 search, 0034 unlock providers, or any new
  accelerator is implemented.
- (P0) Define release gates that prevent a capability from being advertised on a platform until source
  and conformance prove that platform's behavior. This remains target until the gates are generated or
  enforced by release tooling rather than only recorded as evidence inventory.

## 7. Sources

- Browser binding and OPFS storage: `bindings/wasm/src/lib.rs`.
- Browser store abstraction: `crates/loom-store/src/lib.rs`.
- Compute engine selection: `crates/loom-compute/src/engine.rs`,
  `crates/loom-compute/src/engine_wasmtime.rs`, and `crates/loom-compute/Cargo.toml`.
- Exact vector search and portable index substrate: `crates/loom-core/src/vector.rs`,
  `crates/loom-core/src/vindex.rs`.
- Native HNSW accelerator: `crates/loom-hnsw/src/lib.rs`, `crates/loom-hnsw/Cargo.toml`.
- Hosted protocol absence and target protocol design: `specs/0008-wire-protocols.md`.
- Target search design: `specs/0033-search-layer.md`.
