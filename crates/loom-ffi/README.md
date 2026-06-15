# uldren-loom-ffi

The C ABI for Uldren Loom: a `cdylib` + `staticlib` (`libuldren_loom`) exposing the stable contract
that every language binding (Node, JVM, Swift, Kotlin, C/C++, React Native, WASM) wraps.

Part of [Uldren Loom](https://github.com/uldrenai/uldren-loom).

## Build

```bash
cargo build -p uldren-loom-ffi --release   # -> target/release/libuldren_loom.{a,so,dylib,dll}
```

The C header is generated with cbindgen (`just header` -> `include/loom.h`). The surface is
`loom_version`, `loom_blob_digest`, and `loom_string_free`; every string the library returns is
owned by the library and must be freed with `loom_string_free`.

## License

Business Source License 1.1 (BUSL-1.1). See the [repository](https://github.com/uldrenai/uldren-loom).
