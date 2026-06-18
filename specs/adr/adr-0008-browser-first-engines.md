# ADR-0008 - Browser-first analytical and vector engines

**Status:** Accepted · **Date:** 2026-06-17 · **Deciders:** Nas
**Supersedes (in part):** ADR-0003 (the optional analytical engine choice).
**Related:** 0011 §5 (tabular engines), 0017 §5 (vector engine), 0023 §5 (columnar engine),
ADR-0001 (Rust core + a first-class `wasm32` browser build).

## Context

Loom's default build is **pure Rust and must compile to `wasm32` for the browser**; heavy,
native-only engines are acceptable only when feature-gated off the default/browser path (the
established `wasmi`-default / `Wasmtime`-gated precedent). Three engine choices were open:

1. the **analytical (OLAP)** engine for the tabular (0011) and columnar (0023) facets - ADR-0003 had
   tentatively named **Apache DataFusion**;
2. the **approximate-nearest-neighbour (ANN)** engine for the vector facet (0017) - `hnsw_rs` vs
   `instant-distance` vs `usearch`;
3. whether the columnar facet (0023) is served by the in-engine `StateAccess` substrate, by an
   analytical engine, or both.

Research (2026-06) into the candidates and into production vector databases (Qdrant, Weaviate,
Pinecone serverless, LanceDB) informs the decision.

## Findings (vetted)

- **DataFusion has a real wasm problem.** No official `wasm32-unknown-unknown` support; its
  transitive `stacker → psm` dependency runs a C build that needs a clang wasm toolchain, and the
  Arrow compute surface produces a large bundle. It is a fine *native* engine but a poor fit for the
  default browser build. (MIT/Apache-2.0; not a licence concern - a portability one.)
- **Polars** is MIT, a strong native analytical engine, and *can* be coerced to wasm, but only with
  the same `psm`/clang friction, single-threaded rayon fallback, feature pruning, and multi-MB
  bundles. So Polars is the right **native** OLAP engine but **not** a frictionless browser engine
  either.
- **Vector ANN:** `hnsw_rs` is actively maintained (2026), supports incremental insert, index
  persistence (dump/reload), true pre-filtering during traversal, and a rich metric set; but it
  pulls `mmap-rs`/`cpu-time`, which are **not** wasm-clean. `instant-distance` is leaner/closer to
  wasm-clean but **dormant since 2023**, immutable (build-once), and has no filtering. `usearch` is a
  C++ core behind a Rust binding - not pure Rust, browser only via Emscripten. (All permissive.)
- **Production vector DBs converge** on: fix vector **dimension + metric at creation and reject
  mismatches**; **pre-filter** (filter-aware traversal backed by a metadata index) rather than
  post-filter; and, for a single embedded instance with no replicas, keep the derived index
  consistent by **indexing a snapshot, brute-forcing the unindexed delta for correctness, and
  reindexing incrementally on a threshold** (the LanceDB model).
- **`wee_alloc` is dead** (unmaintained; repo archived 2025; RUSTSEC-2022-0054). The modern wasm
  default is the standard allocator, or `lol_alloc`/`talc` only if size profiling demands it.
- **Per-crate `[profile.*]` is ignored by Cargo** in dependency crates; size/perf profile knobs
  (`opt-level="z"`, `lto`, `codegen-units=1`, `panic="abort"`) belong in the **consuming
  application's** profile, not in a library crate.

## Decisions

1. **Avoid DataFusion and usearch entirely.** Neither is adopted, on any path.
2. **OLAP engine = Polars, native-only, feature-gated.** The analytical engine for the tabular
   (0011 `sql-olap`) and columnar (0023) facets is **Polars** (MIT), behind a native cargo feature
   (e.g. `engine-polars`). It is **never** on the default or `wasm32` path. GlueSQL remains the
   default tabular engine (pure-Rust, wasm-capable) per ADR-0003 - **confirmed**.
3. **Browser analytics use the in-engine substrate, not Polars.** In the browser, analytical reads
   run over Loom's own `StateAccess`/prolly-tree scans (correct, if slower); Polars is a native
   accelerator only. The columnar facet (0023) is therefore **both**: `StateAccess` is the always-on,
   wasm-safe storage + scan substrate; Polars is the optional native fast-path over the same
   segments.
4. **Vector ANN: exact search is the default + the cross-platform contract; `hnsw_rs` is the
   adopted opt-in accelerator.** Two facts settle this (researched & verified 2026-06):
   - **Exact (flat) search is feature-equivalent to HNSW**, not a lesser fallback. HNSW is purely a
     latency optimisation that is *approximate* (recall < 1); exact returns the guaranteed top-k
     (recall = 1.0) and gives **cleaner** metadata filtering (pre-filter then brute-force is always
     correct, whereas filtered-HNSW can lose recall). On-demand recompute is orthogonal to both. So
     HNSW adds **scale/speed at large N**, never a capability exact lacks.
   - **`hnsw_rs` *can* reach the browser** with modest work. Its wasm blockers are all **off** the
     in-memory build/search path: `mmap-rs` is confined to one reload-only module (`datamap.rs`),
     `cpu-time` is a single logging line, and `rand`/`getrandom` needs the `wasm_js` feature. A
     feature-gated fork (or upstream PR to issue #20, maintainer receptive) yields single-threaded
     browser HNSW in ~1-3 days. It is **not** permanently native-only.

   Therefore: the **default and the cross-platform contract is deterministic pure-Rust exact (flat)
   search** - same code on native and `wasm32`, identical results (same IEEE-754 arithmetic, ties
   broken by id), and since the index is derived and **never synced** (decision 5), a namespace built
   on one platform and opened on another searches identically. **`hnsw_rs` is adopted** as an
   **explicit opt-in approximate accelerator** behind a cargo feature: native today (its deps are
   fine off-wasm), browser later via the gated fork when corpus size makes exact's O(N·d) too slow.
   Because enabling HNSW is a deliberate *approximate/fast* choice the caller makes (results may
   differ from exact and across engines), the exact default keeps native and web in agreement by
   default. `instant-distance` is the wasm-clean-but-immutable alternative if we ever prefer it over
   forking. (For corpora up to ~tens of thousands of vectors - typical single-user browser data -
   exact is fully interactive and HNSW is unnecessary; the prototype in `prototypes/vector-tradeoff`
   quantifies this.)

   *Definition - "wasm-clean":* compiles to `wasm32-unknown-unknown` with no C/C++ build step and no
   dependency on facilities that target lacks (`mmap`, OS threads as a hard requirement, process/CPU
   clocks, filesystem syscalls); it must run single-threaded. A crate is wasm-clean only if **all**
   its non-optional dependencies are. `hnsw_rs` is **not** wasm-clean as published (non-optional
   `cpu-time` + `mmap-rs`), but is made so by a small fork because neither is on the core path.
5. **Vector semantics follow production practice (0017):** dimension + metric fixed at namespace
   creation, mismatch rejected (`DIMENSION_MISMATCH`); metadata **pre-filtering** pushed into search;
   the index is excluded from sync and rebuilt on receipt, kept consistent for a single instance by
   snapshot-index + exact-scan of the unindexed delta + threshold/background incremental rebuild.
6. **Build posture.** Drop `wee_alloc` (use the std allocator). Keep size/perf profile knobs in the
   consuming app: native `[profile.release]` favours speed (`opt-level=3`, `lto="thin"`, unwinding);
   a separate wasm profile favours size (`opt-level="z"`, `lto=true`, `codegen-units=1`,
   `panic="abort"`, `strip=true`) plus `wasm-opt -Oz`. Library crates stay profile-agnostic.

## Consequences

- **Positive:** the default build stays pure-Rust and browser-clean; no C/C++ toolchain, no clang
  wasm requirement, no multi-MB Arrow bundle in the browser. Heavy analytics are available natively
  when wanted, never silently required. **Vector search returns identical results on native and web
  by default** (exact is the contract everywhere), including across sync, since the index is derived
  and rebuilt locally rather than shipped; `hnsw_rs` is adopted as an explicit opt-in accelerator
  (native now, browser via a gated fork later) that the caller knowingly selects when trading recall
  for speed at scale.
- **Negative / risks:** exact search is O(n) per query, so vector latency grows with namespace size
  until a wasm-capable, results-reconciled HNSW lands - acceptable for small/medium sets and honestly
  documented (0017 §8). The analytical *performance* (not results) differs between the browser
  substrate and native Polars; that is a speed gap, not a correctness one, and is documented per
  facet. Bringing HNSW to the browser later needs a wasm-clean engine (a gated `hnsw_rs` fork, or a
  small in-house HNSW) and a result-reconciliation step so the platforms still agree.
- **Reversibility:** every engine sits behind a feature/trait, so swapping one later does not touch
  the facet's object model or interface; and because the ANN contract is defined by exact search,
  any future accelerator is validated against it rather than redefining behaviour.
