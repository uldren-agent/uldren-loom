# FP-SUB-002 Conditional Mutation And Entity Tags

## Packet Metadata

| Field | Value |
| --- | --- |
| Packet ID | FP-SUB-002 |
| Lane | Shared substrate |
| Status | Ready |
| ROI | 8 |
| Lift | 5 |
| Risk | 6 |
| Source dependency | Full `specs/_FACET_PRIMITIVES.md` |
| Packet dependencies | none |
| Allowed files | `specs/_FACET_PRIMITIVES.md`, relevant numbered specs only if needed for source anchors, and this packet file |
| Blocked files | Code, queues outside `specs/matrix2/`, implementation plans, lockfiles |

## Source Anchors

Read these before acting:

| Source | Required section |
| --- | --- |
| `specs/_FACET_PRIMITIVES.md` | Whole file for dependency scope; Appendix D, Appendix G, calendar, contacts, mail, files, `kv`, `cache`, and `cas` sections for the first deep pass |
| `specs/matrix2/MATRIX.md` | Active Matrix and Review Rule |

The targeted sections above are not the full dependency boundary. Check the rest of the primitive
file for any additional compare, state-token, policy, hosted, or facade interactions before writing
results.

## Task

Design the shared conditional-mutation and entity-tag primitive as an owning-spec handoff packet.
Do not implement code. The goal is one reusable lost-update contract that files, WebDAV, CalDAV,
CardDAV, IMAP/JMAP state, `kv`, `cache`, document updates, hosted resources, and S3-style
conditional operations can map onto.

The design must identify:

| Concern | Required direction |
| --- | --- |
| Token source | Whether a token comes from canonical bytes, mutable state version, operation anchor, or facade-specific representation. |
| Compare behavior | Exact match, absent match, generation match, stale-progress rejection, and conflict errors. |
| Entity tags | Which surfaces expose entity tags and which only use internal compare tokens. |
| Audit | What must be recorded when a compare fails or succeeds. |
| Cross-facet reuse | Which facets consume the primitive without owning it privately. |

## Stop Conditions

Stop and record a decision point if:

| Condition | Why |
| --- | --- |
| A global token format is being proposed. | Token shape can become a public contract. |
| A facade-specific conditional header would distort the native primitive. | Product protocol behavior must not define native storage semantics. |
| The packet would require implementation edits. | This packet is design and handoff only. |

## Required Output

Update the `Results` section with:

1. Files changed.
2. Source anchors checked.
3. Proposed owning specs.
4. Shared primitive requirements.
5. Facet and facade mappings.
6. Follow-on implementation packets.
7. Checks run.
8. Blockers or decision points.

## Results

Status: Complete - design handoff ready for review.

### Files Changed

- `specs/matrix2/prompts/FP-SUB-002-conditional-mutation-entity-tags.md`

### Source Anchors Checked

- Full `specs/_FACET_PRIMITIVES.md`, including the shared control-plane table, `files`, `kv`,
  `cache`, `cas`, PIM, Appendix D, Appendix F, Appendix G, and Appendix H.
- `specs/matrix2/MATRIX.md` Active Matrix and Review Rule.
- `specs/0003c-filesystem-projection.md`, `specs/0019-key-value-layer.md`,
  `specs/0020-document-layer.md`, `specs/0024-cas-layer.md`, `specs/0037-calendar-layer.md`,
  `specs/0038-contacts-layer.md`, `specs/0039-mail-layer.md`,
  `specs/0004a-hosted-provider-facade.md`, and `specs/0009-security-and-capabilities.md` for
  existing surface and audit boundaries.
- `crates/loom-types/src/error.rs` for the existing stable `CONFLICT`, `FENCING_STALE`, and
  related lock error vocabulary. This was read as a source anchor only; no code change is proposed.

### Proposed Owning Specs

1. Add one new shared-substrate numbered specification, tentatively **Conditional Mutation and
   Comparison Anchors**, as the sole owner of compare semantics, compare result shape, audit
   requirements, and conformance vectors. It owns no facet data model and no printed token format.
2. `0019` consumes the shared contract for single-key and ordered-batch compare-and-apply.
   `0019a` and `0019b` consume it for cache CAS and Redis or Memcached compatibility.
3. `0003` and `0003c` consume it for atomic tree mutation and WebDAV compare-before-write.
   File paths and tree identity remain file-owned.
4. `0020` consumes it for document replacement and declared atomic patch operations. Query and
   index semantics remain document-owned.
5. `0037`, `0038`, and `0039` consume it for calendar/contact entity tags and mutable mail state.
   PIM collection, sync, and lifecycle semantics remain PIM-owned.
6. `0004a` and `0008` consume it for hosted request/kernel and protocol error projection. They do
   not define native compare semantics. `0009` consumes it for authorization, redaction, and audit.
7. `0024` does not consume it for immutable digest-addressed CAS bytes. Its S3 presentation maps
   S3 version identifiers and conditional headers through the shared contract only for mutable S3
   object-name or representation state.

### Shared Primitive Requirements

The primitive is a reusable atomic mutation guard, not a global token format.

1. Every guarded mutation names an owner-defined target and atomic scope, an owner-defined mutation,
   and one of these semantic conditions: `any`, `absent`, `exact`, `generation`, or
   `operation_anchor`.
2. `exact` compares an owner-issued opaque current-state token. The token can be derived from
   canonical record bytes, a mutable-state revision, or a facade-specific projection, but its bytes,
   syntax, lifetime, and disclosure rules remain with the owning facet or facade.
3. `absent` succeeds only when the owner-defined target is absent in the same atomic scope. A
   create collision returns `ALREADY_EXISTS`; a failed caller-supplied absence precondition returns
   `CONFLICT` with compare disposition `absent_mismatch`.
4. `generation` compares a monotonic owner-defined generation when content equality is insufficient,
   such as a mailbox state, collection revision, or mutable named object. It is not a universal
   integer representation.
5. `operation_anchor` binds a mutation to an owner or coordination authority. It is used for
   fencing and stale-progress rejection, not as an HTTP entity tag. A stale fence returns the
   established `FENCING_STALE`; a missing or expired lease retains its established lock error.
6. Evaluation is atomic with the mutation after authorization and policy checks, against one
   documented read point. The success result returns the owner-defined new anchor or token only when
   policy permits disclosure. No failed compare partially applies a mutation.
7. A normal failed exact, absent, or generation condition returns stable `CONFLICT`, a machine
   readable compare disposition, and no current token unless the caller is authorized to read the
   target. Facades translate that native result to their documented precondition or conflict form.
8. Entity tags are an optional owner-declared presentation of a readable exact token. A strong
   entity tag is allowed only when its derivation covers the representation selected by the facade.
   The shared primitive standardizes provenance and compare behavior, not an ETag string, digest
   algorithm, quoting rule, or weak-tag syntax.
9. Success and failure audit records must include principal or service identity, target and scope,
   operation kind, compare semantic kind, outcome, stable error or disposition, resulting anchor
   fingerprint on success, policy decision, correlation id, and observation time. They must not
   store raw credentials, raw opaque tokens, protected current values, or topology details. Failure
   records retain a presented-token fingerprint only when it is safe and useful for correlation.

### Facet And Facade Mappings

| Consumer | Native token source and compare use | Entity tag or facade projection |
| --- | --- | --- |
| Files and WebDAV | Tree or entry anchor for atomic write, rename, delete, and compare-before-write. Lock fencing is a separate operation anchor. | WebDAV maps owner-approved readable exact state to `If-Match` or `If-None-Match`; it must not make a mount cache token native identity. |
| Calendar and CalDAV | Canonical calendar-entry bytes supply an exact token; collection changes remain sync tokens, not write ETags. | Canonical entity tag is exposed through CalDAV conditional requests. |
| Contacts and CardDAV | Canonical contact-record bytes supply an exact token. | Canonical entity tag is exposed through CardDAV conditions. |
| Mail, IMAP, and JMAP | Mutable mailbox and flag state uses generation or state tokens; operation-style flag updates use an observed state and merge rules, while stale replacement is rejected. | IMAP and JMAP expose their own state/precondition forms. They do not relabel mailbox state tokens as HTTP entity tags. |
| `kv`, etcd, Redis | Typed-map revision or per-key owner token supports exact, absent, and ordered batch compare-and-apply. | No native entity tag. etcd revisions and Redis command semantics stay facade-specific. |
| `cache`, Memcached | Volatile entry generation supports CAS only within the cache lifecycle and coordinator policy. | No native entity tag. Memcached CAS tokens are facade tokens and expire with entry lifecycle. |
| Document, MongoDB, Couchbase | Owner-defined document revision or canonical replacement anchor guards replace and declared atomic patch operations. | No native ETag requirement. HTTP or product facades expose one only after their representation and revision rules are specified. |
| Hosted resources | Hosted kernel invokes the owner primitive after auth and before mutation, then maps its stable result and redacted audit record. | REST, JSON-RPC, gRPC, and product headers remain adapters. |
| CAS and S3 | Immutable CAS put is idempotent by digest and does not require this primitive. Mutable S3 object-key/version state may use an owner-defined representation revision. | S3 owns ETag/version-id syntax and conditional headers. Loom digests are never advertised as S3 ETags by implication. |

### Follow-On Implementation Packets

1. **CM-001 Shared contract and conformance**: define the shared IDL/Rust result model without a
   universal serialized token; add canonical operation transcripts, negative cases, and stable-error
   vectors for exact, absent, generation, and operation-anchor conditions.
2. **CM-002 KV and cache adoption**: add single-key and ordered-batch compare-and-apply, cache CAS
   lifecycle bounds, Redis/Memcached translation, and race/concurrency conformance.
3. **CM-003 Files, document, and hosted adoption**: bind atomic file and document operations to
   owner anchors; route hosted mutations through the request kernel with policy redaction and audit.
4. **CM-004 PIM adoption**: normalize calendar/contact entity-tag compare and mail observed-state
   replacement semantics without conflating sync tokens, ETags, and mutable-state versions.
5. **CM-005 S3/WebDAV compatibility transcripts**: map native outcomes to conditional headers and
   status responses, prove foreign identifiers remain facade-owned, and run real-client transcripts.

### Checks Run

- Read the complete `_FACET_PRIMITIVES.md` in six sequential source ranges, then re-read the
  required shared-substrate, PIM, CAS, and priority anchors.
- Read `specs/matrix2/MATRIX.md` Active Matrix and Review Rule.
- Searched relevant numbered specs and the stable error enum for existing conditional, ETag,
  state-token, compare, audit, and fencing behavior.
- No code, lockfile, queue, or implementation-plan checks were run because this is a design-only
  packet. No implementation claim is made.

### Blockers Or Decision Points

Decision Points: none.

The design deliberately avoids a global serialized token format and keeps facade header and entity-tag
syntax outside the shared primitive. The new shared owning specification is a scoped follow-on
documentation decision, not an implementation edit in this packet.
