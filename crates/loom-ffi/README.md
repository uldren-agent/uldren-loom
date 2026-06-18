# uldren-loom-ffi

The C ABI for Uldren Loom: a `cdylib` + `staticlib` (`libuldren_loom`) exposing the stable contract
that every language binding (Node, JVM, iOS/Swift, Android/Kotlin, C/C++, React Native, WASM) wraps.

Part of [Uldren Loom](https://github.com/uldrenai/uldren-loom).

## Build

```bash
cargo build -p uldren-loom-ffi --release   # -> target/release/libuldren_loom.{a,so,dylib,dll}
```

### Error contract

Every fallible function returns an `int32_t` status: `0` on success, else the stable error `Code` as
an integer (`Code::as_i32`, 1-based in declaration order). Results are written through out-pointers.
After a non-zero status, `loom_last_error(&code, &msg, &len)` returns the same code plus an owned
message string. Owned strings/handles from out-pointers are the caller's: free with `loom_string_free`
/ `loom_sql_close` / `loom_close`. (`loom_version` / `loom_blob_digest` are infallible and return the
string directly.) Structured result payloads cross as length-prefixed Loom Canonical CBOR bytes (free
with `loom_bytes_free`); `loom_result_to_json` renders a buffer to text for debugging only.

The C header is generated with cbindgen (`just header` -> `include/loom.h`). The surface is:

- `loom_version`, `loom_blob_digest` - library version and content-address helpers.
- `loom_sql_open` / `loom_sql_exec` / `loom_sql_commit` / `loom_sql_close` - a SQL session over a
  workspace SQL facet in a `.loom`: open it (the workspace is created on first use), run arbitrary SQL
  (results return as canonical-CBOR result payloads), and commit the staged result. This single path
  exposes the whole versioned tabular + SQL stack to any language - callers exchange SQL text and
  canonical CBOR rather than marshalling each rich column type across the boundary.
- `loom_open` / `loom_close` plus the direct, non-SQL engine surface for consumers that want
  structured access without writing SQL: version-control verbs (`loom_commit`, `loom_branch`,
  `loom_checkout`, `loom_log`, `loom_merge`), workspace history (`loom_vcs_blame`, `loom_vcs_diff`),
  and SQL table inspection (`loom_sql_read_table`,
  `loom_sql_index_scan`, `loom_sql_blame`, `loom_sql_diff`). Table data crosses as canonical CBOR in
  the same value shape as the SQL session.
- `loom_last_error(&code, &msg, &len)` - the calling thread's most recent error (code `0` + null
  message after success).
- `loom_string_free` - frees every string the library returns; sessions are freed with
  `loom_sql_close`, handles with `loom_close`.

## License

Business Source License 1.1 (BUSL-1.1). See the [repository](https://github.com/uldrenai/uldren-loom).
