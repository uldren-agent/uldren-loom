# 0063 - Decisions App and Structured Decision Elicitation

**Status:** Draft (first slice source-backed in `loom-mcp`). **Version:** 0.1.2.
**Capability:** builds on `mcp-apps` (0043 §2.2); tool area `ask`.

An assistant that needs a human decision should not bury it in prose or squeeze it into a host's
native question widget. This spec defines the Ask surface: an agent opens a set of structured
decision questions as an MCP App (the Decisions app), the human answers in a purpose-built view,
and the agent blocks until the answers land. The question structure is the decision-point contract
this repository already mandates for agents (AGENTS.md "Decision visibility mode"): Question,
Context, Examples, Options, Recommendation. The count of questions is unbounded by design; host
question tools that cap the number of questions or the room to read them are exactly what this
surface replaces.

Naming is deliberately asymmetric so agents pick the right entry point. The one tool an agent
reaches for is `ask_questions`; the wait tool is `ask_answers`; the write-back tool `ask_record` is
app-only and hidden from the model's tool list; and the app itself is named Decisions
(`internal/decisions`) so neither the app nor its launcher competes with the `ask.*` verbs in a
tool listing.

## 1. Interaction model

One loop, three tools, one app:

1. `ask_questions` (agent) validates and durably records the question set, then returns the
   standard app-launch payload with `_meta.ui.resourceUri` pointing at the Decisions app instance
   for this ask. It returns immediately: a tool that blocked here would deadlock, because the host
   only sees the resource URI (and thus renders the app) when the call returns.
2. The host reads the `ui://` resource; template processing (0043) renders the pending questions
   into the app HTML through the `loom.ask` binding.
3. `ask_answers` (agent) blocks on the ask id until the ask leaves `pending`, then returns the
   answers. Timeouts return `status: "timeout"` so the agent can re-wait; cancellation aborts the
   poll cleanly.
4. `ask_record` (app, over the host bridge `tools/call`) records the human's answers, flips the ask
   to `answered` or `aborted`, and wakes the waiter.

The split between 2 and 4 follows the 0043 data-channel ordering: reads render through templates and
resource re-reads; the write back is a PEP-gated tool call, for which the bridge is the sanctioned
channel (0043 §2.2).

## 2. Storage contract

Asks are workspace state, recorded where every other facet write goes so they version, sync, and
audit like anything else.

- **Workspace:** the `workspace` argument of the ask tools; workspace-bound servers elide it from
  the schemas and inject the bound workspace at dispatch, like every other tool.
- **Facet:** Document (`FacetKind::Document`). The `ask` tool area maps to the Document facet for
  policy: `ask_answers` requires Document read, `ask_questions`/`ask_record` require Document
  write.
- **Collection:** `loom.ask` (reserved by convention; the `loom.` prefix marks host-owned
  collections, following the `.config` precedent in the kv facet). The collection name is a durable
  data contract and keeps the `ask` spelling independently of app and tool display names.
- **Format:** JSON documents (UTF-8 `serde_json` bytes). Two kinds of document ids:
  - `ask-{created_ms}-{seq}`: the archive document, one per ask, holding its full lifecycle.
  - `current`: a pointer document, a copy of the most recently begun ask. The bare Decisions app
    resource renders this document. `ask_record` refreshes it only when it still points at the
    submitted ask.

The archive document shape:

```json
{
  "id": "ask-1751687000000-0",
  "status": "pending | answered | aborted",
  "created_ms": 1751687000000,
  "submitted_ms": 1751687100000,
  "questions": [
    {
      "question": "Which storage facet should asks use?",
      "context": "Asks must be durable and auditable.",
      "examples": "Document keeps JSON readable; kv is byte-oriented.",
      "options": [
        { "label": "document", "description": "JSON documents" },
        { "label": "kv", "description": null }
      ],
      "recommendation": "Document: JSON documents with string ids fit the ask shape.",
      "shape": "radio"
    }
  ],
  "answers": [
    {
      "question": "Which storage facet should asks use?",
      "status": "answered | skipped",
      "selected": ["document"],
      "text": ""
    }
  ]
}
```

`submitted_ms` is absent until the ask resolves. `answers` is index-aligned with `questions` and is
empty while pending. Because documents live in the working tree like any facet state, an ask is
durable across sessions immediately and becomes part of workspace history when the workspace
commits.

## 3. Question structure

Every question carries the decision-point fields:

| Field | Presence | Meaning |
| --- | --- | --- |
| `question` | required | The firm question that needs an answer. |
| `context` | desired | Why the question exists; the facts that make it answerable. |
| `examples` | desired | A worked example of the question plus context. |
| `options` | required for `radio`/`checkbox` | The concrete choices, each `{label, description?}`. |
| `recommendation` | desired | The asker's own recommendation with rationale; rendered below the options when present. |
| `shape` | required | `radio` (single choice), `checkbox` (multiple choice), or `text` (free input). |

Presence here is the wire contract (the JSON schema): a field marked desired is optional at the
wire and never causes an invalid-params rejection when absent. `recommendation` is desired, not
required: repository convention (AGENTS.md) expects agents in this repo to always provide one, but
the tool accepts questions without it and the app simply omits the callout. `shape` is explicit
rather than inferred. `options` are required and non-empty for `radio` and `checkbox`; for `text`
they are optional and render as one-tap suggestion chips that prefill the input. `ask_questions`
rejects unknown shapes, optionless choice questions, and empty question sets with invalid-params.

## 4. Tool contracts

All three tools live in the curated tool surface (0008 tool table, area `ask`, IDL interface
Document with no direct method projection, like the `apps.*` curated storage tools). Wire names
sanitize as usual (`ask_questions` advertises as `ask_questions`; dispatch reverses it).

**`ask_questions`** (write, model-visible) - input `{workspace, questions[]}`. Writes the archive
and `current` documents with `status: "pending"`, then returns the launch payload of the Decisions
app (`workspace`, `app`, `uri`, `name`, `description`, `processing`) extended with `ask_id`, where
`uri` is the ask's instance URI (§6). The result `_meta` carries `ui.resourceUri` (and the
deprecated flat `ui/resourceUri`) exactly like an `apps.launch.*` tool, so any MCP Apps host
renders it the same way.

**`ask_answers`** (read, model-visible) - input `{workspace, id, timeout_ms?}`. Polls the archive
document (400ms interval) until `status != "pending"`, then returns `{id, status, answers}`. The
default timeout is 600000ms, capped at 3600000ms; expiry returns `{id, status: "timeout",
answers: []}` without mutating the ask. Unknown ids are invalid-params, not a hang. The poll runs
inside the server's standard cancellation boundary, so a cancelled request stops waiting
immediately.

**`ask_record`** (write, app-only) - input `{workspace, id, answers[], aborted?}` where each answer
is `{index, status: "answered" | "skipped", selected?, text?}`. Rejects unknown ids, non-pending
asks (a second submit is an error), out-of-range indexes, and unknown statuses. Questions without a
submitted answer record as `skipped`; when `aborted` is true every answer records as `skipped` and
the ask resolves `aborted`.

`ask_record` is declared `_meta.ui.visibility: ["app"]`, the MCP Apps visibility surfaces already
used by app-only launchers: `tools/list` omits it for the model, while `tools/call` still
dispatches it, so the Decisions app calls it over the host bridge and the agent's tool list stays
free of a tool it should never pick. This is within the MCP contract (a server may accept calls to
tools it does not advertise); the practical constraint runs the other way, and is why
`ask_answers` stays model-visible: most hosts only let the model call advertised tools, so the tool
the agent must call to finish the flow has to be listed. Direct MCP clients and tests may still
call `ask_record` by name; it remains an ordinary PEP-gated tool, and nothing an app can do exceeds
what the assistant can do (the office-apps rule).

The ask id returned by `ask_questions` is the correlation key across the whole flow:
`ask_questions` mints it, the launch payload, the instance URI, and the stored documents carry it,
the rendered app embeds it, and `ask_answers`/`ask_record` address it. Concurrent asks are
unambiguous at every layer (§6).

## 5. The Decisions app

`internal/decisions` is the second binary-sourced internal app (0043 §2.2 internal hierarchy),
beside `internal/vcs`:

```text
/.loom/facets/mcp/apps/internal/decisions/_meta.md
/.loom/facets/mcp/apps/internal/decisions/index.html
```

`_meta.md` declares `name: Decisions`, `loom.processing: templates`, `ui.visibility: [model, app]`,
and display modes `inline` and `fullscreen`. Its compatibility launcher is
`apps.launch.internal.decisions`, deliberately outside the `ask` verb family. Template bindings
expose `meta.*` (as for every templated app) plus `loom.ask`:

```json
{ "workspace": { "...": "workspace summary" }, "current": { "...": "the ask document or null" } }
```

Rendering rules, matching how Claude-style hosts present questions:

- One card per question: ordinal, question, muted context, collapsible example, inputs, then the
  recommendation callout (rendered when present, visually distinct, below the options).
- `radio` renders radio inputs, `checkbox` renders checkboxes, `text` renders a textarea with
  optional suggestion chips.
- Every question has a Skip toggle; a skipped question dims and submits as `"skipped"`. Answering a
  skipped question un-skips it.
- A footer offers Submit answers and Abort. Submit records unanswered questions as skipped; Abort
  resolves the whole ask as `aborted`. After either, the card list is replaced with a read-only
  summary of what was recorded.
- Already-resolved asks render the summary; an absent ask renders an empty state. The app never
  renders editable inputs for a non-pending ask.

The app performs the standard MCP Apps readiness handshake and content-height size notifications
(same client half as the VCS app) and submits through bridge `tools/call` of `ask_record`, trying
the sanitized name first and falling back to the canonical dotted name.

## 6. Concurrency and app instances

State and tools are multi-ask safe: every ask has its own archive document and the id threads
through questions/answers/record. Hosts can run multiple app instances in one session; instances
are keyed by resource URI, and each ask renders as its own instance through an instance-addressed
URI:

```text
ui://{workspace}/mcp/apps/internal/decisions/{ask_id}
```

`ask_questions` returns this URI in both the launch payload and `_meta.ui.resourceUri`. URI parsing
recognizes the instance suffix on the Decisions app only (segment charset `[A-Za-z0-9._-]`, no
leading dot; invalid segments do not resolve; other internal apps accept no instance). Template
binding resolves `loom.ask.current` from that ask's archive document, so two `ask_questions` calls
produce two app instances rendering two distinct question sets, each with its own view, waiter, and
submit path. Resource subscriptions and delivery streams key by URI, so per-instance wakeups come
for free.

The bare URI (`ui://{workspace}/mcp/apps/internal/decisions`) stays valid and is what
`resources/list` and the `apps.launch.*` compatibility projection advertise: it renders the
`current` pointer, the most recently begun ask. An instance URI whose ask document does not exist
renders the app's empty state rather than erroring; unknown asks fail loudly at the tool layer
(`ask_answers`/`ask_record` reject unknown ids), not the render layer.

## 7. Security

Both ask writes and the read pass through the policy enforcement point via the engine facade, like
every curated tool. There is no bypass for the app: the bridge call lands in the same `tools/call`
dispatch, the same visibility check, and the same Document-facet ACL as an agent call; app-only
listing (§4) is presentation, not authorization. Read-only servers (writes disallowed) omit
`ask_questions` and `ask_record` and the flow is unavailable, which is correct: an ask that can
never be answered must not be opened.

## 8. Conformance

Source-backed today in `loom-mcp` server tests: the end-to-end flow (questions validates shapes and
optionless choice questions, launch payload carries `ask_id` and the instance URI, the rendered
resource contains the question text and pending status, record accepts answered and skipped answers
and rejects a second submit, answers returns the recorded values, unknown ids error, zero-timeout
waits report `timeout`), concurrency (two pending asks render distinct question sets at their
instance URIs while the bare URI follows the latest, the first ask's record leaves the second
pending, the second resolves `aborted` independently, path-escaping instance segments do not
resolve), tool-list visibility (`ask_record` is hidden from the model, `ask_questions` is not),
optional recommendation (an ask without one is accepted), plus the
internal-app inventory and resource listing counts covering both internal apps. Golden-render
vectors and host-level bridge certification follow the 0043 conformance posture: Loom certifies its
server-side surfaces; a host that proxies bridge `tools/call` certifies that in the hosted-protocol
suite.

## 9. Decision log

Decisions resolved (owner-confirmed):

1. **App kind: internal, binary-sourced.** The Decisions app ships in the `loom` binary under
   `internal/decisions` like `internal/vcs`, because per-call template bindings and the ask tools
   require host code regardless; a file-backed user app would add a generic launcher-input binding
   without removing the Rust work. User apps may still implement their own ask-like views against
   the same tools.
2. **Wait mechanic: two-phase blocking tools, not elicitation.** MCP elicitation blocks correctly
   but the host renders its own schema form, discarding the Question/Context/Examples/Options/
   Recommendation presentation. A single blocking launcher deadlocks (the host renders the app only
   after the tool returns). `ask_questions` (immediate) plus `ask_answers` (blocking) preserves
   both the custom view and the wait semantics.
3. **Shape is an explicit field.** `radio | checkbox | text` on every question; options required
   for the choice shapes, optional suggestion chips for text. Explicit beats structural inference.
4. **Storage: durable, Document facet, collection `loom.ask`, JSON.** Asks and answers are
   sequenced workspace state: auditable, versionable, and readable by the template binding through
   the same PEP-gated path as every read.
5. **Per-ask instance URIs (§6).** Concurrent asks render as separate app instances keyed by
   `ui://{ns}/mcp/apps/internal/decisions/{ask_id}`; the bare URI renders the latest ask. Chosen
   over auto-aborting the previous pending ask (blocks nothing, reuses URI-keyed subscription and
   delivery machinery) and over rejecting `ask_questions` while one is pending (blocks legitimate
   concurrent flows). Auto-abort remains available to hosts as policy: nothing prevents an agent
   from aborting its own stale asks through `ask_record` with `aborted: true`.
6. **Disambiguated naming and app-only listing.** An `ask.`-saturated tool list caused agents to
   call the wrong tool. The agent entry point is `ask_questions`; the wait tool is `ask_answers`;
   the write-back is `ask_record`, hidden from the model's tool list via `_meta.ui.visibility:
   ["app"]` (still dispatchable; §4 records why `ask_answers` must stay listed). The app and its
   launcher renamed to Decisions / `internal/decisions` so the launcher tool
   (`apps.launch.internal.decisions`) leaves the `ask` prefix entirely. The storage collection
   `loom.ask` is unchanged: it is a data contract, and renaming it would orphan recorded asks.
7. **Recommendation is desired, not required, at the wire.** A session was told the field was
   optional and got rejected; schema and behavior now agree with the softer reading. The repo's own
   agents remain expected to supply one (AGENTS.md decision format); the contract just no longer
   turns its absence into a failed call.

Decision points (open):

1. **Bridge-less host fallback.**
   - Question: when a host does not proxy bridge `tools/call`, should the app fall back to
     `ui/message` prompt handoff to deliver answers into chat?
   - Context: submit is a write and needs a tool call; 0043 keeps the bridge host-owned. Without
     it, the app can render but not submit (it surfaces the submit error today).
   - Examples: the app posts "Answers for ask-X: 1) document 2) Skip" as a user message; the agent
     then records them via `ask_record` itself.
   - Options: (a) no fallback (current); (b) `ui/message` fallback with agent-side recording.
   - Recommendation: (a) until a real bridge-less host matters; (b) is additive later and needs no
     contract change.
   - Consequence of deferring: none on bridge-capable hosts.
2. **Archive retention.**
   - Question: prune resolved ask documents, and if so when?
   - Context: every ask leaves one document in `loom.ask` forever; the collection is small but
     unbounded.
   - Options: (a) keep everything (current; history is the point); (b) host-side retention window;
     (c) leave pruning to workspace tooling (`document_delete` works today).
   - Recommendation: (a) now, (c) as the documented answer; asks are decision records and deleting
     them should be a deliberate act.
   - Consequence of deferring: unbounded but slow growth; no correctness impact.

## 10. Change log

- 0.1.2: renamed the surface for disambiguation: tools `ask.begin`/`ask.wait`/`ask.submit` became
  `ask_questions`/`ask_answers`/`ask_record`; `ask_record` is app-only (hidden from the model's
  tool list, still dispatchable); the app renamed from Ask (`internal/ask`) to Decisions
  (`internal/decisions`), moving its launcher out of the `ask` prefix. `recommendation` relaxed
  from required to desired at the wire. Storage collection `loom.ask` unchanged.
- 0.1.1: per-ask instance URIs implemented and resolved into the decision log; conformance section
  extended with the concurrency coverage.
- 0.1.0: initial draft. Records the shipped first slice (internal app, three tools, Document-facet
  storage, end-to-end server test) and the open decision points above.
