# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Initial compute layer: the fine-grained capability model (`Capability`, `Scope`, `Mode`,
  `Grant`, `GrantSet`), the content-addressed program `Manifest` with a deterministic encoding, the
  files-facet WASM engine, and the run-on-a-branch gate over `loom-core::vcs`.
