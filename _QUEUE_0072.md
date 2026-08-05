# Queue 0072 - Generated Public Contract Architecture

This is the active working queue for the 0072 generated public-contract architecture. It is separate
from `specs/IMPLEMENTATION-PLAN.md`; do not use this file as a historical changelog.

## Goal

Implement `specs/0072-generated-public-contract-architecture.md` so one validated semantic contract
drives all low-level public projections, omissions fail mechanically, and semantic drift fails
conformance.

Queue type: Implementation

## Definition Of Done

Queue 0072 is complete when every public IDL operation is semantically classified; all applicable
local, remote, hosted, ABI, binding, CLI, MCP, capability, and conformance projections consume the
Contract IR or its generated manifests; superseded handwritten low-level inventories are removed; and
all completion evidence is satisfied.

## Completion State

Current state: Not Started

Current cursor: 10
Next task: Confirm MU-6 strict completion and capture the 0072 migration baseline.

Decision Points: none.

## No Buried Work Rule

Before every status update, pause point, handoff, or final control-return message, audit the response
for future-tense work, prevention work, risks, blockers, follow-ups, or "should do next" statements.

If the response mentions work that is not already represented in the queue, do one of these before
handing off:

- Add it to the Ordered Task List when it is in current scope.
- Add it to Missed Or Hidden Work Found when it needs user vetting.
- Add it to Decision Points when user choice blocks the next action.
- Move it to another queue or planning document when it belongs outside this queue.

Do not describe future work only in chat.

## Decision Log

| Date | Decision | Rationale | Source |
| --- | --- | --- | --- |
| 2026-07-30 | Finish MU-6 before replacing its temporary proof mechanisms. | The recovery work must close against a stable contract before architecture migration deletes or supersedes its evidence paths. | User decision in active thread. |
| 2026-07-30 | Use one rich Contract IR and generate all low-level public projections from it. | Mechanical completeness is more durable than independent handwritten inventories. | User-approved 0072 design discussion. |
| 2026-07-30 | Keep product CLI and MCP ergonomics handwritten above generated typed adapters. | Command and tool design requires product judgment, while request, response, effect, and capability contracts require mechanical consistency. | `specs/0072-generated-public-contract-architecture.md`. |
| 2026-07-30 | Prototype with Document and Chat before broad migration. | Together they exercise text, bytes, nullability, conditional mutation, records, async behavior, and existing ABI forwarding. | `specs/0072-generated-public-contract-architecture.md`. |
| 2026-08-01 | Close MU-6 without a test-only CLI mutation-ownership claim and implement the compile-time ownership boundary in 0072. | The accepted IDL effect metadata is durable groundwork, but production runner capabilities must be designed once across generated projections instead of added as a CLI-only patch. | MU-6j-b2 owner decision. |

## Source Authority Order

When sources disagree, resolve them in this order:

1. Current repository source.
2. User decisions in the active thread.
3. `specs/0072-generated-public-contract-architecture.md`.
4. Other owning specifications.
5. Generated artifacts.
6. Agent inference.

## Assumptions

| Assumption | Why acceptable | Revisit trigger |
| --- | --- | --- |
| The existing generated Rust service traits remain a useful migration foundation. | They already enforce part of local, remote, and hosted method completeness. | Prototype proves their shape cannot carry required semantics without replacement. |
| Contract metadata may be colocated in IDL or imported from one authoritative companion form. | 0072 requires one validated model, not one physical file format. | Metadata becomes duplicated or emitter-specific. |
| Draft public shapes can be replaced without compatibility aliases. | Loom has not been released. | A stable release or external compatibility commitment occurs before migration finishes. |

## Current Source-Backed State

| Claim | Source |
| --- | --- |
| The current generator parses interface signatures into interface, name, return type, and arguments. | `crates/loom-remote-codegen/src/main.rs:16`, `crates/loom-remote-codegen/src/main.rs:47` |
| It emits four Rust artifacts for registry, API traits, remote client, and hosted dispatch. | `crates/loom-remote-codegen/src/main.rs:752` |
| `LocalLoomClient` implements generated service traits. | `crates/loom-client/src/service.rs:1`, `crates/loom-client/src/service.rs:26` |
| C ABI modules can call generated traits but retain handwritten exports and conversions. | `crates/loom-ffi/src/chat.rs:5`, `crates/loom-ffi/src/chat.rs:29`, `crates/loom-ffi/src/chat.rs:122` |
| `cbindgen` owns header rendering and header drift checks. | `crates/loom-ffi/cbindgen.toml:1`, `justfile:325` |
| IDL declares built-in scalar, optional, list, stream, struct, enum, and handle concepts. | `idl/loom.idl:16` |

## Scope Boundary

Queue 0072 includes:

- Contract IR, validation, and deterministic generation.
- Rust service, remote, hosted, C ABI, and binding low-level projections.
- CLI and MCP typed projection classification and schema enforcement.
- Capability and conformance generation.
- Interface-by-interface migration.
- Removal of superseded handwritten low-level registries and temporary inventories.
- Architecture, security, performance, API evolution, and cross-language review gates.

Queue 0072 does not own:

- Facet storage redesign.
- New hosted protocols.
- Daemon lifecycle redesign.
- Product CLI grammar or help design.
- New facet semantics.
- UI generation.

## Priority Definitions

- P0: Blocks the queue goal.
- P1: Required for a correct, durable result.
- P2: Valuable follow-up that must be completed or explicitly re-homed before closure.
- P3: Long-tail work that may be moved with rationale.

## Lift Scale

- 1: Trivial.
- 2-3: Small and clear.
- 4-5: Moderate and bounded.
- 6-8: Large or ambiguous; split before starting.
- 9+: Too large for one task.

Every implementation task in this queue is bounded at lift 5 or lower.

## Research Notes

| Topic | Finding | Source |
| --- | --- | --- |
| Existing generation | The current generator is deterministic enough to compare committed output in `--check` mode, but its parser and model are emitter-oriented. | `crates/loom-remote-codegen/src/main.rs` |
| C header | `cbindgen` can render Rust ABI declarations but cannot prove IDL operation completeness. | `crates/loom-ffi/cbindgen.toml`, `justfile` |
| Binding architecture | Bindings share the C ABI but use separate build systems and language wrappers. | `AGENTS.md`, `bindings/` |

## Completion Evidence

| Evidence | Required? | Result | Notes |
| --- | --- | --- | --- |
| MU-6 strict completion and baseline inventory. | Yes | Pending | Prevents 0072 from invalidating recovery evidence. |
| Contract IR parser, resolver, and negative validation tests. | Yes | Pending | Must cover every semantic rule in 0072. |
| Deterministic generation and stale/orphan artifact checks. | Yes | Pending | Same inputs must produce byte-identical outputs. |
| Document and Chat prototype acceptance. | Yes | Pending | Must satisfy every prototype criterion before broad migration. |
| ABI manifest and `include/loom.h` agreement. | Yes | Pending | `cbindgen` output alone is insufficient. |
| Binding completeness reports for all supported bindings. | Yes | Pending | Exclusions require stable reasons. |
| MCP schema snapshot and runtime response validation. | Yes | Pending | Must catch structured-object degradation and envelope drift. |
| CLI operation classification check. | Yes | Pending | No public operation may be unclassified. |
| Capability agreement tests. | Yes | Pending | Runtime state must not overclaim generated support. |
| Local, daemon, remote, and hosted parity suites. | Yes | Pending | Use dedicated integration recipes where process boundaries exist. |
| Cross-language boundary conformance. | Yes | Pending | Cover null, bytes, widths, enums, ownership, and errors. |
| Architecture, security, evolution, performance, and cleanup reviews. | Yes | Pending | Each review must cite inspected source. |
| `just ci`. | Yes | Pending | Run once after coherent final integration, not after each narrow task. |
| Relevant binding and integration recipes. | Yes | Pending | Run serially and record commands and results. |

## Ordered Task List

Current cursor: 10
Next task: Confirm MU-6 strict completion and capture the 0072 migration baseline.

Status values: Not Started, In Progress, Blocked, Waiting On Decision, Done, Cut.
Evidence types: Source, Test, Review, Artifact, User Decision, External.

| Order | Status | Priority | Lift | Task | Owning specs | Depends on | Output | Verification | User input needed |
| --- | --- | --- | ---: | --- | --- | --- | --- | --- | --- |
| 10 | Not Started | P0 | 3 | Confirm MU-6 strict completion and record the public-surface baseline that 0072 may replace. | 0072, `_MESSEDUP.md` | None | Source-backed baseline of interfaces, projections, temporary proof artifacts, and unresolved gaps. | Source review against current tree and accepted MU-6 evidence. | No. |
| 20 | Not Started | P0 | 4 | Audit the current IDL parser, emitters, generated files, and generation checks. | 0072 | 10 | Ownership map with parsing limitations, emitter inputs, and generated artifact inventory. | Source anchors for every generator path. | No. |
| 30 | Not Started | P0 | 4 | Audit all handwritten low-level operation registries and inventories across Rust, ABI, bindings, CLI, MCP, capabilities, and conformance. | 0072 | 10 | Classified inventory of authoritative, generated, facade, duplicate, and temporary code. | Source review with no unclassified public registry. | No. |
| 40 | Not Started | P0 | 3 | Define stable Contract IR identifiers and canonical ordering rules. | 0072 | 20 | Reviewed identifier and ordering contract. | Design review and deterministic examples. | No. |
| 50 | Not Started | P0 | 4 | Define the Contract IR type model for scalars, generics, structs, enums, aliases, and handles. | 0072 | 20, 40 | Typed IR schema with nullability and width semantics. | Design review against all IDL type forms. | No. |
| 60 | Not Started | P0 | 4 | Define the Contract IR operation model for invocation, effects, idempotency, concurrency, auth, availability, errors, and compatibility. | 0072 | 20, 40 | Typed operation metadata schema. | Design review against representative reads, mutations, streams, admin, and destructive methods. | No. |
| 70 | Not Started | P0 | 3 | Select and specify the authoritative metadata syntax and its relationship to IDL declarations. | 0072 | 50, 60 | Metadata grammar and source-authority rules. | Negative examples prove stale and duplicate metadata fail. | Yes only if source research exposes materially different viable contracts. |
| 80 | Not Started | P0 | 3 | Define projection-state and exclusion-reason enums for every target. | 0072 | 60 | Typed projection classification contract. | Review proves every operation has exactly one state per projection. | No. |
| 90 | Not Started | P0 | 4 | Establish the reusable parser and Contract IR library boundary behind a thin generator binary. | 0072 | 20, 50, 60, 70 | Buildable library API and orchestration binary. | Focused package tests and architecture review. | No. |
| 100 | Not Started | P0 | 4 | Implement source spans, parser diagnostics, and syntax-model tests. | 0072 | 90 | Parser with actionable file and span errors. | Positive and malformed-IDL tests. | No. |
| 110 | Not Started | P0 | 4 | Implement name resolution and Contract IR construction. | 0072 | 90, 100 | Resolved immutable semantic model. | Tests for references, aliases, nested generics, and duplicate names. | No. |
| 120 | Not Started | P0 | 4 | Implement type and nullability validators. | 0072 | 110 | Validation for legal types, widths, nullable boundaries, and structured values. | Negative tests for invalid and ambiguous types. | No. |
| 130 | Not Started | P0 | 4 | Implement operation semantic validators. | 0072 | 110 | Validation for effects, idempotency, compare tokens, auth, errors, and availability. | Negative tests for missing security and mutation metadata. | No. |
| 140 | Not Started | P0 | 4 | Implement projection completeness and exclusion validators. | 0072 | 80, 110 | Build failure for unclassified or contradictory projections. | Negative matrix tests across all projection kinds. | No. |
| 150 | Not Started | P0 | 3 | Implement stable-ID registry validation and compatibility reporting. | 0072 | 40, 110 | Checked ID registry and machine-readable change report. | Rename, removal, reuse, and replacement tests. | No. |
| 160 | Not Started | P0 | 3 | Add a normalized Contract IR snapshot for the current IDL. | 0072 | 120, 130, 140, 150 | Reviewable deterministic semantic snapshot. | Regeneration is byte-identical. | No. |
| 170 | Not Started | P0 | 3 | Run the Contract IR architecture and security design gate. | 0072 | 120, 130, 140, 150, 160 | Accepted review of authority, trust boundaries, defaults, and diagnostics. | Skeptical source and test review. | No. |
| 180 | Not Started | P0 | 4 | Refactor the method-registry emitter to consume only the Contract IR. | 0072 | 170 | Registry output with stable IDs and semantic descriptors. | Existing registry parity plus new metadata tests. | No. |
| 190 | Not Started | P0 | 4 | Refactor generated service traits to consume only the Contract IR. | 0072 | 170 | Complete typed service port and supertrait. | Missing interface implementation produces compile failure. | No. |
| 200 | Not Started | P0 | 4 | Refactor the remote-client emitter to consume shared operation and codec plans. | 0072 | 180, 190 | Generated remote adapter without parallel signature logic. | Focused remote round-trip tests. | No. |
| 210 | Not Started | P0 | 4 | Refactor hosted dispatch to consume shared operation, authorization, and codec plans. | 0072 | 180, 190 | Generated dispatch that preserves PEP and stable errors. | Focused hosted auth, denial, and dispatch tests. | No. |
| 220 | Not Started | P0 | 3 | Generate a manifest of artifacts, source digest, contract digest, and generator identity. | 0072 | 180, 190, 200, 210 | Deterministic generated manifest. | Drift and reproducibility tests. | No. |
| 230 | Not Started | P0 | 4 | Harden check mode to reject stale, orphaned, missing, and unexpectedly handwritten generated artifacts. | 0072 | 220 | Hermetic generation gate. | Mutation tests for each drift class. | No. |
| 240 | Not Started | P0 | 3 | Define the canonical C ABI type, ownership, task, stream, and error mapping. | 0072 | 170 | Reviewed ABI mapping contract. | Cross-check against current FFI and header. | No. |
| 250 | Not Started | P0 | 4 | Generate the machine-readable ABI symbol and ownership manifest. | 0072 | 220, 240 | ABI manifest for every applicable operation and type. | Manifest covers all generated service operations or exclusions. | No. |
| 260 | Not Started | P0 | 4 | Generate C ABI conversion helpers and mandatory adapter traits or skeletons. | 0072 | 240, 250 | Typed conversion layer for null, text, bytes, numbers, records, handles, tasks, and streams. | Boundary unit tests and missing-adapter compile failure. | No. |
| 270 | Not Started | P0 | 4 | Generate C ABI exports for directly projectable operations. | 0072 | 260 | Exported low-level operation surface with stable symbols. | Symbol and behavior tests. | No. |
| 280 | Not Started | P0 | 3 | Validate `cbindgen` header output against the generated ABI manifest. | 0072 | 250, 270 | Header-to-manifest agreement gate. | `just header-check` plus manifest comparison. | No. |
| 290 | Not Started | P0 | 3 | Run the C ABI design and security review gate. | 0072 | 260, 270, 280 | Accepted ownership, lifetime, thread, error, and compatibility review. | Skeptical source review and boundary tests. | No. |
| 300 | Not Started | P0 | 3 | Define the language-binding manifest and common completeness protocol. | 0072 | 250, 290 | Language-neutral binding manifest and checker contract. | Review against every supported binding. | No. |
| 310 | Not Started | P0 | 4 | Implement C++ low-level generation and completeness checking. | 0072 | 300 | Generated C++ adapter beneath ergonomic wrappers. | Focused CMake build and boundary vectors. | No. |
| 320 | Not Started | P0 | 4 | Implement JVM low-level generation and completeness checking. | 0072 | 300 | Generated JVM adapter beneath ergonomic wrappers. | Focused JVM build and boundary vectors. | No. |
| 330 | Not Started | P0 | 4 | Implement Android low-level generation and completeness checking. | 0072 | 300 | Generated Android adapter beneath ergonomic wrappers. | Focused Android build and boundary vectors. | No. |
| 340 | Not Started | P0 | 4 | Implement Swift low-level generation and completeness checking for iOS and macOS. | 0072 | 300 | Generated Swift adapter beneath ergonomic wrappers. | Focused Swift build and boundary vectors. | No. |
| 350 | Not Started | P0 | 4 | Implement React Native low-level generation and completeness checking. | 0072 | 300 | Generated TurboModule adapter beneath ergonomic wrappers. | Focused React Native build and boundary vectors. | No. |
| 360 | Not Started | P0 | 4 | Implement Node.js low-level generation and completeness checking. | 0072 | 300 | Generated Node adapter beneath ergonomic wrappers. | Focused Node build and boundary vectors. | No. |
| 370 | Not Started | P0 | 4 | Implement Python low-level generation and completeness checking. | 0072 | 300 | Generated Python adapter beneath ergonomic wrappers. | Focused Python build and boundary vectors. | No. |
| 380 | Not Started | P0 | 4 | Implement WASM low-level generation and completeness checking. | 0072 | 300 | Generated WASM adapter using the declared authority path. | Focused WASM build and browser-safe boundary vectors. | No. |
| 390 | Not Started | P0 | 3 | Run the cross-language mapping and ergonomics design gate. | 0072 | 310, 320, 330, 340, 350, 360, 370, 380 | Accepted review proving generated low-level completeness and handwritten facade separation. | Binding manifest review and representative source inspection. | No. |
| 400 | Not Started | P0 | 3 | Define CLI operation projection metadata, effect classification, and typed read, mutation, control, and reviewed non-IDL capability rules. | 0072 | 170 | Typed declaration for command owner, orchestration, effects, capabilities, hidden reasons, and allowed authority. | Review representative command families and prove mutation runners cannot request direct mutable-store authority. | No. |
| 410 | Not Started | P0 | 4 | Generate the compile-checked CLI operation registry, typed request adapters, and operation capabilities from the Contract IR. | 0072 | 400 | Registry with no unclassified IDL operation and production capability types that reject mismatched effects. | Classification tests plus compile-failure fixtures for direct or incorrectly typed mutation ownership. | No. |
| 420 | Not Started | P0 | 4 | Migrate CLI client selection and operation routing to the generated service port and typed operation capabilities. | 0072 | 190, 410 | Direct and daemon CLI paths share typed authority; ordinary mutation runners have no direct mutable-store path. | Focused direct/daemon parity tests and negative compile-time ownership proof. | No. |
| 430 | Not Started | P0 | 3 | Define MCP tool projection metadata and annotation rules. | 0072 | 170 | Typed declaration for tool owner, request, response, effects, and capability requirements. | Review against representative read, write, and destructive tools. | No. |
| 440 | Not Started | P0 | 4 | Generate MCP input and structured-output JSON Schemas from Contract IR types. | 0072 | 430 | Typed schemas without handwritten empty object placeholders. | Schema snapshots for nested objects, nulls, arrays, and integers. | No. |
| 450 | Not Started | P0 | 4 | Generate the MCP operation registry and typed request/response adapters. | 0072 | 190, 430, 440 | Mechanically complete MCP low-level projection. | Missing classification fails generation. | No. |
| 460 | Not Started | P0 | 3 | Add MCP runtime structured-output validation against generated schemas. | 0072 | 440, 450 | Focused validation harness for actual tool responses. | Negative tests for missing, additional, stringified, and nullable fields. | No. |
| 470 | Not Started | P0 | 3 | Run the CLI and MCP projection design gate. | 0072 | 410, 420, 450, 460 | Accepted review of ergonomic ownership and generated correctness boundaries. | Skeptical source and schema review. | No. |
| 480 | Not Started | P0 | 3 | Define generated capability descriptors, states, and stable reason codes. | 0072 | 60, 80 | Contract mapping operations to compile, dependency, configuration, auth, readiness, and support states. | Design review against current capability model. | No. |
| 490 | Not Started | P0 | 4 | Generate capability registries from operation and projection metadata. | 0072 | 480 | Generated capability declarations with covered operations. | Completeness and no-overclaim tests. | No. |
| 500 | Not Started | P0 | 3 | Add runtime capability agreement tests. | 0072 | 490 | Tests that reconcile generated declarations with runtime state. | Focused unavailable, configured, authorized, ready, and unsupported cases. | No. |
| 510 | Not Started | P0 | 3 | Define the generated conformance manifest and case taxonomy. | 0072 | 170 | Machine-readable required-case manifest. | Review against every semantic category in 0072 section 17. | No. |
| 520 | Not Started | P0 | 4 | Generate canonical boundary vectors for null, bytes, numbers, enums, records, ownership, and errors. | 0072 | 510 | Shared language-neutral fixtures. | Canonical byte checks and negative vectors. | No. |
| 530 | Not Started | P0 | 4 | Generate operation conformance harness adapters for local, daemon, remote, and hosted clients. | 0072 | 190, 200, 210, 510 | One harness contract across deployment modes. | Representative parity tests. | No. |
| 540 | Not Started | P0 | 4 | Add idempotency, compare-token, task, stream, and destructive-operation conformance cases. | 0072 | 510, 530 | Behavioral cases beyond signature parity. | Focused positive and negative tests. | No. |
| 550 | Not Started | P0 | 3 | Run capability and conformance architecture review gate. | 0072 | 500, 520, 530, 540 | Accepted review of coverage and false-confidence risks. | Skeptical manifest, fixture, and harness review. | No. |
| 560 | Not Started | P0 | 4 | Migrate Document through every applicable prototype projection. | 0072 | 290, 390, 470, 500, 550 | End-to-end generated Document surface. | Cross-surface text, bytes, null, conditional mutation, and record tests. | No. |
| 570 | Not Started | P0 | 4 | Remove superseded Document low-level wrappers and inventories. | 0072 | 560 | Document ergonomic facades depend only on generated adapters. | Source audit finds no duplicate low-level path. | No. |
| 580 | Not Started | P0 | 4 | Migrate Chat through every applicable prototype projection. | 0072 | 290, 390, 470, 500, 550 | End-to-end generated Chat surface. | Cross-surface bytes, async, entity-tag, auth, and mutation tests. | No. |
| 590 | Not Started | P0 | 4 | Remove superseded Chat low-level wrappers and inventories. | 0072 | 580 | Chat ergonomic facades depend only on generated adapters. | Source audit finds no duplicate low-level path. | No. |
| 600 | Not Started | P0 | 4 | Run the prototype acceptance and generator-correction gate. | 0072 | 570, 590 | Accepted proof that routine operations do not require handwritten forwarding. | Full prototype criteria review. | No. |
| 610 | Not Started | P0 | 4 | Migrate foundational store, session, workspace, result, and management interfaces. | 0072 | 600 | Generated foundational projections and removed duplicates. | Focused cross-surface conformance. | No. |
| 620 | Not Started | P0 | 4 | Review and clean the foundational migration wave. | 0072 | 610 | Accepted source, schema, ABI, binding, and cleanup review. | Skeptical wave review. | No. |
| 630 | Not Started | P0 | 4 | Migrate files, VCS, CAS, KV, SQL, and transfer interfaces. | 0072 | 620 | Generated core-data projections and removed duplicates. | Focused cross-surface conformance. | No. |
| 640 | Not Started | P0 | 4 | Review and clean the core-data migration wave. | 0072 | 630 | Accepted source, schema, ABI, binding, and cleanup review. | Skeptical wave review. | No. |
| 650 | Not Started | P0 | 4 | Migrate ticket, lane, page, lifecycle, meeting, drive, and collaboration interfaces. | 0072 | 640 | Generated workflow projections and removed duplicates. | Focused cross-surface conformance. | No. |
| 660 | Not Started | P0 | 4 | Review and clean the workflow migration wave. | 0072 | 650 | Accepted source, schema, ABI, binding, and cleanup review. | Skeptical wave review. | No. |
| 670 | Not Started | P0 | 4 | Migrate columnar, dataframe, graph, vector, search, inference, program, and execution interfaces. | 0072 | 660 | Generated data and compute projections and removed duplicates. | Focused cross-surface conformance. | No. |
| 680 | Not Started | P0 | 4 | Review and clean the data and compute migration wave. | 0072 | 670 | Accepted source, schema, ABI, binding, and cleanup review. | Skeptical wave review. | No. |
| 690 | Not Started | P0 | 4 | Migrate metrics, logs, traces, time-series, queue, calendar, contacts, mail, ledger, and remaining interfaces. | 0072 | 680 | Generated remaining projections and removed duplicates. | Focused cross-surface conformance. | No. |
| 700 | Not Started | P0 | 4 | Review and clean the final migration wave. | 0072 | 690 | Accepted source, schema, ABI, binding, and cleanup review. | Skeptical wave review. | No. |
| 710 | Not Started | P1 | 3 | Add generated API compatibility reports for operation, type, projection, and ABI changes. | 0072 | 600 | Reviewable breaking-change report. | Golden old/new contract tests. | No. |
| 720 | Not Started | P1 | 3 | Add generated documentation for operation semantics and projection availability. | 0072 | 600 | Human-readable contract reference derived from Contract IR. | Documentation snapshot and link validation. | No. |
| 730 | Not Started | P1 | 3 | Measure generator time, generated code size, compile time, and runtime dispatch overhead. | 0072 | 700 | Performance report with per-wave comparison. | Dedicated non-default measurement recipe. | No. |
| 740 | Not Started | P0 | 4 | Remove all superseded handwritten low-level registries, task inventories, temporary scripts, and orphaned generated files. | 0072 | 700, 710, 720 | Clean source tree with one authority per contract concern. | Source audit, manifest orphan check, and `git status --short`. | No. |
| 750 | Not Started | P0 | 3 | Run final security and authorization review. | 0072 | 700, 740 | Accepted review of PEP preservation, secret handling, destructive annotations, and remote exposure. | Skeptical source and conformance review. | No. |
| 760 | Not Started | P0 | 3 | Run final API evolution and compatibility review. | 0072 | 710, 740 | Accepted review of stable IDs, replacements, removed draft surfaces, and reports. | Compatibility report inspection. | No. |
| 770 | Not Started | P0 | 3 | Run final architecture and DRY review. | 0072 | 740, 750, 760 | Accepted proof that emitters share one Contract IR and facades do not duplicate low-level behavior. | Cross-repository source audit. | No. |
| 780 | Not Started | P0 | 3 | Run final performance and repository-hygiene review. | 0072 | 730, 740, 770 | Accepted build-time, code-size, runtime, deterministic-generation, and artifact-hygiene evidence. | Measurement report and source-tree audit. | No. |
| 790 | Not Started | P0 | 4 | Run final focused Rust checks, generated checks, schema checks, and conformance suites serially. | 0072 | 750, 760, 770, 780 | Recorded final package and generator evidence. | Exact commands and outcomes. | No. |
| 800 | Not Started | P0 | 4 | Run dedicated ABI, binding, daemon, remote, hosted, browser, and device recipes serially where available. | 0072 | 790 | Recorded integration and binding evidence with explicit unavailable environments. | Exact recipes and outcomes. | No. |
| 810 | Not Started | P0 | 3 | Run one final `just ci` gate after all coherent changes are integrated. | 0072 | 790, 800 | Authoritative default gate result. | `just ci`. | No. |
| 820 | Not Started | P0 | 3 | Reconcile 0072, owning specs, implementation plan, and generated documentation to source-backed final state. | 0072 | 810 | Accurate completion state without overclaims. | Spec and source cross-review plus `git diff --check`. | No. |
| 830 | Not Started | P0 | 3 | Perform queue closure audit and final handoff. | 0072 | 820 | Completed closure evidence, empty hidden-work section, and final changed-file inventory. | Queue Closure Rules. | No. |

## Missed Or Hidden Work Found

None at queue creation.

Discovered work must not be hidden in chat. Before closure, every item must be promoted, moved, or cut
with rationale.

## Risk Register

| Risk | Impact | Mitigation | Status |
| --- | --- | --- | --- |
| Metadata becomes a second inconsistent authority. | Emitters drift even though an IR exists. | Load metadata with IDL, reject stale keys and duplicates, and expose only normalized Contract IR to emitters. | Open |
| Generation expands into product grammar design. | CLI and MCP become rigid or poorly designed. | Generate typed adapters and require explicit ergonomic projection metadata; keep product grouping handwritten. | Open |
| ABI generation hides ownership mistakes. | Memory corruption or leaks cross every native binding. | Define ownership first, generate manifests, run boundary vectors, and require ABI review before migration. | Open |
| Escape hatches recreate handwritten completeness gaps. | Platform adapters silently omit operations. | Declare each escape hatch per operation, generate mandatory traits, and fail compilation when missing. | Open |
| Broad migration creates long broken periods. | Parallel work stalls and review quality falls. | Prototype first, migrate bounded waves, and require cleanup review after every wave. | Open |
| Generated code increases compile time and binary size. | Developer throughput and distribution size regress. | Measure per wave and deduplicate shared runtime helpers. | Open |
| Temporary recovery artifacts survive 0072. | The repository retains multiple authorities. | Baseline them in Task 10 and remove them in prototype or final cleanup tasks. | Open |

## Implementation Batch Map

Each batch ends at a safe integration point. "Buildable" means authoritative inputs and generated
artifacts agree, affected Cargo packages compile, changed bindings pass focused builds, changed
contracts pass focused conformance, and existing unaffected public surfaces remain source-compatible.
Device, browser, hosted, and network runtime suites remain dedicated integration evidence and do not
run at every batch boundary.

| Batch | Tasks | Purpose | Exit gate |
| --- | --- | --- | --- |
| Baseline and semantic design | 10-80 | Establish current ownership and the complete target contract. | Documentation and inventories are source-backed; no runtime or generated artifact changes; `git diff --check` passes. |
| Contract IR foundation | 90-170 | Build and review parser, model, validation, IDs, and snapshots. | Contract IR packages compile; parser and negative validators pass; production generation remains unchanged; deterministic snapshot passes. |
| Rust generation | 180-230 | Move current remote generation onto the Contract IR. | Generated registry, service traits, remote client, and hosted dispatch agree; affected Rust packages compile; focused local, remote, and hosted tests pass; old emitter logic is removed or explicitly retained for the prototype. |
| C ABI | 240-290 | Generate and validate native ABI completeness and ownership. | ABI additions are source-compatible with existing bindings; FFI compiles; ABI manifest matches exports and header; ownership vectors pass. Existing symbols are not removed in this batch. |
| Bindings | 300-390 | Generate low-level adapters and prove language completeness. | Every maintained binding passes its focused compile or build recipe; manifest completeness passes; ergonomic facades still compile; no binding depends on a removed symbol. |
| CLI and MCP | 400-470 | Generate typed projection registries and schemas while preserving product ergonomics. | CLI and MCP packages compile; every operation is classified; focused routing and schema-runtime tests pass; existing command and tool grammar remains intact. |
| Capabilities and conformance | 480-550 | Derive support claims and behavioral proof from one contract. | Capability and conformance packages compile; generated declarations match runtime states; representative cross-target harness tests pass. |
| Prototype | 560-600 | Prove Document and Chat end to end before broad migration. | Document and Chat pass every applicable projection and conformance check; superseded low-level prototype adapters are deleted; all affected Rust and binding targets compile. |
| Foundational migration wave | 610-620 | Migrate foundational store, session, workspace, result, and management interfaces. | Focused cross-surface conformance passes; affected projections compile; wave cleanup review finds no duplicate low-level authority. |
| Core-data migration wave | 630-640 | Migrate files, VCS, CAS, KV, SQL, and transfer interfaces. | Focused cross-surface conformance passes; affected projections compile; wave cleanup review finds no duplicate low-level authority. |
| Workflow migration wave | 650-660 | Migrate ticket, lane, page, lifecycle, meeting, drive, and collaboration interfaces. | Focused cross-surface conformance passes; affected projections compile; wave cleanup review finds no duplicate low-level authority. |
| Data and compute migration wave | 670-680 | Migrate columnar, dataframe, graph, vector, search, inference, program, and execution interfaces. | Focused cross-surface conformance passes; affected projections compile; wave cleanup review finds no duplicate low-level authority. |
| Final interface migration wave | 690-700 | Migrate observability, PIM, ledger, queue, and all remaining interfaces. | All remaining projections compile; focused conformance passes; final wave review finds no duplicate low-level authority or unclassified operation. |
| Evolution and cleanup | 710-780 | Add reports, documentation, measurement, and remove duplicate authorities. | Compatibility and documentation generation pass; measurements are recorded; all superseded registries and temporary artifacts are removed; supported targets remain buildable. |
| Final verification | 790-830 | Run serial gates, reconcile specs, and close the queue. | Focused and integration evidence is recorded; one final `just ci` passes; specs and generated documentation match source. |

## Blocked Task Protocol

Blocked tasks must include:

- Blocking condition.
- Attempted resolution.
- Decision needed, if any.
- Next unblock action.

## Queue Closure Rules

Do not close this queue until:

- Every task is Done, Cut with rationale, or moved.
- Missed Or Hidden Work Found is empty, promoted, cut, or moved.
- Decision Points are resolved, cut, or moved.
- Completion Evidence is satisfied.
- No public operation is unclassified.
- No applicable low-level projection depends on an untracked handwritten registry.
- Every Implementation Batch Map exit gate is satisfied or explicitly superseded by stronger final
  evidence.
- Final Handoff is complete.

Do not reorder, reprioritize, or cut tasks without recording the reason. Ask the user before changing
P0 or P1 priority unless a blocker carve-out applies.

## Final Handoff

- Summary:
- Completed tasks:
- Cut or deferred tasks and where they moved:
- Decisions resolved:
- Completion evidence:
- Remaining risks:
- Files changed:

## Working Rules For This Queue

- Check authoritative source before relying on a boundary.
- Keep source-backed state separate from target design.
- Treat generated output as read-only.
- Update task status as work progresses.
- Apply the No Buried Work Rule before every handoff.
- Record discovered work immediately.
- Split work above lift 5 before assignment.
- Give implementers prescriptive acceptance criteria without granting unbounded architectural discretion.
- Run focused checks during development and one coherent `just ci` gate at the end.
- Run expensive binding and integration recipes serially.
- When a blocker appears, report it in chat using the repository decision format.
