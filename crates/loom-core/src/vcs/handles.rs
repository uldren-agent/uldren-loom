use super::*;

impl<S: ObjectStore> Loom<S> {
    /// Read `[offset, offset + len)` of the file at `path`, loading only the overlapping chunks (bounded
    /// memory). Reads past the end return fewer bytes (POSIX `pread`); a missing file is `NOT_FOUND`.
    pub fn read_at(&self, ns: WorkspaceId, path: &str, offset: u64, len: u64) -> Result<Vec<u8>> {
        let path = normalize_path(path)?;
        self.authorize_path(ns, &path, AclRight::Read)?;
        match self.file_slot(ns, &path)? {
            Some((addr, _)) => self.content_read_range(addr, offset, len),
            None => Err(LoomError::not_found(format!("{path:?} not staged"))),
        }
    }

    /// Write `data` at byte `offset` of the file at `path`, creating the file if missing and zero-filling
    /// any gap between the old end and `offset` (POSIX `pwrite`). The parent directory must exist
    /// (`NOT_FOUND`), a directory path is `ALREADY_EXISTS`, and an open handle on `path` sees the change.
    /// Only the affected chunks are re-stored; the file is never fully materialized.
    pub fn write_at(
        &mut self,
        ns: WorkspaceId,
        path: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<()> {
        let path = normalize_path(path)?;
        self.authorize_path(ns, &path, AclRight::Write)?;
        guard_reserved_write(&path)?;
        self.check_file_target(ns, &path)?;
        let (src, old_size, mode) = match self.file_slot(ns, &path)? {
            Some((addr, mode)) => (Some(addr), self.content_size(addr)?, mode),
            None => (None, 0, 0o100644),
        };
        let (addr, size) =
            self.build_edited_content(ns, EditPlan::write(src, old_size, offset, data.to_vec()))?;
        self.put_file_slot(ns, &path, addr, size, mode);
        Ok(())
    }

    /// Resize the file at `path` to `size`, dropping bytes past it or zero-extending up to it (POSIX
    /// `truncate`). A missing file is created zero-filled to `size`. The parent directory must exist
    /// (`NOT_FOUND`); a directory path is `ALREADY_EXISTS`; an open handle on `path` sees the change.
    pub fn truncate_file(&mut self, ns: WorkspaceId, path: &str, size: u64) -> Result<()> {
        let path = normalize_path(path)?;
        self.authorize_path(ns, &path, AclRight::Write)?;
        guard_reserved_write(&path)?;
        self.check_file_target(ns, &path)?;
        let (src, old_size, mode) = match self.file_slot(ns, &path)? {
            Some((addr, mode)) => (Some(addr), self.content_size(addr)?, mode),
            None => (None, 0, 0o100644),
        };
        let (addr, new_size) =
            self.build_edited_content(ns, EditPlan::truncate(src, old_size, size))?;
        self.put_file_slot(ns, &path, addr, new_size, mode);
        Ok(())
    }

    // ---- file handles (open file descriptions) -------------------------------------------------

    /// Open a file handle on `path` in `ns` with `mode`, returning the handle id. The handle binds to an
    /// inode, not the path: two opens of the same path share one inode (each with its own offset), and
    /// the handle survives the path being renamed or unlinked. `Read` requires the file to exist
    /// (`NOT_FOUND`); the other modes create it if missing (the parent directory must exist). `Write`
    /// truncates the shared inode to empty on open. A directory path is `ALREADY_EXISTS`.
    pub fn file_open(&mut self, ns: WorkspaceId, path: &str, mode: OpenMode) -> Result<u64> {
        let path = normalize_path(path)?;
        if mode.can_read() {
            self.authorize_path(ns, &path, AclRight::Read)?;
        }
        if mode.can_write() {
            self.authorize_path(ns, &path, AclRight::Write)?;
        }
        // Opening for any write/create mode mutates the reserved subtree; only `Read` is allowed there.
        if mode != OpenMode::Read {
            guard_reserved_write(&path)?;
        }
        if self.dir_exists(ns, &path) {
            return Err(LoomError::new(
                Code::AlreadyExists,
                format!("{path:?} is a directory"),
            ));
        }
        let key = (ns, path.clone());
        let inode_id = if let Some(&id) = self.path_to_inode.get(&key) {
            if let Some(ino) = self.inodes.get_mut(&id) {
                ino.open_count += 1;
            }
            id
        } else {
            match self.file_slot(ns, &path)? {
                Some((addr, file_mode)) => {
                    let size = self.content_size(addr)?;
                    let id = self.alloc_inode(Inode {
                        ns,
                        path: Some(path.clone()),
                        content_addr: addr,
                        size,
                        mode: file_mode,
                        open_count: 1,
                    });
                    self.path_to_inode.insert(key, id);
                    id
                }
                None => {
                    if !mode.creates() {
                        return Err(LoomError::not_found(format!("{path:?} not staged")));
                    }
                    if let Some((parent, _)) = path.rsplit_once('/')
                        && !self.dir_exists(ns, parent)
                    {
                        return Err(LoomError::not_found(format!(
                            "parent directory of {path:?} does not exist"
                        )));
                    }
                    let empty = self.store_content(ns, b"")?;
                    let id = self.alloc_inode(Inode {
                        ns,
                        path: Some(path.clone()),
                        content_addr: empty,
                        size: 0,
                        mode: 0o100644,
                        open_count: 1,
                    });
                    self.path_to_inode.insert(key, id);
                    self.work.entry(ns).or_default().insert(
                        path.clone(),
                        StagedEntry::File(StagedFile {
                            content_addr: empty,
                            mode: 0o100644,
                        }),
                    );
                    id
                }
            }
        };
        // O_TRUNC: empty the shared inode on open for write-only mode.
        if matches!(mode, OpenMode::Write) {
            let empty = self.store_content(ns, b"")?;
            self.apply_inode_content(inode_id, empty, 0)?;
        }
        let handle = self.alloc_handle(OpenHandle {
            inode: inode_id,
            offset: 0,
            mode,
        });
        Ok(handle)
    }

    /// Read up to `len` bytes from the handle's current offset, advancing it by the bytes read (POSIX
    /// `read`). Returns fewer bytes (or empty) at end of file.
    pub fn file_read(&mut self, handle: u64, len: u64) -> Result<Vec<u8>> {
        let h = self.handle(handle)?;
        if !h.mode.can_read() {
            return Err(LoomError::invalid("handle opened write-only"));
        }
        self.authorize_handle(h, AclRight::Read)?;
        let addr = self.handle_inode(h.inode)?.content_addr;
        let data = self.content_read_range(addr, h.offset, len)?;
        if let Some(hr) = self.handles.get_mut(&handle) {
            hr.offset += data.len() as u64;
        }
        Ok(data)
    }

    /// Read up to `len` bytes at an explicit `offset` without moving the handle's cursor (POSIX
    /// `pread`).
    pub fn file_read_at(&self, handle: u64, offset: u64, len: u64) -> Result<Vec<u8>> {
        let h = self.handle(handle)?;
        if !h.mode.can_read() {
            return Err(LoomError::invalid("handle opened write-only"));
        }
        self.authorize_handle(h, AclRight::Read)?;
        let addr = self.handle_inode(h.inode)?.content_addr;
        self.content_read_range(addr, offset, len)
    }

    /// Write `data` at the handle's current offset (or, for an `Append` handle, at the current end of
    /// file), advancing the cursor past the written bytes; returns the byte count (POSIX `write`).
    pub fn file_write(&mut self, handle: u64, data: &[u8]) -> Result<u64> {
        let h = self.handle(handle)?;
        if !h.mode.can_write() {
            return Err(LoomError::invalid("handle opened read-only"));
        }
        self.authorize_handle(h, AclRight::Write)?;
        let ino = self.handle_inode(h.inode)?;
        let (addr, old_size, ns) = (ino.content_addr, ino.size, ino.ns);
        let at = if matches!(h.mode, OpenMode::Append) {
            old_size
        } else {
            h.offset
        };
        let (new_addr, new_size) = self
            .build_edited_content(ns, EditPlan::write(Some(addr), old_size, at, data.to_vec()))?;
        self.apply_inode_content(h.inode, new_addr, new_size)?;
        if let Some(hr) = self.handles.get_mut(&handle) {
            hr.offset = at + data.len() as u64;
        }
        Ok(data.len() as u64)
    }

    /// Write `data` at an explicit `offset` without moving the handle's cursor (POSIX `pwrite`),
    /// zero-filling any gap past the old end. Returns the byte count.
    pub fn file_write_at(&mut self, handle: u64, offset: u64, data: &[u8]) -> Result<u64> {
        let h = self.handle(handle)?;
        if !h.mode.can_write() {
            return Err(LoomError::invalid("handle opened read-only"));
        }
        self.authorize_handle(h, AclRight::Write)?;
        let ino = self.handle_inode(h.inode)?;
        let (addr, old_size, ns) = (ino.content_addr, ino.size, ino.ns);
        let (new_addr, new_size) = self.build_edited_content(
            ns,
            EditPlan::write(Some(addr), old_size, offset, data.to_vec()),
        )?;
        self.apply_inode_content(h.inode, new_addr, new_size)?;
        Ok(data.len() as u64)
    }

    /// Resize the handle's file to `size`, zero-extending or dropping bytes (POSIX `ftruncate`).
    pub fn file_truncate(&mut self, handle: u64, size: u64) -> Result<()> {
        let h = self.handle(handle)?;
        if !h.mode.can_write() {
            return Err(LoomError::invalid("handle opened read-only"));
        }
        self.authorize_handle(h, AclRight::Write)?;
        let ino = self.handle_inode(h.inode)?;
        let (addr, old_size, ns) = (ino.content_addr, ino.size, ino.ns);
        let (new_addr, new_size) =
            self.build_edited_content(ns, EditPlan::truncate(Some(addr), old_size, size))?;
        self.apply_inode_content(h.inode, new_addr, new_size)
    }

    /// Flush the handle. Writes already apply to the inode per operation, and durability is the caller's
    /// `save_state`, so this only validates the handle and returns; it exists for API completeness.
    pub fn file_flush(&self, handle: u64) -> Result<()> {
        self.handle(handle)?;
        Ok(())
    }

    /// The handle's live size and mode (POSIX `fstat`).
    pub fn file_stat(&self, handle: u64) -> Result<FileStat> {
        let h = self.handle(handle)?;
        if h.mode.can_read() {
            self.authorize_handle(h, AclRight::Read)?;
        } else {
            self.authorize_handle(h, AclRight::Write)?;
        }
        let ino = self.handle_inode(h.inode)?;
        Ok(FileStat {
            size: ino.size,
            mode: ino.mode,
        })
    }

    /// Close the handle, releasing it. When the last handle on an inode closes, the inode is dropped; if
    /// it was already unlinked, its bytes become unreferenced and are reclaimed by GC (delete-on-last-
    /// close). Closing an unknown handle is `NOT_FOUND`.
    pub fn file_close(&mut self, handle: u64) -> Result<()> {
        let h = self
            .handles
            .remove(&handle)
            .ok_or_else(|| LoomError::not_found("handle"))?;
        let drop_inode = match self.inodes.get_mut(&h.inode) {
            Some(ino) => {
                ino.open_count = ino.open_count.saturating_sub(1);
                ino.open_count == 0
            }
            None => false,
        };
        if drop_inode
            && let Some(ino) = self.inodes.remove(&h.inode)
            && let Some(p) = ino.path
        {
            self.path_to_inode.remove(&(ino.ns, p));
        }
        Ok(())
    }

    fn authorize_handle(&self, handle: OpenHandle, right: AclRight) -> Result<()> {
        let inode = self.handle_inode(handle.inode)?;
        match inode.path.as_deref() {
            Some(path) => self.authorize_path(inode.ns, path, right),
            None => self.authorize(inode.ns, FacetKind::Files, right),
        }
    }

    /// Copy of the open handle, or `NOT_FOUND`.
    fn handle(&self, handle: u64) -> Result<OpenHandle> {
        self.handles
            .get(&handle)
            .copied()
            .ok_or_else(|| LoomError::not_found("handle"))
    }

    /// Reference to the inode `inode_id` refers to, or `NOT_FOUND`.
    fn handle_inode(&self, inode_id: u64) -> Result<&Inode> {
        self.inodes
            .get(&inode_id)
            .ok_or_else(|| LoomError::not_found("inode"))
    }

    /// Allocate the next inode id and insert `inode`.
    fn alloc_inode(&mut self, inode: Inode) -> u64 {
        let id = self.next_inode;
        self.next_inode += 1;
        self.inodes.insert(id, inode);
        id
    }

    /// Allocate the next handle id and insert `handle`.
    fn alloc_handle(&mut self, handle: OpenHandle) -> u64 {
        let id = self.next_handle;
        self.next_handle += 1;
        self.handles.insert(id, handle);
        id
    }

    // ---- append-log streams (structured storage) ------------------------------------------------
}
