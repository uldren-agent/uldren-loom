//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
struct GcReclaimEvidence {
    generation: u64,
    page_count: u64,
    reference_root: Option<Digest>,
    control_root: Option<Digest>,
    index_root: Option<PageId>,
    control_fingerprint: Option<Digest>,
    derived_roots: BTreeSet<Digest>,
}

type GcInterleave<'a> = Option<&'a mut dyn FnMut(&FileStore) -> Result<()>>;

impl FileStore {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn compaction_capacity(&self) -> Result<CompactionCapacity> {
        let status = self.maintenance_status()?;
        Ok(CompactionCapacity {
            required_temp_bytes: status.physical_bytes,
            available_temp_bytes: compaction_available_bytes(&self.path)?,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn ensure_compaction_capacity(&self) -> Result<CompactionCapacity> {
        let capacity = self.compaction_capacity()?;
        if let Some(available) = capacity.available_temp_bytes
            && available < capacity.required_temp_bytes
        {
            return Err(LoomError::new(
                Code::ResourceExhausted,
                format!(
                    "loom-store: compaction requires at least {} temporary bytes in the store directory, but only {} are available",
                    capacity.required_temp_bytes, available
                ),
            ));
        }
        Ok(capacity)
    }

    /// Reclaim dead space (superseded copy-on-write B-tree nodes from every prior `put`) by rewriting
    /// the live objects into a fresh `.loom` with a single bulk-built index, then atomically replacing
    /// the file via `rename`. **Retains every stored object** (object-store-level GC); to also drop
    /// engine-unreachable objects, use [`FileStore::compact_retaining`].
    ///
    /// This is the whole-file defragmenter / fallback: it rebuilds everything in one pass and leaves a
    /// dense file. For routine reclamation prefer [`FileStore::gc_segments`], which collects only
    /// mostly-dead segments in place, at a cost proportional to the garbage rather than the total size.
    ///
    /// Native-file-only (it rebuilds into a sibling temp file and atomically `rename`s it into place);
    /// a non-file backing has no such replace, so `compact*` is cfg-gated off for wasm32. Use
    /// [`FileStore::gc_segments`], which reclaims in place over any backing, in the browser.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn compact(&mut self) -> Result<CompactStats> {
        self.compact_inner(None, None)
    }

    /// Like [`FileStore::compact`], but **drops any object whose digest is not in `retain`** -
    /// engine-reachability garbage collection. The caller supplies the live set (e.g. [`gc_loom`] via
    /// `loom_core::Loom::live_object_set`); the current reference root object is always kept regardless,
    /// so the engine can still reload after GC. Native-file-only (see [`FileStore::compact`]).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn compact_retaining(&mut self, retain: &BTreeSet<[u8; 32]>) -> Result<CompactStats> {
        self.compact_inner(Some(retain), None)
    }

    /// Incrementally reclaim space without a whole-file rewrite: relocate the live records out of each
    /// record segment that is mostly dead (live ratio below half) into fresh pages, drop the dead
    /// records there, and free the segment's pages - all in one crash-safe transaction (the
    /// region-table swap). Dense segments are left in place, so cost is proportional to the garbage,
    /// not the total size. `live` is the engine reachability set; the engine-state root object is kept
    /// regardless. Freed pages return to the free-page map for reuse, so a later write reuses them
    /// rather than growing the file; reclaiming file size by truncation is a separate step.
    pub fn gc_segments(&mut self, live: &BTreeSet<[u8; 32]>) -> Result<GcStats> {
        self.gc_segments_inner(live, None, GcSegmentBudget::unlimited(), true, None, None)
    }

    pub fn gc_validated_segments(&mut self, budget: GcSegmentBudget) -> Result<GcStats> {
        self.gc_validated_segments_impl(budget, true, None, None)
    }

    pub fn gc_validated_segments_without_tail_trim(
        &mut self,
        budget: GcSegmentBudget,
    ) -> Result<GcStats> {
        self.gc_validated_segments_impl(budget, false, None, None)
    }

    #[cfg(test)]
    pub(crate) fn gc_validated_segments_with_pre_reclaim_interleave(
        &mut self,
        budget: GcSegmentBudget,
        mut interleave: impl FnMut(&FileStore) -> Result<()>,
    ) -> Result<GcStats> {
        self.gc_validated_segments_impl(budget, true, Some(&mut interleave), None)
    }

    #[cfg(test)]
    pub(crate) fn gc_validated_segments_with_read_phase_interleave(
        &mut self,
        budget: GcSegmentBudget,
        mut interleave: impl FnMut(&FileStore) -> Result<()>,
    ) -> Result<GcStats> {
        self.gc_validated_segments_impl(budget, true, None, Some(&mut interleave))
    }

    fn gc_validated_segments_impl(
        &mut self,
        budget: GcSegmentBudget,
        trim_tail: bool,
        pre_reclaim_interleave: GcInterleave<'_>,
        read_phase_interleave: GcInterleave<'_>,
    ) -> Result<GcStats> {
        let epoch = self
            .active_reachability_mark_epoch()?
            .ok_or_else(|| LoomError::not_found("reachability mark epoch not found"))?;
        if !epoch.state.completed {
            return Err(LoomError::new(
                Code::Conflict,
                "reachability mark epoch is incomplete",
            ));
        }
        let status = self.maintenance_status()?;
        if status.last_validated_mark_epoch < epoch.epoch {
            return Err(LoomError::new(
                Code::Conflict,
                "reachability mark epoch is not validated",
            ));
        }
        if let Err(error) = self.validate_reachability_mark_epoch_current(&epoch) {
            if error.code == Code::Conflict {
                self.clear_reachability_mark_epoch()?;
            }
            return Err(error);
        }
        let candidates = status
            .candidate_segments
            .into_iter()
            .collect::<BTreeSet<_>>();
        if candidates.is_empty() || budget.max_segments == 0 || budget.max_pages == 0 {
            return Ok(GcStats::default());
        }
        if let Some(interleave) = pre_reclaim_interleave {
            interleave(self)?;
        }
        self.gc_segments_inner(
            &epoch.retain_set(),
            Some(&candidates),
            budget,
            trim_tail,
            Some(&epoch),
            read_phase_interleave,
        )
    }

    fn gc_segments_inner(
        &mut self,
        live: &BTreeSet<[u8; 32]>,
        eligible_segments: Option<&BTreeSet<u64>>,
        budget: GcSegmentBudget,
        trim_tail: bool,
        validated_epoch: Option<&ReachabilityMarkEpoch>,
        read_phase_interleave: GcInterleave<'_>,
    ) -> Result<GcStats> {
        let codec = self.default_codec; // re-frame relocated records per the current default
        let (evidence, keep_reference, keep_control, keep_derived) = {
            let mut inner = self.inner.lock().map_err(|_| poisoned())?;
            let control_map = self.control_map_locked(&mut inner)?;
            let evidence = self.gc_reclaim_evidence_locked(&inner, &control_map)?;
            if let Some(epoch) = validated_epoch
                && let Err(error) = self.validate_reachability_mark_epoch_evidence(&evidence, epoch)
            {
                if error.code == Code::Conflict {
                    drop(inner);
                    self.clear_reachability_mark_epoch()?;
                }
                return Err(error);
            }
            (
                evidence,
                inner.reference_root.map(|d| *d.bytes()),
                inner.control_root.map(|d| *d.bytes()),
                self.derived_payload_digests_from_control_map(&control_map)?,
            )
        };
        let index_snapshot = self.index_snapshot_from_evidence(&evidence, read_phase_interleave)?;
        let alive = |digest: &[u8; 32]| {
            live.contains(digest)
                || keep_reference.as_ref() == Some(digest)
                || keep_control.as_ref() == Some(digest)
                || keep_derived.contains(digest)
        };
        // Group index entries by record page: whether any object on the page survives, and the object
        // count (so a packed slab page, span 1, is distinguished from a single-object page that may be
        // a multi-page large run).
        let mut page_live: BTreeMap<u64, bool> = BTreeMap::new();
        let mut page_objs: BTreeMap<u64, u32> = BTreeMap::new();
        for (digest, loc) in &index_snapshot {
            let p = loc.global_page();
            *page_live.entry(p).or_insert(false) |= alive(digest);
            *page_objs.entry(p).or_insert(0) += 1;
        }
        // Resolve each page's span (slab = 1 page; a single-object page may be a large run, so read its
        // header) and accumulate per-segment live vs total *pages*. Pages, not object counts, are the
        // right basis for the decision: object counts mislead when large multi-page records dominate.
        let mut page_span_map: BTreeMap<u64, u64> = BTreeMap::new();
        let mut occupancy: BTreeMap<u64, (u64, u64)> = BTreeMap::new(); // segment -> (live_pages, total_pages)
        for (&p, &objs) in &page_objs {
            let span = if objs > 1 {
                1
            } else {
                let mut file = self.file.lock().map_err(|_| poisoned())?;
                page_span(&mut **file, p)?
            };
            page_span_map.insert(p, span);
            let e = occupancy
                .entry(p / page::PAGES_PER_SEGMENT)
                .or_insert((0, 0));
            e.1 += span;
            if page_live[&p] {
                e.0 += span;
            }
        }
        let chosen: BTreeSet<u64> =
            choose_sparse_segments_bounded(&occupancy, eligible_segments, budget)
                .into_iter()
                .collect();
        if chosen.is_empty() {
            return Ok(GcStats::default());
        }
        let mut survivors: Vec<(Digest, Vec<u8>)> = Vec::new();
        let mut dropped: Vec<[u8; 32]> = Vec::new();
        let mut pages_to_free: BTreeSet<u64> = BTreeSet::new();
        for (digest, loc) in &index_snapshot {
            if !chosen.contains(&loc.segment_id) {
                continue;
            }
            pages_to_free.insert(loc.global_page());
            if alive(digest) {
                let d = Digest::of(self.digest_algo, *digest);
                let payload = self
                    .read_indexed_payload_snapshot(loc, evidence.page_count, &d)?
                    .ok_or_else(|| corrupt("live object missing during gc"))?;
                survivors.push((d, payload));
            } else {
                dropped.push(*digest);
            }
        }

        // Phase B: one transaction - relocate survivors to fresh pages, point-update their index
        // entries, delete the dropped keys, and free the reclaimed segments' pages.
        let mut inner = self.inner.lock().map_err(|_| poisoned())?;
        let control_map = self.control_map_locked(&mut inner)?;
        let current_evidence = self.gc_reclaim_evidence_locked(&inner, &control_map)?;
        if current_evidence != evidence {
            if validated_epoch.is_some() {
                drop(inner);
                self.clear_reachability_mark_epoch()?;
            }
            return Err(LoomError::new(
                Code::Conflict,
                "store changed during segment gc",
            ));
        }
        if let Some(epoch) = validated_epoch
            && let Err(error) =
                self.validate_reachability_mark_epoch_evidence(&current_evidence, epoch)
        {
            if error.code == Code::Conflict {
                drop(inner);
                self.clear_reachability_mark_epoch()?;
            }
            return Err(error);
        }
        let new_gen = inner.generation + 1;
        self.materialize_index_locked(&mut inner)?;
        let before_page_count = evidence.page_count;
        let (roots, index_root, placements, pages_freed) = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            let mut alloc = PageAllocator::new(inner.page_count, new_gen, inner.free.clone());
            let borrowed: Vec<(Digest, &[u8], Codec)> = survivors
                .iter()
                .map(|(d, p)| (*d, p.as_slice(), codec))
                .collect();
            // Survivors are re-sealed under the current DEK as they are relocated, so GC never
            // demotes an encrypted store to plaintext frames.
            let dek = self.dek.lock().map_err(|_| poisoned())?;
            let placements = write_record_pages(&mut **file, &mut alloc, &borrowed, dek.as_ref())?;
            drop(dek);
            let touched_segments: BTreeSet<u64> =
                placements.iter().map(|(_, loc)| loc.segment_id).collect();
            let mut index_root = inner.index_root;
            for (key, loc) in &placements {
                let bound = alloc.page_count();
                index_root = Some(pagebtree::insert(
                    &mut **file,
                    DATA_START,
                    &mut alloc,
                    index_root,
                    key,
                    *loc,
                    bound,
                )?);
            }
            for key in &dropped {
                let bound = alloc.page_count();
                index_root =
                    pagebtree::delete(&mut **file, DATA_START, &mut alloc, index_root, key, bound)?;
            }
            // Free the reclaimed segments' record pages: a slab page is one page, a large record its
            // whole run. The pages were never in the seeded free list, so survivor/index writes above
            // could not have reused them.
            let mut pages_freed = 0u64;
            for &p in &pages_to_free {
                let span = page_span_map[&p];
                alloc.free(PageId(p), span);
                pages_freed += span;
            }
            let object_count = inner
                .maintenance
                .object_count
                .saturating_sub(dropped.len() as u64);
            let roots = finish_txn(
                &mut **file,
                &mut alloc,
                new_gen,
                object_count,
                index_root,
                inner.open_segment,
                keep_reference,
                keep_control,
                &inner.maintenance,
                &touched_segments,
                (
                    inner.freemap,
                    inner.region_table_root,
                    inner.maintenance_root,
                ),
                inner.encryption_meta.clone(),
                self.digest_algo,
            )?;
            (roots, index_root, placements, pages_freed)
        };

        let pages_trimmed = before_page_count.saturating_sub(roots.page_count);
        let root_page_count = roots.page_count;
        inner.generation = new_gen;
        inner.page_count = root_page_count;
        inner.index_root = index_root;
        Self::clear_index_page_cache_locked(&mut inner);
        inner.free = roots.free;
        inner.freemap = roots.freemap;
        inner.region_table_root = Some(roots.region_table_root);
        inner.maintenance_root = Some(roots.maintenance_root);
        inner.maintenance = roots.maintenance;
        for (key, loc) in &placements {
            Self::cache_locator_locked(&mut inner, *key, *loc);
        }
        for key in &dropped {
            inner.index.remove(key);
        }
        drop(inner);
        let mut stats = GcStats {
            segments_reclaimed: chosen.len() as u64,
            pages_freed,
            pages_trimmed,
            objects_relocated: survivors.len() as u64,
            objects_dropped: dropped.len() as u64,
        };
        if trim_tail && stats.pages_freed > 0 {
            stats.pages_trimmed = stats
                .pages_trimmed
                .saturating_add(self.trim_tail_free_pages()?);
        }
        Ok(stats)
    }

    pub(crate) fn trim_tail_free_pages(&mut self) -> Result<u64> {
        let mut inner = self.inner.lock().map_err(|_| poisoned())?;
        let before = inner.page_count;
        let new_gen = inner.generation + 1;
        let free = inner
            .free
            .iter()
            .map(|run| FreePageRun {
                start: run.start,
                len: run.len,
                freed_gen: new_gen.saturating_sub(REUSE_SAFE_WINDOW),
            })
            .collect::<Vec<_>>();
        let roots = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            let mut alloc =
                PageAllocator::new_with_current_free_reusable(inner.page_count, new_gen, free);
            finish_txn(
                &mut **file,
                &mut alloc,
                new_gen,
                inner.maintenance.object_count,
                inner.index_root,
                inner.open_segment,
                inner.reference_root.map(|d| *d.bytes()),
                inner.control_root.map(|d| *d.bytes()),
                &inner.maintenance,
                &BTreeSet::new(),
                (
                    inner.freemap,
                    inner.region_table_root,
                    inner.maintenance_root,
                ),
                inner.encryption_meta.clone(),
                self.digest_algo,
            )?
        };
        let trimmed = before.saturating_sub(roots.page_count);
        let root_page_count = roots.page_count;
        inner.generation = new_gen;
        inner.page_count = root_page_count;
        inner.free = roots.free;
        inner.freemap = roots.freemap;
        inner.region_table_root = Some(roots.region_table_root);
        inner.maintenance_root = Some(roots.maintenance_root);
        inner.maintenance = roots.maintenance;
        Ok(trimmed)
    }

    pub fn compact_tail_once(
        &mut self,
        max_pages: u64,
        max_objects: u64,
        max_bytes: u64,
    ) -> Result<TailCompactionStats> {
        self.compact_tail_once_impl(max_pages, max_objects, max_bytes, None)
    }

    #[cfg(test)]
    pub(crate) fn compact_tail_once_with_pre_commit_interleave(
        &mut self,
        max_pages: u64,
        max_objects: u64,
        max_bytes: u64,
        mut interleave: impl FnMut(&FileStore) -> Result<()>,
    ) -> Result<TailCompactionStats> {
        self.compact_tail_once_impl(max_pages, max_objects, max_bytes, Some(&mut interleave))
    }

    fn compact_tail_once_impl(
        &mut self,
        max_pages: u64,
        max_objects: u64,
        max_bytes: u64,
        pre_commit_interleave: GcInterleave<'_>,
    ) -> Result<TailCompactionStats> {
        if max_pages == 0 || max_objects == 0 || max_bytes == 0 {
            return Err(LoomError::new(
                Code::InvalidArgument,
                "tail compaction budgets must be nonzero",
            ));
        }
        let codec = self.default_codec;
        let evidence = {
            let mut inner = self.inner.lock().map_err(|_| poisoned())?;
            let control_map = self.control_map_locked(&mut inner)?;
            self.gc_reclaim_evidence_locked(&inner, &control_map)?
        };
        let status = self.maintenance_status()?;
        let tail_end = status
            .physical_page_count
            .saturating_sub(status.tail_free_pages);
        if tail_end == 0 || status.reusable_free_pages <= status.tail_free_pages {
            return Ok(TailCompactionStats {
                attempted: true,
                skipped: true,
                ..TailCompactionStats::default()
            });
        }
        let scan_start = tail_end.saturating_sub(max_pages);
        let index_snapshot = self.index_snapshot_from_evidence(&evidence, None)?;
        let mut page_objs: BTreeMap<u64, u32> = BTreeMap::new();
        for (_, loc) in &index_snapshot {
            *page_objs.entry(loc.global_page()).or_insert(0) += 1;
        }
        let mut selected: Vec<(Digest, RecordLoc, u64, Vec<u8>)> = Vec::new();
        let mut selected_pages = 0u64;
        let mut selected_bytes = 0u64;
        let mut sorted = index_snapshot;
        sorted.sort_by_key(|(_, loc)| std::cmp::Reverse(loc.global_page()));
        for (key, loc) in sorted {
            let page = loc.global_page();
            if page < scan_start || page >= tail_end || page_objs.get(&page) != Some(&1) {
                continue;
            }
            let span = {
                let mut file = self.file.lock().map_err(|_| poisoned())?;
                page_span(&mut **file, page)?
            };
            if selected.len() as u64 >= max_objects
                || selected_pages.saturating_add(span) > max_pages
            {
                break;
            }
            let digest = Digest::of(self.digest_algo, key);
            let payload = self
                .read_indexed_payload_snapshot(&loc, evidence.page_count, &digest)?
                .ok_or_else(|| corrupt("tail object missing during compaction"))?;
            if selected_bytes.saturating_add(payload.len() as u64) > max_bytes {
                break;
            }
            selected_pages = selected_pages.saturating_add(span);
            selected_bytes = selected_bytes.saturating_add(payload.len() as u64);
            selected.push((digest, loc, span, payload));
        }
        if selected.is_empty() {
            return Ok(TailCompactionStats {
                attempted: true,
                skipped: true,
                ..TailCompactionStats::default()
            });
        }

        if let Some(interleave) = pre_commit_interleave {
            interleave(self)?;
        }

        let mut inner = self.inner.lock().map_err(|_| poisoned())?;
        let control_map = self.control_map_locked(&mut inner)?;
        let current_evidence = self.gc_reclaim_evidence_locked(&inner, &control_map)?;
        if current_evidence != evidence {
            return Ok(TailCompactionStats {
                attempted: true,
                conflicts: 1,
                skipped: true,
                ..TailCompactionStats::default()
            });
        }
        self.materialize_index_locked(&mut inner)?;
        for (digest, loc, _, _) in &selected {
            if inner.index.get(digest.bytes()) != Some(loc) {
                return Ok(TailCompactionStats {
                    attempted: true,
                    conflicts: 1,
                    skipped: true,
                    ..TailCompactionStats::default()
                });
            }
        }
        let new_gen = inner.generation + 1;
        let before_page_count = inner.page_count;
        let keep_reference = inner.reference_root.map(|d| *d.bytes());
        let keep_control = inner.control_root.map(|d| *d.bytes());
        let selected_pages_set: BTreeSet<u64> = selected
            .iter()
            .map(|(_, loc, _, _)| loc.global_page())
            .collect();
        let (roots, index_root, placements, relocated_pages) = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            let mut alloc = PageAllocator::new_reusing_before(
                inner.page_count,
                new_gen,
                inner.free.clone(),
                scan_start,
            );
            let borrowed: Vec<(Digest, &[u8], Codec)> = selected
                .iter()
                .map(|(digest, _, _, payload)| (*digest, payload.as_slice(), codec))
                .collect();
            let dek = self.dek.lock().map_err(|_| poisoned())?;
            let placements = write_record_pages(&mut **file, &mut alloc, &borrowed, dek.as_ref())?;
            drop(dek);
            if placements
                .iter()
                .any(|(_, loc)| loc.global_page() >= scan_start)
            {
                return Ok(TailCompactionStats {
                    attempted: true,
                    skipped: true,
                    ..TailCompactionStats::default()
                });
            }
            let mut index_root = inner.index_root;
            for (key, loc) in &placements {
                let bound = alloc.page_count();
                index_root = Some(pagebtree::insert(
                    &mut **file,
                    DATA_START,
                    &mut alloc,
                    index_root,
                    key,
                    *loc,
                    bound,
                )?);
            }
            let mut relocated_pages = 0u64;
            for (_, loc, span, _) in &selected {
                alloc.free(PageId(loc.global_page()), *span);
                relocated_pages = relocated_pages.saturating_add(*span);
            }
            let touched_segments: BTreeSet<u64> = placements
                .iter()
                .map(|(_, loc)| loc.segment_id)
                .chain(
                    selected_pages_set
                        .iter()
                        .map(|page| page / page::PAGES_PER_SEGMENT),
                )
                .collect();
            let roots = finish_txn(
                &mut **file,
                &mut alloc,
                new_gen,
                inner.maintenance.object_count,
                index_root,
                inner.open_segment,
                keep_reference,
                keep_control,
                &inner.maintenance,
                &touched_segments,
                (
                    inner.freemap,
                    inner.region_table_root,
                    inner.maintenance_root,
                ),
                inner.encryption_meta.clone(),
                self.digest_algo,
            )?;
            (roots, index_root, placements, relocated_pages)
        };
        let root_page_count = roots.page_count;
        inner.generation = new_gen;
        inner.page_count = root_page_count;
        inner.index_root = index_root;
        Self::clear_index_page_cache_locked(&mut inner);
        inner.free = roots.free;
        inner.freemap = roots.freemap;
        inner.region_table_root = Some(roots.region_table_root);
        inner.maintenance_root = Some(roots.maintenance_root);
        inner.maintenance = roots.maintenance;
        for (key, loc) in &placements {
            Self::cache_locator_locked(&mut inner, *key, *loc);
        }
        drop(inner);
        let truncated_pages = before_page_count.saturating_sub(root_page_count);
        let trimmed = self.trim_tail_free_pages()?;
        Ok(TailCompactionStats {
            attempted: true,
            relocated_objects: placements.len() as u64,
            relocated_pages,
            relocated_bytes: selected_bytes,
            truncated_pages: truncated_pages.saturating_add(trimmed),
            conflicts: 0,
            skipped: false,
        })
    }

    fn index_snapshot_from_evidence(
        &self,
        evidence: &GcReclaimEvidence,
        mut read_phase_interleave: GcInterleave<'_>,
    ) -> Result<Vec<([u8; 32], RecordLoc)>> {
        let Some(root) = evidence.index_root else {
            return Ok(Vec::new());
        };
        let mut interleaved = false;
        pagebtree::load_all_with_page_reader(root, evidence.page_count, |page| {
            let mut buf = [0u8; PAGE_SIZE as usize];
            {
                let mut file = self.file.lock().map_err(|_| poisoned())?;
                read_exact_at(&mut **file, page.offset(DATA_START), &mut buf)
                    .map_err(|_| corrupt("truncated btree node page"))?;
            }
            if !interleaved && let Some(interleave) = read_phase_interleave.as_mut() {
                interleaved = true;
                interleave(self)?;
            }
            Ok(buf)
        })
    }

    fn gc_reclaim_evidence_locked(
        &self,
        inner: &Inner,
        control_map: &BTreeMap<Vec<u8>, Vec<u8>>,
    ) -> Result<GcReclaimEvidence> {
        Ok(GcReclaimEvidence {
            generation: inner.generation,
            page_count: inner.page_count,
            reference_root: inner.reference_root,
            control_root: inner.control_root,
            index_root: inner.index_root,
            control_fingerprint: self.control_reachability_fingerprint_from_map(control_map),
            derived_roots: self
                .derived_payload_digests_from_control_map(control_map)?
                .into_iter()
                .map(|bytes| Digest::of(self.digest_algo, bytes))
                .collect(),
        })
    }

    fn validate_reachability_mark_epoch_evidence(
        &self,
        evidence: &GcReclaimEvidence,
        epoch: &ReachabilityMarkEpoch,
    ) -> Result<()> {
        if evidence.reference_root != epoch.reference_root {
            return Err(LoomError::new(
                Code::Conflict,
                "reachability mark epoch reference root changed",
            ));
        }
        if evidence.control_fingerprint != epoch.control_fingerprint {
            return Err(LoomError::new(
                Code::Conflict,
                "reachability mark epoch control root changed",
            ));
        }
        if evidence.derived_roots != epoch.derived_roots {
            return Err(LoomError::new(
                Code::Conflict,
                "reachability mark epoch derived roots changed",
            ));
        }
        Ok(())
    }

    fn control_map_locked(&self, inner: &mut Inner) -> Result<BTreeMap<Vec<u8>, Vec<u8>>> {
        let Some(root) = inner.control_root else {
            return Ok(BTreeMap::new());
        };
        let Some(loc) = self.lookup_loc_locked(inner, root.bytes())? else {
            return Err(corrupt("control-plane root object missing"));
        };
        let bytes = self
            .read_indexed_payload_snapshot(&loc, inner.page_count, &root)?
            .ok_or_else(|| corrupt("control-plane root object missing"))?;
        crate::record_io::decode_control_map(&bytes)
    }

    fn read_indexed_payload_snapshot(
        &self,
        loc: &RecordLoc,
        page_count: u64,
        digest: &Digest,
    ) -> Result<Option<Vec<u8>>> {
        let global = loc.global_page();
        if global >= page_count {
            return Err(corrupt("record locator past the page array"));
        }
        let mut file = self.file.lock().map_err(|_| poisoned())?;
        let dek = self.dek.lock().map_err(|_| poisoned())?;
        let mut first = [0u8; PAGE_SIZE as usize];
        read_exact_at(&mut **file, PageId(global).offset(DATA_START), &mut first)
            .map_err(io_err)?;
        let payload = match first[0] {
            record::SLAB_MAGIC => {
                let rec = record::read_slab_slot(&first, loc.slot)
                    .ok_or_else(|| corrupt("bad slab slot on read"))?;
                decode_record(rec, digest, dek.as_ref(), self.digest_algo)?
            }
            record::LARGE_MAGIC => {
                let blob_len = record::large_blob_len(&first)
                    .ok_or_else(|| corrupt("bad large record header"))?;
                let pages = record::large_pages(blob_len);
                if global + pages > page_count {
                    return Err(corrupt("large record run past the page array"));
                }
                let mut buf = vec![0u8; (pages * PAGE_SIZE) as usize];
                read_exact_at(&mut **file, PageId(global).offset(DATA_START), &mut buf)
                    .map_err(io_err)?;
                let rec = record::decode_large(&buf)
                    .ok_or_else(|| corrupt("large record parse failure"))?;
                decode_record(rec, digest, dek.as_ref(), self.digest_algo)?
            }
            _ => return Err(corrupt("bad record page magic on read")),
        };
        Ok(Some(payload))
    }

    /// Rotate an encrypted store's key material by re-sealing every object: read each object under the
    /// current (unlocked) DEK, then re-seal it under `new_session`'s DEK/suite while rewriting the file,
    /// recording `new_encryption_meta` in the compacted superblock. This is distinct
    /// from the cheap `rekey` (which only re-wraps the *same* DEK under a new passphrase). It is what
    /// makes DEK rotation and AEAD-suite rotation possible, at the cost of rewriting the whole store.
    /// Native-file-only (it reuses the compaction rewrite + atomic rename); the store stays unlocked
    /// under the new session afterward. The plaintext digests are unchanged, so object identity, the
    /// index, and conformance vectors are unaffected - only the sealed bytes change.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn rekey_reseal(
        &mut self,
        new_encryption_meta: Vec<u8>,
        new_session: DekSession,
    ) -> Result<CompactStats> {
        if !self.is_encrypted() {
            return Err(LoomError::new(
                Code::Unsupported,
                "loom-store: rekey-reseal on an unencrypted store",
            ));
        }
        if !self.is_unlocked() {
            return Err(LoomError::new(
                Code::E2eLocked,
                "loom-store: rekey-reseal requires the store to be unlocked",
            ));
        }
        self.compact_inner(None, Some((new_encryption_meta, new_session)))
    }

    /// Rewrite the store into a fresh, dense file. `retain`, when set, drops any object outside the live
    /// set (engine-reachability GC). `reseal`, when set, re-seals every surviving object under a new DEK
    /// session and records the new `encryption_meta`; otherwise objects are
    /// re-framed under the current DEK and the existing `encryption_meta` rides through unchanged.
    #[cfg(not(target_arch = "wasm32"))]
    fn compact_inner(
        &mut self,
        retain: Option<&BTreeSet<[u8; 32]>>,
        reseal: Option<(Vec<u8>, DekSession)>,
    ) -> Result<CompactStats> {
        self.ensure_compaction_capacity()?;
        // The current engine-state root object MUST survive even under a retain filter (the engine
        // reloads from it); never let a caller's live set accidentally drop it.
        let (before, keep_reference, keep_control) = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            (
                DATA_START + inner.page_count * PAGE_SIZE,
                inner.reference_root.map(|d| *d.bytes()),
                inner.control_root.map(|d| *d.bytes()),
            )
        };
        let keep_derived = self.derived_payload_digests()?;
        let path = self.path.clone();
        let tmp = compact_tmp_path(&path);
        let _ = std::fs::remove_file(&tmp); // discard any stale temp from a previously aborted compaction
        let codec = self.default_codec; // re-frame surviving objects per the current default

        {
            let mut out = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp)
                .map_err(io_err)?;
            // Reserve the header (two superblock slots + the journal slot); the superblocks are written
            // last, once the roots are known.
            write_at(&mut out, 0, &vec![0u8; DATA_START as usize]).map_err(io_err)?;

            let (keys, enc_meta): (Vec<[u8; 32]>, Option<Vec<u8>>) = {
                let mut i = self.inner.lock().map_err(|_| poisoned())?;
                self.materialize_index_locked(&mut i)?;
                (i.index.keys().copied().collect(), i.encryption_meta.clone())
            };
            // Read each retained object back through `get` (digest-verified) and collect it for packing.
            let mut retained: Vec<(Digest, Vec<u8>, Codec)> = Vec::with_capacity(keys.len());
            for k in &keys {
                if let Some(set) = retain
                    && !set.contains(k)
                    && keep_reference.as_ref() != Some(k)
                    && keep_control.as_ref() != Some(k)
                    && !keep_derived.contains(k)
                {
                    continue; // unreachable garbage: drop it
                }
                let digest = Digest::from_blake3_bytes(*k);
                let payload = self
                    .get(&digest)?
                    .ok_or_else(|| corrupt("indexed object missing during compaction"))?;
                retained.push((digest, payload, codec));
            }
            // Compaction rewrites into a fresh file: no prior free list, so the allocator extends from
            // page 0. Pack records, bulk-build the index, then commit via two identical superblocks.
            let mut alloc = PageAllocator::new(0, 0, Vec::new());
            let borrowed: Vec<(Digest, &[u8], Codec)> = retained
                .iter()
                .map(|(d, p, c)| (*d, p.as_slice(), *c))
                .collect();
            // Objects were read (and decrypted) into `retained` above under the *current* DEK. Seal them
            // as they are packed: under the new session for a rekey-reseal, otherwise under the
            // current DEK (plain compaction preserves the encrypted invariant). The superblock carries
            // the new `encryption_meta` for a reseal, or the existing one unchanged otherwise.
            let superblock_meta = match &reseal {
                Some((new_meta, _)) => Some(new_meta.clone()),
                None => enc_meta,
            };
            let mut entries = match &reseal {
                Some((_, new_session)) => {
                    write_record_pages(&mut out, &mut alloc, &borrowed, Some(new_session))?
                }
                None => {
                    let dek = self.dek.lock().map_err(|_| poisoned())?;
                    write_record_pages(&mut out, &mut alloc, &borrowed, dek.as_ref())?
                }
            };
            entries.sort_unstable_by_key(|e| e.0); // build_packed needs ascending, unique keys
            let index_root = pagebtree::build_packed(&mut out, DATA_START, &mut alloc, &entries)?;
            let maintenance_page = alloc.extend(1);
            let rt_page = alloc.extend(1);
            let page_count = alloc.page_count();
            let maintenance = MaintenanceState {
                generation: 0,
                object_count: entries.len() as u64,
                object_count_known: true,
                physical_page_count: page_count,
                reusable_free_pages: 0,
                candidate_dead_pages: 0,
                last_validated_mark_epoch: 0,
                touched_segments: Vec::new(),
                candidate_segments: Vec::new(),
                segment_overflow: false,
            };
            maintenance::write_maintenance(&mut out, maintenance_page, &maintenance)?;
            let region = RegionTable {
                page_size: PAGE_SIZE,
                index_root,
                freemap_root: None, // a freshly compacted file has no dead pages
                maintenance_root: Some(maintenance_page),
                open_segment: 0,
            };
            let mut rt_buf = [0u8; PAGE_SIZE as usize];
            rt_buf[..page::REGION_TABLE_LEN].copy_from_slice(&region.encode());
            write_at(&mut out, rt_page.offset(DATA_START), &rt_buf).map_err(io_err)?;
            out.sync_all().map_err(io_err)?;
            let sb = Superblock {
                generation: 0,
                page_count,
                digest_algo: self.digest_algo, // identity profile is immutable across compaction/rekey
                region_table: Some(rt_page),
                reference: keep_reference,
                control: keep_control,
                encryption: superblock_meta, // current meta on compaction; the new meta on a rekey-reseal
            }
            .encode();
            write_at(&mut out, 0, &sb).map_err(io_err)?;
            write_at(&mut out, SLOT_SIZE, &sb).map_err(io_err)?;
            out.sync_all().map_err(io_err)?;
        }

        // Atomic replace: rename is atomic on POSIX; a crash here leaves either the old or new file
        // wholly intact. fsync the directory so the rename itself is durable.
        std::fs::rename(&tmp, &path).map_err(io_err)?;
        sync_parent_dir(&path);
        // Carry the unlocked DEK session across the reopen so the handle stays usable: on a rekey-reseal
        // that is the *new* session the file was re-sealed under; on a plain compaction it is the
        // existing session moved through (a freshly opened handle is otherwise locked).
        let session = match reseal {
            Some((_, new_session)) => Some(new_session),
            None => self.dek.lock().map_err(|_| poisoned())?.take(),
        };
        *self = FileStore::open(&path)?;
        *self.dek.lock().map_err(|_| poisoned())? = session;
        let after = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            DATA_START + inner.page_count * PAGE_SIZE
        };
        Ok(CompactStats { before, after })
    }
}

#[cfg(all(not(target_arch = "wasm32"), unix))]
fn compaction_available_bytes(path: &std::path::Path) -> Result<Option<u64>> {
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let stats = nix::sys::statvfs::statvfs(dir)
        .map_err(|e| LoomError::new(Code::Io, format!("statvfs compaction directory: {e}")))?;
    let blocks = stats.blocks_available() as u64;
    let fragment_size = stats.fragment_size() as u64;
    Ok(Some(blocks.saturating_mul(fragment_size)))
}

#[cfg(all(not(target_arch = "wasm32"), not(unix)))]
fn compaction_available_bytes(_path: &std::path::Path) -> Result<Option<u64>> {
    Ok(None)
}
