//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use crate::mark_epoch::ReachabilityMarkReclaimEvidence;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GcReclaimEvidence {
    pub(crate) generation: u64,
    pub(crate) page_count: u64,
    pub(crate) reference_root: Option<Digest>,
    pub(crate) control_root: Option<Digest>,
    pub(crate) index_root: Option<PageId>,
    pub(crate) overlay_root: Option<PageId>,
    pub(crate) control_fingerprint: Option<Digest>,
    pub(crate) derived_roots: BTreeSet<Digest>,
    pub(crate) canonical_roots: Vec<GcCanonicalRootEvidence>,
    pub(crate) canonical_roots_fingerprint: Option<Digest>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GcCanonicalRootEvidence {
    pub(crate) name: String,
    pub(crate) family_id: Option<u16>,
    pub(crate) page_root: Option<PageId>,
    pub(crate) digest_root: Option<Digest>,
    pub(crate) reachability: String,
    pub(crate) semantic_liveness: bool,
    pub(crate) advisory: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GcCompactionClassification {
    SemanticLiveness,
    PhysicalSafety,
    AdvisoryPreservation,
    ReclaimNeutral,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GcCompactionRootPlan {
    pub name: String,
    pub(crate) family_id: Option<u16>,
    pub(crate) page_root: Option<PageId>,
    pub(crate) digest_root: Option<Digest>,
    pub classification: GcCompactionClassification,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GcCompactionPageCandidate {
    pub page: u64,
    pub segment: u64,
    pub owner: String,
    pub classification: GcCompactionClassification,
    pub eligible: bool,
    pub blocker: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GcCanonicalCompactionPlan {
    pub(crate) evidence: GcReclaimEvidence,
    pub roots: Vec<GcCompactionRootPlan>,
    pub page_candidates: Vec<GcCompactionPageCandidate>,
    pub eligible_pages: u64,
    pub blocked_pages: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GcCanonicalRelocationStats {
    pub objects_preserved: u64,
    pub objects_dropped: u64,
    pub root_pages_rebuilt: u64,
    pub pages_reclaimed: u64,
    pub source_page_count: u64,
    pub destination_page_count: u64,
    pub conflicts: u64,
}

type GcInterleave<'a> = Option<&'a mut dyn FnMut(&FileStore) -> Result<()>>;
type GcPreCommitHook<'a> = Option<&'a mut dyn FnMut() -> Result<()>>;
type GcDeadline<'a> = Option<&'a dyn Fn() -> bool>;
type IndexScanEntry = ([u8; 32], RecordLoc);
type IndexScanState = (pagebtree::ScanCursor, Vec<IndexScanEntry>);
type FullCompactionSnapshot = (
    Vec<[u8; 32]>,
    Option<Vec<u8>>,
    Option<PageId>,
    Vec<RootCatalogEntry>,
    u64,
    u64,
);
const INDEX_SCAN_STATE_MAGIC: &[u8; 8] = b"LIDXCUR1";

fn check_gc_deadline(deadline: GcDeadline<'_>) -> Result<()> {
    if deadline.is_some_and(|deadline| deadline()) {
        return Err(LoomError::new(
            Code::ResourceExhausted,
            "maintenance work budget exhausted",
        ));
    }
    Ok(())
}

fn root_family_reachability_label(reachability: RootFamilyReachability) -> &'static str {
    match reachability {
        RootFamilyReachability::SemanticRoot => "semantic",
        RootFamilyReachability::ControlRoot => "control",
        RootFamilyReachability::PhysicalSafetyRoot => "physical_safety",
        RootFamilyReachability::AdvisoryPreserveOnly => "advisory_preserve_only",
    }
}

fn root_compaction_classification(root: &GcCanonicalRootEvidence) -> GcCompactionClassification {
    if root.advisory {
        GcCompactionClassification::AdvisoryPreservation
    } else if root.semantic_liveness {
        GcCompactionClassification::SemanticLiveness
    } else if matches!(
        root.name.as_str(),
        "object_index_records" | "root_catalog" | "free_map" | "maintenance"
    ) || root.reachability == "physical_safety"
    {
        GcCompactionClassification::PhysicalSafety
    } else {
        GcCompactionClassification::ReclaimNeutral
    }
}

fn compaction_blocker_label(classification: GcCompactionClassification) -> Option<&'static str> {
    match classification {
        GcCompactionClassification::SemanticLiveness => Some("semantic_liveness"),
        GcCompactionClassification::PhysicalSafety => Some("physical_safety"),
        GcCompactionClassification::AdvisoryPreservation => Some("advisory_preservation"),
        GcCompactionClassification::ReclaimNeutral => None,
    }
}

fn merge_compaction_classification(
    current: GcCompactionClassification,
    next: GcCompactionClassification,
) -> GcCompactionClassification {
    match (current, next) {
        (GcCompactionClassification::SemanticLiveness, _)
        | (_, GcCompactionClassification::SemanticLiveness) => {
            GcCompactionClassification::SemanticLiveness
        }
        (GcCompactionClassification::PhysicalSafety, _)
        | (_, GcCompactionClassification::PhysicalSafety) => {
            GcCompactionClassification::PhysicalSafety
        }
        (GcCompactionClassification::AdvisoryPreservation, _)
        | (_, GcCompactionClassification::AdvisoryPreservation) => {
            GcCompactionClassification::AdvisoryPreservation
        }
        _ => GcCompactionClassification::ReclaimNeutral,
    }
}

#[derive(Clone, Debug)]
struct GcCompactionPageAccumulator {
    page: u64,
    segment: u64,
    owners: BTreeSet<String>,
    classification: GcCompactionClassification,
}

impl GcCompactionPageAccumulator {
    fn new(page: u64, owner: String, classification: GcCompactionClassification) -> Self {
        let mut owners = BTreeSet::new();
        owners.insert(owner);
        Self {
            page,
            segment: page / page::PAGES_PER_SEGMENT,
            owners,
            classification,
        }
    }

    fn merge(&mut self, owner: String, classification: GcCompactionClassification) {
        self.owners.insert(owner);
        self.classification = merge_compaction_classification(self.classification, classification);
    }

    fn into_candidate(self) -> GcCompactionPageCandidate {
        let blocker = compaction_blocker_label(self.classification).map(str::to_string);
        GcCompactionPageCandidate {
            page: self.page,
            segment: self.segment,
            owner: self.owners.into_iter().collect::<Vec<_>>().join(","),
            classification: self.classification,
            eligible: blocker.is_none(),
            blocker,
        }
    }
}

fn add_compaction_page_candidate(
    candidates: &mut BTreeMap<u64, GcCompactionPageAccumulator>,
    page: u64,
    owner: String,
    classification: GcCompactionClassification,
) {
    candidates
        .entry(page)
        .and_modify(|candidate| candidate.merge(owner.clone(), classification))
        .or_insert_with(|| GcCompactionPageAccumulator::new(page, owner, classification));
}

fn digest_owner_label(digest: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity("object:".len() + 64);
    out.push_str("object:");
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn root_records_from_btree(
    file: &mut dyn BackingIo,
    family_id: Option<u16>,
    root: Option<PageId>,
    page_count: u64,
) -> Result<BTreeMap<[u8; 32], Vec<u8>>> {
    let Some(root) = root else {
        return Ok(BTreeMap::new());
    };
    let mut records = BTreeMap::new();
    let entries = match family_id {
        Some(family_id) => crate::root_family_load_all(file, family_id, root, page_count)?,
        None => pagebtree::load_all(file, DATA_START, root, page_count)?,
    };
    for (address, loc) in entries {
        records.insert(address, read_blob_from_loc(file, loc)?);
    }
    Ok(records)
}

fn record_refs(records: &BTreeMap<[u8; 32], Vec<u8>>) -> Vec<MutableOverlayRecordRef<'_>> {
    records
        .iter()
        .map(|(address, value)| (*address, value.as_slice()))
        .collect()
}

fn build_record_family_root(
    file: &mut dyn BackingIo,
    alloc: &mut PageAllocator,
    family_id: Option<u16>,
    records: &BTreeMap<[u8; 32], Vec<u8>>,
) -> Result<Option<PageId>> {
    let refs = record_refs(records);
    let (root, _) = match family_id {
        Some(family_id) => write_root_family_record_refs_to_root(
            file,
            alloc,
            family_id,
            None,
            alloc.page_count(),
            &refs,
            None,
            false,
        )?,
        None => write_mutable_record_refs_to_root(
            file,
            alloc,
            None,
            alloc.page_count(),
            &refs,
            None,
            false,
        )?,
    };
    Ok(root)
}

fn lock_until<'a, T>(
    mutex: &'a std::sync::Mutex<T>,
    deadline: GcDeadline<'_>,
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

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct MaintenancePublicationState {
        generation: u64,
        object_index_root: Option<PageId>,
        region_table_root: Option<PageId>,
        maintenance_root: Option<PageId>,
        object_count: u64,
        canonical_free_runs: Vec<FreePageRun>,
        page_count: u64,
        physical_page_count: u64,
        affected_locators: Vec<([u8; 32], RecordLoc)>,
    }

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

    fn maintenance_publication_state(
        store: &FileStore,
        affected: &[Digest],
    ) -> MaintenancePublicationState {
        let mut inner = store.inner.lock().unwrap();
        store.materialize_index_locked(&mut inner).unwrap();
        let mut canonical_free_runs = inner.free.clone();
        canonical_free_runs.sort_by_key(|run| (run.start, run.len, run.freed_gen));
        let affected_locators = affected
            .iter()
            .map(|digest| {
                (
                    *digest.bytes(),
                    *inner
                        .index
                        .get(digest.bytes())
                        .expect("affected object locator"),
                )
            })
            .collect();
        MaintenancePublicationState {
            generation: inner.generation,
            object_index_root: inner.index_root,
            region_table_root: inner.region_table_root,
            maintenance_root: inner.maintenance_root,
            object_count: inner.maintenance.object_count,
            canonical_free_runs,
            page_count: inner.page_count,
            physical_page_count: inner.maintenance.physical_page_count,
            affected_locators,
        }
    }

    fn assert_object_payloads(store: &FileStore, objects: &[(Digest, Vec<u8>)]) {
        for (digest, payload) in objects {
            assert!(store.has(digest).unwrap());
            assert_eq!(
                store.get(digest).unwrap().as_deref(),
                Some(payload.as_slice())
            );
        }
    }

    fn assert_maintenance_publication_state_eq(
        actual: &MaintenancePublicationState,
        expected: &MaintenancePublicationState,
    ) {
        assert_eq!(actual.generation, expected.generation, "generation");
        assert_eq!(
            actual.object_index_root, expected.object_index_root,
            "object-index root"
        );
        assert_eq!(
            actual.region_table_root, expected.region_table_root,
            "RegionTable root"
        );
        assert_eq!(
            actual.maintenance_root, expected.maintenance_root,
            "maintenance root"
        );
        assert_eq!(actual.object_count, expected.object_count, "object count");
        assert_eq!(
            actual.canonical_free_runs, expected.canonical_free_runs,
            "canonical free runs"
        );
        assert_eq!(actual.page_count, expected.page_count, "page count");
        assert_eq!(
            actual.physical_page_count, expected.physical_page_count,
            "physical page count"
        );
        assert_eq!(
            actual.affected_locators, expected.affected_locators,
            "affected locator cache entries"
        );
    }

    fn append_committed_free_pages(store: &FileStore, page_len: u64) {
        let mut inner = store.inner.lock().unwrap();
        let new_gen = inner.generation + 1;
        let roots = {
            let mut file = store.file.lock().unwrap();
            let mut alloc = PageAllocator::new(inner.page_count, new_gen, inner.free.clone());
            alloc
                .install_metadata_bootstrap_reserve(&inner.metadata_bootstrap_reserve)
                .unwrap();
            let pages = alloc.alloc(page_len);
            alloc.free(pages, page_len).unwrap();
            file.grow(DATA_START + alloc.page_count() * PAGE_SIZE)
                .unwrap();
            finish_txn(
                &mut **file,
                &mut alloc,
                new_gen,
                inner.maintenance.object_count,
                TxnRootInputs {
                    object_index: inner.index_root,
                    legacy_overlay: legacy_overlay_root_for_publication(
                        &inner,
                        inner.current_record_root,
                        inner.root_catalog_root,
                    ),
                    current_records: inner.current_record_root,
                    root_catalog: TxnRootCatalog {
                        root: inner.root_catalog_root,
                        entries: inner.root_catalog_entries.clone(),
                    },
                    previous_mutable_overlay_generation_floor: inner
                        .mutable_overlay_generation_floor,
                    mutable_overlay_generation_floor: inner.mutable_overlay_generation_floor,
                    reference: inner.reference_root.map(|digest| *digest.bytes()),
                    control: inner.control_root.map(|digest| *digest.bytes()),
                },
                inner.open_segment,
                &inner.maintenance,
                &BTreeSet::new(),
                (
                    inner.freemap,
                    inner.region_table_root,
                    inner.maintenance_root,
                ),
                inner.encryption_meta.clone(),
                store.digest_algo,
                None,
            )
            .unwrap()
        };
        store
            .adopt_committed_roots_locked(&mut inner, roots)
            .unwrap();
    }

    fn install_fragmented_free_map(store: &FileStore, extent_count: u64) {
        let mut inner = store.inner.lock().unwrap();
        let new_gen = inner.generation + 1;
        let first = inner.page_count;
        let page_count = first + extent_count * 2;
        let mut free = inner.free.clone();
        free.extend((0..extent_count).map(|index| FreePageRun {
            start: first + index * 2,
            len: 1,
            freed_gen: new_gen,
        }));
        let roots = {
            let mut file = store.file.lock().unwrap();
            file.grow(DATA_START + page_count * PAGE_SIZE).unwrap();
            let mut alloc = PageAllocator::new(page_count, new_gen, free);
            alloc
                .install_metadata_bootstrap_reserve(&inner.metadata_bootstrap_reserve)
                .unwrap();
            finish_txn(
                &mut **file,
                &mut alloc,
                new_gen,
                inner.maintenance.object_count,
                TxnRootInputs {
                    object_index: inner.index_root,
                    legacy_overlay: legacy_overlay_root_for_publication(
                        &inner,
                        inner.current_record_root,
                        inner.root_catalog_root,
                    ),
                    current_records: inner.current_record_root,
                    root_catalog: TxnRootCatalog {
                        root: inner.root_catalog_root,
                        entries: inner.root_catalog_entries.clone(),
                    },
                    previous_mutable_overlay_generation_floor: inner
                        .mutable_overlay_generation_floor,
                    mutable_overlay_generation_floor: inner.mutable_overlay_generation_floor,
                    reference: inner.reference_root.map(|digest| *digest.bytes()),
                    control: inner.control_root.map(|digest| *digest.bytes()),
                },
                inner.open_segment,
                &inner.maintenance,
                &BTreeSet::new(),
                (
                    inner.freemap,
                    inner.region_table_root,
                    inner.maintenance_root,
                ),
                inner.encryption_meta.clone(),
                store.digest_algo,
                None,
            )
            .unwrap()
        };
        store
            .adopt_committed_roots_locked(&mut inner, roots)
            .unwrap();
    }

    fn sorted_free_runs(store: &FileStore) -> Vec<FreePageRun> {
        let mut runs = store.inner.lock().unwrap().free.clone();
        runs.sort_by_key(|run| (run.start, run.len, run.freed_gen));
        runs
    }

    fn typed_free_map_pages(store: &FileStore) -> BTreeSet<u64> {
        let inner = store.inner.lock().unwrap();
        let root = inner.freemap.unwrap().0;
        let page_count = inner.page_count;
        drop(inner);
        crate::pagebtree::collect_free_page_extent_pages(
            &mut **store.file.lock().unwrap(),
            DATA_START,
            root,
            page_count,
        )
        .unwrap()
        .into_iter()
        .map(|page| page.0)
        .collect()
    }

    #[test]
    fn compaction_accounting_and_reopen_preserve_multilevel_typed_free_map() {
        let path = temp_store("typed-free-map-compaction-accounting");
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        install_fragmented_free_map(&store, 150);
        let expected_runs = sorted_free_runs(&store);
        let expected_pages = typed_free_map_pages(&store);
        assert!(expected_pages.len() > 1);

        let live_pages = {
            let mut inner = store.inner.lock().unwrap();
            let control_map = store.control_map_locked(&mut inner).unwrap();
            store
                .current_metadata_live_pages_locked(&inner, &control_map)
                .unwrap()
        };
        assert!(expected_pages.is_subset(&live_pages));
        {
            let mut file = store.file.lock().unwrap();
            for page in &expected_pages {
                let mut raw = [0u8; PAGE_SIZE as usize];
                read_exact_at(&mut **file, PageId(*page).offset(DATA_START), &mut raw).unwrap();
                assert_eq!(
                    raw[1] & 0xF0,
                    pagebtree::ValueCodecKind::FreePageExtent.discriminator()
                );
            }
        }
        drop(store);

        let reopened = FileStore::open(&path).unwrap();
        assert_eq!(sorted_free_runs(&reopened), expected_runs);
        assert_eq!(typed_free_map_pages(&reopened), expected_pages);
        std::fs::remove_file(path).unwrap();
    }

    fn completed_mark_state(live: Digest) -> loom_core::ReachabilityMarkState {
        loom_core::ReachabilityMarkState {
            pinned: BTreeSet::from([live]),
            marked: BTreeSet::from([live]),
            queue: VecDeque::new(),
            stream_roots: VecDeque::new(),
            content_roots: VecDeque::new(),
            prolly_cursors: VecDeque::new(),
            completed: true,
        }
    }

    #[test]
    fn validated_gc_preserves_advanced_typed_free_map_and_reopen_state() {
        let path = temp_store("typed-free-map-validated-gc");
        let mut store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let live_payload = b"typed-free-map-live".to_vec();
        let live = store.put(&live_payload).unwrap();
        store.set_reference_root(Some(live)).unwrap();
        install_fragmented_free_map(&store, 150);
        let captured_root = store.inner.lock().unwrap().freemap.unwrap().0;
        let captured_pages = typed_free_map_pages(&store);
        let mut epoch = store
            .begin_reachability_mark_epoch(Some(live), BTreeSet::new(), completed_mark_state(live))
            .unwrap();

        store.inner.lock().unwrap().metadata_bootstrap_reserve =
            MetadataBootstrapReserve::default();
        let successor_payload = b"typed-free-map-successor".to_vec();
        let successor = store.put(&successor_payload).unwrap();
        install_fragmented_free_map(&store, 1_200);
        let advanced_root = store.inner.lock().unwrap().freemap.unwrap().0;
        let advanced_pages = typed_free_map_pages(&store);
        assert_ne!(advanced_root, captured_root);
        let advanced_only_pages = advanced_pages
            .difference(&captured_pages)
            .filter(|page| **page >= epoch.page_high_water_mark)
            .copied()
            .collect::<BTreeSet<_>>();
        assert!(
            !advanced_only_pages.is_empty(),
            "captured_hwm={} captured_pages={captured_pages:?} advanced_pages={advanced_pages:?}",
            epoch.page_high_water_mark
        );
        while !epoch.metadata_completed {
            store
                .step_reachability_metadata_mark_epoch(&mut epoch, 8, None)
                .unwrap();
        }
        for page in &captured_pages {
            assert!(
                store
                    .reachability_mark_metadata_page_state_for_test(&epoch, *page)
                    .unwrap()
                    .0,
                "captured typed free-map page {page} was not marked"
            );
        }
        for page in &advanced_only_pages {
            assert!(
                !store
                    .reachability_mark_metadata_page_state_for_test(&epoch, *page)
                    .unwrap()
                    .0,
                "post-capture typed free-map page {page} entered captured evidence"
            );
        }
        store.complete_reachability_mark_epoch(&epoch).unwrap();
        store
            .gc_validated_segments(GcSegmentBudget::unlimited())
            .unwrap();

        assert_eq!(store.get(&live).unwrap().unwrap(), live_payload);
        assert_eq!(store.get(&successor).unwrap().unwrap(), successor_payload);
        let current_pages = typed_free_map_pages(&store);
        assert!(!current_pages.is_empty());
        drop(store);

        let reopened = FileStore::open(&path).unwrap();
        assert_eq!(reopened.get(&live).unwrap().unwrap(), live_payload);
        assert_eq!(
            reopened.get(&successor).unwrap().unwrap(),
            successor_payload
        );
        assert_eq!(typed_free_map_pages(&reopened), current_pages);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn reader_lease_blocks_reuse_of_superseded_typed_free_map_nodes() {
        let path = temp_store("typed-free-map-reader-lease");
        let mut store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let live = store.put(b"typed-free-map-reader-live").unwrap();
        store.set_reference_root(Some(live)).unwrap();
        install_fragmented_free_map(&store, 150);
        let before_pages = typed_free_map_pages(&store);
        let reader = FileStore::open_read(&path).unwrap();
        store.put(b"advance typed free map").unwrap();
        let after_pages = typed_free_map_pages(&store);
        let superseded = before_pages
            .difference(&after_pages)
            .next()
            .copied()
            .expect("typed free-map publication must supersede at least one node");
        let mut epoch = store
            .begin_reachability_mark_epoch(Some(live), BTreeSet::new(), completed_mark_state(live))
            .unwrap();
        while !epoch.metadata_completed {
            store
                .step_reachability_metadata_mark_epoch(&mut epoch, 8, None)
                .unwrap();
        }
        store.complete_reachability_mark_epoch(&epoch).unwrap();
        let blocked = store
            .gc_validated_segments(GcSegmentBudget::unlimited())
            .unwrap_err();
        assert_eq!(blocked.code, Code::Conflict);
        assert!(!store.inner.lock().unwrap().free.iter().any(|run| {
            superseded >= run.start && superseded < run.start.saturating_add(run.len)
        }));
        drop(reader);
        store
            .gc_validated_segments(GcSegmentBudget::unlimited())
            .unwrap();
        assert!(store.inner.lock().unwrap().free.iter().any(|run| {
            superseded >= run.start && superseded < run.start.saturating_add(run.len)
        }));
        let reuse_reader = FileStore::open_read(&path).unwrap();
        for generation in 0..REUSE_SAFE_WINDOW {
            store
                .put(format!("age superseded typed node {generation}").as_bytes())
                .unwrap();
        }
        let actual_free = store.inner.lock().unwrap().free.clone();
        assert!(actual_free.iter().any(|run| {
            superseded >= run.start && superseded < run.start.saturating_add(run.len)
        }));
        let horizon = store.inner.lock().unwrap().minimum_recoverable_generation;
        let (blocked_reuse, _) = store
            .transaction_reusable_free(&actual_free, None, horizon)
            .unwrap();
        assert!(!blocked_reuse.iter().any(|run| {
            superseded >= run.start && superseded < run.start.saturating_add(run.len)
        }));
        drop(reuse_reader);
        let (allowed, _) = store
            .transaction_reusable_free(&actual_free, None, horizon)
            .unwrap();
        assert!(allowed.iter().any(|run| {
            superseded >= run.start && superseded < run.start.saturating_add(run.len)
        }));
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn segment_gc_publication_failure_preserves_authoritative_state() {
        let path = temp_store("segment-gc-publication-failure");
        let mut store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let objects = (0..300usize)
            .map(|index| {
                let payload = format!("segment-gc-object-{index:04}").into_bytes();
                let digest = store.put(&payload).unwrap();
                (digest, payload)
            })
            .collect::<Vec<_>>();
        append_committed_free_pages(&store, page::PAGES_PER_SEGMENT);

        let affected = objects
            .iter()
            .map(|(digest, _)| *digest)
            .collect::<Vec<_>>();
        let before = maintenance_publication_state(&store, &affected);
        let file_len_before = std::fs::metadata(&path).unwrap().len();
        let live = objects
            .iter()
            .enumerate()
            .filter(|(index, _)| index % 10 == 0)
            .map(|(_, (digest, _))| *digest.bytes())
            .collect::<BTreeSet<_>>();
        let hits = Arc::new(AtomicU64::new(0));
        let injected_hits = Arc::clone(&hits);
        let guard = install_store_publication_failure_test_injector(
            std::path::PathBuf::from(&path),
            Arc::new(move |boundary| {
                assert_eq!(
                    boundary,
                    StorePublicationFailureTestBoundary::SegmentGcBeforeFinishTxn
                );
                injected_hits.fetch_add(1, Ordering::SeqCst);
                Err(LoomError::new(Code::Io, "injected segment GC failure"))
            }),
        );

        let error = store.gc_segments(&live).unwrap_err();
        assert_eq!(error.code, Code::Io);
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert_maintenance_publication_state_eq(
            &maintenance_publication_state(&store, &affected),
            &before,
        );
        assert_object_payloads(&store, &objects);
        assert!(store.has(&objects[1].0).unwrap());
        let file_len_after = std::fs::metadata(&path).unwrap().len();
        assert!(file_len_after >= file_len_before);
        assert_eq!((file_len_after - file_len_before) % PAGE_SIZE, 0);
        eprintln!(
            "segment GC unreachable candidate growth: {} bytes",
            file_len_after - file_len_before
        );

        drop(guard);
        drop(store);
        let reopened = FileStore::open(&path).unwrap();
        assert_maintenance_publication_state_eq(
            &maintenance_publication_state(&reopened, &affected),
            &before,
        );
        assert_object_payloads(&reopened, &objects);
        assert!(reopened.has(&objects[1].0).unwrap());
        drop(reopened);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn tail_compaction_publication_failure_preserves_authoritative_state() {
        let path = temp_store("tail-compaction-publication-failure");
        let mut store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        append_committed_free_pages(&store, 64);
        let first_bytes = b"first shared slab object".to_vec();
        let second_bytes = b"second shared slab object".to_vec();
        let first = Digest::hash(store.digest_algo, &first_bytes);
        let second = Digest::hash(store.digest_algo, &second_bytes);
        store
            .group_commit(&[
                (first, first_bytes.as_slice(), store.default_codec),
                (second, second_bytes.as_slice(), store.default_codec),
            ])
            .unwrap();
        {
            let mut inner = store.inner.lock().unwrap();
            inner.generation = REUSE_SAFE_WINDOW + 10;
        }
        store.set_reference_root(store.reference_root()).unwrap();

        let objects = vec![(first, first_bytes), (second, second_bytes)];
        let affected = vec![first, second];
        let before = maintenance_publication_state(&store, &affected);
        let file_len_before = std::fs::metadata(&path).unwrap().len();
        let hits = Arc::new(AtomicU64::new(0));
        let injected_hits = Arc::clone(&hits);
        let guard = install_store_publication_failure_test_injector(
            std::path::PathBuf::from(&path),
            Arc::new(move |boundary| {
                assert_eq!(
                    boundary,
                    StorePublicationFailureTestBoundary::TailCompactionBeforeFinishTxn
                );
                injected_hits.fetch_add(1, Ordering::SeqCst);
                Err(LoomError::new(Code::Io, "injected tail compaction failure"))
            }),
        );

        let error = store.compact_tail_once(16, 1, 32).unwrap_err();
        assert_eq!(error.code, Code::Io);
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert_maintenance_publication_state_eq(
            &maintenance_publication_state(&store, &affected),
            &before,
        );
        assert_object_payloads(&store, &objects);
        let file_len_after = std::fs::metadata(&path).unwrap().len();
        assert!(file_len_after >= file_len_before);
        assert_eq!((file_len_after - file_len_before) % PAGE_SIZE, 0);
        eprintln!(
            "tail compaction unreachable candidate growth: {} bytes",
            file_len_after - file_len_before
        );

        drop(guard);
        drop(store);
        let reopened = FileStore::open(&path).unwrap();
        assert_maintenance_publication_state_eq(
            &maintenance_publication_state(&reopened, &affected),
            &before,
        );
        assert_object_payloads(&reopened, &objects);
        drop(reopened);
        std::fs::remove_file(path).unwrap();
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

        let expired = || true;
        let error = store
            .index_snapshot_from_evidence(&evidence, None, Some(&expired))
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
            false,
            None,
            None,
        )
    }

    pub fn gc_validated_segments(&mut self, budget: GcSegmentBudget) -> Result<GcStats> {
        self.gc_validated_segments_impl(budget, true, None, None, None, None)
    }

    pub(crate) fn gc_validated_segments_retaining(
        &mut self,
        budget: GcSegmentBudget,
        current_live: &BTreeSet<[u8; 32]>,
    ) -> Result<GcStats> {
        self.gc_validated_segments_impl(budget, true, Some(current_live), None, None, None)
    }

    pub fn gc_validated_segments_without_tail_trim(
        &mut self,
        budget: GcSegmentBudget,
    ) -> Result<GcStats> {
        self.gc_validated_segments_impl(budget, false, None, None, None, None)
    }

    pub fn gc_validated_segments_until(
        &mut self,
        budget: GcSegmentBudget,
        trim_tail: bool,
        deadline: std::time::Instant,
    ) -> Result<GcStats> {
        let expired = || std::time::Instant::now() >= deadline;
        self.gc_validated_segments_impl(budget, trim_tail, None, None, None, Some(&expired))
    }

    pub(crate) fn gc_validated_segments_while(
        &mut self,
        budget: GcSegmentBudget,
        trim_tail: bool,
        deadline_expired: &dyn Fn() -> bool,
    ) -> Result<GcStats> {
        self.gc_validated_segments_impl(budget, trim_tail, None, None, None, Some(deadline_expired))
    }

    #[cfg(test)]
    pub(crate) fn gc_validated_segments_with_pre_reclaim_interleave(
        &mut self,
        budget: GcSegmentBudget,
        mut interleave: impl FnMut(&FileStore) -> Result<()>,
    ) -> Result<GcStats> {
        self.gc_validated_segments_impl(budget, true, None, Some(&mut interleave), None, None)
    }

    #[cfg(test)]
    pub(crate) fn gc_validated_segments_with_read_phase_interleave(
        &mut self,
        budget: GcSegmentBudget,
        mut interleave: impl FnMut(&FileStore) -> Result<()>,
    ) -> Result<GcStats> {
        self.gc_validated_segments_impl(budget, true, None, None, Some(&mut interleave), None)
    }

    fn gc_validated_segments_impl(
        &mut self,
        budget: GcSegmentBudget,
        trim_tail: bool,
        current_live: Option<&BTreeSet<[u8; 32]>>,
        pre_reclaim_interleave: GcInterleave<'_>,
        read_phase_interleave: GcInterleave<'_>,
        deadline: GcDeadline<'_>,
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
        let reclaim_evidence =
            self.active_reachability_mark_reclaim_evidence()
                .and_then(|evidence| {
                    evidence
                        .filter(|evidence| evidence.matches_epoch(&epoch, self.digest_algo))
                        .ok_or_else(|| {
                            LoomError::new(
                                Code::Conflict,
                                "reachability mark reclaim evidence mismatch",
                            )
                        })
                });
        let reclaim_evidence = match reclaim_evidence {
            Ok(evidence) => evidence,
            Err(error) => {
                self.clear_reachability_mark_epoch()?;
                return Err(error);
            }
        };
        let status = self.maintenance_status()?;
        if status.last_validated_mark_epoch < epoch.epoch {
            return Err(LoomError::new(
                Code::Conflict,
                "reachability mark epoch is not validated",
            ));
        }
        let candidates = status
            .candidate_segments
            .into_iter()
            .collect::<BTreeSet<_>>();
        let current_page_count = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            inner.page_count
        };
        let metadata_reclaim_pages = self.reachability_mark_metadata_reclaim_candidate_pages(
            &reclaim_evidence,
            current_page_count,
            budget.max_pages,
        )?;
        let candidates = match reclaim_evidence
            .unreachable_pre_snapshot_pages
            .iter()
            .chain(metadata_reclaim_pages.iter())
            .map(|page| page / page::PAGES_PER_SEGMENT)
            .collect::<BTreeSet<_>>()
        {
            evidence_segments if evidence_segments.is_empty() => candidates,
            evidence_segments => candidates.union(&evidence_segments).copied().collect(),
        };
        if candidates.is_empty() || budget.max_segments == 0 || budget.max_pages == 0 {
            if !candidates.is_empty() || (budget.max_segments > 0 && budget.max_pages > 0) {
                self.clear_reachability_mark_epoch()?;
            }
            return Ok(GcStats::default());
        }
        if let Some(interleave) = pre_reclaim_interleave {
            interleave(self)?;
        }
        let mut retain = epoch.retain_set();
        if let Some(current_live) = current_live {
            retain.extend(current_live);
        }
        let stats = self.gc_segments_inner(
            &retain,
            Some(&candidates),
            budget,
            trim_tail,
            Some((&epoch, &reclaim_evidence, metadata_reclaim_pages)),
            current_live.is_none(),
            read_phase_interleave,
            deadline,
        )?;
        self.clear_reachability_mark_epoch()?;
        Ok(stats)
    }

    fn gc_segments_inner(
        &mut self,
        live: &BTreeSet<[u8; 32]>,
        eligible_segments: Option<&BTreeSet<u64>>,
        budget: GcSegmentBudget,
        trim_tail: bool,
        validated_epoch: Option<(
            &ReachabilityMarkEpoch,
            &ReachabilityMarkReclaimEvidence,
            BTreeSet<u64>,
        )>,
        protect_post_snapshot: bool,
        read_phase_interleave: GcInterleave<'_>,
        deadline: GcDeadline<'_>,
    ) -> Result<GcStats> {
        check_gc_deadline(deadline)?;
        let codec = self.default_codec; // re-frame relocated records per the current default
        let (evidence, keep_reference, mut keep_control, keep_derived) = {
            let mut inner = lock_until(&self.inner, deadline)?;
            let control_map = self.control_map_locked(&mut inner)?;
            let evidence = self.gc_reclaim_evidence_locked(&inner, &control_map)?;
            (
                evidence,
                inner.reference_root.map(|d| *d.bytes()),
                inner.control_root.map(|d| *d.bytes()),
                self.derived_payload_digests_from_control_map(&control_map)?,
            )
        };
        let index_snapshot =
            self.index_snapshot_from_evidence(&evidence, read_phase_interleave, deadline)?;
        let captured_free_protected = if let Some((epoch, _, _)) = validated_epoch.as_ref() {
            let mut file = lock_until(&self.file, deadline)?;
            crate::mark_epoch::captured_free_consumed_runs(&mut **file, self.digest_algo, epoch)?
        } else {
            Vec::new()
        };
        let alive = |digest: &[u8; 32]| {
            live.contains(digest)
                || keep_reference.as_ref() == Some(digest)
                || keep_control.as_ref() == Some(digest)
                || keep_derived.contains(digest)
        };
        let protected_by_epoch = |pages: &[u64]| {
            protect_post_snapshot
                && validated_epoch.as_ref().is_some_and(|(epoch, _, _)| {
                    pages.iter().any(|page| {
                        *page >= epoch.page_high_water_mark
                            || captured_free_protected.iter().any(|run| {
                                *page >= run.start && *page < run.start.saturating_add(run.len)
                            })
                    })
                })
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
            let record_live = alive(digest) || protected_by_epoch(&pages);
            for page in &pages {
                *page_live.entry(*page).or_insert(false) |= record_live;
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
        if let Some((_, reclaim_evidence, metadata_reclaim_pages)) = validated_epoch.as_ref() {
            for page in &reclaim_evidence.unreachable_pre_snapshot_pages {
                let e = occupancy
                    .entry(page / page::PAGES_PER_SEGMENT)
                    .or_insert((0, 0));
                e.1 += 1;
            }
            for page in metadata_reclaim_pages {
                let e = occupancy
                    .entry(page / page::PAGES_PER_SEGMENT)
                    .or_insert((0, 0));
                e.1 += 1;
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
        let mut pages_to_free: BTreeSet<u64> = validated_epoch
            .as_ref()
            .map(|(_, reclaim_evidence, metadata_reclaim_pages)| {
                reclaim_evidence
                    .unreachable_pre_snapshot_pages
                    .union(metadata_reclaim_pages)
                    .copied()
                    .collect()
            })
            .unwrap_or_default();
        for (digest, loc) in &index_snapshot {
            check_gc_deadline(deadline)?;
            let pages = &record_pages[digest];
            let touches_chosen = pages
                .iter()
                .any(|page| chosen.contains(&(page / page::PAGES_PER_SEGMENT)));
            if !touches_chosen {
                continue;
            }
            let record_live = alive(digest) || protected_by_epoch(pages);
            let touches_evacuation = pages
                .iter()
                .any(|page| evacuation_segments.contains(&(page / page::PAGES_PER_SEGMENT)));
            if record_live && touches_evacuation {
                pages_to_free.extend(pages);
                let d = Digest::of(self.digest_algo, *digest);
                let payload = self
                    .read_indexed_payload_snapshot(loc, evidence.page_count, &d)?
                    .ok_or_else(|| corrupt("live object missing during gc"))?;
                survivors.push((d, payload));
            } else if !record_live {
                dropped.push(*digest);
                pages_to_free.extend(
                    pages
                        .iter()
                        .copied()
                        .filter(|page| !page_live.get(page).copied().unwrap_or(false)),
                );
            }
        }
        if survivors.is_empty() && dropped.is_empty() && pages_to_free.is_empty() {
            return Ok(GcStats::default());
        }

        check_gc_deadline(deadline)?;

        // Phase B: one transaction - relocate survivors to fresh pages, point-update their index
        // entries, delete the dropped keys, and free the reclaimed segments' pages.
        let mut inner = lock_until(&self.inner, deadline)?;
        let mut control_map = self.control_map_locked(&mut inner)?;
        let current_evidence = self.gc_reclaim_evidence_locked(&inner, &control_map)?;
        let mut current_metadata_pages =
            self.current_metadata_live_pages_locked(&inner, &control_map)?;
        if let Some((epoch, _, _)) = validated_epoch.as_ref() {
            current_metadata_pages.extend(epoch.captured_metadata_bootstrap_reserve.pages());
        }
        if validated_epoch.is_none() && current_evidence != evidence {
            return Err(LoomError::new(
                Code::Conflict,
                "store changed during segment gc",
            ));
        }
        let new_gen = inner.generation + 1;
        self.materialize_index_locked(&mut inner)?;
        let before_page_count = evidence.page_count;
        let (reusable_free, _reclamation_lease) = self.transaction_reusable_free(
            &inner.free,
            inner.active_mark_epoch_reclaim_fence,
            inner.minimum_recoverable_generation,
        )?;
        if !_reclamation_lease.allowed {
            return Err(LoomError::new(
                Code::Conflict,
                "loom-store: active readers block physical reclamation",
            ));
        }
        let relocated_object_count = survivors.len() as u64;
        let (roots, placements, pages_freed) = {
            let mut file = lock_until(&self.file, deadline)?;
            let captured_free_authority = if let Some((epoch, _, _)) = validated_epoch.as_ref() {
                Some(
                    crate::mark_epoch::captured_free_reuse_runs(
                        &mut **file,
                        self.digest_algo,
                        epoch,
                        &inner.free,
                        inner.minimum_recoverable_generation,
                        usize::MAX,
                    )?
                    .allocation_authority,
                )
            } else {
                None
            };
            let mut alloc = PageAllocator::new_with_reusable_runs(
                inner.page_count,
                new_gen,
                inner.free.clone(),
                reusable_free,
            );
            if let Some(authority) = captured_free_authority {
                alloc.install_captured_free_authority(authority)?;
            }
            alloc.install_metadata_bootstrap_reserve(&inner.metadata_bootstrap_reserve)?;
            let borrowed: Vec<(Digest, &[u8], Codec)> = survivors
                .iter()
                .map(|(d, p)| (*d, p.as_slice(), codec))
                .collect();
            // Survivors are re-sealed under the current DEK as they are relocated, so GC never
            // demotes an encrypted store to plaintext frames.
            let dek = self.dek.lock().map_err(|_| poisoned())?;
            let mut placements =
                write_record_pages(&mut **file, &mut alloc, &borrowed, dek.as_ref())?;
            drop(dek);
            let index_batch = pagebtree::batch_upsert(
                &mut **file,
                DATA_START,
                &mut alloc,
                inner.index_root,
                &placements,
                inner.page_count,
            )?;
            #[cfg(any(test, feature = "test-hooks"))]
            observe_object_index_batch(index_batch.stats);
            let mut index_root = index_batch.root;
            for key in &dropped {
                let bound = alloc.page_count();
                index_root =
                    pagebtree::delete(&mut **file, DATA_START, &mut alloc, index_root, key, bound)?;
            }
            if let Some((epoch, _, _)) = validated_epoch.as_ref() {
                let consumed_through = alloc
                    .captured_free_consumed_through()
                    .unwrap_or(epoch.captured_free_consumed_through);
                if consumed_through > epoch.captured_free_consumed_through {
                    crate::mark_epoch::advance_captured_free_consumption_in_control_map(
                        &mut control_map,
                        epoch,
                        consumed_through,
                        self.digest_algo,
                    )?;
                    let control_bytes = crate::record_io::encode_control_map(&control_map);
                    let control_digest = Digest::hash(self.digest_algo, &control_bytes);
                    keep_control = Some(*control_digest.bytes());
                    alloc.activate_publication_reserve();
                    if pagebtree::get(
                        &mut **file,
                        DATA_START,
                        index_root,
                        control_digest.bytes(),
                        alloc.page_count(),
                    )?
                    .is_none()
                    {
                        let control_records =
                            [(control_digest, control_bytes.as_slice(), self.default_codec)];
                        let dek = self.dek.lock().map_err(|_| poisoned())?;
                        let control_placements = write_record_pages(
                            &mut **file,
                            &mut alloc,
                            &control_records,
                            dek.as_ref(),
                        )?;
                        drop(dek);
                        let control_index_bound = alloc.page_count();
                        let control_index_batch = pagebtree::batch_upsert(
                            &mut **file,
                            DATA_START,
                            &mut alloc,
                            index_root,
                            &control_placements,
                            control_index_bound,
                        )?;
                        #[cfg(any(test, feature = "test-hooks"))]
                        observe_object_index_batch(control_index_batch.stats);
                        index_root = control_index_batch.root;
                        placements.extend(control_placements);
                    }
                }
            }
            let touched_segments: BTreeSet<u64> =
                placements.iter().map(|(_, loc)| loc.segment_id).collect();
            // The pages were never in the seeded free list, so survivor/index writes above could not
            // have reused them.
            let mut pages_freed = 0u64;
            let already_free = |page: u64| {
                inner
                    .free
                    .iter()
                    .any(|run| page >= run.start && page < run.start.saturating_add(run.len))
            };
            for &p in &pages_to_free {
                if already_free(p)
                    || alloc.allocated_in_transaction(p)
                    || current_metadata_pages.contains(&p)
                {
                    continue;
                }
                if let Some((epoch, reclaim_evidence, metadata_reclaim_pages)) =
                    validated_epoch.as_ref()
                    && (p >= epoch.page_high_water_mark
                        || (!reclaim_evidence.unreachable_pre_snapshot_pages.contains(&p)
                            && !metadata_reclaim_pages.contains(&p)))
                {
                    continue;
                }
                alloc.free(PageId(p), 1)?;
                pages_freed += 1;
            }
            let object_count = inner
                .maintenance
                .object_count
                .saturating_sub(dropped.len() as u64);
            #[cfg(any(test, feature = "test-hooks"))]
            invoke_store_publication_failure_test_injector(
                &self.path,
                StorePublicationFailureTestBoundary::SegmentGcBeforeFinishTxn,
            )?;
            let roots = finish_txn(
                &mut **file,
                &mut alloc,
                new_gen,
                object_count,
                TxnRootInputs {
                    object_index: index_root,
                    legacy_overlay: legacy_overlay_root_for_publication(
                        &inner,
                        inner.current_record_root,
                        inner.root_catalog_root,
                    ),
                    current_records: inner.current_record_root,
                    root_catalog: TxnRootCatalog {
                        root: inner.root_catalog_root,
                        entries: inner.root_catalog_entries.clone(),
                    },
                    previous_mutable_overlay_generation_floor: inner
                        .mutable_overlay_generation_floor,
                    mutable_overlay_generation_floor: inner.mutable_overlay_generation_floor,
                    reference: keep_reference,
                    control: keep_control,
                },
                inner.open_segment,
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
            (roots, placements, pages_freed)
        };

        let pages_trimmed = before_page_count.saturating_sub(roots.page_count);
        self.adopt_committed_roots_locked(&mut inner, roots)?;
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
            objects_relocated: relocated_object_count,
            objects_dropped: dropped.len() as u64,
        };
        if trim_tail && stats.pages_freed > 0 {
            stats.pages_trimmed = stats
                .pages_trimmed
                .saturating_add(self.trim_tail_free_pages()?);
        }
        Ok(stats)
    }

    fn current_metadata_live_pages_locked(
        &self,
        inner: &crate::Inner,
        control_map: &BTreeMap<Vec<u8>, Vec<u8>>,
    ) -> Result<BTreeSet<u64>> {
        let page_count = inner.page_count;
        let mut tree_roots = BTreeMap::new();
        let mut value_roots = BTreeMap::new();
        let mut insert_root = |root: Option<PageId>, codec| -> Result<()> {
            let Some(root) = root else {
                return Ok(());
            };
            if tree_roots
                .insert(root, codec)
                .is_some_and(|known| known != codec)
                || value_roots
                    .insert(root, codec)
                    .is_some_and(|known| known != codec)
            {
                return Err(corrupt(
                    "canonical metadata root has conflicting value codecs",
                ));
            }
            Ok(())
        };
        insert_root(inner.index_root, pagebtree::ValueCodecKind::RecordLoc)?;
        insert_root(inner.overlay_root, pagebtree::ValueCodecKind::RecordLoc)?;
        insert_root(
            inner.current_record_root,
            crate::root_family_value_codec(CURRENT_RECORDS_FAMILY_ID)?,
        )?;
        for entry in &inner.root_catalog_entries {
            insert_root(
                Some(entry.root),
                crate::root_family_value_codec(entry.family_id)?,
            )?;
        }
        if let Some(epoch) =
            crate::mark_epoch::active_mark_epoch_from_control_map(&control_map, self.digest_algo)?
            && let Some(root) = epoch.metadata_evidence_root
        {
            let root = PageId(root);
            insert_root(Some(root), pagebtree::ValueCodecKind::RecordLoc)?;
        }
        let mut pages = BTreeSet::new();
        pages.extend(inner.metadata_bootstrap_reserve.pages());
        pages.extend(inner.region_table_root.map(|page| page.0));
        pages.extend(inner.maintenance_root.map(|page| page.0));
        pages.extend(inner.root_catalog_root.map(|page| page.0));
        let freemap_root = inner.freemap.map(|(root, _)| root);
        let mut file = self.file.lock().map_err(|_| poisoned())?;
        for (root, codec) in tree_roots {
            pages.extend(
                pagebtree::collect_pages_with_codec(
                    &mut **file,
                    DATA_START,
                    root,
                    page_count,
                    codec,
                )?
                .into_iter()
                .map(|page| page.0),
            );
        }
        for (root, codec) in value_roots {
            for (_, loc) in
                pagebtree::load_all_with_codec(&mut **file, DATA_START, root, page_count, codec)?
            {
                pages.extend(crate::record_io::blob_pages(
                    &mut **file,
                    loc.global_page(),
                    page_count,
                )?);
            }
        }
        if let Some(root) = freemap_root {
            pages.extend(
                pagebtree::collect_free_page_extent_pages(
                    &mut **file,
                    DATA_START,
                    root,
                    page_count,
                )?
                .into_iter()
                .map(|page| page.0),
            );
        }
        Ok(pages)
    }

    pub(crate) fn trim_tail_free_pages(&mut self) -> Result<u64> {
        let _reclamation_lease = self.try_reclamation_write_lease()?;
        if !_reclamation_lease.allowed {
            return Ok(0);
        }
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
            alloc.install_metadata_bootstrap_reserve(&inner.metadata_bootstrap_reserve)?;
            finish_txn(
                &mut **file,
                &mut alloc,
                new_gen,
                inner.maintenance.object_count,
                TxnRootInputs {
                    object_index: inner.index_root,
                    legacy_overlay: legacy_overlay_root_for_publication(
                        &inner,
                        inner.current_record_root,
                        inner.root_catalog_root,
                    ),
                    current_records: inner.current_record_root,
                    root_catalog: TxnRootCatalog {
                        root: inner.root_catalog_root,
                        entries: inner.root_catalog_entries.clone(),
                    },
                    previous_mutable_overlay_generation_floor: inner
                        .mutable_overlay_generation_floor,
                    mutable_overlay_generation_floor: inner.mutable_overlay_generation_floor,
                    reference: inner.reference_root.map(|d| *d.bytes()),
                    control: inner.control_root.map(|d| *d.bytes()),
                },
                inner.open_segment,
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
        if trimmed > 0 {
            self.file
                .lock()
                .map_err(|_| poisoned())?
                .grow(DATA_START + roots.page_count * PAGE_SIZE)
                .map_err(io_err)?;
        }
        self.adopt_committed_roots_locked(&mut inner, roots)?;
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
        let expired = || std::time::Instant::now() >= deadline;
        self.compact_tail_once_impl(max_pages, max_objects, max_bytes, None, Some(&expired))
    }

    pub(crate) fn compact_tail_once_while(
        &mut self,
        max_pages: u64,
        max_objects: u64,
        max_bytes: u64,
        deadline_expired: &dyn Fn() -> bool,
    ) -> Result<TailCompactionStats> {
        self.compact_tail_once_impl(
            max_pages,
            max_objects,
            max_bytes,
            None,
            Some(deadline_expired),
        )
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
        deadline: GcDeadline<'_>,
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
        let (reusable_free, _reclamation_lease) = self.transaction_reusable_free(
            &inner.free,
            inner.active_mark_epoch_reclaim_fence,
            inner.minimum_recoverable_generation,
        )?;
        if !_reclamation_lease.allowed {
            return Ok(TailCompactionStats {
                attempted: true,
                conflicts: 1,
                skipped: true,
                ..TailCompactionStats::default()
            });
        }
        let (roots, placements, relocated_pages) = {
            let mut file = lock_until(&self.file, deadline)?;
            let mut alloc = PageAllocator::new_reusing_before(
                inner.page_count,
                new_gen,
                reusable_free,
                scan_start,
            );
            alloc.install_metadata_bootstrap_reserve(&inner.metadata_bootstrap_reserve)?;
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
            let index_batch = pagebtree::batch_upsert(
                &mut **file,
                DATA_START,
                &mut alloc,
                inner.index_root,
                &placements,
                inner.page_count,
            )?;
            #[cfg(any(test, feature = "test-hooks"))]
            observe_object_index_batch(index_batch.stats);
            let index_root = index_batch.root;
            let mut relocated_pages = 0u64;
            for page in &selected_page_set {
                alloc.free(PageId(*page), 1)?;
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
            #[cfg(any(test, feature = "test-hooks"))]
            invoke_store_publication_failure_test_injector(
                &self.path,
                StorePublicationFailureTestBoundary::TailCompactionBeforeFinishTxn,
            )?;
            let roots = finish_txn(
                &mut **file,
                &mut alloc,
                new_gen,
                inner.maintenance.object_count,
                TxnRootInputs {
                    object_index: index_root,
                    legacy_overlay: legacy_overlay_root_for_publication(
                        &inner,
                        inner.current_record_root,
                        inner.root_catalog_root,
                    ),
                    current_records: inner.current_record_root,
                    root_catalog: TxnRootCatalog {
                        root: inner.root_catalog_root,
                        entries: inner.root_catalog_entries.clone(),
                    },
                    previous_mutable_overlay_generation_floor: inner
                        .mutable_overlay_generation_floor,
                    mutable_overlay_generation_floor: inner.mutable_overlay_generation_floor,
                    reference: keep_reference,
                    control: keep_control,
                },
                inner.open_segment,
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
            (roots, placements, relocated_pages)
        };
        let root_page_count = roots.page_count;
        self.adopt_committed_roots_locked(&mut inner, roots)?;
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
        deadline: GcDeadline<'_>,
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
            if deadline.is_some_and(|deadline| deadline()) {
                self.save_index_scan_state(evidence_key, &cursor, &out)?;
                check_gc_deadline(deadline)?;
            }
            let step = pagebtree::scan_step_with_page_reader(
                &mut cursor,
                evidence.page_count,
                64,
                None,
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
            if deadline.is_some_and(|deadline| deadline()) {
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
        put_optional_digest_bytes(&mut bytes, evidence.canonical_roots_fingerprint);
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

    fn canonical_compaction_plan_from_evidence(
        &self,
        evidence: &GcReclaimEvidence,
        live: &BTreeSet<[u8; 32]>,
        eligible_segments: Option<&BTreeSet<u64>>,
        budget: GcSegmentBudget,
        deadline: GcDeadline<'_>,
    ) -> Result<GcCanonicalCompactionPlan> {
        let current_evidence = {
            let mut inner = lock_until(&self.inner, deadline)?;
            let control_map = self.control_map_locked(&mut inner)?;
            self.gc_reclaim_evidence_locked(&inner, &control_map)?
        };
        if &current_evidence != evidence {
            return Err(LoomError::new(
                Code::Conflict,
                "canonical compaction evidence is stale",
            ));
        }

        let mut roots = Vec::new();
        let mut page_candidates = BTreeMap::<u64, GcCompactionPageAccumulator>::new();
        for root in &evidence.canonical_roots {
            let classification = root_compaction_classification(root);
            roots.push(GcCompactionRootPlan {
                name: root.name.clone(),
                family_id: root.family_id,
                page_root: root.page_root,
                digest_root: root.digest_root,
                classification,
            });
            if let Some(page) = root.page_root {
                add_compaction_page_candidate(
                    &mut page_candidates,
                    page.0,
                    root.name.clone(),
                    classification,
                );
            }
        }

        let index_snapshot = self.index_snapshot_from_evidence(evidence, None, deadline)?;
        let alive = |digest: &[u8; 32]| {
            live.contains(digest)
                || evidence
                    .reference_root
                    .as_ref()
                    .is_some_and(|root| root.bytes() == digest)
                || evidence
                    .control_root
                    .as_ref()
                    .is_some_and(|root| root.bytes() == digest)
                || evidence
                    .derived_roots
                    .iter()
                    .any(|root| root.bytes() == digest)
        };
        let mut file = lock_until(&self.file, deadline)?;
        for (digest, loc) in &index_snapshot {
            check_gc_deadline(deadline)?;
            let pages =
                crate::record_io::blob_pages(&mut **file, loc.global_page(), evidence.page_count)?;
            let live_object = alive(digest);
            for page in pages {
                let segment = page / page::PAGES_PER_SEGMENT;
                if let Some(segments) = eligible_segments
                    && !segments.contains(&segment)
                {
                    continue;
                }
                let owner = digest_owner_label(digest);
                let classification = if live_object {
                    GcCompactionClassification::SemanticLiveness
                } else {
                    GcCompactionClassification::ReclaimNeutral
                };
                add_compaction_page_candidate(&mut page_candidates, page, owner, classification);
            }
        }
        drop(file);

        let mut page_candidates = page_candidates
            .into_values()
            .map(GcCompactionPageAccumulator::into_candidate)
            .collect::<Vec<_>>();
        page_candidates.sort_by_key(|candidate| candidate.page);
        let mut eligible_budget = budget.max_pages;
        for candidate in &mut page_candidates {
            if candidate.eligible {
                if eligible_budget == 0 {
                    candidate.eligible = false;
                    candidate.blocker = Some("page_budget_exhausted".to_string());
                } else {
                    eligible_budget = eligible_budget.saturating_sub(1);
                }
            }
        }
        if budget.max_segments != u64::MAX {
            let allowed = page_candidates
                .iter()
                .filter(|candidate| candidate.eligible)
                .map(|candidate| candidate.segment)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .take(budget.max_segments as usize)
                .collect::<BTreeSet<_>>();
            for candidate in &mut page_candidates {
                if candidate.eligible && !allowed.contains(&candidate.segment) {
                    candidate.eligible = false;
                    candidate.blocker = Some("segment_budget_exhausted".to_string());
                }
            }
        }
        let eligible_pages = page_candidates
            .iter()
            .filter(|candidate| candidate.eligible)
            .count() as u64;
        let blocked_pages = page_candidates
            .iter()
            .filter(|candidate| !candidate.eligible)
            .count() as u64;

        Ok(GcCanonicalCompactionPlan {
            evidence: evidence.clone(),
            roots,
            page_candidates,
            eligible_pages,
            blocked_pages,
        })
    }

    pub fn canonical_compaction_plan(
        &self,
        live: &BTreeSet<[u8; 32]>,
        budget: GcSegmentBudget,
    ) -> Result<GcCanonicalCompactionPlan> {
        let evidence = {
            let mut inner = self.inner.lock().map_err(|_| poisoned())?;
            let control_map = self.control_map_locked(&mut inner)?;
            self.gc_reclaim_evidence_locked(&inner, &control_map)?
        };
        self.canonical_compaction_plan_from_evidence(&evidence, live, None, budget, None)
    }

    fn canonical_relocate_from_evidence(
        &self,
        evidence: &GcReclaimEvidence,
        live: &BTreeSet<[u8; 32]>,
        budget: GcSegmentBudget,
        reclaim_source_pages: bool,
        mut pre_publish_interleave: GcInterleave<'_>,
        pre_commit_hook: GcPreCommitHook<'_>,
        deadline: GcDeadline<'_>,
    ) -> Result<GcCanonicalRelocationStats> {
        let reclamation_lease = self.try_reclamation_write_lease()?;
        if !reclamation_lease.allowed {
            return Err(LoomError::new(
                Code::Conflict,
                "loom-store: active readers block canonical relocation",
            ));
        }
        let plan =
            self.canonical_compaction_plan_from_evidence(evidence, live, None, budget, deadline)?;
        let candidate_by_page = plan
            .page_candidates
            .iter()
            .map(|candidate| (candidate.page, candidate))
            .collect::<BTreeMap<_, _>>();
        let reclaim_pages = plan
            .page_candidates
            .iter()
            .filter(|candidate| candidate.eligible)
            .map(|candidate| candidate.page)
            .collect::<BTreeSet<_>>();
        let index_snapshot = self.index_snapshot_from_evidence(evidence, None, deadline)?;
        let mut preserve = Vec::<(Digest, Vec<u8>)>::new();
        let mut dropped = Vec::<[u8; 32]>::new();
        for (digest, loc) in &index_snapshot {
            check_gc_deadline(deadline)?;
            let pages = {
                let mut file = lock_until(&self.file, deadline)?;
                crate::record_io::blob_pages(&mut **file, loc.global_page(), evidence.page_count)?
            };
            let drop_record = pages.iter().all(|page| {
                candidate_by_page
                    .get(page)
                    .is_some_and(|candidate| candidate.eligible)
            });
            if drop_record {
                dropped.push(*digest);
            } else {
                let digest = Digest::of(self.digest_algo, *digest);
                let payload = self
                    .read_indexed_payload_snapshot(loc, evidence.page_count, &digest)?
                    .ok_or_else(|| {
                        corrupt("relocated object missing during canonical compaction")
                    })?;
                preserve.push((digest, payload));
            }
        }

        if let Some(interleave) = pre_publish_interleave.as_mut() {
            interleave(self)?;
        }

        let mut inner = lock_until(&self.inner, deadline)?;
        let control_map = self.control_map_locked(&mut inner)?;
        let current_evidence = self.gc_reclaim_evidence_locked(&inner, &control_map)?;
        if &current_evidence != evidence {
            return Err(LoomError::new(
                Code::Conflict,
                "canonical compaction evidence is stale",
            ));
        }
        self.materialize_index_locked(&mut inner)?;
        let new_gen = inner.generation + 1;
        let source_page_count = inner.page_count;
        let source_current_root = inner.current_record_root;
        let source_legacy_overlay_root = inner.overlay_root;
        let source_root_catalog_entries = inner.root_catalog_entries.clone();
        let keep_reference = inner.reference_root.map(|digest| *digest.bytes());
        let keep_control = inner.control_root.map(|digest| *digest.bytes());
        let source_freemap = inner.freemap;
        let source_region_table = inner.region_table_root;
        let source_maintenance = inner.maintenance_root;
        let source_free = inner.free.clone();
        let source_maintenance_state = inner.maintenance.clone();
        let source_encryption_meta = inner.encryption_meta.clone();

        let (roots, placements, destination_page_count, pages_reclaimed) = {
            let mut file = lock_until(&self.file, deadline)?;
            let current_records = root_records_from_btree(
                &mut **file,
                Some(CURRENT_RECORDS_FAMILY_ID),
                source_current_root,
                source_page_count,
            )?;
            let legacy_records = root_records_from_btree(
                &mut **file,
                None,
                source_legacy_overlay_root,
                source_page_count,
            )?;
            let catalog_records = source_root_catalog_entries
                .iter()
                .map(|entry| {
                    root_records_from_btree(
                        &mut **file,
                        Some(entry.family_id),
                        Some(entry.root),
                        source_page_count,
                    )
                    .map(|records| (entry.family_id, entry.flags, records))
                })
                .collect::<Result<Vec<_>>>()?;
            let mut alloc =
                PageAllocator::new_reusing_before(source_page_count, new_gen, source_free, 0);
            alloc.install_metadata_bootstrap_reserve(&inner.metadata_bootstrap_reserve)?;
            let borrowed = preserve
                .iter()
                .map(|(digest, payload)| (*digest, payload.as_slice(), self.default_codec))
                .collect::<Vec<_>>();
            let dek = self.dek.lock().map_err(|_| poisoned())?;
            let placements = write_record_pages(&mut **file, &mut alloc, &borrowed, dek.as_ref())?;
            drop(dek);
            let mut entries = placements.clone();
            entries.sort_unstable_by_key(|entry| entry.0);
            let index_root =
                pagebtree::build_packed(&mut **file, DATA_START, &mut alloc, &entries)?;
            let current_refs = record_refs(&current_records);
            let current_page_count = alloc.page_count();
            let (current_record_root, _) = write_mutable_record_refs_to_root(
                &mut **file,
                &mut alloc,
                None,
                current_page_count,
                &current_refs,
                None,
                false,
            )?;
            let legacy_overlay_root =
                build_record_family_root(&mut **file, &mut alloc, None, &legacy_records)?;
            let mut root_catalog_entries = Vec::new();
            for (family_id, flags, records) in &catalog_records {
                let root =
                    build_record_family_root(&mut **file, &mut alloc, Some(*family_id), records)?;
                if let Some(root) = root {
                    if *flags == ROOT_FLAG_ADVISORY {
                        root_catalog_entries.push(RootCatalogEntry::advisory(*family_id, root));
                    } else {
                        root_catalog_entries
                            .push(RootCatalogEntry::authoritative(*family_id, root));
                    }
                }
            }
            root_catalog_entries.sort_by_key(|entry| entry.family_id);
            let root_catalog_root = write_root_catalog_page(
                &mut **file,
                &mut alloc,
                None,
                inner.page_count,
                &root_catalog_entries,
            )?;
            let touched_segments = placements
                .iter()
                .map(|(_, loc)| loc.segment_id)
                .collect::<BTreeSet<_>>();
            let pages_reclaimed = if reclaim_source_pages {
                for page in &reclaim_pages {
                    alloc.defer_free(PageId(*page), 1)?;
                }
                reclaim_pages.len() as u64
            } else {
                0
            };
            let roots = finish_txn_with_pre_commit_hook(
                &mut **file,
                &mut alloc,
                new_gen,
                preserve.len() as u64,
                TxnRootInputs {
                    object_index: index_root,
                    legacy_overlay: legacy_overlay_root,
                    current_records: current_record_root,
                    root_catalog: TxnRootCatalog {
                        root: root_catalog_root,
                        entries: root_catalog_entries,
                    },
                    previous_mutable_overlay_generation_floor: inner
                        .mutable_overlay_generation_floor,
                    mutable_overlay_generation_floor: inner.mutable_overlay_generation_floor,
                    reference: keep_reference,
                    control: keep_control,
                },
                inner.open_segment,
                &source_maintenance_state,
                &touched_segments,
                (None, None, None),
                source_encryption_meta,
                self.digest_algo,
                None,
                pre_commit_hook,
            )?;
            let _ = (source_freemap, source_region_table, source_maintenance);
            (roots, placements, alloc.page_count(), pages_reclaimed)
        };

        self.adopt_committed_roots_locked(&mut inner, roots)?;
        inner.index.clear();
        inner.locator_cache_order.clear();
        inner.index_materialized = true;
        for (key, loc) in &placements {
            Self::cache_locator_locked(&mut inner, *key, *loc);
        }
        Ok(GcCanonicalRelocationStats {
            objects_preserved: preserve.len() as u64,
            objects_dropped: dropped.len() as u64,
            root_pages_rebuilt: plan
                .roots
                .iter()
                .filter(|root| root.page_root.is_some())
                .count() as u64,
            pages_reclaimed,
            source_page_count,
            destination_page_count,
            conflicts: 0,
        })
    }

    pub fn canonical_compaction_relocate(
        &self,
        live: &BTreeSet<[u8; 32]>,
        budget: GcSegmentBudget,
    ) -> Result<GcCanonicalRelocationStats> {
        let evidence = {
            let mut inner = self.inner.lock().map_err(|_| poisoned())?;
            let control_map = self.control_map_locked(&mut inner)?;
            self.gc_reclaim_evidence_locked(&inner, &control_map)?
        };
        self.canonical_relocate_from_evidence(&evidence, live, budget, false, None, None, None)
    }

    pub fn canonical_compaction_reclaim(
        &self,
        live: &BTreeSet<[u8; 32]>,
        budget: GcSegmentBudget,
    ) -> Result<GcCanonicalRelocationStats> {
        let evidence = {
            let mut inner = self.inner.lock().map_err(|_| poisoned())?;
            let control_map = self.control_map_locked(&mut inner)?;
            self.gc_reclaim_evidence_locked(&inner, &control_map)?
        };
        self.canonical_relocate_from_evidence(&evidence, live, budget, true, None, None, None)
    }

    #[cfg(test)]
    pub(crate) fn canonical_compaction_plan_from_evidence_for_test(
        &self,
        evidence: &GcReclaimEvidence,
        live: &BTreeSet<[u8; 32]>,
        budget: GcSegmentBudget,
    ) -> Result<GcCanonicalCompactionPlan> {
        self.canonical_compaction_plan_from_evidence(evidence, live, None, budget, None)
    }

    #[cfg(test)]
    pub(crate) fn canonical_compaction_relocate_from_evidence_for_test(
        &self,
        evidence: &GcReclaimEvidence,
        live: &BTreeSet<[u8; 32]>,
        budget: GcSegmentBudget,
    ) -> Result<GcCanonicalRelocationStats> {
        self.canonical_relocate_from_evidence(evidence, live, budget, false, None, None, None)
    }

    #[cfg(test)]
    pub(crate) fn canonical_compaction_reclaim_from_evidence_for_test(
        &self,
        evidence: &GcReclaimEvidence,
        live: &BTreeSet<[u8; 32]>,
        budget: GcSegmentBudget,
    ) -> Result<GcCanonicalRelocationStats> {
        self.canonical_relocate_from_evidence(evidence, live, budget, true, None, None, None)
    }

    #[cfg(test)]
    pub(crate) fn canonical_compaction_relocate_with_pre_publish_interleave_for_test(
        &self,
        live: &BTreeSet<[u8; 32]>,
        budget: GcSegmentBudget,
        mut interleave: impl FnMut(&FileStore) -> Result<()>,
    ) -> Result<GcCanonicalRelocationStats> {
        let evidence = self.gc_reclaim_evidence_for_test()?;
        self.canonical_relocate_from_evidence(
            &evidence,
            live,
            budget,
            false,
            Some(&mut interleave),
            None,
            None,
        )
    }

    #[cfg(test)]
    pub(crate) fn canonical_compaction_reclaim_with_pre_publish_interleave_for_test(
        &self,
        live: &BTreeSet<[u8; 32]>,
        budget: GcSegmentBudget,
        mut interleave: impl FnMut(&FileStore) -> Result<()>,
    ) -> Result<GcCanonicalRelocationStats> {
        let evidence = self.gc_reclaim_evidence_for_test()?;
        self.canonical_relocate_from_evidence(
            &evidence,
            live,
            budget,
            true,
            Some(&mut interleave),
            None,
            None,
        )
    }

    #[cfg(test)]
    pub(crate) fn canonical_compaction_reclaim_with_pre_commit_hook_for_test(
        &self,
        live: &BTreeSet<[u8; 32]>,
        budget: GcSegmentBudget,
        mut hook: impl FnMut() -> Result<()>,
    ) -> Result<GcCanonicalRelocationStats> {
        let evidence = self.gc_reclaim_evidence_for_test()?;
        self.canonical_relocate_from_evidence(
            &evidence,
            live,
            budget,
            true,
            None,
            Some(&mut hook),
            None,
        )
    }

    pub(crate) fn gc_reclaim_evidence_locked(
        &self,
        inner: &Inner,
        control_map: &BTreeMap<Vec<u8>, Vec<u8>>,
    ) -> Result<GcReclaimEvidence> {
        let canonical_roots = self.gc_canonical_roots_locked(inner, control_map);
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
            canonical_roots_fingerprint: self.gc_canonical_roots_fingerprint(&canonical_roots),
            canonical_roots,
        })
    }

    pub(crate) fn gc_canonical_roots_locked(
        &self,
        inner: &Inner,
        control_map: &BTreeMap<Vec<u8>, Vec<u8>>,
    ) -> Vec<GcCanonicalRootEvidence> {
        let mut roots = vec![
            GcCanonicalRootEvidence {
                name: "object_index_records".to_string(),
                family_id: None,
                page_root: inner.index_root,
                digest_root: None,
                reachability: "object_index".to_string(),
                semantic_liveness: false,
                advisory: false,
            },
            GcCanonicalRootEvidence {
                name: "reference_root".to_string(),
                family_id: None,
                page_root: None,
                digest_root: inner.reference_root,
                reachability: "semantic_object_graph".to_string(),
                semantic_liveness: inner.reference_root.is_some(),
                advisory: false,
            },
            GcCanonicalRootEvidence {
                name: "control_root".to_string(),
                family_id: None,
                page_root: None,
                digest_root: inner.control_root,
                reachability: "control_object".to_string(),
                semantic_liveness: self
                    .control_reachability_fingerprint_from_map(control_map)
                    .is_some(),
                advisory: false,
            },
            GcCanonicalRootEvidence {
                name: "current_records".to_string(),
                family_id: Some(CURRENT_RECORDS_FAMILY_ID),
                page_root: inner.current_record_root,
                digest_root: None,
                reachability: root_family_reachability_label(RootFamilyReachability::SemanticRoot)
                    .to_string(),
                semantic_liveness: inner.current_record_root.is_some(),
                advisory: false,
            },
            GcCanonicalRootEvidence {
                name: "root_catalog".to_string(),
                family_id: None,
                page_root: inner.root_catalog_root,
                digest_root: None,
                reachability: "catalog".to_string(),
                semantic_liveness: false,
                advisory: false,
            },
            GcCanonicalRootEvidence {
                name: "free_map".to_string(),
                family_id: None,
                page_root: inner.freemap.map(|(root, _)| root),
                digest_root: None,
                reachability: "physical_safety".to_string(),
                semantic_liveness: false,
                advisory: false,
            },
            GcCanonicalRootEvidence {
                name: "maintenance".to_string(),
                family_id: None,
                page_root: inner.maintenance_root,
                digest_root: None,
                reachability: "physical_safety".to_string(),
                semantic_liveness: false,
                advisory: false,
            },
        ];
        let catalog_roots = inner
            .root_catalog_entries
            .iter()
            .map(|entry| (entry.family_id, entry.root))
            .collect::<BTreeMap<_, _>>();
        for descriptor in ROOT_FAMILY_REGISTRY {
            if descriptor.family_id == CURRENT_RECORDS_FAMILY_ID {
                continue;
            }
            let page_root = catalog_roots.get(&descriptor.family_id).copied();
            let advisory = descriptor.role == RootFamilyRole::RebuildableAdvisory
                || descriptor.gc_reachability == RootFamilyReachability::AdvisoryPreserveOnly;
            let semantic_liveness = page_root.is_some()
                && matches!(
                    descriptor.gc_reachability,
                    RootFamilyReachability::SemanticRoot | RootFamilyReachability::ControlRoot
                );
            roots.push(GcCanonicalRootEvidence {
                name: descriptor.name.to_string(),
                family_id: Some(descriptor.family_id),
                page_root,
                digest_root: None,
                reachability: root_family_reachability_label(descriptor.gc_reachability)
                    .to_string(),
                semantic_liveness,
                advisory,
            });
        }
        roots
    }

    pub(crate) fn gc_canonical_roots_fingerprint(
        &self,
        roots: &[GcCanonicalRootEvidence],
    ) -> Option<Digest> {
        let mut bytes = Vec::new();
        for root in roots {
            if matches!(
                root.name.as_str(),
                "object_index_records" | "root_catalog" | "free_map" | "maintenance"
            ) {
                continue;
            }
            if root.page_root.is_none()
                && root.digest_root.is_none()
                && root.name != "control_root"
                && root.name != "reference_root"
            {
                continue;
            }
            bytes.extend_from_slice(root.name.as_bytes());
            bytes.push(0);
            match root.family_id {
                Some(family_id) => {
                    bytes.push(1);
                    bytes.extend_from_slice(&family_id.to_le_bytes());
                }
                None => bytes.push(0),
            }
            put_optional_page_bytes(&mut bytes, root.page_root);
            if root.name == "control_root" {
                put_optional_digest_bytes(&mut bytes, None);
            } else {
                put_optional_digest_bytes(&mut bytes, root.digest_root);
            }
            bytes.push(u8::from(root.semantic_liveness));
            bytes.push(u8::from(root.advisory));
        }
        if bytes.is_empty() {
            None
        } else {
            Some(Digest::hash(self.digest_algo, &bytes))
        }
    }

    #[cfg(test)]
    pub(crate) fn gc_reclaim_evidence_for_test(&self) -> Result<GcReclaimEvidence> {
        let mut inner = self.inner.lock().map_err(|_| poisoned())?;
        let control_map = self.control_map_locked(&mut inner)?;
        self.gc_reclaim_evidence_locked(&inner, &control_map)
    }

    pub(crate) fn index_snapshot_from_gc_evidence(
        &self,
        evidence: &GcReclaimEvidence,
    ) -> Result<Vec<([u8; 32], RecordLoc)>> {
        self.index_snapshot_from_evidence(evidence, None, None)
    }

    pub(crate) fn control_map_locked(
        &self,
        inner: &mut Inner,
    ) -> Result<BTreeMap<Vec<u8>, Vec<u8>>> {
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

            let (
                keys,
                enc_meta,
                source_overlay_root,
                source_root_catalog_entries,
                source_page_count,
                mutable_overlay_generation_floor,
            ): FullCompactionSnapshot = {
                let mut i = self.inner.lock().map_err(|_| poisoned())?;
                self.materialize_index_locked(&mut i)?;
                (
                    i.index.keys().copied().collect(),
                    i.encryption_meta.clone(),
                    i.overlay_root,
                    i.root_catalog_entries.clone(),
                    i.page_count,
                    i.mutable_overlay_generation_floor,
                )
            };
            let (
                current_records,
                retained_records,
                owner_token_records,
                secondary_index_records,
                mutable_idempotency_records,
                workflow_idempotency_records,
                audit_retention_records,
                mvcc_generation_records,
                retention_index_records,
                checkpoint_index_records,
                reclaim_index_records,
                delta_pack_candidate_records,
                legacy_records,
            ) = {
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
                let _ = generation;
                records.extend(entries.iter().map(|entry| {
                    (
                        mutable_overlay_entry_address(&entry.key),
                        encode_mutable_overlay_entry(entry),
                    )
                }));
                let (current_records, legacy_records) = split_mutable_overlay_records(&records);
                let mut legacy_records = legacy_records
                    .into_iter()
                    .map(|(address, value)| (address, value.to_vec()))
                    .collect::<BTreeMap<_, _>>();
                let mut retained_records = BTreeMap::<[u8; 32], Vec<u8>>::new();
                let mut owner_token_records = BTreeMap::<[u8; 32], Vec<u8>>::new();
                let mut secondary_index_records = BTreeMap::<[u8; 32], Vec<u8>>::new();
                let mut mutable_idempotency_records = BTreeMap::<[u8; 32], Vec<u8>>::new();
                let mut workflow_idempotency_records = BTreeMap::<[u8; 32], Vec<u8>>::new();
                let mut audit_retention_records = BTreeMap::<[u8; 32], Vec<u8>>::new();
                let mut mvcc_generation_records = BTreeMap::<[u8; 32], Vec<u8>>::new();
                let mut retention_index_records = BTreeMap::<[u8; 32], Vec<u8>>::new();
                let mut checkpoint_index_records = BTreeMap::<[u8; 32], Vec<u8>>::new();
                let mut reclaim_index_records = BTreeMap::<[u8; 32], Vec<u8>>::new();
                let mut delta_pack_candidate_records = BTreeMap::<[u8; 32], Vec<u8>>::new();
                let mut classify_record = |address: [u8; 32], value: Vec<u8>| -> Result<()> {
                    if value.starts_with(RETAINED_HISTORY_HEAD_RECORD)
                        || value.starts_with(RETAINED_HISTORY_ENTRY_RECORD)
                    {
                        retained_records.insert(address, value);
                    } else if value.starts_with(MUTABLE_OVERLAY_OWNER_TOKEN_RECORD) {
                        owner_token_records.insert(address, value);
                    } else if value.starts_with(MUTABLE_OVERLAY_SECONDARY_INDEX_RECORD) {
                        secondary_index_records.insert(address, value);
                    } else if value.starts_with(MUTABLE_OVERLAY_IDEMPOTENCY_RECORD) {
                        mutable_idempotency_records.insert(address, value);
                    } else if value.starts_with(MUTABLE_OVERLAY_TRANSACTION_IDEMPOTENCY_RECORD) {
                        workflow_idempotency_records.insert(address, value);
                    } else if value.starts_with(AUDIT_RETENTION_RECORD) {
                        audit_retention_records.insert(address, value);
                    } else {
                        legacy_records.insert(address, value);
                    }
                    Ok(())
                };
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
                        classify_record(address, value)?;
                    }
                }
                for entry in &source_root_catalog_entries {
                    let mut file = self.file.lock().map_err(|_| poisoned())?;
                    for (address, loc) in crate::root_family_load_all(
                        &mut **file,
                        entry.family_id,
                        entry.root,
                        source_page_count,
                    )? {
                        let value = read_blob_from_loc(&mut **file, loc)?;
                        match entry.family_id {
                            RETAINED_HISTORY_FAMILY_ID => {
                                retained_records.insert(address, value);
                            }
                            OWNER_TOKEN_FAMILY_ID => {
                                owner_token_records.insert(address, value);
                            }
                            SECONDARY_INDEX_FAMILY_ID => {
                                secondary_index_records.insert(address, value);
                            }
                            MUTABLE_IDEMPOTENCY_FAMILY_ID => {
                                mutable_idempotency_records.insert(address, value);
                            }
                            WORKFLOW_IDEMPOTENCY_FAMILY_ID => {
                                workflow_idempotency_records.insert(address, value);
                            }
                            AUDIT_RETENTION_FAMILY_ID => {
                                audit_retention_records.insert(address, value);
                            }
                            MVCC_GENERATION_FAMILY_ID => {
                                mvcc_generation_records.insert(address, value);
                            }
                            RETENTION_INDEX_FAMILY_ID => {
                                retention_index_records.insert(address, value);
                            }
                            CHECKPOINT_INDEX_FAMILY_ID => {
                                checkpoint_index_records.insert(address, value);
                            }
                            RECLAIM_INDEX_FAMILY_ID => {
                                reclaim_index_records.insert(address, value);
                            }
                            DELTA_PACK_CANDIDATE_FAMILY_ID => {
                                delta_pack_candidate_records.insert(address, value);
                            }
                            _ => {
                                legacy_records.insert(address, value);
                            }
                        }
                    }
                }
                let current_records = current_records
                    .into_iter()
                    .map(|(address, value)| (address, value.to_vec()))
                    .collect::<BTreeMap<_, _>>();
                (
                    current_records,
                    retained_records,
                    owner_token_records,
                    secondary_index_records,
                    mutable_idempotency_records,
                    workflow_idempotency_records,
                    audit_retention_records,
                    mvcc_generation_records,
                    retention_index_records,
                    checkpoint_index_records,
                    reclaim_index_records,
                    delta_pack_candidate_records,
                    legacy_records,
                )
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
            let mut build_root_with_codec = |records: &BTreeMap<[u8; 32], Vec<u8>>,
                                             codec: pagebtree::ValueCodecKind|
             -> Result<Option<PageId>> {
                let borrowed = records
                    .iter()
                    .map(|(key, value)| (*key, value.as_slice()))
                    .collect::<Vec<_>>();
                let mut entries = write_blob_pages(&mut out, &mut alloc, &borrowed)?;
                entries.sort_unstable_by_key(|e| e.0);
                pagebtree::build_packed_with_codec(
                    &mut out, DATA_START, &mut alloc, &entries, codec,
                )
            };
            let root_family_codec = |family_id| -> Result<pagebtree::ValueCodecKind> {
                root_family_descriptor(family_id)
                    .map(|descriptor| descriptor.value_codec)
                    .ok_or_else(|| corrupt("unknown root family during compaction"))
            };
            let current_root = build_root_with_codec(
                &current_records,
                root_family_codec(CURRENT_RECORDS_FAMILY_ID)?,
            )?;
            let retained_history_root = build_root_with_codec(
                &retained_records,
                root_family_codec(RETAINED_HISTORY_FAMILY_ID)?,
            )?;
            let owner_token_root = build_root_with_codec(
                &owner_token_records,
                root_family_codec(OWNER_TOKEN_FAMILY_ID)?,
            )?;
            let secondary_index_root = build_root_with_codec(
                &secondary_index_records,
                root_family_codec(SECONDARY_INDEX_FAMILY_ID)?,
            )?;
            let mutable_idempotency_root = build_root_with_codec(
                &mutable_idempotency_records,
                root_family_codec(MUTABLE_IDEMPOTENCY_FAMILY_ID)?,
            )?;
            let workflow_idempotency_root = build_root_with_codec(
                &workflow_idempotency_records,
                root_family_codec(WORKFLOW_IDEMPOTENCY_FAMILY_ID)?,
            )?;
            let audit_retention_root = build_root_with_codec(
                &audit_retention_records,
                root_family_codec(AUDIT_RETENTION_FAMILY_ID)?,
            )?;
            let mvcc_generation_root = build_root_with_codec(
                &mvcc_generation_records,
                root_family_codec(MVCC_GENERATION_FAMILY_ID)?,
            )?;
            let retention_index_root = build_root_with_codec(
                &retention_index_records,
                root_family_codec(RETENTION_INDEX_FAMILY_ID)?,
            )?;
            let checkpoint_index_root = build_root_with_codec(
                &checkpoint_index_records,
                root_family_codec(CHECKPOINT_INDEX_FAMILY_ID)?,
            )?;
            let reclaim_index_root = build_root_with_codec(
                &reclaim_index_records,
                root_family_codec(RECLAIM_INDEX_FAMILY_ID)?,
            )?;
            let delta_pack_candidate_root = build_root_with_codec(
                &delta_pack_candidate_records,
                root_family_codec(DELTA_PACK_CANDIDATE_FAMILY_ID)?,
            )?;
            let overlay_root =
                build_root_with_codec(&legacy_records, pagebtree::ValueCodecKind::RecordLoc)?;
            let mut root_catalog_entries = Vec::new();
            for (family_id, root) in [
                (RETAINED_HISTORY_FAMILY_ID, retained_history_root),
                (OWNER_TOKEN_FAMILY_ID, owner_token_root),
                (SECONDARY_INDEX_FAMILY_ID, secondary_index_root),
                (MUTABLE_IDEMPOTENCY_FAMILY_ID, mutable_idempotency_root),
                (WORKFLOW_IDEMPOTENCY_FAMILY_ID, workflow_idempotency_root),
                (AUDIT_RETENTION_FAMILY_ID, audit_retention_root),
                (MVCC_GENERATION_FAMILY_ID, mvcc_generation_root),
                (RETENTION_INDEX_FAMILY_ID, retention_index_root),
                (CHECKPOINT_INDEX_FAMILY_ID, checkpoint_index_root),
                (RECLAIM_INDEX_FAMILY_ID, reclaim_index_root),
            ] {
                if let Some(root) = root {
                    root_catalog_entries.push(RootCatalogEntry::authoritative(family_id, root));
                }
            }
            if let Some(root) = delta_pack_candidate_root {
                root_catalog_entries.push(RootCatalogEntry::advisory(
                    DELTA_PACK_CANDIDATE_FAMILY_ID,
                    root,
                ));
            }
            root_catalog_entries.sort_by_key(|entry| entry.family_id);
            let root_catalog_page_bound = alloc.page_count();
            let root_catalog_root = write_root_catalog_page(
                &mut out,
                &mut alloc,
                None,
                root_catalog_page_bound,
                &root_catalog_entries,
            )?;
            alloc.ensure_metadata_bootstrap_capacity()?;
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
                current_record_root: current_root,
                root_catalog_root,
                open_segment: 0,
                mutable_overlay_generation_floor,
                minimum_recoverable_generation: 0,
                metadata_bootstrap_reserve: alloc.metadata_bootstrap_descriptor(0),
            };
            let rt_buf = region
                .encode_page(page_count)
                .map_err(|_| corrupt("canonical region table encode failure"))?;
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
