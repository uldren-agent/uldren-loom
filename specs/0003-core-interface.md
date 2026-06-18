# 0003 - Core Interface

**Status:** Complete for current source-backed interface; target extensions split. **Version:**
0.1.0-draft. **Normative.**

This is the public interface contract every Loom implementation projects as source catches up. Its
current source-backed contract is the checked-in IDL and ABI surface: workspace lifecycle, key-source
wrap management, a narrow C ABI `LoomSession` authentication bridge, SQL/result access, workspace
history operations, direct table/history readers, CAS, queue, and queue consumer offsets. Its target
enterprise contract is grouped around facades: `fs` (filesystem, §4), `vcs` (workspace history, §5),
`sync` (synchronization, summarized in §7, fully specified in 0006), and `db` (relational, §7.5 /
0011). The error taxonomy (§8) is shared by every promoted facade, and every facade method is
workspace-scoped (§2.1). Additional data, identity administration, ACL administration, compute,
trigger, and watch facades are target promotion candidates. A facade is part of the stable interface
only after source, IDL, ABI or binding projection, wire projection, and conformance coverage agree.

A **facade** is an interface surface (a method group); a **facet** is a data shape reached through one
(0001 glossary). Files have the `fs` facade (§4) and relational tables the `db` facade (§7.5 / 0011);
every other facet in the 0013 catalog has its own facade doc (§7.6).

> **Current implementation:** the checked-in `idl/loom.idl` is the source-backed language-neutral
> inventory for the current ABI and bindings, not yet a generator. Implemented source includes
> workspace lifecycle and selection primitives, whole-file working-tree reads, writes, and removals,
> directory metadata, listing, move/copy/walk helpers, commits, checkout, branch, log, diff, file-path
> merge, direct sync clone/push/bundles, deterministic facet substrates, SQL execution surfaces, and
> the stable `loom_core::error::Code` enum. The C ABI also exposes
> `loom_authenticate_passphrase` and `loom_clear_authentication` so a handle can carry a principal
> session across per-call opens. The current C ABI still uses several direct functions rather than the
> target facade object model. Its workspace-scoped history and table functions select a workspace by
> `ns_name` plus a required `facet`; the target `NsSelector` facade projection remains future
> reconciliation work. Identity and ACL administration, watch, rebase, cherry-pick, revert, squash,
> generated artifacts, and full hosted wire projection remain target work unless a crate exports them.
> File-facade extensions are tracked in 0003a; workspace-history extensions are tracked in 0003b.

## 1. Design principles

- **P1 - Path-based and handle-based both.** Convenience path methods (`read_file(path)`) and
  explicit handle methods (`open` → `read`/`write` → `close`) are both provided; handles enable
  streaming and byte-range I/O without buffering whole files.
- **P2 - Async-first.** Every method that may touch storage or network is asynchronous in its
  binding form (Promise / CompletableFuture / future). Synchronous variants MAY be offered by a
  binding but MUST be suffixed `Sync` and MUST NOT be the default (0007 §5).
- **P3 - Explicit transactions for multi-step atomicity.** Single methods are atomic; multi-step
  atomicity uses a `Batch`/transaction (§6).
- **P4 - Capabilities are queryable.** `capabilities()` returns the provider's profile; calling an
  unsupported method returns `UNSUPPORTED`, never a silent no-op (0001 §6 A3).
- **P5 - Stable, typed errors.** Every failure is one of the codes in §8; bindings map these to
  idiomatic error types but preserve the code.

## 2. Top-level objects

```idl
// Opening a Loom yields the root handle exposing the three facades.
interface Loom {
  fs:    FileSystem
  vcs:   VersionControl
  sync:  Sync
  db:    Option<Db>        // relational facade; present iff the provider advertises capability `sql` (0011)
  exec:  Option<Exec>      // compute facade; present iff the provider advertises capability `exec` (0015, exploratory)
  meta:  LoomMeta          // identity profile, capabilities, stats
  close():  Future<void>
}

// Construction is provider-driven (0004). Conceptually:
Loom.open(provider: Provider, options: OpenOptions): Future<Loom>
Loom.create(provider: Provider, options: CreateOptions): Future<Loom>   // init a new, empty Loom
```

`LoomMeta` exposes the identity profile (0002 §8), the capability set (§10 of 0010), provider
info, and counters (object count, store size, ref count).

## 2.1 Workspace selector (normative; required on every facade method)

A Loom holds independent, named workspaces (0014), and they are **first-class**: there is no implicit
"current workspace" and no silent default on the call path. **Every method of the `fs`, `vcs`, and
`db` facades takes a workspace selector as its first parameter, `ns: NsSelector`.** This is mandatory
for all of those methods; the IDL below shows it on the file and history operations and it applies
identically to the rest.

```idl
oneof NsSelector {
  id:      Uuid                            // by workspace id (0014 §2)
  name:    string                          // by workspace name
  default: bool                            // the workspace named "Default"
}
```

Each operation touches **exactly one** workspace. Cross-workspace operations are disallowed (0014 §5):
any operation given two selectors that resolve to different workspaces - a `merge`, `rebase`, `diff`,
`cherry_pick`, or a low-level object/ref op across workspaces - raises `CROSS_WORKSPACE` (§8). Passing
a selector whose workspace lacks the required facet or projection for the facade raises `NOT_FOUND`
or `UNSUPPORTED` according to that facade's contract.

**Current implementation:** Rust and CLI workspace operations resolve by UUID or name. Several
checked-in C ABI history and table functions use the shape `(facet, ns_name, ...)`: `facet` is parsed
as a `FacetKind` and requires the selected workspace to contain that facet. That shape is a current
projection of workspace selection, not a separate workspace class. `idl/loom.idl` does not yet carry
the target `NsSelector` parameter.

## 3. The Loom IDL (normative type language)

The target IDL is language-neutral. Bindings map it per 0007; protocols project it per 0008. The
checked-in `idl/loom.idl` is the current source-backed IDL inventory for existing ABI and binding
surfaces. It is not yet the full target facade model: it does not expose the target `NsSelector`,
top-level `Loom` object, full filesystem facade, generic object facade, live sync facade, compute,
identity administration, ACL administration, watch, trigger, hosted protocol schemas, or generated
artifacts. The C ABI `LoomSession` bridge is source-backed but not yet projected into this IDL.

### 3.1 Scalars

`bool, i32, i64, u32, u64, f64, string (UTF-8), bytes, Timestamp (i64 ms since Unix epoch + optional i16 tz_minutes display offset), Digest (0002 §2), Uuid`

To mimic git, file `mtime` is **advisory and never hash-affecting** - git tree entries carry no
timestamp - so byte-identical content dedups regardless of clock or timezone differences (0002 §3.6).
Commit author/committer timestamps hash the UTC instant only. In source today, commit objects store
`timestamp_ms` and no timezone offset; any future display offset is metadata and is not
hash-affecting.

### 3.2 Composites

`Option<T>` (nullable), `List<T>`, `Map<K,V>` (string keys), `enum`, `struct`, `oneof` (tagged
union), `Stream<T>` (asynchronous sequence, for listings and large I/O).

### 3.3 Core domain types

```idl
enum NodeKind { TREE, FILE, SYMLINK, SUBLOOM }

struct Stat {
  path:        string
  kind:        NodeKind
  size:        u64            // 0 for directories
  digest:      Digest         // content digest (Tree/Blob/ChunkList per kind)
  mode:        u32            // POSIX-style; advisory if provider lacks `posix-perms`
  mtime:       Option<Timestamp>
  executable:  bool
  symlink_target: Option<string>   // present iff kind == SYMLINK
}

struct DirEntry { name: string; kind: NodeKind; digest: Digest; size: u64 }

struct Identity { name: string; id: string; timestamp: Timestamp }  // id is email or key id

struct OpenOptions  { ref: Option<string>; read_only: bool; working_tree: WorkingTreeMode }
struct CreateOptions{ identity_profile: Option<IdentityProfile>; bare: bool; default_branch: string }
enum   WorkingTreeMode { NONE /*bare*/, LAZY /*materialize on demand*/, EAGER /*full checkout*/ }
```

> **Open topic (unowned): `WorkingTreeMode` / bare Loom.** The source implements a single always-present
> working tree per workspace; `WorkingTreeMode` (`NONE`/`LAZY`/`EAGER`), the `bare` create flag, and the
> "bare Loom returns `UNSUPPORTED`" wording below are target-only and are not reflected in source. The
> owner has indicated there is always a working tree, so this concept is likely to be removed or
> redefined. This is left as an open spec-cleanup topic for a separate pass; treat the bare-Loom wording
> in §4 and §5.9 as not source-backed until then.

## 4. FileSystem facade (`fs`)

Operates on the workspace working tree of the currently checked-out commit, with copy-on-write
staging. The `fs` facade applies to workspaces that have the `files` facet or a file projection
defined by another facet (0014). Edits land in an unstaged working area and are committed through
workspace history operations. On a bare Loom (`WorkingTreeMode.NONE`), write operations and path
reads that require a materialized tree return `UNSUPPORTED` under the current stable error enum;
callers use the VCS object API (§5.9) to read content by digest instead.

**Current implementation:** `loom-core::vcs::Loom` implements whole-file `write_file`, `read_file`,
`remove_file`, and `append_file` over the workspace working tree. `loom-core::fs` adds `exists`, `stat`,
`list_directory`, `create_directory`, `remove_directory`, `move_path`, `copy_path`, and `walk`.
Directories are first-class and file writes require an existing parent directory. Every public `fs`
mutator rejects user writes within the reserved `.loom` subtree (facet storage and Loom metadata) with
`PermissionDenied`, per the 0014a baseline; reads and listings of that subtree are allowed, and facet
implementations (in-core facades and external facet crates such as `loom-sql`) write their own
`.loom/facets/...` storage through the privileged `write_file_reserved` / `create_directory_reserved`
methods. No facet is special-cased. The
whole-file write surface (`write_file`, `read_file`, `append_file`, `remove_file`) is now projected
through the C ABI
(`loom_write_file` / `loom_read_file` / `loom_append_file` / `loom_remove_file`), the `FileSystem`
interface in `idl/loom.idl`, and the Node and Python bindings, with conformance coverage. `append_file`
is POSIX-style: it creates the file if absent and concatenates otherwise, requires an existing parent
(`NOT_FOUND`), and rejects a directory path (`ALREADY_EXISTS`). Byte-range `read_at` / `write_at` /
`truncate_file` and the full file-handle surface (§4.5) are now source-backed as well, projected through
the C ABI, the `FileSystem` / `FileHandle` interfaces in `idl/loom.idl`, and the Node and Python
bindings, with conformance coverage. `crates/loom-hosted` projects a listener-scoped files REST and
JSON-RPC subset for whole-file read/write, stat, list, create-directory, and delete, with daemon-opened
`files/rest` and `files/json_rpc` support through durable `loom serve` records. Hosted gRPC, append,
range, file-handle, symlink, move, copy, and full protocol conformance remain target work here.

### 4.1 Path model

- Paths are **absolute, POSIX-style**, `/`-separated, UTF-8, **NFC-normalized** on input
  (capability `unicode-nfc`; if unsupported, paths are compared bytewise and the provider MUST
  document this). The root is `/`.
- A path MUST NOT contain a NUL byte or an empty segment; `.` and `..` are resolved
  lexically before use and MUST NOT escape the root (`/..` is an error `INVALID_ARGUMENT`).
- Case sensitivity is a provider property (`case-sensitive`); the local shim inherits the host
  filesystem's behavior and MUST report it.

### 4.2 Directory operations

```idl
// read_directory / list_directory: list immediate children.
//   list_directory streams entries (good for huge dirs); read_directory buffers them.
fs.list_directory(path: string, opts: ListOptions): Stream<DirEntry>
fs.read_directory(path: string, opts: ListOptions): Future<List<DirEntry>>
   // errors: NOT_FOUND, INVALID_ARGUMENT, UNSUPPORTED

// create_directory: create a directory; `recursive` makes parents (mkdir -p).
fs.create_directory(path: string, opts: { recursive: bool, mode: Option<u32> }): Future<void>
   // errors: ALREADY_EXISTS (unless recursive), NOT_FOUND (missing parent & !recursive),
   //         INVALID_ARGUMENT (a path component is a file)

// remove_directory: remove a directory; `recursive` removes contents.
fs.remove_directory(path: string, opts: { recursive: bool }): Future<void>
   // errors: NOT_FOUND, INVALID_ARGUMENT, CONFLICT (if !recursive and non-empty)

struct ListOptions { recursive: bool; include_stat: bool; glob: Option<string>; limit: Option<u64>; cursor: Option<string> }
```

### 4.3 File operations

Each method takes the workspace selector `ns` first (§2.1), then operates per-operation on the working
tree (a `write`/`truncate`/`delete` applies when it is called, POSIX-style; §4.5).

```idl
// read_file: read entire file content. For ranges/streaming use open() (§4.5).
fs.read_file(ns: NsSelector, path: string): Future<bytes>
   // errors: NOT_FOUND, INVALID_ARGUMENT, UNSUPPORTED

// create_file: create a new file; fails if it exists (use write_file to overwrite).
fs.create_file(ns: NsSelector, path: string, content: bytes, opts: { mode: Option<u32>, parents: bool }): Future<Stat>
   // errors: ALREADY_EXISTS, NOT_FOUND (missing parent & !parents), INVALID_ARGUMENT

// write_file: create-or-replace the full content (truncating write). Atomic.
fs.write_file(ns: NsSelector, path: string, content: bytes, opts: { create: bool, mode: Option<u32>, parents: bool }): Future<Stat>
   // errors: NOT_FOUND (no file & !create), INVALID_ARGUMENT

// append_file: append bytes to the end.
fs.append_file(ns: NsSelector, path: string, content: bytes): Future<Stat>   // errors: NOT_FOUND, INVALID_ARGUMENT

// delete_file: remove a file (not a directory).
fs.delete_file(ns: NsSelector, path: string): Future<void>                   // errors: NOT_FOUND, INVALID_ARGUMENT

// stat / exists
fs.stat(ns: NsSelector, path: string): Future<Stat>                          // errors: NOT_FOUND
fs.exists(ns: NsSelector, path: string): Future<bool>

// move / copy (src and dst are in the same workspace; crossing workspaces raises CROSS_WORKSPACE)
fs.move(ns: NsSelector, src: string, dst: string, opts: { overwrite: bool }): Future<void>
   // errors: NOT_FOUND(src), ALREADY_EXISTS(dst &!overwrite), INVALID_ARGUMENT(dst under src)
fs.copy(ns: NsSelector, src: string, dst: string, opts: { overwrite: bool, recursive: bool }): Future<void>
   // copy is O(1) metadata where possible (content is shared by digest - copy-on-write)

// symlink (capability `symlinks`)
fs.symlink(ns: NsSelector, target: string, link_path: string): Future<void>  // errors: UNSUPPORTED, ALREADY_EXISTS
fs.read_link(ns: NsSelector, path: string): Future<string>                   // errors: NOT_FOUND, INVALID_ARGUMENT
```

### 4.4 Walking

```idl
fs.walk(root: string, opts: WalkOptions): Stream<Stat>   // pre-order DFS by default; depth/glob filters
struct WalkOptions { max_depth: Option<u32>; glob: Option<string>; follow_symlinks: bool; include_dirs: bool }
```

### 4.5 Handles (streaming & byte ranges)

```idl
enum OpenMode { READ, WRITE_TRUNCATE, WRITE_APPEND, READ_WRITE }
fs.open(ns: NsSelector, path: string, mode: OpenMode, opts: OpenFileOptions): Future<FileHandle>

interface FileHandle {
  read(len: u64): Future<bytes>                  // sequential
  read_at(offset: u64, len: u64): Future<bytes>  // random access (capability `random-read`, always true for CAS backends)
  write(data: bytes): Future<u64>                // applies to the working tree immediately; returns bytes written
  write_at(offset: u64, data: bytes): Future<u64>// capability `random-write`; applies immediately
  truncate(size: u64): Future<void>              // applies immediately
  flush(): Future<void>                          // force durability of writes already applied
  stat(): Future<Stat>
  close(): Future<void>                          // release the handle; writes were already applied per-operation
}
```

Streaming reads/writes MUST be backed by chunked I/O (0002 §3.2) so arbitrarily large files do not
require full buffering. **Each mutating operation (`write`, `write_at`, `truncate`) applies to the
working tree when it is called**, exactly as a POSIX or local filesystem behaves: a handle held open
for hours with no writes changes nothing, and a write is visible to a subsequent read without waiting
for `close()`. `flush()` forces durability of writes already applied; `close()` only releases the
handle. Concurrency between handles follows the filesystem model in §9.

**Current implementation:** source-backed. `loom-core::vcs::Loom` implements byte-range `read_at` /
`write_at` / `truncate_file` (path form) and the open-file-description surface `file_open` / `file_read`
/ `file_read_at` / `file_write` / `file_write_at` / `file_truncate` / `file_flush` / `file_stat` /
`file_close` with `OpenMode { Read, Write, ReadWrite, Append }`. A handle binds to an inode, not a path:
two opens of the same path share one inode (each with its own cursor); a handle survives the path being
renamed or unlinked (delete-on-last-close, with no resurrection of the path by a surviving handle's
write); and a whole-file `write_file` on an open path is `O_TRUNC` on the same inode. `write_at` past the
end zero-fills the gap, `truncate` zero-extends or drops, and `read_at` clamps at the end (POSIX
`pwrite`/`ftruncate`/`pread`). Reads and writes are backed by content-defined chunking: a `read_at` loads
only the overlapping chunks, and a `write_at`/`truncate` streams the edit so the whole file is never held
in memory and unchanged chunks dedup to their existing objects (the edited content address equals storing
the same final bytes wholesale). The open-file table (inodes + handles) is operational metadata persisted
with the local engine state only; it is excluded from commits, reachability, clone, push, bundle, and
ordinary sync, and a handle id stays valid across the stateless per-op reopen until `close`. The surface
is projected through the C ABI (`loom_read_at` / `loom_write_at` / `loom_truncate_file` and
`loom_file_open` / `_read` / `_read_at` / `_write` / `_write_at` / `_truncate` / `_flush` / `_stat` /
`_close`), the `FileSystem` / `FileHandle` interfaces in `idl/loom.idl`, and the Node and Python
bindings, with an executable `file-handle` conformance suite. Not yet implemented: advisory locks
(`flock`/`fcntl`), `mmap`, hard links (one path per inode), and `watch` (§4.6).

### 4.6 Watching (capability `watch`)

`watch` is a target capability owned by 0030. The source-backed filesystem facade does not expose a
public watch surface today. The v1 promotion path is the pull baseline from 0030 and CP-0003:
`watch.subscribe(selector, from?) -> cursor` and `watch.poll(cursor, max) -> { events, next }`, where
the selector names one workspace, one exact ref, a scope, domain filters, and normalized change-kind
filters. Push streams and durable delivery layer later through 0035.

The baseline event granularity is a 0030 `DataChange` workspace revision envelope per workspace/ref.
File-domain `DomainChange` detail is source-backed with path keys, normalized change-kind strings, and
before or after digests where derivable from the existing VCS diff substrate. Other domain-owned
`DomainChange` detail is promoted by the owning domain specs; until then, broad workspace watch reports
capability-labeled unsupported-domain detail markers for readable non-file facet changes. The watch
source cursor is an opaque versioned string, not a durable delivery cursor and not a server-side
subscription row. Legacy direct filesystem watch shapes such as `fs.watch(path, ...) ->
Stream<FsEvent>` are not the Loom contract.

## 5. VersionControl facade (`vcs`)

`loom.vcs` is the workspace history facade. It keeps the familiar VCS method names while operating
on workspace history, not on a dedicated `vcs` facet or workspace class. All history-rewriting
operations create new objects and move refs; they never mutate existing objects.

**Workspace scope (normative).** The full facade applies to a workspace selected by `ns`. Branch,
merge, rebase, cherry-pick, diff, log, checkout, tag, and ref operations operate within that one
workspace's history. The data being versioned may include files plus other facets such as SQL, KV,
document, program, or graph data when those facets are present in the workspace (0014).

**Current implementation:** `loom-core::vcs::Loom` implements commit, checkout by branch or commit,
branch creation, first-parent log, path-level diff between two commits, and merge from another branch
inside the same workspace. A conflicting merge now enters a recoverable in-progress state, and
`merge_resolve`, `merge_abort`, and `merge_continue` (with `merge_in_progress` / `merge_conflicts`
introspection) are source-backed across core, C ABI, IDL, and the Node and Python bindings (§5.6).
Status/index staging, staged commit, tag operations, rebase, cherry-pick, revert, squash,
path-restricted checkout, and restore are source-backed in core and tracked in 0003b. `crates/loom-hosted`
projects a listener-scoped VCS REST and JSON-RPC subset for commit, commit-staged, log, branch,
checkout, status, stage, stage-all, unstage, merge, and structural diff, with daemon-opened `vcs/rest`
and `vcs/json_rpc` support through durable `loom serve` records. Hosted merge-resolution routes, gRPC,
generated schemas, and full protocol conformance remain target work.

### 5.1 init / open

```idl
// init is performed via Loom.create (§2). vcs.init re-initializes refs on an existing object store.
vcs.init(ns: NsSelector, opts: { default_branch: string }): Future<void>   // errors: ALREADY_EXISTS
vcs.head(ns: NsSelector): Future<RefStatus>                                // current ref, commit, attached/detached
struct RefStatus { ref: Option<string>; commit: Option<Digest>; detached: bool; bare: bool }
```

### 5.2 staging & status

```idl
vcs.status(ns: NsSelector, opts:{paths:List<string>}): Future<Status>   // working tree vs HEAD
vcs.stage(ns: NsSelector, paths: List<string>): Future<void>            // add to next commit (a.k.a. add)
vcs.unstage(ns: NsSelector, paths: List<string>): Future<void>
struct Status { staged: List<Change>; unstaged: List<Change>; untracked: List<string>; conflicts: List<string> }
struct Change { path: string; kind: enum{ADDED,MODIFIED,DELETED,RENAMED}; old: Option<Digest>; new: Option<Digest>; from: Option<string> }
```

**Staging modes (normative).** A workspace is in exactly one of two staging modes, reported by
`capabilities()`:

- **explicit** (default, git-like): `stage`/`unstage` manage an index, and `commit` snapshots the
  staged set. `status` reports `staged`, `unstaged`, and `untracked` against that index.
- **implicit** (capability `implicit-staging`): every `fs` write is staged as it happens and `commit`
  snapshots the whole working tree. There is no separate index to add to or remove from, so `stage`
  succeeds as a **no-op** (the paths are already staged), `unstage` returns **`UNSUPPORTED`** (there
  is nothing to unstage from), and `status` reports all changes under `staged` with an empty
  `untracked` set.

A tool that only `stage`s then `commit`s works unchanged in either mode; a tool that depends on
`unstage` must handle `UNSUPPORTED` or require the explicit mode.

**Current implementation (source-backed).** The engine implements a single shared staging index per
workspace that spans every facet (files, SQL, KV, queue, and the rest): `stage` / `stage_all` /
`unstage` move working-tree changes into that one index, and `status` reports `staged` (index vs HEAD),
`unstaged` (working tree vs index), `untracked`, and the merge `conflicts`. There are two commit verbs:
`commit` records the whole working tree (staged plus unstaged), so a caller that simply writes and
commits never has to think about staging, while `commit_staged` records only the index for a partial
commit. `Change` carries `path` and `kind` (`ADDED`/`MODIFIED`/`DELETED`); rename detection and the
`old`/`new`/`from` fields remain target. Cross-facet operations (commit, and later cherry-pick, rebase,
revert) act over this one shared stage, not per facet. Source-backed across `loom-core`, the C ABI
(`loom_status`, `loom_stage`, `loom_stage_all`, `loom_unstage`, `loom_commit_staged`), `idl/loom.idl`,
and the Node and Python bindings; conformance covers status classification, partial staged commit,
commit-everything, and unstage. The per-workspace staging-mode capability flag above is target framing;
current source always provides the shared index plus both commit verbs.

**Auto-commit policy (target).** Every workspace has an unstaged working area. The default policy is
explicit commit through `vcs_commit`. A workspace may later opt into an automatic snapshot policy,
but that is a workspace policy decision and does not create a separate `files` or `vcs` workspace
class.

### 5.3 commit

```idl
vcs.commit(ns: NsSelector, opts: CommitOptions): Future<Digest>
struct CommitOptions {
  message: string
  author: Option<Identity>            // defaults to configured identity
  parents: Option<List<Digest>>       // defaults to current HEAD; explicit for low-level use
  amend: bool                         // replace HEAD commit (produces a new digest)
  allow_empty: bool
  sign: bool                          // capability `signing` (0009 §5)
}
// errors: CONFLICT (nothing to commit unless allow_empty), UNSUPPORTED(sign)
```

### 5.4 log / show / diff

```idl
vcs.log(ns: NsSelector, opts: LogOptions): Stream<Commit>      // DAG walk, newest-first by default
vcs.show(ns: NsSelector, commit: Digest): Future<Commit>
vcs.diff(ns: NsSelector, opts: DiffOptions): Future<Diff>      // two trees/commits, or working tree vs HEAD
struct DiffOptions { from: Option<Rev>; to: Option<Rev>; paths: List<string>; rename_detection: bool }
struct Diff { changes: List<Change>; stats: DiffStats }
// Rev = a Digest, a ref name, or a revision expression (§5.11).
```

### 5.5 branch / tag / checkout

```idl
vcs.branch_create(ns: NsSelector, name: string, at: Option<Rev>): Future<void>   // errors: ALREADY_EXISTS, NOT_FOUND(at)
vcs.branch_list(ns: NsSelector): Future<List<BranchInfo>>
vcs.branch_delete(ns: NsSelector, name: string, opts:{force:bool}): Future<void> // errors: NOT_FOUND, CONFLICT(&!force)
vcs.tag_create(ns: NsSelector, name: string, at: Rev, opts:{annotate:bool, message:string, sign:bool}): Future<void>
vcs.tag_list(ns: NsSelector): Future<List<string>>

// checkout / switch: move HEAD and materialize the working tree.
vcs.checkout(ns: NsSelector, target: Rev, opts: { create_branch: Option<string>, force: bool, paths: List<string> }): Future<void>
   // errors: NOT_FOUND, CONFLICT(path-restricted checkout or dirty working tree)

// restore one file's content from a past snapshot into the unstaged working area.
vcs.restore_file(ns: NsSelector, snapshot: Rev, path: string): Future<void>   // errors: NOT_FOUND
```

### 5.6 merge

```idl
vcs.merge(ns: NsSelector, source: Rev, opts: MergeOptions): Future<MergeResult>  // source must resolve within ns
struct MergeOptions { strategy: enum{AUTO, ORT_3WAY, FAST_FORWARD_ONLY, OURS, THEIRS}; commit: bool; message: Option<string>; sign: bool }
struct MergeResult { commit: Option<Digest>; fast_forwarded: bool; conflicts: List<Conflict> }
struct Conflict { path: string; base: Option<Digest>; ours: Option<Digest>; theirs: Option<Digest>; markers: bool }
// Semantics: compute merge base(s) (0002 §6); 3-way merge per file; on conflict, leave conflict
// state in the working tree and return conflicts (commit not created unless conflicts==∅).
// errors: NOT_FOUND(source), NOT_FAST_FORWARD (FAST_FORWARD_ONLY), CONFLICT, CROSS_WORKSPACE
vcs.merge_continue(ns: NsSelector): Future<Digest>    // after resolving conflicts + staging
vcs.merge_abort(ns: NsSelector): Future<void>
```

**Current implementation (source-backed).** A conflicting `merge` enters a per-workspace in-progress
state instead of just reporting paths: it stages the auto-merged result, presents text-file conflicts as
whole-file conflict markers in the working tree, records a structured per-path conflict (base / ours /
theirs slots) as the source of truth, snapshots the pre-merge working tree for abort, and leaves the
branch tip unmoved. The in-progress state is operational metadata persisted with the local engine state
only: it is never part of commits, reachability, clone, push, bundle, or ordinary sync, and it survives
reopen. The resolution surface is `merge_in_progress`, `merge_conflicts`, `merge_resolve(path,
resolution)` where `resolution` is `ours` / `theirs` / `working` (accept the staged content), `merge_abort`
(restore the pre-merge tree and clear the state), and `merge_continue` (require every path resolved, then
record a two-parent merge commit and advance the branch with a compare-and-swap from the recorded `ours`
tip, raising `CONFLICT` while conflicts remain and `CAS_MISMATCH` if the branch moved). This is the
explicit S2 merge tool of `CONFLICT-RESOLUTION-MATRIX.md`, not implicit sync behavior. The full
`MergeOptions` strategy menu, `Conflict.markers` reporting flag, and a `status` view of conflicts remain
target work (0003b). Source-backed across `loom-core`, the C ABI (`loom_merge_in_progress`,
`loom_merge_conflicts`, `loom_merge_resolve`, `loom_merge_abort`, `loom_merge_continue`), `idl/loom.idl`,
and the Node and Python bindings; conformance covers conflict, abort, and resolve-then-continue.

### 5.7 rebase / cherry-pick / revert

```idl
vcs.rebase(opts: RebaseOptions): Future<RebaseResult>
struct RebaseOptions { upstream: Rev; onto: Option<Rev>; interactive_plan: Option<List<RebaseStep>>; autosquash: bool; sign: bool }
struct RebaseStep { action: enum{PICK,SQUASH,FIXUP,EDIT,DROP,REWORD}; commit: Digest; message: Option<string> }
struct RebaseResult { new_head: Option<Digest>; conflicts: List<Conflict>; stopped_at: Option<Digest> }
vcs.rebase_continue(): Future<RebaseResult>
vcs.rebase_abort(): Future<void>

vcs.cherry_pick(commits: List<Digest>, opts:{commit:bool}): Future<RebaseResult>
vcs.revert(commits: List<Digest>, opts:{commit:bool}): Future<RebaseResult>
```

### 5.8 squash

`squash` is specified as a convenience over rebase/merge to collapse a contiguous range of commits
into one:

```idl
vcs.squash(opts: SquashOptions): Future<Digest>
struct SquashOptions { from: Rev; to: Rev; message: string; author: Option<Identity>; sign: bool }
// Semantics: produce a single new commit whose tree == tree(to) and whose parent == parent(from),
// then move the branch ref to it. Equivalent to a non-interactive rebase folding [from..to].
// errors: NOT_FOUND, INVALID_ARGUMENT (from..to spans a merge; caller must use interactive rebase)
```

### 5.9 low-level object API (works on bare Looms)

```idl
vcs.read_object(d: Digest): Future<Object>           // typed object (0002 §3)
vcs.read_blob(d: Digest): Future<bytes>              // assembles ChunkLists transparently
vcs.write_object(o: Object): Future<Digest>          // returns the content address; idempotent
vcs.read_tree(d: Digest, path: Option<string>): Future<List<DirEntry>>
vcs.resolve(rev: Rev): Future<Digest>                // revision expression → digest (§5.11)
vcs.has_object(d: Digest): Future<bool>
```

### 5.10 refs (low-level, CAS)

```idl
vcs.list_refs(prefix: Option<string>): Future<List<Ref>>
vcs.read_ref(name: string): Future<Option<Digest>>
vcs.update_ref(name: string, new: Option<Digest>, opts: { expected_old: Option<Digest>, reflog_msg: string }): Future<void>
   // compare-and-swap; new==None deletes. errors: CAS_MISMATCH, NOT_FOUND
vcs.reflog(name: string): Stream<ReflogEntry>        // capability `reflog`
```

### 5.11 revision expressions

`Rev` accepts: a `Digest`; a ref name (`branch/main`, `tag/v1`, `HEAD`); ancestry shorthands
`HEAD~3` (3rd first-parent ancestor), `HEAD^2` (2nd parent); ranges `A..B`, `A...B`; and the empty
tree sentinel `∅`. Resolution rules are given in Appendix B of this document.

The empty-tree sentinel `∅` is accepted only where a **tree** (not a commit) is meaningful: as
`from`/`to` in `diff` and as a base in low-level tree ops. Commit-expecting methods (`checkout`,
`merge`, `rebase` targets) reject it with `INVALID_ARGUMENT`. This mirrors git, which uses the empty
tree as a diff / read-tree base but never as a checkout target.

### 5.12 maintenance

```idl
vcs.gc(ns: NsSelector, opts:{grace:Duration, dry_run:bool}): Future<GcReport>   // capability `gc` (0002 §7)
vcs.verify(ns: NsSelector, opts:{full:bool}): Future<VerifyReport>             // fsck: recompute & check digests
vcs.pack(ns: NsSelector): Future<PackReport>                                   // repack loose objects into packs
```

Current native maintenance commands run through a writable `FileStore` handle and the single-writer
store guard described in 0036 §8. Public hosted maintenance must add a named runtime lease before
concurrent hosted writers are promoted.

## 6. Batches (multi-step atomicity)

```idl
vcs.batch(ns: NsSelector): Future<Batch>
interface Batch {
  // queue any fs/vcs mutation in this workspace; nothing is visible outside until commit()
  ... mirrors fs/vcs mutating methods ...
  commit(): Future<void>     // atomically apply all (single journal transaction, 0005 §6)
  rollback(): Future<void>
}
```

A Batch maps to a single journal transaction in the single-file backend (0005 §6) - fully atomic. A
batch **reads its own queued writes** (read-your-writes), and all ref compare-and-swaps validate
atomically at `commit()`, surfacing `CAS_MISMATCH`/`CONFLICT` there. A batch is scoped to the one
workspace given to `vcs.batch(ns)`; queuing a mutation in another workspace raises `CROSS_WORKSPACE`.

## 7. Sync facade (`sync`) - summary

Fully specified in 0006. Surface:

```idl
sync.remote_add(name: string, url: string, opts: RemoteOptions): Future<void>
sync.remote_list(): Future<List<Remote>>
sync.fetch(remote: string, refspecs: List<string>): Future<FetchReport>
sync.pull(remote: string, refspec: string, opts: MergeOptions): Future<PullReport>
sync.push(opts: PushOptions): Future<PushReport>
sync.clone(source: Provider|url, dest: Provider, opts: CloneOptions): Future<Loom>
sync.bundle_export(refs: List<string>, out: WritableStream): Future<BundleReport>   // offline sync
sync.bundle_import(in: ReadableStream): Future<ImportReport>
```

## 7.5 Db facade (`db`) - summary

The `db` facade is a required part of this interface, present when the provider advertises the `sql`
capability (`loom.db` is `None` otherwise, and calling into it returns `UNSUPPORTED`). It exposes
**versioned SQL tables** stored in the same object model (tables as prolly trees, 0002 §4). The facade (`create_table`, `exec`, `query`,
transactions, and version-control-over-rows: `diff_rows`, `merge_table`, `blame`, `as_of`) and its
storage encoding are specified in **0011**. Files and tables coexist in one Loom and commit in one
transaction (§6). This section is a pointer; 0011 is normative for the `db` facade.

## 7.6 Data-facet facades - catalog

The data model supports more facets than files and relational tables; each is a *facet* (a data
shape) reached through a *facade* (an interface surface). The target contract promotes each facet
from the 0013 catalog through its own normative facade doc and runtime capability. A promoted facade
is not optional for providers that advertise the capability, but a catalog row alone does not mean the
facade is implemented in source.

| Facet | Capability | Facade home |
| ----- | ---------- | ----------- |
| files / workspace history | (core) | `fs` (§4) + `vcs` (§5); `vcs` is the history facade name, not a facet, 0014 |
| relational | `sql` | `db`, §7.5 / 0011 |
| graph | `graph` | §7.7 / 0016 |
| vector | `vector` | §7.8 / 0017 |
| ledger | `ledger` | §7.9 / 0018 |
| key-value | `kv` | 0019 (ephemeral cache tier 0019a) |
| document | `document` | 0020 |
| append-log/queue | `queue` | 0021 |
| time-series | `time-series` | 0022 |
| columnar | `columnar` | 0023 |
| content-addressed store | `cas` | 0024 |
| coordination (locks) | `lock` | 0036 (control-plane facet; state non-versioned, not synced) |

A program reaches a **minimal, least-privilege subset** of any facet through the private
`StateAccess` surface (0015 §6) - a program-facing capability surface, not a public facade, and far
smaller. A new facet is added to the 0013 catalog first, then gets a facade doc and a row here.

**Current implementation:** source contains deterministic substrates for several facets and public
SQL surfaces, but most catalog rows do not yet have complete public facades, C ABI and language
binding projection, wire projection, and conformance. Their owning specs decide promotion order and
must not claim merge semantics that remain unresolved by `CONFLICT-RESOLUTION-MATRIX.md`.

## 7.7 Graph facade (`graph`) - summary

The target `graph` facade is present when the provider advertises the `graph` capability (absent
otherwise, and calling into it returns `UNSUPPORTED`). It exposes a
**versioned property graph** (nodes and edges) stored in the same object model. The facade
(`upsert_node`, `add_edge`, `out_edges`/`in_edges`/`neighbors`, `query`, ...) operates within a
workspace that has the `graph` facet (0014) and composes with version control and transactions (§6).
This section is a pointer; 0016 is normative for the `graph` facade.

## 7.8 Vector facade (`vector`) - summary

The target `vector` facade is present when the provider advertises the `vector` capability (absent
otherwise, and calling into it returns `UNSUPPORTED`). It exposes
**versioned embeddings with approximate nearest-neighbor search**. The facade (`upsert`,
`get`, `remove`, `search`, ...) operates within a workspace that has the `vector` facet (0014); the
embeddings are versioned while the ANN index is a derived, rebuildable artifact (0017 §3). This
section is a pointer; 0017 is normative for the `vector` facade.

## 7.9 Ledger facade (`ledger`) - summary

The target `ledger` facade is present when the provider advertises the `ledger` capability (absent
otherwise, and calling into it returns `UNSUPPORTED`). It exposes an
**append-only, hash-chained, verifiable** log. The facade (`append`, `get`, `len`, `scan`, `verify`)
operates within a workspace that has the `ledger` facet (0014); appends are the only mutation and
serialize through ref CAS (§9), and `verify` recomputes the chain. This section is a pointer; 0018 is
normative for the `ledger` facade.

## 8. Error taxonomy (normative)

Every failure surfaces a `LoomError { code, message }` whose `code` is one of the stable
`loom_core::error::Code` variants:

| Code | Meaning |
| --- | --- |
| `NOT_FOUND` | Path, ref, workspace, facet, object, or record does not exist. |
| `ALREADY_EXISTS` | Target already exists where creation expected none. |
| `CORRUPT_OBJECT` | An object failed canonical-form validation. |
| `INTEGRITY_FAILURE` | A digest did not match its address. |
| `UNSUPPORTED` | Provider or facade lacks the required capability. |
| `INVALID_ARGUMENT` | Malformed input or unsupported argument shape. |
| `IO` | Underlying I/O error. |
| `INTERNAL` | Unexpected invariant violation. |
| `CROSS_WORKSPACE` | An operation spanned two workspaces; disallowed by 0014. |
| `CAS_MISMATCH` | Compare-and-swap ref update saw an unexpected old value. |
| `NOT_FAST_FORWARD` | Push, pull, or ref advancement would not fast-forward. |
| `DIMENSION_MISMATCH` | A vector's dimension or metric does not match its fixed profile. |
| `PERMISSION_DENIED` | Authorization denied, or an OS permission failure. |
| `AUTHENTICATION_FAILED` | A presented credential did not resolve to a principal. |
| `IDENTITY_NO_ROOT_CREDENTIAL` | Authenticated mode was enabled without a usable root credential. |
| `TRIGGER_NOT_FOUND` | No trigger binding exists for the given id. |
| `TRIGGER_DENIED` | A trigger's `run_as` principal is unauthorized at fire time. |
| `CURSOR_INVALID` | A change-feed cursor is no longer reachable. |
| `E2E_LOCKED` | An end-to-end-encrypted workspace needs an unlocked data-encryption key. |
| `E2E_KEY_INVALID` | The presented secret did not unwrap the data-encryption key. |
| `CONFLICT` | A same-element merge collision, referential conflict, or failed owner-scoped conditional mutation rejected the operation. |
| `LOCKED` | A lock is held by another owner, or a bounded acquire timed out. |
| `LOCK_LEASE_EXPIRED` | A caller's lock lease elapsed before the operation. |
| `FENCING_STALE` | A fenced write presented an older token than the applied high-water mark. |
| `LOCK_NOT_HELD` | A release or refresh targeted a lock the caller does not hold. |

More granular target errors such as `NOT_A_DIRECTORY`, `WORKING_TREE_DIRTY`, `MERGE_CONFLICT`,
`NETWORK`, `TIMEOUT`, or `QUOTA_EXCEEDED` are not stable source codes today. They must be
added to `loom_core::error::Code` before 0003, 0008, bindings, or conformance can claim them as
machine-readable contract values.

Mapping to HTTP and JSON-RPC codes is defined in 0008 §6. Bindings MUST preserve `code` verbatim
(0007 §6) so cross-language and cross-protocol error handling is uniform.

Sync methods currently raise the stable core codes. For example, a `push` that would not
fast-forward raises `NOT_FAST_FORWARD`, not an opaque protocol error. Protocol-specific transport
codes are target work for 0006/0008 unless promoted into `loom_core::error::Code`.

## 9. Concurrency & consistency model (normative)

The two mutating facades follow different, deliberately familiar models, and concurrency is always
**within a single workspace** (there are no cross-workspace operations to coordinate, 0014 §5).

- **`fs` - filesystem semantics.** The working tree behaves like a local/POSIX filesystem (§4.5):
  each mutating operation applies when it is called, with no hidden per-handle overlay. Concurrent
  operations on the same path are last-writer-wins at operation granularity, and a reader observes
  whatever has been applied so far. This is exactly what callers expect of a real filesystem.
- **`vcs` - workspace history locking.** A mutating workspace-history operation (`commit`, `merge`,
  `rebase`, `branch_*`, `checkout`, ref updates) takes a per-workspace lock for its duration, using
  the same concurrency model as a local VCS command. Concurrent mutating `vcs` operations on the same
  workspace are serialized; a second one that cannot take the lock is rejected with `CONFLICT` rather
  than interleaving. At the storage layer this is the ref compare-and-swap of §5.10: a losing update
  returns `CAS_MISMATCH` to retry or merge.
- **Single-method atomicity (both):** every mutating method in §4-§5 is atomic and durable on success
  (subject to provider durability mode, 0004 §4 / 0005 §6).
- **Isolation (both):** readers see a consistent snapshot; a long read (`walk`, `log` stream) MUST
  observe the object graph as of its start, even if writers advance refs meanwhile, because objects
  are immutable and only ref moves are visible.
- **Cross-process:** the single-file backend supports multi-process access via the single-writer lock
  protocol (0005 §6.4).
- **Explicit locks (target):** leased, reentrant, fenced application- and runtime-level locks beyond
  this built-in model - and the optimistic compare-and-set primitives that should be preferred over
  them - are specified in **0036**. Lock state is per-coordinator control-plane state, kept out of the
  versioned object graph and never synced.

### 9.1 Conditional mutation and comparison anchors

0003 owns the shared public contract for conditional mutation. A facet owns its target identity,
atomic scope, mutation semantics, and token provenance. A facade owns its command grammar, header or
state-token representation, and response shape. Neither changes the core compare meaning.

Every guarded mutation has an owner-defined target and atomic scope, an owner-defined mutation, and
one condition:

- `any` applies without a caller-supplied precondition;
- `absent` requires that the target be absent in the same atomic scope;
- `exact` requires equality with an owner-issued current-state token;
- `generation` requires equality with an owner-defined monotonic generation; or
- `operation_anchor` requires an owner or coordination authority such as a valid fencing token.

Token bytes, syntax, lifetime, derivation, and disclosure remain owner-defined and opaque to this
shared contract. No universal ETag, digest, string, integer, or serialized token format is defined.
An entity tag is an optional facade rendering of a readable owner token, not a new native identity.

Authorization and policy are evaluated before condition evaluation. The condition and mutation commit
at one documented atomic read point. A failed condition performs no partial mutation. An ordered
batch may use these conditions for its entries only when all comparisons and all writes commit under
the Section 6 batch transaction boundary.

`absent` used by a create operation returns `ALREADY_EXISTS` when the target already exists. A failed
caller-supplied `absent`, `exact`, or `generation` precondition returns `CONFLICT` with an
owner-defined machine-readable disposition. A stale `operation_anchor` returns `FENCING_STALE`; a
missing or elapsed lease returns the existing lock error. `CAS_MISMATCH` remains the specific ref
compare-and-swap result. A condition failure does not disclose the protected current value, token, or
target existence beyond what the caller is already authorized to read.

On success, an owner may return a new anchor or token only when policy permits that disclosure. Facet
specifications must state their token provenance, permitted condition kinds, atomic scope, merge
boundary, and whether a facade may expose an entity tag. The shared contract does not make a sync
cursor, client cache key, foreign revision, or facade header a general Loom token.

#### 9.1.1 Target placement

Conditional behavior is placed with the owner that can define the target identity and atomic scope.
The following placement keeps public compare-token semantics single-sourced:

| Surface | Placement | Target scope |
| --- | --- | --- |
| Document text and binary | Owner-token surface | Document owns entity-tag provenance, absent and exact replacement, delete guards, and document-id atomic scope. Existing digest guards are legacy CAS compatibility and MUST NOT become a new native token name. |
| KV maps | Owner-token surface | KV owns per-entry exact tokens, absent creates, blind writes, deletes, and map replacement invalidation. The token is scoped to one typed key within one map. |
| Workspace history and refs | Existing CAS surface | Ref compare-and-swap remains the VCS and storage-layer conflict boundary. It keeps `CAS_MISMATCH` and does not consume document or KV tokens. |
| Versioned substrate roots, tickets, pages, and drive profiles | Owner-root guard surface | The owning model may use `expected_root` or equivalent current-root checks for its aggregate root. These are aggregate owner tokens, not shared entity tags. |
| Locks and operation fences | Operation-anchor surface | Lock fences and leases consume the `operation_anchor` condition kind and keep lock-specific stale or missing-lease errors. |
| Compute guards, derivations, workflows, triggers, and SQL transactions | Atomicity or validation surface | These features evaluate preconditions, invariants, derived freshness, or transaction rollback over their own proposed state. They SHOULD NOT expose first-class conditional mutation unless a future owner spec defines target identity, token provenance, and atomic scope. |
| Graph, ledger, time-series, search, vector, inference, admin, queue, PIM, and profile/import surfaces | No first-class conditional mutation by default | These surfaces may have idempotency, cursors, conflict logs, retention gaps, or freshness markers, but those values are not Loom compare tokens. Add conditional mutation only through an owner spec update or by consuming a shared owner-token primitive from document, KV, VCS, locks, or an aggregate-root owner. |
| Hosted, MCP, SQL-wire, S3, OCI, WebDAV, JMAP, IMAP, SMTP, OpenSearch, Qdrant, Pinecone, and binding facades | Adapter consumer | A facade may preserve protocol vocabulary such as `If-Match`, ETag, revision, generation, or offset only as a documented adapter into a native owner token, aggregate-root guard, CAS operation, or operation anchor. A facade MUST NOT define a second compare meaning for the same owner target. |

## Appendix A - Object body grammars

(Reference grammars for Blob/ChunkList/Tree/Commit/Tag canonical bodies; to be expanded to a
machine-checkable ABNF/Kaitai schema in the RFC-grade revision. Each body is the Loom Canonical CBOR
encoding (ADR-0010) referenced in 0002 §3.7.)

## Appendix B - Revision-expression resolution

(Formal grammar and resolution algorithm for `Rev`, including `~`, `^`, `..`, `...`, ref
disambiguation precedence: exact digest → full ref name → `branch/` → `tag/` → `remote/`.)

## Resolved decisions

1. **Workspace selector (was Q1) - first-class, per-call.** Every `fs`/`vcs`/`db` method takes a
   per-call `ns: NsSelector` first parameter; there is no implicit "current workspace" and no silent
   default (§2.1). Workspaces are a first-class design property, so this is mandatory rather than a
   convenience.
2. **`CROSS_WORKSPACE` (was Q2) - disallowed.** Cross-workspace operations are forbidden; the code is
   in the taxonomy (§8) and raised by any operation spanning two workspaces (§2.1, §6; 0014 §5).
3. **Staging modes (was Q3) - two defined.** `explicit` (default, git-like index) and `implicit`
   (`implicit-staging`, snapshot the whole working tree). Under `implicit`, `stage` is a no-op success
   and `unstage` returns `UNSUPPORTED` (§5.2).
4. **Write-handle semantics (was Q4) - POSIX, per-operation.** Each mutating handle op (`write`,
   `write_at`, `truncate`) applies to the working tree when called, not on `close()`; `close()` only
   releases the handle (§4.5).
5. **Concurrency (was Q4/Q5) - fs and vcs separately.** `fs` follows local-filesystem semantics; `vcs`
   takes a workspace-history lock and rejects a contending mutating op with `CONFLICT` (§9).
   Batches are read-your-writes with commit-time CAS validation (§6).
6. **Empty-tree `∅` (was Q6) - tree contexts only.** Accepted in `diff` and low-level tree ops;
   rejected from commit-expecting methods with `INVALID_ARGUMENT`, matching git (§5.11).
7. **Timestamp identity (was Q7) - UTC instant only.** Only the UTC instant is hash-affecting;
   `tz_minutes` is display metadata excluded from every digest (§3.1).
8. **Sync error overlap (was Q8) - stable core codes first.** Sync methods raise stable
   `loom_core::error::Code` values. Protocol-specific transport mappings remain target work unless
   promoted into the stable code enum (§8; 0006 §7; 0008 §6).
