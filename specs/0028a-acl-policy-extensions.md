# 0028a - ACL Policy Extensions

**Status:** Draft target extension. **Version:** 0.1.0. **Normative target.**

This document owns fine-grained ACL work that is intentionally outside the main 0028 dependency gate.
0028 defines the grant axes: workspace, ref glob, prefix scope, authorization domain, `Merge`, and `Exec`. This
extension tracks predicates, conditional policy, and attenuable sessions.

## Current source boundary

Current source implements principal ACLs, scoped grants, hosted PEP checks, and `Exec` projection in
0027 and 0028. Current source also stores optional ACL predicate metadata on grants using the CEL
language tag, bounds predicate expressions to 4096 bytes, persists the grant codec using the final
non-versioned `LACL` discriminator, projects
predicates through local/C ABI/admin JSON and IDL grant records, and fails closed when a stored
predicate reaches the current PEP without an evaluator. Current source defines a stable
`AclEvaluationContext` for PEP-time evaluation, lets `Loom` install a runtime ACL predicate evaluator,
passes the evaluator through the central engine PEP, and source-backs a deterministic CEL evaluator in
`loom-compute` behind the `guards` feature. Current source installs that evaluator in the `loom` CLI,
the hosted kernel, the C ABI backed runtime path, and the direct Node, Python, and WASM binding
runtimes. The CEL context exposes principal id, roles, workspace, domain, right, ref name, scope kind,
scope hex, and scope text.

Current source does not implement row policy, column policy, automatic predicate conformance across
every binding package, attenuable token sessions, or public policy-language projection beyond CEL
grant metadata. Public predicate authoring is source-backed for the CLI (`--predicate-cel`), hosted
admin REST and JSON-RPC nested predicate objects, IDL `AclPredicateRecord`, C ABI
`loom_acl_grant_scoped_predicate` and `loom_acl_revoke_scoped_predicate`, Node, Python, WASM, C++,
Swift/iOS, JVM, Android, and React Native. MCP does not expose ACL grant authoring today.

## Public Predicate Authoring Boundary

Public predicate authoring is partially source-backed for v1 hardening. The durable grant format can
store CEL predicate metadata, promoted runtimes can evaluate stored CEL predicates through the
central PEP, and the first public authoring surfaces now accept CEL predicate expressions.

Promotion requires:

- (P0) A strict CEL profile that names allowed variables, allowed functions, size limits, cost limits,
  and fail-closed error mapping.
- (P1) Cross-binding conformance vectors for matching, denial, malformed expressions, unsupported
  functions, and missing context.
- (P1) Documentation that distinguishes stored internal predicate metadata from public authoring.

Until those items are complete, predicates remain a partial source-backed capability: storage,
projection, runtime installation, central evaluation, public creation, exact-revoke surfaces, and
promoted binding parity are source-backed; a strict CEL execution profile and cross-binding
conformance remain target work.

## Row And Column Policy Prerequisite

Row-level and column-level ACL policy must use one shared cross-facet resource identity model. Loom
must not invent a separate policy coordinate system for each facet because that would make ACL
semantics inconsistent across SQL, columnar, dataframe, search, vector, graph, document, and future
facets.

The shared model is `ResourceIdentity`. It is a policy coordinate, not a new storage layer:

```
ResourceIdentity {
  workspace: WorkspaceId,
  facet: FacetKind,
  ref_name: Option<RefName>,
  collection: Option<CollectionName>,
  kind: ResourceIdentityKind,
  coordinate: CanonicalCbor,
  provenance: ResourceIdentityProvenance,
}
```

`coordinate` is facet-specific but always canonical CBOR. Textual APIs may render it as JSON or hex,
but stored policy and conformance vectors use canonical bytes. The identity never points at an engine
cache, index posting, result handle, or UI row number.

`ResourceIdentityKind` has these target values:

- `collection`: a facet collection or table, equivalent to today's typed prefix scope.
- `record`: a committed logical row, key, object, document, message, event, vector, node, edge, or
  segment entry.
- `field`: a named column, field, property, vector dimension range, or document attribute.
- `cell`: a record plus field pair.
- `projection`: a derived row, aggregate row, search hit, dataframe row, or materialized view row that
  can be traced back to one or more canonical records.

`ResourceIdentityProvenance` has these target values:

- `canonical`: committed source-of-truth state. A durable grant or predicate may target this.
- `derived`: reproducible from committed state. A result may expose it, but a durable grant must either
  map it back to canonical inputs or explicitly state the derived materialization digest it depends on.
- `runtime`: a cursor, result handle, stream offset, task id, or temporary adapter row. It may be
  audited, but it is never a durable ACL target.

Canonical coordinates by facet:

| Facet family | Canonical record coordinate | Canonical field coordinate | Derived/runtime rule |
| --- | --- | --- | --- |
| SQL/tabular | `[table_name, primary_key_tuple]`; tables without a stable primary key are not eligible for durable row ACLs. | `[table_name, column_name]` | Query result row ordinals are runtime-only. Views and joins are `projection` identities that must map back to base tables and primary keys before enforcement. |
| KV | `[collection, canonical_key_bytes]` | none unless a value schema declares named fields | Value-path predicates are derived unless the value schema is committed and versioned. |
| Document/mail/calendar/contacts | `[collection, document_id_or_message_id]` | `[collection, field_name]` for committed fields defined by the facet spec | Parsed headers, recurrence expansions, and mailbox/search views are projections over the canonical item. |
| Queue/time-series/ledger | `[collection, sequence_or_event_id]` | `[collection, field_name]` when the record schema is committed | Consumer positions, replay cursors, and retention windows are runtime or management identities, not row ACL keys. |
| Graph | `[collection, node_id]` or `[collection, edge_id]` | `[collection, property_name]` | Traversal result positions are projections over node and edge identities. |
| Vector/search | `[collection, vector_id]` or `[collection, indexed_source_id]` | `[collection, metadata_field]` | Similarity rank, score, posting id, and search hit offset are projections. Grants target the vector/source id or metadata field, not the ranking artifact. |
| Columnar/dataframe | A declared logical primary key when present; otherwise `[manifest_digest, segment_id, row_id]` only when the segment profile declares stable row ids. | `[dataset, column_name]` | Row ordinals, row groups, scan batches, lazy-frame rows, and native-engine chunk ids are derived/runtime unless materialized with a committed stable row id. |

Merge, branch, rebase, and compaction rules:

- A `canonical` record identity survives a merge when the same logical key survives in the merged
  state. If both sides edit the same key, the merge policy decides the value, but the identity remains
  the same.
- A `canonical` field identity survives schema changes only when the field name and declared semantic
  type remain compatible. Rename is a delete plus create unless the owning facet defines a committed
  field-id map.
- A `derived` projection identity is invalidated when its materialization digest changes, unless it can
  be remapped to the same canonical record set.
- A `runtime` identity expires with its cursor, handle, task, or session and cannot appear in stored
  grants.
- Compaction must preserve canonical coordinates or emit a remap record before row or column ACLs can
  be promoted for that facet.

The shared model still requires implementation work:

- (P1) How a facet names a stable row, record, key, cell, column, field, vector, edge, segment, or
  derived row projection without turning a cache or presentation artifact into canonical state. The
  target categories above are settled; each owning facet must bind its public IDs to them.
- (P1) How resource identities appear in `AclEvaluationContext`, audit logs, hosted protocol errors,
  conformance reports, and binding result envelopes.
- (P1) How merged, branched, rebased, and compacted data preserves or invalidates row and column
  identities.

The long-term enterprise answer is to finish that shared identity model before promoting row or
column predicates. Per-facet shortcuts are rejected because they would be harder to audit, harder to
test, and harder to preserve across hosted protocols and bindings.

## Target predicate track

- (P1) Bind each owning data-facet spec to the `ResourceIdentity` categories before promoting
  row-level or column-level ACL authoring for that facet.
- (P1) Project `ResourceIdentity` through `AclEvaluationContext`, audit logs, hosted protocol errors,
  conformance reports, and binding result envelopes.
- (P1) Add cross-binding predicate conformance that proves the promoted runtimes use the installed CEL
  evaluator consistently.
- (P0) Keep predicate evaluation fail-closed on parse error, missing input, unsupported function, or
  evaluation error. Source now backs this behavior for the engine evaluator path.
- (P0) Bound evaluation cost and available functions before any public API accepts a predicate. Source
  already bounds stored predicate expression length to 4096 bytes.
- (P1) Add conformance for matching, denial, malformed predicates, cost limits, and cross-binding
  error mapping.

## Target attenuable-session track

Attenuable sessions are an optional hosted optimization, not a replacement authorization model. A
token may narrow a principal's live authority for a specific client, route, or continuation, but stored
grants remain authoritative.

Target `AttenuationToken` fields:

```
AttenuationToken {
  token_id: Uuid,
  loom_id: Uuid,
  principal: PrincipalId,
  session_family: Uuid,
  audience: string,
  issued_at_ms: u64,
  expires_at_ms: u64,
  grant_version: bytes,
  max_rights: Set<AclRight>,
  max_resources: List<ResourceIdentity | AclScope>,
  parent_token_id: Option<Uuid>,
  attenuation_depth: u8,
  replay: ReplayPolicy,
}
```

Rules:

- (P0) Every token use is re-checked against live stored grants. The token is an upper bound only:
  effective rights are `live_acl(principal, resource) ∩ token_bounds`. If the stored grant is revoked,
  the next token use fails.
- (P0) `loom_id`, `principal`, `session_family`, `audience`, `expires_at_ms`, and `grant_version` are
  mandatory. A token presented to another Loom, another audience, an expired time, or an incompatible
  grant version fails closed.
- (P0) A child token can only attenuate. It may reduce rights, narrow resources, shorten expiry, or
  reduce replay allowance. It cannot widen rights, add resources, change principal, change Loom, or
  outlive its parent.
- (P1) `attenuation_depth` is bounded. The target maximum is 8 unless conformance proves a larger value
  has acceptable verification cost.
- (P1) Token size is bounded. The target maximum serialized token size is 8 KiB for wire transport and
  4 KiB for header transport. Larger continuations use served result handles or durable delivery
  cursors, not bearer tokens.
- (P1) `ReplayPolicy` is one of `bearer`, `nonce-bound`, or `single-use`. Hosted write paths should
  prefer `nonce-bound` or `single-use`; read continuations may use `bearer` only over authenticated TLS
  or a local trusted transport.
- (P1) Tokens that name row or column resources use the `ResourceIdentity` model above. Tokens that
  only name prefix scopes may use today's `AclScope`.
- (P2) A future portable token format such as Biscuit may be evaluated only after the stored ACL model,
  resource identity model, and live re-check conformance are source-backed. Until then, an opaque
  server-side token id is the safer implementation shape.

Conformance requirements before promotion:

- token cannot grant a right absent from live ACL;
- token fails immediately after live grant revocation;
- child token cannot widen parent authority;
- audience, Loom id, session family, expiry, and grant-version mismatch fail closed;
- replay policy is enforced consistently across REST, JSON-RPC, gRPC, MCP, and durable delivery
  continuations.

## Sequencing

1. (P0) Implement 0026 and 0027 first.
2. (P1) Implement 0028 prefix/ref/facet fine-grained ACLs before predicate evaluation.
3. (P1) Promote row or column predicates only after 0011 and the owning data-facet specs define stable
   row, key, and column resource references.
4. (P2) Promote attenuable sessions only after hosted authentication and transport replay semantics are
   settled in 0008 and 0035, and only with live ACL re-check conformance.

## Resolved decisions

1. **Stored grants remain authoritative.** Tokens, if added, are never the source of truth for
   authorization.
2. **Predicates fail closed.** A predicate that cannot be evaluated denies the operation. Current
   source applies that rule by denying predicate-gated allows until a PEP evaluator is promoted.
3. **No broad policy language in the main ACL gate.** The main 0028 contract stops at deterministic
   prefix, ref, and facet matching.
4. **Tokens are attenuation, not authority.** Any promoted token is an upper bound over live stored
   grants. Revocation in the stored ACL must affect the next token use.
