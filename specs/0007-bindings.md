# 0007 - Language Bindings

**Status:** Complete for the current source-backed binding boundary; target expansion is split.
**Version:** 0.1.0. **Normative for the ABI and error contract; informative for tooling choices.**

Loom ships one shared Rust core through a stable C ABI, with thin per-language bindings. This
document separates the source-backed binding surface from the target enterprise binding contract:
current bindings expose the C ABI, SQL/result surfaces, selected `LoomSession` and control-plane
helpers, and selected store creation paths; target bindings eventually project the full stable Loom IDL
(0003 section 3) and wire-safe error contract (section 6). The choice of core language is recorded in **ADR-0001**;
this document is written so that choice can change without altering the binding contract.

## Current implementation

Source-backed binding status today:

- `include/loom.h` and `crates/loom-ffi` expose the stable C ABI: version/digest helpers, store
  creation/opening including encryption keys, key-wrap management, workspace create/list/rename/delete,
  `LoomSession` authenticate/clear, identity list/add/set-passphrase/remove/role-assign/role-revoke/
  external-credential-create/external-credential-revoke, ACL list/grant/revoke,
  management KV tier configuration, CAS helpers, queue and queue-consumer helpers, SQL sessions and
  batches, SQL row streaming, typed result views, result-to-JSON helpers, direct VCS/table reader
  functions, raw execution CBOR through `loom_exec_cbor` including the SQL-backed guest execution host
  ABI, cooperative task handles, Lane management through canonical-CBOR `loom_lanes_*_cbor` functions,
  local daemon
  status/session/pin and lock acquire/refresh/release client helpers, local calendar/contacts/mail
  facet helpers, and bridge JSON for React Native.
- Node (`bindings/node`) exposes `version`, `blobDigest`, `createLoom`, workspace lifecycle helpers,
  CAS helpers, queue helpers, key-wrap management helpers, direct table/history readers, `LoomSql`,
  `LoomSqlBatch`, calendar/contacts/mail helpers, typed SQL results, canonical-CBOR bytes, result
  diagnostics helpers, row query, async SQL exec, encrypted opens, local daemon status/session/pin
  and lock client helpers, typed lock tokens, `lockAcquire`/`lockTryAcquire`/`lockRefresh`,
  release-by-token, scoped `withLock`, identity management including role and external credential
  management, SQL-backed raw execution CBOR, and VCS commits.
- Python (`bindings/python`) exposes workspace lifecycle helpers, queue helpers, key-wrap management
  helpers, direct table/history readers, calendar/contacts/mail helpers, SQL sessions, batches, row
  iterators, typed results, canonical bytes, result diagnostics helpers, JSON, local daemon
  status/session/pin and lock client helpers, typed lock tokens, `lock_acquire`/`lock_try_acquire`/
  `lock_refresh`, release-by-token, a `LockGuard` context manager, identity management including role
  and external credential management, SQL-backed raw execution CBOR, commit, and asyncio wrapper
  surfaces.
- C++ (`bindings/cpp`) wraps the C ABI with RAII helpers for errors, values, result views, SQL
  sessions, batches, row streams, async SQL exec, store creation, workspace lifecycle, queue helpers,
  `LoomSession` authenticate/clear, identity management including role assignment, role revocation,
  and external credential management, direct ACL management, KV tier configuration, key-wrap
  management helpers, direct table/history readers, local daemon status/session/pin and lock client
  helpers, typed lock tokens, a move-only RAII `lock_guard`, SQL-backed raw execution CBOR, and
  calendar/contacts/mail helpers.
- JVM (`bindings/jvm`) uses JDK 22 FFM and exposes version/digest helpers, store creation, workspace
  lifecycle helpers, queue helpers, direct table/history readers, calendar/contacts/mail helpers, SQL
  sessions, batches, row streams, typed result views, identity management including role assignment,
  role revocation, and external credential management, async SQL exec, local daemon status/session/pin
  and lock client helpers, typed lock tokens, and an `AutoCloseable` `LockGuard` for try-with-resources
  cleanup, and SQL-backed raw execution CBOR.
- iOS/Swift (`bindings/ios`) exposes version/digest helpers, store creation, workspace lifecycle
  helpers, `LoomSession` authenticate/clear, identity management including role and external
  credential management, direct ACL management, KV tier configuration, queue helpers, direct
  table/history readers, calendar/contacts/mail helpers, SQL sessions, batches, row streams, typed
  result views, async SQL exec, SQL-backed raw execution CBOR, commits, and checked-in runtime tests for result
  vectors and direct table/history readers.
- Android/Kotlin (`bindings/android`) uses JNI and exposes version/digest helpers, store creation,
  workspace lifecycle helpers, queue helpers, direct table/history readers, calendar/contacts/mail
  helpers, identity management including role assignment, role revocation, and external credential
  management, SQL sessions, batches, row streams, typed result views, SQL-backed raw execution CBOR, and commits.
- React Native (`bindings/react-native`) uses a TurboModule and exposes version/digest helpers, the
  capability report, workspace lifecycle, CAS helpers, queue helpers, calendar/contacts/mail helpers,
  identity management including role assignment, role revocation, and external credential management,
  direct table/history readers, stateless write-capable SQL exec/batch/bytes/JSON calls, read-only SQL
  row streaming, SQL-backed raw execution CBOR, and commit calls through bridge JSON where needed. Queue sequence
  and length values cross the bridge as unsigned 64-bit decimal strings, not JS numbers.
- WASM (`bindings/wasm`) exposes version/digest helpers, the capability report, conformance helpers, and
  a wasm32 OPFS session with typed SQL results, workspace lifecycle, CAS helpers, queue helpers,
  calendar/contacts/mail helpers, identity management including role assignment, role revocation, and
  external credential management, direct table/history readers, canonical bytes, JSON, query, commit,
  encrypted opens, SQL-backed raw execution CBOR, and read-only snapshots.

Not implemented today: generated bindings from `idl/loom.idl`, full filesystem/object facade
projection in every binding, Node/Python/JVM/Android/React Native/WASM session projection, full binding
distribution packaging, runtime conformance harnesses for every binding family, cross-binding
interoperability certification, the `watch` facade projection, and protocol projections generated from
the IDL. 0007a owns those target tracks.

## Target contract

The enterprise target remains one native core and one source of observable truth: every binding
preserves the stable `Code` enum, Loom Canonical CBOR result bytes, ownership rules, task semantics,
and conformance vectors. Language-specific APIs may be idiomatic, but they must not invent behavior
that is absent from the C ABI or source-backed IDL.

The stable digest, error, and tabular scalar model contracts are source-backed in `loom-types` and
re-exported by `loom-core`. The dataframe model, plan digest domain hashing, CSV/JSON/NDJSON portable
parsing, schema inference, row/scalar coercion, and loaded-batch portable executor are source-backed
in `loom-dataframe` and re-exported by `loom-core` where the public core surface requires it.
`loom-polars` owns the optional native dataframe executor. `loom-columnar` owns reusable columnar
model/interchange contracts. `loom-vector` owns reusable vector model, predicate, exact search,
accelerator policy, PQ, and vector accelerator contracts. `loom-pim` owns calendar, contacts, and mail
record/projection contracts. `loom-watch`, `loom-delivery`, and `loom-triggers` own reusable
observation, durable delivery, and reactive trigger contracts. Bindings still observe these contracts
through the C ABI and `loom_core::*` surfaces; reusable component crates use `loom-types` and domain
crates directly so they do not depend on the engine crate for foundational model types.

## 1. Strategy (informative)

The architecture is the proven "one native core, many thin bindings" model (tree-sitter, libgit2,
SQLite, re2): build the merkle/git engine **once**; expose it everywhere. Benefits: a single
correctness surface to test and fuzz; identical behavior across languages by construction; no 5×
reimplementation of subtle data-model logic (0002).

A second, independent implementation is valuable for differential testing, but **a pure-TypeScript
implementation is explicitly out of scope** (ADR-0001): porting the full feature set out of Rust
would add build/maintenance cost and invite divergence, and the browser is reached via the **WASM
build** (§7), not a TS port. The cross-checking role is filled instead by an **in-memory reference
Provider written in Rust** (a `BTreeMap`-backed `MemoryProvider`) used as the oracle for
property-based differential tests, together with the language-neutral canonical test vectors
(0010 §3).

## 2. The core engine & C ABI (normative)

The core exposes a **C ABI** - the lingua franca every target can call. The ABI is the contract;
the implementation language behind it is an internal detail (ADR-0001).

### 2.1 ABI shape

- A flat C header (`loom.h`) with opaque handle types (`LoomSession*`, `LoomSqlSession*`, `LoomIter*`,
  `LoomResultView*`, `LoomSqlBatch*`, `LoomTask*`) and
  functions taking/returning **plain C types and byte buffers**. No C++ name mangling, no language
  runtime assumptions.
- **Ownership:** every buffer/handle returned by the core is owned by the core and freed by its
  matching destructor - text strings with `loom_string_free`, structured result byte buffers with
  `loom_bytes_free`, task handles with `loom_task_free`, sessions/handles with
  `loom_sql_close` / `loom_close`. The caller never frees core memory with its own allocator. Buffers
  passed *into* the core are borrowed for the duration of the call only. This rule is the source of
  cross-language memory safety.
- **Errors:** every fallible function returns an `int32 status` (0 = ok) and writes results through
  out-pointers; on error it populates a thread-local `LoomError` retrievable via
  `loom_last_error(&code, &msg_ptr, &msg_len)`. `code` is the stable 0003 §8 enum.
- **Encoding:** structured results cross the ABI as **Loom Canonical CBOR** bytes (ADR-0010, §3)
  through an `(out_ptr, out_len)` pair freed with `loom_bytes_free`; JSON is debug-only, rendered on
  demand from a result buffer by `loom_result_to_json`. Text scalars whose value is genuinely a string
  (`loom_version`, `loom_last_error`, `loom_blob_digest`, and the `algo:hex` addresses from
  `loom_commit` / `loom_sql_commit`) cross as C strings. Other scalars cross directly.

### 2.1.1 Implementation status (informative)

What is built in `crates/loom-ffi` today (header `include/loom.h`, `cdylib` + `staticlib`
`libuldren_loom`):

- The **error contract of §2.1 is implemented**: every fallible function returns an `int32` status
  (`0` = ok, else the stable §8 `Code` as an integer via `loom_core::error::Code::as_i32`, 1-based in
  declaration order) and writes its result through an out-pointer; `loom_last_error(&code, &msg, &len)`
  returns the same stable code plus an owned message. `loom_version` / `loom_blob_digest` are
  infallible and return the string directly.
- `loom_version`, `loom_blob_digest` - version and content-address helpers.
- A **SQL session** that exposes the whole versioned tabular + SQL stack (0011) through one path:
  `loom_sql_open(loom_path, ns_name, db, &session)` (the `sql` workspace is created on first use),
  `loom_sql_exec(session, sql, &out_ptr, &out_len)` (result payloads as Loom Canonical CBOR),
  `loom_sql_commit(session, message, author, &out)` (the new commit's `algo:hex` content address), and
  `loom_sql_close(session)`. Callers send SQL text and receive canonical CBOR, so every language gets
  the full rich column-type system without per-variant marshalling across the boundary.
- A **direct (non-SQL) engine surface** over a `loom_open(loom_path, &handle)` handle, for consumers
  that want structured access without writing SQL: version-control verbs `loom_commit`, `loom_branch`,
  `loom_checkout`, `loom_log`, `loom_merge` (with a `cell_level` flag), and table inspection
  `loom_sql_read_table`, `loom_sql_index_scan`, `loom_sql_blame`, `loom_sql_diff`. Compute execution
  crosses as raw canonical CBOR through `loom_exec_cbor`: callers provide `loom.exec.request.v1` and
  receive `loom.exec.result.v1`. Structured results cross
  as Loom Canonical CBOR built by `loom-sql`'s `result_cbor`: each result is an explicit canonical
  envelope whose every scalar rides through the one type-faithful cell codec
  (`loom_core::tabular::cell_value`, the same codec the facets and storage identity use), so 128-bit
  integers, non-finite floats, exact `f32`, decimals, and byte strings all cross bit-exact (no
  serde_json route). `loom_result_to_json` renders any buffer to JSON for debugging only. The
  `loom_sql_index_scan` lookup prefix is itself canonical CBOR (a cell array, parsed by `loom-sql`'s
  `lookup_cbor` via the same `cell_from` codec), so the ABI speaks one canonical form for both argument
  and result - no JSON anywhere on the wire. Callers build the prefix with `loom_core::tabular::encode_cells`.
- The **async poll/handle primitive** (§2.2): `loom_sql_exec_async(session, sql, &task) -> LoomTask*`
  plus `loom_task_poll` / `loom_task_status` / `loom_task_result` / `loom_task_cancel` /
  `loom_task_free`. Result buffers reuse the `(out_ptr, out_len)` + `loom_bytes_free` convention.
- A **result-view** surface (§3.1): the C-ABI bindings do not decode Loom Canonical CBOR or replicate
  the cell tag table - they `loom_result_open(ptr, len, &view)` a result buffer once and read it through
  indexed, typed accessors backed by the one shared decoder (`loom-sql` `result_view::decode`) and the
  one faithful cell codec. `loom_result_len` / `loom_result_item_kind` enumerate items (SQL statements
  or a reader result); `loom_result_column_*` / `loom_result_row_count` / `loom_result_row_len` /
  `loom_result_cell` read tabular rows; `loom_result_count`, `loom_result_string_*`,
  `loom_result_variable_kind`, `loom_result_row_commit` (blame), `loom_result_diff_*`, and
  `loom_result_merge_outcome` cover the other shapes; `loom_result_close` frees the view. A scalar comes
  back in one `LoomValue` (one call per cell): 128-bit integers, decimal mantissas, and UUIDs as 16-byte
  little-endian fields (`scale` for decimal); floats as raw bits plus a convenience `f64`; text/bytes as
  borrowed pointers valid until close; `LIST`/`MAP` as canonical-CBOR bytes for the rare nested case.
  The Rust-native bindings (node/python/wasm) decode in-process via `result_view::decode` instead.
- A **lossless bridge-JSON projection** (`loom_result_to_bridge_json`): the one exception to the
  result-view surface, for React Native, whose bridge (folly::dynamic / JSI codegen) cannot carry
  `BigInt`, `Uint8Array`, or non-finite numbers. It renders a result buffer to a JSON string the RN
  layer returns and TypeScript `JSON.parse`s, decoded once in Rust on top of `result_view::decode` (so
  the CBOR decoder and cell tag table stay in Rust). It is lossless, not the debug shape: `null`,
  `bool`, in-range `i8`/`i16`/`i32`/`u8`/`u16`/`u32`, finite non-`-0.0` `f64`, and text are bare JSON;
  everything a JS `number` cannot hold exactly is a single-key tagged object (`$i64`, `$u64`, `$i128`,
  `$u128`, `$f32`, `$f64`, `$decimal`, `$bytes` as base64, `$uuid`, `$inet`, `$date`, `$time`,
  `$timestamp`, `$interval`, `$point`, `$map`); a `List` is a bare array; floats that can be NaN/Inf/
  `-0.0` carry raw IEEE-754 bits. This is a binding projection (`loom-sql` `bridge_json`), not a second
  wire form; the normative wire form remains Loom Canonical CBOR. Only React Native uses it.

The §2.1-§2.3 result-codec and async divergences are now closed at the C ABI: results carry Loom
Canonical CBOR (ADR-0010) and the poll/handle async form is built for SQL exec and the current direct
reader functions - `loom_sql_exec_async`, `loom_log_async`, `loom_merge_async`,
`loom_sql_read_table_async`, `loom_sql_index_scan_async`, `loom_sql_blame_async`, `loom_sql_diff_async` -
plus `loom_task_wait` as the synchronous drive convenience. Each synchronous reader and its `_async`
form share one implementation. Still tracked in 0007a: generated IDL projection, distribution
packaging, runtime harness expansion, typed result-view parity for bindings that currently expose raw
canonical bytes only, and full binding conformance coverage.

#### 2.1.2 Session lifetime & locking (normative)

A binding session/handle (`LoomSqlSession`, `LoomSession`, and the per-language `LoomSql` classes) is a
**reopenable handle** - it stores only `(path, workspace, database)` and does **not** hold the `.loom`
or its lock between calls. Each operation opens the loom for its own duration: **reads** use the
lock-free read path (concurrent readers never block, §6.4 of 0005 / #144), **writes** take the
exclusive single-writer lock only for that op. This is the LMDB/RocksDB-style model and matches the
`loom` CLI (one self-contained open per invocation). Consequences a binding must honor: (a) multiple
sessions over the same `.loom`, even in one process, coexist - none holds a lifetime lock; (b) the
exclusive lock is held only during an actual write, so a second writer is serialized, not rejected, as
long as no single session holds it open; (c) holding the loom open across many statements (for
throughput or multi-statement atomicity) is an **opt-in transaction/batch scope**, not the default.
Sessions must never acquire the lifetime write lock implicitly - that previously surfaced as a spurious
`CONFLICT: open for writing by another process` on a second open.

#### 2.1.3 Transaction / batch scope (normative)

The opt-in held-open scope of §2.1.2 is the **batch** (`LoomSqlBatch`, the C-ABI `loom_sql_batch_*`
surface, and the per-language batch classes). A batch holds the loom open - and its exclusive
single-writer lock - for its whole lifetime, and loads the SQL store **once**, so statements run against
one in-memory store. Two distinct notions of "commit" coexist and MUST be kept separate:

- **SQL transaction** (`BEGIN` / `COMMIT` / `ROLLBACK`): a logical scope **within** a batch. `BEGIN`
  snapshots the store, `COMMIT` promotes it, `ROLLBACK` discards every change since `BEGIN`. These are
  real, not advisory: an implementation MUST NOT silently accept `BEGIN` while leaving it a no-op
  (`loom-sql` implements GlueSQL's `Transaction` trait over the in-memory store). Nested transactions
  and a bare `COMMIT` / `ROLLBACK` (no open transaction) are **rejected**, not ignored. Because a SQL
  transaction needs the store to live across statements, it is valid only inside a batch (or self-
  contained in one multi-statement `exec`); a per-op session rejects a transaction left open at the end
  of a single call.
- **Batch commit** (the durability boundary): `loom_sql_batch_commit` persists the accumulated state and
  performs **one atomic save** - the engine's true atomic persistence boundary (#137), so a crash mid-
  batch leaves the pre-batch state intact. It is rejected while an explicit SQL transaction is still
  open. `loom_sql_batch_abort` discards un-persisted in-memory changes (and any open transaction);
  closing a batch without committing discards un-persisted changes.
- **VCS commit** is separate again: `loom_sql_batch_commit_vcs` persists + saves **and** records a
  history entry, returning its content address. It is distinct from a SQL `COMMIT`.

Bindings whose session already holds the loom open for its lifetime (the wasm32 OPFS session, whose
sync-access handle is acquired once) ARE the batch scope: they expose `BEGIN`/`COMMIT`/`ROLLBACK`
directly and persist only when no transaction is open. A binding whose bridge cannot hold a native
handle across calls (e.g. React Native's stateless TurboModule) MAY instead expose a single coarse
batch call (run N statements, commit, close in one round-trip).

### 2.2 Async model (normative)

The engine is synchronous and per-op lock-scoped (§2.1.2), and `wasm32` has no threads, so the ABI's
async surface is the **portable cooperative poll/handle** primitive - the single normative async form.
The core does **not** own a worker pool; concurrency is the binding's responsibility (Node's libuv
pool, a Python executor, the browser's dedicated Web Worker of 0160/#205). An `..._async` constructor
returns a pending `LoomTask*` that does no work until polled:

- `loom_task_poll(task, &done)` runs a pending task to completion and sets `done`. **Under this
  backend the first poll MAY block** the calling thread (there is no background pool), so a binding's
  async wrapper (`Promise` / `CompletableFuture` / `asyncio` task) MUST drive `poll` / wait **off the
  event loop or UI thread**, on its own worker.
- `loom_task_status(task)` reports `PENDING` / `READY` / `ERROR` / `CANCELLED` / `TAKEN` (`-1` for a
  null task).
- `loom_task_result(task, &out_ptr, &out_len)` transfers **exactly one** owned canonical-CBOR buffer
  (freed with `loom_bytes_free`) on success, or re-publishes the task's stored error to
  `loom_last_error` and returns its stable code (repeatably). Pending/cancelled/taken is
  `INVALID_ARGUMENT`.
- `loom_task_cancel(task)` is guaranteed only while the task is still **pending** (it becomes
  `CANCELLED` and never runs); a polled task is unaffected.
- `loom_task_free(task)` frees the handle (and any un-taken buffer it still owns); it is **separate**
  from `loom_bytes_free`, since a transferred result buffer is owned by the caller and outlives the
  task.
- `loom_task_wait(task, &out_ptr, &out_len)` is the synchronous-drive convenience: it polls to
  completion (the wait MAY block) and then behaves like `loom_task_result`. This is the one place a
  blocking wait on the caller's thread is sanctioned - a binding's own worker calls it.

Every reader has both shapes from one `*_ns` implementation: the synchronous `loom_X` and the
`loom_X_async` constructor (`loom_sql_exec`, `loom_log`, `loom_merge`, `loom_sql_read_table`,
`loom_sql_index_scan`, `loom_sql_blame`, `loom_sql_diff`). A binding drives the async form on its own
worker via `loom_task_wait` (or `poll`) to map onto its native async abstraction.

The source-backed local `watch` binding uses the same ABI rules: pull batches cross as canonical
`loom.watch.batch.v1` CBOR with `DataChange` envelopes and domain-owned `DomainChange` records, while
the watch cursor itself crosses as an opaque UTF-8 string. Bindings MUST NOT parse cursor internals for
behavior; they store and replay the string verbatim and surface `CURSOR_INVALID` unchanged. Hosted
streaming projection remains target work.

### 2.3 Streaming

Streams cross the ABI as iterator handles: `loom_iter_next(it, &item_bytes, &len, &done)` yields one
self-describing canonical-CBOR item (0007 §3) per call and sets `done = 1` when exhausted (then
`item_bytes`/`len` are `null`/`0`); `loom_iter_free` releases the handle. Each `item_bytes` is freed
with `loom_bytes_free`. The current constructor is `loom_sql_query`, which streams the rows of a
`SELECT`, one canonical-CBOR cell array per row. Listings, `walk`, `log`, large reads, and sync frames
are target users of the same `loom_iter_*` surface once those facades are projected.

## 3. Result/value codec (normative)

The normative codec for structured ABI result payloads is **Loom Canonical CBOR v1** (ADR-0010) - the
same codec that defines content-addressed object identity, so the ABI and the store share one byte
discipline rather than a second framing. It is:

- **Deterministic** (definite lengths, shortest-form integers, sorted+dedup'd map keys), so it doubles
  as a canonical form and gives conformance vectors one unambiguous byte target.
- **Self-describing** (a single canonical top-level item; composite results are CBOR arrays/maps).
- **Versioned** via the object frame's epoch (ADR-0010) where object identity is involved.

Result payloads are produced by `loom-sql`'s `result_cbor`, which builds each result envelope directly
in canonical CBOR and encodes every scalar through the one type-faithful cell codec
(`loom_core::tabular::cell_value` / `cell_from`) shared with the facet and storage-identity paths -
there is no serde_json route, so a value can never silently widen, truncate, or drop. JSON is never
the wire form: `loom_result_to_json` decodes a canonical buffer back to JSON for inspection only (a
faithful cell renders as a type-tagged scalar, e.g. `{"Int":1}`, a debug convenience rather than a
stable contract). An implementation MUST NOT substitute another codec for the
content-addressed forms; it MAY use Protocol Buffers internally for non-addressed transport (0008)
provided the external mapping round-trips.

### 3.1 Cross-language result-payload conformance vectors (normative)

The result codec is pinned by a small shared fixture, `bindings/conformance/result-vectors.json`, whose
single source of truth is the engine: `loom-sql` exposes `result_vector_payload` (a hard-typed reader
result exercising a 128-bit integer beyond `u64`, a NaN `f64`, an exact `f32`, a decimal, raw bytes, and
the easy scalars) and `result_exec_vector` (a portable `Int`/`Text`/`NULL` SQL exec), each with a pinned
blake3 content address (`RESULT_VECTOR_DIGEST`, `RESULT_EXEC_VECTOR_DIGEST`). A `loom-sql` test asserts
the live bytes equal the fixture, so the fixture can never silently drift from the engine.

Every binding's test suite MUST reproduce the exec vector through its own typed `exec` path and assert
the raw canonical bytes (`execBytes` / `sqlExecBytes`) are byte-for-byte equal to the fixture. Identical
canonical CBOR proves identical typed values across all bindings, because they all decode through the
one shared decoder (`result_view::decode`) or its RN projection (`bridge_json`). The Rust core verifies
two of these paths directly in-tree (the `loom-sql` decoder/bridge round-trip and the `loom-ffi` C-ABI
exec); the language suites (node, python, ios, ...) verify the rest.

## 4. Node.js binding (`@uldrenai/loom`)

- **Mechanism:** `napi-rs` (if the core is Rust) or `node-addon-api`/N-API (if C/C++), producing a
  Node-API addon. N-API gives ABI stability across Node versions and avoids `node-gyp` rebuilds.
- **Current source:** Node requires Node 20+ and exposes version/digest, store creation, `LoomSql`,
  `LoomSqlBatch`, typed SQL results, canonical bytes, row query, async SQL exec, encrypted opens,
  workspace lifecycle, CAS helpers, queue helpers, key-wrap management, direct table/history readers,
  calendar/contacts/mail helpers, result diagnostics helpers, local daemon status/session/pin and lock
  client helpers, and commits. Generated IDL projection and full distribution packaging remain target
  work in 0007a.
- **Target distribution:** prebuilt binaries per `(os, arch, libc)` - at minimum
  `{linux-x64-gnu, linux-arm64-gnu, linux-x64-musl, darwin-x64, darwin-arm64, win32-x64}` -
  published as optional-dependency packages so `npm install @uldrenai/loom` fetches the right binary
  with no toolchain. A WASM fallback (§7) covers unlisted platforms.
- **Target mapping:** IDL to TypeScript types (shipped `.d.ts`); `Future<T>` to `Promise<T>`;
  `Stream<T>` to `AsyncIterable<T>`; `bytes` to `Uint8Array`/`Buffer`; `Digest` to branded string
  `algo:hex`; `LoomError` to an `Error` subclass with `.code` (0003 §8) preserved verbatim (§6).
- **Threads:** the core owns no worker pool. Node async wrappers must drive blocking C ABI work off
  the event loop through the binding/runtime worker mechanism.

## 5. JVM binding (`ai.uldren:loom`)

- **Mechanism:** the **Foreign Function & Memory (FFM) API** (Project Panama), finalized in JDK 22
  (JEP 454). FFM replaces JNI with `MemorySegment`/`Arena`/`Linker`, drastically less boilerplate
  and faster calls. The checked-in JVM binding is hand-written over FFM rather than generated by
  `jextract`. **JDK 22+ is the supported floor; the JNI fallback is dropped** for the dedicated JVM
  binding. Android keeps its own JNI bridge because that is the Android platform boundary.
- **Current source:** JVM exposes version/digest helpers plus SQL sessions, batches, row streams,
  typed result views, async SQL exec, commits, encrypted opens, store creation, workspace lifecycle,
  queue helpers, direct table/history readers, and calendar/contacts/mail helpers. Full generated IDL
  projection and packaged native classifiers are target work in 0007a.
- **Target distribution:** the native library packaged in the JAR per platform, extracted to a temp/lib
  path at load, or located via `java.library.path`. A Maven/Gradle artifact per classifier.
- **Mapping:** `Future<T>` to `CompletableFuture<T>`; `Stream<T>` to `java.util.stream.Stream` or a
  custom `LoomStream` for backpressure; `bytes` to `byte[]`/`ByteBuffer`; `Digest` to a value-type
  `record Digest(String algo, byte[] bytes)`; `LoomError` to `LoomException` with an enum `code`.
  Memory crossing the boundary uses confined `Arena`s so native buffers are freed deterministically.

## 6. Error contract (normative, all bindings)

- Every binding MUST surface the stable `code` from 0003 §8 **unchanged**. Idiomatic wrapping
  (exception type, message localization) is allowed; the `code` field MUST be programmatically
  accessible and identical across languages and protocols (0008 §6).
- A binding MUST NOT collapse distinct codes into one, invent new codes outside the registry
  (0010 §4), or swallow errors into nullish returns.
- Panics/aborts in the core MUST be converted to `INTERNAL` errors at the ABI boundary, never
  unwound across it (no Rust panic / C++ exception crossing the C ABI - they are caught and
  translated).

## 7. C/C++ and WASM

- **C/C++ (`uldren-loom`, header `loom.h`):** consume `loom.h` directly, or the checked-in header-only
  C++ wrapper (`loom.hpp`) for RAII errors, result views, SQL sessions, batches, row streams, async SQL
  exec, store creation, workspace lifecycle, queue helpers, key-wrap management, direct table/history
  readers, local daemon status/session/pin and lock client helpers, and calendar/contacts/mail helpers.
  This is the most direct binding - it *is* the ABI.
- **WASM (`@uldrenai/loom-wasm`):** the core compiled to `wasm32` via `wasm-bindgen`. **This is the
  sole browser/JS-runtime path** - there is no pure-TS engine (ADR-0001). Current source exposes
  version/digest helpers, conformance helpers, and a wasm32 OPFS SQL session with typed SQL results,
  direct table/history readers, calendar/contacts/mail helpers, canonical bytes, JSON, query, commit,
  encrypted opens, and read-only snapshots. Browser filesystem interchange, live sync over
  `fetch`/WebSocket, and a full generated IDL projection remain target work.

## 8. Binding conformance (normative)

Current checked-in runtime suites cover Node, Python, and iOS direct table/history readers. Other
binding families remain build-gated for those wrappers until a CI-runnable runtime harness is chosen.

Target binding promotion requirements are tracked in 0007a and 0010a.

## 9. Python binding (`uldrenai-loom`)

- **Mechanism:** `PyO3` with `maturin` as the build backend, producing a native CPython extension
  built against the **stable ABI (abi3)** so one wheel per platform serves CPython 3.9+.
- **Distribution:** prebuilt `abi3` wheels per `(os, arch, libc)` published to PyPI, so
  `pip install uldrenai-loom` fetches a binary with no toolchain; an sdist covers unlisted platforms
  where a Rust toolchain is present.
- **Mapping:** IDL to Python types (shipped `.pyi` type declarations + `py.typed`); `Future<T>` to an `asyncio`
  awaitable; `Stream<T>` to an async iterator; `bytes` to `bytes`; `Digest` to a branded `algo:hex`
  string; `LoomError` to a `LoomError` exception with `.code` (0003 §8) preserved verbatim (§6).

## Resolved decisions

1. **JVM binding mechanism: FFM-only for the dedicated JVM binding.** JDK 22+ FFM is the v1 JVM path.
   Android keeps a separate JNI bridge because JNI is the platform boundary there. A desktop/server JNI
   fallback can be added only for a concrete enterprise requirement.
2. **Codec choice: Loom Canonical CBOR v1.** Structured ABI results use the same canonical codec as
   content identity. Protobuf is permitted only for non-content-addressed transport (0008) behind a
   round-trip guarantee.
3. **Schema-epoch policy: core accepts own-or-older, refuses newer with `UNSUPPORTED`.** This lets
   older bindings continue to work during rollout while making a binding that outruns its bundled core
   fail clearly instead of misparsing.
4. **Async shape: poll/handle only.** The C ABI exposes `LoomTask*` plus poll/status/result/cancel/free
   and wait helpers. Bindings wrap that in idiomatic async APIs and drive blocking work off event loops
   or UI threads.
5. **Digest representation: `algo:hex` at boundaries.** Bindings may expose local digest helper types,
   but C ABI, wire, fixtures, and cross-binding comparisons use the canonical textual address.
6. **Worker ownership: bindings own concurrency.** The core owns no worker pool and exposes no runtime
   pool-sizing knob. Bindings use their runtime's worker, executor, thread, or Web Worker model.
7. **Panic-to-`INTERNAL`: sanitize production messages.** The ABI boundary must not unwind core panics.
   Production messages are sanitized; debug builds may expose richer diagnostics.
8. **WASM durability: advertise platform durability.** OPFS-backed writable sessions must not claim
   stronger crash-consistency than the browser can provide. A reduced-durability capability flag is the
   target projection when the platform cannot honor native fsync semantics.

## Change log

### React Native Android Kotlin module: UldrenLoomNative FFI split (task 233c, Kotlin portion)

`bindings/react-native/android/.../rn/UldrenLoomModule.kt` (2,589) went to 1,844 with **no API change**.
Its 85 `override fun`s implement the codegen `NativeUldrenLoomSpec`, so they must stay in the class; the
decomposition pulls out the FFI boundary instead. The 84 `private external fun nativeX` moved to a new
`internal object UldrenLoomNative` (757 lines, with its own `System.loadLibrary("uldren_loom_rn")` init);
the override bodies now call `UldrenLoomNative.nativeX(...)`. The 84 JNI symbols renamed from
`Java_ai_uldren_loom_rn_UldrenLoomModule_native*` to `Java_..._UldrenLoomNative_native*` across the 13
per-facet `.cpp` files from the C++ split. The module keeps its `executor`, the `keyBytes`/`bytesToArray`
helpers, `getName`, and the companion's `loadLibrary` (the C++ TurboModule shares the same `.so`).
Verified: the 84 externals match exactly across the original module, `UldrenLoomNative`, and the renamed
C symbols; no override body has an unprefixed `native*` call and no `private external` remains in the
module; braces balance. This completes task 233 (non-Rust binding decomposition). Not buildable in the
dev sandbox; verified on Mac via `just test-bindings`.

### React Native iOS Obj-C++ module split (task 233d, iOS portion)

`bindings/react-native/ios/UldrenLoom.mm` (2,775) was split into per-facet Objective-C **categories**.
The primary `UldrenLoom.mm` keeps `RCT_EXPORT_MODULE()`, `version`/`blobDigest`, the ObjC helper methods
(`loomError`/`workQueue`/`openSession`/`openStore`/`beginBatch`), `getTurboModule`, and the five C
helpers (`loomBytesFromArray`/`loomArrayFromOwnedBytes`/`loomStringFromU64`/`loomParseU64`/
`loomResolveU64`), now lifted to non-`static` file scope. The 82 facet methods moved into 13
`@implementation UldrenLoom (<Facet>)` files. A new `UldrenLoom+Internal.h` declares the five C helpers
and an `@interface UldrenLoom (Internal)` with the five ObjC helper selectors, so each category TU
(`#import "UldrenLoom+Internal.h"`) can reach them; method dispatch is unchanged (categories are part of
the class at runtime, so TurboModule selector dispatch still resolves). The podspec already globs
`ios/**/*.{h,mm}`, so no podspec change. Verified: all 90 ObjC methods reconstruct exactly, the 5 C
helpers are preserved, `@implementation`/`@end` balance, and every category imports the internal header.

### React Native Android JNI bridge split (task 233d, C++ portion)

`bindings/react-native/android/src/main/cpp/UldrenLoom.cpp` (2,855) was split into per-facet
translation units with **no symbol change** (so the Kotlin module's `external` decls still resolve and
nothing else moves). The shared static helpers (`throwLoom`, `throwIllegalArgument`, `parseU64String`,
`u64String`, `openSessionKeyed`, `openStoreKeyed`, `ownedBytes`, `beginBatchKeyed`) became non-`static`
definitions in `UldrenLoom_common.cpp` behind a new `UldrenLoom_jni.h` (the JNI/ABI includes + helper
declarations); the 84 `Java_ai_uldren_loom_rn_UldrenLoomModule_*` functions moved into 13 per-facet
`UldrenLoom_<facet>.cpp` files, each `#include "UldrenLoom_jni.h"`. `CMakeLists.txt` now lists the 14
TUs. Verified: the 84 JNI symbols reconstruct exactly, all 8 helpers are declared in the header and
defined once (no `static` in `UldrenLoom_common.cpp`), braces balance, and the CMake source list matches
the files on disk. (The iOS `.mm` and the RN Kotlin module are separate slices.)

### Android KMP facet decomposition + LoomNative FFI split (task 233b)

The Android KMP binding's three monolithic `Loom` files were decomposed with **no public API change**:
`commonMain/.../Loom.kt` (1,023), `jvmMain/.../Loom.jvm.kt` (1,611), and `androidMain/.../Loom.android.kt`
(1,609). A Kotlin object body can't span files, so the 75 facet operations became `expect`/`actual fun
Loom.<op>(...)` **extension functions** in per-facet files under each source set's `facets/`
(`lifecycle`, `workspace`, `sql`, `vcs`, `queue`, `cas`, `kv`, `document`, `timeseries`, `ledger`,
`calendar`, `contacts`, `mail`) - callers still write `Loom.kvPut(...)` unchanged. `object Loom` keeps
only `init { System.loadLibrary(...) }` and the three direct externals (`version`/`blobDigest`/
`capabilities`). The raw JNI surface moved to a new `internal object LoomNative` (its own `loadLibrary`
init) holding the 75 `external fun nativeX`, and the facet bodies now call `LoomNative.nativeX(...)`. The
shared JNI C (`native/uldren_loom_jni.c`) renamed the 75 `Java_ai_uldren_loom_Loom_native*` symbols to
`Java_ai_uldren_loom_LoomNative_native*` (the 3 direct `Java_..._Loom_{version,blobDigest,capabilities}`
symbols, and the `LoomSql`/`LoomSqlBatch`/`LoomRowStream`/`LoomResult` class symbols, are untouched).
Kotlin source sets glob `facets/*.kt`, so no Gradle/CMake change was needed. Applying one mechanical rule
across all three source sets keeps expect/actual parity by construction; verified: public API lossless
(78 = 3 kept + 75 facets), facet-fun names identical across common/jvm/android, the 75 `LoomNative`
externals match the 75 renamed C symbols exactly, braces balance, and no facet body calls an unprefixed
`native*`. Not buildable in the dev sandbox; verified on Mac via `just test-bindings`.

### C++ wrapper header per-class decomposition (task 233e)

`bindings/cpp/include/loom.hpp` went from 1,585 lines to a 14-line umbrella that `#include`s eight
per-class sub-headers under `loom/`, preserving the single-include contract (`#include "loom.hpp"` is
unchanged). The shared prologue (banner, `#pragma once`, system includes, `#include "loom.h"`) moved to
`loom/prelude.hpp`; each class moved whole into its own header - `loom/error.hpp`, `loom/detail.hpp`,
`loom/value.hpp`, `loom/result.hpp`, `loom/row_stream.hpp`, `loom/engine.hpp` (the `Loom` session class,
the largest at ~937 lines), `loom/sql.hpp`, `loom/batch.hpp` - each `#include`-ing the previous so the
original top-to-bottom dependency order holds and every sub-header is independently includable. Because
whole classes moved (no out-of-line method surgery), inline bodies and default arguments are untouched.
Verified lossless (1,393 non-blank class lines reconstruct exactly) and **compile-checked** with
`g++ -std=c++17 -fsyntax-only` (clean). Further splitting `loom/engine.hpp` by facet would require
out-of-line method definitions; deferred as a follow-up.

The generated C ABI header `bindings/ios/.../include/loom.h` (cbindgen, "do not edit by hand";
regenerated by `just header`) is intentionally **not** split - hand-editing a generated single-header C
ABI would be clobbered on regeneration and is the kind of legacy debt to avoid.

### React Native TS facet decomposition (task 233c, TS portion)

`bindings/react-native/src/index.ts` went from 1,232 lines to a 16-line re-export hub with **no public
API change**. The shared pieces - the `keyArgs` helper and the `QueueSeq`/`LoomCell`/`LoomKey`/
`LoomStatement` types - moved to `src/internal.ts` (`keyArgs` is exported there so facet modules can
import it, but stays package-internal: `index.ts` re-exports only the four types via `export type`, so
the public surface is unchanged). The exported functions split into 13 per-facet modules under
`src/facets/` (`lifecycle`, `workspace`, `sql`, `vcs`, `queue`, `cas`, `kv`, `document`, `timeseries`,
`ledger`, `calendar`, `contacts`, `mail`), each importing `UldrenLoom` from `../NativeUldrenLoom` and
`keyArgs`/types from `../internal` as used; `index.ts` re-exports them with `export * from
'./facets/<f>'`. Verified lossless: the 1,142 non-blank declaration lines reconstruct exactly (multiset)
and no facet module calls a function defined in another. (The RN Kotlin TurboModule split is JNI-coupled
and lands with the RN C++ unit.)

### iOS Swift facet decomposition (task 233a)

`bindings/ios/Sources/UldrenLoom/Loom.swift` went from 1,514 lines to 585 with **no API change**. The
`Loom` class kept its stored `session` handle, the lifecycle statics (`version`/`blobDigest`/
`capabilities`/`create`/`open`/`openEncrypted`/`authenticate`), and the `takeBytes`/`openResult` result
helpers; the other types (`LoomError`/`LoomSql`/`LoomSqlBatch`/`LoomCell`/`LoomResult`/`LoomRowStream`)
were already separate and stayed. The 14 facets' instance methods moved into `extension Loom` files in
the same SwiftPM target (`Loom+Workspace`, `Loom+Identity`, `Loom+Acl`, `Loom+SqlTables`, `Loom+Vcs`,
`Loom+Cas`, `Loom+Kv`, `Loom+Document`, `Loom+TimeSeries`, `Loom+Ledger`, `Loom+Calendar`,
`Loom+Contacts`, `Loom+Mail`, `Loom+Queue`), which the target globs in with no `Package.swift` change.
Because Swift `private` is file-scoped, the three members the extensions reach - `session`, `takeBytes`,
`openResult` - were relaxed from `private` to the default `internal` (`LoomSql.lastError()` was already
internal; LoomSql's own `private session` is untouched). Verified lossless: the 839 non-blank facet
lines reconstruct byte-exact and every file's braces balance. Not buildable in the dev sandbox; verified
on Mac/Xcode via `swift build`.

### JVM binding Phase B: static facets removed, FFM into accessors (task 232 / 217i)

`bindings/jvm/src/main/java/ai/uldren/loom/Loom.java` went from 4,289 lines to 1,782 with **no
public API change** for callers already on the session model (task 222). Phase A had introduced the
twelve grouped accessor classes (`CasOps`, `KvOps`, `DocumentOps`, `TimeSeriesOps`, `LedgerOps`,
`QueueOps`, `VcsOps`, `CalendarOps`, `ContactsOps`, `MailOps`, `SqlTableOps`, `WorkspaceOps`) but had
them delegate to the flat `Loom.kvPut(...)`-style static methods; Phase B inverts that. The FFM
downcalls now live in the accessor classes and the 183 flat static facet methods are deleted. `Loom`
retains only the lifecycle surface (`version`/`blobDigest`/`capabilities`/`create`/`open`/
`openEncrypted`/`authenticate`), the `MethodHandle` constants, the `LoomResult`/`LoomSql`/`TsPoint`
types, and the shared FFM helpers. These are package-private so the accessor classes can reach them.

To keep the accessors DRY, a single combinator carries the open/close/last-error dance:

```java
@FunctionalInterface
interface HandleOp<T> { T run(Arena arena, MemorySegment handle) throws Throwable; }
static <T> T onHandle(String path, byte[] passphrase, byte[] kek, String op, HandleOp<T> body) { ... }
```

Every accessor method is now `Loom.onHandle(session.path, session.passphraseBytes(), null, op, (arena,
handle) -> { ... })`; the sql/vcs readers return a `Loom.LoomResult` via `Loom.openResult(...)` inside
the lambda. `LoomException` gained a `serialVersionUID`. Verified by reconstruction: all twelve accessors
route through `onHandle`, zero references to any deleted static remain (the only mention was a stale
`LoomSession` doc comment, now corrected), and braces balance. Not buildable in the dev sandbox; verified
on Mac via `just test-bindings`.

### WASM binding per-facet split (task 231 / 217h)

`bindings/wasm/src/lib.rs` went from 2,230 lines to 62 with **no API change**. The free
`#[wasm_bindgen]` functions (`version`/`blob_digest`/`capabilities`/`conformance_*`) stay in `lib.rs`;
the large inline `#[cfg(target_arch = "wasm32")] mod opfs_sql { .. }` became a file module `opfs_sql.rs`
(1,240 lines: imports, helpers, the `LoomSql` struct, the core `#[wasm_bindgen] impl LoomSql` -
open/create/exec/query/commit/workspace/sql/vcs - and the JS value-conversion helpers). The 63 facet
methods moved into nine per-facet sub-submodules under `opfs_sql/` (`cas`, `kv`, `document`,
`timeseries`, `ledger`, `queue`, `calendar_fns`, `contacts_fns`, `mail_fns` - the last three carry the
`_fns` suffix to avoid colliding with `use loom_core::calendar::{self, ...}` etc.), each a separate
`#[wasm_bindgen] impl LoomSql` doing `use super::*`. (wasm-bindgen supports multiple impl blocks across
modules; this is the one thing not buildable in the dev sandbox - it is verified via `just test-bindings` on
the wasm32 target.) Verified lossless: the de-indented method bodies are content-identical to the
original.

### Python binding per-facet module split (task 226 / 217c)

`bindings/python/src/lib.rs` went from 3,676 lines to 1,367 with **no API change** - the same 129
`#[pyfunction]`s are exported. They moved into the same 15 per-facet modules as the Node split (`cas`,
`kv`, `document`, `timeseries`, `ledger`, `calendar_fns`, `contacts_fns`, `mail_fns`, `queue`, `files`,
`vcs`, `sql`, `workspace`, `daemon_fns`, `admin`), each `use super::*`. pyo3 registers explicitly rather
than by ctor, so two things changed mechanically: the moved functions became `pub(crate)`, and every
`wrap_pyfunction!(name, m)` call in the crate `#[pymodule]` was rewritten to the module-qualified path
`wrap_pyfunction!(module::name, m)`. Helpers, the `LoomSql`/`LoomRows`/`LoomSqlBatch` pyclasses, their
`#[pymethods]`, and the `#[pymodule]` registration all stay in `lib.rs`. Verified lossless: the multiset
of non-blank source lines is unchanged modulo the added `pub(crate)` and `module::` qualifiers.

### Node binding per-facet module split (task 225 / 217b)

`bindings/node/src/lib.rs` went from 3,649 lines to 1,278 with **no API change** - the same 129
`#[napi]` functions are exported, registered exactly as before (napi-rs registers each `#[napi]` item
at its definition site, so module location is irrelevant). The 129 free napi functions moved into 15
per-facet modules (`cas`, `kv`, `document`, `timeseries`, `ledger`, `calendar_fns`, `contacts_fns`,
`mail_fns`, `queue`, `files`, `vcs`, `sql`, `workspace`, `daemon_fns`, `admin`), each doing `use
super::*` to reach the
crate-root helpers (`reason`, `ensure_*_ns`, `key_spec`, codecs, ...), type imports, and the `LoomSql`/
`LoomSqlBatch` class surface, all of which stay in `lib.rs`. `loom_core::*` calls travel with the
functions as extern-crate paths. (The lifecycle bucket is named `admin` rather than `core` to avoid
shadowing the `core` crate; the calendar/contacts/mail/daemon buckets carry a `_fns` suffix because
those bare names are already bound at the crate root by `use loom_core::calendar::{self, ...}` and
`use loom_store::{daemon}`.) The split was verified lossless: the multiset of non-blank source lines is
unchanged.

### Session/instance API for the JVM binding (task 222)

`Loom.open(path)` / `Loom.openEncrypted(path, passphrase)` / `Loom.authenticate(path, passphrase)`
return a `LoomSession` (`AutoCloseable`) that holds the path and unlock key so facet operations no
longer repeat them. Facet operations are grouped behind per-facet accessor classes, each its own file:
`session.kv()`, `.cas()`, `.document()`, `.timeSeries()`, `.ledger()`, `.queue()`, `.vcs()`,
`.calendar()`, `.contacts()`, `.mail()`, `.tables()` (read-only SQL readers), `.workspaces()`, and
`session.sql(workspace, db)` returning the existing `LoomSql` exec session. `workspace` stays a
per-call argument (matching the C ABI). The session stores the passphrase as a `String` and exposes a
`passphraseBytes()` helper, reconciling the binding's split key types (kv/cas/document/time-series/
ledger/calendar/contacts/mail take `byte[]`; queue/vcs/sql/workspace take `String`).

This phase is additive: the accessors delegate to the existing static `Loom.*` methods, which remain
during the transition.

The Android Kotlin binding has the same shape (`commonMain/.../LoomSession.kt`): a concrete
`LoomSession(path, passphrase?, kek?)` (with `LoomSession.open` / `openEncrypted` / `authenticate`
companion factories) and the identical grouped accessors, delegating to the existing `Loom` expect
object. Kotlin's facet functions are uniform (every call takes trailing `passphrase`/`kek`), so the
session forwards both; `session.sql(workspace, db)` selects the `LoomSql` constructor by which key is
held. It lives entirely in `commonMain`, so one source set covers JVM and Android.

Remaining (folded into task 217, since it is a file-size refactor needing a compiler in the loop):
removing the static facet methods and relocating the FFM machinery into the session/accessors, which
is what actually shrinks `Loom.java`. API-breaking is acceptable pre-release.

### Capability report binding projection (0004 section 4, 0010 section 5)

The source-backed `capabilities()` facade (canonical-CBOR build report; see 0010 section 5) is projected
into the bindings:

- `capabilities()` source-backed in: Node, Python (exported via `__init__`), C++
  (`loom::capabilities()`), Swift/iOS (`Loom.capabilities()`), WASM, JVM (FFM downcall), and Android
  (KMP `expect`/`actual` over a JNI `Java_..._capabilities`). Each returns the canonical-CBOR report,
  decoded like the other reader buffers; Node and Python carry focused tests.
- CAS facade now projected to JVM (`casPut`/`casGet`/`casHas`/`casListJson` via FFM over the existing
  `loom_cas_*` C ABI) and Android (KMP `expect`/`actual` + JNI), joining the existing Node/Python/C++/
  iOS CAS surfaces.
- Pending follow-on (next pass): React Native (`capabilities()` + CAS, TurboModule) and WASM CAS
  (direct over the OPFS loom).

### React Native + WASM completion (0004 section 4, 0010 section 5, 0024)

The follow-on above is now done; `capabilities()` and the CAS facade are source-backed in every
shipped binding:

- **React Native** (`bindings/react-native`): `capabilities()` and `casPut`/`casGet`/`casHas`/`casList`
  added across the TurboModule codegen Spec (`NativeUldrenLoom.ts`), the public TS wrappers
  (`index.ts`), the iOS Obj-C++ module (`UldrenLoom.mm`), the Android Kotlin module
  (`UldrenLoomModule.kt`), and the Android JNI shims (`UldrenLoom.cpp`). `capabilities()` is handle-free
  and resolves the canonical-CBOR report as a 0-255 number array; the CAS surface mirrors the queue
  methods (passphrase/`kek` keying, bytes as number arrays, `casGet` null on absence, `casHas` boolean,
  `casList` parsed from the JSON array).
- **WASM** (`bindings/wasm`): `capabilities()` was already source-backed; `cas_put`/`cas_get`/`cas_has`/
  `cas_list` are now methods on the `LoomSql` session handle (which holds the open OPFS loom and already
  exposes workspace/queue ops), calling the `loom_core::cas` functions with an `ensure_cas_ns` helper.

With this pass, `capabilities()` and CAS are source-backed in Node, Python, C++, Swift/iOS, JVM,
Android, React Native, and WASM. Remaining binding work is hosted-transport projection, not embedded
parity.

### Runtime provider/profile report

Native bindings expose a source-backed runtime provider/profile report as canonical CBOR:

- Node: `runtimeProfile()`.
- Python: `runtime_profile()`.
- C++: `loom::runtime_profile()`.
- Swift/iOS: `Loom.runtimeProfile()`.
- JVM: `Loom.runtimeProfile()`.
- Android/Kotlin: `Loom.runtimeProfile()`.
- React Native: `runtimeProfile()`.

The report is handle-free and describes the linked native artifact, not an open store. Its fields are
`binary_channel`, `runtime_policy`, `default_identity_profile`, `crypto_provider`, `tls_provider`,
`fips_capable`, and `fips_tls_claim`.

WASM exposes `runtime_profile()` with the same canonical-CBOR field shape. For WASM this is
compatibility evidence for the browser artifact, not a native FIPS certification claim.

FIPS package evidence is source-backed at the release-material layer. Node, Python, and WASM binding
crates expose a `fips` feature that propagates to `loom-core`; C ABI based bindings inherit the FIPS
profile from the packaged `libuldren_loom` artifact. `just binding-release-materials` and
`just binding-release-materials-fips` write binding package manifests with package names, build
recipes, runtime-profile surfaces, FIPS claim eligibility, discovered artifacts, and checksums. These
manifests are release evidence, not a publishing decision: alternate package names, classifiers, and
registry channels remain release-policy work.

Generated-header evidence is source-backed. `just header-check` verifies the public C header against
`crates/loom-ffi` cbindgen output and verifies the iOS vendored header against the public header.
Node, Python, and WASM Rust binding manifests compile through the stable `loom-core` projection
surface, while the Swift package builds against the vendored C header. Full generated bindings and
full wrapper drift detection remain 0007a target work.

Lane management evidence is source-backed at the shared C ABI layer. `include/loom.h` and the iOS
vendored header expose `loom_lanes_create_cbor`, `loom_lanes_get_cbor`, `loom_lanes_list_cbor`,
`loom_lanes_update_cbor`, `loom_lanes_ticket_add_cbor`, and `loom_lanes_ticket_remove_cbor`. These functions operate on
canonical-CBOR Lane records from the shared `loom-lanes` model. The read-only view functions
`loom_lanes_get_view_json` and `loom_lanes_list_views_json` return JSON Lane views resolved against a
ticket workspace, compact by default and detailed under a flag, joining per-ticket status, priority,
and title from the Tickets facet. Node and Python expose these Lane
operations as raw canonical-CBOR wrapper functions with binding-level export tests and supported
capability overlays. C++ and Swift/iOS expose ergonomic C ABI wrapper methods over the same
functions. WASM, JVM, Android, and React Native do not yet expose high-level Lane wrapper methods and
remain explicit unsupported follow-on surfaces for Lane wrapper ergonomics; that status does not
change the C ABI contract.

### Local lock binding ergonomics (0036)

Host-native bindings expose the daemon-backed local lock client through the shared C ABI rather than
duplicating the daemon wire protocol. The source-backed ergonomic layer is:

- Node: `lockAcquire`, `lockTryAcquire`, `lockRefresh`, `lockReleaseToken`, `parseLockToken`, and
  scoped async `withLock`. Numeric fence and deadline fields stay `bigint`.
- Python: `LockToken`, `lock_acquire`, `lock_try_acquire`, `lock_refresh`, `lock_release_token`, and
  `LockGuard`/`lock_guard` context manager helpers.
- C++: `lock_token`, `lock_acquire`, `lock_try_acquire`, `lock_refresh`, release-by-token, and a
  move-only RAII `lock_guard`.
- JVM: `LockToken`, `lockAcquire`, `lockTryAcquire`, `lockRefresh`, release-by-token, and an
  `AutoCloseable` `LockGuard` for try-with-resources.

Node and Python raw `lock_acquire_json` wrappers accept optional `wait_ms`, matching the C ABI and JVM
FFM surface. The default remains the daemon's bounded wait; `try_acquire` maps to `wait_ms = 0`.
General bindings still do not start or stop the CLI daemon.
