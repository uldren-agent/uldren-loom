# Loom Specification - Index

**Spec series:** Loom Core Specification (LCS)
**Version:** 0.1.0-draft
**Status:** Draft
**Last updated:** 2026-06-14

This is the authoritative index for the Loom specification. Read documents in order on first
pass; thereafter each document is self-contained and cross-references the others by number.

## Documents

| #    | Title                        | Normative?                       | Summary                                                                                                                       |
| ---- | ---------------------------- | -------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| 0000 | Index (this file)            | Informative                      | Conventions, reading order, terminology pointers.                                                                             |
| 0001 | Overview & Architecture      | Mixed                            | Vision, goals/non-goals, layered architecture, glossary.                                                                      |
| 0002 | Data Model                   | **Normative**                    | Objects, digests, chunking, trees, prolly sharding, commits, refs, the DAG.                                                   |
| 0003 | Core Interface               | **Normative**                    | The language-neutral API: types, filesystem ops, VCS ops, sync ops, error taxonomy, IDL.                                      |
| 0003a | File Facade Extensions      | Mixed                            | Source-backed append, offset I/O, truncation, file handles, and symlink create/read-link; symlink following and remaining binding/projection breadth stay target. |
| 0003b | Workspace History Extensions | Mixed                           | Source-backed merge lifecycle, staging, tags, restore, replay, and squash; persisted replay sequencer, interactive rebase, and richer per-facet mergers stay target. |
| 0003c | Filesystem Projection        | Source-backed                    | Portable projection layer (`loom-vfs`) plus committed FUSE and NFSv3 backends projecting a workspace working tree as a real filesystem. |
| 0003d | Cross-Facet Commit Diff      | Draft target                     | Uniform commit-to-commit structural diff grouped by facet collection and natural unit; target contract split out of 0003b. |
| 0004 | Providers                    | **Normative**                    | Source-backed provider model: lean `ObjectStore`, `FileStore`, `BackingIo`, sync relation, and capability boundary.            |
| 0004a | Hosted Provider Facade      | Draft target                     | Hosted, remote, or generated Provider facade split out from the current source-backed provider gate.                          |
| 0005 | Single-File Container Format | **Normative**                    | Source-backed byte-level page-engine format for a self-contained `.loom` file.                                                 |
| 0005a | Storage Format Extensions   | Draft target                     | Pack-split, standalone ref/reflog regions, pins, generated schema, embedded capabilities, and remote-provider storage regions. |
| 0006 | Synchronization              | **Normative**                    | Source-backed direct workspace clone, fast-forward branch push, and v4 offline bundles.                                       |
| 0006a | Live Sync and Remotes       | Draft target                     | Hosted remotes, live fetch/pull/push, remote-tracking refs, resumability, incremental bundles, and sync protocols.            |
| 0007 | Language Bindings            | Mixed                            | Source-backed C ABI and current hand-written language binding contracts.                                                       |
| 0007a | Binding Generation, Packaging, and Certification | Draft target     | Generated bindings, package distribution, runtime certification, ABI drift checks, and cross-binding interop.                  |
| 0008 | Wire Protocols               | **Normative**                    | REST, JSON-RPC 2.0, gRPC projections; IDL as the single source of truth.                                                      |
| 0009 | Security & Capabilities      | Mixed                            | Source-backed at-rest encryption, compression, integrity, key-wrap behavior, threat model, and target security umbrella.       |
| 0009a | Governance and Authenticity | Draft target                     | Commit/ref authenticity, transparency, audit, retention, legal hold, redaction, deletion proofs, and policy-controlled GC.     |
| 0010 | Conformance & Versioning     | **Normative**                    | Source-backed conformance boundary: canonical vectors, executable behavior runners, binding evidence tiers, and versioning.    |
| 0010a | Conformance Reporting and Certification | Draft target          | Machine-readable reports, hosted protocol certification, generated capability reports, and full binding certification.         |
| 0011 | Tabular & Query Layer        | Capability (`sql`)                 | Source-backed versioned SQL tables stored as prolly trees over the object model; GlueSQL frontend; current ABI/binding projections. |
| 0011a | SQL Facade and Conformance  | Draft target                       | Generated SQL facade, hosted protocols, full historical query, schema-change diff, SQL error taxonomy, and SQL certification. |
| 0012 | Interchange                  | Capability (`import-*`/`export-*`) | Moving data in/out of Loom: import git repo / DB table; checkout a commit to disk; commit from a directory.                   |
| 0013 | Extended Capabilities        | Catalog                          | Source-backed capability and facet taxonomy; implementation gates live in owning specs and the dependency table. |
| 0014 | Workspaces                   | Mixed (core data-model addition) | Named buckets with stable ids, facets, one workspace tree, per-workspace refs, sync identity, and lifecycle rules.             |
| 0014a | Facet Interoperability      | Draft target                     | File-style projection policy for `.loom/facets/<facet>/...` paths and their interaction with typed facet APIs.                |
| 0015 | Execution & Logic            | Partial (capability `exec`)  | Programmable compute over the store: deterministic, metered, sandboxed state transitions; layered engines; programs as content-addressed objects. Source-backed `engine=wasm` `exec` facade (gated/direct/batched), multi-facet WASM host ABI, and guard/derivation/statechart/workflow logic layers with executable conformance; first-class Program lifecycle surfaces and persistent `engine=cel` interpreted programs remain target; reactive firing deferred to 0029/0030/0035/0041. |
| 0016 | Graph Layer                  | Capability (`graph`)               | Property-graph facet: versioned nodes/edges over prolly trees; the `graph` facade; query via an embedded engine.                                  |
| 0017 | Vector Layer                 | Capability (`vector`)              | Embeddings facet: versioned vectors over prolly trees, a derived ANN index; the `vector` facade; nearest-neighbor search.                          |
| 0018 | Ledger Layer                 | Capability (`ledger`)              | Append-only hash-chained verifiable log over the object model; the `ledger` facade; signing/transparency/retention ties (0009).                    |
| 0019 | Key-Value Layer              | Capability (`kv`)                  | Source-backed workspace-scoped KV facade over the current canonical-map storage; structured prolly-map storage and per-key merge remain target. |
| 0019a | Ephemeral Cache Tier        | Draft (`kv` tier modifier)         | Memcache-shaped ephemeral, non-versioned, non-synced KV tier: TTL/idle-TTL, LRU/LFU eviction, capacity bounds, optional write-through/-behind to a versioned map; volatile and empty on restart/rebuild by design. |
| 0020 | Document Layer               | Capability (`document`)            | Versioned JSON/CBOR documents by id with optional indexes; the `document` facade.                                                                  |
| 0021 | Append-Log Layer             | Capability (`queue`)               | Append-only ordered streams / FIFO queues over prolly trees; the `queue` facade.                                                                   |
| 0021a | Structured Append-Log Storage | Implemented storage sub-spec      | Canonical stream-root, sequence-keyed prolly entry map, payload reachability, and append/range algorithms for scalable queue storage.              |
| 0021b | Queue Consumer Offsets       | Implemented sub-spec              | Authority-local operational consumer progress for queue streams: explicit read/advance, at-least-once delivery, no ordinary sync transfer.         |
| 0021c | Kafka Compatibility Surface  | Draft target                      | First-class `kafka` served surface over queue collections: workspace-scoped listener, topics, partitions, offsets, consumer groups, producer epochs, transactions through 0036a, and honest single-node capability reporting. |
| 0022 | Time-Series Layer            | Capability (`time-series`)         | Points by (series, timestamp) with range queries; rollups as derived views; the `time-series` facade.                                              |
| 0023 | Columnar Layer               | Capability (`columnar`)            | Read-optimized Arrow/Parquet segments as versioned blobs; the `columnar` facade (StateAccess default, Polars native-gated; ADR-0008).               |
| 0024 | Content-Addressed Store      | Capability (`cas`)                 | Put/get immutable blobs by digest; the `cas` facade over the object store.                                                                          |
| 0025 | Behavioral Conformance       | Normative (behavior)               | Source-backed behavior runners and scenario inventory for the current conformance boundary.                                   |
| 0025a | Behavior Runner Expansion   | Draft target                       | Future files, workspace-history, SQL, data-facet, provider, binding, and protocol behavior runners split out from 0025.       |
| 0026 | Principals & Identity        | Draft (capability `identity`)      | What a principal is (user/service/root); the internal principal store; pluggable authentication (password + Ed25519 first, certificate deferred); the owner-vs-authenticated mode and root bootstrap. |
| 0027 | Access Control               | Draft (capability `acl`)           | The policy enforcement point and the stored grant model with deny-precedence; workspace-level read/write/advance grants; roles; cross-workspace read rule; `PERMISSION_DENIED`. |
| 0027a | Authorization Surface Matrix | Draft target                       | Operation-by-operation authorization matrix for CLI, IDL, C ABI, bindings, hosted protocols, projections, and source-backed facades. |
| 0028 | Fine-grained Access Control  | Draft (capability `acl-fine`)      | Grants below the workspace (ref-glob, path/scope prefix, facet, `merge`/`exec` rights); matching and deny-precedence. Extends 0027 without changing its evaluation rule. |
| 0028a | ACL Policy Extensions       | Draft target                       | Row/column predicates, conditional CEL policy, and attenuable sessions split out from the main 0028 dependency gate. |
| 0029 | Events & Triggers            | Draft (capability `trigger`)       | Reactive execution: a keeper runs a stored program on a cron schedule (croner) or on change; the `trigger` workspace, event spine, and run-as identity. |
| 0030 | Observability                | Draft (capability `watch`)         | Observing state changes across facets as an ordered, resumable feed with source-backed pull and hosted watch stream/materialization. Widens `watch` (0009 §8). |
| 0031 | End-to-End Encrypted Sync    | Draft (capability `e2e-sync`)      | A client seals content so an untrusted host stores, relays, and syncs a Loom without reading it; digests over plaintext keep addressing intact. Distinct from access control. |
| 0031a | Blind Replica Sync Labels and Protocol | Draft target              | Opaque remote-label derivation, blind have/want negotiation, decrypt-then-verify pull, and conformance split out from the 0031 umbrella. |
| 0032 | Native / Web Platform Parity | Normative (parity classes)         | The single tracker of what behaves identically vs differently on native and `wasm32`, with the divergence playbook (avoid non-portable deps / dual-compile byte-identical / graceful degradation) and a per-mismatch section for each difference. |
| 0033 | Search Layer                 | Capability (`search`)              | Full-text search facet over versioned documents: BM25 ranking, boolean/term/phrase/range queries, facets, highlighting, aggregations; backed by `tantivy`; index derived/rebuildable; the `search` facade.                                          |
| 0034 | Key Sources & Unlock Providers | Draft (mixed: model normative)   | How Loom acquires/derives the key that unwraps an encrypted store's DEK across every platform: the passphrase-KEK vs external-KEK model, the `--key-source` selector (prompt/file/fd/keystore/secure-enclave/passkey/kms/hsm), and a per-platform hook-point map (Keychain+Secure Enclave, DPAPI/Hello/TPM, Secret Service/keyring/TPM2, Android Keystore/StrongBox, WebAuthn PRF passkeys incl. password managers, AWS KMS/Nitro, Azure Key Vault, PKCS#11). Env vars are never the source. |
| 0035 | Durable Delivery             | Draft (capability `delivery`)       | Generic durable outbox, ack, reconnect, and replay semantics for WebSocket, SSE, MCP, watch, exec, trigger, and hosted streaming surfaces. |
| 0036 | Locking & Coordination       | Draft (capability `lock`)           | Leased, reentrant, fenced exclusive/shared/semaphore locks scoped per workspace and per coordinator; optimistic CAS preferred; fencing tokens and lock state kept out of the versioned tree and never synced; sync-destination and internal coordination use sites. |
| 0036a | Coordination Substrate      | Partial                           | `loom-coordination` crate boundary for single-node authority state now and future cluster backends: source-backed authority ids, epochs, fencing, sequencing, producer epochs, group generations, transaction records, and Kafka-compatible coordination without making Kafka own the substrate. |
| 0037 | Calendar Layer               | Draft (capability `calendar`)       | The `calendar` facet (events + task lists): structured records as the source of truth, with iCalendar (RFC 5545), mounted `.ics` files, and hosted CalDAV (RFC 4791) projected on demand; local core/ABI/bindings/VFS/conformance are source-backed, while hosted/auth/change-feed work remains target. |
| 0038 | Contacts Layer               | Draft (capability `contacts`)       | The `contacts` facet (vCard, RFC 6350/6352 CardDAV): structured contact records as the source of truth, with vCard/`.vcf`/CardDAV projected on demand; local core/ABI/bindings/VFS/conformance are source-backed, while hosted/auth/lifecycle work remains target. |
| 0039 | Mail Layer                   | Draft (capability `mail`)           | The `mail` facet (email for the agent): immutable RFC 5322 message bodies in CAS plus structured index and separate versioned flags; local core/ABI/bindings/VFS/conformance are source-backed, while MCP, hosted IMAP, filters, flag-retention policy, and ACL-aware serving remain target. |
| 0040 | Versioned GraphRAG           | Draft (capability `graphrag`)       | GraphRAG composed over graph (0016) + vector (0017) + columnar/Parquet (0023) on the versioned engine (not a new storage facet): co-located node embeddings, Parquet bulk ingestion, and the versioned-native differentiators - auditable/reproducible, temporal/time-travel, incremental (via 0003d structural diff), branchable, and lazy retrieval. |
| 0041 | Facet Lifecycle Hooks        | Draft (capability `hooks`)          | A uniform cross-facet hook mechanism: programs (0015) register for facet lifecycle events (before/after create/update/delete plus facet-specific events like mail `on_message_ingested`) via the keeper (0029), run_as a principal (0026/0027); filters/reminders/de-dup are programs. |
| 0042 | Collections                  | Draft (foundational; invariant A7)  | The uniform intra-facet container: units live in named, possibly-nested **collections** (one level for kv/document/vector/ts/queue/ledger; `database > table` for sql; nested folders for files; `principal > collection` for calendar/contacts/mail). One concept serving grouping, ACL scope (0027), projection, and diff roll-up (0003d); address is `facet.<collection-path>.<unit>`. Not a workspace. |
| 0045 | DataFrame Layer              | Draft (capability `dataframe`)      | First-class dataframe transformation facet over CSV, JSON/NDJSON, Arrow, Parquet, SQL, files/CAS, and columnar inputs; Polars is the default native execution layer, while materialized durable analytical outputs belong in `columnar`. |
| 0046 | Metrics Layer                | Source-backed (capability `metrics`) | Native metrics descriptor, observation, bounded query, IDL, C ABI, CLI, MCP, local and remote clients, bindings, and conformance; hosted compatibility facades remain target. |
| 0047 | Logs Layer                   | Source-backed (capability `logs`)    | Native log records, bounded query, IDL, C ABI, CLI, MCP, local and remote clients, bindings, and conformance; hosted compatibility facades remain target. |
| 0048 | Traces Layer                 | Source-backed (capability `traces`)  | Native span records, trace span listing, bounded query, IDL, C ABI, CLI, MCP, local and remote clients, bindings, and conformance; hosted compatibility facades remain target. |
| 0050 | Embedding Providers          | Partial capability (`providers.embedding`) | Core `EmbeddingProvider`/`Embeddings` seam is source-backed; 0062 source-backs curated Candle CPU text embedding activation and records optional runtime/provider contracts for hosted API, Ollama, MLX, Core ML, CUDA, and llama.cpp paths. Per-workspace model identity plus the determinism spine remain local-only and never synced. LEANN "multi-provider". |
| 0051 | LLM / Chat Providers         | Capability (`providers.llm`)       | Chat/completion provider abstraction split out of 0050: OpenAI-compatible HTTP (primary) + optional in-process candle LLM; token streaming, tool calls, and backpressure over 0008; completions are non-deterministic and never a sync source of truth. |
| 0052 | Certificate Bundles          | Draft target (`certificate-bundles`) | Store-global certificate bundles copied into the `.loom` file for portable hosted TLS, plus `loom certificate` management, safe audit output, computed reference guards, and daemon restart validation. |
| 0060 | FIPS Distribution and Compliance | Mixed capability (`fips-distribution`) | Dual standard/FIPS release channels, runtime FIPS policy, FIPS-required hosted-listener rejection, certificate trust, compliance artifacts, support boundary, and default-profile to FIPS-profile migration. |
| 0061 | Operation Substrate          | Draft target (`substrate`)          | Shared operation envelope, blind-capable sequencer, durable cursors, order tokens, conflict records, annotations and shared facilities, entity versioning, materialized views, predicate trees, and cross-facet search/changes/refs/transact tools; the Studio profiles consume it. |
| 0062 | Inference Model Downloads    | Complete scoped source-backed (`inference-downloads`) | `loom-inference` crate split, `Llm` and `TextEmbedding` provider contracts, shared Hugging Face cache downloads, live cache discovery, `loom inference` model and instance CLI, doctor hardware/model checks, curated model catalog, Candle CPU LLM/text-embedding activation, runtime adapter contracts, vector text integration, and full workspace gate evidence. |
| 0063 | Decisions App and Structured Decision Elicitation | Draft (first slice source-backed; `mcp-apps`) | Structured decision questions (Question/Context/Examples/Options/Recommendation, unbounded count) rendered as the internal Decisions MCP App; `ask_questions`/`ask_answers` tools with two-phase blocking plus the app-only `ask_record` write-back, Document-facet storage in `loom.ask`, per-question skip and whole-ask abort, radio/checkbox/text shapes, and per-ask instance URIs so concurrent asks render as separate app instances. |
| 0064 | Unified Search / Discovery   | Draft target (`unified-search`)    | Composition (like 0040, not a facet) over 0033 (lexical/Tantivy) + 0017 (vector/semantic) + 0040 (GraphRAG) behind the 0061 §10 `substrate_search` / whole-Loom `loom search` interface: the engine ladder (scan→lexical→semantic→hybrid→graph), RRF rank-based fusion with `match_via` provenance and deterministic tie-breaks, per-rung `as_of`/`history`, the embedding pipeline (bodies+append-only, async keyed index-worker, keyed-only reconciled with 0061 §3), permission-scoped pre-filter fusion, the Studio decoupling request/response signature, and the shared-index Tier-2 edge-port contract (one index, two faces). |
| 0065 | Admin Interface              | Draft, deferred target (`admin`)   | One administrative control plane for policy profiles, retention and compaction controls, redacted audit reads, hook and trigger administration, hosted listener posture, capability evidence, conformance reports, and reference-client certification records. |
| 0066 | Network Access               | Draft target (`network-access`)    | Reusable listener admission policy for Loom-owned TCP ports, including ordered allow/deny rules, CIDR matching, trusted proxy handling, mTLS criteria, served-listener integration, and audit behavior. |
| 0067 | Remote Loom Protocol         | Draft target (`remote-loom`)       | Loom-native remote source protocol, locator grammar, alias config, endpoint discovery, canonical CBOR call envelopes, streams, auth/TLS, sessions, locks, MCP remote support, binding policy, and conformance requirements. |
| 0067c | Remote Loom Client Parity Report | Informative (companion to 0067)  | Authoritative, generated per-method map of every current `idl/loom.idl` method (41 interfaces / 353 methods) to its client-parity status; backs section 12 of 0067 and is kept in sync by `uldren-loom-remote-codegen`. |
| 0068 | Multimodal Vector Sources    | Draft target (`multimodal-vector-sources`) | Non-text source pipelines for PDFs, images, audio, video, and multimodal bundles before they become ordinary vectors in 0017; owns source unit identity, extraction profiles, provenance, modality metadata, and multimodal embedding provider contracts. |
| 0070 | Release Maturity             | Informative                       | Alpha, Beta, Release Candidate, and Stable definitions, promotion gates, evidence requirements, and SemVer pre-release progression. |

Exploratory landscape notes (`EXECUTION-LOGIC-LANDSCAPE.md`, `EVENTS-TRIGGERS-LANDSCAPE.md`,
`PRINCIPALS-AND-ACCESS-CONTROL-LANDSCAPE.md`, `AI-CAPABILITIES-LANDSCAPE.md`) are research inputs that
feed the numbered specs and are not themselves normative.

`1000-Deferred.md` is the non-normative deferred-work ledger for ideas removed from the active
implementation gate. A deferred item returns to the main spec series only through a focused subspec,
source-backed implementation plan, and conformance path.

`platform/PLATFORM.md` is the informative product architecture map for `loom-cli`, `loom-server`, and
`loom-cloud`. The `platform/` folder also contains product deployment notes such as
`platform/inflow-cloud.md`. These documents record deployment relationships, trust boundaries, and
backend architecture options, while the numbered specs remain authoritative for protocol, encryption,
sync, and conformance behavior.

`telemetry-public-surface-inventory.md` is the source-backed inventory for the promoted metrics,
logs, and traces public surfaces across core, CLI, MCP, IDL, C ABI/header, bindings, remote clients,
conformance, and explicit non-goals.

`CONFLICT-RESOLUTION-MATRIX.md` is the **cross-cutting** analysis of concurrent-write / sync-collision
resolution across every data layer (the "two peers wrote the same named target, one must win" problem).
It records the resolved default policy: sync transfers objects but does not silently resolve divergent
branch/ref state. Each data-layer spec defers its same-element collision behavior to that matrix unless
it defines a stricter explicit merge policy.

ADR files live in `adr/` and use the `adr-NNNN-` filename prefix so they never collide numerically
with the spec series above.

| ADR      | Title                 | Status   | Summary                                                                                         |
| -------- | --------------------- | -------- | ----------------------------------------------------------------------------------------------- |
| adr-0001 | Reference Language    | Accepted | Rust core with thin language bindings (Node/JVM-FFM/C++/WASM); no pure-TS impl.                 |
| adr-0002 | Drop LocalShim        | Accepted | Remove the LocalShim backend; re-home "in/out" movement to Interchange (0012).                  |
| adr-0003 | Tabular / SQL Overlay | Accepted | Adopt versioned SQL via prolly-tree tables + embedded GlueSQL (analytical engine superseded by ADR-0008). |
| adr-0004 | Licensing & branding  | Accepted | Brand "Uldren Loom"; BSL 1.1 to Apache-2.0 with a competing-use AUG; bindings BSL; CLA required. |
| adr-0005 | Access-control model   | Accepted | Keys-to-principals-to-roles; in-engine stored grants with deny-precedence; owner-vs-authenticated mode; one PEP; authn/authz/encryption separated (0026/0027/0028). |
| adr-0006 | Reactive layer (keeper)| Accepted | Shared core keeper logic driven by hosts; time as a seeded input; Loom is the durable backend with no external job queue; croner; `run_as` is a dynamic principal (0029/0030).  |
| adr-0007 | Whole-Loom encryption + e2e-sync topologies | Accepted | Encryption at rest is whole-Loom (chosen at creation, suite-agile, random keying); `e2e-sync` is a sync topology decided by who holds the key (opaque / keyed / selective-mount), distinct from authz (revises the earlier per-workspace framing; 0009 §3, 0031). |
| adr-0008 | Browser-first engines  | Accepted | OLAP = Polars (native-gated); vector = `hnsw_rs` behind a pure-Rust exact-search default; no DataFusion/usearch (0011/0017/0023).                   |

## Bindings (facet + control-plane) and companion analyses

The binding plans (exposing capabilities over the wire / MCP / FUSE) and the reference-landscape and
fidelity analyses live in three folders: **[`facet-bindings/`](./facet-bindings/)** (the data facets, phase
P9), **[`control-plane-bindings/`](./control-plane-bindings/)** (`exec`/`watch`/`trigger`/`identity`/`acl`;
phases P11-P13), and **[`ai/`](./ai/)** (the embedding/LLM providers; 0050/0051). Start at each folder's
index. These are informative/normative-track **Draft** (promoted per decision), not part of the numbered
LCS series.

| Document | What's in it |
| -------- | ------------ |
| [`facet-bindings/P9-0000-index.md`](./facet-bindings/P9-0000-index.md) | Binding sub-series index + build-priority order, quickest win to largest lift. |
| [`facet-bindings/P9-0001-binding-architecture.md`](./facet-bindings/P9-0001-binding-architecture.md) | Two-tier binding model (native IDL projection + foreign adapters), per-facet matrix, resolved decisions. |
| [`facet-bindings/P9-0002-projection-conventions.md`](./facet-bindings/P9-0002-projection-conventions.md) | Shared REST/JSON-RPC/gRPC/MCP/auth/error conventions + the per-facet doc template. |
| `facet-bindings/P9-0003` through `P9-0017` | Per-facet bindings (files, vcs, cas, sql, kv, document, columnar, time-series, queue, vector, search, graph, ledger) plus the MCP server and FUSE mount. |
| [`facet-bindings/REFERENCE-IMPLEMENTATIONS.md`](./facet-bindings/REFERENCE-IMPLEMENTATIONS.md) | Reference landscape per facet: OSS/commercial impls, interface standards, default ports. |
| [`facet-bindings/IMPLEMENTATION-FIDELITY.md`](./facet-bindings/IMPLEMENTATION-FIDELITY.md) | How faithfully each facet tracks its reference (🟢🟡(high risk)🔴 flags) + the exposure gap. |
| [`facet-bindings/REFERENCE-CLIENTS-AND-FORMATS.md`](./facet-bindings/REFERENCE-CLIENTS-AND-FORMATS.md) | Per-facet file formats and GUI/desktop client tools (the reverse mapping). |
| [`facet-bindings/SPEC-RECONCILIATION-FINDINGS.md`](./facet-bindings/SPEC-RECONCILIATION-FINDINGS.md) | **Temporary** backlog of spec<->build mismatches surfaced by the binding work (resolve, then delete). |
| [`control-plane-bindings/CP-0000-index.md`](./control-plane-bindings/CP-0000-index.md) | Control-plane binding plan: `exec`, `watch`, `trigger`, `identity`, `acl` (spans P11-P13). |
| [`ai/AI-0000-index.md`](./ai/AI-0000-index.md) | AI provider bindings: `providers.embedding` (0050, intended first-class) + `providers.llm` (0051, small-models-leaning, multi-source loading). |

## Conventions

- **Version-control behavior section.** Every **data-layer** spec (0011, 0016-0024) MUST include a
  "Version-control behavior (against the <facet> dataset)" section describing how commit / checkout /
  branch / merge / diff / log / sync behave against *that* facet's data, and what is **derived**
  (rebuilt, never versioned or synced - e.g. an ANN index, a BM25 index, an analytical view) versus
  what is the versioned source of truth. Template and the canonical example: 0017 §7b.
- **Priority labels in child specs.** Actionable future items in `00##a`, `00##b`, `00##c`, and later
  child specs use `(P0)` for release blockers, `(P1)` for core feature work, `(P2)` for enhancements,
  and `(P3)` for optional post-release work.

## Reading order

1. **0001** for the mental model and vocabulary.
2. **0002** and **0003** together - the data model and the interface are two views of the same thing.
3. Pick the backend you care about in **0004**, then its detail doc (**0005** for single-file).
4. **0006** if you need replication; **0007** if you are implementing a language binding;
   **0008** if you are exposing Loom over a network; **0009** for hardening; **0010** to certify.

## Requirement levels (RFC 2119 / RFC 8174)

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**, **SHALL NOT**, **SHOULD**,
**SHOULD NOT**, **RECOMMENDED**, **MAY**, and **OPTIONAL** in this spec series are to be
interpreted as described in BCP 14 (RFC 2119 and RFC 8174) when, and only when, they appear in
all capitals.

A **conformant implementation** is one that satisfies every **MUST**/**MUST NOT**/**SHALL**
applicable to the conformance level and capability profile it claims (see 0010). Sections marked
*Informative* are explanatory and carry no conformance weight; sections marked *Normative* do.

## Notational conventions

- **Type signatures** use the Loom IDL (defined in 0003 §3). The IDL is language-neutral; each
  binding maps it to native types per 0007.
- **Byte layouts** (0005) use diagrams where each cell is one octet unless a bit count is given.
  Multi-byte integers are **little-endian** unless stated otherwise.
- **Digests** are written as `algo:hex` (e.g. `blake3:9f86d0...`) per 0002 §2.
- **Pseudocode** is illustrative only and never normative.
- Examples and non-normative asides are set off with a leading `> NOTE:` or in fenced blocks.

## Document lifecycle

Each document carries a `Status` of `Draft`, `Review`, `Stable`, or `Deprecated`, and a
`Version`. A document at `Stable` MUST NOT make breaking changes without a major version bump of
the spec series (0010 §5). Changes are proposed as numbered RFCs that amend or supersede a
document; superseded documents are retained with a `Deprecated` banner pointing forward.
