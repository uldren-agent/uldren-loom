# 0007a - Binding Generation, Packaging, and Certification

**Status:** Draft target extension. **Version:** 0.1.0. **Normative target.**

This document owns target binding work split out of 0007. It does not change the current binding
boundary: 0007 is complete for the source-backed ABI, checked-in binding surfaces, and current evidence
tiers.

## Current source boundary

Current source provides:

- hand-written C ABI declarations in `include/loom.h`;
- `just header-check`, which verifies `include/loom.h` against `crates/loom-ffi` cbindgen output and
  verifies the iOS vendored header against `include/loom.h`;
- hand-written binding wrappers for the currently promoted surfaces;
- binding recipes in `justfile`;
- checked-in Node, Python, iOS, C++, JVM, Android/JVM, and WASM browser-worker runtime suites;
- a Linux pull-request CI gate for the C++ runtime suite, which builds the release FFI library and
  runs the registered CTest target;
- a bindings workflow gate for the WASM browser-worker runtime suite, which builds the web package
  and runs `browser-test/run.mjs` under headless Chrome;
- a bindings workflow gate for the Android Kotlin/JVM runtime suite, which builds the release FFI
  library and runs `:compileKotlinJvm :jvmTest`;
- a bindings workflow gate for React Native Android connected runtime certification, which runs
  `just release-test-binding react-native-android` under an Android emulator;
- standard and FIPS binding material manifests through `just binding-release-materials` and
  `just binding-release-materials-fips`, including binding package checksums, workspace-version
  compatibility metadata, IDL and C-header digests, publication-route policy, credential policy,
  install-validation policy, attestation policy, and an unsigned binding signing manifest;
- a binding publication dry-run gate through `just binding-publication-dry-run`, which validates the
  manifest publication routes, credential policy, install-validation policy, unpublished package
  status, unsigned artifact status, and target attestation status without reading registry credentials
  or publishing packages;
- binding conformance inventory tiers and runtime coverage labels in `uldren-loom-conformance`;
- a serialized `binding_runtime_certification` report section that records each runtime-certified
  binding family, release workflow, evidence files, status, and promoted-surface coverage labels;
- a serialized `binding_package_certification` report section that records each binding package name,
  package kind, build recipe, source-backed material evidence, source-backed compatibility metadata,
  source-backed unsigned signing manifest evidence, and target publication status;
- a Node/Python cross-binding interop release gate through `just binding-cross-interop`, which proves
  shared `.loom` store open/read/write for SQL rows and document text;
- release certification inventory rows in `uldren-loom-conformance` for binding material capture,
  browser/device runtime evidence, provider profile evidence, unsupported WASM native-FIPS claims, and
  source-backed compatibility metadata, signing-manifest material, registry-route policy, credential
  policy, install-validation policy, attestation policy, dry-run publication-gate evidence, and target
  publishing/signing work;
- a conformance-crate drift guard that asserts promoted runtime-suite surface names remain present in
  both `idl/loom.idl` and `include/loom.h`.

Current source does not provide:

- (P1) generated bindings from `idl/loom.idl`;
- (P1) generated REST, JSON-RPC, gRPC, or MCP client projections from the IDL;
- (P1) full native package distribution for every supported platform;
- (P1) runtime conformance harnesses for every binding family;
- (P2) full cross-binding interoperability certification across every supported binding family;
- (P0) full ABI/IDL/wrapper drift detection beyond the current generated-header, vendored-header, and
  promoted-name presence guards.

## Target generation track

Generated binding work must preserve the source-backed ABI and error contract:

- (P0) generate types only from source-backed IDL surfaces;
- (P0) preserve stable `Code` values exactly;
- (P0) preserve Loom Canonical CBOR bytes for structured results;
- (P0) preserve C ABI ownership and freeing rules;
- (P1) generate idiomatic language facades for supported bindings;
- (P1) generate REST, JSON-RPC, gRPC, and MCP projections only after 0008 implements the hosted
  surface;
- (P2) generate examples and docs from the same source descriptions.

Target-only facades must not appear in generated bindings until the owning spec is source-backed.

## Target packaging track

Packaging work must be proof-backed:

- (P1) package Node prebuilds by platform and architecture;
- (P1) package Python abi3 wheels by platform and architecture;
- (P1) package JVM native classifiers or a reliable native-library loader;
- (P1) package Android AAR artifacts for supported ABIs;
- (P1) package iOS/macOS SwiftPM or XCFramework artifacts;
- (P2) package React Native templates or host-app integration checks;
- (P2) package WASM npm artifacts with OPFS capability reporting;
- (P0) publish version compatibility metadata tying each binding package to its compatible core ABI.
  The source-backed release-materials manifest now records workspace version, IDL digest, and C-header
  digest, but registry publication of that metadata remains target.
- (P0) sign binding package artifacts from the binding signing manifest. The source-backed manifest
  records the unsigned artifact set and checksums, but signature application and registry attachment
  remain target.
- (P0) use the source-backed publication-route policy as the release gate input. The target routes are
  npm for Node/React Native/WASM, PyPI for Python, Maven Central for JVM/Android, SwiftPM tags and
  GitHub Release assets for iOS, CocoaPods for the React Native podspec where published, and GitHub
  Release assets for C/C++ native packages.
- (P0) require registry OIDC or trusted publishing where the registry supports it. Scoped release
  secrets are allowed only for registries without an OIDC path, and they must be limited to the
  package namespace being published.
- (P0) validate installability from the published package, not from the workspace build output. Each
  package route needs a clean-project install test that calls the public version and runtime-profile
  surface before the package is release-certified.
- (P0) keep publication dry-runs credentialless. The current dry-run gate validates publication policy
  and emits `binding-publication-dry-run.json`, but it must not be used as proof that a package was
  published, signed, attested, or installable from a registry.

## Binding registry publication and signing release gate

Completion state: active implementation owner. Package material capture, checksums, compatibility
metadata, publication-route policy, credential policy, install-validation policy, attestation policy,
unsigned signing-manifest material, and a protected dry-run publication gate are source-backed, but
registry publication, signature application, registry attestation attachment, published-package
install validation, and release-channel promotion automation are not complete.

Decision Points: none.

| Gate | Source-backed evidence | Remaining implementation work | Disposition |
| --- | --- | --- | --- |
| Package material capture | `scripts/binding-release-materials.sh` records package material manifests, checksums, and artifact discovery. `uldren-loom-conformance` serializes binding package certification evidence. | Keep material capture as input evidence for publication, not as proof that packages were published or signed. | Source-backed input evidence. |
| Compatibility metadata | `scripts/binding-release-materials.sh` records `loom.binding-compatibility.v1` metadata with the workspace version, `include/loom.h` digest, and `idl/loom.idl` digest. `uldren-loom-conformance` records `binding-package-compatibility` as source-backed release evidence. | Publish the metadata with each registry package and assert registry-visible compatibility in release certification. | Source-backed material evidence; registry publication remains target P0. |
| Signing manifest | `scripts/binding-release-materials.sh` records `loom.binding-signing-manifest.v1` with the unsigned artifact set and SHA-256 checksums. `uldren-loom-conformance` records `binding-package-signing-materials` as source-backed release evidence. | Apply signatures, bind attestations to the registry packages, and verify those signatures from the published artifacts. | Source-backed unsigned manifest; signature application remains target P0. |
| Registry publication | `scripts/binding-release-materials.sh` records target publication routes: npm for Node, React Native, and WASM; PyPI for Python; Maven Central for JVM and Android; SwiftPM tag and GitHub Release assets for iOS; CocoaPods for the React Native podspec where published; GitHub Release assets for C/C++ native packages. | Add guarded publish jobs for the selected registries and prove package availability from the registry after publication. | Source-backed route policy; registry publishing remains target P0. |
| Credential policy | `scripts/binding-release-materials.sh` requires registry OIDC or trusted publishing where supported, with scoped release secrets only where a registry lacks an OIDC path. | Wire registry credentials through protected release environments and prove publish jobs cannot run from pull requests or untrusted refs. | Source-backed credential policy; credential wiring remains target P0. |
| Provenance and attestation | Current package material rows identify generated artifacts and material inputs, and the signing manifest identifies checksummed release-candidate artifacts. They do not prove immutable provenance or registry attestation. | Add provenance records, artifact immutability rules, and verification rows that bind packages to the git tag, core ABI, generated header, IDL, and conformance report. | Target P0. |
| Install validation | `scripts/binding-release-materials.sh` records per-package install-validation policy: clean-project installation, public version call, runtime-profile call, and package-specific smoke or connected fixture. | Run those install validations against published artifacts, not workspace build outputs. | Source-backed validation policy; published-package validation remains target P0. |
| Protected dry-run gate | `.github/workflows/bindings.yml` runs `just binding-publication-dry-run` after the binding/runtime jobs in the `binding-release` environment and uploads the generated dry-run evidence. `scripts/binding-publication-dry-run.sh` validates routes, credential policies, install-validation policies, unpublished status, unsigned status, and target attestation status without registry credential access. | Convert the dry-run gate into guarded publish jobs only after registry credentials, environments, rollback/revocation, and post-publication install validation are configured. | Source-backed dry-run gate; live publishing remains target P0. |
| Release-channel policy | Current workflows separate PR CI, tag/manual binding builds, lean `test-bindings`, release-test binding entry points, and target publication routes. They do not automate registry channel promotion. | Define when artifacts move from local build to release candidate to published package, including retry, rollback, revocation, and unsupported-platform reporting. | Target P0. |

## Target runtime certification track

Every binding promoted as runtime-certified needs checked-in tests:

- (P0) stable error-code preservation;
- (P0) ownership and memory-freeing behavior for returned strings, bytes, views, iterators, tasks, and
  handles;
- (P0) result-vector byte equality for canonical CBOR payloads;
- (P0) workspace lifecycle behavior;
- (P0) store create/open behavior, including encrypted stores where exposed;
- (P1) SQL session and batch behavior;
- (P1) queue and queue-consumer behavior where exposed;
- (P1) CAS behavior where exposed;
- (P1) direct table/history readers where exposed;
- (P1) key-wrap management where exposed;
- (P1) async task behavior off event loops and UI threads;
- (P2) full cross-binding open/read/write interoperability across every supported binding family.

Bindings without checked-in runtime suites remain implemented-but-ungated, even when they compile.

## Target ABI drift checks

The release path should include drift detection:

- (P0) `include/loom.h` agrees with `crates/loom-ffi` - source-backed by `just header-check`;
- (P0) vendored binding headers agree with `include/loom.h` - source-backed by `just header-check`;
- (P0) `idl/loom.idl` names only source-backed promoted surfaces;
- (P1) generated declarations match checked-in hand-written wrappers where both exist;
- (P1) binding READMEs match exported APIs.

## Sequencing

1. (P0) Extend ABI/header/IDL drift checks beyond current generated-header and promoted-name guards.
2. (P1) Select mandatory v1 runtime-gated binding families.
3. (P1) Add runtime conformance harnesses for the selected binding families.
4. (P1) Generate bindings only after 0003 and 0008 source-backed surfaces stabilize.
5. (P1) Add package distribution checks for selected release targets.
6. (P2) Add cross-binding interop after each participating binding has a runtime harness.

## Resolved decisions

1. **Generated bindings are not a prerequisite for current 0007 closure.** Hand-written bindings are
   source-backed today.
2. **Compile checks are not runtime certification.** A build-gated binding remains
   implemented-but-ungated until a runtime suite exists.
3. **Target facades wait for source.** Generation must not expose target-only surfaces from planning
   specs.
4. **Generated headers are current release evidence.** The C header and iOS vendored header are
   checked by `just header-check`; full generated bindings remain target work.
