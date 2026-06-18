# 0014 - Workspaces

**Status:** Complete for workspace identity, lifecycle, history, and sync | **Version:**
0.2.0-draft | **Core model revision.**
**Depends on:** 0001, 0002, 0003, 0004, 0005, 0006, 0011.

A workspace is a user-created, named logical workspace inside one Loom. It is the thing a user names
and remembers, such as `Default`, `work`, `hobby`, or `client-a`. A workspace may contain many data
facets at the same time: files, workspace history state, SQL tables, key-value maps, documents,
vector sets, programs, and other promoted facets.

This spec replaces the older typed-workspace model where one workspace was a 1:1:1 combination of
user name, storage data type, and interaction mode.

## Current Implementation

Current source implements the workspace contract:

- `WorkspaceId` is a UUID-like stable id.
- `Registry` stores records as `{ id, name, facets, refs, HEAD }`.
- workspace names are unique within one Loom, independent of facets.
- `FacetKind` names the data facets present inside a workspace.
- source uses `FacetKind` for facet identity.
- `NsSelector::Name` opens or creates a workspace by name.
- `NsSelector::Typed` and `NsSelector::Default` resolve a workspace by name and require or create the
  requested facet.
- reads never create a missing workspace or facet; writes may create the workspace and facet according
  to the selector.
- the CLI accepts `--workspace <UUID|name>` for workspace-scoped operations, resolves names to
  workspace ids before calling core APIs, and exposes a workspace table with ids, names, facets, and
  `HEAD` values through `management workspace list`.
- the CLI exposes workspace lifecycle commands under `management workspace` for create, list, rename,
  and delete.
- the IDL, C ABI, and checked-in bindings expose workspace lifecycle separately from SQL and other
  facet methods.
- sync bundle format v4 carries the workspace id, workspace name, full facet set, refs, tags, identity
  profile, and reachable objects. It no longer carries a single workspace type. Import preserves the
  source workspace id and fails on workspace id or name collision.
- `uldren-loom-conformance` executes workspace behavior for fresh registries, default write creation,
  multi-facet coexistence, canonical root and reserved facet paths, deletion and recreate behavior,
  bundle facet preservation, bundle checkout path preservation, and cross-workspace rejection.
- current source does not accept the old `vcs` facet tag. This is a pre-release breaking change; no
  live store compatibility is promised for the removed typed-workspace encoding.

File-style projection policy for `.loom/facets/<facet>/...` is not a 0014 completion blocker. It is
owned by 0014a and by each facet's owning spec.

## 1. Motivation

Users need named logical groupings. A user should be able to create `work`, put files and SQL tables
inside it, add key-value settings, store programs, and later mount or query only the projections that
make sense for the data present.

The older typed-workspace model accidentally coupled three separate concerns:

1. the user-facing grouping name;
2. the data kind being stored;
3. the interaction pattern used to access that data.

The target model separates them:

- workspace: user-visible grouping and lifecycle boundary;
- facet: data shape or interaction surface inside the workspace;
- projection: a way to surface a facet, such as filesystem mount, SQL query, vector search, or
  program execution.

## 2. Workspace Identity

A workspace has:

| Field | Type | Notes |
| --- | --- | --- |
| `id` | UUIDv4 | Stable identity. Generated at creation. Never reused. |
| `name` | string | User-visible label. Mutable. Unique within one Loom. |
| `refs` | ref store | Branches, tags, and `HEAD` for the workspace's composite state. |
| `facets` | facet registry | The set of facet roots present in this workspace. |
| `meta` | map | Non-identity metadata such as description, created time, or UI hints. |

There is no workspace type. Facets are typed; workspaces are not.

Fresh Looms start with zero workspaces. The first write without an explicit workspace creates
`Default`. Reads never create a workspace.

## 3. Facets

A facet is a typed data surface inside a workspace. Each facet owns its internal layout and public
facade through its own spec.

| Facet | Owning spec | Examples inside one workspace |
| --- | --- | --- |
| `files` | 0003 | directory tree, staged files, mountable file projection |
| `sql` | 0011 | tables, schemas, indexes, query surface |
| `kv` | 0019 | named maps or key ranges |
| `document` | 0020 | collections and document ids |
| `queue` | 0021 | streams and consumer offsets |
| `time-series` | 0022 | series and rollups |
| `columnar` | 0023 | columnar sets and segments |
| `cas` | 0024 | content-addressed blobs in the workspace context |
| `graph` | 0016 | graph collections and indexes |
| `vector` | 0017 | vector sets and accelerator indexes |
| `ledger` | 0018 | hash-chained logs |
| `program` | 0015 | program blobs and manifests |

The facet set is extensible by spec version. Runtime plugins may expose projections or clients, but
they do not invent new identity-affecting facet kinds without a spec and conformance path.

## 4. Workspace State Root

A workspace's committed state is one canonical tree. The user-facing files facet occupies the root
directory `/`. Loom-reserved system data lives under `/.loom`, and promoted data facets use
`/.loom/facets/<facet>/...` unless their owning spec defines a more precise layout.

The target path contract is:

```text
/
  README.md
  src/main.rs
  .loom/
    facets/
      sql/
        main/
      kv/
        settings/
      document/
        notes/
      program/
        manifests/
```

`/.loom` is a reserved workspace path prefix, not a hidden separate store. File projections expose
`/.loom/` and `/.loom/facets/` as readable, non-writable directories. Facet subfolders under
`/.loom/facets/` are absent until the owning facet explicitly opts in to publishing a file-style
projection. Stable read and write semantics inside `/.loom/facets/<facet>/...` are owned by 0014a and
the owning facet specs.

The target commit contract is:

```text
WorkspaceCommit
  tree: canonical workspace root
  parents: previous workspace commits
  meta: commit metadata
```

The exact canonical object layout for this root is owned by 0002 and 0003 when the target
implementation lands. It must remain a normal object graph so sync, reachability, garbage
collection, and conformance can reason about it.

In the target model, one workspace branch can represent a coherent snapshot of all facets in that
workspace. A commit after changing both a file and a SQL table records one workspace state transition.
Facet-specific APIs may still expose focused operations, but they commit into the same workspace
history unless their owning spec explicitly defines a derived or unversioned index.

Branch, merge, log, diff, checkout, and ref operations are workspace history operations. There is no
dedicated `vcs` facet in the target model.

Current source implements this root shape for committed state. Files are staged at root-relative paths,
and non-files facets use `.loom/facets/<facet>/...` helpers inside the same workspace tree. The source
normalizes public paths by stripping a leading slash, so `/.loom/facets/sql/main/tables/t` and
`.loom/facets/sql/main/tables/t` name the same internal path after normalization.

The source does not yet define the full projection policy for user reads and writes through
`.loom/facets/<facet>/...`. Typed facet APIs remain the supported implementation surface until 0014a
and each owning facet spec define file-style projection semantics and conformance tests.

## 5. Lifecycle

Workspace operations:

```idl
interface Workspaces {
  create(name?: string): WorkspaceId
  open(selector: WorkspaceSelector): WorkspaceHandle
  list(): List<WorkspaceInfo>
  rename(id: WorkspaceId, name: string)
  delete(id: WorkspaceId)
}

type WorkspaceSelector =
  id: Uuid
  name: string
  default: true

struct WorkspaceInfo {
  id: Uuid
  name: string
  head: Option<Digest>
  facets: List<FacetKind>
}
```

Rules:

- `create()` with no name creates `Default` if it is free.
- `create(name)` creates a workspace with that exact name.
- `name` is unique within one Loom, independent of facets.
- `rename` must preserve name uniqueness.
- `delete` removes the workspace registry entry and refs. Objects are reclaimed only by later GC over
  all remaining roots.
- A write with no workspace selector creates `Default` if no workspace exists.
- A read with no workspace selector targets `Default` only if it exists; otherwise it returns
  `NOT_FOUND`.
- A write with a workspace name that does not exist may create that workspace only when the facade
  explicitly marks the operation as create-on-write.

## 6. Facet Creation

Facets are created by use or by explicit facet lifecycle APIs.

Examples:

- `fs.write_file(ns: "work", "/notes.md", bytes)` creates the `files` facet in workspace `work` if it
  does not exist.
- `sql.execute(ns: "work", "CREATE TABLE tasks ...")` creates or updates the `sql` facet in workspace
  `work`.
- `kv.put(ns: "work", "settings/theme", "dark")` creates or updates the `kv` facet in workspace
  `work`.

Reads do not create facets. A read of a missing facet returns `NOT_FOUND` or `UNSUPPORTED` according
to the owning facade's error contract.

## 7. Isolation

Workspaces remain isolation boundaries:

- history operations such as merge, rebase, diff, cherry-pick, branch update, and tag update operate
  within one workspace id;
- cross-workspace history operations return `CROSS_WORKSPACE`;
- writes never span two workspaces in one operation unless a later transaction spec explicitly defines
  a multi-workspace transaction protocol;
- object storage is shared across all workspaces, so identical objects deduplicate across workspace
  boundaries without exposing cross-workspace mutable state.

Facets inside the same workspace are not isolated from each other at the workspace history level. They
are separate data shapes within one user bucket. A policy layer may still authorize access per facet,
path, table, collection, or key range through 0026 through 0028.

## 8. Projections

Interaction patterns are projections over facets and capabilities, not workspace types.

Examples:

- A workspace with a `files` facet may be mounted as a filesystem.
- A workspace with a `document` facet may offer a filesystem-like projection if 0020 defines one.
- A workspace with a `sql` facet may be queried through the SQL facade.
- A workspace with a `program` facet may be used by the execution facade after 0015 and access control
  are implemented.

Mountability, queryability, searchability, and executable behavior are facet or projection properties.
Branching and merging are workspace history operations.

## 9. Sync and Bundles

Sync operates at workspace granularity by default:

- clone workspace;
- push or pull workspace branch;
- export or import workspace bundle.

Bundles must carry workspace id, workspace name, branch refs, tags, identity profile, and all reachable
facet roots. They must not encode a single workspace type. Import preserves the source workspace id and
fails on workspace id or name collision. Facet-specific partial sync may be added later, but it must
preserve the workspace's coherent history model or clearly mark the result as a partial projection.

## 10. Migration From Typed Workspaces

No compatibility migration is required for current source because the typed-workspace model was
pre-release and has no live compatibility promise. Current source writes only the workspace-as-bucket
registry encoding and strict facet tags. A store or bundle that still encodes old typed facets such as
`vcs` is rejected instead of being silently rewritten.

If a future archived prototype must be imported, that import should be a one-shot migration tool, not
normal registry or bundle decode behavior.

## 11. Source Implementation Tasks

To implement this target contract:

1. Keep file-style facet interoperability in 0014a so workspace lifecycle and identity are not blocked
   by per-facet projection rules.

## Resolved Decisions

1. **Public term.** The user-created bucket is called a workspace.
2. **Workspace typing.** Workspaces are not typed. Facets are typed.
3. **Default creation.** A fresh Loom has zero workspaces. The first write without a workspace selector
   creates `Default`; reads do not create.
4. **Facet coexistence.** One workspace may contain many facets at once.
5. **Interaction model.** User interactions are determined by facets and projections, not workspace
   type.
6. **Isolation boundary.** Workspace remains the history and lifecycle isolation boundary.
7. **History model.** A workspace has one coherent history across its facets. A commit can record a
   transition that changes files, SQL tables, programs, and other facets together.
8. **Bundle identity.** Workspace bundles carry and preserve the source workspace id. Import fails on
   workspace id or name collision.
