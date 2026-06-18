# Execution & Logic Layer - Landscape and Options

**Status:** Exploratory / research note **Version:** 0.1.0-draft **Normative?** No.

Working data-gathering doc. It maps the option space for adding a *programmable logic* layer to
Loom: a way to attach executable behavior to the content-addressed store so that an author (often an
AI) can produce a "script" and have Loom run it deterministically against the data. Nothing here is
committed. It is the input to a future spec decision, in the same spirit as `0013` (exploratory
capabilities). House conventions apply: no em-dashes, no emoji, every claim reads as current fact.

## 1. Why this exists

Loom today is storage and access: a filesystem face, a version-control face, an optional SQL face,
and exploratory facets/adapters (`0013`), all over one content-addressed Merkle model (`0002`). It
has no way to *execute logic*. The missing third leg is computation: behavior that reads and writes
the store under rules.

The natural framing, because Loom is already a Merkle store with branch/merge, is a deterministic
**state transition**:

```
state_root_before  --( program_digest + inputs )-->  state_root_after
```

This is the smart-contract / "world computer" pattern minus distributed consensus. That pattern is
the most battle-tested answer to "run untrusted logic over a verifiable state store," which is
exactly the AI-authored case. Algorand is the reference point that started this thread: it runs the
**AVM** (Algorand Virtual Machine), a stack-based VM executing **TEAL** bytecode; logic is bounded by
a dynamic **opcode budget** (gas) and was deliberately non-Turing-complete before loops/subroutines
arrived in TEAL v4. The lessons we steal are bounded, deterministic, metered execution that *guards*
a state change, and code that can be statically analyzed before it runs.

## 2. The design axes

Every option is a setting of these dials. They are the columns of Table 1.

- **Expressiveness** - total/bounded (provably terminates, cannot express everything) vs.
  Turing-complete (expresses anything, cannot prove halting). Algorand's arc (start total, add
  bounded Turing-completeness) is the pragmatic middle.
- **Determinism** - same code + same input must yield the same output, so results are content-
  addressable. Requires no wall-clock, no ambient randomness, no float/iteration-order nondeterminism.
- **Termination / metering** - a gas/fuel/opcode budget so author logic cannot hang the engine.
- **Sandbox / capabilities** - the program touches only the state handed to it, through an explicit
  interface; no ambient filesystem or network. This is what makes AI-authored code safe to run.
- **Verifiability** - can the program be checked before it runs (types, totality, a static verifier)?
  This matters more when the author is an AI than when it is a human.
- **AI-authoring fit** - how reliably an LLM emits correct artifacts for this target. Small, typed,
  schema-validated, popular-language targets win.

## 3. Loom-specific constraints (the baselines these tables are graded against)

1. **Pure-Rust-linkable.** Anything adopted must compile in the Rust workspace and link into the
   native library (`uldren-loom-ffi` produces `cdylib` + `staticlib`). Source: `AGENTS.md`,
   `crates/loom-ffi`.
2. **WASM reach.** The browser story is WASM-only (no pure-TS engine; `adr-0001`, `0007` §7), and
   GlueSQL was chosen partly because it compiles to `wasm32` (`adr-0003`, `0011` §5). An execution
   engine that cannot compile to `wasm32` would be unavailable in the browser binding and on any
   JIT-forbidden target. JIT-based engines are the risk here.
3. **Permissive licenses only.** `deny.toml` allows `Apache-2.0` (incl. LLVM-exception), `MIT`,
   `MIT-0`, `BSD-2/3-Clause`, `ISC`, `CC0-1.0`, `Unicode-3.0`, `Zlib`. Copyleft is denied. **MPL-2.0
   is not on the allowlist**, which directly affects Cozo (see Table 4 and the open questions).
4. **Self-contained verifier.** If a candidate ships a "contract" verifier/prover, that verifier must
   itself be pure Rust and self-contained. We cannot shell out to external web/Python/solver tools
   (this rules out provers that depend on Boogie/Z3 or external toolchains).
5. **No `unsafe` outside `loom-ffi`** (`Cargo.toml` workspace lint), so engines relying on heavy
   `unsafe` in the core path add review burden.

## 4. Table 1 - Libraries mapped to the design axes

Ratings: Expressiveness {Total/bounded, Turing-complete}; Determinism {By design, Configurable,
Needs care}; Metering {Built-in, Structural, Host-imposed, None}; Sandbox {Hard, Soft, Read-only};
Verifiability {Static verifier, Type/schema check, None}; AI-authoring {Excellent, Good, Fair,
Build-time only}.

| Library / approach | Category | Expressiveness | Determinism | Metering | Sandbox | Verifiability | AI-authoring fit |
| ------------------ | -------- | -------------- | ----------- | -------- | ------- | ------------- | ---------------- |
| **cel-interpreter** (CEL) | Expression/predicate | Total/bounded | By design | Structural (no loops) | Read-only | Type check | Excellent |
| **regorus** (OPA/Rego) | Policy language | Total/bounded | By design | Structural | Read-only | Type/schema | Excellent |
| **serde_dhall** (Dhall) | Total config lang | Total/bounded | By design | Structural (total) | Read-only | Type check (total) | Good |
| **Cozo** (Datalog) | Declarative logic / DB | Total/bounded (stratified) | By design | Structural (terminates) | Soft (query scope) | Schema | Good |
| **Ascent** / **Datafrog** | Datalog libs (in-Rust) | Total/bounded | By design | Structural | n/a (host code) | Rust types | Build-time only |
| **statig** / **rust-fsm** | State machine (compile-time) | Total/bounded | By design | Structural | n/a (host code) | Rust types | Build-time only |
| Statechart interpreter (SCXML model, custom) | State machine (runtime) | Total/bounded | By design (if pure actions) | Host-imposed | Hard (host actions) | Schema (validate graph) | Excellent |
| **differential-dataflow** / **timely** | Incremental dataflow | Turing-complete (host code) | Configurable | Host-imposed | n/a (host code) | None | Build-time only |
| **Salsa** | Incremental memoized queries | Turing-complete (host code) | By design (pure queries) | Host-imposed | n/a (host code) | None | Build-time only |
| **Rhai** | Embedded scripting | Turing-complete | Configurable | Built-in (op limit) | Hard | None | Good |
| **mlua** (Lua) | Embedded scripting | Turing-complete | Needs care | Built-in (instruction hook) | Hard | None | Good |
| **starlark-rust** | Embedded scripting (no recursion/while) | Total/bounded | By design | Structural | Hard | Type/lint | Good |
| **Boa** (JavaScript) | Embedded scripting | Turing-complete | Needs care | Host-imposed | Soft | None | Excellent (JS) |
| **wasmi** | WASM interpreter | Turing-complete | Configurable (deterministic profile) | Built-in (fuel) | Hard (WASI caps) | Validation (well-formed module) | Excellent (any source lang) |
| **Wasmtime** | WASM JIT/interpreter | Turing-complete | Configurable | Built-in (fuel/epoch) | Hard (WASI caps) | Validation | Excellent (any source lang) |
| **Wasmer** | WASM multi-backend | Turing-complete | Configurable | Built-in (metering middleware) | Hard (WASI caps) | Validation | Excellent (any source lang) |
| **solana_rbpf** (eBPF VM) | Bytecode VM + verifier | Turing-complete (bounded) | By design | Built-in (compute budget) | Hard | **Static verifier (Rust)** | Fair (compile to eBPF) |
| **Move VM** (`move-vm-runtime`) | Resource bytecode VM | Turing-complete (bounded) | By design | Built-in (gas) | Hard | Bytecode verifier (Rust); Prover external | Fair |
| **Unison** (pattern only) | Content-addressed code | Turing-complete | By design | Built-in (abilities) | Hard (abilities) | n/a in Rust | n/a (no Rust runtime) |

Reading toward "maximize coverage" (see §11, decision 1): no single row scores high on every axis. The
axes split into two clusters that pull against each other - *expressiveness* (scripting, WASM, VMs)
versus *safety/analyzability* (expression languages, Datalog, statecharts). Coverage is therefore
maximized by **layering**, not by picking one engine: a constrained, verifiable layer for the common
case and a sandboxed Turing-complete substrate for the rest, sharing one determinism-and-metering
discipline. wasmi is the single row that gets closest to "expressive AND hard-sandboxed AND metered
AND deterministic-capable," at the cost of no pre-execution semantic verification beyond module
validation - which the constrained layer and the run-on-a-branch gate (§6) supply.

## 5. Table 2 - Libraries mapped to the storage facets they can drive

The facets are Loom's data shapes over the Merkle model: virtual filesystem (VFS), SQL/relational,
object/blob, key-value (KV), document, graph, columnar, append-only log, time series,
content-addressed store (CAS), and ledger (append-only, verifiable history). An execution engine can
only touch a facet through the host API Loom exposes to it, so the meaningful distinction is:
**Native** (the engine's data model *is* this facet), **Via host** (reachable through host functions
Loom binds in), **Read** (can read but not idiomatically mutate), or blank (not a fit).

| Library / approach | VFS | SQL | Blob | KV | Document | Graph | Columnar | Append-log | Time series | CAS | Ledger |
| ------------------ | --- | --- | ---- | -- | -------- | ----- | -------- | ---------- | ----------- | --- | ------ |
| cel-interpreter (CEL) | Read | Read | Read | Read | Read | Read | Read | Read | Read | Read | Read |
| regorus (Rego) | Read | Read | Read | Read | Read | Read | Read | Read | Read | Read | Read |
| serde_dhall (Dhall) | Read | | | Read | Read | | | | | | |
| Cozo (Datalog) | | Native | | Via host | Native | Native | Read | | Read | | |
| Ascent / Datafrog | | Native | | | | Native | | | | | |
| statig / rust-fsm | Via host | Via host | Via host | Native | Native | Via host | | Via host | | | Via host |
| Statechart interpreter | Via host | Via host | Via host | Native | Native | Via host | | Via host | Via host | | Via host |
| differential-dataflow / timely | | Via host | | Via host | Via host | Native | Via host | Native | Native | | |
| Salsa | Via host | Via host | Via host | Via host | Via host | Via host | | | | Via host | |
| Rhai | Via host | Via host | Via host | Via host | Via host | Via host | Via host | Via host | Via host | Via host | Via host |
| mlua (Lua) | Via host | Via host | Via host | Via host | Via host | Via host | Via host | Via host | Via host | Via host | Via host |
| starlark-rust | Via host | Via host | Via host | Via host | Via host | Via host | Via host | Via host | Via host | Via host | Via host |
| Boa (JS) | Via host | Via host | Via host | Via host | Via host | Via host | Via host | Via host | Via host | Via host | Via host |
| wasmi / Wasmtime / Wasmer (WASM) | Via host | Via host | Via host | Via host | Via host | Via host | Via host | Via host | Via host | Via host | Via host |
| solana_rbpf (eBPF) | Via host | Via host | Via host | Via host | Via host | Via host | Via host | Via host | Via host | Via host | Via host |
| Move VM | Via host | Via host | Via host | Native (resources) | Native | Via host | | Via host | | | Native |

Reading toward "possible coverage across storage types" (see §11, decision 2): the imperative engines
(WASM, scripting, eBPF) reach **every** facet, but only as far as the **host capability API** Loom
exposes - their coverage is really a statement about *Loom's* host interface, not the engine. The
declarative engines are narrow but deep: Cozo natively covers relational + document + graph (and is
the strongest single fit for the AI-memory facets in `0013`). The practical conclusion is that
storage-facet coverage is decided by **the capability/host-function surface Loom defines for executed
code**, and that surface should be designed once (a `StateAccess` capability interface) and shared by
every engine, rather than re-bound per engine. CEL/Rego are deliberately read-only and belong on the
*guard* path (validate a proposed transition), never the *mutate* path.

## 6. Table 3 - The proposed build, as a solving / not-solving matrix

This renders the "what I'd actually build" recommendation as components, so coverage can be grown by
filling gaps. The architecture is layered: a sandboxed Turing-complete substrate, a constrained
verifiable layer on top for the common case, content-addressed program identity, one
determinism-and-metering discipline, and a run-on-a-branch verification gate.

| Component | Solves | Explicitly does NOT solve (yet) | Loom seam it hooks into | Effort | Primary risk |
| --------- | ------ | ------------------------------- | ----------------------- | ------ | ------------ |
| **Execution substrate** (wasmi default; Wasmtime optional native fast-path) | Run arbitrary author logic, sandboxed, metered, deterministic; any source language to WASM | Pre-execution *semantic* proof; cross-program composition/scheduling | New `exec` facade over `0003`; capability host API over `0002`/`0011` | High | Determinism config drift; WASM toolchain UX for authors |
| **Constrained layer** (statecharts + CEL/Rego guards; Datalog for derivations) | The 80% case authored reliably by an AI and validated before running; pure guards/predicates | General computation; long-running stateful workflows | Sits above the substrate; guards gate `vcs_commit`-style transitions | Medium | Two authoring models to document and test |
| **Content-addressed code** (program is an object, addressed by digest) | Dedup of identical logic; memoizable execution; replayable audit; "no build" identity (Unison idea) | A package/version/dependency story for programs | New object kind or reserved workspace in `0002` §3 / `0014` | Medium | New core object type touches the identity profile (`0002` §8) |
| **Determinism + metering discipline** (fuel budget; banned nondeterministic ops) | Hashable, reproducible results; DoS/hang protection (Algorand opcode-budget analogue) | Wall-clock-dependent or randomized logic (only via seeded, host-mediated inputs) | Cross-cuts the substrate; threat model in `0009` | Medium | Floating-point and iteration-order nondeterminism are easy to leak |
| **Run-on-a-branch gate** (execute against a copy-on-write branch, diff, then merge) | Safe execution of untrusted/AI code: inspect the diff before it touches `main` | Real-time/low-latency mutation (adds a branch+merge per transition) | Reuses VCS branch/merge (`0002` §6, `0003` §5) | Low-Medium | Merge/conflict semantics for machine-generated transitions |
| **Triggers / derived views** (reactive recompute on change) | Materialized views, derived data, change-driven logic | High-throughput streaming; exactly-once delivery guarantees | Extends `0011` views + `0013` facets; sync policy per `0006` | Medium | Do derived results sync or rebuild (mirrors `0013` OQ#2) |
| **Capability host API** (`StateAccess`: scoped read/write to facets) | One sandbox surface every engine shares; per-program least privilege | A full effect-system / capability-token economy | Extends `0009` §6 (capabilities) and the MCP surface (`0013` §B) | Medium | Getting the surface right once; it is hard to change later |

To "evolve this matrix for coverage," each row's *does NOT solve* cell is the backlog: composition
across programs, a program package/version story, seeded nondeterminism inputs, low-latency execution
without a branch, and streaming triggers are the named gaps.

## 7. Table 4 - Engineering fit against Loom's constraints

The decision table. "Self-contained Rust" means no external C/Python/solver toolchain at build or
run time (constraint §3.4). "wasm32" means the engine itself compiles to and runs on the `wasm32`
target (needed for the browser binding). Footprint is a rough order-of-magnitude band for added
binary size and is highly config-dependent; treat as relative, not absolute. License is checked
against the `deny.toml` allowlist.

| Library | Author language(s) | Compiles in Rust & links | Self-contained Rust | Runs on `wasm32` | Built-in metering | Self-contained verifier | Footprint band | License | Allowed by `deny.toml`? |
| ------- | ------------------ | ------------------------ | ------------------- | ---------------- | ----------------- | ----------------------- | -------------- | ------- | ----------------------- |
| cel-interpreter | CEL text | Yes | Yes | Yes | Structural | n/a | Tiny (<0.5 MB) | Apache-2.0 | Yes |
| regorus | Rego text | Yes | Yes | Yes | Structural | n/a | Small | Apache-2.0 | Yes |
| serde_dhall | Dhall text | Yes | Yes | Likely (heavier deps) | Structural | Type/total checker (Rust) | Small-Medium | BSD-3-Clause | Yes |
| Cozo | Datalog (CozoScript) | Yes | Yes (mem/sqlite backend) | Yes (has WASM build) | Structural | n/a | Medium | **MPL-2.0** | **No (not on allowlist)** |
| Ascent / Datafrog | Rust (build-time) | Yes | Yes | Yes | n/a | Rust types | Tiny | MIT / Apache-2.0+MIT | Yes |
| statig / rust-fsm | Rust (build-time) | Yes | Yes | Yes | n/a | Rust types | Tiny | MIT / Apache-2.0 | Yes |
| Statechart interpreter (build it) | JSON/SCXML | Yes (own code) | Yes | Yes | Host-imposed | Validate graph (Rust) | Tiny | n/a (first-party) | Yes |
| differential-dataflow / timely | Rust (build-time) | Yes | Yes | Yes (single-threaded) | Host-imposed | n/a | Small-Medium | MIT | Yes |
| Salsa | Rust (build-time) | Yes | Yes | Yes | Host-imposed | n/a | Small | Apache-2.0 / MIT | Yes |
| Rhai | Rhai text | Yes | Yes | Yes | Built-in (op limit) | n/a | Small (~1-2 MB) | MIT / Apache-2.0 | Yes |
| mlua | Lua text | Yes (builds C) | No (bundles Lua C) | Hard (C-to-wasm) | Built-in (hook) | n/a | Small | MIT | Yes (but not pure Rust) |
| starlark-rust | Starlark text | Yes | Yes | Yes | Structural | Lint/type pass (Rust) | Medium | Apache-2.0 | Yes |
| Boa | JavaScript | Yes | Yes | Yes | Host-imposed | n/a | Medium-Large | Unlicense OR MIT | Yes (via MIT) |
| **wasmi** | any to WASM | Yes | **Yes (pure-Rust interpreter)** | **Yes** | **Built-in (fuel)** | Module validation | Small-Medium | MIT / Apache-2.0 | Yes |
| Wasmtime | any to WASM | Yes | Cranelift JIT is Rust; JIT disallowed on some targets | Limited (JIT not on wasm; interpreter path needed) | Built-in (fuel/epoch) | Module validation | Large | Apache-2.0 WITH LLVM-exception | Yes |
| Wasmer | any to WASM | Yes | Depends on backend | Limited (backend-dependent) | Built-in (middleware) | Module validation | Large | MIT | Yes |
| **solana_rbpf** | C/Rust to eBPF | Yes | Yes (interpreter); JIT is native-only | Interpreter likely; JIT no | Built-in (compute budget) | **Static verifier (Rust)** | Medium | MIT / Apache-2.0 | Yes |
| Move VM | Move | Yes | Runtime/bytecode-verifier yes | Unverified; heavy | Built-in (gas) | Bytecode verifier yes; **Prover needs Boogie/Z3 (external)** | Large | Apache-2.0 | Yes (Prover fails §3.4) |
| Unison | Unison | **No (Haskell runtime)** | No | No | n/a | n/a | n/a | MIT (impl) | Pattern only |

The constraint filter is decisive. **wasmi** is the standout substrate: pure-Rust, links cleanly,
runs on `wasm32`, has fuel metering, permissively licensed - it satisfies every baseline at once,
trading raw speed for portability. **Wasmtime/Wasmer** are faster but their JIT path conflicts with
the `wasm32` and JIT-forbidden-target baselines, so they fit only as an optional native fast-path
behind the same interface. **solana_rbpf** is the strongest answer to the self-contained-verifier
baseline (§3.4): its verifier is Rust and in-process. **Cozo** is attractive for facets but currently
**fails the license gate** (MPL-2.0 is not on the `deny.toml` allowlist), which must be resolved
before it can be a dependency. **Move's Prover** fails §3.4 (external Boogie/Z3), though its in-Rust
bytecode verifier does not. **mlua** works but is not pure Rust and is awkward on `wasm32`.

## 8. Library reference and maturity

One row per candidate with its repository, a directional maturity band, and an AI-authoring score.
The maturity band is GitHub stars in broad buckets; it is directional, not exact, and the live count
is at the URL (the fetch path used here could not read the GitHub API, so bands are used deliberately
rather than stale point numbers). Bands: `>=10k` very large, `3-10k` large, `1-3k` moderate, `<1k`
small or niche. The **AI-authoring score** (0-10) is the author's estimate of how reliably a current
LLM writes correct logic in that engine's target language, weighing training-corpus prevalence,
syntactic regularity, and how easily the output can be validated before it runs; build-time-only rows
are scored on writing the host Rust they require.

| Library | Repository | Stars (band) | Author target language | AI-authoring score (0-10) |
| ------- | ---------- | ------------ | ---------------------- | ------------------------- |
| cel-interpreter | https://github.com/clarkmcc/cel-rust | `<1k` | CEL expressions | 6 |
| regorus | https://github.com/microsoft/regorus | `<1k` | Rego | 5 |
| serde_dhall (dhall-rust) | https://github.com/Nadrieril/dhall-rust | `<1k` | Dhall | 4 |
| Cozo | https://github.com/cozodb/cozo | `3-10k` | CozoScript (Datalog) | 5 |
| Ascent | https://github.com/s-arash/ascent | `<1k` | Rust (build-time) | 8 |
| Datafrog | https://github.com/rust-lang/datafrog | `<1k` | Rust (build-time) | 8 |
| statig | https://github.com/mdeloof/statig | `<1k` | Rust (build-time) | 8 |
| rust-fsm | https://github.com/eugene-babichenko/rust-fsm | `<1k` | Rust (build-time) | 8 |
| Statechart interpreter (first-party) | n/a (to build) | n/a | JSON / SCXML | 8 |
| differential-dataflow | https://github.com/TimelyDataflow/differential-dataflow | `1-3k` | Rust (build-time) | 8 |
| timely-dataflow | https://github.com/TimelyDataflow/timely-dataflow | `3-10k` | Rust (build-time) | 8 |
| Salsa | https://github.com/salsa-rs/salsa | `1-3k` | Rust (build-time) | 8 |
| Rhai | https://github.com/rhaiscript/rhai | `3-10k` | Rhai script | 6 |
| mlua | https://github.com/mlua-rs/mlua | `1-3k` | Lua | 7 |
| starlark-rust | https://github.com/facebook/starlark-rust | `1-3k` | Starlark (Python subset) | 7 |
| Boa | https://github.com/boa-dev/boa | `3-10k` | JavaScript | 9 |
| wasmi | https://github.com/wasmi-labs/wasmi | `1-3k` | any source language to WASM | 8 |
| Wasmtime | https://github.com/bytecodealliance/wasmtime | `>=10k` | any source language to WASM | 8 |
| Wasmer | https://github.com/wasmerio/wasmer | `>=10k` | any source language to WASM | 8 |
| solana_rbpf | https://github.com/solana-labs/rbpf | `<1k` | C / Rust to eBPF | 5 |
| Move VM | https://github.com/move-language/move | `1-3k` | Move | 4 |
| Unison (pattern only) | https://github.com/unisonweb/unison | `3-10k` | Unison | 3 |

Facet-engine alternatives referenced by the Cozo question (§11 decision 3):

| Library | Repository | Stars (band) | Pure Rust? | Covers | AI-authoring score (0-10) |
| ------- | ---------- | ------------ | ---------- | ------ | ------------------------- |
| usearch | https://github.com/unum-cloud/usearch | `1-3k` | No (C++ core, Rust binding) | Vector ANN | n/a (index API) |
| hnsw_rs | https://github.com/jean-pierreBoth/hnswlib-rs | `<1k` | Yes | Vector ANN | n/a (index API) |
| instant-distance | https://github.com/instant-labs/instant-distance | `<1k` | Yes | Vector ANN | n/a (index API) |
| petgraph | https://github.com/petgraph/petgraph | `3-10k` | Yes | In-memory graph algorithms | n/a (graph API) |
| indradb | https://github.com/indradb/indradb | `1-3k` | Yes | Graph datastore | n/a (graph API) |
| oxigraph | https://github.com/oxigraph/oxigraph | `1-3k` | Yes | RDF triplestore + SPARQL | 6 (SPARQL) |

The score concentrates at two poles: JavaScript (Boa, 9) and the WASM substrate authored from a
mainstream language (8) score highest because the corpus is enormous; the niche declarative and
resource languages (Rego, CozoScript, Dhall, Move, 4-5) score lowest. The statechart interpreter
scores 8 despite being first-party because the author target is schema-validated JSON, which LLMs
emit reliably and which can be checked before execution. This is the empirical case for the
constrained layer carrying the common path (see §9 and §11 decision 1).

## 9. Layering plan (libraries mapped to layers and facets)

This renders the agreed layered approach (see §11 decision 1). The stack is a convenience composition: the
embedding host can invoke the whole layered solution or call any individual engine directly, and the
general substrate (L4) is always present beneath the constrained layers, so nothing is locked away.

| Layer | Role | Candidate engine(s) | Author target | Facets it drives | Mutates state? |
| ----- | ---- | ------------------- | ------------- | ---------------- | -------------- |
| **L0 Identity** | Programs are content-addressed objects | First-party (reserved `program` workspace + manifest) | n/a | CAS; all facets as inputs | n/a |
| **L1 CEL Programs and Guards** | Validate a proposed transition before it commits, and provide the target interpreted `engine=cel` profile for persistent AI-authored decision logic | cel-interpreter, regorus | CEL / Rego | Reads all facets | No in the baseline profile; constrained action envelopes are target design |
| **L2 Derivations** | Declarative logic; derived and materialized views | Cozo or the permissive basket (§11 decision 3); differential-dataflow / Salsa | CozoScript / Rust | Relational, graph, document; columnar (read) | Derived writes only |
| **L3 Workflows** | Stateful processes and lifecycles | Statechart interpreter; statig / rust-fsm | JSON-SCXML / Rust | KV, document, append-log, ledger | Yes (through L4 caps) |
| **L4 Substrate** | General computation, sandboxed and metered | wasmi (default); Wasmtime / Wasmer (native fast-path); solana_rbpf | any to WASM / eBPF | All facets (through StateAccess) | Yes |
| **L5 Verification** | Check before commit | WASM module validation; eBPF static verifier; run-on-a-branch gate (first-party) | n/a | Gates writes to all facets | No (gates) |

The facet coverage of L2-L4 is the union of what the engines reach and what the L0/L4 capability
surface (`StateAccess`, §11 decision 2) exposes. L1 is deliberately read-only in the baseline profile:
it covers guards and the target `engine=cel` interpreted program shape for persistent AI-authored
decision logic. If CEL programs later produce writes, they should do so through an explicit constrained
action envelope rather than direct `StateAccess` host calls. Storage-facet coverage is therefore
maximized by L4 plus a complete `StateAccess` surface, with L2 adding deep declarative coverage of the
relational/graph/document facets.

## 10. Plain-language view: what a program can do against each facet

Cutting through "facets, layers, and components": this is the plain answer to the question that
matters. For each tree type in the store, can an AI reliably write a `program` (a small "smart
contract") and run it, and what can or cannot that program do? The short answer is yes for every
facet. A program runs on the general substrate (wasmi) and reaches each facet through the private
`StateAccess` surface (0015 §6), executed on a branch and verified before merge. What differs per
facet is the write semantics and how reliably an AI authors the logic.

Columns: **Read / Write** is what a program may do to that tree; **What the AI writes** is the
artifact the AI authors; **Can do / Cannot do** are concrete examples and limits; **AI reliability**
is a 0-10 estimate of how reliably a current LLM writes a correct program against that facet.

| Facet (tree type) | Read | Write | What the AI writes | A program can | A program cannot | AI reliability (0-10) |
| ----------------- | ---- | ----- | ------------------ | ------------- | ---------------- | --------------------- |
| **Filesystem tree (files)** | Yes | Yes (copy-on-write) | WASM via `StateAccess` file ops; statechart for workflows | Read, write, move, delete files; walk directories | Guarantee exact POSIX semantics (Loom is POSIX-like) | 9 |
| **Relational / SQL** | Yes | Yes (rows) | WASM via `StateAccess` row ops; SQL statements | Query and insert/update/delete rows; transact with files in one commit | Run heavy analytical scans cheaply (row-oriented) | 9 |
| **Key-value** | Yes | Yes | WASM via `StateAccess` KV ops | get / put / delete / scan by key | Serve as a RAM-speed cache (it is versioned) | 9 |
| **Document (JSON)** | Yes | Yes | WASM via `StateAccess` doc ops | get / put documents, lookup by index, read JSON paths | Rich cross-document queries without the SQL/JSON layer | 8 |
| **Graph** | Yes | Yes (nodes/edges) | Datalog (Cozo) for queries; WASM for mutations | Add and traverse nodes/edges; recursive queries if Datalog is present | Declarative recursive queries under the permissive basket (traversal turns imperative) | 7 |
| **Vector** | Yes (search) | Yes (insert; index rebuilt) | WASM via `StateAccess` vector ops | Insert embeddings, nearest-neighbor search | Diff/merge the ANN index, or guarantee exact recall | 8 |
| **Columnar** | Yes (scan/aggregate) | Yes (batch append) | WASM via `StateAccess`; SQL-OLAP (DataFusion) | Scan, aggregate, append segments | Efficient row-level random writes (read-optimized) | 7 |
| **Append-log / queue** | Yes (scan) | Append-only | WASM via `StateAccess` log ops | Enqueue, dequeue (advance a consumer), replay | Mutate or delete entries in the middle | 8 |
| **Time series** | Yes (range) | Append (by time) | WASM via `StateAccess` ts ops; rollups as derived views | Append points, range-query, downsample | Sustain very high ingest, or cheaply back-date edits | 8 |
| **Content-addressed store (CAS)** | Yes (by digest) | Yes (put returns digest) | WASM via `StateAccess` put/get | Store and retrieve immutable blobs; dedup | Mutate a blob in place; enumerate without an index | 9 |
| **Ledger** | Yes (verify) | Append-only (signed) | WASM via `StateAccess` plus signing; guard programs | Append verifiable entries, verify history, WORM holds | Rewrite or delete history; reach multi-writer consensus | 8 |

Rules that apply to **every** row: a program is deterministic (no clock or randomness except as a
seeded input) and runs under a budget; it reaches a facet only through `StateAccess`, on a branch,
with the diff reviewable before merge; and it touches a facet only within the **scope and mode** its
manifest grants (fine-grained facet + scope + mode least-privilege, 0015 §6.1). AI reliability is
highest where the author target is a mainstream language or a simple key/value model, and lowest
where it is a niche query dialect (graph Datalog) or has a subtle index model (columnar).

## 11. Decisions (baked into the specs)

The questions raised while building these tables are resolved; the answers live in the specs, not
here. For traceability:

1. **Coverage and layering: branches first, layer only where needed.** Leveraging the Merkle tree
   (run a program on a copy-on-write branch, diff, then merge) is the primary mechanism; engine
   layering (L1-L5, §9) is available but kept minimal rather than mandatory, since heavy layering
   trades one kind of complexity for another. The host can also call any individual engine directly.
   Baked into 0015 §3 and §8.
2. **One private `StateAccess` surface, fine-grained.** A single, versioned capability interface
   gives uniform facet coverage and one surface to audit, and it is private to the library (not in
   the C ABI, the IDL, or any binding). Grants are fine-grained (facet + scope + mode), declared in
   the program manifest and enforced per operation. Baked into 0015 §6.1/§7 and 0009 §1/§8.
3. **License: MPL-2.0 supported; the logic layer uses the permissive basket.** MPL-2.0 is now
   allowed (file-level copyleft, safe to link), so Cozo is available for the optional facet and
   AI-memory work; the compute/logic layer still prefers permissive alternatives where they suffice.
   Baked into `deny.toml` and adr-0004 Decision #8.
4. **Program identity: a content-addressed `Blob` plus a `program` workspace and manifest.** No new
   object type. Nothing is built or shipped, so the choice is made on merits (simplicity and reuse of
   the object model and a flexible manifest), not on avoiding breakage. The manifest's `engine` selects
   the execution profile: `engine=wasm` is source-backed for externally built WASM bodies, while
   `engine=cel` is the target interpreted profile for persistent AI-authored CEL instructions. Baked
   into 0002 §3.3 and 0015 §7.
5. **Home: a numbered doc (0015) plus propagation across the series.** Baked into 0000-index, 0001,
   0002, 0003, 0008, 0009, 0011, 0013, 0014, and the README.

## 12. Guard and derivation engine selection (L1 / L2)

The prototype proved the layers, then adopted the real L1 engine: guards are now **CEL** expressions
(`cel-interpreter`, `prototypes/loom-compute/src/guard.rs`), and recursive derivations now use
**ascent** (`ReachableCount`, a transitive closure, `workflow.rs`); scalar reductions remain folds.
This section picks the real
engines they would graduate to. The decision lens is the same one that chose wasmi over Wasmtime for
execution (§3): **permissive license, `wasm32` reach, small footprint, and bounded/terminating
evaluation** - because guards and derivations should also run in the browser binding, and we do not
want the logic layer to drag heavyweight native-only dependencies into the `wasm32` path.

**We pick exactly one engine per layer.** The engine is an implementation detail chosen by the Loom
maintainers; a program author or AI agent never sees it and never selects it - they write a guard
expression or a derivation rule against a stable surface. So "optional" is not a runtime menu and not
a choice we hand to the agent. The only place two engines legitimately coexist is execution, and only
because of a hard per-target forcing function: `wasm32` cannot run Wasmtime, so the *compile target*
picks the engine automatically (wasmi for the browser; Wasmtime if a native build wants the JIT). L1
and L2 have **no** such forcing function - CEL and `ascent` both reach `wasm32` - so there is no
reason to ship two; we choose one and integrate only that. Compiled footprint numbers to ground the
choice come from the probe harness in `prototypes/size-probes/` (`compare-l1-l2-size.sh`).

### 12.1 CEL (L1): read-only predicates and target interpreted programs

| Engine | Crate | License | `wasm32` | Bounded | Footprint | Fit for Loom guards |
| --- | --- | --- | --- | --- | --- | --- |
| CEL | `cel-interpreter` | MIT | yes | yes (non-Turing-complete by design) | light | Best baseline. Purpose-built for predicate/policy expressions; terminates structurally (no fuel needed); the prototype's `Predicate` AST is already a hand-rolled CEL subset, so this is a drop-in swap behind the guard surface. |
| Rego (OPA) | `regorus` (Microsoft) | MIT AND Apache-2.0 AND BSD-3-Clause | yes (wasm-pack; browser playground) | yes (terminating) | medium | Richer policy-as-code (rules, partial eval, data documents). Heavier and OPA-flavored; reach for it only if guards grow into full policy. Permissive, so `cargo deny` is happy. |

**Decision: pick CEL (`cel-interpreter`). One engine.** CEL matches the shape of a transition guard and
the target interpreted `engine=cel` program profile: it is non-Turing-complete (so the "bounded engine,
structural termination" property in 0015 §5 holds without a fuel budget), MIT, light, and
`wasm32`-capable. Rego (`regorus`) is the table above for comparison only - it is what we would choose
*instead* if guards ever needed full policy-as-code (rules, partial eval, external data). We would not
integrate both: an interpreted AI-authored program profile is one language, and the agent writes CEL
against it. The first `engine=cel` profile should be read-only decision logic; mutation should require
a separately specified constrained action envelope.

### 12.2 Derivations (L2): materialized / derived views

| Engine | Crate | License | `wasm32` | Kind | Footprint | Fit for Loom derivations |
| --- | --- | --- | --- | --- | --- | --- |
| Datalog (macro) | `ascent` | MIT | yes (emits plain Rust) | compile-time, semi-naive eval, lattices | very light (no runtime engine) | Best baseline. Generates ordinary Rust that compiles to `wasm32` like any other code, near-zero added runtime footprint, and more expressive than pure Datalog (aggregation via lattices fits incremental views). |
| Datalog (macro) | `crepe` | MIT | yes (emits plain Rust) | compile-time proc-macro | very light | Simpler and smaller than `ascent`, fewer features. Pick it if minimalism matters more than expressiveness. |
| Relational-graph-vector | `cozo` | MPL-2.0 (allowlisted, adr-0004 #8) | yes (wasm backend) | runtime engine | heavy | The "Wasmtime" of data engines: a full transactional Datalog DB that could *also* back the graph/vector facets. Optional native backend behind a feature, not the derivation baseline. |

**Decision: pick `ascent` (or `crepe` if its smaller feature set is enough). One engine.** The macro
engines emit ordinary Rust, so they inherit `wasm32` reach and add almost nothing to the binary - the
footprint probe should show `ascent` ~ baseline. Cozo is in the table for comparison, but it is a
*different category* - a full transactional Datalog DB - not a drop-in derivation engine. If Loom ever
wants a full data engine (e.g. to also back the graph/vector facets), that is a separate facet-layer
decision (0013), governed by the MPL-2.0 allowance (adr-0004 #8); it is not how we would implement L2
derivations, and we would not carry both.

### 12.3 Why this is one-each, unlike execution

Execution ships two WASM runtimes only because of a hard forcing function: Wasmtime cannot target
`wasm32`, so the browser binding *must* use wasmi, and the compile target selects the runtime
automatically - not the author, not the agent, not a runtime switch. The author-visible program engines
are different: `engine=wasm` is for uploaded externally built WASM bodies, and target `engine=cel` is for
AI-authored interpreted CEL instructions. L1 and L2 still have no reason to carry multiple languages:
we pick one interpreted language and one derivation engine, integrate only those, and the agent authors
against the one CEL surface. The guard evaluator is adopted through `cel-interpreter`; persisted
`engine=cel` program execution remains target work.

## 13. Sources

- Algorand TEAL / AVM specification: https://developer.algorand.org/docs/get-details/dapps/avm/teal/
- Discover AVM 1.0 (Turing-completeness, opcode budget): https://developer.algorand.org/articles/discover-avm-10/
- Algorand Python / PuyaPy: https://algorandfoundation.github.io/puya/
- Cozo (MPL-2.0, relational-graph-vector Datalog): https://github.com/cozodb/cozo
- wasmi (MIT/Apache-2.0, pure-Rust WASM interpreter): https://crates.io/crates/wasmi/0.30.0
- solana_rbpf (MIT/Apache-2.0, eBPF VM + verifier): https://crates.io/crates/solana_rbpf
- Rhai (MIT/Apache-2.0): https://rhai.rs/book/about/license.html
- starlark-rust (Apache-2.0): https://github.com/facebook/starlark-rust
- oxigraph (MIT/Apache-2.0): https://github.com/oxigraph/oxigraph
- Boa (Unlicense OR MIT): https://github.com/boa-dev/boa
- regorus (MIT/Apache-2.0/BSD-3-Clause, Rego/OPA interpreter, wasm): https://crates.io/crates/regorus
- cel-interpreter (MIT, Common Expression Language): https://crates.io/crates/cel-interpreter
- ascent (MIT, Datalog/logic programming via macros): https://github.com/s-arash/ascent
- crepe (MIT, Datalog as a procedural macro): https://github.com/ekzhang/crepe
