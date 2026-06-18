# @uldrenai/loom-react-native (React Native binding)

React Native binding for Uldren Loom, implemented as a **TurboModule** (new architecture) over the
C ABI: iOS calls `include/loom.h` directly, Android calls it through a small JNI bridge. Standard
React Native cannot load the Node `.node` addon, so this binding targets `libuldren_loom` instead.

Licensed under **BUSL-1.1** - the binding embeds the engine (see the repo `LICENSE`).

## Layout

- `src/NativeUldrenLoom.ts` - the codegen TurboModule spec; `src/index.ts` - the public JS/TS API.
- `ios/UldrenLoom.{h,mm}` + `UldrenLoom.podspec` - the iOS module over the C ABI.
- `android/` - the Android library: `src/main/cpp/UldrenLoom.cpp` (JNI), `CMakeLists.txt`, and the Kotlin module/package.

## Native library

Build the Uldren Loom C ABI for each platform (run from the repo root). The Android JNI bridge
statically links the per-ABI Rust lib from `target/<triple>/release`, so no copy step is needed:

```bash
# Android: add the Rust targets, then build per ABI with cargo-ndk.
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 build -p uldren-loom-ffi --release
# iOS: build the static lib per target, then lipo/xcframework into ios/.
cargo build -p uldren-loom-ffi --release --target aarch64-apple-ios
```

> This is a new-architecture (TurboModule) scaffold; align the codegen spec name, `react-native`
> peer version, and `compileSdk`/Gradle plugin versions with your app before building.

## API

- `version(): string`
- `blobDigest(bytes: Uint8Array | number[]): string`

The SQL methods are **async** (each resolves a `Promise`). The engine has no worker pool of its own,
so the native module dispatches every call to a background queue (iOS) or thread pool (Android) and
resolves off the JS thread - the JS thread never blocks on engine work.

- `sqlExec(loomPath, ns, db, sql): Promise<LoomStatement[]>` - write-capable **typed** results: an array of
  statement objects with idiomatic, lossless cells. The native layer returns lossless bridge JSON
  (decoded once in Rust, since the RN bridge can't carry `BigInt`/`Uint8Array`) and the binding
  `JSON.parse`s it - no CBOR is decoded in JS. Big ints / decimals / uuid / inet / bytes(base64) /
  `f32` / non-finite `f64` / point arrive as single-key tagged objects (`$i64`, `$decimal`, `$bytes`,
  ...); see `LoomCell` for the full set.
- `sqlBatch(loomPath, ns, db, statements): Promise<LoomStatement[]>` - run a list of statements as one
  **atomic transaction/batch** in a single native round-trip: the native layer opens a
  held-open batch, runs each statement in order (including `BEGIN`/`COMMIT`/`ROLLBACK`), and on success
  commits with one atomic save; any error aborts and discards every change. The writer lock stays
  entirely inside native code, off the JS thread. Resolves the typed results of the **final** statement.
- `sqlExecBytes(loomPath, ns, db, sql): Promise<Uint8Array>` - result payloads as canonical-CBOR
  bytes from the write-capable exec path, the raw wire form for your own decoding.
- `sqlQueryBytes(loomPath, ns, db, sql): Promise<Uint8Array[]>` - read-only row streaming; resolves one
  canonical-CBOR row byte array per row and rejects mutating statements.
- `sqlExecJson(loomPath, ns, db, sql): Promise<string>` - a JSON array of result payloads (debug/admin
  form, not type-faithful) from the write-capable exec path.
- `sqlCommit(loomPath, ns, db, message, author): Promise<string>` - the new commit's content address.

Single `sqlExec`/`sqlExecBytes`/`sqlExecJson`/`sqlQueryBytes`/`sqlCommit` calls open the loom, run,
and close (the engine's per-op model); no native handle is held across the JS bridge. `sqlBatch` holds
the writer lock for the duration of one native call only - never across the bridge. Interactive
cross-call transactions (app-code branching between statements inside one transaction) are
intentionally not exposed; the core/C ABI can support a held-open handle, but that needs a guarded
native handle registry and a concrete consumer first.
