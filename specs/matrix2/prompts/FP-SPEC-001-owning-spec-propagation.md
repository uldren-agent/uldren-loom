# FP-SPEC-001 Owning-Spec Propagation

## Packet Metadata

| Field | Value |
| --- | --- |
| Packet ID | FP-SPEC-001 |
| Lane | Owning-spec propagation |
| Status | Ready |
| ROI | 9 |
| Lift | 5 |
| Risk | 5 |
| Source dependency | Full `specs/_FACET_PRIMITIVES.md` |
| Packet dependencies | none |
| Allowed files | `specs/_FACET_PRIMITIVES.md`, owning numbered specs selected by the worker, and this packet file |
| Blocked files | Code, queues outside `specs/matrix2/`, implementation plans, lockfiles |

## Source Anchors

Read these before acting:

| Source | Required section |
| --- | --- |
| `specs/_FACET_PRIMITIVES.md` | Whole file for dependency scope; Appendix D, Appendix E, Appendix G, and the per-facet sections selected for propagation for the first deep pass |
| `specs/matrix2/MATRIX.md` | Active Matrix and Review Rule |

The targeted sections above are not the full dependency boundary. The propagation plan must account
for the facet inventory, planned facets, all per-facet sections, and all appendices because this
file is the source for remediating primitive quality across Loom.

## Task

Create a batch plan for moving primitive decisions into owning numbered specs. Do not attempt to
update every spec in one pass. Identify the first coherent propagation batch, the owning specs,
the decisions that should move, and the task packets needed for later sessions.

Good propagation batches are cross-cutting and DRY, for example:

| Batch | Why it is coherent |
| --- | --- |
| Capability reporting | Affects CLI, MCP, hosted routes, bindings, engines, and packaging. |
| Derived artifacts | Affects FTS, vector, graph, dataframe, columnar, metrics, PIM, and IPFS. |
| Conditional mutation | Affects files, PIM, `kv`, `cache`, document, hosted resources, and S3. |
| Change sets | Affects calendar, contacts, mail, queue, ledger, delivery, watches, and push. |

## Stop Conditions

Stop and record a decision point if:

| Condition | Why |
| --- | --- |
| Two specs disagree about ownership. | The owner decision must be visible before propagation. |
| The worker cannot identify source anchors. | Moving ungrounded claims into specs creates drift. |
| The packet would require code edits. | This packet is spec propagation planning only. |

## Required Output

Update the `Results` section with:

1. Files changed.
2. Source anchors checked.
3. Recommended first propagation batch.
4. Owning specs and exact sections to update.
5. New prompt packets that should be created.
6. Checks run.
7. Blockers or decision points.

## Results

Status: Complete - first propagation batch ready for review.

### Files Changed

- `specs/matrix2/prompts/FP-SPEC-001-owning-spec-propagation.md`

### Source Anchors Checked

- Full `specs/_FACET_PRIMITIVES.md`, including the complete facet inventory, planned facets,
  per-facet sections, Appendix D priority analysis, Appendix E readiness conclusions, Appendix F
  proof matrix, Appendix G shared-substrate dependency graph, and Appendix H profiles.
- `specs/matrix2/MATRIX.md` Active Matrix and Review Rule.
- Accepted-with-caveat capability-reporting verification and prior Matrix handoffs for conditional
  mutation and derived-artifact lifecycle. Capability reporting is already propagated enough to be
  removed from this first batch.
- `0003` Sections 4, 6, 8, and 9; `0003c` Section 1.2; `0019` Sections 6 and 10; `0019a`
  Sections 5 and 6; `0019b` Sections 3, 7, and 8; `0020` Sections 6 and 10; `0024` Sections 6,
  8, and 9; `0037` Sections 2.1, 2.3, and 6; `0038` Sections 3, 4, and 7; `0039` Sections 1, 2,
  5, and 8; `0004a` Sections 2 and 3; `0008` Sections 3.4, 3.5, 6, and 7; and `0009` Sections
  5.1, 5.3, and 5.7.

### Recommended First Propagation Batch

**Conditional mutation and comparison anchors** is the recommended first batch.

It is the next Appendix G critical-path substrate after capability reporting, has a bounded semantic
surface, and prevents files, PIM, `kv`, cache, document, hosted routes, WebDAV, and S3 from creating
incompatible lost-update rules. Derived artifacts should follow it: that batch is larger and depends
on the same shared maintenance, audit, capability, and stable-error posture.

### Ownership Placement Decision

**Question:** Should the shared conditional-mutation contract be a new numbered shared-substrate
specification, or should existing `0003 - Core Interface` Sections 8 and 9 own it?

**Context:** FP-SUB-002 tentatively proposed a new shared specification because the contract spans
files, PIM, `kv`, cache, document, hosted resources, and S3-style operations. Existing `0003`
already defines multi-step atomicity in Section 6, stable public errors in Section 8, and normative
concurrency and consistency in Section 9. This batch must make ownership visible before any
consumer specification is changed.

**Examples:** A `kv` ordered compare-and-apply and a CalDAV `If-Match` request need the same atomic
condition result and audit boundary, but neither Redis revision syntax nor HTTP ETag syntax should
become native. Conversely, a new shared specification can make the cross-facet contract visually
distinct but introduces another public specification boundary and cross-reference set.

**Options:**

1. Create a new numbered shared-substrate specification that owns the conditional-mutation contract.
   `0003` would only reference it.
2. Extend `0003` with a new Section 9.1 and Section 8 cross-references. Every facet and facade would
   consume that existing core contract.

**Recommendation:** Select option 2, existing `0003` ownership. The contract is a core public
concurrency and stable-error primitive, not a new data facet. This produces one contract and one
public error vocabulary, avoids a new registry and migration boundary, and prevents `kv`, WebDAV,
or S3 from becoming an accidental owner.

**Consequence of deferring:** `FP-SPEC-001A`, `FP-SPEC-001B`, `FP-SPEC-001C`, `FP-CONF-002`, and
`FP-IMPL-002` remain blocked. Proceeding would risk competing public token, error, and audit
contracts in their selected specifications.

### Owning Specs And Exact Sections To Update

| Specification | Exact section | Propagation change |
| --- | --- | --- |
| `0003 - Core Interface` | Add 9.1, **Conditional mutation and comparison anchors**, under Section 9; cross-reference Section 8 error taxonomy and Section 6 batches. | Normatively define owner-scoped atomic compare conditions `any`, `absent`, `exact`, `generation`, and `operation_anchor`; opaque owner-issued tokens; atomic read point; no partial write; `CONFLICT`, `ALREADY_EXISTS`, and existing lock or fencing errors; safe result disclosure; audit hooks; and no global serialized token or entity-tag syntax. |
| `0003c - Filesystem projection` | Section 1.2 Concurrency with history and Section 1.3 Error mapping. | Bind file write, rename, copy, and delete to 0003 anchors; keep mount cache and handle state out of native identity; map stale anchors and lock fencing without inventing a filesystem-private compare model. |
| `0019 - Key-Value Layer` | Section 6 Target Contract and Section 10 Unfinished Work. | Define single-key and ordered-batch compare-and-apply as 0003 consumers; state per-key/map token provenance and batch atomicity; remove any implication that etcd or Redis revisions define native semantics. |
| `0019a - Ephemeral Cache Tier` | Section 5 Durability and rebuild semantics and Section 6 Concurrency. | Bind cache CAS to lifecycle-bounded volatile generations; specify invalidation, eviction, coordinator restart, and no durable or synchronizable cache token claim. |
| `0019b - Redis And Memcached Presentations` | Sections 3 Redis Surface, 7 Memcached Surface, and 8 Relationship To Base KV And Cache. | Map Redis conditional commands and Memcached CAS to the shared outcomes while retaining RESP/text grammar, byte-key rules, token syntax, and client-visible errors as facade-owned. |
| `0020 - Document Layer` | Section 6 Target Contract and Section 10 Unfinished Work. | Add owner-defined document revision or canonical replacement-anchor requirements for replace and declared atomic patch operations. Keep MongoDB/Couchbase revision and session grammar outside the native contract. |
| `0037 - Calendar Layer` | Sections 2.1 Lossless round-trip and ETag, 2.3 ETag and sync-token, and 6 Hosted CalDAV Projection. | State that canonical entry bytes provide a 0003 exact anchor and CalDAV ETag projection; keep collection sync tokens separate from write preconditions. |
| `0038 - Contacts Layer` | Sections 3 Facade, 4 Resolved decisions RD3, and 7 Hosted CardDAV Projection. | Preserve canonical-record ETag semantics while replacing implicit write checks with the 0003 condition/result/audit contract; keep CardDAV syntax facade-owned. |
| `0039 - Mail Layer` | Sections 1 Model, 2 flags storage, 5 Resolved decisions, and 8 Hosted Mail Projection. | Bind mutable mailbox and flag state to generation and operation anchors; distinguish mergeable per-keyword operations from stale full replacement; keep IMAP/JMAP state tokens distinct from HTTP entity tags. |
| `0004a - Hosted Provider Facade` | Section 2 Target Interface and Section 3 Hosted Capability Evaluation. | Require the hosted kernel to authorize, call the owning 0003 compare primitive, redact result disclosure, and emit audit correlation before protocol mapping. |
| `0008 - Wire Protocols` | Sections 3.4 Caching, conditional requests, and idempotency; 3.5 Hosted PIM protocol projections; 6 Error Mapping; and 7 Served adapters and write authority. | Define HTTP/WebDAV/CalDAV/CardDAV conditional-header translations, REST/JSON-RPC/gRPC conflict mappings, and the rule that product headers cannot change native compare meaning. |
| `0009 - Security And Capabilities` | Sections 5.1 Principals and access control, 5.3 Lifecycle and governance, and 5.7 Hosted capability disclosure. | Define compare authorization order, conflict redaction, success/failure audit fields, sensitive-token handling, and retention treatment for compare evidence. |
| `0024 - CAS Layer` | Sections 6 Target Contract, 8 Relationship to Other Facets, and 9 Non-Goals and Limits. | Record the explicit exclusion: immutable digest-addressed CAS put is idempotent and is not a universal conditional-write primitive. S3 object-key/version and ETag semantics remain S3-facade-owned adapters. |

### New Prompt Packets To Create

1. **FP-SPEC-001A - Core conditional-mutation propagation** - **Blocked by acceptance of the
   ownership placement decision.**
   - Scope: `specs/0003-core-interface.md`, `specs/0009-security-and-capabilities.md`.
   - Deliverable: normative 0003 Section 9.1 contract plus 0009 authorization, audit, redaction, and
     retention rules. It must preserve opaque owner tokens and existing stable error ownership.
2. **FP-SPEC-001B - Native consumer propagation** - **Blocked by acceptance of the ownership
   placement decision and FP-SPEC-001A.**
   - Scope: `specs/0019-key-value-layer.md`, `specs/0019a-ephemeral-cache-tier.md`,
     `specs/0020-document-layer.md`, `specs/0037-calendar-layer.md`,
     `specs/0038-contacts-layer.md`, `specs/0039-mail-layer.md`.
   - Deliverable: each facet declares anchor provenance, atomic scope, allowed compare kinds, merge
     boundary, and whether it exposes a readable entity tag.
3. **FP-SPEC-001C - Facade and hosted projection propagation** - **Blocked by acceptance of the
   ownership placement decision and FP-SPEC-001A.**
   - Scope: `specs/0003c-filesystem-projection.md`, `specs/0019b-redis-memcached-presentations.md`,
     `specs/0004a-hosted-provider-facade.md`, `specs/0008-wire-protocols.md`,
     `specs/0024-cas-layer.md`.
   - Deliverable: protocol mapping matrix for WebDAV, CalDAV, CardDAV, S3, Redis, Memcached, REST,
     JSON-RPC, and gRPC. Each mapping must consume 0003 and cannot create a new native token format.
4. **FP-CONF-002 - Conditional-mutation proof matrix** - **Blocked by acceptance of the ownership
   placement decision and the normative core contract.**
   - Scope: `specs/0010a-conformance-reporting-and-certification.md`,
     `specs/0032-platform-parity.md`, and a future implementation/conformance packet.
   - Deliverable: canonical owner-token test harness, exact/absent/generation/fencing negatives,
     atomicity races, policy redaction, audit, and real facade transcript requirements.
5. **FP-IMPL-002 - Shared conditional-mutation implementation handoff** - **Blocked by acceptance
   of the ownership placement decision, accepted FP-SPEC-001A through FP-SPEC-001C, and
   FP-CONF-002.**
   - Dependency: accepted ownership placement, FP-SPEC-001A through FP-SPEC-001C, and FP-CONF-002.
   - Deliverable: crate-level implementation slices that first add the 0003 contract and then adopt
     it without changing stable public semantics by accident.

### Checks Run

- Read the full primitive inventory and the required Appendix D, Appendix E, Appendix F, Appendix
  G, and profile anchors.
- Read the active Matrix and the Loom prompt document.
- Read accepted and in-review Matrix handoffs to avoid duplicating accepted capability work.
- Inspected exact numbered-spec sections selected for the proposed first batch.
- No code, lockfile, implementation-plan, or queue change was made. No implementation test is
  claimed for this planning packet.

### Blockers Or Decision Points

**Ownership placement is awaiting owner or arbiter acceptance.** The recommendation is existing
`0003` Sections 8 and 9, but this packet does not settle that public placement unilaterally. No
selected consumer specification claims competing ownership; the unresolved issue is whether the
shared contract deserves a separate public specification boundary.
