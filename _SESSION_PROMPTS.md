# Session Prompts

Copy-paste prompts for the next design sessions. Each is self-contained: it tells a fresh session
where context lives and how to work. Run them in the order given by `_QUEUE8.md` -> Spec Landing
Order.

Shared working rules (embedded in every prompt below): read `_QUEUE8.md` first for state and
decisions; ask owner questions in chat using Question / Context / Options / Examples /
Recommendation / Consequence-of-deferring, with details; record every decision and example into the
owning spec immediately (assume the session can vanish); no "v1" contracts - all contracts are
long-term, staging is implementation-only; tool names dotted in specs (flattened later); check the
source before relying on a boundary; keep source-backed facts separate from target design.

Store sync (added 2026-07-04, after four sessions ran in parallel): the dogfood store in
`loom-harness/plan.loom` mirrors `_QUEUE8.md`. At session end, append one batch decision entry to
the `queue8.decisions` ledger summarizing your session (follow the d-014/d-016 convention: JSON
payload with id/date/topic/decision/source), append resolution events to `queue8.questions` for any
questions you closed, and update the touched sections of the `plan/queue8.md` mirror (bump the seq
ranges in the headers). Never rewrite or re-bootstrap the store - ledgers are append-only; a rerun
duplicates history. If your session and another ran in parallel, whichever reconciles last appends
last; the ledger order is the record of who knew what when.

---

## Prompt 1 - JIRAISH design session

```
Design session for specs/studio/JIRAISH.md.

Context to read first, in order: _QUEUE8.md (state, resolved decisions, spec landing order),
specs/0061.md (the operation substrate JIRAISH builds on), specs/studio/JIRAISH.md (all of it,
including §20 field model, §21 planning, §22 agile, §23 example tools, §24 unfinished tasks),
specs/studio/ADOPTION.md §1.3 and G2, specs/studio/SURFACES.md §2 and §3a.

Goal: close the uniquely-Jiraish open decisions. Work through, in order:
1. The workflow transition validation machine (§7.3, §24): edge guards, required-field checks,
   revalidation-after-sequence rules, rejected-operation audit shape.
2. The field conflict policy matrix (§7.2) as profile policy over the 0061 §6 conflict record:
   per-field-class rules (last-writer-wins vs guarded vs human-review).
3. The custom field schema and typed field values (§18.7), validated by the 0061 §14 predicate
   tree's type system.
4. Issue-key alias format and project rename semantics (§18.3) over the 0061 §4 allocator.
5. Sprint/release conflict policy (§7.5) and sprint-close disposition details (§22).
6. The Jira import mapping table (requirement, ADOPTION §1.3): Jira export entities -> Jiraish
   operations, custom-field mapping, workflow mapping, alias preservation, fidelity report design.
   Treat import mapping as the stress test: where real Jira data does not fit §20-§22, fix the
   model, not the importer.
7. Rewrite the superseded local envelope/sequencer text (§5, §6) to reference 0061, and convert
   §19 "Recommended v1" to long-term-contract language.

Ask me decisions one batch at a time in the required question format. Record everything into
JIRAISH.md as you go; update _QUEUE8.md resolved-decisions and landing order at session end.
```

---

## Prompt 2 - PAGES body model: research then design session

```
Research-then-design session for the page body model in specs/studio/PAGES.md - the
deepest unresolved decision in the Studio suite (open decisions §19.3/§19.4).

Context to read first: _QUEUE8.md, specs/0061.md (§2 envelope, §6 conflicts, §9 versioning, §14
predicates), specs/studio/PAGES.md (all, esp. §5 body model, §8 concurrency, §21
structures, §23 unfinished tasks), specs/studio/SURFACES.md (document viewer, diagram editor
requirements), specs/studio/ADOPTION.md §1.3 (Confluence import).

Phase 1 - research (web + repo). Survey the three candidate models against Loom's constraints:
  (a) whole-page snapshots + conflict records (current stopgap);
  (b) sequenced operation deltas (ProseMirror-style steps / OT with a central orderer - note Loom
      already has a blind central sequencer, which changes the usual OT tradeoff);
  (c) CRDTs (Yjs, Automerge, Loro; also Peritext for rich text).
Constraints to score against: canonical-bytes rule (identity-affecting formats need one pinned
canonical encoding + cross-language conformance vectors); blind replica (server cannot read
content; merges must be client-side and deterministic); retention/crypto-shred (a revision's
content must be hard-deletable; beware CRDT tombstones); inline-comment anchoring (§8.3); diagram
and structure-node binding (§21); import fidelity from Confluence storage format; history size.
Produce a comparison table with citations before proposing anything.

Phase 2 - design. Propose the body model (likely: pinned block-structured document + sequenced
deltas + snapshot revisions at publish boundaries, with (a) as the degenerate no-collab mode ---
but validate against Phase 1, don't assume). Then pin, in the spec: block structure and canonical
normalization, the delta operation set, the anchor model for inline comments, and the Confluence
import mapping for bodies. Ask me the decisions in the required question format, one batch at a
time. Record everything into PAGES.md and _QUEUE8.md as you go.
```

---

## Prompt 3 - WEBISH audit + cross-profile reference grammar

```
Short session, two bounded jobs.

Context to read first: _QUEUE8.md, specs/0061.md (§4 aliases, §7.1 mentions, §16.6),
specs/studio/WEBISH.md (never yet audited against the substrate), specs/studio/ADOPTION.md G4.

Job 1 - WEBISH substrate audit: read WEBISH.md; identify what it restates that specs/0061.md now
owns (envelope, sequencer, cursors, annotations, attachments, watches, versioning); add the same
Contract Boundaries bullets and Unfinished Tasks push-back section the other four profiles have;
list what is uniquely WEBISH. Also add its example tool surface table (dotted names, marked
illustrative only).

Job 2 - pin the cross-profile reference grammar (0061 §16 open decision 6): the typed reference
forms (issue keys like LOOM-52, page:..., msg:..., file:..., principal mentions), alias resolution at
sequence time (bind to stable ids), mention-extraction rules for free text, rename stability, and
how references serialize in operation payloads. Write it into 0061 as a numbered section, resolve
open decision 6, and update the worked example in ADOPTION G4 if the grammar differs. Ask me
decisions in the required question format with details.
```

---

## Prompt 4 - Dogfood bootstrap of plan.loom

```
Working session, not a design session: bootstrap the dogfood planning store.

Context to read first: _QUEUE8.md (especially Spec Landing Order and resolved decisions),
specs/studio/LIFECYCLE.md (the canonical feature lifecycle), specs/0061.md §8 (bootstrap view,
markdown mirror). Check what is actually implemented in the tree before planning anything ---
design-target tools do not exist yet; use the currently served facet tools (document, columnar,
graph, ledger, kv, cas) directly.

Goal: recreate the Queue 8 working state inside plan.loom using the interim facet-level design:
per-project collections, work table (type task|bug|spike), decisions ledger, questions queue,
graph edges for dependencies, kv state and ID counters. Then generate the read-only markdown
mirror back into the repo as the search/grep surface. Migrate the current _QUEUE8.md contents in
as the first lifecycle instance (stage: Build). Ask before anything destructive. Record the
store schema you actually used in _QUEUE8.md so later sessions can rely on it.
```

---

## Prompt 5 - Notion + Asana analysis, imports, and the adopted-model schema pass

```
Research-then-design session. Owner decisions already made (2026-07-04, recorded in _QUEUE8.md and
the specs): Rally's items are adopted (JIRAISH §24.1: configurable portfolio hierarchy, nested
stories, `accepted` status category, Rally capacity model); Redmine import is a requirement;
PAGES's surface is "Notion surface, Notion internals"; block_ref is a committed substrate
block kind (0061 §16 decision 8). This session designs the detail those decisions imply.

Context to read first: _QUEUE8.md, specs/0061.md (§9.1 body model incl. block_ref commitment, §19
reference grammar), specs/studio/JIRAISH.md (§24.1 adoptions, §25 import), specs/studio/
PAGES.md (surface decision in the intro, §5, §21), specs/studio/ADOPTION.md §1.3 matrix.

Phase 1 - research (web). Asana data model via API docs (tasks, multi-homing, sections, custom
fields, portfolios, goals, rules, forms, approval tasks, stories stream, JSON-lines resource
export); Notion API block/database model and export formats; Obsidian vault format (frontmatter,
wikilinks, embeds, .canvas).

Phase 2 - design, in order:
1. Portfolio hierarchy schema (JIRAISH §24.1.1): level taxonomy as workspace data, rollup rules
   per level, taxonomy-constrained parent edges, Jira-epic as degenerate taxonomy, Rally/Asana
   portfolio import mapping onto it.
2. `accepted` category integration (§24.1.3): workflow policy hooks, Jira/Asana import category
   mapping, DoD gate interaction (LIFECYCLE).
3. Capacity schema (§24.1.4): per-member per-iteration capacity, plan-estimate vs task-remaining,
   load projection.
4. Asana import mapping table: the multi-homing tension is the headline - Asana tasks live in many
   projects; JIRAISH keys are per-project (§4.1). Design membership edges + primary-scope key (or
   keyless issues?) without breaking §4.1. Stories stream -> operation synthesis; sections, custom
   fields, portfolios/goals -> roadmap items; approval -> accepted; rules -> automations (fidelity-
   reported); forms -> intake (likely out of scope, record as gap).
5. Notion import mapping table: block tree -> §9.1 blocks; databases -> structures (schema mapping
   for properties incl. relations/rollups/formulas - formulas likely fidelity-reported); synced
   blocks -> block_ref.
6. block_ref semantics (0061 §16 decision 8): shred/removal degradation, pinned-revision fallback,
   cycle prevention, render depth, search indexing policy.
7. PAGES Notion-surface reframe pass (intro decision): surface kinds vs import kinds in
   §5.1, space/macro language audit, database-view surface contract.
8. Obsidian/markdown-tree mapping - promoted to build focus (owner 2026-07-04): frontmatter ->
   fields, wikilinks/embeds -> refs/block_ref, .canvas -> canvas structures, plugin data
   fidelity-reported. Include the archive-container note (ADOPTION §1.3): the import framework
   owns zip/gzip/tar ingestion; Loom storage-frame compression (0005) is not archive tooling.

Treat every import mapping as a schema stress test: fix the model, not the importer. Ask decisions
one batch at a time in the required question format. Record into the owning specs as you go; update
_QUEUE8.md and sync the dogfood store at session end per the shared rules.
```

---

## Prompt 6 - SLACKISH design session (includes agent metering, ADOPTION G7)

```
Design session for specs/studio/SLACKISH.md.

Context to read first: _QUEUE8.md, specs/0061.md (§2/§3 envelope+sequencer, §7 annotations, §7.1
shared facilities incl. watches and operating modes, §12 changes/cursors), specs/studio/SLACKISH.md
(all, esp. §15 open decisions, §17 example tools, §18 unfinished tasks), specs/studio/ADOPTION.md
§1.3 (Slack import row) and G7.

Work through, in order:
1. Uniquely-Slackish open decisions: Merkle log structure choice (§15.2, pinned with 0021a);
   channel-log/namespace-branch relationship (§15.4) and remote-tracking refs (§15.5); retention
   live-root for chat GC (§15.6); MCP resource URI stability (§15.7); blind-vs-keyed compute
   boundary (§15.8); presence/typing/unread ephemerals; task claim/complete exclusivity over 0036.
2. Slack import mapping table (requirement, ADOPTION §1.3): export zip -> chat events, threads,
   reactions, files -> attachments, bots -> agent principals, users -> imported principals; coexistence
   bridge as a 0061 §7.1 mirror mode; fidelity report. Fix the model, not the importer.
3. ADOPTION G7 - agent rate limiting and metering (folded in here because SLACKISH §13 demands
   agents be rate-limited and permissioned independently, and chat is where agent behavior is most
   visible): per-agent budgets (operations/hour, automation compute, spend), anomaly surfacing
   ("agent posted 500 messages in 4 minutes") gated by elicitation to continue, per-agent kill
   switch, trace-digest retention. If the design generalizes beyond chat (it should), lift it to
   0061 and make SLACKISH the first consumer.
4. Supersession rewrite: §3/§4 envelope and log text to reference 0061 §2/§3; §16 "Recommended v1"
   converted to long-term-contract language; §15 items annotated resolved/0061-owned. Fix the
   forward reference near the watches text once agent metering lands in 0061 (it currently names
   this session as its target). Also fold in the orphaned WEBISH supersession rewrite (WEBISH §23
   "Recommended v1" + its superseded local text) - WEBISH's own session already ran without it.

Ask decisions one batch at a time in the required question format. Record into SLACKISH.md (and
0061 for lifted designs) as you go; update _QUEUE8.md and sync the dogfood store at session end.
```

---

## Prompt 7 - DRIVEISH design session

```
Design session for specs/studio/DRIVEISH.md.

Context to read first: _QUEUE8.md, specs/0061.md (§2/§3, §6 conflicts, §7.1 attachments and
operating modes, §9 versioning), specs/studio/DRIVEISH.md (all, esp. §17 open decisions, §19
example tools, §20 unfinished tasks), specs/studio/ADOPTION.md §1.3 (Drive/SharePoint row and the
archive-containers note).

Work through, in order:
1. Uniquely-Driveish open decisions: folder index structure and canonical path normalization
   (§17.3); chunking policy for large files and range updates (§17.4); conflict-copy naming
   (§17.5) as profile policy over the 0061 §6 conflict record; metadata merge matrix (§17.6);
   editor lease semantics over 0036 (§17.7); dehydrated-file markers (§17.8); retention live-root
   for trash (§17.9).
2. Drive/SharePoint import mapping table (requirement): trees, files, revisions where exportable,
   shares -> grants, comments -> annotations, users -> imported principals; coexistence bridge as a
   0061 §7.1 mirror mode; fidelity report.
3. Archive question from ADOPTION §1.3: decide whether archive preview/extract (zip/gz/tgz)
   becomes a Driveish worker feature (content-aware, keyed-worker only) or stays out of scope;
   record either way.
4. Supersession rewrite: envelope/log text to 0061 §2/§3; §18 "Recommended v1" converted; §17
   items annotated resolved/0061-owned.

Ask decisions one batch at a time in the required question format. Record into DRIVEISH.md as you
go; update _QUEUE8.md and sync the dogfood store at session end.
```

---

## Prompt 8 - Unified Search / Discovery (new spec, the `loom search` cross-Loom surface)

```
**COMPLETED 2026-07-05 - spec landed as specs/0064.md (Unified Search / Discovery); DP1-DP8
decided. Kept for reference.** Design session for a new composition spec 0064 (note: the provisional
"0062" in the original prompt was already taken by Inference Model Downloads), plus its binding to Studio.

Context to read first: _QUEUE8.md (Tier-1/Tier-2 audit note + this decision), specs/0061.md §10
(cross-facet search interface, engine ladder §10.2, decoupling rule §10.3), specs/0033 (search /
FTS, native engine deferred), specs/0017 (vector layer - exact-is-contract, accelerators rebuildable),
specs/0040 (GraphRAG composition), specs/0050 + 0051 (embedding / LLM providers), _QUEUE2 (Tier-2
OpenSearch task 280b, Qdrant/Pinecone vector ports, and the reserved `loom search` cross-Loom
discovery in Missed/Hidden Work).

The framing is settled (owner, 2026-07-05): `substrate.search` is an interface, not an engine;
Studio is decoupled from how search happens; `substrate.search` and `loom search` are the same
interface at two scopes sharing one engine ladder (scan -> Tantivy lexical -> 0017 vector semantic ->
hybrid RRF -> 0040 graph-expanded); Tier-2 compatibility ports are egress skins over the same shared
accelerators. Design, don't relitigate that.

Design, in order:
1. [decided] New spec 0064 (composition, like 0040), not an extension of 0033.
2. The engine-ladder ranking/fusion contract: score normalization, reciprocal-rank fusion for
   hybrid, `match_via` provenance, deterministic tie-breaks, and how `as_of`/`history` scope (0061
   §10.1) interacts with each rung.
3. The embedding pipeline: which content_types get embedded, on-write vs batch, provider posture via
   0050, freshness/staleness signalling, and the keyed-only constraint (blind hosts store opaque
   vectors, cannot embed) - reconcile with SLACKISH §2.1 envelope visibility.
4. The cross-Loom `loom search` scope: index-set discovery across files/documents/mail/calendar/
   metadata/text/semantic, permission-scoped result fusion, and the CLI/tool surface (`loom search`
   vs `loom fts`).
5. The decoupling contract Studio depends on: exact `substrate.search` request/response, `match_via`,
   degraded-mode signalling, and the guarantee that engine swaps never change the signature.
6. Edge ports: confirm the OpenSearch (Tantivy) and Qdrant/Pinecone (0017) presentations project the
   same indexes the internal ladder uses; record the shared-index contract so Tier-2 build (_QUEUE2
   280b + vector ports) doesn't fork a second copy.

Ask decisions one batch at a time in the required question format. Record into 0064 and
0061 §10 as you go; update _QUEUE8.md and sync the dogfood store at session end.
```

---

## Prompt 9 - MEETINGS design session and Granola import contract

```
Design session for specs/studio/MEETINGS.md.

Context to read first, in order: _QUEUE8.md (state, build order, tasks 440a-d, 615, 735-737, 905),
specs/0061.md (operation envelope, identity aliases, annotations, views/projections, body model,
search interface, cross-profile references), specs/studio/MEETINGS.md (all of it), specs/studio/
ADOPTION.md §1.3 (import source matrix), specs/studio/SURFACES.md (app mechanism and catalog),
specs/0012-interchange-format.md if present, specs/0017-vector-layer.md, specs/0033-search-layer.md,
specs/0040-graphrag.md, and specs/0064.md.

Owner decisions already made (2026-07-06, recorded in MEETINGS.md and _QUEUE8.md):
1. The profile is `MEETINGS`, capability/tool namespace `meetings`. "Meeting Memory" is the
   user-facing product label. Granola is only an import and bridge source, not the product boundary.
2. The Granola local-cache reader is not core Loom. It is an external script that discovers and
   extracts desktop-local cache data, normalizes it, and calls the official Loom command. The public
   interface is `loom meetings import --input-profile <profile> --input <snapshot.json>
   [--dry-run] [--report-format text|json]`.
3. Studio permissions and PEP are the real access-control model. Meetings must not invent a
   parallel privacy system, but it must define meeting-specific redaction, retention, evidence, and
   export behavior.
4. Native meeting capture, live transcription, audio processing, and a notes editor are out of this
   profile pass. Ingest existing source artifacts first.

Work through, in order:
1. Meetings core schema (tasks 440a / 905): pin the canonical operation payloads and projection
   records for sources, meetings, spans, annotations, extraction runs, import runs, fidelity reports,
   and redaction records. Keep every payload grounded in 0061 envelopes and stable identity aliases.
   Define the canonical CBOR/vector conformance cases that will prove the schema.
2. Official import command and interchange shape (task 735): design the `loom meetings import`
   command contract, input schema, output schema, dry-run behavior, idempotency keys, error/fidelity
   report fields, and batch semantics. The command must accept normalized meeting snapshots from
   API importers and external scripts without exposing Granola-specific cache details as Loom API.
3. Granola API importer mapping (task 735): pin the API-backed importer through 0012. Use source
   facts from the current Granola API docs: list notes with cursor/page filters, get note with
   `include=transcript`, folders with parent ids, note title/owner/timestamps/web URL/calendar/
   attendees/folders/summary/transcript items, and personal/public note scopes. If the source shape
   does not fit Meetings, fix Meetings rather than hiding the mismatch in the importer.
4. External local-cache script contract (task 736): define what a recommended script may do, what it
   must submit to Loom, and what Loom rejects. Do not make cache file paths, cache schemas, or
   source-app private details part of Loom's stable public contract.
5. Redaction propagation (tasks 440b/905): specify how redaction invalidates or degrades document,
   files, search, vector, graph, accepted memory, exports, and conformance fixtures.
6. Extraction review and vocabulary (task 440c): pin suggested/accepted/rejected annotation
   lifecycle, entity merge, vocabulary proposal/accept/reject, source-span requirements, and
   promotion preconditions.
7. Cross-profile promotion (MEETINGS §11): define how accepted tasks, decisions, questions,
   artifacts, references, and summaries promote into tickets, pages, and lifecycle scopes. Promotion
   must be explicit, auditable, and permission-checked. No transcript extraction silently edits a
   page, creates a blocking ticket, or changes a lifecycle scope.
8. Surfaces (task 615): design the Meeting Details, Extraction Review, Memory Graph, Meeting Search,
   Import Coverage, and Access Audit apps as projections over the profile model and 0043 app
   mechanism. Decide which golden renders and app fixtures belong in conformance once SURFACES has
   fixture support.
9. Bridge and cutover (task 737): define one-way Granola bridge checkpoints, mirror mode, source
   deletion/disappearance handling, cutover, and fidelity reporting. There is no write-back to
   Granola.
10. Queue and dogfood sync: update MEETINGS.md, ADOPTION.md, SURFACES.md, _QUEUE8.md, and the
    plan.loom dogfood store/mirror with every decision and task-state change. Remove completed rows
    from _QUEUE8.md only after the owning spec records the source-backed or design-pinned status.

Ask owner decisions one batch at a time in the required Question / Context / Examples / Options /
Recommendation / Consequence-of-deferring format. Record decisions immediately in the owning spec
and _QUEUE8.md before continuing. At session end, append one batch decision entry to
`queue8.decisions`, append resolution events to `queue8.questions` for closed questions, and update
the touched sections of `plan/queue8.md` according to the shared dogfood-store sync rules at the
top of this file.
```
