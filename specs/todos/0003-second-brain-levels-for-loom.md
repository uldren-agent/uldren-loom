# Second brain levels for Loom

## Idea

Use the "five levels" model from Nate Herk's video `Every Level of a Claude Second Brain Explained`
as a planning lens for Loom-based personal and team memory.

The video argues that an AI second brain should be designed backward from the questions it needs to
answer. The storage form should match the recall mode: routed files for exact lookup, wikis for topic
navigation, vectors for semantic recall, graphs for relationship chains, and always-on sync only when
the extra autonomy creates more value than noise.

Loom is a strong substrate for this because those levels map naturally to typed workspaces inside one
versioned store.

## Sources checked

- Video metadata from YouTube oEmbed identifies the video as `Every Level of a Claude Second Brain
  Explained` by `Nate Herk | AI Automation`.
  Source: `https://www.youtube.com/oembed?url=https://www.youtube.com/watch?v=DTCyvo6cC54&format=json`.
- The user-provided transcript describes the five levels: routing files, wiki-style topic grouping,
  semantic search, knowledge graphs, and an always-on brain OS.
  Source: attached transcript for `https://www.youtube.com/watch?v=DTCyvo6cC54`.
- Loom workspaces are independent typed trees. Workspace types include `files`, `document`, `vector`,
  `graph`, `search`, `sql`, `ledger`, and other facets. History and writes are scoped to one
  workspace, while explicit read-only queries may span workspaces.
  Source: `specs/0014-workspaces.md:7`, `specs/0014-workspaces.md:32`,
  `specs/0014-workspaces.md:99`.
- Loom's files binding is the built surface for raw file trees. It supports read, write, create, stat,
  list, move, copy, and walk. `append_file` is not built.
  Source: `specs/facet-bindings/P9-0003-files-binding.md:14`.
- Loom's document binding is currently keyed JSON or CBOR documents. Secondary indexes and `find` are
  not built.
  Source: `specs/facet-bindings/P9-0008-document-binding.md:9`.
- Loom's vector binding supports `upsert`, `get`, `remove`, `scan`, and `search`.
  Source: `specs/facet-bindings/P9-0012-vector-binding.md:9`.
- Loom's graph and search bindings are provisional. They are target surfaces for relationship chains
  and keyword search once those facets land.
  Source: `specs/facet-bindings/P9-0014-graph-binding.md:1`,
  `specs/facet-bindings/P9-0013-search-binding.md:1`.

## Level model

The levels are not a maturity ladder where higher is always better. They are retrieval strategies.
Different folders, projects, or data types can live at different levels inside the same Loom.

### Level 1: routed files

Question answered: can the agent or user find a file or fact by exact name, path, word, or routing
rule?

Core shape:

- A routing document, such as `AGENTS.md`, explains where facts live.
- Files are organized in predictable folders.
- Decisions, projects, personal context, and reference files have explicit homes.
- The agent reads the routing file first and then drills into the right folder.

Loom mapping:

- `files:"brain"` stores markdown and source files.
- `document:"brain"` may store a normalized index of file metadata.
- `search:"brain"` can later provide keyword lookup, but the MVP only needs file paths and routing.

Good fit:

- Small or medium corpora.
- Stable context files.
- Facts that users remember by name.
- Data where the whole document should be read to answer the question.

Risk:

- Routing files can grow stale.
- Exact-word lookup misses related concepts.
- Agents may waste tokens if routing is vague.

### Level 2: wiki and topic routing

Question answered: can the agent pull together everything on a topic?

Core shape:

- A wiki adds concept pages, source pages, topic pages, comparisons, and indexes.
- Markdown links or backlinks connect pages.
- The relationship is mostly navigational: a link says "related enough to read next," not a typed
  semantic relation.

Loom mapping:

- `files:"brain"` stores wiki pages under stable paths such as `/wiki/concepts/`.
- `document:"brain"` stores normalized page records with title, aliases, source digests, and outgoing
  links.
- `graph:"brain"` can later import wiki links as weak `LINKS_TO` or `RELATED_TO` edges.

Good fit:

- Dozens to hundreds of notes.
- Topic-based navigation.
- YouTube transcript wikis, meeting transcript wikis, project references, and decision logs.

Risk:

- Links are not enough for precise relationship questions.
- The agent still may need to read whole pages.
- Wiki pages can become a second organization project unless ingestion is structured.

### Level 3: semantic search

Question answered: can the agent find relevant material even when the query uses different words
from the source?

Core shape:

- Documents are chunked.
- Chunks are embedded.
- The agent searches by meaning instead of exact keyword match.
- Search results return snippets or chunks, not necessarily the full source document.

Loom mapping:

- `files:"brain"` remains the source of truth for complete documents.
- `vector:"brain"` stores embeddings for chunks, summaries, concepts, and tags.
- `document:"brain"` stores chunk manifests and embedding metadata.
- `search:"brain"` can later provide hybrid keyword plus vector retrieval.

Good fit:

- Large text corpora.
- Queries where the exact words are unknown.
- Rules, references, transcripts, support snippets, and examples.

Risk:

- Vector search is not a magic full-context answer engine.
- A summary question over a specific meeting should often read the whole meeting file, not only a few
  vector chunks.
- Chunking policy becomes part of correctness.

### Level 4: relationship graph

Question answered: can the agent trace relationship chains across people, projects, ideas, tasks,
decisions, sources, and time?

Core shape:

- The system extracts entities and typed relations.
- It distinguishes a backlink from an explicit relation such as `WORKS_AT`, `ENDORSED_BY`,
  `COMPETES_WITH`, `DECIDED`, or `BLOCKED_BY`.
- It supports questions that require following chains, not only finding similar text.

Loom mapping:

- `graph:"brain"` stores entities, source spans, and typed relations.
- `files:"brain"` stores raw evidence and readable markdown.
- `document:"brain"` stores annotations and extraction runs.
- `vector:"brain"` stores embeddings for entities, spans, ideas, decisions, and tasks.
- `search:"brain"` indexes labels, aliases, titles, and evidence snippets.

Good fit:

- CRM-like relationship memory.
- Project histories.
- "When did this idea start?" and "how has it evolved?" questions.
- Cross-meeting task, decision, and risk analysis.

Risk:

- Extraction can hallucinate if it is not span-grounded.
- The graph needs review, confidence, and provenance.
- Relationship schema drift can create duplicate and unqueryable edges.

### Level 5: always-on brain OS

Question answered: can the system update itself from live sources and keep memory fresh without the
user manually choosing every ingest?

Core shape:

- Scheduled or event-driven sync pulls from source systems.
- It refreshes memory, extracts tags, updates summaries, and links new material.
- It may coordinate multiple agents or team stores.

Loom mapping:

- `ledger:"brain-ingest"` stores append-only import and extraction events.
- `files:"brain"` stores raw snapshots.
- `document:"brain"` stores current normalized state and source checkpoints.
- `graph:"brain"` stores durable relationship memory.
- `vector:"brain"` stores recomputable embeddings.
- `program:"brain"` can later host import and extraction manifests or compute programs.

Good fit:

- High-volume recurring sources.
- Team or agent systems that need fresh context.
- Sources where missing a new item is costly.

Risk:

- Always-on import can add noise.
- Ephemeral conversations and operational chatter can pollute durable memory.
- Privacy and source permissions become harder.
- The user loses editorial control unless review policies are explicit.

## Design principle

The importer should choose the lowest level that answers the target question.

Examples:

- "Summarize the March 5 meeting" should read the full markdown transcript from `files`, not rely on
  vector chunks.
- "Find videos where the creator discussed semantic search but did not say semantic search" should use
  `vector`.
- "When was the idea of a meeting memory graph first introduced?" should use `graph`, with source span
  evidence from `files`.
- "Which projects changed after this decision?" should use `graph` plus document records.
- "What should the agent read first for this area?" should use routing files and wiki index pages.

## Loom workspace strategy

Use named workspaces rather than forcing the whole second brain into one shape.

| Workspace | Level | Data |
| --- | --- | --- |
| `files:"brain"` | 1 and 2 | Raw markdown, routing files, wiki pages, transcripts, source snapshots |
| `document:"brain"` | 2 through 5 | Normalized records, chunk manifests, annotations, extraction runs |
| `vector:"brain"` | 3 through 5 | Embeddings for chunks, tags, summaries, ideas, and decisions |
| `graph:"brain"` | 4 and 5 | Entities, relations, source spans, evolution chains |
| `search:"brain"` | 1 through 5 | Keyword search, facets, highlights when search lands |
| `sql:"brain-analytics"` | 4 and 5 | Optional tabular reports over notes, tags, tasks, decisions, sources |
| `ledger:"brain-ingest"` | 5 | Append-only import, extraction, review, and promotion events |

The same source object can project into multiple workspaces. The raw file remains the source of truth;
vectors, graph nodes, search documents, and SQL rows are derived and can be regenerated.

## Routing file model

The `files` workspace should include portable routing files for agent harnesses:

- `/AGENTS.md`: Codex-compatible routing and operating rules.
- `/CLAUDE.md`: Claude-compatible routing when needed.
- `/memory.md`: curated durable memories.
- `/wiki/index.md`: topic and source map.
- `/decisions/index.md`: decision log and current accepted decisions.
- `/sources/index.md`: source systems and freshness policy.

These files should route agents to Loom-backed workspaces when tools exist. Until then, they should
route agents to readable files and sidecars.

## Ingest decision matrix

Before ingesting a new source, answer these questions:

- What future questions should this source answer?
- Does the answer require the whole source, a snippet, a relationship chain, or a fresh external read?
- Is the source durable context or temporary operational chatter?
- Is the source safe to store long term?
- Which workspace owns the raw source?
- Which projections are derived from it?
- What is the review policy before extracted facts become accepted memory?

## Implementation plan

1. Define a `brain_level` field for ingest manifests.
   - Allowed values: `routed_files`, `wiki`, `semantic`, `graph`, `always_on`.
   - A source can declare multiple levels, but one level should be the default retrieval path.

2. Add routing file templates.
   - Generate `AGENTS.md`, optional `CLAUDE.md`, `wiki/index.md`, `sources/index.md`, and
     `decisions/index.md`.
   - Keep templates short and source-specific.

3. Add source classification.
   - Classify each source as durable context, reference, operational feed, meeting record, media
     transcript, decision source, or external lookup.
   - Store the classification in the import manifest.

4. Add retrieval policy hints.
   - Mark source folders as `read_whole`, `semantic_search`, `graph_query`, `keyword_search`, or
     `external_live_lookup`.
   - Let agents pick the cheapest retrieval path that satisfies the question.

5. Add projection checks.
   - Verify that every vector chunk points back to a source file digest.
   - Verify that every graph edge has a source span.
   - Verify that every accepted decision has evidence.

6. Add review gates.
   - Keep LLM-created relationship facts as `suggested` until confidence or policy promotes them.
   - Keep routing file edits explicit and reviewable.

7. Add diagnostics.
   - Report stale routing files, orphan vector chunks, graph edges without evidence, and source files
     with no projections.

## Open questions

- Should a Loom second brain default to `files` or `vcs` for raw markdown?
- Which source types should be allowed into always-on ingestion by default?
- How should a user mark a source as ephemeral so it stays queryable but does not become durable
  memory?
- Should routing files be generated from workspace manifests, or manually authored and validated?
- What is the first useful cross-workspace query API for agent recall?
