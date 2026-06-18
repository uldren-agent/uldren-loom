# Loom Assurance Harness Test Plan

This document defines the plan for building a Loom assurance harness. The harness is the system that
discovers Loom's public surfaces, runs test suites against shipped artifacts, detects drift, preserves
evidence, and grows into a release-grade assurance program with smaller scheduled subsets.

The goal is not to build every test at once. The goal is to build a harness that can expand in a
controlled order until it can exercise every category of risk: correctness, content identity, public
contract stability, security, durability, performance, interoperability, operability, and enterprise
evidence.

## Harness Mission

The harness produces evidence for five product claims:

- **Safe:** malformed input, hostile clients, corrupt storage, and binding misuse do not cause memory
  unsafety, privilege escalation, silent data loss, or undefined behavior.
- **Functional:** every exposed operation behaves as its public contract says it behaves.
- **Reliable:** acknowledged writes survive crashes, upgrades, concurrency, and operational failure.
- **Secure:** authority, capability, encryption, dependency, release, and hosted-service boundaries are
  tested as product contracts.
- **Enterprise-ready:** releases are observable, operable, portable, supportable, upgradeable, and
  compatible across languages and platforms.

The harness exercises Loom as a product, not only as Rust crates. It must run against built binaries,
generated headers, package exports, language bindings, `.loom` stores, hosted projections, MCP
surfaces, and conformance fixtures.

The harness implementation should be hybrid:

- The runner is compiled Rust.
- Product behavior tests are mostly declarative JSON test vectors.
- Suite manifests are declarative TOML or JSON.
- Surface adapters are compiled Rust modules.
- Heavy workloads, crash injection, fuzz replay, and complex oracles are compiled adapters invoked
  through manifests or vectors.

This keeps the product contract portable and reviewable while keeping execution fast, typed, and
reliable.

## Harness Principles

- **Inventory before assertion:** the harness first discovers what Loom exposes, then checks whether
  each exposed surface has tests, docs, specs, and stable output where required.
- **Evidence over confidence:** each run records what binary, source revision, feature set,
  dependency lockfile, operating system, runtime, fixture set, and corpus version produced the result.
- **Golden contracts are explicit:** canonical bytes, digests, schemas, CLI output, protocol payloads,
  and binding result vectors are stored as versioned artifacts.
- **Drift fails early:** new, removed, renamed, or shape-changed public surfaces fail the harness until
  the change has an explicit contract and test owner.
- **Negative cases are first-class:** unsupported, malformed, unauthorized, corrupt, and adversarial
  inputs must have stable rejection behavior.
- **Suites are additive:** early harness phases create reusable discovery, fixture, oracle, and result
  infrastructure so later suites compound instead of becoming one-off scripts.
- **Slow evidence still matters:** some suites belong in nightly, weekly, release, quarterly, or manual
  modes. They still need manifests, owners, artifacts, and result history.

## Assurance Map

| Claim | Harness subsystem | First useful milestone | Long-term maturity |
| --- | --- | --- | --- |
| Public contract correctness | Surface registry and drift detector | CLI, C ABI, IDL, binding, and capability inventories run in scheduled and release modes | Full CLI, MCP, hosted, ABI, IDL, schema, and binding coverage with ownership |
| Content identity correctness | Golden vector and conformance runner | Canonical positive and negative vectors run in scheduled and release modes | Cross-language vectors, property tests, fuzzing, differential checks, and platform determinism |
| Core functionality | Workflow runner | Files, history, SQL, vector, sync, and reopen smoke workflows run against built artifacts | Full end-to-end matrix across facets, projections, and language bindings |
| Data durability | Fault-injection runner | Reopen after interrupted write is tested | Crash, fsync, disk-full, corruption, recovery, backup, restore, and upgrade matrix |
| Security | Hostile-input and authority runner | Capability negatives and secret-leak checks run in scheduled and release modes | Red-team suites, compliance evidence, tenant isolation, supply-chain proof, and release provenance |
| Performance and scale | Benchmark and regression runner | Per-facet smoke benchmarks produce baselines | Capacity envelopes, resource exhaustion, regression thresholds, and platform-specific scale curves |
| Interoperability | Real-client and parity runner | CLI, C ABI, and primary binding parity smoke tests run in scheduled and release modes | Real MCP, SQL, filesystem, hosted, package, and language-runtime clients exercise release artifacts |
| Operability | Evidence and operations runner | Logs, metrics, health checks, and admin paths have smoke coverage | Disaster recovery drills, SLO checks, audit-ready reports, and support diagnostics |

## Harness Architecture

### Repository Shape

The target implementation is an independent repository. This plan is stored in the Loom repository so
it can be developed with source context, but the harness runner should not be a Loom workspace crate
and should not link to Loom internals. The harness tests copied or supplied Loom artifacts through
product surfaces.

Recommended shape:

```text
loom-assurance-harness/
  Harness.toml
  Cargo.toml
  runner/
    Cargo.toml
    src/
      main.rs
      config.rs
      manifest.rs
      schema.rs
      result.rs
      adapters/
        mod.rs
        cli.rs
        inventory.rs
      report/
        mod.rs
        html.rs
  bin/
    loom-0.3.0-a1b2c3d/
      loom
      include/
      packages/
  suites/
    smoke.toml
    nightly.toml
    release.toml
    soak.toml
    manual.toml
  schemas/
    test-case.schema.json
    operation-spec.schema.json
    suite.schema.json
    result.schema.json
    surface-inventory.schema.json
  operation-specs/
    cli/
      commands/
    mcp/
    hosted-projections/
    c-abi/
    bindings/
  test-vectors/
    cli/
      binary/
      commands/
    mcp/
    hosted-projections/
    c-abi/
    bindings/
    store/
    sql/
    vectors/
    load/
    stress/
    crash/
    security/
  fixtures/
    files/
    stores/
    corrupt-stores/
    sql/
    mcp/
    hosted/
    upgrade/
  expected/
    cli/
    mcp/
    hosted-projections/
    c-abi/
    bindings/
  corpora/
    fuzz/
    malformed-cbor/
    malformed-stores/
    malformed-mcp/
  work/
  results/
  reports/
```

`bin/` contains externally provided artifacts. The initial implementation should require the developer
or CI job to copy the tested `loom` binary into `bin/loom-<version>-<short-git-sha>/loom`. The harness
must not assume the binary was built by the current checkout. A release-gate run should group results
by the tested artifact:

```text
loom-assurance-harness/
  results/
    loom-0.3.0-a1b2c3d/
      release/
        2026-07-07T153012Z/
          run.json
          results.json
          metrics.json
          failures.json
          artifacts/
          logs/
          raw/
          normalized/
      nightly/
        2026-07-07T020000Z/
  reports/
    loom-0.3.0-a1b2c3d/
      release/
        2026-07-07T153012Z/
          index.html
          results.json
          junit.xml
```

The stable grouping key is `loom-<version>-<short-git-sha>/<mode>/<timestamp>`. This makes the
evidence answer "what artifact did we test?" before it answers "when did this run happen?"

If the harness is checked out inside the Loom repository during early work, it should still keep all
implementation files under one `harness/` directory and preserve the same internal shape.

### Implementation Model

The harness should be a Rust runner with a small core and typed adapters.

Recommended runner shape:

```text
harness-runner
  core
    config
    manifest
    schema_validation
    result_model
    artifact_store
    report_writer
    selectors
    timeouts
  adapters
    cli
    mcp
    hosted_http
    hosted_pgwire
    hosted_mysql
    c_abi
    node_binding
    python_binding
    store_corruption
    fuzz_replay
    benchmark
    crash
```

The core should not know Loom semantics beyond common result and artifact rules. Adapters know how to
execute a surface. Declarative vectors describe the test intent. Compiled adapters perform mechanics
that need speed, process control, protocol clients, fault injection, or precise measurement.

### Initial Implementation Contract

The first implementation must build the harness foundation and prove the model with CLI and inventory
adapters. It must not attempt every adapter category at once.

Required first adapter set:

- `cli`: runs the copied `bin/.../loom` binary through argument arrays and validates stdout, stderr,
  exit code, JSON output, files, and artifacts.
- `inventory`: extracts public surface inventory for the copied artifact, starting with CLI command
  inventory, binary metadata, and help output hashes.

Required first runner commands:

```text
loom-harness validate
loom-harness inventory --artifact loom-0.3.0-a1b2c3d
loom-harness run --suite suites/smoke.toml --artifact loom-0.3.0-a1b2c3d
loom-harness report --run results/loom-0.3.0-a1b2c3d/smoke/2026-07-07T153012Z
```

Required exit codes:

- `0`: selected suites passed.
- `1`: product behavior, contract drift, coverage, or security failure.
- `2`: harness configuration, manifest, schema, or vector validation failure.
- `3`: environment failure outside the product, such as missing binary, unavailable port, or missing
  runtime.
- `4`: internal harness error.

Required first output files:

- `run.json`.
- `results.json`.
- `failures.json`.
- `metrics.json`.
- `surface-inventory.json`.
- `coverage.json`.
- `reports/.../index.html`.
- `reports/.../results.json`.
- `reports/.../junit.xml`.

Required first seed vectors:

- `test-vectors/cli/binary/version/test-0001.json`.
- `test-vectors/cli/binary/help/test-0001.json`.
- `test-vectors/cli/commands/init/test-0001.json`.
- `test-vectors/cli/commands/cas/test-0001.json`.
- `test-vectors/cli/commands/unknown-command/test-0001.json`.
- `test-vectors/inventory/cli/test-0001.json`.

Required first operation specs:

- `operation-specs/cli/commands/version.json`.
- `operation-specs/cli/commands/help.json`.
- `operation-specs/cli/commands/init.json`.
- `operation-specs/cli/commands/cas.json`.

Initial non-goals:

- MCP adapter.
- Hosted projection adapters.
- Binding adapters.
- C ABI adapter.
- Crash injection.
- Fuzz execution.
- Heavy workload execution.
- Container or virtual-machine isolation.

These non-goals remain part of the roadmap. They are only excluded from the first implementation.

### Schema Contract

The authoritative schemas should be defined by Rust types and committed as JSON Schema artifacts.
The runner must validate every suite manifest, test vector, expected inventory, and result file
against committed schemas.

Required committed schemas:

- `schemas/test-case.schema.json`.
- `schemas/operation-spec.schema.json`.
- `schemas/suite.schema.json`.
- `schemas/result.schema.json`.
- `schemas/failure.schema.json`.
- `schemas/metrics.schema.json`.
- `schemas/coverage.schema.json`.
- `schemas/run.schema.json`.
- `schemas/surface-inventory.schema.json`.

Schema evolution rules:

- Every schema has a `schema_version`.
- Vectors and results declare the schema version they use.
- The runner rejects unknown required fields.
- Backward-incompatible schema changes require a migration note and updated validation tests.
- Committed schemas are contract artifacts and must not drift from the Rust types.

### Harness Trust Boundary

The harness runs external artifacts, malformed inputs, fuzz cases, crash tests, and hosted services.
It must be safe to run locally and in automation.

Baseline isolation requirements:

- Each test runs in an isolated per-test work directory under `harness/work/`.
- Tests cannot write outside their assigned work directory unless an adapter explicitly grants a
  controlled artifact path.
- Every process has a timeout.
- Child processes are terminated and reaped after each test.
- Ports are allocated by the runner, not hard-coded in vectors.
- Environment variables are explicit and minimized.
- Secrets are redacted from stdout, stderr, logs, reports, and retained artifacts.
- Resource limits are enforced where the platform supports them.
- The runner preserves stores, logs, and work directories on failure according to manifest policy.

Dangerous H3, H4, and H5 suites should be able to run in stronger isolation, such as containers or
virtual machines. This applies to destructive fault injection, crash tests, hostile corpora, resource
exhaustion, and hosted multi-service tests.

### Surface Registry

The surface registry discovers and records Loom's public contract inventory.

It owns:

- CLI command and subcommand inventory from `loom --help` and every subcommand help page.
- MCP tool, resource, prompt, schema, and naming-rule inventory.
- Hosted projection route, port, method, request schema, response schema, status code, and error-shape
  inventory.
- C ABI symbol inventory from `include/loom.h` and built libraries.
- IDL method and type inventory from `idl/loom.idl`.
- Binding export inventories for Node, Python, WASM, JVM, Swift, Kotlin, C++, and React Native.
- Capability registry and supported-feature reports.
- Public error `Code` inventory.
- Machine-readable output schema inventory.

The registry is the first harness subsystem because it tells every later suite what must be covered.

### Surface Taxonomy

A public surface is anything a user, agent, binding, protocol client, package consumer, operator, or
auditor can observe or depend on. The harness must treat each surface as inventory first and test
target second.

The taxonomy below is intentionally broad. Early milestones cover only CLI and inventory. Later
milestones promote more surfaces into enforced inventory and coverage.

| Surface | Examples | Harness source of truth |
| --- | --- | --- |
| Binary identity | executable exists, version, build profile, feature flags, FIPS mode | copied artifact, `loom version`, release metadata |
| CLI command tree | `loom`, `loom cas`, `loom init`, `loom daemon`, every subcommand | `loom --help`, subcommand help, expected inventory |
| CLI argument contracts | flags, positionals, defaults, required args, aliases, conflicts | generated inventory plus operation specs |
| CLI output contracts | stdout, stderr, JSON shape, human text, exit codes | golden outputs, schemas, vectors |
| CLI filesystem side effects | created `.loom`, lockfiles, stores, exports, temporary files | workdir checks, file assertions |
| CLI process behavior | timeouts, signals, daemon lifecycle, port binding | process adapter |
| C ABI | exported symbols, header shape, memory ownership, error mapping | `include/loom.h`, built library, ABI vectors |
| IDL | methods, types, field names, enum variants | `idl/loom.idl` |
| Error-code contract | stable `Code` enum and mappings to CLI, ABI, bindings, MCP, hosted surfaces | source inventory, expected errors |
| Capability registry | supported capabilities, feature reports, owning specs | capabilities output or API equivalent |
| Store file format | `.loom` create, open, reopen, corruption behavior | store fixtures, golden stores |
| Canonical object model | canonical bytes, digests, negative decode behavior | conformance vectors |
| Filesystem facet | read, write, list, move, copy, symlink, ranges, streaming | CLI, API, and binding vectors |
| CAS facet | put, get, list, delete, digest validation, duplicate content | CLI, API, and binding vectors |
| Workspace facet | create, list, select, facet attachment, identity behavior | operation specs and workflow vectors |
| Version-control facet | commit, branch, merge, diff, rebase, squash, conflicts | workflow vectors |
| SQL facet | sessions, exec, transactions, errors, result encodings | SQL matrices and external baselines |
| Vector facet | create set, upsert, search, filters, metadata indexes | workload and conformance vectors |
| Other facets | graph, kv, document, queue, columnar, search, time-series, ledger, calendar, contacts, mail | facet operation matrices |
| MCP surface | tools, resources, prompts, schemas, names, JSON-RPC errors | MCP inventory and vectors |
| Hosted projections | HTTP, Postgres wire, MySQL wire, SMTP, IMAP, NFS, FUSE when exposed | protocol adapters and conformance |
| Bindings | Node, Python, WASM, JVM, Swift, Kotlin, C++, React Native | export inventory and parity tests |
| Package surfaces | npm, PyPI, Maven, SwiftPM, Android packages, headers, WASM package | package inventory |
| Config and environment | env vars, config files, feature gates, runtime profiles | config matrix |
| Auth and security | passphrase, encryption, capabilities, revocation, denied paths | negative authorization vectors |
| Observability | logs, metrics, health checks, admin output | operations vectors |
| Release artifacts | SBOM, signatures, hashes, release materials | evidence runner |

Every inventory item should eventually map to one of:

- A generated operation spec.
- A hand-authored test vector.
- A conformance vector.
- A documented non-goal or deferred status.
- A release-blocking coverage gap.

The drift detector fails when an enforced inventory item lacks one of those mappings.

### Runner

The runner executes suites under named modes.

Required modes:

- `smoke`: fast local confidence for harness development.
- `nightly`: a scheduled subset for drift, smoke, and regression coverage.
- `targeted`: run suites affected by a changed surface, spec, crate, binding, or fixture.
- `release`: evidence required before shipping.
- `soak`: long-running performance, fuzzing, concurrency, and reliability runs.
- `manual`: suites that need human setup, special hardware, external services, or security review.

The runner must capture command lines, environment, feature flags, exit codes, stdout, stderr,
artifacts, durations, metrics, and failure classifications.

### Fixture Manager

The fixture manager owns test inputs and expected persistent state.

It owns:

- Golden `.loom` stores.
- Canonical object and digest vectors.
- Negative decode vectors.
- Binding result vectors.
- Malformed store files.
- Corrupt object graphs.
- Fuzz seeds and regression corpora.
- SQL conformance cases.
- Protocol request and response fixtures.
- Performance datasets.
- Backup and restore fixtures.
- Upgrade fixtures from historical releases.
- Privacy-safe synthetic datasets.

Fixtures are versioned. A fixture removal or golden-output change is a contract change unless the
suite manifest explicitly marks it as test-only cleanup.

The harness separates test inputs by role:

- `test-vectors/` contains structured test definitions with expected behavior.
- `fixtures/` contains normal input files, stores, SQL inputs, protocol payloads, and upgrade
  artifacts used by tests.
- `expected/` contains large or shared expected outputs that should not be embedded in each vector.
- `corpora/` contains collections of samples used by fuzzers, malformed-input tests, and regression
  replay.

A corpus is an input collection, not a single test. Fuzzers and adversarial tests grow corpora over
time. When a fuzzer finds a crash or interesting input, the input is minimized, stored in the relevant
corpus, and replayed by the harness as a regression sample.

### Oracle Layer

The oracle layer decides whether a harness result is correct.

It owns:

- Golden bytes, digests, schemas, and output snapshots.
- External conformance baselines for SQL, MCP, hosted protocols, PIM, and filesystem behavior.
- Differential baselines against selected reference systems.
- Property invariants for identity, ordering, import/export, batch equivalence, rebuild behavior, and
  version-control semantics.
- Expected error codes and rejection shapes.
- Performance baselines and regression thresholds.
- Upgrade and migration expectations.

Each vector or suite declares an oracle type:

- `golden`: compares against checked-in bytes, output, schema, or result snapshots.
- `spec`: compares against a Loom spec or an externally governed protocol specification.
- `external_baseline`: compares against a named reference implementation or conformance suite.
- `differential`: compares Loom behavior against another system under the same generated operations.
- `property`: checks invariants instead of fixed output.
- `policy`: checks project rules such as naming, ownership, security, or retention requirements.

When oracles disagree, spec and conformance oracles outrank implementation snapshots. Golden outputs
are not self-justifying; a golden change needs a linked contract decision unless the suite manifest
marks it as fixture-only cleanup.

### Drift Detector

The drift detector compares current inventories and outputs against expected inventories and outputs.

It fails when:

- A CLI command appears without tests, docs, and expected output snapshots.
- A CLI command is removed without a recorded breaking-change decision.
- A command argument, MCP method, hosted route, response field, binding export, C ABI symbol, IDL
  method, capability record, or error code changes without a matching contract update.
- A public name violates a protocol rule, such as an MCP tool name using a disallowed character.
- A spec requirement is implemented without test coverage.
- A test fixture or golden artifact changes without an owner-approved explanation.

### Coverage Accountant

The coverage accountant measures assurance coverage, not only source coverage.

It reports:

- **Surface coverage:** percent of CLI commands, MCP tools, hosted routes, C ABI functions, IDL
  methods, binding exports, and capability records covered by at least one black-box test.
- **Spec coverage:** percent of normative requirements with executable tests.
- **Conformance coverage:** percent of supported SQL, filesystem, MCP, PIM, hosted, and binding
  behavior covered by vectors or matrices.
- **Negative coverage:** percent of invalid, unauthorized, malformed, corrupt, and unsupported cases
  with stable rejection tests.
- **Parity coverage:** percent of public operations exercised across each binding.
- **Upgrade coverage:** percent of historical store shapes and released artifacts covered by
  compatibility tests.
- **Performance coverage:** percent of facets with defined capacity envelopes and regression
  thresholds.
- **Evidence coverage:** percent of release evidence artifacts captured and retained.

### Evidence Store

The evidence store preserves results for triage, release review, and enterprise proof.

Each run records:

- Git commit and dirty-state status.
- Built artifact hashes.
- Cargo lockfile hash.
- Feature flags and build profile.
- Rust toolchain and binding runtime versions.
- Operating system, CPU architecture, filesystem, and relevant hardware.
- Suite manifests and fixture versions.
- Fuzz corpus version.
- Benchmark dataset version.
- Test command lines and environment.
- Logs, metrics, output artifacts, and failure classifications.
- Waivers, owners, expiration dates, and linked decisions where applicable.

Evidence retention is tiered:

- Release evidence is retained long term.
- Nightly summaries are retained medium term.
- Full artifacts are retained for failures.
- Large passing artifacts are retained only when policy or a suite manifest requires them.
- Performance metrics are retained long enough to support trend and regression analysis.
- Secrets and sensitive payloads are redacted before retention.
- Corpora and regression samples are retained as source-controlled or artifact-versioned inputs.
- Retention policy records what can be pruned, when, and by which owner.

## Harness Build Priorities

Priorities are for harness build-out. They are not labels for individual PR tests.

| Level | Meaning | Exit condition |
| --- | --- | --- |
| `H0` | Harness foundation | The harness can run suites, capture artifacts, and report durable results |
| `H1` | Surface discovery and drift prevention | Public surfaces are inventoried and unowned drift fails scheduled and release modes |
| `H2` | Core correctness and conformance | Core identity, workflow, ABI, binding, and spec behavior have executable coverage |
| `H3` | Safety and adversarial testing | Hostile input, authorization negatives, corruption, fuzzing, and fault-seeded checks run |
| `H4` | Reliability, scale, and compatibility | Crash, concurrency, performance, upgrade, backup, restore, and platform suites run |
| `H5` | Enterprise evidence and long-tail assurance | Compliance, operability, disaster recovery, real-client, and audit evidence are produced |

### H0: Harness Foundation

Build this first. Without it, later test suites become disconnected scripts.

Required capabilities:

- Suite manifest format.
- Common runner.
- Result schema.
- Artifact capture.
- Feature and build matrix representation.
- Fixture directory layout.
- Golden artifact format.
- Failure taxonomy.
- Basic report output.
- Run-mode job skeleton.
- Local smoke mode for harness development.

First useful milestone:

- One trivial suite runs in `smoke` and `nightly` modes, captures artifacts, writes a result record, and
  produces a readable report.

### H1: Surface Discovery and Drift Prevention

This is the highest-value early layer because it prevents silent public-contract expansion.

Required capabilities:

- CLI inventory discovery.
- C ABI inventory discovery.
- IDL inventory discovery.
- Binding export inventory discovery for primary bindings.
- Capability inventory discovery.
- Public error-code inventory discovery.
- Golden machine-readable output snapshot support.
- Drift comparison against checked-in expected inventories.
- Ownership metadata for each public surface.
- Failure mode for new, removed, renamed, or shape-changed public surfaces.

First useful milestone:

- A new CLI command, C ABI symbol, IDL method, binding export, capability, or error code fails the
  scheduled and release harness modes until its inventory, owner, and test coverage metadata are
  updated.

Long-term maturity:

- MCP and hosted projection inventories are included once those surfaces are active.
- Protocol naming rules are checked automatically.
- Drift reports link each changed surface to owning specs, docs, tests, and conformance coverage.

### H2: Core Correctness and Conformance

This phase makes the harness prove Loom's core behavior through product surfaces.

Required capabilities:

- Canonical positive vectors.
- Negative decode vectors.
- Golden `.loom` stores.
- Black-box CLI smoke workflows.
- C ABI smoke workflows.
- Primary binding parity smoke workflows.
- Filesystem, workspace, history, SQL, vector, sync, and reopen workflows.
- SQL conformance matrix with explicit supported and unsupported cases.
- Spec coverage matrix for promoted specs.
- Golden output snapshots for machine-readable output.

First useful milestone:

- The scheduled or release harness creates a `.loom`, writes files, commits, branches, queries SQL,
  adds vectors, reopens the store, runs equivalent operations through CLI and C ABI, and verifies
  canonical outputs.

Long-term maturity:

- Every promoted facet and public operation has at least one black-box test, one negative case, and
  owner-linked spec coverage.

### H3: Safety and Adversarial Testing

This phase tests unsafe inputs, denied authority, corrupt data, and test-suite strength.

Required capabilities:

- Fuzz corpus execution for codecs, object decoding, store loading, SQL inputs, vector filters, CLI
  parsing, MCP payloads, hosted payloads, C ABI entry points, bindings, import, export, sync, and
  bundle formats.
- Malformed `.loom` files.
- Corrupt object graphs.
- Invalid canonical encodings.
- Authorization and capability negative tests.
- Cross-surface bypass attempts.
- Secret-leak checks in logs, errors, traces, panic messages, crash reports, exported stores, and
  hosted responses.
- Mutation or fault-seeded checks that prove tests catch real bugs.
- Red-team regression fixtures for known abuse cases.

First useful milestone:

- The scheduled or release harness runs a small hostile-input corpus, capability-negative suite,
  malformed-store suite, and mutation smoke check that intentionally breaks a digest, error-code, or
  capability guard in a controlled test fixture and verifies the suite fails.

Long-term maturity:

- Long-running fuzzing, broad negative coverage, corrupt-storage matrices, tenant isolation tests, and
  red-team findings become repeatable harness suites.

### H4: Reliability, Scale, and Compatibility

This phase proves Loom survives production-like conditions.

Required capabilities:

- Crash injection during object write, index update, reference update, commit, sync, compaction, and
  hosted projection writes.
- Filesystem fault injection for disk-full, permission-denied, partial-write, fsync-failure, and
  rename-failure cases.
- Reopen and recovery verification.
- Concurrency and linearizability checks.
- Performance baselines and regression thresholds.
- Per-facet capacity envelopes.
- Resource exhaustion tests.
- Upgrade tests from golden stores and released artifacts.
- Backup and restore tests.
- Cross-platform matrix.
- Real-client interoperability tests.

First useful milestone:

- The release harness verifies interrupted-write recovery, runs a small concurrent read/write suite,
  records baseline performance for the primary facets, and opens at least one historical golden store.

Long-term maturity:

- The harness reports capacity envelopes for every facet, runs platform-specific compatibility suites,
  validates backup and restore drills, and catches performance regressions before release.

### H5: Enterprise Evidence and Long-Tail Assurance

This phase turns the harness into procurement-grade and audit-grade evidence.

Required capabilities:

- Compliance and privacy evidence packs.
- Audit-log validation.
- Data deletion, retention, exportability, tenant isolation, and encryption-posture tests.
- SLO, SLA, recovery-time, and recovery-point objective checks.
- Chaos and dependency-failure testing.
- Disaster recovery drills.
- Observability and operability checks.
- Release provenance, SBOM, signing, reproducibility, and artifact-verification checks.
- Extended platform and language-runtime matrix.
- Extended real-client ecosystem matrix.
- Manual security review and red-team campaign integration.

First useful milestone:

- The release harness produces an evidence bundle containing release artifacts, SBOM, dependency and
  license status, surface drift status, coverage summaries, known waivers, and retained run artifacts.

Long-term maturity:

- The harness can support enterprise reviews with repeatable audit evidence for security, privacy,
  reliability, operations, disaster recovery, and supply chain.

## Harness Capability Map

The categories below are the test capabilities the harness must eventually support. Each capability
should have a suite manifest, owner, mode, fixtures, oracle, output artifacts, and coverage accounting.

| Capability | Primary subsystem | Build priority | Main output |
| --- | --- | --- | --- |
| Contract and drift testing | Surface registry, drift detector | H1 | Surface inventory diff and coverage gaps |
| Unit and component test ingestion | Runner, evidence store | H0 | Imported crate-level results |
| Property-based and generative testing | Oracle layer, runner | H2 | Invariant result records and failing seeds |
| Fuzzing | Runner, fixture manager | H3 | Corpus results, crashes, minimized regressions |
| Black-box surface testing | Runner, surface registry | H2 | Product-surface pass/fail evidence |
| Integration testing | Runner | H2 | Cross-component workflow evidence |
| End-to-end functional testing | Runner, oracle layer | H2 | User workflow evidence |
| Conformance and differential testing | Oracle layer | H2 | Matrix results and external-baseline gaps |
| Spec coverage testing | Coverage accountant | H2 | Requirement coverage report |
| Cross-language binding parity | Real-client and parity runner | H2 | Per-binding parity matrix |
| Security testing | Hostile-input and authority runner | H3 | Threat and vulnerability evidence |
| Authorization and capability testing | Hostile-input and authority runner | H3 | Denied-case matrix |
| Crash consistency and durability testing | Fault-injection runner | H4 | Recovery and acknowledged-write evidence |
| Concurrency and linearizability testing | Fault-injection runner | H4 | Interleaving and atomicity evidence |
| Performance, load, and stress testing | Benchmark runner | H4 | Baselines, regressions, and capacity envelopes |
| Resource exhaustion testing | Benchmark runner, hostile-input runner | H4 | Limit and backpressure evidence |
| Data corruption and adversarial storage testing | Fault-injection runner | H3 | Corruption detection evidence |
| Upgrade, migration, and compatibility testing | Upgrade runner | H4 | Historical-store compatibility report |
| Platform and environment matrix testing | Runner, evidence store | H4 | Platform result matrix |
| Observability and operability testing | Operations runner | H5 | Logs, metrics, health, and admin evidence |
| Backup, restore, replication, and disaster recovery | Operations runner | H5 | Recovery drill evidence |
| Supply chain, build, and release integrity | Evidence store | H5 | Release evidence bundle |
| Formal and model-based testing | Oracle layer | H4 | Model trace and invariant evidence |
| Red-team and abuse-case testing | Hostile-input runner | H5 | Repeatable abuse-case regressions |
| Compliance and privacy testing | Evidence store, operations runner | H5 | Audit and privacy evidence |
| Chaos and dependency-failure testing | Fault-injection runner | H5 | Dependency-failure evidence |
| Real-client interoperability testing | Real-client runner | H4 | Ecosystem client compatibility matrix |
| Test governance and waiver tracking | Evidence store | H0 | Owners, waivers, expiration, and failure policy |
| Test data and corpus management | Fixture manager | H0 | Versioned fixtures and corpus metadata |

## Suite Manifest Requirements

Every harness suite needs a manifest. The manifest prevents suites from becoming undocumented scripts.

Each manifest records:

- Suite name.
- Owner.
- Build priority.
- Run modes.
- Product surfaces covered.
- Specs covered.
- Fixtures and corpus versions used.
- Oracle type.
- Expected artifacts.
- Timeout and resource limits.
- Failure classification.
- Waiver policy.
- Last reviewed date.

## Test Definition Model

The default product-behavior test should be a declarative JSON vector. The runner validates the
schema, prepares an isolated work directory, invokes the requested adapter, evaluates expectations,
and stores raw and normalized results.

Vectors should stay declarative. They may describe setup, steps, expected outputs, workload
parameters, thresholds, artifact policy, and required features. They should not grow loops,
conditionals, embedded scripts, shell fragments, or arbitrary code execution. Complex behavior belongs
in compiled adapters with stable names and typed parameters.

The first implementation should support only these expectation operators:

- `exit_code`.
- `equals`.
- `contains`.
- `matches`.
- `not_matches`.
- `json_path`.
- `file_exists`.
- `file_not_exists`.
- `file_hash_equals`.
- `stdout.format`.
- `stderr.equals`.

Unsupported in the first implementation:

- Loops.
- Embedded shell.
- Arbitrary scripts.
- Dynamic expressions.
- Network access outside a named adapter.
- Writes outside the assigned work directory.

Simple CLI tests should use argument arrays, not shell strings:

```json
{
  "schema_version": 1,
  "id": "cli.commands.cas.test-0001",
  "surface": "cli",
  "priority": "H2",
  "modes": ["smoke", "nightly", "release"],
  "requires": {
    "binary": "loom",
    "features": ["cas"]
  },
  "setup": [
    {
      "type": "copy_fixture",
      "from": "fixtures/files/hello.txt",
      "to": "input/hello.txt"
    }
  ],
  "steps": [
    {
      "type": "command",
      "argv": ["loom", "cas", "put", "input/hello.txt"],
      "expect": {
        "exit_code": 0,
        "stdout": {
          "format": "json",
          "json_path": [
            {
              "path": "$.digest",
              "matches": "^[a-z0-9:]+$"
            }
          ]
        },
        "stderr": {
          "equals": ""
        }
      }
    }
  ],
  "cleanup": {
    "preserve_on_failure": true
  }
}
```

Protocol tests use the same structure with different step types:

```json
{
  "schema_version": 1,
  "id": "mcp.tools.list.test-0001",
  "surface": "mcp",
  "priority": "H1",
  "modes": ["nightly", "release"],
  "steps": [
    {
      "type": "mcp_request",
      "method": "tools/list",
      "expect": {
        "json_path": [
          {
            "path": "$.tools[*].name",
            "not_matches": ".*\\..*"
          }
        ]
      }
    }
  ]
}
```

### Workload and Stress Tests

Heavy load tests should also be declared as vectors, but executed by compiled adapters. The vector
describes the workload, scale, metrics, thresholds, and artifact policy. The Rust adapter owns the
mechanics.

Example workload vector:

```json
{
  "schema_version": 1,
  "id": "load.vector.upsert-search.test-0001",
  "surface": "cli",
  "priority": "H4",
  "modes": ["release", "soak"],
  "type": "workload",
  "workload": {
    "adapter": "vector_cli_load",
    "store": "work/vector-load.loom",
    "workspace": "bench",
    "set": "embeddings",
    "dimensions": 768,
    "records": 1000000,
    "batch_size": 1000,
    "queries": 10000,
    "top_k": 10,
    "metadata_filter_ratio": 0.25
  },
  "metrics": [
    "insert_rows_per_second",
    "query_p50_ms",
    "query_p95_ms",
    "query_p99_ms",
    "max_rss_mb",
    "store_size_bytes"
  ],
  "thresholds": {
    "query_p95_ms": { "max": 50 },
    "insert_rows_per_second": { "min": 20000 },
    "max_rss_mb": { "max": 4096 }
  },
  "artifacts": {
    "preserve_metrics": true,
    "preserve_store_on_failure": true
  }
}
```

Compiled workload adapters should:

- Create or open the test store.
- Generate or load records.
- Execute the workload through the requested product surface.
- Measure latency, throughput, memory, file size, and error rates.
- Write structured metrics.
- Preserve logs and stores on failure.
- Return pass or fail against thresholds.

Use directly compiled tests only when JSON cannot express the mechanics cleanly. Examples include
crash injection, filesystem fault injection, linearizability checking, model-based operation
generation, fuzz replay internals, C ABI ownership stress, and precise latency measurement loops.
Even then, register the compiled test through a vector or manifest so the harness can report it
consistently:

```json
{
  "schema_version": 1,
  "id": "crash.store.commit-interrupt.test-0001",
  "type": "compiled",
  "adapter": "store_crash_commit_interrupt",
  "priority": "H4",
  "modes": ["release", "soak"],
  "expect": {
    "acknowledged_writes_lost": 0,
    "invalid_objects_visible": 0
  }
}
```

## Result and Inventory Contracts

The first implementation must write stable machine-readable result files. HTML reports are derived
from these files, not separate sources of truth.

`run.json` records:

- Run id.
- Artifact id.
- Mode.
- Started and ended timestamps.
- Harness version.
- Host metadata.
- Selected suite list.
- Schema versions.
- Result file paths.

`results.json` records:

- Test id.
- Suite id.
- Adapter name.
- Status.
- Duration.
- Work directory.
- Artifact paths.
- Captured stdout and stderr paths.
- Normalized output path.
- Metrics path when present.
- Failure id when present.

`coverage.json` records:

- Surface coverage by taxonomy group.
- Operation coverage by inventory item.
- Parameter and parameter-value coverage when operation specs exist.
- Required-case coverage.
- Negative coverage.
- Boundary coverage.
- Stateful-sequence coverage.
- Deferred surfaces.
- Non-goal surfaces.
- Release-blocking coverage gaps.

`failures.json` records:

- Failure id.
- Test id.
- Failure class.
- Message.
- Expected value.
- Actual value.
- Reproduction command.
- Artifact paths.
- Waiver id when a waiver is applied.

`surface-inventory.json` records:

- Artifact id.
- Extracted binary version.
- Extracted binary hash.
- CLI command list.
- CLI subcommand list.
- CLI help output hashes.
- Capability records when available.
- Public error-code records when available.
- Schema version.

The initial drift check compares `surface-inventory.json` to an expected inventory under
`expected/inventory/`. A missing expected inventory is a harness configuration failure. A changed
inventory is a contract drift failure unless the changed surface is marked as expected and owned.

The HTML report must show:

- Summary counts.
- Failed tests first.
- Failure class and owner.
- Reproduction command.
- stdout and stderr artifact links.
- Metrics table.
- Surface inventory summary.
- Surface drift summary.
- Operation coverage summary.
- Parameter coverage summary when operation specs exist.
- Deferred and non-goal surface summary.
- Waiver summary.
- Links to raw JSON result files.

## Path Rules

Path handling is part of the safety contract.

Rules:

- All vector paths are relative to the harness root unless explicitly documented as artifact paths.
- Absolute paths are rejected in vectors.
- `..` path components are rejected in vectors.
- Each test receives one assigned work directory.
- Test steps may only write inside the assigned work directory.
- Preserved failure artifacts are copied into the run result directory.
- Report links must point only to retained artifacts under `results/` or `reports/`.
- The copied Loom binary path is resolved through the selected artifact id, not through `PATH`.

## Operation Specs and Parameter Coverage

Hand-authored vectors are not enough to cover command and protocol parameter spaces. They are best
for smoke cases, examples, regressions, and high-risk workflows. Exhaustive or systematic parameter
coverage should come from operation specs.

Operation specs live under `operation-specs/`. They describe a public operation, its parameters,
valid values, invalid values, defaults, conflicts, dependencies, state prerequisites, modes, coverage
strategy, oracle, and owner. The harness generates concrete cases from operation specs and records
the generated case ids in results.

Use this split:

- `operation-specs/` defines generated coverage for full parameter spaces.
- `test-vectors/` defines hand-authored examples, regressions, and workflows.
- `fixtures/` provides input material for both generated and hand-authored cases.
- `expected/` provides shared expected output and inventory snapshots.

The first implementation can validate operation specs without generating all cases. Generation should
be introduced after the CLI and inventory adapters are stable.

Each operation spec records:

- Surface.
- Operation id.
- Owner.
- Command or protocol method.
- Parameters.
- Valid values.
- Invalid values.
- Defaults.
- Required parameters.
- Mutually exclusive parameters.
- Parameter dependencies.
- State prerequisites.
- Modes.
- Coverage strategy.
- Oracles.
- Expected failure classes.
- Fixtures used.
- Result metrics when relevant.

### Coverage Strategies

The harness should support multiple parameter-space strategies. One strategy does not fit every
operation.

| Strategy | Use when | Example |
| --- | --- | --- |
| `exhaustive` | Few combinations and high contract risk | output format flags, simple boolean flags |
| `pairwise` | Many mostly independent parameters | file type x output format x store state |
| `boundary` | Numeric, size, count, path, or limit parameters | 0 bytes, 1 byte, max path, large file |
| `negative_matrix` | Invalid input, conflicts, malformed values, denied state | missing file, bad digest, invalid flag |
| `stateful_sequence` | Behavior depends on prior operations | cas put, cas get, cas delete, cas get |
| `property_generated` | Large input space with invariants | random bytes, digest, retrieve same bytes |
| `sampled_load` | Scale and performance | 1k, 100k, 1m records |
| `compatibility_matrix` | Platform, runtime, profile, package combinations | macOS x Linux x ARM64 x x86_64 |

Rules:

- Use `exhaustive` only when the combination count is small or the contract risk justifies it.
- Use `pairwise` for large independent parameter sets.
- Always include `boundary` cases for sizes, paths, counts, and limits.
- Always include `negative_matrix` cases for required arguments, invalid values, conflicts, and denied
  authority.
- Use `stateful_sequence` for stores, history, sync, sessions, transactions, and lifecycle behavior.
- Use `property_generated` for canonical identity, digest, import/export, and codec invariants.
- Use `sampled_load` for stress and scale, not for normal command correctness.

### CLI CAS Example

Do not encode every CAS combination as separate hand-authored files. Define the operation model once
and let the harness generate the systematic cases.

Example operation spec:

```json
{
  "schema_version": 1,
  "id": "cli.commands.cas",
  "surface": "cli",
  "command": ["loom", "cas"],
  "owner": "storage",
  "operations": [
    {
      "name": "put",
      "argv_prefix": ["loom", "cas", "put"],
      "parameters": [
        {
          "name": "input",
          "kind": "positional",
          "required": true,
          "values": [
            { "id": "small_text", "fixture": "fixtures/files/small.txt" },
            { "id": "empty_file", "fixture": "fixtures/files/empty.bin" },
            { "id": "binary", "fixture": "fixtures/files/random-1k.bin" },
            { "id": "large_file", "fixture": "fixtures/files/random-10m.bin", "modes": ["release"] },
            { "id": "missing_file", "path": "missing.bin", "expect_failure": "not_found" }
          ]
        },
        {
          "name": "format",
          "kind": "option",
          "argv": "--format",
          "values": ["json", "text"]
        },
        {
          "name": "store",
          "kind": "option",
          "argv": "--store",
          "values": [
            { "id": "new_store", "fixture": "fixtures/stores/new" },
            { "id": "existing_store", "fixture": "fixtures/stores/basic.loom" },
            { "id": "locked_store", "fixture": "fixtures/stores/locked.loom", "expect_failure": "locked" }
          ]
        }
      ],
      "coverage": {
        "strategy": "pairwise",
        "required_cases": [
          "small_text+json+new_store",
          "missing_file+json+new_store"
        ],
        "exhaustive_when_values_lte": 24
      },
      "oracles": [
        "exit_code",
        "stdout_schema",
        "digest_roundtrip",
        "store_reopen"
      ]
    }
  ]
}
```

The generated CAS matrix should cover:

- Commands: `put`, `get`, `list`, and `delete` when exposed.
- Input content: empty file, small text, binary bytes, Unicode filename, large file, duplicate content,
  and many small files.
- Store state: new store, existing store, encrypted store, wrong passphrase, locked store, corrupt
  store, read-only path.
- Digest values: valid digest, unknown digest, malformed digest, wrong algorithm or profile, and
  accepted encoding variants.
- Output modes: JSON, human text, quiet mode, and verbose mode when exposed.
- Error behavior: missing input, extra positional argument, invalid flag, conflicting flags,
  permission denied, and disk-full when fault injection exists.
- Invariants: same bytes produce the same digest, retrieved bytes match input bytes, duplicate put
  does not duplicate logical content, list includes stored digest, delete behavior is stable, reopen
  preserves object, and CLI, binding, and C ABI agree on digest.

### Parameter Coverage Accounting

Coverage accounting should report more than whether a command has one test.

It should report:

- Surface coverage.
- Operation coverage.
- Parameter coverage.
- Parameter-value coverage.
- Required-case coverage.
- Pairwise or exhaustive coverage.
- Boundary coverage.
- Negative coverage.
- Stateful-sequence coverage.
- Property-generated coverage.
- Load-sample coverage.

The drift detector should fail when an inventory item has no operation spec, no vector, and no
documented deferred or non-goal status. The coverage accountant should fail release mode when an
enforced operation's required cases are missing.

### Build-Out Guidance For AI-Assisted Implementation

An AI assistant implementing the harness should use the following order for surface coverage:

1. Implement surface inventory extraction before writing deep tests.
2. Implement CLI command tree inventory first.
3. Add expected inventory snapshots.
4. Add operation specs for the first CLI commands.
5. Generate simple CLI cases from operation specs.
6. Add hand-authored regression vectors only for cases that need explicit workflow or fixture control.
7. Add coverage accounting for inventory items with no operation spec or vector.
8. Add parameter coverage reports before expanding to MCP, hosted projections, C ABI, and bindings.

The key rule is that newly discovered surfaces must become either covered, deferred, or release
blocking. They should not disappear into prose or remain visible only in logs.

## Failure Taxonomy

The harness should classify failures consistently.

Required classes:

- **Product regression:** Loom behavior changed unexpectedly.
- **Contract drift:** public surface changed without matching contract or test updates.
- **Spec gap:** implementation and spec disagree or the spec lacks an answer.
- **Coverage gap:** a surface or requirement exists without required test coverage.
- **Harness failure:** runner, fixture, environment, or oracle failed.
- **Flake:** failure is nondeterministic and not yet attributed.
- **Environment failure:** machine, dependency, network, runtime, or hosted dependency failed.
- **Performance regression:** metric crossed a threshold.
- **Security regression:** authority, leak, dependency, or hostile-input suite failed.
- **Waived known gap:** failure has an owner-approved, unexpired waiver.

Flakes are not passing tests. A flaky suite needs captured seed, command, environment, artifacts,
reproduction notes, owner, expiration, and a decision about whether it blocks release.

## Regression Promotion

The harness should convert confirmed product bugs into permanent regression assets.

Rules:

- Every confirmed product bug gets a regression asset.
- The asset can be a vector, fixture, corpus sample, compiled adapter case, golden store, or expected
  output.
- Fuzz and hostile-input failures are minimized before promotion where practical.
- The regression asset records the failure class, owner, linked issue or decision, original failing
  run, and affected product surface.
- A bug is not considered fully closed until the regression asset fails on the broken behavior and
  passes on the fixed behavior.
- Repeated failures in the same area should update coverage accounting instead of staying as isolated
  incidents.

## Governance and Waivers

The harness needs governance because scheduled and release results become meaningless if failures are
ignored.

Rules:

- Every suite has an owner.
- Every public surface has an owner.
- Every waiver has an owner, reason, expiration date, affected surfaces, affected suites, and linked
  decision.
- Expired waivers fail the harness.
- New unowned public surfaces fail the harness.
- Known gaps are classified as rejected, deferred, non-gating, or release-blocking.
- Release evidence must include active waivers and the reason each waiver is acceptable.

## Run Cadence

Harness priorities define build order. Run cadence defines when suites execute after they exist.

| Cadence | Purpose |
| --- | --- |
| `smoke` | Fast local and CI confidence for harness and product sanity |
| `nightly` | Scheduled subset for drift, smoke, and regression coverage |
| `weekly` | Longer compatibility, platform, and fuzzing runs |
| `release` | Blocking evidence bundle before shipping |
| `soak` | Long-running performance, fuzzing, concurrency, and reliability runs |
| `quarterly` | Enterprise audit, disaster recovery, compliance, and red-team evidence |
| `manual` | Human-led suites requiring credentials, special hardware, external systems, or security review |

The broadest automated run is `release`, not `nightly`. A release run is allowed to take hours and
should exercise the most complete automated harness coverage available for the artifact. A nightly run
is a smaller subset that keeps drift, core smoke coverage, and important regressions visible between
release gates.

## Initial Dependency Choices

The independent runner should keep dependencies small and permissively licensed.

Recommended Rust crates:

- `clap` for command-line parsing.
- `serde`, `serde_json`, and `toml` for manifests, vectors, and result files.
- `schemars` for generating JSON Schema from Rust types.
- `jsonschema` for validating committed schemas.
- `tempfile` for isolated work directories.
- `walkdir` for discovery.
- `regex` for assertions.
- `sha2` or `blake3` for artifact and output hashing.
- `camino` for UTF-8 path handling.
- `tera` or handwritten HTML for reports.

Dependency decisions should preserve the harness trust model. The runner should not require a shell
for normal command execution and should not use dependencies that execute embedded scripts from test
vectors.

## Initial Definition of Done

The first implementation is complete when all of these are true:

- `loom-harness validate` validates all committed schemas, suite manifests, and seed vectors.
- `loom-harness inventory --artifact <artifact>` writes `surface-inventory.json`.
- `loom-harness run --suite suites/smoke.toml --artifact <artifact>` creates a complete result
  directory under `results/<artifact>/smoke/<timestamp>/`.
- The run writes `run.json`, `results.json`, `failures.json`, `metrics.json`, and
  `surface-inventory.json`, and `coverage.json`.
- The report command writes `reports/<artifact>/smoke/<timestamp>/index.html`, `results.json`, and
  `junit.xml`.
- The runner validates committed operation specs even if full case generation is not yet implemented.
- A passing CLI vector passes.
- A deliberately failing CLI vector fails with failure class `Product regression`.
- A malformed vector fails validation before execution with exit code `2`.
- A missing artifact fails with exit code `3`.
- A changed CLI inventory fails drift with failure class `Contract drift`.
- An enforced inventory item with no vector, operation spec, deferred status, or non-goal status is
  reported as a release-blocking coverage gap.
- A vector attempting an absolute path or `..` path fails validation before execution.
- The harness test suite covers schema validation, CLI adapter execution, inventory extraction, result
  writing, coverage accounting, report generation, path rejection, and exit-code mapping.
- The first implementation can run without network access.

## Build-Out Roadmap

### Milestone 0: Harness Can Run

Target priority: H0.

Deliver:

- Runner.
- Suite manifest format.
- Result schema.
- Artifact capture.
- Fixture layout.
- Failure taxonomy.
- Basic report.
- Run-mode job skeleton.

Success condition:

- A trivial suite runs in local, scheduled, and release modes and leaves durable evidence.

### Milestone 1: Drift Is Visible

Target priority: H1.

Deliver:

- CLI inventory.
- C ABI inventory.
- IDL inventory.
- Primary binding export inventory.
- Capability inventory.
- Error-code inventory.
- Expected inventory snapshots.
- Initial CLI operation specs.
- Drift report.
- Surface ownership metadata.

Success condition:

- Adding, removing, or renaming a public surface fails scheduled and release modes until ownership and
  coverage metadata are updated.
- Enforced CLI inventory items without operation specs, vectors, deferred status, or non-goal status
  are reported as release-blocking coverage gaps.

### Milestone 2: Core Loom Works As A Product

Target priority: H2.

Deliver:

- Canonical vector runner.
- Negative vector runner.
- Golden `.loom` store runner.
- Black-box CLI workflows.
- C ABI workflows.
- Primary binding parity workflows.
- Files, history, SQL, vector, sync, and reopen workflows.
- Initial spec coverage report.

Success condition:

- The release harness can prove the core product works through built artifacts, not only internal unit
  tests.

### Milestone 3: Bad Inputs Are Safe

Target priority: H3.

Deliver:

- Small fuzz corpus runner.
- Malformed store suite.
- Corrupt object graph suite.
- Authorization negative suite.
- Secret-leak suite.
- Mutation or fault-seeded smoke suite.

Success condition:

- The scheduled or release harness catches malformed input, denied authority, corrupt storage, and
  seeded product faults.

### Milestone 4: Production Conditions Are Exercised

Target priority: H4.

Deliver:

- Crash injection.
- Filesystem fault injection.
- Concurrency suite.
- Performance baseline suite.
- Resource exhaustion suite.
- Upgrade suite.
- Backup and restore suite.
- Platform matrix.
- Real-client interoperability smoke tests.

Success condition:

- The harness produces reliability, scale, and compatibility evidence that can block a release.

### Milestone 5: Enterprise Evidence Is Repeatable

Target priority: H5.

Deliver:

- Release evidence bundle.
- Compliance and privacy evidence.
- Observability and operability suite.
- Disaster recovery drills.
- Chaos and dependency-failure suite.
- Supply-chain and provenance evidence.
- Extended real-client matrix.
- Quarterly security and red-team evidence workflow.

Success condition:

- Loom can produce repeatable audit-grade evidence for enterprise security, privacy, reliability,
  operations, disaster recovery, compatibility, and supply chain.

## Initial Nightly Shape

The first useful nightly should be intentionally small.

It should run:

- Harness self-check.
- Surface inventory extraction.
- Drift comparison.
- Canonical vector smoke suite.
- Golden `.loom` reopen suite.
- CLI smoke workflow.
- C ABI smoke workflow.
- Primary binding export check.
- Capability inventory check.
- Error-code inventory check.
- Evidence bundle creation.

This does not prove Loom is enterprise-ready. It proves the harness is alive, it can see Loom's public
surfaces, and it can prevent silent surface expansion.

## Long-Term Release Shape

The mature release gate should run:

- Full surface inventory and drift checks.
- Full canonical and negative conformance vectors.
- Full promoted-spec coverage matrix.
- Black-box CLI, C ABI, binding, MCP, and hosted projection suites.
- Core end-to-end workflows.
- Cross-language parity suites.
- SQL, filesystem, vector, sync, and hosted conformance matrices.
- Fuzz regression corpus.
- Malformed and corrupt storage suites.
- Authorization and capability negative suites.
- Secret-leak checks.
- Crash consistency suite.
- Concurrency and linearizability suite.
- Performance regression suite.
- Upgrade and migration suite.
- Backup and restore suite.
- Real-client interoperability smoke suite.
- Evidence bundle and coverage accounting.

## Long-Tail Suites

These suites should be planned from the beginning but do not need to block early harness milestones:

- Extended platform matrix.
- Long-running fuzzing.
- Very large performance datasets.
- Deep chaos testing.
- Quarterly disaster recovery drill.
- Compliance and privacy audit packs.
- Manual red-team campaigns.
- Formal models for complex state machines.
- Rare ecosystem clients.
- Future MCP primitives and hosted protocols.
- Exploratory compute, workflow, and statechart surfaces until promoted.

## Practical Build Order

The highest-value order is:

1. Build H0 so the harness can run and preserve evidence.
2. Build H1 so public surface drift becomes visible.
3. Build the first H2 product workflows and canonical vector checks.
4. Add H3 hostile-input and authority negatives.
5. Add H4 crash, concurrency, performance, upgrade, and real-client coverage.
6. Add H5 enterprise evidence, compliance, disaster recovery, and long-tail suites.

The key constraint is that H1 should not wait for full conformance. A shallow drift harness that sees
every public surface is more valuable early than a deep suite that covers only one surface.

Decision Points: none.
