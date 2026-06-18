# P9-0013 - `search` Capability And `fts` Served Binding

**Series:** P9 binding plan (normative-track sub-series; Draft)
**Version:** 0.1.0-draft - **Status:** Draft, source-backed local facade, FTS CLI and hosted native subset
**Last updated:** 2026-07-05
**Reads first:** [`P9-0002-projection-conventions.md`](./P9-0002-projection-conventions.md), facade spec
**0033** (Search; tantivy/BM25), **0032 section 4.6** (search absence and wasm posture), fidelity doc.

The local source now backs a reduced search facet in `loom-core::search`, IDL, bindings, CLI, and
MCP. The public operator spelling for the local collection/index CLI is `loom fts`; the canonical
served surface is `fts`. The top-level `loom search` command is source-backed store-wide discovery
over readable full-text collections, not an alias for `loom fts`; served `search` is intentionally
unmapped. Hosted REST and JSON-RPC expose the native
create/index/get/delete/ids/remap/query subset over daemon-opened `fts` listeners. Native hosted gRPC
exposes the same source-backed subset with typed mapping, document, and query messages. Local CLI
status and synchronous rebuild project the durable Tantivy derived-artifact lifecycle. Generated gRPC
schema artifacts and broader hosted conformance remain target work.

## 1. Facade surface (0033 section 4 `Search`)

`create(name, mapping)`, `index(name, id, doc)`, `delete(name, id)`, `get(name, id)`, `ids(name,
prefix?)`, `query(name, QueryRequest) -> QueryResponse`, and `remap(name, mapping)`. `QueryRequest`
currently carries `query`, `limit`, `offset`, requested facet fields, requested highlight fields, and
aggregation requests. `Query` is a boolean tree and leaf set of `MatchAll`, `Match`, `Term`, `Phrase`,
`Range`, `Prefix`, `Wildcard`, `Fuzzy`, `Similar`, and `Bool`.

The portable core is deterministic and reports reduced capability. BM25/Tantivy-backed ranking,
full analyzer execution, hosted nested or pipeline aggregations, and OpenSearch-compatible serving are
presentations or native engine work, not the base storage contract. Analyzer metadata is stored in
field mappings as declared names and forces source-digest changes; portable execution does not claim
native analyzer parity.

### 1.1 Binding Boundary

The base layer is the Loom search source collection: mapping, documents, ids, and portable reduced query.
Native projections expose create, index, get, delete, ids, remap, and query. OpenSearch and
Elasticsearch-compatible HTTP are presentations. JSONL document dumps and mapping exports are
interchange. Tantivy/Lucene-style inverted indexes, refresh state, analyzer caches, and aggregation
materializations are derived artifacts stamped against the source collection.

## 2. Tier-1 REST

Current source-backed hosted REST is the native action-shaped subset used by the served `fts`
listener: `/fts:create`, `/fts:index`, `/fts:get`, `/fts:delete`, `/fts:ids`, `/fts:remap`, and
`/fts:query`. The resource-shaped routes below remain the target REST profile.

Facet-root `/v1/workspaces/{workspace_id}/fts`:

| Facade method | HTTP |
| --- | --- |
| `create` | `PUT /fts/{name}` with mapping body |
| `index` | `PUT /fts/{name}/documents/{id}` (body = field map) |
| `delete` / `get` | `DELETE /fts/{name}/documents/{id}` and `GET /fts/{name}/documents/{id}` |
| `ids` | `GET /fts/{name}/documents?prefix={b64}&list=1` -> NDJSON ids |
| `query` | `POST /fts/{name}:query {QueryRequest}` -> `QueryResponse` |
| `remap` | `POST /fts/{name}:remap {mapping}` |

## 3-4. JSON-RPC / gRPC

Current hosted JSON-RPC is source-backed for `fts.create`, `fts.index`, `fts.get`, `fts.delete`,
`fts.ids`, `fts.remap`, and `fts.query`.

Current source has `loom.hosted.v1.Fts` over configured `fts/grpc` listeners. `Create`, `Index`,
`Get`, `Delete`, `Remap`, and `Query` are unary. `Ids` is server-streaming with bounded response
batches. The current handwritten service uses typed mapping, document, query, and hit messages over
the source-backed portable query model. Generated protobuf artifacts and broader hosted conformance
remain target work.

## 5. Tier-1 MCP

- **Read tools:** `search.query`, `search.get`, `search.ids`.
- **Write tools (token-gated):** `search.create`, `search.index`, `search.delete`, `search.remap`.

## 6. Tier-2 foreign adapter - search HTTP compatibility

The target presentation is an OpenSearch-compatible search surface, backed by Tantivy where available,
with Elasticsearch compatibility recorded route by route in a matrix. OpenSearch is the primary target
unless an owner decision switches a row to Elasticsearch-specific behavior.

Served syntax uses the Loom `fts` surface name and generic transport labels:

```text
loom serve configure <store> fts <workspace> --transport rest --bind 127.0.0.1:9200
loom serve configure <store> fts <workspace> --transport ndjson --bind 127.0.0.1:9200
```

An OpenSearch index maps to one Loom full-text collection inside the served workspace. There is no
cross-workspace index resolution in this profile. Multi-index and wildcard search expand across
collections and aliases in that workspace only.

### 6.0 Source anchors

The compatibility profile is grounded in these external and local anchors:

| Anchor | Evidence for this binding |
| --- | --- |
| OpenSearch Rust client docs, <https://docs.opensearch.org/latest/clients/rust/> | The official Rust client covers connection, index create/delete, document index/delete, bulk, and search examples. This is the first generated-client transcript target. |
| OpenSearch Rust crate docs, <https://docs.rs/opensearch/latest/opensearch/> | The current crate is Apache-2.0 and exposes the official client API surface used for compatibility tests. |
| OpenSearch Search API, <https://docs.opensearch.org/latest/api-reference/search-apis/search/> | Defines `GET` and `POST` `/{index}/_search` plus `/_search`. |
| OpenSearch Bulk API, <https://docs.opensearch.org/latest/api-reference/document-apis/bulk/> | Defines `_bulk`, NDJSON action/source lines, index-path and body-selected target indexes, independent item errors, and mixed index/create/update/delete actions. |
| OpenSearch Multi-Search API, <https://docs.opensearch.org/latest/api-reference/search-apis/multi-search/> | Defines `_msearch`, path-level default indexes, metadata/body NDJSON pairs, and independent sub-search execution. |
| OpenSearch Create Index API, <https://docs.opensearch.org/latest/api-reference/index-apis/create-index/> | Defines `PUT /{index}`, mappings, settings, aliases, index naming restrictions, and acknowledged responses. |
| Elasticsearch Rust client docs, <https://www.elastic.co/docs/reference/elasticsearch/clients/rust> | Confirms the comparable Rust ecosystem also uses fluent REST endpoint builders and the same `_search` style endpoint family. |
| Local source, `crates/loom-core/src/search.rs` | Current portable search state supports `Text` and `Keyword` field mappings with analyzer metadata, deterministic reduced `match_all`, `match`, `term`, `phrase`, `range`, `prefix`, `wildcard`, `fuzzy`, `similar`, and `bool` queries, reduced highlights, matched-set facets, and portable terms/value-count aggregations. |

### 6.1 Compatibility matrix

| Area | OpenSearch target | Elasticsearch comparison | Loom target behavior | Initial status |
| --- | --- | --- | --- | --- |
| Client grounding | Official OpenSearch Rust client over HTTP/JSON | Elasticsearch Rust client is a comparison transcript, not the primary target | OpenSearch Rust client transcript is the first compatibility proof; Elasticsearch transcript can be added where route/body behavior is intentionally compatible | Source-backed for the official OpenSearch Rust client against create, index, get, bulk, search with supported query clauses and aggregations, count, msearch, aliases, analyze, document delete, and index delete; hosted route-matrix certification covers additional supported and unsupported route boundaries |
| Served surface | `fts` with `--transport rest` for JSON routes and `--transport ndjson` for NDJSON-heavy compatibility | Same HTTP shape, different product identity expectations | `search` is the store-wide CLI discovery command, not a collection-local FTS command or served alias. OpenSearch product-shaped responses are compatibility output, not surface names | Source-backed for `fts/rest` and daemon-opened `fts/ndjson` bounded profiles |
| Root/product info | `GET /` returns product/version-like metadata clients can sanity-check | Elasticsearch clients may require different product headers or version markers | Return a Loom compatibility envelope that is sufficient for OpenSearch Rust client setup while capability-reporting native Loom identity | Source-backed bounded response |
| Index identity | Index | Index | Index maps to one Loom full-text collection inside the served workspace | Source-backed for one served collection |
| Index create/delete | `PUT /{index}` and `DELETE /{index}` | Similar route family | Create/delete collections. Shard/replica settings are accepted only as compatibility metadata or rejected with stable unsupported errors | Source-backed for create/delete, including alias pruning on delete |
| Index naming | OpenSearch index naming restrictions | Similar with version differences | Enforce compatibility restrictions at the served adapter boundary without changing internal Loom collection naming rules | Target |
| Mapping fields | Mappings and analysis settings | Similar with version drift | Current local mapping is `text` and `keyword`; numeric/date/boolean/object/nested/geo/vector rows require explicit capability states before acceptance | Target |
| Alias identity | Index alias, optional write index | Index alias, optional write index | Alias metadata is versioned inside the search facet. Writes through aliases require `is_write_index` when multiple targets exist | Source-backed for add/remove, read, and write-target resolution |
| Multi-index search | Comma list and wildcard expansion | Similar | Expand collections and aliases inside the served workspace only. No cross-workspace resolution | Source-backed for comma lists, aliases, `_all`, and simple `*` wildcards |
| Document APIs | `_doc`, index/create/get/delete/update | Similar | `_id` maps to Loom document id. Version, shard, seq-no, routing, refresh, and conflict fields are compatibility metadata unless source-backed | Source-backed for index/get/delete; update target |
| Bulk | `_bulk` NDJSON with mixed item actions and independent item errors | Similar | Parse action/source lines, route each item through hosted auth, PEP, stable errors, audit, request limits, and store save behavior | Source-backed for index/create/delete/update and independent item errors for known item failures; malformed NDJSON framing remains a request error |
| Multi-search | `_msearch` NDJSON metadata/body pairs | Similar | Resolve each sub-search independently inside the served workspace and return per-search errors without hiding partial failures | Source-backed for bound collection searches |
| Query DSL | Full Query DSL target | Compared route/body by route/body | Stage implementation by DSL row, but every unsupported query body must return stable compatibility errors and appear in capability reports | Source-backed for match_all, match, term, terms, match_phrase, range, bool, prefix, wildcard, fuzzy, multi_match, query_string, and simple_query_string; exists, ids, regexp, nested, function_score, constant_score, dis_max, and boosting return stable unsupported errors |
| Aggregations | Full aggregation target | Compared aggregation by aggregation | Stage terms, range, histogram, stats, cardinality, and nested rows separately; approximate or engine-specific behavior must be labeled | Source-backed for terms, missing, numeric range, histogram, date_histogram with fixed_interval, value_count, avg, sum, min, max, stats, extended_stats, percentiles, exact cardinality, top_hits, and recursive sub-aggregations; filters, calendar_interval date histograms, and pipeline aggregations remain stable unsupported |
| Analyzers | Built-in and custom analyzers | Compared analyzer by analyzer | Target includes built-ins, custom analyzer composition, normalizers, `_analyze`, index analyzer, and search analyzer; Tantivy-native support is required for full behavior | Source-backed for bounded `_analyze` with the default simple analyzer, `simple`, and `loom_simple`; custom analyzer component composition, text analyzer/search_analyzer mapping settings, and keyword normalizer settings return stable unsupported errors |
| Refresh | Auto-refresh, explicit refresh APIs, and `refresh` query params | Similar | Refresh controls derived index visibility and never turns Tantivy segments into canonical state | Source-backed immediate-visibility no-op with `refresh=true` and `refresh=wait_for` accepted on write routes |
| Security | Security plugin APIs exist | Different security model | Loom hosted auth, ACL, and audit are authoritative; OpenSearch-shaped security mutation is rejected; read-only identity/capability shims are allowed | Source-backed for account shim, unsupported read responses, and forbidden unsupported mutations |
| Cluster/node/admin APIs | Cluster and node APIs | Different operational model | Out of scope except narrow read-only compatibility responses needed by clients. No cluster mutation, shard allocation, snapshot, or plugin management in v1 | Target |
| Capability reporting | Product-specific APIs vary | Product-specific APIs vary | Loom exposes machine-readable compatibility rows: supported, degraded, unsupported, approximate, native-only, and target | Target |

### 6.2 Query DSL, aggregation, and analyzer target

The design target is full OpenSearch Query DSL, aggregation, and analyzer compatibility tracked by the
matrix. Implementation may ship in stages, but each unsupported body shape must return a stable
compatibility error and appear in capability reporting.

Full analyzer behavior beyond the bounded simple analyzer, native-only query variants, nested query execution,
filters aggregations, and pipeline aggregations
remain target work because they change public mapping/query contracts across core, IDL, C ABI,
bindings, MCP, CLI, hosted compatibility, and conformance.

Full analyzer compatibility means built-in analyzers, custom analyzers composed from character filters,
one tokenizer, token filters, normalizers, `_analyze`, index-time analyzers, and search-time analyzers.
Tantivy-backed native execution is required for full analyzer behavior.

### 6.3 Refresh target

The OpenSearch-compatible listener targets OpenSearch-style refresh behavior:

- default writes update Loom source state and become searchable after automatic refresh;
- `refresh=wait_for` waits for visibility when the listener policy allows it;
- `refresh=true` forces refresh when the listener policy allows it;
- `POST /{index}/_refresh` refreshes the target collection;
- capability and status responses distinguish ready, stale, refreshing, unsupported, and failed states.

The Tantivy index remains a derived artifact. Refresh changes search visibility; it does not make the
index canonical source data.

### 6.4 Aliases and multi-index

Aliases are versioned search-facet metadata. Wildcard expansion is configurable with OpenSearch-shaped
parameters where supported. Writes through an alias require a single write target, using
`is_write_index` when the alias points at more than one collection.

Examples:

```text
POST /events-2026-07,events-2026-08/_search
POST /events-*/_search
POST /events-current/_search
PUT /events-write/_doc/123
```

### 6.5 Security posture

Loom hosted auth, ACL, and audit are authoritative. The OpenSearch-compatible surface may expose
read-only auth or capability shim responses such as current-principal information, but security admin
routes do not mutate Loom ACL in v1.

## 7. Errors / parity / concurrency

- **Errors:** search-specific codes per 0033/0010 section 4 + core set.
- **Parity (0032 section 4.6):** the portable local query/read/index path is source-backed without Tantivy.
  Tantivy-backed ranking, derived inverted indexes, and OpenSearch-compatible serving are native-only
  target work. Hosted capability reporting must distinguish reduced portable search from native search.
- **Concurrency:** the source collection is the synchronized base layer. Derived indexes are
  rebuildable, source-digest-stamped, and excluded from sync; single writer.

## 8. Resolved Decisions

### RD-SE1 - Initial OpenSearch-compatible profile

OpenSearch is the primary compatibility target, with Elasticsearch compatibility tracked in the matrix
where useful. The target matrix covers full Query DSL, full aggregations, full analyzer compatibility,
bulk, multi-search, aliases, multi-index search, OpenSearch-style refresh, and read-only security shims.
Implementation can be staged, but unsupported features must return stable compatibility errors and
appear in capability reporting.

### RD-SE2 - Index-to-collection mapping

Each OpenSearch index maps to one Loom full-text collection in the served workspace. Multi-index and
wildcard expansion never cross workspace boundaries in this profile.

### RD-SE3 - Alias semantics

Aliases are versioned search-facet metadata. Wildcard expansion includes collections and aliases when
the request parameters allow it. Writes through aliases use OpenSearch-style `is_write_index` to select
a single write collection.

### RD-SE4 - Served transport names

The served surface is `fts`. Compatibility uses generic transport labels, not a product-named
transport, and `search` is not a compatibility alias:

```text
--transport rest
--transport ndjson
```

### RD-SE5 - Security posture

Loom hosted auth, ACL, and audit remain authoritative. OpenSearch-shaped security admin mutation is out
of v1 scope; read-only auth/capability shims may be exposed for client compatibility.
