# P9-0003 - `files` Binding

**Series:** P9 binding plan (normative-track sub-series; Draft)
**Version:** 0.1.0-draft - **Status:** Draft - **Last updated:** 2026-07-02
**Reads first:** [`P9-0002-projection-conventions.md`](./P9-0002-projection-conventions.md), facade spec
**0003 section 4** (filesystem), **0008 section 3.2** (the REST mapping already exists), **0013 section C** (FUSE), **0014**
(mountable workspace types), **0032** (parity).

The `files` facet is the **best-grounded** binding: 0008 section 3.2 already defines its REST projection
normatively, and the facade is built in `loom-core` (the `fs` ops on `Loom<S>`). Hosted `files/rest` and
`files/json_rpc`, and native `files/grpc` now have source-backed listener-scoped subsets for whole-file
read/write, stat, list, create-directory, and delete through durable `loom serve` listener records. This
doc records that projection in the per-facet template and adds the Tier-2 consumers. **FUSE mode
semantics are resolved as read-only commit mounts plus read-write tip mounts with explicit commit
(section 8 RD-F1).**

## 1. Facade surface (0003 section 4)

Methods (built in `loom-core`, exposed on `Loom<S>` / `fs`): `read_file`, `write_file`, `create_file`,
`append_file`, `delete_file`, `stat -> Stat{kind,size,mode}`, `exists`, `list_directory -> [DirEntry{name,kind}]`,
`walk`, `create_directory`, `remove_directory`, `move`, `copy`. Working-tree (Weft) backed; a bare Loom
with no working tree returns `NO_WORKING_TREE`.

Build status: directory ops, `stat`, `list_directory`, `move`, `copy`, `walk`, file read/write,
`append_file`, byte-range I/O, file handles, and symlink create/read-link are built in the local core
facade. Hosted REST, JSON-RPC, and native gRPC currently cover the listener-scoped whole-file subset;
hosted append, range, file-handle, symlink, move, copy, broader streaming, generated protobuf artifacts,
and S3-backed path materialization remain target.

### 1.1 Binding Boundary

The base layer is the Loom working tree and committed tree model. Native projections are REST,
JSON-RPC, gRPC, and MCP over the `fs` facade. FUSE is a presentation over that base layer. Archive
formats are interchange, not the source of truth. The first-class `s3` served surface may use file-tree
materialization internally for path-like objects, but S3 bucket lifecycle, object metadata, multipart
state, and conditional-write behavior belong to the S3 surface rather than the base `files` contract.
FUSE mount caches and OS metadata projections are derived runtime state and must not change canonical
tree identity.

## 2. Tier-1 - REST (0008 section 3.2 - already normative)

Facet-root: `/v1/workspaces/{workspace_id}/tree/{path}` (the `ns` segment per P9-0002 section 2 / OQ4). The mapping is
0008 section 3.2 verbatim; reproduced for convenience:

| Facade method | HTTP |
| --- | --- |
| `read_file` | `GET /tree/{path}` (`Accept: application/octet-stream`); `Range:` -> `206` |
| `stat` | `HEAD /tree/{path}` or `GET ...?stat=1` |
| `list_directory` | `GET /tree/{path}?list=1` (NDJSON or paged) |
| `create_file` | `POST /tree/{path}` (`If-None-Match: *` to fail-if-exists) |
| `write_file` | `PUT /tree/{path}` (full replace) |
| `append_file` *(unbuilt)* | `PATCH /tree/{path}` (`Content-Range` append) |
| `delete_file` / `remove_directory` | `DELETE /tree/{path}` (`?recursive=1` for dirs) |
| `create_directory` | `PUT /tree/{path}/` (trailing slash; `?recursive=1`) |
| `move` / `copy` | `POST /tree/{path}:move` / `:copy` `{ dst, overwrite }` |

`GET /tree/{path}` returns a content-digest `ETag` for cheap `304`s (P9-0002 section 2; 0008 section 3.4).

Current source-backed hosted files REST is listener-scoped to one workspace and implements
`GET /tree/{path}`, `GET /tree/{path}?stat=1`, `GET /tree/{path}?list=1`, `HEAD /tree/{path}`,
`PUT /tree/{path}`, `POST /tree:mkdir`, and `DELETE /tree/{path}` with hosted kernel auth/PEP, stable
errors, request limits, daemon-opened listener support, and focused HTTP tests. The canonical
resource-shaped create-directory route (`PUT /tree/{path}/`) remains target even though the current
operator-facing action route is source-backed.

## 3. Tier-1 - JSON-RPC

1:1, nothing special: `fs.readFile`, `fs.writeFile`, `fs.stat`, `fs.listDirectory`, `fs.walk`,
`fs.createDirectory`, `fs.move`, `fs.copy`, ... (P9-0002 section 3). `fs.listDirectory`/`fs.walk` stream via
`*.next`/`*.end`.

Current source-backed hosted JSON-RPC uses the method names implemented by `crates/loom-hosted`:
`fs.read_file`, `fs.write_file`, `fs.stat`, `fs.list_directory`, `fs.create_directory`, and
`fs.delete`. Camel-case method aliases, streaming list/walk, move/copy, append, range, handle, symlink,
and gRPC parity remain target.

## 4. Tier-1 - gRPC

Per 0008 section 4.2: `ReadFileStream` (server-streaming), `WriteFileStream` (client-streaming),
`ListDirectory`/`Walk` (server-streaming); `Stat`, `Move`, `Copy`, `CreateDirectory`, `Delete` unary.

Current source-backed hosted gRPC serves `loom.hosted.v1.Files` over daemon-opened `files/grpc`
listeners for unary `Read`, `Write`, `Stat`, `Mkdir`, and `Delete`, plus server-streaming `List`.
The built subset reuses the shared hosted kernel for metadata auth, PEP, stable error mapping, and
store writes. Client-streaming read/write, `Walk`, `Move`, `Copy`, append, range, file handles,
symlink operations, generated protobuf artifacts, and broader conformance remain target.

## 5. Tier-1 - MCP

- **Read tools (always on):** `files.read`, `files.stat`, `files.list`, `files.walk`. The working tree is
  also exposed as MCP **resources** (a path resolves to its bytes).
- **Write tools (token-gated, P9-0002 section 5):** `files.write`, `files.create`, `files.delete`, `files.move`,
  `files.copy`, `files.mkdir`. Hidden unless the session presents a capability token with write on the
  workspace/path prefix.

## 6. Tier-2 - foreign adapters

Two references make `files` feel native to existing tools:

- **FUSE mount (primary; `fuser` on Linux/macOS, `winfsp`/`dokan-rust` on Windows; 0013 section C).** Mounts a
  `files` (or `vcs`) workspace as a real folder so unmodified apps (`ls`, `cp`, `vim`) operate on it. This
  is the headline UX win. It exposes the resolved modes in section 8 RD-F1. Native-only (a FUSE
  server cannot run in `wasm32`). Fully specified in `P9-0017-fuse-mount.md`.
- **S3 backing role.** S3 is now a first-class served surface, not a `files` transport. A served S3
  service endpoint selects a workspace as its authority scope and resolves buckets from virtual-host-
  style Host headers, CNAME-style Host headers, or path-style compatibility fallback. A bucket-scoped
  S3 endpoint binds one bucket to one listener so the request path is the object key root. Bucket state
  lives under `.loom/facets/s3/buckets/{bucketname}`. Daemon-opened `s3/rest` is source-backed for
  service-scoped and bucket-scoped listeners, bucket create/list/delete, object put/get/head/delete,
  metadata headers, byte ranges, conditional writes, S3-compatible version records, basic multipart
  upload, hosted auth/PEP, SigV4 app credential verification, configured unauthenticated public-read
  ACLs, guarded AWS CLI create/put/get transcript coverage, and conformance rows. The `files` facade
  may back path-like object materialization, but S3 bucket APIs, public-access configuration,
  metadata, S3-compatible versioning records, multipart state, error XML, SigV4 app credentials,
  Loom-native auth, configured unauthenticated public access, and conditional writes are specified by
  the S3 surface. Externally visible S3 `versionId` values are
  opaque S3-safe tokens over
  deterministic internal version identity, not raw Loom commit IDs. S3 ETags are
  representation/version ETags; Loom digests remain separate metadata.
- **Archive import/export.** Archive formats are file-tree interchange, not source storage. The
  canonical archive format is `tar.zstd`; `tar`, `tar.gz`, and `zip` are compatibility formats.
  Source-backed Rust and CLI import/export preserve deterministic ordering, reject unsafe import
  paths, and map supported metadata into Loom file-tree state without changing the base `files`
  contract. Single-file gzip remains an import-only compatibility input.

(Git remote is **not** a `files`/`vcs` adapter - permanently excluded, ADR-0002/0012.)

## 7. Errors / parity / concurrency

- **Errors:** the filesystem codes are already in 0008 section 6 (`NOT_FOUND`, `NOT_A_DIRECTORY`,
  `IS_A_DIRECTORY`, `DIRECTORY_NOT_EMPTY`, `NOT_A_SYMLINK`, `INVALID_PATH`, `NO_WORKING_TREE`,
  `WORKING_TREE_DIRTY`). No new codes.
- **Parity (0032):** Tier-1 REST/JSON-RPC/gRPC are portable; the FUSE server is native-only. The
  first-class S3 server has its own parity and hosted certification rows under task 370.
- **Concurrency:** working-tree writes serialize through the single-writer engine (P9-0002 section 10; 0005 section 6.4).

## 8. Resolved Decisions And Remaining Questions

### RD-F1 - What filesystem does FUSE expose?

> **Resolved - both modes, chosen at mount:** **mounting a commit is read-only**; **mounting the tip is
> read-write by default, or read-only via a mount option.** Read-write edits the working tree live with
> **explicit** commit (`loom commit`); mountable types are `files`/`vcs` only (0014). Specified in
> `P9-0017-fuse-mount.md`; only symlink/xattr sub-questions remain (findings F11). The options/recommendation
> below are retained as the rationale record.


- **Context.** 0013 section C allows mounting a `files`/`vcs` workspace as "the live tip, read-write" or "a commit
  id, read-only," and the owner chose both modes. This decision gates `P9-0017-fuse-mount.md`.
- **Example.** `vim /mnt/loom/notes.md` on a read-write live-tip mount must decide when the edit becomes a
  commit (on `close()`? on `loom commit`? on a debounce timer?); a read-only commit mount avoids the
  question but only serves inspection.
- **Options.** (a) **read-only commit mount first** (safe MVP), read-write later; (b) **read-write
  live-tip** with explicit-commit (edits stage into the working tree, commit only on `loom commit`); (c)
  **both**, mode chosen at mount. Sub-decisions either way: symlink behavior (no ops today), POSIX
  mode/owner mapping, case sensitivity, `xattrs`/sparse-file support, and whether the mount is per-branch.
- **Recommendation.** (c) support both, but **land (a) read-only commit mount as the normative MVP** and
  **(b) read-write live-tip behind a flag** with explicit-commit semantics; defer symlinks/xattrs/
  auto-commit cadence to named sub-questions in `P9-0017`. Mountable types stay `files`/`vcs` only (0014).

### OQ-F2 - `append_file` and streaming handles

- **Context.** 0008 section 3.2 maps `append_file` (`PATCH` + `Content-Range`) and byte-range reads, but
  `append_file` and a stateful open-handle API are unbuilt; the facade is currently whole-file
  read/replace plus byte-range read.
- **Example.** A log writer wants `O_APPEND` semantics over FUSE/REST without rewriting the whole file each
  time; today it must `read_file` + `write_file`.
- **Options.** (a) implement `append_file` (PATCH) now and keep I/O otherwise stateless; (b) add a full
  open/seek/write handle API (bigger surface, needed for some FUSE workloads); (c) defer both until a
  consumer needs them.
- **Recommendation.** (a) implement `append_file` (it is already in the 0008 mapping and is cheap), defer a
  stateful handle API to (c) until the FUSE read-write mode (OQ-F1 (b)) actually requires it.
