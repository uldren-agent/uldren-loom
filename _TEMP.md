# Open tasks

## Active Implementation Slices

The current local SQL slice is source-complete. React Native Android host verification passed on the
owner's system with `just react-native-android`.

| Priority | Active slice | Specs | Source-backed status | Remaining work | User effort needed |
| --- | --- | --- | --- | --- | --- |
| 1 | Finish promoted local SQL historical readers, schema-aware table diff, and cross-binding SQL fixtures. | 0011a, 0011b, 0008, 0010a | Core historical table/index readers, schema-aware table diff records, C ABI sync entry points, Node/Python/C++/iOS/Android/React Native/WASM projections, Android JVM smoke coverage, React Native host-test coverage, and `sql-history` conformance tracking are implemented. | Complete for local source-backed SQL history scope. | No owner decision needed. |

## Missed, Hidden, Or Incomplete Tasks

| Priority | Task | Why it exists | User effort needed |
| --- | --- | --- | --- |
| None | No hidden SQL-history issue remains. | `cargo test -p uldren-loom-conformance sql_history` passes with an indexed table that has a prior row change before a schema change. | None. |

## Spec-Backed Future Work

These items have owning specs and are not active implementation tasks until promoted.

| Future slice | Specs | Source-backed status | User effort needed |
| --- | --- | --- | --- |
| Hosted SQL protocols | 0008, 0011a | Local SQL facade and current folded MCP SQL tools are source-backed. Hosted REST, JSON-RPC, gRPC, and full MCP SQL session/batch tools remain target work. | Yes, before promoting hosted SQL transactions or full MCP SQL sessions/batches. |
| Foreign SQL wire adapters | 0011b | No PostgreSQL-wire or MySQL-wire adapter is source-backed. 0011b owns the target contract. | Yes, before promoting PostgreSQL-wire or MySQL-wire implementation. |
