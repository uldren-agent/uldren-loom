# 0033 - Search Layer

**Status:** Partial, current search substrate, public facade, and bounded native FTS engine
source-backed. **Version:** 0.1.0.
**Capability:** `search`, optional when implemented.

**Depends on:** 0002 (object model and prolly trees), 0003 (interface and errors), 0011
(tabular text source), 0014 (workspace buckets and facets), 0020 (document text source), 0032
(platform parity). **Relates to:** 0015 (program-facing access), 0008 (wire projection), 0007
(bindings).

This spec defines the full-text search capability for Loom. Current source implements a portable,
reduced search facade: versioned document collections, explicit field mappings, local linear-scan
query, C ABI and C header projection, all eight local binding projections, local MCP data-tool
projection, hosted REST and JSON-RPC native projection for create/index/get/query/delete/ids/remap,
daemon-opened hosted listeners, and executable facade conformance. The optional native full-text engine is
source-backed for the current BM25 match plus deterministic parity subset in `crates/loom-tantivy`.
The committed documents and mapping are Loom state; any native inverted index is a derived cache and is
not part of commit identity or sync payloads.

## Current implementation

Current source implements a promoted portable search layer.

- `crates/loom-core/src/search.rs` defines `SearchCollection`, `Mapping`, `FieldMapping`,
  `FieldType`, `Document`, `FieldValue`, `Query`, `QueryRequest`, `QueryResponse`, and `SearchHit`.
- `FacetKind::Search` is source-backed in workspace storage.
- The public facade exposes `search_create`, `search_index`, `search_get`, `search_delete`,
  `search_ids`, `search_remap`, and `search_query`.
- Collection storage uses a structured source root. The root records the schema, digest profile,
  mapping, and sorted document component references; canonical document source records live under the
  reserved `.documents` search-facet subtree and derived native engine artifacts remain rebuildable
  outside the committed collection root.
- The IDL defines `interface Search`; the C ABI exposes `loom_search_*`; the C header is in sync.
- CLI, Node, Python, C++, Swift, JVM, Android, React Native, and WASM project the local facade.
- The portable query path is deterministic and reduced. It supports `match_all`, `match`, `term`,
  `phrase`, `range`, `prefix`, `wildcard`, `fuzzy`, `similar`, and `bool`, orders hits by score
  descending then id ascending, returns reduced highlights for requested stored text fields, returns
  matched-set facet buckets for faceted fields, returns portable `terms` and `value_count`
  aggregations, and sets `reduced = true`.
- Search mappings carry analyzer metadata slots for index analyzer, search analyzer, and normalizer
  names. The portable query path preserves this metadata in canonical state and request decoding; it
  does not claim full analyzer execution parity with native engines.
- `NO_SUCH_FIELD`, `QUERY_PARSE_ERROR`, and `INDEX_NOT_READY` are stable error variants. The
  `INDEX_NOT_READY` variant is source-backed for the derived-index lifecycle; current hosted source
  does not yet route ordinary hosted queries through a native index.
- `SearchEngine` and `search_query_auto` define the wasm-clean native-engine seam in `loom-core`.
- `loom-store` exposes the durable-local derived-artifact store plus search-specific Tantivy artifact
  key, stamp, status, rebuild, finish, fail, and unsupported helpers for source-digest-stamped native
  indexes.
- `loom fts status` reports the source digest and derived Tantivy artifact status for an explicit
  engine version.
- The default CLI build links the optional `crates/loom-tantivy` native engine through the
  `native-fts` feature and `loom fts rebuild` performs a synchronous local rebuild of the derived
  Tantivy artifact. Builds without `native-fts` still compile and record unsupported status for
  rebuild attempts when an explicit engine version is provided.
- The daemon local control protocol exposes `fts-status` and `fts-rebuild` commands. The daemon binds
  the authenticated operator into the Loom engine for ACL/PEP checks, starts source-digest-stamped
  rebuild records through the shared derived-artifact lifecycle, coalesces concurrent rebuild requests,
  returns rebuilding/readiness status, and completes native Tantivy payload generation asynchronously
  when `native-fts` is linked. Builds without `native-fts` record unsupported status.
- `crates/loom-tantivy` implements the `SearchEngine` trait over Tantivy 0.26.1 for in-memory BM25
  `match` queries plus deterministic term, phrase, range, bool, match-all, prefix, wildcard, fuzzy,
  and similar parity-subset queries. It builds reloadable derived Tantivy index payload bytes for
  durable-local storage, returns `reduced = false`, maps unknown fields to `NO_SUCH_FIELD`, decorates
  source-backed native hits with portable facets and highlights when the source collection is
  available, and keeps payload-only deterministic queries unsupported until the payload format carries
  the source-side fields required by those rows.
- Hosted REST and JSON-RPC expose source-backed protocol conformance for create, index, get, delete,
  ids, remap, and query. Hosted REST also
  exposes the first OpenSearch-compatible subset over `fts`: root info, cluster health, index create/delete,
  index metadata, document index/get/delete, match/term/match_phrase/range/bool search, `_count`,
  `_bulk` index/create/delete/update with independent item errors for known item failures, `_msearch`,
  `match_all`, terms/missing/range/histogram/value_count/avg/sum/min/max/stats/cardinality aggregations, explicit unsupported
  errors for custom analyzers, normalizers, and `_analyze`, refresh as an immediate-visibility no-op with
  `refresh=true` and `refresh=wait_for` accepted on write routes, versioned alias metadata, alias
  read/write-target resolution, comma-separated multi-index search, wildcard expansion, a
  machine-readable capability report, a read-only security-account shim,
  unsupported responses for other security reads, and forbidden unsupported responses for security
  mutations. Daemon-opened `fts/ndjson` uses this REST-compatible route set for NDJSON bulk ingestion.
  `HOSTED_PROTOCOL_FEATURES` reports the native FTS hosted profiles, the bounded
  OpenSearch-compatible profile, supported aggregation rows, supported `match_all` and analyzer-boundary
  rows, supported alias/multi-index/wildcard rows, supported security shims, unsupported security mutations,
  route-matrix certification, official Rust client transcript evidence, and target OpenSearch rows that remain
  unimplemented.
- Hosted query routes still use the portable source path unless a caller explicitly rebuilds and
  queries through native local Tantivy surfaces. Nested and pipeline aggregation coverage, full
  analyzer execution controls, and broader native payload query parity remain target work.

## Goals and non-goals

Goals:

- Version text-bearing documents by id, with content-addressed identity.
- Index selected fields for BM25-ranked full-text retrieval.
- Support structured query leaves for match, term, phrase, and range predicates, composed with
  boolean operators.
- Return deterministic result ordering: score descending, then document id ascending.
- Support facet counts, highlighting, and simple aggregations once the native engine exists.
- Coexist with other workspace facets under the 0014 bucket model.

Non-goals:

- No distributed search cluster, sharding, replicas, or cluster coordination.
- No cluster-wide OpenSearch or Elasticsearch administration model. The OpenSearch-compatible served
  profile is a hosted presentation over Loom full-text collections; cluster coordination, sharding,
  replicas, snapshots, and plugin administration are outside this spec.
- No use of the inverted index as the system of record.
- No relational joins or SQL planning. Those remain 0011 responsibilities.

## Target data model

A search collection is a facet inside one workspace bucket. It is not a separate "search workspace"
type under the stale typed-workspace model. Current source resolves the earlier open question by making
`search` a first-class `FacetKind::Search`.

| Role | Encoding | Notes |
| --- | --- | --- |
| Collection root | Canonical CBOR `loom.search.structured-collection-root.v1` record | Versioned source root that pins digest profile, mapping, and sorted document component references. |
| Document source record | Canonical CBOR field map stored under reserved `.documents/<collection-key>/<digest>` paths | Source of truth for indexed text. The digest and length are checked from the root before decode. Large values may use 0002 chunking once chunked search values are promoted. |
| Document map | Root-owned id to document-component references | Versioned Loom state, diffable by id. Merge semantics depend on the conflict matrix. |
| Mapping | Explicit metadata for field type, analyzer, stored flag, faceted flag in the collection root | Versioned Loom state that syncs with the collection. |
| Aliases | Canonical CBOR alias metadata under reserved `.aliases` paths | Versioned source metadata. Alias targets validate against collection roots, and collection deletion prunes aliases deterministically. |
| Inverted index | Native full-text engine segments behind an internal trait | Derived cache, rebuildable from documents plus mapping, not content-addressed. |

Document ids are opaque byte strings with deterministic ordering. A field absent from the explicit
mapping is stored in the source document but is not indexed. A mapping change is an explicit `remap`
operation that rebuilds the local derived index from the committed documents.

## Versioning and sync

The collection root, document source records, mapping, and alias metadata are committed Loom source
state. They are the only search state that participates in branch, diff, merge, and sync.

The future inverted index is excluded from commit identity and sync. Segment boundaries, compression,
and merge timing are implementation artifacts, so a native full-text index is treated as a local cache.
Peers rebuild it from committed documents and mapping when their platform supports the full search
engine.

This spec does not claim row-level, key-level, or document-level merge behavior beyond the behavior
implemented by the relevant source substrate. Any automatic merge contract for document maps must be
resolved through `CONFLICT-RESOLUTION-MATRIX.md` before it is promoted as implemented.

## Target facade

Loom exposes a local `search` facade when the `search` capability is advertised. The concrete surface
is source-backed in `idl/loom.idl`, `include/loom.h`, the C ABI, and local bindings. Hosted REST,
JSON-RPC, and native gRPC expose create, index, get, delete, ids, remap, and query over the shared
hosted kernel. Generated gRPC schema artifacts and full protocol conformance remain target work.

Illustrative target shape:

```idl
interface Search {
  index(id: bytes, doc: bytes): Future<void>
  delete(id: bytes): Future<void>
  get(id: bytes): Future<Option<bytes>>
  ids(prefix: Option<bytes>): Stream<bytes>
  query(request: QueryRequest): Future<QueryResponse>
  remap(mapping: Mapping): Future<void>
}

struct QueryRequest {
  query: Query
  limit: u32
  offset: u32
  facets: list<string>
  highlight: list<string>
  aggregations: list<Aggregation>
}

enum Query {
  Match  { field: string, text: string }
  Term   { field: string, value: bytes }
  Phrase { field: string, terms: list<string>, slop: u32 }
  Range  { field: string, lower: optional<bytes>, upper: optional<bytes>, include_lower: bool, include_upper: bool }
  Bool   { must: list<Query>, should: list<Query>, must_not: list<Query> }
}
```

`NO_SUCH_FIELD` and `QUERY_PARSE_ERROR` are source-backed stable `Code` variants and are projected
unchanged through the local binding and C ABI surfaces.

## Platform contract

Native may use a full-text engine such as Tantivy behind an internal trait, subject to dependency
policy and conformance tests. The repository does not currently include that dependency or engine.

The web target is reduced by default. It may store and sync the committed search documents and
mapping, but it should not claim full BM25 indexing until an in-browser writer is implemented and
tested. The long-term web posture is:

- Native provides the full search capability when the full-text engine is present.
- Web may provide a deterministic linear fallback over committed documents.
- Web fallback must report reduced capability. It must not pretend to support BM25 ranking,
  highlighting, faceting, or aggregation.
- Shipping native-built index blobs to web peers is rejected as the default because it turns a
  derived cache into transferred state.

## Relationship to other facets

Document and tabular facets are natural text sources. This spec does not require cross-facet kept
views in v1. A first implementation can index documents written directly through the search facade.
Maintained indexes over `.loom/facets/document/...` or SQL rows need an explicit facet
interoperability contract before they are promoted.

Programs may eventually receive a bounded search subset through the compute access surface, but that
surface is not implemented today. Any program access must respect the 0015 capability model and the
principal/access-control work in 0026 through 0028.

## Resolved decisions

1. **Mapping model:** explicit mapping plus `remap`.

   The mapping is explicit versioned metadata. It is not inferred from ingestion order. This keeps
   rebuilds deterministic and avoids peers deriving different schemas from the same document set.
   `remap(mapping)` is the deliberate path for changing field types, analyzers, stored flags, or
   faceted flags.

2. **Web indexing posture:** reduced web capability.

   Web does not need a full in-browser writer for v1. A deterministic linear fallback is acceptable
   if it is advertised as reduced capability. A full web search engine can be added later without
   changing the query API.

3. **Spec split:** 0033 remains the parent target contract.

   Child specs should be created when implementation begins if the work needs separate review slices.
   Likely slices are search document storage and mapping, native full-text engine integration, web
   fallback and capability reporting, and binding/wire projection.

4. **CLI and served naming:** `fts` is the canonical full-text collection/index surface.

   The current local collection/index surface creates collections, indexes documents, manages ids,
   remaps fields, and queries within an explicit search collection. That surface is projected as
   `loom fts ...` and `loom serve configure <store> fts <workspace> ...`. The top-level `loom search`
   command is a store-wide discovery command over readable full-text collections; it is not an alias
   for `loom fts`. Internal `search` names remain the capability and source API names; they are not
   served listener aliases.

5. **Lucene posture:** Lucene is not a hosted compatibility target.

   OpenSearch remains the primary served compatibility target because it has a concrete HTTP client
   ecosystem. Lucene is an in-process engine and query syntax family, not a comparable hosted
   protocol. Loom may later accept a Lucene query-string dialect as user convenience, but not as a base
   facet or served profile.

## Native engine design (Tantivy, 244d - full target)

This section is the complete, self-contained design for the native full-text engine. `tantivy` is a
heavy, native-only dependency and must not enter the default wasm-clean workspace dependency path. The
portable pieces (the trait, the engine-version stamping, the conformance match-set vectors) are
implemented in workspace crates; the optional `loom-tantivy` engine is built by manifest path on a
native toolchain until a deliberate binary-linking decision promotes it.

### N.1 Trait seam and crate (resolved, Q1=A)

A `SearchEngine` trait is defined in `loom-core` (wasm-clean, no `tantivy`); the heavy implementation
lives in a standalone `loom-tantivy` crate depending on `loom-core` + `tantivy`, injected at the call
site. The default workspace build never links `tantivy`. A `search_query_auto(loom, ns, name, request,
engine: Option<&dyn SearchEngine>)` runs the portable path when `engine` is `None` and the native
engine otherwise.

### N.2 Semantics contract (resolved): native-authoritative, portable-reduced

Full-text ranking and analysis deliberately diverge from the portable path, so the "switch is invisible
except speed" guarantee that vector/columnar hold does **not** apply here. Instead:

- The **native engine is the authoritative search semantics** - real analyzers, BM25 ranking, and the
  full feature surface (N.5).
- The **portable / wasm path is an explicitly degraded approximation**: a linear scan with the simple
  built-in tokenizer (lowercase alphanumeric, no stemming/stop-words), unscored, always marked
  `reduced = true`. It does **not** promise the same matches as the native engine for analyzed text.
- **Conformance** pins exact cross-engine parity only for the *deterministic subset*: `term`, `range`,
  and `bool` queries plus `match`/`phrase` over **un-analyzed** (`keyword`) fields, where membership is
  well-defined regardless of engine. Analyzed-text matching is native-authoritative; the portable path
  is best-effort recall there and is not held to parity.

This is the long-term contract precisely because Q3 enables full analyzers (N.4): once stemming is on,
"running" matches "run" natively but not portably, so a single match-set parity rule would be a lie. The
`reduced` flag is the wire signal that a result came from the degraded path.

### N.3 Schema mapping (resolved, Q7=C - richer typed schema)

The `SearchCollection` mapping (`field -> {field_type, stored, faceted}`) maps to an explicit Tantivy
schema with typed fields, not a flat text/keyword pair:

- `Text` -> Tantivy `TEXT` with the field's analyzer (N.4); `Keyword` -> `STRING` (exact).
- Numeric/date/bool shapes declarable on a field (extending `FieldType`) -> Tantivy typed fast fields,
  so `range` is an indexed numeric range, not a byte-lexical compare.
- `stored = true` -> `STORED`; `faceted = true` -> a `FAST` field (facet counts, N.5).
- The opaque document id (bytes) is a stored id field used to map Tantivy hits back to Loom ids.

`FieldType` gains the richer variants (numeric/date/bool) as an additive, versioned extension to the
mapping wire form; existing `text`/`keyword` tags are unchanged.

### N.4 Analyzers (resolved, Q3=C - full Tantivy functionality)

Analyzers are configured per `Text` field in the mapping: `simple` (parity with the portable
tokenizer), `standard`, language analyzers (stemming + stop-words), `ngram`, and `whitespace`. A field
using anything other than `simple` is native-only by definition (the portable path cannot reproduce
stemming) and is excluded from the parity subset (N.2). The analyzer id is stored in the field mapping
(versioned, synced) and stamped into the derived index (N.6) so an analyzer change forces a rebuild.

### N.5 Feature surface (resolved, Q6=C - full native surface)

The native engine exposes, beyond BM25-ranked `match`/`term`/`phrase`/`range`/`bool`:

- **Ranking:** BM25 scores, `reduced = false`.
- **Highlighting:** matched snippets per hit (opt-in per request; extends the response).
- **Faceting:** per-value counts over `faceted` fields (extends request + response).
- **Fuzzy / wildcard / prefix:** `Query` gains `Fuzzy{field, term, distance}`, `Wildcard{field,
  pattern}`, `Prefix{field, term}` variants (native-only; portable returns `UNSUPPORTED` for them).
- **More-like-this:** a `Similar{id}` query.

Each beyond-baseline feature extends the request/response AST and is projected across C ABI + IDL + 8
bindings. The portable path answers the deterministic subset and returns `UNSUPPORTED` (a stable error)
for native-only query variants and options, never a silent empty result.

### N.6 Index lifecycle and the embedded derived-artifact store (resolved, Q4 + Q9)

The Tantivy index is a **derived, rebuildable view** of the committed document map (the source of
truth), stored in the shared embedded derived-artifact store. Its full lifecycle - keys,
source-anchor stamp, states, rebuild coalescing, stale-before-trust, copy-carried/sync-rebuilt
carriage, and serve-read policy - follows the canonical contract in 0005 §8.2 (implemented by
`loom-store::derived`) and is not restated here. The vector ANN index (0017 §5) and the columnar
Arrow projection (0023 §12) bind to the same 0005 §8.2 contract.

Search-specific bindings (Q9): the artifact's `source_digest` is taken over the committed document
map, and its stamp additionally covers the `tantivy` + scoring version and the analyzer set, so an
engine upgrade or an analyzer change moves the stamp and forces a rebuild. Cross-engine conformance
remains match-set parity on the deterministic subset (N.2); native BM25 score vectors are pinned
*per stamped engine version* and regenerated when the stamp moves (the same rebuild). Tantivy-specific
local rebuild commands, daemon coalescing, readiness status projection, unsupported no-native status
recording, and native index payload generation are source-backed for the current payload format.

### N.7 Engine selection (resolved, Q5=A - always native when present)

When a `SearchEngine` is present, it is **always** used; the portable path is used only when no engine
is linked (wasm) or the index is not ready (N.8). Search does **not** threshold-switch on collection
size: ranking semantics must not silently change at a size boundary (the failure mode that the vector
threshold has - see the 0017 revisit). The only declared divergence is native (server) vs portable
(wasm), surfaced by `reduced`.

### N.8 Build policy: `INDEX_NOT_READY` + explicit rebuild (resolved, Q8)

The engine never silently performs an expensive rebuild on a read. A query against a missing or stale
index returns the stable status **`INDEX_NOT_READY`** (never a silent fallback that pretends, never a
surprise multi-minute build). A dedicated `search_rebuild_index(ns, collection)` command builds the
index into the embedded store (N.6) and reports status/progress:

- **Stateless invocation.** Loading the file, running the rebuild **synchronously**, persisting the
  index into the image, then unloading. A subsequent stateless run finds it warm; until then, queries
  return `INDEX_NOT_READY`.
- **Daemon.** The daemon **intercepts** the rebuild and runs it **asynchronously**, returning a rebuild
  status/handle to the (MCP) caller so progress is observable.
- **Concurrent rebuild policy (resolved).** Rebuild is **idempotent and coalescing**: a rebuild request
  for a collection that already has a rebuild in flight does **not** start a second - it attaches to the
  running rebuild and returns that rebuild's status/handle. A rebuild builds against a content-digest
  snapshot; if the committed documents change mid-rebuild, the finished index is stamped with the digest
  it built from, and the next query detecting a newer digest reports `INDEX_NOT_READY` (or, with an
  optional `force`, the daemon chains a fresh rebuild for the newer digest after the current completes).

### N.9 Implementation slices (for the separate native session)

- **tantivy-A:** define the `SearchEngine` trait + `search_query_auto` + the `INDEX_NOT_READY` /
  `UNSUPPORTED` status variants in `loom-core` (portable, sandbox-verifiable), with the engine `None`.
- **tantivy-B:** the embedded derived-artifact store (identity-excluded region in the loom container,
  version + digest stamping, rebuild-on-mismatch); shared with 0017/0023.
- **tantivy-C (native):** `loom-tantivy` crate - source-backed for a first in-memory Tantivy
  `SearchEngine` over mapped text and keyword fields, BM25-ranked `match` queries, deterministic
  hit ordering after Tantivy scoring, `NO_SUCH_FIELD`, and `UNSUPPORTED` for unimplemented query
  shapes. Remaining target work is analyzer profile control, full query-family support, build/persist
  into the embedded store, the rebuild command, and the concurrent-rebuild policy (N.8).

### N.10 OpenSearch-compatible served profile

The OpenSearch-compatible target is a served presentation over the Loom `search` capability, not a
separate storage facet. The served syntax uses the `fts` surface and generic transport labels:

```text
loom serve configure <store> fts <workspace> --transport rest --bind 127.0.0.1:9200
loom serve configure <store> fts <workspace> --transport ndjson --bind 127.0.0.1:9200
```

OpenSearch is the primary target. Elasticsearch compatibility is tracked row by row in the compatibility
matrix owned by P9-0013. Each OpenSearch index maps to one Loom full-text collection in the served
workspace. Aliases are versioned search-facet metadata. Multi-index and wildcard search expand across
collections and aliases inside that workspace only. Writes through an alias require a single write
target selected with `is_write_index` when an alias references more than one collection.

The target matrix covers full Query DSL, full aggregation compatibility, full analyzer compatibility,
bulk ingestion, multi-search, aliases, multi-index search, OpenSearch-style refresh, and read-only
security shims. Unsupported features return stable compatibility errors and appear in capability
reporting rather than silently degrading.

OpenSearch-style refresh is the target for the served compatibility profile. Default writes update Loom
source state and become searchable after automatic refresh. `refresh=wait_for` waits for visibility when
listener policy allows it. `refresh=true` forces refresh when listener policy allows it. The derived
Tantivy index remains identity-excluded source-derived state.
- **tantivy-D:** the full native feature surface (N.5: highlighting, faceting, fuzzy/wildcard/prefix,
  more-like-this) - request/response AST extension + C ABI + IDL + 8 bindings; portable returns
  `UNSUPPORTED` for native-only features.
- **tantivy-E:** conformance - shared match-set vectors on the deterministic subset run against both
  engines; per-version native BM25 score vectors; `INDEX_NOT_READY`/rebuild behavior; spec flip.

## Unfinished Work

| Order | Parent | Work item | Status | Exit criteria |
| --- | --- | --- | --- | --- |
| T1 | Current implementation | Spec/source reconciliation | Complete local | Current implementation, target facade, and unfinished-work text describe the implemented portable public search facade instead of stale target-only language. |
| T2 | Current implementation | FTS CLI projection plus store-wide search | Complete local | `loom fts ...` commands expose create, index, get, delete, ids, remap, and query with canonical CBOR forms, including file-backed opaque document ids. `loom search` is source-backed as the store-wide discovery command over readable full-text collections with optional workspace, collection, and field filters. |
| T3 | Current implementation | MCP FTS projection plus store-wide search | Complete local | MCP `fts_*` tools expose create, index, get, delete, ids, remap, and query with registered schemas and canonical CBOR payload tests. MCP `search` is source-backed as the store-wide discovery tool over readable full-text collections with optional workspace, collection, and field filters. |
| T4 | Current implementation | Non-MCP hosted search wire projection | Partial source-backed | REST and JSON-RPC served protocol conformance proves create, index, get, delete, ids, remap, and query with hosted auth/PEP over the current data route. Native gRPC served protocol methods expose create, index, get, delete, ids, remap, and query with daemon-opened listener tests. OpenSearch-compatible REST/NDJSON has its first bounded subset. Remaining work: generated gRPC artifacts and broader protocol conformance. |
| T5 | N.1 | Native search engine trait and `search_query_auto` | Complete local | `loom-core` defines the wasm-clean engine seam and auto path without linking Tantivy. |
| T6 | N.6 | Embedded derived-artifact store | Partial source-backed | Shared cache storage supports identity-excluded, copy-carried, version-stamped artifacts for search, vector, and columnar accelerators. Search-specific Tantivy artifact key/stamp/status/rebuild helpers, `loom fts status`, synchronous local `loom fts rebuild`, daemon `fts-status`/`fts-rebuild` async scheduling and coalescing, no-native unsupported status recording, and `INDEX_NOT_READY` are source-backed. |
| T7 | N.3-N.8 | Native Tantivy engine | Partial source-backed | `crates/loom-tantivy` is an optional native crate linked by the default CLI through `native-fts`. It implements `SearchEngine` with Tantivy 0.26.1 for in-memory BM25 `match` queries, deterministic term/phrase/range/bool/match-all/prefix/wildcard/fuzzy/similar parity-subset queries, source-backed facets and highlights when the source collection is available, and reloadable derived index payload bytes. Payload-only deterministic queries remain unsupported until the payload format carries source-side fields. |
| T8 | N.5 | Native-only feature surface | Partial source-backed | Request/response ASTs include analyzer metadata, highlight fields, facet requests, aggregation requests, and match-all/prefix/wildcard/fuzzy/similar query variants. Portable execution is deterministic and reduced; full analyzer execution controls, nested aggregations, and pipeline aggregations remain target. |
| T9 | N.2 | Search conformance expansion | Partial source-backed | Core tests cover extended query shapes, facets, highlights, aggregations, analyzer-bearing CBOR, the search-specific derived Tantivy lifecycle test pins missing, rebuilding, ready, stale, failed, and unsupported status transitions with source-digest and engine-version stamps, hosted native REST/JSON-RPC operation conformance proves create, index, get, hit query, no-hit query, ids, remap, and delete, and native Tantivy score tests prove BM25 term-frequency ranking, deterministic id tie-breaks, empty collections, and no-hit collections for the current engine seam. Exact numeric BM25 pins for document-length normalization, field weighting if a weighted field model is promoted, source-map merge vectors, future root-version migration vectors, and mixed-version binding behavior remain target. |
| T10 | N.10 | OpenSearch-compatible served profile | Partial source-backed | `fts` served with `rest` and `ndjson` transports maps OpenSearch indexes to Loom full-text collections for the current subset: index create/get/delete, document index/get/delete, match/match_all/term/terms/match_phrase/range/bool/prefix/wildcard/fuzzy/multi_match/query_string/simple_query_string search, `_count`, `_bulk` index/create/delete/update with independent item errors for known item failures, `_msearch`, terms/missing/range/histogram/date_histogram/value_count/avg/sum/min/max/stats/extended_stats/percentiles/cardinality/top_hits aggregations, alias metadata, multi-index search, wildcard expansion, immediate refresh no-op with wait-for accepted on write routes, capability reporting, read-only security account, bounded simple `_analyze`, unsupported custom analyzer and normalizer mapping settings, unsupported analyzer component composition, route-matrix certification, official OpenSearch Rust client transcript evidence, unsupported query and aggregation families with stable compatibility errors, unsupported security reads beyond account, forbidden unsupported security mutations, and hosted protocol report rows. Nested/object query execution, filters and pipeline aggregations, full analyzer controls, and broader certification beyond the official Rust client transcript remain target work. `search` is not a served-surface alias. |

### Storage Promotion Closure Audit

Promotion scope: the native `search`/FTS structured collection root and source document store.

| Closure gate | Status | Evidence | Remaining work classification |
| --- | --- | --- | --- |
| Canonical root contract | Source-backed | The structured collection root is `loom.search.structured-collection-root.v1`; it pins digest profile, mapping, sorted document component references, and alias metadata as committed search-facet source state. | Large search-value chunking and future root-version migration vectors are closure-blocking promotion debt for any later root-format promotion. |
| Source boundary | Source-backed | Committed documents, mapping, aliases, and collection roots are the only source state. Native inverted indexes and Tantivy payload bytes are derived caches excluded from commit identity and sync. | None for the current native storage scope. |
| Unit addressing | Source-backed | Document ids are opaque byte strings with deterministic ordering; source document records live under reserved `.documents/<collection-key>/<digest>` paths and are referenced from the root by digest and length. | Chunked large search values remain P1 facet primitive work until promoted. |
| Incremental mutation | Source-backed subset | `search_index`, `search_delete`, `search_ids`, and `search_remap` operate on id-keyed source documents and explicit mappings; derived indexes are rebuilt from source digest stamps. Structural VCS diffs now emit search document-id units and mapping metadata units for the current structured root. | Source-document merge semantics remain closure-blocking promotion debt before full storage-promotion closure. |
| Diff and merge semantics | Partial source-backed | Structural VCS diffs classify structured search roots by collection, ignore source component paths as derived from the root, and report added, removed, and changed source documents plus mapping-only changes. Divergent search collection roots stop VCS merge as explicit collection conflicts and do not merge derived Tantivy or OpenSearch artifacts as source truth. | Independent source-document auto-merge, same-document conflict classification, mapping-change conflict classification, alias-change conflict classification, and search document-map semantic merge vectors remain closure-blocking promotion debt. |
| Query and scan behavior | Source-backed subset | Portable source queries cover match_all, match, term, phrase, range, prefix, wildcard, fuzzy, similar, bool, reduced highlights, facets, and aggregations with deterministic ordering. Native Tantivy covers the parity subset when available. Native score tests prove BM25 term-frequency ranking, deterministic id tie-breaks for equal scores, empty collection behavior, and no-hit behavior. | Full analyzer controls, nested and pipeline aggregations, payload-only deterministic native queries, exact numeric BM25 document-length-normalization pins, and field weighting if a weighted field model is promoted are P1 facet primitive work. |
| Retention and garbage collection | Missing for full closure | Derived index payloads are rebuildable and can be discarded without source loss; source document retention and physical reclamation policy are not promoted here. | Source-document retention, tombstone policy, physical reclamation, and derived-payload prune policy are P1 facet primitive work. |
| Migration and compatibility | Source-backed subset | Mapping changes use explicit `remap`; derived artifacts are stamped by source digest, engine version, analyzer set, and format version. Unsupported engine absence records unsupported status. Current-root negative regression coverage rejects digest-profile mismatches. | Future root-version migration vectors and mixed-version behavior across bindings remain closure-blocking promotion debt when a second search root schema is introduced. |
| Conformance proof | Source-backed subset | Public facade, CLI, MCP, hosted REST/JSON-RPC operations, derived-artifact lifecycle states, query shapes, facets, highlights, aggregations, analyzer-bearing CBOR, route-matrix certification, official OpenSearch Rust client transcript evidence, structural diff vectors for source document added, removed, changed, and mapping-only changes, malformed structured-root tests for duplicate ids, digest profile mismatch, component length mismatch, and component digest mismatch, bounded native BM25 score-order vectors, and hosted native no-hit query vectors are source-backed for the current profile. | Exact numeric BM25 normalization vectors, field-weight vectors after a weighted field contract exists, source-map merge vectors, future root-version migration vectors, mixed-version binding behavior after a second root schema exists, and broader hosted conformance expansion are closure-blocking promotion debt for full closure. |
| Product-profile boundary | Recorded | OpenSearch-compatible REST/NDJSON is a served compatibility profile over Loom full-text collections; `search` is not a served-surface alias and product cluster administration is not native search storage. | Nested/object query execution, filters and pipeline aggregations, full analyzer controls, broader client certification, and OpenSearch cluster behavior are compatibility/profile work. |

### Pre-release root migration guardrail

The current native search root schema is `loom.search.structured-collection-root.v1`. A second
schema must not be introduced by silently adding another decoder branch. Before any `v2` root ships,
the change must include:

1. A current-store migration function that reads every supported `v1` collection through the public
   source path, writes the new root shape, and preserves document ids, mapping, aliases, source
   digest semantics, and derived-artifact invalidation.
2. Canonical positive vectors for `v1 -> v2` migration and canonical negative vectors for malformed
   `v2` roots, including duplicate ids, digest profile mismatch, component length mismatch, component
   digest mismatch, unsupported analyzer metadata, and unknown required fields.
3. Mixed-version behavior for every generated protocol and binding surface. Older binaries that see
   `loom.search.structured-collection-root.v2` must fail with the stable corrupt-object contract
   instead of decoding it as a legacy portable collection or returning partial data.
4. Hosted, MCP, CLI, C ABI, IDL, and binding conformance rows that prove current `v1` behavior still
   works and that unsupported future roots fail consistently until the migration is enabled.

Current source enforces the first guardrail by rejecting any root whose first field starts with
`loom.search.structured-collection-root.` but does not equal the current v1 schema before falling back
to the legacy portable collection decoder. This keeps pre-release schema drift visible and prevents a
future structured root from being accepted as a different byte shape by accident.

## Change log

### 0.1.0

The search facet's workspace-scoped public facade is now source-backed end to end, mirroring the KV and
CAS facets. Search is a first-class `FacetKind::Search` (P0 resolved): a versioned collection holds an
explicit field mapping plus an id-keyed document map through a structured collection root and canonical
document source records. Create, index, get, delete, id listing, remap, and
the portable linear-scan query cross the C ABI (`loom_search_*`), the IDL `Search` interface, the C
header, and the Node, Python, C++, iOS, JVM, Android, React Native, and WASM bindings. A mapping crosses
as canonical CBOR `field -> [type_tag, stored, faceted]`, a document as `field -> value` (text or
bytes), a query request as the recursive `[query, limit, offset]` form, and a response as
`[reduced, [[id, score_cell] ...]]`. The `NO_SUCH_FIELD` and `QUERY_PARSE_ERROR` error variants are
source-backed. An executable behavioral suite exercises the facade against the in-memory store, so the
`search` capability is reported executable. The native full-text engine (Tantivy) behind an internal
trait remains deferred behind a feature gate; the portable linear-scan fallback marks every response
`reduced` and is the source-backed query path today.
