# Uldren Loom - Knowledge Transfer (session priming)

Read this first in any new session, then open the target phase in `specs/IMPLEMENTATION-PLAN.md`,
then read that phase's "Review first" specs. This doc is internal (lives in `specs/`, which is not
committed and is never referenced from production code or public docs).

## 1. What Uldren Loom is

A universal, content-addressed, versioned store exposing one interface that behaves as a filesystem,
a git-style version history, and a queryable database (SQL + vectors), packable into a single
portable file. Rust core, thin language bindings over a stable C ABI. Full design in `specs/0001`+.

## 2. Current state (what exists vs not)

- **Built:** BLAKE3-256 digests, the canonical type-tagged object model (`Object::Blob`), the
  `ObjectStore` trait with an in-memory `MemoryStore`, the `loom` CLI (`version`, `hash`), the C ABI
  (`loom_version`, `loom_blob_digest`, `loom_string_free`), conformance blob vectors, and binding
  scaffolds (Node, WASM, Python, JVM, C/C++, iOS/Swift, Android/Kotlin, React Native) that expose only
  `version()`/`blobDigest()`.
- **Not built yet:** persistent on-disk store, Tree/Commit, chunking, workspaces, refs/VCS,
  filesystem facade, interchange, sync, SQL/vector, wire protocols, FUSE. That is what the plan does.

## 3. Repo map

- `crates/loom-core` -> package `uldren-loom-core`, lib imported as `loom_core` (the engine).
- `crates/loom-cli` -> `uldren-loom-cli`, installs the `loom` binary.
- `crates/loom-ffi` -> `uldren-loom-ffi`, the C ABI (`libuldren_loom`, `cdylib`+`staticlib`).
- `crates/loom-conformance` -> `uldren-loom-conformance`, canonical vectors + runner.
- `bindings/{node,wasm,python,jvm,cpp,ios,android,react-native}` - excluded from the cargo workspace;
  each wraps the C ABI; `node`/`wasm`/`python` Cargo crates stay `loom-node`/`loom-wasm`/`loom-python` (`publish = false`).
- `include/loom.h` - generated C header (`just header`). `idl/loom.idl` - language-neutral interface.
- `specs/` - design (internal, uncommitted): `00NN-*.md`, `adr/`, this doc, `IMPLEMENTATION-PLAN.md`,
  `BUSINESS-LEGAL-CHECKLIST.md`.
- `.github/workflows/` - `ci`, `bindings`, `release-plz`, `scorecard`, `cla`, `codeql`.

## 4. Conventions (full list in `AGENTS.md` - that file is the source of truth)

- **Final-quality only:** no placeholders, drafts, stub `TODO`/`phase 2` comments, or session prose
  in shipped files. Files read as if finished.
- **No thematic names in code** (see the mapping in section 8); thematic terms live in `specs/` only.
- **No em-dashes or en-dashes** anywhere - use a plain hyphen. **No emoji.**
- **Edition 2024, MSRV 1.85.** `unsafe` only in `loom-ffi`, every `unsafe` block has a `// SAFETY:`.
- The error `Code` enum, `include/loom.h`, `idl/loom.idl`, and the conformance vectors are a contract
  - change the definition and the vectors together.
- `specs/` is internal/temporary: never reference it from code, `CONTRIBUTING.md`, or public docs.
- Ask the owner (in chat, with context, examples, and a recommendation) before non-trivial decisions.

## 5. Build, test, verify

- `just ci` mirrors GitHub CI (fmt, clippy `-D warnings`, test, `cargo deny`). Run before pushing.
- `just all` does the full local pass (fmt-fix, header, sync-versions, lint, build, test, coverage,
  deny, audit). `just coverage` writes `lcov.info` + an HTML report; gate with `just cov_min=NN`.
- `just test-bindings` builds every binding (each needs its own toolchain; see `bindings/*/README.md`).
- **Do not run `cargo` builds in the synced working folder for throwaway checks** - it pollutes
  `target/` with environment-specific paths (this broke a coverage run once). Use
  `CARGO_TARGET_DIR=/tmp/<name>` for scratch builds; only real, intended builds touch `./target`.
- Toolchain: cargo at `~/.cargo/bin` (source `~/.cargo/env`); `cargo-llvm-cov` needs
  `rustup component add llvm-tools-preview`.

## 6. CI and release

- Versioning is **commit-driven via release-plz** (Conventional Commits): `fix:` -> patch,
  `feat:` -> minor, breaking -> major. There are no changeset files. A `fix:` commit touching a
  crate makes release-plz open a release PR; merging it runs the publish.
- crates.io publishing uses **Trusted Publishing** (OIDC, no token) - see `release-plz.yml`.
- `main` is **protected**: land changes via a branch + PR; required status checks gate the merge.
- Crate names are `uldren-loom-*` (the bare `loom-*` names are taken on crates.io); the import name
  stays `loom_core`.

## 7. How to execute a plan phase

1. Read this doc, then the phase in `specs/IMPLEMENTATION-PLAN.md`, then the phase's "Review first"
   specs.
2. If a spec needs refinement, update the spec and get owner sign-off **before** coding.
3. Implement to the spec. Add/extend conformance vectors for any data-model change.
4. `just ci` (and `just all` for a fuller pass) must be green; update `include/loom.h` via
   `just header` if the C ABI changed.
5. Open a PR to `main`; ensure the required checks pass.

## 8. Glossary -> code-name mapping (specs use the left; code uses the right)

| Spec term  | Code name                         |
| ---------- | --------------------------------- |
| Warp       | object store (`ObjectStore`)      |
| Weft       | working tree / staging            |
| Heddle     | reference store                   |
| Bobbin     | pack / segment                    |
| Selvedge   | journal (write-ahead log)         |
| Shuttle    | sync engine                       |
| Loom       | Loom (the engine; product name)   |
| Provider   | provider (backend)                |

## 9. Active design item: Workspaces (`specs/0014`)

A new core concept under refinement: independent, named, **typed** trees in one Loom (UUIDv4 id +
name + type; default name `Default` per type; isolation so you cannot merge across types or across
two trees of the same type). It changes the object model and is scheduled as plan phase **P3**; it
amends `0002`/`0003`/`0004`/`0005`. Resolve its open-questions section before P3.
