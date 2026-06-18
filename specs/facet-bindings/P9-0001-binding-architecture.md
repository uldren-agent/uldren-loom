# P9-0001 - Binding Architecture (Wire Protocols, MCP, FUSE)

**Series:** P9 binding plan (normative-track sub-series; promoted per-decision as the owner reviews)
**Version:** 0.1.0-draft
**Status:** Draft - foundation doc for the per-facet binding specs
**Last updated:** 2026-06-18

This is the entry point for **Phase P9** of [`IMPLEMENTATION-PLAN.md`](../IMPLEMENTATION-PLAN.md) - "Wire
protocols, MCP server, FUSE." It exists because of the gap named in
[`IMPLEMENTATION-FIDELITY.md`](./IMPLEMENTATION-FIDELITY.md): every facet is an in-process Rust API today,
reachable on **no port and no mount**, so none of the reference software's connection methods (a 3306
MySQL socket, a 9200 REST API, a FUSE mount) yet apply.

This doc does **not** itself define per-facet bindings. It defines the *shared architecture* they all
follow, a per-facet **binding matrix**, the cross-cutting checklist each per-facet spec must satisfy, and
the resolved decisions or remaining questions that gate the per-facet drafts. The per-facet specs are the subsequent
`P9-00NN-<facet>-binding.md` documents (file plan in §6).

> **Status caveat.** This sub-series is **normative-track but Draft**, and is promoted *per decision*, not
> all at once: a section becomes binding only when the owner has reviewed and settled it, and every
> still-open choice (e.g. *what* `files` exposes over FUSE) stays an explicit **Open Question** until then.
> The normative wire-protocol contract it builds on is spec **0008** (REST/JSON-RPC/gRPC from one IDL); the
> protocol-adapter and consumer ideas are catalogued in spec **0013**. Everything below is grounded in
> already-written specs; where a fact is not yet verified (e.g. a facet's exact facade method list), it is
> marked *[transcribe from <spec>]* rather than guessed.

## 0. Decisions on this plan (owner, 2026-06-18)

These steer the per-facet drafts; the rationale for each is the correspondingly-numbered Open Question in
§7.

- **Adapter scope - full Tier-2 ambition (OQ1).** Every facet gets a Tier-2 foreign-protocol adapter
  wherever a credible reference protocol exists, on top of the Tier-1 native projection. The per-facet
  docs specify both tiers; Tier-2 remains capability-gated and is where reference-protocol *fidelity* is
  realized.
- **Layout - hybrid (OQ2).** One doc per facet (deltas only) plus a shared conventions doc
  (`P9-0002`) so the 0008 projection rules are written once.
- **Draft order - all facets, in batches, refocusing between (OQ3).** Batches are sequenced in §6; after
  each batch the work pauses for review/refocus before the next.
- **Status - normative-track, promoted per decision (OQ5).** Authored toward normative, but nothing is
  forced ahead of owner review; undecided design choices live in the relevant doc's remaining-questions section. MCP
  defaults read-only, writes require a capability token.

## 1. What already exists (so P9 does not reinvent it)

Verified from the specs:

- **Spec 0008 (normative)** projects the 0003 interface over **REST**, **JSON-RPC 2.0**, and **gRPC** from
  **one IDL** (`loom.proto` / `loom.openapi.yaml` / `loom.jsonrpc.json`), so the three never drift (0008
  §2). It fully details the **`fs`** projection (0008 §3.2), the **`vcs`** projection (0008 §3.3),
  caching/conditional requests via `Digest`-as-ETag (§3.4), **auth** (§5, `Authorization: Bearer` /
  mTLS), a normative **error mapping** table (§6), and the **Shuttle sync** transport binding (§7). It
  explicitly says the **`db`** (0011) and **`exec`** (0015) facades "project the same way."
- **Spec 0013 (exploratory)** catalogues the **protocol adapters** - REST/GraphQL (`axum`/`async-graphql`),
  an **S3 endpoint** (`s3s`), a **SQL wire** server (`pgwire` for Postgres, a profiled MySQL-wire
  implementation for MySQL),
  and an **MCP server** (`rmcp`) - and the **consumers**: a **FUSE mount** (`fuser` on Linux/macOS,
  `dokan-rust`/`winfsp` on Windows) and the **`loom` CLI/REPL** (which also launches `loom mcp`). A
  **git-remote adapter is permanently excluded** (ADR-0002, 0012).
- **Spec 0007** defines the binding/ABI layer; **0009 §6** defines the capability-token authorization model
  the wire auth projects; **0014** makes refs/objects/trees **workspace-scoped**. Spec 0008 now expresses
  that by serving one `.loom` store per listener and using canonical workspace UUID paths.

So P9's actual work is: (a) extend the **IDL** beyond `fs`/`vcs` to **every facet facade** (0011, 0016-
0024, 0033) so the three native projections + MCP are generated for them too; (b) build the **MCP server**
and **FUSE mount** consumers; and (c) decide which **foreign-protocol adapters** to promote from
exploratory to built.

## 2. The two-tier binding model

Every facet gets bindings in up to two tiers. Separating them is what lets the per-facet specs be written
without resolving the "how faithful to the reference protocol?" debate first.

**Tier 1 - native projection (the baseline; what makes a facet reachable at all).**
The facet's facade methods enter the **IDL** and are projected, with no per-facet protocol design, into:

- **REST** routes under the facet's resource root (extending the 0008 §3.1 resource model),
- **JSON-RPC 2.0** methods mirroring the IDL 1:1 (0008 §4.1),
- **gRPC** unary + streaming RPCs (0008 §4.2),
- **MCP** tools + resources (0013 §B/§C) - the AI-agent surface, launched by `loom mcp`.

All four reuse 0008's shared error mapping (§6), auth (§5), streaming, and sync transport (§7). Tier 1 is
the part that should become **normative** (it is just 0008 applied to more facades).

**Tier 2 - foreign-protocol adapter (optional; "the same `.loom` masquerades as the tool you already use").**
A "Loom Server" that speaks a *reference* protocol so unmodified existing clients attach - e.g. a
**pg-wire** endpoint so `psql`/JDBC query the `sql` facet, an **S3** endpoint for `cas`/`files`, an
**Elasticsearch-style REST** surface for `search`, a **FUSE mount** for `files`/`vcs`. This is the tier
where the "what a faithful binding would expose" column of the fidelity doc becomes real at the protocol
level. Per 0013 these are higher-effort, capability-gated, and currently exploratory.

The **MCP server** and **FUSE mount** are slightly special: MCP is a Tier-1 surface (generated from the
same IDL) but, like a Tier-2 adapter, is a separately-built server process; FUSE is a Tier-2 *consumer*
that only applies to the mountable workspace types (`files`/`vcs`, per 0014).

## 3. Per-facet binding matrix

Tier-1 native projection applies to **every** facet uniformly (IDL  to  REST/JSON-RPC/gRPC/MCP). The columns
below therefore focus on each facet's **REST resource root**, its **headline MCP tools**, the **Tier-2
foreign adapter** that would make it feel like its reference, the **reference connection** that adapter
targets, and **how much already exists in 0008**. Method enumerations are deferred to each per-facet spec
(*[transcribe from the cited facade spec]*).

| Facet | Facade spec | Tier-1 REST root (proposed) | Headline MCP tools | Tier-2 foreign adapter (reference port) | In 0008 today? |
| --- | --- | --- | --- | --- | --- |
| `files` | 0003 §4 | `/tree/{path}` | `fs.read/write/list/stat` | **FUSE mount**; optionally S3 (`s3s`) | **Yes** (0008 §3.2) |
| `vcs` | 0003 §5 | `/commits`, `/refs/{name}` | `vcs.commit/log/branch/merge/diff` | none - git remote **excluded** (0012) | **Yes** (0008 §3.3) |
| `cas` | 0024 | `/objects/{digest}` | `cas.put/get/has/delete/list` | **S3** object API (`s3s`) | Partial - objects in 0008 §3.1 |
| `sql` | 0011 | `/db/{db}/query` (+ `/tables`) | `sql.query` (read), `sql.exec` (token-gated) | **pg-wire** (5432, `pgwire`) / **MySQL** (3306, profiled MySQL-wire implementation) | Gestured (0008 §2 "`db` projects the same way") |
| `kv` | 0019 | `/kv/{name}/{key}` | `kv.get/put/delete/range/list_collections` | Redis/RESP, Memcache, etcd-style, Couchbase-KV presentations | No |
| `document` | 0020 | `/documents/{name}/{id}` | `document.get/put/delete/list/list_collections` | MongoDB/Couchbase-style document presentations | No |
| `columnar` | 0023 | `/columnar/{dataset}` (scan/select) | `columnar.scan/columns/rows/select/create/append` | **Arrow Flight** (gRPC) / Parquet export plus analytical presentations | No |
| `time-series` | 0022 | `/series/{name}` (range) | `timeseries.get/range/latest/put/list_collections` | line-protocol write endpoint (8086-style) | No |
| `queue` | 0021 | `/streams/{name}` (append/range) | `queue.get/range/len/append/consumer_read` | Kafka-style produce/consume | No |
| `ledger` | 0018 | `/ledger/{name}` (append/verify) | `ledger.append/get/head/len/verify/list_collections` | gRPC append/verify; signed checkpoints | No |
| `graph` | 0016 | `/graph/{name}` (nodes/edges/traversal) | `graph.neighbors/reachable/shortest_path` | Bolt/GQL or Gremlin - only if a standard grammar is adopted | No |
| `vector` | 0017 | `/vectors/{name}/search` (+ upsert) | `vector.search/upsert` | Qdrant-shaped REST/gRPC | No |
| `search` | 0033 | `/search/{name}/query` | `search.create/index/get/ids/query/remap` | **OpenSearch-style REST** (9200) with aggregations | No |

(`sync` rides every transport already, 0008 §7; `exec` projects per 0008 §2 but is exploratory, 0015.)

## 4. Cross-cutting checklist (every per-facet spec must answer these)

So the per-facet specs stay uniform, each `P9-00NN-<facet>-binding.md` MUST cover:

1. **IDL additions** - the facade methods (from the facet's spec) added to `loom.proto` / OpenAPI /
   JSON-RPC manifest (0008 §2), including streaming shapes (list/scan/range/log are streams).
2. **REST** - resource model, path/verb mapping, conditional requests/caching where `Digest` ETags apply
   (0008 §3.4), and pagination/streaming (`application/x-ndjson`).
3. **gRPC** - service + unary/server-streaming/client-streaming/bidi RPC choices (0008 §4.2).
4. **JSON-RPC** - 1:1 method names and the streaming mechanism (0008 §4.1).
5. **MCP** - tools + resources, and the **write-safety default** (read-only unless a capability token is
   presented; see 0013 RD5).
6. **Auth/authz** - token scopes mapped to this facet's workspaces/paths/keys (0008 §5; 0009 §6; phase
   P12), including existence-hiding posture (0008 §5 / Open Question 4).
7. **Error mapping** - reuse 0008 §6; list any facet-specific codes (e.g. `DATASET_NOT_FOUND` for
   `columnar` 0023; a `verify` mismatch code for `ledger` 0018).
8. **Workspace addressing** - how the workspace appears in the URL/RPC. The resolved hosted REST shape is
   `/v1/workspaces/{workspace_id}/...` under one `.loom` store selected by the listener.
9. **Platform parity** - which surface degrades on `wasm32` (e.g. `search` indexing is native-only, 0032/
   0033; HNSW acceleration native-only, 0017).
10. **Concurrency** - single-writer/multi-process discipline (0005 §6.4; `1000-Deferred.md` §0013
    Concurrent Protocol Adapters) when this
    facet is served alongside others on one `.loom`.
11. **Tier-2 adapter (if any)** - the foreign protocol, the mapping from reference concepts to Loom
    concepts, the fidelity ceiling (what the reference does that Loom cannot), and the capability gate.

## 5. Dependencies and sequencing

- **Upstream:** P9 needs the L2 facades to exist (P5-P8) and **P12 (principals/access control)** for the
  write-safety/token model. Tier-1 for `fs`/`vcs` can start immediately (their facades and the 0008
  projection are the most complete); the other facets' Tier-1 follows their facade builds.
- **0013's suggested order** (advisory): CLI + `loom mcp` first, then FUSE, then adapters
  (REST/GraphQL, S3, SQL-wire). That implies **MCP is the cheapest first win** (it is a thin `rmcp` adapter over the same
  interface) and a good first per-facet target across all built facets.
- **E2E-encrypted sync (0031)** rides the sync transport (0008 §7), not a per-facet binding.

## 6. File plan and batch sequence

Decided layout (hybrid) and numbering. Each per-facet doc states only its *deltas* over `P9-0002` and
covers **both tiers** (full Tier-2 ambition). Batches pause for review/refocus between them.

| Doc | Scope | Batch |
| --- | --- | --- |
| `P9-0001-binding-architecture.md` | this foundation doc | 1 |
| `P9-0002-projection-conventions.md` | shared IDL-extension + REST/JSON-RPC/gRPC/MCP/auth/error conventions + the per-facet doc template | 1 |
| `P9-0003-files-binding.md` | `files` (incl. FUSE + S3 Tier-2) | 1 |
| `P9-0004-vcs-binding.md` | `vcs` (Tier-2 n/a - git excluded, 0012) | 1 |
| `P9-0005-cas-binding.md` | `cas` (incl. S3 Tier-2) | 1 |
| `P9-0006-sql-binding.md` | `sql` (incl. pg-wire / MySQL-wire Tier-2) | 2 |
| `P9-0007-kv-binding.md` | `kv` (incl. Redis/RESP, Memcache, etcd-style, and Couchbase-KV presentations) | 2 |
| `P9-0008-document-binding.md` | `document` | 2 |
| `P9-0009-columnar-binding.md` | `columnar` (incl. Arrow Flight / Parquet Tier-2) | 3 |
| `P9-0010-time-series-binding.md` | `time-series` (incl. line-protocol Tier-2) | 3 |
| `P9-0011-queue-binding.md` | `queue` (incl. Kafka-style Tier-2) | 3 |
| `P9-0012-vector-binding.md` | `vector` (incl. Qdrant-shaped Tier-2) | 4 |
| `P9-0013-search-binding.md` | `search` (incl. OpenSearch-style Tier-2, source-backed local facade) | 4 |
| `P9-0014-graph-binding.md` | `graph` (incl. Bolt/GQL/Gremlin Tier-2, grammar-dependent, source-backed local traversal) | 4 |
| `P9-0015-ledger-binding.md` | `ledger` (incl. gRPC append/verify Tier-2, source-backed local facade) | 4 |
| `P9-0016-mcp-server.md` | the cross-facet `loom mcp` server (`rmcp`) | 5 |
| `P9-0017-fuse-mount.md` | the cross-facet FUSE consumer (resolves the files-doc FUSE Open Question) | 5 |

Batch 1 establishes the template on the most-complete, best-grounded facets. The two special
consumers, FUSE and MCP, are surfaced here and fully specified in Batch 5. The
formerly designed-only facets (`search`/`graph`/`ledger`, Batch 4) now have source-backed local facades,
but their hosted protocol and Tier-2 presentation surfaces are still target work.

## 7. Resolved Decisions And Remaining Questions

Format: **Context, Example, Options, Recommendation.** **OQ1, OQ2, OQ3, and OQ5 are now resolved** by
the owner (§0); their entries are kept below as the rationale record, annotated with the chosen option.
**All open questions are now resolved.** OQ6 (FUSE exposure) is settled (owner, 2026-06-18) and specified
in `P9-0017-fuse-mount.md`; only minor residual sub-questions remain there (symlink ops, xattrs, gated on
findings F11), not the mode model.

**Resolved:** OQ1 to full Tier-2 ambition; OQ2 to hybrid layout; OQ3 to all facets in batches; OQ4 to
canonical UUID workspace paths; OQ5 to normative-track, promoted per decision, with MCP read-only by
default; OQ6 to **both modes, chosen at mount** (commit = read-only; tip = read-write, or read-only by
mount option).

### OQ1 - Tier-2 foreign-protocol ambition (the big one)

- **Context.** §2 splits bindings into Tier-1 (native IDL projection, makes a facet reachable) and Tier-2
  (foreign adapters that emulate the reference so existing tools attach). 0013 lists S3/SQL-wire/MCP as
  attractive but exploratory. How far P9 commits to Tier-2 changes the size and shape of every per-facet
  spec.
- **Example.** For `sql`, Tier-1 alone means agents/tools call `/db/{db}/query` or the `sql.query` MCP
  tool. Tier-2 means standing up a `pgwire` server so `psql "host=... dbname=..."` and JDBC apps connect to a
  `.loom` unmodified - much higher value, much higher effort, and a new fidelity surface to test.
- **Options.** (a) **P9 = Tier-1 (native projection) + MCP + FUSE only**; defer all other foreign adapters
  to a later phase. (b) **Tier-1 + MCP + FUSE now, plus a short, named Tier-2 shortlist** (most likely
  pg-wire for `sql` and S3 for `cas`/`files`) promoted as deliverables. (c) **Full Tier-2 ambition** -
  every facet gets a foreign adapter where one exists.
- **Recommendation.** (b). Tier-1 + MCP + FUSE is the spine that makes everything reachable and AI-usable;
  adding only pg-wire and S3 captures the highest "it just works with existing tools" value (0013 calls
  these out specifically) without committing to a long tail of bespoke adapters whose references have no
  single standard (vector, search, queue). The per-facet specs would then write Tier-1 fully and mark
  Tier-2 as "designed, gated" except for `sql`/`cas`/`files`.
- **Resolved - (c) full Tier-2 ambition** (owner, 2026-06-18): every facet gets a Tier-2 adapter where a
  credible reference protocol exists. Each per-facet doc therefore specifies both tiers, with Tier-2
  capability-gated; where a facet's reference has no single standard (vector/search/queue), the doc picks
  the most representative reference and says so.

### OQ2 - Document organization and numbering

- **Context.** You asked for a `P9-00xx-...` prefix and "per-facet binding specs." There are 13 facets
  plus shared concerns (IDL conventions, MCP, FUSE, adapters), which can be sliced per-facet, per-surface,
  or a hybrid.
- **Example.** A per-surface layout would be `P9-REST.md`, `P9-gRPC.md`, `P9-MCP.md`, `P9-FUSE.md`; a
  per-facet layout is `P9-0003-sql-binding.md`, `P9-0004-vector-binding.md`, ...; the hybrid (this doc's
  §6) is per-facet docs **plus** a shared `P9-0002-conventions` doc so the 0008 boilerplate is written
  once.
- **Options.** (a) **Hybrid** (per-facet docs + one shared conventions doc) - §6. (b) **Pure per-facet**
  (each facet doc repeats the conventions). (c) **Per-surface** (one doc per protocol, all facets inside).
- **Recommendation.** (a) hybrid. It matches your "per-facet" request, keeps each facet doc short (deltas
  only), and avoids restating 0008 thirteen times - which would drift.

### OQ3 - Which facets to draft first (scope of the next step)

- **Context.** The built facets (`cas`, `files`, `vcs`, `kv`, `document`, `sql`, `columnar`,
  `time-series`, `queue`, `vector`) could have grounded bindings when this decision was first written.
  Graph, ledger, and search now also have source-backed local facades, while their hosted protocol and
  Tier-2 presentation work remains target work.
- **Example.** A `P9-vector-binding.md` can cite real method names from `loom-core::vector`; a
  `P9-graph-binding.md` can now cite the source-backed traversal facade while still treating
  Bolt/openCypher/GQL/Gremlin as target presentations.
- **Options.** (a) **Built facets first** (the 10), designed facets deferred until they are built. (b)
  **A vertical slice first** - `files`+`vcs` (most complete, exercise FUSE + the 0008 projection) then
  `sql`+`vector` (exercise Tier-2 + MCP), then the rest. (c) **All 13 at once**, marking designed ones
  provisional.
- **Recommendation.** (b) the vertical slice. It validates the per-facet template against the richest,
  most-complete facets (and both special consumers, FUSE and MCP) before mass-producing the rest, which is
  the lowest-rework path.
- **Resolved - all facets, in batches** (owner, 2026-06-18): a refinement of (c), cover every facet, but
  sequenced into the §6 batches with a deliberate review/refocus between each, rather than all at once.
  Batch 1 (this batch) is the `files`/`vcs`/`cas` core, which doubles as the template-validation slice.

### OQ4 - Workspace addressing in URLs/RPCs

- **Context.** 0014 makes refs/objects/trees workspace-scoped, so REST paths need workspace identity in
  the resource path.
- **Example.** A `sql` facet and a `kv` facet both live inside one workspace. A CDN must cache scoped
  resources under distinct Loom and workspace UUIDs.
- **Options.** (a) canonical UUID path segment; (b) a `?ns=` query parameter; (c) an
  `X-Loom-Workspace` header.
- **Recommendation.** (a) path segment. It makes scope part of resource identity and is unambiguous and
  per-workspace cacheable.
- **Resolved - canonical workspace UUID path.** Every facet's REST root is
  `/v1/workspaces/{workspace_id}/{facet_root}` inside the one `.loom` store selected by the hosted
  listener. P9-0002 sections 2 and 8 and 0008 section 3.1 now state this as settled.

### OQ5 - MCP write-safety default (aligned with 0013 RD5) and promotion path

- **Context.** `loom mcp` exposes facets to AI agents; its default authority sets the blast radius, and
  this whole sub-series is informative until promoted via the 0010 process.
- **Example.** An agent given an MCP connection could advance `branch/main` or drop a table mid-loop if
  writes are ambient and untokened.
- **Options (write default).** (a) full read+write by default; (b) **read-only by default, writes require
  an explicit capability token** (0009 §6); (c) read-only with no write surface. **Options (promotion).**
  (i) keep P9 docs informative, promote Tier-1 to normative via a single 0010 RFC once facades land; (ii)
  author them as normative from the start.
- **Recommendation.** Write default **(b)** - safe-by-default, versioned writes only on a deliberate token,
  matching 0013 RD5. Promotion **(i)** - keep this sub-series informative now and promote **Tier-1** (the
  IDL projection) to normative via one 0010 RFC when P5-P8 facades exist; Tier-2 adapters stay
  optional/capability-gated indefinitely.
- **Resolved - normative-track, per-decision** (owner, 2026-06-18): author toward normative, but promote
  only what the owner has reviewed; track unresolved choices in the owning docs. MCP write default is
  **(b)** read-only + token-gated writes.

### OQ6 - What does FUSE actually expose? (resolved)

- **Context.** 0013 §C says only `files`/`vcs` workspaces are mountable, and a mount may be "the live tip,
  read-write" or "a commit id, read-only." The owner has not decided which filesystem semantics to expose,
  so this cannot be declared normative yet. It gates both `P9-0003-files-binding.md` and
  `P9-0017-fuse-mount.md`.
- **Example.** A read-write live-tip mount lets unmodified apps (`vim`, `cp`) edit a `files` workspace, but
  raises "when does an edit become a commit?" (every `close()`? an explicit `loom commit`? a debounce?). A
  read-only commit mount sidesteps that entirely but only serves inspection/checkout.
- **Options.** (a) **read-only commit mount first** (simplest, safe), add read-write later; (b)
  **read-write live-tip mount** with an explicit-commit model (edits stage into the working tree; nothing
  commits until `loom commit`); (c) **both modes**, selected at mount time - plus sub-decisions: symlink
  handling (the `Symlink` EntryKind exists but has no ops), POSIX mode/owner mapping, case sensitivity, and
  whether `xattrs`/sparse files are supported.
- **Resolved - both modes, chosen at mount** (owner, 2026-06-18; a long-standing decision): **mounting a
  commit is read-only**; **mounting the tip is read-write by default, or read-only via a mount option**.
  Read-write edits the working tree live with **explicit** commit (`loom commit`); mountable types stay
  `files`/`vcs` only (0014). Residual sub-questions (symlink ops, xattrs) are gated on findings F11 and
  specified in `P9-0017-fuse-mount.md`.
