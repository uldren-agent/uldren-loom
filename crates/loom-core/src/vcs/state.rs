use super::*;

const STATE_SECTION_NAMES: [&str; 10] = [
    "00-registry",
    "01-content",
    "02-work",
    "03-dirs",
    "04-compression",
    "05-consumer-offsets",
    "06-merge-state",
    "07-index",
    "08-open-files",
    "09-protected-refs",
];

const CONTENT_SECTION_NAME: &str = "01-content";
const WORK_SECTION_NAME: &str = "02-work";
const INDEX_SECTION_NAME: &str = "07-index";

impl<S: ObjectStore> Loom<S> {
    /// Serialize the engine's **recoverable** state to canonical bytes: the workspace
    /// registry (branches, tags, HEAD), the content-address -> Blob-object map, and the
    /// per-workspace **working trees + directories** (the durable staging index, so uncommitted edits
    /// survive a restart). [`Loom::import_state`] restores all of it verbatim.
    pub fn export_state(&self) -> Vec<u8> {
        let reg = self.registry.encode();
        let mut out = Vec::new();
        put_uvarint(&mut out, reg.len() as u64);
        out.extend_from_slice(&reg);
        // Content-address -> object map.
        put_uvarint(&mut out, self.content.len() as u64);
        for (content_addr, object_addr) in &self.content {
            out.extend_from_slice(content_addr.bytes());
            out.extend_from_slice(object_addr.bytes());
        }
        // Working trees (the durable staging index): each workspace's staged, possibly-uncommitted
        // files, so a `write` in one process survives until a `commit` in a later process.
        put_uvarint(&mut out, self.work.len() as u64);
        for (ns, wt) in &self.work {
            out.extend_from_slice(ns.as_bytes());
            put_uvarint(&mut out, wt.len() as u64);
            for (path, staged) in wt {
                put_lp(&mut out, path.as_bytes());
                match staged {
                    // tag 0 = file: content address + mode.
                    StagedEntry::File(f) => {
                        out.push(0);
                        out.extend_from_slice(f.content_addr.bytes());
                        put_uvarint(&mut out, u64::from(f.mode));
                    }
                    // tag 1 = table: the TABLE-entry Tree digest.
                    StagedEntry::Table(tree) => {
                        out.push(1);
                        out.extend_from_slice(tree.bytes());
                    }
                    // tag 2 = stream: the stream-root Tree digest.
                    StagedEntry::Stream(tree) => {
                        out.push(2);
                        out.extend_from_slice(tree.bytes());
                    }
                    StagedEntry::TimeSeries(tree) => {
                        out.push(3);
                        out.extend_from_slice(tree.bytes());
                    }
                    StagedEntry::Graph(tree) => {
                        out.push(4);
                        out.extend_from_slice(tree.bytes());
                    }
                    StagedEntry::Ledger(tree) => {
                        out.push(5);
                        out.extend_from_slice(tree.bytes());
                    }
                    StagedEntry::Columnar(tree) => {
                        out.push(6);
                        out.extend_from_slice(tree.bytes());
                    }
                    StagedEntry::Document(tree) => {
                        out.push(7);
                        out.extend_from_slice(tree.bytes());
                    }
                }
            }
        }
        // Explicit directories per workspace.
        put_uvarint(&mut out, self.dirs.len() as u64);
        for (ns, set) in &self.dirs {
            out.extend_from_slice(ns.as_bytes());
            put_uvarint(&mut out, set.len() as u64);
            for dir in set {
                put_lp(&mut out, dir.as_bytes());
            }
        }
        // Per-workspace compression-hint overrides.
        put_uvarint(&mut out, self.compression.len() as u64);
        for (ns, hint) in &self.compression {
            out.extend_from_slice(ns.as_bytes());
            out.push(hint_to_u8(*hint));
        }
        // Queue consumer offsets: operational metadata persisted with the local engine state only.
        put_uvarint(&mut out, self.consumer_offsets.len() as u64);
        for ((ns, stream, consumer), next_seq) in &self.consumer_offsets {
            out.extend_from_slice(ns.as_bytes());
            put_lp(&mut out, stream.as_bytes());
            put_lp(&mut out, consumer.as_bytes());
            put_uvarint(&mut out, *next_seq);
        }
        // In-progress merge state: operational metadata persisted with the local engine state only.
        put_uvarint(&mut out, self.merge_state.len() as u64);
        for (ns, m) in &self.merge_state {
            out.extend_from_slice(ns.as_bytes());
            out.extend_from_slice(m.other_parent.bytes());
            out.extend_from_slice(m.our_head.bytes());
            put_lp(&mut out, m.message.as_bytes());
            put_uvarint(&mut out, m.conflicts.len() as u64);
            for cf in &m.conflicts {
                put_lp(&mut out, cf.path.as_bytes());
                put_opt_slot(&mut out, &cf.base);
                put_opt_slot(&mut out, &cf.ours);
                put_opt_slot(&mut out, &cf.theirs);
            }
            put_uvarint(&mut out, m.pre_work.len() as u64);
            for (path, slot) in &m.pre_work {
                put_lp(&mut out, path.as_bytes());
                put_slot(&mut out, slot);
            }
            put_uvarint(&mut out, m.pre_dirs.len() as u64);
            for dir in &m.pre_dirs {
                put_lp(&mut out, dir.as_bytes());
            }
        }
        // Staging index: one shared stage per workspace, persisted with the local engine state.
        put_uvarint(&mut out, self.index.len() as u64);
        for (ns, idx) in &self.index {
            out.extend_from_slice(ns.as_bytes());
            put_uvarint(&mut out, idx.len() as u64);
            for (path, slot) in idx {
                put_lp(&mut out, path.as_bytes());
                put_slot(&mut out, slot);
            }
        }
        // Open-file table (inodes + handles) for live file handles: operational metadata persisted with
        // the local engine state only. `path_to_inode` is derived from the inodes and rebuilt on import.
        put_uvarint(&mut out, self.next_inode);
        put_uvarint(&mut out, self.next_handle);
        put_uvarint(&mut out, self.inodes.len() as u64);
        for (id, ino) in &self.inodes {
            put_uvarint(&mut out, *id);
            out.extend_from_slice(ino.ns.as_bytes());
            match &ino.path {
                None => out.push(0),
                Some(p) => {
                    out.push(1);
                    put_lp(&mut out, p.as_bytes());
                }
            }
            out.extend_from_slice(ino.content_addr.bytes());
            put_uvarint(&mut out, ino.size);
            put_uvarint(&mut out, u64::from(ino.mode));
            put_uvarint(&mut out, u64::from(ino.open_count));
        }
        put_uvarint(&mut out, self.handles.len() as u64);
        for (id, h) in &self.handles {
            put_uvarint(&mut out, *id);
            put_uvarint(&mut out, h.inode);
            put_uvarint(&mut out, h.offset);
            out.push(h.mode.to_u8());
        }
        put_uvarint(&mut out, self.protected_refs.len() as u64);
        for ((ns, ref_name), policy) in &self.protected_refs {
            out.extend_from_slice(ns.as_bytes());
            put_lp(&mut out, ref_name.as_bytes());
            out.push(u8::from(policy.fast_forward_only));
            out.push(u8::from(policy.signed_commits_required));
            out.push(u8::from(policy.signed_ref_advance_required));
            put_uvarint(&mut out, u64::from(policy.required_review_count));
            out.push(u8::from(policy.retention_lock));
            out.push(u8::from(policy.governance_lock));
        }
        // KV map tier config lives in a committed reserved file, so it versions and syncs with the
        // workspace.
        out
    }

    /// Restore engine state from [`Loom::export_state`] bytes: replace the registry, content map, and
    /// the **persisted working trees + directories** (the staging index). Unlike a checkout-on-load,
    /// this preserves uncommitted staged changes. Objects the working trees reference must be present
    /// (they are kept live by [`Loom::live_object_set`]).
    pub fn import_state(&mut self, bytes: &[u8]) -> Result<()> {
        // Persisted digests carry only their 32 bytes; tag them with the store's identity profile
        // so a FIPS (sha256) store reconstructs sha256-tagged addresses, which
        // matters because the store verifies a `get` under the digest's own algorithm.
        let algo = self.store.digest_algo();
        let mut c = StateCur { buf: bytes, pos: 0 };
        let reg_len = c.uvarint()? as usize;
        let reg_bytes = c.take(reg_len)?;
        let registry = Registry::decode(reg_bytes)?;
        let n = c.uvarint()?;
        let mut content = BTreeMap::new();
        for _ in 0..n {
            let content_addr = Digest::of(algo, c.take32()?);
            let object_addr = Digest::of(algo, c.take32()?);
            content.insert(content_addr, object_addr);
        }
        self.import_state_after_content(registry, content, c)
    }

    fn import_state_after_content(
        &mut self,
        registry: Registry,
        content: BTreeMap<Digest, Digest>,
        mut c: StateCur<'_>,
    ) -> Result<()> {
        let algo = self.store.digest_algo();
        let nw = c.uvarint()?;
        let mut work: BTreeMap<WorkspaceId, WorkTree> = BTreeMap::new();
        for _ in 0..nw {
            let ns = WorkspaceId::from_bytes(c.take16()?);
            let nf = c.uvarint()?;
            let mut wt = WorkTree::new();
            for _ in 0..nf {
                let path = c.lp_str()?;
                let entry = match c.u8()? {
                    0 => {
                        let content_addr = Digest::of(algo, c.take32()?);
                        let mode = c.uvarint()? as u32;
                        StagedEntry::File(StagedFile { content_addr, mode })
                    }
                    1 => StagedEntry::Table(Digest::of(algo, c.take32()?)),
                    2 => StagedEntry::Stream(Digest::of(algo, c.take32()?)),
                    3 => StagedEntry::TimeSeries(Digest::of(algo, c.take32()?)),
                    4 => StagedEntry::Graph(Digest::of(algo, c.take32()?)),
                    5 => StagedEntry::Ledger(Digest::of(algo, c.take32()?)),
                    6 => StagedEntry::Columnar(Digest::of(algo, c.take32()?)),
                    7 => StagedEntry::Document(Digest::of(algo, c.take32()?)),
                    other => {
                        return Err(LoomError::corrupt(format!(
                            "unknown staged-slot tag {other:#x}"
                        )));
                    }
                };
                wt.insert(path, entry);
            }
            work.insert(ns, wt);
        }
        let nd = c.uvarint()?;
        let mut dirs: BTreeMap<WorkspaceId, BTreeSet<String>> = BTreeMap::new();
        for _ in 0..nd {
            let ns = WorkspaceId::from_bytes(c.take16()?);
            let ndir = c.uvarint()?;
            let mut set = BTreeSet::new();
            for _ in 0..ndir {
                set.insert(c.lp_str()?);
            }
            dirs.insert(ns, set);
        }
        let ncomp = c.uvarint()?;
        let mut compression: BTreeMap<WorkspaceId, CompressionHint> = BTreeMap::new();
        for _ in 0..ncomp {
            let ns = WorkspaceId::from_bytes(c.take16()?);
            compression.insert(ns, hint_from_u8(c.u8()?));
        }
        // Queue consumer offsets: an optional trailing section, absent in encodings that omit it.
        let mut consumer_offsets: BTreeMap<(WorkspaceId, String, String), u64> = BTreeMap::new();
        if c.pos < c.buf.len() {
            let noff = c.uvarint()?;
            for _ in 0..noff {
                let ns = WorkspaceId::from_bytes(c.take16()?);
                let stream = c.lp_str()?;
                let consumer = c.lp_str()?;
                let next_seq = c.uvarint()?;
                consumer_offsets.insert((ns, stream, consumer), next_seq);
            }
        }
        // In-progress merge state: an optional trailing section, absent in encodings that omit it.
        let mut merge_state: BTreeMap<WorkspaceId, MergeInProgress> = BTreeMap::new();
        if c.pos < c.buf.len() {
            let nms = c.uvarint()?;
            for _ in 0..nms {
                let ns = WorkspaceId::from_bytes(c.take16()?);
                let other_parent = Digest::of(algo, c.take32()?);
                let our_head = Digest::of(algo, c.take32()?);
                let message = c.lp_str()?;
                let ncf = c.uvarint()?;
                let mut conflicts = Vec::with_capacity(ncf as usize);
                for _ in 0..ncf {
                    let path = c.lp_str()?;
                    let base = c.opt_slot(algo)?;
                    let ours = c.opt_slot(algo)?;
                    let theirs = c.opt_slot(algo)?;
                    conflicts.push(MergeConflict {
                        path,
                        base,
                        ours,
                        theirs,
                    });
                }
                let npw = c.uvarint()?;
                let mut pre_work = WorkTree::new();
                for _ in 0..npw {
                    let path = c.lp_str()?;
                    let slot = c.slot(algo)?;
                    pre_work.insert(path, slot);
                }
                let npd = c.uvarint()?;
                let mut pre_dirs = BTreeSet::new();
                for _ in 0..npd {
                    pre_dirs.insert(c.lp_str()?);
                }
                merge_state.insert(
                    ns,
                    MergeInProgress {
                        other_parent,
                        our_head,
                        message,
                        conflicts,
                        pre_work,
                        pre_dirs,
                    },
                );
            }
        }
        // Staging index: an optional trailing section. Older encodings predate it; for those the
        // working tree is the staged baseline (matching the prior implicit-staging behavior), so the
        // index defaults to a clone of the working tree.
        let index_present = c.pos < c.buf.len();
        let mut index: BTreeMap<WorkspaceId, WorkTree> = BTreeMap::new();
        if index_present {
            let ni = c.uvarint()?;
            for _ in 0..ni {
                let ns = WorkspaceId::from_bytes(c.take16()?);
                let nf = c.uvarint()?;
                let mut idx = WorkTree::new();
                for _ in 0..nf {
                    let path = c.lp_str()?;
                    idx.insert(path, c.slot(algo)?);
                }
                index.insert(ns, idx);
            }
        }
        // Open-file table: an optional trailing section. Older encodings predate it; for those there are
        // no live handles (allocators start at 1, matching a fresh engine).
        let mut next_inode = 1u64;
        let mut next_handle = 1u64;
        let mut inodes: BTreeMap<u64, Inode> = BTreeMap::new();
        let mut handles: BTreeMap<u64, OpenHandle> = BTreeMap::new();
        if c.pos < c.buf.len() {
            next_inode = c.uvarint()?;
            next_handle = c.uvarint()?;
            let nin = c.uvarint()?;
            for _ in 0..nin {
                let id = c.uvarint()?;
                let ns = WorkspaceId::from_bytes(c.take16()?);
                let path = if c.u8()? == 0 {
                    None
                } else {
                    Some(c.lp_str()?)
                };
                let content_addr = Digest::of(algo, c.take32()?);
                let size = c.uvarint()?;
                let mode = c.uvarint()? as u32;
                let open_count = c.uvarint()? as u32;
                inodes.insert(
                    id,
                    Inode {
                        ns,
                        path,
                        content_addr,
                        size,
                        mode,
                        open_count,
                    },
                );
            }
            let nh = c.uvarint()?;
            for _ in 0..nh {
                let id = c.uvarint()?;
                let inode = c.uvarint()?;
                let offset = c.uvarint()?;
                let mode = OpenMode::from_u8(c.u8()?)?;
                handles.insert(
                    id,
                    OpenHandle {
                        inode,
                        offset,
                        mode,
                    },
                );
            }
        }
        let mut protected_refs: BTreeMap<(WorkspaceId, String), ProtectedRefPolicy> =
            BTreeMap::new();
        if c.pos < c.buf.len() {
            let npr = c.uvarint()?;
            for _ in 0..npr {
                let ns = WorkspaceId::from_bytes(c.take16()?);
                let ref_name = c.lp_str()?;
                let fast_forward_only = c.bool()?;
                let signed_commits_required = c.bool()?;
                let signed_ref_advance_required = c.bool()?;
                let required_review_count = c.uvarint()? as u32;
                let retention_lock = c.bool()?;
                let governance_lock = c.bool()?;
                protected_refs.insert(
                    (ns, ref_name),
                    ProtectedRefPolicy {
                        fast_forward_only,
                        signed_commits_required,
                        signed_ref_advance_required,
                        required_review_count,
                        retention_lock,
                        governance_lock,
                    },
                );
            }
        }
        // KV map tier config lives in a committed reserved file. State blobs may carry a trailing
        // config section, which is ignored here.
        // Rebuild the linked-path -> inode reverse map from the inodes themselves.
        let mut path_to_inode: BTreeMap<(WorkspaceId, String), u64> = BTreeMap::new();
        for (id, ino) in &inodes {
            if let Some(p) = &ino.path {
                path_to_inode.insert((ino.ns, p.clone()), *id);
            }
        }
        self.registry = registry;
        self.content = content;
        self.work = work;
        self.dirs = dirs;
        self.compression = compression;
        self.consumer_offsets = consumer_offsets;
        self.stream_low_water_marks = BTreeMap::new();
        self.merge_state = merge_state;
        self.index = if index_present {
            index
        } else {
            self.work.clone()
        };
        self.inodes = inodes;
        self.handles = handles;
        self.path_to_inode = path_to_inode;
        self.next_inode = next_inode;
        self.next_handle = next_handle;
        self.protected_refs = protected_refs;
        self.lazy_state_sections = None;
        self.ephemeral_kv.clear();
        Ok(())
    }

    /// Encode the engine state into independently rooted section objects under one Tree root,
    /// returning the **engine-state root** a persistence backend records as its mutable root.
    /// [`Loom::load_state`] reverses it.
    pub fn save_state(&mut self) -> Result<Digest> {
        self.ensure_full_state_loaded()?;
        let bytes = self.export_state();
        let mut entries = Vec::new();
        for (name, section) in split_state_sections(&bytes)? {
            let target = if name == CONTENT_SECTION_NAME {
                self.put_object(&content_section_tree(&section, self.store.digest_algo())?)?
            } else if name == WORK_SECTION_NAME || name == INDEX_SECTION_NAME {
                let object = self.staged_map_section_tree(&section)?;
                self.put_object(&object)?
            } else {
                self.put_object(&Object::Blob(section))?
            };
            entries.push(TreeEntry {
                name: name.to_string(),
                kind: EntryKind::Tree,
                target,
                mode: 0o100644,
            });
        }
        self.put_object(&Object::tree(entries)?)
    }

    /// Load engine state from a section Tree root.
    pub fn load_state(&mut self, root: Digest) -> Result<()> {
        match self.get_object(&root)? {
            Object::Tree(entries) => self.load_state_sections(entries),
            other => Err(LoomError::corrupt(format!(
                "engine-state root is not a section Tree: {:?}",
                other.object_type()
            ))),
        }
    }

    /// Load the workspace registry and retain section handles for explicit materialization.
    pub fn load_state_lazy(&mut self, root: Digest) -> Result<()> {
        let Object::Tree(entries) = self.get_object(&root)? else {
            return Err(LoomError::corrupt(
                "engine-state root is not a section Tree",
            ));
        };
        validate_state_section_entries(&entries)?;
        let Object::Blob(bytes) = self.get_object(&entries[0].target)? else {
            return Err(LoomError::corrupt(
                "engine-state registry section has unsupported root",
            ));
        };
        self.registry = parse_registry_section(&bytes)?;
        self.lazy_state_sections = Some(entries);
        Ok(())
    }

    /// Load only the workspace registry from an engine-state section Tree root.
    pub fn load_state_registry(&mut self, root: Digest) -> Result<()> {
        self.load_state_lazy(root)
    }

    pub fn is_state_lazy(&self) -> bool {
        self.lazy_state_sections.is_some()
    }

    pub fn ensure_full_state_available(&self) -> Result<()> {
        if self.lazy_state_sections.is_some() {
            return Err(LoomError::new(
                Code::InvalidArgument,
                "engine state section is not materialized for this operation",
            ));
        }
        Ok(())
    }

    pub fn ensure_full_state_loaded(&mut self) -> Result<()> {
        let Some(entries) = self.lazy_state_sections.take() else {
            return Ok(());
        };
        self.load_state_sections(entries)
    }

    fn load_state_sections(&mut self, entries: Vec<TreeEntry>) -> Result<()> {
        validate_state_section_entries(&entries)?;
        let mut registry = None;
        let mut content = None;
        let mut tail = Vec::new();
        for (entry, expected) in entries.into_iter().zip(STATE_SECTION_NAMES) {
            if entry.name != expected {
                return Err(LoomError::corrupt("engine-state section name"));
            }
            match self.get_object(&entry.target)? {
                Object::Blob(bytes) if entry.name == STATE_SECTION_NAMES[0] => {
                    registry = Some(parse_registry_section(&bytes)?);
                }
                Object::Tree(entries)
                    if entry.name == WORK_SECTION_NAME || entry.name == INDEX_SECTION_NAME =>
                {
                    tail.extend_from_slice(&self.staged_map_section_bytes(entries)?);
                }
                Object::Blob(bytes)
                    if entry.name != CONTENT_SECTION_NAME
                        && entry.name != WORK_SECTION_NAME
                        && entry.name != INDEX_SECTION_NAME =>
                {
                    tail.extend_from_slice(&bytes);
                }
                Object::Tree(entries) if entry.name == CONTENT_SECTION_NAME => {
                    content = Some(content_section_map(entries)?);
                }
                other => {
                    return Err(LoomError::corrupt(format!(
                        "engine-state section has unsupported root: {:?}",
                        other.object_type()
                    )));
                }
            }
        }
        let registry =
            registry.ok_or_else(|| LoomError::corrupt("engine-state registry section missing"))?;
        let content =
            content.ok_or_else(|| LoomError::corrupt("engine-state content section missing"))?;
        self.import_state_after_content(registry, content, StateCur { buf: &tail, pos: 0 })
    }

    // ---- working tree ---------------------------------------------------------------------------

    fn staged_map_section_tree(&mut self, bytes: &[u8]) -> Result<Object> {
        let algo = self.store.digest_algo();
        let mut c = StateCur { buf: bytes, pos: 0 };
        let n = c.uvarint()?;
        let mut workspace_entries = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let ns = WorkspaceId::from_bytes(c.take16()?);
            let nf = c.uvarint()?;
            let mut path_entries = Vec::with_capacity(nf as usize);
            for _ in 0..nf {
                let path = c.lp_str()?;
                let staged = c.slot(algo)?;
                path_entries.push(TreeEntry {
                    name: hex::encode(path.as_bytes()),
                    ..tree_entry_from_staged_slot(staged)
                });
            }
            let target = self.put_object(&Object::tree(path_entries)?)?;
            workspace_entries.push(TreeEntry {
                name: ns.to_string(),
                kind: EntryKind::Tree,
                target,
                mode: 0o100644,
            });
        }
        if c.pos != bytes.len() {
            return Err(LoomError::corrupt("staged map section trailing bytes"));
        }
        Object::tree(workspace_entries)
    }

    fn staged_map_section_bytes(&self, entries: Vec<TreeEntry>) -> Result<Vec<u8>> {
        let algo = self.store.digest_algo();
        let mut out = Vec::new();
        put_uvarint(&mut out, entries.len() as u64);
        for entry in entries {
            if entry.kind != EntryKind::Tree {
                return Err(LoomError::corrupt("staged map workspace entry kind"));
            }
            let ns = WorkspaceId::parse(&entry.name)?;
            let Object::Tree(path_entries) = self.get_object(&entry.target)? else {
                return Err(LoomError::corrupt("staged map workspace target"));
            };
            out.extend_from_slice(ns.as_bytes());
            put_uvarint(&mut out, path_entries.len() as u64);
            for path_entry in path_entries {
                let path = hex::decode(&path_entry.name)
                    .map_err(|_| LoomError::corrupt("staged map path name is not hex"))?;
                put_lp(&mut out, &path);
                put_slot(&mut out, &staged_slot_from_tree_entry(&path_entry, algo)?);
            }
        }
        Ok(out)
    }
}

fn tree_entry_from_staged_slot(staged: StagedEntry) -> TreeEntry {
    match staged {
        StagedEntry::File(file) => TreeEntry {
            name: String::new(),
            kind: EntryKind::Blob,
            target: file.content_addr,
            mode: file.mode,
        },
        StagedEntry::Table(root) => TreeEntry {
            name: String::new(),
            kind: EntryKind::Table,
            target: root,
            mode: 0o100644,
        },
        StagedEntry::Stream(root) => TreeEntry {
            name: String::new(),
            kind: EntryKind::Stream,
            target: root,
            mode: 0o100644,
        },
        StagedEntry::TimeSeries(root) => TreeEntry {
            name: String::new(),
            kind: EntryKind::TimeSeries,
            target: root,
            mode: 0o100644,
        },
        StagedEntry::Graph(root) => TreeEntry {
            name: String::new(),
            kind: EntryKind::Graph,
            target: root,
            mode: 0o100644,
        },
        StagedEntry::Ledger(root) => TreeEntry {
            name: String::new(),
            kind: EntryKind::Ledger,
            target: root,
            mode: 0o100644,
        },
        StagedEntry::Columnar(root) => TreeEntry {
            name: String::new(),
            kind: EntryKind::Columnar,
            target: root,
            mode: 0o100644,
        },
        StagedEntry::Document(root) => TreeEntry {
            name: String::new(),
            kind: EntryKind::Document,
            target: root,
            mode: 0o100644,
        },
    }
}

fn validate_state_section_entries(entries: &[TreeEntry]) -> Result<()> {
    if entries.len() != STATE_SECTION_NAMES.len() {
        return Err(LoomError::corrupt("engine-state section count"));
    }
    for (entry, expected) in entries.iter().zip(STATE_SECTION_NAMES) {
        if entry.name != expected {
            return Err(LoomError::corrupt("engine-state section name"));
        }
    }
    Ok(())
}

fn staged_slot_from_tree_entry(
    entry: &TreeEntry,
    algo: crate::digest::Algo,
) -> Result<StagedEntry> {
    let digest = Digest::of(algo, *entry.target.bytes());
    Ok(match entry.kind {
        EntryKind::Blob => StagedEntry::File(StagedFile {
            content_addr: digest,
            mode: entry.mode,
        }),
        EntryKind::Table => StagedEntry::Table(digest),
        EntryKind::Stream => StagedEntry::Stream(digest),
        EntryKind::TimeSeries => StagedEntry::TimeSeries(digest),
        EntryKind::Graph => StagedEntry::Graph(digest),
        EntryKind::Ledger => StagedEntry::Ledger(digest),
        EntryKind::Columnar => StagedEntry::Columnar(digest),
        EntryKind::Document => StagedEntry::Document(digest),
        EntryKind::Tree
        | EntryKind::Symlink
        | EntryKind::Subloom
        | EntryKind::TreeShard
        | EntryKind::ProllyMap => {
            return Err(LoomError::corrupt("staged map path entry kind"));
        }
    })
}

fn split_state_sections(bytes: &[u8]) -> Result<Vec<(&'static str, Vec<u8>)>> {
    let mut c = StateCur { buf: bytes, pos: 0 };
    let mut sections = Vec::with_capacity(STATE_SECTION_NAMES.len());

    let start = c.pos;
    let reg_len = c.uvarint()? as usize;
    c.take(reg_len)?;
    sections.push((STATE_SECTION_NAMES[0], bytes[start..c.pos].to_vec()));

    let start = c.pos;
    let n = c.uvarint()?;
    for _ in 0..n {
        c.take32()?;
        c.take32()?;
    }
    sections.push((STATE_SECTION_NAMES[1], bytes[start..c.pos].to_vec()));

    let start = c.pos;
    let nw = c.uvarint()?;
    for _ in 0..nw {
        c.take16()?;
        let nf = c.uvarint()?;
        for _ in 0..nf {
            c.lp_str()?;
            skip_staged_entry(&mut c)?;
        }
    }
    sections.push((STATE_SECTION_NAMES[2], bytes[start..c.pos].to_vec()));

    let start = c.pos;
    let nd = c.uvarint()?;
    for _ in 0..nd {
        c.take16()?;
        let ndir = c.uvarint()?;
        for _ in 0..ndir {
            c.lp_str()?;
        }
    }
    sections.push((STATE_SECTION_NAMES[3], bytes[start..c.pos].to_vec()));

    let start = c.pos;
    let ncomp = c.uvarint()?;
    for _ in 0..ncomp {
        c.take16()?;
        c.u8()?;
    }
    sections.push((STATE_SECTION_NAMES[4], bytes[start..c.pos].to_vec()));

    let start = c.pos;
    let noff = c.uvarint()?;
    for _ in 0..noff {
        c.take16()?;
        c.lp_str()?;
        c.lp_str()?;
        c.uvarint()?;
    }
    sections.push((STATE_SECTION_NAMES[5], bytes[start..c.pos].to_vec()));

    let start = c.pos;
    let nms = c.uvarint()?;
    for _ in 0..nms {
        c.take16()?;
        c.take32()?;
        c.take32()?;
        c.lp_str()?;
        let ncf = c.uvarint()?;
        for _ in 0..ncf {
            c.lp_str()?;
            skip_optional_staged_entry(&mut c)?;
            skip_optional_staged_entry(&mut c)?;
            skip_optional_staged_entry(&mut c)?;
        }
        let npw = c.uvarint()?;
        for _ in 0..npw {
            c.lp_str()?;
            skip_staged_entry(&mut c)?;
        }
        let npd = c.uvarint()?;
        for _ in 0..npd {
            c.lp_str()?;
        }
    }
    sections.push((STATE_SECTION_NAMES[6], bytes[start..c.pos].to_vec()));

    let start = c.pos;
    let ni = c.uvarint()?;
    for _ in 0..ni {
        c.take16()?;
        let nf = c.uvarint()?;
        for _ in 0..nf {
            c.lp_str()?;
            skip_staged_entry(&mut c)?;
        }
    }
    sections.push((STATE_SECTION_NAMES[7], bytes[start..c.pos].to_vec()));

    let start = c.pos;
    c.uvarint()?;
    c.uvarint()?;
    let nin = c.uvarint()?;
    for _ in 0..nin {
        c.uvarint()?;
        c.take16()?;
        if c.u8()? != 0 {
            c.lp_str()?;
        }
        c.take32()?;
        c.uvarint()?;
        c.uvarint()?;
        c.uvarint()?;
    }
    let nh = c.uvarint()?;
    for _ in 0..nh {
        c.uvarint()?;
        c.uvarint()?;
        c.uvarint()?;
        c.u8()?;
    }
    sections.push((STATE_SECTION_NAMES[8], bytes[start..c.pos].to_vec()));

    let start = c.pos;
    let npr = c.uvarint()?;
    for _ in 0..npr {
        c.take16()?;
        c.lp_str()?;
        c.bool()?;
        c.bool()?;
        c.bool()?;
        c.uvarint()?;
        c.bool()?;
        c.bool()?;
    }
    sections.push((STATE_SECTION_NAMES[9], bytes[start..c.pos].to_vec()));

    if c.pos != bytes.len() {
        return Err(LoomError::corrupt("engine-state trailing bytes"));
    }
    Ok(sections)
}

fn content_section_tree(bytes: &[u8], algo: crate::digest::Algo) -> Result<Object> {
    let mut c = StateCur { buf: bytes, pos: 0 };
    let n = c.uvarint()?;
    let mut entries = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let content_addr = Digest::of(algo, c.take32()?);
        let object_addr = Digest::of(algo, c.take32()?);
        entries.push(TreeEntry {
            name: content_addr.to_string(),
            kind: EntryKind::Tree,
            target: object_addr,
            mode: 0o100644,
        });
    }
    if c.pos != bytes.len() {
        return Err(LoomError::corrupt("content section trailing bytes"));
    }
    Object::tree(entries)
}

fn parse_registry_section(bytes: &[u8]) -> Result<Registry> {
    let mut c = StateCur { buf: bytes, pos: 0 };
    let reg_len = c.uvarint()? as usize;
    let reg_bytes = c.take(reg_len)?;
    if c.pos != bytes.len() {
        return Err(LoomError::corrupt("registry section trailing bytes"));
    }
    Registry::decode(reg_bytes)
}

fn content_section_map(entries: Vec<TreeEntry>) -> Result<BTreeMap<Digest, Digest>> {
    let mut content = BTreeMap::new();
    for entry in entries {
        if entry.kind != EntryKind::Tree {
            return Err(LoomError::corrupt("content section entry kind"));
        }
        let content_addr = Digest::parse(&entry.name)?;
        content.insert(content_addr, entry.target);
    }
    Ok(content)
}

fn skip_staged_entry(c: &mut StateCur<'_>) -> Result<()> {
    match c.u8()? {
        0 => {
            c.take32()?;
            c.uvarint()?;
        }
        1..=7 => {
            c.take32()?;
        }
        other => {
            return Err(LoomError::corrupt(format!(
                "unknown staged-slot tag {other:#x}"
            )));
        }
    }
    Ok(())
}

fn skip_optional_staged_entry(c: &mut StateCur<'_>) -> Result<()> {
    match c.u8()? {
        0 => Ok(()),
        1 => skip_staged_entry(c),
        _ => Err(LoomError::corrupt("engine-state optional slot tag")),
    }
}
