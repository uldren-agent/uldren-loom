# 1000 - Deferred Work

This document holds ideas found during spec/source alignment that are intentionally outside the
current implementation pass. A deferred item is not rejected; it needs a separate contract decision,
migration plan, conformance vectors, and implementation work before it can become a gating
requirement.

## 0002 Deferred

The `0002` audit found several identity-affecting ideas that are useful but not part of the current
v1 data model. They are deferred because adding them changes canonical object bytes, digest vectors,
or cross-language object compatibility.

### (P2) Split Author and Committer

Current source has one commit author string plus `timestamp_ms`. Some VCS systems, including Git,
distinguish the person who authored content from the person or process that recorded the commit.

Example:

- `author`: Alice wrote the change on Monday.
- `committer`: CI imported or rebased the change on Tuesday.

Why it matters:

- Better provenance for imports, rebases, automated migrations, and service-written commits.
- More audit detail for enterprise environments.

Why it is deferred:

- Adding a committer field changes the canonical Commit object shape.
- Existing Commit digests and conformance vectors would change unless this is introduced through a
  new object epoch or migration.

### (P1) Tag Signatures

Current source has annotated Tag objects with `target`, `target_type`, `name`, `tagger`,
`timestamp_ms`, and `message`. It does not encode a detached signature field.

Example:

- A release tag could include a signature proving that an authorized release key approved the tag.

Why it matters:

- Useful for supply-chain verification, release attestations, and compliance workflows.

Why it is deferred:

- A signature field changes the canonical Tag object shape.
- Signature canonicalization, algorithms, key discovery, revocation, and verification errors need a
  security contract, likely owned with 0009 and the access-control specs.

### (P2) TreeEntry Metadata

Current TreeEntry identity includes `name`, `kind`, `target`, and `mode`. It does not encode a full
metadata table for `mtime`, size, xattrs, platform attributes, or executable convenience flags.

Example:

- A later profile might preserve xattrs or exact modification times for backup/restore use cases.

Why it matters:

- Improves fidelity for archival, backup, OS integration, and enterprise file workflows.

Why it is deferred:

- Volatile metadata such as `mtime` can destroy deduplication and deterministic identity if included
  by default.
- Metadata policy needs clear profiles: advisory metadata, identity-affecting metadata, or
  facet-specific metadata.
- Any identity-affecting metadata changes Tree digests and every Commit digest above them.

### (P3) Multihash Binary Digest Encoding

Current source stores object links and persisted engine state as fixed 32-byte digest slots, with the
algorithm supplied by the enclosing store, superblock, bundle header, or parsing context. Older spec
text described a multihash/uvarint binary form.

Example:

- Multihash style: `[algorithm code, digest length, digest bytes]`.
- Current v1 style: `[32 digest bytes]`, with algorithm known from the identity profile.

Why it matters:

- Self-describing links can be useful for multi-algorithm stores or content exchanged outside a Loom
  identity profile.

Why it is deferred:

- The current store model has exactly one identity profile per store.
- Switching object links to multihash changes canonical object bytes and all vectors.
- It also complicates cross-profile equality, ordering, storage indexes, and ABI shapes.

### (P2) Additional Identity Profiles

Current source supports the default `blake3` profile and the FIPS `sha256` profile. Additional
profiles such as SHA3-256 or a post-quantum-oriented hash are not implemented.

Why it matters:

- Some deployments may require a different approved hash suite.
- Long-lived archives may need an algorithm migration path.

Why it is deferred:

- Each profile needs conformance vectors, store creation support, sync negotiation behavior, and
  migration rules.
- Cross-profile sync is intentionally rejected today instead of silently rehashing.

### Recommended Promotion Path

If any deferred `0002` item becomes required, promote it through a focused subspec such as
`0002a-identity-extensions.md`. That subspec should define:

- the new canonical fields or binary encoding;
- whether the change requires a new object epoch;
- migration from existing objects;
- conformance vectors for every affected object type and identity profile;
- ABI and binding exposure;
- compatibility and sync behavior across old and new stores.

## 0014 Deferred

The `0014` review separated workspace identity from data facets and chose one canonical workspace
tree. User files occupy `/`. Loom-reserved data lives under `/.loom`, with promoted facets under
`/.loom/facets/<facet>/...`.

### (P1) Facet Interoperability

Promoted to `0014a-facet-interoperability.md`.

## 0013 Deferred

The `0013` review converted the extended-capabilities document from an open-question idea list into a
source-backed facet catalog. The items below remain useful, but they are not v1 gates until promoted
through focused specs with source, conformance, and access-control decisions.

### (P3) RDF / SPARQL Facet

RDF/SPARQL was listed as a possible semantic-web facet using engines such as Oxigraph or Sophia. It is
not present in `FacetKind`, has no owning spec, and has no source-backed substrate.

Example:

- A knowledge-management deployment might want RDF triples and SPARQL queries over semantic metadata.

Why it matters:

- RDF/SPARQL is a mature ecosystem for linked-data use cases.
- It can model a subset of graph relationships, but its query semantics and storage requirements are
  distinct from the property-graph facet.

Why it is deferred:

- The current v1 graph and vector specs already cover the primary AI-memory use cases.
- RDF engine storage would need to be adapted to Loom's content-addressed object model before commits,
  diff, merge, and sync could see the data.
- A promoted RDF facet would need its own path layout, query contract, merge policy, conformance
  vectors, and binding/wire projection.

### (P3) Unified Graph / Vector / Datalog Engine

Cozo or a similar engine could eventually provide graph, vector, and Datalog-style query behavior in
one dependency. Current source keeps graph and vector as separate facets, with exact vector search as
the portable contract and graph traversal as a separate substrate.

Example:

- An agent-memory store might want semantic recall, relationship traversal, and recursive Datalog
  queries over the same notes.

Why it matters:

- A unified engine could simplify higher-level knowledge workloads.
- It may reduce duplicated indexing and query planning across graph and vector surfaces.

Why it is deferred:

- Engines such as Cozo bring their own storage assumptions. A side store would be invisible to Loom
  commits and sync.
- A Loom-backed storage adapter must be proven before the engine can become a normative dependency.
- Graph and vector already have separate owning specs and source-backed substrates.

### (P2) Foreign Protocol Adapters

S3-compatible service, Postgres-wire service, GraphQL, and MCP server projections are useful adapter
ideas. They are not core facets and are not current source-backed public contracts.

Example:

- An S3 endpoint could expose file or CAS content to existing S3 SDKs.
- A Postgres-wire endpoint could let `psql` query promoted SQL tables.
- An MCP server could expose Loom resources and tools to AI agents.

Why it matters:

- Enterprise adoption often depends on existing client protocols.
- Adapters can make one `.loom` usable by tools that do not know Loom's native API.

Why it is deferred:

- Served write paths need principal identity, ACL, and capability policy decisions from 0026-0028.
- Protocol schemas, transaction behavior, error mapping, and conformance belong in 0008 or focused
  adapter specs.

MCP status update: the MCP serving surface is now specified in 0008 section 9 with RD10 (hand-written,
agent-curated; REST and gRPC gated on a true IDL-codegen prerequisite track) and RD11 (a passwordless
default loom serves stdio and Streamable HTTP in owner mode with full read and write now; OAuth 2.1
enforcement is a phased follow-on once a loom configures authentication - this supersedes the earlier
"read-only by default" posture). Two MCP primitives remain deferred and are recorded here:

- **(P2) MCP Sampling (`sampling/createMessage`).** Server-initiated host-LLM completions. Gated on
  the GraphRAG layer (0040) and the program facet (0015), which define the server-side workloads
  (record summarization, program-shaped mail filters per 0041 section 8) that would use it.
- **(P3) MCP Tasks (experimental).** Durable execution wrappers for deferred results and status.
  Gated on the primitive stabilizing in the MCP spec; when promoted, Loom backs task state with the
  ledger (0018), append-log/queue (0021), and durable delivery (0035).
- **(P3, optional) IDL-codegen track (consolidation, not a prerequisite).** Per RD10 (revised), MCP,
  REST, and gRPC are hand-written and guarded by a mandatory drift/coverage conformance layer (coverage
  of every facet operation plus golden-contract schema stability), which is the anti-drift boundary. A
  codegen track that emits OpenAPI/protobuf/JSON-RPC from an enriched IDL and subsumes the hand-written
  bindings is an optional later consolidation; it does not gate any served protocol.

### (P1) Concurrent Protocol Adapters

Serving REST, GraphQL, S3, SQL-wire, MCP, and FUSE against one `.loom` at the same time raises a
composition issue separate from any one adapter's resource mapping.

Example:

- An MCP write and an S3 PUT arrive at the same time against one `.loom`.
- A SQL-wire transaction and a REST file upload both want to advance the same workspace branch.

Why it matters:

- The single-file store has a single-writer discipline.
- Cross-adapter transactions and branch advancement need one coherent locking and ref-update model.
- Without a shared serving model, adapters could produce surprising ordering, lock contention, or
  inconsistent authorization checks.

Why it is deferred:

- No served adapter is source-backed today.
- Write surfaces depend on principal identity, ACL, and fine-grained grant decisions.
- The likely enterprise posture is one shared engine instance with a single serialized writer, while
  read-only adapters may be isolated. That needs a focused server/runtime spec before it becomes a
  contract.

### (P2) FUSE Mount and REPL

FUSE and an interactive REPL are useful consumers of the public interface, but they are not core data
contracts.

Example:

- A FUSE mount could expose the files facet as a normal folder.
- A REPL could drive workspace, filesystem, VCS, and SQL operations interactively.

Why it matters:

- These surfaces improve usability and inspection.
- FUSE especially makes the filesystem facet available to unmodified applications.

Why it is deferred:

- Non-files facet projection depends on 0014a and each owning facet's path semantics.
- FUSE write behavior needs OS-specific locking, watcher, and crash-recovery rules.
- A REPL should follow the promoted CLI and IDL surface rather than inventing its own interface.

### (P2) Binding Comment Quality Cleanup

Some checked-in binding files were already dirty during the 0026-0028 queue and still need a focused
comment-quality pass against AGENTS.md before release cleanup.

Example:

- Remove comments that restate signatures, describe previous changes, or preserve draft-process notes.
- Keep comments only when they explain ownership, memory safety, generated-code constraints, or a
  non-obvious binding invariant.

Why it matters:

- Binding files are public examples for downstream users and language maintainers.
- Comment drift can hide stale ABI assumptions even when the wrapper still compiles.

Why it is deferred:

- It is not a behavior or interface blocker for 0026-0028.
- It should be done as a mechanical review after active binding feature work settles, so the cleanup
  does not churn the same files repeatedly.
