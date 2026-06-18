//! FUSE backend for the Uldren Loom filesystem projection.
//!
//! [`LoomFuse`] is a thin `fuser` adapter over the portable [`loom_vfs::Projection`]: each FUSE callback
//! locks the projection, calls the matching op, and translates loom errors to `errno`. It targets a
//! `Loom<FileStore>` (an on-disk `.loom`) and persists after each mutation. Native-only - this crate is
//! intentionally outside `loom-core`/`loom-vfs`, which stay free of the libfuse dependency.
//!
//! On Linux a mount needs either a setuid `fusermount3` or a user workspace; on macOS it needs macFUSE.
//! In a restricted environment, run the mount inside a mapped-root user workspace
//! (`unshare -Urm --map-root-user`).
//!
//! The whole crate is gated behind the `fuse` feature. The workspace enables it by default, and the
//! root `just` recipes skip this crate on platforms where FUSE cannot be built. Build the CLI without
//! FUSE via `--no-default-features --features nfs` when a driver-free binary is needed. Licensed under
//! BUSL-1.1.
#![cfg(feature = "fuse")]

use std::path::Path;
#[cfg(target_os = "macos")]
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
#[cfg(target_os = "macos")]
use std::{fs::File, fs::OpenOptions, os::fd::AsRawFd, os::fd::OwnedFd, process::Command};

use fuser::{
    Config, FileAttr, FileType, Filesystem, Generation, INodeNo, MountOption, ReplyAttr,
    ReplyCreate, ReplyData, ReplyDirectory, ReplyEmpty, ReplyEntry, ReplyOpen, ReplyWrite, Request,
    TimeOrNow,
};
use loom_core::error::LoomError;
use loom_core::workspace::WorkspaceId;
use loom_store::{
    FileStore, LocalOpenAuth, attach_local_auth, open_loom_daemon_authorized_unlocked,
    open_loom_read_unlocked, save_loom,
};
use loom_vfs::{Attr, Mode, NodeKind, Projection, errno};
#[cfg(target_os = "macos")]
use nix::{
    fcntl::{FcntlArg, FdFlag, fcntl},
    mount::MntFlags,
};

/// Attribute cache lifetime handed to the kernel.
const TTL: Duration = Duration::from_secs(1);
#[cfg(target_os = "macos")]
const FUSE_DEVICE: &str = "/dev/fuse";
#[cfg(target_os = "macos")]
const MOUNT_FUSEFS_BIN: &str = "mount_fusefs";

/// A FUSE filesystem backed by a loom workspace working tree.
pub struct LoomFuse {
    proj: Mutex<Projection<FileStore>>,
    persist: bool,
    uid: u32,
    gid: u32,
}

impl LoomFuse {
    /// Wrap a projection. `persist` saves the loom after each mutation (true for an on-disk store).
    pub fn new(proj: Projection<FileStore>, persist: bool, uid: u32, gid: u32) -> Self {
        Self {
            proj: Mutex::new(proj),
            persist,
            uid,
            gid,
        }
    }
}

/// Translate a loom error to the FUSE `errno`.
fn fuse_errno(e: &LoomError) -> fuser::Errno {
    fuser::Errno::from_i32(errno(e.code))
}

/// Build a kernel `FileAttr` from a portable [`Attr`].
fn file_attr(a: &Attr, uid: u32, gid: u32) -> FileAttr {
    let (kind, perm, nlink) = match a.kind {
        NodeKind::Dir => (FileType::Directory, 0o755u16, 2),
        NodeKind::Symlink => (FileType::Symlink, 0o777u16, 1),
        NodeKind::File => (FileType::RegularFile, (a.mode & 0o7777) as u16, 1),
    };
    FileAttr {
        ino: INodeNo(a.ino),
        size: a.size,
        blocks: a.size.div_ceil(512),
        atime: UNIX_EPOCH,
        mtime: UNIX_EPOCH,
        ctime: UNIX_EPOCH,
        crtime: UNIX_EPOCH,
        kind,
        perm,
        nlink,
        uid,
        gid,
        rdev: 0,
        blksize: 4096,
        flags: 0,
    }
}

impl LoomFuse {
    /// Save the loom after a mutation (no-op when `persist` is false).
    fn save(proj: &mut Projection<FileStore>, persist: bool) -> Result<(), LoomError> {
        if persist {
            save_loom(proj.loom_mut())
        } else {
            Ok(())
        }
    }
}

impl Filesystem for LoomFuse {
    fn lookup(&self, _req: &Request, parent: INodeNo, name: &std::ffi::OsStr, reply: ReplyEntry) {
        let Some(name) = name.to_str() else {
            reply.error(fuser::Errno::EINVAL);
            return;
        };
        let mut p = self.proj.lock().unwrap();
        match p.lookup(parent.0, name) {
            Ok(a) => reply.entry(&TTL, &file_attr(&a, self.uid, self.gid), Generation(0)),
            Err(e) => reply.error(fuse_errno(&e)),
        }
    }

    fn getattr(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: Option<fuser::FileHandle>,
        reply: ReplyAttr,
    ) {
        let p = self.proj.lock().unwrap();
        match p.getattr(ino.0) {
            Ok(a) => reply.attr(&TTL, &file_attr(&a, self.uid, self.gid)),
            Err(e) => reply.error(fuse_errno(&e)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn setattr(
        &self,
        _req: &Request,
        ino: INodeNo,
        _mode: Option<u32>,
        _uid: Option<u32>,
        _gid: Option<u32>,
        size: Option<u64>,
        _atime: Option<TimeOrNow>,
        _mtime: Option<TimeOrNow>,
        _ctime: Option<SystemTime>,
        _fh: Option<fuser::FileHandle>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<SystemTime>,
        _flags: Option<fuser::BsdFileFlags>,
        reply: ReplyAttr,
    ) {
        let mut p = self.proj.lock().unwrap();
        // Only size changes (truncate) are applied; other attribute changes are accepted as no-ops.
        if let Some(size) = size
            && let Err(e) = p.truncate(ino.0, size)
        {
            reply.error(fuse_errno(&e));
            return;
        }
        if size.is_some()
            && let Err(e) = Self::save(&mut p, self.persist)
        {
            reply.error(fuse_errno(&e));
            return;
        }
        match p.getattr(ino.0) {
            Ok(a) => reply.attr(&TTL, &file_attr(&a, self.uid, self.gid)),
            Err(e) => reply.error(fuse_errno(&e)),
        }
    }

    fn readlink(&self, _req: &Request, ino: INodeNo, reply: ReplyData) {
        let p = self.proj.lock().unwrap();
        match p.readlink(ino.0) {
            Ok(target) => reply.data(target.as_bytes()),
            Err(e) => reply.error(fuse_errno(&e)),
        }
    }

    fn mkdir(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &std::ffi::OsStr,
        _mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        let Some(name) = name.to_str() else {
            reply.error(fuser::Errno::EINVAL);
            return;
        };
        let mut p = self.proj.lock().unwrap();
        let made = p
            .mkdir(parent.0, name)
            .and_then(|a| Self::save(&mut p, self.persist).map(|()| a));
        match made {
            Ok(a) => reply.entry(&TTL, &file_attr(&a, self.uid, self.gid), Generation(0)),
            Err(e) => reply.error(fuse_errno(&e)),
        }
    }

    fn unlink(&self, _req: &Request, parent: INodeNo, name: &std::ffi::OsStr, reply: ReplyEmpty) {
        let Some(name) = name.to_str() else {
            reply.error(fuser::Errno::EINVAL);
            return;
        };
        let mut p = self.proj.lock().unwrap();
        let done = p
            .unlink(parent.0, name)
            .and_then(|()| Self::save(&mut p, self.persist));
        match done {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(fuse_errno(&e)),
        }
    }

    fn rmdir(&self, _req: &Request, parent: INodeNo, name: &std::ffi::OsStr, reply: ReplyEmpty) {
        let Some(name) = name.to_str() else {
            reply.error(fuser::Errno::EINVAL);
            return;
        };
        let mut p = self.proj.lock().unwrap();
        let done = p
            .rmdir(parent.0, name)
            .and_then(|()| Self::save(&mut p, self.persist));
        match done {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(fuse_errno(&e)),
        }
    }

    fn symlink(
        &self,
        _req: &Request,
        parent: INodeNo,
        link_name: &std::ffi::OsStr,
        target: &Path,
        reply: ReplyEntry,
    ) {
        let (Some(link_name), Some(target)) = (link_name.to_str(), target.to_str()) else {
            reply.error(fuser::Errno::EINVAL);
            return;
        };
        let mut p = self.proj.lock().unwrap();
        match p.symlink(parent.0, link_name, target).and_then(|a| {
            Self::save(&mut p, self.persist)?;
            Ok(a)
        }) {
            Ok(a) => reply.entry(&TTL, &file_attr(&a, self.uid, self.gid), Generation(0)),
            Err(e) => reply.error(fuse_errno(&e)),
        }
    }

    fn rename(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &std::ffi::OsStr,
        newparent: INodeNo,
        newname: &std::ffi::OsStr,
        _flags: fuser::RenameFlags,
        reply: ReplyEmpty,
    ) {
        let (Some(name), Some(newname)) = (name.to_str(), newname.to_str()) else {
            reply.error(fuser::Errno::EINVAL);
            return;
        };
        let mut p = self.proj.lock().unwrap();
        match p
            .rename(parent.0, name, newparent.0, newname)
            .and_then(|()| Self::save(&mut p, self.persist))
        {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(fuse_errno(&e)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn read(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: fuser::FileHandle,
        offset: u64,
        size: u32,
        _flags: fuser::OpenFlags,
        _lock_owner: Option<fuser::LockOwner>,
        reply: ReplyData,
    ) {
        let p = self.proj.lock().unwrap();
        match p.read(ino.0, offset, u64::from(size)) {
            Ok(bytes) => reply.data(&bytes),
            Err(e) => reply.error(fuse_errno(&e)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn write(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: fuser::FileHandle,
        offset: u64,
        data: &[u8],
        _write_flags: fuser::WriteFlags,
        _flags: fuser::OpenFlags,
        _lock_owner: Option<fuser::LockOwner>,
        reply: ReplyWrite,
    ) {
        let mut p = self.proj.lock().unwrap();
        match p.write(ino.0, offset, data).and_then(|n| {
            Self::save(&mut p, self.persist)?;
            Ok(n)
        }) {
            Ok(n) => reply.written(n),
            Err(e) => reply.error(fuse_errno(&e)),
        }
    }

    fn create(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &std::ffi::OsStr,
        mode: u32,
        _umask: u32,
        _flags: i32,
        reply: ReplyCreate,
    ) {
        let Some(name) = name.to_str() else {
            reply.error(fuser::Errno::EINVAL);
            return;
        };
        let mut p = self.proj.lock().unwrap();
        match p.create(parent.0, name, mode).and_then(|a| {
            Self::save(&mut p, self.persist)?;
            Ok(a)
        }) {
            Ok(a) => reply.created(
                &TTL,
                &file_attr(&a, self.uid, self.gid),
                Generation(0),
                fuser::FileHandle(0),
                fuser::FopenFlags::empty(),
            ),
            Err(e) => reply.error(fuse_errno(&e)),
        }
    }

    fn readdir(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: fuser::FileHandle,
        offset: u64,
        mut reply: ReplyDirectory,
    ) {
        let mut p = self.proj.lock().unwrap();
        let entries = match p.readdir(ino.0) {
            Ok(e) => e,
            Err(e) => {
                reply.error(fuse_errno(&e));
                return;
            }
        };
        // `.` and `..` precede the real children; the kernel resumes at the last returned offset.
        let mut all: Vec<(u64, FileType, String)> = vec![
            (ino.0, FileType::Directory, ".".to_string()),
            (ino.0, FileType::Directory, "..".to_string()),
        ];
        for it in entries {
            let kind = match it.kind {
                NodeKind::Dir => FileType::Directory,
                NodeKind::Symlink => FileType::Symlink,
                NodeKind::File => FileType::RegularFile,
            };
            all.push((it.ino, kind, it.name));
        }
        for (i, (e_ino, e_kind, e_name)) in all.into_iter().enumerate().skip(offset as usize) {
            // The reported offset is the index of the *next* entry to fetch.
            if reply.add(INodeNo(e_ino), (i + 1) as u64, e_kind, e_name) {
                break;
            }
        }
        reply.ok();
    }

    fn open(&self, _req: &Request, _ino: INodeNo, _flags: fuser::OpenFlags, reply: ReplyOpen) {
        reply.opened(fuser::FileHandle(0), fuser::FopenFlags::empty());
    }

    fn flush(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: fuser::FileHandle,
        _lock_owner: fuser::LockOwner,
        reply: ReplyEmpty,
    ) {
        // On close, finalize a facet write-in: parse the just-written bytes into a record, or quarantine
        // them. A non-facet file is a no-op.
        let mut p = self.proj.lock().unwrap();
        match p
            .flush_overlay(ino.0)
            .and_then(|_| Self::save(&mut p, self.persist))
        {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(fuse_errno(&e)),
        }
    }

    fn fsync(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: fuser::FileHandle,
        _datasync: bool,
        reply: ReplyEmpty,
    ) {
        let mut p = self.proj.lock().unwrap();
        match p
            .flush_overlay(ino.0)
            .and_then(|_| Self::save(&mut p, self.persist))
        {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(fuse_errno(&e)),
        }
    }

    fn getxattr(
        &self,
        _req: &Request,
        ino: INodeNo,
        name: &std::ffi::OsStr,
        size: u32,
        reply: fuser::ReplyXattr,
    ) {
        let name = name.to_string_lossy();
        let p = self.proj.lock().unwrap();
        match p.getxattr(ino.0, &name) {
            Ok(Some(value)) => {
                if size == 0 {
                    reply.size(value.len() as u32);
                } else if (value.len() as u32) <= size {
                    reply.data(&value);
                } else {
                    reply.error(fuser::Errno::ERANGE);
                }
            }
            // No such attribute: NO_XATTR (ENODATA on Linux, ENOATTR on macOS) - portable.
            Ok(None) => reply.error(fuser::Errno::NO_XATTR),
            Err(e) => reply.error(fuse_errno(&e)),
        }
    }

    fn listxattr(&self, _req: &Request, ino: INodeNo, size: u32, reply: fuser::ReplyXattr) {
        let p = self.proj.lock().unwrap();
        match p.listxattr(ino.0) {
            Ok(names) => {
                // The xattr name list is NUL-terminated names concatenated.
                let mut buf = Vec::new();
                for n in names {
                    buf.extend_from_slice(n.as_bytes());
                    buf.push(0);
                }
                if size == 0 {
                    reply.size(buf.len() as u32);
                } else if (buf.len() as u32) <= size {
                    reply.data(&buf);
                } else {
                    reply.error(fuser::Errno::ERANGE);
                }
            }
            Err(e) => reply.error(fuse_errno(&e)),
        }
    }
}

/// Build mount options: a read-only flag when requested, plus a stable source name.
fn mount_config(read_only: bool) -> Config {
    let mut cfg = Config::default();
    cfg.mount_options = vec![MountOption::FSName("loom".into())];
    if read_only {
        cfg.mount_options.push(MountOption::RO);
    }
    cfg
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
struct DarwinMount {
    mountpoint: PathBuf,
}

#[cfg(target_os = "macos")]
impl DarwinMount {
    fn mount(
        mountpoint: &Path,
        options: &[MountOption],
        acl: fuser::SessionACL,
    ) -> std::io::Result<(OwnedFd, Self)> {
        use std::os::unix::fs::PermissionsExt;

        let mountpoint = mountpoint.canonicalize()?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(FUSE_DEVICE)?;
        let mountpoint_mode = File::open(&mountpoint)?.metadata()?.permissions().mode();
        match Self::mount_sys(&file, &mountpoint, mountpoint_mode, options, acl) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                Self::mount_helper(&file, &mountpoint, options)?;
            }
            Err(e) => return Err(e),
        }
        Ok((file.into(), Self { mountpoint }))
    }

    fn mount_sys(
        file: &File,
        mountpoint: &Path,
        mountpoint_mode: u32,
        options: &[MountOption],
        acl: fuser::SessionACL,
    ) -> std::io::Result<()> {
        let mut mount_options = format!(
            "fd={},rootmode={:o},user_id={},group_id={}",
            file.as_raw_fd(),
            mountpoint_mode,
            nix::unistd::getuid(),
            nix::unistd::getgid()
        );
        for option in options {
            if let Some(value) = darwin_kernel_option(option) {
                mount_options.push(',');
                mount_options.push_str(&value);
            }
        }
        if matches!(
            acl,
            fuser::SessionACL::All | fuser::SessionACL::RootAndOwner
        ) {
            mount_options.push_str(",allow_other");
        }
        let source = darwin_mount_source(options);
        nix::mount::mount(
            source.as_str(),
            mountpoint,
            darwin_mount_flags(options),
            Some(mount_options.as_str()),
        )
        .map_err(std::io::Error::from)
    }

    fn mount_helper(
        file: &File,
        mountpoint: &Path,
        options: &[MountOption],
    ) -> std::io::Result<()> {
        let fd_flags =
            FdFlag::from_bits_retain(fcntl(file, FcntlArg::F_GETFD).map_err(std::io::Error::from)?);
        fcntl(file, FcntlArg::F_SETFD(fd_flags & !FdFlag::FD_CLOEXEC))
            .map_err(std::io::Error::from)?;
        let mut command = Command::new(MOUNT_FUSEFS_BIN);
        let helper_options = darwin_helper_options(options);
        if !helper_options.is_empty() {
            command.arg("-o").arg(helper_options.join(","));
        }
        let output = command
            .arg(file.as_raw_fd().to_string())
            .arg(mountpoint)
            .output();
        let restore = fcntl(file, FcntlArg::F_SETFD(fd_flags)).map_err(std::io::Error::from);
        let output = output?;
        restore?;
        if output.status.success() {
            Ok(())
        } else {
            Err(std::io::Error::other(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ))
        }
    }

    fn unmount(&self) -> std::io::Result<()> {
        nix::mount::unmount(self.mountpoint.as_path(), MntFlags::MNT_FORCE)
            .map_err(std::io::Error::from)
    }
}

#[cfg(target_os = "macos")]
impl Drop for DarwinMount {
    fn drop(&mut self) {
        let _ = self.unmount();
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
pub struct BackgroundSession {
    session: Option<fuser::BackgroundSession>,
    mount: Option<DarwinMount>,
}

#[cfg(target_os = "macos")]
impl BackgroundSession {
    pub fn umount_and_join(mut self) -> std::io::Result<()> {
        if let Some(mount) = self.mount.take() {
            mount.unmount()?;
        }
        self.join()
    }

    pub fn join(mut self) -> std::io::Result<()> {
        if let Some(session) = self.session.take() {
            session.join()
        } else {
            Ok(())
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for BackgroundSession {
    fn drop(&mut self) {
        if let Some(mount) = self.mount.take() {
            let _ = mount.unmount();
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub type BackgroundSession = fuser::BackgroundSession;

#[cfg(target_os = "macos")]
fn darwin_mount_source(options: &[MountOption]) -> String {
    let mut source = FUSE_DEVICE.to_string();
    for option in options {
        match option {
            MountOption::Subtype(subtype) | MountOption::FSName(subtype) => {
                source = subtype.to_string();
            }
            _ => {}
        }
    }
    source
}

#[cfg(target_os = "macos")]
fn darwin_mount_flags(options: &[MountOption]) -> MntFlags {
    let mut flags = MntFlags::MNT_NODEV | MntFlags::MNT_NOSUID;
    for option in options {
        match option {
            MountOption::Dev => flags.remove(MntFlags::MNT_NODEV),
            MountOption::NoDev => flags.insert(MntFlags::MNT_NODEV),
            MountOption::Suid => flags.remove(MntFlags::MNT_NOSUID),
            MountOption::NoSuid => flags.insert(MntFlags::MNT_NOSUID),
            MountOption::RO => flags.insert(MntFlags::MNT_RDONLY),
            MountOption::RW => flags.remove(MntFlags::MNT_RDONLY),
            _ => {}
        }
    }
    flags
}

#[cfg(target_os = "macos")]
fn darwin_kernel_option(option: &MountOption) -> Option<String> {
    match option {
        MountOption::CUSTOM(value) => Some(value.to_string()),
        MountOption::DefaultPermissions => Some("default_permissions".to_string()),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
fn darwin_helper_options(options: &[MountOption]) -> Vec<String> {
    options.iter().map(darwin_option_value).collect()
}

#[cfg(target_os = "macos")]
fn darwin_option_value(option: &MountOption) -> String {
    match option {
        MountOption::FSName(name) => format!("fsname={name}"),
        MountOption::Subtype(subtype) => format!("subtype={subtype}"),
        MountOption::CUSTOM(value) => value.to_string(),
        MountOption::AutoUnmount => "auto_unmount".to_string(),
        MountOption::DefaultPermissions => "default_permissions".to_string(),
        MountOption::Dev => "dev".to_string(),
        MountOption::NoDev => "nodev".to_string(),
        MountOption::Suid => "suid".to_string(),
        MountOption::NoSuid => "nosuid".to_string(),
        MountOption::RO => "ro".to_string(),
        MountOption::RW => "rw".to_string(),
        MountOption::Exec => "exec".to_string(),
        MountOption::NoExec => "noexec".to_string(),
        MountOption::Atime => "atime".to_string(),
        MountOption::NoAtime => "noatime".to_string(),
        MountOption::DirSync => "dirsync".to_string(),
        MountOption::Sync => "sync".to_string(),
        MountOption::Async => "async".to_string(),
    }
}

/// Mount the workspace `workspace` of the loom at `loom_path` at `mountpoint`, blocking until the
/// filesystem is unmounted. `read_only` projects the working tree without mutations.
pub fn mount(
    loom_path: &Path,
    workspace: &str,
    mountpoint: &Path,
    read_only: bool,
) -> std::io::Result<()> {
    mount_with_auth(
        loom_path,
        workspace,
        mountpoint,
        read_only,
        LocalOpenAuth::default(),
    )
}

pub fn mount_with_auth(
    loom_path: &Path,
    workspace: &str,
    mountpoint: &Path,
    read_only: bool,
    auth: LocalOpenAuth,
) -> std::io::Result<()> {
    let fs = build_fs(loom_path, workspace, read_only, &auth)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    #[cfg(target_os = "macos")]
    {
        let cfg = mount_config(read_only);
        let (fd, mount) = DarwinMount::mount(mountpoint, &cfg.mount_options, cfg.acl)?;
        let session = fuser::Session::from_fd(fs, fd, cfg.acl, cfg)?.spawn()?;
        BackgroundSession {
            session: Some(session),
            mount: Some(mount),
        }
        .join()
    }
    #[cfg(not(target_os = "macos"))]
    {
        fuser::mount2(fs, mountpoint, &mount_config(read_only))
    }
}

/// Mount in a background thread, returning the session; dropping it unmounts. Used by the smoke test.
pub fn spawn(
    loom_path: &Path,
    workspace: &str,
    mountpoint: &Path,
    read_only: bool,
) -> std::io::Result<BackgroundSession> {
    spawn_with_auth(
        loom_path,
        workspace,
        mountpoint,
        read_only,
        LocalOpenAuth::default(),
    )
}

pub fn spawn_with_auth(
    loom_path: &Path,
    workspace: &str,
    mountpoint: &Path,
    read_only: bool,
    auth: LocalOpenAuth,
) -> std::io::Result<BackgroundSession> {
    let fs = build_fs(loom_path, workspace, read_only, &auth)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    #[cfg(target_os = "macos")]
    {
        let cfg = mount_config(read_only);
        let (fd, mount) = DarwinMount::mount(mountpoint, &cfg.mount_options, cfg.acl)?;
        let session = fuser::Session::from_fd(fs, fd, cfg.acl, cfg)?.spawn()?;
        Ok(BackgroundSession {
            session: Some(session),
            mount: Some(mount),
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        fuser::spawn_mount2(fs, mountpoint, &mount_config(read_only))
    }
}

/// Open the loom and build the [`LoomFuse`] for `workspace`.
fn build_fs(
    loom_path: &Path,
    workspace: &str,
    read_only: bool,
    auth: &LocalOpenAuth,
) -> Result<LoomFuse, LoomError> {
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
    Ok(LoomFuse::new(proj, !read_only, 0, 0))
}

fn resolve_ns(loom: &loom_core::Loom<FileStore>, name: &str) -> Result<WorkspaceId, LoomError> {
    let selector = match WorkspaceId::parse(name) {
        Ok(id) => loom_core::WsSelector::Id(id),
        Err(_) => loom_core::WsSelector::Name(name.to_string()),
    };
    loom.registry().open(&selector)
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::error::Code;

    #[test]
    fn mount_config_sets_read_only_option_only_when_requested() {
        let rw = mount_config(false);
        assert!(
            rw.mount_options
                .iter()
                .any(|option| matches!(option, MountOption::FSName(name) if name == "loom"))
        );
        assert!(
            !rw.mount_options
                .iter()
                .any(|option| matches!(option, MountOption::RO))
        );

        let ro = mount_config(true);
        assert!(
            ro.mount_options
                .iter()
                .any(|option| matches!(option, MountOption::RO))
        );
    }

    #[test]
    fn file_attr_maps_portable_attrs_to_fuse_attrs() {
        let file = file_attr(
            &Attr {
                ino: 7,
                kind: NodeKind::File,
                size: 513,
                mode: 0o100640,
            },
            501,
            20,
        );
        assert_eq!(file.ino, INodeNo(7));
        assert_eq!(file.kind, FileType::RegularFile);
        assert_eq!(file.perm, 0o640);
        assert_eq!(file.uid, 501);
        assert_eq!(file.gid, 20);
        assert_eq!(file.blocks, 2);

        let dir = file_attr(
            &Attr {
                ino: 8,
                kind: NodeKind::Dir,
                size: 0,
                mode: 0o040700,
            },
            501,
            20,
        );
        assert_eq!(dir.kind, FileType::Directory);
        assert_eq!(dir.perm, 0o755);

        let symlink = file_attr(
            &Attr {
                ino: 9,
                kind: NodeKind::Symlink,
                size: 4,
                mode: 0o120000,
            },
            501,
            20,
        );
        assert_eq!(symlink.kind, FileType::Symlink);
        assert_eq!(symlink.perm, 0o777);
    }

    #[test]
    fn fuse_errno_tracks_shared_projection_errno_mapping() {
        assert_eq!(
            fuse_errno(&LoomError::new(Code::NotFound, "missing")).code(),
            fuser::Errno::ENOENT.code()
        );
        assert_eq!(
            fuse_errno(&LoomError::new(Code::PermissionDenied, "denied")).code(),
            fuser::Errno::EACCES.code()
        );
        assert_eq!(
            fuse_errno(&LoomError::new(Code::Unsupported, "read-only")).code(),
            fuser::Errno::EROFS.code()
        );
    }
}
