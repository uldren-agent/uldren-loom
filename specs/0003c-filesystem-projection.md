# 0003c - Filesystem projection (FUSE / NFS / hosted)

**Status:** Source-backed. The portable projection layer (`loom-vfs`) and both committed platform
backends (FUSE via `loom-vfs-fuse`, NFSv3 via `loom-vfs-nfs`) are implemented, wired into the CLI
(`loom mount fuse` / `loom mount nfs`), and recorded in the 0032 parity matrix and the 0008
cross-reference.
The shared projection policy, backend-neutral metadata model, and user-facing reserved-tree hiding are
also source-backed in `loom-vfs`.
**Version:** 0.1.0.
**Normative:** the portable projection model, the platform matrix, and the errno mapping.
**Informative:** the per-backend effort assessment.

**Depends on:** 0003 (the `fs` facade), 0003a (byte-range I/O, file handles, symlinks), 0014
(workspaces). **Relates to:** 0008 (wire protocols - FUSE and hosted transports), 0032 (native/web
platform parity).

A *projection* exposes a workspace working tree as an actual filesystem to programs that are not loom
clients - so a user can `cd` into a mounted folder, or a tool can open files by ordinary OS paths -
rather than only through the C ABI or bindings. Every platform mechanism (FUSE, NFS, hosted protocols)
is a thin adapter over one portable projection layer; the filesystem semantics are written and tested
once, in `loom-vfs`, with no platform or native dependency.

## 1. Portable projection model (source-backed: `loom-vfs`)

A backend speaks an inode/handle filesystem protocol; the loom `fs` facade is path-based.
`loom_vfs::Projection<S>` bridges the two:

- It owns a `Loom<S>`, targets one workspace, and keeps a stable **inode <-> path** table (the root
  working-tree path is inode `1`; child inodes are allocated on first `lookup`/`readdir`).
- It exposes the operation surface a backend needs, each mapping directly to a loom working-tree op:
  `lookup`, `getattr`, `readdir` (real children only; backends add `.`/`..`), `read` (bounded chunk
  read), `write` (positional), `create`, `mkdir`, `unlink`, `rmdir`, `rename`, `truncate`, `symlink`,
  `readlink`.
- `Attr { ino, kind: File|Dir|Symlink, size, mode }` is the `stat`-shaped subset a backend reports.
- After a delete or rename the affected inodes are forgotten so they re-resolve on the next lookup
  rather than pointing at stale paths.

This layer is `wasm32`-clean: it has no FUSE/NFS/native dependency, so it satisfies the 0032 rule that
the portable contract is the source of truth and native backends sit outside `loom-core`.

### 1.1 Modes

- **ReadWrite** - mutations land in the workspace working tree. Committing is a separate `vcs`
  operation; the projection never auto-commits.
- **ReadOnly** - reads only; every mutating op returns `EROFS`. A read-only snapshot of a specific
  revision is established by checking that revision out (`checkout_commit`) before serving, then
  projecting read-only.

### 1.1a Reserved paths and user-facing visibility

The projection calls the `fs` facade, which rejects user writes anywhere in the reserved `.loom`
subtree (facet storage and Loom metadata) per the 0014a baseline. Through a mount this surfaces as
`EACCES`, so an ordinary `rm -rf .loom` or a write into `.loom/facets/...` cannot corrupt facet
storage. The guard lives in `loom-core`, so it protects FUSE and NFS mounts identically without
backend-specific code, and it is uniform across facets (no per-facet carve-out); facet implementations
write their storage through privileged methods.

The user-facing projection is a virtual workspace, not a promise to publish internal storage paths.
Direct core APIs may still inspect reserved storage for diagnostics and facet implementation work, but
`loom-vfs` hides the internal `.loom` tree from mounted projections. User-facing facet files appear only
through facet-declared projection roots and handlers, such as the source-backed calendar/contacts/mail
overlay roots. A facet storage path under `.loom/facets/<facet>/...` does not become a public file API
unless that facet's owning spec opts in.

### 1.1b Projection policy and ACL matrix

`loom-vfs` owns the shared projection-policy matrix for local FUSE, local NFS, and future hosted
filesystem projections. The backends stay thin: they call one portable `Projection` operation and
translate the resulting stable error code. The policy matrix maps each projection operation to the core
ACL right before the operation can reveal data or mutate state:

| Projection operation | Required core right | Notes |
| --- | --- | --- |
| `lookup`, `getattr`, `readdir`, `read`, `readlink` | Files `Read` on the path | Directory listing is read-like because it reveals names. |
| `getxattr`, `listxattr`, `metadata` | Files `Read` on the path | Metadata may reveal validation status, errors, or ETags. |
| `write`, `create`, `mkdir`, `unlink`, `rmdir`, `truncate`, `symlink`, `flush_overlay` | Files `Write` on the path | Read-only projections reject these before ACL evaluation with `EROFS`. |
| `rename` | Files `Write` on both source and destination paths | Source and destination are checked before mutation. |

Core remains the source of truth for ACL evaluation. `loom-core` exposes a file-path authorization
helper that normalizes projection paths, including the root path, and evaluates the same ACL store as
the direct file facade. This makes FUSE, NFS, and future hosted filesystem transports share one policy
contract instead of reimplementing authorization per adapter.

### 1.1c Projection metadata and facet handlers

Projection metadata is one backend-neutral model. `ProjectionMetadata` records the projection status,
optional validation error, and optional ETag. FUSE maps it to `user.loom.*` extended attributes; hosted
protocols can map it to response metadata; protocols that cannot carry the full model report a degraded
or absent metadata view without changing the canonical semantics.

`loom-vfs` owns traversal, inode identity, policy, and metadata envelopes. Facet-specific conversion is
behind a `ProjectionFacet` interface. The current built-in handler covers the source-backed
calendar/contacts/mail roots by classifying paths, listing projected records, serializing records on
read, ingesting written files on flush/reconcile, and returning canonical metadata. Future facets extend
that handler model from their owning specs rather than adding ad hoc backend logic.

### 1.2 Concurrency with history

Projection ops are per-operation-immediate over the working tree, the same model as the `fs` facade and
the open-file/inode layer (0003a, 0003 section 9). A `commit`/`checkout`/`merge`/`rebase` run against the same
engine while a mount is live changes the working tree under the mount; backends should expect to
re-resolve inodes (the layer already forgets renamed/removed paths). A single engine is not shared
across OS processes; a mount holds one live engine.

### 1.2a Conditional mutation projection

The projection consumes the conditional-mutation contract owned by 0003 section 9.1 when a file or
facet write is promoted with a comparison condition. The comparison anchor, atomic read point, and
merge boundary remain those of the owning native file or facet operation. FUSE and NFS do not define a
second conditional-write model; their ordinary write and close behavior remains the current immediate
projection behavior. A hosted WebDAV projection may map native conditions to its precondition headers,
but an HTTP ETag or a backend inode is not a universal Loom comparison token.

The backend resolves its principal and authorizes the requested write before invoking the native
operation. It then preserves the native redaction and audit requirements from 0009 and maps the stable
0003 section 8 outcome to its protocol error. A failed comparison does not expose file content,
protected metadata, or an opaque anchor merely because a mount or hosted client supplied a stale value.

### 1.3 Error mapping

`loom_vfs::errno(Code)` maps loom error codes to POSIX `errno` for backends: `NotFound` -> `ENOENT`,
`AlreadyExists` -> `EEXIST`, `InvalidArgument` -> `EINVAL`, `PermissionDenied` -> `EACCES`,
`Unsupported` -> `EROFS` (the read-only / unsupported-op case), `Io` and the remaining
internal/integrity codes -> `EIO`.

## 2. Platform matrix

| Backend | Platforms | Driver needed | Status | Notes |
| --- | --- | --- | --- | --- |
| FUSE | Linux (required), macOS (optional) | Linux: none; macOS: macFUSE | **Source-backed** (`loom-vfs-fuse` + CLI `mount`) | Canonical "mount as a folder". Native `fuser`/libfuse dependency lives in the separate `loom-vfs-fuse` crate and a feature-gated CLI `mount` subcommand, never in core. macOS works only if the user installed macFUSE, so it is not the primary macOS path. Mounts unprivileged in the CI sandbox via a mapped-root user+mount workspace (`unshare -Urm --map-root-user`); where the setuid `fusermount3` path is restricted, the integration test uses that wrapper. |
| NFSv3 | macOS, Linux | none (built-in client) | **Source-backed** (`loom-vfs-nfs` + CLI `mount`) | The driverless macOS path. An in-process ONC RPC + MOUNT + NFSv3 server (the `nfsserve` crate) over TCP; `AUTH_UNIX` (uid/gid, no crypto). `loom mount nfs` runs the OS mount for you (it needs `sudo`: `mount_nfs` on macOS, `mount -t nfs` on Linux). Pure Rust (no native lib), but it pulls a tokio runtime, so the dependency lives in the separate `loom-vfs-nfs` crate, never in core; tokio is confined there with a lean feature set (net, io-util, sync, fs, rt, macros) on a single-threaded runtime. |
| SMB | macOS, Windows, Linux | none (built-in client) | **Declined** | Implementing an SMB2/3 server is disproportionate: large protocol surface (Create/QueryInfo/oplocks/leases/change-notify), mandatory SPNEGO/NTLMv2 auth crypto, and no mature Rust SMB *server* crate. The maintenance and security cost is not justified when NFSv3 already gives driverless macOS mounts. |

## 3. Promotion requirements (per backend)

Before a backend is marked source-backed in 0008 / 0032:

- (P0) a backend crate (or feature) over `loom-vfs`, outside `loom-core`, that keeps `loom-core` and
  `loom-vfs` free of its native dependency;
- (P0) a mount entry point (CLI subcommand or library API) and documented mount instructions per OS;
- (P1) an integration test that mounts, exercises read/write/readdir/rename/symlink, and unmounts,
  where the platform allows it in CI; otherwise a compile-gated build plus a documented manual recipe;
- (P0) the 0032 parity row and the 0008 protocol entry updated only after the test or recipe exists.

## Change log

### P2 - portable projection layer (`loom-vfs`) source-backed

The portable projection layer is implemented and source-backed.

- **Crate (`crates/loom-vfs`).** New `wasm32`-clean crate over `loom-core`. `Projection<S>` with the
  inode <-> path table, `Attr` / `NodeKind` / `Mode`, the operation surface (`lookup` / `getattr` /
  `readdir` / `read` / `write` / `create` / `mkdir` / `unlink` / `rmdir` / `rename` / `truncate` /
  `symlink` / `readlink`), stale-path forgetting on delete/rename, and the `errno` mapping. No native
  dependency; not in `loom-core`.
- **Tests.** Unit tests drive the mapping directly over `Loom<MemoryStore>`: read/write/readdir
  round-trip through inodes, truncate + positional write, create/unlink/mkdir/rmdir, symlink/readlink,
  rename (content moves, path re-resolves), read-only mode rejecting mutations with `EROFS`, ACL
  rejection, reserved-tree hiding, facet overlay metadata, bad-name and unknown-inode rejection, and
  the errno mapping.
- **Decisions recorded.** Backends are thin adapters over this one layer. FUSE (Linux required, macOS
  optional behind macFUSE) and NFSv3 (driverless macOS + Linux) are the two committed backends. SMB is
  declined (no Rust server crate; SMB2/3 + auth complexity). WebDAV and the OPFS / File System Access
  working-tree projection are **out of scope** (eliminated, not deferred): the browser has no mount
  mechanism, so OPFS could only be an in-app File System Access API shim - too large for the value - and
  WebDAV's only advantage (driverless macOS) is already covered by NFSv3.

### P2 - FUSE backend (`loom-vfs-fuse`) source-backed

The FUSE backend is implemented and source-backed (Linux; macOS with macFUSE).

- **Crate (`crates/loom-vfs-fuse`).** A `fuser` 0.17 adapter over `loom-vfs`: `LoomFuse` implements
  `fuser::Filesystem` (lookup, getattr, setattr/truncate, readlink, mkdir, unlink, rmdir, symlink,
  rename, read, write, create, readdir, open/flush/fsync), locking a `Mutex<Projection<FileStore>>`,
  translating loom errors via `loom_vfs::errno`, and persisting (`save_loom`) after each mutation.
  Public `mount` (blocking) and `spawn` (background) entry points. Not in `loom-core` or `loom-vfs`.
- **CLI.** A `loom mount fuse <store> <workspace> <mountpoint> [--read-only]` subcommand behind the
  `fuse` feature, which is on by default (see the build note). A read-write `loom mount fuse` /
  `loom mount nfs` auto-creates the projected workspace if it does not exist; a read-only mount still
  requires the workspace to exist. `loom workspace list <store>` prints the workspace table
  (id, name, facets, head).
- **Manual mount check.** Real kernel-mount verification lives in `prototypes/loom-fuse-smoke` (a
  manual tool excluded from the workspace gate): it creates an on-disk loom, mounts it, and through the
  kernel reads the seed file, writes a new file, makes a subdirectory with a file, lists the directory,
  and unmounts, printing `SMOKE: OK`. On Linux run it under a mapped-root user workspace
  (`unshare -Urm --map-root-user`); verified green here. The portable mapping it exercises is covered by
  the `loom-vfs` unit tests in the gate.
- **Build note.** The `fuse` feature is **on by default**. On Linux/BSD it needs no native library:
  `fuser` (no default features) uses its pure-Rust mount implementation, so the backend builds and
  mounts for real with no libfuse, pkg-config, or dev package (mounting needs `/dev/fuse` plus a setuid
  `fusermount3` or a user workspace). On macOS, FUSE mounting is a kernel extension and requires macFUSE
  linked at build time; the `just` recipes detect macFUSE (via pkg-config) and build FUSE when present,
  and gracefully skip the FUSE crate - building/checking the NFS-only CLI - when it is absent, so the
  gate still runs on a Mac without macFUSE. The driverless macOS mount path is the NFS backend. Build a
  smaller NFS-only CLI with `just build-no-fuse` (`--no-default-features --features nfs`).

### P2 - NFSv3 backend (`loom-vfs-nfs`) source-backed

The NFSv3 backend is implemented and source-backed (driverless macOS and Linux mounts).

- **Crate (`crates/loom-vfs-nfs`).** An `nfsserve` 0.11 adapter over `loom-vfs`: `LoomNfs` implements
  the `NFSFileSystem` trait (lookup, getattr, setattr/truncate, read with EOF, write, create,
  create_exclusive, mkdir, remove [dispatching REMOVE vs RMDIR on the target kind], rename, readdir
  [deterministic, cookie-paginated by fileid], symlink, readlink), locking a
  `Mutex<Projection<FileStore>>` per procedure (never across an `.await`), translating loom errors to
  `nfsstat3` via the shared `loom_vfs::errno` mapping, and persisting (`save_loom`) after each mutation.
  Public `serve` (async) and `serve_blocking` entry points run the ONC RPC/MOUNT/NFSv3 server over TCP.
  Pure Rust (no native NFS dependency); outside `loom-core`/`loom-vfs`.
- **Dependency footprint.** `nfsserve` is taken with default features only (no `demo`), so the heavy
  `tracing-subscriber` and `intaglio` deps and `tokio/rt-multi-thread` stay out. `nfsserve` pins tokio
  to `net, io-util, sync, fs, rt, macros`, which is the union that fixes the tokio surface for the
  crate; this crate's own `tokio` dependency is `default-features = false, features = ["rt"]` (a subset
  that adds nothing) and the server runs on a single-threaded runtime. tokio is the only async runtime
  pulled, and it is confined to this crate. (Alternatives surveyed: `zerofs_nfsserve` - maintained fork,
  heavier tree, `tracing-subscriber` non-optional; `nfs3_server` - newer family, different API;
  `fractal-nfs` - runtime-agnostic but v0.1.0. `nfsserve` chosen for the leanest tree and a stable
  trait.)
- **CLI.** A feature-gated `loom mount nfs <store> <workspace> <mountpoint> [--listen
  127.0.0.1:12049] [--read-only]` subcommand: it starts the in-process NFS server on a background
  thread, waits until it accepts connections, then runs the OS mount (`sudo mount_nfs` on macOS, `sudo
  mount -t nfs` on Linux - NFS mounts need root) against its own port, and unmounts (`sudo umount`) on
  Ctrl-C via a handler (the `ctrlc` crate, since the workspace forbids raw `unsafe` signal handlers).
  The `nfs` feature is **on by default** (pure Rust server, driverless mounts); disable it with
  `--no-default-features`.
- **Integration test.** The sandbox has no kernel NFS client (no `mount.nfs`, NFS absent from
  `/proc/filesystems`), so promotion uses trait-level integration plus a documented manual recipe.
  Three tests over an on-disk `Loom<FileStore>` drive the `NFSFileSystem` trait on a current-thread
  runtime: a full round trip (lookup/read seed, create+write, mkdir+child, deterministic readdir,
  symlink, rename, remove), read-only mode rejecting mutations with `ROFS` while still serving reads,
  and the server binding its NFS/MOUNT TCP listener (proving the RPC stack wires up). The manual
  `mount_nfs -o vers=3` recipe is in the platform matrix above.
- **Build note.** `loom-vfs-nfs` is a default workspace member and the CLI `nfs` feature is default-on,
  so `cargo build --workspace` now compiles `nfsserve` + tokio. No native library is required (unlike
  the FUSE backend).
### P2 - projection closeout: parity, cross-reference, verification

The projection is recorded in the cross-cutting specs and the slice is verified end to end.

- **0032 platform parity.** Added a "Local filesystem projection (mount as a folder)" matrix row
  (native source-backed via `loom-vfs-fuse` / `loom-vfs-nfs` over `loom-vfs`; web not applicable, OPFS /
  File System Access and WebDAV eliminated) and a new section 4.7. Clarified the prose and the
  "hosted protocol and server role absence" section so "FUSE" there means the *hosted/network* role
  (still target), distinct from the source-backed local mount.
- **0008 wire protocols.** Added a cross-reference distinguishing the hosted/network FUSE shape (target,
  served with the section 9 authorization rules) from the local filesystem projection (source-backed in
  0003c); a future hosted FUSE endpoint reuses the same `loom-vfs` semantics over a transport.
- **Conformance.** The portable inode/path layer is covered by `loom-vfs`'s unit tests and
  `loom-vfs-nfs`'s trait-level tests; real kernel-mount verification is the `prototypes/loom-fuse-smoke`
  manual tool. This meets the promotion rule (a test, plus a documented manual recipe); a dedicated
  cross-implementation conformance vector in `loom-conformance` is deferred until a second, independent
  projection implementation exists.
- **CLI ergonomics folded in.** `workspace list` prints an aligned table showing each workspace's
  facets; read-write `loom mount fuse` / `loom mount nfs` auto-create the projected workspace if it
  does not exist; `loom mount nfs` runs the OS mount and unmounts on Ctrl-C.
- **Verification.** `cargo test --workspace`, `cargo clippy --workspace --all-targets -D warnings`,
  ASCII/dash scans, and `git diff --check` run clean here; `just all` / `just test-bindings` are run
  separately on a full toolchain.

### P0 - shared projection policy, metadata, and virtual reserved-tree contract

The projection policy decisions are implemented in source.

- **Shared policy.** `loom-vfs::policy` defines the backend-neutral operation matrix and gates every
  portable `Projection` operation before data is revealed or mutated. `loom-core` exposes a normalized
  file-path authorization helper so policy evaluation stays in the core ACL engine while FUSE/NFS
  remain thin adapters.
- **Directory-list protection.** Authenticated projections no longer leak names through `readdir` when
  the principal lacks Files `Read`; the denial maps through the shared `PermissionDenied` to `EACCES`.
- **Virtual reserved tree.** The mounted projection hides internal `.loom` storage while direct core APIs
  keep diagnostic access. The test suite proves core can list reserved storage while the projection
  neither lists nor resolves `.loom`.
- **Metadata envelope and facet handler.** `ProjectionMetadata` is the canonical metadata model, and the
  built-in `ProjectionFacet` handler owns the current calendar/contacts/mail overlay behavior. FUSE
  continues to expose metadata through xattrs; future hosted surfaces should map the same model into
  their response metadata.

## Active filesystem projection certification owner gate

Completion state: active implementation owner. The portable projection layer, FUSE backend, NFSv3
backend, shared projection policy, backend-neutral metadata envelope, reserved-tree hiding, and local
CLI mount wiring are source-backed. Hosted or network projection reuse, future independent-backend
conformance, platform-specific release reporting, and response metadata mapping remain implementation
work.

Decision Points: none.

| Gate | Source-backed evidence | Remaining implementation work | Disposition |
| --- | --- | --- | --- |
| Local projection baseline | `loom-vfs`, `loom-vfs-fuse`, `loom-vfs-nfs`, and CLI mount commands are implemented over the shared projection policy. | Keep local portable, FUSE, and NFS evidence separate from hosted or network projection proof in capability and release reports. | Source-backed local subset. |
| Hosted or network projection reuse | 0008 records hosted or network filesystem projection as future work that should reuse the same `loom-vfs` semantics. | Promote any hosted or network filesystem projection only after auth/session propagation, ACL checks, reserved-tree behavior, error mapping, and transport conformance are implemented. | Target P0. |
| Response metadata mapping | `ProjectionMetadata` defines the backend-neutral status, validation error, and ETag envelope; FUSE maps it to xattrs. | Map the same metadata model into each promoted hosted or network response surface, including degraded or absent metadata reporting where the protocol cannot carry it. | Target P0. |
| Backend and platform certification | FUSE has manual kernel smoke evidence; NFS has trait-level on-disk tests, listener bind evidence, and a documented manual mount recipe. | Report backend, platform, driver dependency, manual-proof, degraded, and unsupported states explicitly. Add cross-implementation vectors only after a second independent backend exists. | Target P0 for release reporting and future vectors. |
