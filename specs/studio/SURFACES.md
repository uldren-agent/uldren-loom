# Studio Surfaces - Human Experience Layer

**Status:** Target design. **Version:** 0.1.0-target.
**Capability:** `surfaces` (builds on `mcp-apps` from 0043).

This document maps the planned human experience onto MCP Apps, elicitation, and prompt handoff. The
substrate (`0061`) and Studio profiles (`SLACKISH`, `DRIVEISH`, `JIRAISH`, `PAGES`, `MEETINGS`) define
data and tools; this document defines what people see and touch. The app *mechanism* - `ui://` HTML
resources, multi-file app directories, `apps.*` authoring tools, the app data channel - is owned by
`0043-mcp-serving-surface.md` and is not restated here.

## 0. Current Source Boundary

Current source backs the shared Studio surface record layer in `loom-substrate::surfaces`.
Source-backed records are canonical CBOR shapes for app definitions, elicitation requests,
elicitation responses, prompt handoffs, and render frames pinned to an `as_of` root. App definitions
validate `ui://` resource URIs and deduplicated projection/tool/schema/subscription references.
Elicitation requests require a schema reference and schema digest, responses cannot remain pending,
prompt handoffs carry the prompt digest plus source entity refs, and render frames carry cursor and
staleness state.

The MCP app resource mechanism remains owned by 0043 and is source-backed in `loom-mcp`: valid
multi-file apps are stored under `/.loom/facets/mcp/apps/{app-name}/`, surfaced as `ui://`
resources, and managed by the `apps.*` authoring tools. The source-backed Surfaces runtime boundary
also includes dynamic launcher tools with `_meta.ui.resourceUri`, template-backed rendering through
`loom-templates`, the app-only `ask_record` write path used by the internal Decisions app, app-only
`apps_call_tool` dispatch through visible MCP tools, MCP app resource subscriptions, and `mcp-apps`
capability advertisement. Host browser rendering and upstream iframe bridge certification remain
host-conformance work owned by 0043, not proof the Loom server can produce by itself.
Profile-specific app bundles for Tickets, Pages, Chat, Drive, and Meetings are source-backed at the
current promoted-tool boundary and are visually verified by `just verify-apps`.

Current source also backs the app catalog at the contract level. `core_surface_catalog` returns
deterministic app definitions for Ticket Details, Board, Spec Document Viewer, and Directed Graph.
`surface_app_catalog` extends that set with Roadmap, Search Palette, Changes Inbox, Sprint Planner,
Backlog Triage, Decision Log, Audit Timeline, Dashboards, Revision Diff, Mind Map, Canvas, and
Diagram Editor. Each entry carries a `ui://{workspace}/mcp/apps/{app-id}` resource, projection refs,
read/write tools, elicitation schemas, prompt-handoff refs, and change subscription refs. The catalog
is exposed through `loom studio surfaces catalog <workspace> --set core|all|meeting-memory` for
source-backed CLI inspection and through `studio_surface_catalog_json` in the IDL, C ABI, C++ helper,
Node, Python, iOS/Swift, JVM, Android/KMP, React Native, and WASM bindings. It is not yet an app
bundle generator; it does not write HTML files, produce visual verification renders, execute app
actions, or render visualizations.

The first core runtime bundles are source-backed. `loom-mcp` ships built-in template-backed apps for
Directed Graph, Chat Channel, Chat Thread, Chat Tasks, Chat Presence, Chat Handoffs, Ticket Details,
Board, Roadmap, Sprint Planner, Backlog Triage, Dashboards, Spec Document Viewer, Mind Map, Canvas,
and Diagram Editor at their catalog URIs.
They are listed by app inventory, exposed as valid MCP resources, launchable through dynamic app
launcher tools, support workspace-bound URI elision, and read their bundled files through the same
app inspection path as other binary-sourced apps. Directed Graph renders graph nodes and edges derived
from the Studio catalog plus Meeting Memory catalog. The Chat bundles render through `loom.chat` with
workspace/profile, app-definition, channel, message, thread, task, handoff, presence, cursor, and
event-summary data from channel-addressed app URIs. Chat Channel accepts
`ui://{workspace}/mcp/apps/chat-channel/channel/{channel_id}`; Chat Thread accepts
`ui://{workspace}/mcp/apps/chat-thread/channel/{channel_id}/thread/{thread_id}`; Chat Tasks, Chat
Presence, and Chat Handoffs accept channel-scoped routes. The ticket and planning bundles render through
`loom.tickets` with workspace/profile, app-definition, ticket, ticket-history, reference, and lane
data. Ticket Details accepts `ui://{workspace}/mcp/apps/ticket-details/ticket/{ticket_id}`. The
ticket app payload advertises only source-backed tools (`tickets_get`, `tickets_history`,
`tickets_update`, selected ticket relation tools, and lane coordination tools where applicable), even
where the broader catalog records target transition/rank semantics. The Pages bundles render through
`loom.pages` with workspace/profile, app-definition, space data, and selected page/history or
structure data from instance-addressed app URIs. Document Viewer accepts
`ui://{workspace}/mcp/apps/document-viewer/page/{page_id}`; Mind Map, Canvas, and Diagram Editor
accept `ui://{workspace}/mcp/apps/{app}/structure/{structure_id}`. Document Viewer publishes the
selected page through the app-only `apps_call_tool` bridge. Ticket Details prepares `tickets_update`
through the same bridge. Chat Channel prepares `chat_post_message`, and Chat Presence prepares
`chat_set_presence`. Mind Map, Canvas, and Diagram Editor call the bridge for structure node add,
move, and link actions. Browser-host visual verification through `just verify-apps` is source-backed
for VCS, Decisions, Directed Graph, Chat Channel, Chat Thread, Chat Tasks, Chat Presence, Chat
Handoffs, Drive Browser, Drive Preview, Drive Sharing, Drive Conflicts, Drive Retention, Ticket
Details, Board, Roadmap, Sprint Planner, Backlog Triage, Dashboards, Document Viewer, Mind Map,
Canvas, and Diagram Editor representative fixtures.

The shared built-in app shell CSS is source-backed for MCP Apps shipped from `loom-mcp`. VCS,
Decisions, and Directed Graph render the same binary-sourced shell through `loom.app_shell.css`,
while retaining app-specific layout CSS in each bundle. This proves the reusable shell boundary
without changing user-authored app storage or requiring a separate resource fetch.

The shared app-only tool bridge is source-backed through `apps_call_tool`. It validates the caller's
visible app resource URI, rejects app-only and app-launcher recursion, resolves the requested visible
tool through the normal MCP tool registry, and returns the called tool plus its structured result.

The Ticket and planning app bundles are not yet complete workflow editors. Current source exposes
source-backed ticket details, board-style grouping, roadmap/date grouping, sprint grouping, backlog
triage grouping, dashboard metrics, selected-ticket history, selected-ticket references, lane
summaries, and app-only `tickets_update` dispatch. Drag rank semantics, workflow guard
elicitations, comments, attachments, and advanced planning interactions remain target work.

The Chat app bundles are not yet complete Slack-class clients. Current source exposes source-backed
channel and thread views, message rendering, task state, handoff and agent-invocation summaries,
presence display, cursor/unread display, event summaries, and app-only `chat_post_message` and
`chat_set_presence` dispatch. Attachment byte handling and durable notification delivery integration
remain target work because those promoted source APIs are not present yet.

The Drive app bundles are not yet complete desktop-sync clients. Current source exposes
source-backed folder browsing, file preview bytes, file version summaries, conflict records, share
grants, retention pins, lease-tool descriptions, and app-only `drive_create_folder` and
`drive_create_upload` dispatch. Folder instances use
`ui://{workspace}/mcp/apps/drive-browser/folder/{folder_id}`; file-preview instances use
`ui://{workspace}/mcp/apps/drive-preview/file/{file_id}`. OS placeholder and hydration controls
remain target app work because the promoted MCP surface does not yet expose app-callable hydrate,
dehydrate, or worker-plan tools.

The Pages app bundles are not yet complete document editors. Current source exposes public
`pages_list` and `structures_list` readers and the bundles receive those arrays through `loom.pages`.
The Document Viewer renders selected-page backlinks from `substrate_refs` and can publish the
selected page. The structure bundles can add, move, and link structure nodes. This is the accepted
Pages app editor boundary.

`meeting_memory_surface_catalog` adds the Meeting Memory app set owned by `MEETINGS.md`: Meeting
Details, Memory Graph, Extraction Review, Meeting Search, Import Coverage, and Access Audit. These
entries use the same app-definition contract but remain profile-scoped rather than part of the base
catalog. The source-backed MCP Apps for these entries render through `loom.meetings`, including
meeting list/detail, projection output, extraction review, import-coverage status, and
access-audit status. Import-run browsing, export workflows, and a Meetings-specific audit-log
projection remain target controls until promoted tools exist.

`crates/loom-conformance` covers this source-backed boundary in the `substrate-model` vector suite:
core app definitions, the full surface app catalog, the Meeting Memory catalog, elicitation request
and response records, prompt handoff records, and render frame records all round-trip through their
canonical encodings.

## 1. Interaction Model

Every app follows one loop:

1. The app renders a **projection** (0061 §8 view or profile projection) fetched through profile or
   substrate read tools, executing as the resolved principal.
2. User actions call profile **write tools** (`tickets.transition`, `pages.publish_draft`); every
   write passes the policy enforcement point and becomes a sequenced operation.
3. When a write needs a structured human decision the flow cannot infer, the server raises an
   **elicitation** with a schema; the response becomes part of the durable operation.
4. When the user's intent is open-ended rather than structured, the app **hands off a prompt** to
   the assistant chat instead of growing its own UI ("ask the assistant to split this ticket").
5. Resource **subscriptions** wake the app when the underlying root advances; the app refetches from
   its cursor and re-renders. Notifications are wakeups; cursors are truth (0035).

Rule of thumb: schemas become elicitation; free intent becomes prompt handoff; everything visible is a
projection, never a bespoke query path.

Two consequences of the versioning model (0061 §9-§10) shape every app: first, any rendered view is
pinned to a root, so apps display an `as_of` badge when their subscription is stale rather than
pretending to be live; second, history is not a separate feature - the revision slider, time-lapse
scrubbing, and baseline-vs-current comparisons are the same projection evaluated at two roots.

The experience layer exists because the substrate is structured where markdown is not. A markdown
workflow gives an assistant text to edit and a human text to read; this layer gives both of them the
same typed objects through different doors - the assistant through tools, the human through apps -
and every mutation from either door is the same sequenced, policy-checked, attributable operation.
Nothing an app can do exceeds what the assistant can do, and vice versa; the app is ergonomics, not
authority (the same rule the profiles pin for WebSocket).

## 2. Elicitation Flows (worked end to end)

**Board transition with workflow guards.** User drags ISSUE-9 from In Progress to Done on the Board
app. The app calls `tickets.transition`. The workflow edge requires `resolution` and a code-review
link, which are absent. The server responds with an elicitation whose schema has exactly those two
fields; the host renders the form in place; the user fills it; the transition operation commits with
the fields attached; the sequencer validates the edge (JIRAISH §7.3); the board's subscription fires
and the card settles into the Done column. If a concurrent transition already moved the issue, the
revalidation rejects, and the app shows the visible rejected-operation record instead of silently
snapping the card back.

**Conflict resolution.** The Changes Inbox app surfaces a 0061 §6 conflict record: two assistants
set `priority` from the same base version. The app raises an elicitation: message names both values,
authors, and timestamps; schema is `{priority: enum[P0..P3], record_note: boolean}`. The resolution
becomes an operation that closes the conflict record; both prior operations remain in history.

**Agent-authored content approval.** An agent drafts a page update (PAGES draft state). The
Document viewer shows the draft as a pending revision with a body-model diff against the published
revision. Elicitation: `{choice: enum[publish, edit-first, discard], watch_page: boolean}`. Publish
becomes `pages.publish_draft` under the *user's* principal with the agent identity block
preserved in the envelope.

## 3. App Catalog

| App | What it renders | Data sources | Writes / elicitations |
| --- | --- | --- | --- |
| **Ticket Details** | One issue: fields, description body, annotations, revision history, links, audit | issue projection, revision index (0061 §9), `substrate_refs` | field edits, transitions (guard elicitations), comments, link add |
| **Board** | Kanban columns by workflow state, rank-ordered | board projection, order tokens | drag = `tickets.rank` / `tickets.transition`; missing-field elicitations |
| **Roadmap / Gantt** | Initiatives and epics on a timeline, dependencies, milestones, progress rollup, critical path | planning projection (JIRAISH §21), graph projection, calendar facet | timeframe drag, dependency draw, milestone edit; confirm-reschedule elicitation when a move cascades |
| **Spec / Document / Plan viewer** | Page body, TOC, inline comments, backlinks panel, revision slider with diff view | document projection, revision index, `substrate_refs`, annotations | draft edit, publish (approval elicitation), inline comment, resolve |
| **Directed Graph viewer** | The Obsidian-style navigable graph: entities as nodes, typed edges (blocks, links_to, mentions, parent_of), filterable by facet/type/status, force-directed d3 | graph projection + `substrate_refs` | click-through navigation; "explain this cluster" prompt handoff |
| **Search palette** | Omnibox over everything; typed, context-enriched hits grouped by profile | `substrate_search` (falls back to profile search when scoped) | open-in-app; "refine with assistant" handoff |
| **Changes Inbox** | What happened since my cursor: operations, conflicts, mentions, review requests | `substrate_changes`, conflict records, annotations | acknowledge (cursor advance), conflict elicitations |
| **Sprint planner** | Backlog vs sprint capacity, points rollup, DoR gate flags | issue table projection, order tokens | drag into sprint; over-capacity confirm elicitation |
| **Backlog triage** | Queue-driven: one untriaged item at a time | queue facet + issue projection | classify/assign/close; each action an elicitation-shaped form |
| **Decision log** | Ledger of decision operations, filterable by affected entity | ledger projection, `substrate_refs` | record decision; reverse-decision links |
| **Audit timeline** | Who/what/when across a scope, agent actions flagged | audit projection, operation log | read-only; export elicitation where policy requires |
| **Dashboards** | Burndown, velocity, cycle time, cumulative flow | dataframe over issue table + op history | read-only; drill-through to Board/Ticket |
| **Revision diff** | Any entity at version A vs B | revision index + 0003d / body-model diff | restore-revision (confirm elicitation) |
| **Mind map** | A PAGES structure (kind: mindmap) as an editable radial tree; nodes link to pages and tickets | structure projection (PAGES §21), `substrate_refs` | node add/move/link; "decompose this branch into tickets" prompt handoff |
| **Canvas / whiteboard** | Freeform spatial arrangement of typed nodes (pages, tickets, notes) with position as view state | structure projection + per-user layout in app data channel | node placement; linking draws real graph edges |
| **Diagram editor** | Data-backed diagrams (flowchart, sequence, architecture) where shapes bind to entities; AI edits the data, every bound diagram re-renders | structure projection, graph facet | shape/edge ops are graph ops; free-drawing stays view-local until bound |

## 3a. Worked Example: Tool Input To Data Model To Rendered Graph

The Directed Graph viewer renders nothing special - it renders the graph projection. The question is
whether the *write* path that feeds it stays intuitive for an assistant. Worked through for ticket
creation (tool shape illustrative, not a design decision):

```json
tickets.create {
  "project": "LOOM",
  "type": "story",
  "summary": "Predicate grammar for columnar_select",
  "description_md": "...",
  "parent": "LOOM-40",
  "links": [
    {"type": "blocks", "target": "LOOM-52"},
    {"type": "implements", "target": "page:substrate-predicates"}
  ],
  "sprint": "2026-Q3-S2",
  "points": 3
}
```

Design rules that keep this assistant-intuitive while feeding every visualization:

1. **Flat, domain-named scalars.** `summary`, `points`, `sprint` - no envelope fields, no facet
   vocabulary. The assistant should not know rank tokens exist; omitting `rank` appends to the
   backlog tail.
2. **Relationships as typed reference arrays.** `links: [{type, target}]` is the one nested shape,
   and it is exactly an edge list - the tool input *is* the graph increment. Targets accept aliases
   (`LOOM-52`) or cross-profile refs (`page:substrate-predicates`); the server resolves aliases to stable ids at
   sequence time.
3. **The tool collects increments, not the model.** The graph viewer renders from edges derived over
   *all* operations - links from this tool call, `parent` edges, mention edges extracted from
   `description_md`, `implements` edges added later from the page side. The assistant never has to
   assemble or even know the full node-link model; no single write is responsible for the render
   being complete.
4. **One write, one transaction.** The call expands server-side into row + node + edges + ledger
   entry under `substrate_transact` semantics. The assistant sees one intuitive tool; the substrate
   sees atomic operations.
5. **Round-trip symmetry.** `tickets_get` returns the same shape the create accepted (plus derived
   and identity fields), so an assistant can read-modify-write without translating between an input
   and an output vocabulary.

The test for every future tool: can the assistant produce the input from a user sentence without
consulting the data model, and can the visualizations render from the resulting operations without
consulting the tool? When both hold, the tool surface and data model are properly decoupled.

## 4. Visualizations Unlocked by the Substrate

These exist because the substrate is structured where markdown is not:

- **Page graph (Obsidian-class, but typed).** Obsidian infers links from markdown; here edges
  carry types and status, so the graph can color by health, filter "only blocking edges", and
  animate over time by replaying operations.
- **Blast radius.** Select a node; `graph_reachable` shades everything transitively affected if it
  slips. The gantt critical path is the same query on the planning graph.
- **Workflow Sankey.** Transition operations aggregate into flow ribbons between states - where work
  stalls is visible as ribbon width; cycle-time outliers link back to tickets.
- **Scope treemap.** Epics sized by points or issue count, colored by status rollup; scope creep is
  the treemap growing between two `as_of` roots.
- **Activity heatmap.** Operation counts by scope × day from the op log; agent vs human activity
  split by `actor_kind`.
- **Burnup with scope annotations.** Because scope changes are operations, the burnup chart can pin
  exactly which decision (ledger ref) moved the goalposts.
- **Dependency structure matrix (DSM).** Issue-link adjacency matrix; cycles highlight as blocks -
  unbuildable plans are visible before anyone starts.
- **Time-lapse.** Any of the above scrubbed across sequence numbers: replay the quarter in ten
  seconds. Free because history is operations, not file states.
- **Semantic map (deferred to 0017/0040).** Pages and tickets as an embedding scatter; clusters
  reveal duplicate work and undocumented areas.

## 5. Queued Design Work

The source-backed app layer covers the promoted-tool boundary. The remaining questions below apply
when a future surface promotes a new app workflow or host capability.

1. App-to-assistant prompt handoff contract (0043 app data channel vs a dedicated tool).
2. Which projections are precomputed views (0061 §8) versus app-side queries, per app.
3. Elicitation schema versioning and reuse across profiles (shared form registry or per-profile).
4. Offline/stale rendering rules when a subscription is disconnected (show `as_of` root badge?).
5. The conformance story for apps: fixture-backed visual render verification through
   `just verify-apps` per 0043 example fixtures.

## 6. Recommended Shape and Sequencing

All app contracts in this document are long-term decisions; there is no "v1 contract" to be replaced
later. Sequencing is an implementation concern only: build Ticket Details, Board, Spec/Document
viewer with revision slider, and the Directed Graph viewer first, against the dogfood planning
store, because those four exercise every substrate primitive (revisions, refs, changes, conflicts,
order tokens, views) and force the §2 elicitation flows to be real. The remaining catalog follows on
the same contracts.
