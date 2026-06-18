# RFC-0001 - Promote `exec` to a conformance-profiled capability

**Status:** Proposed · **Targets:** 0010 §4 (capability registry), 0015 (execution & logic) ·
**Author:** Uldren · **Date:** 2026-06

## Summary

Promote the `exec` capability from **experimental** (the 0010 §4 registry) to
**conformance-profiled** - a capability backed by normative determinism vectors any implementation
must pass - now that a real, in-workspace execution core exists. This RFC defines the acceptance
criteria, records which are met, and gates the registry flip on the remainder.

## Motivation

`exec` (0015) has been exploratory: an engine prototype (`prototypes/loom-compute/`) excluded from
the workspace gate. With P2-P4 built, the **files-facet execution core** now lives in
`crates/loom-compute` and rides `just ci`: the wasmi engine, capability enforcement, and the
run-on-a-branch gate over the real `loom-core::vcs`. That is enough to begin profiling `exec` against
normative vectors rather than a sketch. Per 0010's resolved decision, promotion is **always an
explicit RFC** (not an implicit milestone flip); this is that RFC.

## Acceptance criteria

For `exec` to be **conformance-profiled**, an implementation MUST:

1. **Determinism.** The same program over the same base and inputs produces the same state root
   (root Tree digest) on the workspace engine (wasmi). [0015 §4]
2. **Metering.** An over-budget program aborts with `BUDGET_EXCEEDED` and leaves the base untouched -
   no partial state. [0015 §5]
3. **Capability confinement.** A program reaches state only through the private `StateAccess` surface,
   and each operation is checked against the manifest's fine-grained facet+scope+mode grants; a denied
   operation has no effect. [0015 §6]
4. **Run-on-a-branch gate.** A transition runs against a fork and is reviewable (before/after roots +
   diff) before it reaches a shared branch; nothing merges implicitly. [0015 §8]
5. **In-workspace conformance.** The engine rides `just ci` (fmt, clippy `-D warnings`, test, deny),
   and the determinism vector lives in `crates/loom-conformance` and runs against the engine.
6. **Cross-engine equivalence (if a second engine ships).** wasmi and any alternative engine
   (Wasmtime) yield the same state root for the vectors. [0015 cross-engine decision]

## Current status

| Criterion | Status |
| --- | --- |
| 1 Determinism | **Met** for the files facet (`loom-compute::gate`, test `run_is_deterministic`). |
| 2 Metering | **Met** (`budget_exceeded_leaves_base_untouched`). |
| 3 Capability confinement | **Met** for the files facet (manifest `GrantSet` enforced per op; `capability_denied_write_is_a_noop`). |
| 4 Run-on-a-branch gate | **Met** on `loom-core::vcs` (`gate::run_on_branch`: fork / run / diff / adopt-or-discard). |
| 5 In-workspace conformance | **Partial** - the engine rides `just ci`; the determinism vector is a `loom-compute` test today, not yet graduated into `loom-conformance`. |
| 6 Cross-engine equivalence | **Met (in-workspace)** - Wasmtime is promoted via dual-compile (wasmi is the default and the only `wasm32` engine; `engine-wasmtime` selects Wasmtime on native). `loom-compute`'s `wasmtime_matches_wasmi` test confirms both engines produce the same state root for the same program. |

Scope today is the **files facet**. The multi-facet `StateAccess` (KV, vector, graph, ...), the CEL
guard layer (L1), and the ascent trigger/statechart layers (L2/L3) remain in the prototype and
migrate as their facets land (P8+); they are out of scope for this profile and join it as they move
in.

## Proposal

1. **Now:** record `exec` as **conformance-profiled (files facet)** in the 0010 §4 registry, with
   criteria 1-4 and 6 met in-workspace and criterion 5 tracked below. The capability stays version 1.
2. **Before the unqualified flip:** graduate the determinism and cross-engine vectors from
   `loom-compute` tests into `crates/loom-conformance` (criterion 5). Widen the profile
   facet-by-facet as each facet's `StateAccess` migrates.

## Alternatives considered

- **Leave `exec` experimental until every facet and engine is migrated.** Rejected: the files core is
  real, tested, and in-workspace today; profiling it now pins the determinism / metering / confinement
  guarantees that matter most, and the registry can record a profiled *scope* short of `stable`.
- **Promote straight to `stable`.** Rejected: `stable` implies the full surface and cross-engine
  guarantees; this profile is partial by design.

## References

0010 §4 (capability registry) and its resolved RFC-promotion decision; 0015 (execution & logic,
status section); `crates/loom-compute` (`engine.rs`, `gate.rs`); `prototypes/loom-compute/` (the
un-migrated slice).
