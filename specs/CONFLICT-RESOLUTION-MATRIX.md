# CONFLICT-RESOLUTION-MATRIX - concurrent writes to the same named target across peers

**Status:** Resolved default policy. **For:** sync, branches, workspaces, programs, and data facets.
**Relates to:** 0006 (sync), 0003 section 6/9 (batch and ref CAS), 0014 (workspaces/branches),
0015 (programs), and every data-layer spec (0011, 0016-0024). **Convention:** each data-layer spec
defers same-element collision behavior here unless it defines a stricter facet-specific policy.

## 1. Problem

The same logical Loom can be live in two places: two replicas, two devices, or an online copy plus an
offline copy. Both replicas may write against the same named target:

`workspace + branch + (stream | table | key | path | series | node | object)`

The two histories share a common ancestor, then diverge. Later they sync. Loom needs one durable,
user-friendly, enterprise-grade policy for what happens next.

The selected default mirrors cloud-storage conflict handling more than Git-style automatic merge:

- Sync may transfer objects.
- Sync does not silently merge divergent branch/ref state.
- If the same named branch/ref diverges, the branch/ref advance is refused.
- A human, AI, authorized program, or administrator explicitly chooses what happens to the competing
  versions.
- The non-chosen version can be preserved as a branch, conflict ref, or saved version when useful.

## 1.1 Settled invariants

- **Identical content is not a conflict.** If two sides write identical bytes, the content digest is
  identical and the object deduplicates.
- **Different branches do not conflict.** `experiment-x` and `experiment-y` can coexist until a user
  explicitly adopts, discards, or reconciles them.
- **Branch advance is fast-forward-checked.** Advancing a ref to a non-descendant is a
  `NOT_FAST_FORWARD`. Sync can transfer missing objects, but it must not silently move the contested
  ref.
- **Local unstaged writes are local mutable state.** Whole-object local overwrites can use local
  last-writer-wins behavior. Structured local editors may merge disjoint portions of an object when
  their storage layer supports it. These local unstaged rules do not decide distributed committed
  branch conflicts.
- **Merge helpers are explicit tools.** File 3-way merge, SQL row merge, SQL cell merge, AI-assisted
  merge, or program-defined merge can create an explicit resolved commit. They are not implicit sync
  behavior.

## 1.2 Redaction and history rewrite

Redaction is the high-stakes version of divergent branch history. One replica rewrites history to
remove content while another replica still has the old objects.

Default policy:

- Ordinary sync must not resurrect redacted content.
- A rewritten ref is a non-fast-forward update and requires explicit administrative action.
- Force sync, rebase, reclone, or administrative recovery must be explicit and auditable.
- Target work may add signed redaction/rewrite markers that peers honor during sync.

## 2. Resolution strategy menu

The table keeps the strategy vocabulary used by dependent specs.

| # | Strategy | Meaning | v1 position |
| --- | --- | --- | --- |
| S1 | Fast-forward only | Nobody auto-wins; divergent ref advance is rejected with `NOT_FAST_FORWARD` or `CONFLICT`. | Default for branch/ref sync. |
| S2 | 3-way structural merge | Disjoint structured edits can be merged; overlap raises `CONFLICT`. | Explicit helper, not implicit sync default. |
| S3 | Last-writer-wins | A later write wins, usually by timestamp. | Local unstaged overwrite behavior only; not distributed sync policy. |
| S4 | Deterministic tiebreak | A fixed digest, peer id, or sequence rule picks a winner. | Not a silent default; may be used inside explicit facet policy. |
| S5 | CRDT / mergeable type | A type-specific rule converges without conflict. | Explicit facet policy only. |
| S6 | Keep-both siblings | Both values remain under one logical key and reads resolve later. | No general v1 sibling reads. Preserve competing versions as branches/conflict refs instead. |
| S7 | Program-defined merge driver | A deterministic program computes a resolved value. | Explicit target work gated by compute, provenance, authorization, and conformance. |

## 3. Per-layer collision shape

| Layer (spec) | Element key | Collision shape | Default policy |
| --- | --- | --- | --- |
| files / VCS (0003) | path within a branch | same branch tip diverges, or same path differs during explicit merge | Sync uses S1. Explicit merge tools may use S2. |
| SQL / tabular (0011) | primary key | same PK row differs | Sync uses S1 at branch/ref level. Explicit table merge may use row-level S2 or opt-in cell-level S2. |
| graph (0016) | node id / edge id | same node or edge differs | Sync uses S1. Facet merge policy can be explicit target work. |
| vector (0017) | vector id | same id differs | Sync uses S1. Recomputed vectors are normally local derived data. |
| ledger (0018) | sequence in hash chain | divergent appends rehash the chain | S1-like fast-forward/replay only; no silent branch winner. |
| KV (0019) | key | same key differs | Sync uses S1. Local unstaged overwrite can use local last-writer-wins. |
| document (0020) | document id | same document differs | Sync uses S1. Explicit document merge can be target work. |
| append-log / queue (0021) | sequence in stream | overlapping sequence claims | S1 by default; deterministic replay/resequence only as explicit policy. |
| time-series (0022) | series and timestamp | same timestamp differs | Sync uses S1. Timestamp is data, not a distributed winner. |
| columnar (0023) | segment id / row | same segment differs | Sync uses S1. Explicit segment merge can be target work. |
| CAS (0024) | content digest | digest collision would imply identical content | No conflict by construction. |

## 4. Resolved decisions

### 4.1 Global default strategy

**Context:** When two peers advance the same branch/ref differently, Loom needs a default that is
safe for unknown object types and understandable to users.

**Examples:**

- Two machines advance `main` differently.
- One user has `main`, `experiment-x`, and `experiment-y`.
- One replica syncs to upstream first; the second replica later tries to sync its different `main`.

**Options:**

- Automatically 3-way merge.
- Use last-writer-wins.
- Pick a deterministic winner.
- Preserve both divergent states and require explicit adoption.

**Recommendation:** Use S1 fast-forward only for branch/ref sync. Sync transfers objects but refuses
the contested ref move. The user, AI, authorized program, or administrator explicitly chooses whether
to adopt local, adopt remote, force-publish, save a competing tip as a branch/conflict ref, or create
a resolved commit. This is the enterprise default because it avoids silent semantic data loss.

### 4.2 Per-workspace and per-type override

**Context:** Different facets can offer different explicit merge helpers, but hidden policy variance
creates support, audit, and conformance problems.

**Examples:**

- Files may have a text merge helper.
- SQL may have row-level and cell-level merge helpers.
- Ledger should remain fast-forward/replay only.
- A regulated workspace may forbid automatic merge entirely.

**Options:**

- One global policy for every facet.
- Per-facet defaults.
- Per-workspace overrides.
- Per-key or per-path overrides.

**Recommendation:** Use the global S1 sync default everywhere. Facets may expose explicit merge tools,
but sync does not invoke them implicitly. Per-workspace policy overrides are target work only after
access control, audit, and conformance can enforce them.

### 4.3 Last-writer-wins

**Context:** Last-writer-wins is user-friendly inside one local mutable workspace, but unsafe as a
distributed branch/ref policy.

**Examples:**

- A local whole-object unstaged write overwrites a previous local write.
- A laptop clock is wrong while offline.
- A malicious or buggy peer reports an incorrect timestamp.

**Options:**

- Timestamp wins.
- Timestamp plus deterministic tiebreak.
- Commit digest or peer id wins.
- Forbid LWW for committed distributed sync.

**Recommendation:** Use last-writer-wins only for local unstaged whole-object overwrite behavior. Do
not use timestamp LWW to resolve committed branch/ref divergence. If a facet later defines LWW, it
must be explicit, deterministic, and conformance-tested.

### 4.4 Siblings and multi-value conflicts

**Context:** General sibling reads avoid data loss, but force every API and binding to handle
multi-value results for ordinary reads.

**Examples:**

- A KV read returns two values for one key.
- A document read returns two versions.
- A SQL primary-key read returns sibling rows.

**Options:**

- No siblings; surface conflicts.
- General siblings everywhere.
- Facet-specific conflict objects.
- Preserve competing states as branches or conflict refs.

**Recommendation:** Do not include general sibling reads in v1. Preserve competing versions as
branches, conflict refs, or saved versions. Ordinary reads remain single-valued and user-facing
conflict handling stays explicit.

### 4.5 Program merge drivers

**Context:** A deterministic program or AI-assisted flow can reconcile domain-specific conflicts, but
it must be explicit, authorized, and auditable.

**Examples:**

- Merge two JSON documents by schema.
- Merge a shopping cart by item id.
- Merge two SQL row versions using business rules.

**Options:**

- No program merge drivers.
- Run program drivers automatically on sync.
- Allow program drivers only as explicit resolution actions.
- Defer program drivers until compute and access control mature.

**Recommendation:** Model program merge drivers as explicit resolution actions. A human or AI chooses
the VCS state, or an authorized deterministic program creates a new resolved commit. Program drivers
are target work gated by deterministic compute, provenance, authorization, resource limits, and
conformance vectors.

### 4.6 Provenance capture

**Context:** Even without automatic merge, Loom needs provenance for conflict detection, explicit
resolution, audit, replay, AI-assisted merge, and force sync.

**Examples:**

- Local `main` differs from upstream `main`.
- A user adopts remote and saves local as `experiment-y`.
- An administrator force-publishes rewritten history.
- An AI-assisted resolver creates a new commit from local and remote tips.

**Options:**

- Record no provenance.
- Record only the chosen winner.
- Record local and remote tips.
- Record full conflict and resolution provenance.

**Recommendation:** Record full provenance for every detected conflict and every explicit resolution:
workspace id, branch/ref or named target, common base tip when known, local tip, remote tip, chosen tip
or new resolution commit, action, actor/principal, and program digest plus input digests when a
program or AI-assisted resolver participates. Timestamps are audit metadata, not conflict-ordering
inputs.

### 4.7 Symmetry requirement

**Context:** With explicit resolution, symmetry means sync direction does not choose a winner. Both
peers must detect the same divergence and expose equivalent choices.

**Examples:**

- A syncs to B.
- B syncs to A.
- Both peers sync through an upstream server.
- Rust, Python, Node, and C bindings expose the same conflict shape.

**Options:**

- Best-effort symmetry.
- Symmetry only for some facets.
- Hard symmetry for conflict detection and preservation.

**Recommendation:** Make symmetry a hard rule. Sync direction, peer order, platform, language binding,
and transport must not determine a winner. The only winner is the one selected by explicit user, AI,
program, or administrative action.

### 4.8 Redaction and rewrite resurrection

**Context:** Redaction/history rewrite is the same branch-divergence problem with stricter safety
requirements. A peer that still holds old objects must not silently reintroduce removed content.

**Examples:**

- A secret file is removed with history rewrite.
- An offline peer still has the old commit.
- Sync sees the old objects as available.
- A naive merge would bring the removed content back.

**Options:**

- Treat redaction as local-only.
- Require explicit force sync, rebase, or reclone.
- Add signed redaction/rewrite markers peers honor during sync.
- Allow old content to return only through explicit administrative recovery.

**Recommendation:** Ordinary sync must not resurrect redacted content. In v1, treat rewrite/redaction
as an administrative non-fast-forward event that requires explicit force sync, peer rebase, or
reclone. Target work can add signed redaction/rewrite markers so peers decline to offer redacted
objects back during sync. Any recovery that reintroduces removed content must be explicit and
audited.

## 5. Program angle

Collisions often originate when a program runs independently on two replicas with different inputs.

- Same program plus same inputs produces the same output digest and deduplicates.
- Same program plus different inputs can produce divergent outputs to the same named target.
- A program can participate in explicit resolution only when its digest, inputs, authorization, and
  resource limits are recorded.
- Program output provenance lets the engine distinguish idempotent reruns from genuine divergence.

## 6. Conformance permutations

Each behavior below needs source-backed tests before a facet claims enterprise merge behavior:

1. Identical writes produce the same digest and no conflict.
2. Different branches coexist without conflict.
3. Same branch, divergent committed tips reject ref advancement.
4. The rejected side can be preserved as a branch, conflict ref, or saved version.
5. Explicit adopt-local and adopt-remote actions produce auditable ref outcomes.
6. Explicit program merge creates a new commit with recorded provenance.
7. Swapped sync direction exposes the same divergence and does not choose a different winner.
8. Redaction/rewrite cannot be silently resurrected by ordinary sync.

Until a facet implements and proves a stricter explicit policy, source-backed behavior remains:
transfer objects, fast-forward where possible, and surface `NOT_FAST_FORWARD` or `CONFLICT` rather
than silently choosing a winner.
