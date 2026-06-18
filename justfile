# Uldren Loom task runner. Install `just`: https://github.com/casey/just
# `just`           -> list recipes
# `just ci`        -> everything CI runs (fmt, lint, test, deny)
# Cross-platform (bash recipes); on Windows use Git Bash / WSL.
set shell := ["bash", "-cu"]

# Minimum line-coverage percent enforced by `just coverage` (0 = report only).
# Override per run, e.g. `just cov_min=80 coverage` or `just cov_min=80 all`.
cov_min := "0"

# Can the default-on FUSE backend be built here? Linux/BSD always can (fuser's pure-Rust mount needs no
# native library); macOS can only when macFUSE is installed (FUSE mounting there is a kernel extension
# that fuser links via pkg-config). When it cannot, the workspace recipes gracefully skip the FUSE crate
# and check the NFS-only CLI instead of failing, so a Mac without macFUSE can still run `just lint`/etc.
have_fuse := `case "$(uname -s)" in Linux|*BSD) echo 1;; *) pkg-config --exists fuse 2>/dev/null && echo 1 || echo 0;; esac`

# Show available recipes.
default:
    @just --list

# --- core checks -----------------------------------------------------------
# Verify formatting (no changes).
fmt:
    cargo fmt --all --check

# Apply formatting.
fmt-fix:
    cargo fmt --all

# Lint library and binary targets with warnings denied. Builds FUSE on Linux/BSD (and macOS with
# macFUSE); where FUSE can't build it skips that crate and lints the NFS-only CLI instead.
lint:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ "{{have_fuse}}" = "1" ]; then
        cargo clippy --workspace --lib --bins -- -D warnings
    else
        echo "lint: macFUSE not found - skipping FUSE backend, linting NFS-only CLI"
        cargo clippy --workspace --lib --bins --exclude uldren-loom-vfs-fuse --exclude uldren-loom-cli -- -D warnings
        cargo clippy -p uldren-loom-cli --lib --bins --no-default-features --features nfs -- -D warnings
    fi

# Run workspace library unit tests. FUSE backend skipped where it can't build (a Mac without macFUSE).
test:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ "{{have_fuse}}" = "1" ]; then
        cargo test --workspace --lib --exclude uldren-loom-hosted-pim
        cargo test -p uldren-loom-cli --bin loom
        just test-hosted-pim-unit
    else
        echo "test: macFUSE not found - skipping FUSE backend, testing NFS-only CLI"
        cargo test --workspace --lib --exclude uldren-loom-vfs-fuse --exclude uldren-loom-cli --exclude uldren-loom-hosted-pim
        cargo test -p uldren-loom-cli --bin loom --no-default-features --features nfs
        just test-hosted-pim-unit
    fi

# Run hosted PIM unit tests that stay in the CI gate while protocol transcripts live in integration diagnostics.
test-hosted-pim-unit:
    cargo test -p uldren-loom-hosted-pim --features http dav::tests::webdav_property_update
    cargo test -p uldren-loom-hosted-pim --features http dav::tests::webdav_precondition
    cargo test -p uldren-loom-hosted-pim --features http dav::tests::caldav_mkcalendar
    cargo test -p uldren-loom-hosted-pim --features http dav::tests::carddav
    cargo test -p uldren-loom-hosted-pim --features http imap::tests::imap_sequence_sets
    cargo test -p uldren-loom-hosted-pim --features http imap::tests::imap_flag_ops
    cargo test -p uldren-loom-hosted-pim --features http imap::tests::imap_idle_update_lines
    cargo test -p uldren-loom-hosted-pim --features http imap::tests::imap_list_pattern_matching
    cargo test -p uldren-loom-hosted-pim --features http imap::tests::imap_base64_decoder

# Run hosted protocol integration tests. This is not a substitute for `just ci`.
test-hosted-integration:
    cargo test -p uldren-loom-hosted-core --features http,tls --test remote_carrier
    cargo test -p uldren-loom-hosted --features integration-tests

# Run hosted PIM protocol integration tests. This is not a substitute for `just ci`.
test-hosted-pim-integration:
    cargo test -p uldren-loom-hosted-pim --features http

# Run hosted and MCP protocol conformance. This is not a substitute for `just ci`.
test-protocol-conformance:
    cargo test -p uldren-loom-mcp --features integration-tests conformance
    cargo test -p uldren-loom-protocol-conformance --features integration-tests

# Run C ABI integration tests. This is not a substitute for `just ci`.
test-ffi-integration:
    cargo build -p uldren-loom-ffi
    cargo test -p uldren-loom-ffi --test abi_contract
    cargo test -p uldren-loom-ffi --features integration-tests

# Run CLI command, daemon, and serving integration tests. This is not a substitute for `just ci`.
test-cli-integration:
    cargo test -p uldren-loom-cli --bin loom --no-default-features --features nfs,integration-tests

# Run daemon socket integration tests. This is not a substitute for `just ci`.
test-store-daemon-integration:
    cargo test -p uldren-loom-store --test daemon_integration
    cargo test -p uldren-loom-store --features integration-tests daemon_request_stream

# Run VFS network listener integration tests. This is not a substitute for `just ci`.
test-vfs-integration:
    cargo test -p uldren-loom-vfs-nfs --test nfs_listener

# Run MCP attached-daemon integration tests. This is not a substitute for `just ci`.
test-mcp-integration:
    cargo test -p uldren-loom-mcp --test attached_daemon

# Run host-toolchain dynamic-library integration tests. This is not a substitute for `just ci`.
test-native-integration:
    cargo test -p uldren-loom-native --test dynamic_library

# Run inference HTTP fixture integration tests. This is not a substitute for `just ci`.
test-inference-integration:
    cargo test -p uldren-loom-inference --test http_fixture

# Lint integration diagnostic targets. This is not a substitute for `just ci`.
lint-integration:
    cargo clippy -p uldren-loom-hosted-core --features http,tls --test remote_carrier -- -D warnings
    cargo clippy -p uldren-loom-hosted --features integration-tests --lib -- -D warnings
    cargo clippy -p uldren-loom-hosted-pim --features http --tests -- -D warnings
    cargo clippy -p uldren-loom-mcp --features integration-tests --lib -- -D warnings
    cargo clippy -p uldren-loom-protocol-conformance --features integration-tests --lib -- -D warnings
    cargo clippy -p uldren-loom-ffi --features integration-tests --tests -- -D warnings
    cargo clippy -p uldren-loom-cli --bin loom --no-default-features --features nfs,integration-tests -- -D warnings
    cargo clippy -p uldren-loom-store --test daemon_integration -- -D warnings
    cargo clippy -p uldren-loom-store --features integration-tests --lib -- -D warnings
    cargo clippy -p uldren-loom-vfs-nfs --test nfs_listener -- -D warnings
    cargo clippy -p uldren-loom-mcp --test attached_daemon -- -D warnings
    cargo clippy -p uldren-loom-native --test dynamic_library -- -D warnings
    cargo clippy -p uldren-loom-inference --test http_fixture -- -D warnings

# Run all integration diagnostics. Slow; may bind sockets, build native artifacts, and run protocol suites. Not a substitute for `just ci`.
test-integration:
    just lint-integration
    just test-hosted-integration
    just test-hosted-pim-integration
    just test-protocol-conformance
    just test-ffi-integration
    just test-cli-integration
    just test-store-daemon-integration
    just test-vfs-integration
    just test-mcp-integration
    just test-native-integration
    just test-inference-integration

# Fast type-check (no codegen). FUSE backend skipped where it can't build (a Mac without macFUSE).
check:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ "{{have_fuse}}" = "1" ]; then
        cargo check --workspace --all-targets
    else
        cargo check --workspace --all-targets --exclude uldren-loom-vfs-fuse --exclude uldren-loom-cli
        cargo check -p uldren-loom-cli --all-targets --no-default-features --features nfs
    fi

# Debug build of the whole workspace. FUSE backend skipped where it can't build (a Mac without macFUSE).
build:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ "{{have_fuse}}" = "1" ]; then
        cargo build --workspace
    else
        cargo build --workspace --exclude uldren-loom-vfs-fuse --exclude uldren-loom-cli
        cargo build -p uldren-loom-cli --no-default-features --features nfs
    fi

# NFS-only build: the CLI with the FUSE backend feature disabled (smaller binary, no FUSE subcommand).
build-no-fuse:
    cargo build -p uldren-loom-cli --no-default-features --features nfs

# Optimized release build (produces the `loom` binary + libuldren_loom). FUSE backend skipped where it
# can't build (a Mac without macFUSE).
build-release:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ "{{have_fuse}}" = "1" ]; then
        cargo build --workspace --release
    else
        cargo build --workspace --release --exclude uldren-loom-vfs-fuse --exclude uldren-loom-cli
        cargo build -p uldren-loom-cli --release --no-default-features --features nfs
    fi

# Optimized FIPS-channel CLI build. Skips FUSE where the host cannot build it.
build-release-fips:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ "{{have_fuse}}" = "1" ]; then
        cargo build -p uldren-loom-cli --release --no-default-features --features nfs,fuse,mcp,fips
    else
        cargo build -p uldren-loom-cli --release --no-default-features --features nfs,mcp,fips
    fi

# Compile and test the FIPS feature path that controls hosted runtime policy.
fips-check:
    cargo test -p uldren-loom-hosted --features fips hosted_runtime_profile_reports_build_policy
    cargo check -p uldren-loom-cli --no-default-features --features nfs,mcp,fips

# Capture release and SBOM input materials for a built channel.
release-materials channel="standard" out="target/release-materials":
    bash scripts/release-materials.sh "{{channel}}" "{{out}}"

# Build the FIPS CLI and capture its release material inputs.
release-materials-fips: build-release-fips
    bash scripts/release-materials.sh fips target/release-materials

# Remove generated local build outputs across the workspace, bindings, and prototypes.
clean:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo clean
    while IFS= read -r manifest; do
        cargo clean --manifest-path "$manifest"
    done < <(find bindings prototypes -name Cargo.toml -print | sort)
    rm -rf \
        target \
        lcov.info \
        bindings/node/*.node \
        bindings/python/python/uldrenai_loom/_native* \
        prototypes/loom-wasm-sql/web/pkg
    find bindings -type d \( \
        -name target -o \
        -name build -o \
        -name .gradle -o \
        -name .cxx -o \
        -name node_modules -o \
        -name pkg -o \
        -name dist -o \
        -name .pytest_cache -o \
        -name .venv -o \
        -name .build \
        \) -prune -exec rm -rf {} +
    find prototypes -type d \( \
        -name target -o \
        -name pkg -o \
        -name dist -o \
        -name node_modules -o \
        -name .gradle -o \
        -name build \
        \) -prune -exec rm -rf {} +
    find . -name '.fuse_hidden*' -delete 2>/dev/null || true
    echo "clean: generated workspace, binding, and prototype build outputs removed"

# Dependency license/advisory/source policy.
deny:
    cargo deny check

# Known-vulnerability scan.
audit:
    cargo audit \
        --no-fetch \
        --no-yanked \
        --stale \
        --ignore RUSTSEC-2023-0071 \
        --ignore RUSTSEC-2024-0370 \
        --ignore RUSTSEC-2024-0436 \
        --ignore RUSTSEC-2025-0141 \
        --ignore RUSTSEC-2026-0194 \
        --ignore RUSTSEC-2026-0195

# Report direct dependencies behind their latest release, including the major bumps a version
# requirement hides (how a 0.x -> 1.x gap like wasmi surfaces; `cargo update` and Dependabot only
# move within the requirement). Needs cargo-outdated: cargo install cargo-outdated
# To then raise requirements to latest (majors included): cargo upgrade --incompatible  (cargo-edit)
# The grep drops cargo-outdated's "could not be found" notes for internal path deps (built from local
# source, so no registry version to compare); `--ignore` only filters table rows, not those notes.
outdated:
    cargo outdated --workspace --root-deps-only 2>&1 | grep -v 'could not be found'

# Coverage: writes lcov.info (for CI/Codecov) and an HTML report to open, and fails when line
# coverage is below `cov_min` (default 0 = report only). Needs cargo-llvm-cov:
# cargo install cargo-llvm-cov
#
# Note: llvm-cov prints a benign "N functions have mismatched data" line - coverage maps that differ
# across instrumented objects in the dependency graph (cargo-llvm-cov FAQ), not a real error. Left
# visible on purpose (no filtering, so cargo keeps its colored output).
coverage:
    cargo llvm-cov --workspace --lib --exclude uldren-loom-hosted-pim --no-report
    cargo llvm-cov report --lcov --output-path lcov.info
    cargo llvm-cov report --html
    cargo llvm-cov report --fail-under-lines {{cov_min}}
    @echo "coverage: wrote lcov.info + target/llvm-cov/html/index.html"

# Public-API/ABI compatibility guard.
semver:
    cargo semver-checks check-release

# --- artifacts -------------------------------------------------------------
# Build the native C ABI (release): target/release/libuldren_loom.{so,dylib,dll} + .a
ffi:
    cargo build -p uldren-loom-ffi --release

# Build the FIPS-channel native C ABI (release).
ffi-fips:
    cargo build -p uldren-loom-ffi --release --features fips

# Capture binding package material inputs for a channel.
binding-release-materials channel="standard" out="target/release-materials":
    bash scripts/binding-release-materials.sh "{{channel}}" "{{out}}"

# Validate binding publication policy without publishing packages or reading registry credentials.
binding-publication-dry-run channel="standard" out="target/release-materials":
    just binding-release-materials "{{channel}}" "{{out}}"
    bash scripts/binding-publication-dry-run.sh "{{out}}/{{channel}}/bindings/binding-release-materials.json" "{{out}}/{{channel}}/bindings"

# Build the FIPS C ABI and capture binding package material inputs.
binding-release-materials-fips: ffi-fips
    bash scripts/binding-release-materials.sh fips target/release-materials

# Regenerate the public C header from loom-ffi (requires cbindgen).
header:
    cbindgen --config crates/loom-ffi/cbindgen.toml --crate uldren-loom-ffi --output include/loom.h
    cp include/loom.h bindings/ios/Sources/CUldrenLoom/include/loom.h

# Verify include/loom.h matches what cbindgen would generate (CI guard against drift).
header-check:
    cbindgen --config crates/loom-ffi/cbindgen.toml --crate uldren-loom-ffi --output /tmp/loom.h.gen
    diff -u include/loom.h /tmp/loom.h.gen && echo "header up to date"
    diff -u bindings/ios/Sources/CUldrenLoom/include/loom.h include/loom.h && echo "ios header in sync"

# Sync binding manifest versions to the workspace version (single source of truth).
sync-versions:
    ./scripts/sync-binding-versions.sh

# --- bindings (need their own toolchains) ----------------------------------
# Build the Node addon (@uldrenai/loom) with pnpm.
node:
    cd bindings/node && pnpm install && pnpm run build && pnpm test
# Build the WASM package and run the browser/worker OPFS runtime suite.
wasm:
    cd bindings/wasm && wasm-pack build --target web --release && node browser-test/run.mjs
# Build the JVM binding (needs JDK 22+ and the native lib).
jvm: ffi
    cd bindings/jvm && LD_LIBRARY_PATH="$PWD/../../target/release:${LD_LIBRARY_PATH:-}" ./gradlew build
# Build the C++ targets and run their CTest suite.
cpp: ffi
    cmake -S bindings/cpp -B bindings/cpp/build && cmake --build bindings/cpp/build && ctest --test-dir bindings/cpp/build --output-on-failure
# Build + test the iOS/Apple Swift package (needs the Swift toolchain / Xcode; builds the lib first).
ios: ffi
    cd bindings/ios && swift test
# Compile the Android binding's Kotlin Multiplatform JVM target and run the host-JNI runtime suite.
android: ffi
    cd bindings/android && ./gradlew :compileKotlinJvm :jvmTest
# Build + test the Python binding (uldrenai-loom, PyO3 via maturin). Creates bindings/python/.venv on
# first run and installs maturin + pytest into it, so no global Python setup is needed.
python:
    cd bindings/python && python3 -m venv .venv && { source .venv/bin/activate 2>/dev/null || source .venv/Scripts/activate; } && unset CONDA_PREFIX && pip install --quiet --upgrade maturin pytest && maturin develop --release && python -m pytest
# Build Node and Python packages, then prove they can share one .loom store.
binding-cross-interop:
    cd bindings/node && pnpm install && pnpm run build
    cd bindings/python && python3 -m venv .venv && { source .venv/bin/activate 2>/dev/null || source .venv/Scripts/activate; } && unset CONDA_PREFIX && pip install --quiet --upgrade maturin && maturin develop --release
    bash scripts/binding-cross-interop.sh
# Build the React Native Android host-app fixture.
react-native-android:
    cd bindings/react-native && npm install
    cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 build -p uldren-loom-ffi --release
    bindings/android/gradlew -p bindings/react-native/host-test/android :app:assembleDebug :app:assembleAndroidTest

# List Android virtual devices available to the local Android emulator.
android-emulators:
    emulator -list-avds 2>/dev/null

# Start an Android emulator by AVD name, e.g. `just android-emulator Medium_Phone_API_35`.
android-emulator avd:
    #!/usr/bin/env bash
    set -euo pipefail
    log_avd="$(printf '%s' "{{avd}}" | tr -c 'A-Za-z0-9_.-' '_')"
    log="${TMPDIR:-/tmp}/loom-android-emulator-${log_avd}.log"
    nohup emulator -avd "{{avd}}" >"$log" 2>&1 < /dev/null &
    pid="$!"
    disown "$pid" 2>/dev/null || true
    echo "android emulator '{{avd}}' started in background pid=$pid log=$log"

# List available iOS simulators.
ios-emulators:
    xcrun simctl list devices available

# Start an iOS simulator by device name or UDID, e.g. `just ios-emulator "iPhone 16"`.
ios-emulator device:
    -xcrun simctl boot "{{device}}"
    open -a Simulator

# Build and run the React Native Android host-app runtime test on a connected device or emulator.
react-native-android-connected: react-native-android
    bindings/android/gradlew -p bindings/react-native/host-test/android :app:connectedAndroidTest

# Run one binding's local sample or runtime fixture. Names: node, wasm, cpp, jvm, android, ios, python, react-native-android.
binding-sample binding:
    #!/usr/bin/env bash
    set -euo pipefail
    case "{{binding}}" in
        node) just node ;;
        wasm) just wasm ;;
        cpp) just cpp && ./bindings/cpp/build/loom_example ;;
        jvm) just jvm ;;
        android) just android ;;
        ios) just ios ;;
        python) just python ;;
        react-native-android) just react-native-android-connected ;;
        react-native) echo "react-native is ambiguous: use react-native-android; no React Native iOS host fixture exists yet"; exit 2 ;;
        *) echo "unknown binding '{{binding}}'"; echo "names: node wasm cpp jvm android ios python react-native-android"; exit 2 ;;
    esac

# Run one binding's release certification test. Heavy device and emulator tests belong here, not in test-bindings.
release-test-binding binding:
    #!/usr/bin/env bash
    set -euo pipefail
    case "{{binding}}" in
        node) just node ;;
        wasm) just wasm ;;
        cpp) just cpp ;;
        jvm) just jvm ;;
        android) just android ;;
        ios) just ios ;;
        python) just python ;;
        react-native-android) just react-native-android-connected ;;
        react-native) echo "react-native is ambiguous: use react-native-android; no React Native iOS host fixture exists yet"; exit 2 ;;
        *) echo "unknown binding '{{binding}}'"; echo "names: node wasm cpp jvm android ios python react-native-android"; exit 2 ;;
    esac

# Verify built-in MCP app bundles in a browser harness.
verify-apps:
    npm --prefix tools/verify-apps run verify

# --- aggregate -------------------------------------------------------------
# CI-faithful gate (no mutation): exactly what GitHub runs on every PR. Use this before pushing.
ci: fmt lint test deny
    @echo "ci: all checks passed"

# Requires cbindgen + cargo-deny + cargo-audit + cargo-outdated (see docs/DEVELOPMENT.md).
# Full local gate. Every step runs and a final summary reports failures.
all:
    #!/usr/bin/env bash
    set +e
    failed=0
    results=()
    for gate in fmt-fix header sync-versions lint build-release test coverage deny audit outdated; do
        echo "==> $gate"
        just "$gate"
        status=$?
        results+=("$gate:$status")
        if [ "$status" -ne 0 ]; then
            failed=1
            echo "all: $gate failed with exit $status"
        fi
    done
    echo "==> summary"
    for result in "${results[@]}"; do
        gate="${result%%:*}"
        status="${result##*:}"
        if [ "$status" -eq 0 ]; then
            echo "ok     $gate"
        else
            echo "failed $gate exit=$status"
        fi
    done
    exit "$failed"

# Build and test every language binding (each needs its own toolchain; see bindings/*/README.md).
test-bindings: node wasm cpp jvm ios android react-native-android python
    @echo "test-bindings: node + wasm + cpp + jvm + ios + android + react-native-android + python built and tested"
