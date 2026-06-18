# Loom MLX Native Runtime Bundles

This directory is the local landing area for optional MLX native runtime bundles.
The generated libraries are ignored by git because they are host-built artifacts,
large, and refreshable independently from the Rust source.

Expected layout after running `scripts/inference-mlx-runtime.sh stage`:

```text
crates/loom-inference/native/mlx/
  aarch64-apple-darwin/
    manifest.txt
    checksums.sha256
    bundle.env
    libjaccl.dylib
    libmlx.dylib
    libmlxc.dylib
    mlx.metallib
```

The script stages every `*.dylib` from the MLX C install prefix so transitive
local runtime dependencies stay beside `libmlxc.dylib`.
`checksums.sha256` records the SHA-256 digest for every staged file except
itself.

A loadable Loom MLX runtime bundle also needs `libloom_mlx_adapter.dylib`. That
library is the Loom-owned ABI layer above MLX C. The raw MLX C bundle is useful
for validation and footprint measurement, but Loom runtime loading must report
`missing-adapter-library` until the adapter library is present.

The main Loom binary must not require this directory to exist. MLX is an optional
Apple runtime profile. Non-Apple builds and default Loom builds must run without
these files.

Use a macOS host with Xcode command line tools to build the bundle. Linux
container or VM build paths are not targets for this Apple `dylib` because the
build links Apple frameworks such as Metal, Foundation, QuartzCore, and
Accelerate.

The script can manage the MLX C source checkout itself:

```sh
scripts/inference-mlx-runtime.sh refresh
scripts/inference-mlx-runtime.sh clean-build
```

Run the manual MLX C smoke after staging a bundle:

```sh
scripts/inference-mlx-smoke.sh
```

The smoke builds a small C program under `target/inference/mlx/smoke`, links it
against the staged `libmlxc.dylib`, `libmlx.dylib`, and `libjaccl.dylib`, then
runs an MLX array operation. Exit code `77` means the libraries loaded but MLX
could not access a Metal device from the current host session.

Defaults:

```text
MLX_C_REPO=https://github.com/ml-explore/mlx-c.git
MLX_C_REF=main
MLX_C_SOURCE=target/inference/mlx/src/mlx-c
LOOM_MLX_BUILD_DIR=target/inference/mlx
```
