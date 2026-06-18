use super::*;

impl<S: ObjectStore> Loom<S> {
    /// The per-slot 3-way merge seam: whole-slot 3-way for every slot kind, with a row-level (or
    /// cell-level) override for `sql`/`columnar` tables. Returns the merged working tree and the paths
    /// left unresolved (a file content conflict, a table schema change, or a genuine same-row conflict).
    /// This is the single dispatch point the merge and all replay ops share; richer per-facet mergers
    /// (graph, kv, vector) plug in here (see the 0003b "per-facet merge strategy" design note).
    pub(crate) fn resolve_three_way(
        &mut self,
        ns: WorkspaceId,
        base: &FileMap,
        ours: &FileMap,
        theirs: &FileMap,
        cell_level: bool,
    ) -> Result<(WorkTree, Vec<String>)> {
        let (mut merged, conflicts) = three_way_merge_files(base, ours, theirs);
        let mut unresolved = Vec::new();
        for path in conflicts {
            if let (Some(StagedEntry::Table(o)), Some(StagedEntry::Table(t))) =
                (ours.get(&path), theirs.get(&path))
            {
                let base_t = match base.get(&path) {
                    Some(StagedEntry::Table(b)) => Some(*b),
                    _ => None,
                };
                if let Some(tree) = self.try_row_merge_table(base_t, *o, *t, cell_level)? {
                    merged.insert(path, StagedEntry::Table(tree));
                    continue;
                }
            }
            if let (Some(StagedEntry::Document(o)), Some(StagedEntry::Document(t))) =
                (ours.get(&path), theirs.get(&path))
            {
                let base_d = match base.get(&path) {
                    Some(StagedEntry::Document(b)) => Some(*b),
                    _ => None,
                };
                if let Some(root) =
                    crate::document::try_merge_document_roots(self, ns, base_d, *o, *t)?
                {
                    merged.insert(path, StagedEntry::Document(root));
                    continue;
                }
            }
            if let Some(slot) = self.try_vector_entry_merge(
                ns,
                &path,
                base.get(&path),
                ours.get(&path),
                theirs.get(&path),
            )? {
                merged.insert(path, slot);
                continue;
            }
            if self.try_search_collection_merge(ns, &path, base, ours, theirs, &mut merged)? {
                continue;
            }
            unresolved.push(path);
        }
        Ok((merged, unresolved))
    }

    fn try_search_collection_merge(
        &mut self,
        ns: WorkspaceId,
        path: &str,
        base_flat: &FileMap,
        ours_flat: &FileMap,
        theirs_flat: &FileMap,
        merged: &mut WorkTree,
    ) -> Result<bool> {
        let DiffPath::SearchCollection { collection } =
            classify_diff_path(path, self.store.digest_algo())
        else {
            return Ok(false);
        };
        let base = base_flat.get(path);
        let (Some(StagedEntry::File(ours)), Some(StagedEntry::File(theirs))) =
            (ours_flat.get(path), theirs_flat.get(path))
        else {
            return Ok(false);
        };
        if ours.mode != theirs.mode {
            return Ok(false);
        }
        let base_collection = match base {
            Some(StagedEntry::File(file)) => {
                Some(self.search_collection_at(base_flat, &collection, file)?)
            }
            Some(_) => return Ok(false),
            None => None,
        };
        let ours_collection = self.search_collection_at(ours_flat, &collection, ours)?;
        let theirs_collection = self.search_collection_at(theirs_flat, &collection, theirs)?;
        let Some(collection_root) = crate::search::merge_search_collections(
            base_collection.as_ref(),
            &ours_collection,
            &theirs_collection,
        ) else {
            return Ok(false);
        };
        let (root, components) =
            crate::search::encode_structured_search_storage(self, &collection, &collection_root);
        let document_prefix = format!("{}/", crate::search::source_document_dir(&collection));
        let component_paths = components.keys().cloned().collect::<BTreeSet<_>>();
        for stale in merged
            .keys()
            .filter(|candidate| {
                candidate.starts_with(&document_prefix) && !component_paths.contains(*candidate)
            })
            .cloned()
            .collect::<Vec<_>>()
        {
            merged.remove(&stale);
        }
        for (component_path, bytes) in components {
            let content_addr = self.store_content(ns, &bytes)?;
            merged.insert(
                component_path,
                StagedEntry::File(StagedFile {
                    content_addr,
                    mode: 0o100644,
                }),
            );
        }
        let content_addr = self.store_content(ns, &root)?;
        merged.insert(
            path.to_string(),
            StagedEntry::File(StagedFile {
                content_addr,
                mode: ours.mode,
            }),
        );
        Ok(true)
    }

    fn search_collection_at(
        &self,
        flat: &FileMap,
        collection: &str,
        file: &StagedFile,
    ) -> Result<crate::search::SearchCollection> {
        let root = self.load_content(file.content_addr)?;
        crate::search::decode_search_storage_with_components(
            self.store.digest_algo(),
            &root,
            |digest| match flat.get(&crate::search::source_document_path(collection, digest)) {
                Some(StagedEntry::File(file)) => self.load_content(file.content_addr),
                Some(_) => Err(LoomError::corrupt(
                    "search document component is not a file",
                )),
                None => Err(LoomError::not_found("search document component")),
            },
        )
    }

    fn try_vector_entry_merge(
        &mut self,
        ns: WorkspaceId,
        path: &str,
        base: Option<&StagedEntry>,
        ours: Option<&StagedEntry>,
        theirs: Option<&StagedEntry>,
    ) -> Result<Option<StagedEntry>> {
        let DiffPath::VectorEntry { .. } = classify_diff_path(path, self.store.digest_algo())
        else {
            return Ok(None);
        };
        let (Some(StagedEntry::File(ours)), Some(StagedEntry::File(theirs))) = (ours, theirs)
        else {
            return Ok(None);
        };
        if ours.mode != theirs.mode {
            return Ok(None);
        }
        let base_bytes = match base {
            Some(StagedEntry::File(file)) => Some(self.load_content(file.content_addr)?),
            Some(_) => return Ok(None),
            None => None,
        };
        let ours_bytes = self.load_content(ours.content_addr)?;
        let theirs_bytes = self.load_content(theirs.content_addr)?;
        let Some(merged) =
            crate::vector::merge_entry_bytes(base_bytes.as_deref(), &ours_bytes, &theirs_bytes)?
        else {
            return Ok(None);
        };
        let content_addr = self.store_content(ns, &merged)?;
        Ok(Some(StagedEntry::File(StagedFile {
            content_addr,
            mode: ours.mode,
        })))
    }

    /// Replay a sequence of patches onto `start` (the commit to build on, or `None` for an empty base),
    /// advancing the current `HEAD` branch to the result. Each patch is a 3-way apply (its `base` ->
    /// `theirs`) onto the running tree. Atomic: the first conflicting step returns
    /// [`ReplayOutcome::Conflicts`] with the branch and working tree untouched. A `dry_run` computes the
    /// same result in memory and returns [`ReplayOutcome::Clean`] / `Conflicts` without creating commits
    /// or moving the branch. On a clean real run the branch advances by compare-and-swap and the working
    /// tree is checked out to the result.
    fn replay_onto(
        &mut self,
        ns: WorkspaceId,
        start: Option<Digest>,
        steps: Vec<ReplayPatch>,
        timestamp_ms: u64,
        dry_run: bool,
    ) -> Result<ReplayOutcome> {
        if !dry_run && self.merge_state.contains_key(&ns) {
            return Err(LoomError::new(
                Code::Conflict,
                "a merge is in progress; resolve and continue it, or abort it first",
            ));
        }
        if steps.is_empty() {
            return Ok(ReplayOutcome::Empty);
        }
        let head = self.registry.head_branch(ns)?;
        let orig_tip = self.registry.branch_tip(ns, &head)?;
        let (mut running, mut running_dirs) = match start {
            Some(d) => self.flatten_commit(d)?,
            None => (FileMap::new(), BTreeSet::new()),
        };
        let mut tip = start;
        for patch in steps {
            let (merged, unresolved) =
                self.resolve_three_way(ns, &patch.base, &running, &patch.theirs, false)?;
            if !unresolved.is_empty() {
                return Ok(ReplayOutcome::Conflicts(unresolved));
            }
            running = merged;
            running_dirs.extend(patch.theirs_dirs);
            if !dry_run {
                let root = self.build_subtree(&running, &running_dirs, "")?;
                let commit = Object::Commit(Commit {
                    tree: root,
                    parents: tip.into_iter().collect(),
                    author: patch.author,
                    timestamp_ms,
                    message: patch.message,
                    meta: BTreeMap::new(),
                });
                tip = Some(self.put_object(&commit)?);
            }
        }
        if dry_run {
            return Ok(ReplayOutcome::Clean);
        }
        let final_tip = tip.expect("a non-empty real replay produces a tip");
        let head_ref = format!("branch/{head}");
        self.authorize_branch_update(ns, &head_ref, orig_tip, final_tip)?;
        self.registry
            .update_branch(ns, &head, orig_tip, final_tip)?;
        self.checkout_commit(ns, final_tip)?;
        Ok(ReplayOutcome::Replayed(final_tip))
    }

    /// Build the `(base, theirs)` patch pair for applying commit `c` (cherry-pick direction): `base` is
    /// `c`'s first parent (empty for a root commit) and `theirs` is `c`.
    fn pick_patch(&self, c: Digest, author: String, message: String) -> Result<ReplayPatch> {
        let commit = self.get_commit(c)?;
        let (theirs, theirs_dirs) = self.flatten_commit(c)?;
        let base = match commit.parents.first() {
            Some(p) => self.flatten_commit(*p)?.0,
            None => FileMap::new(),
        };
        Ok(ReplayPatch {
            base,
            theirs,
            theirs_dirs,
            author,
            message,
        })
    }

    /// Cherry-pick `commits` onto the current branch tip, in order, each as a new single-parent commit
    /// preserving the original author and message. Atomic and dry-runnable (see [`ReplayOutcome`]).
    pub fn cherry_pick(
        &mut self,
        ns: WorkspaceId,
        commits: &[Digest],
        timestamp_ms: u64,
        dry_run: bool,
    ) -> Result<ReplayOutcome> {
        self.authorize(ns, FacetKind::Vcs, AclRight::Write)?;
        self.authorize(ns, FacetKind::Vcs, AclRight::Advance)?;
        let head = self.registry.head_branch(ns)?;
        let tip = self.registry.branch_tip(ns, &head)?;
        let mut steps = Vec::with_capacity(commits.len());
        for &c in commits {
            let commit = self.get_commit(c)?;
            steps.push(self.pick_patch(c, commit.author.clone(), commit.message.clone())?);
        }
        self.replay_onto(ns, tip, steps, timestamp_ms, dry_run)
    }

    /// Revert `commits` on the current branch, in order, each as a new commit that undoes the original
    /// (author is `author`, message is `Revert "<subject>"`). Atomic and dry-runnable.
    pub fn revert(
        &mut self,
        ns: WorkspaceId,
        commits: &[Digest],
        author: &str,
        timestamp_ms: u64,
        dry_run: bool,
    ) -> Result<ReplayOutcome> {
        self.authorize(ns, FacetKind::Vcs, AclRight::Write)?;
        self.authorize(ns, FacetKind::Vcs, AclRight::Advance)?;
        let head = self.registry.head_branch(ns)?;
        let tip = self.registry.branch_tip(ns, &head)?;
        let mut steps = Vec::with_capacity(commits.len());
        for &c in commits {
            let commit = self.get_commit(c)?;
            // Inverse patch: base is the commit, theirs is its parent (empty for a root commit).
            let (base, _) = self.flatten_commit(c)?;
            let (theirs, theirs_dirs) = match commit.parents.first() {
                Some(p) => self.flatten_commit(*p)?,
                None => (FileMap::new(), BTreeSet::new()),
            };
            let subject = commit.message.lines().next().unwrap_or("");
            steps.push(ReplayPatch {
                base,
                theirs,
                theirs_dirs,
                author: author.to_string(),
                message: format!("Revert {subject:?}"),
            });
        }
        self.replay_onto(ns, tip, steps, timestamp_ms, dry_run)
    }

    /// Rebase the current branch onto the commit `onto` resolves to (HEAD, a branch name, or a digest):
    /// replay the branch's first-parent commits since the merge base onto `onto`, linearly. If the branch
    /// is already an ancestor of `onto` it fast-forwards; if already based on `onto` it is
    /// [`ReplayOutcome::Empty`]. Atomic and dry-runnable.
    pub fn rebase(
        &mut self,
        ns: WorkspaceId,
        onto: &str,
        timestamp_ms: u64,
        dry_run: bool,
    ) -> Result<ReplayOutcome> {
        self.authorize(ns, FacetKind::Vcs, AclRight::Write)?;
        self.authorize(ns, FacetKind::Vcs, AclRight::Advance)?;
        let onto_d = self.resolve_rev(ns, onto)?;
        let head = self.registry.head_branch(ns)?;
        let tip = self
            .registry
            .branch_tip(ns, &head)?
            .ok_or_else(|| LoomError::not_found("current branch has no commits"))?;
        if tip == onto_d {
            return Ok(ReplayOutcome::Empty);
        }
        // The branch is behind `onto`: fast-forward to it.
        if self.is_ancestor(tip, onto_d)? {
            if dry_run {
                return Ok(ReplayOutcome::Clean);
            }
            let head_ref = format!("branch/{head}");
            self.authorize_branch_update(ns, &head_ref, Some(tip), onto_d)?;
            self.registry.update_branch(ns, &head, Some(tip), onto_d)?;
            self.checkout_commit(ns, onto_d)?;
            return Ok(ReplayOutcome::Replayed(onto_d));
        }
        // First-parent commits on the branch not yet contained in `onto`, oldest first.
        let mut to_replay = Vec::new();
        let mut cur = Some(tip);
        while let Some(c) = cur {
            if self.is_ancestor(c, onto_d)? {
                break;
            }
            to_replay.push(c);
            cur = self.get_commit(c)?.parents.first().copied();
        }
        to_replay.reverse();
        if to_replay.is_empty() {
            return Ok(ReplayOutcome::Empty);
        }
        self.authorize(ns, FacetKind::Vcs, AclRight::Admin)?;
        let mut steps = Vec::with_capacity(to_replay.len());
        for c in to_replay {
            let commit = self.get_commit(c)?;
            steps.push(self.pick_patch(c, commit.author.clone(), commit.message.clone())?);
        }
        self.replay_onto(ns, Some(onto_d), steps, timestamp_ms, dry_run)
    }

    /// Collapse every commit after `onto` up to the current branch tip into one commit whose tree is the
    /// tip's tree and whose parent is the commit `onto` resolves to, with the given `author`/`message`.
    /// `onto` must be an ancestor of the tip (`INVALID_ARGUMENT` otherwise) and not the tip itself. The
    /// branch tip is rewritten by compare-and-swap; the working tree is unchanged (its content already
    /// matches the tip). Returns the new commit.
    pub fn squash(
        &mut self,
        ns: WorkspaceId,
        onto: &str,
        author: &str,
        message: &str,
        timestamp_ms: u64,
    ) -> Result<Digest> {
        self.authorize(ns, FacetKind::Vcs, AclRight::Write)?;
        self.authorize(ns, FacetKind::Vcs, AclRight::Advance)?;
        self.authorize(ns, FacetKind::Vcs, AclRight::Admin)?;
        if self.merge_state.contains_key(&ns) {
            return Err(LoomError::new(
                Code::Conflict,
                "a merge is in progress; resolve and continue it, or abort it first",
            ));
        }
        let onto_d = self.resolve_rev(ns, onto)?;
        let head = self.registry.head_branch(ns)?;
        let tip = self
            .registry
            .branch_tip(ns, &head)?
            .ok_or_else(|| LoomError::not_found("current branch has no commits"))?;
        if tip == onto_d {
            return Err(LoomError::invalid(
                "nothing to squash: onto is already the branch tip",
            ));
        }
        if !self.is_ancestor(onto_d, tip)? {
            return Err(LoomError::invalid(
                "onto must be an ancestor of the branch tip",
            ));
        }
        let (files, dirs) = self.flatten_commit(tip)?;
        let root = self.build_subtree(&files, &dirs, "")?;
        let commit = Object::Commit(Commit {
            tree: root,
            parents: vec![onto_d],
            author: author.to_string(),
            timestamp_ms,
            message: message.to_string(),
            meta: BTreeMap::new(),
        });
        let new = self.put_object(&commit)?;
        let head_ref = format!("branch/{head}");
        self.authorize_branch_update(ns, &head_ref, Some(tip), new)?;
        self.registry.update_branch(ns, &head, Some(tip), new)?;
        Ok(new)
    }

    // ---- history queries ------------------------------------------------------------------------
}
