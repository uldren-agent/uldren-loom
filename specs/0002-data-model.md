# 0002 - Data Model

**Status:** Accepted | **Version:** 0.2.0-draft | **Normative.**

This document defines Loom object identity: digest profiles, canonical object bytes, object types,
directory sharding, reachability, and the boundary between identity and storage transforms. Source
code and conformance vectors are authoritative for the current v1 object shape.

## Current Implementation

`loom-core` implements:

- profile-scoped digests with `blake3` as the default profile and `sha256` as the FIPS profile;
- five canonical object types: Blob, ChunkList, Tree, Commit, and Tag;
- strict Loom Canonical CBOR v1 object framing through `loom-codec`;
- deterministic FastCDC file chunking below file identity;
- deterministic prolly-sharded large directories;
- workspace-scoped refs, branches, tags, and `HEAD` through the workspace and VCS engine;
- source-backed conformance vectors for default-profile blobs, object-model objects, table identity,
  and profiled blob/object digests.

The current canonical object shape does not include a split author/committer identity, TreeEntry
metadata table, tag signature field, multihash binary digest encoding, or signed object envelope.
Those are identity-affecting extensions and must be specified as explicit migrations or subspecs
before implementation.

## 1. Model

A Loom repository has:

1. an immutable object store keyed by object digest;
2. mutable workspace state containing refs, `HEAD`, staged file/table content, and a content-address
   index for file payloads;
3. zero or more workspaces, each with isolated refs and optional working tree state.

Objects are immutable. Mutations create new objects and move workspace refs or staged state. Every
provider must preserve the same canonical object bytes and identity-profile digest behavior for
objects it stores.

## 2. Digests

### 2.1 Text Form

A digest is displayed as:

```text
algo:lowercase-hex
```

The supported v1 algorithms are:

| Algorithm | Code | Profile | Output |
| --- | ---: | --- | --- |
| `blake3` | `0x1e` | default | 32 bytes |
| `sha256` | `0x12` | FIPS | 32 bytes |

Unknown algorithm tags or codes are rejected as unsupported.

### 2.2 Identity Profiles

Every store has exactly one identity profile, chosen at creation and immutable for that store. The
profile determines which hash function addresses every object and file-content digest in that store.

- Default profile: BLAKE3-256.
- FIPS profile: SHA-256.

Stores with different identity profiles are not directly synchronizable. Sync rejects profile
mismatches instead of silently rehashing objects. Cross-profile conversion is an explicit migration
operation outside this spec.

### 2.3 Binary Form

Canonical object links and persisted engine state store digest bytes in fixed 32-byte slots. The
algorithm is supplied by the enclosing store, superblock, bundle header, or parsing context. This is
the source-backed binary contract for v1.

The prior multihash/uvarint form is not implemented and is not part of the v1 data model.

### 2.4 What Is Hashed

An object digest is the identity-profile hash of the object's canonical serialized bytes. The
canonical bytes include the object epoch, object type, and object fields, so type is bound into the
address.

A file-content digest is the identity-profile hash of the raw full file payload. Tree entries for
ordinary files point at this file-content digest, not at the Blob or ChunkList object's digest. The
engine keeps a derived content-address to object-address index so it can resolve file-content digests
to stored Blob or ChunkList objects.

## 3. Canonical Serialization

Every object has exactly one canonical byte form. The canonical form is Loom Canonical CBOR v1:

```text
[epoch, type, ...fields]
```

Rules:

- `epoch` is `1`.
- `type` is the object type code.
- Fields are positional and type-specific.
- CBOR encoding is strict: definite lengths, shortest-form integers and lengths, sorted and
  duplicate-free map keys, no tags, no indefinite items, no trailing bytes, and no non-finite floats.
- Decode rejects non-canonical input instead of normalizing it.
- `decode(canonical(obj)) == obj`.
- `canonical(decode(bytes)) == bytes`.

## 4. Object Types

There are exactly five framed object types in v1.

| Code | Type | Purpose |
| ---: | --- | --- |
| `0x01` | Blob | Opaque bytes. |
| `0x02` | ChunkList | Ordered list of chunk Blob or nested ChunkList references. |
| `0x03` | Tree | Directory or shard node. |
| `0x04` | Commit | Snapshot plus parent links and authorship metadata. |
| `0x05` | Tag | Annotated pointer to another object. |

### 4.1 Blob

Fields:

| Field | Type | Notes |
| --- | --- | --- |
| `payload` | bytes | Opaque bytes. Compression and encryption are storage-layer transforms, not object fields. |

### 4.2 ChunkList

Fields:

| Field | Type | Notes |
| --- | --- | --- |
| `total_size` | u64 | Logical byte length of the assembled payload. |
| `entries` | array of `[Digest, u64]` | Ordered chunk object digest and chunk length pairs. |

Chunking is below file identity. The current engine uses deterministic FastCDC with minimum 2 KiB,
target 8 KiB, maximum 64 KiB, and chunking threshold 64 KiB. The file-content digest remains the hash
of the whole raw payload under the store identity profile.

### 4.3 Tree

Fields:

| Field | Type | Notes |
| --- | --- | --- |
| `entries` | array of TreeEntry | Serialized in ascending raw UTF-8 name-byte order. |

TreeEntry fields:

| Field | Type | Notes |
| --- | --- | --- |
| `name` | string | Unique within the Tree. The current source rejects duplicate names and serializes by raw UTF-8 byte order. |
| `kind` | u8 | Entry kind code. |
| `target` | Digest | File-content digest for Blob and Symlink entries; object digest or prolly root digest for other kinds. |
| `mode` | u32 | POSIX-style mode bits. |

Entry kind codes:

| Code | Kind | Target |
| ---: | --- | --- |
| `0x01` | Tree | Tree object digest. |
| `0x02` | Blob | File-content digest. |
| `0x03` | Symlink | File-content digest of a Blob containing the link target path. |
| `0x04` | Subloom | Nested Loom root commit digest. |
| `0x05` | TreeShard | Child shard Tree object digest. |
| `0x06` | Table | Table Tree object digest, specified by 0011. |
| `0x07` | ProllyMap | Prolly-tree root digest used inside table Trees. |

No additional metadata field is encoded in v1 TreeEntry. Volatile metadata such as mtime, xattrs,
and size is outside default object identity.

### 4.4 Commit

Fields:

| Field | Type | Notes |
| --- | --- | --- |
| `tree` | Digest | Root Tree object digest. |
| `parents` | array of Digest | Empty for root commits, one for ordinary commits, more than one for merges. Order is significant. |
| `author` | string | Source-backed author identity string. |
| `timestamp_ms` | u64 | Authoring time in milliseconds since Unix epoch. |
| `message` | string | Commit message. |
| `meta` | map of string to string | Ordered metadata map. |

The source-backed v1 Commit does not split author and committer. Adding that split changes canonical
bytes and requires a migration or new object epoch.

### 4.5 Tag

Fields:

| Field | Type | Notes |
| --- | --- | --- |
| `target` | Digest | Tagged object digest. |
| `target_type` | u8 | Tagged object type. |
| `name` | string | Advisory tag name inside the object. The ref name is authoritative for lookup. |
| `tagger` | string | Source-backed tagger identity string. |
| `timestamp_ms` | u64 | Tagging time in milliseconds since Unix epoch. |
| `message` | string | Annotation. |

The source-backed v1 Tag does not include a detached signature field. Signed tags are an
identity-affecting extension and must be specified separately before implementation.

## 5. Large Directories

A directory with more than `DIR_SHARD_THRESHOLD = 256` entries is stored as a prolly-sharded Tree.
Shard nodes are ordinary Tree objects:

- leaf shard nodes hold ordinary entries;
- interior shard nodes hold only TreeShard entries;
- a TreeShard entry's `name` is the maximum entry name in the child subtree;
- a TreeShard entry's `target` is the child shard Tree object digest.

The boundary function hashes `name || level` with BLAKE3 for structural chunking. This boundary hash
does not address objects and does not change with the store identity profile. The shard node digests
still use the store identity profile because shard nodes are Tree objects.

Directory sharding is transparent to callers. Directory listing, checkout, diff, reachability, and
sync must descend shard nodes.

## 6. References and Commit DAG

Refs are mutable workspace state. A branch maps a branch name to a Commit digest. A tag maps a tag
name to a target digest. `HEAD` is attached to a branch in current source.

Commits form a directed acyclic graph through parent links. The engine computes merge-base sets and
uses recursive base reduction for crisscross merges. Rebase, squash, amend, and cherry-pick style
operations create new commits and move refs; they do not mutate existing commits.

Detailed workspace lifecycle and ref isolation are specified by 0014. Public VCS operations are
specified by 0003. Provider durability and compare-and-swap requirements are specified by 0004 and
0005.

## 7. Reachability and Storage Transforms

An object is reachable if it is referenced by workspace state or transitively by another reachable
object. Reachability includes:

- Commit parent links;
- Commit root Tree links;
- Tree, Subloom, TreeShard, and Table entry links;
- Blob and Symlink content-address links resolved through the content index;
- ProllyMap roots and their reachable nodes.

Compression and encryption happen below identity. `put` and `get` speak plaintext canonical object
bytes. Stores may compress or encrypt records, but object digests are still computed over plaintext
canonical bytes under the store identity profile.

## 8. Identity Stability Checklist

For two stores to produce identical object addresses for identical logical content, these values must
match:

1. identity profile;
2. Loom Canonical CBOR epoch and profile;
3. canonical object field shape;
4. directory sharding boundary function and threshold;
5. TreeEntry mode normalization rules.

File chunking affects stored chunk objects and transfer deduplication, but ordinary file Tree entries
are addressed by whole-file content digest, so a caller's file identity is not the ChunkList object's
digest.

## 9. Conformance

The v1 data model is pinned by executable vectors for:

- Blob canonical bytes and default/FIPS digests;
- empty Tree and empty ChunkList canonical bytes and default/FIPS digests;
- sample Tree, Commit, and Tag canonical bytes and default/FIPS digests;
- strict decode and canonical round trips;
- table Tree identity vectors for the tabular substrate;
- ledger chain-head vectors under both identity profiles.

Any change to object fields, canonical encoding, digest profiles, or identity-affecting metadata must
update conformance vectors in the same change.

## Deferred Extensions

These ideas are not part of the current v1 canonical object shape:

- split author and committer identities;
- detached tag signatures or signed object envelopes;
- a TreeEntry metadata field;
- multihash/uvarint binary digest encoding;
- alternate identity profiles beyond `blake3` and `sha256`;
- generic GC, reflog, and durable pin policy beyond current source behavior.

If any of these are promoted, they should be handled as explicit implementation work. Identity-shape
changes should be split into a `0002a`-style subspec or a new object epoch so the main data model does
not become a blocker for unrelated source-backed behavior.

## Resolved Decisions

1. **Digest profiles.** v1 supports a default BLAKE3 profile and a FIPS SHA-256 profile. A store has
   one immutable identity profile.
2. **No silent cross-profile sync.** Direct sync and bundle import reject profile mismatches.
3. **Canonical object framing.** Object identity is the identity-profile hash of Loom Canonical CBOR
   v1 bytes `[epoch, type, ...fields]`.
4. **File content identity.** File Tree entries use whole-file content digests. Chunking is below
   caller-visible file identity.
5. **Directory sharding.** Large directories use TreeShard entries and fixed deterministic boundary
   rules.
6. **Current Commit shape.** Commit identity uses `tree`, `parents`, `author`, `timestamp_ms`,
   `message`, and `meta`.
7. **Current Tag shape.** Tag identity uses `target`, `target_type`, `name`, `tagger`,
   `timestamp_ms`, and `message`. The tag ref name is authoritative for lookup.
