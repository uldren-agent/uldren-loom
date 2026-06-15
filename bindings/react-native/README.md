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
