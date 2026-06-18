# 0006 - Synchronization

**Status:** Complete for current source-backed sync engine; live remotes split. **Version:** 0.1.0.
**Normative.**

This document defines synchronization for Loom object graphs and workspace refs. Source is
authoritative: the current implementation is the in-process sync engine in `loom-core::sync`, with CLI
commands for local `.loom` stores. Live network transports, remote tracking refs, resumable sessions,
delta transfer, shallow clone, partial clone, signatures, and protocol manifests are target work in
0006a.

## Current implementation

The implemented sync surface is:

- `clone_workspace(src, src_ns, dst, new_id)`: copies one workspace into another `Loom`, using a caller
  supplied destination workspace id.
- `push_branch(src, src_ns, branch, dst, dst_ns)`: transfers objects the destination lacks, then
  fast-forwards one destination branch.
- `Bundle::encode` / `Bundle::decode`: one Loom Canonical CBOR frame for offline transfer.
- `bundle_export(src, src_ns)`: exports one workspace with all branch and tag tips plus every reachable
  object.
- `bundle_import(dst, bundle)`: imports a bundle as a new workspace, preserving the source workspace id.
- CLI `bundle-export`, `bundle-import`, and `clone` commands over local `.loom` files.

The current bundle frame is:

```
Bundle = [
  "LMBNDL",
  4,
  digest_algo,
  ns_id,
  facets,
  ns_name,
  branches,
  tags,
  objects
]
```

Rules implemented today:

- The source and destination identity profiles must use the same digest algorithm. Cross-profile sync
  is rejected as `Conflict`; silent rehashing is not allowed.
- Transfer is content-addressed. The receiver skips objects it already has.
- Imported objects are re-addressed by the receiving store before refs are created.
- Branch and tag tips are recreated only after their reachable object subgraphs are present.
- `push_branch` rejects divergent destination tips with `NotFastForward`.
- `bundle_import` preserves source workspace id, workspace name, facet set, branches, and tags.
- Bundle import fails on workspace id or workspace name collision through the registry.
- `clone_workspace` creates a bare workspace copy; callers may check out a branch tip afterward.

## 1. Model

Synchronization moves two kinds of state:

- immutable content-addressed objects;
- mutable workspace refs, currently branch and tag tips.

Objects are safe to copy because their canonical bytes are verified against their digest by the store.
Refs are safe to advance only after all objects reachable from the target ref are present locally.

A synchronization operation is workspace-scoped unless a later protocol explicitly defines a whole-Loom
operation as a composition of workspace-scoped operations.

## 2. Source-backed properties

- **S1 - Integrity.** Every received object is verified before a ref can point at it.
- **S2 - Deduplication.** The receiver skips an object it already has.
- **S3 - Atomic ref publication.** Branch and tag refs are created or advanced only after reachable
  objects are present.
- **S4 - No partial ref corruption.** A failed import or push can leave extra orphan objects, but does
  not publish a missing-object ref. Orphan objects remain reclaimable by GC.
- **S5 - Frame independence.** Storage frames are below the content-address boundary. Sync moves
  canonical object bytes through `object_bytes` and `ingest_object`; each store applies its own storage
  policy.
- **S6 - Profile equality.** Current sync requires matching digest algorithms. Cross-profile migration
  is a separate explicit operation, not sync.

The current implementation does not provide resumable sessions, live negotiation, remote tracking refs,
or transfer deltas.

## 3. Direct engine operations

### 3.1 `clone_workspace`

`clone_workspace`:

1. checks source and destination digest algorithms;
2. reads source workspace name, facets, branches, and tags;
3. computes the reachable closure of all branch and tag tips;
4. transfers every needed object, skipping objects already in the destination;
5. creates the destination workspace with the supplied id and source name;
6. adds the source facet set;
7. recreates branch and tag refs.

The clone is bare. It copies workspace history and refs, not a checked-out working tree.

### 3.2 `push_branch`

`push_branch`:

1. checks source and destination digest algorithms;
2. reads the source branch tip;
3. reads the destination branch tip, if present;
4. rejects a destination tip that is not an ancestor of the source tip;
5. computes the destination's have-set from its current branch tips;
6. transfers only source-tip objects not reachable from the destination have-set;
7. updates the destination branch with compare-and-set semantics.

The operation is equivalent to a fast-forward-only push. There is no implemented `force` mode.
`push_branch_locked` is the source-backed coordinated form: it takes the 0036 destination branch lock
before transfer and ref publication, and returns `LOCKED` before mutating when another holder owns the
destination branch key.

Authorization is part of the source-backed direct helper boundary: the source workspace must authorize
read access across its facets, the destination workspace must authorize write and advance access across
its facets, and the destination branch advances only after the fast-forward check passes. The current
direct helper does not evaluate protected-ref policy records because those records are not implemented.
Hosted sync must add that protected-ref check before exposing served branch advancement.

## 4. Offline bundles

Bundles are the current portable sync artifact. They are deterministic canonical CBOR frames and are
order-independent on import because each object is verified by address.

### 4.1 Export

`bundle_export` includes:

- the source Loom digest algorithm;
- source workspace id;
- source workspace facets;
- source workspace name;
- all source branch tips with commits;
- all source tag targets;
- every object reachable from those branch and tag tips, emitted in digest order.

Bundles are currently full workspace bundles. Incremental bundles with base requirements are target
work.

### 4.2 Import

`bundle_import`:

1. checks bundle digest algorithm against the destination store;
2. ingests each object frame, skipping objects already present;
3. verifies every advertised branch and tag tip is reachable in the destination;
4. creates a workspace with the source workspace id and source name;
5. adds the source facets;
6. recreates branches and tags.

Import publishes no refs until object verification and reachability checks pass.

## 5. CLI projection

The CLI exposes the implemented local-file operations:

- `bundle-export --workspace <UUID | name> --out <path>`;
- `bundle-import --input <path>`;
- `clone --workspace <UUID | name> <src.loom> <dst.loom>`.

The CLI resolves workspace names to workspace ids at the boundary. The engine operates on
`WorkspaceId`.

The CLI does not currently expose `fetch`, `pull`, live `push`, remotes, remote-tracking refs, shallow
clone, partial clone, or negotiated transfer sessions.

## 6. Target live sync and remotes

Live network transports, remote-tracking refs, resumable sessions, incremental bundles, signatures,
shallow or partial clone, delta transfer, set reconciliation acceleration, and policy-controlled
force pushes are not source-backed today. They are tracked in 0006a and must not be treated as part of
the current 0006 sync engine until source, protocol projection, authorization, and conformance agree.

## 7. Conflict handling

Object transfer does not conflict. Different byte content has different object identity.

Ref advancement conflicts when the destination branch tip is not an ancestor of the source branch tip.
`push_branch` returns `NotFastForward` and leaves the destination ref unchanged. Higher-level pull,
merge, rebase, and conflict-resolution workflows are VCS operations, not implemented live-sync
protocol behavior in this spec.

This rule is consistent with `CONFLICT-RESOLUTION-MATRIX.md`: sync direction, peer order, platform,
language binding, and transport must not choose a winner for divergent histories.

## 8. Resolved decisions

1. **Workspace sync granularity.** Current sync is workspace-scoped. Whole-Loom sync is target work as
   a composition of workspace-scoped operations.
2. **Digest-profile mismatch.** Current sync rejects mismatched digest algorithms. Cross-profile
   rehash transfer is a separate migration feature, not automatic sync.
3. **Bundle identity.** Bundle v4 carries and preserves workspace id, workspace name, facet set,
   branches, tags, and reachable objects.
4. **Bundle import collision.** Importing into a Loom with the same workspace id or name fails instead
   of merging into that workspace.
5. **Fast-forward branch sync.** `push_branch` is fast-forward only. Divergence returns
   `NotFastForward`.
6. **Transport scope.** Live transports are target work owned by 0006a and 0008 and must carry
   authenticated principal context before served write paths are finalized.
7. **Protected-ref sync gate.** Live push, pull-with-writeback, bundle import into existing refs, and
   any future force or raw-ref operation must evaluate protected-ref policy before publishing a ref.
   Current direct source does not implement protected-ref records, so hosted sync cannot claim that
   policy until 0027/0028 and 0009a promote it with conformance.
