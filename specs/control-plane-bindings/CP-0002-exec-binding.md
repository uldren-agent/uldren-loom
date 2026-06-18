# CP-0002 - `exec` Binding

**Series:** Control-plane bindings (normative-track sub-series; Draft)
**Version:** 0.1.0-draft. **Status:** Draft. **Last updated:** 2026-07-12
**Reads first:** [`CP-0000-index.md`](./CP-0000-index.md),
[`../facet-bindings/P9-0002-projection-conventions.md`](../facet-bindings/P9-0002-projection-conventions.md),
facade spec **0015** (`exec`), **0008 section 2**, and **0015 section 8** (`StateAccess` target).

`exec` is the target control-plane surface for running deterministic, metered programs against Loom
state. Program authoring/storage is a sibling lifecycle surface: callers store `engine=wasm` programs
from externally built WASM bytes or `engine=cel` programs from interpreted CEL source, then run the
stored program by digest or reference. A caller should be able to run a program on a review branch,
inspect the proposed diff, and adopt it explicitly.

## 1. Current Source Boundary

Current source backs the raw canonical `exec` projection:

- `Capability`, `Mode`, `Scope`, `Grant`, and `GrantSet`;
- deterministic `Manifest` with content-addressed program body digest;
- wasmi `engine=wasm` execution;
- optional native Wasmtime fast path behind `engine-wasmtime`;
- multi-facet Rust `StateAccess` and guest WASM host calls;
- gated, direct, and batched execution modes;
- canonical `loom.exec.request.v1` and `loom.exec.result.v1` CBOR envelopes;
- IDL, C ABI, generated header, CLI, checked-in language bindings, hosted REST/JSON-RPC/gRPC adapters,
  and served REST/JSON-RPC/gRPC listeners for raw `exec_cbor`;
- guard, derivation, statechart, workflow, and trigger execution substrate integration;
- fuel metering;
- `run_on_branch` over `loom-core::vcs`;
- `ExecError::Program`, `ExecError::BudgetExceeded`, and wrapped core errors.

Current source does not yet back a decoded Program lifecycle surface, persisted `engine=cel` program
execution, MCP `program.*` / `exec.*` tools, or ergonomic `exec` methods that accept a stored program ref
instead of a caller-assembled canonical CBOR request.

## 2. Target Facade Surface

Target public `exec` shape:

```text
dry_run(ns, program: Digest, inputs: Map<string, Digest>, budget) -> ExecResult
apply(ns, program: Digest, inputs: Map<string, Digest>, budget, target) -> Digest
```

`dry_run` runs on a throwaway or proposal branch and returns the proposed commit, diff, cost, and logs.
It never advances the target branch. `apply` runs and adopts the result only after policy and merge
checks pass.

Programs are content-addressed objects plus a manifest declaring required grants. A program may use
`engine=wasm`, where the body is externally built WASM bytes run by Loom, or target `engine=cel`, where
the body is CEL source text interpreted by Loom. The target `StateAccess` surface is private to the
execution engine and must not appear in `loom.h`, the IDL, or public bindings.

Target public Program lifecycle shape:

```text
program.put_wasm(store, manifest, wasm_body) -> Digest
program.put_cel(store, manifest, cel_source) -> Digest
program.inspect(store, program) -> ProgramInfo
program.get(store, program) -> ProgramRecord
program.list(store, selector) -> [ProgramInfo]
program.remove(store, program) -> bool
```

`program.put_cel` is the AI-agent-oriented authoring path. It stores inspectable CEL instructions under
the same content-addressed program identity discipline as WASM programs. The first promoted CEL profile
is deterministic, bounded, read-only result logic. Mutation from CEL is a follow-on constrained action
envelope profile: CEL returns canonical action data, and Loom validates and applies the action outside
the CEL evaluator.

## 3. Target Errors

The current stable `loom_core::error::Code` enum does not include execution-specific variants such as
`BUDGET_EXCEEDED`, `NONDETERMINISM_DETECTED`, `CAPABILITY_DENIED`, `PROGRAM_INVALID`, or
`GUARD_REJECTED`. Those names are target work unless 0015 defines a separate execution result envelope.

Until promotion, source-backed errors are `loom-compute::ExecError` values inside Rust tests and any
wrapped core `LoomError` values they carry.

## 4. Tier-1 REST

Target roots:

```text
/v1/programs
/v1/exec
```

| Method | HTTP |
| --- | --- |
| store WASM program | `POST /programs:putWasm` with manifest plus WASM body |
| store CEL program | `POST /programs:putCel` with manifest plus CEL source |
| inspect program | `GET /programs/{program}` |
| `dry_run` | `POST /exec:dryRun` with `{program, inputs, budget}` and streamed logs |
| `apply` | `POST /exec:apply` with `{program, inputs, budget, target}` and streamed logs |

Both `exec` methods are write-class from an authorization perspective because both execute code and
spend budget. Program lifecycle mutations are also write-class on the Program facet.

## 5. Tier-1 JSON-RPC and gRPC

Target JSON-RPC methods: `program.putWasm`, `program.putCel`, `program.inspect`, `program.get`,
`program.list`, `program.remove`, `exec.dryRun`, and `exec.apply`.

Target gRPC services: `Program` and `Exec`. `Exec.DryRun` and `Exec.Apply` stream logs if promoted by
0008.

## 6. Tier-1 MCP

- **Tools:** `program_put_wasm`, `program_put_cel`, `program_inspect`, `program_get`, `program_list`,
  `program_remove`, `exec_dry_run`, and `exec_apply`.
- **Authorization:** both are write-class and token-gated. The session must grant `exec`, and the
  program manifest grants must intersect with principal/ACL policy before any state operation runs.
  Program creation requires Program facet write and may require Exec grant validation before the stored
  program is runnable.

## 7. Tier-2 Foreign Adapter

There is no faithful foreign protocol for Loom execution. FaaS invoke APIs, OPA query APIs, and WASM
host APIs do not carry Loom's branch/diff/merge or capability-manifest semantics. A generic function
invoke wrapper is possible later, but the enterprise value is the native projection and MCP tool.

## 8. Resolved Decisions

### CP-RD-E1 - `dry_run` is write-class

- **Decision.** Treat `dry_run` as write-class and token-gated because it executes code and spends
  budget even when it does not merge. Gate both `dry_run` and `apply`.

### CP-RD-E2 - `StateAccess` stays private

- **Decision.** Public projections expose only `dry_run` and `apply`. The engine-private state surface
  may evolve without public ABI churn, but every promoted state operation still needs tests and
  conformance.

### CP-RD-E3 - Program lifecycle is first-class

- **Decision.** Do not make agents assemble raw `loom.exec.request.v1` CBOR or rely only on CAS upload.
  Promote a Program lifecycle surface that can store, inspect, list, and remove `engine=wasm` and
  `engine=cel` programs. `exec` then runs stored programs by digest or ref.

### CP-RD-E4 - CEL is a program engine, not only a guard implementation

- **Decision.** Keep CEL guard and ACL predicate support, and add target `engine=cel` program support for
  persisted AI-authored interpreted programs. CEL execution must remain deterministic, bounded, and
  inspectable. WASM remains the source-backed mutation-capable engine and the upload path for externally
  built programs.

## 9. Open Questions

### CP-RD-E5 - CEL program mutation profile

- **Context.** CEL is excellent for AI-authored decision logic and already integrated as deterministic
  guard/predicate evaluation. Whether an `engine=cel` program may mutate Loom state directly, emit a
  constrained action envelope, or remain read-only decision logic affects safety, conformance, and
  agent ergonomics.
- **Options.** (a) read-only CEL result programs only; (b) CEL plus a constrained action envelope; (c)
  direct `StateAccess` mutation from CEL host functions.
- **Decision.** Start with (a), then promote (b) as a closed, versioned constrained action envelope with
  authorization, anchor/idempotency, per-facet validation, and conformance proof. Do not expose direct
  `StateAccess` mutation from CEL host functions.
