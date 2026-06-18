# 0001 - Overview and Architecture

**Status:** Accepted | **Version:** 0.2.0-draft | **Normative?** Mixed

This document defines the architectural map for Loom. It is normative for layer boundaries,
terminology, and ownership between specs. It is informative for motivation and examples. Later
specs are authoritative for their concrete contracts.

## Current Implementation

The workspace currently contains the Rust engine, a persistent `.loom` store, a C ABI, a CLI,
language binding directories, conformance crates, SQL support, compute support, and deterministic
substrates for several data facets. The core crate exports the object model, filesystem and VCS
engine, workspaces, direct and bundle sync, tabular helpers, CAS helpers, and facet helper modules.

The current source does not implement every surface described by the full architecture. Hosted
wire protocols, generated protocol schemas, OS mounting, complete binding parity, principal-aware
authorization, reactive triggers, observation feeds, and some enterprise facade contracts are target
work owned by later specs.

## 1. Abstract

Loom is a universal, content-addressed, versioned filesystem. It exposes a single engine that
behaves like a filesystem and a version-control repository over a Merkle object model. Backends store
canonical object bytes and mutable repository state, while the engine provides higher-level file,
history, workspace, and synchronization behavior.

The same logical repository can be backed by memory, a portable `.loom` file, or another conformant
provider. Synchronization transfers immutable objects and ref updates between compatible stores. Data
facets such as SQL, key-value, document, graph, vector, ledger, append-log, time-series, columnar, and
CAS build on the same object model when promoted by their own specs.

Compute, access control, hosted protocols, reactive triggers, observation feeds, encrypted sync, and
mounts are architectural extensions. They must integrate through the same layer boundaries, but they
do not become implemented product behavior until their owning specs and source are complete.

## 2. Motivation

Three capabilities are usually built separately:

1. File storage with read, write, list, and stat operations.
2. Versioning with commits, branches, tags, diffs, and merges.
3. Portable replication of both content and history.

Loom's thesis is that a content-addressed Merkle store is a strong substrate for all three. If the
interface is defined independently of the backend, then local files, a single portable file, a browser
backing store, or a remote service can preserve the same logical behavior. Synchronization can then
operate over the same canonical objects instead of inventing a separate replication model per backend.

## 3. Goals

| Goal | Contract | Implementation status |
| --- | --- | --- |
| G1 | One engine for filesystem and VCS behavior. | Implemented in `loom-core`; exact public interface is audited in 0003. |
| G2 | Pluggable storage behind the engine. | Implemented as the low-level `ObjectStore`; the broader provider contract is audited in 0004. |
| G3 | Uniform content addressing and canonical objects. | Implemented in source; exact digest and encoding rules are audited in 0002. |
| G4 | Synchronize compatible stores. | Direct workspace clone, branch push, and bundles exist; live transports are target work in 0006 and 0008. |
| G5 | One native core with language bindings. | C ABI and binding directories exist; parity and release status are audited in 0007. |
| G6 | Protocol projections from the same contract. | Target work in 0008 after 0003 and access-control decisions settle. |
| G7 | Security and integrity as first-class concerns. | Integrity and store encryption pieces exist; authorization and key-source policy are audited in 0009 and later specs. |
| G8 | Executable conformance. | Partial; coverage and promotion rules are audited in 0010 and 0025. |

## 4. Non-Goals

- Loom is not a POSIX-certified filesystem. It targets a POSIX-like application interface, not exact
  kernel filesystem semantics.
- Loom is not the `git` CLI and does not implement Git's object format or wire protocol.
- Loom does not require a specific database, cloud provider, or hosted service.
- Loom does not require a GUI, background daemon, or OS mount mechanism. Mounting is an optional
  projection owned by later specs.
- Real-time multi-writer collaborative editing is outside the core filesystem and VCS contract.

## 5. Glossary

0002 is authoritative for object-model terms. 0003 is authoritative for public interface terms.
This table is a convenience map for the architecture.

| Term | Definition |
| --- | --- |
| Object | Immutable, content-addressed storage unit. Concrete object types are defined by 0002. |
| Digest | Cryptographic content address written as `algo:hex`; exact algorithm policy is defined by 0002. |
| Object store | Low-level store for canonical object bytes. In Rust this is `ObjectStore`. |
| Working tree | Mutable workspace state used by filesystem operations before commit. |
| Reference store | Mutable branches, tags, and `HEAD` state for a workspace. |
| Commit | Snapshot object linking a tree to zero or more parent commits. |
| Ref | Named mutable pointer to a commit or tag target. |
| Workspace | Isolated repository space with its own refs and optional working tree. Defined by 0014. |
| Provider | Backend that hosts the storage and state needed by a Loom engine. Defined by 0004. |
| Capability | Negotiated optional behavior. Capability names and promotion rules are defined by 0010. |
| Facet | Data shape over the object model, such as files, SQL, KV, graph, vector, document, or CAS. |
| Facade | Public method surface used to access a facet or engine function, such as filesystem, VCS, sync, SQL, or CAS. |
| Principal | Authenticated actor used by access-control specs. Target work in 0026 through 0028. |

Older drafts may contain internal component aliases for the object store, working tree, reference
store, pack or segment, journal, or sync engine. Those aliases are not public API terms and must not
be used as production code names.

## 6. Architecture

Loom is defined as four observable layers. Implementations may fuse layers internally for performance
only when the public behavior is unchanged.

```text
+------------------------------------------------------------------------+
| L4  Projections        CLI, bindings, hosted protocols, mount           |
|                        Protocols and mount are target projections.      |
+------------------------------------------------------------------------+
| L3  ABI and bindings   C ABI plus per-language wrappers                 |
|                        Exact parity is owned by 0007.                  |
+------------------------------------------------------------------------+
| L2  Engine             filesystem, VCS, workspaces, sync, data facades  |
|                        Operates over canonical objects and refs.        |
+------------------------------------------------------------------------+
| L1  Provider/store     memory store, single-file store, other stores    |
|                        Stores canonical objects and repository state.   |
+------------------------------------------------------------------------+
| L0  Storage substrate  memory, OS file, browser backing, network media  |
+------------------------------------------------------------------------+
```

Access control, when enabled, is a cross-cutting policy decision at the public engine or served API
boundary, not a separate storage layer. Encrypted sync, when enabled, is a storage and transport
profile that must preserve the object model's identity rules. Reactive triggers and observation feeds
are host-driven projections over committed state; they must not change canonical object identity.

### 6.1 Reusable component boundaries

`loom-core` is the kernel crate. It owns the object model, digest and canonical codec integration,
provider-facing traits, workspace registry, `FacetKind`, ACL primitives, VCS behavior, synchronization
primitives, and small foundational substrates that are necessary for the engine to understand
versioned state.

Higher-level domains SHOULD become reusable component crates when they have their own public model,
growth path, conformance burden, platform profile, or dependency profile. External dependencies are a
signal, not the rule. A domain may justify extraction even when it has no heavy dependency if it is
reused by CLI, hosted protocols, FFI, MCP, bindings, compute, conformance, or other facets.

Reusable component crates preserve the kernel contract: committed source-of-truth bytes still use the
canonical object model, public behavior is still specified by the owning LCS document, and capability
reporting still distinguishes source-backed, target, degraded, unsupported, and platform-specific
behavior. Extracting a component MUST NOT turn an engine-specific presentation or derived artifact into
canonical source state.

Current reusable component crates and target extraction candidates are:

| Component | Target ownership |
| --- | --- |
| `loom-dataframe` | Dataframe plans, source bindings, adapters, portable executor, materialization, and dataframe conformance helpers. |
| `loom-polars` | Native Polars executor behind the dataframe contract; never default or wasm-required, and never canonical state. |
| `loom-vector` | Vector source model, predicates, exact/PQ behavior, and vector conformance helpers. Native accelerators and hosted compatibility profiles remain layers over it. |
| `loom-columnar` | Columnar manifest, segment/storage profile, Arrow/Parquet interchange, executor seams, and columnar conformance helpers. |
| `loom-tickets` | Source-backed ticket-domain component: canonical project, issue, workflow, board, sprint, planning, and operation-log models plus the persistent ticket service. MCP, hosted, CLI, interchange, and conformance consume it as projections. Project, prefix-route, issue, project-number, and operation storage use private incrementally-mutated tables. Derived ticket-key alias resolution and O(1) project re-key/released-prefix operations are source-backed; hosted, CLI, interchange, and broader ticket conformance remain target work. |
| `loom-pim` | Source-backed. Calendar, contacts, and mail local record models, codecs, and projection helpers as one PIM component. Hosted CalDAV, CardDAV, IMAP, and JMAP remain protocol projections. |
| `loom-watch` | Source-backed. Observation selectors, cursors, canonical event batches, file-domain records, unsupported-domain markers, domain support reporting, and watch conformance helpers over `loom-types`. `loom-core` consumes it for engine-integrated materialization, ACL filtering, and workspace history walks. |
| `loom-delivery` | Source-backed. Durable delivery envelope, canonical envelope codec, replay message shapes, produce request contract, and component-level delivery tests over `loom-types`. `loom-core` consumes it for CAS payload storage, queue-backed stream sequencing, subscriber ack mutation, ACL checks, and engine conformance. |
| `loom-triggers` | Source-backed for reusable trigger binding, fire-record contracts, croner-backed time evaluation, and keeper fire candidates. `loom-core` consumes it for reserved binding storage, due-fire planning, and fire-log append/history under the `program` facet. `loom-compute` provides source-backed run-as trigger execution and overlap handling. Hosted or binding projections and durable host keeper loops remain target work. |

`loom-compute` remains the execution subsystem. Trigger components invoke compute; they do not absorb
the compute engine or redefine program grants.

### 6.2 State classes (storage buckets)

Loom state falls into three classes that differ in *how they version* and *how they sync*. Keeping them
distinct prevents category errors (for example, putting security policy where a version-control revert
could roll it back).

- **Bucket 1 - Versioned workspace content.** The facet trees inside each workspace. Fully
  version-controlled: commit, branch, tag, checkout, merge, diff, revert, all scoped per workspace
  (invariant A4). Synced per workspace (clone, fast-forward push, bundle). This is the bulk of user
  data: files, SQL, KV, documents, time-series, ledgers, CAS, queues.

- **Bucket 2 - Store-global engine state (synced, not under workspace VCS).** Store-scoped state that is
  not any one workspace's content and is not subject to per-workspace commit/branch/checkout/revert: the
  workspace registry (which workspaces exist and their branch/tag tips), whole-store encryption
  metadata, and the principal + access-control control region (0026-0028). It travels with the store
  (clone, bundle, whole-file sync), but each sub-region reconciles by its own writer model: workspace
  refs reconcile per ref via the existing fast-forward rules (each replica that uses a workspace
  advances its tips), while the principal/ACL control region is single-authority and replicated
  fast-forward-only (see 0026/0027). A workspace VCS operation can never alter bucket-2 state - that is
  the property that keeps a `checkout` or `revert` from silently rolling back a revocation.

- **Bucket 3 - Authority-local operational metadata (not synced).** Per-store-instance runtime state
  that is intentionally never transferred by sync: queue consumer offsets (0021b) and lock fencing
  tokens and lease state (0036). Reads do not mutate committed trees; ordinary sync does not carry it.

A given capability must state which bucket its state lives in. Most facets are bucket 1; identity and
policy are bucket 2; coordination and consumer progress are bucket 3.

## 7. Architectural Invariants

- **A1 - Content addressing is canonical and profile-scoped.** Object identity is computed from the
  canonical bytes under the store's identity profile. 0002 is authoritative for algorithms,
  encodings, and cross-profile compatibility rules.
- **A2 - Engine behavior is backend-independent.** L2 behavior depends on the provider contract, not
  on concrete storage internals. Capability differences must be explicit.
- **A3 - Optional behavior is negotiated.** Optional features must be advertised, queried, and tested.
  Unsupported capabilities fail visibly instead of being silently ignored.
- **A4 - Workspace state is isolated.** Refs, `HEAD`, and the working tree are scoped to a workspace.
- **A5 - Target facades do not imply implementation.** A facade is product behavior only after its
  owning spec, public API surface, and conformance coverage are source-backed.
- **A6 - Committed state is unit-addressable and diffable.** Every facet stores its committed state
  keyed by its natural unit (a path, row, key, id, point, node, edge, entry - never an opaque whole-facet
  blob), so a commit-to-commit diff reports changes at that unit (added / removed / changed). An
  application or agent can therefore traverse any prior commit's state and the structural delta between
  any two commits. The per-facet diff unit and the uniform diff contract are defined in 0003b.
- **A7 - Units live in named collections.** Within a workspace, every facet's units are grouped into
  named **collections** - a uniform, possibly nested container concept (one level for kv/document/vector/
  time-series/queue/ledger; `database > table` for sql; unbounded nesting for files; `principal >
  collection` for calendar/contacts/mail). A collection is the single boundary for grouping inside a
  facet, for ACL scoping (0027), for projection (a collection may be exposed as a mount path, a DAV URL,
  or a dedicated port), and for diff roll-up; the unit address is `facet.<collection-path>.<unit>`.
  Collections are intra-workspace and intra-facet (they are not workspaces, which are the version-control
  boundary). The collection model is defined in 0042.

## 8. Worked Example

This example is illustrative target API shape. Current language bindings may expose a smaller or
different surface until 0007 is complete.

```ts
const loom = await Loom.open(provider);

await loom.fs.createDirectory("/docs");
await loom.fs.writeFile("/docs/readme.md", bytes);
const head = await loom.vcs.commit({ message: "init docs", author });

await loom.sync.push({ remote: "origin", ref: "branch/main" });
```

Given identical canonical objects and the same identity profile, the same logical object has the same
digest across conformant providers. Synchronization can therefore skip objects the receiver already
has and transfer only the missing reachable set.

## 9. Relationship to Prior Art

Loom borrows established ideas deliberately:

- Git's commit, tree, blob, branch, tag, diff, and merge concepts.
- Content-addressed storage systems such as IPFS, restic, bup, and OSTree.
- Prolly-tree approaches used by systems such as Noms and Dolt.
- Deterministic native cores wrapped by language bindings, as used by projects such as SQLite and
  tree-sitter.

Each later spec defines where Loom follows, narrows, or diverges from those systems.

## Resolved Decisions

1. **Layer fusion.** Conformance is defined by observable behavior at public boundaries. Internal
   layer fusion is allowed only when that behavior is preserved.
2. **Workspace cardinality.** Refs, `HEAD`, and the working tree are per workspace. A workspace may be
   bare.
3. **Digest policy ownership.** 0002 owns the exact digest algorithm policy. 0001 only requires
   canonical, deterministic, profile-scoped content addressing.
4. **Nested Loom entries.** Nested Loom traversal is an optional engine capability. A nested Loom is
   addressed by a root commit digest in the parent.
5. **Mount.** Mounting is an optional L4 projection. The concrete mechanism is outside the core
   architecture.
6. **CRDT scope.** Live collaborative CRDT behavior is not part of the core filesystem and VCS
   contract.
7. **Placeholder handling.** Architecture ideas that require substantial implementation must be owned
   by their later specs or split into a subspec before they can gate completion of 0001.
8. **Reusable component extraction.** Crate boundaries follow reusable domain ownership, not only
   external dependency weight. High-growth domains move out of `loom-core` when their public model,
   platform profile, conformance burden, or reuse across projections justifies it.
