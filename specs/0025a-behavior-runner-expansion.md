# 0025a - Behavior Runner Expansion

**Status:** Draft target extension. **Version:** 0.1.0. **Normative target.**

This document owns behavioral conformance expansion beyond the current source-backed runners in 0025.
It does not change current certification: source-backed executable runners are defined by
`EXECUTABLE_BEHAVIOR_SUITES` in `crates/loom-conformance`.

## Current source boundary

Current source provides:

- `run_cas_behavior`;
- `run_cas_facade_behavior`;
- `run_workspace_behavior`;
- `run_sync_behavior`;
- `run_queue_behavior`;
- `run_consumer_behavior`;
- executable facade and subsystem runners for VCS diff, lock, identity, ACL, KV, ephemeral KV,
  document, time-series, ledger, graph, vector, columnar, search, calendar, contacts, mail,
  merge-conflict, staging, file operations, file handles, symlink, tags, restore, replay, and squash;
- local coordination evidence inventory in the conformance report for daemon, transport, host-native
  binding, MCP-attached, unsupported mobile/browser, and unsupported hosted lock surfaces;
- scenario inventory for files, VCS, and every executable suite.

Scenario inventory is not certification.

## Target runner tracks

### Files and workspace history

Add executable runners only for operations promoted by 0003 and 0003b:

- (P0) whole-file reads and writes;
- (P0) directory lifecycle;
- (P0) commit, checkout, branch, log, diff, and merge;
- (P2) target history operations such as status, staging, tags, rebase, cherry-pick, revert, squash,
  restore, and merge continuation only after source implements them.

File-style facet projection under `.loom/facets` waits for 0014a and each owning facet's projection
contract.

### SQL and table history

Add SQL behavior only after 0011 settles the promoted public SQL surface:

- (P1) create, insert, select, delete, and update where supported;
- (P1) transaction and session behavior;
- (P0) table identity;
- (P1) index scan behavior;
- (P1) table diff and blame;
- (P2) row merge and conflict reporting;
- (P2) historical reads when source-backed.

### Data facets

Add runners for graph, vector, ledger, KV, document, time-series, columnar, and public CAS projection
only after each owning spec promotes a stable public facade. A substrate or scenario table alone is not
enough.

### Provider variants

Add provider-backed behavior variants where the behavior depends on persistence or store profile:

- (P0) reopen behavior;
- (P0) crash recovery;
- (P0) corruption detection;
- (P1) GC and compaction;
- (P0) encryption and key-source behavior;
- (P1) browser backing behavior.

### Bindings and protocols

Binding and protocol behavior runners wait for public projection:

- (P1) binding runners after 0007 selects runtime gates for the relevant platform;
- (P1) protocol runners after 0008 implements the hosted surface;
- (P0) principal-aware runners after 0026-0028 implement identity and authorization.

## Promotion checklist

1. (P0) Public facade or provider surface exists.
2. (P0) Expected failures map to stable `Code` values.
3. (P0) Runner executes against the source-backed surface users call.
4. (P1) MemoryStore and FileStore coverage are both considered where persistence matters.
5. (P1) Binding or protocol runners are added only after projection exists.
6. (P0) 0010 capability proof status is updated in the same change.

## Resolved decisions

1. **Scenario inventory remains useful but non-certifying.** It can guide future runners without being
   counted as proof.
2. **Runner expansion follows facade promotion.** No behavior suite becomes executable before the
   source-backed public surface exists.
3. **Projection behavior waits for projection specs.** Binding, hosted protocol, MCP, and FUSE
   conformance belong after 0007, 0008, and the owning facade specs are source-backed.
