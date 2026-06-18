# 0029 - Events & Triggers (reactive execution over the store)

**Status:** Partial, trigger contracts, storage, fire history, change-trigger keeper decisions, and
Rust execution handoff source-backed; promoted facade, bindings, wire projection, and keeper loop
remain target. **Version:** 0.1.0-target.
**Optional facade:** capability `trigger`.

**Depends on:** 0015 (execution and logic; the `exec` facade, determinism, metering, run-on-a-branch
gate), 0014 (workspaces and reserved system paths), 0009 (security, signing, audit), 0003 (core
interface, transactions), 0005 (single-file storage, journal), 0006 (sync), 0030
(observability; the change-feed substrate the change-trigger class consumes). **Relates to:** 0018
(ledger) and 0021 (append-log), either of which backs the event spine; 0026/0027 (principals and
access control, proposed) for trigger identity. **Promoted from:** `EVENTS-TRIGGERS-LANDSCAPE.md`,
which this document supersedes as the normative source; that note is retained as research input.

This document specifies the reactive layer over Loom: running a stored program in response to time (a
cron schedule) or to a change in the store (a ref advance or a path write), without weakening the
determinism, metering, or run-on-a-branch guarantees of the compute face (0015). It is gated by the
`trigger` capability; absence yields `UNSUPPORTED`.

## Current implementation

The current Rust workspace implements the reusable trigger contract, reserved trigger binding storage,
fire-history storage, croner-backed time-trigger evaluation, the first core keeper decision API for
time and change triggers, and a Rust compute handoff that resolves a program digest into a manifest and
current `engine=wasm` body, runs it through the promoted `exec` facade, and appends a fire record. The
target keeper is engine-neutral and must also support 0015's target `engine=cel` interpreted programs
once promoted. It does not yet implement the public trigger facade, trigger ABI, binding wrapper, wire
projection, or durable keeper loop.

The source-backed boundary today is:

- `loom_core::error::Code` already contains the stable `TRIGGER_NOT_FOUND` and `TRIGGER_DENIED`
  values.
- `crates/loom-triggers` owns the reusable trigger binding contract, `TriggerKind`, execution mode,
  missed-fire and overlap policies, seeded stimulus hashing, `FireRecord`, `TriggerFireCandidate`,
  and canonical `loom.trigger.binding.v1` / `loom.trigger.fire.v2` CBOR codecs over `loom-types` and
  `loom-watch`. `FireRecord` stores both the concrete `TriggerStimulus` and its digest, so the fire
  log can serve as both an idempotency log and a keeper watermark source.
- `crates/loom-triggers` owns croner-backed time-trigger evaluation. It parses the approved croner
  dialect, parses IANA timezone names, evaluates due instants from the last time-stimulus watermark to
  a caller-supplied `now_ms`, applies `skip`/`collapse`/`backfill` due-instant selection, and returns
  deterministic next-wakeup milliseconds.
- `loom_core::triggers` stores trigger bindings as reserved system content under the `program` facet,
  exposes create/update/read/list/enable/remove helpers, appends fire records, and reads fire history
  through the canonical `loom-triggers` codecs. The current storage placement is
  `.loom/facets/program/triggers/...`, which avoids adding a new identity-affecting `FacetKind` before
  the trigger facade is executable.
- `loom_core::triggers::trigger_keeper_due` is source-backed for time-trigger and change-trigger fire
  decisions. It lists enabled bindings, resumes from the latest stimulus cursor in the fire log,
  deduplicates by stimulus digest, and returns a `TriggerKeeperPlan` containing ordered
  `TriggerFireCandidate` records and the next time-trigger wakeup. Change triggers poll the
  source-backed watch substrate. With `catch_up = false`, a change binding starts from the current
  branch tip; with `catch_up = true`, a change binding without prior fire history replays available
  watch history.
- `crates/loom-compute` contains the `run_as_context` authorization primitive. It resolves the
  trigger principal at fire time, fails closed with `TRIGGER_DENIED` if that principal is missing or
  disabled, and returns an `ExecContext` that reuses the public execution PEP.
- `crates/loom-compute::fire_trigger_candidate` is source-backed as the Rust execution handoff. It
  accepts a host-provided resolver for the program manifest and current `engine=wasm` body, injects
  canonical trigger inputs, runs gated/direct/batched execution, classifies failures into fire-record
  outcomes, and appends the fire record through `loom_core::triggers`.
- `crates/loom-compute::fire_trigger_candidate_with_state` is source-backed for host-supplied overlap
  state. `allow` runs even when a binding is already running. `skip-if-running` appends a `skipped`
  fire record without resolving the program. `queue` returns the candidate to the host driver without
  appending a fire record, so later execution is not deduped away by the fire log.
- `crates/loom-conformance` includes the executable `pim-trigger` runner for this handoff: a trigger
  candidate runs a WASM program through PIM host calls, commits a calendar entry, and appends one fire
  record. The same runner proves `skip-if-running` appends a `skipped` record and `queue` returns a
  candidate without appending a deduping record.
- `specs/0030-observability.md` defines the source-backed portable pull watch substrate that change
  triggers consume. Durable push handoff remains target.
- `crates/loom-core` contains raw append-log and ledger substrates that can back future trigger
  delivery or tamper-evident variants, but the current source-backed trigger fire log uses the
  reserved `program` facet placement above.

## 1. Goals & non-goals

**Goals.** (G1) Run a content-addressed program (0015 §7) on a schedule or on a change, against one
workspace. (G2) Preserve determinism: a triggered execution is still a pure function of program digest
and input digests (0015 §4), so it stays cacheable and replayable. (G3) Keep schedules and bindings as
first-class, versioned, syncable data. (G4) Make every fire auditable and idempotent. (G5) Keep the
keeper's portable scheduling and trigger-evaluation logic in the shared Loom core surface so the CLI,
WASM environment, bindings, and any service wrapper drive the same behavior.

**Non-goals.** (N1) Not an external daemon or distributed scheduler in v1: ordinary use has one active
keeper driver per opened Loom, and multi-process coordination over the same store belongs to host
deployment or a later coordination spec. (N2) Not a streaming engine: triggers fire per change or per
schedule tick, not per high-frequency event with exactly-once network semantics (idempotence is
achieved by content addressing, §6, not by a delivery protocol). (N3) Triggers do not decide
promotion; merging a proposed transition into a protected ref remains a separate policy (0015 §8.2).
(N4) A trigger grants no authority of its own; what it may touch is bounded by its program's manifest
(0015 §6) and its principal's grants (§8).

## 2. The keeper and the determinism boundary

A program may not read a wall clock or any ambient input; time and external change are declared,
seeded inputs (0015 §4). The reactive layer therefore lives outside the deterministic program
interior, in a shared core component called the **keeper**.

- The keeper's portable scheduling, binding lookup, stimulus encoding, and fire-record logic belongs
  in the Loom core surface. Hosts such as `loom-cli`, WASM, native bindings, or a service wrapper open
  a Loom and drive the keeper by supplying wakeups, a current instant, and host runtime execution.
- When a binding fires, the keeper captures the stimulus (the firing instant, or the changed ref/path
  set), encodes it as a small canonical blob, takes its digest, and passes that digest as a declared
  input to `exec.dry_run` / `exec.apply` (0015 §11). The recorded execution is
  `(manifest_digest, { "fired_at": <digest>, ... })` for a time trigger, or
  `(manifest_digest, { "event": <digest>, ... })` for a change trigger.
- Determinism (0015 §4) is therefore preserved unchanged: the program sees the stimulus as an input,
  never as an ambient read, and the run is replayable because the stimulus is pinned in its inputs.
- The keeper owns no separate durable database. Its source of record is the Loom content that stores
  trigger bindings and the fire log.

This boundary is normative: an implementation MUST NOT expose a clock or a change notification to a
program except as a seeded input through `StateAccess` (0015 §6).

## 3. Trigger storage and binding records

Bindings are data, stored as Loom-managed system content under the workspace model from 0014. The
target storage is a reserved trigger system facet or path under the Loom system area, not the older
typed-workspace model. The binding tree maps a binding id to a canonical binding record. Because it is
ordinary Loom content, the binding set is branchable, diffable, syncable, and auditable like any other
content, which is what makes a schedule a versioned source of truth.

```idl
struct Binding {
  id:        Uuid,                 // stable identity, assigned at creation
  kind:      TriggerKind,          // time or change (§4)
  program:   Digest,               // a program manifest (0015 §7), by content identity
  target_ns: NsSelector,           // the workspace the program runs against
  budget:    u64,                  // fuel ceiling (0015 §5)
  mode:      ExecMode?,            // gated | direct | batched; defaults per facet (0015 §8.1)
  options:   TriggerOptions,       // §5.1
  run_as:    PrincipalId?,         // §8; absent only in owner-mode Looms (0026)
  enabled:   bool,
}

enum TriggerKind {
  Time   { cron: string, tz: string },          // §5
  Change { watch: WatchSelector },               // §4; WatchSelector is defined in 0030
}
```

The binding record carries the program by manifest digest (its content identity), not by name, so a
binding pins an exact program version; updating the program is an explicit edit of the binding (or of
the `program` workspace ref the binding resolves through).

The program engine is defined by the 0015 manifest. Source-backed trigger execution currently proves
`engine=wasm` programs. Target trigger execution must not special-case WASM as the only program class:
when 0015 promotes `engine=cel`, trigger bindings must be able to reference those interpreted programs
where their read-only or constrained-action profile is valid for the trigger's effect.

## 4. Trigger classes

| Class | Fires on | Seeded input | Substrate |
| ----- | -------- | ------------ | --------- |
| **Time (cron)** | the schedule reaching its next instant | digest of the fired timestamp | the keeper's scheduler (§9) over a cron dialect (§5) |
| **Change** | a ref advance or a path write in `target_ns` matching the binding's `WatchSelector` | digest of the new commit, or the changed path set | the observability change feed (0030) |

Both reduce to the same shape: an external stimulus captured as a content address and handed to a
deterministic program. The change class consumes the observation layer specified in 0030; this
document defines how a change observation becomes a program execution, while 0030 defines how a change
is observed and delivered.

## 5. Cron dialect

The time-trigger dialect is **croner's** (the `croner` crate, MIT; selected and measured in
`EVENTS-TRIGGERS-LANDSCAPE.md` §7.1): five to seven fields, with optional seconds and year, and the
quartz-style special tokens `L` (last), `#` (nth weekday), and `W` (nearest weekday). An
implementation MUST parse and evaluate this dialect identically to the pinned `croner` version
recorded in the build, so that the next-fire set is a deterministic function of `(expression, tz,
base instant)`.

### 5.1 Loom options on a binding

The cron expression sets *when*; a binding's `options` set *how the keeper treats firing*, so that an
author tunes behavior without encoding it in the expression:

```idl
struct TriggerOptions {
  missed:    MissedFirePolicy,     // skip (default) | collapse | backfill   (§9, decision 11.2)
  catch_up:  bool,                 // whether to evaluate fires missed during downtime at all
  jitter_ms: u32,                  // optional spread to avoid thundering herds across many bindings
  overlap:   OverlapPolicy,        // skip-if-running (default) | allow | queue
}
```

`missed` defaults to `skip` and is settable per binding and as a Loom-wide default the owner
configures (decision 11.2). `overlap` governs a binding whose previous fire has not finished when the
next is due.

## 6. The event spine: audit, watermark, idempotency, catch-up

Each fire appends a record to an append-only log (the append-log facet 0021, or the hash-chained
ledger facet 0018 when tamper-evidence is wanted):

```idl
struct FireRecord {
  binding:        Uuid,
  stimulus:       TriggerStimulus, // fired timestamp or change cursor plus commit (§2)
  stimulus_digest: Digest,         // deterministic idempotency key for the stimulus
  proposed:       Digest?,         // the proposed commit, if any
  outcome:        Outcome,         // applied | proposed | skipped | denied | budget_exceeded | error
  cost:           u64,             // fuel used (0015 §5)
  fired_at_seq:   u64,             // log sequence
}
```

The log is three things at once:

- **An audit trail** (the `audit` capability, 0009 §7): what fired, on what stimulus, and what it
  produced.
- **A watermark store:** the last concrete stimulus per binding is the cursor the keeper resumes from.
  Time triggers resume from the last fired timestamp. Change triggers resume from the last watch
  cursor. The keeper holds no durable state of its own beyond what it reads from this log and the
  `trigger` workspace (§14, the durability rationale).
- **An idempotency key.** Executions are cacheable on `(manifest_digest, input_digests)` (0015 §2,
  §7), so a binding that fires twice on the same `stimulus` digest produces the identical state root
  the second time. At-least-once firing plus content-addressed results yields effectively-once
  outcomes; an implementation SHOULD dedup on `(binding, stimulus_digest)` against the log rather than
  build a delivery protocol (N2).

Catch-up after downtime is deterministic: cron evaluation against the watermark is a pure function, so
a keeper that was offline computes exactly which fires it missed and applies the binding's
`MissedFirePolicy` (§9), recording the decision.

## 7. Execution modes and promotion

A trigger inherits the compute safety story; it does not add a new one. Each fire runs through one of
the three execution modes (0015 §8.1): gated (run on a copy-on-write branch, produce a diff, merge
after review), direct (apply in place, for append-only high-frequency facets), batched. The mode
defaults per facet (0015 §8.1) and MAY be overridden per binding.

Promotion stays separate from the trigger (0015 §8.2): a trigger proposes a transition; whether the
proposal merges into a protected ref is a promotion decision made by policy or a human. The
"tag `main` after 15 minutes idle" pattern is a time trigger whose program proposes a tag, gated by
the same promotion policy as any merge.

## 8. Trigger identity and authorization

In an authenticated Loom (0026/0027) a trigger acts on its own and so MUST act as a principal. The
binding stores a **principal reference** (`run_as: PrincipalId`), not a snapshot of that principal's
permissions. This is normative and load-bearing: permissions are resolved at each fire against the
**current** grant model, never against the rights the principal held when the binding was created.

- At each fire the keeper resolves `run_as` to its **current** grants (0027). If the principal is
  disabled, deleted, or lacks a required grant (for example `exec` on `target_ns`), the fire **fails
  closed**: it does not execute, and the outcome is recorded as `denied` (§6). This mirrors the CEL
  guard's fail-closed discipline (0015 §4).
- A binding MAY carry an optional `scope` bounding the trigger to a subset of the principal's rights
  (least privilege); the effective authority of a fire is then the intersection of the principal's
  current grants and that scope. With no scope, the trigger runs with the principal's current grants
  in full.
- Independently, the program's manifest grants (0015 §6) bound what the program's code may touch. A
  fire can never exceed either the principal's authorization or the program's manifest, whichever is
  narrower.

A binding's `run_as` is itself **reassignable by management**. A principal holding the `admin` right
(0027) - typically root or a delegated manager - MAY change a binding's `run_as` to a different
principal with `reassign` (§11), and MAY create a dedicated **service principal** (0026, a non-human
account that carries a role) and point the binding at it. This is the recommended path for longevity:
a binding is created running as the individual who authored it (the default), and an administrator
moves it onto a durable service principal so a person leaving does not silently stop the automation
(the consequence below). Reassignment is itself an `admin`-gated, audited operation (0009 §7); it
changes only the identity a fire resolves against, never a snapshot of permissions.

Consequences the deployment should plan for: because authority is dynamic, revoking a principal's
grant immediately narrows every trigger that runs as it, and disabling a principal stops its triggers
(fail-closed). An individual MAY own a binding, and that is the default; for longevity, production
bindings SHOULD be reassigned to a durable service principal (above) rather than left on an
individual. (Decision 11.5.)

## 9. Keeper scheduling and missed fires

The keeper targets the exact next instant rather than a coarse poll: it computes the next instant from
the cron dialect (§5) and exposes that instant to the host driver. The host runtime may sleep until
that instant and call back into the keeper for due bindings (a timer wheel or equivalent). This matches
croner's seconds-granularity dialect, where callers expect sub-minute precision. (Decision 11.1.)

Missed fires (the keeper was down across one or more due instants) are governed by the binding's
`MissedFirePolicy`, default `skip`:

| Policy | Behavior |
| ------ | -------- |
| `skip` (default) | drop missed instants; fire only the next due one |
| `collapse` | fire once to catch up, regardless of how many instants were missed |
| `backfill` | fire once per missed instant, in order, each with its own seeded timestamp |

The default is owner-configurable Loom-wide and overridable per binding. `backfill` is offered because
some workloads (for example per-day rollups) are defined per instant, but it is never the default
because backfilling a long outage is rarely intended. (Decision 11.2.)

## 10. Sync of triggered results

A triggered execution often produces derived, rebuildable state (a materialized view, an index, a
rollup). Such results are **excluded from sync and rebuilt on the receiver**, consistent with the
derived-index policy in 0013 RD4 and with sync minimality (0006
§S2). This is the data-safe choice: the canonical source of truth is the inputs plus the deterministic
program, so a receiver reconstructs the result rather than trusting transferred computed state, and
two peers cannot diverge on a derived value. Non-derived results a trigger writes (ordinary commits to
`target_ns`) sync as any other content. (Decision 11.4.)

## 11. Interface sketch (`trigger` facade)

A new optional facade beside `exec` (0015 §11), present iff the `trigger` capability is advertised.
Illustrative IDL only (non-normative):

```idl
interface Triggers {
  create(binding: Binding): Uuid
  update(id: Uuid, binding: Binding): void
  enable(id: Uuid, on: bool): void
  remove(id: Uuid): void
  list(filter?: { kind?: TriggerKind; target_ns?: NsSelector }): List<Binding>
  // Reassign the identity a binding runs as (requires the `admin` right, 0027). The target is any
  // principal, typically a service principal (0026). Audited (0009 §7). (§8, decision 11.5.)
  reassign(id: Uuid, run_as: PrincipalId): void
  // Inspect the event spine (§6).
  history(id: Uuid, from_seq?: u64): List<FireRecord>
  // Fire now, ignoring the schedule, for testing; honors authorization (§8) and modes (§7).
  fire_now(id: Uuid): ExecResult
}
```

The trigger-specific errors are already registered as stable core codes: `TRIGGER_NOT_FOUND` and
`TRIGGER_DENIED` (the `run_as` principal is not authorized at fire time, §8). Budget and determinism
failures depend on the promoted `exec` envelope or future stable execution codes; the current
`loom_core::error::Code` registry does not include execution-specific variants.

## 12. Interaction with existing specs

- **0015** - triggers invoke the `exec` facade; determinism, metering, modes, and promotion are
  inherited unchanged; time and change arrive as seeded inputs.
- **0014** - trigger bindings are Loom-managed system content under reserved workspace paths.
- **0030** - the change-trigger class consumes the observability change feed; this spec defines the
  execution side, 0030 the observation side.
- **0018 / 0021** - the event spine is an append-log or ledger workspace.
- **0009** - the fire log is the audit record; trigger identity composes with access control.
- **0026 / 0027** - `run_as` is a principal; authorization is resolved dynamically at fire time (§8).
- **0006** - derived triggered results rebuild on the receiver rather than transferring (§10).
- **0003 / 0008** - the `trigger` facade projects over the wire protocols like the other facades.

## 13. Security

This document inherits the 0009 and 0015 threat models. The relevant adversary is A-program (0015
§13): untrusted or AI-authored logic. Triggers do not widen it, because a fire reaches state only
through the program's manifest grants and the `run_as` principal's current grants (§8), runs under a
budget (0015 §5) and the run-on-a-branch gate (0015 §8), and is recorded in the audit log (§6). The
new surface a trigger adds is autonomy (it runs unattended), which is mitigated by fail-closed
authorization (§8), the per-binding budget, and the requirement that promotion to a protected ref
stays a separate, human-or-policy decision (§7).

## 14. Resolved decisions

For traceability against `EVENTS-TRIGGERS-LANDSCAPE.md` §11:

1. **Scheduling precision (11.1):** fire at the exact next instant via a per-binding timer, not a
   coarse poll, matching croner's seconds dialect. Baked into §9.
2. **Missed-fire default (11.2):** default `skip`, owner-configurable Loom-wide and overridable per
   binding, with `collapse` and `backfill` available. Baked into §5.1 and §9.
3. **Cron dialect (11.3):** adopt croner's dialect, extended with per-binding Loom `options` (§5.1) so
   behavior is tuned on the binding, not in the expression. Baked into §5.
4. **Sync of triggered results (11.4):** derived, rebuildable results are excluded from sync and
   rebuilt on the receiver (consistent with 0013 RD4 and 0006 §S2); the inputs plus the program are
   the source of truth. Baked into §10.
5. **Trigger identity (11.5):** the binding stores the principal's id (`run_as`), and permissions are
   resolved against the current grant model at each fire, fail-closed; an optional per-binding scope
   bounds it to least privilege, and the program manifest always applies on top. An individual may own
   a binding (the default), and a principal with the `admin` right may `reassign` it to another
   principal, typically a service principal (0026), as the durable path. Baked into §8 and §11.

6. **Keeper placement and system of record:** the keeper's portable scheduling and evaluation logic
   belongs in the shared Loom core surface, while hosts only drive it by opening the Loom and supplying
   runtime wakeups. Loom content is the durable source of record; an external job system, when present,
   may drive wakeups but MUST NOT own trigger binding state. Baked into §2 and §6.
7. **Multi-process firing:** v1 has no separate daemon, distributed scheduler, or leader election.
   Keeping the keeper inside Loom removes the server-daemon interpretation, but it does not provide
   cross-process coordination for multiple hosts opening the same store. A deployment that runs
   multiple active keeper drivers over one Loom must coordinate externally until a later coordination
   spec defines claims or locks. Baked into N1.
8. **Change-trigger coalescing:** debounce and coalescing are owned by 0030 because they are
   observation semantics. A change trigger consumes whatever event or commit range 0030 produces.
   Baked into §4.

## 15. Unfinished work

- (P1) Complete durable loop driving for host keepers. Cron wake computation, time-trigger due
  evaluation, missed-fire due-instant selection, overlap policy evaluation, change-trigger due
  evaluation over the watch substrate, and Rust execution handoff are source-backed.
- (P1) Define workspace/facet projection rules for the reserved trigger storage placement.
- (P1) Implement the trigger facade, manual `fire_now`, and reassignment.
- (P1) Promote trigger execution into keeper/facade surfaces that call the source-backed Rust handoff.
- (P0) Apply the source-backed `run_as_context` primitive to future public trigger projections.
- (P1) Connect change triggers to the source-backed 0030 watch feed. Durable trigger delivery,
  acknowledgements, reconnect replay, and host-side redelivery remain owned by 0035.
- (P0) Project the public trigger facade through IDL, C ABI, bindings, wire protocols, and CLI.

## 16. Sources

- Research input and library metrics: `specs/EVENTS-TRIGGERS-LANDSCAPE.md` (croner selected, §7.1;
  durability analysis, §8).
- Compute facade, determinism, metering, modes, promotion: `specs/0015-execution-and-logic.md`
  §4-§8, §11.
- Reserved system paths and workspace roots: `specs/0014-workspaces.md` §7.
- Event spine: `specs/0018-ledger-layer.md`, `specs/0021-append-log-layer.md`.
- Derived-result sync precedent: `specs/0013-extended-capabilities.md` Q2; `specs/0006-synchronization.md` §S2.
- croner (MIT): https://crates.io/crates/croner.
