# Meetings - Source-Grounded Meeting Memory

**Status:** Target design. **Version:** 0.1.0-target.
**Capability:** `meetings`.

This document defines the Meetings Studio profile on top of Loom. Granola is an import and bridge
source for the profile; it is not the product boundary. The profile owns durable meeting sources,
transcript spans, summaries, extracted facts, decisions, action items, entity memory, review
workflows, and human surfaces for source-grounded recall. Assistant-generated answers may cite
Meetings records, but assistant execution belongs to the GraphRAG, search, and inference specs.
"Meeting Memory" is the user-facing product label; `meetings` is the capability and tool namespace.

Meetings exist because meeting memory is not just a folder of transcripts. People and agents need
stable source identity, source-span evidence, permissions, relation graphs, semantic recall,
reviewable extraction, and audit history that survive source-tool churn. Granola notes are a strong
first source because they provide notes, summaries, attendees, folders, calendar context, and
transcripts, but the same profile can later ingest Zoom transcripts, calendar notes, interviews,
guided capture sessions, and support calls.

## 1. Contract Boundaries

The design builds on these contracts:

- `0061.md` defines the shared operation substrate: envelope, sequencer, durable cursors, order
  tokens, conflict records, annotations, entity versioning, projections/views, rich body model,
  cross-profile references, and search projection requirements.
- `SURFACES.md` defines the human experience layer: apps render projections, writes call profile
  tools, structured human choices use elicitation, and open-ended intent hands off to the assistant.
- `LIFECYCLE.md` defines cross-profile choreography. Meeting evidence may enter a feature,
  incident, design, or customer lifecycle by explicit edges.
- `ADOPTION.md` defines enterprise import, coexistence, identity mapping, fidelity reports,
  effective-access review, retention, and procurement requirements.
- `0012-interchange.md` defines the reusable source-backed import/export report, fidelity issue,
  normalized batch, checkpoint, and archive manifest contracts used by Meetings import.
- `JIRAISH.md`, `PAGES.md`, `SLACKISH.md`, and `DRIVEISH.md` define the Studio pattern used
  here: local-first replicas, blind cloud by default, keyed workers where content-aware compute is
  required, operation logs as source of truth, and import through the 0012 interchange layer.
- `0017-vector-layer.md`, `0033-search-layer.md`, and `0040-graphrag.md` define vector, search, and
  graph-backed recall surfaces that may read Meetings evidence.

This document does not make a blind host able to read transcripts, summarize meetings, resolve
people, or extract facts. Those require a key-holding client or a tenant-controlled keyed worker.

## 2. Product Model

A Meetings organization exposes source-grounded meeting memory:

```text
meetings
  sources
  meetings
  transcript-spans
  note-bodies
  summaries
  attendees
  folders
  calendar-links
  annotations
  facts
  relations
  decisions
  tasks
  questions
  vocabulary
  extraction-runs
  import-runs
  fidelity-reports
  watches
  audit
  retention
```

The user-facing contract is:

- source snapshots are preserved by digest and external identity;
- meeting identities are stable across re-imports and source renames;
- transcript spans and note-body ranges are first-class evidence;
- imported source-provided annotations are committed as observed meeting data;
- generated annotations remain suggested until accepted by policy or review;
- accepted facts and relations can be queried without rereading the whole transcript;
- source spans are available for citation through governed read APIs;
- imported access scopes, sharing boundaries, and source limitations are visible;
- retention and redaction can remove sensitive payloads while preserving audit facts;
- agents can search, summarize, extract, propose links, and draft follow-up work under scoped grants.

Meetings is not a clone of Granola. It does not define meeting capture, live transcription, audio
processing, or a notes editor. It is the Studio profile for durable, source-grounded meeting memory.

## 3. Cloud and Encryption Model

The same Studio cloud topologies apply.

```text
Private meeting memory:
  Local Loom replicas hold keys and full meeting content.
  Loom Cloud is blind storage, operation sequencing, notification, and opaque coordination.
  Extraction, search, summaries, and agents run locally or in a tenant-controlled keyed worker.

Managed enterprise meeting memory:
  A keyed service runs inside the tenant trust boundary.
  It can index, extract, summarize, run review workflows, and host agents.

Hybrid meeting memory:
  Blind Loom Cloud stores canonical encrypted history.
  Selected organizations, folders, or meetings are mirrored to keyed compute replicas for approved jobs.
```

A blind remote sees envelope metadata only. It may sequence operations, enforce operating mode,
enforce per-kind agent budgets, and emit wakeups. It cannot read transcript text, attendee names
inside payloads, summary bodies, extraction evidence, semantic vectors, or assistant provenance
records that cite Meetings evidence.

## 4. Storage Layer

The operation log is the source of truth. Tables, graphs, files, search indexes, vectors, and
dashboards are projections.

```text
meetings root
  operation log root
  source index root
  meeting index root
  span index root
  annotation index root
  fact index root
  relation graph root
  vocabulary index root
  extraction-run index root
  import-run index root
  audit index root
```

Required projections:

| Projection | Data | Role |
| --- | --- | --- |
| document | source records, meeting records, spans, annotations, extraction runs | deterministic read and update by stable id |
| files | raw source snapshots, readable markdown exports, transcript JSONL, sidecars, fidelity reports | audit source and external inspection |
| graph | meetings, people, organizations, topics, decisions, tasks, questions, relations, evidence edges | multi-hop memory and evolution over time |
| vector | embeddings for spans, summaries, facts, decisions, tasks, questions, entities | semantic recall and fuzzy entity lookup |
| search | titles, summary text, transcript text, labels, aliases, accepted facts, evidence snippets | keyword lookup, facets, highlighting |
| SQL/dataframe | meeting tables, attendee tables, extraction quality, task counts, decision timelines | reports and analytics |
| ledger | sensitive reads, accepts, rejects, redactions, exports, connector events | audit and compliance |

Meeting identity is a stable `meeting_id`. External ids, such as Granola `not_...` note ids, are
aliases bound to `meeting_id` through the 0061 stable identity model. A source may have multiple
aliases: Granola note id, Granola web URL, calendar event id, imported file path, or user-provided
meeting key. Aliases never replace the stable id.

## 5. Operation Log

Operation kinds:

```text
source.connected
source.updated
source.disconnected
source.snapshot_observed
meeting.created
meeting.updated
meeting.alias_bound
meeting.deleted_at_source
span.observed
span.redacted
note.body_updated
summary.observed
annotation.proposed
annotation.accepted
annotation.rejected
annotation.superseded
entity.created
entity.merged
relation.proposed
relation.accepted
relation.rejected
fact.proposed
fact.accepted
fact.rejected
task.promoted
decision.promoted
question.promoted
vocabulary.proposed
vocabulary.accepted
vocabulary.rejected
extraction.run_started
extraction.run_completed
import.run_started
import.run_completed
fidelity.reported
retention.applied
audit.recorded
```

Each operation carries the 0061 canonical envelope. Source payloads are content-addressed and
referenced by digest from operations. Derived operations carry the source digests, extractor version,
model id, prompt version, and policy id that produced them.

Owner decision 2026-07-07: Meetings uses a hybrid schema boundary. Identity, evidence, redaction,
and import runs have strict operation-specific payloads. Source snapshots may
carry generic normalized sidecars when the source has extra fields that should be preserved without
becoming stable Meetings operation vocabulary. Projection records are defined separately from
operation payloads and are rebuildable from the operation log.

## 6. Source and Span Model

### 6.1 Source Record

```text
source_id
source_type: meeting_note | transcript | calendar_event | imported_file | connector_output
source_system: granola | zoom | google_meet | teams | calendar | guided_capture | other
external_id
external_url
source_digest
observed_at
source_created_at
source_updated_at
owner_principal
access_scope
sensitivity
retention_policy
import_run_id
```

The source record identifies the imported or connected artifact. It is not the normalized meeting.
A single meeting may have several source records: an API note, a local cache snapshot, a CSV row, and
a later connector refresh.

### 6.2 Meeting Record

```text
meeting_id
title
time_range
calendar_event_ref
owner_principal
attendee_refs
folder_refs
source_refs
current_source_digest
summary_ref
status: active | deleted_at_source | redacted | retained_metadata_only
created_at
updated_at
```

The current meeting record is a projection over observed sources and user edits. Source snapshots
remain available by digest until retention policy removes them.

### 6.3 Span Record

```text
span_id
meeting_id
source_id
span_kind: transcript_entry | transcript_chunk | summary_range | note_body_range | metadata_field
range: transcript entry id, timestamp range, byte range, or block range
speaker_ref
speaker_source
diarization_label
text_digest
language
provenance
redaction_state
```

Transcript entries are the preferred spans for meeting evidence. Long summaries and notes are chunked
deterministically after source normalization. A span can be redacted without deleting the fact that
the span existed.

Owner decision 2026-07-07: span identity is split by provenance. Raw source spans use a deterministic
source locator derived from `meeting_id`, `source_id`, `span_kind`, normalized source range, and source
text digest. Derived chunk spans use allocated stable ids and retain a pointer to the source spans and
chunking policy that produced them. This keeps source transcript entries idempotent across API and
cache imports while allowing chunking policy changes without rewriting source identity.

## 7. Annotation and Vocabulary Model

Meetings uses a fixed core annotation schema with an open organization vocabulary. The core schema
keeps query semantics stable; the open vocabulary lets meeting corpora accumulate product names,
customer names, incidents, code names, and local terms without a schema migration.

### 7.1 Annotation Record

```text
annotation_id
meeting_id
source_spans
kind
label
normalized_id
attributes
confidence
evidence_ref
extractor
status: observed | suggested | accepted | rejected | superseded | merged
created_at
accepted_by
accepted_at
```

The source span is mandatory. A tag without source evidence is not promotable to an accepted memory
fact. Evidence may be a short excerpt for display or a digest pointer when duplicating text would
violate retention policy.

### 7.2 Core Kinds

| Kind | Purpose | Important attributes |
| --- | --- | --- |
| Meeting | The call, note, interview, or session itself | meeting_id, source_system, time_range |
| Person | Human participant or mentioned person | email, role, source |
| Organization | Company, team, customer, vendor | domain, org_type |
| Location | Physical or virtual place | location_type |
| Time | Date, period, deadline, recurrence | start, end, timezone, certainty |
| Task | Action item or requested work | owner, due_at, status, priority |
| Decision | Accepted choice or ruling | decided_by, decided_at, state |
| Question | Open question needing answer | owner, status |
| Idea | Proposal, concept, or opportunity | maturity, first_seen_at |
| Topic | Discussion subject | parent_topic, domain |
| Project | Ongoing initiative | status, owner |
| ProductArea | Product, feature, component, or system | area_type, repo_path |
| Artifact | Linkable object | url, path, artifact_type |
| Reference | External URL, citation, or named source | url, retrieved_at |
| Risk | Possible negative outcome | severity, likelihood |
| Constraint | Requirement or boundary condition | scope, hardness |
| Requirement | Desired or mandatory capability | priority, status |
| Metric | Number or measured target | value, unit, period |
| Status | State update | subject, state, as_of |
| Sentiment | Opinion, preference, concern, support | polarity, intensity, holder |
| CodeSymbol | Code-level entity | language, repo_path, symbol_kind |
| DomainTerm | Corpus-specific term not yet modeled | definition, aliases |

Core kinds should change rarely. Subtypes, aliases, labels, and organization vocabulary records carry
local variation.

### 7.3 Relation Kinds

| Relation | Source to target | Meaning |
| --- | --- | --- |
| attended_by | Meeting to Person | Person attended or was listed as attendee |
| mentions | Meeting or Span to any entity | Source span mentions the entity |
| discusses | Meeting or Span to Topic, Idea, or Project | Sustained discussion |
| assigned_to | Task to Person | Person owns the task |
| due_at | Task to Time | Task deadline |
| decides | Meeting to Decision | Meeting produced or records a decision |
| decision_about | Decision to Topic, Project, or ProductArea | Subject of the decision |
| asks | Meeting or Person to Question | Question was raised |
| answers | Decision or Span to Question | Later answer to a question |
| introduces | Meeting or Person to Idea | First observed introduction |
| evolves_to | Idea, Decision, or Requirement to later state | Later state refines earlier state |
| blocked_by | Task or Project to Risk, Constraint, or Question | Progress depends on resolving target |
| supports | Span or Artifact to Decision, Idea, Requirement, or Fact | Evidence supports target |
| contradicts | Span or Artifact to Decision, Idea, Requirement, or Fact | Evidence conflicts with target |
| part_of | Task, Topic, or ProductArea to parent | Hierarchical grouping |
| references | Meeting, Artifact, or Span to Reference or Artifact | Explicit citation or link |

Relations are accepted memory only when their endpoints exist and their source spans are available or
retained as redacted evidence records.

### 7.4 Import and Extraction Rules

Meeting import is automatic. A normalized source snapshot is parsed directly into the Meetings data
model in one import operation. It is not staged as a draft and does not require a separate acceptance
step before it becomes visible through `loom meetings`, MCP, or hosted Meetings reads.

Imported fields from external meeting apps become source-backed Meetings records:

- note or meeting identity becomes a `SourceRecord` and `MeetingRecord`;
- title, owner, calendar event, attendees, folders, and source timestamps become meeting metadata;
- summaries and raw sidecars are retained by digest when supplied by the importer;
- transcript entries become `SpanRecord` values;
- source-provided tasks, action items, topics, decisions, questions, risks, artifacts, references,
  and generic annotations become `AnnotationRecord` values with status `observed`;
- when a source-provided structured item lacks an exact transcript span, the importer creates a
  deterministic metadata span so the annotation still has source provenance.

Users and agents may edit, add, remove, or rearrange imported meeting data after import through the
normal profile operation surface. Those edits are ordinary Meetings writes with principal
attribution, revision history, policy checks, and audit records. They are not modeled as import
approval.

Model extraction is a separate pipeline over already imported source snapshots:

1. Segment source text into deterministic spans.
2. Apply rule-based extraction for metadata, attendees, calendar links, URLs, emails, file paths,
   dates, obvious action markers, and explicit issue or page references.
3. Apply model extraction for open labels, facts, decisions, questions, risks, requirements, and
   relation proposals.
4. Resolve entities by stable keys first, aliases second, normalized labels third.
5. Store all model output as `suggested` unless an explicit promotion policy covers that kind.
6. Project observed or accepted annotations to graph, vector, search, SQL, and ledger projections.
7. Propose vocabulary additions from repeated `DomainTerm` labels, alias clusters, and recurring
   subtypes.

Model output is never the source of truth. It is a proposal derived from source spans and extractor
metadata.

## 8. Import from Granola

Import runs through the 0012 interchange layer and uses the Studio import pattern from JIRAISH:
mapping table, identity handling, one-way bridge, and fidelity report. Granola import stress-tests
the meetings model; if real source data does not fit, the profile model is fixed rather than
forcing the importer to hide the mismatch.

External API facts checked on 2026-07-07:

- The Granola API lists notes at `GET /v1/notes` with pagination and filters including
  `created_before`, `created_after`, `updated_after`, `folder_id`, `cursor`, and `page_size`.
  Source: `https://docs.granola.ai/api-reference/list-notes`.
- The Granola API retrieves one note at `GET /v1/notes/{note_id}` and accepts
  `include=transcript`.
  Source: `https://docs.granola.ai/api-reference/get-note`.
- The note response includes title, owner, timestamps, web URL, calendar event, attendees, folder
  membership, summary text, summary markdown, and transcript items.
  Source: `https://docs.granola.ai/api-reference/get-note`.
- The Granola API lists folders at `GET /v1/folders` and exposes folder parent relationships when
  present.
  Source: `https://docs.granola.ai/api-reference/list-folders`.
- API keys carry personal-notes and public-notes access scopes. The documented API is the primary
  headless import path.
  Source: `https://docs.granola.ai/introduction`.
- The API only returns notes that have a generated AI summary and transcript. Notes still processing
  or never summarized do not appear in list responses, and get-note returns `404` for them.
  Source: `https://docs.granola.ai/introduction`.
- Rate limits are documented as burst capacity 25 requests, a 5-second window, sustained
  5 requests/second, and `429 Too Many Requests` when exceeded.
  Source: `https://docs.granola.ai/introduction`.

### 8.1 Source Priority

| Source | Role | Fidelity stance |
| --- | --- | --- |
| Granola normalized snapshot | Primary CLI and MCP-assisted import path | Complete for fields present in the submitted snapshot; missing source visibility is a fidelity gap |
| Granola local cache script | External personal desktop fallback | Outside core Loom; extracts desktop-local artifacts and calls the official Meetings import command |
| Granola CSV export | Degraded manual backfill | Useful for historical export; missing deleted or unsummarized notes when source omits them |
| Granola MCP | Interactive assistant connector | Can submit one note or a small normalized batch through the official import command |

The durable Loom-owned path is normalized import execution, not live Granola API fetching. Local
cache extraction is intentionally outside the core Loom product because cache layout, availability,
and platform paths are source-application details. A local script may read Granola desktop cache
files, normalize one note or a batch into the Meetings interchange shape, and submit the result
through:

```text
loom meetings import <store> <workspace>
                     --input-profile <profile> --input <snapshot.json>
```

The input profile selector names the normalized input shape, such as `generic`, `granola-api`,
`granola-app`, `granola-mcp`, or `csv`. Loom may validate a Granola-shaped normalized snapshot
without owning how that snapshot was extracted. The external script owns Granola-specific discovery
and cache parsing; a Granola MCP session owns interactive source access; a filesystem importer owns
file discovery and byte extraction. Loom owns validation, canonical import operations, idempotency,
fidelity reporting, permissions, redaction, projections, and conformance.

MCP source-backs the same profile-specific import execution through `meetings_import_snapshot`.
The tool accepts a workspace selector, `input_profile`, normalized snapshot bytes, and an optional
dry-run flag. Workspace-scoped MCP sessions elide the workspace argument. The tool delegates to
`loom-interchange-io::import_meetings_bytes`, so CLI and MCP imports share validation, retained
source payload materialization, audit behavior, idempotent merge semantics, revision-row updates, and
the structured 0012 import report summary.

Owner decision 2026-07-07: the official command is profile-specific now, before a generic
`loom.interchange` facade exists:

```text
loom meetings import <store> <workspace>
                     --input-profile <generic|granola-api|granola-app|granola-mcp|csv>
                     --input <snapshot.json>
                     [--dry-run] [--report-format text|json]
```

The command follows 0012 principles: explicit conversion, deterministic mapping, source fidelity,
capability gating, and a returned report. `--input-profile` selects the accepted input shape.
`--input` points to the normalized snapshot document or `-` for standard input. `--report-format`
selects the import report presentation. Connector credentials, API pagination, local cache
discovery, filesystem discovery, and MCP-assisted source discovery are outside this command.

### 8.1a Normalized Snapshot Shape

All importer and external-script inputs lower into one batch envelope:

```text
meeting_import_snapshot:
  snapshot_version
  profile
  source_system
  source_scope
  observed_at
  coverage: complete | partial | degraded
  source_cursor
  source_sidecar_digest
  folders[]
  items[]
```

`items[]` contains normalized meeting artifacts:

```text
meeting_import_item:
  source_entity_id
  source_digest
  source_created_at
  source_updated_at
  source_state: active | deleted_at_source | redacted | retained_metadata_only
  title
  owner
  external_url
  calendar_event
  attendees[]
  folder_refs[]
  summary_text
  summary_markdown_digest
  transcript_spans[]
  annotations[]
  tasks[]
  action_items[]
  topics[]
  decisions[]
  questions[]
  risks[]
  artifacts[]
  references[]
  source_sidecar_digest
```

`annotations[]` is the generic structured-item shape:

```text
annotation_id
kind
label
source_span_ids[]
normalized_id
confidence_ppm
evidence_digest
extractor
```

The convenience arrays `tasks[]`, `action_items[]`, `topics[]`, `decisions[]`, `questions[]`,
`risks[]`, `artifacts[]`, and `references[]` use the same shape without `kind`; the field name
selects the core annotation kind. All imported structured items are `observed`, not `suggested` or
`accepted`.

The stable input contract is the normalized shape above. Profile-specific sidecars preserve source
fields that Loom should retain for audit or later reprocessing without promoting them to Meetings
operation vocabulary. A Granola normalized importer, a local-cache script, and an assistant using a
Granola MCP connector may all submit this shape, provided required source identity, digest,
coverage, `source_state`, and span-order fields are present. `source_state` defaults to `active`
when omitted by older normalized snapshots. Deletion is explicit only: absence from an import does
not change meeting status.

Owner decision 2026-07-07: Granola API omissions caused by API visibility limits, missing generated
summaries, missing transcripts, processing state, access denial, or rate limits are coverage gaps,
not source deletions. If a Granola MCP session, filesystem importer, or local app/cache importer
discovers a concrete meeting or note id that the API did not expose, Loom records that as a source
observation from that input profile and reports the API gap. A missing item becomes
`meeting.deleted_at_source` only when a source observation confirms disappearance or deletion.

Owner decision 2026-07-07: raw Granola API, app, MCP, and filesystem sidecars are retained by digest
with no source-imposed retention ceiling. They remain readable until the Loom user, organization admin,
retention policy, legal-hold policy, or explicit redaction removes or shreds them. Source product
plan limits and source-system legal holds do not shorten or extend the user's Loom-side audit record.

### 8.1b Dry Run

`--dry-run` performs the full validation and fidelity pass without sequencing operations, writing
projections, or changing aliases. It returns the same report shape as a committed import, with
operation counts marked as planned rather than sequenced. Dry-run validates required fields,
deterministic span locators, principal mapping outcomes, duplicate/idempotency outcomes, missing
transcript coverage, suppressed emails, source sidecar digests, and unsupported profile fields.

### 8.2 Mapping Table

| Granola source entity | Meetings target | Notes / degrade path |
| --- | --- | --- |
| Organization or API key scope | `source.connected` with access scope | Personal and public scopes recorded; effective coverage appears in fidelity report |
| Folder | folder reference and alias | Parent folder id preserved when available |
| Note id | `meeting.alias_bound` to `meeting_id` | Note id is a stable external alias, not the meeting identity |
| Note title | meeting title and searchable field | Empty titles become untitled with source id disambiguation |
| Owner | principal mapping | Unmapped owners become inactive imported principals per ADOPTION identity rules |
| Attendees | Person entities plus `attended_by` edges | Emails are retained only when policy allows |
| Calendar event | calendar reference and meeting time range | Calendar event id becomes an alias when present |
| Summary text | summary record and span source | Stored as source-derived text, not accepted fact |
| Summary markdown | note body using the 0061 rich body model when converted | Native markdown snapshot retained by digest |
| Transcript item | span records | Speaker source, diarization label, start time, and end time preserved |
| Web URL | source alias and Reference annotation | URL access does not imply Loom grant |
| Deleted or missing note on later sync | `meeting.deleted_at_source` | Loom retention policy decides whether payload remains |
| API error or access denial | fidelity report entry | No synthetic deletion unless source confirms disappearance |

### 8.2.1 Granola and Meetings Current Coverage Matrix

The current broad Granola fixture proves the normalized `granola-app` import boundary for complete,
partial, and degraded source observations. Dedicated `granola-api`, `granola-mcp`, and `csv`
fixtures execute the remaining input-profile selectors. Connector-driven source-deletion discovery
and shared retained-source policy enforcement are outside the normalized importer: the importer
requires explicit `source_state` and retained payloads remain subject to Loom-side policy work.

| Source field or entity | Current handling | Fixture coverage |
| --- | --- | --- |
| Source system/profile | Import run profile and source records | Broad `granola-app` fixture covered; API/MCP/CSV profile execution covered |
| Source scope, connector session, account, folder scope | Import run/source metadata and retained source sidecar | Broad normalized coverage |
| Granola note id | External alias for meeting identity | Covered |
| Note title | Meeting title/search field | Covered |
| Explicit source state | Meeting status; missing observations never imply deletion | Covered for active, deleted-at-source, retained-metadata-only, and invalid-value rejection |
| Folder id and parent folder | Folder refs on meeting; source folder details retained in sidecar | Broad normalized coverage |
| Owner | Owner principal reference on source and meeting | Broad normalized coverage |
| Attendees and attendee emails | Attendee refs on meeting; rich attendee details retained in sidecar | Broad normalized coverage |
| Calendar event id and meeting time range | Calendar event ref is first-class; time range is retained in sidecar | Broad normalized coverage with degraded time-range handling |
| Summary text | Retained `summary.txt` payload and meeting summary ref | Broad normalized coverage |
| Summary markdown | Native markdown digest is accepted; rich-body conversion target remains target | Partially covered |
| Transcript items | Span records with order, locator, speaker source, language, and retained transcript JSONL | Broad normalized coverage |
| Transcript timestamps | Timestamp-like locator retained; structured start/end fields remain target | Broad normalized coverage with degraded structured-time handling |
| Speaker labels and diarization source | Raw source speaker retained; principal mapping target remains target | Broad normalized coverage |
| Source web URL | Retained sidecar and reference annotation | Broad normalized coverage |
| Raw source sidecars | Retained by digest and readable through source-read | Broad normalized coverage |
| Missing generated summary | Fidelity coverage gap | Broad normalized coverage |
| Missing transcript | Fidelity coverage gap; prior transcript retention policy target | Broad normalized coverage |
| API visibility limit, access denial, rate limit | Fidelity coverage gap and retry/checkpoint metadata | Broad normalized and API-profile fixture coverage; live API fetching remains outside Loom |
| Deleted or disappeared source note | Explicit `source_state: deleted_at_source`; missing observations are not deletion | Covered |
| Local cache encrypted-auth path | External script extraction path | Source-backed by script, not fixture-complete |
| CSV degraded import | Degraded manual backfill | Covered by CSV profile execution fixture |
| MCP single-note import | MCP-assisted normalized snapshot | Covered by MCP profile execution fixture and MCP tool |
| Idempotent retry and existing snapshot merge | Duplicate-safe import behavior with canonical record ordering | Covered by execution-fidelity vector |
| Import checkpoints and resume state | Import-run cursor, retry window, observed ids, coverage gaps, resume state, and persisted 0012 checkpoint record | Broad normalized coverage with checkpoint readback |
| Redaction and retention policy | Loom-side source-payload handling; source-system legal holds are not inherited | Target |
| Cross-profile promotion hooks | Explicit operations that create or link target-profile records from Meetings annotations | Partial hook coverage; target-profile writes remain target |

### 8.3 Local Cache Script Contract

The local cache reader is a script outside the Loom binary. `scripts/granola-cache-import.py`
source-backs the recommended local-cache adapter: it reads one JSON cache file, many JSON files, a
directory of JSON files, or a zip/tar/gzip archive containing JSON files; accepts the top-level
`cache` field as either nested JSON text or an object; reads `state.documents`,
`state.sharedDocuments`, and `state.transcripts` when present; emits the normalized Meetings
snapshot shape; and submits that snapshot through `loom meetings import --input-profile
granola-app`. It also supports the Granola 7 encrypted-auth path described by upstream issue 24:
unwrap `storage.dek` through the macOS Keychain `Granola Safe Storage` service, decrypt
`supabase.json.enc`, use the resulting WorkOS bearer token for `POST /v2/get-documents`, optionally
fetch transcripts through `POST /v1/get-document-transcript`, and submit the normalized
`granola-api` snapshot through the same official import command. `specs/studio/fixtures/granola-cache-v1.json`
pins the exercised cache shape. Granola desktop cache discovery remains outside Loom's stable
contract.

Current source also backs retained source payload readback through
`loom meetings source-read <store> <workspace> <source_id> <leaf> [--out <file>]`. The command reads
only the importer-owned retained payload leaves `source.json`, `summary.txt`, and `transcript.jsonl`
from `.loom/meetings/{profile}/sources/{source}/`, preserving the raw bytes written by
`loom meetings import`.

Local-cache rules:

- The script must preserve original note ids, timestamps, transcript order, and raw speaker source
  values in the submitted snapshot.
- The display labels `Me` and `Them` are rendering conveniences only; raw source values stay in the
  submitted span payload.
- A later cache run with a missing transcript must submit an observation of missing coverage, not a
  synthetic delete.
- Every cache-derived source snapshot is marked `source_system = granola_cache` and
  `coverage = partial`.
- Loom may reject a cache snapshot that omits required source identity, digest, or span-order
  fields.

### 8.4 History Synthesis

Each source observation creates unconditional import operations carrying:

```text
import_provenance:
  source_system
  source_entity_id
  source_snapshot_digest
  import_run_id
  observed_at
  access_scope
```

Granola does not expose a full edit changelog for every note through the documented API. The importer
therefore records observed snapshots, not fabricated edit events. Meeting history can show what Loom
observed at each sync point, and the fidelity report states that source-internal edit history is not
available unless a future source adds it.

### 8.5 Bridge and Cutover

The bridge is incremental one-way sync from Granola. There is no write-back to Granola.

A bridged Granola source scope is in mirror mode for source snapshots: imports may append new source
observations, but users cannot edit the source payload as if it were native content. Derived
annotations, accepted facts, comments, and promoted tasks live in the Meetings organization and may be
read-write according to policy.

Cutover means the admin stops the connector, records the last checkpoint, and marks the source scope
as retained. It does not turn Meetings into a transcription source. Native meetings capture, if
added later, is a separate source connector.

### 8.6 Fidelity Report

Every import run emits a fidelity report view retained with the run:

```text
fidelity report:
  import_run_id, source_system, scope, timestamps
  source coverage: API | local_cache | CSV | MCP-assisted
  notes: discovered, fetched, skipped, inaccessible, disappeared
  transcripts: present, missing, retained_from_prior_import, redacted
  identities: principals mapped, imported principals created, emails suppressed by policy
  folders: mapped, missing parent, access denied
  summaries: present, missing, converted, retained native markdown only
  extraction: runs started, runs completed, suggestions created, automatic accepts
  degraded items: entity id, field, reason, original payload digest
  rate limits and retry windows observed
```

Runs are idempotent by `import_run_id` plus source entity id and source digest. Re-running a failed
batch must not duplicate meetings, spans, annotations, or aliases.

## 9. Permissions, Privacy, Consent, and Retention

Meeting transcripts are sensitive. The primary access-control model is the shared Studio permission
system: all reads, writes, search, export, promotion, and assistant access must pass the same
principal, grant, and PEP checks used by the rest of Studio. Meetings does not introduce a parallel
privacy model.

A Meetings source configuration must require explicit choices for:

- source connector and account;
- organization or folder scope;
- whether shared notes are included;
- whether attendee emails are retained, hashed, or dropped;
- whether raw transcript text is retained, redacted, or summary-only;
- whether model extraction is allowed;
- whether suggested facts can be auto-accepted by policy;
- retention class for raw transcripts, summaries, accepted facts, and assistant provenance records
  that cite Meetings evidence;
- whether exports are allowed and which principals can run them.

Default policy:

- raw transcripts remain inside the target Loom store;
- decrypted staging files outside the store are forbidden unless the user selects an export path;
- accepted facts retain source-span pointers;
- redaction removes payload text and vectors while preserving audit facts and redacted span records;
- external URLs do not bypass Loom grants;
- assistant provenance records are sensitive audit records because they reveal what evidence an agent
  read.

Consent evidence is intentionally lightweight: when a source or admin policy provides consent or
recording-state metadata, Meetings preserves it as source metadata. Meetings does not infer
participant consent from transcript presence and does not block core profile design on proving source
consent semantics that the source system does not expose.

### 9.1 Redaction Propagation

Redaction is a sequenced operation that targets spans, source payloads, annotations, or derived
records. Applying a redaction must make every affected projection deterministic:

- document records retain identity, source, and redaction state but remove redacted payload text;
- files projection snapshots either omit redacted content or emit a redacted marker sidecar;
- search and vector projections remove or invalidate entries derived from redacted text;
- graph evidence edges remain only as retained metadata when their evidence text is redacted;
- accepted facts and relations that no longer have live evidence become retained-metadata-only until
  reviewed or superseded;
- assistant provenance records preserve that evidence was consulted without retaining redacted payload
  text;
- exports must either omit redacted material or fail if the requested export cannot represent the
  redaction faithfully.

## 10. Surfaces

Meetings contributes apps and views to `SURFACES.md` without changing the app mechanism:

Current source backs the Meeting Memory app catalog at the contract level in
`loom-substrate::surfaces`. `meeting_memory_surface_catalog` returns deterministic app definitions
for Meeting Details, Memory Graph, Extraction Review, Meeting Search, Import Coverage, and Access
Audit, each with a `ui://{workspace}/mcp/apps/{app-id}` resource, projection refs, read/write tools,
elicitation schemas, prompt-handoff refs, and change subscription refs.

Current MCP source also backs the app bundles themselves. `crates/loom-mcp` exposes built-in
template-backed Apps for the six Meetings views and injects a shared `loom.meetings` binding with
workspace, profile id, app definition, meeting list, selected meeting detail, projection outputs,
extraction review, import-coverage status, and access-audit status. Meeting Details supports
path-shaped imported ids through
`ui://{workspace}/mcp/apps/meeting-details/meeting/{meeting_id...}`, so imported ids such as
`meeting/note-1` remain addressable without lossy rewriting. App actions are limited to promoted
MCP tools, including annotation accept/reject, vocabulary/entity-merge review writes, import
snapshot submission, and explicit promotion tools. The app test suite renders the source-backed
bundle over data imported by `meetings_import_snapshot`, and `just verify-apps` includes visual
coverage for all six Meetings apps.

Import Coverage currently renders source-backed meeting-list and `meetings_import_snapshot`
availability. A dedicated import-run read projection, rich import execution links, and export
workflows remain target work until corresponding promoted tools exist. Access Audit currently shows
the shared Studio ACL boundary and marks the Meetings-specific audit projection as target; it does
not fabricate an audit log view.

| App | What it renders | Writes / elicitations |
| --- | --- | --- |
| Meeting Details | One meeting: source snapshots, summary, transcript spans, attendees, extracted items, history | accept or reject annotations, redact span, link to page or issue |
| Memory Graph | People, meetings, topics, decisions, questions, tasks, and evidence edges | prompt handoff for explain cluster; explicit edge accept |
| Extraction Review | Suggested facts, decisions, tasks, risks, and vocabulary proposals | accept, reject, merge entity, promote vocabulary |
| Meeting Search | Search and semantic recall over summaries, spans, facts, and decisions | open in Meeting Details; ask assistant with selected evidence |
| Import Coverage | Connector health, fidelity reports, missing transcript counts, scope coverage | rerun import, change scope, stop bridge |
| Access Audit | Who can see a meeting, source, span, fact, or assistant provenance record | export with policy elicitation |

Assistant-facing tools read Meetings sources and spans through the same search and profile-read
contracts as other clients. Meetings does not define assistant answer generation; that contract lives
in GraphRAG, search, and inference. The assistant does not receive a special bypass around profile
policy.

## 11. Cross-Profile Promotion

Observed or accepted meeting memory can create or link ordinary Studio entities:

- observed or accepted `Task` annotations can promote to JIRAISH issues through explicit
  `task.promoted`;
- observed or accepted `Decision` annotations can append to the Decision log and link to pages or
  issues;
- observed or accepted `Question` annotations can become open items in a lifecycle scope;
- observed or accepted `Artifact` and `Reference` annotations bind through the 0061 reference
  grammar;
- meeting summaries can be embedded into PAGES pages by reference, not copied by default.

Promotion is explicit. Extraction from a transcript never silently creates a blocking issue, edits a
page, or changes lifecycle scope. The surface may offer an elicitation such as "create issue from
these three tasks", and the resulting write is a normal profile operation.

Current source backs the promotion hook as a canonical `PromotionRecord` in the Meetings profile
snapshot. A promotion cites one observed or accepted annotation, records the operation kind
(`task.promoted`, `decision.promoted`, or equivalent profile vocabulary), target profile, target
entity ref, promoting principal, and timestamp. Promotion targets validate through the shared 0061
Studio promotion target contract: `tickets` requires `ticket:`, `decision-log` requires
`decision:`, `lifecycle` requires `lifecycle:`, `references` requires `reference:` or `artifact:`,
and `pages` requires `page:`. MCP exposes record-only promotion through `meetings_add_promotion`.
MCP also source-backs explicit target writers for the backed targets:
`meetings_promote_task_to_ticket` creates a Tickets record from an observed or accepted `Task`
annotation, records the source annotation as the ticket external identity, then records the
Meetings promotion against the created ticket UUID; `meetings_promote_decision_to_decision_log`
appends a decision-log entry through the Ledger facet and records a `decision:` promotion target;
and `meetings_promote_question_to_lifecycle` creates a lifecycle instance from an observed or
accepted `Question` annotation and records a `lifecycle:` promotion target.
`meetings_promote_artifact_to_reference_artifact` and
`meetings_promote_reference_to_reference_artifact` create durable records in the reusable
reference/artifact substrate and record `artifact:` or `reference:` promotion targets.

Each concrete writer prevalidates the promotion envelope before creating the target object where the
target surface can be created by that operation; regression coverage proves invalid task-to-ticket
promotion input does not leave an unpromoted ticket behind. Pages promotion remains reserved until
the Pages workflow owns meeting-summary embedding and page creation semantics.

## 12. Evidence Source Rules

When Meetings records are displayed, promoted, cited, or indexed by another capability, consumers
should prefer evidence in this order:

1. Direct source span.
2. Accepted fact with live source-span evidence.
3. Accepted relation with source-span evidence.
4. Summary record with source references.
5. Suggested annotation.
6. Model-generated commentary.

Whole-source mode wins when the user asks about a specific meeting or asks for totality. Graph-first
mode wins for relation questions, such as who attended meetings about a topic. Vector-first mode wins
for fuzzy recall. Search-first mode wins for exact terms, names, issue keys, and quoted phrases.
Meetings supplies governed evidence ids, spans, summaries, observed or accepted annotations, and
policy checks.

## 13. Implementation Sequence

1. Define Meetings operation payload schemas and projection records over 0061 envelopes.
   Source-backed for core schema records in `crates/loom-substrate/src/meetings.rs` under the
   `studio-meetings` feature.
2. Define the `loom meetings import <store> <workspace> --input-profile
   <profile> --input <snapshot.json> [--dry-run] [--report-format text|json]` command and
   interchange validation path.
   Source-backed for normalized snapshot validation, deterministic source/meeting/span lowering,
   idempotent merge into the Meetings profile snapshot control record, dry-run reporting, and JSON
   or text import reports.
3. Build Granola normalized snapshot import through 0012 with idempotent import runs and fidelity
   reports.
4. Publish the external Granola local-cache script against the official import command.
   Source-backed by `scripts/granola-cache-import.py` and
   `specs/studio/fixtures/granola-cache-v1.json`.
5. Materialize source, meeting, span, and import-run document projections.
6. Add readable files projection for raw snapshots, markdown exports, transcript JSONL, sidecars,
   and fidelity reports.
7. Add deterministic span chunking and vector derivation through the shared Studio reindex queue.
8. Add source-backed structured-item import for source-provided tasks, topics, decisions, questions,
   risks, artifacts, references, and generic annotations.
9. Add annotation proposal, review, and vocabulary promotion for model-generated suggestions.
10. Add graph and search projections for observed or accepted annotations and source spans.
11. Add Meeting Details, Extraction Review, Memory Graph, Meeting Search, Import Coverage, and Access
   Audit surfaces.
   Source-backed for built-in MCP App bundles, `loom.meetings` template binding, app resource
   rendering, path-shaped Meeting Details ids, promoted-tool app action preparation, and visual
   verification. Dedicated import-run reads, export workflow controls, and Meetings-specific audit
   log projection remain target.
12. Add one-way Granola bridge mode with checkpoints, operating-mode enforcement, and cutover.
13. Add cross-profile promotion to JIRAISH, PAGES, and lifecycle scopes.
    Source-backed for the Meetings-side promotion hook and MCP `meetings_add_promotion`.

## 14. Unfinished Tasks

- Promote Meetings-specific import operation payloads beyond normalized source, meeting, span,
  annotation, and import-run records into the official `loom meetings import` command.
- Define the default extraction prompt contract and negative tests for unsupported inference.
- Add conformance fixtures for physical facet writes from materialized projection-output records and
  end-to-end target-profile promotion operations.
- Define retention-class defaults for raw transcripts, summaries, vectors, accepted facts, and
  assistant provenance records that cite Meetings evidence.
- Promote a dedicated Meetings import-run read projection and export workflow tools before exposing
  richer Import Coverage controls in Apps.
- Promote a Meetings-specific access-audit projection before exposing audit-log browsing in the
  Access Audit App.

## 15. Decision Points

Decision Points: none.

## 16. Resolved Decisions

Source-backed status 2026-07-07: `loom-substrate` exposes the `studio-meetings` feature and
canonical schema records for sources, meetings, spans, annotations, import runs, redactions, and
profile snapshots. `loom-interchange` exposes reusable import/export reports,
fidelity issues, normalized import batches, checkpoints, and archive manifests.
`loom-interchange-io` and the CLI expose source-backed filesystem import/export. The modules validate
required source evidence, time ranges, duplicate profile ids, digest-bearing records, input-profile
tags, coverage tags, redaction state, safe archive paths, duplicate source ids, and canonical
encode/decode round trips. `loom-substrate::meetings` also source-backs canonical projection-effect
records, deterministic effect generation, and canonical materialized projection-output records for
the required document, files, graph, vector, search, SQL/dataframe, and ledger projections, including
redaction invalidation/retained-metadata effects.
Canonical vocabulary term records, entity merge records, extraction-review projections over
suggested, accepted, and rejected annotations, annotation accept/reject transitions, and vocabulary
accept/reject transitions are also source-backed. Answer-evidence record types belong to the GraphRAG,
search, and inference work; they are not public Meetings product commands.
`crates/loom-conformance` now carries the `meetings-profile` canonical vector suite for profile
snapshots, projection-effect coverage, projection-output coverage, redaction invalidation,
extraction-review buckets, and review transitions.
`crates/loom-mcp` now source-backs Meetings profile tools over stored
`MeetingsProfileSnapshot` control records: `meetings_list`, `meetings_get`,
`meetings_search`, `meetings_projection_outputs`, `meetings_extraction_review`,
`meetings_accept_annotation`, `meetings_reject_annotation`, `meetings_propose_vocabulary`,
`meetings_accept_vocabulary`, `meetings_reject_vocabulary`, and `meetings_add_entity_merge`. The
protocol conformance runner seeds a canonical snapshot and certifies product-shaped list/get reads,
scoped Meetings search over materialized projection text, projection outputs, extraction review, and
review writes through the in-process MCP host while preserving canonical CBOR hex for exact
projection output and review records.
`crates/loom-hosted` now source-backs matching REST and JSON-RPC adapter methods over the same
stored profile snapshots for product-shaped list/get/search reads, projection outputs, extraction
review, and review writes.
The served `meetings` surface is also source-backed for REST and JSON-RPC:
`loom serve <store> meetings <workspace> --transport rest|json_rpc` opens routes
for list/get/search reads, projection outputs, extraction review, annotation accept/reject,
vocabulary propose/accept/reject, and entity merge writes over the stored snapshot.
The hosted `apply_projection_outputs` operation physically materializes projection outputs into the
deterministic target facets: document, files, graph, search, SQL/dataframe, and ledger. SQL/dataframe
outputs persist into SQL database `meetings/{workspace}` table `meetings_projection_outputs` with
the projection id, projection/action, target reference, entity id, source ids, payload CBOR hex,
record CBOR hex, redaction state, and recorded time. Vector outputs use Loom's built-in vector facet
plus built-in inference/embedding capability.
`loom studio reindex`, MCP `studio_reindex`, vector workspace binding changes, inference instance
updates, and the hosted Meetings apply route now persist idempotent Studio embedding-projection jobs;
missing inference is persisted as `no_engine`, not a terminal skip. The hosted apply route records
vector jobs by projection output id and treats an existing job as already applied on retry. CLI
`loom studio reindex` and per-request local MCP `studio_reindex` drain Meetings vector outputs into
physical vector records when a text-embedding instance is bound, delete invalidated or
retained-metadata vector records, and mark projection-output-level jobs ready. A served Meetings
listener also attaches the same embedding runtime when the daemon can resolve a configured
text-embedding binding for the workspace; otherwise it records the durable `no_engine` job. Persistent
MCP hosts enqueue the same durable job and report `no_engine` unless a process-local inference binding
is available through the local store path. Meeting FTS projections are retrievable through the unified
MCP `search` tool after hosted apply materializes them. Product-shaped CLI, MCP, and hosted list,
get, search, and materialized projection-output readback surfaces are source-backed. The hosted
`materialized_outputs` operation reports the concrete document/file/graph/FTS/SQL-dataframe/ledger
artifacts, physical vector records when present, and durable vector job records produced by
`apply_projection_outputs`; it reports invalidated or retained outputs as materialized when the target
artifact is absent by design. Assistant answer generation belongs to the AI/search owning work rather
than the Meetings product API.
`crates/loom-mcp` also source-backs the built-in Meeting Details, Memory Graph, Extraction Review,
Meeting Search, Import Coverage, and Access Audit MCP Apps. The Apps render through a shared
`loom.meetings` template binding and use only promoted Meetings MCP tools for bridge actions.
Ledger appends are retry-safe by deterministic projection output id. `uldren-loom-cli` tests
source-back physical Meetings vector output draining through the real vector facet when a bound
text-embedding instance is available.
`uldren-loom-protocol-conformance` certifies the served REST and JSON-RPC read and apply routes, plus
the post-apply MCP `search` read, against a real store-backed Meetings snapshot.
`loom-interchange-io::import_meetings_bytes` now source-backs reusable Meetings import execution,
and the CLI delegates `loom meetings import <store> <workspace>
--input-profile <generic|granola-api|granola-app|granola-mcp|csv> --input <snapshot.json>
[--dry-run] [--report-format text|json]` to that shared service. The importer validates the
normalized envelope, lowers sources, meetings, transcript spans, source-provided observed
annotations, deterministic metadata spans, and import-run metadata into a canonical
`MeetingsProfileSnapshot`, derives deterministic meeting/span/annotation ids when absent, merges
records by stable id with any existing snapshot, writes the snapshot through the same control record
read by hosted and MCP surfaces, records an audit event on committed changes, and returns the shared
0012 import report shape. It canonicalizes imported source, meeting, span, and annotation order
before first persistence, so broad imports retry without a profile-state change caused only by merge
sorting.

Committed imports persist retained source payloads under reserved
`.loom/meetings/{profile}/sources/{source}/...` paths: `source.json` for the submitted source
sidecar, `summary.txt` for source summary text when present, and `transcript.jsonl` for ordered
transcript span text when present. This keeps the canonical profile compact while preserving exact
source reconstruction for retained imports. Committed imports maintain shared substrate revision rows
for changed `meeting:{meeting_id}` records through the generic profile transaction helper without
fabricating source-tool edit events. `loom meetings get`, MCP `meetings_get`, and hosted Meetings get
reads expose stored observed annotations for the selected meeting. Meetings review-write tools
currently persist the snapshot and audit record directly; generic profile-transaction revision rows
for review-write entities remain target work for the same transaction rollout.

Meetings importer execution fidelity is covered by
`specs/studio/fixtures/meetings/expected/execution-fidelity.json` and
`import_meetings_execution_fidelity_vectors_pass`. The same vector set is also source-backed from
`uldren-loom-conformance::studio_imports` for provider-facing certification. The vector executes
`granola-app`, `granola-api`, `granola-mcp`, and `csv` fixtures through the real committed importer
and verifies profile selectors, coverage state, row counts, snapshot counts, retained source
payloads, persisted import checkpoints, explicit source states, retry idempotence, and invalid
`source_state` rejection.

Meetings-specific import operation payloads beyond the normalized first slice, dataframe plan
integration, dedicated import-run read tools, richer import/export workflows, Meetings-specific
audit projection, assistant answer generation, and MCP-assisted bridge mode remain unfinished.

`scripts/granola-cache-import.py` source-backs the external Granola local-cache adapter by converting
cache-shaped JSON files, directories, and archives into the normalized `granola-app` Meetings
snapshot and invoking the official `loom meetings import` command. The script keeps Granola desktop
cache discovery outside the Loom binary, emits SHA-256 digest-qualified source records, preserves
source note ids, title, owner, attendees, folders, summary text, transcript order, and transcript
speakers when present, records missing transcripts as coverage gaps, and batches multiple source
files into one import snapshot. The same script source-backs Granola 7 encrypted local auth
interoperation when the user explicitly authorizes macOS Keychain access. A live host validation on
2026-07-12 created `granola.loom` from the encrypted local auth path with 47 imported meetings,
31,822 transcript spans in the normalized input snapshot, 31,917 applied import operations, and zero
Loom import warnings. The latest live Granola meeting was read back from the Loom and matched the
source meeting id, title, timestamps, source digest, sidecar JSON, summary text, 1,421 transcript
span records, and every retained transcript JSONL line.

1. Owner decision 2026-07-07: the Meetings core schema uses strict operation-specific payloads for
   identity, evidence, redaction, and import runs, with generic normalized source
   snapshot sidecars for extra source fields. Projection records stay separate and rebuildable.
2. Owner decision 2026-07-07: raw source spans use deterministic source-locator identity, while derived
   chunk spans use allocated stable ids with source-span and chunking-policy provenance.
3. Owner decision 2026-07-07: the first Meetings conformance set covers canonical schema plus minimum
   projection effects for redaction and import idempotency. Surface golden renders
   wait for SURFACES fixture support.
4. Owner decision 2026-07-07: the official import command is `loom meetings import <store>
   <workspace> --input-profile <generic|granola-api|granola-app|granola-mcp|csv>
   --input <snapshot.json> [--dry-run] [--report-format text|json]`. The input profile selector
   makes Loom aware of normalized input shape without making source extraction, connector
   credentials, API pagination, cache paths, filesystem discovery, or MCP-assisted source discovery part of the
   stable command.
5. Owner decision 2026-07-07, revised 2026-07-12: normalized imports use a batch envelope with
   profile, source scope, coverage, cursor, folders, items, and sidecar digests. The stable input is
   the normalized snapshot, not live Granola API access. CLI importers may normalize one file, many
   files, a folder, or an archive when the source format supports it. MCP-assisted imports usually
   submit one normalized item at a time.
6. Owner decision 2026-07-07: dry-run performs full validation and fidelity reporting without
   sequencing operations, writing projections, or changing aliases.
7. Owner decision 2026-07-07, revised 2026-07-12: source omissions are coverage gaps, not deletions.
   Meetings found through Granola MCP, filesystem import, or local app/cache import are source
   observations from those input profiles and can explain gaps.
8. Owner decision 2026-07-07: raw Granola API, app, MCP, and filesystem sidecars are retained by
   digest without a source-imposed retention ceiling, subject only to Loom-side user/admin retention,
   legal hold, redaction, or shredding policy.
9. Owner decision 2026-07-07, revised 2026-07-12: import checkpoints are full and
   input-profile-specific. Every checkpoint records common fields for source scope, observed ids,
   completed units, coverage gaps, retry windows, and resume state. Filesystem and local app/cache
   checkpoints record root digest, discovered file ids, and completed file count. Granola MCP
   checkpoints record connector session id when present, retrieved note ids, and known coverage
   limits. Checkpoints support duplicate-free retries, assistant-assisted bridge cutover, and Import
   Coverage explanations.
