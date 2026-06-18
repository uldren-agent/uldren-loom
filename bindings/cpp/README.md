# Uldren Loom - C/C++ binding

The most direct binding: it *is* the C ABI. Use `include/loom.h` (C) directly, or the header-only
C++ wrapper `bindings/cpp/include/loom.hpp` for RAII errors, result views, SQL sessions, batches, row
streams, async SQL exec, store creation, workspace lifecycle, queue helpers, key-wrap management, and
direct table/history readers.

Licensed under **BUSL-1.1** (see the repo `LICENSE`).

## Build the example

```bash
# produces target/release/libuldren_loom.{a,so,dylib}
cargo build -p uldren-loom-ffi --release
cmake -S bindings/cpp -B bindings/cpp/build
cmake --build bindings/cpp/build
# prints version + the "abc" digest
./bindings/cpp/build/loom_example
```

Or directly with a compiler:

```bash
c++ -std=c++20 -Ibindings/cpp/include -Iinclude bindings/cpp/example/main.cpp -Ltarget/release -luldren_loom -o /tmp/loom_example
```

The printed digest must match `loom hash` and the canonical `abc` vector (`blake3:314b0f56...4058`).
