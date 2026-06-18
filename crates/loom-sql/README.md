# uldren-loom-sql

The SQL frontend for Uldren Loom: GlueSQL over the `loom-core` tabular substrate.

`LoomSqlStore` implements GlueSQL's `Store` / `StoreMut` (the other storage traits use their
defaults), so `gluesql_core::prelude::Glue` runs `CREATE TABLE` / `INSERT` / `SELECT` / `DELETE`. The
whole database snapshots into a workspace SQL facet and **versions through the engine**
(`persist` / `load`) - commit, branch, checkout, and sync apply to it like any other Loom data.

GlueSQL is pure-Rust and wasm-capable, so SQL behaves identically on native and `wasm32`. The
storage traits are `async`, but the bodies are synchronous (ready futures), so no async runtime is
needed inside the store.

Status: whole-database snapshot granularity. The row-level refinement (mapping each SQL table onto a
`loom_core::tabular::Table` for prolly-tree row diff/merge) is future work.
