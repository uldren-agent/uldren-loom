# Uldren Loom - Business & Legal Checklist

> Forward-looking task list of non-code work. Owner: Nas. **Not legal advice** - items marked ⚖️
> should be confirmed with a licensing/IP attorney. Loom is licensed BSL 1.1 → **Apache-2.0** with a
> **competing-use Additional Use Grant** (rationale in `specs/adr/adr-0004-licensing.md`).

## Licensing plan (orientation)

| Component                                                  | Repo                               | License               |
| ---------------------------------------------------------- | ---------------------------------- | --------------------- |
| Loom engine + bindings (the bindings embed the engine)     | `uldren-loom` (public)             | BUSL-1.1 → Apache-2.0 |
| Enterprise capabilities (KMS, ACL, transparency, OLAP hub) | `uldren-loom-enterprise` (private) | Proprietary           |
| Hosted sync / hosting service                              | `uldren-cloud` (private)           | Proprietary + SaaS    |

## Phase 0 - Legal foundation

- [ ] ⚖️ Confirm **Uldren Technologies LLC** holds copyright for `uldren-loom`; assign founder IP.
- [ ] Add `SPDX-License-Identifier: BUSL-1.1` headers to source files.
- [ ] Add a **THIRD-PARTY-NOTICES** file attributing the permissive dependencies.
- [ ] Set up the **CLA process** (referenced from `CONTRIBUTING.md`): finalize the CLA text and keep
      the `cla-signatures` branch unprotected (signatures land there via the default `GITHUB_TOKEN`;
      no separate repo or PAT needed). The workflow is `.github/workflows/cla.yml`.
- [ ] Verify the published **email addresses** exist and are monitored: `security@uldren.com`
      (`SECURITY.md`), `conduct@uldren.com` (`CODE_OF_CONDUCT.md`), and `licensing@uldren.com`.
- [ ] ⚖️ Draft the **commercial / embedder (OEM) license** template for Competing Offerings.

## Phase 1 - Brand & trademark

- [ ] ⚖️ **Trademark clearance** for "Uldren Loom" (Nice Class 9 downloadable software; Class 42
      SaaS). Bare "Loom" is high-risk (existing well-known marks); use the composite mark.
- [ ] ⚖️ File the trademark application(s) for the name + logo in primary markets (USPTO first;
      consider EUIPO / Madrid Protocol).
- [ ] Defensive registration: domains, GitHub org, npm/crates/Maven names, social handles.
- [ ] Add a **TRADEMARK.md** brand-use policy (the license grants no trademark rights).

## Phase 2 - Public launch

- [ ] Stand up commercial intake: `licensing@uldren.com` and a pricing page (`uldren.com/pricing`).
- [ ] Publish the first release: set the `CARGO_REGISTRY_TOKEN` and `RELEASE_PLZ_TOKEN` secrets. The
      crates publish as `uldren-loom-core` / `uldren-loom-cli` / `uldren-loom-ffi` /
      `uldren-loom-conformance` (the bare `loom-*` names are already taken on crates.io).
- [x] Wire up **binding version sync** - done: `scripts/sync-binding-versions.sh` propagates the
      workspace version into the binding manifests and runs in `bindings.yml` (and `just sync-versions`).
- [ ] **Publish the Node bindings to npm** (`@uldrenai/loom`, `@uldrenai/loom-wasm`): add a publish
      workflow gated on release tags; authenticate with npm Trusted Publishing (OIDC) or an
      `NPM_TOKEN` secret; build per-platform with `napi` and `wasm-pack publish`.
- [ ] **Publish the JVM binding to Maven Central** (`ai.uldren:loom`): register the `ai.uldren`
      workspace on Central, configure the Gradle publishing plugin, and set the Central (Sonatype)
      credentials and a GPG signing key as secrets.
- [ ] **Publish the Python binding to PyPI** (`uldrenai-loom`): reserve the project name, add a
      publish workflow gated on release tags that builds `abi3` wheels per platform plus an sdist
      with `maturin`, and authenticate with PyPI Trusted Publishing (OIDC).
- [ ] **Full mobile release jobs** - CI jobs that build the Android AAR (NDK + cargo-ndk, all ABIs)
      and the iOS device artifacts (`.xcframework`), beyond the current android/React Native smoke checks.
- [ ] **Licensing web page** - model the content and friendly tone on
      <https://docs.n8n.io/sustainable-use-license/>; explain BSL + the competing-use AUG + the
      BSL→Apache-2.0 conversion in plain language.
- [ ] Add **README badges** once the corresponding services exist: crates.io version, docs.rs,
      npm version + downloads (`@uldrenai/loom`), Codecov (the `coverage` CI job uploads `lcov.info`
      via `codecov-action`), OpenSSF Scorecard (`scorecard.yml` publishes results), and Discord
      (if a server is created).
- [ ] **Example apps** - build a set of reference apps on top of the crates and each binding (a CLI
      demo, a Node/TS app, a Swift/iOS app, an Android/Kotlin app, a React Native app, and a JVM
      app) to serve as living documentation and integration references.

## Phase 3 - Monetization build-out

- [ ] Build the **enterprise add-on** repo (`uldren-loom-enterprise`, proprietary) on the capability
      boundary - keep the public core fully functional without it.
- [ ] Build the **hosted service** (`uldren-cloud`) with free + paid SaaS tiers.
- [ ] Complete **trademark registration**; set up a watch service for infringers.
- [ ] ⚖️ Decide a **patent strategy** for novel mechanisms (the single-file container format,
      identity-profile sync) - provisional patents if defensibility matters.

## Phase 4 - Operate

- [ ] Track **per-version Change Dates** (each release starts its own 4-year BSL→Apache-2.0 clock);
      announce conversions ahead of time.
- [ ] Maintain the commercial-subscription value post-conversion (enterprise features, support, and
      a commercial license that exempts a Competing Offering from the AUG during the BSL window).
- [ ] SaaS compliance as you grow (e.g., SOC 2), data-processing terms.

## Open questions for counsel ⚖️

1. Validate the drafted "Competing Offering" definition and the embedded-infrastructure carve-out so
   the boundary is enforceable as intended.
2. Confirm BSL on an embedded library is enforceable as intended, and how the embedder commercial
   license should read.
3. Trademark clearance and registrability of "Uldren Loom".
4. CLA wording covering the BSL→Apache-2.0 conversion and commercial/embedder dual-licensing.
