# @uldrenai/loom-wasm (WASM binding)

The Uldren Loom Rust core compiled to `wasm32` via wasm-bindgen - the browser and edge path.

Licensed under **BUSL-1.1** (see the repo `LICENSE`).

## Build

```bash
rustup target add wasm32-unknown-unknown
wasm-pack build --target web --release
# emits pkg/ (js + .wasm + .d.ts)
```

## API

- `version(): string`
- `blob_digest(bytes: Uint8Array): string`
- `runtime_profile(): Uint8Array`
- `conformance_expected(): string`
- `conformance_digest(): string`

`runtime_profile()` returns the same canonical-CBOR runtime report shape as native bindings. For the
browser package it is compatibility evidence for the linked WASM artifact and not a native FIPS
certification claim.

On `wasm32` builds, the OPFS SQL module also exports `LoomSql`:

- `LoomSql.open(path, ns, db)`
- `LoomSql.open_encrypted(path, ns, db, passphrase)`
- `LoomSql.open_with_kek(path, ns, db, kek)`
- `LoomSql.create(path, ns, db, profile, passphrase?, suite?)`
- `LoomSql.create_with_kek(path, ns, db, profile, kek, suite?)`
- `LoomSql.open_read(bytes, ns, db)`
- `exec(sql)`, `exec_json(sql)`, `exec_bytes(sql)`, `query(sql)`, `commit(message, author)`

On `wasm32` builds, the OPFS management module also exports `LoomStore`:

- `LoomStore.open(path)`
- `LoomStore.open_encrypted(path, passphrase)`
- `LoomStore.open_with_kek(path, kek)`
- `LoomStore.create(path, profile, suite?, passphrase?)`
- `LoomStore.create_with_kek(path, profile, suite?, kek)`
- `authenticate_passphrase(principal, passphrase)`
- `acl_list_json()`, `acl_grant_scoped(...)`, `acl_revoke_scoped(...)`
- `protected_ref_list_json(workspace)`, `protected_ref_get_json(workspace, ref)`
- `protected_ref_set(...)`, `protected_ref_remove(workspace, ref)`
