# uldren-loom-core

The Uldren Loom engine: a content-addressed object model (BLAKE3-256 digests with a canonical,
type-tagged encoding) and the `ObjectStore` provider trait, with an in-memory implementation.

This is the core that the `loom` CLI, the C ABI (`uldren-loom-ffi`), and every language binding build
on. Part of [Uldren Loom](https://github.com/uldrenai/uldren-loom) - a universal, content-addressed,
versioned store that behaves as a filesystem, a git-style history, and a queryable database.

## Usage

```bash
cargo add uldren-loom-core
```

The library is imported as `loom_core`:

```rust
use loom_core::Object;

// The content address ("algo:hex") of a blob.
let address = Object::Blob(b"hello".to_vec()).digest().to_string();
assert!(address.starts_with("blake3:"));
```

## License

Business Source License 1.1 (BUSL-1.1). See the [repository](https://github.com/uldrenai/uldren-loom).
