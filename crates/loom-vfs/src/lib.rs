//! Portable filesystem-projection layer over a Uldren Loom working tree.
//!
//! A platform backend (FUSE, NFSv3, ...) speaks an inode/handle filesystem protocol; the loom `fs`
//! facade is path-based. [`Projection`] bridges the two: it owns a [`Loom`], targets one workspace,
//! keeps a stable inode <-> path table (allocate-on-lookup), and exposes the operation surface a
//! backend needs ([`Projection::lookup`], `getattr`, `readdir`, `read`, `write`, `create`, `mkdir`,
//! `unlink`, `rmdir`, `rename`, `truncate`, `symlink`, `readlink`). Each method maps to the loom
//! working-tree ops, so the FS semantics are written and tested once, here, with no platform or native
//! dependency. Backends translate [`errno`] for protocol error codes.
//!
//! Two [`Mode`]s: [`Mode::ReadWrite`] mutates the working tree (callers `commit` through `vcs`
//! separately); [`Mode::ReadOnly`] serves the current working tree and rejects mutations with `EROFS`
//! (a read-only snapshot of a specific revision is established by checking that revision out before
//! mounting). Licensed under BUSL-1.1.

pub mod facet;
pub mod metadata;
pub mod overlay;
pub mod policy;

use std::collections::BTreeMap;

use loom_core::error::{Code, LoomError, Result};
use loom_core::provider::ObjectStore;
use loom_core::workspace::{WorkspaceId, is_reserved_path};
use loom_core::{FileKind, Loom};

use crate::facet::{BuiltInFacetProjection, ProjectionFacet};
use crate::metadata::ProjectionMetadata;
use crate::policy::{ProjectionOperation, ProjectionPolicy};

/// The inode of the projection root (the workspace working-tree root). POSIX filesystems expect the
/// root inode to be `1`.
pub const ROOT_INO: u64 = 1;

/// What an inode resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// A regular file.
    File,
    /// A directory.
    Dir,
    /// A symbolic link (read its target with [`Projection::readlink`]).
    Symlink,
}

/// Metadata a backend reports for an inode (POSIX `stat`-shaped subset).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attr {
    /// Stable inode number.
    pub ino: u64,
    /// File, directory, or symlink.
    pub kind: NodeKind,
    /// Byte length (target length for a symlink; 0 for a directory).
    pub size: u64,
    /// POSIX mode bits (type + permissions), suitable to hand a backend directly.
    pub mode: u32,
}

/// One entry returned by [`Projection::readdir`] (real children only; backends add `.`/`..`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirItem {
    /// The child's own name (no path separators).
    pub name: String,
    /// The child's stable inode number.
    pub ino: u64,
    /// File, directory, or symlink.
    pub kind: NodeKind,
}

/// Whether the projection permits mutations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Reads and writes; mutations land in the workspace working tree.
    ReadWrite,
    /// Reads only; every mutating operation returns `EROFS`.
    ReadOnly,
}

impl Mode {
    pub fn check_writable(self) -> Result<()> {
        match self {
            Mode::ReadWrite => Ok(()),
            Mode::ReadOnly => Err(LoomError::new(Code::Unsupported, "read-only projection")),
        }
    }
}

/// Map a loom [`Code`] to a POSIX `errno` for a projection backend. Best-effort: `Unsupported` maps to
/// `EROFS` (the read-only / unsupported-operation case in this layer).
pub fn errno(code: Code) -> i32 {
    match code {
        Code::NotFound | Code::TriggerNotFound | Code::CursorInvalid | Code::SqlTableNotFound => 2, // ENOENT
        Code::Io => 5, // EIO
        Code::PermissionDenied
        | Code::AuthenticationFailed
        | Code::IdentityNoRootCredential
        | Code::TriggerDenied => 13, // EACCES
        Code::AlreadyExists => 17, // EEXIST
        Code::InvalidArgument
        | Code::DimensionMismatch
        | Code::SqlSyntax
        | Code::SqlConstraintViolation
        | Code::SqlTypeMismatch
        | Code::SqlExecutionFailed => 22, // EINVAL
        Code::Unsupported => 30, // EROFS (read-only projection / unsupported op)
        _ => 5,        // EIO for the remaining internal/integrity codes
    }
}

/// A filesystem projection of one workspace working tree, bridging an inode/callback backend to the
/// path-based loom `fs` ops.
#[derive(Debug)]
pub struct Projection<S: ObjectStore> {
    loom: Loom<S>,
    ns: WorkspaceId,
    mode: Mode,
    policy: ProjectionPolicy,
    facets: BuiltInFacetProjection,
    /// Inode -> normalized working-tree path (the root inode maps to the empty path).
    ino_to_path: BTreeMap<u64, String>,
    /// Reverse map, so a path keeps a stable inode across lookups.
    path_to_ino: BTreeMap<String, u64>,
    /// Next inode to hand out.
    next_ino: u64,
}

impl<S: ObjectStore> Projection<S> {
    /// Wrap `loom`, projecting workspace `ns` under `mode`. The root path (`""`) is inode [`ROOT_INO`].
    pub fn new(loom: Loom<S>, ns: WorkspaceId, mode: Mode) -> Self {
        let mut ino_to_path = BTreeMap::new();
        let mut path_to_ino = BTreeMap::new();
        ino_to_path.insert(ROOT_INO, String::new());
        path_to_ino.insert(String::new(), ROOT_INO);
        Self {
            loom,
            ns,
            mode,
            policy: ProjectionPolicy,
            facets: BuiltInFacetProjection,
            ino_to_path,
            path_to_ino,
            next_ino: ROOT_INO + 1,
        }
    }

    /// Shared access to the wrapped engine (e.g. to `commit` through `vcs`).
    pub fn loom(&self) -> &Loom<S> {
        &self.loom
    }

    /// Mutable access to the wrapped engine (setup, `commit`, `checkout`).
    pub fn loom_mut(&mut self) -> &mut Loom<S> {
        &mut self.loom
    }

    /// Consume the projection and return the engine.
    pub fn into_loom(self) -> Loom<S> {
        self.loom
    }

    /// The projected workspace.
    pub fn workspace(&self) -> WorkspaceId {
        self.ns
    }

    /// The projection mode.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    // ---- inode table ---------------------------------------------------------------------------

    /// The stable inode for `path`, allocating one if unseen.
    fn intern(&mut self, path: &str) -> u64 {
        if let Some(ino) = self.path_to_ino.get(path) {
            return *ino;
        }
        let ino = self.next_ino;
        self.next_ino += 1;
        self.ino_to_path.insert(ino, path.to_string());
        self.path_to_ino.insert(path.to_string(), ino);
        ino
    }

    /// The path an inode resolves to, or `ENOENT` if the inode is unknown.
    fn path_of(&self, ino: u64) -> Result<String> {
        self.ino_to_path
            .get(&ino)
            .cloned()
            .ok_or_else(|| LoomError::not_found(format!("inode {ino}")))
    }

    /// Forget `path` and any descendant paths from the inode table (after a delete or rename), so the
    /// inodes are re-resolved on the next lookup rather than pointing at stale paths.
    fn forget_subtree(&mut self, path: &str) {
        let prefix = format!("{path}/");
        let stale: Vec<String> = self
            .path_to_ino
            .keys()
            .filter(|p| p.as_str() == path || p.starts_with(&prefix))
            .cloned()
            .collect();
        for p in stale {
            if let Some(ino) = self.path_to_ino.remove(&p) {
                self.ino_to_path.remove(&ino);
            }
        }
    }

    /// Join a parent path and a single-component child name, validating the name.
    fn child_path(&self, parent_ino: u64, name: &str) -> Result<String> {
        if name.is_empty() || name == "." || name == ".." || name.contains('/') {
            return Err(LoomError::invalid(format!("invalid name {name:?}")));
        }
        let parent = self.path_of(parent_ino)?;
        Ok(if parent.is_empty() {
            name.to_string()
        } else {
            format!("{parent}/{name}")
        })
    }

    fn authorize(&self, op: ProjectionOperation, path: &str) -> Result<()> {
        self.policy
            .authorize(&self.loom, self.ns, self.mode, op, path)
    }

    fn guard_visible(path: &str) -> Result<()> {
        if is_reserved_path(path) {
            return Err(LoomError::not_found(format!("{path:?}")));
        }
        Ok(())
    }

    // ---- attributes ----------------------------------------------------------------------------

    /// The attributes for an already-interned `ino` at `path`.
    fn attr_at(&self, ino: u64, path: &str) -> Result<Attr> {
        if path.is_empty() {
            // The root is always a directory.
            return Ok(Attr {
                ino,
                kind: NodeKind::Dir,
                size: 0,
                mode: 0o040000 | 0o755,
            });
        }
        let st = self.loom.stat(self.ns, path)?;
        let (kind, mode) = match st.kind {
            FileKind::Directory => (NodeKind::Dir, 0o040000 | 0o755),
            FileKind::Symlink => (NodeKind::Symlink, st.mode | 0o777),
            FileKind::File => (NodeKind::File, st.mode),
        };
        Ok(Attr {
            ino,
            kind,
            size: st.size,
            mode,
        })
    }

    // ---- operations ----------------------------------------------------------------------------

    /// Resolve `name` within directory `parent`, returning its attributes (allocates the child inode).
    /// `ENOENT` if it does not exist.
    pub fn lookup(&mut self, parent: u64, name: &str) -> Result<Attr> {
        let path = self.child_path(parent, name)?;
        Self::guard_visible(&path)?;
        self.authorize(ProjectionOperation::Lookup, &path)?;
        if !self.loom.exists(self.ns, &path)? {
            // No working-tree file: a facet file backed by a record projects as a regular file.
            if let Some(size) = self.projected_size(&path)? {
                let ino = self.intern(&path);
                return Ok(Attr {
                    ino,
                    kind: NodeKind::File,
                    size,
                    mode: 0o100644,
                });
            }
            return Err(LoomError::not_found(format!("{path:?}")));
        }
        let ino = self.intern(&path);
        self.attr_at(ino, &path)
    }

    /// The byte length of a facet file's projected record, or `None` if the path is not a facet file or
    /// has no backing record.
    fn projected_size(&self, path: &str) -> Result<Option<u64>> {
        match self.facets.classify(path) {
            Some(f) => Ok(self
                .facets
                .project(&self.loom, self.ns, &f)?
                .map(|b| b.len() as u64)),
            None => Ok(None),
        }
    }

    /// Attributes for `ino`.
    pub fn getattr(&self, ino: u64) -> Result<Attr> {
        let path = self.path_of(ino)?;
        Self::guard_visible(&path)?;
        self.authorize(ProjectionOperation::Getattr, &path)?;
        self.attr_at(ino, &path)
    }

    /// The immediate children of directory `ino` (real entries only).
    pub fn readdir(&mut self, ino: u64) -> Result<Vec<DirItem>> {
        let dir = self.path_of(ino)?;
        self.authorize(ProjectionOperation::Readdir, &dir)?;
        let entries = self.loom.list_directory(self.ns, &dir)?;
        let mut out = Vec::with_capacity(entries.len());
        for e in entries {
            let child = if dir.is_empty() {
                e.name.clone()
            } else {
                format!("{dir}/{}", e.name)
            };
            if is_reserved_path(&child) {
                continue;
            }
            let child_ino = self.intern(&child);
            let kind = match e.kind {
                FileKind::Directory => NodeKind::Dir,
                FileKind::Symlink => NodeKind::Symlink,
                FileKind::File => NodeKind::File,
            };
            out.push(DirItem {
                name: e.name,
                ino: child_ino,
                kind,
            });
        }
        // In a facet collection directory, projected records (which are not working-tree files) appear as
        // regular files alongside the ordinary entries (raw/pending/quarantined/arbitrary files).
        if let Some((facet, principal, collection)) = self.facets.classify_collection(&dir) {
            let have: std::collections::BTreeSet<String> =
                out.iter().map(|d| d.name.clone()).collect();
            for name in
                self.facets
                    .list_projected(&self.loom, self.ns, facet, &principal, &collection)?
            {
                if have.contains(&name) {
                    continue;
                }
                let child = format!("{dir}/{name}");
                let child_ino = self.intern(&child);
                out.push(DirItem {
                    name,
                    ino: child_ino,
                    kind: NodeKind::File,
                });
            }
        }
        Ok(out)
    }

    /// Read up to `len` bytes at `offset` from file `ino` (bounded chunk read; clamps at EOF). A facet
    /// file backed by a record is served from its on-demand projection; a raw (pending/quarantined) or
    /// ordinary file is read normally.
    pub fn read(&self, ino: u64, offset: u64, len: u64) -> Result<Vec<u8>> {
        let path = self.path_of(ino)?;
        Self::guard_visible(&path)?;
        self.authorize(ProjectionOperation::Read, &path)?;
        let projected = match self.facets.classify(&path) {
            Some(f) => self.facets.project(&self.loom, self.ns, &f)?,
            None => None,
        };
        if let Some(bytes) = projected {
            let start = (offset as usize).min(bytes.len());
            let end = start.saturating_add(len as usize).min(bytes.len());
            return Ok(bytes[start..end].to_vec());
        }
        self.loom.read_at(self.ns, &path, offset, len)
    }

    /// Finalize a facet file after its bytes have been written (called by a backend on flush/fsync): the
    /// raw bytes at the path are parsed into a structured record (the raw file is then removed and the
    /// record projects), or, on a parse failure, the raw file is kept and the error recorded. A non-facet
    /// path, or a path with nothing written, is a no-op.
    pub fn flush_overlay(&mut self, ino: u64) -> Result<Option<overlay::WriteOutcome>> {
        let path = self.path_of(ino)?;
        Self::guard_visible(&path)?;
        self.authorize(ProjectionOperation::FlushOverlay, &path)?;
        let Some(f) = self.facets.classify(&path) else {
            return Ok(None);
        };
        match self.loom.read_file(self.ns, &path) {
            Ok(bytes) => {
                let outcome = self.facets.ingest(&mut self.loom, self.ns, &f, &bytes)?;
                // On a Stored outcome the raw file was removed (the record now projects, possibly under a
                // different name); drop the stale inode mapping so it re-resolves.
                self.forget_subtree(&path);
                Ok(Some(outcome))
            }
            Err(e) if e.code == Code::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// An extended attribute for `ino` (`user.loom.status`/`error`/`etag` for a facet file), or `None`.
    pub fn getxattr(&self, ino: u64, name: &str) -> Result<Option<Vec<u8>>> {
        let path = self.path_of(ino)?;
        Self::guard_visible(&path)?;
        self.authorize(ProjectionOperation::MetadataRead, &path)?;
        if let Some(f) = self.facets.classify(&path) {
            for (k, v) in self.metadata_for(&f)?.xattrs() {
                if k == name {
                    return Ok(Some(v));
                }
            }
        }
        Ok(None)
    }

    /// The extended-attribute names available on `ino` (the `user.loom.*` set for a facet file).
    pub fn listxattr(&self, ino: u64) -> Result<Vec<String>> {
        let path = self.path_of(ino)?;
        Self::guard_visible(&path)?;
        self.authorize(ProjectionOperation::MetadataRead, &path)?;
        if let Some(f) = self.facets.classify(&path) {
            return Ok(self
                .metadata_for(&f)?
                .xattrs()
                .into_iter()
                .map(|(k, _)| k)
                .collect());
        }
        Ok(Vec::new())
    }

    pub fn metadata(&self, ino: u64) -> Result<Option<ProjectionMetadata>> {
        let path = self.path_of(ino)?;
        Self::guard_visible(&path)?;
        self.authorize(ProjectionOperation::MetadataRead, &path)?;
        self.facets
            .classify(&path)
            .map(|f| self.metadata_for(&f))
            .transpose()
    }

    fn metadata_for(&self, f: &overlay::FacetFile) -> Result<ProjectionMetadata> {
        self.facets.metadata(&self.loom, self.ns, f)
    }

    /// Write `data` at `offset` of file `ino`; returns the byte count.
    pub fn write(&mut self, ino: u64, offset: u64, data: &[u8]) -> Result<u32> {
        let path = self.path_of(ino)?;
        Self::guard_visible(&path)?;
        self.authorize(ProjectionOperation::Write, &path)?;
        self.loom.write_at(self.ns, &path, offset, data)?;
        Ok(data.len() as u32)
    }

    /// Create an empty regular file `name` in directory `parent` with `mode` (`0` uses the default
    /// `0o100644`); returns its attributes.
    pub fn create(&mut self, parent: u64, name: &str, mode: u32) -> Result<Attr> {
        let path = self.child_path(parent, name)?;
        self.authorize(ProjectionOperation::Create, &path)?;
        let mode = if mode == 0 { 0o100644 } else { mode };
        self.loom.write_file(self.ns, &path, b"", mode)?;
        let ino = self.intern(&path);
        self.attr_at(ino, &path)
    }

    /// Create directory `name` in directory `parent`; returns its attributes.
    pub fn mkdir(&mut self, parent: u64, name: &str) -> Result<Attr> {
        let path = self.child_path(parent, name)?;
        self.authorize(ProjectionOperation::Mkdir, &path)?;
        self.loom.create_directory(self.ns, &path, false)?;
        // Creating a `<root>/<principal>/<collection>` directory also creates the backing facet
        // collection, so facet files dropped into it can be ingested.
        if let Some((facet, principal, collection)) = self.facets.classify_collection(&path) {
            self.facets.ensure_collection(
                &mut self.loom,
                self.ns,
                facet,
                &principal,
                &collection,
            )?;
        }
        let ino = self.intern(&path);
        self.attr_at(ino, &path)
    }

    /// Remove file (or symlink) `name` from directory `parent`.
    pub fn unlink(&mut self, parent: u64, name: &str) -> Result<()> {
        let path = self.child_path(parent, name)?;
        self.authorize(ProjectionOperation::Unlink, &path)?;
        if let Some(f) = self.facets.classify(&path)
            && self.facets.project(&self.loom, self.ns, &f)?.is_some()
            && self.facets.delete_record(&mut self.loom, self.ns, &f)?
        {
            match self.loom.remove_file(self.ns, &path) {
                Ok(()) => {}
                Err(e) if e.code == Code::NotFound => {}
                Err(e) => return Err(e),
            }
            self.forget_subtree(&path);
            return Ok(());
        }
        match self.loom.remove_file(self.ns, &path) {
            Ok(()) => {}
            Err(e) if e.code == Code::NotFound => {
                let Some(f) = self.facets.classify(&path) else {
                    return Err(e);
                };
                if !self.facets.delete_record(&mut self.loom, self.ns, &f)? {
                    return Err(e);
                }
            }
            Err(e) => return Err(e),
        }
        self.forget_subtree(&path);
        Ok(())
    }

    /// Remove (empty) directory `name` from directory `parent`.
    pub fn rmdir(&mut self, parent: u64, name: &str) -> Result<()> {
        let path = self.child_path(parent, name)?;
        self.authorize(ProjectionOperation::Rmdir, &path)?;
        self.loom.remove_directory(self.ns, &path, false)?;
        self.forget_subtree(&path);
        Ok(())
    }

    /// Rename `name` in `parent` to `new_name` in `new_parent`.
    pub fn rename(
        &mut self,
        parent: u64,
        name: &str,
        new_parent: u64,
        new_name: &str,
    ) -> Result<()> {
        let src = self.child_path(parent, name)?;
        let dst = self.child_path(new_parent, new_name)?;
        self.policy
            .authorize_rename(&self.loom, self.ns, self.mode, &src, &dst)?;
        self.loom.move_path(self.ns, &src, &dst)?;
        // The moved inode (and any cached descendants) are re-resolved on the next lookup.
        self.forget_subtree(&src);
        self.forget_subtree(&dst);
        Ok(())
    }

    /// Resize file `ino` to `size` (zero-extend or drop).
    pub fn truncate(&mut self, ino: u64, size: u64) -> Result<()> {
        let path = self.path_of(ino)?;
        Self::guard_visible(&path)?;
        self.authorize(ProjectionOperation::Truncate, &path)?;
        self.loom.truncate_file(self.ns, &path, size)
    }

    /// Create a symbolic link `name` in directory `parent` pointing at the opaque `target`; returns its
    /// attributes.
    pub fn symlink(&mut self, parent: u64, name: &str, target: &str) -> Result<Attr> {
        let path = self.child_path(parent, name)?;
        self.authorize(ProjectionOperation::Symlink, &path)?;
        self.loom.symlink(self.ns, target, &path)?;
        let ino = self.intern(&path);
        self.attr_at(ino, &path)
    }

    /// Read the target of symlink `ino`.
    pub fn readlink(&self, ino: u64) -> Result<String> {
        let path = self.path_of(ino)?;
        Self::guard_visible(&path)?;
        self.authorize(ProjectionOperation::Readlink, &path)?;
        self.loom.read_link(self.ns, &path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::workspace::FacetKind;
    use loom_core::{AclStore, IdentityStore, MemoryStore, PrincipalKind};

    fn proj(mode: Mode) -> Projection<MemoryStore> {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Files, None, WorkspaceId::from_bytes([1; 16]))
            .unwrap();
        Projection::new(loom, ns, mode)
    }

    fn enable_authenticated_projection(p: &mut Projection<MemoryStore>) {
        let root = WorkspaceId::from_bytes([9; 16]);
        let mut identity = IdentityStore::new(root);
        identity
            .add_principal(
                WorkspaceId::from_bytes([8; 16]),
                "alice",
                PrincipalKind::User,
            )
            .unwrap();
        identity
            .set_passphrase(root, "root-pass", b"12345678")
            .unwrap();
        let session = identity
            .authenticate_passphrase(root, "root-pass", "projection-test")
            .unwrap();
        p.loom_mut().set_identity_store(identity);
        p.loom_mut().set_session(session.id);
        p.loom_mut().set_acl_store(AclStore::new());
    }

    #[test]
    fn read_write_round_trip_through_inodes() {
        let mut p = proj(Mode::ReadWrite);
        let dir = p.mkdir(ROOT_INO, "sub").unwrap();
        assert_eq!(dir.kind, NodeKind::Dir);
        let f = p.create(dir.ino, "a.txt", 0).unwrap();
        assert_eq!(f.kind, NodeKind::File);
        assert_eq!(p.write(f.ino, 0, b"hello").unwrap(), 5);
        assert_eq!(p.read(f.ino, 0, 100).unwrap(), b"hello");
        // lookup returns the same stable inode; getattr reflects the size.
        assert_eq!(p.lookup(dir.ino, "a.txt").unwrap().ino, f.ino);
        assert_eq!(p.getattr(f.ino).unwrap().size, 5);
        // readdir lists children at each level.
        assert!(
            p.readdir(ROOT_INO)
                .unwrap()
                .iter()
                .any(|e| e.name == "sub" && e.kind == NodeKind::Dir)
        );
        assert!(
            p.readdir(dir.ino)
                .unwrap()
                .iter()
                .any(|e| e.name == "a.txt" && e.kind == NodeKind::File)
        );
    }

    #[test]
    fn truncate_and_offset_write() {
        let mut p = proj(Mode::ReadWrite);
        let f = p.create(ROOT_INO, "f", 0).unwrap();
        p.write(f.ino, 0, b"hello world").unwrap();
        p.truncate(f.ino, 5).unwrap();
        assert_eq!(p.read(f.ino, 0, 100).unwrap(), b"hello");
        // A positional write past the end zero-fills the gap (POSIX pwrite, via loom write_at).
        p.write(f.ino, 7, b"X").unwrap();
        assert_eq!(
            p.read(f.ino, 0, 100).unwrap(),
            vec![b'h', b'e', b'l', b'l', b'o', 0, 0, b'X']
        );
    }

    #[test]
    fn create_unlink_mkdir_rmdir() {
        let mut p = proj(Mode::ReadWrite);
        p.mkdir(ROOT_INO, "d").unwrap();
        p.create(ROOT_INO, "x", 0).unwrap();
        p.unlink(ROOT_INO, "x").unwrap();
        assert_eq!(p.lookup(ROOT_INO, "x").unwrap_err().code, Code::NotFound);
        p.rmdir(ROOT_INO, "d").unwrap();
        assert_eq!(p.lookup(ROOT_INO, "d").unwrap_err().code, Code::NotFound);
    }

    #[test]
    fn symlink_round_trip() {
        let mut p = proj(Mode::ReadWrite);
        p.create(ROOT_INO, "real", 0).unwrap();
        let l = p.symlink(ROOT_INO, "link", "real").unwrap();
        assert_eq!(l.kind, NodeKind::Symlink);
        assert_eq!(p.readlink(l.ino).unwrap(), "real");
        assert_eq!(p.getattr(l.ino).unwrap().kind, NodeKind::Symlink);
    }

    #[test]
    fn rename_moves_content_and_reassigns_path() {
        let mut p = proj(Mode::ReadWrite);
        let f = p.create(ROOT_INO, "old", 0).unwrap();
        p.write(f.ino, 0, b"data").unwrap();
        p.rename(ROOT_INO, "old", ROOT_INO, "new").unwrap();
        assert_eq!(p.lookup(ROOT_INO, "old").unwrap_err().code, Code::NotFound);
        let moved = p.lookup(ROOT_INO, "new").unwrap();
        assert_eq!(p.read(moved.ino, 0, 100).unwrap(), b"data");
    }

    #[test]
    fn read_only_mode_rejects_mutations() {
        let mut p = proj(Mode::ReadOnly);
        let ns = p.workspace();
        // Seed directly through the engine, then verify the projection reads but refuses writes.
        p.loom_mut().write_file(ns, "f", b"hi", 0o100644).unwrap();
        let f = p.lookup(ROOT_INO, "f").unwrap();
        assert_eq!(p.read(f.ino, 0, 100).unwrap(), b"hi");
        let err = p.write(f.ino, 0, b"x").unwrap_err();
        assert_eq!(err.code, Code::Unsupported);
        assert_eq!(errno(err.code), 30, "read-only write maps to EROFS");
        assert_eq!(
            p.create(ROOT_INO, "y", 0).unwrap_err().code,
            Code::Unsupported
        );
        assert_eq!(p.mkdir(ROOT_INO, "d").unwrap_err().code, Code::Unsupported);
    }

    #[test]
    fn acl_denial_maps_projection_lookup_read_and_write_to_eacces() {
        let mut p = proj(Mode::ReadWrite);
        let ns = p.workspace();
        p.loom_mut()
            .write_file(ns, "visible.txt", b"secret", 0o100644)
            .unwrap();
        enable_authenticated_projection(&mut p);

        for err in [
            p.readdir(ROOT_INO).unwrap_err(),
            p.lookup(ROOT_INO, "visible.txt").unwrap_err(),
            p.create(ROOT_INO, "new.txt", 0).unwrap_err(),
        ] {
            assert_eq!(err.code, Code::PermissionDenied);
            assert_eq!(errno(err.code), 13);
        }

        let ino = p.intern("visible.txt");
        let err = p.read(ino, 0, 100).unwrap_err();
        assert_eq!(err.code, Code::PermissionDenied);
        assert_eq!(errno(err.code), 13);
    }

    #[test]
    fn reserved_internal_tree_is_hidden_from_projection() {
        let mut p = proj(Mode::ReadWrite);
        let ns = p.workspace();
        p.loom_mut()
            .create_directory_reserved(ns, ".loom/facets/cas", true)
            .unwrap();
        p.loom_mut()
            .write_file_reserved(ns, ".loom/facets/cas/blob", b"hidden", 0o100644)
            .unwrap();

        assert!(p.loom().list_directory(ns, ".loom").is_ok());
        assert!(
            !p.readdir(ROOT_INO)
                .unwrap()
                .iter()
                .any(|entry| entry.name == ".loom")
        );
        assert_eq!(
            p.lookup(ROOT_INO, ".loom").unwrap_err().code,
            Code::NotFound
        );
    }

    #[test]
    fn rejects_bad_names_and_unknown_inodes() {
        let mut p = proj(Mode::ReadWrite);
        assert_eq!(
            p.lookup(ROOT_INO, "a/b").unwrap_err().code,
            Code::InvalidArgument
        );
        assert_eq!(
            p.lookup(ROOT_INO, "..").unwrap_err().code,
            Code::InvalidArgument
        );
        assert_eq!(p.getattr(9999).unwrap_err().code, Code::NotFound);
    }

    const ICS: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:ev1\r\nSUMMARY:Standup\r\nDTSTART:20240101T090000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    fn mkdirs_to_collection(p: &mut Projection<MemoryStore>) -> u64 {
        let cal = p.mkdir(ROOT_INO, "calendar").unwrap();
        let alice = p.mkdir(cal.ino, "alice").unwrap();
        p.mkdir(alice.ino, "work").unwrap().ino
    }

    #[test]
    fn overlay_ingests_valid_facet_file_and_projects_it() {
        let mut p = proj(Mode::ReadWrite);
        let work = mkdirs_to_collection(&mut p);
        let f = p.create(work, "ev1.ics", 0).unwrap();
        p.write(f.ino, 0, ICS.as_bytes()).unwrap();
        // Before flush the bytes are an ordinary file; flush ingests them into a record.
        let outcome = p.flush_overlay(f.ino).unwrap().unwrap();
        assert!(matches!(outcome, overlay::WriteOutcome::Stored { .. }));
        // The record now projects under its UID-derived name; reading serializes it.
        let looked = p.lookup(work, "ev1.ics").unwrap();
        assert_eq!(looked.kind, NodeKind::File);
        let bytes = p.read(looked.ino, 0, 4096).unwrap();
        assert!(String::from_utf8_lossy(&bytes).contains("UID:ev1"));
        // readdir lists the projected record; xattr reports ok + an etag.
        assert!(p.readdir(work).unwrap().iter().any(|d| d.name == "ev1.ics"));
        assert_eq!(
            p.getxattr(looked.ino, "user.loom.status")
                .unwrap()
                .as_deref(),
            Some(&b"ok"[..])
        );
        assert!(p.getxattr(looked.ino, "user.loom.etag").unwrap().is_some());
    }

    #[test]
    fn overlay_unlinks_projected_facet_record() {
        let mut p = proj(Mode::ReadWrite);
        let work = mkdirs_to_collection(&mut p);
        let f = p.create(work, "ev1.ics", 0).unwrap();
        p.write(f.ino, 0, ICS.as_bytes()).unwrap();
        p.flush_overlay(f.ino).unwrap().unwrap();
        let looked = p.lookup(work, "ev1.ics").unwrap();
        assert_eq!(looked.kind, NodeKind::File);

        p.unlink(work, "ev1.ics").unwrap();

        assert_eq!(p.lookup(work, "ev1.ics").unwrap_err().code, Code::NotFound);
        assert!(!p.readdir(work).unwrap().iter().any(|d| d.name == "ev1.ics"));
        assert!(
            loom_core::calendar::get_entry(p.loom(), p.workspace(), "alice", "work", "ev1")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn overlay_quarantines_unparseable_facet_file() {
        let mut p = proj(Mode::ReadWrite);
        let work = mkdirs_to_collection(&mut p);
        let f = p.create(work, "broken.ics", 0).unwrap();
        p.write(f.ino, 0, b"not iCalendar").unwrap();
        let outcome = p.flush_overlay(f.ino).unwrap().unwrap();
        assert!(matches!(outcome, overlay::WriteOutcome::Quarantined { .. }));
        // The raw file stays "there" and reads back verbatim; xattr marks it quarantined.
        let looked = p.lookup(work, "broken.ics").unwrap();
        assert_eq!(p.read(looked.ino, 0, 4096).unwrap(), b"not iCalendar");
        assert_eq!(
            p.getxattr(looked.ino, "user.loom.status")
                .unwrap()
                .as_deref(),
            Some(&b"quarantined"[..])
        );
        assert!(p.getxattr(looked.ino, "user.loom.error").unwrap().is_some());
    }

    #[test]
    fn overlay_leaves_arbitrary_files_alone() {
        let mut p = proj(Mode::ReadWrite);
        let work = mkdirs_to_collection(&mut p);
        let f = p.create(work, "cat.jpg", 0).unwrap();
        p.write(f.ino, 0, b"\xff\xd8\xff\x00jpeg").unwrap();
        // A non-facet file is untouched by the overlay: flush is a no-op, bytes are verbatim.
        assert!(p.flush_overlay(f.ino).unwrap().is_none());
        assert_eq!(p.read(f.ino, 0, 4096).unwrap(), b"\xff\xd8\xff\x00jpeg");
        assert!(p.getxattr(f.ino, "user.loom.status").unwrap().is_none());
    }

    #[test]
    fn errno_mapping() {
        assert_eq!(errno(Code::NotFound), 2);
        assert_eq!(errno(Code::AlreadyExists), 17);
        assert_eq!(errno(Code::InvalidArgument), 22);
        assert_eq!(errno(Code::Unsupported), 30);
        assert_eq!(errno(Code::PermissionDenied), 13);
    }
}
