# 0020 - Document Layer

**Status:** Partial. Structured document canonical root (collection manifest, prolly-tree document-record map, `body_ref`, canonical index-declaration catalog, retained tombstones), chunked large-body storage, bounded per-id merge, and public facade source-backed; richer document conflict records and full document-level diff reporting remain target. **Version:** 0.1.0.
**Capability:** `document`.

This spec defines the document facet: versioned collections of id-keyed opaque documents, commonly
JSON or CBOR bytes. Current source implements the Rust substrate in `loom-core::document`, the
workspace-scoped public facade (`document_put_text`/`document_get_text`/`document_put_binary`/
`document_get_binary`/`document_list_binary`/`doc_delete`), the language-neutral IDL
shape, the C ABI and C header projection, all eight language bindings, C ABI tests, native
index/query projection, hosted REST/JSON-RPC/gRPC native routes, and an executable facade behavior
runner in `loom-conformance`. The structured document canonical root (collection manifest, a prolly-tree document-identifier map of canonical
document records, content-addressed document bodies referenced by `body_ref`, and the canonical
index-declaration catalog) is source-backed. Chunked large-body storage and policy-governed retained
tombstones use the same document root schema with pinned canonical component roots. Bounded per-id
merge auto-merges independent document-id changes and leaves same-id or concurrent manifest-policy
changes as ordinary merge conflicts. Rich conflict records and full document-level diff reporting
remain target work.
Documents are keyed by string id; `document_list_binary` returns the canonical CBOR array of `[id, bytes]` pairs in id
order, and an absent id or collection reads as absent. Secondary indexes are a first-class design surface:
index declarations, field extraction, typed scalar normalization, collation, uniqueness, write-time
maintenance, rebuild status, query AST, and cursor semantics must be specified and source-backed before
MongoDB or Couchbase compatibility is implemented.

Every operation is scoped to one workspace's document facet. Cross-workspace document writes are out
of contract and must fail with `CROSS_WORKSPACE` once a public facade exposes them.

## 1. Current Implementation

`loom-core::document` implements:

- `Collection::new`;
- `len` and `is_empty`;
- `put(id, doc)`;
- `get(id)`;
- `delete(id)`;
- `ids()`;
- `iter()`;
- canonical `encode` and `decode`;
- `put_collection(loom, ns, name, collection)`;
- `get_collection(loom, ns, name)`.

Document ids are caller-supplied strings. Document values are opaque byte strings. Native index/query
operations parse JSON payloads when callers declare JSON field indexes or submit JSON query bodies, but
the base storage contract does not require every document payload to be JSON.

The collection segment (the document collection that holds id-keyed documents) has the canonical
parameter name `collection` (0042 section 5.1), replacing the legacy `name`.

`ids` and `iter` return entries in id order. `put` replaces an existing document at the same id inside
the in-memory collection. `delete` returns whether the id existed.

The public `Document` facade is source-backed through the IDL, C ABI, C header, CLI, Node, Python,
C++, Swift, JVM, Android, React Native, WASM, MCP data tools, and native hosted REST/JSON-RPC/gRPC
routes. There is no source-backed document-specific merge helper today.

### 1.1 Target Text-First Public Presentation Profile

The native document storage contract remains opaque bytes. Every public presentation - Rust facade,
CLI, IDL, C ABI, C header, language bindings, MCP, hosted REST, JSON-RPC, and gRPC - must distinguish
text content from arbitrary binary content rather than serializing normal task and operator documents
as JSON arrays of byte values.

The target public operation family is:

| Tool | Payload contract | Intended use |
| --- | --- | --- |
| `document_get_text` | Reads one document as valid UTF-8 and returns text, content digest, and the document `entity_tag`. Invalid UTF-8 returns a stable `DOCUMENT_NOT_TEXT` error. | Default for boards, tasks, decisions, prompts, results, learnings, Markdown, JSON, and other text content. |
| `document_put_text` | Accepts text, encodes it as exact UTF-8 bytes, and returns digest plus `entity_tag`. `expected_entity_tag` is optional. | Default text creation and update. A supplied entity tag performs a guarded compare-and-swap update; omission is an intentional blind upsert. |
| `document_replace_text` | Retains guarded UTF-8 find-and-replace behavior and returns replacements, digest, and `entity_tag`. | Bounded edits when whole-document replacement is not appropriate. |
| `document_get_binary` | Returns raw document bytes, content digest, and `entity_tag`. | Intentionally non-text payloads such as CBOR vectors, archives, image fixtures, and protocol payloads. |
| `document_put_binary` | Accepts raw document bytes and returns digest plus `entity_tag`. `expected_entity_tag` is optional. | Intentionally non-text payloads. |
| `document_list_binary` | Returns the canonical byte-oriented collection representation. | Binary collection export and low-level tooling. |

The ambiguous public raw names `document_get`, `document_put`, and `document_list` have been removed
in favor of their explicit `*_binary` forms across the IDL, C ABI, CLI, MCP, client, generated remote,
and binding-facing surfaces. `document_query` remains metadata-first and does not return document
payloads by default. Callers fetch a selected payload through the explicit
text or binary read operation.

This profile changes public facade names and shapes but does not change canonical document encoding or
the document facet's opaque-byte storage semantics. The engine may retain private byte-oriented helpers
where they are not externally observable. Text operations must not parse, normalize, or otherwise
reinterpret supplied text beyond validating and storing its exact UTF-8 encoding. All bindings must
expose the same text and binary distinction, including guarded text updates and stable error mapping.

The native compare-token name for document mutation is `expected_entity_tag`. Document surfaces must
not define a document-only `expected_digest` contract. Digest-named guards are permitted only inside
legacy private helpers or external compatibility adapters that explicitly translate into the shared
entity-tag conditional-mutation primitive.

## 2. Current Storage Shape

The document facet path is:

```text
/.loom/facets/document/<name>
```

`put_collection` writes a structured canonical root, not a single collection blob. The collection path
holds a `DocumentCollectionManifest` (format tag `loom.document.collection-root.v1`) that references
its component roots: `document_map_root` (the document-identifier map staged under `.maps/<collection>`),
`index_catalog_root` (the canonical index-declaration catalog under `.indexes/<collection>`), and an
optional `tombstone_root` (retained tombstones under `.tombstones/<collection>`). Document bodies are
content-addressed under `.bodies/<collection>/<digest>` and referenced from each record by `body_ref`.
`get_collection` reads the manifest, verifies `document_map_root`, `index_catalog_root`, and any
`tombstone_root` against the staged component bytes, and decodes the live-document map.
Workspace commit, branch, checkout, bundle sync, and clone see the manifest and its component trees as
ordinary committed content under the document facet.

The reserved internal roots (`.maps`, `.bodies`, `.chunks`, `.tombstones`, `.indexes`,
`.index-data`) are hidden from public collection listings: both the document lister
(`doc_list_collections`) and the shared generic lister
(`Loom::list_collections`, used by MCP and other read surfaces) exclude dot-prefixed reserved
segments, so implementation roots are never presented as user collections.

## 3. Current Encoding

The document-identifier map is a `ProllyMap` child named `documents` under the structured document
root. It is keyed by `DocumentId::canonical_key()` and stores canonical `DocumentRecord` bytes, so
shared prolly subtrees are preserved across commits, clone/sync reachability, and three-way merge.
A legacy flat `loom.document.map.v1` decoder remains for stale manifests. A `DocumentRecord` carries
`document_id`, `body_ref` (a content-addressed
`Direct` body digest for content at or below `DOCUMENT_CHUNK_THRESHOLD`, or a `Chunked` ChunkList root
for larger content),
`byte_length`, `entity_tag`, `record_revision`, `record_state` (`live`/`deleted`), and optional
`media_type`, `charset`, `content_encoding`, `source_metadata`, and `policy_flags`. The index catalog
(`loom.document.index-catalog.v1`) encodes canonical `DocumentIndexDeclaration` records and is the
load-bearing source of truth; its digest is the manifest `index_catalog_root`, verified on load and
refreshed atomically when index DDL changes it, with document records and their entity tags left
untouched.

The retained tombstone component (`loom.document.tombstones.v1`) encodes canonical
`[DocumentId, DocumentTombstoneRecord]` entries in id order. The default retention policy is
`no-retained-tombstones.v1`, so delete removes the live map entry and leaves `tombstone_root` unset.
When the collection manifest carries `retain-tombstones.v1`, delete removes the live map entry,
retains a tombstone record, and sets `tombstone_root` to the digest of the tombstone component.
Retained tombstones carry `prior_entity_tag`, deterministic `deletion_revision`, `deleted_entity_tag`,
`retention_class`, optional `reclaim_after`, and optional `deletion_reason`; normal reads keep hiding
deleted documents while verifying the retained tombstone root.

`DocumentId`, `DocumentBodyRef`, `DocumentRecord`, `DocumentTombstoneRecord`, `DocumentIndexDeclaration`,
and `DocumentCollectionManifest` canonical encodings have conformance vectors. Chunked body storage has
focused threshold and ChunkList layout vectors. Retained tombstones have positive and negative
source-backed vectors. Current source does not yet encode parse-cache records or merge/conflict
metadata.

## 4. Current Versioning and Merge Behavior

Current document collections version with the workspace because they are staged as structured
document entries in the workspace working tree. A commit snapshots the document-root Tree with every
other staged workspace path. `checkout_commit` and `checkout_branch` restore the document-root Tree
with the rest of the workspace tree.

Current source implements bounded per-id merge for document roots. If two branches change different
document ids and the non-map manifest fields match, merge combines the changed document records. If
both sides change the same document id differently, or if they concurrently change manifest policy,
index catalog roots, retention policy, tombstone roots, or capabilities, the document path remains an
ordinary unresolved merge conflict. Sync follows `CONFLICT-RESOLUTION-MATRIX.md`: branch/ref
divergence uses the S1 fast-forward boundary, and the source-backed document merge hook covers the
bounded S2 per-id merge case.

Current document diff uses the structured document snapshot path and reads records from the committed
prolly document map. Chunked large bodies are split at the shared FastCDC threshold and referenced
from `body_ref::Chunked`, so large body replacement can reuse unchanged chunk objects in the
committed component tree. Full document-specific diff records beyond the current cross-facet summary
remain target work. Native secondary indexes are maintained derived state for
declared single-field JSON scalar paths and can be rebuilt from committed documents after checkout,
clone, or sync.

## 5. Current Conformance

`loom-core::document` has unit tests for:

- put, get, and delete;
- ordered id iteration;
- canonical encode/decode;
- commit and checkout versioning.

`loom-conformance` contains document behavior scenarios and an executable public document facade
runner. The runner exercises put, get, replace, delete, commit and checkout versioning, clone
reachability, native exact indexed lookup, JSON query parsing/result formatting, cursor pagination, and
projection against the in-memory store.

## 6. Target Contract

The target public document facade should provide:

- put;
- get;
- delete;
- id scan;
- declared secondary indexes;
- indexed lookup and query;
- document-level diff;
- explicit per-id merge tooling;
- optional content-type or canonical-payload policy if required by callers.

Before hosted and full enterprise promotion, the remaining facade needs:

- hosted protocol methods in 0008;
- stable error mapping through `loom_core::error::Code`;
- access-control review for served document writes;
- clear file-projection behavior for `/.loom/facets/document/...`;
- same-id collision behavior aligned with `CONFLICT-RESOLUTION-MATRIX.md`.

### 6.1 Conditional mutation and comparison anchors

The document facade consumes the conditional-mutation contract owned by 0003 section 9.1. The anchor
for a document write is the current canonical document state at one document id, or an owner-issued
opaque revision for that state. The atomic scope is one document id for replacement, deletion, or a
declared atomic patch. A multi-document operation uses the 0003 section 6 batch transaction boundary;
it is not an implicit cross-document transaction.

Document mutations consume `any`, `absent`, `exact`, and `generation`. They do not consume
`operation_anchor` unless a promoted coordination facade explicitly protects a document operation. The
native contract does not prescribe an ETag, digest, or numeric version syntax. A MongoDB or Couchbase
adapter can expose a facade-specific token but cannot redefine the document comparison anchor.

Conditional mutation is not per-id merge. A stale replacement or patch fails without changing the
document; explicit per-id merge tooling remains this specification's target boundary. Authorization,
redacted audit evidence, and errors inherit 0009 and 0003 section 8, including no disclosure of a
protected current document when comparison fails.

## 7. Target Storage Contract

The structured canonical root has replaced the single whole-collection blob. The remaining enterprise
target is the prolly-tree component form that adds structural sharing and sublinear diff:

| Role | Encoding | Status |
| --- | --- | --- |
| Collection root | `DocumentCollectionManifest` referencing component roots and policy | Source-backed |
| Document map | Prolly-tree keyed by encoded `DocumentId`; values are canonical `DocumentRecord` bytes | Source-backed |
| Document body | Content-addressed `body_ref::Direct` digest | Source-backed |
| Large documents | `body_ref::Chunked` root / ChunkLists for bodies above `DOCUMENT_CHUNK_THRESHOLD` | Source-backed |
| Secondary indexes | Canonical `DocumentIndexDeclaration` catalog (source) plus derived materializations | Declarations + derived materializations source-backed; prolly-tree materialization form target |
| Tombstones | Retained `DocumentTombstoneRecord`s under `tombstone_root` | Source-backed for `retain-tombstones.v1`; richer lifecycle policy target |
| Parse cache | Optional derived records | Target, not identity unless explicitly promoted |

The id envelope, document record, collection manifest, index declaration, index catalog, chunked body
threshold/layout paths, and retained tombstone component are pinned with conformance vectors. If a
policy change alters canonical bytes, update conformance vectors with the implementation.

## 7.1 Native Index And Query Primitive Plan

The native document index/query plan is the next reusable substrate before any MongoDB or Couchbase
compatibility work. It must be useful without those product surfaces.

Current source-backed behavior: document collections can declare single-field exact-match indexes over
dotted JSON scalar paths; index creation backfills existing documents; `document_put_binary` and `doc_delete`
maintain index state; unique indexes reject duplicate scalar keys before writing the new document;
readiness/status, rebuild, drop cleanup, and exact-match `document.find` are available in core,
hosted REST/JSON-RPC, MCP `document_query`, CLI, C ABI/IDL/header, C++, iOS, JVM, Android KMP, React
Native, Node, Python, and WASM. The native query AST is also source-backed in core, hosted
REST/JSON-RPC, MCP, CLI, C ABI/IDL/header, C++, iOS, JVM, Android KMP, React Native, Node, Python, and
WASM for equality/range comparisons, `and`/`or`, exclusive id cursors, selected scalar projections,
and optional document bytes. Executable conformance covers native exact indexed lookup, JSON query
parsing/result formatting, cursor pagination, and projection.

| Primitive | Native purpose | Compatibility value | Priority |
| --- | --- | --- | --- |
| Declared index catalog | Source-backed for single-field dotted JSON paths, uniqueness, and canonical catalog storage | Required before MongoDB `createIndexes` or Couchbase query service claims | P1 done |
| Field-path extraction | Source-backed for nested JSON object fields and explicit array-index path segments | Required by MongoDB predicates and Couchbase-style queries | P1 done |
| Typed scalar normalization | Source-backed for JSON null, boolean, signed integer, unsigned integer, finite float, and text values | Avoids product adapters inventing incompatible comparison rules | P1 done |
| Maintained secondary index storage | Source-backed for exact-match lookup, backfill, incremental put/delete maintenance, and unique rejection | Required for performant MongoDB and Couchbase compatibility | P1 done |
| Rebuild and readiness status | Source-backed for ready/not-ready reporting, explicit rebuild, and drop cleanup | Required for operationally honest served ports | P1 done |
| Query AST | Source-backed for core, hosted REST/JSON-RPC, MCP, CLI, C ABI/IDL/header, C++, iOS, JVM, Android KMP, React Native, Node, Python, and WASM | Lets product adapters lower into one checked model | P1 done |
| Equality, range, and boolean predicates | Source-backed for comparisons plus `and`/`or` | Covers the compatible subset that can be truthfully supported first | P1 done |
| Cursor/page tokens | Source-backed as exclusive id cursors for the native contract; product cursor mapping remains target | Maps later to MongoDB cursors and Couchbase paged results | P2 done |
| Projection | Source-backed for selected scalar fields and optional document bytes in core, hosted, MCP, CLI, C ABI/IDL/header, C++, iOS, JVM, Android KMP, React Native, Node, Python, and WASM | Maps later to product projection fields | P2 done |
| Unique, compound, sparse, partial, and multikey indexes | Advanced native indexing after the single-field model lands | Improves future compatibility fidelity | P2/P3 |

Product-only work is outside the native index/query slice: MongoDB wire sessions, BSON command
compatibility, MongoDB aggregation, Couchbase SQL++/N1QL, Couchbase bucket/scope/admin service semantics,
analytics services, geospatial indexes, and CouchDB revision/replication semantics.

## 8. Relationship to Other Facets

- **KV:** document collections are KV-like maps with document-specific payload and index semantics.
- **SQL:** rich document queries should project into SQL or another query layer rather than becoming an
  ad hoc query language inside the document facet.
- **Vector and graph:** document chunks may reference vector ids or graph nodes, but those indexes and
  relationships remain in their owning facets.
- **Compute:** `loom-compute` has a document capability tag, but document state access from programs is
  target work until 0015 defines and implements it.

## 9. Non-Goals and Limits

- Current source is not a MongoDB or Couchbase-compatible document database service.
- Current source validates JSON only for native index/query inputs that require JSON semantics.
- Current source provides the bounded native single-field index/query substrate, not product database indexes.
- Current source provides a structured canonical document root, a prolly-tree document map with
  structural sharing, chunked large-body storage, retained tombstones, and bounded per-id merge.
- Current source does not provide rich document conflict records or full document-level diff reporting.

## 10. Unfinished Work

| Order | Parent | Work item | Status | Exit criteria |
| --- | --- | --- | --- | --- |
| T1 | RD5 | Spec/source reconciliation | Complete local | Current implementation and conformance text describe the implemented local document facade, CLI, and MCP data projection instead of stale target-only language. |
| T2 | RD5 | Document data CLI projection | Complete local | `loom document ...` commands expose put, get, delete, and list with byte-stable output forms. |
| T3 | RD5 | Hosted document wire projection | Complete local plus native hosted | REST, JSON-RPC, and native gRPC adapters expose put, get, delete, list, index create/drop/list/status/rebuild, find, query, ACL behavior, and stable errors. Collection discovery, generated gRPC artifacts, and broader protocol conformance remain target. |
| T4 | RD4 | Native secondary-index and query surface | Complete local plus native hosted | Index declarations, field-path extraction, typed scalar normalization, maintained secondary indexes, query AST, cursor tokens, readiness status, bindings, hosted REST/JSON-RPC, MCP, CLI, and conformance are source-backed before product compatibility ports. |
| T5a | RD6 | Structured document canonical root | Source-backed | Collection manifest, document-identifier map of canonical records, content-addressed `body_ref`, and canonical index-declaration catalog with a load-bearing/verified/atomically-refreshed `index_catalog_root` are source-backed with conformance vectors. |
| T5b | RD6 | Prolly-tree map, sublinear diff, per-id merge | Source-backed subset | Prolly-tree document map and bounded per-id merge are source-backed with focused tests and conformance root proof; rich conflict records and full document-level diff reporting remain target. |
| T5c | RD6 | Chunked large-body storage | Source-backed | Bodies above `DOCUMENT_CHUNK_THRESHOLD` use `body_ref::Chunked` with canonical ChunkList roots and chunk component files; bodies at or below the threshold stay `body_ref::Direct`. |
| T5d | RD6 | Retained tombstones | Source-backed | Delete under `retain-tombstones.v1` writes canonical `DocumentTombstoneRecord` entries under `tombstone_root`, verifies that root on load, and keeps normal reads hiding deleted documents. |
| T6 | 0061 | Body-aware `document.patch` | Target | Reserve `document.patch` for schema-aware or 0061 body-model patches with base entity versions, stale-base rejection, and conflict records. The current MCP text edit surface is `document_replace_text`, not `document.patch`. |

### 10.1 Storage Promotion Closure Audit

Promotion scope: the native document facet structured storage root for a named collection.

| Closure gate | Status | Evidence | Remaining work classification |
| --- | --- | --- | --- |
| Canonical root contract | Source-backed | `DocumentCollectionManifest` names the format tag, document map root, index catalog root, optional tombstone root, retention policy, and capabilities; `DocumentRecord`, `DocumentBodyRef`, `DocumentTombstoneRecord`, and `DocumentIndexDeclaration` have canonical encodings and vectors. | None for the current native storage scope. |
| Source boundary | Source-backed | Canonical source is the collection manifest plus document records, body refs, index declarations, and retained tombstones. Native secondary-index materializations remain derived rebuildable state. | None for the current native storage scope. |
| Unit addressing | Source-backed | Document records are keyed by `DocumentId::canonical_key()` in the prolly document map, with bodies addressed by digest or ChunkList root. | None for the current native storage scope. |
| Incremental mutation | Source-backed subset | The prolly document map and chunked body refs preserve structural sharing for changed document ids and large bodies. | Full document-body delta reporting remains P1 facet primitive work, not closure-blocking native root debt. |
| Diff and merge semantics | Source-backed subset | Bounded per-id merge is source-backed for disjoint document-id changes when non-map manifest fields match. Current diff reads committed structured document snapshots. | Rich conflict records and full document-level diff reporting are closure-blocking promotion debt before the full document merge/diff contract can close. |
| Query and scan behavior | Source-backed subset | Native list, find, query, cursor pagination, projection, index status, index rebuild, and hosted REST/JSON-RPC/gRPC projection are source-backed for the bounded native query profile. | Body-aware `document.patch` and richer body-model predicates are P1 facet primitive work owned by 0061. |
| Retention and garbage collection | Source-backed subset | `retain-tombstones.v1` writes canonical retained tombstones under `tombstone_root`; normal reads hide deleted documents while verifying the retained root. | Richer tombstone lifecycle policy, physical reclamation policy, and retention scheduling are P1 facet primitive work. |
| Migration and compatibility | Source-backed subset | The legacy flat `loom.document.map.v1` decoder remains for stale manifests, and structured roots are restored through commit, checkout, clone, and sync. | Migration vectors for future root versions remain closure-blocking promotion debt for any later root-format promotion. |
| Conformance proof | Source-backed subset | Conformance vectors cover manifest, record, tombstone, ChunkList threshold/layout, public facade behavior, clone reachability, native exact indexed lookup, query formatting, cursor pagination, and projection. | Rich conflict vectors, full document-level diff vectors, and future-version migration vectors are closure-blocking promotion debt for full closure. |
| Product-profile boundary | Recorded | Current source is not MongoDB, Couchbase, or CouchDB compatible; MongoDB and Couchbase remain P3/spec-owned compatibility candidates, and CouchDB serving is cut from active scope. | MongoDB/Couchbase/CouchDB behavior is compatibility/profile work, not evidence for native document root promotion. |

## 11. Resolved Decisions

- **RD1 - Current storage.** Current source stores each named collection as a structured canonical root
  under the workspace document facet: a `DocumentCollectionManifest` referencing a document-identifier
  map of canonical `DocumentRecord`s, content-addressed document bodies (`body_ref`), and a canonical
  `DocumentIndexDeclaration` catalog whose digest is the manifest `index_catalog_root` (verified on
  load, refreshed atomically on index DDL). The legacy single-collection blob is no longer the stored
  form; reserved implementation roots (`.maps`/`.bodies`/`.indexes`/`.index-data`) are hidden from
  public collection listings.
- **RD2 - Current identity.** Caller-supplied string document ids are the current identity keys.
- **RD3 - Current payload.** Document values are opaque bytes.
- **RD4 - Index boundary.** Native secondary indexes are source-backed derived state for the bounded
  single-field JSON scalar contract, not a separate committed source of truth.
- **RD5 - Public facade status.** The workspace-scoped public `document` facade (put/get/delete/list,
  string ids) is source-backed across IDL, C ABI, C header, CLI, and all eight bindings, with an
  executable facade conformance runner. Native single-field indexes/query and hosted
  REST/JSON-RPC/gRPC projection are source-backed; product compatibility ports, broader protocol
  conformance, rich conflict records, and full document-level diff reporting remain target work.
- **RD6 - Merge boundary.** Bounded per-id merge is source-backed for disjoint document-id changes
  when non-map manifest fields match. Same-id conflicts, concurrent manifest-policy changes, rich
  conflict records, and full document-level diff reporting remain target work.
- **RD7 - Compatibility priority.** MongoDB and Couchbase compatibility are P3/spec-owned until native
  document indexes/query are source-backed. CouchDB serving is cut from the active Queue 2 build plan.

## Change log

### Document public facade (engine slice; 0007, 0010)

The id-keyed document facade is source-backed end to end (engine portion of the Document + Time-series +
Ledger batch): `loom-core::document` adds explicit text and binary document functions plus `doc_delete` (string id, absent
reads as empty/None); projected to IDL `interface Document`, C ABI `loom_doc_*` (with a round-trip
test), the C header, and all eight bindings; covered by the executable `run_document_facade_behavior`
runner (put/get/replace/delete, commit/checkout versioning, clone reachability) wired into
`certify_memory_store`, with the `document` capability flipped from `scenario` to `executable` (registry
+ 0010 section 5). Secondary indexes are deferred to a 0020a decision record per RD5.

The MCP host adds source-backed ergonomics over the same opaque-byte collection contract:
`document_query` lists id-filtered metadata rows (`id`, byte length, and store-profile content digest),
`document_get_binary` exposes binary payloads, and `document_replace_text` performs guarded UTF-8
find/replace using a caller-supplied base digest. Body-field predicates and the public
`document.patch` name remain target work until a collection schema or the 0061 body model is available.

- 2026-06-27 (P-bindings): Document facade (explicit text/binary put/get/list plus delete) now has
  full language-binding parity across all eight families (Node, Python, WASM, C++, iOS/Swift, JVM,
  Android JNI+Kotlin, React Native) over the `loom_doc_*` C ABI / `loom_core` facades. Verified
  via `just test-bindings`.

- 2026-07-18 (DOC-ROOT reconciliation; MX-131/MX-138/MX-152): storage is now the structured canonical
  root, not a single collection blob. `put_collection`/`get_collection` persist a
  `DocumentCollectionManifest` referencing a document-identifier map of canonical `DocumentRecord`s,
  content-addressed bodies (`body_ref::Direct`), and the canonical `DocumentIndexDeclaration` catalog;
  `index_catalog_root` is load-bearing, verified on load, and refreshed atomically on index DDL with
  document entity tags preserved. Reserved implementation roots (`.maps`/`.bodies`/`.indexes`/
  `.index-data`) are hidden from public collection listings. MX-188 adds chunked large-body storage
  through `body_ref::Chunked` with canonical ChunkList roots and threshold vectors. MX-187 adds
  policy-governed retained tombstones through `tombstone_root` with positive and negative vectors.
  Bounded per-id merge is source-backed. Rich conflict records, full document-level diff reporting,
  richer tombstone lifecycle policy, and full `DocumentIndexDeclaration` surface projection remain
  target work (tracked as concrete tickets).
