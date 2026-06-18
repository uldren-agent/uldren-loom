# 0041 - Facet Lifecycle Hooks

**Status:** Draft, target. **Version:** 0.1.0.
**Capability:** `hooks`.

This spec defines a uniform mechanism by which `programs` (0015) register for and react to **lifecycle
events** of facet records - so an agent-authored program can, for example, label and move incoming mail
(Gmail-style filters), add a reminder when a calendar event is created, or de-duplicate a contact on
write. It is a single cross-facet mechanism, not per-facet bespoke callbacks, layered on the keeper and
trigger machinery of 0029 and the program/grant model of 0015/0027.

For PIM facets, lifecycle hooks are the first compute automation surface. Calendar, contacts, and mail
do not expose raw reserved-record CRUD through 0015 `StateAccess`; direct Rust and guest WASM PIM
operations are source-backed as domain-shaped calls that preserve each facet's protocol semantics. The
0029 trigger-fire bridge can run a current `engine=wasm` PIM program and append a fire record. The
target hook runner is engine-neutral and may also run 0015 `engine=cel` interpreted programs once that
profile is promoted. Hooks carry the changed domain record as input and let the handler call only
grant-approved domain operations.

## Current source-backed boundary

Source currently backs the execution side of PIM automation, durable registration records, canonical PIM
event envelopes, and first-pass PIM event emission. The source-backed pieces are direct Rust PIM
`StateAccess`, guest WASM PIM host calls, `run_as_context`, a trigger-fire bridge that resolves a
manifest and current `engine=wasm` body, a focused PIM trigger execution test plus executable
`pim-trigger` conformance runner that appends a fire record, `loom-core::hooks` registration and PIM
event envelope CBOR
round-trip tests, and `loom-core::hooks::hook_event_history` evidence for calendar, contacts, mail
ingest, mail flags, and mail move events when matching hooks are registered, plus `hook_plan_event`
enforcement for run-as presence, deterministic priority ordering, depth bounds, direct loop refusal,
required before-hook refusal, and trigger-candidate handoff to the existing 0029 execution bridge.
Remaining lifecycle-hook work is hosted or binding projection and the 0065 operator interface.

## 1. Model

A **hook** binds `(facet, event, scope) -> program`, run as a principal. When a commit changes a facet
record, the keeper (0029) evaluates registered hooks for the affected units and fires the matching
programs with the change as input. Hooks are bucket-2-adjacent registrations (stored as versioned
content so they sync and audit); fired-hook records and program runs follow 0029's fire log.

- **Program** - the handler is a 0015 program (deterministic, metered, sandboxed), authored by a user or
  an agent. Source-backed handlers are currently `engine=wasm`; target `engine=cel` handlers are
  interpreted CEL programs for inspectable AI-authored filters and bounded decision logic. Filters are
  just programs.
- **run_as** - each hook carries a principal (0026); the program executes with that principal's grants
  (0027), so a hook cannot exceed its owner's permissions.
- **Scope** - a hook may target a whole facet, a collection/mailbox/workspace, or a predicate over the
  record, so "only new mail in inbox" or "only VEVENT in the work calendar" is expressible.

## 2. Lifecycle events (uniform)

Every facet that opts in emits the uniform set, fired relative to the commit that applies the change:

| Event | When | Input to the program |
| --- | --- | --- |
| `before_create` | a new unit is staged, pre-commit | the proposed record; the program MAY reject or transform it |
| `after_create` | the unit is committed | the new record + commit id |
| `before_update` | an existing unit is staged for change | old + proposed record; MAY reject/transform |
| `after_update` | the change is committed | old + new record + commit id |
| `before_delete` | a unit is staged for removal | the record; MAY reject |
| `after_delete` | the removal is committed | the removed record + commit id |

`before_*` hooks run inside the staging transaction and can veto or rewrite (so a filter can set a label
before commit); `after_*` hooks run post-commit and may themselves stage further changes (which fire
their own hooks, with a depth bound to prevent loops). The unit is the facet's natural unit (0003b /
0001 A6): a row, key, id, point, node/edge, calendar entry, contact, or mail message.

## 3. Facet-specific events

Beyond the uniform set, facets declare domain events so programs need not pattern-match generic ones:

- **mail (0039):** `on_message_ingested` (new message stored, the Gmail-filter entry point),
  `on_flags_changed`, `on_moved` (folder/label change). Filters are programs registered on
  `on_message_ingested` that set labels / move to subfolders.
- **calendar (0037):** `on_event_added`, `on_event_updated`, `on_event_cancelled`,
  `on_occurrence_due` (a materialized occurrence reaches its start - the reminder/alarm entry point).
- **contacts (0038):** `on_contact_added`, `on_contact_updated`, `on_contact_merged` (the de-dup entry
  point).
- **Data facets (kv/document/sql/vector/...):** the uniform create/update/delete set is sufficient; a
  facet MAY add domain events as it is promoted.

## 4. Execution semantics

- **Ordering.** Hooks for one unit fire in a deterministic order (registration order within a priority
  class); a `before_*` rejection aborts the operation with a stable error.
- **Transactionality.** `before_*` runs in the same transaction as the change; a failing required hook
  rolls the change back. `after_*` runs after the commit; its own writes are a new commit.
- **Idempotency + loop safety.** Programs must be idempotent on replay (sync/checkout can re-present
  state); cascading hook depth is bounded and cycles are refused.
- **Determinism.** Programs are 0015 (deterministic, metered); a hook firing is recorded in the 0029
  fire log for audit and replay. Non-deterministic work (LLM calls) is a program that records its output
  as a seeded input, never an un-replayable side effect.
- **Authorization.** A hook's program runs under its `run_as` principal's grants (0027); registering a
  hook requires the `admin`/owner right on the target scope.

## 5. Facade (target IDL, illustrative)

```idl
interface Hooks {
    HookId register(LoomHandle handle, string workspace, string facet, string event, string scope,
                    ProgramRef program, PrincipalId run_as);
    void unregister(LoomHandle handle, string workspace, HookId id);
    bytes list(LoomHandle handle, string workspace);
}
```

## 6. Dependencies and gating

Depends on 0015 (programs/sandbox/metering), 0029 (keeper, scheduler, fire log, `run_as`), 0027 (grants
for `run_as` and registration), and each facet declaring its hook points (0037/0038/0039 do so
explicitly; data facets use the uniform set). 0015 PIM execution and the 0029 fire-record bridge are
source-backed; hook registration and facet event emission remain target work.

## 7. Resolved decisions

- **RD1 - One mechanism.** A single cross-facet hook system, not per-facet callbacks; facets only
  declare which events they emit.
- **RD2 - Filters are programs.** Mail filters, calendar reminders, and contact de-dup are 0015 programs
  registered on lifecycle events; the AI authors them like any other program.
- **RD3 - before/after split.** `before_*` can veto/transform inside the transaction; `after_*` reacts
  post-commit and may stage new changes (bounded, loop-safe).
- **RD4 - run_as authorization.** A hook executes with its owner principal's grants; it can never exceed
  them.
- **RD5 - PIM automation path.** Direct Rust and guest WASM PIM operations are source-backed, and the
  trigger-fire bridge can execute PIM programs. Calendar, contacts, and mail automation still starts
  through lifecycle hooks. Direct PIM program operations must be domain-shaped, not raw CRUD over
  reserved records.

## 8. PIM Lifecycle Completion Target

PIM lifecycle completion requires source-backed hook registration records, canonical event envelopes,
facet event emission, before/after policy enforcement, run-as authorization, deterministic priority
ordering, bounded cascade depth, loop refusal, and fire-history evidence. Registration records,
event envelopes, matching, and first-pass event history are source-backed in `loom-core::hooks`: records
store `(facet, event, scope) -> program`, `run_as`, `priority`, `enabled`, `required`, `max_depth`, and
an optional predicate; PIM event envelopes store workspace, facet, event, principal, collection or
mailbox, unit id, optional commit, before/after record bytes, depth, and causation digest. Matching
registrations are sorted by ascending numeric priority and then hook id. Calendar, contacts, and mail
facades emit event-history records only when at least one enabled hook matches the changed facet, event,
and scope. The emit path enforces depth and immediate loop policy before recording an event; required
or `before_*` hook policy failures reject the originating operation with `TRIGGER_DENIED`. The planner
returns existing `TriggerFireCandidate` values so `loom-compute::fire_trigger_candidate` remains the
only program execution bridge.

The first PIM event set is:

- calendar: `on_event_added`, `on_event_updated`, `on_event_cancelled`, and `on_occurrence_due`;
- contacts: `on_contact_added`, `on_contact_updated`, and `on_contact_merged`;
- mail: `on_message_ingested`, `on_flags_changed`, and `on_moved`.

Registration and administration are projected through 0065. This spec owns execution semantics; 0065 owns
operator-visible policy, status, fire-history inspection, and evidence export.

## 9. Deferred follow-up: mail filters (Gmail-style)

The concrete mail-filter feature is tracked here for a later pass, not the near-term roadmap. Planned
shape: the mail facet (0039) emits `on_message_ingested` from `ingest_message`; a user/agent registers a
0015 program on that event via the `Hooks` facade, `run_as` a principal (0027); the program inspects the
new `MailMessage` (parsed headers, body) and applies labels / moves mailbox by calling `set_flags` (and a
future move), exactly the Gmail filter model (RD2). It is gated on 0015 (programs/sandbox/metering) and
0029 (keeper + fire log) being source-backed, plus the `on_message_ingested` emission point in
`loom-core::mail::ingest_message` and hook registration here. Until those land, mail stores/indexes/flags
mail but applies no automatic filters. The same pattern serves calendar reminders (`on_occurrence_due`)
and contact de-dup (`on_contact_merged`).
