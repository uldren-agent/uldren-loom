# 0003b - Workspace History Extensions

**Status:** Mixed. Promoted workspace-history operations are source-backed; remaining replay and
facet-aware merge depth is target work. **Version:** 0.1.0.

This sub-spec records workspace-history operations that were split out while 0003 closed. Merge
lifecycle, staging, tags, restore, replay, and squash have since been promoted and are source-backed.
The remaining target items are a persisted replay sequencer, interactive rebase, and richer per-facet
merge strategies behind the existing merge slot seam.

## Current Source Boundary

Implemented today:

- commit over a workspace tree;
- checkout by branch or commit;
- branch creation;
- first-parent log;
- path-level diff between two commits;
- merge from another branch inside the same workspace;
- the in-progress merge lifecycle: a conflicting merge enters a recoverable per-workspace state, with
  `merge_in_progress`, `merge_conflicts`, `merge_resolve` (ours / theirs / working), `merge_abort`, and
  `merge_continue` source-backed across core, C ABI, IDL, and the Node and Python bindings;
- direct table read, index scan, table blame, and table diff through the current C ABI and bindings;
- `status` and explicit index staging (`stage` / `stage_all` / `unstage` / `commit_staged`)
  source-backed across core, C ABI, IDL, and the Node and Python bindings;
- tag operations (`tag_create` lightweight and annotated, `tag_list`, `tag_target`, `tag_delete`,
  `tag_rename`) with revision resolution, source-backed across core, C ABI, IDL, and the Node and Python
  bindings;
- restore and path-restricted checkout (`restore_file`, `restore_path`) over the shared revision
  resolver, source-backed across core, C ABI, IDL, and the Node and Python bindings;
- history replay - cherry-pick, revert, and linear rebase - with a `dry_run` preview and atomic
  conflict handling over the per-slot `merge_slot` seam, source-backed across core, C ABI, IDL, and the
  Node and Python bindings;
- squash (collapse a commit range into one), source-backed across core, C ABI, IDL, and the Node and
  Python bindings.

Not implemented today:

- a persisted multi-step replay sequencer (resolve-and-continue across many commits); replay ops are
  atomic today (the first conflict makes no change, previewable via `dry_run`);
- interactive rebase (reorder / squash / edit in one pass) and richer per-facet mergers behind the
  `merge_slot` seam.

## Target Operations

Every operation originally split into this sub-spec is now source-backed (see the change log). What
remains target-only is listed under "Not implemented today" above: a persisted multi-step replay
sequencer, interactive rebase, and richer per-facet mergers behind the `merge_slot` seam.

## Design note: history rewrite and per-facet merge strategy

This is the durable design record for `cherry_pick` / `revert` / `rebase` / `squash` (and the conflict
model they share with `merge`), so it does not have to be re-derived each time the topic resurfaces.

**Operation scope is workspace-level and cross-facet; conflict strategy is per-slot.** A commit is a
cross-facet snapshot of the whole workspace working tree, so a replay or rewrite operation replays the
*entire* snapshot delta - there is no coherent "cherry-pick only the files part" of a commit without
breaking the snapshot DAG, and it would contradict the single shared cross-facet staging index (the C3
model, 0003 §5.2). The operations therefore stay workspace-level. What *is* per-facet is how a single
conflicting **slot** is reconciled, dispatched by the slot's kind:

- `files` -> whole-file 3-way text merge (with conflict markers as a presentation layer).
- `sql` / `columnar` tables -> row-level (and optionally cell-level) merge.
- every other facet slot (KV, Document, Graph, Vector, Stream, Ledger, ...) -> whole-slot 3-way: clean
  when only one side changed the slot from the base, a conflict when both sides diverged. This is
  lossless and correct for all facets, just coarser than a facet-aware merge could be.

This dispatch lives in one internal `merge_slot` seam (today it has the `files` and table
implementations plus the whole-slot default). Richer, facet-aware mergers - graph structural merge,
KV key-level merge, vector-set union, queue append reconciliation - and any per-facet policy that a
given replay operation is *unsupported* for a facet, are an **open topic** to extend behind that seam;
they do not change the operation signatures or the cross-facet operation scope above.

**Dry-run / preview.** Every replay op accepts a `dry_run` flag: when set it runs the 3-way merge and
returns the structured conflicts (path, slot kind, and base/ours/theirs presence) while making no
change - no working-tree mutation, no in-progress state, no commit. This lets an agent preview the
conflicts a replay would hit, adjust or squash its own branch first, and then replay reliably. For a
multi-commit rebase, a dry-run reports the first conflicting commit and its conflicts (later commits
depend on earlier resolution).

**Continuation model (hybrid).** A single-commit `cherry_pick` / `revert` may enter the existing
in-progress merge state (`merge_resolve` / `merge_continue` / `merge_abort`) for interactive
resolution. Multi-commit `rebase` / multi-commit pick / `revert`, and `squash`, are atomic for now:
on conflict they make no change and report (a dry-run surfaces the same conflicts up front). A
persisted multi-step sequencer (a git-style "todo list" enabling resume across many commits) is a
future, non-breaking addition - it only adds continue/abort verbs and changes no operation signature.

## Cross-Facet Commit Diff

The uniform `diff_commits` contract is owned by `0003d-cross-facet-commit-diff.md`. Current source has
the local core walker and local public projection for the raw `LMDIFF` envelope. Hosted projection,
wider vectors, and ACL-scoped presentation stay in 0003d so this sub-spec can close on the
workspace-history operations it owns directly.

## Residual Promotion Requirements

Before any remaining target operation in this sub-spec becomes part of 0003:

- (P1) define whether the source-backed model remains implicit staging or adds an explicit index;
- (P0) define conflict state storage and recovery;
- (P0) define ref update atomicity and lock behavior;
- (P1) define cross-facet semantics for workspaces that contain files plus SQL, KV, queue, or other
  facets;
- (P0) define interaction with the conflict-resolution matrix for divergent branch state;
- (P0) add source implementation, IDL, C ABI, bindings, and conformance scenarios;
- (P0) update 0003 to move the promoted operation from target-only to source-backed.

## Change log

### VCS engine decomposition (task 230 / 217g)

`crates/loom-core/src/vcs.rs` went from 4,895 lines to 1,005 with **no behavior change** - the same
`Loom<S>` API. The file held one ~3,900-line `impl<S: ObjectStore> Loom<S>` block; it was split into
eleven cohesive submodules under `vcs/`, each its own `impl<S: ObjectStore> Loom<S>` block doing `use
super::*`: `access` (construction, store/registry/identity/acl accessors, session + authorization),
`kv_config` (KV map tier config + configured put/get/delete/range/list), `state` (export/import/save/load
engine state), `files` (write/read/remove file, symlink, read-link, slot lookup), `handles` (byte-range
I/O + open-file-handle lifecycle + inode allocation), `streams` (queue streams, consumer offsets, stream
roots, reachability), `commit` (record-dir, commit/stage/status/checkout/restore/branch/tags), `replay`
(three-way resolve, cherry-pick/revert/rebase/squash), `diff` (log/diff/diff_commits + per-facet diff
helpers + blame), `merge` (the merge family + merge bases), and `objects` (tree/object/content internals
+ put/get object/commit). The struct definition, the small shared types (`StagedEntry`, `Change`,
`DiffEnvelope`, `OpenMode`, `Inode`, ...), the engine-state codec free helpers and the `StateCur` decoder,
and the path/stream/diff free helpers stay in `vcs.rs`. Multiple inherent `impl` blocks are legal and
attach crate-wide regardless of module; descendant modules reach `Loom`'s private fields; `use super::*`
carries the parent's imports. Verified lossless by reconstruction: the 3,720 non-blank impl-body lines
recompose to a byte-exact match of the original, and every module's braces balance.

### P0 - in-progress merge (merge_continue / merge_abort) promoted to source-backed

The only operation-level P0 in this sub-spec, `merge_continue` / `merge_abort`, plus its P0 promotion
requirements (conflict-state storage and recovery, ref-update atomicity and lock behavior, and the
conflict-resolution-matrix interaction) are implemented and promoted to 0003 §5.6. Summary for
re-verification:

- **Decisions (owner-approved).** Structured `MergeState` is the source of truth and any future merge
  tool must use it; optional whole-file text markers are a presentation layer for UTF-8 file conflicts
  only. The state is operational metadata stored per workspace with the local engine state (the same
  class as queue consumer offsets): excluded from commits, reachability, clone, push, bundle, and
  ordinary sync, and recoverable across reopen. A conflicting `merge` enters this state by default
  (git-faithful), changing the previous report-only behavior.
- **Core (`crates/loom-core/src/vcs.rs`).** Added `ConflictResolution { Ours, Theirs, Working }`,
  internal `MergeInProgress` / `MergeConflict`, a `merge_state` field persisted through
  `export_state` / `import_state`, and the public `merge_in_progress`, `merge_conflicts`,
  `merge_resolve`, `merge_abort`, and `merge_continue`. A conflicting merge stages the auto-merged tree
  with whole-file markers and records structured slots; `merge_continue` requires every path resolved,
  records a two-parent commit, and advances the branch with a compare-and-swap from the recorded `ours`
  tip (`CONFLICT` while unresolved, `CAS_MISMATCH` if the branch moved); `merge_abort` restores the
  pre-merge tree. A second merge while one is in progress is rejected with `CONFLICT`. Five unit tests,
  including an `export_state` / `import_state` round trip.
- **Conformance (`crates/loom-conformance`).** New executable `merge-conflict` suite (declarative
  scenarios + `run_merge_conflict_behavior`) covering conflict-enters-in-progress, abort-restores,
  continue-requires-resolution, and resolve-and-continue; wired into `certify_memory_store` and the
  serialized report taxonomy.
- **C ABI (`crates/loom-ffi`, `include/loom.h`, iOS C header).** `loom_merge_in_progress`,
  `loom_merge_conflicts` (JSON array), `loom_merge_resolve` (resolution `0` ours / `1` theirs / `2`
  working), `loom_merge_abort`, and `loom_merge_continue`, with a happy-path FFI test over a SQL
  same-row table conflict plus the no-merge error paths.
- **IDL (`idl/loom.idl`).** `ConflictResolution` enum and the five methods added to `VersionControl`.
- **Bindings.** Node (`mergeInProgress` / `mergeConflicts` / `mergeResolve` / `mergeAbort` /
  `mergeContinue`) and Python (`merge_in_progress` / `merge_conflicts` / `merge_resolve` / `merge_abort`
  / `merge_continue`), stateless path-based, with reachable-path tests. The conflict happy-path through
  these two bindings awaits projecting the `merge` / `branch` VCS facade into Node and Python (tracked
  separately); the behavior is fully exercised at the core, conformance, and C ABI layers.
- **Remaining target.** The full `MergeOptions` strategy menu, the `Conflict.markers` reporting flag, a
  `status` view of conflicts, line-level (diff3) markers, and merge wrappers in the remaining binding
  families (C++, iOS, JVM, Android, React Native, wasm) remain target work.

### P1 - explicit staging index (status / stage / unstage) promoted to source-backed

The `status` and explicit-index staging P1 (0003 §5.2) is implemented and promoted to source-backed.
Owner decision (option C3): one shared per-workspace index across all facets; `commit` records the whole
working tree (staged plus unstaged) so the common path needs no staging, and `commit_staged` records
only the index for partial commits; cross-facet operations act over the single shared stage. Summary for
re-verification:

- **Core (`crates/loom-core/src/vcs.rs`).** Added a per-workspace `index` (a shared stage spanning all
  facets), persisted through `export_state` / `import_state` (older states default the index to the
  working tree). New public `stage`, `stage_all`, `unstage`, `status` (returning the new `Status` /
  `Change` / `ChangeKind` types), and `commit_staged`. `commit` is unchanged behaviorally (records the
  whole working tree) and now also resets the index to the committed tree; `checkout` resets the index
  too. Five unit tests (partial staged commit, commit-everything, status classification, unstage,
  export/import round trip). No facet/binding commit call sites changed, since `commit` stays the
  everything-snapshot.
- **Conformance (`crates/loom-conformance`).** New executable `staging` suite
  (`run_staging_behavior` + scenarios) covering status classification, `commit_staged` vs `commit`, and
  unstage; wired into `certify_memory_store` and the report taxonomy.
- **C ABI (`crates/loom-ffi`, `include/loom.h`, iOS C header).** `loom_status` (JSON
  `{ staged, unstaged, untracked, conflicts }`), `loom_stage`, `loom_stage_all`, `loom_unstage`, and
  `loom_commit_staged`, with a happy-path FFI test over a SQL-facet working change.
- **IDL (`idl/loom.idl`).** `ChangeKind` / `Change` / `Status` types and the `status` / `stage` /
  `stage_all` / `unstage` / `commit_staged` methods added to `VersionControl`.
- **Bindings.** Node (`stage` / `stageAll` / `unstage` / `statusJson` / `commitStaged`) and Python
  (`stage` / `stage_all` / `unstage` / `status_json` / `commit_staged`), stateless path-based, with
  tests that drive the flow through the SQL facet.
- **Remaining target.** Rename detection and the `Change.old`/`new`/`from` fields; a typed (non-JSON)
  status object in the bindings; the per-workspace staging-mode capability flag; and staging wrappers in
  the remaining binding families (C++, iOS, JVM, Android, React Native, wasm) remain target work.

### P1 - tag operations (create / list / target / delete / rename) promoted to source-backed

The tag-operations P1 is implemented and promoted to source-backed. Owner-approved enterprise decisions:
a shared revision resolver, annotated tags supported (lightweight by default), and a raw `tag_target`
read. Summary for re-verification:

- **Decisions.** (1) `tag_create`/`tag_target` take a revision string resolved with fixed precedence -
  `HEAD` (current branch tip), then a digest (`algo:hex` or bare 64-hex tagged with the store profile),
  then an exact branch name - validated to a commit; `resolve_rev` is a shared primitive that
  restore/rebase/cherry-pick/squash will reuse. (2) Tags are lightweight by default (ref points at the
  commit) and annotated when a non-empty message is given (a stored `Object::Tag` with tagger/message/
  timestamp; the annotation-ready signature is locked now to avoid a later breaking ABI change).
  (3) `tag_target` returns the raw ref target (the commit for lightweight, the tag object for annotated)
  so clone/sync recreate annotated tags without dropping the tag object.
- **Core (`crates/loom-core`).** `workspace.rs` adds registry `tag_delete` / `tag_rename` (git-faithful
  errors). `vcs.rs` adds `resolve_rev` and the engine `tag_create` / `tag_list` / `tag_target` /
  `tag_delete` / `tag_rename`, plus a `parse_rev_digest` helper. Tags live in the persisted registry, so
  they survive `export_state` and transfer via clone/push/bundle; `reachable()` already peels
  `Object::Tag`, so an annotated tag's object and target commit are GC-retained and synced with no new
  plumbing. Three unit tests (lightweight + annotated round trip with reachability and reload; rev
  resolution rules; delete/rename error matrix).
- **Conformance (`crates/loom-conformance`).** New executable `tags` suite (`run_tags_behavior` +
  scenarios) covering lightweight/annotated creation, rev resolution, list/target, rename, delete, and
  the error matrix; wired into `certify_memory_store`, the aggregate report, and the README.
- **C ABI (`crates/loom-ffi`, `include/loom.h`, iOS C header).** `loom_tag_create` (returns the ref
  target digest), `loom_tag_list` (JSON array), `loom_tag_target` (digest + found flag),
  `loom_tag_delete`, and `loom_tag_rename`, with an FFI test.
- **IDL (`idl/loom.idl`).** `tag_create` / `tag_list` / `tag_target` / `tag_delete` / `tag_rename` added
  to `VersionControl`.
- **Bindings.** Node (`tagCreate` / `tagList` / `tagTarget` / `tagDelete` / `tagRename`) and Python
  (snake_case equivalents), with round-trip + annotated tests. (Both binding crates take a crate-level
  `too_many_arguments` allow: these shims mirror the C-ABI argument lists one-to-one.)
- **Remaining target.** Annotated-tag detail readers (message/tagger peel), signed tags, tag operations
  in the C++/iOS/JVM/Android/React Native/wasm binding families, and the `branch`/`merge` facade in
  bindings remain target work.

### P2 - restore and path-restricted checkout (restore_file / restore_path) promoted to source-backed

Restore is implemented and promoted to source-backed, built on the shared revision resolver from the
tags slice. Decisions: restore is slot-level (it restores whatever slot kind the snapshot held - file,
symlink, table, or stream), working-tree-only (`HEAD`, the branch, and the staging index are untouched),
and a path absent in the snapshot is removed from the working tree (git `restore` behavior). Summary for
re-verification:

- **Core (`crates/loom-core/src/vcs.rs`).** `restore_file(rev, path)` resolves `rev` via `resolve_rev`,
  reads the snapshot via `flatten_commit`, and sets or removes the working-tree slot at `path`;
  `restore_path(rev, prefix)` resets the subtree under `prefix` (a `""` prefix restores the whole tree),
  removing working-tree paths absent in the snapshot and resyncing directories. A new
  `ensure_ancestor_dirs` helper keeps a restored deep path's parents present. Two unit tests (file
  revert + remove-when-absent with `HEAD` unchanged; subtree-only restore).
- **Conformance (`crates/loom-conformance`).** New executable `restore` suite (`run_restore_behavior` +
  scenarios) covering revert, remove-when-absent, and subtree-only restore; wired into
  `certify_memory_store`, the aggregate report, and the README.
- **C ABI / IDL / bindings.** `loom_restore_file` / `loom_restore_path` (+ `include/loom.h` and iOS
  header), the `VersionControl` IDL `restore_file` / `restore_path`, and Node (`restoreFile` /
  `restorePath`) + Python (`restore_file` / `restore_path`) wrappers, each with an FFI/binding test.
- **Remaining target.** A `--staged` restore variant (restoring into the index rather than the working
  tree), and restore in the binding families beyond the C ABI, Node, and Python.

### P2 - history replay (cherry-pick / revert / rebase) promoted to source-backed

Cherry-pick, revert, and linear rebase are implemented and promoted to source-backed, on the decisions
recorded in the "history rewrite and per-facet merge strategy" design note above. Recap: the ops are
workspace-level and cross-facet (a commit is a cross-facet snapshot); conflict resolution is per-slot
via the shared `merge_slot` seam; every op takes a `dry_run` that previews structured conflicts without
changing anything; and replay is atomic (the first conflict makes no change). Summary for
re-verification:

- **Core (`crates/loom-core/src/vcs.rs`).** Extracted the merge's per-slot resolution into
  `resolve_three_way` (the `merge_slot` seam: files whole-slot, tables row/cell-level, default
  whole-slot) and routed `merge_inner` through it. Added the public `ReplayOutcome`
  (`Replayed`/`Clean`/`Conflicts`/`Empty`), the internal `replay_onto` driver, and `cherry_pick` /
  `revert` / `rebase`. `cherry_pick` preserves each commit's author and message; `revert` authors as the
  caller with a `Revert "<subject>"` message; `rebase` replays the branch's first-parent commits since
  the merge base onto the target (fast-forwarding when behind). Branch tips advance by compare-and-swap.
  Four unit tests (cherry-pick apply, revert undo, dry-run + atomic conflict, rebase).
- **Conformance (`crates/loom-conformance`).** New executable `replay` suite (`run_replay_behavior` +
  scenarios) covering cherry-pick, revert, rebase, and a dry-run conflict preview; wired into
  `certify_memory_store`, the aggregate report, and the README.
- **C ABI / IDL / bindings.** `loom_cherry_pick` / `loom_revert` / `loom_rebase` (comma-separated commit
  digests, a `dry_run` flag, and an outcome JSON; + `include/loom.h` and iOS header), the
  `VersionControl` IDL `cherry_pick` / `revert` / `rebase` with a `ReplayOutcome` struct, and Node
  (`cherryPick` / `revert` / `rebase`) + Python (`cherry_pick` / `revert` / `rebase`) wrappers returning
  the outcome JSON, each with an FFI/binding test.
- **Remaining target.** A persisted multi-step sequencer (resolve-and-continue across many commits),
  richer per-facet mergers behind the `merge_slot` seam (graph/kv/vector), interactive rebase (reorder /
  squash / edit), and replay in the binding families beyond the C ABI, Node, and Python.

### P3 - squash promoted to source-backed

Squash is implemented and promoted to source-backed, completing the history-rewrite set.

- **Core (`crates/loom-core/src/vcs.rs`).** `squash(onto, author, message, ts)` resolves `onto` via the
  shared resolver, requires it to be an ancestor of the tip and not the tip itself
  (`INVALID_ARGUMENT`), and records one commit whose tree is the tip's tree and whose parent is `onto`,
  advancing the branch by compare-and-swap. The working tree is unchanged (its content already matches
  the tip). One unit test (collapse three commits to one; reject a bad base).
- **Conformance (`crates/loom-conformance`).** New executable `squash` suite (`run_squash_behavior` +
  scenarios) covering the collapse and the bad-base rejection; wired into `certify_memory_store`, the
  aggregate report, and the README.
- **C ABI / IDL / bindings.** `loom_squash` (+ `include/loom.h` and iOS header), the `VersionControl`
  IDL `squash`, and Node (`squash`) + Python (`squash`) wrappers, each with an FFI/binding test.
- **Remaining target.** Combined/interactive squash (selecting and reordering individual commits) and
  squash in the binding families beyond the C ABI, Node, and Python.
