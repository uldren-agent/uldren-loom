# 0006a - Live Sync and Remotes

**Status:** Draft target. **Version:** 0.1.0-draft. **Normative only after promotion.**

This sub-spec holds live, hosted, and remote synchronization work that is not part of the current
source-backed 0006 engine. The source-backed 0006 contract remains direct in-process workspace clone,
fast-forward branch push, and v4 offline bundles.

## Current Source Boundary

Implemented today:

- direct `clone_workspace` between two `Loom` values;
- direct fast-forward-only `push_branch`;
- full workspace `Bundle` encode/decode/export/import;
- digest-profile equality checks;
- reachable object transfer with destination deduplication;
- branch and tag publication only after reachable objects are present;
- CLI local-file `clone`, `bundle-export`, and `bundle-import`.

Not implemented today:

- (P1) hosted remotes;
- (P1) remote-tracking refs;
- (P1) fetch and pull protocol sessions;
- (P1) live push over transport;
- (P0) authenticated remote refs;
- (P1) resumable sessions;
- (P2) incremental bundles with base requirements;
- (P2) shallow or partial clone;
- (P3) delta transfer;
- (P3) set reconciliation acceleration;
- (P0) signed sync manifests;
- (P1) protocol conformance for REST, JSON-RPC, gRPC, WebSocket, MCP, FUSE, or server-local transports.

## Target Sync Contract

The enterprise sync contract adds live negotiated sessions around the current object/ref semantics:

- (P0) protocol handshake with version, identity profile, advertised refs, authenticated principal context,
  authorization filter, and capability set;
- (P1) workspace-scoped refspecs for fetch, pull, push, and clone;
- (P0) remote-tracking refs and authenticated remote ref workspaces;
- (P1) resumable sessions with durable progress records;
- (P1) missing-object reports for interrupted or partial transfers;
- (P2) incremental bundles with base requirements;
- (P0) optional signatures over bundle manifests and ref tips;
- (P1) transport frames for gRPC, HTTP, WebSocket, MCP, FUSE, and local files;
- (P2) shallow and partial clone with backfill guarantees;
- (P3) delta transfer and set reconciliation as transport optimizations;
- (P0) policy-controlled force pushes;
- (P0) conformance tests for every promoted operation.

## Promotion Requirements

Before live sync or remotes move into 0006:

- (P1) define hosted protocol projection in 0008;
- (P0) define principal context and served write authorization through 0026-0028;
- (P0) define protected-ref and force-push policy against `CONFLICT-RESOLUTION-MATRIX.md`;
- (P0) define capability reporting and sync conformance levels through 0010 and 0025;
- (P1) define remote refs, tracking refs, resumability, retry, and failure recovery;
- (P0) add source implementation, CLI or binding projection, protocol projection, and conformance
  tests;
- (P0) update 0006 to move promoted behavior from target-only to source-backed.
