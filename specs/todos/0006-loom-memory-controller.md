# Loom memory controller

## Idea

Build a Loom memory controller: the orchestration layer between an LLM and Loom that turns a small
model context window into a managed working set over durable, typed, versioned memory.

The controller decides what memory to retrieve, where to retrieve it from, how to pack it into the
model context, when to call tools, and what new facts should be written back. The LLM should not need
the entire chat history or the entire knowledge store in its prompt. Loom holds long-term memory; the
controller pages the right memory into the model's short-term working set.

Mental model:

```text
Loom = durable memory
LLM context = working set
Memory controller = cache manager + librarian + fact writer + policy gate
```

This should work with local models running through Candle or similar runtimes, hosted models with
tool calling, and agent harnesses that expose MCP tools. The core concept is independent of the model
runtime: model execution can happen on GPU, while Loom queries and policy checks run in the host
process on CPU.

## Sources checked

- Loom workspaces are independent typed trees inside one Loom. Workspace types include `files`,
  `document`, `vector`, `graph`, `search`, `sql`, `ledger`, and other facets. History and writes are
  scoped to one workspace, while explicit read-only queries may span workspaces.
  Source: `specs/0014-workspaces.md:7`, `specs/0014-workspaces.md:32`,
  `specs/0014-workspaces.md:99`.
- Loom's files binding is the built surface for raw file trees. It supports read, write, create, stat,
  list, move, copy, and walk. `append_file` is not built.
  Source: `specs/facet-bindings/P9-0003-files-binding.md:14`.
- Loom's document binding is currently keyed JSON or CBOR documents. Secondary indexes and `find` are
  not built.
  Source: `specs/facet-bindings/P9-0008-document-binding.md:9`.
- Loom's vector binding supports `upsert`, `get`, `remove`, `scan`, and `search`. The binding notes
  that the built reality is exact search with metadata equality filtering, while older facade text
  still needs reconciliation.
  Source: `specs/facet-bindings/P9-0012-vector-binding.md:9`,
  `specs/facet-bindings/P9-0012-vector-binding.md:15`.
- Loom's graph and search bindings are provisional. They are target surfaces for relationship-chain
  queries and keyword search once those facets land.
  Source: `specs/facet-bindings/P9-0014-graph-binding.md:1`,
  `specs/facet-bindings/P9-0013-search-binding.md:1`.
- The second-brain levels plan captures the design rule that the retrieval mode should match the
  question: routed files, wiki, semantic search, relationship graph, or always-on sync.
  Source: `specs/todos/0003-second-brain-levels-for-loom.md:42`.
- The Meetings, YouTube, and guided-capture plans define source-span annotations, tag extraction,
  real-time workspace projection, and graph-ready memory records.
  Source: `specs/studio/MEETINGS.md:262`,
  `specs/todos/0004-youtube-channel-ingest.md:280`,
  `specs/todos/0005-guided-interrogation-capture.md:342`.

## Problem

LLM chat history is not durable memory.

Long conversations are pruned, summarized, compacted, or lost. Even when a model has a large context
window, putting everything in the prompt is expensive and often wrong. Some questions need an exact
file, some need a whole transcript, some need a graph path, some need semantic neighbors, and some
need current external data.

The memory controller exists to prevent this failure mode:

```text
Stuff more context into the prompt
-> hit cost and attention limits
-> compact chat history
-> lose details and provenance
-> ask the model to infer missing context
```

The target flow is:

```text
Store durable source truth in Loom
-> derive structured memory into workspaces
-> retrieve a compact evidence pack per turn
-> answer with citations and provenance
-> write back only durable, policy-allowed memory
```

## Non-goals

- The controller is not a new model architecture.
- The controller does not make GPU-executed model kernels call Loom directly.
- The controller does not treat every chat turn as permanent memory.
- The controller does not replace Loom facets. It coordinates them.
- The controller does not accept LLM-inferred facts as accepted memory without policy.

## Architecture

```text
User turn
  -> Memory controller
       -> classify intent
       -> resolve entities
       -> plan retrieval
       -> query Loom workspaces
       -> rank and deduplicate evidence
       -> pack context
  -> LLM runtime
       -> answer or request a tool call
  -> Memory controller
       -> execute tool calls
       -> validate outputs
       -> propose writeback
       -> apply policy and review gates
       -> commit memory
```

The controller can be embedded in Uldren Desktop, exposed as a local service, or provided as an MCP
server to agent harnesses. In all cases, the controller owns retrieval planning, context packing, and
writeback policy.

## Core responsibilities

### Intent classification

Classify the user's turn before retrieval.

Common intent classes:

- `read_whole_source`: summarize or inspect a known meeting, file, note, video, or session.
- `semantic_recall`: find related ideas when the user may not know exact words.
- `relationship_query`: trace people, ideas, tasks, decisions, evidence, or timelines.
- `keyword_lookup`: find exact names, paths, symbols, URLs, or quoted text.
- `timeline_query`: determine first seen, latest state, evolution, or sequence.
- `comparison`: compare two sources, ideas, versions, people, or decisions.
- `diff_query`: ask what changed across revisions or import runs.
- `write_memory`: remember, correct, tag, accept, reject, or update durable memory.
- `external_lookup`: fetch live data from a connector instead of relying on stored memory.
- `sensitive_query`: HR, legal, investigation, security, finance, or private personal data.

The intent controls the retrieval plan. For example, "summarize the March 5 meeting" should read the
whole meeting file, while "where did this idea first appear?" should start with graph and timeline
records.

### Entity resolution

Map user language to stable Loom identities.

Examples:

```text
"Sarah"
"Sarah Chen"
"sarah@example.com"
"the PM on onboarding"
-> granola:person:sarah@example.com
```

Resolution should use:

- Exact IDs and file paths.
- Aliases from documents and graph nodes.
- Email, URL, handle, note ID, video ID, issue ID, or code symbol.
- Recent conversation focus.
- Vector search for fuzzy concept labels.
- User clarification when ambiguity is high.

The controller should keep ambiguity explicit. It can ask "which Sarah?" or retrieve both candidates
with their distinguishing context.

### Retrieval planning

Build a query plan across Loom workspaces.

Example:

```text
Question: How has the Granola ingest idea evolved?

Plan:
- graph:"brain": resolve Idea node and traverse INTRODUCES, EVOLVES_TO, DECISION_ABOUT, HAS_TASK.
- files:"brain": fetch source spans for the strongest claims.
- vector:"brain": find semantically similar chunks around meeting memory and Granola ingest.
- document:"brain": fetch normalized note and annotation records.
- ledger:"brain-ingest": inspect accepted/rejected memory events when provenance matters.
```

The controller should choose the cheapest sufficient plan. It should avoid vector search when an exact
ID or file path is known, and avoid whole-file reads when a small graph path or source span answers
the question.

### Evidence ranking

Rank evidence before it reaches the model.

Ranking signals:

- Direct source span beats summary.
- Accepted fact beats suggested fact.
- Recent correction beats older derived summary.
- Source with stronger provenance beats inferred relation.
- Exact entity match beats fuzzy semantic match.
- Whole-source read wins when the question asks for totality.
- User-authored or imported source text beats model-generated commentary.

The evidence pack should include why each item was selected.

### Context packing

Pack evidence into a bounded prompt budget.

The packer should budget:

- System and developer instructions.
- Current user turn.
- Recent working conversation.
- Retrieved evidence.
- Tool schemas or tool affordances.
- Scratch space for model reasoning where the runtime supports it.
- Reserved response budget.

Evidence order should usually be:

1. Task-specific instructions.
2. Resolved entities and query interpretation.
3. Direct source evidence.
4. Accepted graph or document facts.
5. Summaries of larger clusters.
6. Lower-confidence suggestions.
7. Raw chunks only when needed.

The context pack should be inspectable. Users and developers should be able to see what memory was
provided and what was left out.

### Tool-call mediation

The controller should expose memory tools to the model, but mediate them.

Candidate tools:

- `memory.resolveEntity`.
- `memory.retrieve`.
- `memory.readSource`.
- `memory.queryGraph`.
- `memory.searchVector`.
- `memory.searchText`.
- `memory.getDocument`.
- `memory.proposeFact`.
- `memory.proposeRelation`.
- `memory.proposeTask`.
- `memory.explainEvidence`.

The model may request tools, but the controller should enforce:

- Capability checks.
- Workspace permissions.
- Sensitive-data policy.
- Query budget.
- Loop limits.
- Result-size limits.
- Writeback review requirements.

The model should propose durable writes. The controller validates and commits.

### Writeback

Write back only memory that is durable enough, source-grounded enough, and policy-allowed enough.

Good writeback candidates:

- User-confirmed facts.
- Decisions.
- Tasks.
- Corrections.
- Durable preferences.
- Accepted tags.
- Entity aliases.
- Evidence-backed relations.
- Session summaries.
- Import manifests.
- Review outcomes.

Poor writeback candidates:

- Ephemeral chatter.
- Unconfirmed guesses.
- Low-confidence inference.
- Temporary debugging state.
- Sensitive data without explicit policy.
- Connector output that should stay live rather than stored.

Writeback status should include:

- `suggested`.
- `accepted`.
- `rejected`.
- `superseded`.
- `merged`.
- `redacted`.

### Compaction replacement

The controller should replace chat compaction as the primary long-term memory mechanism.

Old approach:

```text
Conversation gets summarized into a compacted chat prefix.
The summary may lose details, evidence, and uncertainty.
```

Loom-backed approach:

```text
Conversation transcript -> files
Durable facts -> documents
Entities and relations -> graph
Embeddings -> vector
Keywords -> search
Audit events -> ledger
Next turn -> retrieve only the relevant working set
```

A compacted chat summary can still exist, but it should become a navigation hint, not the only
memory.

## Memory record model

The controller should use explicit record types.

### Source record

- `source_id`.
- `source_type`: meeting, video, guided capture, file, connector output, chat turn, issue, document.
- `path` or external URL.
- `digest`.
- `created_at`.
- `imported_at`.
- `owner`.
- `sensitivity`.
- `retention_policy`.

### Span record

- `span_id`.
- `source_id`.
- `range`: byte range, timestamp range, line range, transcript entry ID, or DOM selector.
- `text_digest`.
- `speaker` when known.
- `language`.
- `provenance`.

### Fact record

- `fact_id`.
- `kind`: decision, preference, task, claim, status, constraint, risk, metric, requirement, definition.
- `subject_id`.
- `predicate`.
- `object`.
- `source_spans`.
- `confidence`.
- `status`.
- `valid_time` when the fact is about a historical period.
- `transaction_time` when Loom observed or accepted it.

### Relation record

- `relation_id`.
- `src`.
- `label`.
- `dst`.
- `properties`.
- `source_spans`.
- `confidence`.
- `status`.

### Summary record

- `summary_id`.
- `source_ids`.
- `scope`.
- `summary_text`.
- `source_spans`.
- `model`.
- `prompt_version`.
- `created_at`.
- `supersedes`.

## Workspace mapping

| Workspace | Stored data | Controller role |
| --- | --- | --- |
| `files:"memory"` | Raw chat turns, transcripts, imported files, evidence snapshots, readable reports | Source truth and audit material |
| `document:"memory"` | Sources, spans, facts, summaries, annotations, writeback proposals | Normalized records and controller state |
| `graph:"memory"` | Entities, relations, fact links, timelines, supersession chains, provenance edges | Relationship and multi-hop memory |
| `vector:"memory"` | Embeddings for spans, summaries, entities, facts, decisions, tasks, and questions | Semantic recall and fuzzy entity lookup |
| `search:"memory"` | Indexed source text, labels, aliases, facts, tasks, and evidence snippets | Keyword lookup and facets |
| `sql:"memory-analytics"` | Optional projected tables for usage, retrieval quality, tasks, decisions, and writeback review | Reporting and evaluation |
| `ledger:"memory-audit"` | Optional append-only events for sensitive reads, writes, accepts, rejects, redactions, exports | Audit and compliance |
| `program:"memory"` | Optional retrieval policies, extraction manifests, and controller programs when the facet lands | Versioned controller behavior |

## Runtime integration

### Local model with Candle

Candle can run the model, but the memory controller remains host-side.

```text
Rust host process
  -> Candle model runtime on CPU/GPU
  -> Loom memory controller on CPU
  -> Loom store and workspaces
```

The model emits text or structured tool requests. The host parses those requests, calls Loom, and
feeds the result back into the next model step.

The GPU runs tensor kernels. It should not be expected to call arbitrary Loom functions directly.
The controller is the bridge between model output and CPU-side memory operations.

### Hosted model

For hosted models, the same controller can provide tool schemas and execute tool calls. The hosted
model sees only the tools and returned evidence pack. Loom stays local or server-side depending on the
deployment.

### MCP surface

The controller can expose an MCP server with tools such as:

- `memory.ask`.
- `memory.retrieve`.
- `memory.resolve_entity`.
- `memory.read_source`.
- `memory.propose_write`.
- `memory.commit_reviewed_writes`.
- `memory.inspect_trace`.

This gives other harnesses a single memory interface rather than direct access to every Loom facet.

## Retrieval modes

### Whole-source mode

Use when the question asks about a specific bounded source.

Examples:

- Summarize this meeting.
- What did this video say about vector databases?
- Review this post-mortem.

Read from `files`, then optionally fetch related annotations.

### Graph-first mode

Use when the question asks about relationships, evolution, provenance, ownership, decisions, or
timelines.

Examples:

- When was this idea first introduced?
- Which tasks are blocked by this risk?
- Who was involved in this decision?

Query `graph`, then fetch source spans from `files`.

### Vector-first mode

Use when the question is fuzzy, exploratory, or phrased differently from the source.

Examples:

- Find similar ideas.
- Where did we discuss something like agent memory?
- What past meetings sound related to this problem?

Search `vector`, then rerank with metadata and source spans.

### Search-first mode

Use when exact text, names, symbols, URLs, or quoted phrases matter.

Examples:

- Find every mention of `NsSelector`.
- Which videos mention `LightRAG`?
- Where is this URL cited?

Query `search` when available, or use files and document indexes as fallback.

### Hybrid mode

Use when the controller needs graph, vector, search, and whole-source reads.

The controller should record why hybrid mode was selected because it is more expensive and easier to
overfetch.

## Policy model

Memory policy should be explicit.

Policy inputs:

- Workspace.
- Source sensitivity.
- User role.
- Session mode.
- Data type.
- Retrieval intent.
- Writeback kind.
- External model or local model.
- Sync target.

Policy decisions:

- May retrieve.
- May pack into context.
- May show to user.
- May send to hosted model.
- May write as suggested.
- May auto-accept.
- May project to graph, vector, search, or SQL.
- May export.
- Must redact.
- Must audit.

Sensitive templates from guided capture, such as HR and investigations, should default to stricter
policy.

## Evaluation

The controller needs evaluation, not just demos.

Metrics:

- Retrieval precision: selected evidence was relevant.
- Retrieval recall: necessary evidence was not missed.
- Source faithfulness: answer claims cite supporting spans.
- Writeback precision: stored facts are durable and correct.
- Writeback restraint: ephemeral facts are not stored.
- Token efficiency: answer quality per retrieved token.
- Latency: retrieval and model round-trip time.
- Policy correctness: sensitive data is not leaked.
- Trace explainability: developers can explain why memory was selected.

Evaluation corpora:

- Granola meetings.
- YouTube channel transcripts.
- Guided-capture sessions.
- Synthetic ambiguity sets for entity resolution.
- Regression cases where vector search alone gives the wrong answer.

## Failure modes

- Over-retrieval: too much context hides the useful evidence.
- Under-retrieval: the model answers without needed memory.
- Wrong retrieval mode: vector chunks used when whole-source read was required.
- Entity collision: two people, projects, or ideas merged incorrectly.
- Stale memory: old fact retrieved without newer correction.
- Unreviewed inference: model guess becomes accepted memory.
- Sensitive leak: private fact packed into hosted-model context.
- Tool loop: model keeps asking for more retrieval without converging.
- Graph drift: relation labels multiply and become unqueryable.
- Summary rot: repeated summarization loses source nuance.

Each failure mode should have a traceable controller decision and a mitigation.

## Implementation plan

1. Define memory record schemas.
   - Source, span, fact, relation, summary, writeback proposal.

2. Build a read-only retrieval controller.
   - Intent classification.
   - Entity resolution.
   - Retrieval planning.
   - Context packing.
   - Trace output.

3. Add Loom workspace readers.
   - Files read and walk.
   - Document get and ids.
   - Vector search.
   - Graph query when available.
   - Search query when available.

4. Add writeback proposals.
   - Model can propose facts, relations, tasks, preferences, and corrections.
   - Controller stores proposals as documents with source spans and status.

5. Add review and commit.
   - User or policy accepts proposals.
   - Accepted memory projects to graph, vector, search, and SQL where available.

6. Add policy gates.
   - Local versus hosted model restrictions.
   - Sensitive workspace restrictions.
   - Audit ledger for sensitive reads and writes.

7. Add context pack inspection.
   - Show selected evidence, rejected evidence, token budget, and policy decisions.
   - Allow developers to replay a retrieval plan.

8. Add compaction replacement.
   - Convert long chat sessions into files, facts, summaries, graph relations, and embeddings.
   - Store a compacted chat summary only as a navigation hint.

9. Add MCP server.
   - Expose controller-level tools to agent harnesses.
   - Hide direct low-level Loom facets unless explicitly needed.

10. Add local model loop.
    - Integrate with Candle or another local runtime.
    - Parse structured tool requests from model output.
    - Feed tool results back into the next generation step.

11. Add evaluation harness.
    - Build test questions over known Loom stores.
    - Score evidence selection, answer faithfulness, writeback quality, and token efficiency.

## Open questions

- Should memory controller policy live in Loom as versioned `program` data, or in the app config?
- What is the minimum graph facade needed for useful relationship retrieval before the full graph
  binding lands?
- Should writeback ever auto-accept, or should user confirmation be mandatory for all durable facts?
- How should the controller handle conflicting memories across branches or workspaces?
- What is the first context pack format that should be exposed to models and users?
- Should the controller be a Desktop-only service first, a CLI library, or an MCP server first?
- What is the deletion and redaction model for memories that have derived vectors and graph edges?
