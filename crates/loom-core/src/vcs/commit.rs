use super::*;

impl<S: ObjectStore> Loom<S> {
    /// Record `dir` and all its ancestor directories as existing in `ns` (the root is implicit).
    pub(crate) fn record_dir(&mut self, ns: WorkspaceId, dir: &str) {
        let set = self.dirs.entry(ns).or_default();
        let mut cur = dir;
        while !cur.is_empty() {
            set.insert(cur.to_string());
            match cur.rsplit_once('/') {
                Some((parent, _)) => cur = parent,
                None => break,
            }
        }
    }

    /// Whether `dir` (normalized, no leading `/`) is an existing directory in `ns`. The root (`""`)
    /// always exists; otherwise the directory must have been created explicitly or recovered on
    /// checkout. Directories are never implicit.
    pub(crate) fn dir_exists(&self, ns: WorkspaceId, dir: &str) -> bool {
        dir.is_empty() || self.dirs.get(&ns).is_some_and(|set| set.contains(dir))
    }

    // ---- commit / checkout ----------------------------------------------------------------------

    /// Snapshot `ns`'s working tree into a commit on its current `HEAD` branch and advance the branch
    /// tip (compare-and-swap). The parent is the previous tip (none for the first commit).
    pub fn commit(
        &mut self,
        ns: WorkspaceId,
        author: &str,
        message: &str,
        timestamp_ms: u64,
    ) -> Result<Digest> {
        self.ensure_full_state_loaded()?;
        let head = self.registry.head_branch(ns)?;
        let head_ref = format!("branch/{head}");
        self.authorize_ref(ns, &head_ref, AclRight::Write)?;
        self.authorize_ref(ns, &head_ref, AclRight::Advance)?;
        let parent = self.registry.branch_tip(ns, &head)?;
        let files = self.work.get(&ns).cloned().unwrap_or_default();
        let dirs = self.dirs.get(&ns).cloned().unwrap_or_default();
        let root = self.build_subtree(&files, &dirs, "")?;
        let parents = parent.map_or_else(Vec::new, |p| vec![p]);
        let commit = Object::Commit(Commit {
            tree: root,
            parents,
            author: author.to_string(),
            timestamp_ms,
            message: message.to_string(),
            meta: BTreeMap::new(),
        });
        let digest = self.put_object(&commit)?;
        self.authorize_branch_update(ns, &head_ref, parent, digest)?;
        match parent {
            None => self.registry.branch_create(ns, &head, digest)?,
            Some(prev) => self.registry.update_branch(ns, &head, Some(prev), digest)?,
        }
        // A `commit` snapshots the whole working tree, so the index matches the committed tree after it:
        // the workspace is clean (no staged-vs-committed difference).
        self.index.insert(ns, files);
        Ok(digest)
    }

    /// The `HEAD` tip's tree as a working-tree map, or empty when the branch has no commits.
    fn head_tree(&self, ns: WorkspaceId) -> Result<WorkTree> {
        self.ensure_full_state_available()?;
        let head = self.registry.head_branch(ns)?;
        match self.registry.branch_tip(ns, &head)? {
            Some(tip) => Ok(self.flatten_commit(tip)?.0),
            None => Ok(WorkTree::new()),
        }
    }

    /// Stage `paths`: copy each path's current working-tree slot into the one shared index (a path
    /// missing from the working tree stages its deletion). The index spans every facet in the workspace.
    pub fn stage(&mut self, ns: WorkspaceId, paths: &[&str]) -> Result<()> {
        self.ensure_full_state_loaded()?;
        self.authorize(ns, FacetKind::Vcs, AclRight::Write)?;
        let work = self.work.get(&ns).cloned().unwrap_or_default();
        let idx = self.index.entry(ns).or_default();
        for p in paths {
            let path = normalize_path(p)?;
            match work.get(&path) {
                Some(slot) => {
                    idx.insert(path, *slot);
                }
                None => {
                    idx.remove(&path);
                }
            }
        }
        Ok(())
    }

    /// Stage the entire working tree (every change across every facet) into the shared index.
    pub fn stage_all(&mut self, ns: WorkspaceId) -> Result<()> {
        self.ensure_full_state_loaded()?;
        self.authorize(ns, FacetKind::Vcs, AclRight::Write)?;
        let work = self.work.get(&ns).cloned().unwrap_or_default();
        self.index.insert(ns, work);
        Ok(())
    }

    /// Unstage `paths`: reset each path's index entry to its `HEAD` state, removing a staged change.
    pub fn unstage(&mut self, ns: WorkspaceId, paths: &[&str]) -> Result<()> {
        self.ensure_full_state_loaded()?;
        self.authorize(ns, FacetKind::Vcs, AclRight::Write)?;
        let head = self.head_tree(ns)?;
        let idx = self.index.entry(ns).or_default();
        for p in paths {
            let path = normalize_path(p)?;
            match head.get(&path) {
                Some(slot) => {
                    idx.insert(path, *slot);
                }
                None => {
                    idx.remove(&path);
                }
            }
        }
        Ok(())
    }

    /// The working state of `ns` relative to `HEAD` and the index (see [`Status`]).
    pub fn status(&self, ns: WorkspaceId) -> Result<Status> {
        self.ensure_full_state_available()?;
        self.authorize(ns, FacetKind::Vcs, AclRight::Read)?;
        let head = self.head_tree(ns)?;
        let index = self.index.get(&ns).cloned().unwrap_or_default();
        let work = self.work.get(&ns).cloned().unwrap_or_default();
        let staged = worktree_changes(&head, &index);
        let mut unstaged = Vec::new();
        let mut untracked = Vec::new();
        for change in worktree_changes(&index, &work) {
            if change.kind == ChangeKind::Added && !head.contains_key(&change.path) {
                untracked.push(change.path);
            } else {
                unstaged.push(change);
            }
        }
        Ok(Status {
            staged,
            unstaged,
            untracked,
            conflicts: self.merge_conflicts(ns)?,
        })
    }

    /// Commit only the staged index (`commit --staged`): record a commit whose tree is the shared index
    /// and advance the branch (compare-and-swap). The working tree's unstaged changes are left in place,
    /// and the index continues to match the new `HEAD` tree.
    pub fn commit_staged(
        &mut self,
        ns: WorkspaceId,
        author: &str,
        message: &str,
        timestamp_ms: u64,
    ) -> Result<Digest> {
        let head = self.registry.head_branch(ns)?;
        let head_ref = format!("branch/{head}");
        self.authorize_ref(ns, &head_ref, AclRight::Write)?;
        self.authorize_ref(ns, &head_ref, AclRight::Advance)?;
        let parent = self.registry.branch_tip(ns, &head)?;
        let files = self.index.get(&ns).cloned().unwrap_or_default();
        let dirs = self.dirs.get(&ns).cloned().unwrap_or_default();
        let root = self.build_subtree(&files, &dirs, "")?;
        let parents = parent.map_or_else(Vec::new, |p| vec![p]);
        let commit = Object::Commit(Commit {
            tree: root,
            parents,
            author: author.to_string(),
            timestamp_ms,
            message: message.to_string(),
            meta: BTreeMap::new(),
        });
        let digest = self.put_object(&commit)?;
        self.authorize_branch_update(ns, &head_ref, parent, digest)?;
        match parent {
            None => self.registry.branch_create(ns, &head, digest)?,
            Some(prev) => self.registry.update_branch(ns, &head, Some(prev), digest)?,
        }
        Ok(digest)
    }

    /// Materialize `commit`'s tree into `ns`'s working tree (replacing it), resetting the staging index
    /// to match. Does not move `HEAD`.
    pub fn checkout_commit(&mut self, ns: WorkspaceId, commit: Digest) -> Result<()> {
        self.authorize(ns, FacetKind::Vcs, AclRight::Read)?;
        let (files, dirs) = self.flatten_commit(commit)?;
        self.index.insert(ns, files.clone());
        self.work.insert(ns, files);
        self.dirs.insert(ns, dirs);
        // The working tree was replaced, so any ephemeral cache over a backing map in this workspace is
        // now stale; drop it (entries and buffered write-behind deltas). Callers wanting write-behind
        // durability flush_pending first - the host does on its cadence.
        self.drop_ephemeral_caches(ns);
        Ok(())
    }

    /// Switch `HEAD` to `branch` and materialize its tip into the working tree.
    pub fn checkout_branch(&mut self, ns: WorkspaceId, branch: &str) -> Result<()> {
        let branch_ref = format!("branch/{branch}");
        self.authorize_ref(ns, &branch_ref, AclRight::Read)?;
        self.authorize_ref(ns, &branch_ref, AclRight::Write)?;
        self.registry.set_head(ns, branch)?;
        match self.registry.branch_tip(ns, branch)? {
            Some(tip) => self.checkout_commit(ns, tip)?,
            None => {
                self.work.insert(ns, WorkTree::new());
                self.index.insert(ns, WorkTree::new());
                self.dirs.insert(ns, BTreeSet::new());
            }
        }
        Ok(())
    }

    /// Mark every ancestor directory of `path` as existing in `ns` (so a restored deep path has its
    /// parents present). The root (`""`) is always implied and not recorded.
    fn ensure_ancestor_dirs(&mut self, ns: WorkspaceId, path: &str) {
        let dirs = self.dirs.entry(ns).or_default();
        let mut acc = String::new();
        let segments: Vec<&str> = path.split('/').collect();
        // Every segment except the last names a directory.
        for seg in &segments[..segments.len().saturating_sub(1)] {
            if !acc.is_empty() {
                acc.push('/');
            }
            acc.push_str(seg);
            dirs.insert(acc.clone());
        }
    }

    /// Restore one path in `ns`'s working tree to its state at the snapshot `rev` resolves to (HEAD, a
    /// branch name, or a digest), like `git restore --source`. The restored slot is whatever kind the
    /// snapshot held at that path (file, symlink, table, or stream); if the path did not exist in the
    /// snapshot it is removed from the working tree. This touches the working tree only - `HEAD`, the
    /// branch, and the staging index are unchanged.
    pub fn restore_file(&mut self, ns: WorkspaceId, rev: &str, path: &str) -> Result<()> {
        self.authorize(ns, FacetKind::Vcs, AclRight::Write)?;
        let path = normalize_path(path)?;
        let commit = self.resolve_rev(ns, rev)?;
        let (files, _dirs) = self.flatten_commit(commit)?;
        match files.get(&path) {
            Some(slot) => {
                let slot = *slot;
                self.ensure_ancestor_dirs(ns, &path);
                self.work.entry(ns).or_default().insert(path, slot);
            }
            None => {
                if let Some(w) = self.work.get_mut(&ns) {
                    w.remove(&path);
                }
            }
        }
        Ok(())
    }

    /// Restore an entire subtree under `prefix` in `ns`'s working tree to its state at the snapshot `rev`
    /// resolves to (path-restricted checkout). A `prefix` of `""` or `"/"` restores the whole tree
    /// (a checkout that does not move `HEAD`). Working-tree paths under `prefix` that are absent in the
    /// snapshot are removed; directories under `prefix` are resynced to the snapshot. This touches the
    /// working tree only - `HEAD`, the branch, and the staging index are unchanged.
    pub fn restore_path(&mut self, ns: WorkspaceId, rev: &str, prefix: &str) -> Result<()> {
        self.authorize(ns, FacetKind::Vcs, AclRight::Write)?;
        let commit = self.resolve_rev(ns, rev)?;
        let (files, snap_dirs) = self.flatten_commit(commit)?;
        // Normalize the prefix; an empty/root prefix selects everything.
        let prefix = prefix
            .trim_start_matches('/')
            .trim_end_matches('/')
            .to_string();
        let under = |key: &str| -> bool {
            prefix.is_empty() || key == prefix || key.starts_with(&format!("{prefix}/"))
        };
        // Replace working-tree slots under the prefix with the snapshot's.
        let work = self.work.entry(ns).or_default();
        let remove: Vec<String> = work.keys().filter(|k| under(k)).cloned().collect();
        for k in remove {
            work.remove(&k);
        }
        for (k, slot) in &files {
            if under(k) {
                work.insert(k.clone(), *slot);
            }
        }
        // Resync directories under the prefix; keep the prefix's own ancestors present.
        let dirs = self.dirs.entry(ns).or_default();
        let remove_dirs: Vec<String> = dirs.iter().filter(|d| under(d)).cloned().collect();
        for d in remove_dirs {
            dirs.remove(&d);
        }
        for d in &snap_dirs {
            if under(d) {
                dirs.insert(d.clone());
            }
        }
        if !prefix.is_empty() {
            self.ensure_ancestor_dirs(ns, &prefix);
        }
        Ok(())
    }

    /// Create `name` at the current `HEAD` tip.
    pub fn branch(&mut self, ns: WorkspaceId, name: &str) -> Result<()> {
        let branch_ref = format!("branch/{name}");
        self.authorize_ref(ns, &branch_ref, AclRight::Advance)?;
        let head = self.registry.head_branch(ns)?;
        let tip = self
            .registry
            .branch_tip(ns, &head)?
            .ok_or_else(|| LoomError::not_found("no commit to branch from"))?;
        self.evaluate_protected_ref_update(ns, &branch_ref, None, Some(tip))?;
        self.registry.branch_create(ns, name, tip)
    }

    /// Delete branch `name`. The current branch is rejected; protected-ref deletion policy applies.
    pub fn branch_delete(&mut self, ns: WorkspaceId, name: &str) -> Result<()> {
        let branch_ref = format!("branch/{name}");
        self.authorize_ref(ns, &branch_ref, AclRight::Admin)?;
        let old = self.registry.branch_tip(ns, name)?;
        self.evaluate_protected_ref_update(ns, &branch_ref, old, None)?;
        self.registry.branch_delete(ns, name)
    }

    // ---- tags -----------------------------------------------------------------------------------

    /// Resolve a revision string to a commit digest in `ns`.
    pub fn resolve_rev(&self, ns: WorkspaceId, rev: &str) -> Result<Digest> {
        let digest = if rev == "HEAD" {
            let head = self.registry.head_branch(ns)?;
            self.registry
                .branch_tip(ns, &head)?
                .ok_or_else(|| LoomError::not_found(format!("revision {rev:?}: HEAD is unborn")))?
        } else if let Some(value) = rev.strip_prefix("commit:") {
            parse_rev_digest(self.store.digest_algo(), value).ok_or_else(|| {
                LoomError::not_found(format!(
                    "revision {rev:?} did not resolve to a commit digest"
                ))
            })?
        } else if let Some(branch) = rev.strip_prefix("branch:") {
            self.registry.branch_tip(ns, branch)?.ok_or_else(|| {
                LoomError::not_found(format!("revision {rev:?} did not resolve to a branch"))
            })?
        } else if let Some(tag) = rev.strip_prefix("tag:") {
            self.resolve_tag_rev(ns, tag, rev)?
        } else if let Some(d) = parse_rev_digest(self.store.digest_algo(), rev) {
            d
        } else if let Some(tip) = self.registry.branch_tip(ns, rev)? {
            tip
        } else {
            return Err(LoomError::not_found(format!(
                "revision {rev:?} did not resolve to HEAD, a digest, a branch, or a tag"
            )));
        };
        match self.get_object(&digest)? {
            Object::Commit(_) => Ok(digest),
            other => Err(LoomError::invalid(format!(
                "revision {rev:?} resolves to a {:?}, not a commit",
                other.object_type()
            ))),
        }
    }

    fn resolve_tag_rev(&self, ns: WorkspaceId, tag: &str, rev: &str) -> Result<Digest> {
        let target = self.registry.tag_target(ns, tag)?.ok_or_else(|| {
            LoomError::not_found(format!("revision {rev:?} did not resolve to a tag"))
        })?;
        match self.get_object(&target)? {
            Object::Commit(_) => Ok(target),
            Object::Tag(tag_object) if tag_object.target_type == ObjectType::Commit => {
                Ok(tag_object.target)
            }
            other => Err(LoomError::invalid(format!(
                "revision {rev:?} resolves to a {:?}, not a commit",
                other.object_type()
            ))),
        }
    }

    /// Create tag `name` in `ns` pointing at the commit `rev` resolves to. An empty `message` makes a
    /// lightweight tag (the ref points straight at the commit); a non-empty `message` makes an annotated
    /// tag (a stored [`Object::Tag`] carrying `tagger`/`message`/`timestamp_ms`, with the ref pointing at
    /// that tag object). Returns the ref target digest (the commit, or the tag object). `ALREADY_EXISTS`
    /// if the name is taken; `NOT_FOUND`/`INVALID_ARGUMENT` if `rev` does not resolve to a commit.
    pub fn tag_create(
        &mut self,
        ns: WorkspaceId,
        name: &str,
        rev: &str,
        tagger: &str,
        message: &str,
        timestamp_ms: u64,
    ) -> Result<Digest> {
        let tag_ref = format!("tag/{name}");
        self.authorize_ref(ns, &tag_ref, AclRight::Admin)?;
        let commit = self.resolve_rev(ns, rev)?;
        let target = if message.is_empty() {
            commit
        } else {
            self.put_object(&Object::Tag(Tag {
                target: commit,
                target_type: ObjectType::Commit,
                name: name.to_string(),
                tagger: tagger.to_string(),
                timestamp_ms,
                message: message.to_string(),
            }))?
        };
        self.evaluate_protected_ref_update(ns, &tag_ref, None, Some(target))?;
        self.registry.tag_create(ns, name, target)?;
        Ok(target)
    }

    /// All tag names in `ns`, sorted.
    pub fn tag_list(&self, ns: WorkspaceId) -> Result<Vec<String>> {
        self.authorize(ns, FacetKind::Vcs, AclRight::Read)?;
        self.registry.tag_list(ns)
    }

    /// The raw ref target of tag `name` (the commit for a lightweight tag, the [`Object::Tag`] digest for
    /// an annotated one), or `None` if the tag is absent. Kept raw so sync/clone recreate annotated tags
    /// without dropping their tag object.
    pub fn tag_target(&self, ns: WorkspaceId, name: &str) -> Result<Option<Digest>> {
        self.authorize_ref(ns, &format!("tag/{name}"), AclRight::Read)?;
        self.registry.tag_target(ns, name)
    }

    /// Delete tag `name`. `NOT_FOUND` if absent.
    pub fn tag_delete(&mut self, ns: WorkspaceId, name: &str) -> Result<()> {
        let tag_ref = format!("tag/{name}");
        self.authorize_ref(ns, &tag_ref, AclRight::Admin)?;
        let old = self.registry.tag_target(ns, name)?;
        self.evaluate_protected_ref_update(ns, &tag_ref, old, None)?;
        self.registry.tag_delete(ns, name)
    }

    /// Rename tag `old` to `new`, preserving its target. `NOT_FOUND` if `old` is absent; `ALREADY_EXISTS`
    /// if `new` is taken.
    pub fn tag_rename(&mut self, ns: WorkspaceId, old: &str, new: &str) -> Result<()> {
        let old_ref = format!("tag/{old}");
        let new_ref = format!("tag/{new}");
        self.authorize_ref(ns, &old_ref, AclRight::Admin)?;
        self.authorize_ref(ns, &new_ref, AclRight::Admin)?;
        let target = self.registry.tag_target(ns, old)?;
        self.evaluate_protected_ref_update(ns, &old_ref, target, None)?;
        self.evaluate_protected_ref_update(ns, &new_ref, None, target)?;
        self.registry.tag_rename(ns, old, new)
    }

    // ---- history replay (cherry-pick / revert / rebase) -----------------------------------------
}
