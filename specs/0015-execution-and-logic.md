# 0015 - Execution and Logic

**Status:** Partial. The `engine=wasm` `exec` facade (gated/direct/batched), the multi-facet WASM host
ABI, the guard/derivation/statechart/workflow logic layers, the local Rust Program lifecycle
foundation for `engine=wasm`, `engine=template`, and `engine=cel`, and the render-only
`engine=template` result-mapping engine are source-backed in `crates/loom-compute`; the local CLI
Program lifecycle projection is source-backed in `crates/loom-cli`; IDL and C ABI Program lifecycle
symbols exist but remain degraded until executable runtime certification is added; executable
conformance remains source-backed in `crates/loom-conformance`; MCP, binding, and hosted Program
lifecycle projection, public template projection, and `engine=cel` interpreted execution remain target
work, and the reactive infrastructure is deferred to 0029/0030/0035/0041. **Version:** 0.2.0.
**Capability:** `exec`.

This spec defines programmable execution over Loom state. Current source implements a Rust compute
substrate in `crates/loom-compute`: a program grant vocabulary, a canonical deterministic program
manifest, a local durable Program facet record for `engine=wasm`, `engine=template`, and `engine=cel`
manifest/body storage and inspection, a render-only `engine=template` evaluator over Loom Templates
bindings, a metered WASM engine for `engine=wasm`, a multi-facet Rust `StateAccess` path, and an
`exec` facade with gated, direct, and batched modes plus canonical request/result envelopes.
`StateAccess` intersects the authenticated principal's `Exec` ACL with the program manifest's grants
before touching Loom state. The IDL, C ABI, C header, Node, Python, C++, Swift/iOS, JVM, Android,
React Native, and WASM now expose the canonical CBOR execution request through `exec_cbor` /
`loom_exec_cbor` equivalents. `crates/loom-hosted` also exposes REST, JSON-RPC, and gRPC execution
adapters and served listeners through the same request/result bytes and hosted auth/write path.
The WASM host ABI now spans files, KV (including bounded `kv_scan`), document, ledger, CAS, queue,
graph, vector, columnar, search, dataframe, time-series, SQL, and calendar/contacts/mail domain calls.
Guards are folded into the manifest's content-addressed
identity and enforced fail-closed in the gated, direct, and batched modes; derivations, statecharts,
and workflows are graduated into `crates/loom-compute` behind the `guards`, `derivations`,
`statecharts`, and `workflows` cargo features and live-wired through `StateAccess`. Executable exec
conformance runs in `crates/loom-conformance`. The synchronous `exec` contract is source-backed.
The local Rust Program lifecycle foundation stores externally built WASM programs (`engine=wasm`),
render-only templates (`engine=template`), and inspectable CEL source programs (`engine=cel`) as
durable Loom Program facet records before they are run or bound to triggers/hooks. The local CLI
`program` surface exposes those same records for local stores through `put-wasm`, `put-template`,
`put-cel`, `inspect`, `get`, `list`, and `remove`. The target non-CLI public `program` surface
exposes those same records through MCP, IDL, C ABI, bindings, and hosted protocols. Reactive firing
(triggers/keeper/scheduler), durable delivery, the change feed, and
lifecycle hooks are deliberately out of scope here and specified in 0029, 0035, 0030, and 0041
respectively.

Every execution is scoped to the workspace supplied to the current Rust API. Target cross-workspace
execution is an access-control decision, not a blanket permission: it must require explicit
workspace-aware grants before promotion.

## 1. Current Implementation

`crates/loom-compute` implements:

- `Capability`;
- `Mode`;
- `Scope`;
- `Grant`;
- `GrantSet`;
- `Manifest`;
- `StoredProgram`;
- `ProgramBody`;
- `program_put`;
- `program_put_wasm`;
- `program_put_template`;
- `program_put_cel`;
- `program_get`;
- `program_list`;
- `program_remove`;
- `program_inspect`;
- `TemplateExecution`;
- `render_template_program`;
- `FileSet`;
- `RunResult`;
- `ExecError`;
- `RunReport`;
- `ExecContext`;
- `StateAccess`;
- `ExecRequest`;
- `DirectExecRequest`;
- `BatchExecRequest`;
- `ExecStep`;
- `ExecReport`;
- `ExecCommitReport`;
- `ExecCommitMode`;
- `dry_run`;
- `direct`;
- `batch`;
- `apply`;
- `execute_cbor`;
- `run_on_branch`.

`Capability` is an alias for `loom_core::FacetKind`, the single facet vocabulary source of truth
(`FacetKind::ALL`, 18 variants): files, VCS, SQL, key-value, document, vector, graph, columnar, queue,
time-series, CAS, ledger, program, calendar, contacts, mail, search, and dataframe. `Mode` is read,
write, or read-write. `Scope` is
`All` or a single string prefix. `GrantSet::new` canonicalizes grants and scopes, and
`GrantSet::permits` checks facet, mode, and prefix. `ExecContext::authorize_operation` is the current
private execution PEP: it requires the principal ACL to allow `Execute` on the requested facet and
`exec` scope, then requires the manifest grant to allow the same facet, mode, and target.

Current manifest grants do not carry subjects, deny effects, row predicates, column scopes, or policy
metadata. The workspace and principal are supplied by `ExecContext` rather than by the manifest.
The current public execution request envelope is canonical `loom.exec.request.v1` CBOR. It binds the
workspace UUID, principal UUID, role set, authentication state, base branch, context grant upper bound,
mode, per-step manifest bytes, `engine=wasm` body bytes, inputs, fuel, optional fork branch, author,
message, and timestamp. A public batch request carries a manifest per step, and the context grants are
only an upper bound: every step narrows authority to its own manifest grants before execution.

`ExecReport::to_cbor` and `ExecCommitReport::to_cbor` encode `loom.exec.result.v1` as Loom Canonical
CBOR. The envelope carries schema, mode (`gated`, `direct`, or `batch`), committed flag, principal,
branch, before/after/after-root digests, path changes, fuel used, and bounded logs. This is the
source-backed result shape future binding and wire projections should preserve.

## 2. Current Program Manifest

`Manifest` is the content-addressed program declaration. It contains:

- `name`;
- `engine`;
- `abi_version`;
- `entry`;
- `grants`;
- optional `input_schema`;
- optional `output_schema`;
- `body`;
- `guards`.

`engine=wasm` programs store a WASM module body. `Manifest::for_wasm` computes that body digest through
the Loom object model. `Manifest::encode` emits Loom Canonical CBOR v1 through `loom-codec`, using a
manifest-local type code and schema version. `Manifest::decode` rejects non-canonical bytes, unknown
schema versions, non-grantable facets, unknown mode tags, malformed scopes, wrong shapes, trailing
fields, and trailing bytes. `Manifest::store` stores the canonical manifest bytes as a
content-addressed blob and returns the program identity digest.

`engine=template` programs store a Loom Templates source body. `Manifest::for_template` computes that
body digest through the Loom object model, sets ABI v1, and uses entry `render`. The template engine
reuses the existing `loom-templates` MiniJinja-compatible binding model:

- `loom.<name>` inputs are JSON values exposed under the `loom` object;
- `program.<name>` inputs are UTF-8 strings exposed through `loom.program("<name>")`;
- `meta` is a JSON object exposed under `meta.*`;
- `request.<name>`, `response.<name>`, `session.<name>`, and `cookie.<name>` are UTF-8 string maps.

The `engine=template` output contract is strict. The rendered template text must be JSON with an
`outputs` object and optional `logs` array of strings:

```json
{
  "outputs": {
    "html": "<section>ready</section>",
    "summary": "ready"
  },
  "logs": ["rendered"]
}
```

Loom converts that rendered mapping to canonical `loom.template.result.v1` CBOR with `outputs`,
`logs`, source digest, AST digest, planned host calls, and diagnostics. Templates do not mutate Loom
state directly. They produce deterministic data, HTML, or action documents as values, and a later
validated action-envelope or stored-program wrapper may decide how to use those values.

`engine=cel` programs store CEL source text as the program body. `Manifest::for_cel` computes that
body digest through the Loom object model, sets ABI v1, and uses entry `eval`. The local Program
lifecycle stores and inspects CEL source programs through the same manifest/body record as other
engines. Current source integrates `cel-interpreter` for guard and ACL predicate evaluation, but it
does not yet execute persisted `engine=cel` program bodies through `exec`.

The first promoted `engine=cel` execution profile is read-only result logic. A CEL program may compute
deterministic decisions, classifications, projections, or proposed action documents from canonical
inputs and authorized read context, but the CEL evaluator itself does not mutate Loom state. Mutation
from CEL is a separate target profile: the CEL result may be a closed, versioned constrained action
envelope that Loom validates and applies outside the CEL evaluator. Direct CEL host functions over
`StateAccess` are not the enterprise target because they would duplicate authorization, rollback,
conformance, and per-facet mutation semantics inside the CEL evaluator.

Current and target program lifecycle surfaces are:

| Surface | Purpose | Current status |
| --- | --- | --- |
| `program_put` Rust API | Store a supported engine body plus manifest as a durable local Program facet record | Source-backed in `crates/loom-compute::program_lifecycle`; one storage path validates program name, manifest name, grantability, engine shape, and body digest for `engine=wasm`, `engine=template`, and `engine=cel` |
| `program_put_wasm` / `program_put_template` / `program_put_cel` Rust APIs | Engine-specific convenience wrappers over `program_put` | Source-backed in `crates/loom-compute::program_lifecycle` |
| `program_inspect` Rust API | Decode the durable local Program facet record, manifest, engine, grants, schemas, guards, body digest, and body metadata | Source-backed in `crates/loom-compute::program_lifecycle` |
| `program_get` / `program_list` / `program_remove` Rust APIs | Load program body plus record, list named records, and remove named records under the Program facet | Source-backed in `crates/loom-compute::program_lifecycle`; remove unlinks the named record and leaves content-addressed body/manifest storage reusable |
| `render_template_program` Rust API | Render an `engine=template` source body with existing Loom Templates bindings and canonicalize the rendered `outputs` mapping | Source-backed in `crates/loom-compute::template_engine`; public projection and `exec` envelope integration remain target |
| Local CLI `program put-wasm` / `program put-template` / `program put-cel` projection | Store supported engine bodies in a local Loom store through the source-backed Rust API | Source-backed in `crates/loom-cli`; commands ensure the Program facet workspace, persist the record, and emit canonical CBOR summaries |
| Local CLI `program inspect` projection | Decode manifest, engine, grants, schemas, guards, body digest, and body metadata for local Loom stores | Source-backed in `crates/loom-cli`; output is a canonical CBOR summary over the stored record |
| Local CLI `program get` / `program list` / `program remove` projection | Manage durable local program records | Source-backed in `crates/loom-cli`; `get` emits raw body bytes, `list` emits canonical CBOR summaries, and `remove` unlinks the named record |
| IDL and C ABI `program.put` projection | Store supported engine bodies through IDL and C ABI symbols | Symbols and FFI implementation are source-backed, but runtime certification remains degraded until executable C ABI tests prove put/inspect/get/list/remove behavior |
| IDL and C ABI `program.inspect` projection | Decode manifest, engine, grants, schemas, guards, body digest, and body metadata through IDL and C ABI symbols | Symbols and FFI implementation are source-backed, but runtime certification remains degraded until executable C ABI tests prove inspection, absent-record, and malformed-record behavior |
| IDL and C ABI `program.get` / `program.list` / `program.remove` projection | Manage durable program records through IDL and C ABI symbols | Symbols and FFI implementation are source-backed, but runtime certification remains degraded until executable C ABI tests prove body retrieval, list ordering, removal, idempotent absence, and error preservation |
| MCP, binding, and hosted `program.*` projections | Store, inspect, retrieve, list, and remove durable Program records through MCP, bindings, and hosted protocols | Target ergonomic projection over the source-backed Rust API; raw `exec` can carry WASM manifest/body bytes today |
| `exec.run` / `exec.dry_run` / `exec.apply` by program ref | Build canonical `loom.exec.request.v1` from stored program refs and inputs | Target ergonomic wrapper over source-backed raw `exec_cbor` |

## 2.1 Active Program and CEL owner gates

Completion state: active implementation owner. The local Rust and CLI Program lifecycle foundation is
source-backed, IDL and C ABI symbols exist with degraded runtime certification, and CEL source storage,
guard evaluation, and ACL predicate evaluation are source-backed. Public Program lifecycle projection,
runtime-certified C ABI behavior, binding certification, MCP and hosted projection, public template
execution envelopes, persisted `engine=cel` execution, and constrained CEL action envelopes remain P0
implementation work.

Decision Points: none.

| Gate | Source-backed evidence | Remaining implementation work | Disposition |
| --- | --- | --- | --- |
| Program lifecycle runtime certification | Rust and CLI Program lifecycle storage, inspection, get, list, and remove are source-backed; IDL and C ABI symbols exist. | Add executable C ABI tests for put, inspect, get, list, remove, absence, malformed records, error preservation, body retrieval, list ordering, and removal idempotence. | Target P0. |
| Binding Program projection | Raw `exec_cbor` exists across hand-written binding families. | Add generated or hand-written binding runtime suites for `program.put`, `program.inspect`, `program.get`, `program.list`, `program.remove`, and degraded or unsupported reporting where a binding does not expose them. | Target P0. |
| MCP and hosted Program projection | The Rust API and local CLI define the durable record semantics. | Project `program.*` through MCP and hosted REST, JSON-RPC, and gRPC using the same canonical records, authorization checks, errors, and report rows. | Target P0. |
| Stored-program execution wrappers | Raw `exec_cbor` can run supplied manifest and body bytes. | Add public `exec.run`, `exec.dry_run`, and `exec.apply` wrappers that resolve stored program refs, construct canonical `loom.exec.request.v1`, preserve gated/direct/batched fail-closed behavior, and report unsupported engines honestly. | Target P0. |
| Public template execution | `render_template_program` is source-backed for render-only template bodies and canonical `loom.template.result.v1` output. | Integrate `engine=template` into the public exec envelope, binding projections, hosted projections, and conformance without granting templates ambient mutation authority. | Target P0. |
| Persisted CEL execution | CEL source storage, CEL guard evaluation, and ACL predicate evaluation are source-backed. | Define the persisted CEL evaluator context, input/result shape, structural or cost bounds, deterministic errors, and conformance for read-only result logic. | Target P0. |
| Constrained CEL action envelopes | The target mutation posture requires CEL to emit action documents that Loom validates outside the evaluator. | Define versioned action schemas, action-kind registry, authorization, rollback behavior, canonical CBOR, negative vectors, and conformance for each promoted action kind. | Target P0. |

The local Rust Program lifecycle record is stored under the reserved Program facet and separates the
name record, manifest bytes, and program body bytes. The manifest remains content-addressed through
`Manifest::store`, and the body is validated against the digest embedded in the manifest before the
record is accepted. This foundation does not execute stored programs, build `loom.exec.request.v1`
wrappers, or expose a public wire or binding shape.

Guard declarations are data in the manifest, not an ambient policy side channel. Changing a guard
changes the manifest bytes and therefore the program identity. Workspace, principal, role set,
authenticated state, base branch, and context-grant upper bound are supplied by the execution request
and `ExecContext`, not by the manifest.

## 3. Current WASM Engine

The guest module imports host functions from module `env` and must export `memory` and `run`.
Execution is metered with WASM fuel. Exhausting fuel returns `ExecError::BudgetExceeded`, which maps
to `RESOURCE_EXHAUSTED`; denied host operations fail closed with `PERMISSION_DENIED`.

The legacy in-memory `run` path remains available for files-only execution tests. The current
stateful execution path is `run_state`, which uses wasmi because it carries a borrowed
`StateAccess` through host state. Wasmtime remains an optional native engine for the in-memory path
behind `engine-wasmtime`.

The current private `run_state` ABI exposes:

- files: `file_write`, `file_remove`, `file_read`;
- inputs: `input_get`;
- KV: `kv_put`, `kv_get`, `kv_delete`, `kv_len`, `kv_scan`;
- document: explicit text/binary put/get, `document_list_binary`, and `doc_delete`;
- ledger: `ledger_append`, `ledger_get`, `ledger_len`;
- CAS: `cas_put`, `cas_get`, `cas_has`, `cas_delete`;
- queue: `queue_append`, `queue_get`, `queue_range`, `queue_len`;
- graph: adjacency, node, edge, reachability, shortest-path, upsert, and remove calls;
- vector: create, upsert, get, delete, ids, unfiltered search, and filtered search;
- columnar: create, append, scan, columns, row count, select, and aggregate;
- search: create, index, get, delete, ids, and query;
- dataframe: create, put plan, get plan, collect, preview, materialize, plan digest, and source
  digests;
- time-series: put, get, latest, and range;
- SQL: `sql_query` and `sql_exec`;
- calendar, contacts, and mail domain calls.

`run_state` does not bypass authorization. Each files or KV host operation calls `StateAccess`, and
`StateAccess` calls `ExecContext::authorize_operation` before reading or mutating Loom state. Denied
operations fail the run with `PERMISSION_DENIED`; the caller does not get a successful result with
missing writes. SQL `StateAccess` is feature-backed by `sql-state-access`: `sql_query_cbor`
authorizes `Sql` read access and rejects dirty statements, while `sql_exec_cbor` authorizes `Sql`
write access and persists changed SQL state. The WASM host ABI exposes `sql_query` and `sql_exec`
through the same canonical SQL result CBOR bytes, and public CLI/hosted/binding `exec_cbor` builds
enable the `sql-state-access` feature. Dataframe execution preflights the source bindings and
materialization target, so a dataframe grant does not bypass Files, CAS, or Columnar grants.

## 4. Current Run-On-A-Branch Gate

`run_on_branch` implements the current verification gate for files-facet WASM programs:

1. confirm the workspace supports branching;
2. read the base branch tip;
3. checkout the base branch;
4. materialize staged workspace files into an in-memory `FileSet`;
5. run the WASM program under grants, inputs, and fuel;
6. create and checkout the fork branch;
7. rewrite the fork working tree to the proposed file set;
8. commit the proposal;
9. return `RunReport` with fork branch, before commit, after commit, after root, changes, and fuel.

The base branch is not advanced by `run_on_branch`. Adoption is a separate merge by the caller.

Current run-on-a-branch behavior is path/file based. It does not know about non-files facets, guard
evaluation, trigger firing, workflow state, or served authorization. The Rust `exec` facade's gated,
direct, and batched modes use the files/KV `run_state` path rather than this older files-only gate.

## 5. Current Conformance

`crates/loom-compute` has unit tests for:

- grant mode coverage;
- facet, mode, and prefix permission checks;
- canonical grant ordering;
- capability and mode tag round trips;
- manifest round trip;
- deterministic manifest encoding;
- manifest content address;
- local `engine=wasm` Program lifecycle put/inspect round trip;
- local `engine=wasm` Program lifecycle digest-mismatch rejection;
- local generic Program lifecycle put/inspect for `engine=template` and `engine=cel`;
- local generic Program lifecycle get/list/remove;
- local CLI Program lifecycle parser coverage;
- local CLI Program lifecycle put/list/get round trip across `engine=wasm`, `engine=template`, and
  `engine=cel`;
- invalid `engine=template` source rejection at Program lifecycle storage time;
- `engine=template` render input binding and canonical output mapping;
- `engine=template` missing `outputs` rejection;
- `engine=template` source digest mismatch rejection;
- files-facet WASM execution;
- run-on-branch proposal and diff;
- deterministic proposed roots;
- denied operations fail closed;
- out-of-fuel behavior;
- adopt-by-merge behavior;
- `ExecContext` ACL and manifest-grant intersection;
- `StateAccess` files, KV, CAS, document, queue, time-series, ledger, graph, columnar, search,
  vector, dataframe, calendar, contacts, and mail reads/writes through real Loom state;
- wasmi `run_state` files/KV and SQL host calls through `StateAccess`;
- Rust `exec` gated, direct, and batched modes;
- canonical `loom.exec.request.v1` request envelopes;
- canonical `loom.exec.result.v1` result envelopes;
- canonical `exec-manifest` positive and negative byte vectors in `crates/loom-conformance`;
- C ABI execution through `loom_exec_cbor`;
- raw `exec` CBOR projection for Node, Python, C++, Swift/iOS, JVM, Android, React Native, and WASM;
- hosted adapter-level REST, JSON-RPC, and gRPC `exec` methods;
- served REST, JSON-RPC, and gRPC `exec` listeners;
- fail-closed denied-operation rollback for direct execution;
- optional Wasmtime and wasmi equivalence when the feature is enabled.

`crates/loom-conformance` includes executable runners for `exec`, `sql-state-access`, `dataframe`,
and `pim-trigger` behavior. The `exec` runner exercises the source-backed facade and promoted
multi-facet `StateAccess` operations against a live `Loom<MemoryStore>`, and `certify_memory_store`
includes it in the aggregate executable behavior boundary. The canonical vector runner includes
`exec-manifest` vectors for positive manifest identity bytes and negative decode rejection.

## 6. Component Boundary

`crates/loom-compute` owns the current execution subsystem: the manifest, grant vocabulary, engines,
`StateAccess`, facade, request/result envelopes, guard layer, derivation layer, statechart layer,
workflow layer, and trigger execution bridge. Reactive scheduling, durable event transport, and
facet lifecycle event emission are outside this crate's ownership and remain assigned to 0029, 0035,
0030, and 0041.

## 7. Promoted Contract

The source-backed enterprise contract is a durable cross-language `engine=wasm` `exec` capability:

- content-addressed program manifests in a promoted canonical format;
- workspace-aware grants aligned with principal and access-control specs;
- deterministic metered WASM execution;
- private multi-facet `StateAccess`;
- run-on-a-branch dry-run and apply flows;
- failed gated-run scratch-fork discard through protected branch deletion;
- explicit gated, direct, and batched execution modes;
- guard, derivation, workflow, and statechart layers;
- stable error mapping;
- public IDL, C ABI, binding, and wire projections;
- executable conformance vectors that pin manifest identity bytes and output object graphs.

Remaining promotion gaps: MCP, IDL, C ABI, binding, and hosted Program lifecycle projection, public
`engine=template` projection, `engine=template` integration into the public `loom.exec.request.v1`
envelope, persisted `engine=cel` read-only result execution, constrained CEL action envelopes,
ergonomic `exec` wrappers that accept stored program refs instead of requiring callers to assemble raw
canonical CBOR, and conformance for each promoted action kind. Reactive scheduling, durable event
transport, and lifecycle hook emission remain owned by 0029, 0035, 0030, and 0041.

## 8. Target `StateAccess`

`StateAccess` is target private engine API, not a public ABI. External callers should reach execution
through the public `exec` facade once promoted.

The target private surface should expose small, least-privilege operations for:

| Facet | Target program operations | Current status |
| --- | --- | --- |
| Files | read, write, list, remove | Source-backed through Rust `StateAccess` and the WASM host ABI (`file_*`) |
| SQL | create table, insert, update, delete, select, scan | Source-backed through feature-gated Rust `StateAccess` and guest WASM host ABI: `sql_exec_cbor`/`sql_exec` are mutation-capable, `sql_query_cbor`/`sql_query` are read-only, and all return canonical SQL result CBOR |
| KV | get, put, delete, scan | Source-backed: get, put, delete, len, and bounded scan through private `StateAccess` and the WASM host ABI (`kv_scan`) |
| Document | get, put, delete, ids | Source-backed through Rust `StateAccess` and the WASM host ABI (`doc_*`) |
| Graph | add node, add edge, neighbors, nodes, reachability, shortest path | Source-backed through Rust `StateAccess` and the WASM host ABI (`graph_*`) |
| Vector | create, upsert, get, delete, ids, search | Source-backed through Rust `StateAccess` and the WASM host ABI, including filtered search through `vector_search_filtered` |
| Columnar | create, append, scan, schema, row count, select, aggregate | Source-backed through Rust `StateAccess` and the WASM host ABI (`columnar_*`) |
| Queue | append, read from, len | Source-backed through Rust `StateAccess` and the WASM host ABI (`queue_*`) |
| Time-series | append or put point, range | Source-backed through Rust `StateAccess` and the WASM host ABI (`ts_*`) |
| CAS | put blob, get blob | Source-backed through Rust `StateAccess` and the WASM host ABI (`cas_*`) |
| Ledger | append entry, len, verify | Source-backed through Rust `StateAccess` and the WASM host ABI (`ledger_*`) |
| Search | create, index, get, delete, ids, query | Source-backed through Rust `StateAccess` and the WASM host ABI (`search_*`) |
| Dataframe | create, put/get plan, preview, collect, materialize, plan digest, source digests | Source-backed through Rust `StateAccess` and the WASM host ABI (`dataframe_*`) |
| Calendar, contacts, mail | lifecycle-triggered automation plus domain-shaped direct operations | Source-backed through Rust `StateAccess` and the WASM host ABI as domain-shaped collection/book/mailbox and entry/message operations. The trigger-fire bridge can run PIM programs and append 0029 fire records; facet event emission remains target work in 0041. |

The private surface can evolve without public ABI breakage, but every promoted operation still needs
source tests and execution conformance.

## 9. Target Execution Modes

Current source implements all three modes in the Rust `exec` facade:

- **Gated:** run on a fork branch, return diff, adopt by explicit merge.
- **Direct:** apply immediately for low-risk append-only or immutable operations.
- **Batched:** run many operations against one branch and merge once.

The IDL, C ABI, bindings, hosted adapters, and served REST/JSON-RPC/gRPC listeners project those modes
through the canonical CBOR request. Every projection must preserve the same fail-closed behavior: a
denied operation aborts the run, no direct or batched commit is made, and the working tree is restored
to the starting commit.

## 10. Determinism and Metering Requirements

The target public contract should preserve these rules:

- no wall clock, ambient randomness, locale, or environment access from guest programs;
- all external inputs are declared inputs;
- all state access goes through grants;
- execution is metered;
- output state is content-addressed and replayable;
- fuel accounting is engine-specific, while output object graphs are conformance-relevant.

Current source enforces this through the restricted WASM host ABI, fuel metering, deterministic
canonical envelopes, and `StateAccess` grant checks over the promoted multi-facet execution surface.
Target `engine=cel` preserves the same determinism rules through structural termination and the
deterministic CEL evaluator profile already used for guards. CEL program mutation semantics are not
source-backed; until the constrained action envelope is promoted, mutation-capable programs are
`engine=wasm`.

## 11. Non-Goals and Limits

- Source provides a Rust `exec` facade, IDL projection, C ABI projection, canonical request envelope,
  canonical result envelope, binding projections, hosted adapters, and served listeners, all carrying
  the same execution request/result bytes.
- The WASM host ABI covers files, KV (with bounded `kv_scan`), document, ledger, CAS, queue, graph,
  vector filtered search, columnar select and aggregate, search, dataframe, time-series, SQL, and
  calendar/contacts/mail domain calls.
- Guards, derivations, statecharts, and workflows are implemented in `crates/loom-compute` behind cargo
  features and live-wired through `StateAccess`; guards are additionally part of the manifest's
  content-addressed identity and enforced in all three execution modes.
- Executable exec conformance runs in `crates/loom-conformance` (`run_exec_behavior`).
- Calendar, contacts, and mail are not exposed as raw reserved-record CRUD through `StateAccess`.
  Source-backed Rust operations preserve domain boundaries: calendar collections and entries, contact
  books and entries, mailboxes, message indexes, immutable message bodies, and flags. The WASM host
  ABI exposes those same domain calls, and the 0029 trigger-fire bridge can execute a PIM program and
  append a fire record. `crates/loom-conformance` includes an executable `pim-trigger` runner for that
  bridge. Facet event emission and hook registration remain target work in 0041.
- Reactive firing, durable delivery, the change feed, and lifecycle hooks are out of scope for this
  spec and belong to 0029, 0035, 0030, and 0041; this spec covers only synchronous, request-scoped
  execution and the logic layers that run inside it.
- Source provides principal-aware Rust execution checks through `ExecContext`, an IDL/C ABI request
  contract, raw CBOR projection for every hand-written binding family, and hosted REST, JSON-RPC, and
  gRPC execution listeners.
- Source does not yet provide persisted `engine=cel` execution, MCP, binding, or hosted Program
  lifecycle projection, runtime-certified IDL/C ABI Program lifecycle behavior, public template
  projection, public `exec` envelope support for `engine=template`, or decoded program-ref execution
  wrappers. Persisted `engine=cel` target execution is read-only result logic first. CEL-originated
  mutation requires a constrained action envelope that Loom validates and applies outside the CEL
  evaluator; direct CEL host mutation through `StateAccess` is not the target.

## 12. Resolved Decisions

- **RD1 - Current substrate.** Current source-backed execution includes the canonical manifest,
  metered `engine=wasm` execution, multi-facet `StateAccess`, the canonical `exec` request/result
  envelopes, and the gated/direct/batched facade.
- **RD2 - Current gate.** Gated execution creates a proposal branch and leaves adoption to an explicit
  merge. Direct and batched execution commit only after every step succeeds.
- **RD3 - Current grant model.** Manifest grants are facet, mode, and scope. `ExecContext` supplies
  workspace, principal, branch, roles, authenticated state, and a context-grant upper bound for the
  private execution PEP.
- **RD4 - Current manifest.** The current manifest is Loom Canonical CBOR v1, deterministic,
  content-addressed, and includes guard declarations in program identity.
- **RD5 - Engine posture.** wasmi is the default and `wasm32`-capable engine. Wasmtime is optional
  native acceleration behind a feature.
- **RD5a - Program engine posture.** `engine=wasm` is the source-backed mutation-capable execution
  engine. `engine=cel` is the target interpreted, inspectable program engine for AI-authored persistent
  rules, filters, and bounded decision logic; current CEL source support is guard and ACL predicate
  evaluation only.
- **RD5b - CEL mutation posture.** The first promoted `engine=cel` execution profile is read-only result
  logic. Mutation from CEL must go through a constrained action envelope that is validated and applied
  by Loom outside the CEL evaluator. Direct CEL host mutation through `StateAccess` is not the target.
- **RD6 - Reactive boundary.** Trigger scheduling, durable delivery, change feeds, and facet lifecycle
  event emission are not owned by the synchronous `exec` facade.
- **RD7 - Public facade status.** The public `exec` facade is source-backed in Rust, IDL, C ABI, the
  generated C header, Node, Python, C++, Swift/iOS, JVM, Android, React Native, and WASM. Hosted wire
  adapters and served REST, JSON-RPC, and gRPC listeners are source-backed.
- **RD8 - Execution modes.** Rust `exec` v1 supports gated, direct, and batched modes. Direct and
  batched modes commit only after every step succeeds.
- **RD9 - Denial behavior.** A denied guest operation fails the run with `PERMISSION_DENIED`; direct and
  batched modes restore the working tree and do not create a commit.
- **RD10 - Request and result envelopes.** Public `exec` requests are canonical
  `loom.exec.request.v1` CBOR, and reports encode to canonical `loom.exec.result.v1` CBOR. Public
  projections must preserve those shapes rather than inventing per-binding result models.
- **RD11 - Batch authority.** Public batches carry a manifest per step. Request context/session grants
  are an upper bound, and each step narrows execution to its own manifest grants.
- **RD12 - PIM compute shape.** PIM automation combines lifecycle hooks and a domain-shaped
  `StateAccess` API. Calendar, contacts, and mail must not be exposed to programs as raw reserved-record
  CRUD because recurrence, privacy, contact merge semantics, mail bodies, flags, folders, and
  per-principal scoping are part of the contract. The Rust direct API, WASM host ABI, and trigger-fire
  execution bridge are source-backed; facet event emission and hook registration remain target work in
  0041.
- **RD13 - Dataframe compute shape.** Dataframe is program-grantable through Rust `StateAccess` with
  source and materialization target preflight. Guest WASM dataframe host calls are source-backed;
  dataframe-specific exec conformance expansion remains unfinished.
