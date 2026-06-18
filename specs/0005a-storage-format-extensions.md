# 0005a - Storage Format Extensions

**Status:** Draft target. **Version:** 0.1.0-draft. **Normative only after promotion.**

This sub-spec holds storage-format ideas that are useful for future enterprise operation but are not
part of the current source-backed `.loom` format. The source-backed 0005 contract remains the
single-file page engine implemented by `crates/loom-store::FileStore`.

## Current Source Boundary

Implemented today:

- one `.loom` file with two 4096-byte superblock slots, one journal-ring slot, and a 4096-byte page
  array;
- page-resident object records, region table, free-page map, and object-location B-tree;
- one superblock `reference_digest` anchoring exported engine state;
- per-object compression and encryption frames below the digest boundary;
- native whole-file compaction and in-place segment GC;
- caller-supplied `BackingIo` for browser and in-memory hosts.

Not implemented today:

- (P0) bounded cold-open cost independent of total object count; the current open path walks the
  complete object-location B-tree into an in-memory map and decodes the complete exported engine-state
  object;
- (P2) pack-split sibling files;
- (P2) standalone ref or reflog B-tree regions;
- (P0) signed commit or tag enforcement flags in the file header;
- (P2) pin-retention records;
- (P1) generated binary schemas;
- (P0) public capability metadata embedded in the file - a runtime capability registry now exists
  (`loom_core::capability`, 0010 §5); *embedding* it in the `.loom` header remains target;
- (P2) remote-provider storage regions;
- (P1) additional identity-profile fields beyond stored digest algorithm plus engine-state objects.

## Target Extension Candidates

Each extension needs its own promotion review before it can change 0005:

- **(P0) Scalable cold open:** persist the object count and the roots needed to open a committed
  generation without walking the complete object index. Add logarithmic point lookup and bounded range
  traversal to the page-resident object-location B-tree, retain a bounded locator cache instead of a
  complete in-memory copy, and split exported engine state into independently loadable structured
  roots for workspace registry, references, working trees, staging indexes, open handles, and policy
  state. Preserve lazy per-object integrity verification, crash recovery, encrypted-store behavior,
  and the single-file contract. Promotion requires cold-open benchmarks over object-count and
  engine-state-size axes, bounded-memory assertions, malformed-tree negative tests, and controlled
  migration of pre-release stores to the one resulting format.
  The MCP process-level mitigation is source-backed: unchanged local reads reuse a validated snapshot,
  file metadata supplies a cheap external-change token, missed polling ticks are skipped, and resource
  and tool inventories rebuild only after that token changes. This removes idle and repeated-read
  amplification but does not replace the storage target. A write still requires one writable open and
  the first subsequent read refreshes the snapshot. Implement persisted root metadata first, then
  logarithmic B-tree lookup, independently loadable engine roots, a bounded validated locator/page
  cache, a process-owned local MCP session with external-write detection, coalesced post-write
  invalidation, and latency/open-amplification instrumentation. These are normal open-path
  requirements; correctness and scalability must not depend on garbage collection reducing the file.
- **(P2) Pack-split sibling files:** define naming, atomicity, crash recovery, encryption, copy semantics,
  and sync behavior when object pages move outside the main `.loom` file.
- **(P2) Standalone ref or reflog regions:** define how refs interact with exported engine state,
  workspace registries, crash recovery, protected refs, and sync.
- **(P2) Pins and retention:** define retention records, delete refusal, GC interaction, legal hold, and
  conformance.
- **(P1) Generated binary schema:** define schema ownership, compatibility, canonical diagrams, decoder
  rejection behavior, and cross-language vectors.
- **(P0) Embedded capability metadata:** define whether it is advisory or normative, how it stays honest,
  and how it interacts with 0010 capability reports.
- **(P2) Remote-provider storage regions:** keep this aligned with 0004a, 0008, and authorization before
  any on-disk region is reserved.

## Promotion Requirements

Before any extension becomes part of the main `.loom` format:

- (P0) define exact bytes, recovery rules, versioning, and downgrade behavior;
- (P0) add source implementation and crash tests;
- (P0) update conformance vectors or provider behavior tests where identity or recovery changes;
- (P0) update 0005 to move the promoted extension from target-only to source-backed;
- (P1) document migration behavior for pre-release stores, even if backwards compatibility is not
  required.
