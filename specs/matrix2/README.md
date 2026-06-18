# Matrix2 Dynamic Work Matrix

This folder turns the analysis in `specs/_FACET_PRIMITIVES.md` into a pull-based work matrix.
It is not a serial queue. It is a registry of scoped task packets that can be claimed by any
available agent session when the packet is ready.

The whole `specs/_FACET_PRIMITIVES.md` file is the dependency source for this matrix. Individual
appendices or facet sections are navigation aids only. A packet must not treat one appendix as the
complete dependency boundary, because the file was created to remediate primitive clarity across
the entire facet, facade, substrate, conformance, and packaging landscape.

## Corrected Option Labels

The approved model is Option C below.

| Option | Model | Assessment |
| --- | --- | --- |
| Option A | One serial queue | Simple, but it blocks high-ROI work behind unrelated lower-value work. |
| Option B | One permanent queue per lane | Clearer than a flat queue, but it can trap an agent in one workstream even when better ready work exists elsewhere. |
| Option C | Dynamic pull matrix with prompt-file packets | Recommended. Lanes categorize work, while 3-5 generic agent sessions pull the highest-value ready packet. |

The key correction is that "one agent per lane" is not Option C. It is a rejected fixed-assignment
model. Matrix2 uses generic worker sessions that pull task packets from the matrix.

## Folder Layout

| Path | Purpose |
| --- | --- |
| `README.md` | Defines the operating model and option labels. |
| `MATRIX.md` | Active task matrix, pull rules, lane definitions, and packet registry. |
| `prompts/README.md` | Prompt-packet format and execution rules for agent sessions. |
| `prompts/*.md` | One executable prompt packet per task. Each packet includes a results section for the executing session to update. |

## Operating Model

| Concept | Meaning |
| --- | --- |
| Lane | A managed work coordination object assignable to an agent, principal, or team. |
| Packet | A ready-to-run scoped task file under `prompts/`. |
| Agent queue | A generic worker session. It does not permanently own a lane. |
| Pull rule | Pick the highest-ROI ready packet that matches the worker's context, lift budget, and dependency state. |
| Result handoff | The worker updates the packet's results section. A review session can then evaluate the packet output. |
| Source dependency | The full `specs/_FACET_PRIMITIVES.md` file is in scope for every packet. Targeted sections narrow reading order, not dependency scope. |

## Owner Question Routing

Owner questions must route through the active ticket before chat notification. Chat is not the
authoritative record for unresolved decisions.

When an agent needs owner input, it must:

1. Create or update `decisions/<active-ticket-id>/<short-decision-id>` with the required Markdown
   sections: Question, Context, Examples, Options, Recommendation, and Consequence of Deferring.
2. Update the active ticket with `status: awaiting_decision`, `decision_id`, and
   `decision_resource`, using the ticket's optimistic-concurrency root.
3. Set the Lane to `waiting_for_decision` if the question blocks the Lane's active ticket.
4. Stop at the affected boundary.
5. Report only the ticket key and decision resource in chat.

The arbiter verifies the decision record, creates any transitional graph relation while automatic
projection is unfinished, records the owner answer, and updates the Lane or board before the worker
resumes. If no ticket write surface is available, the agent must not ask only in chat; it must record
the missing ticket update surface as a blocker in its result.

## MCP Text and Ticket Operation Friction

Matrix coordination documents are agent-readable UTF-8 text or JSON unless a task explicitly needs
opaque bytes. Workers should therefore prefer text document tools for prompts, boards, decisions,
and results when those tools are available:

| Operation | Preferred path | Fallback path |
| --- | --- | --- |
| Read an agent-readable document | `document_get_text` | `document_get_binary`, then decode the returned bytes as UTF-8. |
| Write an agent-readable document | `document_put_text` with `expected_entity_tag` when updating an existing record | `document_put_binary` with UTF-8 bytes and `expected_entity_tag` when only the binary tool is exposed. |
| Write opaque content | `document_put_binary` | None; do not force binary content through text tools. |

When a client only exposes integer byte arrays, the worker should generate the byte array from a
local UTF-8 string instead of hand-writing long arrays. The result must state that a binary fallback
was used so the arbiter can distinguish tooling friction from task substance.

Ticket operations are separate from document writes. A worker must not infer that writing a result
document updates ticket state. If the active ticket update surface is unavailable, the worker records
the missing ticket operation as a blocker in the result and stops at any boundary that requires a
ticket state transition.

## Selection Rule

When several packets are ready, prefer the packet with the best combined value:

1. Highest ROI.
2. Highest cross-facet unblock value.
3. Lowest unresolved owner-decision risk.
4. Smallest lift that still delivers meaningful progress.
5. Clearest source anchors and allowed file scope.

The worker should read the whole primitive file enough to understand the cross-file model, then
use targeted sections to go deep on the packet. This avoids local optimizations that contradict
another facet or shared-substrate section.

This means a high-value conformance packet can be pulled before a lower-value shared-substrate
packet, even if both live near each other in the matrix.

## Completion Rule

A packet is complete only when its results section records:

| Required result | Why it matters |
| --- | --- |
| Files changed | Makes review concrete. |
| Source anchors checked | Prevents invented contract claims. |
| Decisions recorded | Shows whether the packet created or resolved design choices. |
| Remaining work | Keeps unfinished scope visible. |
| Checks run | Separates verified work from unverified analysis. |
| Blockers or decision points | Prevents hidden owner decisions. |

Completed packets can be moved out of the active table later or marked complete in place. Do not
delete a packet unless the completed work is captured in an owning spec, review note, or completed
packet archive.

## Managed Lane Object

A Lane is its own managed object. It is not a ticket, project, or agile board. A real board is a
ticket view like Jira or Asana. A Lane coordinates ordered work or tracking membership without
owning the member tickets.

Lane records use the following fields:

| Field | Meaning |
| --- | --- |
| `lane_id` | Stable Lane identity. |
| `lane_key` | Human-facing Lane key. |
| `title` | Concise display label for the Lane. |
| `description` | Durable statement of the Lane's intention and goal. |
| `lane_kind` | Required Lane kind: `assignment` or `tracking`. |
| `owner_principal` | Optional coordinator or responsible contact. It does not imply ticket ownership or assignment. |
| `lane_status` | Current coordination state. |
| `lane_tickets` | Ordered ticket ids on public surfaces; internal storage may keep sparse order metadata. |
| `active_ticket_id` | Ticket currently being worked, or null. |
| `status_report` | Prose status written by the Lane owner or agent. |
| `reviewer_feedback` | Prose feedback written by the arbiter or reviewer. |
| `updated_at` | Last update timestamp. |
| `updated_by` | Actor that last updated the Lane. |

`lane_status` is one of: `idle`, `ready`, `working`, `waiting_for_review`,
`feedback_available`, `waiting_for_decision`, `blocked`, `paused`, or `closed`.

An `assignment` Lane is an executable work-routing queue. A ticket may belong to at most one
non-closed assignment Lane. Ticket assignee and ticket status remain independently managed by the
Tickets facet.

A `tracking` Lane is an ordered coordination set. Tickets may appear in multiple tracking Lanes and
in one assignment Lane at the same time.

`lane_tickets` is an ordered list of ticket ids on public surfaces. Internal storage may retain
sparse order metadata, but public mutation and read surfaces do not expose numeric ranks. Lane
membership must not carry relation roles, `added_at`, or `added_by`. Removing a ticket from a Lane
deletes that association. Deleting a closed Lane deletes only the Lane coordination record and its
membership list; it never deletes tickets, mutates ticket status, erases ticket history, or removes
ticket-owned relations. Ticket lifecycle state stays on tickets.

The only Lane prose fields are `status_report` and `reviewer_feedback`. Rich ticket fields, large
rich text, canonical ticket fields, ticket projections, and ticket schema validation stay outside
this Lane contract.

### Coordination boundary (what belongs on the ticket, not the Lane)

A Lane record holds only coordination state: `owner_principal`, `lane_status`, `active_ticket_id`,
the ordered `lane_tickets` list, the two prose fields (`status_report`, `reviewer_feedback`), and the
update metadata (`updated_at`, `updated_by`). Everything an agent produces while working -- evidence,
source anchors, decisions, decision records, questions, blockers, result summaries, and review
findings -- belongs on the **active ticket** through typed ticket fields or comments, never on the
Lane. `status_report` and `reviewer_feedback` are short prose pointers to that ticket state, not a
place to store it.

This boundary is enforced, not advisory: the Lane document uses `deny_unknown_fields`, so ad-hoc
coordination fields (for example `decision_id`, `decision_resource`, `added_by`) cannot be persisted
through the Lane APIs -- a write carrying them fails with `INVALID_ARGUMENT`. The equivalent decision
detail is recorded on the ticket instead (e.g. a `decisions/<ticket>/<name>` resource or a typed
ticket field). Agents that need to record a question or closeout detail put it on the active ticket
and leave only a one-line pointer in `status_report`.

## Lane Promotion Across Public Surfaces

Lane management promotes only after the shared Lane model is source-backed. Public surfaces must
share one Lane contract rather than defining per-surface variants.

Current source backs the shared Lane model in `uldren-loom-lanes`. Lane records persist in the
document facet's `lanes` collection through a reusable boundary rather than ad hoc board documents.
The model validates the exact Lane fields above, the closed `lane_status` set, ordered ticket-id
membership, active-ticket membership, `assignment` versus `tracking` membership semantics, and the
two prose fields.

Promotion order:

1. Add the reusable Lane model and persistence boundary with validation for the fields in the
   Managed Lane Object section.
2. Add local CLI and MCP operations over that shared model.
3. Add hosted REST and JSON-RPC routes with the same request and response shapes.
4. Add behavioral conformance that covers local, MCP, hosted REST, and hosted JSON-RPC.
5. Promote remote protocol, C ABI, and language bindings only after the shared local and hosted
   semantics are conformance-backed.

Target surface matrix:

| Surface | Target Lane capability | Promotion requirement |
| --- | --- | --- |
| Shared model | Create, get, update status/report fields, list tickets, set active ticket, add/remove/reorder ticket membership, and read reviewer feedback. | One source-backed model validates `lane_status`, `lane_kind`, optional coordinator ownership, exact Lane fields, ordered ticket-id membership, assignment exclusivity, and the two prose-field limit. |
| CLI | Human-facing `loom lanes` commands for Lane lifecycle, status reports, active ticket, and ordered membership. | CLI requires `--kind assignment|tracking`, keeps owner optional, delegates to the shared model, and exposes machine-readable output for automation. |
| MCP | Tool surface for agents to read assigned Lane state, update `status_report`, move to waiting/review states, and manage Lane ticket membership. | MCP tools preserve ticket lifecycle boundaries and do not add relation roles or membership metadata. |
| Hosted REST and JSON-RPC | Served Lane operations equivalent to MCP and CLI. | Served routes use the same optimistic-concurrency roots as local writes and return the same Lane summary shape. |
| Remote protocol | Remote Lane methods or explicit unsupported capability reporting. | Do not silently proxy through host-only tools; remote parity requires conformance rows before advertising support. |
| C ABI and bindings | Stable Lane structs and functions in generated or hand-written binding surfaces. | ABI and binding promotion waits until the Lane model, route names, request shapes, and conformance vectors are stable. |
| Conformance | Cross-surface Lane parity suite. | Cover create/update/read/reorder, invalid status rejection, membership replacement/removal, active-ticket consistency, and prose-field ownership behavior. |

Lane management is not a ticket relation surface. Lane membership uses the ordered
`lane_tickets` list; ticket-owned relations remain the ticket profile's relationship mechanism.
Ticket list filters that accept a Lane id join against Lane records and normalize each
`lane_tickets.ticket_id` as either a primary ticket key or an internal ticket id. Ticket fields such
as `lane_owner` and `queue_lane` are not required for Lane membership and must not be treated as the
canonical assignment source.

## First-Class Ticket Board Object

A Ticket Board is its own managed coordination object. It is not a ticket, not a Lane, and not an
app-only projection. A Board groups and presents Tickets for planning, triage, review, and imported
Jira/Asana/Notion/Redmine board views. Lanes remain assignment and tracking queues; Boards remain
planning and review views.

Current source contains a model-only `TicketBoard` inside ticket profile snapshots
(`loom-tickets::model::TicketProfileSnapshot.boards`). That model has `board_id`, `project_id`,
`mode`, and `columns`, where `mode` is `status_mapped` or `manual`, and each column has
`column_id`, `name`, `mapped_statuses`, and optional `wip_limit`. That shape is useful evidence but
is not first-class enough: it has no board key, name, description, filter scope, swimlanes, ordering
policy, card placement state, owner metadata, lifecycle operations, MCP tools, CLI commands, IDL
methods, hosted routes, C ABI, or binding surface.

The first-class Board schema uses these records:

| Record | Fields |
| --- | --- |
| `TicketBoard` | `board_id`, `board_key`, `name`, `description`, `project_id`, `scope`, `board_kind`, `columns`, `swimlanes`, `ordering_policy`, `card_display`, `owner_principal`, `coordinator_principal`, `board_status`, `updated_at`, `updated_by` |
| `BoardScope` | `project`, `filter`, or `manual_set`; a filter scope stores a 0061 predicate root or named saved query reference, not ad hoc app JSON |
| `BoardKind` | `status_mapped` or `manual` |
| `BoardColumn` | `column_id`, `name`, `mapped_statuses`, `wip_limit`, `hidden`, `rank` |
| `BoardSwimlane` | `swimlane_id`, `name`, `predicate`, `rank`; swimlanes are optional and are display grouping, not ticket ownership |
| `BoardOrderingPolicy` | `rank_token` plus deterministic `ticket_id` tie-break; ordering is per board and per column |
| `BoardCardDisplay` | selected ticket fields and badges to render on cards; display config never adds ticket fields |
| `BoardCardPlacement` | `board_id`, `ticket_id`, `column_id`, `rank_token`, optional `swimlane_id`, `updated_at`, `updated_by` |

`board_status` is one of: `active`, `archived`, or `deleted`. Delete is an audited tombstone: the
Board id, key, history, and import identity stay readable for references, while default list calls
hide deleted boards.

## Board Modes and Membership Semantics

`status_mapped` Boards derive card membership and column placement from Ticket status or status
category. A card move across status-mapped columns is a Ticket lifecycle transition and must use the
same workflow validation, optimistic-concurrency root, expected observed source status, and
authorization path as `tickets_update`. Board-local rank can still be stored per column, but the
column itself is not authoritative unless the corresponding Ticket status transition succeeds.

`manual` Boards store explicit Board membership and placement in `BoardCardPlacement`. Manual
columns have no workflow semantics. Moving a card between manual columns writes Board placement only
and must not invent fake Ticket statuses or workflow states. A Ticket may be on multiple manual
Boards with independent placements.

Both Board modes use sparse rank tokens or another deterministic order-maintenance structure. Board
views sort cards by `(rank_token, ticket_id)` inside each column and swimlane. Rank compaction may
rewrite Board rank tokens without changing Ticket identity, Ticket status, or Board membership.

Completed Tickets are hidden by default in Board projections. A Board read may include terminal
Tickets only when the caller sets `include_completed`. Explicit status filters override the default
visibility rule so a caller can request a terminal status intentionally.

## Board Promotion Across Public Surfaces

Board management promotes only after the shared Board model is source-backed. Public surfaces must
share one Board contract rather than defining per-surface variants.

Promotion order:

1. Add the reusable Board model and persistence boundary with validation for the fields in the
   First-Class Ticket Board Object section.
2. Migrate model-only `TicketProfileSnapshot.boards` records into first-class Boards at profile read
   or import time. Existing `status_mapped` and `manual` mode tags remain valid migration inputs.
3. Add local create/get/list/update/delete, column configure, card move, card reorder, and membership
   operations over the shared model.
4. Add ticket listing support for `--board`, with stored Board order by default and the same
   incomplete-default plus `include_completed` semantics described above.
5. Update MCP, hosted REST, JSON-RPC, IDL, remote client/server, C ABI, and language bindings only
   after the shared Board model and local operations are conformance-backed.
6. Update Studio Board app and import/export projections to read first-class Board ids, not app-only
   board data or Lane membership.

Target surface matrix:

| Surface | Target Board capability | Promotion requirement |
| --- | --- | --- |
| Shared model | Create, get, list, update metadata, archive/delete, configure columns/swimlanes/display, move cards, reorder cards, and read Board projection. | One source-backed model validates Board mode, scope, column ids, status mappings, rank tokens, optional swimlanes, display fields, tombstone state, and update metadata. |
| CLI | Human-facing `loom tickets board ...` or dedicated `loom boards ...` commands selected by the implementation ticket. | CLI delegates to the shared model and must not create a partial CLI-only Board feature. |
| MCP | Agent-facing board tools with typed fields, including get/list/create/update/delete, column configure, card move/reorder, and board-scoped ticket list. | MCP schemas use typed request fields instead of opaque maps where avoidable, preserve ticket lifecycle validation, and return first-class Board ids. |
| Hosted REST and JSON-RPC | Served Board operations equivalent to local and MCP operations. | Served routes use the same optimistic-concurrency roots as local writes and return the same Board summary and projection shapes. |
| Remote protocol | IDL-backed Board methods or explicit unsupported capability reporting. | Do not proxy through app-only tools; remote parity requires conformance rows before advertising support. |
| C ABI and bindings | Stable Board JSON or canonical-CBOR functions across generated and hand-written binding surfaces. | ABI and binding promotion waits until route names, request shapes, migration behavior, and conformance vectors are stable. |
| Studio Board app | Read and manipulate first-class Boards. | The app reads Board records and Board projections, writes through Board and Ticket tools as appropriate, and never treats Lane membership as the canonical Board model. |
| Imports and exports | Jira boards/status columns, Asana sections/manual boards, Redmine and Notion board equivalents map into first-class Boards. | Imports preserve source ids, Board mode, column order, card order, status mappings, and fidelity gaps without polluting Ticket workflow semantics. |
| Workgraph telemetry | Board-read facts refer to first-class `board_id`. | `BoardReadObserved` correlation ids and payloads identify the durable Board, not an app instance or transient projection. |
| Conformance | Cross-surface Board parity suite. | Cover schema decode/validation, migration from model-only boards, status-mapped card move validation, manual card movement, board ordering, include-completed behavior, pagination, import fixture listing/readback, and app data projection. |

Board management is not a Lane surface. Lane membership remains `lane_tickets`. Board membership and
card placement belong to Board records. Ticket-owned relations remain relationship edges. Ticket
status remains the Tickets lifecycle field.
