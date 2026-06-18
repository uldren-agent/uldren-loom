use super::*;

fn mark_step(state: &ReachabilityMarkState, visited: usize) -> ReachabilityMarkStep {
    ReachabilityMarkStep {
        visited,
        pending: state.queue.len() + state.stream_roots.len(),
        completed: state.completed,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReachabilityMarkState {
    pub pinned: BTreeSet<Digest>,
    pub marked: BTreeSet<Digest>,
    pub queue: VecDeque<Digest>,
    pub stream_roots: VecDeque<Digest>,
    pub completed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReachabilityMarkStep {
    pub visited: usize,
    pub pending: usize,
    pub completed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveRootDiagnostics {
    pub sample_limit: usize,
    pub classes: Vec<LiveRootClassDiagnostics>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveRootClassDiagnostics {
    pub class: &'static str,
    pub count: u64,
    pub examples: Vec<LiveRootExample>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveRootExample {
    pub id: String,
    pub digest: Digest,
}

struct LiveRootDiagnosticsBuilder {
    sample_limit: usize,
    classes: BTreeMap<&'static str, LiveRootClassDiagnostics>,
}

impl LiveRootDiagnosticsBuilder {
    fn new(sample_limit: usize) -> Self {
        Self {
            sample_limit,
            classes: BTreeMap::new(),
        }
    }

    fn push(&mut self, class: &'static str, id: String, digest: Digest) {
        let entry = self
            .classes
            .entry(class)
            .or_insert_with(|| LiveRootClassDiagnostics {
                class,
                count: 0,
                examples: Vec::new(),
                truncated: false,
            });
        entry.count = entry.count.saturating_add(1);
        if entry.examples.len() < self.sample_limit {
            entry.examples.push(LiveRootExample { id, digest });
        } else {
            entry.truncated = true;
        }
    }

    fn finish(self) -> LiveRootDiagnostics {
        LiveRootDiagnostics {
            sample_limit: self.sample_limit,
            classes: self.classes.into_values().collect(),
        }
    }
}

impl<S: ObjectStore> Loom<S> {
    /// The root Tree digest (the state root) of a commit.
    pub fn commit_tree(&self, commit: Digest) -> Result<Digest> {
        Ok(self.get_commit(commit)?.tree)
    }

    // ---- synchronization primitives ------------------------------------------------------

    /// Every object digest reachable from `tips`, pruning any subgraph the receiver already holds
    /// (`have`) - the "want" closure: a commit reaches its tree and parents; a tree reaches its
    /// entries (a `Tree`/`Subloom` entry's target is an object digest; a `Blob`/`Symlink` entry's
    /// target is a *content address* resolved through the content index to its Blob object); a tag
    /// reaches its target. Stopping at `have` is sound because a ref only advances once its whole
    /// subgraph is present, so a held object implies a held subgraph.
    pub fn reachable(&self, tips: &[Digest], have: &BTreeSet<Digest>) -> Result<BTreeSet<Digest>> {
        let mut out = BTreeSet::new();
        let mut queue: VecDeque<Digest> = tips.iter().copied().collect();
        while let Some(d) = queue.pop_front() {
            if have.contains(&d) || !out.insert(d) {
                continue;
            }
            match self.get_object(&d)? {
                Object::Blob(_) => {}
                Object::ChunkList { entries, .. } => {
                    for e in entries {
                        queue.push_back(e.target); // chunk object digests
                    }
                }
                Object::Tree(entries) => {
                    // Shard nodes are also Tree objects; a TreeShard entry enqueues the child shard
                    // Tree, so this walk traverses the whole prolly tree of a sharded directory. A
                    // Table entry enqueues its table Tree; a ProllyMap entry folds its prolly nodes
                    // straight into `out`.
                    for e in &entries {
                        self.enqueue_entry_target(e, &mut queue, &mut out, have)?;
                    }
                }
                Object::Commit(c) => {
                    queue.push_back(c.tree);
                    for p in c.parents {
                        queue.push_back(p);
                    }
                }
                Object::Tag(t) => queue.push_back(t.target),
            }
        }
        Ok(out)
    }

    /// Every object digest that must be retained by a garbage collector: the reachable closure from
    /// all branch tips and tag targets of every workspace, the current engine-state `root` if a
    /// persistence backend recorded one, persisted working-tree and staging entries, open inode
    /// content, stream roots, and facet roots. Objects reachable only from deleted refs or
    /// superseded engine-state roots are reclaimable when no current durable live root references
    /// them. Retention is represented by explicit live roots, not per-object lifecycle state.
    pub fn live_object_set(&self, root: Option<Digest>) -> Result<BTreeSet<Digest>> {
        let mut tips: Vec<Digest> = Vec::new();
        for info in self.registry.list(None) {
            for branch in self.registry.branch_list(info.id)? {
                if let Some(tip) = self.registry.branch_tip(info.id, &branch)? {
                    tips.push(tip);
                }
            }
            for tag in self.registry.tag_list(info.id)? {
                if let Some(target) = self.registry.tag_target(info.id, &tag)? {
                    tips.push(target);
                }
            }
        }
        // Keep the persisted working trees' content alive too: a staged-but-uncommitted file is only
        // reachable through the content map, not from any ref, but it survives a reload (it is in the
        // engine state), so GC must not drop it.
        let mut stream_roots: Vec<Digest> = Vec::new();
        for wt in self.work.values() {
            for staged in wt.values() {
                match staged {
                    StagedEntry::File(f) => {
                        if let Some(obj) = self.content.get(&f.content_addr) {
                            tips.push(*obj);
                        }
                    }
                    // A staged table's whole structure is reachable from its TABLE-entry Tree digest.
                    StagedEntry::Table(tree)
                    | StagedEntry::Graph(tree)
                    | StagedEntry::Ledger(tree)
                    | StagedEntry::Columnar(tree)
                    | StagedEntry::Document(tree) => tips.push(*tree),
                    StagedEntry::Stream(root) => stream_roots.push(*root),
                    StagedEntry::TimeSeries(root) => tips.push(*root),
                }
            }
        }
        // Keep open inodes' bytes alive: an unlinked-but-open file is reachable from no path or ref, but
        // its handles can still read and write it until the last close, and it survives a reload (it is
        // in the persisted open-file table), so GC must not drop its content objects.
        for ino in self.inodes.values() {
            if let Some(obj) = self.content.get(&ino.content_addr) {
                tips.push(*obj);
            }
        }
        let mut live = self.reachable(&tips, &BTreeSet::new())?;
        for root in stream_roots {
            for d in self.stream_reachable(root, &BTreeSet::new())? {
                live.insert(d);
            }
        }
        if let Some(root) = root {
            live.extend(self.reachable(&[root], &BTreeSet::new())?);
        }
        Ok(live)
    }

    pub fn live_root_diagnostics(
        &self,
        root: Option<Digest>,
        extra_roots: impl IntoIterator<Item = (&'static str, String, Digest)>,
        sample_limit: usize,
    ) -> Result<LiveRootDiagnostics> {
        let mut diagnostics = LiveRootDiagnosticsBuilder::new(sample_limit);
        for info in self.registry.list(None) {
            for branch in self.registry.branch_list(info.id)? {
                if let Some(tip) = self.registry.branch_tip(info.id, &branch)? {
                    diagnostics.push(
                        "current_branch_tips",
                        format!("{}/{}", info.name, branch),
                        tip,
                    );
                }
            }
            for tag in self.registry.tag_list(info.id)? {
                if let Some(target) = self.registry.tag_target(info.id, &tag)? {
                    diagnostics.push(
                        "current_tag_targets",
                        format!("{}/{}", info.name, tag),
                        target,
                    );
                }
            }
        }
        for (ns, wt) in &self.work {
            let workspace = self
                .registry
                .name(*ns)
                .unwrap_or_else(|_| format!("{ns:?}"));
            for (path, staged) in wt {
                self.record_staged_entry_root(
                    &mut diagnostics,
                    "persisted_working_tree_roots",
                    &workspace,
                    path,
                    staged,
                );
            }
        }
        for (ns, idx) in &self.index {
            let workspace = self
                .registry
                .name(*ns)
                .unwrap_or_else(|_| format!("{ns:?}"));
            for (path, staged) in idx {
                self.record_staged_entry_root(
                    &mut diagnostics,
                    "persisted_staging_roots",
                    &workspace,
                    path,
                    staged,
                );
            }
        }
        for (inode, ino) in &self.inodes {
            if let Some(obj) = self.content.get(&ino.content_addr) {
                diagnostics.push(
                    "open_handle_or_transaction_roots",
                    format!("inode:{inode}"),
                    *obj,
                );
            }
        }
        if let Some(root) = root {
            diagnostics.push(
                "current_reference_root",
                "store_reference_root".to_string(),
                root,
            );
        }
        for (class, id, digest) in extra_roots {
            diagnostics.push(class, id, digest);
        }
        Ok(diagnostics.finish())
    }

    fn record_staged_entry_root(
        &self,
        diagnostics: &mut LiveRootDiagnosticsBuilder,
        fallback_class: &'static str,
        workspace: &str,
        path: &str,
        staged: &StagedEntry,
    ) {
        match staged {
            StagedEntry::File(file) => {
                if let Some(obj) = self.content.get(&file.content_addr) {
                    diagnostics.push(fallback_class, format!("{workspace}:{path}"), *obj);
                }
            }
            StagedEntry::Stream(root) => {
                diagnostics.push("stream_roots", format!("{workspace}:{path}"), *root);
            }
            StagedEntry::Table(root)
            | StagedEntry::Graph(root)
            | StagedEntry::Ledger(root)
            | StagedEntry::Columnar(root)
            | StagedEntry::Document(root)
            | StagedEntry::TimeSeries(root) => {
                diagnostics.push(
                    "structured_facet_roots",
                    format!("{workspace}:{path}"),
                    *root,
                );
            }
        }
    }

    pub fn begin_live_object_mark(
        &self,
        pinned_roots: impl IntoIterator<Item = Digest>,
    ) -> Result<ReachabilityMarkState> {
        let mut queue = VecDeque::new();
        let mut stream_roots = VecDeque::new();
        for info in self.registry.list(None) {
            for branch in self.registry.branch_list(info.id)? {
                if let Some(tip) = self.registry.branch_tip(info.id, &branch)? {
                    queue.push_back(tip);
                }
            }
            for tag in self.registry.tag_list(info.id)? {
                if let Some(target) = self.registry.tag_target(info.id, &tag)? {
                    queue.push_back(target);
                }
            }
        }
        for wt in self.work.values() {
            for staged in wt.values() {
                match staged {
                    StagedEntry::File(f) => {
                        if let Some(obj) = self.content.get(&f.content_addr) {
                            queue.push_back(*obj);
                        }
                    }
                    StagedEntry::Table(tree)
                    | StagedEntry::Graph(tree)
                    | StagedEntry::Ledger(tree)
                    | StagedEntry::Columnar(tree)
                    | StagedEntry::Document(tree) => queue.push_back(*tree),
                    StagedEntry::Stream(root) => stream_roots.push_back(*root),
                    StagedEntry::TimeSeries(root) => queue.push_back(*root),
                }
            }
        }
        for ino in self.inodes.values() {
            if let Some(obj) = self.content.get(&ino.content_addr) {
                queue.push_back(*obj);
            }
        }
        let pinned = pinned_roots.into_iter().collect::<BTreeSet<_>>();
        for root in &pinned {
            queue.push_back(*root);
        }
        Ok(ReachabilityMarkState {
            pinned,
            marked: BTreeSet::new(),
            queue,
            stream_roots,
            completed: false,
        })
    }

    pub fn step_live_object_mark(
        &self,
        state: &mut ReachabilityMarkState,
        budget: usize,
    ) -> Result<ReachabilityMarkStep> {
        if state.completed || budget == 0 {
            return Ok(mark_step(state, 0));
        }
        let mut visited = 0usize;
        while visited < budget {
            if let Some(d) = state.queue.pop_front() {
                if !state.marked.insert(d) {
                    continue;
                }
                visited += 1;
                match self.get_object(&d)? {
                    Object::Blob(_) => {}
                    Object::ChunkList { entries, .. } => {
                        for e in entries {
                            state.queue.push_back(e.target);
                        }
                    }
                    Object::Tree(entries) => {
                        for e in &entries {
                            self.enqueue_mark_entry_target(e, state)?;
                        }
                    }
                    Object::Commit(c) => {
                        state.queue.push_back(c.tree);
                        for p in c.parents {
                            state.queue.push_back(p);
                        }
                    }
                    Object::Tag(t) => state.queue.push_back(t.target),
                }
                continue;
            }
            if let Some(root) = state.stream_roots.pop_front() {
                visited += 1;
                for d in self.stream_reachable(root, &BTreeSet::new())? {
                    state.marked.insert(d);
                }
                continue;
            }
            state.completed = true;
            break;
        }
        Ok(mark_step(state, visited))
    }

    /// Canonical bytes of a stored object (for transfer), or `NOT_FOUND`.
    pub fn object_bytes(&self, digest: Digest) -> Result<Vec<u8>> {
        self.store
            .get(&digest)?
            .ok_or_else(|| LoomError::not_found(format!("object {digest}")))
    }

    /// Whether the object is present locally.
    pub fn has_object(&self, digest: Digest) -> Result<bool> {
        self.store.has(&digest)
    }

    /// Ingest one transferred object's canonical bytes: store it (the store re-derives and verifies
    /// the address) and, for a Blob, rebuild the content-address -> object entry the
    /// receiver needs to resolve Tree entries. The content index is derived, never shipped, so it is
    /// rebuilt here from the bytes themselves. Returns the object's digest.
    pub fn ingest_object(&mut self, canonical: &[u8]) -> Result<Digest> {
        match Object::decode(canonical) {
            Ok(Object::Blob(payload)) => {
                let digest = self.store.put(canonical)?;
                let algo = self.store.digest_algo();
                self.content
                    .insert(content_address_with(algo, &payload), digest);
                // A newly present chunk Blob may complete a deferred ChunkList's whole-content address.
                self.resolve_pending_chunklists()?;
                Ok(digest)
            }
            Ok(Object::ChunkList { .. }) => {
                let digest = self.store.put(canonical)?;
                self.resolve_chunklist_content(digest)?;
                Ok(digest)
            }
            Ok(_) => self.store.put(canonical),
            // Not a framed Loom object. Admit a structurally valid raw prolly node (table row maps,
            // stream entry maps); reject anything else so a malformed framed object is never stored.
            Err(_) => {
                crate::prolly::validate_node_bytes(canonical)?;
                self.store.put(canonical)
            }
        }
    }

    /// Rebuild the whole-content address -> ChunkList object mapping for `chunklist` by reassembling its
    /// chunk Blobs. If some chunk has not been ingested yet, defer the rebuild; a chunk count, chunk
    /// length, or total-size mismatch is rejected rather than silently accepted.
    fn resolve_chunklist_content(&mut self, chunklist: Digest) -> Result<()> {
        let Object::ChunkList {
            total_size,
            entries,
        } = self.get_object(&chunklist)?
        else {
            return Err(LoomError::corrupt("expected a ChunkList"));
        };
        let mut content = Vec::with_capacity(total_size as usize);
        for e in &entries {
            let Some(bytes) = self.store.get(&e.target)? else {
                self.pending_chunklists.insert(chunklist);
                return Ok(());
            };
            match Object::decode(&bytes)? {
                Object::Blob(chunk) => {
                    if chunk.len() as u64 != e.size {
                        return Err(LoomError::corrupt("ChunkList chunk length does not match"));
                    }
                    content.extend_from_slice(&chunk);
                }
                other => {
                    return Err(LoomError::corrupt(format!(
                        "ChunkList chunk {} is a {:?}, not a Blob",
                        e.target,
                        other.object_type()
                    )));
                }
            }
        }
        if content.len() as u64 != total_size {
            return Err(LoomError::corrupt(
                "ChunkList total_size does not match its chunks",
            ));
        }
        let algo = self.store.digest_algo();
        self.content
            .insert(content_address_with(algo, &content), chunklist);
        self.pending_chunklists.remove(&chunklist);
        Ok(())
    }

    /// Retry every deferred ChunkList; ones still missing a chunk stay deferred.
    fn resolve_pending_chunklists(&mut self) -> Result<()> {
        for chunklist in std::mem::take(&mut self.pending_chunklists) {
            self.resolve_chunklist_content(chunklist)?;
        }
        Ok(())
    }

    // ---- internals ------------------------------------------------------------------------------

    pub(crate) fn is_ancestor(&self, a: Digest, b: Digest) -> Result<bool> {
        Ok(self.ancestors(b)?.contains(&a))
    }

    pub(crate) fn ancestors(&self, start: Digest) -> Result<BTreeSet<Digest>> {
        let mut seen = BTreeSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(start);
        while let Some(d) = queue.pop_front() {
            if !seen.insert(d) {
                continue;
            }
            for parent in self.get_commit(d)?.parents {
                queue.push_back(parent);
            }
        }
        Ok(seen)
    }

    /// Recursively build Tree objects for the files and directories under `prefix`, returning the
    /// subtree's digest. Explicit (possibly empty) directories from `dirs` become Tree entries -
    /// an empty directory is an empty Tree - so directories persist across commit/checkout/sync.
    pub(crate) fn build_subtree(
        &mut self,
        files: &WorkTree,
        dirs: &BTreeSet<String>,
        prefix: &str,
    ) -> Result<Digest> {
        let mut file_children: Vec<(String, StagedEntry)> = Vec::new();
        let mut dir_children: BTreeSet<String> = BTreeSet::new();
        for (path, staged) in files {
            let Some(rest) = path.strip_prefix(prefix) else {
                continue;
            };
            match rest.split_once('/') {
                None => file_children.push((rest.to_string(), *staged)),
                Some((seg, _)) => {
                    dir_children.insert(seg.to_string());
                }
            }
        }
        for dir in dirs {
            let Some(rest) = dir.strip_prefix(prefix) else {
                continue;
            };
            if rest.is_empty() {
                continue;
            }
            let seg = rest.split_once('/').map_or(rest, |(s, _)| s);
            dir_children.insert(seg.to_string());
        }

        let mut entries = Vec::new();
        for (name, staged) in file_children {
            let entry = match staged {
                StagedEntry::File(f) => TreeEntry {
                    name,
                    kind: EntryKind::Blob,
                    target: f.content_addr,
                    mode: f.mode,
                },
                StagedEntry::Table(tree) => TreeEntry {
                    name,
                    kind: EntryKind::Table,
                    target: tree,
                    mode: 0,
                },
                StagedEntry::Stream(root) => TreeEntry {
                    name,
                    kind: EntryKind::Stream,
                    target: root,
                    mode: 0,
                },
                StagedEntry::TimeSeries(root) => TreeEntry {
                    name,
                    kind: EntryKind::TimeSeries,
                    target: root,
                    mode: 0,
                },
                StagedEntry::Graph(root) => TreeEntry {
                    name,
                    kind: EntryKind::Graph,
                    target: root,
                    mode: 0,
                },
                StagedEntry::Ledger(root) => TreeEntry {
                    name,
                    kind: EntryKind::Ledger,
                    target: root,
                    mode: 0,
                },
                StagedEntry::Columnar(root) => TreeEntry {
                    name,
                    kind: EntryKind::Columnar,
                    target: root,
                    mode: 0,
                },
                StagedEntry::Document(root) => TreeEntry {
                    name,
                    kind: EntryKind::Document,
                    target: root,
                    mode: 0,
                },
            };
            entries.push(entry);
        }
        for seg in dir_children {
            let sub_prefix = format!("{prefix}{seg}/");
            let sub = self.build_subtree(files, dirs, &sub_prefix)?;
            entries.push(TreeEntry {
                name: seg,
                kind: EntryKind::Tree,
                target: sub,
                mode: 0o040000,
            });
        }
        // A large directory is stored as a prolly-sharded tree; a small one stays a single flat Tree.
        // The split point is a fixed constant, so the choice is deterministic and peers converge.
        if entries.len() > DIR_SHARD_THRESHOLD {
            self.put_sharded_tree(entries)
        } else {
            self.put_object(&Object::tree(entries)?)
        }
    }

    /// Store a directory's entries as a prolly-sharded tree and return the root shard `Tree` digest.
    /// Shard nodes are `Tree` objects: leaf nodes hold the ordinary entries; interior nodes hold
    /// [`EntryKind::TreeShard`] entries whose `name` is the max entry name in the child subtree and
    /// whose `target` is the child shard Tree. Node boundaries are a hash of the entry name and level,
    /// so the structure is a pure function of the entry set. Entries are canonicalized and
    /// dedup-checked via [`Object::tree`] first.
    fn put_sharded_tree(&mut self, entries: Vec<TreeEntry>) -> Result<Digest> {
        let Object::Tree(sorted) = Object::tree(entries)? else {
            unreachable!("Object::tree always returns a Tree")
        };

        // Level 0: chunk the ordinary entries into leaf shard Trees at content-defined boundaries,
        // emitting one TreeShard entry per leaf for the level above.
        let mut level: Vec<TreeEntry> = Vec::new();
        let mut run: Vec<TreeEntry> = Vec::new();
        for entry in sorted {
            let boundary = crate::prolly::is_boundary(entry.name.as_bytes(), 0);
            run.push(entry);
            if boundary {
                level.push(self.flush_shard_node(&mut run)?);
            }
        }
        if !run.is_empty() {
            level.push(self.flush_shard_node(&mut run)?);
        }

        // Interior levels: chunk the TreeShard entries the same way until one root shard remains.
        let mut depth: u8 = 1;
        while level.len() > 1 {
            let mut parents: Vec<TreeEntry> = Vec::new();
            let mut crun: Vec<TreeEntry> = Vec::new();
            for child in std::mem::take(&mut level) {
                let boundary = crate::prolly::is_boundary(child.name.as_bytes(), depth);
                crun.push(child);
                if boundary {
                    parents.push(self.flush_shard_node(&mut crun)?);
                }
            }
            if !crun.is_empty() {
                parents.push(self.flush_shard_node(&mut crun)?);
            }
            level = parents;
            depth = depth
                .checked_add(1)
                .ok_or_else(|| LoomError::corrupt("sharded directory too tall"))?;
        }
        // The single remaining shard entry's target is the root shard Tree's digest.
        Ok(level
            .into_iter()
            .next()
            .expect("a sharded directory has at least one shard node")
            .target)
    }

    /// Build one shard `Tree` object from `run` (ordinary entries make a leaf, or `TreeShard` entries
    /// make an interior node) and return a `TreeShard` entry pointing at it, named by the maximum entry
    /// name in the run (the child's max key).
    fn flush_shard_node(&mut self, run: &mut Vec<TreeEntry>) -> Result<TreeEntry> {
        let max_name = run.last().expect("non-empty shard run").name.clone();
        let node = self.put_object(&Object::tree(std::mem::take(run))?)?;
        Ok(TreeEntry {
            name: max_name,
            kind: EntryKind::TreeShard,
            target: node,
            mode: 0o040000,
        })
    }

    /// The full, flat entry list of a directory, resolving prolly sharding transparently: a flat
    /// directory or a shard leaf yields its ordinary entries; an interior shard node (all
    /// [`EntryKind::TreeShard`]) is descended in order. Every committed-directory walk routes here.
    pub(crate) fn tree_entries(&self, dir: Digest) -> Result<Vec<TreeEntry>> {
        self.collect_dir_entries(dir, 0)
    }

    fn collect_dir_entries(&self, node: Digest, depth: usize) -> Result<Vec<TreeEntry>> {
        if depth > MAX_SHARD_DEPTH {
            return Err(LoomError::corrupt(
                "directory sharded deeper than the maximum",
            ));
        }
        let Object::Tree(entries) = self.get_object(&node)? else {
            return Err(LoomError::corrupt(format!("expected a Tree at {node}")));
        };
        // A node is either all-ordinary (flat dir / shard leaf) or all-TreeShard (interior node).
        if entries.iter().all(|e| e.kind != EntryKind::TreeShard) {
            return Ok(entries);
        }
        let mut out = Vec::new();
        for entry in entries {
            match entry.kind {
                EntryKind::TreeShard => {
                    out.extend(self.collect_dir_entries(entry.target, depth + 1)?);
                }
                _ => {
                    return Err(LoomError::corrupt(
                        "shard node mixes TreeShard and ordinary entries",
                    ));
                }
            }
        }
        Ok(out)
    }

    /// Enqueue the object a tree entry points at for a reachability walk: a `Tree`/`Subloom`/`TreeShard`
    /// entry's target is an object digest directly (a `TreeShard` target is a child shard `Tree`); a
    /// `Blob`/`Symlink` entry's target is a *content address* resolved through the content index to its
    /// Blob object.
    fn enqueue_entry_target(
        &self,
        entry: &TreeEntry,
        queue: &mut VecDeque<Digest>,
        out: &mut BTreeSet<Digest>,
        have: &BTreeSet<Digest>,
    ) -> Result<()> {
        match entry.kind {
            // A `Table` entry's target is the table `Tree` (schema + row-map + index roots), walked
            // like any other Tree object.
            EntryKind::Tree
            | EntryKind::Subloom
            | EntryKind::TreeShard
            | EntryKind::Table
            | EntryKind::TimeSeries
            | EntryKind::Graph
            | EntryKind::Ledger
            | EntryKind::Columnar
            | EntryKind::Document => queue.push_back(entry.target),
            EntryKind::Blob | EntryKind::Symlink => {
                let obj = self.content.get(&entry.target).copied().ok_or_else(|| {
                    LoomError::not_found(format!(
                        "content {} for tree entry {:?}",
                        entry.target, entry.name
                    ))
                })?;
                queue.push_back(obj);
                let bytes = self.load_content(entry.target)?;
                if let Some(roots) =
                    crate::kv::prolly_roots_from_storage_bytes(self.store.digest_algo(), &bytes)?
                {
                    for root in roots {
                        for node in
                            crate::prolly::reachable_with_leaves(&self.store, &root, have)?.nodes
                        {
                            out.insert(node);
                        }
                    }
                }
            }
            // A `ProllyMap` entry's target is a prolly-tree root (raw node blobs, not framed objects),
            // so its nodes are collected straight into the live set, pruned by `have` for sync.
            EntryKind::ProllyMap => {
                for node in
                    crate::prolly::reachable_with_leaves(&self.store, &entry.target, have)?.nodes
                {
                    out.insert(node);
                }
            }
            // A `Stream` entry's target is the stream-root Tree; its metadata blob, entry-map prolly
            // nodes, and entry payload content are folded straight into the live set.
            EntryKind::Stream => {
                for d in self.stream_reachable(entry.target, have)? {
                    out.insert(d);
                }
            }
        }
        Ok(())
    }

    fn enqueue_mark_entry_target(
        &self,
        entry: &TreeEntry,
        state: &mut ReachabilityMarkState,
    ) -> Result<()> {
        match entry.kind {
            EntryKind::Tree
            | EntryKind::Subloom
            | EntryKind::TreeShard
            | EntryKind::Table
            | EntryKind::TimeSeries
            | EntryKind::Graph
            | EntryKind::Ledger
            | EntryKind::Columnar
            | EntryKind::Document => state.queue.push_back(entry.target),
            EntryKind::Blob | EntryKind::Symlink => {
                let obj = self.content.get(&entry.target).copied().ok_or_else(|| {
                    LoomError::not_found(format!(
                        "content {} for tree entry {:?}",
                        entry.target, entry.name
                    ))
                })?;
                state.queue.push_back(obj);
            }
            EntryKind::ProllyMap => {
                for node in crate::prolly::reachable_with_leaves(
                    &self.store,
                    &entry.target,
                    &BTreeSet::new(),
                )?
                .nodes
                {
                    state.marked.insert(node);
                }
            }
            EntryKind::Stream => state.stream_roots.push_back(entry.target),
        }
        Ok(())
    }

    pub(crate) fn flatten_commit(&self, commit: Digest) -> Result<(FileMap, BTreeSet<String>)> {
        let mut files = FileMap::new();
        let mut dirs = BTreeSet::new();
        let tree = self.get_commit(commit)?.tree;
        self.flatten_tree(tree, "", &mut files, &mut dirs)?;
        Ok((files, dirs))
    }

    fn flatten_tree(
        &self,
        tree: Digest,
        prefix: &str,
        files: &mut FileMap,
        dirs: &mut BTreeSet<String>,
    ) -> Result<()> {
        for entry in self.tree_entries(tree)? {
            let path = format!("{prefix}{}", entry.name);
            match entry.kind {
                EntryKind::Tree => {
                    dirs.insert(path.clone());
                    self.flatten_tree(entry.target, &format!("{path}/"), files, dirs)?;
                }
                EntryKind::Table => {
                    files.insert(path, StagedEntry::Table(entry.target));
                }
                EntryKind::Stream => {
                    files.insert(path, StagedEntry::Stream(entry.target));
                }
                EntryKind::TimeSeries => {
                    files.insert(path, StagedEntry::TimeSeries(entry.target));
                }
                EntryKind::Graph => {
                    files.insert(path, StagedEntry::Graph(entry.target));
                }
                EntryKind::Ledger => {
                    files.insert(path, StagedEntry::Ledger(entry.target));
                }
                EntryKind::Columnar => {
                    files.insert(path, StagedEntry::Columnar(entry.target));
                }
                EntryKind::Document => {
                    files.insert(path, StagedEntry::Document(entry.target));
                }
                _ => {
                    files.insert(
                        path,
                        StagedEntry::File(StagedFile {
                            content_addr: entry.target,
                            mode: entry.mode,
                        }),
                    );
                }
            }
        }
        Ok(())
    }

    pub(crate) fn store_content(&mut self, ns: WorkspaceId, bytes: &[u8]) -> Result<Digest> {
        let addr = content_address_with(self.store.digest_algo(), bytes);
        // The workspace's compression preference is hinted to the store; the frame stays below the
        // content address, so `addr` is unaffected.
        let hint = self.compression_for(ns);
        // Small content is one Blob; large content is split into chunk Blobs referenced by a
        // ChunkList, so an edit re-stores only the changed chunks. The content address is the hash of
        // the whole content either way.
        let obj = if bytes.len() <= crate::chunk::CHUNK_THRESHOLD {
            self.store
                .put_hint(&Object::Blob(bytes.to_vec()).canonical(), hint)?
        } else {
            let mut entries = Vec::new();
            for piece in crate::chunk::chunk(bytes) {
                let target = self
                    .store
                    .put_hint(&Object::Blob(piece.to_vec()).canonical(), hint)?;
                entries.push(crate::object::ChunkRef {
                    target,
                    size: piece.len() as u64,
                });
            }
            self.store.put_hint(
                &Object::ChunkList {
                    total_size: bytes.len() as u64,
                    entries,
                }
                .canonical(),
                hint,
            )?
        };
        self.content.insert(addr, obj);
        Ok(addr)
    }

    /// The compression hint for `ns`: an explicit per-workspace override if set, else a default derived
    /// from the workspace's facets. A write policy only; it never affects object identity.
    pub fn compression_for(&self, ns: WorkspaceId) -> CompressionHint {
        if let Some(h) = self.compression.get(&ns) {
            return *h;
        }
        self.registry
            .facets(ns)
            .map(|facets| default_compression_for_facets(&facets))
            .unwrap_or_default()
    }

    /// Set an explicit compression preference for `ns`, overriding the facet-derived default.
    pub fn set_workspace_compression(&mut self, ns: WorkspaceId, hint: CompressionHint) {
        self.compression.insert(ns, hint);
    }

    pub(crate) fn load_content(&self, addr: Digest) -> Result<Vec<u8>> {
        let obj = self
            .content
            .get(&addr)
            .ok_or_else(|| LoomError::not_found(format!("content {addr}")))?;
        match self.get_object(obj)? {
            Object::Blob(bytes) => Ok(bytes),
            // Reassemble a chunked file by concatenating its chunk Blobs in order.
            Object::ChunkList {
                total_size,
                entries,
            } => {
                let mut out = Vec::with_capacity(total_size as usize);
                for e in entries {
                    match self.get_object(&e.target)? {
                        Object::Blob(chunk) => out.extend_from_slice(&chunk),
                        other => {
                            return Err(LoomError::corrupt(format!(
                                "chunk {} is a {:?}, not a Blob",
                                e.target,
                                other.object_type()
                            )));
                        }
                    }
                }
                Ok(out)
            }
            other => Err(LoomError::corrupt(format!(
                "expected a Blob or ChunkList, got {:?}",
                other.object_type()
            ))),
        }
    }

    /// The byte length of the content at `addr`, read from the Blob or ChunkList header without
    /// materializing the bytes.
    pub(crate) fn content_size(&self, addr: Digest) -> Result<u64> {
        let obj = self
            .content
            .get(&addr)
            .ok_or_else(|| LoomError::not_found(format!("content {addr}")))?;
        match self.get_object(obj)? {
            Object::Blob(bytes) => Ok(bytes.len() as u64),
            Object::ChunkList { total_size, .. } => Ok(total_size),
            other => Err(LoomError::corrupt(format!(
                "expected a Blob or ChunkList, got {:?}",
                other.object_type()
            ))),
        }
    }

    /// Read `[offset, offset + len)` of the content at `addr`, loading only the chunks that overlap the
    /// range (bounded memory; a large file is never reassembled to serve a small read). The result is
    /// clamped to the end of content, so a read past the end returns fewer bytes (empty at or beyond
    /// the end), matching POSIX `pread`.
    pub(crate) fn content_read_range(
        &self,
        addr: Digest,
        offset: u64,
        len: u64,
    ) -> Result<Vec<u8>> {
        let obj = self
            .content
            .get(&addr)
            .ok_or_else(|| LoomError::not_found(format!("content {addr}")))?;
        match self.get_object(obj)? {
            Object::Blob(bytes) => {
                let total = bytes.len() as u64;
                let start = offset.min(total);
                let end = offset.saturating_add(len).min(total);
                Ok(bytes[start as usize..end as usize].to_vec())
            }
            Object::ChunkList {
                total_size,
                entries,
            } => {
                let end = offset.saturating_add(len).min(total_size);
                if offset >= end {
                    return Ok(Vec::new());
                }
                let mut out = Vec::with_capacity((end - offset) as usize);
                let mut pos = 0u64;
                for e in &entries {
                    let chunk_start = pos;
                    let chunk_end = pos + e.size;
                    pos = chunk_end;
                    if chunk_end <= offset {
                        continue;
                    }
                    if chunk_start >= end {
                        break;
                    }
                    let lo = offset.max(chunk_start);
                    let hi = end.min(chunk_end);
                    match self.get_object(&e.target)? {
                        Object::Blob(chunk) => {
                            out.extend_from_slice(
                                &chunk[(lo - chunk_start) as usize..(hi - chunk_start) as usize],
                            );
                        }
                        other => {
                            return Err(LoomError::corrupt(format!(
                                "chunk {} is a {:?}, not a Blob",
                                e.target,
                                other.object_type()
                            )));
                        }
                    }
                }
                Ok(out)
            }
            other => Err(LoomError::corrupt(format!(
                "expected a Blob or ChunkList, got {:?}",
                other.object_type()
            ))),
        }
    }

    /// The new content bytes for `[start, end)` of an [`EditPlan`]: old bytes where they exist, the
    /// written `data` where it lands, and zeros for any gap (a write past the old end, or a truncate
    /// that grows the file). Reads only the old chunks overlapping the window, so it stays bounded.
    fn edit_window(&self, plan: &EditPlan, start: u64, end: u64) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; (end - start) as usize];
        match &plan.kind {
            EditKind::Write { offset, data } => {
                if let Some(src) = plan.src {
                    let lo = start.min(plan.old_size);
                    let hi = end.min(plan.old_size);
                    if hi > lo {
                        let old = self.content_read_range(src, lo, hi - lo)?;
                        let b = (lo - start) as usize;
                        buf[b..b + old.len()].copy_from_slice(&old);
                    }
                }
                let dlen = data.len() as u64;
                let dlo = start.max(*offset);
                let dhi = end.min(offset + dlen);
                if dhi > dlo {
                    let bo = (dlo - start) as usize;
                    let ds = (dlo - offset) as usize;
                    let de = (dhi - offset) as usize;
                    buf[bo..bo + (de - ds)].copy_from_slice(&data[ds..de]);
                }
            }
            EditKind::Truncate => {
                if let Some(src) = plan.src {
                    let hi = end.min(plan.old_size);
                    if hi > start {
                        let old = self.content_read_range(src, start, hi - start)?;
                        buf[..old.len()].copy_from_slice(&old);
                    }
                }
            }
        }
        Ok(buf)
    }

    /// Build the edited content for `plan` and return its `(content address, size)`. Small results are a
    /// single Blob; larger ones are streamed through the content-defined chunker so only changed chunks
    /// are stored (unchanged chunks dedup to their existing objects), the whole file is never held in
    /// memory, and the whole-content address is computed incrementally. The address and chunk objects
    /// are byte-for-byte identical to writing the same final bytes through [`Loom::store_content`].
    pub(crate) fn build_edited_content(
        &mut self,
        ns: WorkspaceId,
        plan: EditPlan,
    ) -> Result<(Digest, u64)> {
        let new_size = plan.new_size;
        let algo = self.store.digest_algo();
        let hint = self.compression_for(ns);
        // Window size for streaming the new content; bounds peak memory.
        const BLOCK: u64 = crate::chunk::CHUNK_THRESHOLD as u64;
        if (new_size as usize) <= crate::chunk::CHUNK_THRESHOLD {
            let mut buf = Vec::with_capacity(new_size as usize);
            let mut pos = 0u64;
            while pos < new_size {
                let end = (pos + BLOCK).min(new_size);
                buf.extend_from_slice(&self.edit_window(&plan, pos, end)?);
                pos = end;
            }
            let addr = content_address_with(algo, &buf);
            let obj = self.store.put_hint(&Object::Blob(buf).canonical(), hint)?;
            self.content.insert(addr, obj);
            return Ok((addr, new_size));
        }
        let mut chunker = crate::chunk::StreamChunker::new();
        let mut entries: Vec<crate::object::ChunkRef> = Vec::new();
        let mut hasher = crate::digest::ContentHasher::new(algo);
        let mut pos = 0u64;
        while pos < new_size {
            let end = (pos + BLOCK).min(new_size);
            let window = self.edit_window(&plan, pos, end)?;
            pos = end;
            hasher.update(&window);
            // Collect emitted chunks first, then store them: the chunker borrow must end before the
            // mutable store write.
            let mut emitted: Vec<Vec<u8>> = Vec::new();
            chunker.push(&window, |c| emitted.push(c.to_vec()));
            for chunk in emitted {
                let size = chunk.len() as u64;
                let target = self
                    .store
                    .put_hint(&Object::Blob(chunk).canonical(), hint)?;
                entries.push(crate::object::ChunkRef { target, size });
            }
        }
        let mut tail: Vec<Vec<u8>> = Vec::new();
        chunker.finish(|c| tail.push(c.to_vec()));
        for chunk in tail {
            let size = chunk.len() as u64;
            let target = self
                .store
                .put_hint(&Object::Blob(chunk).canonical(), hint)?;
            entries.push(crate::object::ChunkRef { target, size });
        }
        let addr = hasher.finish();
        let obj = self.store.put_hint(
            &Object::ChunkList {
                total_size: new_size,
                entries,
            }
            .canonical(),
            hint,
        )?;
        self.content.insert(addr, obj);
        Ok((addr, new_size))
    }

    /// Set the file slot at `path` to `(addr, size, mode)`, routing through the live inode when the path
    /// is open so all handles see the change, and mirroring linked content into the working tree.
    pub(crate) fn put_file_slot(
        &mut self,
        ns: WorkspaceId,
        path: &str,
        addr: Digest,
        size: u64,
        mode: u32,
    ) {
        if let Some(&id) = self.path_to_inode.get(&(ns, path.to_string()))
            && let Some(ino) = self.inodes.get_mut(&id)
        {
            ino.content_addr = addr;
            ino.size = size;
            ino.mode = mode;
        }
        self.work.entry(ns).or_default().insert(
            path.to_string(),
            StagedEntry::File(StagedFile {
                content_addr: addr,
                mode,
            }),
        );
    }

    /// Apply new content to an open inode and mirror it to the working tree if the inode is still linked
    /// (an unlinked inode's bytes are private to its handles until the last close).
    pub(crate) fn apply_inode_content(
        &mut self,
        inode: u64,
        addr: Digest,
        size: u64,
    ) -> Result<()> {
        let (ns, path, mode) = {
            let ino = self
                .inodes
                .get_mut(&inode)
                .ok_or_else(|| LoomError::not_found("inode"))?;
            ino.content_addr = addr;
            ino.size = size;
            (ino.ns, ino.path.clone(), ino.mode)
        };
        if let Some(p) = path {
            self.work.entry(ns).or_default().insert(
                p,
                StagedEntry::File(StagedFile {
                    content_addr: addr,
                    mode,
                }),
            );
        }
        Ok(())
    }

    pub(crate) fn put_object(&mut self, obj: &Object) -> Result<Digest> {
        self.store.put(&obj.canonical())
    }

    pub(crate) fn get_object(&self, digest: &Digest) -> Result<Object> {
        let bytes = self
            .store
            .get(digest)?
            .ok_or_else(|| LoomError::not_found(format!("object {digest}")))?;
        Object::decode(&bytes)
    }

    pub(crate) fn get_commit(&self, digest: Digest) -> Result<Commit> {
        match self.get_object(&digest)? {
            Object::Commit(c) => Ok(c),
            other => Err(LoomError::corrupt(format!(
                "expected a Commit, got {:?}",
                other.object_type()
            ))),
        }
    }
}
