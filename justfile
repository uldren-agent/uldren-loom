# Uldren Loom task runner. Install `just`: https://github.com/casey/just
# `just`           -> list recipes
# `just ci`        -> everything CI runs (fmt, lint, test, deny)
# Cross-platform (bash recipes); on Windows use Git Bash / WSL.
set shell := ["bash", "-cu"]

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

# Lint with warnings denied.
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Run the workspace test suite (unit + integration + doctests + conformance).
test:
    cargo test --workspace

# Fast type-check (no codegen).
check:
    cargo check --workspace --all-targets

# Debug build of the whole workspace.
build:
    cargo build --workspace

# Optimized release build (produces the `loom` binary + libuldren_loom).
build-release:
    cargo build --workspace --release

# Remove all build artifacts: workspace target, each standalone binding crate's target, and the
# per-toolchain outputs (node_modules, native addons, wasm pkg/, cmake build/, gradle build/).
clean:
    cargo clean
    cargo clean --manifest-path bindings/node/Cargo.toml
    cargo clean --manifest-path bindings/wasm/Cargo.toml
    rm -rf bindings/node/node_modules bindings/node/*.node bindings/wasm/pkg bindings/cpp/build bindings/jvm/build bindings/jvm/.gradle
    @echo "clean: workspace + node/wasm/cpp/jvm artifacts removed"

# Dependency license/advisory/source policy.
deny:
    cargo deny check

# Known-vulnerability scan.
audit:
    cargo audit

# Public-API/ABI compatibility guard.
semver:
    cargo semver-checks check-release

# --- artifacts -------------------------------------------------------------
# Build the native C ABI (release): target/release/libuldren_loom.{so,dylib,dll} + .a
ffi:
    cargo build -p loom-ffi --release

# Regenerate the public C header from loom-ffi (requires cbindgen).
header:
    cbindgen --config crates/loom-ffi/cbindgen.toml --crate loom-ffi --output include/loom.h

# Verify include/loom.h matches what cbindgen would generate (CI guard against drift).
header-check:
    cbindgen --config crates/loom-ffi/cbindgen.toml --crate loom-ffi --output /tmp/loom.h.gen
    diff -u include/loom.h /tmp/loom.h.gen && echo "header up to date"

# Sync binding manifest versions to the workspace version (single source of truth).
sync-versions:
    ./scripts/sync-binding-versions.sh

# --- bindings (need their own toolchains) ----------------------------------
# Build the Node addon (@uldrenai/loom) with pnpm.
node:
    cd bindings/node && pnpm install && pnpm run build && pnpm test
# Build the WASM package (@uldrenai/loom-wasm).
wasm:
    cd bindings/wasm && wasm-pack build --target web --release
# Build the JVM binding (needs JDK 22+ and the native lib).
jvm: ffi
    cd bindings/jvm && LD_LIBRARY_PATH="$PWD/../../target/release:${LD_LIBRARY_PATH:-}" ./gradlew build
# Build the C++ example via CMake.
cpp: ffi
    cmake -S bindings/cpp -B bindings/cpp/build && cmake --build bindings/cpp/build

# --- aggregate -------------------------------------------------------------
# CI-faithful gate (no mutation): exactly what GitHub runs on every PR. Use this before pushing.
ci: fmt lint test deny
    @echo "ci: all checks passed"

# Requires cbindgen + cargo-deny + cargo-audit (see docs/DEVELOPMENT.md §3).
# Full local "do everything": format, regen C header, lint, test, release build, deny + audit.
all: fmt-fix header sync-versions lint build-release test deny audit
    @echo "all: format + header + sync-versions + lint + release build + test + deny + audit  ✔"

# Build every language binding (each needs its own toolchain; see bindings/*/README.md).
bindings: node wasm cpp jvm
    @echo "bindings: node + wasm + cpp + jvm built"
