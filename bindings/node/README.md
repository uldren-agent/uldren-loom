# @uldrenai/loom (Node.js binding)

napi-rs binding over the Uldren Loom Rust core. The package declares native prebuild targets; the
published optional-package and WASM fallback flow is target packaging work.

Licensed under **BUSL-1.1** - the binding embeds the engine (see the repo `LICENSE`).

## Build (Node >= 20)

```bash
pnpm install
pnpm run build # release; emits loom.<triple>.node + index.js / index.d.ts
pnpm test      # prints version + blobDigest("abc")
```

`pnpm test` prints the same digest as `loom hash` (`blake3:314b0f56...4058`).

## API

- `version(): string`
- `blobDigest(bytes: Uint8Array | Buffer): string`
- `class LoomSql` - a SQL session over a workspace SQL facet in a `.loom`, exposing the whole versioned
  tabular + SQL stack:
  - `new LoomSql(loomPath, nsName, db)` - open (the workspace is created on first use).
  - `exec(sql): Statement[]` - run SQL; returns **typed** results: an array of `{ kind, ... }` statement
    objects. A `select` carries `columns` and `rows` of idiomatic cells (`BigInt` for 64/128-bit
    integers, `number` for <=32-bit integers and floats, `Uint8Array` for bytes, `string`, `boolean`,
    `null`, and `{ mantissa: BigInt, scale }` for an exact decimal). Mutations are staged and persisted.
  - `execBytes(sql): Uint8Array` - the canonical-CBOR result payload; the type-faithful wire
    form for your own decoding. `execAsync(sql): Promise<Uint8Array>` runs it off the event loop.
  - `execJson(sql): string` - a JSON array of result payloads (debug/admin form, not type-faithful).
  - `commit(message, author): string` - commit the staged state; returns the commit's content address.

```js
const { LoomSql } = require('@uldrenai/loom')
const db = new LoomSql('app.loom', 'app', 'main')
db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
db.exec("INSERT INTO t VALUES (1, 'hello')")
const [{ rows }] = db.exec('SELECT id, v FROM t') // rows === [[1n, 'hello']]
db.commit('seed', 'you@example.com')
```
