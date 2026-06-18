# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Prolly-tree sharding under `Tree`: a directory over `DIR_SHARD_THRESHOLD` (256) entries
  is stored as a prolly tree whose shard nodes are themselves `Tree` objects - interior nodes hold
  `EntryKind::TreeShard` entries, leaf nodes hold ordinary entries - so **no new object type** is
  introduced (the five object types are unchanged). Boundaries are content-defined (`blake3(name ||
  level)`), so the structure is a pure function of the entry set; checkout/diff resolve sharding
  transparently and the existing sync/GC reachability walk follows `TreeShard` edges, giving
  O(changed) transfer on an edit.
- Row-level prolly storage for the tabular facet: `Table::build_rows` / `load_rows` /
  `get_row` store rows in a prolly tree keyed by an **order-preserving** primary-key encoding, with
  point lookups and structural sharing. (Committed tables still ride the whole-table file blob;
  promoting them to the `TABLE`-entry form awaits a structured working-tree slot.)

## [0.0.1](https://github.com/uldrenai/uldren-loom/compare/uldren-loom-core-v0.0.0...uldren-loom-core-v0.0.1) - 2026-06-15

### Other

- Migrate crates.io to use oidc.
