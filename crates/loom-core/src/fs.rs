//! The `fs` filesystem facade over a workspace's working tree.
//!
//! POSIX-like file and directory operations layered on a workspace working tree
//! (`crate::vcs::Loom`). Read/write/delete of file content are the `Loom` working-tree methods
//! (`read_file`/`write_file`/`remove_file`); this module adds metadata, directory listing, and
//! move/copy/walk.
//!
//! **Directories are first-class, never implicit.** `create_directory` records a directory that
//! then exists, including an empty one, and it persists across commit/checkout/sync (an empty
//! directory commits as an empty Tree). Writing a file requires its parent directory to
//! already exist (`NOT_FOUND` otherwise), like `open(O_CREAT)` on a real filesystem; create the
//! directory first.

use crate::error::{Code, LoomError, Result};
use crate::provider::ObjectStore;
use crate::vcs::{Loom, StagedEntry, guard_reserved_write, normalize_path};
use crate::workspace::WorkspaceId;
use std::collections::BTreeMap;

const DIR_MODE: u32 = 0o040000;
const DEFAULT_FILE_MODE: u32 = 0o100644;

const fn file_kind_rank(kind: FileKind) -> u8 {
    match kind {
        FileKind::Directory => 0,
        FileKind::File => 1,
        FileKind::Symlink => 2,
    }
}

/// What a path resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// A regular file.
    File,
    /// A directory.
    Directory,
    /// A symbolic link (its content is the opaque target path; see [`Loom::read_link`]).
    Symlink,
}

/// Metadata about a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stat {
    /// The normalized path (no leading `/`).
    pub path: String,
    /// File or directory.
    pub kind: FileKind,
    /// Byte length for a file; 0 for a directory.
    pub size: u64,
    /// POSIX-style mode bits.
    pub mode: u32,
}

/// One entry in a directory listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// The entry's own name (no path separators).
    pub name: String,
    /// File or directory.
    pub kind: FileKind,
}

/// One file-tree entry materialized from a committed revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommittedFsEntry {
    /// The normalized path (no leading `/`).
    pub path: String,
    /// File, directory, or symlink.
    pub kind: FileKind,
    /// POSIX-style mode bits.
    pub mode: u32,
    /// File or symlink payload bytes; empty for directories.
    pub bytes: Vec<u8>,
}

impl<S: ObjectStore> Loom<S> {
    /// Materialize committed filesystem entries from `rev` without changing the working tree.
    pub fn committed_fs_entries(
        &self,
        ns: WorkspaceId,
        rev: &str,
    ) -> Result<Vec<CommittedFsEntry>> {
        self.authorize(ns, crate::FacetKind::Vcs, crate::acl::AclRight::Read)?;
        let commit = self.resolve_rev(ns, rev)?;
        let (files, dirs) = self.flatten_commit(commit)?;
        let mut entries = Vec::new();
        for path in dirs {
            entries.push(CommittedFsEntry {
                path,
                kind: FileKind::Directory,
                mode: DIR_MODE,
                bytes: Vec::new(),
            });
        }
        for (path, slot) in files {
            match slot {
                StagedEntry::File(file) => {
                    let kind = if crate::vcs::is_symlink_mode(file.mode) {
                        FileKind::Symlink
                    } else {
                        FileKind::File
                    };
                    entries.push(CommittedFsEntry {
                        path,
                        kind,
                        mode: file.mode,
                        bytes: self.load_content(file.content_addr)?,
                    });
                }
                StagedEntry::Table(_) => {
                    return Err(LoomError::unsupported(format!(
                        "committed path {path:?} is a table, not a filesystem entry"
                    )));
                }
                StagedEntry::Stream(_) => {
                    return Err(LoomError::unsupported(format!(
                        "committed path {path:?} is a stream, not a filesystem entry"
                    )));
                }
                StagedEntry::TimeSeries(_) => {
                    return Err(LoomError::unsupported(format!(
                        "committed path {path:?} is a time-series collection, not a filesystem entry"
                    )));
                }
                StagedEntry::Graph(_) => {
                    return Err(LoomError::unsupported(format!(
                        "committed path {path:?} is a graph collection, not a filesystem entry"
                    )));
                }
                StagedEntry::Ledger(_) => {
                    return Err(LoomError::unsupported(format!(
                        "committed path {path:?} is a ledger collection, not a filesystem entry"
                    )));
                }
                StagedEntry::Columnar(_) => {
                    return Err(LoomError::unsupported(format!(
                        "committed path {path:?} is a columnar dataset, not a filesystem entry"
                    )));
                }
                StagedEntry::Document(_) => {
                    return Err(LoomError::unsupported(format!(
                        "committed path {path:?} is a document collection, not a filesystem entry"
                    )));
                }
            }
        }
        entries.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then_with(|| file_kind_rank(a.kind).cmp(&file_kind_rank(b.kind)))
        });
        Ok(entries)
    }

    /// Whether `path` resolves to a file or a directory in `ns`'s working tree.
    pub fn exists(&self, ns: WorkspaceId, path: &str) -> Result<bool> {
        let norm = normalize_path(path)?;
        Ok(self.is_file(ns, &norm) || self.is_dir(ns, &norm))
    }

    /// Metadata for `path`; `NOT_FOUND` if it resolves to neither a file nor a directory.
    pub fn stat(&self, ns: WorkspaceId, path: &str) -> Result<Stat> {
        let norm = normalize_path(path)?;
        if let Some(mode) = self.file_mode(ns, &norm) {
            let size = self.read_file(ns, &norm)?.len() as u64;
            let kind = if crate::vcs::is_symlink_mode(mode) {
                FileKind::Symlink
            } else {
                FileKind::File
            };
            return Ok(Stat {
                path: norm,
                kind,
                size,
                mode,
            });
        }
        if self.is_dir(ns, &norm) {
            return Ok(Stat {
                path: norm,
                kind: FileKind::Directory,
                size: 0,
                mode: DIR_MODE,
            });
        }
        Err(LoomError::not_found(format!("{path:?}")))
    }

    /// List the immediate children of directory `path` (root is `""` or `"/"`), sorted by name.
    /// `NOT_FOUND` if `path` is not an existing directory.
    pub fn list_directory(&self, ns: WorkspaceId, path: &str) -> Result<Vec<DirEntry>> {
        let dir = dir_path(path);
        if !dir.is_empty() && !self.is_dir(ns, &dir) {
            return Err(LoomError::not_found(format!("{path:?}")));
        }
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        // child name -> is it a directory?
        let mut children: BTreeMap<String, bool> = BTreeMap::new();
        for key in self.staged_paths(ns) {
            if let Some(rest) = key.strip_prefix(&prefix) {
                if rest.is_empty() {
                    continue;
                }
                match rest.split_once('/') {
                    Some((seg, _)) => {
                        children.insert(seg.to_string(), true);
                    }
                    None => {
                        children.entry(rest.to_string()).or_insert(false);
                    }
                }
            }
        }
        if let Some(set) = self.dirs.get(&ns) {
            for d in set {
                if let Some(rest) = d.strip_prefix(&prefix) {
                    if rest.is_empty() {
                        continue;
                    }
                    let seg = rest.split_once('/').map_or(rest, |(s, _)| s);
                    children.insert(seg.to_string(), true);
                }
            }
        }
        Ok(children
            .into_iter()
            .map(|(name, is_dir)| DirEntry {
                name,
                kind: if is_dir {
                    FileKind::Directory
                } else {
                    FileKind::File
                },
            })
            .collect())
    }

    /// Create directory `path`. It then exists (even empty) and persists across commit/checkout.
    /// `ALREADY_EXISTS` if `path` is a file; idempotent if it is already a directory. Without
    /// `recursive`, the parent must already exist (`NOT_FOUND`), as on a real filesystem.
    pub fn create_directory(&mut self, ns: WorkspaceId, path: &str, recursive: bool) -> Result<()> {
        let norm = normalize_path(path)?;
        guard_reserved_write(&norm)?;
        self.create_directory_norm(ns, &norm, path, recursive)
    }

    /// Privileged `create_directory` for facet implementations - the in-core typed facades and external
    /// facet crates such as `loom-sql` - to create directories in their own `.loom/facets/<facet>/...`
    /// storage. The public [`create_directory`](Self::create_directory) refuses that reserved subtree
    /// for user callers (0014a); this is the sanctioned facet-storage path and is not projected through
    /// the C ABI, CLI, or language bindings.
    pub fn create_directory_reserved(
        &mut self,
        ns: WorkspaceId,
        path: &str,
        recursive: bool,
    ) -> Result<()> {
        let norm = normalize_path(path)?;
        self.create_directory_norm(ns, &norm, path, recursive)
    }

    /// Shared body of [`create_directory`](Self::create_directory). `norm` is the normalized path;
    /// `path` is the caller's original spelling, used only for error messages.
    fn create_directory_norm(
        &mut self,
        ns: WorkspaceId,
        norm: &str,
        path: &str,
        recursive: bool,
    ) -> Result<()> {
        if self.is_file(ns, norm) {
            return Err(LoomError::new(
                Code::AlreadyExists,
                format!("{path:?} is a file"),
            ));
        }
        if self.is_dir(ns, norm) {
            return Ok(());
        }
        if !recursive
            && let Some((parent, _)) = norm.rsplit_once('/')
            && !parent.is_empty()
            && !self.is_dir(ns, parent)
        {
            return Err(LoomError::not_found(format!(
                "parent directory of {path:?} does not exist"
            )));
        }
        self.record_dir(ns, norm);
        Ok(())
    }

    /// Remove directory `path`. With `recursive`, delete every file and sub-directory under it;
    /// without, error if it has any child. `NOT_FOUND` if the directory does not exist.
    pub fn remove_directory(&mut self, ns: WorkspaceId, path: &str, recursive: bool) -> Result<()> {
        let norm = normalize_path(path)?;
        guard_reserved_write(&norm)?;
        if !self.is_dir(ns, &norm) {
            return Err(LoomError::not_found(format!("{path:?}")));
        }
        let prefix = format!("{norm}/");
        let child_files: Vec<String> = self
            .staged_paths(ns)
            .into_iter()
            .filter(|k| k.starts_with(&prefix))
            .collect();
        let has_child_dir = self
            .dirs
            .get(&ns)
            .is_some_and(|set| set.iter().any(|k| k.starts_with(&prefix)));
        if !recursive && (!child_files.is_empty() || has_child_dir) {
            return Err(LoomError::new(
                Code::InvalidArgument,
                format!("directory {path:?} is not empty"),
            ));
        }
        for key in child_files {
            self.remove_file(ns, &key)?;
        }
        if let Some(set) = self.dirs.get_mut(&ns) {
            set.retain(|k| k != &norm && !k.starts_with(&prefix));
        }
        Ok(())
    }

    /// Move a file or directory subtree from `src` to `dst` within `ns`.
    pub fn move_path(&mut self, ns: WorkspaceId, src: &str, dst: &str) -> Result<()> {
        // A move both writes the destination and removes the source, so neither end may be reserved.
        guard_reserved_write(&normalize_path(src)?)?;
        guard_reserved_write(&normalize_path(dst)?)?;
        self.copy_path(ns, src, dst, true)?;
        let s = normalize_path(src)?;
        if self.is_file(ns, &s) {
            self.remove_file(ns, &s)?;
            return Ok(());
        }
        let prefix = format!("{s}/");
        let under: Vec<String> = self
            .staged_paths(ns)
            .into_iter()
            .filter(|k| k.starts_with(&prefix))
            .collect();
        for key in under {
            self.remove_file(ns, &key)?;
        }
        if let Some(set) = self.dirs.get_mut(&ns) {
            set.retain(|k| k != &s && !k.starts_with(&prefix));
        }
        Ok(())
    }

    /// Copy a file (or a directory subtree, with `recursive`) from `src` to `dst` within `ns`.
    pub fn copy_path(
        &mut self,
        ns: WorkspaceId,
        src: &str,
        dst: &str,
        recursive: bool,
    ) -> Result<()> {
        let s = normalize_path(src)?;
        let d = normalize_path(dst)?;
        if self.is_file(ns, &s) {
            let mode = self.file_mode(ns, &s).unwrap_or(DEFAULT_FILE_MODE);
            let bytes = self.read_file(ns, &s)?;
            self.write_file(ns, &d, &bytes, mode)?;
            return Ok(());
        }
        if !self.is_dir(ns, &s) {
            return Err(LoomError::not_found(format!("{src:?}")));
        }
        if !recursive {
            return Err(LoomError::new(
                Code::InvalidArgument,
                format!("{src:?} is a directory; pass recursive"),
            ));
        }
        let prefix = format!("{s}/");
        // Snapshot files and sub-directories under src before mutating the working tree.
        let mut files: Vec<(String, u32, Vec<u8>)> = Vec::new();
        for key in self
            .staged_paths(ns)
            .into_iter()
            .filter(|k| k.starts_with(&prefix))
        {
            let mode = self.file_mode(ns, &key).unwrap_or(DEFAULT_FILE_MODE);
            let bytes = self.read_file(ns, &key)?;
            files.push((key[prefix.len()..].to_string(), mode, bytes));
        }
        let sub_dirs: Vec<String> = self
            .dirs
            .get(&ns)
            .map(|set| {
                set.iter()
                    .filter(|k| *k == &s || k.starts_with(&prefix))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        // Create destination directories first, so the strict `write_file` finds their parents.
        self.record_dir(ns, &d);
        for sd in sub_dirs {
            let rel = if sd == s { "" } else { &sd[prefix.len()..] };
            let target = if rel.is_empty() {
                d.clone()
            } else {
                format!("{d}/{rel}")
            };
            self.record_dir(ns, &target);
        }
        for (rel, mode, bytes) in files {
            self.write_file(ns, &format!("{d}/{rel}"), &bytes, mode)?;
        }
        Ok(())
    }

    /// All file paths at or under `root` (root is `""`/`"/"`), sorted. A file `root` returns itself.
    pub fn walk(&self, ns: WorkspaceId, root: &str) -> Result<Vec<String>> {
        let dir = dir_path(root);
        if !dir.is_empty() && self.is_file(ns, &dir) {
            return Ok(vec![dir]);
        }
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        Ok(self
            .staged_paths(ns)
            .into_iter()
            .filter(|k| k.starts_with(&prefix))
            .collect())
    }

    fn is_file(&self, ns: WorkspaceId, norm: &str) -> bool {
        self.work
            .get(&ns)
            .and_then(|w| w.get(norm))
            .is_some_and(|e| matches!(e, crate::vcs::StagedEntry::File(_)))
    }

    fn file_mode(&self, ns: WorkspaceId, norm: &str) -> Option<u32> {
        match self.work.get(&ns).and_then(|w| w.get(norm)) {
            Some(crate::vcs::StagedEntry::File(f)) => Some(f.mode),
            _ => None,
        }
    }

    fn is_dir(&self, ns: WorkspaceId, norm: &str) -> bool {
        self.dir_exists(ns, norm)
    }
}

/// Normalize a directory path: strip leading/trailing `/`; root (`""`/`"/"`) maps to `""`.
fn dir_path(path: &str) -> String {
    path.trim_start_matches('/')
        .trim_end_matches('/')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStore;
    use crate::workspace::{FacetKind, WorkspaceId};

    fn fs_loom() -> (Loom<MemoryStore>, WorkspaceId) {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Files, None, WorkspaceId::from_bytes([1; 16]))
            .unwrap();
        loom.create_directory(ns, "/src", false).unwrap();
        loom.create_directory(ns, "/docs", false).unwrap();
        for (p, c) in [
            ("/README.md", "r"),
            ("/src/main.rs", "m"),
            ("/src/lib.rs", "ll"),
            ("/docs/guide.md", "g"),
        ] {
            loom.write_file(ns, p, c.as_bytes(), 0o100644).unwrap();
        }
        (loom, ns)
    }

    #[test]
    fn list_root_and_subdir() {
        let (loom, ns) = fs_loom();
        assert_eq!(
            loom.list_directory(ns, "/").unwrap(),
            vec![
                DirEntry {
                    name: "README.md".into(),
                    kind: FileKind::File
                },
                DirEntry {
                    name: "docs".into(),
                    kind: FileKind::Directory
                },
                DirEntry {
                    name: "src".into(),
                    kind: FileKind::Directory
                },
            ]
        );
        assert_eq!(
            loom.list_directory(ns, "/src").unwrap(),
            vec![
                DirEntry {
                    name: "lib.rs".into(),
                    kind: FileKind::File
                },
                DirEntry {
                    name: "main.rs".into(),
                    kind: FileKind::File
                },
            ]
        );
    }

    #[test]
    fn write_into_missing_directory_fails() {
        let (mut loom, ns) = fs_loom();
        // `touch /f/f/f/file` with no such folders -> error (no implicit directories).
        assert_eq!(
            loom.write_file(ns, "/f/f/f/file", b"x", 0o100644)
                .unwrap_err()
                .code,
            Code::NotFound
        );
        // After creating the chain, the write succeeds.
        loom.create_directory(ns, "/f/f/f", true).unwrap();
        loom.write_file(ns, "/f/f/f/file", b"x", 0o100644).unwrap();
        assert_eq!(loom.read_file(ns, "/f/f/f/file").unwrap(), b"x");
    }

    #[test]
    fn stat_and_exists() {
        let (loom, ns) = fs_loom();
        let f = loom.stat(ns, "/src/lib.rs").unwrap();
        assert_eq!(f.kind, FileKind::File);
        assert_eq!(f.size, 2);
        assert_eq!(loom.stat(ns, "/src").unwrap().kind, FileKind::Directory);
        assert!(loom.exists(ns, "/docs").unwrap());
        assert!(!loom.exists(ns, "/nope").unwrap());
        assert_eq!(loom.stat(ns, "/nope").unwrap_err().code, Code::NotFound);
    }

    #[test]
    fn committed_entries_read_revision_without_checkout() {
        let (mut loom, ns) = fs_loom();
        let first = loom.commit(ns, "nas", "first", 1).unwrap();
        loom.write_file(ns, "/README.md", b"current", 0o100644)
            .unwrap();

        let entries = loom.committed_fs_entries(ns, &first.to_string()).unwrap();
        let readme = entries
            .iter()
            .find(|entry| entry.path == "README.md")
            .unwrap();
        assert_eq!(readme.kind, FileKind::File);
        assert_eq!(readme.bytes, b"r");
        assert!(entries.iter().any(|entry| {
            entry.path == "docs" && entry.kind == FileKind::Directory && entry.bytes.is_empty()
        }));
        assert_eq!(loom.read_file(ns, "/README.md").unwrap(), b"current");
    }

    #[test]
    fn create_directory_is_real() {
        let (mut loom, ns) = fs_loom();
        // A brand-new empty directory exists after creation and shows up in its parent listing.
        loom.create_directory(ns, "/src/empty", false).unwrap();
        assert!(loom.exists(ns, "/src/empty").unwrap());
        assert_eq!(
            loom.stat(ns, "/src/empty").unwrap().kind,
            FileKind::Directory
        );
        assert!(
            loom.list_directory(ns, "/src")
                .unwrap()
                .iter()
                .any(|e| e.name == "empty" && e.kind == FileKind::Directory)
        );
        // Non-recursive create under a missing parent is rejected (POSIX-like).
        assert_eq!(
            loom.create_directory(ns, "/no/such/deep", false)
                .unwrap_err()
                .code,
            Code::NotFound
        );
        // Recursive creates the whole chain.
        loom.create_directory(ns, "/a/b/c", true).unwrap();
        assert!(loom.exists(ns, "/a").unwrap());
        assert!(loom.exists(ns, "/a/b/c").unwrap());
        // Not over an existing file.
        assert_eq!(
            loom.create_directory(ns, "/README.md", false)
                .unwrap_err()
                .code,
            Code::AlreadyExists
        );
    }

    #[test]
    fn empty_directory_persists_through_commit_and_checkout() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Files, None, WorkspaceId::from_bytes([2; 16]))
            .unwrap();
        loom.write_file(ns, "/a.txt", b"a", 0o100644).unwrap();
        loom.create_directory(ns, "/empty", false).unwrap();
        let c0 = loom.commit(ns, "nas", "with empty dir", 1).unwrap();
        // Remove the directory locally, then check out the commit to restore it.
        loom.remove_directory(ns, "/empty", true).unwrap();
        assert!(!loom.exists(ns, "/empty").unwrap());
        loom.checkout_commit(ns, c0).unwrap();
        assert!(loom.exists(ns, "/empty").unwrap());
        assert_eq!(loom.stat(ns, "/empty").unwrap().kind, FileKind::Directory);
    }

    #[test]
    fn remove_directory_rules() {
        let (mut loom, ns) = fs_loom();
        assert_eq!(
            loom.remove_directory(ns, "/docs", false).unwrap_err().code,
            Code::InvalidArgument
        );
        loom.remove_directory(ns, "/docs", true).unwrap();
        assert!(!loom.exists(ns, "/docs").unwrap());
    }

    #[test]
    fn move_and_copy_subtree() {
        let (mut loom, ns) = fs_loom();
        loom.create_directory(ns, "/src/empty", false).unwrap();
        loom.move_path(ns, "/src", "/lib").unwrap();
        assert!(!loom.exists(ns, "/src").unwrap());
        assert!(loom.exists(ns, "/lib/main.rs").unwrap());
        assert!(loom.exists(ns, "/lib/empty").unwrap()); // empty subdir moved too
        assert_eq!(loom.read_file(ns, "/lib/main.rs").unwrap(), b"m");

        loom.copy_path(ns, "/docs/guide.md", "/docs/guide-2.md", false)
            .unwrap();
        assert!(loom.exists(ns, "/docs/guide-2.md").unwrap());
    }

    #[test]
    fn walk_lists_descendants() {
        let (loom, ns) = fs_loom();
        assert_eq!(
            loom.walk(ns, "/").unwrap(),
            vec!["README.md", "docs/guide.md", "src/lib.rs", "src/main.rs"]
        );
        assert_eq!(loom.walk(ns, "/README.md").unwrap(), vec!["README.md"]);
    }
}
