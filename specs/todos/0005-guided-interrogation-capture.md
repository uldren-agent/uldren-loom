# Guided interrogation capture

## Idea

Build a first-class Uldren Desktop workflow for guided interrogation capture: an agent-led interview
that keeps asking focused follow-up questions until a concept, event, process, review, or post-mortem
has enough detail to become durable Loom memory.

This is inspired by the "grill me" skill described in Nate Herk's second-brain video. The skill
interviews the user relentlessly about a topic, creates a brainstorm file, and stops only when it has
enough context. Uldren Desktop should make that pattern native, structured, reviewable, and connected
to Loom workspaces instead of leaving it as an ad hoc prompt.

The actual `grill-me` skill is intentionally tiny: it delegates to a reusable `grilling` loop. The
Uldren version should keep that directness, but add first-class capture, live workspace projection,
visual feedback, and review gates so the conversation turns into durable, queryable knowledge while
the user is still being interviewed.

## Sources checked

- Matt Pocock's `grill-me` article publishes the core skill text and describes it as a flexible
  interview skill for stress-testing a plan or design until shared understanding is reached. It says
  the skill walks the design tree, resolves dependencies between decisions, explores the codebase when
  the answer is discoverable there, and asks the AI to provide a recommended answer for each question.
  Source: `https://www.aihero.dev/my-grill-me-skill-has-gone-viral`.
- Matt Pocock's `5 Agent Skills I Use Every Day` article describes `/grill-me` as a way to flesh out
  an idea before committing to a plan. It explains the design-tree concept, gives examples of sessions
  with many questions, and places `grill-me` in a broader workflow with `/to-prd`, `/to-issues`, TDD,
  codebase architecture, `grill-with-docs`, and domain modeling.
  Source: `https://www.aihero.dev/5-agent-skills-i-use-every-day`.
- The repository implementation of `grill-me` is a user-invoked wrapper that runs a `/grilling`
  session.
  Source: `https://raw.githubusercontent.com/mattpocock/skills/main/skills/productivity/grill-me/SKILL.md`.
- The reusable `grilling` skill asks one question at a time, waits for feedback before continuing,
  walks the design tree, provides a recommended answer for each question, and explores the codebase
  instead of asking when the answer is discoverable there.
  Source: `https://raw.githubusercontent.com/mattpocock/skills/main/skills/productivity/grilling/SKILL.md`.
- The `grill-with-docs` skill combines the grilling session with domain modeling.
  Source: `https://raw.githubusercontent.com/mattpocock/skills/main/skills/engineering/grill-with-docs/SKILL.md`.
- The `domain-modeling` skill actively builds a project domain model, challenges terms against the
  glossary, sharpens fuzzy language, probes edge cases, cross-checks code where relevant, updates
  glossary terms as they crystallize, and creates ADRs only for meaningful trade-off decisions.
  Source: `https://raw.githubusercontent.com/mattpocock/skills/main/skills/engineering/domain-modeling/SKILL.md`.
- The attached transcript says the "grill me" skill interviews the user relentlessly about a topic,
  creates a brainstorm file, and can accept files, transcripts, contracts, or other source material.
  Source: `/Users/nxkavian/.codex/attachments/98eb6b32-4317-4e65-b6fc-b0b1f7c2d5cc/pasted-text.txt:424`.
- Local search did not find an installed Codex skill or plugin named `grill me`, `grill-me`, or
  `grillme`; `tool_search` also returned no matching installed tool. The behavior is therefore based
  on the linked upstream skill and articles, not on a locally installed implementation.
- Loom workspaces are independent typed trees inside one Loom. Workspace types include `files`,
  `document`, `vector`, `graph`, `search`, `sql`, `ledger`, and other facets. History and writes are
  scoped to one workspace, while explicit read-only queries may span workspaces.
  Source: `specs/0014-workspaces.md:7`, `specs/0014-workspaces.md:32`,
  `specs/0014-workspaces.md:99`.
- The Meetings and YouTube ingest plans already use a source-span annotation model with fixed core
  kinds, open project vocabulary, provenance, confidence, and graph-ready relations.
  Source: `specs/studio/MEETINGS.md:262`,
  `specs/todos/0004-youtube-channel-ingest.md:280`.

## Product shape

Guided interrogation capture is an interactive session with a defined objective, source context,
question plan, evidence buffer, completion rubric, and Loom projection.

The user starts with a prompt such as:

- "Grill me about this startup idea."
- "Help me reconstruct this incident timeline."
- "Run a product post-mortem."
- "Capture yearly performance review feedback for this employee."
- "Interview me about this employee workflow and identify automation opportunities."

Uldren Desktop then:

1. Selects or asks for a capture template.
2. Ingests optional source material.
3. Asks one focused question at a time.
4. Tracks answered, partially answered, and missing slots.
5. Extracts entities, claims, tasks, decisions, events, risks, evidence, and open questions.
6. Shows a live outline and coverage score.
7. Stops only when the completion rubric is satisfied or the user chooses to stop.
8. Writes raw transcript, structured notes, annotations, and derived graph/vector/search records to
   Loom.

The key product constraint: the agent should not just make a polished summary. It should reveal gaps,
ask for missing details, and preserve evidence for later audit.

The second product constraint: the session should ingest as it goes. Each answer should update Loom
records, graph-ready annotations, embeddings, and visualizations incrementally rather than waiting
until the end for a summary pass.

## Core session model

Each capture session should have:

- `session_id`: stable ID.
- `template_id`: chosen capture template.
- `objective`: user-stated goal.
- `subject`: concept, incident, employee, workflow, product, customer, or investigation target.
- `participants`: user, interview subject, reviewers, optional agents.
- `source_refs`: files, transcripts, links, prior Loom records, MCP tool outputs.
- `question_plan`: ordered and adaptive set of questions.
- `coverage_model`: required fields and completion rules.
- `conversation_log`: raw question and answer turns.
- `evidence`: source spans or user answers supporting extracted facts.
- `annotations`: normalized tags and relations.
- `open_items`: unanswered questions, contradictions, missing source material.
- `outputs`: markdown, JSON records, graph-ready triples, task records, and automation candidates.
- `review_status`: `draft`, `needs_more_input`, `ready_for_review`, `accepted`, or `archived`.

## Templates

Templates should be first-class assets, not hardcoded prompts. A template defines the interview goal,
required fields, question families, extraction schema, completion rubric, and output projections.

### Concept to full idea

Use case: an entrepreneur wants to map a rough concept into a full-fledged idea.

Required capture areas:

- Problem statement.
- Target user or buyer.
- Current alternatives.
- Proposed solution.
- Differentiation.
- Business model.
- Distribution path.
- Risks and constraints.
- Evidence and assumptions.
- First milestone.
- Open questions.

Derived outputs:

- Idea brief.
- Lean canvas or similar structured model.
- Risk register.
- Validation plan.
- First tasks.
- Graph nodes for idea, user segment, problem, solution, competitors, assumptions, and milestones.

Example questions:

- Who has this problem today, and how do you know?
- What breaks if the idea works?
- What is the smallest proof that would change your confidence?
- Which assumption would kill the idea if false?

### Investigation timeline

Use case: an investigator wants to map a full timeline of investigative details.

Required capture areas:

- Scope and investigation question.
- People and organizations.
- Events with timestamps and timezone.
- Source documents and evidence.
- Claims and counterclaims.
- Gaps and contradictions.
- Confidence level per event.
- Chain of custody or source provenance.
- Next investigative steps.

Derived outputs:

- Timeline.
- Entity and relation map.
- Evidence matrix.
- Contradiction list.
- Open lead list.
- Graph nodes for people, organizations, events, artifacts, locations, claims, and evidence.

Example questions:

- What is the earliest event we can timestamp with evidence?
- Which details are directly observed, and which are inferred?
- What would disprove the current sequence?
- Which source is the strongest evidence for this event?

### Product post-mortem

Use case: a product team wants to perform a post-mortem.

Required capture areas:

- Incident or launch summary.
- Impacted users.
- Timeline.
- Expected behavior.
- Actual behavior.
- Detection path.
- Root causes.
- Contributing factors.
- What went well.
- What went poorly.
- Corrective actions.
- Owners and due dates.
- Prevention checks.

Derived outputs:

- Post-mortem report.
- Action-item register.
- Decision and root-cause graph.
- Follow-up task list.
- Metrics and regression checks.

Example questions:

- When did the team first know something was wrong?
- What signal should have detected this earlier?
- Which decision made the impact larger or smaller?
- What owner and date make each corrective action real?

### Yearly performance review

Use case: a manager wants to capture an employee's yearly performance review feedback against a
template.

Required capture areas:

- Role and review period.
- Responsibilities.
- Goals and outcomes.
- Strengths.
- Growth areas.
- Examples and evidence.
- Peer or stakeholder feedback.
- Values and behavior.
- Promotion or compensation considerations when permitted.
- Goals for next period.
- Manager commitments.

Derived outputs:

- Review draft.
- Evidence-backed feedback notes.
- Achievement timeline.
- Growth plan.
- Follow-up commitments.

Example questions:

- What is the strongest concrete example of this strength?
- Which missed expectation was most important, and what evidence supports it?
- What feedback would be unfair without more context?
- What should change in the next review period?

Privacy requirement:

- Performance review sessions should default to private storage and explicit sharing.
- Sensitive HR fields should be policy-gated and omitted from broad graph projections unless the user
  chooses otherwise.

### Workflow automation capture

Use case: a manager wants to capture an employee's workflow for potential automation against
available MCP tools and agentic tools.

Required capture areas:

- Trigger.
- Inputs.
- Systems used.
- Authentication and permissions.
- Steps and decisions.
- Exceptions.
- Outputs.
- Quality checks.
- Frequency and duration.
- Failure modes.
- Human judgment points.
- Candidate MCP tools or app connectors.
- Candidate agentic actions.
- Automation risk.

Derived outputs:

- Workflow map.
- Automation opportunity brief.
- Tool and connector requirements.
- Human-in-the-loop policy.
- Task decomposition.
- ROI estimate.
- Implementation plan.

Example questions:

- What tells the employee to start this workflow?
- Which steps are copy, transform, decide, approve, or communicate?
- Which inputs are trusted, and which require judgment?
- What mistake would be expensive or embarrassing?
- Which MCP tools already cover the systems involved?

## Interview algorithm

1. Initialize.
   - Choose template.
   - Capture objective and subject.
   - Attach source material.
   - Create session manifest and draft output files.

2. Build the first question plan.
   - Load template required fields.
   - Inspect source material and pre-fill known facts.
   - Rank missing fields by importance and dependency order.
   - Ask one question at a time.
   - For each question, show the recommended answer when the system has enough context to make one.

3. Ask and update.
   - Record each answer as a source span.
   - Extract facts, entities, relations, decisions, tasks, risks, and questions.
   - Update coverage and confidence.
   - Detect contradictions or ambiguous answers.
   - Update Loom projections and visualizations after each accepted answer or source attachment.

4. Drill down.
   - Ask follow-up questions when an answer lacks evidence, owner, date, scope, example, or decision.
   - Stop drilling when the answer is actionable or the user marks the detail unknown.
   - Keep unknowns as explicit open items.

5. Validate.
   - Show a coverage summary.
   - List weak facts, contradictions, and missing evidence.
   - Ask the user to accept, correct, or defer.

6. Project.
   - Write raw interview transcript to `files`.
   - Write structured session and extracted records to `document`.
   - Write accepted facts and relations to `graph` when available.
   - Write embeddings to `vector`.
   - Write text fields to `search` when available.
   - Append session events to `ledger` when audit is required.

7. Review and promote.
   - Keep LLM-created facts as `suggested` until accepted by the user or policy.
   - Promote accepted vocabulary and relation aliases into project vocabulary.
   - Keep every accepted fact tied to source evidence.

## Real-time workspace projection

The Desktop app should treat guided capture as a live multi-workspace transaction stream. A session
starts as draft data, then promotes accepted facts as the user confirms them.

Per turn:

1. Write the raw question and answer to `files:"guided-capture"` as an append-only session log.
2. Update `document:"guided-capture"` with the current session manifest, coverage state, extracted
   facts, open questions, and template-field status.
3. Upsert provisional graph nodes and edges into `graph:"guided-capture"` when the graph facade is
   available, using `status = suggested` until review.
4. Upsert embeddings for the latest answer, changed sections, and accepted entities into
   `vector:"guided-capture"`.
5. Update `search:"guided-capture"` documents for the current report, answers, tags, and evidence
   snippets when search is available.
6. Append audit events to `ledger:"guided-capture-audit"` for sensitive templates, including answer
   captured, fact suggested, fact accepted, fact rejected, export created, and redaction applied.

Real-time projection does not mean every draft fact becomes durable truth. It means every intermediate
state is captured with status, provenance, and source spans so the user can inspect, accept, reject,
or resume without losing the reasoning path.

## Real-time visualizations

Uldren Desktop should render the interview state live. Visuals are not decorative; they show coverage,
gaps, dependencies, and relationships while the user is still able to answer follow-up questions.

First-class views:

- Mind map: live graph of the subject, entities, ideas, decisions, tasks, risks, evidence, and open
  questions. This is the default visual for broad ideation and investigation.
- Timeline: chronological event stream for investigations, incidents, product launches, and employee
  performance evidence.
- Gantt chart: tasks, owners, dependencies, and due dates for post-mortems, plans, and workflow
  automation.
- Org chart: people, teams, reporting relationships, stakeholders, reviewers, and handoff paths.
- Block diagram: systems, tools, MCP connectors, agentic tools, data flows, approvals, and automation
  boundaries.
- Table view: structured fields, evidence rows, tasks, risks, decisions, metrics, and review status.
- Coverage map: template sections with answered, weak, missing, contradicted, and accepted states.

Visualization rules:

- Every visual node links back to source evidence.
- Suggested facts render differently from accepted facts.
- Missing fields are visible as gaps, not hidden.
- Contradictions appear as explicit conflict markers.
- The user can click a node, table row, timeline item, or chart bar to continue the grilling from
  that point.
- Visual layouts are derived from Loom records and can be regenerated.

The mind map is the most important first view. It turns the grilling session into an immediately
visible model of what is known, what is connected, and what is missing.

## Workspace mapping

| Workspace | Stored data | Query role |
| --- | --- | --- |
| `files:"guided-capture"` | Raw conversation logs, attached source snapshots, markdown reports, template files | Audit source and user-readable output |
| `document:"guided-capture"` | Session manifests, template records, coverage reports, extracted facts, tasks, risks, decisions | CRUD-by-id and deterministic derived records |
| `graph:"guided-capture"` | People, organizations, ideas, events, tasks, decisions, claims, tools, workflows, relations | Relationship chains and timeline questions |
| `vector:"guided-capture"` | Embeddings for answers, sections, concepts, claims, and decisions | Semantic recall across sessions |
| `search:"guided-capture"` | Indexed reports, answers, tags, aliases, source titles, and evidence snippets | Keyword lookup and facets when search lands |
| `sql:"guided-capture-analytics"` | Optional session, coverage, task, decision, and workflow tables | Reports, counts, review queues, ROI summaries |
| `ledger:"guided-capture-audit"` | Optional append-only session, edit, acceptance, and export events | Audit trail for sensitive workflows |

## Annotation model

Use the annotation schema from `specs/studio/MEETINGS.md`.

Additional guided-capture fields:

- `question_id`: question that elicited the fact.
- `answer_turn_id`: turn where the fact was stated.
- `template_field`: template field the annotation satisfies.
- `coverage_weight`: contribution to completion.
- `reviewer`: user or policy that accepted or rejected the annotation.
- `sensitivity`: `public`, `internal`, `confidential`, `hr`, `legal`, or `security`.

## Completion rubric

A session is complete when:

- All required template fields are answered or explicitly marked unknown.
- Required evidence-bearing fields have source spans.
- Contradictions are resolved or listed.
- Every task has an owner or is marked unassigned.
- Every date-sensitive item has a date, date range, or explicit unknown.
- The output report can be generated without inventing facts.

The agent should be allowed to say that the session is not complete. That is the value of the
workflow.

## Skill-chain extensions

The second AI Hero article frames `grill-me` as the front of a larger process: clarify the idea,
turn the conversation into a PRD, break the PRD into independently executable issues, execute with a
feedback loop, and improve the codebase architecture when the design gets hard to navigate.

Uldren Desktop should generalize that chain for non-code work:

- `grill`: interrogate until shared understanding exists.
- `document`: turn the session into the appropriate artifact, such as PRD, post-mortem, review,
  timeline, investigation brief, workflow map, or automation brief.
- `decompose`: break the artifact into tasks, open questions, evidence requests, approvals, and
  follow-ups.
- `execute`: hand selected tasks to agents, MCP tools, app connectors, or human owners.
- `measure`: track completion, quality checks, owner feedback, and follow-up outcomes.
- `refine`: update templates, vocabulary, graph schema, and automation candidates when the session
  reveals missing structure.

The app should let each template opt into the chain. A performance review might stop at `document`;
an automation capture might continue through `decompose` and `execute`; a product post-mortem should
continue through `measure`.

## Uldren Desktop UX

First-class app behavior:

- Template picker.
- Source attachment panel.
- Live coverage checklist.
- One-question interview mode.
- Evidence side panel.
- Contradiction and open-item queue.
- Structured preview.
- Accept or reject extracted facts.
- Export to markdown.
- Commit to Loom.
- Resume prior session.
- Compare two sessions.

The app should support voice input later, but the first version can be text-only.

## Privacy and safety

Guided capture will often include sensitive information. Defaults should be conservative:

- Store raw sessions locally unless the user chooses a remote model or sync target.
- Mark HR, legal, investigation, and security templates as sensitive.
- Require explicit confirmation before projecting sensitive facts into broad graph or search
  workspaces.
- Keep source-span evidence for audit, but avoid exposing sensitive spans in generated summaries unless
  the target audience is authorized.
- Support redacted exports.

## Implementation plan

1. Define template schema.
   - Required fields, optional fields, question families, extraction kinds, completion rubric, output
     sections, sensitivity defaults.

2. Build text-only session runner.
   - One question at a time.
   - Session manifest.
   - Raw transcript file.
   - Coverage checklist.

3. Build five starter templates.
   - Concept to full idea.
   - Investigation timeline.
   - Product post-mortem.
   - Yearly performance review.
   - Workflow automation capture.

4. Add source attachment.
   - Attach files, transcripts, URLs, and prior Loom records.
   - Pre-fill known facts from source material.

5. Add extraction and annotation.
   - Extract facts, entities, tasks, decisions, risks, events, tools, and relations.
   - Require evidence spans and confidence.

6. Add review UI.
   - Accept, reject, merge, or edit extracted facts.
   - Promote accepted facts to derived workspaces.

7. Add Loom projections.
   - Write files and documents first.
   - Add vector projection.
   - Add graph and search projections when available.
   - Add ledger for sensitive templates.

8. Add tool-awareness for workflow automation.
   - Inspect installed MCP tools, app connectors, and agentic tools.
   - Map workflow steps to available tools.
   - Identify missing connector requirements.

9. Add resume and compare.
   - Resume incomplete sessions.
   - Compare current and prior sessions for idea evolution, employee review changes, or incident
     follow-up.

## Open questions

- Should guided capture be implemented in Uldren Desktop first, or as a CLI/template engine that the
  desktop app wraps?
- Which templates require local-only model execution by default?
- How should HR and investigation records be permissioned inside a shared Loom?
- What is the minimum graph schema needed before this becomes more than structured markdown?
- Should tool-awareness inspect only installed tools, or also suggest installable connectors?
- How should the app distinguish user statements, source evidence, and model inferences in the UI?
