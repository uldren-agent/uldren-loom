# Release Maturity

## Status

Loom is Alpha. The first Alpha release is `0.1.0-alpha.1`.

Alpha identifies a usable product whose scope and contracts can still change materially. It is not
a claim of production readiness. Production suitability is evaluated separately against a
documented workload, deployment model, and support envelope.

Decision Points: none.

## Version progression

Loom uses Semantic Versioning pre-release identifiers for maturity:

```text
0.1.0-alpha.1
0.1.0-alpha.2
0.1.0-beta.1
0.1.0-rc.1
0.1.0
```

The workspace package version in the root `Cargo.toml` is authoritative. `loom --version` prints
that version. `just sync-versions` propagates it to binding manifests.

Increment the trailing pre-release number for releases within a maturity stage. Promotion changes
the identifier and resets that number to `1`. A release candidate that requires a public contract
change returns to Beta. Stable releases omit the pre-release identifier.

## Maturity definitions

### Experimental or prototype

The implementation tests feasibility. Capabilities can be incomplete, replaced, or removed, and
compatibility is not promised. This stage is suitable for internal research and demonstrations.

### Developer Preview

Selected workflows are available for early evaluation, but broad completeness is not claimed. The
stage is suitable for design partners and controlled evaluation. Preview is not used as a SemVer
identifier because its compatibility meaning is ambiguous.

### Alpha

The intended product exists and important workflows operate end to end. Scope, APIs, ABI, storage
formats, protocols, and behavior can change materially. Alpha is suitable for development, testing,
and non-critical trials.

### Beta

The intended v1 scope and public contracts are substantially settled. Work focuses on
compatibility, hardening, conformance, migration, security, and operational proof. Beta is suitable
for staging and carefully controlled production pilots with backups and explicit risk acceptance.

### Release Candidate

The candidate has no known release-blocking defect and is expected to become the stable release
without contract changes. It is suitable for final qualification and migration rehearsals.

### Stable

Compatibility, migration, security response, support, and operational commitments are active for
the documented support envelope. Stable does not imply suitability for every workload.

## Alpha entry gate

All requirements must be satisfied before describing a release as Alpha:

- A documented path can create a store, write and read data, commit or version it, close it, reopen
  it, and recover it.
- Canonical encoding and digest behavior have positive and negative tests.
- Persistent-store corruption produces controlled errors rather than silent acceptance.
- Supported platforms and explicitly unsupported surfaces are documented.
- The Rust workspace CI gate passes consistently.
- Security-sensitive features have documented boundaries and do not imply an external audit.
- Release notes identify breaking API, ABI, storage-format, and protocol changes.

## Beta promotion gate

Promotion from Alpha to Beta requires evidence for every item:

- The intended v1 feature scope is frozen.
- The canonical object model, digest rules, `.loom` format, C ABI, stable error codes, CLI output
  contracts, and primary protocol shapes are substantially frozen.
- Every binding advertised as supported passes shared conformance vectors.
- Upgrade and migration behavior between supported versions is defined and tested.
- Crash recovery, interrupted writes, corruption detection, and concurrent access have adversarial
  tests.
- Fuzzing covers identity-affecting decoders and untrusted input boundaries.
- Security threat models are documented, dependencies are continuously scanned, and the
  vulnerability-response process is operational.
- Supported platform matrices are exercised by release CI.
- Performance and resource ceilings are measured with representative workloads.
- No known data-loss, integrity, authentication-bypass, or compatibility blocker remains open.
- Documentation supports an evaluator who has no internal project knowledge.

Beta means that Loom is proving the v1 contract. Feature count alone is not evidence for promotion.

## Release Candidate promotion gate

Promotion from Beta to Release Candidate requires:

- Every Beta gate has current evidence.
- The exact candidate artifacts pass the complete release and binding matrix.
- Migration from the latest Beta succeeds against representative stores.
- Backup, restore, recovery, and rollback procedures have been rehearsed.
- No release-blocking defect remains open.
- Candidate changes are limited to release-blocking fixes. A public contract change returns the
  project to Beta.

## Stable promotion gate

Promotion from Release Candidate to Stable requires:

- A published compatibility policy covers Rust APIs, C ABI, storage formats, wire protocols, CLI
  behavior, and supported bindings.
- A supported upgrade window and tested migration path are defined.
- Release, patch, deprecation, and security-response processes are operational.
- Production-like soak testing supplies operating evidence.
- Durability and concurrency guarantees are documented and tested.
- Security review is appropriate to the supported threat model.
- Every advertised surface has complete conformance coverage.
- Support boundaries and known limitations are published.

## Promotion review

Each promotion proposal records evidence for every gate in the release issue or release pull
request. A gate is satisfied only by a linked test, CI job, report, policy, or published document.
An assertion without evidence remains incomplete.

Promotion is a deliberate owner decision after all gates are satisfied. Failure to satisfy a gate
keeps the current maturity label; it does not justify weakening or silently excluding the gate.

Review this specification when public contracts, supported surfaces, release tooling, or the threat
model changes. Changes to a gate apply to the next promotion review and must not rewrite the claimed
maturity of an already published artifact.
