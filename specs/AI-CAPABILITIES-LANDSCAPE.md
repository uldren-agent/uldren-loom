# AI Capabilities & Agent OS - Landscape

**Status:** Exploratory / vision note. **Version:** 0.1.0-draft. **Normative?** No.
**Relates to:** 0015 (programs), 0029 (triggers), 0030 (observability), 0013 (the facet catalog), and
each facet spec (0011 sql, 0016 graph, 0017 vector, 0018 ledger, 0019 kv, 0020 document, 0021
append-log, 0022 time-series, 0023 columnar, 0024 cas). **Grows from:** `EVENTS-TRIGGERS-LANDSCAPE.md`
§9 (use cases).

This note explores what an AI assistant can do when its memory is a Loom: not a vector store bolted to
a chat loop, but a content-addressed, versioned, branchable, deterministic substrate with four legs.
It is a vision document, deliberately broad rather than precise, to map the design space. House
conventions apply: no em-dashes, no emoji, every claim reads as current fact.

## 1. The four legs, and why they matter for an agent

An agent on Loom has four capabilities most memory stacks lack:

- **Store** (0002, the facet catalog 0013): one substrate that is files, tables, key-value, documents,
  a graph, vectors, columns, logs, time-series, blobs, and a verifiable ledger at once. The agent does
  not pick a database per data shape; it picks a facet.
- **Version** (0002, 0003): every memory state is a commit. The agent can branch to explore a
  hypothesis, diff what changed, merge what worked, and roll back what did not. Memory has history, not
  just a current value.
- **Compute** (0015): the agent authors a program and runs it deterministically, metered, sandboxed,
  on a throwaway branch, and reviews the diff before anything merges. Logic lives next to the data.
- **Observe and react** (0030 observe, 0029 act): the store can tell the agent that it changed, and a
  stored program can run on a schedule or on a change. The memory is no longer passive.

Four properties of the substrate turn these legs into capabilities that a plain database plus a vector
index cannot offer:

- **Content addressing** gives dedup, verifiable integrity, and replay for free: identical memory
  dedupes, every state is verifiable, and any computation over fixed inputs is cacheable and
  reproducible.
- **Branch and merge** gives hypothesis exploration: the agent forks its memory, tries a line of
  reasoning, and keeps it only if it improves on the trunk.
- **Determinism** gives reproducible memory operations: a consolidation run yields the same result
  every time, so memory maintenance is auditable rather than mysterious.
- **One syncable file** gives portability: the whole memory, history included, moves between agent
  instances and machines, and merges rather than overwrites.

## 2. Capabilities by storage facet (at least one each)

| Facet | Agent capability | Legs used | Why Loom specifically |
| ----- | ---------------- | --------- | --------------------- |
| **Files** (0003 fs) | Draft-and-revise memory: keep notes, transcripts, and documents as files; draft an alternative on a branch; diff and keep or discard | store, version, compute, observe | branching lets the agent try a rewrite without destroying the original; observe notices an external edit and reconciles |
| **Relational / SQL** (0011) | Structured fact base: entities, attributes, and relations as rows the agent queries and transacts | store, version, compute | point-in-time queries (open the table "as of" last week) let the agent reason about what it believed before, not just now |
| **Key-value** (0019) | Working memory and settings: session state, scratchpad values, feature flags, per-task counters | store, version | versioned KV means a bad config change is one rollback away, and every setting has a change history |
| **Document** (0020) | Cache of structured observations: tool outputs, API responses, and parsed results as JSON/CBOR with indexed lookup | store, compute, observe | content addressing dedupes identical observations; a change feed recomputes a derived index when documents land |
| **Graph** (0016) | Associative memory and multi-hop reasoning: a knowledge graph of entities and their relationships, traversed and queried recursively | store, compute | recursive queries (Datalog via the integrated ascent path, 0015) over a versioned graph, so the agent can ask "what connects A to D" and branch the graph to test a new link |
| **Vector** (0017) | Semantic recall: embed everything and retrieve by similarity, the RAG-over-own-memory case | store, version, compute, observe | branch the index to test a different chunking or embedding model, compare recall, merge only if better; rebuild the index on change rather than syncing it (0013 RD4) |
| **Columnar** (0023) | Self-analytics: aggregate over the agent's own interaction history (tools used, success rates, cost) | store, compute | read-optimized scans over large behavioral logs without an external warehouse, versioned alongside the rest of memory |
| **Append-log / queue** (0021) | Inbox and task queue: work the agent must do, messages to process, enqueued by users or by triggers and drained by the agent | store, observe, act | a durable, ordered, replayable queue inside the same memory file; a change trigger fires the agent when work arrives |
| **Time-series** (0022) | Longitudinal tracking: quantities over time such as a user's stated mood, token spend, latencies, or sensor readings, with downsampled rollups | store, compute, act | rollups are derived views recomputed by a scheduled program (0029), so the agent keeps cheap long-horizon summaries |
| **CAS** (0024) | Artifact vault: every generated artifact (an image, a file, a model output) stored by digest, deduped and verifiable | store | identical artifacts dedupe globally; an artifact is addressable and integrity-checked, so the agent can prove what it produced |
| **Ledger** (0018) | Accountable action log: a tamper-evident, optionally signed record of decisions, approvals, and actions taken | store, compute, act | hash-chained and verifiable, so the agent (or an auditor) can prove the sequence of what it did and that nothing was altered |

## 3. Cross-cutting capabilities (facets plus programs, triggers, observability)

These are the capabilities that emerge only when the legs combine:

- **Hypothesis branches.** The agent forks memory, runs a reasoning program against the fork under a
  budget, and inspects the diff. If the branch improves on trunk, it merges; if not, it discards. This
  is the run-on-a-branch gate (0015 §8) used as a thinking tool, not just a safety tool.
- **Reactive derived memory.** A change to one workspace (a new transcript file) triggers
  (0029) a program that updates a derived workspace (re-embeds into the vector index, extracts entities
  into the graph), observed through the change feed (0030). Memory keeps itself coherent.
- **Scheduled reflection.** A nightly time trigger runs a consolidation program: summarize the day,
  promote durable facts from working memory to the fact base, prune noise. Reflection is a cron job,
  not a hope.
- **Deterministic, auditable memory operations.** Because consolidation is a deterministic program over
  content-addressed inputs, the same inputs yield the same result, every run is replayable, and the
  ledger records what each run changed. The agent's memory hygiene is inspectable.
- **Self-authored logic, safely.** The agent writes a small program (a validation rule, a derived
  view, a workflow), runs it gated, and a guard (CEL, integrated at 0015 §6) or a human approves the
  diff before merge. The agent extends its own behavior without an unattended write to trunk.
- **Config promotion on a clock.** The agent (or an operator) stages a configuration change on a
  branch; a scheduled program promotes it to the live ref only after a soak window with no error
  signal, and rolls back otherwise. A clever, low-risk way to let an agent change its own settings or a
  fleet's config without a human in every loop. (Carried from `EVENTS-TRIGGERS-LANDSCAPE.md` §9.)

## 4. The Agent OS: standing programs

If a single program is an app, a set of standing programs that keep an agent's memory healthy,
responsive, and accountable is an operating system for the agent. Each is a content-addressed program
(0015 §7) bound to a schedule or a change (0029), running under least privilege (its manifest grants,
0015 §6, and its `run_as` principal, 0029 §8). A proposed starter set:

1. **Memory maintenance / consolidation.** Nightly. Summarizes recent activity, promotes durable facts
   from working memory (KV/document) to the fact base (SQL/graph), dedupes, and compacts. The canonical
   example; gated so the agent reviews what was consolidated.
2. **Forgetting / garbage collection.** Periodic. Applies a decay or value policy to prune low-value
   memories, while retention holds (0009 §7) pin the ones that must never be dropped. Forgetting is a
   policy, not an accident.
3. **Indexer.** Change-driven (0030). Rebuilds the vector, graph, and document indexes when their
   underlying data changes, treating indexes as derived and rebuildable (0013 RD4) rather than syncing
   them.
4. **Reflection / synthesis.** Weekly or on a milestone. Synthesizes patterns across recent memory into
   higher-level notes ("what I learned about this user," "what tends to fail"), written to the file or
   document facet for later recall.
5. **Attention / notifier.** Change-driven. Watches for conditions the agent or user cares about (a
   threshold crossed in the time-series facet, a new high-priority item in the queue) and surfaces them.
6. **Guard / policy enforcer.** On every proposed merge to a protected workspace. A read-only CEL
   program (0015 §6) validates the transition (no PII in the public workspace, schema invariants hold)
   and fails closed.
7. **Config promoter on a clock.** Scheduled. Promotes staged config after a clean soak window, rolls
   back otherwise (§3).
8. **Inbox / queue processor.** Change-driven or scheduled. Drains the append-log task queue, executing
   or dispatching each item, advancing the consumer cursor.
9. **Integrity sweeper.** Daily. Runs verify/fsck (0009 §4) over the workspaces and appends the result
   to a ledger workspace, so memory integrity has a tamper-evident history.
10. **Replication agent.** Scheduled. Pushes and pulls memory between agent instances or to a backup
    (0006), so the same memory is available across machines and survives loss.
11. **Watchdog.** Change-driven. Detects anomalous or runaway patterns (a flood of writes, a budget
    repeatedly exhausted) and pauses the offending binding, failing closed.
12. **Journaler.** On every significant action. Appends a signed ledger entry (0018) recording the
    decision and its rationale, building the accountable action log of §2.

These compose: the indexer and watchdog are change-driven (observe), consolidation and reflection are
scheduled (time), the guard and journaler run inline with writes, and all of them are ordinary
programs the agent or operator can read, diff, branch, and replace. An Agent OS is therefore not a
framework to adopt but a set of small programs to install into a Loom, each independently revocable and
auditable.

## 5. A day in the life (worked illustration)

Morning: a user message lands in the agent's inbox (append-log); a change trigger wakes the inbox
processor, which drafts a reply on a branch, runs a guard (no secrets leaked), and merges. Through the
day: each tool call writes a document (observation) and a time-series point (latency, cost); the
indexer re-embeds new documents into the vector facet on change; the journaler appends a signed ledger
entry per action taken. Evening: a clean soak window passes, so the config promoter merges a staged
prompt change to the live ref. Night: the consolidation program summarizes the day, promotes durable
facts to the graph and SQL facets, and the forgetting program prunes low-value scratch; the integrity
sweeper verifies everything and records the result. The whole day, history included, is one `.loom`
file the agent can branch, diff, sync to another instance, and replay.

## 5b. Feature idea: the browser as a capture surface (scrape-to-vector)

A concrete, near-term shape for the browser build. Ship the `wasm32` Loom as a browser extension so a
person, while they browse, can capture the current page or a whole site straight into a local memory
workspace (`vector` for semantic recall, or `files`/`document` for the raw content), indexed on
device with no cloud. It is a private, live, incremental version of LEANN's browser-history app: read
the web, and your memory builds itself, with history and dedup for free.

Why Loom fits this unusually well. The capture lands as content-addressed objects, so revisiting a
page or scraping overlapping sites dedupes automatically. Each capture session is a commit, so the
memory has a timeline the agent can diff and roll back. Semantic recall uses exact vector search,
which is feature-complete and interactive at the corpus size a single person's browsing produces (see
`prototypes/vector-tradeoff`), and embeddings come from a remote or host-provided encoder so the
browser carries no heavy model. The whole memory is one `.loom` the person can sync to their laptop,
where the same data gains the native accelerators (HNSW, Polars) without any change to results.

In the parity model this is a **web-only** capability (0032): it lives in the browser's page context
and has no native analogue, the mirror image of the native-only server roles. It pairs the four legs
(store, version, compute, observe) with the one surface only a browser has, the live page.

## 6. Pointers

- Programs and the run-on-a-branch gate: `specs/0015-execution-and-logic.md`.
- Triggers and the keeper: `specs/0029-events-and-triggers.md`.
- Observability and change feeds: `specs/0030-observability.md`.
- The facet catalog and per-facet specs: `specs/0013-extended-capabilities.md` and 0011/0016-0024.
- Use-case seeds: `specs/EVENTS-TRIGGERS-LANDSCAPE.md` §9; README "What you can build".
