# UldrenLoom (Swift binding)

Swift binding over the Uldren Loom C ABI - the path for macOS, iOS, and any Swift project. It wraps
`include/loom.h` (`libuldren_loom`) through Swift's C interop.

Licensed under **BUSL-1.1** - the binding embeds the engine (see the repo `LICENSE`).

## Build (macOS / Swift on the host)

```bash
# 1) build the native C ABI from the repo root:
#    -> target/release/libuldren_loom.{dylib,a}
cargo build -p uldren-loom-ffi --release
# 2) build + test the Swift package (pass the lib path so the test binary can load it at run time)
cd bindings/swift
DYLD_LIBRARY_PATH="$PWD/../../target/release" swift test   # Linux: LD_LIBRARY_PATH
```

The C header is vendored at `Sources/CUldrenLoom/include/loom.h` and kept in sync by `just header`,
so the package builds without referencing files outside its root.

`swift test` checks `version()` is present and that `blobDigest("abc")` has the canonical
`blake3:` + 64-hex shape (the exact vector lives in `uldren-loom-conformance`).

## iOS

Build the static library (`libuldren_loom.a`) for each iOS architecture (e.g. with
`cargo build -p uldren-loom-ffi --release --target aarch64-apple-ios` and the simulator targets),
combine them into an `.xcframework`, and reference that from `Package.swift` as a `binaryTarget`
instead of the host `-L` flag. The Swift source (`Loom`) is unchanged.

## API

- `Loom.version() -> String`
- `Loom.blobDigest(_ data: Data) -> String`
