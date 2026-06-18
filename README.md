# Uldren Loom

[![CI](https://github.com/uldrenai/uldren-loom/actions/workflows/ci.yml/badge.svg)](https://github.com/uldrenai/uldren-loom/actions/workflows/ci.yml)
[![License: BUSL-1.1](https://img.shields.io/badge/License-BUSL--1.1-blue.svg)](./LICENSE)
[![Rust 1.85+](https://img.shields.io/badge/Rust-1.85%2B-orange.svg)](https://www.rust-lang.org)
[![Conventional Commits](https://img.shields.io/badge/Conventional%20Commits-1.0.0-fe5196.svg)](https://www.conventionalcommits.org)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/uldrenai/uldren-loom/badge)](https://scorecard.dev/viewer/?uri=github.com/uldrenai/uldren-loom)

> A universal, content-addressed, versioned store - one interface that is a filesystem, a git-style
> version history, and a queryable database (SQL + vectors), packable into a single portable file.

Uldren Loom gives you one coherent interface over a content-addressed Merkle store. Through it you
read and write **files** (`read_file`, `write_file`, `list_directory`), keep **history** the way git
does (`commit`, `branch`, `merge`, `diff`, `rebase`), and query **structured data** - SQL tables and
vector embeddings - all in the same versioned, syncable substrate. A whole repository, history
included, fits in a single `.loom` file you can copy, encrypt, and move; or it can live in a
database or behind a remote service. The Rust engine has binding projects for Node, Python, the JVM,
C/C++, iOS, Android, React Native, and the browser (WASM). MCP and hosted protocol surfaces are target
work described in the specs.

## What you can build

### 1. Memory for AI agents - short-term and long-term

Loom is built to be an agent's memory substrate, not just a file store:

- **Long-term memory** - notes, transcripts, and documents as versioned Markdown/files.
- **Queryable semantic memory** - embeddings stored as **vectors** with nearest-neighbor search.
- **Structured memory** - facts and state in versioned **SQL** tables.

Because it is versioned, an agent can branch its memory to explore a hypothesis and merge back only
what worked; because it is content-addressed, memory is deduplicated and verifiable; because it is
syncable, the same memory moves between agent instances and machines. The target MCP surface lets an
agent read, write, query, and version-control its own memory through standard tools. One portable
`.loom` file becomes a complete, forkable, queryable long-term memory.

### 2. Local-first app data that syncs and merges

Give an application a single embedded data file per user that works fully offline, then synchronizes
across devices the way git synchronizes code - with real branch/merge and conflict handling rather
than last-write-wins. Files, structured rows, and history live together in one place, so an
offline-first notes app, a field-data collector, or a local-first SaaS can ship durable,
conflict-aware multi-device sync without standing up a custom backend.

### 3. A versioned source of truth for configuration and infrastructure

Keep your fleet's configuration, feature flags, and environment manifests in one versioned `.loom`
file instead of scattered YAML and ad-hoc backups. You get git-style history for *data*: branch
`staging`, test a change, `merge` it to `prod`, and roll back instantly if it misbehaves. Every
change is content-addressed, so you can prove exactly what each node ran and `diff` two releases
down to the line. It is embeddable (no server to operate) and syncs to every host, which makes it a
clean backbone for GitOps-style config promotion and audit.

### 4. Logic an AI agent can author and run against its own data

Give an agent not just memory but a place to run logic over it. An agent writes a small program - a
validation rule, a workflow, a derived view - and Loom executes it deterministically and under a
budget against a throwaway branch, so you can see exactly what it changed before anything merges
into `main`. Because programs are content-addressed like the rest of the store, identical logic
deduplicates and every run is replayable and auditable. This is an exploratory capability; the
design is in the spec series ([`specs/0015-execution-and-logic.md`](./specs/0015-execution-and-logic.md)).

## Capabilities

- **AI-native** - target MCP and protocol surfaces so agents can use Loom as tools and memory.
- **Backends** - an entire store in one `.loom` file, in a SQL/KV database, or behind a remote: serve any store over HTTP/2-over-TLS with `loom serve remote` and address it by URL or a secret-free CLI context (see the [CLI README](./crates/loom-cli/README.md#remote-stores)).
- **Compute** (exploratory) - deterministic, metered, capability-scoped (per-facet, per-scope, per-mode), sandboxed execution of content-addressed programs against the store; see [`specs/0015-execution-and-logic.md`](./specs/0015-execution-and-logic.md).
- **Filesystem** - directories, files, byte-range and streaming I/O, move/copy, symlinks.
- **Polyglot** - a Rust core with bindings for Node, WASM, Python, the JVM, C/C++, iOS (Swift), Android (Kotlin), and React Native, behind a stable C ABI.
- **Security** - content integrity by construction, plus encryption at rest and compression.
- **Structured data** - versioned SQL tables and vector search over the same store.
- **Synchronization** - `push`/`pull`/`clone` between any two backends, online or via a bundle file.
- **Version control** - commits, branches, tags, `diff`, three-way `merge`, `rebase`, `squash`.

## Build

Requires the Rust toolchain (`rustup`) and [`just`](https://github.com/casey/just). See
[`docs/DEVELOPMENT.md`](./docs/DEVELOPMENT.md) for full setup, cross-compilation, and bindings.

```bash
just all      # full local pass: format, header, sync-versions, lint, build, test, deny, audit
just test-bindings # build and test the Node, WASM, JVM, and C++ bindings (each needs its own toolchain)
just ci       # format check + clippy + tests + dependency policy
just clean    # remove all build artifacts (workspace + every binding)
```

Packages: crate `uldren-loom`, Node `@uldrenai/loom`, PyPI `uldrenai-loom`, Maven `ai.uldren:loom`.

## License

**Business Source License 1.1** (BUSL-1.1), © Uldren Technologies LLC. Source-available, not
OSI-open: free for internal business use, embedding as infrastructure inside your own product (even
a commercial one), and personal/non-commercial use; a commercial license is required only for a
*Competing Offering* (hosting/SaaS/white-label of Uldren Loom itself). Each version converts to the
permissive **Apache-2.0** four years after its release. See [`LICENSE`](./LICENSE). "Uldren Loom" is
a trademark of Uldren Technologies LLC.

## Documentation

| Document                                       | What's in it                                                           |
| ---------------------------------------------- | ---------------------------------------------------------------------- |
| [`docs/DEVELOPMENT.md`](./docs/DEVELOPMENT.md) | Install the toolchain, build, test, cross-compile, build the bindings. |
| [`AGENTS.md`](./AGENTS.md)                     | Operating notes and conventions for contributors and AI agents.        |
| [`CODE_OF_CONDUCT.md`](./CODE_OF_CONDUCT.md)   | Community expectations.                                                |
| [`CONTRIBUTING.md`](./CONTRIBUTING.md)         | How to contribute, commit conventions, and the CLA.                    |
| [`LICENSE`](./LICENSE)                         | The full Business Source License 1.1 text.                             |
| [`SECURITY.md`](./SECURITY.md)                 | How to report a vulnerability.                                         |
