# 0003d - Cross-Facet Commit Diff

**Status:** Partial, core and local public projection source-backed. **Version:** 0.1.0-draft.
**Normative only after promotion.**

This sub-spec owns the target uniform commit-to-commit diff contract that was split out of 0003b.
Current source provides path-level file diffs, table diffs, per-facet helper diffs for selected
facets, a local core `diff_commits` walker that emits the canonical envelope below, and local public
projection through the public `diff` operation in the IDL, C ABI, CLI, and current binding families.
Executable conformance covers
files, SQL rows, queue appends, CAS digests, and foreign-commit rejection. Wider facet vectors,
hosted projection, and conformance-report projection are still target work.

0042's collection/unit addresses are reconciled for the current promoted facets in this document. The
next implementation slices should pin wider vectors against this envelope. Hosted or untrusted
projection still waits on ACL-scoped unit presentation.

## Current Source Boundary

Source-backed today:

- file path-level diff between two commits through the workspace-history surface;
- SQL table diff through the table-history reader surface;
- per-facet helper diffs in source for calendar entries, contacts entries, and mail messages;
- `Loom::diff_commits(ns, from, to)` in `loom-core`, returning the `LMDIFF` canonical CBOR envelope
  for files, SQL rows, KV keys, document ids, queue appends, CAS digests, calendar entries, contacts
  entries, mail messages or flags, and coarse sections for whole-blob facets;
- local projection through the public `diff` operation in `idl/loom.idl`, the C ABI and generated
  header, CLI, Node, Python, C++, iOS/Swift, JVM, Android, React Native, and wasm;
- source-backed `vcs` ACL gating for workspace-history operations, including `diff_commits`;
- ACL-scoped presentation in core: a caller with `vcs` `Read` but without the changed facet's `Read`
  receives an opaque facet-level roll-up, not collection paths, unit ids, row keys, file paths, or
  payload digests for that facet;
- executable conformance for the local `diff_commits` envelope over files, SQL rows, KV keys,
  document ids, queue appends, CAS digests, calendar entries, contacts entries, mail flags, coarse
  whole-blob fallback, and foreign-commit rejection;
- executable conformance for the promoted facade runners that prove each selected facet versions,
  clones, and checks out through the workspace tree.

Not implemented today:

- (P1) public `diff` projection through hosted protocols and conformance reports;
- (P1) sublinear unit-level diff for facets still stored as whole canonical blobs;
- (P1) Hosted protocol projection and conformance-report projection.

## Target Contract

Per invariant 0001 A6, every promoted facet should store committed state keyed by its natural unit, and a
commit-to-commit diff should report changes at that unit rather than only reporting that an opaque blob
changed. The target public operation is:

```idl
// Structural delta between two commits, grouped by facet, at each facet's natural unit.
bytes diff(LoomSession session, string workspace, Digest from_commit, Digest to_commit);
```

The result reports, per facet and collection, units that were `added`, `removed`, or `changed`. A
rename or move is represented as a changed key or path unless the owning facet promotes a richer move
record. Append-only facets report appended units.

The target storage expectation is structural sharing: when a facet has a unit-addressed prolly tree,
the diff walks only differing subtrees, and an older commit's unit state remains directly traversable.
Facets that still store one whole canonical blob cannot claim this target granularity until their
structured-storage promotion lands.

## Canonical Envelope

The canonical result is Loom Canonical CBOR. The top frame is an array with fixed field order:

```text
[
  "LMDIFF",
  1,
  workspace_id,
  from_commit,
  to_commit,
  facet_sections
]
```

`workspace_id` is the 16-byte workspace UUID. Commit ids are digest values encoded with the ordinary
digest codec. `facet_sections` is sorted by `facet`, then contains one section per changed facet:

```text
[
  facet,
  collection_sections
]
```

`facet` is the `FacetKind::as_str()` value. `collection_sections` is sorted lexicographically by
`collection_path` after segment encoding:

```text
[
  collection_path,
  summary,
  unit_changes
]
```

`collection_path` is an array of text segments from 0042. CAS uses an empty array because current
source stores digests directly under the CAS facet root. `summary` is:

```text
[
  added_count,
  removed_count,
  changed_count,
  appended_count,
  coarse
]
```

`coarse` is `true` when the implementation can prove only that the collection blob changed, not which
unit changed. A coarse collection section has an empty `unit_changes` array. `unit_changes` is sorted
by `(unit_kind, unit_key, change)`:

```text
[
  unit_kind,
  unit_key,
  change,
  before,
  after,
  detail_kind,
  detail
]
```

Field meanings:

- `unit_kind`: text naming the facet unit class, such as `path`, `row`, `key`, `document`, `point`,
  `vector`, `node`, `edge`, `entry`, `digest`, `event`, `contact`, `message`, or `flags`.
- `unit_key`: Loom Canonical CBOR bytes for the facet's normalized unit key. Text ids are encoded as
  text values, SQL row keys as arrays of tabular values in primary-key order, KV keys as the canonical
  typed key value, timestamps as signed integers, stream and ledger sequence numbers as unsigned
  integers, and CAS digests as digest values.
- `change`: one of `added`, `removed`, `changed`, or `appended`.
- `before` and `after`: optional digest values for the canonical unit payload before and after the
  change. For append-only additions, `before` is absent. For removals, `after` is absent. For coarse
  sections, no unit record is emitted.
- `detail_kind`: `none`, `bytes`, `text`, `fields`, `cells`, `flags`, or a facet-owned text tag.
- `detail`: optional canonical detail payload. The first implementation may use `none` and leave
  derived byte, line, field, and cell details for later vectors.

This array shape is the identity-affecting envelope. JSON and bridge projections may use object field
names, but they must preserve the same information and ordering.

## Unit Granularity

The diff unit is addressed within its collection path from 0042 as
`facet.<collection-path>.<unit>`.

| Facet | Collection | Unit | Example summary |
| --- | --- | --- | --- |
| files | folder | path | `files: +2 added, 1 modified` |
| sql | database > table | row by primary key | `sql.sales.orders: 3 rows inserted, 1 updated, 2 deleted` |
| kv | map | key | `kv.settings: 1 key changed, 1 added` |
| document | collection | document id | `document.people: 1 updated, 1 added` |
| time-series | series-set | series plus timestamp point | `time-series.metrics.cpu: 96 points added` |
| vector | collection | vector id | `vector.docs: 12 added, 3 updated, 1 removed` |
| columnar | dataset | target row ordinal or sealed segment | `columnar.events: +1 segment` |
| graph | graph | node or edge | `graph.kg: +5 nodes, -1 node, +8 edges` |
| cas | implicit store | digest | `cas: 2 added, 1 unreferenced` |
| queue | stream | appended entry | `queue.events: 4 appended` |
| ledger | log | appended entry | `ledger.audit: 1 appended` |
| calendar | principal > collection | entry UID | `calendar.alice.work: 1 event updated, 1 added` |
| contacts | principal > address book | contact UID | `contacts.alice.personal: 1 contact updated, 2 added` |
| mail | principal > mailbox | message UID; flags separate | `mail.alice.inbox: 3 messages ingested; flags: 12 changed` |

## Facet Readiness

This table defines what Priority 2 may implement without overclaiming source support:

| Facet | Current source address | Priority 2 diff level |
| --- | --- | --- |
| files | workspace paths | path unit changes through existing path diff |
| sql | `.loom/facets/sql/<db>/tables/<table>` structured table | row unit changes through existing table diff |
| kv | `.loom/facets/kv/<collection>` whole map blob | derived key changes by decoding both blobs; coarse if decode fails |
| document | `.loom/facets/document/<collection>` whole collection blob | derived document-id changes by decoding both blobs; coarse if decode fails |
| queue | structured stream entry map keyed by sequence | appended entry changes by sequence |
| calendar | `.loom/facets/calendar/<principal>/<collection>/<uid>` | entry UID changes through helper diff |
| contacts | `.loom/facets/contacts/<principal>/<book>/<uid>` | contact UID changes through helper diff |
| mail | `.loom/facets/mail/<principal>/<mailbox>/msg/<uid>` and `flags/<uid>` | message UID and flags changes through helper diff |
| ledger | `.loom/facets/ledger/<collection>` whole ledger blob | derived appended entries by decoding both blobs; coarse if non-append divergence |
| time-series | `.loom/facets/time-series/<collection>` whole series blob | derived point changes by decoding both blobs; coarse if decode fails |
| cas | `.loom/facets/cas/<digest>` | digest added or removed from workspace reachability |
| graph | `.loom/facets/graph/<collection>` whole graph blob | coarse collection change until public graph facade and structural storage promote |
| vector | `.loom/facets/vector/<collection>` whole vector-set blob | coarse collection change until public vector facade and structural storage promote |
| columnar | `.loom/facets/columnar/<dataset>` whole dataset blob | coarse collection change until public columnar facade and structured segment storage promote |

## Derived Sub-Unit Views

The unit level is committed state. Finer views are derived on demand:

- Files may derive byte, line, or word diffs from before and after blobs.
- Structured facets may derive field or cell diffs from before and after records.
- Immutable or append-only facets such as CAS, queue, and ledger have no finer meaningful unit.

## Authorization and ACL-Scoped Presentation

Public `diff` is a `vcs_*` history operation, not a free-standing read of every facet. The first PEP
decision is whether the caller may run the VCS diff for the workspace. A caller without that permission
gets `PERMISSION_DENIED` before seeing commit messages, parent ids, changed-unit ids, or aggregates. A
caller with that permission can see the commit metadata for the operation, including messages.

The stored diff result uses fully qualified unit ids. After the VCS gate passes, the presentation layer
filters or rolls up unit payload details according to the viewer's owning-facet grants. A caller with
read access to a unit sees the fully qualified unit and any derived sub-unit diff. A caller without read
access to that unit sees no unit detail, or only an opaque rolled-up count when the enclosing scope
permits that aggregate. The stored truth is not coarsened to avoid leakage; the policy enforcement
point controls presentation.

## Promotion Requirements

- (P1) Complete - pin vectors for files, SQL, KV, document, queue, CAS, calendar, contacts, mail, and
  coarse whole-blob fallback using the canonical envelope above.
- (P1) Keep the core `diff_commits` walker aligned with those vectors as additional facet unit
  walkers promote.
- (P1) Promote structured-storage diffs facet by facet; whole-blob facets may report only path/blob
  changes until their owner promotes unit-addressed storage.
- (P1) Project the ACL-scoped presentation behavior through hosted protocols and conformance reports.
- (P1) Project the API through hosted protocols and conformance reports only after authorization policy
  and wider conformance vectors are stable.

## Proposed implementation phases

| Phase | Priority | Work | Dependencies | User effort |
| --- | --- | --- | --- | --- |
| 1 | P1 | Complete - 0042 addresses are reconciled here and the canonical envelope is pinned. | 0042, owning facet specs. | No. |
| 2 | P1 | Complete - local core walker emits the canonical envelope for files, SQL, queue, CAS, selected path-addressed facets, and coarse whole-blob facets. | Phase 1. | No. |
| 3 | P1 | Complete - executable conformance pins files, SQL, KV, document, queue, CAS, calendar, contacts, mail, coarse whole-blob fallback, and foreign-commit rejection. | Phase 2. | No. |
| 4 | P0 | Complete - local core `diff_commits` applies the `vcs` gate and rolls up unauthorized facets. | 0026, 0027, 0027a, 0028. | No. |
| 5 | P1 | Complete for local projection - public `diff` in IDL, C ABI, CLI, Node, Python, C++, iOS/Swift, JVM, Android, React Native, and wasm exposes the raw `LMDIFF` envelope. Hosted protocols and conformance reports remain target. | Phase 4 for hosted exposure. | No after policy is settled. |
