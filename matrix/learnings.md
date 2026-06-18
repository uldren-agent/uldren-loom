# Work Graph Learnings

This file records reusable lessons from operating `matrix/matrix.loom` as a work graph. It is a
standalone operating notebook kept beside the Loom file so future work graphs can reuse the model
without relying on chat history.

## Current Context

| Item | Current state |
| --- | --- |
| Loom file | `matrix/matrix.loom` |
| Local binary | `matrix/loom` |
| Workspace | `matrix2` |
| MCP server | `matrix` |
| Primary model | Graph-backed work coordination with document-backed payloads |
| Human-readable fallback | Mounted files or generated Markdown views |

## Core Operating Lesson

The work system should be modeled as a graph, not as a matrix or serial queue. A matrix is useful
for human scanning, but dependencies, assignment, prompt resources, results, verification, and
decision points are graph relationships.

## Throughput Model

The main throughput constraint is not raw agent count. It is work-definition quality, review churn,
shared-worktree contention, and too-frequent control handoffs.

Adding agents before fixing those will make the graph noisier and increase merge and verification
cost.

`KV-CM-001` is the right task shape after its latest correction: one coherent vertical that owns
native behavior, durable state, caller migration, conformance, and only the adjacent projections
that can be completed end-to-end. Earlier packets were not adequate because they created narrow
patches and review churn instead of closing a behavior boundary.

The recommended model is graph-driven delivery with one arbiter and implementation lanes. The
arbiter owns architecture, task readiness, dependency release, verification, and integration.
Workers own a coherent vertical with explicit source anchors, a concrete write set, focused checks,
and a single result boundary. A batch should usually contain 30-90 minutes of coherent work. A
successful compilation or focused-test checkpoint is a valid handoff boundary when the assigned
work unit is complete and evidence is recorded.

Maintain a ready frontier at least two task levels deep for each active lane. Prioritize the
critical path, ROI, dependency readiness, write-set isolation, verification cost, and public
decision risk. Workers may share the worktree by owner choice. A collision is an integration event
for arbiter review, not an automatic reason to block parallel work.

## Active-Frontier Discipline

High return on investment is a ranking input, not authorization to select unrelated work. When the
owner identifies an appendix, program, or architectural boundary as the active frontier, the arbiter
must constrain new assignments to that boundary. Tasks outside it may remain recorded as ready but
must not be selected merely because they rank highly.

## Recommended Data Shape

| Work-graph concept | Loom representation | Reason |
| --- | --- | --- |
| Task | Graph node plus task document | The graph stores relationships; the document stores rich metadata. |
| Prompt | Document or file node linked from a task and agent | Prompt text is large and should not live directly in graph edge state. |
| Agent session | Graph node | Assignments, claims, and inbox resources can point to the agent. |
| Result | Document or file node linked to task and agent | Results should stay separate from task definitions. |
| Source anchor | Graph node linked to specs, files, or resources | Verification needs to know what the worker relied on. |
| Decision point | Graph node linked to blocked work | Owner questions should stay visible and queryable. |
| Dashboard | Generated app, document, or file view over the graph | The dashboard is a view, not the source of truth. |

## Useful Edge Types

| Edge | Meaning |
| --- | --- |
| `depends_on` | A task cannot complete until another node is complete. |
| `derived_from` | A task came from a source file, source section, decision, or earlier task. |
| `assigned_to` | A task is currently assigned to an agent node. |
| `prompted_by` | A task or agent uses a prompt resource. |
| `writes_result` | An agent must write output to the linked result resource. |
| `blocked_by` | A task is waiting on a decision, missing source, or external condition. |
| `verifies` | A verification node or main-session review result applies to the claimed work. |
| `updates` | A task is expected to update a spec, prompt, graph node, dashboard, or result. |
| `supersedes` | A new prompt, task, or result replaces an older one. |
| `related_to` | Non-blocking context relationship. |

## Main Session And Worker Loop

| Step | Actor | Action |
| ---: | --- | --- |
| 1 | Main session | Creates or updates graph nodes, task documents, and prompt resources. |
| 2 | Main session | Assigns a ready task to an agent by linking task, agent, prompt, and result target. |
| 3 | User | Relays the prompt resource URI or instruction to the worker session. |
| 4 | Worker agent | Reads the prompt resource, performs the scoped task, and writes results to the assigned result resource. |
| 5 | User | Tells the main session the worker finished. |
| 6 | Main session | Reads the result, checks source backing, checks files or graph updates, and records verification. |
| 7 | Main session | Marks the task accepted, rejected, blocked, needs revision, or complete. |
| 8 | Main session | Writes the next prompt resource for that worker. |
| 9 | User | Tells the worker to go again. |

## Why Loom Should Own The Work Graph

| Need | Loom-backed graph behavior |
| --- | --- |
| Dependency traversal | Query graph edges instead of manually reading Markdown tables. |
| Assignment | Link task nodes to agent nodes and prompt resources. |
| Result review | Link result documents to tasks, agents, source anchors, and verification records. |
| Reassignment | Replace or supersede assignment edges without rewriting the task definition. |
| Auditability | Use Loom history and explicit result records instead of relying on chat memory. |
| Mounted inspection | Mount the Loom workspace and inspect generated files or app views. |
| Dashboarding | Query unfinished, claimed, blocked, finished, and verification-failed nodes into a full-screen app. |

## Dashboard Direction

The dashboard should be a generated view over graph and document data. It should not become the
source of truth.

| Panel | Query intent |
| --- | --- |
| Ready high-ROI work | Tasks with no incomplete `depends_on` edges and no active `blocked_by` edge. |
| Agent inboxes | Active `assigned_to`, `prompted_by`, and `writes_result` relationships per agent. |
| Awaiting verification | Result nodes written by agents but not accepted or rejected by the main session. |
| Blocked work | Tasks linked to unresolved decision nodes or missing source anchors. |
| Recent completions | Accepted result nodes and completed task nodes. |
| Graph health | Orphan tasks, missing prompt resources, stale assignments, missing result targets, and inconsistent status. |

## Prompt Resource Requirements

Each prompt resource should tell the worker:

| Requirement | Reason |
| --- | --- |
| Scope | Prevents the worker from touching unrelated files or graph nodes. |
| Source anchors | Forces the worker to ground claims before editing. |
| Result target | Ensures the worker writes output where the main session can verify it. |
| Stop conditions | Prevents guessing when an owner decision is needed. |
| Required output | Makes verification consistent. |
| Checks | Separates verified work from unverified analysis. |

## Agent Operating Rules

These rules should be stored in the work graph and included in every first-job prompt for a worker
agent.

### Questions

Questions must use readable Markdown sections, not a buried two-column table.

Workers must attach an owner decision to the active task document and mark the task or board
`awaiting_decision` before asking in chat. The graph record is authoritative. Chat only notifies the
owner that a decision is waiting; it must not be the sole location of the question.

Each question must include:

- Context: explain the contrast between possible solutions and why the decision matters.
- Options: describe each option with enough detail, tradeoffs, and consequences to decide. Do not use
  short labels or one-sentence placeholders.
- Recommendation: recommend the best choice and explain why.
- Consequence of deferring: explain what remains blocked or risky if no decision is made.

### Decision Persistence Before Automatic Graph Projection

Until Ticket relationship projection is implemented, an agent with an owner decision must record it
before asking in chat:

1. Create a decision document at `decisions/<task-id>/<short-decision-id>` using the required
   Question, Context, Examples, Options, Recommendation, and Consequence of Deferring sections.
2. Update the assigned Ticket with `status: awaiting_decision`, `decision_id`, and
   `decision_resource`, using the Ticket's optimistic-concurrency root.
3. Stop at the affected boundary and report the Ticket key plus decision resource in chat.
4. The arbiter verifies the record, creates the transitional Graph relation, records the owner answer,
   and updates the stable board before the worker resumes.

Chat is notification only. A question without the Ticket and decision-resource records is incomplete.

### Recommendations

Recommendations must optimize for DRY, performant, long-term, enterprise decisions.

That means:

- Prefer reusable shared primitives over duplicate per-facet systems.
- Prefer durable contracts and conformance paths over shortcuts.
- Preserve performance and source-backed verification.
- Keep design scope holistic while keeping implementation scope precise.
- Stop for owner input when a public contract, naming choice, or ownership boundary is unclear.

### First-Assignment Priming

Before a worker starts its first assigned batch, its board must state that it must re-read the active
task and operating rules, ground implementation claims in source, attach required owner decisions to
the task document, and use the required decision format. The board must also state that recommendations
optimize for DRY, performance, long-term maintenance, and enterprise contracts. This avoids relying on
unstated chat conventions that a newly attached worker has not read.

## Open Implementation Notes

| Topic | Current note |
| --- | --- |
| Graph properties | The CLI accepts canonical CBOR `text -> bytes` props. Large prompt and result text should live in documents or files, not directly in graph props. |
| MCP availability | The `matrix` MCP is connected and can list document collections. |
| Workspace state | `matrix2` exists in `matrix/matrix.loom`. |
| Initial collections | No document collections were present when first checked through MCP. |
| First build step | Seed task, prompt, agent, source, and result documents, then add graph nodes and edges that link them. |
| First seeded graph | The `workgraph` graph now contains the initial Matrix2 task, prompt, source, agent, assignment, and result nodes. |
| First assigned task | `FP-SUB-001` is assigned to `agent:1` with result target `results/FP-SUB-001/run-001`. |
| MCP document reads | Use `document_get_text` for boards, tasks, decisions, prompts, results, learnings, Markdown, JSON, and other UTF-8 coordination content. Use binary document reads only for intentional binary payloads. |
| MCP document writes | Use `document_put_text` with the normal `text` field for text-native coordination content. Supply `expected_digest` for guarded updates after `document_get_text`; omit it only for intentional blind upserts. Use binary document writes only for intentional binary payloads. |
| Current prompt pattern | Store an agent's active prompt in `prompts/<agent>/current` as a self-contained prompt body that includes task coordinates, result target, stop conditions, and the task prompt body. |
| Operating rules | Store shared agent rules as work-graph metadata, then inject them into each agent's current prompt before assignment execution. |
| Agent board pattern | Store the preferred stable worker handoff in `boards/<agent>`. The main session updates that board; the user tells the worker `go`; the worker re-reads the board and executes the current assignment. |
| Verification records | Keep worker results immutable as claims and record main-session acceptance, fixes, or rejection in a separate `verifications` document. Link accepted task state to the verification record. |

## Stable Agent Board Contract

This is the required operating model for repeat worker sessions. It replaces per-turn pasted
prompts.

The user should give a worker the board resource once. After that, the main session updates the
same board document, and the user only tells the worker `go`.

| Element | Contract |
| --- | --- |
| Board resource | Store each worker's current assignment in a stable document such as collection `boards`, id `agent-1`. |
| Initial setup | The user tells the worker the board resource once, including that `go` means re-read the board. |
| Main-session update | The main session updates the same board document after verification, revision, or reassignment. |
| User signal | The user tells the worker `go`; this is not a new prompt and does not carry hidden assignment details. |
| Worker behavior | On `go`, the worker re-reads the board, executes the current assignment, writes the named result target, and reports finished. |
| Main-session rule | Do not make the user copy and paste a fresh prompt after every assignment. Update the board instead. |
| Worker rule | Do not rely on a cached assignment. Re-read the board every time the user says `go`. |
| Review rule | The main session verifies the result, updates graph state, and writes the next assignment into the same board resource. |

### One-Time Worker Setup Prompt

Use this once when attaching a worker to the graph:

```text
Use the Matrix MCP board resource as your standing assignment source:

collection `boards`, id `agent-1`

Whenever I say `go`, re-read that board, execute the current assignment in its `body`, write the
result to the result target named there, and then tell me you are finished.
```

After this setup, do not paste new assignment prompts to that worker. The main session changes the
board; the user relays only `go`.

## `go` Contract

The same short signal has different responsibilities by role:

| Session | Meaning of `go` |
| --- | --- |
| Worker | Re-read the stable board. Execute the assigned batch. If no assignment exists, remain idle and honor a future `go` by re-reading the board again. |
| Arbiter | Verify any completed worker results, update or correct worker boards, release ready dependencies, assign available workers, and generate the next ready graph tasks. If there is no result to verify, continue with task generation and queue preparation instead of stopping. |

The arbiter `go` operation is intentionally lenient. It is a work-advancement signal, not merely a
request to inspect a particular result. The user should not need to choose whether the arbiter is
verifying, assigning, or preparing the next frontier.

## Guardrails

| Guardrail | Reason |
| --- | --- |
| Treat the graph as authoritative | Markdown tables drift under concurrent agent work. |
| Keep rich text out of graph props | Documents and files are easier to inspect, mount, and update. |
| Keep targeted sections as navigation hints | The full source file remains the dependency scope for primitive-derived work. |
| Verify before accepting worker results | A worker claim is not complete until the main session checks source backing and actual changes. |
| Preserve owner questions as nodes | Questions should not disappear into chat or result prose. |
| Make current prompts self-contained | A worker should not need to reconstruct the task from multiple tables before it can act. Linked task documents remain useful for verification, but the current prompt should be executable. |
| Prime agents with operating rules | A worker's first prompt should restate question and recommendation rules so the result format does not regress. |
| Use stable boards for repeat work | Once an agent has a board resource, assignment handoff must happen by updating that board and telling the agent `go`. Do not ask the user to copy and paste replacement prompts. |

## Arbiter Decomposition Gate

The arbiter must turn a broad capability request into an executable delivery contract before assigning
it to a worker. A feature label such as "implement metrics" is not an assignment. It leaves the
worker to make public-contract, ownership, and scope decisions while coding, which causes premature
handoffs and review churn.

Before a task is assigned, its graph task document must state:

| Required field | Arbiter responsibility |
| --- | --- |
| Outcome | Name the user-observable behavior or durable internal contract that will exist when the task is accepted. |
| Boundary | Name the owning crates, public surfaces, source anchors, and explicitly excluded areas. |
| Decisions | Resolve architectural choices first, or record the task as blocked on a named owner decision. Do not assign unresolved product design as implementation work. |
| Work units | Break the result into cohesive implementation units: model, persistence or execution, public projection, conformance, and focused tests. Keep a unit below lift 6 unless an atomic migration requires otherwise. |
| Acceptance evidence | Define exact behavior, negative cases, migration expectations, and the focused checks that prove the contract. |
| Handoff rule | Permit handoff after a successful focused check when the bounded work unit is complete and evidence is recorded. Do not force adjacent work into the task merely to suppress handoffs. |
| Ticket-sourced assignments | Assignments must be sourced from Tickets. A pasted chat instruction can help an agent continue when lookup is unavailable, but it is not an updatable Ticket. If the agent cannot read the Ticket through Matrix, it must report evidence in chat and stop rather than inventing a document, result resource, or hybrid workflow. |

For larger work, the arbiter creates a parent capability task and independently assignable child tasks.
Dependencies must describe real ordering, such as canonical model before ABI projection, rather than
the order in which the work happens to be convenient. Batches may group completed-ready child tasks
only when they share source ownership and verification scope.

## Design Before Decomposition

Architecture and implementation planning are separate responsibilities. During design review, the
arbiter must recommend the durable, DRY, performant, long-term enterprise target without reducing it
to fit the current task, current implementation, or a worker's immediate convenience. After the owner
selects that target, the arbiter decomposes it into bounded implementation tasks with explicit
dependencies, ownership, and acceptance evidence. Task size is a reason to split implementation work,
not a reason to weaken the design.

## Facet Promotion Completeness

When a new facet is queued, its parent task must define the full promotion path before workers begin
implementation. The path includes native storage and semantics, local client and CLI, IDL and C ABI,
all applicable language bindings, MCP local and remote operation, hosted transports where the facet has
a served surface, capability reporting, conformance, and source-backed specification reconciliation.
Native implementation alone is a foundation task, not facet completion. Every inapplicable surface must
be explicitly recorded with its reason; it may not disappear by omission.

## Worker Ticket Queues

A worker board can carry a queue of tickets instead of a single current assignment. This is the better
default once a lane has several dependency-ordered tasks with the same source area and review pattern.
The worker's `go` contract becomes:

1. Re-read the stable board.
2. Resolve arbiter feedback on queued tickets before starting new work.
3. Select the next unblocked ticket in queue order.
4. Treat the ticket as the source of truth.
5. Record owner questions on the ticket, with context, options, and recommendation.
6. Write closeout evidence to the ticket result target.
7. Continue to the next unblocked queued ticket when the completed unit and focused checks are done.

The arbiter's `go` contract becomes queue-aware: review completed queued tickets, record feedback on
the relevant tickets, update the worker board with lane guidance, and keep enough ready tickets in the
queue that the worker does not bounce after one small task. This reduces chat relay overhead while
preserving review quality.

## Ticket Workflow State

Default Loom ticket projects should be permissive. The single workflow field is `status`; strict
workflow-edge validation is an opt-in project configuration through an active workflow. Arbiter review,
worker feedback, and lane coordination should use `status` plus ordinary ticket fields, comments, and
result resources, not hidden `review_status` or `arbiter_review_status` fields.
