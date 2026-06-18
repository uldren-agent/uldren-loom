//! NFSv3 backend for the Uldren Loom filesystem projection.
//!
//! [`LoomNfs`] is a thin [`nfsserve`] adapter over the portable [`loom_vfs::Projection`]: each NFS
//! procedure locks the projection, calls the matching op, and translates loom errors to `nfsstat3`. It
//! targets a `Loom<FileStore>` (an on-disk `.loom`) and persists after each mutation. The server speaks
//! ONC RPC + MOUNT + NFSv3 over TCP, so macOS and Linux can mount it driverless with their built-in NFS
//! client (`mount_nfs -o vers=3` / `mount -t nfs -o vers=3`).
//!
//! Pure Rust: there is no native NFS dependency, but the crate does pull a tokio runtime, so it sits
//! outside `loom-core`/`loom-vfs`. The server runs on a single-threaded tokio runtime; loom
//! working-tree ops are synchronous and run under a `Mutex` while the procedure holds it, never across
//! an `.await`. Licensed under BUSL-1.1.

use std::path::Path;
use std::sync::Mutex;

use async_trait::async_trait;
use loom_core::error::LoomError;
use loom_core::workspace::WorkspaceId;
use loom_store::{
    FileStore, LocalOpenAuth, attach_local_auth, open_loom_daemon_authorized_unlocked,
    open_loom_read_unlocked, save_loom,
};
use loom_vfs::{Attr, Mode, NodeKind, Projection, errno};
use nfsserve::nfs::{
    fattr3, fileid3, filename3, ftype3, nfspath3, nfsstat3, nfsstring, nfstime3, sattr3, set_mode3,
    set_size3, specdata3,
};
use nfsserve::tcp::{NFSTcp, NFSTcpListener};
use nfsserve::vfs::{DirEntry, NFSFileSystem, ReadDirResult, VFSCapabilities};

/// An NFSv3 filesystem backed by a loom workspace working tree.
pub struct LoomNfs {
    proj: Mutex<Projection<FileStore>>,
    persist: bool,
    uid: u32,
    gid: u32,
}

impl LoomNfs {
    /// Wrap a projection. `persist` saves the loom after each mutation (true for an on-disk store).
    pub fn new(proj: Projection<FileStore>, persist: bool, uid: u32, gid: u32) -> Self {
        Self {
            proj: Mutex::new(proj),
            persist,
            uid,
            gid,
        }
    }

    /// Save the loom after a mutation (no-op when `persist` is false).
    fn save(proj: &mut Projection<FileStore>, persist: bool) -> Result<(), LoomError> {
        if persist {
            save_loom(proj.loom_mut())
        } else {
            Ok(())
        }
    }
}

/// Translate a loom error to the NFSv3 status code (via the shared `errno` mapping).
fn nfs_err(e: &LoomError) -> nfsstat3 {
    match errno(e.code) {
        2 => nfsstat3::NFS3ERR_NOENT,
        13 => nfsstat3::NFS3ERR_ACCES,
        17 => nfsstat3::NFS3ERR_EXIST,
        22 => nfsstat3::NFS3ERR_INVAL,
        30 => nfsstat3::NFS3ERR_ROFS,
        _ => nfsstat3::NFS3ERR_IO,
    }
}

/// Decode an NFS filename to UTF-8, or `NFS3ERR_INVAL` if it is not valid UTF-8.
fn name_str(name: &filename3) -> Result<&str, nfsstat3> {
    std::str::from_utf8(name.as_ref()).map_err(|_| nfsstat3::NFS3ERR_INVAL)
}

/// Build an NFS `fattr3` from a portable [`Attr`]. Times are reported as the epoch; loom is the source
/// of truth for content and structure, not POSIX timestamps.
fn file_attr(a: &Attr, uid: u32, gid: u32) -> fattr3 {
    let (ftype, mode, nlink) = match a.kind {
        NodeKind::Dir => (ftype3::NF3DIR, 0o755u32, 2),
        NodeKind::Symlink => (ftype3::NF3LNK, 0o777u32, 1),
        NodeKind::File => (ftype3::NF3REG, a.mode & 0o7777, 1),
    };
    let zero = nfstime3 {
        seconds: 0,
        nseconds: 0,
    };
    fattr3 {
        ftype,
        mode,
        nlink,
        uid,
        gid,
        size: a.size,
        used: a.size,
        rdev: specdata3 {
            specdata1: 0,
            specdata2: 0,
        },
        fsid: 0,
        fileid: a.ino,
        atime: zero,
        mtime: zero,
        ctime: zero,
    }
}

/// The permission bits requested by a `sattr3`, or `0` (projection default) when unset.
fn requested_mode(attr: &sattr3) -> u32 {
    match attr.mode {
        set_mode3::mode(m) => m,
        set_mode3::Void => 0,
    }
}

#[async_trait]
impl NFSFileSystem for LoomNfs {
    fn capabilities(&self) -> VFSCapabilities {
        let read_only = matches!(self.proj.lock().unwrap().mode(), Mode::ReadOnly);
        if read_only {
            VFSCapabilities::ReadOnly
        } else {
            VFSCapabilities::ReadWrite
        }
    }

    fn root_dir(&self) -> fileid3 {
        loom_vfs::ROOT_INO
    }

    async fn lookup(&self, dirid: fileid3, filename: &filename3) -> Result<fileid3, nfsstat3> {
        let name = name_str(filename)?;
        let mut p = self.proj.lock().unwrap();
        p.lookup(dirid, name)
            .map(|a| a.ino)
            .map_err(|e| nfs_err(&e))
    }

    async fn getattr(&self, id: fileid3) -> Result<fattr3, nfsstat3> {
        let p = self.proj.lock().unwrap();
        p.getattr(id)
            .map(|a| file_attr(&a, self.uid, self.gid))
            .map_err(|e| nfs_err(&e))
    }

    async fn setattr(&self, id: fileid3, setattr: sattr3) -> Result<fattr3, nfsstat3> {
        let mut p = self.proj.lock().unwrap();
        // Only a size change (truncate) is applied to the working tree; mode/owner/time changes are
        // accepted as no-ops. A truncate in a read-only projection returns ROFS.
        if let set_size3::size(size) = setattr.size {
            p.truncate(id, size).map_err(|e| nfs_err(&e))?;
            Self::save(&mut p, self.persist).map_err(|e| nfs_err(&e))?;
        }
        p.getattr(id)
            .map(|a| file_attr(&a, self.uid, self.gid))
            .map_err(|e| nfs_err(&e))
    }

    async fn read(
        &self,
        id: fileid3,
        offset: u64,
        count: u32,
    ) -> Result<(Vec<u8>, bool), nfsstat3> {
        let p = self.proj.lock().unwrap();
        let bytes = p
            .read(id, offset, u64::from(count))
            .map_err(|e| nfs_err(&e))?;
        let size = p.getattr(id).map_err(|e| nfs_err(&e))?.size;
        let eof = offset.saturating_add(bytes.len() as u64) >= size;
        Ok((bytes, eof))
    }

    async fn write(&self, id: fileid3, offset: u64, data: &[u8]) -> Result<fattr3, nfsstat3> {
        let mut p = self.proj.lock().unwrap();
        p.write(id, offset, data).map_err(|e| nfs_err(&e))?;
        Self::save(&mut p, self.persist).map_err(|e| nfs_err(&e))?;
        p.getattr(id)
            .map(|a| file_attr(&a, self.uid, self.gid))
            .map_err(|e| nfs_err(&e))
    }

    async fn create(
        &self,
        dirid: fileid3,
        filename: &filename3,
        attr: sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        let name = name_str(filename)?;
        let mut p = self.proj.lock().unwrap();
        let made = p
            .create(dirid, name, requested_mode(&attr))
            .map_err(|e| nfs_err(&e))?;
        Self::save(&mut p, self.persist).map_err(|e| nfs_err(&e))?;
        Ok((made.ino, file_attr(&made, self.uid, self.gid)))
    }

    async fn create_exclusive(
        &self,
        dirid: fileid3,
        filename: &filename3,
    ) -> Result<fileid3, nfsstat3> {
        let name = name_str(filename)?;
        let mut p = self.proj.lock().unwrap();
        // Exclusive create is idempotent on retry: if the name already exists, return its id; otherwise
        // create an empty file.
        match p.lookup(dirid, name) {
            Ok(a) => Ok(a.ino),
            Err(e) if e.code == loom_core::error::Code::NotFound => {
                let made = p.create(dirid, name, 0).map_err(|e| nfs_err(&e))?;
                Self::save(&mut p, self.persist).map_err(|e| nfs_err(&e))?;
                Ok(made.ino)
            }
            Err(e) => Err(nfs_err(&e)),
        }
    }

    async fn mkdir(
        &self,
        dirid: fileid3,
        dirname: &filename3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        let name = name_str(dirname)?;
        let mut p = self.proj.lock().unwrap();
        let made = p.mkdir(dirid, name).map_err(|e| nfs_err(&e))?;
        Self::save(&mut p, self.persist).map_err(|e| nfs_err(&e))?;
        Ok((made.ino, file_attr(&made, self.uid, self.gid)))
    }

    async fn remove(&self, dirid: fileid3, filename: &filename3) -> Result<(), nfsstat3> {
        let name = name_str(filename)?;
        let mut p = self.proj.lock().unwrap();
        // The NFS server dispatches both REMOVE and RMDIR here, so dispatch on the target kind: an empty
        // directory goes to rmdir, anything else to unlink.
        let kind = p.lookup(dirid, name).map_err(|e| nfs_err(&e))?.kind;
        match kind {
            NodeKind::Dir => p.rmdir(dirid, name).map_err(|e| nfs_err(&e))?,
            NodeKind::File | NodeKind::Symlink => p.unlink(dirid, name).map_err(|e| nfs_err(&e))?,
        }
        Self::save(&mut p, self.persist).map_err(|e| nfs_err(&e))
    }

    async fn rename(
        &self,
        from_dirid: fileid3,
        from_filename: &filename3,
        to_dirid: fileid3,
        to_filename: &filename3,
    ) -> Result<(), nfsstat3> {
        let from = name_str(from_filename)?;
        let to = name_str(to_filename)?;
        let mut p = self.proj.lock().unwrap();
        p.rename(from_dirid, from, to_dirid, to)
            .map_err(|e| nfs_err(&e))?;
        Self::save(&mut p, self.persist).map_err(|e| nfs_err(&e))
    }

    async fn readdir(
        &self,
        dirid: fileid3,
        start_after: fileid3,
        max_entries: usize,
    ) -> Result<ReadDirResult, nfsstat3> {
        let mut p = self.proj.lock().unwrap();
        let mut items = p.readdir(dirid).map_err(|e| nfs_err(&e))?;
        // Deterministic order so pagination by cookie is stable across calls.
        items.sort_by(|a, b| a.name.cmp(&b.name));
        // `start_after` is the fileid (cookie) of the last entry already returned; 0 starts at the top.
        // An unknown cookie restarts from the beginning (best effort).
        let start = if start_after == 0 {
            0
        } else {
            items
                .iter()
                .position(|it| it.ino == start_after)
                .map_or(0, |i| i + 1)
        };
        let take = max_entries.max(1);
        let mut entries = Vec::new();
        let mut idx = start;
        while idx < items.len() && entries.len() < take {
            let it = &items[idx];
            let attr = p.getattr(it.ino).map_err(|e| nfs_err(&e))?;
            entries.push(DirEntry {
                fileid: it.ino,
                name: nfsstring(it.name.clone().into_bytes()),
                attr: file_attr(&attr, self.uid, self.gid),
            });
            idx += 1;
        }
        let end = idx >= items.len();
        Ok(ReadDirResult { entries, end })
    }

    async fn symlink(
        &self,
        dirid: fileid3,
        linkname: &filename3,
        symlink: &nfspath3,
        _attr: &sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        let name = name_str(linkname)?;
        let target = std::str::from_utf8(symlink.as_ref()).map_err(|_| nfsstat3::NFS3ERR_INVAL)?;
        let mut p = self.proj.lock().unwrap();
        let made = p.symlink(dirid, name, target).map_err(|e| nfs_err(&e))?;
        Self::save(&mut p, self.persist).map_err(|e| nfs_err(&e))?;
        Ok((made.ino, file_attr(&made, self.uid, self.gid)))
    }

    async fn readlink(&self, id: fileid3) -> Result<nfspath3, nfsstat3> {
        let p = self.proj.lock().unwrap();
        let target = p.readlink(id).map_err(|e| nfs_err(&e))?;
        Ok(nfsstring(target.into_bytes()))
    }
}

/// Open the loom at `loom_path` and build the [`LoomNfs`] for `workspace`.
pub fn build_fs(loom_path: &Path, workspace: &str, read_only: bool) -> Result<LoomNfs, LoomError> {
    build_fs_with_auth(loom_path, workspace, read_only, &LocalOpenAuth::default())
}

pub fn build_fs_with_auth(
    loom_path: &Path,
    workspace: &str,
    read_only: bool,
    auth: &LocalOpenAuth,
) -> Result<LoomNfs, LoomError> {
    let loom = if read_only {
        open_loom_read_unlocked(loom_path, auth.unlock_key.as_ref())?
    } else {
        open_loom_daemon_authorized_unlocked(loom_path, auth.unlock_key.as_ref())?
    };
    let loom = attach_local_auth(loom, auth)?;
    let ns = resolve_ns(&loom, workspace)?;
    let mode = if read_only {
        Mode::ReadOnly
    } else {
        Mode::ReadWrite
    };
    let proj = Projection::new(loom, ns, mode);
    Ok(LoomNfs::new(proj, !read_only, 0, 0))
}

fn resolve_ns(loom: &loom_core::Loom<FileStore>, name: &str) -> Result<WorkspaceId, LoomError> {
    let selector = match WorkspaceId::parse(name) {
        Ok(id) => loom_core::WsSelector::Id(id),
        Err(_) => loom_core::WsSelector::Name(name.to_string()),
    };
    loom.registry().open(&selector)
}

/// Serve `workspace` of the loom at `loom_path` over NFSv3 on `listen` (e.g. `127.0.0.1:12049`),
/// blocking forever (until the process is stopped). Mount it driverless, for example on macOS:
/// `mount_nfs -o vers=3,tcp,port=12049,mountport=12049,nolocks localhost:/ /mnt/loom`.
pub fn serve_blocking(
    loom_path: &Path,
    workspace: &str,
    listen: &str,
    read_only: bool,
) -> std::io::Result<()> {
    serve_blocking_with_auth(
        loom_path,
        workspace,
        listen,
        read_only,
        LocalOpenAuth::default(),
    )
}

pub fn serve_blocking_with_auth(
    loom_path: &Path,
    workspace: &str,
    listen: &str,
    read_only: bool,
    auth: LocalOpenAuth,
) -> std::io::Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(serve_with_auth(
        loom_path, workspace, listen, read_only, auth,
    ))
}

/// Async form of [`serve_blocking`]: bind the NFS/MOUNT server on `listen` and handle connections
/// forever.
pub async fn serve(
    loom_path: &Path,
    workspace: &str,
    listen: &str,
    read_only: bool,
) -> std::io::Result<()> {
    serve_with_auth(
        loom_path,
        workspace,
        listen,
        read_only,
        LocalOpenAuth::default(),
    )
    .await
}

pub async fn serve_with_auth(
    loom_path: &Path,
    workspace: &str,
    listen: &str,
    read_only: bool,
    auth: LocalOpenAuth,
) -> std::io::Result<()> {
    let fs = build_fs_with_auth(loom_path, workspace, read_only, &auth)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let listener = NFSTcpListener::bind(listen, fs).await?;
    print_serve_banner(&listener, workspace, read_only);
    listener.handle_forever().await
}

/// Announce the actual bound address (resolving the real port even when `listen` used port 0). The
/// `loom mount-nfs` CLI drives the OS mount itself and prints that separately; this is the server's
/// own one-line notice, also useful when `serve`/`serve_blocking` is run directly or headless.
fn print_serve_banner(listener: &NFSTcpListener<LoomNfs>, workspace: &str, read_only: bool) {
    let ip = listener.get_listen_ip();
    let port = listener.get_listen_port();
    let mode = if read_only { "read-only" } else { "read-write" };
    println!(
        "loom: serving workspace {workspace:?} over NFSv3 on {ip}:{port} (export \"/\", {mode})"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::workspace::FacetKind;
    use loom_core::{Algo, Loom, WorkspaceId};
    use loom_store::{FileStore, save_loom};

    /// Build an on-disk loom with a Files workspace `docs` seeded with `hello.txt`, returning its path.
    fn seed_loom(dir: &Path) -> std::path::PathBuf {
        let loom_path = dir.join("t.loom");
        let store = FileStore::create_with_profile(&loom_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("docs"),
                WorkspaceId::from_bytes([7; 16]),
            )
            .unwrap();
        loom.write_file(ns, "hello.txt", b"hi", 0o100644).unwrap();
        save_loom(&mut loom).unwrap();
        loom_path
    }

    fn fname(s: &str) -> filename3 {
        nfsstring(s.as_bytes().to_vec())
    }

    /// Drive the NFS trait directly (no kernel mount) over a current-thread runtime: lookup, read,
    /// create, write, mkdir, readdir, symlink, rename, remove.
    #[test]
    fn nfs_trait_round_trip() {
        let dir = std::env::temp_dir().join(format!("loomnfs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let loom_path = seed_loom(&dir);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let fs = build_fs(&loom_path, "docs", false).unwrap();
            let root = fs.root_dir();

            // Seed file is visible and readable.
            let hello = fs.lookup(root, &fname("hello.txt")).await.unwrap();
            let (bytes, eof) = fs.read(hello, 0, 100).await.unwrap();
            assert_eq!(bytes, b"hi");
            assert!(eof);

            // Create + write a new file.
            let (nid, _) = fs
                .create(root, &fname("new.txt"), sattr3::default())
                .await
                .unwrap();
            fs.write(nid, 0, b"world").await.unwrap();
            let (nbytes, _) = fs.read(nid, 0, 100).await.unwrap();
            assert_eq!(nbytes, b"world");

            // Subdirectory with a child, then list the root deterministically.
            let (sid, _) = fs.mkdir(root, &fname("sub")).await.unwrap();
            fs.create(sid, &fname("a.txt"), sattr3::default())
                .await
                .unwrap();
            let listing = fs.readdir(root, 0, 100).await.unwrap();
            let names: Vec<String> = listing
                .entries
                .iter()
                .map(|e| String::from_utf8(e.name.as_ref().to_vec()).unwrap())
                .collect();
            assert_eq!(names, vec!["hello.txt", "new.txt", "sub"]);
            assert!(listing.end);

            // Symlink round-trips.
            let (lid, _) = fs
                .symlink(
                    root,
                    &fname("link"),
                    &nfsstring(b"hello.txt".to_vec()),
                    &sattr3::default(),
                )
                .await
                .unwrap();
            assert_eq!(fs.readlink(lid).await.unwrap().as_ref(), b"hello.txt");

            // Rename then remove.
            fs.rename(root, &fname("new.txt"), root, &fname("renamed.txt"))
                .await
                .unwrap();
            assert!(fs.lookup(root, &fname("new.txt")).await.is_err());
            let renamed = fs.lookup(root, &fname("renamed.txt")).await.unwrap();
            fs.remove(root, &fname("renamed.txt")).await.unwrap();
            assert!(fs.getattr(renamed).await.is_err());
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A read-only projection rejects mutations with `ROFS` and still serves reads.
    #[test]
    fn nfs_read_only_rejects_mutations() {
        let dir = std::env::temp_dir().join(format!("loomnfs-ro-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let loom_path = seed_loom(&dir);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let fs = build_fs(&loom_path, "docs", true).unwrap();
            let root = fs.root_dir();
            assert!(matches!(fs.capabilities(), VFSCapabilities::ReadOnly));
            let hello = fs.lookup(root, &fname("hello.txt")).await.unwrap();
            assert_eq!(fs.read(hello, 0, 100).await.unwrap().0, b"hi");
            let err = fs
                .create(root, &fname("nope.txt"), sattr3::default())
                .await
                .unwrap_err();
            assert!(matches!(err, nfsstat3::NFS3ERR_ROFS));
        });
        let _ = std::fs::remove_dir_all(&dir);
    }
}
