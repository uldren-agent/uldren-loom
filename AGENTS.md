# AGENTS.md

Operating notes for working in this repo. If anything here conflicts with the configs (`Cargo.toml`,
`rust-toolchain.toml`, `rustfmt.toml`, `clippy.toml`, `deny.toml`, `justfile`), the configs win - fix
the drift, don't paper over it.

## What this repo is

Uldren Loom is a universal, content-addressed, versioned filesystem: one language-neutral interface
that behaves like a filesystem, a version-control repository, and (optionally) a versioned SQL
database, over a content-addressed Merkle data model. A Rust core is exposed to other languages
through a stable C ABI.

A Cargo workspace of four crates, plus language bindings built with their own toolchains.

- `uldren-loom-core` (`crates/loom-core`) - the engine: digests, the canonical object model, and the
  `ObjectStore` provider trait with an in-memory implementation. No `unsafe`. Rust import name stays `loom_core`.
- `uldren-loom-cli` (`crates/loom-cli`) - the `loom` binary (the crate is `uldren-loom-cli`; the installed binary is `loom`).
- `uldren-loom-ffi` (`crates/loom-ffi`) - the C ABI (`cdylib` + `staticlib`, `libuldren_loom`). The only crate permitted to use `unsafe`.
- `uldren-loom-conformance` (`crates/loom-conformance`) - canonical test vectors and a generic runner that every backend must pass.

## Repo map

- `crates/` - the workspace crates listed above.
- `bindings/` - language bindings, each with its own toolchain, excluded from the cargo workspace:
  `node/` (napi-rs → `@uldrenai/loom`), `wasm/` (wasm-bindgen → `@uldrenai/loom-wasm`),
  `jvm/` (FFM/Panama, JDK 22+ → `ai.uldren:loom`), `cpp/` (header + CMake sample),
  `swift/` (SwiftPM, iOS + macOS), `kotlin/` (KMP over JNI, Android + JVM), and
  `react-native/` (TurboModule → `@uldrenai/loom-react-native`). All wrap the same C ABI.
- `include/loom.h` - the public C header; regenerate with `just header` (cbindgen).
- `idl/loom.idl` - the language-neutral interface definition.
- `docs/DEVELOPMENT.md` - toolchain setup, cross-compilation, and per-binding build steps.
- `.github/workflows/` - `ci`, `bindings`, `release`, `scorecard`, `cla`.

Load-bearing root files - touch with care: `Cargo.toml`, `rust-toolchain.toml`, `rustfmt.toml`, `clippy.toml`, `deny.toml`, `justfile`.

## Before merging

Run the gate. CI runs the same set.

- `just ci` - `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `cargo deny check`. No mutation.
- `just all` - the full local pass when you want it: format, regenerate the C header, lint, release build, test, dependency policy, vulnerability scan.

Bindings build with their own toolchains - see each `bindings/*/README.md` and `docs/DEVELOPMENT.md`.

## Conventions

Rules the tooling can't fully enforce. Breaking them lands a regression.

- **Edition 2024, MSRV 1.85.** `rust-toolchain.toml` pins the toolchain (stable + rustfmt + clippy);
  `clippy.toml` pins the MSRV. Keep both in sync with `rust-version` in `Cargo.toml`.
- **No `unsafe` outside `uldren-loom-ffi`.** The workspace forbids `unsafe_code`; `uldren-loom-ffi` is the sole
  exception because the C ABI requires it. Every `unsafe` block carries a `// SAFETY:` comment that
  states the invariant being upheld.
- **The error `Code` enum is a stable contract.** Bindings and wire protocols preserve it verbatim.
  Don't rename or collapse variants; add new ones rather than repurposing existing ones.
- **Dependencies must be permissively licensed.** `cargo deny` (see `deny.toml`) denies copyleft;
  the project's own crates are BUSL-1.1. New dependencies that aren't permissive are rejected.
- **Data-model changes update the canonical vectors.** Any change to digests or the canonical object
  encoding updates `crates/loom-conformance` and is justified - these vectors pin behavior across
  every language binding.
- **Output and ABI shapes are part of the contract.** The C ABI (`include/loom.h`), the interface
  definition (`idl/loom.idl`), and the canonical vectors define observable behavior. Don't drift a
  shape without updating the definition and the conformance vectors together.
- **No emoji** in code, commits, or PR descriptions unless the request explicitly calls for them.
- **No em-dashes or en-dashes.** Use a plain hyphen `-`, or rewrite the sentence. This applies to
  every file: code, comments, docs, commits, and PR descriptions.
- **No stub comments.** Don't leave `TODO`, `phase 2`, or `refactor later` notes in shipped code -
  describe what the code does now, or delete the comment. Splitting a task or deferring genuinely
  out-of-scope work to a separate change is expected; leaving a marker in the tree is not.
- **Comments only for what the code can't say.** No restatement of behavior, no rationale-padding,
  no historical justification. Applies to every comment syntax - rustdoc, inline, YAML, shell.
- **Write to the current state, not the change.** Comments and docs address a reader who has only the
  current tree. State facts directly - not "the flag no longer defaults to true" but "the flag
  defaults to false". Change-relative narration belongs in the commit message, not in code or docs.
- **Minimal rustdoc.** Document the public surface where the signature can't say it; don't paraphrase
  parameter names or types. Internal items get a comment only when the code genuinely needs one.
- **Files are final.** Every committed file reads as a finished version - no placeholders, draft
  banners, or internal process notes.

## Branch model, commits, releases

- Short-lived branches off `main`. **Conventional Commits**, scoped by crate: `feat(core): …`, `fix(ffi): …`, `docs(cli): …`.
- Releases are cut from version tags via `.github/workflows/release.yml` (crate publish + GitHub
  Release). The bindings are built and published from `.github/workflows/bindings.yml`.
- Public, contributor-facing repos require a CLA before merging - see `CONTRIBUTING.md`.

## When stuck

- Project overview, build, and license: root `README.md`.
- Toolchain, cross-compilation, and bindings: `docs/DEVELOPMENT.md`.
- API behavior: read the crate source and its rustdoc; the interface definition is `idl/loom.idl`.
- Tool configuration: the configs themselves (`Cargo.toml`, `rustfmt.toml`, `clippy.toml`, `deny.toml`, `justfile`).

## Working as an agent

These rules apply to LLM agents picking up tasks in this repo. CI can't enforce them; the cost of breaking them is wasted reviewer cycles or a shipped regression.

### Non-negotiables

- **Check the contract before you build on it.** Before relying on anything across a boundary - a
  function's behavior, what a crate exports, the shape of an ABI call - read the authoritative source
  and cite where you found it (file and line). Two things looking alike by name is not proof. If the
  right target doesn't exist or the instruction is ambiguous, stop and ask.
- **Pausing to confirm is never a failure.** Shipping on an unchecked assumption is. Stop at any
  point to confirm context, check a fact, or ask for direction, at a low threshold.
- **Don't trust a check that fakes the thing you're unsure about.** A test or stand-in that imitates
  unverified behavior proves nothing about the real thing. Confirm against the real implementation.

### Interaction

- **Always ask in chat, with context, examples, and recommendations.** Every question to the user
  goes in the chat - never a tool that caps the number of questions or the room to read them. Phrase
  each as a short numbered list where each item carries the context that makes it answerable, a
  concrete example or the options to choose from, and your own recommendation with the tradeoff. A
  bare "what do you want?" is not acceptable; neither is proceeding silently when a question is due.
- **Confirm context before doing the work.** Surface missing facts before writing - and treat a fact
  you haven't checked against the source as missing. If it's knowable by reading the code, read it
  and cite it before relying on it.
- **Don't execute on questions, ideas, or plans until the user explicitly says so.** A question is a
  question; a plan is a plan. Wait for an unambiguous "go" before writing code or files. Surfacing
  options is not approval to pick one.
- **You are the architect; the user decides.** For how to structure, name, or pattern something,
  propose and recommend with the tradeoffs. When the choice is genuinely the user's - a public
  interface or output shape, scope, licensing, anything where their words are ambiguous - ask. Ask
  in the chat as a numbered list, each item with context and your recommendation.
- **No hand waving.** Be concrete. If you have a recommendation, make it; if you don't, name what
  you'd need to know to form one.
- **Ground every explanation.** Point at the file, quote the call site, sketch the example. A claim
  without a referent is noise.

### Code work

- **Don't guess at signatures or behavior.** If you don't know what a function does, read it.
- **Don't fabricate.** Never claim a function exists, a type is exported, or a behavior is
  implemented without verifying. Surface gaps; don't invent.
- **Don't improvise patterns.** If a similar problem is solved already, follow that pattern. A new
  helper or dependency needs a reason the existing pattern doesn't cover.
- **Minimal diffs.** Change as little as needed. Don't reformat unrelated lines or bump dependencies
  unless the task is the bump. A long comment over a short code change is not a minimal diff.

### Done

- **Run the real checks before you say it works.** Run `just ci` (or the specific recipes). Name each
  command you ran and its result. Never write "done" or "passing" for a check you didn't run; if you
  couldn't run one, say so plainly.
- **Surface conflicts; don't paper over them.** If the request would break a convention above, stop
  and say so. Don't reach for `#[allow(...)]`, `unsafe`, or `unwrap` to make a check pass.
- **Show your work.** List the files you changed, the exact commands you ran with outcomes, and mark
  each assumption as checked-against-source or not-yet-checked.
