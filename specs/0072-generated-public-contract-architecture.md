# 0072 - Generated Public Contract Architecture

Status: Proposed

## 1. Purpose

Loom exposes one conceptual product through local Rust clients, daemon clients, remote clients,
hosted dispatch, a C ABI, language bindings, CLI commands, MCP tools, capability reports, and
conformance tests. Those surfaces must not depend on each team or wrapper manually rediscovering the
same operation contract.

This specification defines a generated public-contract architecture in which:

- `idl/loom.idl` is the authoritative declaration of public operations and public data types.
- One validated semantic model is built from the IDL.
- Every low-level projection consumes that same model.
- Every operation is either projected, explicitly excluded with a reason, or rejected by generation.
- Handwritten product ergonomics remain possible above generated, mechanically complete adapters.
- Omissions become generation or compile failures.
- Semantic forwarding mistakes become conformance failures.

The architecture follows a ports-and-adapters model. The IDL defines the port. Generated adapters
connect local, remote, hosted, ABI, binding, CLI, MCP, capability, and conformance surfaces to that
port. Product-specific facades may add ergonomic behavior, but they cannot redefine the underlying
operation contract.

## 2. Problem Statement

The current repository already generates important remote-protocol artifacts from `idl/loom.idl`,
but the generator models only interface name, method name, return type, and arguments. Other public
surfaces still rely on handwritten registries, wrappers, schemas, and completeness checks.

That split creates recurring failure modes:

- A method exists in the IDL but is absent from one binding.
- A wrapper substitutes text for bytes, collapses null into an empty value, or narrows an integer.
- A local method exists but the daemon or remote implementation is missing.
- MCP input or output JSON Schema does not match the actual structured value.
- CLI and MCP routes bypass the shared client authority.
- Capability reporting claims support that is compiled but not configured, authorized, or ready.
- A generated trait proves that a method name exists but does not prove its authorization,
  idempotency, ownership, or platform behavior.
- Temporary inventories and task-specific scripts become the only proof of cross-surface parity.

These failures are architectural. Adding another handwritten checklist after each incident does not
make the contract complete by construction.

## 3. Current Source-Backed State

The following facts describe the current tree, not the target:

| Current behavior | Source |
| --- | --- |
| The existing generator declares `idl/loom.idl` as its source of truth and parses interface method signatures. | `crates/loom-remote-codegen/src/main.rs:1` |
| The parser's semantic record contains interface, method name, return type, and argument pairs. | `crates/loom-remote-codegen/src/main.rs:16` |
| The generator emits a method registry, generated API traits, remote-client implementations, and hosted dispatch. | `crates/loom-remote-codegen/src/main.rs:752` |
| `LocalLoomClient` implements the generated service traits and bridges them to in-process behavior. | `crates/loom-client/src/service.rs:1`, `crates/loom-client/src/service.rs:26` |
| The C ABI can call generated client traits, but public exports and type conversions remain substantially handwritten. | `crates/loom-ffi/src/chat.rs:5`, `crates/loom-ffi/src/chat.rs:29`, `crates/loom-ffi/src/chat.rs:122` |
| `cbindgen` renders `include/loom.h` from Rust exports and checks header drift. | `crates/loom-ffi/cbindgen.toml:1`, `justfile:325` |
| The IDL states that it describes the C ABI and direct bindings, while hosted protocols and generated binding schemas are projections. | `idl/loom.idl:1` |
| Generated artifacts are committed and have a generator `--check` mode. | `crates/loom-remote-codegen/src/main.rs:787` |

The existing generated service traits are a useful foundation. This specification extends that
foundation rather than replacing generation with a second independent registry.

## 4. Goals

0072 must achieve all of the following:

1. Define one complete semantic contract for every public operation and type.
2. Parse and validate that contract once per generation run.
3. Generate mechanically complete low-level adapters for every supported projection.
4. Require explicit projection metadata for CLI, MCP, ABI, bindings, remote, and hosted surfaces.
5. Preserve cross-language value semantics, ownership, errors, and concurrency behavior.
6. Generate or mechanically validate capability declarations and conformance obligations.
7. Support local, daemon, remote, and hosted parity without duplicating business logic.
8. Permit handwritten ergonomic facades only above generated low-level adapters.
9. Remove superseded handwritten registries, task-specific inventories, and compatibility scaffolding.
10. Provide a controlled migration that keeps each interface buildable and testable.

## 5. Non-Goals

0072 does not define:

- New facet storage semantics.
- New hosted wire protocols.
- Daemon process lifecycle or listener ownership.
- Product-level CLI command names, command grouping, help prose, or interactive workflows.
- Product-level MCP tool grouping or naming without explicit projection declarations.
- New facet behavior.
- User-interface generation.
- A stable-release compatibility promise before Loom has a stable release.

0072 may detect missing public operations or projection metadata. The owning facet specification must
define the behavior before the operation is added.

## 6. Design Principles

### 6.1 One authority, multiple projections

The IDL and its imported metadata define the operation contract. Generated artifacts are projections,
not independent authorities.

### 6.2 Mechanical completeness

For every public operation, each projection must have one of these states:

- `generated`: the projection emits an implementation or adapter.
- `handwritten_facade`: a generated low-level adapter exists and a handwritten ergonomic facade uses it.
- `excluded`: the projection is inapplicable and carries a validated reason code.

An unclassified operation is a generation error.

### 6.3 Generated core, handwritten ergonomics

Generation owns repetitive and correctness-sensitive work:

- operation identifiers;
- type mappings;
- request and response records;
- low-level client methods;
- dispatch;
- ABI entry points or mandatory adapter skeletons;
- schema declarations;
- capability descriptors;
- conformance case manifests.

Handwritten code may own:

- command hierarchy and help text;
- friendly overloads and builders;
- language-idiomatic convenience APIs;
- orchestration across multiple generated operations;
- UI behavior.

Handwritten code must call the generated adapter and must not duplicate validation, authorization,
transport selection, or wire encoding.

### 6.4 Compile-time and conformance enforcement

The architecture uses three enforcement levels:

| Failure | Required detection |
| --- | --- |
| Operation or projection omitted | IDL validation, generation failure, or compile failure |
| Generated artifact stale | Generator check |
| Type mapping or schema drift | Snapshot and schema-conformance tests |
| Wrong forwarding behavior | Cross-surface conformance tests |
| Capability overclaim | Capability agreement tests |
| Unsupported target behavior | Explicit capability state and exclusion reason |

### 6.5 No hidden fallback

A projection must not silently fall back to a different method, transport, value representation, or
authorization path. Any fallback is declared in the contract and covered by conformance tests.

## 7. Authoritative Semantic Model

Generation must parse the IDL into one immutable, validated intermediate representation, called the
Contract IR in this specification.

The Contract IR is the only input to projection emitters. Emitters must not reparse IDL text or
maintain parallel operation inventories.

### 7.1 Contract IR contents

The Contract IR must represent:

- modules and namespaces;
- interfaces;
- operations;
- structs;
- enums;
- aliases and opaque handles;
- scalar and generic types;
- field and parameter order;
- required and nullable semantics;
- documentation intended for public generated surfaces;
- stable symbolic and numeric identifiers;
- projection metadata;
- lifecycle and compatibility metadata.

### 7.2 Operation semantics

Every operation must declare:

| Property | Required meaning |
| --- | --- |
| Stable identity | Interface ID, operation ID, and canonical symbolic name |
| Invocation shape | Unary, server stream, client stream, bidirectional stream, or local-only |
| Effect class | Read, mutation, administrative, or destructive |
| Idempotency | Inherently idempotent, requires idempotency key, retry-unsafe, or not applicable |
| Concurrency | Compare token or entity tag behavior, conflict result, and retry contract |
| Authentication | Whether a principal or session is required |
| Authorization | Owning authorization domain and action |
| Availability | Required features, platforms, runtime dependencies, and configuration |
| Errors | Stable error codes the operation may expose |
| Projection | Inclusion and explicit names or exclusions for each target |
| Compatibility | Introduced state, deprecation state, and replacement operation when applicable |

Defaults are allowed only when they are unambiguous and validated. Security, mutation, destructive,
and availability properties must not be inferred from operation names.

### 7.3 Type semantics

Every type must preserve:

- null distinct from empty;
- text distinct from arbitrary bytes;
- signed and unsigned integer width;
- finite floating-point constraints where applicable;
- enum identity and unknown-value behavior;
- field order where canonical encoding depends on it;
- UTC timestamp boundaries;
- digest, entity tag, UUID, and handle domain identity;
- borrowing, ownership, allocation, and release requirements at ABI boundaries.

Known structured values must use typed records. Opaque JSON or opaque bytes are valid only when the
owning contract intentionally defines an extensible or encoded payload.

### 7.4 Stable identifiers

Stable IDs must be explicit or deterministically allocated from a checked registry. Renaming a symbol
must not silently change its stable identity. Reusing a retired ID is forbidden.

Before stable release, breaking changes may replace draft shapes, but the generator must still report
the change and update every projection and conformance vector together.

## 8. IDL Metadata

The IDL requires syntax or an associated authoritative metadata form for semantics that signatures
cannot express.

The selected representation must satisfy these constraints:

- it is colocated with, imported by, or mechanically keyed to IDL declarations;
- stale references and unknown operation names fail validation;
- one operation cannot receive contradictory metadata from multiple files;
- projection emitters receive normalized values only through the Contract IR;
- generated files do not become metadata authorities.

An associated metadata file is acceptable when it keeps the IDL readable, but it must be loaded by
the same parser and validated as one contract. Ad hoc emitter-specific tables are forbidden.

## 9. Generator Architecture

The generator must be organized as reusable components rather than one emitter-oriented binary:

1. **Source loader** reads IDL and authoritative metadata.
2. **Parser** builds a syntax model with source spans.
3. **Semantic analyzer** resolves names and constructs the Contract IR.
4. **Validator** rejects incomplete or contradictory contracts.
5. **Projection planner** determines required outputs and exclusions.
6. **Emitters** render deterministic artifacts from the Contract IR.
7. **Formatter adapters** invoke deterministic language formatters.
8. **Manifest writer** records generated files, source digest, generator version, and contract digest.
9. **Check mode** regenerates in memory and rejects drift, orphaned artifacts, and unexpected files.

The reusable parser, Contract IR, validators, and projection planner must live behind a library
boundary. A thin command-line binary may orchestrate generation.

### 9.1 Determinism

Given identical authoritative inputs and tool versions, generation must produce byte-identical output.
Ordering must derive from stable IDs or declared source order, never hash-map iteration.

### 9.2 Diagnostics

Generation errors must report:

- source path and span;
- interface and operation;
- violated rule;
- affected projection;
- concrete remediation.

### 9.3 Escape hatches

An operation may require handwritten platform code. The generator may emit a required adapter trait or
skeleton, but the exception must:

- be declared per operation and projection;
- name the handwritten implementation;
- preserve generated type and error contracts;
- fail compilation when missing;
- have conformance coverage.

An escape hatch cannot exclude an entire interface merely because one method needs platform code.

## 10. Canonical Service Port

Generated Rust traits form the canonical service port for in-process and transported operation
execution.

- `LocalLoomClient` implements the complete port.
- `RemoteLoomClient` implements the same port through transport calls.
- Hosted dispatch accepts the same port rather than calling lower-level facet APIs directly.
- Daemon-backed CLI and MCP clients select an implementation of the port.
- Direct-local execution uses the same operation contract.

Transport, session, and deployment differences are adapters. They must not fork business semantics.

The generated supertrait must make a missing interface implementation a compile error. Platform
availability is expressed through capabilities and stable runtime errors, not by omitting methods
from a platform binding.

## 11. C ABI Projection

The C ABI is the lowest common denominator for native language bindings and must be generated or
mechanically complete from the Contract IR.

### 11.1 Required ABI contract

The ABI projection must define:

- symbol naming and calling convention;
- scalar width and signedness;
- UTF-8 text validation;
- arbitrary byte buffers including embedded NUL;
- nullable values;
- input borrowing duration;
- output ownership;
- allocator and release functions;
- handles and handle invalidation;
- asynchronous task ownership, polling, cancellation, and result taking;
- callback lifetime and thread rules;
- thread safety and reentrancy;
- stable error code and error-detail retrieval;
- struct layout and ABI alignment;
- stream lifecycle;
- compatibility and symbol retirement.

### 11.2 Generated exports

The preferred form is generated ABI exports and generated conversion code. Where Rust attributes or
platform behavior require handwritten code, generation must emit a mandatory typed adapter boundary.

`cbindgen` may continue to render the public header from Rust exports. It does not prove that every IDL
operation has an ABI export. The 0072 generator owns that completeness proof.

### 11.3 ABI manifests

Generation must emit a machine-readable ABI manifest containing symbols, signatures, stable operation
IDs, ownership rules, error shape, and feature requirements. CI compares the manifest to the IDL
contract and uses it to drive binding completeness checks.

## 12. Language Binding Projection

Each language binding has two layers:

1. A generated low-level adapter that is mechanically complete.
2. An optional handwritten ergonomic facade that is idiomatic for the language.

This applies to:

- C++;
- JVM;
- Android;
- Swift for iOS and macOS;
- React Native;
- Node.js;
- Python;
- WASM.

### 12.1 Mapping requirements

| IDL concept | Binding requirement |
| --- | --- |
| `optional<string>` | Preserve null independently from empty text |
| `bytes` | Preserve arbitrary bytes without UTF-8 conversion |
| `u64` | Use a native unsigned type when exact, otherwise a validated exact representation |
| enum | Generate a typed enum and define unknown-value handling |
| timestamp | Expose a UTC-aware native type or a validated canonical UTC boundary |
| digest and entity tag | Preserve domain identity instead of accepting arbitrary unvalidated text where practical |
| struct | Generate a typed record with required and nullable fields |
| task or stream | Use the language's async or iterator conventions without changing cancellation semantics |
| handle | Define ownership, close behavior, and use-after-close failure |

### 12.2 Binding completeness

Every binding build must consume the ABI or language manifest and prove:

- all applicable operations are present;
- excluded operations carry approved reason codes;
- signatures match;
- feature gates match;
- ownership and release paths exist;
- no orphaned handwritten low-level operation remains.

## 13. CLI Projection

The generator must not invent product command grammar.

Every operation must instead be classified as:

- exposed by a declared CLI command;
- consumed internally by a declared CLI workflow;
- intentionally hidden from CLI with a reason.

CLI projection metadata must identify:

- command owner;
- generated request adapter;
- generated result type;
- effect and destructive annotations;
- idempotency and compare-token requirements;
- capability requirements;
- stable error presentation.

CLI command implementations must call the shared typed client port. A command may orchestrate several
operations, but it must not bypass their generated validation, authorization, or transport contract.

The CLI projection must enforce this boundary through production types rather than test-only source
inspection. Ordinary command runners receive typed read, mutation, or control capabilities derived
from operation metadata. A mutation runner cannot acquire direct mutable store authority; it can
publish only through the generated mutation capability. Physical store administration and other
reviewed non-IDL operations use separate explicit capabilities.

Generation must emit a compile-checked CLI operation registry. An IDL operation without CLI
classification fails validation.

## 14. MCP Projection

The generator must not invent product tool grouping, but it must generate and validate typed tool
contracts.

MCP projection metadata must identify:

- tool owner and public tool name;
- request record;
- response record;
- read-only, idempotent, destructive, and open-world annotations;
- capability requirements;
- stable errors;
- whether the operation is direct or part of an orchestrated tool.

JSON Schema for tool input and structured output must derive from typed Contract IR definitions.
Handwritten `{}` schemas for known structured values are forbidden.

Runtime tool responses must be validated against the same generated schema in focused tests.
Schema snapshots must detect:

- missing required fields;
- newly required fields;
- additional fields where the contract is closed;
- object values degraded into strings;
- nullability drift;
- integer-width drift;
- response envelope drift.

## 15. Remote and Hosted Projections

Remote client and hosted dispatch generation must share:

- method IDs;
- request and response codecs;
- streaming classification;
- idempotency requirements;
- authorization metadata;
- capability requirements;
- stable errors.

Hosted handlers must use the canonical service port and policy enforcement path. A generated dispatch
adapter must not call lower-level store APIs in a way that bypasses authentication, authorization,
auditing, or save behavior.

Local, daemon, remote, and hosted operation results must be semantically equivalent for the same
request and state.

## 16. Capability Projection

Capabilities must derive from the same operation and projection metadata instead of a parallel
handwritten matrix.

The generated capability model must distinguish at least:

- not compiled;
- compiled but runtime dependency absent;
- available but not configured;
- configured but not authorized;
- authorized but not ready;
- ready;
- unsupported on the target.

Each non-ready state must include a stable reason code. Capability declarations must identify the
operations they cover. A capability cannot report `ready` if a required operation is excluded or its
runtime dependency is absent.

## 17. Conformance Projection

The Contract IR must generate a conformance manifest that enumerates required semantic cases for each
operation and type.

The manifest must cover applicable cases including:

- null versus empty;
- arbitrary bytes and embedded NUL;
- minimum and maximum numeric boundaries;
- invalid narrowing and overflow;
- unknown enum values;
- canonical struct encoding;
- stable error mapping;
- idempotency replay;
- compare-token conflict;
- destructive-operation annotation;
- handle ownership and use after close;
- task cancellation;
- stream completion and failure;
- local, daemon, remote, and hosted parity;
- capability declaration agreement.

Generation can provide fixtures and harness adapters. Facet-owned tests must provide operation-specific
state and expected behavior.

## 18. Security and Authorization

Security metadata is mandatory for every public operation.

Validation must reject:

- a mutation without an authorization action;
- an administrative or destructive operation without explicit classification;
- a remotely exposed operation marked local-only;
- a projection that weakens authentication requirements;
- a binding that maps secrets through uncontrolled text logging;
- generated debug output that can expose credentials or key material.

Generated dispatch and adapters must preserve policy enforcement points. Authorization decisions remain
runtime behavior, but their required domain and action are contract data.

## 19. API Evolution

Loom is not yet released. 0072 therefore targets one clean current contract rather than preserving
interim generated and handwritten shapes.

The migration must:

- update authoritative definitions;
- regenerate all affected projections;
- update canonical vectors;
- migrate callers in the same bounded batch;
- delete superseded draft adapters and registries;
- avoid permanent compatibility aliases unless explicitly approved.

The architecture must still prepare for stable releases by supporting:

- stable IDs;
- explicit deprecation metadata;
- replacement references;
- generated compatibility reports;
- detection of removed or changed operations;
- a controlled breaking-change review.

## 20. Hermetic Generation and Repository Hygiene

Generation must be runnable from repository-declared tooling and must not depend on a developer's
global mutable state beyond pinned toolchains.

Generated artifacts must:

- carry a generated-file marker;
- be listed in a generated manifest;
- be reproducible;
- be checked for staleness;
- be removed when no longer emitted;
- never be manually edited.

Generator scratch files, task inventories, source-tree TSV files, migration helpers, and temporary
comparison programs must not remain in production source directories.

## 21. Migration Strategy

Migration is interface-by-interface, with explicit proof before deleting handwritten code.

### 21.1 Prerequisite

The active MU-6 recovery and parity work must reach strict completion before 0072 replaces its
temporary proof mechanisms. 0072 may perform source audits and build the generator foundation while
MU-6 finishes, but it must not invalidate MU-6 acceptance evidence.

### 21.2 Prototype

Use two interfaces as prototypes:

- `Document`, because it exercises text, bytes, nullable values, conditional mutation, and result
  records.
- `Chat`, because it exercises a broad mutation surface, bytes, async behavior, entity tags, and
  existing generated-trait use in the C ABI.

The prototype succeeds only when both interfaces prove:

- Contract IR completeness;
- generated Rust service and remote projections;
- generated or mandatory C ABI adapters;
- one native binding and one managed or web binding;
- CLI and MCP classification;
- generated schemas;
- capability agreement;
- cross-surface conformance;
- deletion of superseded handwritten low-level inventory for the migrated operations.

If the prototype requires routine per-operation handwritten forwarding, the generator design has
failed and must be corrected before broader migration.

### 21.3 Migration waves

After the prototype:

1. Migrate foundational store, session, workspace, result, and management interfaces.
2. Migrate files, VCS, CAS, document, KV, SQL, and transfer interfaces.
3. Migrate workflow and collaboration interfaces.
4. Migrate data, search, inference, compute, and observability interfaces.
5. Migrate remaining administrative and platform-specific interfaces.

Each wave must compile and pass focused conformance before the next wave begins.

### 21.4 Buildable batch boundaries

Every implementation batch must leave the repository in a buildable state. A batch may introduce
generated adapters alongside existing adapters, but it must not remove or incompatibly change an
adapter while an existing caller still depends on it.

The batch boundary contract is:

1. Authoritative inputs and generated artifacts agree.
2. The affected Cargo packages compile.
3. Existing unaffected public surfaces remain source-compatible.
4. Every binding changed by the batch compiles through its focused build recipe.
5. Focused conformance for the changed contract passes.
6. No temporary source-tree artifact is required to make the next batch possible.
7. Any compatibility bridge retained across the boundary is explicit, tested, and assigned to a
   later removal task.

Design-only batches satisfy the boundary by changing no runtime or generated artifact. Generator
foundation batches may add new artifacts without switching production callers. Projection batches
switch callers only after their generated replacement exists. Cleanup batches remove old adapters
only after all dependent callers have migrated.

A batch boundary does not require every device, browser, network, or hosted integration suite to run.
Those checks remain dedicated integration evidence. It does require the repository to contain a
coherent source state from which all supported targets can be built.

Within a batch, a narrow task may temporarily leave only its affected package incomplete while the
next immediately dependent task is in progress. Such an intermediate state must not be presented as
a handoff, accepted batch, or safe integration point.

## 22. Testing Strategy

### 22.1 Default checks

Fast deterministic parser, validator, emitter, snapshot, and focused adapter tests belong in the
default Cargo test path.

### 22.2 Integration checks

Cross-process daemon, remote, hosted, native dynamic-library, language runtime, browser, and device
tests remain outside `just ci` and use dedicated `just test-*` recipes.

### 22.3 Required gates

The final architecture requires:

- generator unit tests;
- invalid-contract negative tests;
- deterministic regeneration tests;
- generated-artifact drift check;
- ABI manifest and header agreement;
- per-binding manifest completeness;
- MCP schema snapshots and runtime validation;
- CLI operation classification check;
- capability agreement tests;
- local/daemon/remote/hosted conformance;
- cross-language boundary vectors;
- final `just ci`;
- relevant manual binding and integration recipes.

## 23. Performance Requirements

0072 must not make runtime calls reflect over IDL metadata or dynamically interpret schemas.

- Contract parsing and validation occur at build or generation time.
- Runtime dispatch uses generated static identifiers and typed functions.
- Generated lookup tables use deterministic static data or efficient indexed structures.
- Request and response conversion avoids unnecessary copies.
- Byte buffers preserve ownership and permit zero-copy borrowing where the ABI lifetime contract makes
  it safe.
- Code size and compile-time growth are measured per migration wave.

A performance regression must be justified by correctness or platform constraints and recorded with
measurements.

## 24. Completion Criteria

0072 is complete only when:

1. The Contract IR represents all public IDL interfaces, operations, and types.
2. Every operation has validated semantic and projection metadata.
3. Local, remote, hosted, C ABI, binding, CLI, MCP, capability, and conformance projections consume
   the Contract IR or a generated manifest derived from it.
4. Missing applicable projections fail generation or compilation.
5. Generated MCP schemas match runtime structured values.
6. Generated ABI manifests match exports and the public header.
7. Every supported binding passes manifest completeness and boundary conformance.
8. Local, daemon, remote, and hosted parity is source-backed and tested.
9. Superseded handwritten low-level registries, inventories, and temporary recovery artifacts are
   removed.
10. Generated artifacts are deterministic and current.
11. Security, architecture, API evolution, performance, and cross-language reviews are accepted.
12. The completion evidence in `_QUEUE_0072.md` is satisfied.

## 25. Implementation State

Current state: Not implemented.

The existing remote generator is a partial foundation. It does not yet provide the complete semantic
model, projection coverage, schema generation, capability derivation, ABI completeness, or
cross-language enforcement required by this specification.

Decision Points: none.
