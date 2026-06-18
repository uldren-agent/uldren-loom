# ADR-0001 - Reference implementation language

**Status:** Accepted · **Date:** 2026-06-14 · **Deciders:** Nas (+ Loom maintainers)

## Context

Loom must ship the *same* implementation to Node.js (the earliest consumers), the JVM, C++, and the
browser, and expose REST/JSON-RPC/gRPC. The hard, correctness-critical core is the content-addressed
Merkle data model and the git-like VCS engine (0002, 0003). Two architectural questions:

1. **Reuse vs. reimplement:** one shared engine behind a stable ABI, or N independent
   implementations against a conformance suite?
2. **If shared, which language** hosts the engine?

The owner is fluent in **C++** and trusts compiled languages, has **never used Rust**, and notes the
earliest consumers are **TypeScript** with its strong type system. The decision must weigh available
merkle/CAS libraries and binding maturity, researched below.

## Research findings (June 2026)

- **"One core, many bindings" is proven.** tree-sitter (C core; Rust, Python, Node, … bindings),
  libgit2, SQLite, and re2 all distribute a single native core with thin per-language wrappers. This
  is the lowest-risk way to guarantee identical behavior across languages and to avoid reimplementing
  subtle data-model logic five times.
- **Rust has the richest ready-made substrate.** `blake3` (official, SIMD, *itself a Merkle tree*
  with verified streaming), `fastcdc` (content-defined chunking) and `wasmtime`/`wasmi` (the
  execution substrate, 0015), plus jellyfish-merkle-tree and content-addressed-store
  crates. Bindings are best-in-class: `napi-rs` produces a native npm package with **prebuilt
  binaries for Windows/Linux/macOS on x64+arm64 and no `node-gyp`**, and `wasm-bindgen` gives a
  browser build.
- **Java native interop is no longer the blocker it was.** The **Foreign Function & Memory API
  (Project Panama)** was **finalized in JDK 22 (March 2024, JEP 454)**, replacing JNI with ~90% less
  boilerplate and reportedly 4-5× faster calls. "Same engine in Java" is now first-class for *any*
  C-ABI core.
- **TypeScript-first is fastest to prototype but a poor shared engine.** `isomorphic-git` (pure JS,
  runs in browser) and `merkletreejs` exist, but TS can't be the single engine for JVM/C++ without
  reimplementation, and `isomorphic-git` is volunteer-maintained with documented performance caveats
  and no native bindings.
- **C++ fits the one-core pattern** (libgit2 + OpenSSL, SWIG bindings) and the owner trusts it, but
  you build the merkle/CAS layer by hand, carry the memory-safety burden in exactly the component
  where corruption is unacceptable, and SWIG bindings are clunkier than `napi-rs`/Panama.

## Decision drivers

- **D1** Correctness/integrity of the storage core (memory safety matters most here).
- **D2** First-class, low-friction Node distribution (earliest consumers).
- **D3** Honest "same implementation" in Java and C++ without 5× reimplementation.
- **D4** Availability of merkle/CAS/git primitives to avoid building everything from scratch.
- **D5** Owner familiarity & long-term maintainability (C++ known; Rust new; TS known).
- **D6** Browser/WASM reach.

## Options considered

| Option                                                 | D1 safety  | D2 Node       | D3 JVM/C++           | D4 libraries            | D5 familiarity   | D6 WASM            |
| ------------------------------------------------------ | ---------- | ------------- | -------------------- | ----------------------- | ---------------- | ------------------ |
| **A. Rust core + bindings**                            | ★★★        | ★★★ (napi-rs) | ★★★ (Panama / C ABI) | ★★★ (blake3, fastcdc, jmt) | ★ (new to owner) | ★★★ (wasm-bindgen) |
| **B. C++ core + bindings**                             | ★ (manual) | ★★ (N-API)    | ★★ (JNI/SWIG)        | ★★ (libgit2; build CAS) | ★★★              | ★★ (Emscripten)    |
| **C. TypeScript-first, port later**                    | ★★         | ★★★           | ✗ (reimplement)      | ★★ (isomorphic-git)     | ★★★              | ★★★                |
| **D. Language-neutral, N impls vs. conformance suite** | varies     | ★★★           | ★★★                  | varies                  | ★★★              | ★★★                |

## Decision (recommended)

**Adopt Option A with a TypeScript twist:** a **Rust core engine** behind a **stable C ABI**, with
thin bindings via `napi-rs` (Node), `PyO3`/maturin (Python), Panama/FFM (JVM), `cbindgen` (C/C++),
and `wasm-bindgen` (browser) - **plus a pure-TypeScript reference implementation** maintained as the
readable
*executable spec* and a second conformance oracle (0007 §1, 0010 §3).

> **Superseded in part by the Decision outcome below:** the owner declined the pure-TypeScript
> reference implementation (q3). The oracle role moves to an in-memory Rust `MemoryProvider`; the
> browser is served by the WASM build. The Rust-core / C-ABI / bindings recommendation stands.

Rationale: it maximizes D1 (memory-safe core for the corruption-critical layer), D2 (native npm with
prebuilt binaries today), D3 (Panama makes JVM first-class; C ABI is C++), D4 (the merkle/git
substrate already exists in Rust), and D6 (WASM). The one weak driver, D5, is mitigated structurally:
**the Rust is the only Rust anyone owns** - every consumer (TS, Java, C++) writes their familiar
language against the binding, and the TS reference impl gives the owner an auditable codebase in a
trusted language.

## Consequences

- **Positive:** single correctness surface; identical cross-language behavior by construction; native
  Node perf and DX; honest multi-language story; browser support; rich library reuse.
- **Negative / risks:** the team takes on Rust for the core (learning curve; mitigated by the small,
  well-bounded engine surface and the in-memory Rust `MemoryProvider` oracle). FFM fixes the floor at
  **JDK 22+** (the JNI fallback is dropped per the outcome below).
- **Revisitability:** the interface (0003) and wire protocols (0008) are strictly language-neutral
  and the ABI (0007 §2) is the only coupling point. If Rust proves untenable, the core can be
  reimplemented in C++ behind the *same* C ABI with **zero** change to bindings, protocols, or
  callers - Option B becomes a drop-in swap, and Option D (independent impls) remains available
  because the conformance suite (0010) is the ultimate arbiter.

## Decision outcome (2026-06-14)

The owner ratified the recommendation with the following resolutions to the open questions:

1. **Core language: Rust.** Accepted. The core engine is Rust behind the stable C ABI (0007 §2).
2. **JVM binding: FFM (Project Panama) only.** JDK **22+** is the supported floor; the optional
   JNI fallback for JDK 17-21 is **dropped** unless a concrete consumer requires it. This keeps the
   JVM binding small and modern.
3. **No pure-TypeScript implementation.** Dropped, for two reasons the owner raised: (a) it would
   add build-time and maintenance burden, and (b) porting the full feature set out of Rust into TS
   would be costly and a likely source of divergence. Consequences and the replacement plan:
   - **Browser/JS support comes from the WASM build** of the Rust core (0007 §7), *not* from a TS
     port. WASM fully covers the browser target; a pure-TS engine was never the only path to the
     browser. (Filesystem import/export (0012) is unavailable in-browser; `SingleFile` runs over in-memory/OPFS
     byte storage; sync runs over `fetch`/WebSocket.)
   - The TS impl's *other* role - a second, independent implementation acting as a conformance
     oracle - is replaced by an **in-memory reference Provider written in Rust** (a `BTreeMap`-backed
     `MemoryProvider`) used for property-based differential testing, plus the language-neutral
     **canonical test vectors** (0010 §3). This preserves the cross-checking benefit without a
     second language runtime.

> Confirmation requested by the owner, answered: **yes**, the browser is reachable purely via the
> WASM path; a hand-written TS port is unnecessary and is not planned.

This ADR is now **Accepted**; future reversals would be recorded in a superseding ADR rather than
by editing this one.
