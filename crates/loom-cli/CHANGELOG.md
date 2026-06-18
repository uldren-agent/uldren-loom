# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `init`, `put`, `get`, and `stat` commands backed by the persistent single-file (`.loom`) object
  store (`loom-store` `FileStore`): create a store, add/fetch content-addressed Blobs, and
  report object counts. `get` re-verifies object integrity before returning.

## [0.0.1](https://github.com/uldrenai/uldren-loom/compare/uldren-loom-cli-v0.0.0...uldren-loom-cli-v0.0.1) - 2026-06-15

### Other

- Migrate crates.io to use oidc.
