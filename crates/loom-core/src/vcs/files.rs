use super::*;

impl<S: ObjectStore> Loom<S> {
    /// Stage `bytes` at `path` in `ns`'s working tree. The content is stored as a Blob and indexed
    /// by its content address; the working tree records the address and `mode`. If a file handle is open
    /// on `path`, the write routes through that live inode (whole-content replace), so every open handle
    /// sees the new bytes (`O_TRUNC` on the same inode).
    pub fn write_file(
        &mut self,
        ns: WorkspaceId,
        path: &str,
        bytes: &[u8],
        mode: u32,
    ) -> Result<()> {
        self.ensure_full_state_loaded()?;
        let path = normalize_path(path)?;
        self.authorize_path(ns, &path, AclRight::Write)?;
        guard_reserved_write(&path)?;
        self.write_file_norm(ns, &path, bytes, mode)
    }

    /// Privileged `write_file` for facet implementations - the in-core typed facades (kv, cas, graph,
    /// ...) and external facet crates such as `loom-sql` - to write their own `.loom/facets/<facet>/...`
    /// storage. The public [`write_file`](Self::write_file) refuses that reserved subtree for user
    /// callers (0014a baseline); this is the sanctioned facet-storage write and is not projected through
    /// the C ABI, CLI, or language bindings, so end users cannot reach it.
    pub fn write_file_reserved(
        &mut self,
        ns: WorkspaceId,
        path: &str,
        bytes: &[u8],
        mode: u32,
    ) -> Result<()> {
        self.ensure_full_state_loaded()?;
        let path = normalize_path(path)?;
        self.write_file_norm(ns, &path, bytes, mode)
    }

    /// Shared body of [`write_file`](Self::write_file): `path` is already normalized and the
    /// reserved-path policy has been applied by the caller.
    fn write_file_norm(
        &mut self,
        ns: WorkspaceId,
        path: &str,
        bytes: &[u8],
        mode: u32,
    ) -> Result<()> {
        if self.dir_exists(ns, path) {
            return Err(LoomError::new(
                Code::AlreadyExists,
                format!("{path:?} is a directory"),
            ));
        }
        if let Some((parent, _)) = path.rsplit_once('/')
            && !self.dir_exists(ns, parent)
        {
            return Err(LoomError::not_found(format!(
                "parent directory of {path:?} does not exist"
            )));
        }
        let content_addr = self.store_content(ns, bytes)?;
        self.put_file_slot(ns, path, content_addr, bytes.len() as u64, mode);
        Ok(())
    }

    /// Append `bytes` to the end of file `path` in `ns`'s working tree, POSIX-style: the file is created
    /// if absent (its content becomes `bytes`), and an existing file keeps its mode. The parent
    /// directory must already exist (`NOT_FOUND` otherwise), and appending to a directory is
    /// `ALREADY_EXISTS`. A single atomic working-tree mutation.
    pub fn append_file(&mut self, ns: WorkspaceId, path: &str, bytes: &[u8]) -> Result<()> {
        self.ensure_full_state_loaded()?;
        let path = normalize_path(path)?;
        self.authorize_path(ns, &path, AclRight::Write)?;
        guard_reserved_write(&path)?;
        if self.dir_exists(ns, &path) {
            return Err(LoomError::new(
                Code::AlreadyExists,
                format!("{path:?} is a directory"),
            ));
        }
        if let Some((parent, _)) = path.rsplit_once('/')
            && !self.dir_exists(ns, parent)
        {
            return Err(LoomError::not_found(format!(
                "parent directory of {path:?} does not exist"
            )));
        }
        // Read the current content if the file exists (preserving its mode), else start empty with the
        // default regular-file mode - create-if-missing, like `>>`.
        let (mut content, mode) = match self.work.get(&ns).and_then(|w| w.get(&path)) {
            Some(StagedEntry::File(f)) => (self.load_content(f.content_addr)?, f.mode),
            Some(StagedEntry::Table(_)) => {
                return Err(LoomError::invalid(format!(
                    "{path:?} is a table, not a file"
                )));
            }
            Some(StagedEntry::Stream(_)) => {
                return Err(LoomError::invalid(format!(
                    "{path:?} is a stream, not a file"
                )));
            }
            Some(StagedEntry::TimeSeries(_)) => {
                return Err(LoomError::invalid(format!(
                    "{path:?} is a time-series collection, not a file"
                )));
            }
            Some(StagedEntry::Graph(_)) => {
                return Err(LoomError::invalid(format!(
                    "{path:?} is a graph collection, not a file"
                )));
            }
            Some(StagedEntry::Ledger(_)) => {
                return Err(LoomError::invalid(format!(
                    "{path:?} is a ledger collection, not a file"
                )));
            }
            Some(StagedEntry::Columnar(_)) => {
                return Err(LoomError::invalid(format!(
                    "{path:?} is a columnar dataset, not a file"
                )));
            }
            Some(StagedEntry::Document(_)) => {
                return Err(LoomError::invalid(format!(
                    "{path:?} is a document collection, not a file"
                )));
            }
            None => (Vec::new(), 0o100644),
        };
        content.extend_from_slice(bytes);
        let content_addr = self.store_content(ns, &content)?;
        self.put_file_slot(ns, &path, content_addr, content.len() as u64, mode);
        Ok(())
    }

    /// Read a staged file's bytes from `ns`'s working tree.
    pub fn read_file(&self, ns: WorkspaceId, path: &str) -> Result<Vec<u8>> {
        self.ensure_full_state_available()?;
        let path = normalize_path(path)?;
        self.authorize_path(ns, &path, AclRight::Read)?;
        self.read_file_norm(ns, &path)
    }

    /// Privileged `read_file` for typed facet implementations reading their reserved storage.
    pub fn read_file_reserved(&self, ns: WorkspaceId, path: &str) -> Result<Vec<u8>> {
        self.ensure_full_state_available()?;
        let path = normalize_path(path)?;
        self.read_file_norm(ns, &path)
    }

    fn read_file_norm(&self, ns: WorkspaceId, path: &str) -> Result<Vec<u8>> {
        match self.work.get(&ns).and_then(|w| w.get(path)) {
            Some(StagedEntry::File(f)) => self.load_content(f.content_addr),
            Some(StagedEntry::Table(_)) => Err(LoomError::invalid(format!(
                "{path:?} is a table, not a file"
            ))),
            Some(StagedEntry::Stream(_)) => Err(LoomError::invalid(format!(
                "{path:?} is a stream, not a file"
            ))),
            Some(StagedEntry::TimeSeries(_)) => Err(LoomError::invalid(format!(
                "{path:?} is a time-series collection, not a file"
            ))),
            Some(StagedEntry::Graph(_)) => Err(LoomError::invalid(format!(
                "{path:?} is a graph collection, not a file"
            ))),
            Some(StagedEntry::Ledger(_)) => Err(LoomError::invalid(format!(
                "{path:?} is a ledger collection, not a file"
            ))),
            Some(StagedEntry::Columnar(_)) => Err(LoomError::invalid(format!(
                "{path:?} is a columnar dataset, not a file"
            ))),
            Some(StagedEntry::Document(_)) => Err(LoomError::invalid(format!(
                "{path:?} is a document collection, not a file"
            ))),
            None => Err(LoomError::not_found(format!("{path:?} not staged"))),
        }
    }

    /// Remove a path from `ns`'s working tree (a staged deletion). If a file handle is open on `path`,
    /// this unlinks it POSIX-style: the path goes away immediately, but the open handles keep reading and
    /// writing the now-detached inode until the last one closes (delete-on-last-close), and a write
    /// through such a handle does not resurrect the path.
    pub fn remove_file(&mut self, ns: WorkspaceId, path: &str) -> Result<()> {
        self.ensure_full_state_loaded()?;
        let path = normalize_path(path)?;
        self.authorize_path(ns, &path, AclRight::Write)?;
        guard_reserved_write(&path)?;
        self.remove_file_norm(ns, &path);
        Ok(())
    }

    /// Privileged twin of [`remove_file`](Self::remove_file) for facet implementers: unlinks a path
    /// under a reserved facet root (e.g. `.loom/facets/cas/...`) without the reserved-path guard. Like
    /// `remove_file`, it is idempotent - removing an absent path is a no-op.
    pub fn remove_file_reserved(&mut self, ns: WorkspaceId, path: &str) -> Result<()> {
        self.ensure_full_state_loaded()?;
        let path = normalize_path(path)?;
        self.remove_file_norm(ns, &path);
        Ok(())
    }

    /// Shared body of [`remove_file`](Self::remove_file): `path` is already normalized and the
    /// reserved-path policy has been applied by the caller. Unlinks the working-tree entry; the bytes
    /// become unreferenced and are reclaimed by GC once no other root retains them.
    fn remove_file_norm(&mut self, ns: WorkspaceId, path: &str) {
        if let Some(id) = self.path_to_inode.remove(&(ns, path.to_string()))
            && let Some(ino) = self.inodes.get_mut(&id)
        {
            ino.path = None;
        }
        if let Some(w) = self.work.get_mut(&ns) {
            w.remove(path);
        }
    }

    /// Create a symbolic link at `link_path` whose target is `target`, an opaque path string (it need
    /// not exist; dangling links are allowed, like `symlink(2)`). The link is stored git-style as a file
    /// slot carrying the symlink mode (`S_IFLNK`) with the target bytes as its content, so it commits,
    /// checks out, diffs, and syncs through the ordinary file machinery. The parent directory must exist
    /// (`NOT_FOUND`), and `link_path` must not already exist as a file, directory, or link
    /// (`ALREADY_EXISTS`, like `symlink(2)` `EEXIST`). Symlinks are opaque: other `fs` operations do not
    /// follow them.
    pub fn symlink(&mut self, ns: WorkspaceId, target: &str, link_path: &str) -> Result<()> {
        self.ensure_full_state_loaded()?;
        let path = normalize_path(link_path)?;
        guard_reserved_write(&path)?;
        if self.dir_exists(ns, &path) {
            return Err(LoomError::new(
                Code::AlreadyExists,
                format!("{path:?} is a directory"),
            ));
        }
        if self.work.get(&ns).and_then(|w| w.get(&path)).is_some() {
            return Err(LoomError::new(
                Code::AlreadyExists,
                format!("{path:?} already exists"),
            ));
        }
        if target.is_empty() {
            return Err(LoomError::invalid("symlink target must not be empty"));
        }
        if let Some((parent, _)) = path.rsplit_once('/')
            && !self.dir_exists(ns, parent)
        {
            return Err(LoomError::not_found(format!(
                "parent directory of {path:?} does not exist"
            )));
        }
        let content_addr = self.store_content(ns, target.as_bytes())?;
        self.put_file_slot(ns, &path, content_addr, target.len() as u64, SYMLINK_MODE);
        Ok(())
    }

    /// Read the target of the symbolic link at `path`. `NOT_FOUND` if absent; `INVALID_ARGUMENT` if the
    /// path is a regular file, table, or stream rather than a symlink (like `readlink(2)` `EINVAL`).
    pub fn read_link(&self, ns: WorkspaceId, path: &str) -> Result<String> {
        self.ensure_full_state_available()?;
        let path = normalize_path(path)?;
        match self.work.get(&ns).and_then(|w| w.get(&path)) {
            Some(StagedEntry::File(f)) if is_symlink_mode(f.mode) => {
                let bytes = self.load_content(f.content_addr)?;
                String::from_utf8(bytes).map_err(|_| {
                    LoomError::invalid(format!("symlink {path:?} target is not UTF-8"))
                })
            }
            Some(_) => Err(LoomError::invalid(format!("{path:?} is not a symlink"))),
            None => Err(LoomError::not_found(format!("{path:?}"))),
        }
    }

    /// The paths currently staged in `ns`'s working tree, sorted.
    pub fn staged_paths(&self, ns: WorkspaceId) -> Vec<String> {
        self.work
            .get(&ns)
            .map(|w| w.keys().cloned().collect())
            .unwrap_or_default()
    }

    // ---- byte-range file I/O (path form) --------------------------------------------------------

    /// The current content address and mode of the file staged at `path`, or an error if it is not a
    /// file. `None` means the path is absent (the caller decides whether to create it).
    pub(crate) fn file_slot(&self, ns: WorkspaceId, path: &str) -> Result<Option<(Digest, u32)>> {
        match self.work.get(&ns).and_then(|w| w.get(path)) {
            Some(StagedEntry::File(f)) => Ok(Some((f.content_addr, f.mode))),
            Some(StagedEntry::Table(_)) => Err(LoomError::invalid(format!(
                "{path:?} is a table, not a file"
            ))),
            Some(StagedEntry::Stream(_)) => Err(LoomError::invalid(format!(
                "{path:?} is a stream, not a file"
            ))),
            Some(StagedEntry::TimeSeries(_)) => Err(LoomError::invalid(format!(
                "{path:?} is a time-series collection, not a file"
            ))),
            Some(StagedEntry::Graph(_)) => Err(LoomError::invalid(format!(
                "{path:?} is a graph collection, not a file"
            ))),
            Some(StagedEntry::Ledger(_)) => Err(LoomError::invalid(format!(
                "{path:?} is a ledger collection, not a file"
            ))),
            Some(StagedEntry::Columnar(_)) => Err(LoomError::invalid(format!(
                "{path:?} is a columnar dataset, not a file"
            ))),
            Some(StagedEntry::Document(_)) => Err(LoomError::invalid(format!(
                "{path:?} is a document collection, not a file"
            ))),
            None => Ok(None),
        }
    }

    /// Reject a path that names an existing directory, and require its parent directory to exist.
    pub(crate) fn check_file_target(&self, ns: WorkspaceId, path: &str) -> Result<()> {
        if self.dir_exists(ns, path) {
            return Err(LoomError::new(
                Code::AlreadyExists,
                format!("{path:?} is a directory"),
            ));
        }
        if let Some((parent, _)) = path.rsplit_once('/')
            && !self.dir_exists(ns, parent)
        {
            return Err(LoomError::not_found(format!(
                "parent directory of {path:?} does not exist"
            )));
        }
        Ok(())
    }
}
