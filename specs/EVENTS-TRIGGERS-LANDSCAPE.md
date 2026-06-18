# Events & Triggers Layer - Landscape and Options

**Status:** Exploratory / research note. **Version:** 0.1.0-draft. **Normative?** No.
**Promoted to:** 0029 (Events & Triggers), which is now the normative source; this note is retained as
the research input that fed it (library scoping and the durability analysis).
**Relates to:** 0015 (execution and logic), 0009 (security and capabilities), 0014 (workspaces),
0018 (ledger), 0021 (append-log), 0003 (core interface), 0006 (sync), 0030 (observability).

Working data-gathering doc. It maps the option space for adding a reactive layer to Loom: a way to
run a stored program in response to time (a cron schedule) or to change (a ref advance or a path
write), without weakening the determinism, metering, and run-on-a-branch guarantees the compute face
already provides (0015). Nothing here is committed. House conventions apply: no em-dashes, no emoji,
every claim reads as current fact.

## 1. Why this exists

Loom today is passive. A caller reads, writes, commits, queries, and (exploratory, 0015) executes a
program when it asks to. The store never acts on its own. The compute face added the third leg,
deterministic logic over the store; this note explores the fourth, deterministic logic that fires on
its own when a condition is met.

The specs already gesture at this in three places, and none of them is a clock:

- **0015 section 10 (Triggers and derived views)** names it directly: "Triggers - a program bound to
  a path or ref runs on change, like a database trigger or a git hook, gated by `StateAccess`." It is
  listed unbuilt in the 0015 status block (`[ ] Durable workflows and triggers`).
- **0009 section 8** lists the `watch` capability: "change feeds on paths/refs (0003 section 4.6) for
  reactive tools," and **0003 section 4.6** is the change-feed surface.
- **0015 section 8.2** gives a time-based example, but as a promotion rule, not a trigger: "tag the
  files workspace `main` after 15 minutes of idle."

The gap this note targets is the one not covered anywhere: **every trigger the specs describe is
change-driven. There is no clock-driven scheduler.** Cron is new. The adjacent "durable workflows"
idea (the L3 statechart layer, 0015 section 3 and section 9) is a different thing: stateful
lifecycles, not "run this program at 06:00." So adding cron means adding a time source to a system
that was deliberately built to have none.

## 2. The determinism problem, and the keeper pattern

That tension is the whole design problem. 0015 section 4 forbids a program from reading a wall clock:
time must be "a declared, seeded input passed as an input digest, never read ambiently." A cron
schedule is the definition of ambient time. So the scheduler cannot live inside the deterministic
interior.

It resolves the way smart-contract platforms resolve it. A contract cannot read the clock; an
external automation network (Chainlink Automation, Gelato, Ethereum Alarm Clock) watches the world
and submits a transaction when a condition holds, and the block timestamp arrives as an input the
contract may read. Loom's analogue is a host-side driver, called here the **keeper**:

1. The keeper is the only component allowed to observe real time and external change. It lives in the
   host or daemon (the `loom-server` of IMPLEMENTATION-PLAN.md P9), never in `loom-core`.
2. When a schedule fires, the keeper encodes the firing instant as a small blob, takes its digest, and
   passes that digest as a declared input to the existing `exec.apply` / `exec.dry_run` facade
   (0015 section 11). The recorded execution is `(manifest_digest, { "fired_at": <digest>, ... })`.
3. Determinism (0015 section 4) is therefore untouched, and the run is replayable, because the time
   the program saw is pinned in its inputs rather than read from the host.

The single most important boundary in this note: **the reactive layer is host machinery, not a
core-engine change.** The deterministic kernel stays deterministic and single-actor; reactivity is a
property of the node that wraps it. This is the same split that keeps access control out of the core
(see PRINCIPALS-AND-ACCESS-CONTROL-LANDSCAPE.md).

## 3. Two trigger classes

| Class | Fires on | The seeded input is | Loom seam |
| ----- | -------- | ------------------- | --------- |
| **Time (cron)** | a cron schedule reaching its next instant | the digest of the fired timestamp | keeper evaluates the schedule, calls `exec` |
| **Change (event)** | a ref advance or a path write in a watched workspace | the digest of the new commit, or the changed path set | reuses `watch` (0009 section 8) / change feeds (0003 section 4.6) |

Both reduce to the same shape: an external stimulus is captured as a content address and handed to a
deterministic program as an input. A change trigger is the database-trigger / git-hook case the specs
already name; a time trigger is the new cron case. A single binding record can carry either kind.

## 4. Where schedules live: a versioned trigger registry

Schedules and trigger bindings are themselves data, so they belong in the store, not in host config.
This mirrors how programs live in a reserved `program` workspace (0014 section 2, 0015 section 7.1).
A reserved **`trigger` workspace** maps a binding id to a binding record:

```
binding_id -> {
  kind:      Time { cron: string, tz: string } | Change { watch: RefGlob | PathGlob },
  program:   Digest,            // a program manifest (0015 section 7), by content identity
  target_ns: NsSelector,        // the workspace the program runs against
  budget:    u64,               // fuel ceiling (0015 section 5)
  mode:      Gated | Direct | Batched,   // 0015 section 8.1; default by facet if omitted
  enabled:   bool,
}
```

Because the registry is an ordinary workspace, the schedule set is branchable, diffable, syncable, and
auditable like any other content. That matches Loom's "versioned source of truth for configuration"
use case (README use case 3): you branch `staging`, change a schedule, diff it, and merge it to
`prod`. The keeper reads this registry and drives it; it holds no schedule state of its own beyond a
last-fired watermark (section 5).

## 5. The event spine: append-log / ledger, idempotency, and catch-up

Each fire appends a record to an append-only log (the append-log facet 0021, or the hash-chained
ledger facet 0018 when tamper-evidence is wanted): `(binding_id, stimulus_digest, proposed_commit,
cost, outcome)`. That log is three things at once:

- **An audit trail** of what fired, when, on what stimulus, and what it produced (the `audit`
  capability of 0009 section 7, for free).
- **A watermark store.** The last-fired instant per binding is the cursor the keeper resumes from.
- **An idempotency key.** This is where Loom's content-addressing pays off. Executions are cacheable
  on `(manifest_digest, input_digests)` (0015 section 2 and section 7). A trigger that fires twice on
  the same stimulus digest therefore produces the identical state root the second time. At-least-once
  delivery plus content-addressed results gives effectively-once outcomes, which is the property the
  EXECUTION-LOGIC-LANDSCAPE.md Table 3 row flagged as unsolved ("exactly-once delivery guarantees").
  You dedup on the input digest rather than building a distributed-transaction protocol.

Catch-up after downtime is also a determinism dividend. Cron evaluation against a pinned watermark is
a pure function, so a keeper that was offline can compute exactly which fires it missed and apply a
recorded policy (backfill each, collapse to one, or skip), and that decision is itself logged.

## 6. Execution modes and promotion

Triggers do not get a new safety story; they inherit the compute one. A trigger runs through the same
three execution modes (0015 section 8.1): gated (run on a copy-on-write branch, produce a diff, merge
after review), direct (apply in place, for append-only high-frequency facets), and batched. The mode
defaults per facet, so a time trigger on a relational workspace defaults to gated and a trigger that
only appends to a log defaults to direct.

Promotion stays separate from the trigger (0015 section 8.2). A trigger proposes a transition; whether
that proposal merges into a protected ref is a promotion decision, made by policy or a human, not by
the trigger. This is the hook for "tag `main` after 15 minutes idle" style automation: such a rule is
a time trigger whose program proposes a tag, gated by the same promotion policy as any merge.

## 7. The library landscape

The reactive layer splits into two host-side jobs: parse and evaluate schedules, and durably drive the
firing loop. Neither runs in the deterministic sandbox, so the `wasm32` baseline that constrains the
compute engines (EXECUTION-LOGIC-LANDSCAPE.md section 3) does not bind here; these crates live in the
native daemon. License is checked against the `deny.toml` allowlist (Apache-2.0, MIT, BSD-2/3-Clause,
ISC, CC0-1.0, Unicode-3.0, Zlib, MPL-2.0).

### 7.1 Cron-expression parsers (measured)

Three candidates were linked into the footprint probe (`prototypes/size-probes`, run via
`compare-cron-size.sh`) and built release+stripped against an empty baseline on
`aarch64-unknown-linux-gnu`, rustc 1.96.0. The probe parses one expression and computes the next fire
instants from a fixed base time, which is the real keeper workload and confirms each parser computes
"next after a base" deterministically.

| Parser | Version | Binary delta over baseline | Transitive deps (normal) | Cron dialect | License | `deny.toml` |
| ------ | ------- | -------------------------- | ------------------------ | ------------ | ------- | ----------- |
| **saffron** (cloudflare) | 0.1.0 | **+52.0 KiB** | **7** (chrono, nom 5, memchr) | Vixie 5-field; supports `L` and nearest-weekday `W`; no seconds field | BSD-3-Clause (via `license-file`) | Allowed, but needs a `[licenses.clarify]` entry because it ships a license file, not an SPDX `license` field |
| **croner** (Hexagon) | 3.0.1 | +60.0 KiB | 25 (chrono, derive_builder, darling, strum, syn) | 5-7 fields; optional seconds and year; `L`, `#` (nth weekday), `W`; built-in human-readable describe | MIT | Allowed |
| **cron** (zslayton) | 0.16.0 | +104.3 KiB | 21 (chrono, phf, winnow, rand, syn) | 6-7 fields; seconds and optional year; ranges/steps/lists; no `L`/`W`/`#` | MIT OR Apache-2.0 | Allowed |

Baseline binary was 300.7 KiB; all deltas are small because these are thin parsers over `chrono`. The
proc-macro dependencies (syn, darling, strum_macros, phf_macros) are build-time only and do not enter
the runtime binary, but they dominate compile time, so the dependency-count column is a better
compile-cost proxy than the size column.

Reading the table: **size does not decide this one.** At 50-100 KiB of marginal binary in a daemon, all
three are negligible. The real axes are dialect, maintenance, and supply-chain surface:

- **croner 3.0.1** is the most capable and the most actively maintained. It is the only candidate with
  the full quartz-style special set (`L`, `#`, `W`), optional seconds, and a human-readable describe
  (useful for surfacing a schedule back to a user or an AI author). Its 25-crate tree is the heaviest,
  driven by `derive_builder`/`darling` at build time. MIT, clean for `deny.toml`.
- **saffron 0.1.0** is the minimalist and the most production-proven path (it parses Cloudflare Workers
  Cron Triggers). Seven crates, no proc-macros, fastest to compile. The costs are that it is a single
  0.1.0 release on the old `nom` 5, shows little recent activity, has no seconds field, and its license
  is shipped as a file rather than an SPDX id, so `cargo deny` needs a one-line clarify mapping it to
  BSD-3-Clause.
- **cron 0.16.0** sits in between: actively maintained, seconds and year, dual-licensed, but the
  largest binary and no `L`/`W`/`#`.

Recommendation for the future spec: **croner if the schedule dialect should be rich and authorable by
an AI** (describe + specials), **saffron if minimal dependency surface and a proven Vixie parser
matter more** and per-second granularity is not required. Pin whichever in `Cargo.toml` and delete the
other two probe features.

### 7.2 The firing loop (driver / job runner)

| Library | Role | Persistence model | License | Note |
| ------- | ---- | ----------------- | ------- | ---- |
| **(first-party keeper)** | evaluate the trigger registry, call `exec` | none of its own; state lives in the trigger workspace + event log (section 4, section 5) | n/a | the recommended spine; see section 8 |
| **tokio-cron-scheduler** | in-process recurring scheduler | optional Postgres or Nats | MIT | useful if a non-Loom external store is acceptable |
| **apalis** | background job queue with retries, tower middleware, built-in cron source | SQLite / Postgres / MySQL / Redis | MIT | useful for high-volume external job processing; brings its own durable backend |

## 8. What "durable" gets us, when Loom is already durable

This is the load-bearing question for choosing a driver. `apalis` and `tokio-cron-scheduler` advertise
"durable" or "persistent" scheduling. Their durability means the job queue and the schedule survive a
process restart by being written to an external store (Postgres, Redis, SQLite, Nats).

For Loom that durability is mostly redundant, and in one respect actively harmful:

- **Redundant**, because Loom is the durable, crash-consistent store (the single-file journal of 0005,
  the Selvedge). The two pieces of state a scheduler must persist are the schedule set and the
  last-fired watermark. Section 4 and section 5 put both inside Loom: the schedule set is a versioned
  `trigger` workspace, the watermark is a cursor in the event log. A keeper that reads its state from
  Loom is durable by construction, with no second datastore to run, back up, or keep consistent.
- **Harmful if duplicated**, because pointing `apalis` at its own Postgres creates a second source of
  truth for "what is scheduled" that can diverge from the versioned, syncable, auditable copy in Loom.
  You would lose exactly the properties (branch/diff/merge/sync of the schedule, audit of every fire)
  that motivated putting the registry in the store. A schedule that lives in Postgres does not travel
  with the `.loom` file.

So the guidance is: **do not adopt an external durable job queue as the system of record.** Use a
cron parser (section 7.1) for expression evaluation, and a thin first-party keeper that treats Loom as
its durable backend. The value an `apalis` would otherwise provide (retries, backoff, concurrency
limits, dead-letter handling) is worth borrowing as patterns, and `apalis` remains a reasonable choice
if and only if a deployment already runs one of its backends and wants Loom triggers to ride an
existing job-processing fabric; in that case the external queue is a transport, and the trigger
registry and event log in Loom stay the system of record.

One caveat where external durability does help: a multi-node deployment that needs exactly one keeper
to fire a given schedule (leader election, distributed locks) is genuinely outside Loom's single-store
model, and there an external coordinator earns its keep. That is a scaling concern to name in the spec,
not a v1 requirement.

## 9. Use cases (carry into the spec)

- **An agent that schedules its own memory maintenance.** A nightly time trigger runs a program that
  compacts an agent's vector and document workspaces, summarizes the day's transcripts, and proposes
  the result on a branch for review before it merges into long-term memory. Branching memory to explore
  and merging only what worked is the README agent-memory use case; a trigger makes it recurring and
  hands-free, and the run-on-a-branch gate keeps it safe.
- **Config promotion on a clock.** "Tag the files workspace `main` after 15 minutes idle" (0015 section
  8.2) is a time trigger whose program proposes a tag; promotion policy decides the merge. GitOps-style
  config promotion (README use case 3) becomes time-driven without a separate CI system.
- **Reactive derived views and materialized indexes.** A change trigger bound to a `sql` workspace ref
  recomputes a derived view when its inputs change, cacheable on input digests (0015 section 10, 0011
  views). This is the database materialized-view pattern, made replayable by content addressing.
- **Audit and integrity sweeps.** A daily trigger runs `verify`/`fsck` (0009 section 4) over a
  workspace and appends the result to a ledger workspace, producing a tamper-evident record of
  integrity over time.
- **Retention and lifecycle.** A scheduled program enforces retention or legal-hold policy (0009
  section 7) by proposing deletions for review, so data governance runs on a cadence rather than by
  hand.

## 10. Proposed build, as a components matrix

| Component | Solves | Does NOT solve (yet) | Loom seam | Effort | Primary risk |
| --------- | ------ | -------------------- | --------- | ------ | ------------ |
| **Keeper (first-party driver)** | observe time and change; call `exec` with the stimulus as a seeded input | multi-node leader election; sub-second precision | host/daemon (P9); `exec` facade (0015 section 11) | Medium | clock skew handling; missed-fire policy |
| **Cron parser (croner or saffron)** | parse and evaluate schedules; next-after-base | timezone DST edge cases (delegated to chrono-tz) | inside the keeper | Low | dialect mismatch with author expectation |
| **Trigger registry (`trigger` workspace)** | versioned, syncable, auditable schedule set | cross-workspace trigger fan-out | 0014 reserved workspace, like `program` | Low-Medium | registry schema churn before promotion |
| **Event spine (append-log / ledger)** | audit, watermark, idempotency key | high-throughput streaming ingest | 0021 / 0018 facets | Low | log growth and compaction |
| **Change feed (`watch`)** | fire change triggers on ref/path moves | ordering across workspaces | 0009 section 8, 0003 section 4.6 | Medium | coalescing rapid changes |

The first-party keeper plus a cron parser plus the registry workspace is the minimal slice. The event
spine and change feed extend it to the change-driven class.

## 11. Open questions

1. **Time-trigger precision and the keeper tick**
   - **Context:** section 2 fires by evaluating a cron expression, but the keeper polls on some
     interval, so the actual fire is the schedule instant rounded to the next tick.
   - **Options:** (a) a fixed coarse tick (for example one minute, matching Vixie cron and saffron's
     dialect); (b) a configurable tick; (c) a timer-wheel that sleeps until the exact next instant.
   - **Recommendation:** (a) for v1 with saffron, (c) if croner's seconds field is adopted, since a
     seconds dialect implies callers expect sub-minute firing.
2. **Missed-fire policy default**
   - **Context:** section 5 lets a keeper backfill, collapse, or skip fires missed during downtime.
   - **Options:** (a) collapse to a single catch-up fire by default; (b) skip by default; (c) backfill
     every missed instant by default.
   - **Recommendation:** (a) collapse, with the binding able to override; backfilling every instant of
     a multi-day outage is rarely intended and (b) silently drops work.
3. **Cron dialect surface**
   - **Context:** section 7.1 shows the candidates disagree on seconds, year, and `L`/`W`/`#`.
   - **Options:** (a) adopt one library's dialect verbatim; (b) define a Loom cron dialect and validate
     against it independent of the parser.
   - **Recommendation:** (a) for v1 to avoid building a parser; document the chosen dialect explicitly
     so a later library swap is a known-scope change.
4. **Do triggered results sync or rebuild**
   - **Context:** the same derived-index policy captured in 0013 RD4 and governed by 0006 for sync. A
     triggered derived view is rebuildable from its inputs.
   - **Options:** (a) rebuild on the receiver; (b) transfer the result.
   - **Recommendation:** (a) prefer rebuild for derived, rebuildable results (consistent with 0015
     section 10 and 0013 RD4), transfer only when recomputation is more expensive than transfer.
5. **Trigger identity and authorization**
   - **Context:** a trigger acts on its own; in a shared Loom it must act as some principal
     (PRINCIPALS-AND-ACCESS-CONTROL-LANDSCAPE.md).
   - **Options:** (a) a trigger runs as a dedicated principal whose grants are the intersection of the
     binding's intent and the program's manifest grants; (b) a trigger inherits the grants of whoever
     created the binding.
   - **Recommendation:** (a), so a scheduled program is never more privileged than its manifest
     declares and is independently revocable; (b) leaks the author's full rights into automation.

## 12. Sources

- Trigger and derived-view sketch: `specs/0015-execution-and-logic.md` section 10, section 8.1-8.2.
- Watch / change feeds: `specs/0009-security-and-capabilities.md` section 8; `specs/0003-core-interface.md` section 4.6.
- Determinism and seeded inputs: `specs/0015-execution-and-logic.md` section 4.
- Footprint and dependency metrics: `prototypes/size-probes/` (`compare-cron-size.sh`), measured rustc 1.96.0, release+stripped.
- croner: https://crates.io/crates/croner (MIT). cron: https://github.com/zslayton/cron (MIT/Apache-2.0). saffron: https://github.com/cloudflare/saffron (BSD-3-Clause).
- apalis: https://github.com/sandhose/apalis (MIT). tokio-cron-scheduler: https://crates.io/crates/tokio-cron-scheduler (MIT).
