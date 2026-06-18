# Analytical datatypes and pattern detection

## Idea

Define the semantic datatypes and analytical layer that let Loom find needles, surface patterns, and
support prediction-oriented workflows over large corpora.

The important correction from the previous discussion is terminology: `files`, `document`, `vector`,
`graph`, `sql`, and `time-series` are Loom workspace types. They describe isolation, history, refs,
and storage behavior. The user-facing mental model should be datatypes: source files, spans,
entities, claims, measurements, events, signals, patterns, anomalies, predictions, evaluations, and
review items.

Workspaces answer where a record lives. Datatypes answer what a record means.

The system should not depend on one large prompt, one vector database, or one graph. It should convert
large raw data into durable typed records, then run analytical jobs over those records. That is how
Loom can identify a negative-result research paper inside a corpus of positive papers, or surface a
stock trading setup that has repeated under similar market conditions.

## Sources checked

- Loom workspaces are independent named typed trees inside one Loom. A workspace has an id, name,
  type, refs, and root. Workspace types include `files`, `vcs`, `sql`, `kv`, `document`, `vector`,
  `graph`, `columnar`, `queue`, `time-series`, `cas`, `ledger`, `search`, and `program`.
  Source: `specs/0014-workspaces.md:1`, `specs/0014-workspaces.md:25`.
- Workspace reads may span workspaces only through explicit capability-gated cross-workspace reads,
  while writes and history operations never cross a workspace.
  Source: `specs/0014-workspaces.md:99`.
- The memory controller is the component that classifies intent, resolves entities, plans retrieval,
  ranks evidence, packs context, mediates tool calls, and writes durable memory back to Loom.
  Source: `specs/todos/0006-loom-memory-controller.md:94`,
  `specs/todos/0006-loom-memory-controller.md:119`.
- The GPU executable memory prototype ladder defines a path from CPU-side retrieval to GPU vector
  snapshots, GPU graph snapshots, hybrid snapshots, KV cache snapshots, retrieval cross-attention,
  and adapter-based memory.
  Source: `specs/todos/0007-gpu-executable-memory-prototypes.md:60`.

## Mental model

The correct stack is four layers:

```text
Storage layer
  Raw files, document stores, vectors, graph edges, tables, time series, search indexes.

Semantic datatype layer
  Source, span, entity, claim, measurement, event, signal, feature, pattern, anomaly,
  prediction, evaluation, review item.

Analytical layer
  Extraction, normalization, clustering, consensus building, outlier scoring, pattern mining,
  forecasting, backtesting, review routing.

Presentation and action layer
  Reports, dashboards, evidence packs, mind maps, timelines, alerts, agent tasks, user review.
```

The workspace layer remains critical, but it should not be the user-facing ontology. A `claim` may
be stored as a JSON document, indexed in search, connected in graph, embedded in vector space, and
projected into SQL. The datatype is `claim`. The storage projections are implementation choices.

## Why large workloads benefit from AI

The workloads that benefit most from AI have more context than a person can hold in working memory,
and the useful signal is scattered across many files, dates, people, systems, or formats.

AI adds value when the task is not simple lookup:

- It extracts structure from messy sources.
- It normalizes equivalent concepts that are written differently.
- It links evidence across documents and time.
- It summarizes competing explanations.
- It finds contradictions, gaps, and outliers.
- It proposes patterns and hypotheses for review.
- It writes durable intermediate records so future queries do not pay the full token cost again.

The key product shift is from passive storage to active memory. Loom should not merely hold PDFs,
transcripts, tables, code, and logs. It should hold typed analytical records derived from them, with
source-span provenance.

## Core datatypes

### Source

A source is the raw thing that entered Loom.

Examples:

- Research paper PDF.
- Earnings call transcript.
- YouTube transcript.
- Meeting note.
- Git commit.
- Support ticket.
- Lab notebook.
- CSV or Parquet dataset.
- News article.

Required fields:

```text
id
content_digest
source_kind
origin
created_at
ingested_at
version
access_policy
metadata
```

Storage projection:

- `files` for original bytes and deterministic normalized renderings.
- `document` for source metadata.
- `search` for full-text indexing.

### Span

A span is a precise region inside a source. Every derived record should point back to one or more
spans.

Examples:

- Page 7 paragraph 2 of a paper.
- Timestamp 00:14:32 to 00:15:10 in a transcript.
- Cell range `B7:E12` in a spreadsheet.
- Lines 120 to 160 in a source file.
- Table 2 row 4 of a study.

Required fields:

```text
id
source_id
locator
text
text_digest
char_range
page_range
time_range
table_locator
```

Storage projection:

- `document` for span records.
- `vector` for embeddings.
- `search` for exact text and filtered search.

### Entity

An entity is a normalized thing that can be mentioned many ways.

Examples:

- Person.
- Organization.
- Gene.
- Disease.
- Compound.
- Product.
- Ticker.
- Location.
- Method.
- Dataset.
- Instrument.
- Regulation.
- Software component.

Required fields:

```text
id
kind
canonical_name
aliases
external_ids
first_seen_at
confidence
merge_state
```

Storage projection:

- `document` for entity records.
- `graph` for entity relationships.
- `search` for alias lookup.
- `vector` for fuzzy entity matching.

### Claim

A claim is a structured assertion extracted from a source. This is the most important datatype for
contradiction detection.

Example:

```text
subject: Compound X
predicate: reduces
object: tumor growth
context: mouse model Y, dose Z, 14 days
polarity: positive
evidence_strength: moderate
source_spans: [paper-a page 7 paragraph 2, paper-a table 2]
```

Required fields:

```text
id
subject_entity_id
predicate
object_entity_id
context_id
polarity
modality
confidence
evidence_strength
source_span_ids
extractor_id
review_state
```

Polarity values:

```text
positive
negative
null
mixed
unclear
```

Modality values:

```text
observed
reported
hypothesized
recommended
predicted
disputed
```

Storage projection:

- `document` for claim records.
- `graph` for subject-predicate-object and claim-to-evidence links.
- `vector` for semantic claim clustering.
- `sql` or `columnar` for aggregation and comparison.

### Measurement

A measurement is a numeric or categorical observation that can be compared.

Examples:

- Effect size.
- P-value.
- Confidence interval.
- Sample size.
- Price.
- Volume.
- Revenue.
- Gross margin.
- Variant allele frequency.
- Blood marker value.
- Latency.
- Error rate.

Required fields:

```text
id
measurement_kind
value
unit
time
entity_ids
context_id
source_span_ids
uncertainty
method
```

Storage projection:

- `sql`, `columnar`, or `time-series` for numeric analysis.
- `graph` for links to entities, claims, and contexts.
- `document` for raw extraction record.

### Context

A context describes the conditions under which a claim, event, or measurement is true.

Examples:

- Study population.
- Experiment setup.
- Market regime.
- Geography.
- Time window.
- Software version.
- Customer segment.
- Data collection method.

Required fields:

```text
id
context_kind
attributes
entity_ids
time_range
source_span_ids
confidence
```

Storage projection:

- `document` for context records.
- `graph` for context-to-claim and context-to-measurement links.
- `sql` or `columnar` for filtering.

### Event

An event is a timestamped thing that happened.

Examples:

- Earnings release.
- Guidance change.
- Insider purchase.
- FDA approval.
- Paper publication.
- Experiment run.
- Incident.
- Meeting decision.
- Pull request merge.
- Customer churn.

Required fields:

```text
id
event_kind
time
entity_ids
description
source_span_ids
confidence
```

Storage projection:

- `time-series` for ordered event streams.
- `graph` for event participants and causal links.
- `document` for canonical event records.

### Signal

A signal is a computed observation derived from sources, events, measurements, or claims.

Examples:

- Sentiment shift.
- Volume spike.
- Volatility expansion.
- Citation acceleration.
- Topic emergence.
- Replication failure count.
- Claim polarity imbalance.
- Customer complaint frequency.
- Executive language drift.

Required fields:

```text
id
signal_kind
entity_ids
time_range
value
method
inputs
confidence
```

Storage projection:

- `time-series` for signal history.
- `sql` or `columnar` for feature generation.
- `graph` for signal-to-entity links.

### Feature

A feature is a model-ready variable with explicit availability time.

Features are different from signals because features are built to feed analytical or predictive
models. Every feature must know when it became available so backtests do not leak future information.

Required fields:

```text
id
feature_name
entity_id
event_time
available_time
value
source_signal_ids
transform_id
```

Storage projection:

- `columnar` or `sql` for model input.
- `time-series` for temporal joins.
- `document` for transform metadata.

### Pattern

A pattern is a repeated combination of signals, features, claims, events, or relationships.

Examples:

- A company mentions inventory weakness, then margins fall two quarters later.
- A compound shows positive animal studies but repeated null replication in human cohorts.
- A support ticket theme appears before an incident.
- A code module with rising churn and falling test coverage produces more bugs.

Required fields:

```text
id
pattern_kind
definition
input_datatypes
support_count
examples
counterexamples
confidence
review_state
```

Storage projection:

- `document` for pattern definitions and examples.
- `graph` for links to examples and entities.
- `sql` or `columnar` for support statistics.

### Anomaly

An anomaly is a record saying that one item deviates from an expected group, baseline, consensus, or
historical pattern.

Examples:

- One paper reports null results while many comparable papers report positive results.
- One cohort has the opposite effect direction.
- One earnings call uses unusually negative language.
- One experimental run has a measurement outside the historical range.
- One table contradicts a paper's abstract.

Required fields:

```text
id
anomaly_kind
target_id
comparison_set_id
score
reason
detector_id
source_span_ids
review_state
```

Storage projection:

- `document` for anomaly records.
- `graph` for anomaly-to-target and anomaly-to-comparison links.
- `sql` or `columnar` for detector outputs.

### Prediction

A prediction is a forward-looking statement generated by a model or rule.

Examples:

- Expected price move over 20 trading days.
- Probability of replication failure.
- Likelihood that a customer escalates.
- Expected project delay.
- Risk of incident recurrence.

Required fields:

```text
id
prediction_kind
target_entity_id
horizon
created_at
expires_at
prediction
confidence
evidence_ids
model_id
invalidation_conditions
```

Storage projection:

- `document` for prediction records.
- `time-series` for prediction history.
- `graph` for prediction-to-evidence links.

### Evaluation

An evaluation records whether a prediction, extraction, anomaly, or pattern held up later.

Required fields:

```text
id
evaluated_record_id
evaluation_kind
outcome
score
evaluated_at
evidence_ids
notes
```

Storage projection:

- `document` for evaluation records.
- `sql` or `columnar` for performance metrics.
- `graph` for feedback loops.

### Review item

A review item is a durable work queue entry for a human or model.

Examples:

- Confirm whether a contradiction is real.
- Merge duplicate entities.
- Validate an extracted table.
- Approve a prediction report.
- Resolve a low-confidence claim.

Required fields:

```text
id
review_kind
target_id
priority
created_at
assignee
status
resolution
```

Storage projection:

- `queue` for operational review flow.
- `document` for review records.
- `graph` for review-to-target links.

## Datatype to workspace mapping

One datatype can be projected into several workspaces.

| Datatype | Primary workspace type | Secondary projections |
| --- | --- | --- |
| Source | `files` | `document`, `search` |
| Span | `document` | `vector`, `search` |
| Entity | `document` | `graph`, `search`, `vector` |
| Claim | `document` | `graph`, `vector`, `sql`, `columnar` |
| Measurement | `sql` or `columnar` | `time-series`, `graph`, `document` |
| Context | `document` | `graph`, `sql`, `columnar` |
| Event | `time-series` | `graph`, `document`, `search` |
| Signal | `time-series` | `sql`, `columnar`, `graph` |
| Feature | `columnar` | `time-series`, `document` |
| Pattern | `document` | `graph`, `sql`, `columnar` |
| Anomaly | `document` | `graph`, `sql`, `columnar`, `queue` |
| Prediction | `document` | `time-series`, `graph` |
| Evaluation | `document` | `sql`, `columnar`, `graph` |
| Review item | `queue` | `document`, `graph` |

This keeps the model clean:

- Datatypes are the product and analytical contract.
- Workspaces are the storage and history contract.
- Projections are reproducible derived views.

## Analytical pipeline

The general pipeline is:

```text
source ingestion
-> span extraction
-> entity extraction
-> entity resolution
-> claim, event, measurement, and context extraction
-> datatype validation
-> projection into storage workspaces
-> clustering and aggregation
-> anomaly detection and pattern mining
-> prediction or review routing
-> evaluation and feedback
```

The LLM should be used where semantic judgment matters:

- Extracting claims from prose.
- Resolving ambiguous entities.
- Explaining why an anomaly matters.
- Generating human-readable hypotheses.
- Choosing which evidence should be shown in a review pack.

Deterministic or statistical jobs should be used where repeatability matters:

- Numeric aggregation.
- Distance scoring.
- Time-window joins.
- Backtesting.
- Feature availability checks.
- Threshold-based alerting.

## Needle finding in research papers

Scenario: 100 papers report that Compound X reduces tumor growth, but one comparable paper reports a
null or negative result.

If Loom stores only raw files and vectors, the negative paper may stay buried. The system needs typed
claims, measurements, contexts, and claim clusters.

Example extracted claim:

```text
claim:
  subject: Compound X
  predicate: reduces
  object: tumor growth
  context: mouse model Y, dose Z, 14 days
  polarity: positive
  source_spans: [paper-a page 7 paragraph 2]
```

Example negative-result claim:

```text
claim:
  subject: Compound X
  predicate: reduces
  object: tumor growth
  context: mouse model Y, dose Z, 14 days
  polarity: null
  source_spans: [paper-z table 2, paper-z results paragraph 4]
```

The detector should:

1. Normalize entities.
2. Normalize claim predicates.
3. Normalize contexts.
4. Cluster comparable claims.
5. Compute cluster consensus.
6. Compare each claim to the cluster.
7. Score deviations.
8. Produce an anomaly record with evidence spans.
9. Route the anomaly to review.

The output should be evidence-backed:

```text
This paper reports a null effect for Compound X on tumor growth in mouse model Y, while the comparable
claim cluster contains 100 positive claims. The divergence is based on Table 2 and the Results
section. The context match is high because dose, model, and endpoint align.
```

## Anomaly types

The first anomaly detector family should support these types:

| Anomaly type | What it means |
| --- | --- |
| Polarity anomaly | Most claims say positive, one says negative or null. |
| Magnitude anomaly | Same direction, unusually different effect size. |
| Method anomaly | Same claim, but a method or assay differs in a material way. |
| Population anomaly | Result appears only in one cohort or fails in another. |
| Temporal anomaly | Newer evidence reverses older consensus. |
| Citation anomaly | Many claims depend on one weak or repeated source. |
| Replication anomaly | Initial claim is positive, replication is null or negative. |
| Missing-data anomaly | A comparable study omits a measurement others report. |
| Internal-contradiction anomaly | Abstract, conclusion, table, or figure disagree. |
| Language anomaly | Narrative strength does not match numeric evidence. |

## Consensus records

Consensus should be a derived datatype or a derived view over claim clusters.

Consensus fields:

```text
id
claim_cluster_id
entity_ids
context_id
claim_count
positive_count
negative_count
null_count
mixed_count
weighted_direction
weighted_effect_size
method_diversity
sample_size_total
evidence_quality_score
last_updated_at
```

Consensus is not truth. It is a current aggregate state over available evidence. The system should
make this distinction visible so a corpus-wide majority does not erase a high-quality contradiction.

## Pattern surfacing

Pattern surfacing looks for repeated structures rather than one-off deviations.

A pattern can emerge from:

- Repeated entity relationships.
- Repeated event sequences.
- Repeated signal combinations.
- Repeated claim structures.
- Repeated measurement movements.
- Repeated graph motifs.
- Repeated language shifts.

Examples:

- A gene repeatedly appears near a disease and pathway across papers.
- A company mentions inventory correction, then margins fall later.
- Customer complaints rise before an incident.
- A code area has churn, ownership loss, and rising defects.
- A product idea appears in meetings before becoming roadmap work.

Useful pattern-mining methods:

- Frequent itemset mining over entities, tags, and signals.
- Sequence mining over events.
- Motif detection over graph neighborhoods.
- Clustering over claim embeddings.
- Change-point detection over time series.
- Topic modeling over spans.
- Correlation screens over features and outcomes.
- Causal-hypothesis tracking for reviewable explanations.

## Prediction workflows

Prediction should be treated as a separate datatype with lifecycle management, not just a chat answer.

Prediction lifecycle:

```text
pattern found
-> hypothesis generated
-> features created with availability time
-> model or rule produces prediction
-> prediction stored with expiry and invalidation conditions
-> outcome observed
-> evaluation stored
-> model, rule, or extraction process adjusted
```

Prediction records must include:

- The horizon.
- The target.
- The known-at time.
- The feature set.
- The evidence.
- The confidence.
- The invalidation conditions.
- The later evaluation.

This is the difference between "the model thinks this stock will go up" and a reproducible analytical
system that can be judged later.

## Stock trading pattern example

Trading is a useful stress test because false patterns are easy to create.

Raw inputs:

- Prices.
- Volume.
- Options data.
- Earnings dates.
- Guidance changes.
- News.
- Filings.
- Earnings call transcripts.
- Analyst revisions.
- Insider transactions.
- Macro data.
- Sector performance.

Derived datatypes:

- `event`: earnings release, guidance cut, insider buy.
- `measurement`: revenue, margin, volume, volatility.
- `signal`: sentiment shift, volume spike, estimate revision, language drift.
- `feature`: known-at-time model input.
- `pattern`: recurring setup.
- `prediction`: expected move over a horizon.
- `evaluation`: realized return, drawdown, hit rate, calibration.

Example pattern:

```text
pattern:
  kind: earnings-language-drift
  definition:
    - inventory concern language increases quarter over quarter
    - management lowers forward-looking certainty
    - sector relative strength weakens
    - options implied volatility expands
  outcome:
    - elevated probability of 20 trading day underperformance
```

Required guardrails:

- No lookahead bias.
- No survivorship bias.
- No revised data unless it was available at the prediction time.
- No future transcript, price, or macro data in past features.
- Walk-forward validation.
- Out-of-sample evaluation.
- Regime labels.
- Transaction-cost assumptions.
- Counterexamples preserved.

Loom's advantage is not that an LLM predicts stocks directly. Loom's advantage is that it can preserve
what was known when, version feature pipelines, store predictions before outcomes, and evaluate later.

## Workload families

### Scientific research

How AI helps:

- Extracts genes, proteins, diseases, pathways, compounds, methods, cohorts, and instruments.
- Extracts claims and evidence spans.
- Links claims to tables, figures, and methods.
- Detects contradictions, weak evidence, and replication gaps.
- Tracks how an idea developed across papers and time.

Why it matters:

Researchers need to know not only which papers mention a topic, but what each paper claims, under
which conditions, with which evidence, and how that compares with the rest of the field.

Primary datatypes:

- Source.
- Span.
- Entity.
- Claim.
- Measurement.
- Context.
- Pattern.
- Anomaly.
- Review item.

### Finance and markets

How AI helps:

- Extracts business drivers, risks, guidance, and management claims.
- Compares current language with prior filings and calls.
- Links narrative to numbers.
- Detects sentiment, risk, and guidance shifts.
- Builds and evaluates trading or investing hypotheses.

Why it matters:

The hard part is tracking how a company's story changes over time and whether the numbers support
that story.

Primary datatypes:

- Source.
- Entity.
- Event.
- Measurement.
- Signal.
- Feature.
- Pattern.
- Prediction.
- Evaluation.

### Legal, compliance, and investigations

How AI helps:

- Extracts people, organizations, dates, events, locations, documents, and claims.
- Builds timelines.
- Finds conflicting statements.
- Preserves evidence trails.
- Creates review queues for unresolved facts.

Why it matters:

The central question is often not "find a document." It is "reconstruct what happened, who knew it,
when they knew it, and which evidence supports that interpretation."

Primary datatypes:

- Source.
- Span.
- Entity.
- Event.
- Claim.
- Context.
- Anomaly.
- Review item.

### Product and company memory

How AI helps:

- Extracts decisions, owners, risks, tasks, assumptions, and unresolved questions.
- Links meetings to specs, tickets, pull requests, incidents, customers, and outcomes.
- Tracks why decisions were made.
- Summarizes project history without relying on one person's memory.

Why it matters:

Teams often remember the final decision but lose the tradeoffs, rejected options, and context that
made the decision reasonable at the time.

Primary datatypes:

- Source.
- Span.
- Entity.
- Event.
- Claim.
- Task.
- Pattern.
- Review item.

### Code intelligence

How AI helps:

- Maps components, APIs, dependencies, invariants, and ownership.
- Links bugs to commits, tickets, tests, logs, and incidents.
- Finds similar changes.
- Extracts contracts and migration constraints.
- Surfaces risky code areas.

Why it matters:

A useful code assistant needs the current code, the history, the design rationale, the incidents, and
the tests. The repo alone is not the whole memory.

Primary datatypes:

- Source.
- Span.
- Entity.
- Event.
- Measurement.
- Signal.
- Pattern.
- Anomaly.

### Media intelligence

How AI helps:

- Transcribes and chunks long-form audio and video.
- Extracts topics, people, products, claims, recommendations, and examples.
- Tracks recurring ideas across episodes.
- Finds where an idea first appeared and how it changed.

Why it matters:

A single summary is useful once. A channel-level or podcast-level memory graph becomes a reusable
research database.

Primary datatypes:

- Source.
- Span.
- Entity.
- Claim.
- Event.
- Pattern.
- Anomaly.

### Medical and bio research

How AI helps:

- Links diseases, symptoms, drugs, genes, variants, pathways, trials, outcomes, and protocols.
- Extracts study endpoints, biomarkers, adverse events, and inclusion criteria.
- Supports literature-backed interpretation.
- Keeps privacy-sensitive memory local when required.

Why it matters:

The data is dense, specialized, and sensitive. Local structured memory is useful only if provenance,
review, and governance are first-class.

Primary datatypes:

- Source.
- Span.
- Entity.
- Claim.
- Measurement.
- Context.
- Anomaly.
- Review item.

## Detection algorithms

The initial detector set should be small and explainable.

Claim polarity detector:

```text
input: claim cluster
output: anomaly records for claims whose polarity differs from weighted consensus
```

Effect-size detector:

```text
input: claim cluster plus measurements
output: anomalies whose effect size is outside robust cluster bounds
```

Context-split detector:

```text
input: claim cluster grouped by context fields
output: contexts where direction or magnitude changes materially
```

Temporal-reversal detector:

```text
input: claim cluster ordered by publication or event time
output: time windows where consensus direction changes
```

Internal-consistency detector:

```text
input: claims, measurements, spans from one source
output: anomalies where abstract, conclusion, table, or figure disagree
```

Pattern miner:

```text
input: events, signals, features, outcomes
output: pattern records with examples, counterexamples, and support statistics
```

Prediction evaluator:

```text
input: predictions and later outcomes
output: evaluation records with calibration and error metrics
```

## Quality controls

Analytical datatypes are only useful if their provenance and confidence are explicit.

Required controls:

- Every derived record links to source spans or derived inputs.
- Every extraction records extractor identity and version.
- Entity merges are reviewable.
- Low-confidence claims enter a review queue.
- Counterexamples are preserved with patterns.
- Consensus records expose minority evidence.
- Prediction features include availability time.
- Backtests are reproducible from a Loom commit.
- Anomalies explain why they were flagged.
- Human review decisions are stored as evaluations.

## Desktop experience

Uldren Desktop can make this feel alive rather than batch-only.

Views:

- Corpus map: sources, entities, claims, and clusters.
- Evidence graph: entities, claims, spans, and measurements.
- Timeline: events, signals, predictions, and outcomes.
- Consensus board: claim clusters with majority and minority evidence.
- Anomaly inbox: reviewable contradictions and outliers.
- Pattern explorer: recurring setups with examples and counterexamples.
- Prediction ledger: predictions, expiry, outcomes, and calibration.
- Feature audit: what was known at each point in time.

The important interaction is drilldown:

```text
pattern or anomaly
-> explanation
-> supporting records
-> exact source spans
-> raw file
```

## Prototype order

1. Build the datatype schemas as `document` records with source-span links.
2. Add extraction for sources, spans, entities, claims, measurements, contexts, and events.
3. Add projections into `graph`, `vector`, `search`, and `sql` or `columnar`.
4. Build claim clustering and consensus records.
5. Build the first polarity and internal-consistency anomaly detectors.
6. Build the review queue.
7. Add time-series signals and features.
8. Add pattern records with examples and counterexamples.
9. Add prediction records and later evaluations.
10. Add Desktop views for anomaly inbox, consensus board, and pattern explorer.

## Open questions

1. Should `claim`, `entity`, `event`, `pattern`, `anomaly`, `prediction`, and `evaluation` remain
   generic `document` records at first, or should Loom define typed facades for them after the first
   prototype proves stable?
2. Should consensus be stored as a datatype record, or generated as a view over claim clusters?
3. What minimum source-span model is needed to cover PDFs, transcripts, spreadsheets, code, and
   tables without overfitting to one format?
4. Which extraction tasks should require human review before graph projection?
5. Should prediction records be allowed to trigger agents automatically, or should they only produce
   review items until governance is mature?
6. How should Loom represent "known at time" for sources that are ingested late but describe earlier
   events?
7. What is the smallest trading-pattern prototype that can prove the value of feature availability
   and walk-forward evaluation without becoming a trading system first?
