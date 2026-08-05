use super::*;
use crate::compact::{GcCanonicalRootEvidence, GcCompactionClassification, GcReclaimEvidence};
use crate::derived::{
    CALENDAR_DERIVED_INDEX_FORMAT_VERSION, CONTACTS_DERIVED_INDEX_FORMAT_VERSION,
    DATAFRAME_MATERIALIZATION_ARTIFACT_PREFIX, DATAFRAME_MATERIALIZATION_FORMAT_VERSION,
    GRAPH_PROPERTY_INDEX_ARTIFACT_PREFIX, GRAPH_PROPERTY_INDEX_FORMAT_VERSION,
    GRAPH_SPATIAL_INDEX_ARTIFACT_PREFIX, GRAPH_SPATIAL_INDEX_FORMAT_VERSION,
    MAIL_DERIVED_INDEX_FORMAT_VERSION, PIM_DERIVED_INDEX_ARTIFACT_PREFIX,
    SearchEmbeddingProjection, calendar_derived_index_artifact_key,
    calendar_derived_index_artifact_stamp, contacts_derived_index_artifact_key,
    contacts_derived_index_artifact_stamp, dataframe_materialization_artifact_key,
    dataframe_materialization_artifact_stamp, derived_artifact_format_version,
    graph_property_index_artifact_key, graph_property_index_artifact_stamp,
    graph_spatial_index_artifact_key, graph_spatial_index_artifact_stamp,
    mail_derived_index_artifact_key, mail_derived_index_artifact_stamp,
    search_embedding_artifact_key, search_embedding_artifact_stamp, vector_hnsw_artifact_key,
    vector_hnsw_artifact_stamp, vector_pq_artifact_key, vector_pq_artifact_stamp,
};
use loom_core::{
    AtomicityBoundary, AuditIntent, CompareToken, FacetSideEffect, FacetSideEffects, FacetWrite,
    FacetWriteOp, Object, OverlayDurabilityPolicy, SecondaryIndexWrite, SecondaryIndexWriteOp,
    WorkflowPlanningSnapshot, WorkflowTransaction, document, workspace::FacetKind,
};
use std::sync::Barrier;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A shared in-memory [`BackingIo`]: cloning shares the same byte buffer, so a `FileStore` can be
/// dropped and a fresh one reopened over the identical bytes - the persistence guarantee the OPFS
/// backend must also provide.
#[derive(Debug, Clone, Default)]
struct SharedMem(Arc<Mutex<Vec<u8>>>);

impl SharedMem {
    fn mutate_bytes(&self, mutate: impl FnOnce(&mut Vec<u8>)) {
        mutate(&mut self.0.lock().unwrap());
    }

    fn bytes(&self) -> Vec<u8> {
        self.0.lock().unwrap().clone()
    }
}

impl BackingIo for SharedMem {
    fn pread(&mut self, off: u64, buf: &mut [u8]) -> std::io::Result<()> {
        let g = self.0.lock().unwrap();
        let (off, end) = (off as usize, off as usize + buf.len());
        if end > g.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "eof",
            ));
        }
        buf.copy_from_slice(&g[off..end]);
        Ok(())
    }
    fn pwrite(&mut self, off: u64, buf: &[u8]) -> std::io::Result<()> {
        let mut g = self.0.lock().unwrap();
        let (off, end) = (off as usize, off as usize + buf.len());
        if end > g.len() {
            g.resize(end, 0);
        }
        g[off..end].copy_from_slice(buf);
        Ok(())
    }
    fn size(&self) -> std::io::Result<u64> {
        Ok(self.0.lock().unwrap().len() as u64)
    }
    fn grow(&mut self, len: u64) -> std::io::Result<()> {
        self.0.lock().unwrap().resize(len as usize, 0);
        Ok(())
    }
    fn fsync(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
struct CountingMem {
    shared: SharedMem,
    data_pages_read: Arc<AtomicU64>,
    data_pages_written: Arc<AtomicU64>,
    written_pages: Arc<Mutex<BTreeSet<u64>>>,
}

impl CountingMem {
    fn reset_pages_read(&self) {
        self.data_pages_read.store(0, Ordering::SeqCst);
    }

    fn reset_pages_written(&self) {
        self.data_pages_written.store(0, Ordering::SeqCst);
        self.written_pages.lock().unwrap().clear();
    }

    fn reset_io_pages(&self) {
        self.reset_pages_read();
        self.reset_pages_written();
    }

    fn pages_read(&self) -> u64 {
        self.data_pages_read.load(Ordering::SeqCst)
    }

    fn pages_written(&self) -> u64 {
        self.data_pages_written.load(Ordering::SeqCst)
    }

    fn wrote_page(&self, page: PageId) -> bool {
        self.written_pages.lock().unwrap().contains(&page.0)
    }
}

impl BackingIo for CountingMem {
    fn pread(&mut self, off: u64, buf: &mut [u8]) -> std::io::Result<()> {
        if !buf.is_empty() {
            let end = off.saturating_add(buf.len() as u64);
            let data_start = DATA_START;
            if end > data_start {
                let first = off.max(data_start).saturating_sub(data_start) / PAGE_SIZE;
                let last = (end - 1).saturating_sub(data_start) / PAGE_SIZE;
                self.data_pages_read.fetch_add(
                    last.saturating_sub(first).saturating_add(1),
                    Ordering::SeqCst,
                );
            }
        }
        self.shared.pread(off, buf)
    }

    fn pwrite(&mut self, off: u64, buf: &[u8]) -> std::io::Result<()> {
        if !buf.is_empty() {
            let end = off.saturating_add(buf.len() as u64);
            let data_start = DATA_START;
            if end > data_start {
                let first = off.max(data_start).saturating_sub(data_start) / PAGE_SIZE;
                let last = (end - 1).saturating_sub(data_start) / PAGE_SIZE;
                self.data_pages_written.fetch_add(
                    last.saturating_sub(first).saturating_add(1),
                    Ordering::SeqCst,
                );
                let mut pages = self.written_pages.lock().unwrap();
                for page in first..=last {
                    pages.insert(page);
                }
            }
        }
        self.shared.pwrite(off, buf)
    }

    fn size(&self) -> std::io::Result<u64> {
        self.shared.size()
    }

    fn grow(&mut self, len: u64) -> std::io::Result<()> {
        self.shared.grow(len)
    }

    fn fsync(&mut self) -> std::io::Result<()> {
        self.shared.fsync()
    }
}

#[derive(Debug, Clone)]
struct FailNthFsyncMem {
    shared: SharedMem,
    fsyncs: Arc<AtomicU64>,
    fail_on: u64,
}

impl FailNthFsyncMem {
    fn new(shared: SharedMem, fail_on: u64) -> FailNthFsyncMem {
        FailNthFsyncMem {
            shared,
            fsyncs: Arc::new(AtomicU64::new(0)),
            fail_on,
        }
    }
}

impl BackingIo for FailNthFsyncMem {
    fn pread(&mut self, off: u64, buf: &mut [u8]) -> std::io::Result<()> {
        self.shared.pread(off, buf)
    }

    fn pwrite(&mut self, off: u64, buf: &[u8]) -> std::io::Result<()> {
        self.shared.pwrite(off, buf)
    }

    fn size(&self) -> std::io::Result<u64> {
        self.shared.size()
    }

    fn grow(&mut self, len: u64) -> std::io::Result<()> {
        self.shared.grow(len)
    }

    fn fsync(&mut self) -> std::io::Result<()> {
        let next = self.fsyncs.fetch_add(1, Ordering::SeqCst) + 1;
        if next == self.fail_on {
            Err(std::io::Error::other("injected fsync failure"))
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Default)]
struct FsyncGateMem {
    bytes: Arc<Mutex<Vec<u8>>>,
    gate: Arc<FsyncGate>,
}

#[derive(Debug, Default)]
struct FsyncGate {
    enabled: AtomicBool,
    first_blocked: AtomicBool,
    fsyncs: AtomicU64,
    release: Mutex<bool>,
    cv: Condvar,
}

impl FsyncGate {
    fn enable(&self) {
        self.enabled.store(true, Ordering::SeqCst);
    }

    fn wait_until_first_blocked(&self) {
        let mut release = self.release.lock().unwrap();
        for _ in 0..10 {
            if self.first_blocked.load(Ordering::SeqCst) {
                return;
            }
            let (next, _) = self
                .cv
                .wait_timeout(release, std::time::Duration::from_millis(100))
                .unwrap();
            release = next;
        }
        panic!("first fsync did not block");
    }

    fn release(&self) {
        *self.release.lock().unwrap() = true;
        self.cv.notify_all();
    }
}

impl BackingIo for FsyncGateMem {
    fn pread(&mut self, off: u64, buf: &mut [u8]) -> std::io::Result<()> {
        let g = self.bytes.lock().unwrap();
        let (off, end) = (off as usize, off as usize + buf.len());
        if end > g.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "eof",
            ));
        }
        buf.copy_from_slice(&g[off..end]);
        Ok(())
    }
    fn pwrite(&mut self, off: u64, buf: &[u8]) -> std::io::Result<()> {
        let mut g = self.bytes.lock().unwrap();
        let (off, end) = (off as usize, off as usize + buf.len());
        if end > g.len() {
            g.resize(end, 0);
        }
        g[off..end].copy_from_slice(buf);
        Ok(())
    }
    fn size(&self) -> std::io::Result<u64> {
        Ok(self.bytes.lock().unwrap().len() as u64)
    }
    fn grow(&mut self, len: u64) -> std::io::Result<()> {
        self.bytes.lock().unwrap().resize(len as usize, 0);
        Ok(())
    }
    fn fsync(&mut self) -> std::io::Result<()> {
        self.gate.fsyncs.fetch_add(1, Ordering::SeqCst);
        if self.gate.enabled.load(Ordering::SeqCst)
            && !self.gate.first_blocked.swap(true, Ordering::SeqCst)
        {
            self.gate.cv.notify_all();
            let mut release = self.gate.release.lock().unwrap();
            while !*release {
                release = self.gate.cv.wait(release).unwrap();
            }
        }
        Ok(())
    }
}

fn hot_mutable_record(seed: u8) -> ([u8; 32], Vec<u8>) {
    ([seed; 32], vec![seed, seed.wrapping_add(1)])
}

#[test]
fn hot_mutable_commit_queue_preserves_order_and_generation_window() {
    let mut queue = HotMutableCommitQueue::default();

    let first = queue
        .enqueue(
            7,
            StoreDurabilityPolicy::Normal,
            vec![hot_mutable_record(1), hot_mutable_record(2)],
        )
        .unwrap();
    let second = queue
        .enqueue(
            7,
            StoreDurabilityPolicy::Normal,
            vec![hot_mutable_record(3)],
        )
        .unwrap();

    assert_eq!(
        first,
        HotMutableCommitWindow {
            first_sequence: 0,
            last_sequence: 0,
            base_generation: 7,
            pending_generation: 8,
            transaction_count: 1,
            record_count: 2,
        }
    );
    assert_eq!(
        second,
        HotMutableCommitWindow {
            first_sequence: 0,
            last_sequence: 1,
            base_generation: 7,
            pending_generation: 9,
            transaction_count: 2,
            record_count: 3,
        }
    );
}

#[test]
fn hot_mutable_commit_queue_rejects_strict_boundaries_and_invalid_entries() {
    let mut queue = HotMutableCommitQueue::default();
    let empty = queue
        .enqueue(0, StoreDurabilityPolicy::Normal, Vec::new())
        .unwrap_err();
    let strict = queue
        .enqueue(
            0,
            StoreDurabilityPolicy::Strict,
            vec![hot_mutable_record(1)],
        )
        .unwrap_err();
    let relaxed = queue
        .enqueue(
            0,
            StoreDurabilityPolicy::Relaxed,
            vec![hot_mutable_record(1)],
        )
        .unwrap_err();
    let ephemeral = queue
        .enqueue(
            0,
            StoreDurabilityPolicy::Ephemeral,
            vec![hot_mutable_record(1)],
        )
        .unwrap_err();

    assert_eq!(empty.code, Code::InvalidArgument);
    assert_eq!(strict.code, Code::InvalidArgument);
    assert_eq!(relaxed.code, Code::InvalidArgument);
    assert_eq!(ephemeral.code, Code::InvalidArgument);
    assert!(queue.pending_window().is_none());
}

#[test]
fn hot_mutable_commit_queue_drains_ordered_batches_without_torn_entries() {
    let mut queue = HotMutableCommitQueue::default();
    queue
        .enqueue(
            11,
            StoreDurabilityPolicy::Normal,
            vec![hot_mutable_record(1), hot_mutable_record(2)],
        )
        .unwrap();
    queue
        .enqueue(
            11,
            StoreDurabilityPolicy::Normal,
            vec![hot_mutable_record(3), hot_mutable_record(4)],
        )
        .unwrap();
    queue
        .enqueue(
            11,
            StoreDurabilityPolicy::Normal,
            vec![hot_mutable_record(5)],
        )
        .unwrap();

    let first_batch = queue.drain_ready(2);
    let window = queue.pending_window().unwrap();
    let second_batch = queue.drain_ready(8);

    assert_eq!(first_batch.len(), 1);
    assert_eq!(first_batch[0].sequence, 0);
    assert_eq!(first_batch[0].pending_generation, 12);
    assert_eq!(first_batch[0].records.len(), 2);
    assert_eq!(window.first_sequence, 1);
    assert_eq!(window.last_sequence, 2);
    assert_eq!(window.pending_generation, 14);
    assert_eq!(window.transaction_count, 2);
    assert_eq!(window.record_count, 3);
    assert_eq!(
        second_batch
            .iter()
            .map(|commit| commit.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(queue.pending_window().is_none());
}

#[test]
fn normal_hot_mutable_publisher_drains_queue_and_publishes_complete_records() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let first = hot_mutable_record(61);
    let second = hot_mutable_record(62);

    store
        .enqueue_hot_mutable_commit_for_test(vec![first.clone(), second.clone()])
        .unwrap();
    assert_eq!(
        store
            .hot_mutable_commit_window()
            .unwrap()
            .unwrap()
            .record_count,
        2
    );
    store.flush_hot_mutable_commits().unwrap();

    assert!(store.hot_mutable_commit_window().unwrap().is_none());
    assert_eq!(
        store.mutable_overlay_record_payload(&first.0).unwrap(),
        Some(first.1)
    );
    assert_eq!(
        store.mutable_overlay_record_payload(&second.0).unwrap(),
        Some(second.1)
    );
}

#[test]
fn group_commit_diagnostics_reflect_hot_mutable_commits() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let first = hot_mutable_record(71);
    let second = hot_mutable_record(72);
    let third = hot_mutable_record(73);

    // Enqueued but not yet drained: the point-in-time pending-window gauges reflect the queued
    // transactions/records, and no batch has been published yet.
    store
        .enqueue_hot_mutable_commit_for_test(vec![first.clone(), second.clone()])
        .unwrap();
    store
        .enqueue_hot_mutable_commit_for_test(vec![third.clone()])
        .unwrap();
    let pending = store.group_commit_diagnostics().unwrap();
    assert_eq!(pending.pending_durable_window_transactions, 2);
    assert_eq!(pending.pending_durable_window_records, 3);
    assert_eq!(pending.group_commit_batches_total, 0);

    // Drain and publish the queued transactions.
    store.flush_hot_mutable_commits().unwrap();

    let after = store.group_commit_diagnostics().unwrap();
    assert!(after.group_commit_batches_total > 0);
    assert_eq!(after.group_commit_transactions_total, 2);
    assert_eq!(after.group_commit_records_total, 3);
    assert!(after.fsync_count > 0);
    assert_eq!(after.pending_durable_window_transactions, 0);
    assert_eq!(after.pending_durable_window_records, 0);
    assert_eq!(after.pinned_reader_blockers, Some(0));

    // The diagnostics are surfaced through the maintenance-status surface and serialize (Debug).
    let status = store.maintenance_status().unwrap();
    assert_eq!(status.group_commit, after);
    assert!(format!("{:?}", status.group_commit).contains("group_commit_batches_total"));
}

#[test]
fn strict_mutable_publication_flushes_pending_normal_queue_first() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let normal = hot_mutable_record(63);
    let strict = hot_mutable_record(64);

    store
        .enqueue_hot_mutable_commit_for_test(vec![normal.clone()])
        .unwrap();
    store
        .publish_mutable_overlay_records_for_test(
            StoreDurabilityPolicy::Strict,
            vec![strict.clone()],
        )
        .unwrap();

    assert!(store.hot_mutable_commit_window().unwrap().is_none());
    assert_eq!(
        store.mutable_overlay_record_payload(&normal.0).unwrap(),
        Some(normal.1)
    );
    assert_eq!(
        store.mutable_overlay_record_payload(&strict.0).unwrap(),
        Some(strict.1)
    );
}

#[test]
fn normal_hot_mutable_public_writes_batch_under_contention() {
    let backing = FsyncGateMem::default();
    let reopened_backing = backing.clone();
    let gate = Arc::clone(&backing.gate);
    let store = Arc::new(FileStore::with_backing(Box::new(backing), true).unwrap());
    gate.enable();
    let barrier = Arc::new(std::sync::Barrier::new(9));
    let mut threads = Vec::new();
    for worker in 0..8u8 {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        threads.push(std::thread::spawn(move || {
            let key = OverlayKey::from_segments([
                b"workspace",
                &[worker; 16],
                b"tickets",
                b"matrix",
                b"ticket",
                b"MX-433",
            ])
            .unwrap();
            barrier.wait();
            store.put_mutable_overlay_value(key, vec![worker])
        }));
    }
    barrier.wait();
    gate.wait_until_first_blocked();
    std::thread::sleep(std::time::Duration::from_millis(20));
    gate.release();
    for thread in threads {
        thread.join().unwrap().unwrap();
    }
    let committed_generation = store.generation();
    assert!(committed_generation < 8);
    drop(store);

    let reopened = FileStore::with_backing(Box::new(reopened_backing), true).unwrap();
    assert_eq!(reopened.generation(), committed_generation);
    for worker in 0..8u8 {
        let key = OverlayKey::from_segments([
            b"workspace",
            &[worker; 16],
            b"tickets",
            b"matrix",
            b"ticket",
            b"MX-433",
        ])
        .unwrap();
        assert_eq!(
            reopened
                .mutable_overlay_snapshot()
                .unwrap()
                .read_composite(&key, |_| Ok(None))
                .unwrap(),
            Some(vec![worker])
        );
    }
}

#[test]
fn unpublished_hot_mutable_queue_entries_do_not_recover_as_partial_state() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let pending = hot_mutable_record(65);

    store
        .enqueue_hot_mutable_commit_for_test(vec![pending.clone()])
        .unwrap();
    assert!(store.hot_mutable_commit_window().unwrap().is_some());
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();

    assert!(reopened.hot_mutable_commit_window().unwrap().is_none());
    assert_eq!(
        reopened.mutable_overlay_record_payload(&pending.0).unwrap(),
        None
    );
}

#[test]
fn strict_boundary_flushes_pending_normal_batch_and_survives_reopen() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let normal_key = durability_test_key("strict-boundary-normal");
    let strict_key = durability_test_key("strict-boundary-strict");
    let normal_token = loom_core::OverlayOwnerToken::from_bytes([66; 32]);
    let strict_token = loom_core::OverlayOwnerToken::from_bytes([67; 32]);
    let normal = (
        mutable_overlay_owner_token_address(&normal_key),
        encode_mutable_overlay_owner_token_record(&normal_token),
    );
    let strict = (
        mutable_overlay_owner_token_address(&strict_key),
        encode_mutable_overlay_owner_token_record(&strict_token),
    );

    store
        .enqueue_hot_mutable_commit_for_test(vec![normal.clone()])
        .unwrap();
    store
        .publish_mutable_overlay_records_for_test(
            StoreDurabilityPolicy::Strict,
            vec![strict.clone()],
        )
        .unwrap();
    let committed_generation = store.generation();
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();

    assert_eq!(reopened.generation(), committed_generation);
    assert_eq!(
        reopened.owner_token_record_payload(&normal.0).unwrap(),
        Some(normal.1)
    );
    assert_eq!(
        reopened.owner_token_record_payload(&strict.0).unwrap(),
        Some(strict.1)
    );
}

#[test]
fn file_store_over_a_non_file_backing_round_trips_and_reopens() {
    // The path the OPFS backend follows: a FileStore built over a BackingIo that is not a
    // std::fs::File. Put an object, read it back, drop the store, then reopen over the SAME bytes -
    // the committed object survives, proving the backing abstraction carries the whole format.
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let d = store.put(b"hello, backing").unwrap();
    assert_eq!(
        store.get(&d).unwrap().as_deref(),
        Some(&b"hello, backing"[..])
    );
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    assert_eq!(
        reopened.get(&d).unwrap().as_deref(),
        Some(&b"hello, backing"[..])
    );

    // A plain MemoryBacking also initializes + serves within one lifetime.
    let mem = FileStore::with_backing(Box::new(MemoryBacking::new()), true).unwrap();
    let d2 = mem.put(b"x").unwrap();
    assert!(mem.has(&d2).unwrap());
}

#[test]
fn maintenance_status_is_persisted_and_rejects_corrupt_record() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let d = store.put(b"maintenance metadata").unwrap();
    let first = store.maintenance_status().unwrap();
    assert_eq!(first.generation, 1);
    assert_eq!(first.object_count, 1);
    assert!(first.physical_page_count > 0);
    assert_eq!(
        first.physical_bytes,
        DATA_START + first.physical_page_count * PAGE_SIZE
    );
    assert!(!first.touched_segments.is_empty());
    assert!(!first.candidate_segments.is_empty());

    store.set_reference_root(Some(d)).unwrap();
    let second = store.maintenance_status().unwrap();
    assert_eq!(second.generation, 2);
    assert_eq!(second.object_count, 1);
    assert!(second.reusable_free_pages > 0);
    assert_eq!(second.candidate_dead_pages, second.reusable_free_pages);
    let maintenance_root = store
        .inner
        .lock()
        .unwrap()
        .maintenance_root
        .expect("maintenance root");
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let reopened_status = reopened.maintenance_status().unwrap();
    assert_eq!(reopened_status.generation, second.generation);
    assert_eq!(reopened_status.object_count, second.object_count);
    assert_eq!(
        reopened_status.physical_page_count,
        second.physical_page_count
    );
    assert_eq!(reopened_status.physical_bytes, second.physical_bytes);
    assert_eq!(
        reopened_status.reusable_free_pages,
        second.reusable_free_pages
    );
    assert_eq!(
        reopened_status.candidate_dead_pages,
        second.candidate_dead_pages
    );
    assert_eq!(reopened_status.tail_free_pages, second.tail_free_pages);
    assert_eq!(reopened_status.tail_free_bytes, second.tail_free_bytes);
    assert_eq!(
        reopened_status.last_validated_mark_epoch,
        second.last_validated_mark_epoch
    );
    assert_eq!(reopened_status.touched_segments, second.touched_segments);
    assert_eq!(
        reopened_status.candidate_segments,
        second.candidate_segments
    );
    assert_eq!(reopened_status.segment_overflow, second.segment_overflow);
    assert_eq!(
        reopened_status.group_commit,
        GroupCommitDiagnostics {
            pinned_reader_blockers: Some(0),
            ..GroupCommitDiagnostics::default()
        }
    );
    drop(reopened);

    shared.mutate_bytes(|bytes| {
        let pos = (DATA_START + maintenance_root.0 * PAGE_SIZE) as usize;
        bytes[pos] ^= 0x80;
    });
    let err = FileStore::with_backing(Box::new(shared), true).unwrap_err();
    assert_eq!(err.code, Code::CorruptObject);
}

#[test]
fn store_maintenance_policy_and_run_state_persist() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let policy = StoreMaintenancePolicy {
        min_candidate_pages: 4,
        min_reusable_pages: 5,
        interval_ms: 10_000,
        backoff_ms: 30_000,
        max_segments: 2,
        max_pages: 128,
        full_compaction_enabled: true,
        tail_trim_enabled: true,
        tail_compaction_enabled: true,
        tail_compaction_max_pages: 32,
        tail_compaction_max_objects: 16,
        tail_compaction_max_bytes: 512 * 1024,
        tail_compaction_interval_ms: 20_000,
        tail_compaction_backoff_ms: 60_000,
    };
    store.set_store_maintenance_policy(policy).unwrap();
    let run_state = StoreMaintenanceRunState {
        last_run_ms: Some(42),
        next_eligible_ms: 99,
        last_skip_reason: Some("candidate_debt_below_threshold".to_string()),
        last_error: Some("io pressure".to_string()),
        last_tail_trim_attempted: true,
        last_tail_trim_pages: 3,
        last_tail_trim_bytes: 3 * PAGE_SIZE,
        last_tail_compaction_attempted: true,
        last_tail_compaction_relocated_objects: 2,
        last_tail_compaction_relocated_pages: 3,
        last_tail_compaction_relocated_bytes: 3 * PAGE_SIZE,
        last_tail_compaction_truncated_pages: 1,
        last_tail_compaction_conflicts: 4,
        last_shrink_skip_reason: Some("tail_blocked_by_live_objects".to_string()),
        last_progress_steps: 7,
        last_yield_count: 8,
        last_overrun_count: 9,
    };
    store
        .record_store_maintenance_run_state(run_state.clone())
        .unwrap();
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    assert_eq!(reopened.store_maintenance_policy().unwrap(), policy);
    assert_eq!(reopened.store_maintenance_run_state().unwrap(), run_state);
}

#[test]
fn store_maintenance_policy_rejects_invalid_updates_without_overwrite() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let policy = StoreMaintenancePolicy {
        min_candidate_pages: 4,
        min_reusable_pages: 5,
        interval_ms: 10_000,
        backoff_ms: 30_000,
        max_segments: 2,
        max_pages: 128,
        full_compaction_enabled: true,
        ..StoreMaintenancePolicy::default()
    };
    store.set_store_maintenance_policy(policy).unwrap();
    for invalid in [
        StoreMaintenancePolicy {
            interval_ms: 0,
            ..policy
        },
        StoreMaintenancePolicy {
            backoff_ms: 0,
            ..policy
        },
        StoreMaintenancePolicy {
            max_segments: 0,
            ..policy
        },
        StoreMaintenancePolicy {
            max_pages: 0,
            ..policy
        },
        StoreMaintenancePolicy {
            tail_compaction_interval_ms: 0,
            ..policy
        },
        StoreMaintenancePolicy {
            tail_compaction_backoff_ms: 0,
            ..policy
        },
        StoreMaintenancePolicy {
            tail_compaction_max_pages: 0,
            ..policy
        },
        StoreMaintenancePolicy {
            tail_compaction_max_objects: 0,
            ..policy
        },
        StoreMaintenancePolicy {
            tail_compaction_max_bytes: 0,
            ..policy
        },
    ] {
        let error = store.set_store_maintenance_policy(invalid).unwrap_err();
        assert_eq!(error.code, Code::InvalidArgument);
        assert_eq!(store.store_maintenance_policy().unwrap(), policy);
    }
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    assert_eq!(reopened.store_maintenance_policy().unwrap(), policy);
}

#[test]
fn store_maintenance_report_projects_debt_and_mark_readiness() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared), true).unwrap();
    let keep = store.put(b"live").unwrap();
    store.set_reference_root(Some(keep)).unwrap();
    let status = store.maintenance_status().unwrap();
    assert_eq!(status.candidate_dead_pages, status.reusable_free_pages);
    let default_report = store.store_maintenance_report(100).unwrap();
    assert_eq!(default_report.reason, "mark_epoch_missing");
    assert!(default_report.eligible);
    assert_eq!(default_report.overlay_health.current_generation, 0);
    assert_eq!(default_report.overlay_health.current_record_count, 0);
    assert_eq!(default_report.overlay_health.tombstone_count, 0);
    assert_eq!(default_report.overlay_health.live_checkpoint_references, 0);
    assert_eq!(default_report.overlay_health.reclaimable_overlay_pages, 0);
    assert!(
        default_report
            .overlay_health
            .blocked_reclamation_reasons
            .is_empty()
    );
    assert_eq!(default_report.overlay_health.hot_write_count, 0);
    assert_eq!(
        default_report
            .overlay_health
            .active_writer_contention_indicators,
        0
    );
    assert_eq!(
        default_report.candidate_reclaimable_bytes,
        status
            .candidate_dead_pages
            .saturating_sub(status.reusable_free_pages)
            * PAGE_SIZE
    );
    assert_eq!(
        default_report.reusable_free_bytes,
        status.reusable_free_pages * PAGE_SIZE
    );
    assert_eq!(
        default_report.live_bytes,
        status.physical_bytes
            - default_report.candidate_reclaimable_bytes
            - default_report.reusable_free_bytes
    );
    assert_eq!(default_report.tail_free_pages, status.tail_free_pages);
    assert_eq!(default_report.tail_free_bytes, status.tail_free_bytes);
    assert_eq!(
        default_report.tail_free_bytes,
        default_report.tail_free_pages * PAGE_SIZE
    );
    assert!(!default_report.tail_trim_eligible);
    assert_eq!(
        default_report.tail_compaction_eligible,
        default_report.tail_blocked_by_live_objects
    );
    assert_eq!(
        default_report.full_compaction_required_for_shrink,
        default_report.tail_blocked_by_live_objects && !default_report.tail_compaction_eligible
    );

    store
        .set_store_maintenance_policy(StoreMaintenancePolicy {
            min_candidate_pages: u64::MAX,
            min_reusable_pages: u64::MAX,
            interval_ms: 1_000,
            backoff_ms: 2_000,
            max_segments: 1,
            max_pages: 64,
            full_compaction_enabled: false,
            ..StoreMaintenancePolicy::default()
        })
        .unwrap();
    let enabled = store.store_maintenance_report(100).unwrap();
    assert_eq!(enabled.reason, "mark_epoch_missing");
    assert!(enabled.eligible);
    assert!(enabled.policy.tail_trim_enabled);
    assert!(enabled.policy.tail_compaction_enabled);
}

#[derive(Debug)]
struct FixedMaintenanceClock {
    now_ms: u64,
    instant: std::time::Instant,
    elapsed_ms: AtomicU64,
    monotonic_calls: AtomicU64,
}

impl StoreMaintenanceClock for FixedMaintenanceClock {
    fn now_ms(&self) -> u64 {
        self.now_ms
    }

    fn monotonic_now(&self) -> std::time::Instant {
        let call = self.monotonic_calls.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            return self.instant;
        }
        self.instant
            .checked_add(std::time::Duration::from_millis(
                self.elapsed_ms.load(Ordering::SeqCst),
            ))
            .unwrap()
    }
}

impl FixedMaintenanceClock {
    fn new(now_ms: u64) -> Self {
        Self {
            now_ms,
            instant: std::time::Instant::now(),
            elapsed_ms: AtomicU64::new(0),
            monotonic_calls: AtomicU64::new(0),
        }
    }

    fn with_elapsed(now_ms: u64, elapsed_ms: u64) -> Self {
        let clock = Self::new(now_ms);
        clock.elapsed_ms.store(elapsed_ms, Ordering::SeqCst);
        clock
    }
}

#[test]
fn store_maintenance_executor_uses_injected_clock_and_persists_mark_state() {
    let tp = TempPath::new("maintenance-executor-clock");
    let mut loom = open_loom(tp.path()).unwrap();
    let ns = loom
        .registry_mut()
        .create(
            FacetKind::Files,
            Some("p"),
            WorkspaceId::from_bytes([201; 16]),
        )
        .unwrap();
    for i in 0..8u64 {
        loom.write_file(
            ns,
            &format!("f{i}.txt"),
            format!("v{i}").as_bytes(),
            0o100644,
        )
        .unwrap();
        loom.commit(ns, "nas", "edit", i + 1).unwrap();
    }
    save_loom(&mut loom).unwrap();
    loom.store()
        .set_store_maintenance_policy(StoreMaintenancePolicy {
            min_candidate_pages: 0,
            min_reusable_pages: 0,
            interval_ms: 1_000,
            backoff_ms: 2_000,
            tail_trim_enabled: false,
            tail_compaction_enabled: false,
            ..StoreMaintenancePolicy::default()
        })
        .unwrap();
    let clock = FixedMaintenanceClock {
        now_ms: 5_000,
        instant: std::time::Instant::now(),
        elapsed_ms: AtomicU64::new(0),
        monotonic_calls: AtomicU64::new(0),
    };

    let outcome = run_store_maintenance_once_with_clock(
        &mut loom,
        false,
        None,
        None,
        Some(StoreMaintenanceRunBudget {
            mark_objects: 1,
            max_segments: 1,
            max_pages: 1,
            tail_compaction_max_pages: 1,
            tail_compaction_max_objects: 1,
            tail_compaction_max_bytes: 4096,
            slice_ms: 1_000,
        }),
        None,
        &clock,
    )
    .unwrap();

    assert_eq!(outcome.kind, StoreMaintenanceRunKind::Marked);
    assert_eq!(outcome.visited, Some(1));
    assert!(outcome.pending.unwrap_or(0) > 0);
    let state = loom.store().store_maintenance_run_state().unwrap();
    assert_eq!(state.last_run_ms, Some(5_000));
    assert_eq!(state.next_eligible_ms, 6_000);
    assert_eq!(
        state.last_skip_reason.as_deref(),
        Some("mark_epoch_incomplete")
    );
    assert_eq!(state.last_yield_count, 1);

    drop(loom);
    let reopened = open_loom(tp.path()).unwrap();
    assert_eq!(
        reopened
            .store()
            .store_maintenance_run_state()
            .unwrap()
            .last_run_ms,
        Some(5_000)
    );
}

#[test]
fn store_maintenance_executor_validates_budget_and_honors_cancel() {
    let tp = TempPath::new("maintenance-executor-cancel");
    let mut loom = open_loom(tp.path()).unwrap();
    let err = run_store_maintenance_once_with_clock(
        &mut loom,
        true,
        Some(0),
        None,
        None,
        None,
        &FixedMaintenanceClock {
            now_ms: 10,
            instant: std::time::Instant::now(),
            elapsed_ms: AtomicU64::new(0),
            monotonic_calls: AtomicU64::new(0),
        },
    )
    .unwrap_err();
    assert_eq!(err.code, Code::InvalidArgument);
    assert_eq!(err.message, "max-segments must be nonzero");

    let cancel = AtomicBool::new(true);
    let outcome = run_store_maintenance_once_with_clock(
        &mut loom,
        false,
        None,
        None,
        Some(StoreMaintenanceRunBudget::daemon_automatic()),
        Some(&cancel),
        &FixedMaintenanceClock {
            now_ms: 20,
            instant: std::time::Instant::now(),
            elapsed_ms: AtomicU64::new(0),
            monotonic_calls: AtomicU64::new(0),
        },
    )
    .unwrap();
    assert_eq!(outcome.kind, StoreMaintenanceRunKind::Skipped);
    assert_eq!(outcome.reason.as_deref(), Some("shutdown_cancelled"));
    assert_eq!(
        loom.store()
            .store_maintenance_run_state()
            .unwrap()
            .last_skip_reason
            .as_deref(),
        Some("never_run")
    );
}

#[test]
fn store_maintenance_executor_fake_deadline_controls_overrun_and_elapsed() {
    let tp = TempPath::new("maintenance-executor-deadline");
    let mut loom = open_loom(tp.path()).unwrap();
    let ns = loom
        .registry_mut()
        .create(
            FacetKind::Files,
            Some("p"),
            WorkspaceId::from_bytes([202; 16]),
        )
        .unwrap();
    loom.write_file(ns, "f.txt", b"v", 0o100644).unwrap();
    loom.commit(ns, "nas", "edit", 1).unwrap();
    save_loom(&mut loom).unwrap();
    loom.store()
        .set_store_maintenance_policy(StoreMaintenancePolicy {
            min_candidate_pages: 0,
            min_reusable_pages: 0,
            interval_ms: 100,
            backoff_ms: 200,
            tail_trim_enabled: false,
            tail_compaction_enabled: false,
            ..StoreMaintenancePolicy::default()
        })
        .unwrap();

    let outcome = run_store_maintenance_once_with_clock(
        &mut loom,
        false,
        None,
        None,
        Some(StoreMaintenanceRunBudget {
            mark_objects: 8,
            max_segments: 1,
            max_pages: 1,
            tail_compaction_max_pages: 1,
            tail_compaction_max_objects: 1,
            tail_compaction_max_bytes: 4096,
            slice_ms: 1,
        }),
        None,
        &FixedMaintenanceClock::with_elapsed(9_000, 2),
    )
    .unwrap();

    assert_eq!(outcome.kind, StoreMaintenanceRunKind::Marked);
    assert_eq!(outcome.visited, Some(0));
    let state = loom.store().store_maintenance_run_state().unwrap();
    assert_eq!(state.last_run_ms, Some(9_000));
    assert_eq!(state.last_overrun_count, 1);
    assert_eq!(state.last_yield_count, 1);
}

#[test]
fn store_maintenance_executor_completes_mark_and_reclaims_with_budget_caps() {
    let tp = TempPath::new("maintenance-executor-reclaim");
    let mut loom = open_loom(tp.path()).unwrap();
    let ns = loom
        .registry_mut()
        .create(
            FacetKind::Files,
            Some("p"),
            WorkspaceId::from_bytes([203; 16]),
        )
        .unwrap();
    for i in 0..6u64 {
        loom.write_file(ns, &format!("f{i}.txt"), b"v", 0o100644)
            .unwrap();
        loom.commit(ns, "nas", "edit", i + 1).unwrap();
    }
    save_loom(&mut loom).unwrap();
    loom.store()
        .set_store_maintenance_policy(StoreMaintenancePolicy {
            min_candidate_pages: 0,
            min_reusable_pages: 0,
            interval_ms: 100,
            backoff_ms: 200,
            max_segments: 64,
            max_pages: 64,
            tail_trim_enabled: false,
            tail_compaction_enabled: false,
            ..StoreMaintenancePolicy::default()
        })
        .unwrap();

    let mut outcome = run_store_maintenance_once_with_clock(
        &mut loom,
        false,
        None,
        None,
        Some(StoreMaintenanceRunBudget {
            mark_objects: 64,
            max_segments: 1,
            max_pages: 1,
            tail_compaction_max_pages: 1,
            tail_compaction_max_objects: 1,
            tail_compaction_max_bytes: 4096,
            slice_ms: 1_000,
        }),
        None,
        &FixedMaintenanceClock::new(10_000),
    )
    .unwrap();
    for i in 0..8u64 {
        if outcome.kind == StoreMaintenanceRunKind::Reclaimed {
            break;
        }
        outcome = run_store_maintenance_once_with_clock(
            &mut loom,
            false,
            None,
            None,
            Some(StoreMaintenanceRunBudget {
                mark_objects: 64,
                max_segments: 1,
                max_pages: 1,
                tail_compaction_max_pages: 1,
                tail_compaction_max_objects: 1,
                tail_compaction_max_bytes: 4096,
                slice_ms: 1_000,
            }),
            None,
            &FixedMaintenanceClock::new(10_100 + i),
        )
        .unwrap();
    }

    assert_eq!(outcome.kind, StoreMaintenanceRunKind::Reclaimed);
    assert!(outcome.segments_reclaimed.unwrap_or(0) <= 1);
    assert!(outcome.pages_freed.unwrap_or(0) <= 1);
    assert_eq!(
        loom.store()
            .active_reachability_mark_epoch()
            .unwrap()
            .unwrap()
            .state
            .completed,
        true
    );
    assert!(
        loom.store()
            .store_maintenance_run_state()
            .unwrap()
            .last_run_ms
            >= Some(10_000)
    );
}

#[test]
fn store_maintenance_executor_manual_compaction_returns_typed_outcome() {
    let tp = TempPath::new("maintenance-executor-compact");
    let mut loom = open_loom(tp.path()).unwrap();
    let ns = loom
        .registry_mut()
        .create(
            FacetKind::Files,
            Some("p"),
            WorkspaceId::from_bytes([204; 16]),
        )
        .unwrap();
    for i in 0..4u64 {
        loom.write_file(ns, &format!("f{i}.txt"), b"v", 0o100644)
            .unwrap();
        loom.commit(ns, "nas", "edit", i + 1).unwrap();
    }
    save_loom(&mut loom).unwrap();
    loom.store()
        .set_store_maintenance_policy(StoreMaintenancePolicy {
            min_candidate_pages: 0,
            min_reusable_pages: 0,
            interval_ms: 100,
            backoff_ms: 200,
            full_compaction_enabled: true,
            tail_trim_enabled: false,
            tail_compaction_enabled: false,
            ..StoreMaintenancePolicy::default()
        })
        .unwrap();

    let outcome = run_store_maintenance_once_with_clock(
        &mut loom,
        true,
        None,
        None,
        None,
        None,
        &FixedMaintenanceClock::new(11_000),
    )
    .unwrap();

    assert_eq!(outcome.kind, StoreMaintenanceRunKind::Compacted);
    assert!(outcome.before.unwrap_or(0) >= outcome.after.unwrap_or(0));
    assert_eq!(
        loom.store()
            .store_maintenance_run_state()
            .unwrap()
            .last_run_ms,
        Some(11_000)
    );
}

#[test]
fn mutable_overlay_entries_survive_file_store_reopen() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let key = OverlayKey::from_segments([
        b"workspace",
        &[1; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"MX-388",
    ])
    .unwrap();
    store
        .put_mutable_overlay_value(key.clone(), b"current".to_vec())
        .unwrap();
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let snapshot = reopened.mutable_overlay_snapshot().unwrap();
    let read = snapshot
        .read_composite(&key, |_| Ok(Some(b"base".to_vec())))
        .unwrap();
    let report = reopened.store_maintenance_report(100).unwrap();

    assert_eq!(read.as_deref(), Some(&b"current"[..]));
    assert_eq!(report.overlay_health.current_generation, 1);
    assert_eq!(report.overlay_health.current_record_count, 1);
    assert_eq!(report.overlay_health.hot_write_count, 1);
    assert_eq!(report.overlay_obsolete_record_count, 0);
    assert_eq!(report.growth_domains[0].domain, "tickets");
    assert_eq!(report.growth_domains[0].current_records, 1);
    assert_eq!(report.growth_domains[0].payload_bytes, 7);
}

#[test]
fn canonical_current_record_write_publishes_direct_root() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let key = OverlayKey::from_segments([
        b"workspace",
        &[2; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"MX-389",
    ])
    .unwrap();
    let mut overlay = loom_core::MutableOverlay::new();
    overlay
        .put_value(key.clone(), None, b"direct-current".to_vec())
        .unwrap();
    let latest = overlay.current_entry(&key).unwrap();
    let record = (
        mutable_overlay_entry_address(&key),
        encode_mutable_overlay_entry(&latest),
    );

    store
        .commit_current_root_records_for_test(&[record])
        .unwrap();
    let (region_table_root, page_count, current_record_root, overlay_root, root_catalog_root) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.region_table_root.unwrap(),
            inner.page_count,
            inner.current_record_root,
            inner.overlay_root,
            inner.root_catalog_root,
        )
    };
    let mut backing = shared.clone();
    let region = read_region_table(&mut backing, region_table_root, page_count).unwrap();

    assert_eq!(region.current_record_root, current_record_root);
    assert!(current_record_root.is_some());
    assert_eq!(region.overlay_root, None);
    assert_eq!(overlay_root, None);
    assert_eq!(region.root_catalog_root, None);
    assert_eq!(root_catalog_root, None);
    assert_eq!(
        store
            .mutable_overlay_record_payload(&mutable_overlay_current_root_address())
            .unwrap(),
        None
    );
}

#[test]
fn mu15b_smoke_e_legacy_only_publication_retains_legacy_root() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let key = durability_test_key("mu15b-smoke-e-legacy-only");
    let token = loom_core::OverlayOwnerToken::from_bytes([0xe1; 32]);
    store
        .commit_raw_overlay_records_for_test(&[(
            mutable_overlay_owner_token_address(&key),
            encode_mutable_overlay_owner_token_record(&token),
        )])
        .unwrap();

    let (region_table_root, page_count, overlay_root) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.region_table_root.unwrap(),
            inner.page_count,
            inner.overlay_root,
        )
    };
    let mut backing = shared.clone();
    let region = read_region_table(&mut backing, region_table_root, page_count).unwrap();
    assert!(overlay_root.is_some());
    assert_eq!(region.overlay_root, overlay_root);
    assert_eq!(region.current_record_root, None);
    assert_eq!(region.root_catalog_root, None);

    drop(store);
    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    assert!(reopened.inner.lock().unwrap().overlay_root.is_some());
    assert_eq!(
        reopened
            .mutable_overlay_owner_token_record(&mutable_overlay_owner_token_address(&key))
            .unwrap(),
        Some(token)
    );
}

#[test]
fn mu15b_smoke_e_canonical_publication_suppresses_stale_legacy_root() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let legacy_key = durability_test_key("mu15b-smoke-e-stale-legacy");
    store
        .commit_raw_overlay_records_for_test(&[(
            mutable_overlay_owner_token_address(&legacy_key),
            encode_mutable_overlay_owner_token_record(&loom_core::OverlayOwnerToken::from_bytes(
                [0xe2; 32],
            )),
        )])
        .unwrap();
    assert!(store.inner.lock().unwrap().overlay_root.is_some());

    let current_key = durability_test_key("mu15b-smoke-e-current");
    let mut overlay = loom_core::MutableOverlay::new();
    overlay
        .put_value(current_key.clone(), None, b"canonical".to_vec())
        .unwrap();
    let latest = overlay.current_entry(&current_key).unwrap();
    store
        .commit_current_root_records_for_test(&[(
            mutable_overlay_entry_address(&current_key),
            encode_mutable_overlay_entry(&latest),
        )])
        .unwrap();

    let (region_table_root, page_count, current_record_root, overlay_root) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.region_table_root.unwrap(),
            inner.page_count,
            inner.current_record_root,
            inner.overlay_root,
        )
    };
    let mut backing = shared.clone();
    let region = read_region_table(&mut backing, region_table_root, page_count).unwrap();
    assert!(current_record_root.is_some());
    assert_eq!(region.current_record_root, current_record_root);
    assert_eq!(overlay_root, None);
    assert_eq!(region.overlay_root, None);

    drop(store);
    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    assert_eq!(reopened.inner.lock().unwrap().overlay_root, None);
    assert_eq!(
        reopened
            .mutable_overlay_snapshot()
            .unwrap()
            .read_composite(&current_key, |_| Ok(None))
            .unwrap(),
        Some(b"canonical".to_vec())
    );
}

#[test]
fn mu15b_smoke_e_explicit_mixed_root_transaction_remains_rejected() {
    assert!(
        t188_18b_finish_with_root_inputs(TxnRootInputs {
            object_index: None,
            legacy_overlay: Some(PageId(2)),
            current_records: Some(PageId(3)),
            root_catalog: TxnRootCatalog {
                root: None,
                entries: Vec::new(),
            },
            reference: None,
            control: None,
            previous_mutable_overlay_generation_floor: 0,
            mutable_overlay_generation_floor: 0,
        })
        .unwrap_err()
        .message
        .contains("legacy overlay cannot publish with canonical mutable roots")
    );
}

#[test]
fn canonical_current_record_open_hydrates_without_overlay_or_catalog() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let key = OverlayKey::from_segments([
        b"workspace",
        &[3; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"MX-390",
    ])
    .unwrap();
    let mut overlay = loom_core::MutableOverlay::new();
    overlay
        .put_value(key.clone(), None, b"reopened-direct".to_vec())
        .unwrap();
    let latest = overlay.current_entry(&key).unwrap();
    store
        .commit_current_root_records_for_test(&[(
            mutable_overlay_entry_address(&key),
            encode_mutable_overlay_entry(&latest),
        )])
        .unwrap();
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let (current_record_root, overlay_root, root_catalog_root, used_current_root) = {
        let inner = reopened.inner.lock().unwrap();
        (
            inner.current_record_root,
            inner.overlay_root,
            inner.root_catalog_root,
            inner.io_stats.open_mutable_used_current_root,
        )
    };
    let snapshot = reopened.mutable_overlay_snapshot().unwrap();
    let read = snapshot
        .read_composite(&key, |_| Ok(Some(b"base".to_vec())))
        .unwrap();

    assert!(current_record_root.is_some());
    assert_eq!(overlay_root, None);
    assert_eq!(root_catalog_root, None);
    assert!(used_current_root);
    assert_eq!(read.as_deref(), Some(&b"reopened-direct"[..]));
}

#[test]
fn mvcc_snapshot_identity_pins_generation_and_base_root() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared), true).unwrap();
    let first_root = store.put(b"first-root").unwrap();
    let second_root = store.put(b"second-root").unwrap();
    let key = OverlayKey::from_segments([
        b"workspace",
        &[43; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"MX-442",
    ])
    .unwrap();

    store.set_reference_root(Some(first_root)).unwrap();
    store
        .put_mutable_overlay_value(key.clone(), b"current-1".to_vec())
        .unwrap();
    let snapshot = store.open_mvcc_snapshot_with_owner(Some("mx-442")).unwrap();
    store.set_reference_root(Some(second_root)).unwrap();
    store
        .put_mutable_overlay_value(key.clone(), b"current-2".to_vec())
        .unwrap();

    assert_eq!(snapshot.overlay_generation().as_u64(), 1);
    assert_eq!(snapshot.immutable_base_root(), Some(first_root));
    assert_eq!(
        snapshot
            .read_composite(&key, |base_root, _| {
                assert_eq!(base_root, Some(first_root));
                Ok(Some(b"base".to_vec()))
            })
            .unwrap()
            .as_deref(),
        Some(&b"current-1"[..])
    );
    assert_eq!(
        store
            .mutable_overlay_snapshot()
            .unwrap()
            .read_composite(&key, |_| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"current-2"[..])
    );

    let diagnostics = store.mvcc_snapshot_diagnostics().unwrap();
    assert_eq!(diagnostics.active_snapshot_count, 1);
    assert_eq!(
        diagnostics.oldest_pinned_overlay_generation,
        Some(snapshot.overlay_generation())
    );
    assert_eq!(diagnostics.pins[0].pin_id, snapshot.pin_id());
    assert_eq!(diagnostics.pins[0].identity, snapshot.identity());
    assert_eq!(diagnostics.pins[0].owner.as_deref(), Some("mx-442"));

    assert!(snapshot.release().unwrap());
    assert!(!snapshot.release().unwrap());
    assert!(snapshot.is_released());
    assert_eq!(
        store
            .mvcc_snapshot_diagnostics()
            .unwrap()
            .active_snapshot_count,
        0
    );
}

#[test]
fn store_maintenance_report_surfaces_mvcc_snapshot_diagnostics() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared), true).unwrap();
    // No pins: the maintenance report shows zero active MVCC snapshots.
    let report = store.store_maintenance_report(0).unwrap();
    assert_eq!(report.mvcc_snapshots.active_snapshot_count, 0);
    assert_eq!(report.mvcc_snapshots.oldest_pinned_overlay_generation, None);
    // An open snapshot is surfaced through the same maintenance report path.
    let snapshot = store.open_mvcc_snapshot().unwrap();
    let report = store.store_maintenance_report(0).unwrap();
    assert_eq!(report.mvcc_snapshots.active_snapshot_count, 1);
    assert_eq!(
        report.mvcc_snapshots.oldest_pinned_overlay_generation,
        Some(snapshot.overlay_generation())
    );
    assert_eq!(report.mvcc_snapshots.pins.len(), 1);
    // Releasing the pin clears it from the report.
    assert!(snapshot.release().unwrap());
    let report = store.store_maintenance_report(0).unwrap();
    assert_eq!(report.mvcc_snapshots.active_snapshot_count, 0);
}

#[test]
fn mvcc_snapshot_lifetime_releases_pins_and_tracks_oldest_generation() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared), true).unwrap();
    let key = OverlayKey::from_segments([
        b"workspace",
        &[44; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"MX-442",
    ])
    .unwrap();

    store
        .put_mutable_overlay_value(key.clone(), b"current-1".to_vec())
        .unwrap();
    let first = store.open_mvcc_snapshot().unwrap();
    store.put_mutable_overlay_tombstone(key.clone()).unwrap();
    let second = store.open_mvcc_snapshot().unwrap();

    assert_eq!(
        first
            .read_composite(&key, |_, _| Ok(Some(b"base".to_vec())))
            .unwrap()
            .as_deref(),
        Some(&b"current-1"[..])
    );
    assert_eq!(
        second
            .read_composite(&key, |_, _| Ok(Some(b"base".to_vec())))
            .unwrap(),
        None
    );
    assert_eq!(
        store.oldest_pinned_mvcc_snapshot_generation().unwrap(),
        Some(first.overlay_generation())
    );

    assert!(first.release().unwrap());
    assert_eq!(
        store.oldest_pinned_mvcc_snapshot_generation().unwrap(),
        Some(second.overlay_generation())
    );
    drop(second);
    assert_eq!(
        store.oldest_pinned_mvcc_snapshot_generation().unwrap(),
        None
    );
}

#[test]
fn mvcc_snapshot_read_path_stays_stable_while_writer_publishes() {
    let shared = SharedMem::default();
    let store = Arc::new(FileStore::with_backing(Box::new(shared), true).unwrap());
    let key = OverlayKey::from_segments([
        b"workspace",
        &[45; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"MX-443",
    ])
    .unwrap();
    store
        .put_mutable_overlay_value(key.clone(), b"current-0".to_vec())
        .unwrap();
    let snapshot = store
        .open_mvcc_snapshot_with_owner(Some("mx-443.concurrent-read"))
        .unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let writer_store = Arc::clone(&store);
    let writer_key = key.clone();
    let writer_barrier = Arc::clone(&barrier);
    let writer = std::thread::spawn(move || {
        writer_barrier.wait();
        for update in 1..=32u64 {
            writer_store
                .put_mutable_overlay_value(
                    writer_key.clone(),
                    format!("current-{update}").into_bytes(),
                )
                .unwrap();
        }
    });

    barrier.wait();
    writer.join().unwrap();

    assert_eq!(snapshot.overlay_generation().as_u64(), 1);
    assert_eq!(
        snapshot
            .read_composite(&key, |_, _| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"current-0"[..])
    );
    assert_eq!(
        store
            .mutable_overlay_snapshot()
            .unwrap()
            .read_composite(&key, |_| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"current-32"[..])
    );
    assert_eq!(
        store.oldest_pinned_mvcc_snapshot_generation().unwrap(),
        Some(snapshot.overlay_generation())
    );
    snapshot.release().unwrap();
}

#[test]
fn mutable_overlay_owner_token_index_survives_reopen() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let key = OverlayKey::from_segments([
        b"workspace",
        &[41; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"MX-429",
    ])
    .unwrap();
    let first = store
        .put_mutable_overlay_value(key.clone(), b"current".to_vec())
        .unwrap();
    let second = store
        .put_mutable_overlay_value(key.clone(), b"current-2".to_vec())
        .unwrap();

    assert_ne!(first.as_bytes(), second.as_bytes());
    assert_eq!(
        store
            .mutable_overlay_durable_owner_token(&key)
            .unwrap()
            .as_ref()
            .map(|token| token.as_bytes()),
        Some(second.as_bytes())
    );
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();

    assert_eq!(
        reopened
            .mutable_overlay_durable_owner_token(&key)
            .unwrap()
            .as_ref()
            .map(|token| token.as_bytes()),
        Some(second.as_bytes())
    );
    assert_eq!(
        reopened
            .mutable_overlay_snapshot()
            .unwrap()
            .owner_token(&key)
            .unwrap()
            .as_ref()
            .map(|token| token.as_bytes()),
        Some(second.as_bytes())
    );
}

#[test]
fn mutable_overlay_idempotency_key_retries_survive_reopen() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let key = OverlayKey::from_segments([
        b"workspace",
        &[42; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"MX-429",
    ])
    .unwrap();
    let first = store
        .put_mutable_overlay_value_idempotent(key.clone(), b"current".to_vec(), "retry-1")
        .unwrap();
    let retry = store
        .put_mutable_overlay_value_idempotent(key.clone(), b"current".to_vec(), "retry-1")
        .unwrap();

    assert_eq!(first.as_bytes(), retry.as_bytes());
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let reopened_retry = reopened
        .put_mutable_overlay_value_idempotent(key.clone(), b"current".to_vec(), "retry-1")
        .unwrap();
    let conflict = reopened
        .put_mutable_overlay_value_idempotent(key, b"different".to_vec(), "retry-1")
        .unwrap_err();

    assert_eq!(reopened_retry.as_bytes(), first.as_bytes());
    assert_eq!(conflict.code, Code::Conflict);
}

#[test]
fn concurrent_idempotency_key_with_different_payload_serializes_conflict() {
    let shared = SharedMem::default();
    let store = Arc::new(FileStore::with_backing(Box::new(shared), true).unwrap());
    let barrier = Arc::new(std::sync::Barrier::new(16));
    let mut threads = Vec::new();
    for worker in 0..16u8 {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        threads.push(std::thread::spawn(move || {
            let key = OverlayKey::from_segments([
                b"workspace",
                &[44; 16],
                b"tickets",
                b"matrix",
                b"ticket",
                b"MX-429",
            ])
            .unwrap();
            let payload = if worker == 0 {
                b"winner".to_vec()
            } else {
                format!("loser-{worker}").into_bytes()
            };
            barrier.wait();
            store.put_mutable_overlay_value_idempotent(key, payload, "race-key")
        }));
    }

    let mut successes = 0;
    let mut conflicts = 0;
    for thread in threads {
        match thread.join().unwrap() {
            Ok(_) => successes += 1,
            Err(error) if error.code == Code::Conflict => conflicts += 1,
            Err(error) => panic!("unexpected error: {error:?}"),
        }
    }

    assert_eq!(successes, 1);
    assert_eq!(conflicts, 15);
}

#[test]
fn concurrent_idempotent_regular_and_tombstone_writes_keep_owner_index_consistent() {
    let shared = SharedMem::default();
    let store = Arc::new(FileStore::with_backing(Box::new(shared), true).unwrap());
    let key = OverlayKey::from_segments([
        b"workspace",
        &[45; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"MX-429",
    ])
    .unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(18));
    let mut threads = Vec::new();
    for worker in 0..18u8 {
        let store = Arc::clone(&store);
        let key = key.clone();
        let barrier = Arc::clone(&barrier);
        threads.push(std::thread::spawn(move || {
            barrier.wait();
            let result = match worker % 3 {
                0 => store.put_mutable_overlay_value_idempotent(
                    key,
                    b"idempotent".to_vec(),
                    "mixed-race-key",
                ),
                1 => store.put_mutable_overlay_value(key, format!("regular-{worker}").into_bytes()),
                _ => store.put_mutable_overlay_tombstone(key),
            };
            (worker, result)
        }));
    }

    let mut idempotent_token: Option<loom_core::OverlayOwnerToken> = None;
    for thread in threads {
        let (worker, result) = thread.join().unwrap();
        let token = result.unwrap();
        if worker % 3 == 0 {
            if let Some(expected) = &idempotent_token {
                assert_eq!(token.as_bytes(), expected.as_bytes());
            } else {
                idempotent_token = Some(token);
            }
        }
    }

    let durable = store
        .mutable_overlay_durable_owner_token(&key)
        .unwrap()
        .unwrap();
    let current = store
        .mutable_overlay_snapshot()
        .unwrap()
        .owner_token(&key)
        .unwrap()
        .unwrap();
    let retry = store
        .put_mutable_overlay_value_idempotent(key.clone(), b"idempotent".to_vec(), "mixed-race-key")
        .unwrap();
    let conflict = store
        .put_mutable_overlay_value_idempotent(key, b"different".to_vec(), "mixed-race-key")
        .unwrap_err();

    assert_eq!(durable.as_bytes(), current.as_bytes());
    assert_eq!(retry.as_bytes(), idempotent_token.unwrap().as_bytes());
    assert_eq!(conflict.code, Code::Conflict);
}

#[test]
fn malformed_durable_owner_token_index_is_rejected() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared), true).unwrap();
    let key = OverlayKey::from_segments([
        b"workspace",
        &[43; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"MX-429",
    ])
    .unwrap();
    store
        .commit_family_root_records_for_test(
            OWNER_TOKEN_FAMILY_ID,
            &[(mutable_overlay_owner_token_address(&key), b"bad".to_vec())],
        )
        .unwrap();

    let error = store.mutable_overlay_durable_owner_token(&key).unwrap_err();

    assert_eq!(error.code, Code::CorruptObject);
}

#[test]
fn owner_token_routes_through_catalog_family_root() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let key = OverlayKey::from_segments([
        b"workspace",
        &[44; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"MX-430",
    ])
    .unwrap();
    let token = loom_core::OverlayOwnerToken::from_bytes([68; 32]);

    store
        .commit_family_root_records_for_test(
            OWNER_TOKEN_FAMILY_ID,
            &[(
                mutable_overlay_owner_token_address(&key),
                encode_mutable_overlay_owner_token_record(&token),
            )],
        )
        .unwrap();
    let (
        region_table_root,
        page_count,
        root_catalog_root,
        owner_token_root,
        overlay_root,
        current_record_root,
        retained_history_root,
    ) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.region_table_root.unwrap(),
            inner.page_count,
            inner.root_catalog_root,
            inner.owner_token_root,
            inner.overlay_root,
            inner.current_record_root,
            inner.retained_history_root,
        )
    };
    let mut backing = shared.clone();
    let region = read_region_table(&mut backing, region_table_root, page_count).unwrap();
    let catalog = read_root_catalog(&mut backing, root_catalog_root.unwrap(), page_count).unwrap();

    assert_eq!(region.root_catalog_root, root_catalog_root);
    assert_eq!(region.overlay_root, None);
    assert_eq!(overlay_root, None);
    assert_eq!(current_record_root, None);
    assert_eq!(retained_history_root, None);
    assert_eq!(
        catalog
            .entries
            .iter()
            .find(|entry| entry.family_id == OWNER_TOKEN_FAMILY_ID)
            .map(|entry| entry.root),
        owner_token_root
    );
    assert!(owner_token_root.is_some());
    assert_eq!(
        store
            .mutable_overlay_record_payload(&mutable_overlay_owner_token_address(&key))
            .unwrap(),
        None
    );
    assert_eq!(
        store
            .mutable_overlay_durable_owner_token(&key)
            .unwrap()
            .as_ref()
            .map(|token| token.as_bytes()),
        Some(token.as_bytes())
    );
}

#[test]
fn owner_token_catalog_family_survives_reopen_without_current_hydration() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let key = OverlayKey::from_segments([
        b"workspace",
        &[45; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"MX-431",
    ])
    .unwrap();
    let token = loom_core::OverlayOwnerToken::from_bytes([69; 32]);
    store
        .commit_family_root_records_for_test(
            OWNER_TOKEN_FAMILY_ID,
            &[(
                mutable_overlay_owner_token_address(&key),
                encode_mutable_overlay_owner_token_record(&token),
            )],
        )
        .unwrap();
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let stats = reopened.io_stats().unwrap();
    let (owner_token_root, root_catalog_root, overlay_root, current_record_root) = {
        let inner = reopened.inner.lock().unwrap();
        (
            inner.owner_token_root,
            inner.root_catalog_root,
            inner.overlay_root,
            inner.current_record_root,
        )
    };

    assert!(owner_token_root.is_some());
    assert!(root_catalog_root.is_some());
    assert_eq!(overlay_root, None);
    assert_eq!(current_record_root, None);
    assert_eq!(stats.open_mutable_current_records_loaded, 0);
    assert_eq!(stats.open_mutable_control_records_skipped, 0);
    assert_eq!(
        reopened
            .mutable_overlay_durable_owner_token(&key)
            .unwrap()
            .as_ref()
            .map(|token| token.as_bytes()),
        Some(token.as_bytes())
    );
}

#[test]
fn absent_owner_token_catalog_family_reads_empty() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let key = OverlayKey::from_segments([
        b"workspace",
        &[46; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"MX-432",
    ])
    .unwrap();

    assert!(
        store
            .mutable_overlay_durable_owner_token(&key)
            .unwrap()
            .is_none()
    );
}

#[test]
fn owner_token_family_root_does_not_fall_back_to_stale_legacy_overlay() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let canonical_key = OverlayKey::from_segments([
        b"workspace",
        &[47; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"MX-433",
    ])
    .unwrap();
    let stale_legacy_key = OverlayKey::from_segments([
        b"workspace",
        &[48; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"MX-434",
    ])
    .unwrap();
    let canonical_token = loom_core::OverlayOwnerToken::from_bytes([70; 32]);
    let stale_token = loom_core::OverlayOwnerToken::from_bytes([71; 32]);
    store
        .commit_raw_overlay_records_for_test(&[(
            mutable_overlay_owner_token_address(&stale_legacy_key),
            encode_mutable_overlay_owner_token_record(&stale_token),
        )])
        .unwrap();
    let stale_overlay_root = store.inner.lock().unwrap().overlay_root;
    store.inner.lock().unwrap().overlay_root = None;
    store
        .commit_family_root_records_for_test(
            OWNER_TOKEN_FAMILY_ID,
            &[(
                mutable_overlay_owner_token_address(&canonical_key),
                encode_mutable_overlay_owner_token_record(&canonical_token),
            )],
        )
        .unwrap();
    store.inner.lock().unwrap().overlay_root = stale_overlay_root;

    let (owner_token_root, overlay_root) = {
        let inner = store.inner.lock().unwrap();
        (inner.owner_token_root, inner.overlay_root)
    };

    assert!(owner_token_root.is_some());
    assert!(overlay_root.is_some());
    assert_eq!(
        store
            .mutable_overlay_record_payload(&mutable_overlay_owner_token_address(&stale_legacy_key))
            .unwrap()
            .map(|bytes| decode_mutable_overlay_owner_token_record(&bytes).unwrap())
            .as_ref()
            .map(|token| token.as_bytes()),
        Some(stale_token.as_bytes())
    );
    assert!(
        store
            .mutable_overlay_durable_owner_token(&stale_legacy_key)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .mutable_overlay_durable_owner_token(&canonical_key)
            .unwrap()
            .as_ref()
            .map(|token| token.as_bytes()),
        Some(canonical_token.as_bytes())
    );
}

#[test]
fn owner_token_mixed_root_set_publication_fails_closed() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let canonical_key = OverlayKey::from_segments([
        b"workspace",
        &[49; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"MX-435",
    ])
    .unwrap();
    let legacy_key = OverlayKey::from_segments([
        b"workspace",
        &[50; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"MX-436",
    ])
    .unwrap();
    let canonical_token = loom_core::OverlayOwnerToken::from_bytes([72; 32]);
    let legacy_token = loom_core::OverlayOwnerToken::from_bytes([73; 32]);
    store
        .commit_family_root_records_for_test(
            OWNER_TOKEN_FAMILY_ID,
            &[(
                mutable_overlay_owner_token_address(&canonical_key),
                encode_mutable_overlay_owner_token_record(&canonical_token),
            )],
        )
        .unwrap();

    let error = store
        .commit_raw_overlay_records_for_test(&[(
            mutable_overlay_owner_token_address(&legacy_key),
            encode_mutable_overlay_owner_token_record(&legacy_token),
        )])
        .unwrap_err();

    assert_eq!(error.code, Code::CorruptObject);
    assert_eq!(
        store
            .mutable_overlay_durable_owner_token(&canonical_key)
            .unwrap()
            .as_ref()
            .map(|token| token.as_bytes()),
        Some(canonical_token.as_bytes())
    );
    assert!(
        store
            .mutable_overlay_durable_owner_token(&legacy_key)
            .unwrap()
            .is_none()
    );
}

fn secondary_index_record(
    generation: u64,
    index: OverlayKey,
    op: SecondaryIndexWriteOp,
) -> Vec<u8> {
    encode_mutable_overlay_secondary_index_record(
        OverlayGeneration::new(generation),
        &SecondaryIndexWrite { index, op },
    )
}

#[test]
fn secondary_index_routes_through_catalog_family_root() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let index = OverlayKey::from_segments([
        b"workspace",
        &[51; 16],
        b"tickets",
        b"matrix",
        b"index",
        b"open",
    ])
    .unwrap();
    let payload = b"ticket/MX-440".to_vec();

    store
        .commit_family_root_records_for_test(
            SECONDARY_INDEX_FAMILY_ID,
            &[(
                mutable_overlay_secondary_index_address(&index),
                secondary_index_record(
                    1,
                    index.clone(),
                    SecondaryIndexWriteOp::Put {
                        payload: payload.clone(),
                    },
                ),
            )],
        )
        .unwrap();
    let (
        region_table_root,
        page_count,
        root_catalog_root,
        secondary_index_root,
        owner_token_root,
        retained_history_root,
        overlay_root,
        current_record_root,
    ) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.region_table_root.unwrap(),
            inner.page_count,
            inner.root_catalog_root,
            inner.secondary_index_root,
            inner.owner_token_root,
            inner.retained_history_root,
            inner.overlay_root,
            inner.current_record_root,
        )
    };
    let mut backing = shared.clone();
    let region = read_region_table(&mut backing, region_table_root, page_count).unwrap();
    let catalog = read_root_catalog(&mut backing, root_catalog_root.unwrap(), page_count).unwrap();

    assert_eq!(region.root_catalog_root, root_catalog_root);
    assert_eq!(region.overlay_root, None);
    assert_eq!(overlay_root, None);
    assert_eq!(current_record_root, None);
    assert_eq!(owner_token_root, None);
    assert_eq!(retained_history_root, None);
    assert_eq!(
        catalog
            .entries
            .iter()
            .find(|entry| entry.family_id == SECONDARY_INDEX_FAMILY_ID)
            .map(|entry| entry.root),
        secondary_index_root
    );
    assert!(secondary_index_root.is_some());
    assert_eq!(
        store
            .mutable_overlay_record_payload(&mutable_overlay_secondary_index_address(&index))
            .unwrap(),
        None
    );
    assert_eq!(
        store.mutable_overlay_secondary_index_value(&index).unwrap(),
        Some(payload)
    );
}

#[test]
fn secondary_index_catalog_family_survives_reopen_without_current_hydration() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let index = OverlayKey::from_segments([
        b"workspace",
        &[52; 16],
        b"tickets",
        b"matrix",
        b"index",
        b"review",
    ])
    .unwrap();
    let payload = b"ticket/MX-441".to_vec();
    store
        .commit_family_root_records_for_test(
            SECONDARY_INDEX_FAMILY_ID,
            &[(
                mutable_overlay_secondary_index_address(&index),
                secondary_index_record(
                    2,
                    index.clone(),
                    SecondaryIndexWriteOp::Put {
                        payload: payload.clone(),
                    },
                ),
            )],
        )
        .unwrap();
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let stats = reopened.io_stats().unwrap();
    let (secondary_index_root, root_catalog_root, overlay_root, current_record_root) = {
        let inner = reopened.inner.lock().unwrap();
        (
            inner.secondary_index_root,
            inner.root_catalog_root,
            inner.overlay_root,
            inner.current_record_root,
        )
    };

    assert!(secondary_index_root.is_some());
    assert!(root_catalog_root.is_some());
    assert_eq!(overlay_root, None);
    assert_eq!(current_record_root, None);
    assert_eq!(stats.open_mutable_current_records_loaded, 0);
    assert_eq!(stats.open_mutable_control_records_skipped, 0);
    assert_eq!(
        reopened
            .mutable_overlay_secondary_index_value(&index)
            .unwrap(),
        Some(payload)
    );
}

#[test]
fn absent_secondary_index_catalog_family_reads_empty() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let index = OverlayKey::from_segments([
        b"workspace",
        &[53; 16],
        b"tickets",
        b"matrix",
        b"index",
        b"absent",
    ])
    .unwrap();

    assert_eq!(
        store.mutable_overlay_secondary_index_value(&index).unwrap(),
        None
    );
}

#[test]
fn secondary_index_family_root_does_not_fall_back_to_stale_legacy_overlay() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let canonical_index = OverlayKey::from_segments([
        b"workspace",
        &[54; 16],
        b"tickets",
        b"matrix",
        b"index",
        b"canonical",
    ])
    .unwrap();
    let stale_legacy_index = OverlayKey::from_segments([
        b"workspace",
        &[55; 16],
        b"tickets",
        b"matrix",
        b"index",
        b"legacy",
    ])
    .unwrap();
    let canonical_payload = b"ticket/MX-442".to_vec();
    let stale_payload = b"ticket/MX-443".to_vec();
    store
        .commit_raw_overlay_records_for_test(&[(
            mutable_overlay_secondary_index_address(&stale_legacy_index),
            secondary_index_record(
                3,
                stale_legacy_index.clone(),
                SecondaryIndexWriteOp::Put {
                    payload: stale_payload.clone(),
                },
            ),
        )])
        .unwrap();
    let stale_overlay_root = store.inner.lock().unwrap().overlay_root;
    store.inner.lock().unwrap().overlay_root = None;
    store
        .commit_family_root_records_for_test(
            SECONDARY_INDEX_FAMILY_ID,
            &[(
                mutable_overlay_secondary_index_address(&canonical_index),
                secondary_index_record(
                    4,
                    canonical_index.clone(),
                    SecondaryIndexWriteOp::Put {
                        payload: canonical_payload.clone(),
                    },
                ),
            )],
        )
        .unwrap();
    store.inner.lock().unwrap().overlay_root = stale_overlay_root;

    let (secondary_index_root, overlay_root) = {
        let inner = store.inner.lock().unwrap();
        (inner.secondary_index_root, inner.overlay_root)
    };

    assert!(secondary_index_root.is_some());
    assert!(overlay_root.is_some());
    assert_eq!(
        store
            .mutable_overlay_record_payload(&mutable_overlay_secondary_index_address(
                &stale_legacy_index
            ))
            .unwrap()
            .map(
                |bytes| decode_mutable_overlay_secondary_index_record(&bytes)
                    .unwrap()
                    .payload
                    .unwrap()
            ),
        Some(stale_payload)
    );
    assert_eq!(
        store
            .mutable_overlay_secondary_index_value(&stale_legacy_index)
            .unwrap(),
        None
    );
    assert_eq!(
        store
            .mutable_overlay_secondary_index_value(&canonical_index)
            .unwrap(),
        Some(canonical_payload)
    );
}

#[test]
fn secondary_index_malformed_key_mismatch_is_rejected() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let requested_index = OverlayKey::from_segments([
        b"workspace",
        &[56; 16],
        b"tickets",
        b"matrix",
        b"index",
        b"requested",
    ])
    .unwrap();
    let stored_index = OverlayKey::from_segments([
        b"workspace",
        &[57; 16],
        b"tickets",
        b"matrix",
        b"index",
        b"stored",
    ])
    .unwrap();
    store
        .commit_family_root_records_for_test(
            SECONDARY_INDEX_FAMILY_ID,
            &[(
                mutable_overlay_secondary_index_address(&requested_index),
                secondary_index_record(
                    5,
                    stored_index,
                    SecondaryIndexWriteOp::Put {
                        payload: b"ticket/MX-444".to_vec(),
                    },
                ),
            )],
        )
        .unwrap();

    let error = store
        .mutable_overlay_secondary_index_value(&requested_index)
        .unwrap_err();

    assert_eq!(error.code, Code::CorruptObject);
}

#[test]
fn secondary_index_mixed_root_set_publication_fails_closed() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let canonical_index = OverlayKey::from_segments([
        b"workspace",
        &[58; 16],
        b"tickets",
        b"matrix",
        b"index",
        b"canonical-mixed",
    ])
    .unwrap();
    let legacy_index = OverlayKey::from_segments([
        b"workspace",
        &[59; 16],
        b"tickets",
        b"matrix",
        b"index",
        b"legacy-mixed",
    ])
    .unwrap();
    store
        .commit_family_root_records_for_test(
            SECONDARY_INDEX_FAMILY_ID,
            &[(
                mutable_overlay_secondary_index_address(&canonical_index),
                secondary_index_record(
                    6,
                    canonical_index.clone(),
                    SecondaryIndexWriteOp::Put {
                        payload: b"ticket/MX-445".to_vec(),
                    },
                ),
            )],
        )
        .unwrap();

    let error = store
        .commit_raw_overlay_records_for_test(&[(
            mutable_overlay_secondary_index_address(&legacy_index),
            secondary_index_record(
                7,
                legacy_index.clone(),
                SecondaryIndexWriteOp::Put {
                    payload: b"ticket/MX-446".to_vec(),
                },
            ),
        )])
        .unwrap_err();

    assert_eq!(error.code, Code::CorruptObject);
    assert_eq!(
        store
            .mutable_overlay_secondary_index_value(&canonical_index)
            .unwrap()
            .as_deref(),
        Some(&b"ticket/MX-445"[..])
    );
    assert_eq!(
        store
            .mutable_overlay_secondary_index_value(&legacy_index)
            .unwrap(),
        None
    );
}

#[test]
fn mutable_idempotency_routes_through_catalog_family_root() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let key = OverlayKey::from_segments([
        b"workspace",
        &[60; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"MX-447",
    ])
    .unwrap();
    let idempotency_key = "canonical-mutable-idempotency";
    let token = loom_core::OverlayOwnerToken::from_bytes([80; 32]);
    let request_digest = mutable_overlay_idempotency_request_digest(&key, b"canonical-payload");

    store
        .commit_family_root_records_for_test(
            MUTABLE_IDEMPOTENCY_FAMILY_ID,
            &[(
                mutable_overlay_idempotency_address(idempotency_key),
                encode_mutable_overlay_idempotency_record(&request_digest, &token),
            )],
        )
        .unwrap();
    let (
        region_table_root,
        page_count,
        root_catalog_root,
        mutable_idempotency_root,
        workflow_idempotency_root,
        secondary_index_root,
        overlay_root,
        current_record_root,
    ) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.region_table_root.unwrap(),
            inner.page_count,
            inner.root_catalog_root,
            inner.mutable_idempotency_root,
            inner.workflow_idempotency_root,
            inner.secondary_index_root,
            inner.overlay_root,
            inner.current_record_root,
        )
    };
    let mut backing = shared.clone();
    let region = read_region_table(&mut backing, region_table_root, page_count).unwrap();
    let catalog = read_root_catalog(&mut backing, root_catalog_root.unwrap(), page_count).unwrap();

    assert_eq!(region.root_catalog_root, root_catalog_root);
    assert_eq!(region.overlay_root, None);
    assert_eq!(overlay_root, None);
    assert_eq!(current_record_root, None);
    assert_eq!(workflow_idempotency_root, None);
    assert_eq!(secondary_index_root, None);
    assert_eq!(
        catalog
            .entries
            .iter()
            .find(|entry| entry.family_id == MUTABLE_IDEMPOTENCY_FAMILY_ID)
            .map(|entry| entry.root),
        mutable_idempotency_root
    );
    assert!(mutable_idempotency_root.is_some());
    assert_eq!(
        store
            .mutable_overlay_record_payload(&mutable_overlay_idempotency_address(idempotency_key))
            .unwrap(),
        None
    );
    let record = store
        .mutable_overlay_idempotency_record(idempotency_key)
        .unwrap()
        .unwrap();
    assert_eq!(record.request_digest, request_digest);
    assert_eq!(record.owner_token.as_bytes(), token.as_bytes());
}

#[test]
fn mutable_idempotency_catalog_family_replays_and_conflicts_without_current_hydration() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let key = OverlayKey::from_segments([
        b"workspace",
        &[61; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"MX-448",
    ])
    .unwrap();
    let idempotency_key = "canonical-mutable-idempotency-replay";
    let token = loom_core::OverlayOwnerToken::from_bytes([81; 32]);
    let request_digest = mutable_overlay_idempotency_request_digest(&key, b"canonical-payload");
    store
        .commit_family_root_records_for_test(
            MUTABLE_IDEMPOTENCY_FAMILY_ID,
            &[(
                mutable_overlay_idempotency_address(idempotency_key),
                encode_mutable_overlay_idempotency_record(&request_digest, &token),
            )],
        )
        .unwrap();
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let stats = reopened.io_stats().unwrap();
    let (mutable_idempotency_root, overlay_root, current_record_root, root_catalog_root) = {
        let inner = reopened.inner.lock().unwrap();
        (
            inner.mutable_idempotency_root,
            inner.overlay_root,
            inner.current_record_root,
            inner.root_catalog_root,
        )
    };

    assert!(mutable_idempotency_root.is_some());
    assert!(root_catalog_root.is_some());
    assert_eq!(overlay_root, None);
    assert_eq!(current_record_root, None);
    assert_eq!(stats.open_mutable_current_records_loaded, 0);
    assert_eq!(stats.open_mutable_control_records_skipped, 0);
    assert_eq!(
        reopened
            .put_mutable_overlay_value_idempotent(
                key.clone(),
                b"canonical-payload".to_vec(),
                idempotency_key,
            )
            .unwrap()
            .as_bytes(),
        token.as_bytes()
    );
    let conflict = reopened
        .put_mutable_overlay_value_idempotent(key, b"different-payload".to_vec(), idempotency_key)
        .unwrap_err();
    assert_eq!(conflict.code, Code::Conflict);
}

#[test]
fn workflow_idempotency_routes_through_catalog_family_root() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let idempotency_key = b"canonical-workflow-idempotency";
    let target = durability_facet_test_key(b"documents", "workflow-idempotency-target");
    let token = loom_core::OverlayOwnerToken::from_bytes([82; 32]);
    let request_digest = Digest::blake3(b"canonical-workflow-request");
    let receipt = CommitReceipt {
        generation: OverlayGeneration::new(11),
        root_after: Digest::blake3(b"canonical-workflow-root"),
        writes: vec![loom_core::WriteOutcome {
            facet: FacetKind::Document,
            target: target.clone(),
            owner_token: token.clone(),
            change: loom_core::OverlayEntryKind::Value,
        }],
        operation_identities: Vec::new(),
        revision_identities: Vec::new(),
        audit_sequences: Vec::new(),
        retained_sequences: Vec::new(),
        delivery_receipts: Vec::new(),
        post_commit_delta: None,
        replayed: false,
    };

    store
        .commit_family_root_records_for_test(
            WORKFLOW_IDEMPOTENCY_FAMILY_ID,
            &[(
                mutable_overlay_transaction_idempotency_address(idempotency_key),
                encode_workflow_transaction_idempotency_record(&request_digest, &receipt).unwrap(),
            )],
        )
        .unwrap();
    let (
        region_table_root,
        page_count,
        root_catalog_root,
        workflow_idempotency_root,
        mutable_idempotency_root,
        secondary_index_root,
        overlay_root,
        current_record_root,
    ) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.region_table_root.unwrap(),
            inner.page_count,
            inner.root_catalog_root,
            inner.workflow_idempotency_root,
            inner.mutable_idempotency_root,
            inner.secondary_index_root,
            inner.overlay_root,
            inner.current_record_root,
        )
    };
    let mut backing = shared.clone();
    let region = read_region_table(&mut backing, region_table_root, page_count).unwrap();
    let catalog = read_root_catalog(&mut backing, root_catalog_root.unwrap(), page_count).unwrap();

    assert_eq!(region.root_catalog_root, root_catalog_root);
    assert_eq!(region.overlay_root, None);
    assert_eq!(overlay_root, None);
    assert_eq!(current_record_root, None);
    assert_eq!(mutable_idempotency_root, None);
    assert_eq!(secondary_index_root, None);
    assert_eq!(
        catalog
            .entries
            .iter()
            .find(|entry| entry.family_id == WORKFLOW_IDEMPOTENCY_FAMILY_ID)
            .map(|entry| entry.root),
        workflow_idempotency_root
    );
    assert!(workflow_idempotency_root.is_some());
    assert_eq!(
        store
            .mutable_overlay_record_payload(&mutable_overlay_transaction_idempotency_address(
                idempotency_key
            ))
            .unwrap(),
        None
    );
    let record = store
        .workflow_transaction_idempotency_record(idempotency_key)
        .unwrap()
        .unwrap();
    assert_eq!(record.request_digest, request_digest);
    assert!(record.receipt.replayed);
    assert_eq!(
        record.receipt.writes[0].owner_token.as_bytes(),
        token.as_bytes()
    );
}

#[test]
fn workflow_idempotency_receipt_round_trips_extended_fields() {
    let target = durability_facet_test_key(b"documents", "workflow-extended-receipt");
    let request_digest = Digest::blake3(b"workflow-extended-request");
    let receipt = CommitReceipt {
        generation: OverlayGeneration::new(31),
        root_after: Digest::blake3(b"workflow-extended-root"),
        writes: vec![loom_core::WriteOutcome {
            facet: FacetKind::Document,
            target,
            owner_token: loom_core::OverlayOwnerToken::from_bytes([0x31; 32]),
            change: loom_core::OverlayEntryKind::Value,
        }],
        operation_identities: vec!["operation-31".to_string()],
        revision_identities: vec![loom_core::RevisionReceipt {
            entity_id: "entity-31".to_string(),
            revision_id: "revision-31".to_string(),
        }],
        audit_sequences: vec![41, 42],
        retained_sequences: vec![loom_core::RetainedSequenceReceipt {
            key: b"retained-31".to_vec(),
            first_sequence: 7,
            last_sequence: 9,
        }],
        delivery_receipts: vec![loom_core::DeliveryReceipt {
            stream_id: "stream-31".to_string(),
            sequence: 11,
            envelope_id: "envelope-31".to_string(),
            payload_digest: Digest::blake3(b"delivery-31"),
        }],
        post_commit_delta: Some(loom_core::PostCommitDeltaReceipt {
            workspace: WorkspaceId::from_bytes([0x31; 16]),
            changed_paths: vec!["a.txt".to_string(), "b.txt".to_string()],
            changed_content_count: 2,
        }),
        replayed: false,
    };

    let encoded =
        encode_workflow_transaction_idempotency_record(&request_digest, &receipt).unwrap();
    let decoded = decode_workflow_transaction_idempotency_record(&encoded).unwrap();

    assert_eq!(decoded.request_digest, request_digest);
    let mut expected = receipt;
    expected.replayed = true;
    assert_eq!(decoded.receipt, expected);
}

fn workflow_receipt_legacy_prefix(write_count: u64) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MUTABLE_OVERLAY_TRANSACTION_IDEMPOTENCY_RECORD);
    out.extend_from_slice(Digest::blake3(b"bounded-request").bytes());
    put_uvarint(&mut out, 1);
    out.extend_from_slice(Digest::blake3(b"bounded-root").bytes());
    put_uvarint(&mut out, write_count);
    out
}

fn append_empty_extended_receipt_tail(out: &mut Vec<u8>) {
    put_uvarint(out, 0);
    put_uvarint(out, 0);
    put_uvarint(out, 0);
    put_uvarint(out, 0);
    put_uvarint(out, 0);
}

fn assert_workflow_receipt_corrupt(bytes: Vec<u8>) {
    let error = decode_workflow_transaction_idempotency_record(&bytes).unwrap_err();
    assert_eq!(error.code, Code::CorruptObject);
}

#[test]
fn workflow_idempotency_receipt_preserves_legacy_record_without_appended_fields() {
    let decoded =
        decode_workflow_transaction_idempotency_record(&workflow_receipt_legacy_prefix(0)).unwrap();

    assert!(decoded.receipt.replayed);
    assert!(decoded.receipt.writes.is_empty());
    assert!(decoded.receipt.operation_identities.is_empty());
    assert!(decoded.receipt.revision_identities.is_empty());
    assert!(decoded.receipt.audit_sequences.is_empty());
    assert!(decoded.receipt.retained_sequences.is_empty());
    assert!(decoded.receipt.delivery_receipts.is_empty());
    assert_eq!(decoded.receipt.post_commit_delta, None);
}

#[test]
fn workflow_idempotency_receipt_rejects_oversized_counts_and_lengths() {
    let too_many_cases = [
        ("write count", {
            let bytes =
                workflow_receipt_legacy_prefix(loom_core::WORKFLOW_RECEIPT_MAX_WRITES as u64 + 1);
            bytes
        }),
        ("operation count", {
            let mut bytes = workflow_receipt_legacy_prefix(0);
            put_uvarint(
                &mut bytes,
                loom_core::WORKFLOW_RECEIPT_MAX_OPERATIONS as u64 + 1,
            );
            bytes
        }),
        ("revision count", {
            let mut bytes = workflow_receipt_legacy_prefix(0);
            put_uvarint(&mut bytes, 0);
            put_uvarint(
                &mut bytes,
                loom_core::WORKFLOW_RECEIPT_MAX_REVISIONS as u64 + 1,
            );
            bytes
        }),
        ("audit sequence count", {
            let mut bytes = workflow_receipt_legacy_prefix(0);
            put_uvarint(&mut bytes, 0);
            put_uvarint(&mut bytes, 0);
            put_uvarint(
                &mut bytes,
                loom_core::WORKFLOW_RECEIPT_MAX_AUDIT_SEQUENCES as u64 + 1,
            );
            bytes
        }),
        ("retained sequence count", {
            let mut bytes = workflow_receipt_legacy_prefix(0);
            put_uvarint(&mut bytes, 0);
            put_uvarint(&mut bytes, 0);
            put_uvarint(&mut bytes, 0);
            put_uvarint(
                &mut bytes,
                loom_core::WORKFLOW_RECEIPT_MAX_RETAINED_SEQUENCES as u64 + 1,
            );
            bytes
        }),
        ("delivery receipt count", {
            let mut bytes = workflow_receipt_legacy_prefix(0);
            put_uvarint(&mut bytes, 0);
            put_uvarint(&mut bytes, 0);
            put_uvarint(&mut bytes, 0);
            put_uvarint(&mut bytes, 0);
            put_uvarint(
                &mut bytes,
                loom_core::WORKFLOW_RECEIPT_MAX_DELIVERY_RECEIPTS as u64 + 1,
            );
            bytes
        }),
    ];
    for (_name, bytes) in too_many_cases {
        assert_workflow_receipt_corrupt(bytes);
    }

    let string_too_long = loom_core::WORKFLOW_RECEIPT_MAX_STRING_BYTES as u64 + 1;
    let key_too_long = loom_core::WORKFLOW_RECEIPT_MAX_KEY_BYTES as u64 + 1;

    let mut write_key = workflow_receipt_legacy_prefix(1);
    write_key.push(FacetKind::Document.stable_tag());
    put_uvarint(&mut write_key, key_too_long);
    assert_workflow_receipt_corrupt(write_key);

    let mut operation_id = workflow_receipt_legacy_prefix(0);
    put_uvarint(&mut operation_id, 1);
    put_uvarint(&mut operation_id, string_too_long);
    assert_workflow_receipt_corrupt(operation_id);

    let mut revision_entity = workflow_receipt_legacy_prefix(0);
    put_uvarint(&mut revision_entity, 0);
    put_uvarint(&mut revision_entity, 1);
    put_uvarint(&mut revision_entity, string_too_long);
    assert_workflow_receipt_corrupt(revision_entity);

    let mut revision_id = workflow_receipt_legacy_prefix(0);
    put_uvarint(&mut revision_id, 0);
    put_uvarint(&mut revision_id, 1);
    put_workflow_receipt_string(&mut revision_id, "entity");
    put_uvarint(&mut revision_id, string_too_long);
    assert_workflow_receipt_corrupt(revision_id);

    let mut retained_key = workflow_receipt_legacy_prefix(0);
    put_uvarint(&mut retained_key, 0);
    put_uvarint(&mut retained_key, 0);
    put_uvarint(&mut retained_key, 0);
    put_uvarint(&mut retained_key, 1);
    put_uvarint(&mut retained_key, key_too_long);
    assert_workflow_receipt_corrupt(retained_key);

    let mut delivery_stream = workflow_receipt_legacy_prefix(0);
    put_uvarint(&mut delivery_stream, 0);
    put_uvarint(&mut delivery_stream, 0);
    put_uvarint(&mut delivery_stream, 0);
    put_uvarint(&mut delivery_stream, 0);
    put_uvarint(&mut delivery_stream, 1);
    put_uvarint(&mut delivery_stream, string_too_long);
    assert_workflow_receipt_corrupt(delivery_stream);

    let mut delivery_envelope = workflow_receipt_legacy_prefix(0);
    put_uvarint(&mut delivery_envelope, 0);
    put_uvarint(&mut delivery_envelope, 0);
    put_uvarint(&mut delivery_envelope, 0);
    put_uvarint(&mut delivery_envelope, 0);
    put_uvarint(&mut delivery_envelope, 1);
    put_workflow_receipt_string(&mut delivery_envelope, "stream");
    put_uvarint(&mut delivery_envelope, 1);
    put_uvarint(&mut delivery_envelope, string_too_long);
    assert_workflow_receipt_corrupt(delivery_envelope);

    let mut post_path_count = workflow_receipt_legacy_prefix(0);
    append_empty_extended_receipt_tail(&mut post_path_count);
    post_path_count.push(1);
    post_path_count.extend_from_slice(WorkspaceId::from_bytes([32; 16]).as_bytes());
    put_uvarint(
        &mut post_path_count,
        loom_core::WORKFLOW_RECEIPT_MAX_CHANGED_PATHS as u64 + 1,
    );
    assert_workflow_receipt_corrupt(post_path_count);

    let mut post_path = workflow_receipt_legacy_prefix(0);
    append_empty_extended_receipt_tail(&mut post_path);
    post_path.push(1);
    post_path.extend_from_slice(WorkspaceId::from_bytes([33; 16]).as_bytes());
    put_uvarint(&mut post_path, 1);
    put_uvarint(&mut post_path, string_too_long);
    assert_workflow_receipt_corrupt(post_path);

    let mut content_count = workflow_receipt_legacy_prefix(0);
    append_empty_extended_receipt_tail(&mut content_count);
    content_count.push(1);
    content_count.extend_from_slice(WorkspaceId::from_bytes([34; 16]).as_bytes());
    put_uvarint(&mut content_count, 0);
    put_uvarint(
        &mut content_count,
        loom_core::WORKFLOW_RECEIPT_MAX_CHANGED_CONTENT_COUNT + 1,
    );
    assert_workflow_receipt_corrupt(content_count);
}

#[test]
fn workflow_idempotency_receipt_rejects_truncated_malformed_overflowed_and_trailing_input() {
    let mut truncated = workflow_receipt_legacy_prefix(1);
    truncated.push(FacetKind::Document.stable_tag());
    put_uvarint(&mut truncated, 4);
    assert_workflow_receipt_corrupt(truncated);

    let mut overflowed_varint = workflow_receipt_legacy_prefix(0);
    overflowed_varint.extend_from_slice(&[0x80; 10]);
    assert_workflow_receipt_corrupt(overflowed_varint);

    let mut malformed_utf8 = workflow_receipt_legacy_prefix(0);
    put_uvarint(&mut malformed_utf8, 1);
    put_uvarint(&mut malformed_utf8, 1);
    malformed_utf8.push(0xff);
    assert_workflow_receipt_corrupt(malformed_utf8);

    let mut trailing = encode_workflow_transaction_idempotency_record(
        &Digest::blake3(b"trailing-request"),
        &CommitReceipt {
            generation: OverlayGeneration::new(44),
            root_after: Digest::blake3(b"trailing-root"),
            writes: Vec::new(),
            operation_identities: Vec::new(),
            revision_identities: Vec::new(),
            audit_sequences: Vec::new(),
            retained_sequences: Vec::new(),
            delivery_receipts: Vec::new(),
            post_commit_delta: None,
            replayed: false,
        },
    )
    .unwrap();
    trailing.push(0);
    assert_workflow_receipt_corrupt(trailing);
}

fn workflow_receipt_boundary_text(len: usize) -> String {
    "x".repeat(len)
}

fn workflow_receipt_boundary_payload(len: usize) -> Vec<u8> {
    vec![b'p'; len]
}

fn workflow_individual_boundary_transaction(
    workspace: WorkspaceId,
    idempotency: &'static [u8],
    apply: impl FnOnce(&mut WorkflowTransaction),
) -> WorkflowTransaction {
    let mut txn = WorkflowTransaction {
        workspace,
        actor: workspace,
        expected_generation: None,
        writes: vec![workflow_put(
            FacetKind::Document,
            durability_facet_test_key(b"documents", "workflow-boundary-write"),
            b"v",
            None,
        )],
        prepared_operations: Vec::new(),
        revision_metadata: Vec::new(),
        delivery_intents: Vec::new(),
        durability: OverlayDurabilityPolicy::Normal,
        boundary: AtomicityBoundary::Single,
        idempotency: Some(loom_core::IdempotencyKey::opaque(idempotency)),
        owner_state: loom_core::WorkflowOwnerState::default(),
        post_commit_delta: None,
    };
    apply(&mut txn);
    txn
}

#[test]
fn workflow_transaction_individual_boundary_values_publish_replay_and_decode() {
    let cases: Vec<(&str, Box<dyn FnOnce(&mut WorkflowTransaction)>)> = vec![
        (
            "write payload",
            Box::new(|txn| {
                txn.writes[0].op = loom_core::FacetWriteOp::Put {
                    payload: workflow_receipt_boundary_payload(
                        loom_core::WORKFLOW_RECEIPT_MAX_PAYLOAD_BYTES,
                    ),
                };
            }),
        ),
        (
            "prepared operation id",
            Box::new(|txn| {
                txn.prepared_operations.push(loom_core::PreparedOperation {
                    operation_id: workflow_receipt_boundary_text(
                        loom_core::WORKFLOW_RECEIPT_MAX_STRING_BYTES,
                    ),
                    payload: Vec::new(),
                });
            }),
        ),
        (
            "prepared operation payload",
            Box::new(|txn| {
                txn.prepared_operations.push(loom_core::PreparedOperation {
                    operation_id: "operation".to_string(),
                    payload: workflow_receipt_boundary_payload(
                        loom_core::WORKFLOW_RECEIPT_MAX_PAYLOAD_BYTES,
                    ),
                });
            }),
        ),
        (
            "revision payload",
            Box::new(|txn| {
                txn.revision_metadata
                    .push(loom_core::PreparedRevisionMetadata {
                        entity_id: "entity".to_string(),
                        revision_id: "revision".to_string(),
                        payload: workflow_receipt_boundary_payload(
                            loom_core::WORKFLOW_RECEIPT_MAX_PAYLOAD_BYTES,
                        ),
                    });
            }),
        ),
        (
            "retained payload",
            Box::new(|txn| {
                txn.owner_state
                    .controls
                    .push(loom_core::WorkflowControlWrite::AppendRetained {
                        key: b"history-boundary".to_vec(),
                        expected_next_sequence: 1,
                        records: vec![workflow_receipt_boundary_payload(
                            loom_core::WORKFLOW_RECEIPT_MAX_PAYLOAD_BYTES,
                        )],
                    });
            }),
        ),
        (
            "changed paths",
            Box::new(|txn| {
                let mut paths = (0..loom_core::WORKFLOW_RECEIPT_MAX_CHANGED_PATHS)
                    .map(|index| format!("path-{index}"))
                    .collect::<Vec<_>>();
                paths[0] =
                    workflow_receipt_boundary_text(loom_core::WORKFLOW_RECEIPT_MAX_STRING_BYTES);
                txn.post_commit_delta = Some(loom_core::EngineStateDelta::summary(
                    txn.workspace,
                    paths,
                    0,
                ));
            }),
        ),
    ];

    for (index, (name, apply)) in cases.into_iter().enumerate() {
        let shared = SharedMem::default();
        let store = FileStore::with_backing(Box::new(shared), true).unwrap();
        let workspace = WorkspaceId::from_bytes([35; 16]);
        let idempotency: &'static [u8] = Box::leak(
            format!("workflow-boundary-{index}")
                .into_bytes()
                .into_boxed_slice(),
        );
        let txn = workflow_individual_boundary_transaction(workspace, idempotency, apply);

        let receipt = store.commit_workflow_transaction(txn.clone()).unwrap();
        let replay = store.commit_workflow_transaction(txn).unwrap();
        let encoded = encode_workflow_transaction_idempotency_record(
            &Digest::blake3(name.as_bytes()),
            &receipt,
        )
        .unwrap();
        let decoded = decode_workflow_transaction_idempotency_record(&encoded).unwrap();

        assert!(!receipt.replayed, "{name}");
        assert!(replay.replayed, "{name}");
        assert_eq!(decoded.receipt.writes.len(), receipt.writes.len(), "{name}");
        assert_eq!(
            decoded.receipt.operation_identities, receipt.operation_identities,
            "{name}"
        );
    }
}

fn workflow_receipt_with_operation_aggregate(target: usize) -> CommitReceipt {
    for count in 1..=loom_core::WORKFLOW_RECEIPT_MAX_OPERATIONS {
        let fixed = if count == 1 {
            Vec::new()
        } else {
            vec![
                workflow_receipt_boundary_text(loom_core::WORKFLOW_RECEIPT_MAX_STRING_BYTES);
                count - 1
            ]
        };
        let mut with_empty_final = fixed.clone();
        with_empty_final.push(String::new());
        let base = workflow_receipt_unchecked_aggregate_len(&workflow_receipt_with_operation_ids(
            with_empty_final,
        ));
        if base > target {
            break;
        }
        let remaining = target - base;
        for final_len in remaining.saturating_sub(10)..=remaining {
            if final_len > loom_core::WORKFLOW_RECEIPT_MAX_STRING_BYTES {
                continue;
            }
            let encoded_delta =
                loom_core::workflow_varint_encoded_len(final_len as u64) + final_len - 1;
            if encoded_delta == remaining {
                let mut ids = fixed;
                ids.push(workflow_receipt_boundary_text(final_len));
                return workflow_receipt_with_operation_ids(ids);
            }
        }
    }
    panic!("could not construct receipt with aggregate size {target}");
}

fn workflow_receipt_with_operation_ids(operation_identities: Vec<String>) -> CommitReceipt {
    CommitReceipt {
        generation: OverlayGeneration::new(51),
        root_after: Digest::blake3(b"aggregate-root"),
        writes: Vec::new(),
        operation_identities,
        revision_identities: Vec::new(),
        audit_sequences: Vec::new(),
        retained_sequences: Vec::new(),
        delivery_receipts: Vec::new(),
        post_commit_delta: None,
        replayed: false,
    }
}

fn encode_workflow_transaction_idempotency_record_unchecked(
    request_digest: &Digest,
    receipt: &CommitReceipt,
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MUTABLE_OVERLAY_TRANSACTION_IDEMPOTENCY_RECORD);
    out.extend_from_slice(request_digest.bytes());
    put_uvarint(&mut out, receipt.generation.as_u64());
    out.extend_from_slice(receipt.root_after.bytes());
    put_uvarint(&mut out, receipt.writes.len() as u64);
    put_uvarint(&mut out, receipt.operation_identities.len() as u64);
    for operation_id in &receipt.operation_identities {
        put_workflow_receipt_string(&mut out, operation_id);
    }
    put_uvarint(&mut out, 0);
    put_uvarint(&mut out, 0);
    put_uvarint(&mut out, 0);
    put_uvarint(&mut out, 0);
    out.push(0);
    out
}

fn workflow_receipt_unchecked_aggregate_len(receipt: &CommitReceipt) -> usize {
    encode_workflow_transaction_idempotency_record_unchecked(
        &Digest::blake3(b"unchecked-aggregate-len"),
        receipt,
    )
    .len()
        - MUTABLE_OVERLAY_TRANSACTION_IDEMPOTENCY_RECORD.len()
        - 32
}

#[test]
fn workflow_idempotency_receipt_exact_aggregate_limit_round_trips_and_one_byte_over_fails() {
    let request_digest = Digest::blake3(b"aggregate-receipt-request");
    let exact = workflow_receipt_with_operation_aggregate(
        loom_core::WORKFLOW_TRANSACTION_MAX_AGGREGATE_ENCODED_BYTES,
    );
    assert_eq!(
        workflow_receipt_unchecked_aggregate_len(&exact),
        loom_core::WORKFLOW_TRANSACTION_MAX_AGGREGATE_ENCODED_BYTES
    );
    assert_eq!(
        exact.aggregate_encoded_len().unwrap(),
        loom_core::WORKFLOW_TRANSACTION_MAX_AGGREGATE_ENCODED_BYTES
    );
    let encoded = encode_workflow_transaction_idempotency_record(&request_digest, &exact).unwrap();
    let decoded = decode_workflow_transaction_idempotency_record(&encoded).unwrap();
    assert_eq!(
        decoded.receipt.operation_identities,
        exact.operation_identities
    );

    let over = workflow_receipt_with_operation_aggregate(
        loom_core::WORKFLOW_TRANSACTION_MAX_AGGREGATE_ENCODED_BYTES + 1,
    );
    let encode_error =
        encode_workflow_transaction_idempotency_record(&request_digest, &over).unwrap_err();
    assert_eq!(encode_error.code, Code::InvalidArgument);
    let persisted =
        encode_workflow_transaction_idempotency_record_unchecked(&request_digest, &over);
    assert_workflow_receipt_corrupt(persisted);
}

#[test]
fn workflow_transaction_aggregate_over_budget_preserves_state() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let workspace = WorkspaceId::from_bytes([37; 16]);
    let before_generation = store.mutable_overlay_generation().unwrap();
    let before_roots = {
        let inner = store.inner.lock().unwrap();
        (
            inner.region_table_root,
            inner.current_record_root,
            inner.root_catalog_root,
            inner.control_root,
            inner.audit_retention_root,
            inner.retained_history_root,
        )
    };
    let before_audits = store.audit_records().unwrap();
    let before_retained = store
        .retained_history_records(b"aggregate-history", 1, usize::MAX)
        .unwrap();
    let rejected = workflow_individual_boundary_transaction(workspace, b"aggregate-over", |txn| {
        txn.prepared_operations = vec![
            loom_core::PreparedOperation {
                operation_id: "aggregate-a".to_string(),
                payload: workflow_receipt_boundary_payload(
                    loom_core::WORKFLOW_RECEIPT_MAX_PAYLOAD_BYTES,
                ),
            },
            loom_core::PreparedOperation {
                operation_id: "aggregate-b".to_string(),
                payload: workflow_receipt_boundary_payload(
                    loom_core::WORKFLOW_RECEIPT_MAX_PAYLOAD_BYTES,
                ),
            },
        ];
    });
    let error = store.commit_workflow_transaction(rejected).unwrap_err();
    let after_roots = {
        let inner = store.inner.lock().unwrap();
        (
            inner.region_table_root,
            inner.current_record_root,
            inner.root_catalog_root,
            inner.control_root,
            inner.audit_retention_root,
            inner.retained_history_root,
        )
    };
    assert_eq!(error.code, Code::InvalidArgument);
    assert_eq!(
        store.mutable_overlay_generation().unwrap(),
        before_generation
    );
    assert_eq!(after_roots, before_roots);
    assert_eq!(store.audit_records().unwrap(), before_audits);
    assert_eq!(
        store
            .retained_history_records(b"aggregate-history", 1, usize::MAX)
            .unwrap(),
        before_retained
    );
}

fn workflow_invalid_transaction_base(store: &FileStore, suffix: &str) -> WorkflowTransaction {
    let workspace = WorkspaceId::from_bytes([36; 16]);
    WorkflowTransaction {
        workspace,
        actor: workspace,
        expected_generation: Some(store.mutable_overlay_generation().unwrap()),
        writes: vec![workflow_put(
            FacetKind::Document,
            durability_facet_test_key(b"documents", suffix),
            b"v",
            None,
        )],
        prepared_operations: Vec::new(),
        revision_metadata: Vec::new(),
        delivery_intents: Vec::new(),
        durability: OverlayDurabilityPolicy::Normal,
        boundary: AtomicityBoundary::Single,
        idempotency: None,
        owner_state: loom_core::WorkflowOwnerState::default(),
        post_commit_delta: None,
    }
}

#[test]
fn workflow_transaction_above_boundary_fails_before_mutation() {
    let cases: Vec<(&str, Box<dyn FnOnce(&FileStore) -> WorkflowTransaction>)> = vec![
        (
            "writes",
            Box::new(|store| {
                let mut txn = workflow_invalid_transaction_base(store, "invalid-writes");
                txn.writes = (0..=loom_core::WORKFLOW_RECEIPT_MAX_WRITES)
                    .map(|index| {
                        workflow_put(
                            FacetKind::Document,
                            durability_facet_test_key(
                                b"documents",
                                &format!("invalid-write-{index}"),
                            ),
                            b"v",
                            None,
                        )
                    })
                    .collect();
                txn
            }),
        ),
        (
            "prepared operation count",
            Box::new(|store| {
                let mut txn = workflow_invalid_transaction_base(store, "invalid-op-count");
                txn.prepared_operations = (0..=loom_core::WORKFLOW_RECEIPT_MAX_OPERATIONS)
                    .map(|index| loom_core::PreparedOperation {
                        operation_id: format!("operation-{index}"),
                        payload: Vec::new(),
                    })
                    .collect();
                txn
            }),
        ),
        (
            "prepared operation id",
            Box::new(|store| {
                let mut txn = workflow_invalid_transaction_base(store, "invalid-op-id");
                txn.prepared_operations.push(loom_core::PreparedOperation {
                    operation_id: workflow_receipt_boundary_text(
                        loom_core::WORKFLOW_RECEIPT_MAX_STRING_BYTES + 1,
                    ),
                    payload: Vec::new(),
                });
                txn
            }),
        ),
        (
            "prepared operation payload",
            Box::new(|store| {
                let mut txn = workflow_invalid_transaction_base(store, "invalid-op-payload");
                txn.prepared_operations.push(loom_core::PreparedOperation {
                    operation_id: "operation".to_string(),
                    payload: workflow_receipt_boundary_payload(
                        loom_core::WORKFLOW_RECEIPT_MAX_PAYLOAD_BYTES + 1,
                    ),
                });
                txn
            }),
        ),
        (
            "revision count",
            Box::new(|store| {
                let mut txn = workflow_invalid_transaction_base(store, "invalid-revision-count");
                txn.revision_metadata = (0..=loom_core::WORKFLOW_RECEIPT_MAX_REVISIONS)
                    .map(|index| loom_core::PreparedRevisionMetadata {
                        entity_id: format!("entity-{index}"),
                        revision_id: format!("revision-{index}"),
                        payload: Vec::new(),
                    })
                    .collect();
                txn
            }),
        ),
        (
            "revision entity",
            Box::new(|store| {
                let mut txn = workflow_invalid_transaction_base(store, "invalid-revision-entity");
                txn.revision_metadata
                    .push(loom_core::PreparedRevisionMetadata {
                        entity_id: workflow_receipt_boundary_text(
                            loom_core::WORKFLOW_RECEIPT_MAX_STRING_BYTES + 1,
                        ),
                        revision_id: "revision".to_string(),
                        payload: Vec::new(),
                    });
                txn
            }),
        ),
        (
            "revision id",
            Box::new(|store| {
                let mut txn = workflow_invalid_transaction_base(store, "invalid-revision-id");
                txn.revision_metadata
                    .push(loom_core::PreparedRevisionMetadata {
                        entity_id: "entity".to_string(),
                        revision_id: workflow_receipt_boundary_text(
                            loom_core::WORKFLOW_RECEIPT_MAX_STRING_BYTES + 1,
                        ),
                        payload: Vec::new(),
                    });
                txn
            }),
        ),
        (
            "revision payload",
            Box::new(|store| {
                let mut txn = workflow_invalid_transaction_base(store, "invalid-revision-payload");
                txn.revision_metadata
                    .push(loom_core::PreparedRevisionMetadata {
                        entity_id: "entity".to_string(),
                        revision_id: "revision".to_string(),
                        payload: workflow_receipt_boundary_payload(
                            loom_core::WORKFLOW_RECEIPT_MAX_PAYLOAD_BYTES + 1,
                        ),
                    });
                txn
            }),
        ),
        (
            "audit count",
            Box::new(|store| {
                let mut txn = workflow_invalid_transaction_base(store, "invalid-audit-count");
                txn.owner_state.audits = (0..=loom_core::WORKFLOW_RECEIPT_MAX_AUDIT_SEQUENCES)
                    .map(|index| loom_core::WorkflowAuditWrite {
                        principal: None,
                        action: format!("audit.{index}"),
                        target: None,
                    })
                    .collect();
                txn
            }),
        ),
        (
            "audit action",
            Box::new(|store| {
                let mut txn = workflow_invalid_transaction_base(store, "invalid-audit-action");
                txn.owner_state.audits.push(loom_core::WorkflowAuditWrite {
                    principal: None,
                    action: workflow_receipt_boundary_text(
                        loom_core::WORKFLOW_TRANSACTION_MAX_AUDIT_ACTION_BYTES + 1,
                    ),
                    target: None,
                });
                txn
            }),
        ),
        (
            "audit target",
            Box::new(|store| {
                let mut txn = workflow_invalid_transaction_base(store, "invalid-audit-target");
                txn.owner_state.audits.push(loom_core::WorkflowAuditWrite {
                    principal: None,
                    action: "audit.invalid".to_string(),
                    target: Some(workflow_receipt_boundary_text(
                        loom_core::WORKFLOW_TRANSACTION_MAX_AUDIT_TARGET_BYTES + 1,
                    )),
                });
                txn
            }),
        ),
        (
            "retained append count",
            Box::new(|store| {
                let mut txn = workflow_invalid_transaction_base(store, "invalid-retained-count");
                txn.owner_state.controls = (0..=loom_core::WORKFLOW_RECEIPT_MAX_RETAINED_SEQUENCES)
                    .map(|index| loom_core::WorkflowControlWrite::AppendRetained {
                        key: format!("history-{index}").into_bytes(),
                        expected_next_sequence: 1,
                        records: vec![b"history".to_vec()],
                    })
                    .collect();
                txn
            }),
        ),
        (
            "retained key",
            Box::new(|store| {
                let mut txn = workflow_invalid_transaction_base(store, "invalid-retained-key");
                txn.owner_state
                    .controls
                    .push(loom_core::WorkflowControlWrite::AppendRetained {
                        key: vec![b'k'; loom_core::WORKFLOW_RECEIPT_MAX_KEY_BYTES + 1],
                        expected_next_sequence: 1,
                        records: vec![b"history".to_vec()],
                    });
                txn
            }),
        ),
        (
            "retained record count",
            Box::new(|store| {
                let mut txn =
                    workflow_invalid_transaction_base(store, "invalid-retained-record-count");
                txn.owner_state
                    .controls
                    .push(loom_core::WorkflowControlWrite::AppendRetained {
                        key: b"history".to_vec(),
                        expected_next_sequence: 1,
                        records: vec![
                            b"history".to_vec();
                            loom_core::WORKFLOW_RECEIPT_MAX_RETAINED_SEQUENCES + 1
                        ],
                    });
                txn
            }),
        ),
        (
            "retained payload",
            Box::new(|store| {
                let mut txn = workflow_invalid_transaction_base(store, "invalid-retained-payload");
                txn.owner_state
                    .controls
                    .push(loom_core::WorkflowControlWrite::AppendRetained {
                        key: b"history".to_vec(),
                        expected_next_sequence: 1,
                        records: vec![workflow_receipt_boundary_payload(
                            loom_core::WORKFLOW_RECEIPT_MAX_PAYLOAD_BYTES + 1,
                        )],
                    });
                txn
            }),
        ),
        (
            "delivery count",
            Box::new(|store| {
                let mut txn = workflow_invalid_transaction_base(store, "invalid-delivery-count");
                txn.delivery_intents = (0..=loom_core::WORKFLOW_RECEIPT_MAX_DELIVERY_RECEIPTS)
                    .map(|index| loom_core::PreparedDeliveryIntent {
                        stream_id: format!("stream-{index}"),
                        sequence: index as u64,
                        envelope_id: format!("envelope-{index}"),
                        payload_digest: Digest::blake3(format!("payload-{index}").as_bytes()),
                    })
                    .collect();
                txn
            }),
        ),
        (
            "delivery stream",
            Box::new(|store| {
                let mut txn = workflow_invalid_transaction_base(store, "invalid-delivery-stream");
                txn.delivery_intents
                    .push(loom_core::PreparedDeliveryIntent {
                        stream_id: workflow_receipt_boundary_text(
                            loom_core::WORKFLOW_RECEIPT_MAX_STRING_BYTES + 1,
                        ),
                        sequence: 1,
                        envelope_id: "envelope".to_string(),
                        payload_digest: Digest::blake3(b"payload"),
                    });
                txn
            }),
        ),
        (
            "delivery envelope",
            Box::new(|store| {
                let mut txn = workflow_invalid_transaction_base(store, "invalid-delivery-envelope");
                txn.delivery_intents
                    .push(loom_core::PreparedDeliveryIntent {
                        stream_id: "stream".to_string(),
                        sequence: 1,
                        envelope_id: workflow_receipt_boundary_text(
                            loom_core::WORKFLOW_RECEIPT_MAX_STRING_BYTES + 1,
                        ),
                        payload_digest: Digest::blake3(b"payload"),
                    });
                txn
            }),
        ),
        (
            "post path count",
            Box::new(|store| {
                let mut txn = workflow_invalid_transaction_base(store, "invalid-post-path-count");
                txn.post_commit_delta = Some(loom_core::EngineStateDelta::summary(
                    txn.workspace,
                    (0..=loom_core::WORKFLOW_RECEIPT_MAX_CHANGED_PATHS)
                        .map(|index| format!("path-{index}")),
                    0,
                ));
                txn
            }),
        ),
        (
            "post path",
            Box::new(|store| {
                let mut txn = workflow_invalid_transaction_base(store, "invalid-post-path");
                txn.post_commit_delta = Some(loom_core::EngineStateDelta::summary(
                    txn.workspace,
                    [workflow_receipt_boundary_text(
                        loom_core::WORKFLOW_RECEIPT_MAX_STRING_BYTES + 1,
                    )],
                    0,
                ));
                txn
            }),
        ),
        (
            "post content count",
            Box::new(|store| {
                let mut txn = workflow_invalid_transaction_base(store, "invalid-post-content");
                txn.post_commit_delta = Some(loom_core::EngineStateDelta::summary(
                    txn.workspace,
                    Vec::<String>::new(),
                    loom_core::WORKFLOW_RECEIPT_MAX_CHANGED_CONTENT_COUNT as usize + 1,
                ));
                txn
            }),
        ),
    ];

    for (name, build) in cases {
        let shared = SharedMem::default();
        let store = FileStore::with_backing(Box::new(shared), true).unwrap();
        let before_generation = store.mutable_overlay_generation().unwrap();
        let before_roots = {
            let inner = store.inner.lock().unwrap();
            (
                inner.region_table_root,
                inner.current_record_root,
                inner.root_catalog_root,
                inner.control_root,
                inner.audit_retention_root,
            )
        };
        let err = store
            .commit_workflow_transaction(build(&store))
            .unwrap_err();
        let after_roots = {
            let inner = store.inner.lock().unwrap();
            (
                inner.region_table_root,
                inner.current_record_root,
                inner.root_catalog_root,
                inner.control_root,
                inner.audit_retention_root,
            )
        };

        assert_eq!(err.code, Code::InvalidArgument, "{name}");
        assert_eq!(
            store.mutable_overlay_generation().unwrap(),
            before_generation,
            "{name}"
        );
        assert_eq!(after_roots, before_roots, "{name}");
        assert!(store.audit_records().unwrap().is_empty(), "{name}");
    }
}

#[test]
fn workflow_transaction_receipt_reports_audit_and_retained_sequences() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let history_key = b"workflow/receipt/history".to_vec();
    let principal = WorkspaceId::from_bytes([18; 16]);
    let target = durability_facet_test_key(b"documents", "workflow-receipt-sequences");
    let txn = WorkflowTransaction {
        workspace: principal,
        actor: principal,
        expected_generation: None,
        writes: vec![workflow_put(
            FacetKind::Document,
            target,
            b"receipt-current",
            None,
        )],
        prepared_operations: vec![loom_core::PreparedOperation {
            operation_id: "receipt-operation".to_string(),
            payload: b"prepared-operation".to_vec(),
        }],
        revision_metadata: vec![loom_core::PreparedRevisionMetadata {
            entity_id: "receipt-entity".to_string(),
            revision_id: "receipt-revision".to_string(),
            payload: b"revision-metadata".to_vec(),
        }],
        delivery_intents: vec![loom_core::PreparedDeliveryIntent {
            stream_id: "receipt-stream".to_string(),
            sequence: 11,
            envelope_id: "receipt-envelope".to_string(),
            payload_digest: Digest::blake3(b"receipt-delivery"),
        }],
        durability: OverlayDurabilityPolicy::Normal,
        boundary: AtomicityBoundary::Single,
        idempotency: Some(loom_core::IdempotencyKey::opaque(
            b"workflow-receipt-sequences",
        )),
        owner_state: loom_core::WorkflowOwnerState {
            controls: vec![loom_core::WorkflowControlWrite::AppendRetained {
                key: history_key.clone(),
                expected_next_sequence: 1,
                records: vec![b"history-1".to_vec(), b"history-2".to_vec()],
            }],
            audits: vec![loom_core::WorkflowAuditWrite {
                principal: Some(principal),
                action: "workflow.receipt".to_string(),
                target: Some("receipt-target".to_string()),
            }],
            ..loom_core::WorkflowOwnerState::default()
        },
        post_commit_delta: None,
    };

    let receipt = store.commit_workflow_transaction(txn.clone()).unwrap();
    let replay = store.commit_workflow_transaction(txn).unwrap();

    assert!(!receipt.replayed);
    assert_eq!(receipt.operation_identities, ["receipt-operation"]);
    assert_eq!(
        receipt.revision_identities[0].revision_id,
        "receipt-revision"
    );
    assert_eq!(receipt.audit_sequences, [0]);
    assert_eq!(
        receipt.retained_sequences,
        [loom_core::RetainedSequenceReceipt {
            key: history_key.clone(),
            first_sequence: 1,
            last_sequence: 2,
        }]
    );
    assert_eq!(receipt.delivery_receipts[0].sequence, 11);
    assert!(replay.replayed);
    assert_eq!(replay.audit_sequences, receipt.audit_sequences);
    assert_eq!(replay.retained_sequences, receipt.retained_sequences);
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    assert_eq!(
        reopened
            .retained_history_records(&history_key, 1, usize::MAX)
            .unwrap(),
        vec![b"history-1".to_vec(), b"history-2".to_vec()]
    );
    let audit = reopened.audit_records().unwrap();
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].seq, 0);
    assert_eq!(audit[0].action, "workflow.receipt");
}

#[test]
fn workflow_idempotency_catalog_family_replays_and_conflicts_without_current_hydration() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let target = durability_facet_test_key(b"documents", "workflow-idempotency-replay");
    let txn = workflow_transaction_test(
        "workflow-idempotency-replay",
        vec![workflow_put(
            FacetKind::Document,
            target.clone(),
            b"canonical-workflow-payload",
            None,
        )],
        Some(b"canonical-workflow-idempotency-replay"),
    );
    let policy = store.store_policy().unwrap();
    let write_durabilities = txn
        .writes
        .iter()
        .map(|write| workflow_write_durability(&txn, &policy, write))
        .collect::<Vec<_>>();
    let request_digest = workflow_transaction_request_digest(&txn, &write_durabilities);
    let token = loom_core::OverlayOwnerToken::from_bytes([83; 32]);
    let receipt = CommitReceipt {
        generation: OverlayGeneration::new(12),
        root_after: Digest::blake3(b"canonical-workflow-replay-root"),
        writes: vec![loom_core::WriteOutcome {
            facet: FacetKind::Document,
            target: target.clone(),
            owner_token: token.clone(),
            change: loom_core::OverlayEntryKind::Value,
        }],
        operation_identities: Vec::new(),
        revision_identities: Vec::new(),
        audit_sequences: Vec::new(),
        retained_sequences: Vec::new(),
        delivery_receipts: Vec::new(),
        post_commit_delta: None,
        replayed: false,
    };
    let idempotency = txn.idempotency.as_ref().unwrap().as_bytes();
    store
        .commit_family_root_records_for_test(
            WORKFLOW_IDEMPOTENCY_FAMILY_ID,
            &[(
                mutable_overlay_transaction_idempotency_address(idempotency),
                encode_workflow_transaction_idempotency_record(&request_digest, &receipt).unwrap(),
            )],
        )
        .unwrap();
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let stats = reopened.io_stats().unwrap();
    let (workflow_idempotency_root, overlay_root, current_record_root, root_catalog_root) = {
        let inner = reopened.inner.lock().unwrap();
        (
            inner.workflow_idempotency_root,
            inner.overlay_root,
            inner.current_record_root,
            inner.root_catalog_root,
        )
    };

    assert!(workflow_idempotency_root.is_some());
    assert!(root_catalog_root.is_some());
    assert_eq!(overlay_root, None);
    assert_eq!(current_record_root, None);
    assert_eq!(stats.open_mutable_current_records_loaded, 0);
    assert_eq!(stats.open_mutable_control_records_skipped, 0);
    let replay = reopened.commit_workflow_transaction(txn.clone()).unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.writes[0].owner_token.as_bytes(), token.as_bytes());

    let conflict_txn = workflow_transaction_test(
        "workflow-idempotency-replay-conflict",
        vec![workflow_put(
            FacetKind::Document,
            target,
            b"different-workflow-payload",
            None,
        )],
        Some(b"canonical-workflow-idempotency-replay"),
    );
    let error = reopened
        .commit_workflow_transaction(conflict_txn)
        .unwrap_err();
    assert_eq!(error.code, Code::Conflict);
}

#[test]
fn absent_idempotency_catalog_families_read_empty() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();

    assert!(
        store
            .mutable_overlay_idempotency_record("absent-mutable-idempotency")
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .workflow_transaction_idempotency_record(b"absent-workflow-idempotency")
            .unwrap()
            .is_none()
    );
}

fn t188_20_current_entry(key: OverlayKey, generation: u64, payload: &[u8]) -> Vec<u8> {
    encode_mutable_overlay_entry(&loom_core::MutableOverlayEntrySnapshot {
        generation: OverlayGeneration::new(generation),
        key,
        owner_token: loom_core::OverlayOwnerToken::from_bytes([0x20; 32]),
        kind: loom_core::OverlayEntryKind::Value,
        payload: payload.to_vec(),
    })
}

fn t188_20_workflow_receipt(key: OverlayKey, generation: u64) -> CommitReceipt {
    CommitReceipt {
        generation: OverlayGeneration::new(generation),
        root_after: Digest::blake3(b"t188-20-workflow-root"),
        writes: vec![loom_core::WriteOutcome {
            facet: FacetKind::Document,
            target: key,
            owner_token: loom_core::OverlayOwnerToken::from_bytes([0x21; 32]),
            change: loom_core::OverlayEntryKind::Value,
        }],
        operation_identities: Vec::new(),
        revision_identities: Vec::new(),
        audit_sequences: Vec::new(),
        retained_sequences: Vec::new(),
        delivery_receipts: Vec::new(),
        post_commit_delta: None,
        replayed: false,
    }
}

fn t188_20_decoded_family_count(
    report: &SourceLayoutDiscoveryReport,
    family: SourceLayoutFamily,
) -> usize {
    report
        .entries
        .iter()
        .filter(|entry| {
            entry.family == family && entry.decode_state == SourceLayoutDecodeState::Decoded
        })
        .count()
}

fn t188_20_entry<'a>(
    report: &'a SourceLayoutDiscoveryReport,
    family: SourceLayoutFamily,
    state: SourceLayoutDecodeState,
) -> &'a SourceLayoutDiscoveryEntry {
    report
        .entries
        .iter()
        .find(|entry| entry.family == family && entry.decode_state == state)
        .unwrap()
}

fn t188_20_decoded_entry(
    source_address: &str,
    family: SourceLayoutFamily,
    key_or_identity: Option<&str>,
    sequence: Option<u64>,
    payload_digest: &str,
) -> SourceLayoutDiscoveryEntry {
    SourceLayoutDiscoveryEntry {
        source_address: source_address.to_string(),
        family,
        key_or_identity: key_or_identity.map(str::to_string),
        generation: None,
        sequence,
        payload_digest: Some(payload_digest.to_string()),
        payload_len: Some(1),
        ownership: SourceLayoutOwnership::LegacyOverlay,
        decode_state: SourceLayoutDecodeState::Decoded,
        rejection_reason: None,
    }
}

fn t188_20_conflict_reasons(mut entries: Vec<SourceLayoutDiscoveryEntry>) -> BTreeSet<String> {
    source_layout_append_conflicts(&mut entries);
    entries
        .into_iter()
        .filter(|entry| entry.decode_state == SourceLayoutDecodeState::Conflict)
        .map(|entry| entry.rejection_reason.unwrap())
        .collect()
}

fn t188_20_commit_current_root_preserving_overlay(
    store: &FileStore,
    records: &[([u8; 32], Vec<u8>)],
) {
    let overlay_root = store.inner.lock().unwrap().overlay_root;
    store.inner.lock().unwrap().overlay_root = None;
    store.commit_current_root_records_for_test(records).unwrap();
    store.inner.lock().unwrap().overlay_root = overlay_root;
}

fn t188_22_commit_pointer_only_current_root(
    store: &FileStore,
    records: &[([u8; 32], Vec<u8>)],
) -> PageId {
    let overlay_root = store.inner.lock().unwrap().overlay_root;
    store.inner.lock().unwrap().overlay_root = None;
    store.commit_current_root_records_for_test(records).unwrap();
    let current_root = store.inner.lock().unwrap().current_record_root.unwrap();
    {
        let mut inner = store.inner.lock().unwrap();
        inner.current_record_root = None;
        inner.overlay_root = overlay_root;
    }
    store
        .commit_raw_overlay_records_for_test(&[(
            mutable_overlay_current_root_address(),
            encode_mutable_overlay_current_root_record(Some(current_root)),
        )])
        .unwrap();
    current_root
}

fn t188_21_family(
    plan: &SourceLayoutMigrationPlan,
    family_id: u16,
) -> &SourceLayoutMigrationFamilyPlan {
    plan.catalog_families
        .iter()
        .find(|family| family.family_id == family_id)
        .unwrap()
}

fn t188_22_source_roots(
    store: &FileStore,
) -> (
    u64,
    u64,
    Option<PageId>,
    Option<PageId>,
    Option<PageId>,
    Option<PageId>,
) {
    let inner = store.inner.lock().unwrap();
    (
        inner.generation,
        inner.page_count,
        inner.region_table_root,
        inner.overlay_root,
        inner.current_record_root,
        inner.root_catalog_root,
    )
}

fn t188_22_replace_record_bytes(record: &mut SourceLayoutMigrationRecord, bytes: Vec<u8>) {
    record.payload_digest = Digest::blake3(&bytes).to_hex();
    record.payload_len = bytes.len();
    record.bytes = bytes;
}

fn t188_22_retained_family_mut(
    plan: &mut SourceLayoutMigrationPlan,
) -> &mut SourceLayoutMigrationFamilyPlan {
    plan.catalog_families
        .iter_mut()
        .find(|family| family.family_id == RETAINED_HISTORY_FAMILY_ID)
        .unwrap()
}

fn t188_23_populate_all_source_families(store: &FileStore) {
    let current_key = durability_facet_test_key(b"documents", "t188-23-current");
    let history_key = durability_facet_test_key(b"documents", "t188-23-history");
    let owner_key = durability_facet_test_key(b"documents", "t188-23-owner");
    let secondary_key = durability_facet_test_key(b"tickets", "t188-23-secondary");
    let workflow_target = durability_facet_test_key(b"documents", "t188-23-workflow");
    let owner_token = loom_core::OverlayOwnerToken::from_bytes([0x23; 32]);
    let mutable_request =
        mutable_overlay_idempotency_request_digest(&current_key, b"t188-23-current-payload");
    let workflow_request = Digest::blake3(b"t188-23-workflow-request");
    let workflow_receipt = t188_20_workflow_receipt(workflow_target, 9);
    store
        .commit_raw_overlay_records_for_test(&[
            (
                retained_history_head_address(history_key.as_bytes()),
                encode_retained_history_head(history_key.as_bytes(), 2),
            ),
            (
                retained_history_record_address(history_key.as_bytes(), 1),
                encode_retained_history_entry(history_key.as_bytes(), 1, b"t188-23-history-1"),
            ),
            (
                retained_history_record_address(history_key.as_bytes(), 2),
                encode_retained_history_entry(history_key.as_bytes(), 2, b"t188-23-history-2"),
            ),
            (
                mutable_overlay_owner_token_address(&owner_key),
                encode_mutable_overlay_owner_token_record(&owner_token),
            ),
            (
                mutable_overlay_secondary_index_address(&secondary_key),
                secondary_index_record(
                    7,
                    secondary_key,
                    SecondaryIndexWriteOp::Put {
                        payload: b"ticket/MX-T188-23".to_vec(),
                    },
                ),
            ),
            (
                mutable_overlay_idempotency_address("t188-23-mutable-idempotency"),
                encode_mutable_overlay_idempotency_record(&mutable_request, &owner_token),
            ),
            (
                mutable_overlay_transaction_idempotency_address(b"t188-23-workflow-idempotency"),
                encode_workflow_transaction_idempotency_record(
                    &workflow_request,
                    &workflow_receipt,
                )
                .unwrap(),
            ),
        ])
        .unwrap();
    let mut control = BTreeMap::new();
    control.insert(b"control/t188-23".to_vec(), b"control-payload".to_vec());
    control.insert(audit_entry_key(8), b"audit-payload".to_vec());
    store.commit_raw_control_map_for_test(control).unwrap();
    t188_22_commit_pointer_only_current_root(
        store,
        &[(
            mutable_overlay_entry_address(&current_key),
            t188_20_current_entry(current_key, 6, b"t188-23-current-payload"),
        )],
    );
}

fn t188_23_roots(
    store: &FileStore,
) -> (
    u64,
    Option<PageId>,
    Option<PageId>,
    Option<PageId>,
    Option<PageId>,
) {
    let inner = store.inner.lock().unwrap();
    (
        inner.generation,
        inner.region_table_root,
        inner.overlay_root,
        inner.current_record_root,
        inner.root_catalog_root,
    )
}

fn t188_23_family_root(store: &FileStore, family_id: u16) -> Option<PageId> {
    let inner = store.inner.lock().unwrap();
    inner
        .root_catalog_entries
        .iter()
        .find(|entry| entry.family_id == family_id)
        .map(|entry| entry.root)
}

fn t188_23_btree_payload(store: &FileStore, root: PageId, address: &[u8; 32]) -> Vec<u8> {
    let page_count = store.inner.lock().unwrap().page_count;
    let mut file = store.file.lock().unwrap();
    let loc = pagebtree::get(&mut **file, DATA_START, Some(root), address, page_count)
        .unwrap()
        .unwrap();
    read_blob_from_loc(&mut **file, loc).unwrap()
}

fn t188_23_assert_plan_on_canonical_roots(store: &FileStore, plan: &SourceLayoutMigrationPlan) {
    let (current_root, overlay_root, root_catalog_root, control_root) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.current_record_root,
            inner.overlay_root,
            inner.root_catalog_root,
            inner.control_root,
        )
    };
    assert!(overlay_root.is_none());
    assert!(root_catalog_root.is_some());
    let current_root = current_root.unwrap();
    for record in &plan.current_records {
        let address = source_layout_decode_hex_address(&record.canonical_address).unwrap();
        assert_eq!(
            t188_23_btree_payload(store, current_root, &address),
            record.bytes
        );
        assert_eq!(
            store
                .mutable_overlay_record_payload(&address)
                .unwrap()
                .unwrap(),
            record.bytes
        );
    }
    for family in &plan.catalog_families {
        let root = t188_23_family_root(store, family.family_id).unwrap();
        for record in &family.records {
            let address = source_layout_decode_hex_address(&record.canonical_address).unwrap();
            assert_eq!(t188_23_btree_payload(store, root, &address), record.bytes);
        }
    }
    let control_map = store.control_root_map().unwrap();
    assert_eq!(control_root.is_some(), !plan.control_records.is_empty());
    for record in &plan.control_records {
        let key = source_layout_decode_hex_bytes(&record.canonical_address).unwrap();
        assert_eq!(control_map.get(&key).unwrap(), &record.bytes);
        assert_eq!(store.control_get(&key).unwrap().unwrap(), record.bytes);
    }
}

#[test]
fn t188_22_validation_builds_temporary_roots_without_source_mutation() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let current_key = durability_facet_test_key(b"documents", "t188-22-current");
    let history_key = durability_facet_test_key(b"documents", "t188-22-history");
    store
        .commit_raw_overlay_records_for_test(&[
            (
                retained_history_head_address(history_key.as_bytes()),
                encode_retained_history_head(history_key.as_bytes(), 1),
            ),
            (
                retained_history_record_address(history_key.as_bytes(), 1),
                encode_retained_history_entry(history_key.as_bytes(), 1, b"history"),
            ),
        ])
        .unwrap();
    let mut control = BTreeMap::new();
    control.insert(b"control/t188-22".to_vec(), b"control-payload".to_vec());
    store.commit_raw_control_map_for_test(control).unwrap();
    t188_20_commit_current_root_preserving_overlay(
        &store,
        &[(
            mutable_overlay_entry_address(&current_key),
            t188_20_current_entry(current_key, 3, b"current"),
        )],
    );
    let before_bytes = shared.bytes();
    let before_roots = t188_22_source_roots(&store);
    let plan = store.source_layout_migration_plan().unwrap();

    let validation = store.validate_source_layout_migration_plan(&plan).unwrap();

    assert_eq!(validation.current_record_count, 1);
    assert_eq!(validation.control_record_count, 1);
    assert!(validation.temporary_current_root.is_some());
    assert!(validation.temporary_control_root.is_some());
    assert!(validation.temporary_root_catalog_root.is_some());
    assert!(validation.temporary_region_table_root.is_some());
    assert!(
        validation
            .temporary_catalog_roots
            .iter()
            .any(|(family_id, _)| *family_id == RETAINED_HISTORY_FAMILY_ID)
    );
    assert!(validation.temporary_page_count > 0);
    assert_eq!(before_bytes, shared.bytes());
    assert_eq!(before_roots, t188_22_source_roots(&store));
}

#[test]
fn t188_22_validation_rejects_bad_records_and_missing_current_root() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let current_key = durability_facet_test_key(b"documents", "t188-22-missing-current-root");
    store
        .commit_raw_overlay_records_for_test(&[(
            mutable_overlay_entry_address(&current_key),
            t188_20_current_entry(current_key, 1, b"legacy-current"),
        )])
        .unwrap();
    let missing_current = store.source_layout_migration_plan().unwrap();
    assert_eq!(
        store
            .validate_source_layout_migration_plan(&missing_current)
            .unwrap_err()
            .code,
        Code::CorruptObject
    );

    let mut bad_address = missing_current;
    bad_address.current_records[0].source_ownership = SourceLayoutOwnership::NestedCurrentRoot;
    bad_address.current_records[0].canonical_address = source_layout_address([0x55; 32]);
    assert_eq!(
        store
            .validate_source_layout_migration_plan(&bad_address)
            .unwrap_err()
            .code,
        Code::CorruptObject
    );
}

#[test]
fn t188_22_validation_rejects_stale_generation_and_roots() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let history_key = durability_facet_test_key(b"documents", "t188-22-stale-history");
    store
        .commit_raw_overlay_records_for_test(&[(
            retained_history_head_address(history_key.as_bytes()),
            encode_retained_history_head(history_key.as_bytes(), 1),
        )])
        .unwrap();
    let plan = store.source_layout_migration_plan().unwrap();
    let mut stale_generation = plan.clone();
    stale_generation.source_identity.generation = stale_generation
        .source_identity
        .generation
        .saturating_sub(1);
    assert_eq!(
        store
            .validate_source_layout_migration_plan(&stale_generation)
            .unwrap_err()
            .code,
        Code::Conflict
    );

    let mut stale_root = plan.clone();
    stale_root.source_identity.overlay_root = None;
    assert_eq!(
        store
            .validate_source_layout_migration_plan(&stale_root)
            .unwrap_err()
            .code,
        Code::Conflict
    );

    let mut live_stale = plan;
    live_stale.source_identity.page_count += 1;
    assert_eq!(
        store
            .validate_source_layout_migration_plan(&live_stale)
            .unwrap_err()
            .code,
        Code::Conflict
    );
}

#[test]
fn t188_22_validation_rejects_forged_ownership_duplicates_and_bad_catalog() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let current_key = durability_facet_test_key(b"documents", "t188-22-forged-current");
    let history_key = durability_facet_test_key(b"documents", "t188-22-forged-history");
    store
        .commit_raw_overlay_records_for_test(&[(
            retained_history_head_address(history_key.as_bytes()),
            encode_retained_history_head(history_key.as_bytes(), 1),
        )])
        .unwrap();
    t188_20_commit_current_root_preserving_overlay(
        &store,
        &[(
            mutable_overlay_entry_address(&current_key),
            t188_20_current_entry(current_key, 1, b"current"),
        )],
    );
    let plan = store.source_layout_migration_plan().unwrap();

    let mut forged = plan.clone();
    forged.current_records[0].source_ownership = SourceLayoutOwnership::LegacyOverlay;
    assert_eq!(
        store
            .validate_source_layout_migration_plan(&forged)
            .unwrap_err()
            .code,
        Code::CorruptObject
    );

    let mut duplicate_address = plan.clone();
    duplicate_address
        .current_records
        .push(duplicate_address.current_records[0].clone());
    assert_eq!(
        store
            .validate_source_layout_migration_plan(&duplicate_address)
            .unwrap_err()
            .code,
        Code::CorruptObject
    );

    let mut duplicate_family = plan.clone();
    duplicate_family
        .catalog_families
        .push(duplicate_family.catalog_families[0].clone());
    assert_eq!(
        store
            .validate_source_layout_migration_plan(&duplicate_family)
            .unwrap_err()
            .code,
        Code::CorruptObject
    );

    let mut bad_catalog = plan;
    bad_catalog.catalog_families[0].family_id = OWNER_TOKEN_FAMILY_ID;
    assert_eq!(
        store
            .validate_source_layout_migration_plan(&bad_catalog)
            .unwrap_err()
            .code,
        Code::CorruptObject
    );
}

#[test]
fn t188_22_validation_rejects_unplannable_source_records_before_activation() {
    let malformed = vec![SourceLayoutDiscoveryEntry {
        source_address: "malformed".to_string(),
        family: SourceLayoutFamily::CurrentEntry,
        key_or_identity: None,
        generation: None,
        sequence: None,
        payload_digest: None,
        payload_len: None,
        ownership: SourceLayoutOwnership::LegacyOverlay,
        decode_state: SourceLayoutDecodeState::Malformed,
        rejection_reason: Some("bad record".to_string()),
    }];
    assert_eq!(
        source_layout_reject_unplannable_entries(&malformed)
            .unwrap_err()
            .code,
        Code::CorruptObject
    );

    let unknown = vec![SourceLayoutDiscoveryEntry {
        source_address: "unknown".to_string(),
        family: SourceLayoutFamily::Unknown,
        key_or_identity: None,
        generation: None,
        sequence: None,
        payload_digest: None,
        payload_len: None,
        ownership: SourceLayoutOwnership::LegacyOverlay,
        decode_state: SourceLayoutDecodeState::UnknownFamily,
        rejection_reason: Some("unknown source-layout family".to_string()),
    }];
    assert_eq!(
        source_layout_reject_unplannable_entries(&unknown)
            .unwrap_err()
            .code,
        Code::CorruptObject
    );

    let mut duplicates = vec![
        t188_20_decoded_entry(
            "current-a",
            SourceLayoutFamily::CurrentEntry,
            Some("key"),
            None,
            "digest",
        ),
        t188_20_decoded_entry(
            "current-b",
            SourceLayoutFamily::CurrentEntry,
            Some("key"),
            None,
            "digest",
        ),
    ];
    source_layout_append_conflicts(&mut duplicates);
    assert_eq!(
        source_layout_reject_unplannable_entries(&duplicates)
            .unwrap_err()
            .code,
        Code::Conflict
    );
    let mut conflicts = vec![
        t188_20_decoded_entry(
            "current-a",
            SourceLayoutFamily::CurrentEntry,
            Some("key"),
            None,
            "digest-a",
        ),
        t188_20_decoded_entry(
            "current-b",
            SourceLayoutFamily::CurrentEntry,
            Some("key"),
            None,
            "digest-b",
        ),
    ];
    source_layout_append_conflicts(&mut conflicts);
    assert_eq!(
        source_layout_reject_unplannable_entries(&conflicts)
            .unwrap_err()
            .code,
        Code::Conflict
    );
}

#[test]
fn t188_22_validation_accepts_absent_optional_families_deterministically() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let plan = store.source_layout_migration_plan().unwrap();

    let first = store.validate_source_layout_migration_plan(&plan).unwrap();
    let second = store.validate_source_layout_migration_plan(&plan).unwrap();

    assert_eq!(first, second);
    assert_eq!(first.current_record_count, 0);
    assert!(first.catalog_families.is_empty());
    assert_eq!(first.temporary_current_root, None);
    assert!(first.temporary_catalog_roots.is_empty());
}

#[test]
fn t188_22_validation_interruption_before_activation_reopens_prior_source_generation() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let history_key = durability_facet_test_key(b"documents", "t188-22-rollback-history");
    store
        .commit_raw_overlay_records_for_test(&[
            (
                retained_history_head_address(history_key.as_bytes()),
                encode_retained_history_head(history_key.as_bytes(), 1),
            ),
            (
                retained_history_record_address(history_key.as_bytes(), 1),
                encode_retained_history_entry(history_key.as_bytes(), 1, b"history"),
            ),
        ])
        .unwrap();
    let before_bytes = shared.bytes();
    let before_roots = t188_22_source_roots(&store);
    let before_report = store.source_layout_discovery_report().unwrap();
    let plan = store.source_layout_migration_plan().unwrap();

    let validation = store.validate_source_layout_migration_plan(&plan).unwrap();

    assert!(
        validation
            .temporary_catalog_roots
            .iter()
            .any(|(family_id, _)| *family_id == RETAINED_HISTORY_FAMILY_ID)
    );
    assert_eq!(before_bytes, shared.bytes());
    assert_eq!(before_roots, t188_22_source_roots(&store));
    drop(store);
    let reopened = FileStore::with_backing(Box::new(shared.clone()), false).unwrap();
    assert_eq!(before_bytes, shared.bytes());
    assert_eq!(before_roots, t188_22_source_roots(&reopened));
    assert_eq!(
        before_report,
        reopened.source_layout_discovery_report().unwrap()
    );
}

#[test]
fn t188_22_validation_rejects_overlay_current_without_pointer() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let current_key = durability_facet_test_key(b"documents", "t188-22-overlay-without-pointer");
    store
        .commit_raw_overlay_records_for_test(&[(
            mutable_overlay_entry_address(&current_key),
            t188_20_current_entry(current_key, 1, b"current"),
        )])
        .unwrap();
    let plan = store.source_layout_migration_plan().unwrap();

    assert_eq!(
        store
            .validate_source_layout_migration_plan(&plan)
            .unwrap_err()
            .code,
        Code::CorruptObject
    );
}

#[test]
fn t188_22_validation_rejects_pointer_payload_mismatch() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let current_key = durability_facet_test_key(b"documents", "t188-22-pointer-mismatch");
    let current_root = t188_22_commit_pointer_only_current_root(
        &store,
        &[(
            mutable_overlay_entry_address(&current_key),
            t188_20_current_entry(current_key, 2, b"current"),
        )],
    );
    let mut plan = store.source_layout_migration_plan().unwrap();
    plan.source_pointers[0].bytes =
        encode_mutable_overlay_current_root_record(Some(PageId(current_root.0 + 1)));

    assert_eq!(
        store
            .validate_source_layout_migration_plan(&plan)
            .unwrap_err()
            .code,
        Code::CorruptObject
    );
}

#[test]
fn t188_22_validation_rejects_multiple_current_root_pointers() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let current_key = durability_facet_test_key(b"documents", "t188-22-multiple-pointers");
    t188_22_commit_pointer_only_current_root(
        &store,
        &[(
            mutable_overlay_entry_address(&current_key),
            t188_20_current_entry(current_key, 3, b"current"),
        )],
    );
    let mut plan = store.source_layout_migration_plan().unwrap();
    plan.source_pointers.push(plan.source_pointers[0].clone());

    assert_eq!(
        store
            .validate_source_layout_migration_plan(&plan)
            .unwrap_err()
            .code,
        Code::CorruptObject
    );
}

#[test]
fn t188_22_validation_rejects_forged_non_current_ownership() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let current_key = durability_facet_test_key(b"documents", "t188-22-forged-non-current");
    let history_key = durability_facet_test_key(b"documents", "t188-22-forged-history-family");
    store
        .commit_raw_overlay_records_for_test(&[(
            retained_history_head_address(history_key.as_bytes()),
            encode_retained_history_head(history_key.as_bytes(), 1),
        )])
        .unwrap();
    t188_20_commit_current_root_preserving_overlay(
        &store,
        &[(
            mutable_overlay_entry_address(&current_key),
            t188_20_current_entry(current_key, 4, b"current"),
        )],
    );
    let mut plan = store.source_layout_migration_plan().unwrap();
    plan.catalog_families[0].records[0].source_ownership = SourceLayoutOwnership::NestedCurrentRoot;

    assert_eq!(
        store
            .validate_source_layout_migration_plan(&plan)
            .unwrap_err()
            .code,
        Code::CorruptObject
    );
}

#[test]
fn t188_22_validation_builds_control_root_through_temp_object_index() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let mut control = BTreeMap::new();
    control.insert(
        b"control/t188-22-object-index".to_vec(),
        b"control-payload".to_vec(),
    );
    store.commit_raw_control_map_for_test(control).unwrap();
    let before_bytes = shared.bytes();
    let before_roots = t188_22_source_roots(&store);
    let plan = store.source_layout_migration_plan().unwrap();

    let validation = store.validate_source_layout_migration_plan(&plan).unwrap();

    assert!(validation.temporary_control_root.is_some());
    assert!(validation.temporary_object_index_root.is_some());
    assert!(validation.temporary_region_table_root.is_some());
    assert_eq!(before_bytes, shared.bytes());
    assert_eq!(before_roots, t188_22_source_roots(&store));
}

#[test]
fn t188_22_validation_rejects_tampered_current_record_bytes() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let current_key = durability_facet_test_key(b"documents", "t188-22-current-membership");
    t188_20_commit_current_root_preserving_overlay(
        &store,
        &[(
            mutable_overlay_entry_address(&current_key),
            t188_20_current_entry(current_key.clone(), 1, b"current"),
        )],
    );
    let mut plan = store.source_layout_migration_plan().unwrap();
    t188_22_replace_record_bytes(
        &mut plan.current_records[0],
        t188_20_current_entry(current_key, 2, b"tampered-current"),
    );

    assert_eq!(
        store
            .validate_source_layout_migration_plan(&plan)
            .unwrap_err()
            .code,
        Code::CorruptObject
    );
}

#[test]
fn t188_22_validation_rejects_tampered_pointer_record_bytes() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let current_key = durability_facet_test_key(b"documents", "t188-22-pointer-membership");
    let current_root = t188_22_commit_pointer_only_current_root(
        &store,
        &[(
            mutable_overlay_entry_address(&current_key),
            t188_20_current_entry(current_key, 1, b"current"),
        )],
    );
    let mut plan = store.source_layout_migration_plan().unwrap();
    t188_22_replace_record_bytes(
        &mut plan.source_pointers[0],
        encode_mutable_overlay_current_root_record(Some(PageId(current_root.0 + 1))),
    );

    assert_eq!(
        store
            .validate_source_layout_migration_plan(&plan)
            .unwrap_err()
            .code,
        Code::CorruptObject
    );
}

#[test]
fn t188_22_validation_rejects_tampered_catalog_record_bytes() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let history_key = durability_facet_test_key(b"documents", "t188-22-catalog-membership");
    store
        .commit_raw_overlay_records_for_test(&[(
            retained_history_head_address(history_key.as_bytes()),
            encode_retained_history_head(history_key.as_bytes(), 1),
        )])
        .unwrap();
    let mut plan = store.source_layout_migration_plan().unwrap();
    t188_22_replace_record_bytes(
        &mut t188_22_retained_family_mut(&mut plan).records[0],
        encode_retained_history_head(history_key.as_bytes(), 2),
    );

    assert_eq!(
        store
            .validate_source_layout_migration_plan(&plan)
            .unwrap_err()
            .code,
        Code::CorruptObject
    );
}

#[test]
fn t188_22_validation_rejects_tampered_control_derived_record_bytes() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let mut control = BTreeMap::new();
    control.insert(
        b"control/t188-22-control-membership".to_vec(),
        b"control-payload".to_vec(),
    );
    store.commit_raw_control_map_for_test(control).unwrap();
    let mut plan = store.source_layout_migration_plan().unwrap();
    t188_22_replace_record_bytes(&mut plan.control_records[0], b"tampered-control".to_vec());

    assert_eq!(
        store
            .validate_source_layout_migration_plan(&plan)
            .unwrap_err()
            .code,
        Code::CorruptObject
    );
}

#[test]
fn t188_22_validation_rejects_omitted_and_injected_control_records() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let mut control = BTreeMap::new();
    control.insert(
        b"control/t188-22-omitted".to_vec(),
        b"control-payload".to_vec(),
    );
    store.commit_raw_control_map_for_test(control).unwrap();
    let plan = store.source_layout_migration_plan().unwrap();

    let mut omitted = plan.clone();
    omitted.control_records.clear();
    assert_eq!(
        store
            .validate_source_layout_migration_plan(&omitted)
            .unwrap_err()
            .code,
        Code::CorruptObject
    );

    let mut injected = plan;
    let mut injected_record = injected.control_records[0].clone();
    injected_record.canonical_address = source_layout_bytes_identity(b"control/t188-22-injected");
    t188_22_replace_record_bytes(&mut injected_record, b"injected-control".to_vec());
    injected.control_records.push(injected_record);
    assert_eq!(
        store
            .validate_source_layout_migration_plan(&injected)
            .unwrap_err()
            .code,
        Code::CorruptObject
    );
}

#[test]
fn t188_22_validation_rejects_missing_current_record_from_deterministic_plan() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let first_key = durability_facet_test_key(b"documents", "t188-22-current-first");
    let second_key = durability_facet_test_key(b"documents", "t188-22-current-second");
    t188_20_commit_current_root_preserving_overlay(
        &store,
        &[
            (
                mutable_overlay_entry_address(&first_key),
                t188_20_current_entry(first_key, 1, b"first"),
            ),
            (
                mutable_overlay_entry_address(&second_key),
                t188_20_current_entry(second_key, 2, b"second"),
            ),
        ],
    );
    let mut plan = store.source_layout_migration_plan().unwrap();
    assert_eq!(plan.current_records.len(), 2);
    plan.current_records.remove(0);

    assert_eq!(
        store
            .validate_source_layout_migration_plan(&plan)
            .unwrap_err()
            .code,
        Code::CorruptObject
    );
}

#[test]
fn t188_22_validation_rejects_missing_catalog_record_from_deterministic_plan() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let history_key = durability_facet_test_key(b"documents", "t188-22-catalog-missing");
    store
        .commit_raw_overlay_records_for_test(&[
            (
                retained_history_head_address(history_key.as_bytes()),
                encode_retained_history_head(history_key.as_bytes(), 2),
            ),
            (
                retained_history_record_address(history_key.as_bytes(), 1),
                encode_retained_history_entry(history_key.as_bytes(), 1, b"one"),
            ),
            (
                retained_history_record_address(history_key.as_bytes(), 2),
                encode_retained_history_entry(history_key.as_bytes(), 2, b"two"),
            ),
        ])
        .unwrap();
    let mut plan = store.source_layout_migration_plan().unwrap();
    let family = t188_22_retained_family_mut(&mut plan);
    assert_eq!(family.records.len(), 3);
    family.records.remove(1);

    assert_eq!(
        store
            .validate_source_layout_migration_plan(&plan)
            .unwrap_err()
            .code,
        Code::CorruptObject
    );
}

#[test]
fn t188_22_validation_rejects_injected_current_record_absent_from_deterministic_plan() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let first_key = durability_facet_test_key(b"documents", "t188-22-current-inject-first");
    let second_key = durability_facet_test_key(b"documents", "t188-22-current-inject-second");
    t188_20_commit_current_root_preserving_overlay(
        &store,
        &[(
            mutable_overlay_entry_address(&first_key),
            t188_20_current_entry(first_key, 1, b"first"),
        )],
    );
    let mut plan = store.source_layout_migration_plan().unwrap();
    let mut injected = plan.current_records[0].clone();
    injected.source_address = source_layout_address(mutable_overlay_entry_address(&second_key));
    injected.canonical_address = source_layout_address(mutable_overlay_entry_address(&second_key));
    t188_22_replace_record_bytes(
        &mut injected,
        t188_20_current_entry(second_key, 2, b"second"),
    );
    plan.current_records.push(injected);

    assert_eq!(
        store
            .validate_source_layout_migration_plan(&plan)
            .unwrap_err()
            .code,
        Code::CorruptObject
    );
}

#[test]
fn t188_22_validation_rejects_injected_catalog_record_absent_from_deterministic_plan() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let history_key = durability_facet_test_key(b"documents", "t188-22-catalog-inject");
    store
        .commit_raw_overlay_records_for_test(&[(
            retained_history_head_address(history_key.as_bytes()),
            encode_retained_history_head(history_key.as_bytes(), 1),
        )])
        .unwrap();
    let mut plan = store.source_layout_migration_plan().unwrap();
    let mut injected = t188_22_retained_family_mut(&mut plan).records[0].clone();
    injected.source_address =
        source_layout_address(retained_history_record_address(history_key.as_bytes(), 2));
    injected.canonical_address = injected.source_address.clone();
    injected.source_family = SourceLayoutFamily::RetainedHistoryRecord;
    injected.sequence = Some(2);
    t188_22_replace_record_bytes(
        &mut injected,
        encode_retained_history_entry(history_key.as_bytes(), 2, b"injected"),
    );
    t188_22_retained_family_mut(&mut plan)
        .records
        .push(injected);

    assert_eq!(
        store
            .validate_source_layout_migration_plan(&plan)
            .unwrap_err()
            .code,
        Code::CorruptObject
    );
}

#[test]
fn t188_23_activation_publishes_one_canonical_generation_and_reopens() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    t188_23_populate_all_source_families(&store);
    let plan = store.source_layout_migration_plan().unwrap();
    let before_generation = store.inner.lock().unwrap().generation;

    let validation = store.activate_source_layout_migration_plan(&plan).unwrap();

    assert_eq!(validation.current_record_count, plan.current_records.len());
    assert_eq!(validation.control_record_count, plan.control_records.len());
    assert_eq!(
        store.inner.lock().unwrap().generation,
        before_generation + 1
    );
    t188_23_assert_plan_on_canonical_roots(&store, &plan);
    drop(store);
    let reopened = FileStore::with_backing(Box::new(shared), false).unwrap();
    t188_23_assert_plan_on_canonical_roots(&reopened, &plan);
}

#[test]
fn t188_23_activation_interruption_before_finish_leaves_roots_unchanged() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    t188_23_populate_all_source_families(&store);
    let plan = store.source_layout_migration_plan().unwrap();
    let before = t188_23_roots(&store);
    store
        .set_source_layout_activation_pre_finish_hook_for_test(Box::new(|| {
            Err(LoomError::invalid("t188-23 injected pre-finish failure"))
        }))
        .unwrap();

    assert_eq!(
        store
            .activate_source_layout_migration_plan(&plan)
            .unwrap_err()
            .code,
        Code::InvalidArgument
    );
    assert_eq!(before, t188_23_roots(&store));
    drop(store);
    let reopened = FileStore::with_backing(Box::new(shared), false).unwrap();
    assert_eq!(before, t188_23_roots(&reopened));
}

#[test]
fn t188_23_activation_rejects_stale_source_identity() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    t188_23_populate_all_source_families(&store);
    let plan = store.source_layout_migration_plan().unwrap();
    let mut control = BTreeMap::new();
    control.insert(b"control/t188-23-stale".to_vec(), b"changed".to_vec());
    store.commit_raw_control_map_for_test(control).unwrap();

    assert_eq!(
        store
            .activate_source_layout_migration_plan(&plan)
            .unwrap_err()
            .code,
        Code::Conflict
    );
}

#[test]
fn t188_23_activation_routes_every_family_through_canonical_roots() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    t188_23_populate_all_source_families(&store);
    let plan = store.source_layout_migration_plan().unwrap();

    store.activate_source_layout_migration_plan(&plan).unwrap();

    assert!(store.inner.lock().unwrap().overlay_root.is_none());
    for family_id in [
        RETAINED_HISTORY_FAMILY_ID,
        OWNER_TOKEN_FAMILY_ID,
        SECONDARY_INDEX_FAMILY_ID,
        MUTABLE_IDEMPOTENCY_FAMILY_ID,
        WORKFLOW_IDEMPOTENCY_FAMILY_ID,
        AUDIT_RETENTION_FAMILY_ID,
    ] {
        assert!(t188_23_family_root(&store, family_id).is_some());
    }
    t188_23_assert_plan_on_canonical_roots(&store, &plan);
}

#[test]
fn t188_23_activation_attribution_matches_validated_plan() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    t188_23_populate_all_source_families(&store);
    let plan = store.source_layout_migration_plan().unwrap();
    store.activate_source_layout_migration_plan(&plan).unwrap();

    let report = store.root_storage_attribution(128).unwrap();
    assert!(t188_16_attr(&report, "current_records").present);
    for family in &plan.catalog_families {
        let entry = report
            .roots
            .iter()
            .find(|entry| entry.family_id == Some(family.family_id))
            .unwrap();
        assert!(entry.present);
        assert!(entry.tree_pages > 0);
        assert!(entry.payload_bytes > 0);
    }
    assert!(t188_16_attr(&report, "control_root").present);
}

#[test]
fn t188_23_activation_preserves_source_pages_for_normal_gc() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    t188_23_populate_all_source_families(&store);
    let plan = store.source_layout_migration_plan().unwrap();
    let source_overlay_root = PageId(plan.source_identity.overlay_root.unwrap());
    let retained = t188_21_family(&plan, RETAINED_HISTORY_FAMILY_ID).records[0].clone();
    let retained_address = source_layout_decode_hex_address(&retained.source_address).unwrap();

    store.activate_source_layout_migration_plan(&plan).unwrap();

    assert!(store.inner.lock().unwrap().overlay_root.is_none());
    assert_eq!(
        t188_23_btree_payload(&store, source_overlay_root, &retained_address),
        retained.bytes
    );
}

fn mu_14b_count(
    report: &SourceLayoutReplacementPreflight,
    family: SourceLayoutFamily,
    ownership: SourceLayoutOwnership,
    decode_state: SourceLayoutDecodeState,
) -> usize {
    report
        .classified_owner_counts
        .iter()
        .find(|count| {
            count.family == family
                && count.ownership == ownership
                && count.decode_state == decode_state
        })
        .map(|count| count.count)
        .unwrap_or_default()
}

#[test]
fn mu_14b_preflight_returns_canonical_noop_without_source_plan() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let current_key = durability_facet_test_key(b"documents", "mu-14b-canonical-current");
    store
        .commit_current_root_records_for_test(&[(
            mutable_overlay_entry_address(&current_key),
            t188_20_current_entry(current_key, 1, b"canonical-current"),
        )])
        .unwrap();

    let preflight = store.source_layout_replacement_preflight().unwrap();

    assert_eq!(
        preflight.disposition,
        SourceLayoutReplacementPreflightDisposition::CanonicalNoop
    );
    assert_eq!(preflight.source_identity.overlay_root, None);
    assert!(preflight.source_identity.current_record_root.is_some());
    assert!(preflight.validation.is_none());
}

#[test]
fn mu_14b_preflight_reports_valid_legacy_readiness() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    t188_23_populate_all_source_families(&store);

    let preflight = store.source_layout_replacement_preflight().unwrap();
    let plan = store.source_layout_migration_plan().unwrap();

    assert_eq!(
        preflight.disposition,
        SourceLayoutReplacementPreflightDisposition::LegacyReady
    );
    assert_eq!(preflight.source_identity, plan.source_identity);
    assert!(preflight.source_identity.overlay_root.is_some());
    let validation = preflight.validation.as_ref().unwrap();
    assert_eq!(validation.current_record_count, plan.current_records.len());
    assert_eq!(validation.source_pointer_count, plan.source_pointers.len());
    assert_eq!(
        mu_14b_count(
            &preflight,
            SourceLayoutFamily::CurrentEntry,
            SourceLayoutOwnership::NestedCurrentRoot,
            SourceLayoutDecodeState::Decoded,
        ),
        1
    );
    assert_eq!(
        mu_14b_count(
            &preflight,
            SourceLayoutFamily::RetainedHistoryRecord,
            SourceLayoutOwnership::LegacyOverlay,
            SourceLayoutDecodeState::Decoded,
        ),
        2
    );
    assert_eq!(
        mu_14b_count(
            &preflight,
            SourceLayoutFamily::AuditControl,
            SourceLayoutOwnership::ControlRootObject,
            SourceLayoutDecodeState::Decoded,
        ),
        1
    );
}

#[test]
fn mu15b_b_prepared_historical_get_and_has_do_not_clone_snapshot() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let payload = blob(b"mu15b-b historical payload");
    let digest = store.put(&payload).unwrap();
    t188_23_populate_all_source_families(&store);
    let before = shared.bytes();

    FileStore::reset_copy_source_read_view_test_counters();
    store.prepare_copy_source_read_view().unwrap();
    assert_eq!(FileStore::copy_source_read_view_test_counters(), (0, 1));

    {
        let mut inner = store.inner.lock().unwrap();
        inner.index.clear();
        inner.locator_cache_order.clear();
        inner.index_page_cache.clear();
        inner.index_page_cache_order.clear();
        inner.index_root = None;
        inner.index_materialized = false;
    }
    FileStore::reset_copy_source_read_view_test_counters();
    for _ in 0..3 {
        assert_eq!(store.get(&digest).unwrap().unwrap(), payload);
        assert!(store.has(&digest).unwrap());
    }
    assert_eq!(FileStore::copy_source_read_view_test_counters(), (0, 0));

    let current_key = durability_facet_test_key(b"documents", "t188-23-current");
    assert_eq!(
        store
            .mutable_overlay_current_entry(&current_key)
            .unwrap()
            .unwrap()
            .payload,
        b"t188-23-current-payload"
    );
    assert_eq!(before, shared.bytes());

    let canonical = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let canonical_payload = blob(b"mu15b-b canonical payload");
    let canonical_digest = canonical.put(&canonical_payload).unwrap();
    canonical.prepare_copy_source_read_view().unwrap();
    FileStore::reset_copy_source_read_view_test_counters();
    for _ in 0..3 {
        assert_eq!(
            canonical.get(&canonical_digest).unwrap().unwrap(),
            canonical_payload
        );
        assert!(canonical.has(&canonical_digest).unwrap());
    }
    assert_eq!(FileStore::copy_source_read_view_test_counters(), (0, 0));
}

#[test]
fn mu_14b_preflight_control_root_only_audit_returns_legacy_ready() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let mut control = BTreeMap::new();
    control.insert(audit_entry_key(14), b"audit-only".to_vec());
    store.commit_raw_control_map_for_test(control).unwrap();

    let preflight = store.source_layout_replacement_preflight().unwrap();

    assert_eq!(
        preflight.disposition,
        SourceLayoutReplacementPreflightDisposition::LegacyReady
    );
    assert_eq!(preflight.source_identity.overlay_root, None);
    assert_eq!(
        mu_14b_count(
            &preflight,
            SourceLayoutFamily::AuditControl,
            SourceLayoutOwnership::ControlRootObject,
            SourceLayoutDecodeState::Decoded,
        ),
        1
    );
    let validation = preflight.validation.as_ref().unwrap();
    assert_eq!(
        validation
            .catalog_families
            .iter()
            .find(|family| family.family_id == AUDIT_RETENTION_FAMILY_ID)
            .unwrap()
            .record_count,
        1
    );
}

#[test]
fn mu_14b_preflight_control_root_only_ordinary_control_returns_canonical_noop() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let mut control = BTreeMap::new();
    control.insert(b"control/mu-14b-ordinary".to_vec(), b"ordinary".to_vec());
    store.commit_raw_control_map_for_test(control).unwrap();

    let preflight = store.source_layout_replacement_preflight().unwrap();

    assert_eq!(
        preflight.disposition,
        SourceLayoutReplacementPreflightDisposition::CanonicalNoop
    );
    assert_eq!(preflight.source_identity.overlay_root, None);
    assert!(preflight.validation.is_none());
    assert_eq!(
        mu_14b_count(
            &preflight,
            SourceLayoutFamily::Control,
            SourceLayoutOwnership::ControlRootObject,
            SourceLayoutDecodeState::Decoded,
        ),
        1
    );
}

#[test]
fn mu_14b_preflight_rejects_legacy_overlay_with_canonical_current_root() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let current_key = durability_facet_test_key(b"documents", "mu-14b-mixed-current");
    store
        .commit_raw_overlay_records_for_test(&[(
            retained_history_record_address(current_key.as_bytes(), 1),
            encode_retained_history_entry(current_key.as_bytes(), 1, b"legacy-history"),
        )])
        .unwrap();
    t188_20_commit_current_root_preserving_overlay(
        &store,
        &[(
            mutable_overlay_entry_address(&current_key),
            t188_20_current_entry(current_key, 1, b"canonical-current"),
        )],
    );

    let error = store.source_layout_replacement_preflight().unwrap_err();

    assert_eq!(error.code, Code::Conflict);
    assert!(
        error
            .message
            .contains("legacy overlay with canonical current-record root")
    );
}

#[test]
fn mu_14b_preflight_rejects_legacy_overlay_with_canonical_root_catalog() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let owner_key = durability_facet_test_key(b"documents", "mu-14b-mixed-catalog");
    let owner_token = loom_core::OverlayOwnerToken::from_bytes([0x14; 32]);
    store
        .commit_raw_overlay_records_for_test(&[(
            mutable_overlay_owner_token_address(&owner_key),
            encode_mutable_overlay_owner_token_record(&owner_token),
        )])
        .unwrap();
    let overlay_root = store.inner.lock().unwrap().overlay_root;
    store.inner.lock().unwrap().overlay_root = None;
    store
        .commit_family_root_records_for_test(
            OWNER_TOKEN_FAMILY_ID,
            &[(
                mutable_overlay_owner_token_address(&owner_key),
                encode_mutable_overlay_owner_token_record(&owner_token),
            )],
        )
        .unwrap();
    store.inner.lock().unwrap().overlay_root = overlay_root;

    let error = store.source_layout_replacement_preflight().unwrap_err();

    assert_eq!(error.code, Code::Conflict);
    assert!(
        error
            .message
            .contains("legacy overlay with canonical root-catalog root")
    );
}

#[test]
fn mu_14b_preflight_preserves_unknown_and_malformed_fail_closed() {
    let malformed = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let malformed_key = durability_facet_test_key(b"documents", "mu-14b-malformed");
    malformed
        .commit_raw_overlay_records_for_test(&[(
            mutable_overlay_entry_address(&malformed_key),
            b"loom.store.mutable-overlay.entry.v3".to_vec(),
        )])
        .unwrap();
    assert_eq!(
        malformed
            .source_layout_replacement_preflight()
            .unwrap_err()
            .code,
        Code::CorruptObject
    );

    let unknown = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    unknown
        .commit_raw_overlay_records_for_test(&[([0x14; 32], b"unknown-source-family".to_vec())])
        .unwrap();
    assert_eq!(
        unknown
            .source_layout_replacement_preflight()
            .unwrap_err()
            .code,
        Code::CorruptObject
    );
}

#[test]
fn mu_14b_preflight_malformed_control_root_fails_closed_before_noop() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    store.inner.lock().unwrap().control_root = Some(Digest::blake3(b"mu-14b-missing-control-root"));

    let error = store.source_layout_replacement_preflight().unwrap_err();

    assert_eq!(error.code, Code::CorruptObject);
    assert!(
        error
            .message
            .contains("source-layout migration plan rejected malformed Control")
    );
}

#[test]
fn mu_14b_preflight_is_deterministic_and_read_only() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    t188_23_populate_all_source_families(&store);
    let before_bytes = shared.bytes();
    let before_roots = t188_23_roots(&store);

    let first = store.source_layout_replacement_preflight().unwrap();
    let second = store.source_layout_replacement_preflight().unwrap();

    assert_eq!(first, second);
    assert_eq!(before_bytes, shared.bytes());
    assert_eq!(before_roots, t188_23_roots(&store));
}

#[test]
fn mu_14b_preflight_discovery_identity_mismatch_returns_conflict() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    store
        .set_source_layout_preflight_after_discovery_hook_for_test(Box::new(|inner| {
            inner.lock().unwrap().generation += 1;
            Ok(())
        }))
        .unwrap();

    let error = store.source_layout_replacement_preflight().unwrap_err();

    assert_eq!(error.code, Code::Conflict);
    assert!(
        error
            .message
            .contains("discovery report does not match captured source identity")
    );
}

#[test]
fn mu_14b_preflight_source_identity_is_freshness_evidence_not_a_lock() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    t188_23_populate_all_source_families(&store);
    let preflight = store.source_layout_replacement_preflight().unwrap();
    let plan = store.source_layout_migration_plan().unwrap();
    assert_eq!(preflight.source_identity, plan.source_identity);

    let mut control = BTreeMap::new();
    control.insert(b"control/mu-14b-stale".to_vec(), b"changed".to_vec());
    store.commit_raw_control_map_for_test(control).unwrap();

    assert_eq!(
        store
            .validate_source_layout_migration_plan(&plan)
            .unwrap_err()
            .code,
        Code::Conflict
    );
}

#[test]
fn t188_21_source_layout_plan_preserves_counts_and_exact_bytes() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let current_key = durability_facet_test_key(b"documents", "t188-21-current");
    let history_key = durability_facet_test_key(b"documents", "t188-21-history");
    let owner_key = durability_facet_test_key(b"documents", "t188-21-owner");
    let secondary_key = durability_facet_test_key(b"tickets", "t188-21-secondary");
    let idempotency_key = "t188-21-mutable-idempotency";
    let workflow_idempotency_key = b"t188-21-workflow-idempotency";
    let workflow_target = durability_facet_test_key(b"documents", "t188-21-workflow");
    let owner_token = loom_core::OverlayOwnerToken::from_bytes([0x23; 32]);
    let current_bytes = t188_20_current_entry(current_key.clone(), 6, b"t188-21-current-payload");
    let retained_head = encode_retained_history_head(history_key.as_bytes(), 2);
    let retained_one =
        encode_retained_history_entry(history_key.as_bytes(), 1, b"t188-21-history-1");
    let retained_two =
        encode_retained_history_entry(history_key.as_bytes(), 2, b"t188-21-history-2");
    let owner_bytes = encode_mutable_overlay_owner_token_record(&owner_token);
    let secondary_bytes = secondary_index_record(
        7,
        secondary_key.clone(),
        SecondaryIndexWriteOp::Put {
            payload: b"ticket/MX-T188-21".to_vec(),
        },
    );
    let mutable_request =
        mutable_overlay_idempotency_request_digest(&current_key, b"t188-21-current-payload");
    let mutable_idempotency_bytes =
        encode_mutable_overlay_idempotency_record(&mutable_request, &owner_token);
    let workflow_receipt = t188_20_workflow_receipt(workflow_target, 9);
    let workflow_request = Digest::blake3(b"t188-21-workflow-request");
    let workflow_idempotency_bytes =
        encode_workflow_transaction_idempotency_record(&workflow_request, &workflow_receipt)
            .unwrap();

    store
        .commit_raw_overlay_records_for_test(&[
            (
                retained_history_head_address(history_key.as_bytes()),
                retained_head.clone(),
            ),
            (
                retained_history_record_address(history_key.as_bytes(), 1),
                retained_one.clone(),
            ),
            (
                retained_history_record_address(history_key.as_bytes(), 2),
                retained_two.clone(),
            ),
            (
                mutable_overlay_owner_token_address(&owner_key),
                owner_bytes.clone(),
            ),
            (
                mutable_overlay_secondary_index_address(&secondary_key),
                secondary_bytes.clone(),
            ),
            (
                mutable_overlay_idempotency_address(idempotency_key),
                mutable_idempotency_bytes.clone(),
            ),
            (
                mutable_overlay_transaction_idempotency_address(workflow_idempotency_key),
                workflow_idempotency_bytes.clone(),
            ),
        ])
        .unwrap();
    let mut control = BTreeMap::new();
    control.insert(b"control/t188-21".to_vec(), b"control-payload".to_vec());
    control.insert(audit_entry_key(8), b"audit-payload".to_vec());
    store.commit_raw_control_map_for_test(control).unwrap();
    t188_20_commit_current_root_preserving_overlay(
        &store,
        &[(
            mutable_overlay_entry_address(&current_key),
            current_bytes.clone(),
        )],
    );

    let plan = store.source_layout_migration_plan().unwrap();

    assert_eq!(plan.current_records.len(), 1);
    assert_eq!(plan.current_records[0].bytes, current_bytes);
    assert_eq!(
        t188_21_family(&plan, RETAINED_HISTORY_FAMILY_ID)
            .records
            .iter()
            .map(|record| record.bytes.clone())
            .collect::<Vec<_>>(),
        vec![retained_head, retained_one, retained_two]
    );
    assert_eq!(
        t188_21_family(&plan, OWNER_TOKEN_FAMILY_ID).records[0].bytes,
        owner_bytes
    );
    assert_eq!(
        t188_21_family(&plan, SECONDARY_INDEX_FAMILY_ID).records[0].bytes,
        secondary_bytes
    );
    assert_eq!(
        t188_21_family(&plan, MUTABLE_IDEMPOTENCY_FAMILY_ID).records[0].bytes,
        mutable_idempotency_bytes
    );
    assert_eq!(
        t188_21_family(&plan, WORKFLOW_IDEMPOTENCY_FAMILY_ID).records[0].bytes,
        workflow_idempotency_bytes
    );
    let audit = t188_21_family(&plan, AUDIT_RETENTION_FAMILY_ID);
    assert_eq!(audit.records.len(), 1);
    assert_eq!(
        decode_audit_retention_record(&audit.records[0].bytes).unwrap(),
        (audit_entry_key(8), b"audit-payload".to_vec())
    );
    assert_eq!(plan.control_records.len(), 1);
    assert_eq!(plan.control_records[0].bytes, b"control-payload");
}

#[test]
fn t188_21_source_layout_plan_is_deterministic_and_read_only() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let history_key = durability_facet_test_key(b"documents", "t188-21-read-only-history");
    store
        .commit_raw_overlay_records_for_test(&[
            (
                retained_history_record_address(history_key.as_bytes(), 2),
                encode_retained_history_entry(history_key.as_bytes(), 2, b"revision-2"),
            ),
            (
                retained_history_head_address(history_key.as_bytes()),
                encode_retained_history_head(history_key.as_bytes(), 2),
            ),
            (
                retained_history_record_address(history_key.as_bytes(), 1),
                encode_retained_history_entry(history_key.as_bytes(), 1, b"revision-1"),
            ),
        ])
        .unwrap();
    let before = shared.bytes();
    let first = store.source_layout_migration_plan().unwrap();
    let second = store.source_layout_migration_plan().unwrap();
    let after = shared.bytes();

    assert_eq!(first, second);
    assert_eq!(before, after);
    assert_eq!(before.len(), after.len());
    assert_eq!(
        t188_21_family(&first, RETAINED_HISTORY_FAMILY_ID)
            .records
            .iter()
            .map(|record| (record.source_family, record.sequence))
            .collect::<Vec<_>>(),
        vec![
            (SourceLayoutFamily::RetainedHistoryHead, Some(2)),
            (SourceLayoutFamily::RetainedHistoryRecord, Some(1)),
            (SourceLayoutFamily::RetainedHistoryRecord, Some(2)),
        ]
    );
}

#[test]
fn t188_21_source_layout_plan_allows_absent_optional_families() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let plan = store.source_layout_migration_plan().unwrap();

    assert!(plan.current_records.is_empty());
    assert!(plan.source_pointers.is_empty());
    assert!(plan.catalog_families.is_empty());
    assert!(plan.control_records.is_empty());
}

#[test]
fn t188_21_source_layout_plan_rejects_unplannable_records() {
    let malformed = vec![SourceLayoutDiscoveryEntry {
        source_address: "malformed".to_string(),
        family: SourceLayoutFamily::CurrentEntry,
        key_or_identity: None,
        generation: None,
        sequence: None,
        payload_digest: None,
        payload_len: None,
        ownership: SourceLayoutOwnership::LegacyOverlay,
        decode_state: SourceLayoutDecodeState::Malformed,
        rejection_reason: Some("bad record".to_string()),
    }];
    assert_eq!(
        source_layout_reject_unplannable_entries(&malformed)
            .unwrap_err()
            .code,
        Code::CorruptObject
    );

    let conflicting = vec![SourceLayoutDiscoveryEntry {
        source_address: "conflict".to_string(),
        family: SourceLayoutFamily::CurrentEntry,
        key_or_identity: Some("key".to_string()),
        generation: None,
        sequence: None,
        payload_digest: None,
        payload_len: None,
        ownership: SourceLayoutOwnership::LegacyOverlay,
        decode_state: SourceLayoutDecodeState::Conflict,
        rejection_reason: Some("duplicate conflicting source-layout records".to_string()),
    }];
    assert_eq!(
        source_layout_reject_unplannable_entries(&conflicting)
            .unwrap_err()
            .code,
        Code::Conflict
    );
}

#[test]
fn t188_20_source_layout_discovery_allows_multiple_retained_revisions() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let history_key = durability_facet_test_key(b"documents", "t188-20-retained-revisions");
    store
        .commit_raw_overlay_records_for_test(&[
            (
                retained_history_head_address(history_key.as_bytes()),
                encode_retained_history_head(history_key.as_bytes(), 2),
            ),
            (
                retained_history_record_address(history_key.as_bytes(), 1),
                encode_retained_history_entry(history_key.as_bytes(), 1, b"revision-1"),
            ),
            (
                retained_history_record_address(history_key.as_bytes(), 2),
                encode_retained_history_entry(history_key.as_bytes(), 2, b"revision-2"),
            ),
        ])
        .unwrap();

    let report = store.source_layout_discovery_report().unwrap();

    assert_eq!(
        t188_20_decoded_family_count(&report, SourceLayoutFamily::RetainedHistoryRecord),
        2
    );
    assert!(
        report
            .entries
            .iter()
            .all(|entry| entry.decode_state != SourceLayoutDecodeState::Conflict)
    );
}

#[test]
fn t188_20_source_layout_conflicts_use_family_logical_identity() {
    let retained_reasons = t188_20_conflict_reasons(vec![
        t188_20_decoded_entry(
            "retained-address-a",
            SourceLayoutFamily::RetainedHistoryRecord,
            Some("retained-key"),
            Some(3),
            "digest-a",
        ),
        t188_20_decoded_entry(
            "retained-address-b",
            SourceLayoutFamily::RetainedHistoryRecord,
            Some("retained-key"),
            Some(3),
            "digest-b",
        ),
    ]);
    assert!(retained_reasons.contains("duplicate conflicting source-layout records"));

    let retained_distinct_reasons = t188_20_conflict_reasons(vec![
        t188_20_decoded_entry(
            "retained-address-a",
            SourceLayoutFamily::RetainedHistoryRecord,
            Some("retained-key"),
            Some(3),
            "digest-a",
        ),
        t188_20_decoded_entry(
            "retained-address-b",
            SourceLayoutFamily::RetainedHistoryRecord,
            Some("retained-key"),
            Some(4),
            "digest-b",
        ),
    ]);
    assert!(retained_distinct_reasons.is_empty());

    let pointer_reasons = t188_20_conflict_reasons(vec![
        t188_20_decoded_entry(
            "current-root-pointer-address",
            SourceLayoutFamily::CurrentRootPointer,
            Some("page:1"),
            None,
            "digest-a",
        ),
        t188_20_decoded_entry(
            "current-root-pointer-address",
            SourceLayoutFamily::CurrentRootPointer,
            Some("page:2"),
            None,
            "digest-b",
        ),
    ]);
    assert!(pointer_reasons.contains("duplicate conflicting source-layout records"));

    let owner_reasons = t188_20_conflict_reasons(vec![
        t188_20_decoded_entry(
            "owner-token-address",
            SourceLayoutFamily::OwnerToken,
            Some("token-a"),
            None,
            "digest-a",
        ),
        t188_20_decoded_entry(
            "owner-token-address",
            SourceLayoutFamily::OwnerToken,
            Some("token-b"),
            None,
            "digest-b",
        ),
    ]);
    assert!(owner_reasons.contains("duplicate conflicting source-layout records"));
}

#[test]
fn t188_20_source_layout_discovery_classifies_complete_inventory() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let current_key = durability_facet_test_key(b"documents", "t188-20-current");
    let history_key = durability_facet_test_key(b"documents", "t188-20-history");
    let owner_key = durability_facet_test_key(b"documents", "t188-20-owner");
    let secondary_key = durability_facet_test_key(b"tickets", "t188-20-secondary");
    let idempotency_key = "t188-20-mutable-idempotency";
    let workflow_idempotency_key = b"t188-20-workflow-idempotency";
    let workflow_target = durability_facet_test_key(b"documents", "t188-20-workflow");
    let owner_token = loom_core::OverlayOwnerToken::from_bytes([0x22; 32]);
    let mutable_request =
        mutable_overlay_idempotency_request_digest(&current_key, b"t188-20-current-payload");
    let workflow_request = Digest::blake3(b"t188-20-workflow-request");
    let workflow_receipt = t188_20_workflow_receipt(workflow_target, 9);

    store
        .commit_raw_overlay_records_for_test(&[
            (
                mutable_overlay_current_root_address(),
                encode_mutable_overlay_current_root_record(None),
            ),
            (
                retained_history_head_address(history_key.as_bytes()),
                encode_retained_history_head(history_key.as_bytes(), 4),
            ),
            (
                retained_history_record_address(history_key.as_bytes(), 4),
                encode_retained_history_entry(
                    history_key.as_bytes(),
                    4,
                    b"t188-20-history-payload",
                ),
            ),
            (
                mutable_overlay_owner_token_address(&owner_key),
                encode_mutable_overlay_owner_token_record(&owner_token),
            ),
            (
                mutable_overlay_secondary_index_address(&secondary_key),
                secondary_index_record(
                    7,
                    secondary_key.clone(),
                    SecondaryIndexWriteOp::Put {
                        payload: b"ticket/MX-T188-20".to_vec(),
                    },
                ),
            ),
            (
                mutable_overlay_idempotency_address(idempotency_key),
                encode_mutable_overlay_idempotency_record(&mutable_request, &owner_token),
            ),
            (
                mutable_overlay_transaction_idempotency_address(workflow_idempotency_key),
                encode_workflow_transaction_idempotency_record(
                    &workflow_request,
                    &workflow_receipt,
                )
                .unwrap(),
            ),
        ])
        .unwrap();
    let mut control = BTreeMap::new();
    control.insert(b"control/t188-20".to_vec(), b"control-payload".to_vec());
    control.insert(audit_entry_key(5), b"audit-payload".to_vec());
    store.commit_raw_control_map_for_test(control).unwrap();
    t188_20_commit_current_root_preserving_overlay(
        &store,
        &[(
            mutable_overlay_entry_address(&current_key),
            t188_20_current_entry(current_key.clone(), 6, b"t188-20-current-payload"),
        )],
    );

    let report = store.source_layout_discovery_report().unwrap();

    for family in [
        SourceLayoutFamily::CurrentEntry,
        SourceLayoutFamily::CurrentRootPointer,
        SourceLayoutFamily::RetainedHistoryHead,
        SourceLayoutFamily::RetainedHistoryRecord,
        SourceLayoutFamily::OwnerToken,
        SourceLayoutFamily::SecondaryIndex,
        SourceLayoutFamily::MutableIdempotency,
        SourceLayoutFamily::WorkflowIdempotency,
        SourceLayoutFamily::AuditControl,
        SourceLayoutFamily::Control,
    ] {
        assert_eq!(t188_20_decoded_family_count(&report, family), 1);
    }
    assert_eq!(
        t188_20_entry(
            &report,
            SourceLayoutFamily::CurrentEntry,
            SourceLayoutDecodeState::Decoded,
        )
        .generation,
        Some(6)
    );
    assert_eq!(
        t188_20_entry(
            &report,
            SourceLayoutFamily::RetainedHistoryRecord,
            SourceLayoutDecodeState::Decoded,
        )
        .sequence,
        Some(4)
    );
    assert_eq!(
        t188_20_entry(
            &report,
            SourceLayoutFamily::AuditControl,
            SourceLayoutDecodeState::Decoded,
        )
        .sequence,
        Some(5)
    );
    assert!(
        report
            .entries
            .iter()
            .filter(|entry| entry.decode_state == SourceLayoutDecodeState::Decoded)
            .all(|entry| entry.payload_digest.is_some() && entry.payload_len.is_some())
    );
}

#[test]
fn t188_20_source_layout_discovery_is_read_only_and_deterministic() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let history_key = durability_facet_test_key(b"documents", "t188-20-read-only-history");
    store
        .commit_raw_overlay_records_for_test(&[
            (
                mutable_overlay_current_root_address(),
                encode_mutable_overlay_current_root_record(None),
            ),
            (
                retained_history_head_address(history_key.as_bytes()),
                encode_retained_history_head(history_key.as_bytes(), 1),
            ),
            (
                retained_history_record_address(history_key.as_bytes(), 1),
                encode_retained_history_entry(history_key.as_bytes(), 1, b"history"),
            ),
        ])
        .unwrap();
    let before_bytes = shared.bytes();
    let first = store.source_layout_discovery_report().unwrap();
    let second = store.source_layout_discovery_report().unwrap();
    let after_bytes = shared.bytes();

    assert_eq!(first, second);
    assert_eq!(before_bytes, after_bytes);
    assert_eq!(before_bytes.len(), after_bytes.len());
    assert_eq!(first.generation, second.generation);
    assert_eq!(first.overlay_root, second.overlay_root);
    assert_eq!(first.current_record_root, second.current_record_root);
    assert_eq!(first.root_catalog_root, second.root_catalog_root);
    assert_eq!(first.control_root, second.control_root);

    drop(store);
    let reopened = FileStore::with_backing(Box::new(shared.clone()), false).unwrap();
    let reopened_report = reopened.source_layout_discovery_report().unwrap();
    assert_eq!(first, reopened_report);
    assert_eq!(before_bytes, shared.bytes());
}

#[test]
fn t188_20_source_layout_discovery_reports_malformed_unknown_and_conflicts() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let duplicate_key = durability_facet_test_key(b"documents", "t188-20-duplicate");
    let conflict_key = durability_facet_test_key(b"documents", "t188-20-conflict");
    let duplicate_record = t188_20_current_entry(duplicate_key.clone(), 1, b"same");
    store
        .commit_raw_overlay_records_for_test(&[
            (
                mutable_overlay_entry_address(&duplicate_key),
                duplicate_record.clone(),
            ),
            (
                mutable_overlay_entry_address(&conflict_key),
                t188_20_current_entry(conflict_key.clone(), 1, b"old"),
            ),
            ([0x40; 32], b"loom.store.mutable-overlay.entry.v3".to_vec()),
            ([0x41; 32], b"unknown-source-family".to_vec()),
        ])
        .unwrap();
    t188_20_commit_current_root_preserving_overlay(
        &store,
        &[
            (
                mutable_overlay_entry_address(&duplicate_key),
                duplicate_record,
            ),
            (
                mutable_overlay_entry_address(&conflict_key),
                t188_20_current_entry(conflict_key, 2, b"new"),
            ),
        ],
    );

    let report = store.source_layout_discovery_report().unwrap();
    let malformed = t188_20_entry(
        &report,
        SourceLayoutFamily::CurrentEntry,
        SourceLayoutDecodeState::Malformed,
    );
    assert!(
        malformed
            .rejection_reason
            .as_deref()
            .unwrap()
            .contains("truncated")
    );
    let unknown = t188_20_entry(
        &report,
        SourceLayoutFamily::Unknown,
        SourceLayoutDecodeState::UnknownFamily,
    );
    assert_eq!(
        unknown.rejection_reason.as_deref(),
        Some("unknown source-layout family")
    );
    let conflicts = report
        .entries
        .iter()
        .filter(|entry| entry.decode_state == SourceLayoutDecodeState::Conflict)
        .map(|entry| entry.rejection_reason.as_deref().unwrap())
        .collect::<BTreeSet<_>>();
    assert!(conflicts.contains("duplicate equivalent source-layout records"));
    assert!(conflicts.contains("duplicate conflicting source-layout records"));
}

#[test]
fn t188_20_source_layout_discovery_distinguishes_absent_and_malformed_required_state() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let absent = store.source_layout_discovery_report().unwrap();
    assert!(
        absent
            .entries
            .iter()
            .filter(|entry| entry.decode_state == SourceLayoutDecodeState::Absent)
            .any(|entry| entry.family == SourceLayoutFamily::CurrentRootPointer)
    );
    assert!(
        absent
            .entries
            .iter()
            .all(|entry| entry.decode_state != SourceLayoutDecodeState::Malformed)
    );

    store.inner.lock().unwrap().control_root = Some(Digest::blake3(b"missing-control-root"));
    let malformed = store.source_layout_discovery_report().unwrap();
    let control = t188_20_entry(
        &malformed,
        SourceLayoutFamily::Control,
        SourceLayoutDecodeState::Malformed,
    );
    assert_eq!(
        control.rejection_reason.as_deref(),
        Some("control-plane root object missing")
    );
}

#[test]
fn idempotency_family_roots_do_not_fall_back_to_stale_legacy_overlay() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let key = OverlayKey::from_segments([
        b"workspace",
        &[62; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"MX-449",
    ])
    .unwrap();
    let canonical_mutable_key = "canonical-mutable-idempotency-authoritative";
    let stale_mutable_key = "stale-mutable-idempotency";
    let canonical_mutable_digest =
        mutable_overlay_idempotency_request_digest(&key, b"canonical-mutable");
    let stale_mutable_digest = mutable_overlay_idempotency_request_digest(&key, b"stale-mutable");
    let canonical_mutable_token = loom_core::OverlayOwnerToken::from_bytes([84; 32]);
    let stale_mutable_token = loom_core::OverlayOwnerToken::from_bytes([85; 32]);
    let canonical_workflow_key = b"canonical-workflow-idempotency-authoritative";
    let stale_workflow_key = b"stale-workflow-idempotency";
    let canonical_workflow_digest = Digest::blake3(b"canonical-workflow");
    let stale_workflow_digest = Digest::blake3(b"stale-workflow");
    let canonical_receipt = CommitReceipt {
        generation: OverlayGeneration::new(13),
        root_after: Digest::blake3(b"canonical-workflow-root-authoritative"),
        writes: Vec::new(),
        operation_identities: Vec::new(),
        revision_identities: Vec::new(),
        audit_sequences: Vec::new(),
        retained_sequences: Vec::new(),
        delivery_receipts: Vec::new(),
        post_commit_delta: None,
        replayed: false,
    };
    let stale_receipt = CommitReceipt {
        generation: OverlayGeneration::new(14),
        root_after: Digest::blake3(b"stale-workflow-root"),
        writes: Vec::new(),
        operation_identities: Vec::new(),
        revision_identities: Vec::new(),
        audit_sequences: Vec::new(),
        retained_sequences: Vec::new(),
        delivery_receipts: Vec::new(),
        post_commit_delta: None,
        replayed: false,
    };
    store
        .commit_raw_overlay_records_for_test(&[
            (
                mutable_overlay_idempotency_address(stale_mutable_key),
                encode_mutable_overlay_idempotency_record(
                    &stale_mutable_digest,
                    &stale_mutable_token,
                ),
            ),
            (
                mutable_overlay_transaction_idempotency_address(stale_workflow_key),
                encode_workflow_transaction_idempotency_record(
                    &stale_workflow_digest,
                    &stale_receipt,
                )
                .unwrap(),
            ),
        ])
        .unwrap();
    let stale_overlay_root = store.inner.lock().unwrap().overlay_root;
    store.inner.lock().unwrap().overlay_root = None;
    store
        .commit_family_root_records_for_test(
            MUTABLE_IDEMPOTENCY_FAMILY_ID,
            &[(
                mutable_overlay_idempotency_address(canonical_mutable_key),
                encode_mutable_overlay_idempotency_record(
                    &canonical_mutable_digest,
                    &canonical_mutable_token,
                ),
            )],
        )
        .unwrap();
    store
        .commit_family_root_records_for_test(
            WORKFLOW_IDEMPOTENCY_FAMILY_ID,
            &[(
                mutable_overlay_transaction_idempotency_address(canonical_workflow_key),
                encode_workflow_transaction_idempotency_record(
                    &canonical_workflow_digest,
                    &canonical_receipt,
                )
                .unwrap(),
            )],
        )
        .unwrap();
    store.inner.lock().unwrap().overlay_root = stale_overlay_root;

    let (mutable_idempotency_root, workflow_idempotency_root, overlay_root) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.mutable_idempotency_root,
            inner.workflow_idempotency_root,
            inner.overlay_root,
        )
    };

    assert!(mutable_idempotency_root.is_some());
    assert!(workflow_idempotency_root.is_some());
    assert!(overlay_root.is_some());
    assert!(
        store
            .mutable_overlay_record_payload(&mutable_overlay_idempotency_address(stale_mutable_key))
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .mutable_overlay_record_payload(&mutable_overlay_transaction_idempotency_address(
                stale_workflow_key
            ))
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .mutable_overlay_idempotency_record(stale_mutable_key)
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .workflow_transaction_idempotency_record(stale_workflow_key)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .mutable_overlay_idempotency_record(canonical_mutable_key)
            .unwrap()
            .unwrap()
            .owner_token
            .as_bytes(),
        canonical_mutable_token.as_bytes()
    );
    assert_eq!(
        store
            .workflow_transaction_idempotency_record(canonical_workflow_key)
            .unwrap()
            .unwrap()
            .request_digest,
        canonical_workflow_digest
    );
}

#[test]
fn idempotency_mixed_root_set_publication_fails_closed() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let key = OverlayKey::from_segments([
        b"workspace",
        &[63; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"MX-450",
    ])
    .unwrap();
    let canonical_key = "canonical-mutable-idempotency-mixed";
    let legacy_key = "legacy-mutable-idempotency-mixed";
    let canonical_token = loom_core::OverlayOwnerToken::from_bytes([86; 32]);
    let legacy_token = loom_core::OverlayOwnerToken::from_bytes([87; 32]);
    let canonical_digest = mutable_overlay_idempotency_request_digest(&key, b"canonical");
    let legacy_digest = mutable_overlay_idempotency_request_digest(&key, b"legacy");
    store
        .commit_family_root_records_for_test(
            MUTABLE_IDEMPOTENCY_FAMILY_ID,
            &[(
                mutable_overlay_idempotency_address(canonical_key),
                encode_mutable_overlay_idempotency_record(&canonical_digest, &canonical_token),
            )],
        )
        .unwrap();

    let error = store
        .commit_raw_overlay_records_for_test(&[(
            mutable_overlay_idempotency_address(legacy_key),
            encode_mutable_overlay_idempotency_record(&legacy_digest, &legacy_token),
        )])
        .unwrap_err();

    assert_eq!(error.code, Code::CorruptObject);
    assert_eq!(
        store
            .mutable_overlay_idempotency_record(canonical_key)
            .unwrap()
            .unwrap()
            .owner_token
            .as_bytes(),
        canonical_token.as_bytes()
    );
    assert!(
        store
            .mutable_overlay_idempotency_record(legacy_key)
            .unwrap()
            .is_none()
    );
}

fn audit_retention_family_records(map: &BTreeMap<Vec<u8>, Vec<u8>>) -> Vec<([u8; 32], Vec<u8>)> {
    map.iter()
        .map(|(key, value)| {
            (
                audit_retention_record_address(key),
                encode_audit_retention_record(key, value),
            )
        })
        .collect()
}

fn mvcc_generation_family_record(
    generation: OverlayGeneration,
    immutable_base_root: Option<Digest>,
) -> ([u8; 32], Vec<u8>) {
    let record = MvccGenerationRecord {
        generation,
        immutable_base_root,
    };
    (
        mvcc_generation_record_address(generation),
        encode_mvcc_generation_record(&record),
    )
}

fn retention_index_test_key(id: &str) -> OverlayKey {
    OverlayKey::from_segments([
        b"workspace",
        &[65; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        id.as_bytes(),
    ])
    .unwrap()
}

fn retention_index_family_record(
    target: &OverlayKey,
    retention_class: &[u8],
    expires_at_ms: Option<u64>,
) -> ([u8; 32], Vec<u8>) {
    let record = RetentionIndexRecord {
        target: target.clone(),
        retention_class: retention_class.to_vec(),
        expires_at_ms,
    };
    (
        retention_index_record_address(target),
        encode_retention_index_record(&record),
    )
}

fn checkpoint_index_family_record(
    checkpoint_id: &[u8],
    generation: OverlayGeneration,
    base_root: Option<Digest>,
    retained_root: Option<PageId>,
) -> ([u8; 32], Vec<u8>) {
    let record = CheckpointIndexRecord {
        checkpoint_id: checkpoint_id.to_vec(),
        generation,
        base_root,
        retained_root,
    };
    (
        checkpoint_index_record_address(checkpoint_id),
        encode_checkpoint_index_record(&record),
    )
}

fn reclaim_index_family_record(
    reclaim_key: &[u8],
    blocker: &[u8],
    blocked_page: Option<PageId>,
    blocked_object: Option<Digest>,
) -> ([u8; 32], Vec<u8>) {
    let record = ReclaimIndexRecord {
        reclaim_key: reclaim_key.to_vec(),
        blocker: blocker.to_vec(),
        blocked_page,
        blocked_object,
    };
    (
        reclaim_index_record_address(reclaim_key),
        encode_reclaim_index_record(&record),
    )
}

fn delta_pack_advisory_family_record(
    advisory_key: &[u8],
    kind: DeltaPackAdvisoryKind,
    generation: OverlayGeneration,
    source_root: Option<Digest>,
    estimated_pages: u64,
    stale: bool,
) -> ([u8; 32], Vec<u8>) {
    let record = DeltaPackAdvisoryRecord {
        advisory_key: advisory_key.to_vec(),
        kind,
        generation,
        source_root,
        estimated_pages,
        stale,
    };
    (
        delta_pack_advisory_record_address(advisory_key),
        encode_delta_pack_advisory_record(&record),
    )
}

#[test]
fn mvcc_generation_routes_through_catalog_family_root() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let generation = OverlayGeneration::new(7);
    let base_root = Digest::blake3(b"mvcc-generation-base");
    let record = mvcc_generation_family_record(generation, Some(base_root));

    store
        .commit_family_root_records_for_test(MVCC_GENERATION_FAMILY_ID, &[record])
        .unwrap();
    let (
        region_table_root,
        page_count,
        root_catalog_root,
        mvcc_generation_root,
        audit_retention_root,
        overlay_root,
        current_record_root,
    ) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.region_table_root.unwrap(),
            inner.page_count,
            inner.root_catalog_root,
            inner.mvcc_generation_root,
            inner.audit_retention_root,
            inner.overlay_root,
            inner.current_record_root,
        )
    };
    let mut backing = shared.clone();
    let region = read_region_table(&mut backing, region_table_root, page_count).unwrap();
    let catalog = read_root_catalog(&mut backing, root_catalog_root.unwrap(), page_count).unwrap();

    assert_eq!(region.root_catalog_root, root_catalog_root);
    assert_eq!(region.overlay_root, None);
    assert_eq!(overlay_root, None);
    assert_eq!(current_record_root, None);
    assert_eq!(audit_retention_root, None);
    assert_eq!(store.control_root(), None);
    assert_eq!(
        catalog
            .entries
            .iter()
            .find(|entry| entry.family_id == MVCC_GENERATION_FAMILY_ID)
            .map(|entry| entry.root),
        mvcc_generation_root
    );
    assert!(mvcc_generation_root.is_some());
    assert_eq!(
        store
            .mutable_overlay_record_payload(&mvcc_generation_record_address(generation))
            .unwrap(),
        None
    );
    assert_eq!(
        store.mvcc_generation_record(generation).unwrap().unwrap(),
        MvccGenerationRecord {
            generation,
            immutable_base_root: Some(base_root),
        }
    );
}

#[test]
fn mvcc_generation_catalog_family_survives_reopen_without_current_hydration() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let generation = OverlayGeneration::new(9);
    let base_root = Digest::blake3(b"mvcc-generation-reopen-base");
    store
        .commit_family_root_records_for_test(
            MVCC_GENERATION_FAMILY_ID,
            &[mvcc_generation_family_record(generation, Some(base_root))],
        )
        .unwrap();
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let stats = reopened.io_stats().unwrap();
    let (mvcc_generation_root, root_catalog_root, overlay_root, current_record_root) = {
        let inner = reopened.inner.lock().unwrap();
        (
            inner.mvcc_generation_root,
            inner.root_catalog_root,
            inner.overlay_root,
            inner.current_record_root,
        )
    };

    assert!(mvcc_generation_root.is_some());
    assert!(root_catalog_root.is_some());
    assert_eq!(overlay_root, None);
    assert_eq!(current_record_root, None);
    assert_eq!(stats.open_mutable_current_records_loaded, 0);
    assert_eq!(stats.open_mutable_control_records_skipped, 0);
    assert_eq!(
        reopened
            .mvcc_generation_record(generation)
            .unwrap()
            .unwrap(),
        MvccGenerationRecord {
            generation,
            immutable_base_root: Some(base_root),
        }
    );
}

#[test]
fn absent_mvcc_generation_catalog_family_reads_empty() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();

    assert_eq!(
        store
            .mvcc_generation_record(OverlayGeneration::new(1))
            .unwrap(),
        None
    );
}

#[test]
fn mvcc_generation_reader_does_not_fall_back_to_stale_legacy_overlay() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let generation = OverlayGeneration::new(11);
    let stale_root = Digest::blake3(b"mvcc-generation-stale-base");
    let stale_record = mvcc_generation_family_record(generation, Some(stale_root));
    store
        .commit_raw_overlay_records_for_test(&[stale_record.clone()])
        .unwrap();

    assert_eq!(
        decode_mvcc_generation_record(
            &store
                .mutable_overlay_record_payload(&stale_record.0)
                .unwrap()
                .unwrap()
        )
        .unwrap()
        .immutable_base_root,
        Some(stale_root)
    );
    assert_eq!(store.mvcc_generation_record(generation).unwrap(), None);
}

#[test]
fn mvcc_generation_key_mismatch_is_rejected() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let requested = OverlayGeneration::new(12);
    let stored = OverlayGeneration::new(13);

    store
        .commit_family_root_records_for_test(
            MVCC_GENERATION_FAMILY_ID,
            &[(
                mvcc_generation_record_address(requested),
                encode_mvcc_generation_record(&MvccGenerationRecord {
                    generation: stored,
                    immutable_base_root: None,
                }),
            )],
        )
        .unwrap();

    assert_eq!(
        store.mvcc_generation_record(requested).unwrap_err().code,
        Code::CorruptObject
    );
}

#[test]
fn mvcc_generation_mixed_root_set_publication_fails_closed() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    store
        .commit_family_root_records_for_test(
            MVCC_GENERATION_FAMILY_ID,
            &[mvcc_generation_family_record(
                OverlayGeneration::new(14),
                None,
            )],
        )
        .unwrap();

    let error = store
        .commit_raw_overlay_records_for_test(&[mvcc_generation_family_record(
            OverlayGeneration::new(15),
            None,
        )])
        .unwrap_err();

    assert_eq!(error.code, Code::CorruptObject);
}

#[test]
fn retention_index_routes_through_catalog_family_root() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let target = retention_index_test_key("MX-510");
    let record = retention_index_family_record(&target, b"retain-tombstones.v1", Some(123_456));

    store
        .commit_family_root_records_for_test(RETENTION_INDEX_FAMILY_ID, &[record])
        .unwrap();
    let (
        region_table_root,
        page_count,
        root_catalog_root,
        retention_index_root,
        mvcc_generation_root,
        retained_history_root,
        overlay_root,
        current_record_root,
    ) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.region_table_root.unwrap(),
            inner.page_count,
            inner.root_catalog_root,
            inner.retention_index_root,
            inner.mvcc_generation_root,
            inner.retained_history_root,
            inner.overlay_root,
            inner.current_record_root,
        )
    };
    let mut backing = shared.clone();
    let region = read_region_table(&mut backing, region_table_root, page_count).unwrap();
    let catalog = read_root_catalog(&mut backing, root_catalog_root.unwrap(), page_count).unwrap();

    assert_eq!(region.root_catalog_root, root_catalog_root);
    assert_eq!(region.overlay_root, None);
    assert_eq!(overlay_root, None);
    assert_eq!(current_record_root, None);
    assert_eq!(mvcc_generation_root, None);
    assert_eq!(retained_history_root, None);
    assert_eq!(store.control_root(), None);
    assert_eq!(
        catalog
            .entries
            .iter()
            .find(|entry| entry.family_id == RETENTION_INDEX_FAMILY_ID)
            .map(|entry| entry.root),
        retention_index_root
    );
    assert!(retention_index_root.is_some());
    assert!(
        catalog
            .entries
            .iter()
            .all(|entry| !matches!(entry.family_id, 0x0200 | 0x0220 | 0x0230))
    );
    assert_eq!(
        store
            .mutable_overlay_record_payload(&retention_index_record_address(&target))
            .unwrap(),
        None
    );
    assert_eq!(
        store.retention_index_record(&target).unwrap().unwrap(),
        RetentionIndexRecord {
            target,
            retention_class: b"retain-tombstones.v1".to_vec(),
            expires_at_ms: Some(123_456),
        }
    );
}

#[test]
fn retention_index_catalog_family_survives_reopen_without_current_hydration() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let target = retention_index_test_key("MX-511");
    store
        .commit_family_root_records_for_test(
            RETENTION_INDEX_FAMILY_ID,
            &[retention_index_family_record(
                &target,
                b"expire-after-window.v1",
                Some(987_654),
            )],
        )
        .unwrap();
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let stats = reopened.io_stats().unwrap();
    let (retention_index_root, root_catalog_root, overlay_root, current_record_root) = {
        let inner = reopened.inner.lock().unwrap();
        (
            inner.retention_index_root,
            inner.root_catalog_root,
            inner.overlay_root,
            inner.current_record_root,
        )
    };

    assert!(retention_index_root.is_some());
    assert!(root_catalog_root.is_some());
    assert_eq!(overlay_root, None);
    assert_eq!(current_record_root, None);
    assert_eq!(stats.open_mutable_current_records_loaded, 0);
    assert_eq!(stats.open_mutable_control_records_skipped, 0);
    assert_eq!(
        reopened.retention_index_record(&target).unwrap().unwrap(),
        RetentionIndexRecord {
            target,
            retention_class: b"expire-after-window.v1".to_vec(),
            expires_at_ms: Some(987_654),
        }
    );
}

#[test]
fn absent_retention_index_catalog_family_reads_empty() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let target = retention_index_test_key("MX-512");

    assert_eq!(store.retention_index_record(&target).unwrap(), None);
}

#[test]
fn ordinary_exact_read_does_not_consult_retention_index() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let target = retention_index_test_key("MX-513");
    store
        .commit_family_root_records_for_test(
            RETENTION_INDEX_FAMILY_ID,
            &[retention_index_family_record(
                &target,
                b"retention-only-not-current",
                Some(42),
            )],
        )
        .unwrap();

    let snapshot = store.mutable_overlay_snapshot().unwrap();
    let read = snapshot
        .read_composite(&target, |_| Ok(Some(b"base-value".to_vec())))
        .unwrap();

    assert_eq!(read.as_deref(), Some(&b"base-value"[..]));
    assert_eq!(store.mutable_overlay_current_entry(&target).unwrap(), None);
    assert_eq!(
        store
            .retention_index_record(&target)
            .unwrap()
            .unwrap()
            .retention_class,
        b"retention-only-not-current"
    );
}

#[test]
fn retention_index_is_independent_from_current_and_retained_liveness_roots() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let target = retention_index_test_key("MX-514");
    let history_key = b"pages/workspace/retention-independent-history".to_vec();
    store
        .commit_family_root_records_for_test(
            RETENTION_INDEX_FAMILY_ID,
            &[retention_index_family_record(
                &target,
                b"retain-with-current",
                None,
            )],
        )
        .unwrap();
    let mut overlay = loom_core::MutableOverlay::new();
    overlay
        .put_value(target.clone(), None, b"current-authority".to_vec())
        .unwrap();
    let latest = overlay.current_entry(&target).unwrap();
    store
        .commit_current_root_records_for_test(&[(
            mutable_overlay_entry_address(&target),
            encode_mutable_overlay_entry(&latest),
        )])
        .unwrap();
    store
        .commit_family_root_records_for_test(
            RETAINED_HISTORY_FAMILY_ID,
            &[
                (
                    retained_history_head_address(&history_key),
                    encode_retained_history_head(&history_key, 1),
                ),
                (
                    retained_history_record_address(&history_key, 1),
                    encode_retained_history_entry(&history_key, 1, b"retained-authority"),
                ),
            ],
        )
        .unwrap();
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();

    let (
        retention_index_root,
        retained_history_root,
        current_record_root,
        mvcc_generation_root,
        overlay_root,
    ) = {
        let inner = reopened.inner.lock().unwrap();
        (
            inner.retention_index_root,
            inner.retained_history_root,
            inner.current_record_root,
            inner.mvcc_generation_root,
            inner.overlay_root,
        )
    };
    let snapshot = reopened.mutable_overlay_snapshot().unwrap();
    let read = snapshot.read_composite(&target, |_| Ok(None)).unwrap();

    assert!(retention_index_root.is_some());
    assert!(retained_history_root.is_some());
    assert!(current_record_root.is_some());
    assert_ne!(retention_index_root, retained_history_root);
    assert_ne!(retention_index_root, current_record_root);
    assert_eq!(mvcc_generation_root, None);
    assert_eq!(overlay_root, None);
    assert_eq!(read.as_deref(), Some(&b"current-authority"[..]));
    assert_eq!(reopened.retained_history_head(&history_key).unwrap(), 1);
    assert_eq!(
        reopened
            .retained_history_records(&history_key, 1, usize::MAX)
            .unwrap(),
        vec![b"retained-authority".to_vec()]
    );
    assert_eq!(
        reopened
            .retention_index_record(&target)
            .unwrap()
            .unwrap()
            .retention_class,
        b"retain-with-current"
    );
}

#[test]
fn retention_index_key_mismatch_is_rejected() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let requested = retention_index_test_key("MX-515");
    let stored = retention_index_test_key("MX-516");

    store
        .commit_family_root_records_for_test(
            RETENTION_INDEX_FAMILY_ID,
            &[(
                retention_index_record_address(&requested),
                encode_retention_index_record(&RetentionIndexRecord {
                    target: stored,
                    retention_class: b"wrong-key".to_vec(),
                    expires_at_ms: None,
                }),
            )],
        )
        .unwrap();

    assert_eq!(
        store.retention_index_record(&requested).unwrap_err().code,
        Code::CorruptObject
    );
}

#[test]
fn retention_index_mixed_root_set_publication_fails_closed() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let canonical = retention_index_test_key("MX-517");
    let legacy = retention_index_test_key("MX-518");
    store
        .commit_family_root_records_for_test(
            RETENTION_INDEX_FAMILY_ID,
            &[retention_index_family_record(
                &canonical,
                b"canonical-retention",
                None,
            )],
        )
        .unwrap();

    let error = store
        .commit_raw_overlay_records_for_test(&[retention_index_family_record(
            &legacy,
            b"legacy-retention",
            None,
        )])
        .unwrap_err();

    assert_eq!(error.code, Code::CorruptObject);
    assert!(store.retention_index_record(&canonical).unwrap().is_some());
    assert_eq!(store.retention_index_record(&legacy).unwrap(), None);
}

#[test]
fn checkpoint_index_routes_through_catalog_family_root() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let checkpoint_id = b"checkpoint-route";
    let base_root = Digest::blake3(b"checkpoint-route-base");
    let record = checkpoint_index_family_record(
        checkpoint_id,
        OverlayGeneration::new(21),
        Some(base_root),
        Some(PageId(77)),
    );

    store
        .commit_family_root_records_for_test(CHECKPOINT_INDEX_FAMILY_ID, &[record])
        .unwrap();
    let (
        region_table_root,
        page_count,
        root_catalog_root,
        checkpoint_index_root,
        mvcc_generation_root,
        retention_index_root,
        overlay_root,
        current_record_root,
    ) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.region_table_root.unwrap(),
            inner.page_count,
            inner.root_catalog_root,
            inner.checkpoint_index_root,
            inner.mvcc_generation_root,
            inner.retention_index_root,
            inner.overlay_root,
            inner.current_record_root,
        )
    };
    let mut backing = shared.clone();
    let region = read_region_table(&mut backing, region_table_root, page_count).unwrap();
    let catalog = read_root_catalog(&mut backing, root_catalog_root.unwrap(), page_count).unwrap();

    assert_eq!(region.root_catalog_root, root_catalog_root);
    assert_eq!(region.overlay_root, None);
    assert_eq!(overlay_root, None);
    assert_eq!(current_record_root, None);
    assert_eq!(mvcc_generation_root, None);
    assert_eq!(retention_index_root, None);
    assert_eq!(store.control_root(), None);
    assert_eq!(
        catalog
            .entries
            .iter()
            .find(|entry| entry.family_id == CHECKPOINT_INDEX_FAMILY_ID)
            .map(|entry| entry.root),
        checkpoint_index_root
    );
    assert!(checkpoint_index_root.is_some());
    assert!(
        catalog
            .entries
            .iter()
            .all(|entry| !matches!(entry.family_id, 0x0200 | 0x0210 | 0x0230))
    );
    assert_eq!(
        store
            .mutable_overlay_record_payload(&checkpoint_index_record_address(checkpoint_id))
            .unwrap(),
        None
    );
    assert_eq!(
        store
            .checkpoint_index_record(checkpoint_id)
            .unwrap()
            .unwrap(),
        CheckpointIndexRecord {
            checkpoint_id: checkpoint_id.to_vec(),
            generation: OverlayGeneration::new(21),
            base_root: Some(base_root),
            retained_root: Some(PageId(77)),
        }
    );
}

#[test]
fn checkpoint_index_catalog_family_survives_reopen_without_current_hydration() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let checkpoint_id = b"checkpoint-reopen";
    store
        .commit_family_root_records_for_test(
            CHECKPOINT_INDEX_FAMILY_ID,
            &[checkpoint_index_family_record(
                checkpoint_id,
                OverlayGeneration::new(22),
                None,
                Some(PageId(88)),
            )],
        )
        .unwrap();
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let stats = reopened.io_stats().unwrap();
    let (checkpoint_index_root, root_catalog_root, overlay_root, current_record_root) = {
        let inner = reopened.inner.lock().unwrap();
        (
            inner.checkpoint_index_root,
            inner.root_catalog_root,
            inner.overlay_root,
            inner.current_record_root,
        )
    };

    assert!(checkpoint_index_root.is_some());
    assert!(root_catalog_root.is_some());
    assert_eq!(overlay_root, None);
    assert_eq!(current_record_root, None);
    assert_eq!(stats.open_mutable_current_records_loaded, 0);
    assert_eq!(stats.open_mutable_control_records_skipped, 0);
    assert_eq!(
        reopened
            .checkpoint_index_record(checkpoint_id)
            .unwrap()
            .unwrap(),
        CheckpointIndexRecord {
            checkpoint_id: checkpoint_id.to_vec(),
            generation: OverlayGeneration::new(22),
            base_root: None,
            retained_root: Some(PageId(88)),
        }
    );
}

#[test]
fn absent_checkpoint_index_catalog_family_reads_empty() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();

    assert_eq!(
        store
            .checkpoint_index_record(b"missing-checkpoint")
            .unwrap(),
        None
    );
}

#[test]
fn checkpoint_index_metadata_does_not_replace_journal_recovery_roots() {
    let tp = TempPath::new("checkpoint-index-journal-authority");
    let checkpoint_id = b"checkpoint-journal-authority";
    let digest = {
        let store = FileStore::open(tp.path()).unwrap();
        let digest = store.put(&blob(b"journal-owned-object")).unwrap();
        store
            .commit_family_root_records_for_test(
                CHECKPOINT_INDEX_FAMILY_ID,
                &[checkpoint_index_family_record(
                    checkpoint_id,
                    OverlayGeneration::new(23),
                    Some(digest),
                    None,
                )],
            )
            .unwrap();
        digest
    };
    let bytes = std::fs::read(tp.path()).unwrap();
    let slot_a: &[u8; SLOT_SIZE as usize] = bytes[..SLOT_SIZE as usize].try_into().unwrap();
    assert_eq!(Superblock::decode(slot_a).unwrap().generation, 0);
    let mut newest: Option<journal::Roots> = None;
    for i in 0..RING_SLOTS {
        let off = (JOURNAL_OFFSET + i * journal::RECORD_SIZE as u64) as usize;
        if let Some((journal::KIND_COMMIT, roots)) =
            journal::decode(&bytes[off..off + journal::RECORD_SIZE])
            && newest.is_none_or(|known| roots.generation > known.generation)
        {
            newest = Some(roots);
        }
    }
    let newest = newest.unwrap();
    let region_table_root = newest.region_table.unwrap();
    let region_offset = (DATA_START + region_table_root.0 * PAGE_SIZE) as usize;
    let region =
        RegionTable::decode(&bytes[region_offset..region_offset + PAGE_SIZE as usize]).unwrap();

    assert!(newest.generation > Superblock::decode(slot_a).unwrap().generation);
    assert!(region.root_catalog_root.is_some());
    assert!(region.index_root.is_some());
    assert!(newest.reference.is_none());
    assert!(newest.control.is_none());

    let reopened = open_read_bytes(&bytes, "checkpoint-index-journal-reopen").unwrap();
    assert!(reopened.has(&digest).unwrap());
    assert_eq!(
        reopened
            .checkpoint_index_record(checkpoint_id)
            .unwrap()
            .unwrap()
            .base_root,
        Some(digest)
    );
}

#[test]
fn checkpoint_index_key_mismatch_is_rejected() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();

    store
        .commit_family_root_records_for_test(
            CHECKPOINT_INDEX_FAMILY_ID,
            &[(
                checkpoint_index_record_address(b"checkpoint-requested"),
                encode_checkpoint_index_record(&CheckpointIndexRecord {
                    checkpoint_id: b"checkpoint-stored".to_vec(),
                    generation: OverlayGeneration::new(24),
                    base_root: None,
                    retained_root: None,
                }),
            )],
        )
        .unwrap();

    assert_eq!(
        store
            .checkpoint_index_record(b"checkpoint-requested")
            .unwrap_err()
            .code,
        Code::CorruptObject
    );
}

#[test]
fn checkpoint_index_mixed_root_set_publication_fails_closed() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    store
        .commit_family_root_records_for_test(
            CHECKPOINT_INDEX_FAMILY_ID,
            &[checkpoint_index_family_record(
                b"checkpoint-canonical",
                OverlayGeneration::new(25),
                None,
                None,
            )],
        )
        .unwrap();

    let error = store
        .commit_raw_overlay_records_for_test(&[checkpoint_index_family_record(
            b"checkpoint-legacy",
            OverlayGeneration::new(26),
            None,
            None,
        )])
        .unwrap_err();

    assert_eq!(error.code, Code::CorruptObject);
    assert!(
        store
            .checkpoint_index_record(b"checkpoint-canonical")
            .unwrap()
            .is_some()
    );
    assert_eq!(
        store.checkpoint_index_record(b"checkpoint-legacy").unwrap(),
        None
    );
}

#[test]
fn reclaim_index_routes_through_catalog_family_root() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let reclaim_key = b"stale-page/blocker-route";
    let blocked_object = Digest::blake3(b"blocked-object-route");
    let record = reclaim_index_family_record(
        reclaim_key,
        b"recovery-generation-floor",
        Some(PageId(91)),
        Some(blocked_object),
    );

    store
        .commit_family_root_records_for_test(RECLAIM_INDEX_FAMILY_ID, &[record])
        .unwrap();
    let (
        region_table_root,
        page_count,
        root_catalog_root,
        reclaim_index_root,
        checkpoint_index_root,
        retention_index_root,
        mvcc_generation_root,
        overlay_root,
        current_record_root,
        freemap,
    ) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.region_table_root.unwrap(),
            inner.page_count,
            inner.root_catalog_root,
            inner.reclaim_index_root,
            inner.checkpoint_index_root,
            inner.retention_index_root,
            inner.mvcc_generation_root,
            inner.overlay_root,
            inner.current_record_root,
            inner.freemap,
        )
    };
    let mut backing = shared.clone();
    let region = read_region_table(&mut backing, region_table_root, page_count).unwrap();
    let catalog = read_root_catalog(&mut backing, root_catalog_root.unwrap(), page_count).unwrap();

    assert_eq!(region.root_catalog_root, root_catalog_root);
    assert_eq!(region.overlay_root, None);
    assert_eq!(region.freemap_root, None);
    assert_eq!(overlay_root, None);
    assert_eq!(current_record_root, None);
    assert_eq!(checkpoint_index_root, None);
    assert_eq!(retention_index_root, None);
    assert_eq!(mvcc_generation_root, None);
    assert_eq!(freemap, None);
    assert_eq!(store.control_root(), None);
    assert_eq!(
        catalog
            .entries
            .iter()
            .find(|entry| entry.family_id == RECLAIM_INDEX_FAMILY_ID)
            .map(|entry| entry.root),
        reclaim_index_root
    );
    assert!(reclaim_index_root.is_some());
    assert!(
        catalog
            .entries
            .iter()
            .all(|entry| !matches!(entry.family_id, 0x0200 | 0x0210 | 0x0220))
    );
    assert_eq!(
        store
            .mutable_overlay_record_payload(&reclaim_index_record_address(reclaim_key))
            .unwrap(),
        None
    );
    assert_eq!(
        store.reclaim_index_record(reclaim_key).unwrap().unwrap(),
        ReclaimIndexRecord {
            reclaim_key: reclaim_key.to_vec(),
            blocker: b"recovery-generation-floor".to_vec(),
            blocked_page: Some(PageId(91)),
            blocked_object: Some(blocked_object),
        }
    );
}

#[test]
fn reclaim_index_catalog_family_survives_reopen_without_current_hydration() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let reclaim_key = b"stale-page/reopen";
    store
        .commit_family_root_records_for_test(
            RECLAIM_INDEX_FAMILY_ID,
            &[reclaim_index_family_record(
                reclaim_key,
                b"checkpoint-window",
                Some(PageId(102)),
                None,
            )],
        )
        .unwrap();
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let stats = reopened.io_stats().unwrap();
    let (reclaim_index_root, root_catalog_root, overlay_root, current_record_root) = {
        let inner = reopened.inner.lock().unwrap();
        (
            inner.reclaim_index_root,
            inner.root_catalog_root,
            inner.overlay_root,
            inner.current_record_root,
        )
    };

    assert!(reclaim_index_root.is_some());
    assert!(root_catalog_root.is_some());
    assert_eq!(overlay_root, None);
    assert_eq!(current_record_root, None);
    assert_eq!(stats.open_mutable_current_records_loaded, 0);
    assert_eq!(stats.open_mutable_control_records_skipped, 0);
    assert_eq!(
        reopened.reclaim_index_record(reclaim_key).unwrap().unwrap(),
        ReclaimIndexRecord {
            reclaim_key: reclaim_key.to_vec(),
            blocker: b"checkpoint-window".to_vec(),
            blocked_page: Some(PageId(102)),
            blocked_object: None,
        }
    );
}

#[test]
fn absent_reclaim_index_catalog_family_reads_empty() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();

    assert_eq!(
        store.reclaim_index_record(b"missing-reclaim").unwrap(),
        None
    );
}

#[test]
fn reclaim_index_does_not_participate_in_exact_reads_or_semantic_liveness() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let target = retention_index_test_key("MX-519");
    let blocked_object = Digest::blake3(b"reclaim-index-not-live-object");
    store
        .commit_family_root_records_for_test(
            RECLAIM_INDEX_FAMILY_ID,
            &[reclaim_index_family_record(
                target.as_bytes(),
                b"candidate-only",
                Some(PageId(119)),
                Some(blocked_object),
            )],
        )
        .unwrap();

    let snapshot = store.mutable_overlay_snapshot().unwrap();
    let read = snapshot
        .read_composite(&target, |_| Ok(Some(b"base-value".to_vec())))
        .unwrap();

    assert_eq!(read.as_deref(), Some(&b"base-value"[..]));
    assert_eq!(store.mutable_overlay_current_entry(&target).unwrap(), None);
    assert!(!store.has(&blocked_object).unwrap());
    assert_eq!(
        store
            .reclaim_index_record(target.as_bytes())
            .unwrap()
            .unwrap()
            .blocker,
        b"candidate-only"
    );
}

#[test]
fn reclaim_index_is_independent_from_physical_freemap() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let (before_freemap, before_region_table_root) = {
        let inner = store.inner.lock().unwrap();
        (inner.freemap, inner.region_table_root)
    };

    store
        .commit_family_root_records_for_test(
            RECLAIM_INDEX_FAMILY_ID,
            &[reclaim_index_family_record(
                b"stale-page/freemap-independent",
                b"catalog-only",
                Some(PageId(130)),
                None,
            )],
        )
        .unwrap();

    let (after_freemap, after_region_table_root, page_count, reclaim_index_root) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.freemap,
            inner.region_table_root,
            inner.page_count,
            inner.reclaim_index_root,
        )
    };
    let mut backing = shared.clone();
    let region =
        read_region_table(&mut backing, after_region_table_root.unwrap(), page_count).unwrap();

    assert_eq!(before_freemap, None);
    assert_eq!(before_region_table_root, None);
    assert_eq!(after_freemap, None);
    assert_eq!(region.freemap_root, None);
    assert!(reclaim_index_root.is_some());
}

#[test]
fn reclaim_index_key_mismatch_is_rejected() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();

    store
        .commit_family_root_records_for_test(
            RECLAIM_INDEX_FAMILY_ID,
            &[(
                reclaim_index_record_address(b"reclaim-requested"),
                encode_reclaim_index_record(&ReclaimIndexRecord {
                    reclaim_key: b"reclaim-stored".to_vec(),
                    blocker: b"wrong-key".to_vec(),
                    blocked_page: None,
                    blocked_object: None,
                }),
            )],
        )
        .unwrap();

    assert_eq!(
        store
            .reclaim_index_record(b"reclaim-requested")
            .unwrap_err()
            .code,
        Code::CorruptObject
    );
}

#[test]
fn reclaim_index_mixed_root_set_publication_fails_closed() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    store
        .commit_family_root_records_for_test(
            RECLAIM_INDEX_FAMILY_ID,
            &[reclaim_index_family_record(
                b"reclaim-canonical",
                b"canonical-reclaim",
                Some(PageId(141)),
                None,
            )],
        )
        .unwrap();

    let error = store
        .commit_raw_overlay_records_for_test(&[reclaim_index_family_record(
            b"reclaim-legacy",
            b"legacy-reclaim",
            Some(PageId(142)),
            None,
        )])
        .unwrap_err();

    assert_eq!(error.code, Code::CorruptObject);
    assert!(
        store
            .reclaim_index_record(b"reclaim-canonical")
            .unwrap()
            .is_some()
    );
    assert_eq!(store.reclaim_index_record(b"reclaim-legacy").unwrap(), None);
}

#[test]
fn delta_pack_advisory_routes_through_advisory_catalog_family() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let advisory_key = b"delta-pack/candidate-route";
    let source_root = Digest::blake3(b"delta-pack-source-root");

    store
        .commit_family_root_records_for_test(
            DELTA_PACK_CANDIDATE_FAMILY_ID,
            &[delta_pack_advisory_family_record(
                advisory_key,
                DeltaPackAdvisoryKind::Candidate,
                OverlayGeneration::new(31),
                Some(source_root),
                17,
                false,
            )],
        )
        .unwrap();

    let (region_table_root, page_count, root_catalog_root, hydrated_authoritative_roots) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.region_table_root.unwrap(),
            inner.page_count,
            inner.root_catalog_root,
            (
                inner.retention_index_root,
                inner.checkpoint_index_root,
                inner.reclaim_index_root,
                inner.mvcc_generation_root,
                inner.overlay_root,
                inner.current_record_root,
                inner.freemap,
            ),
        )
    };
    let mut backing = shared.clone();
    let region = read_region_table(&mut backing, region_table_root, page_count).unwrap();
    let catalog = read_root_catalog(&mut backing, root_catalog_root.unwrap(), page_count).unwrap();
    let advisory_entry = catalog
        .entries
        .iter()
        .find(|entry| entry.family_id == DELTA_PACK_CANDIDATE_FAMILY_ID)
        .unwrap();

    assert_eq!(region.root_catalog_root, root_catalog_root);
    assert_eq!(region.overlay_root, None);
    assert_eq!(region.freemap_root, None);
    assert_eq!(
        hydrated_authoritative_roots,
        (None, None, None, None, None, None, None)
    );
    assert_eq!(store.control_root(), None);
    assert_eq!(advisory_entry.flags, 0x0002);
    assert!(
        catalog
            .entries
            .iter()
            .all(|entry| !matches!(entry.family_id, 0x0200 | 0x0210 | 0x0220 | 0x0230))
    );
    assert_eq!(
        store
            .mutable_overlay_record_payload(&delta_pack_advisory_record_address(advisory_key))
            .unwrap(),
        None
    );
    assert_eq!(
        store
            .delta_pack_advisory_record(advisory_key)
            .unwrap()
            .unwrap(),
        DeltaPackAdvisoryRecord {
            advisory_key: advisory_key.to_vec(),
            kind: DeltaPackAdvisoryKind::Candidate,
            generation: OverlayGeneration::new(31),
            source_root: Some(source_root),
            estimated_pages: 17,
            stale: false,
        }
    );
}

#[test]
fn absent_delta_pack_advisory_state_is_harmless() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let target = retention_index_test_key("MX-520");
    let snapshot = store.mutable_overlay_snapshot().unwrap();
    let read = snapshot
        .read_composite(&target, |_| Ok(Some(b"base-value".to_vec())))
        .unwrap();

    assert_eq!(
        store
            .delta_pack_advisory_record(b"missing-advisory")
            .unwrap(),
        None
    );
    assert_eq!(read.as_deref(), Some(&b"base-value"[..]));
    assert_eq!(store.mutable_overlay_current_entry(&target).unwrap(), None);
}

#[test]
fn stale_delta_pack_advisory_state_is_harmless_to_reads_and_liveness() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let target = retention_index_test_key("MX-521");
    let stale_source = Digest::blake3(b"stale-delta-pack-source");
    store
        .commit_family_root_records_for_test(
            DELTA_PACK_CANDIDATE_FAMILY_ID,
            &[delta_pack_advisory_family_record(
                target.as_bytes(),
                DeltaPackAdvisoryKind::Debt,
                OverlayGeneration::new(1),
                Some(stale_source),
                999,
                true,
            )],
        )
        .unwrap();

    let snapshot = store.mutable_overlay_snapshot().unwrap();
    let read = snapshot
        .read_composite(&target, |_| Ok(Some(b"base-value".to_vec())))
        .unwrap();

    assert_eq!(read.as_deref(), Some(&b"base-value"[..]));
    assert_eq!(store.mutable_overlay_current_entry(&target).unwrap(), None);
    assert!(!store.has(&stale_source).unwrap());
    assert!(
        store
            .delta_pack_advisory_record(target.as_bytes())
            .unwrap()
            .unwrap()
            .stale
    );
}

#[test]
fn rebuilding_delta_pack_advisory_restores_equivalent_state() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let advisory_key = b"delta-pack/rebuild-equivalent";
    store
        .commit_family_root_records_for_test(
            DELTA_PACK_CANDIDATE_FAMILY_ID,
            &[delta_pack_advisory_family_record(
                advisory_key,
                DeltaPackAdvisoryKind::Candidate,
                OverlayGeneration::new(2),
                None,
                44,
                true,
            )],
        )
        .unwrap();
    let rebuilt_source = Digest::blake3(b"rebuilt-delta-pack-source");
    store
        .commit_family_root_records_for_test(
            DELTA_PACK_CANDIDATE_FAMILY_ID,
            &[delta_pack_advisory_family_record(
                advisory_key,
                DeltaPackAdvisoryKind::Candidate,
                OverlayGeneration::new(3),
                Some(rebuilt_source),
                44,
                false,
            )],
        )
        .unwrap();

    assert_eq!(
        store
            .delta_pack_advisory_record(advisory_key)
            .unwrap()
            .unwrap(),
        DeltaPackAdvisoryRecord {
            advisory_key: advisory_key.to_vec(),
            kind: DeltaPackAdvisoryKind::Candidate,
            generation: OverlayGeneration::new(3),
            source_root: Some(rebuilt_source),
            estimated_pages: 44,
            stale: false,
        }
    );
}

#[test]
fn malformed_present_delta_pack_advisory_metadata_fails_when_read() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let advisory_key = b"delta-pack/malformed";
    store
        .commit_family_root_records_for_test(
            DELTA_PACK_CANDIDATE_FAMILY_ID,
            &[(
                delta_pack_advisory_record_address(advisory_key),
                b"bad".to_vec(),
            )],
        )
        .unwrap();
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let target = retention_index_test_key("MX-522");
    let snapshot = reopened.mutable_overlay_snapshot().unwrap();
    let read = snapshot
        .read_composite(&target, |_| Ok(Some(b"base-value".to_vec())))
        .unwrap();

    assert_eq!(read.as_deref(), Some(&b"base-value"[..]));
    assert_eq!(
        reopened
            .delta_pack_advisory_record(advisory_key)
            .unwrap_err()
            .code,
        Code::CorruptObject
    );
}

#[test]
fn delta_pack_advisory_mixed_root_set_publication_fails_closed() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    store
        .commit_family_root_records_for_test(
            DELTA_PACK_CANDIDATE_FAMILY_ID,
            &[delta_pack_advisory_family_record(
                b"delta-pack/canonical",
                DeltaPackAdvisoryKind::Candidate,
                OverlayGeneration::new(4),
                None,
                8,
                false,
            )],
        )
        .unwrap();

    let error = store
        .commit_raw_overlay_records_for_test(&[delta_pack_advisory_family_record(
            b"delta-pack/legacy",
            DeltaPackAdvisoryKind::Debt,
            OverlayGeneration::new(4),
            None,
            8,
            false,
        )])
        .unwrap_err();

    assert_eq!(error.code, Code::CorruptObject);
    assert!(
        store
            .delta_pack_advisory_record(b"delta-pack/canonical")
            .unwrap()
            .is_some()
    );
    assert_eq!(
        store
            .delta_pack_advisory_record(b"delta-pack/legacy")
            .unwrap(),
        None
    );
}

#[test]
fn canonical_production_mutable_write_publishes_direct_current_and_owner_family() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let key = retention_index_test_key("MX-523");
    let token = store
        .put_mutable_overlay_value(key.clone(), b"canonical-production-value".to_vec())
        .unwrap();
    let (
        region_table_root,
        page_count,
        root_catalog_root,
        current_record_root,
        owner_token_root,
        overlay_root,
    ) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.region_table_root.unwrap(),
            inner.page_count,
            inner.root_catalog_root,
            inner.current_record_root,
            inner.owner_token_root,
            inner.overlay_root,
        )
    };
    let mut backing = shared.clone();
    let region = read_region_table(&mut backing, region_table_root, page_count).unwrap();
    let catalog = read_root_catalog(&mut backing, root_catalog_root.unwrap(), page_count).unwrap();

    assert_eq!(region.overlay_root, None);
    assert_eq!(overlay_root, None);
    assert_eq!(region.current_record_root, current_record_root);
    assert!(current_record_root.is_some());
    assert_eq!(region.root_catalog_root, root_catalog_root);
    assert_eq!(
        catalog
            .entries
            .iter()
            .find(|entry| entry.family_id == OWNER_TOKEN_FAMILY_ID)
            .map(|entry| entry.root),
        owner_token_root
    );
    assert!(owner_token_root.is_some());
    assert_eq!(
        store.mutable_overlay_durable_owner_token(&key).unwrap(),
        Some(token.clone())
    );
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let snapshot = reopened.mutable_overlay_snapshot().unwrap();
    let read = snapshot.read_composite(&key, |_| Ok(None)).unwrap();

    assert_eq!(read.as_deref(), Some(&b"canonical-production-value"[..]));
    assert_eq!(
        reopened.mutable_overlay_durable_owner_token(&key).unwrap(),
        Some(token)
    );
    let inner = reopened.inner.lock().unwrap();
    assert_eq!(inner.overlay_root, None);
    assert!(inner.current_record_root.is_some());
    assert!(inner.owner_token_root.is_some());
}

#[test]
fn canonical_production_idempotent_write_routes_idempotency_family() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let key = retention_index_test_key("MX-524");
    let token = store
        .put_mutable_overlay_value_idempotent(
            key.clone(),
            b"canonical-idempotent-value".to_vec(),
            "canonical-production-idempotency",
        )
        .unwrap();
    let replayed = store
        .put_mutable_overlay_value_idempotent(
            key.clone(),
            b"canonical-idempotent-value".to_vec(),
            "canonical-production-idempotency",
        )
        .unwrap();

    assert_eq!(replayed, token);
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let snapshot = reopened.mutable_overlay_snapshot().unwrap();
    let read = snapshot.read_composite(&key, |_| Ok(None)).unwrap();
    let (
        region_table_root,
        page_count,
        root_catalog_root,
        current_record_root,
        owner_token_root,
        mutable_idempotency_root,
        overlay_root,
    ) = {
        let inner = reopened.inner.lock().unwrap();
        (
            inner.region_table_root.unwrap(),
            inner.page_count,
            inner.root_catalog_root,
            inner.current_record_root,
            inner.owner_token_root,
            inner.mutable_idempotency_root,
            inner.overlay_root,
        )
    };
    let mut backing = shared.clone();
    let region = read_region_table(&mut backing, region_table_root, page_count).unwrap();
    let catalog = read_root_catalog(&mut backing, root_catalog_root.unwrap(), page_count).unwrap();

    assert_eq!(read.as_deref(), Some(&b"canonical-idempotent-value"[..]));
    assert_eq!(region.overlay_root, None);
    assert_eq!(overlay_root, None);
    assert_eq!(region.current_record_root, current_record_root);
    assert!(owner_token_root.is_some());
    assert!(mutable_idempotency_root.is_some());
    assert_eq!(
        catalog
            .entries
            .iter()
            .find(|entry| entry.family_id == MUTABLE_IDEMPOTENCY_FAMILY_ID)
            .map(|entry| entry.root),
        mutable_idempotency_root
    );
}

#[test]
fn canonical_production_workflow_routes_all_affected_families_atomically() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let overlay_key = durability_facet_test_key(b"documents", "canonical-workflow");
    let index_key = OverlayKey::from_segments([
        b"workspace",
        &[91; 16],
        b"documents",
        b"canonical-workflow",
        b"index",
        b"primary",
    ])
    .unwrap();
    let history_key = b"pages/workspace/canonical-workflow-history".to_vec();
    let txn = WorkflowTransaction {
        owner_state: loom_core::WorkflowOwnerState {
            controls: vec![loom_core::WorkflowControlWrite::AppendRetained {
                key: history_key.clone(),
                expected_next_sequence: 1,
                records: vec![b"history-1".to_vec()],
            }],
            ..loom_core::WorkflowOwnerState::default()
        },
        ..workflow_transaction_test(
            "canonical-workflow",
            vec![workflow_put_with_secondary_index(
                overlay_key.clone(),
                b"workflow-current",
                index_key.clone(),
                b"workflow-index",
            )],
            Some(b"canonical-workflow-idempotency"),
        )
    };
    let receipt = store.commit_workflow_transaction(txn.clone()).unwrap();
    let replay = store.commit_workflow_transaction(txn).unwrap();

    assert!(!receipt.replayed);
    assert!(replay.replayed);
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let snapshot = reopened.mutable_overlay_snapshot().unwrap();
    assert_eq!(
        snapshot
            .read_composite(&overlay_key, |_| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"workflow-current"[..])
    );
    assert_eq!(
        reopened
            .mutable_overlay_secondary_index_value(&index_key)
            .unwrap()
            .as_deref(),
        Some(&b"workflow-index"[..])
    );
    assert_eq!(reopened.retained_history_head(&history_key).unwrap(), 1);
    assert_eq!(
        reopened
            .retained_history_records(&history_key, 1, usize::MAX)
            .unwrap(),
        vec![b"history-1".to_vec()]
    );
    let (
        region_table_root,
        page_count,
        root_catalog_root,
        current_record_root,
        retained_history_root,
        owner_token_root,
        secondary_index_root,
        workflow_idempotency_root,
        overlay_root,
    ) = {
        let inner = reopened.inner.lock().unwrap();
        (
            inner.region_table_root.unwrap(),
            inner.page_count,
            inner.root_catalog_root,
            inner.current_record_root,
            inner.retained_history_root,
            inner.owner_token_root,
            inner.secondary_index_root,
            inner.workflow_idempotency_root,
            inner.overlay_root,
        )
    };
    let mut backing = shared.clone();
    let region = read_region_table(&mut backing, region_table_root, page_count).unwrap();
    let catalog = read_root_catalog(&mut backing, root_catalog_root.unwrap(), page_count).unwrap();

    assert_eq!(region.overlay_root, None);
    assert_eq!(overlay_root, None);
    assert_eq!(region.current_record_root, current_record_root);
    assert!(current_record_root.is_some());
    for (family_id, root) in [
        (RETAINED_HISTORY_FAMILY_ID, retained_history_root),
        (OWNER_TOKEN_FAMILY_ID, owner_token_root),
        (SECONDARY_INDEX_FAMILY_ID, secondary_index_root),
        (WORKFLOW_IDEMPOTENCY_FAMILY_ID, workflow_idempotency_root),
    ] {
        assert!(root.is_some());
        assert_eq!(
            catalog
                .entries
                .iter()
                .find(|entry| entry.family_id == family_id)
                .map(|entry| entry.root),
            root
        );
    }

    let cross_family_locs = [
        root_family_get(
            &mut backing,
            RETAINED_HISTORY_FAMILY_ID,
            retained_history_root,
            &retained_history_head_address(&history_key),
            page_count,
        )
        .unwrap()
        .unwrap(),
        root_family_get(
            &mut backing,
            OWNER_TOKEN_FAMILY_ID,
            owner_token_root,
            &mutable_overlay_owner_token_address(&overlay_key),
            page_count,
        )
        .unwrap()
        .unwrap(),
        root_family_get(
            &mut backing,
            SECONDARY_INDEX_FAMILY_ID,
            secondary_index_root,
            &mutable_overlay_secondary_index_address(&index_key),
            page_count,
        )
        .unwrap()
        .unwrap(),
    ];
    assert!(
        cross_family_locs
            .iter()
            .all(|loc| loc.global_page() == cross_family_locs[0].global_page()),
        "one workflow must pack eligible records from different root families into one slab"
    );
    assert_eq!(
        cross_family_locs
            .iter()
            .map(|loc| loc.slot)
            .collect::<BTreeSet<_>>()
            .len(),
        cross_family_locs.len(),
        "cross-family slab members must retain distinct slots"
    );
    let candidate = catalog
        .entries
        .iter()
        .find(|entry| entry.family_id == DELTA_PACK_CANDIDATE_FAMILY_ID)
        .unwrap();
    assert_eq!(candidate.flags, ROOT_FLAG_ADVISORY);
    let advisory_address =
        delta_pack::PackAdvisory::address(Algo::Blake3, cross_family_locs[0].global_page());
    let advisory_loc = root_family_get(
        &mut backing,
        DELTA_PACK_CANDIDATE_FAMILY_ID,
        Some(candidate.root),
        &advisory_address,
        page_count,
    )
    .unwrap()
    .unwrap();
    let advisory =
        delta_pack::PackAdvisory::decode(&read_blob_from_loc(&mut backing, advisory_loc).unwrap())
            .unwrap();
    assert_eq!(advisory.page, cross_family_locs[0].global_page());
    for loc in cross_family_locs {
        assert!(
            advisory
                .members
                .iter()
                .any(|member| member.slot == loc.slot)
        );
    }
}

#[test]
fn root_family_record_batch_accepts_only_locator_value_codecs() {
    assert!(root_family_uses_record_locators(
        pagebtree::ValueCodecKind::RecordLoc
    ));
    assert!(root_family_uses_record_locators(
        pagebtree::ValueCodecKind::PackedRecordRef
    ));
    assert!(!root_family_uses_record_locators(
        pagebtree::ValueCodecKind::FreePageExtent
    ));
}

#[test]
fn root_family_record_batch_rejects_cross_family_address_alias_before_writes() {
    let mut backing = SharedMem::default();
    let mut alloc = PageAllocator::new(0, 1, Vec::new());
    let address = [0x5a; 32];
    let owner_records = [(address, b"owner".as_slice())];
    let index_records = [(address, b"index".as_slice())];
    let before_size = backing.size().unwrap();
    let before_page_count = alloc.page_count();

    let error = write_root_family_record_batches(
        &mut backing,
        &mut alloc,
        0,
        &[
            RootFamilyRecordBatch {
                family_id: OWNER_TOKEN_FAMILY_ID,
                root: None,
                records: &owner_records,
            },
            RootFamilyRecordBatch {
                family_id: SECONDARY_INDEX_FAMILY_ID,
                root: None,
                records: &index_records,
            },
        ],
        None,
        1,
        Algo::Blake3,
        false,
        None,
        false,
    )
    .unwrap_err();

    assert_eq!(error.code, Code::CorruptObject);
    assert!(error.message.contains("multiple root families"));
    assert_eq!(backing.size().unwrap(), before_size);
    assert_eq!(alloc.page_count(), before_page_count);
}

#[test]
fn empty_root_family_record_batches_preserve_roots_without_writes() {
    let mut backing = SharedMem::default();
    let mut alloc = PageAllocator::new(9, 1, Vec::new());
    let before_size = backing.size().unwrap();
    let before_page_count = alloc.page_count();

    let outcome = write_root_family_record_batches(
        &mut backing,
        &mut alloc,
        9,
        &[
            RootFamilyRecordBatch {
                family_id: OWNER_TOKEN_FAMILY_ID,
                root: Some(PageId(7)),
                records: &[],
            },
            RootFamilyRecordBatch {
                family_id: MUTABLE_IDEMPOTENCY_FAMILY_ID,
                root: Some(PageId(8)),
                records: &[],
            },
        ],
        None,
        1,
        Algo::Blake3,
        false,
        None,
        false,
    )
    .unwrap();

    assert_eq!(outcome.roots[&OWNER_TOKEN_FAMILY_ID], Some(PageId(7)));
    assert_eq!(
        outcome.roots[&MUTABLE_IDEMPOTENCY_FAMILY_ID],
        Some(PageId(8))
    );
    assert!(outcome.touched_segments.is_empty());
    assert_eq!(backing.size().unwrap(), before_size);
    assert_eq!(alloc.page_count(), before_page_count);
}

#[test]
fn audit_retention_routes_through_catalog_family_root() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let principal = WorkspaceId::from_bytes([64; 16]);
    let config = AuditConfig {
        retention_days: 90,
        legal_hold: true,
    };
    let mut audit_map = BTreeMap::new();
    audit_map.insert(AUDIT_CONFIG_KEY.to_vec(), encode_audit_config(config));
    append_audit_record(
        &mut audit_map,
        store.digest_algo,
        Some(principal),
        "audit.retention.seed",
        Some("canonical"),
    )
    .unwrap();
    let records = audit_retention_family_records(&audit_map);

    store
        .commit_family_root_records_for_test(AUDIT_RETENTION_FAMILY_ID, &records)
        .unwrap();
    let (
        region_table_root,
        page_count,
        root_catalog_root,
        audit_retention_root,
        mutable_idempotency_root,
        overlay_root,
        current_record_root,
    ) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.region_table_root.unwrap(),
            inner.page_count,
            inner.root_catalog_root,
            inner.audit_retention_root,
            inner.mutable_idempotency_root,
            inner.overlay_root,
            inner.current_record_root,
        )
    };
    let mut backing = shared.clone();
    let region = read_region_table(&mut backing, region_table_root, page_count).unwrap();
    let catalog = read_root_catalog(&mut backing, root_catalog_root.unwrap(), page_count).unwrap();

    assert_eq!(region.root_catalog_root, root_catalog_root);
    assert_eq!(region.overlay_root, None);
    assert_eq!(overlay_root, None);
    assert_eq!(current_record_root, None);
    assert_eq!(mutable_idempotency_root, None);
    assert_eq!(store.control_root(), None);
    assert_eq!(
        catalog
            .entries
            .iter()
            .find(|entry| entry.family_id == AUDIT_RETENTION_FAMILY_ID)
            .map(|entry| entry.root),
        audit_retention_root
    );
    assert!(audit_retention_root.is_some());
    assert_eq!(
        store
            .mutable_overlay_record_payload(&audit_retention_record_address(AUDIT_CONFIG_KEY))
            .unwrap(),
        None
    );
    assert_eq!(store.audit_config().unwrap(), config);
    let records = store.audit_records().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].principal, Some(principal));
    assert_eq!(records[0].action, "audit.retention.seed");
}

#[test]
fn audit_retention_catalog_family_survives_reopen_without_current_hydration() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let mut audit_map = BTreeMap::new();
    append_audit_record(
        &mut audit_map,
        store.digest_algo,
        None,
        "audit.retention.reopen",
        Some("canonical"),
    )
    .unwrap();
    store
        .commit_family_root_records_for_test(
            AUDIT_RETENTION_FAMILY_ID,
            &audit_retention_family_records(&audit_map),
        )
        .unwrap();
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let stats = reopened.io_stats().unwrap();
    let (audit_retention_root, root_catalog_root, overlay_root, current_record_root) = {
        let inner = reopened.inner.lock().unwrap();
        (
            inner.audit_retention_root,
            inner.root_catalog_root,
            inner.overlay_root,
            inner.current_record_root,
        )
    };

    assert!(audit_retention_root.is_some());
    assert!(root_catalog_root.is_some());
    assert_eq!(overlay_root, None);
    assert_eq!(current_record_root, None);
    assert_eq!(stats.open_mutable_current_records_loaded, 0);
    assert_eq!(stats.open_mutable_control_records_skipped, 0);
    assert_eq!(
        reopened.audit_records().unwrap()[0].action,
        "audit.retention.reopen"
    );
}

#[test]
fn absent_audit_retention_catalog_family_reads_source_layout_empty() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();

    assert_eq!(store.audit_config().unwrap(), AuditConfig::default());
    assert!(store.audit_records().unwrap().is_empty());
}

#[test]
fn audit_retention_production_audit_only_publishes_family_root() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let seq = store
        .audit_append(None, "audit.retention.production", Some("audit-only"))
        .unwrap();

    assert_eq!(seq, 0);
    let (
        region_table_root,
        page_count,
        root_catalog_root,
        audit_retention_root,
        control_root,
        overlay_root,
        current_record_root,
    ) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.region_table_root.unwrap(),
            inner.page_count,
            inner.root_catalog_root,
            inner.audit_retention_root,
            inner.control_root,
            inner.overlay_root,
            inner.current_record_root,
        )
    };
    let mut backing = shared.clone();
    let region = read_region_table(&mut backing, region_table_root, page_count).unwrap();
    let catalog = read_root_catalog(&mut backing, root_catalog_root.unwrap(), page_count).unwrap();

    assert_eq!(region.overlay_root, None);
    assert_eq!(overlay_root, None);
    assert_eq!(current_record_root, None);
    assert_eq!(control_root, None);
    assert_eq!(region.root_catalog_root, root_catalog_root);
    assert_eq!(
        catalog
            .entries
            .iter()
            .find(|entry| entry.family_id == AUDIT_RETENTION_FAMILY_ID)
            .map(|entry| entry.root),
        audit_retention_root
    );
    assert!(audit_retention_root.is_some());
    assert!(store.control_root_map().unwrap().is_empty());

    drop(store);
    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let records = reopened.audit_records().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].action, "audit.retention.production");
    assert_eq!(reopened.control_root(), None);
    assert!(
        reopened
            .inner
            .lock()
            .unwrap()
            .audit_retention_root
            .is_some()
    );
}

#[test]
fn audit_retention_append_after_history_rewrites_bounded_pages() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    for seq in 0..40 {
        store
            .audit_append(None, "audit.retention.history", Some(&format!("seq={seq}")))
            .unwrap();
    }
    let before_page_count = store.inner.lock().unwrap().page_count;
    pagebtree::reset_load_all_calls_for_test();
    store.reset_audit_retention_instrumentation_for_test();

    store
        .audit_append(None, "audit.retention.bounded", Some("new-entry"))
        .unwrap();

    let after_page_count = store.inner.lock().unwrap().page_count;
    let page_growth = after_page_count.saturating_sub(before_page_count);
    let (point_puts, point_deletes) = store.audit_retention_point_write_counts_for_test();
    let full_enumerations = store.audit_retention_full_family_enumerations_for_test();

    assert_eq!(point_puts, 2);
    assert_eq!(point_deletes, 0);
    assert_eq!(full_enumerations, 0);
    assert!(
        page_growth <= 16,
        "audit append grew by {page_growth} pages for one changed next key and one new entry"
    );
    let records = store.audit_records().unwrap();
    assert_eq!(records.len(), 41);
    assert_eq!(records[40].action, "audit.retention.bounded");
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AuditRetentionPublicationState {
    generation: u64,
    object_index_root: Option<PageId>,
    region_table_root: Option<PageId>,
    maintenance_root: Option<PageId>,
    root_catalog_root: Option<PageId>,
    audit_retention_root: Option<PageId>,
    object_count: u64,
    canonical_free_runs: Vec<FreePageRun>,
    page_count: u64,
    physical_page_count: u64,
    records: BTreeMap<Vec<u8>, Vec<u8>>,
}

fn audit_retention_publication_state(store: &FileStore) -> AuditRetentionPublicationState {
    let records = store.audit_retention_map().unwrap();
    let inner = store.inner.lock().unwrap();
    let mut canonical_free_runs = inner.free.clone();
    canonical_free_runs.sort_by_key(|run| (run.start, run.len, run.freed_gen));
    AuditRetentionPublicationState {
        generation: inner.generation,
        object_index_root: inner.index_root,
        region_table_root: inner.region_table_root,
        maintenance_root: inner.maintenance_root,
        root_catalog_root: inner.root_catalog_root,
        audit_retention_root: inner.audit_retention_root,
        object_count: inner.maintenance.object_count,
        canonical_free_runs,
        page_count: inner.page_count,
        physical_page_count: inner.maintenance.physical_page_count,
        records,
    }
}

#[test]
fn audit_retention_prune_batches_puts_after_deletes_and_reopens() {
    let tp = TempPath::new("audit-retention-batch-prune");
    let after = {
        let store = FileStore::open(tp.path()).unwrap();
        for index in 0..4 {
            store
                .audit_append(
                    None,
                    "audit.retention.batch",
                    Some(&format!("entry={index}")),
                )
                .unwrap();
        }
        let before = audit_retention_publication_state(&store);
        assert_eq!(
            decode_audit_next(&before.records[AUDIT_NEXT_KEY]).unwrap(),
            4
        );
        store.reset_audit_retention_instrumentation_for_test();
        take_btree_batch_transaction_page_stats();

        let stats = store.audit_prune_through(None, 1).unwrap();

        assert_eq!(stats.pruned, 2);
        assert_eq!(stats.checkpoint_seq, Some(1));
        assert_eq!(stats.audit_seq, 4);
        assert_eq!(store.audit_retention_point_write_counts_for_test(), (3, 2));
        assert_eq!(store.audit_retention_full_family_enumerations_for_test(), 0);
        let batches = take_btree_batch_transaction_page_stats();
        assert_eq!(batches.len(), 1);
        assert!(batches[0].existing_pages_replaced > 0);
        let after = audit_retention_publication_state(&store);
        assert_eq!(after.generation, before.generation + 1);
        assert_ne!(after.audit_retention_root, before.audit_retention_root);
        assert_eq!(after.object_count, before.object_count);
        assert!(!after.records.contains_key(&audit_entry_key(0)));
        assert!(!after.records.contains_key(&audit_entry_key(1)));
        assert!(after.records.contains_key(&audit_entry_key(2)));
        assert!(after.records.contains_key(&audit_entry_key(3)));
        assert!(after.records.contains_key(&audit_entry_key(4)));
        assert!(after.records.contains_key(AUDIT_PRUNE_CHECKPOINT_KEY));
        assert_eq!(
            decode_audit_next(&after.records[AUDIT_NEXT_KEY]).unwrap(),
            5
        );
        after
    };

    let reopened = FileStore::open(tp.path()).unwrap();
    assert_eq!(audit_retention_publication_state(&reopened), after);
    assert_eq!(
        reopened
            .audit_records()
            .unwrap()
            .into_iter()
            .map(|record| (record.seq, record.action))
            .collect::<Vec<_>>(),
        vec![
            (2, "audit.retention.batch".to_string()),
            (3, "audit.retention.batch".to_string()),
            (4, "audit.prune".to_string()),
        ]
    );
}

#[test]
fn audit_retention_publication_failure_preserves_authoritative_state() {
    let tp = TempPath::new("audit-retention-publication-failure");
    let before = {
        let store = FileStore::open(tp.path()).unwrap();
        for index in 0..4 {
            store
                .audit_append(
                    None,
                    "audit.retention.rollback",
                    Some(&format!("entry={index}")),
                )
                .unwrap();
        }
        let before = audit_retention_publication_state(&store);
        let file_len_before = std::fs::metadata(tp.path()).unwrap().len();
        let hits = Arc::new(AtomicU64::new(0));
        let injected_hits = Arc::clone(&hits);
        let guard = install_store_publication_failure_test_injector(
            tp.path().to_path_buf(),
            Arc::new(move |boundary| {
                assert_eq!(
                    boundary,
                    StorePublicationFailureTestBoundary::AuditRetentionBeforeFinishTxn
                );
                injected_hits.fetch_add(1, Ordering::SeqCst);
                Err(LoomError::new(
                    Code::Io,
                    "injected audit-retention publication failure",
                ))
            }),
        );

        let error = store.audit_prune_through(None, 1).unwrap_err();
        assert_eq!(error.code, Code::Io);
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert_eq!(audit_retention_publication_state(&store), before);
        let file_len_after = std::fs::metadata(tp.path()).unwrap().len();
        assert!(file_len_after >= file_len_before);
        assert_eq!((file_len_after - file_len_before) % PAGE_SIZE, 0);
        eprintln!(
            "audit-retention unreachable candidate growth: {} bytes",
            file_len_after - file_len_before
        );
        assert_eq!(
            store
                .audit_records()
                .unwrap()
                .into_iter()
                .map(|record| record.seq)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );
        drop(guard);
        before
    };

    let reopened = FileStore::open(tp.path()).unwrap();
    assert_eq!(audit_retention_publication_state(&reopened), before);
    assert_eq!(
        reopened
            .audit_records()
            .unwrap()
            .into_iter()
            .map(|record| record.seq)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
}

#[test]
fn audit_retention_production_preserves_non_audit_control_root() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    store
        .save_store_policy_audited(
            StorePolicy {
                fips_required: true,
                ..StorePolicy::default()
            },
            None,
            "store.policy.set",
            Some("canonical-control"),
        )
        .unwrap();

    let raw = store.control_root_map().unwrap();
    assert!(raw.contains_key(STORE_POLICY_KEY));
    assert!(raw.keys().all(|key| !is_audit_retention_control_key(key)));
    assert!(store.control_root().is_some());
    assert!(store.inner.lock().unwrap().audit_retention_root.is_some());
    assert_eq!(
        store.store_policy().unwrap(),
        StorePolicy {
            fips_required: true,
            ..StorePolicy::default()
        }
    );
    assert_eq!(store.audit_records().unwrap()[0].action, "store.policy.set");
}

#[test]
fn audit_retention_source_control_keys_migrate_without_duplicate_authority() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let mut source = BTreeMap::new();
    source.insert(
        STORE_POLICY_KEY.to_vec(),
        encode_store_policy(StorePolicy {
            fips_required: true,
            ..StorePolicy::default()
        }),
    );
    append_audit_record(
        &mut source,
        store.digest_algo,
        None,
        "audit.retention.legacy",
        Some("source-control"),
    )
    .unwrap();
    store.commit_raw_control_map_for_test(source).unwrap();
    let legacy_control_root = store.control_root();

    store
        .audit_append(None, "audit.retention.migrated", Some("canonical"))
        .unwrap();

    assert_ne!(store.control_root(), legacy_control_root);
    assert!(store.inner.lock().unwrap().audit_retention_root.is_some());
    let raw = store.control_root_map().unwrap();
    assert!(raw.contains_key(STORE_POLICY_KEY));
    assert!(raw.keys().all(|key| !is_audit_retention_control_key(key)));
    let records = store.audit_records().unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].action, "audit.retention.legacy");
    assert_eq!(records[1].action, "audit.retention.migrated");
}

#[test]
fn audit_retention_family_root_does_not_fall_back_to_stale_control_root() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let mut audit_map = BTreeMap::new();
    append_audit_record(
        &mut audit_map,
        store.digest_algo,
        None,
        "audit.retention.canonical",
        Some("catalog"),
    )
    .unwrap();

    store
        .commit_family_root_records_for_test(
            AUDIT_RETENTION_FAMILY_ID,
            &audit_retention_family_records(&audit_map),
        )
        .unwrap();
    let mut stale = BTreeMap::new();
    append_audit_record(
        &mut stale,
        store.digest_algo,
        None,
        "audit.retention.stale",
        Some("control-root"),
    )
    .unwrap();
    store.commit_raw_control_map_for_test(stale).unwrap();
    let stale_control_root = store.control_root();

    assert!(stale_control_root.is_some());
    assert_eq!(store.control_root(), stale_control_root);
    assert!(store.control_map().unwrap().contains_key(AUDIT_NEXT_KEY));
    assert!(
        store
            .control_root_map()
            .unwrap()
            .contains_key(AUDIT_NEXT_KEY)
    );
    let records = store.audit_records().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].action, "audit.retention.canonical");
}

#[test]
fn audit_retention_key_mismatch_is_rejected() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let config = AuditConfig {
        retention_days: 30,
        legal_hold: false,
    };

    store
        .commit_family_root_records_for_test(
            AUDIT_RETENTION_FAMILY_ID,
            &[(
                audit_retention_record_address(AUDIT_CONFIG_KEY),
                encode_audit_retention_record(AUDIT_NEXT_KEY, &encode_audit_config(config)),
            )],
        )
        .unwrap();

    assert_eq!(store.audit_config().unwrap_err().code, Code::CorruptObject);
}

#[test]
fn audit_retention_rejects_out_of_family_record_keys() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();

    store
        .commit_family_root_records_for_test(
            AUDIT_RETENTION_FAMILY_ID,
            &[(
                audit_retention_record_address(STORE_POLICY_KEY),
                encode_audit_retention_record(STORE_POLICY_KEY, b"not-audit-retention"),
            )],
        )
        .unwrap();

    assert_eq!(store.audit_records().unwrap_err().code, Code::CorruptObject);
}

#[test]
fn mixed_workflow_and_audit_changes_publish_one_canonical_root_set() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let overlay_key = durability_facet_test_key(b"documents", "canonical-workflow-audit");
    let mut txn = workflow_transaction_test(
        "canonical-workflow-audit",
        vec![workflow_put(
            FacetKind::Document,
            overlay_key.clone(),
            b"workflow-audit-current",
            None,
        )],
        None,
    );
    txn.owner_state = loom_core::WorkflowOwnerState {
        controls: vec![loom_core::WorkflowControlWrite::Put {
            key: b"owner/current".to_vec(),
            payload: b"control-state".to_vec(),
        }],
        audits: vec![loom_core::WorkflowAuditWrite {
            principal: None,
            action: "workflow.audit".to_string(),
            target: Some("owner/current".to_string()),
        }],
        ..loom_core::WorkflowOwnerState::default()
    };

    store.commit_workflow_transaction(txn).unwrap();
    let (
        region_table_root,
        page_count,
        root_catalog_root,
        audit_retention_root,
        owner_token_root,
        current_record_root,
        control_root,
        overlay_root,
    ) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.region_table_root.unwrap(),
            inner.page_count,
            inner.root_catalog_root,
            inner.audit_retention_root,
            inner.owner_token_root,
            inner.current_record_root,
            inner.control_root,
            inner.overlay_root,
        )
    };
    let mut backing = shared.clone();
    let region = read_region_table(&mut backing, region_table_root, page_count).unwrap();
    let catalog = read_root_catalog(&mut backing, root_catalog_root.unwrap(), page_count).unwrap();

    assert_eq!(region.overlay_root, None);
    assert_eq!(overlay_root, None);
    assert_eq!(region.current_record_root, current_record_root);
    assert_eq!(region.root_catalog_root, root_catalog_root);
    assert!(current_record_root.is_some());
    assert!(owner_token_root.is_some());
    assert!(control_root.is_some());
    assert!(audit_retention_root.is_some());
    assert_eq!(
        catalog
            .entries
            .iter()
            .find(|entry| entry.family_id == AUDIT_RETENTION_FAMILY_ID)
            .map(|entry| entry.root),
        audit_retention_root
    );
    let raw = store.control_root_map().unwrap();
    assert_eq!(
        raw.get(b"owner/current".as_slice()).map(Vec::as_slice),
        Some(&b"control-state"[..])
    );
    assert!(raw.keys().all(|key| !is_audit_retention_control_key(key)));
    assert_eq!(store.audit_records().unwrap()[0].action, "workflow.audit");
    assert_eq!(
        store
            .mutable_overlay_snapshot()
            .unwrap()
            .read_composite(&overlay_key, |_| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"workflow-audit-current"[..])
    );
}

#[test]
fn other_control_records_remain_on_control_root_after_t188_14() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    store
        .save_store_policy_audited(
            StorePolicy {
                fips_required: true,
                ..StorePolicy::default()
            },
            None,
            "store.policy.set",
            Some("source-layout"),
        )
        .unwrap();
    let (control_root, root_catalog_root, audit_retention_root, overlay_root, current_record_root) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.control_root,
            inner.root_catalog_root,
            inner.audit_retention_root,
            inner.overlay_root,
            inner.current_record_root,
        )
    };

    assert!(control_root.is_some());
    assert!(root_catalog_root.is_some());
    assert!(audit_retention_root.is_some());
    assert_eq!(overlay_root, None);
    assert_eq!(current_record_root, None);
    assert!(
        store
            .control_root_map()
            .unwrap()
            .contains_key(STORE_POLICY_KEY)
    );
    assert!(
        store
            .control_root_map()
            .unwrap()
            .keys()
            .all(|key| !is_audit_retention_control_key(key))
    );
    assert_eq!(
        store.store_policy().unwrap(),
        StorePolicy {
            fips_required: true,
            ..StorePolicy::default()
        }
    );
    assert_eq!(store.audit_records().unwrap()[0].action, "store.policy.set");
}

#[test]
fn t188_3b_put_mutable_overlay_value_reopens_as_canonical_layout_after_t188_14() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let key = OverlayKey::from_segments([
        b"workspace",
        &[51; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"MX-437",
    ])
    .unwrap();
    let token = store
        .put_mutable_overlay_value(key.clone(), b"source-current".to_vec())
        .unwrap();

    let (root_catalog_root, owner_token_root, current_record_root, overlay_root) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.root_catalog_root,
            inner.owner_token_root,
            inner.current_record_root,
            inner.overlay_root,
        )
    };

    assert!(root_catalog_root.is_some());
    assert!(owner_token_root.is_some());
    assert!(current_record_root.is_some());
    assert_eq!(overlay_root, None);
    assert!(
        store
            .owner_token_record_payload(&mutable_overlay_owner_token_address(&key))
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .mutable_overlay_record_payload(&mutable_overlay_entry_address(&key))
            .unwrap()
            .is_some()
    );
    assert_eq!(
        store
            .mutable_overlay_durable_owner_token(&key)
            .unwrap()
            .as_ref()
            .map(|owner_token| owner_token.as_bytes()),
        Some(token.as_bytes())
    );
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let (root_catalog_root, owner_token_root, current_record_root, overlay_root) = {
        let inner = reopened.inner.lock().unwrap();
        (
            inner.root_catalog_root,
            inner.owner_token_root,
            inner.current_record_root,
            inner.overlay_root,
        )
    };
    let snapshot = reopened.mutable_overlay_snapshot().unwrap();
    let read = snapshot.read_composite(&key, |_| Ok(None)).unwrap();

    assert!(root_catalog_root.is_some());
    assert!(owner_token_root.is_some());
    assert!(current_record_root.is_some());
    assert_eq!(overlay_root, None);
    assert_eq!(read.as_deref(), Some(&b"source-current"[..]));
    assert_eq!(
        reopened
            .mutable_overlay_durable_owner_token(&key)
            .unwrap()
            .as_ref()
            .map(|owner_token| owner_token.as_bytes()),
        Some(token.as_bytes())
    );
}

#[test]
fn t188_3b_put_mutable_overlay_values_reopen_as_canonical_layout_after_t188_14() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let key_a = OverlayKey::from_segments([
        b"workspace",
        &[52; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"MX-438",
    ])
    .unwrap();
    let key_b = OverlayKey::from_segments([
        b"workspace",
        &[52; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"MX-439",
    ])
    .unwrap();
    let tokens = store
        .put_mutable_overlay_values(vec![
            (key_a.clone(), b"batch-current-a".to_vec()),
            (key_b.clone(), b"batch-current-b".to_vec()),
        ])
        .unwrap();
    assert_eq!(tokens.len(), 2);

    let (root_catalog_root, owner_token_root, current_record_root, overlay_root) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.root_catalog_root,
            inner.owner_token_root,
            inner.current_record_root,
            inner.overlay_root,
        )
    };
    assert!(root_catalog_root.is_some());
    assert!(owner_token_root.is_some());
    assert!(current_record_root.is_some());
    assert_eq!(overlay_root, None);
    assert!(
        store
            .owner_token_record_payload(&mutable_overlay_owner_token_address(&key_a))
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .owner_token_record_payload(&mutable_overlay_owner_token_address(&key_b))
            .unwrap()
            .is_some()
    );
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let snapshot = reopened.mutable_overlay_snapshot().unwrap();
    let read_a = snapshot.read_composite(&key_a, |_| Ok(None)).unwrap();
    let read_b = snapshot.read_composite(&key_b, |_| Ok(None)).unwrap();

    assert_eq!(read_a.as_deref(), Some(&b"batch-current-a"[..]));
    assert_eq!(read_b.as_deref(), Some(&b"batch-current-b"[..]));
    assert_eq!(
        reopened
            .mutable_overlay_durable_owner_token(&key_a)
            .unwrap()
            .as_ref()
            .map(|owner_token| owner_token.as_bytes()),
        Some(tokens[0].as_bytes())
    );
    assert_eq!(
        reopened
            .mutable_overlay_durable_owner_token(&key_b)
            .unwrap()
            .as_ref()
            .map(|owner_token| owner_token.as_bytes()),
        Some(tokens[1].as_bytes())
    );
}

#[test]
fn t188_3b_mixed_public_idempotent_write_reopens_as_canonical_layout_after_t188_14() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let key = durability_facet_test_key(b"documents", "t188-3b-mixed-control");
    store
        .put_mutable_overlay_value(key.clone(), b"legacy-before-mixed".to_vec())
        .unwrap();
    let current = store
        .put_mutable_overlay_value_idempotent(
            key.clone(),
            b"canonical-after-mixed".to_vec(),
            "t188-3b-mixed-control",
        )
        .unwrap();

    let (root_catalog_root, owner_token_root, current_record_root, overlay_root) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.root_catalog_root,
            inner.owner_token_root,
            inner.current_record_root,
            inner.overlay_root,
        )
    };
    assert!(root_catalog_root.is_some());
    assert!(owner_token_root.is_some());
    assert!(current_record_root.is_some());
    assert_eq!(overlay_root, None);
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    assert_eq!(
        reopened
            .put_mutable_overlay_value_idempotent(
                key.clone(),
                b"canonical-after-mixed".to_vec(),
                "t188-3b-mixed-control",
            )
            .unwrap()
            .as_bytes(),
        current.as_bytes()
    );
    assert_eq!(
        reopened
            .mutable_overlay_snapshot()
            .unwrap()
            .read_composite(&key, |_| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"canonical-after-mixed"[..])
    );
    assert_eq!(
        reopened
            .mutable_overlay_durable_owner_token(&key)
            .unwrap()
            .as_ref()
            .map(|token| token.as_bytes()),
        Some(current.as_bytes())
    );
}

#[test]
fn mutable_overlay_entries_hydrate_opened_loom() {
    let shared = SharedMem::default();
    let mut loom = loom_over_backing(Box::new(shared.clone()), true).unwrap();
    let key = OverlayKey::from_segments([
        b"workspace",
        &[2; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"MX-388",
    ])
    .unwrap();
    loom.store()
        .put_mutable_overlay_value(key.clone(), b"current".to_vec())
        .unwrap();
    save_loom(&mut loom).unwrap();
    drop(loom);

    let reopened = loom_over_backing(Box::new(shared), true).unwrap();
    let read = reopened
        .mutable_overlay()
        .snapshot()
        .read_composite(&key, |_| Ok(Some(b"base".to_vec())))
        .unwrap();
    let report = reopened.store().store_maintenance_report(100).unwrap();

    assert_eq!(read.as_deref(), Some(&b"current"[..]));
    assert_eq!(report.overlay_health.current_record_count, 1);
    assert_eq!(report.overlay_obsolete_record_count, 0);
}

#[test]
fn mutable_overlay_hot_updates_keep_one_durable_current_entry() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let key = OverlayKey::from_segments([
        b"workspace",
        &[3; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"MX-388",
    ])
    .unwrap();
    for update in 0..64u64 {
        store
            .put_mutable_overlay_value(key.clone(), format!("current-{update}").into_bytes())
            .unwrap();
    }
    store
        .control_set(b"unrelated-control-entry", vec![1])
        .unwrap();
    let control_root = store.control_root();
    for update in 64..128u64 {
        store
            .put_mutable_overlay_value(key.clone(), format!("current-{update}").into_bytes())
            .unwrap();
    }
    assert_eq!(store.control_root(), control_root);
    let report = store.store_maintenance_report(100).unwrap();
    assert_eq!(report.overlay_health.current_record_count, 1);
    assert_eq!(report.overlay_obsolete_record_count, 127);
    assert_eq!(report.growth_domains[0].domain, "tickets");
    assert_eq!(report.growth_domains[0].current_records, 1);
    assert_eq!(report.growth_domains[0].obsolete_records, 127);
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let read = reopened
        .mutable_overlay_snapshot()
        .unwrap()
        .read_composite(&key, |_| Ok(None))
        .unwrap();
    let report = reopened.store_maintenance_report(100).unwrap();

    assert_eq!(read.as_deref(), Some(&b"current-127"[..]));
    assert_eq!(report.overlay_health.current_generation, 128);
    assert_eq!(report.overlay_health.current_record_count, 1);
    assert_eq!(report.overlay_health.hot_write_count, 1);
}

#[test]
fn mutable_overlay_superseded_current_pages_return_to_allocator_and_attribution() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let key = OverlayKey::from_segments([
        b"workspace",
        &[46; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"allocator-reuse",
    ])
    .unwrap();
    for update in 0..24u64 {
        store
            .put_mutable_overlay_value(
                key.clone(),
                format!("reclaimable-current-{update}").into_bytes(),
            )
            .unwrap();
    }

    let report = store.store_maintenance_report(100).unwrap();
    let attribution = store.page_class_attribution(100).unwrap();
    let stale_records = attribution
        .classes
        .iter()
        .filter(|class| class.class.starts_with("stale_record_"))
        .map(|class| class.bytes)
        .sum::<u64>();
    let current = attribution
        .classes
        .iter()
        .find(|class| class.class.starts_with("mutable_overlay_record_"))
        .unwrap();
    let reusable = attribution
        .classes
        .iter()
        .find(|class| class.class == "reusable_free_page")
        .unwrap();

    assert_eq!(report.overlay_health.current_record_count, 1);
    assert_eq!(report.overlay_obsolete_record_count, 23);
    assert_eq!(reusable.bytes, report.reusable_free_bytes);
    assert!(report.reusable_free_bytes > 0);
    assert!(current.pages > 0);
    assert_eq!(stale_records, 0);
    let pages_before_reuse = report.status.physical_page_count;

    store
        .put_mutable_overlay_value(key.clone(), b"reclaimable-current-24".to_vec())
        .unwrap();
    let reused = store.store_maintenance_report(100).unwrap();
    assert!(reused.status.physical_page_count <= pages_before_reuse + 4);
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let read = reopened
        .mutable_overlay_snapshot()
        .unwrap()
        .read_composite(&key, |_| Ok(None))
        .unwrap();

    assert_eq!(read.as_deref(), Some(&b"reclaimable-current-24"[..]));
}

#[test]
fn page_attribution_does_not_recognize_legacy_free_map_blobs() {
    let mut page = [0u8; PAGE_SIZE as usize];
    page[0] = 0xB4;
    page[1..5].copy_from_slice(&0u32.to_le_bytes());
    let checksum = crc32c(&page[..5]);
    page[5..9].copy_from_slice(&checksum.to_le_bytes());
    assert!(pagemap::decode(&page).is_some());

    let mut file = MemoryBacking::new();
    file.grow(DATA_START + PAGE_SIZE).unwrap();
    file.pwrite(DATA_START, &page).unwrap();
    let mut classes = BTreeMap::new();
    assert_eq!(
        classify_unreferenced_page(&mut file, &mut classes, 0, 1).unwrap(),
        "unreferenced_unclassified_page"
    );
    assert_eq!(
        classes.get(&0).map(String::as_str),
        Some("unreferenced_unclassified_page")
    );
}

#[test]
fn document_workflow_releases_planning_pin_before_reclaiming_current_records() {
    let shared = SharedMem::default();
    let mut loom = loom_over_backing(Box::new(shared.clone()), true).unwrap();
    let workspace = loom
        .registry_mut()
        .create(
            FacetKind::Document,
            Some("documents"),
            WorkspaceId::from_bytes([61; 16]),
        )
        .unwrap();

    for update in 0..24u64 {
        document::document_put_text(
            &mut loom,
            workspace,
            "notes",
            "current",
            &format!("current-{update}"),
            None,
        )
        .unwrap();
    }

    let stale_record_bytes = loom
        .store()
        .page_class_attribution(0)
        .unwrap()
        .classes
        .iter()
        .filter(|class| class.class.starts_with("stale_record_"))
        .map(|class| class.bytes)
        .sum::<u64>();
    assert_eq!(
        stale_record_bytes,
        0,
        "page classes: {:?}",
        loom.store().page_class_attribution(0).unwrap().classes
    );
    assert_eq!(
        document::document_get_text(&loom, workspace, "notes", "current")
            .unwrap()
            .unwrap()
            .text,
        "current-23"
    );

    drop(loom);
    let reopened = loom_over_backing(Box::new(shared), true).unwrap();
    assert_eq!(
        document::document_get_text(&reopened, workspace, "notes", "current")
            .unwrap()
            .unwrap()
            .text,
        "current-23"
    );
}

#[test]
fn mutable_overlay_reclaim_preserves_pinned_mvcc_snapshot_window() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared), true).unwrap();
    let key = OverlayKey::from_segments([
        b"workspace",
        &[47; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"pinned-reclaim",
    ])
    .unwrap();

    store
        .put_mutable_overlay_value(key.clone(), b"pinned-current-0".to_vec())
        .unwrap();
    let pinned = store.open_mvcc_snapshot().unwrap();
    store
        .put_mutable_overlay_value(key.clone(), b"pinned-current-1".to_vec())
        .unwrap();

    let blocked = store.page_class_attribution(100).unwrap();
    let stale_while_pinned = blocked
        .classes
        .iter()
        .filter(|class| class.class.starts_with("stale_record_"))
        .map(|class| class.bytes)
        .sum::<u64>();
    assert!(stale_while_pinned > 0);
    assert_eq!(
        pinned
            .read_composite(&key, |_, _| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"pinned-current-0"[..])
    );

    drop(pinned);
    store
        .put_mutable_overlay_value(key.clone(), b"pinned-current-2".to_vec())
        .unwrap();
    let unpinned = store.store_maintenance_report(100).unwrap();
    assert!(unpinned.reusable_free_bytes > 0);
}

#[test]
fn mutable_overlay_reclaim_blocks_audit_retention_and_tombstone_entries() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared), true).unwrap();
    store
        .save_audit_config_audited(
            AuditConfig {
                retention_days: 365,
                legal_hold: true,
            },
            None,
            "audit.config.set",
            Some("legal_hold=true"),
        )
        .unwrap();
    let audit_key = OverlayKey::from_segments([
        b"workspace",
        &[48; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"audit-retention",
    ])
    .unwrap();

    store
        .put_mutable_overlay_value(audit_key.clone(), b"audit-current-0".to_vec())
        .unwrap();
    store
        .put_mutable_overlay_value(audit_key, b"audit-current-1".to_vec())
        .unwrap();
    let audit_blocked = store.page_class_attribution(100).unwrap();
    let audit_stale_records = audit_blocked
        .classes
        .iter()
        .filter(|class| class.class.starts_with("stale_record_"))
        .map(|class| class.bytes)
        .sum::<u64>();
    assert!(audit_stale_records > 0);

    let tombstone_key = OverlayKey::from_segments([
        b"workspace",
        &[49; 16],
        b"documents",
        b"matrix",
        b"doc",
        b"tombstone-retention",
    ])
    .unwrap();
    store
        .save_audit_config_audited(
            AuditConfig {
                retention_days: 365,
                legal_hold: false,
            },
            None,
            "audit.config.set",
            Some("legal_hold=false"),
        )
        .unwrap();
    store
        .put_mutable_overlay_value(tombstone_key.clone(), b"document-current".to_vec())
        .unwrap();
    store.put_mutable_overlay_tombstone(tombstone_key).unwrap();
    let tombstone_blocked = store.page_class_attribution(100).unwrap();
    let tombstone_stale_records = tombstone_blocked
        .classes
        .iter()
        .filter(|class| class.class.starts_with("stale_record_"))
        .map(|class| class.bytes)
        .sum::<u64>();
    assert!(tombstone_stale_records >= audit_stale_records);
}

#[test]
fn mutable_overlay_checkpoint_plan_reports_compactable_current_records_and_page_classes() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let key = OverlayKey::from_segments([
        b"workspace",
        &[51; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"checkpoint-planner",
    ])
    .unwrap();

    for update in 0..16u64 {
        store
            .put_mutable_overlay_value(key.clone(), format!("current-{update}").into_bytes())
            .unwrap();
    }

    let plan = store.mutable_overlay_checkpoint_plan(100).unwrap();
    assert_eq!(plan.current_record_count, 1);
    assert_eq!(plan.compactable_current_records, 1);
    assert_eq!(plan.blocked_current_records, 0);
    assert_eq!(plan.stale_record_bytes, 0);
    assert!(plan.reusable_free_bytes > 0);
    assert_eq!(plan.current_records.len(), 1);
    assert_eq!(plan.current_records[0].key, key);
    assert_eq!(plan.current_records[0].kind, OverlayEntryKind::Value);
    assert_eq!(plan.current_records[0].blockers, Vec::new());

    drop(store);
    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let reopened_plan = reopened.mutable_overlay_checkpoint_plan(100).unwrap();
    assert_eq!(reopened_plan.current_records[0].generation.as_u64(), 16);
}

#[test]
fn mutable_overlay_checkpoint_plan_reports_pinned_generations_and_retention_blockers() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let key = OverlayKey::from_segments([
        b"workspace",
        &[52; 16],
        b"documents",
        b"matrix",
        b"doc",
        b"checkpoint-retention",
    ])
    .unwrap();

    store
        .put_mutable_overlay_value(key.clone(), b"checkpoint-current".to_vec())
        .unwrap();
    let pinned = store.open_mvcc_snapshot().unwrap();
    store
        .save_audit_config_audited(
            AuditConfig {
                retention_days: 365,
                legal_hold: true,
            },
            None,
            "audit.config.set",
            Some("legal_hold=true"),
        )
        .unwrap();

    let plan = store
        .mutable_overlay_checkpoint_plan_with_durable_floor(100, Some(0))
        .unwrap();
    assert_eq!(plan.active_snapshot_count, 1);
    assert_eq!(
        plan.oldest_pinned_generation,
        Some(pinned.overlay_generation())
    );
    assert_eq!(plan.pinned_generations, vec![pinned.overlay_generation()]);
    assert_eq!(plan.compactable_current_records, 0);
    let blockers = &plan.current_records[0].blockers;
    assert!(blockers.contains(&MutableOverlayReclaimBlocker::PinnedSnapshot));
    assert!(blockers.contains(&MutableOverlayReclaimBlocker::RetainedHistory));
    assert!(blockers.contains(&MutableOverlayReclaimBlocker::AuditRetention));
    assert!(blockers.contains(&MutableOverlayReclaimBlocker::DurableGenerationWindow));
    assert!(blockers.contains(&MutableOverlayReclaimBlocker::StrictPromotionBoundary));
}

#[test]
fn mutable_overlay_checkpoint_plan_keeps_tombstones_blocked() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let key = OverlayKey::from_segments([
        b"workspace",
        &[53; 16],
        b"documents",
        b"matrix",
        b"doc",
        b"checkpoint-tombstone",
    ])
    .unwrap();

    store
        .put_mutable_overlay_value(key.clone(), b"deleted-current".to_vec())
        .unwrap();
    store.put_mutable_overlay_tombstone(key.clone()).unwrap();

    let plan = store.mutable_overlay_checkpoint_plan(100).unwrap();
    assert_eq!(plan.tombstone_count, 1);
    assert_eq!(plan.current_records[0].key, key);
    assert_eq!(plan.current_records[0].kind, OverlayEntryKind::Tombstone);
    assert!(!plan.current_records[0].compactable);
    assert!(
        plan.current_records[0]
            .blockers
            .contains(&MutableOverlayReclaimBlocker::TombstoneRetention)
    );
}

#[test]
fn mutable_overlay_checkpoint_writer_compacts_current_pages_and_preserves_reads() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let key = OverlayKey::from_segments([
        b"workspace",
        &[54; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"checkpoint-writer",
    ])
    .unwrap();
    for update in 0..16u64 {
        store
            .put_mutable_overlay_value(
                key.clone(),
                format!("checkpoint-writer-{update}").into_bytes(),
            )
            .unwrap();
    }
    let before = store.mutable_overlay_checkpoint_plan(100).unwrap();
    assert_eq!(before.current_record_count, 1);
    assert!(before.compactable_current_records > 0);
    assert_eq!(before.stale_record_bytes, 0);
    let before_status = store.store_maintenance_report(100).unwrap();

    let report = store.checkpoint_mutable_overlay_pages(100).unwrap();
    let after = store.store_maintenance_report(100).unwrap();

    assert_eq!(report.planned_current_records, 1);
    assert_eq!(report.compacted_current_records, 0);
    assert_eq!(report.rewritten_record_bytes, 0);
    assert_eq!(report.freed_record_pages, 0);
    assert!(after.reusable_free_bytes >= report.reusable_free_bytes);
    assert!(
        report.physical_page_count
            <= before_status.status.physical_page_count + report.freed_record_pages
    );
    assert_eq!(
        store
            .mutable_overlay_snapshot()
            .unwrap()
            .read_composite(&key, |_| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"checkpoint-writer-15"[..])
    );
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    assert_eq!(
        reopened
            .mutable_overlay_snapshot()
            .unwrap()
            .read_composite(&key, |_| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"checkpoint-writer-15"[..])
    );
}

#[test]
fn mutable_overlay_checkpoint_writer_respects_pinned_readers() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let key = OverlayKey::from_segments([
        b"workspace",
        &[55; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"checkpoint-pinned",
    ])
    .unwrap();
    store
        .put_mutable_overlay_value(key.clone(), b"pinned-0".to_vec())
        .unwrap();
    store
        .put_mutable_overlay_value(key.clone(), b"pinned-1".to_vec())
        .unwrap();
    let pinned = store.open_mvcc_snapshot().unwrap();

    let blocked = store.checkpoint_mutable_overlay_pages(100).unwrap();

    assert_eq!(blocked.compacted_current_records, 0);
    assert_eq!(blocked.blocked_current_records, 1);
    assert_eq!(
        pinned
            .read_composite(&key, |_, _| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"pinned-1"[..])
    );
    drop(pinned);

    store
        .put_mutable_overlay_value(key.clone(), b"pinned-2".to_vec())
        .unwrap();
    let compacted = store.checkpoint_mutable_overlay_pages(100).unwrap();
    assert_eq!(compacted.compacted_current_records, 0);
}

#[test]
fn mutable_overlay_checkpoint_writer_keeps_tombstones_and_reports_no_churn() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let key = OverlayKey::from_segments([
        b"workspace",
        &[56; 16],
        b"documents",
        b"matrix",
        b"doc",
        b"checkpoint-tombstone-writer",
    ])
    .unwrap();
    store
        .put_mutable_overlay_value(key.clone(), b"deleted-current".to_vec())
        .unwrap();
    store.put_mutable_overlay_tombstone(key.clone()).unwrap();

    let report = store.checkpoint_mutable_overlay_pages(100).unwrap();

    assert_eq!(report.compacted_current_records, 0);
    assert_eq!(report.blocked_current_records, 1);
    assert_eq!(
        store
            .mutable_overlay_snapshot()
            .unwrap()
            .read_composite(&key, |_| Ok(Some(b"base".to_vec())))
            .unwrap(),
        None
    );
}

#[test]
fn mutable_overlay_tombstone_reclaim_rules_gate_delete_reopen_checkpoint_and_horizon() {
    use MutableOverlayReclaimBlocker::{
        DurableGenerationWindow, PinnedSnapshot, TombstoneRetention,
    };

    // A superseded current-record page at generation 5, superseded by generation 6, with every
    // retention horizon already passed. This is the baseline "reopen" shape: a value supersedes the
    // prior entry so no tombstone masks the base.
    let base = MutableOverlayReclaimState {
        superseded_generation: 5,
        superseding_generation: 6,
        latest_index_generation: 6,
        oldest_pinned_snapshot_generation: None,
        retained_history_generation: None,
        audit_retention_active: false,
        tombstone_masks_base: false,
        durable_reclaim_floor: 6,
        strict_promotion_generation: None,
    };
    assert!(base.is_eligible().unwrap());

    // Delete: the superseding entry is a tombstone that must keep hiding a value reachable from the
    // immutable base, so the page is retained under the tombstone-retention rule and nothing else.
    let deleted = MutableOverlayReclaimState {
        tombstone_masks_base: true,
        ..base
    };
    assert_eq!(deleted.blockers().unwrap(), vec![TombstoneRetention]);

    // Checkpoint: a pinned MVCC snapshot inside the superseded window holds the page even once the
    // tombstone no longer masks the base; releasing the checkpoint clears the block.
    let pinned = MutableOverlayReclaimState {
        oldest_pinned_snapshot_generation: Some(5),
        ..base
    };
    assert!(pinned.blockers().unwrap().contains(&PinnedSnapshot));
    assert!(!pinned.is_eligible().unwrap());
    let released = MutableOverlayReclaimState {
        oldest_pinned_snapshot_generation: None,
        ..pinned
    };
    assert!(released.is_eligible().unwrap());

    // Retention horizon: the durable-generation floor has not reached the superseding generation, so
    // recovery could still roll the replacement back. Advancing the floor clears the block.
    let below_horizon = MutableOverlayReclaimState {
        durable_reclaim_floor: 5,
        ..base
    };
    assert!(
        below_horizon
            .blockers()
            .unwrap()
            .contains(&DurableGenerationWindow)
    );
    let at_horizon = MutableOverlayReclaimState {
        durable_reclaim_floor: 6,
        ..base
    };
    assert!(at_horizon.is_eligible().unwrap());
}

#[test]
fn page_attribution_materializes_lazy_object_index_after_reopen() {
    let shared = SharedMem::default();
    {
        let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
        for value in 0..8u8 {
            store
                .put(&Object::Blob(vec![value; 512]).canonical())
                .unwrap();
        }
    }

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let attribution = reopened.page_class_attribution(100).unwrap();
    let live_record_bytes = attribution
        .classes
        .iter()
        .filter(|class| class.class.starts_with("record_"))
        .map(|class| class.bytes)
        .sum::<u64>();
    let stale_record_bytes = attribution
        .classes
        .iter()
        .filter(|class| class.class.starts_with("stale_record_"))
        .map(|class| class.bytes)
        .sum::<u64>();

    assert!(live_record_bytes > 0);
    assert_eq!(stale_record_bytes, 0);
}

#[test]
fn mutable_overlay_tombstone_delete_retains_then_reopen_preserves_reads_and_reclaims() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared), true).unwrap();
    let key = OverlayKey::from_segments([
        b"workspace",
        &[50; 16],
        b"documents",
        b"matrix",
        b"doc",
        b"delete-reopen",
    ])
    .unwrap();

    store
        .put_mutable_overlay_value(key.clone(), b"v0".to_vec())
        .unwrap();
    // Delete: the tombstone masks the immutable base through composite reads, and the buried value
    // page is retained under the tombstone-retention rule. A composite read is not-found even though
    // the base still exposes a value.
    store.put_mutable_overlay_tombstone(key.clone()).unwrap();
    assert_eq!(
        store
            .mutable_overlay_snapshot()
            .unwrap()
            .read_composite(&key, |_| Ok(Some(b"base".to_vec())))
            .unwrap(),
        None
    );
    let stale_after_delete = store
        .page_class_attribution(100)
        .unwrap()
        .classes
        .iter()
        .filter(|class| class.class.starts_with("stale_record_"))
        .map(|class| class.bytes)
        .sum::<u64>();
    assert!(stale_after_delete > 0);

    // Checkpoint preservation: a snapshot pinned at the tombstone generation keeps observing the
    // delete even after the key is reopened, so tombstones preserve composite reads until the
    // checkpoint is released.
    let pinned = store.open_mvcc_snapshot().unwrap();
    store
        .put_mutable_overlay_value(key.clone(), b"reopened".to_vec())
        .unwrap();
    assert_eq!(
        pinned
            .read_composite(&key, |_, _| Ok(Some(b"base".to_vec())))
            .unwrap(),
        None
    );
    assert!(pinned.release().unwrap());

    // Reopen churn with no pinned checkpoint: each value supersedes a tombstone (reclaimable) and each
    // tombstone supersedes a value (retained). The superseded tombstone pages return to the allocator,
    // so the free map grows even though deleted-value pages stay retained.
    for cycle in 0..24u64 {
        store.put_mutable_overlay_tombstone(key.clone()).unwrap();
        store
            .put_mutable_overlay_value(key.clone(), format!("reopen-{cycle}").into_bytes())
            .unwrap();
    }

    // tombstone_count is a cumulative hot-write-log metric, so the current view is asserted through
    // the single live logical key and a composite read rather than that counter.
    let reopened = store.store_maintenance_report(100).unwrap();
    assert_eq!(reopened.overlay_health.current_record_count, 1);
    assert!(reopened.reusable_free_bytes > 0);
    assert_eq!(
        store
            .mutable_overlay_snapshot()
            .unwrap()
            .read_composite(&key, |_| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"reopen-23"[..])
    );
}

#[test]
fn document_current_records_survive_reopen_without_rewriting_collection_root() {
    let shared = SharedMem::default();
    let mut loom = loom_over_backing(Box::new(shared.clone()), true).unwrap();
    let ns = loom
        .registry_mut()
        .create(
            FacetKind::Document,
            Some("docs"),
            WorkspaceId::from_bytes([4; 16]),
        )
        .unwrap();
    document::document_put_text(&mut loom, ns, "notes", "a", "one", None).unwrap();
    let control_root = loom.store().control_root();
    for update in 0..32u64 {
        document::document_put_text(
            &mut loom,
            ns,
            "notes",
            "a",
            &format!("current-{update}"),
            None,
        )
        .unwrap();
    }
    assert_eq!(loom.store().control_root(), control_root);
    assert_eq!(
        document::document_get_text(&loom, ns, "notes", "a")
            .unwrap()
            .unwrap()
            .text,
        "current-31"
    );
    assert_eq!(
        document::doc_list_collections(&loom, ns).unwrap(),
        vec!["notes".to_string()]
    );
    save_loom(&mut loom).unwrap();
    drop(loom);

    let mut reopened = loom_over_backing(Box::new(shared), true).unwrap();
    assert_eq!(
        document::document_get_text(&reopened, ns, "notes", "a")
            .unwrap()
            .unwrap()
            .text,
        "current-31"
    );
    assert_eq!(
        reopened
            .store()
            .store_maintenance_report(100)
            .unwrap()
            .overlay_health
            .current_record_count,
        2
    );
    reopened
        .commit(ns, "tester", "checkpoint documents", 1)
        .unwrap();
    assert_eq!(
        document::document_get_text(&reopened, ns, "notes", "a")
            .unwrap()
            .unwrap()
            .text,
        "current-31"
    );
}

#[test]
fn document_delete_collection_tombstones_mutable_overlay_head() {
    let shared = SharedMem::default();
    let mut loom = loom_over_backing(Box::new(shared.clone()), true).unwrap();
    let ns = loom
        .registry_mut()
        .create(
            FacetKind::Document,
            Some("docs"),
            WorkspaceId::from_bytes([5; 16]),
        )
        .unwrap();
    document::document_put_text(&mut loom, ns, "notes", "a", "one", None).unwrap();
    document::document_put_text(&mut loom, ns, "logs", "b", "two", None).unwrap();

    assert!(document::doc_delete_collection(&mut loom, ns, "notes").unwrap());
    assert_eq!(
        document::doc_list_collections(&loom, ns).unwrap(),
        vec!["logs".to_string()]
    );
    assert!(
        document::document_get_text(&loom, ns, "notes", "a")
            .unwrap()
            .is_none()
    );
    save_loom(&mut loom).unwrap();
    drop(loom);

    let reopened = loom_over_backing(Box::new(shared), true).unwrap();
    assert_eq!(
        document::doc_list_collections(&reopened, ns).unwrap(),
        vec!["logs".to_string()]
    );
    assert!(
        document::document_get_text(&reopened, ns, "notes", "a")
            .unwrap()
            .is_none()
    );
}

#[test]
fn store_maintenance_report_attributes_reclaimable_and_derived_state() {
    // A synthetic churned store: keep one object reachable via the reference root, then write and
    // discard several more to create reclaimable dead space. The diagnostic must attribute the
    // reclaimable garbage and report the new derived/control-root fields (MX-303).
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared), true).unwrap();
    let keep = store.put(b"live-data").unwrap();
    store.set_reference_root(Some(keep)).unwrap();
    for byte in 0..8u8 {
        let _unreachable = store.put(&[byte; 512]).unwrap();
    }
    let status = store.maintenance_status().unwrap();
    assert!(
        status.candidate_dead_pages > 0,
        "churn should create reclaimable dead pages"
    );

    let report = store.store_maintenance_report(100).unwrap();
    // Candidate and reusable bytes are disjoint at the reporting boundary.
    assert_eq!(
        report.candidate_reclaimable_bytes,
        status
            .candidate_dead_pages
            .saturating_sub(status.reusable_free_pages)
            * PAGE_SIZE
    );
    assert_eq!(report.candidate_reclaimable_bytes, 0);
    assert!(report.reusable_free_bytes > 0);
    // New attribution fields: a fresh store has no durable-local derived artifacts, and with no
    // active reachability-mark epoch there are no retained control roots or marked-live objects.
    assert_eq!(report.derived_payload_count, 0);
    assert_eq!(report.retained_control_roots, 0);
    assert_eq!(report.marked_live_objects, 0);
}

#[test]
fn reachability_mark_epoch_resumes_after_reopen_without_validating_completion() {
    use loom_core::WsSelector;
    use loom_core::workspace::{FacetKind, WorkspaceId};

    let tp = TempPath::new("mark-epoch-resume");
    let epoch_id;
    {
        let mut loom = open_loom(tp.path()).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("p"),
                WorkspaceId::from_bytes([31; 16]),
            )
            .unwrap();
        for i in 0..8u64 {
            loom.write_file(
                ns,
                &format!("f{i}.txt"),
                format!("v{i}").as_bytes(),
                0o100644,
            )
            .unwrap();
            loom.commit(ns, "nas", "edit", i + 1).unwrap();
        }
        save_loom(&mut loom).unwrap();
        let epoch = begin_loom_reachability_mark_epoch(&loom).unwrap();
        epoch_id = epoch.epoch;
        let step = step_loom_reachability_mark_epoch(&loom, 1).unwrap();
        assert!(!step.completed);
        loom.store()
            .record_store_maintenance_run_state(StoreMaintenanceRunState {
                last_run_ms: Some(100),
                next_eligible_ms: 1_100,
                last_skip_reason: Some("mark_epoch_incomplete".to_string()),
                last_error: None,
                ..StoreMaintenanceRunState::default()
            })
            .unwrap();
        assert_eq!(
            loom.store()
                .maintenance_status()
                .unwrap()
                .last_validated_mark_epoch,
            0
        );
    }

    let loom = open_loom(tp.path()).unwrap();
    assert_eq!(
        loom.store()
            .active_reachability_mark_epoch()
            .unwrap()
            .unwrap()
            .epoch,
        epoch_id
    );
    let mut completed = false;
    for _ in 0..256 {
        let step = step_loom_reachability_mark_epoch(&loom, 2).unwrap();
        if step.completed {
            completed = true;
            break;
        }
    }
    assert!(completed);
    assert_eq!(
        loom.store()
            .maintenance_status()
            .unwrap()
            .last_validated_mark_epoch,
        epoch_id
    );
    let active = loom
        .store()
        .active_reachability_mark_epoch()
        .unwrap()
        .unwrap();
    assert!(
        loom.store()
            .active_reachability_mark_reclaim_evidence()
            .unwrap()
            .unwrap()
            .matches_epoch(&active, loom.store().digest_algo)
    );
    let expected = loom.live_object_set(loom.store().reference_root()).unwrap();
    assert!(expected.is_subset(&active.state.marked));
    assert!(active.state.completed);
    assert_eq!(
        loom.store()
            .inner
            .lock()
            .unwrap()
            .active_mark_epoch_reclaim_fence,
        Some(active.page_high_water_mark)
    );
    let ns = loom
        .registry()
        .open(&WsSelector::Typed {
            ty: FacetKind::Files,
            name: "p".to_string(),
        })
        .unwrap();
    assert_eq!(loom.read_file(ns, "f7.txt").unwrap(), b"v7");
}

#[test]
fn reachability_mark_epoch_persists_snapshot_identity() {
    let tp = TempPath::new("mark-epoch-snapshot-identity");
    let store = FileStore::open(tp.path()).unwrap();
    let digest = store.put(b"epoch root").unwrap();
    store.set_reference_root(Some(digest)).unwrap();
    store
        .control_set(b"epoch/control", b"control root".to_vec())
        .unwrap();
    let control_root = store.control_root().unwrap();
    let status = store.maintenance_status().unwrap();
    let state = loom_core::ReachabilityMarkState {
        pinned: BTreeSet::from([digest]),
        marked: BTreeSet::new(),
        queue: std::collections::VecDeque::from([digest]),
        stream_roots: std::collections::VecDeque::new(),
        content_roots: std::collections::VecDeque::new(),
        prolly_cursors: std::collections::VecDeque::new(),
        completed: false,
    };

    let epoch = store
        .begin_reachability_mark_epoch(Some(digest), BTreeSet::new(), state)
        .unwrap();

    assert_eq!(epoch.base_generation, status.generation);
    assert_eq!(epoch.page_high_water_mark, status.physical_page_count);
    assert_eq!(
        epoch
            .captured_root_vector
            .iter()
            .copied()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([digest, control_root])
    );
    assert!(store.get(&digest).unwrap().is_some());
    assert!(store.get(&control_root).unwrap().is_some());
    assert_eq!(
        store
            .active_reachability_mark_epoch()
            .unwrap()
            .unwrap()
            .reclaim_fence_identity,
        epoch.reclaim_fence_identity
    );
    assert_eq!(
        store.inner.lock().unwrap().active_mark_epoch_reclaim_fence,
        Some(status.physical_page_count)
    );
}

#[test]
fn reachability_mark_epoch_interleaving_cannot_reuse_fenced_pages() {
    let backing = SharedMem::default();
    let store = Arc::new(FileStore::with_backing(Box::new(backing.clone()), true).unwrap());
    let digest = store.put(b"epoch root").unwrap();
    store.set_reference_root(Some(digest)).unwrap();
    let high_water = store.maintenance_status().unwrap().physical_page_count;
    {
        let mut inner = store.inner.lock().unwrap();
        let fenced_page = (2..high_water)
            .find(|page| !inner.metadata_bootstrap_reserve.contains_page(*page))
            .expect("fixture requires one non-reserve page below the high-water mark");
        inner.free = vec![FreePageRun {
            start: fenced_page,
            len: 1,
            freed_gen: 1,
        }];
    }
    let state = loom_core::ReachabilityMarkState {
        pinned: BTreeSet::from([digest]),
        marked: BTreeSet::new(),
        queue: std::collections::VecDeque::from([digest]),
        stream_roots: std::collections::VecDeque::new(),
        content_roots: std::collections::VecDeque::new(),
        prolly_cursors: std::collections::VecDeque::new(),
        completed: false,
    };
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    store
        .set_reachability_epoch_pre_finish_hook_for_test(Box::new(move || {
            entered_tx.send(()).unwrap();
            release_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            Ok(())
        }))
        .unwrap();
    let begin_store = Arc::clone(&store);
    let begin = std::thread::spawn(move || {
        begin_store.begin_reachability_mark_epoch(Some(digest), BTreeSet::new(), state)
    });
    entered_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    let (write_started_tx, write_started_rx) = std::sync::mpsc::channel();
    let write_store = Arc::clone(&store);
    let writer = std::thread::spawn(move || {
        write_started_tx.send(()).unwrap();
        write_store.put(b"foreground after snapshot")
    });
    write_started_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    release_tx.send(()).unwrap();
    let epoch = begin.join().unwrap().unwrap();
    let written = writer.join().unwrap().unwrap();
    let loc = {
        let mut inner = store.inner.lock().unwrap();
        store
            .lookup_loc_locked(&mut inner, written.bytes())
            .unwrap()
            .unwrap()
    };

    assert_eq!(epoch.page_high_water_mark, high_water);
    assert!(loc.global_page() >= high_water);
    drop(store);
    let reopened = FileStore::with_backing(Box::new(backing), false).unwrap();
    let reopened_epoch = reopened.active_reachability_mark_epoch().unwrap().unwrap();
    assert_eq!(reopened_epoch, epoch);
    assert_eq!(
        reopened
            .inner
            .lock()
            .unwrap()
            .active_mark_epoch_reclaim_fence,
        Some(high_water)
    );
}

#[test]
fn reachability_mark_epoch_failure_before_publication_leaves_no_epoch_or_fence() {
    let backing = SharedMem::default();
    let store = FileStore::with_backing(Box::new(backing.clone()), true).unwrap();
    let digest = store.put(b"epoch root").unwrap();
    store.set_reference_root(Some(digest)).unwrap();
    let state = loom_core::ReachabilityMarkState {
        pinned: BTreeSet::from([digest]),
        marked: BTreeSet::new(),
        queue: std::collections::VecDeque::from([digest]),
        stream_roots: std::collections::VecDeque::new(),
        content_roots: std::collections::VecDeque::new(),
        prolly_cursors: std::collections::VecDeque::new(),
        completed: false,
    };
    store
        .set_reachability_epoch_pre_finish_hook_for_test(Box::new(|| {
            Err(LoomError::invalid("injected pre-publication failure"))
        }))
        .unwrap();

    assert!(
        store
            .begin_reachability_mark_epoch(Some(digest), BTreeSet::new(), state)
            .is_err()
    );
    assert!(store.active_reachability_mark_epoch().unwrap().is_none());
    assert_eq!(
        store.inner.lock().unwrap().active_mark_epoch_reclaim_fence,
        None
    );
    drop(store);
    let reopened = FileStore::with_backing(Box::new(backing), false).unwrap();
    assert!(reopened.active_reachability_mark_epoch().unwrap().is_none());
    assert_eq!(
        reopened
            .inner
            .lock()
            .unwrap()
            .active_mark_epoch_reclaim_fence,
        None
    );
}

#[test]
fn reachability_mark_epoch_post_commit_failure_recovers_epoch_and_fence_on_reopen() {
    let backing = SharedMem::default();
    let store = FileStore::with_backing(Box::new(backing.clone()), true).unwrap();
    let digest = store.put(b"epoch root").unwrap();
    store.set_reference_root(Some(digest)).unwrap();
    let high_water = store.maintenance_status().unwrap().physical_page_count;
    let state = loom_core::ReachabilityMarkState {
        pinned: BTreeSet::from([digest]),
        marked: BTreeSet::new(),
        queue: std::collections::VecDeque::from([digest]),
        stream_roots: std::collections::VecDeque::new(),
        content_roots: std::collections::VecDeque::new(),
        prolly_cursors: std::collections::VecDeque::new(),
        completed: false,
    };
    store
        .set_post_commit_pre_adopt_hook_for_test(Box::new(|_| {
            Err(LoomError::invalid("injected post-commit failure"))
        }))
        .unwrap();

    assert!(
        store
            .begin_reachability_mark_epoch(Some(digest), BTreeSet::new(), state)
            .is_err()
    );
    assert!(store.active_reachability_mark_epoch().unwrap().is_none());
    assert_eq!(
        store.inner.lock().unwrap().active_mark_epoch_reclaim_fence,
        None
    );
    drop(store);
    let reopened = FileStore::with_backing(Box::new(backing), false).unwrap();
    let epoch = reopened.active_reachability_mark_epoch().unwrap().unwrap();
    assert_eq!(epoch.page_high_water_mark, high_water);
    assert_eq!(
        reopened
            .inner
            .lock()
            .unwrap()
            .active_mark_epoch_reclaim_fence,
        Some(high_water)
    );
}

#[test]
fn reachability_mark_epoch_reopen_hydrates_reclaim_fence() {
    let tp = TempPath::new("mark-epoch-reopen-fence");
    let high_water;
    {
        let store = FileStore::open(tp.path()).unwrap();
        let digest = store.put(b"epoch root").unwrap();
        store.set_reference_root(Some(digest)).unwrap();
        let state = loom_core::ReachabilityMarkState {
            pinned: BTreeSet::from([digest]),
            marked: BTreeSet::new(),
            queue: std::collections::VecDeque::from([digest]),
            stream_roots: std::collections::VecDeque::new(),
            content_roots: std::collections::VecDeque::new(),
            prolly_cursors: std::collections::VecDeque::new(),
            completed: false,
        };
        high_water = store
            .begin_reachability_mark_epoch(Some(digest), BTreeSet::new(), state)
            .unwrap()
            .page_high_water_mark;
    }

    let reopened = FileStore::open(tp.path()).unwrap();
    let epoch = reopened.active_reachability_mark_epoch().unwrap().unwrap();
    assert_eq!(epoch.page_high_water_mark, high_water);
    assert_eq!(
        reopened
            .inner
            .lock()
            .unwrap()
            .active_mark_epoch_reclaim_fence,
        Some(high_water)
    );
}

#[test]
fn active_mark_epoch_reclaim_fence_filters_reusable_pages() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let free = vec![
        FreePageRun {
            start: 2,
            len: 3,
            freed_gen: 1,
        },
        FreePageRun {
            start: 8,
            len: 5,
            freed_gen: 1,
        },
        FreePageRun {
            start: 20,
            len: 2,
            freed_gen: 1,
        },
    ];

    let (reusable, _lease) = store
        .transaction_reusable_free(&free, Some(10), u64::MAX)
        .unwrap();

    assert_eq!(
        reusable,
        vec![
            FreePageRun {
                start: 10,
                len: 3,
                freed_gen: 1,
            },
            FreePageRun {
                start: 20,
                len: 2,
                freed_gen: 1,
            },
        ]
    );
    let mut allocator = PageAllocator::new_with_current_free_reusable(30, 9, reusable);
    assert_eq!(allocator.alloc(1), PageId(10));
    assert_eq!(allocator.alloc(1), PageId(11));
    assert_eq!(allocator.alloc(1), PageId(12));
    assert_eq!(allocator.alloc(1), PageId(20));
}

#[test]
fn foreground_reuse_requires_the_durable_horizon_and_active_epoch_fence() {
    let free = vec![
        FreePageRun {
            start: 8,
            len: 4,
            freed_gen: 1,
        },
        FreePageRun {
            start: 20,
            len: 3,
            freed_gen: 68,
        },
        FreePageRun {
            start: 30,
            len: 2,
            freed_gen: 40,
        },
    ];

    assert_eq!(
        foreground_recovery_safe_reusable_free(&free, Some(10), 40),
        vec![
            FreePageRun {
                start: 10,
                len: 2,
                freed_gen: 1,
            },
            FreePageRun {
                start: 30,
                len: 2,
                freed_gen: 40,
            },
        ]
    );
}

#[test]
fn committed_recovery_horizon_advances_and_reopens_with_each_root_set() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let key = durability_facet_test_key(b"documents", "recovery-horizon");

    store
        .put_mutable_overlay_value(key.clone(), b"one".to_vec())
        .unwrap();
    let first = {
        let inner = store.inner.lock().unwrap();
        assert_eq!(inner.minimum_recoverable_generation, inner.generation);
        inner.generation
    };
    store
        .put_mutable_overlay_value(key, b"two".to_vec())
        .unwrap();
    let second = {
        let inner = store.inner.lock().unwrap();
        assert!(inner.generation > first);
        assert_eq!(inner.minimum_recoverable_generation, inner.generation);
        inner.generation
    };
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let inner = reopened.inner.lock().unwrap();
    assert_eq!(inner.generation, second);
    assert_eq!(inner.minimum_recoverable_generation, second);
}

#[test]
fn torn_root_set_commit_does_not_advance_the_recovery_horizon() {
    let shared = SharedMem::default();
    let key = durability_facet_test_key(b"documents", "recovery-horizon-torn");
    {
        let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
        store
            .put_mutable_overlay_value(key.clone(), b"one".to_vec())
            .unwrap();
    }
    let committed_horizon = {
        let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
        let horizon = store.inner.lock().unwrap().minimum_recoverable_generation;
        drop(store);
        horizon
    };

    let failing = FailNthFsyncMem::new(shared.clone(), 2);
    let store = FileStore::with_backing(Box::new(failing), true).unwrap();
    assert!(
        store
            .put_mutable_overlay_value(key, b"two".to_vec())
            .is_err()
    );
    assert_eq!(
        store.inner.lock().unwrap().minimum_recoverable_generation,
        committed_horizon
    );
    drop(store);

    let failed_generation = committed_horizon.saturating_add(1);
    let failed_record_offset =
        JOURNAL_OFFSET + (failed_generation % RING_SLOTS) * journal::RECORD_SIZE as u64;
    shared.mutate_bytes(|bytes| bytes[failed_record_offset as usize] ^= 0xff);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    assert_eq!(
        reopened
            .inner
            .lock()
            .unwrap()
            .minimum_recoverable_generation,
        committed_horizon
    );
}

#[test]
fn reachability_mark_epoch_reference_root_advance_does_not_restart_or_invalidate_slice() {
    use loom_core::workspace::{FacetKind, WorkspaceId};

    let tp = TempPath::new("mark-epoch-conflict");
    let mut loom = open_loom(tp.path()).unwrap();
    let ns = loom
        .registry_mut()
        .create(
            FacetKind::Files,
            Some("p"),
            WorkspaceId::from_bytes([32; 16]),
        )
        .unwrap();
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
    loom.commit(ns, "nas", "initial", 1).unwrap();
    save_loom(&mut loom).unwrap();
    begin_loom_reachability_mark_epoch(&loom).unwrap();
    step_loom_reachability_mark_epoch(&loom, 1).unwrap();

    loom.write_file(ns, "b.txt", b"b", 0o100644).unwrap();
    loom.commit(ns, "nas", "concurrent", 2).unwrap();
    save_loom(&mut loom).unwrap();

    let mut completed = false;
    for _ in 0..256 {
        let step = step_loom_reachability_mark_epoch(&loom, 8).unwrap();
        if step.completed {
            completed = true;
            break;
        }
    }
    assert!(completed);
    let active = loom
        .store()
        .active_reachability_mark_epoch()
        .unwrap()
        .unwrap();
    assert_eq!(
        loom.store()
            .maintenance_status()
            .unwrap()
            .last_validated_mark_epoch,
        active.epoch
    );
    assert!(
        loom.store()
            .active_reachability_mark_reclaim_evidence()
            .unwrap()
            .unwrap()
            .matches_epoch(&active, loom.store().digest_algo)
    );
    assert!(active.state.completed);
    assert_eq!(
        loom.store()
            .inner
            .lock()
            .unwrap()
            .active_mark_epoch_reclaim_fence,
        Some(active.page_high_water_mark)
    );
}

#[test]
fn reachability_mark_epoch_control_and_derived_advancement_do_not_restart_or_invalidate_slice() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let root = store.put(&blob(b"snapshot-root")).unwrap();
    store.set_reference_root(Some(root)).unwrap();
    let state = loom_core::ReachabilityMarkState {
        pinned: BTreeSet::from([root]),
        marked: BTreeSet::new(),
        queue: std::collections::VecDeque::from([root]),
        stream_roots: std::collections::VecDeque::new(),
        content_roots: std::collections::VecDeque::new(),
        prolly_cursors: std::collections::VecDeque::new(),
        completed: false,
    };
    let loom = Loom::new(store);
    let epoch = loom
        .store()
        .begin_reachability_mark_epoch(Some(root), BTreeSet::new(), state)
        .unwrap();
    let original_fence = epoch.page_high_water_mark;
    loom.store()
        .control_set(b"application/config", b"changed".to_vec())
        .unwrap();
    let ns = loom_core::WorkspaceId::from_bytes([17; 16]);
    let key =
        DerivedArtifactKey::new(ns, loom_core::FacetKind::Vector, "embeddings", "hnsw").unwrap();
    let stamp = DerivedArtifactStamp::new(
        loom_core::Digest::blake3(b"vector-root"),
        "hnsw-0",
        "ann-v1",
    )
    .unwrap();
    loom.store()
        .put_derived_artifact(&key, stamp, b"native index payload")
        .unwrap();

    let step = step_loom_reachability_mark_epoch(&loom, 8).unwrap();

    assert!(step.completed);
    let active = loom
        .store()
        .active_reachability_mark_epoch()
        .unwrap()
        .unwrap();
    assert_eq!(active.epoch, epoch.epoch);
    assert_eq!(active.captured_root_vector, epoch.captured_root_vector);
    assert_eq!(active.page_high_water_mark, original_fence);
    assert_eq!(
        loom.store()
            .inner
            .lock()
            .unwrap()
            .active_mark_epoch_reclaim_fence,
        Some(original_fence)
    );
    assert_eq!(
        loom.store()
            .maintenance_status()
            .unwrap()
            .last_validated_mark_epoch,
        active.epoch
    );
    assert!(
        loom.store()
            .active_reachability_mark_reclaim_evidence()
            .unwrap()
            .unwrap()
            .matches_epoch(&active, loom.store().digest_algo)
    );
}

#[test]
fn reachability_mark_epoch_reopen_resumes_persisted_queue_not_current_roots() {
    let backing = SharedMem::default();
    let first;
    let second;
    let replacement;
    let epoch_id;
    {
        let store = FileStore::with_backing(Box::new(backing.clone()), true).unwrap();
        first = store.put(&blob(b"first snapshot object")).unwrap();
        second = store.put(&blob(b"second snapshot object")).unwrap();
        store.set_reference_root(Some(first)).unwrap();
        let state = loom_core::ReachabilityMarkState {
            pinned: BTreeSet::from([first, second]),
            marked: BTreeSet::new(),
            queue: std::collections::VecDeque::from([first, second]),
            stream_roots: std::collections::VecDeque::new(),
            content_roots: std::collections::VecDeque::new(),
            prolly_cursors: std::collections::VecDeque::new(),
            completed: false,
        };
        let loom = Loom::new(store);
        let epoch = loom
            .store()
            .begin_reachability_mark_epoch(Some(first), BTreeSet::new(), state)
            .unwrap();
        epoch_id = epoch.epoch;
        let step = step_loom_reachability_mark_epoch(&loom, 1).unwrap();
        assert_eq!(step.visited, 1);
        replacement = loom
            .store()
            .put(&blob(b"replacement current root"))
            .unwrap();
        loom.store().set_reference_root(Some(replacement)).unwrap();
    }

    let reopened = FileStore::with_backing(Box::new(backing), false).unwrap();
    let loom = Loom::new(reopened);
    assert_eq!(loom.store().reference_root(), Some(replacement));
    let before = loom
        .store()
        .active_reachability_mark_epoch()
        .unwrap()
        .unwrap();
    assert_eq!(before.epoch, epoch_id);
    assert!(before.state.marked.contains(&first));
    assert!(!before.state.marked.contains(&second));
    assert!(!before.state.marked.contains(&replacement));

    let step = step_loom_reachability_mark_epoch(&loom, 1).unwrap();

    assert_eq!(step.visited, 1);
    let after = loom
        .store()
        .active_reachability_mark_epoch()
        .unwrap()
        .unwrap();
    assert!(after.state.marked.contains(&second));
    assert!(!after.state.marked.contains(&replacement));
}

#[test]
fn reachability_mark_epoch_post_snapshot_objects_remain_conservatively_present() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let root = store.put(&blob(b"snapshot-root")).unwrap();
    store.set_reference_root(Some(root)).unwrap();
    let state = loom_core::ReachabilityMarkState {
        pinned: BTreeSet::from([root]),
        marked: BTreeSet::new(),
        queue: std::collections::VecDeque::from([root]),
        stream_roots: std::collections::VecDeque::new(),
        content_roots: std::collections::VecDeque::new(),
        prolly_cursors: std::collections::VecDeque::new(),
        completed: false,
    };
    let loom = Loom::new(store);
    let epoch = loom
        .store()
        .begin_reachability_mark_epoch(Some(root), BTreeSet::new(), state)
        .unwrap();
    let post_snapshot = loom.store().put(&blob(b"post snapshot object")).unwrap();
    let loc = {
        let mut inner = loom.store().inner.lock().unwrap();
        loom.store()
            .lookup_loc_locked(&mut inner, post_snapshot.bytes())
            .unwrap()
            .unwrap()
    };

    assert!(loc.global_page() >= epoch.page_high_water_mark);
    assert!(loom.store().has(&post_snapshot).unwrap());
    let step = step_loom_reachability_mark_epoch(&loom, 8).unwrap();
    assert!(step.completed);
    assert!(loom.store().has(&post_snapshot).unwrap());
    let active = loom
        .store()
        .active_reachability_mark_epoch()
        .unwrap()
        .unwrap();
    assert!(!active.state.marked.contains(&post_snapshot));
    assert_eq!(
        loom.store()
            .inner
            .lock()
            .unwrap()
            .active_mark_epoch_reclaim_fence,
        Some(epoch.page_high_water_mark)
    );
}

#[test]
fn reachability_mark_epoch_bounded_slices_progress_while_foreground_writes_continue() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let roots = (0..12)
        .map(|i| {
            store
                .put(&blob(format!("snapshot-{i}").as_bytes()))
                .unwrap()
        })
        .collect::<Vec<_>>();
    store.set_reference_root(Some(roots[0])).unwrap();
    let state = loom_core::ReachabilityMarkState {
        pinned: roots.iter().copied().collect(),
        marked: BTreeSet::new(),
        queue: roots.iter().copied().collect(),
        stream_roots: std::collections::VecDeque::new(),
        content_roots: std::collections::VecDeque::new(),
        prolly_cursors: std::collections::VecDeque::new(),
        completed: false,
    };
    let loom = Loom::new(store);
    let epoch = loom
        .store()
        .begin_reachability_mark_epoch(Some(roots[0]), BTreeSet::new(), state)
        .unwrap();
    let mut last_marked = 0usize;
    for i in 0..12 {
        let foreground = loom
            .store()
            .put(&blob(format!("foreground-{i}").as_bytes()))
            .unwrap();
        let loc = {
            let mut inner = loom.store().inner.lock().unwrap();
            loom.store()
                .lookup_loc_locked(&mut inner, foreground.bytes())
                .unwrap()
                .unwrap()
        };
        assert!(loc.global_page() >= epoch.page_high_water_mark);
        let step = step_loom_reachability_mark_epoch(&loom, 1).unwrap();
        assert!(step.visited <= 1);
        let active = loom
            .store()
            .active_reachability_mark_epoch()
            .unwrap()
            .unwrap();
        assert!(active.state.marked.len() >= last_marked);
        last_marked = active.state.marked.len();
        assert_eq!(
            loom.store()
                .inner
                .lock()
                .unwrap()
                .active_mark_epoch_reclaim_fence,
            Some(epoch.page_high_water_mark)
        );
    }
    let active = loom
        .store()
        .active_reachability_mark_epoch()
        .unwrap()
        .unwrap();
    assert!(roots.iter().all(|root| active.state.marked.contains(root)));
}

#[test]
fn gc_validated_segments_preserves_post_snapshot_commit_after_later_commit() {
    use loom_core::WsSelector;
    use loom_core::workspace::{FacetKind, WorkspaceId};

    let tp = TempPath::new("gc-validated-stale-commit");
    let ns;
    {
        let mut loom = open_loom(tp.path()).unwrap();
        ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("p"),
                WorkspaceId::from_bytes([33; 16]),
            )
            .unwrap();
        loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
        loom.commit(ns, "nas", "initial", 1).unwrap();
        save_loom(&mut loom).unwrap();
        begin_loom_reachability_mark_epoch(&loom).unwrap();
        let mut completed = false;
        for _ in 0..256 {
            let step = step_loom_reachability_mark_epoch(&loom, 8).unwrap();
            if step.completed {
                completed = true;
                break;
            }
        }
        assert!(completed);

        loom.write_file(ns, "b.txt", b"b", 0o100644).unwrap();
        loom.commit(ns, "nas", "later", 2).unwrap();
        save_loom(&mut loom).unwrap();

        loom.store_mut()
            .gc_validated_segments(GcSegmentBudget {
                max_segments: 1,
                max_pages: u64::MAX,
            })
            .unwrap();
        assert!(
            loom.store()
                .active_reachability_mark_epoch()
                .unwrap()
                .is_none()
        );
    }

    let loom = open_loom(tp.path()).unwrap();
    let reopened_ns = loom
        .registry()
        .open(&WsSelector::Typed {
            ty: FacetKind::Files,
            name: "p".to_string(),
        })
        .unwrap();
    assert_eq!(reopened_ns, ns);
    assert_eq!(loom.read_file(reopened_ns, "b.txt").unwrap(), b"b");
}

fn complete_validated_segment_epoch(store: &FileStore) {
    let n = 300usize;
    let mut digests = Vec::with_capacity(n);
    for i in 0..n {
        digests.push(store.put(&blob(format!("obj-{i:04}").as_bytes())).unwrap());
    }
    let live_digests = digests
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 10 == 0)
        .map(|(_, digest)| *digest)
        .collect::<BTreeSet<_>>();
    let state = loom_core::ReachabilityMarkState {
        pinned: BTreeSet::new(),
        marked: live_digests,
        queue: std::collections::VecDeque::new(),
        stream_roots: std::collections::VecDeque::new(),
        content_roots: std::collections::VecDeque::new(),
        prolly_cursors: std::collections::VecDeque::new(),
        completed: true,
    };
    let epoch = store
        .begin_reachability_mark_epoch(
            store.reference_root(),
            store.derived_artifact_roots().unwrap(),
            state,
        )
        .unwrap();
    store.complete_reachability_mark_epoch(&epoch).unwrap();
}

#[test]
fn gc_validated_segments_preserves_pre_reclaim_interleaved_write() {
    let tp = TempPath::new("gc-validated-pre-reclaim-interleave");
    let mut store = FileStore::open(tp.path()).unwrap();
    complete_validated_segment_epoch(&store);
    let new_root = store.put(&blob(b"new-root")).unwrap();

    store
        .gc_validated_segments_with_pre_reclaim_interleave(
            GcSegmentBudget {
                max_segments: 1,
                max_pages: u64::MAX,
            },
            |store| store.set_reference_root(Some(new_root)),
        )
        .unwrap();
    assert!(store.active_reachability_mark_epoch().unwrap().is_none());
    assert!(store.has(&new_root).unwrap());
    assert_eq!(store.reference_root(), Some(new_root));
}

#[test]
fn gc_validated_segments_allows_foreground_write_during_read_phase() {
    let tp = TempPath::new("gc-validated-read-phase-write");
    let mut store = FileStore::open(tp.path()).unwrap();
    complete_validated_segment_epoch(&store);
    let mut foreground = None;

    store
        .gc_validated_segments_with_read_phase_interleave(
            GcSegmentBudget {
                max_segments: 1,
                max_pages: u64::MAX,
            },
            |store| {
                let digest = store.put(&blob(b"foreground-write"))?;
                foreground = Some(digest);
                Ok(())
            },
        )
        .unwrap();
    let foreground = foreground.expect("foreground write did not run");
    assert!(store.has(&foreground).unwrap());
    assert!(store.active_reachability_mark_epoch().unwrap().is_none());
}

#[test]
fn gc_validated_segments_keeps_snapshot_identity_after_branch_change() {
    use loom_core::workspace::{FacetKind, WorkspaceId};

    let tp = TempPath::new("gc-validated-stale-branch");
    let mut loom = open_loom(tp.path()).unwrap();
    let ns = loom
        .registry_mut()
        .create(
            FacetKind::Files,
            Some("p"),
            WorkspaceId::from_bytes([34; 16]),
        )
        .unwrap();
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
    loom.commit(ns, "nas", "initial", 1).unwrap();
    save_loom(&mut loom).unwrap();
    begin_loom_reachability_mark_epoch(&loom).unwrap();
    for _ in 0..256 {
        if step_loom_reachability_mark_epoch(&loom, 8)
            .unwrap()
            .completed
        {
            break;
        }
    }

    loom.branch(ns, "feature").unwrap();
    save_loom(&mut loom).unwrap();
    loom.store_mut()
        .gc_validated_segments(GcSegmentBudget {
            max_segments: 1,
            max_pages: u64::MAX,
        })
        .unwrap();
    assert!(
        loom.store()
            .active_reachability_mark_epoch()
            .unwrap()
            .is_none()
    );
}

#[test]
fn gc_validated_segments_keeps_snapshot_identity_after_tag_change() {
    use loom_core::workspace::{FacetKind, WorkspaceId};

    let tp = TempPath::new("gc-validated-stale-tag");
    let mut loom = open_loom(tp.path()).unwrap();
    let ns = loom
        .registry_mut()
        .create(
            FacetKind::Files,
            Some("p"),
            WorkspaceId::from_bytes([35; 16]),
        )
        .unwrap();
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
    loom.commit(ns, "nas", "initial", 1).unwrap();
    save_loom(&mut loom).unwrap();
    begin_loom_reachability_mark_epoch(&loom).unwrap();
    for _ in 0..256 {
        if step_loom_reachability_mark_epoch(&loom, 8)
            .unwrap()
            .completed
        {
            break;
        }
    }

    loom.tag_create(ns, "v1", "HEAD", "", "", 0).unwrap();
    save_loom(&mut loom).unwrap();
    loom.store_mut()
        .gc_validated_segments(GcSegmentBudget {
            max_segments: 1,
            max_pages: u64::MAX,
        })
        .unwrap();
    assert!(
        loom.store()
            .active_reachability_mark_epoch()
            .unwrap()
            .is_none()
    );
}

#[test]
fn gc_drops_commit_only_reachable_from_deleted_branch() {
    use loom_core::workspace::{DEFAULT_BRANCH, FacetKind, WorkspaceId};

    let tp = TempPath::new("gc-deleted-branch-root");
    let mut loom = open_loom(tp.path()).unwrap();
    let ns = loom
        .registry_mut()
        .create(
            FacetKind::Files,
            Some("p"),
            WorkspaceId::from_bytes([37; 16]),
        )
        .unwrap();
    loom.write_file(ns, "shared.txt", b"shared", 0o100644)
        .unwrap();
    let main = loom.commit(ns, "nas", "main", 1).unwrap();
    loom.branch(ns, "feature").unwrap();
    loom.checkout_branch(ns, "feature").unwrap();
    loom.write_file(ns, "unique.txt", b"unique", 0o100644)
        .unwrap();
    let feature = loom.commit(ns, "nas", "feature", 2).unwrap();
    loom.checkout_branch(ns, DEFAULT_BRANCH).unwrap();
    loom.branch_delete(ns, "feature").unwrap();
    save_loom(&mut loom).unwrap();

    let live = loom.live_object_set(loom.store().reference_root()).unwrap();
    assert!(live.contains(&main));
    assert!(!live.contains(&feature));
    gc_loom(&mut loom).unwrap();
    assert!(loom.store().has(&main).unwrap());
    assert!(!loom.store().has(&feature).unwrap());
}

#[test]
fn gc_drops_commit_only_reachable_from_deleted_tag() {
    use loom_core::workspace::{DEFAULT_BRANCH, FacetKind, WorkspaceId};

    let tp = TempPath::new("gc-deleted-tag-root");
    let mut loom = open_loom(tp.path()).unwrap();
    let ns = loom
        .registry_mut()
        .create(
            FacetKind::Files,
            Some("p"),
            WorkspaceId::from_bytes([38; 16]),
        )
        .unwrap();
    loom.write_file(ns, "shared.txt", b"shared", 0o100644)
        .unwrap();
    let main = loom.commit(ns, "nas", "main", 1).unwrap();
    loom.branch(ns, "feature").unwrap();
    loom.checkout_branch(ns, "feature").unwrap();
    loom.write_file(ns, "tagged.txt", b"tagged", 0o100644)
        .unwrap();
    let tagged = loom.commit(ns, "nas", "tagged", 2).unwrap();
    loom.tag_create(ns, "snapshot", &tagged.to_string(), "nas", "", 3)
        .unwrap();
    loom.checkout_branch(ns, DEFAULT_BRANCH).unwrap();
    loom.branch_delete(ns, "feature").unwrap();
    save_loom(&mut loom).unwrap();
    assert!(
        loom.live_object_set(loom.store().reference_root())
            .unwrap()
            .contains(&tagged)
    );

    loom.tag_delete(ns, "snapshot").unwrap();
    save_loom(&mut loom).unwrap();
    let live = loom.live_object_set(loom.store().reference_root()).unwrap();
    assert!(live.contains(&main));
    assert!(!live.contains(&tagged));
    gc_loom(&mut loom).unwrap();
    assert!(loom.store().has(&main).unwrap());
    assert!(!loom.store().has(&tagged).unwrap());
}

#[test]
fn gc_validated_segments_ignores_maintenance_metadata_changes() {
    let tp = TempPath::new("gc-validated-maintenance-metadata");
    let mut store = FileStore::open(tp.path()).unwrap();
    let digest = store.put(&blob(b"live")).unwrap();
    let state = loom_core::ReachabilityMarkState {
        pinned: BTreeSet::from([digest]),
        marked: BTreeSet::from([digest]),
        queue: std::collections::VecDeque::new(),
        stream_roots: std::collections::VecDeque::new(),
        content_roots: std::collections::VecDeque::new(),
        prolly_cursors: std::collections::VecDeque::new(),
        completed: true,
    };
    let epoch = store
        .begin_reachability_mark_epoch(None, BTreeSet::new(), state)
        .unwrap();
    store.complete_reachability_mark_epoch(&epoch).unwrap();
    store
        .set_store_maintenance_policy(StoreMaintenancePolicy {
            min_candidate_pages: 0,
            min_reusable_pages: 0,
            interval_ms: 1_000,
            backoff_ms: 2_000,
            max_segments: 1,
            max_pages: 64,
            full_compaction_enabled: false,
            ..StoreMaintenancePolicy::default()
        })
        .unwrap();
    store
        .record_store_maintenance_run_state(StoreMaintenanceRunState {
            last_run_ms: Some(100),
            next_eligible_ms: 1_100,
            last_skip_reason: Some("mark_epoch_incomplete".to_string()),
            last_error: None,
            ..StoreMaintenanceRunState::default()
        })
        .unwrap();

    store
        .gc_validated_segments(GcSegmentBudget {
            max_segments: 1,
            max_pages: u64::MAX,
        })
        .unwrap();
    assert!(store.active_reachability_mark_epoch().unwrap().is_none());
}

#[test]
fn gc_validated_segments_preserves_control_root_changes() {
    let tp = TempPath::new("gc-validated-stale-control");
    let mut store = FileStore::open(tp.path()).unwrap();
    complete_validated_segment_epoch(&store);
    store
        .control_set(b"application/config", b"changed".to_vec())
        .unwrap();

    store
        .gc_validated_segments(GcSegmentBudget {
            max_segments: 1,
            max_pages: u64::MAX,
        })
        .unwrap();
    assert!(store.active_reachability_mark_epoch().unwrap().is_none());
    assert_eq!(
        store.control_get(b"application/config").unwrap().as_deref(),
        Some(b"changed".as_slice())
    );
}

#[test]
fn control_set_with_reference_commits_both_roots_atomically() {
    let tp = TempPath::new("atomic-control-reference");
    let store = FileStore::open(tp.path()).unwrap();
    // A reference-root digest to publish atomically with the control value. For this store-level
    // atomicity check it only needs to be a stored object digest.
    let reference = store.put(b"reference-root-object").unwrap();
    let key = b"profile/tickets/v2/ws/state";
    let gen_before = store.generation();

    store
        .control_set_with_reference(key, b"state-bytes".to_vec(), Some(reference))
        .unwrap();

    // Exactly one superblock swap advanced BOTH roots together: no interruption could expose one
    // root advanced without the other (the mixed committed state a recovery pass would face).
    assert_eq!(store.generation(), gen_before + 1);
    assert_eq!(store.reference_root(), Some(reference));
    assert_eq!(
        store.control_get(key).unwrap().as_deref(),
        Some(b"state-bytes".as_slice())
    );

    // Durable across reopen from the single atomic commit.
    drop(store);
    let re = FileStore::open(tp.path()).unwrap();
    assert_eq!(re.reference_root(), Some(reference));
    assert_eq!(
        re.control_get(key).unwrap().as_deref(),
        Some(b"state-bytes".as_slice())
    );
}

#[test]
fn gc_validated_segments_preserves_derived_artifact_root_changes() {
    let tp = TempPath::new("gc-validated-stale-derived");
    let mut store = FileStore::open(tp.path()).unwrap();
    complete_validated_segment_epoch(&store);
    let ns = loom_core::WorkspaceId::from_bytes([36; 16]);
    let key =
        DerivedArtifactKey::new(ns, loom_core::FacetKind::Vector, "embeddings", "hnsw").unwrap();
    let stamp = DerivedArtifactStamp::new(
        loom_core::Digest::blake3(b"vector-root"),
        "hnsw-0",
        "ann-v1",
    )
    .unwrap();
    store
        .put_derived_artifact(&key, stamp, b"native index payload")
        .unwrap();

    store
        .gc_validated_segments(GcSegmentBudget {
            max_segments: 1,
            max_pages: u64::MAX,
        })
        .unwrap();
    assert!(store.active_reachability_mark_epoch().unwrap().is_none());
    assert!(store.derived_artifact_record(&key).unwrap().is_some());
}

/// Build encryption metadata + an unlocked session from fixed test inputs (no RNG in the key layer).
fn test_encryption() -> (Vec<u8>, loom_core::keys::DekSession) {
    let (meta, session) = loom_core::keys::EncryptionMeta::create(
        &loom_core::keys::KeySpec::passphrase("pw"),
        loom_core::keys::Suite::Aes256Gcm,
        [7u8; 16].to_vec(),
        [0x42; 32],
        [9u8; 24].to_vec(),
    )
    .unwrap();
    (meta.encode(), session)
}

#[test]
fn encrypted_store_persists_meta_across_reopen_commits_and_rekey() {
    use loom_core::keys::{EncryptionMeta, KeySpec};
    let shared = SharedMem::default();
    let (meta_bytes, session) = test_encryption();
    let store = FileStore::with_backing_encrypted(
        Box::new(shared.clone()),
        meta_bytes.clone(),
        session,
        Algo::Blake3,
    )
    .unwrap();
    assert!(store.is_encrypted() && store.is_unlocked());
    // Drive enough reference-root commits to cross a checkpoint interval, so the superblock is
    // rewritten and we prove the immutable encryption_meta is carried forward, not erased.
    let d = store
        .put(b"a secret-bearing object payload of a reasonable size for framing")
        .unwrap();
    for _ in 0..(CHECKPOINT_INTERVAL + 2) {
        store.set_reference_root(Some(d)).unwrap();
        store.set_reference_root(None).unwrap();
    }
    drop(store);

    // Reopen over the same bytes: still encrypted, meta round-trips byte-for-byte, and locked (no DEK).
    let re = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    assert!(re.is_encrypted() && !re.is_unlocked());
    assert_eq!(
        re.encryption_meta().unwrap().unwrap(),
        EncryptionMeta::decode(&meta_bytes).unwrap()
    );
    // Wrong passphrase is E2eKeyInvalid; the right one unlocks.
    assert_eq!(
        re.unlock(&KeySpec::passphrase("nope")).unwrap_err().code,
        Code::E2eKeyInvalid
    );
    re.unlock(&KeySpec::passphrase("pw")).unwrap();
    assert!(re.is_unlocked());
    // Rekey under a new passphrase, then reopen: the old passphrase no longer unlocks, the new one
    // does - proving the rewrapped meta is durable (forced checkpoint).
    re.rekey(
        &KeySpec::passphrase("pw2"),
        [1u8; 16].to_vec(),
        [2u8; 24].to_vec(),
    )
    .unwrap();
    drop(re);
    let re2 = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    assert_eq!(
        re2.unlock(&KeySpec::passphrase("pw")).unwrap_err().code,
        Code::E2eKeyInvalid
    );
    re2.unlock(&KeySpec::passphrase("pw2")).unwrap();
}

#[test]
fn encrypted_store_adds_and_removes_wraps_durably() {
    use loom_core::keys::{KeySpec, WrapSource};
    let shared = SharedMem::default();
    let (meta_bytes, session) = test_encryption();
    let store = FileStore::with_backing_encrypted(
        Box::new(shared.clone()),
        meta_bytes,
        session,
        Algo::Blake3,
    )
    .unwrap();
    let digest = store.put(b"secret").unwrap();
    let kek = [0x5au8; loom_core::keys::KEY_LEN];
    store
        .add_wrap(
            &KeySpec::raw_kek(kek),
            Vec::new(),
            [3u8; 24].to_vec(),
            false,
        )
        .unwrap();
    let meta = store.encryption_meta().unwrap().unwrap();
    assert_eq!(meta.wraps.len(), 2);
    assert_eq!(meta.wraps[0].source, WrapSource::Passphrase);
    assert_eq!(meta.wraps[1].source, WrapSource::RawKek);
    drop(store);

    let by_passphrase = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    by_passphrase.unlock(&KeySpec::passphrase("pw")).unwrap();
    assert_eq!(by_passphrase.get(&digest).unwrap().unwrap(), b"secret");
    drop(by_passphrase);

    let by_kek = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    by_kek.unlock(&KeySpec::raw_kek(kek)).unwrap();
    assert_eq!(by_kek.get(&digest).unwrap().unwrap(), b"secret");
    assert_eq!(
        by_kek.remove_wrap(0, false).unwrap_err().code,
        Code::InvalidArgument
    );
    by_kek.remove_wrap(0, true).unwrap();
    drop(by_kek);

    let after_remove = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    assert_eq!(
        after_remove
            .unlock(&KeySpec::passphrase("pw"))
            .unwrap_err()
            .code,
        Code::E2eKeyInvalid
    );
    after_remove.unlock(&KeySpec::raw_kek(kek)).unwrap();
    assert_eq!(after_remove.get(&digest).unwrap().unwrap(), b"secret");
}

#[test]
fn unencrypted_store_reports_not_encrypted_and_rejects_unlock() {
    let store = FileStore::with_backing(Box::new(MemoryBacking::new()), true).unwrap();
    assert!(!store.is_encrypted());
    assert!(store.encryption_meta().unwrap().is_none());
    assert_eq!(
        store
            .unlock(&loom_core::keys::KeySpec::passphrase("x"))
            .unwrap_err()
            .code,
        Code::Unsupported
    );
}

#[test]
fn cannot_enable_encryption_on_an_existing_store() {
    let shared = SharedMem::default();
    {
        let s = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
        s.put(b"already has data").unwrap();
    }
    let (meta_bytes, session) = test_encryption();
    let err = FileStore::with_backing_encrypted(
        Box::new(shared.clone()),
        meta_bytes,
        session,
        Algo::Blake3,
    )
    .unwrap_err();
    assert_eq!(err.code, Code::AlreadyExists);
}

/// Build an unlocked encrypted store over `shared` with the given suite, from fixed test inputs.
fn encrypted_over(shared: &SharedMem, suite: loom_core::keys::Suite) -> FileStore {
    let (meta, session) = loom_core::keys::EncryptionMeta::create(
        &loom_core::keys::KeySpec::passphrase("pw"),
        suite,
        [7u8; 16].to_vec(),
        [0x42; 32],
        [9u8; 24].to_vec(),
    )
    .unwrap();
    FileStore::with_backing_encrypted(
        Box::new(shared.clone()),
        meta.encode(),
        session,
        Algo::Blake3,
    )
    .unwrap()
}

/// On an unlocked encrypted store an object round-trips through `get`, but the plaintext is never
/// written to the backing (no plaintext object frame), and reopening locked makes reads return
/// `E2eLocked`. Both suites are exercised so the XChaCha keyed-BLAKE3 and AES-GCM HKDF CEK paths
/// both round-trip end to end.
#[test]
fn encrypted_object_round_trips_unlocked_and_never_stores_plaintext() {
    use loom_core::keys::{KeySpec, Suite};
    for suite in [Suite::XChaCha20Poly1305, Suite::Aes256Gcm] {
        let shared = SharedMem::default();
        // A long, compressible, recognizable plaintext: large enough to take a real inner codec,
        // and a distinctive marker we can search for in the raw backing.
        let marker = b"TOPSECRET-MARKER-do-not-leak-this-string";
        let mut plain = Vec::new();
        while plain.len() < 4096 {
            plain.extend_from_slice(marker);
            plain.extend_from_slice(b" the quick brown loom commit tree branch ");
        }
        let store = encrypted_over(&shared, suite);
        let d = store.put(&plain).unwrap();
        assert_eq!(
            store.get(&d).unwrap().unwrap(),
            plain,
            "round trip {suite:?}"
        );
        drop(store);

        // The raw backing must not contain the plaintext marker anywhere.
        let raw = shared.0.lock().unwrap().clone();
        assert!(
            !raw.windows(marker.len()).any(|w| w == marker),
            "plaintext marker leaked into the backing under {suite:?}"
        );

        // Reopen locked: the object is present but reads are E2eLocked until unlocked.
        let locked = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
        assert!(locked.has(&d).unwrap());
        assert_eq!(locked.get(&d).unwrap_err().code, Code::E2eLocked);
        locked.unlock(&KeySpec::passphrase("pw")).unwrap();
        assert_eq!(
            locked.get(&d).unwrap().unwrap(),
            plain,
            "post-unlock {suite:?}"
        );
    }
}

/// Corrupting a stored object byte makes `get` fail (CRC or AEAD) rather than return wrong or
/// partial plaintext: the record CRC catches accidental corruption, and the frame-level tests cover
/// CRC-consistent (adversarial) tampering failing AEAD authentication before any plaintext.
#[test]
fn corrupting_an_encrypted_record_byte_fails_get_not_leaks() {
    let shared = SharedMem::default();
    let store = encrypted_over(&shared, loom_core::keys::Suite::Aes256Gcm);
    let plain = b"a single small encrypted object record".to_vec();
    let d = store.put(&plain).unwrap();
    drop(store);

    // The first data page holds the slab with this lone record; flip a byte inside the record's
    // framed bytes (well past the slab header) and confirm the read no longer yields the plaintext.
    {
        let mut g = shared.0.lock().unwrap();
        let pos = DATA_START as usize + 64;
        g[pos] ^= 0xff;
    }
    let reopened = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    reopened
        .unlock(&loom_core::keys::KeySpec::passphrase("pw"))
        .unwrap();
    let got = reopened.get(&d);
    assert!(
        got.is_err() || got.as_ref().unwrap().as_deref() != Some(plain.as_slice()),
        "corrupted record must not return the original plaintext"
    );
}

/// Compaction rewrites every record into a fresh file; on an encrypted store the relocated records
/// must be re-sealed (not demoted to plaintext) and still decrypt afterward. This exercises the
/// compaction write path, which reads each object through `get` (decrypt) then re-seals on write.
#[test]
fn compaction_reseals_encrypted_records() {
    use loom_core::keys::{EncryptionMeta, KeySpec, Suite};
    let tmp = TempPath::new("enc-compact");
    let marker = b"COMPACT-SECRET-MARKER";
    let mut digests = Vec::new();
    {
        let (meta, session) = EncryptionMeta::create(
            &KeySpec::passphrase("pw"),
            Suite::Aes256Gcm,
            [7u8; 16].to_vec(),
            [0x42; 32],
            [9u8; 24].to_vec(),
        )
        .unwrap();
        let mut store = FileStore::create_encrypted(tmp.path(), meta.encode(), session).unwrap();
        for i in 0..8u8 {
            let mut obj = marker.to_vec();
            obj.push(i);
            digests.push(store.put(&obj).unwrap());
        }
        store.compact().unwrap();
        // Every object still decrypts to its plaintext after the rewrite.
        for (i, d) in digests.iter().enumerate() {
            let mut want = marker.to_vec();
            want.push(i as u8);
            assert_eq!(store.get(d).unwrap().unwrap(), want);
        }
    }
    // The compacted file on disk contains no plaintext marker.
    let raw = std::fs::read(tmp.path()).unwrap();
    assert!(
        !raw.windows(marker.len()).any(|w| w == marker),
        "plaintext leaked into the compacted file"
    );
    // Reopen the compacted file: locked, and unlock-then-read still works.
    let re = FileStore::open(tmp.path()).unwrap();
    assert!(re.is_encrypted() && !re.is_unlocked());
    re.unlock(&KeySpec::passphrase("pw")).unwrap();
    let mut want0 = marker.to_vec();
    want0.push(0);
    assert_eq!(re.get(&digests[0]).unwrap().unwrap(), want0);
}

/// The rekey data pass rotates the DEK and the suite by re-sealing every object:
/// after it, the old passphrase no longer unlocks, the new one does, objects still decrypt to their
/// plaintext, the on-disk suite changed, and no plaintext leaked. The plaintext digests (object
/// identity) are unchanged, so the same handles read the same objects.
#[test]
fn rekey_reseal_rotates_dek_and_suite() {
    use loom_core::keys::{EncryptionMeta, KeySpec, Suite};
    let tmp = TempPath::new("enc-rekey");
    let marker = b"REKEY-SECRET-MARKER";
    let mut digests = Vec::new();
    let (meta0, sess0) = EncryptionMeta::create(
        &KeySpec::passphrase("old-pw"),
        Suite::XChaCha20Poly1305,
        [7u8; 16].to_vec(),
        [0x11; 32],
        [9u8; 24].to_vec(),
    )
    .unwrap();
    let mut store = FileStore::create_encrypted(tmp.path(), meta0.encode(), sess0).unwrap();
    for i in 0..6u8 {
        let mut obj = marker.to_vec();
        obj.push(i);
        digests.push(store.put(&obj).unwrap());
    }
    // Rotate to a fresh DEK under the AES-256-GCM suite and a new passphrase, re-sealing all objects.
    let (meta1, sess1) = EncryptionMeta::create(
        &KeySpec::passphrase("new-pw"),
        Suite::Aes256Gcm,
        [3u8; 16].to_vec(),
        [0x22; 32],
        [4u8; 24].to_vec(), // the DEK wrap always uses XChaCha20-Poly1305 (24-byte nonce)
    )
    .unwrap();
    store.rekey_reseal(meta1.encode(), sess1).unwrap();
    // The handle stays unlocked under the new key and reads every object.
    assert!(store.is_unlocked());
    for (i, d) in digests.iter().enumerate() {
        let mut want = marker.to_vec();
        want.push(i as u8);
        assert_eq!(store.get(d).unwrap().unwrap(), want);
    }
    drop(store);

    // On-disk: no plaintext leak, and the recorded suite is now AES-256-GCM.
    let raw = std::fs::read(tmp.path()).unwrap();
    assert!(!raw.windows(marker.len()).any(|w| w == marker));
    let re = FileStore::open(tmp.path()).unwrap();
    assert_eq!(
        re.encryption_meta().unwrap().unwrap().active_suite,
        Suite::Aes256Gcm
    );
    // The old passphrase no longer unlocks; the new one does and reads the re-sealed objects.
    assert_eq!(
        re.unlock(&KeySpec::passphrase("old-pw")).unwrap_err().code,
        Code::E2eKeyInvalid
    );
    re.unlock(&KeySpec::passphrase("new-pw")).unwrap();
    let mut want0 = marker.to_vec();
    want0.push(0);
    assert_eq!(re.get(&digests[0]).unwrap().unwrap(), want0);
}

/// rekey-reseal requires an encrypted, unlocked store: an unencrypted store is `Unsupported` and a
/// locked one is `E2eLocked` (it cannot read objects to re-seal them).
#[test]
fn rekey_reseal_requires_encrypted_and_unlocked() {
    use loom_core::keys::{EncryptionMeta, KeySpec, Suite};
    let tmp = TempPath::new("enc-rekey-guard");
    // Unencrypted store -> Unsupported.
    let (meta, session) = EncryptionMeta::create(
        &KeySpec::passphrase("pw"),
        Suite::Aes256Gcm,
        [7u8; 16].to_vec(),
        [0x42; 32],
        [9u8; 24].to_vec(),
    )
    .unwrap();
    {
        let mut plain = FileStore::open(tmp.path()).unwrap();
        assert_eq!(
            plain.rekey_reseal(meta.encode(), session).unwrap_err().code,
            Code::Unsupported
        );
    }
    // Encrypted but locked -> E2eLocked.
    let tmp2 = TempPath::new("enc-rekey-locked");
    let (m0, s0) = EncryptionMeta::create(
        &KeySpec::passphrase("pw"),
        Suite::Aes256Gcm,
        [7u8; 16].to_vec(),
        [0x42; 32],
        [9u8; 24].to_vec(),
    )
    .unwrap();
    {
        let s = FileStore::create_encrypted(tmp2.path(), m0.encode(), s0).unwrap();
        s.put(b"x").unwrap();
    }
    let (m1, s1) = EncryptionMeta::create(
        &KeySpec::passphrase("pw2"),
        Suite::Aes256Gcm,
        [1u8; 16].to_vec(),
        [0x43; 32],
        [2u8; 24].to_vec(),
    )
    .unwrap();
    let mut locked = FileStore::open(tmp2.path()).unwrap();
    assert!(locked.is_encrypted() && !locked.is_unlocked());
    assert_eq!(
        locked.rekey_reseal(m1.encode(), s1).unwrap_err().code,
        Code::E2eLocked
    );
}

/// A FIPS-profile store addresses objects with SHA-256, not blake3: `put`
/// returns a `sha256` digest equal to `Digest::hash(Sha256, canonical)`, `get` round-trips, the
/// profile is recorded in the superblock and survives reopen, and the identity is profile-specific
/// (the blake3 address of the same bytes is not the address here).
#[test]
fn fips_profile_store_addresses_with_sha256() {
    let tmp = TempPath::new("fips-profile");
    let canonical = b"a canonical object under the FIPS identity profile".to_vec();
    let d = {
        let store = FileStore::create_with_profile(tmp.path(), Algo::Sha256).unwrap();
        assert_eq!(store.digest_algo(), Algo::Sha256);
        let d = store.put(&canonical).unwrap();
        assert_eq!(d.algo(), Algo::Sha256);
        assert_eq!(d, Digest::hash(Algo::Sha256, &canonical));
        assert_ne!(d.bytes(), Digest::blake3(&canonical).bytes());
        assert_eq!(store.get(&d).unwrap().unwrap(), canonical);
        d
    };
    // Reopen: the profile is read back from the superblock, and the object still round-trips.
    let re = FileStore::open(tmp.path()).unwrap();
    assert_eq!(re.digest_algo(), Algo::Sha256);
    assert_eq!(re.get(&d).unwrap().unwrap(), canonical);
}

/// The default profile remains blake3, and survives reopen.
#[test]
fn default_profile_store_addresses_with_blake3() {
    let tmp = TempPath::new("default-profile");
    let store = FileStore::open(tmp.path()).unwrap();
    assert_eq!(store.digest_algo(), Algo::Blake3);
    let d = store.put(b"obj").unwrap();
    assert_eq!(d, Digest::blake3(b"obj"));
    drop(store);
    assert_eq!(
        FileStore::open(tmp.path()).unwrap().digest_algo(),
        Algo::Blake3
    );
}

/// Corrupted encryption metadata is rejected, not silently accepted: the encoded `EncryptionMeta`
/// (which the superblock stores inside its CRC-covered span) fails to decode once tampered. The
/// superblock's own CRC additionally guards the in-place bytes on every reopen (see #147b).
#[test]
fn corrupted_encryption_meta_fails_to_decode() {
    use loom_core::keys::EncryptionMeta;
    let (meta_bytes, _session) = test_encryption();
    assert!(EncryptionMeta::decode(&meta_bytes).is_ok());
    let mut corrupt = meta_bytes.clone();
    corrupt[0] ^= 0xff; // break the "LKM1" magic
    assert!(EncryptionMeta::decode(&corrupt).is_err());
    let mut truncated = meta_bytes.clone();
    truncated.truncate(meta_bytes.len() - 1); // a short buffer must not panic or half-decode
    assert!(EncryptionMeta::decode(&truncated).is_err());
}

/// A unique temp path; the file is removed by [`TempPath`]'s drop.
struct TempPath(std::path::PathBuf);
impl TempPath {
    fn new(tag: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let mut p = std::env::temp_dir();
        p.push(format!("loomstore-{tag}-{pid}-{n}.loom"));
        let _ = std::fs::remove_file(&p);
        Self(p)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn blob(s: &[u8]) -> Vec<u8> {
    Object::Blob(s.to_vec()).canonical()
}

#[test]
fn put_get_has_len_and_idempotent() {
    let tp = TempPath::new("basic");
    let store = FileStore::open(tp.path()).unwrap();
    assert!(store.is_empty());

    let c = blob(b"hello loom");
    let d = store.put(&c).unwrap();
    assert_eq!(d, Digest::blake3(&c));
    assert!(store.has(&d).unwrap());
    assert_eq!(store.get(&d).unwrap().as_deref(), Some(c.as_slice()));
    assert_eq!(store.len(), 1);

    // Idempotent: same content, same digest, no growth.
    let d2 = store.put(&c).unwrap();
    assert_eq!(d, d2);
    assert_eq!(store.len(), 1);

    // Absent object.
    let absent = Digest::blake3(&blob(b"absent"));
    assert!(!store.has(&absent).unwrap());
    assert_eq!(store.get(&absent).unwrap(), None);
}

#[test]
fn control_plane_map_survives_reopen_and_delete() {
    let tp = TempPath::new("control-map");
    {
        let store = FileStore::open(tp.path()).unwrap();
        store
            .control_set(b"lock/ns/a", b"fence-1".to_vec())
            .unwrap();
        store
            .control_set(b"lock/ns/b", b"fence-2".to_vec())
            .unwrap();
        store.control_set(b"cache/ns/a", b"value".to_vec()).unwrap();
        assert_eq!(
            store.control_get(b"lock/ns/a").unwrap().as_deref(),
            Some(&b"fence-1"[..])
        );
        assert_eq!(
            store.control_scan_prefix(b"lock/ns/").unwrap(),
            vec![
                (b"lock/ns/a".to_vec(), b"fence-1".to_vec()),
                (b"lock/ns/b".to_vec(), b"fence-2".to_vec()),
            ]
        );
    }
    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(
        store.control_get(b"cache/ns/a").unwrap().as_deref(),
        Some(&b"value"[..])
    );
    assert!(store.control_delete(b"cache/ns/a").unwrap());
    assert!(!store.control_delete(b"cache/ns/a").unwrap());
    assert_eq!(store.control_get(b"cache/ns/a").unwrap(), None);
}

#[test]
fn lock_fence_state_survives_reopen() {
    let tp = TempPath::new("lock-fence");
    let key = b"sync/branch/ns/main";
    {
        let store = FileStore::open(tp.path()).unwrap();
        let mut coordinator = store.lock_coordinator().unwrap();
        let first = coordinator
            .try_acquire(
                key,
                loom_core::LockOwner {
                    principal: "root".into(),
                    session: "s1".into(),
                },
                loom_core::LockMode::Exclusive,
                100,
                0,
            )
            .unwrap();
        coordinator.apply_fence(key, first.fence).unwrap();
        store.save_lock_coordinator(&coordinator).unwrap();
    }
    let store = FileStore::open(tp.path()).unwrap();
    let mut coordinator = store.lock_coordinator().unwrap();
    let second = coordinator
        .try_acquire(
            key,
            loom_core::LockOwner {
                principal: "root".into(),
                session: "s2".into(),
            },
            loom_core::LockMode::Exclusive,
            100,
            0,
        )
        .unwrap();
    assert_eq!(second.fence, loom_core::Fence::embedded(2));
    coordinator.apply_fence(key, second.fence).unwrap();
    assert_eq!(
        coordinator
            .apply_fence(key, loom_core::Fence::embedded(1))
            .unwrap_err()
            .code,
        Code::FencingStale
    );
}

#[test]
fn identity_store_survives_reopen_without_sessions() {
    let tp = TempPath::new("identity-store");
    let root = loom_core::PrincipalId::from_bytes([1; 16]);
    let user = loom_core::PrincipalId::from_bytes([2; 16]);
    {
        let store = FileStore::open(tp.path()).unwrap();
        let mut identity = loom_core::IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        identity
            .add_principal(user, "alice", loom_core::PrincipalKind::User)
            .unwrap();
        identity.set_passphrase(user, "alice", b"abcdefgh").unwrap();
        identity
            .authenticate_passphrase(user, "alice", "session")
            .unwrap();
        store.save_identity_store(&identity).unwrap();
    }
    let store = FileStore::open(tp.path()).unwrap();
    let mut identity = store.identity_store().unwrap().unwrap();
    assert_eq!(identity.principals().count(), 2);
    assert_eq!(
        identity.session_principal("session").unwrap_err().code,
        Code::AuthenticationFailed
    );
    assert_eq!(
        identity
            .authenticate_passphrase(user, "alice", "new-session")
            .unwrap()
            .principal,
        user
    );
}

#[test]
fn preauthenticated_local_auth_binds_session_without_passphrase() {
    let tp = TempPath::new("preauthenticated-local-auth");
    let root = loom_core::PrincipalId::from_bytes([1; 16]);
    let user = loom_core::PrincipalId::from_bytes([2; 16]);
    {
        let store = FileStore::open(tp.path()).unwrap();
        let mut identity = loom_core::IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        identity
            .add_principal(user, "alice", loom_core::PrincipalKind::User)
            .unwrap();
        identity.set_passphrase(user, "alice", b"abcdefgh").unwrap();
        store.save_identity_store(&identity).unwrap();
    }

    let loom = open_loom_read_unlocked(tp.path(), None).unwrap();
    let loom = attach_local_auth(
        loom,
        &LocalOpenAuth {
            preauthenticated_principal: Some(user),
            session_id: Some("cached-dav".to_string()),
            ..LocalOpenAuth::default()
        },
    )
    .unwrap();
    assert_eq!(loom.effective_principal().unwrap(), Some(user));

    let loom = open_loom_read_unlocked(tp.path(), None).unwrap();
    let err = attach_local_auth(
        loom,
        &LocalOpenAuth {
            principal: Some(user),
            passphrase: Some("alice".to_string()),
            preauthenticated_principal: Some(user),
            session_id: Some("mixed".to_string()),
            ..LocalOpenAuth::default()
        },
    )
    .unwrap_err();
    assert_eq!(err.code, Code::InvalidArgument);
}

#[test]
fn acl_store_survives_reopen() {
    let tp = TempPath::new("acl-store");
    let principal = loom_core::PrincipalId::from_bytes([1; 16]);
    let ns = loom_core::WorkspaceId::from_bytes([9; 16]);
    {
        let store = FileStore::open(tp.path()).unwrap();
        let mut acl = loom_core::AclStore::new();
        acl.allow(
            loom_core::AclSubject::Principal(principal),
            Some(ns),
            Some(loom_core::FacetKind::Kv),
            [loom_core::AclRight::Read],
        )
        .unwrap();
        acl.deny(
            loom_core::AclSubject::Everyone,
            Some(ns),
            Some(loom_core::FacetKind::Kv),
            [loom_core::AclRight::Write],
        )
        .unwrap();
        store.save_acl_store(&acl).unwrap();
    }
    let store = FileStore::open(tp.path()).unwrap();
    let acl = store.acl_store().unwrap().unwrap();
    acl.authorize(
        true,
        principal,
        ns,
        loom_core::FacetKind::Kv,
        loom_core::AclRight::Read,
    )
    .unwrap();
    assert_eq!(
        acl.authorize(
            true,
            principal,
            ns,
            loom_core::FacetKind::Kv,
            loom_core::AclRight::Write,
        )
        .unwrap_err()
        .code,
        Code::PermissionDenied
    );
}

#[test]
fn audit_records_chain_and_survive_reopen() {
    let tp = TempPath::new("audit-records");
    let principal = WorkspaceId::from_bytes([6; 16]);
    {
        let store = FileStore::open(tp.path()).unwrap();
        assert_eq!(
            store
                .audit_append(Some(principal), "identity.create", Some("alice"))
                .unwrap(),
            0
        );
        assert_eq!(
            store.audit_append(None, "acl.grant", Some("kv")).unwrap(),
            1
        );
    }

    let store = FileStore::open(tp.path()).unwrap();
    let records = store.audit_records().unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].seq, 0);
    assert_eq!(records[0].principal, Some(principal));
    assert_eq!(records[0].action, "identity.create");
    assert_eq!(records[0].target.as_deref(), Some("alice"));
    assert_eq!(records[0].prev_hash, None);
    assert_eq!(records[1].seq, 1);
    assert_eq!(records[1].principal, None);
    assert_eq!(records[1].action, "acl.grant");
    assert_eq!(records[1].target.as_deref(), Some("kv"));
    assert_eq!(records[1].prev_hash, Some(records[0].hash));
}

#[test]
fn audit_records_reject_tampered_payloads() {
    let tp = TempPath::new("audit-tamper");
    let store = FileStore::open(tp.path()).unwrap();
    store
        .audit_append(
            Some(WorkspaceId::from_bytes([8; 16])),
            "identity.disable",
            Some("principal"),
        )
        .unwrap();
    let mut map = store.audit_retention_map().unwrap();
    let value = map.get_mut(&audit_entry_key(0)).unwrap();
    value[20] ^= 0x01;
    store
        .commit_family_root_records_for_test(
            AUDIT_RETENTION_FAMILY_ID,
            &audit_retention_family_records(&map),
        )
        .unwrap();

    assert_eq!(
        store.audit_records().unwrap_err().code,
        Code::IntegrityFailure
    );
}

#[test]
fn audit_config_defaults_and_survives_reopen() {
    let tp = TempPath::new("audit-config");
    let principal = WorkspaceId::from_bytes([9; 16]);
    {
        let store = FileStore::open(tp.path()).unwrap();
        assert_eq!(store.audit_config().unwrap(), AuditConfig::default());
        let config = AuditConfig {
            retention_days: 730,
            legal_hold: true,
        };
        assert_eq!(
            store
                .save_audit_config_audited(
                    config,
                    Some(principal),
                    "audit.config.set",
                    Some("retention_days=730;legal_hold=true"),
                )
                .unwrap(),
            0
        );
    }

    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(
        store.audit_config().unwrap(),
        AuditConfig {
            retention_days: 730,
            legal_hold: true,
        }
    );
    let records = store.audit_records().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].principal, Some(principal));
    assert_eq!(records[0].action, "audit.config.set");
}

#[test]
fn store_policy_defaults_and_survives_reopen() {
    let tp = TempPath::new("store-policy");
    let principal = WorkspaceId::from_bytes([10; 16]);
    {
        let store = FileStore::open(tp.path()).unwrap();
        assert_eq!(store.store_policy().unwrap(), StorePolicy::default());
        assert_eq!(
            store
                .save_store_policy_audited(
                    StorePolicy {
                        fips_required: true,
                        ..StorePolicy::default()
                    },
                    Some(principal),
                    "store.policy.set",
                    Some("fips_required=true"),
                )
                .unwrap(),
            0
        );
    }

    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(
        store.store_policy().unwrap(),
        StorePolicy {
            fips_required: true,
            ..StorePolicy::default()
        }
    );
    let records = store.audit_records().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].principal, Some(principal));
    assert_eq!(records[0].action, "store.policy.set");
}

#[test]
fn local_runtime_rejects_fips_required_store_when_not_fips_capable() {
    let tp = TempPath::new("store-policy-runtime");
    {
        let store = FileStore::create_with_profile(tp.path(), Algo::Sha256).unwrap();
        store
            .save_store_policy_audited(
                StorePolicy {
                    fips_required: true,
                    ..StorePolicy::default()
                },
                None,
                "store.policy.set",
                None,
            )
            .unwrap();
    }

    let result = open_loom_read_unlocked(tp.path(), None);
    if loom_core::runtime_profile().fips_capable {
        assert!(result.is_ok());
    } else {
        let err = result.unwrap_err();
        assert_eq!(err.code, Code::PermissionDenied);
        assert!(err.message.contains("FIPS-required"));
    }
}

#[test]
fn audit_legal_hold_blocks_prune() {
    let tp = TempPath::new("audit-legal-hold");
    let store = FileStore::open(tp.path()).unwrap();
    store
        .audit_append(None, "identity.create", Some("root"))
        .unwrap();
    store
        .save_audit_config_audited(
            AuditConfig {
                retention_days: 365,
                legal_hold: true,
            },
            None,
            "audit.config.set",
            Some("legal_hold=true"),
        )
        .unwrap();

    assert_eq!(
        store.audit_prune_through(None, 0).unwrap_err().code,
        Code::PermissionDenied
    );
    assert_eq!(store.audit_records().unwrap().len(), 2);
}

#[test]
fn audit_prune_keeps_checkpoint_and_chain_appendable() {
    let tp = TempPath::new("audit-prune");
    {
        let store = FileStore::open(tp.path()).unwrap();
        for i in 0..4 {
            store
                .audit_append(None, "acl.grant", Some(&format!("grant={i}")))
                .unwrap();
        }
        let stats = store.audit_prune_through(None, 1).unwrap();
        assert_eq!(stats.pruned, 2);
        assert_eq!(stats.checkpoint_seq, Some(1));
        assert!(stats.checkpoint_hash.is_some());
        assert_eq!(stats.audit_seq, 4);
        assert_eq!(
            store
                .audit_append(None, "daemon.start", Some("local"))
                .unwrap(),
            5
        );
    }

    let store = FileStore::open(tp.path()).unwrap();
    let records = store.audit_records().unwrap();
    assert_eq!(
        records
            .iter()
            .map(|record| (record.seq, record.action.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (2, "acl.grant"),
            (3, "acl.grant"),
            (4, "audit.prune"),
            (5, "daemon.start"),
        ]
    );
    assert_eq!(records[2].prev_hash, Some(records[1].hash));
}

#[test]
fn served_listener_config_persists_and_is_audited() {
    let tp = TempPath::new("served-listener");
    let principal = WorkspaceId::from_bytes([10; 16]);
    {
        let store = FileStore::open(tp.path()).unwrap();
        let record = FileStore::served_listener_record(
            "cas",
            vec!["main".to_string()],
            "rest",
            "127.0.0.1:8001",
            true,
        )
        .unwrap();
        let target = format!("id={};surface=cas", record.id);
        assert_eq!(
            store
                .save_served_listener_audited(
                    &record,
                    Some(principal),
                    "serve.listener.configure",
                    Some(&target),
                )
                .unwrap(),
            0
        );
    }

    let store = FileStore::open(tp.path()).unwrap();
    let listeners = store.served_listeners().unwrap();
    assert_eq!(listeners.len(), 1);
    assert_eq!(listeners[0].surface, "cas");
    assert_eq!(listeners[0].selectors, vec!["main"]);
    assert_eq!(listeners[0].transport, "rest");
    assert_eq!(listeners[0].profile, None);
    assert_eq!(listeners[0].bind, "127.0.0.1:8001");
    assert!(listeners[0].enabled);
    assert_eq!(listeners[0].schema_version, 3);
    assert_eq!(listeners[0].last_modified_audit_seq, Some(0));
    assert_eq!(listeners[0].tls.mode, "off");
    assert_eq!(listeners[0].auth.mode, "owner-or-passphrase");
    assert_eq!(listeners[0].route_scope, "workspace");
    assert_eq!(listeners[0].exposure, "read-write");
    assert_eq!(listeners[0].network_access_policy_ref, None);
    assert_eq!(
        listeners[0].limits,
        ServedListenerLimits {
            request_size_limit: 16 * 1024 * 1024,
            idle_timeout_ms: 60_000,
            session_timeout_ms: 3_600_000,
        }
    );
    let records = store.audit_records().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].principal, Some(principal));
    assert_eq!(records[0].action, "serve.listener.configure");
}

#[test]
fn served_listener_policy_profile_and_network_access_persist() {
    let mut record = FileStore::served_listener_record_with_profile(
        "vector",
        vec!["main".into(), "items".into()],
        "rest",
        Some("qdrant"),
        "127.0.0.1:8002",
        true,
    )
    .unwrap();
    record.tls.mode = "direct".to_string();
    record.tls.certificate_bundle_ref = Some("admin".to_string());
    record.auth.mode = "passphrase".to_string();
    record.exposure = "read-only".to_string();
    record.audit.mode = "all".to_string();
    record.network_access_policy_ref = Some("office".to_string());
    record.limits.request_size_limit = 1024;
    record.limits.idle_timeout_ms = 2500;
    record.limits.session_timeout_ms = 5000;
    record.last_modified_audit_seq = Some(9);

    let decoded = decode_served_listener(&encode_served_listener(&record)).unwrap();
    assert_eq!(decoded, record);
    assert_eq!(decoded.profile.as_deref(), Some("qdrant"));
    assert_eq!(decoded.network_access_policy_ref.as_deref(), Some("office"));
}

#[test]
fn served_listener_rejects_legacy_record_without_schema_version() {
    let legacy = legacy_served_listener_bytes("cas", &["main"], "rest", "127.0.0.1:8004", true);
    assert!(decode_served_listener(&legacy).is_err());
}

#[test]
fn network_access_policy_persists_is_audited_and_hashes() {
    let tp = TempPath::new("network-access-policy");
    let principal = WorkspaceId::from_bytes([13; 16]);
    let rule = NetworkAccessRule {
        id: "office-ip".to_string(),
        action: NetworkAccessAction::Allow,
        source_cidr: Some(NetworkAccessCidr::parse("203.0.113.0/24").unwrap()),
        trusted_proxy_cidr: None,
        require_mtls: false,
        client_cert_subject: None,
        client_cert_san: None,
        client_cert_issuer: None,
        description: Some("office egress".to_string()),
    };
    {
        let store = FileStore::open(tp.path()).unwrap();
        let policy = FileStore::network_access_policy_record(
            "office",
            Some("office network".to_string()),
            NetworkAccessAction::Deny,
            vec![rule.clone()],
        )
        .unwrap();
        let digest = store.network_access_policy_digest(&policy).unwrap();
        assert_eq!(digest.algo(), Algo::Blake3);
        let seq = store
            .save_network_access_policy_audited(
                &policy,
                Some(principal),
                "network-access.policy.set",
                Some("name=office"),
            )
            .unwrap();
        assert_eq!(seq, 0);
    }

    let store = FileStore::open(tp.path()).unwrap();
    let policies = store.network_access_policies().unwrap();
    assert_eq!(policies.len(), 1);
    assert_eq!(policies[0].name, "office");
    assert_eq!(policies[0].schema_version, 1);
    assert_eq!(policies[0].description.as_deref(), Some("office network"));
    assert_eq!(policies[0].default_action, NetworkAccessAction::Deny);
    assert_eq!(policies[0].rules, vec![rule]);
    assert_eq!(policies[0].created_audit_seq, Some(0));
    assert_eq!(policies[0].updated_audit_seq, Some(0));
    assert_eq!(
        store.audit_records().unwrap()[0].action,
        "network-access.policy.set"
    );
}

#[test]
fn network_access_policy_validation_rejects_noncanonical_cidr_and_duplicate_rules() {
    assert_eq!(
        NetworkAccessCidr::parse("203.0.113.9/24").unwrap_err().code,
        Code::InvalidArgument
    );
    assert!(NetworkAccessCidr::parse("203.0.113.9").is_ok());
    let rule = NetworkAccessRule {
        id: "dup".to_string(),
        action: NetworkAccessAction::Allow,
        source_cidr: Some(NetworkAccessCidr::parse("203.0.113.0/24").unwrap()),
        trusted_proxy_cidr: None,
        require_mtls: false,
        client_cert_subject: None,
        client_cert_san: None,
        client_cert_issuer: None,
        description: None,
    };
    assert_eq!(
        FileStore::network_access_policy_record(
            "office",
            None,
            NetworkAccessAction::Deny,
            vec![rule.clone(), rule]
        )
        .unwrap_err()
        .code,
        Code::InvalidArgument
    );
}

#[test]
fn network_access_policy_remove_requires_existing_record() {
    let tp = TempPath::new("network-access-remove");
    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(
        store
            .remove_network_access_policy_audited(
                "missing",
                None,
                "network-access.policy.remove",
                Some("name=missing")
            )
            .unwrap_err()
            .code,
        Code::NotFound
    );
    let policy = FileStore::network_access_policy_record(
        "empty",
        None,
        NetworkAccessAction::Deny,
        Vec::new(),
    )
    .unwrap();
    store
        .save_network_access_policy_audited(
            &policy,
            None,
            "network-access.policy.set",
            Some("name=empty"),
        )
        .unwrap();
    assert_eq!(
        store
            .remove_network_access_policy_audited(
                "empty",
                None,
                "network-access.policy.remove",
                Some("name=empty")
            )
            .unwrap(),
        1
    );
    assert!(store.network_access_policy("empty").unwrap().is_none());
}

#[test]
fn authority_replication_policy_persists_and_is_audited() {
    let tp = TempPath::new("authority-replication-policy");
    let principal = WorkspaceId::from_bytes([12; 16]);
    {
        let store = FileStore::open(tp.path()).unwrap();
        let mut policy =
            FileStore::authority_replication_policy("office", "/srv/policy.loom", true).unwrap();
        policy.interval_ms = Some(30_000);
        policy.jitter_ms = 1_000;
        policy.backoff_ms = 5_000;
        let target = format!("id={};source={}", policy.id, policy.source);
        assert_eq!(
            store
                .save_authority_replication_policy_audited(
                    &policy,
                    Some(principal),
                    "authority.replication.configure",
                    Some(&target),
                )
                .unwrap(),
            0
        );
    }

    let store = FileStore::open(tp.path()).unwrap();
    let policies = store.authority_replication_policies().unwrap();
    assert_eq!(policies.len(), 1);
    assert_eq!(policies[0].id, "office");
    assert_eq!(policies[0].source, "/srv/policy.loom");
    assert!(policies[0].enabled);
    assert!(policies[0].pull_on_start);
    assert_eq!(policies[0].interval_ms, Some(30_000));
    assert_eq!(policies[0].jitter_ms, 1_000);
    assert_eq!(policies[0].backoff_ms, 5_000);
    assert!(policies[0].publish_witness);
    assert_eq!(policies[0].last_modified_audit_seq, Some(0));
    assert!(store.audit_records().unwrap().iter().any(|record| {
        record.principal == Some(principal) && record.action == "authority.replication.configure"
    }));
}

#[test]
fn authority_replication_policy_rejects_invalid_and_removes() {
    let tp = TempPath::new("authority-replication-policy-remove");
    let store = FileStore::open(tp.path()).unwrap();
    assert!(FileStore::authority_replication_policy("bad/id", "/srv/a.loom", true).is_err());
    let mut policy =
        FileStore::authority_replication_policy("office", "/srv/policy.loom", true).unwrap();
    policy.interval_ms = Some(0);
    assert!(
        store
            .save_authority_replication_policy_audited(
                &policy,
                None,
                "authority.replication.configure",
                None,
            )
            .is_err()
    );

    policy.interval_ms = None;
    store
        .save_authority_replication_policy_audited(
            &policy,
            None,
            "authority.replication.configure",
            None,
        )
        .unwrap();
    assert!(
        store
            .authority_replication_policy_by_id("office")
            .unwrap()
            .is_some()
    );
    store
        .remove_authority_replication_policy_audited(
            "office",
            None,
            "authority.replication.remove",
            Some("id=office"),
        )
        .unwrap();
    assert!(
        store
            .authority_replication_policy_by_id("office")
            .unwrap()
            .is_none()
    );
}

#[test]
fn certificate_bundle_persists_and_is_audited_with_force_for_unencrypted_store() {
    let tp = TempPath::new("certificate-bundle");
    let principal = WorkspaceId::from_bytes([11; 16]);
    {
        let store = FileStore::open(tp.path()).unwrap();
        let record = store
            .certificate_bundle_record(
                "public-api",
                b"-----BEGIN CERTIFICATE-----\ncert\n-----END CERTIFICATE-----\n".to_vec(),
                b"-----BEGIN PRIVATE KEY-----\nkey\n-----END PRIVATE KEY-----\n".to_vec(),
                Some(b"-----BEGIN CERTIFICATE-----\nca\n-----END CERTIFICATE-----\n".to_vec()),
            )
            .unwrap();
        let err = store
            .save_certificate_bundle_audited(
                &record,
                Some(principal),
                "certificate.bundle.add",
                Some("name=public-api"),
                false,
            )
            .unwrap_err();
        assert_eq!(err.code, Code::PermissionDenied);
        assert!(err.message.contains("--force"));
        assert_eq!(
            store
                .save_certificate_bundle_audited(
                    &record,
                    Some(principal),
                    "certificate.bundle.add.force",
                    Some("name=public-api"),
                    true,
                )
                .unwrap(),
            0
        );
    }

    let store = FileStore::open(tp.path()).unwrap();
    let bundles = store.certificate_bundles().unwrap();
    assert_eq!(bundles.len(), 1);
    let bundle = &bundles[0];
    assert_eq!(bundle.name, "public-api");
    assert_eq!(bundle.schema_version, 1);
    assert_eq!(bundle.profile, "tls-server-direct");
    assert_eq!(bundle.created_audit_seq, Some(0));
    assert_eq!(bundle.updated_audit_seq, Some(0));
    assert!(bundle.unencrypted_private_key_override);
    assert_eq!(
        bundle.server_cert_chain_digest,
        Digest::hash(store.digest_algo(), &bundle.server_cert_chain_pem)
    );
    assert_eq!(
        bundle.private_key_digest,
        Digest::hash(store.digest_algo(), &bundle.private_key_pem)
    );
    assert_eq!(
        bundle.trust_bundle_digest,
        bundle
            .trust_bundle_pem
            .as_ref()
            .map(|bytes| Digest::hash(store.digest_algo(), bytes))
    );
    assert_eq!(
        store
            .certificate_bundle("public-api")
            .unwrap()
            .unwrap()
            .name,
        "public-api"
    );
    let records = store.audit_records().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].principal, Some(principal));
    assert_eq!(records[0].action, "certificate.bundle.add.force");
    assert_eq!(
        store
            .remove_certificate_bundle_audited(
                "public-api",
                Some(principal),
                "certificate.bundle.remove",
                Some("name=public-api"),
            )
            .unwrap(),
        1
    );
    assert!(store.certificate_bundles().unwrap().is_empty());
}

#[test]
fn encrypted_store_accepts_certificate_bundle_without_force() {
    let shared = SharedMem::default();
    let (meta_bytes, session) = test_encryption();
    let store = FileStore::with_backing_encrypted(
        Box::new(shared.clone()),
        meta_bytes,
        session,
        Algo::Blake3,
    )
    .unwrap();
    let record = store
        .certificate_bundle_record(
            "admin",
            b"cert-chain".to_vec(),
            b"private-key".to_vec(),
            None,
        )
        .unwrap();
    assert_eq!(
        store
            .save_certificate_bundle_audited(
                &record,
                None,
                "certificate.bundle.add",
                Some("name=admin"),
                false,
            )
            .unwrap(),
        0
    );
    let saved = store.certificate_bundle("admin").unwrap().unwrap();
    assert!(!saved.unencrypted_private_key_override);
    assert_eq!(saved.created_audit_seq, Some(0));
    assert_eq!(saved.updated_audit_seq, Some(0));
}

fn legacy_served_listener_bytes(
    surface: &str,
    selectors: &[&str],
    transport: &str,
    bind: &str,
    enabled: bool,
) -> Vec<u8> {
    let selectors = selectors
        .iter()
        .map(|selector| selector.to_string())
        .collect::<Vec<_>>();
    let id = served_listener_id_with_profile(surface, &selectors, transport, None, bind);
    let mut out = Vec::new();
    out.extend_from_slice(SERVED_LISTENER_MAGIC);
    put_lp(&mut out, id.as_bytes());
    put_lp(&mut out, surface.as_bytes());
    put_uvarint(&mut out, selectors.len() as u64);
    for selector in selectors {
        put_lp(&mut out, selector.as_bytes());
    }
    put_lp(&mut out, transport.as_bytes());
    put_lp(&mut out, bind.as_bytes());
    out.push(u8::from(enabled));
    out
}

#[test]
fn derived_artifact_survives_reopen_and_reports_stale() {
    let tp = TempPath::new("derived-artifact");
    let ns = loom_core::WorkspaceId::from_bytes([3; 16]);
    let key = DerivedArtifactKey::new(ns, loom_core::FacetKind::Search, "docs", "tantivy").unwrap();
    let source = loom_core::Digest::blake3(b"source-v1");
    let stamp = DerivedArtifactStamp::new(source, "tantivy-0", "search-v1").unwrap();
    {
        let store = FileStore::open(tp.path()).unwrap();
        let record = store
            .put_derived_artifact(&key, stamp.clone(), b"index bytes")
            .unwrap();
        assert_eq!(record.payload_len, 11);
    }

    let store = FileStore::open(tp.path()).unwrap();
    match store.read_derived_artifact(&key, &stamp).unwrap() {
        DerivedArtifactRead::Ready { record, payload } => {
            assert_eq!(record.stamp, stamp);
            assert_eq!(payload, b"index bytes");
        }
        other => panic!("expected ready artifact, got {other:?}"),
    }

    let stale_stamp = DerivedArtifactStamp::new(
        loom_core::Digest::blake3(b"source-v2"),
        "tantivy-0",
        "search-v1",
    )
    .unwrap();
    match store.read_derived_artifact(&key, &stale_stamp).unwrap() {
        DerivedArtifactRead::Stale { record } => assert_eq!(record.stamp, stamp),
        other => panic!("expected stale artifact, got {other:?}"),
    }
    assert!(store.delete_derived_artifact(&key).unwrap());
    assert_eq!(
        store.read_derived_artifact(&key, &stale_stamp).unwrap(),
        DerivedArtifactRead::Missing
    );
}

#[test]
fn compact_retaining_keeps_derived_artifact_payloads() {
    let tp = TempPath::new("derived-compact");
    let ns = loom_core::WorkspaceId::from_bytes([4; 16]);
    let key =
        DerivedArtifactKey::new(ns, loom_core::FacetKind::Vector, "embeddings", "hnsw").unwrap();
    let stamp = DerivedArtifactStamp::new(
        loom_core::Digest::blake3(b"vector-root"),
        "hnsw-0",
        "ann-v1",
    )
    .unwrap();
    {
        let store = FileStore::open(tp.path()).unwrap();
        store
            .put_derived_artifact(&key, stamp.clone(), b"native index payload")
            .unwrap();
    }

    {
        let mut store = FileStore::open(tp.path()).unwrap();
        store.compact_retaining(&BTreeSet::new()).unwrap();
    }

    let store = FileStore::open(tp.path()).unwrap();
    match store.read_derived_artifact(&key, &stamp).unwrap() {
        DerivedArtifactRead::Ready { payload, .. } => assert_eq!(payload, b"native index payload"),
        other => panic!("expected retained artifact, got {other:?}"),
    }
}

#[test]
fn derived_artifact_rebuild_lifecycle_coalesces_and_reports_status() {
    let tp = TempPath::new("derived-rebuild");
    let ns = loom_core::WorkspaceId::from_bytes([5; 16]);
    let store = FileStore::open(tp.path()).unwrap();
    let key =
        DerivedArtifactKey::new(ns, loom_core::FacetKind::Columnar, "events", "arrow").unwrap();
    let stamp = DerivedArtifactStamp::new(
        loom_core::Digest::blake3(b"columnar-root"),
        "arrow-writer-0",
        "arrow-cache-v1",
    )
    .unwrap();

    assert_eq!(
        store.derived_artifact_status(&key, &stamp).unwrap(),
        DerivedArtifactStatus::Missing
    );
    let run_id = match store
        .begin_derived_artifact_rebuild(&key, stamp.clone())
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected started rebuild, got {other:?}"),
    };
    assert_eq!(
        store
            .begin_derived_artifact_rebuild(&key, stamp.clone())
            .unwrap(),
        DerivedArtifactRebuild::Coalesced {
            run_id: run_id.clone()
        }
    );
    assert_eq!(
        store.derived_artifact_status(&key, &stamp).unwrap(),
        DerivedArtifactStatus::Rebuilding {
            run_id: run_id.clone(),
            stamp: stamp.clone()
        }
    );
    let record = store
        .finish_derived_artifact_rebuild(&key, &run_id, stamp.clone(), b"arrow bytes")
        .unwrap();
    assert_eq!(
        store.derived_artifact_status(&key, &stamp).unwrap(),
        DerivedArtifactStatus::Ready {
            record: record.clone()
        }
    );
    assert_eq!(
        store
            .begin_derived_artifact_rebuild(&key, stamp.clone())
            .unwrap(),
        DerivedArtifactRebuild::AlreadyReady { record }
    );

    let stale_stamp = DerivedArtifactStamp::new(
        loom_core::Digest::blake3(b"columnar-root-2"),
        "arrow-writer-0",
        "arrow-cache-v1",
    )
    .unwrap();
    let failed_run = match store
        .begin_derived_artifact_rebuild(&key, stale_stamp.clone())
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected started rebuild, got {other:?}"),
    };
    store
        .fail_derived_artifact_rebuild(
            &key,
            &failed_run,
            stale_stamp.clone(),
            "source changed during build",
        )
        .unwrap();
    assert_eq!(
        store.derived_artifact_status(&key, &stale_stamp).unwrap(),
        DerivedArtifactStatus::Failed {
            stamp: stale_stamp.clone(),
            message: "source changed during build".into()
        }
    );

    store
        .mark_derived_artifact_unsupported(&key, stale_stamp.clone(), "native engine unavailable")
        .unwrap();
    assert_eq!(
        store.derived_artifact_status(&key, &stale_stamp).unwrap(),
        DerivedArtifactStatus::Unsupported {
            stamp: stale_stamp,
            message: "native engine unavailable".into()
        }
    );
}

#[test]
fn derived_artifact_durability_defaults_to_relaxed_unless_retained() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let ns = loom_core::WorkspaceId::from_bytes([91; 16]);
    let search = DerivedArtifactKey::new(ns, FacetKind::Search, "docs", "tantivy").unwrap();
    let vector = vector_pq_artifact_key(ns, "emb").unwrap();
    let dataframe =
        dataframe_materialization_artifact_key(ns, "etl/purchases", "columnar").unwrap();
    let columnar = columnar_arrow_artifact_key(ns, "events").unwrap();

    assert_eq!(
        store.derived_artifact_durability(&search, None).unwrap(),
        StoreDurabilityPolicy::Relaxed
    );
    assert_eq!(
        store.derived_artifact_durability(&vector, None).unwrap(),
        StoreDurabilityPolicy::Relaxed
    );
    assert_eq!(
        store.derived_artifact_durability(&dataframe, None).unwrap(),
        StoreDurabilityPolicy::Relaxed
    );
    assert_eq!(
        store.derived_artifact_durability(&columnar, None).unwrap(),
        StoreDurabilityPolicy::Relaxed
    );
    assert_eq!(
        store
            .derived_artifact_durability(&dataframe, Some(StoreDurabilityPolicy::Normal))
            .unwrap(),
        StoreDurabilityPolicy::Normal
    );
}

#[test]
fn derived_artifact_facet_policy_can_retain_artifacts() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let ns = loom_core::WorkspaceId::from_bytes([92; 16]);
    let search = DerivedArtifactKey::new(ns, FacetKind::Search, "docs", "tantivy").unwrap();
    let vector = vector_hnsw_artifact_key(ns, "emb").unwrap();
    let mut policy = store.store_policy().unwrap();
    policy
        .set_default_durability(StoreDurabilityPolicy::Ephemeral)
        .unwrap();
    policy
        .set_facet_durability(FacetKind::Search, Some(StoreDurabilityPolicy::Strict))
        .unwrap();
    store
        .save_store_policy_audited(policy, None, "store.policy.set", None)
        .unwrap();

    assert_eq!(
        store.derived_artifact_durability(&search, None).unwrap(),
        StoreDurabilityPolicy::Strict
    );
    assert_eq!(
        store.derived_artifact_durability(&vector, None).unwrap(),
        StoreDurabilityPolicy::Relaxed
    );
    assert_eq!(
        store
            .derived_artifact_durability(&vector, Some(StoreDurabilityPolicy::Normal))
            .unwrap(),
        StoreDurabilityPolicy::Normal
    );
}

#[test]
fn derived_artifact_serving_policy_covers_all_non_ready_states() {
    use loom_core::capability::CapabilityOperationalState;

    let source = loom_core::Digest::blake3(b"derived-source");
    let stamp = DerivedArtifactStamp::new(source, "engine-1", "format-1").unwrap();
    let record = DerivedArtifactRecord {
        stamp: stamp.clone(),
        payload_digest: loom_core::Digest::blake3(b"payload"),
        payload_len: 7,
    };
    let cases = [
        (
            DerivedArtifactStatus::Ready {
                record: record.clone(),
            },
            "ready",
            DerivedArtifactServingMode::DerivedArtifact,
            CapabilityOperationalState::Supported,
            None,
            None,
        ),
        (
            DerivedArtifactStatus::Missing,
            "missing",
            DerivedArtifactServingMode::AuthoritativeSource,
            CapabilityOperationalState::Degraded,
            Some("derived_artifact_missing"),
            None,
        ),
        (
            DerivedArtifactStatus::Stale {
                record: record.clone(),
            },
            "stale",
            DerivedArtifactServingMode::AuthoritativeSource,
            CapabilityOperationalState::Degraded,
            Some("derived_artifact_stale"),
            None,
        ),
        (
            DerivedArtifactStatus::Rebuilding {
                run_id: "run-1".into(),
                stamp: stamp.clone(),
            },
            "rebuilding",
            DerivedArtifactServingMode::AuthoritativeSource,
            CapabilityOperationalState::Degraded,
            Some("index_rebuilding"),
            None,
        ),
        (
            DerivedArtifactStatus::Failed {
                stamp: stamp.clone(),
                message: "build failed".into(),
            },
            "failed",
            DerivedArtifactServingMode::AuthoritativeSource,
            CapabilityOperationalState::Degraded,
            Some("derived_artifact_failed"),
            None,
        ),
        (
            DerivedArtifactStatus::Unsupported {
                stamp,
                message: "engine unavailable".into(),
            },
            "unsupported",
            DerivedArtifactServingMode::AuthoritativeSource,
            CapabilityOperationalState::Unsupported,
            Some("profile_unsupported"),
            Some(loom_core::Code::Unsupported),
        ),
    ];

    for (status, name, mode, operational_state, reason_code, stable_error) in cases {
        assert_eq!(status.name(), name);
        let policy = status.serving_policy();
        assert_eq!(policy.mode, mode);
        assert_eq!(policy.operational_state, operational_state);
        assert_eq!(policy.reason_code, reason_code);
        assert_eq!(policy.stable_error, stable_error);
    }
}

#[test]
fn derived_artifact_serving_policy_projects_capability_state() {
    use loom_core::capability::{CapabilityDegradation, CapabilityOperationalState, CapabilitySet};

    let source = loom_core::Digest::blake3(b"derived-source");
    let stamp = DerivedArtifactStamp::new(source, "engine-1", "format-1").unwrap();
    let failed = DerivedArtifactStatus::Failed {
        stamp: stamp.clone(),
        message: "build failed".into(),
    }
    .apply_serving_policy_to_capabilities(CapabilitySet::registry(), "search");
    let failed_search = failed.get("search").unwrap();
    assert_eq!(
        failed_search.operational_state,
        CapabilityOperationalState::Degraded
    );
    assert_eq!(failed_search.reason_code, Some("derived_artifact_failed"));
    assert_eq!(failed_search.stable_error, None);
    assert_eq!(
        failed_search.degradation,
        Some(CapabilityDegradation {
            fallback: "authoritative-source",
            result_equivalence: "source-equivalent",
        })
    );

    let unsupported = DerivedArtifactStatus::Unsupported {
        stamp,
        message: "engine unavailable".into(),
    }
    .apply_serving_policy_to_capabilities(CapabilitySet::registry(), "search");
    let unsupported_search = unsupported.get("search").unwrap();
    assert_eq!(
        unsupported_search.operational_state,
        CapabilityOperationalState::Unsupported
    );
    assert_eq!(unsupported_search.reason_code, Some("profile_unsupported"));
    assert_eq!(
        unsupported_search.stable_error,
        Some(loom_core::Code::Unsupported)
    );
    assert_eq!(unsupported_search.degradation, None);
}

#[test]
fn columnar_arrow_lifecycle_uses_registered_derived_contract() {
    let tp = TempPath::new("columnar-arrow-derived");
    let ns = loom_core::WorkspaceId::from_bytes([8; 16]);
    let store = FileStore::open(tp.path()).unwrap();
    let source = loom_core::Digest::blake3(b"columnar-structured-root");
    let engine = "arrow-ipc-writer-test-0";
    let key = columnar_arrow_artifact_key(ns, "events").unwrap();
    let stamp = columnar_arrow_artifact_stamp(source, engine).unwrap();

    assert_eq!(key.facet, loom_core::FacetKind::Columnar);
    assert_eq!(key.artifact, COLUMNAR_ARROW_ARTIFACT);
    assert_eq!(stamp.format_version, COLUMNAR_ARROW_FORMAT_VERSION);
    assert_eq!(
        derived_artifact_format_version(loom_core::FacetKind::Columnar, COLUMNAR_ARROW_ARTIFACT),
        Some(COLUMNAR_ARROW_FORMAT_VERSION)
    );
    assert_eq!(
        store
            .columnar_arrow_status(ns, "events", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Missing
    );

    let run_id = match store
        .begin_columnar_arrow_rebuild(ns, "events", source, engine)
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected started rebuild, got {other:?}"),
    };
    assert_eq!(
        store
            .columnar_arrow_status(ns, "events", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Rebuilding {
            run_id: run_id.clone(),
            stamp: stamp.clone()
        }
    );
    let record = store
        .finish_columnar_arrow_rebuild(ns, "events", &run_id, source, engine, b"arrow-ipc")
        .unwrap();
    assert_eq!(
        store
            .columnar_arrow_status(ns, "events", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Ready {
            record: record.clone()
        }
    );

    let changed_source = loom_core::Digest::blake3(b"columnar-structured-root-v2");
    assert_eq!(
        store
            .columnar_arrow_status(ns, "events", changed_source, engine)
            .unwrap(),
        DerivedArtifactStatus::Stale { record }
    );
    let changed_run = match store
        .begin_columnar_arrow_rebuild(ns, "events", changed_source, engine)
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected started rebuild, got {other:?}"),
    };
    store
        .fail_columnar_arrow_rebuild(
            ns,
            "events",
            &changed_run,
            changed_source,
            engine,
            "arrow writer failed",
        )
        .unwrap();
    assert_eq!(
        store
            .columnar_arrow_status(ns, "events", changed_source, engine)
            .unwrap(),
        DerivedArtifactStatus::Failed {
            stamp: columnar_arrow_artifact_stamp(changed_source, engine).unwrap(),
            message: "arrow writer failed".into()
        }
    );
}

#[test]
fn graph_property_index_lifecycle_uses_registered_derived_contract() {
    let tp = TempPath::new("graph-property-index-derived");
    let ns = loom_core::WorkspaceId::from_bytes([10; 16]);
    let store = FileStore::open(tp.path()).unwrap();
    let source = loom_core::Digest::blake3(b"graph-root-plus-property-index-catalog");
    let engine = "graph-property-index-writer-test-0";
    let key = graph_property_index_artifact_key(ns, "people", "person_name").unwrap();
    let stamp = graph_property_index_artifact_stamp(source, engine).unwrap();

    assert_eq!(key.facet, loom_core::FacetKind::Graph);
    assert_eq!(key.collection, "people");
    assert_eq!(
        key.artifact,
        format!("{GRAPH_PROPERTY_INDEX_ARTIFACT_PREFIX}person_name")
    );
    assert_eq!(stamp.format_version, GRAPH_PROPERTY_INDEX_FORMAT_VERSION);
    assert_eq!(
        derived_artifact_format_version(loom_core::FacetKind::Graph, &key.artifact),
        Some(GRAPH_PROPERTY_INDEX_FORMAT_VERSION)
    );
    assert_eq!(
        store
            .graph_property_index_status(ns, "people", "person_name", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Missing
    );

    let run_id = match store
        .begin_graph_property_index_rebuild(ns, "people", "person_name", source, engine)
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected started rebuild, got {other:?}"),
    };
    assert_eq!(
        store
            .graph_property_index_status(ns, "people", "person_name", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Rebuilding {
            run_id: run_id.clone(),
            stamp: stamp.clone()
        }
    );
    let record = store
        .finish_graph_property_index_rebuild(
            ns,
            "people",
            "person_name",
            &run_id,
            source,
            engine,
            b"property-index-bytes",
        )
        .unwrap();
    assert_eq!(
        store
            .graph_property_index_status(ns, "people", "person_name", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Ready {
            record: record.clone()
        }
    );

    let changed_source = loom_core::Digest::blake3(b"graph-root-plus-property-index-catalog-v2");
    assert_eq!(
        store
            .graph_property_index_status(ns, "people", "person_name", changed_source, engine)
            .unwrap(),
        DerivedArtifactStatus::Stale { record }
    );
    let changed_run = match store
        .begin_graph_property_index_rebuild(ns, "people", "person_name", changed_source, engine)
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected started rebuild, got {other:?}"),
    };
    store
        .fail_graph_property_index_rebuild(
            ns,
            "people",
            "person_name",
            &changed_run,
            changed_source,
            engine,
            "property index writer failed",
        )
        .unwrap();
    assert_eq!(
        store
            .graph_property_index_status(ns, "people", "person_name", changed_source, engine)
            .unwrap(),
        DerivedArtifactStatus::Failed {
            stamp: graph_property_index_artifact_stamp(changed_source, engine).unwrap(),
            message: "property index writer failed".into()
        }
    );
}

#[test]
fn graph_spatial_index_lifecycle_reports_unsupported() {
    let tp = TempPath::new("graph-spatial-index-derived");
    let ns = loom_core::WorkspaceId::from_bytes([11; 16]);
    let store = FileStore::open(tp.path()).unwrap();
    let source = loom_core::Digest::blake3(b"graph-root-plus-spatial-index-catalog");
    let engine = "graph-spatial-index-writer-test-0";
    let key = graph_spatial_index_artifact_key(ns, "places", "place_loc").unwrap();
    let stamp = graph_spatial_index_artifact_stamp(source, engine).unwrap();

    assert_eq!(key.facet, loom_core::FacetKind::Graph);
    assert_eq!(key.collection, "places");
    assert_eq!(
        key.artifact,
        format!("{GRAPH_SPATIAL_INDEX_ARTIFACT_PREFIX}place_loc")
    );
    assert_eq!(stamp.format_version, GRAPH_SPATIAL_INDEX_FORMAT_VERSION);
    assert_eq!(
        derived_artifact_format_version(loom_core::FacetKind::Graph, &key.artifact),
        Some(GRAPH_SPATIAL_INDEX_FORMAT_VERSION)
    );
    store
        .mark_graph_spatial_index_unsupported(
            ns,
            "places",
            "place_loc",
            source,
            engine,
            "spatial profile unavailable",
        )
        .unwrap();
    assert_eq!(
        store
            .graph_spatial_index_status(ns, "places", "place_loc", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Unsupported {
            stamp,
            message: "spatial profile unavailable".into()
        }
    );
}

#[test]
fn dataframe_materialization_lifecycle_uses_registered_derived_contract() {
    let tp = TempPath::new("dataframe-materialization-derived");
    let ns = loom_core::WorkspaceId::from_bytes([14; 16]);
    let store = FileStore::open(tp.path()).unwrap();
    let source = loom_core::Digest::blake3(b"dataframe-plan-plus-source-digests");
    let engine = "portable-dataframe-executor-test-0";
    let key = dataframe_materialization_artifact_key(ns, "etl/purchases", "columnar").unwrap();
    let stamp = dataframe_materialization_artifact_stamp(source, engine).unwrap();

    assert_eq!(key.facet, loom_core::FacetKind::Dataframe);
    assert_eq!(key.collection, "etl/purchases");
    assert_eq!(
        key.artifact,
        format!("{DATAFRAME_MATERIALIZATION_ARTIFACT_PREFIX}columnar")
    );
    assert_eq!(
        stamp.format_version,
        DATAFRAME_MATERIALIZATION_FORMAT_VERSION
    );
    assert_eq!(
        derived_artifact_format_version(loom_core::FacetKind::Dataframe, &key.artifact),
        Some(DATAFRAME_MATERIALIZATION_FORMAT_VERSION)
    );
    assert_eq!(
        store
            .dataframe_materialization_status(ns, "etl/purchases", "columnar", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Missing
    );

    let run_id = match store
        .begin_dataframe_materialization_rebuild(ns, "etl/purchases", "columnar", source, engine)
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected started dataframe rebuild, got {other:?}"),
    };
    assert_eq!(
        store
            .dataframe_materialization_status(ns, "etl/purchases", "columnar", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Rebuilding {
            run_id: run_id.clone(),
            stamp: stamp.clone()
        }
    );
    let record = store
        .finish_dataframe_materialization_rebuild(
            ns,
            "etl/purchases",
            "columnar",
            &run_id,
            source,
            engine,
            b"dataframe-materialization-bytes",
        )
        .unwrap();
    assert_eq!(
        store
            .dataframe_materialization_status(ns, "etl/purchases", "columnar", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Ready {
            record: record.clone()
        }
    );

    let changed_source = loom_core::Digest::blake3(b"dataframe-plan-plus-source-digests-v2");
    assert_eq!(
        store
            .dataframe_materialization_status(
                ns,
                "etl/purchases",
                "columnar",
                changed_source,
                engine,
            )
            .unwrap(),
        DerivedArtifactStatus::Stale { record }
    );
    let changed_run = match store
        .begin_dataframe_materialization_rebuild(
            ns,
            "etl/purchases",
            "columnar",
            changed_source,
            engine,
        )
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected started dataframe rebuild, got {other:?}"),
    };
    store
        .fail_dataframe_materialization_rebuild(
            ns,
            "etl/purchases",
            "columnar",
            &changed_run,
            changed_source,
            engine,
            "dataframe materialization failed",
        )
        .unwrap();
    assert_eq!(
        store
            .dataframe_materialization_status(
                ns,
                "etl/purchases",
                "columnar",
                changed_source,
                engine,
            )
            .unwrap(),
        DerivedArtifactStatus::Failed {
            stamp: dataframe_materialization_artifact_stamp(changed_source, engine).unwrap(),
            message: "dataframe materialization failed".into()
        }
    );
    store
        .mark_dataframe_materialization_unsupported(
            ns,
            "etl/purchases",
            "parquet",
            changed_source,
            engine,
            "parquet profile unavailable",
        )
        .unwrap();
    assert_eq!(
        store
            .dataframe_materialization_status(
                ns,
                "etl/purchases",
                "parquet",
                changed_source,
                engine,
            )
            .unwrap(),
        DerivedArtifactStatus::Unsupported {
            stamp: dataframe_materialization_artifact_stamp(changed_source, engine).unwrap(),
            message: "parquet profile unavailable".into()
        }
    );
}

#[test]
fn pim_derived_indexes_use_registered_lifecycle_contracts() {
    let tp = TempPath::new("pim-derived-indexes");
    let ns = loom_core::WorkspaceId::from_bytes([15; 16]);
    let store = FileStore::open(tp.path()).unwrap();
    let calendar_source = loom_core::Digest::blake3(b"calendar-record-root-plus-index-profile");
    let contacts_source = loom_core::Digest::blake3(b"contacts-record-root-plus-index-profile");
    let mail_source = loom_core::Digest::blake3(b"mail-record-root-plus-index-profile");
    let engine = "pim-index-writer-test-0";

    let calendar_key =
        calendar_derived_index_artifact_key(ns, "alice", "work", "range-search").unwrap();
    let contacts_key =
        contacts_derived_index_artifact_key(ns, "alice", "people", "text-search").unwrap();
    let mail_key = mail_derived_index_artifact_key(ns, "alice", "inbox", "text-search").unwrap();

    assert_eq!(calendar_key.facet, loom_core::FacetKind::Calendar);
    assert_eq!(calendar_key.collection, "alice/work");
    assert_eq!(
        calendar_key.artifact,
        format!("{PIM_DERIVED_INDEX_ARTIFACT_PREFIX}range-search")
    );
    assert_eq!(contacts_key.facet, loom_core::FacetKind::Contacts);
    assert_eq!(contacts_key.collection, "alice/people");
    assert_eq!(
        contacts_key.artifact,
        format!("{PIM_DERIVED_INDEX_ARTIFACT_PREFIX}text-search")
    );
    assert_eq!(mail_key.facet, loom_core::FacetKind::Mail);
    assert_eq!(mail_key.collection, "alice/inbox");
    assert_eq!(
        mail_key.artifact,
        format!("{PIM_DERIVED_INDEX_ARTIFACT_PREFIX}text-search")
    );
    assert_eq!(
        derived_artifact_format_version(loom_core::FacetKind::Calendar, &calendar_key.artifact),
        Some(CALENDAR_DERIVED_INDEX_FORMAT_VERSION)
    );
    assert_eq!(
        derived_artifact_format_version(loom_core::FacetKind::Contacts, &contacts_key.artifact),
        Some(CONTACTS_DERIVED_INDEX_FORMAT_VERSION)
    );
    assert_eq!(
        derived_artifact_format_version(loom_core::FacetKind::Mail, &mail_key.artifact),
        Some(MAIL_DERIVED_INDEX_FORMAT_VERSION)
    );

    let calendar_stamp = calendar_derived_index_artifact_stamp(calendar_source, engine).unwrap();
    assert_eq!(
        calendar_stamp.format_version,
        CALENDAR_DERIVED_INDEX_FORMAT_VERSION
    );
    assert_eq!(
        store
            .derived_artifact_status(&calendar_key, &calendar_stamp)
            .unwrap(),
        DerivedArtifactStatus::Missing
    );
    let run_id = match store
        .begin_derived_artifact_rebuild(&calendar_key, calendar_stamp.clone())
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected calendar index rebuild, got {other:?}"),
    };
    let record = store
        .finish_derived_artifact_rebuild(
            &calendar_key,
            &run_id,
            calendar_stamp.clone(),
            b"calendar-index-bytes",
        )
        .unwrap();
    assert_eq!(
        store
            .derived_artifact_status(&calendar_key, &calendar_stamp)
            .unwrap(),
        DerivedArtifactStatus::Ready {
            record: record.clone()
        }
    );
    let changed_calendar = loom_core::Digest::blake3(b"calendar-record-root-plus-index-profile-v2");
    assert_eq!(
        store
            .derived_artifact_status(
                &calendar_key,
                &calendar_derived_index_artifact_stamp(changed_calendar, engine).unwrap(),
            )
            .unwrap(),
        DerivedArtifactStatus::Stale { record }
    );

    let contacts_stamp = contacts_derived_index_artifact_stamp(contacts_source, engine).unwrap();
    store
        .mark_derived_artifact_unsupported(
            &contacts_key,
            contacts_stamp.clone(),
            "contacts index profile unavailable",
        )
        .unwrap();
    assert_eq!(
        store
            .derived_artifact_status(&contacts_key, &contacts_stamp)
            .unwrap(),
        DerivedArtifactStatus::Unsupported {
            stamp: contacts_stamp,
            message: "contacts index profile unavailable".into()
        }
    );

    let mail_stamp = mail_derived_index_artifact_stamp(mail_source, engine).unwrap();
    let mail_run = match store
        .begin_derived_artifact_rebuild(&mail_key, mail_stamp.clone())
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected mail index rebuild, got {other:?}"),
    };
    store
        .fail_derived_artifact_rebuild(
            &mail_key,
            &mail_run,
            mail_stamp.clone(),
            "mail index writer failed",
        )
        .unwrap();
    assert_eq!(
        store
            .derived_artifact_status(&mail_key, &mail_stamp)
            .unwrap(),
        DerivedArtifactStatus::Failed {
            stamp: mail_stamp,
            message: "mail index writer failed".into()
        }
    );
}

#[test]
fn vector_pq_lifecycle_uses_vector_source_stamp_and_serving_policy() {
    let tp = TempPath::new("vector-pq-derived");
    let store = FileStore::open(tp.path()).unwrap();
    let mut loom = loom_core::Loom::new(store);
    let ns = loom
        .registry_mut()
        .create(
            loom_core::FacetKind::Vector,
            Some("vector-pq-derived"),
            loom_core::WorkspaceId::from_bytes([12; 16]),
        )
        .unwrap();
    loom_core::vector_create(&mut loom, ns, "emb", 2, loom_core::Metric::Dot).unwrap();
    loom_core::vector_upsert(&mut loom, ns, "emb", "a", vec![1.0, 0.0], BTreeMap::new()).unwrap();
    let source = loom_core::vector_source_digest(&loom, ns, "emb").unwrap();
    let engine = "pq-writer-0";
    let key = vector_pq_artifact_key(ns, "emb").unwrap();
    let stamp = vector_pq_artifact_stamp(source, engine).unwrap();

    assert_eq!(key.facet, loom_core::FacetKind::Vector);
    assert_eq!(key.collection, "emb");
    assert_eq!(key.artifact, VECTOR_PQ_ARTIFACT);
    assert_eq!(stamp.format_version, VECTOR_PQ_FORMAT_VERSION);
    assert_eq!(
        derived_artifact_format_version(loom_core::FacetKind::Vector, VECTOR_PQ_ARTIFACT),
        Some(VECTOR_PQ_FORMAT_VERSION)
    );
    assert_eq!(
        loom.store()
            .vector_pq_status(ns, "emb", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Missing
    );

    let run_id = match loom
        .store()
        .begin_vector_pq_rebuild(ns, "emb", source, engine)
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected started PQ rebuild, got {other:?}"),
    };
    assert_eq!(
        loom.store()
            .vector_pq_status(ns, "emb", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Rebuilding {
            run_id: run_id.clone(),
            stamp: stamp.clone()
        }
    );
    assert_eq!(
        loom.store()
            .vector_pq_status(ns, "emb", source, engine)
            .unwrap()
            .serving_policy()
            .reason_code,
        Some("index_rebuilding")
    );
    let record = loom
        .store()
        .finish_vector_pq_rebuild(ns, "emb", &run_id, source, engine, b"pq bytes")
        .unwrap();
    assert_eq!(
        loom.store()
            .vector_pq_status(ns, "emb", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Ready {
            record: record.clone()
        }
    );

    loom_core::vector_upsert(&mut loom, ns, "emb", "b", vec![0.0, 1.0], BTreeMap::new()).unwrap();
    let changed_source = loom_core::vector_source_digest(&loom, ns, "emb").unwrap();
    assert_eq!(
        loom.store()
            .vector_pq_status(ns, "emb", changed_source, engine)
            .unwrap(),
        DerivedArtifactStatus::Stale { record }
    );
}

#[test]
fn vector_hnsw_lifecycle_reports_failed_and_unsupported_policy() {
    let tp = TempPath::new("vector-hnsw-derived");
    let ns = loom_core::WorkspaceId::from_bytes([13; 16]);
    let store = FileStore::open(tp.path()).unwrap();
    let source = loom_core::Digest::blake3(b"vector-source-hnsw");
    let engine = "hnsw-writer-0";
    let key = vector_hnsw_artifact_key(ns, "emb").unwrap();
    let stamp = vector_hnsw_artifact_stamp(source, engine).unwrap();

    assert_eq!(key.facet, loom_core::FacetKind::Vector);
    assert_eq!(key.collection, "emb");
    assert_eq!(key.artifact, VECTOR_HNSW_ARTIFACT);
    assert_eq!(stamp.format_version, VECTOR_HNSW_FORMAT_VERSION);
    assert_eq!(
        derived_artifact_format_version(loom_core::FacetKind::Vector, VECTOR_HNSW_ARTIFACT),
        Some(VECTOR_HNSW_FORMAT_VERSION)
    );

    let run_id = match store
        .begin_vector_hnsw_rebuild(ns, "emb", source, engine)
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected started HNSW rebuild, got {other:?}"),
    };
    store
        .fail_vector_hnsw_rebuild(ns, "emb", &run_id, source, engine, "hnsw writer failed")
        .unwrap();
    let failed = store.vector_hnsw_status(ns, "emb", source, engine).unwrap();
    assert_eq!(
        failed,
        DerivedArtifactStatus::Failed {
            stamp: stamp.clone(),
            message: "hnsw writer failed".into()
        }
    );
    assert_eq!(
        failed.serving_policy().reason_code,
        Some("derived_artifact_failed")
    );

    store
        .mark_vector_hnsw_unsupported(ns, "emb", source, engine, "native hnsw unavailable")
        .unwrap();
    let unsupported = store.vector_hnsw_status(ns, "emb", source, engine).unwrap();
    assert_eq!(
        unsupported,
        DerivedArtifactStatus::Unsupported {
            stamp,
            message: "native hnsw unavailable".into()
        }
    );
    assert_eq!(
        unsupported.serving_policy().stable_error,
        Some(loom_core::Code::Unsupported)
    );
}

#[test]
fn search_tantivy_lifecycle_uses_search_artifact_contract() {
    let tp = TempPath::new("search-tantivy-derived");
    let ns = loom_core::WorkspaceId::from_bytes([6; 16]);
    let store = FileStore::open(tp.path()).unwrap();
    let source = loom_core::Digest::blake3(b"search-root");
    let engine = "tantivy-test-0";

    assert_eq!(
        search_tantivy_artifact_key(ns, "docs").unwrap(),
        DerivedArtifactKey::new(ns, loom_core::FacetKind::Search, "docs", "tantivy").unwrap()
    );
    assert_eq!(
        search_tantivy_artifact_stamp(source, engine).unwrap(),
        DerivedArtifactStamp::new(source, engine, "search-tantivy-v1").unwrap()
    );
    assert_eq!(
        store
            .search_tantivy_status(ns, "docs", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Missing
    );

    let run_id = match store
        .begin_search_tantivy_rebuild(ns, "docs", source, engine)
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected started search rebuild, got {other:?}"),
    };
    assert_eq!(
        store
            .begin_search_tantivy_rebuild(ns, "docs", source, engine)
            .unwrap(),
        DerivedArtifactRebuild::Coalesced {
            run_id: run_id.clone()
        }
    );
    assert_eq!(
        store
            .search_tantivy_status(ns, "docs", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Rebuilding {
            run_id: run_id.clone(),
            stamp: search_tantivy_artifact_stamp(source, engine).unwrap()
        }
    );

    let record = store
        .finish_search_tantivy_rebuild(ns, "docs", &run_id, source, engine, b"tantivy bytes")
        .unwrap();
    assert_eq!(
        store
            .search_tantivy_status(ns, "docs", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Ready {
            record: record.clone()
        }
    );

    let next_source = loom_core::Digest::blake3(b"search-root-2");
    assert_eq!(
        store
            .search_tantivy_status(ns, "docs", next_source, engine)
            .unwrap(),
        DerivedArtifactStatus::Stale {
            record: record.clone()
        }
    );
    let failed_run = match store
        .begin_search_tantivy_rebuild(ns, "docs", next_source, engine)
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected started stale-source rebuild, got {other:?}"),
    };
    store
        .fail_search_tantivy_rebuild(
            ns,
            "docs",
            &failed_run,
            next_source,
            engine,
            "source changed during search index build",
        )
        .unwrap();
    assert_eq!(
        store
            .search_tantivy_status(ns, "docs", next_source, engine)
            .unwrap(),
        DerivedArtifactStatus::Failed {
            stamp: search_tantivy_artifact_stamp(next_source, engine).unwrap(),
            message: "source changed during search index build".into()
        }
    );

    store
        .mark_search_tantivy_unsupported(ns, "docs", next_source, engine, "tantivy unavailable")
        .unwrap();
    assert_eq!(
        store
            .search_tantivy_status(ns, "docs", next_source, engine)
            .unwrap(),
        DerivedArtifactStatus::Unsupported {
            stamp: search_tantivy_artifact_stamp(next_source, engine).unwrap(),
            message: "tantivy unavailable".into()
        }
    );
}

#[test]
fn search_status_result_round_trips_every_variant() {
    let source = loom_core::Digest::blake3(b"search-root");
    let stamp = DerivedArtifactStamp::new(source, "tantivy-1", "search-tantivy-v1").unwrap();
    let record = DerivedArtifactRecord {
        stamp: stamp.clone(),
        payload_digest: loom_core::Digest::blake3(b"payload"),
        payload_len: 42,
    };
    for status in [
        DerivedArtifactStatus::Missing,
        DerivedArtifactStatus::Stale {
            record: record.clone(),
        },
        DerivedArtifactStatus::Ready {
            record: record.clone(),
        },
        DerivedArtifactStatus::Rebuilding {
            run_id: "run-1".into(),
            stamp: stamp.clone(),
        },
        DerivedArtifactStatus::Failed {
            stamp: stamp.clone(),
            message: "boom".into(),
        },
        DerivedArtifactStatus::Unsupported {
            stamp: stamp.clone(),
            message: "tantivy unavailable".into(),
        },
    ] {
        let bytes = encode_search_status_result(&source, &status).unwrap();
        let (got_source, got_status) = decode_search_status_result(&bytes).unwrap();
        assert_eq!(got_source, source);
        assert_eq!(got_status, status);
    }
    // A corrupt/short payload is rejected, not silently misparsed.
    assert!(decode_search_status_result(b"nope").is_err());
}

#[test]
fn search_embedding_lifecycle_uses_entity_projection_contract() {
    let tp = TempPath::new("search-embedding-derived");
    let ns = loom_core::WorkspaceId::from_bytes([26; 16]);
    let store = FileStore::open(tp.path()).unwrap();
    let source = loom_core::Digest::blake3(b"doc body");
    let projection = SearchEmbeddingProjection {
        workspace: ns,
        collection: "docs",
        entity_id: "doc-1",
        content_digest: source,
        model_id: "embed-small",
        model_weights_digest: Some("weights-a"),
        engine_version: "semantic-v1",
    };

    assert_eq!(
        search_embedding_artifact_key(ns, "docs", "doc-1").unwrap(),
        DerivedArtifactKey::new(ns, loom_core::FacetKind::Search, "docs", "embedding:doc-1")
            .unwrap()
    );
    assert_eq!(
        search_embedding_artifact_stamp(source, "embed-small", Some("weights-a"), "semantic-v1")
            .unwrap(),
        DerivedArtifactStamp::new(
            source,
            "11:embed-small|9:weights-a|11:semantic-v1",
            "search-embedding-v1"
        )
        .unwrap()
    );
    assert_eq!(
        store.search_embedding_status(projection).unwrap(),
        DerivedArtifactStatus::Missing
    );

    let run_id = match store.begin_search_embedding_rebuild(projection).unwrap() {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected started embedding rebuild, got {other:?}"),
    };
    assert_eq!(
        store.begin_search_embedding_rebuild(projection).unwrap(),
        DerivedArtifactRebuild::Coalesced {
            run_id: run_id.clone()
        }
    );
    assert_eq!(
        store.search_embedding_status(projection).unwrap(),
        DerivedArtifactStatus::Rebuilding {
            run_id: run_id.clone(),
            stamp: search_embedding_artifact_stamp(
                source,
                "embed-small",
                Some("weights-a"),
                "semantic-v1"
            )
            .unwrap()
        }
    );

    let record = store
        .finish_search_embedding_rebuild(projection, &run_id, b"vector bytes")
        .unwrap();
    assert_eq!(
        store.search_embedding_status(projection).unwrap(),
        DerivedArtifactStatus::Ready {
            record: record.clone()
        }
    );

    let changed_content = SearchEmbeddingProjection {
        content_digest: loom_core::Digest::blake3(b"doc body changed"),
        ..projection
    };
    assert_eq!(
        store.search_embedding_status(changed_content).unwrap(),
        DerivedArtifactStatus::Stale { record }
    );

    let blind_projection = SearchEmbeddingProjection {
        entity_id: "doc-2",
        content_digest: loom_core::Digest::blake3(b"opaque doc"),
        ..projection
    };
    store
        .mark_search_embedding_no_keys(blind_projection, "plaintext unavailable")
        .unwrap();
    assert_eq!(
        store.search_embedding_status(blind_projection).unwrap(),
        DerivedArtifactStatus::Unsupported {
            stamp: search_embedding_artifact_stamp(
                blind_projection.content_digest,
                "embed-small",
                Some("weights-a"),
                "semantic-v1"
            )
            .unwrap(),
            message: "plaintext unavailable".into()
        }
    );
}

#[test]
fn facet_source_digests_change_when_sources_change() {
    let tp = TempPath::new("derived-source-digests");
    let store = FileStore::open(tp.path()).unwrap();
    let mut loom = loom_core::Loom::new(store);
    let vector_ns = loom
        .registry_mut()
        .create(
            loom_core::FacetKind::Vector,
            Some("vector-digest"),
            loom_core::WorkspaceId::from_bytes([21; 16]),
        )
        .unwrap();
    loom_core::vector_create(&mut loom, vector_ns, "emb", 2, loom_core::Metric::Dot).unwrap();
    loom_core::vector_upsert(
        &mut loom,
        vector_ns,
        "emb",
        "a",
        vec![1.0, 0.0],
        BTreeMap::new(),
    )
    .unwrap();
    let vector_before = loom_core::vector_source_digest(&loom, vector_ns, "emb").unwrap();
    loom_core::vector_upsert(
        &mut loom,
        vector_ns,
        "emb",
        "b",
        vec![0.0, 1.0],
        BTreeMap::new(),
    )
    .unwrap();
    let vector_after = loom_core::vector_source_digest(&loom, vector_ns, "emb").unwrap();
    assert_ne!(vector_before, vector_after);
    let vector_tip = loom.commit(vector_ns, "test", "vector source", 1).unwrap();
    let vector_key = vector_pq_artifact_key(vector_ns, "emb").unwrap();
    let vector_stamp = vector_pq_artifact_stamp(vector_after, "pq-writer-0").unwrap();
    loom.store()
        .put_derived_artifact(&vector_key, vector_stamp.clone(), b"pq bytes")
        .unwrap();
    assert!(matches!(
        loom.store()
            .derived_artifact_status(&vector_key, &vector_stamp)
            .unwrap(),
        DerivedArtifactStatus::Ready { .. }
    ));

    let dst_path = TempPath::new("derived-clone-dst");
    let dst_store = FileStore::open(dst_path.path()).unwrap();
    let mut dst = loom_core::Loom::new(dst_store);
    let (dst_ns, _) = loom_core::clone_workspace(
        &loom,
        vector_ns,
        &mut dst,
        loom_core::WorkspaceId::from_bytes([24; 16]),
    )
    .unwrap();
    dst.checkout_commit(dst_ns, vector_tip).unwrap();
    let dst_key = vector_pq_artifact_key(dst_ns, "emb").unwrap();
    let dst_stamp = vector_pq_artifact_stamp(
        loom_core::vector_source_digest(&dst, dst_ns, "emb").unwrap(),
        "pq-writer-0",
    )
    .unwrap();
    assert_eq!(
        dst.store()
            .derived_artifact_status(&dst_key, &dst_stamp)
            .unwrap(),
        DerivedArtifactStatus::Missing
    );

    let search_ns = loom
        .registry_mut()
        .create(
            loom_core::FacetKind::Search,
            Some("search-digest"),
            loom_core::WorkspaceId::from_bytes([22; 16]),
        )
        .unwrap();
    let mut mapping = loom_core::Mapping::new();
    mapping.insert("title".into(), loom_core::FieldMapping::text());
    loom_core::search_create(&mut loom, search_ns, "docs", mapping).unwrap();
    let search_before = loom_core::search_source_digest(&loom, search_ns, "docs").unwrap();
    let mut doc = loom_core::Document::new();
    doc.insert("title".into(), loom_core::FieldValue::Text("first".into()));
    loom_core::search_index(&mut loom, search_ns, "docs", b"a".to_vec(), doc).unwrap();
    let search_after = loom_core::search_source_digest(&loom, search_ns, "docs").unwrap();
    assert_ne!(search_before, search_after);

    let columnar_ns = loom
        .registry_mut()
        .create(
            loom_core::FacetKind::Columnar,
            Some("columnar-digest"),
            loom_core::WorkspaceId::from_bytes([23; 16]),
        )
        .unwrap();
    loom_core::columnar_create(
        &mut loom,
        columnar_ns,
        "events",
        vec![("id".into(), loom_core::ColumnType::Int)],
        4,
    )
    .unwrap();
    let columnar_before = loom_core::columnar_source_digest(&loom, columnar_ns, "events").unwrap();
    loom_core::columnar_append(
        &mut loom,
        columnar_ns,
        "events",
        vec![loom_core::Value::Int(1)],
    )
    .unwrap();
    let columnar_after = loom_core::columnar_source_digest(&loom, columnar_ns, "events").unwrap();
    assert_ne!(columnar_before, columnar_after);
}

#[test]
fn objects_survive_reopen() {
    let tp = TempPath::new("reopen");
    let (d1, d2);
    {
        let store = FileStore::open(tp.path()).unwrap();
        d1 = store.put(&blob(b"alpha")).unwrap();
        d2 = store.put(&blob(b"beta")).unwrap();
        assert_eq!(store.len(), 2);
    } // drop -> file closed
    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(store.len(), 2);
    assert_eq!(
        store.get(&d1).unwrap().as_deref(),
        Some(blob(b"alpha").as_slice())
    );
    assert_eq!(
        store.get(&d2).unwrap().as_deref(),
        Some(blob(b"beta").as_slice())
    );
}

/// The full engine works over a FIPS (sha256) identity profile: a files workspace
/// commits, persists, reopens, and reads back, and every stored object is addressed under
/// SHA-256 - proving content addressing, the content map, commits, and verification are all coherent
/// under the store profile (not hard-coded BLAKE3).
#[test]
fn full_loom_over_fips_profile_round_trips() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{ObjectStore, WsSelector};

    let tp = TempPath::new("fips-loom");
    let ns_id = WorkspaceId::from_bytes([9; 16]);
    // Create the store under the FIPS profile, then drive the engine over it via open_loom (which
    // reopens and reads the sha256 profile from the superblock).
    FileStore::create_with_profile(tp.path(), Algo::Sha256).unwrap();
    {
        let mut loom = open_loom(tp.path()).unwrap();
        assert_eq!(loom.store().digest_algo(), Algo::Sha256);
        let ns = loom
            .registry_mut()
            .create(FacetKind::Files, Some("proj"), ns_id)
            .unwrap();
        loom.write_file(ns, "README.md", b"# fips hello", 0o100644)
            .unwrap();
        loom.create_directory(ns, "src", false).unwrap();
        loom.write_file(ns, "src/main.rs", b"fn main() {}", 0o100644)
            .unwrap();
        loom.commit(ns, "nas", "init", 1).unwrap();
        save_loom(&mut loom).unwrap();
        // Every object stored is addressed with SHA-256.
        assert!(loom.store().len() > 0);
    }
    // Reopen: the profile is still sha256, and the committed files read back through the engine.
    let loom = open_loom(tp.path()).unwrap();
    assert_eq!(loom.store().digest_algo(), Algo::Sha256);
    let ns = loom
        .registry()
        .open(&WsSelector::Typed {
            ty: FacetKind::Files,
            name: "proj".to_string(),
        })
        .unwrap();
    assert_eq!(loom.read_file(ns, "README.md").unwrap(), b"# fips hello");
    assert_eq!(loom.read_file(ns, "src/main.rs").unwrap(), b"fn main() {}");
}

#[test]
fn full_loom_survives_restart() {
    use loom_core::WsSelector;
    use loom_core::workspace::{DEFAULT_BRANCH, FacetKind, WorkspaceId};

    let tp = TempPath::new("full-loom");
    let ns_id = WorkspaceId::from_bytes([7; 16]);
    let tip;
    {
        // Build a real engine: a files workspace, a commit, a second branch, and a tag.
        let mut loom = open_loom(tp.path()).unwrap();
        let ns = loom
            .registry_mut()
            .create(FacetKind::Files, Some("proj"), ns_id)
            .unwrap();
        loom.write_file(ns, "README.md", b"# hello", 0o100644)
            .unwrap();
        loom.create_directory(ns, "src", false).unwrap();
        loom.write_file(ns, "src/main.rs", b"fn main() {}", 0o100644)
            .unwrap();
        let c0 = loom.commit(ns, "nas", "init", 1).unwrap();
        loom.branch(ns, "feature").unwrap();
        loom.registry_mut().tag_create(ns, "v1", c0).unwrap();
        tip = c0;
        save_loom(&mut loom).unwrap();
    } // drop -> file closed

    // Reopen from disk: registry (refs/tags/HEAD), content map, and working tree must all return.
    let loom = open_loom(tp.path()).unwrap();
    let ns = loom
        .registry()
        .open(&WsSelector::Typed {
            ty: FacetKind::Files,
            name: "proj".to_string(),
        })
        .unwrap();
    assert_eq!(ns, ns_id);
    assert_eq!(loom.registry().head_branch(ns).unwrap(), DEFAULT_BRANCH);
    assert_eq!(
        loom.registry().branch_tip(ns, DEFAULT_BRANCH).unwrap(),
        Some(tip)
    );
    assert_eq!(
        loom.registry().branch_tip(ns, "feature").unwrap(),
        Some(tip)
    );
    assert_eq!(loom.registry().tag_target(ns, "v1").unwrap(), Some(tip));
    // Working tree was rebuilt by checking out HEAD on open.
    assert_eq!(loom.read_file(ns, "README.md").unwrap(), b"# hello");
    assert_eq!(loom.read_file(ns, "src/main.rs").unwrap(), b"fn main() {}");
}

#[test]
fn optional_runtime_config_survives_full_loom_restart_without_activation() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{
        OptionalRuntimeConfig, OptionalRuntimeKind, activate_optional_runtime,
        get_optional_runtime_config, set_optional_runtime_config,
    };

    let tp = TempPath::new("optional-runtime-config-restart");
    let ns_id = WorkspaceId::from_bytes([41; 16]);
    let mut settings = BTreeMap::new();
    settings.insert("gateway".to_string(), "https://example.test".to_string());
    let config = OptionalRuntimeConfig::new(OptionalRuntimeKind::Ipfs, true, settings).unwrap();
    {
        let mut loom = open_loom(tp.path()).unwrap();
        let ns = loom
            .registry_mut()
            .create(FacetKind::Files, Some("proj"), ns_id)
            .unwrap();
        set_optional_runtime_config(&mut loom, ns, &config).unwrap();
        loom.commit(ns, "nas", "optional runtime config", 1)
            .unwrap();
        save_loom(&mut loom).unwrap();
    }

    let loom = open_loom(tp.path()).unwrap();
    assert_eq!(
        get_optional_runtime_config(&loom, ns_id, OptionalRuntimeKind::Ipfs).unwrap(),
        Some(config)
    );
    assert_eq!(
        activate_optional_runtime(&loom, ns_id, OptionalRuntimeKind::Ipfs)
            .unwrap_err()
            .code,
        loom_core::Code::Unsupported
    );
}

#[test]
fn set_reference_root_survives_reopen_and_clears() {
    let tp = TempPath::new("reference");
    let root = Digest::blake3(b"engine-state-root");
    {
        let store = FileStore::open(tp.path()).unwrap();
        assert_eq!(store.reference_root(), None);
        store.put(&blob(b"obj")).unwrap(); // a data commit before setting the root
        store.set_reference_root(Some(root)).unwrap();
        assert_eq!(store.reference_root(), Some(root));
    }
    // Reopen: the committed root and the object both survive.
    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(store.reference_root(), Some(root));
    assert_eq!(store.len(), 1);
    // A further object commit must preserve the existing root (it rides through `put`).
    store.put(&blob(b"obj2")).unwrap();
    assert_eq!(store.reference_root(), Some(root));
    // Clearing the root persists too.
    store.set_reference_root(None).unwrap();
    drop(store);
    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(store.reference_root(), None);
    assert_eq!(store.len(), 2);
}

// Build a file with `n` committed puts, returning the raw bytes after the final commit and the
// committed digests in order.
fn build_committed(n: usize) -> (Vec<u8>, Vec<Digest>) {
    let tp = TempPath::new("crash-src");
    let mut digests = Vec::new();
    {
        let store = FileStore::open(tp.path()).unwrap();
        for i in 0..n {
            digests.push(store.put(&blob(format!("obj-{i}").as_bytes())).unwrap());
        }
    }
    (std::fs::read(tp.path()).unwrap(), digests)
}

fn open_bytes(bytes: &[u8], tag: &str) -> Result<FileStore> {
    let tp = TempPath::new(tag);
    std::fs::write(tp.path(), bytes).unwrap();
    let r = FileStore::open(tp.path());
    // keep tp alive until after open
    drop(tp);
    r
}

// Like `open_bytes` but read-only, so recovery does not checkpoint-on-open - the on-disk
// superblock stays as written, exercising pure ring recovery with a lagging checkpoint.
fn open_read_bytes(bytes: &[u8], tag: &str) -> Result<FileStore> {
    let tp = TempPath::new(tag);
    std::fs::write(tp.path(), bytes).unwrap();
    let r = FileStore::open_read(tp.path());
    drop(tp);
    r
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct T188CanonicalRoots {
    generation: u64,
    page_count: u64,
    region_table_root: Option<PageId>,
    index_root: Option<PageId>,
    freemap_root: Option<PageId>,
    maintenance_root: Option<PageId>,
    overlay_root: Option<PageId>,
    current_record_root: Option<PageId>,
    root_catalog_root: Option<PageId>,
    retained_history_root: Option<PageId>,
    audit_retention_root: Option<PageId>,
    owner_token_root: Option<PageId>,
    secondary_index_root: Option<PageId>,
    mutable_idempotency_root: Option<PageId>,
    workflow_idempotency_root: Option<PageId>,
    mvcc_generation_root: Option<PageId>,
    retention_index_root: Option<PageId>,
    checkpoint_index_root: Option<PageId>,
    reclaim_index_root: Option<PageId>,
    delta_pack_candidate_root: Option<PageId>,
    reference_root: Option<Digest>,
    control_root: Option<Digest>,
}

fn t188_15_roots(store: &FileStore) -> T188CanonicalRoots {
    let inner = store.inner.lock().unwrap();
    let delta_pack_candidate_root = inner
        .root_catalog_entries
        .iter()
        .find(|entry| entry.family_id == DELTA_PACK_CANDIDATE_FAMILY_ID)
        .map(|entry| entry.root);
    T188CanonicalRoots {
        generation: inner.generation,
        page_count: inner.page_count,
        region_table_root: inner.region_table_root,
        index_root: inner.index_root,
        freemap_root: inner.freemap.map(|(root, _)| root),
        maintenance_root: inner.maintenance_root,
        overlay_root: inner.overlay_root,
        current_record_root: inner.current_record_root,
        root_catalog_root: inner.root_catalog_root,
        retained_history_root: inner.retained_history_root,
        audit_retention_root: inner.audit_retention_root,
        owner_token_root: inner.owner_token_root,
        secondary_index_root: inner.secondary_index_root,
        mutable_idempotency_root: inner.mutable_idempotency_root,
        workflow_idempotency_root: inner.workflow_idempotency_root,
        mvcc_generation_root: inner.mvcc_generation_root,
        retention_index_root: inner.retention_index_root,
        checkpoint_index_root: inner.checkpoint_index_root,
        reclaim_index_root: inner.reclaim_index_root,
        delta_pack_candidate_root,
        reference_root: inner.reference_root,
        control_root: inner.control_root,
    }
}

fn t188_15_commit_workflow(
    store: &FileStore,
    key: &OverlayKey,
    payload: &[u8],
    action: &str,
    expected: Option<loom_core::OverlayOwnerToken>,
) -> loom_core::OverlayOwnerToken {
    let mut txn = workflow_transaction_test(
        action,
        vec![workflow_put(
            FacetKind::Document,
            key.clone(),
            payload,
            expected,
        )],
        None,
    );
    txn.owner_state = loom_core::WorkflowOwnerState {
        controls: vec![loom_core::WorkflowControlWrite::Put {
            key: b"owner/current".to_vec(),
            payload: payload.to_vec(),
        }],
        audits: vec![loom_core::WorkflowAuditWrite {
            principal: None,
            action: action.to_string(),
            target: Some("owner/current".to_string()),
        }],
        ..loom_core::WorkflowOwnerState::default()
    };
    store.commit_workflow_transaction(txn).unwrap().writes[0]
        .owner_token
        .clone()
}

fn t188_15_populate_required_roots(store: &FileStore, suffix: &str) {
    let reference = store
        .put(&blob(format!("t188-15-reference-{suffix}").as_bytes()))
        .unwrap();
    store.set_reference_root(Some(reference)).unwrap();

    let history_key = format!("pages/workspace/t188-15-history-{suffix}").into_bytes();
    store
        .commit_family_root_records_for_test(
            RETAINED_HISTORY_FAMILY_ID,
            &[
                (
                    retained_history_head_address(&history_key),
                    encode_retained_history_head(&history_key, 1),
                ),
                (
                    retained_history_record_address(&history_key, 1),
                    encode_retained_history_entry(&history_key, 1, b"t188-15-history"),
                ),
            ],
        )
        .unwrap();

    let secondary_index =
        durability_facet_test_key(b"documents", &format!("t188-15-index-{suffix}"));
    store
        .commit_family_root_records_for_test(
            SECONDARY_INDEX_FAMILY_ID,
            &[(
                mutable_overlay_secondary_index_address(&secondary_index),
                secondary_index_record(
                    1,
                    secondary_index.clone(),
                    SecondaryIndexWriteOp::Put {
                        payload: format!("ticket/{suffix}").into_bytes(),
                    },
                ),
            )],
        )
        .unwrap();

    let idempotency_key = format!("t188-15-mutable-idempotency-{suffix}");
    let idempotency_target =
        durability_facet_test_key(b"documents", &format!("t188-15-idempotency-{suffix}"));
    let idempotency_digest =
        mutable_overlay_idempotency_request_digest(&idempotency_target, b"t188-15-payload");
    let idempotency_token = loom_core::OverlayOwnerToken::from_bytes([0x15; 32]);
    store
        .commit_family_root_records_for_test(
            MUTABLE_IDEMPOTENCY_FAMILY_ID,
            &[(
                mutable_overlay_idempotency_address(&idempotency_key),
                encode_mutable_overlay_idempotency_record(&idempotency_digest, &idempotency_token),
            )],
        )
        .unwrap();

    let workflow_idempotency_key = format!("t188-15-workflow-idempotency-{suffix}");
    let workflow_target =
        durability_facet_test_key(b"documents", &format!("t188-15-workflow-target-{suffix}"));
    let workflow_token = loom_core::OverlayOwnerToken::from_bytes([0x16; 32]);
    let workflow_receipt = CommitReceipt {
        generation: OverlayGeneration::new(16),
        root_after: Digest::blake3(format!("t188-15-workflow-root-{suffix}").as_bytes()),
        writes: vec![loom_core::WriteOutcome {
            facet: FacetKind::Document,
            target: workflow_target,
            owner_token: workflow_token,
            change: loom_core::OverlayEntryKind::Value,
        }],
        operation_identities: Vec::new(),
        revision_identities: Vec::new(),
        audit_sequences: Vec::new(),
        retained_sequences: Vec::new(),
        delivery_receipts: Vec::new(),
        post_commit_delta: None,
        replayed: false,
    };
    store
        .commit_family_root_records_for_test(
            WORKFLOW_IDEMPOTENCY_FAMILY_ID,
            &[(
                mutable_overlay_transaction_idempotency_address(
                    workflow_idempotency_key.as_bytes(),
                ),
                encode_workflow_transaction_idempotency_record(
                    &Digest::blake3(format!("t188-15-workflow-request-{suffix}").as_bytes()),
                    &workflow_receipt,
                )
                .unwrap(),
            )],
        )
        .unwrap();

    store
        .commit_family_root_records_for_test(
            MVCC_GENERATION_FAMILY_ID,
            &[mvcc_generation_family_record(
                OverlayGeneration::new(17),
                Some(Digest::blake3(format!("t188-15-mvcc-{suffix}").as_bytes())),
            )],
        )
        .unwrap();

    let retention_key = retention_index_test_key(&format!("T188-15-{suffix}"));
    store
        .commit_family_root_records_for_test(
            RETENTION_INDEX_FAMILY_ID,
            &[retention_index_family_record(
                &retention_key,
                b"t188-15-retention",
                Some(188_015),
            )],
        )
        .unwrap();

    store
        .commit_family_root_records_for_test(
            CHECKPOINT_INDEX_FAMILY_ID,
            &[checkpoint_index_family_record(
                format!("t188-15-checkpoint-{suffix}").as_bytes(),
                OverlayGeneration::new(18),
                Some(Digest::blake3(
                    format!("t188-15-checkpoint-root-{suffix}").as_bytes(),
                )),
                None,
            )],
        )
        .unwrap();

    store
        .commit_family_root_records_for_test(
            RECLAIM_INDEX_FAMILY_ID,
            &[reclaim_index_family_record(
                format!("t188-15-reclaim-{suffix}").as_bytes(),
                b"t188-15-blocker",
                Some(PageId(1)),
                Some(Digest::blake3(
                    format!("t188-15-reclaim-object-{suffix}").as_bytes(),
                )),
            )],
        )
        .unwrap();

    store
        .commit_family_root_records_for_test(
            DELTA_PACK_CANDIDATE_FAMILY_ID,
            &[delta_pack_advisory_family_record(
                format!("t188-15-delta-{suffix}").as_bytes(),
                DeltaPackAdvisoryKind::Candidate,
                OverlayGeneration::new(19),
                Some(Digest::blake3(
                    format!("t188-15-delta-root-{suffix}").as_bytes(),
                )),
                3,
                false,
            )],
        )
        .unwrap();
}

fn t188_15_mutate_retained_history(store: &FileStore, suffix: &str) {
    let history_key = format!("pages/workspace/t188-15-transition-retained-{suffix}").into_bytes();
    store
        .commit_family_root_records_for_test(
            RETAINED_HISTORY_FAMILY_ID,
            &[
                (
                    retained_history_head_address(&history_key),
                    encode_retained_history_head(&history_key, 1),
                ),
                (
                    retained_history_record_address(&history_key, 1),
                    encode_retained_history_entry(&history_key, 1, b"transition-retained"),
                ),
            ],
        )
        .unwrap();
}

fn t188_15_mutate_owner_token(store: &FileStore, suffix: &str) {
    let key =
        durability_facet_test_key(b"documents", &format!("t188-15-transition-owner-{suffix}"));
    let token = loom_core::OverlayOwnerToken::from_bytes([0x21; 32]);
    store
        .commit_family_root_records_for_test(
            OWNER_TOKEN_FAMILY_ID,
            &[(
                mutable_overlay_owner_token_address(&key),
                encode_mutable_overlay_owner_token_record(&token),
            )],
        )
        .unwrap();
}

fn t188_15_mutate_secondary_index(store: &FileStore, suffix: &str) {
    let index = durability_facet_test_key(
        b"documents",
        &format!("t188-15-transition-secondary-{suffix}"),
    );
    store
        .commit_family_root_records_for_test(
            SECONDARY_INDEX_FAMILY_ID,
            &[(
                mutable_overlay_secondary_index_address(&index),
                secondary_index_record(
                    2,
                    index.clone(),
                    SecondaryIndexWriteOp::Put {
                        payload: format!("secondary/{suffix}").into_bytes(),
                    },
                ),
            )],
        )
        .unwrap();
}

fn t188_15_mutate_mutable_idempotency(store: &FileStore, suffix: &str) {
    let key = durability_facet_test_key(
        b"documents",
        &format!("t188-15-transition-mutable-idempotency-{suffix}"),
    );
    let request_digest = mutable_overlay_idempotency_request_digest(&key, b"transition-payload");
    let token = loom_core::OverlayOwnerToken::from_bytes([0x22; 32]);
    store
        .commit_family_root_records_for_test(
            MUTABLE_IDEMPOTENCY_FAMILY_ID,
            &[(
                mutable_overlay_idempotency_address(&format!(
                    "t188-15-transition-mutable-idempotency-{suffix}"
                )),
                encode_mutable_overlay_idempotency_record(&request_digest, &token),
            )],
        )
        .unwrap();
}

fn t188_15_mutate_workflow_idempotency(store: &FileStore, suffix: &str) {
    let target = durability_facet_test_key(
        b"documents",
        &format!("t188-15-transition-workflow-target-{suffix}"),
    );
    let token = loom_core::OverlayOwnerToken::from_bytes([0x23; 32]);
    let receipt = CommitReceipt {
        generation: OverlayGeneration::new(23),
        root_after: Digest::blake3(format!("t188-15-transition-workflow-root-{suffix}").as_bytes()),
        writes: vec![loom_core::WriteOutcome {
            facet: FacetKind::Document,
            target,
            owner_token: token,
            change: loom_core::OverlayEntryKind::Value,
        }],
        operation_identities: Vec::new(),
        revision_identities: Vec::new(),
        audit_sequences: Vec::new(),
        retained_sequences: Vec::new(),
        delivery_receipts: Vec::new(),
        post_commit_delta: None,
        replayed: false,
    };
    store
        .commit_family_root_records_for_test(
            WORKFLOW_IDEMPOTENCY_FAMILY_ID,
            &[(
                mutable_overlay_transaction_idempotency_address(
                    format!("t188-15-transition-workflow-idempotency-{suffix}").as_bytes(),
                ),
                encode_workflow_transaction_idempotency_record(
                    &Digest::blake3(
                        format!("t188-15-transition-workflow-request-{suffix}").as_bytes(),
                    ),
                    &receipt,
                )
                .unwrap(),
            )],
        )
        .unwrap();
}

fn t188_15_mutate_audit_retention(store: &FileStore, suffix: &str) {
    let mut audit_map = BTreeMap::new();
    append_audit_record(
        &mut audit_map,
        store.digest_algo,
        None,
        &format!("t188.15.transition.audit.{suffix}"),
        Some("transition"),
    )
    .unwrap();
    store
        .commit_family_root_records_for_test(
            AUDIT_RETENTION_FAMILY_ID,
            &audit_retention_family_records(&audit_map),
        )
        .unwrap();
}

fn t188_15_mutate_mvcc_generation(store: &FileStore, suffix: &str) {
    store
        .commit_family_root_records_for_test(
            MVCC_GENERATION_FAMILY_ID,
            &[mvcc_generation_family_record(
                OverlayGeneration::new(24),
                Some(Digest::blake3(
                    format!("t188-15-transition-mvcc-{suffix}").as_bytes(),
                )),
            )],
        )
        .unwrap();
}

fn t188_15_mutate_retention_index(store: &FileStore, suffix: &str) {
    let key = retention_index_test_key(&format!("T188-15-transition-{suffix}"));
    store
        .commit_family_root_records_for_test(
            RETENTION_INDEX_FAMILY_ID,
            &[retention_index_family_record(
                &key,
                b"t188-15-transition-retention",
                Some(188_150),
            )],
        )
        .unwrap();
}

fn t188_15_mutate_checkpoint_index(store: &FileStore, suffix: &str) {
    store
        .commit_family_root_records_for_test(
            CHECKPOINT_INDEX_FAMILY_ID,
            &[checkpoint_index_family_record(
                format!("t188-15-transition-checkpoint-{suffix}").as_bytes(),
                OverlayGeneration::new(25),
                Some(Digest::blake3(
                    format!("t188-15-transition-checkpoint-root-{suffix}").as_bytes(),
                )),
                None,
            )],
        )
        .unwrap();
}

fn t188_15_mutate_reclaim_index(store: &FileStore, suffix: &str) {
    store
        .commit_family_root_records_for_test(
            RECLAIM_INDEX_FAMILY_ID,
            &[reclaim_index_family_record(
                format!("t188-15-transition-reclaim-{suffix}").as_bytes(),
                b"transition-blocker",
                Some(PageId(1)),
                Some(Digest::blake3(
                    format!("t188-15-transition-reclaim-object-{suffix}").as_bytes(),
                )),
            )],
        )
        .unwrap();
}

fn t188_15_mutate_delta_pack_candidate(store: &FileStore, suffix: &str) {
    store
        .commit_family_root_records_for_test(
            DELTA_PACK_CANDIDATE_FAMILY_ID,
            &[delta_pack_advisory_family_record(
                format!("t188-15-transition-delta-{suffix}").as_bytes(),
                DeltaPackAdvisoryKind::Candidate,
                OverlayGeneration::new(26),
                Some(Digest::blake3(
                    format!("t188-15-transition-delta-root-{suffix}").as_bytes(),
                )),
                4,
                false,
            )],
        )
        .unwrap();
}

fn t188_15_catalog_family_root(roots: T188CanonicalRoots, family_id: u16) -> Option<PageId> {
    match family_id {
        RETAINED_HISTORY_FAMILY_ID => roots.retained_history_root,
        OWNER_TOKEN_FAMILY_ID => roots.owner_token_root,
        SECONDARY_INDEX_FAMILY_ID => roots.secondary_index_root,
        MUTABLE_IDEMPOTENCY_FAMILY_ID => roots.mutable_idempotency_root,
        WORKFLOW_IDEMPOTENCY_FAMILY_ID => roots.workflow_idempotency_root,
        AUDIT_RETENTION_FAMILY_ID => roots.audit_retention_root,
        MVCC_GENERATION_FAMILY_ID => roots.mvcc_generation_root,
        RETENTION_INDEX_FAMILY_ID => roots.retention_index_root,
        CHECKPOINT_INDEX_FAMILY_ID => roots.checkpoint_index_root,
        RECLAIM_INDEX_FAMILY_ID => roots.reclaim_index_root,
        DELTA_PACK_CANDIDATE_FAMILY_ID => roots.delta_pack_candidate_root,
        _ => None,
    }
}

struct T188CatalogFamilyTransition {
    name: &'static str,
    family_id: u16,
    mutate: fn(&FileStore, &str),
}

const T188_15_CATALOG_FAMILY_TRANSITIONS: &[T188CatalogFamilyTransition] = &[
    T188CatalogFamilyTransition {
        name: "retained_history",
        family_id: RETAINED_HISTORY_FAMILY_ID,
        mutate: t188_15_mutate_retained_history,
    },
    T188CatalogFamilyTransition {
        name: "owner_tokens",
        family_id: OWNER_TOKEN_FAMILY_ID,
        mutate: t188_15_mutate_owner_token,
    },
    T188CatalogFamilyTransition {
        name: "secondary_indexes",
        family_id: SECONDARY_INDEX_FAMILY_ID,
        mutate: t188_15_mutate_secondary_index,
    },
    T188CatalogFamilyTransition {
        name: "mutable_idempotency",
        family_id: MUTABLE_IDEMPOTENCY_FAMILY_ID,
        mutate: t188_15_mutate_mutable_idempotency,
    },
    T188CatalogFamilyTransition {
        name: "workflow_idempotency",
        family_id: WORKFLOW_IDEMPOTENCY_FAMILY_ID,
        mutate: t188_15_mutate_workflow_idempotency,
    },
    T188CatalogFamilyTransition {
        name: "audit_retention",
        family_id: AUDIT_RETENTION_FAMILY_ID,
        mutate: t188_15_mutate_audit_retention,
    },
    T188CatalogFamilyTransition {
        name: "mvcc_generations",
        family_id: MVCC_GENERATION_FAMILY_ID,
        mutate: t188_15_mutate_mvcc_generation,
    },
    T188CatalogFamilyTransition {
        name: "retention_index",
        family_id: RETENTION_INDEX_FAMILY_ID,
        mutate: t188_15_mutate_retention_index,
    },
    T188CatalogFamilyTransition {
        name: "checkpoint_index",
        family_id: CHECKPOINT_INDEX_FAMILY_ID,
        mutate: t188_15_mutate_checkpoint_index,
    },
    T188CatalogFamilyTransition {
        name: "reclaim_index",
        family_id: RECLAIM_INDEX_FAMILY_ID,
        mutate: t188_15_mutate_reclaim_index,
    },
    T188CatalogFamilyTransition {
        name: "delta_pack_candidates",
        family_id: DELTA_PACK_CANDIDATE_FAMILY_ID,
        mutate: t188_15_mutate_delta_pack_candidate,
    },
];

fn t188_15_build_canonical_generations()
-> (Vec<u8>, OverlayKey, T188CanonicalRoots, T188CanonicalRoots) {
    let tp = TempPath::new("t188-15-canonical");
    let key = durability_facet_test_key(b"documents", "t188-15-current");
    let (old_roots, new_roots) = {
        let store = FileStore::open(tp.path()).unwrap();
        let token = t188_15_commit_workflow(&store, &key, b"old-current", "t188.15.old", None);
        t188_15_populate_required_roots(&store, "old");
        let old_roots = t188_15_roots(&store);
        t188_15_commit_workflow(&store, &key, b"new-current", "t188.15.new", Some(token));
        let new_roots = t188_15_roots(&store);
        (old_roots, new_roots)
    };
    (std::fs::read(tp.path()).unwrap(), key, old_roots, new_roots)
}

fn t188_15_newest_journal_roots(bytes: &[u8]) -> journal::Roots {
    let mut newest: Option<journal::Roots> = None;
    for i in 0..RING_SLOTS {
        let off = (JOURNAL_OFFSET + i * journal::RECORD_SIZE as u64) as usize;
        if let Some((journal::KIND_COMMIT, roots)) =
            journal::decode(&bytes[off..off + journal::RECORD_SIZE])
            && newest.is_none_or(|known| roots.generation > known.generation)
        {
            newest = Some(roots);
        }
    }
    newest.unwrap()
}

fn t188_15_journal_offset(generation: u64) -> usize {
    (JOURNAL_OFFSET + (generation % RING_SLOTS) * journal::RECORD_SIZE as u64) as usize
}

fn t188_15_corrupt_page(bytes: &mut [u8], page: PageId) {
    let off = (DATA_START + page.0 * PAGE_SIZE) as usize;
    bytes[off + 17] ^= 0x5A;
}

fn t188_15_corrupt_digest_record(bytes: &mut [u8], digest: Digest) {
    let store = open_read_bytes(bytes, "t188-15-locate-digest").unwrap();
    assert!(store.get(&digest).unwrap().is_some());
    let loc = store
        .inner
        .lock()
        .unwrap()
        .index
        .get(digest.bytes())
        .copied()
        .unwrap();
    drop(store);
    t188_15_corrupt_page(bytes, PageId(loc.global_page()));
}

fn t188_15_assert_roots(store: &FileStore, expected: T188CanonicalRoots) {
    assert_eq!(t188_15_roots(store), expected);
}

fn t188_15_assert_required_roots_populated(roots: T188CanonicalRoots) {
    assert!(roots.region_table_root.is_some());
    assert!(roots.index_root.is_some());
    assert!(roots.freemap_root.is_some());
    assert!(roots.maintenance_root.is_some());
    assert_eq!(roots.overlay_root, None);
    assert!(roots.current_record_root.is_some());
    assert!(roots.root_catalog_root.is_some());
    assert!(roots.retained_history_root.is_some());
    assert!(roots.audit_retention_root.is_some());
    assert!(roots.owner_token_root.is_some());
    assert!(roots.secondary_index_root.is_some());
    assert!(roots.mutable_idempotency_root.is_some());
    assert!(roots.workflow_idempotency_root.is_some());
    assert!(roots.mvcc_generation_root.is_some());
    assert!(roots.retention_index_root.is_some());
    assert!(roots.checkpoint_index_root.is_some());
    assert!(roots.reclaim_index_root.is_some());
    assert!(roots.delta_pack_candidate_root.is_some());
    assert!(roots.reference_root.is_some());
    assert!(roots.control_root.is_some());
}

fn t188_15_assert_visible_current_and_audit(
    store: &FileStore,
    key: &OverlayKey,
    value: &[u8],
    actions: &[&str],
) {
    assert_eq!(
        store
            .mutable_overlay_snapshot()
            .unwrap()
            .read_composite(key, |_| Ok(None))
            .unwrap()
            .as_deref(),
        Some(value)
    );
    let audit_actions = store
        .audit_records()
        .unwrap()
        .into_iter()
        .map(|record| record.action)
        .collect::<Vec<_>>();
    assert_eq!(audit_actions, actions);
}

#[test]
fn t188_15_complete_latest_journal_recovers_complete_new_canonical_root_set() {
    let (bytes, key, _old_roots, new_roots) = t188_15_build_canonical_generations();
    let newest = t188_15_newest_journal_roots(&bytes);

    t188_15_assert_required_roots_populated(new_roots);
    assert_eq!(newest.generation, new_roots.generation);
    assert_eq!(newest.page_count, new_roots.page_count);
    assert_eq!(newest.region_table, new_roots.region_table_root);

    let store = open_read_bytes(&bytes, "t188-15-complete-new").unwrap();

    t188_15_assert_roots(&store, new_roots);
    t188_15_assert_visible_current_and_audit(
        &store,
        &key,
        b"new-current",
        &["t188.15.old", "t188.15.new"],
    );
}

#[test]
fn t188_15_torn_latest_journal_recovers_complete_old_canonical_root_set() {
    let (mut bytes, key, old_roots, new_roots) = t188_15_build_canonical_generations();
    let off = t188_15_journal_offset(new_roots.generation);
    for byte in &mut bytes[off..off + journal::RECORD_SIZE] {
        *byte ^= 0xFF;
    }

    t188_15_assert_required_roots_populated(old_roots);
    t188_15_assert_required_roots_populated(new_roots);
    let store = open_read_bytes(&bytes, "t188-15-torn-journal").unwrap();

    t188_15_assert_roots(&store, old_roots);
    t188_15_assert_visible_current_and_audit(&store, &key, b"old-current", &["t188.15.old"]);
}

#[test]
fn t188_15_catalog_family_transitions_recover_complete_old_or_new_vectors() {
    for family in T188_15_CATALOG_FAMILY_TRANSITIONS {
        let tp = TempPath::new(&format!("t188-15-transition-{}", family.name));
        let key = durability_facet_test_key(
            b"documents",
            &format!("t188-15-transition-current-{}", family.name),
        );
        let (prior_roots, new_roots) = {
            let store = FileStore::open(tp.path()).unwrap();
            t188_15_commit_workflow(
                &store,
                &key,
                format!("current-before-{}", family.name).as_bytes(),
                "t188.15.transition.before",
                None,
            );
            t188_15_populate_required_roots(&store, &format!("transition-before-{}", family.name));
            let prior_roots = t188_15_roots(&store);
            t188_15_assert_required_roots_populated(prior_roots);

            (family.mutate)(&store, &format!("transition-after-{}", family.name));
            let new_roots = t188_15_roots(&store);
            t188_15_assert_required_roots_populated(new_roots);
            assert_ne!(
                t188_15_catalog_family_root(prior_roots, family.family_id),
                t188_15_catalog_family_root(new_roots, family.family_id),
                "{} root did not change",
                family.name
            );
            (prior_roots, new_roots)
        };

        let reopened = FileStore::open_read(tp.path()).unwrap();
        t188_15_assert_roots(&reopened, new_roots);
        drop(reopened);

        let mut torn = std::fs::read(tp.path()).unwrap();
        let off = t188_15_journal_offset(new_roots.generation);
        for byte in &mut torn[off..off + journal::RECORD_SIZE] {
            *byte ^= 0xFF;
        }
        let torn_reopen =
            open_read_bytes(&torn, &format!("t188-15-transition-torn-{}", family.name)).unwrap();
        t188_15_assert_roots(&torn_reopen, prior_roots);

        assert_eq!(
            t188_15_newest_journal_roots(&std::fs::read(tp.path()).unwrap()).generation,
            new_roots.generation,
            "{} did not leave newest generation in journal",
            family.name
        );
    }
}

#[test]
fn t188_15_corrupt_latest_region_table_fails_closed() {
    let (mut bytes, _key, _old_roots, new_roots) = t188_15_build_canonical_generations();
    t188_15_corrupt_page(&mut bytes, new_roots.region_table_root.unwrap());

    let err = open_read_bytes(&bytes, "t188-15-corrupt-region-table").unwrap_err();

    assert_eq!(err.code, Code::CorruptObject);
}

#[test]
fn t188_15_partial_region_table_publication_fails_closed() {
    let (mut bytes, _key, old_roots, new_roots) = t188_15_build_canonical_generations();
    let new_off = (DATA_START + new_roots.region_table_root.unwrap().0 * PAGE_SIZE) as usize;
    let old_off = (DATA_START + old_roots.region_table_root.unwrap().0 * PAGE_SIZE) as usize;
    let old_page = bytes[old_off..old_off + PAGE_SIZE as usize].to_vec();
    bytes[new_off..new_off + 128].copy_from_slice(&old_page[..128]);

    let err = open_read_bytes(&bytes, "t188-15-partial-region-table").unwrap_err();

    assert_eq!(err.code, Code::CorruptObject);
}

#[test]
fn t188_15_corrupt_root_catalog_fails_closed_on_open() {
    let (mut bytes, _key, _old_roots, new_roots) = t188_15_build_canonical_generations();
    t188_15_corrupt_page(&mut bytes, new_roots.root_catalog_root.unwrap());

    let err = open_read_bytes(&bytes, "t188-15-corrupt-root-catalog").unwrap_err();

    assert_eq!(err.code, Code::CorruptObject);
}

#[test]
fn t188_15_corrupt_freemap_fails_closed_on_open() {
    let (mut bytes, _key, _old_roots, new_roots) = t188_15_build_canonical_generations();
    t188_15_corrupt_page(&mut bytes, new_roots.freemap_root.unwrap());

    let err = open_read_bytes(&bytes, "t188-15-corrupt-freemap").unwrap_err();

    assert_eq!(err.code, Code::CorruptObject);
}

#[test]
fn t188_15_corrupt_maintenance_fails_closed_on_open() {
    let (mut bytes, _key, _old_roots, new_roots) = t188_15_build_canonical_generations();
    t188_15_corrupt_page(&mut bytes, new_roots.maintenance_root.unwrap());

    let err = open_read_bytes(&bytes, "t188-15-corrupt-maintenance").unwrap_err();

    assert_eq!(err.code, Code::CorruptObject);
}

#[test]
fn t188_15_corrupt_current_root_fails_closed_on_open() {
    let (mut bytes, _key, _old_roots, new_roots) = t188_15_build_canonical_generations();
    t188_15_corrupt_page(&mut bytes, new_roots.current_record_root.unwrap());

    let err = open_read_bytes(&bytes, "t188-15-corrupt-current-root").unwrap_err();

    assert_eq!(err.code, Code::CorruptObject);
}

#[test]
fn t188_15_corrupt_family_root_does_not_expose_mixed_audit_state() {
    let (mut bytes, key, _old_roots, new_roots) = t188_15_build_canonical_generations();
    t188_15_corrupt_page(&mut bytes, new_roots.audit_retention_root.unwrap());

    let store = open_read_bytes(&bytes, "t188-15-corrupt-family-root").unwrap();

    t188_15_assert_roots(&store, new_roots);
    assert_eq!(
        store
            .mutable_overlay_snapshot()
            .unwrap()
            .read_composite(&key, |_| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"new-current"[..])
    );
    assert_eq!(store.audit_records().unwrap_err().code, Code::CorruptObject);
}

#[test]
fn t188_15_corrupt_advisory_family_root_fails_when_read_without_stale_fallback() {
    let (mut bytes, _key, _old_roots, new_roots) = t188_15_build_canonical_generations();
    t188_15_corrupt_page(&mut bytes, new_roots.delta_pack_candidate_root.unwrap());

    let store = open_read_bytes(&bytes, "t188-15-corrupt-advisory").unwrap();

    t188_15_assert_roots(&store, new_roots);
    assert_eq!(
        store
            .delta_pack_advisory_record(b"t188-15-delta-old")
            .unwrap_err()
            .code,
        Code::CorruptObject
    );
}

#[test]
fn t188_15_corrupt_reference_root_fails_when_read_without_stale_fallback() {
    let (mut bytes, _key, _old_roots, new_roots) = t188_15_build_canonical_generations();
    let reference = new_roots.reference_root.unwrap();
    t188_15_corrupt_digest_record(&mut bytes, reference);

    let store = open_read_bytes(&bytes, "t188-15-corrupt-reference").unwrap();

    t188_15_assert_roots(&store, new_roots);
    assert_eq!(store.get(&reference).unwrap_err().code, Code::CorruptObject);
}

#[test]
fn t188_15_corrupt_control_root_fails_when_read_without_stale_fallback() {
    let (mut bytes, _key, _old_roots, new_roots) = t188_15_build_canonical_generations();
    let control = new_roots.control_root.unwrap();
    t188_15_corrupt_digest_record(&mut bytes, control);

    let store = open_read_bytes(&bytes, "t188-15-corrupt-control").unwrap();

    t188_15_assert_roots(&store, new_roots);
    assert_eq!(
        store.control_root_map().unwrap_err().code,
        Code::CorruptObject
    );
}

fn t188_16_attr<'a>(
    report: &'a StoreRootStorageAttribution,
    root: &str,
) -> &'a StoreRootStorageClass {
    report
        .roots
        .iter()
        .find(|entry| entry.root == root)
        .unwrap_or_else(|| panic!("missing root attribution for {root}"))
}

#[test]
fn t188_16_root_storage_attribution_reports_every_canonical_root_class() {
    let (bytes, _key, _old_roots, _new_roots) = t188_15_build_canonical_generations();
    let store = open_read_bytes(&bytes, "t188-16-root-attribution").unwrap();
    let report = store.root_storage_attribution(100).unwrap();

    assert_eq!(report.page_size, PAGE_SIZE);
    assert!(report.physical_bytes >= DATA_START);
    for (root, role) in [
        ("object_index_records", "object_index"),
        ("current_records", "current"),
        ("root_catalog", "root_catalog"),
        ("free_map", "physical_metadata"),
        ("maintenance", "physical_metadata"),
        ("reference_root", "reference"),
        ("control_root", "control"),
    ] {
        let entry = t188_16_attr(&report, root);
        assert!(entry.present, "{root} should be present");
        assert_eq!(entry.role, role);
    }
    assert!(t188_16_attr(&report, "object_index_records").tree_pages > 0);
    assert!(t188_16_attr(&report, "object_index_records").record_pages > 0);
    assert!(t188_16_attr(&report, "object_index_records").payload_bytes > 0);
    assert!(t188_16_attr(&report, "current_records").tree_pages > 0);
    assert!(t188_16_attr(&report, "current_records").payload_bytes > 0);
    assert!(t188_16_attr(&report, "reference_root").payload_bytes > 0);
    assert!(t188_16_attr(&report, "control_root").payload_bytes > 0);

    for descriptor in ROOT_FAMILY_REGISTRY {
        if descriptor.family_id == CURRENT_RECORDS_FAMILY_ID {
            continue;
        }
        let entry = report
            .roots
            .iter()
            .find(|entry| entry.family_id == Some(descriptor.family_id))
            .unwrap_or_else(|| panic!("missing family attribution for {}", descriptor.name));
        assert_eq!(entry.root, descriptor.name);
        assert!(entry.present, "{} should be present", descriptor.name);
        assert!(entry.tree_pages > 0, "{} tree pages", descriptor.name);
        assert!(entry.payload_bytes > 0, "{} payload bytes", descriptor.name);
        if descriptor.family_id == DELTA_PACK_CANDIDATE_FAMILY_ID {
            assert_eq!(entry.role, "advisory:advisory");
        } else if descriptor.flags == ROOT_FLAG_AUTHORITATIVE {
            assert!(entry.role.ends_with(":authoritative"));
        }
    }
    assert!(
        report
            .stale_owner_reasons
            .iter()
            .any(|reason| reason.reason == "pending_free_map_age"
                || reason.reason == "unknown_ownership"
                || reason.reason.starts_with("stale_")),
        "expected concrete stale, unknown, or pending free-map attribution"
    );
}

#[test]
fn t188_16_root_storage_attribution_is_read_only_and_reports_stale_reasons() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let key = durability_facet_test_key(b"documents", "t188-16-stale-reasons");
    for update in 0..12 {
        store
            .put_mutable_overlay_value(key.clone(), format!("current-{update}").into_bytes())
            .unwrap();
    }
    let before = t188_15_roots(&store);
    let report = store.root_storage_attribution(8).unwrap();
    let after = t188_15_roots(&store);

    assert_eq!(after, before);
    assert!(
        report
            .stale_owner_reasons
            .iter()
            .any(|reason| reason.reason == "recovery_generation_floor"
                || reason.reason == "pending_free_map_age"
                || reason.reason.starts_with("stale_record_")),
        "expected concrete stale blocker reasons"
    );
    drop(store);
    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    assert_eq!(t188_15_roots(&reopened), before);
}

#[test]
fn t188_16_root_storage_attribution_walks_descendant_only_reference_objects() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let leaf = store.put(&blob(b"t188-16-descendant-leaf")).unwrap();
    let tree = store
        .put(
            &Object::tree(vec![loom_core::TreeEntry {
                name: "leaf".to_string(),
                kind: loom_core::EntryKind::Symlink,
                target: leaf,
                mode: 0o120000,
            }])
            .unwrap()
            .canonical(),
        )
        .unwrap();
    let commit = store
        .put(
            &Object::Commit(loom_core::Commit {
                tree,
                parents: Vec::new(),
                author: "t188-16".to_string(),
                timestamp_ms: 16,
                message: "descendant graph".to_string(),
                meta: BTreeMap::new(),
            })
            .canonical(),
        )
        .unwrap();
    store.set_reference_root(Some(commit)).unwrap();

    let report = store.root_storage_attribution(32).unwrap();
    let root = t188_16_attr(&report, "reference_root");
    assert!(root.record_pages >= 3);
    assert!(report.object_reverse_ownership.iter().any(|owner| {
        owner.digest == leaf
            && owner
                .retaining_roots
                .iter()
                .any(|root| root == "reference_root")
            && owner
                .logical_owners
                .iter()
                .any(|owner| owner == "reference_object_graph")
    }));
}

#[test]
fn t188_16_root_storage_attribution_reports_shared_multiple_root_owners() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let leaf = store.put(&blob(b"t188-16-shared-leaf")).unwrap();
    let tree = store
        .put(
            &Object::tree(vec![loom_core::TreeEntry {
                name: "leaf".to_string(),
                kind: loom_core::EntryKind::Symlink,
                target: leaf,
                mode: 0o120000,
            }])
            .unwrap()
            .canonical(),
        )
        .unwrap();
    let commit = store
        .put(
            &Object::Commit(loom_core::Commit {
                tree,
                parents: Vec::new(),
                author: "t188-16".to_string(),
                timestamp_ms: 16,
                message: "shared graph".to_string(),
                meta: BTreeMap::new(),
            })
            .canonical(),
        )
        .unwrap();
    store.set_reference_root(Some(commit)).unwrap();
    store.set_control_root(Some(commit)).unwrap();

    let report = store.root_storage_attribution(32).unwrap();
    let owner = report
        .object_reverse_ownership
        .iter()
        .find(|owner| owner.digest == leaf)
        .unwrap();
    assert!(owner.frame_kind.starts_with("record_"));
    assert!(owner.byte_span >= owner.payload_bytes);
    assert!(
        owner
            .retaining_roots
            .iter()
            .any(|root| root == "reference_root")
    );
    assert!(
        owner
            .retaining_roots
            .iter()
            .any(|root| root == "control_root")
    );
    assert!(
        owner
            .logical_owners
            .iter()
            .any(|logical| logical == "reference_object_graph")
    );
    assert!(
        owner
            .logical_owners
            .iter()
            .any(|logical| logical == "control_object_graph")
    );
}

#[test]
fn t188_16_root_storage_attribution_reports_concrete_current_record_blockers() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let key = durability_facet_test_key(b"documents", "t188-16-concrete-blocker");
    store
        .put_mutable_overlay_value(key.clone(), b"one".to_vec())
        .unwrap();
    let _snapshot = store
        .open_mvcc_snapshot_with_owner(Some("t188-16-blocker"))
        .unwrap();

    let report = store.root_storage_attribution(16).unwrap();
    assert!(
        report
            .stale_owner_reasons
            .iter()
            .any(|reason| reason.reason == "pinned_snapshot"
                && reason.current_key.as_deref() == Some(key.as_bytes())),
        "{:?}",
        report.stale_owner_reasons
    );
    assert!(report.object_reverse_ownership.iter().any(|owner| {
        owner
            .retaining_roots
            .iter()
            .any(|root| root == "current_records")
            && owner.current_key.as_deref() == Some(key.as_bytes())
    }));
}

#[test]
fn t188_16_root_storage_attribution_separates_index_membership_from_semantic_ownership() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let unowned = store.put(&blob(b"t188-16-indexed-unowned")).unwrap();

    let report = store.root_storage_attribution(1).unwrap();
    let owner = report
        .object_reverse_ownership
        .iter()
        .find(|owner| owner.digest == unowned)
        .unwrap();
    assert!(
        owner
            .physical_roots
            .iter()
            .any(|root| root == "object_index_records")
    );
    assert!(owner.retaining_roots.is_empty());
    assert!(owner.logical_owners.is_empty());
}

#[test]
fn t188_16_root_storage_attribution_does_not_truncate_authoritative_owner_sets() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let payload = b"t188-16-shared-family-record".to_vec();
    for (family_id, address_seed) in [
        (RETENTION_INDEX_FAMILY_ID, 0xA1),
        (CHECKPOINT_INDEX_FAMILY_ID, 0xA2),
        (RECLAIM_INDEX_FAMILY_ID, 0xA3),
        (DELTA_PACK_CANDIDATE_FAMILY_ID, 0xA4),
    ] {
        store
            .commit_family_root_records_for_test(
                family_id,
                &[([address_seed; 32], payload.clone())],
            )
            .unwrap();
    }

    let report = store.root_storage_attribution(1).unwrap();
    let digest = Digest::hash(Algo::Blake3, &payload);
    let owner = report
        .object_reverse_ownership
        .iter()
        .find(|owner| owner.digest == digest)
        .unwrap();
    assert!(owner.retaining_roots.len() > 1, "{:?}", owner);
    assert!(
        owner
            .retaining_roots
            .iter()
            .any(|root| root == "retention_index")
    );
    assert!(
        owner
            .retaining_roots
            .iter()
            .any(|root| root == "checkpoint_index")
    );
    assert!(
        owner
            .retaining_roots
            .iter()
            .any(|root| root == "reclaim_index")
    );
    assert!(
        owner
            .retaining_roots
            .iter()
            .any(|root| root == "delta_pack_candidates")
    );
}

#[test]
fn t188_16_root_storage_attribution_reports_unresolved_authoritative_descendants() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let missing = Digest::hash(Algo::Blake3, b"t188-16-missing-child");
    let malformed = store.put(b"not a canonical loom object").unwrap();
    let tree = store
        .put(
            &Object::tree(vec![
                loom_core::TreeEntry {
                    name: "malformed".to_string(),
                    kind: loom_core::EntryKind::Symlink,
                    target: malformed,
                    mode: 0o120000,
                },
                loom_core::TreeEntry {
                    name: "missing".to_string(),
                    kind: loom_core::EntryKind::Symlink,
                    target: missing,
                    mode: 0o120000,
                },
            ])
            .unwrap()
            .canonical(),
        )
        .unwrap();
    let commit = store
        .put(
            &Object::Commit(loom_core::Commit {
                tree,
                parents: Vec::new(),
                author: "t188-16".to_string(),
                timestamp_ms: 16,
                message: "unresolved graph".to_string(),
                meta: BTreeMap::new(),
            })
            .canonical(),
        )
        .unwrap();
    store.set_reference_root(Some(commit)).unwrap();

    let report = store.root_storage_attribution(1).unwrap();
    let missing_owner = report
        .object_reverse_ownership
        .iter()
        .find(|owner| owner.digest == missing)
        .unwrap();
    assert_eq!(
        missing_owner.unresolved_reason.as_deref(),
        Some("missing_object_locator")
    );
    assert!(missing_owner.record_loc.is_none());
    assert!(
        missing_owner
            .retaining_roots
            .iter()
            .any(|root| root == "reference_root")
    );

    let malformed_owner = report
        .object_reverse_ownership
        .iter()
        .find(|owner| owner.digest == malformed)
        .unwrap();
    assert_eq!(
        malformed_owner.unresolved_reason.as_deref(),
        Some("invalid_canonical_object")
    );
    assert!(
        malformed_owner
            .physical_roots
            .iter()
            .any(|root| root == "object_index_records")
    );
    assert!(
        malformed_owner
            .retaining_roots
            .iter()
            .any(|root| root == "reference_root")
    );
}

#[test]
fn t188_16_root_storage_attribution_reports_missing_tree_blob_descendant() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let missing_blob = Digest::hash(Algo::Blake3, b"t188-16-missing-blob-child");
    let tree = store
        .put(
            &Object::tree(vec![loom_core::TreeEntry {
                name: "missing-file".to_string(),
                kind: loom_core::EntryKind::Blob,
                target: missing_blob,
                mode: 0o100644,
            }])
            .unwrap()
            .canonical(),
        )
        .unwrap();
    let commit = store
        .put(
            &Object::Commit(loom_core::Commit {
                tree,
                parents: Vec::new(),
                author: "t188-16".to_string(),
                timestamp_ms: 16,
                message: "missing blob graph".to_string(),
                meta: BTreeMap::new(),
            })
            .canonical(),
        )
        .unwrap();
    store.set_reference_root(Some(commit)).unwrap();

    let report = store.root_storage_attribution(1).unwrap();
    let missing_owner = report
        .object_reverse_ownership
        .iter()
        .find(|owner| owner.digest == missing_blob)
        .unwrap();
    assert_eq!(
        missing_owner.unresolved_reason.as_deref(),
        Some("missing_object_locator")
    );
    assert!(
        missing_owner
            .retaining_roots
            .iter()
            .any(|root| root == "reference_root")
    );
    assert!(
        missing_owner
            .logical_owners
            .iter()
            .any(|owner| owner == "reference_object_graph")
    );
}

fn t188_17_root<'a>(evidence: &'a GcReclaimEvidence, name: &str) -> &'a GcCanonicalRootEvidence {
    evidence
        .canonical_roots
        .iter()
        .find(|root| root.name == name)
        .unwrap_or_else(|| panic!("missing GC canonical root evidence for {name}"))
}

fn t188_17_family_root(evidence: &GcReclaimEvidence, family_id: u16) -> &GcCanonicalRootEvidence {
    evidence
        .canonical_roots
        .iter()
        .find(|root| root.family_id == Some(family_id))
        .unwrap_or_else(|| panic!("missing GC family root evidence for {family_id:#06x}"))
}

#[test]
fn t188_17_gc_evidence_reports_each_canonical_root_independently() {
    for descriptor in ROOT_FAMILY_REGISTRY {
        let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
        if descriptor.family_id == CURRENT_RECORDS_FAMILY_ID {
            let key = durability_facet_test_key(b"documents", "t188-17-current");
            store
                .put_mutable_overlay_value(key, b"current".to_vec())
                .unwrap();
        } else {
            store
                .commit_family_root_records_for_test(
                    descriptor.family_id,
                    &[(
                        [descriptor.family_id as u8; 32],
                        b"t188-17-family-root".to_vec(),
                    )],
                )
                .unwrap();
        }

        let evidence = store.gc_reclaim_evidence_for_test().unwrap();
        let root = t188_17_family_root(&evidence, descriptor.family_id);
        assert_eq!(root.name, descriptor.name);
        assert!(root.page_root.is_some(), "{}", descriptor.name);
        assert_eq!(
            root.advisory,
            descriptor.gc_reachability == RootFamilyReachability::AdvisoryPreserveOnly
        );
        assert_eq!(
            root.semantic_liveness,
            matches!(
                descriptor.gc_reachability,
                RootFamilyReachability::SemanticRoot | RootFamilyReachability::ControlRoot
            ),
            "{}",
            descriptor.name
        );
    }

    let (bytes, _key, _old_roots, _new_roots) = t188_15_build_canonical_generations();
    let store = open_read_bytes(&bytes, "t188-17-root-evidence").unwrap();
    let evidence = store.gc_reclaim_evidence_for_test().unwrap();
    for root in [
        "object_index_records",
        "reference_root",
        "control_root",
        "current_records",
        "root_catalog",
        "free_map",
        "maintenance",
    ] {
        t188_17_root(&evidence, root);
    }
    assert!(
        t188_17_root(&evidence, "object_index_records")
            .page_root
            .is_some()
    );
    assert!(
        t188_17_root(&evidence, "reference_root")
            .digest_root
            .is_some()
    );
    assert!(
        t188_17_root(&evidence, "control_root")
            .digest_root
            .is_some()
    );
    assert!(t188_17_root(&evidence, "free_map").page_root.is_some());
    assert!(t188_17_root(&evidence, "maintenance").page_root.is_some());
    assert!(evidence.canonical_roots_fingerprint.is_some());
}

#[test]
fn t188_17_advisory_family_does_not_create_semantic_liveness() {
    let tp = TempPath::new("t188-17-advisory-not-live");
    let mut store = FileStore::open(tp.path()).unwrap();
    store
        .commit_family_root_records_for_test(
            DELTA_PACK_CANDIDATE_FAMILY_ID,
            &[([0xD7; 32], b"advisory candidate".to_vec())],
        )
        .unwrap();
    let advisory_only = store.put(&blob(b"t188-17-advisory-only-object")).unwrap();
    for i in 0..300usize {
        store
            .put(&blob(format!("t188-17-garbage-{i:04}").as_bytes()))
            .unwrap();
    }
    let evidence = store.gc_reclaim_evidence_for_test().unwrap();
    let advisory = t188_17_family_root(&evidence, DELTA_PACK_CANDIDATE_FAMILY_ID);
    assert!(advisory.page_root.is_some());
    assert!(advisory.advisory);
    assert!(!advisory.semantic_liveness);

    let state = loom_core::ReachabilityMarkState {
        pinned: BTreeSet::new(),
        marked: BTreeSet::new(),
        queue: std::collections::VecDeque::new(),
        stream_roots: std::collections::VecDeque::new(),
        content_roots: std::collections::VecDeque::new(),
        prolly_cursors: std::collections::VecDeque::new(),
        completed: true,
    };
    let epoch = store
        .begin_reachability_mark_epoch(None, BTreeSet::new(), state)
        .unwrap();
    store.complete_reachability_mark_epoch(&epoch).unwrap();
    store
        .gc_validated_segments(GcSegmentBudget::unlimited())
        .unwrap();
    assert!(!store.has(&advisory_only).unwrap());
}

#[test]
fn t188_17_mark_epoch_allows_canonical_family_root_advancement() {
    for descriptor in ROOT_FAMILY_REGISTRY {
        let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
        let state = loom_core::ReachabilityMarkState {
            pinned: BTreeSet::new(),
            marked: BTreeSet::new(),
            queue: std::collections::VecDeque::new(),
            stream_roots: std::collections::VecDeque::new(),
            content_roots: std::collections::VecDeque::new(),
            prolly_cursors: std::collections::VecDeque::new(),
            completed: true,
        };
        let epoch = store
            .begin_reachability_mark_epoch(None, BTreeSet::new(), state)
            .unwrap();
        let captured_fingerprint = epoch.canonical_roots_fingerprint;
        let current_key = OverlayKey::from_segments([
            b"t188-17",
            descriptor.name.as_bytes(),
            b"current-root",
            b"gc",
            b"family",
            b"change",
        ])
        .unwrap();
        if descriptor.family_id == CURRENT_RECORDS_FAMILY_ID {
            store
                .put_mutable_overlay_value(current_key.clone(), b"current root advanced".to_vec())
                .unwrap();
        } else {
            let records = if descriptor.family_id == AUDIT_RETENTION_FAMILY_ID {
                let mut audit_map = BTreeMap::new();
                append_audit_record(
                    &mut audit_map,
                    store.digest_algo,
                    Some(WorkspaceId::from_bytes([17; 16])),
                    "t188-17.audit-retention-root-change",
                    Some(descriptor.name),
                )
                .unwrap();
                audit_retention_family_records(&audit_map)
            } else {
                let mut address = [0x17; 32];
                address[0..2].copy_from_slice(&descriptor.family_id.to_le_bytes());
                vec![(
                    address,
                    format!("{} root changed", descriptor.name).into_bytes(),
                )]
            };
            store
                .commit_family_root_records_for_test(descriptor.family_id, &records)
                .unwrap();
        }
        let advanced_roots = t188_15_roots(&store);
        let advanced_family_root = if descriptor.family_id == CURRENT_RECORDS_FAMILY_ID {
            advanced_roots.current_record_root
        } else {
            t188_15_catalog_family_root(advanced_roots, descriptor.family_id)
        };
        assert!(advanced_family_root.is_some(), "{}", descriptor.name);

        store.complete_reachability_mark_epoch(&epoch).unwrap();

        let active = store.active_reachability_mark_epoch().unwrap().unwrap();
        let reclaim_evidence = store
            .active_reachability_mark_reclaim_evidence()
            .unwrap()
            .unwrap();
        assert_eq!(active, epoch, "{}", descriptor.name);
        assert_eq!(
            active.canonical_roots_fingerprint, captured_fingerprint,
            "{}",
            descriptor.name
        );
        assert!(
            reclaim_evidence.matches_epoch(&active, store.digest_algo),
            "{}",
            descriptor.name
        );
        let completed_roots = t188_15_roots(&store);
        let completed_family_root = if descriptor.family_id == CURRENT_RECORDS_FAMILY_ID {
            completed_roots.current_record_root
        } else {
            t188_15_catalog_family_root(completed_roots, descriptor.family_id)
        };
        assert_eq!(
            completed_family_root, advanced_family_root,
            "{}",
            descriptor.name
        );
        let current_evidence = store.gc_reclaim_evidence_for_test().unwrap();
        assert_eq!(
            t188_17_family_root(&current_evidence, descriptor.family_id).page_root,
            advanced_family_root,
            "{}",
            descriptor.name
        );
        if descriptor.family_id == CURRENT_RECORDS_FAMILY_ID {
            assert_eq!(
                store
                    .mutable_overlay_current_entry(&current_key)
                    .unwrap()
                    .unwrap()
                    .payload,
                b"current root advanced",
                "{}",
                descriptor.name
            );
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum T18818aDisposition {
    CallerSuppliedPageRoot,
    CallerSuppliedDigestRoot,
    FinishTxnRebuiltPhysicalRoot,
    FinishTxnRegionTableRoot,
    CatalogAuthoritativePageRoot,
    CatalogAdvisoryPageRoot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct T18818aRootDisposition {
    root: &'static str,
    family_id: Option<u16>,
    role: &'static str,
    disposition: T18818aDisposition,
    input: &'static str,
    output: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum T18818aCallerMode {
    Canonical,
    LegacyTestOnly,
    PhysicalRewrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct T18818aCallerInventory {
    caller: &'static str,
    mode: T18818aCallerMode,
}

fn t188_18a_named_root_inputs() -> [&'static str; 6] {
    [
        "object_index",
        "legacy_overlay",
        "current_records",
        "root_catalog",
        "reference",
        "control",
    ]
}

fn t188_18a_root_rewrite_contract() -> Vec<T18818aRootDisposition> {
    let mut rows = vec![
        T18818aRootDisposition {
            root: "object_index",
            family_id: None,
            role: "object_index",
            disposition: T18818aDisposition::CallerSuppliedPageRoot,
            input: "TxnRootInputs.object_index",
            output: "RegionTable.index_root",
        },
        T18818aRootDisposition {
            root: "reference",
            family_id: None,
            role: "reference",
            disposition: T18818aDisposition::CallerSuppliedDigestRoot,
            input: "TxnRootInputs.reference",
            output: "journal_and_superblock.reference",
        },
        T18818aRootDisposition {
            root: "control",
            family_id: None,
            role: "control",
            disposition: T18818aDisposition::CallerSuppliedDigestRoot,
            input: "TxnRootInputs.control",
            output: "journal_and_superblock.control",
        },
        T18818aRootDisposition {
            root: "current_records",
            family_id: Some(CURRENT_RECORDS_FAMILY_ID),
            role: "current",
            disposition: T18818aDisposition::CallerSuppliedPageRoot,
            input: "TxnRootInputs.current_records",
            output: "RegionTable.current_record_root",
        },
        T18818aRootDisposition {
            root: "root_catalog",
            family_id: None,
            role: "catalog",
            disposition: T18818aDisposition::CallerSuppliedPageRoot,
            input: "TxnRootInputs.root_catalog",
            output: "RegionTable.root_catalog_root",
        },
        T18818aRootDisposition {
            root: "free_map",
            family_id: None,
            role: "physical_safety",
            disposition: T18818aDisposition::FinishTxnRebuiltPhysicalRoot,
            input: "PageAllocator.snapshot_free",
            output: "RegionTable.freemap_root",
        },
        T18818aRootDisposition {
            root: "maintenance",
            family_id: None,
            role: "physical_safety",
            disposition: T18818aDisposition::FinishTxnRebuiltPhysicalRoot,
            input: "MaintenanceState::next",
            output: "RegionTable.maintenance_root",
        },
        T18818aRootDisposition {
            root: "region_table",
            family_id: None,
            role: "publication",
            disposition: T18818aDisposition::FinishTxnRegionTableRoot,
            input: "RegionTable",
            output: "journal_and_superblock.region_table",
        },
    ];
    for descriptor in ROOT_FAMILY_REGISTRY {
        if descriptor.family_id == CURRENT_RECORDS_FAMILY_ID {
            continue;
        }
        let disposition = if descriptor.flags == ROOT_FLAG_ADVISORY
            || descriptor.role == RootFamilyRole::RebuildableAdvisory
        {
            T18818aDisposition::CatalogAdvisoryPageRoot
        } else {
            T18818aDisposition::CatalogAuthoritativePageRoot
        };
        rows.push(T18818aRootDisposition {
            root: descriptor.name,
            family_id: Some(descriptor.family_id),
            role: root_family_role(descriptor),
            disposition,
            input: "RootCatalogEntry.root",
            output: "RootCatalogEntry.root",
        });
    }
    rows
}

fn t188_18a_caller_inventory() -> [T18818aCallerInventory; 12] {
    [
        T18818aCallerInventory {
            caller: "commit_workflow_owner_state_records",
            mode: T18818aCallerMode::Canonical,
        },
        T18818aCallerInventory {
            caller: "commit_mutable_overlay_records",
            mode: T18818aCallerMode::Canonical,
        },
        T18818aCallerInventory {
            caller: "commit_raw_overlay_records_for_test",
            mode: T18818aCallerMode::LegacyTestOnly,
        },
        T18818aCallerInventory {
            caller: "commit_family_root_records_for_test",
            mode: T18818aCallerMode::Canonical,
        },
        T18818aCallerInventory {
            caller: "commit_current_root_records_for_test",
            mode: T18818aCallerMode::Canonical,
        },
        T18818aCallerInventory {
            caller: "checkpoint_mutable_overlay_pages",
            mode: T18818aCallerMode::Canonical,
        },
        T18818aCallerInventory {
            caller: "commit_control_map_and_audit_retention_map",
            mode: T18818aCallerMode::Canonical,
        },
        T18818aCallerInventory {
            caller: "commit_control_map_and_audit_retention_delta",
            mode: T18818aCallerMode::Canonical,
        },
        T18818aCallerInventory {
            caller: "commit_txn",
            mode: T18818aCallerMode::Canonical,
        },
        T18818aCallerInventory {
            caller: "gc_segments_inner",
            mode: T18818aCallerMode::PhysicalRewrite,
        },
        T18818aCallerInventory {
            caller: "trim_tail_free_pages",
            mode: T18818aCallerMode::PhysicalRewrite,
        },
        T18818aCallerInventory {
            caller: "compact_tail_once_impl",
            mode: T18818aCallerMode::PhysicalRewrite,
        },
    ]
}

fn t188_18a_checkpoint_callers() -> [&'static str; 3] {
    ["rekey", "add_wrap", "remove_wrap"]
}

fn t188_18a_row<'a>(
    rows: &'a [T18818aRootDisposition],
    root: &str,
    family_id: Option<u16>,
) -> &'a T18818aRootDisposition {
    rows.iter()
        .find(|row| row.root == root && row.family_id == family_id)
        .unwrap_or_else(|| panic!("missing rewrite disposition for {root} {family_id:?}"))
}

#[test]
fn t188_18a_root_rewrite_contract_has_one_disposition_per_canonical_root() {
    let rows = t188_18a_root_rewrite_contract();
    let mut counts = BTreeMap::<(&str, Option<u16>), usize>::new();
    for row in &rows {
        *counts.entry((row.root, row.family_id)).or_default() += 1;
        assert!(!row.input.is_empty(), "{}", row.root);
        assert!(!row.output.is_empty(), "{}", row.root);
    }
    for ((root, family_id), count) in counts {
        assert_eq!(count, 1, "{root} {family_id:?}");
    }

    assert_eq!(
        t188_18a_row(&rows, "object_index", None).disposition,
        T18818aDisposition::CallerSuppliedPageRoot
    );
    assert_eq!(
        t188_18a_row(&rows, "reference", None).disposition,
        T18818aDisposition::CallerSuppliedDigestRoot
    );
    assert_eq!(
        t188_18a_row(&rows, "control", None).disposition,
        T18818aDisposition::CallerSuppliedDigestRoot
    );
    assert_eq!(
        t188_18a_row(&rows, "root_catalog", None).disposition,
        T18818aDisposition::CallerSuppliedPageRoot
    );
    assert_eq!(
        t188_18a_row(&rows, "free_map", None).disposition,
        T18818aDisposition::FinishTxnRebuiltPhysicalRoot
    );
    assert_eq!(
        t188_18a_row(&rows, "maintenance", None).disposition,
        T18818aDisposition::FinishTxnRebuiltPhysicalRoot
    );
    assert_eq!(
        t188_18a_row(&rows, "region_table", None).disposition,
        T18818aDisposition::FinishTxnRegionTableRoot
    );

    for descriptor in ROOT_FAMILY_REGISTRY {
        let row = t188_18a_row(&rows, descriptor.name, Some(descriptor.family_id));
        assert_eq!(
            row.role,
            root_family_role(descriptor),
            "{}",
            descriptor.name
        );
        let expected = if descriptor.family_id == CURRENT_RECORDS_FAMILY_ID {
            T18818aDisposition::CallerSuppliedPageRoot
        } else if descriptor.flags == ROOT_FLAG_ADVISORY
            || descriptor.role == RootFamilyRole::RebuildableAdvisory
        {
            T18818aDisposition::CatalogAdvisoryPageRoot
        } else {
            T18818aDisposition::CatalogAuthoritativePageRoot
        };
        assert_eq!(row.disposition, expected, "{}", descriptor.name);
    }
    assert_eq!(rows.len(), ROOT_FAMILY_REGISTRY.len() + 7);
}

#[test]
fn t188_18a_root_rewrite_contract_excludes_legacy_overlay_from_canonical_set() {
    let rows = t188_18a_root_rewrite_contract();
    assert_eq!(
        t188_18a_named_root_inputs()
            .into_iter()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "object_index",
            "legacy_overlay",
            "current_records",
            "root_catalog",
            "reference",
            "control"
        ])
    );
    assert!(rows.iter().all(|row| row.root != "legacy_overlay"));
    assert_eq!(
        rows.iter()
            .filter(|row| row.disposition == T18818aDisposition::FinishTxnRebuiltPhysicalRoot)
            .map(|row| row.root)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["free_map", "maintenance"])
    );
    assert_eq!(
        t188_18a_row(
            &rows,
            "delta_pack_candidates",
            Some(DELTA_PACK_CANDIDATE_FAMILY_ID)
        )
        .disposition,
        T18818aDisposition::CatalogAdvisoryPageRoot
    );
    assert!(
        ROOT_FAMILY_REGISTRY
            .iter()
            .filter(|descriptor| descriptor.gc_reachability
                == RootFamilyReachability::PhysicalSafetyRoot)
            .all(
                |descriptor| t188_18a_row(&rows, descriptor.name, Some(descriptor.family_id))
                    .disposition
                    == T18818aDisposition::CatalogAuthoritativePageRoot
            )
    );
}

#[test]
fn t188_18a_finish_txn_caller_inventory_has_prescribed_modes() {
    let callers = t188_18a_caller_inventory();
    let names = callers
        .iter()
        .map(|entry| entry.caller)
        .collect::<BTreeSet<_>>();
    assert_eq!(callers.len(), 12);
    assert_eq!(names.len(), 12);
    assert_eq!(
        callers
            .iter()
            .filter(|entry| entry.mode == T18818aCallerMode::LegacyTestOnly)
            .map(|entry| entry.caller)
            .collect::<Vec<_>>(),
        vec!["commit_raw_overlay_records_for_test"]
    );
    assert_eq!(
        callers
            .iter()
            .filter(|entry| entry.mode == T18818aCallerMode::PhysicalRewrite)
            .map(|entry| entry.caller)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "compact_tail_once_impl",
            "gc_segments_inner",
            "trim_tail_free_pages"
        ])
    );
    for expected in [
        "commit_workflow_owner_state_records",
        "commit_mutable_overlay_records",
        "commit_family_root_records_for_test",
        "commit_current_root_records_for_test",
        "checkpoint_mutable_overlay_pages",
        "commit_control_map_and_audit_retention_map",
        "commit_control_map_and_audit_retention_delta",
        "commit_txn",
    ] {
        assert!(
            callers
                .iter()
                .any(|entry| entry.caller == expected && entry.mode == T18818aCallerMode::Canonical),
            "{expected}"
        );
    }
}

#[test]
fn t188_18a_superblock_checkpoint_copies_published_root_set() {
    let callers = t188_18a_checkpoint_callers();
    assert_eq!(callers.into_iter().collect::<BTreeSet<_>>().len(), 3);
    assert_eq!(
        callers.into_iter().collect::<BTreeSet<_>>(),
        BTreeSet::from(["add_wrap", "rekey", "remove_wrap"])
    );
    for root in ["region_table", "reference", "control"] {
        assert!(
            t188_18a_root_rewrite_contract()
                .iter()
                .any(|row| row.output.contains(root)),
            "{root}"
        );
    }
}

fn t188_18b_assert_roots_match_result(snapshot: T188CanonicalRoots, roots: &TxnRoots) {
    assert_eq!(snapshot.generation, roots.generation);
    assert_eq!(snapshot.page_count, roots.page_count);
    assert_eq!(snapshot.region_table_root, Some(roots.region_table_root));
    assert_eq!(snapshot.index_root, roots.object_index);
    assert_eq!(snapshot.freemap_root, roots.freemap.map(|(root, _)| root));
    assert_eq!(snapshot.maintenance_root, Some(roots.maintenance_root));
    assert_eq!(snapshot.overlay_root, roots.legacy_overlay);
    assert_eq!(snapshot.current_record_root, roots.current_record_root);
    assert_eq!(snapshot.root_catalog_root, roots.root_catalog.root);
    assert_eq!(
        snapshot.reference_root,
        roots.reference.map(|bytes| Digest::of(Algo::Blake3, bytes))
    );
    assert_eq!(
        snapshot.control_root,
        roots.control.map(|bytes| Digest::of(Algo::Blake3, bytes))
    );
    let families = root_catalog_family_roots(&roots.root_catalog.entries);
    assert_eq!(snapshot.retained_history_root, families.retained_history);
    assert_eq!(snapshot.owner_token_root, families.owner_token);
    assert_eq!(snapshot.secondary_index_root, families.secondary_index);
    assert_eq!(
        snapshot.mutable_idempotency_root,
        families.mutable_idempotency
    );
    assert_eq!(
        snapshot.workflow_idempotency_root,
        families.workflow_idempotency
    );
    assert_eq!(snapshot.audit_retention_root, families.audit_retention);
    assert_eq!(snapshot.mvcc_generation_root, families.mvcc_generation);
    assert_eq!(snapshot.retention_index_root, families.retention_index);
    assert_eq!(snapshot.checkpoint_index_root, families.checkpoint_index);
    assert_eq!(snapshot.reclaim_index_root, families.reclaim_index);
    assert_eq!(
        snapshot.delta_pack_candidate_root,
        root_catalog_family_root(&roots.root_catalog.entries, DELTA_PACK_CANDIDATE_FAMILY_ID)
    );
}

fn t188_18b_populate_complete_roots(store: &FileStore, suffix: &str) {
    let current_key =
        durability_facet_test_key(b"documents", &format!("t188-18b-current-{suffix}"));
    t188_15_commit_workflow(
        store,
        &current_key,
        format!("t188-18b-current-{suffix}").as_bytes(),
        "t188.18b.current",
        None,
    );
    t188_15_mutate_owner_token(store, suffix);
    t188_15_populate_required_roots(store, suffix);
    store
        .control_set(
            format!("t188.18b.control.{suffix}").as_bytes(),
            format!("control-{suffix}").into_bytes(),
        )
        .unwrap();
}

fn diagnostic_root_record_loc(page: u64) -> RecordLoc {
    RecordLoc::from_global(page, 0)
}

fn write_diagnostic_root_btree(store: &FileStore, seed: u8, family_locator_codec: bool) -> PageId {
    write_diagnostic_root_btree_with_entries(store, seed, family_locator_codec, 1)
}

fn write_diagnostic_root_btree_with_entries(
    store: &FileStore,
    seed: u8,
    family_locator_codec: bool,
    count: u64,
) -> PageId {
    let (page_count, generation, free) = {
        let inner = store.inner.lock().unwrap();
        (inner.page_count, inner.generation, inner.free.clone())
    };
    let mut entries = (0..count)
        .map(|i| {
            let mut key = [seed; 32];
            key[0..8].copy_from_slice(&i.to_be_bytes());
            (key, diagnostic_root_record_loc(i + 1))
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.0);
    let mut alloc = PageAllocator::new(page_count, generation + 1, free);
    let root = {
        let mut file = store.file.lock().unwrap();
        pagebtree::build_packed_with_codec(
            &mut **file,
            DATA_START,
            &mut alloc,
            &entries,
            if family_locator_codec {
                pagebtree::ValueCodecKind::PackedRecordRef
            } else {
                pagebtree::ValueCodecKind::RecordLoc
            },
        )
        .unwrap()
        .expect("non-empty diagnostic root")
    };
    let mut inner = store.inner.lock().unwrap();
    inner.page_count = alloc.page_count();
    inner.maintenance.physical_page_count = alloc.page_count();
    root
}

fn install_diagnostic_root(
    store: &FileStore,
    family_id: Option<u16>,
    family_locator_codec: bool,
) -> PageId {
    let seed = family_id.map(|family_id| family_id as u8).unwrap_or(0xee);
    let root = write_diagnostic_root_btree(store, seed, family_locator_codec);
    let mut inner = store.inner.lock().unwrap();
    match family_id {
        None => inner.index_root = Some(root),
        Some(CURRENT_RECORDS_FAMILY_ID) => inner.current_record_root = Some(root),
        Some(RETAINED_HISTORY_FAMILY_ID) => inner.retained_history_root = Some(root),
        Some(OWNER_TOKEN_FAMILY_ID) => inner.owner_token_root = Some(root),
        Some(SECONDARY_INDEX_FAMILY_ID) => inner.secondary_index_root = Some(root),
        Some(MUTABLE_IDEMPOTENCY_FAMILY_ID) => inner.mutable_idempotency_root = Some(root),
        Some(WORKFLOW_IDEMPOTENCY_FAMILY_ID) => inner.workflow_idempotency_root = Some(root),
        Some(AUDIT_RETENTION_FAMILY_ID) => inner.audit_retention_root = Some(root),
        Some(_) => {}
    }
    if let Some(family_id) = family_id.filter(|id| *id != CURRENT_RECORDS_FAMILY_ID) {
        inner.root_catalog_entries =
            root_catalog_entries_with_family(&inner.root_catalog_entries, family_id, Some(root));
    }
    root
}

fn rewrite_diagnostic_root_page(store: &FileStore, root: PageId, edit: impl FnOnce(&mut [u8])) {
    let mut page = [0u8; PAGE_SIZE as usize];
    let mut file = store.file.lock().unwrap();
    read_exact_at(&mut **file, root.offset(DATA_START), &mut page).unwrap();
    edit(&mut page);
    write_at(&mut **file, root.offset(DATA_START), &page).unwrap();
}

fn corrupt_diagnostic_descendant_codec(store: &FileStore, root: PageId) -> PageId {
    let page_count = store.inner.lock().unwrap().page_count;
    let descendant = {
        let mut file = store.file.lock().unwrap();
        pagebtree::collect_pages_with_codec(
            &mut **file,
            DATA_START,
            root,
            page_count,
            pagebtree::ValueCodecKind::PackedRecordRef,
        )
        .unwrap()
        .into_iter()
        .find(|page| *page != root)
        .expect("diagnostic descendant")
    };
    rewrite_diagnostic_root_page(store, descendant, |page| {
        page[1] = (page[1] & !0xF0) | pagebtree::ValueCodecKind::RecordLoc.discriminator();
        let crc = crc32c(&page[..PAGE_SIZE as usize - 4]);
        page[PAGE_SIZE as usize - 4..].copy_from_slice(&crc.to_le_bytes());
    });
    descendant
}

#[test]
fn root_codec_diagnostics_accept_empty_and_healthy_roots() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let empty = store.root_codec_diagnostics().unwrap();
    assert_eq!(empty.checked_roots, 0);
    assert!(empty.failures.is_empty());

    install_diagnostic_root(&store, None, false);
    install_diagnostic_root(&store, Some(CURRENT_RECORDS_FAMILY_ID), false);
    install_diagnostic_root(&store, Some(RETAINED_HISTORY_FAMILY_ID), true);
    install_diagnostic_root(&store, Some(OWNER_TOKEN_FAMILY_ID), true);
    install_diagnostic_root(&store, Some(SECONDARY_INDEX_FAMILY_ID), true);
    install_diagnostic_root(&store, Some(MUTABLE_IDEMPOTENCY_FAMILY_ID), false);
    install_diagnostic_root(&store, Some(WORKFLOW_IDEMPOTENCY_FAMILY_ID), true);
    install_diagnostic_root(&store, Some(AUDIT_RETENTION_FAMILY_ID), false);

    let diagnostics = store.root_codec_diagnostics().unwrap();
    assert_eq!(diagnostics.checked_roots, 8);
    assert!(diagnostics.failures.is_empty());
    assert!(
        diagnostics
            .details
            .iter()
            .any(|detail| detail.expected_codec == "RecordLocCodec")
    );
    assert!(
        diagnostics
            .details
            .iter()
            .any(|detail| detail.expected_codec == "PackedRecordRefCodec")
    );
}

#[test]
fn root_codec_diagnostics_accepts_production_publication_after_reopen() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    t188_18b_populate_complete_roots(&store, "root-codec-production");
    let before = store.root_codec_diagnostics().unwrap();
    assert!(
        before.failures.is_empty(),
        "production publication wrote codec-invalid roots: {:?}",
        before.failures
    );
    assert!(
        before
            .details
            .iter()
            .any(|detail| detail.expected_codec == "PackedRecordRefCodec")
    );
    assert!(
        before
            .details
            .iter()
            .any(|detail| detail.expected_codec == "RecordLocCodec")
    );

    drop(store);
    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let after = reopened.root_codec_diagnostics().unwrap();
    assert_eq!(after, before);
}

#[test]
fn packed_root_families_preserve_codec_across_update_mark_compaction_and_reopen() {
    let tp = TempPath::new("packed-root-family-production-paths");
    let mut store = FileStore::open(tp.path()).unwrap();
    let family_ids = [
        RETAINED_HISTORY_FAMILY_ID,
        OWNER_TOKEN_FAMILY_ID,
        SECONDARY_INDEX_FAMILY_ID,
        WORKFLOW_IDEMPOTENCY_FAMILY_ID,
    ];
    let mut expected = BTreeMap::new();
    for family_id in family_ids {
        // Sixty-five entries exceed the PackedRecordRef leaf capacity and force a multi-level tree.
        let records = (0..65u64)
            .map(|index| {
                let mut address = [family_id as u8; 32];
                address[..8].copy_from_slice(&index.to_be_bytes());
                let value = format!("family-{family_id:04x}-value-{index:02}").into_bytes();
                (address, value)
            })
            .collect::<Vec<_>>();
        expected.insert(family_id, records[17].clone());
        store
            .commit_family_root_records_for_test(family_id, &records)
            .unwrap();
    }

    let replacement_targets = expected
        .iter()
        .map(|(family_id, (address, _))| (*family_id, *address))
        .collect::<Vec<_>>();
    for (family_id, address) in replacement_targets {
        let replacement = format!("family-{family_id:04x}-replacement").into_bytes();
        store
            .commit_family_root_records_for_test(family_id, &[(address, replacement.clone())])
            .unwrap();
        expected.get_mut(&family_id).unwrap().1 = replacement;
    }

    let assert_families = |store: &FileStore| {
        let (page_count, catalog_roots) = {
            let inner = store.inner.lock().unwrap();
            (
                inner.page_count,
                inner
                    .root_catalog_entries
                    .iter()
                    .map(|entry| (entry.family_id, entry.root))
                    .collect::<BTreeMap<_, _>>(),
            )
        };
        let mut file = store.file.lock().unwrap();
        for (family_id, (address, expected_bytes)) in &expected {
            let root = catalog_roots[family_id];
            assert!(
                pagebtree::tree_depth_with_codec(
                    &mut **file,
                    DATA_START,
                    root,
                    page_count,
                    pagebtree::ValueCodecKind::PackedRecordRef,
                )
                .unwrap()
                    > 1
            );
            let pages =
                root_family_collect_pages(&mut **file, *family_id, root, page_count).unwrap();
            assert!(pages.len() > 1);
            for page in &pages {
                let mut raw = [0u8; PAGE_SIZE as usize];
                read_exact_at(&mut **file, page.offset(DATA_START), &mut raw).unwrap();
                assert_eq!(
                    raw[1] & 0xF0,
                    pagebtree::ValueCodecKind::PackedRecordRef.discriminator()
                );
            }
            let loc = root_family_get(&mut **file, *family_id, Some(root), address, page_count)
                .unwrap()
                .unwrap();
            assert_eq!(
                read_blob_from_loc(&mut **file, loc).unwrap(),
                *expected_bytes
            );
        }
    };

    assert_families(&store);
    assert!(store.root_codec_diagnostics().unwrap().failures.is_empty());
    store.root_storage_attribution(4).unwrap();
    let state = loom_core::ReachabilityMarkState {
        pinned: BTreeSet::new(),
        marked: BTreeSet::new(),
        queue: VecDeque::new(),
        stream_roots: VecDeque::new(),
        content_roots: VecDeque::new(),
        prolly_cursors: VecDeque::new(),
        completed: true,
    };
    let mut epoch = store
        .begin_reachability_mark_epoch(None, BTreeSet::new(), state)
        .unwrap();
    while !epoch.metadata_completed {
        store
            .step_reachability_metadata_mark_epoch(&mut epoch, 64, None)
            .unwrap();
    }
    store.complete_reachability_mark_epoch(&epoch).unwrap();
    assert_eq!(
        store
            .active_reachability_mark_epoch()
            .unwrap()
            .unwrap()
            .epoch,
        epoch.epoch
    );
    assert_eq!(
        store
            .maintenance_status()
            .unwrap()
            .last_validated_mark_epoch,
        epoch.epoch
    );
    let gc = store
        .gc_validated_segments(GcSegmentBudget {
            max_segments: 1,
            max_pages: 64,
        })
        .unwrap();
    assert!(gc.segments_reclaimed <= 1);
    assert!(gc.pages_freed <= 64);
    assert!(store.active_reachability_mark_epoch().unwrap().is_none());
    assert_families(&store);

    store.compact().unwrap();
    assert_families(&store);
    assert!(store.root_codec_diagnostics().unwrap().failures.is_empty());
    drop(store);

    let reopened = FileStore::open(tp.path()).unwrap();
    assert_families(&reopened);
    assert!(
        reopened
            .root_codec_diagnostics()
            .unwrap()
            .failures
            .is_empty()
    );
}

#[test]
fn root_codec_diagnostics_reports_mismatched_family_codec() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    install_diagnostic_root(&store, Some(RETAINED_HISTORY_FAMILY_ID), false);

    let diagnostics = store.root_codec_diagnostics().unwrap();
    assert_eq!(diagnostics.checked_roots, 1);
    assert_eq!(diagnostics.failures.len(), 1);
    let failure = &diagnostics.failures[0];
    assert_eq!(failure.root_name, "retained_history");
    assert_eq!(failure.family_id, Some(RETAINED_HISTORY_FAMILY_ID));
    assert_eq!(failure.expected_codec, "PackedRecordRefCodec");
    assert_eq!(failure.expected_discriminator, 0x10);
    assert_eq!(failure.actual_discriminator, Some(0x00));
    assert_eq!(
        failure.failure,
        Some("btree_node_codec_discriminator_mismatch")
    );
}

#[test]
fn root_codec_diagnostics_rejects_unknown_family() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let root = install_diagnostic_root(&store, None, false);
    {
        let mut inner = store.inner.lock().unwrap();
        inner.index_root = None;
        inner
            .root_catalog_entries
            .push(RootCatalogEntry::authoritative(0x7777, root));
    }

    let diagnostics = store.root_codec_diagnostics().unwrap();
    assert_eq!(diagnostics.checked_roots, 1);
    assert_eq!(diagnostics.failures.len(), 1);
    let failure = &diagnostics.failures[0];
    assert_eq!(failure.root_name, "unknown_family");
    assert_eq!(failure.family_id, Some(0x7777));
    assert_eq!(failure.failure, Some("unknown_root_family"));
}

#[test]
fn root_codec_diagnostics_reports_out_of_range_root() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    {
        let mut inner = store.inner.lock().unwrap();
        inner.root_catalog_entries = root_catalog_entries_with_family(
            &inner.root_catalog_entries,
            RETAINED_HISTORY_FAMILY_ID,
            Some(PageId(inner.page_count + 10)),
        );
    }

    let diagnostics = store.root_codec_diagnostics().unwrap();
    assert_eq!(diagnostics.failures.len(), 1);
    let failure = &diagnostics.failures[0];
    assert_eq!(failure.root_page, 10);
    assert!(!failure.in_range);
    assert_eq!(failure.failure, Some("root_page_out_of_range"));
}

#[test]
fn root_codec_diagnostics_reports_checksum_and_magic_failures() {
    let checksum_store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let checksum_root =
        install_diagnostic_root(&checksum_store, Some(RETAINED_HISTORY_FAMILY_ID), true);
    rewrite_diagnostic_root_page(&checksum_store, checksum_root, |page| page[8] ^= 0x55);
    let checksum = checksum_store.root_codec_diagnostics().unwrap();
    assert_eq!(checksum.failures.len(), 1);
    assert_eq!(
        checksum.failures[0].failure,
        Some("btree_node_crc_mismatch")
    );

    let magic_store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let magic_root = install_diagnostic_root(&magic_store, Some(RETAINED_HISTORY_FAMILY_ID), true);
    rewrite_diagnostic_root_page(&magic_store, magic_root, |page| {
        page[0] = 0x42;
        let crc = crc32c(&page[..PAGE_SIZE as usize - 4]);
        page[PAGE_SIZE as usize - 4..].copy_from_slice(&crc.to_le_bytes());
    });
    let magic = magic_store.root_codec_diagnostics().unwrap();
    assert_eq!(magic.failures.len(), 1);
    assert_eq!(magic.failures[0].failure, Some("bad_btree_node_magic"));
}

#[test]
fn root_codec_diagnostics_rejects_correct_root_with_mismatched_descendant() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let root = write_diagnostic_root_btree_with_entries(&store, 0x52, true, 65);
    {
        let mut inner = store.inner.lock().unwrap();
        inner.root_catalog_entries = root_catalog_entries_with_family(
            &inner.root_catalog_entries,
            RETAINED_HISTORY_FAMILY_ID,
            Some(root),
        );
    }
    let descendant = corrupt_diagnostic_descendant_codec(&store, root);

    let diagnostics = store.root_codec_diagnostics().unwrap();
    assert_eq!(diagnostics.failures.len(), 1);
    let failure = &diagnostics.failures[0];
    assert_eq!(failure.root_name, "retained_history");
    assert_eq!(failure.root_page, descendant.0);
    assert_eq!(failure.expected_codec, "PackedRecordRefCodec");
    assert_eq!(failure.actual_discriminator, Some(0x00));
    assert_eq!(
        failure.failure,
        Some("btree_node_codec_discriminator_mismatch")
    );
}

#[test]
fn t188_18b_complete_adoption_updates_every_committed_root() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    t188_18b_populate_complete_roots(&store, "complete");
    let live = t188_15_roots(&store);
    assert!(live.region_table_root.is_some());
    assert!(live.index_root.is_some());
    assert!(live.maintenance_root.is_some());
    assert!(live.current_record_root.is_some());
    assert!(live.root_catalog_root.is_some());
    assert!(live.reference_root.is_some());
    assert!(live.control_root.is_some());
    assert!(live.retained_history_root.is_some());
    assert!(live.owner_token_root.is_some());
    assert!(live.secondary_index_root.is_some());
    assert!(live.mutable_idempotency_root.is_some());
    assert!(live.workflow_idempotency_root.is_some());
    assert!(live.audit_retention_root.is_some());
    assert!(live.mvcc_generation_root.is_some());
    assert!(live.retention_index_root.is_some());
    assert!(live.checkpoint_index_root.is_some());
    assert!(live.reclaim_index_root.is_some());
    assert!(live.delta_pack_candidate_root.is_some());
    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    assert_eq!(t188_15_roots(&reopened), live);
}

#[test]
fn t188_18b_pre_commit_interruption_preserves_old_complete_root_set() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    t188_18b_populate_complete_roots(&store, "old");
    let before = t188_15_roots(&store);
    drop(store);

    let failing = FailNthFsyncMem::new(shared.clone(), 2);
    let store = FileStore::with_backing(Box::new(failing), true).unwrap();
    let failed = store.put(&blob(b"t188-18b-failed-before-commit"));
    assert!(failed.is_err());
    assert_eq!(t188_15_roots(&store), before);
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    assert_eq!(t188_15_roots(&reopened), before);
}

#[test]
fn checkpoint_fsync_failure_reopens_complete_committed_successor() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    // At most one checkpoint interval is needed to place the next commit on the checkpoint edge.
    while !(store.generation() + 1).is_multiple_of(CHECKPOINT_INTERVAL) {
        let generation = store.generation();
        store
            .put(format!("checkpoint-boundary-seed-{generation}").as_bytes())
            .unwrap();
    }
    let prior_generation = store.generation();
    let prior_roots = t188_15_roots(&store);
    drop(store);

    let payload = blob(b"checkpoint-fsync-complete-successor");
    let digest = Digest::hash(Algo::Blake3, &payload);
    let expected_backing = SharedMem::default();
    expected_backing.mutate_bytes(|bytes| *bytes = shared.bytes());
    let expected_store =
        FileStore::with_backing(Box::new(expected_backing.clone()), false).unwrap();
    assert_eq!(expected_store.put(&payload).unwrap(), digest);
    let expected_roots = t188_15_roots(&expected_store);
    let expected_free = expected_store.free_runs();
    let expected_status = expected_store.maintenance_status().unwrap();
    drop(expected_store);

    let failing =
        FileStore::with_backing(Box::new(FailNthFsyncMem::new(shared.clone(), 3)), true).unwrap();
    let error = failing.put(&payload).unwrap_err();
    assert_eq!(error.code, Code::Io);
    drop(failing);

    let reopened = FileStore::with_backing(Box::new(shared), false).unwrap();
    let successor = t188_15_roots(&reopened);
    assert_eq!(successor.generation, prior_generation + 1);
    assert_ne!(successor, prior_roots);
    assert_eq!(successor, expected_roots);
    assert_eq!(reopened.free_runs(), expected_free);
    assert_eq!(
        reopened.get(&digest).unwrap().as_deref(),
        Some(payload.as_slice())
    );
    let reopened_status = reopened.maintenance_status().unwrap();
    assert_eq!(reopened_status.generation, expected_status.generation);
    assert_eq!(reopened_status.object_count, expected_status.object_count);
    assert_eq!(
        reopened_status.physical_page_count,
        expected_status.physical_page_count
    );
    let free = reopened.free_runs();
    for root in [
        successor.index_root,
        successor.freemap_root,
        successor.region_table_root,
        successor.maintenance_root,
        successor.current_record_root,
        successor.root_catalog_root,
    ]
    .into_iter()
    .flatten()
    {
        assert!(
            !free
                .iter()
                .any(|run| root.0 >= run.start && root.0 < run.start.saturating_add(run.len)),
            "committed successor root {} is listed as free",
            root.0
        );
    }
}

#[test]
fn t188_18b_post_commit_pre_adopt_reopen_observes_complete_new_roots() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    t188_18b_populate_complete_roots(&store, "before-hook");
    let before = t188_15_roots(&store);
    let hook_shared = shared.clone();
    store
        .set_post_commit_pre_adopt_hook_for_test(Box::new(move |roots| {
            let reopened = FileStore::with_backing(Box::new(hook_shared), true).unwrap();
            let reopened_roots = t188_15_roots(&reopened);
            t188_18b_assert_roots_match_result(reopened_roots, roots);
            assert_ne!(reopened_roots, before);
            Ok(())
        }))
        .unwrap();
    t188_15_mutate_retained_history(&store, "after-hook");
    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    assert_eq!(t188_15_roots(&reopened), t188_15_roots(&store));
}

fn t188_18b_finish_with_root_inputs(root_inputs: TxnRootInputs) -> Result<()> {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared), true).unwrap();
    let inner = store.inner.lock().unwrap();
    let mut file = store.file.lock().unwrap();
    let mut alloc = PageAllocator::new_with_current_free_reusable(
        inner.page_count,
        inner.generation + 1,
        inner.free.clone(),
    );
    finish_txn(
        &mut **file,
        &mut alloc,
        inner.generation + 1,
        inner.maintenance.object_count,
        root_inputs,
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
    .map(|_| ())
}

#[test]
fn t188_18b_malformed_root_catalog_publication_rejects_before_adoption() {
    let entry = RootCatalogEntry::authoritative(RETAINED_HISTORY_FAMILY_ID, PageId(2));
    assert!(
        t188_18b_finish_with_root_inputs(TxnRootInputs {
            object_index: None,
            legacy_overlay: None,
            current_records: Some(PageId(3)),
            root_catalog: TxnRootCatalog {
                root: None,
                entries: vec![entry],
            },
            reference: None,
            control: None,
            previous_mutable_overlay_generation_floor: 0,
            mutable_overlay_generation_floor: 0,
        })
        .unwrap_err()
        .message
        .contains("entries without root")
    );
    assert!(
        t188_18b_finish_with_root_inputs(TxnRootInputs {
            object_index: None,
            legacy_overlay: None,
            current_records: Some(PageId(3)),
            root_catalog: TxnRootCatalog {
                root: Some(PageId(4)),
                entries: Vec::new(),
            },
            reference: None,
            control: None,
            previous_mutable_overlay_generation_floor: 0,
            mutable_overlay_generation_floor: 0,
        })
        .unwrap_err()
        .message
        .contains("root without entries")
    );
    assert!(
        t188_18b_finish_with_root_inputs(TxnRootInputs {
            object_index: None,
            legacy_overlay: None,
            current_records: Some(PageId(3)),
            root_catalog: TxnRootCatalog {
                root: Some(PageId(4)),
                entries: vec![entry, entry],
            },
            reference: None,
            control: None,
            previous_mutable_overlay_generation_floor: 0,
            mutable_overlay_generation_floor: 0,
        })
        .is_err()
    );
}

#[test]
fn t188_18b_reference_and_control_set_and_clear_match_reopen() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let reference = store.put(&blob(b"t188-18b-reference")).unwrap();
    store.set_reference_root(Some(reference)).unwrap();
    store
        .control_set(b"t188-18b-control", b"value".to_vec())
        .unwrap();
    let set_roots = t188_15_roots(&store);
    assert_eq!(set_roots.reference_root, Some(reference));
    assert!(set_roots.control_root.is_some());
    let reopened = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    assert_eq!(t188_15_roots(&reopened), set_roots);
    drop(reopened);

    store.set_reference_root(None).unwrap();
    store.set_control_root(None).unwrap();
    let cleared_roots = t188_15_roots(&store);
    assert_eq!(cleared_roots.reference_root, None);
    assert_eq!(cleared_roots.control_root, None);
    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    assert_eq!(t188_15_roots(&reopened), cleared_roots);
}

#[test]
fn t188_18b_compaction_preserves_logical_family_reference_and_control_roots() {
    let shared = SharedMem::default();
    let mut store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    t188_18b_populate_complete_roots(&store, "compact");
    for index in 0..8 {
        store
            .put(&blob(format!("t188-18b-compact-object-{index}").as_bytes()))
            .unwrap();
    }
    let before = t188_15_roots(&store);
    let _ = store.compact_tail_once(64, 64, 1024 * 1024).unwrap();
    let after = t188_15_roots(&store);
    assert_eq!(after.reference_root, before.reference_root);
    assert_eq!(after.control_root, before.control_root);
    assert_eq!(after.retained_history_root, before.retained_history_root);
    assert_eq!(after.owner_token_root, before.owner_token_root);
    assert_eq!(after.secondary_index_root, before.secondary_index_root);
    assert_eq!(
        after.mutable_idempotency_root,
        before.mutable_idempotency_root
    );
    assert_eq!(
        after.workflow_idempotency_root,
        before.workflow_idempotency_root
    );
    assert_eq!(after.audit_retention_root, before.audit_retention_root);
    assert_eq!(after.mvcc_generation_root, before.mvcc_generation_root);
    assert_eq!(after.retention_index_root, before.retention_index_root);
    assert_eq!(after.checkpoint_index_root, before.checkpoint_index_root);
    assert_eq!(after.reclaim_index_root, before.reclaim_index_root);
    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    assert_eq!(t188_15_roots(&reopened), after);
}

#[test]
fn t188_18b_all_finish_txn_callers_adopt_complete_roots_once() {
    let lib = include_str!("lib.rs");
    let compact = include_str!("compact.rs");
    let callers = t188_18a_caller_inventory();
    for caller in callers {
        let source = if caller.mode == T18818aCallerMode::PhysicalRewrite {
            compact
        } else {
            lib
        };
        let signature = format!("fn {}", caller.caller);
        let start = source.find(&signature).unwrap_or_else(|| {
            panic!("missing caller {}", caller.caller);
        });
        let rest = &source[start..];
        let end = ["\n    fn ", "\n    pub fn ", "\n    pub(crate) fn "]
            .into_iter()
            .filter_map(|marker| rest[1..].find(marker).map(|index| index + 1))
            .min()
            .unwrap_or(rest.len());
        let body = &rest[..end];
        assert_eq!(body.matches("finish_txn(").count(), 1, "{}", caller.caller);
        assert_eq!(
            body.matches("adopt_committed_roots_locked").count(),
            1,
            "{}",
            caller.caller
        );
        for assignment in [
            "inner.generation =",
            "inner.page_count = roots.page_count",
            "inner.index_root =",
            "inner.overlay_root =",
            "inner.current_record_root =",
            "inner.root_catalog_root =",
            "inner.root_catalog_entries =",
            "inner.reference_root =",
            "inner.control_root =",
            "inner.free = roots.free",
            "inner.freemap = roots.freemap",
            "inner.region_table_root =",
            "inner.maintenance_root =",
            "inner.maintenance = roots.maintenance",
        ] {
            assert!(!body.contains(assignment), "{} {assignment}", caller.caller);
        }
    }
}

fn t188_19a_root_classification(descriptor: &RootFamilyDescriptor) -> GcCompactionClassification {
    if descriptor.gc_reachability == RootFamilyReachability::AdvisoryPreserveOnly {
        GcCompactionClassification::AdvisoryPreservation
    } else if matches!(
        descriptor.gc_reachability,
        RootFamilyReachability::SemanticRoot | RootFamilyReachability::ControlRoot
    ) {
        GcCompactionClassification::SemanticLiveness
    } else {
        GcCompactionClassification::PhysicalSafety
    }
}

fn t188_19a_record_page(store: &FileStore, digest: Digest) -> u64 {
    store
        .inner
        .lock()
        .unwrap()
        .index
        .get(digest.bytes())
        .unwrap_or_else(|| panic!("missing test digest {digest}"))
        .global_page()
}

fn t188_19_free_pages(store: &FileStore) -> BTreeSet<u64> {
    store
        .inner
        .lock()
        .unwrap()
        .free
        .iter()
        .flat_map(|run| run.start..run.start + run.len)
        .collect()
}

fn t188_19_free_page_count(store: &FileStore) -> u64 {
    store
        .inner
        .lock()
        .unwrap()
        .free
        .iter()
        .map(|run| run.len)
        .sum()
}

fn t188_19_root_pages(roots: &T188CanonicalRoots) -> BTreeSet<u64> {
    [
        roots.region_table_root,
        roots.index_root,
        roots.freemap_root,
        roots.maintenance_root,
        roots.overlay_root,
        roots.current_record_root,
        roots.root_catalog_root,
        roots.retained_history_root,
        roots.audit_retention_root,
        roots.owner_token_root,
        roots.secondary_index_root,
        roots.mutable_idempotency_root,
        roots.workflow_idempotency_root,
        roots.mvcc_generation_root,
        roots.retention_index_root,
        roots.checkpoint_index_root,
        roots.reclaim_index_root,
        roots.delta_pack_candidate_root,
    ]
    .into_iter()
    .flatten()
    .map(|page| page.0)
    .collect()
}

fn t188_19_put_until_free_page_reused(
    store: &FileStore,
    free_pages: &BTreeSet<u64>,
    tag: &str,
) -> Digest {
    for generation in 0..=REUSE_SAFE_WINDOW + 4 {
        let mut payload = vec![0xE7; 96 * 1024];
        payload[..tag.len()].copy_from_slice(tag.as_bytes());
        payload[tag.len()..tag.len() + 8].copy_from_slice(&generation.to_le_bytes());
        let digest = store.put(&payload).unwrap();
        if free_pages.contains(&t188_19a_record_page(store, digest)) {
            return digest;
        }
    }
    panic!("no reclaimed page reused for {tag}");
}

fn hex_digest(digest: Digest) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in digest.bytes() {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn t188_19_live_index_set(store: &FileStore) -> BTreeSet<[u8; 32]> {
    store.inner.lock().unwrap().index.keys().copied().collect()
}

#[test]
fn t188_19a_canonical_plan_classifies_every_root_family_without_rewriting() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared), true).unwrap();
    t188_18b_populate_complete_roots(&store, "t188-19a-classify");
    let before = t188_15_roots(&store);

    let plan = store
        .canonical_compaction_plan(&BTreeSet::new(), GcSegmentBudget::unlimited())
        .unwrap();
    let after = t188_15_roots(&store);
    assert_eq!(after, before);

    for (name, classification) in [
        (
            "object_index_records",
            GcCompactionClassification::PhysicalSafety,
        ),
        ("root_catalog", GcCompactionClassification::PhysicalSafety),
        ("free_map", GcCompactionClassification::PhysicalSafety),
        ("maintenance", GcCompactionClassification::PhysicalSafety),
        (
            "reference_root",
            GcCompactionClassification::SemanticLiveness,
        ),
        ("control_root", GcCompactionClassification::SemanticLiveness),
    ] {
        let root = plan
            .roots
            .iter()
            .find(|root| root.name == name)
            .unwrap_or_else(|| panic!("missing root plan for {name}"));
        assert_eq!(root.classification, classification, "{name}");
    }

    for descriptor in ROOT_FAMILY_REGISTRY {
        let root = plan
            .roots
            .iter()
            .find(|root| root.family_id == Some(descriptor.family_id))
            .unwrap_or_else(|| panic!("missing family plan for {}", descriptor.name));
        assert_eq!(root.name, descriptor.name);
        assert_eq!(
            root.classification,
            t188_19a_root_classification(descriptor),
            "{}",
            descriptor.name
        );
    }
    assert_eq!(
        plan.roots.len(),
        ROOT_FAMILY_REGISTRY.len() + 6,
        "each direct root plus each registered family must be represented once"
    );
    assert!(plan.blocked_pages > 0);
}

#[test]
fn t188_19a_canonical_plan_rejects_stale_root_evidence() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    t188_18b_populate_complete_roots(&store, "t188-19a-stale-before");
    let evidence = store.gc_reclaim_evidence_for_test().unwrap();
    t188_15_mutate_owner_token(&store, "t188-19a-stale-after");

    let err = store
        .canonical_compaction_plan_from_evidence_for_test(
            &evidence,
            &BTreeSet::new(),
            GcSegmentBudget::unlimited(),
        )
        .unwrap_err();
    assert_eq!(err.code, Code::Conflict);
    assert!(
        err.to_string()
            .contains("canonical compaction evidence is stale")
    );
}

#[test]
fn t188_19a_canonical_plan_separates_blocked_and_eligible_candidates() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let live = store.put(&vec![0x19; 96 * 1024]).unwrap();
    store.set_reference_root(Some(live)).unwrap();
    let dead = store.put(&vec![0x91; 96 * 1024]).unwrap();
    let plan = store
        .canonical_compaction_plan(&BTreeSet::new(), GcSegmentBudget::unlimited())
        .unwrap();

    assert!(plan.eligible_pages > 0);
    assert!(plan.blocked_pages > 0);
    assert!(
        plan.page_candidates
            .iter()
            .any(|candidate| candidate.classification
                == GcCompactionClassification::SemanticLiveness
                && candidate.blocker.as_deref() == Some("semantic_liveness"))
    );
    assert!(
        plan.page_candidates
            .iter()
            .any(|candidate| candidate.classification
                == GcCompactionClassification::PhysicalSafety
                && candidate.blocker.as_deref() == Some("physical_safety"))
    );
    assert!(
        plan.page_candidates
            .iter()
            .any(|candidate| candidate.eligible
                && candidate.classification == GcCompactionClassification::ReclaimNeutral)
    );
    assert!(store.has(&live).unwrap());
    assert!(store.has(&dead).unwrap());
}

#[test]
fn t188_19a_advisory_state_is_preserved_without_semantic_liveness() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    store
        .commit_family_root_records_for_test(
            DELTA_PACK_CANDIDATE_FAMILY_ID,
            &[delta_pack_advisory_family_record(
                b"t188-19a-advisory",
                DeltaPackAdvisoryKind::Candidate,
                OverlayGeneration::new(19),
                Some(Digest::blake3(b"t188-19a-advisory-source")),
                7,
                false,
            )],
        )
        .unwrap();
    let advisory_only = store.put(&vec![0xA7; 96 * 1024]).unwrap();

    let plan = store
        .canonical_compaction_plan(&BTreeSet::new(), GcSegmentBudget::unlimited())
        .unwrap();
    let advisory = plan
        .roots
        .iter()
        .find(|root| root.family_id == Some(DELTA_PACK_CANDIDATE_FAMILY_ID))
        .unwrap();
    assert_eq!(
        advisory.classification,
        GcCompactionClassification::AdvisoryPreservation
    );
    assert_ne!(
        advisory.classification,
        GcCompactionClassification::SemanticLiveness
    );
    assert!(
        plan.page_candidates
            .iter()
            .any(|candidate| candidate.blocker.as_deref() == Some("advisory_preservation"))
    );
    assert!(
        plan.page_candidates
            .iter()
            .any(|candidate| candidate.eligible
                && candidate.classification == GcCompactionClassification::ReclaimNeutral)
    );
    assert!(store.has(&advisory_only).unwrap());
}

#[test]
fn t188_19a_shared_slab_live_and_dead_records_block_one_page_once() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let live_bytes = b"t188-19a-shared-slab-live";
    let dead_bytes = b"t188-19a-shared-slab-dead";
    let live = Digest::hash(store.digest_algo, live_bytes);
    let dead = Digest::hash(store.digest_algo, dead_bytes);
    store
        .group_commit(&[
            (live, live_bytes.as_slice(), store.default_codec),
            (dead, dead_bytes.as_slice(), store.default_codec),
        ])
        .unwrap();
    let shared_page = t188_19a_record_page(&store, live);
    assert_eq!(shared_page, t188_19a_record_page(&store, dead));
    store.set_reference_root(Some(live)).unwrap();

    let plan = store
        .canonical_compaction_plan(&BTreeSet::new(), GcSegmentBudget::unlimited())
        .unwrap();
    let candidates = plan
        .page_candidates
        .iter()
        .filter(|candidate| candidate.page == shared_page)
        .collect::<Vec<_>>();
    assert_eq!(candidates.len(), 1);
    let candidate = candidates[0];
    assert!(!candidate.eligible);
    assert_eq!(
        candidate.classification,
        GcCompactionClassification::SemanticLiveness
    );
    assert_eq!(candidate.blocker.as_deref(), Some("semantic_liveness"));
    assert!(
        candidate
            .owner
            .contains(&format!("object:{}", hex_digest(live)))
    );
    assert!(
        candidate
            .owner
            .contains(&format!("object:{}", hex_digest(dead)))
    );
    assert!(store.has(&live).unwrap());
    assert!(store.has(&dead).unwrap());
}

#[test]
fn t188_19a_shared_slab_neutral_records_consume_one_page_budget() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let first_bytes = b"t188-19a-neutral-shared-slab-first";
    let second_bytes = b"t188-19a-neutral-shared-slab-second";
    let first = Digest::hash(store.digest_algo, first_bytes);
    let second = Digest::hash(store.digest_algo, second_bytes);
    store
        .group_commit(&[
            (first, first_bytes.as_slice(), store.default_codec),
            (second, second_bytes.as_slice(), store.default_codec),
        ])
        .unwrap();
    let shared_page = t188_19a_record_page(&store, first);
    assert_eq!(shared_page, t188_19a_record_page(&store, second));

    let plan = store
        .canonical_compaction_plan(
            &BTreeSet::new(),
            GcSegmentBudget {
                max_segments: u64::MAX,
                max_pages: 1,
            },
        )
        .unwrap();
    let candidates = plan
        .page_candidates
        .iter()
        .filter(|candidate| candidate.page == shared_page)
        .collect::<Vec<_>>();
    assert_eq!(candidates.len(), 1);
    let candidate = candidates[0];
    assert!(candidate.eligible);
    assert_eq!(
        candidate.classification,
        GcCompactionClassification::ReclaimNeutral
    );
    assert_eq!(candidate.blocker, None);
    assert_eq!(plan.eligible_pages, 1);
    assert!(
        candidate
            .owner
            .contains(&format!("object:{}", hex_digest(first)))
    );
    assert!(
        candidate
            .owner
            .contains(&format!("object:{}", hex_digest(second)))
    );
    assert!(store.has(&first).unwrap());
    assert!(store.has(&second).unwrap());
}

#[test]
fn t188_19b_shared_slab_relocation_preserves_all_owners_and_updates_locators() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let live_bytes = b"t188-19b-shared-live";
    let dead_bytes = b"t188-19b-shared-dead";
    let live = Digest::hash(store.digest_algo, live_bytes);
    let dead = Digest::hash(store.digest_algo, dead_bytes);
    store
        .group_commit(&[
            (live, live_bytes.as_slice(), store.default_codec),
            (dead, dead_bytes.as_slice(), store.default_codec),
        ])
        .unwrap();
    let before_live_page = t188_19a_record_page(&store, live);
    let before_dead_page = t188_19a_record_page(&store, dead);
    assert_eq!(before_live_page, before_dead_page);
    store.set_reference_root(Some(live)).unwrap();
    let before_page_count = store.inner.lock().unwrap().page_count;

    let stats = store
        .canonical_compaction_relocate(&BTreeSet::new(), GcSegmentBudget::unlimited())
        .unwrap();

    let after_live_page = t188_19a_record_page(&store, live);
    let after_dead_page = t188_19a_record_page(&store, dead);
    assert_eq!(after_live_page, after_dead_page);
    assert_ne!(after_live_page, before_live_page);
    assert!(after_live_page >= before_page_count);
    assert!(store.has(&live).unwrap());
    assert!(store.has(&dead).unwrap());
    assert_eq!(store.get(&live).unwrap().unwrap(), live_bytes);
    assert_eq!(store.get(&dead).unwrap().unwrap(), dead_bytes);
    assert!(stats.objects_preserved >= 2);
    assert_eq!(
        stats.destination_page_count,
        store.inner.lock().unwrap().page_count
    );
}

#[test]
fn t188_19b_relocation_reopens_complete_canonical_root_vector() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    t188_18b_populate_complete_roots(&store, "t188-19b-complete");
    let before = t188_15_roots(&store);
    let live = t188_19_live_index_set(&store);
    let current_key = durability_facet_test_key(b"documents", "t188-18b-current-t188-19b-complete");

    let stats = store
        .canonical_compaction_relocate(&live, GcSegmentBudget::unlimited())
        .unwrap();
    let after = t188_15_roots(&store);
    assert!(stats.destination_page_count >= stats.source_page_count);
    assert_ne!(after.region_table_root, before.region_table_root);
    assert_ne!(after.index_root, before.index_root);
    assert_ne!(after.current_record_root, before.current_record_root);
    assert_ne!(after.root_catalog_root, before.root_catalog_root);
    assert!(after.region_table_root.is_some());
    assert!(after.index_root.is_some());
    assert!(after.current_record_root.is_some());
    assert!(after.root_catalog_root.is_some());
    assert!(after.reference_root.is_some());
    assert!(after.control_root.is_some());
    assert!(after.retained_history_root.is_some());
    assert!(after.owner_token_root.is_some());
    assert!(after.secondary_index_root.is_some());
    assert!(after.mutable_idempotency_root.is_some());
    assert!(after.workflow_idempotency_root.is_some());
    assert!(after.audit_retention_root.is_some());
    assert!(after.mvcc_generation_root.is_some());
    assert!(after.retention_index_root.is_some());
    assert!(after.checkpoint_index_root.is_some());
    assert!(after.reclaim_index_root.is_some());
    assert!(after.delta_pack_candidate_root.is_some());

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    assert_eq!(t188_15_roots(&reopened), after);
    let snapshot = reopened.mutable_overlay_snapshot().unwrap();
    assert_eq!(
        snapshot
            .read_composite(&current_key, |_| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"t188-18b-current-t188-19b-complete"[..])
    );
}

#[test]
fn t188_19b_pre_publication_interruption_preserves_old_root_vector() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    t188_18b_populate_complete_roots(&store, "t188-19b-interrupt");
    let before = t188_15_roots(&store);
    let live = t188_19_live_index_set(&store);

    let err = store
        .canonical_compaction_relocate_with_pre_publish_interleave_for_test(
            &live,
            GcSegmentBudget::unlimited(),
            |_| Err(LoomError::new(Code::Conflict, "injected relocation stop")),
        )
        .unwrap_err();
    assert_eq!(err.code, Code::Conflict);
    assert_eq!(t188_15_roots(&store), before);
}

#[test]
fn t188_19b_post_commit_pre_adopt_reopen_observes_complete_new_roots() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    t188_18b_populate_complete_roots(&store, "t188-19b-hook");
    let before = t188_15_roots(&store);
    let hook_shared = shared.clone();
    store
        .set_post_commit_pre_adopt_hook_for_test(Box::new(move |roots| {
            let reopened = FileStore::with_backing(Box::new(hook_shared), true).unwrap();
            let reopened_roots = t188_15_roots(&reopened);
            t188_18b_assert_roots_match_result(reopened_roots, roots);
            assert_ne!(reopened_roots, before);
            Ok(())
        }))
        .unwrap();

    let live = t188_19_live_index_set(&store);
    store
        .canonical_compaction_relocate(&live, GcSegmentBudget::unlimited())
        .unwrap();
}

#[test]
fn t188_19b_stale_evidence_conflicts_without_partial_publication() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    t188_18b_populate_complete_roots(&store, "t188-19b-stale-before");
    let evidence = store.gc_reclaim_evidence_for_test().unwrap();
    let live = t188_19_live_index_set(&store);
    t188_15_mutate_owner_token(&store, "t188-19b-stale-after");
    let after_mutation = t188_15_roots(&store);

    let err = store
        .canonical_compaction_relocate_from_evidence_for_test(
            &evidence,
            &live,
            GcSegmentBudget::unlimited(),
        )
        .unwrap_err();
    assert_eq!(err.code, Code::Conflict);
    assert_eq!(t188_15_roots(&store), after_mutation);
}

#[test]
fn t188_19b_concurrent_mutation_conflicts_without_partial_publication() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    t188_18b_populate_complete_roots(&store, "t188-19b-concurrent");
    let live = t188_19_live_index_set(&store);
    let mutation_roots = Arc::new(Mutex::new(None));
    let mutation_roots_for_hook = mutation_roots.clone();

    let err = store
        .canonical_compaction_relocate_with_pre_publish_interleave_for_test(
            &live,
            GcSegmentBudget::unlimited(),
            move |store| {
                t188_15_mutate_owner_token(store, "t188-19b-concurrent-mutation");
                *mutation_roots_for_hook.lock().unwrap() = Some(t188_15_roots(store));
                Ok(())
            },
        )
        .unwrap_err();
    assert_eq!(err.code, Code::Conflict);
    assert_eq!(Some(t188_15_roots(&store)), *mutation_roots.lock().unwrap());
}

#[test]
fn t188_19b_relocation_does_not_reuse_or_reclaim_source_pages() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let live = store.put(&vec![0x19; 96 * 1024]).unwrap();
    store.set_reference_root(Some(live)).unwrap();
    let dead = store.put(&vec![0xB9; 96 * 1024]).unwrap();
    let (before_page_count, before_free) = {
        let inner = store.inner.lock().unwrap();
        (inner.page_count, inner.free.clone())
    };

    let stats = store
        .canonical_compaction_relocate(&BTreeSet::new(), GcSegmentBudget::unlimited())
        .unwrap();
    let inner = store.inner.lock().unwrap();
    assert_eq!(stats.source_page_count, before_page_count);
    assert!(stats.destination_page_count >= before_page_count);
    assert_eq!(inner.page_count, stats.destination_page_count);
    assert_eq!(inner.free, before_free);
    drop(inner);
    assert!(t188_19a_record_page(&store, live) >= before_page_count);
    assert!(!store.has(&dead).unwrap());
}

#[test]
fn t188_19c_blocked_pages_remain_unavailable_and_stale_pages_become_reusable() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let live_bytes = b"t188-19c-shared-live";
    let blocked_dead_bytes = b"t188-19c-shared-dead";
    let live = Digest::hash(store.digest_algo, live_bytes);
    let blocked_dead = Digest::hash(store.digest_algo, blocked_dead_bytes);
    store
        .group_commit(&[
            (live, live_bytes.as_slice(), store.default_codec),
            (
                blocked_dead,
                blocked_dead_bytes.as_slice(),
                store.default_codec,
            ),
        ])
        .unwrap();
    let blocked_page = t188_19a_record_page(&store, live);
    assert_eq!(blocked_page, t188_19a_record_page(&store, blocked_dead));
    store.set_reference_root(Some(live)).unwrap();
    let eligible_dead = store.put(&vec![0xC1; 96 * 1024]).unwrap();
    let eligible_page = t188_19a_record_page(&store, eligible_dead);

    let stats = store
        .canonical_compaction_reclaim(&BTreeSet::new(), GcSegmentBudget::unlimited())
        .unwrap();
    let free_pages = t188_19_free_pages(&store);
    assert!(stats.pages_reclaimed > 0);
    assert!(free_pages.contains(&eligible_page));
    assert!(!free_pages.contains(&blocked_page));
    assert!(store.has(&live).unwrap());
    assert!(store.has(&blocked_dead).unwrap());
    assert!(!store.has(&eligible_dead).unwrap());

    let replacement = t188_19_put_until_free_page_reused(&store, &free_pages, "t188-19c-reuse-a");
    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    assert_eq!(reopened.get(&live).unwrap().unwrap(), live_bytes);
    assert_eq!(
        reopened.get(&blocked_dead).unwrap().unwrap(),
        blocked_dead_bytes
    );
    assert!(reopened.has(&replacement).unwrap());
}

#[test]
fn t188_19c_reused_pages_do_not_corrupt_reopened_canonical_roots() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    t188_18b_populate_complete_roots(&store, "t188-19c-reopen");
    let live = t188_19_live_index_set(&store);
    let dead = store.put(&vec![0xC2; 96 * 1024]).unwrap();
    let dead_page = t188_19a_record_page(&store, dead);
    let current_key = durability_facet_test_key(b"documents", "t188-18b-current-t188-19c-reopen");

    store
        .canonical_compaction_reclaim(&live, GcSegmentBudget::unlimited())
        .unwrap();
    let free_pages = t188_19_free_pages(&store);
    assert!(free_pages.contains(&dead_page));
    let replacement = t188_19_put_until_free_page_reused(&store, &free_pages, "t188-19c-reuse-b");
    let after = t188_15_roots(&store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    assert_eq!(t188_15_roots(&reopened), after);
    let snapshot = reopened.mutable_overlay_snapshot().unwrap();
    assert_eq!(
        snapshot
            .read_composite(&current_key, |_| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"t188-18b-current-t188-19c-reopen"[..])
    );
    assert!(reopened.has(&replacement).unwrap());
    assert!(!reopened.has(&dead).unwrap());
}

#[test]
fn t188_19c_pre_commit_interruption_preserves_previous_generation() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    t188_18b_populate_complete_roots(&store, "t188-19c-pre-commit");
    let live = t188_19_live_index_set(&store);
    let dead = store.put(&vec![0xC7; 96 * 1024]).unwrap();
    let before_with_dead = t188_15_roots(&store);
    let before_free = store.inner.lock().unwrap().free.clone();
    let hook_shared = shared.clone();

    let err = store
        .canonical_compaction_reclaim_with_pre_commit_hook_for_test(
            &live,
            GcSegmentBudget::unlimited(),
            move || {
                let reopened =
                    FileStore::with_backing(Box::new(hook_shared.clone()), true).unwrap();
                assert_eq!(t188_15_roots(&reopened), before_with_dead);
                assert!(reopened.has(&dead).unwrap());
                assert_eq!(reopened.get(&dead).unwrap().unwrap(), vec![0xC7; 96 * 1024]);
                Err(LoomError::new(Code::Conflict, "injected pre-commit stop"))
            },
        )
        .unwrap_err();

    assert_eq!(err.code, Code::Conflict);
    assert_eq!(t188_15_roots(&store), before_with_dead);
    assert_eq!(store.inner.lock().unwrap().free, before_free);
    assert!(store.has(&dead).unwrap());
}

#[test]
fn t188_19c_interruption_publishes_no_free_map_or_root_change() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    t188_18b_populate_complete_roots(&store, "t188-19c-interrupt");
    let live = t188_19_live_index_set(&store);
    let before_roots = t188_15_roots(&store);
    let before_free = store.inner.lock().unwrap().free.clone();

    let err = store
        .canonical_compaction_reclaim_with_pre_publish_interleave_for_test(
            &live,
            GcSegmentBudget::unlimited(),
            |_| Err(LoomError::new(Code::Conflict, "injected reclaim stop")),
        )
        .unwrap_err();
    assert_eq!(err.code, Code::Conflict);
    assert_eq!(t188_15_roots(&store), before_roots);
    assert_eq!(store.inner.lock().unwrap().free, before_free);
}

#[test]
fn t188_19c_stale_evidence_publishes_no_free_map_change() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    t188_18b_populate_complete_roots(&store, "t188-19c-stale-before");
    let evidence = store.gc_reclaim_evidence_for_test().unwrap();
    let live = t188_19_live_index_set(&store);
    t188_15_mutate_owner_token(&store, "t188-19c-stale-after");
    let after_mutation = t188_15_roots(&store);
    let after_mutation_free = store.inner.lock().unwrap().free.clone();

    let err = store
        .canonical_compaction_reclaim_from_evidence_for_test(
            &evidence,
            &live,
            GcSegmentBudget::unlimited(),
        )
        .unwrap_err();
    assert_eq!(err.code, Code::Conflict);
    assert_eq!(t188_15_roots(&store), after_mutation);
    assert_eq!(store.inner.lock().unwrap().free, after_mutation_free);
}

#[test]
fn t188_19c_maintenance_counters_track_reclaimed_free_pages() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let live = store.put(&vec![0x19; 96 * 1024]).unwrap();
    store.set_reference_root(Some(live)).unwrap();
    store.put(&vec![0xC3; 96 * 1024]).unwrap();

    let stats = store
        .canonical_compaction_reclaim(&BTreeSet::new(), GcSegmentBudget::unlimited())
        .unwrap();
    let status = store.maintenance_status().unwrap();
    let free_pages = t188_19_free_page_count(&store);
    assert_eq!(status.reusable_free_pages, free_pages);
    assert_eq!(status.candidate_dead_pages, free_pages);
    assert_eq!(
        status.physical_page_count,
        store.inner.lock().unwrap().page_count
    );
    assert_eq!(stats.destination_page_count, status.physical_page_count);
}

#[test]
fn t188_19c_deferred_pages_are_not_used_by_reclaiming_transaction() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let live = store.put(&vec![0x19; 96 * 1024]).unwrap();
    store.set_reference_root(Some(live)).unwrap();
    let dead = store.put(&vec![0xC8; 96 * 1024]).unwrap();
    let dead_page = t188_19a_record_page(&store, dead);

    let stats = store
        .canonical_compaction_reclaim(&BTreeSet::new(), GcSegmentBudget::unlimited())
        .unwrap();
    let reclaimed = t188_19_free_pages(&store);
    let roots = t188_15_roots(&store);
    let live_page = t188_19a_record_page(&store, live);

    assert!(stats.pages_reclaimed > 0);
    assert!(reclaimed.contains(&dead_page));
    assert!(t188_19_root_pages(&roots).is_disjoint(&reclaimed));
    assert!(!reclaimed.contains(&live_page));
    assert!(!store.has(&dead).unwrap());
    assert!(store.has(&live).unwrap());
}

#[test]
fn t188_19c_page_and_segment_budgets_bound_reclamation() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    store.put(&vec![0xC4; 96 * 1024]).unwrap();
    store.put(&vec![0xC5; 96 * 1024]).unwrap();
    let before_free_pages = t188_19_free_page_count(&store);

    let stats = store
        .canonical_compaction_reclaim(
            &BTreeSet::new(),
            GcSegmentBudget {
                max_segments: u64::MAX,
                max_pages: 1,
            },
        )
        .unwrap();
    assert_eq!(stats.pages_reclaimed, 1);
    assert_eq!(
        t188_19_free_page_count(&store).saturating_sub(before_free_pages),
        1
    );

    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    store.put(&vec![0xC6; 96 * 1024]).unwrap();
    let before_free_pages = t188_19_free_page_count(&store);
    let stats = store
        .canonical_compaction_reclaim(
            &BTreeSet::new(),
            GcSegmentBudget {
                max_segments: 0,
                max_pages: u64::MAX,
            },
        )
        .unwrap();
    assert_eq!(stats.pages_reclaimed, 0);
    assert_eq!(t188_19_free_page_count(&store), before_free_pages);
}

#[test]
fn crash_torn_append_recovers_last_commit() {
    // After N commits, simulate a crash mid-(N+1)th append: garbage appended beyond logical_end,
    // no new superblock. Recovery must yield exactly the N committed objects.
    let (mut bytes, digests) = build_committed(3);
    bytes.extend_from_slice(&[0xB0, 1, 2, 3, 4, 5]); // a partial/garbage record beyond logical_end
    let store = open_bytes(&bytes, "torn-append").unwrap();
    assert_eq!(store.len(), 3);
    for d in &digests {
        assert!(store.has(d).unwrap());
    }
}

#[test]
fn ring_recovers_latest_commit_when_superblock_lags() {
    // Fewer than CHECKPOINT_INTERVAL commits write no superblock checkpoint, so the on-disk
    // superblock stays at generation 0 while the ring holds gens 1..=3. A read-only reopen (no
    // checkpoint-on-open) must still recover all three from the ring.
    let (bytes, digests) = build_committed(3);
    let slot_a: &[u8; SLOT_SIZE as usize] = bytes[..SLOT_SIZE as usize].try_into().unwrap();
    assert_eq!(
        Superblock::decode(slot_a).unwrap().generation,
        0,
        "superblock genuinely lags: no checkpoint at gen < CHECKPOINT_INTERVAL"
    );
    let store = open_read_bytes(&bytes, "ring-lag").unwrap();
    assert_eq!(store.len(), 3);
    for d in &digests {
        assert!(store.has(d).unwrap());
    }
}

#[test]
fn ring_torn_latest_record_falls_back_to_previous() {
    // A crash that tears the latest commit's ring record (bad CRC) falls back to the previous
    // durable commit: the ring keeps each record in its own slot, so a newer record's torn write
    // cannot destroy an earlier acked commit (a single shared slot would).
    let (bytes, _digests) = build_committed(3); // gens 1..=3 in ring slots 1, 2, 3
    let mut torn = bytes.clone();
    let gen3_off = (JOURNAL_OFFSET + 3 * journal::RECORD_SIZE as u64) as usize;
    for byte in torn[gen3_off..gen3_off + journal::RECORD_SIZE].iter_mut() {
        *byte ^= 0xFF; // corrupt gen 3's ring record only
    }
    let store = open_bytes(&torn, "ring-torn-latest").unwrap();
    assert_eq!(store.len(), 2); // recovered gen 2; gen-3's data beyond it is dead space
}

#[test]
fn ring_checkpoint_advances_superblock() {
    // After CHECKPOINT_INTERVAL commits a checkpoint is written, so the on-disk superblock
    // advances to that generation (bounding the recovery scan and freeing ring slots for reuse).
    let n = CHECKPOINT_INTERVAL as usize;
    let (bytes, digests) = build_committed(n);
    let slot_a: &[u8; SLOT_SIZE as usize] = bytes[..SLOT_SIZE as usize].try_into().unwrap();
    let slot_b: &[u8; SLOT_SIZE as usize] = bytes[SLOT_SIZE as usize..2 * SLOT_SIZE as usize]
        .try_into()
        .unwrap();
    let best = [slot_a, slot_b]
        .into_iter()
        .filter_map(Superblock::decode)
        .map(|sb| sb.generation)
        .max()
        .unwrap();
    assert_eq!(best, CHECKPOINT_INTERVAL);
    let store = open_read_bytes(&bytes, "ring-checkpoint").unwrap();
    assert_eq!(store.len(), n);
    for d in &digests {
        assert!(store.has(d).unwrap());
    }
}

#[test]
fn ring_wraps_and_recovers_past_checkpoint() {
    // Commit past RING_SLOTS so the ring wraps and multiple checkpoints land. Recovery overlays
    // the ring's newest generations on the latest superblock checkpoint; every object survives.
    let n = (RING_SLOTS + 8) as usize; // checkpoints at 16 and 32; ring wrapped at gen 33
    let (bytes, digests) = build_committed(n);
    let store = open_read_bytes(&bytes, "ring-wrap").unwrap();
    assert_eq!(store.len(), n);
    for d in &digests {
        assert!(store.has(d).unwrap());
    }
}

#[test]
fn lost_committed_data_is_a_clean_error_not_a_panic() {
    // Truncating into the committed data region destroys data a valid superblock references.
    // Recovery must report a clean CORRUPT error (never panic, never silently wrong).
    let (bytes, _) = build_committed(3);
    let truncated = &bytes[..bytes.len() - 4]; // chop into the last committed record
    let err = open_bytes(truncated, "lost-data").unwrap_err();
    assert!(matches!(err.code, Code::CorruptObject | Code::Io));
}

#[test]
fn put_batch_commits_atomically_in_one_generation() {
    let tp = TempPath::new("batch");
    let store = FileStore::open(tp.path()).unwrap();
    // Three single puts advance the generation three times...
    store.put(&blob(b"x")).unwrap();
    store.put(&blob(b"y")).unwrap();
    store.put(&blob(b"z")).unwrap();
    assert_eq!(store.generation(), 3);
    // ...whereas a batch of three commits in a single superblock swap (one generation bump).
    let before = store.generation();
    let ds = store
        .put_batch(&[blob(b"a").as_slice(), &blob(b"b"), &blob(b"c")])
        .unwrap();
    assert_eq!(ds.len(), 3);
    assert_eq!(
        store.generation(),
        before + 1,
        "batch must be one atomic commit"
    );
    assert_eq!(store.len(), 6);
    // The batched objects survive a reopen (the swap committed them all).
    drop(store);
    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(store.len(), 6);
    for d in &ds {
        assert!(store.has(d).unwrap());
    }
}

#[test]
fn put_batch_dedups_within_batch_and_against_store() {
    let tp = TempPath::new("batch-dedup");
    let store = FileStore::open(tp.path()).unwrap();
    let a = store.put(&blob(b"a")).unwrap(); // already stored
    // Batch repeats `a`, repeats `b` twice, plus a fresh `c`.
    let ds = store
        .put_batch(&[blob(b"a").as_slice(), &blob(b"b"), &blob(b"b"), &blob(b"c")])
        .unwrap();
    assert_eq!(ds.len(), 4); // one digest reported per input...
    assert_eq!(ds[0], a);
    assert_eq!(ds[1], ds[2]); // ...the two `b`s share a digest
    assert_eq!(store.len(), 3); // ...but only a, b, c are stored
    drop(store);
    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(store.len(), 3);
}

#[test]
fn interrupted_batch_leaves_the_prior_committed_state() {
    // A crash mid-batch (records/index nodes appended, superblock not yet flipped) appears as
    // bytes beyond the committed logical_end. Recovery must show none of the in-flight batch -
    // the all-or-nothing guarantee, identical to the single-record torn-append case.
    let (mut bytes, digests) = build_committed(2);
    bytes.extend_from_slice(&[0xAB; 256]); // a partially written, uncommitted batch
    let store = open_bytes(&bytes, "torn-batch").unwrap();
    assert_eq!(store.len(), 2);
    for d in &digests {
        assert!(store.has(d).unwrap());
    }
}

#[test]
fn mid_txn_crash_reclaims_pages_and_keeps_committed() {
    // The region-table-page swap is the prepare/commit boundary: a crash after a txn wrote its
    // pages but before its COMMIT record was fsynced leaves exactly the prior committed state, and
    // the crashed txn's pages return to free. Simulate the crash by appending uncommitted pages (a
    // crashed txn's file extension) past the committed page array.
    let (mut bytes, digests) = build_committed(4);
    let committed_len = bytes.len();
    bytes.extend_from_slice(&vec![0xCDu8; 8 * PAGE_SIZE as usize]); // a crashed txn's appended pages
    let tp = TempPath::new("mid-txn-crash");
    std::fs::write(tp.path(), &bytes).unwrap();

    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(store.len(), 4); // nothing from the in-flight txn is visible
    for d in &digests {
        assert!(store.has(d).unwrap());
    }
    assert_eq!(
        store.logical_end() as usize,
        committed_len,
        "recovery reverts to the committed page array; the crashed txn's pages are not retained"
    );
    // A fresh commit reuses the space the crashed txn occupied rather than leaking it.
    let d = store.put(&blob(b"after-crash")).unwrap();
    drop(store);
    let reopened = FileStore::open(tp.path()).unwrap();
    assert_eq!(reopened.len(), 5);
    assert!(reopened.has(&d).unwrap());
    for d in &digests {
        assert!(reopened.has(d).unwrap());
    }
}

#[test]
fn gc_chooses_only_mostly_dead_segments() {
    let occ = BTreeMap::from([
        (0u64, (9u64, 10u64)), // 90% live -> keep
        (1u64, (1u64, 10u64)), // 10% live -> collect
        (2u64, (5u64, 10u64)), // exactly half live -> keep (not below half)
        (3u64, (0u64, 4u64)),  // fully dead -> collect
        (4u64, (4u64, 4u64)),  // fully live -> keep
    ]);
    assert_eq!(
        choose_sparse_segments_bounded(&occ, None, GcSegmentBudget::unlimited()),
        vec![1, 3]
    );
    let eligible = BTreeSet::from([3u64]);
    assert_eq!(
        choose_sparse_segments_bounded(&occ, Some(&eligible), GcSegmentBudget::unlimited()),
        vec![3]
    );
    assert_eq!(
        choose_sparse_segments_bounded(
            &occ,
            None,
            GcSegmentBudget {
                max_segments: 1,
                max_pages: u64::MAX
            }
        ),
        vec![1]
    );
}

#[test]
fn gc_segments_reclaims_a_mostly_dead_segment_and_keeps_live() {
    let tp = TempPath::new("gc-seg");
    let mut store = FileStore::open(tp.path()).unwrap();
    let n = 300usize;
    let mut digests = Vec::with_capacity(n);
    for i in 0..n {
        digests.push(store.put(&blob(format!("obj-{i:04}").as_bytes())).unwrap());
    }
    store
        .control_set(b"lock/ns/fence", b"301".to_vec())
        .unwrap();
    {
        let mut inner = store.inner.lock().unwrap();
        let start = inner.page_count;
        let len = page::PAGES_PER_SEGMENT;
        let freed_gen = inner.generation;
        inner.free.push(FreePageRun {
            start,
            len,
            freed_gen,
        });
        inner.page_count += len;
        inner.maintenance.physical_page_count = inner.page_count;
        inner.maintenance.reusable_free_pages += len;
        let mut file = store.file.lock().unwrap();
        file.grow(DATA_START + inner.page_count * PAGE_SIZE)
            .unwrap();
    }
    // Keep only every tenth object: segment 0 becomes ~90% dead, so GC reclaims it.
    let live: BTreeSet<[u8; 32]> = digests
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 10 == 0)
        .map(|(_, d)| *d.bytes())
        .collect();
    let free_before: u64 = store.free_runs().iter().map(|r| r.len).sum();
    let index_root_before = store.inner.lock().unwrap().index_root;
    take_object_index_batch_page_stats();

    let stats = store.gc_segments(&live).unwrap();
    let batch_stats = take_object_index_batch_page_stats();
    assert_eq!(batch_stats.len(), 1);
    assert!(
        batch_stats[0].existing_pages_replaced > 0,
        "batch={batch_stats:?}; gc={stats:?}"
    );
    assert!(stats.objects_relocated > 1, "gc={stats:?}");
    assert!(stats.objects_dropped > 0, "GC should drop dead objects");
    assert!(stats.pages_freed > 0, "GC should free reclaimed pages");
    assert_eq!(store.len(), live.len() + 1);
    assert_eq!(
        store.control_get(b"lock/ns/fence").unwrap().as_deref(),
        Some(&b"301"[..])
    );
    // Survivors still resolve to their bytes; dropped objects are gone.
    for (i, d) in digests.iter().enumerate() {
        let want_live = i % 10 == 0;
        assert_eq!(store.has(d).unwrap(), want_live);
        if want_live {
            assert_eq!(
                store.get(d).unwrap().unwrap(),
                blob(format!("obj-{i:04}").as_bytes())
            );
        }
    }
    // Reclaimed pages went back to the free-page map (reusable, not yet truncated).
    let free_after: u64 = store.free_runs().iter().map(|r| r.len).sum();
    assert!(
        free_after > free_before,
        "reclaimed pages should be free now"
    );
    let (committed_index_root, committed_region_table_root, committed_object_count) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.index_root,
            inner.region_table_root,
            inner.maintenance.object_count,
        )
    };
    assert_ne!(committed_index_root, index_root_before);
    assert!(committed_region_table_root.is_some());
    assert_eq!(committed_object_count, (live.len() + 1) as u64);

    // Everything survives a reopen of the GC'd file.
    drop(store);
    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(store.len(), live.len() + 1);
    let reopened = store.inner.lock().unwrap();
    assert_eq!(reopened.index_root, committed_index_root);
    assert_eq!(reopened.region_table_root, committed_region_table_root);
    assert_eq!(reopened.maintenance.object_count, committed_object_count);
    let reopened_free = reopened.free.iter().map(|run| run.len).sum::<u64>();
    assert!(reopened_free >= stats.pages_freed);
    drop(reopened);
    assert_eq!(
        store.control_get(b"lock/ns/fence").unwrap().as_deref(),
        Some(&b"301"[..])
    );
    for (i, d) in digests.iter().enumerate() {
        assert_eq!(store.has(d).unwrap(), i % 10 == 0);
    }
}

#[test]
fn gc_validated_segments_requires_completed_epoch_and_obeys_budget() {
    let tp = TempPath::new("gc-validated-seg");
    let mut store = FileStore::open(tp.path()).unwrap();
    let n = 300usize;
    let mut digests = Vec::with_capacity(n);
    for i in 0..n {
        digests.push(store.put(&blob(format!("obj-{i:04}").as_bytes())).unwrap());
    }
    let missing = store
        .gc_validated_segments(GcSegmentBudget {
            max_segments: 1,
            max_pages: u64::MAX,
        })
        .unwrap_err();
    assert_eq!(missing.code, Code::NotFound);

    let live_digests = digests
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 10 == 0)
        .map(|(_, digest)| *digest)
        .collect::<BTreeSet<_>>();
    let state = loom_core::ReachabilityMarkState {
        pinned: BTreeSet::new(),
        marked: live_digests,
        queue: std::collections::VecDeque::new(),
        stream_roots: std::collections::VecDeque::new(),
        content_roots: std::collections::VecDeque::new(),
        prolly_cursors: std::collections::VecDeque::new(),
        completed: false,
    };
    let mut epoch = store
        .begin_reachability_mark_epoch(None, BTreeSet::new(), state)
        .unwrap();
    let incomplete = store
        .gc_validated_segments(GcSegmentBudget {
            max_segments: 1,
            max_pages: u64::MAX,
        })
        .unwrap_err();
    assert_eq!(incomplete.code, Code::Conflict);

    epoch.state.completed = true;
    while !epoch.metadata_completed {
        let visited = store
            .step_reachability_metadata_mark_epoch(&mut epoch, 64, None)
            .unwrap();
        assert!(visited > 0, "metadata reachability traversal stalled");
    }
    store.complete_reachability_mark_epoch(&epoch).unwrap();
    let stats = store
        .gc_validated_segments(GcSegmentBudget {
            max_segments: 1,
            max_pages: u64::MAX,
        })
        .unwrap();
    assert_eq!(
        stats.objects_relocated, 0,
        "the active segment must be swept without relocating live objects"
    );
    assert_eq!(stats.segments_reclaimed, 0);
    assert!(stats.objects_dropped > 0);
    assert!(stats.pages_freed > 0);
    for (i, digest) in digests.iter().enumerate() {
        assert_eq!(store.has(digest).unwrap(), i % 10 == 0);
    }
}

fn completed_validated_reclaim_fixture(
    path: &std::path::Path,
) -> (FileStore, Vec<Digest>, BTreeSet<Digest>, Digest, u64) {
    let store = FileStore::open(path).unwrap();
    let mut digests = Vec::new();
    for i in 0..300usize {
        digests.push(store.put(&blob(format!("obj-{i:04}").as_bytes())).unwrap());
    }
    let live = digests
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 10 == 0)
        .map(|(_, digest)| *digest)
        .collect::<BTreeSet<_>>();
    let state = loom_core::ReachabilityMarkState {
        pinned: BTreeSet::new(),
        marked: live.clone(),
        queue: std::collections::VecDeque::new(),
        stream_roots: std::collections::VecDeque::new(),
        content_roots: std::collections::VecDeque::new(),
        prolly_cursors: std::collections::VecDeque::new(),
        completed: true,
    };
    let mut epoch = store
        .begin_reachability_mark_epoch(None, BTreeSet::new(), state)
        .unwrap();
    let high_water = epoch.page_high_water_mark;
    while !epoch.metadata_completed {
        let visited = store
            .step_reachability_metadata_mark_epoch(&mut epoch, 64, None)
            .unwrap();
        assert!(visited > 0, "metadata reachability traversal stalled");
    }
    store.complete_reachability_mark_epoch(&epoch).unwrap();
    let post_snapshot = store.put(&blob(b"post-snapshot")).unwrap();
    (store, digests, live, post_snapshot, high_water)
}

fn exposed_reusable_pages_below(store: &FileStore, high_water: u64) -> u64 {
    let (free, fence, horizon) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.free.clone(),
            inner.active_mark_epoch_reclaim_fence,
            inner.minimum_recoverable_generation,
        )
    };
    let (reusable, lease) = store
        .transaction_reusable_free(&free, fence, horizon)
        .unwrap();
    if !lease.allowed {
        return 0;
    }
    reusable
        .iter()
        .map(|run| {
            let end = run.start.saturating_add(run.len);
            end.min(high_water).saturating_sub(run.start)
        })
        .sum()
}

fn assert_validated_reclaim_payloads(
    store: &FileStore,
    digests: &[Digest],
    live: &BTreeSet<Digest>,
    post_snapshot: Digest,
) {
    for digest in live {
        assert!(store.has(digest).unwrap());
    }
    for digest in digests {
        assert_eq!(store.has(digest).unwrap(), live.contains(digest));
    }
    assert!(store.has(&post_snapshot).unwrap());
}

fn assert_committed_object_index_root_not_free(store: &FileStore, stage: &str) {
    let inner = store.inner.lock().unwrap();
    let object_index_root = inner.index_root.expect("committed object index root");
    assert!(
        inner.free.iter().all(|run| {
            object_index_root.0 < run.start
                || object_index_root.0 >= run.start.saturating_add(run.len)
        }),
        "committed object index root {} is listed as free in generation {} {stage}",
        object_index_root.0,
        inner.generation
    );
}

fn assert_persisted_object_index_root_not_free(store: &FileStore, stage: &str) {
    let (object_index_root, free_map_root, page_count, generation) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.index_root.expect("committed object index root"),
            inner.freemap.expect("committed free-map root").0,
            inner.page_count,
            inner.generation,
        )
    };
    let free = {
        let mut file = store.file.lock().unwrap();
        pagemap::read_map_with_root_span(&mut **file, DATA_START, free_map_root, page_count)
            .unwrap()
            .0
    };
    let containing_run = free.iter().find(|run| {
        object_index_root.0 >= run.start && object_index_root.0 < run.start.saturating_add(run.len)
    });
    assert!(
        containing_run.is_none(),
        "committed object index root {} is listed in persisted free run {containing_run:?} in generation {} {stage}",
        object_index_root.0,
        generation
    );
}

#[test]
fn committed_free_extent_reused_for_current_record_root_is_removed_from_persisted_free_map() {
    let backing = SharedMem::default();
    let mut store = FileStore::with_backing(Box::new(backing.clone()), true).unwrap();
    let first_key = durability_facet_test_key(b"documents", "committed-free-current-root-first");
    store
        .put_mutable_overlay_value(first_key, b"first".to_vec())
        .unwrap();

    let committed_run = {
        let mut inner = store.inner.lock().unwrap();
        let start = inner.page_count.saturating_add(8);
        let first = FreePageRun {
            start,
            len: 16,
            freed_gen: inner.generation.saturating_add(1),
        };
        let second = FreePageRun {
            start: first.start.saturating_add(first.len),
            len: 496,
            freed_gen: first.freed_gen,
        };
        inner.free.extend([first, second]);
        inner.page_count = second.start.saturating_add(second.len);
        inner.maintenance.physical_page_count = inner.page_count;
        store
            .file
            .lock()
            .unwrap()
            .grow(DATA_START + inner.page_count * PAGE_SIZE)
            .unwrap();
        FreePageRun {
            start: first.start,
            len: first.len.saturating_add(second.len),
            freed_gen: first.freed_gen,
        }
    };
    store
        .commit_raw_control_map_for_test(BTreeMap::from([(b"free-run".to_vec(), vec![1])]))
        .unwrap();
    for generation in 0..REUSE_SAFE_WINDOW {
        store
            .commit_raw_control_map_for_test(BTreeMap::from([(
                b"free-run".to_vec(),
                generation.to_le_bytes().to_vec(),
            )]))
            .unwrap();
    }

    let state = loom_core::ReachabilityMarkState {
        pinned: BTreeSet::new(),
        marked: BTreeSet::new(),
        queue: std::collections::VecDeque::new(),
        stream_roots: std::collections::VecDeque::new(),
        content_roots: std::collections::VecDeque::new(),
        prolly_cursors: std::collections::VecDeque::new(),
        completed: true,
    };
    let mut epoch = store
        .begin_reachability_mark_epoch(None, BTreeSet::new(), state)
        .unwrap();

    let second_key = durability_facet_test_key(b"documents", "committed-free-current-root-second");
    let mut entries = vec![(second_key.clone(), b"second".to_vec())];
    entries.extend((0..48u64).map(|index| {
        (
            durability_facet_test_key(
                b"documents",
                &format!("committed-free-current-root-batch-{index:03}"),
            ),
            index.to_le_bytes().to_vec(),
        )
    }));
    store.put_mutable_overlay_values(entries).unwrap();
    let (root, free_map_root, page_count) = {
        let inner = store.inner.lock().unwrap();
        let root = inner.current_record_root.expect("current-record root");
        assert!(
            root.0 >= committed_run.start
                && root.0 < committed_run.start.saturating_add(committed_run.len),
            "current-record root {} did not reuse committed run {committed_run:?}",
            root.0
        );
        assert!(
            inner
                .free
                .iter()
                .all(|run| { root.0 < run.start || root.0 >= run.start.saturating_add(run.len) })
        );
        (root, inner.freemap.unwrap().0, inner.page_count)
    };
    let persisted_free = {
        let mut file = store.file.lock().unwrap();
        pagemap::read_map_with_root_span(&mut **file, DATA_START, free_map_root, page_count)
            .unwrap()
            .0
    };
    assert!(
        persisted_free
            .iter()
            .all(|run| { root.0 < run.start || root.0 >= run.start.saturating_add(run.len) })
    );
    while !epoch.metadata_completed {
        assert!(
            store
                .step_reachability_metadata_mark_epoch(&mut epoch, 64, None)
                .unwrap()
                > 0
        );
    }
    store.complete_reachability_mark_epoch(&epoch).unwrap();
    store
        .gc_validated_segments(GcSegmentBudget::unlimited())
        .unwrap();
    {
        let inner = store.inner.lock().unwrap();
        assert_eq!(inner.current_record_root, Some(root));
        assert!(
            inner
                .free
                .iter()
                .all(|run| { root.0 < run.start || root.0 >= run.start.saturating_add(run.len) })
        );
    }
    drop(store);

    let reopened = FileStore::with_backing(Box::new(backing), true).unwrap();
    let inner = reopened.inner.lock().unwrap();
    assert_eq!(inner.current_record_root, Some(root));
    assert!(
        inner
            .free
            .iter()
            .all(|run| { root.0 < run.start || root.0 >= run.start.saturating_add(run.len) })
    );
    drop(inner);
    assert_eq!(
        reopened
            .mutable_overlay_current_entry(&second_key)
            .unwrap()
            .unwrap()
            .payload,
        b"second"
    );
}

#[test]
fn gc_validated_segments_clear_failure_preserves_epoch_evidence_and_fence() {
    let tp = TempPath::new("gc-validated-clear-failure");
    let (mut store, digests, live, post_snapshot, high_water) =
        completed_validated_reclaim_fixture(tp.path());
    assert_committed_object_index_root_not_free(&store, "before validated GC");
    let attempts = Arc::new(AtomicU64::new(0));
    store
        .set_reachability_epoch_pre_finish_hook_for_test(Box::new({
            let attempts = Arc::clone(&attempts);
            move || {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err(LoomError::new(
                    Code::Conflict,
                    "injected reachability epoch clear failure",
                ))
            }
        }))
        .unwrap();

    let err = store
        .gc_validated_segments(GcSegmentBudget {
            max_segments: 1,
            max_pages: u64::MAX,
        })
        .unwrap_err();
    assert_eq!(err.code, Code::Conflict);
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert_validated_reclaim_payloads(&store, &digests, &live, post_snapshot);
    assert_committed_object_index_root_not_free(&store, "after validated GC");
    assert_persisted_object_index_root_not_free(&store, "after validated GC");

    let active = store.active_reachability_mark_epoch().unwrap().unwrap();
    assert_eq!(active.page_high_water_mark, high_water);
    assert!(
        store
            .active_reachability_mark_reclaim_evidence()
            .unwrap()
            .unwrap()
            .matches_epoch(&active, store.digest_algo)
    );
    assert_eq!(
        store.inner.lock().unwrap().active_mark_epoch_reclaim_fence,
        Some(high_water)
    );
    assert_eq!(exposed_reusable_pages_below(&store, high_water), 0);
    drop(store);

    let store = FileStore::open(tp.path()).unwrap();
    let active = store.active_reachability_mark_epoch().unwrap().unwrap();
    assert_eq!(active.page_high_water_mark, high_water);
    assert!(
        store
            .active_reachability_mark_reclaim_evidence()
            .unwrap()
            .unwrap()
            .matches_epoch(&active, store.digest_algo)
    );
    assert_eq!(
        store.inner.lock().unwrap().active_mark_epoch_reclaim_fence,
        Some(high_water)
    );
    assert_eq!(exposed_reusable_pages_below(&store, high_water), 0);
    assert_committed_object_index_root_not_free(&store, "after reopen");

    assert!(store.clear_reachability_mark_epoch().unwrap());
    assert!(store.active_reachability_mark_epoch().unwrap().is_none());
    assert!(
        store
            .active_reachability_mark_reclaim_evidence()
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store.inner.lock().unwrap().active_mark_epoch_reclaim_fence,
        None
    );
    assert!(exposed_reusable_pages_below(&store, high_water) > 0);
    assert_validated_reclaim_payloads(&store, &digests, &live, post_snapshot);
}

#[test]
fn gc_validated_segments_reader_lease_blocks_epoch_clear_after_reclaim() {
    let tp = TempPath::new("gc-validated-clear-reader");
    let (mut store, digests, live, post_snapshot, high_water) =
        completed_validated_reclaim_fixture(tp.path());
    store
        .set_reachability_epoch_pre_finish_hook_for_test(Box::new(|| {
            Err(LoomError::new(Code::Conflict, "injected first clear stop"))
        }))
        .unwrap();
    let err = store
        .gc_validated_segments(GcSegmentBudget {
            max_segments: 1,
            max_pages: u64::MAX,
        })
        .unwrap_err();
    assert_eq!(err.code, Code::Conflict);

    let reader = FileStore::open_read(tp.path()).unwrap();
    let err = store.clear_reachability_mark_epoch().unwrap_err();
    assert_eq!(err.code, Code::Conflict);
    let active = store.active_reachability_mark_epoch().unwrap().unwrap();
    assert_eq!(active.page_high_water_mark, high_water);
    assert!(
        store
            .active_reachability_mark_reclaim_evidence()
            .unwrap()
            .unwrap()
            .matches_epoch(&active, store.digest_algo)
    );
    assert_eq!(
        store.inner.lock().unwrap().active_mark_epoch_reclaim_fence,
        Some(high_water)
    );
    assert_eq!(exposed_reusable_pages_below(&store, high_water), 0);
    assert!(reader.has(&post_snapshot).unwrap());

    drop(reader);
    assert!(store.clear_reachability_mark_epoch().unwrap());
    assert!(store.active_reachability_mark_epoch().unwrap().is_none());
    assert!(
        store
            .active_reachability_mark_reclaim_evidence()
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store.inner.lock().unwrap().active_mark_epoch_reclaim_fence,
        None
    );
    assert!(exposed_reusable_pages_below(&store, high_water) > 0);
    assert_validated_reclaim_payloads(&store, &digests, &live, post_snapshot);
}

#[test]
fn repeated_validated_segment_gc_does_not_leave_untracked_record_pages() {
    let tp = TempPath::new("gc-repeated-no-stale-records");
    let mut store = FileStore::open(tp.path()).unwrap();
    let mut retained = BTreeSet::new();

    for cycle in 0..4u32 {
        for object in 0..300u32 {
            let digest = store
                .put(&blob(
                    format!("cycle-{cycle}-object-{object:04}").as_bytes(),
                ))
                .unwrap();
            if object % 50 == 0 {
                retained.insert(digest);
            }
        }
        let state = loom_core::ReachabilityMarkState {
            pinned: BTreeSet::new(),
            marked: retained.clone(),
            queue: std::collections::VecDeque::new(),
            stream_roots: std::collections::VecDeque::new(),
            content_roots: std::collections::VecDeque::new(),
            prolly_cursors: std::collections::VecDeque::new(),
            completed: true,
        };
        let epoch = store
            .begin_reachability_mark_epoch(None, BTreeSet::new(), state)
            .unwrap();
        store.complete_reachability_mark_epoch(&epoch).unwrap();
        store
            .gc_validated_segments(GcSegmentBudget::unlimited())
            .unwrap();

        let stale_record_bytes = store
            .page_class_attribution(0)
            .unwrap()
            .classes
            .iter()
            .filter(|class| class.class.starts_with("stale_record_"))
            .map(|class| class.bytes)
            .sum::<u64>();
        assert_eq!(
            stale_record_bytes, 0,
            "GC cycle {cycle} left record pages outside both the object index and free map"
        );
    }
}

#[test]
fn tail_trim_shrinks_only_an_already_free_eof_suffix() {
    let tp = TempPath::new("tail-trim-free-eof");
    let mut store = FileStore::open(tp.path()).unwrap();
    let live_digest = store.put(&blob(b"live-before-free-tail")).unwrap();
    store.set_reference_root(Some(live_digest)).unwrap();
    let before = store.maintenance_status().unwrap().physical_page_count;
    {
        let mut inner = store.inner.lock().unwrap();
        let suffix = 128;
        let start = inner.page_count;
        let freed_gen = inner.generation;
        inner.free.push(FreePageRun {
            start,
            len: suffix,
            freed_gen,
        });
        inner.page_count += suffix;
        inner.maintenance.physical_page_count = inner.page_count;
        inner.maintenance.reusable_free_pages += suffix;
        inner.maintenance.candidate_dead_pages += suffix;
        let mut file = store.file.lock().unwrap();
        file.grow(DATA_START + inner.page_count * PAGE_SIZE)
            .unwrap();
    }

    let expanded = store.maintenance_status().unwrap().physical_page_count;
    assert_eq!(expanded, before + 128);
    let trimmed = store.trim_tail_free_pages().unwrap();
    let after = store.maintenance_status().unwrap().physical_page_count;
    assert!(trimmed > 0);
    assert!(after < expanded);
    assert_eq!(store.has(&live_digest).unwrap(), true);
    drop(store);

    let reopened = FileStore::open(tp.path()).unwrap();
    assert_eq!(reopened.has(&live_digest).unwrap(), true);
    assert_eq!(
        reopened.maintenance_status().unwrap().physical_page_count,
        after
    );
}

#[test]
fn reader_lease_blocks_tail_trim_until_the_reader_closes() {
    let tp = TempPath::new("reader-lease-tail-trim");
    let mut store = FileStore::open(tp.path()).unwrap();
    let live_digest = store.put(&blob(b"live-before-reader-lease")).unwrap();
    store.set_reference_root(Some(live_digest)).unwrap();
    {
        let mut inner = store.inner.lock().unwrap();
        let suffix = 32;
        let start = inner.page_count;
        let freed_gen = inner.generation;
        inner.free.push(FreePageRun {
            start,
            len: suffix,
            freed_gen,
        });
        inner.page_count += suffix;
        inner.maintenance.physical_page_count = inner.page_count;
        inner.maintenance.reusable_free_pages += suffix;
        inner.maintenance.candidate_dead_pages += suffix;
        let mut file = store.file.lock().unwrap();
        file.grow(DATA_START + inner.page_count * PAGE_SIZE)
            .unwrap();
    }

    let reader = FileStore::open_read(tp.path()).unwrap();
    let expanded = store.maintenance_status().unwrap().physical_page_count;
    assert_eq!(
        store
            .group_commit_diagnostics()
            .unwrap()
            .pinned_reader_blockers,
        Some(1)
    );
    assert_eq!(store.trim_tail_free_pages().unwrap(), 0);
    assert_eq!(
        store.maintenance_status().unwrap().physical_page_count,
        expanded
    );
    assert_eq!(reader.has(&live_digest).unwrap(), true);

    drop(reader);
    assert_eq!(
        store
            .group_commit_diagnostics()
            .unwrap()
            .pinned_reader_blockers,
        Some(0)
    );
    assert!(store.trim_tail_free_pages().unwrap() > 0);
    assert!(store.maintenance_status().unwrap().physical_page_count < expanded);
    assert_eq!(store.has(&live_digest).unwrap(), true);
}

#[test]
fn tail_compaction_relocates_live_tail_object_and_shrinks() {
    let tp = TempPath::new("tail-compact-live");
    let mut store = FileStore::open(tp.path()).unwrap();
    {
        let mut inner = store.inner.lock().unwrap();
        let free_pages = 512;
        inner.free.push(FreePageRun {
            start: 0,
            len: free_pages,
            freed_gen: 1,
        });
        inner.page_count = free_pages;
        inner.maintenance.physical_page_count = free_pages;
        inner.maintenance.reusable_free_pages = free_pages;
        inner.maintenance.candidate_dead_pages = free_pages;
        let mut file = store.file.lock().unwrap();
        file.grow(DATA_START + inner.page_count * PAGE_SIZE)
            .unwrap();
    }
    let live = store.put(&vec![0xC7; 300 * 1024]).unwrap();
    store.set_reference_root(Some(live)).unwrap();
    {
        let mut inner = store.inner.lock().unwrap();
        inner.generation = REUSE_SAFE_WINDOW + 10;
        for run in &mut inner.free {
            run.freed_gen = 1;
        }
    }
    let before = store.maintenance_status().unwrap().physical_page_count;

    let stats = store.compact_tail_once(256, 1, 512 * 1024).unwrap();
    assert!(stats.attempted);
    assert_eq!(stats.relocated_objects, 1);
    assert!(stats.relocated_pages > 0);
    assert!(stats.truncated_pages > 0);
    let after = store.maintenance_status().unwrap().physical_page_count;
    assert!(after < before);
    assert_eq!(store.get(&live).unwrap().unwrap(), vec![0xC7; 300 * 1024]);
    drop(store);

    let reopened = FileStore::open(tp.path()).unwrap();
    assert_eq!(
        reopened.get(&live).unwrap().unwrap(),
        vec![0xC7; 300 * 1024]
    );
}

#[test]
fn tail_compaction_relocates_every_object_on_a_selected_slab_page() {
    let tp = TempPath::new("tail-compact-shared-slab");
    let mut store = FileStore::open(tp.path()).unwrap();
    {
        let mut inner = store.inner.lock().unwrap();
        let free_pages = 64;
        inner.free.push(FreePageRun {
            start: 0,
            len: free_pages,
            freed_gen: 1,
        });
        inner.page_count = free_pages;
        inner.maintenance.physical_page_count = free_pages;
        inner.maintenance.reusable_free_pages = free_pages;
        inner.maintenance.candidate_dead_pages = free_pages;
        let mut file = store.file.lock().unwrap();
        file.grow(DATA_START + inner.page_count * PAGE_SIZE)
            .unwrap();
    }
    let first_bytes = b"first shared slab object";
    let second_bytes = b"second shared slab object";
    let first = Digest::hash(store.digest_algo, first_bytes);
    let second = Digest::hash(store.digest_algo, second_bytes);
    store
        .group_commit(&[
            (first, first_bytes.as_slice(), store.default_codec),
            (second, second_bytes.as_slice(), store.default_codec),
        ])
        .unwrap();
    {
        let mut inner = store.inner.lock().unwrap();
        inner.generation = REUSE_SAFE_WINDOW + 10;
        for run in &mut inner.free {
            run.freed_gen = 1;
        }
    }
    let before = store.maintenance_status().unwrap();
    let index_root_before = store.inner.lock().unwrap().index_root;
    take_object_index_batch_page_stats();

    let stats = store.compact_tail_once(16, 1, 32).unwrap();
    let batch_stats = take_object_index_batch_page_stats();
    assert_eq!(batch_stats.len(), 1);
    assert!(batch_stats[0].existing_pages_replaced > 0);
    assert!(stats.attempted);
    assert_eq!(stats.relocated_objects, 2);
    assert_eq!(stats.relocated_pages, 1);
    let after = store.maintenance_status().unwrap();
    assert_eq!(after.object_count, before.object_count);
    assert_eq!(after.object_count, 2);
    let (committed_index_root, committed_region_table_root, committed_free) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.index_root,
            inner.region_table_root,
            inner.free.iter().map(|run| run.len).sum::<u64>(),
        )
    };
    assert_ne!(committed_index_root, index_root_before);
    assert!(committed_region_table_root.is_some());
    assert!(committed_free > 0);
    assert_eq!(
        store.get(&first).unwrap().unwrap(),
        b"first shared slab object"
    );
    assert_eq!(
        store.get(&second).unwrap().unwrap(),
        b"second shared slab object"
    );
    drop(store);

    let reopened = FileStore::open(tp.path()).unwrap();
    let reopened_status = reopened.maintenance_status().unwrap();
    assert_eq!(reopened_status.object_count, after.object_count);
    assert_eq!(
        reopened_status.physical_page_count,
        after.physical_page_count
    );
    let reopened_inner = reopened.inner.lock().unwrap();
    assert_eq!(reopened_inner.index_root, committed_index_root);
    assert_eq!(
        reopened_inner.region_table_root,
        committed_region_table_root
    );
    assert_eq!(
        reopened_inner.free.iter().map(|run| run.len).sum::<u64>(),
        committed_free
    );
    drop(reopened_inner);
    assert_eq!(
        reopened.get(&first).unwrap().unwrap(),
        b"first shared slab object"
    );
    assert_eq!(
        reopened.get(&second).unwrap().unwrap(),
        b"second shared slab object"
    );
}

#[test]
fn tail_compaction_skips_without_earlier_free_space() {
    let tp = TempPath::new("tail-compact-no-space");
    let mut store = FileStore::open(tp.path()).unwrap();
    let live = store.put(&vec![0xD1; 300 * 1024]).unwrap();
    store.set_reference_root(Some(live)).unwrap();

    let stats = store.compact_tail_once(160, 1, 512 * 1024).unwrap();
    assert!(stats.attempted);
    assert!(stats.skipped);
    assert_eq!(stats.relocated_objects, 0);
    assert_eq!(store.get(&live).unwrap().unwrap(), vec![0xD1; 300 * 1024]);
}

#[test]
fn canonical_relocation_rejects_while_a_reader_lease_is_active() {
    let tp = TempPath::new("reader-lease-canonical-relocation");
    let store = FileStore::open(tp.path()).unwrap();
    let live = store.put(&blob(b"canonical-relocation-live")).unwrap();
    store.set_reference_root(Some(live)).unwrap();
    let reader = FileStore::open_read(tp.path()).unwrap();
    let live_set = BTreeSet::from([*live.bytes()]);

    let err = store
        .canonical_compaction_reclaim(&live_set, GcSegmentBudget::unlimited())
        .unwrap_err();
    assert_eq!(err.code, Code::Conflict);
    assert!(err.message.contains("active readers"));
    assert_eq!(
        reader.get(&live).unwrap().unwrap(),
        blob(b"canonical-relocation-live")
    );
}

#[test]
fn tail_compaction_aborts_on_evidence_drift() {
    let tp = TempPath::new("tail-compact-drift");
    let mut store = FileStore::open(tp.path()).unwrap();
    {
        let mut inner = store.inner.lock().unwrap();
        let free_pages = 512;
        inner.free.push(FreePageRun {
            start: 0,
            len: free_pages,
            freed_gen: 1,
        });
        inner.page_count = free_pages;
        inner.maintenance.physical_page_count = free_pages;
        inner.maintenance.reusable_free_pages = free_pages;
        inner.maintenance.candidate_dead_pages = free_pages;
        let mut file = store.file.lock().unwrap();
        file.grow(DATA_START + inner.page_count * PAGE_SIZE)
            .unwrap();
    }
    let live = store.put(&vec![0xE3; 300 * 1024]).unwrap();
    store.set_reference_root(Some(live)).unwrap();
    {
        let mut inner = store.inner.lock().unwrap();
        inner.generation = REUSE_SAFE_WINDOW + 10;
        for run in &mut inner.free {
            run.freed_gen = 1;
        }
    }
    let before = store.maintenance_status().unwrap().physical_page_count;

    let stats = store
        .compact_tail_once_with_pre_commit_interleave(256, 1, 512 * 1024, |observed| {
            let mut inner = observed.inner.lock().map_err(|_| poisoned())?;
            inner.generation += 1;
            Ok(())
        })
        .unwrap();
    assert!(stats.attempted);
    assert!(stats.skipped);
    assert_eq!(stats.conflicts, 1);
    assert_eq!(stats.truncated_pages, 0);
    assert_eq!(
        store.maintenance_status().unwrap().physical_page_count,
        before
    );
    assert_eq!(store.get(&live).unwrap().unwrap(), vec![0xE3; 300 * 1024]);
}

#[test]
fn gc_keeps_a_segment_that_is_mostly_live_by_pages() {
    // One multi-page large record (live) plus a single slab page of many tiny dead objects. By
    // object count the segment looks ~95% dead (1 of 21 live) and a count-based GC would relocate
    // the big live record to reclaim one slab page; by PAGES it is mostly live, so page-based GC
    // correctly leaves it alone.
    let big: Vec<u8> = {
        let mut s = 0x51A7_u64 | 1;
        (0..16_000u32)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                (s >> 24) as u8
            })
            .collect()
    };
    let tp = TempPath::new("gc-page-ratio");
    let mut store = FileStore::open(tp.path()).unwrap();
    let big_live = store.put(&blob(&big)).unwrap(); // a multi-page large run
    let tiny: Vec<Vec<u8>> = (0..20).map(|i| blob(format!("t{i}").as_bytes())).collect();
    let refs: Vec<&[u8]> = tiny.iter().map(|v| v.as_slice()).collect();
    store.put_batch(&refs).unwrap(); // one shared slab page

    let live: BTreeSet<[u8; 32]> = std::iter::once(*big_live.bytes()).collect();
    let stats = store.gc_segments(&live).unwrap();
    assert_eq!(
        stats.segments_reclaimed, 0,
        "a segment that is mostly live by pages must not be collected"
    );
    assert_eq!(stats.pages_trimmed, 0);
    assert_eq!(store.len(), 21); // nothing dropped (count-based GC would have dropped the 20 tiny)
    assert!(store.has(&big_live).unwrap());
}

#[test]
fn truncate_trailing_drops_only_the_top_free_run() {
    let run = |start, len| FreePageRun {
        start,
        len,
        freed_gen: 0,
    };
    // Trailing run [7,10) is free -> shrink to 7; [2,4) stays.
    let (pc, runs) = truncate_trailing(vec![run(2, 2), run(7, 3)], 10);
    assert_eq!((pc, runs), (7, vec![run(2, 2)]));
    // A live page at the top (no free run ends at page_count) blocks the shrink.
    let (pc, runs) = truncate_trailing(vec![run(2, 2)], 10);
    assert_eq!((pc, runs), (10, vec![run(2, 2)]));
    // Adjacent trailing runs collapse together.
    let (pc, runs) = truncate_trailing(vec![run(0, 1), run(5, 2), run(7, 3)], 10);
    assert_eq!((pc, runs), (5, vec![run(0, 1)]));
    // A wholly free array shrinks to zero.
    let (pc, runs) = truncate_trailing(vec![run(0, 10)], 10);
    assert_eq!(pc, 0);
    assert!(runs.is_empty());
}

#[test]
fn decoders_never_panic_on_arbitrary_bytes() {
    // A lightweight fuzz: throw pseudo-random byte buffers of many lengths at every on-disk
    // decoder and require a clean Result/Option, never a panic (no out-of-bounds slice, integer
    // overflow, or huge allocation from a crafted length).
    fn xorshift(s: &mut u64) -> u64 {
        *s ^= *s << 13;
        *s ^= *s >> 7;
        *s ^= *s << 17;
        *s
    }
    let mut s = 0x1234_5678_9abc_def0u64;
    for _ in 0..20_000 {
        let len = (xorshift(&mut s) % 600) as usize;
        let mut buf = Vec::with_capacity(len);
        for _ in 0..len {
            buf.push((xorshift(&mut s) >> 33) as u8);
        }
        let _ = page::RegionTable::decode(&buf);
        let _ = pagemap::decode(&buf);
        let _ = record::read_slab_slot(&buf, (xorshift(&mut s) % 256) as u32);
        let _ = record::decode_large(&buf);
        let _ = record::large_blob_len(&buf);
        let _ = journal::decode(&buf);
        let _ = decode_record(&buf, &Digest::blake3(&buf), None, Algo::Blake3);
        let mut pos = (xorshift(&mut s) as usize) % (len + 1);
        let _ = record::RecordLoc::decode(&buf, &mut pos);
        let mut pos = (xorshift(&mut s) as usize) % (len + 1);
        let _ = get_uvarint(&buf, &mut pos);
    }
}

fn legacy_extent_bytes(len: u64, freed_gen: u64) -> Vec<u8> {
    let mut bytes = b"LFMEXT1\0".to_vec();
    bytes.extend_from_slice(&len.to_le_bytes());
    bytes.extend_from_slice(&freed_gen.to_le_bytes());
    let checksum = crc32c(&bytes);
    bytes.extend_from_slice(&checksum.to_le_bytes());
    bytes
}

fn legacy_extent_key(start: u64) -> [u8; 32] {
    let mut key = [0u8; 32];
    key[24..].copy_from_slice(&start.to_be_bytes());
    key
}

#[test]
fn legacy_recordloc_free_map_inventory_is_complete_and_strict() {
    let mut file = MemoryBacking::new();
    file.grow(DATA_START + 1_024 * PAGE_SIZE).unwrap();
    let mut alloc = PageAllocator::new(1_024, 7, Vec::new());
    let runs = (0..150u64)
        .map(|index| FreePageRun {
            start: 100 + index * 2,
            len: 1,
            freed_gen: 3,
        })
        .collect::<Vec<_>>();
    let payloads = runs
        .iter()
        .map(|run| {
            (
                Digest::blake3(&run.start.to_le_bytes()),
                legacy_extent_bytes(run.len, run.freed_gen),
            )
        })
        .collect::<Vec<_>>();
    let borrowed = payloads
        .iter()
        .map(|(digest, bytes)| (*digest.bytes(), bytes.as_slice()))
        .collect::<Vec<_>>();
    let placements =
        record_io::write_dedicated_blob_pages(&mut file, &mut alloc, &borrowed).unwrap();
    let entries = runs
        .iter()
        .zip(placements)
        .map(|(run, (_, loc))| (legacy_extent_key(run.start), loc))
        .collect::<Vec<_>>();
    let root = pagebtree::build_packed(&mut file, DATA_START, &mut alloc, &entries)
        .unwrap()
        .unwrap();
    let inventory = pagemap::read_legacy_recordloc_map_for_promotion(
        &mut file,
        DATA_START,
        root,
        alloc.page_count(),
    )
    .unwrap();
    assert_eq!(inventory.runs, runs);
    assert!(inventory.tree_pages.len() > 1);
    assert_eq!(inventory.blob_pages.len(), entries.len());
    assert!(inventory.tree_pages.is_disjoint(&inventory.blob_pages));

    let first_blob = *inventory.blob_pages.iter().next().unwrap();
    let offset = PageId(first_blob).offset(DATA_START) + 14;
    file.pwrite(offset, &[0xff]).unwrap();
    let error = pagemap::read_legacy_recordloc_map_for_promotion(
        &mut file,
        DATA_START,
        root,
        alloc.page_count(),
    )
    .unwrap_err();
    assert_eq!(error.code, Code::CorruptObject);
}

#[test]
fn btree_root_depth_diagnostics_accept_typed_free_map() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    for index in 0..8u64 {
        store.put(&blob(&index.to_le_bytes())).unwrap();
    }

    let depths = store.btree_root_depths_for_test().unwrap();
    assert!(depths.iter().any(|depth| depth.root == "free_map"));
}

fn promote_loaded_legacy_free_map(store: &FileStore) -> Result<()> {
    let mut inner = store.inner.lock().map_err(|_| poisoned())?;
    let new_generation = inner.generation.saturating_add(1);
    let mut alloc = PageAllocator::new(inner.page_count, new_generation, inner.free.clone());
    alloc.install_metadata_bootstrap_reserve(&inner.metadata_bootstrap_reserve)?;
    let root_inputs = record_io::TxnRootInputs {
        object_index: inner.index_root,
        legacy_overlay: inner.overlay_root,
        current_records: inner.current_record_root,
        root_catalog: record_io::TxnRootCatalog {
            root: inner.root_catalog_root,
            entries: inner.root_catalog_entries.clone(),
        },
        previous_mutable_overlay_generation_floor: inner.mutable_overlay_generation_floor,
        mutable_overlay_generation_floor: inner.mutable_overlay_generation_floor,
        reference: inner.reference_root.map(|root| *root.bytes()),
        control: inner.control_root.map(|root| *root.bytes()),
    };
    let mut file = store.file.lock().map_err(|_| poisoned())?;
    let roots = record_io::finish_txn(
        &mut **file,
        &mut alloc,
        new_generation,
        inner.maintenance.object_count,
        root_inputs,
        inner.open_segment,
        &inner.maintenance,
        &BTreeSet::new(),
        (None, inner.region_table_root, inner.maintenance_root),
        inner.encryption_meta.clone(),
        store.digest_algo,
        Some(&store.group_commit_metrics),
    )?;
    drop(file);
    store.adopt_committed_roots_locked(&mut inner, roots)
}

fn streaming_file_fingerprint(path: &Path) -> (u64, u64) {
    use std::hash::Hasher;
    use std::io::Read;

    let mut file = File::open(path).unwrap();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut total = 0u64;
    let mut chunk = vec![0u8; 1024 * 1024];
    loop {
        let read = file.read(&mut chunk).unwrap();
        if read == 0 {
            break;
        }
        hasher.write(&chunk[..read]);
        total += read as u64;
    }
    (hasher.finish(), total)
}

#[test]
#[ignore = "diagnostic: one-off copied-store promotion; requires LOOM_PROMOTION_SOURCE and LOOM_PROMOTION_DEST"]
fn offline_promote_legacy_free_map_copy() {
    let source = PathBuf::from(std::env::var_os("LOOM_PROMOTION_SOURCE").unwrap());
    let destination = PathBuf::from(std::env::var_os("LOOM_PROMOTION_DEST").unwrap());
    assert_ne!(source, destination);
    assert!(
        !destination.exists(),
        "promotion destination must not exist"
    );

    let source_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&source)
        .unwrap();
    acquire_write_lock(&source_file).unwrap();
    let before_meta = std::fs::metadata(&source).unwrap();
    let before_modified = before_meta.modified().unwrap();
    let before_digest = streaming_file_fingerprint(&source);
    eprintln!("promotion phase=copy bytes={}", before_digest.1);
    std::fs::copy(&source, &destination).unwrap();

    let store = FileStore::open(&destination).unwrap();
    let inventory = pagemap::take_legacy_promotion_inventory()
        .expect("promotion open must inventory the legacy free map");
    let before_roots = {
        let inner = store.inner.lock().unwrap();
        (
            inner.index_root,
            inner.overlay_root,
            inner.current_record_root,
            inner.root_catalog_root,
            inner.root_catalog_entries.clone(),
            inner.reference_root,
            inner.control_root,
            inner.maintenance.object_count,
            inner.generation,
            inner.region_table_root,
            inner.maintenance_root,
        )
    };
    eprintln!(
        "promotion phase=publish runs={} tree_pages={} blob_pages={}",
        inventory.runs.len(),
        inventory.tree_pages.len(),
        inventory.blob_pages.len()
    );
    promote_loaded_legacy_free_map(&store).unwrap();
    let expected_generation = before_roots.8 + 1;
    let after_roots = {
        let inner = store.inner.lock().unwrap();
        assert_eq!(inner.generation, expected_generation);
        assert!(inner.freemap.is_some());
        (
            inner.index_root,
            inner.overlay_root,
            inner.current_record_root,
            inner.root_catalog_root,
            inner.root_catalog_entries.clone(),
            inner.reference_root,
            inner.control_root,
            inner.maintenance.object_count,
            inner.free.clone(),
            inner.freemap.unwrap().0,
            inner.metadata_bootstrap_reserve.clone(),
            inner.region_table_root,
            inner.maintenance_root,
        )
    };
    assert_eq!(after_roots.0, before_roots.0);
    assert_eq!(after_roots.1, before_roots.1);
    assert_eq!(after_roots.2, before_roots.2);
    assert_eq!(after_roots.3, before_roots.3);
    assert_eq!(after_roots.4, before_roots.4);
    assert_eq!(after_roots.5, before_roots.5);
    assert_eq!(after_roots.6, before_roots.6);
    assert_eq!(after_roots.7, before_roots.7);
    let retired = inventory
        .tree_pages
        .iter()
        .chain(inventory.blob_pages.iter())
        .copied()
        .collect::<BTreeSet<_>>();
    let page_count = store.inner.lock().unwrap().page_count;
    let typed_pages = {
        let mut file = store.file.lock().unwrap();
        pagebtree::collect_free_page_extent_pages(
            &mut **file,
            DATA_START,
            after_roots.9,
            page_count,
        )
        .unwrap()
        .into_iter()
        .map(|page| page.0)
        .collect::<BTreeSet<_>>()
    };
    assert!(typed_pages.iter().all(|page| {
        !after_roots
            .8
            .iter()
            .any(|run| *page >= run.start && *page < run.start.saturating_add(run.len))
    }));
    let bootstrap_pages = after_roots.10.pages().collect::<BTreeSet<_>>();
    assert!(bootstrap_pages.is_disjoint(&typed_pages));
    let current_metadata_pages = typed_pages
        .iter()
        .copied()
        .chain(after_roots.11.map(|page| page.0))
        .chain(after_roots.12.map(|page| page.0))
        .collect::<BTreeSet<_>>();
    assert!(current_metadata_pages.iter().all(|page| {
        !after_roots
            .8
            .iter()
            .any(|run| *page >= run.start && *page < run.start.saturating_add(run.len))
    }));
    for page in retired {
        let is_free = after_roots
            .8
            .iter()
            .any(|run| page >= run.start && page < run.start.saturating_add(run.len));
        assert!(
            is_free || current_metadata_pages.contains(&page),
            "retired legacy free-map page {page} is neither free nor current metadata; legacy_tree={:?} legacy_blob={:?}",
            inventory.tree_pages,
            inventory.blob_pages,
        );
    }
    drop(store);

    let reopened = FileStore::open_read(&destination).unwrap();
    assert_eq!(reopened.reference_root(), before_roots.5);
    assert_eq!(reopened.control_root(), before_roots.6);
    assert_eq!(reopened.len() as u64, before_roots.7);
    drop(reopened);
    let reopened_loom = open_loom(&destination).unwrap();
    let workspace_count = reopened_loom.registry().list(None).len();
    assert!(workspace_count > 0, "promoted Loom has no workspaces");
    eprintln!("promotion phase=loom-reopen workspaces={workspace_count}");
    drop(reopened_loom);
    assert_eq!(streaming_file_fingerprint(&source), before_digest);
    assert_eq!(
        std::fs::metadata(&source).unwrap().modified().unwrap(),
        before_modified
    );
    drop(source_file);
    eprintln!(
        "promotion phase=complete destination={}",
        destination.display()
    );
}

#[test]
fn online_truncate_shrinks_a_freed_trailing_region() {
    let tp = TempPath::new("truncate");
    let store = FileStore::open(tp.path()).unwrap();
    let anchor = store.put(&blob(b"anchor")).unwrap();
    // Build up: the per-commit region/free-map churn repeatedly extends the top of the file.
    let mut small = Vec::new();
    for i in 0..120u32 {
        small.push(store.put(&blob(format!("s-{i:04}").as_bytes())).unwrap());
    }
    let peak = store.logical_end();
    // Subsequent commits place their region/map pages on low aged holes instead of extending, so
    // the build-up's trailing churn pages become free; once aged they are truncated and the file
    // shrinks below its peak.
    for _ in 0..40 {
        store.set_reference_root(Some(anchor)).unwrap();
        store.set_reference_root(None).unwrap();
    }
    let after = store.logical_end();
    assert!(
        after < peak,
        "online truncate should shrink the file below its peak: peak={peak} after={after}"
    );
    // The shrink loses nothing: every object still reads and the file reopens intact.
    assert!(store.has(&anchor).unwrap());
    for d in &small {
        assert!(store.has(d).unwrap());
    }
    drop(store);
    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(store.len(), 121);
    assert!(store.has(&anchor).unwrap());
}

#[test]
fn compact_reclaims_dead_space_and_preserves_objects() {
    let tp = TempPath::new("compact");
    let mut store = FileStore::open(tp.path()).unwrap();
    let n = 300usize;
    let mut digests = Vec::with_capacity(n);
    for i in 0..n {
        // Each individual put CoW-rewrites the B-tree path, leaving dead nodes behind.
        digests.push(store.put(&blob(format!("obj-{i}").as_bytes())).unwrap());
    }
    store.set_reference_root(Some(digests[0])).unwrap(); // point the reference at a real, stored object
    store
        .control_set(b"lock/ns/fence", b"300".to_vec())
        .unwrap();

    let stats = store.compact().unwrap();
    assert!(
        stats.after < stats.before,
        "compaction should reclaim dead B-tree nodes: before={} after={}",
        stats.before,
        stats.after
    );
    assert!(stats.reclaimed() > 0);
    // Everything is intact post-compaction: count, every object, and the reference root.
    assert_eq!(store.len(), n + 1);
    assert_eq!(store.reference_root(), Some(digests[0]));
    assert_eq!(
        store.control_get(b"lock/ns/fence").unwrap().as_deref(),
        Some(&b"300"[..])
    );
    for (i, d) in digests.iter().enumerate() {
        assert_eq!(
            store.get(d).unwrap().unwrap(),
            blob(format!("obj-{i}").as_bytes())
        );
    }
    // ...and it all survives a reopen of the freshly compacted file.
    drop(store);
    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(store.len(), n + 1);
    assert_eq!(store.reference_root(), Some(digests[0]));
    assert_eq!(
        store.control_get(b"lock/ns/fence").unwrap().as_deref(),
        Some(&b"300"[..])
    );
}

#[test]
fn compaction_capacity_reports_required_temp_bytes() {
    let tp = TempPath::new("compact-capacity");
    let store = FileStore::open(tp.path()).unwrap();
    let d = store.put(b"capacity").unwrap();
    store.set_reference_root(Some(d)).unwrap();
    let capacity = store.compaction_capacity().unwrap();
    assert!(capacity.required_temp_bytes >= DATA_START);
    #[cfg(unix)]
    assert!(capacity.available_temp_bytes.unwrap() >= capacity.required_temp_bytes);
    store.ensure_compaction_capacity().unwrap();
}

#[test]
fn compact_preserves_a_full_loom() {
    use loom_core::WsSelector;
    use loom_core::workspace::{FacetKind, WorkspaceId};

    let tp = TempPath::new("compact-loom");
    {
        let mut loom = open_loom(tp.path()).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("p"),
                WorkspaceId::from_bytes([3; 16]),
            )
            .unwrap();
        // Churn: repeated edits + commits + saves leave dead engine-state blobs and B-tree nodes.
        for i in 0..6u64 {
            loom.write_file(ns, "f.txt", format!("v{i}").as_bytes(), 0o100644)
                .unwrap();
            loom.commit(ns, "nas", "edit", i + 1).unwrap();
            save_loom(&mut loom).unwrap();
        }
        loom.store_mut().compact().unwrap();
    }
    // Reopen the compacted file as a full Loom: refs + working tree must round-trip.
    let loom = open_loom(tp.path()).unwrap();
    let ns = loom
        .registry()
        .open(&WsSelector::Typed {
            ty: FacetKind::Files,
            name: "p".to_string(),
        })
        .unwrap();
    assert_eq!(loom.read_file(ns, "f.txt").unwrap(), b"v5");
}

#[test]
fn gc_drops_unreachable_engine_state_blobs_and_keeps_history() {
    use loom_core::WsSelector;
    use loom_core::workspace::{FacetKind, WorkspaceId};

    let tp = TempPath::new("gc");
    {
        let mut loom = open_loom(tp.path()).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("p"),
                WorkspaceId::from_bytes([5; 16]),
            )
            .unwrap();
        // Churn: each save_loom writes a NEW engine-state blob, so the prior ones become
        // unreachable garbage (not ref-reachable, not the current reference root).
        for i in 0..6u64 {
            loom.write_file(ns, "f.txt", format!("v{i}").as_bytes(), 0o100644)
                .unwrap();
            loom.commit(ns, "nas", "edit", i + 1).unwrap();
            save_loom(&mut loom).unwrap();
        }
        let before = loom.store().len();
        let stats = gc_loom(&mut loom).unwrap();
        // The five superseded engine-state blobs are gone; the object count drops and space is freed.
        assert!(
            loom.store().len() < before,
            "GC should drop stale engine-state blobs: before={before}, after={}",
            loom.store().len()
        );
        assert!(stats.reclaimed() > 0);
        // Committed history is intact: HEAD content still reads after GC.
        assert_eq!(loom.read_file(ns, "f.txt").unwrap(), b"v5");
    }
    // ...and the GC'd file reopens as a full Loom.
    let loom = open_loom(tp.path()).unwrap();
    let ns = loom
        .registry()
        .open(&WsSelector::Typed {
            ty: FacetKind::Files,
            name: "p".to_string(),
        })
        .unwrap();
    assert_eq!(loom.read_file(ns, "f.txt").unwrap(), b"v5");
}

#[test]
fn default_codec_compresses_a_large_object() {
    let tp = TempPath::new("compress");
    let store = FileStore::open(tp.path()).unwrap(); // default Deflate
    let data = blob(&b"loom object commit tree branch ".repeat(2200)); // ~68 KiB, repetitive
    let before = std::fs::metadata(tp.path()).unwrap().len();
    let d = store.put(&data).unwrap();
    let after = std::fs::metadata(tp.path()).unwrap().len();
    // The file grew far less than the plaintext size: the record was stored compressed.
    assert!(
        after - before < data.len() as u64 / 2,
        "expected compression: file grew {} for a {}-byte plaintext",
        after - before,
        data.len()
    );
    // ...and `get` still returns the exact plaintext (frame inverted + integrity-verified).
    assert_eq!(store.get(&d).unwrap().unwrap(), data);
}

#[test]
fn frame_independent_across_store_codecs() {
    // The same content stored under different codecs has the same digest and yields identical
    // plaintext on read: the property that makes compression invisible to sync.
    let data = blob(&b"the loom content-addressed object store ".repeat(1500));
    let tp_d = TempPath::new("fi-deflate");
    let tp_l = TempPath::new("fi-lz4");
    let tp_n = TempPath::new("fi-none");

    let sd = FileStore::open(tp_d.path()).unwrap(); // Deflate (default)
    let mut sl = FileStore::open(tp_l.path()).unwrap();
    sl.set_default_codec(Codec::Lz4);
    let mut sn = FileStore::open(tp_n.path()).unwrap();
    sn.set_default_codec(Codec::None);

    let dd = sd.put(&data).unwrap();
    let dl = sl.put(&data).unwrap();
    let dn = sn.put(&data).unwrap();
    assert_eq!(dd, dl, "digest must be codec-independent");
    assert_eq!(dd, dn);
    for s in [&sd, &sl, &sn] {
        assert_eq!(s.get(&dd).unwrap().unwrap(), data);
    }
    // Self-describing on reopen: each frame round-trips without knowing the writer's codec.
    drop((sd, sl, sn));
    for tp in [&tp_d, &tp_l, &tp_n] {
        assert_eq!(
            FileStore::open(tp.path())
                .unwrap()
                .get(&dd)
                .unwrap()
                .unwrap(),
            data
        );
    }
}

#[test]
fn put_hint_applies_the_per_call_codec() {
    // `put_hint` maps the engine's CompressionHint to a frame per write, independent of the store
    // default. `Small` compresses; `None` stores identity. Both round-trip and share a digest.
    let data = blob(&b"loom commit tree branch object store ".repeat(2000)); // repetitive, >1 KiB
    let tp_s = TempPath::new("hint-small");
    let tp_n = TempPath::new("hint-none");

    let ss = FileStore::open(tp_s.path()).unwrap();
    let sn = FileStore::open(tp_n.path()).unwrap();

    let before_s = std::fs::metadata(tp_s.path()).unwrap().len();
    let ds = ss.put_hint(&data, CompressionHint::Small).unwrap();
    let grew_s = std::fs::metadata(tp_s.path()).unwrap().len() - before_s;

    let before_n = std::fs::metadata(tp_n.path()).unwrap().len();
    let dn = sn.put_hint(&data, CompressionHint::None).unwrap();
    let grew_n = std::fs::metadata(tp_n.path()).unwrap().len() - before_n;

    assert_eq!(ds, dn, "the hint must not affect the digest");
    assert!(
        grew_s < grew_n,
        "Small hint should compress (grew {grew_s}) vs None identity (grew {grew_n})"
    );
    assert_eq!(ss.get(&ds).unwrap().unwrap(), data);
    assert_eq!(sn.get(&dn).unwrap().unwrap(), data);
}

#[test]
fn second_writer_is_locked_out_until_the_first_drops() {
    let tp = TempPath::new("writer-lock");
    let a = FileStore::open(tp.path()).unwrap();
    // A second writer process (handle) cannot open the same loom while the first holds it.
    let err = FileStore::open(tp.path()).unwrap_err();
    assert_eq!(err.code, Code::Conflict);
    // Dropping the first releases the lock, so a new writer can open.
    drop(a);
    FileStore::open(tp.path()).unwrap();
}

#[test]
fn readers_are_lock_free_and_do_not_block_a_writer() {
    let tp = TempPath::new("read-lock-free");
    let data = blob(b"hello loom reader");
    let digest = {
        let w = FileStore::open(tp.path()).unwrap();
        w.put(&data).unwrap()
    };
    // A lock-free reader sees the committed object.
    let r = FileStore::open_read(tp.path()).unwrap();
    assert_eq!(r.get(&digest).unwrap().unwrap(), data);
    // The open reader does not block a writer (readers hold no lock)...
    let _w = FileStore::open(tp.path()).unwrap();
    // ...and a second reader coexists with that writer.
    let r2 = FileStore::open_read(tp.path()).unwrap();
    assert_eq!(r2.get(&digest).unwrap().unwrap(), data);
}

#[test]
fn ring_recovery_restores_the_reference_root_too() {
    // The reference (engine-state) root rides in every ring record, so a state set after only a few
    // commits - with no superblock checkpoint - is still recovered from the ring on reopen.
    let tp = TempPath::new("journal-reference");
    let (da, root) = {
        let s = FileStore::open(tp.path()).unwrap();
        let da = s.put(&blob(b"object-A")).unwrap(); // gen 1
        let root = s.put(&blob(b"reference-state")).unwrap(); // gen 2
        s.set_reference_root(Some(root)).unwrap(); // gen 3, all below CHECKPOINT_INTERVAL
        (da, root)
    };
    let s = FileStore::open_read(tp.path()).unwrap();
    assert!(s.get(&da).unwrap().is_some());
    assert_eq!(
        s.reference_root(),
        Some(root),
        "the reference root must be recovered from the ring with no checkpoint"
    );
}

#[test]
fn t188_24a_object_only_publication_and_recovery_remain_canonical() {
    let tp = TempPath::new("t188-24a-object-only");
    let object = blob(b"t188-24a-object-only");
    let (digest, region_table_root, generation) = {
        let store = FileStore::open(tp.path()).unwrap();
        let digest = store.put(&object).unwrap();
        let roots = t188_15_roots(&store);
        assert!(roots.index_root.is_some());
        assert_eq!(roots.overlay_root, None);
        assert_eq!(roots.current_record_root, None);
        assert_eq!(roots.root_catalog_root, None);
        (digest, roots.region_table_root.unwrap(), roots.generation)
    };

    let journal_bytes = std::fs::read(tp.path()).unwrap();
    let journal_roots = t188_15_newest_journal_roots(&journal_bytes);
    assert_eq!(journal_roots.generation, generation);
    assert_eq!(journal_roots.region_table, Some(region_table_root));
    let region_offset = (DATA_START + region_table_root.0 * PAGE_SIZE) as usize;
    assert_eq!(&journal_bytes[region_offset..region_offset + 4], b"LRT4");

    {
        let reopened = FileStore::open(tp.path()).unwrap();
        assert_eq!(
            reopened.get(&digest).unwrap().as_deref(),
            Some(object.as_slice())
        );
        let roots = t188_15_roots(&reopened);
        assert_eq!(roots.generation, generation);
        assert_eq!(roots.region_table_root, Some(region_table_root));
        assert!(roots.index_root.is_some());
        assert_eq!(roots.overlay_root, None);
        assert_eq!(roots.current_record_root, None);
        assert_eq!(roots.root_catalog_root, None);
    }

    let checkpoint_bytes = std::fs::read(tp.path()).unwrap();
    let checkpoint = [
        &checkpoint_bytes[..SLOT_SIZE as usize],
        &checkpoint_bytes[SLOT_SIZE as usize..2 * SLOT_SIZE as usize],
    ]
    .into_iter()
    .filter_map(|slot| {
        let slot: &[u8; SLOT_SIZE as usize] = slot.try_into().unwrap();
        Superblock::decode(slot)
    })
    .max_by_key(|slot| slot.generation)
    .unwrap();
    assert_eq!(checkpoint.generation, generation);
    assert_eq!(checkpoint.region_table, Some(region_table_root));
    assert_eq!(&checkpoint_bytes[region_offset..region_offset + 4], b"LRT4");

    let reopened = FileStore::open(tp.path()).unwrap();
    assert_eq!(
        reopened.get(&digest).unwrap().as_deref(),
        Some(object.as_slice())
    );
}

#[test]
fn open_read_rejects_a_missing_loom() {
    let tp = TempPath::new("read-missing");
    assert!(FileStore::open_read(tp.path()).is_err());
}

#[test]
fn freemap_survives_a_reuse_heavy_workload() {
    // Enough distinct puts to drive the B-tree multi-level and well past the reuse window, so later
    // commits genuinely reuse aged superseded-node extents. The allocator unit tests above prove
    // the reuse mechanism; this proves the store stays correct when its on-disk B-tree nodes partly
    // live in reused holes and online tail trimming moves the logical end in either direction.
    let n = 400usize;
    let tp = TempPath::new("freemap-workload");
    let end = {
        let store = FileStore::open(tp.path()).unwrap();
        for i in 0..n {
            store.put(&blob(format!("obj-{i:08}").as_bytes())).unwrap();
            assert!(store.logical_end() >= DATA_START);
        }
        assert_eq!(store.len(), n);
        store.logical_end()
    };
    let reopened = FileStore::open(tp.path()).unwrap();
    assert_eq!(reopened.logical_end(), end); // recovered state matches what was committed
    assert_eq!(reopened.len(), n);
    for i in 0..n {
        let d = Digest::blake3(&blob(format!("obj-{i:08}").as_bytes()));
        assert!(reopened.has(&d).unwrap(), "object {i} lost after reuse");
    }
}

#[test]
fn freemap_persists_across_reopen() {
    // The free list is written to disk each commit and restored on open, so reuse survives a
    // restart instead of starting empty. After a churning workload, the reopened store's free list
    // must match the one committed at close (as a set; on-open validation returns it sorted).
    let n = 200usize;
    let tp = TempPath::new("freemap-persist");
    let mut before = {
        let store = FileStore::open(tp.path()).unwrap();
        for i in 0..n {
            store.put(&blob(format!("obj-{i:08}").as_bytes())).unwrap();
        }
        store.free_runs()
    };
    before.sort_by_key(|r| r.start);
    assert!(
        !before.is_empty(),
        "the workload should have freed superseded CoW node pages"
    );
    let reopened = FileStore::open(tp.path()).unwrap();
    let mut after = reopened.free_runs();
    after.sort_by_key(|r| r.start);
    assert_eq!(
        after, before,
        "the free-page map must be restored across a reopen"
    );
    // And a subsequent put still lands correctly with the restored free list in play.
    let d = reopened.put(&blob(b"after-reopen")).unwrap();
    assert!(reopened.has(&d).unwrap());
}

#[test]
fn concurrent_writers_share_one_store() {
    use std::sync::Arc;
    // The store takes `&self` writes, so one `FileStore` is shared across threads via `Arc`. Writes
    // funnel through the group-commit coordinator: under contention a leader commits many threads'
    // objects in one fsync while the rest wait, then later arrivals lead the next batch. This
    // storms the leader/follower handoff; every distinct object must land and stay retrievable.
    let tp = TempPath::new("concurrent");
    let store = Arc::new(FileStore::open(tp.path()).unwrap());
    let mut handles = Vec::new();
    for t in 0..8u32 {
        let s = Arc::clone(&store);
        handles.push(std::thread::spawn(move || {
            let mut mine = Vec::new();
            for i in 0..50u32 {
                mine.push(s.put(&blob(format!("obj-{t}-{i}").as_bytes())).unwrap());
            }
            mine
        }));
    }
    let digests: Vec<Digest> = handles
        .into_iter()
        .flat_map(|h| h.join().unwrap())
        .collect();
    assert_eq!(store.len(), 400); // 8 threads x 50 distinct objects, all committed
    // Each digest resolves through the index to its record, intact - no coalesced write was lost
    // or pointed at the wrong offset.
    for (t, d) in digests.iter().enumerate() {
        let want = blob(format!("obj-{}-{}", t / 50, t % 50).as_bytes());
        assert_eq!(store.get(d).unwrap().as_deref(), Some(want.as_slice()));
    }
}

#[test]
fn crafted_bogus_index_root_is_clean_error_not_panic() {
    // A committed file whose index-root page is corrupted: loading the index on open must be a
    // clean CORRUPT error - no panic, no wild read - because every node page is CRC- and
    // bound-checked. The index root is located via the newest committed journal record.
    let (mut bytes, digests) = build_committed(100); // > one node: forces a multi-node tree
    let mut newest: Option<journal::Roots> = None;
    for i in 0..RING_SLOTS {
        let off = (JOURNAL_OFFSET + i * journal::RECORD_SIZE as u64) as usize;
        if let Some((journal::KIND_COMMIT, r)) =
            journal::decode(&bytes[off..off + journal::RECORD_SIZE])
            && newest.is_none_or(|n| r.generation > n.generation)
        {
            newest = Some(r);
        }
    }
    let rt = newest.unwrap().region_table.unwrap();
    let rt_off = (DATA_START + rt.0 * PAGE_SIZE) as usize;
    let region = RegionTable::decode(&bytes[rt_off..rt_off + PAGE_SIZE as usize]).unwrap();
    let index_root = region.index_root.unwrap();
    // Flip every byte of the index-root page: its magic and CRC checks must reject it.
    let node_off = (DATA_START + index_root.0 * PAGE_SIZE) as usize;
    for b in &mut bytes[node_off..node_off + PAGE_SIZE as usize] {
        *b ^= 0xFF;
    }
    let store = open_bytes(&bytes, "bogus-index").unwrap();
    let err = store.has(&digests[0]).unwrap_err();
    assert!(matches!(err.code, Code::CorruptObject | Code::Io));
}

#[test]
fn many_objects_round_trip_through_btree() {
    // Enough objects to force several B-tree splits (order 64), then reopen and confirm every one
    // is found via the index rebuilt by walking the on-disk tree (no payload scan).
    let tp = TempPath::new("btree-many");
    let n = 500usize;
    let mut digests = Vec::with_capacity(n);
    {
        let store = FileStore::open(tp.path()).unwrap();
        for i in 0..n {
            digests.push(store.put(&blob(format!("item-{i}").as_bytes())).unwrap());
        }
        assert_eq!(store.len(), n);
    }
    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(store.len(), n);
    for (i, d) in digests.iter().enumerate() {
        assert!(store.has(d).unwrap());
        assert_eq!(
            store.get(d).unwrap().unwrap(),
            blob(format!("item-{i}").as_bytes())
        );
    }
}

#[test]
fn sparse_lookup_uses_bounded_locator_and_page_caches() {
    let tp = TempPath::new("sparse-cache");
    let n = LOCATOR_CACHE_LIMIT + 24;
    let mut digests = Vec::with_capacity(n);
    {
        let store = FileStore::open(tp.path()).unwrap();
        for i in 0..n {
            digests.push(
                store
                    .put(&blob(format!("cached-item-{i}").as_bytes()))
                    .unwrap(),
            );
        }
    }

    let store = FileStore::open(tp.path()).unwrap();
    let initial = store.io_stats().unwrap();
    assert!(!initial.open_index_materialized);
    assert_eq!(initial.locator_cache_entries, 0);

    let first = digests[0];
    assert!(store.has(&first).unwrap());
    let after_first = store.io_stats().unwrap();
    assert_eq!(after_first.locator_cache_misses, 1);
    assert!(after_first.index_pages_read > 0);
    assert_eq!(after_first.locator_cache_entries, 1);

    assert!(store.has(&first).unwrap());
    let after_cached = store.io_stats().unwrap();
    assert_eq!(after_cached.locator_cache_hits, 1);
    assert_eq!(after_cached.index_pages_read, after_first.index_pages_read);

    for digest in &digests {
        assert!(store.has(digest).unwrap());
    }
    let after_sweep = store.io_stats().unwrap();
    assert!(after_sweep.index_page_cache_hits > 0);
    assert!(after_sweep.locator_cache_entries <= LOCATOR_CACHE_LIMIT as u64);
}

#[test]
fn truncation_never_panics() {
    // Property: opening the file truncated to any length >= DATA_START either succeeds with a valid
    // committed prefix or returns a clean error - never a panic. The page format makes every page
    // boundary (and the header edge) the interesting cases, so sweep those and the bytes around
    // them plus a coarse stride; an exhaustive byte-by-byte sweep of the page-granular file is
    // needless and rewrites gigabytes.
    let (bytes, _) = build_committed(3);
    let header = DATA_START as usize;
    let mut lengths: Vec<usize> = Vec::new();
    let mut boundary = header;
    while boundary <= bytes.len() {
        for d in [0usize, 1, 2, 3, 8] {
            if boundary >= d && boundary - d >= header {
                lengths.push(boundary - d);
            }
            if boundary + d <= bytes.len() {
                lengths.push(boundary + d);
            }
        }
        boundary += PAGE_SIZE as usize;
    }
    let mut stride = header;
    while stride <= bytes.len() {
        lengths.push(stride);
        stride += 257; // a prime, to land on varied mid-page offsets
    }
    lengths.sort_unstable();
    lengths.dedup();
    for len in lengths {
        let _ = open_bytes(&bytes[..len], "trunc-sweep"); // must not panic
    }
}

#[test]
fn passes_conformance_vectors() {
    let tp = TempPath::new("conformance");
    let store = FileStore::open(tp.path()).unwrap();
    // Single backend-certification entry point: blob + object-model + table/index identity.
    uldren_loom_conformance::run_all_vectors(store).expect("all conformance vectors");
}

/// A FIPS (SHA-256) FileStore certifies against the parallel `fips/sha256` blob and object-model
/// vectors, and a default store certifies against `default/blake3` - proving the canonical bytes
/// are profile-independent and only the digest layer changes.
#[test]
fn certifies_data_model_vectors_under_both_profiles() {
    let tp_b = TempPath::new("conf-blake3");
    let mut blake3 = FileStore::open(tp_b.path()).unwrap();
    uldren_loom_conformance::run_blob_vectors_profiled(&mut blake3, Algo::Blake3)
        .expect("default profile certifies against blake3 vectors");
    uldren_loom_conformance::run_object_model_vectors_profiled(&mut blake3, Algo::Blake3)
        .expect("default profile certifies object-model vectors");

    let tp_s = TempPath::new("conf-sha256");
    let mut sha = FileStore::create_with_profile(tp_s.path(), Algo::Sha256).unwrap();
    uldren_loom_conformance::run_blob_vectors_profiled(&mut sha, Algo::Sha256)
        .expect("FIPS profile certifies against sha256 vectors");
    uldren_loom_conformance::run_object_model_vectors_profiled(&mut sha, Algo::Sha256)
        .expect("FIPS profile certifies object-model vectors");
}

/// The workspace CAS facade honors the store's identity profile: a SHA-256 (FIPS) store addresses
/// blobs with SHA-256 content addresses, and put/get/list round-trip under that profile. This is the
/// digest-profile dimension of the 0024 workspace-facade contract, which the in-memory conformance
/// runner (BLAKE3 only) cannot exercise.
#[test]
fn cas_facade_honors_sha256_profile() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{cas_get, cas_list, cas_put};

    let tp = TempPath::new("cas-sha256");
    let store = FileStore::create_with_profile(tp.path(), Algo::Sha256).unwrap();
    let mut loom = Loom::new(store);
    let ns = loom
        .registry_mut()
        .create(FacetKind::Cas, None, WorkspaceId::from_bytes([9; 16]))
        .unwrap();

    let addr = cas_put(&mut loom, ns, b"fips blob").unwrap();
    assert_eq!(
        addr.algo(),
        Algo::Sha256,
        "a FIPS store must address CAS blobs with SHA-256"
    );
    assert_eq!(
        cas_get(&loom, ns, &addr).unwrap().as_deref(),
        Some(&b"fips blob"[..]),
        "the blob round-trips under the SHA-256 profile"
    );
    assert_eq!(
        cas_list(&loom, ns).unwrap(),
        vec![addr],
        "list enumerates the SHA-256-addressed blob"
    );
}

#[test]
fn store_durability_policy_parse_and_validation_accept_contract_modes() {
    assert_eq!(
        parse_store_durability_policy("strict").unwrap(),
        StoreDurabilityPolicy::Strict
    );
    assert_eq!(
        parse_store_durability_policy("normal").unwrap(),
        StoreDurabilityPolicy::Normal
    );
    assert_eq!(
        parse_store_durability_policy("relaxed").unwrap(),
        StoreDurabilityPolicy::Relaxed
    );
    assert_eq!(
        parse_store_durability_policy("ephemeral").unwrap(),
        StoreDurabilityPolicy::Ephemeral
    );
    for policy in StoreDurabilityPolicy::ALL {
        validate_store_durability_policy(policy).unwrap();
    }
}

#[test]
fn store_durability_policy_parse_rejects_unsupported_modes() {
    let error = parse_store_durability_policy("unsafe").unwrap_err();

    assert_eq!(error.code, Code::InvalidArgument);
}

#[test]
fn store_policy_durability_defaults_and_facet_overrides_survive_reopen() {
    let tp = TempPath::new("store-durability-policy");
    {
        let store = FileStore::open(tp.path()).unwrap();
        let mut policy = store.store_policy().unwrap();
        policy
            .set_default_durability(StoreDurabilityPolicy::Relaxed)
            .unwrap();
        policy
            .set_facet_durability(FacetKind::Ledger, Some(StoreDurabilityPolicy::Strict))
            .unwrap();
        policy
            .set_facet_durability(FacetKind::Search, Some(StoreDurabilityPolicy::Ephemeral))
            .unwrap();
        store
            .save_store_policy_audited(policy, None, "store.policy.set", None)
            .unwrap();
    }

    let store = FileStore::open(tp.path()).unwrap();
    let policy = store.store_policy().unwrap();

    assert_eq!(
        policy.effective_durability(FacetKind::Document),
        StoreDurabilityPolicy::Relaxed
    );
    assert_eq!(
        policy.effective_durability(FacetKind::Ledger),
        StoreDurabilityPolicy::Strict
    );
    assert_eq!(
        policy.effective_durability(FacetKind::Search),
        StoreDurabilityPolicy::Ephemeral
    );
}

#[test]
fn store_policy_facet_overrides_cover_canonical_facet_inventory() {
    let tp = TempPath::new("store-durability-policy-all-facets");
    {
        let store = FileStore::open(tp.path()).unwrap();
        let mut policy = store.store_policy().unwrap();
        policy
            .set_default_durability(StoreDurabilityPolicy::Relaxed)
            .unwrap();
        for facet in FacetKind::ALL {
            policy
                .set_facet_durability(facet, Some(StoreDurabilityPolicy::Strict))
                .unwrap();
            assert_eq!(
                policy.effective_durability(facet),
                StoreDurabilityPolicy::Strict
            );
        }
        store
            .save_store_policy_audited(policy, None, "store.policy.set", None)
            .unwrap();
    }

    let store = FileStore::open(tp.path()).unwrap();
    let policy = store.store_policy().unwrap();
    assert_eq!(
        FacetKind::ALL.len(),
        policy.facet_durability_overrides.len()
    );
    for facet in FacetKind::ALL {
        assert_eq!(
            policy.effective_durability(facet),
            StoreDurabilityPolicy::Strict
        );
    }
    assert_eq!(
        policy.effective_durability(FacetKind::Inference),
        StoreDurabilityPolicy::Strict
    );

    let inference_artifact = DerivedArtifactKey::new(
        WorkspaceId::from_bytes([177; 16]),
        FacetKind::Inference,
        "requests",
        "provider-cache",
    )
    .unwrap();
    assert_eq!(
        store
            .derived_artifact_durability(&inference_artifact, None)
            .unwrap(),
        StoreDurabilityPolicy::Strict
    );
}

#[test]
fn store_policy_rejects_malformed_durability_configuration() {
    let tp = TempPath::new("store-durability-policy-invalid");
    let store = FileStore::open(tp.path()).unwrap();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(STORE_POLICY_MAGIC);
    bytes.push(2);
    bytes.push(0);
    bytes.push(9);
    bytes.extend_from_slice(&0u16.to_be_bytes());
    store.control_set(STORE_POLICY_KEY, bytes).unwrap();

    let error = store.store_policy().unwrap_err();

    assert_eq!(error.code, Code::CorruptObject);
}

fn reclaim_state_without_blockers() -> MutableOverlayReclaimState {
    MutableOverlayReclaimState {
        superseded_generation: 10,
        superseding_generation: 12,
        latest_index_generation: 12,
        oldest_pinned_snapshot_generation: None,
        retained_history_generation: None,
        audit_retention_active: false,
        tombstone_masks_base: false,
        durable_reclaim_floor: 12,
        strict_promotion_generation: None,
    }
}

#[test]
fn mutable_overlay_reclaim_eligibility_allows_unpinned_superseded_current_record() {
    let state = reclaim_state_without_blockers();

    assert_eq!(state.blockers().unwrap(), Vec::new());
    assert!(state.is_eligible().unwrap());
}

#[test]
fn mutable_overlay_reclaim_eligibility_blocks_visible_current_and_retained_views() {
    let state = MutableOverlayReclaimState {
        latest_index_generation: 10,
        oldest_pinned_snapshot_generation: Some(11),
        retained_history_generation: Some(10),
        strict_promotion_generation: Some(10),
        ..reclaim_state_without_blockers()
    };

    assert_eq!(
        state.blockers().unwrap(),
        vec![
            MutableOverlayReclaimBlocker::CurrentIndexVisible,
            MutableOverlayReclaimBlocker::PinnedSnapshot,
            MutableOverlayReclaimBlocker::RetainedHistory,
            MutableOverlayReclaimBlocker::StrictPromotionBoundary,
        ]
    );
    assert!(!state.is_eligible().unwrap());
}

#[test]
fn mutable_overlay_reclaim_eligibility_blocks_policy_and_durability_windows() {
    let state = MutableOverlayReclaimState {
        audit_retention_active: true,
        tombstone_masks_base: true,
        durable_reclaim_floor: 11,
        ..reclaim_state_without_blockers()
    };

    assert_eq!(
        state.blockers().unwrap(),
        vec![
            MutableOverlayReclaimBlocker::AuditRetention,
            MutableOverlayReclaimBlocker::TombstoneRetention,
            MutableOverlayReclaimBlocker::DurableGenerationWindow,
        ]
    );
    assert!(!state.is_eligible().unwrap());
}

#[test]
fn mutable_overlay_reclaim_eligibility_rejects_invalid_generation_order() {
    let state = MutableOverlayReclaimState {
        superseding_generation: 10,
        ..reclaim_state_without_blockers()
    };
    let error = state.blockers().unwrap_err();

    assert_eq!(error.code, Code::InvalidArgument);
}

fn durability_test_key(name: &str) -> OverlayKey {
    OverlayKey::from_segments([
        b"workspace",
        &[9; 16],
        b"documents",
        b"durability",
        b"current",
        name.as_bytes(),
    ])
    .unwrap()
}

fn durability_facet_test_key(facet: &[u8], name: &str) -> OverlayKey {
    OverlayKey::from_segments([
        b"workspace",
        &[10; 16],
        facet,
        b"durability",
        b"current",
        name.as_bytes(),
    ])
    .unwrap()
}

fn workflow_transaction_test(
    _name: &str,
    writes: Vec<FacetWrite>,
    idempotency: Option<&[u8]>,
) -> WorkflowTransaction {
    WorkflowTransaction {
        workspace: WorkspaceId::from_bytes([11; 16]),
        actor: WorkspaceId::from_bytes([12; 16]),
        expected_generation: None,
        writes,
        prepared_operations: Vec::new(),
        revision_metadata: Vec::new(),
        delivery_intents: Vec::new(),
        durability: OverlayDurabilityPolicy::Normal,
        boundary: AtomicityBoundary::Single,
        idempotency: idempotency.map(loom_core::IdempotencyKey::opaque),
        owner_state: loom_core::WorkflowOwnerState::default(),
        post_commit_delta: None,
    }
}

fn workflow_put(
    facet: FacetKind,
    key: OverlayKey,
    payload: &[u8],
    expected: Option<loom_core::OverlayOwnerToken>,
) -> FacetWrite {
    FacetWrite {
        facet,
        target: key,
        op: FacetWriteOp::Put {
            payload: payload.to_vec(),
        },
        secondary_indexes: Vec::new(),
        expected: expected.map(CompareToken),
        durability: None,
        audit: None,
        side_effects: FacetSideEffects::default(),
    }
}

fn workflow_put_with_side_effect(key: OverlayKey, payload: &[u8], operation: &str) -> FacetWrite {
    FacetWrite {
        audit: Some(AuditIntent {
            operation: operation.to_string(),
        }),
        side_effects: FacetSideEffects {
            intents: vec![FacetSideEffect::OperationLog {
                operation_id: operation.to_string(),
            }],
        },
        ..workflow_put(FacetKind::Document, key, payload, None)
    }
}

fn workflow_put_with_secondary_index(
    key: OverlayKey,
    payload: &[u8],
    index: OverlayKey,
    index_payload: &[u8],
) -> FacetWrite {
    FacetWrite {
        secondary_indexes: vec![SecondaryIndexWrite {
            index,
            op: SecondaryIndexWriteOp::Put {
                payload: index_payload.to_vec(),
            },
        }],
        ..workflow_put(FacetKind::Document, key, payload, None)
    }
}

#[test]
fn workflow_transaction_commits_all_writes_in_one_boundary_and_replays_idempotency() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let document_key = durability_facet_test_key(b"documents", "workflow-document");
    let search_key = durability_facet_test_key(b"search", "workflow-search");
    let txn = workflow_transaction_test(
        "workflow",
        vec![
            workflow_put(
                FacetKind::Document,
                document_key.clone(),
                b"document-current",
                None,
            ),
            workflow_put(
                FacetKind::Search,
                search_key.clone(),
                b"search-current",
                None,
            ),
        ],
        Some(b"workflow-retry"),
    );

    let receipt = store.commit_workflow_transaction(txn.clone()).unwrap();
    let replay = store.commit_workflow_transaction(txn).unwrap();

    assert!(!receipt.replayed);
    assert!(replay.replayed);
    assert_eq!(receipt.writes.len(), 2);
    assert_eq!(replay.writes[0].owner_token, receipt.writes[0].owner_token);
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let snapshot = reopened.mutable_overlay_snapshot().unwrap();
    assert_eq!(
        snapshot
            .read_composite(&document_key, |_| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"document-current"[..])
    );
    assert_eq!(
        snapshot
            .read_composite(&search_key, |_| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"search-current"[..])
    );
}

#[test]
fn mu17j_l_b_real_multi_object_publication_failure_preserves_live_and_reopened_authority() {
    let path = TempPath::new("mu17j-l-b-publication-rollback");
    let store = FileStore::open(path.path()).unwrap();
    let document_key = durability_facet_test_key(b"documents", "mu17j-l-b-document");
    let search_key = durability_facet_test_key(b"search", "mu17j-l-b-search");
    let baseline_object = blob(b"mu17j-l-b-baseline-object");
    let baseline_digest = Digest::hash(Algo::Blake3, &baseline_object);
    let mut baseline = workflow_transaction_test(
        "mu17j-l-b-baseline",
        vec![
            workflow_put(
                FacetKind::Document,
                document_key.clone(),
                b"document-before",
                None,
            ),
            workflow_put(
                FacetKind::Search,
                search_key.clone(),
                b"search-before",
                None,
            ),
        ],
        Some(b"mu17j-l-b-baseline"),
    );
    baseline.owner_state = loom_core::WorkflowOwnerState {
        objects: vec![(baseline_digest, baseline_object.clone())],
        controls: vec![loom_core::WorkflowControlWrite::Put {
            key: b"mu17j-l-b/control".to_vec(),
            payload: b"control-before".to_vec(),
        }],
        audits: vec![loom_core::WorkflowAuditWrite {
            principal: None,
            action: "mu17j-l-b.baseline".to_string(),
            target: Some("mu17j-l-b/control".to_string()),
        }],
        ..loom_core::WorkflowOwnerState::default()
    };
    let baseline_receipt = store.commit_workflow_transaction(baseline).unwrap();
    assert_eq!(baseline_receipt.writes.len(), 2);

    let before_roots = t188_15_roots(&store);
    let before_free = store.free_runs();
    let before_overlay_generation = store.mutable_overlay_generation().unwrap();
    let before_audit = store.audit_records().unwrap();
    let old_index_root = before_roots.index_root.unwrap();
    assert!(!before_free.iter().any(|run| {
        old_index_root.0 >= run.start && old_index_root.0 < run.start.saturating_add(run.len)
    }));
    let old_index_page = {
        let mut bytes = [0u8; PAGE_SIZE as usize];
        let mut file = store.file.lock().unwrap();
        read_exact_at(&mut **file, old_index_root.offset(DATA_START), &mut bytes).unwrap();
        bytes
    };
    let _ = take_object_index_batch_page_stats();

    let failed_objects = [
        blob(b"mu17j-l-b-failed-object-a"),
        blob(b"mu17j-l-b-failed-object-b"),
        blob(b"mu17j-l-b-failed-object-c"),
    ];
    let failed_digests = failed_objects
        .iter()
        .map(|object| Digest::hash(Algo::Blake3, object))
        .collect::<Vec<_>>();
    let mut failed = workflow_transaction_test(
        "mu17j-l-b-failed",
        vec![
            workflow_put(
                FacetKind::Document,
                document_key.clone(),
                b"document-after",
                Some(baseline_receipt.writes[0].owner_token.clone()),
            ),
            workflow_put(
                FacetKind::Search,
                search_key.clone(),
                b"search-after",
                Some(baseline_receipt.writes[1].owner_token.clone()),
            ),
        ],
        Some(b"mu17j-l-b-failed"),
    );
    failed.owner_state = loom_core::WorkflowOwnerState {
        objects: failed_digests
            .iter()
            .copied()
            .zip(failed_objects.iter().cloned())
            .collect(),
        controls: vec![loom_core::WorkflowControlWrite::Put {
            key: b"mu17j-l-b/control".to_vec(),
            payload: b"control-after".to_vec(),
        }],
        audits: vec![loom_core::WorkflowAuditWrite {
            principal: None,
            action: "mu17j-l-b.failed".to_string(),
            target: Some("mu17j-l-b/control".to_string()),
        }],
        ..loom_core::WorkflowOwnerState::default()
    };

    let injector_hits = Arc::new(AtomicU64::new(0));
    let hits = Arc::clone(&injector_hits);
    let guard = install_store_publication_failure_test_injector(
        path.path().to_path_buf(),
        Arc::new(move |boundary| {
            assert_eq!(
                boundary,
                StorePublicationFailureTestBoundary::WorkflowOwnerStateCommit
            );
            hits.fetch_add(1, Ordering::SeqCst);
            Err(LoomError::new(
                Code::Io,
                "injected multi-object publication failure",
            ))
        }),
    );
    let error = store.commit_workflow_transaction(failed).unwrap_err();
    assert_eq!(error.code, Code::Io);
    assert_eq!(injector_hits.load(Ordering::SeqCst), 1);
    let batch_stats = take_object_index_batch_page_stats();
    assert_eq!(batch_stats.len(), 1);
    assert!(batch_stats[0].existing_pages_replaced > 0);

    assert_eq!(t188_15_roots(&store), before_roots);
    assert_eq!(store.free_runs(), before_free);
    assert_eq!(
        store.mutable_overlay_generation().unwrap(),
        before_overlay_generation
    );
    assert_eq!(store.audit_records().unwrap(), before_audit);
    assert_eq!(
        store.control_get(b"mu17j-l-b/control").unwrap().as_deref(),
        Some(&b"control-before"[..])
    );
    assert_eq!(
        store.get(&baseline_digest).unwrap(),
        Some(baseline_object.clone())
    );
    for digest in &failed_digests {
        assert_eq!(store.get(digest).unwrap(), None);
    }
    assert_eq!(
        store
            .mutable_overlay_current_entry(&document_key)
            .unwrap()
            .unwrap()
            .payload,
        b"document-before"
    );
    assert_eq!(
        store
            .mutable_overlay_current_entry(&search_key)
            .unwrap()
            .unwrap()
            .payload,
        b"search-before"
    );
    let live_index_page = {
        let mut bytes = [0u8; PAGE_SIZE as usize];
        let mut file = store.file.lock().unwrap();
        read_exact_at(&mut **file, old_index_root.offset(DATA_START), &mut bytes).unwrap();
        bytes
    };
    assert_eq!(live_index_page, old_index_page);

    drop(guard);
    drop(store);
    let reopened = FileStore::open(path.path()).unwrap();
    assert_eq!(t188_15_roots(&reopened), before_roots);
    assert_eq!(reopened.free_runs(), before_free);
    assert_eq!(
        reopened.mutable_overlay_generation().unwrap(),
        before_overlay_generation
    );
    assert_eq!(reopened.audit_records().unwrap(), before_audit);
    assert_eq!(
        reopened
            .control_get(b"mu17j-l-b/control")
            .unwrap()
            .as_deref(),
        Some(&b"control-before"[..])
    );
    assert_eq!(
        reopened.get(&baseline_digest).unwrap(),
        Some(baseline_object)
    );
    for digest in &failed_digests {
        assert_eq!(reopened.get(digest).unwrap(), None);
    }
    assert_eq!(
        reopened
            .mutable_overlay_current_entry(&document_key)
            .unwrap()
            .unwrap()
            .payload,
        b"document-before"
    );
    assert_eq!(
        reopened
            .mutable_overlay_current_entry(&search_key)
            .unwrap()
            .unwrap()
            .payload,
        b"search-before"
    );
    let reopened_index_page = {
        let mut bytes = [0u8; PAGE_SIZE as usize];
        let mut file = reopened.file.lock().unwrap();
        read_exact_at(&mut **file, old_index_root.offset(DATA_START), &mut bytes).unwrap();
        bytes
    };
    assert_eq!(reopened_index_page, old_index_page);
    assert!(!reopened.free_runs().iter().any(|run| {
        old_index_root.0 >= run.start && old_index_root.0 < run.start.saturating_add(run.len)
    }));
}

#[test]
fn workflow_transaction_receipt_reports_prepared_and_owner_state_outputs() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let key = durability_facet_test_key(b"documents", "workflow-extended-receipt-output");
    let mut txn = workflow_transaction_test(
        "workflow-extended-receipt-output",
        vec![workflow_put(FacetKind::Document, key, b"current", None)],
        Some(b"workflow-extended-receipt-output"),
    );
    txn.prepared_operations.push(loom_core::PreparedOperation {
        operation_id: "operation-output".to_string(),
        payload: b"operation".to_vec(),
    });
    txn.revision_metadata
        .push(loom_core::PreparedRevisionMetadata {
            entity_id: "entity-output".to_string(),
            revision_id: "revision-output".to_string(),
            payload: b"revision".to_vec(),
        });
    txn.delivery_intents
        .push(loom_core::PreparedDeliveryIntent {
            stream_id: "stream-output".to_string(),
            sequence: 17,
            envelope_id: "envelope-output".to_string(),
            payload_digest: Digest::blake3(b"delivery-output"),
        });
    txn.owner_state
        .controls
        .push(loom_core::WorkflowControlWrite::AppendRetained {
            key: b"history-output".to_vec(),
            expected_next_sequence: 1,
            records: vec![b"retained-a".to_vec(), b"retained-b".to_vec()],
        });
    txn.owner_state.audits.push(loom_core::WorkflowAuditWrite {
        principal: Some(txn.actor),
        action: "workflow.extended".to_string(),
        target: Some("workflow-extended-receipt-output".to_string()),
    });

    let receipt = store.commit_workflow_transaction(txn).unwrap();

    assert_eq!(receipt.operation_identities, ["operation-output"]);
    assert_eq!(
        receipt.revision_identities[0].revision_id,
        "revision-output"
    );
    assert_eq!(receipt.delivery_receipts[0].sequence, 17);
    assert_eq!(receipt.audit_sequences.len(), 1);
    assert_eq!(
        receipt.retained_sequences,
        [loom_core::RetainedSequenceReceipt {
            key: b"history-output".to_vec(),
            first_sequence: 1,
            last_sequence: 2,
        }]
    );
    assert!(!receipt.replayed);
}

#[test]
fn workflow_transaction_commits_overlay_control_objects_and_reference_together() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let overlay_key = durability_facet_test_key(b"documents", "cross-storage");
    let canonical = loom_core::Object::Blob(b"owner-state".to_vec()).canonical();
    let object = Digest::hash(Algo::Blake3, &canonical);
    let mut txn = workflow_transaction_test(
        "cross-storage",
        vec![workflow_put(
            FacetKind::Document,
            overlay_key.clone(),
            b"overlay-state",
            None,
        )],
        Some(b"cross-storage"),
    );
    txn.owner_state = loom_core::WorkflowOwnerState {
        objects: vec![(object, canonical.clone())],
        reference: loom_core::WorkflowReferenceUpdate::Set(Some(object)),
        controls: vec![loom_core::WorkflowControlWrite::Put {
            key: b"owner/current".to_vec(),
            payload: b"control-state".to_vec(),
        }],
        audits: vec![loom_core::WorkflowAuditWrite {
            principal: None,
            action: "owner.commit".to_string(),
            target: Some("owner/current".to_string()),
        }],
    };

    store.commit_workflow_transaction(txn).unwrap();
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    assert_eq!(reopened.reference_root(), Some(object));
    assert_eq!(
        reopened.control_get(b"owner/current").unwrap().as_deref(),
        Some(&b"control-state"[..])
    );
    assert_eq!(
        reopened.get(&object).unwrap().as_deref(),
        Some(canonical.as_slice())
    );
    let audit = reopened.audit_records().unwrap();
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].action, "owner.commit");
    assert_eq!(
        reopened
            .mutable_overlay_snapshot()
            .unwrap()
            .read_composite(&overlay_key, |_| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"overlay-state"[..])
    );
}

#[test]
fn saved_state_and_audit_commit_reference_and_audit_atomically() {
    let shared = SharedMem::default();
    let publisher = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let mut loom =
        Loom::new(FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap());
    let ns = loom
        .registry_mut()
        .create(
            FacetKind::Files,
            Some("repo"),
            WorkspaceId::from_bytes([53; 16]),
        )
        .unwrap();
    loom.write_file(ns, "README.md", b"current", 0o100644)
        .unwrap();
    let saved = loom.save_state_objects().unwrap();
    let root = saved.0;

    put_saved_state_and_audit(
        &publisher,
        saved,
        vec![loom_core::WorkflowAuditWrite {
            principal: Some(ns),
            action: "refs.reconcile".to_string(),
            target: Some("workspace=repo;processed=1;resolved=1;failed=0;pending=0".to_string()),
        }],
    )
    .unwrap();
    drop(publisher);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    assert_eq!(reopened.reference_root(), Some(root));
    let audit = reopened.audit_records().unwrap();
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].principal, Some(ns));
    assert_eq!(audit[0].action, "refs.reconcile");
    assert_eq!(
        audit[0].target.as_deref(),
        Some("workspace=repo;processed=1;resolved=1;failed=0;pending=0")
    );
}

#[test]
fn interrupted_saved_state_and_audit_publication_exposes_neither_state_nor_audit() {
    let shared = SharedMem::default();
    let baseline = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let baseline_digest = baseline.put(&blob(b"baseline")).unwrap();
    baseline.set_reference_root(Some(baseline_digest)).unwrap();
    drop(baseline);

    let failing =
        FileStore::with_backing(Box::new(FailNthFsyncMem::new(shared.clone(), 2)), true).unwrap();
    let mut loom =
        Loom::new(FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap());
    let ns = loom
        .registry_mut()
        .create(
            FacetKind::Files,
            Some("repo"),
            WorkspaceId::from_bytes([54; 16]),
        )
        .unwrap();
    loom.write_file(ns, "README.md", b"after", 0o100644)
        .unwrap();
    let failed = put_saved_state_and_audit(
        &failing,
        loom.save_state_objects().unwrap(),
        vec![loom_core::WorkflowAuditWrite {
            principal: Some(ns),
            action: "refs.reconcile".to_string(),
            target: Some("workspace=repo;processed=1;resolved=1;failed=0;pending=0".to_string()),
        }],
    );
    assert!(failed.is_err());
    drop(failing);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    assert_eq!(reopened.reference_root(), Some(baseline_digest));
    assert!(reopened.audit_records().unwrap().is_empty());
}

#[test]
fn workflow_transaction_appends_retained_history_atomically_and_survives_reopen() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let overlay_key = durability_facet_test_key(b"documents", "retained-history-owner");
    let history_key = b"pages/workspace/operation-log".to_vec();
    let mut first = workflow_transaction_test(
        "retained-history-first",
        vec![workflow_put(
            FacetKind::Document,
            overlay_key.clone(),
            b"current-1",
            None,
        )],
        None,
    );
    first.owner_state.controls = vec![loom_core::WorkflowControlWrite::AppendRetained {
        key: history_key.clone(),
        expected_next_sequence: 1,
        records: vec![b"operation-1".to_vec(), b"operation-2".to_vec()],
    }];
    store.commit_workflow_transaction(first).unwrap();

    let token = store.mutable_overlay_owner_token(&overlay_key).unwrap();
    let mut second = workflow_transaction_test(
        "retained-history-second",
        vec![workflow_put(
            FacetKind::Document,
            overlay_key.clone(),
            b"current-2",
            token,
        )],
        None,
    );
    second.owner_state.controls = vec![loom_core::WorkflowControlWrite::AppendRetained {
        key: history_key.clone(),
        expected_next_sequence: 3,
        records: vec![b"operation-3".to_vec()],
    }];
    store.commit_workflow_transaction(second).unwrap();

    assert_eq!(store.retained_history_head(&history_key).unwrap(), 3);
    assert_eq!(
        store
            .retained_history_records(&history_key, 2, usize::MAX)
            .unwrap(),
        vec![b"operation-2".to_vec(), b"operation-3".to_vec()]
    );
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    assert_eq!(reopened.retained_history_head(&history_key).unwrap(), 3);
    assert_eq!(
        reopened
            .retained_history_records(&history_key, 1, usize::MAX)
            .unwrap(),
        vec![
            b"operation-1".to_vec(),
            b"operation-2".to_vec(),
            b"operation-3".to_vec()
        ]
    );
    assert_eq!(
        reopened
            .mutable_overlay_current_entry(&overlay_key)
            .unwrap()
            .unwrap()
            .payload,
        b"current-2"
    );
}

#[test]
fn retained_history_routes_through_catalog_family_root() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let history_key = b"pages/workspace/catalog-retained-history".to_vec();

    store
        .commit_family_root_records_for_test(
            RETAINED_HISTORY_FAMILY_ID,
            &[
                (
                    retained_history_head_address(&history_key),
                    encode_retained_history_head(&history_key, 2),
                ),
                (
                    retained_history_record_address(&history_key, 1),
                    encode_retained_history_entry(&history_key, 1, b"operation-1"),
                ),
                (
                    retained_history_record_address(&history_key, 2),
                    encode_retained_history_entry(&history_key, 2, b"operation-2"),
                ),
            ],
        )
        .unwrap();
    let (
        region_table_root,
        page_count,
        root_catalog_root,
        retained_history_root,
        overlay_root,
        current_record_root,
    ) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.region_table_root.unwrap(),
            inner.page_count,
            inner.root_catalog_root,
            inner.retained_history_root,
            inner.overlay_root,
            inner.current_record_root,
        )
    };
    let mut backing = shared.clone();
    let region = read_region_table(&mut backing, region_table_root, page_count).unwrap();
    let catalog = read_root_catalog(&mut backing, root_catalog_root.unwrap(), page_count).unwrap();

    assert_eq!(region.root_catalog_root, root_catalog_root);
    assert_eq!(region.overlay_root, None);
    assert_eq!(overlay_root, None);
    assert_eq!(current_record_root, None);
    assert_eq!(
        catalog
            .entries
            .iter()
            .find(|entry| entry.family_id == RETAINED_HISTORY_FAMILY_ID)
            .map(|entry| entry.root),
        retained_history_root
    );
    assert!(retained_history_root.is_some());
    assert_eq!(
        store
            .mutable_overlay_record_payload(&retained_history_head_address(&history_key))
            .unwrap(),
        None
    );
    assert_eq!(store.retained_history_head(&history_key).unwrap(), 2);
    assert_eq!(
        store
            .retained_history_records(&history_key, 1, usize::MAX)
            .unwrap(),
        vec![b"operation-1".to_vec(), b"operation-2".to_vec()]
    );
}

#[test]
fn retained_history_catalog_family_survives_reopen_without_current_hydration() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let history_key = b"pages/workspace/catalog-retained-reopen".to_vec();
    store
        .commit_family_root_records_for_test(
            RETAINED_HISTORY_FAMILY_ID,
            &[
                (
                    retained_history_head_address(&history_key),
                    encode_retained_history_head(&history_key, 1),
                ),
                (
                    retained_history_record_address(&history_key, 1),
                    encode_retained_history_entry(&history_key, 1, b"operation-1"),
                ),
            ],
        )
        .unwrap();
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let stats = reopened.io_stats().unwrap();
    let (retained_history_root, overlay_root, current_record_root, root_catalog_root) = {
        let inner = reopened.inner.lock().unwrap();
        (
            inner.retained_history_root,
            inner.overlay_root,
            inner.current_record_root,
            inner.root_catalog_root,
        )
    };

    assert!(retained_history_root.is_some());
    assert!(root_catalog_root.is_some());
    assert_eq!(overlay_root, None);
    assert_eq!(current_record_root, None);
    assert_eq!(stats.open_mutable_current_records_loaded, 0);
    assert_eq!(stats.open_mutable_control_records_skipped, 0);
    assert_eq!(reopened.retained_history_head(&history_key).unwrap(), 1);
    assert_eq!(
        reopened
            .retained_history_records(&history_key, 1, usize::MAX)
            .unwrap(),
        vec![b"operation-1".to_vec()]
    );
}

#[test]
fn retained_history_family_root_does_not_fall_back_to_stale_legacy_overlay() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let canonical_key = b"pages/workspace/catalog-retained-authoritative".to_vec();
    let stale_legacy_key = b"pages/workspace/stale-legacy-retained".to_vec();
    store
        .commit_raw_overlay_records_for_test(&[(
            retained_history_head_address(&stale_legacy_key),
            encode_retained_history_head(&stale_legacy_key, 9),
        )])
        .unwrap();
    let stale_overlay_root = store.inner.lock().unwrap().overlay_root;
    store.inner.lock().unwrap().overlay_root = None;
    store
        .commit_family_root_records_for_test(
            RETAINED_HISTORY_FAMILY_ID,
            &[
                (
                    retained_history_head_address(&canonical_key),
                    encode_retained_history_head(&canonical_key, 1),
                ),
                (
                    retained_history_record_address(&canonical_key, 1),
                    encode_retained_history_entry(&canonical_key, 1, b"canonical-operation"),
                ),
            ],
        )
        .unwrap();
    store.inner.lock().unwrap().overlay_root = stale_overlay_root;

    let (retained_history_root, overlay_root) = {
        let inner = store.inner.lock().unwrap();
        (inner.retained_history_root, inner.overlay_root)
    };

    assert!(retained_history_root.is_some());
    assert!(overlay_root.is_some());
    assert_eq!(
        store
            .mutable_overlay_record_payload(&retained_history_head_address(&stale_legacy_key))
            .unwrap()
            .map(|bytes| decode_retained_history_head(&bytes).unwrap().1),
        Some(9)
    );
    assert_eq!(store.retained_history_head(&stale_legacy_key).unwrap(), 0);
    assert_eq!(
        store
            .retained_history_records(&stale_legacy_key, 1, usize::MAX)
            .unwrap(),
        Vec::<Vec<u8>>::new()
    );
    assert_eq!(store.retained_history_head(&canonical_key).unwrap(), 1);
}

#[test]
fn retained_history_mixed_root_set_publication_fails_closed() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let canonical_key = b"pages/workspace/catalog-retained-mixed-negative".to_vec();
    let legacy_key = b"pages/workspace/legacy-retained-mixed-negative".to_vec();
    store
        .commit_family_root_records_for_test(
            RETAINED_HISTORY_FAMILY_ID,
            &[
                (
                    retained_history_head_address(&canonical_key),
                    encode_retained_history_head(&canonical_key, 1),
                ),
                (
                    retained_history_record_address(&canonical_key, 1),
                    encode_retained_history_entry(&canonical_key, 1, b"canonical-operation"),
                ),
            ],
        )
        .unwrap();

    let error = store
        .commit_raw_overlay_records_for_test(&[(
            retained_history_head_address(&legacy_key),
            encode_retained_history_head(&legacy_key, 7),
        )])
        .unwrap_err();

    assert_eq!(error.code, Code::CorruptObject);
    assert_eq!(store.retained_history_head(&canonical_key).unwrap(), 1);
    assert_eq!(store.retained_history_head(&legacy_key).unwrap(), 0);
}

#[test]
fn retained_history_production_write_reopens_as_canonical_family_after_t188_14() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let history_key = b"pages/workspace/source-layout-retained".to_vec();
    let mut transaction = workflow_transaction_test("source-layout-retained", Vec::new(), None);
    transaction.owner_state.controls = vec![loom_core::WorkflowControlWrite::AppendRetained {
        key: history_key.clone(),
        expected_next_sequence: 1,
        records: vec![b"operation-1".to_vec()],
    }];

    store.commit_workflow_transaction(transaction).unwrap();

    let (root_catalog_root, retained_history_root, current_record_root, overlay_root) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.root_catalog_root,
            inner.retained_history_root,
            inner.current_record_root,
            inner.overlay_root,
        )
    };
    assert!(root_catalog_root.is_some());
    assert!(retained_history_root.is_some());
    assert_eq!(current_record_root, None);
    assert_eq!(overlay_root, None);
    assert!(
        store
            .mutable_overlay_record_payload(&retained_history_head_address(&history_key))
            .unwrap()
            .is_none()
    );
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let (root_catalog_root, retained_history_root, current_record_root, overlay_root) = {
        let inner = reopened.inner.lock().unwrap();
        (
            inner.root_catalog_root,
            inner.retained_history_root,
            inner.current_record_root,
            inner.overlay_root,
        )
    };
    assert!(root_catalog_root.is_some());
    assert!(retained_history_root.is_some());
    assert_eq!(current_record_root, None);
    assert_eq!(overlay_root, None);
    assert_eq!(reopened.retained_history_head(&history_key).unwrap(), 1);
    assert_eq!(
        reopened
            .retained_history_records(&history_key, 1, usize::MAX)
            .unwrap(),
        vec![b"operation-1".to_vec()]
    );
}

#[test]
fn absent_retained_history_catalog_family_reads_empty() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let history_key = b"pages/workspace/no-retained-family".to_vec();

    assert_eq!(store.retained_history_head(&history_key).unwrap(), 0);
    assert_eq!(
        store
            .retained_history_records(&history_key, 1, usize::MAX)
            .unwrap(),
        Vec::<Vec<u8>>::new()
    );
}

#[test]
fn retained_history_owner_appends_point_update_the_overlay_index() {
    let tp = TempPath::new("retained-history-point-update");
    let store = FileStore::open(tp.path()).unwrap();
    let overlay_key = durability_facet_test_key(b"documents", "retained-history-growth");
    let history_key = b"pages/workspace/bounded-operation-log".to_vec();
    let mut warm = 0;

    for sequence in 1..=48u64 {
        let token = store.mutable_overlay_owner_token(&overlay_key).unwrap();
        let mut transaction = workflow_transaction_test(
            &format!("retained-history-growth-{sequence}"),
            vec![workflow_put(
                FacetKind::Document,
                overlay_key.clone(),
                format!("current-{sequence}").as_bytes(),
                token,
            )],
            None,
        );
        transaction.owner_state.controls = vec![loom_core::WorkflowControlWrite::AppendRetained {
            key: history_key.clone(),
            expected_next_sequence: sequence,
            records: vec![format!("operation-{sequence}").into_bytes()],
        }];
        store.commit_workflow_transaction(transaction).unwrap();
        if sequence == 24 {
            warm = store.maintenance_status().unwrap().physical_bytes;
        }
    }

    let measured = store.maintenance_status().unwrap().physical_bytes;
    assert_eq!(store.retained_history_head(&history_key).unwrap(), 48);
    assert!(
        measured.saturating_sub(warm) <= 256 * 1024,
        "24 retained owner appends grew {} bytes after warmup",
        measured.saturating_sub(warm)
    );
}

#[test]
fn mutable_overlay_current_root_survives_canonical_round_trip() {
    let encoded = encode_mutable_overlay_current_root_record(Some(PageId(42)));
    assert_eq!(
        decode_mutable_overlay_current_root_record(&encoded).unwrap(),
        Some(PageId(42))
    );
    assert_eq!(
        decode_mutable_overlay_current_root_record(&encode_mutable_overlay_current_root_record(
            None
        ))
        .unwrap(),
        None
    );

    let mut trailing = encoded.clone();
    trailing.push(0);
    assert!(decode_mutable_overlay_current_root_record(&trailing).is_err());
}

#[test]
fn cold_open_uses_current_root_instead_of_retained_history_scan() {
    let tp = TempPath::new("current-root-open");
    let history_key = b"pages/workspace/current-root-history".to_vec();
    let current_key = durability_facet_test_key(b"documents", "current-root-live");
    {
        let store = FileStore::open(tp.path()).unwrap();
        for sequence in 1..=40u64 {
            let token = store.mutable_overlay_owner_token(&current_key).unwrap();
            let mut transaction = workflow_transaction_test(
                &format!("current-root-open-{sequence}"),
                vec![workflow_put(
                    FacetKind::Document,
                    current_key.clone(),
                    format!("current-{sequence}").as_bytes(),
                    token,
                )],
                None,
            );
            transaction.owner_state.controls =
                vec![loom_core::WorkflowControlWrite::AppendRetained {
                    key: history_key.clone(),
                    expected_next_sequence: sequence,
                    records: vec![format!("operation-{sequence}").into_bytes()],
                }];
            store.commit_workflow_transaction(transaction).unwrap();
        }
    }

    let reopened = FileStore::open(tp.path()).unwrap();
    let stats = reopened.io_stats().unwrap();
    assert!(stats.open_mutable_used_current_root);
    assert_eq!(stats.open_mutable_current_records_loaded, 1);
    assert_eq!(stats.open_mutable_control_records_skipped, 0);
    assert_eq!(reopened.retained_history_head(&history_key).unwrap(), 40);
    assert_eq!(
        reopened
            .retained_history_records(&history_key, 40, 1)
            .unwrap(),
        vec![b"operation-40".to_vec()]
    );
    assert_eq!(
        reopened
            .mutable_overlay_current_entry(&current_key)
            .unwrap()
            .unwrap()
            .payload,
        b"current-40"
    );
}

#[test]
fn point_update_after_many_current_keys_updates_current_root_locally() {
    let tp = TempPath::new("current-root-point-update");
    let store = FileStore::open(tp.path()).unwrap();
    let mut keys = Vec::new();
    for index in 0..64 {
        let key = durability_facet_test_key(b"documents", &format!("point-update-{index}"));
        store
            .put_mutable_overlay_value(key.clone(), format!("value-{index}").into_bytes())
            .unwrap();
        keys.push(key);
    }
    let warm = store.maintenance_status().unwrap().physical_bytes;
    store
        .put_mutable_overlay_value(keys[0].clone(), b"value-updated".to_vec())
        .unwrap();
    let measured = store.maintenance_status().unwrap().physical_bytes;

    assert!(
        measured.saturating_sub(warm) <= 32 * 1024,
        "one point update after 64 current keys grew {} bytes",
        measured.saturating_sub(warm)
    );
}

fn mu14d_c6_floor(store: &FileStore) -> u64 {
    store.inner.lock().unwrap().mutable_overlay_generation_floor
}

#[test]
fn mu14d_c6_successful_mutable_publish_advances_and_reopens_floor() {
    let tp = TempPath::new("mu14d-c6-successful-floor");
    {
        let store = FileStore::open(tp.path()).unwrap();
        let key = durability_facet_test_key(b"documents", "floor-success");
        store
            .put_mutable_overlay_value(key.clone(), b"one".to_vec())
            .unwrap();
        assert_eq!(mu14d_c6_floor(&store), 1);
        store
            .put_mutable_overlay_value(key, b"two".to_vec())
            .unwrap();
        assert_eq!(mu14d_c6_floor(&store), 2);
    }
    let reopened = FileStore::open(tp.path()).unwrap();
    assert_eq!(mu14d_c6_floor(&reopened), 2);
}

#[test]
fn mu14d_c6_noop_and_rejected_work_preserve_floor() {
    let tp = TempPath::new("mu14d-c6-preserve-floor");
    let store = FileStore::open(tp.path()).unwrap();
    let key = durability_facet_test_key(b"documents", "floor-preserve");
    store
        .put_mutable_overlay_value(key.clone(), b"one".to_vec())
        .unwrap();
    assert_eq!(mu14d_c6_floor(&store), 1);

    let object = store.put(&blob(b"mu14d-c6-object-only")).unwrap();
    store.set_reference_root(Some(object)).unwrap();
    assert_eq!(mu14d_c6_floor(&store), 1);

    let rejected = store.commit_workflow_transaction(workflow_transaction_test(
        "mu14d-c6-rejected",
        vec![workflow_put(
            FacetKind::Document,
            key,
            b"rejected",
            Some(loom_core::OverlayOwnerToken::from_bytes([0x6c; 32])),
        )],
        None,
    ));
    assert!(rejected.is_err());
    assert_eq!(mu14d_c6_floor(&store), 1);
}

#[test]
fn mu14d_c6_failed_publication_preserves_live_and_reopened_floor() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let key = durability_facet_test_key(b"documents", "floor-failure");
    store
        .put_mutable_overlay_value(key.clone(), b"one".to_vec())
        .unwrap();
    assert_eq!(mu14d_c6_floor(&store), 1);
    drop(store);

    let failing = FailNthFsyncMem::new(shared.clone(), 2);
    let store = FileStore::with_backing(Box::new(failing), true).unwrap();
    let failed = store.put_mutable_overlay_value(key, b"two".to_vec());
    assert!(failed.is_err());
    assert_eq!(mu14d_c6_floor(&store), 1);
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    assert_eq!(mu14d_c6_floor(&reopened), 1);
}

#[test]
fn mu14d_c6_compaction_preserves_floor_and_reopens() {
    let tp = TempPath::new("mu14d-c6-compact-floor");
    {
        let mut store = FileStore::open(tp.path()).unwrap();
        let key = durability_facet_test_key(b"documents", "floor-compact");
        for update in 0..4u64 {
            store
                .put_mutable_overlay_value(key.clone(), format!("value-{update}").into_bytes())
                .unwrap();
        }
        assert_eq!(mu14d_c6_floor(&store), 4);
        store.compact().unwrap();
        assert_eq!(mu14d_c6_floor(&store), 4);
    }

    let reopened = FileStore::open(tp.path()).unwrap();
    assert_eq!(mu14d_c6_floor(&reopened), 4);
}

#[test]
fn mu14d_c6_root_input_rejects_generation_floor_regression() {
    assert!(
        t188_18b_finish_with_root_inputs(TxnRootInputs {
            object_index: None,
            legacy_overlay: None,
            current_records: None,
            root_catalog: TxnRootCatalog {
                root: None,
                entries: Vec::new(),
            },
            reference: None,
            control: None,
            previous_mutable_overlay_generation_floor: 8,
            mutable_overlay_generation_floor: 7,
        })
        .unwrap_err()
        .message
        .contains("mutable overlay generation floor cannot decrease")
    );
}

#[test]
fn cold_open_rejects_corrupt_current_root_pointer() {
    let tp = TempPath::new("current-root-corrupt");
    {
        let store = FileStore::open(tp.path()).unwrap();
        let key = durability_facet_test_key(b"documents", "corrupt-current-root");
        store
            .put_mutable_overlay_value(key, b"current".to_vec())
            .unwrap();
        let inner = store.inner.lock().unwrap();
        let region_table_root = inner.region_table_root.unwrap();
        let encoded = page::CanonicalRegionTable {
            index_root: inner.index_root,
            freemap_root: inner.freemap.map(|(root, _)| root),
            maintenance_root: inner.maintenance_root,
            current_record_root: Some(PageId(u64::MAX)),
            root_catalog_root: inner.root_catalog_root,
            open_segment: inner.open_segment,
            mutable_overlay_generation_floor: inner.mutable_overlay_generation_floor,
            minimum_recoverable_generation: inner.minimum_recoverable_generation,
            metadata_bootstrap_reserve: inner.metadata_bootstrap_reserve.clone(),
        }
        .encode(inner.page_count)
        .unwrap();
        drop(inner);
        let mut file = store.file.lock().unwrap();
        write_at(&mut **file, region_table_root.offset(DATA_START), &encoded).unwrap();
    }

    let err = FileStore::open(tp.path()).unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains("btree node page out of range"),
        "{message}"
    );
}

#[test]
fn compact_preserves_nested_current_root_and_reopens_bounded() {
    let tp = TempPath::new("current-root-compact");
    let key = durability_facet_test_key(b"documents", "compact-current-root");
    {
        let mut store = FileStore::open(tp.path()).unwrap();
        for update in 0..8u64 {
            store
                .put_mutable_overlay_value(key.clone(), format!("compact-{update}").into_bytes())
                .unwrap();
        }

        store.compact().unwrap();
    }

    let reopened = FileStore::open(tp.path()).unwrap();
    let stats = reopened.io_stats().unwrap();
    assert!(stats.open_mutable_used_current_root);
    assert_eq!(stats.open_mutable_current_records_loaded, 1);
    assert_eq!(
        reopened
            .mutable_overlay_snapshot()
            .unwrap()
            .read_composite(&key, |_| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"compact-7"[..])
    );
}

#[test]
fn compact_preserves_durable_overlay_control_records() {
    let tp = TempPath::new("current-root-compact-controls");
    let ordinary_key = durability_facet_test_key(b"documents", "compact-idempotency");
    let tombstone_key = durability_facet_test_key(b"documents", "compact-tombstone");
    let workflow_key = durability_facet_test_key(b"documents", "compact-workflow");
    let index_key = durability_facet_test_key(b"tickets", "compact-workflow-index");
    let history_key = b"pages/workspace/compact-history".to_vec();
    let mut workflow = workflow_transaction_test(
        "compact-workflow-idempotency",
        vec![workflow_put_with_secondary_index(
            workflow_key.clone(),
            b"workflow-current",
            index_key.clone(),
            workflow_key.as_bytes(),
        )],
        Some(b"compact-workflow-idempotency"),
    );
    workflow.owner_state.controls = vec![loom_core::WorkflowControlWrite::AppendRetained {
        key: history_key.clone(),
        expected_next_sequence: 1,
        records: vec![b"operation-1".to_vec(), b"operation-2".to_vec()],
    }];
    let workflow_replay = workflow.clone();

    let ordinary_token;
    {
        let mut store = FileStore::open(tp.path()).unwrap();
        ordinary_token = store
            .put_mutable_overlay_value_idempotent(
                ordinary_key.clone(),
                b"ordinary-current".to_vec(),
                "compact-idempotency",
            )
            .unwrap();
        store
            .put_mutable_overlay_value(tombstone_key.clone(), b"deleted".to_vec())
            .unwrap();
        store
            .put_mutable_overlay_tombstone(tombstone_key.clone())
            .unwrap();
        store.commit_workflow_transaction(workflow).unwrap();

        store.compact().unwrap();
        let diagnostics = store.root_codec_diagnostics().unwrap();
        assert!(
            diagnostics.failures.is_empty(),
            "compaction wrote codec-invalid roots: {:?}",
            diagnostics.failures
        );
        assert!(
            diagnostics
                .details
                .iter()
                .any(|detail| detail.expected_codec == "RecordLocCodec")
        );
        assert!(
            diagnostics
                .details
                .iter()
                .any(|detail| detail.expected_codec == "PackedRecordRefCodec")
        );
    }

    let reopened = FileStore::open(tp.path()).unwrap();
    let diagnostics = reopened.root_codec_diagnostics().unwrap();
    assert!(
        diagnostics.failures.is_empty(),
        "compacted reopen found codec-invalid roots: {:?}",
        diagnostics.failures
    );
    assert_eq!(
        reopened
            .put_mutable_overlay_value_idempotent(
                ordinary_key.clone(),
                b"ordinary-current".to_vec(),
                "compact-idempotency",
            )
            .unwrap()
            .as_bytes(),
        ordinary_token.as_bytes()
    );
    assert_eq!(
        reopened
            .mutable_overlay_durable_owner_token(&ordinary_key)
            .unwrap()
            .as_ref()
            .map(|token| token.as_bytes()),
        Some(ordinary_token.as_bytes())
    );
    assert_eq!(reopened.retained_history_head(&history_key).unwrap(), 2);
    assert_eq!(
        reopened
            .retained_history_records(&history_key, 1, usize::MAX)
            .unwrap(),
        vec![b"operation-1".to_vec(), b"operation-2".to_vec()]
    );
    assert_eq!(
        reopened
            .mutable_overlay_secondary_index_value(&index_key)
            .unwrap()
            .as_deref(),
        Some(workflow_key.as_bytes())
    );
    let replay = reopened
        .commit_workflow_transaction(workflow_replay)
        .unwrap();
    assert_eq!(replay.writes[0].target.as_bytes(), workflow_key.as_bytes());
    let snapshot = reopened.mutable_overlay_snapshot().unwrap();
    assert_eq!(
        snapshot
            .read_composite(&tombstone_key, |_| Ok(Some(b"base".to_vec())))
            .unwrap(),
        None
    );
    assert_eq!(
        snapshot
            .read_composite(&workflow_key, |_| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"workflow-current"[..])
    );
}

#[test]
fn rekey_reseal_preserves_durable_overlay_control_records() {
    let tp = TempPath::new("current-root-reseal-controls");
    let (meta0, session0) = loom_core::keys::EncryptionMeta::create(
        &KeySpec::passphrase("old-pw"),
        loom_core::keys::Suite::XChaCha20Poly1305,
        [2u8; 16].to_vec(),
        [0x11; 32],
        [3u8; 24].to_vec(),
    )
    .unwrap();
    let ordinary_key = durability_facet_test_key(b"documents", "reseal-idempotency");
    let workflow_key = durability_facet_test_key(b"documents", "reseal-workflow");
    let index_key = durability_facet_test_key(b"tickets", "reseal-workflow-index");
    let history_key = b"pages/workspace/reseal-history".to_vec();
    let mut workflow = workflow_transaction_test(
        "reseal-workflow-idempotency",
        vec![workflow_put_with_secondary_index(
            workflow_key.clone(),
            b"reseal-workflow-current",
            index_key.clone(),
            workflow_key.as_bytes(),
        )],
        Some(b"reseal-workflow-idempotency"),
    );
    workflow.owner_state.controls = vec![loom_core::WorkflowControlWrite::AppendRetained {
        key: history_key.clone(),
        expected_next_sequence: 1,
        records: vec![b"reseal-operation".to_vec()],
    }];
    let workflow_replay = workflow.clone();
    let ordinary_token;
    {
        let mut store = FileStore::create_encrypted(tp.path(), meta0.encode(), session0).unwrap();
        ordinary_token = store
            .put_mutable_overlay_value_idempotent(
                ordinary_key.clone(),
                b"reseal-current".to_vec(),
                "reseal-idempotency",
            )
            .unwrap();
        store.commit_workflow_transaction(workflow).unwrap();
        let (meta1, session1) = loom_core::keys::EncryptionMeta::create(
            &KeySpec::passphrase("new-pw"),
            loom_core::keys::Suite::Aes256Gcm,
            [4u8; 16].to_vec(),
            [0x22; 32],
            [5u8; 24].to_vec(),
        )
        .unwrap();
        store.rekey_reseal(meta1.encode(), session1).unwrap();
    }

    let reopened = FileStore::open(tp.path()).unwrap();
    reopened.unlock(&KeySpec::passphrase("new-pw")).unwrap();
    assert_eq!(
        reopened
            .put_mutable_overlay_value_idempotent(
                ordinary_key.clone(),
                b"reseal-current".to_vec(),
                "reseal-idempotency",
            )
            .unwrap()
            .as_bytes(),
        ordinary_token.as_bytes()
    );
    assert_eq!(
        reopened
            .retained_history_records(&history_key, 1, usize::MAX)
            .unwrap(),
        vec![b"reseal-operation".to_vec()]
    );
    assert_eq!(
        reopened
            .mutable_overlay_secondary_index_value(&index_key)
            .unwrap()
            .as_deref(),
        Some(workflow_key.as_bytes())
    );
    assert_eq!(
        reopened
            .commit_workflow_transaction(workflow_replay)
            .unwrap()
            .writes[0]
            .target
            .as_bytes(),
        workflow_key.as_bytes()
    );
}

#[test]
fn replacing_one_packed_overlay_record_preserves_its_slab_neighbors() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let first_key = durability_facet_test_key(b"documents", "packed-first");
    let second_key = durability_facet_test_key(b"documents", "packed-second");

    store
        .put_mutable_overlay_values(vec![
            (first_key.clone(), b"first-v1".to_vec()),
            (second_key.clone(), b"second-v1".to_vec()),
        ])
        .unwrap();
    store
        .put_mutable_overlay_value(first_key.clone(), b"first-v2".to_vec())
        .unwrap();

    assert_eq!(
        store
            .mutable_overlay_current_entry(&first_key)
            .unwrap()
            .unwrap()
            .payload,
        b"first-v2"
    );
    assert_eq!(
        store
            .mutable_overlay_current_entry(&second_key)
            .unwrap()
            .unwrap()
            .payload,
        b"second-v1"
    );
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    assert_eq!(
        reopened
            .mutable_overlay_current_entry(&first_key)
            .unwrap()
            .unwrap()
            .payload,
        b"first-v2"
    );
    assert_eq!(
        reopened
            .mutable_overlay_current_entry(&second_key)
            .unwrap()
            .unwrap()
            .payload,
        b"second-v1"
    );
}

#[test]
fn workflow_transaction_rejects_stale_retained_history_sequence_without_partial_publish() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared), true).unwrap();
    let baseline_key = durability_facet_test_key(b"documents", "retained-history-baseline");
    let history_key = b"pages/workspace/operation-log".to_vec();
    let mut baseline = workflow_transaction_test(
        "retained-history-baseline",
        vec![workflow_put(
            FacetKind::Document,
            baseline_key.clone(),
            b"baseline",
            None,
        )],
        None,
    );
    baseline.owner_state.controls = vec![loom_core::WorkflowControlWrite::AppendRetained {
        key: history_key.clone(),
        expected_next_sequence: 1,
        records: vec![b"operation-1".to_vec()],
    }];
    store.commit_workflow_transaction(baseline).unwrap();

    let rejected_key = durability_facet_test_key(b"documents", "retained-history-rejected");
    let mut rejected = workflow_transaction_test(
        "retained-history-rejected",
        vec![workflow_put(
            FacetKind::Document,
            rejected_key.clone(),
            b"must-not-publish",
            None,
        )],
        None,
    );
    rejected.owner_state.controls = vec![loom_core::WorkflowControlWrite::AppendRetained {
        key: history_key.clone(),
        expected_next_sequence: 1,
        records: vec![b"duplicate".to_vec()],
    }];

    let error = store.commit_workflow_transaction(rejected).unwrap_err();
    assert_eq!(error.code, Code::Conflict);
    assert_eq!(store.retained_history_head(&history_key).unwrap(), 1);
    assert_eq!(
        store
            .retained_history_records(&history_key, 1, usize::MAX)
            .unwrap(),
        vec![b"operation-1".to_vec()]
    );
    assert!(
        store
            .mutable_overlay_current_entry(&rejected_key)
            .unwrap()
            .is_none()
    );
}

#[test]
fn rejected_workflow_owner_state_leaves_prior_state_after_reopen() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let baseline_key = durability_facet_test_key(b"documents", "baseline");
    store
        .commit_workflow_transaction(workflow_transaction_test(
            "baseline",
            vec![workflow_put(
                FacetKind::Document,
                baseline_key.clone(),
                b"baseline",
                None,
            )],
            Some(b"baseline"),
        ))
        .unwrap();
    let baseline_generation = store.generation();
    let rejected_key = durability_facet_test_key(b"documents", "rejected");
    let mut txn = workflow_transaction_test(
        "rejected-owner-state",
        vec![workflow_put(
            FacetKind::Document,
            rejected_key.clone(),
            b"rejected",
            None,
        )],
        Some(b"rejected-owner-state"),
    );
    txn.owner_state = loom_core::WorkflowOwnerState {
        objects: vec![(Digest::blake3(b"wrong"), b"owner-state".to_vec())],
        reference: loom_core::WorkflowReferenceUpdate::Set(Some(Digest::blake3(b"root"))),
        controls: vec![loom_core::WorkflowControlWrite::Put {
            key: b"owner/rejected".to_vec(),
            payload: b"rejected".to_vec(),
        }],
        audits: Vec::new(),
    };

    let error = store.commit_workflow_transaction(txn).unwrap_err();
    assert_eq!(error.code, Code::IntegrityFailure);
    assert_eq!(store.generation(), baseline_generation);
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    assert_eq!(reopened.generation(), baseline_generation);
    assert_eq!(reopened.reference_root(), None);
    assert_eq!(reopened.control_get(b"owner/rejected").unwrap(), None);
    let snapshot = reopened.mutable_overlay_snapshot().unwrap();
    assert_eq!(
        snapshot
            .read_composite(&baseline_key, |_| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"baseline"[..])
    );
    assert_eq!(
        snapshot
            .read_composite(&rejected_key, |_| Ok(None))
            .unwrap(),
        None
    );
}

#[test]
fn normal_workflow_transactions_batch_under_contention_and_replay_after_reopen() {
    let backing = FsyncGateMem::default();
    let reopened_backing = backing.clone();
    let gate = Arc::clone(&backing.gate);
    let store = Arc::new(FileStore::with_backing(Box::new(backing), true).unwrap());
    gate.enable();
    let barrier = Arc::new(Barrier::new(9));
    let mut threads = Vec::new();
    for worker in 0..8u8 {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        threads.push(std::thread::spawn(move || {
            let key =
                durability_facet_test_key(b"documents", &format!("workflow-normal-group-{worker}"));
            let idempotency = format!("workflow-normal-group-{worker}");
            let txn = workflow_transaction_test(
                "workflow-normal-group",
                vec![workflow_put(
                    FacetKind::Document,
                    key.clone(),
                    &[worker],
                    None,
                )],
                Some(idempotency.as_bytes()),
            );
            barrier.wait();
            let receipt = store.commit_workflow_transaction(txn.clone()).unwrap();
            (key, txn, receipt)
        }));
    }
    barrier.wait();
    gate.wait_until_first_blocked();
    std::thread::sleep(std::time::Duration::from_millis(20));
    gate.release();
    let committed = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    let committed_generation = store.generation();
    assert!(committed_generation < 8);
    drop(store);

    let reopened = FileStore::with_backing(Box::new(reopened_backing), true).unwrap();
    assert_eq!(reopened.generation(), committed_generation);
    for (key, txn, receipt) in committed {
        assert_eq!(
            reopened
                .mutable_overlay_snapshot()
                .unwrap()
                .read_composite(&key, |_| Ok(None))
                .unwrap(),
            Some(match &txn.writes[0].op {
                FacetWriteOp::Put { payload } => payload.clone(),
                FacetWriteOp::Delete => Vec::new(),
            })
        );
        let replay = reopened.commit_workflow_transaction(txn).unwrap();
        assert!(replay.replayed);
        assert_eq!(replay.generation, receipt.generation);
    }
}

#[test]
fn workflow_transaction_commits_secondary_index_with_current_record_and_reopens() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let document_key = durability_facet_test_key(b"documents", "workflow-index-document");
    let index_key = durability_facet_test_key(b"tickets", "workflow-index-by-status-open");
    let txn = workflow_transaction_test(
        "workflow-index",
        vec![workflow_put_with_secondary_index(
            document_key.clone(),
            b"document-current",
            index_key.clone(),
            document_key.as_bytes(),
        )],
        Some(b"workflow-index-retry"),
    );

    store.commit_workflow_transaction(txn).unwrap();
    assert_eq!(
        store
            .mutable_overlay_secondary_index_value(&index_key)
            .unwrap()
            .as_deref(),
        Some(document_key.as_bytes())
    );
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let snapshot = reopened.mutable_overlay_snapshot().unwrap();
    assert_eq!(
        snapshot
            .read_composite(&document_key, |_| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"document-current"[..])
    );
    assert_eq!(
        reopened
            .mutable_overlay_secondary_index_value(&index_key)
            .unwrap()
            .as_deref(),
        Some(document_key.as_bytes())
    );
}

#[test]
fn mu17j_h_a_one_key_workflow_updates_are_path_local_as_cardinality_grows() {
    let backing = CountingMem::default();
    let store = FileStore::with_backing(Box::new(backing.clone()), true).unwrap();
    let mut policy = store.store_policy().unwrap();
    policy.default_durability = StoreDurabilityPolicy::Strict;
    store
        .save_store_policy_audited(policy, None, "store.policy.set", None)
        .unwrap();
    let indexed_key = durability_facet_test_key(b"documents", "mu17j-ha-indexed");
    let index_key = durability_facet_test_key(b"tickets", "mu17j-ha-index");
    store
        .commit_workflow_transaction(workflow_transaction_test(
            "mu17j-ha-indexed",
            vec![workflow_put_with_secondary_index(
                indexed_key,
                b"indexed",
                index_key.clone(),
                b"indexed",
            )],
            None,
        ))
        .unwrap();

    let mut measurements = Vec::new();
    let mut seeded = 0usize;
    for cardinality in [1usize, 8, 32, 64] {
        for i in seeded..cardinality {
            let key = durability_facet_test_key(b"documents", &format!("mu17j-ha-seed-{i:03}"));
            store
                .put_mutable_overlay_value(key, format!("seed-{i:03}").into_bytes())
                .unwrap();
        }
        seeded = cardinality;
        let key = durability_facet_test_key(b"documents", "mu17j-ha-target");
        if cardinality == 1 {
            store
                .put_mutable_overlay_value(key.clone(), b"target-seed".to_vec())
                .unwrap();
        }
        let before = t188_15_roots(&store);
        pagebtree::reset_load_all_calls_for_test();
        record_io::reset_record_encode_calls_for_test();
        backing.reset_pages_written();
        store
            .put_mutable_overlay_value(
                key.clone(),
                format!("target-update-{cardinality}").into_bytes(),
            )
            .unwrap();
        let after = t188_15_roots(&store);
        assert_ne!(after.current_record_root, before.current_record_root);
        assert_ne!(after.owner_token_root, before.owner_token_root);
        assert_eq!(after.secondary_index_root, before.secondary_index_root);
        assert_eq!(after.retained_history_root, before.retained_history_root);
        assert_eq!(
            after.mutable_idempotency_root,
            before.mutable_idempotency_root
        );
        assert_eq!(pagebtree::load_all_calls_for_test(), 0);
        assert_eq!(
            store
                .mutable_overlay_secondary_index_value(&index_key)
                .unwrap()
                .as_deref(),
            Some(b"indexed".as_slice())
        );
        let encodes = record_io::record_encode_calls_for_test();
        let pages_written = backing.pages_written();
        eprintln!(
            "mu17j_h_a cardinality={cardinality} record_encodes={encodes} data_pages_written={pages_written}"
        );
        assert!(
            encodes <= 2,
            "one-key update encoded {encodes} records at cardinality {cardinality}"
        );
        assert!(
            pages_written <= 24,
            "one-key update wrote {pages_written} pages at cardinality {cardinality}"
        );
        measurements.push((cardinality, encodes, pages_written));
    }
    let first_encodes = measurements[0].1;
    for (cardinality, encodes, _) in measurements {
        assert_eq!(
            encodes, first_encodes,
            "record encodes grew with cardinality {cardinality}"
        );
    }
    assert_mu17j_h_a_current_only_publication_keeps_unaffected_root_catalog_identity();
}

fn assert_mu17j_h_a_current_only_publication_keeps_unaffected_root_catalog_identity() {
    let backing = CountingMem::default();
    let store = FileStore::with_backing(Box::new(backing.clone()), true).unwrap();
    let indexed_key = durability_facet_test_key(b"documents", "mu17j-ha-current-only-indexed");
    let index_key = durability_facet_test_key(b"tickets", "mu17j-ha-current-only-index");
    store
        .commit_workflow_transaction(workflow_transaction_test(
            "mu17j-ha-current-only-indexed",
            vec![workflow_put_with_secondary_index(
                indexed_key,
                b"indexed",
                index_key,
                b"indexed",
            )],
            None,
        ))
        .unwrap();
    let key = durability_facet_test_key(b"documents", "mu17j-ha-current-only");
    store
        .put_mutable_overlay_value(key.clone(), b"before".to_vec())
        .unwrap();
    let mut entry = store.mutable_overlay_current_entry(&key).unwrap().unwrap();
    entry.payload = b"after".to_vec();
    let records = vec![(
        mutable_overlay_entry_address(&key),
        encode_mutable_overlay_entry(&entry),
    )];
    let before = t188_15_roots(&store);
    pagebtree::reset_load_all_calls_for_test();
    record_io::reset_record_encode_calls_for_test();
    backing.reset_pages_written();
    store
        .publish_mutable_overlay_records_for_test(StoreDurabilityPolicy::Strict, records)
        .unwrap();
    let after = t188_15_roots(&store);
    assert_ne!(after.current_record_root, before.current_record_root);
    assert_eq!(after.root_catalog_root, before.root_catalog_root);
    assert_eq!(after.owner_token_root, before.owner_token_root);
    assert_eq!(after.secondary_index_root, before.secondary_index_root);
    assert_eq!(pagebtree::load_all_calls_for_test(), 0);
    assert_eq!(record_io::record_encode_calls_for_test(), 0);
    eprintln!(
        "mu17j_h_a current_only record_encodes={} data_pages_written={}",
        record_io::record_encode_calls_for_test(),
        backing.pages_written()
    );
    assert!(
        backing.pages_written() <= 18,
        "current-only update wrote {} pages",
        backing.pages_written()
    );
    drop(store);

    let reopened = FileStore::with_backing(Box::new(backing), true).unwrap();
    assert_eq!(
        reopened
            .mutable_overlay_current_entry(&key)
            .unwrap()
            .unwrap()
            .payload,
        b"after"
    );
}

fn mu17j_h_b_install_free_run_cardinality(store: &FileStore, count: usize) {
    let requested = count.saturating_add(8);
    {
        let mut inner = store.inner.lock().unwrap();
        let start = inner.page_count.saturating_add(16);
        let page_count = start.saturating_add((requested as u64).saturating_mul(2));
        inner.free = (0..requested)
            .map(|index| FreePageRun {
                start: start + (index as u64) * 2,
                len: 1,
                freed_gen: 0,
            })
            .collect();
        inner.page_count = page_count;
        inner.maintenance.physical_page_count = page_count;
        let mut file = store.file.lock().unwrap();
        file.grow(DATA_START + page_count * PAGE_SIZE).unwrap();
    }
    let mut control = BTreeMap::new();
    control.insert(
        format!("mu17j-h-b/free-cardinality/{count}").into_bytes(),
        count.to_be_bytes().to_vec(),
    );
    store.commit_raw_control_map_for_test(control).unwrap();
    let free_len = store.inner.lock().unwrap().free.len();
    assert!(
        free_len >= count,
        "expected at least {count} persisted free runs, got {free_len}"
    );
}

fn mu17j_h_b_prepare_measured_store(
    count: usize,
) -> (CountingMem, FileStore, loom_core::OverlayKey) {
    let backing = CountingMem::default();
    let store = FileStore::with_backing(Box::new(backing.clone()), true).unwrap();
    let mut policy = store.store_policy().unwrap();
    policy.default_durability = StoreDurabilityPolicy::Strict;
    store
        .save_store_policy_audited(policy, None, "store.policy.set", None)
        .unwrap();
    mu17j_h_b_install_free_run_cardinality(&store, count);
    let key = durability_facet_test_key(b"documents", &format!("mu17j-h-b-target-{count}"));
    store
        .put_mutable_overlay_value(key.clone(), b"before".to_vec())
        .unwrap();
    (backing, store, key)
}

#[test]
fn mu17j_h_b_metadata_publication_is_bounded_as_free_map_cardinality_grows() {
    let mut measurements = Vec::new();
    for cardinality in [1usize, 8, 32, 64] {
        let (backing, store, key) = mu17j_h_b_prepare_measured_store(cardinality);
        let before = t188_15_roots(&store);
        let before_free_runs = store.inner.lock().unwrap().free.len();
        backing.reset_pages_written();
        store
            .put_mutable_overlay_value(
                key.clone(),
                format!("after-cardinality-{cardinality}").into_bytes(),
            )
            .unwrap();
        let after = t188_15_roots(&store);
        assert_eq!(after.generation, before.generation + 1);
        assert_ne!(after.region_table_root, before.region_table_root);
        assert_ne!(after.maintenance_root, before.maintenance_root);
        assert_ne!(after.freemap_root, before.freemap_root);
        assert_eq!(after.secondary_index_root, before.secondary_index_root);
        assert_eq!(after.retained_history_root, before.retained_history_root);
        assert_eq!(
            after.mutable_idempotency_root,
            before.mutable_idempotency_root
        );
        assert!(backing.wrote_page(after.region_table_root.unwrap()));
        assert!(backing.wrote_page(after.maintenance_root.unwrap()));
        assert!(backing.wrote_page(after.freemap_root.unwrap()));
        let pages_written = backing.pages_written();
        let live_free = store.inner.lock().unwrap().free.clone();
        eprintln!(
            "mu17j_h_b cardinality={cardinality} before_free_runs={before_free_runs} metadata_pages=3 total_data_pages_written={pages_written}"
        );
        assert!(
            pages_written <= 56,
            "metadata-bounded one-key update wrote {pages_written} data pages at free cardinality {cardinality}"
        );
        drop(store);
        let reopened = FileStore::with_backing(Box::new(backing), true).unwrap();
        assert_eq!(reopened.inner.lock().unwrap().free, live_free);
        assert_eq!(
            reopened
                .mutable_overlay_current_entry(&key)
                .unwrap()
                .unwrap()
                .payload,
            format!("after-cardinality-{cardinality}").into_bytes()
        );
        measurements.push((cardinality, pages_written));
    }
    for (cardinality, pages_written) in measurements {
        assert!(
            pages_written <= 56,
            "metadata writes exceeded the path-local split budget at free cardinality {cardinality}: observed={pages_written}"
        );
    }
}

#[test]
fn mu17j_h_c_small_mutation_growth_is_bounded_as_current_state_grows() {
    let backing = CountingMem::default();
    let store = FileStore::with_backing(Box::new(backing.clone()), true).unwrap();
    let mut policy = store.store_policy().unwrap();
    policy.default_durability = StoreDurabilityPolicy::Strict;
    store
        .save_store_policy_audited(policy, None, "store.policy.set", None)
        .unwrap();
    let target_key = durability_facet_test_key(b"documents", "mu17j-h-c-target");
    store
        .put_mutable_overlay_value(target_key.clone(), b"seed".to_vec())
        .unwrap();

    let mut seeded = 0usize;
    let mut measurements = Vec::new();
    for cardinality in [1usize, 32, 128, 512] {
        for index in seeded..cardinality {
            let key =
                durability_facet_test_key(b"documents", &format!("mu17j-h-c-seed-{index:04}"));
            store
                .put_mutable_overlay_value(key, format!("seed-{index:04}").into_bytes())
                .unwrap();
        }
        seeded = cardinality;

        let before = t188_15_roots(&store);
        let before_backing_bytes = backing.size().unwrap();
        backing.reset_io_pages();
        pagebtree::reset_load_all_calls_for_test();
        record_io::reset_record_encode_calls_for_test();
        store
            .put_mutable_overlay_value(
                target_key.clone(),
                format!("small-update-{cardinality}").into_bytes(),
            )
            .unwrap();
        let after = t188_15_roots(&store);
        let after_backing_bytes = backing.size().unwrap();
        let generation_delta = after.generation.saturating_sub(before.generation);
        let page_delta = after.page_count.saturating_sub(before.page_count);
        let pages_per_generation = page_delta / generation_delta.max(1);
        let conservative_metadata_bytes = backing.pages_written().saturating_mul(PAGE_SIZE);
        let committed_logical_page_span_bytes = page_delta.saturating_mul(PAGE_SIZE);
        let actual_backing_byte_delta = after_backing_bytes.saturating_sub(before_backing_bytes);
        let encodes = record_io::record_encode_calls_for_test();
        eprintln!(
            "mu17j_h_c current_records={cardinality} generation_delta={generation_delta} page_delta={page_delta} pages_per_generation={pages_per_generation} data_pages_written={} conservative_metadata_bytes={conservative_metadata_bytes} committed_logical_page_span_bytes={committed_logical_page_span_bytes} actual_backing_byte_delta={actual_backing_byte_delta} record_encodes={encodes}",
            backing.pages_written()
        );
        assert_eq!(generation_delta, 1);
        assert_eq!(pagebtree::load_all_calls_for_test(), 0);
        assert!(
            pages_per_generation <= 64,
            "small mutation grew by {pages_per_generation} pages per generation at current cardinality {cardinality}"
        );
        assert!(
            conservative_metadata_bytes <= 64 * PAGE_SIZE,
            "small mutation wrote {conservative_metadata_bytes} conservative metadata bytes at current cardinality {cardinality}"
        );
        assert!(
            encodes <= 2,
            "small mutation encoded {encodes} records at current cardinality {cardinality}"
        );
        measurements.push((
            cardinality,
            pages_per_generation,
            conservative_metadata_bytes,
            committed_logical_page_span_bytes,
            actual_backing_byte_delta,
        ));
    }

    let baseline_pages = measurements[0].1;
    let baseline_bytes = measurements[0].2;
    for (
        cardinality,
        pages_per_generation,
        conservative_metadata_bytes,
        committed_logical_page_span_bytes,
        actual_backing_byte_delta,
    ) in measurements
    {
        assert!(
            pages_per_generation <= baseline_pages + 16,
            "pages per generation drifted from {baseline_pages} to {pages_per_generation} at current cardinality {cardinality}"
        );
        assert!(
            conservative_metadata_bytes <= baseline_bytes + 16 * PAGE_SIZE,
            "metadata bytes drifted from {baseline_bytes} to {conservative_metadata_bytes} at current cardinality {cardinality}"
        );
        assert_eq!(
            committed_logical_page_span_bytes, actual_backing_byte_delta,
            "logical page span and backing growth diverged at current cardinality {cardinality}"
        );
    }
}

const DIAGNOSTIC_MU17J_E_OPS: usize = 64;
const DIAGNOSTIC_MU17J_E_MAX_GENERATIONS_PER_OP: u64 = 1;
const DIAGNOSTIC_MU17J_E_MAX_PAGES_PER_GENERATION: u64 = 64;
const DIAGNOSTIC_MU17J_E_MAX_FOREGROUND_LATENCY_MS: u128 = 750;
const DIAGNOSTIC_MU17J_E_MIN_THROUGHPUT_OPS_PER_SEC: f64 = 5.0;
const DIAGNOSTIC_MU17J_E_MAX_OVERWRITE_MAINTENANCE_STALE_PAGES: u64 = 64;
const DIAGNOSTIC_MU17J_E_MAX_APPEND_MAINTENANCE_STALE_PAGES_PER_OP: u64 = 3;

#[derive(Debug, Clone)]
struct DiagnosticMu17jEGrowthReport {
    workload: &'static str,
    operations: usize,
    generation_delta: u64,
    page_delta: u64,
    backing_byte_delta: u64,
    stale_pages_before_gc: u64,
    stale_pages_after_gc: u64,
    reusable_pages_before_gc: u64,
    reusable_pages_after_gc: u64,
    max_foreground_latency_ms: u128,
    average_foreground_latency_ms: f64,
    throughput_ops_per_sec: f64,
    mark_slices: usize,
    mark_visited: usize,
    gc_passes: usize,
    gc_pages_freed: u64,
}

fn diagnostic_mu17j_e_class_pages(store: &FileStore, class: impl Fn(&str) -> bool) -> u64 {
    store
        .page_class_attribution(0)
        .unwrap()
        .classes
        .iter()
        .filter(|entry| class(&entry.class))
        .map(|entry| entry.pages)
        .sum()
}

fn diagnostic_mu17j_e_stale_pages(store: &FileStore) -> u64 {
    diagnostic_mu17j_e_class_pages(store, |class| class.starts_with("stale_"))
}

fn diagnostic_mu17j_e_stale_classes(store: &FileStore) -> Vec<(String, u64, Vec<String>)> {
    store
        .page_class_attribution(4)
        .unwrap()
        .classes
        .into_iter()
        .filter(|entry| entry.class.starts_with("stale_"))
        .map(|entry| (entry.class, entry.pages, entry.examples))
        .collect()
}

fn diagnostic_mu17j_e_reusable_pages(store: &FileStore) -> u64 {
    diagnostic_mu17j_e_class_pages(store, |class| class == "reusable_free_page")
}

fn diagnostic_mu17j_e_complete_mark_and_gc(
    loom: &mut Loom<FileStore>,
) -> (usize, usize, usize, u64) {
    let mut mark_slices = 0usize;
    let mut mark_visited = 0usize;
    begin_loom_reachability_mark_epoch(loom).unwrap();
    loop {
        let step = step_loom_reachability_mark_epoch(loom, 64).unwrap();
        mark_visited += step.visited;
        mark_slices += 1;
        if step.completed {
            break;
        }
        assert!(
            mark_slices <= 512,
            "diagnostic MU-17j-e mark traversal did not converge"
        );
    }
    let stats = loom
        .store_mut()
        .gc_validated_segments(GcSegmentBudget::unlimited())
        .unwrap();
    let stale_after = diagnostic_mu17j_e_stale_pages(loom.store());
    eprintln!(
        "diagnostic_mu17j_e gc_passes=1 mark_slices={mark_slices} mark_visited={mark_visited} pages_freed={} stale_after={stale_after} stale_classes={:?}",
        stats.pages_freed,
        diagnostic_mu17j_e_stale_classes(loom.store()),
    );
    (mark_slices, mark_visited, 1, stats.pages_freed)
}

fn diagnostic_mu17j_e_measure_growth(
    workload: &'static str,
    path: &std::path::Path,
    loom: &mut Loom<FileStore>,
    mut operation: impl FnMut(&FileStore, usize),
) -> DiagnosticMu17jEGrowthReport {
    let before_status = loom.store().maintenance_status().unwrap();
    let before_bytes = std::fs::metadata(path).unwrap().len();
    let started = std::time::Instant::now();
    let mut max_latency = std::time::Duration::ZERO;
    let mut total_latency = std::time::Duration::ZERO;
    for index in 0..DIAGNOSTIC_MU17J_E_OPS {
        let op_started = std::time::Instant::now();
        operation(loom.store(), index);
        let latency = op_started.elapsed();
        max_latency = max_latency.max(latency);
        total_latency += latency;
    }
    let elapsed = started.elapsed();
    let after_status = loom.store().maintenance_status().unwrap();
    let after_bytes = std::fs::metadata(path).unwrap().len();
    let generation_delta = after_status
        .generation
        .saturating_sub(before_status.generation);
    let page_delta = after_status
        .physical_page_count
        .saturating_sub(before_status.physical_page_count);
    let stale_pages_before_gc = diagnostic_mu17j_e_stale_pages(loom.store());
    let reusable_pages_before_gc = diagnostic_mu17j_e_reusable_pages(loom.store());
    let (mark_slices, mark_visited, gc_passes, gc_pages_freed) =
        diagnostic_mu17j_e_complete_mark_and_gc(loom);
    let stale_pages_after_gc = diagnostic_mu17j_e_stale_pages(loom.store());
    let reusable_pages_after_gc = diagnostic_mu17j_e_reusable_pages(loom.store());
    let average_foreground_latency_ms =
        total_latency.as_secs_f64() * 1000.0 / DIAGNOSTIC_MU17J_E_OPS as f64;
    let throughput_ops_per_sec = DIAGNOSTIC_MU17J_E_OPS as f64 / elapsed.as_secs_f64();
    let report = DiagnosticMu17jEGrowthReport {
        workload,
        operations: DIAGNOSTIC_MU17J_E_OPS,
        generation_delta,
        page_delta,
        backing_byte_delta: after_bytes.saturating_sub(before_bytes),
        stale_pages_before_gc,
        stale_pages_after_gc,
        reusable_pages_before_gc,
        reusable_pages_after_gc,
        max_foreground_latency_ms: max_latency.as_millis(),
        average_foreground_latency_ms,
        throughput_ops_per_sec,
        mark_slices,
        mark_visited,
        gc_passes,
        gc_pages_freed,
    };
    eprintln!(
        "diagnostic_mu17j_e workload={} ops={} generation_delta={} generations_per_op={:.2} page_delta={} pages_per_generation={} backing_byte_delta={} bytes_per_op={} stale_before_gc={} stale_after_gc={} reusable_before_gc={} reusable_after_gc={} max_latency_ms={} avg_latency_ms={:.3} throughput_ops_per_sec={:.2} mark_slices={} mark_visited={} gc_passes={} gc_pages_freed={}",
        report.workload,
        report.operations,
        report.generation_delta,
        report.generation_delta as f64 / report.operations as f64,
        report.page_delta,
        report.page_delta / report.generation_delta.max(1),
        report.backing_byte_delta,
        report.backing_byte_delta / report.operations as u64,
        report.stale_pages_before_gc,
        report.stale_pages_after_gc,
        report.reusable_pages_before_gc,
        report.reusable_pages_after_gc,
        report.max_foreground_latency_ms,
        report.average_foreground_latency_ms,
        report.throughput_ops_per_sec,
        report.mark_slices,
        report.mark_visited,
        report.gc_passes,
        report.gc_pages_freed
    );
    assert_eq!(
        report.generation_delta,
        report.operations as u64 * DIAGNOSTIC_MU17J_E_MAX_GENERATIONS_PER_OP
    );
    assert!(
        report.page_delta / report.generation_delta.max(1)
            <= DIAGNOSTIC_MU17J_E_MAX_PAGES_PER_GENERATION,
        "diagnostic MU-17j-e {workload} exceeded accepted pages-per-generation bound: {:?}",
        report
    );
    let post_maintenance_stale_page_limit = match workload {
        "overwrite" => DIAGNOSTIC_MU17J_E_MAX_OVERWRITE_MAINTENANCE_STALE_PAGES,
        "append" => {
            report.operations as u64 * DIAGNOSTIC_MU17J_E_MAX_APPEND_MAINTENANCE_STALE_PAGES_PER_OP
        }
        _ => panic!("diagnostic MU-17j-e unknown workload {workload}"),
    };
    assert!(
        report.stale_pages_after_gc <= post_maintenance_stale_page_limit,
        "diagnostic MU-17j-e {workload} exceeded the post-snapshot maintenance residue bound: {:?}",
        report,
    );
    assert!(
        report.max_foreground_latency_ms <= DIAGNOSTIC_MU17J_E_MAX_FOREGROUND_LATENCY_MS,
        "diagnostic MU-17j-e {workload} exceeded foreground latency liveness threshold: {:?}",
        report
    );
    assert!(
        report.throughput_ops_per_sec >= DIAGNOSTIC_MU17J_E_MIN_THROUGHPUT_OPS_PER_SEC,
        "diagnostic MU-17j-e {workload} fell below throughput liveness threshold: {:?}",
        report
    );
    report
}

#[test]
#[ignore = "diagnostic: sustained mutable overwrite and append growth; run via just diagnostic-mu17j-e"]
fn diagnostic_mu17j_e_sustained_overwrite_and_append_growth() {
    let overwrite_path = TempPath::new("diagnostic-mu17j-e-overwrite");
    let mut overwrite_loom = Loom::new(FileStore::open(overwrite_path.path()).unwrap());
    let mut policy = overwrite_loom.store().store_policy().unwrap();
    policy.default_durability = StoreDurabilityPolicy::Strict;
    overwrite_loom
        .store()
        .save_store_policy_audited(policy, None, "store.policy.set", None)
        .unwrap();
    let overwrite_key = durability_facet_test_key(b"documents", "diagnostic-mu17j-e-overwrite");
    overwrite_loom
        .store()
        .put_mutable_overlay_value(overwrite_key.clone(), b"seed".to_vec())
        .unwrap();
    let overwrite_report = diagnostic_mu17j_e_measure_growth(
        "overwrite",
        overwrite_path.path(),
        &mut overwrite_loom,
        |store, index| {
            store
                .put_mutable_overlay_value(
                    overwrite_key.clone(),
                    format!("overwrite-{index:04}").into_bytes(),
                )
                .unwrap();
        },
    );
    assert!(
        overwrite_report.stale_pages_before_gc > 0,
        "sustained overwrite did not create reclaimable stale pages"
    );
    assert!(
        overwrite_report.reusable_pages_after_gc > overwrite_report.reusable_pages_before_gc,
        "validated GC did not increase reusable pages for sustained overwrite"
    );

    let append_path = TempPath::new("diagnostic-mu17j-e-append");
    let mut append_loom = Loom::new(FileStore::open(append_path.path()).unwrap());
    let mut policy = append_loom.store().store_policy().unwrap();
    policy.default_durability = StoreDurabilityPolicy::Strict;
    append_loom
        .store()
        .save_store_policy_audited(policy, None, "store.policy.set", None)
        .unwrap();
    let append_report = diagnostic_mu17j_e_measure_growth(
        "append",
        append_path.path(),
        &mut append_loom,
        |store, index| {
            let key = durability_facet_test_key(
                b"documents",
                &format!("diagnostic-mu17j-e-append-{index:04}"),
            );
            store
                .put_mutable_overlay_value(key, format!("append-{index:04}").into_bytes())
                .unwrap();
        },
    );
    assert!(
        append_report.page_delta / append_report.generation_delta.max(1)
            <= DIAGNOSTIC_MU17J_E_MAX_PAGES_PER_GENERATION
    );
}

#[test]
#[ignore = "diagnostic: external reader lease reuse boundary; run via just diagnostic-mu17j-e"]
fn diagnostic_mu17j_e_external_reader_blocks_reuse_until_release() {
    let path = TempPath::new("diagnostic-mu17j-e-reader");
    let mut loom = Loom::new(FileStore::open(path.path()).unwrap());
    let key = durability_facet_test_key(b"documents", "diagnostic-mu17j-e-reader-overwrite");
    loom.store()
        .put_mutable_overlay_value(key.clone(), b"seed".to_vec())
        .unwrap();
    for index in 0..DIAGNOSTIC_MU17J_E_OPS {
        loom.store()
            .put_mutable_overlay_value(key.clone(), format!("reader-{index:04}").into_bytes())
            .unwrap();
    }
    assert!(diagnostic_mu17j_e_stale_pages(loom.store()) > 0);
    let (mark_slices, mark_visited, gc_passes, gc_pages_freed) =
        diagnostic_mu17j_e_complete_mark_and_gc(&mut loom);
    let (reclaimed_free, horizon) = {
        let inner = loom.store().inner.lock().unwrap();
        (inner.free.clone(), inner.minimum_recoverable_generation)
    };
    let reclaimed_pages = diagnostic_mu17j_e_reusable_pages(loom.store());
    assert!(reclaimed_pages > 0);
    for pair in reclaimed_free.windows(2) {
        assert!(
            pair[0].start.saturating_add(pair[0].len) <= pair[1].start,
            "diagnostic MU-17j-e overlapping free runs before reopen: {:?}",
            pair
        );
    }
    let reader = FileStore::open_read(path.path()).unwrap();
    let (blocked_reusable, _blocked_lease) = loom
        .store()
        .transaction_reusable_free(&reclaimed_free, None, horizon)
        .unwrap();
    assert!(
        blocked_reusable.is_empty(),
        "external reader lease allowed reusable pages while active"
    );
    drop(reader);
    let (allowed_reusable, _allowed_lease) = loom
        .store()
        .transaction_reusable_free(&reclaimed_free, None, horizon)
        .unwrap();
    let allowed_pages = allowed_reusable.iter().map(|run| run.len).sum::<u64>();
    eprintln!(
        "diagnostic_mu17j_e reader ops={} stale_after_gc={} reusable_after_gc={} blocked_reusable_pages=0 allowed_reusable_pages={} mark_slices={} mark_visited={} gc_passes={} gc_pages_freed={}",
        DIAGNOSTIC_MU17J_E_OPS,
        diagnostic_mu17j_e_stale_pages(loom.store()),
        reclaimed_pages,
        allowed_pages,
        mark_slices,
        mark_visited,
        gc_passes,
        gc_pages_freed
    );
    assert!(allowed_pages > 0);
}

#[test]
fn mu17j_h_b_post_commit_pre_adopt_failure_recovers_durable_metadata_roots() {
    let backing = CountingMem::default();
    let store = FileStore::with_backing(Box::new(backing.clone()), true).unwrap();
    mu17j_h_b_install_free_run_cardinality(&store, 32);
    let key = durability_facet_test_key(b"documents", "mu17j-h-b-pre-adopt");
    store
        .put_mutable_overlay_value(key.clone(), b"before".to_vec())
        .unwrap();
    let before = t188_15_roots(&store);
    let observed_roots = Arc::new(Mutex::new(None));
    let observed_roots_for_hook = Arc::clone(&observed_roots);
    store
        .set_post_commit_pre_adopt_hook_for_test(Box::new(move |roots| {
            *observed_roots_for_hook.lock().unwrap() = Some(roots.clone());
            Err(LoomError::new(
                Code::Internal,
                "injected post-commit pre-adopt failure",
            ))
        }))
        .unwrap();
    let error = store
        .put_mutable_overlay_value(key.clone(), b"after".to_vec())
        .unwrap_err();
    assert_eq!(error.code, Code::Internal);
    assert_eq!(t188_15_roots(&store), before);
    drop(store);

    let reopened = FileStore::with_backing(Box::new(backing), true).unwrap();
    let reopened_roots = t188_15_roots(&reopened);
    let committed_roots = observed_roots.lock().unwrap().clone().unwrap();
    assert_eq!(reopened_roots.generation, committed_roots.generation);
    assert_eq!(
        reopened_roots.freemap_root,
        committed_roots.freemap.map(|(root, _)| root)
    );
    assert_eq!(
        reopened_roots.region_table_root,
        Some(committed_roots.region_table_root)
    );
    assert_eq!(
        reopened_roots.maintenance_root,
        Some(committed_roots.maintenance_root)
    );
    assert_ne!(reopened_roots, before);
    assert_eq!(
        reopened
            .mutable_overlay_current_entry(&key)
            .unwrap()
            .unwrap()
            .payload,
        b"after"
    );
}

#[test]
fn mu17j_h_b_extent_tree_with_more_than_4096_extents_reopens_without_delta_replay() {
    let backing = CountingMem::default();
    let store = FileStore::with_backing(Box::new(backing.clone()), true).unwrap();
    mu17j_h_b_install_free_run_cardinality(&store, 4_097);
    let before_free_runs = store.inner.lock().unwrap().free.len();
    assert!(before_free_runs >= 4_097);
    drop(store);

    backing.reset_io_pages();
    let reopened = FileStore::with_backing(Box::new(backing.clone()), true).unwrap();
    let after_free_runs = reopened.inner.lock().unwrap().free.len();
    assert!(after_free_runs >= 4_097);
    let pages_read = backing.pages_read();
    eprintln!(
        "mu17j_h_b extents=4097 free_runs={after_free_runs} reopen_data_pages_read={pages_read}"
    );
    assert!(
        pages_read <= (after_free_runs as u64).saturating_mul(3),
        "reopen read {pages_read} data pages for {after_free_runs} current extents"
    );
}

#[test]
fn mu17j_h_b_more_than_4096_successive_freemap_publications_reopen_from_current_tree() {
    let backing = CountingMem::default();
    let mut file = backing.clone();
    let mut alloc = PageAllocator::new(20_000, 1, Vec::new());
    file.grow(DATA_START + alloc.page_count() * PAGE_SIZE)
        .unwrap();
    let mut root = None;
    backing.reset_io_pages();
    for index in 0..4_097u64 {
        alloc.free(PageId(128 + index * 2), 1).unwrap();
        let updates = alloc.take_free_map_extent_updates();
        root =
            pagemap::write_tree_map(&mut file, DATA_START, &mut alloc, root, &[], updates).unwrap();
        assert_eq!(alloc.pending_free_map_extent_update_count(), 0);
    }
    let root = root.unwrap();
    let pages_written = backing.pages_written();

    backing.reset_pages_read();
    let (runs, _) =
        pagemap::read_map_with_root_span(&mut file, DATA_START, root, alloc.page_count()).unwrap();
    let pages_read = backing.pages_read();
    eprintln!(
        "mu17j_h_b successive_freemap_publications=4097 current_free_runs={} publication_data_pages_written={pages_written} reopen_data_pages_read={pages_read}",
        runs.len()
    );
    assert_eq!(runs.len(), 4_097);
    assert!(
        pages_read <= (runs.len() as u64).saturating_mul(3),
        "reopen read {pages_read} data pages for {} current extents",
        runs.len()
    );
}

fn mu17j_h_b_completed_state(live: Digest) -> loom_core::ReachabilityMarkState {
    loom_core::ReachabilityMarkState {
        pinned: BTreeSet::from([live]),
        marked: BTreeSet::from([live]),
        queue: std::collections::VecDeque::new(),
        stream_roots: std::collections::VecDeque::new(),
        content_roots: std::collections::VecDeque::new(),
        prolly_cursors: std::collections::VecDeque::new(),
        completed: true,
    }
}

fn mu17j_h_b_advance_metadata_epoch(
    store: &FileStore,
    epoch: &mut ReachabilityMarkEpoch,
    budget: usize,
) -> usize {
    let visited = store
        .step_reachability_metadata_mark_epoch(epoch, budget, None)
        .unwrap();
    store.save_reachability_mark_epoch(epoch).unwrap();
    visited
}

fn mu17j_h_b_epoch_with_large_metadata_value(store: &FileStore) -> (ReachabilityMarkEpoch, u64) {
    let live = store.put(b"mu17j-h-b-large-metadata-live-root").unwrap();
    store.set_reference_root(Some(live)).unwrap();
    let payload = vec![0x5au8; (PAGE_SIZE as usize * 3) + 777];
    let value_root = {
        let mut inner = store.inner.lock().unwrap();
        let mut file = store.file.lock().unwrap();
        let mut alloc =
            PageAllocator::new(inner.page_count, inner.generation + 1, inner.free.clone());
        let key = *Digest::blake3(b"mu17j-h-b-large-metadata-value-key").bytes();
        let placements =
            record_io::write_blob_pages(&mut **file, &mut alloc, &[(key, payload.as_slice())])
                .unwrap();
        let loc = placements[0].1;
        let root = pagebtree::insert(
            &mut **file,
            DATA_START,
            &mut alloc,
            None,
            &key,
            loc,
            inner.page_count,
        )
        .unwrap();
        inner.page_count = alloc.page_count();
        inner.maintenance.physical_page_count = inner.page_count;
        root.0
    };
    let mut epoch = store
        .begin_reachability_mark_epoch(Some(live), BTreeSet::new(), mu17j_h_b_completed_state(live))
        .unwrap();
    epoch.captured_metadata_roots.clear();
    epoch.captured_metadata_value_roots = vec![value_root];
    epoch.metadata_work_initialized = false;
    epoch.metadata_root_cursor = 0;
    epoch.metadata_value_root_cursor = 0;
    epoch.metadata_value_blob_cursor = 0;
    epoch.metadata_expansion_cursor = 0;
    epoch.metadata_classify_next_page = 0;
    epoch.metadata_evidence_root = None;
    epoch.metadata_reachable_count = 0;
    epoch.metadata_reclaim_candidate_count = 0;
    epoch.metadata_completed = false;
    store.save_reachability_mark_epoch(&epoch).unwrap();
    (epoch, value_root)
}

fn mu17j_h_b_epoch_with_wide_metadata_value_root(store: &FileStore) -> ReachabilityMarkEpoch {
    let live = store.put(b"mu17j-h-b-wide-metadata-live-root").unwrap();
    store.set_reference_root(Some(live)).unwrap();
    let value_root = {
        let mut inner = store.inner.lock().unwrap();
        let mut file = store.file.lock().unwrap();
        let mut alloc =
            PageAllocator::new(inner.page_count, inner.generation + 1, inner.free.clone());
        let mut root = None;
        for index in 0..40u64 {
            let key = *Digest::blake3(format!("mu17j-h-b-wide-key-{index}").as_bytes()).bytes();
            let payload = format!("mu17j-h-b-wide-value-{index}").into_bytes();
            let placements =
                record_io::write_blob_pages(&mut **file, &mut alloc, &[(key, payload.as_slice())])
                    .unwrap();
            let bound = alloc.page_count();
            root = Some(
                pagebtree::insert(
                    &mut **file,
                    DATA_START,
                    &mut alloc,
                    root,
                    &key,
                    placements[0].1,
                    bound,
                )
                .unwrap(),
            );
        }
        inner.page_count = alloc.page_count();
        inner.maintenance.physical_page_count = inner.page_count;
        root.unwrap().0
    };
    let mut epoch = store
        .begin_reachability_mark_epoch(Some(live), BTreeSet::new(), mu17j_h_b_completed_state(live))
        .unwrap();
    epoch.captured_metadata_roots.clear();
    epoch.captured_metadata_value_roots = vec![value_root];
    epoch.metadata_work_initialized = false;
    epoch.metadata_root_cursor = 0;
    epoch.metadata_value_root_cursor = 0;
    epoch.metadata_value_blob_cursor = 0;
    epoch.metadata_expansion_cursor = 0;
    epoch.metadata_classify_next_page = 0;
    epoch.metadata_evidence_root = None;
    epoch.metadata_reachable_count = 0;
    epoch.metadata_reclaim_candidate_count = 0;
    epoch.metadata_completed = false;
    store.save_reachability_mark_epoch(&epoch).unwrap();
    epoch
}

#[test]
fn mu17j_h_b_tiny_metadata_epoch_slice_requires_resumable_calls() {
    let tp = TempPath::new("mu17j-h-b-tiny-metadata-slice");
    let store = FileStore::open(tp.path()).unwrap();
    let live = store.put(b"mu17j-h-b-live-root").unwrap();
    store.set_reference_root(Some(live)).unwrap();
    mu17j_h_b_install_free_run_cardinality(&store, 32);
    store
        .put_mutable_overlay_value(
            durability_facet_test_key(b"documents", "mu17j-h-b-tiny-slice"),
            b"value".to_vec(),
        )
        .unwrap();
    let mut epoch = store
        .begin_reachability_mark_epoch(Some(live), BTreeSet::new(), mu17j_h_b_completed_state(live))
        .unwrap();
    let first = mu17j_h_b_advance_metadata_epoch(&store, &mut epoch, 1);
    assert_eq!(first, 1);
    assert!(!epoch.metadata_completed);
    let mut calls = 1usize;
    while !epoch.metadata_completed {
        assert!(mu17j_h_b_advance_metadata_epoch(&store, &mut epoch, 1) <= 1);
        calls += 1;
        assert!(calls < 10_000);
    }
    assert!(calls > 1);
    assert_eq!(
        epoch.metadata_classify_next_page,
        epoch.page_high_water_mark
    );
}

#[test]
fn mu17j_h_b_wide_root_expansion_is_budgeted_and_reopen_resumable() {
    let incremental_path = TempPath::new("mu17j-h-b-wide-root-incremental");
    let uninterrupted_path = TempPath::new("mu17j-h-b-wide-root-uninterrupted");
    let incremental = FileStore::open(incremental_path.path()).unwrap();
    let uninterrupted = FileStore::open(uninterrupted_path.path()).unwrap();
    let mut incremental_epoch = mu17j_h_b_epoch_with_wide_metadata_value_root(&incremental);
    let mut uninterrupted_epoch = mu17j_h_b_epoch_with_wide_metadata_value_root(&uninterrupted);

    let mut calls = 0usize;
    let mut previous_chunks = 0usize;
    while !incremental_epoch.metadata_completed && calls < 2 {
        let visited = mu17j_h_b_advance_metadata_epoch(&incremental, &mut incremental_epoch, 1);
        let chunks = incremental
            .reachability_mark_metadata_evidence_chunk_count_for_test(&incremental_epoch)
            .unwrap();
        assert!(visited <= 1);
        assert!(chunks <= previous_chunks + 1);
        previous_chunks = chunks;
        calls += 1;
    }
    assert!(!incremental_epoch.metadata_completed);
    assert!(calls > 1);

    drop(incremental);
    let reopened = FileStore::open(incremental_path.path()).unwrap();
    incremental_epoch = reopened.active_reachability_mark_epoch().unwrap().unwrap();
    let resumed_expansion = incremental_epoch.metadata_expansion_cursor;
    while !incremental_epoch.metadata_completed {
        let visited = mu17j_h_b_advance_metadata_epoch(&reopened, &mut incremental_epoch, 1);
        assert!(visited <= 1);
        assert!(incremental_epoch.metadata_expansion_cursor >= resumed_expansion);
        assert!(calls < 512);
        calls += 1;
    }

    while !uninterrupted_epoch.metadata_completed {
        mu17j_h_b_advance_metadata_epoch(&uninterrupted, &mut uninterrupted_epoch, 64);
    }
    assert_eq!(
        incremental_epoch.metadata_reachable_count,
        uninterrupted_epoch.metadata_reachable_count
    );
    assert_eq!(
        incremental_epoch.metadata_reclaim_candidate_count,
        uninterrupted_epoch.metadata_reclaim_candidate_count
    );
}

#[test]
fn mu17j_h_b_large_metadata_value_traversal_is_budgeted_and_reopen_resumable() {
    let incremental_path = TempPath::new("mu17j-h-b-large-value-incremental");
    let uninterrupted_path = TempPath::new("mu17j-h-b-large-value-uninterrupted");
    let incremental = FileStore::open(incremental_path.path()).unwrap();
    let uninterrupted = FileStore::open(uninterrupted_path.path()).unwrap();
    let (mut incremental_epoch, _) = mu17j_h_b_epoch_with_large_metadata_value(&incremental);
    let (mut uninterrupted_epoch, _) = mu17j_h_b_epoch_with_large_metadata_value(&uninterrupted);

    let first_len = mark_epoch::encoded_mark_epoch_len_for_test(&incremental_epoch);
    let first = mu17j_h_b_advance_metadata_epoch(&incremental, &mut incremental_epoch, 1);
    assert_eq!(first, 1);
    assert!(!incremental_epoch.metadata_completed);
    assert!(mark_epoch::encoded_mark_epoch_len_for_test(&incremental_epoch) < 4096);
    assert!(mark_epoch::encoded_mark_epoch_len_for_test(&incremental_epoch) >= first_len);

    drop(incremental);
    let reopened = FileStore::open(incremental_path.path()).unwrap();
    incremental_epoch = reopened.active_reachability_mark_epoch().unwrap().unwrap();
    let resumed_blob_cursor = incremental_epoch.metadata_value_blob_cursor;
    let mut calls = 1usize;
    let mut previous_chunks = reopened
        .reachability_mark_metadata_evidence_chunk_count_for_test(&incremental_epoch)
        .unwrap();
    while !incremental_epoch.metadata_completed {
        let visited = mu17j_h_b_advance_metadata_epoch(&reopened, &mut incremental_epoch, 1);
        let after_len = mark_epoch::encoded_mark_epoch_len_for_test(&incremental_epoch);
        let chunks = reopened
            .reachability_mark_metadata_evidence_chunk_count_for_test(&incremental_epoch)
            .unwrap();
        assert!(visited <= 1);
        assert!(after_len < 4096);
        assert!(chunks <= previous_chunks + 1);
        previous_chunks = chunks;
        calls += 1;
        assert!(
            calls < 256,
            "large metadata value traversal did not converge"
        );
    }
    assert!(calls > 4);
    assert!(incremental_epoch.metadata_value_blob_cursor >= resumed_blob_cursor);

    while !uninterrupted_epoch.metadata_completed {
        mu17j_h_b_advance_metadata_epoch(&uninterrupted, &mut uninterrupted_epoch, 32);
    }
    assert_eq!(
        incremental_epoch.metadata_reachable_count,
        uninterrupted_epoch.metadata_reachable_count
    );
    assert_eq!(
        incremental_epoch.metadata_reclaim_candidate_count,
        uninterrupted_epoch.metadata_reclaim_candidate_count
    );
    let incremental_evidence = mark_epoch::ReachabilityMarkReclaimEvidence {
        epoch: incremental_epoch.epoch,
        base_generation: incremental_epoch.base_generation,
        reclaim_fence_identity: incremental_epoch.reclaim_fence_identity,
        page_high_water_mark: incremental_epoch.page_high_water_mark,
        captured_root_identity: Digest::blake3(b"test"),
        captured_metadata_bootstrap_reserve: incremental_epoch
            .captured_metadata_bootstrap_reserve
            .clone(),
        metadata_bootstrap_evidence_provenance:
            mark_epoch::MetadataBootstrapEvidenceProvenance::Current,
        captured_free_root: incremental_epoch.captured_free_root,
        captured_free_identity: incremental_epoch.captured_free_identity,
        captured_free_consumed_through: incremental_epoch.captured_free_consumed_through,
        metadata_evidence_root: incremental_epoch.metadata_evidence_root,
        metadata_reclaim_candidate_count: incremental_epoch.metadata_reclaim_candidate_count,
        metadata_evidence_identity: incremental_epoch.metadata_evidence_identity,
        unreachable_pre_snapshot_pages: BTreeSet::new(),
    };
    let uninterrupted_evidence = mark_epoch::ReachabilityMarkReclaimEvidence {
        epoch: uninterrupted_epoch.epoch,
        base_generation: uninterrupted_epoch.base_generation,
        reclaim_fence_identity: uninterrupted_epoch.reclaim_fence_identity,
        page_high_water_mark: uninterrupted_epoch.page_high_water_mark,
        captured_root_identity: Digest::blake3(b"test"),
        captured_metadata_bootstrap_reserve: uninterrupted_epoch
            .captured_metadata_bootstrap_reserve
            .clone(),
        metadata_bootstrap_evidence_provenance:
            mark_epoch::MetadataBootstrapEvidenceProvenance::Current,
        captured_free_root: uninterrupted_epoch.captured_free_root,
        captured_free_identity: uninterrupted_epoch.captured_free_identity,
        captured_free_consumed_through: uninterrupted_epoch.captured_free_consumed_through,
        metadata_evidence_root: uninterrupted_epoch.metadata_evidence_root,
        metadata_reclaim_candidate_count: uninterrupted_epoch.metadata_reclaim_candidate_count,
        metadata_evidence_identity: uninterrupted_epoch.metadata_evidence_identity,
        unreachable_pre_snapshot_pages: BTreeSet::new(),
    };
    assert_eq!(
        reopened
            .reachability_mark_metadata_reclaim_candidate_pages(
                &incremental_evidence,
                reopened.inner.lock().unwrap().page_count,
                u64::MAX,
            )
            .unwrap(),
        uninterrupted
            .reachability_mark_metadata_reclaim_candidate_pages(
                &uninterrupted_evidence,
                uninterrupted.inner.lock().unwrap().page_count,
                u64::MAX,
            )
            .unwrap()
    );
}

#[test]
fn mu17j_h_b_prior_epoch_and_chunk_formats_decode_without_new_layout_aliasing() {
    let tp = TempPath::new("mu17j-h-b-old-format-restart");
    let store = FileStore::open(tp.path()).unwrap();
    let live = store.put(b"mu17j-h-b-old-format-live-root").unwrap();
    store.set_reference_root(Some(live)).unwrap();
    let mut epoch = store
        .begin_reachability_mark_epoch(Some(live), BTreeSet::new(), mu17j_h_b_completed_state(live))
        .unwrap();
    epoch.metadata_work_initialized = true;
    epoch.metadata_root_cursor = 10;
    epoch.metadata_value_root_cursor = 11;
    epoch.metadata_value_blob_cursor = 12;
    epoch.metadata_expansion_cursor = 13;
    epoch.metadata_classify_next_page = 13;
    epoch.metadata_evidence_root = Some(14);
    epoch.metadata_reachable_count = 15;
    epoch.metadata_reclaim_candidate_count = 16;
    epoch.metadata_completed = false;
    let old_bytes = mark_epoch::encode_mark_epoch_v8_for_test(&epoch);
    let restarted = mark_epoch::decode_mark_epoch_for_test(&old_bytes, Algo::Blake3).unwrap();
    assert!(!restarted.metadata_work_initialized);
    assert_eq!(restarted.metadata_root_cursor, 0);
    assert_eq!(restarted.metadata_value_root_cursor, 0);
    assert_eq!(restarted.metadata_value_blob_cursor, 0);
    assert_eq!(restarted.metadata_expansion_cursor, 0);
    assert_eq!(restarted.metadata_classify_next_page, 0);
    assert_eq!(restarted.metadata_evidence_root, None);
    assert_eq!(restarted.metadata_reachable_count, 0);
    assert_eq!(restarted.metadata_reclaim_candidate_count, 0);
    assert!(!restarted.metadata_completed);
    for (metadata_roots, metadata_value_roots, metadata_pages) in [
        (vec![], vec![], vec![]),
        (vec![2], vec![4], vec![6]),
        (vec![2, 3], vec![4, 5], vec![6, 7]),
    ] {
        for completed in [false, true] {
            epoch.metadata_completed = completed;
            let queue_bytes = mark_epoch::encode_mark_epoch_v8_queue_layout_for_test(
                &epoch,
                &metadata_roots,
                &metadata_value_roots,
                &metadata_pages,
            );
            let queue_restarted =
                mark_epoch::decode_mark_epoch_for_test(&queue_bytes, Algo::Blake3).unwrap();
            assert!(!queue_restarted.metadata_work_initialized);
            assert_eq!(queue_restarted.metadata_root_cursor, 0);
            assert_eq!(queue_restarted.metadata_value_root_cursor, 0);
            assert_eq!(queue_restarted.metadata_value_blob_cursor, 0);
            assert_eq!(queue_restarted.metadata_expansion_cursor, 0);
            assert_eq!(queue_restarted.metadata_evidence_root, None);
            assert!(!queue_restarted.metadata_completed);
        }
    }
    for completed in [false, true] {
        epoch.metadata_completed = completed;
        let scalar_bytes = mark_epoch::encode_mark_epoch_v8_for_test(&epoch);
        let scalar_restarted =
            mark_epoch::decode_mark_epoch_for_test(&scalar_bytes, Algo::Blake3).unwrap();
        assert!(!scalar_restarted.metadata_work_initialized);
        assert_eq!(scalar_restarted.metadata_root_cursor, 0);
        assert_eq!(scalar_restarted.metadata_value_root_cursor, 0);
        assert_eq!(scalar_restarted.metadata_value_blob_cursor, 0);
        assert_eq!(scalar_restarted.metadata_expansion_cursor, 0);
        assert_eq!(scalar_restarted.metadata_evidence_root, None);
        assert!(!scalar_restarted.metadata_completed);
    }
    assert_eq!(
        mark_epoch::metadata_chunk_v1_decodes_for_test().unwrap(),
        (true, true)
    );
}

#[test]
fn mu17j_h_b_metadata_epoch_reopen_resumes_monotonically_between_slices() {
    let tp = TempPath::new("mu17j-h-b-reopen-metadata-slice");
    {
        let store = FileStore::open(tp.path()).unwrap();
        let live = store.put(b"mu17j-h-b-live-root").unwrap();
        store.set_reference_root(Some(live)).unwrap();
        mu17j_h_b_install_free_run_cardinality(&store, 64);
        let mut epoch = store
            .begin_reachability_mark_epoch(
                Some(live),
                BTreeSet::new(),
                mu17j_h_b_completed_state(live),
            )
            .unwrap();
        mu17j_h_b_advance_metadata_epoch(&store, &mut epoch, 3);
        assert!(epoch.metadata_classify_next_page < epoch.page_high_water_mark);
    }
    let reopened = FileStore::open(tp.path()).unwrap();
    let mut epoch = reopened.active_reachability_mark_epoch().unwrap().unwrap();
    let resumed_from = epoch.metadata_classify_next_page;
    assert!(resumed_from < epoch.page_high_water_mark);
    mu17j_h_b_advance_metadata_epoch(&reopened, &mut epoch, 2);
    assert!(epoch.metadata_classify_next_page >= resumed_from);
    while !epoch.metadata_completed {
        mu17j_h_b_advance_metadata_epoch(&reopened, &mut epoch, 16);
    }
    assert_eq!(
        epoch.metadata_classify_next_page,
        epoch.page_high_water_mark
    );
}

#[test]
fn mu17j_h_b_epoch_record_size_and_chunk_touches_stay_bounded_as_pages_grow() {
    let tp = TempPath::new("mu17j-h-b-bounded-epoch-record-size");
    let store = FileStore::open(tp.path()).unwrap();
    let live = store.put(b"mu17j-h-b-live-root").unwrap();
    store.set_reference_root(Some(live)).unwrap();
    mu17j_h_b_install_free_run_cardinality(&store, 4_097);
    let mut epoch = store
        .begin_reachability_mark_epoch(Some(live), BTreeSet::new(), mu17j_h_b_completed_state(live))
        .unwrap();
    let mut calls = 0usize;
    let mut previous_chunks = 0usize;
    while !epoch.metadata_completed {
        let before_len = mark_epoch::encoded_mark_epoch_len_for_test(&epoch);
        mu17j_h_b_advance_metadata_epoch(&store, &mut epoch, 1);
        let after_len = mark_epoch::encoded_mark_epoch_len_for_test(&epoch);
        let chunks = store
            .reachability_mark_metadata_evidence_chunk_count_for_test(&epoch)
            .unwrap();
        eprintln!(
            "mu17j_h_b bounded_epoch_slice={calls} before_len={before_len} after_len={after_len} chunks={chunks}"
        );
        assert!(after_len < 4096);
        assert!(chunks <= previous_chunks + 1);
        previous_chunks = chunks;
        calls += 1;
        assert!(calls < 128);
    }
    assert!(calls > 1);
}

#[test]
fn mu17j_h_b_foreground_writes_remain_safe_during_metadata_epoch_traversal() {
    let tp = TempPath::new("mu17j-h-b-foreground-write-during-metadata");
    let mut store = FileStore::open(tp.path()).unwrap();
    let live = store.put(b"mu17j-h-b-live-root").unwrap();
    store.set_reference_root(Some(live)).unwrap();
    mu17j_h_b_install_free_run_cardinality(&store, 16);
    let mut epoch = store
        .begin_reachability_mark_epoch(Some(live), BTreeSet::new(), mu17j_h_b_completed_state(live))
        .unwrap();
    mu17j_h_b_advance_metadata_epoch(&store, &mut epoch, 2);
    let post_snapshot_key =
        durability_facet_test_key(b"documents", "mu17j-h-b-post-snapshot-write");
    store
        .put_mutable_overlay_value(post_snapshot_key.clone(), b"after".to_vec())
        .unwrap();
    while !epoch.metadata_completed {
        mu17j_h_b_advance_metadata_epoch(&store, &mut epoch, 8);
    }
    store.complete_reachability_mark_epoch(&epoch).unwrap();
    store
        .gc_validated_segments(GcSegmentBudget {
            max_segments: u64::MAX,
            max_pages: u64::MAX,
        })
        .unwrap();
    assert_eq!(
        store
            .mutable_overlay_current_entry(&post_snapshot_key)
            .unwrap()
            .unwrap()
            .payload,
        b"after"
    );
}

#[test]
fn mu17j_l_captured_free_age_uses_the_immutable_epoch_snapshot() {
    let tp = TempPath::new("mu17j-l-captured-free-age");
    let store = FileStore::open(tp.path()).unwrap();
    for generation in 0..REUSE_SAFE_WINDOW {
        store
            .put(format!("mu17j-l-captured-free-age-{generation}").as_bytes())
            .unwrap();
    }
    let live = store.put(b"mu17j-l-captured-free-live-root").unwrap();
    store.set_reference_root(Some(live)).unwrap();
    mu17j_h_b_install_free_run_cardinality(&store, 16);
    let epoch = store
        .begin_reachability_mark_epoch(Some(live), BTreeSet::new(), mu17j_h_b_completed_state(live))
        .unwrap();
    let current_generation = epoch
        .base_generation
        .saturating_add(REUSE_SAFE_WINDOW)
        .saturating_add(1);
    let mut current_free = store.inner.lock().unwrap().free.clone();
    for run in &mut current_free {
        run.freed_gen = current_generation;
    }
    let selected = {
        let mut file = store.file.lock().unwrap();
        mark_epoch::captured_free_reuse_runs(
            &mut **file,
            store.digest_algo,
            &epoch,
            &current_free,
            current_generation,
            1,
        )
        .unwrap()
    };
    assert_eq!(selected.runs.iter().map(|run| run.len).sum::<u64>(), 1);
    assert!(selected.runs[0].freed_gen < current_generation);
}

struct CapturedFreeSelectionFixture {
    path: TempPath,
    store: FileStore,
    epoch: ReachabilityMarkEpoch,
    young: FreePageRun,
    older_first: FreePageRun,
    older_second: FreePageRun,
    captured: Vec<FreePageRun>,
}

fn captured_free_selection_fixture(tag: &str) -> CapturedFreeSelectionFixture {
    let path = TempPath::new(tag);
    let store = FileStore::open(path.path()).unwrap();
    let (young, older_first, older_second, page_count) = {
        let mut inner = store.inner.lock().unwrap();
        let sacrificial = FreePageRun {
            start: inner.page_count.saturating_add(32),
            len: 24,
            freed_gen: 0,
        };
        let young = FreePageRun {
            start: sacrificial.start.saturating_add(40),
            len: 128,
            freed_gen: u64::MAX / 2,
        };
        let older_first = FreePageRun {
            start: young.start.saturating_add(160),
            len: 64,
            freed_gen: 0,
        };
        let older_second = FreePageRun {
            start: older_first.start.saturating_add(96),
            len: 32,
            freed_gen: 0,
        };
        let page_count = older_second.start.saturating_add(64);
        inner.free = vec![sacrificial, young, older_first, older_second];
        inner.page_count = page_count;
        inner.maintenance.physical_page_count = page_count;
        inner.active_mark_epoch_reclaim_fence = None;
        store
            .file
            .lock()
            .unwrap()
            .grow(DATA_START + page_count * PAGE_SIZE)
            .unwrap();
        (young, older_first, older_second, page_count)
    };
    let mut control = BTreeMap::new();
    control.insert(b"captured-free-selection/fixture".to_vec(), vec![1]);
    store.commit_raw_control_map_for_test(control).unwrap();
    {
        let mut inner = store.inner.lock().unwrap();
        assert!(inner.freemap.is_some());
        let page_count = inner.page_count;
        inner.active_mark_epoch_reclaim_fence = Some(page_count);
    }
    let mut policy = store.store_policy().unwrap();
    policy.default_durability = StoreDurabilityPolicy::Strict;
    store
        .save_store_policy_audited(policy, None, "store.policy.set", None)
        .unwrap();
    let live = store.put(b"captured-free-selection-live").unwrap();
    store.set_reference_root(Some(live)).unwrap();
    for generation in 0..REUSE_SAFE_WINDOW {
        store
            .put(format!("captured-free-selection-aging-{generation}").as_bytes())
            .unwrap();
    }
    {
        let inner = store.inner.lock().unwrap();
        for expected in [young, older_first, older_second] {
            assert!(inner.free.iter().any(|run| {
                run.start <= expected.start
                    && run.start.saturating_add(run.len)
                        >= expected.start.saturating_add(expected.len)
                    && run.freed_gen == expected.freed_gen
            }));
        }
        assert!(inner.page_count >= page_count);
    }
    store
        .set_active_reachability_mark_epoch_reclaim_fence(None)
        .unwrap();
    let epoch = store
        .begin_reachability_mark_epoch(Some(live), BTreeSet::new(), mu17j_h_b_completed_state(live))
        .unwrap();
    let captured = {
        let mut file = store.file.lock().unwrap();
        pagemap::read_map_with_root_span(
            &mut **file,
            DATA_START,
            PageId(epoch.captured_free_root.expect("captured free root")),
            epoch.page_high_water_mark,
        )
        .unwrap()
        .0
    };
    assert!(
        [young, older_first, older_second]
            .iter()
            .all(|expected| captured.contains(expected)),
        "captured={captured:?}"
    );
    CapturedFreeSelectionFixture {
        path,
        store,
        epoch,
        young,
        older_first,
        older_second,
        captured,
    }
}

fn captured_free_selection_current_free(
    fixture: &CapturedFreeSelectionFixture,
) -> Vec<FreePageRun> {
    vec![
        fixture.young,
        FreePageRun {
            start: fixture.older_first.start,
            len: 1,
            freed_gen: fixture.older_first.freed_gen,
        },
        FreePageRun {
            start: fixture.older_first.start + 2,
            len: fixture.older_first.len - 2,
            freed_gen: fixture.older_first.freed_gen,
        },
        fixture.older_second,
    ]
}

#[test]
fn mu17j_l_e_captured_free_selection_persists_exact_cursor_and_digest() {
    let fixture = captured_free_selection_fixture("mu17j-l-e-selection");
    let current_free = captured_free_selection_current_free(&fixture);
    let current_generation = fixture
        .epoch
        .base_generation
        .saturating_add(REUSE_SAFE_WINDOW)
        .saturating_add(1);
    let selection = {
        let mut file = fixture.store.file.lock().unwrap();
        mark_epoch::captured_free_reuse_runs(
            &mut **file,
            fixture.store.digest_algo,
            &fixture.epoch,
            &current_free,
            current_generation,
            64,
        )
        .unwrap()
    };
    assert_eq!(
        selection.runs,
        vec![
            FreePageRun {
                start: fixture.older_first.start,
                len: 1,
                freed_gen: 0,
            },
            FreePageRun {
                start: fixture.older_first.start + 2,
                len: fixture.older_first.len - 2,
                freed_gen: 0,
            },
            FreePageRun {
                start: fixture.older_second.start,
                len: 1,
                freed_gen: 0,
            },
        ]
    );
    let expected_cursor = fixture.young.len + fixture.older_first.len + 1;
    assert_eq!(selection.consumed_through, expected_cursor);
    let selected_pages = selection
        .runs
        .iter()
        .flat_map(|run| run.start..run.start + run.len)
        .collect::<BTreeSet<_>>();
    assert_eq!(selected_pages.len(), 64);
    assert!(
        selected_pages.is_subset(
            &current_free
                .iter()
                .flat_map(|run| run.start..run.start + run.len)
                .collect()
        )
    );
    assert!(!selected_pages.contains(&(fixture.older_first.start + 1)));
    assert!(!selected_pages.contains(&(fixture.older_second.start + 1)));
    let mut direct_control_map = fixture.store.control_root_map().unwrap();
    let direct_epoch = mark_epoch::advance_captured_free_consumption_in_control_map(
        &mut direct_control_map,
        &fixture.epoch,
        expected_cursor,
        fixture.store.digest_algo,
    )
    .unwrap();
    let consumed = {
        let mut file = fixture.store.file.lock().unwrap();
        mark_epoch::captured_free_consumed_runs(
            &mut **file,
            fixture.store.digest_algo,
            &direct_epoch,
        )
        .unwrap()
    };
    let consumed_pages = consumed
        .iter()
        .flat_map(|run| run.start..run.start + run.len)
        .collect::<BTreeSet<_>>();
    let conservatively_consumed = consumed_pages
        .difference(&selected_pages)
        .copied()
        .collect::<BTreeSet<_>>();
    let mut expected_conservative =
        (fixture.young.start..fixture.young.start + fixture.young.len).collect::<BTreeSet<_>>();
    expected_conservative.insert(fixture.older_first.start + 1);
    assert_eq!(conservatively_consumed, expected_conservative);

    {
        let mut inner = fixture.store.inner.lock().unwrap();
        inner.free = current_free.clone();
    }
    let (authority, prepared) = {
        let mut inner = fixture.store.inner.lock().unwrap();
        let control_map = fixture.store.control_map_locked(&mut inner).unwrap();
        let authority = fixture
            .store
            .begin_foreground_transaction_publication(&inner, control_map)
            .unwrap();
        let mut allocator = PageAllocator::new_with_reusable_authorities(
            inner.page_count,
            inner.generation + 1,
            inner.free.clone(),
            authority.ordinary_reusable_runs.clone(),
            authority.publication_eligible_runs.clone(),
        );
        allocator
            .install_captured_free_authority(authority.captured_free_authority.clone())
            .unwrap();
        let mut file = fixture.store.file.lock().unwrap();
        let prepared = fixture
            .store
            .prepare_foreground_transaction_finalization(
                &mut **file,
                &inner,
                &allocator,
                &authority,
                inner.index_root,
            )
            .unwrap();
        (authority, prepared)
    };
    let total_demand = prepared
        .control_frame
        .as_ref()
        .map_or(0, |frame| prepared_record_page_allocations(&frame.frame))
        .saturating_add(prepared.index_delta.allocation_calls())
        .saturating_add(2);
    assert!(total_demand > 0);
    let plan_generation = fixture.store.inner.lock().unwrap().generation + 1;
    let mut expected_allocator = PageAllocator::new_with_reusable_authorities(
        fixture.store.inner.lock().unwrap().page_count,
        plan_generation,
        current_free.clone(),
        authority.ordinary_reusable_runs.clone(),
        authority.publication_eligible_runs.clone(),
    );
    expected_allocator
        .install_captured_free_authority(authority.captured_free_authority.clone())
        .unwrap();
    let expected_plan_runs = expected_allocator
        .select_captured_publication_reserve(prepared.publication_reserve_pages)
        .unwrap();
    let expected_plan_cursor = expected_allocator.captured_free_consumed_through().unwrap();
    assert_eq!(prepared.selected_publication_runs, expected_plan_runs);
    assert!(authority.ordinary_reusable_runs.is_empty());
    assert!(prepared.selected_publication_runs.iter().all(|selected| {
        authority.publication_eligible_runs.iter().any(|eligible| {
            selected.start >= eligible.start
                && selected.start + selected.len <= eligible.start + eligible.len
                && selected.freed_gen == eligible.freed_gen
        })
    }));
    let mut expected_control_map = fixture.store.control_root_map().unwrap();
    let _planned_next_epoch = mark_epoch::advance_captured_free_consumption_in_control_map(
        &mut expected_control_map,
        &fixture.epoch,
        expected_plan_cursor,
        fixture.store.digest_algo,
    )
    .unwrap();
    let expected_control_bytes = record_io::encode_control_map(&expected_control_map);
    let expected_control_digest = Digest::hash(fixture.store.digest_algo, &expected_control_bytes);
    assert_eq!(prepared.control, Some(*expected_control_digest.bytes()));
    let planned_pages = prepared
        .selected_publication_runs
        .iter()
        .flat_map(|run| run.start..run.start + run.len)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        planned_pages.len(),
        prepared
            .selected_publication_runs
            .iter()
            .map(|run| run.len as usize)
            .sum::<usize>()
    );
    let current_free_pages = current_free
        .iter()
        .flat_map(|run| run.start..run.start + run.len)
        .collect::<BTreeSet<_>>();
    assert!(planned_pages.is_subset(&current_free_pages));
    assert!(!planned_pages.contains(&(fixture.older_first.start + 1)));
    assert!(planned_pages.iter().all(
        |page| *page < fixture.young.start || *page >= fixture.young.start + fixture.young.len
    ));

    fixture
        .store
        .write_control_map_validating_mark_epoch(expected_control_map, fixture.epoch.epoch)
        .unwrap();
    assert_eq!(fixture.store.control_root(), Some(expected_control_digest));
    let path = fixture.path.path().to_path_buf();
    drop(fixture.store);
    let reopened = FileStore::open(&path).unwrap();
    let reopened_epoch = mark_epoch::active_mark_epoch_from_control_map(
        &reopened.control_root_map().unwrap(),
        reopened.digest_algo,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        reopened_epoch.captured_free_consumed_through,
        expected_plan_cursor
    );
    assert_eq!(reopened.control_root(), Some(expected_control_digest));
    let expected_next = {
        let mut file = reopened.file.lock().unwrap();
        mark_epoch::captured_free_reuse_runs(
            &mut **file,
            reopened.digest_algo,
            &reopened_epoch,
            &current_free,
            current_generation,
            2,
        )
        .unwrap()
    };
    let next = {
        let mut file = reopened.file.lock().unwrap();
        mark_epoch::captured_free_reuse_runs(
            &mut **file,
            reopened.digest_algo,
            &reopened_epoch,
            &current_free,
            current_generation,
            2,
        )
        .unwrap()
    };
    assert_eq!(next.runs, expected_next.runs);
    assert_eq!(next.consumed_through, expected_next.consumed_through);
}

#[test]
fn mu17j_l_e_precommit_failure_preserves_cursor_and_roots() {
    let fixture = captured_free_selection_fixture("mu17j-l-e-precommit-failure");
    let before = {
        let inner = fixture.store.inner.lock().unwrap();
        (
            inner.generation,
            inner.control_root,
            inner.freemap,
            inner.reference_root,
            inner.metadata_bootstrap_reserve.clone(),
        )
    };
    let before_cursor = fixture.epoch.captured_free_consumed_through;
    let planned_cursor = {
        let mut inner = fixture.store.inner.lock().unwrap();
        let control_map = fixture.store.control_map_locked(&mut inner).unwrap();
        let authority = fixture
            .store
            .begin_foreground_transaction_publication(&inner, control_map)
            .unwrap();
        let mut allocator = PageAllocator::new_with_reusable_authorities(
            inner.page_count,
            inner.generation + 1,
            inner.free.clone(),
            authority.ordinary_reusable_runs.clone(),
            authority.publication_eligible_runs.clone(),
        );
        allocator
            .install_metadata_bootstrap_reserve(&inner.metadata_bootstrap_reserve)
            .unwrap();
        let mut file = fixture.store.file.lock().unwrap();
        let prepared = fixture
            .store
            .prepare_foreground_transaction_finalization(
                &mut **file,
                &inner,
                &allocator,
                &authority,
                inner.index_root,
            )
            .unwrap();
        let demand = prepared
            .control_frame
            .as_ref()
            .map_or(0, |frame| prepared_record_page_allocations(&frame.frame))
            .saturating_add(prepared.index_delta.allocation_calls())
            .saturating_add(2);
        mark_epoch::captured_free_reuse_runs(
            &mut **file,
            fixture.store.digest_algo,
            &fixture.epoch,
            &inner.free,
            inner.generation + 1,
            demand as usize,
        )
        .unwrap()
        .consumed_through
    };
    assert!(planned_cursor > before_cursor);

    let hits = Arc::new(AtomicU64::new(0));
    let injected_hits = Arc::clone(&hits);
    let _guard = install_store_publication_failure_test_injector(
        fixture.path.path().to_path_buf(),
        Arc::new(move |boundary| {
            assert_eq!(
                boundary,
                StorePublicationFailureTestBoundary::WorkflowOwnerStateCommit
            );
            injected_hits.fetch_add(1, Ordering::SeqCst);
            Err(LoomError::new(Code::Io, "injected precommit failure"))
        }),
    );
    let key = durability_facet_test_key(b"documents", "mu17j-l-e-precommit");
    let mut transaction = workflow_transaction_test(
        "mu17j-l-e-precommit",
        vec![workflow_put(
            FacetKind::Document,
            key.clone(),
            b"after",
            None,
        )],
        None,
    );
    transaction.owner_state = loom_core::WorkflowOwnerState {
        controls: vec![loom_core::WorkflowControlWrite::Put {
            key: b"mu17j-l-e/precommit".to_vec(),
            payload: b"after".to_vec(),
        }],
        ..loom_core::WorkflowOwnerState::default()
    };
    let err = fixture
        .store
        .commit_workflow_transaction(transaction)
        .unwrap_err();
    assert_eq!(err.code, Code::Io);
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert!(
        fixture
            .store
            .mutable_overlay_current_entry(&key)
            .unwrap()
            .is_none()
    );
    let after = {
        let inner = fixture.store.inner.lock().unwrap();
        (
            inner.generation,
            inner.control_root,
            inner.freemap,
            inner.reference_root,
            inner.metadata_bootstrap_reserve.clone(),
        )
    };
    assert_eq!(after, before);
    assert_eq!(
        mark_epoch::active_mark_epoch_from_control_map(
            &fixture.store.control_root_map().unwrap(),
            fixture.store.digest_algo,
        )
        .unwrap()
        .unwrap()
        .captured_free_consumed_through,
        before_cursor
    );
    let path = fixture.path.path().to_path_buf();
    drop(_guard);
    drop(fixture.store);
    let reopened = FileStore::open(path).unwrap();
    let reopened_state = {
        let inner = reopened.inner.lock().unwrap();
        (
            inner.generation,
            inner.control_root,
            inner.freemap,
            inner.reference_root,
            inner.metadata_bootstrap_reserve.clone(),
        )
    };
    assert_eq!(reopened_state, before);
    assert_eq!(
        mark_epoch::active_mark_epoch_from_control_map(
            &reopened.control_root_map().unwrap(),
            reopened.digest_algo,
        )
        .unwrap()
        .unwrap()
        .captured_free_consumed_through,
        before_cursor
    );
    assert!(
        reopened
            .mutable_overlay_current_entry(&key)
            .unwrap()
            .is_none()
    );
}

#[test]
fn mu17j_l_e_positive_reservation_persists_cursor_only_progress() {
    let fixture = captured_free_selection_fixture("mu17j-l-e-no-selection");
    {
        let mut inner = fixture.store.inner.lock().unwrap();
        inner.free = vec![fixture.young];
    }
    let expected_cursor = fixture.captured.iter().map(|run| run.len).sum::<u64>();
    let mut expected_control_map = fixture.store.control_root_map().unwrap();
    mark_epoch::advance_captured_free_consumption_in_control_map(
        &mut expected_control_map,
        &fixture.epoch,
        expected_cursor,
        fixture.store.digest_algo,
    )
    .unwrap();
    let expected_control_bytes = record_io::encode_control_map(&expected_control_map);
    let expected_control_digest = Digest::hash(fixture.store.digest_algo, &expected_control_bytes);
    let prepared = {
        let mut inner = fixture.store.inner.lock().unwrap();
        let control_map = fixture.store.control_map_locked(&mut inner).unwrap();
        let authority = fixture
            .store
            .begin_foreground_transaction_publication(&inner, control_map)
            .unwrap();
        assert!(authority.ordinary_reusable_runs.is_empty());
        assert!(authority.publication_eligible_runs.is_empty());
        let mut allocator = PageAllocator::new_with_reusable_authorities(
            inner.page_count,
            inner.generation + 1,
            inner.free.clone(),
            authority.ordinary_reusable_runs.clone(),
            authority.publication_eligible_runs.clone(),
        );
        allocator
            .install_captured_free_authority(authority.captured_free_authority.clone())
            .unwrap();
        let mut file = fixture.store.file.lock().unwrap();
        fixture
            .store
            .prepare_foreground_transaction_finalization(
                &mut **file,
                &inner,
                &allocator,
                &authority,
                inner.index_root,
            )
            .unwrap()
    };
    assert!(prepared.selected_publication_runs.is_empty());
    assert_eq!(prepared.control, Some(*expected_control_digest.bytes()));
    assert!(prepared.free_map_publication.demand().allocation_pages() > 0);
    assert!(expected_cursor > fixture.epoch.captured_free_consumed_through);
    assert_eq!(
        mark_epoch::active_mark_epoch_from_control_map(
            &fixture.store.control_root_map().unwrap(),
            fixture.store.digest_algo,
        )
        .unwrap()
        .unwrap()
        .captured_free_consumed_through,
        0
    );
}

#[test]
fn mu17j_l_f_free_map_publication_reserve_covers_exact_demand() {
    let fixture = captured_free_selection_fixture("mu17j-l-f-attribution");
    fixture.store.inner.lock().unwrap().free = captured_free_selection_current_free(&fixture);
    let _ = take_foreground_allocator_page_stats();

    let key = durability_facet_test_key(b"documents", "mu17j-l-f-attribution");
    let mut transaction = workflow_transaction_test(
        "mu17j-l-f-attribution",
        vec![workflow_put(FacetKind::Document, key, b"attributed", None)],
        None,
    );
    transaction.owner_state = loom_core::WorkflowOwnerState {
        controls: vec![loom_core::WorkflowControlWrite::Put {
            key: b"mu17j-l-f/control".to_vec(),
            payload: b"attributed".to_vec(),
        }],
        ..loom_core::WorkflowOwnerState::default()
    };
    fixture
        .store
        .commit_workflow_transaction(transaction)
        .unwrap();

    let measurements = take_foreground_allocator_page_stats();
    assert_eq!(measurements.len(), 1);
    let measured = measurements[0];
    let attributed_publication_pages = measured
        .free_map_unique_btree_nodes_touched
        .saturating_add(measured.free_map_split_pages)
        .saturating_add(measured.fixed_metadata_pages);
    assert_eq!(measured.free_map_extent_deletes, 2);
    assert_eq!(measured.free_map_extent_upserts, 3);
    assert_eq!(measured.free_map_unique_btree_nodes_touched, 1);
    assert_eq!(measured.free_map_split_pages, 0);
    assert_eq!(measured.fixed_metadata_pages, 2);
    assert_eq!(measured.publication_reserved_pages, 7);
    assert_eq!(measured.publication_reused_pages, 4);
    assert_eq!(measured.publication_unused_pages, 3);
    assert_eq!(measured.publication_reserve_exhaustions, 0);
    assert_eq!(measured.reusable_eligible_pages_left, 3);
    assert_eq!(measured.metadata_bootstrap_reused_pages, 4);
    assert_eq!(measured.metadata_bootstrap_extended_pages, 0);
    assert_eq!(measured.extended_pages, 0);
    assert_eq!(attributed_publication_pages, 6);
    assert_eq!(measured.free_map_updates, 5);
    assert_eq!(
        measured.metadata_bootstrap_reused_pages,
        measured.free_map_unique_btree_nodes_touched + measured.free_map_split_pages
    );
    assert_eq!(
        measured.publication_reused_pages + measured.metadata_bootstrap_reused_pages,
        attributed_publication_pages + 2
    );
}

#[test]
fn mu17j_l_metadata_bootstrap_free_map_pair_reopens_from_one_generation() {
    let fixture = captured_free_selection_fixture("mu17j-l-bootstrap-pair-reopen");
    fixture.store.inner.lock().unwrap().free = captured_free_selection_current_free(&fixture);
    let key = durability_facet_test_key(b"documents", "mu17j-l-bootstrap-pair");
    fixture
        .store
        .commit_workflow_transaction(workflow_transaction_test(
            "mu17j-l-bootstrap-pair",
            vec![workflow_put(
                FacetKind::Document,
                key.clone(),
                b"committed",
                None,
            )],
            None,
        ))
        .unwrap();
    let committed = {
        let inner = fixture.store.inner.lock().unwrap();
        assert_eq!(
            inner.metadata_bootstrap_reserve.owning_generation,
            inner.generation
        );
        for extent in &inner.metadata_bootstrap_reserve.extents {
            assert!(inner.free.iter().all(|run| {
                extent.start.saturating_add(extent.len) <= run.start
                    || run.start.saturating_add(run.len) <= extent.start
            }));
        }
        (
            inner.generation,
            inner.freemap,
            inner.region_table_root,
            inner.metadata_bootstrap_reserve.clone(),
        )
    };
    let path = fixture.path.path().to_path_buf();
    drop(fixture.store);
    let reopened = FileStore::open(path).unwrap();
    let reopened_pair = {
        let inner = reopened.inner.lock().unwrap();
        (
            inner.generation,
            inner.freemap,
            inner.region_table_root,
            inner.metadata_bootstrap_reserve.clone(),
        )
    };
    assert_eq!(reopened_pair, committed);
    assert_eq!(
        reopened
            .mutable_overlay_current_entry(&key)
            .unwrap()
            .unwrap()
            .payload,
        b"committed"
    );
}

fn mu17j_l_rotate_metadata_bootstrap_reserve(
    store: &FileStore,
) -> (Vec<PageId>, page::MetadataBootstrapReserve) {
    let mut inner = store.inner.lock().unwrap();
    let new_gen = inner.generation + 1;
    let roots;
    let retired;
    {
        let mut file = store.file.lock().unwrap();
        let mut alloc = PageAllocator::new(inner.page_count, new_gen, inner.free.clone());
        alloc
            .install_metadata_bootstrap_reserve(&inner.metadata_bootstrap_reserve)
            .unwrap();
        retired = alloc
            .alloc_metadata_bootstrap_pages(alloc.metadata_bootstrap_page_count())
            .unwrap();
        alloc.ensure_metadata_bootstrap_capacity().unwrap();
        roots = finish_txn(
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
                previous_mutable_overlay_generation_floor: inner.mutable_overlay_generation_floor,
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
        .unwrap();
    }
    store
        .adopt_committed_roots_locked(&mut inner, roots)
        .unwrap();
    (retired, inner.metadata_bootstrap_reserve.clone())
}

#[test]
fn mu17j_l_a_epoch_and_current_metadata_bootstrap_reserves_survive_validated_gc() {
    let tp = TempPath::new("mu17j-l-a-bootstrap-gc-safety");
    let mut store = FileStore::open(tp.path()).unwrap();
    let live = store.put(b"mu17j-l-a-live").unwrap();
    store.set_reference_root(Some(live)).unwrap();
    let mut epoch = store
        .begin_reachability_mark_epoch(Some(live), BTreeSet::new(), mu17j_h_b_completed_state(live))
        .unwrap();
    let captured_pages = epoch
        .captured_metadata_bootstrap_reserve
        .pages()
        .collect::<BTreeSet<_>>();
    let current_before_rotation = store
        .inner
        .lock()
        .unwrap()
        .metadata_bootstrap_reserve
        .clone();
    let stale_page = current_before_rotation
        .pages()
        .find(|page| captured_pages.contains(page))
        .expect("epoch capture and begin publication must retain an unused reserve page");
    {
        let inner = store.inner.lock().unwrap();
        let region_root = inner.region_table_root.unwrap();
        let mut file = store.file.lock().unwrap();
        let mut recognizable = [0u8; PAGE_SIZE as usize];
        read_exact_at(
            &mut **file,
            region_root.offset(DATA_START),
            &mut recognizable,
        )
        .unwrap();
        write_at(
            &mut **file,
            PageId(stale_page).offset(DATA_START),
            &recognizable,
        )
        .unwrap();
    }

    let (retired_pages, committed_new_reserve) = mu17j_l_rotate_metadata_bootstrap_reserve(&store);
    assert!(retired_pages.iter().any(|page| page.0 == stale_page));
    let new_reserve_pages = committed_new_reserve.pages().collect::<BTreeSet<_>>();
    assert!(captured_pages.is_disjoint(&new_reserve_pages));

    mark_epoch::reset_metadata_page_classifications_for_test();
    while !epoch.metadata_completed {
        let visited = store
            .step_reachability_metadata_mark_epoch(&mut epoch, 8, None)
            .unwrap();
        assert!(visited > 0);
        store.save_reachability_mark_epoch(&epoch).unwrap();
    }
    assert!(!mark_epoch::metadata_page_was_classified_for_test(
        stale_page
    ));
    assert_eq!(
        store
            .reachability_mark_metadata_page_state_for_test(&epoch, stale_page)
            .unwrap(),
        (true, false)
    );
    store.complete_reachability_mark_epoch(&epoch).unwrap();
    let evidence = store
        .active_reachability_mark_reclaim_evidence()
        .unwrap()
        .unwrap();
    assert_eq!(
        evidence.captured_metadata_bootstrap_reserve,
        epoch.captured_metadata_bootstrap_reserve
    );

    store
        .gc_validated_segments(GcSegmentBudget::unlimited())
        .unwrap();
    let free_after_gc = store.inner.lock().unwrap().free.clone();
    assert!(captured_pages.iter().all(|page| {
        free_after_gc
            .iter()
            .all(|run| *page < run.start || *page >= run.start.saturating_add(run.len))
    }));
    assert!(new_reserve_pages.iter().all(|page| {
        free_after_gc
            .iter()
            .all(|run| *page < run.start || *page >= run.start.saturating_add(run.len))
    }));
    let descriptor_before_reopen = store
        .inner
        .lock()
        .unwrap()
        .metadata_bootstrap_reserve
        .clone();

    drop(store);
    let reopened = FileStore::open(tp.path()).unwrap();
    let inner = reopened.inner.lock().unwrap();
    assert_eq!(inner.metadata_bootstrap_reserve, descriptor_before_reopen);
    assert!(
        inner
            .metadata_bootstrap_reserve
            .extents
            .iter()
            .all(|extent| {
                inner.free.iter().all(|run| {
                    extent.start.saturating_add(extent.len) <= run.start
                        || run.start.saturating_add(run.len) <= extent.start
                })
            })
    );
}

#[test]
fn mu17j_l_a_legacy_epoch_and_evidence_fail_closed_before_reclamation() {
    let tp = TempPath::new("mu17j-l-a-legacy-bootstrap-evidence");
    let store = FileStore::open(tp.path()).unwrap();
    let live = store.put(b"mu17j-l-a-legacy-bootstrap-live").unwrap();
    store.set_reference_root(Some(live)).unwrap();
    let mut epoch = store
        .begin_reachability_mark_epoch(Some(live), BTreeSet::new(), mu17j_h_b_completed_state(live))
        .unwrap();
    let captured_reserve = epoch.captured_metadata_bootstrap_reserve.clone();
    assert!(captured_reserve.page_count() > 0);

    while !epoch.metadata_completed {
        assert!(
            store
                .step_reachability_metadata_mark_epoch(&mut epoch, 8, None)
                .unwrap()
                > 0
        );
        store.save_reachability_mark_epoch(&epoch).unwrap();
    }
    store.complete_reachability_mark_epoch(&epoch).unwrap();
    let evidence = store
        .active_reachability_mark_reclaim_evidence()
        .unwrap()
        .unwrap();
    let legacy_epoch_bytes = mark_epoch::encode_mark_epoch_v8_for_test(&epoch);
    let legacy_evidence_bytes = mark_epoch::encode_mark_reclaim_evidence_v6_for_test(&evidence);
    let (_, rotated_reserve) = mu17j_l_rotate_metadata_bootstrap_reserve(&store);
    assert!(
        captured_reserve
            .pages()
            .collect::<BTreeSet<_>>()
            .is_disjoint(&rotated_reserve.pages().collect())
    );

    let decoded_legacy_epoch =
        mark_epoch::decode_mark_epoch_for_test(&legacy_epoch_bytes, Algo::Blake3).unwrap();
    assert_eq!(
        decoded_legacy_epoch
            .captured_metadata_bootstrap_reserve
            .page_count(),
        0
    );
    assert_eq!(
        store
            .save_reachability_mark_epoch(&decoded_legacy_epoch)
            .unwrap_err()
            .code,
        Code::CorruptObject
    );
    let decoded_legacy_evidence =
        mark_epoch::decode_mark_reclaim_evidence_for_test(&legacy_evidence_bytes, Algo::Blake3)
            .unwrap();
    assert_eq!(
        store
            .reachability_mark_metadata_reclaim_candidate_pages(
                &decoded_legacy_evidence,
                store.inner.lock().unwrap().page_count,
                u64::MAX,
            )
            .unwrap_err()
            .code,
        Code::CorruptObject
    );
    assert_eq!(
        mark_epoch::encode_mark_reclaim_evidence(&decoded_legacy_evidence)
            .unwrap_err()
            .code,
        Code::CorruptObject
    );

    store
        .control_set(
            b"maintenance/v1/reachability-mark/active",
            legacy_epoch_bytes.clone(),
        )
        .unwrap();
    store
        .control_set(
            mark_epoch::MARK_EPOCH_RECLAIM_EVIDENCE_KEY,
            legacy_evidence_bytes.clone(),
        )
        .unwrap();
    drop(store);

    let mut reopened = FileStore::open(tp.path()).unwrap();
    assert!(reopened.active_reachability_mark_epoch().unwrap().is_none());
    assert!(
        reopened
            .active_reachability_mark_reclaim_evidence()
            .unwrap()
            .is_none()
    );
    let before = {
        let inner = reopened.inner.lock().unwrap();
        (
            inner.generation,
            inner.region_table_root,
            inner.maintenance_root,
            inner.free.clone(),
        )
    };
    assert_eq!(
        reopened
            .gc_validated_segments(GcSegmentBudget::unlimited())
            .unwrap_err()
            .code,
        Code::NotFound
    );
    let after = {
        let inner = reopened.inner.lock().unwrap();
        (
            inner.generation,
            inner.region_table_root,
            inner.maintenance_root,
            inner.free.clone(),
        )
    };
    assert_eq!(after, before);
    let legacy_control = reopened.control_map().unwrap();
    assert_eq!(
        legacy_control
            .get(b"maintenance/v1/reachability-mark/active".as_slice())
            .unwrap(),
        &legacy_epoch_bytes
    );
    assert_eq!(
        legacy_control
            .get(mark_epoch::MARK_EPOCH_RECLAIM_EVIDENCE_KEY)
            .unwrap(),
        &legacy_evidence_bytes
    );

    let fresh_tp = TempPath::new("mu17j-l-a-current-bootstrap-evidence");
    let mut fresh_store = FileStore::open(fresh_tp.path()).unwrap();
    let fresh_live = fresh_store
        .put(b"mu17j-l-a-current-bootstrap-live")
        .unwrap();
    fresh_store.set_reference_root(Some(fresh_live)).unwrap();
    let mut fresh = fresh_store
        .begin_reachability_mark_epoch(
            Some(fresh_live),
            BTreeSet::new(),
            mu17j_h_b_completed_state(fresh_live),
        )
        .unwrap();
    assert!(fresh.captured_metadata_bootstrap_reserve.page_count() > 0);
    assert!(
        fresh_store
            .active_reachability_mark_reclaim_evidence()
            .unwrap()
            .is_none()
    );
    while !fresh.metadata_completed {
        assert!(
            fresh_store
                .step_reachability_metadata_mark_epoch(&mut fresh, 8, None)
                .unwrap()
                > 0
        );
        fresh_store.save_reachability_mark_epoch(&fresh).unwrap();
    }
    fresh_store
        .complete_reachability_mark_epoch(&fresh)
        .unwrap();
    fresh_store
        .gc_validated_segments(GcSegmentBudget::unlimited())
        .unwrap();
}

#[test]
fn mu17j_l_metadata_bootstrap_generation_ordinary_open_uses_recovered_generation() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let data = blob(b"metadata-bootstrap-generation-open");
    let digest = store.put(&data).unwrap();
    let committed = {
        let inner = store.inner.lock().unwrap();
        assert_eq!(
            inner.metadata_bootstrap_reserve.owning_generation,
            inner.generation
        );
        (
            inner.generation,
            inner.region_table_root,
            inner.metadata_bootstrap_reserve.clone(),
        )
    };
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let reopened_state = {
        let inner = reopened.inner.lock().unwrap();
        (
            inner.generation,
            inner.region_table_root,
            inner.metadata_bootstrap_reserve.clone(),
        )
    };
    assert_eq!(reopened_state, committed);
    assert_eq!(reopened.get(&digest).unwrap().unwrap(), data);
}

#[test]
fn mu17j_l_metadata_bootstrap_generation_mismatch_fails_before_open_state() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let data = blob(b"metadata-bootstrap-generation-mismatch");
    let digest = store.put(&data).unwrap();
    let (generation, page_count, region_table_root) = {
        let inner = store.inner.lock().unwrap();
        (
            inner.generation,
            inner.page_count,
            inner.region_table_root.unwrap(),
        )
    };
    drop(store);
    let original = shared.bytes();
    shared.mutate_bytes(|bytes| {
        let offset = (DATA_START + region_table_root.0 * PAGE_SIZE) as usize;
        let end = offset + PAGE_SIZE as usize;
        let mut region = CanonicalRegionTable::decode(&bytes[offset..end]).unwrap();
        region.metadata_bootstrap_reserve.owning_generation = generation + 1;
        bytes[offset..end].copy_from_slice(&region.encode(page_count).unwrap());
    });
    let mismatched = shared.bytes();

    let error = FileStore::with_backing(Box::new(shared.clone()), false).unwrap_err();
    assert_eq!(error.code, Code::CorruptObject);
    assert_eq!(
        error.message,
        "loom-store: metadata bootstrap reserve owning generation mismatch"
    );
    assert_eq!(shared.bytes(), mismatched);

    shared.mutate_bytes(|bytes| *bytes = original);
    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    assert_eq!(reopened.get(&digest).unwrap().unwrap(), data);
    let inner = reopened.inner.lock().unwrap();
    assert_eq!(inner.generation, generation);
    assert_eq!(
        inner.metadata_bootstrap_reserve.owning_generation,
        generation
    );
}

#[test]
fn mu17j_l_metadata_bootstrap_generation_failed_commit_reopens_prior_pair() {
    let shared = SharedMem::default();
    let baseline = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let baseline_data = blob(b"metadata-bootstrap-generation-baseline");
    let digest = baseline.put(&baseline_data).unwrap();
    let before = {
        let inner = baseline.inner.lock().unwrap();
        (
            inner.generation,
            inner.region_table_root,
            inner.freemap,
            inner.metadata_bootstrap_reserve.clone(),
        )
    };
    drop(baseline);

    let failing =
        FileStore::with_backing(Box::new(FailNthFsyncMem::new(shared.clone(), 2)), true).unwrap();
    let error = failing.put(&blob(b"metadata-bootstrap-generation-failed"));
    assert!(error.is_err());
    let failed_live = {
        let inner = failing.inner.lock().unwrap();
        (
            inner.generation,
            inner.region_table_root,
            inner.freemap,
            inner.metadata_bootstrap_reserve.clone(),
        )
    };
    assert_eq!(failed_live, before);
    drop(failing);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    let reopened_state = {
        let inner = reopened.inner.lock().unwrap();
        (
            inner.generation,
            inner.region_table_root,
            inner.freemap,
            inner.metadata_bootstrap_reserve.clone(),
        )
    };
    assert_eq!(reopened_state, before);
    assert_eq!(reopened.get(&digest).unwrap().unwrap(), baseline_data);
    assert_eq!(reopened_state.3.owning_generation, reopened_state.0);
}

#[test]
fn mu17j_l_fresh_store_bootstrap_reserve_stays_bounded() {
    let path = TempPath::new("mu17j-l-fresh-bootstrap");
    let store = FileStore::open(path.path()).unwrap();
    let before = std::fs::metadata(path.path()).unwrap().len();
    store.put(&blob(b"fresh bootstrap admission")).unwrap();
    let after = std::fs::metadata(path.path()).unwrap().len();
    let growth = after.saturating_sub(before);
    eprintln!("fresh-store bootstrap growth: before={before} after={after} delta={growth}");
    assert!(
        growth < 1024 * 1024,
        "fresh-store growth was {growth} bytes"
    );
    let inner = store.inner.lock().unwrap();
    assert!(
        inner.metadata_bootstrap_reserve.page_count() <= pagemap::METADATA_BOOTSTRAP_TARGET_PAGES
    );
}

#[test]
fn mu17j_l_foreground_finalization_has_no_fixed_point_retry() {
    let source = include_str!("lib.rs");
    let start = source
        .find("fn prepare_foreground_transaction_finalization(")
        .unwrap();
    let end = source[start..]
        .find("fn apply_foreground_transaction_finalization(")
        .map(|offset| start + offset)
        .unwrap();
    let body = &source[start..end];
    assert!(!body.contains("MAX_FIXED_POINT_STEPS"));
    assert!(!body.contains("for _ in"));
    assert!(!body.contains("did not converge"));
}

fn mu17j_l_captured_metadata_snapshot(
    store: &FileStore,
    epoch: &ReachabilityMarkEpoch,
) -> BTreeMap<u64, [u8; PAGE_SIZE as usize]> {
    let page_count = store.inner.lock().unwrap().page_count;
    let mut file = store.file.lock().unwrap();
    let mut pages = BTreeSet::new();
    for page in &epoch.captured_metadata_roots {
        let mut bytes = [0u8; PAGE_SIZE as usize];
        read_exact_at(&mut **file, PageId(*page).offset(DATA_START), &mut bytes).unwrap();
        if pagebtree::looks_like_node_page(&bytes) {
            pages.extend(
                pagebtree::collect_pages(&mut **file, DATA_START, PageId(*page), page_count)
                    .unwrap()
                    .into_iter()
                    .map(|page| page.0),
            );
        } else {
            pages.insert(*page);
        }
    }
    for root in &epoch.captured_metadata_value_roots {
        for (_, loc) in
            pagebtree::load_all(&mut **file, DATA_START, PageId(*root), page_count).unwrap()
        {
            pages
                .extend(record_io::blob_pages(&mut **file, loc.global_page(), page_count).unwrap());
        }
    }
    pages
        .into_iter()
        .map(|page| {
            let mut bytes = [0u8; PAGE_SIZE as usize];
            read_exact_at(&mut **file, PageId(page).offset(DATA_START), &mut bytes).unwrap();
            (page, bytes)
        })
        .collect()
}

#[test]
fn mu17j_l_foreground_reuse_preserves_the_captured_metadata_snapshot() {
    let tp = TempPath::new("mu17j-l-captured-metadata-snapshot");
    let store = FileStore::open(tp.path()).unwrap();
    for operation in 0..REUSE_SAFE_WINDOW {
        store
            .put(format!("mu17j-l-aging-{operation}").as_bytes())
            .unwrap();
    }
    mu17j_h_b_install_free_run_cardinality(&store, 64);
    let key = durability_facet_test_key(b"documents", "mu17j-l-captured-metadata");
    store
        .put_mutable_overlay_value(key.clone(), b"before".to_vec())
        .unwrap();
    let live = store.put(b"mu17j-l-captured-metadata-live").unwrap();
    store.set_reference_root(Some(live)).unwrap();
    let epoch = store
        .begin_reachability_mark_epoch(Some(live), BTreeSet::new(), mu17j_h_b_completed_state(live))
        .unwrap();
    let before = mu17j_l_captured_metadata_snapshot(&store, &epoch);
    for operation in 0..5 {
        store
            .put_mutable_overlay_value(key.clone(), format!("after-{operation}").into_bytes())
            .unwrap();
    }
    let after = mu17j_l_captured_metadata_snapshot(&store, &epoch);
    let changed = before
        .iter()
        .filter_map(|(page, bytes)| (after.get(page) != Some(bytes)).then_some(*page))
        .collect::<Vec<_>>();
    assert!(
        changed.is_empty(),
        "captured metadata pages changed: {changed:?}"
    );
}

#[test]
fn mu17j_h_b_epoch_completion_consumes_persisted_metadata_without_full_scan() {
    let tp = TempPath::new("mu17j-h-b-completion-no-full-scan");
    let store = FileStore::open(tp.path()).unwrap();
    let live = store.put(b"mu17j-h-b-live-root").unwrap();
    store.set_reference_root(Some(live)).unwrap();
    mu17j_h_b_install_free_run_cardinality(&store, 32);
    let mut epoch = store
        .begin_reachability_mark_epoch(Some(live), BTreeSet::new(), mu17j_h_b_completed_state(live))
        .unwrap();
    while !epoch.metadata_completed {
        mu17j_h_b_advance_metadata_epoch(&store, &mut epoch, 8);
    }
    mark_epoch::reset_metadata_page_classifications_for_test();
    store.complete_reachability_mark_epoch(&epoch).unwrap();
    assert_eq!(mark_epoch::metadata_page_classifications_for_test(), 0);
    let evidence = store
        .active_reachability_mark_reclaim_evidence()
        .unwrap()
        .unwrap();
    assert_eq!(evidence.epoch, epoch.epoch);
}

#[test]
fn mu17j_h_b_superseded_extent_tree_pages_use_delayed_reclaim_and_reader_lease() {
    let tp = TempPath::new("mu17j-h-b-reader-lease-reuse");
    let mut store = FileStore::open(tp.path()).unwrap();
    let live = store.put(b"mu17j-h-b-live-root").unwrap();
    store.set_reference_root(Some(live)).unwrap();
    mu17j_h_b_install_free_run_cardinality(&store, 64);
    let key = durability_facet_test_key(b"documents", "mu17j-h-b-reader-lease");
    store
        .put_mutable_overlay_value(key, b"before".to_vec())
        .unwrap();
    FileStore::open_read(tp.path()).unwrap();
    let before_pages = mu17j_h_b_freemap_pages(&store);
    store
        .put_mutable_overlay_value(
            durability_facet_test_key(b"documents", "mu17j-h-b-reader-lease-update"),
            b"after".to_vec(),
        )
        .unwrap();
    FileStore::open_read(tp.path()).unwrap();
    let after_pages = mu17j_h_b_freemap_pages(&store);
    let superseded_page = before_pages
        .difference(&after_pages)
        .next()
        .copied()
        .expect("expected at least one superseded extent-tree page");
    let free = store.inner.lock().unwrap().free.clone();
    assert!(
        !free
            .iter()
            .any(|run| superseded_page >= run.start && superseded_page < run.start + run.len),
        "superseded extent-tree page was immediately reusable through the same-generation free map"
    );
    eprintln!("mu17j_h_b superseded_extent_tree_page={superseded_page}");

    let mut epoch = store
        .begin_reachability_mark_epoch(Some(live), BTreeSet::new(), mu17j_h_b_completed_state(live))
        .unwrap();
    while !epoch.metadata_completed {
        mu17j_h_b_advance_metadata_epoch(&store, &mut epoch, 8);
    }
    FileStore::open_read(tp.path()).unwrap();
    store.complete_reachability_mark_epoch(&epoch).unwrap();
    FileStore::open_read(tp.path()).unwrap();
    let evidence = store
        .active_reachability_mark_reclaim_evidence()
        .unwrap()
        .unwrap();
    eprintln!(
        "mu17j_h_b evidence_pages={} evidence_contains_superseded={}",
        evidence.unreachable_pre_snapshot_pages.len(),
        evidence
            .unreachable_pre_snapshot_pages
            .contains(&superseded_page)
    );
    let stats = store
        .gc_validated_segments(GcSegmentBudget {
            max_segments: u64::MAX,
            max_pages: u64::MAX,
        })
        .unwrap();
    eprintln!("mu17j_h_b gc_pages_freed={}", stats.pages_freed);
    let (reclaimed_free, horizon) = {
        let inner = store.inner.lock().unwrap();
        (inner.free.clone(), inner.minimum_recoverable_generation)
    };
    eprintln!(
        "mu17j_h_b reclaimed_runs_near={:?}",
        reclaimed_free
            .iter()
            .filter(|run| {
                let end = run.start.saturating_add(run.len);
                superseded_page.saturating_sub(8) <= end
                    && run.start <= superseded_page.saturating_add(8)
            })
            .collect::<Vec<_>>()
    );
    assert!(
        reclaimed_free
            .iter()
            .any(|run| superseded_page >= run.start && superseded_page < run.start + run.len),
        "validated GC did not expose superseded extent-tree page through the delayed authority"
    );

    let reader = FileStore::open_read(tp.path()).unwrap();
    let (blocked_reusable, _blocked_lease) = store
        .transaction_reusable_free(&reclaimed_free, None, horizon)
        .unwrap();
    assert!(blocked_reusable.is_empty());
    drop(reader);
    let (allowed_reusable, _allowed_lease) = store
        .transaction_reusable_free(&reclaimed_free, None, horizon)
        .unwrap();
    assert!(
        allowed_reusable
            .iter()
            .any(|run| superseded_page >= run.start && superseded_page < run.start + run.len),
        "released reader lease did not permit reuse of superseded extent-tree page"
    );
}

fn mu17j_h_b_freemap_pages(store: &FileStore) -> BTreeSet<u64> {
    let inner = store.inner.lock().unwrap();
    let Some((root, _)) = inner.freemap else {
        return BTreeSet::new();
    };
    let page_count = inner.page_count;
    drop(inner);
    let mut file = store.file.lock().unwrap();
    let mut pages: BTreeSet<u64> =
        pagebtree::collect_pages(&mut **file, DATA_START, root, page_count)
            .unwrap()
            .into_iter()
            .map(|page| page.0)
            .collect();
    for (_, loc) in pagebtree::load_all(&mut **file, DATA_START, root, page_count).unwrap() {
        for page in
            record_io::chunked_blob_pages(&mut **file, loc.global_page(), page_count).unwrap()
        {
            pages.insert(page);
        }
    }
    pages
}

#[test]
fn document_write_commits_declared_index_and_current_record_before_reopen() {
    let tp = TempPath::new("document-index-workflow");
    let store = FileStore::open(tp.path()).unwrap();
    let mut loom = Loom::new(store);
    let workspace = loom
        .registry_mut()
        .create(FacetKind::Document, None, WorkspaceId::from_bytes([91; 16]))
        .unwrap();
    document::doc_put(
        &mut loom,
        workspace,
        "people",
        "ann",
        br#"{"city":"Paris"}"#.to_vec(),
    )
    .unwrap();
    document::doc_create_index(
        &mut loom,
        workspace,
        "people",
        document::DocumentIndexDef::new(
            "by_city",
            document::DocumentFieldPath::dotted("city").unwrap(),
            false,
        )
        .unwrap(),
    )
    .unwrap();
    document::doc_put(
        &mut loom,
        workspace,
        "people",
        "ann",
        br#"{"city":"Rome"}"#.to_vec(),
    )
    .unwrap();
    drop(loom);

    let reopened = open_loom_unlocked(tp.path(), None).unwrap();
    assert_eq!(
        document::doc_get(&reopened, workspace, "people", "ann")
            .unwrap()
            .as_deref(),
        Some(br#"{"city":"Rome"}"#.as_slice())
    );
    assert_eq!(
        document::doc_find(
            &reopened,
            workspace,
            "people",
            "by_city",
            &loom_core::tabular::Value::Text("Rome".to_string()),
        )
        .unwrap(),
        vec!["ann".to_string()]
    );
    assert!(
        document::doc_find(
            &reopened,
            workspace,
            "people",
            "by_city",
            &loom_core::tabular::Value::Text("Paris".to_string()),
        )
        .unwrap()
        .is_empty()
    );
}

#[test]
fn workflow_transaction_aborts_all_writes_on_stale_compare_token() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let first_key = durability_facet_test_key(b"documents", "workflow-conflict-first");
    let second_key = durability_facet_test_key(b"documents", "workflow-conflict-second");
    let stale = store
        .put_mutable_overlay_value(second_key.clone(), b"original".to_vec())
        .unwrap();
    store
        .put_mutable_overlay_value(second_key.clone(), b"newer".to_vec())
        .unwrap();
    let error = store
        .commit_workflow_transaction(workflow_transaction_test(
            "workflow-conflict",
            vec![
                workflow_put(FacetKind::Document, first_key.clone(), b"first", None),
                workflow_put(FacetKind::Document, second_key, b"second", Some(stale)),
            ],
            None,
        ))
        .unwrap_err();

    assert_eq!(error.code, Code::Conflict);
    assert_eq!(
        store
            .mutable_overlay_snapshot()
            .unwrap()
            .read_composite(&first_key, |_| Ok(None))
            .unwrap(),
        None
    );
}

#[test]
fn workflow_planning_snapshot_binds_reads_tokens_and_generation() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let key = durability_facet_test_key(b"documents", "planning-coherence");
    store
        .put_mutable_overlay_value(key.clone(), b"first".to_vec())
        .unwrap();
    let snapshot = WorkflowPlanningSnapshot::open(&store, Some("planning-coherence")).unwrap();
    let planned_generation = snapshot.expected_generation();
    let planned_token = snapshot.owner_token(&key).unwrap().unwrap();

    store
        .put_mutable_overlay_value(key.clone(), b"concurrent".to_vec())
        .unwrap();

    assert_eq!(
        snapshot
            .read_composite(&key, |_, _| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"first"[..])
    );
    assert_eq!(
        snapshot.owner_token(&key).unwrap(),
        Some(planned_token.clone())
    );
    assert_eq!(snapshot.expected_generation(), planned_generation);

    let mut stale = workflow_transaction_test(
        "planning-coherence",
        vec![workflow_put(
            FacetKind::Document,
            key.clone(),
            b"stale",
            Some(planned_token),
        )],
        None,
    );
    stale.expected_generation = Some(planned_generation);
    let error = store.commit_workflow_transaction(stale).unwrap_err();

    assert_eq!(error.code, Code::Conflict);
    assert_eq!(
        store
            .mutable_overlay_snapshot()
            .unwrap()
            .read_composite(&key, |_| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"concurrent"[..])
    );
}

#[test]
fn concurrent_workflow_plans_publish_only_one_generation() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let first_key = durability_facet_test_key(b"documents", "planning-first");
    let second_key = durability_facet_test_key(b"documents", "planning-second");
    let first_snapshot = WorkflowPlanningSnapshot::open(&store, Some("planning-first")).unwrap();
    let second_snapshot = WorkflowPlanningSnapshot::open(&store, Some("planning-second")).unwrap();
    assert_eq!(
        first_snapshot.expected_generation(),
        second_snapshot.expected_generation()
    );

    let mut first = workflow_transaction_test(
        "planning-first",
        vec![workflow_put(
            FacetKind::Document,
            first_key.clone(),
            b"first",
            None,
        )],
        None,
    );
    first.expected_generation = Some(first_snapshot.expected_generation());
    store.commit_workflow_transaction(first).unwrap();

    let mut second = workflow_transaction_test(
        "planning-second",
        vec![workflow_put(
            FacetKind::Document,
            second_key.clone(),
            b"second",
            None,
        )],
        None,
    );
    second.expected_generation = Some(second_snapshot.expected_generation());
    let error = store.commit_workflow_transaction(second).unwrap_err();

    assert_eq!(error.code, Code::Conflict);
    let current = store.mutable_overlay_snapshot().unwrap();
    assert_eq!(
        current
            .read_composite(&first_key, |_| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"first"[..])
    );
    assert_eq!(
        current.read_composite(&second_key, |_| Ok(None)).unwrap(),
        None
    );
}

#[test]
fn workflow_transaction_does_not_write_secondary_index_when_current_compare_fails() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let first_key = durability_facet_test_key(b"documents", "workflow-index-conflict-first");
    let second_key = durability_facet_test_key(b"documents", "workflow-index-conflict-second");
    let index_key = durability_facet_test_key(b"tickets", "workflow-index-conflict-status");
    let stale = store
        .put_mutable_overlay_value(second_key.clone(), b"original".to_vec())
        .unwrap();
    store
        .put_mutable_overlay_value(second_key.clone(), b"newer".to_vec())
        .unwrap();
    let error = store
        .commit_workflow_transaction(workflow_transaction_test(
            "workflow-index-conflict",
            vec![
                workflow_put_with_secondary_index(
                    first_key.clone(),
                    b"first",
                    index_key.clone(),
                    b"first-index",
                ),
                workflow_put(FacetKind::Document, second_key, b"second", Some(stale)),
            ],
            None,
        ))
        .unwrap_err();

    assert_eq!(error.code, Code::Conflict);
    assert_eq!(
        store
            .mutable_overlay_snapshot()
            .unwrap()
            .read_composite(&first_key, |_| Ok(None))
            .unwrap(),
        None
    );
    assert_eq!(
        store
            .mutable_overlay_secondary_index_value(&index_key)
            .unwrap(),
        None
    );
}

#[test]
fn workflow_transaction_rejects_unimplemented_separate_boundary() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let key = durability_facet_test_key(b"documents", "workflow-separate");
    let mut txn = workflow_transaction_test(
        "workflow-separate",
        vec![workflow_put(FacetKind::Document, key, b"value", None)],
        None,
    );
    txn.boundary = AtomicityBoundary::Separate;
    let error = store.commit_workflow_transaction(txn).unwrap_err();

    assert_eq!(error.code, Code::Unsupported);
}

#[test]
fn workflow_transaction_rejects_idempotent_ephemeral_boundary() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let key = durability_facet_test_key(b"search", "workflow-ephemeral-idempotent");
    let mut txn = workflow_transaction_test(
        "workflow-ephemeral-idempotent",
        vec![workflow_put(FacetKind::Search, key, b"value", None)],
        Some(b"ephemeral-workflow-retry"),
    );
    txn.durability = OverlayDurabilityPolicy::Ephemeral;
    let error = store.commit_workflow_transaction(txn).unwrap_err();

    assert_eq!(error.code, Code::InvalidArgument);
}

#[test]
fn workflow_transaction_idempotency_digest_includes_side_effect_intents() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let key = durability_facet_test_key(b"documents", "workflow-side-effects");
    let first = workflow_transaction_test(
        "workflow-side-effects",
        vec![workflow_put_with_side_effect(
            key.clone(),
            b"value",
            "operation-a",
        )],
        Some(b"side-effect-retry"),
    );
    let second = workflow_transaction_test(
        "workflow-side-effects",
        vec![workflow_put_with_side_effect(key, b"value", "operation-b")],
        Some(b"side-effect-retry"),
    );

    store.commit_workflow_transaction(first).unwrap();
    let error = store.commit_workflow_transaction(second).unwrap_err();

    assert_eq!(error.code, Code::Conflict);
}

#[test]
fn workflow_transaction_idempotency_digest_includes_secondary_indexes() {
    let store = FileStore::with_backing(Box::new(SharedMem::default()), true).unwrap();
    let key = durability_facet_test_key(b"documents", "workflow-index-idempotency");
    let index_key = durability_facet_test_key(b"tickets", "workflow-index-idempotency-status");
    let first = workflow_transaction_test(
        "workflow-index-idempotency",
        vec![workflow_put_with_secondary_index(
            key.clone(),
            b"value",
            index_key.clone(),
            b"open",
        )],
        Some(b"index-retry"),
    );
    let second = workflow_transaction_test(
        "workflow-index-idempotency",
        vec![workflow_put_with_secondary_index(
            key, b"value", index_key, b"closed",
        )],
        Some(b"index-retry"),
    );

    store.commit_workflow_transaction(first).unwrap();
    let error = store.commit_workflow_transaction(second).unwrap_err();

    assert_eq!(error.code, Code::Conflict);
}

#[test]
fn workflow_transaction_facet_policy_can_strengthen_ephemeral_transaction_default() {
    let shared = SharedMem::default();
    let key = durability_facet_test_key(b"documents", "workflow-policy-document");
    {
        let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
        let mut policy = store.store_policy().unwrap();
        policy
            .set_default_durability(StoreDurabilityPolicy::Ephemeral)
            .unwrap();
        policy
            .set_facet_durability(FacetKind::Document, Some(StoreDurabilityPolicy::Normal))
            .unwrap();
        store
            .save_store_policy_audited(policy, None, "store.policy.set", None)
            .unwrap();
        let mut txn = workflow_transaction_test(
            "workflow-policy-document",
            vec![workflow_put(
                FacetKind::Document,
                key.clone(),
                b"document-current",
                None,
            )],
            None,
        );
        txn.durability = StoreDurabilityPolicy::Ephemeral;

        store.commit_workflow_transaction(txn).unwrap();
    }

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();

    assert_eq!(
        reopened
            .mutable_overlay_snapshot()
            .unwrap()
            .read_composite(&key, |_| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"document-current"[..])
    );
}

#[test]
fn workflow_transaction_ephemeral_policy_acknowledges_without_persisting() {
    let shared = SharedMem::default();
    let key = durability_facet_test_key(b"search", "workflow-policy-search");
    {
        let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
        let mut policy = store.store_policy().unwrap();
        policy
            .set_default_durability(StoreDurabilityPolicy::Ephemeral)
            .unwrap();
        store
            .save_store_policy_audited(policy, None, "store.policy.set", None)
            .unwrap();
        let mut txn = workflow_transaction_test(
            "workflow-policy-search",
            vec![workflow_put(
                FacetKind::Search,
                key.clone(),
                b"search-current",
                None,
            )],
            None,
        );
        txn.durability = StoreDurabilityPolicy::Ephemeral;

        store.commit_workflow_transaction(txn).unwrap();
        assert_eq!(
            store
                .mutable_overlay_snapshot()
                .unwrap()
                .read_composite(&key, |_| Ok(None))
                .unwrap()
                .as_deref(),
            Some(&b"search-current"[..])
        );
    }

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();

    assert_eq!(
        reopened
            .mutable_overlay_snapshot()
            .unwrap()
            .read_composite(&key, |_| Ok(None))
            .unwrap(),
        None
    );
}

#[test]
fn workflow_transaction_default_can_strengthen_ephemeral_facet_policy() {
    let shared = SharedMem::default();
    let key = durability_facet_test_key(b"search", "workflow-policy-ephemeral-mixed");
    {
        let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
        let mut policy = store.store_policy().unwrap();
        policy
            .set_facet_durability(FacetKind::Search, Some(StoreDurabilityPolicy::Ephemeral))
            .unwrap();
        store
            .save_store_policy_audited(policy, None, "store.policy.set", None)
            .unwrap();
        let txn = workflow_transaction_test(
            "workflow-policy-ephemeral-mixed",
            vec![workflow_put(
                FacetKind::Search,
                key.clone(),
                b"search-current",
                None,
            )],
            None,
        );

        store.commit_workflow_transaction(txn).unwrap();
    }

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();

    assert_eq!(
        reopened
            .mutable_overlay_snapshot()
            .unwrap()
            .read_composite(&key, |_| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"search-current"[..])
    );
}

fn write_current_record_with_default_durability(
    durability: StoreDurabilityPolicy,
    name: &str,
) -> (Option<Vec<u8>>, StorePolicy) {
    let tp = TempPath::new(name);
    let key = durability_test_key(name);
    {
        let store = FileStore::open(tp.path()).unwrap();
        let mut policy = store.store_policy().unwrap();
        policy.set_default_durability(durability).unwrap();
        store
            .save_store_policy_audited(policy, None, "store.policy.set", None)
            .unwrap();
        store
            .put_mutable_overlay_value(key.clone(), format!("{name}-acknowledged").into_bytes())
            .unwrap();
    }
    let reopened = FileStore::open(tp.path()).unwrap();
    let policy = reopened.store_policy().unwrap();
    let read = reopened
        .mutable_overlay_snapshot()
        .unwrap()
        .read_composite(&key, |_| Ok(None))
        .unwrap();

    (read, policy)
}

#[test]
fn facet_durability_override_takes_precedence_over_ephemeral_store_default() {
    let tp = TempPath::new("facet-durability-precedence-normal");
    let key = durability_facet_test_key(b"documents", "document-normal-override");
    {
        let store = FileStore::open(tp.path()).unwrap();
        let mut policy = store.store_policy().unwrap();
        policy
            .set_default_durability(StoreDurabilityPolicy::Ephemeral)
            .unwrap();
        policy
            .set_facet_durability(FacetKind::Document, Some(StoreDurabilityPolicy::Normal))
            .unwrap();
        store
            .save_store_policy_audited(policy, None, "store.policy.set", None)
            .unwrap();
        store
            .put_mutable_overlay_value(key.clone(), b"document-current".to_vec())
            .unwrap();
    }

    let reopened = FileStore::open(tp.path()).unwrap();

    assert_eq!(
        reopened
            .mutable_overlay_snapshot()
            .unwrap()
            .read_composite(&key, |_| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"document-current"[..])
    );
}

#[test]
fn facet_durability_override_can_make_one_hot_facet_ephemeral() {
    let tp = TempPath::new("facet-durability-precedence-ephemeral");
    let document_key = durability_facet_test_key(b"documents", "document-default-normal");
    let search_key = durability_facet_test_key(b"search", "search-ephemeral-override");
    {
        let store = FileStore::open(tp.path()).unwrap();
        let mut policy = store.store_policy().unwrap();
        policy
            .set_default_durability(StoreDurabilityPolicy::Normal)
            .unwrap();
        policy
            .set_facet_durability(FacetKind::Search, Some(StoreDurabilityPolicy::Ephemeral))
            .unwrap();
        store
            .save_store_policy_audited(policy, None, "store.policy.set", None)
            .unwrap();
        store
            .put_mutable_overlay_value(document_key.clone(), b"document-current".to_vec())
            .unwrap();
        store
            .put_mutable_overlay_value(search_key.clone(), b"search-current".to_vec())
            .unwrap();
    }

    let reopened = FileStore::open(tp.path()).unwrap();
    let snapshot = reopened.mutable_overlay_snapshot().unwrap();

    assert_eq!(
        snapshot
            .read_composite(&document_key, |_| Ok(None))
            .unwrap()
            .as_deref(),
        Some(&b"document-current"[..])
    );
    assert_eq!(
        snapshot.read_composite(&search_key, |_| Ok(None)).unwrap(),
        None
    );
}

#[test]
fn strict_durability_acknowledged_current_record_survives_reopen() {
    let (read, policy) =
        write_current_record_with_default_durability(StoreDurabilityPolicy::Strict, "strict");

    assert_eq!(policy.default_durability, StoreDurabilityPolicy::Strict);
    assert_eq!(read.as_deref(), Some(&b"strict-acknowledged"[..]));
}

#[test]
fn normal_durability_contract_fixture_uses_configured_policy() {
    let (read, policy) =
        write_current_record_with_default_durability(StoreDurabilityPolicy::Normal, "normal");

    assert_eq!(policy.default_durability, StoreDurabilityPolicy::Normal);
    assert_eq!(read.as_deref(), Some(&b"normal-acknowledged"[..]));
}

#[test]
fn relaxed_durability_contract_fixture_uses_configured_policy() {
    let (read, policy) =
        write_current_record_with_default_durability(StoreDurabilityPolicy::Relaxed, "relaxed");

    assert_eq!(policy.default_durability, StoreDurabilityPolicy::Relaxed);
    assert_eq!(read.as_deref(), Some(&b"relaxed-acknowledged"[..]));
}

#[test]
fn ephemeral_durability_acknowledged_current_record_does_not_survive_reopen() {
    let (read, policy) =
        write_current_record_with_default_durability(StoreDurabilityPolicy::Ephemeral, "ephemeral");

    assert_eq!(policy.default_durability, StoreDurabilityPolicy::Ephemeral);
    assert_eq!(read, None);
}

#[test]
fn ephemeral_durability_idempotent_current_record_does_not_survive_reopen() {
    let tp = TempPath::new("ephemeral-idempotent");
    let key = durability_test_key("ephemeral-idempotent");
    {
        let store = FileStore::open(tp.path()).unwrap();
        let mut policy = store.store_policy().unwrap();
        policy
            .set_default_durability(StoreDurabilityPolicy::Ephemeral)
            .unwrap();
        store
            .save_store_policy_audited(policy, None, "store.policy.set", None)
            .unwrap();
        store
            .put_mutable_overlay_value_idempotent(
                key.clone(),
                b"ephemeral-idempotent-acknowledged".to_vec(),
                "ephemeral-idempotent",
            )
            .unwrap();
        assert_eq!(
            store
                .mutable_overlay_snapshot()
                .unwrap()
                .read_composite(&key, |_| Ok(None))
                .unwrap()
                .as_deref(),
            Some(&b"ephemeral-idempotent-acknowledged"[..])
        );
    }
    let reopened = FileStore::open(tp.path()).unwrap();
    let read = reopened
        .mutable_overlay_snapshot()
        .unwrap()
        .read_composite(&key, |_| Ok(None))
        .unwrap();

    assert_eq!(read, None);
    assert_eq!(
        reopened.mutable_overlay_durable_owner_token(&key).unwrap(),
        None
    );
}

#[test]
fn ephemeral_durability_tombstone_does_not_replace_durable_current_record_on_reopen() {
    let tp = TempPath::new("ephemeral-tombstone");
    let key = durability_test_key("ephemeral-tombstone");
    let original_token = {
        let store = FileStore::open(tp.path()).unwrap();
        store
            .put_mutable_overlay_value(key.clone(), b"durable-before-ephemeral".to_vec())
            .unwrap()
    };
    {
        let store = FileStore::open(tp.path()).unwrap();
        let mut policy = store.store_policy().unwrap();
        policy
            .set_default_durability(StoreDurabilityPolicy::Ephemeral)
            .unwrap();
        store
            .save_store_policy_audited(policy, None, "store.policy.set", None)
            .unwrap();
        store.put_mutable_overlay_tombstone(key.clone()).unwrap();

        assert_eq!(
            store
                .mutable_overlay_snapshot()
                .unwrap()
                .read_composite(&key, |_| Ok(None))
                .unwrap(),
            None
        );
        assert_eq!(
            store
                .mutable_overlay_durable_owner_token(&key)
                .unwrap()
                .as_ref()
                .map(|token| token.as_bytes()),
            Some(original_token.as_bytes())
        );
    }
    let reopened = FileStore::open(tp.path()).unwrap();
    let read = reopened
        .mutable_overlay_snapshot()
        .unwrap()
        .read_composite(&key, |_| Ok(None))
        .unwrap();

    assert_eq!(read.as_deref(), Some(&b"durable-before-ephemeral"[..]));
    assert_eq!(
        reopened
            .mutable_overlay_durable_owner_token(&key)
            .unwrap()
            .as_ref()
            .map(|token| token.as_bytes()),
        Some(original_token.as_bytes())
    );
}

#[test]
fn mu17j_l_prepared_foreground_metadata_admission_is_prewrite_for_all_callers() {
    let tp = TempPath::new("prepared-foreground-metadata-admission");
    let store = FileStore::open(tp.path()).unwrap();
    let seed = store.put(b"prepared-foreground-visible-seed").unwrap();
    let inputs = [
        ForegroundMutationInput::WorkflowOwnerState,
        ForegroundMutationInput::MutableOverlayRecords,
        ForegroundMutationInput::AuditRetentionMap,
        ForegroundMutationInput::AuditRetentionDelta,
        ForegroundMutationInput::ObjectBatch,
    ];

    for input in inputs {
        let before_len = std::fs::metadata(tp.path()).unwrap().len();
        let before_identity = {
            let inner = store.inner.lock().unwrap();
            FileStore::foreground_publication_source_identity(&inner)
        };
        let error = {
            let mut inner = store.inner.lock().unwrap();
            let control_map = store.control_map_locked(&mut inner).unwrap();
            let authority = store
                .begin_foreground_transaction_publication(&inner, control_map)
                .unwrap();
            let roots = TxnRoots {
                generation: inner.generation + 1,
                page_count: inner.page_count,
                object_index: inner.index_root,
                free: inner.free.clone(),
                freemap: inner.freemap,
                region_table_root: inner.region_table_root.unwrap(),
                maintenance_root: inner.maintenance_root.unwrap(),
                legacy_overlay: legacy_overlay_root_for_publication(
                    &inner,
                    inner.current_record_root,
                    inner.root_catalog_root,
                ),
                current_record_root: inner.current_record_root,
                root_catalog: TxnRootCatalog {
                    root: inner.root_catalog_root,
                    entries: inner.root_catalog_entries.clone(),
                },
                mutable_overlay_generation_floor: inner.mutable_overlay_generation_floor,
                minimum_recoverable_generation: inner.minimum_recoverable_generation,
                reference: inner.reference_root.map(|digest| *digest.bytes()),
                control: inner.control_root.map(|digest| *digest.bytes()),
                maintenance: inner.maintenance.clone(),
                metadata_bootstrap_reserve: inner.metadata_bootstrap_reserve.clone(),
            };
            let mut file = store.file.lock().unwrap();
            let source_len = file.size().unwrap();
            store
                .prepare_foreground_transaction_publication(
                    &mut **file,
                    &inner,
                    input,
                    &authority,
                    move |planning, _allocator| {
                        planning.grow(source_len + PAGE_SIZE).map_err(io_err)?;
                        planning
                            .pwrite(source_len, &[0xA5; PAGE_SIZE as usize])
                            .map_err(io_err)?;
                        Ok(PreparedForegroundTransactionOutcome {
                            publication: record_io::PreparedForegroundTxnResult::for_test(
                                roots,
                                pagemap::FreeMapPublicationDemand {
                                    btree_node_pages:
                                        pagemap::FOREGROUND_TRANSACTION_METADATA_LIMIT_PAGES + 1,
                                    ..pagemap::FreeMapPublicationDemand::default()
                                },
                            ),
                            value: (),
                        })
                    },
                )
                .err()
                .expect("over-budget prepared foreground transaction")
        };
        assert_eq!(error.code, Code::ResourceExhausted);
        assert_eq!(std::fs::metadata(tp.path()).unwrap().len(), before_len);
        let after_identity = {
            let inner = store.inner.lock().unwrap();
            FileStore::foreground_publication_source_identity(&inner)
        };
        assert_eq!(after_identity, before_identity);
        assert_eq!(
            store.get(&seed).unwrap().as_deref(),
            Some(&b"prepared-foreground-visible-seed"[..])
        );
    }

    drop(store);
    let reopened = FileStore::open(tp.path()).unwrap();
    assert_eq!(
        reopened.get(&seed).unwrap().as_deref(),
        Some(&b"prepared-foreground-visible-seed"[..])
    );
}

#[test]
fn mu17j_l_all_foreground_callers_use_one_prepared_boundary() {
    let source = include_str!("lib.rs");
    let callers = [
        (
            "fn commit_workflow_owner_state_records(",
            "ForegroundMutationInput::WorkflowOwnerState",
        ),
        (
            "fn commit_mutable_overlay_records(",
            "ForegroundMutationInput::MutableOverlayRecords",
        ),
        (
            "fn commit_control_map_and_audit_retention_map(",
            "ForegroundMutationInput::AuditRetentionMap",
        ),
        (
            "fn commit_control_map_and_audit_retention_delta(",
            "ForegroundMutationInput::AuditRetentionDelta",
        ),
        ("fn commit_txn(", "ForegroundMutationInput::ObjectBatch"),
    ];
    for (index, (start, variant)) in callers.iter().enumerate() {
        let start_offset = source.find(start).expect("foreground caller");
        let end_offset = callers
            .get(index + 1)
            .and_then(|(next, _)| source[start_offset + start.len()..].find(next))
            .map_or(source.len(), |relative| {
                start_offset + start.len() + relative
            });
        let body = &source[start_offset..end_offset];
        assert_eq!(
            body.matches(".prepare_foreground_transaction_publication(")
                .count(),
            1,
            "{start}"
        );
        assert_eq!(body.matches(variant).count(), 1, "{start}");
        assert_eq!(
            body.matches(".finish_foreground_txn(").count(),
            1,
            "{start}"
        );
    }
    fn enclosing_function(source: &str, offset: usize) -> String {
        source[..offset]
            .lines()
            .rev()
            .find_map(|line| {
                let function = line.find("fn ")?;
                let rest = &line[function + 3..];
                let name = rest
                    .chars()
                    .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
                    .collect::<String>();
                (!name.is_empty()).then_some(name)
            })
            .expect("finish call is enclosed by a function")
    }

    fn production_rust_sources(
        root: &std::path::Path,
        directory: &std::path::Path,
        sources: &mut Vec<(String, String)>,
    ) {
        for entry in std::fs::read_dir(directory).expect("read loom-store source directory") {
            let path = entry.expect("read loom-store source entry").path();
            if path.is_dir() {
                production_rust_sources(root, &path, sources);
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs")
                && path.file_name().and_then(|name| name.to_str()) != Some("tests.rs")
            {
                sources.push((
                    path.strip_prefix(root)
                        .expect("source path under crate src")
                        .to_string_lossy()
                        .replace('\\', "/"),
                    std::fs::read_to_string(&path).expect("read loom-store production source"),
                ));
            }
        }
    }

    let source_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut production_sources = Vec::new();
    production_rust_sources(&source_root, &source_root, &mut production_sources);
    production_sources.sort_by(|left, right| left.0.cmp(&right.0));
    let call_names = [
        "finish_txn(",
        "finish_txn_with_pre_commit_hook(",
        "finish_foreground_txn_on_planning_backing(",
    ];
    let mut actual = Vec::<(String, String, String)>::new();
    let mut authority_delegations = Vec::<(String, String, String)>::new();
    for (file, contents) in &production_sources {
        for call in call_names {
            for (offset, _) in contents.match_indices(call) {
                let line_start = contents[..offset].rfind('\n').map_or(0, |line| line + 1);
                let prefix = &contents[line_start..offset];
                if prefix.contains("fn ") {
                    continue;
                }
                let function = enclosing_function(contents, offset);
                let entry = (file.clone(), function, call.to_string());
                if entry
                    == (
                        "record_io.rs".to_string(),
                        "finish_txn".to_string(),
                        "finish_txn_with_pre_commit_hook(".to_string(),
                    )
                {
                    authority_delegations.push(entry);
                } else {
                    actual.push(entry);
                }
            }
        }
    }
    assert_eq!(
        authority_delegations,
        vec![(
            "record_io.rs".to_string(),
            "finish_txn".to_string(),
            "finish_txn_with_pre_commit_hook(".to_string(),
        )],
        "the finish_txn wrapper has one internal authority delegation"
    );

    let classified = [
        (
            "lib.rs",
            "commit_workflow_owner_state_records",
            "finish_foreground_txn_on_planning_backing(",
            "prepared foreground",
        ),
        (
            "lib.rs",
            "commit_mutable_overlay_records",
            "finish_foreground_txn_on_planning_backing(",
            "prepared foreground",
        ),
        (
            "lib.rs",
            "commit_control_map_and_audit_retention_map",
            "finish_foreground_txn_on_planning_backing(",
            "prepared foreground",
        ),
        (
            "lib.rs",
            "commit_control_map_and_audit_retention_delta",
            "finish_foreground_txn_on_planning_backing(",
            "prepared foreground",
        ),
        (
            "lib.rs",
            "commit_txn",
            "finish_foreground_txn_on_planning_backing(",
            "prepared foreground",
        ),
        (
            "lib.rs",
            "checkpoint_mutable_overlay_pages",
            "finish_txn(",
            "maintenance/mark epoch",
        ),
        (
            "lib.rs",
            "commit_txn",
            "finish_txn(",
            "maintenance/mark epoch",
        ),
        (
            "mark_epoch.rs",
            "publish_metadata_evidence_chunks_and_epoch",
            "finish_txn(",
            "maintenance/mark epoch",
        ),
        (
            "mark_epoch.rs",
            "publish_reachability_mark_epoch_begin_locked",
            "finish_txn(",
            "maintenance/mark epoch",
        ),
        (
            "mark_epoch.rs",
            "publish_reachability_mark_epoch_clear_locked",
            "finish_txn(",
            "maintenance/mark epoch",
        ),
        (
            "compact.rs",
            "gc_segments_inner",
            "finish_txn(",
            "compaction/GC",
        ),
        (
            "compact.rs",
            "trim_tail_free_pages",
            "finish_txn(",
            "compaction/GC",
        ),
        (
            "compact.rs",
            "compact_tail_once_impl",
            "finish_txn(",
            "compaction/GC",
        ),
        (
            "compact.rs",
            "canonical_relocate_from_evidence",
            "finish_txn_with_pre_commit_hook(",
            "compaction/GC",
        ),
        (
            "lib.rs",
            "activate_source_layout_migration_plan",
            "finish_txn(",
            "migration",
        ),
        (
            "lib.rs",
            "commit_raw_overlay_records_for_test",
            "finish_txn(",
            "test-only",
        ),
        (
            "lib.rs",
            "commit_family_root_records_for_test",
            "finish_txn(",
            "test-only",
        ),
        (
            "lib.rs",
            "commit_current_root_records_for_test",
            "finish_txn(",
            "test-only",
        ),
        (
            "compact.rs",
            "append_committed_free_pages",
            "finish_txn(",
            "test-only",
        ),
    ];
    let mut expected = BTreeMap::<(String, String, String), &'static str>::new();
    for (file, function, call, category) in classified {
        assert!(
            expected
                .insert(
                    (file.to_string(), function.to_string(), call.to_string()),
                    category,
                )
                .is_none(),
            "duplicate finish-call ownership for {file}:{function}:{call}"
        );
    }
    let mut actual_counts = BTreeMap::<(String, String, String), usize>::new();
    for entry in actual {
        *actual_counts.entry(entry).or_default() += 1;
    }
    for (entry, count) in &actual_counts {
        assert_eq!(*count, 1, "duplicate production finish call: {entry:?}");
        assert!(
            expected.contains_key(entry),
            "unclassified production finish call: {entry:?}"
        );
    }
    assert_eq!(
        actual_counts.keys().cloned().collect::<BTreeSet<_>>(),
        expected.keys().cloned().collect::<BTreeSet<_>>(),
        "finish-call inventory changed"
    );
    assert_eq!(
        expected
            .values()
            .filter(|category| **category == "prepared foreground")
            .count(),
        5
    );
    let audit_delta_start = source
        .find("fn commit_control_map_and_audit_retention_delta(")
        .unwrap();
    let audit_delta_end = source[audit_delta_start..]
        .find("fn decode_lock_fence_records(")
        .map_or(source.len(), |relative| audit_delta_start + relative);
    assert_eq!(
        source[audit_delta_start..audit_delta_end]
            .matches("write_audit_retention_delta_to_root(")
            .count(),
        1
    );
}

#[test]
fn mu17j_l_prepared_foreground_root_catalog_absent_unchanged_and_replaced() {
    let tp = TempPath::new("prepared-foreground-root-catalog-cases");
    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(store.inner.lock().unwrap().root_catalog_root, None);

    store.put(b"object-only-before-catalog").unwrap();
    assert_eq!(store.inner.lock().unwrap().root_catalog_root, None);

    let first_key = durability_test_key("prepared-root-catalog-first");
    store
        .put_mutable_overlay_value(first_key, b"first".to_vec())
        .unwrap();
    let first_root = store
        .inner
        .lock()
        .unwrap()
        .root_catalog_root
        .expect("first canonical root catalog");

    store.put(b"object-only-with-catalog").unwrap();
    assert_eq!(
        store.inner.lock().unwrap().root_catalog_root,
        Some(first_root)
    );

    let second_key = durability_test_key("prepared-root-catalog-second");
    store
        .put_mutable_overlay_value(second_key, b"second".to_vec())
        .unwrap();
    let second_root = store
        .inner
        .lock()
        .unwrap()
        .root_catalog_root
        .expect("replacement canonical root catalog");
    assert_ne!(second_root, first_root);

    drop(store);
    let reopened = FileStore::open(tp.path()).unwrap();
    assert_eq!(
        reopened.inner.lock().unwrap().root_catalog_root,
        Some(second_root)
    );
}

#[test]
fn mu17j_l_prepared_foreground_rejects_free_catalog_family_root() {
    let tp = TempPath::new("prepared-foreground-free-catalog-family-root");
    let store = FileStore::open(tp.path()).unwrap();
    store
        .put_mutable_overlay_value(
            durability_test_key("prepared-free-family-root"),
            b"value".to_vec(),
        )
        .unwrap();

    let inner = store.inner.lock().unwrap();
    let free_page = inner.page_count;
    let mut entries = inner.root_catalog_entries.clone();
    entries[0].root = PageId(free_page);
    let mut file = store.file.lock().unwrap();
    let mut planning = PlanningBacking::new(&mut **file).unwrap();
    let mut allocator = PageAllocator::new(
        inner.page_count + 1,
        inner.generation + 1,
        vec![FreePageRun {
            start: free_page,
            len: 1,
            freed_gen: inner.generation,
        }],
    );
    allocator
        .install_metadata_bootstrap_reserve(&inner.metadata_bootstrap_reserve)
        .unwrap();
    let error = finish_txn(
        &mut planning,
        &mut allocator,
        inner.generation + 1,
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
                entries,
            },
            previous_mutable_overlay_generation_floor: inner.mutable_overlay_generation_floor,
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
    .unwrap_err();
    assert_eq!(error.code, Code::CorruptObject);
    assert!(error.message.contains("root catalog family"));
    assert!(error.message.contains("is listed as free"));
}

#[test]
fn rejected_free_map_publication_observer_is_test_scoped_and_resets() {
    let first_diagnostic = RejectedFreeMapPublicationDiagnostic {
        demanded_pages: 513,
        reserve_capacity_pages: 512,
        reserve_available_pages: 64,
        extent_deletes: 2,
        extent_upserts: 3,
        btree_node_pages: 513,
        affected_existing_btree_pages: 7,
        split_decisions: 4,
        dirty_range_count: 5,
        free_map_depth: 2,
    };
    let second_diagnostic = RejectedFreeMapPublicationDiagnostic {
        demanded_pages: 514,
        ..first_diagnostic
    };
    let first_observations = Arc::new(Mutex::new(Vec::new()));
    {
        let observations = Arc::clone(&first_observations);
        let _guard =
            install_rejected_free_map_publication_test_observer(Arc::new(move |diagnostic| {
                observations.lock().unwrap().push(diagnostic)
            }));
        observe_rejected_free_map_publication(first_diagnostic);
    }
    observe_rejected_free_map_publication(second_diagnostic);
    assert_eq!(*first_observations.lock().unwrap(), vec![first_diagnostic]);

    let second_observations = Arc::new(Mutex::new(Vec::new()));
    {
        let observations = Arc::clone(&second_observations);
        let _guard =
            install_rejected_free_map_publication_test_observer(Arc::new(move |diagnostic| {
                observations.lock().unwrap().push(diagnostic)
            }));
        observe_rejected_free_map_publication(second_diagnostic);
    }
    assert_eq!(*first_observations.lock().unwrap(), vec![first_diagnostic]);
    assert_eq!(
        *second_observations.lock().unwrap(),
        vec![second_diagnostic]
    );
}

#[test]
#[ignore = "diagnostic: constructs a typed free-map delta beyond the 512-page admission limit"]
fn diagnostic_rejected_free_map_publication_observer_captures_real_capacity_branch_and_resets() {
    let tp = TempPath::new("rejected-free-map-publication-branch");
    let store = FileStore::open(tp.path()).unwrap();
    // More entries than 512 maximally full extent nodes can hold force real node demand over the
    // metadata admission limit without relying on the removed per-extent payload-page cost.
    let extent_count = 40_000;
    {
        let mut inner = store.inner.lock().unwrap();
        assert!(inner.freemap.is_none());
        let start = inner.page_count.saturating_add(16);
        inner.free = (0..extent_count)
            .map(|index| FreePageRun {
                start: start + index * 2,
                len: 1,
                freed_gen: inner.generation,
            })
            .collect();
        inner.page_count = start + extent_count * 2;
        inner.maintenance.physical_page_count = inner.page_count;
        store
            .file
            .lock()
            .unwrap()
            .grow(DATA_START + inner.page_count * PAGE_SIZE)
            .unwrap();
    }

    let expected_demand = {
        let inner = store.inner.lock().unwrap();
        let mut file = store.file.lock().unwrap();
        pagemap::prepare_tree_map_publication(
            &mut **file,
            DATA_START,
            inner.freemap.map(|(root, _)| root),
            &inner.free,
            Vec::new(),
            Vec::new(),
            inner.page_count,
        )
        .unwrap()
        .demand()
    };
    assert!(
        expected_demand.allocation_pages() > pagemap::FOREGROUND_TRANSACTION_METADATA_LIMIT_PAGES
    );

    let reject = || {
        let mut inner = store.inner.lock().unwrap();
        let control_map = store.control_map_locked(&mut inner).unwrap();
        let authority = store
            .begin_foreground_transaction_publication(&inner, control_map)
            .unwrap();
        let mut allocator = PageAllocator::new_with_reusable_authorities(
            inner.page_count,
            inner.generation + 1,
            inner.free.clone(),
            authority.ordinary_reusable_runs.clone(),
            authority.publication_eligible_runs.clone(),
        );
        allocator
            .install_captured_free_authority(authority.captured_free_authority.clone())
            .unwrap();
        allocator
            .install_metadata_bootstrap_reserve(&inner.metadata_bootstrap_reserve)
            .unwrap();
        let mut file = store.file.lock().unwrap();
        store
            .prepare_foreground_transaction_finalization(
                &mut **file,
                &inner,
                &allocator,
                &authority,
                inner.index_root,
            )
            .err()
            .expect("capacity rejection")
    };

    let observations = Arc::new(Mutex::new(Vec::new()));
    {
        let captured = Arc::clone(&observations);
        let _guard =
            install_rejected_free_map_publication_test_observer(Arc::new(move |diagnostic| {
                captured.lock().unwrap().push(diagnostic)
            }));
        let error = reject();
        assert_eq!(error.code, Code::ResourceExhausted);
        assert_eq!(
            error.message,
            "loom-store: free-map publication exceeds metadata bootstrap capacity"
        );
    }

    let diagnostic = observations.lock().unwrap()[0];
    assert_eq!(observations.lock().unwrap().len(), 1);
    assert_eq!(
        diagnostic.demanded_pages,
        expected_demand.allocation_pages()
    );
    assert_eq!(
        diagnostic.reserve_capacity_pages,
        pagemap::FOREGROUND_TRANSACTION_METADATA_LIMIT_PAGES
    );
    assert_eq!(
        diagnostic.reserve_available_pages,
        pagemap::METADATA_BOOTSTRAP_TARGET_PAGES
    );
    assert_eq!(diagnostic.extent_deletes, expected_demand.extent_deletes);
    assert_eq!(diagnostic.extent_upserts, expected_demand.extent_upserts);
    assert_eq!(
        diagnostic.btree_node_pages,
        expected_demand.btree_node_pages
    );
    assert_eq!(
        diagnostic.affected_existing_btree_pages,
        expected_demand.affected_existing_btree_pages
    );
    assert_eq!(diagnostic.split_decisions, expected_demand.split_decisions);
    assert_eq!(diagnostic.dirty_range_count, 0);
    assert_eq!(diagnostic.free_map_depth, 0);

    let error = reject();
    assert_eq!(error.code, Code::ResourceExhausted);
    assert_eq!(observations.lock().unwrap().as_slice(), &[diagnostic]);
}
