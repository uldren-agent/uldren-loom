# 0047 - Logs Layer

**Status:** Source-backed native public contract. **Version:** 0.1.0.

## 1. Scope

The `logs` contract defines canonical native log records and native engine storage. It owns
deterministic record identity, validation, canonical CBOR encoding, and bounded retrieval for
timestamped event records with severity, body, typed attributes, telemetry resource and
instrumentation scope context, and trace correlation.

This contract does not define hosted listeners, OTLP, Grafana, Elasticsearch, or other compatibility
facades.

The `logs` facet has stable tag `19` and stores canonical records under:

```text
.loom/facets/logs/records/{record_id}
```

`record_id` is the lowercase BLAKE3 hex digest of the canonical log record bytes.

## 2. Canonical Record

`LogRecord` canonical CBOR is an array whose first item is `loom.logs.record.v1`, followed by event
timestamp in nanoseconds, observed timestamp when present, severity number, severity text, body,
attributes, resource attributes, instrumentation scope attributes, and optional trace context.

Log record identity is the BLAKE3 digest of the canonical bytes. Sequence assignment, index
materialization, and query cursors are deliberately outside this contract.

## 3. Validation

Timestamps are unsigned nanoseconds. The event timestamp is required and nonzero. Observed timestamp,
when present, is nonzero and not earlier than the event timestamp.

Severity numbers follow the OpenTelemetry numeric range `1..=24`. Severity text is required, bounded,
and preserved as supplied by the source.

Body and attributes use one typed value model: null, bool, signed integer, finite float, UTF-8 string,
bytes, array, and string-keyed map. Attribute maps are sorted by canonical key through `BTreeMap`.
Keys are nonempty, bounded, and reject control characters. Values reject non-finite floats, oversized
strings, oversized byte arrays, excessive attribute counts, and excessive nesting.

Trace context is optional. When present it contains a 16-byte nonzero trace identifier, 8-byte nonzero
span identifier, and trace flags in the supported range `0..=1`.

## 4. Source-Backed Interfaces

`uldren-loom-logs` owns the reusable canonical contract. It depends only on `loom-codec` and
`loom-types`. It provides construction, validation, encode, decode, and deterministic record identity.

`loom-core` persists log records under the logs facet, reads records by record id, and executes
bounded half-open timestamp queries. Query results are canonical CBOR `[records, partial]`, where
`records` is an array of canonical log record byte strings.

The C ABI, IDL, local client, remote Loom protocol/client dispatch, CLI, MCP, and supported language
bindings expose canonical CBOR operations for record put/get and bounded query. Public surfaces
preserve the native `logs` vocabulary.

`uldren-loom-conformance` pins positive canonical vectors, malformed decode vectors, native storage
round trip, and bounded query behavior.

## 5. Target Work

Target work includes retention policy, indexing, redaction policy, cursor semantics, lifecycle
operations, cross-signal correlation indexes, hosted compatibility projection, and typed result-view
helpers beyond the raw-CBOR public boundary.

## 6. Non-goals

This specification does not define OTLP serving, Grafana Explore behavior, syslog or JSON log
compatibility, hosted listeners, hosted compatibility routes, or product compatibility facades.
