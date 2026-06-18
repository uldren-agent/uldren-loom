# ADR-0004 - Licensing, branding, and contribution model for Uldren Loom

**Status:** Accepted · **Date:** 2026-06-14 · **Deciders:** Nas
**Related:** company-wide `licensing-strategy.md`; ADR-0001 (Rust core); the `LICENSE` file (draft).
**Caveat:** ⚖️ Not legal advice. The `LICENSE` text and the AUG below are drafts for review by a
licensing/IP attorney before public release.

## Context

The company-wide `licensing-strategy.md` standardizes on BSL 1.1 → AGPL-3.0 with a **Personal-Use-
only** Additional Use Grant (AUG), and a funnel of **Apache-2.0 thin-client SDKs** talking to a
**BSL Gateway**. That strategy was written for a different Uldren product built around a Gateway and
WebSocket SDKs. **Uldren Loom has no Gateway**, and its language bindings are **not** thin clients -
they statically embed the Rust engine in-process. So two assumptions of the company default do not
hold for Loom, which is an embeddable **library/engine**, not a hosted application.

## Decisions

1. **Branding: "Uldren Loom".** The product/display name is *Uldren Loom* (not bare "Loom", which
   collides with well-known existing marks). The composite mark is what we file and use (see
   `BUSINESS-LEGAL-CHECKLIST.md`, Phase 1).

2. **Core license: BSL 1.1, Change License Apache-2.0, Change Date 4 years per version.**
   `Licensed Work = "Uldren Loom"`, `Licensor = Uldren Technologies LLC`. **Diverges from the
   company default of AGPL-3.0 as the change license** - deliberately, because Loom is an embeddable
   library: an AGPL conversion would trap embedders of converted versions in copyleft, fighting the
   whole point of the competing-use AUG (#4). Apache-2.0 means each version becomes fully permissive
   after the window, so embedders are never caught. The 4-year-old permissive version is a weak
   competitive threat (it trails the current release, the enterprise add-ons, and the SaaS).

3. **Bindings are BSL too (not Apache-2.0).** Unlike the company's thin-client SDKs, Loom's bindings
   (`@uldrenai/loom` Node, `uldrenai-loom` Python, the JVM/C++/WASM packages) embed the BSL engine, so the distributed
   packages contain BSL code and cannot honestly be Apache-2.0. They carry the same BSL as the core.
   *(This is the resolution of checklist D2.)*

4. **AUG: competing-use-scoped (the *wide* AUG).** Loom is infrastructure; adoption depends on low
   friction. The AUG permits all use **except a Competing Offering** - so internal business use,
   **embedding Loom as infrastructure inside your own commercial product** (e.g. a note-taking app),
   and personal/non-commercial/research use are all **free**; a commercial license is required only
   for a Competing Offering (hosting/SaaS/white-label/redistributing Loom's *own* functionality to
   third parties). This deliberately does **not** tax ordinary commercial embedding the way n8n's
   SUL does - because for an *ingredient* library, ubiquity is the moat (cf. SQLite/libgit2), and
   "avoid competing hosting" was the stated primary goal; taxing every commercial embedder is a
   different goal we chose not to pursue. *(Resolution of checklist D1 - boundary now closed.)*

   **Alternatives considered & rejected:** the **n8n-style AUG** (internal free, all commercial
   provision-to-others paid) and a **revenue-threshold AUG** - both capture more embedder revenue
   but at adoption cost; rejected for an ingredient library. **FSL-1.1** - rejected: its 2-year
   change window is too short. **SUL-1.0** (n8n's license; it *is* SPDX-listed) - rejected: its
   restriction is perpetual with no eventual openness, and it diverges from the BSL family; we
   prefer BSL's eventual conversion to Apache-2.0.

5. **Contributions require a CLA.** Because we dual-license (sell commercial licenses for Competing
   Offerings) and convert versions to Apache-2.0 on a schedule, we must hold relicensing rights to
   all contributed code. A CLA is therefore **mandatory** - the opposite of Dolt, which dropped its
   CLA precisely because it is permissive (Apache-2.0) and never intends to relicense. The CLA must
   state that contributions are under BSL and convert to Apache-2.0 on the Change Date, and grant
   Uldren commercial/dual-licensing rights. Use CLA Assistant; keep it lightweight; explain the
   "why" in `CONTRIBUTING.md`.

6. **Monetization:** open-core - public BSL engine + private **proprietary enterprise add-ons**
   (`uldren-loom-enterprise`) on the capability boundary (0009) + a **free/paid hosted SaaS**
   (`uldren-cloud`). The competing-hosting AUG is what protects the SaaS from being cloned.

7. **Licensing web page (future task):** model the public licensing page's content, tone, and prose
   on n8n's friendly explainer at https://docs.n8n.io/sustainable-use-license/ (plain-language FAQ,
   "can I use this at work? yes" framing). Tracked in `BUSINESS-LEGAL-CHECKLIST.md`.

8. **Dependency licenses: permissive plus MPL-2.0.** `deny.toml` allows the permissive set
   (Apache-2.0 incl. LLVM-exception, MIT, MIT-0, BSD-2/3, ISC, CC0-1.0, Unicode-3.0, Zlib) and, as a
   deliberate exception, **MPL-2.0**. MPL-2.0 is **file-level** copyleft: linking an unmodified
   MPL-2.0 crate into Loom does not impose copyleft on Loom or on a downstream product, and only
   modifications to the MPL files themselves must be shared. This keeps the door open to MPL-2.0
   engines (notably Cozo for the optional graph/vector/Datalog facet and AI-memory work, 0013) while
   preserving Loom's embed-everywhere posture. **Stronger copyleft (GPL/LGPL/AGPL) remains denied.**
   The known cost is that some enterprise procurement teams flag any copyleft in a dependency tree;
   that is accepted as the price of keeping the engine available, and the compute/logic layer (0015)
   still prefers permissive alternatives where they suffice.

## Consequences

- **Positive:** much lower adoption friction than Personal-Use-only (orgs can use Loom internally
  and embed it for free), while still blocking the specific thing we care about - competitors
  hosting/reselling Loom - and preserving commercial licensing for embedders who make Loom's
  functionality their offering. Uses BSL with an **Apache-2.0** change license and the CLA.
- **Negative / risk:** "BSL" on a *library* still triggers some enterprise-legal review (less than
  AGPL, more than Apache); the boundary between "embedded infrastructure" (free) and "Loom as the
  offering" (paid) is inherently fuzzy and must be drafted carefully (⚖️). BSL bindings mean
  `@uldrenai/loom` is source-available, not OSI-open - set expectations in README/docs.
- **Reversible-ish:** the AUG can be loosened over time (loosening is easy; tightening needs notice
  per the change policy). Per-version conversion to Apache-2.0 is irrevocable once it occurs.

## Open

None. The embedder boundary (the last open D1 detail) is resolved to the **competing-use (wide)
AUG** per Decision #4. Remaining work is execution + counsel review of the drafted `LICENSE`
wording (tracked in `BUSINESS-LEGAL-CHECKLIST.md`), not further decisions.
