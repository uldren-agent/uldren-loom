use crate::maintenance_policy::{MAINTENANCE_POLICY_KEY, MAINTENANCE_RUN_KEY};
use crate::page::{
    MetadataBootstrapExtent, MetadataBootstrapReserve, PageId, RECLAIM_INDEX_FAMILY_ID,
    RegionTable, RootCatalog,
};
use crate::record_io::encode_control_map;
use crate::{FileStore, corrupt};
use loom_core::error::{Code, LoomError, Result};
use loom_core::{
    Algo, Digest, Loom, ReachabilityMarkState, ReachabilityMarkStep, ReachabilityProllyCursor,
    ReachabilityStreamRoot,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const MARK_EPOCH_KEY: &[u8] = b"maintenance/v1/reachability-mark/active";
pub(crate) const MARK_EPOCH_RECLAIM_EVIDENCE_KEY: &[u8] =
    b"maintenance/v1/reachability-mark/reclaim-evidence";
const MARK_EPOCH_MAGIC: &[u8; 8] = b"LMARKEP1";
const MARK_EPOCH_RECLAIM_EVIDENCE_MAGIC: &[u8; 8] = b"LMARKEV1";
const MARK_EPOCH_VERSION: u16 = 14;
const MARK_EPOCH_RECLAIM_EVIDENCE_VERSION: u16 = 7;
const MARK_EPOCH_CHUNK_MAGIC: &[u8; 8] = b"LMARKCH1";
const MARK_EPOCH_CHUNK_VERSION: u16 = 4;
const MARK_EPOCH_CHUNK_PAGES: u64 = 4096;
const MARK_EPOCH_CHUNK_BITMAP_BYTES: usize = (MARK_EPOCH_CHUNK_PAGES as usize) / 8;
const MARK_EPOCH_CAPTURED_FREE_PUBLICATION_PAGES: usize = 64;
const MAX_DIGEST_LIST: usize = 1_000_000;
const MAX_PAGE_LIST: usize = 16_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MetadataBootstrapEvidenceProvenance {
    Legacy,
    Current,
}

impl MetadataBootstrapEvidenceProvenance {
    fn require_current(self, message: &'static str) -> Result<()> {
        if self == Self::Current {
            return Ok(());
        }
        Err(corrupt(message))
    }
}

#[cfg(test)]
thread_local! {
    static METADATA_PAGE_CLASSIFICATIONS_FOR_TEST: std::cell::Cell<u64> =
        const { std::cell::Cell::new(0) };
    static METADATA_CLASSIFIED_PAGES_FOR_TEST: std::cell::RefCell<BTreeSet<u64>> =
        const { std::cell::RefCell::new(BTreeSet::new()) };
}

#[cfg(test)]
pub(crate) fn reset_metadata_page_classifications_for_test() {
    METADATA_PAGE_CLASSIFICATIONS_FOR_TEST.set(0);
    METADATA_CLASSIFIED_PAGES_FOR_TEST.with(|pages| pages.borrow_mut().clear());
}

#[cfg(test)]
pub(crate) fn metadata_page_classifications_for_test() -> u64 {
    METADATA_PAGE_CLASSIFICATIONS_FOR_TEST.get()
}

#[cfg(test)]
pub(crate) fn metadata_page_was_classified_for_test(page: u64) -> bool {
    METADATA_CLASSIFIED_PAGES_FOR_TEST.with(|pages| pages.borrow().contains(&page))
}

#[cfg(test)]
pub(crate) fn encoded_mark_epoch_len_for_test(epoch: &ReachabilityMarkEpoch) -> usize {
    encode_mark_epoch(epoch).expect("current mark epoch").len()
}

#[cfg(test)]
pub(crate) fn decode_mark_epoch_for_test(
    bytes: &[u8],
    algo: Algo,
) -> Result<ReachabilityMarkEpoch> {
    decode_mark_epoch(bytes, algo)
}

#[cfg(test)]
pub(crate) fn decode_mark_reclaim_evidence_for_test(
    bytes: &[u8],
    algo: Algo,
) -> Result<ReachabilityMarkReclaimEvidence> {
    decode_mark_reclaim_evidence(bytes, algo)
}

#[cfg(test)]
pub(crate) fn encode_mark_epoch_v8_for_test(epoch: &ReachabilityMarkEpoch) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MARK_EPOCH_MAGIC);
    out.extend_from_slice(&8u16.to_le_bytes());
    out.extend_from_slice(&epoch.epoch.to_le_bytes());
    out.extend_from_slice(&epoch.base_generation.to_le_bytes());
    out.extend_from_slice(&epoch.page_high_water_mark.to_le_bytes());
    put_digest_list(&mut out, &epoch.captured_root_vector);
    put_u64_list(&mut out, &epoch.captured_metadata_roots);
    put_u64_list(&mut out, &epoch.captured_metadata_value_roots);
    out.push(u8::from(epoch.metadata_work_initialized));
    out.extend_from_slice(&epoch.metadata_root_cursor.to_le_bytes());
    out.extend_from_slice(&epoch.metadata_value_root_cursor.to_le_bytes());
    out.extend_from_slice(&epoch.metadata_classify_next_page.to_le_bytes());
    put_optional_u64(&mut out, epoch.metadata_evidence_root);
    out.extend_from_slice(&epoch.metadata_reachable_count.to_le_bytes());
    out.extend_from_slice(&epoch.metadata_reclaim_candidate_count.to_le_bytes());
    out.extend_from_slice(epoch.metadata_evidence_identity.bytes());
    out.push(u8::from(epoch.metadata_completed));
    out.extend_from_slice(epoch.reclaim_fence_identity.bytes());
    out.push(u8::from(epoch.state.completed));
    put_optional_digest(&mut out, epoch.reference_root);
    put_optional_digest(&mut out, epoch.control_fingerprint);
    put_optional_digest(&mut out, epoch.canonical_roots_fingerprint);
    put_digest_set(&mut out, &epoch.derived_roots);
    put_digest_set(&mut out, &epoch.state.pinned);
    put_digest_set(&mut out, &epoch.state.marked);
    put_digest_queue(&mut out, &epoch.state.queue);
    put_stream_root_queue(&mut out, &epoch.state.stream_roots);
    put_digest_queue(&mut out, &epoch.state.content_roots);
    put_prolly_cursor_queue(&mut out, &epoch.state.prolly_cursors);
    out
}

#[cfg(test)]
pub(crate) fn encode_mark_reclaim_evidence_v6_for_test(
    evidence: &ReachabilityMarkReclaimEvidence,
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MARK_EPOCH_RECLAIM_EVIDENCE_MAGIC);
    out.extend_from_slice(&6u16.to_le_bytes());
    out.extend_from_slice(&evidence.epoch.to_le_bytes());
    out.extend_from_slice(&evidence.base_generation.to_le_bytes());
    out.extend_from_slice(evidence.reclaim_fence_identity.bytes());
    out.extend_from_slice(&evidence.page_high_water_mark.to_le_bytes());
    out.extend_from_slice(evidence.captured_root_identity.bytes());
    put_optional_u64(&mut out, evidence.captured_free_root);
    put_optional_digest(&mut out, evidence.captured_free_identity);
    out.extend_from_slice(&evidence.captured_free_consumed_through.to_le_bytes());
    put_optional_u64(&mut out, evidence.metadata_evidence_root);
    out.extend_from_slice(&evidence.metadata_reclaim_candidate_count.to_le_bytes());
    out.extend_from_slice(evidence.metadata_evidence_identity.bytes());
    put_page_set(&mut out, &evidence.unreachable_pre_snapshot_pages);
    out
}

#[cfg(test)]
pub(crate) fn encode_mark_epoch_v8_queue_layout_for_test(
    epoch: &ReachabilityMarkEpoch,
    metadata_roots: &[u64],
    metadata_value_roots: &[u64],
    metadata_value_tree_pages: &[u64],
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MARK_EPOCH_MAGIC);
    out.extend_from_slice(&8u16.to_le_bytes());
    out.extend_from_slice(&epoch.epoch.to_le_bytes());
    out.extend_from_slice(&epoch.base_generation.to_le_bytes());
    out.extend_from_slice(&epoch.page_high_water_mark.to_le_bytes());
    put_digest_list(&mut out, &epoch.captured_root_vector);
    put_u64_list(&mut out, &epoch.captured_metadata_roots);
    put_u64_list(&mut out, &epoch.captured_metadata_value_roots);
    put_u64_list(&mut out, metadata_roots);
    put_u64_list(&mut out, metadata_value_roots);
    put_page_set(
        &mut out,
        &metadata_value_tree_pages.iter().copied().collect(),
    );
    out.extend_from_slice(&8u64.to_le_bytes());
    put_optional_u64(&mut out, Some(9));
    out.extend_from_slice(&10u64.to_le_bytes());
    out.extend_from_slice(&11u64.to_le_bytes());
    out.extend_from_slice(epoch.metadata_evidence_identity.bytes());
    out.push(u8::from(epoch.metadata_completed));
    out.extend_from_slice(epoch.reclaim_fence_identity.bytes());
    out.push(u8::from(epoch.state.completed));
    put_optional_digest(&mut out, epoch.reference_root);
    put_optional_digest(&mut out, epoch.control_fingerprint);
    put_optional_digest(&mut out, epoch.canonical_roots_fingerprint);
    put_digest_set(&mut out, &epoch.derived_roots);
    put_digest_set(&mut out, &epoch.state.pinned);
    put_digest_set(&mut out, &epoch.state.marked);
    put_digest_queue(&mut out, &epoch.state.queue);
    put_stream_root_queue(&mut out, &epoch.state.stream_roots);
    put_digest_queue(&mut out, &epoch.state.content_roots);
    put_prolly_cursor_queue(&mut out, &epoch.state.prolly_cursors);
    out
}

#[cfg(test)]
pub(crate) fn metadata_chunk_v1_decodes_for_test() -> Result<(bool, bool)> {
    let chunk = MetadataEvidenceChunk::empty(7, 0);
    let mut original = Vec::with_capacity(8 + 2 + 8 + 8 + MARK_EPOCH_CHUNK_BITMAP_BYTES * 2);
    original.extend_from_slice(MARK_EPOCH_CHUNK_MAGIC);
    original.extend_from_slice(&1u16.to_le_bytes());
    original.extend_from_slice(&chunk.epoch.to_le_bytes());
    original.extend_from_slice(&chunk.page_start.to_le_bytes());
    original.extend_from_slice(&chunk.reachable);
    original.extend_from_slice(&chunk.reclaim_candidate);
    let original_decoded = decode_metadata_evidence_chunk(&original)?;

    let mut later = Vec::with_capacity(8 + 2 + 8 + 8 + MARK_EPOCH_CHUNK_BITMAP_BYTES * 5);
    later.extend_from_slice(MARK_EPOCH_CHUNK_MAGIC);
    later.extend_from_slice(&1u16.to_le_bytes());
    later.extend_from_slice(&chunk.epoch.to_le_bytes());
    later.extend_from_slice(&chunk.page_start.to_le_bytes());
    later.extend_from_slice(&chunk.pending_roots);
    later.extend_from_slice(&chunk.pending_value_roots);
    later.extend_from_slice(&chunk.value_tree);
    later.extend_from_slice(&chunk.reachable);
    later.extend_from_slice(&chunk.reclaim_candidate);
    let later_decoded = decode_metadata_evidence_chunk(&later)?;

    Ok((
        original_decoded.pending_roots.iter().all(|byte| *byte == 0)
            && original_decoded.reachable == chunk.reachable,
        later_decoded
            .pending_value_blobs
            .iter()
            .all(|byte| *byte == 0)
            && later_decoded.large_value_start.is_none(),
    ))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReachabilityMarkEpoch {
    pub epoch: u64,
    pub base_generation: u64,
    pub page_high_water_mark: u64,
    pub captured_root_vector: Vec<Digest>,
    pub captured_metadata_roots: Vec<u64>,
    pub captured_metadata_value_roots: Vec<u64>,
    pub(crate) captured_metadata_bootstrap_reserve: MetadataBootstrapReserve,
    pub(crate) metadata_bootstrap_evidence_provenance: MetadataBootstrapEvidenceProvenance,
    pub captured_free_root: Option<u64>,
    pub captured_free_identity: Option<Digest>,
    pub captured_free_consumed_through: u64,
    pub metadata_work_initialized: bool,
    pub metadata_root_cursor: u64,
    pub metadata_value_root_cursor: u64,
    pub metadata_value_blob_cursor: u64,
    pub metadata_expansion_cursor: u64,
    pub metadata_classify_next_page: u64,
    pub metadata_evidence_root: Option<u64>,
    pub metadata_reachable_count: u64,
    pub metadata_reclaim_candidate_count: u64,
    pub metadata_evidence_identity: Digest,
    pub metadata_completed: bool,
    pub reclaim_fence_identity: Digest,
    pub reference_root: Option<Digest>,
    pub control_fingerprint: Option<Digest>,
    pub canonical_roots_fingerprint: Option<Digest>,
    pub derived_roots: BTreeSet<Digest>,
    pub state: ReachabilityMarkState,
}

impl ReachabilityMarkEpoch {
    pub fn retain_set(&self) -> BTreeSet<[u8; 32]> {
        self.state
            .marked
            .iter()
            .map(|digest| *digest.bytes())
            .collect()
    }

    fn require_current_metadata_bootstrap_evidence(&self) -> Result<()> {
        self.metadata_bootstrap_evidence_provenance
            .require_current("legacy reachability mark epoch lacks metadata-bootstrap evidence")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReachabilityMarkReclaimEvidence {
    pub(crate) epoch: u64,
    pub(crate) base_generation: u64,
    pub(crate) reclaim_fence_identity: Digest,
    pub(crate) page_high_water_mark: u64,
    pub(crate) captured_root_identity: Digest,
    pub(crate) captured_metadata_bootstrap_reserve: MetadataBootstrapReserve,
    pub(crate) metadata_bootstrap_evidence_provenance: MetadataBootstrapEvidenceProvenance,
    pub(crate) captured_free_root: Option<u64>,
    pub(crate) captured_free_identity: Option<Digest>,
    pub(crate) captured_free_consumed_through: u64,
    pub(crate) metadata_evidence_root: Option<u64>,
    pub(crate) metadata_reclaim_candidate_count: u64,
    pub(crate) metadata_evidence_identity: Digest,
    pub(crate) unreachable_pre_snapshot_pages: BTreeSet<u64>,
}

impl ReachabilityMarkReclaimEvidence {
    fn require_current_metadata_bootstrap_evidence(&self) -> Result<()> {
        self.metadata_bootstrap_evidence_provenance.require_current(
            "legacy reachability mark reclaim evidence lacks metadata-bootstrap evidence",
        )
    }

    pub(crate) fn matches_epoch(&self, epoch: &ReachabilityMarkEpoch, algo: Algo) -> bool {
        self.metadata_bootstrap_evidence_provenance == MetadataBootstrapEvidenceProvenance::Current
            && epoch.metadata_bootstrap_evidence_provenance
                == MetadataBootstrapEvidenceProvenance::Current
            && self.epoch == epoch.epoch
            && self.base_generation == epoch.base_generation
            && self.reclaim_fence_identity == epoch.reclaim_fence_identity
            && self.page_high_water_mark == epoch.page_high_water_mark
            && self.captured_root_identity
                == mark_epoch_captured_root_identity(algo, &epoch.captured_root_vector)
            && self.captured_metadata_bootstrap_reserve == epoch.captured_metadata_bootstrap_reserve
            && self.captured_free_root == epoch.captured_free_root
            && self.captured_free_identity == epoch.captured_free_identity
            && self.captured_free_consumed_through == epoch.captured_free_consumed_through
            && self.metadata_evidence_root == epoch.metadata_evidence_root
            && self.metadata_reclaim_candidate_count == epoch.metadata_reclaim_candidate_count
            && self.metadata_evidence_identity == epoch.metadata_evidence_identity
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MetadataEvidenceChunk {
    epoch: u64,
    page_start: u64,
    pending_roots: [u8; MARK_EPOCH_CHUNK_BITMAP_BYTES],
    pending_value_roots: [u8; MARK_EPOCH_CHUNK_BITMAP_BYTES],
    pending_value_blobs: [u8; MARK_EPOCH_CHUNK_BITMAP_BYTES],
    large_value_start: Option<u64>,
    large_value_next: u64,
    large_value_end: u64,
    expansion_node: Option<u64>,
    expansion_child_offset: u16,
    expansion_value_offset: u16,
    expansion_value_tree: bool,
    expansion_free_page_extent_tree: bool,
    free_page_extent_tree: [u8; MARK_EPOCH_CHUNK_BITMAP_BYTES],
    value_tree: [u8; MARK_EPOCH_CHUNK_BITMAP_BYTES],
    reachable: [u8; MARK_EPOCH_CHUNK_BITMAP_BYTES],
    reclaim_candidate: [u8; MARK_EPOCH_CHUNK_BITMAP_BYTES],
}

impl FileStore {
    pub fn begin_reachability_mark_epoch(
        &self,
        reference_root: Option<Digest>,
        derived_roots: BTreeSet<Digest>,
        state: ReachabilityMarkState,
    ) -> Result<ReachabilityMarkEpoch> {
        let _publication_guard = self
            .overlay_publication
            .lock()
            .map_err(|_| crate::poisoned())?;
        let mut inner = self.inner.lock().map_err(|_| crate::poisoned())?;
        if reference_root != inner.reference_root {
            return Err(LoomError::new(
                Code::Conflict,
                "reachability mark epoch reference root changed before capture",
            ));
        }
        let mut control_map = self.control_map_locked(&mut inner)?;
        let actual_derived_roots = self
            .derived_payload_digests_from_control_map(&control_map)?
            .into_iter()
            .map(|bytes| Digest::of(self.digest_algo, bytes))
            .collect::<BTreeSet<_>>();
        if derived_roots != actual_derived_roots {
            return Err(LoomError::new(
                Code::Conflict,
                "reachability mark epoch derived roots changed before capture",
            ));
        }
        let control_fingerprint = self.control_reachability_fingerprint_from_map(&control_map);
        let canonical_roots = self.gc_canonical_roots_locked(&inner, &control_map);
        let canonical_roots_fingerprint = self.gc_canonical_roots_fingerprint(&canonical_roots);
        let captured_root_vector = mark_epoch_captured_root_vector(
            inner.reference_root,
            inner.control_root,
            &actual_derived_roots,
        );
        let (captured_metadata_roots, captured_metadata_value_roots) =
            mark_epoch_captured_metadata_roots(&inner, &canonical_roots)?;
        let captured_metadata_bootstrap_reserve = inner.metadata_bootstrap_reserve.clone();
        let active_epoch = control_map
            .get(MARK_EPOCH_KEY)
            .map(|bytes| decode_mark_epoch(bytes, self.digest_algo))
            .transpose()?;
        let next_epoch = active_epoch
            .map(|epoch| epoch.epoch.saturating_add(1))
            .unwrap_or(
                inner
                    .maintenance
                    .last_validated_mark_epoch
                    .saturating_add(1),
            )
            .max(
                inner
                    .maintenance
                    .last_validated_mark_epoch
                    .saturating_add(1),
            );
        let page_high_water_mark = inner.page_count;
        let base_generation = inner.generation;
        let captured_free_root = inner.freemap.map(|(root, _)| root.0);
        let captured_free = if let Some(root) = captured_free_root {
            let mut file = self.file.lock().map_err(|_| crate::poisoned())?;
            crate::pagemap::read_map_with_root_span(
                &mut **file,
                crate::DATA_START,
                PageId(root),
                page_high_water_mark,
            )?
            .0
        } else {
            Vec::new()
        };
        let captured_free_identity = Some(mark_epoch_captured_free_identity(
            self.digest_algo,
            base_generation,
            page_high_water_mark,
            captured_free_root,
            &captured_free,
        ));
        let reclaim_fence_identity = mark_epoch_reclaim_fence_identity(
            self.digest_algo,
            next_epoch,
            base_generation,
            page_high_water_mark,
            &captured_root_vector,
            &captured_metadata_roots,
            &captured_metadata_value_roots,
            &captured_metadata_bootstrap_reserve,
        );
        let epoch = ReachabilityMarkEpoch {
            epoch: next_epoch,
            base_generation,
            page_high_water_mark,
            captured_root_vector,
            captured_metadata_roots,
            captured_metadata_value_roots,
            captured_metadata_bootstrap_reserve,
            metadata_bootstrap_evidence_provenance: MetadataBootstrapEvidenceProvenance::Current,
            captured_free_root,
            captured_free_identity,
            captured_free_consumed_through: 0,
            metadata_work_initialized: page_high_water_mark == 0,
            metadata_root_cursor: 0,
            metadata_value_root_cursor: 0,
            metadata_value_blob_cursor: 0,
            metadata_expansion_cursor: 0,
            metadata_classify_next_page: 0,
            metadata_evidence_root: None,
            metadata_reachable_count: 0,
            metadata_reclaim_candidate_count: 0,
            metadata_evidence_identity: mark_epoch_metadata_evidence_identity(
                self.digest_algo,
                None,
                0,
                0,
            ),
            metadata_completed: page_high_water_mark == 0,
            reclaim_fence_identity,
            reference_root,
            control_fingerprint,
            canonical_roots_fingerprint,
            derived_roots: actual_derived_roots,
            state,
        };
        self.publish_reachability_mark_epoch_begin_locked(&mut inner, &mut control_map, &epoch)?;
        Ok(epoch)
    }

    pub fn active_reachability_mark_epoch(&self) -> Result<Option<ReachabilityMarkEpoch>> {
        Ok(self
            .control_get(MARK_EPOCH_KEY)?
            .map(|bytes| decode_mark_epoch(&bytes, self.digest_algo))
            .transpose()?
            .filter(|epoch| {
                epoch.metadata_bootstrap_evidence_provenance
                    == MetadataBootstrapEvidenceProvenance::Current
            }))
    }

    pub(crate) fn active_reachability_mark_reclaim_evidence(
        &self,
    ) -> Result<Option<ReachabilityMarkReclaimEvidence>> {
        Ok(self
            .control_get(MARK_EPOCH_RECLAIM_EVIDENCE_KEY)?
            .map(|bytes| decode_mark_reclaim_evidence(&bytes, self.digest_algo))
            .transpose()?
            .filter(|evidence| {
                evidence.metadata_bootstrap_evidence_provenance
                    == MetadataBootstrapEvidenceProvenance::Current
            }))
    }

    pub(crate) fn reachability_mark_metadata_reclaim_candidate_pages(
        &self,
        evidence: &ReachabilityMarkReclaimEvidence,
        page_count: u64,
        max_pages: u64,
    ) -> Result<BTreeSet<u64>> {
        evidence.require_current_metadata_bootstrap_evidence()?;
        let Some(root) = evidence.metadata_evidence_root else {
            return Ok(BTreeSet::new());
        };
        let limit = usize::try_from(max_pages).unwrap_or(usize::MAX);
        if limit == 0 {
            return Ok(BTreeSet::new());
        }
        let mut file = self.file.lock().map_err(|_| crate::poisoned())?;
        let captured_free = match (evidence.captured_free_root, evidence.captured_free_identity) {
            (Some(root), Some(identity)) => {
                let free = crate::pagemap::read_map_with_root_span(
                    &mut **file,
                    crate::DATA_START,
                    PageId(root),
                    evidence.page_high_water_mark,
                )?
                .0;
                if mark_epoch_captured_free_identity(
                    self.digest_algo,
                    evidence.base_generation,
                    evidence.page_high_water_mark,
                    Some(root),
                    &free,
                ) != identity
                {
                    return Err(corrupt("reachability mark captured-free identity mismatch"));
                }
                free
            }
            (None, None) if evidence.captured_free_consumed_through == 0 => Vec::new(),
            _ => return Ok(BTreeSet::new()),
        };
        let captured_free_protected = captured_free_prefix_runs(
            &captured_free,
            evidence.page_high_water_mark,
            evidence.captured_free_consumed_through,
        )?;
        let mut cursor = crate::pagebtree::ScanCursor::new(PageId(root));
        let mut pages = BTreeSet::new();
        while !cursor.completed() && pages.len() < limit {
            let step = crate::pagebtree::scan_step_with_page_reader_and_codec(
                &mut cursor,
                page_count,
                1,
                None,
                crate::root_family_value_codec(RECLAIM_INDEX_FAMILY_ID)?,
                |page| {
                    let mut buf = [0u8; crate::page::PAGE_SIZE as usize];
                    crate::read_exact_at(&mut **file, page.offset(crate::DATA_START), &mut buf)
                        .map_err(crate::io_err)?;
                    Ok(buf)
                },
            )?;
            if step.pages_read == 0 && step.entries.is_empty() {
                break;
            }
            for (_, loc) in step.entries {
                if pages.len() >= limit {
                    break;
                }
                let bytes = crate::record_io::read_blob_from_loc(&mut **file, loc, page_count)?;
                let chunk = decode_metadata_evidence_chunk(&bytes)?;
                if chunk.epoch != evidence.epoch {
                    return Err(corrupt("reachability mark metadata chunk epoch mismatch"));
                }
                for page in chunk.reclaim_candidate_pages(limit - pages.len()) {
                    let protected_captured_page = captured_free_protected
                        .iter()
                        .any(|run| page >= run.start && page < run.start.saturating_add(run.len));
                    if page < evidence.page_high_water_mark && !protected_captured_page {
                        pages.insert(page);
                    }
                }
            }
        }
        Ok(pages)
    }

    #[cfg(test)]
    pub(crate) fn reachability_mark_metadata_evidence_chunk_count_for_test(
        &self,
        epoch: &ReachabilityMarkEpoch,
    ) -> Result<usize> {
        let Some(root) = epoch.metadata_evidence_root else {
            return Ok(0);
        };
        let page_count = self.inner.lock().map_err(|_| crate::poisoned())?.page_count;
        let mut file = self.file.lock().map_err(|_| crate::poisoned())?;
        Ok(crate::root_family_load_all(
            &mut **file,
            RECLAIM_INDEX_FAMILY_ID,
            PageId(root),
            page_count,
        )?
        .len())
    }

    pub fn save_reachability_mark_epoch(&self, epoch: &ReachabilityMarkEpoch) -> Result<()> {
        self.control_set(MARK_EPOCH_KEY, encode_mark_epoch(epoch)?)?;
        self.set_active_reachability_mark_epoch_reclaim_fence(Some(epoch.page_high_water_mark))
    }

    pub fn complete_reachability_mark_epoch(&self, epoch: &ReachabilityMarkEpoch) -> Result<()> {
        epoch.require_current_metadata_bootstrap_evidence()?;
        if !epoch.state.completed || !epoch.metadata_completed {
            return Err(LoomError::invalid(
                "reachability mark epoch is not complete",
            ));
        }
        let reclaim_evidence = self.build_reachability_mark_reclaim_evidence(epoch)?;
        let mut map = self.control_map()?;
        map.insert(MARK_EPOCH_KEY.to_vec(), encode_mark_epoch(epoch)?);
        map.insert(
            MARK_EPOCH_RECLAIM_EVIDENCE_KEY.to_vec(),
            encode_mark_reclaim_evidence(&reclaim_evidence)?,
        );
        self.write_control_map_validating_mark_epoch(map, epoch.epoch)
    }

    pub fn clear_reachability_mark_epoch(&self) -> Result<bool> {
        let _publication_guard = self
            .overlay_publication
            .lock()
            .map_err(|_| crate::poisoned())?;
        let mut inner = self.inner.lock().map_err(|_| crate::poisoned())?;
        let mut map = self.control_map_locked(&mut inner)?;
        let removed = map.remove(MARK_EPOCH_KEY).is_some();
        map.remove(MARK_EPOCH_RECLAIM_EVIDENCE_KEY);
        if !removed {
            inner.active_mark_epoch_reclaim_fence = None;
            return Ok(false);
        }
        let reclamation_lease = self.try_reclamation_write_lease()?;
        if !reclamation_lease.allowed {
            return Err(LoomError::new(
                Code::Conflict,
                "loom-store: active readers block reachability epoch clearing",
            ));
        }
        self.publish_reachability_mark_epoch_clear_locked(&mut inner, &map)?;
        Ok(removed)
    }

    fn build_reachability_mark_reclaim_evidence(
        &self,
        epoch: &ReachabilityMarkEpoch,
    ) -> Result<ReachabilityMarkReclaimEvidence> {
        epoch.require_current_metadata_bootstrap_evidence()?;
        let retain = epoch.retain_set();
        let mut inner = self.inner.lock().map_err(|_| crate::poisoned())?;
        let control_map = self.control_map_locked(&mut inner)?;
        let evidence = self.gc_reclaim_evidence_locked(&inner, &control_map)?;
        let index_snapshot = self.index_snapshot_from_gc_evidence(&evidence)?;
        drop(inner);
        let mut file = self.file.lock().map_err(|_| crate::poisoned())?;
        let captured_free_protected =
            captured_free_consumed_runs(&mut **file, self.digest_algo, epoch)?;
        let mut page_live: BTreeMap<u64, bool> = BTreeMap::new();
        let mut record_pages = BTreeMap::<[u8; 32], Vec<u64>>::new();
        for (digest, loc) in &index_snapshot {
            let pages =
                crate::record_io::blob_pages(&mut **file, loc.global_page(), evidence.page_count)?;
            let pre_snapshot = pages.iter().all(|page| *page < epoch.page_high_water_mark);
            let live = !pre_snapshot || retain.contains(digest);
            for page in &pages {
                *page_live.entry(*page).or_insert(false) |= live;
            }
            record_pages.insert(*digest, pages);
        }
        let mut unreachable_pre_snapshot_pages = BTreeSet::new();
        for (digest, pages) in record_pages {
            if retain.contains(&digest) {
                continue;
            }
            for page in pages {
                if page < epoch.page_high_water_mark
                    && !page_live.get(&page).copied().unwrap_or(false)
                    && !epoch
                        .captured_metadata_bootstrap_reserve
                        .contains_page(page)
                    && !captured_free_protected
                        .iter()
                        .any(|run| page >= run.start && page < run.start.saturating_add(run.len))
                {
                    unreachable_pre_snapshot_pages.insert(page);
                }
            }
        }
        Ok(ReachabilityMarkReclaimEvidence {
            epoch: epoch.epoch,
            base_generation: epoch.base_generation,
            reclaim_fence_identity: epoch.reclaim_fence_identity,
            page_high_water_mark: epoch.page_high_water_mark,
            captured_root_identity: mark_epoch_captured_root_identity(
                self.digest_algo,
                &epoch.captured_root_vector,
            ),
            captured_metadata_bootstrap_reserve: epoch.captured_metadata_bootstrap_reserve.clone(),
            metadata_bootstrap_evidence_provenance: MetadataBootstrapEvidenceProvenance::Current,
            captured_free_root: epoch.captured_free_root,
            captured_free_identity: epoch.captured_free_identity,
            captured_free_consumed_through: epoch.captured_free_consumed_through,
            metadata_evidence_root: epoch.metadata_evidence_root,
            metadata_reclaim_candidate_count: epoch.metadata_reclaim_candidate_count,
            metadata_evidence_identity: epoch.metadata_evidence_identity,
            unreachable_pre_snapshot_pages,
        })
    }

    pub(crate) fn step_reachability_metadata_mark_epoch(
        &self,
        epoch: &mut ReachabilityMarkEpoch,
        mut budget: usize,
        deadline_expired: Option<&dyn Fn() -> bool>,
    ) -> Result<usize> {
        epoch.require_current_metadata_bootstrap_evidence()?;
        if epoch.metadata_completed || budget == 0 {
            return Ok(0);
        }
        let mut visited = 0usize;
        let evidence_page_count = self.inner.lock().map_err(|_| crate::poisoned())?.page_count;
        let mut file = self.file.lock().map_err(|_| crate::poisoned())?;
        let packed_locator_tree_pages = captured_packed_locator_tree_pages(&mut **file, epoch)?;
        let mut touched_chunks = BTreeMap::<u64, MetadataEvidenceChunk>::new();
        while budget > 0 {
            if deadline_expired.is_some_and(|expired| expired()) {
                break;
            }
            if !epoch.metadata_work_initialized {
                if let Some(root) = epoch
                    .captured_metadata_roots
                    .iter()
                    .copied()
                    .find(|root| *root >= epoch.metadata_root_cursor)
                {
                    if root >= epoch.page_high_water_mark {
                        return Err(corrupt("reachability mark metadata root out of range"));
                    }
                    let chunk_start = metadata_evidence_chunk_start(root);
                    let chunk_end = chunk_start
                        .saturating_add(MARK_EPOCH_CHUNK_PAGES)
                        .min(epoch.page_high_water_mark);
                    for root in epoch
                        .captured_metadata_roots
                        .iter()
                        .copied()
                        .filter(|root| *root >= chunk_start && *root < chunk_end)
                    {
                        if root >= epoch.page_high_water_mark {
                            return Err(corrupt("reachability mark metadata root out of range"));
                        }
                        let chunk = metadata_evidence_chunk_for_page(
                            &mut **file,
                            epoch.metadata_evidence_root,
                            self.digest_algo,
                            epoch.epoch,
                            epoch.reclaim_fence_identity,
                            root,
                            evidence_page_count,
                            &mut touched_chunks,
                        )?;
                        chunk.set_pending_root(root)?;
                        if epoch.captured_free_root == Some(root) {
                            chunk.set_free_page_extent_tree(root)?;
                        }
                    }
                    epoch.metadata_root_cursor = chunk_end;
                    visited = visited.saturating_add(1);
                    budget -= 1;
                    continue;
                }
                if let Some(root) = epoch
                    .captured_metadata_value_roots
                    .iter()
                    .copied()
                    .find(|root| *root >= epoch.metadata_value_root_cursor)
                {
                    if root >= epoch.page_high_water_mark {
                        return Err(corrupt(
                            "reachability mark metadata value root out of range",
                        ));
                    }
                    let chunk_start = metadata_evidence_chunk_start(root);
                    let chunk_end = chunk_start
                        .saturating_add(MARK_EPOCH_CHUNK_PAGES)
                        .min(epoch.page_high_water_mark);
                    for root in epoch
                        .captured_metadata_value_roots
                        .iter()
                        .copied()
                        .filter(|root| *root >= chunk_start && *root < chunk_end)
                    {
                        if root >= epoch.page_high_water_mark {
                            return Err(corrupt(
                                "reachability mark metadata value root out of range",
                            ));
                        }
                        metadata_evidence_chunk_for_page(
                            &mut **file,
                            epoch.metadata_evidence_root,
                            self.digest_algo,
                            epoch.epoch,
                            epoch.reclaim_fence_identity,
                            root,
                            evidence_page_count,
                            &mut touched_chunks,
                        )?
                        .set_pending_value_root(root)?;
                    }
                    epoch.metadata_value_root_cursor = chunk_end;
                    visited = visited.saturating_add(1);
                    budget -= 1;
                    continue;
                }
                epoch.metadata_work_initialized = true;
                epoch.metadata_root_cursor = epoch
                    .captured_metadata_roots
                    .iter()
                    .copied()
                    .next()
                    .unwrap_or(epoch.page_high_water_mark);
                epoch.metadata_value_root_cursor = epoch
                    .captured_metadata_value_roots
                    .iter()
                    .copied()
                    .next()
                    .unwrap_or(epoch.page_high_water_mark);
                epoch.metadata_value_blob_cursor = epoch.page_high_water_mark;
                epoch.metadata_expansion_cursor = epoch.page_high_water_mark;
                visited = visited.saturating_add(1);
                budget -= 1;
                continue;
            }
            if let Some(value_root) = metadata_take_next_pending_page(
                &mut **file,
                epoch.metadata_evidence_root,
                self.digest_algo,
                epoch.epoch,
                epoch.reclaim_fence_identity,
                &mut epoch.metadata_value_root_cursor,
                epoch.page_high_water_mark,
                evidence_page_count,
                &mut touched_chunks,
                MetadataPendingKind::ValueRoot,
            )? {
                if value_root >= epoch.page_high_water_mark {
                    return Err(corrupt(
                        "reachability mark metadata value root out of range",
                    ));
                }
                let chunk = metadata_evidence_chunk_for_page(
                    &mut **file,
                    epoch.metadata_evidence_root,
                    self.digest_algo,
                    epoch.epoch,
                    epoch.reclaim_fence_identity,
                    value_root,
                    evidence_page_count,
                    &mut touched_chunks,
                )?;
                chunk.set_value_tree(value_root)?;
                chunk.set_pending_root(value_root)?;
                epoch.metadata_root_cursor = epoch.metadata_root_cursor.min(value_root);
                visited = visited.saturating_add(1);
                budget -= 1;
                continue;
            }
            if let Some(value_page) = metadata_take_next_pending_page(
                &mut **file,
                epoch.metadata_evidence_root,
                self.digest_algo,
                epoch.epoch,
                epoch.reclaim_fence_identity,
                &mut epoch.metadata_value_blob_cursor,
                epoch.page_high_water_mark,
                evidence_page_count,
                &mut touched_chunks,
                MetadataPendingKind::ValueBlob,
            )? {
                let value_chunk_start = metadata_evidence_chunk_start(value_page);
                let value_chunk_end = value_chunk_start
                    .saturating_add(MARK_EPOCH_CHUNK_PAGES)
                    .min(epoch.page_high_water_mark);
                metadata_process_value_blob_page(
                    &mut **file,
                    self.digest_algo,
                    epoch,
                    value_page,
                    evidence_page_count,
                    &mut touched_chunks,
                )?;
                while epoch.metadata_value_blob_cursor < value_chunk_end {
                    let Some(value_page) = metadata_take_next_large_value_page(
                        &mut **file,
                        epoch.metadata_evidence_root,
                        self.digest_algo,
                        epoch.epoch,
                        epoch.reclaim_fence_identity,
                        &mut epoch.metadata_value_blob_cursor,
                        epoch.page_high_water_mark,
                        evidence_page_count,
                        &mut touched_chunks,
                    )?
                    else {
                        break;
                    };
                    if metadata_evidence_chunk_start(value_page) != value_chunk_start {
                        epoch.metadata_value_blob_cursor =
                            epoch.metadata_value_blob_cursor.min(value_page);
                        break;
                    }
                    metadata_mark_reachable_page(
                        &mut **file,
                        self.digest_algo,
                        epoch,
                        value_page,
                        evidence_page_count,
                        &mut touched_chunks,
                    )?;
                }
                while epoch.metadata_value_blob_cursor < value_chunk_end {
                    let Some(value_page) = metadata_take_next_pending_page(
                        &mut **file,
                        epoch.metadata_evidence_root,
                        self.digest_algo,
                        epoch.epoch,
                        epoch.reclaim_fence_identity,
                        &mut epoch.metadata_value_blob_cursor,
                        epoch.page_high_water_mark,
                        evidence_page_count,
                        &mut touched_chunks,
                        MetadataPendingKind::ValueBlob,
                    )?
                    else {
                        break;
                    };
                    if metadata_evidence_chunk_start(value_page) != value_chunk_start {
                        epoch.metadata_value_blob_cursor =
                            epoch.metadata_value_blob_cursor.min(value_page);
                        break;
                    }
                    metadata_process_value_blob_page(
                        &mut **file,
                        self.digest_algo,
                        epoch,
                        value_page,
                        evidence_page_count,
                        &mut touched_chunks,
                    )?;
                    while epoch.metadata_value_blob_cursor < value_chunk_end {
                        let Some(value_page) = metadata_take_next_large_value_page(
                            &mut **file,
                            epoch.metadata_evidence_root,
                            self.digest_algo,
                            epoch.epoch,
                            epoch.reclaim_fence_identity,
                            &mut epoch.metadata_value_blob_cursor,
                            epoch.page_high_water_mark,
                            evidence_page_count,
                            &mut touched_chunks,
                        )?
                        else {
                            break;
                        };
                        if metadata_evidence_chunk_start(value_page) != value_chunk_start {
                            epoch.metadata_value_blob_cursor =
                                epoch.metadata_value_blob_cursor.min(value_page);
                            break;
                        }
                        metadata_mark_reachable_page(
                            &mut **file,
                            self.digest_algo,
                            epoch,
                            value_page,
                            evidence_page_count,
                            &mut touched_chunks,
                        )?;
                    }
                }
                visited = visited.saturating_add(1);
                budget -= 1;
                continue;
            }
            if let Some(value_page) = metadata_take_next_large_value_page(
                &mut **file,
                epoch.metadata_evidence_root,
                self.digest_algo,
                epoch.epoch,
                epoch.reclaim_fence_identity,
                &mut epoch.metadata_value_blob_cursor,
                epoch.page_high_water_mark,
                evidence_page_count,
                &mut touched_chunks,
            )? {
                let value_chunk_start = metadata_evidence_chunk_start(value_page);
                let value_chunk_end = value_chunk_start
                    .saturating_add(MARK_EPOCH_CHUNK_PAGES)
                    .min(epoch.page_high_water_mark);
                metadata_mark_reachable_page(
                    &mut **file,
                    self.digest_algo,
                    epoch,
                    value_page,
                    evidence_page_count,
                    &mut touched_chunks,
                )?;
                while epoch.metadata_value_blob_cursor < value_chunk_end {
                    let Some(value_page) = metadata_take_next_large_value_page(
                        &mut **file,
                        epoch.metadata_evidence_root,
                        self.digest_algo,
                        epoch.epoch,
                        epoch.reclaim_fence_identity,
                        &mut epoch.metadata_value_blob_cursor,
                        epoch.page_high_water_mark,
                        evidence_page_count,
                        &mut touched_chunks,
                    )?
                    else {
                        break;
                    };
                    if metadata_evidence_chunk_start(value_page) != value_chunk_start {
                        epoch.metadata_value_blob_cursor =
                            epoch.metadata_value_blob_cursor.min(value_page);
                        break;
                    }
                    metadata_mark_reachable_page(
                        &mut **file,
                        self.digest_algo,
                        epoch,
                        value_page,
                        evidence_page_count,
                        &mut touched_chunks,
                    )?;
                }
                visited = visited.saturating_add(1);
                budget -= 1;
                continue;
            }
            if metadata_process_root_expansion_continuation(
                &mut **file,
                self.digest_algo,
                epoch,
                evidence_page_count,
                &mut touched_chunks,
                &packed_locator_tree_pages,
            )? {
                visited = visited.saturating_add(1);
                budget -= 1;
                continue;
            }
            if let Some(page) = metadata_take_next_pending_page(
                &mut **file,
                epoch.metadata_evidence_root,
                self.digest_algo,
                epoch.epoch,
                epoch.reclaim_fence_identity,
                &mut epoch.metadata_root_cursor,
                epoch.page_high_water_mark,
                evidence_page_count,
                &mut touched_chunks,
                MetadataPendingKind::Root,
            )? {
                if page >= epoch.page_high_water_mark {
                    return Err(corrupt("reachability mark metadata root out of range"));
                }
                let chunk = metadata_evidence_chunk_for_page(
                    &mut **file,
                    epoch.metadata_evidence_root,
                    self.digest_algo,
                    epoch.epoch,
                    epoch.reclaim_fence_identity,
                    page,
                    evidence_page_count,
                    &mut touched_chunks,
                )?;
                let value_tree = chunk.contains_value_tree(page)?;
                let free_page_extent_tree = chunk.contains_free_page_extent_tree(page)?;
                let first_visit = chunk.set_reachable(page)?;
                if first_visit {
                    epoch.metadata_reachable_count =
                        epoch.metadata_reachable_count.saturating_add(1);
                    let (child_count, value_count) = if free_page_extent_tree {
                        let Some(links) = crate::pagebtree::free_page_extent_node_links(
                            &mut **file,
                            crate::DATA_START,
                            PageId(page),
                            epoch.page_high_water_mark,
                        )?
                        else {
                            visited = visited.saturating_add(1);
                            budget -= 1;
                            continue;
                        };
                        (links.children.len(), 0)
                    } else {
                        let codec = if packed_locator_tree_pages.contains(&page) {
                            crate::pagebtree::ValueCodecKind::PackedRecordRef
                        } else {
                            crate::pagebtree::ValueCodecKind::RecordLoc
                        };
                        let Some(links) = crate::pagebtree::node_page_links_with_codec(
                            &mut **file,
                            crate::DATA_START,
                            PageId(page),
                            epoch.page_high_water_mark,
                            codec,
                        )?
                        else {
                            visited = visited.saturating_add(1);
                            budget -= 1;
                            continue;
                        };
                        (links.children.len(), links.values.len())
                    };
                    let chunk = metadata_evidence_chunk_for_page(
                        &mut **file,
                        epoch.metadata_evidence_root,
                        self.digest_algo,
                        epoch.epoch,
                        epoch.reclaim_fence_identity,
                        page,
                        evidence_page_count,
                        &mut touched_chunks,
                    )?;
                    chunk.start_root_expansion(
                        page,
                        value_tree,
                        free_page_extent_tree,
                        child_count,
                        value_count,
                    )?;
                    epoch.metadata_expansion_cursor = epoch.metadata_expansion_cursor.min(page);
                    if child_count == 0 {
                        let _ = metadata_process_root_expansion_continuation(
                            &mut **file,
                            self.digest_algo,
                            epoch,
                            evidence_page_count,
                            &mut touched_chunks,
                            &packed_locator_tree_pages,
                        )?;
                        let _ = metadata_process_root_expansion_continuation(
                            &mut **file,
                            self.digest_algo,
                            epoch,
                            evidence_page_count,
                            &mut touched_chunks,
                            &packed_locator_tree_pages,
                        )?;
                    }
                }
                visited = visited.saturating_add(1);
                budget -= 1;
                continue;
            }
            if epoch.metadata_classify_next_page < epoch.page_high_water_mark {
                let chunk_start = metadata_evidence_chunk_start(epoch.metadata_classify_next_page);
                let chunk_end = chunk_start
                    .saturating_add(MARK_EPOCH_CHUNK_PAGES)
                    .min(epoch.page_high_water_mark);
                let mut page = epoch.metadata_classify_next_page;
                while page < chunk_end {
                    if epoch
                        .captured_metadata_bootstrap_reserve
                        .contains_page(page)
                    {
                        let chunk = metadata_evidence_chunk_for_page(
                            &mut **file,
                            epoch.metadata_evidence_root,
                            self.digest_algo,
                            epoch.epoch,
                            epoch.reclaim_fence_identity,
                            page,
                            evidence_page_count,
                            &mut touched_chunks,
                        )?;
                        let (newly_reachable, removed_candidate) =
                            chunk.protect_current_epoch_page(page)?;
                        if newly_reachable {
                            epoch.metadata_reachable_count =
                                epoch.metadata_reachable_count.saturating_add(1);
                        }
                        if removed_candidate {
                            epoch.metadata_reclaim_candidate_count =
                                epoch.metadata_reclaim_candidate_count.saturating_sub(1);
                        }
                    } else if !metadata_evidence_page_reachable(
                        &mut **file,
                        epoch.metadata_evidence_root,
                        self.digest_algo,
                        epoch.epoch,
                        epoch.reclaim_fence_identity,
                        page,
                        evidence_page_count,
                        &touched_chunks,
                    )? && page_contains_reclaimable_metadata(
                        &mut **file,
                        page,
                        epoch.page_high_water_mark,
                    )? {
                        let chunk = metadata_evidence_chunk_for_page(
                            &mut **file,
                            epoch.metadata_evidence_root,
                            self.digest_algo,
                            epoch.epoch,
                            epoch.reclaim_fence_identity,
                            page,
                            evidence_page_count,
                            &mut touched_chunks,
                        )?;
                        if chunk.set_reclaim_candidate(page)? {
                            epoch.metadata_reclaim_candidate_count =
                                epoch.metadata_reclaim_candidate_count.saturating_add(1);
                        }
                    }
                    page = page.saturating_add(1);
                }
                epoch.metadata_classify_next_page = chunk_end;
                visited = visited.saturating_add(1);
                budget -= 1;
                continue;
            }
            epoch.metadata_completed = true;
            break;
        }
        if epoch.metadata_work_initialized
            && epoch.metadata_classify_next_page >= epoch.page_high_water_mark
            && epoch.metadata_root_cursor >= epoch.page_high_water_mark
            && epoch.metadata_value_root_cursor >= epoch.page_high_water_mark
            && epoch.metadata_value_blob_cursor >= epoch.page_high_water_mark
            && epoch.metadata_expansion_cursor >= epoch.page_high_water_mark
        {
            epoch.metadata_completed = true;
        }
        drop(file);
        if !touched_chunks.is_empty() {
            self.publish_metadata_evidence_chunks_and_epoch(epoch, touched_chunks)?;
        }
        Ok(visited)
    }

    #[cfg(test)]
    pub(crate) fn reachability_mark_metadata_page_state_for_test(
        &self,
        epoch: &ReachabilityMarkEpoch,
        page: u64,
    ) -> Result<(bool, bool)> {
        let Some(root) = epoch.metadata_evidence_root else {
            return Ok((false, false));
        };
        let page_count = self.inner.lock().map_err(|_| crate::poisoned())?.page_count;
        let mut file = self.file.lock().map_err(|_| crate::poisoned())?;
        let chunk = read_metadata_evidence_chunk(
            &mut **file,
            Some(root),
            self.digest_algo,
            epoch.epoch,
            epoch.reclaim_fence_identity,
            metadata_evidence_chunk_start(page),
            page_count,
        )?;
        Ok(chunk.map_or((false, false), |chunk| {
            (
                chunk.contains_reachable(page).unwrap_or(false),
                metadata_bit(&chunk.reclaim_candidate, chunk.page_start, page),
            )
        }))
    }

    fn publish_metadata_evidence_chunks_and_epoch(
        &self,
        epoch: &mut ReachabilityMarkEpoch,
        mut chunks: BTreeMap<u64, MetadataEvidenceChunk>,
    ) -> Result<()> {
        if chunks.is_empty() {
            return self.save_reachability_mark_epoch(epoch);
        }
        let _publication_guard = self
            .overlay_publication
            .lock()
            .map_err(|_| crate::poisoned())?;
        let mut inner = self.inner.lock().map_err(|_| crate::poisoned())?;
        let new_gen = inner.generation + 1;
        let (reusable_free, reclamation_lease) = self.transaction_reusable_free(
            &inner.free,
            inner.active_mark_epoch_reclaim_fence,
            inner.minimum_recoverable_generation,
        )?;
        let mut next_evidence_root = epoch.metadata_evidence_root.map(PageId);
        let mut control_map = self.control_map_locked(&mut inner)?;
        let persisted_epoch = control_map
            .get(MARK_EPOCH_KEY)
            .map(|bytes| decode_mark_epoch(bytes, self.digest_algo))
            .transpose()?
            .ok_or_else(|| corrupt("reachability mark epoch disappeared during publication"))?;
        persisted_epoch.require_current_metadata_bootstrap_evidence()?;
        if persisted_epoch.epoch != epoch.epoch
            || persisted_epoch.base_generation != epoch.base_generation
            || persisted_epoch.page_high_water_mark != epoch.page_high_water_mark
            || persisted_epoch.captured_metadata_bootstrap_reserve
                != epoch.captured_metadata_bootstrap_reserve
            || persisted_epoch.captured_free_root != epoch.captured_free_root
            || persisted_epoch.captured_free_identity != epoch.captured_free_identity
        {
            return Err(corrupt("reachability mark captured-free state widened"));
        }
        if persisted_epoch.captured_free_consumed_through < epoch.captured_free_consumed_through {
            return Err(corrupt("reachability mark captured-free cursor regression"));
        }
        epoch.captured_free_consumed_through = persisted_epoch.captured_free_consumed_through;
        let mut next_epoch = epoch.clone();
        let (roots, placements, next_control_digest) = {
            let mut file = self.file.lock().map_err(|_| crate::poisoned())?;
            let captured_reuse = if reclamation_lease.allowed {
                captured_free_reuse_runs(
                    &mut **file,
                    self.digest_algo,
                    epoch,
                    &inner.free,
                    inner.minimum_recoverable_generation,
                    MARK_EPOCH_CAPTURED_FREE_PUBLICATION_PAGES,
                )?
            } else {
                CapturedFreeReuseSelection::default()
            };
            if !captured_reuse.runs.is_empty() {
                for run in &captured_reuse.runs {
                    for page in run.start..run.start.saturating_add(run.len) {
                        let chunk = metadata_evidence_chunk_for_page(
                            &mut **file,
                            epoch.metadata_evidence_root,
                            self.digest_algo,
                            epoch.epoch,
                            epoch.reclaim_fence_identity,
                            page,
                            inner.page_count,
                            &mut chunks,
                        )?;
                        let (newly_reachable, removed_candidate) =
                            chunk.protect_current_epoch_page(page)?;
                        if newly_reachable {
                            next_epoch.metadata_reachable_count =
                                next_epoch.metadata_reachable_count.saturating_add(1);
                        }
                        if removed_candidate {
                            next_epoch.metadata_reclaim_candidate_count = next_epoch
                                .metadata_reclaim_candidate_count
                                .saturating_sub(1);
                        }
                    }
                }
                next_epoch.captured_free_consumed_through = captured_reuse.consumed_through;
            }
            let records = chunks
                .iter()
                .map(|(page_start, chunk)| {
                    (
                        metadata_evidence_chunk_key(
                            self.digest_algo,
                            epoch.epoch,
                            epoch.reclaim_fence_identity,
                            *page_start,
                        ),
                        encode_metadata_evidence_chunk(chunk),
                    )
                })
                .collect::<Vec<_>>();
            let mut reusable_free = reusable_free;
            reusable_free.extend(captured_reuse.runs);
            reusable_free.sort_by_key(|run| run.start);
            let mut alloc = crate::PageAllocator::new_with_reusable_runs(
                inner.page_count,
                new_gen,
                inner.free.clone(),
                reusable_free,
            );
            alloc.install_metadata_bootstrap_reserve(&inner.metadata_bootstrap_reserve)?;
            let mut evidence_entries = next_evidence_root
                .map(|root| {
                    crate::root_family_load_all(
                        &mut **file,
                        RECLAIM_INDEX_FAMILY_ID,
                        root,
                        inner.page_count,
                    )
                })
                .transpose()?
                .unwrap_or_default()
                .into_iter()
                .collect::<BTreeMap<_, _>>();
            if reclamation_lease.allowed {
                if let Some(root) = next_evidence_root {
                    for page in crate::root_family_collect_pages(
                        &mut **file,
                        RECLAIM_INDEX_FAMILY_ID,
                        root,
                        inner.page_count,
                    )? {
                        alloc.free(page, 1)?;
                    }
                }
                for (address, _) in &records {
                    if let Some(loc) = evidence_entries.get(address) {
                        for page in crate::record_io::blob_pages(
                            &mut **file,
                            loc.global_page(),
                            inner.page_count,
                        )? {
                            alloc.free(PageId(page), 1)?;
                        }
                    }
                }
            }
            let record_refs = records
                .iter()
                .map(|(address, value)| (*address, value.as_slice()))
                .collect::<Vec<_>>();
            for (address, loc) in
                crate::record_io::write_dedicated_blob_pages(&mut **file, &mut alloc, &record_refs)?
            {
                evidence_entries.insert(address, loc);
            }
            let packed_entries = evidence_entries.into_iter().collect::<Vec<_>>();
            next_evidence_root = crate::pagebtree::build_packed_with_codec(
                &mut **file,
                crate::DATA_START,
                &mut alloc,
                &packed_entries,
                crate::root_family_value_codec(RECLAIM_INDEX_FAMILY_ID)?,
            )?;
            next_epoch.metadata_evidence_root = next_evidence_root.map(|root| root.0);
            next_epoch.metadata_evidence_identity = mark_epoch_metadata_evidence_identity(
                self.digest_algo,
                next_epoch.metadata_evidence_root,
                next_epoch.metadata_reachable_count,
                next_epoch.metadata_reclaim_candidate_count,
            );
            control_map.insert(MARK_EPOCH_KEY.to_vec(), encode_mark_epoch(&next_epoch)?);
            let control_bytes = encode_control_map(&control_map);
            let control_digest = Digest::hash(self.digest_algo, &control_bytes);
            let fresh = vec![(control_digest, control_bytes.as_slice(), self.default_codec)];
            let dek = self.dek.lock().map_err(|_| crate::poisoned())?;
            let placements =
                crate::write_record_pages(&mut **file, &mut alloc, &fresh, dek.as_ref())?;
            drop(dek);
            let index_batch = crate::pagebtree::batch_upsert(
                &mut **file,
                crate::DATA_START,
                &mut alloc,
                inner.index_root,
                &placements,
                inner.page_count,
            )?;
            #[cfg(any(test, feature = "test-hooks"))]
            crate::observe_object_index_batch(index_batch.stats);
            let index_root = index_batch.root;
            let root_catalog_entries = crate::root_catalog_entries_with_family(
                &inner.root_catalog_entries,
                RECLAIM_INDEX_FAMILY_ID,
                next_evidence_root,
            );
            let root_catalog_root = crate::write_root_catalog_page(
                &mut **file,
                &mut alloc,
                inner.root_catalog_root,
                inner.page_count,
                &root_catalog_entries,
            )?;
            let touched_segments = placements
                .iter()
                .map(|(_, loc)| loc.segment_id)
                .collect::<BTreeSet<_>>();
            let object_count = inner
                .maintenance
                .object_count
                .saturating_add(fresh.len() as u64);
            let roots = crate::finish_txn(
                &mut **file,
                &mut alloc,
                new_gen,
                object_count,
                crate::TxnRootInputs {
                    object_index: index_root,
                    legacy_overlay: crate::legacy_overlay_root_for_publication(
                        &inner,
                        inner.current_record_root,
                        root_catalog_root,
                    ),
                    current_records: inner.current_record_root,
                    root_catalog: crate::TxnRootCatalog {
                        root: root_catalog_root,
                        entries: root_catalog_entries.clone(),
                    },
                    previous_mutable_overlay_generation_floor: inner
                        .mutable_overlay_generation_floor,
                    mutable_overlay_generation_floor: inner.mutable_overlay_generation_floor,
                    reference: inner.reference_root.map(|digest| *digest.bytes()),
                    control: Some(*control_digest.bytes()),
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
                Some(&self.group_commit_metrics),
            )?;
            (roots, placements, control_digest)
        };
        self.adopt_committed_roots_locked(&mut inner, roots)?;
        *epoch = next_epoch;
        inner.active_mark_epoch_reclaim_fence = Some(epoch.page_high_water_mark);
        for (key, loc) in placements {
            Self::cache_locator_locked(&mut inner, key, loc);
        }
        let _ = next_control_digest;
        Ok(())
    }

    pub fn derived_artifact_roots(&self) -> Result<BTreeSet<Digest>> {
        Ok(self
            .derived_payload_digests()?
            .into_iter()
            .map(|bytes| Digest::of(self.digest_algo, bytes))
            .collect())
    }

    pub(crate) fn control_reachability_fingerprint_from_map(
        &self,
        map: &std::collections::BTreeMap<Vec<u8>, Vec<u8>>,
    ) -> Option<Digest> {
        let mut map = map.clone();
        map.remove(MARK_EPOCH_KEY);
        map.remove(MARK_EPOCH_RECLAIM_EVIDENCE_KEY);
        map.remove(MAINTENANCE_POLICY_KEY);
        map.remove(MAINTENANCE_RUN_KEY);
        if map.is_empty() {
            return None;
        }
        let bytes = crate::record_io::encode_control_map(&map);
        Some(Digest::hash(self.digest_algo, &bytes))
    }

    pub(crate) fn write_control_map_validating_mark_epoch(
        &self,
        map: std::collections::BTreeMap<Vec<u8>, Vec<u8>>,
        epoch: u64,
    ) -> Result<()> {
        if map.is_empty() {
            return self.commit_txn(&[], None, Some(None), Some(epoch));
        }
        let bytes = crate::record_io::encode_control_map(&map);
        let digest = Digest::hash(self.digest_algo, &bytes);
        let codec = self.default_codec;
        self.commit_txn(
            &[(digest, bytes.as_slice(), codec)],
            None,
            Some(Some(*digest.bytes())),
            Some(epoch),
        )
    }

    pub(crate) fn set_active_reachability_mark_epoch_reclaim_fence(
        &self,
        high_water_mark: Option<u64>,
    ) -> Result<()> {
        self.inner
            .lock()
            .map_err(|_| crate::poisoned())?
            .active_mark_epoch_reclaim_fence = high_water_mark;
        Ok(())
    }

    fn publish_reachability_mark_epoch_begin_locked(
        &self,
        inner: &mut crate::Inner,
        control_map: &mut BTreeMap<Vec<u8>, Vec<u8>>,
        epoch: &ReachabilityMarkEpoch,
    ) -> Result<()> {
        control_map.insert(MARK_EPOCH_KEY.to_vec(), encode_mark_epoch(epoch)?);
        control_map.remove(MARK_EPOCH_RECLAIM_EVIDENCE_KEY);
        let control_bytes = encode_control_map(control_map);
        let control_digest = Digest::hash(self.digest_algo, &control_bytes);
        let mut fresh = Vec::new();
        if self
            .lookup_loc_locked(inner, control_digest.bytes())?
            .is_none()
        {
            fresh.push((control_digest, control_bytes.as_slice(), self.default_codec));
        }
        let new_gen = inner.generation + 1;
        let (reusable_free, _reclamation_lease) = self.transaction_reusable_free(
            &inner.free,
            Some(epoch.page_high_water_mark),
            inner.minimum_recoverable_generation,
        )?;
        let (roots, placements) = {
            let mut file = self.file.lock().map_err(|_| crate::poisoned())?;
            let mut alloc = crate::PageAllocator::new_with_reusable_runs(
                inner.page_count,
                new_gen,
                inner.free.clone(),
                reusable_free,
            );
            alloc.install_metadata_bootstrap_reserve(&inner.metadata_bootstrap_reserve)?;
            let dek = self.dek.lock().map_err(|_| crate::poisoned())?;
            let placements =
                crate::write_record_pages(&mut **file, &mut alloc, &fresh, dek.as_ref())?;
            drop(dek);
            let index_batch = crate::pagebtree::batch_upsert(
                &mut **file,
                crate::DATA_START,
                &mut alloc,
                inner.index_root,
                &placements,
                inner.page_count,
            )?;
            #[cfg(any(test, feature = "test-hooks"))]
            crate::observe_object_index_batch(index_batch.stats);
            let index_root = index_batch.root;
            self.run_reachability_epoch_pre_finish_hook()?;
            let touched_segments = placements
                .iter()
                .map(|(_, loc)| loc.segment_id)
                .collect::<BTreeSet<_>>();
            let object_count = inner
                .maintenance
                .object_count
                .saturating_add(fresh.len() as u64);
            let roots = crate::finish_txn(
                &mut **file,
                &mut alloc,
                new_gen,
                object_count,
                crate::TxnRootInputs {
                    object_index: index_root,
                    legacy_overlay: crate::legacy_overlay_root_for_publication(
                        inner,
                        inner.current_record_root,
                        inner.root_catalog_root,
                    ),
                    current_records: inner.current_record_root,
                    root_catalog: crate::TxnRootCatalog {
                        root: inner.root_catalog_root,
                        entries: inner.root_catalog_entries.clone(),
                    },
                    previous_mutable_overlay_generation_floor: inner
                        .mutable_overlay_generation_floor,
                    mutable_overlay_generation_floor: inner.mutable_overlay_generation_floor,
                    reference: inner.reference_root.map(|digest| *digest.bytes()),
                    control: Some(*control_digest.bytes()),
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
                Some(&self.group_commit_metrics),
            )?;
            (roots, placements)
        };
        self.adopt_committed_roots_locked(inner, roots)?;
        inner.active_mark_epoch_reclaim_fence = Some(epoch.page_high_water_mark);
        for (key, loc) in placements {
            Self::cache_locator_locked(inner, key, loc);
        }
        Ok(())
    }

    fn publish_reachability_mark_epoch_clear_locked(
        &self,
        inner: &mut crate::Inner,
        control_map: &BTreeMap<Vec<u8>, Vec<u8>>,
    ) -> Result<()> {
        let control_bytes = (!control_map.is_empty()).then(|| encode_control_map(control_map));
        let control_digest = control_bytes
            .as_ref()
            .map(|bytes| Digest::hash(self.digest_algo, bytes));
        let mut fresh = Vec::new();
        if let (Some(digest), Some(bytes)) = (control_digest, control_bytes.as_ref())
            && self.lookup_loc_locked(inner, digest.bytes())?.is_none()
        {
            fresh.push((digest, bytes.as_slice(), self.default_codec));
        }
        let new_gen = inner.generation + 1;
        let (roots, placements) = {
            let mut file = self.file.lock().map_err(|_| crate::poisoned())?;
            let reclaim_index_pages = if let Some(root) = inner
                .root_catalog_entries
                .iter()
                .find(|entry| entry.family_id == RECLAIM_INDEX_FAMILY_ID)
                .map(|entry| entry.root)
            {
                let mut pages = crate::root_family_collect_pages(
                    &mut **file,
                    RECLAIM_INDEX_FAMILY_ID,
                    root,
                    inner.page_count,
                )?
                .into_iter()
                .map(|page| page.0)
                .collect::<BTreeSet<_>>();
                for (_, loc) in crate::root_family_load_all(
                    &mut **file,
                    RECLAIM_INDEX_FAMILY_ID,
                    root,
                    inner.page_count,
                )? {
                    pages.extend(crate::record_io::blob_pages(
                        &mut **file,
                        loc.global_page(),
                        inner.page_count,
                    )?);
                }
                pages
            } else {
                BTreeSet::new()
            };
            let mut alloc =
                crate::PageAllocator::new(inner.page_count, new_gen, inner.free.clone());
            alloc.install_metadata_bootstrap_reserve(&inner.metadata_bootstrap_reserve)?;
            for page in reclaim_index_pages {
                if !inner
                    .free
                    .iter()
                    .any(|run| page >= run.start && page < run.start.saturating_add(run.len))
                {
                    alloc.free(PageId(page), 1)?;
                }
            }
            let dek = self.dek.lock().map_err(|_| crate::poisoned())?;
            let placements =
                crate::write_record_pages(&mut **file, &mut alloc, &fresh, dek.as_ref())?;
            drop(dek);
            let index_batch = crate::pagebtree::batch_upsert(
                &mut **file,
                crate::DATA_START,
                &mut alloc,
                inner.index_root,
                &placements,
                inner.page_count,
            )?;
            #[cfg(any(test, feature = "test-hooks"))]
            crate::observe_object_index_batch(index_batch.stats);
            let index_root = index_batch.root;
            let had_reclaim_index = inner
                .root_catalog_entries
                .iter()
                .any(|entry| entry.family_id == RECLAIM_INDEX_FAMILY_ID);
            let root_catalog_entries = had_reclaim_index
                .then(|| {
                    crate::root_catalog_entries_with_family(
                        &inner.root_catalog_entries,
                        RECLAIM_INDEX_FAMILY_ID,
                        None,
                    )
                })
                .unwrap_or_else(|| inner.root_catalog_entries.clone());
            let root_catalog_root = if !had_reclaim_index {
                inner.root_catalog_root
            } else if root_catalog_entries.is_empty() {
                if let Some(root) = inner.root_catalog_root {
                    alloc.free(root, 1)?;
                }
                None
            } else {
                crate::write_root_catalog_page(
                    &mut **file,
                    &mut alloc,
                    inner.root_catalog_root,
                    inner.page_count,
                    &root_catalog_entries,
                )?
            };
            let touched_segments = placements
                .iter()
                .map(|(_, loc)| loc.segment_id)
                .collect::<BTreeSet<_>>();
            self.run_reachability_epoch_pre_finish_hook()?;
            crate::finish_txn(
                &mut **file,
                &mut alloc,
                new_gen,
                inner.maintenance.object_count,
                crate::TxnRootInputs {
                    object_index: index_root,
                    legacy_overlay: crate::legacy_overlay_root_for_publication(
                        inner,
                        inner.current_record_root,
                        root_catalog_root,
                    ),
                    current_records: inner.current_record_root,
                    root_catalog: crate::TxnRootCatalog {
                        root: root_catalog_root,
                        entries: root_catalog_entries,
                    },
                    previous_mutable_overlay_generation_floor: inner
                        .mutable_overlay_generation_floor,
                    mutable_overlay_generation_floor: inner.mutable_overlay_generation_floor,
                    reference: inner.reference_root.map(|digest| *digest.bytes()),
                    control: control_digest.map(|digest| *digest.bytes()),
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
                Some(&self.group_commit_metrics),
            )
            .map(|roots| (roots, placements))
        }?;
        self.adopt_committed_roots_locked(inner, roots)?;
        inner.active_mark_epoch_reclaim_fence = None;
        for (key, loc) in placements {
            Self::cache_locator_locked(inner, key, loc);
        }
        Ok(())
    }
}

pub fn begin_loom_reachability_mark_epoch(loom: &Loom<FileStore>) -> Result<ReachabilityMarkEpoch> {
    let store = loom.store();
    let reference_root = store.reference_root();
    let derived_roots = store.derived_artifact_roots()?;
    let pinned_roots = reference_root
        .into_iter()
        .chain(derived_roots.iter().copied())
        .collect::<Vec<_>>();
    let state = loom.begin_live_object_mark(pinned_roots)?;
    store.begin_reachability_mark_epoch(reference_root, derived_roots, state)
}

pub fn step_loom_reachability_mark_epoch(
    loom: &Loom<FileStore>,
    budget: usize,
) -> Result<ReachabilityMarkStep> {
    step_loom_reachability_mark_epoch_until(loom, budget, None)
}

pub fn step_loom_reachability_mark_epoch_until(
    loom: &Loom<FileStore>,
    budget: usize,
    deadline: Option<std::time::Instant>,
) -> Result<ReachabilityMarkStep> {
    let deadline_expired = deadline.map(|deadline| move || std::time::Instant::now() >= deadline);
    step_loom_reachability_mark_epoch_while(
        loom,
        budget,
        deadline_expired
            .as_ref()
            .map(|deadline| deadline as &dyn Fn() -> bool),
    )
}

pub fn step_loom_reachability_mark_epoch_while(
    loom: &Loom<FileStore>,
    budget: usize,
    deadline_expired: Option<&dyn Fn() -> bool>,
) -> Result<ReachabilityMarkStep> {
    let store = loom.store();
    let mut epoch = store
        .active_reachability_mark_epoch()?
        .ok_or_else(|| LoomError::not_found("reachability mark epoch not found"))?;
    let mut total_visited = 0usize;
    while total_visited < budget {
        if deadline_expired.is_some_and(|expired| expired()) {
            break;
        }
        let mut made_progress = false;
        if !epoch.state.completed {
            let step = loom.step_live_object_mark(&mut epoch.state, 1)?;
            if step.visited > 0 {
                made_progress = true;
            }
            total_visited = total_visited.saturating_add(step.visited);
        }
        if total_visited < budget && !epoch.metadata_completed {
            let metadata_visited = store.step_reachability_metadata_mark_epoch(
                &mut epoch,
                budget.saturating_sub(total_visited),
                deadline_expired,
            )?;
            if metadata_visited > 0 {
                made_progress = true;
            }
            total_visited = total_visited.saturating_add(metadata_visited);
        }
        if !made_progress {
            break;
        }
    }
    let step = ReachabilityMarkStep {
        visited: total_visited,
        pending: epoch.state.queue.len()
            + epoch.state.stream_roots.len()
            + epoch.state.content_roots.len()
            + epoch.state.prolly_cursors.len()
            + usize::try_from(
                epoch
                    .page_high_water_mark
                    .saturating_sub(epoch.metadata_classify_next_page),
            )
            .unwrap_or(usize::MAX),
        completed: epoch.state.completed && epoch.metadata_completed,
    };
    if step.completed {
        store.complete_reachability_mark_epoch(&epoch)?;
    } else {
        store.save_reachability_mark_epoch(&epoch)?;
    }
    Ok(step)
}

fn encode_mark_epoch(epoch: &ReachabilityMarkEpoch) -> Result<Vec<u8>> {
    epoch.require_current_metadata_bootstrap_evidence()?;
    let mut out = Vec::new();
    out.extend_from_slice(MARK_EPOCH_MAGIC);
    out.extend_from_slice(&MARK_EPOCH_VERSION.to_le_bytes());
    out.extend_from_slice(&epoch.epoch.to_le_bytes());
    out.extend_from_slice(&epoch.base_generation.to_le_bytes());
    out.extend_from_slice(&epoch.page_high_water_mark.to_le_bytes());
    put_digest_list(&mut out, &epoch.captured_root_vector);
    put_u64_list(&mut out, &epoch.captured_metadata_roots);
    put_u64_list(&mut out, &epoch.captured_metadata_value_roots);
    put_metadata_bootstrap_reserve(&mut out, &epoch.captured_metadata_bootstrap_reserve);
    put_optional_u64(&mut out, epoch.captured_free_root);
    put_optional_digest(&mut out, epoch.captured_free_identity);
    out.extend_from_slice(&epoch.captured_free_consumed_through.to_le_bytes());
    out.push(u8::from(epoch.metadata_work_initialized));
    out.extend_from_slice(&epoch.metadata_root_cursor.to_le_bytes());
    out.extend_from_slice(&epoch.metadata_value_root_cursor.to_le_bytes());
    out.extend_from_slice(&epoch.metadata_value_blob_cursor.to_le_bytes());
    out.extend_from_slice(&epoch.metadata_expansion_cursor.to_le_bytes());
    out.extend_from_slice(&epoch.metadata_classify_next_page.to_le_bytes());
    put_optional_u64(&mut out, epoch.metadata_evidence_root);
    out.extend_from_slice(&epoch.metadata_reachable_count.to_le_bytes());
    out.extend_from_slice(&epoch.metadata_reclaim_candidate_count.to_le_bytes());
    out.extend_from_slice(epoch.metadata_evidence_identity.bytes());
    out.push(u8::from(epoch.metadata_completed));
    out.extend_from_slice(epoch.reclaim_fence_identity.bytes());
    out.push(u8::from(epoch.state.completed));
    put_optional_digest(&mut out, epoch.reference_root);
    put_optional_digest(&mut out, epoch.control_fingerprint);
    put_optional_digest(&mut out, epoch.canonical_roots_fingerprint);
    put_digest_set(&mut out, &epoch.derived_roots);
    put_digest_set(&mut out, &epoch.state.pinned);
    put_digest_set(&mut out, &epoch.state.marked);
    put_digest_queue(&mut out, &epoch.state.queue);
    put_stream_root_queue(&mut out, &epoch.state.stream_roots);
    put_digest_queue(&mut out, &epoch.state.content_roots);
    put_prolly_cursor_queue(&mut out, &epoch.state.prolly_cursors);
    Ok(out)
}

#[derive(Clone, Copy)]
enum Version8MetadataLayout {
    Scalar,
    Queue,
}

fn decode_mark_epoch(bytes: &[u8], algo: Algo) -> Result<ReachabilityMarkEpoch> {
    let mut header = Cursor { bytes, pos: 0 };
    if header.take(MARK_EPOCH_MAGIC.len())? != MARK_EPOCH_MAGIC {
        return Err(corrupt("reachability mark epoch magic"));
    }
    let version = header.u16()?;
    if version == 8 {
        let scalar =
            decode_mark_epoch_with_layout(bytes, algo, Some(Version8MetadataLayout::Scalar));
        let queue = decode_mark_epoch_with_layout(bytes, algo, Some(Version8MetadataLayout::Queue));
        return match (scalar, queue) {
            (Ok(epoch), Err(_)) | (Err(_), Ok(epoch)) => Ok(epoch),
            (Ok(_), Ok(_)) => Err(corrupt(
                "ambiguous reachability mark epoch version 8 layout",
            )),
            (Err(_), Err(_)) => Err(corrupt("invalid reachability mark epoch version 8 layout")),
        };
    }
    decode_mark_epoch_with_layout(bytes, algo, None)
}

fn decode_mark_epoch_with_layout(
    bytes: &[u8],
    algo: Algo,
    version8_layout: Option<Version8MetadataLayout>,
) -> Result<ReachabilityMarkEpoch> {
    let mut cur = Cursor { bytes, pos: 0 };
    if cur.take(MARK_EPOCH_MAGIC.len())? != MARK_EPOCH_MAGIC {
        return Err(corrupt("reachability mark epoch magic"));
    }
    let version = cur.u16()?;
    if !(1..=MARK_EPOCH_VERSION).contains(&version) {
        return Err(corrupt("reachability mark epoch version"));
    }
    let epoch = cur.u64()?;
    let base_generation = cur.u64()?;
    let (
        page_high_water_mark,
        captured_root_vector,
        captured_metadata_roots,
        captured_metadata_value_roots,
        captured_metadata_bootstrap_reserve,
        captured_free_root,
        captured_free_identity,
        captured_free_consumed_through,
        metadata_work_initialized,
        metadata_root_cursor,
        metadata_value_root_cursor,
        metadata_value_blob_cursor,
        metadata_expansion_cursor,
        metadata_classify_next_page,
        metadata_evidence_root,
        metadata_reachable_count,
        metadata_reclaim_candidate_count,
        metadata_evidence_identity,
        metadata_completed,
        reclaim_fence_identity,
    ) = if version >= 5 {
        let page_high_water_mark = cur.u64()?;
        let captured_root_vector = cur.digest_list(algo)?;
        let captured_metadata_roots = if version >= 7 {
            cur.u64_list()?
        } else {
            Vec::new()
        };
        let captured_metadata_value_roots = if version >= 7 {
            cur.u64_list()?
        } else {
            Vec::new()
        };
        let captured_metadata_bootstrap_reserve = if version >= 14 {
            cur.metadata_bootstrap_reserve(page_high_water_mark)?
        } else {
            MetadataBootstrapReserve::default()
        };
        let (captured_free_root, captured_free_identity, captured_free_consumed_through) =
            if version >= 11 {
                let root = cur.optional_u64()?;
                let identity = cur.optional_digest(algo)?;
                let consumed_through = cur.u64()?;
                if version >= 13 {
                    (root, identity, consumed_through)
                } else {
                    (None, None, 0)
                }
            } else {
                (None, None, 0)
            };
        let (
            metadata_work_initialized,
            metadata_root_cursor,
            metadata_value_root_cursor,
            metadata_value_blob_cursor,
            metadata_expansion_cursor,
            metadata_classify_next_page,
            metadata_evidence_root,
            metadata_reachable_count,
            metadata_reclaim_candidate_count,
            metadata_evidence_identity,
            metadata_completed,
        ) = if version >= 10 {
            let metadata_work_initialized = match cur.u8()? {
                0 => false,
                1 => true,
                _ => {
                    return Err(corrupt("reachability mark epoch metadata initialized flag"));
                }
            };
            let metadata_root_cursor = cur.u64()?;
            let metadata_value_root_cursor = cur.u64()?;
            let metadata_value_blob_cursor = cur.u64()?;
            let metadata_expansion_cursor = cur.u64()?;
            let metadata_classify_next_page = cur.u64()?;
            let metadata_evidence_root = cur.optional_u64()?;
            let metadata_reachable_count = cur.u64()?;
            let metadata_reclaim_candidate_count = cur.u64()?;
            let metadata_evidence_identity = cur.digest(algo)?;
            let metadata_completed = match cur.u8()? {
                0 => false,
                1 => true,
                _ => return Err(corrupt("reachability mark epoch metadata completed flag")),
            };
            (
                metadata_work_initialized,
                metadata_root_cursor,
                metadata_value_root_cursor,
                metadata_value_blob_cursor,
                metadata_expansion_cursor,
                metadata_classify_next_page,
                metadata_evidence_root,
                metadata_reachable_count,
                metadata_reclaim_candidate_count,
                metadata_evidence_identity,
                metadata_completed,
            )
        } else if version == 9 {
            let metadata_work_initialized = match cur.u8()? {
                0 => false,
                1 => true,
                _ => {
                    return Err(corrupt("reachability mark epoch metadata initialized flag"));
                }
            };
            let metadata_root_cursor = cur.u64()?;
            let metadata_value_root_cursor = cur.u64()?;
            let metadata_value_blob_cursor = cur.u64()?;
            let metadata_classify_next_page = cur.u64()?;
            let metadata_evidence_root = cur.optional_u64()?;
            let metadata_reachable_count = cur.u64()?;
            let metadata_reclaim_candidate_count = cur.u64()?;
            let metadata_evidence_identity = cur.digest(algo)?;
            let metadata_completed = match cur.u8()? {
                0 => false,
                1 => true,
                _ => return Err(corrupt("reachability mark epoch metadata completed flag")),
            };
            (
                metadata_work_initialized,
                metadata_root_cursor,
                metadata_value_root_cursor,
                metadata_value_blob_cursor,
                page_high_water_mark,
                metadata_classify_next_page,
                metadata_evidence_root,
                metadata_reachable_count,
                metadata_reclaim_candidate_count,
                metadata_evidence_identity,
                metadata_completed,
            )
        } else if version == 8 {
            match version8_layout.ok_or_else(|| corrupt("missing version 8 metadata layout"))? {
                Version8MetadataLayout::Scalar => {
                    decode_version8_scalar_metadata_epoch(&mut cur, algo)?
                }
                Version8MetadataLayout::Queue => {
                    decode_version8_queue_metadata_epoch(&mut cur, algo)?
                }
            }
        } else {
            (
                true,
                page_high_water_mark,
                page_high_water_mark,
                page_high_water_mark,
                page_high_water_mark,
                page_high_water_mark,
                None,
                0,
                0,
                mark_epoch_metadata_evidence_identity(algo, None, 0, 0),
                true,
            )
        };
        let reclaim_fence_identity = cur.digest(algo)?;
        (
            page_high_water_mark,
            captured_root_vector,
            captured_metadata_roots,
            captured_metadata_value_roots,
            captured_metadata_bootstrap_reserve,
            captured_free_root,
            captured_free_identity,
            captured_free_consumed_through,
            metadata_work_initialized,
            metadata_root_cursor,
            metadata_value_root_cursor,
            metadata_value_blob_cursor,
            metadata_expansion_cursor,
            metadata_classify_next_page,
            metadata_evidence_root,
            metadata_reachable_count,
            metadata_reclaim_candidate_count,
            metadata_evidence_identity,
            metadata_completed,
            reclaim_fence_identity,
        )
    } else {
        (
            u64::MAX,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            MetadataBootstrapReserve::default(),
            None,
            None,
            0,
            true,
            u64::MAX,
            u64::MAX,
            u64::MAX,
            u64::MAX,
            u64::MAX,
            None,
            0,
            0,
            mark_epoch_metadata_evidence_identity(algo, None, 0, 0),
            true,
            mark_epoch_reclaim_fence_identity(
                algo,
                epoch,
                base_generation,
                u64::MAX,
                &[],
                &[],
                &[],
                &MetadataBootstrapReserve::default(),
            ),
        )
    };
    let completed = match cur.u8()? {
        0 => false,
        1 => true,
        _ => return Err(corrupt("reachability mark epoch completed flag")),
    };
    let reference_root = cur.optional_digest(algo)?;
    let control_fingerprint = if version >= 2 {
        cur.optional_digest(algo)?
    } else {
        None
    };
    let canonical_roots_fingerprint = if version >= 4 {
        cur.optional_digest(algo)?
    } else {
        None
    };
    let derived_roots = cur.digest_set(algo)?;
    let pinned = cur.digest_set(algo)?;
    let marked = cur.digest_set(algo)?;
    let queue = cur.digest_queue(algo)?;
    let stream_roots = if version >= 6 {
        cur.stream_root_queue(algo)?
    } else {
        cur.digest_queue(algo)?
            .into_iter()
            .map(|root| ReachabilityStreamRoot {
                root,
                retained_low_water: 0,
            })
            .collect()
    };
    let content_roots = if version >= 3 {
        cur.digest_queue(algo)?
    } else {
        VecDeque::new()
    };
    let prolly_cursors = if version >= 3 {
        cur.prolly_cursor_queue(algo, version)?
    } else {
        VecDeque::new()
    };
    if cur.pos != bytes.len() {
        return Err(corrupt("reachability mark epoch trailing bytes"));
    }
    Ok(ReachabilityMarkEpoch {
        epoch,
        base_generation,
        page_high_water_mark,
        captured_root_vector,
        captured_metadata_roots,
        captured_metadata_value_roots,
        captured_metadata_bootstrap_reserve,
        metadata_bootstrap_evidence_provenance: if version >= 14 {
            MetadataBootstrapEvidenceProvenance::Current
        } else {
            MetadataBootstrapEvidenceProvenance::Legacy
        },
        captured_free_root,
        captured_free_identity,
        captured_free_consumed_through,
        metadata_work_initialized,
        metadata_root_cursor,
        metadata_value_root_cursor,
        metadata_value_blob_cursor,
        metadata_expansion_cursor,
        metadata_classify_next_page,
        metadata_evidence_root,
        metadata_reachable_count,
        metadata_reclaim_candidate_count,
        metadata_evidence_identity,
        metadata_completed,
        reclaim_fence_identity,
        reference_root,
        control_fingerprint,
        canonical_roots_fingerprint,
        derived_roots,
        state: ReachabilityMarkState {
            pinned,
            marked,
            queue,
            stream_roots,
            content_roots,
            prolly_cursors,
            completed,
        },
    })
}

fn put_optional_digest(out: &mut Vec<u8>, digest: Option<Digest>) {
    match digest {
        Some(digest) => {
            out.push(1);
            out.extend_from_slice(digest.bytes());
        }
        None => out.push(0),
    }
}

fn put_optional_u64(out: &mut Vec<u8>, value: Option<u64>) {
    match value {
        Some(value) => {
            out.push(1);
            out.extend_from_slice(&value.to_le_bytes());
        }
        None => out.push(0),
    }
}

fn put_digest_set(out: &mut Vec<u8>, digests: &BTreeSet<Digest>) {
    out.extend_from_slice(&(digests.len() as u32).to_le_bytes());
    for digest in digests {
        out.extend_from_slice(digest.bytes());
    }
}

fn put_digest_list(out: &mut Vec<u8>, digests: &[Digest]) {
    out.extend_from_slice(&(digests.len() as u32).to_le_bytes());
    for digest in digests {
        out.extend_from_slice(digest.bytes());
    }
}

fn put_u64_list(out: &mut Vec<u8>, values: &[u64]) {
    out.extend_from_slice(&(values.len() as u32).to_le_bytes());
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

fn put_metadata_bootstrap_reserve(out: &mut Vec<u8>, reserve: &MetadataBootstrapReserve) {
    out.extend_from_slice(&reserve.owning_generation.to_le_bytes());
    out.extend_from_slice(&reserve.capacity.to_le_bytes());
    out.extend_from_slice(&(reserve.extents.len() as u32).to_le_bytes());
    for extent in &reserve.extents {
        out.extend_from_slice(&extent.start.to_le_bytes());
        out.extend_from_slice(&extent.len.to_le_bytes());
    }
}

pub(crate) fn encode_mark_reclaim_evidence(
    evidence: &ReachabilityMarkReclaimEvidence,
) -> Result<Vec<u8>> {
    evidence.require_current_metadata_bootstrap_evidence()?;
    let mut out = Vec::new();
    out.extend_from_slice(MARK_EPOCH_RECLAIM_EVIDENCE_MAGIC);
    out.extend_from_slice(&MARK_EPOCH_RECLAIM_EVIDENCE_VERSION.to_le_bytes());
    out.extend_from_slice(&evidence.epoch.to_le_bytes());
    out.extend_from_slice(&evidence.base_generation.to_le_bytes());
    out.extend_from_slice(evidence.reclaim_fence_identity.bytes());
    out.extend_from_slice(&evidence.page_high_water_mark.to_le_bytes());
    out.extend_from_slice(evidence.captured_root_identity.bytes());
    put_metadata_bootstrap_reserve(&mut out, &evidence.captured_metadata_bootstrap_reserve);
    put_optional_u64(&mut out, evidence.captured_free_root);
    put_optional_digest(&mut out, evidence.captured_free_identity);
    out.extend_from_slice(&evidence.captured_free_consumed_through.to_le_bytes());
    put_optional_u64(&mut out, evidence.metadata_evidence_root);
    out.extend_from_slice(&evidence.metadata_reclaim_candidate_count.to_le_bytes());
    out.extend_from_slice(evidence.metadata_evidence_identity.bytes());
    put_page_set(&mut out, &evidence.unreachable_pre_snapshot_pages);
    Ok(out)
}

fn decode_mark_reclaim_evidence(
    bytes: &[u8],
    algo: Algo,
) -> Result<ReachabilityMarkReclaimEvidence> {
    let mut cur = Cursor { bytes, pos: 0 };
    if cur.take(MARK_EPOCH_RECLAIM_EVIDENCE_MAGIC.len())? != MARK_EPOCH_RECLAIM_EVIDENCE_MAGIC {
        return Err(corrupt("reachability mark reclaim evidence magic"));
    }
    let version = cur.u16()?;
    if !(3..=MARK_EPOCH_RECLAIM_EVIDENCE_VERSION).contains(&version) {
        return Err(corrupt("reachability mark reclaim evidence version"));
    }
    let epoch = cur.u64()?;
    let base_generation = cur.u64()?;
    let reclaim_fence_identity = cur.digest(algo)?;
    let page_high_water_mark = cur.u64()?;
    let captured_root_identity = cur.digest(algo)?;
    let captured_metadata_bootstrap_reserve = if version >= 7 {
        cur.metadata_bootstrap_reserve(page_high_water_mark)?
    } else {
        MetadataBootstrapReserve::default()
    };
    let captured_free_root = if version >= 4 {
        cur.optional_u64()?
    } else {
        None
    };
    let captured_free_identity = cur.optional_digest(algo)?;
    let captured_free_consumed_through = cur.u64()?;
    let evidence = ReachabilityMarkReclaimEvidence {
        epoch,
        base_generation,
        reclaim_fence_identity,
        page_high_water_mark,
        captured_root_identity,
        captured_metadata_bootstrap_reserve,
        metadata_bootstrap_evidence_provenance: if version >= 7 {
            MetadataBootstrapEvidenceProvenance::Current
        } else {
            MetadataBootstrapEvidenceProvenance::Legacy
        },
        captured_free_root: (version >= 6).then_some(captured_free_root).flatten(),
        captured_free_identity: (version >= 6).then_some(captured_free_identity).flatten(),
        captured_free_consumed_through: if version >= 6 {
            captured_free_consumed_through
        } else {
            0
        },
        metadata_evidence_root: cur.optional_u64()?,
        metadata_reclaim_candidate_count: cur.u64()?,
        metadata_evidence_identity: cur.digest(algo)?,
        unreachable_pre_snapshot_pages: cur.page_set()?,
    };
    if cur.pos != bytes.len() {
        return Err(corrupt("reachability mark reclaim evidence trailing bytes"));
    }
    Ok(evidence)
}

fn put_page_set(out: &mut Vec<u8>, pages: &BTreeSet<u64>) {
    out.extend_from_slice(&(pages.len() as u64).to_le_bytes());
    for page in pages {
        out.extend_from_slice(&page.to_le_bytes());
    }
}

fn put_digest_queue(out: &mut Vec<u8>, digests: &VecDeque<Digest>) {
    out.extend_from_slice(&(digests.len() as u32).to_le_bytes());
    for digest in digests {
        out.extend_from_slice(digest.bytes());
    }
}

fn put_stream_root_queue(out: &mut Vec<u8>, roots: &VecDeque<ReachabilityStreamRoot>) {
    out.extend_from_slice(&(roots.len() as u32).to_le_bytes());
    for root in roots {
        out.extend_from_slice(root.root.bytes());
        out.extend_from_slice(&root.retained_low_water.to_le_bytes());
    }
}

fn put_prolly_cursor_queue(out: &mut Vec<u8>, queue: &VecDeque<ReachabilityProllyCursor>) {
    out.extend_from_slice(&(queue.len() as u32).to_le_bytes());
    for entry in queue {
        out.push(u8::from(entry.collect_stream_payloads));
        out.extend_from_slice(&entry.retained_low_water.to_le_bytes());
        out.extend_from_slice(&(entry.cursor.stack.len() as u32).to_le_bytes());
        for (digest, depth) in &entry.cursor.stack {
            out.extend_from_slice(digest.bytes());
            out.extend_from_slice(&(*depth as u32).to_le_bytes());
        }
    }
}

type DecodedMetadataEpoch = (
    bool,
    u64,
    u64,
    u64,
    u64,
    u64,
    Option<u64>,
    u64,
    u64,
    Digest,
    bool,
);

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(len)
            .ok_or_else(|| corrupt("reachability mark epoch offset overflow"))?;
        let out = self
            .bytes
            .get(self.pos..end)
            .ok_or_else(|| corrupt("reachability mark epoch truncated"))?;
        self.pos = end;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn optional_digest(&mut self, algo: Algo) -> Result<Option<Digest>> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.digest(algo)?)),
            _ => Err(corrupt("reachability mark epoch optional digest")),
        }
    }

    fn optional_u64(&mut self) -> Result<Option<u64>> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.u64()?)),
            _ => Err(corrupt("reachability mark epoch optional u64")),
        }
    }

    fn digest(&mut self, algo: Algo) -> Result<Digest> {
        Ok(Digest::of(algo, self.take(32)?.try_into().unwrap()))
    }

    fn digest_set(&mut self, algo: Algo) -> Result<BTreeSet<Digest>> {
        let len = self.digest_len()?;
        let mut out = BTreeSet::new();
        for _ in 0..len {
            if !out.insert(self.digest(algo)?) {
                return Err(corrupt("reachability mark epoch duplicate digest"));
            }
        }
        Ok(out)
    }

    fn digest_list(&mut self, algo: Algo) -> Result<Vec<Digest>> {
        let len = self.digest_len()?;
        let mut out = Vec::with_capacity(len);
        for _ in 0..len {
            out.push(self.digest(algo)?);
        }
        Ok(out)
    }

    fn u64_list(&mut self) -> Result<Vec<u64>> {
        let len = self.digest_len()?;
        let mut out = Vec::with_capacity(len);
        for _ in 0..len {
            out.push(self.u64()?);
        }
        Ok(out)
    }

    fn metadata_bootstrap_reserve(&mut self, page_count: u64) -> Result<MetadataBootstrapReserve> {
        let owning_generation = self.u64()?;
        let capacity = self.u64()?;
        let extent_count = self.u32()? as usize;
        if extent_count > crate::page::METADATA_BOOTSTRAP_MAX_EXTENTS {
            return Err(corrupt("reachability mark metadata bootstrap extent count"));
        }
        let mut extents = Vec::with_capacity(extent_count);
        for _ in 0..extent_count {
            extents.push(MetadataBootstrapExtent {
                start: self.u64()?,
                len: self.u64()?,
            });
        }
        let reserve = MetadataBootstrapReserve {
            owning_generation,
            capacity,
            extents,
        };
        if capacity == 0 && reserve.extents.is_empty() {
            return Ok(reserve);
        }
        reserve
            .validate(page_count)
            .map_err(|_| corrupt("reachability mark metadata bootstrap reserve"))?;
        Ok(reserve)
    }

    fn digest_queue(&mut self, algo: Algo) -> Result<VecDeque<Digest>> {
        let len = self.digest_len()?;
        let mut out = VecDeque::with_capacity(len);
        for _ in 0..len {
            out.push_back(self.digest(algo)?);
        }
        Ok(out)
    }

    fn stream_root_queue(&mut self, algo: Algo) -> Result<VecDeque<ReachabilityStreamRoot>> {
        let len = self.digest_len()?;
        let mut out = VecDeque::with_capacity(len);
        for _ in 0..len {
            out.push_back(ReachabilityStreamRoot {
                root: self.digest(algo)?,
                retained_low_water: self.u64()?,
            });
        }
        Ok(out)
    }

    fn prolly_cursor_queue(
        &mut self,
        algo: Algo,
        version: u16,
    ) -> Result<VecDeque<ReachabilityProllyCursor>> {
        let len = self.digest_len()?;
        let mut out = VecDeque::with_capacity(len);
        for _ in 0..len {
            let collect_stream_payloads = match self.u8()? {
                0 => false,
                1 => true,
                _ => return Err(corrupt("reachability mark epoch prolly cursor kind")),
            };
            let retained_low_water = if version >= 6 { self.u64()? } else { 0 };
            let stack_len = self.digest_len()?;
            let mut stack = Vec::with_capacity(stack_len);
            for _ in 0..stack_len {
                let digest = self.digest(algo)?;
                stack.push((digest, self.u32()? as usize));
            }
            out.push_back(ReachabilityProllyCursor {
                cursor: loom_core::prolly::ProllyReachCursor { stack },
                collect_stream_payloads,
                retained_low_water,
            });
        }
        Ok(out)
    }

    fn page_set(&mut self) -> Result<BTreeSet<u64>> {
        let len = self.u64()?;
        let len = usize::try_from(len)
            .map_err(|_| corrupt("reachability mark reclaim evidence page count"))?;
        if len > MAX_PAGE_LIST {
            return Err(corrupt("reachability mark reclaim evidence page count"));
        }
        let mut out = BTreeSet::new();
        let mut previous = None;
        for _ in 0..len {
            let page = self.u64()?;
            if previous.is_some_and(|last| page <= last) {
                return Err(corrupt("reachability mark reclaim evidence page order"));
            }
            if !out.insert(page) {
                return Err(corrupt("reachability mark reclaim evidence duplicate page"));
            }
            previous = Some(page);
        }
        Ok(out)
    }

    fn digest_len(&mut self) -> Result<usize> {
        let len = self.u32()? as usize;
        if len > MAX_DIGEST_LIST {
            return Err(corrupt("reachability mark epoch digest count"));
        }
        Ok(len)
    }
}

fn decode_version8_scalar_metadata_epoch(
    cur: &mut Cursor<'_>,
    algo: Algo,
) -> Result<DecodedMetadataEpoch> {
    let _old_metadata_work_initialized = match cur.u8()? {
        0 => false,
        1 => true,
        _ => {
            return Err(corrupt("reachability mark epoch metadata initialized flag"));
        }
    };
    let _old_metadata_root_cursor = cur.u64()?;
    let _old_metadata_value_root_cursor = cur.u64()?;
    let _old_metadata_classify_next_page = cur.u64()?;
    let _old_metadata_evidence_root = cur.optional_u64()?;
    let _old_metadata_reachable_count = cur.u64()?;
    let _old_metadata_reclaim_candidate_count = cur.u64()?;
    let _old_metadata_evidence_identity = cur.digest(algo)?;
    let _old_metadata_completed = match cur.u8()? {
        0 => false,
        1 => true,
        _ => return Err(corrupt("reachability mark epoch metadata completed flag")),
    };
    Ok(restarted_metadata_epoch(algo))
}

fn decode_version8_queue_metadata_epoch(
    cur: &mut Cursor<'_>,
    algo: Algo,
) -> Result<DecodedMetadataEpoch> {
    let _metadata_root_queue = cur.u64_list()?;
    let _metadata_value_root_queue = cur.u64_list()?;
    let _metadata_value_tree_pages = cur.page_set()?;
    let _metadata_classify_next_page = cur.u64()?;
    let _metadata_evidence_root = cur.optional_u64()?;
    let _metadata_reachable_count = cur.u64()?;
    let _metadata_reclaim_candidate_count = cur.u64()?;
    let _metadata_evidence_identity = cur.digest(algo)?;
    let _metadata_completed = match cur.u8()? {
        0 => false,
        1 => true,
        _ => return Err(corrupt("reachability mark epoch metadata completed flag")),
    };
    Ok(restarted_metadata_epoch(algo))
}

fn restarted_metadata_epoch(algo: Algo) -> DecodedMetadataEpoch {
    (
        false,
        0,
        0,
        0,
        0,
        0,
        None,
        0,
        0,
        mark_epoch_metadata_evidence_identity(algo, None, 0, 0),
        false,
    )
}

fn mark_epoch_captured_root_vector(
    reference_root: Option<Digest>,
    control_root: Option<Digest>,
    derived_roots: &BTreeSet<Digest>,
) -> Vec<Digest> {
    reference_root
        .into_iter()
        .chain(control_root)
        .chain(derived_roots.iter().copied())
        .collect()
}

fn mark_epoch_captured_metadata_roots(
    inner: &crate::Inner,
    canonical_roots: &[crate::compact::GcCanonicalRootEvidence],
) -> Result<(Vec<u64>, Vec<u64>)> {
    let mut roots = BTreeSet::new();
    let mut value_roots = BTreeSet::new();
    if let Some(root) = inner.region_table_root {
        roots.insert(root.0);
    }
    for evidence in canonical_roots {
        let codec = match evidence.family_id {
            Some(family_id) => Some(crate::root_family_value_codec(family_id)?),
            None if evidence.name == "object_index_records" => {
                Some(crate::pagebtree::ValueCodecKind::RecordLoc)
            }
            None => None,
        };
        let Some(root) = evidence.page_root else {
            continue;
        };
        roots.insert(root.0);
        if matches!(
            codec,
            Some(
                crate::pagebtree::ValueCodecKind::RecordLoc
                    | crate::pagebtree::ValueCodecKind::PackedRecordRef
            )
        ) {
            value_roots.insert(root.0);
        }
    }
    Ok((
        roots.into_iter().collect(),
        value_roots.into_iter().collect(),
    ))
}

fn captured_packed_locator_tree_pages(
    file: &mut dyn crate::BackingIo,
    epoch: &ReachabilityMarkEpoch,
) -> Result<BTreeSet<u64>> {
    let captured_roots = epoch
        .captured_metadata_roots
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut root_catalog_roots = BTreeSet::new();
    for root in &captured_roots {
        let mut page = [0u8; crate::page::PAGE_SIZE as usize];
        crate::read_exact_at(file, PageId(*root).offset(crate::DATA_START), &mut page)
            .map_err(crate::io_err)?;
        if let Some(region) = RegionTable::decode(&page)
            && let Some(root_catalog_root) = region.root_catalog_root
        {
            root_catalog_roots.insert(root_catalog_root);
        }
    }
    let mut packed_pages = BTreeSet::new();
    for root_catalog_root in root_catalog_roots {
        if !captured_roots.contains(&root_catalog_root.0) {
            return Err(corrupt(
                "reachability mark captured root catalog is absent from metadata roots",
            ));
        }
        let mut page = [0u8; crate::page::PAGE_SIZE as usize];
        crate::read_exact_at(file, root_catalog_root.offset(crate::DATA_START), &mut page)
            .map_err(crate::io_err)?;
        let catalog = RootCatalog::decode(&page)
            .map_err(|_| corrupt("reachability mark captured root catalog is invalid"))?;
        for entry in catalog.entries {
            let descriptor = crate::page::root_family_descriptor(entry.family_id)
                .ok_or_else(|| corrupt("reachability mark captured unknown root family"))?;
            if descriptor.value_codec != crate::pagebtree::ValueCodecKind::PackedRecordRef {
                continue;
            }
            if !captured_roots.contains(&entry.root.0) {
                return Err(corrupt(
                    "reachability mark captured family root is absent from metadata roots",
                ));
            }
            packed_pages.extend(
                crate::pagebtree::collect_pages_with_codec(
                    file,
                    crate::DATA_START,
                    entry.root,
                    epoch.page_high_water_mark,
                    descriptor.value_codec,
                )?
                .into_iter()
                .map(|page| page.0),
            );
        }
    }
    Ok(packed_pages)
}

fn page_contains_reclaimable_metadata(
    file: &mut dyn crate::BackingIo,
    page: u64,
    _page_count: u64,
) -> Result<bool> {
    #[cfg(test)]
    {
        METADATA_PAGE_CLASSIFICATIONS_FOR_TEST
            .set(METADATA_PAGE_CLASSIFICATIONS_FOR_TEST.get() + 1);
        METADATA_CLASSIFIED_PAGES_FOR_TEST.with(|pages| {
            pages.borrow_mut().insert(page);
        });
    }
    let mut buf = [0u8; crate::page::PAGE_SIZE as usize];
    crate::read_exact_at(file, PageId(page).offset(crate::DATA_START), &mut buf)
        .map_err(crate::io_err)?;
    if crate::pagebtree::looks_like_node_page(&buf)
        || RegionTable::decode(&buf).is_some()
        || crate::maintenance::looks_like_maintenance_page(&buf)
    {
        return Ok(true);
    }
    Ok(false)
}

fn metadata_evidence_chunk_start(page: u64) -> u64 {
    page - (page % MARK_EPOCH_CHUNK_PAGES)
}

fn metadata_evidence_chunk_key(
    algo: Algo,
    epoch: u64,
    reclaim_fence_identity: Digest,
    page_start: u64,
) -> [u8; 32] {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"loom.store.mark-epoch.chunk.v1");
    bytes.extend_from_slice(&epoch.to_be_bytes());
    bytes.extend_from_slice(reclaim_fence_identity.bytes());
    bytes.extend_from_slice(&page_start.to_be_bytes());
    *Digest::hash(algo, &bytes).bytes()
}

fn mark_epoch_metadata_evidence_identity(
    algo: Algo,
    root: Option<u64>,
    reachable_count: u64,
    reclaim_candidate_count: u64,
) -> Digest {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"loom.store.mark-epoch.metadata-evidence.v1");
    match root {
        Some(root) => {
            bytes.push(1);
            bytes.extend_from_slice(&root.to_le_bytes());
        }
        None => bytes.push(0),
    }
    bytes.extend_from_slice(&reachable_count.to_le_bytes());
    bytes.extend_from_slice(&reclaim_candidate_count.to_le_bytes());
    Digest::hash(algo, &bytes)
}

fn metadata_bit(bitmap: &[u8; MARK_EPOCH_CHUNK_BITMAP_BYTES], page_start: u64, page: u64) -> bool {
    let offset = (page - page_start) as usize;
    bitmap[offset / 8] & (1 << (offset % 8)) != 0
}

fn set_metadata_bit(
    bitmap: &mut [u8; MARK_EPOCH_CHUNK_BITMAP_BYTES],
    page_start: u64,
    page: u64,
) -> bool {
    let offset = (page - page_start) as usize;
    let mask = 1 << (offset % 8);
    let byte = &mut bitmap[offset / 8];
    let changed = *byte & mask == 0;
    *byte |= mask;
    changed
}

fn clear_metadata_bit(
    bitmap: &mut [u8; MARK_EPOCH_CHUNK_BITMAP_BYTES],
    page_start: u64,
    page: u64,
) -> bool {
    let offset = (page - page_start) as usize;
    let mask = 1 << (offset % 8);
    let byte = &mut bitmap[offset / 8];
    let changed = *byte & mask != 0;
    *byte &= !mask;
    changed
}

#[derive(Clone, Copy)]
enum MetadataPendingKind {
    Root,
    ValueRoot,
    ValueBlob,
}

impl MetadataEvidenceChunk {
    fn empty(epoch: u64, page_start: u64) -> Self {
        Self {
            epoch,
            page_start,
            pending_roots: [0; MARK_EPOCH_CHUNK_BITMAP_BYTES],
            pending_value_roots: [0; MARK_EPOCH_CHUNK_BITMAP_BYTES],
            pending_value_blobs: [0; MARK_EPOCH_CHUNK_BITMAP_BYTES],
            large_value_start: None,
            large_value_next: 0,
            large_value_end: 0,
            expansion_node: None,
            expansion_child_offset: 0,
            expansion_value_offset: 0,
            expansion_value_tree: false,
            expansion_free_page_extent_tree: false,
            free_page_extent_tree: [0; MARK_EPOCH_CHUNK_BITMAP_BYTES],
            value_tree: [0; MARK_EPOCH_CHUNK_BITMAP_BYTES],
            reachable: [0; MARK_EPOCH_CHUNK_BITMAP_BYTES],
            reclaim_candidate: [0; MARK_EPOCH_CHUNK_BITMAP_BYTES],
        }
    }

    fn contains_reachable(&self, page: u64) -> Result<bool> {
        if page < self.page_start || page >= self.page_start + MARK_EPOCH_CHUNK_PAGES {
            return Err(corrupt(
                "reachability mark metadata chunk page out of range",
            ));
        }
        Ok(metadata_bit(&self.reachable, self.page_start, page))
    }

    fn contains_value_tree(&self, page: u64) -> Result<bool> {
        if page < self.page_start || page >= self.page_start + MARK_EPOCH_CHUNK_PAGES {
            return Err(corrupt(
                "reachability mark metadata chunk page out of range",
            ));
        }
        Ok(metadata_bit(&self.value_tree, self.page_start, page))
    }

    fn contains_free_page_extent_tree(&self, page: u64) -> Result<bool> {
        if page < self.page_start || page >= self.page_start + MARK_EPOCH_CHUNK_PAGES {
            return Err(corrupt(
                "reachability mark metadata chunk page out of range",
            ));
        }
        Ok(metadata_bit(
            &self.free_page_extent_tree,
            self.page_start,
            page,
        ))
    }

    fn set_pending_root(&mut self, page: u64) -> Result<bool> {
        if page < self.page_start || page >= self.page_start + MARK_EPOCH_CHUNK_PAGES {
            return Err(corrupt(
                "reachability mark metadata chunk page out of range",
            ));
        }
        Ok(set_metadata_bit(
            &mut self.pending_roots,
            self.page_start,
            page,
        ))
    }

    fn set_pending_value_root(&mut self, page: u64) -> Result<bool> {
        if page < self.page_start || page >= self.page_start + MARK_EPOCH_CHUNK_PAGES {
            return Err(corrupt(
                "reachability mark metadata chunk page out of range",
            ));
        }
        Ok(set_metadata_bit(
            &mut self.pending_value_roots,
            self.page_start,
            page,
        ))
    }

    fn set_value_tree(&mut self, page: u64) -> Result<bool> {
        if page < self.page_start || page >= self.page_start + MARK_EPOCH_CHUNK_PAGES {
            return Err(corrupt(
                "reachability mark metadata chunk page out of range",
            ));
        }
        Ok(set_metadata_bit(
            &mut self.value_tree,
            self.page_start,
            page,
        ))
    }

    fn set_free_page_extent_tree(&mut self, page: u64) -> Result<bool> {
        if page < self.page_start || page >= self.page_start + MARK_EPOCH_CHUNK_PAGES {
            return Err(corrupt(
                "reachability mark metadata chunk page out of range",
            ));
        }
        Ok(set_metadata_bit(
            &mut self.free_page_extent_tree,
            self.page_start,
            page,
        ))
    }

    fn set_pending_value_blob(&mut self, page: u64) -> Result<bool> {
        if page < self.page_start || page >= self.page_start + MARK_EPOCH_CHUNK_PAGES {
            return Err(corrupt(
                "reachability mark metadata chunk page out of range",
            ));
        }
        Ok(set_metadata_bit(
            &mut self.pending_value_blobs,
            self.page_start,
            page,
        ))
    }

    fn set_reachable(&mut self, page: u64) -> Result<bool> {
        if page < self.page_start || page >= self.page_start + MARK_EPOCH_CHUNK_PAGES {
            return Err(corrupt(
                "reachability mark metadata chunk page out of range",
            ));
        }
        Ok(set_metadata_bit(&mut self.reachable, self.page_start, page))
    }

    fn protect_current_epoch_page(&mut self, page: u64) -> Result<(bool, bool)> {
        if page < self.page_start || page >= self.page_start + MARK_EPOCH_CHUNK_PAGES {
            return Err(corrupt(
                "reachability mark metadata chunk page out of range",
            ));
        }
        let was_candidate = metadata_bit(&self.reclaim_candidate, self.page_start, page);
        if was_candidate {
            clear_metadata_bit(&mut self.reclaim_candidate, self.page_start, page);
        }
        Ok((self.set_reachable(page)?, was_candidate))
    }

    fn set_reclaim_candidate(&mut self, page: u64) -> Result<bool> {
        if page < self.page_start || page >= self.page_start + MARK_EPOCH_CHUNK_PAGES {
            return Err(corrupt(
                "reachability mark metadata chunk page out of range",
            ));
        }
        Ok(set_metadata_bit(
            &mut self.reclaim_candidate,
            self.page_start,
            page,
        ))
    }

    fn take_next_pending(
        &mut self,
        kind: MetadataPendingKind,
        from_page: u64,
    ) -> Result<Option<u64>> {
        let start = from_page.max(self.page_start);
        if start >= self.page_start + MARK_EPOCH_CHUNK_PAGES {
            return Ok(None);
        }
        let bitmap = match kind {
            MetadataPendingKind::Root => &mut self.pending_roots,
            MetadataPendingKind::ValueRoot => &mut self.pending_value_roots,
            MetadataPendingKind::ValueBlob => &mut self.pending_value_blobs,
        };
        for page in start..self.page_start + MARK_EPOCH_CHUNK_PAGES {
            if metadata_bit(bitmap, self.page_start, page) {
                clear_metadata_bit(bitmap, self.page_start, page);
                return Ok(Some(page));
            }
        }
        Ok(None)
    }

    fn has_large_value_continuation(&self) -> bool {
        self.large_value_start.is_some() && self.large_value_next < self.large_value_end
    }

    fn take_large_value_continuation(&mut self) -> Option<u64> {
        if !self.has_large_value_continuation() {
            self.large_value_start = None;
            self.large_value_next = 0;
            self.large_value_end = 0;
            return None;
        }
        let page = self.large_value_next;
        self.large_value_next = self.large_value_next.saturating_add(1);
        if self.large_value_next >= self.large_value_end {
            self.large_value_start = None;
            self.large_value_next = 0;
            self.large_value_end = 0;
        }
        Some(page)
    }

    fn start_large_value_continuation(&mut self, start: u64, end: u64) {
        if start.saturating_add(1) < end {
            self.large_value_start = Some(start);
            self.large_value_next = start.saturating_add(1);
            self.large_value_end = end;
        }
    }

    fn start_root_expansion(
        &mut self,
        node: u64,
        value_tree: bool,
        free_page_extent_tree: bool,
        child_count: usize,
        value_count: usize,
    ) -> Result<()> {
        if child_count == 0 && (!value_tree || value_count == 0) {
            self.expansion_node = None;
            self.expansion_child_offset = 0;
            self.expansion_value_offset = 0;
            self.expansion_value_tree = false;
            self.expansion_free_page_extent_tree = false;
            return Ok(());
        }
        let child_len = u16::try_from(child_count)
            .map_err(|_| corrupt("reachability mark metadata child count overflow"))?;
        let value_len = u16::try_from(value_count)
            .map_err(|_| corrupt("reachability mark metadata value count overflow"))?;
        self.expansion_node = Some(node);
        self.expansion_child_offset = child_len.min(0);
        self.expansion_value_offset = value_len.min(0);
        self.expansion_value_tree = value_tree;
        self.expansion_free_page_extent_tree = free_page_extent_tree;
        Ok(())
    }

    fn clear_root_expansion(&mut self) {
        self.expansion_node = None;
        self.expansion_child_offset = 0;
        self.expansion_value_offset = 0;
        self.expansion_value_tree = false;
        self.expansion_free_page_extent_tree = false;
    }

    fn reclaim_candidate_pages(&self, limit: usize) -> Vec<u64> {
        let mut pages = Vec::new();
        for offset in 0..MARK_EPOCH_CHUNK_PAGES {
            if pages.len() >= limit {
                break;
            }
            let page = self.page_start + offset;
            if metadata_bit(&self.reclaim_candidate, self.page_start, page) {
                pages.push(page);
            }
        }
        pages
    }
}

fn encode_metadata_evidence_chunk(chunk: &MetadataEvidenceChunk) -> Vec<u8> {
    let mut out = Vec::with_capacity(
        8 + 2 + 8 + 8 + 1 + 8 + 8 + 8 + 1 + 8 + 2 + 2 + 2 + MARK_EPOCH_CHUNK_BITMAP_BYTES * 7,
    );
    out.extend_from_slice(MARK_EPOCH_CHUNK_MAGIC);
    out.extend_from_slice(&MARK_EPOCH_CHUNK_VERSION.to_le_bytes());
    out.extend_from_slice(&chunk.epoch.to_le_bytes());
    out.extend_from_slice(&chunk.page_start.to_le_bytes());
    out.extend_from_slice(&chunk.pending_roots);
    out.extend_from_slice(&chunk.pending_value_roots);
    out.extend_from_slice(&chunk.pending_value_blobs);
    match chunk.large_value_start {
        Some(start) => {
            out.push(1);
            out.extend_from_slice(&start.to_le_bytes());
            out.extend_from_slice(&chunk.large_value_next.to_le_bytes());
            out.extend_from_slice(&chunk.large_value_end.to_le_bytes());
        }
        None => {
            out.push(0);
            out.extend_from_slice(&0u64.to_le_bytes());
            out.extend_from_slice(&0u64.to_le_bytes());
            out.extend_from_slice(&0u64.to_le_bytes());
        }
    }
    match chunk.expansion_node {
        Some(node) => {
            out.push(1);
            out.extend_from_slice(&node.to_le_bytes());
            out.extend_from_slice(&chunk.expansion_child_offset.to_le_bytes());
            out.extend_from_slice(&chunk.expansion_value_offset.to_le_bytes());
            out.push(u8::from(chunk.expansion_value_tree));
            out.push(u8::from(chunk.expansion_free_page_extent_tree));
        }
        None => {
            out.push(0);
            out.extend_from_slice(&0u64.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.push(0);
            out.push(0);
        }
    }
    out.extend_from_slice(&chunk.free_page_extent_tree);
    out.extend_from_slice(&chunk.value_tree);
    out.extend_from_slice(&chunk.reachable);
    out.extend_from_slice(&chunk.reclaim_candidate);
    out
}

fn decode_metadata_evidence_chunk(bytes: &[u8]) -> Result<MetadataEvidenceChunk> {
    if bytes.len() < 10 {
        return Err(corrupt("reachability mark metadata chunk length"));
    }
    if &bytes[..8] != MARK_EPOCH_CHUNK_MAGIC {
        return Err(corrupt("reachability mark metadata chunk magic"));
    }
    let version = u16::from_le_bytes(bytes[8..10].try_into().unwrap());
    if !(1..=MARK_EPOCH_CHUNK_VERSION).contains(&version) {
        return Err(corrupt("reachability mark metadata chunk version"));
    }
    let original_v1_expected = 8 + 2 + 8 + 8 + MARK_EPOCH_CHUNK_BITMAP_BYTES * 2;
    let later_v1_expected = 8 + 2 + 8 + 8 + MARK_EPOCH_CHUNK_BITMAP_BYTES * 5;
    let v2_expected = 8 + 2 + 8 + 8 + 1 + 8 + 8 + 8 + MARK_EPOCH_CHUNK_BITMAP_BYTES * 6;
    let v3_expected =
        8 + 2 + 8 + 8 + 1 + 8 + 8 + 8 + 1 + 8 + 2 + 2 + 1 + MARK_EPOCH_CHUNK_BITMAP_BYTES * 6;
    let v4_expected =
        8 + 2 + 8 + 8 + 1 + 8 + 8 + 8 + 1 + 8 + 2 + 2 + 2 + MARK_EPOCH_CHUNK_BITMAP_BYTES * 7;
    let expected = match version {
        1 if bytes.len() == original_v1_expected => original_v1_expected,
        1 if bytes.len() == later_v1_expected => later_v1_expected,
        2 => v2_expected,
        3 => v3_expected,
        4 => v4_expected,
        _ => return Err(corrupt("reachability mark metadata chunk length")),
    };
    if bytes.len() != expected {
        return Err(corrupt("reachability mark metadata chunk length"));
    }
    let epoch = u64::from_le_bytes(bytes[10..18].try_into().unwrap());
    let page_start = u64::from_le_bytes(bytes[18..26].try_into().unwrap());
    if !page_start.is_multiple_of(MARK_EPOCH_CHUNK_PAGES) {
        return Err(corrupt("reachability mark metadata chunk start"));
    }
    let mut offset = 26;
    let mut pending_roots = [0; MARK_EPOCH_CHUNK_BITMAP_BYTES];
    let mut pending_value_roots = [0; MARK_EPOCH_CHUNK_BITMAP_BYTES];
    let mut pending_value_blobs = [0; MARK_EPOCH_CHUNK_BITMAP_BYTES];
    let mut free_page_extent_tree = [0; MARK_EPOCH_CHUNK_BITMAP_BYTES];
    let mut value_tree = [0; MARK_EPOCH_CHUNK_BITMAP_BYTES];
    let mut reachable = [0; MARK_EPOCH_CHUNK_BITMAP_BYTES];
    let mut reclaim_candidate = [0; MARK_EPOCH_CHUNK_BITMAP_BYTES];
    let large_value_start;
    let large_value_next;
    let large_value_end;
    let expansion_node;
    let expansion_child_offset;
    let expansion_value_offset;
    let expansion_value_tree;
    let expansion_free_page_extent_tree;
    if version == 1 && bytes.len() == original_v1_expected {
        reachable.copy_from_slice(&bytes[offset..offset + MARK_EPOCH_CHUNK_BITMAP_BYTES]);
        offset += MARK_EPOCH_CHUNK_BITMAP_BYTES;
        reclaim_candidate.copy_from_slice(&bytes[offset..offset + MARK_EPOCH_CHUNK_BITMAP_BYTES]);
        large_value_start = None;
        large_value_next = 0;
        large_value_end = 0;
        expansion_node = None;
        expansion_child_offset = 0;
        expansion_value_offset = 0;
        expansion_value_tree = false;
        expansion_free_page_extent_tree = false;
        return Ok(MetadataEvidenceChunk {
            epoch,
            page_start,
            pending_roots,
            pending_value_roots,
            pending_value_blobs,
            large_value_start,
            large_value_next,
            large_value_end,
            expansion_node,
            expansion_child_offset,
            expansion_value_offset,
            expansion_value_tree,
            expansion_free_page_extent_tree,
            free_page_extent_tree,
            value_tree,
            reachable,
            reclaim_candidate,
        });
    }
    pending_roots.copy_from_slice(&bytes[offset..offset + MARK_EPOCH_CHUNK_BITMAP_BYTES]);
    offset += MARK_EPOCH_CHUNK_BITMAP_BYTES;
    pending_value_roots.copy_from_slice(&bytes[offset..offset + MARK_EPOCH_CHUNK_BITMAP_BYTES]);
    offset += MARK_EPOCH_CHUNK_BITMAP_BYTES;
    if version >= 2 {
        pending_value_blobs.copy_from_slice(&bytes[offset..offset + MARK_EPOCH_CHUNK_BITMAP_BYTES]);
        offset += MARK_EPOCH_CHUNK_BITMAP_BYTES;
        let active = bytes[offset];
        offset += 1;
        let start = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
        offset += 8;
        let next = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
        offset += 8;
        let end = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
        offset += 8;
        match active {
            0 => {
                large_value_start = None;
                large_value_next = 0;
                large_value_end = 0;
            }
            1 if start < next && next <= end => {
                large_value_start = Some(start);
                large_value_next = next;
                large_value_end = end;
            }
            _ => {
                return Err(corrupt(
                    "reachability mark metadata chunk value continuation",
                ));
            }
        }
    } else {
        large_value_start = None;
        large_value_next = 0;
        large_value_end = 0;
    }
    if version >= 3 {
        let active = bytes[offset];
        offset += 1;
        let node = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
        offset += 8;
        let child_offset = u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap());
        offset += 2;
        let value_offset = u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap());
        offset += 2;
        let value_tree_flag = bytes[offset];
        offset += 1;
        let free_page_extent_tree_flag = if version >= 4 {
            let flag = bytes[offset];
            offset += 1;
            flag
        } else {
            0
        };
        match (active, value_tree_flag, free_page_extent_tree_flag) {
            (0, 0, 0) => {
                expansion_node = None;
                expansion_child_offset = 0;
                expansion_value_offset = 0;
                expansion_value_tree = false;
                expansion_free_page_extent_tree = false;
            }
            (1, 0 | 1, 0 | 1) if value_tree_flag == 0 || free_page_extent_tree_flag == 0 => {
                expansion_node = Some(node);
                expansion_child_offset = child_offset;
                expansion_value_offset = value_offset;
                expansion_value_tree = value_tree_flag == 1;
                expansion_free_page_extent_tree = free_page_extent_tree_flag == 1;
            }
            _ => return Err(corrupt("reachability mark metadata chunk root expansion")),
        }
    } else {
        expansion_node = None;
        expansion_child_offset = 0;
        expansion_value_offset = 0;
        expansion_value_tree = false;
        expansion_free_page_extent_tree = false;
    }
    if version >= 4 {
        free_page_extent_tree
            .copy_from_slice(&bytes[offset..offset + MARK_EPOCH_CHUNK_BITMAP_BYTES]);
        offset += MARK_EPOCH_CHUNK_BITMAP_BYTES;
    }
    value_tree.copy_from_slice(&bytes[offset..offset + MARK_EPOCH_CHUNK_BITMAP_BYTES]);
    offset += MARK_EPOCH_CHUNK_BITMAP_BYTES;
    reachable.copy_from_slice(&bytes[offset..offset + MARK_EPOCH_CHUNK_BITMAP_BYTES]);
    offset += MARK_EPOCH_CHUNK_BITMAP_BYTES;
    reclaim_candidate.copy_from_slice(&bytes[offset..offset + MARK_EPOCH_CHUNK_BITMAP_BYTES]);
    Ok(MetadataEvidenceChunk {
        epoch,
        page_start,
        pending_roots,
        pending_value_roots,
        pending_value_blobs,
        large_value_start,
        large_value_next,
        large_value_end,
        expansion_node,
        expansion_child_offset,
        expansion_value_offset,
        expansion_value_tree,
        expansion_free_page_extent_tree,
        free_page_extent_tree,
        value_tree,
        reachable,
        reclaim_candidate,
    })
}

fn read_metadata_evidence_chunk(
    file: &mut dyn crate::BackingIo,
    root: Option<u64>,
    algo: Algo,
    epoch: u64,
    reclaim_fence_identity: Digest,
    page_start: u64,
    page_count: u64,
) -> Result<Option<MetadataEvidenceChunk>> {
    let Some(root) = root else {
        return Ok(None);
    };
    let key = metadata_evidence_chunk_key(algo, epoch, reclaim_fence_identity, page_start);
    let Some(loc) = crate::pagebtree::get_with_codec(
        file,
        crate::DATA_START,
        Some(PageId(root)),
        &key,
        page_count,
        crate::root_family_value_codec(RECLAIM_INDEX_FAMILY_ID)?,
    )?
    else {
        return Ok(None);
    };
    let bytes = crate::record_io::read_blob_from_loc(file, loc, page_count)?;
    let chunk = decode_metadata_evidence_chunk(&bytes)?;
    if chunk.epoch != epoch || chunk.page_start != page_start {
        return Err(corrupt("reachability mark metadata chunk identity"));
    }
    Ok(Some(chunk))
}

fn metadata_evidence_chunk_for_page<'a>(
    file: &mut dyn crate::BackingIo,
    root: Option<u64>,
    algo: Algo,
    epoch: u64,
    reclaim_fence_identity: Digest,
    page: u64,
    page_count: u64,
    touched_chunks: &'a mut BTreeMap<u64, MetadataEvidenceChunk>,
) -> Result<&'a mut MetadataEvidenceChunk> {
    let page_start = metadata_evidence_chunk_start(page);
    if !touched_chunks.contains_key(&page_start) {
        let chunk = read_metadata_evidence_chunk(
            file,
            root,
            algo,
            epoch,
            reclaim_fence_identity,
            page_start,
            page_count,
        )?
        .unwrap_or_else(|| MetadataEvidenceChunk::empty(epoch, page_start));
        touched_chunks.insert(page_start, chunk);
    }
    Ok(touched_chunks
        .get_mut(&page_start)
        .expect("metadata evidence chunk inserted"))
}

fn metadata_take_next_pending_page(
    file: &mut dyn crate::BackingIo,
    root: Option<u64>,
    algo: Algo,
    epoch: u64,
    reclaim_fence_identity: Digest,
    cursor: &mut u64,
    high_water_mark: u64,
    page_count: u64,
    touched_chunks: &mut BTreeMap<u64, MetadataEvidenceChunk>,
    kind: MetadataPendingKind,
) -> Result<Option<u64>> {
    if *cursor >= high_water_mark {
        return Ok(None);
    }
    let page_start = metadata_evidence_chunk_start(*cursor);
    let chunk_end = page_start
        .saturating_add(MARK_EPOCH_CHUNK_PAGES)
        .min(high_water_mark);
    if !touched_chunks.contains_key(&page_start) {
        let Some(chunk) = read_metadata_evidence_chunk(
            file,
            root,
            algo,
            epoch,
            reclaim_fence_identity,
            page_start,
            page_count,
        )?
        else {
            *cursor = chunk_end;
            return Ok(None);
        };
        touched_chunks.insert(page_start, chunk);
    }
    let chunk = touched_chunks
        .get_mut(&page_start)
        .expect("metadata evidence chunk inserted");
    if let Some(page) = chunk.take_next_pending(kind, *cursor)? {
        *cursor = page.saturating_add(1);
        return Ok(Some(page));
    }
    *cursor = chunk_end;
    Ok(None)
}

fn metadata_take_next_large_value_page(
    file: &mut dyn crate::BackingIo,
    root: Option<u64>,
    algo: Algo,
    epoch: u64,
    reclaim_fence_identity: Digest,
    cursor: &mut u64,
    high_water_mark: u64,
    page_count: u64,
    touched_chunks: &mut BTreeMap<u64, MetadataEvidenceChunk>,
) -> Result<Option<u64>> {
    if *cursor >= high_water_mark {
        return Ok(None);
    }
    let page_start = metadata_evidence_chunk_start(*cursor);
    let chunk_end = page_start
        .saturating_add(MARK_EPOCH_CHUNK_PAGES)
        .min(high_water_mark);
    if !touched_chunks.contains_key(&page_start) {
        let Some(chunk) = read_metadata_evidence_chunk(
            file,
            root,
            algo,
            epoch,
            reclaim_fence_identity,
            page_start,
            page_count,
        )?
        else {
            return Ok(None);
        };
        touched_chunks.insert(page_start, chunk);
    }
    let chunk = touched_chunks
        .get_mut(&page_start)
        .expect("metadata evidence chunk inserted");
    let Some(page) = chunk.take_large_value_continuation() else {
        return Ok(None);
    };
    *cursor = if chunk.has_large_value_continuation() {
        page.saturating_add(1)
    } else {
        chunk_end
    };
    Ok(Some(page))
}

fn metadata_process_root_expansion_continuation(
    file: &mut dyn crate::BackingIo,
    algo: Algo,
    epoch: &mut ReachabilityMarkEpoch,
    evidence_page_count: u64,
    touched_chunks: &mut BTreeMap<u64, MetadataEvidenceChunk>,
    packed_locator_tree_pages: &BTreeSet<u64>,
) -> Result<bool> {
    if epoch.metadata_expansion_cursor >= epoch.page_high_water_mark {
        return Ok(false);
    }
    let page = epoch.metadata_expansion_cursor;
    let page_start = metadata_evidence_chunk_start(page);
    if !touched_chunks.contains_key(&page_start) {
        let Some(chunk) = read_metadata_evidence_chunk(
            file,
            epoch.metadata_evidence_root,
            algo,
            epoch.epoch,
            epoch.reclaim_fence_identity,
            page_start,
            evidence_page_count,
        )?
        else {
            epoch.metadata_expansion_cursor = page_start
                .saturating_add(MARK_EPOCH_CHUNK_PAGES)
                .min(epoch.page_high_water_mark);
            return Ok(true);
        };
        touched_chunks.insert(page_start, chunk);
    }
    let chunk = touched_chunks
        .get_mut(&page_start)
        .expect("metadata evidence chunk inserted");
    let Some(node_page) = chunk.expansion_node else {
        epoch.metadata_expansion_cursor = metadata_evidence_chunk_start(page)
            .saturating_add(MARK_EPOCH_CHUNK_PAGES)
            .min(epoch.page_high_water_mark);
        return Ok(false);
    };
    let child_offset = usize::from(chunk.expansion_child_offset);
    let value_offset = usize::from(chunk.expansion_value_offset);
    let value_tree = chunk.expansion_value_tree;
    let free_page_extent_tree = chunk.expansion_free_page_extent_tree;
    let (children, values) = if free_page_extent_tree {
        let Some(links) = crate::pagebtree::free_page_extent_node_links(
            file,
            crate::DATA_START,
            PageId(node_page),
            epoch.page_high_water_mark,
        )?
        else {
            chunk.clear_root_expansion();
            epoch.metadata_expansion_cursor = epoch.page_high_water_mark;
            return Ok(true);
        };
        (links.children, Vec::new())
    } else {
        let codec = if packed_locator_tree_pages.contains(&node_page) {
            crate::pagebtree::ValueCodecKind::PackedRecordRef
        } else {
            crate::pagebtree::ValueCodecKind::RecordLoc
        };
        let Some(links) = crate::pagebtree::node_page_links_with_codec(
            file,
            crate::DATA_START,
            PageId(node_page),
            epoch.page_high_water_mark,
            codec,
        )?
        else {
            chunk.clear_root_expansion();
            epoch.metadata_expansion_cursor = epoch.page_high_water_mark;
            return Ok(true);
        };
        (links.children, links.values)
    };
    if child_offset < children.len() {
        let child = children[child_offset];
        if child.0 >= epoch.page_high_water_mark {
            return Err(corrupt(
                "reachability mark metadata child root out of range",
            ));
        }
        let child_chunk_start = metadata_evidence_chunk_start(child.0);
        let child_chunk_exists = touched_chunks.contains_key(&child_chunk_start)
            || read_metadata_evidence_chunk(
                file,
                epoch.metadata_evidence_root,
                algo,
                epoch.epoch,
                epoch.reclaim_fence_identity,
                child_chunk_start,
                evidence_page_count,
            )?
            .is_some();
        let child_chunk = metadata_evidence_chunk_for_page(
            file,
            epoch.metadata_evidence_root,
            algo,
            epoch.epoch,
            epoch.reclaim_fence_identity,
            child.0,
            evidence_page_count,
            touched_chunks,
        )?;
        if value_tree {
            child_chunk.set_value_tree(child.0)?;
        }
        if free_page_extent_tree {
            child_chunk.set_free_page_extent_tree(child.0)?;
        }
        child_chunk.set_pending_root(child.0)?;
        if !child_chunk_exists {
            return Ok(true);
        }
        epoch.metadata_root_cursor = epoch.metadata_root_cursor.min(child.0);
        let mut next_child_offset = child_offset + 1;
        while next_child_offset < children.len() {
            let next_child = children[next_child_offset];
            if next_child.0 >= epoch.page_high_water_mark {
                return Err(corrupt(
                    "reachability mark metadata child root out of range",
                ));
            }
            if metadata_evidence_chunk_start(next_child.0) != child_chunk_start {
                break;
            }
            let next_child_chunk = metadata_evidence_chunk_for_page(
                file,
                epoch.metadata_evidence_root,
                algo,
                epoch.epoch,
                epoch.reclaim_fence_identity,
                next_child.0,
                evidence_page_count,
                touched_chunks,
            )?;
            if value_tree {
                next_child_chunk.set_value_tree(next_child.0)?;
            }
            if free_page_extent_tree {
                next_child_chunk.set_free_page_extent_tree(next_child.0)?;
            }
            next_child_chunk.set_pending_root(next_child.0)?;
            epoch.metadata_root_cursor = epoch.metadata_root_cursor.min(next_child.0);
            next_child_offset += 1;
        }
        let chunk = metadata_evidence_chunk_for_page(
            file,
            epoch.metadata_evidence_root,
            algo,
            epoch.epoch,
            epoch.reclaim_fence_identity,
            page,
            evidence_page_count,
            touched_chunks,
        )?;
        chunk.expansion_child_offset = u16::try_from(next_child_offset)
            .map_err(|_| corrupt("reachability mark metadata child offset overflow"))?;
        epoch.metadata_expansion_cursor = page;
        return Ok(true);
    }
    if value_tree && value_offset < values.len() {
        let value_page = values[value_offset].global_page();
        if value_page >= epoch.page_high_water_mark {
            return Err(corrupt(
                "reachability mark metadata value locator out of range",
            ));
        }
        let value_chunk_start = metadata_evidence_chunk_start(value_page);
        let value_chunk_exists = touched_chunks.contains_key(&value_chunk_start)
            || read_metadata_evidence_chunk(
                file,
                epoch.metadata_evidence_root,
                algo,
                epoch.epoch,
                epoch.reclaim_fence_identity,
                value_chunk_start,
                evidence_page_count,
            )?
            .is_some();
        if !value_chunk_exists {
            metadata_evidence_chunk_for_page(
                file,
                epoch.metadata_evidence_root,
                algo,
                epoch.epoch,
                epoch.reclaim_fence_identity,
                value_page,
                evidence_page_count,
                touched_chunks,
            )?
            .set_pending_value_blob(value_page)?;
            return Ok(true);
        }
        metadata_process_value_blob_page(
            file,
            algo,
            epoch,
            value_page,
            evidence_page_count,
            touched_chunks,
        )?;
        let mut next_value_offset = value_offset + 1;
        while next_value_offset < values.len() {
            let next_value_page = values[next_value_offset].global_page();
            if next_value_page >= epoch.page_high_water_mark {
                return Err(corrupt(
                    "reachability mark metadata value locator out of range",
                ));
            }
            if metadata_evidence_chunk_start(next_value_page) != value_chunk_start {
                break;
            }
            metadata_process_value_blob_page(
                file,
                algo,
                epoch,
                next_value_page,
                evidence_page_count,
                touched_chunks,
            )?;
            next_value_offset += 1;
        }
        let chunk = metadata_evidence_chunk_for_page(
            file,
            epoch.metadata_evidence_root,
            algo,
            epoch.epoch,
            epoch.reclaim_fence_identity,
            page,
            evidence_page_count,
            touched_chunks,
        )?;
        chunk.expansion_value_offset = u16::try_from(next_value_offset)
            .map_err(|_| corrupt("reachability mark metadata value offset overflow"))?;
        epoch.metadata_expansion_cursor = page;
        return Ok(true);
    }
    let chunk = metadata_evidence_chunk_for_page(
        file,
        epoch.metadata_evidence_root,
        algo,
        epoch.epoch,
        epoch.reclaim_fence_identity,
        page,
        evidence_page_count,
        touched_chunks,
    )?;
    chunk.clear_root_expansion();
    epoch.metadata_expansion_cursor = epoch.page_high_water_mark;
    Ok(true)
}

fn metadata_mark_reachable_page(
    file: &mut dyn crate::BackingIo,
    algo: Algo,
    epoch: &mut ReachabilityMarkEpoch,
    page: u64,
    evidence_page_count: u64,
    touched_chunks: &mut BTreeMap<u64, MetadataEvidenceChunk>,
) -> Result<()> {
    if page >= epoch.page_high_water_mark {
        return Err(corrupt(
            "reachability mark metadata value page out of range",
        ));
    }
    let chunk = metadata_evidence_chunk_for_page(
        file,
        epoch.metadata_evidence_root,
        algo,
        epoch.epoch,
        epoch.reclaim_fence_identity,
        page,
        evidence_page_count,
        touched_chunks,
    )?;
    if chunk.set_reachable(page)? {
        epoch.metadata_reachable_count = epoch.metadata_reachable_count.saturating_add(1);
    }
    Ok(())
}

fn metadata_process_value_blob_page(
    file: &mut dyn crate::BackingIo,
    algo: Algo,
    epoch: &mut ReachabilityMarkEpoch,
    page: u64,
    evidence_page_count: u64,
    touched_chunks: &mut BTreeMap<u64, MetadataEvidenceChunk>,
) -> Result<()> {
    if page >= epoch.page_high_water_mark {
        return Err(corrupt(
            "reachability mark metadata value page out of range",
        ));
    }
    let mut header = [0u8; crate::page::PAGE_SIZE as usize];
    crate::read_exact_at(file, PageId(page).offset(crate::DATA_START), &mut header)
        .map_err(crate::io_err)?;
    metadata_mark_reachable_page(file, algo, epoch, page, evidence_page_count, touched_chunks)?;
    match header[0] {
        crate::record::SLAB_MAGIC => Ok(()),
        crate::record::LARGE_MAGIC => {
            let len = crate::record::large_blob_len(&header)
                .ok_or_else(|| corrupt("bad large blob header"))?;
            let span = crate::record::large_pages(len);
            let end = page
                .checked_add(span)
                .ok_or_else(|| corrupt("large blob run overflow"))?;
            if end > epoch.page_high_water_mark {
                return Err(corrupt("large blob run past the page array"));
            }
            let chunk = metadata_evidence_chunk_for_page(
                file,
                epoch.metadata_evidence_root,
                algo,
                epoch.epoch,
                epoch.reclaim_fence_identity,
                page,
                evidence_page_count,
                touched_chunks,
            )?;
            chunk.start_large_value_continuation(page, end);
            epoch.metadata_value_blob_cursor =
                epoch.metadata_value_blob_cursor.min(page.saturating_add(1));
            Ok(())
        }
        crate::record::CHUNKED_BLOB_MAGIC => {
            let Some((next, _, _)) = crate::record::decode_chunked_blob_page(&header) else {
                return Err(corrupt("bad mutable blob chunk page"));
            };
            if let Some(next) = next {
                if next >= epoch.page_high_water_mark {
                    return Err(corrupt(
                        "mutable blob chunk chain is cyclic or out of bounds",
                    ));
                }
                metadata_evidence_chunk_for_page(
                    file,
                    epoch.metadata_evidence_root,
                    algo,
                    epoch.epoch,
                    epoch.reclaim_fence_identity,
                    next,
                    evidence_page_count,
                    touched_chunks,
                )?
                .set_pending_value_blob(next)?;
                epoch.metadata_value_blob_cursor = epoch.metadata_value_blob_cursor.min(next);
            }
            Ok(())
        }
        _ => Err(corrupt("bad blob page magic")),
    }
}

fn metadata_evidence_page_reachable(
    file: &mut dyn crate::BackingIo,
    root: Option<u64>,
    algo: Algo,
    epoch: u64,
    reclaim_fence_identity: Digest,
    page: u64,
    page_count: u64,
    touched_chunks: &BTreeMap<u64, MetadataEvidenceChunk>,
) -> Result<bool> {
    let page_start = metadata_evidence_chunk_start(page);
    if let Some(chunk) = touched_chunks.get(&page_start) {
        return chunk.contains_reachable(page);
    }
    Ok(read_metadata_evidence_chunk(
        file,
        root,
        algo,
        epoch,
        reclaim_fence_identity,
        page_start,
        page_count,
    )?
    .is_some_and(|chunk| chunk.contains_reachable(page).unwrap_or(false)))
}

fn mark_epoch_reclaim_fence_identity(
    algo: Algo,
    epoch: u64,
    base_generation: u64,
    page_high_water_mark: u64,
    captured_roots: &[Digest],
    captured_metadata_roots: &[u64],
    captured_metadata_value_roots: &[u64],
    captured_metadata_bootstrap_reserve: &MetadataBootstrapReserve,
) -> Digest {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"loom.store.mark-epoch.reclaim-fence.v1");
    bytes.extend_from_slice(&epoch.to_le_bytes());
    bytes.extend_from_slice(&base_generation.to_le_bytes());
    bytes.extend_from_slice(&page_high_water_mark.to_le_bytes());
    put_digest_list(&mut bytes, captured_roots);
    put_u64_list(&mut bytes, captured_metadata_roots);
    put_u64_list(&mut bytes, captured_metadata_value_roots);
    put_metadata_bootstrap_reserve(&mut bytes, captured_metadata_bootstrap_reserve);
    Digest::hash(algo, &bytes)
}

fn mark_epoch_captured_root_identity(algo: Algo, captured_roots: &[Digest]) -> Digest {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"loom.store.mark-epoch.captured-roots.v1");
    put_digest_list(&mut bytes, captured_roots);
    Digest::hash(algo, &bytes)
}

fn mark_epoch_captured_free_identity(
    algo: Algo,
    base_generation: u64,
    page_high_water_mark: u64,
    captured_free_root: Option<u64>,
    free: &[crate::FreePageRun],
) -> Digest {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"loom.store.mark-epoch.captured-free.v3");
    bytes.extend_from_slice(&base_generation.to_le_bytes());
    bytes.extend_from_slice(&page_high_water_mark.to_le_bytes());
    put_optional_u64(&mut bytes, captured_free_root);
    let captured = canonical_captured_free_runs(free, page_high_water_mark);
    bytes.extend_from_slice(&(captured.len() as u64).to_le_bytes());
    for run in captured {
        bytes.extend_from_slice(&run.start.to_le_bytes());
        bytes.extend_from_slice(&run.len.to_le_bytes());
        bytes.extend_from_slice(&run.freed_gen.to_le_bytes());
    }
    Digest::hash(algo, &bytes)
}

fn canonical_captured_free_runs(
    free: &[crate::FreePageRun],
    page_high_water_mark: u64,
) -> Vec<crate::FreePageRun> {
    let mut captured = free
        .iter()
        .filter_map(|run| {
            (run.start < page_high_water_mark).then_some(crate::FreePageRun {
                start: run.start,
                len: run.len.min(page_high_water_mark - run.start),
                freed_gen: run.freed_gen,
            })
        })
        .filter(|run| run.len > 0)
        .collect::<Vec<_>>();
    captured.sort_by(|left, right| {
        right
            .len
            .cmp(&left.len)
            .then_with(|| left.freed_gen.cmp(&right.freed_gen))
            .then_with(|| left.start.cmp(&right.start))
    });
    captured
}

#[derive(Default)]
pub(crate) struct CapturedFreeReuseSelection {
    pub(crate) runs: Vec<crate::FreePageRun>,
    pub(crate) allocation_authority: crate::pagemap::CapturedFreeAllocationAuthority,
    pub(crate) consumed_through: u64,
}

pub(crate) fn captured_free_reuse_runs(
    file: &mut dyn crate::BackingIo,
    algo: Algo,
    epoch: &ReachabilityMarkEpoch,
    current_free: &[crate::FreePageRun],
    minimum_recoverable_generation: u64,
    max_pages: usize,
) -> Result<CapturedFreeReuseSelection> {
    epoch.require_current_metadata_bootstrap_evidence()?;
    let Some(expected_identity) = epoch.captured_free_identity else {
        return Ok(CapturedFreeReuseSelection::default());
    };
    let captured = match epoch.captured_free_root {
        Some(root) => {
            crate::pagemap::read_map_with_root_span(
                file,
                crate::DATA_START,
                PageId(root),
                epoch.page_high_water_mark,
            )?
            .0
        }
        None => Vec::new(),
    };
    if mark_epoch_captured_free_identity(
        algo,
        epoch.base_generation,
        epoch.page_high_water_mark,
        epoch.captured_free_root,
        &captured,
    ) != expected_identity
    {
        return Err(corrupt("reachability mark captured-free identity mismatch"));
    }

    let captured = canonical_captured_free_runs(&captured, epoch.page_high_water_mark);
    let captured_page_count = captured
        .iter()
        .fold(0u64, |count, run| count.saturating_add(run.len));
    if epoch.captured_free_consumed_through > captured_page_count {
        return Err(corrupt(
            "reachability mark captured-free cursor out of range",
        ));
    }
    let mut selected = Vec::<crate::FreePageRun>::new();
    let mut allocation_runs = Vec::<crate::pagemap::CapturedFreeRun>::new();
    let mut selected_pages = 0usize;
    let mut consumed_through = epoch.captured_free_consumed_through;
    let mut ordinal = 0u64;
    'captured: for captured_run in &captured {
        let run_ordinal = ordinal;
        let run_ordinal_end = run_ordinal.saturating_add(captured_run.len);
        ordinal = run_ordinal_end;
        if consumed_through >= run_ordinal_end {
            continue;
        }
        if captured_run.freed_gen > minimum_recoverable_generation {
            consumed_through = run_ordinal_end;
            continue;
        }

        let offset = consumed_through
            .saturating_sub(run_ordinal)
            .min(captured_run.len);
        let mut page = captured_run.start.saturating_add(offset);
        let captured_end = captured_run.start.saturating_add(captured_run.len);
        while page < captured_end {
            if selected_pages >= max_pages {
                break 'captured;
            }
            let current_index =
                current_free.partition_point(|run| run.start.saturating_add(run.len) <= page);
            let Some(current_run) = current_free.get(current_index) else {
                consumed_through = run_ordinal_end;
                break;
            };
            if current_run.start >= captured_end {
                consumed_through = run_ordinal_end;
                break;
            }
            if current_run.start > page {
                let absent_end = current_run.start.min(captured_end);
                consumed_through = consumed_through.saturating_add(absent_end - page);
                page = absent_end;
                continue;
            }
            let available_end = current_run
                .start
                .saturating_add(current_run.len)
                .min(captured_end);
            let take = (available_end - page).min((max_pages - selected_pages) as u64);
            if take == 0 {
                break 'captured;
            }
            selected.push(crate::FreePageRun {
                start: page,
                len: take,
                freed_gen: captured_run.freed_gen,
            });
            allocation_runs.push(crate::pagemap::CapturedFreeRun {
                run: crate::FreePageRun {
                    start: page,
                    len: take,
                    freed_gen: captured_run.freed_gen,
                },
                cursor_start: consumed_through,
                cursor_end: consumed_through.saturating_add(take),
            });
            selected_pages += take as usize;
            consumed_through = consumed_through.saturating_add(take);
            page = page.saturating_add(take);
            if page < available_end {
                break 'captured;
            }
        }
    }
    selected.sort_by_key(|run| run.start);
    let mut coalesced = Vec::<crate::FreePageRun>::new();
    for run in selected {
        if let Some(last) = coalesced.last_mut()
            && last.start.saturating_add(last.len) == run.start
            && last.freed_gen == run.freed_gen
        {
            last.len = last.len.saturating_add(run.len);
        } else {
            coalesced.push(run);
        }
    }
    Ok(CapturedFreeReuseSelection {
        runs: coalesced,
        allocation_authority: crate::pagemap::CapturedFreeAllocationAuthority {
            runs: allocation_runs,
            consumed_through: epoch.captured_free_consumed_through,
            page_count: captured_page_count,
        },
        consumed_through,
    })
}

pub(crate) fn captured_free_consumed_runs(
    file: &mut dyn crate::BackingIo,
    algo: Algo,
    epoch: &ReachabilityMarkEpoch,
) -> Result<Vec<crate::FreePageRun>> {
    epoch.require_current_metadata_bootstrap_evidence()?;
    let Some(expected_identity) = epoch.captured_free_identity else {
        return Ok(Vec::new());
    };
    let captured = match epoch.captured_free_root {
        Some(root) => {
            crate::pagemap::read_map_with_root_span(
                file,
                crate::DATA_START,
                PageId(root),
                epoch.page_high_water_mark,
            )?
            .0
        }
        None => return Ok(Vec::new()),
    };
    if mark_epoch_captured_free_identity(
        algo,
        epoch.base_generation,
        epoch.page_high_water_mark,
        epoch.captured_free_root,
        &captured,
    ) != expected_identity
    {
        return Err(corrupt("reachability mark captured-free identity mismatch"));
    }
    captured_free_prefix_runs(
        &captured,
        epoch.page_high_water_mark,
        epoch.captured_free_consumed_through,
    )
}

fn captured_free_prefix_runs(
    captured: &[crate::FreePageRun],
    page_high_water_mark: u64,
    consumed_through: u64,
) -> Result<Vec<crate::FreePageRun>> {
    let captured = canonical_captured_free_runs(captured, page_high_water_mark);
    let captured_page_count = captured
        .iter()
        .fold(0u64, |count, run| count.saturating_add(run.len));
    if consumed_through > captured_page_count {
        return Err(corrupt(
            "reachability mark captured-free cursor out of range",
        ));
    }
    let mut remaining = consumed_through;
    let mut consumed = Vec::<crate::FreePageRun>::new();
    for run in captured {
        if remaining == 0 {
            break;
        }
        let len = run.len.min(remaining);
        if let Some(last) = consumed.last_mut()
            && last.start.saturating_add(last.len) == run.start
            && last.freed_gen == run.freed_gen
        {
            last.len = last.len.saturating_add(len);
        } else {
            consumed.push(crate::FreePageRun { len, ..run });
        }
        remaining -= len;
    }
    Ok(consumed)
}

pub(crate) fn advance_captured_free_consumption_in_control_map(
    control_map: &mut BTreeMap<Vec<u8>, Vec<u8>>,
    epoch: &ReachabilityMarkEpoch,
    consumed_through: u64,
    algo: Algo,
) -> Result<ReachabilityMarkEpoch> {
    epoch.require_current_metadata_bootstrap_evidence()?;
    if consumed_through < epoch.captured_free_consumed_through
        || consumed_through > epoch.page_high_water_mark
    {
        return Err(corrupt("reachability mark captured-free cursor regression"));
    }
    let persisted = control_map
        .get(MARK_EPOCH_KEY)
        .map(|bytes| decode_mark_epoch(bytes, algo))
        .transpose()?
        .ok_or_else(|| corrupt("reachability mark epoch missing during captured-free advance"))?;
    persisted.require_current_metadata_bootstrap_evidence()?;
    if persisted.epoch != epoch.epoch
        || persisted.base_generation != epoch.base_generation
        || persisted.page_high_water_mark != epoch.page_high_water_mark
        || persisted.captured_metadata_bootstrap_reserve
            != epoch.captured_metadata_bootstrap_reserve
        || persisted.captured_free_root != epoch.captured_free_root
        || persisted.captured_free_identity != epoch.captured_free_identity
        || persisted.captured_free_consumed_through != epoch.captured_free_consumed_through
    {
        return Err(corrupt("reachability mark captured-free identity mismatch"));
    }
    let mut next_epoch = epoch.clone();
    next_epoch.captured_free_consumed_through = consumed_through;
    control_map.insert(MARK_EPOCH_KEY.to_vec(), encode_mark_epoch(&next_epoch)?);
    if let Some(bytes) = control_map.get(MARK_EPOCH_RECLAIM_EVIDENCE_KEY) {
        let mut evidence = decode_mark_reclaim_evidence(bytes, algo)?;
        evidence.require_current_metadata_bootstrap_evidence()?;
        if !evidence.matches_epoch(epoch, algo) {
            return Err(corrupt("reachability mark reclaim evidence mismatch"));
        }
        evidence.captured_free_consumed_through = consumed_through;
        control_map.insert(
            MARK_EPOCH_RECLAIM_EVIDENCE_KEY.to_vec(),
            encode_mark_reclaim_evidence(&evidence)?,
        );
    }
    Ok(next_epoch)
}

pub(crate) fn active_mark_epoch_from_control_map(
    control_map: &BTreeMap<Vec<u8>, Vec<u8>>,
    algo: Algo,
) -> Result<Option<ReachabilityMarkEpoch>> {
    Ok(control_map
        .get(MARK_EPOCH_KEY)
        .map(|bytes| decode_mark_epoch(bytes, algo))
        .transpose()?
        .filter(|epoch| {
            epoch.metadata_bootstrap_evidence_provenance
                == MetadataBootstrapEvidenceProvenance::Current
        }))
}

#[cfg(test)]
mod inline_free_page_extent_tests {
    use super::*;
    use crate::backing::MemoryBacking;
    use crate::pagebtree::FreePageExtentValue;
    use crate::pagemap::PageAllocator;
    use loom_core::ObjectStore;

    fn empty_state() -> ReachabilityMarkState {
        ReachabilityMarkState {
            pinned: BTreeSet::new(),
            marked: BTreeSet::new(),
            queue: VecDeque::new(),
            stream_roots: VecDeque::new(),
            content_roots: VecDeque::new(),
            prolly_cursors: VecDeque::new(),
            completed: true,
        }
    }

    fn expansion_epoch(root: PageId, page_count: u64) -> ReachabilityMarkEpoch {
        ReachabilityMarkEpoch {
            epoch: 1,
            base_generation: 1,
            page_high_water_mark: page_count,
            captured_root_vector: Vec::new(),
            captured_metadata_roots: vec![root.0],
            captured_metadata_value_roots: Vec::new(),
            captured_metadata_bootstrap_reserve: MetadataBootstrapReserve::default(),
            metadata_bootstrap_evidence_provenance: MetadataBootstrapEvidenceProvenance::Current,
            captured_free_root: Some(root.0),
            captured_free_identity: None,
            captured_free_consumed_through: 0,
            metadata_work_initialized: true,
            metadata_root_cursor: 0,
            metadata_value_root_cursor: 0,
            metadata_value_blob_cursor: 0,
            metadata_expansion_cursor: root.0,
            metadata_classify_next_page: page_count,
            metadata_evidence_root: None,
            metadata_reachable_count: 0,
            metadata_reclaim_candidate_count: 0,
            metadata_evidence_identity: Digest::blake3(b"extent-tree-evidence"),
            metadata_completed: false,
            reclaim_fence_identity: Digest::blake3(b"extent-tree-fence"),
            reference_root: None,
            control_fingerprint: None,
            canonical_roots_fingerprint: None,
            derived_roots: BTreeSet::new(),
            state: empty_state(),
        }
    }

    fn extent_key(start: u64) -> [u8; 32] {
        let mut key = [0u8; 32];
        key[24..].copy_from_slice(&start.to_be_bytes());
        key
    }

    #[test]
    fn free_page_extent_expansion_crosses_chunk_boundary_without_locator_values() {
        let mut backing = MemoryBacking::new();
        let mut allocator = PageAllocator::new(MARK_EPOCH_CHUNK_PAGES - 2, 1, Vec::new());
        let entries = (0..150u64)
            .map(|index| {
                (
                    extent_key(index * 2),
                    FreePageExtentValue {
                        len: 1,
                        freed_gen: 1,
                    },
                )
            })
            .collect::<Vec<_>>();
        let root = crate::pagebtree::build_packed_free_page_extents(
            &mut backing,
            crate::DATA_START,
            &mut allocator,
            &entries,
        )
        .unwrap()
        .unwrap();
        let page_count = allocator.page_count();
        let links = crate::pagebtree::free_page_extent_node_links(
            &mut backing,
            crate::DATA_START,
            root,
            page_count,
        )
        .unwrap()
        .unwrap();
        assert!(links.children.len() > 1);
        assert!(
            links
                .children
                .iter()
                .any(|child| metadata_evidence_chunk_start(child.0)
                    != metadata_evidence_chunk_start(root.0))
        );

        let mut epoch = expansion_epoch(root, page_count);
        let mut chunks = BTreeMap::new();
        let chunk = metadata_evidence_chunk_for_page(
            &mut backing,
            None,
            Algo::Blake3,
            epoch.epoch,
            epoch.reclaim_fence_identity,
            root.0,
            page_count,
            &mut chunks,
        )
        .unwrap();
        chunk.set_free_page_extent_tree(root.0).unwrap();
        chunk
            .start_root_expansion(root.0, false, true, links.children.len(), 0)
            .unwrap();

        let mut calls = 0usize;
        while epoch.metadata_expansion_cursor < epoch.page_high_water_mark {
            assert!(
                metadata_process_root_expansion_continuation(
                    &mut backing,
                    Algo::Blake3,
                    &mut epoch,
                    page_count,
                    &mut chunks,
                    &BTreeSet::new(),
                )
                .unwrap()
            );
            calls += 1;
            assert!(calls < 32);
        }
        assert!(calls > 1);
        for child in links.children {
            let child_chunk = chunks.get(&metadata_evidence_chunk_start(child.0)).unwrap();
            assert!(child_chunk.contains_free_page_extent_tree(child.0).unwrap());
            assert!(!child_chunk.contains_value_tree(child.0).unwrap());
            assert!(metadata_bit(
                &child_chunk.pending_roots,
                child_chunk.page_start,
                child.0
            ));
            assert!(!metadata_bit(
                &child_chunk.pending_value_blobs,
                child_chunk.page_start,
                child.0
            ));
        }
    }

    #[test]
    fn canonical_locator_roots_retain_payload_pages_and_unknown_families_fail_capture() {
        let store = FileStore::with_backing(Box::new(MemoryBacking::new()), true).unwrap();
        let payload = vec![0x5au8; crate::page::PAGE_SIZE as usize * 2];
        let digest = store.put(&payload).unwrap();
        let (index_root, locator_page, canonical_roots) = {
            let mut inner = store.inner.lock().unwrap();
            let control_map = store.control_map_locked(&mut inner).unwrap();
            let canonical_roots = store.gc_canonical_roots_locked(&inner, &control_map);
            let index_root = inner.index_root.unwrap();
            let page_count = inner.page_count;
            let locator = crate::pagebtree::get(
                &mut **store.file.lock().unwrap(),
                crate::DATA_START,
                Some(index_root),
                digest.bytes(),
                page_count,
            )
            .unwrap()
            .unwrap();
            (index_root, locator.global_page(), canonical_roots)
        };
        let (metadata_roots, value_roots) = {
            let inner = store.inner.lock().unwrap();
            mark_epoch_captured_metadata_roots(&inner, &canonical_roots).unwrap()
        };
        assert!(metadata_roots.contains(&index_root.0));
        assert!(value_roots.contains(&index_root.0));
        if let Some(free_root) = store.inner.lock().unwrap().freemap.map(|(root, _)| root.0) {
            assert!(!value_roots.contains(&free_root));
        }

        let mut epoch = store
            .begin_reachability_mark_epoch(None, BTreeSet::new(), empty_state())
            .unwrap();
        while !epoch.metadata_completed {
            store
                .step_reachability_metadata_mark_epoch(&mut epoch, 16, None)
                .unwrap();
        }
        assert!(
            store
                .reachability_mark_metadata_page_state_for_test(&epoch, locator_page)
                .unwrap()
                .0
        );

        let mut unknown = canonical_roots;
        unknown.push(crate::compact::GcCanonicalRootEvidence {
            name: "unknown".to_string(),
            family_id: Some(u16::MAX),
            page_root: Some(index_root),
            digest_root: None,
            reachability: "control".to_string(),
            semantic_liveness: false,
            advisory: false,
        });
        let inner = store.inner.lock().unwrap();
        let error = mark_epoch_captured_metadata_roots(&inner, &unknown).unwrap_err();
        assert_eq!(error.code, Code::CorruptObject);
    }

    #[test]
    fn legacy_contiguous_free_map_bytes_are_not_reclaimable_metadata() {
        let mut page = [0u8; crate::page::PAGE_SIZE as usize];
        page[0] = 0xB4;
        page[1..5].copy_from_slice(&1u32.to_le_bytes());
        page[5..13].copy_from_slice(&7u64.to_le_bytes());
        page[13..21].copy_from_slice(&2u64.to_le_bytes());
        page[21..29].copy_from_slice(&3u64.to_le_bytes());
        let crc = crate::crc32c(&page[..29]);
        page[29..33].copy_from_slice(&crc.to_le_bytes());
        assert!(crate::pagemap::decode(&page).is_some());

        let mut backing = MemoryBacking::new();
        crate::write_at(&mut backing, crate::DATA_START, &page).unwrap();
        assert!(!page_contains_reclaimable_metadata(&mut backing, 0, 1).unwrap());
    }
}
