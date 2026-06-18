# Jiraish - Shared Issue and Workflow Tracker

**Status:** Target design with a source-backed identity foundation. **Version:** 0.1.0-target.
**Capability:** `tickets`.

This document defines a Jira-style issue, project, board, and workflow tracker on top of Loom. It is an
Studio application profile, not a replacement for the core filesystem, SQL, graph, append-log,
execution, trigger, synchronization, access-control, or MCP specs.

Current source backs the reusable `loom-tickets` component foundation: project records, canonical
project key-prefix validation, monotonic issue-key allocation, issue records, typed field values,
policy labels, key lookup, canonical CBOR round-trips, scalar field conflict classification, and the
pure deterministic workflow transition validator. Current source stores an optional active workflow
definition on each ticket project, routes ticket status changes through that configured workflow at
the service boundary, and falls back to the versioned default workflow when a project has not set one.
CLI JSON updates and MCP `tickets_update` expose the transition path via `target_status`; legacy
action strings are compatibility aliases, not the status-transition model. The default workflow
uses the normalized status set `backlog`, `planned`, `ready`, `in_progress`, `blocked`,
`waiting_for_review`, `accepted`, `rejected`, and `closed`. Assignment is an ownership field update,
not a workflow status. The default workflow includes auditable rework edges from accepted and
rejected work back to active `in_progress` work, and operation history records
`ticket.transitioned` events with applied workflow-validation records. Current source also backs
board definitions, sprint state and membership, explicit sprint-close disposition, and portfolio
hierarchy adjacency validation. Current source backs planning primitives for capacity records, load
calculation, progress rollups, roadmap item validation, and dependency validation. Current public
promotion covers ticket project create/re-key/release, ticket create/update/get/history, and
ticket-owned relation set/remove through CLI, MCP, hosted REST, and hosted JSON-RPC. The MCP ticket
tools remain host/composite local-only over remote MCP until a ticket IDL contract exists. Planning UI
projections, imports, broader Jiraish MCP tools, remote-Loom ticket IDL promotion, and
conformance-vector promotion remain target work. The durable profile bridge
uses private, incrementally-mutated tables for projects, prefix routes, issues, project-number
lookups, and operations. A compact control-plane state record pins the profile table roots and next
operation sequence for optimistic conflict checks; it does not contain a whole profile snapshot or
operation log.

Jiraish exists because an issue tracker is not just a table of tickets. It exposes concurrent project
state to people and AI agents: issue fields, comments, attachments, workflow transitions, rank ordering,
sprints, boards, links, automation, audit trails, reports, and notification streams. Ordinary users
expect immediate collaborative behavior, not branch divergence and manual merges.

## 1. Contract Boundaries

The design builds on these contracts:

- `0006-synchronization.md` defines current sync as movement of immutable content-addressed objects plus
  mutable workspace refs. Current branch sync is fast-forward only.
- `0021-append-log-layer.md` defines the current queue boundary and the target structured stream shape.
- `0011-tabular-and-query-layer.md` and the SQL facet provide a natural projection for issue tables and
  reports, but the issue operation log remains the collaboration source of truth.
- `0016-graph-layer.md` is the natural projection for issue links, dependency graphs, epics, and parent
  relationships.
- `0015-execution-and-logic.md` and `0029-events-and-triggers.md` define target execution and reactive
  automation boundaries. Jiraish automation must preserve their determinism, metering, and audit model.
- `0031-end-to-end-encrypted-sync.md` defines the blind-replica topology where a local client holds keys
  and the remote stores ciphertext by opaque labels.
- `SLACKISH.md` and `DRIVEISH.md` define the Studio pattern used here: local-first replicas, blind cloud
  by default, keyed workers where content-aware compute is required, MCP resources, and durable
  callbacks.
- `0061.md` defines the shared operation substrate: envelope, sequencer, durable cursors, order
  tokens, conflict records, annotations, entity versioning, projections/views, and cross-facet
  search. §5 and §6 reference it directly (local envelope/sequencer text rewritten 2026-07-04);
  this document keeps only operation kinds, validation rules, projection layouts, and tool
  vocabulary.
- `SURFACES.md` defines the human experience layer (MCP Apps, elicitation flows, visualizations)
  rendered over this profile's projections.

This document depends on those boundaries. It does not make raw current branch sync sufficient for five
local assistants concurrently editing the same project board.

## 2. Product Model

A Jiraish organization exposes projects, issues, boards, workflows, and planning artifacts:

```text
issues
  projects
  boards
  workflows
  issue-types
  fields
  issues
  comments
  attachments
  links
  ranks
  sprints
  releases
  filters
  reports
  automations
  cursors
  audit
  retention
```

The user-facing contract is:

- issue creation is durable and idempotent;
- comments append without overwriting each other;
- workflow transitions are validated against the current workflow and actor grants;
- rank and sprint changes converge to a stable board order;
- concurrent field edits do not silently discard data;
- agents can observe, triage, assign, transition, comment, and create issues under scoped grants;
- audit history can reconstruct who or what changed an issue and why.

## 3. Cloud and Encryption Model

The same cloud topologies from Slackish and Driveish apply.

```text
Private tracker:
  Local Loom replicas hold keys and full project state.
  Loom Cloud is blind storage, operation sequencing, notification, and opaque coordination.
  Search, reports, and agent execution happen locally or in a tenant-controlled keyed worker.

Managed enterprise tracker:
  A keyed service runs inside the tenant trust boundary.
  It can index, report, summarize, run workflow automation, and host agents.

Hybrid tracker:
  Blind Loom Cloud stores canonical encrypted issue history.
  Selected projects or issue sets are mirrored to keyed compute replicas for approved workloads.
```

A blind remote can sequence issue operations and coordinate opaque rank or workflow updates, but it
cannot inspect issue text, compute semantic reports, summarize backlogs, classify incidents, or evaluate
content-aware policies.

## 4. Storage Layer

Jiraish uses Loom content-addressed objects for immutable payloads and Merkle-backed indexes for current
project state.

```text
tracker root
  operation log root
  project index root
  issue index root
  issue history index root
  board index root
  workflow index root
  rank index root
  sprint index root
  link graph root
  comment index root
  attachment index root
  audit index root
```

The operation log is the source of truth. Tables and graphs are projections:

- SQL projection for issue queries, reports, filters, and dashboards;
- graph projection for links, blockers, duplicates, epics, parent-child relationships, and dependencies;
- append-log projection for issue history and audit trails;
- search and vector projections for full-text and semantic discovery where keys are available.

Ticket identity is a Loom-generated UUID `ticket_id`. Human-readable keys such as `PROJ-123` are mutable project-scoped
aliases assigned by the sequencer. Renaming a project must not change ticket identity.
Current source backs seeded ticket type definitions for Epic, Story, Task, Bug, Spike, and Subtask.
Definitions carry stable `type_id`, display name, semantic kind, retired state, and optional project
applicability. The existing canonical ticket record still stores the built-in type tag until the
ticket schema lane reaches the one-time Matrix migration; custom and imported type definition
persistence and public CRUD are target work for the remaining tickets-schema queue.

### 4.1 Ticket Keys, Re-Key, and Moves (pinned 2026-07-04)

Profile policy over the 0061 §4 allocator. References bind to stable ids at sequence time; aliases
are display (0061 §16.6, ADOPTION G4).

**Format.** `PREFIX-N`. Prefix matches `[A-Z][A-Z0-9]{1,9}`, unique organization-wide. `N` is a
per-project monotonic counter starting at 1, never reused, gaps permitted - deletion leaves a hole
and the counter never rewinds. All ticket types (epics, subtasks) draw from the same project
counter. Keys are URL-safe, case-insensitive on lookup, canonical uppercase. The format is
Jira-compatible so imported keys are preserved verbatim; import sets the counter past the highest
imported number. Key allocation is content-free (`prefix + counter`), so a blind sequencer
allocates it.

**Project re-key (LOOM -> CORE).** One operation changes the project prefix route while every ticket
keeps its immutable number. `LOOM-52` resolves through the retired `LOOM` route and `CORE-52`
through the active `CORE` route, without rewriting issue records or materializing one alias per
issue. Current source backs derived ticket-key results through `substrate_alias_resolve`, rejects
generic alias bindings that use an active or retired ticket prefix, and exposes
`tickets_project_rekey`. Re-key appends a ticket operation and durable audit record. The retired
prefix is reserved organization-wide; a new project cannot claim it and normal public CLI, MCP, REST,
and JSON-RPC surfaces cannot release it. This keeps old-key redirects stable by default. The
project-admin policy remains target work until tickets have a first-class ACL surface.

**Cross-project move.** `ticket.moved` preserves `ticket_id` - links, mentions, and history are
unaffected. The issue takes the next key from the target project's counter (the number is not
preserved; it may collide); the old key becomes a retired alias via the same redirect machinery as
re-key. Values of fields whose project context (§20.1) is absent in the target are kept but flagged
unavailable-in-context; the move operation records what dropped.

**Secondary keys and multi-homing (owner decision 2026-07-04, Asana import session).** An issue is
owned by exactly one project - the key assigned at creation by the owning project is its
**primary key**: canonical display, one per issue. An issue homed into other projects via
membership edges (§21) may additionally be assigned a **secondary key** drawn from the host
project's counter - an explicit `issue.key_assigned` operation (or a membership-edge option),
never automatic on multi-home. Invariants: at most one active key per (issue, project); secondary
keys follow the same format, counter, and never-reuse rules; they resolve to the issue like any
alias and render flagged `secondary` beside the primary. A cross-project move into a project that
already holds a secondary key for the issue **promotes that key to primary** rather than drawing a
new number; the old primary retires as usual. References bind to `ticket_id` whichever key was
written (0061 §19), so primary, secondary, and retired keys are all rename- and move-stable.

## 5. Operation Log

The source of truth for shared tracker coordination is an operation log, not raw branch sync.

Operation kinds:

```text
project.created
project.updated
board.created
board.updated
workflow.created
workflow.updated
project.rekeyed
project.cutover
field.created
field.updated
field.retired
issue.created
issue.field_updated
issue.transitioned
issue.assigned
issue.ranked
issue.sprint_changed
issue.release_changed
issue.linked
issue.unlinked
issue.moved
issue.deleted
issue.restored
sprint.created
sprint.started
sprint.scope_changed
sprint.closed
release.created
release.released
release.archived
comment.added
comment.edited
comment.redacted
attachment.added
attachment.removed
automation.created
automation.updated
automation.fired
audit.recorded
```

Operations use the 0061 §2 canonical envelope - this profile no longer defines a local envelope
(the former field list is superseded per §24). Profile mapping into the envelope: `app_id` is
`tickets`; `scope_id` is the project; `target_entity_id` is the issue, board, sprint, or
release; `base_entity_version` drives the §7.2 conflict rules. Agent-authored operations carry the
0061 §2 agent identity block.

The visible issue state, board columns, reports, and audit views are projections over operations.

## 6. Multi-Replica Coordination

Five laptops, each with a local Loom and an AI assistant, cannot safely coordinate edits to the same
project board by independently editing the same workspace branch and relying on ordinary current sync.
Current sync moves objects and fast-forwards refs. It rejects divergent branch tips instead of choosing
or merging a winner.

Jiraish therefore needs explicit coordination above raw branch sync.

### 6.1 Blind Central Operation Sequencer

The recommended enterprise default is the 0061 §3 sequencer contract in its blind deployment mode -
this profile no longer defines a local sequencer protocol (superseded per §24). The sequencer
authorizes by label, validates envelope shape without reading payloads, assigns the tracker
sequence, allocates issue keys (§4.1 - content-free, so blind-allocatable) and rank tokens
(0061 §5), persists the opaque operation, advances the tracker root by compare-and-swap, and emits
wakeups; replicas pull, decrypt, verify digests, update projections, and advance cursors
(0035).

Profile-specific consequence: content-dependent validation (workflow transitions §7.3, field
validation §20.1) is **not** the sequencer's job - it is deterministic post-sequence projection
state, so blind mode keeps full workflow semantics.

Properties:

- total order for project and board operations;
- stable issue keys and rank tokens;
- simple user experience;
- durable replay by sequence;
- zero-knowledge compatible;
- no hosted search, reports, summaries, DLP, or workflow automation unless a keyed worker is added.

### 6.2 Per-Actor Operation Logs

The per-actor-log topology is a deployment mode of the same 0061 §3 contract, not a separate
protocol: each device or assistant appends to its own log
(`tickets/{ticket_id}/actors/{actor_id}/operations`), and clients deterministically merge
authorized logs into the project view. The profile rules this document pins (§7.2 conflict matrix,
§7.3 validation function, §7.5 planning policy, 0061 §5 tie-breaks) *are* the deterministic merge
function; the topology adds only causal metadata and compaction rules preserving audit proofs.

Actor logs are appropriate for decentralized or offline-heavy issue trackers. They are harder for
users because board order, issue status, and assignment state can be provisional until logs
converge.

### 6.3 Keyed Central Issue Server

A keyed central issue server is operationally closest to Jira. It reads content, validates workflow
conditions, runs automations, indexes search, computes reports, summarizes issues, invokes hosted
agents, and broadcasts updates.

This is compatible with Loom, but it is not zero-knowledge. It is appropriate when the tenant chooses
hosted compute inside a trust boundary.

### 6.4 Decision

Jiraish should support the blind central operation sequencer as the default shared-project coordination
model. Actor logs remain a target topology for decentralized or offline-heavy deployments. A keyed
central issue server is a deployment option, not a requirement.

Raw current branch sync alone is not a sufficient coordination protocol for multiple local assistants
editing the same board or issue set.

## 7. Concurrent Editing Semantics

Jiraish must handle concurrent edits by field, operation type, and workflow state.

### 7.1 Append-Only Fields

Comments, audit records, work logs, and attachment additions are append operations. Concurrent appends
merge by operation sequence. Redaction is a later operation and does not erase the original audit fact
unless a retention policy permits hard deletion.

### 7.2 Scalar Fields (conflict policy matrix pinned 2026-07-04)

Scalar fields include title, priority, due date, story points, and assignee. Concurrent updates
must not silently disappear. The matrix below is profile policy over the 0061 §6 conflict record;
outcomes are deterministic projection state, computable by any keyed replica (same validation-site
rule as §7.3).

**Policy classes.** Every scalar field has exactly one class:

```text
lww           later operation applies; no conflict record (both operations remain in history)
guarded       later operation applies (sequence-and-record: the later value is visible);
              a 0061 §6 conflict record opens for review          <- default
human_review  the conflicting operation is sequenced but HELD: the field keeps the incumbent
              value, a conflict record opens, and only an explicit resolution operation
              (typically elicitation, SURFACES.md §2) applies a chosen value
```

**Default assignments.** All §20 scalar fields default to `guarded` (title, reporter, assignee,
priority, severity, start/due dates, estimates, story_points, components, custom fields §20.1).
`lww` is reserved for fields where the later intent is trivially right and churn is high -
board/display cosmetics (column color, avatar, personal flags); no §20 core field is `lww`.
`human_review` applies to `security_level` and policy-labeled governance fields. Per-project policy
may promote or demote individual fields; the matrix is scope-level data, not code. Non-scalar
routing: `status` -> §7.3 revalidation; `description` -> body versioning (0061 §9); labels and
watchers -> set-merge (0061 §7.1, concurrent adds merge, removals sequence); `rank_token` -> order
tokens (0061 §5, conflict-free by construction); sprint/release membership -> §7.5.

**Conflict detection.** A write to field F carrying `base_entity_version = v` conflicts iff some
applied operation with sequence in `(v, now]` also wrote F. No intervening write to F means
disjoint merge - no record - regardless of how far the entity advanced. A write carrying no base
version is an *unconditional* write: it applies lww-style and never opens a record. Profile tools
always attach base versions; imports (§25) deliberately do not.

**Record lifecycle.** An explicit resolution operation (references `conflict_id`, sets the chosen
value) closes a record as `resolved`. Any later uncontested applied write to the same field
auto-closes open records on that field as `superseded` - the field moved on, the stale dispute is
moot. Held `human_review` records are exempt from auto-supersede: further conflicting writes hold
and attach to the same open record until an explicit resolution lands. Clients surface open records
in issue history, the Changes Inbox, and the bootstrap view (0061 §8).

### 7.3 Workflow Transitions (validation machine pinned 2026-07-04)

Status changes are workflow transitions, not plain field writes. A transition is valid only when:

- the current workflow version has an edge from the issue's current status to the target status;
- the edge's guards are satisfied;
- the project actor-enforcement policy allows the actor to perform the validated transition;
- automation side effects are idempotent.

Workflow edge validation and actor enforcement are separate checks. Projects choose actor
enforcement independently from the workflow graph: `write_access` permits any principal already
authorized for the workspace write; `assignee` requires the current assignee for transitions;
`review_authority` gates accepted and rejected transitions on project owner or acceptance authority;
and `ownership_governed` combines assignee gating for ordinary transitions with review-authority
gating for accepted and rejected. New projects default to `write_access`.

**Validation site: deterministic post-sequence.** The sequencer sequences transition operations
without reading content (blind-capable, 0061 §3). Validation is a pure deterministic function of
`(ordered log prefix, operation)`: every keyed replica computes the identical `applied | rejected`
outcome during projection. Keyed sequencer deployments may pre-validate at sequence time as a
fast-feedback optimization, but the projection outcome is the contract - blind and keyed topologies
agree exactly. Rejected operations remain in the log as audit facts. Validation ships as a
conformance-vectored pure function.

**Edge guards.** A workflow edge carries zero or more guards from a closed set of named kinds, plus
optional predicate trees:

```text
guard kinds:
  required_fields [field_id...]   fields must be set on the issue or attached to the operation
  permission role | principal-set actor must hold the role or grant
  linked_issue_state {edge_kind, all_in [status...]}  e.g. all child_of issues resolved
  checklist_gate gate_id          DoR/DoD structured checklist (§22); all criteria attested
  resolution_required             resolution must be set by this transition
  predicate <0061 §14 tree>       condition over issue fields, actor, transition context
```

All guard kinds are deterministic and blind-replica-computable; the policy enforcement point
inspects them as data. No executable code participates in validation - automation (§8) reacts to
transitions but never gates them. New guard kinds can be added later without contract break because
workflows are versioned data.

**Transition operation shape.** A transition operation records the target status, the source status
the client built against (audit), the workflow version validated against (audit), and optionally
attached field updates. Attached fields count toward `required_fields` guards and apply atomically
with the transition - the guard-elicitation flow in `SURFACES.md` §2 produces exactly this shape.
Current source exposes this shape as `target_status`, optional `observed_source_status`, optional
`observed_workflow_version`, and optional attached `set_fields` on the ticket update surface.
Native or projected `status` values inside `set_fields` are lifted into the same transition path
before field validation, so callers do not need a separate transition command and cannot bypass
workflow validation.

**Revalidation after sequence (retarget rule).** A transition means "take this issue to the target
status," not "traverse this exact edge." Validation evaluates against the sequenced state, never the
client's stale view: does the *current* workflow version contain an edge `current_status -> target`,
and do its guards pass, counting fields attached to the operation? The recorded source status and
workflow version are audit metadata, not pins - revalidation always uses the current workflow
version.

Example:

```text
Laptop A moves ISSUE-9 from In Progress to Done.
Laptop B moves ISSUE-9 from In Progress to Blocked.
The sequencer accepts both in sequence order.
A validates: edge In Progress -> Done exists, guards pass -> applied.
B revalidates against Done: applied only if the workflow permits Done -> Blocked and its guards
pass; otherwise rejected(edge_missing).
```

**Rejected-operation audit shape.** Rejection is derived, deterministic projection state on the
operation itself - no second operation is appended (in blind mode no central party could author
one). Schema:

```text
validation outcome (derived, deterministic):
  operation_id
  validation_state applied | rejected
  rule             edge_missing | guard_failed {guard_id} | field_missing [field_id...]
                   | permission | workflow_version_gone
  validated_against {status, workflow_version, root}
```

Rejection records surface in issue history (`tickets_history` returns them inline with applied
operations) and in the Changes Inbox (`SURFACES.md` §3). They are not 0061 §6 conflict records: a
conflict record means two *accepted* writes need review; a rejected transition was never applied.
Acting on a rejection - retrying with a different target or fields - is a new intent operation,
which is the correct audit trail.

### 7.4 Rank and Board Ordering

Board rank is a collaborative ordering problem. Jiraish must not use array positions as durable rank
identity. Use sparse rank tokens or another order-maintenance structure.

Rules:

- moving an issue assigns a new rank token between neighboring tokens;
- concurrent moves in the same gap receive deterministic tie-break tokens;
- periodic rank compaction rewrites rank tokens without changing issue identity;
- board views sort by `(rank_token, ticket_id)` for deterministic display.

**Board column modes (owner decision 2026-07-04, Asana import session).** A board is either
`status_mapped` - columns bind to workflow states or categories, card position derives from
status plus rank (the Jira model; the default) - or `manual` - columns are board-local ordered
groupings with **no status semantics**, and card placement is stored board state (the
Asana/Trello model). Manual placement is a per-(board, issue) guarded scalar
`(column_id, rank_token)` written by `board.card_moved`; the rank rules above apply per column in
both modes; WIP limits (§22) apply to both. Multi-homed issues (§4.1, §21) hold independent
placement per board. Manual boards are the model fix the Asana import surfaced: sections/columns
that are free groupings ("This week") must not pollute workflows with fake statuses, and they are
also what makes §22's personal Trello-style kanban honest.

### 7.5 Sprints, Releases, and Planning (conflict policy pinned 2026-07-04)

Sprint and release membership are metadata operations, routed through the machinery already pinned
in §7.2 and §7.3 - no planning-specific conflict mechanism exists.

**Sprint membership invariant.** An issue holds at most one membership in a sprint with state
`planned | active`. Membership is a set of facts; `sprint_ids` history falls out of closed
memberships plus `carried_from` edges (§22). Placing an issue into sprint B while it is in
non-closed sprint A is a move A->B: one operation, both effects; if A was started, the removal is a
recorded scope change (§22).

**Concurrent membership conflicts.** Current-sprint membership is a `guarded` scalar (§7.2):
concurrent different placements from the same base - later applies, conflict record opens. The
Sprint planner and Changes Inbox surfacing open records *is* the "conflict planning queue"; there
is no separate queue entity. Per-project policy may promote membership to `human_review`.

**Sprint-state races.** An operation referencing a sprint that closed before it sequenced rejects
with `sprint_closed` (extending the §7.3 rule enum). Adding to a started sprint is valid and
recorded as a scope change.

**Releases.** Release membership (fixVersions) is a set: concurrent adds merge, removals sequence
and are auditable (0061 §7.1 label semantics - adds are conflict-free by construction). Assigning
to a `released` release rejects with `release_closed` unless the actor holds an elevated grant
(backport bookkeeping); `archived` always rejects. Release state changes are operations;
`release.released` may carry a `checklist_gate` guard (§7.3) per project policy.

## 8. Workflows, Automation, and Agents

Workflows are versioned data. A transition operation records the workflow version it was validated
against as audit metadata; revalidation at sequence time always uses the current workflow version
(§7.3 retarget rule). If a concurrent workflow update removed the workflow, the operation is
rejected with `workflow_version_gone`; if it removed the edge, with `edge_missing`.

Every workflow state carries required metadata `category: todo | in_progress | done | accepted`
(Jira status categories plus Rally acceptance; §24.1.3, integration pinned 2026-07-04). `done` =
work finished; `accepted` = verified by the responsible owner. Board columns, cumulative flow
diagrams, and "open issues" queries (including the sprint-close disposition set, §22) are defined
over categories, never over status names; "complete" always means category ∈ {done, accepted},
while velocity counting is project policy (§21.2). Acceptance composes existing §7.3 guard kinds -
no new kinds: entry into an accepted-category state typically carries a `permission` guard (only
the responsible owner role transitions in) and/or a `checklist_gate` (acceptance criteria). A
workflow may declare `requires_acceptance`, making accepted the only valid terminal category
(checked at definition time, G2 dry-run reports violations). The category enum is ordered for
reporting only (todo < in_progress < done < accepted); workflow edges alone define reachability -
nothing requires passing through a done-category state to reach accepted. Jira-imported workflows
map resolutions onto done, never silently onto accepted (§24.1.3); Asana approval tasks map
`approved` onto accepted (§26).

Current source backs project-level active workflow storage and validation, with the default workflow
as version `v1` fallback. The default includes `accepted -> in_progress` and
`rejected -> in_progress` rework edges. Reopening appends a new `ticket.transitioned` operation and
does not rewrite or delete the earlier review result operation in immutable ticket history.

Automation must use the trigger and execution model:

- automation definitions are versioned Loom content;
- fires are recorded as durable operations;
- automation runs as a principal with scoped grants;
- every side effect is idempotent;
- content-aware automation requires a keyed local replica or keyed worker;
- blind Loom Cloud may wake automation runners but cannot evaluate plaintext guards.

Agents are first-class principals. They can triage, assign, comment, transition, link, create subtasks,
and propose sprint changes only within their grants.

Agent operations must identify:

```text
agent_id
model_or_runtime
operator_principal optional
source_issue_ids
tool_calls optional
confidence optional
policy_labels
trace_digest optional
```

## 9. Attachments, Links, and Comments

Attachments are content-addressed Loom objects referenced from issue operations. The attachment model
matches Driveish:

```text
attachment_id
digest
name
media_type
size
uploaded_by
created_at_ms
scan_status
retention_class
```

Issue links are graph edges:

```text
blocks
blocked_by
duplicates
duplicated_by
relates_to
parent_of
child_of
epic_of
implemented_by
```

Link operations are append-only facts with optional redaction or removal operations. The graph projection
is derived from the operation log.

Current source backs typed ticket-owned relations for `depends_on`, `blocks`, `parent_of`,
`child_of`, `relates_to`, `duplicates`, `supersedes`, `references_page`, `references_document`,
`has_prompt`, `has_result`, `has_decision`, and `assigned_to`. Ticket relation mutations validate
the target type implied by the relation kind, enforce singleton cardinality for parent, child,
duplicate, supersede, and assignment relations, and store the relation on the source ticket. The
Tickets service materializes each relation into the `ticket-relations` graph as a derived edge with a
deterministic id from source ticket, relation kind, target type, target id, and relation id.
Replacing or removing a relation removes the old derived edge. A bounded reconciler can rebuild the
derived graph from ticket-owned relation state after interruption.

Current source also backs the shared typed reference index used across Tickets, Pages, and Documents.
The source content remains owned by its native facet: ticket fields stay on tickets, page bodies and
block references stay in Pages, and document bytes stay in Documents. Explicit typed references such
as `!ticket:{ticket_id}` and source-declared references are indexed as `ReferenceEdge` records with a
`ReferenceSource`, target `EntityRef`, relation label, span, and evidence. The reference index
supports inbound and outbound lookup, while a deterministic `entity-references` graph projection
materializes the same typed edges for graph queries. That graph projection is derived from the
reference index and is reconciled from source-owned reference state; Graph is not the source of truth.
Lexical scanners for display keys such as `MX-60` enqueue unresolved candidates for confirmation or
resolver action, and do not silently create semantic dependencies.

Comments are append-only records with edit and redaction operations. Comment edits keep version history
unless retention policy permits hard deletion.

Current source backs native ticket collaboration primitives below the public promotion surface:
profile-owned comment records, shared content-addressed attachment metadata records, watcher-set rows,
sparse rank-token rows, operation history, revision indexing, and durable delivery notifications on the
ticket change stream. Comments, attachment additions, watcher changes, and rank moves use Tickets
service authorization, record a typed ticket operation, advance the revision index, and emit a delivery
message whose subject is `ticket:{ticket_id}` without changing the canonical Ticket record schema.
Attachment metadata stores digest, name, media type, size, uploader, creation time, and shared
visibility; attachment bytes remain in shared content storage. Rank display order remains
`(rank_token, ticket_id)` for deterministic views.

## 10. Sharing and Permissions

Jiraish permissions apply to organizations, projects, boards, issues, fields, comments, attachments,
workflow transitions, reports, and automations.

Grant scopes:

```text
viewer
commenter
reporter
developer
triager
project-admin
workflow-admin
automation-admin
agent-reader
agent-editor
```

Fine-grained policy must support:

- project-level browse and write rights;
- issue security levels;
- field edit restrictions;
- transition restrictions;
- board and sprint management rights;
- automation authoring rights;
- external share restrictions;
- agent-specific grants independent from installer grants.

Revocation prevents new reads and writes, but cannot erase content already synced to a device. Stronger
revocation requires per-project or per-issue encryption keys and key rotation.

## 11. Retention, Redaction, and Audit

Deleting an issue moves it to a deleted or archived state by default. It does not immediately remove the
issue history, comments, attachments, or audit facts.

Hard deletion is a policy operation:

- remove live references from issue and board projections;
- remove or expire search, graph, report, and vector indexes;
- retire content encryption keys when crypto-shredding is allowed;
- mark old roots outside the retention live set;
- let garbage collection reclaim unreachable objects after the retention window.

Legal hold overrides hard deletion. Issue retention, comment retention, attachment retention, audit
retention, and account deprovisioning are separate policy inputs.

## 12. Background Workers

Required workers:

- **Sync worker:** resumes transfers, verifies roots, applies operations, and backfills missing payloads.
- **Projection worker:** maintains issue tables, board views, rank indexes, workflow state, and graph
  links.
- **Search worker:** indexes full-text and semantic issue content where keys are available.
- **Report worker:** computes burndown, velocity, cycle time, SLA, and custom dashboards.
- **Automation worker:** runs trigger-bound workflow automation under scoped principals.
- **Attachment worker:** scans files, extracts text, computes previews, and expires unused uploads.
- **Retention worker:** applies archive, legal hold, key retirement, and deletion policy.
- **GC worker:** reclaims unreachable objects after policy permits it.
- **Notification worker:** converts tracker root advancement into MCP and WebSocket wakeups.

Keyless workers can operate only on labels, sizes, encrypted frames, and visible metadata. Content-aware
workers require keys and must run as auditable principals.

## 13. MCP as the Primary Protocol

Expose ticket state as MCP resources:

```text
loom://{workspace}/tickets
loom://{workspace}/tickets/project/{project_id}
loom://{workspace}/tickets/board/{board_id}
loom://{workspace}/tickets/{ticket_id}
loom://{workspace}/tickets/{ticket_id}/history
loom://{workspace}/tickets/sprint/{sprint_id}
loom://{workspace}/tickets/workflow/{workflow_id}
loom://{workspace}/tickets/filter/{filter_id}
```

Expose operations through MCP tools:

```text
tickets.search
tickets.get
tickets.create
tickets.update
tickets.transition
tickets.assign
tickets.comment
tickets.redact_comment
tickets.add_attachment
tickets.link
tickets.unlink
tickets.rank
tickets.set_sprint
tickets.create_board
tickets.update_workflow
tickets.create_filter
tickets.run_report
tickets.create_automation
tickets.update_cursor
```

All write tools execute as the resolved principal and are checked by the policy enforcement point.

## 14. Agent Callbacks and Subscriptions

Agents subscribe to projects, boards, filters, issues, sprint queues, assignment queues, or automation
fire logs.

Example subscription:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "resources/subscribe",
  "params": {
    "uri": "loom://organization/acme/tracker/main/board/backend"
  }
}
```

When the board projection changes, the MCP server emits a resource update notification. The agent then
fetches operations from its durable tracker cursor:

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/call",
  "params": {
    "name": "tickets.search",
    "arguments": {
      "filter_id": "backend-open",
      "after_sequence": 81720
    }
  }
}
```

The notification is a wakeup, not the source of truth. Durable delivery uses tracker sequence cursors.

## 15. Elicitation

MCP elicitation is used when an agent or server needs structured input before proceeding.

Use elicitation for:

- approving a workflow transition;
- resolving a field conflict;
- choosing an assignee;
- selecting a sprint or release;
- confirming creation of issues from a conversation;
- approving an agent-authored comment;
- deciding whether a failed automation should retry or stop;
- selecting whether to split, duplicate, or link related tickets.

Example:

```json
{
  "jsonrpc": "2.0",
  "id": 7,
  "method": "elicitation/create",
  "params": {
    "message": "Resolve concurrent priority changes for ISSUE-9.",
    "requestedSchema": {
      "type": "object",
      "properties": {
        "priority": {
          "type": "string",
          "enum": ["P0", "P1", "P2", "P3"]
        },
        "record_note": { "type": "boolean" }
      },
      "required": ["priority"]
    }
  }
}
```

Elicitation responses become durable operations when they affect tracker state.

## 16. WebSocket Secondary Transport

WebSocket can be offered for board UI fanout, drag progress, and high-frequency notifications. It must
preserve the same semantics as MCP:

- authenticated principal context;
- authorized project, board, issue, and filter subscriptions;
- durable tracker sequence cursors;
- idempotent writes;
- replay after reconnect;
- no stronger write authority than MCP tools.

## 17. Performance Requirements

The design must meet these requirements:

- creating or updating one issue does not rewrite the whole project;
- comments and audit records append without rewriting issue history;
- board rank moves are logarithmic or better in the rank index;
- listing a board is paginated and index-backed;
- filters and reports are projection-backed, not full-log scans;
- workflow transition validation is deterministic and auditable;
- sync transfers only missing objects and metadata nodes;
- local project views converge after replay;
- blind cloud mode remains usable for sequencing, coordination, and wakeup without content access.

## 18. Open Design Decisions

These choices must be pinned before implementation:

1. The canonical operation envelope and payload encoding. **Resolved: owned by 0061 §2** (§5 maps
   the profile into it).
2. The tracker operation sequencer protocol and replay guarantees. **Resolved: owned by 0061 §3**
   (§6.1 is its blind deployment mode).
3. The issue key allocator and project rename semantics. **Resolved 2026-07-04: §4.1**
   (Jira-compatible format, never-reused counters, retired-alias redirects, reserved prefixes,
   move semantics).
4. The rank token algorithm and compaction rules. **Resolved: algorithm owned by 0061 §5**;
   compaction cadence remains profile policy (§24).
5. The field conflict policy matrix. **Resolved 2026-07-04: §7.2** (lww/guarded/human_review
   classes, guarded default, field-level intervening-write detection, unconditional-write escape,
   resolved/superseded lifecycle).
6. The workflow validation and revalidation rules. **Resolved 2026-07-04: §7.3** (deterministic
   post-sequence validation, named guard kinds + §14 predicates, retarget rule, derived rejection
   state).
7. The schema for custom fields and typed field values. **Resolved 2026-07-04: §20.1** (closed
   type set + opaque_json escape, organization-global definitions with project contexts, option
   retirement, pinned widening list).
8. The SQL and graph projection canonical layouts.
9. The retention live-root algorithm for issue deletion and archive.
10. The conformance vector set for create, update, transition, rank, comment, link, automation, and cursor
    replay. **Partially resolved: envelope and cursor vectors owned by 0061 §16**; profile vectors
    (transition validation §7.3, conflict classes §7.2, allocator §4.1, close disposition §22,
    import fidelity §25) remain here.

## 19. Recommended Shape

All contracts in this document are long-term decisions; sequencing below is implementation staging
only and never changes a contract (no-v1 principle, `_QUEUE8.md` / ADOPTION §1.5). The recommended
shape is:

- local-first Loom replicas hold organization keys and can see the full tracker;
- Loom Cloud is a blind sync, operation sequencing, coordination, and notification replica by default;
- a separate keyed compute deployment is used when hosted search, reports, summaries, DLP, or workflow
  automation are required;
- issue state is an operation-log projection, not raw current branch sync;
- comments, audit, and attachments append by operation;
- scalar field conflicts are visible in issue history (§7.2 classes);
- workflow transitions validate as deterministic post-sequence projection state (§7.3);
- board rank uses sparse rank tokens, not array positions;
- MCP resources and subscriptions are the primary agent callback mechanism;
- WebSocket mirrors the same event and cursor contract for clients that need low-latency board updates;
- deletion is archive by default, with hard deletion handled by retention, key retirement, and GC.

## 20. Issue Field Model (decomposition)

What one ticket encapsulates, grouped by storage behavior. Rendering surfaces (`specs/studio/SURFACES.md`)
derive their field lists from this model.

```text
identity        ticket_id (stable UUID), ticket_key (0061 §4 alias), project_id,
                ticket_type epic | story | task | bug | spike | subtask
classification  title, description (body digest per 0061 §9), labels, components,
                custom fields (typed per §20.1)
people          reporter (assigner), assignee, watchers, participants (derived),
                agent principals (envelope identity block)
workflow        status (workflow state), resolution, priority, severity (bugs)
time            created_at, updated_at (derived from operations), start_date, due_date,
                original_estimate, remaining_estimate, time_spent (work-log annotations)
planning        parent (epic or subtask edge), sprint_ids, fix_release_ids, affects_release_ids
                (two release sets per §7.5; Jira fixVersions/affectsVersions), rank_token
                (0061 §5), story_points, roadmap_item_ref (§21)
relationships   links (§9 edge kinds), attachments, mentions
collaboration   comments and reactions (0061 §7 annotations), votes, watches
governance      security_level, policy_labels, audit trail (operations)
```

Storage behavior per group: identity is immutable; classification and people and workflow and time
and planning are guarded scalar operations (§7.2) except description, which versions as a body
(0061 §9); relationships and collaboration are append-only operations; governance is policy-owned.
`updated_at` and participants are projections, never stored fields.
Current source backs a native `TicketCoreFields` projection for the source-backed core fields:
title, description, normalized status, status category, assignee, reporter, priority, resolution,
labels, planning dates, estimates, story points, security level, and policy labels. The projection
reads existing field-map values into first-class typed accessors while retaining unpromoted field-map
entries as custom fields until the one-time clean-model migration lands. The normalized status set is
`backlog`, `planned`, `ready`, `in_progress`, `blocked`, `waiting_for_review`, `accepted`,
`rejected`, and `closed`; legacy `in_review` projects as `waiting_for_review`, and legacy
`review_status` is only a migration input.

### 20.1 Custom Field Schema and Typed Values (pinned 2026-07-04)

Custom field values are typed so that 0061 §14 predicate trees validate literals against them
deterministically; field validation, like transition validation (§7.3), is a deterministic
post-sequence function.

**Promotion (owner decision 2026-07-04, Notion/Asana session):** this field system is a substrate
shared facility (0061 §7.1). PAGES database structures (PAGES §21) consume the same
definitions, contexts, option semantics, widenings, and validation; this section remains the
defining text.

**Type system.** A closed set - no tenant-defined primitives:

```text
primitives   string, text (long-form; body-versioned per 0061 §9 when flagged),
             integer, number, boolean, date, datetime,
             date_range (start/end, date or datetime; added 2026-07-04 - Notion date
             properties carry optional ends and timelines need ranges; additive,
             non-breaking), duration,
             principal (user/agent ref), entity_ref(kind) (issue | page | file | component | ...,
             resolved per the cross-profile reference grammar, 0061 §16.6),
             enum(option_set), url
composites   list<T> for any primitive (multi-select = list<enum>, multi-user = list<principal>)
escape       opaque_json - schemaless payload; predicate support limited to exists;
             every field degraded to it is flagged in the import fidelity report (§25)
```

Types map 1:1 onto columnar projection columns and predicate literal types.

**Definitions: organization-global, per-project contexts.** A field definition is organization-scoped,
versioned data (same rule as workflows, §8):

```text
field definition:
  field_id        stable identity
  key             mutable alias (e.g. customer)
  type            from the set above; see mutability rule
  description
  option_set      for enum types; see evolution rule
  conflict_class  §7.2 class; default guarded
  constraints     range, regex, required-by-default
project context: available?, default value, option subset, required-on-create
```

Definition changes are operations (`field.created`, `field.updated`, `field.retired`) so the G2
authoring/dry-run flow applies. Cross-project reporting aggregates on `field_id`; two projects
sharing "Customer" share one field.

**Option-set evolution.** Options carry stable `option_id`; display labels are mutable (rename never
touches stored values). Removal is retirement, never deletion: existing values keep the retired
option (rendered as retired); new writes referencing a retired option are rejected with
`field_invalid` (extending the §7.3 rule enum to field validation); bulk migration off a retired
option is an explicit, attributable operation, never implicit.

**Type mutability.** Pinned safe widenings apply in place via one `field.updated` operation, values
untouched: `integer -> number`, `enum -> list<enum>`, `principal -> list<principal>`,
`string -> text`. Any other change follows create-new-field -> explicit bulk migration -> retire-old.

Current source backs the shared `loom-substrate::facilities` field-definition and field-value model,
including the closed type set, canonical field-value tags used by ticket issue fields, deterministic
value validation, JSON projection helpers, safe widening checks, and `loom-conformance`
`substrate-model` coverage. Ticket issue fields use this shared value model rather than a
ticket-private type. Field-definition CRUD operations, project-context CRUD operations, and
profile-specific UX remain target work.
The widening list is contract; it can grow without break, never shrink.
Current ticket source adds `TicketCustomFieldDefinition` as the ticket-specific schema layer over
shared substrate field definitions. It carries ticket length policy, searchable and orderable flags,
single/optional/list cardinality, typed default value validation, project applicability, and ticket
type applicability. Current source persists these definitions on the ticket project record and exposes
project custom-field management through CLI `loom tickets field-put`, CLI
`loom tickets field-retire`, MCP `tickets_field_put`, and MCP `tickets_field_retire`. The operations
reuse ticket profile expected-root handling and operation history; retire marks the definition as
inactive rather than deleting historical schema. Local ticket create/update validation rejects
unknown project-local fields, retired fields, inapplicable fields, invalid field-value types,
cardinality violations, field-specific length violations, and missing required applicable fields.
Hosted REST/JSON-RPC field-definition surfaces remain queued work.
Ticket source now applies ticket-specific text policy instead of inheriting the substrate compact
text cap everywhere: compact scalar fields remain capped at 4,096 bytes, while description and comment
bodies validate as rich text bodies up to the ticket rich-text cap. Comment records carry a body
content type and default to `text/markdown`. Custom field definitions own their text length policy
through `max_length`; text defaults validate against the field definition's limit rather than the
substrate compact-text limit.
Substrate Date and DateTime field values are native temporal values rather than text-shaped storage:
Date stores canonical days since the Unix epoch and DateTime stores UTC Unix milliseconds. JSON
projection emits ISO-8601 strings; DateTime input requires a UTC offset. Ticket start and due date
fields use these native values and project through `TicketCoreFields` as ISO-8601 strings for current
callers.

### 20.2 Ticket Projection Profiles

Current ticket source defines explicit projection contracts for `native`, `jira`, `asana`, `notion`,
and `redmine`. Every projection contract reads from the canonical ticket source and returns a tagged
response kind; projection selection must not silently mutate the native ticket schema.
Current source also backs project projection configuration: each ticket project carries a
`default_display_projection`, an enabled projection set, and per-projection configuration. Native
projection remains mandatory and cannot be disabled. Explicit caller requests override the project
display default only when the requested projection is enabled. Human display contexts use the project
default when no projection is requested; machine/API contexts use native when no projection is
requested, so MCP/CLI/API response shapes do not silently adapt to project display preferences.
Current source exposes the project configuration through one project settings surface. CLI
`loom tickets project-settings-get` / `loom tickets project-settings-set` and MCP
`tickets_project_settings_get` / `tickets_project_settings_set` read and update the project display
projection, lifecycle actor-enforcement policy, project owner, and acceptance authorities as one
project-settings contract. Hosted REST exposes `/tickets:project-settings-get`
and `/tickets:project-settings-set`; hosted JSON-RPC exposes `tickets.project_settings_get` /
`tickets.projectSettingsGet` and `tickets.project_settings_set` / `tickets.projectSettingsSet`.
The earlier project-policy read/set CLI and MCP surfaces are not public; lifecycle actor policy is a
project setting.
The current public read surfaces expose this contract with an explicit optional projection parameter:
CLI `loom tickets list --projection` and `loom tickets get --projection`, MCP `tickets_get`
`projection`, hosted REST `/tickets:get` `projection`, and hosted JSON-RPC `tickets.get`
`projection`. Ticket responses use a public `projection` discriminator and never serialize internal
selection metadata such as projection source, selection source, or response-kind tags. Native reads
return the native Loom ticket envelope; Jira reads return `{projection,id,key,fields}` with Jira
field names; Asana reads return `{projection,gid,data}`; Notion reads return
`{projection,id,properties}`; Redmine reads return `{projection,issue}`. Field aliasing is applied
only for explicitly configured projected profiles; without an explicit machine/API projection
request, responses stay native. Built-in projection configs include title-field aliases for Jira,
Asana, Notion, and Redmine so public-surface projection tests prove non-native field keys without
requiring a separate project-configuration write surface. The C ABI, IDL, and language bindings
currently have no ticket read or write surface, so ticket projection parity is not promoted there
until tickets receive an ABI/remote contract.

Current source also exposes projection-aware field discovery for machine writers. CLI
`loom tickets fields <store> <workspace> --projection <native|jira|asana|notion|redmine>
--operation <create|update|write>` and MCP `tickets_fields` return the canonical core field catalog:
public `projection`, native field id, projection write path, aliases, field type, cardinality,
whether the field is settable, create/update required flags, search/orderability flags, max byte
length, enum/status values, and write semantics. Jira discovery exposes `title` as
`fields.summary` and `status` as `transition.to.name`; Asana exposes title as `data.name`; Notion
exposes title as `properties.Name.title`; Redmine exposes title as `issue.subject`. The catalog is
source-backed by `loom-tickets` rather than duplicated in CLI or MCP.

When `project_id` is supplied, CLI `loom tickets fields` and MCP `tickets_fields` include active
project custom fields in the catalog. The project field catalog reports `strict_unknown_fields: true`
because local ticket create/update validation now requires project-local fields to be declared before
write. Existing work-graph projects that already carry ad hoc fields must register those fields as
project custom-field definitions, or import them through an explicit import field-policy infer path,
before strict writers can continue updating those project-local fields.

Current ticket import source backs only two unknown-field policies: `strict` and `infer`.
Direct Jira, Asana, and Redmine CLI imports default to `strict` through
`loom interchange import-jira`, `loom interchange import-asana`, and
`loom interchange import-redmine`; callers opt into inference with `--field-policy infer`.
MCP `redmine_import_snapshot` defaults to `strict` and accepts `field_policy: "infer"` for the same
behavior. Strict import rejects undeclared retained-source fields with an actionable field list and
guidance to rerun with infer. Infer creates project-wide custom-field definitions from the normalized
source values before ticket creation, using native scalar field types where possible and opaque JSON
for retained object payloads. Normal ticket create/update paths remain strict and do not implicitly
create fields. Normalized 0012 import execution batches use inference for ticket-profile execution
because the batch is already trusted normalized import metadata.

Current source also backs projected input normalization for public create and update surfaces. CLI
`loom tickets create --projection`, CLI update request JSON `projection`, MCP `tickets_create`
`projection`, MCP `tickets_update` `projection`, hosted REST `tickets:create`/`tickets:update`
`projection`, and hosted JSON-RPC `tickets.create`/`tickets.update` `projection` all normalize
explicit projected input envelopes into native ticket fields before validation and storage. Jira
normalizes `fields.summary`, `summary`, simple `update.*.set` operations, and transition objects
whose target status is expressed as `id`, `name`, `value`, `status`, or `to`; Asana normalizes
`data.name`, `data.notes`, `data.status`, and `data.completed=true` to `accepted`; Notion normalizes
`properties.Name.title`, rich-text properties, and status properties; Redmine normalizes
`issue.subject`, `issue.description`, and string-shaped `status_id`/`status`. Storage remains native.
External numeric transition or status identifiers require a project settings mapping before they can
be treated as workflow states.

**Imported computed fields (owner decision 2026-07-04).** Source-system computed fields - Asana
`formula`/`custom_id` representation types, Notion `formula`/`rollup` properties - import as
normal typed fields whose definition carries `imported_computed: true`: the values are the
source's last computed values, **frozen**. The flag makes the field read-only by policy (writes
reject with `field_invalid` until an admin operation explicitly clears the flag), and the source
expression text is retained in the fidelity report and CAS for later re-authoring (G2 flow).
Nothing recomputes silently; the data stays typed and queryable.

### 20.2 Body Content Types (pinned 2026-07-04)

Every body revision (description, `text`-typed custom fields; 0061 §9) carries a `content_type`.
Bodies are stored in their native format, never converted at write time: an assistant writes
`text/markdown`; a Jira import stores ADF (`application/vnd.atlassian.adf+json`) verbatim; future
types (the PAGES canonical body model, HTML, plain text) register the same way.

Per-content-type registries provide: a **renderer** (display), a **text extractor** (search
indexing, 0061 §10), and a **reference extractor** (mention edges per the cross-profile grammar,
0061 §16.6). A type without an extractor degrades gracefully: it renders and diffs opaquely and is
flagged in search-index coverage, but is never rejected or mutated.

Conversion between content types is an explicit operation producing a new revision (new
`content_type`, new digest) with the prior revision retained - lossy conversions are therefore
attributable and reversible. The PAGES body-model decision picks the canonical
*collaborative-editing* type; it does not retroactively convert stored bodies.

## 21. Projects, Roadmaps, and Plan Semantics

Issues live in projects; projects roll up into plans. The planning layer is the same
operation-log-plus-projections pattern, with the graph and calendar facets doing the heavy lifting.

```text
project         project_id, key_prefix, name, lead, members (grants), workflow refs,
                boards, components, releases
roadmap         roadmap_id, scope (one project or cross-project), swimlane scheme (team | theme)
roadmap item    item_id, title, kind initiative | portfolio_ref(level) (generalizes epic_ref, §21.1),
                owner, status, confidence,
                timeframe (start/target dates, or bucket now | next | later),
                progress_rollup rule: issue_count | story_points | manual
milestone       milestone_id, date, title, criteria (issue set or view), health
dependency      edge item -> item or issue -> issue, type FS | SS | FF | SF, lag days
```

Facet mapping: items and milestones are columnar rows plus graph nodes; dependencies are typed graph
edges; timeframes and milestones project into the calendar facet so date-bearing planning artifacts
appear beside real calendars; progress is a view (0061 §8) over the issue table, never a stored
number when a rollup rule exists.

**Multi-project membership (owner decision 2026-07-04, Asana import session).** An issue is owned
by exactly one project (key, workflow, permissions - §4.1) and may be *homed* into others via
typed `member_of` membership edges: `(ticket_id, host_project_id)` written by
`issue.homed`/`issue.unhomed` operations, carrying per-board placement in the host project's
boards (§7.4 manual placement) and optionally a secondary key (§4.1). Membership affects
projection surfaces - the issue appears on the host's boards, backlogs, and queries, flagged as a
guest - but never moves ownership: validation runs the owning project's workflow, and access
control remains the owning scope's; a host-project viewer without grants covering the issue sees
nothing. This is the edges-over-containers model Asana validates: one task, many
project+placement memberships.

Gantt semantics are projections, not stored state: the schedule view computes earliest starts from
dependency edges, the critical path is the longest path over the dependency graph, and baseline
versus current compares two `as_of` roots (0061 §9). Rescheduling an item with dependents is a
guarded operation: the sequencer applies the profile policy (cascade, or elicit confirmation per
`SURFACES.md` §2, or reject).

Roadmap health (owner decision 2026-07-04): **both, with visible divergence.** Health is computed
by default (schedule/dependency/progress projections); an explicit assertion operation overrides
it; when assertion and computation disagree, surfaces display both ("owner says green, projection
says red") rather than hiding either. Capacity is resolved in §21.2 (Rally adoption §24.1.4).
Remaining open item for this section: cross-project dependency permissions.

### 21.1 Portfolio Hierarchy Taxonomy (pinned 2026-07-04)

Realizes §24.1.1. **Portfolio items are tickets** (owner decision): each taxonomy level binds to a
ticket type, so portfolio items get fields, workflows, annotations, attachments, watches, history,
permissions, and import machinery for free. There is no separate portfolio-item entity kind.

```text
hierarchy taxonomy   organization-scoped, versioned data (same rule as workflows §8 and
                     field definitions §20.1); one active taxonomy per organization
  taxonomy_id        stable identity
  levels[]           ordered, depth 1..N (1 = lowest; level-1 items parent work items)
    level_id         stable
    name             mutable display (Epic, Feature, Initiative, Theme)
    ticket_type      the ticket type bound to this level (binding is organization-unique)
    rollup           progress rule over direct children:
                     issue_count | story_points | manual | weighted(child field)
    timeframed       whether items at this level carry roadmap timeframes (§21)
```

Rules:

- **Strict adjacency (owner decision 2026-07-04; matches Rally and Jira Advanced Roadmaps).**
  A `parent_of` edge from a level-n item may target only level-(n-1) items; level-1 items parent
  work items (story | task | bug | spike). Skip-level parents are invalid. Parentless items are
  allowed at every level. Work-item nesting is unchanged: stories nest freely among stories
  (§24.1.2); the single-level subtask limit is Jira-import policy, not a model constraint.
- **Validation** is deterministic post-sequence validation (§7.3): a parent edge violating the
  active taxonomy rejects with `hierarchy_violation` (extending the §7.3 rule enum - extend, never
  fork, per the shared-enum rule).
- **Evolution.** Taxonomy changes are operations (`taxonomy.updated`), versioned like workflows;
  the G2 dry-run flow applies. Levels retire like enum options (§20.1): a retired level keeps
  existing items and edges valid but blocks new items of its type. Inserting a level grandfathers
  existing edges that now skip it (flagged in the dry-run report); new edges validate against the
  current taxonomy - same philosophy as §7.3 retarget revalidation.
- **Rollups are projections** (0061 §8) evaluated per level over direct children (strict adjacency
  makes "direct children" well-defined); descendant-closure reporting is a view over the same
  edges, never stored.
- **Degenerate taxonomy (Jira compatibility).** The Jira import instantiates the one-level
  taxonomy: `[{name: Epic, ticket_type: epic, rollup: per-epic rule, timeframed: true}]`. Existing
  `epic_of`/`parent_of` edges conform unchanged; §22's epic semantics are the level-1 instance.
- **Rally mapping.** Rally portfolio item types import as levels in hierarchy order (typically
  Theme -> Initiative -> Feature); each type's schedule states become the level's issue-type
  workflow; Feature->story attachment becomes level-1 `parent_of` edges; preliminary estimates
  become §20.1 custom fields.
- **Roadmap reconciliation.** Roadmap item kind `epic_ref` generalizes to `portfolio_ref(level)`:
  a roadmap item is the scheduling/rollup view over a portfolio issue, and its progress follows
  the level's rollup rule. The standalone `initiative` kind is retained only for imported items
  with no issue backing (Asana portfolios and goals contain projects, not issues - §26).

### 21.2 Capacity and Load (pinned 2026-07-04; Rally model per §24.1.4)

```text
capacity record      per (sprint_id, principal); a §7.2 guarded scalar
  unit               hours | points - project policy capacity_unit, default hours
  capacity           number; default = member organization default × availability_pct
  availability_pct   optional (vacations, part-time allocation)
```

- Operations: `sprint.capacity_set` / `sprint.capacity_cleared`; auditable, conflict-handled like
  any guarded scalar. Capacity is data, never derived.
- **Plan estimate vs task remaining (the Rally split).** `story_points` (§20 planning) is the plan
  estimate: it drives velocity and planning-time load. `remaining_estimate` hours (§20 time) is
  task-level remaining work: it drives in-sprint burndown. The two are never mixed in one chart.
- **Load projection** (0061 §8 view, never stored): per member per sprint, load = Σ of
  `remaining_estimate` (or `story_points` when `capacity_unit: points`) over assigned issues and
  subtasks with membership in that sprint and category ∉ {done, accepted}; surfaced as
  load-vs-capacity with unassigned load shown separately.
- **Velocity counting (owner decision 2026-07-04): per-project policy**
  `velocity_counts: done | accepted`, default `done` (Jira-compatible). Rally import sets
  `accepted`. Changing the policy is an audited project operation; charts label which policy
  produced each data point. CFD, open-issue queries, and the sprint-close open set always use
  complete = {done, accepted} regardless of this policy (§8).

## 22. Agile Semantics

Jiraish is not just a ticket table. It must support the working-method spectrum from a personal
Trello-like kanban (a board, columns, cards, nothing else) up to full Scrum, with each concept
opt-in and none of them mandatory. The method concepts, their data models, and their mapping to the
layers discussed elsewhere:

```text
backlog       the rank-ordered issue set with no sprint membership; pure order tokens (0061 §5)
sprint        sprint_id, project_id, name, goal, state planned | active | closed,
              start/end dates (project into calendar facet), capacity (per-member
              records, §21.2), committed scope (issue set at start), scope-change
              operations after start
epic          long-lived issue container: epic_id (an issue of type epic), theme, color,
              progress rollup rule, target timeframe (roadmap item ref, §21)
theme         cross-project label taxonomy over epics; swimlane and reporting dimension
wip limit     board-column policy: max issues per column per assignee or team;
              violations are visible board state, enforcement policy soft | hard
definition of ready / done   checklist gates attached to workflow edges; DoR guards
              entry into a sprint, DoD guards entry into done-category states;
              acceptance gates (§8) guard entry into accepted-category states; gate
              items are structured (boolean per criterion) so elicitation can collect
              them
velocity      derived: points completed per sprint (velocity_counts policy, §21.2)
              over trailing N sprints; never stored
ceremonies    planning -> Sprint planner app; standup -> Changes Inbox scoped to sprint
              and actor; review -> sprint Report view; retro -> annotations on a
              sprint-scoped page (PAGES), linked back by refs
```

Semantics that must be operations, not conventions:

- **Sprint scope change is an operation.** Adding or removing an issue after `sprint.started` is a
  sequenced, attributable fact - this is what makes honest burndown charts and scope-change
  annotations on burnups possible.
- **Sprint close disposition is explicit** (mechanics pinned 2026-07-04). The close flow computes
  open issues, proposes a project-configurable default disposition (`backlog` or `next_sprint`),
  and elicits only per-issue overrides (done-with-resolution collects the resolution per §7.3
  guards). Close is a **single atomic `sprint.closed` operation embedding the full disposition
  list** - one audit fact, replayable; the projection applies memberships, `carried_from` edges,
  and resolution transitions. Each transition inside the disposition validates per §7.3
  individually: a failed guard surfaces as that issue's own rejection record and the issue falls
  back to the default disposition (recorded) - one bad guard cannot wedge or un-close the sprint.
  Carried-over issues keep a `carried_from` edge so carry-over rate is queryable.
- **Epic progress is a projection** over descendant issues by the epic's rollup rule; epics never
  store progress numbers.
- **Kanban mode is the degenerate case:** a board over the backlog with WIP limits and no sprint
  entities. Nothing in the issue model changes between modes; method concepts attach to boards and
  projects, not to tickets.

Visualizations this unlocks (rendered per `SURFACES.md`): burndown/burnup with scope-change pins,
velocity with confidence bands, cumulative flow diagrams from transition operations, carry-over
Sankey between sprints, epic progress bars on the roadmap, WIP-limit heat on board columns.

## 23. Example Tool Surface (illustrative only - not a design decision)

This list exists to make the intended assistant ergonomics concrete. Names, grouping, and parameters
are examples for discussion, not designed contracts. Underscore-flattened per MCP; capability
`tickets`.

| Category | Tool | Description |
| --- | --- | --- |
| Projects | `projects.create` | Create a project with key prefix, lead, workflow refs |
| Projects | `tickets_project_create` | Current source-backed MCP vertical for creating a durable profile project |
| Projects | `tickets_project_rekey` | Current source-backed O(1) active-prefix route change; existing ticket UUIDs and numbers remain unchanged |
| Projects | `projects.get` | Fetch one project |
| Projects | `projects.list` | List projects visible to the principal |
| Projects | `projects.update` | Update project metadata and configuration |
| Tickets | `tickets_create` | Create a ticket; Loom assigns its UUID and returns its derived key (`SURFACES.md` §3a) |
| Tickets | `tickets_get` | Fetch one ticket by UUID or derived key, plus derived fields |
| Tickets | `tickets_delete` | Current source-backed audited tombstone delete; sets `status=closed`, `status_category=done`, `resolution=deleted`, `deleted_at`, and `deleted_by` while retaining ticket identity, history, aliases, and relation history. |
| Issues | `tickets_update` | Apply one optional scalar detail patch and one optional lifecycle action atomically; direct `status` and `assignee` patches are rejected so lifecycle policy remains enforceable. |
| Issues | `tickets_relation_set` | Current source-backed typed relation set/replace; relation source state remains on the ticket and projects to `ticket-relations` |
| Issues | `tickets_relation_remove` | Current source-backed typed relation removal by source ticket and relation id; removes the derived graph edge |
| Issues | `tickets.transition` | Workflow transition; guard-elicitation for missing required fields (§7.3) |
| Issues | `tickets.assign` | Change assignee |
| Issues | `tickets.comment` | Append a comment annotation (0061 §7) |
| Issues | `tickets.redact_comment` | Redact a comment; audit fact persists |
| Issues | `tickets.link` | Add a typed link edge (§9 kinds) |
| Issues | `tickets.unlink` | Remove a link by append-only removal operation |
| Issues | `tickets.rank` | Move an issue in board/backlog order (0061 §5 tokens) |
| Issues | `tickets.watch` | Add the principal as a watcher |
| Issues | `tickets.search` | Domain search over issues; scoped, filterable |
| Issues | `tickets_history` | Revision/operation history for an issue (0061 §9); current MCP vertical returns operation-envelope history |
| Issues | `tickets.add_attachment` | Attach a content-addressed file |
| Sprints | `sprints.create` | Create a sprint with goal, dates, capacity (§22) |
| Sprints | `sprints.plan` | Add/remove sprint scope; scope changes after start are recorded operations |
| Sprints | `sprints.start` | Activate; snapshots committed scope for burndown |
| Sprints | `sprints.close` | Close; elicits disposition per open issue; writes `carried_from` edges |
| Sprints | `sprints.get` | Fetch sprint state and scope |
| Sprints | `sprints.report` | Burndown/velocity/carry-over report projection |
| Epics | `epics.create` | Create an epic (issue of type epic) with theme and rollup rule |
| Epics | `epics.get` | Fetch epic with computed progress rollup |
| Epics | `epics.list` | List epics by project/theme |
| Epics | `epics.rollup` | Recompute/fetch descendant rollup for an epic |
| Backlog | `backlog.list` | Rank-ordered issues with no sprint membership |
| Backlog | `backlog.reorder` | Reorder backlog entries |
| Boards | `boards.create` | Create a first-class Ticket Board over a project, saved filter, or manual set |
| Boards | `boards.get` | Fetch first-class Board projection: columns, cards, WIP state, mode, scope, and ordering |
| Boards | `boards.configure` | Columns, WIP limits, swimlanes, display fields, mode, and scope (§22) |
| Boards | `boards.card_moved` | Move or reorder one card; status-mapped Boards route status changes through workflow validation, manual Boards write board-local placement only |
| Workflows | `workflows.create` | Create a versioned workflow definition |
| Workflows | `workflows.update` | New workflow version; in-flight issues revalidate per policy |
| Workflows | `workflows.validate` | Dry-run a workflow version against existing issues |
| Planning | `roadmap_items.create` | Create initiative/epic-ref roadmap item (§21) |
| Planning | `roadmap_items.update` | Timeframe/status/confidence updates; cascade policy on reschedule |
| Planning | `roadmap_items.depend` | Add typed dependency edge (FS/SS/FF/SF + lag) |
| Planning | `milestones.create` | Create a milestone with date and criteria |
| Planning | `milestones.update` | Update milestone; projects into calendar facet |
| Planning | `releases.create` | Create a release/fixVersion |
| Planning | `releases.assign` | Assign issues to a release |
| Queries | `filters.create` | Save a named filter (predicate tree per 0061 §14) |
| Queries | `reports.run` | Run a report projection (velocity, cycle time, CFD) |
| Automation | `automations.create` | Create trigger-bound automation (§8) |
| Automation | `automations.update` | New automation version |
| Cursors | `cursors.update` | Advance the principal's durable cursor |

## 24. Unfinished Tasks (pushed back from Queue 8)

`specs/0061.md` now owns the shared substrate: operation envelope, sequencer protocol, cursors,
order-token library, conflict record schema, annotation subsystem, view/projection machinery, and
cross-facet search. Open decisions 1, 2, 4 (token algorithm), 5 (conflict schema), and 10 (envelope
and cursor vectors) in §18 resolve there. The following remains uniquely Jiraish and is unowned by
any queue:

- Current source backs ticket creation/get primitives, typed field storage, policy labels, canonical
  key-prefix validation, monotonic key allocation, key lookup, canonical CBOR round-trips, scalar
  conflict outcomes, required-field guards, permission guards, retargeted transition validation, and
  missing-workflow rejection records in `loom-substrate`. Current source also backs board definitions,
  sprint state and membership, explicit sprint close disposition, carried-from edge reporting, and
  portfolio taxonomy strict-adjacency validation. Capacity records, load calculation, progress
  rollups, roadmap item validation, and dependency validation are source-backed. Store projection
  bridge uses indexed tables for projects, prefix routes, tickets, project-number lookups, external
  source identities, and operations. Its compact
  `profile/tickets/v2/{workspace}/state` control record pins table roots and the next operation
  sequence; each operation still records `root_after`. The top-level `loom tickets` CLI now
  source-backs project create, project re-key, ticket create, field update,
  list, get, and operation history over the resolved workspace id. MCP now has a source-backed
  durable vertical for project create/rekey, project policy get/set, project
  settings get/set, field discovery/put/retire, ticket create/update/delete/get/history, relation
  set/remove, and `substrate_changes` operation replay through
  `oplog:<next-sequence>:tickets:<workspace-id>` cursors. Hosted REST and JSON-RPC now source-back
  the same promoted ticket project and ticket vertical for project create, project re-key,
  project settings get/set, ticket create, atomic ticket update, audited ticket delete, ticket get,
  and ticket history through the served `tickets` data surface. The
  hosted listener collection is the ticket workspace id.
  `tickets_project_create`, `tickets_project_settings_set`, `tickets_create`, `tickets_update`, and
  `tickets_delete` across CLI, MCP, hosted REST, and hosted JSON-RPC accept optional
  `expected_root`, reject stale roots with `CONFLICT`, and return `profile_root` after publishing.
  The `loom-tickets` service stores a project-level active workflow definition; the public
  workflow-management tool surface remains target work. The project lifecycle actor policy is
  source-backed as project settings; normal projects default to `write_access`, while stricter
  assignee, review-authority, and ownership-governed policies are configurable project state.
  `tickets_update` accepts one structured request with optional `set_fields`, optional `delete_fields`,
  optional `target_status`, optional observed transition audit fields, optional compatibility
  lifecycle `action`, optional `assignee` ownership update, and `expected_root`. It validates
  and records the combined mutation as one operation. `tickets_create` assigns a UUID `ticket_id`; optional `(external_source, external_id)` is unique
  within the ticket workspace. Derived `PREFIX-N` keys and external identities resolve to
  `ticket:{ticket_id}`. `tickets_delete` is a soft-delete/tombstone operation rather than a hard row
  removal; it records `ticket.deleted`, sets closed/deleted fields, rejects stale `expected_root`, and
  rejects duplicate delete attempts with `CONFLICT`. Ticket create, update, and delete operations maintain the reserved revision index for
  `ticket:{ticket_id}` history. MCP and hosted ticket writes share the `loom-tickets` reference helper
  for text-bearing ticket-field backlinks and unresolved ticket-key candidates with source facet
  `tickets`. The shared reference helper now projects ticket, page, and document references into the
  derived `entity-references` graph while preserving the source facet as the owner of the reference
  declaration. These operations are guarded through the workspace VCS
  policy path until a first-class ticket facet ACL lands. Project re-key is source-backed through
  prefix routes, derived alias resolution, and durable audit records; retired-prefix release is not a
  public surface.
  Planning UI projections, imports, CLI/MCP/hosted parity for the broader ticket/agile tool set,
  IDL/C ABI/language binding promotion, and conformance vectors remain queued work where those
  surfaces do not yet have a ticket contract.
- ~~The workflow transition validation machine (§7.3)~~ - pinned 2026-07-04 in §7.3: deterministic
  post-sequence validation, named guard kinds plus 0061 §14 predicates, retarget revalidation,
  derived rejection state. Remaining: conformance vectors for the validation function.
- ~~The field conflict policy matrix (§7.2)~~ - pinned 2026-07-04 in §7.2. Remaining: conformance
  vectors for the three classes and the auto-supersede rule.
- ~~The custom field schema and typed field values (§18.7)~~ - pinned 2026-07-04 in §20.1.
  Remaining: conformance vectors for typed-literal validation and the widening list.
- ~~The issue-key alias format and project rename semantics (§18.3)~~ - pinned 2026-07-04 in §4.1.
  Remaining: allocator conformance vectors (collision, re-key, move).
- ~~Sprint/release conflict policy (§7.5)~~ - pinned 2026-07-04 in §7.5 and §22 (single-active
  membership invariant, guarded class, sprint_closed/release_closed rules, atomic close
  disposition). Board projection canonical layouts (§18.8) remain open.
- Rank compaction cadence as profile policy over the 0061 §5 order-token library.
- ~~Import (requirement, ADOPTION §1.3)~~ - mapping table, degrade paths, fidelity report, and
  coexistence semantics pinned 2026-07-04 in §25. The stress test surfaced and fixed three model
  gaps: two release sets (§20), workflow status categories (§8), body content types (§20.2).
  Remaining: importer implementation over 0012 interchange; JQL->predicate converter coverage.
- **Structured intake (2026-07-04, from the Asana import):** Asana forms (and Notion form views)
  have no Jiraish target - a public/guest intake surface that creates issues from a field schema
  is a recorded product gap, out of scope for import (neither source exports form definitions
  anyway). If designed later, it composes §20.1 field definitions + elicitation (SURFACES.md).
- **Workflow and schema authoring experience (ADOPTION G2):** the authoring app flow,
  `workflows.validate` dry-run semantics (stranded-issue and referencing-automation report),
  in-flight migration policy on workflow redefinition, and elicitation-based guard/gate editing.

- **Rally/Redmine model considerations (2026-07-04, unowned; see ADOPTION §1.3 matrix):**
  (1) configurable portfolio-hierarchy levels as organization data (Rally Theme->Initiative->Feature
  versus the hardcoded epic type; graph already supports it, the level taxonomy does not exist);
  (2) nested-story policy - `parent_of` permits arbitrary depth, Jira-inherited single-level
  subtask limit is currently implicit, decide explicitly; (3) an `accepted` status category
  distinct from `done` (Rally schedule states independently validate the §8 category enum and add
  acceptance); (4) Rally's per-member iteration capacity model as the concrete candidate for the
  §21 open capacity item (plan-estimate points vs task-hours split; velocity from points, burndown
  from hours); (5) test-case/test-set traceability (Rally Quality) has no JIRAISH target - future
  extension or profile; (6) Rally and Redmine import mapping tables (Redmine journals map near-1:1
  onto operation-log history synthesis; its precedes/follows-with-delay relations validate §21
  dependency lag).
- **Redmine source-backed slice (2026-07-12):** `loom interchange import-redmine` delegates to the
  reusable Redmine importer in `loom-interchange-io`, imports normalized Redmine snapshot JSON or
  Redmine XML for projects, issues, and wiki pages, lowers projects/issues through the ticket
  profile service, lowers wiki pages through the Pages profile service, preserves Redmine issues as
  external identities (`redmine`, `issue:<id>`), retains journals, comments, attachments, time
  entries, watchers, affected-version lists, and relations as structured `redmine_*` ticket fields,
  imports category, fixed-version, affected-version, and closed timestamp fields, supports MCP
  `redmine_import_snapshot`, and skips duplicates or unchanged pages idempotently. The checked-in
  broad Redmine API-shaped fixture in `specs/studio/fixtures/redmine/` imports into a clean store
  and compares project, issue, retained source-extra, relation-variant, watcher, and wiki-page
  output against an expected comparison report. Full identity mapping, native ticket-profile
  projection of retained source extras, Pages wiki revision metadata, and broader
  execution-fidelity vectors remain target work.

## 24.1 Rally Adoptions (owner decision 2026-07-04 - adopted, schema detail in the Notion/Asana session)

The §24 Rally considerations are adopted, not merely considered. Schema designed 2026-07-04
(prompt-5 session): item 1 -> §21.1 (portfolio taxonomy; portfolio items are issues, strict
adjacency - owner decisions), item 3 -> §8 (category enum + acceptance guards), item 4 -> §21.2
(capacity records, plan-estimate/task-remaining split, velocity_counts policy - owner decision).
Items 2 and 5 unchanged as stated below. Original pinned intent:

1. **Configurable portfolio hierarchy.** Organization-defined typed levels (e.g. Theme -> Initiative ->
   Feature) above story/task/defect, as data: a level taxonomy per organization, rollups defined per
   level, `parent_of` edges constrained by the taxonomy. The hardcoded epic type becomes the
   degenerate one-level taxonomy (Jira-import compatibility).
2. **Nested stories.** `parent_of` depth is unlimited for stories; the single-level subtask limit
   is Jira-import policy, not a model constraint.
3. **Schedule states.** The §8 status-category enum gains `accepted`: todo | in_progress | done |
   accepted. `done` = work finished; `accepted` = verified by the responsible owner. Terminal
   transitions may require acceptance per workflow policy; Jira-imported workflows map their
   resolutions onto done (never silently onto accepted). Asana approval tasks map onto accepted.
4. **Capacity.** Rally's model adopted for the §21 open item: per-member capacity per
   iteration (hours or points), plan-estimate (points, drives velocity) split from task-remaining
   (hours, drives in-sprint burndown); load-vs-capacity is a projection.
5. **Test traceability** (Rally Quality) is committed as planned future work, not designed here:
   test case/test set entity kinds with typed edges to stories/defects.


## 25. Import from Jira (pinned 2026-07-04)

Import is a requirement (ADOPTION §1.3), runs through the 0012 interchange layer, and doubles as
the schema stress test: where real Jira data did not fit §20-§22, the model was fixed, not the
importer (release sets, status categories, body content types).

Current source backs the Jira import planning contract in `loom-interchange` and the first
normalized execution slice through the reusable Jira importer in `loom-interchange-io`.
`ProfileImportPlan` supports the Jira source system, and `ProfileImportAction` pins planned ticket
actions for projects, issues, workflows, comments, attachments, source digests, target entity ids,
payload digests, notes, and fidelity issues. `loom interchange import-jira` accepts a normalized
snapshot JSON file, lowers projects through `project.created`, lowers issues through
`issue.created`, stores Jira issue ids as external ticket identities (`jira`, `issue:<id>`), stores
Jira keys and broad issue fields as ticket fields, applies labels as policy labels, skips duplicate
projects and issues idempotently, supports strict or inferred project-field definitions for retained
source fields, and emits the shared 0012 import report. The source-backed fixture
in `specs/studio/fixtures/jira/` is derived from Atlassian Jira Cloud REST API references for issues,
projects, comments, attachments, worklogs, issue links, project components, project versions,
watchers, votes, changelogs, transitions, and Jira Software sprints. It verifies project creation,
issue creation under inferred import-field policy, Jira key retention, ADF body retention,
user/object reference retention, status
category retention, date retention, parent/security/vote/watcher/sprint retention, component and
version retention, issue-link/subtask/transition retention, custom-field bundle retention,
label-to-policy-label preservation, external identity lookup, duplicate-safe storage through
existing identity checks, and fidelity issues for unsupported source structures. Jira live
API/export parsing, issue-key alias registration beyond the ticket field, native changelog/workflow/
agile lowering, identity mapping, comments, attachments, worklogs, native link edges, and native
component/version schema setup remain target work. Present but unsupported project metadata/schema
fields, issue links, subtasks, transitions, changelogs, comments, attachments, and worklog entries
emit shared fidelity issues.

### 25.0a Source-Backed Field Coverage

| Jira source shape | Current importer status | Evidence / remaining work |
| --- | --- | --- |
| Project `id`, `key`, `name` | 1:1 imported | Project row is created through the ticket profile service, with the key preserved as the key prefix. |
| Project description, type, style, simplified/archive/delete/private flags, API URL, avatar URLs, lead, category, insight, roles, issue types, components, and versions | unsupported with fidelity issue | Fixture covers the fields. Native project metadata, schema, component, version, role, and lead mapping remain target work. |
| Issue `id`, `key`, project, source summary, type, status, status category, priority, labels, dates, and custom fields | 1:1 imported or retained as ticket fields | Jira `summary` maps to native `title`. Labels are also preserved as policy labels. |
| Issue description and environment ADF-style bodies | retained as source or opaque ticket fields | Native rich body projection remains target. |
| Issue assignee, reporter, creator, parent, security, votes, watchers, sprint, properties, and development metadata | retained as source or opaque ticket fields | Native principal, hierarchy, security, reaction/watch, agile, property, and development projections remain target. |
| Issue components, fix versions, and affected versions | retained as source or opaque ticket fields | Native component and release projections remain target. |
| Issue links, subtasks, and transitions | retained as source or opaque ticket fields and unsupported with fidelity issue | Native relation, hierarchy, and workflow lowering remain target. |
| Changelog, comments, attachments, and worklogs | unsupported with fidelity issue | Fixture covers representative official shapes. Native operation replay, annotations, content-addressed attachments, and time logs remain target. |
| Live Jira API pagination, backup archive parsing, and Jira Software board export parsing | not yet fixture-covered | Current source accepts normalized JSON only. |

### 25.1 Mapping Table

| Jira export entity | Jiraish target | Notes / degrade path |
| --- | --- | --- |
| Project | `project.created`; key prefix reserved (§4.1) | issue counter set past highest imported number |
| Issue (incl. key) | `issue.created` + synthesized history (§25.2) | keys preserved verbatim as §4.1 aliases |
| Issue type (epic, subtask) | `issue_type`; Epic Link / parent -> `epic_of` / `parent_of`/`child_of` edges | both legacy Epic Link field and new-Jira unified parent map to edges |
| Status + status category | workflow state with `category` (§8) | - |
| Workflow (statuses, transitions) | versioned workflow definition (§8) | one workflow version per imported scheme |
| Conditions / validators | §7.3 guard kinds: permission condition -> `permission`; field-required validator -> `required_fields`; subtask-blocking -> `linked_issue_state`; resolution screens -> `resolution_required` | scripted (ScriptRunner etc.) -> dropped, flagged with original source retained |
| Post-functions | §8 automation where expressible (set field, assign, clear resolution) | webhooks and scripts -> flagged, not imported |
| Custom field | §20.1 definition: text->`string`/`text`, number->`number`, date(time)->`date`/`datetime`, select->`enum`, multi-select->`list<enum>`, user->`principal`, multi-user->`list<principal>`, version picker->`entity_ref(release)`, URL->`url`, labels->label facility | cascading select -> two enum fields with a constraint note, flagged degraded; plugin/computed fields -> `opaque_json`, flagged |
| Field configuration / contexts | §20.1 project contexts | - |
| LexoRank | fresh 0061 §5 order tokens preserving relative order | token values are never imported |
| Sprint (agile field history) | sprint entities + memberships (§7.5); `carried_from` edges where consecutive membership implies carry-over | carry-over derivation is a flagged heuristic |
| Board | board over project/filter; columns map by status category | column↔status mismatches flagged |
| Saved filter / board filter (JQL) | 0061 §14 predicate tree | unconvertible JQL (functions like `membersOf`, scripted) -> imported as flagged text pending assisted reauthoring (G2-style elicitation flow) |
| fixVersions / affectsVersions | `fix_release_ids` / `affects_release_ids` (§7.5, §20) | - |
| Version (release) | release entity with state | released/archived states preserved |
| Issue link | §9 edge kinds by link-type name | unknown custom link types -> `relates_to` with original name retained, flagged |
| Comment | 0061 §7 annotation; body per §20.2 (ADF stored natively) | - |
| Worklog | work-log annotation with `duration` | feeds `time_spent` projection (§20) |
| Attachment | 0061 §7.1 attachment (content-addressed) | - |
| Votes / watchers | reactions / watch registry (0061 §7.1) | - |
| User / group | principal via identity mapping (ADOPTION §1.1); unmapped -> inactive placeholder principals (§25.3) | - |
| Permission scheme / role | nearest §10 grant scopes | differences reported per scheme, flagged |
| Issue security level | `security_level` (`human_review` class, §7.2) | - |
| Changelog | synthesized backdated operations (§25.2) | - |

### 25.2 History Synthesis

Each changelog entry becomes an unconditional operation (no `base_entity_version`, so no conflict
records per §7.2) carrying the original actor's mapped principal, the original timestamp, and an
`import_provenance` envelope block: `{source_system, source_entity_id, import_run_id}`. Sequence
order follows changelog order. Imported issues therefore have first-class history: `tickets_history`,
audit timelines, velocity, CFD, and time-lapse all work over pre-import data, with provenance
distinguishing replayed facts from native ones. Descriptions and comments store their native ADF
under §20.2 content types - no conversion at import, no data destroyed; later conversions are
explicit revisions.

### 25.3 Identity

Jira users without a principal mapping get auto-created **inactive placeholder principals** (no login, no
grants, marked `imported`) so attribution stays accurate in every synthesized operation. SCIM sync
(ADOPTION §1.1) may later merge a placeholder into a real principal as one merge operation; references
follow, since they bind to principal ids.

### 25.4 Coexistence Bridge

The bridge is incremental one-way sync from Jira (ADOPTION §1.3). A mirrored project is
**read-only** in Jiraish until an explicit `project.cutover` operation (admin, confirm-elicitation):
full read surface - search, reports, graphs, watches - zero writes; write attempts reject with a
"mirrored from Jira, cutover pending" policy message. Cutover stops the bridge, records the
last-synced state as baseline, and opens writes. There is no write-back to Jira, ever. Read-only /
mirror / read-write are instances of the substrate scope operating modes (0061 §7.1), set per
project by an admin.

### 25.5 Fidelity Report

Every import run emits a fidelity report as a view (0061 §8), retained with the run:

```text
fidelity report:
  import_run_id, source_system, scope, timestamps
  per entity kind: {mapped, degraded, dropped} counts
  per degrade/drop: entity id, field, reason, original payload digest (retained in CAS)
  JQL conversion outcomes per filter (converted | flagged text)
  guard/post-function conversion outcomes per workflow
  identity: principals mapped, placeholders created
  aliases: keys preserved, counter values set, prefix reservations
```

Runs are idempotent by `import_run_id` + per-entity idempotency keys; re-running a failed batch
never duplicates operations.

## 26. Import from Asana (pinned 2026-07-04)

Build-focus requirement (ADOPTION §1.3). Runs through the 0012 interchange layer; bulk sources are
the organization export (gzip JSON, Enterprise), the resource export (gzip JSON-lines with
embedded `stories[]` and attachment metadata), or MCP-assisted normalized batches. Loom does not own
live Asana API credentials. Asana's CSV export is UI-only and is not an import source. The §25.2
history-synthesis, §25.3 placeholder identity,
§25.4 mirror/cutover, and §25.5 fidelity machinery are reused unchanged; this section defines only
the Asana-specific mapping. The stress test surfaced and fixed three model gaps (multi-project
membership §21, secondary keys §4.1, manual board mode §7.4) - the model was fixed, not the
importer.

Current source backs the normalized project/task slice through the reusable Asana importer in
`loom-interchange-io`. `loom interchange import-asana` accepts a normalized snapshot JSON file,
lowers projects through `project.created`, lowers tasks through `issue.created`, stores Asana task
ids as external ticket identities (`asana`, `task:<gid>`), stores core task fields and custom fields
as ticket fields, creates first-class manual Ticket Boards for project section placement, places
imported task cards on those boards with deterministic observed-order rank tokens, applies source
tags as policy labels, skips duplicate projects and tasks idempotently, and emits the shared 0012
import report. The source-backed fixture in
`specs/studio/fixtures/asana/` is derived from the official Asana task, project, story, attachment,
portfolio, goal, tag, user, team, and workspace API references. It verifies project creation, task
creation, date fields, approval fields, object reference retention, dependency list retention,
membership list retention, follower/like list retention, custom-field bundle retention,
tag-to-policy-label preservation, approval-task retention, first-class Board creation and card
placement, external identity lookup, duplicate-safe storage through existing ticket identity checks,
and fidelity issues for unsupported source structures. Asana organization/resource export parsing,
native multi-project membership edges, native subtasks, native stories, native attachments, native
portfolios, native goals, native identity mapping, and MCP-assisted normalized import remain target
work.
Present but unsupported project metadata/schema fields, memberships, subtasks, stories,
attachments, portfolios, and goals emit shared fidelity issues.

### 26.0a Source-Backed Field Coverage

| Asana source shape | Current importer status | Evidence / remaining work |
| --- | --- | --- |
| Workspace, team, user compact objects | retained as source or opaque data when attached to tasks; project-level references emit fidelity issues | Fixture covers workspace/team/user objects. Native principal/team mapping remains target. |
| Project `gid`, `name`, `key_prefix` | 1:1 imported | Project row is created through the ticket profile service. |
| Project archived/color/icon/date/view/permalink metadata | unsupported with fidelity issue | Fixture covers the fields. Native project metadata is target work. |
| Project current status and custom-field settings | unsupported with fidelity issue | Fixture covers status and schema settings. Native status annotations and project field schema are target work. |
| Project members and followers | unsupported with fidelity issue | Fixture covers both arrays. Native watch/member registries are target work. |
| Task `gid`, `name`, notes, resource subtype, completed state, dates, and source tags | 1:1 imported or retained as ticket fields | Tags are also preserved as policy labels. |
| Task approval status and approval task subtype | retained as ticket fields | Native approval workflow lowering remains target. |
| Task assignee, assigner, creator, completer, workspace, assignee section, external metadata | retained as source or opaque ticket fields; assignee/project section also feeds first-class Board placement | Native identity mapping, My Tasks section semantics, and external-data policy remain target. |
| Task dependencies and dependents | retained as source or opaque ticket fields | Native `blocks` and `blocked_by` relation lowering remains target. |
| Task memberships | retained as source or opaque ticket fields and unsupported with fidelity issue; first available section feeds current Board placement | Native multi-project membership edges remain target. |
| Task followers, likes, like counts, subtasks count, actual-time minutes, separator flag | retained as ticket fields | Native watch/reaction/time-tracking projections remain target. |
| Task custom fields | retained as source or opaque ticket field bundle | Native typed field schema lowering remains target. |
| Subtasks, stories, attachments, portfolios, and goals | unsupported with fidelity issue | Fixture covers representative official shapes. Native lowering remains target. |
| Raw organization export archive and resource export JSON-lines | not yet fixture-covered | Current source accepts normalized JSON only. |

### 26.1 Mapping Table

| Asana entity | Jiraish target | Notes / degrade path |
| --- | --- | --- |
| Team | project grouping label | Asana teams contain projects; no Jiraish entity - recorded as metadata |
| Project | `project.created`; key prefix **generated** from the project name (Asana has no keys) | deterministic slug + collision suffix; counter starts at 1; fidelity lists generated prefixes |
| Section | manual-board column (§7.4) | every project imports with one manual board mirroring its sections; the implicit "(no section)" becomes the first column |
| Task | `issue.created` (type task) + synthesized history (§26.2); primary home = earliest membership, overridable per run config | primary key drawn from the home project's counter; **no secondary keys assigned at import** (nothing to preserve) |
| Multi-homed task (`memberships[]`) | `member_of` edges (§21) with per-board manual placement | project+section pairs map 1:1 onto membership + column placement |
| Subtask (`parent`) | `parent_of`/`child_of` edge | source nests ≤5 levels; unlimited here; source semantics preserved - subtasks get no project membership unless they had one |
| Task order within project/section | fresh 0061 §5 order tokens from read order | Asana exposes no rank value; positional order captured at read time, flagged as observed-order |
| Milestone (`resource_subtype: milestone`) | issue (type task, label `milestone`) + §21 milestone entity whose criteria is that issue | flagged; no milestone issue type exists by design |
| Approval task (`resource_subtype: approval`) | issue on an imported approval workflow; `pending -> in_progress` (category), `approved -> accepted`, `rejected -> done (resolution: rejected)`, `changes_requested -> done (resolution: changes_requested)` | Asana marks changes_requested complete; fidelity notes the convention that teams reopen |
| assignee / followers / likes | assignee / watch registry / reactions (0061 §7.1) | - |
| Tags | label facility (0061 §7.1) | Asana tags are organization-scoped and flat - direct fit |
| due_on/due_at, start_on/start_at | `due_date`, `start_date` (§20 time) | date-only vs datetime preserved |
| dependencies/dependents | §9 `blocks`/`blocked_by` edges | explicit semantic relations in the source - allowed (not extraction) |
| Custom field: text/number/enum/multi_enum/date/people/reference | §20.1: `string`/`text`, `number` (precision + format as constraints), `enum` (Asana's disable-only options map onto §20.1 retired options exactly), `list<enum>`, `date`/`datetime`, `list<principal>`, `entity_ref` | currency/custom-label formats preserved as display constraints |
| Custom field: formula / custom_id representation types | frozen typed field, `imported_computed` (§20.1) | expression retained in fidelity + CAS |
| Custom field settings (`is_important`) | §20.1 project context | - |
| Story (`type: comment`) | 0061 §7 annotation; `html_text` stored native per §20.2 content types | - |
| Story (`type: system`) | backdated operations per §25.2 machinery (§26.2) | - |
| Attachment | 0061 §7.1 attachment | resource-export lines omit download URLs - bytes fetched via API, else metadata-only import flagged partial |
| Portfolio | roadmap item, kind `initiative`, in one imported organization roadmap (§21); portfolio nesting -> item parent edges; contained projects -> `contains` edges | portfolio custom fields -> §20.1 fields on roadmap items |
| Goal | roadmap item, kind `initiative`; `time_period` -> timeframe; metric -> progress rollup (`manual`/`external` sources -> manual; task/milestone-completion sources -> issue_count-style rollup over supporting-work edges) | `goal_relationships` (subgoal, supporting_work, contribution_weight) -> typed weighted edges |
| Status update (project/portfolio/goal) | annotation (kind status_update) on the mapped entity, `status_type` preserved | - |
| Rules | **not importable - no API export exists** (definitions are not readable) | fidelity gap: manual re-authoring via the G2 flow; the report states the gap without a count (even the rule list is unavailable) |
| Forms | **not importable - no API** | fidelity gap; structured intake is a recorded product gap (§24) |
| User | principal via identity mapping; unmapped -> inactive placeholders (§25.3) | - |

### 26.2 History Synthesis from Stories

System stories synthesize backdated unconditional operations exactly per §25.2 (import_provenance,
original actor and timestamp, sequence follows story order): `assigned`, `marked_complete`/
`marked_incomplete`, `due_date_changed` (old/new dates), `section_changed` (-> board placement
operation), `added_to_project`/`removed_from_project` (-> membership operations), approval-status
changes, and typed custom-field deltas (old/new value pairs are present in the story schema).
Caveats, all fidelity-reported: description-edit stories carry no old/new bodies - they synthesize
**edit-marker operations** (the revision fact without a payload, flagged; the final body comes
from the task resource); stories can be trashed in the source, so history is best-effort; system
story `text` is locale-dependent and is never parsed - mapping keys off `resource_subtype` only.

### 26.3 Identity, Bridge, Fidelity

As §25.3-§25.5, with the fidelity report extended per run: generated key prefixes, membership/
placement counts, frozen computed fields with retained expressions, the rules/forms gaps, and
approval/milestone mapping notes.

## 27. Import from Redmine (pinned 2026-07-12)

Redmine import is a requirement (ADOPTION §1.3). It runs through the 0012 interchange layer and uses
the same idempotency, placeholder identity, mirror/cutover, and fidelity machinery as §25. Redmine is
the low-friction importer for proving ticket execution because projects and issues map directly onto
the ticket profile service, while journals, relations, time entries, and wiki pages exercise the
operation-log and Pages boundaries.

Current source backs the normalized project, issue, and wiki-page slice through the reusable Redmine
importer in `loom-interchange-io` and `loom interchange import-redmine`. It accepts a normalized
snapshot JSON file or Redmine XML file, lowers projects through `project.created`, lowers issues through
`issue.created`, lowers wiki pages through the Pages profile service, stores Redmine issue ids as
external ticket identities (`redmine`, `issue:<id>`), skips duplicate projects, issues, spaces, and
unchanged pages idempotently, supports strict or inferred project-field definitions for retained
source fields, and emits the shared 0012 import report. The XML adapter normalizes
`<projects>`, `<issues>`, and `<wiki_pages>` into the same import model as JSON instead of creating a
separate lowering path. `redmine_import_snapshot` exposes the same normalized execution path over MCP.

The source-backed fixture under `specs/studio/fixtures/redmine/` is derived from Redmine's REST API
documentation for issues, projects, wiki pages, attachments, time entries, issue relations, trackers,
statuses, versions, custom-field definitions, and enumerations. It verifies project creation, broad
issue mapped fields, Redmine parent/private/url fields, custom fields, journals, comments, watchers,
affected versions, attachments, time entries, relations, children, changesets, allowed statuses, wiki
parent/title/body import, and fidelity warnings for source structures the current target projections
cannot lower. Native ticket comments, attachment bytes, typed links, work logs, releases, principal
mapping, project schema setup, wiki revision metadata, and synthesized backdated operation replay
remain target profile capabilities rather than hidden importer successes.

### 27.1 Normalized Snapshot Shape

The source-backed snapshot shape is intentionally post-adapter JSON:

```json
{
  "source_scope": "redmine://example",
  "projects": [
    {"id": 1, "identifier": "core", "key_prefix": "CORE", "name": "Core"}
  ],
  "issues": [
    {
      "id": 42,
      "project_identifier": "core",
      "tracker": "Bug",
      "subject": "Login fails",
      "description": "Fails on Safari",
      "status": "New",
      "priority": "High",
      "assigned_to": "alice",
      "custom_fields": {"severity": "critical"},
      "journals": [{"id": 7, "notes": "Status changed"}],
      "comments": [{"id": 8, "text": "Needs logs"}],
      "attachments": [{"id": 9, "filename": "error.txt"}],
      "time_entries": [{"id": 10, "hours": 1.5}],
      "relations": [{"id": 11, "relation_type": "blocks"}]
    }
  ],
  "wiki_pages": [
    {
      "id": "Home",
      "project_identifier": "core",
      "page_id": "home",
      "title": "Home",
      "markdown": "# Home\nRedmine wiki body"
    }
  ]
}
```

File, XML, or assistant-provided adapters must lower into this shape or an equivalent canonical
import batch before calling profile operations. This keeps source-specific XML quirks and attachment
extraction out of the ticket profile service.

### 27.2 Mapping Table

| Redmine entity | Jiraish target | Notes / degrade path |
| --- | --- | --- |
| Project | `project.created`; key prefix from snapshot `key_prefix` or deterministic identifier-derived prefix | duplicate projects skip idempotently |
| Project metadata and schema | fidelity report until project metadata/schema setup exists | description, homepage, status, public flag, parent, default version, default assignee, trackers, categories, modules, activities, and custom-field definitions are source-covered but not natively lowered |
| Issue | `issue.created`; external identity `redmine` + `issue:<id>` | duplicate issues skip idempotently |
| Tracker | ticket type when it matches `bug`, `story`, `epic`, `spike`, or `subtask`; otherwise `task` with original tracker retained as a field | unknown trackers are not dropped |
| Subject / description | `subject` / `description` fields | description body is retained as source text until a richer body profile is wired |
| Status / priority / assignee / author / timestamps / URL | typed ticket fields | assignee and author are text in the current normalized slice; principal mapping remains target |
| Category / fixed version / affected version / closed timestamp | typed ticket fields; later release and history projections | source-backed field import; native release/history projection remains target |
| Parent issue id / private flag | typed ticket fields | typed hierarchy/privacy semantics remain target |
| Custom fields | `custom_fields` opaque field with inferred project-field definitions when import field policy is `infer` | Native source-specific custom-field schema promotion remains target work. |
| Journals | structured `redmine_journals` field; later synthesized backdated operations (§25.2) | source-backed retention; native replay remains target work |
| Notes/comments | structured `redmine_comments` field; later 0061 §7 annotations | source-backed retention; native comments remain target work |
| Attachments | structured `redmine_attachments` field; later 0061 §7.1 attachments with content digests | source-backed metadata retention; attachment byte import remains target work |
| Time entries | structured `redmine_time_entries` field from issue-nested or top-level Redmine time-entry resources; later work-log annotations with duration and activity | source-backed retention; native work logs remain target work |
| Relations (`precedes`, `follows`, `blocks`, `duplicates`, `relates`) | structured `redmine_relations` field; later §9 typed issue edges | source-backed retention; edge projection remains target work |
| Children / changesets / allowed statuses | structured `redmine_children`, `redmine_changesets`, and `redmine_allowed_statuses` fields | source-backed retention; native hierarchy, VCS-link, and transition policy projection remain target |
| Watchers | structured `redmine_watchers` field; later watch-registry mapping | source-backed retention; native watch registry remains target work |
| Versions | release entities and release fields | source-backed as retained ticket fields for issue versions; release entity creation remains target work |
| Users / groups | principal mapping; unmapped users become inactive placeholders (§25.3) | target work |
| Wiki pages | Pages spaces/pages; `space_id` or project id selects the space, `page_id` or source id selects the page, parent title maps to a stable parent page id, and `markdown`/`body`/`text` lowers through the structured Pages body importer | source-backed for normalized snapshots; source revision history, provenance, comments, and wiki attachments remain target work |

### 27.2.1 Current Redmine Coverage Matrix

The current Redmine fixture is a broad source-backed execution fixture for the first production
import slice. It is not a full native-projection acceptance suite: fields classified as retained are
preserved as ticket fields or `redmine_*` source extras, while native projections such as release
entities, principal placeholders, and backdated operation replay remain target work.

| Redmine source field or entity | Current handling | Fixture coverage |
| --- | --- | --- |
| Project id, identifier, key prefix, name | Imported into project identity and metadata | Covered |
| Project description, homepage, status, public flag, parent, default version, default assignee, created_on, updated_on | Unsupported with fidelity issue until project metadata storage exists | Covered |
| Project trackers, issue categories, enabled modules, time-entry activities, issue custom-field definitions | Unsupported with fidelity issue until project schema setup exists | Covered |
| Issue id and project | Imported as external ticket identity and project id | Covered |
| Tracker, subject, description | Imported as ticket fields | Covered |
| Status, priority | Imported as ticket fields; native workflow mapping is target | Covered |
| Status and allowed statuses | Status-mapped first-class Ticket Board columns when workflow lowering exists | Target work; Redmine source retains allowed-status samples today |
| Author, assignee | Imported as text fields; principal mapping is target | Covered |
| Category, fixed version, affected version | Imported as ticket fields; release mapping target | Covered |
| Start date, due date, created_on, updated_on | Imported as ticket fields | Covered |
| Closed_on | Imported as ticket field; timestamp/history mapping target | Covered |
| Done ratio, estimated hours | Imported as numeric ticket fields | Covered |
| Parent issue id, private flag, URL | Imported as ticket fields; native hierarchy/privacy/link semantics target | Covered |
| Custom fields | Retained as opaque `custom_fields`; schema/value mapping target | Covered |
| Journals | Retained structurally in `redmine_journals`; native history synthesis target | Covered |
| Journal details | Retained structurally under `redmine_journals`; field-change replay target | Covered |
| Comments | Retained structurally when present; native annotations target | Covered |
| Attachments | Metadata retained structurally, including filename, content type, URL, description, author, and timestamp; byte import target | Covered |
| Time entries | Retained structurally, including issue/project, user, activity, hours, comments, spent_on, created_on, and updated_on; work-log projection target | Covered |
| Relations | Retained structurally with relation variants; native edge projection target | Covered |
| Children, changesets, allowed statuses | Retained structurally in Redmine-specific fields; native hierarchy, VCS-link, and transition policy target | Covered |
| Watchers | Retained structurally; watch registry mapping target | Covered |
| Wiki title and body | Imported into Pages | Covered |
| Wiki parent | Parent title maps to a stable parent page id when the parent page is present in the bundle | Covered |
| Wiki version, author, comments, timestamps, attachments | Unsupported with fidelity issue until Pages revision/provenance and attachment storage exists | Covered |
| Users, groups, roles | Target principal/group/grant mapping | Covered as source identities where attached to issues, comments, journals, watchers, and time entries; native mapping target |
| Trackers, issue statuses, priorities, enumerations | Target workflow/schema setup; project-local tracker and activity samples are fidelity-reported | Partially covered |

### 27.3 Fidelity Report

The normalized snapshot slice reports planned/applied/skipped counts through the shared 0012 import
report. Wiki pages create or reuse Pages spaces, publish changed page bodies, and skip unchanged
pages idempotently. Redmine journals, comments, attachments, time entries, and relations are retained
as structured ticket fields. The Redmine API-shaped fixture imports into a clean store and compares
the represented source fields against the resulting ticket and page state. The final native
ticket-profile integration must additionally report journal synthesis coverage, principal
mappings/placeholders, relation delays, attachment byte coverage, time-entry activity coverage, and
source wiki-page revision coverage.
