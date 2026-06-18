# uldren-loom-hnsw

The native HNSW accelerator for the Uldren Loom `vector` facet.

`loom-core`'s exact search is the **cross-platform contract** (same results on native and `wasm32`,
recall 1.0). This crate is the **opt-in native accelerator** for above-threshold corpora: it builds an
HNSW graph (`hnsw_rs`) to *narrow* the candidate set, then **re-scores every candidate exactly** with
the facet's own `Metric::score` and `MetaFilter`, returning hits in the exact deterministic order
(score desc, id tie-break). So float noise can only change candidate recall, never the score or order
of a returned item - the accelerator always agrees with the exact contract.

It is **native-only** (`hnsw_rs` pulls `mmap-rs`/`cpu-time`, which are not wasm-clean) and has **no
dependents in the workspace** - `loom-core` must stay wasm-clean, so it never reaches the browser
build. The workspace gate builds/tests it for the host target only.

Status: the accelerator and its exact-reconciliation are implemented and tested. Wiring the
**threshold auto-switch** (below a count -> exact in `loom-core`; above -> this accelerator)
without pulling `hnsw_rs` into the wasm path is the next step, along with two-level/PQ search.

Licensed under **BUSL-1.1** - the crate embeds the engine (see the repo `LICENSE`).
