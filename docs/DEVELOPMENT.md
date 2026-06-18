# Development setup

Setup for building, testing, and cross-compiling Uldren Loom. macOS-first (Homebrew); notes for
Linux/Windows inline. For day-to-day work you only need sections 1 and 2; cross-compilation
(section 4) and the language bindings (section 5) are needed only when you work on them.

## 0. Prerequisites

- Xcode Command Line Tools (provides `clang`, the linker, `git`): `xcode-select --install`
- Homebrew: <https://brew.sh>
- pnpm (used for the Node binding)
- FUSE (the CLI's mount backend is on by default):
  - Linux/BSD: nothing to install. `fuser` uses its pure-Rust mount implementation - real FUSE mounting
    with no libfuse, no pkg-config, no dev package (mounting needs the kernel's `/dev/fuse` plus either
    a setuid `fusermount3` or a user workspace, `unshare -Urm --map-root-user`).
  - macOS: FUSE mounting is a kernel extension, so it requires macFUSE, linked at build time:
    `brew install --cask macfuse` and `brew install pkg-config` (the build locates macFUSE via
    pkg-config). The `just` recipes detect macFUSE and build FUSE when present; without it they skip the
    FUSE backend and build the NFS-only CLI, so `just lint`/`test`/`build` still work. The driverless
    macOS mount path is the NFS backend (`loom mount-nfs`), which needs nothing installed (it uses the
    OS NFS client and runs `sudo mount_nfs` for you).

For a smaller NFS-only binary without the FUSE subcommand, build with `just build-no-fuse`
(`--no-default-features --features nfs`).

## 1. Rust toolchain (required)

Install `rustup`; the repo's `rust-toolchain.toml` then auto-installs the pinned `stable` channel
plus `rustfmt` and `clippy` the first time you run cargo in the repo.

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
# then open a new shell, or source the env in the current one:
. "$HOME/.cargo/env"
cargo --version
```

Use `rustup`, not `brew install rust`, so toolchains, targets, and components are managed and the
`rust-toolchain.toml` pin is honored.

## 2. Build and test (validate your setup)

```bash
cd uldren-loom
# run the default gate used before pushing
just ci
# prints the canonical "abc" digest: blake3:314b0f56...4058
printf 'abc' | cargo run --bin loom -- hash -
```

`cargo` creates a local `target/` directory (gitignored).

## 3. Task runner and dev tools

```bash
brew install just
cargo install cargo-binstall
cargo binstall -y cargo-deny cargo-audit cargo-llvm-cov cargo-semver-checks cbindgen cocogitto
# install the local git hooks (commit-message check + fmt on commit)
cog install-hook --all
```

Key recipes (run `just` to list all):

- `just ci` - the gate that mirrors GitHub CI: `fmt` check, `clippy -D warnings`, `test`, and
  `cargo deny`. It does not mutate files. Run it before pushing.
- `just test-integration` - the full integration diagnostic pass. It is slow, may bind local sockets,
  build native artifacts, and run protocol suites, and is not a substitute for `just ci`.
- `just all` - the full local "do everything": `fmt-fix`, `header` (regenerate `include/loom.h`),
  `sync-versions` (propagate the workspace version into the binding manifests), `lint`,
  `build-release` (optimized artifacts), `test`, `deny`, `audit`. It deliberately does not
  also run `check`, `build`, `ffi`, or `header-check`, because those are subsumed: `build-release`
  builds the whole workspace including `loom-ffi`, and `header` regenerates the header that
  `header-check` would only verify.
- Granular: `just build`, `just build-release`, `just check`, `just test`, `just lint`,
  `just fmt-fix`, `just deny`, `just audit`, `just semver`, `just ffi`, `just header`,
  `just header-check`, `just sync-versions`.
- Bindings: `just node`, `just wasm`, `just jvm`, `just cpp`, or `just test-bindings`.
- Cleanup: `just clean` - remove all build artifacts: the workspace target plus every binding's
  toolchain output (`node_modules`, native addons, `wasm/pkg`, `cpp/build`, `jvm/build`).

Test placement rules:

- Unit tests stay in the default `just ci` path and should be fast, deterministic, and local to the process.
- Integration tests that cross a process, socket, daemon, HTTP server, mounted filesystem, protocol
  transcript, native dynamic library, model runtime, or external tool boundary live under
  `crates/<crate>/tests/` and are wired to a `test-*` recipe.
- If a test must stay in `src` to reach private helpers, keep it unit-sized. It must not bind sockets,
  launch daemons, download models, require devices or emulators, or perform production-cost crypto.

## 4. Cross-compilation of the Rust workspace (`uldren-loom-core`, `uldren-loom-cli`, `uldren-loom-ffi`)

Run these from the repository root; they cross-compile the core engine, the `loom` CLI, and the
C ABI. (Cross-compiling the language bindings is different - see section 5.) Add the target first,
or you will see `can't find crate for std / core ... target may not be installed`:

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin \
  aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu \
  aarch64-unknown-linux-musl x86_64-unknown-linux-musl \
  aarch64-pc-windows-msvc x86_64-pc-windows-msvc
```

| Target family       | How (from the repo root)                                       | Extra setup                                        |
| ------------------- | -------------------------------------------------------------- | -------------------------------------------------- |
| macOS (both arches) | `cargo build --release --target x86_64-apple-darwin`           | none; Apple `clang` handles both                   |
| Linux gnu / musl    | `cargo zigbuild --release --target aarch64-unknown-linux-musl` | `brew install zig && cargo install cargo-zigbuild` |
| Windows MSVC        | `cargo xwin build --release --target x86_64-pc-windows-msvc`   | `cargo install cargo-xwin`                         |
| FreeBSD             | built in CI (`vmactions/freebsd-vm`)                           | not practical locally                              |

Notes:

- The glibc-pinned form (`x86_64-unknown-linux-gnu.2.28`) is a `cargo-zigbuild` feature; it still
  requires the base `x86_64-unknown-linux-gnu` target to be installed via `rustup target add`.
- `cargo zigbuild` and `cargo xwin` build the workspace's normal crate types. They cannot build the
  Node binding (a Node-specific `cdylib`); that is why running them in `bindings/node` fails with
  "does not support these crate types". Build bindings with their own tools (section 5).
- While developing, build natively for your Mac; reach for the cross tools only to reproduce a CI
  target. CI builds the whole matrix on native runners.

## 5. Language bindings

Each binding is built and cross-compiled by its own toolchain, not by `cargo zigbuild` or
`cargo xwin`. `just test-bindings` builds every binding for your host platform; per-target prebuilds
are produced in CI (for example, napi-rs builds one `.node` per platform).

### Node - `@uldrenai/loom` (napi-rs)

Requires Node 20 or newer (`brew install node` if needed).

```bash
cd bindings/node
pnpm install
# release build; emits loom.<triple>.node + index.js / index.d.ts
pnpm run build
# prints version + blobDigest("abc")
pnpm test
```

To cross-compile the Node addon, napi-rs takes a target (it drives zig under the hood), after
`rustup target add`:

```bash
pnpm exec napi build --release --target aarch64-unknown-linux-musl
```

### WASM - `@uldrenai/loom-wasm` (wasm-bindgen)

```bash
cargo binstall -y wasm-pack
cd bindings/wasm
# emits pkg/ (js + .wasm + .d.ts)
wasm-pack build --target web --release
```

### Python - `uldrenai-loom` (PyO3 / maturin)

Requires Python 3.9+ and maturin. Work inside a virtualenv so `maturin develop` installs into it:

```bash
python3 -m venv .venv && source .venv/bin/activate # Windows: .venv\Scripts\activate
pip install maturin pytest
cd bindings/python
# builds the abi3 extension and installs uldrenai_loom into the venv
maturin develop --release
# prints/asserts version + blob_digest(b"abc")
python -m pytest
```

CI builds one `abi3` wheel per platform (`maturin build --release`); a single wheel covers
CPython 3.9+.

### JVM - `ai.uldren:loom` (FFM, JDK 22+)

Install any recent JDK to run Gradle (`brew install --cask temurin`); Gradle's Foojay toolchain
resolver downloads JDK 22 for the build, so you do not need to install version 22 yourself.

```bash
# build the native library first
cargo build -p uldren-loom-ffi --release
cd bindings/jvm
# macOS shown; on Linux use LD_LIBRARY_PATH instead of DYLD_LIBRARY_PATH
DYLD_LIBRARY_PATH="$PWD/../../target/release" ./gradlew build
```

### C / C++ - direct over the C ABI

```bash
brew install cmake
cargo build -p uldren-loom-ffi --release
cmake -S bindings/cpp -B bindings/cpp/build
cmake --build bindings/cpp/build
# prints version + the "abc" digest
./bindings/cpp/build/loom_example
```

### Prerequisites for the mobile bindings

- **Swift / iOS:** Xcode or the Swift toolchain (`xcode-select --install`).
- **Android (Kotlin + React Native):** the Android SDK and **NDK** - install via Android Studio,
  Settings -> Languages & Frameworks -> Android SDK -> SDK Tools, tick **NDK (Side by side)** and
  **CMake**. Then export `ANDROID_HOME` and `ANDROID_NDK_HOME`, install `cargo install cargo-ndk`,
  and add the Rust Android targets:

  ```bash
  rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
  ```

The `android/` Gradle wrapper (`gradlew`) is committed, so no separate Gradle install is needed.
Run every command in this section from the repository root.

### iOS / Apple - `UldrenLoom` (Swift, SwiftPM) - `bindings/ios`

The C header is vendored at `bindings/ios/Sources/CUldrenLoom/include/loom.h` and kept in sync by
`just header`. Pass the library path at run time so the test binary can load `libuldren_loom`:

```bash
cargo build -p uldren-loom-ffi --release
cd bindings/ios && DYLD_LIBRARY_PATH="$PWD/../../target/release" swift test # Linux: LD_LIBRARY_PATH
```

For on-device iOS, build `libuldren_loom.a` for the iOS targets and package an `.xcframework` (see
`bindings/ios/README.md`).

### Android - Kotlin Multiplatform over JNI - `bindings/android`

JVM (off Android, for shared testing) - build the native lib, then compile the Kotlin jvm target:

```bash
cargo build -p uldren-loom-ffi --release
cd bindings/android && ./gradlew :compileKotlinJvm
```

Android - build the per-ABI Rust static lib (CMake links it from `target/<triple>/release`), then assemble the AAR:

```bash
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 build -p uldren-loom-ffi --release
cd bindings/android && ./gradlew :assembleRelease
```

### React Native - `@uldrenai/loom-react-native` (TurboModule)

```bash
# iOS calls the C ABI directly; Android uses a JNI bridge (CMake links the static lib from target/).
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 build -p uldren-loom-ffi --release
```

Standard React Native cannot load the Node `.node` addon, so this binding targets `libuldren_loom`
via a TurboModule rather than the napi binding.
