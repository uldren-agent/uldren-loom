# uldren-loom-cli

The Uldren Loom command-line tool. Installs the `loom` binary.

Part of [Uldren Loom](https://github.com/uldrenai/uldren-loom) - a universal, content-addressed,
versioned store.

## Install

```bash
cargo install uldren-loom-cli   # installs the `loom` binary
```

## Usage

```bash
loom version              # print version information
loom hash path/to/file    # print the Blob content address ("blake3:..."); use - for stdin
```

## License

Business Source License 1.1 (BUSL-1.1). See the [repository](https://github.com/uldrenai/uldren-loom).
