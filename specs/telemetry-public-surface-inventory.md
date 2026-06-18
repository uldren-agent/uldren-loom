# Telemetry Public Surface Inventory

**Status:** Source-backed inventory for specs 0046, 0047, and 0048.

## Scope

This inventory records the promoted native public surfaces for Loom telemetry after MX-55. It covers
metrics, logs, and traces only. It does not define Prometheus, OTLP, Grafana, Influx, Elasticsearch,
hosted listeners, hosted compatibility routes, dashboards, vendor export contracts, or product
compatibility facades.

## Inventory

| Surface | Metrics | Logs | Traces |
| --- | --- | --- | --- |
| Owning spec | `specs/0046-metrics-layer.md` | `specs/0047-logs-layer.md` | `specs/0048-traces-layer.md` |
| Native crate | `crates/loom-metrics/src/lib.rs` owns descriptor and observation CBOR. | `crates/loom-logs/src/lib.rs` owns log record CBOR. | `crates/loom-traces/src/lib.rs` owns span CBOR. |
| Facet registry | `FacetKind::Metrics` in `crates/loom-types/src/workspace.rs`. | `FacetKind::Logs` tag `19` in `crates/loom-types/src/workspace.rs`. | `FacetKind::Traces` tag `20` in `crates/loom-types/src/workspace.rs`. |
| Core API | `crates/loom-core/src/metrics.rs` persists descriptors and observations and serves bounded queries. | `crates/loom-core/src/logs.rs` persists records and serves bounded half-open timestamp queries. | `crates/loom-core/src/traces.rs` persists spans, lists trace spans, and serves bounded start-time queries. |
| Result shape | Query returns canonical CBOR containing observation records, a partial flag, and a stale flag. | Query returns canonical CBOR `[records, partial]`. | Trace listing and query return canonical CBOR `[spans, partial]`. |
| IDL | `idl/loom.idl` exposes `Metrics`. | `idl/loom.idl` exposes `Logs`. | `idl/loom.idl` exposes `Traces`. |
| C ABI and header | `crates/loom-ffi/src/metrics.rs` and `include/loom.h`. | `crates/loom-ffi/src/logs.rs` and `include/loom.h`. | `crates/loom-ffi/src/traces.rs` and `include/loom.h`. |
| CLI | `crates/loom-cli/src/main.rs` exposes metrics descriptor, observation, and query commands. | `crates/loom-cli/src/main.rs` exposes logs put, get, and query commands. | `crates/loom-cli/src/main.rs` exposes traces put, get, trace-spans, and query commands. |
| MCP local | `crates/loom-mcp/src/tools.rs`, `reads.rs`, `writes.rs`, and `server.rs` expose metrics tools. | `crates/loom-mcp/src/tools.rs`, `reads.rs`, `writes.rs`, and `server.rs` expose logs tools. | `crates/loom-mcp/src/tools.rs`, `reads.rs`, `writes.rs`, and `server.rs` expose traces tools. |
| MCP remote | `crates/loom-mcp/src/lib.rs` forwards metrics tools over the remote backend. | `crates/loom-mcp/src/lib.rs` forwards logs tools over the remote backend. | `crates/loom-mcp/src/lib.rs` forwards traces tools over the remote backend. |
| Local client | `crates/loom-client/src/local.rs` and `crates/loom-client/src/service.rs`. | `crates/loom-client/src/local.rs` and `crates/loom-client/src/service.rs`. | `crates/loom-client/src/local.rs` and `crates/loom-client/src/service.rs`. |
| Remote Loom | `crates/loom-remote-protocol/src/generated_api.rs`, `crates/loom-remote-client/src/generated_client.rs`, and `crates/loom-hosted-core/src/generated_dispatch.rs`. | `crates/loom-remote-protocol/src/generated_api.rs`, `crates/loom-remote-client/src/generated_client.rs`, and `crates/loom-hosted-core/src/generated_dispatch.rs`. | `crates/loom-remote-protocol/src/generated_api.rs`, `crates/loom-remote-client/src/generated_client.rs`, and `crates/loom-hosted-core/src/generated_dispatch.rs`. |
| Bindings | Node, Python, WASM, C++, JVM, Android KMP, iOS Swift, and React Native expose raw-CBOR metrics wrappers. | Node, Python, WASM, C++, JVM, Android KMP, iOS Swift, and React Native expose raw-CBOR logs wrappers. | Node, Python, WASM, C++, JVM, Android KMP, iOS Swift, and React Native expose raw-CBOR traces wrappers. |
| Conformance | `crates/loom-conformance/src/behavior/metrics.rs` and metrics vectors. | `crates/loom-conformance/src/behavior/logs.rs` and `crates/loom-conformance/test-vectors/logs/native-v1.json`. | `crates/loom-conformance/src/behavior/traces.rs` and `crates/loom-conformance/test-vectors/traces/native-v1.json`. |
| Explicit non-goals | Prometheus, OTLP, Grafana, Influx, hosted compatibility routes, hosted listeners, and compatibility facades. | OTLP, Grafana Explore behavior, syslog or JSON log compatibility, Elasticsearch, hosted listeners, hosted compatibility routes, and product compatibility facades. | OTLP, distributed-tracing vendor compatibility, hosted listeners, hosted compatibility routes, dashboards, and product compatibility facades. |

## Remaining target work

- Metrics: automatic rollup materialization, typed result-view helpers beyond the raw-CBOR public
  boundary, hosted compatibility projection, and compatibility facades.
- Logs: retention policy, indexing, redaction policy, cursor semantics, lifecycle operations,
  cross-signal correlation indexes, hosted compatibility projection, and typed result-view helpers
  beyond the raw-CBOR public boundary.
- Traces: retention policy, indexing, redaction policy, lifecycle operations, cross-signal
  correlation indexes, hosted compatibility projection, and typed result-view helpers beyond the
  raw-CBOR public boundary.
