//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
struct GcReclaimEvidence {
    generation: u64,
    page_count: u64,
    reference_root: Option<Digest>,
    control_root: Option<Digest>,
    index_root: Option<PageId>,
    overlay_root: Option<PageId>,
    control_fingerprint: Option<Digest>,
    derived_roots: BTreeSet<Digest>,
}

type GcInterleave<'a> = Option<&'a mut dyn FnMut(&FileStore) -> Result<()>>;
type GcDeadline = Option<std::time::Instant>;
type IndexScanEntry = ([u8; 32], RecordLoc);
type IndexScanState = (pagebtree::ScanCursor, Vec<IndexScanEntry>);
type FullCompactionSnapshot = (Vec<[u8; 32]>, Option<Vec<u8>>, Option<PageId>, u64);
const INDEX_SCAN_STATE_MAGIC: &[u8; 8] = b"LIDXCUR1";

fn check_gc_deadline(deadline: GcDeadline) -> Result<()> {
    if deadline.is_some_and(|deadline| std::time::Instant::now() >= deadline) {
        return Err(LoomError::new(
            Code::ResourceExhausted,
            "maintenance work budget exhausted",
        ));
    }
    Ok(())
}

fn lock_until<'a, T>(
    mutex: &'a std::sync::Mutex<T>,
    deadline: GcDeadline,
) -> Result<std::sync::MutexGuard<'a, T>> {
    match deadline {
        None => mutex.lock().map_err(|_| poisoned()),
        Some(deadline) => loop {
            match mutex.try_lock() {
                Ok(guard) => return Ok(guard),
                Err(std::sync::TryLockError::Poisoned(_)) => return Err(poisoned()),
                Err(std::sync::TryLockError::WouldBlock) => {
                    check_gc_deadline(Some(deadline))?;
                    std::thread::yield_now();
                }
            }
        },
    }
}

fn put_optional_digest_bytes(out: &mut Vec<u8>, digest: Option<Digest>) {
    match digest {
        Some(digest) => {
            out.push(1);
            out.extend_from_slice(digest.bytes());
        }
        None => out.push(0),
    }
}

fn put_optional_page_bytes(out: &mut Vec<u8>, page: Option<PageId>) {
    match page {
        Some(page) => {
            out.push(1);
            out.extend_from_slice(&page.0.to_le_bytes());
        }
        None => out.push(0),
    }
}

fn encode_index_scan_state(
    evidence_key: [u8; 32],
    cursor: &pagebtree::ScanCursor,
    entries: &[([u8; 32], RecordLoc)],
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(INDEX_SCAN_STATE_MAGIC);
    out.extend_from_slice(&evidence_key);
    out.extend_from_slice(&(cursor.stack.len() as u32).to_le_bytes());
    for op in &cursor.stack {
        match op {
            pagebtree::ScanOp::Visit { page, depth } => {
                out.push(0);
                out.extend_from_slice(&page.0.to_le_bytes());
                out.extend_from_slice(&(*depth as u32).to_le_bytes());
            }
            pagebtree::ScanOp::Emit((key, loc)) => {
                out.push(1);
                out.extend_from_slice(key);
                loc.encode(&mut out);
            }
        }
    }
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for (key, loc) in entries {
        out.extend_from_slice(key);
        loc.encode(&mut out);
    }
    out
}

fn decode_index_scan_state(bytes: &[u8], evidence_key: [u8; 32]) -> Result<Option<IndexScanState>> {
    let mut pos = 0usize;
    if take(bytes, &mut pos, INDEX_SCAN_STATE_MAGIC.len())? != INDEX_SCAN_STATE_MAGIC {
        return Ok(None);
    }
    if take(bytes, &mut pos, 32)? != evidence_key {
        return Ok(None);
    }
    let stack_len = take_u32(bytes, &mut pos)? as usize;
    let mut stack = Vec::with_capacity(stack_len);
    for _ in 0..stack_len {
        let tag = take(bytes, &mut pos, 1)?[0];
        match tag {
            0 => {
                let page = PageId(take_u64(bytes, &mut pos)?);
                let depth = take_u32(bytes, &mut pos)? as usize;
                stack.push(pagebtree::ScanOp::Visit { page, depth });
            }
            1 => {
                let key = take_array_32(bytes, &mut pos)?;
                let loc = RecordLoc::decode(bytes, &mut pos)
                    .ok_or_else(|| corrupt("maintenance index cursor locator"))?;
                stack.push(pagebtree::ScanOp::Emit((key, loc)));
            }
            _ => return Err(corrupt("maintenance index cursor operation")),
        }
    }
    let entry_len = take_u32(bytes, &mut pos)? as usize;
    let mut entries = Vec::with_capacity(entry_len);
    for _ in 0..entry_len {
        let key = take_array_32(bytes, &mut pos)?;
        let loc = RecordLoc::decode(bytes, &mut pos)
            .ok_or_else(|| corrupt("maintenance index cursor entry locator"))?;
        entries.push((key, loc));
    }
    if pos != bytes.len() {
        return Err(corrupt("maintenance index cursor trailing bytes"));
    }
    Ok(Some((pagebtree::ScanCursor { stack }, entries)))
}

fn take<'a>(bytes: &'a [u8], pos: &mut usize, len: usize) -> Result<&'a [u8]> {
    let end = pos
        .checked_add(len)
        .ok_or_else(|| corrupt("maintenance index cursor offset overflow"))?;
    let out = bytes
        .get(*pos..end)
        .ok_or_else(|| corrupt("maintenance index cursor truncated"))?;
    *pos = end;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::{Algo, ObjectStore};
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_store(tag: &str) -> String {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "loom-store-{tag}-{}-{seq}.loom",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn index_scan_state_round_trips_and_invalidates_by_evidence() {
        let key = [7u8; 32];
        let cursor = pagebtree::ScanCursor {
            stack: vec![
                pagebtree::ScanOp::Visit {
                    page: PageId(42),
                    depth: 2,
                },
                pagebtree::ScanOp::Emit(([9u8; 32], RecordLoc::from_global(11, 3))),
            ],
        };
        let entries = vec![([3u8; 32], RecordLoc::from_global(5, 1))];
        let encoded = encode_index_scan_state(key, &cursor, &entries);

        let decoded = decode_index_scan_state(&encoded, key).unwrap().unwrap();
        assert_eq!(decoded.0, cursor);
        assert_eq!(decoded.1, entries);
        assert!(
            decode_index_scan_state(&encoded, [8u8; 32])
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn index_snapshot_resumes_after_deadline_preserved_cursor() {
        let path = temp_store("index-cursor-resume");
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        for i in 0..256u64 {
            store.put(&i.to_le_bytes()).unwrap();
        }
        let evidence = {
            let mut inner = store.inner.lock().unwrap();
            let control_map = store.control_map_locked(&mut inner).unwrap();
            store
                .gc_reclaim_evidence_locked(&inner, &control_map)
                .unwrap()
        };

        let error = store
            .index_snapshot_from_evidence(&evidence, None, Some(std::time::Instant::now()))
            .unwrap_err();
        assert_eq!(error.code, Code::ResourceExhausted);
        assert!(store.maintenance_index_scan.lock().unwrap().is_some());

        let snapshot = store
            .index_snapshot_from_evidence(&evidence, None, None)
            .unwrap();
        assert!(snapshot.len() >= 256);
        assert!(store.maintenance_index_scan.lock().unwrap().is_none());
        let _ = std::fs::remove_file(path);
    }
}

fn take_u32(bytes: &[u8], pos: &mut usize) -> Result<u32> {
    Ok(u32::from_le_bytes(take(bytes, pos, 4)?.try_into().unwrap()))
}

fn take_u64(bytes: &[u8], pos: &mut usize) -> Result<u64> {
    Ok(u64::from_le_bytes(take(bytes, pos, 8)?.try_into().unwrap()))
}

fn take_array_32(bytes: &[u8], pos: &mut usize) -> Result<[u8; 32]> {
    Ok(take(bytes, pos, 32)?.try_into().unwrap())
}

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
        self.gc_segments_inner(
            live,
            None,
            GcSegmentBudget::unlimited(),
            true,
            None,
            None,
            None,
        )
    }

    pub fn gc_validated_segments(&mut self, budget: GcSegmentBudget) -> Result<GcStats> {
        self.gc_validated_segments_impl(budget, true, None, None, None)
    }

    pub fn gc_validated_segments_without_tail_trim(
        &mut self,
        budget: GcSegmentBudget,
    ) -> Result<GcStats> {
        self.gc_validated_segments_impl(budget, false, None, None, None)
    }

    pub fn gc_validated_segments_until(
        &mut self,
        budget: GcSegmentBudget,
        trim_tail: bool,
        deadline: std::time::Instant,
    ) -> Result<GcStats> {
        self.gc_validated_segments_impl(budget, trim_tail, None, None, Some(deadline))
    }

    #[cfg(test)]
    pub(crate) fn gc_validated_segments_with_pre_reclaim_interleave(
        &mut self,
        budget: GcSegmentBudget,
        mut interleave: impl FnMut(&FileStore) -> Result<()>,
    ) -> Result<GcStats> {
        self.gc_validated_segments_impl(budget, true, Some(&mut interleave), None, None)
    }

    #[cfg(test)]
    pub(crate) fn gc_validated_segments_with_read_phase_interleave(
        &mut self,
        budget: GcSegmentBudget,
        mut interleave: impl FnMut(&FileStore) -> Result<()>,
    ) -> Result<GcStats> {
        self.gc_validated_segments_impl(budget, true, None, Some(&mut interleave), None)
    }

    fn gc_validated_segments_impl(
        &mut self,
        budget: GcSegmentBudget,
        trim_tail: bool,
        pre_reclaim_interleave: GcInterleave<'_>,
        read_phase_interleave: GcInterleave<'_>,
        deadline: GcDeadline,
    ) -> Result<GcStats> {
        check_gc_deadline(deadline)?;
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
            deadline,
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
        deadline: GcDeadline,
    ) -> Result<GcStats> {
        check_gc_deadline(deadline)?;
        let codec = self.default_codec; // re-frame relocated records per the current default
        let (evidence, keep_reference, keep_control, keep_derived) = {
            let mut inner = lock_until(&self.inner, deadline)?;
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
        let index_snapshot =
            self.index_snapshot_from_evidence(&evidence, read_phase_interleave, deadline)?;
        let alive = |digest: &[u8; 32]| {
            live.contains(digest)
                || keep_reference.as_ref() == Some(digest)
                || keep_control.as_ref() == Some(digest)
                || keep_derived.contains(digest)
        };
        // Group index entries by every physical record page. A large record may use an old contiguous
        // run or a fragmented page chain, while multiple small objects may share one slab page.
        let mut page_live: BTreeMap<u64, bool> = BTreeMap::new();
        let mut record_pages = BTreeMap::<[u8; 32], Vec<u64>>::new();
        let mut file = lock_until(&self.file, deadline)?;
        for (digest, loc) in &index_snapshot {
            check_gc_deadline(deadline)?;
            let pages =
                crate::record_io::blob_pages(&mut **file, loc.global_page(), evidence.page_count)?;
            for page in &pages {
                *page_live.entry(*page).or_insert(false) |= alive(digest);
            }
            record_pages.insert(*digest, pages);
        }
        drop(file);
        let mut occupancy: BTreeMap<u64, (u64, u64)> = BTreeMap::new(); // segment -> (live_pages, total_pages)
        for (&page, &is_live) in &page_live {
            let e = occupancy
                .entry(page / page::PAGES_PER_SEGMENT)
                .or_insert((0, 0));
            e.1 += 1;
            if is_live {
                e.0 += 1;
            }
        }
        let chosen: BTreeSet<u64> =
            choose_sparse_segments_bounded(&occupancy, eligible_segments, budget)
                .into_iter()
                .collect();
        if chosen.is_empty() {
            return Ok(GcStats::default());
        }
        let active_segment = evidence
            .page_count
            .saturating_sub(1)
            .checked_div(page::PAGES_PER_SEGMENT)
            .unwrap_or(0);
        let evacuation_segments = chosen
            .iter()
            .copied()
            .filter(|segment| *segment != active_segment)
            .collect::<BTreeSet<_>>();
        let mut survivors: Vec<(Digest, Vec<u8>)> = Vec::new();
        let mut dropped: Vec<[u8; 32]> = Vec::new();
        let mut pages_to_free: BTreeSet<u64> = BTreeSet::new();
        for (digest, loc) in &index_snapshot {
            check_gc_deadline(deadline)?;
            let pages = &record_pages[digest];
            let touches_chosen = pages
                .iter()
                .any(|page| chosen.contains(&(page / page::PAGES_PER_SEGMENT)));
            if !touches_chosen {
                continue;
            }
            let touches_evacuation = pages
                .iter()
                .any(|page| evacuation_segments.contains(&(page / page::PAGES_PER_SEGMENT)));
            if alive(digest) && touches_evacuation {
                pages_to_free.extend(pages);
                let d = Digest::of(self.digest_algo, *digest);
                let payload = self
                    .read_indexed_payload_snapshot(loc, evidence.page_count, &d)?
                    .ok_or_else(|| corrupt("live object missing during gc"))?;
                survivors.push((d, payload));
            } else if !alive(digest) {
                dropped.push(*digest);
                pages_to_free.extend(
                    pages
                        .iter()
                        .copied()
                        .filter(|page| !page_live.get(page).copied().unwrap_or(false)),
                );
            }
        }
        if survivors.is_empty() && dropped.is_empty() {
            return Ok(GcStats::default());
        }

        check_gc_deadline(deadline)?;

        // Phase B: one transaction - relocate survivors to fresh pages, point-update their index
        // entries, delete the dropped keys, and free the reclaimed segments' pages.
        let mut inner = lock_until(&self.inner, deadline)?;
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
            let mut file = lock_until(&self.file, deadline)?;
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
            // The pages were never in the seeded free list, so survivor/index writes above could not
            // have reused them.
            let mut pages_freed = 0u64;
            for &p in &pages_to_free {
                alloc.free(PageId(p), 1);
                pages_freed += 1;
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
                inner.overlay_root,
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
                None,
            )?;
            (roots, index_root, placements, pages_freed)
        };

        let pages_trimmed = before_page_count.saturating_sub(roots.page_count);
        let root_page_count = roots.page_count;
        inner.generation = new_gen;
        inner.page_count = root_page_count;
        inner.index_root = index_root;
        inner.overlay_root = roots.overlay_root;
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
            segments_reclaimed: evacuation_segments.len() as u64,
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
                inner.overlay_root,
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
                None,
            )?
        };
        let trimmed = before.saturating_sub(roots.page_count);
        let root_page_count = roots.page_count;
        inner.generation = new_gen;
        inner.page_count = root_page_count;
        inner.overlay_root = roots.overlay_root;
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
        self.compact_tail_once_impl(max_pages, max_objects, max_bytes, None, None)
    }

    pub fn compact_tail_once_until(
        &mut self,
        max_pages: u64,
        max_objects: u64,
        max_bytes: u64,
        deadline: std::time::Instant,
    ) -> Result<TailCompactionStats> {
        self.compact_tail_once_impl(max_pages, max_objects, max_bytes, None, Some(deadline))
    }

    #[cfg(test)]
    pub(crate) fn compact_tail_once_with_pre_commit_interleave(
        &mut self,
        max_pages: u64,
        max_objects: u64,
        max_bytes: u64,
        mut interleave: impl FnMut(&FileStore) -> Result<()>,
    ) -> Result<TailCompactionStats> {
        self.compact_tail_once_impl(
            max_pages,
            max_objects,
            max_bytes,
            Some(&mut interleave),
            None,
        )
    }

    fn compact_tail_once_impl(
        &mut self,
        max_pages: u64,
        max_objects: u64,
        max_bytes: u64,
        pre_commit_interleave: GcInterleave<'_>,
        deadline: GcDeadline,
    ) -> Result<TailCompactionStats> {
        check_gc_deadline(deadline)?;
        if max_pages == 0 || max_objects == 0 || max_bytes == 0 {
            return Err(LoomError::new(
                Code::InvalidArgument,
                "tail compaction budgets must be nonzero",
            ));
        }
        let codec = self.default_codec;
        let evidence = {
            let mut inner = lock_until(&self.inner, deadline)?;
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
        let index_snapshot = self.index_snapshot_from_evidence(&evidence, None, deadline)?;
        check_gc_deadline(deadline)?;
        let mut physical = Vec::with_capacity(index_snapshot.len());
        {
            let mut file = lock_until(&self.file, deadline)?;
            for (key, loc) in index_snapshot {
                check_gc_deadline(deadline)?;
                let pages = crate::record_io::blob_pages(
                    &mut **file,
                    loc.global_page(),
                    evidence.page_count,
                )?;
                physical.push((key, loc, pages));
            }
        }
        let mut selected: Vec<(Digest, RecordLoc, Vec<u64>, Vec<u8>)> = Vec::new();
        let mut selected_page_set = BTreeSet::new();
        let mut selected_pages = 0u64;
        let mut selected_bytes = 0u64;
        physical.sort_by_key(|(_, _, pages)| {
            std::cmp::Reverse(pages.iter().copied().max().unwrap_or(0))
        });
        for (key, loc, pages) in physical {
            check_gc_deadline(deadline)?;
            if !pages
                .iter()
                .any(|page| *page >= scan_start && *page < tail_end)
            {
                continue;
            }
            let additional_pages = pages
                .iter()
                .filter(|page| !selected_page_set.contains(*page))
                .count() as u64;
            if additional_pages > 0
                && (selected.len() as u64 >= max_objects
                    || selected_pages.saturating_add(additional_pages) > max_pages)
            {
                continue;
            }
            let digest = Digest::of(self.digest_algo, key);
            let payload = self
                .read_indexed_payload_snapshot(&loc, evidence.page_count, &digest)?
                .ok_or_else(|| corrupt("tail object missing during compaction"))?;
            if additional_pages > 0
                && selected_bytes.saturating_add(payload.len() as u64) > max_bytes
            {
                continue;
            }
            selected_pages = selected_pages.saturating_add(additional_pages);
            selected_bytes = selected_bytes.saturating_add(payload.len() as u64);
            selected_page_set.extend(&pages);
            selected.push((digest, loc, pages, payload));
        }
        if selected.is_empty() {
            return Ok(TailCompactionStats {
                attempted: true,
                skipped: true,
                ..TailCompactionStats::default()
            });
        }

        check_gc_deadline(deadline)?;

        if let Some(interleave) = pre_commit_interleave {
            interleave(self)?;
        }

        let mut inner = lock_until(&self.inner, deadline)?;
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
        let (roots, index_root, placements, relocated_pages) = {
            let mut file = lock_until(&self.file, deadline)?;
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
            for (_, loc) in &placements {
                if crate::record_io::blob_pages(&mut **file, loc.global_page(), alloc.page_count())?
                    .iter()
                    .any(|page| *page >= scan_start)
                {
                    return Ok(TailCompactionStats {
                        attempted: true,
                        skipped: true,
                        ..TailCompactionStats::default()
                    });
                }
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
            for page in &selected_page_set {
                alloc.free(PageId(*page), 1);
                relocated_pages = relocated_pages.saturating_add(1);
            }
            let touched_segments: BTreeSet<u64> = placements
                .iter()
                .map(|(_, loc)| loc.segment_id)
                .chain(
                    selected_page_set
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
                inner.overlay_root,
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
                None,
            )?;
            (roots, index_root, placements, relocated_pages)
        };
        let root_page_count = roots.page_count;
        inner.generation = new_gen;
        inner.page_count = root_page_count;
        inner.index_root = index_root;
        inner.overlay_root = roots.overlay_root;
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
        deadline: GcDeadline,
    ) -> Result<Vec<([u8; 32], RecordLoc)>> {
        let Some(root) = evidence.index_root else {
            return Ok(Vec::new());
        };
        let evidence_key = self.index_scan_evidence_key(evidence);
        let mut interleaved = false;
        let (mut cursor, mut out) = self
            .load_index_scan_state(evidence_key)?
            .unwrap_or_else(|| (pagebtree::ScanCursor::new(root), Vec::new()));
        while !cursor.completed() {
            if deadline.is_some_and(|deadline| std::time::Instant::now() >= deadline) {
                self.save_index_scan_state(evidence_key, &cursor, &out)?;
                check_gc_deadline(deadline)?;
            }
            let step = pagebtree::scan_step_with_page_reader(
                &mut cursor,
                evidence.page_count,
                64,
                deadline,
                |page| {
                    check_gc_deadline(deadline)?;
                    let mut buf = [0u8; PAGE_SIZE as usize];
                    {
                        let mut file = lock_until(&self.file, deadline)?;
                        read_exact_at(&mut **file, page.offset(DATA_START), &mut buf)
                            .map_err(|_| corrupt("truncated btree node page"))?;
                    }
                    Ok(buf)
                },
            )?;
            out.extend(step.entries);
            if !interleaved && let Some(interleave) = read_phase_interleave.as_mut() {
                interleaved = true;
                interleave(self)?;
            }
            if deadline.is_some_and(|deadline| std::time::Instant::now() >= deadline) {
                self.save_index_scan_state(evidence_key, &cursor, &out)?;
                check_gc_deadline(deadline)?;
            }
        }
        self.clear_index_scan_state()?;
        Ok(out)
    }

    fn index_scan_evidence_key(&self, evidence: &GcReclaimEvidence) -> [u8; 32] {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&evidence.generation.to_le_bytes());
        bytes.extend_from_slice(&evidence.page_count.to_le_bytes());
        put_optional_digest_bytes(&mut bytes, evidence.reference_root);
        put_optional_digest_bytes(&mut bytes, evidence.control_root);
        put_optional_page_bytes(&mut bytes, evidence.index_root);
        put_optional_page_bytes(&mut bytes, evidence.overlay_root);
        put_optional_digest_bytes(&mut bytes, evidence.control_fingerprint);
        bytes.extend_from_slice(&(evidence.derived_roots.len() as u32).to_le_bytes());
        for root in &evidence.derived_roots {
            bytes.extend_from_slice(root.bytes());
        }
        *Digest::hash(self.digest_algo, &bytes).bytes()
    }

    fn load_index_scan_state(&self, evidence_key: [u8; 32]) -> Result<Option<IndexScanState>> {
        let guard = self.maintenance_index_scan.lock().map_err(|_| poisoned())?;
        let Some(bytes) = guard.as_deref() else {
            return Ok(None);
        };
        decode_index_scan_state(bytes, evidence_key)
    }

    fn save_index_scan_state(
        &self,
        evidence_key: [u8; 32],
        cursor: &pagebtree::ScanCursor,
        entries: &[([u8; 32], RecordLoc)],
    ) -> Result<()> {
        *self.maintenance_index_scan.lock().map_err(|_| poisoned())? =
            Some(encode_index_scan_state(evidence_key, cursor, entries));
        Ok(())
    }

    fn clear_index_scan_state(&self) -> Result<()> {
        *self.maintenance_index_scan.lock().map_err(|_| poisoned())? = None;
        Ok(())
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
            overlay_root: inner.overlay_root,
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
            record::CHUNKED_BLOB_MAGIC => {
                let rec = crate::record_io::read_chunked_blob(&mut **file, global, page_count)?;
                decode_record(&rec, digest, dek.as_ref(), self.digest_algo)?
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

            let (keys, enc_meta, source_overlay_root, source_page_count): FullCompactionSnapshot = {
                let mut i = self.inner.lock().map_err(|_| poisoned())?;
                self.materialize_index_locked(&mut i)?;
                (
                    i.index.keys().copied().collect(),
                    i.encryption_meta.clone(),
                    i.overlay_root,
                    i.page_count,
                )
            };
            let (current_records, mut control_records) = {
                let overlay = self.mutable_overlay.lock().map_err(|_| poisoned())?;
                let generation = overlay.generation().as_u64();
                let entries = overlay
                    .export_entries()?
                    .into_iter()
                    .map(|entry| (entry.key.clone(), entry))
                    .collect::<BTreeMap<_, _>>()
                    .into_values()
                    .collect::<Vec<_>>();
                let mut records = Vec::new();
                if generation > 0 || !entries.is_empty() {
                    records.push((
                        mutable_overlay_meta_address(),
                        encode_mutable_overlay_meta(generation),
                    ));
                }
                records.extend(entries.iter().map(|entry| {
                    (
                        mutable_overlay_entry_address(&entry.key),
                        encode_mutable_overlay_entry(entry),
                    )
                }));
                let (current_records, control_records) = split_mutable_overlay_records(&records);
                let mut control_records = control_records
                    .into_iter()
                    .map(|(address, value)| (address, value.to_vec()))
                    .collect::<Vec<_>>();
                if let Some(root) = source_overlay_root {
                    let mut file = self.file.lock().map_err(|_| poisoned())?;
                    for (address, loc) in
                        pagebtree::load_all(&mut **file, DATA_START, root, source_page_count)?
                    {
                        if address == mutable_overlay_meta_address()
                            || address == mutable_overlay_current_root_address()
                        {
                            continue;
                        }
                        let value = read_blob_from_loc(&mut **file, loc)?;
                        if is_mutable_overlay_current_entry_record(&value) {
                            return Err(corrupt(
                                "mutable overlay control root contains current entry; controlled migration required",
                            ));
                        }
                        if !is_mutable_overlay_control_record_family(&value) {
                            return Err(corrupt("unknown mutable overlay control record family"));
                        }
                        control_records.push((address, value));
                    }
                }
                control_records.sort_by_key(|record| record.0);
                control_records.dedup_by_key(|record| record.0);
                let current_records = current_records
                    .into_iter()
                    .map(|(address, value)| (address, value.to_vec()))
                    .collect::<Vec<_>>();
                (current_records, control_records)
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
            let current_borrowed = current_records
                .iter()
                .map(|(key, value)| (*key, value.as_slice()))
                .collect::<Vec<_>>();
            let mut current_entries = write_blob_pages(&mut out, &mut alloc, &current_borrowed)?;
            current_entries.sort_unstable_by_key(|e| e.0);
            let current_root =
                pagebtree::build_packed(&mut out, DATA_START, &mut alloc, &current_entries)?;
            control_records.push(mutable_overlay_current_root_record(current_root));
            let control_borrowed = control_records
                .iter()
                .map(|(key, value)| (*key, value.as_slice()))
                .collect::<Vec<_>>();
            let mut overlay_entries = write_blob_pages(&mut out, &mut alloc, &control_borrowed)?;
            overlay_entries.sort_unstable_by_key(|e| e.0);
            let overlay_root =
                pagebtree::build_packed(&mut out, DATA_START, &mut alloc, &overlay_entries)?;
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
                overlay_root,
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
