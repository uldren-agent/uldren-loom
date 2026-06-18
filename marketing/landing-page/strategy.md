# Uldren Loom Landing Page Strategy

Source video: [Alex Hormozi's Landing Page Strategy for 2026](https://www.youtube.com/watch?v=zA0B-VwOPn4)

Transcript artifact: `/private/tmp/uldren-loom-hormozi.en.vtt`

## Video Takeaways

The landing page strategy is:

1. Spend most effort above the fold.
   The fold should sell the visitor on staying. Test headline, subheadline, CTA, and hero visual first.

2. Lead with the dream outcome.
   Do not open with "what we are." Open with the result the buyer wants.

3. Make the hero image prove the promise.
   The visual should show the outcome, product, workflow, or proof. It should not be generic atmosphere.

4. Increase perceived likelihood of success.
   Use proof above the fold and throughout the page: logos, stats, real screenshots, demos, testimonials, examples, benchmarks.

5. Reduce risk near the CTA.
   Put concrete trust reducers under the CTA: source-available license, local-first, no required hosted service, stable C ABI, conformance vectors, bindings, reproducible storage.

6. Reduce time delay.
   Say how quickly someone can understand, try, or integrate it.

7. Reduce effort and sacrifice.
   Explain the path in 3 or 4 steps. More steps makes the product feel harder.

8. Make every section headline self-contained.
   Assume visitors only scan headings. "How it works" is weaker than "Create a portable agent memory in three calls."

9. Run continuous CRO.
   If traffic is high, test one meaningful thing per week. If traffic is low, ship larger best-practice revisions and measure directional signals.

## Checked Uldren Facts

The repo supports positioning Uldren Loom as "one interface that is a filesystem, a git-style version history, and a queryable database," packable into a single portable file: `README.md:9`.

The strongest first landing-page angle is AI agent memory: the README says Loom is built as an agent memory substrate with versioned files, vector memory, SQL memory, branching, merging, sync, and MCP: `README.md:22`.

Secondary proof points include MCP, `.loom` backends, polyglot bindings, encryption/compression, SQL/vector data, sync, and version control: `README.md:62`.

SQL is versioned through Loom, with row-level per-table granularity: `crates/loom-sql/src/lib.rs:5`.

The C ABI is the stable contract wrapped by language bindings: `crates/loom-ffi/README.md:3`.

## Recommended Landing Page Scaffold

### Hero

Headline:

```text
Portable memory for AI agents that branches, queries, and syncs like code
```

Subheadline:

```text
Uldren Loom gives agents one local-first substrate for files, SQL tables, vectors, history, and MCP tools, packed into a portable .loom store.
```

Primary CTA:

```text
Read the docs
```

or:

```text
Start with the CLI
```

Secondary CTA:

```text
View GitHub
```

Trust line under CTA:

```text
Rust core. Stable C ABI. Node, WASM, Python, JVM, C/C++, iOS, Android, and React Native bindings planned or present per repo docs.
```

Hero visual:

Use an actual product or architecture visual, not abstract art. Best first version: a split interactive diagram showing an agent writing notes, SQL facts, vector embeddings, and files into one `.loom` store, then branching, querying, and syncing.

### Sections

1. `One store for every kind of agent memory`
   Show cards or rows for files, SQL, vectors, documents, logs, config, and history.

2. `Branch experiments without corrupting main memory`
   Explain branch, test, merge, diff, rollback for agent state.

3. `Query memory instead of scraping folders`
   Show SQL and vector search examples side by side.

4. `Move the whole repository as one file`
   Emphasize `.loom` portability, local-first use, copy/encrypt/move, and backend flexibility.

5. `Built for every runtime`
   Show bindings and stable C ABI. Keep this factual and status-aware.

6. `Create a portable agent memory in three steps`
   Use three steps: create a store, write files/rows/vectors/documents, then commit/branch/query/sync.

7. `Proof`
   Use real assets: CLI screenshots, API snippets, conformance/vector test status, CI badge, repo structure, and a demo recording. Avoid generic testimonials until real ones exist.

8. `Use cases`
   Lead with agent memory. Then local-first apps, configuration/infrastructure source of truth, and agent-authored logic.

9. Final CTA:

```text
Build with Uldren Loom
```

Pair it with docs, GitHub, or an install command.

## Build Strategy

Start with one landing page aimed at AI infrastructure builders, not a broad database audience. The dream outcome is not "content-addressed storage." It is: agents get durable, queryable, forkable memory without inventing a custom persistence layer.

First pass should prioritize the above-the-fold copy and visual. Then add proof from the repo: real commands, screenshots, architecture diagram, CI status, conformance story, and ABI/bindings surface.

After launch, test one variable at a time if traffic supports it: headline, hero visual, CTA wording, and the first proof block.
