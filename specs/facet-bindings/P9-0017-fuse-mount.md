# P9-0017 - FUSE Mount

**Series:** P9 binding plan (normative-track sub-series; Draft)
**Version:** 0.2.0-draft - **Status:** Source-backed local projection - **Last updated:** 2026-07-12
**Reads first:** [`P9-0003-files-binding.md`](./P9-0003-files-binding.md), 0003c (filesystem
projection), 0014 (workspaces), 0003 and 0003a (files facade and byte-range/symlink behavior), and
0032 (platform parity).

The cross-facet consumer that mounts a `files`/`vcs` workspace as a real folder so unmodified apps
(`ls`, `cp`, `vim`) operate on a `.loom`. Per-OS: **`fuser`** (Linux/macOS via libfuse/macFUSE - FSKit
user-space backend on macOS 26). Windows filesystem mounting remains target work. Native-only; shared
projection logic lives in `loom-vfs`.

## 0. Binding Boundary

FUSE is a local filesystem presentation over the `files` and `vcs` base layers. It is not a facet and
does not own canonical tree, commit, or ref identity. Kernel vnode state, attribute caches, open handles,
and mount options are runtime projection state. Hosted file protocols and object APIs belong to their own
served surfaces.

The source-backed CLI surface is:

```text
loom mount fuse <store> <workspace> <mountpoint> [--read-only]
```

NFS uses the same projection model through:

```text
loom mount nfs <store> <workspace> <mountpoint> [--listen <addr>] [--read-only]
```

## 1. Mount modes (resolved; owner; long-standing decision)

Two modes, **chosen at mount**:

| Mount target | Access | Semantics |
| --- | --- | --- |
| a checked-out immutable revision | **read-only** | materializes that revision's tree before serving; writes return `EROFS` |
| the **tip** (a branch's working tree) | **read-write by default**, or **read-only via `--read-only`** | edits apply to the working tree; commits are **explicit** (`loom vcs commit`) |

The current implementation mounts a workspace working tree through `loom-vfs`. A read-write tip mount
does **not** auto-commit. Edits accumulate in the working tree until an explicit commit operation.
Auto-commit/debounce remains out of the source-backed v1 surface.

## 2. FUSE op -> `fs` facade mapping

| FUSE op | `fs` facade | Notes |
| --- | --- | --- |
| `lookup`/`getattr` | `Projection::lookup` / `getattr` | kind/size/mode from `loom-vfs::Attr` |
| `readdir` | `Projection::readdir` | deterministic child listing |
| `open`/`read` | `Projection::read` | bounded byte-range reads |
| `write` | `Projection::write` | positional writes; gaps are zero-filled by the file facade |
| `create`/`mkdir` | `Projection::create` / `mkdir` | persisted after mutation |
| `unlink`/`rmdir` | `Projection::unlink` / `rmdir` | persisted after mutation |
| `rename` | `Projection::rename` | source and destination path authorization |
| `symlink`/`readlink` | `Projection::symlink` / `readlink` | source-backed |
| `truncate` | `Projection::truncate` | source-backed |
| `getxattr`/`listxattr` | `Projection::metadata` | `user.loom.status`, `user.loom.error`, and `user.loom.etag` metadata |
| `setxattr` | none | not part of the source-backed v1 contract |

## 3. Read/write semantics

- **Immutable revision mount (RO):** the engine checks out the revision's tree as an immutable view; all mutating FUSE
  ops return `EROFS`. Safe for inspection, diffing, building against a pinned revision.
- **Tip mount (RW):** mutations call the `fs` working-tree ops live; `loom vcs commit` snapshots
  the working tree into a commit. A tip mounted `-o ro` behaves like a live read-only view that tracks ref
  advances.
- **Authority:** the mounted projection uses the same file-path ACL matrix as `loom-vfs`; denied access
  maps to `EACCES`, and a read-only mount maps writes to `EROFS`.
- **Single live engine:** a mount owns one live file-backed engine. Concurrent commit, checkout, merge,
  or restore behavior follows the working-tree model in 0003c.

## 4. Source-backed closeout

The old F11 blocker is closed for the local projection path. `loom-vfs` and `loom-vfs-fuse` now have
source-backed support for positional writes, truncate, create, delete, mkdir, rmdir, rename, symlink,
readlink, read-only rejection, ACL-backed denial, reserved `.loom` hiding, and metadata/xattr
projection. The FUSE crate adds adapter-level tests for mount-option selection, portable attribute
mapping, and errno mapping. Real kernel mounting remains platform-dependent and is covered by the
manual `prototypes/loom-fuse-smoke` recipe from 0003c.

## 5. Platform, errors, and remaining target work

- **Parity (0032):** native-only. Linux/BSD use the `fuser` backend without a native libfuse build
  dependency; macOS FUSE requires macFUSE. NFS is the driverless macOS/Linux path. No `wasm32` mount.
- **Errors:** stable `Code` values map to POSIX errno (`NOT_FOUND` -> `ENOENT`,
  `PERMISSION_DENIED` -> `EACCES`, `UNSUPPORTED` -> `EROFS`, `INVALID_ARGUMENT` -> `EINVAL`,
  `ALREADY_EXISTS` -> `EEXIST`).
- **Target work:** arbitrary user xattr mutation, Windows mount backend, hosted/network FUSE, and
  auto-commit/debounce policy are not source-backed v1 behavior.
