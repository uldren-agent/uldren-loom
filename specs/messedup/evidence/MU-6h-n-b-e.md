# MU-6h-n-b-e Evidence

## Current Submission

Completion state: Ready for skeptical review.

Agent 1 implemented generated C ABI ownership for seven immutable Drive reads and four hierarchy
mutations.

### Files

- `crates/loom-client/src/service.rs`
- `crates/loom-ffi/src/drive.rs`
- `crates/loom-ffi/src/tests.rs`

### Behavior

- List, stat, read-file, versions, conflicts, shares, retention, create-folder, rename, move, and
  delete route through the generated `Drive` trait.
- The FFI layer performs C boundary conversion and owned result transfer only.
- Generated LocalLoomClient methods own Drive semantics, authorization, persistence, deterministic
  JSON, and raw file bytes.
- Expected-root inputs remain required where defined by the generated contract.

### Verification

- `cargo test -p uldren-loom-ffi --features integration-tests mu_6h_n_b_e -- --nocapture`: passed,
  two tests.
- Scoped Rust formatting: passed.
- Scoped `git diff --check`: passed.
- Header regeneration and consumer bindings were not run by task design.

## Arbiter Review

Review state: Pending skeptical source review.
