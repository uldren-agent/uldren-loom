# MU-6h-n-d-a2-c Evidence

## Current Submission

Completion state: Ready for skeptical review.

Agent 3 implemented Node.js `sqlExecResult` and Python `sql_exec_result` over generated
`Sql.sql_exec_result` through the shared authenticated generated-session helpers.

### Files

- `bindings/node/src/sql_generated.rs`
- `bindings/node/src/lib.rs`
- `bindings/node/index.js`
- `bindings/node/index.d.ts`
- `bindings/node/test.mjs`
- `bindings/python/src/sql_generated.rs`
- `bindings/python/src/lib.rs`
- `bindings/python/python/uldrenai_loom/__init__.py`
- `bindings/python/python/uldrenai_loom/__init__.pyi`
- `bindings/python/tests/test_loom.py`

### Behavior

- Both wrappers return canonical generated result bytes without decoding or reshaping them.
- Neither wrapper routes through the existing stateful SQL session classes.
- Focused tests cover successful mutation, malformed SQL, authentication before SQL parsing, and
  durable readback through a reopened wrapper.

### Verification

- Node.js and Python Cargo checks: passed.
- Node.js focused runtime script: passed.
- Python focused pytest: passed, one test with 36 deselected.
- Scoped Rust formatting and `git diff --check`: passed.

## Arbiter Review

Review state: Pending skeptical source review.
