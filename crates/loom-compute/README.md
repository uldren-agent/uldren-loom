# uldren-loom-compute

The Uldren Loom compute layer: the **capability model** and the program **manifest** for executable
logic over the store.

This crate is the durable compute facet core currently in the workspace:

- `capability` - the fine-grained capability model a program declares: `Capability` (which facet),
  `Scope` (which part), `Mode` (read/write/both), and a `GrantSet` the host enforces per operation.
- `manifest` - the content-addressed program manifest: name, engine/ABI, entry point, declared
  grants, schema digests, and body digest, with a deterministic length-prefixed encoding (a program's
  identity is its manifest digest).

- `engine` - the WASM execution substrate for the files facet. wasmi is the default and the only
  `wasm32` engine; the optional `engine-wasmtime` feature selects the native Wasmtime fast path.
- `gate` - the run-on-a-branch verification gate over `loom-core::vcs`.

Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.
