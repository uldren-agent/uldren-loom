use super::*;

impl<S: ObjectStore> Loom<S> {
    /// Merge `from_branch` into the current `HEAD` branch (3-way, file granularity). Fast-forwards
    /// when possible; otherwise computes the merge base and merges per path, reporting conflicting
    /// paths if any (no commit in that case).
    pub fn merge(
        &mut self,
        ns: WorkspaceId,
        from_branch: &str,
        author: &str,
        timestamp_ms: u64,
    ) -> Result<MergeOutcome> {
        self.merge_inner(ns, from_branch, author, timestamp_ms, false)
    }

    /// Like [`Self::merge`] but reconciles tables at **cell** granularity: two branches
    /// editing different columns of the same row auto-merge instead of conflicting. Opt-in; the default
    /// [`Self::merge`] is row-level.
    pub fn merge_cell_level(
        &mut self,
        ns: WorkspaceId,
        from_branch: &str,
        author: &str,
        timestamp_ms: u64,
    ) -> Result<MergeOutcome> {
        self.merge_inner(ns, from_branch, author, timestamp_ms, true)
    }

    fn merge_inner(
        &mut self,
        ns: WorkspaceId,
        from_branch: &str,
        author: &str,
        timestamp_ms: u64,
        cell_level: bool,
    ) -> Result<MergeOutcome> {
        if self.merge_state.contains_key(&ns) {
            return Err(LoomError::new(
                Code::Conflict,
                "a merge is already in progress; resolve and continue it, or abort it first",
            ));
        }
        let head = self.registry.head_branch(ns)?;
        self.authorize_ref(ns, &format!("branch/{head}"), AclRight::Merge)?;
        self.authorize_ref(ns, &format!("branch/{from_branch}"), AclRight::Read)?;
        let ours = self
            .registry
            .branch_tip(ns, &head)?
            .ok_or_else(|| LoomError::not_found("current branch has no commits"))?;
        let theirs = self
            .registry
            .branch_tip(ns, from_branch)?
            .ok_or_else(|| LoomError::not_found(format!("branch {from_branch:?}")))?;

        if ours == theirs || self.is_ancestor(theirs, ours)? {
            return Ok(MergeOutcome::UpToDate);
        }
        if self.is_ancestor(ours, theirs)? {
            let head_ref = format!("branch/{head}");
            self.authorize_branch_update(ns, &head_ref, Some(ours), theirs)?;
            self.registry.update_branch(ns, &head, Some(ours), theirs)?;
            self.checkout_commit(ns, theirs)?;
            return Ok(MergeOutcome::FastForward(theirs));
        }

        // The merge base. When several maximal common ancestors exist (a crisscross history) we do
        // NOT pick one arbitrarily; we recursively fold the whole base set into a single synthetic
        // "virtual base", removing the spurious conflicts an arbitrary pick would otherwise produce.
        let bases = self.merge_base_set(ours, theirs)?;
        let (base_files, _base_dirs) = self.reduce_bases(&bases)?;
        let (our_files, our_dirs) = self.flatten_commit(ours)?;
        let (their_files, their_dirs) = self.flatten_commit(theirs)?;

        // Merged directory set: the union of both sides keeps empty directories from either branch.
        let mut merged_dirs = our_dirs;
        merged_dirs.extend(their_dirs);
        // The per-slot 3-way merge seam (files whole-slot, tables row/cell-level, default whole-slot).
        let (merged, unresolved) =
            self.resolve_three_way(ns, &base_files, &our_files, &their_files, cell_level)?;
        if !unresolved.is_empty() {
            // Enter an in-progress merge: stage the auto-merged result, record the structured
            // conflicts, snapshot the pre-merge tree for abort, and leave the branch tip untouched
            // until merge_continue. No commit is created here.
            return self.enter_merge_conflict_state(
                ns,
                ours,
                theirs,
                from_branch,
                merged,
                &merged_dirs,
                &base_files,
                &our_files,
                &their_files,
                unresolved,
            );
        }

        let staged: WorkTree = merged;
        let root = self.build_subtree(&staged, &merged_dirs, "")?;
        let commit = Object::Commit(Commit {
            tree: root,
            parents: vec![ours, theirs],
            author: author.to_string(),
            timestamp_ms,
            message: format!("merge {from_branch}"),
            meta: BTreeMap::new(),
        });
        let digest = self.put_object(&commit)?;
        let head_ref = format!("branch/{head}");
        self.authorize_branch_update(ns, &head_ref, Some(ours), digest)?;
        self.registry.update_branch(ns, &head, Some(ours), digest)?;
        self.checkout_commit(ns, digest)?;
        Ok(MergeOutcome::Merged(digest))
    }

    /// Record an in-progress merge: stage the auto-merged tree (with whole-file conflict markers for
    /// text-file conflicts), persist the structured conflict records plus a pre-merge snapshot for
    /// abort, and return the conflict path list. The branch tip is not moved.
    #[allow(clippy::too_many_arguments)]
    fn enter_merge_conflict_state(
        &mut self,
        ns: WorkspaceId,
        ours: Digest,
        theirs: Digest,
        from_branch: &str,
        merged: WorkTree,
        merged_dirs: &BTreeSet<String>,
        base_files: &FileMap,
        our_files: &FileMap,
        their_files: &FileMap,
        unresolved: Vec<String>,
    ) -> Result<MergeOutcome> {
        // Snapshot the working tree as it stood before the merge so merge_abort can restore it exactly.
        let pre_work = self.work.get(&ns).cloned().unwrap_or_default();
        let pre_dirs = self.dirs.get(&ns).cloned().unwrap_or_default();

        let mut staged = merged;
        let mut conflicts = Vec::with_capacity(unresolved.len());
        for path in &unresolved {
            let base = base_files.get(path).copied();
            let ours_slot = our_files.get(path).copied();
            let theirs_slot = their_files.get(path).copied();
            // Present a text-file modify/modify conflict as whole-file markers for human or tool
            // editing. Binary files and table/stream slots keep `ours`; the structured record below is
            // the source of truth a merge tool resolves through in every case.
            if let (Some(StagedEntry::File(of)), Some(StagedEntry::File(tf))) =
                (ours_slot, theirs_slot)
                && let Some(marker) = self.conflict_marker_blob(of, tf)?
            {
                let content_addr = self.store_content(ns, &marker)?;
                staged.insert(
                    path.clone(),
                    StagedEntry::File(StagedFile {
                        content_addr,
                        mode: of.mode,
                    }),
                );
            }
            conflicts.push(MergeConflict {
                path: path.clone(),
                base,
                ours: ours_slot,
                theirs: theirs_slot,
            });
        }

        self.work.insert(ns, staged);
        self.dirs.insert(ns, merged_dirs.clone());
        // The working tree changed to the auto-merged (conflicted) state, so ephemeral caches over
        // backing maps in this workspace are stale; drop them. The clean-merge and
        // fast-forward paths drop via `checkout_commit`.
        self.drop_ephemeral_caches(ns);
        self.merge_state.insert(
            ns,
            MergeInProgress {
                other_parent: theirs,
                our_head: ours,
                message: format!("merge {from_branch}"),
                conflicts,
                pre_work,
                pre_dirs,
            },
        );
        Ok(MergeOutcome::Conflicts(unresolved))
    }

    /// Build a whole-file conflict-marker blob for a text modify/modify conflict, or `None` when either
    /// side is not valid UTF-8 (binary content is left to the structured record, not markered).
    fn conflict_marker_blob(
        &self,
        ours: StagedFile,
        theirs: StagedFile,
    ) -> Result<Option<Vec<u8>>> {
        let ob = self.load_content(ours.content_addr)?;
        let tb = self.load_content(theirs.content_addr)?;
        if std::str::from_utf8(&ob).is_err() || std::str::from_utf8(&tb).is_err() {
            return Ok(None);
        }
        let mut out = Vec::with_capacity(ob.len() + tb.len() + 48);
        out.extend_from_slice(b"<<<<<<< ours\n");
        out.extend_from_slice(&ob);
        if !ob.ends_with(b"\n") {
            out.push(b'\n');
        }
        out.extend_from_slice(b"=======\n");
        out.extend_from_slice(&tb);
        if !tb.ends_with(b"\n") {
            out.push(b'\n');
        }
        out.extend_from_slice(b">>>>>>> theirs\n");
        Ok(Some(out))
    }

    /// Whether `ns` has a conflicted merge awaiting [`Self::merge_continue`] or [`Self::merge_abort`].
    pub fn merge_in_progress(&self, ns: WorkspaceId) -> Result<bool> {
        self.authorize(ns, FacetKind::Vcs, AclRight::Read)?;
        Ok(self.merge_state.contains_key(&ns))
    }

    /// The still-unresolved conflict paths of the in-progress merge in `ns`, in path order. Empty when
    /// no merge is in progress or every conflict has been resolved.
    pub fn merge_conflicts(&self, ns: WorkspaceId) -> Result<Vec<String>> {
        self.authorize(ns, FacetKind::Vcs, AclRight::Read)?;
        Ok(self
            .merge_state
            .get(&ns)
            .map(|m| m.conflicts.iter().map(|c| c.path.clone()).collect())
            .unwrap_or_default())
    }

    /// Settle one conflicted path of the in-progress merge in `ns`, applying the chosen slot to the
    /// working tree and removing it from the unresolved set. `Working` accepts whatever is currently
    /// staged at the path (a hand-merged file, edited markers, or a deletion). Errors with
    /// `INVALID_ARGUMENT` if no merge is in progress or the path is not an unresolved conflict.
    pub fn merge_resolve(
        &mut self,
        ns: WorkspaceId,
        path: &str,
        resolution: ConflictResolution,
    ) -> Result<()> {
        self.authorize(ns, FacetKind::Vcs, AclRight::Merge)?;
        let path = normalize_path(path)?;
        let m = self
            .merge_state
            .get_mut(&ns)
            .ok_or_else(|| LoomError::invalid("no merge in progress"))?;
        let idx = m
            .conflicts
            .iter()
            .position(|c| c.path == path)
            .ok_or_else(|| LoomError::invalid(format!("{path:?} is not an unresolved conflict")))?;
        let chosen = match resolution {
            ConflictResolution::Ours => m.conflicts[idx].ours,
            ConflictResolution::Theirs => m.conflicts[idx].theirs,
            ConflictResolution::Working => self.work.get(&ns).and_then(|w| w.get(&path).copied()),
        };
        m.conflicts.remove(idx);
        let wt = self.work.entry(ns).or_default();
        match chosen {
            Some(slot) => {
                wt.insert(path, slot);
            }
            None => {
                wt.remove(&path);
            }
        }
        Ok(())
    }

    /// Abandon the in-progress merge in `ns`: restore the pre-merge working tree exactly and clear the
    /// merge state. The branch tip was never moved. Errors `INVALID_ARGUMENT` if no merge is in progress.
    pub fn merge_abort(&mut self, ns: WorkspaceId) -> Result<()> {
        self.authorize(ns, FacetKind::Vcs, AclRight::Merge)?;
        let m = self
            .merge_state
            .remove(&ns)
            .ok_or_else(|| LoomError::invalid("no merge in progress"))?;
        self.work.insert(ns, m.pre_work);
        self.dirs.insert(ns, m.pre_dirs);
        // The working tree was restored to its pre-merge state; drop stale ephemeral caches.
        self.drop_ephemeral_caches(ns);
        Ok(())
    }

    /// Finish the in-progress merge in `ns`: require every conflict resolved, then record a two-parent
    /// merge commit over the current working tree and advance the branch with a compare-and-swap from
    /// the recorded `ours` tip. Errors `CONFLICT` if conflicts remain, `INVALID_ARGUMENT` if no merge is
    /// in progress, and `CAS_MISMATCH` if the branch moved since the merge began.
    pub fn merge_continue(
        &mut self,
        ns: WorkspaceId,
        author: &str,
        timestamp_ms: u64,
    ) -> Result<Digest> {
        self.authorize(ns, FacetKind::Vcs, AclRight::Merge)?;
        let m = self
            .merge_state
            .get(&ns)
            .ok_or_else(|| LoomError::invalid("no merge in progress"))?;
        if !m.conflicts.is_empty() {
            let paths: Vec<&str> = m.conflicts.iter().map(|c| c.path.as_str()).collect();
            return Err(LoomError::new(
                Code::Conflict,
                format!("unresolved merge conflicts: {}", paths.join(", ")),
            ));
        }
        let our_head = m.our_head;
        let other_parent = m.other_parent;
        let message = m.message.clone();

        let head = self.registry.head_branch(ns)?;
        let files = self.work.get(&ns).cloned().unwrap_or_default();
        let dirs = self.dirs.get(&ns).cloned().unwrap_or_default();
        let root = self.build_subtree(&files, &dirs, "")?;
        let commit = Object::Commit(Commit {
            tree: root,
            parents: vec![our_head, other_parent],
            author: author.to_string(),
            timestamp_ms,
            message,
            meta: BTreeMap::new(),
        });
        let digest = self.put_object(&commit)?;
        let head_ref = format!("branch/{head}");
        self.authorize_branch_update(ns, &head_ref, Some(our_head), digest)?;
        self.registry
            .update_branch(ns, &head, Some(our_head), digest)?;
        self.checkout_commit(ns, digest)?;
        self.merge_state.remove(&ns);
        Ok(digest)
    }

    /// The merge base of two commits as a single representative commit. For a crisscross history
    /// with several maximal common ancestors this returns the deterministic (lexicographically-least)
    /// one - useful for display (e.g. diff base). Merging itself does NOT use this single pick; it
    /// reduces the full [`Self::merge_base_set`] via `Self::reduce_bases`.
    pub fn merge_base(&self, a: Digest, b: Digest) -> Result<Option<Digest>> {
        Ok(self.merge_base_set(a, b)?.into_iter().min())
    }

    /// The full **merge-base set** of two commits: every common ancestor that is not itself an
    /// ancestor of another common ancestor (the maximal common ancestors). Exactly one entry for a
    /// simple history; several for a crisscross.
    pub fn merge_base_set(&self, a: Digest, b: Digest) -> Result<Vec<Digest>> {
        let aa = self.ancestors(a)?;
        let bb = self.ancestors(b)?;
        let common: Vec<Digest> = aa.intersection(&bb).copied().collect();
        if common.is_empty() {
            return Ok(Vec::new());
        }
        let mut dominated: BTreeSet<Digest> = BTreeSet::new();
        for &c in &common {
            for &other in &self.ancestors(c)? {
                if other != c && common.contains(&other) {
                    dominated.insert(other);
                }
            }
        }
        Ok(common
            .into_iter()
            .filter(|c| !dominated.contains(c))
            .collect())
    }

    /// Reduce a merge-base set to a single **virtual base** file map (recursive/ORT-style).
    /// One base means that commit's files. Several means fold them pairwise: each step 3-way-merges the next
    /// base onto the accumulated virtual base, using the (recursively reduced) merge base *between
    /// the already-folded bases and the next one* as its base. A conflict inside the virtual base
    /// never aborts the real merge; it is resolved deterministically (the lexicographically-least
    /// side) so the result is reproducible across engines. Empty set means an empty map (unrelated roots).
    pub(crate) fn reduce_bases(&self, bases: &[Digest]) -> Result<(FileMap, BTreeSet<String>)> {
        let mut sorted: Vec<Digest> = bases.to_vec();
        sorted.sort();
        sorted.dedup();
        let Some((&first, rest)) = sorted.split_first() else {
            return Ok((FileMap::new(), BTreeSet::new()));
        };
        let (mut acc_files, mut acc_dirs) = self.flatten_commit(first)?;
        let mut folded: Vec<Digest> = vec![first];
        for &next in rest {
            // The base for this fold is the merge base between everything folded so far and `next`,
            // itself recursively reduced (this is what makes the construction reproducible).
            let mut inner: BTreeSet<Digest> = BTreeSet::new();
            for &f in &folded {
                for b in self.merge_base_set(f, next)? {
                    inner.insert(b);
                }
            }
            let inner_vec: Vec<Digest> = inner.into_iter().collect();
            let (inner_files, _) = self.reduce_bases(&inner_vec)?;
            let (next_files, next_dirs) = self.flatten_commit(next)?;
            let (mut merged, conflicts) =
                three_way_merge_files(&inner_files, &acc_files, &next_files);
            for path in conflicts {
                // Deterministic resolution: keep the lexicographically-least value, or whichever
                // side has one. A virtual base must always resolve, so we never abort here.
                let pick = match (acc_files.get(&path), next_files.get(&path)) {
                    (Some(&o), Some(&t)) => Some(o.min(t)),
                    (Some(&o), None) => Some(o),
                    (None, Some(&t)) => Some(t),
                    (None, None) => None,
                };
                match pick {
                    Some(v) => {
                        merged.insert(path, v);
                    }
                    None => {
                        merged.remove(&path);
                    }
                }
            }
            acc_files = merged;
            acc_dirs.extend(next_dirs);
            folded.push(next);
        }
        Ok((acc_files, acc_dirs))
    }
}
