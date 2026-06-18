# Studio Adoption - Enterprise Requirements and Gap Registry

**Status:** Target design. **Version:** 0.1.0-target.
**Capability:** cross-cutting; no single capability flag.

This document records (1) what enterprises need to adopt the Studio suite beyond the user-level
features in the profile specs, and (2) the elaborated gap registry from the 2026-07-04 design
sessions. Items here are unowned until pulled into a queue.

## 1. Enterprise Adoption Requirements

Grouped by buyer concern. Existing specs are cited where the foundation already exists; the Studio
suite work is the integration, not the invention.

### 1.1 Identity and Access

- SSO (SAML/OIDC) and SCIM directory sync mapping external users and groups onto Loom principals
  (0026) and grants (0027/0028). Group-derived grants must recompute on directory change.
- Service accounts and agent principals administered separately from human identity, with
  operator-principal chains preserved in envelopes (0061 §2).
- Effective-access answers: "who can see this issue/page/channel" as a queryable projection, plus
  periodic access reviews/attestation exports. (Gap G6.)

### 1.2 Compliance and Governance

- Audit export to SIEM: the audit projections already exist per profile; enterprises need streaming
  export with schema stability guarantees.
- Retention policy administration per scope and content class; legal hold; eDiscovery export with
  chain-of-custody (export artifacts already record source revisions per PAGES §10).
- Data residency: region pinning for blind cloud and keyed workers.
- FIPS distribution and TLS policy already specified in 0060; Studio deployments inherit it.
- Key management: KMS/HSM integration via 0034 key sources; per-scope key rotation to honor
  revocation limits the profiles acknowledge.
- Certification: conformance reporting per 0010a as the compliance evidence trail.

### 1.3 Migration and Coexistence (Gap G1)

**Status: requirement (owner decision, 2026-07-04), not a proposal.** Import from incumbent tools
is a required capability of the Studio suite. No enterprise starts empty. Required:

- Importers for Jira, Confluence, Slack, and Drive exports through the 0012 interchange layer:
  schema mapping tables (Jira issue -> Jiraish operations; Confluence storage format -> page body
  model; Slack export -> chat events; Drive tree -> Driveish operations), identity mapping into
  principals, and a per-run fidelity report (what mapped, what degraded, what dropped).
- Coexistence bridges: incremental one-way sync from the incumbent during transition, so teams can
  move space-by-space or project-by-project rather than big-bang.
- Alias preservation: imported `PROJ-123` keys and page URLs become 0061 §4 aliases so existing
  links keep resolving; a redirect map covers URLs that cannot be preserved.
- Import is also the schema stress test: mapping real Jira workflows and Confluence macros against
  the profile models will surface modeling errors cheaper than production use will.

**Import source matrix (2026-07-04; requirement tiers per owner decision 2026-07-04):** the full
import schema surface is designed for every source below (mapping tables at spec level, so the
model is stress-tested by all of them); build focus is low-hanging fruit (Redmine, markdown tree)
plus Granola normalized imports, Notion, Asana, and the already-pinned Jira + Confluence.

Importer priority is file-first, then assistant-assisted. Priority 1 importers consume files,
folders, archives, or normalized snapshots through the CLI and must handle one-item and batch inputs
when the source format allows it. Priority 2 importers accept normalized batches from MCP or another
assistant-controlled connector, commonly one item at a time. Loom-owned live vendor API connectors
are not part of the Studio importer target, except that Notion remains allowed to keep an API-backed
input option because its API block tree is the highest-fidelity source.

| Source | Target profile(s) | Status | Fidelity notes |
| --- | --- | --- | --- |
| Jira (Server/DC XML/export snapshots) | JIRAISH | Requirement; mapping pinned (JIRAISH §25) | changelog -> history synthesis; content_type ADF bodies |
| Confluence (storage XHTML + ADF) | PAGES | Requirement; body mapping pinned (§5.4) | cross-format equivalence vectors required |
| Slack export | SLACKISH | **Candidate (demoted from requirement, owner 2026-07-04)**; mapping table pinned (SLACKISH §19); importer/bridge on demand | Slackish is AI-centric first, not a Slack clone; bodies convert to 0061 §9.1 canonical blocks; bots -> agent-kind placeholder principals; stress test yielded pins + emoji registry (0061 §7) |
| Drive/SharePoint | DRIVEISH | Requirement; mapping pinned (DRIVEISH §21) | shares -> grants; revisions where exportable; comments -> annotations; mirror-mode bridge (0061 §7.1) |
| Asana (JSON-lines resource export or normalized assistant batch) | JIRAISH (+ goals -> roadmap) | **Build focus; mapping pinned (JIRAISH §26, 2026-07-04)** | multi-homing -> membership edges + optional secondary keys (§4.1/§21); sections -> manual boards (§7.4); stories -> history synthesis; portfolios/goals -> roadmap items; approval -> `accepted`; unavailable rules/forms are fidelity gaps, not mappings; computed fields frozen (`imported_computed`) |
| Notion (API block tree / organization export) | PAGES | **Build focus; mapping pinned (PAGES §5.6, 2026-07-04)** | API is the source (exports are degraded fallback); databases -> `database` structures on the shared field system (0061 §7.1); status groups -> categories; views (now API-exposed) -> saved queries + view definitions; synced blocks -> block_ref; formulas/rollups frozen |
| Rally (Broadcom, REST/WSAPI) | JIRAISH | Candidate (schema designed via §24.1 adoptions; importer on demand) | typed portfolio hierarchy, nested stories, schedule states incl. Accepted, iterations with capacity, test cases/sets, revision history native |
| Redmine (REST/XML, open source) | JIRAISH + PAGES | **Requirement (owner 2026-07-04)**; cheapest importer | journals approximately operation log (near-1:1 history); precedes/follows with delay validates §21 lag; time entries -> work logs; versioned wiki feeds PAGES |
| Markdown tree / Obsidian vault | PAGES | **Build focus; mapping pinned (PAGES §5.7, 2026-07-04)** | folders -> container pages (folder-note promotion); frontmatter -> shared fields; wikilinks -> late-binding refs; embeds -> block_ref (incl. `section` mode); .canvas -> canvas structures (JSON Canvas 1.0); Dataview/Tasks plugin data verbatim, fidelity-reported |
| Granola normalized snapshot / local cache / MCP-assisted item | MEETINGS | **Build focus; mapping pinned (MEETINGS §8, 2026-07-06)** | notes -> meetings, transcript items -> spans, folders/calendar/attendees preserved; local cache extraction stays outside core Loom and feeds `loom meetings import` |
| MediaWiki / XWiki | PAGES | Candidate (legacy enterprise) | template/macro degradation must be fidelity-reported |

Current source backs the reusable profile-import planning contract for Redmine, Jira, Confluence,
Markdown, Notion, Asana, Slack, Drive, and Granola-family source systems in `loom-interchange`.
`crates/loom-conformance` now covers those planning contracts as reusable interchange vectors:
Redmine mixed ticket/page actions, Jira and Asana ticket actions, Confluence storage XHTML and ADF
page actions, Markdown and Notion page actions, Slack chat actions, Drive actions, Granola-family
Meetings sidecar actions, and duplicate planned-action rejection. `loom interchange import-redmine`
now source-backs normalized Redmine snapshot parsing for projects, issues, and wiki pages,
ticket-profile lowering through the official project/ticket service, Pages-profile lowering for wiki
pages, Redmine issue external-identity preservation, idempotent duplicate skipping, unchanged-page
skipping, structured retention of Redmine journals, comments, attachments, time entries, and
relations as `redmine_*` ticket fields, MCP `redmine_import_snapshot` execution, and shared
import-report output. The Redmine API-shaped fixture in `specs/studio/fixtures/redmine/` verifies a
clean import and comparison report for represented project, issue, source-extra, and wiki fields.
`loom interchange import-asana` now source-backs
normalized Asana snapshot parsing through the reusable Asana importer in `loom-interchange-io` for
projects and tasks, ticket-profile lowering, Asana task external-identity preservation,
first-class manual Ticket Board creation from project sections, deterministic board-card placement,
tag-to-policy-label preservation, custom-field bundle retention, approval-task retention,
idempotent duplicate skipping, broad fixture coverage, and shared import-report output.
`loom interchange import-jira` now source-backs normalized Jira snapshot parsing through the
reusable Jira importer in `loom-interchange-io` for projects and issues, ticket-profile lowering,
Jira issue external-identity preservation, Jira key field preservation, first-class status-mapped
Ticket Board creation, deterministic board-card placement, label-to-policy-label preservation,
custom-field bundle retention, fixture-backed import conformance, idempotent duplicate skipping, and
shared import-report output.
`loom interchange import-confluence` now source-backs normalized Confluence snapshot parsing through
the reusable Confluence importer in `loom-interchange-io` for Pages spaces and pages,
byte-preserving storage XHTML and ADF JSON opaque body blocks, changed-page publishing,
unchanged-page skipping, and shared import-report output. `loom interchange import-slack` now
source-backs normalized Slack snapshot parsing for Chat
channels, plain text messages, present-parent threads, and reaction kinds through the reusable Chat
service. The same command also accepts a standard Slack export zip with `channels.json` and channel
message JSON files. `loom interchange import-drive` now source-backs normalized Drive/SharePoint
snapshot parsing for folders and files, Drive-service folder creation, file upload/commit,
inline text bytes, inline hex bytes, content-path file bytes, changed-file replacement,
unchanged-file skipping, and Drive revision rows for committed uploads. `uldren-loom-conformance::studio_imports`
now executes the broad fixture set for Redmine, Asana, Jira, Confluence, Slack, Drive, Markdown,
Notion, and Granola Meetings through the reusable execution-batch path, checks report source scopes
and row counts, checks that unsupported-field matrices become fidelity issues, and verifies
representative lowered profile state. Meetings import execution conformance also runs the
`granola-app`, `granola-api`, `granola-mcp`, and `csv` fidelity vectors through the committed
importer path. Actual source fetch/parsing remains target work for every importer except the Redmine
normalized project/issue/wiki/source-extra and MCP execution slice, Asana normalized project/task
fixture slice, Jira normalized project/issue fixture slice, Confluence normalized byte-preserving page
fixture slice, Slack normalized channel/message/reaction slice, Slack zip export slice, Drive
normalized folder/file slice, and Granola normalized app/API/MCP/CSV input slice plus the external
local-cache script path.
The normalized Asana, Jira, Confluence, Slack, and Drive execution slices also emit shared
import-report fidelity issues for present source fields they do not yet lower, so unsupported
changelogs, worklogs, subtasks, stories, portfolios, goals, files, pins, custom emoji, channel
members, per-user reaction authorship, permissions, historical revisions, metadata, and shortcuts
are visible instead of silently dropped. Redmine now retains journals, comments, attachments, time
entries, and relations as structured source fields while native ticket-profile replay/comment/link/
attachment/work-log operations remain target work.
The Markdown and Notion execution slices likewise emit shared fidelity issues when visible
frontmatter, wikilinks, attachments, callouts, tables, footnotes, equations, Dataview, Excalidraw,
database, formula, rollup, view, comment, permission, attachment, or synced-block fields are present
before those lowerings exist.
Redmine native replay/comment/link/attachment/work-log projection, Pages wiki revision metadata,
full identity mapping, and richer native profile lowering remain target work. Asana resource/organization parsing, multi-homing, stories, attachments,
portfolios/goals, identity mapping, and MCP-assisted import remain target work. Jira export/API
parsing, key-alias preservation, changelog/workflow/agile lowering, identity mapping, and
MCP-assisted import remain target work. Confluence site/export parsing, full XHTML/ADF block
lowering, attachments, comments, MCP-assisted import, and cross-format
semantic equivalence vectors remain target work. Slack mrkdwn and Block Kit conversion, imported
user/principal mapping, per-user reaction authorship, files, pins, membership, custom emoji assets,
and MCP-assisted import remain target work. Drive and SharePoint export
parsing, direct permission/share mapping, comments, shortcuts, multi-parent lowering, SharePoint
metadata, and MCP-assisted import remain target work. Meetings raw source importer ergonomics beyond
the normalized snapshot boundary, dedicated import-run reads, richer import/export workflow tools,
Meetings-specific audit projection, and MCP-assisted bridge mode remain target work. Redmine's full
source mapping table is pinned in JIRAISH §27; Jira and
Asana are pinned in JIRAISH §25 and §26; Confluence body mapping is pinned in PAGES §5.4; Slack
mapping is pinned in SLACKISH §19; Drive and SharePoint mapping is pinned in DRIVEISH §21.
`loom interchange import-markdown` now source-backs the first Markdown-tree execution slice for `.md`
files: deterministic page ids, page-space creation, heading/paragraph/list/quote/divider Body
lowering, whole-page Obsidian page embeds, folder-note identity, page publish, unchanged-page skips,
shared import-report output, and the fixture comparison under `specs/studio/fixtures/markdown`.
Obsidian-specific semantics and richer Markdown lowering remain target work per PAGES §5.7.
`loom interchange import-notion` now source-backs the first normalized Notion execution slice through
the reusable Notion importer in `loom-interchange-io`: deterministic page ids, Notion API-shaped
page and block-children bundle parsing, page-space creation, heading/paragraph/list/quote/divider
Body lowering, parent placement, page publish, unchanged-page skips, shared import-report output,
and the fixture comparison under `specs/studio/fixtures/notion`. Exact block-tree lowering,
database/structure lowering, formulas/rollups/views/comments/permissions/attachments, and synced
blocks remain target work per PAGES §5.6.

- Archive containers (2026-07-04): import sources arrive compressed - Slack exports are zip,
  Asana resource exports are gzip JSON-lines, Confluence site exports are zip. Loom's own
  compression is a storage frame below the digest boundary (0005: DEFLATE/LZ4, optional AEAD,
  ≥1KiB payloads) and is not archive tooling; the import framework therefore owns zip/gzip/tar
  container ingestion importer-side (0012 layer). **Resolved (owner, 2026-07-04, DRIVEISH
  session):** archive preview/extract is additionally a user-facing Driveish keyed-worker
  feature - entry-listing previews plus `files.extract` materializing entries as sequenced file
  operations; pack is out of scope; blind deployments lack it. Pinned in DRIVEISH §11.1.
- Status 2026-07-04: the Jira mapping is pinned in JIRAISH §25 (mapping table, history synthesis,
  placeholder identities, read-only mirror + cutover, fidelity report). The stress test fixed three model
  gaps (release sets, status categories, body content types) - validating the fix-the-model rule.
  Mirror/cutover semantics generalized into substrate scope operating modes (0061 §7.1).

### 1.4 Operations

- Backup/restore and disaster recovery runbooks for the sequencer and blind replicas; RPO/RTO
  statements per topology.
- Multi-region and offline branch-Studio deployment guidance (the local-first model is an
  advantage here; it needs stating in enterprise terms).
- Observability of the Studio layer per 0030: sequencer lag, projection staleness, index freshness,
  worker backlogs - surfaced to tenant admins, not just operators.
- Quotas and metering: storage, operation rate, automation compute per organization.

### 1.5 Procurement Checkboxes

Accessibility (WCAG) for all SURFACES.md surfaces; internationalization of app chrome and elicitation
prompts; browser support matrix; SLA definitions per deployment topology; documented upgrade paths
with contract stability (no-v1 principle: contracts do not break across upgrades).

### 1.6 Studio Reserved and Mounted Roots

Studio schema names intentionally use `loom.studio.*` because Studio is the product family. Mounted or
reserved file paths do not use a generic Studio root. A profile that publishes retained source files,
derived files, or mount-visible artifacts owns its path root directly.

Current audit result:

| Profile or substrate | Current source-backed path behavior | Root rule |
| --- | --- | --- |
| Meetings | `loom meetings import` persists retained source sidecars, summaries, and transcript JSONL under `.loom/meetings/{profile}/sources/{source}/...`. | Meetings owns `.loom/meetings/...`. |
| Drive | Source-backed profile state is stored as control records; local OS projection and placeholder hooks remain target work. | Future retained or mounted Drive files use a Drive-owned root, not a generic Studio root. |
| Pages / Spaces | Source-backed profile state is stored as control records; import sidecars and app projections remain target work. | Future retained or mounted Pages files use a Pages-owned root. |
| Tickets | Source-backed state uses the ticket component's private table/control boundary; import sidecars remain target work. | Future retained or mounted Tickets files use a Tickets-owned root. |
| Chat | Source-backed channel directories and operation logs use profile control or reserved internal paths; attachment byte handling remains target work. | Future retained or mounted Chat files use a Chat-owned root. |
| Lifecycle / Surfaces | Source-backed lifecycle state is control-plane data; MCP app bundles are owned by the MCP facet path from 0043. | Lifecycle and Surfaces do not create a generic Studio mount root. |
| Shared substrate | Revision indexes, emoji registry, views, and embedding-projection jobs use `.loom/substrate/...`; MCP app bundles use `.loom/facets/mcp/apps/...`. | Shared substrate roots stay shared-substrate roots, not profile roots. |
| Webish | Web server configuration and caches use `.loom/web/...`. | Webish owns `.loom/web/...`. |

Audit 2026-07-12: source and specs no longer contain accidental nested Studio-root mount-path
claims.

### 1.7 Current Public Surface Snapshot

The detailed source-backed boundaries live in each profile spec. This cross-profile snapshot exists
to keep Queue 8 from becoming a historical ledger.

| Profile | Source-backed public surfaces | Remaining public-surface gap |
| --- | --- | --- |
| Tickets | `loom tickets` project create/rekey/release, create, atomic update request, list, get, history; MCP `tickets_update` applies one optional detail patch and one optional lifecycle action in one revision. Hosted REST/JSON-RPC retain project create/rekey/release, create, update fields, get, history, operation-log replay, revision rows, aliases, text-field reference indexing, reusable lifecycle policy primitives in `loom-tickets`, planning primitives, and normalized Redmine/Jira/Asana import execution. Redmine, Asana, and Jira import execution is reusable through `loom-interchange-io`. IDL, C ABI, C++ helper, Node, Python, iOS/Swift, JVM, Android/KMP, React Native, and WASM `LoomStore` bindings expose the project, project settings read/write, custom field put/retire/catalog, ticket CRUD, relation, read/list, and history JSON vertical over the same service boundary. Built-in MCP app bundles exist for Ticket Details, Board, Roadmap, Sprint Planner, Backlog Triage, and Dashboards with selected-ticket deep links, source-backed ticket/history/reference/lane data, source-backed tool metadata filtering, app-only `tickets_update`, and `verify-apps` visual coverage. | Hosted atomic ticket updates, workflow transport endpoints, comment/attachment/link/rank/watch tools, richer import lowering, advanced workflow elicitation UI, and broader profile conformance. |
| Pages / Spaces / Structures | `loom pages` space create/list/get, page create/update/publish/get/history, page list, and structure create/list/get/add-node/update-node/bind/move-node/link-node/decompose-to-tickets; MCP and hosted REST/JSON-RPC cover the same source-backed structures vertical plus spaces/pages reads and writes; IDL, C ABI, C++ helper, Node, Python, iOS/Swift, JVM, Android/KMP, React Native, and WASM `LoomStore` expose the same public Pages / Spaces / Structures vertical; Markdown and Notion import execution is reusable through `loom-interchange-io` plus `loom interchange import-markdown` and `loom interchange import-notion`; built-in MCP app bundles exist for Document Viewer, Mind Map, Canvas, and Diagram Editor with selected page/structure deep links, selected-page backlinks, selected-page publish, structure add/move/link actions, and `verify-apps` visual coverage. | Richer import lowering and broader profile conformance. |
| Chat | Reusable `loom-chat` service boundary, CLI channel/message/event/cursor/reaction/emoji/task/agent/handoff operations, MCP and hosted REST/JSON-RPC message/thread/reaction/task/agent/handoff/cursor/presence surface, hosted emoji registry management, operation logs, revision rows for message create/edit/redact, and reusable Slack import execution through `loom-interchange-io` plus `loom interchange import-slack`. IDL, C ABI, C++ helper, Node, Python, iOS/Swift, JVM, Android/KMP, React Native, and WASM `LoomStore` expose durable channel, message, event, cursor, emoji, task, reaction, thread, agent, and handoff JSON operations. Built-in MCP app bundles exist for Chat Channel, Chat Thread, Chat Tasks, Chat Presence, and Chat Handoffs with channel/thread deep links, source-backed channel/message/thread/task/presence/cursor/event/handoff data, app-only `chat_post_message` and `chat_set_presence`, and `verify-apps` visual coverage. | Attachment byte handling, richer import lowering, notification delivery integration, and broader profile conformance. |
| Drive | Reusable `loom-drive` service boundary, CLI list/stat/read/version/conflict/share/retention/admin/upload/rename/move/delete/resolve operations, MCP, REST, and JSON-RPC read/write/share/retention/conflict/upload verticals, IDL, C ABI, C++ helper, Node, Python, iOS/Swift, JVM, Android/KMP, React Native, and WASM `LoomStore` bindings, attached-daemon lease lifecycle, served listeners, local OS projection planning, revision rows for committed uploads, and reusable Drive import execution through `loom-interchange-io` plus `loom interchange import-drive`. | OS-native placeholder hooks, worker scheduling, lease expiration sweeps, platform hydration/eviction workers, and richer import lowering. |
| Meetings | MCP and hosted list/get/search/projection/review/review-write verticals, projection materialization and materialized-output readback, hosted vector execution when embeddings are configured, normalized import command, retained source payload readback, structured-item import for observed annotations, concrete MCP promotion writers for Tickets, Decision log, Lifecycle, and reference artifacts, WASM `LoomStore` import/source-read methods, executable Meetings import-fidelity conformance, and `verify-apps` visual coverage for the six Meetings apps. | Raw source import completion beyond normalized snapshots, MCP-assisted bridge ergonomics, Pages meeting-summary promotion, dedicated import-run reads, richer import/export workflow tools, Meetings-specific audit projection, and generic review-write revision rows. |
| Lifecycle | `loom lifecycle` definition/instance/transition/snapshot/surface/log vertical; MCP and hosted REST/JSON-RPC routes; canonical definitions, gates, transitions, snapshots, operation logs, registered prompts, session-bound active lifecycle surfacing, Studio status lifecycle summaries, and conformance vectors. | Durable trigger keeper/public facade work remains with 0029; richer lifecycle app visualization remains with Surfaces. |
| Surfaces | Canonical app definitions, deterministic catalog inspection through CLI, IDL, C ABI, C++ helper, Node, Python, iOS/Swift, JVM, Android/KMP, React Native, and WASM bindings, elicitation, prompt handoffs, render frames, app resources, `apps.*` authoring, template rendering, dynamic app launchers, app-only write dispatch through `ask_record`, app-only visible-tool dispatch through `apps_call_tool`, subscriptions, built-in Directed Graph app foundation, catalog-derived profile graph data, Chat, Ticket, Pages, Drive, and Meetings app bundle foundations with selected channel/thread/ticket/page/structure/file/meeting rendering, shared built-in app shell CSS, Playwright-backed `verify-apps` coverage for VCS/Decisions/Directed Graph/Chat/Tickets/Pages/Drive/Meetings bundles, and `mcp-apps` capability advertisement. | Browser-host/iframe conformance, richer app workflows, and expanded visual coverage as new promoted tools become available. |

## 2. Gap Registry (elaborated)

### G1. Import/migration - see §1.3. A requirement as of 2026-07-04. Highest adoption leverage of any item in this file.

### G2. Workflow and Schema Authoring Experience

Workflows are versioned data (JIRAISH §8) and custom fields have typed schemas (JIRAISH §18.7), but
nothing defines who edits them or how. Needed: an authoring app (SURFACES.md catalog addition) with
dry-run validation (`workflows.validate` against live issues, reporting which issues would be
stranded in removed states), migration policy for in-flight issues on workflow change, and
guard/gate editing through structured elicitation rather than free-form JSON. Same pattern applies
to Pages templates and structure kinds.

Worked example: a PM opens the workflow authoring app to remove the `Reopened` state from the bug
workflow. The app calls `workflows.validate` with the proposed v7: "14 issues are currently in
Reopened; 3 automations reference it; the DoD gate for the `bug` lifecycle cites it." Elicitation:
`{migrate_reopened_to: enum[In Progress, Triage], effective: enum[immediately, next-sprint],
update_automations: boolean}`. On confirm, `workflows.update` lands v7 plus a migration operation;
the 14 issues transition with `migrated_by_workflow_change` in their history; the audit trail shows
who removed the state and why. Without this, the "edit" is someone hand-writing workflow JSON and
discovering the stranded issues in production.

### G3. Notification Routing and Digest Policy

0035 provides durable wakeups; nothing turns them into a humane notification product. Needed: a
per-principal preference model (per-scope, per-operation-kind, digest windows, quiet hours,
channel-of-delivery), notification as a *projection* over changes joined with preferences (not a
parallel delivery system), and agent notification budgets so subscribed agents do not burn compute
on every keystroke-level operation. Over-notification is the most common collaboration-suite
complaint; this is a product-defining gap, not plumbing.

Worked example of the failure this prevents: a triage agent runs nightly and updates 200 issues
(priority, labels, sprint suggestions). Naive per-operation notification: every watcher of any of
those issues wakes to dozens of pings; the team lead watching the project gets ~200. With the
notification projection: draft-autosave-class operations notify no one (below the kind threshold);
the 200 triage operations join into one digest per principal - "triage-agent updated 200 issues
overnight; 14 assigned to you changed priority; 2 need your review" - delivered at the principal's
digest window, not at 3 a.m.; and the one operation that crossed a policy line (an agent moved an
issue out of an active sprint) notifies immediately because operation kind, not volume, drives
urgency. Lifecycle stage transitions (LIFECYCLE.md §2) are the preferred coarse event: "feat-013
entered Build" summarizes dozens of operations in one notification.

### G4. Cross-Profile Reference Grammar

**Resolved 2026-07-04: pinned in 0061 §19** (was 0061 open decision 6). One grammar for typed
references (`kind:alias-or-id` forms, `@principal`, `#channel`, bare issue keys as the only
unprefixed extracted form), key-holder alias binding at sequence time against `base_root`,
sequence-with-late-binding for unresolved aliases (imports and forward references are normal
cases), verbatim text plus a payload-resident references block, and rename stability by stable-id
binding. This is the connective tissue that makes ticket ↔ page ↔ message ↔ file one system rather
than four.

Worked example (updated to the pinned grammar): a user writes in a spec page: "blocked by LOOM-52
until the sequencer lands - see page:substrate-predicates and the #eng-infra discussion." Mention
extraction at sequence time creates three typed edges: page -> issue (`refers_to`), page -> page
(`refers_to`), page -> channel (`refers_to`). All three appear in the Directed Graph viewer and in
`substrate_refs` for each target. Extraction never infers semantic relations from prose: "blocked
by" does **not** create a `blocks` relation - an assistant or app may offer "make this a blocking
link?" as a follow-up elicitation, and only that explicit `tickets.link` operation records the
relation. Six months later the project is renamed LOOM -> CORE: the issue key alias changes to
CORE-52, but the reference was bound to the stable `issue_id` at sequence time, so nothing dangles
- the page renders the current alias while `alias_at_write` preserves what the author saw. Had
LOOM-52 not existed yet (a forward reference, or a page imported before the issue), the operation
still sequences with an `unresolved(issue, "LOOM-52")` record, and the re-resolution worker binds
it when the issue arrives.

### G5. Assistant Session Bootstrap

The `_QUEUE` file pattern generalized into product: a named Studio status view (0061 §8) per
principal at `loom://{workspace}/studio/views/status/principal/{principal}` yielding active
sprint/queue, assigned open items, changes since the principal's cursor, open conflicts, and
decision points awaiting them. A fresh assistant session reads one resource and has working context;
no chat history required. This converts a working discipline into a contract.

**Owner:** the view contract lives in 0061 §8 (recorded there as the first standard named view);
the app rendering of it belongs to `SURFACES.md`; this file only tracks the gap.

Worked example: Nas opens a new session Monday morning and says "where were we?" The assistant
reads `loom://acme/studio/views/status/principal/nas` and answers from one fetch: "feat-013
(predicate grammar) is in Build - 3 of 7 issues closed; you have 2 assigned issues and 1 review
request; since Friday: 12 operations, including a redesign decision dec-0021 by the triage agent;
1 open conflict on LOOM-52's priority awaits you; the DoD gate for feat-011 has 2 unattested
items." No transcript needed, no re-reading of specs - the store carried the context.

### G6. Permissions Administration UX

Grant models exist (profiles §10/§11); administration does not. Needed: effective-access viewer
("why can this principal see this?"- the grant chain), bulk grant operations, access review
exports, and share auditing (external links outstanding, per scope).

### G7. Agent Rate Limiting and Metering

**Resolved 2026-07-05 (SLACKISH session): pinned in 0061 §20.** Per-agent-principal budgets
(operations/hour total and per kind, 0015 compute units, bytes/day, spend) as policy data enforced
at three consistent points - blind sequencer (envelope-countable, pre-sequencing), PEP
(reads/tools), 0015 (compute); anomaly detection against the agent's own trailing per-scope
baseline pauses the agent and elicits the `operator_principal` (continue / kill / raise budget),
while budget exhaustion hard-rejects with a typed policy message; audited global kill (principal
suspended, grants intact, one-op reactivation) plus per-scope mute; `trace_digest` required on
agent writes with trace content sealed under the `agent_forensics` retention class (tenant window,
default 90 days, crypto-shred on expiry, digest fact persists). SLACKISH §13 is the first consumer.

## 3. Ownership

Owner decision (2026-07-04): **one queue only.** Queue 8 remains the sole queue; this is a design
session and gap work is pushed into owning specs as unfinished tasks rather than into new queues.

- G1 (import, a requirement): pushed into each profile's Unfinished Tasks - JIRAISH, PAGES,
  SLACKISH, DRIVEISH - as profile-specific mapping tables, alias preservation, coexistence bridges,
  and fidelity reports. The shared import framework rides 0012 interchange.
- G2 (authoring experience): pushed into JIRAISH (workflow/schema authoring) and PAGES
  (template/structure authoring) Unfinished Tasks.
- G3 (notification projection): owned by 0061, recorded as its open decision 7.
- G4: resolved - pinned in 0061 §19 (2026-07-04). G5: contract in 0061 §8, rendering in SURFACES.md.
  G7: resolved - pinned in 0061 §20 (2026-07-05, SLACKISH session). G6: tracked here, unowned.
