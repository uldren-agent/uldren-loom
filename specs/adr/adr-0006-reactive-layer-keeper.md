# ADR-0006 - Reactive layer via shared keeper logic

**Status:** Accepted - **Date:** 2026-06-16 - **Deciders:** Nas
**Related:** 0029 (events and triggers), 0030 (observability), 0015 (the `exec` facade and
determinism), 0009 §8 (the `watch` menu item), `EVENTS-TRIGGERS-LANDSCAPE.md` (the library scoping
and durability analysis).

## Context

Loom is passive: it acts only when called. Adding a reactive layer (run a stored program on a cron
schedule or on a change) introduces a clock and external stimuli into a system whose compute face is
deliberately deterministic and clock-free (0015 §4). The specs already sketched change-driven
triggers (0015 §10) and a `watch` feed (0009 §8), but there was no time-driven scheduler, and the
question of where the reactive machinery lives, and what durably backs it, needed deciding.

## Decisions

1. **Portable keeper logic lives with Loom, while hosts drive wakeups.** The keeper's scheduling,
   trigger lookup, stimulus encoding, and fire-record logic belong in the shared Loom core surface.
   CLI, WASM, bindings, and service hosts open a Loom and drive the keeper by supplying wakeups, a
   current instant, and host runtime execution. `loom-core` stays deterministic and single-actor:
   reactivity is a portable evaluation component, not a separate server daemon. (0029 §2.)

2. **Time is a seeded input, never an ambient read.** When a binding fires, the keeper captures the
   instant (or the changed ref/path set), encodes it canonically, and passes its digest to the program
   as a declared input. The program never reads a clock, so determinism (0015 §4) is preserved and the
   run is replayable. This is the smart-contract "keeper/automation" pattern minus consensus. (0029 §2.)

3. **Loom is the keeper's durable backend; no external job queue is the system of record.** The two
   pieces of scheduler state - the binding set and the per-binding last-fired watermark - live in a
   reserved `trigger` namespace and the append-log/ledger event spine, so they are versioned,
   syncable, auditable, and travel with the `.loom` file. An external durable queue would be a second,
   diverging source of truth. (0029 §3, §6, §14.)

4. **Idempotency comes from content addressing, not a delivery protocol.** Because an execution is
   cacheable on `(program, inputs)` (0015 §2), re-firing on the same stimulus yields the identical
   state root; at-least-once firing plus content-addressed results gives effectively-once outcomes.
   The keeper dedups on `(binding, stimulus)`. (0029 §6.)

5. **`run_as` is a dynamic principal reference, not a permission snapshot.** A binding stores a
   principal id; permissions are resolved against the current grant model at each fire, fail-closed.
   An optional per-binding scope bounds it to least privilege, the program manifest still applies, and
   an `admin` may reassign a binding onto a durable service principal. Revoking a grant immediately
   narrows the trigger; disabling the principal stops it. (0029 §8, ADR-0005.)

6. **croner is the cron dialect; the keeper fires at the exact instant.** croner was selected on
   metrics (`EVENTS-TRIGGERS-LANDSCAPE.md` §7.1: dialect richness, maintenance, small footprint) and
   gives optional seconds plus `L`/`#`/`W`. Per-binding options (missed-fire policy, jitter, overlap)
   ride the binding, not the expression. Missed fires default to `skip`. (0029 §5, §9.)

7. **Observability is its own leg, the observe side of the reactive pair.** `watch` (0009 §8) is
   widened in 0030 into change feeds across every facet; a change trigger is a built-in subscriber
   whose handler is a program. 0030 owns observation, 0029 owns execution. Derived, rebuildable
   results are excluded from sync and rebuilt on the receiver (0013 RD4, 0029 §10).

## Alternatives considered and rejected

- **A clock inside the sandbox** - rejected: it breaks determinism and replay (Decision #2); time
  must be a seeded input.
- **An external durable job queue (apalis/tokio-cron-scheduler) as the system of record** - rejected:
  redundant with Loom's own durability and harmful because it forks the source of truth for "what is
  scheduled," losing branch/diff/sync/audit and not travelling with the file (Decision #3). Such a
  queue may still be a transport where one already runs, with Loom keeping the registry.
- **Storing a snapshot of the creator's permissions on the binding** - rejected: it would freeze
  stale privilege; dynamic resolution makes revocation immediate (Decision #5).
- **An exactly-once network delivery protocol** - rejected as unnecessary: content addressing makes
  outcomes effectively-once without it (Decision #4).

## Consequences

- **Positive:** the deterministic kernel is untouched; schedules are first-class versioned data;
  triggers inherit the compute safety story (budget, run-on-a-branch, fail-closed authorization);
  idempotency and missed-fire catch-up are deterministic. A keeper prototype exists
  (`prototypes/keeper/`).
- **Negative / accepted:** single-node firing in v1 (multi-node leader election is deferred, 0029
  §15); a binding left on an individual stops when that person is disabled, so production bindings
  should be reassigned to a service principal.
- **Follow-on:** the `Code` enum gains `TRIGGER_NOT_FOUND`, `TRIGGER_DENIED`, `CURSOR_INVALID`
  (0003 §8); `trigger` and `watch` (v2) are registered (0010 §4); build sequencing is P13.

## Open

None blocking. Multi-node firing and change-feed coalescing are open questions in 0029/0030.
