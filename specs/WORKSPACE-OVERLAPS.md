# Workspace overlaps & multi-view facets - exploratory note

**Status:** Exploratory / design note. **Version:** 0.1.0-draft. **Normative?** No.
**Relates to:** 0014 (workspaces), 0013 (facet catalog), 0011 (sql), 0016 (graph), 0017 (vector),
0019 (kv), 0020 (document), 0021 (append-log), 0022 (time-series), 0023 (columnar), 0024 (cas),
0018 (ledger), 0015 §derived-views/triggers.

Started from the observation that `files` and `vcs` overlap (`files` is the linear subset of `vcs`).
That is not unique: several workspace types are **specializations**, **derived views**, or **shared
substrate** of one another. This note catalogs those relationships and raises one design question -
whether Loom should let a single dataset carry **multiple facet views** (e.g. `sql` and `search` over
the same rows) rather than forcing a copy per workspace.

## 1. Three kinds of overlap

- **Specialization (a subset of a richer type).** Same substrate, a restricted command set.
  - `files` ⊂ `vcs` - the linear, no-branch/merge subset (the Time Machine view of a repo). Already
    first-class in 0014.
- **Derived view / index (a second representation of the same data, kept in sync, rebuildable).**
  One dataset, several read shapes; the secondary shape is derived and could be rebuilt from the
  primary, exactly like the vector index is derived from the vectors (0017 §3).
  - `columnar` is the OLAP, read-optimized view of `sql`'s OLTP rows (0023 already calls itself "the
    read-optimized counterpart to the relational row store").
  - a full-text `search` index (`tantivy`, the hybrid BM25 leg from LEANN) is a derived view over
    `sql` rows or `document` bodies.
  - `time-series` rollups are `columnar` derived views (0022).
  - `vector` embeddings are a derived semantic view of any text-bearing facet.
- **Shared substrate (the floor everything stands on).**
  - `cas` (0024) is raw put/get over the content-addressed object store - the layer every other facet
    is built on. Every facet "overlaps" `cas` by being structure over it.

## 2. Relationship map

| A | relationship | B | nature | notes |
| --- | --- | --- | --- | --- |
| `files` | subset of | `vcs` | specialization | linear vs full DAG (0014) |
| `kv` | degenerate | `sql` | specialization | a single-key→value table; `sql` adds schema + columns |
| `document` | ≈ | `kv` + indexes | specialization | JSON/CBOR values keyed by id, with secondary indexes (0020 → 0011) |
| `document` | queryable as | `sql` | derived view | SQL over fields extracted from documents |
| `search` (tantivy) | index over | `sql` / `document` | derived view | full-text/BM25; the hybrid dense+BM25 leg |
| `columnar` | OLAP view of | `sql` | derived view | row store (OLTP) vs column segments (OLAP), same data (0023) |
| `time-series` | specialization of | `append-log` + `sql` | both | ordered-by-time append; range queries; rollups are `columnar` |
| `time-series` rollups | derived view of | `columnar` | derived view | aggregates as segments (0022) |
| `ledger` | = | `append-log` + hash chain | specialization | append-only log plus an integrity chain (0018) |
| `queue` / `append-log` | overlap | `ledger` | shared shape | both are ordered append streams |
| `vector` | linked with | `graph` | dual-use | nodes hold vector ids; semantic recall + relationships (0017 §7) |
| any text facet | source for | `vector` / `search` | derived view | embeddings / full-text indexes are derived |
| everything | built on | `cas` | substrate | the content-addressed floor (0024) |

## 3. The design question: one dataset, multiple facet views

Today each workspace is an **independent** typed tree (0014 §3). The overlaps above suggest a second
mode: a **view relationship**, where one workspace is a *derived index/representation* of another's
data and is kept in sync (or rebuilt) rather than holding an independent copy. The user's example -
`sql` and `search` as two views over the same rows - is the canonical case, and `columnar`-over-`sql`
and `vector`-over-text are the same idea.

Two ways to support it:

- **(a) Independent workspaces, derived by a program/trigger.** Keep 0014 as-is; a derived view
  (`search`, `columnar`, `vector`) is produced from a source workspace by a derivation program
  (0015 §derived-views) that re-runs on change (0029). Simple, already on the roadmap; the cost is the
  derived data is a separate workspace the author wires up.
- **(b) First-class "view" workspaces.** A workspace can be declared a *view* of a source workspace +
  a transform, so the engine maintains it automatically and the registry records the link. More
  powerful (one logical dataset, many query shapes) but a real addition to the workspace model
  (lifecycle, capability scoping across the link, what syncs).

This connects directly to decisions already made: the vector index is *already* a derived,
rebuildable, non-synced artifact (0017 §3), and `columnar` is *already* framed as a view of `sql`. So
the "derived view" pattern is implicitly present; the open question is whether to make it an explicit,
uniform mechanism.

## 4. Implications to weigh

- **Sync (0006, 0032):** does a derived view sync, or rebuild on the receiver like the vector index?
  Default should match the vector decision - **derived views do not sync; they rebuild from the
  synced source** - so there is no view-divergence-over-sync and the platforms agree.
- **Determinism:** a view must be a deterministic function of its source (so every peer/platform
  rebuilds the same view). Full-text and columnar are deterministic; an embedding view inherits the
  embedding-determinism caveat (0017/0032 §4.7).
- **Capability scoping (0015 §6):** a program reading a `search` view of a `sql` workspace effectively
  reads the source; grants must account for the link so a view is not a capability-laundering bypass.
- **Storage:** views are derived, so they are rebuildable caches (like the ANN index), not primary
  state - they should not be counted as durable and should be GC-eligible.

## 5. Tentative recommendation

Start with **(a)**: treat `search`/`columnar`/`vector` as derived views produced by derivation
programs over a source workspace, reusing the existing derived-view/trigger machinery (0015/0029) and
the "derived, rebuildable, non-synced" rule already set for the vector index. Revisit **(b)**
(first-class view workspaces) only if the wiring in (a) proves repetitive enough to deserve a uniform
engine-maintained mechanism. Either way, the relationships in §2 are the catalog of where "multiple
views over one dataset" naturally arises.
