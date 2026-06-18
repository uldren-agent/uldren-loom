# ADR-0010: Loom Canonical CBOR v1 - the identity and ABI codec

## Status

Accepted and implemented. `crates/loom-codec` (P1); object identity (P2); all twelve facet encoders
plus the schema/table Blob forms, the kv/columnar/vector/prolly node codecs, and the `Bundle`/`Registry`
serializations (P3); the C ABI / SQL result payloads as canonical CBOR with JSON debug-only (P4); the
async poll/handle ABI reusing the result-buffer ownership rule (P5); this spec sweep (P6).

## Context

Loom is content-addressed: an object's address is `BLAKE3-256` over its canonical bytes. A single codec
must therefore serve two roles - the canonical object form (identity) and the ABI / wire result payload
- and it must be deterministic (one byte form per logical value) or addresses are not stable.

The pre-existing object codec was a bespoke length-prefixed format (`[type:1][len:uvarint][body]`), and
ABI results crossed as interim JSON. The project is greenfield (no stable release, no customers), so the
conformance vectors and draft specs are movable now and expensive to change later. Per AGENTS.md
"Architecture decision mode", current code is evidence, not precedent.

## Decision

Adopt **Loom Canonical CBOR v1** - a strict profile of CBOR (RFC 8949) - as the one codec for both
object identity and ABI result payloads. Standard CBOR on the wire; a strict canonical profile so each
logical value has exactly one byte form and every other form is rejected on decode.

Owned implementation in `crates/loom-codec` (identity-affecting infrastructure, not a `loom-core`
helper): the value model, canonical encoder, strict decoder, object framing, pinned vectors, negative
decode tests, and the fuzz target live there; cross-language vectors land with the first non-Rust
consumer.

### Profile rules (normative)

1. Definite lengths only; indefinite-length items are rejected.
2. Shortest-form integer/length arguments; non-minimal encodings are rejected.
3. Map keys in ascending order by canonical encoded-key bytes; duplicates rejected.
4. Floats are 64-bit only (no f16/f32). NaN and infinities are rejected; `-0.0` encodes as `+0.0` and a
   `-0.0` bit pattern is rejected on decode as an alternate of `+0.0`.
5. No CBOR tags (major type 6). The only simple values are `false` (0xf4), `true` (0xf5), `null` (0xf6).
6. A single top-level item; trailing bytes are rejected.
7. Bounded nesting depth (`MAX_DEPTH`); deeper input is rejected rather than overflowing the decoder.

### Object framing

A Loom object is the canonical array `[epoch, type, ...fields]` where `epoch` is the schema epoch (v1 =
1) and `type` is the object's type code. Positional arrays keep current fixed-shape objects compact and
avoid map-key sorting on the identity hot path. Integer-key maps are used only where a struct is
genuinely sparse or must evolve with optional fields. Adding an identity field is an epoch bump, not an
implicit optional-field trick.

### Facet, value, and ABI framing (normative)

Object identity uses the `[epoch, type, ...fields]` frame above. Everything else that is
content-addressed or crosses the ABI uses bare canonical CBOR under the same profile:

- **Facets** store bare canonical CBOR values (arrays/maps), not object frames: log, ledger, document,
  time-series, graph, kv, columnar, vector, tabular (rows + the whole-table form), and the prolly node
  codec (leaf/internal nodes as `[tag, [entries...]]`). The `Schema` Blob of a `TABLE`-entry Tree and
  the `Bundle` (sync) and `Registry` (namespace) serializations are likewise canonical CBOR; no
  bespoke length-prefixed framing remains in any content-addressed or ABI byte form.
- **The shared SQL cell codec.** A `tabular::Value` is one positional `[tag, payload...]` array (the
  leading `Uint` tag is the value discriminant), shared by kv/columnar/vector metadata, table rows,
  **and ABI result payloads** - `loom_core::tabular::cell_value` / `cell_from` is the single, stable,
  type-faithful value-on-the-wire codec for the whole engine (one implementation, no second value
  framing). Floats (`Float`, `F32`, `Point`) carry their raw IEEE-754 bits in a CBOR `Uint`, **not** a
  CBOR float, so NaN payloads, infinities, and `-0.0` round-trip bit-exact without tripping profile
  rule 4; 128-bit integers carry as little-endian byte strings (no `i64`/`u64` range limit); decode
  range-checks `F32` bits back to `u32`. This is the one place value bits intentionally bypass the
  float rule, and it is sound because the bits are carried as integers.
- **The one non-CBOR codec that remains.** The order-preserving prolly/secondary-index key encoding
  (`encode_pk_values`) stays a bespoke binary format: comparing two encoded keys as raw bytes must
  reproduce the `Value` ordering, and canonical CBOR is not designed to preserve Loom's
  byte-lexicographic key order. It encodes keys only (never decoded back to values), so it is not an
  identity-ambiguity surface.
- **ABI result payloads** are built directly as canonical CBOR by `loom-sql` `result_cbor`: an explicit
  result envelope whose every scalar rides through the shared cell codec above, so the payload is
  type-faithful end to end (no serde_json route). JSON is never the wire form; `result_to_json` renders
  a buffer for debugging only.

### Hash

BLAKE3-256 stays the v1 digest. Conformance is split into **canonical-byte vectors** (hash-independent)
and **digest vectors** (hash-specific), so a future NIST hash (SHA-256 / SHA3-256) is a digest-layer
repin, not a codec ambiguity. A digest wider than 256 bits would additionally require multihash digest
fields; out of scope for v1.

## Alternatives rejected

- **Keep the bespoke length-prefixed codec.** Cheapest patch, but a non-standard format with no
  cross-language tooling; greenfield is the moment to move to a durable standard (AGENTS.md "prefer
  durable standards ... over bespoke").
- **Protocol Buffers as the identity form.** Serialized bytes are explicitly not guaranteed stable
  across versions/languages; unfit as a digest input. Protobuf remains the gRPC transport (0008), not
  identity.
- **Cap'n Proto.** Has a canonical form but encoders are not required to emit it; still needs a
  canonicalization discipline, with no advantage over an owned strict CBOR profile.
- **`ciborium` for decode.** Measured in a spike: it normalizes non-canonical input - indefinite
  lengths, non-minimal ints, duplicate and unsorted map keys, and trailing bytes all decode to `Ok`,
  hiding the wire form - so it cannot enforce this profile. Encode is canonical, but owning the encoder
  too keeps the bytes that define content addresses free of a dependency whose output could drift across
  versions.

## Proof bar

The codec is not "settled" without: pinned canonical-byte vectors; negative-decode tests for every
alternate form (rules 1-7); round-trip and byte-stability tests; and a no-panic fuzz target over
arbitrary input. Cross-language vectors land with the first non-Rust binding that consumes the codec.
All of the above except cross-language vectors are implemented in P1.

## Consequences

- Every digest changes. `crates/loom-conformance` vectors, the `0007 §3` codec text, `0008`,
  `idl/loom.idl`, and the FFI result path are re-pinned / migrated across P2-P6.
- One new workspace crate (`uldren-loom-codec`, BUSL-1.1 via `deny.toml` exception).
- ABI result payloads become canonical bytes; JSON is retained only as a debug/admin format.
