# ADR-0003 - Tabular / SQL overlay

**Status:** Accepted · **Date:** 2026-06-14 · **Deciders:** Nas (+ Loom maintainers)
**Superseded in part by:** ADR-0008 (the optional analytical engine is **Polars**, native-gated, not DataFusion).

## Context

The owner asked whether SQL mechanics can be overlaid on Loom - a queryable, **versioned**
relational layer over the content-addressed object model - and whether it belongs in the core
interface (0003) or a separate spec.

## Findings (vetted)

- **The concept is productized, not speculative.** Dolt is "the world's first version-controlled
  SQL database," built on a layered architecture - **SQL Engine → Version Control → Storage** - where
  table data is stored in **prolly trees** (a map of primary key → row), the exact structure Loom
  already adopts for large collections (0002 §4). Versioning a database is therefore just commits
  over table prolly trees, with structural sharing and fast diff/merge between revisions - the same
  properties Loom relies on for directories.
- **In Rust, the query engine is a buy, not a build.** Two mature, embeddable engines expose
  *pluggable storage*, so Loom supplies storage and reuses the planner/executor:
  - **GlueSQL** - a Rust SQL engine whose `Store`/`StoreMut` traits let any backend become a SQL
    database; it already ships `gluesql-git-storage` and `gluesql-idb-storage` (browser/IndexedDB),
    proving the pattern. Lightweight, OLTP-shaped, and WASM-friendly (aligns with our browser story,
    ADR-0001).
  - **Apache DataFusion** - an Arrow-based, embeddable analytical engine whose `TableProvider` trait
    is how Iceberg/Delta and custom stores plug in. Columnar, OLAP-shaped, higher throughput for
    scans/joins.
- **Honest caveat.** Prolly trees are **row-oriented** and content-addressed; Dolt's own engineering
  notes flag columnar/analytical workloads and very high write throughput as known challenges. A
  versioned prolly-tree table is excellent for OLTP, point/range reads, diff/merge, and moderate
  analytics - it is **not** a substitute for a dedicated columnar warehouse.

## Decision

**Adopt the tabular/SQL overlay as an optional capability (`sql` / `tabular`), specified in a new
document (0011), not folded into the core 0003 interface.**

- **Storage:** a *Table* is a versioned prolly-tree map (PK → row) plus a schema object, expressed in
  the existing object model (0002) so it inherits commit/branch/merge/diff/sync for free. Defined in
  0011, referenced from 0002 §3.
- **Query engine:** **embed GlueSQL as the default** (OLTP, lightweight, WASM-capable) via its
  `Store`/`StoreMut` traits over Loom tables; **offer an optional analytical engine** (`sql-olap`)
  for heavy scans. Do **not** write a SQL parser/planner from scratch (reuse `sqlparser-rs`).
  *(The analytical engine was originally DataFusion; **superseded by ADR-0008**, which selects
  **Polars** as a native-only, feature-gated engine and drops DataFusion for its `wasm32`-toolchain
  friction. GlueSQL as the default is confirmed.)*
- **Surface:** a new optional `query`/`db` facade is *pointed to* from 0003 §8.x but lives in 0011;
  the core FS/VCS interface is unchanged. Capability registered in 0010 §4.

### Why a separate spec, not an edit to 0003

The relational surface is large, optional, and conceptually distinct from the file/VCS interface;
keeping the core lean (and conformance level L2 small) while letting the SQL layer evolve on its own
cadence is cleaner. 0003 gains only a brief pointer; the substance is 0011.

## Consequences

- **Positive:** Loom becomes "Dolt-like" - a versioned database - atop the *same* engine that powers
  the versioned filesystem, with diff/merge/branch/sync working uniformly across files *and* tables.
  Reusing GlueSQL/Polars avoids building a query engine.
- **Negative / risks:** GlueSQL's SQL surface and performance are narrower than a full RDBMS; the
  row-oriented prolly model is weak for heavy OLAP (mitigated by the optional native Polars path
  (ADR-0008) and by
  positioning, not by claiming warehouse parity). Adds an engine dependency behind the `sql`
  capability (absent ⇒ `UNSUPPORTED`, never silent).
- **Sequencing:** lands after the core single-file engine and VCS (roadmap 0010 §6, new milestone),
  since it depends on prolly-tree tables and transactions being solid first.
