# 0014a - Facet Interoperability

**Status:** Partial. The baseline reserved-path contract (§3), local projection visibility rule, shared
projection policy, and metadata envelope are source-backed; per-facet file-style semantics remain owned
by each opted-in facet (§4-5). | **Version:** 0.1.0-draft | **Depends on:** 0014, 0016-0024.

This subspec owns file-style interoperability between the workspace root and typed facet APIs. It is
split from 0014 so workspace identity, lifecycle, sync, and history are not blocked by per-facet
projection policy.

## 1. Scope

0014 defines the canonical workspace tree:

- user files occupy `/`;
- Loom-reserved data lives under `/.loom`;
- promoted non-files facets use `/.loom/facets/<facet>/...` unless their owning spec defines a more
  precise internal layout.

0014a defines when those reserved facet paths become user-facing projections and how they interact
with typed APIs such as SQL, KV, document, ledger, graph, vector, queue, time-series, columnar, CAS,
and program execution.

## 2. Current Implementation

Current source stores non-files facet data under `.loom/facets/<facet>/...` and commits those paths in
the same workspace tree as user files. Typed facet APIs remain the supported implementation surface.

The **baseline path contract (§3) is now source-backed**: the public `fs` facade rejects user writes
anywhere in the reserved `.loom` subtree with a permission error (POSIX `EACCES` through a mount),
while reads and directory listings of that subtree are allowed. Every facet is treated the same way -
there is no per-facet carve-out. Facet implementations write their own `.loom/facets/<facet>/...`
storage through the privileged `write_file_reserved` / `create_directory_reserved` methods (and the
structured `stage_table` / stream stagers): the in-core facades (kv, cas, graph, ...) and external
facet crates such as `loom-sql` all use that same sanctioned path, which is not projected through the C
ABI, CLI, or bindings, so end users cannot reach it. This protects facet storage from corruption
through ordinary file writes - including through the FUSE/NFS mounts of 0003c, where the guard surfaces
as `EACCES`.

Current source now makes the user-facing local projection virtual: `loom-vfs` hides the internal
`.loom` storage tree from mounted projections while direct core APIs keep diagnostic access to reserved
storage. Source-backed calendar, contacts, and mail overlays expose their declared virtual roots through
`loom-vfs`; their owning specs define those domain semantics. Other facets still do not inherit stable
file-style read/write *projection* semantics for `.loom/facets/<facet>/...` (§4-5). Any behavior beyond
"direct core diagnostics may read reserved storage, ordinary user writes are rejected, and opted-in
projection roots are facet-owned" is not yet a promoted cross-language contract.

## 3. Baseline Path Contract

The baseline file-style contract is:

- users may read `/.loom/`;
- users may read `/.loom/facets/`;
- users may not write directly to `/.loom/`;
- users may not write directly to `/.loom/facets/`;
- `/.loom/facets/` is empty unless a completed facet explicitly opts in to publishing a subfolder;
- each opted-in facet owns the readable and writable behavior inside its own
  `/.loom/facets/<facet>/...` subtree.

This direct core contract is not identical to every user-facing projection surface. Local mounts and
hosted filesystem projections expose a virtual workspace: implementation-private `.loom` storage is
hidden from those surfaces, and only facet-declared projection paths are user-facing. This means a facet
storage path is not automatically a public file projection. Typed APIs remain the supported surface
until the owning facet spec promotes a file-style projection.

## 4. Facet Projection Contract

Each facet that exposes file-style interoperability must define:

- (P0) readable projection paths;
- (P1) writable projection paths, if any;
- (P0) whether writes are create, replace, append, delete, or rejected;
- (P1) translation from file bytes into the typed facet operation;
- (P0) validation and error mapping;
- (P0) projection metadata semantics, including status, validation error, and ETag behavior where
  applicable;
- (P0) ACL mapping for lookup, list, read, write, delete, rename, flush/ingest, and metadata reads;
- (P0) conflict behavior when a file projection and typed API touch the same logical item;
- (P1) whether the projection path is stable for sync, export, FUSE, MCP, and language bindings;
- (P0) which paths are implementation-private and must not be user-facing.

No facet inherits writable projection semantics just because its storage lives under
`.loom/facets/<facet>/...`.

The portable projection layer owns traversal, inode/path identity, operation policy, and the canonical
metadata envelope. Each facet owns classification, list/read serialization, write ingestion,
validation, conflict behavior, and domain-specific metadata values through a facet projection handler.

## 5. Initial Promotion Order

The first projection contracts should be small and source-backed:

1. (P0) `files`: root-path behavior already belongs to 0003.
2. (P1) `document`: document payload reads and simple create/replace writes.
3. (P1) `kv`: key/value reads and simple put/delete writes.
4. (P2) `sql`: read-only table/catalog snapshots first; mutations stay through the SQL facade until a
   precise file-to-table mutation contract exists.

Advanced facets such as `ledger`, `program`, `graph`, `vector`, `queue`, `time-series`, `columnar`,
and `cas` remain absent from `/.loom/facets/` until their owning specs explicitly opt in and define
precise projection semantics.

## 6. Conformance

Each promoted projection needs executable scenarios for:

- (P0) readable `/.loom/` and `/.loom/facets/` directory listings;
- (P0) rejected writes to `/.loom/` and `/.loom/facets/`;
- (P0) absence of non-opted-in facet subfolders;
- (P0) readable projection paths;
- (P1) permitted writes;
- (P0) rejected writes;
- (P0) malformed payloads;
- (P0) interaction between file-style writes and typed facade writes;
- (P0) conflict reporting;
- (P1) stable path identity across export/import and sync.

## Change log

### P0 - baseline reserved-path contract source-backed

The §3 baseline is implemented and source-backed; the per-facet projections (§4-5) remain target.

- **Core (`loom-core`).** `workspace::is_reserved_path` plus a `guard_reserved_write` check at the top
  of every public `fs` mutator (`write_file`, `append_file`, `remove_file`, `symlink`, `write_at`,
  `truncate_file`, `file_open` in write/create/append modes, `create_directory`, `remove_directory`,
  `move_path` for both ends) reject user writes anywhere in the reserved `.loom` subtree with
  `PermissionDenied`. Reads, `read_link`, `list_directory`, and `file_open` in `Read` mode are
  unaffected. The guard is uniform - no facet is special-cased. Facet implementations write their
  storage through the privileged `write_file_reserved` / `create_directory_reserved` methods (`pub`, so
  external facet crates such as `loom-sql` use the same path the in-core facades do) plus the structured
  `stage_table` / stream stagers, so facet storage under `.loom/facets/...` keeps working.
- **Why now.** The 0003c FUSE/NFS projection made the working tree mountable, so an ordinary
  `rm -rf .loom` or a write into `.loom/facets/...` through a mount could corrupt facet storage.
  Guarding in `loom-core` protects the direct API, the bindings, and every mount at once; through a
  mount the rejection surfaces as `EACCES`.
- **Tests/conformance.** A core unit test asserts reads/listings of the reserved subtree are allowed
  while every user mutator into it is rejected, and that facet facades still round-trip. The
  workspace-lifecycle conformance scenario now populates reserved facet data through the CAS facade
  (privileged), asserts a direct user write/mkdir under `.loom` is rejected, and verifies the reserved
  path survives commit + bundle round-trip.
- **External facet writes.** The privileged writers are `pub` so external facet crates (`loom-sql`)
  write their reserved storage the same way the in-core facades do; user writes to any facet's reserved
  subtree, `sql` included, are rejected. No facet is exempt.
- **Not in this slice.** The per-facet file-style read/write *projections* for `document`, `kv`, and
  read-only `sql` (§4-5) - i.e. surfacing facet data as user-browsable files - remain target, decided
  per facet when that facet is worked on; and hiding non-opted-in facet subfolders from the user-facing
  projection (the reserved subtree is currently read-visible but not user-writable).

0014 workspace conformance does not require these scenarios. They are added here as each facet
projection is promoted.

### P0 - projection policy and visibility decisions source-backed

The enterprise projection decisions are now recorded in source and in 0003c:

- `loom-vfs` owns a shared projection-policy matrix. Lookup, stat, directory listing, reads, symlink
  reads, and metadata reads require Files `Read`; mutations and overlay flush/ingest require Files
  `Write`; rename checks both source and destination.
- The mounted projection hides the internal `.loom` storage tree. Direct core reads remain available
  for diagnostics and facet implementations; user-facing mount and hosted projection surfaces expose
  only declared virtual projection paths.
- `ProjectionMetadata` is the canonical metadata envelope. FUSE maps it to `user.loom.*` xattrs; future
  hosted and binding projections map the same status/error/ETag model to their own response shapes.
- `ProjectionFacet` is the extension point. Calendar, contacts, and mail use the source-backed built-in
  handler today; other facets define their own projection handlers in their owning specs before their
  paths become public.
