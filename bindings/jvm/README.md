# ai.uldren:loom (JVM binding)

JVM binding over the Uldren Loom C ABI using the Foreign Function & Memory API. **Requires JDK 22+.**

Licensed under **BUSL-1.1** (see the repo `LICENSE`).

## Build

```bash
# 1) build the native C ABI from the repo root:
# -> target/release/libuldren_loom.{so,dylib,dll}
cargo build -p uldren-loom-ffi --release
# 2) build the JVM project (Gradle auto-provisions JDK 22 via the Foojay toolchain resolver):
cd bindings/jvm
DYLD_LIBRARY_PATH="$PWD/../../target/release" ./gradlew build   # Linux: LD_LIBRARY_PATH
```

## API

- `Loom.version()` → `String`
- `Loom.blobDigest(byte[])` → `String`
