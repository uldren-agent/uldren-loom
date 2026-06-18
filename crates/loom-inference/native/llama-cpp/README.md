# Loom llama.cpp Native Runtime Bundles

This directory is the local landing area for optional llama.cpp native runtime
bundles. Generated libraries are ignored by git because they are host-built
artifacts, large, and refreshable independently from the Rust source.

Expected layout after running `scripts/inference-llama-cpp-runtime.sh stage`:

```text
crates/loom-inference/native/llama-cpp/
  aarch64-apple-darwin/
    manifest.txt
    checksums.sha256
    bundle.env
    libllama.dylib
    libggml.dylib
    libggml-base.dylib
    libggml-cpu.dylib
```

Linux bundles use `.so` libraries. Windows bundles use `.dll` libraries.
`checksums.sha256` records the SHA-256 digest for every staged file except
itself.

A loadable Loom llama.cpp runtime bundle also needs the Loom adapter library:
`libloom_llama_cpp_adapter.dylib`, `libloom_llama_cpp_adapter.so`, or
`loom_llama_cpp_adapter.dll` depending on the host. That library is the
Loom-owned ABI layer above upstream llama.cpp. A raw upstream llama.cpp bundle is
useful for validation and footprint measurement, but Loom runtime loading must
report `missing-adapter-library` until the adapter library is present.

The main Loom binary must not require this directory to exist. llama.cpp is an
optional GGUF runtime profile. Default Loom builds must run without these files.

The script can manage the llama.cpp source checkout itself:

```sh
scripts/inference-llama-cpp-runtime.sh refresh
scripts/inference-llama-cpp-runtime.sh clean-build
```

Defaults:

```text
LLAMA_CPP_REPO=https://github.com/ggml-org/llama.cpp.git
LLAMA_CPP_REF=master
LLAMA_CPP_SOURCE=target/inference/llama-cpp/src/llama.cpp
LOOM_LLAMA_CPP_BUILD_DIR=target/inference/llama-cpp
LLAMA_CPP_PREFIX=target/inference/llama-cpp/install
```
