# 0003a - File Facade Extensions

**Status:** Mixed. Promoted file operations are source-backed; remaining projection and binding breadth
is target work. A hosted REST/JSON-RPC subset is source-backed for whole-file tree operations.
**Version:** 0.1.0.

This sub-spec records file-facade operations that were split out while 0003 closed. Append, byte-range
I/O, truncation, streaming file handles, and symlink create/read-link have since been promoted and are
source-backed. The remaining target items are symlink following, a cross-facet symbolic-reference model,
projection-specific behavior beyond the portable VFS layer and the current hosted subset, and binding
breadth beyond the current C ABI, Node, and Python projection.

## Current Source Boundary

Implemented today:

- whole-file `write_file`, `read_file`, `remove_file`, and `append_file` over a workspace working tree,
  projected through the C ABI, the `FileSystem` IDL interface, and the Node and Python bindings;
- byte-range `read_at`, `write_at`, and `truncate_file` (path form), and the full file-handle surface
  (`file_open` / `read` / `read_at` / `write` / `write_at` / `truncate` / `flush` / `stat` / `close`)
  with `OpenMode { Read, Write, ReadWrite, Append }`, backed by content-defined chunk streaming and
  projected through the C ABI, the `FileSystem` / `FileHandle` IDL interfaces, and the Node and Python
  bindings;
- symbolic links (`symlink` / `read_link`, git-style: a file slot with the `S_IFLNK` mode whose content
  is the opaque target; `stat` reports a `Symlink` kind), projected through the C ABI, the `FileSystem`
  IDL interface, and the Node and Python bindings;
- directory `exists`, `stat`, `list_directory`, `create_directory`, `remove_directory`, `move_path`,
  `copy_path`, and `walk`;
- hosted listener-scoped files REST and JSON-RPC for whole-file read/write, stat, list, create-directory,
  and delete, including daemon-opened `files/rest` and `files/json_rpc` durable listener support;
- commits and checkout over the workspace tree.

Not implemented today:

- (P2) hosted gRPC, append, range, file-handle, symlink, move, copy, streaming, and protocol-conformance
  semantics beyond the current listener-scoped hosted subset;
- (P2) WASM-specific file projection semantics for these operations - tracked in 0003c, where the portable
  projection layer, FUSE backend, and NFSv3 backend are source-backed;
- symlink *following* by other `fs` operations (symlinks are opaque today: created and read via
  `symlink`/`read_link`, but not traversed), and a general cross-facet "symbolic reference" concept;
- the file-handle facade in the C++, iOS, JVM, Android, React Native, and wasm binding families (only
  the C ABI plus Node and Python carry it today, matching the whole-file facade's reach).

## Target Operations

Every file operation originally split into this sub-spec - byte-range I/O, truncate, streaming file
handles, and symlink create/read-link - is now source-backed (see the change log). What remains
target-only is listed under "Not implemented today" above: symlink *following* during path resolution
(symlinks are opaque today), a general cross-facet symbolic-reference concept, hosted projection beyond
the current REST/JSON-RPC whole-file subset, FUSE/WASM-specific projection semantics, and the file facade
in binding families beyond the C ABI, Node, and Python.

## Residual Promotion Requirements

Before any remaining target behavior in this sub-spec becomes part of 0003:

- (P0) define overwrite, parent creation, directory conflict, and bare-Loom behavior;
- (P1) define chunking and large-file performance expectations;
- (P0) define concurrency semantics between file handles and workspace-history operations;
- (P1) define platform behavior for native, WASM OPFS, hosted protocols, and future FUSE;
- (P0) add source implementation, IDL, C ABI, bindings, and conformance scenarios for the residual
  behavior being promoted;
- (P0) update 0003 to move the promoted operation from target-only to source-backed.

## Change log

### P1 - whole-file write surface (write/read/remove/append_file) promoted to source-backed

`append_file` (the only file op missing from core) plus the projection of the whole-file write surface
to the C ABI / IDL / bindings is implemented and promoted to 0003 §4.3. The P0 file-op prerequisites
were settled to Mac/Linux semantics; bare-Loom is removed as a concept (there is always a working tree),
and file-handle-vs-history concurrency is deferred to the streaming-handles slice. Summary for
re-verification:

- **`append_file` contract.** Create-if-missing (POSIX `>>`); an existing file keeps its mode, a new
  file uses `0o100644`; the parent directory must exist (`NOT_FOUND`); appending to a directory path is
  `ALREADY_EXISTS`; a single atomic working-tree mutation under the §9 `fs` model.
- **Core (`crates/loom-core/src/vcs.rs`).** Added `Loom::append_file`; `write_file`/`read_file`/
  `remove_file` already existed. Two unit tests (create-then-concatenate; missing-parent and
  append-to-directory rejection).
- **Conformance (`crates/loom-conformance`).** New executable `file-ops` suite (`run_file_ops_behavior`
  + scenarios) covering write/read round-trip, truncating write, append create-and-concatenate,
  missing-parent rejection, and remove; wired into `certify_memory_store` and the report.
- **C ABI (`crates/loom-ffi`, `include/loom.h`, iOS C header).** `loom_write_file` (mode `0` -> default
  `0o100644`), `loom_read_file`, `loom_append_file`, `loom_remove_file` - the first files-facade C ABI
  functions - with a happy-path FFI test.
- **IDL (`idl/loom.idl`).** New `FileSystem` interface with `write_file` / `read_file` / `append_file` /
  `remove_file`.
- **Bindings.** Node (`writeFile` / `readFile` / `appendFile` / `removeFile`) and Python (`write_file` /
  `read_file` / `append_file` / `remove_file`), stateless path-based, with round-trip tests.
- **Remaining target.** `read_at` / `write_at`, `truncate`, streaming file handles, symlink create /
  `read_link`, hosted/FUSE/WASM projection, and the files-facade wrappers in the remaining binding
  families (C++, iOS, JVM, Android, React Native, wasm) remain target work.

### P1 - byte-range I/O and file handles (read_at/write_at/truncate + open file descriptions)

Random/offset I/O, truncate, and the full POSIX-style file-handle surface are implemented and promoted
to 0003 §4.5 (with §4 updated). The P0 concurrency prerequisite is settled: handles are open file
descriptions that bind to an inode, with Mac/Linux semantics confirmed by the user. Summary for
re-verification:

- **Handle / inode model.** A handle is `(inode, cursor, mode)`. Two opens of the same path share one
  inode (each with its own cursor); a handle survives the path being renamed or unlinked
  (delete-on-last-close, and a surviving handle's write does not resurrect the path); a whole-file
  `write_file` on an open path is `O_TRUNC` on the same inode. `OpenMode` is `{ Read, Write, ReadWrite,
  Append }`: `Read` requires the file (`NOT_FOUND`); the others create it if missing; `Write` truncates
  the shared inode on open; `Append` writes at the current end. The open-file table (inodes + handles) is
  operational metadata persisted with the local engine state only - excluded from commits, reachability,
  clone, push, bundle, and ordinary sync - and a handle id stays valid across the stateless per-op reopen
  until `close`. `live_object_set` keeps an open (including unlinked) inode's bytes alive until last
  close. Genuinely parallel processes are serialized by the per-workspace lock (last-writer-wins, as
  POSIX); advisory locks, `mmap`, and hard links are out of scope.
- **Byte semantics.** `write_at` past the end zero-fills the gap; `truncate` zero-extends or drops;
  `read_at` clamps at the end (POSIX `pwrite`/`ftruncate`/`pread`). The path-form `write_at` and
  `truncate_file` create a missing file; the path-form `read_at` on a missing file is `NOT_FOUND`. A
  directory path is `ALREADY_EXISTS` and a missing parent is `NOT_FOUND`.
- **Chunk streaming (Q2=B).** `read_at` loads only the chunks overlapping the range; `write_at` /
  `truncate` stream the edit through a new incremental content-defined chunker (`chunk::StreamChunker`)
  and an incremental hasher (`digest::ContentHasher`), so the whole file is never materialized, unchanged
  chunks dedup to their existing objects, and the edited content address is byte-for-byte identical to
  storing the same final bytes wholesale (proven by a unit test).
- **Core (`crates/loom-core`).** `vcs.rs` adds `read_at` / `write_at` / `truncate_file`, `file_open` /
  `file_read` / `file_read_at` / `file_write` / `file_write_at` / `file_truncate` / `file_flush` /
  `file_stat` / `file_close`, the `inodes` / `handles` / `path_to_inode` engine fields with
  export/import persistence, and routes the whole-file ops through the live inode when a path is open.
  `chunk.rs` adds `StreamChunker` (matches the batch chunker, tested), `digest.rs` adds `ContentHasher`
  (matches `Digest::hash`, tested). Eight `vcs` unit tests cover the byte semantics and the full
  concurrency matrix; `lib.rs` exports `OpenMode` and `FileStat`.
- **Conformance.** New executable `file-handle` suite (`run_file_handle_behavior` + 8 scenarios) covering
  write_at/read_at/truncate, streamed-edit equivalence, shared inode, delete-on-last-close,
  replace-while-open, the open-mode rules, and handle survival across an engine-state reload; wired into
  `certify_memory_store`, the aggregate report, and the README.
- **C ABI (`crates/loom-ffi`, `include/loom.h`, iOS C header).** `loom_read_at` / `loom_write_at` /
  `loom_truncate_file` and the opaque-handle object `loom_file_open` (mode `0` read / `1` write / `2`
  read-write / `3` append) / `loom_file_read` / `_read_at` / `_write` / `_write_at` / `_truncate` /
  `_flush` / `_stat` / `_close`, with an FFI test.
- **IDL (`idl/loom.idl`).** `FileSystem` gains `read_at` / `write_at` / `truncate`; new `OpenMode` enum,
  `FileStat` struct, and `FileHandle` interface.
- **Bindings.** Node (`readAt` / `writeAt` / `truncateFile` and `fileOpen` / `fileRead` / `fileReadAt` /
  `fileWrite` / `fileWriteAt` / `fileTruncate` / `fileFlush` / `fileStat` / `fileClose`) and Python
  (snake_case equivalents), with round-trip + handle tests.
- **Remaining target.** Symlink create / `read_link`, hosted/FUSE/WASM projection, the `watch` API, and
  the file-handle facade in the C++/iOS/JVM/Android/React Native/wasm binding families (the C ABI plus
  Node and Python carry it today, matching the whole-file facade's reach).

### P3 - symbolic links (symlink / read_link) promoted to source-backed

Symlinks are implemented git-style and promoted to source-backed. Decision: store a symlink as a file
slot (`StagedEntry::File`) whose mode carries `S_IFLNK` (`0o120000`) and whose content is the opaque
target path. This was chosen over a new cross-cutting `StagedEntry::Symlink` variant precisely because
the store is multi-facet: a file-slot mode bit keeps the symlink a files-facet detail that commit,
checkout, merge, diff, reachability, and sync already handle, instead of forcing a symlink concept onto
every facet's slot match sites. Symlinks are opaque (other `fs` operations do not follow them);
following is deferred. Summary for re-verification:

- **Core (`crates/loom-core`).** `vcs.rs` adds `SYMLINK_MODE` / `is_symlink_mode`, `Loom::symlink`
  (parent must exist `NOT_FOUND`; existing path `ALREADY_EXISTS`; empty target rejected; dangling target
  allowed), and `Loom::read_link` (`NOT_FOUND` if absent, `INVALID_ARGUMENT` if not a symlink). `fs.rs`
  adds `FileKind::Symlink` and `stat` reports it. Two unit tests, including a commit + checkout
  round-trip proving the symlink mode survives the Tree.
- **Conformance.** New executable `symlink` suite (`run_symlink_behavior` + scenarios) covering
  create/read, dangling targets, stat reporting, the error matrix, and a commit round-trip; wired into
  `certify_memory_store`, the aggregate report, and the README.
- **C ABI / IDL / bindings.** `loom_symlink` / `loom_read_link` (+ `include/loom.h` and iOS header), the
  `FileSystem` IDL `symlink` / `read_link`, and Node (`symlink` / `readLink`) + Python (`symlink` /
  `read_link`) wrappers, each with an FFI/binding test.
- **Remaining target.** Symlink *following* during path resolution, a general cross-facet
  symbolic-reference concept, and symlink-aware directory listings (`list_directory` reports a symlink as
  a plain entry today; `stat` is symlink-aware).
