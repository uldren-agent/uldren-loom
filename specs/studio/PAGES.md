# Pages - Shared Page Organization

**Status:** Target design. **Version:** 0.1.0-target.
**Capability:** `pages`.

This document defines a page organization, wiki, and collaborative page organization on top of Loom. It
is an Studio application profile, not a replacement for the core document, filesystem, SQL, graph,
append-log, execution, trigger, synchronization, access-control, or MCP specs.

**Surface decision (owner, 2026-07-04): "Notion surface, Notion internals."** The internals were
already Notion-shaped (0061 §9.1 block model, §21 structures-as-databases). The target user surface
is now also Notion-class --- block editor, databases/structures as first-class views, flexible page
composition --- rather than Confluence's spaces-and-macros surface. Confluence remains what this
profile *imports from* and the reference for enterprise wiki semantics (permissions, exports,
compliance); it is no longer the UX baseline. The reframe pass ran 2026-07-04 (prompt-5 session,
owner decisions): §5.1 now separates surface kinds from import kinds with Notion names canonical
(`callout` replaces `panel`, `toggle` replaces `expand`); "space" stays as the scope term (it is
the generic 0061 scope name; Notion teamspaces import onto it); "macro" is import-only vocabulary ---
`macro_reference` remains a registered kind, but the surface contract does not describe macros as
a feature; databases are first-class structures with views (§21).

Pages exists because a page organization is not just a collection of files or opaque
documents. It exposes spaces, page trees, rich text, collaborative editing, comments, inline
comments, labels, templates, databases, attachments, mentions, backlinks, permissions, audit
history, and agents that read, update, summarize, and curate pages. Users expect stable page URLs,
live collaboration, page history, and conflict handling without thinking about branches or manual
merges.

## 1. Contract Boundaries

The design builds on these contracts:

- `0020-document-layer.md` defines the current document boundary and the target structured document map.
  Current source stores each named collection as one canonical blob and does not provide per-document
  merge.
- `0006-synchronization.md` defines current sync as movement of immutable content-addressed objects plus
  mutable workspace refs. Current branch sync is fast-forward only.
- `0021-append-log-layer.md` defines the current queue boundary and the target structured stream shape.
- `0011-tabular-and-query-layer.md` and the SQL facet provide a natural projection for page indexes,
  labels, permissions, and reports, but the page operation log remains the collaboration source of truth.
- `0016-graph-layer.md` is the natural projection for page trees, backlinks, includes, references, and
  page graphs.
- `0015-execution-and-logic.md` and `0029-events-and-triggers.md` define target execution and reactive
  automation boundaries. Pages automation must preserve their determinism, metering, and audit
  model.
- `0031-end-to-end-encrypted-sync.md` defines the blind-replica topology where a local client holds keys
  and the remote stores ciphertext by opaque labels.
- `SLACKISH.md`, `DRIVEISH.md`, and `JIRAISH.md` define the Studio pattern used here: local-first
  replicas, blind cloud by default, keyed workers where content-aware compute is required, MCP resources,
  and durable callbacks.
- `0061.md` defines the shared operation substrate: envelope, sequencer, durable cursors, order
  tokens, conflict records, annotations, entity versioning, projections/views, and cross-facet
  search. Where this document's local envelope or sequencer text differs, 0061 supersedes it
  (see §23).
- `SURFACES.md` defines the human experience layer (MCP Apps, elicitation flows, visualizations)
  rendered over this profile's projections and structures.

This document depends on those boundaries. It does not make raw current branch sync sufficient for five
local assistants concurrently editing the same page tree.

## 2. Product Model

A Pages organization exposes spaces, pages, comments, and page structures:

```text
pages
  spaces
  page-trees
  pages
  revisions
  drafts
  comments
  inline-comments
  attachments
  labels
  templates
  macros
  backlinks
  mentions
  watches
  permissions
  exports
  automations
  cursors
  audit
  retention
```

The user-facing contract is:

- page creation is durable and idempotent;
- pages have stable identities independent from title and location;
- page trees converge to a stable order;
- concurrent edits do not silently discard content;
- comments and inline comments append without overwriting each other;
- page history can restore any retained revision;
- agents can observe, draft, summarize, classify, link, and update pages under scoped grants;
- audit history can reconstruct who or what changed a page and why.

## 3. Cloud and Encryption Model

The same cloud topologies from the other Studio specs apply.

```text
Private page organization:
  Local Loom replicas hold keys and full page state.
  Loom Cloud is blind storage, operation sequencing, notification, and opaque coordination.
  Search, summaries, semantic links, and agent execution happen locally or in a tenant-controlled keyed worker.

Managed enterprise page organization:
  A keyed service runs inside the tenant trust boundary.
  It can index, render, summarize, classify, run macros, export pages, and host agents.

Hybrid page organization:
  Blind Loom Cloud stores canonical encrypted page history.
  Selected spaces or page sets are mirrored to keyed compute replicas for approved workloads.
```

A blind remote can sequence page operations and coordinate opaque page-tree updates, but it cannot inspect
page text, compute semantic backlinks, render previews, evaluate content macros, summarize pages, or
classify restricted content.

## 4. Storage Layer

Pages uses Loom content-addressed objects for immutable payloads and Merkle-backed indexes for
current space state.

```text
page organization root
  operation log root
  space index root
  page index root
  page revision index root
  page tree index root
  draft index root
  comment index root
  inline comment index root
  label index root
  backlink graph root
  attachment index root
  permission index root
  audit index root
```

The operation log is the source of truth. Documents, tables, and graphs are projections:

- document projection for current page bodies and drafts;
- SQL projection for labels, watches, permissions, reports, and recent updates;
- graph projection for page tree edges, backlinks, embeds, mentions, and semantic relationships;
- append-log projection for page history, comments, and audit trails;
- search and vector projections for full-text and semantic discovery where keys are available.

Page identity is a stable `page_id`. Human-readable URLs derive from title and tree location but must not
be the identity. Renaming or moving a page changes aliases and tree edges without changing `page_id`.

## 5. Page Body Model

**Pinned (owner decisions, 2026-07-04 body-model session).** The page body model is the 0061 §9.1
rich body model: a pinned block-structured canonical document plus sequenced body deltas, with a
snapshot revision at every publish boundary. The format, delta primitives, canonical
normalization, offset rules, block-id allocation, concurrency protocol, epoch/shred semantics, and
range-anchor mapping are substrate-owned (promoted from this spec by owner decision); this section
defines the page-specific profile over them. The candidate survey that produced the decision is
recorded in §5.5.

### 5.1 Page Blocks and Marks

Pages use the substrate block kinds and marks (0061 §9.1) plus these registered page kinds.
Reframed 2026-07-04 (owner decision: Notion names are canonical --- the surface is Notion-class, so
the surface vocabulary is too; `callout` replaces `panel`, `toggle` replaces `expand`):

Surface kinds (what the editor offers):

```text
callout           block: icon?, color?, kind? --- freeform admonition box; Confluence panels
                  import with kind (info | note | warning | success | error) mapped to a
                  preset icon/color, kind attr retained for round-trip
toggle            block: collapsible section (Confluence expand imports onto it)
layout            block: column container (Notion column_list/column and Confluence
                  layouts import into it)
equation          block: LaTeX expression; inline equations are runs carrying the
                  equation mark
mention           inline mark: principal or entity reference per the reference grammar (0061 §19)
status            inline: label + color chip
attachment_ref    block or inline: attachment_id reference (§10)
structure_embed   block: structure_id reference (§21); binds, does not copy --- inline
                  databases render through it
comment           block or inline: authorial hidden text (never rendered; round-trips) ---
                  Obsidian %%comments%% import onto it
```

Registered page marks beyond the substrate set: `sub`, `sup` (§5.4), `equation(latex)`, and
`color(fg?, bg?)` --- Notion text/background colors and Obsidian `==highlight==` (background)
import onto `color`.

Import-only kinds (never offered by the editor; preserved and round-tripped):

```text
macro_reference   block or inline: macro name, parameter map, original source (see §8.5);
                  "macro" is import vocabulary --- the activation path for these nodes is
                  §8.5/§23, not a surface feature
```

Heading anchors derive from `(block_id)` --- never from heading text --- so anchors survive retitling.
Unknown kinds and marks round-trip untouched per 0061 §9.1; this is what makes import lossless and
version skew safe.

### 5.2 Edits, Publishing, Epochs

- A body edit is a `page.patch_applied` operation whose payload is a list of 0061 §9.1 delta
  primitives applied atomically; `page.body_replaced` carries the degenerate whole-body delta.
- The concurrency protocol is reject-stale-base with client rebase (0061 §9.1): persisted page
  body history is strictly linear. Live co-editing is clients racing small deltas through the
  sequencer; there is no CRDT and no server-side transform.
- Publishing (`page.published`, §8.4) writes the snapshot revision to CAS and closes the delta
  epoch. Drafts (§8.4) accumulate deltas without publishing.
- Retention hard-deletes a published revision by retiring its epoch key (0061 §9.1): snapshot and
  epoch deltas become unreadable; operation facts and version numbers persist for audit (§12).
- Whole-page snapshot + conflict record (§8.1) remains the degenerate mode: the required mode for
  per-actor-log topologies (§7.2) and the fallback for clients that do not speak deltas. CRDT
  bodies are explicitly deferred (0061 §9.1) --- recorded as a future option for offline-heavy
  actor-log deployments only, with shred and canonical-bytes conflicts acknowledged.

### 5.3 Inline Comment Anchors

Inline comments are 0061 §7 annotations with `range` anchors per 0061 §9.1:
`(block_id, start, end, base_revision, quoted_text)`, UTF-8 byte offsets. Re-anchoring maps the
range deterministically through subsequent deltas using the same offset-mapping function that
client rebase uses. On lost or ambiguous mapping (block removed, epoch shredded, range destroyed)
the anchor degrades to block-level then page-level with the §8.3 stale-anchor marker; the stored
quote keeps the comment renderable and feeds heuristic re-anchor assist. Resolving §19.7.

### 5.4 Confluence Import Mapping (bodies)

Required per ADOPTION §1.3. Both source formats carry equal commitment with full conformance
vectors each (owner decision): storage-format XHTML from offline space/site exports and ADF JSON
from file snapshots or MCP-assisted normalized input. Because two mappings can drift semantically,
the vector set must include
**cross-format equivalence vectors**: the same logical page expressed in both source formats must
import to byte-identical canonical bodies (macro references and unknown-node preservation
included), and any intentional divergence must be recorded in the fidelity report. One mapping
table, two source columns, one target column:

Current source backs the Confluence import planning contract in `loom-interchange` and the first
normalized execution slice through the reusable Confluence importer in `loom-interchange-io`.
`ProfileImportPlan` supports distinct `ConfluenceStorage` and `ConfluenceAdf` source systems, and
`ProfileImportAction` pins planned page, body-replacement, annotation, attachment, source-sidecar,
and fidelity actions by source entity id and digest. `loom interchange import-confluence` now accepts
a normalized snapshot JSON file, creates or reuses Pages spaces, preserves storage-format XHTML or
ADF JSON bytes in canonical opaque Body blocks, publishes changed pages through the Pages profile
service, skips unchanged pages idempotently, and emits the shared 0012 import report. The
source-backed fixture in `specs/studio/fixtures/confluence/` verifies space creation, storage XHTML
opaque body retention, ADF opaque body retention, markdown/text body lowering, parent placement,
explicit space metadata coverage, page metadata coverage, label/property/restriction/link coverage,
and fidelity issues for unsupported source structures. Actual site/export parsing, full XHTML/ADF
block lowering, attachment import, comments, native labels, native properties, native restrictions,
native page version metadata, MCP-assisted normalized import, and cross-format semantic equivalence
vectors remain target work. Present but unsupported space metadata, page metadata, labels,
properties, restrictions, attachments, comments, ancestors, descendants, and links emit shared
fidelity issues.

#### 5.4.1 Source-Backed Field Coverage

| Confluence source shape | Current importer status | Evidence / remaining work |
| --- | --- | --- |
| Space id/key and name | 1:1 imported | Explicit spaces create Pages spaces; page-only imports still create spaces from `space_id`. |
| Space description, type, status, homepage, author, created time, links, labels, properties, and permissions | unsupported with fidelity issue | Fixture covers the fields. Native space metadata, labels, properties, and permission lowering remain target work. |
| Page id, title, space, and direct parent id | 1:1 imported | Pages are created or updated through the Pages profile service; direct parent placement is preserved. |
| Page storage-format XHTML body | retained as opaque canonical body block | Fixture verifies `confluence.storage` byte retention. Full storage XHTML block lowering remains target work. |
| Page ADF body | retained as opaque canonical body block | Fixture verifies `confluence.adf` byte retention. Full ADF block lowering remains target work. |
| Opaque body-region references | retained as opaque canonical body blocks instead of invented `block_ref` targets | Covered; expected fixture asserts opaque-body retention for storage XHTML and ADF pages. |
| Page markdown and text fallback | imported through the markdown body path | Fixture verifies lowered text content. |
| Page status, version, author, owner, created time, links, labels, properties, restrictions, ancestors, and descendants | unsupported with fidelity issue | Fixture covers the fields. Native metadata, range-safe permission, label, property, version, and tree projection remain target work. |
| Attachments and comments | unsupported with fidelity issue | Fixture covers representative official shapes. Native attachment import and annotation lowering remain target work. |
| Live Confluence API pagination, site export parsing, offline archive parsing, and cross-format equivalence | not yet fixture-covered | Current source accepts normalized JSON only. |

| Storage format (XHTML) | ADF | Target block/mark |
| --- | --- | --- |
| `<p>` | `paragraph` | `paragraph` |
| `<h1>`-`<h6>` | `heading` | `heading(level)` |
| `<ul>`/`<ol>` + `<li>` | `bulletList`/`orderedList` + `listItem` | `list_item(bullet | ordered)` per item; wrappers dropped (no list-wrapper nodes) |
| `<ac:task-list>` / `<ac:task>` | `taskList` / `taskItem` | `list_item(task)` with checked attr |
| `<table>`, `<tr>`, `<td>`/`<th>` | `table`, `tableRow`, `tableCell`/`tableHeader` | `table`, `table_row`, `table_cell(header?)` |
| `<ac:structured-macro ac:name="code">` | `codeBlock` | `code_block(language)` |
| `<blockquote>` | `blockquote` | `quote` |
| `<hr>` | `rule` | `divider` |
| `<ac:image>` + `<ri:attachment>`/`<ri:url>` | `media`/`mediaSingle` | `attachment_ref` (attachment imported first) or `embed(url)` |
| `<ac:link>` + `<ri:page>`/`<ri:user>` | `inlineCard` / `mention` | `link` mark with reference-grammar target; user refs -> `mention` |
| `<ac:structured-macro ac:name="status">` | `status` | `status` |
| `<ac:structured-macro ac:name="expand">` | `expand` | `toggle` |
| `<ac:structured-macro ac:name="info|note|warning|tip|panel">` | `panel` | `callout(kind)` --- kind retained, preset icon/color (§5.1) |
| `<ac:layout>` / `<ac:layout-section>` | `layoutSection`/`layoutColumn` | `layout` |
| `<ac:emoticon>` | `emoji` | text (unicode) or `embed(emoji)` when custom |
| bold/em/u/s/code/sub/sup marks | equivalent ADF marks | substrate marks; `sub`/`sup` as registered page marks |
| any other `<ac:structured-macro>` | `extension`/`bodiedExtension` | `macro_reference` preserving name, parameters, and original source --- "preserved, not executable" in the fidelity report (owner decision; see §8.5 for later activation) |
| anything unmapped | anything unmapped | unknown-node preservation with a fidelity report entry |

Import writes one `page.body_replaced` operation per page (client-minted block ids, declared in
the envelope per 0061 §9.1), then the snapshot publish. The per-run fidelity report (ADOPTION
§1.3) lists mapped, preserved-not-executable, and degraded nodes per page. Comments and inline
comments import as annotations; inline-comment markers (`<ac:inline-comment-marker>`) map to §5.3
range anchors against the imported revision, with the exported highlight text stored as the
anchor quote.

### 5.5 Candidate Survey (record of the 2026-07-04 decision)

Three candidates were scored against: canonical bytes, blind replica with deterministic
client-side merge, retention/crypto-shred, inline-comment anchoring, structure/diagram binding,
Confluence import fidelity, and history size.

- **(a) Whole-page snapshots + conflict records** (the prior stopgap): trivially canonical and
  shreddable, but no live co-editing and only heuristic anchoring. Retained as the degenerate
  mode, not the model.
- **(b) Sequenced deltas** (chosen): Loom's blind sequencer supplies a total order, which is the
  precondition classic OT lacks without a content-reading server --- the same argument Figma made
  for dropping CRDTs in a centrally-ordered system, and ProseMirror's collab protocol proves the
  reject-and-rebase client loop. Linear persisted history keeps the conformance surface small
  (primitives + offset mapping, not a transform algebra). Epoch snapshots give per-revision shred.
- **(c) CRDTs** (Yjs / Automerge / Loro; Peritext for rich-text marks): best anchor stability and
  the only option for convergence *without* central order, but structurally in conflict with
  per-revision crypto-shred (Automerge retains full history by design; Yjs cannot GC tombstones
  while preserving ordering guarantees; only Loro offers coarse tail-trimming via shallow
  snapshots), with unbounded history growth on heavily edited pages and a canonical-bytes
  conformance surface equal to the entire CRDT algorithm cross-language. Rejected for the default
  topology; recorded as the future option for actor-log offline merge only.

Key sources: ProseMirror collab guide and Marijn Haverbeke's collaborative-editing essay; Figma
"How Figma's multiplayer technology works"; Yjs INTERNALS and Y.RelativePosition docs; Automerge
2.0 and binary format spec, automerge issue #799; Loro shallow-snapshot and movable-tree posts;
Ink & Switch Peritext; Atlassian storage-format and ADF documentation; Notion block/transaction
model post.

### 5.6 Notion Import Mapping (pinned 2026-07-04)

Build-focus requirement (ADOPTION §1.3). **The Notion API is the import source** (block tree +
data sources; the API version is pinned per run and recorded in the fidelity report; type names
below follow the 2025-09/2026-03 API). The organization Markdown/HTML exports lose schema typing,
block ids, view definitions, and synced-block identity and are accepted only as a degraded
fallback, flagged as such. Import writes one `page.body_replaced` per page (client-minted block
ids per 0061 §9.1) plus the snapshot publish, mirroring §5.4; runs are idempotent per §25.5-style
fidelity machinery (JIRAISH §25.5 schema reused).

Current source backs the first Notion API-bundle execution slice through the reusable Notion importer
in `loom-interchange-io`. `loom interchange import-notion` accepts post-adapter snapshot JSON and
Notion API-shaped page plus block-children bundles with page ids, titles, optional parent page ids,
and page body blocks, creates or reuses pages spaces, lowers parent-child page placement, lowers
headings, paragraphs, list items, quote lines, and dividers into canonical Body blocks, publishes
changed pages through the pages service, skips unchanged pages idempotently, and emits the shared 0012
import report.

The source-backed fixture at `specs/studio/fixtures/notion/source/notion-api-bundle.json` is derived
from Notion's Page, Block, block-children, rich-text, data-source, view, comment, user, and file
object documentation. It imports into a clean store and compares supported page content, parent
placement, and fidelity issues against `specs/studio/fixtures/notion/expected/comparison.json`.
Current source classifies Notion API source metadata, property values, database parents, formula and
rollup properties, views, comments, permissions, users, attachments, synced blocks, rich-text
annotations/links/mentions/equations, and unsupported blocks as fidelity issues. Exact block-tree
lowering, native database structures, formulas, rollups, views, comments, permissions, principal
mapping, attachments, synced blocks, and richer rich-text marks remain target work.

#### 5.6.1 Notion Current Coverage Matrix

The current Notion fixture is a broad source-backed execution fixture for the first production
import slice. It is not a full native-projection acceptance suite: fields classified as retained or
degraded are preserved as page text, source metadata, or fidelity records, while native projections
such as databases, comments, permissions, attachments, synced blocks, and rich text marks remain
target work.

| Notion source field or entity | Current handling | Fixture coverage |
| --- | --- | --- |
| Page id | Imported as page id | Covered |
| Title property | Imported as page title | Covered |
| Parent page id | Imported as parent page placement | Covered |
| Workspace/database parent | Workspace parent imports into default space; database parent emits `database` fidelity issue | Covered |
| Created/edited timestamps and users | Fidelity warning as source metadata; revision provenance target | Covered |
| Icon, cover, URL, public URL, trash/archive state | Fidelity warning as source metadata; metadata mapping target | Covered |
| Page property values other than title/formula/rollup | Fidelity warning as `property_values`; shared field mapping target | Covered |
| Paragraph block | Imported as paragraph body block | Covered |
| Heading blocks | Imported as heading body blocks for supported levels | Covered |
| Bulleted, numbered, and to-do blocks | Imported as list text; checked state target | Covered as degraded |
| Quote and divider blocks | Imported as quote and divider blocks | Covered |
| Rich text annotations, links, colors | Plain text imported; fidelity warning as `rich_text_semantics`; marks and references target | Covered |
| Mentions and inline equations | Plain text imported; fidelity warning as `rich_text_semantics`; reference-grammar mention and equation targets | Covered |
| Child page and child database blocks | Fidelity warning as unsupported block; page-tree edge or database structure embed target | Covered |
| Image, video, file, PDF, audio | Fidelity warning as unsupported block and attachment category; attachment mapping target | Covered |
| Bookmark, embed, link preview | Fidelity warning as unsupported block; embed records target | Covered |
| Code, equation, table, column, callout, toggle, table of contents, breadcrumb | Fidelity warning as unsupported block; target structured body blocks or render-only preservation | Covered |
| Synced blocks | Fidelity warning; block reference or copied-original behavior target | Covered; expected fixture asserts synced-block fidelity issue as the current import-time reference behavior |
| Comments | Fidelity warning; target 0061 annotations | Covered |
| Database schema and rows | Fidelity warning for database parent metadata; target structures and field definitions | Covered as source input; native structures target |
| Formula, rollup, relation, status, views | Fidelity warnings for formula, rollup, and view data; target fields, computed fields, edges, saved views | Covered |
| Page permissions and share grants | Fidelity warning; target ACL projection | Covered |
| Users | Fidelity warning; target principal mapping | Covered |
| Unknown block types | Fidelity warning; opaque preservation target | Covered |

Block mapping (unlisted types -> unknown-node preservation + fidelity entry):

| Notion block | Target | Notes |
| --- | --- | --- |
| `paragraph` | `paragraph` | --- |
| `heading_1..4` | `heading(level)` | `is_toggleable` -> `toggleable` attr; children preserved |
| `bulleted_list_item` / `numbered_list_item` / `to_do` | `list_item(bullet | ordered | task)` | flat siblings on both sides --- no wrappers to synthesize; `checked` attr |
| `toggle` | `toggle` | --- |
| `callout` | `callout(icon, color)` | --- |
| `quote` | `quote` | --- |
| `code` | `code_block(language)` | caption -> attr; `mermaid` language preserved |
| `equation` | `equation` | inline equations -> runs with the `equation` mark |
| `divider` | `divider` | --- |
| `table` / `table_row` | `table` / `table_row` / `table_cell` | header flags -> cell attrs |
| `column_list` / `column` | `layout` | `width_ratio` -> attr |
| `synced_block` (original) | its children import in place | original's block id recorded |
| `synced_block` (`synced_from`) | `block_ref(page, original block, mode: block)` | 0061 §9.1 semantics; original outside the import set -> unresolved ref, late binding (0061 §19.3), fidelity entry |
| `child_page` / `child_database` | page-tree edge / `structure_embed` | --- |
| `image` / `video` / `file` / `pdf` / `audio` | `attachment_ref` (bytes fetched) or `embed(url)` for external | --- |
| `bookmark` / `embed` / `link_preview` | `embed(url)` | captions preserved |
| `link_to_page` | `link` mark with reference-grammar target | --- |
| `table_of_contents` / `breadcrumb` | render-only kinds preserved as opaque | no content to lose |
| `template` / `meeting_notes` / `tab` | unknown-node preservation | fidelity entry; `unsupported.block_type` (buttons, forms) recorded by name |
| rich text `annotations` | substrate marks + `color(fg?, bg?)` (§5.1) | --- |
| rich text `mention` (user/page/database/date) | `mention` mark per reference grammar | date mentions -> text + fidelity note |
| Comments (page/block anchored, `discussion_id` threads) | 0061 §7 annotations (`entity` anchor; block-anchored -> block-level anchor) | Notion's API exposes no range anchors; resolved comments are not listed by the API --- fidelity notes both |

Databases (owner decision 2026-07-04: databases are structures, kind `database`, §21; schema =
shared field definitions per 0061 §7.1 / JIRAISH §20.1):

| Notion property | Field type | Notes |
| --- | --- | --- |
| `title` | row-page title | --- |
| `rich_text` | `text` | --- |
| `number` | `number` | format -> display constraint |
| `select` / `multi_select` | `enum` / `list<enum>` | option ids/colors preserved |
| `status` | `enum` + workflow categories | option **groups map onto todo / in_progress / done** --- a direct category fit (JIRAISH §8) |
| `date` | `date` / `datetime` / `date_range` | `date_range` added for this (JIRAISH §20.1) |
| `people` | `list<principal>` | identity mapping + inactive placeholder principals (JIRAISH §25.3) |
| `files` | attachments (0061 §7.1) | --- |
| `checkbox` | `boolean` | --- |
| `url` / `email` / `phone_number` | `url` / `string` + format constraint | --- |
| `relation` (single/dual) | `list<entity_ref>` emitting typed edges | dual relations import both sides; pairing recorded |
| `rollup` / `formula` | frozen typed field, `imported_computed` (JIRAISH §20.1) | expression retained in fidelity + CAS |
| `unique_id` | 0061 §4 alias per structure (prefix preserved) | the §4 allocator generalizes beyond issues |
| `created_/last_edited_time/by` | operation-derived projections | synthesized operation timestamps, never stored fields |
| `place` / `verification` | `opaque_json`, flagged / governance note | --- |
| Views (API-exposed: full CRUD) | saved queries (0061 §7.1) + view definitions (0061 §8); board views over ticket-like databases use first-class Ticket Boards when rows lower to tickets | table/calendar/timeline/gallery/list convert through saved views; board grouping converts to Board columns over status/select fields; unconvertible filters/sorts stay flagged text, JQL-style; form/chart/map/dashboard -> fidelity-flagged |

Page-tree, teamspaces, permissions: teamspaces -> spaces; page parents -> tree edges; page-level
share grants -> nearest §11 grant scopes, differences reported. Wiki-database `verification` and
granular permission data are not API-exposed --- fidelity-listed.

### 5.7 Obsidian / Markdown-Tree Import Mapping (pinned 2026-07-04)

Build-focus requirement (ADOPTION §1.3); the general markdown-tree importer with Obsidian
extensions layered on. A vault is a folder of UTF-8 markdown plus `.obsidian/` config; the import
framework owns any archive container (zip/tar) per the ADOPTION §1.3 note --- Loom storage-frame
compression (0005) is not archive tooling and plays no role here.

| Vault element | Target | Notes |
| --- | --- | --- |
| Vault | space | --- |
| Folder | container page (owner decision): empty body, flagged synthetic; a folder note (`FolderName.md` or `index.md`) becomes its body | tree mirrors the folder tree |
| `.md` file | page; body parsed CommonMark + GFM + Obsidian extensions -> §9.1 blocks | file path -> §4 alias so links keep resolving |
| YAML frontmatter | shared field definitions (0061 §7.1): text->`string`, list->`list<string>`, number->`number`, checkbox->`boolean`, date(time)->`date`/`datetime`; types from `.obsidian/types.json`, else inferred | quoted `"[[wikilinks]]"` in values -> `entity_ref` |
| `aliases` key | §4 aliases on the page | `tags` key + inline `#tags` (incl. nested `a/b`) -> labels (0061 §7.1) |
| `[[Page]]`, `[[Page\|alias]]`, `[[Page#Heading]]`, `[[Page#^id]]` | reference grammar (0061 §19) `page:` references; heading/block targets carry the block anchor | unresolved links sequence with **late binding** --- forward links to never-created notes are normal in Obsidian, not errors; resolution tries bare-name, then relative, then root-absolute regardless of `app.json` |
| `^blockid` markers | the block's imported stable id; marker text dropped from content | id mapping recorded for `#^id` links |
| `![[Note]]` / `![[Note#Heading]]` / `![[Note#^id]]` | `block_ref` whole-body / `mode: section` (owner decision, 0061 §9.1) / `mode: block` | --- |
| `![[img.png\|640x480]]`, `![[Doc.pdf#page=3]]` | `attachment_ref` with size/page attrs | attachment folder read from `app.json`, never assumed |
| `> [!type]` callouts (incl. foldable `+`/`-`) | `callout(kind, folded?)` | unknown callout types -> `callout(note)` + original kind attr |
| `==highlight==` | `color(bg)` mark (§5.1) | --- |
| `%%comment%%` | `comment` kind (§5.1): preserved, never rendered | round-trips |
| GFM tables / task lists / footnotes / strikethrough | `table` / `list_item(task)` / footnote refs (registered attrs) / mark | --- |
| `$...$`, `$$...$$` | `equation` mark / block | --- |
| ` ```mermaid ` blocks | `code_block(mermaid)` | diagram-structure conversion is an offered elicitation, never automatic |
| `.canvas` files | `canvas` structure (§21) per JSON Canvas 1.0: text/file/link/group nodes with x/y/w/h payload; file nodes bind to imported pages/attachments (subpath -> block anchor); edges with sides/ends/labels/colors | groups stay spatial (membership is geometric in the source) |
| Dataview `key:: value`, ` ```dataview ` blocks; Tasks-plugin emoji | **verbatim text, fidelity-reported** (plugin-owned semantics) | an elicitation may offer field extraction; never automatic |
| Excalidraw files | attachments, flagged | plugin-owned scene JSON |
| `.obsidian/` config, themes, plugins, `organization.json` | not imported; fidelity-listed | `app.json` and `types.json` are read as import *inputs* only |

No history exists in a vault (files are current-state only): each page imports as one
`page.body_replaced` + publish with `import_provenance`; if the vault is a git repository, commit
history import is future work (recorded in §23), not assumed.

Current source backs the first Markdown execution slice through the reusable Markdown importer in
`loom-interchange-io` and `loom interchange import-markdown`. It walks a host directory for `.md`
files, derives deterministic page ids from relative paths, creates or reuses a pages space, lowers
CommonMark-style headings, paragraphs, unordered list items, ordered list items, task-list text,
quotes, dividers, and whole-page, heading-target, and block-target Obsidian embeds into canonical Body
blocks, publishes changed pages through the pages service, skips unchanged pages idempotently, and
emits the shared 0012 import report. Generic import execution batches can execute zip, tar, tar.gz,
and tar.zstd Markdown vault payloads through the same importer without consulting ambient host paths.
Folder-note files such as `Folder/Folder.md` and `Folder/index.md` use the folder identity instead of
creating duplicate folder-note pages.

The source-backed fixture at `specs/studio/fixtures/markdown/source/vault` is derived from
CommonMark, GitHub Flavored Markdown, Obsidian Help, and JSON Canvas 1.0. It imports into a clean
store and compares supported page content, folder-note identity, Obsidian page embeds, explicit
import-time `block_ref` preservation, archive execution, and fidelity issues against
`specs/studio/fixtures/markdown/expected/comparison.json`.
YAML/JSON properties, aliases, tags, wikilinks, Markdown links, autolinks, explicit block ids,
attachments, callouts, GFM tables/footnotes/strikethrough, equations, generic code fences, inline
code marks, HTML, highlights, Obsidian comments, diagrams, `.canvas`, Dataview/Tasks, Excalidraw, and
`.obsidian/` config semantics remain target work. Present but unsupported source constructs emit
shared fidelity issues.

#### 5.7.1 Markdown and Obsidian Current Coverage Matrix

The current Markdown/Obsidian fixture is a broad source-backed execution fixture for the first
production import slice. It is not a full native-projection acceptance suite: fields classified as
retained or degraded are preserved as page text or fidelity records, while native projections such
as aliases, labels, attachment objects, canvas structures, and plugin-owned semantics remain target
work.

| Source element | Current handling | Fixture coverage |
| --- | --- | --- |
| Vault root | Imported as Pages space | Covered |
| Folder | Target container page for empty folders | Covered as source input; not imported by current model |
| Folder note | Imported with folder identity | Covered |
| Markdown file path | Used for deterministic page id; alias target | Covered |
| Heading | Imported as heading block | Covered |
| Paragraph | Imported as paragraph block | Covered |
| Unordered list | Imported as list item | Covered |
| Ordered list | Imported as ordered list item | Covered |
| Task list | Imported as list text; checked state target | Covered as degraded |
| Quote | Imported as quote block | Covered |
| Divider | Imported as divider block | Covered |
| Whole-page Obsidian embed | Imported as block reference | Covered |
| Heading/block embeds | Imported as section/block references | Covered |
| Import-time `block_ref` fidelity | Expected fixture asserts native `BlockRef` count, target entity id, optional block id, and section mode | Covered |
| YAML/JSON properties | Fidelity warning; field mapping target | Covered |
| Aliases and tags | Present in properties and text; target page aliases and labels | Covered as source input; native mapping target |
| Wikilinks | Fidelity warning; reference mapping target | Covered |
| Markdown links and autolinks | Fidelity warning; reference/external-link mapping target | Covered |
| Explicit block ids | Fidelity warning; stable block-id mapping target | Covered |
| Attachments/images/PDF/canvas embeds | Fidelity warning; attachment and canvas mapping target | Covered |
| Callouts | Fidelity warning; callout block target | Covered |
| GFM tables | Fidelity warning; table block target | Covered |
| GFM strikethrough | Fidelity warning; inline mark target | Covered |
| Footnotes | Fidelity warning; footnote target | Covered |
| Equations | Fidelity warning; equation marks/blocks target | Covered |
| Generic code blocks, inline code, HTML | Fidelity warning; code and HTML source lowering target | Covered |
| Highlights and Obsidian comments | Fidelity warning; highlight and non-rendered comment targets | Covered |
| Mermaid and code blocks | Fidelity warning for Mermaid; code block lowering target | Covered |
| Canvas files | Fidelity warning; JSON Canvas structures target including text/file/link/group nodes, geometry, colors, and edges | Covered |
| Dataview and Tasks plugin syntax | Fidelity warning or retained text; structured extraction target | Covered |
| Excalidraw | Fidelity warning; attachment/plugin-scene target | Covered |
| `.obsidian/` config | Fidelity warning; attachment path, type metadata, and resolution policy target | Covered |

## 6. Operation Log

The source of truth for shared page organization coordination is an operation log, not raw branch sync.

Operation kinds:

```text
space.created
space.updated
space.archived
page.created
page.body_replaced
page.patch_applied
page.title_changed
page.moved
page.deleted
page.restored
page.published
draft.created
draft.updated
draft.discarded
comment.added
comment.edited
comment.redacted
inline_comment.added
inline_comment.resolved
attachment.added
attachment.removed
label.added
label.removed
watch.added
watch.removed
permission.changed
template.created
macro.updated
automation.created
automation.fired
audit.recorded
```

Each operation uses the 0061 §2 canonical envelope: the workspace is the page organization boundary,
`page_id` is the page entity id, `space_id` maps to `scope_id`, `base_root` records the previous
profile root, and `base_entity_version` records the prior page version. Body-delta operations
additionally use the 0061 §9.1 block-id envelope fields (requested-count or
`introduces_block_ids`).

The visible page tree, page body, comments, labels, search results, and audit views are projections over
operations.

## 7. Multi-Replica Coordination

Five laptops, each with a local Loom and an AI assistant, cannot safely coordinate edits to the same
space or page by independently editing the same workspace branch and relying on ordinary current sync.
Current sync moves objects and fast-forwards refs. It rejects divergent branch tips instead of choosing
or merging a winner. Current document storage also does not define per-document merge.

Pages therefore needs explicit coordination above raw branch sync.

### 7.1 Blind Central Operation Sequencer

The recommended enterprise default is a central operation sequencer that can be blind to page plaintext.

Flow:

1. A local editor or assistant creates an encrypted operation envelope and stores referenced payloads in
   its local Loom.
2. The local Loom submits opaque object labels, an idempotency key, base roots, and the operation envelope
   to Loom Cloud.
3. Loom Cloud verifies authorization by label, assigns the next page organization sequence, assigns any required
   page alias or page-tree order token, persists the opaque operation, advances the page organization root with
   compare-and-swap, and emits wakeups.
4. Other local Looms pull the operation and objects, decrypt locally, verify digests, update projections,
   and advance cursors.

This is a central mediator for ordering and delivery, but not a plaintext wiki server. It can safely
sequence opaque encrypted operations as long as it can authorize principals and validate protocol shape.

Properties:

- total order for space and page operations;
- stable page aliases and tree order tokens;
- simple user experience;
- durable replay by sequence;
- zero-knowledge compatible;
- no hosted search, summaries, macro evaluation, page export, or content classification unless a keyed
  worker is added.

### 7.2 Per-Actor Operation Logs

An alternative is to give each device or assistant its own operation log:

```text
pages/{page_id}/actors/{actor_id}/operations
```

Clients pull all authorized actor logs and deterministically derive the page view. The merge
function must define:

- operation identity and idempotency;
- causal parents or vector-clock metadata;
- page body handling --- pinned 2026-07-04: actor-log topologies run bodies in the degenerate
  whole-page mode with conflict records (§5.2); delta rebase requires the sequencer's total order,
  and a CRDT body remains explicitly deferred (0061 §9.1);
- page title and alias conflict rules;
- page-tree move and ordering rules;
- comment anchor behavior when edited text moves;
- draft versus published page semantics;
- macro side-effect rules;
- compaction rules that preserve page history and audit proofs.

Actor logs are appropriate for decentralized or offline-heavy page organizations. They are harder for users
because page order, published state, and comment anchors can be provisional until logs converge.

### 7.3 Keyed Central Page Organization Server

A keyed central page organization server is operationally closest to Confluence. It reads content, evaluates
macros, indexes search, renders exports, classifies pages, computes backlinks, summarizes content,
invokes hosted agents, and broadcasts updates.

This is compatible with Loom, but it is not zero-knowledge. It is appropriate when the tenant chooses
hosted compute inside a trust boundary.

### 7.4 Decision

Pages should support the blind central operation sequencer as the default shared-space
coordination model. Actor logs remain a target topology for decentralized or offline-heavy deployments.
A keyed central page organization server is a deployment option, not a requirement.

Raw current branch sync alone is not a sufficient coordination protocol for multiple local assistants
editing the same page tree.

## 8. Concurrent Editing Semantics

Pages must handle concurrent edits by operation type, page body format, and publication state.

### 8.1 Page Body Edits

Concurrent body edits follow the pinned model (§5.2, 0061 §9.1): the sequencer rejects a delta
whose base version is stale; the client maps its unconfirmed primitives through the intervening
deltas and resubmits. Persisted history is linear; the mapping function is deterministic,
canonical, and covered by conformance vectors. There is no automatic merge beyond this rebase ---
when a rebase cannot apply (target block deleted), the editor surfaces it rather than guessing.

In the degenerate whole-page mode (§5.2), concurrent whole-page saves from the same base revision
create a page conflict:

```text
Laptop A edits Roadmap from revision r10 and saves r11.
Laptop B edits Roadmap from revision r10 and saves r12.
The page projection keeps one revision as visible and creates a conflict record for the other.
```

The conflict record stores both candidate revisions, their base revision, authors, timestamps, and
resolution state.

### 8.2 Titles, Aliases, and Page Tree Moves

Title and move operations are metadata operations with explicit rules:

- two different pages with the same title under one parent receive deterministic aliases;
- moving a page while another actor edits content merges the move and content edit;
- deleting a page while another actor edits content creates a conflict record unless policy says delete
  wins;
- moving a page into a deleted parent is rejected or placed into an orphan review queue;
- page tree order uses sparse order tokens, not array positions.

### 8.3 Comments and Inline Comments

Comments append by operation sequence. Inline comments anchor per §5.3: `(block_id, byte range,
base revision, quoted text)` with deterministic mapping through subsequent deltas. If the anchored
content moves, the projection re-anchors by that mapping. If the anchor is deleted, shredded, or
ambiguous, the comment degrades to block- or page-level with a stale-anchor marker visible to
users, rendering its stored quote.

Comment edits keep version history. Comment redaction hides visible content while preserving audit facts
unless retention policy permits hard deletion.

### 8.4 Working Copies and Publishing

Working copies are per-principal or per-session artifacts. Publishing a working copy creates a page operation
against a base page version. If the base changed, the publish path either merges, asks for resolution, or
creates a conflict according to the body model and policy.

Autosave is draft state, not a published page revision. Published revision history remains explicit and
auditable.

### 8.5 Macros and Embeds

Macros are deterministic references to content-addressed definitions or trusted external integrations.
Macro execution must not affect canonical page identity unless the macro result is explicitly materialized
as page content. Content-aware macro execution requires keys and must run as an auditable principal.

## 9. Page Graph, Labels, and Search

Pages derives graph and search projections from page operations.

Graph edges:

```text
parent_of
links_to
mentions
embeds
includes
duplicates
supersedes
related_to
owned_by
reviewed_by
```

Labels are metadata operations. Concurrent label additions merge. Concurrent label removal and addition
are sequenced and auditable.

Search indexes and semantic embeddings are derived state. A blind Loom Cloud cannot compute them from
encrypted page content. Keyed local replicas or keyed workers must build those indexes.

## 10. Attachments, Templates, and Exports

Attachments are content-addressed Loom objects referenced from page operations:

```text
attachment_id
digest
name
media_type
size
uploaded_by
created_at_ms
scan_status
retention_class
```

Templates are versioned page bodies with parameter schemas. Creating a page from a template records both
the template version and the filled parameters.

Exports are derived artifacts. Exporting to PDF, HTML, Markdown, or archive formats requires a keyed
renderer if page content is encrypted. Export artifacts must record their source page revisions and
renderer identity.

## 11. Sharing and Permissions

Pages permissions apply to organizations, spaces, page trees, individual pages, comments,
attachments, templates, exports, macros, and automations.

Grant scopes:

```text
viewer
commenter
editor
publisher
space-admin
template-admin
macro-admin
automation-admin
agent-reader
agent-editor
```

Fine-grained policy must support:

- space-level browse and write rights;
- page-level restrictions;
- comment permissions;
- publish permissions;
- template management;
- macro management;
- export restrictions;
- external share restrictions;
- agent-specific grants independent from installer grants.

Revocation prevents new reads and writes, but cannot erase content already synced to a device. Stronger
revocation requires per-space or per-page encryption keys and key rotation.

## 12. Retention, Redaction, and Audit

Deleting a page moves it to a deleted or archived state by default. It does not immediately remove page
history, comments, attachments, backlinks, or audit facts.

Hard deletion is a policy operation:

- remove live references from page and tree projections;
- remove or expire search, graph, report, export, and vector indexes;
- retire content encryption keys when crypto-shredding is allowed;
- mark old roots outside the retention live set;
- let garbage collection reclaim unreachable objects after the retention window.

Legal hold overrides hard deletion. Page retention, comment retention, attachment retention, export
retention, audit retention, and account deprovisioning are separate policy inputs.

## 13. Background Workers

Required workers:

- **Sync worker:** resumes transfers, verifies roots, applies operations, and backfills missing payloads.
- **Projection worker:** maintains page trees, current page bodies, comments, labels, backlinks, and
  permissions.
- **Search worker:** indexes full-text and semantic page content where keys are available.
- **Graph worker:** computes backlinks, mention graphs, include graphs, and related-page suggestions.
- **Render worker:** produces page previews and exports where keys are available.
- **Automation worker:** runs trigger-bound page automation under scoped principals.
- **Attachment worker:** scans files, extracts text, computes previews, and expires unused uploads.
- **Retention worker:** applies archive, legal hold, key retirement, and deletion policy.
- **GC worker:** reclaims unreachable objects after policy permits it.
- **Notification worker:** converts page organization root advancement into MCP and WebSocket wakeups.

Keyless workers can operate only on labels, sizes, encrypted frames, and visible metadata. Content-aware
workers require keys and must run as auditable principals.

## 14. MCP as the Primary Protocol

Expose page state as MCP resources:

```text
loom://{workspace}/pages
loom://{workspace}/pages/space/{space_id}
loom://{workspace}/pages/{page_id}
loom://{workspace}/pages/{page_id}/history
loom://{workspace}/pages/{page_id}/comments
loom://{workspace}/pages/label/{label}
loom://{workspace}/pages/template/{template_id}
loom://{workspace}/pages/search/{filter_id}
```

Expose operations through MCP tools:

```text
spaces.create
spaces.get
spaces.list
pages.create
pages.get
pages.update
pages.publish
pages.move
pages.delete
pages.restore
pages.history
pages.restore_revision
pages.comment
pages.redact_comment
pages.resolve_inline_comment
pages.add_attachment
pages.add_label
pages.remove_label
templates.create
exports.run
watches.add
cursors.update
```

`pages_update` creates or updates the caller's page working state from UTF-8 `body_text` at the
MCP and IDL contract boundary. Current source converts that text into the canonical Page Body model
before storage; the Page profile still stores canonical body bytes internally, and `pages_get`
projects `body_text` and `draft_body_text` for agent-readable text access. Draft is a page status,
not a separate top-level tool group or content type. `pages_publish` publishes that working state
against its recorded base revision. Current source also accepts optional `expected_root` on
`pages_publish`, rejects stale profile roots with `CONFLICT`, returns `profile_root`, and maintains
the reserved substrate revision index for successful publishes. Page space creation, page creation,
page draft updates, and page publishes also append canonical page operation-log records. MCP
`substrate_changes` can replay those records with `oplog:<next-sequence>:pages:<organization-id>`
cursors.

All write tools execute as the resolved principal and are checked by the policy enforcement point.

## 15. Agent Callbacks and Subscriptions

Agents subscribe to spaces, page trees, individual pages, labels, comment queues, review queues, or
automation fire logs.

Example subscription:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "resources/subscribe",
  "params": {
    "uri": "loom://organization/acme/pages/main/space/engineering"
  }
}
```

When the space projection changes, the MCP server emits a resource update notification. The agent then
fetches operations from its durable page organization cursor:

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/call",
  "params": {
    "name": "pages.search",
    "arguments": {
      "space_id": "engineering",
      "after_sequence": 56410
    }
  }
}
```

The notification is a wakeup, not the source of truth. Durable delivery uses page organization sequence cursors.

## 16. Elicitation

MCP elicitation is used when an agent or server needs structured input before proceeding.

Use elicitation for:

- resolving a page conflict;
- choosing whether to publish a draft;
- approving an agent-authored page update;
- selecting a target space or parent page;
- confirming page deletion or archive;
- deciding whether to re-anchor an inline comment;
- approving an export of restricted content;
- selecting labels or owners for generated documentation.

Example:

```json
{
  "jsonrpc": "2.0",
  "id": 7,
  "method": "elicitation/create",
  "params": {
    "message": "Resolve concurrent edits to the Roadmap page.",
    "requestedSchema": {
      "type": "object",
      "properties": {
        "choice": {
          "type": "string",
          "enum": ["keep-current", "keep-conflict", "merge-manually"]
        },
        "publish": { "type": "boolean" }
      },
      "required": ["choice"]
    }
  }
}
```

Elicitation responses become durable operations when they affect page organization state.

## 17. WebSocket Secondary Transport

WebSocket can be offered for editor presence, typing state, cursor hints, page-tree UI fanout, and
high-frequency notifications. It must preserve the same semantics as MCP:

- authenticated principal context;
- authorized space, page, comment, and label subscriptions;
- durable page organization sequence cursors;
- idempotent writes;
- replay after reconnect;
- no stronger write authority than MCP tools.

Editor cursor hints and typing state are ephemeral. They are not the source of truth and do not replace
durable page operations.

## 18. Performance Requirements

The design must meet these requirements:

- editing one page does not rewrite the whole space;
- comments and audit records append without rewriting page history;
- page-tree moves are logarithmic or better in the tree index;
- listing a large space is paginated and index-backed;
- search and backlinks are projection-backed, not full-log scans;
- page body merge is deterministic when automatic merge is supported;
- sync transfers only missing objects and metadata nodes;
- local page views converge after replay;
- blind cloud mode remains usable for sequencing, coordination, and wakeup without content access.

## 19. Open Design Decisions

These choices must be pinned before implementation:

1. The canonical operation envelope and payload encoding. **Resolved:** 0061 §2.
2. The page operation sequencer protocol and replay guarantees. **Resolved:** 0061 §3.
3. The rich page body format and canonical normalization rules. **Resolved 2026-07-04:** 0061 §9.1
   block document; page kinds in §5.1.
4. The collaborative edit operation or CRDT model. **Resolved 2026-07-04:** sequenced deltas with
   reject-stale-base client rebase; CRDT explicitly deferred (0061 §9.1, §5.2, §5.5).
5. The page alias and title collision rules.
6. The page-tree order token algorithm and compaction rules. **Resolved:** 0061 §5.
7. The inline comment anchoring and re-anchoring rules. **Resolved 2026-07-04:** §5.3.
8. The macro execution and materialization boundary.
9. The SQL, graph, search, and vector projection canonical layouts.
10. The retention live-root algorithm for page deletion and archive (body revision shred itself is
    pinned: publish-epoch keys, 0061 §9.1).
11. The conformance vector set for create, edit, publish, move, comment, link, macro, export, and cursor
    replay (body-model vectors pinned in 0061 §9.1; import vectors per §5.4).

## 20. Recommended Shape

All contracts here are long-term decisions; staging is an implementation concern that never changes
a contract (rewritten from "Recommended v1" during the 2026-07-04 body-model session, per the
Queue 8 supersession decision):

- local-first Loom replicas hold organization keys and can see the full page organization;
- Loom Cloud is a blind sync, operation sequencing, coordination, and notification replica by default;
- a separate keyed compute deployment is used when hosted search, rendering, summaries, macros, DLP, or
  automation are required;
- page state is an operation-log projection, not raw current branch sync;
- page bodies are the 0061 §9.1 block document with sequenced deltas and publish-epoch snapshots
  (§5); implementations may stage delivery by shipping the degenerate whole-page mode first, but
  the contract is the delta model from day one;
- comments, audit, and attachments append by operation;
- page tree order uses sparse order tokens, not array positions;
- MCP resources and subscriptions are the primary agent callback mechanism;
- WebSocket mirrors the same event and cursor contract for clients that need low-latency editor hints;
- deletion is archive by default, with hard deletion handled by retention, key retirement, and GC
  (body revisions shred by publish-epoch key, §5.2).

## 21. Structured Thinking Kinds

Pages must model more than page trees. Pages belong to projects (a `belongs_to` edge to a
Jiraish project or a standalone page project), and spaces contain *structures* --- typed,
graph-backed containers for different kinds of thinking. A structure is not a page with a diagram
pasted in; it is data that many surfaces render.

```text
structure       structure_id, space_id, kind, title, root node ref
kind            mindmap | outline | decision_tree | canvas | database |
                diagram(flowchart | sequence | architecture)
node            node_id, structure_id, kind-specific payload (label, body digest, entity ref),
                parent/position semantics per kind
node edge       typed graph edges: child_of, links_to, refers_to(issue | page | file | message)
```

Rules:

- **Structures are graph projections.** Nodes are graph nodes; hierarchy and cross-links are typed
  edges; node bodies version like any entity (0061 §9). The mind map, the force-directed graph, an
  outline view, and an exported diagram are different renderers over the same operations --- an AI
  edit to the data updates every rendering, which is the point.
- **Databases are structures** (owner decision 2026-07-04, Notion session): kind `database` --- a
  typed table whose **schema is the shared field-definition facility** (0061 §7.1; JIRAISH §20.1
  is the defining text --- one type system, one validation path, one vector set with issue custom
  fields) and whose **rows are pages**: each row carries field values plus a full §5 body, parented
  by the database structure. Views over a database --- table, board, calendar, timeline, gallery,
  list --- are saved queries (0061 §7.1 predicate trees) plus view definitions (0061 §8) carrying
  rendering configuration: data, never code. Relations between databases are `list<entity_ref>`
  fields emitting typed edges; imported rollups/formulas freeze per JIRAISH §20.1
  `imported_computed` (native computed columns are recorded future work, §23). Inline databases on
  pages render through `structure_embed` (§5.1). This is the database-view surface contract the
  Notion-surface decision requires.
- **Diagrams bind, they don't embed.** A diagram shape may bind to an entity (issue, page, service);
  bound shapes re-render from entity state. Unbound decoration is view-local app state, not
  operations.
- **Decomposition is a first-class flow.** "Mind map a new project, explain the first five
  features" produces: a structure (kind: mindmap), feature nodes, then pages elaborating each node
  (`refers_to` edges), then issues decomposed from pages (`implemented_by` edges into Jiraish).
  Every step is operations, so the page graph, roadmap, and board light up as the thinking
  progresses --- this ordering (think -> document -> decompose -> track) is the intended arrival path
  for new work, with the traditional page tree as just one structure kind among several.
- **Templates apply to structures** as well as pages: a "new project" template can instantiate a
  mindmap skeleton, a spec page per branch, and a DoR checklist in one templated transaction.

## 22. Example Tool Surface (illustrative only --- not a design decision)

Names, grouping, and parameters are examples to make assistant ergonomics concrete, not designed
contracts. Underscore-flattened per MCP; capability `pages`.

| Category | Tool | Description |
| --- | --- | --- |
| Spaces | `spaces_create` | Create a space, optionally bound to a project |
| Spaces | `spaces_get` | Fetch space metadata and tree root |
| Spaces | `spaces.archive` | Archive a space |
| Pages | `pages_create` | Create a page with stable `page_id`; title/location are aliases |
| Pages | `pages_get` | Fetch a page body and metadata; accepts version or root (0061 §9) |
| Pages | `pages_update` | Replace/update a page body from UTF-8 `body_text` against a base revision |
| Pages | `pages.patch` | Guarded partial edit; conflict record on base mismatch |
| Pages | `pages.move` | Move within the tree; order tokens, identity unchanged |
| Pages | `pages.delete` | Archive-by-default delete |
| Pages | `pages.restore` | Restore an archived page |
| Pages | `pages_history` | Revision index rows for a page |
| Pages | `pages.restore_revision` | Make a prior revision the new head (new operation, history intact) |
| Pages | `pages_publish` | Publish the caller's page working state against a base revision; conflict per §8.4 |
| Annotations | `comments.add` | Append a page comment (0061 §7) |
| Annotations | `comments.redact` | Redact; audit fact persists |
| Annotations | `inline_comments.add` | Range-anchored comment; re-anchoring per §8.3 |
| Annotations | `inline_comments.resolve` | Resolve an inline comment |
| Metadata | `labels.add` | Add labels (concurrent adds merge) |
| Metadata | `labels.remove` | Remove a label (sequenced, auditable) |
| Metadata | `attachments.add` | Attach content-addressed file |
| Templates | `templates.create` | Versioned template with parameter schema |
| Templates | `templates.instantiate` | Create pages/structures from a template in one transaction (§21) |
| Structures | `structures_create` | Create a structure: mindmap, outline, decision_tree, canvas, diagram (§21) |
| Structures | `structures_get` | Fetch structure nodes and edges for rendering |
| Structures | `structures_add_node` | Add a node with kind-specific payload |
| Structures | `structures_move_node` | Reparent/reposition a node |
| Structures | `structures_update_node` | Update node label/body (versions like any entity) |
| Structures | `structures_link_node` | Typed cross-link from a node to any entity |
| Structures | `structures_bind` | Bind a node/shape to a ticket, page, or file; bound shapes re-render from entity state |
| Structures | `structures_decompose_to_tickets` | Create tickets from selected nodes with `implemented_by` edges (§21) |
| Discovery | `pages.search` | Domain search over pages/structures |
| Discovery | `backlinks.get` | Inbound references for a page or node |
| Discovery | `exports.run` | Render export (PDF/HTML/Markdown) recording source revisions |
| Discovery | `watches.add` | Watch a page, tree, or label |
| Cursors | `cursors.update` | Advance the principal's durable cursor |

## 23. Unfinished Tasks (pushed back from Queue 8)

`specs/0061.md` now owns the shared substrate: operation envelope, sequencer protocol, cursors,
order-token library, conflict record schema, annotation subsystem, view/projection machinery,
cross-facet search, and --- as of the 2026-07-04 body-model session --- the rich body model and body
deltas (0061 §9.1, promoted from this spec by owner decision, overriding the earlier "uniquely
Pages" push-back). Open decisions 1, 2, 3, 4, 6, 7, and 11 in §19 are resolved (see §19
for pointers). The following remains uniquely Pages and is unowned by any queue:

- Current source backs the reusable page organization model layer in `loom-substrate::pages` and the
  store-backed service boundary in `loom-pages`:
  spaces, pages, drafts, published revisions, revision body digest validation, and stale-base publish
  conflict records. The model is feature-gated by `studio-pages` and uses canonical CBOR for
  the source-backed records. Current source also backs durable MCP and hosted REST/JSON-RPC verticals
  for `spaces_create`, `spaces_get`, `spaces_list`, `pages_create`, `pages_update`,
  `pages_publish`, `pages_get`, and `pages_history` over `FileStore` control records. The top-level
  `loom pages` CLI source-backs the same space and page vertical for space create/list/get, page
  create/update/publish/get, and page history over the resolved workspace id. Page working state is
  surfaced as page status. Hosted listeners use separate `spaces`, `pages`, and `structures` surfaces
  with the Pages workspace id as the listener collection.
  Structures are source-backed for `structures_create`, `structures_get`, `structures_add_node`,
  `structures_update_node`, `structures_move_node`, `structures_link_node`, `structures_bind`, and
  `structures_decompose_to_tickets`; MCP, hosted REST/JSON-RPC, and the top-level `loom pages` CLI
  cover the same source-backed structure vertical over the shared `loom-pages` service boundary.
  Node, edge, move, and decomposition writes also update the graph-facet render projection.
  Database structures carry
  canonical `field_ids` that point at the
  shared 0061 §7.1 / JIRAISH §20.1 field-definition facility, so Pages database structures and
  ticket custom fields use one type system and one validation path. Page-tree projection workers,
  template schemas, importers, and broader profile-specific field-definition UX remain target work.
- Page alias and title collision rules (§8.2, §19.5) as profile policy over the 0061 §4 allocator.
- The macro execution and materialization boundary (§8.5, §19.8), including the activation path
  for imported `macro_reference` nodes (§5.4).
- Template parameter schemas (§10) and page/space projection canonical layouts (§19.9).
- **Import (requirement, ADOPTION §1.3):** the body mapping table is pinned (§5.4, both source
  formats with equal commitment and full vectors each). Remaining: page trees, labels, users ->
  principals, URL alias preservation, coexistence bridge semantics, and the fidelity report
  format beyond bodies.
- **Template and structure authoring experience (ADOPTION G2 pattern):** authoring flows for
  templates and structure kinds with dry-run validation and elicitation-based parameter editing.
- Implementation of the 0061 §9.1 model for pages: delta primitives, rebase loop, epoch keys,
  anchor mapping, and the §5.4 importers with their conformance vectors (design pinned; build
  unowned).

- **Additional import sources:** ~~Notion~~ and ~~markdown tree / Obsidian~~ --- **mapping tables
  pinned 2026-07-04 in §5.6 and §5.7** (prompt-5 session); importer implementations remain.
  Still candidates: Redmine wikis (requirement per ADOPTION §1.3; mapping table not yet written);
  MediaWiki/XWiki (legacy enterprise, macro degradation fidelity-reported).
- ~~Block transclusion~~ --- **resolved (owner, 2026-07-04, prompt-5 session):** `block_ref`
  semantics pinned in 0061 §9.1 (no content caching / shred-safe placeholder, optional pin,
  render-time cycle+depth enforcement, index-once-at-source, `transcludes` edges, `section` mode
  for Obsidian heading embeds). Current source backs the shared body renderer's read-through,
  missing/shredded placeholder, cycle, depth-limit behavior, canonical body decode, and page-publish
  `transcludes` edge projection into `substrate_refs`; `pages_get` now renders published and draft
  canonical page bodies through `block_ref` read-through. Import-time vectors are source-backed:
  Markdown/Obsidian fixtures assert native `BlockRef` lowering, Notion fixtures assert synced-block
  fidelity reporting, and Confluence fixtures assert opaque storage/ADF body retention rather than
  invented references.
- **Native computed database columns (2026-07-04, from the Notion import):** imported
  formulas/rollups freeze per JIRAISH §20.1 `imported_computed`; a native formula/rollup column
  facility for §21 databases (expression language, recompute semantics, 0015 determinism) is
  recorded future work, unowned.
- **Vault git-history import (2026-07-04):** §5.7 imports current state only; synthesizing page
  history from a vault's git log is future work, unowned.
- **Coexistence bridge as mirror mode (from the JIRAISH session's substrate lift):** frame the
  Confluence coexistence bridge (ADOPTION §1.3) as a 0061 §7.1 scope operating mode (read-only
  mirror during migration, then cutover), matching JIRAISH §25's import model.
