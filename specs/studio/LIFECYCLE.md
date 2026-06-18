# Lifecycles - Cross-Profile Work Choreography

**Status:** Target design. **Version:** 0.1.0-target.
**Capability:** `lifecycle`.

**Depends on:** `0061.md` (substrate: envelope, views, versioning, conflict records, shared
facilities), `0029` (triggers), `0036` (locks/CAS), `0043` (MCP serving), `JIRAISH.md`,
`PAGES.md`, `SURFACES.md`. **Relates to:** `ADOPTION.md` (G3 notifications, G5 bootstrap).

A lifecycle is the state machine *above* the profiles. JIRAISH workflows govern one issue;
Pages drafts govern one page. A lifecycle governs a unit of intent: a feature, a bug, an
incident, a document, as it moves across profiles: the spec page, its decomposed issues, its
decisions, and its snapshots are one choreographed thing. The canonical example:

```text
Ideate -> Draft -> Structure -> Ready -> Build -> Done -> Archive

Ideate/Draft   spec written as a mutable document; structure sketched as a mindmap
Structure      spec decomposed: issues created, dependencies wired into the graph,
               open questions queued
Ready          DoR checklist passes; status flips; a spec snapshot is frozen (CAS digest)
Build          issues consumed; decisions and redesigns appended to the ledger as they happen
Done           DoD passes; final snapshot; closing ledger entry
Archive        status flag; nothing deleted; history stays queryable
```

## 1. Model

```text
lifecycle definition   versioned data: named stages, per-stage entry/exit gates,
                       per-stage facet behaviors, per-stage surfacing rules
lifecycle instance     instance_id, definition version, subject scope (the entity set:
                       pages, structures, issues, decisions), current stage,
                       stage history (operations), gate evaluations
gate                   structured checklist (DoR, DoD): named boolean criteria,
                       each criterion machine-checkable (a predicate over the scope)
                       or human-attested (collected by elicitation)
stage transition       a sequenced, guarded operation: gates evaluated, snapshot policy
                       applied, surfacing recomputed, watchers notified
```

Rules:

- **Definitions are versioned data** like workflows; an instance records which definition version
  each transition validated against.
- **Transitions are operations** (0061 §2): attributable, auditable, revalidated on concurrency like
  any guarded write. Gate results are stored with the transition; "who attested which DoR item" is
  queryable forever.
- **Snapshot policy per stage:** entering Ready freezes the spec revision set (digests recorded on
  the transition operation); entering Done freezes the final set. "What did we commit to?" is
  `substrate.at` on the Ready snapshot; commitment-vs-delivered is the diff between the two
  snapshots (0061 §9-§10).
- **The subject scope is a graph neighborhood,** not a container: entities join a lifecycle instance
  by edge (`governed_by`), so an issue can join late (found during Build) and the instance's blast
  radius stays queryable.
- Lifecycles are optional and plural: a personal kanban user never touches them; an enterprise can
  define `incident`, `feature`, `doc-approval` lifecycles side by side.

## 2. Mapping to Every Layer

| Layer | How lifecycles surface |
| --- | --- |
| **Tools** | Stage-scoped surfacing (0061 §15.1): Ideate advertises document/structure tools; Structure advertises `structures_decompose_to_tickets`, dependency tools; Build advertises ticket/decision tools; Archive advertises read surface only. Advertised via `tools/list_changed`; the PEP, not surfacing, is enforcement. Current public lifecycle MCP tools use the underscore-native names `lifecycles_define`, `lifecycles_define_standard`, `lifecycles_definitions`, `lifecycles_definition`, `lifecycles_instantiate`, `lifecycles_instances`, `lifecycles_instance`, `lifecycles_active_set`, `lifecycles_active_clear`, `lifecycles_snapshot_plan`, `lifecycles_current_surface`, `lifecycles_transition`, `lifecycles_snapshots`, `lifecycles_snapshot`, `lifecycles_snapshot_content`, and `lifecycles_operation_log`. |
| **Prompts** | Curated MCP prompts bound to stages: Ideate offers "expand this idea into a spec skeleton"; Structure offers "decompose this spec into issues with dependencies"; Build offers "record a decision"; Done offers "write the retrospective from the ledger". The prompt list is part of the definition, so methods are shareable. |
| **Resources** | The instance is a resource (`loom://{workspace}/lifecycle/{instance_id}`) exposing stage, gates, scope, snapshots. The bootstrap view (0061 §8) lists the principal's active instances first; stage is the primary orientation signal for a fresh session. |
| **Apps** | Lifecycle rail in every entity app (Ticket Details, Document viewer show the governing instance and stage). A Pipeline app: instances as columns-by-stage, gate status badges, stuck-instance highlighting. Gate check runs as an elicitation checklist (SURFACES.md §2 pattern) for human-attested criteria. |
| **Visualizations** | Funnel/pipeline (instances per stage over time); stage-duration distribution (where does work stall); Sankey of stage transitions including backward moves (Ready -> Structure regressions are a smell worth seeing); commitment-vs-delivered diff view from the Ready/Done snapshots; time-lapse of an instance's graph neighborhood growing across stages. |
| **Workflows/automation** | Stage entry/exit fire 0029 triggers: entering Ready runs the snapshot; entering Build posts to the team channel (SLACKISH) and schedules milestone checks (calendar); entering Done generates the retro page skeleton (PAGES template). |
| **Watches/notifications** | Stage transitions are the coarse-grained notification event the digest policy (ADOPTION G3) prefers over per-operation noise: "feat-013 entered Build" is one notification summarizing dozens of operations. |
| **Locks** | Entering Ready may acquire an advisory freeze on the spec (0036 lease): edits during Build require either a `redesign` decision operation or a stage regression, enforced as gate policy, not filesystem permissions. |
| **Search** | Stage is a first-class filter (`substrate_search` scope): "open questions in Build-stage features". As-of search against stage snapshots answers "what did we know at Ready?". |

## 3. Assistant Choreography

The lifecycle gives the assistant its script. When the user says "let's build feature X", the
assistant instantiates a lifecycle, and from then on the *stage* tells it what "help me with X"
means: in Ideate it drafts prose, in Structure it proposes decomposition, in Build it routes new
information into decisions or issues, in Done it drafts the retro. The stage also tells it what to
do with surprises: a bug found during Build joins the scope by edge; a design flaw found during
Build elicits "record a redesign decision or regress to Structure?". This is the intent-mapping
layer that makes "let's build a new feature" deterministic without hardcoding any single method.

**A lifecycle definition is, functionally, a prompt (owner framing, 2026-07-04).** Embedding a
lifecycle exists for the benefit of the AI assistant: like a prompt, it tells the assistant what
data to fill out at each stage: which entities to create, which fields the gates demand, which
decisions to record. The standard library (`feature`, `bug`, `incident`, `design`) is therefore a
library of executable working methods, not documentation: instantiating one hands the assistant its
script for that unit of work.

## 4. Queued Design Work

These items are intentionally not blocking the 015 review pass. They are owned by Queue 8 lifecycle
tasks 500a/500b unless implementation exposes a new owner decision.

1. Definition schema and gate-predicate language (gates should reuse the 0061 §14 predicate tree
   for machine-checkable criteria).
2. Regression semantics: which stage moves are reversible, and what happens to snapshots on
   regression.
3. Interaction between lifecycle stage and JIRAISH workflow state (an issue Done inside a feature
   still in Build is normal; the reverse should trip a gate). Note (2026-07-04, prompt-5 session):
   the JIRAISH category enum now includes `accepted` (JIRAISH §8); gate predicates over an issue
   scope may require `category = accepted` (not merely done), e.g. a Ship-stage entry gate
   "all scope issues accepted". Machine-checkable via the 0061 §14 predicate tree unchanged;
   `complete` in gate vocabulary means category in {done, accepted}.

Resolved (owner, 2026-07-04):

- **Scope membership: explicit edges only.** An entity is in a lifecycle instance's scope exactly
  when a `governed_by` edge says so; nothing joins by rule. Worked example: a bug is filed while
  feat-013 is in Build. It does not silently join feat-013's scope just because it links to the
  governed page; the assistant (or a human) adds the `governed_by` edge, and only then does the
  bug count against feat-013's gates, snapshots, and rollups. Saved queries may power a "suggested
  members" panel, but a suggestion becomes scope only via an explicit edge operation. Consequence:
  "what did we commit to at Ready" is snapshot-stable and auditable.
- **Standard library ships**: `feature`, `bug`, `incident`, `design` definitions as forkable
  template data (owner-corrected roster; `design` replaces `doc-approval`).

## 5. Recommended Shape

The lifecycle engine is substrate-adjacent (a `loom-substrate` module): definitions, instances,
gates, transitions, snapshot policy, and stage-scoped surfacing hooks. Profiles stay ignorant of
lifecycles; they only see ordinary operations and edges. The dogfood planning store adopts the
canonical feature lifecycle above as its first instance; its stages map one-to-one onto the queue
discipline this repository already uses.

## 6. Current Source Boundary

Source-backed status 2026-07-08: `loom-substrate::lifecycle` exposes canonical lifecycle
definitions, stages, predicate gates by compiled predicate digest, attestation gates, lifecycle
instances with explicit subject refs, transition records, gate evaluations, current-stage
advancement, stale-stage and definition-version rejection, required entry/exit gate enforcement, and
freeze-scope snapshot-digest enforcement. It also source-backs snapshot plans, current-stage surface
projections, archive read-only surfacing, and the standard `feature`, `bug`, `incident`, and
`design` lifecycle definition library. Lifecycle operation-log records, deterministic control keys,
operation-change cursor projection, canonical snapshot records, and lifecycle conformance vectors are
also source-backed. `loom-lifecycle` source-backs the store-backed service boundary for definitions,
standard definitions, instances, transition validation, snapshot plans, current-stage surfaces,
snapshots, and operation logs. The top-level `loom lifecycle` CLI source-backs the same service
boundary for standard and CBOR definition writes, definition and instance reads, instantiation,
transitions with JSON gate evaluations, snapshot plans, current surfaces, snapshots, and operation
logs over the resolved workspace id. Lifecycle instantiate and transition now persist canonical
current-stage surface records, definition writes recompute stored surfaces for matching instances,
and transitions append deterministic lifecycle change records to the shared trigger fire-log
substrate. `loom-mcp` source-backs the public Lifecycle MCP tool vertical with structured output
schemas, and `loom-hosted` source-backs matching served REST and JSON-RPC routes for definitions,
instances, transitions, snapshot plans, current surfaces, snapshot metadata, stored snapshot content,
and operation logs. Standard lifecycle prompt refs point at registered MCP lifecycle prompts.
`loom-mcp` also source-backs session-bound active lifecycle surfacing through
`lifecycles_active_set` and `lifecycles_active_clear`; the active stage filters `tools/list`,
`get_tool`, and direct `tools/call`, and the existing `tools/list_changed` fingerprint reports
stage-surface membership changes. The standard `feature` lifecycle can be
instantiated for the planning store through the same source-backed service path, and the Studio status
view reports lifecycle operation-log summaries. Lifecycle stage transition and snapshot publication
revision rows are source-backed through the generic 0061 `ProfileTransaction`/`ProfileTransactionState`
helper. Generic trigger execution is source-backed by 0015/0029; durable keeper promotion, public
trigger facade projection, and richer app visualization remain owned by their respective trigger and
surface specs rather than this lifecycle engine boundary.
