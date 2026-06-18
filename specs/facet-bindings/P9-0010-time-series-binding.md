# P9-0010 - `time-series` Binding

**Series:** P9 binding plan (normative-track sub-series; Draft)
**Version:** 0.1.0-draft. **Status:** Draft. **Last updated:** 2026-07-02
**Reads first:** [`P9-0002-projection-conventions.md`](./P9-0002-projection-conventions.md), facade spec
**0022 section 4** (TimeSeries), [`IMPLEMENTATION-FIDELITY.md`](./IMPLEMENTATION-FIDELITY.md) (no
tags/labels, no query language; thinner than InfluxDB or Prometheus).

## 1. Current Source Boundary

`loom-core::timeseries` stores each named series as one canonical blob at
`/.loom/facets/time-series/<name>` through the workspace working tree.

The source-backed model is:

- signed 64-bit timestamps with no encoded unit;
- opaque byte values;
- `put(ts, value)`, where a repeated timestamp replaces the point at that timestamp within one writer;
- `get(ts)`;
- half-open `range(from, to)` with `from <= ts < to`;
- `latest()`;
- canonical encode/decode;
- commit, checkout, clone, bundle, and sync through ordinary workspace file contents;
- IDL, CLI, and local MCP projections for put, get, range, latest, and list_collections.

Source now includes listener-bound hosted REST, JSON-RPC, and native gRPC listeners for the core
`put`, `get`, `range`, and `latest` methods, plus structured point put/range, policy, rollup
materialization, rollup range, and explicit prune. Native gRPC range operations are server-streaming.
The structured model includes measurement, sorted tags, typed fields, and nanosecond timestamps.
There is no source-backed point-level merge, generated schema artifact, collection discovery method,
or public hosted conformance runner today.

### 1.1 Binding Boundary

The base layer is a named ordered series of timestamped byte values plus a structured point-tree model.
Native projections expose put, get, range, latest, collection listing, structured points, policy,
rollup materialization, rollup reads, and explicit prune where the binding supports the hosted surface.
Influx, Prometheus, Grafana, and OTLP-facing surfaces are first-class presentations over the structured
model. Line protocol and OTLP JSON are ingest formats. Rollups and downsampling windows are derived
artifacts; automatic retention compaction and query indexes remain target work.

## 2. Target Facade Surface

The source-backed local facade exposes:

```text
put(series: string, ts: i64, value: bytes)
get(series: string, ts: i64) -> Option<Point>
range(series: string, from: i64, to: i64) -> Stream<Point>
latest(series: string) -> Option<Point>
list_collections() -> List<string>
```

`Point` is `{ ts: i64, value: bytes }`. `range` is half-open `[from, to)`.

Use the stable core error set until a time-series-specific `Code` is added. Absent series or points map
to `NOT_FOUND` or `None`, depending on the projection. Do not claim `SERIES_NOT_FOUND` as implemented
unless the stable `Code` enum grows that variant.

## 3. Tier-1 REST

Current source has a listener-bound hosted REST facade for configured `time-series/rest` listeners.
The listener selectors bind `{workspace, series}`, and the current routes are `POST /time-series:put`,
`POST /time-series:get`, `POST /time-series:latest`, `POST /time-series:range`,
`POST /time-series:put-structured`, `POST /time-series:range-structured`,
`POST /time-series:policy`, `POST /time-series:set-policy`,
`POST /time-series:materialize-rollup`, `POST /time-series:range-rollup-structured`, and
`POST /time-series:prune-before` with JSON bodies. Legacy timestamps are signed integers and point
values are carried as `value_hex`.

The resource-oriented target shape remains:

Facet-root `/v1/workspaces/{workspace_id}/series`:

| Facade method | HTTP |
| --- | --- |
| `put` | `PUT /series/{series}/points/{ts}` with bytes body |
| `get` | `GET /series/{series}/points/{ts}` |
| `range` | `GET /series/{series}?from=<from>&to=<to>` as NDJSON or binding-profile stream |
| `latest` | `GET /series/{series}/latest` |
| `list_collections` | `GET /series?list=1` |

The target REST contract must state that `to` is excluded.

## 4. Tier-1 JSON-RPC

Current source has `timeseries.put`, `timeseries.get`, `timeseries.range`, `timeseries.latest`,
`timeseries.put_structured`, `timeseries.range_structured`, `timeseries.policy`,
`timeseries.set_policy`, `timeseries.materialize_rollup`, `timeseries.range_rollup_structured`, and
`timeseries.prune_before` over `time-series/json_rpc` listeners, with `time-series.*` accepted as
canonical method spelling and `timeseries.*` accepted as an alias. `timeseries.list_collections`,
streaming, and generated JSON-RPC schema artifacts remain target work.

## 5. Tier-1 gRPC

Current source has `loom.hosted.v1.TimeSeries` over configured `time-series/grpc` listeners.
`Put`, `Get`, `Latest`, `PutStructured`, `Policy`, `SetPolicy`, `MaterializeRollup`, and
`PruneBefore` are unary. `Range`, `RangeStructured`, and `RangeRollupStructured` are
server-streaming and use bounded response batches. Batched writes, generated protobuf artifacts,
collection discovery, and hosted conformance remain target work. Batched writes must preserve the
repeated-timestamp replacement rule unless 0022 changes it.

## 6. Tier-1 MCP

- **Read tools:** `timeseries.get`, `timeseries.range`, `timeseries.latest`,
  `timeseries.list_collections`.
- **Write tool:** `timeseries.put`, token-gated per P9-0002 section 5.

## 7. Tier-2 Foreign Adapter

Source-backed first-class foreign surfaces:

- `influx/http`: `POST /api/v2/write` and `POST /write` parse Influx line protocol into structured
  points. The Influx bucket or v1 db parameter selects the time-series collection.
- `prometheus/http`: `POST /api/v1/write` accepts Snappy-compressed Prometheus remote-write protobuf
  samples and maps `__name__` to measurement, labels to tags, and sample value to `value`. Simple
  selector `query` and `query_range` JSON responses are source-backed.
- `grafana/http`: datasource health, search, and query routes read canonical structured points and
  return Grafana-style datapoints for exact metric selectors.
- `otlp/http`: `POST /v1/metrics` accepts OTLP HTTP JSON metrics for gauge and sum datapoints. Metrics
  map to structured points; logs and traces remain outside this metrics surface.

Full PromQL, Prometheus remote read, InfluxQL, Flux, full Grafana plugin behavior, OTLP protobuf/gRPC,
grouped aggregations, automatic downsampling, and query indexes remain target work.

## 8. Errors, Parity, and Concurrency

- **Errors:** current source uses the stable core error set. A time-series-specific code is target work.
- **Parity:** the current Rust substrate is portable and has no native engine dependency.
- **Concurrency:** same `(series, ts)` cross-peer collision policy is unresolved and follows
  `CONFLICT-RESOLUTION-MATRIX.md`; no point-level merge is source-backed.

## 9. Resolved Decisions

- **RD1 - Range bounds.** The public target uses half-open `[from, to)` ranges because source already
  does, contiguous windows compose without double-counting, and the convention matches Loom range scans.
- **RD2 - Current timestamp unit.** The source stores a signed 64-bit timestamp with no encoded unit.
  A public facade may document a recommended unit later, but changing identity semantics requires
  conformance coverage.
- **RD3 - Repeated timestamp.** Current source replaces the point at that timestamp within one writer.

## 10. Structured Foreign-Surface Mapping

Influx maps measurement, tags, fields, and nanosecond timestamps directly to the canonical structured
point. Prometheus maps `__name__` to measurement, remaining labels to tags, and each sample to a typed
`value` field. Grafana queries canonical points and derived rollups without owning storage. OTLP metrics
uses a profiled tag mapping; logs and traces remain outside this metrics-only contract.

## 11. Open Questions

### OQ-TS2 - Add a tag/label dimension for Influx/Prometheus fidelity? (open)

- **Context.** Full Influx/Prometheus-style adapters need tags or labels. The current Loom point model
  has only `(series, ts, value)`.
- **Example.** `http_requests_total{method="GET",code="200"}` or
  `cpu,host=a,region=us value=0.6` cannot be queried by tag without a model change.
- **Resolution.** First-class structured tags and typed fields are selected. Foreign adapters normalize
  into the 0022 canonical point model instead of embedding tags in a series key.
