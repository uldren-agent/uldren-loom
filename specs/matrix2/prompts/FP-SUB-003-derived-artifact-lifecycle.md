# FP-SUB-003 Derived Artifact Lifecycle

## Packet Metadata

| Field | Value |
| --- | --- |
| Packet ID | FP-SUB-003 |
| Lane | Shared substrate |
| Status | Ready |
| ROI | 9 |
| Lift | 6 |
| Risk | 7 |
| Source dependency | Full `specs/_FACET_PRIMITIVES.md` |
| Packet dependencies | FP-SUB-001 helpful but not required |
| Allowed files | `specs/_FACET_PRIMITIVES.md`, relevant numbered specs only if needed for source anchors, and this packet file |
| Blocked files | Code, queues outside `specs/matrix2/`, implementation plans, lockfiles |

## Source Anchors

Read these before acting:

| Source | Required section |
| --- | --- |
| `specs/_FACET_PRIMITIVES.md` | Whole file for dependency scope; Appendix C, Appendix D, Appendix G, FTS, vector, graph, dataframe, columnar, PIM, metrics, and IPFS sections for the first deep pass |
| `specs/matrix2/MATRIX.md` | Active Matrix and Review Rule |

The targeted sections above are not the full dependency boundary. Check the rest of the primitive
file for derived state, index, cache, materialization, retention, and maintenance interactions
before writing results.

## Task

Design a shared derived-artifact lifecycle handoff packet. Do not implement code. The target is one
lifecycle model for rebuildable local or generated artifacts such as FTS indexes, vector ANN/PQ
artifacts, graph property indexes, dataframe materializations, columnar statistics, metrics rollups,
PIM search indexes, and IPFS cache indexes.

The design must separate:

| Concern | Direction |
| --- | --- |
| Source truth | Canonical Loom roots and records remain the source of truth. |
| Derived bytes | Engine outputs are rebuildable and must not define source identity. |
| Stamps | Source digest, engine version, format version, config digest, and compatibility profile should be explicit. |
| State | Ready, stale, building, failed, disabled, unsupported, and unavailable states should be reportable. |
| Serving policy | Query surfaces must know whether stale, rebuilding, or reduced artifacts may serve results. |
| Maintenance | Rebuild scheduling belongs with shared background maintenance, not each facade's private loop. |

## Stop Conditions

Stop and record a decision point if:

| Condition | Why |
| --- | --- |
| A derived artifact is proposed as canonical source truth. | That violates the whole-blob promotion rule and versioned Loom model. |
| A facet needs a unique lifecycle state not covered by the shared model. | The shared model may need extension before implementation. |
| The packet would require implementation edits. | This packet is design and handoff only. |

## Required Output

Update the `Results` section with:

1. Files changed.
2. Source anchors checked.
3. Proposed owning specs.
4. Shared lifecycle states and metadata.
5. Facet mappings.
6. Follow-on implementation packets.
7. Checks run.
8. Blockers or decision points.

## Results

Status: Complete - design handoff ready for review.

### Files Changed

- `specs/matrix2/prompts/FP-SUB-003-derived-artifact-lifecycle.md`

### Source Anchors Checked

- Full `specs/_FACET_PRIMITIVES.md`, with deep reads of FTS, vector, graph, columnar, dataframe,
  PIM, metrics, Appendix A IPFS posture, Appendix C promotion rule, Appendix D priorities,
  Appendix F proof requirements, Appendix G dependency graph, and Appendix H profiles.
- `specs/matrix2/MATRIX.md` Active Matrix and Review Rule.
- `specs/0033-search-layer.md` for the existing source-digest-stamped artifact store, status,
  rebuild, coalescing, `INDEX_NOT_READY`, and reduced-engine behavior.
- Relevant numbered-spec searches for vector, graph, columnar, dataframe, metrics, PIM, and CAS
  source anchors, engine stamps, materialization, retention, and rebuild behavior.

### Proposed Owning Specs

1. Add one shared-substrate numbered specification, tentatively **Derived Artifact Lifecycle**, to
   own artifact identity metadata, stamp comparison, lifecycle state vocabulary, safe serving-policy
   vocabulary, invalidation, maintenance handoff, audit, and conformance. It must state that an
   artifact payload is never canonical source truth.
2. The background-maintenance specification owns durable job scheduling, leases, fencing,
   cancellation, retry, progress, and coalescing. It receives lifecycle requests but does not define
   facet artifact meaning.
3. `0033`, `0017`, `0016`, `0023`, `0045`, `0022` and the future metrics facet, `0037`, `0038`,
   `0039`, and the IPFS facade consume the shared lifecycle. Each owning facet still defines its
   canonical inputs, artifact type, query semantics, and whether a safe fallback exists.
4. `0009` owns authorization, redaction, audit retention, and policy checks. `0008` and hosted
   facades only project lifecycle records and serving outcomes. No facade owns a private rebuild loop.

### Shared Lifecycle States And Metadata

The lifecycle record is durable-local operational metadata, not a committed facet root. Payloads may
be retained, copied, or discarded according to local policy, but commit, clone, merge, sync, and
source identity must remain correct when every payload is absent.

| State | Meaning | Required transition rule |
| --- | --- | --- |
| `ready` | Payload passed integrity checks and every required stamp matches the selected source and profile. | May serve only under the owner-declared ready policy. |
| `stale` | A source, configuration, engine, format, profile, or policy stamp no longer matches, or explicit invalidation occurred. | Must not silently serve as current. A declared as-of policy may expose it only with source-anchor disclosure. |
| `building` | A leased, fenced maintenance job is building one immutable candidate from a captured stamp set. | Concurrent requests coalesce by artifact identity and stamp set; a stale job cannot publish. |
| `failed` | The latest candidate build failed after validation or execution. | Preserve safe failure code, redacted diagnostic, failed stamp set, and retry eligibility. |
| `disabled` | Operator configuration or policy intentionally prevents activation while preserving inspectable intent. | No build starts until explicitly enabled and authorized. |
| `unsupported` | The selected artifact/profile cannot be produced by this build or platform. | Preserve configuration and report compile or platform reason without attempting a build. |
| `unavailable` | Support exists but a required runtime, dependency, resource, or provider is temporarily unavailable. | Retry follows maintenance policy; capability output distinguishes it from unsupported. |

`missing` is not an additional persisted state. It means no artifact lifecycle record or payload exists
for a requested identity and is evaluated as not ready. `reduced` is likewise not a lifecycle state:
it is a response capability marking an owner-defined portable or lower-fidelity fallback.

Every lifecycle record requires:

1. Artifact identity: owner facet, workspace and authorized scope, artifact kind, key or selector,
   artifact schema version, and explicit artifact profile.
2. Input stamps: ordered canonical source anchor digests, source format or root version, configuration
   digest, engine identifier and version, artifact format version, compatibility-profile identifier
   and version, and optional platform or feature stamp.
3. Operational metadata: current state, record generation, creation, update, observation, and
   invalidation times, maintenance job and fence reference when building, progress summary, retry
   policy reference, and safe failure reason when failed.
4. Payload metadata: local storage reference, payload digest or integrity checksum, byte and entry
   counts, and optional source-equivalence boundary. Payload metadata is operational evidence, not a
   canonical source reference.
5. Serving and governance metadata: declared serving policy, fallback class, freshness or as-of
   anchor, authorization/redaction class, retention class, and audit correlation identifiers.

The default serving policy is `fresh_required`: only a matching `ready` artifact may serve its
artifact-dependent result; otherwise return owner-mapped not-ready or unavailable behavior. Facets
may declare `reduced_fallback` when a portable computation has a documented semantic boundary and
labels every result as reduced. `as_of_stale_read` is permitted only for a named immutable source
anchor, with the anchor and staleness disclosed. A facade cannot silently choose stale data merely to
avoid a rebuild.

### Facet Mappings

| Consumer | Canonical source truth | Derived artifact and serving rule |
| --- | --- | --- |
| FTS | Versioned field documents and mapping. | Tantivy payload is source-digest, engine, analyzer, and format stamped. Missing or stale native index returns `INDEX_NOT_READY`; portable search may serve only as explicitly `reduced`. |
| Vector | Vector manifest, vectors, metadata, metric, and exact search contract. | ANN and PQ payloads are rebuildable accelerators. Exact search remains the portable authority; approximate output must disclose recall or profile boundary. |
| Graph | Canonical graph records and declared query model. | Property, adjacency, and query indexes are rebuildable. A stale index cannot silently change traversal or authorization visibility. |
| Dataframe | Canonical frame plan, source bindings, and materialization policy. | Materialized results carry plan, input source, executor, output, and policy stamps. They may serve only when refresh policy and source anchors match. |
| Columnar | Dataset manifest, Arrow schema, partition metadata, and immutable segment references. | Statistics, compaction products, and result caches are derived. Planner use requires matching manifest/schema/profile stamps. |
| Metrics | Canonical descriptors and typed points over time-series storage. | Rollups carry descriptor, attribute projection, aggregation window, temporality transform, and rebuild status. Stale rollups may only answer declared as-of windows with disclosure. |
| Calendar, contacts, mail | Canonical records, committed history, and native change state. | Recurrence, contact lookup, mailbox search, header, body, thread, and flag indexes are derived. Scan or bounded native fallback is allowed only when the owning query profile declares it. |
| IPFS | Loom canonical content identity, foreign-CID catalog, and authorized retrieval policy. | Gateway and CID cache indexes are local derived artifacts. Foreign CIDs never become Loom identity; cache misses or stale entries must not masquerade as local content. |

### Follow-On Implementation Packets

1. **DAL-001 Shared record and conformance**: define durable-local lifecycle metadata, state
   transition validation, stamp comparison, payload integrity checks, redaction, canonical
   operational-record vectors, and negative transition tests.
2. **DAL-002 Maintenance integration**: define job request, deduplication key, lease/fence publish
   gate, cancellation, retry, progress, and audit bridge. Prove stale jobs cannot publish after
   source or configuration changes.
3. **DAL-003 FTS migration**: adapt existing search artifact helpers to the shared record while
   preserving `INDEX_NOT_READY`, engine-stamped score vectors, and reduced portable behavior.
4. **DAL-004 Vector, graph, columnar, and dataframe adoption**: add owner-specific stamp builders,
   explicit exact or reduced fallback boundaries, materialization invalidation, and query planner
   readiness checks.
5. **DAL-005 Metrics, PIM, and IPFS adoption**: add rollup, search-index, and cache lifecycle
   records; verify as-of reads, retention, authorization filtering, and foreign-identity boundaries.
6. **DAL-006 Hosted, CLI, MCP, and bindings projection**: expose the same record and capability state
   across surfaces without exposing local payload paths or private diagnostics.

### Checks Run

- Read the complete `_FACET_PRIMITIVES.md` in sequential source ranges, then re-read the required
  facet, appendix, maintenance, retention, capability, and serving-policy anchors.
- Read `specs/matrix2/MATRIX.md` Active Matrix and Review Rule and the Loom prompt resource.
- Searched source-backed FTS and relevant numbered specifications for existing lifecycle stamps,
  status transitions, portability, materialization, and engine boundaries.
- No code, lockfile, queue, or implementation-plan check was run because this is a design-only
  packet. No implementation claim is made.

### Blockers Or Decision Points

Decision Points: none.

No consumer needs an additional lifecycle state. `missing` is absence of a lifecycle record and
`reduced` is a response capability, so both fit the shared model without extending its state
vocabulary. The packet proposes no derived payload as canonical source truth.
