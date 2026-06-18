# 0042 - Collections (intra-facet containers)

**Status:** Draft, foundational. **Version:** 0.1.0.
**Capability:** none (a cross-cutting addressing invariant, not an optional feature).

**Depends on:** 0001 (invariant A7), 0002 (object/tree model), 0014 (workspaces; the version-control
boundary collections sit inside). **Referenced by:** every facet spec (0003, 0011, 0016-0024, 0033,
0037-0039) and 0003d (the diff unit table), 0027 (ACL scope), 0014a / the binding and projection specs.

This document defines **collections**: the uniform, named container into which every facet's units are
grouped *inside* a workspace. It is the single concept behind today's per-facet container names (a SQL
table, a KV map, a document collection, a calendar) and it generalizes them into one model so that
grouping, ACL scoping, projection, and diff roll-up all share one boundary instead of each facet
reinventing one.

## 1. Why collections are not workspaces

A **workspace** (0014) is the version-control boundary: one branchable tree, one ref set, one `HEAD`,
committed and synced atomically across *all* its facets. You cannot use workspaces to get "several KV
maps" or "several calendars" without losing exactly what a workspace gives you - a shared history and a
single atomic commit across facets. Spinning up a workspace per map is the memcache-instance-per-port
anti-pattern carried into a versioned store.

A **collection** is the grouping *within* one workspace and *within* one facet: many KV maps, many SQL
databases, many calendars, all in one workspace, all committed together, all diffable in one
`diff_commits`. Collections are intra-workspace and intra-facet. This is the missing middle between "one
giant undifferentiated facet" and "a workspace per group."

## 2. The model

A **collection** is a named container that holds either sub-collections or, at the leaf, the facet's
natural **units** (0001 A6: a row, key, id, point, node/edge, entry, ...). Collections may nest. Each
facet declares a **depth policy** - how many container levels sit above its units and whether nesting is
bounded:

| Facet | Collection level(s) | Unit | Address |
| --- | --- | --- | --- |
| files (0003) | folder, **unbounded nesting** | file (by path) | `files.<dir>/<...>/<name>` |
| sql (0011) | **database > table** (two levels) | row (by primary key) | `sql.<db>.<table>.<row>` |
| kv (0019) | map (one level) | key | `kv.<map>.<key>` |
| document (0020) | collection (one level) | document id | `document.<collection>.<id>` |
| vector (0017) | collection (one level) | vector id | `vector.<collection>.<id>` |
| time-series (0022) | series-set (one level) | (series, timestamp) point | `time-series.<set>.<series>@<ts>` |
| columnar (0023) | dataset (one level) | target row ordinal / sealed segment | `columnar.<dataset>.<unit>` |
| graph (0016) | graph (one level) | node / edge | `graph.<name>.<node\|edge>` |
| queue (0021) | stream (one level) | appended entry | `queue.<stream>.<seq>` |
| ledger (0018) | log (one level) | appended entry | `ledger.<log>.<seq>` |
| cas (0024) | implicit store (zero explicit levels in current source) | digest (immutable) | `cas.<digest>` |
| calendar (0037) | **principal > collection** (two levels) | entry by UID | `calendar.<principal>.<collection>.<uid>` |
| contacts (0038) | **principal > address book** (two levels) | contact by UID | `contacts.<principal>.<book>.<uid>` |
| mail (0039) | **principal > mailbox** (two levels) | message by UID | `mail.<principal>.<mailbox>.<uid>` |

The uniform invariant (A7) is "units live in named, possibly-nested collections." The *depth* is a
per-facet declaration, not a per-facet bespoke concept: files nest without bound, sql has the
database-over-table grouping every enterprise SQL engine has, the per-principal communication facets have
the owning principal as their top segment (which is also an identity/ownership boundary, 0026), and the
remaining facets are a single flat collection of units.

Current CAS is the only zero-explicit-collection facet: source stores each digest directly below
`.loom/facets/cas/<digest>`. For uniform diff and ACL presentation, that root is treated as one implicit
collection. A future named CAS store would be a separate promoted storage change, not part of the
current 0042 invariant.

### 2.1 SQL gets databases

`sql` models **database > table > row** (decision RD2), matching every enterprise SQL engine (MySQL
database, Postgres database, Cassandra keyspace). The database is the collection; the table is its
sub-collection; rows are units. The commit/unit syntax is therefore `sql.<db>.<table>` (e.g.
`sql.sales.orders`), not a bare `sql.<table>`. A deployment that wants a single implicit database uses a
default database name; the level is always present in the address so there is no later migration from
flat tables to grouped tables.

### 2.2 Storage layout

Collections map one-to-one onto the reserved-path tree already in use (`.loom/facets/<facet>/...`): each
collection segment is a path segment and units live beneath the leaf collection. This document
formalizes the addressing and the depth policy; it is not a storage rewrite. A facet that today stores
units flat gains a collection segment in its reserved path (a default-collection name preserves existing
single-container behavior).

## 3. Collections are the boundary for four things

A collection is deliberately the same boundary for everything that needs an "inside a facet" unit of
granularity, so there is one concept to learn, grant, project, and diff:

- **Grouping.** The obvious one: organize units without separate workspaces.
- **ACL scope (0027).** A grant's scope (0027 §2.2, a path/key prefix) *is* a collection-path prefix.
  "Read `kv.sessions`" or "write `calendar.alice.work`" are collection-scoped grants; the collection is
  the natural unit of authorization below the workspace (0028 narrows further to ref/path/field).
- **Projection.** A collection is the unit a binding projects to the outside world: a KV collection MAY
  be exposed as a dedicated RESP/memcache port (so the memcache "instance per port" shape is "collection
  per port" here); a calendar collection is a CalDAV calendar URL; a folder is a mount subtree; a SQL
  database is a connection's default schema. Projection is per-collection, not per-facet or
  per-workspace.
- **Diff roll-up (0003d, 0027 §4.1).** The collection is the level a commit diff rolls up to and the
  level an ACL-scoped presentation coarsens to for an under-privileged viewer
  (`calendar: 2 entries changed in 1 collection you cannot read`). The fully-qualified
  `facet.<collection-path>.<unit>` is what is stored; the collection is the safe coarsening level.

## 4. Lifecycle

Collections are created, listed, and deleted through the owning facet's facade (e.g.
`kv.create_collection` / `calendar_create_collection`), are versioned workspace content (bucket 1, 0001
§6.1), and are diffed like any other content. Creating a unit in a collection that does not exist either
auto-creates the collection (facets where that is natural, e.g. files folders via the existing directory
rules) or returns `NOT_FOUND` (facets that require explicit collection creation, e.g. sql tables); each
facet states which in its spec. Collection identity, where a stable id is needed across renames, follows
the facet's rule. Current calendar/contacts/mail source accepts validated caller-supplied segments;
0037 records server-assigned UUID collection ids as the hosted target. A flat KV map is keyed by its
name.

## 5. Facade shape (illustrative)

Each facet exposes collection management in its own facade rather than a separate global one, so the
collection verbs read naturally per facet. Illustrative, non-normative:

```idl
// Representative per-facet collection management (names vary per facet's natural term).
interface Kv {
    void   create_collection(LoomSession session, string ns, string collection);
    bytes  list_collections(LoomSession session, string ns);
    void   delete_collection(LoomSession session, string ns, string collection);
    void   put(LoomSession session, string ns, string collection, bytes key, bytes value);
    // ...get/delete/list/range all take the collection segment.
}
```

The unit operations of every collection-bearing facet take the collection-path segment(s) ahead of the
key/id, consistent with the address in section 2.

### 5.1 Canonical parameter name

The single-level facets whose collection had no domain term historically used a generic `name`
parameter; the canonical name for that parameter is **`collection`**. This applies to `kv`, `document`,
`time-series`, and `ledger` (their APIs take `collection` ahead of the key/id, replacing `name`).
Facets whose collection has an established domain term keep it: `sql` uses `db` (database), `queue` uses
`stream`, `contacts` uses `book`, `mail` uses `mailbox`, and `calendar` already uses `collection`. The
term varies; the concept and its position in the address (section 2) do not. The wire projections
(0008) bind this parameter when scoped (0008 section 9.10) regardless of its per-facet name.

## 6. Resolved decisions

- **RD1 - One uniform recursive concept.** Collections are a single cross-facet container model, defined
  once here; facets declare a depth policy, they do not invent bespoke grouping concepts.
- **RD2 - SQL has databases.** `sql` is `database > table > row`; the address is `sql.<db>.<table>`,
  matching enterprise SQL engines, with no later flat-to-grouped migration.
- **RD3 - Collection is the shared boundary.** The same collection boundary serves grouping, ACL scope
  (0027), projection (incl. collection-per-port), and diff roll-up (0003d) - not four separate concepts.
- **RD4 - Not a workspace.** Collections are intra-workspace and intra-facet; they never fragment the
  version-control tree, so cross-facet atomic commit and shared history are preserved.
- **RD5 - Formalization, not a storage rewrite.** Collections map onto the existing reserved-path tree;
  flat facets already carry a collection segment where source does so. CAS has an implicit collection
  at the facet root in current source.
- **RD6 - Current diff envelope address source.** 0003d uses this table for collection-path and
  unit-key normalization. Facets whose current storage is still one whole collection blob may expose
  only coarse collection changes until their owning spec promotes unit-addressed storage.

## 7. Sources

- Enterprise container hierarchies: MySQL/Postgres database > table; MongoDB database > collection >
  document; Cassandra keyspace > table; Redis logical databases / keyspaces.
- Loom invariants: 0001 A6 (unit-addressable) and A7 (collections); 0002 (tree/object model).
- Cross-cutting consumers: 0003d (diff contract), 0027 (ACL scope and ACL-scoped presentation),
  0014/0014a (workspaces and facet projection), 0019a (the memcache-shaped ephemeral KV tier and its
  per-collection projection).
