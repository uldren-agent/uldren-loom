# ADR-0002 - Drop the LocalShim backend; move "in/out" to Interchange

**Status:** Accepted · **Date:** 2026-06-14 · **Deciders:** Nas (+ Loom maintainers)
**Supersedes:** the earlier "demote LocalShim to a Bridge" draft of this ADR.

## Context

The original sketch (0004) included **`LocalShim`**: a Provider that implemented the Loom interface
**on top of a normal filesystem and normal git** - i.e., a real backend whose Warp/Heddle/Weft were
the host's files and a real `.git`, so a user could run Loom against an existing checkout and
interoperate with git tooling.

The owner clarified the intent (it was a normal-FS + normal-git backend, **not** an attempt to force
prolly trees through a shim) and then decided plainly: **drop it.** The end goal is the single
`.loom` file and the features built around it (0005, 0009, 0011); a faithful FS+git backend is a
large, perpetual maintenance surface that does not advance that goal.

## Decision

1. **Remove `LocalShim` from the provider set entirely.** The sole storage backend is `SingleFile`
   (0004 §3.1, flagship), related by `Sync` (0004 §3.2) and composed with the capability layer
   (0004 §3.3). There is no FS-backed or git-backed provider. *(Update: the `Database` backend once
   sketched at 0004 §3.3 has since also been dropped and is no longer supported - see 0004 §3; the
   capability layer was renumbered to §3.3.)*

2. **Re-home the one legitimate need it served - moving data in and out of Loom - into a dedicated
   Interchange layer (spec 0012),** which is a set of import/export *operations*, not a storage
   backend:
   - **import a database table** into a native Loom (rows → versioned table, 0011);
   - **check out** a Loom commit tree **to the filesystem** (materialize files on disk);
   - **create a commit** from a **filesystem tree** (ingest a directory).

   These are unidirectional, explicit conversions across Loom's boundary - distinct from `Sync`
   (0006), which moves history *between Looms* that already share the object model.

## Rationale

- A live FS+git backend must continuously reconcile two object models (git's SHA-1/SHA-256 vs.
  Loom's BLAKE3 + prolly trees), maintain hashing-on-read caches, and accept OS-level atomicity gaps
  it cannot close (it could never offer the Selvedge ACID guarantees of the `.loom` format, 0005
  §6). High cost, tangential value.
- The Interchange operations, by contrast, are cheap and well-bounded: materializing/ingesting a
  tree is work the engine already does for the Weft (0003 §4), and a one-shot git/DB import is a
  read-only traversal of a foreign store followed by ordinary Loom writes.
- Separating "convert across the boundary" (Interchange) from "replicate within the model" (Sync)
  keeps both crisp and keeps the provider set small.

## Consequences

- **Positive:** removes the single largest source of incidental complexity and the only component
  that would have broken the universal identity profile (0002 §8); the provider set is now two clean
  storage backends; onboarding/migration is still fully served by Interchange.
- **Negative:** Loom is not a live, bidirectional git working copy and does not speak git's wire
  protocol, and it does **not** import git repositories at all - git interoperability is permanently
  out of scope (0012). Users migrate *into* Loom via `import_fs` / `import_table` (0012) and then use
  Loom's own sync (0006).
- **Reversible:** the Provider contract (0004 §2) is unchanged, so a future FS-backed provider
  could be added without disturbing the core, interface, or other backends - but it is explicitly
  out of scope and not on the roadmap (0010 §6).

See 0004 (LocalShim removed), 0012 (Interchange), and the roadmap (0010 §6, Milestone 4).
