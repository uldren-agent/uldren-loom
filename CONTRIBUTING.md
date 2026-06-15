# Contributing to Uldren Loom

Thanks for your interest! A few things to know before you open a PR.

## Licensing & the CLA (please read first)

Uldren Loom is **source-available under BUSL-1.1**; each version converts to Apache-2.0 four years
after its release (see [`LICENSE`](./LICENSE)).

Because we offer commercial licenses (for *Competing Offerings*) and run the per-version
BSL→Apache-2.0 conversion, **we require a Contributor License Agreement (CLA)** so the project holds
the rights to do this for all contributed code. The CLA bot will prompt you on your first PR; it's a
one-time, lightweight signature.

### Contributor License Agreement

By submitting a contribution, you agree that your contribution is licensed under BUSL-1.1 (converting
to Apache-2.0 on its Change Date) and you grant Uldren Technologies LLC the right to license your
contribution commercially and to dual-license it as part of the project. You confirm you have the
right to make the contribution.

## Development

```bash
# one-time: install the Rust toolchain (rust-toolchain.toml pins it) and `just`
#   https://www.rust-lang.org/tools/install   https://github.com/casey/just
just         # list tasks
just ci      # fmt + clippy + tests + cargo-deny (what CI runs)
just test    # tests only
just fmt-fix # auto-format
```

Full setup, cross-compilation, and bindings: [`docs/DEVELOPMENT.md`](./docs/DEVELOPMENT.md).
Each binding builds with its own toolchain - see `bindings/*/README.md`.

## Commits & releases

- Work on short-lived branches off `main` (`feat/…`, `fix/…`) and open a pull request; changes land
  on `main` via a merge commit after review and a green CI run.
- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org), scoped by
  crate: `feat(core): …`, `fix(ffi): …`, `docs(cli): …`. CI verifies a PR's commits.
- Install the local git hooks once (commit-message check + `cargo fmt` on commit):

  ```bash
  cargo binstall cocogitto # or: cargo install cocogitto
  cog install-hook --all
  ```

- Releases are automated by [release-plz](https://release-plz.dev): merging to `main` opens a
  release PR that bumps versions and updates the changelog from the commit history; merging that PR
  tags and publishes. Don't hand-edit versions or changelogs.

## Expectations

- `just ci` must pass (formatting, `clippy -D warnings`, tests, dependency policy).
- New dependencies must be permissively licensed (`cargo-deny` enforces this; see `deny.toml`).
- Add tests; for data-model changes, update the canonical vectors in `crates/loom-conformance`.

## Reporting security issues

See [`SECURITY.md`](./SECURITY.md) - please do not open public issues for vulnerabilities.
