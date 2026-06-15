# @uldrenai/loom (Node.js binding)

napi-rs binding over the Uldren Loom Rust core. Ships prebuilt native binaries per platform; falls
back to the WASM build (`@uldrenai/loom-wasm`) where no prebuilt exists.

Licensed under **BUSL-1.1** - the binding embeds the engine (see the repo `LICENSE`).

## Build (Node ≥ 20)

```bash
pnpm install
pnpm run build # release; emits loom.<triple>.node + index.js / index.d.ts
pnpm test      # prints version + blobDigest("abc")
```

`pnpm test` prints the same digest as `loom hash` (`blake3:314b0f56…4058`).

## API

- `version(): string`
- `blobDigest(bytes: Uint8Array | Buffer): string`
