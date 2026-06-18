# ai.uldren:loom-android (Android binding, Kotlin Multiplatform)

The **Android** binding over the Uldren Loom C ABI via a JNI shim, written in Kotlin Multiplatform.
One `Loom` API across two targets:

- **`androidTarget`** - the Android library (AAR); the JNI shim is built per ABI by the NDK. This is
  the binding's reason for existing.
- **`jvm`** - the same Kotlin source compiled for the desktop/server JVM (off Android), as a
  convenience for shared testing; it loads the same JNI shim from `java.library.path`. For a
  standalone Java/server target, use the dedicated `bindings/jvm` (Panama/FFM) binding instead.

Because JNI also runs on the desktop JVM, the identical binding works on and off Android.

Licensed under **BUSL-1.1** - the binding embeds the engine (see the repo `LICENSE`).

## Build

The Gradle wrapper (`gradlew`) is committed. For the Android target you also need the NDK,
`cargo install cargo-ndk`, and the Rust Android targets (see `docs/DEVELOPMENT.md`). Run the
commands below from the repository root.

JVM (off Android):

```bash
cargo build -p uldren-loom-ffi --release
cd bindings/android && ./gradlew :compileKotlinJvm
```

Android (CMake statically links the per-ABI Rust lib from `target/<triple>/release`, so there is no
copy step):

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 build -p uldren-loom-ffi --release
cd bindings/android && ./gradlew :assembleRelease
```

> The plugin versions in `build.gradle.kts` (Kotlin, Android Gradle Plugin) are starting points;
> align them with your toolchain.

## API

- `Loom.version(): String`
- `Loom.blobDigest(data: ByteArray): String`
- `Loom.create(path, profile, suite?, passphrase?)`
- `Loom.createWithKek(path, profile, kek, suite?)`
- `LoomSql` - SQL session with typed `exec`, `execJson`, `execBytes`, `query`, `commit`, and `close`.
- `LoomSqlBatch` - held-open batch with `exec`, `execBytes`, `commit`, `commitVcs`, `abort`, and
  `close`.
- `LoomResult` and `LoomRowStream` - typed result and streaming row views over the C ABI result
  decoder.
