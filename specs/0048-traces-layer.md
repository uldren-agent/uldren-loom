# 0048 Traces Layer

## Status

Source-backed native public contract. Version 0.1.0.

## Scope

The traces layer stores canonical span records under the `traces` facet. It provides a native API for
writing spans, reading one span by trace and span id, listing spans for a trace, and querying spans by
start-time window with caller supplied bounds.

The layer is not an OTLP endpoint, product compatibility facade, hosted listener, dashboard, or
vendor export contract.

## Facet

`FacetKind::Traces` has stable tag `20` and string form `traces`.

Native storage uses reserved facet paths:

```text
.loom/facets/traces/traces/{trace_id_hex}/{span_id_hex}
```

Both path ids are lowercase hex. Trace ids are 16 bytes and span ids are 8 bytes. The path is derived
from the canonical span context, so writing the same span identity replaces the same native record.

## Canonical span record

`loom.traces.span.v1` is encoded with Loom Canonical CBOR v1 as:

1. schema string
2. span context
3. optional parent span id
4. name
5. kind
6. start time in nanoseconds
7. end time in nanoseconds
8. optional observed time in nanoseconds
9. status code
10. status message
11. span attributes
12. resource attributes
13. scope attributes
14. events
15. links

The span context carries trace id, span id, and trace flags. Trace ids and span ids must be nonzero.
Trace flags currently accept only `0` or `1`.

Span kind values are `internal`, `server`, `client`, `producer`, and `consumer`.

Status code values are `unset`, `ok`, and `error`.

Attribute values support null, bool, signed integer, finite float, string, bytes, array, and map.
Maps use sorted canonical keys through Loom Canonical CBOR. Attribute keys must be non-empty,
bounded, and free of control characters.

## Relationships, events, and links

A span may name one parent span id in the same trace. The parent span id must not be zero and must not
equal the span id.

Events carry timestamp, name, and attributes. Event timestamps must be nonzero and event names must be
non-empty and bounded.

Links carry a full span context and attributes. Linked contexts follow the same nonzero id and trace
flag rules as the primary span context.

## Timing

`start_time_ns` must be nonzero. `end_time_ns` must be greater than or equal to `start_time_ns`.
`observed_time_ns`, when present, must be nonzero and greater than or equal to `start_time_ns`.

## Bounded retrieval

The native API enforces caller supplied read bounds:

- `traces_trace_spans` requires nonzero `max_spans` and `max_output_bytes`.
- `traces_query` requires `from_start_time_ns < to_start_time_ns`, nonzero `max_spans`, and nonzero
  `max_output_bytes`.
- Results are returned in deterministic start-time order, then trace id and span id where needed.
- If more matching spans exist than the bounds permit, the result sets `partial = true`.

The implementation applies bounds after deterministic ordering so repeated queries over unchanged
data return the same prefix.

Public C ABI, IDL, local client, remote Loom protocol/client dispatch, CLI, MCP, and supported
language binding surfaces expose canonical CBOR operations for span put/get, trace span listing, and
bounded query. Query results are canonical CBOR `[spans, partial]`, where `spans` is an array of
canonical span record byte strings. Public surfaces preserve the native `traces` vocabulary.

## Retention and redaction

This foundation records spans as explicit native records. It does not silently redact attributes,
events, links, resource attributes, or scope attributes at encode time. Callers that need redaction
must redact before writing, and the redacted bytes become the canonical identity.

Retention is represented by ordinary native storage lifecycle and namespace policy. This layer does
not attach hidden expiration metadata to spans, because hidden retention state would alter retrieval
without changing the span record identity. Hosted and product layers may implement deletion policies
above this contract by deleting span records from the traces facet.

## Conformance

Conformance pins:

- canonical span bytes
- stable span record id
- positive decode round trip
- negative decode vectors for invalid schema and invalid span identity relationships
- native storage round trip
- bounded trace and window queries

Portable vectors live under `crates/loom-conformance/test-vectors/traces/native-v1.json`.

## Target work

Target work includes retention policy, indexing, redaction policy, lifecycle operations,
cross-signal correlation indexes, hosted compatibility projection, and typed result-view helpers
beyond the raw-CBOR public boundary.

## Non-goals

This specification does not define OTLP serving, distributed-tracing vendor compatibility,
hosted listeners, hosted compatibility routes, dashboards, or product compatibility facades.
