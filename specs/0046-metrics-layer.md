# 0046 - Metrics Layer

**Status:** Source-backed native public contract. **Version:** 0.1.0.

## 1. Scope

The `metrics` facet is Loom's native telemetry-metrics contract. It uses OpenTelemetry-aligned
concepts, while Loom owns canonical CBOR, storage, authorization, retention, stable errors, and
conformance. Prometheus, OTLP, Grafana, Influx, hosted listeners, and compatibility facades are not
part of this facet.

## 2. Canonical Records

`MetricDescriptor` is the identity record for a metric family. Its source-backed canonical CBOR form
is an array whose first item is `loom.metrics.descriptor.v1`, followed by name, description, unit,
instrument kind, temporality, attribute schema, per-attribute value-length limits, maximum active
series, stale-after duration, raw retention, distribution policy, and an ordered list of named rollup
profiles. A descriptor cannot be inferred from a sample or an external protocol request.

`MetricObservation` is an immutable measurement. Its source-backed canonical CBOR form starts with
`loom.metrics.observation.v1` and includes descriptor digest, normalized resource attributes, scope
attributes, metric attributes, start timestamp when applicable, timestamp, typed value, and optional
flags and exemplars. Attributes are sorted text pairs. Flags are sorted, unique text labels. The
encoded record rejects duplicate keys, unknown keys, duplicate or unsorted flags, non-finite numeric
values, invalid timestamps, and values outside the descriptor policy.

Instrument kinds are `counter`, `up_down_counter`, `gauge`, `histogram`,
`exponential_histogram`, and `summary`. Counters are monotonic. Temporality is `delta` or
`cumulative`; gauges use `instant`. Histograms retain their descriptor bucket layout. Exemplars are
optional bounded references and never become trace source of truth.

## 3. Source-Backed Policy And Query

Each descriptor declares permitted attributes, maximum values per attribute, maximum active series,
stale-after duration, raw retention, distribution policy, and ordered rollup profiles. Writes over a
cardinality limit fail with a stable limit error. Query results distinguish empty, stale, and partial
data. Queries bound time range, scanned series count, returned group count, returned samples, and
output bytes.

### 3.1 Target Enterprise Rollup Contract

Native rollup materialization is source-backed. A descriptor carries an ordered list of named rollup
profiles, not one global or single-resolution rollup policy. Each profile defines a unique resolution,
derived retention, bounded lateness allowance, aggregation semantics, and deterministic profile
identity. This supports independent short-resolution and long-resolution retention tiers for the same
raw metric series without changing descriptor identity.

Rollup windows will be fixed, epoch-aligned, and use OpenTelemetry-aligned `(window_start, window_end]`
membership. A descriptor-defined bounded lateness allowance keeps a derived window mutable until its
watermark passes. An observation that arrives within the allowance updates the applicable derived window
idempotently. A later observation remains durable raw data but marks the affected derived range stale
until a deterministic rebuild reconciles it. Unlimited historical derived rewrites and rejection of late
raw observations are not Loom policies.

The rollup profile identity, source descriptor digest, source series identity, epoch-aligned window
boundary, and rollup schema version determine derived record identity. Materialized rollup records are
derived state keyed by that identity. They carry the profile name, aggregation, partial/final/stale
window status, sample count, aggregate value, source timestamp range, and watermark. Raw observations
remain source of truth. Rebuild over an explicit range recomputes affected derived windows from raw
observations. Compaction removes derived windows whose tier-specific retention has expired. Query
planning may select derived data only when its aggregation, resolution, staleness, and authorization
semantics are equivalent to the request.

Native query-tier planning is conservative. The planner may select a materialized rollup only for
aligned `(window_start, window_end]` requests whose aggregation, resolution, temporal semantics,
freshness, derived retention, authorization checks, and output bounds match the requested query. If
any equivalence proof is missing, including a missing, partial, stale, unretained, or unauthorized
derived window, the planner selects raw observations. Raw observations remain the source of truth.

OpenTelemetry alignment guides metric point and aggregation semantics, including temporal
reaggregation and multiple independently configured Views. Loom owns durable profile identity,
watermarks, late-arrival handling, compaction, retention, and query-tier selection.

Before persistence, implementations redact attribute and exemplar values whose keys represent
credentials, tokens, cookies, sessions, raw paths, URLs, URIs, payloads, bodies, free-form errors,
messages, or stacks. Authorization is enforced on descriptor and observation collection operations
using the `metrics` facet scope.

## 4. Source-Backed Interfaces

The `uldren-loom-metrics` crate owns canonical descriptor and observation encoding. `loom-core`
persists descriptor records under the metrics facet, stores observations by descriptor digest,
series identifier, and timestamp, validates observations against the descriptor, and enforces the
source-backed cardinality, redaction, retention, staleness, and bounded-query policy.

`uldren-loom-metrics` also owns source-backed rollup profile identity, legal aggregation validation,
epoch-aligned `(window_start, window_end]` membership, bounded lateness representation, derived rollup
identity, materialized rollup record encoding, and partial/final/stale status encoding. `loom-core`
materializes rollups from raw observations, marks beyond-watermark late windows stale, rebuilds
bounded ranges, compacts expired derived windows, and plans native query-tier selection when raw and
derived semantics are equivalent.

The C ABI, IDL, local client, remote Loom protocol/client dispatch, CLI, MCP, and supported language
bindings expose canonical CBOR operations for descriptor put/get, observation put, and bounded query.
The current query result is canonical CBOR containing observation records, a partial flag, and a stale
flag. Public surfaces preserve the native `metrics` vocabulary.

## 5. Conformance

Vectors pin descriptor, observation, rollup profile, derived rollup identity, and materialized rollup
record bytes. Behavioral conformance covers duplicate materialization, within-allowance updates,
beyond-watermark stale marking, rebuild convergence, retention compaction, stale visibility, query
partiality, query-tier rollup selection, forced raw fallback, and exemplar bounds. Negative vectors
cover invalid schema tag, duplicate attributes, unknown attributes, cardinality overflow, invalid
counter values, invalid histogram buckets, invalid enum values, invalid descriptor policy, duplicate
rollup profile identity, invalid rollup aggregation, invalid timestamps, and invalid typed values.

## 6. Target Work

Target work includes monotonicity flags beyond the current counter invariant, automatic rollup
materialization, typed result-view helpers beyond the raw-CBOR public boundary, hosted compatibility
projection, and compatibility facades.

## 7. Non-goals

This specification does not define OTLP serving, Prometheus, Grafana, Influx, hosted compatibility
routes, automatic retention compaction, or a full metrics query language.
