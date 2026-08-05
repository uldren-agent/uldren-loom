//! Persistent single-file (`.loom`) object store - the on-disk `ObjectStore` backend.
//!
//! Runs over a pluggable [`BackingIo`] - a native `std::fs::File`, an in-memory buffer, or a browser
//! OPFS sync handle; see the crate README for the crash-consistency model. [`FileStore`]
//! implements [`loom_core::ObjectStore`] and passes the same `loom-conformance` vectors as
//! `MemoryStore`. The native-file open/lock/compaction lifecycle is `#[cfg]`-gated off for `wasm32`,
//! where the engine instead opens over a caller-supplied backing via [`FileStore::with_backing`].

use loom_core::digest::{Algo, Digest};
use loom_core::error::{Code, LoomError, Result};
use loom_core::lock::LockCoordinator;
use loom_core::{
    AclStore, CommitReceipt, ExternalCredentialKind, IdentityStore, SecondaryIndexWrite,
    VerifiedExternalCredentialAuth, WorkflowTransaction, WorkflowTransactionErrorKind,
    WriteOutcome,
};

#[cfg(not(target_arch = "wasm32"))]
pub mod daemon;
pub mod derived;

/// The capability names (0010 section 5) this crate provides, for the capability-contribution overlay: a build
/// that links `loom-store` supports the single-file store and its at-rest storage transforms. The
/// assembling layer overlays these onto `loom_core::capability::registry()` (see
/// `CapabilitySet::with_supported`).
pub fn provided_capabilities() -> &'static [&'static str] {
    &[
        "single-file-store",
        "compression",
        "encryption-at-rest",
        "rekey",
        "certificate-generate-self-signed",
    ]
}
use loom_core::keys::{DekSession, KeySpec};
use loom_core::provider::ObjectStore;
use loom_core::{CompressionHint, FacetKind, Loom, Object, WorkspaceId};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
#[cfg(not(target_arch = "wasm32"))]
use std::fs::{File, OpenOptions};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;
// `Path` is only referenced by the native-file API (open/open_loom/compaction helpers), all of which
// are cfg-gated off for wasm32; `PathBuf` stays unconditional (it is the `FileStore.path` field type).
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
#[cfg(any(test, feature = "test-hooks"))]
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

#[cfg(any(test, feature = "test-hooks"))]
static MUTABLE_OVERLAY_CURRENT_ENTRIES_ENUMERATIONS: AtomicUsize = AtomicUsize::new(0);

#[cfg(any(test, feature = "test-hooks"))]
pub fn reset_mutable_overlay_current_entries_enumerations() {
    MUTABLE_OVERLAY_CURRENT_ENTRIES_ENUMERATIONS.store(0, Ordering::SeqCst);
}

#[cfg(any(test, feature = "test-hooks"))]
pub fn mutable_overlay_current_entries_enumerations() -> usize {
    MUTABLE_OVERLAY_CURRENT_ENTRIES_ENUMERATIONS.load(Ordering::SeqCst)
}

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ObjectIndexBatchPageStats {
    pub existing_pages_replaced: u64,
    pub new_split_pages_written: u64,
}

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BtreeBatchTransactionPageStats {
    pub existing_pages_replaced: u64,
    pub new_split_pages_written: u64,
}

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ForegroundAllocatorPageStats {
    pub publication_reserved_pages: u64,
    pub publication_reused_pages: u64,
    pub publication_unused_pages: u64,
    pub ordinary_reused_pages: u64,
    pub transaction_reused_pages: u64,
    pub extended_pages: u64,
    pub free_map_updates: u64,
    pub free_map_extent_deletes: u64,
    pub free_map_extent_upserts: u64,
    pub free_map_unique_btree_nodes_touched: u64,
    pub free_map_split_pages: u64,
    pub fixed_metadata_pages: u64,
    pub publication_reserve_exhaustions: u64,
    pub reusable_eligible_pages_left: u64,
    pub metadata_bootstrap_reused_pages: u64,
    pub metadata_bootstrap_extended_pages: u64,
    pub metadata_bootstrap_unused_pages: u64,
}

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreBtreeRootDepth {
    pub root: String,
    pub depth: u64,
}

#[cfg(any(test, feature = "test-hooks"))]
thread_local! {
    static OBJECT_INDEX_BATCH_PAGE_STATS: std::cell::RefCell<Vec<ObjectIndexBatchPageStats>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(any(test, feature = "test-hooks"))]
thread_local! {
    static CURRENT_BTREE_BATCH_TRANSACTION_STATS: std::cell::Cell<BtreeBatchTransactionPageStats> =
        const { std::cell::Cell::new(BtreeBatchTransactionPageStats {
            existing_pages_replaced: 0,
            new_split_pages_written: 0,
        }) };
    static COMPLETED_BTREE_BATCH_TRANSACTION_STATS:
        std::cell::RefCell<Vec<BtreeBatchTransactionPageStats>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static FOREGROUND_ALLOCATOR_PAGE_STATS:
        std::cell::RefCell<Vec<ForegroundAllocatorPageStats>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(any(test, feature = "test-hooks"))]
fn observe_btree_batch(stats: pagebtree::BatchUpsertStats) {
    CURRENT_BTREE_BATCH_TRANSACTION_STATS.with(|current| {
        let current_stats = current.get();
        current.set(BtreeBatchTransactionPageStats {
            existing_pages_replaced: current_stats
                .existing_pages_replaced
                .saturating_add(stats.existing_pages_replaced),
            new_split_pages_written: current_stats
                .new_split_pages_written
                .saturating_add(stats.new_split_pages_written),
        });
    });
}

#[cfg(any(test, feature = "test-hooks"))]
pub(crate) fn complete_btree_batch_transaction_for_test() {
    let stats = CURRENT_BTREE_BATCH_TRANSACTION_STATS
        .with(|current| current.replace(BtreeBatchTransactionPageStats::default()));
    COMPLETED_BTREE_BATCH_TRANSACTION_STATS.with(|completed| {
        completed.borrow_mut().push(stats);
    });
}

#[cfg(any(test, feature = "test-hooks"))]
fn observe_object_index_batch(stats: pagebtree::BatchUpsertStats) {
    observe_btree_batch(stats);
    OBJECT_INDEX_BATCH_PAGE_STATS.with(|observations| {
        observations.borrow_mut().push(ObjectIndexBatchPageStats {
            existing_pages_replaced: stats.existing_pages_replaced,
            new_split_pages_written: stats.new_split_pages_written,
        });
    });
}

#[cfg(any(test, feature = "test-hooks"))]
pub fn take_object_index_batch_page_stats() -> Vec<ObjectIndexBatchPageStats> {
    OBJECT_INDEX_BATCH_PAGE_STATS
        .with(|observations| std::mem::take(&mut *observations.borrow_mut()))
}

#[cfg(any(test, feature = "test-hooks"))]
pub fn take_btree_batch_transaction_page_stats() -> Vec<BtreeBatchTransactionPageStats> {
    CURRENT_BTREE_BATCH_TRANSACTION_STATS.with(|current| {
        current.set(BtreeBatchTransactionPageStats::default());
    });
    COMPLETED_BTREE_BATCH_TRANSACTION_STATS
        .with(|completed| std::mem::take(&mut *completed.borrow_mut()))
}

#[cfg(any(test, feature = "test-hooks"))]
pub(crate) fn complete_foreground_allocator_transaction_for_test(
    stats: pagemap::PageAllocatorTransactionStats,
) {
    FOREGROUND_ALLOCATOR_PAGE_STATS.with(|completed| {
        completed.borrow_mut().push(ForegroundAllocatorPageStats {
            publication_reserved_pages: stats.publication_reserved_pages,
            publication_reused_pages: stats.publication_reused_pages,
            publication_unused_pages: stats.publication_unused_pages,
            ordinary_reused_pages: stats.ordinary_reused_pages,
            transaction_reused_pages: stats.transaction_reused_pages,
            extended_pages: stats.extended_pages,
            free_map_updates: stats.free_map_updates,
            free_map_extent_deletes: stats.free_map_extent_deletes,
            free_map_extent_upserts: stats.free_map_extent_upserts,
            free_map_unique_btree_nodes_touched: stats.free_map_unique_btree_nodes_touched,
            free_map_split_pages: stats.free_map_split_pages,
            fixed_metadata_pages: stats.fixed_metadata_pages,
            publication_reserve_exhaustions: stats.publication_reserve_exhaustions,
            reusable_eligible_pages_left: stats.reusable_eligible_pages_left,
            metadata_bootstrap_reused_pages: stats.metadata_bootstrap_reused_pages,
            metadata_bootstrap_extended_pages: stats.metadata_bootstrap_extended_pages,
            metadata_bootstrap_unused_pages: stats.metadata_bootstrap_unused_pages,
        });
    });
}

#[cfg(any(test, feature = "test-hooks"))]
pub fn take_foreground_allocator_page_stats() -> Vec<ForegroundAllocatorPageStats> {
    FOREGROUND_ALLOCATOR_PAGE_STATS.with(|completed| std::mem::take(&mut *completed.borrow_mut()))
}

#[cfg(any(test, feature = "test-hooks"))]
type StorePublicationFailureTestInjector =
    Arc<dyn Fn(StorePublicationFailureTestBoundary) -> Result<()> + Send + Sync + 'static>;

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorePublicationFailureTestBoundary {
    WorkflowOwnerStateCommit,
    SegmentGcBeforeFinishTxn,
    TailCompactionBeforeFinishTxn,
    AuditRetentionBeforeFinishTxn,
}

#[cfg(any(test, feature = "test-hooks"))]
struct StorePublicationFailureTestEntry {
    id: u64,
    injector: StorePublicationFailureTestInjector,
}

#[cfg(any(test, feature = "test-hooks"))]
static STORE_PUBLICATION_FAILURE_TEST_INJECTORS: std::sync::OnceLock<
    Mutex<BTreeMap<PathBuf, StorePublicationFailureTestEntry>>,
> = std::sync::OnceLock::new();

#[cfg(any(test, feature = "test-hooks"))]
static STORE_PUBLICATION_FAILURE_TEST_NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[cfg(any(test, feature = "test-hooks"))]
pub struct StorePublicationFailureTestGuard {
    path: PathBuf,
    id: u64,
}

#[cfg(any(test, feature = "test-hooks"))]
impl Drop for StorePublicationFailureTestGuard {
    fn drop(&mut self) {
        let Some(registry) = STORE_PUBLICATION_FAILURE_TEST_INJECTORS.get() else {
            return;
        };
        let Ok(mut registry) = registry.lock() else {
            return;
        };
        if registry
            .get(&self.path)
            .is_some_and(|entry| entry.id == self.id)
        {
            registry.remove(&self.path);
        }
    }
}

#[cfg(any(test, feature = "test-hooks"))]
pub fn install_store_publication_failure_test_injector(
    path: PathBuf,
    injector: StorePublicationFailureTestInjector,
) -> StorePublicationFailureTestGuard {
    let id = STORE_PUBLICATION_FAILURE_TEST_NEXT_ID.fetch_add(1, Ordering::Relaxed);
    STORE_PUBLICATION_FAILURE_TEST_INJECTORS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .expect("store publication failure injector registry")
        .insert(
            path.clone(),
            StorePublicationFailureTestEntry { id, injector },
        );
    StorePublicationFailureTestGuard { path, id }
}

#[cfg(any(test, feature = "test-hooks"))]
pub fn store_publication_failure_test_injector_registered(path: &std::path::Path) -> bool {
    STORE_PUBLICATION_FAILURE_TEST_INJECTORS
        .get()
        .and_then(|registry| registry.lock().ok())
        .is_some_and(|registry| registry.contains_key(path))
}

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorePublicationTestEvent {
    WorkflowTransaction,
    DirectPut,
    DirectPutHint,
    BatchReferenceRoot,
    BatchControlReferenceRoot,
    SavedStateAndAudit,
}

#[cfg(any(test, feature = "test-hooks"))]
type StorePublicationTestObserver = Arc<dyn Fn(StorePublicationTestEvent) + Send + Sync + 'static>;

#[cfg(any(test, feature = "test-hooks"))]
struct StorePublicationTestObserverEntry {
    id: u64,
    observer: StorePublicationTestObserver,
}

#[cfg(any(test, feature = "test-hooks"))]
static STORE_PUBLICATION_TEST_OBSERVERS: std::sync::OnceLock<
    Mutex<BTreeMap<PathBuf, StorePublicationTestObserverEntry>>,
> = std::sync::OnceLock::new();

#[cfg(any(test, feature = "test-hooks"))]
static STORE_PUBLICATION_TEST_OBSERVER_NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[cfg(any(test, feature = "test-hooks"))]
pub struct StorePublicationTestGuard {
    path: PathBuf,
    id: u64,
}

#[cfg(any(test, feature = "test-hooks"))]
impl Drop for StorePublicationTestGuard {
    fn drop(&mut self) {
        let Some(registry) = STORE_PUBLICATION_TEST_OBSERVERS.get() else {
            return;
        };
        let Ok(mut registry) = registry.lock() else {
            return;
        };
        if registry
            .get(&self.path)
            .is_some_and(|entry| entry.id == self.id)
        {
            registry.remove(&self.path);
        }
    }
}

#[cfg(any(test, feature = "test-hooks"))]
pub fn install_store_publication_test_observer(
    path: PathBuf,
    observer: StorePublicationTestObserver,
) -> StorePublicationTestGuard {
    let id = STORE_PUBLICATION_TEST_OBSERVER_NEXT_ID.fetch_add(1, Ordering::Relaxed);
    STORE_PUBLICATION_TEST_OBSERVERS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .expect("store publication observer registry")
        .insert(
            path.clone(),
            StorePublicationTestObserverEntry { id, observer },
        );
    StorePublicationTestGuard { path, id }
}

#[cfg(any(test, feature = "test-hooks"))]
pub fn store_publication_test_observer_registered(path: &std::path::Path) -> bool {
    STORE_PUBLICATION_TEST_OBSERVERS
        .get()
        .and_then(|registry| registry.lock().ok())
        .is_some_and(|registry| registry.contains_key(path))
}

#[cfg(any(test, feature = "test-hooks"))]
fn observe_store_publication(path: &std::path::Path, event: StorePublicationTestEvent) {
    let observer = STORE_PUBLICATION_TEST_OBSERVERS
        .get()
        .and_then(|registry| registry.lock().ok())
        .and_then(|registry| registry.get(path).map(|entry| Arc::clone(&entry.observer)));
    if let Some(observer) = observer {
        observer(event);
    }
}

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RejectedFreeMapPublicationDiagnostic {
    pub demanded_pages: u64,
    pub reserve_capacity_pages: u64,
    pub reserve_available_pages: u64,
    pub extent_deletes: u64,
    pub extent_upserts: u64,
    pub btree_node_pages: u64,
    pub affected_existing_btree_pages: u64,
    pub split_decisions: u64,
    pub dirty_range_count: u64,
    pub free_map_depth: u64,
}

#[cfg(any(test, feature = "test-hooks"))]
type RejectedFreeMapPublicationTestObserver =
    Arc<dyn Fn(RejectedFreeMapPublicationDiagnostic) + Send + Sync + 'static>;

#[cfg(any(test, feature = "test-hooks"))]
thread_local! {
    static REJECTED_FREE_MAP_PUBLICATION_TEST_OBSERVERS:
        std::cell::RefCell<Option<RejectedFreeMapPublicationTestObserver>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(any(test, feature = "test-hooks"))]
pub struct RejectedFreeMapPublicationTestGuard {
    previous: Option<RejectedFreeMapPublicationTestObserver>,
    _thread_bound: std::marker::PhantomData<std::rc::Rc<()>>,
}

#[cfg(any(test, feature = "test-hooks"))]
impl Drop for RejectedFreeMapPublicationTestGuard {
    fn drop(&mut self) {
        REJECTED_FREE_MAP_PUBLICATION_TEST_OBSERVERS.with(|observers| {
            *observers.borrow_mut() = self.previous.take();
        });
    }
}

#[cfg(any(test, feature = "test-hooks"))]
pub fn install_rejected_free_map_publication_test_observer(
    observer: RejectedFreeMapPublicationTestObserver,
) -> RejectedFreeMapPublicationTestGuard {
    let previous = REJECTED_FREE_MAP_PUBLICATION_TEST_OBSERVERS
        .with(|observers| observers.borrow_mut().replace(observer));
    RejectedFreeMapPublicationTestGuard {
        previous,
        _thread_bound: std::marker::PhantomData,
    }
}

#[cfg(any(test, feature = "test-hooks"))]
pub(crate) fn observe_rejected_free_map_publication(
    diagnostic: RejectedFreeMapPublicationDiagnostic,
) {
    let observer = REJECTED_FREE_MAP_PUBLICATION_TEST_OBSERVERS
        .with(|observers| observers.borrow().as_ref().map(Arc::clone));
    if let Some(observer) = observer {
        observer(diagnostic);
    }
}

#[cfg(any(test, feature = "test-hooks"))]
fn invoke_store_publication_failure_test_injector(
    path: &std::path::Path,
    boundary: StorePublicationFailureTestBoundary,
) -> Result<()> {
    let injector = STORE_PUBLICATION_FAILURE_TEST_INJECTORS
        .get()
        .and_then(|registry| registry.lock().ok())
        .and_then(|registry| registry.get(path).map(|entry| Arc::clone(&entry.injector)));
    if let Some(injector) = injector {
        injector(boundary)?;
    }
    Ok(())
}

mod delta_pack;
mod frame;
mod journal;
mod maintenance;
#[cfg(not(target_arch = "wasm32"))]
mod maintenance_executor;
mod maintenance_policy;
mod mark_epoch;
mod page;
mod pagebtree;
mod pagemap;
mod record;

use maintenance::{MaintenanceState, read_maintenance};
use page::{
    AUDIT_RETENTION_FAMILY_ID, CHECKPOINT_INDEX_FAMILY_ID, CURRENT_RECORDS_FAMILY_ID,
    CanonicalRegionTable, DELTA_PACK_CANDIDATE_FAMILY_ID, MUTABLE_IDEMPOTENCY_FAMILY_ID,
    MVCC_GENERATION_FAMILY_ID, MetadataBootstrapReserve, OWNER_TOKEN_FAMILY_ID, PAGE_SIZE, PageId,
    RECLAIM_INDEX_FAMILY_ID, RETAINED_HISTORY_FAMILY_ID, RETENTION_INDEX_FAMILY_ID,
    ROOT_FAMILY_REGISTRY, ROOT_FLAG_ADVISORY, ROOT_FLAG_AUTHORITATIVE, RegionTable, RootCatalog,
    RootCatalogEntry, RootFamilyDescriptor, RootFamilyReachability, RootFamilyRole,
    SECONDARY_INDEX_FAMILY_ID, WORKFLOW_IDEMPOTENCY_FAMILY_ID, root_family_descriptor,
};

pub const STORE_PAGE_SIZE: u64 = PAGE_SIZE;

pub fn maintenance_live_root_diagnostics(
    loom: &Loom<FileStore>,
) -> Result<loom_core::LiveRootDiagnostics> {
    let mut extra_roots = Vec::new();
    for (idx, root) in loom
        .store()
        .derived_artifact_roots()?
        .into_iter()
        .enumerate()
    {
        extra_roots.push(("derived_artifact_roots", format!("derived:{idx}"), root));
    }
    if let Some(epoch) = loom.store().active_reachability_mark_epoch()? {
        if let Some(root) = epoch.reference_root {
            extra_roots.push((
                "maintenance_mark_epoch_captured_roots",
                format!("epoch:{}:reference_root", epoch.epoch),
                root,
            ));
        }
        if let Some(root) = epoch.control_fingerprint {
            extra_roots.push((
                "maintenance_mark_epoch_captured_roots",
                format!("epoch:{}:control_fingerprint", epoch.epoch),
                root,
            ));
        }
        for (idx, root) in epoch.derived_roots.into_iter().enumerate() {
            extra_roots.push((
                "maintenance_mark_epoch_captured_roots",
                format!("epoch:{}:derived:{idx}", epoch.epoch),
                root,
            ));
        }
    }
    loom.live_root_diagnostics(loom.store().reference_root(), extra_roots, 8)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreOpenStage {
    Backing,
    JournalRecovery,
    RegionTable,
    RootCatalog,
    FreeMap,
    Index,
    MutableOverlayIndex,
    MutableOverlayRecords,
    MutableOverlayImport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreOpenProgress {
    pub stage: StoreOpenStage,
    pub completed: u64,
    pub total: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoomOpenStage {
    Unlock,
    RuntimePolicy,
    MutableOverlayExport,
    MutableOverlayImport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoomOpenPhaseProgress {
    pub stage: LoomOpenStage,
    pub completed: u64,
    pub total: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoomOpenProgress {
    Store(StoreOpenProgress),
    Engine(loom_core::vcs::EngineStateLoadProgress),
    Loom(LoomOpenPhaseProgress),
    Ready,
}
use pagemap::{FreePageRun, PageAllocator};
use record::{RecordLoc, SlabBuilder};

pub use derived::{
    CALENDAR_DERIVED_INDEX_FORMAT_VERSION, COLUMNAR_ARROW_ARTIFACT, COLUMNAR_ARROW_FORMAT_VERSION,
    CONTACTS_DERIVED_INDEX_FORMAT_VERSION, DATAFRAME_MATERIALIZATION_ARTIFACT_PREFIX,
    DATAFRAME_MATERIALIZATION_FORMAT_VERSION, DerivedArtifactKey, DerivedArtifactRead,
    DerivedArtifactRebuild, DerivedArtifactRecord, DerivedArtifactServingMode,
    DerivedArtifactServingPolicy, DerivedArtifactStamp, DerivedArtifactStatus,
    GRAPH_PROPERTY_INDEX_ARTIFACT_PREFIX, GRAPH_PROPERTY_INDEX_FORMAT_VERSION,
    GRAPH_SPATIAL_INDEX_ARTIFACT_PREFIX, GRAPH_SPATIAL_INDEX_FORMAT_VERSION,
    MAIL_DERIVED_INDEX_FORMAT_VERSION, PIM_DERIVED_INDEX_ARTIFACT_PREFIX, SEARCH_TANTIVY_ARTIFACT,
    SEARCH_TANTIVY_FORMAT_VERSION, VECTOR_HNSW_ARTIFACT, VECTOR_HNSW_FORMAT_VERSION,
    VECTOR_PQ_ARTIFACT, VECTOR_PQ_FORMAT_VERSION, calendar_derived_index_artifact_key,
    calendar_derived_index_artifact_stamp, columnar_arrow_artifact_key,
    columnar_arrow_artifact_stamp, contacts_derived_index_artifact_key,
    contacts_derived_index_artifact_stamp, dataframe_materialization_artifact_key,
    dataframe_materialization_artifact_stamp, decode_search_status_result,
    encode_search_status_result, graph_property_index_artifact_key,
    graph_property_index_artifact_stamp, graph_spatial_index_artifact_key,
    graph_spatial_index_artifact_stamp, mail_derived_index_artifact_key,
    mail_derived_index_artifact_stamp, search_tantivy_artifact_key, search_tantivy_artifact_stamp,
    vector_hnsw_artifact_key, vector_hnsw_artifact_stamp, vector_pq_artifact_key,
    vector_pq_artifact_stamp,
};
pub use frame::Codec;
pub use loom_core::{
    MutableOverlay, MutableOverlayEntrySnapshot, MutableOverlayHealth, OverlayCheckpoint,
    OverlayDurabilityPolicy, OverlayEntryKind, OverlayGeneration, OverlayKey, OverlayOwnerScope,
    OverlayOwnerToken, OverlayPromotionEntry, OverlayPromotionSelection, OverlaySnapshot,
};
#[cfg(not(target_arch = "wasm32"))]
pub use maintenance_executor::{
    StoreMaintenanceClock, StoreMaintenanceRunBudget, StoreMaintenanceRunKind,
    StoreMaintenanceRunOutcome, SystemStoreMaintenanceClock, maintenance_debt_thresholds_met,
    run_store_maintenance_once, run_store_maintenance_once_with_clock,
};
pub use maintenance_policy::{
    StoreMaintenancePolicy, StoreMaintenanceReport, StoreMaintenanceRunState,
};
pub use mark_epoch::{
    ReachabilityMarkEpoch, begin_loom_reachability_mark_epoch, step_loom_reachability_mark_epoch,
    step_loom_reachability_mark_epoch_until, step_loom_reachability_mark_epoch_while,
};

const MAGIC: &[u8; 8] = b"LOOMFS\x00\x01";
const SLOT_SIZE: u64 = 4096;
// The journal ring occupies one slot after the two superblocks; data begins after it. The
// ring holds the newest RING_SLOTS commit records, so an acked commit survives in its own slot until
// a later superblock checkpoint, even as newer commits write other slots.
const JOURNAL_OFFSET: u64 = 2 * SLOT_SIZE;
const RING_SLOTS: u64 = 32; // commit records kept in the ring (32 * RECORD_SIZE = 2112 B < SLOT_SIZE)
const CHECKPOINT_INTERVAL: u64 = 16; // commits between superblock checkpoints; < RING_SLOTS so every
// ring record is folded into a superblock before its slot is reused
pub(crate) const DATA_START: u64 = 3 * SLOT_SIZE; // two superblock slots + one journal-ring slot
const FORMAT_MAJOR: u16 = 1;
const FORMAT_MINOR: u16 = 0;
const REC_MAGIC: u8 = 0xB0;
const CRC_OFFSET: usize = 4092; // CRC-32C over bytes [0, 4092)
const LOCK_NEXT_FENCE_PREFIX: &[u8] = b"lock/fence/next/";
const LOCK_APPLIED_FENCE_PREFIX: &[u8] = b"lock/fence/applied/";
const IDENTITY_STORE_KEY: &[u8] = b"identity/v1";
const ACL_STORE_KEY: &[u8] = b"acl";
const AUDIT_CONFIG_KEY: &[u8] = b"audit/v1/config";
const AUDIT_NEXT_KEY: &[u8] = b"audit/v1/next";
const AUDIT_ENTRY_PREFIX: &[u8] = b"audit/v1/entry/";
const AUDIT_PRUNE_CHECKPOINT_KEY: &[u8] = b"audit/v1/prune-checkpoint";
const SERVED_LISTENER_PREFIX: &[u8] = b"serve/v1/listener/";
const AUTHORITY_REPLICATION_PREFIX: &[u8] = b"authority/v1/replication/";
const CERTIFICATE_BUNDLE_PREFIX: &[u8] = b"certificate/v1/bundle/";
const NETWORK_ACCESS_POLICY_PREFIX: &[u8] = b"network-access/v1/policy/";
const STORE_POLICY_KEY: &[u8] = b"store/v1/policy";
const MUTABLE_OVERLAY_META_ADDRESS: &[u8] = b"mutable-overlay/v1/meta";
const MUTABLE_OVERLAY_CURRENT_ROOT_ADDRESS: &[u8] = b"mutable-overlay/v1/current-root";
const MUTABLE_OVERLAY_ENTRY_ADDRESS_PREFIX: &[u8] = b"mutable-overlay/v1/current/";
const MUTABLE_OVERLAY_OWNER_TOKEN_ADDRESS_PREFIX: &[u8] = b"mutable-overlay/v1/owner-token/";
const MUTABLE_OVERLAY_SECONDARY_INDEX_ADDRESS_PREFIX: &[u8] =
    b"mutable-overlay/v1/secondary-index/";
const MUTABLE_OVERLAY_IDEMPOTENCY_ADDRESS_PREFIX: &[u8] = b"mutable-overlay/v1/idempotency/";
const MUTABLE_OVERLAY_TRANSACTION_IDEMPOTENCY_ADDRESS_PREFIX: &[u8] =
    b"mutable-overlay/v1/transaction-idempotency/";
const RETAINED_HISTORY_HEAD_ADDRESS_PREFIX: &[u8] = b"retained-history/v1/head/";
const RETAINED_HISTORY_RECORD_ADDRESS_PREFIX: &[u8] = b"retained-history/v1/record/";
const MUTABLE_OVERLAY_OWNER_TOKEN_RECORD: &[u8] = b"loom.store.mutable-overlay.owner-token.v1";
const MUTABLE_OVERLAY_SECONDARY_INDEX_RECORD: &[u8] =
    b"loom.store.mutable-overlay.secondary-index.v1";
const MUTABLE_OVERLAY_IDEMPOTENCY_RECORD: &[u8] = b"loom.store.mutable-overlay.idempotency.v1";
const MUTABLE_OVERLAY_TRANSACTION_IDEMPOTENCY_RECORD: &[u8] =
    b"loom.store.mutable-overlay.transaction-idempotency.v1";
const MUTABLE_OVERLAY_CURRENT_ROOT_RECORD: &[u8] = b"loom.store.mutable-overlay.current-root.v1";
const AUDIT_RETENTION_RECORD_ADDRESS_PREFIX: &[u8] = b"audit-retention/v1/record/";
const AUDIT_RETENTION_RECORD: &[u8] = b"loom.store.audit-retention.v1";
#[cfg(test)]
const MVCC_GENERATION_RECORD_ADDRESS_PREFIX: &[u8] = b"mvcc-generation/v1/record/";
#[cfg(test)]
const MVCC_GENERATION_RECORD: &[u8] = b"loom.store.mvcc-generation.v1";
#[cfg(test)]
const RETENTION_INDEX_RECORD_ADDRESS_PREFIX: &[u8] = b"retention-index/v1/record/";
#[cfg(test)]
const RETENTION_INDEX_RECORD: &[u8] = b"loom.store.retention-index.v1";
#[cfg(test)]
const CHECKPOINT_INDEX_RECORD_ADDRESS_PREFIX: &[u8] = b"checkpoint-index/v1/record/";
#[cfg(test)]
const CHECKPOINT_INDEX_RECORD: &[u8] = b"loom.store.checkpoint-index.v1";
#[cfg(test)]
const RECLAIM_INDEX_RECORD_ADDRESS_PREFIX: &[u8] = b"reclaim-index/v1/record/";
#[cfg(test)]
const RECLAIM_INDEX_RECORD: &[u8] = b"loom.store.reclaim-index.v1";
#[cfg(test)]
const DELTA_PACK_ADVISORY_RECORD_ADDRESS_PREFIX: &[u8] = b"delta-pack-advisory/v1/record/";
#[cfg(test)]
const DELTA_PACK_ADVISORY_RECORD: &[u8] = b"loom.store.delta-pack-advisory.v1";
const RETAINED_HISTORY_HEAD_RECORD: &[u8] = b"loom.store.retained-history.head.v1";
const RETAINED_HISTORY_ENTRY_RECORD: &[u8] = b"loom.store.retained-history.entry.v1";
const AUDIT_RECORD_MAGIC: &[u8; 8] = b"LAUDIT1\0";
const AUDIT_CONFIG_MAGIC: &[u8; 8] = b"LAUDCFG1";
const AUDIT_CHECKPOINT_MAGIC: &[u8; 8] = b"LAUDCHK1";
const SERVED_LISTENER_MAGIC: &[u8; 8] = b"LSERVE1\0";
const AUTHORITY_REPLICATION_MAGIC: &[u8; 8] = b"LAUTHR1\0";
const CERTIFICATE_BUNDLE_MAGIC: &[u8; 8] = b"LCERTB1\0";
const NETWORK_ACCESS_POLICY_MAGIC: &[u8; 8] = b"LNETAC1\0";
const STORE_POLICY_MAGIC: &[u8; 8] = b"LSPOLY1\0";
const SERVED_LISTENER_SCHEMA_VERSION: u16 = 3;
const AUTHORITY_REPLICATION_SCHEMA_VERSION: u16 = 1;
const CERTIFICATE_BUNDLE_SCHEMA_VERSION: u16 = 1;
const NETWORK_ACCESS_POLICY_SCHEMA_VERSION: u16 = 1;
const CERTIFICATE_BUNDLE_MAX_PEM_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRecord {
    pub seq: u64,
    pub principal: Option<WorkspaceId>,
    pub action: String,
    pub target: Option<String>,
    pub prev_hash: Option<Digest>,
    pub hash: Digest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditConfig {
    pub retention_days: u32,
    pub legal_hold: bool,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            retention_days: 365,
            legal_hold: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorePolicy {
    pub fips_required: bool,
    pub default_durability: StoreDurabilityPolicy,
    pub facet_durability_overrides: [Option<StoreDurabilityPolicy>; FacetKind::ALL.len()],
}

impl Default for StorePolicy {
    fn default() -> Self {
        Self {
            fips_required: false,
            default_durability: StoreDurabilityPolicy::Normal,
            facet_durability_overrides: [None; FacetKind::ALL.len()],
        }
    }
}

impl StorePolicy {
    pub fn effective_durability(self, facet: FacetKind) -> StoreDurabilityPolicy {
        self.facet_durability_overrides[facet.stable_tag() as usize]
            .unwrap_or(self.default_durability)
    }

    pub fn effective_derived_artifact_durability(
        self,
        facet: FacetKind,
        owner_policy: Option<StoreDurabilityPolicy>,
    ) -> StoreDurabilityPolicy {
        loom_core::strictest_durability([
            StoreDurabilityPolicy::Relaxed,
            self.facet_durability_overrides[facet.stable_tag() as usize]
                .unwrap_or(StoreDurabilityPolicy::Relaxed),
            owner_policy.unwrap_or(StoreDurabilityPolicy::Relaxed),
        ])
    }

    pub fn set_default_durability(&mut self, policy: StoreDurabilityPolicy) -> Result<()> {
        validate_store_durability_policy(policy)?;
        self.default_durability = policy;
        Ok(())
    }

    pub fn set_facet_durability(
        &mut self,
        facet: FacetKind,
        policy: Option<StoreDurabilityPolicy>,
    ) -> Result<()> {
        if let Some(policy) = policy {
            validate_store_durability_policy(policy)?;
        }
        self.facet_durability_overrides[facet.stable_tag() as usize] = policy;
        Ok(())
    }
}

pub type StoreDurabilityPolicy = OverlayDurabilityPolicy;

pub fn validate_store_durability_policy(policy: StoreDurabilityPolicy) -> Result<()> {
    let _ = policy;
    Ok(())
}

pub fn parse_store_durability_policy(value: &str) -> Result<StoreDurabilityPolicy> {
    let policy = StoreDurabilityPolicy::parse(value)?;
    validate_store_durability_policy(policy)?;
    Ok(policy)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditPruneStats {
    pub pruned: u64,
    pub checkpoint_seq: Option<u64>,
    pub checkpoint_hash: Option<Digest>,
    pub audit_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaintenanceStatus {
    pub generation: u64,
    pub object_count: u64,
    pub physical_page_count: u64,
    pub physical_bytes: u64,
    pub reusable_free_pages: u64,
    pub candidate_dead_pages: u64,
    pub tail_free_pages: u64,
    pub tail_free_bytes: u64,
    pub last_validated_mark_epoch: u64,
    pub touched_segments: Vec<u64>,
    pub candidate_segments: Vec<u64>,
    pub segment_overflow: bool,
    /// Group-commit / hot-mutable durability diagnostics.
    pub group_commit: GroupCommitDiagnostics,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreRootCodecDiagnostics {
    pub checked_roots: usize,
    pub failures: Vec<StoreRootCodecDiagnostic>,
    pub details: Vec<StoreRootCodecDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreRootCodecDiagnostic {
    pub root_name: &'static str,
    pub family_id: Option<u16>,
    pub root_page: u64,
    pub byte_offset: u64,
    pub expected_codec: &'static str,
    pub expected_discriminator: u8,
    pub raw_magic: Option<u8>,
    pub raw_flags: Option<u8>,
    pub actual_discriminator: Option<u8>,
    pub in_range: bool,
    pub checksum_ok: bool,
    pub magic_ok: bool,
    pub codec_ok: bool,
    pub reachable: bool,
    pub failure: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLayoutDiscoveryReport {
    pub generation: u64,
    pub page_count: u64,
    pub overlay_root: Option<u64>,
    pub current_record_root: Option<u64>,
    pub root_catalog_root: Option<u64>,
    pub control_root: Option<Digest>,
    pub entries: Vec<SourceLayoutDiscoveryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLayoutDiscoveryEntry {
    pub source_address: String,
    pub family: SourceLayoutFamily,
    pub key_or_identity: Option<String>,
    pub generation: Option<u64>,
    pub sequence: Option<u64>,
    pub payload_digest: Option<String>,
    pub payload_len: Option<usize>,
    pub ownership: SourceLayoutOwnership,
    pub decode_state: SourceLayoutDecodeState,
    pub rejection_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceLayoutMigrationPlan {
    source_identity: SourceLayoutSourceIdentity,
    current_records: Vec<SourceLayoutMigrationRecord>,
    source_pointers: Vec<SourceLayoutMigrationRecord>,
    catalog_families: Vec<SourceLayoutMigrationFamilyPlan>,
    control_records: Vec<SourceLayoutMigrationRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceLayoutSourceIdentity {
    generation: u64,
    page_count: u64,
    region_table_root: Option<u64>,
    overlay_root: Option<u64>,
    current_record_root: Option<u64>,
    root_catalog_root: Option<u64>,
    control_root: Option<Digest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceLayoutMigrationValidation {
    pub(crate) current_record_count: usize,
    pub(crate) source_pointer_count: usize,
    pub(crate) catalog_families: Vec<SourceLayoutMigrationFamilyValidation>,
    pub(crate) control_record_count: usize,
    pub(crate) temporary_current_root: Option<u64>,
    pub(crate) temporary_catalog_roots: Vec<(u16, u64)>,
    pub(crate) temporary_control_root: Option<Digest>,
    pub(crate) temporary_object_index_root: Option<u64>,
    pub(crate) temporary_root_catalog_root: Option<u64>,
    pub(crate) temporary_region_table_root: Option<u64>,
    pub(crate) temporary_page_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceLayoutReplacementPreflight {
    pub(crate) disposition: SourceLayoutReplacementPreflightDisposition,
    pub(crate) source_identity: SourceLayoutSourceIdentity,
    pub(crate) classified_owner_counts: Vec<SourceLayoutClassifiedOwnerCount>,
    pub(crate) validation: Option<SourceLayoutMigrationValidation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceLayoutReplacementPreflightDisposition {
    CanonicalNoop,
    LegacyReady,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourceLayoutClassifiedOwnerCount {
    pub(crate) family: SourceLayoutFamily,
    pub(crate) ownership: SourceLayoutOwnership,
    pub(crate) decode_state: SourceLayoutDecodeState,
    pub(crate) count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceLayoutMigrationFamilyValidation {
    pub(crate) family: SourceLayoutFamily,
    pub(crate) family_id: u16,
    pub(crate) record_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceLayoutMigrationFamilyPlan {
    family: SourceLayoutFamily,
    family_id: u16,
    records: Vec<SourceLayoutMigrationRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceLayoutMigrationRecord {
    source_address: String,
    source_root: Option<u64>,
    canonical_address: String,
    source_family: SourceLayoutFamily,
    source_ownership: SourceLayoutOwnership,
    key_or_identity: Option<String>,
    generation: Option<u64>,
    sequence: Option<u64>,
    payload_digest: String,
    payload_len: usize,
    bytes: Vec<u8>,
}

#[cfg(test)]
static COPY_SOURCE_READ_VIEW_CLONES: AtomicU64 = AtomicU64::new(0);
#[cfg(test)]
static COPY_SOURCE_READ_VIEW_MATERIALIZATIONS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, PartialEq, Eq)]
struct CopySourceReadView {
    historical_index: Option<Vec<([u8; 32], RecordLoc)>>,
}

impl Clone for CopySourceReadView {
    fn clone(&self) -> Self {
        #[cfg(test)]
        if self.historical_index.is_some() {
            COPY_SOURCE_READ_VIEW_CLONES.fetch_add(1, Ordering::Relaxed);
        }
        Self {
            historical_index: self.historical_index.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SourceLayoutFamily {
    CurrentEntry,
    CurrentRootPointer,
    RetainedHistoryHead,
    RetainedHistoryRecord,
    OwnerToken,
    SecondaryIndex,
    MutableIdempotency,
    WorkflowIdempotency,
    AuditControl,
    Control,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SourceLayoutOwnership {
    LegacyOverlay,
    NestedCurrentRoot,
    ControlRootObject,
    OptionalFamilyAbsent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SourceLayoutDecodeState {
    Decoded,
    Absent,
    Malformed,
    UnknownFamily,
    Conflict,
}

/// Group-commit / hot-mutable durability diagnostics.
///
/// Statistic model: cumulative counters plus counts plus
/// point-in-time gauges; consumers derive averages themselves (e.g. mean fsync latency is
/// `fsync_total_micros / fsync_count`, mean records/batch is
/// `group_commit_records_total / group_commit_batches_total`). Durations are microseconds (`u64`);
/// sizes are counts. The cumulative counters are monotonic for the life of an open store handle and
/// reset to zero when the store is reopened.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupCommitDiagnostics {
    /// Number of hot-mutable batch publishes (each a single drain-and-fsync of queued transactions).
    pub group_commit_batches_total: u64,
    /// Total queued transactions folded into those batches.
    pub group_commit_transactions_total: u64,
    /// Total records folded into those batches.
    pub group_commit_records_total: u64,
    /// Cumulative time spent in durable-commit `fsync` calls, microseconds.
    pub fsync_total_micros: u64,
    /// Number of durable-commit `fsync` calls measured.
    pub fsync_count: u64,
    /// Cumulative time spent waiting to acquire the store write lock, microseconds.
    pub write_lock_wait_total_micros: u64,
    /// Number of write-lock acquisitions measured.
    pub write_lock_wait_count: u64,
    /// Point-in-time gauge: transactions currently enqueued in the hot-mutable queue but not yet drained.
    pub pending_durable_window_transactions: u64,
    /// Point-in-time gauge: records currently enqueued in the hot-mutable queue but not yet drained.
    pub pending_durable_window_records: u64,
    /// Whether an external reader lease currently blocks physical reclamation. The value is `0` or
    /// `1`; native advisory locks expose blocker presence, not the number of reader processes.
    /// `None` is reserved for platforms without native file locking.
    pub pinned_reader_blockers: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedStateAndAuditReceipt {
    pub audit_sequences: Vec<u64>,
    pub retained_sequences: Vec<loom_core::RetainedSequenceReceipt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreMvccSnapshotIdentity {
    pub overlay_generation: loom_core::OverlayGeneration,
    pub immutable_base_root: Option<Digest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreMvccSnapshotPin {
    pub pin_id: u64,
    pub identity: StoreMvccSnapshotIdentity,
    pub owner: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreMvccSnapshotDiagnostics {
    pub active_snapshot_count: u64,
    pub oldest_pinned_overlay_generation: Option<loom_core::OverlayGeneration>,
    pub pins: Vec<StoreMvccSnapshotPin>,
}

#[derive(Debug, Default)]
struct StoreMvccSnapshotRegistry {
    next_pin_id: u64,
    pins: BTreeMap<u64, StoreMvccSnapshotPin>,
}

#[derive(Debug)]
pub struct StoreMvccSnapshot {
    pin_id: u64,
    identity: StoreMvccSnapshotIdentity,
    snapshot: loom_core::OverlaySnapshot,
    registry: Arc<Mutex<StoreMvccSnapshotRegistry>>,
    released: AtomicBool,
}

impl StoreMvccSnapshot {
    pub fn pin_id(&self) -> u64 {
        self.pin_id
    }

    pub fn identity(&self) -> StoreMvccSnapshotIdentity {
        self.identity
    }

    pub fn overlay_generation(&self) -> loom_core::OverlayGeneration {
        self.identity.overlay_generation
    }

    pub fn immutable_base_root(&self) -> Option<Digest> {
        self.identity.immutable_base_root
    }

    pub fn is_released(&self) -> bool {
        self.released.load(Ordering::Acquire)
    }

    pub fn read_composite(
        &self,
        key: &loom_core::OverlayKey,
        base_read: impl FnOnce(Option<Digest>, &loom_core::OverlayKey) -> Result<Option<Vec<u8>>>,
    ) -> Result<Option<Vec<u8>>> {
        let base_root = self.identity.immutable_base_root;
        self.snapshot
            .read_composite(key, |key| base_read(base_root, key))
    }

    pub fn release(&self) -> Result<bool> {
        if self.released.swap(true, Ordering::AcqRel) {
            return Ok(false);
        }
        self.registry
            .lock()
            .map_err(|_| poisoned())?
            .pins
            .remove(&self.pin_id);
        Ok(true)
    }
}

impl loom_core::OverlaySnapshotPin for StoreMvccSnapshot {
    fn release(&self) -> Result<bool> {
        StoreMvccSnapshot::release(self)
    }
}

impl Drop for StoreMvccSnapshot {
    fn drop(&mut self) {
        if !self.released.swap(true, Ordering::AcqRel)
            && let Ok(mut registry) = self.registry.lock()
        {
            registry.pins.remove(&self.pin_id);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorePageClassAttribution {
    pub physical_bytes: u64,
    pub page_size: u64,
    pub data_pages: u64,
    pub classes: Vec<StorePageClass>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorePageClass {
    pub class: String,
    pub pages: u64,
    pub bytes: u64,
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreRootStorageAttribution {
    pub physical_bytes: u64,
    pub page_size: u64,
    pub data_pages: u64,
    pub roots: Vec<StoreRootStorageClass>,
    pub object_reverse_ownership: Vec<StoreObjectReverseOwnership>,
    pub stale_owner_reasons: Vec<StoreStaleOwnerReason>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreRootStorageClass {
    pub root: String,
    pub family_id: Option<u16>,
    pub role: String,
    pub present: bool,
    pub tree_pages: u64,
    pub tree_bytes: u64,
    pub record_pages: u64,
    pub payload_bytes: u64,
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreStaleOwnerReason {
    pub reason: String,
    pub pages: u64,
    pub bytes: u64,
    pub current_key: Option<Vec<u8>>,
    pub retained_sequence: Option<u64>,
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreRecordLocationAttribution {
    pub segment_id: u64,
    pub page_index: u64,
    pub slot: u32,
    pub global_page: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreObjectReverseOwnership {
    pub digest: Digest,
    pub record_loc: Option<StoreRecordLocationAttribution>,
    pub frame_kind: String,
    pub byte_span: u64,
    pub payload_bytes: u64,
    pub physical_roots: Vec<String>,
    pub retaining_roots: Vec<String>,
    pub logical_owners: Vec<String>,
    pub current_key: Option<Vec<u8>>,
    pub retained_sequence: Option<u64>,
    pub rebuildable: bool,
    pub unresolved_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutableOverlayCheckpointPlan {
    pub overlay_generation: loom_core::OverlayGeneration,
    pub active_snapshot_count: u64,
    pub oldest_pinned_generation: Option<loom_core::OverlayGeneration>,
    pub pinned_generations: Vec<loom_core::OverlayGeneration>,
    pub current_record_count: u64,
    pub tombstone_count: u64,
    pub compactable_current_records: u64,
    pub blocked_current_records: u64,
    pub stale_record_bytes: u64,
    pub reusable_free_bytes: u64,
    pub current_records: Vec<MutableOverlayCheckpointRecordPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutableOverlayCheckpointRecordPlan {
    pub key: loom_core::OverlayKey,
    pub generation: loom_core::OverlayGeneration,
    pub kind: loom_core::OverlayEntryKind,
    pub page_start: u64,
    pub page_span: u64,
    pub bytes: u64,
    pub blockers: Vec<MutableOverlayReclaimBlocker>,
    pub compactable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutableOverlayCheckpointWriteReport {
    pub planned_current_records: u64,
    pub compacted_current_records: u64,
    pub blocked_current_records: u64,
    pub rewritten_record_bytes: u64,
    pub freed_record_pages: u64,
    pub reusable_free_bytes: u64,
    pub physical_page_count: u64,
}

/// Why a superseded mutable-overlay page run cannot yet return to the allocator. Each variant is
/// reported per page run so operators can see whether storage growth is caused by readers, retention
/// policy, recovery safety, or promotion consumers rather than a collapsed "not reclaimable" state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MutableOverlayReclaimBlocker {
    /// The logical-key current index still points at (or before) the superseded generation.
    CurrentIndexVisible,
    /// A pinned MVCC snapshot can still read the superseded record.
    PinnedSnapshot,
    /// A retained-history checkpoint can still read the superseded record.
    RetainedHistory,
    /// Audit retention (operation log / legal hold) has not reached its compaction horizon.
    AuditRetention,
    /// A tombstone is still required to hide a value reachable from the immutable base through
    /// composite reads. Cleared once a later value supersedes the tombstone or the base no longer
    /// exposes the deleted record.
    TombstoneRetention,
    /// The durable-generation floor has not reached the superseding generation, so recovery could
    /// still roll the replacement back.
    DurableGenerationWindow,
    /// A strict promotion, sync, export, ledger, or audit boundary pins its selected checkpoint.
    StrictPromotionBoundary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MutableOverlayReclaimState {
    pub superseded_generation: u64,
    pub superseding_generation: u64,
    pub latest_index_generation: u64,
    pub oldest_pinned_snapshot_generation: Option<u64>,
    pub retained_history_generation: Option<u64>,
    pub audit_retention_active: bool,
    pub tombstone_masks_base: bool,
    pub durable_reclaim_floor: u64,
    pub strict_promotion_generation: Option<u64>,
}

impl MutableOverlayReclaimState {
    pub fn blockers(self) -> Result<Vec<MutableOverlayReclaimBlocker>> {
        if self.superseding_generation <= self.superseded_generation {
            return Err(LoomError::invalid(
                "mutable overlay reclaim requires a later superseding generation",
            ));
        }
        let mut blockers = Vec::new();
        if self.latest_index_generation <= self.superseded_generation {
            blockers.push(MutableOverlayReclaimBlocker::CurrentIndexVisible);
        }
        if generation_in_superseded_window(
            self.oldest_pinned_snapshot_generation,
            self.superseded_generation,
            self.superseding_generation,
        ) {
            blockers.push(MutableOverlayReclaimBlocker::PinnedSnapshot);
        }
        if generation_in_superseded_window(
            self.retained_history_generation,
            self.superseded_generation,
            self.superseding_generation,
        ) {
            blockers.push(MutableOverlayReclaimBlocker::RetainedHistory);
        }
        if self.audit_retention_active {
            blockers.push(MutableOverlayReclaimBlocker::AuditRetention);
        }
        if self.tombstone_masks_base {
            blockers.push(MutableOverlayReclaimBlocker::TombstoneRetention);
        }
        if self.durable_reclaim_floor < self.superseding_generation {
            blockers.push(MutableOverlayReclaimBlocker::DurableGenerationWindow);
        }
        if generation_in_superseded_window(
            self.strict_promotion_generation,
            self.superseded_generation,
            self.superseding_generation,
        ) {
            blockers.push(MutableOverlayReclaimBlocker::StrictPromotionBoundary);
        }
        blockers.sort();
        blockers.dedup();
        Ok(blockers)
    }

    pub fn is_eligible(self) -> Result<bool> {
        self.blockers().map(|blockers| blockers.is_empty())
    }
}

fn generation_in_superseded_window(
    generation: Option<u64>,
    superseded_generation: u64,
    superseding_generation: u64,
) -> bool {
    generation
        .map(|generation| {
            generation >= superseded_generation && generation < superseding_generation
        })
        .unwrap_or(false)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoreIoStats {
    pub locator_cache_entries: u64,
    pub locator_cache_hits: u64,
    pub locator_cache_misses: u64,
    pub index_page_cache_entries: u64,
    pub index_page_cache_hits: u64,
    pub index_page_cache_misses: u64,
    pub index_pages_read: u64,
    pub sparse_index_lookup_count: u64,
    pub materialized_index_lookup_count: u64,
    pub open_mutable_current_records_loaded: u64,
    pub open_mutable_control_records_skipped: u64,
    pub open_mutable_used_current_root: bool,
    pub open_index_materialized: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AuditCheckpoint {
    seq: u64,
    hash: Digest,
}

#[derive(Debug, Default)]
struct AuditRetentionDelta {
    puts: BTreeMap<Vec<u8>, Vec<u8>>,
    deletes: BTreeSet<Vec<u8>>,
}

impl AuditRetentionDelta {
    fn is_empty(&self) -> bool {
        self.puts.is_empty() && self.deletes.is_empty()
    }

    fn put(&mut self, key: &[u8], value: Vec<u8>) {
        self.deletes.remove(key);
        self.puts.insert(key.to_vec(), value);
    }

    fn delete(&mut self, key: Vec<u8>) {
        self.puts.remove(&key);
        self.deletes.insert(key);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertificateBundleRecord {
    pub name: String,
    pub schema_version: u16,
    pub profile: String,
    pub server_cert_chain_pem: Vec<u8>,
    pub private_key_pem: Vec<u8>,
    pub trust_bundle_pem: Option<Vec<u8>>,
    pub server_cert_chain_digest: Digest,
    pub private_key_digest: Digest,
    pub trust_bundle_digest: Option<Digest>,
    pub created_audit_seq: Option<u64>,
    pub updated_audit_seq: Option<u64>,
    pub unencrypted_private_key_override: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServedListenerRecord {
    pub id: String,
    pub schema_version: u16,
    pub surface: String,
    pub selectors: Vec<String>,
    pub transport: String,
    pub profile: Option<String>,
    pub bind: String,
    pub enabled: bool,
    pub tls: ServedListenerTls,
    pub auth: ServedListenerAuth,
    pub limits: ServedListenerLimits,
    pub audit: ServedListenerAudit,
    pub route_scope: String,
    pub exposure: String,
    pub network_access_policy_ref: Option<String>,
    pub last_modified_audit_seq: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkAccessAction {
    Allow,
    Deny,
}

impl NetworkAccessAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "allow" => Ok(Self::Allow),
            "deny" => Ok(Self::Deny),
            _ => Err(LoomError::invalid("network access action is unsupported")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct NetworkAccessCidr {
    pub addr: IpAddr,
    pub prefix: u8,
}

impl NetworkAccessCidr {
    pub fn parse(value: &str) -> Result<Self> {
        let (addr, prefix) = match value.split_once('/') {
            Some((addr, prefix)) => {
                let addr = addr
                    .parse::<IpAddr>()
                    .map_err(|e| LoomError::invalid(format!("invalid CIDR address: {e}")))?;
                let prefix = prefix
                    .parse::<u8>()
                    .map_err(|e| LoomError::invalid(format!("invalid CIDR prefix: {e}")))?;
                (addr, prefix)
            }
            None => {
                let addr = value
                    .parse::<IpAddr>()
                    .map_err(|e| LoomError::invalid(format!("invalid IP address: {e}")))?;
                let prefix = match addr {
                    IpAddr::V4(_) => 32,
                    IpAddr::V6(_) => 128,
                };
                (addr, prefix)
            }
        };
        Self::new(addr, prefix)
    }

    pub fn new(addr: IpAddr, prefix: u8) -> Result<Self> {
        let max = match addr {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        if prefix > max {
            return Err(LoomError::invalid("CIDR prefix exceeds address width"));
        }
        let normalized = normalize_ip(addr, prefix);
        if normalized != addr {
            return Err(LoomError::invalid(
                "CIDR address contains host bits; use the canonical network address",
            ));
        }
        Ok(Self {
            addr: normalized,
            prefix,
        })
    }

    pub fn contains(self, addr: IpAddr) -> bool {
        match (self.addr, addr) {
            (IpAddr::V4(network), IpAddr::V4(addr)) => {
                let mask = ipv4_mask(self.prefix);
                (u32::from(network) & mask) == (u32::from(addr) & mask)
            }
            (IpAddr::V6(network), IpAddr::V6(addr)) => {
                let mask = ipv6_mask(self.prefix);
                (u128::from(network) & mask) == (u128::from(addr) & mask)
            }
            _ => false,
        }
    }
}

impl std::fmt::Display for NetworkAccessCidr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.addr, self.prefix)
    }
}

fn normalize_ip(addr: IpAddr, prefix: u8) -> IpAddr {
    match addr {
        IpAddr::V4(addr) => IpAddr::V4(Ipv4Addr::from(u32::from(addr) & ipv4_mask(prefix))),
        IpAddr::V6(addr) => IpAddr::V6(Ipv6Addr::from(u128::from(addr) & ipv6_mask(prefix))),
    }
}

fn ipv4_mask(prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - u32::from(prefix))
    }
}

fn ipv6_mask(prefix: u8) -> u128 {
    if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - u32::from(prefix))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkAccessRule {
    pub id: String,
    pub action: NetworkAccessAction,
    pub source_cidr: Option<NetworkAccessCidr>,
    pub trusted_proxy_cidr: Option<NetworkAccessCidr>,
    pub require_mtls: bool,
    pub client_cert_subject: Option<String>,
    pub client_cert_san: Option<String>,
    pub client_cert_issuer: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkAccessPolicyRecord {
    pub name: String,
    pub schema_version: u16,
    pub description: Option<String>,
    pub default_action: NetworkAccessAction,
    pub rules: Vec<NetworkAccessRule>,
    pub created_audit_seq: Option<u64>,
    pub updated_audit_seq: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServedListenerTls {
    pub mode: String,
    pub certificate_bundle_ref: Option<String>,
}

impl Default for ServedListenerTls {
    fn default() -> Self {
        Self {
            mode: "off".to_string(),
            certificate_bundle_ref: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServedListenerAuth {
    pub mode: String,
}

impl Default for ServedListenerAuth {
    fn default() -> Self {
        Self {
            mode: "owner-or-passphrase".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServedListenerLimits {
    pub request_size_limit: u64,
    pub idle_timeout_ms: u64,
    pub session_timeout_ms: u64,
}

impl Default for ServedListenerLimits {
    fn default() -> Self {
        Self {
            request_size_limit: 16 * 1024 * 1024,
            idle_timeout_ms: 60_000,
            session_timeout_ms: 3_600_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServedListenerAudit {
    pub mode: String,
}

impl Default for ServedListenerAudit {
    fn default() -> Self {
        Self {
            mode: "management-and-security".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityReplicationPolicy {
    pub id: String,
    pub schema_version: u16,
    pub source: String,
    pub enabled: bool,
    pub pull_on_start: bool,
    pub interval_ms: Option<u64>,
    pub jitter_ms: u64,
    pub backoff_ms: u64,
    pub publish_witness: bool,
    pub last_success_ms: Option<u64>,
    pub last_failure_ms: Option<u64>,
    pub last_error: Option<String>,
    pub last_modified_audit_seq: Option<u64>,
}

// ---- reuse window -------------------------------------------------------------------------------

// Generations within which a committed root-set can still be recovered (the journal ring plus the two
// alternating superblock checkpoints). A page freed at generation `g` is only safe to reuse once `g`
// is older than this window, so no recoverable generation still references it.
pub(crate) const REUSE_SAFE_WINDOW: u64 = if RING_SLOTS > 2 * CHECKPOINT_INTERVAL {
    RING_SLOTS
} else {
    2 * CHECKPOINT_INTERVAL
};

#[cfg(any(test, feature = "test-hooks"))]
pub const REUSE_SAFE_GENERATION_WINDOW: u64 = REUSE_SAFE_WINDOW;

#[cfg(not(test))]
const LOCATOR_CACHE_LIMIT: usize = 4096;
#[cfg(test)]
const LOCATOR_CACHE_LIMIT: usize = 16;
#[cfg(not(test))]
const INDEX_PAGE_CACHE_LIMIT: usize = 1024;
#[cfg(test)]
const INDEX_PAGE_CACHE_LIMIT: usize = 8;

// ---- the on-disk object store ------------------------------------------------------------------

/// A content-addressed [`ObjectStore`] backed by one `.loom` file. Crash-consistent via the
/// two-slot superblock commit point; the `digest -> offset` index is a copy-on-write B-tree
/// rooted from the superblock and read by bounded B-tree lookups unless a full maintenance operation
/// explicitly materializes the index.
#[derive(Debug)]
pub struct FileStore {
    file: Mutex<Box<dyn BackingIo>>,
    // Mutable committed state behind one lock, so writes take `&self` and the store can be shared
    // across threads; a commit holds this lock for its whole critical section, serializing writers.
    inner: Mutex<Inner>,
    // Read only by native compaction's atomic rename; the wasm32 build has no compaction (those
    // methods are cfg-gated off), so there the field is write-only - allow it rather than drop it,
    // keeping one struct shape across targets.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    path: PathBuf, // the file's path, for compaction's atomic rename-replace
    #[cfg(not(target_arch = "wasm32"))]
    _reclamation_reader_lease: Option<File>,
    default_codec: Codec, // codec attempted for new object records; a runtime write policy only,
    // reads are self-describing, so it isn't persisted
    group: Mutex<GroupCommit>, // staging queue that coalesces concurrent writers into one fsync
    // Lock-free durability diagnostics counters surfaced through GroupCommitDiagnostics.
    group_commit_metrics: GroupCommitMetrics,
    mutable_overlay: Mutex<loom_core::MutableOverlay>,
    copy_source_read_view: Mutex<Option<CopySourceReadView>>,
    mutable_overlay_enumerations: AtomicU64,
    mutable_overlay_prefix_enumerations: AtomicU64,
    mutable_overlay_prefix_entries_returned: AtomicU64,
    overlay_publication: Mutex<()>,
    pending_mutable_idempotency: Mutex<BTreeMap<String, PendingMutableIdempotency>>,
    pending_workflow_idempotency: Mutex<BTreeMap<Vec<u8>, PendingWorkflowIdempotency>>,
    mvcc_snapshot_registry: Arc<Mutex<StoreMvccSnapshotRegistry>>,
    pub(crate) hot_mutable_queue: Mutex<HotMutableCommitQueue>,
    maintenance_index_scan: Mutex<Option<Vec<u8>>>,
    // The unlocked data-encryption-key session for an encrypted Loom, or `None` when
    // the store is unencrypted or still locked. Object seal/unseal requires this; a read that
    // needs it while `None` is `E2eLocked`. Behind its own lock so `unlock` takes `&self`.
    dek: Mutex<Option<loom_core::keys::DekSession>>,
    // The store's identity-profile digest algorithm: every object address in this
    // store is `Digest::hash(digest_algo, ..)`. Chosen at creation, read from the superblock on open,
    // and immutable (a profile change is an explicit migration, never an in-place rekey).
    digest_algo: Algo,
    #[cfg(test)]
    post_commit_pre_adopt_hook: PostCommitPreAdoptHookSlot,
    #[cfg(test)]
    source_layout_activation_pre_finish_hook: SourceLayoutActivationPreFinishHookSlot,
    #[cfg(test)]
    reachability_epoch_pre_finish_hook: ReachabilityEpochPreFinishHookSlot,
    #[cfg(test)]
    source_layout_preflight_after_discovery_hook: SourceLayoutPreflightAfterDiscoveryHookSlot,
    #[cfg(test)]
    audit_retention_test_instrumentation: AuditRetentionTestInstrumentation,
}

#[derive(Debug)]
struct Inner {
    index: BTreeMap<[u8; 32], RecordLoc>, // in-memory cache of digest -> record locator
    locator_cache_order: VecDeque<[u8; 32]>,
    index_page_cache: BTreeMap<PageId, [u8; PAGE_SIZE as usize]>,
    index_page_cache_order: VecDeque<PageId>,
    io_stats: StoreIoStats,
    index_materialized: bool,
    page_count: u64, // pages the array spans; the file is header + page_count pages
    generation: u64,
    reference_root: Option<Digest>, // the engine-state root object digest, if any
    control_root: Option<Digest>,   // durable-local control-plane root object digest, if any
    index_root: Option<PageId>,     // page of the object-index CoW B-tree root, if any
    overlay_root: Option<PageId>,   // page of the mutable overlay current-record CoW B-tree root
    current_record_root: Option<PageId>,
    root_catalog_root: Option<PageId>,
    root_catalog_entries: Vec<RootCatalogEntry>,
    mutable_overlay_generation_floor: u64,
    minimum_recoverable_generation: u64,
    retained_history_root: Option<PageId>,
    owner_token_root: Option<PageId>,
    secondary_index_root: Option<PageId>,
    mutable_idempotency_root: Option<PageId>,
    workflow_idempotency_root: Option<PageId>,
    audit_retention_root: Option<PageId>,
    mvcc_generation_root: Option<PageId>,
    retention_index_root: Option<PageId>,
    checkpoint_index_root: Option<PageId>,
    reclaim_index_root: Option<PageId>,
    freemap: Option<(PageId, u64)>, // (root, page span) of the persisted free-page map
    region_table_root: Option<PageId>, // page holding the region roots, freed and rewritten each commit
    maintenance_root: Option<PageId>,  // page holding conservative maintenance metadata
    maintenance: MaintenanceState,
    active_mark_epoch_reclaim_fence: Option<u64>,
    open_segment: u64,      // segment new record pages are attributed to
    free: Vec<FreePageRun>, // reclaimable page runs (superseded pages), persisted each commit
    metadata_bootstrap_reserve: MetadataBootstrapReserve,
    // Encoded `encryption_meta`, immutable after creation; carried into every
    // superblock write so checkpoints and compaction preserve it. `None` = unencrypted.
    encryption_meta: Option<Vec<u8>>,
}

#[derive(Clone, Copy, Default)]
struct RootCatalogFamilyRoots {
    retained_history: Option<PageId>,
    owner_token: Option<PageId>,
    secondary_index: Option<PageId>,
    mutable_idempotency: Option<PageId>,
    workflow_idempotency: Option<PageId>,
    audit_retention: Option<PageId>,
    mvcc_generation: Option<PageId>,
    retention_index: Option<PageId>,
    checkpoint_index: Option<PageId>,
    reclaim_index: Option<PageId>,
}

#[derive(Clone, Copy)]
struct StoreRootCodecExpectation {
    root_name: &'static str,
    family_id: Option<u16>,
    root: Option<PageId>,
    codec: Option<pagebtree::ValueCodecKind>,
}

fn root_catalog_family_root(entries: &[RootCatalogEntry], family_id: u16) -> Option<PageId> {
    entries
        .iter()
        .find(|entry| entry.family_id == family_id)
        .map(|entry| entry.root)
}

fn root_catalog_codec_expectation(
    family_id: u16,
    root: Option<PageId>,
) -> StoreRootCodecExpectation {
    let descriptor = root_family_descriptor(family_id);
    StoreRootCodecExpectation {
        root_name: descriptor.map_or("unknown_family", |descriptor| descriptor.name),
        family_id: Some(family_id),
        root,
        codec: descriptor.map(|descriptor| descriptor.value_codec),
    }
}

pub(crate) fn root_family_value_codec(family_id: u16) -> Result<pagebtree::ValueCodecKind> {
    root_family_descriptor(family_id)
        .map(|descriptor| descriptor.value_codec)
        .ok_or_else(|| corrupt("unknown root family value codec"))
}

fn root_family_get(
    file: &mut dyn BackingIo,
    family_id: u16,
    root: Option<PageId>,
    key: &[u8; 32],
    page_count: u64,
) -> Result<Option<RecordLoc>> {
    pagebtree::get_with_codec(
        file,
        DATA_START,
        root,
        key,
        page_count,
        root_family_value_codec(family_id)?,
    )
}

pub(crate) fn root_family_load_all(
    file: &mut dyn BackingIo,
    family_id: u16,
    root: PageId,
    page_count: u64,
) -> Result<Vec<([u8; 32], RecordLoc)>> {
    pagebtree::load_all_with_codec(
        file,
        DATA_START,
        root,
        page_count,
        root_family_value_codec(family_id)?,
    )
}

pub(crate) fn root_family_collect_pages(
    file: &mut dyn BackingIo,
    family_id: u16,
    root: PageId,
    page_count: u64,
) -> Result<Vec<PageId>> {
    pagebtree::collect_pages_with_codec(
        file,
        DATA_START,
        root,
        page_count,
        root_family_value_codec(family_id)?,
    )
}

fn root_family_free_all(
    file: &mut dyn BackingIo,
    alloc: &mut PageAllocator,
    family_id: u16,
    root: PageId,
    page_count: u64,
) -> Result<()> {
    pagebtree::free_all_with_codec(
        file,
        DATA_START,
        alloc,
        root,
        page_count,
        root_family_value_codec(family_id)?,
    )
}

fn root_catalog_family_roots(entries: &[RootCatalogEntry]) -> RootCatalogFamilyRoots {
    RootCatalogFamilyRoots {
        retained_history: root_catalog_family_root(entries, RETAINED_HISTORY_FAMILY_ID),
        owner_token: root_catalog_family_root(entries, OWNER_TOKEN_FAMILY_ID),
        secondary_index: root_catalog_family_root(entries, SECONDARY_INDEX_FAMILY_ID),
        mutable_idempotency: root_catalog_family_root(entries, MUTABLE_IDEMPOTENCY_FAMILY_ID),
        workflow_idempotency: root_catalog_family_root(entries, WORKFLOW_IDEMPOTENCY_FAMILY_ID),
        audit_retention: root_catalog_family_root(entries, AUDIT_RETENTION_FAMILY_ID),
        mvcc_generation: root_catalog_family_root(entries, MVCC_GENERATION_FAMILY_ID),
        retention_index: root_catalog_family_root(entries, RETENTION_INDEX_FAMILY_ID),
        checkpoint_index: root_catalog_family_root(entries, CHECKPOINT_INDEX_FAMILY_ID),
        reclaim_index: root_catalog_family_root(entries, RECLAIM_INDEX_FAMILY_ID),
    }
}

fn legacy_overlay_root_for_publication(
    inner: &Inner,
    current_record_root: Option<PageId>,
    root_catalog_root: Option<PageId>,
) -> Option<PageId> {
    if current_record_root.is_some() || root_catalog_root.is_some() {
        None
    } else {
        inner.overlay_root
    }
}

#[cfg(test)]
type PostCommitPreAdoptHook = Box<dyn FnOnce(&TxnRoots) -> Result<()> + Send>;

#[cfg(test)]
struct PostCommitPreAdoptHookSlot(Mutex<Option<PostCommitPreAdoptHook>>);

#[cfg(test)]
impl Default for PostCommitPreAdoptHookSlot {
    fn default() -> Self {
        Self(Mutex::new(None))
    }
}

#[cfg(test)]
impl std::fmt::Debug for PostCommitPreAdoptHookSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostCommitPreAdoptHookSlot")
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
type SourceLayoutActivationPreFinishHook = Box<dyn FnOnce() -> Result<()> + Send>;

#[cfg(test)]
struct SourceLayoutActivationPreFinishHookSlot(Mutex<Option<SourceLayoutActivationPreFinishHook>>);

#[cfg(test)]
impl Default for SourceLayoutActivationPreFinishHookSlot {
    fn default() -> Self {
        Self(Mutex::new(None))
    }
}

#[cfg(test)]
impl std::fmt::Debug for SourceLayoutActivationPreFinishHookSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceLayoutActivationPreFinishHookSlot")
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
type ReachabilityEpochPreFinishHook = Box<dyn FnOnce() -> Result<()> + Send>;

#[cfg(test)]
struct ReachabilityEpochPreFinishHookSlot(Mutex<Option<ReachabilityEpochPreFinishHook>>);

#[cfg(test)]
impl Default for ReachabilityEpochPreFinishHookSlot {
    fn default() -> Self {
        Self(Mutex::new(None))
    }
}

#[cfg(test)]
impl std::fmt::Debug for ReachabilityEpochPreFinishHookSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReachabilityEpochPreFinishHookSlot")
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
type SourceLayoutPreflightAfterDiscoveryHook = Box<dyn FnOnce(&Mutex<Inner>) -> Result<()> + Send>;

#[cfg(test)]
struct SourceLayoutPreflightAfterDiscoveryHookSlot(
    Mutex<Option<SourceLayoutPreflightAfterDiscoveryHook>>,
);

#[cfg(test)]
impl Default for SourceLayoutPreflightAfterDiscoveryHookSlot {
    fn default() -> Self {
        Self(Mutex::new(None))
    }
}

#[cfg(test)]
impl std::fmt::Debug for SourceLayoutPreflightAfterDiscoveryHookSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceLayoutPreflightAfterDiscoveryHookSlot")
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[derive(Default, Debug)]
struct AuditRetentionTestInstrumentation {
    point_puts: AtomicU64,
    point_deletes: AtomicU64,
    full_family_enumerations: AtomicU64,
}

#[cfg(test)]
impl AuditRetentionTestInstrumentation {
    fn reset(&self) {
        self.point_puts.store(0, Ordering::SeqCst);
        self.point_deletes.store(0, Ordering::SeqCst);
        self.full_family_enumerations.store(0, Ordering::SeqCst);
    }

    fn point_write_counts(&self) -> (u64, u64) {
        (
            self.point_puts.load(Ordering::SeqCst),
            self.point_deletes.load(Ordering::SeqCst),
        )
    }

    fn full_family_enumerations(&self) -> u64 {
        self.full_family_enumerations.load(Ordering::SeqCst)
    }
}

/// One submitter's completion slot. The leader fills `outcome` for every submitter whose objects it
/// committed in a batch, then wakes them; each submitter waits on its own slot, so a batch's result
/// is never read by a submitter from a different batch.
#[derive(Debug)]
struct Waiter {
    outcome: Mutex<Option<Result<()>>>,
    cv: Condvar,
}

#[derive(Debug, Clone)]
struct PendingMutableIdempotency {
    request_digest: Digest,
    owner_token: loom_core::OverlayOwnerToken,
    waiter: Arc<Waiter>,
}

#[derive(Debug, Clone)]
struct PendingWorkflowIdempotency {
    request_digest: Digest,
    receipt: CommitReceipt,
    waiter: Arc<Waiter>,
}

#[derive(Debug, Clone)]
struct WorkflowIdempotencyCommitRecord {
    key: Vec<u8>,
    request_digest: Digest,
    receipt: CommitReceipt,
}

/// The group-commit staging area. Concurrent writers enqueue their objects here; whichever writer
/// finds no leader active becomes the leader and commits the whole queue in one fsync'd transaction,
/// while the rest wait. `pending` and `waiters` are non-empty together: every submitter enqueues at
/// least one object and exactly one waiter, so the leader can break when the queue drains.
#[derive(Debug, Default)]
struct GroupCommit {
    pending: Vec<(Digest, Vec<u8>, Codec)>, // owned: the leader commits other threads' objects too
    waiters: Vec<Arc<Waiter>>,
    leader_active: bool,
}

/// Thread-safe accumulator behind [`GroupCommitDiagnostics`]. Plain atomics with `Relaxed`
/// ordering: these are diagnostic counters, not synchronization, so recording a sample never takes a
/// lock and adds only a single relaxed add on the hot path. Recording is one measurement per
/// batch / fsync / lock-acquisition event - never per record.
#[derive(Debug, Default)]
pub(crate) struct GroupCommitMetrics {
    batches_total: AtomicU64,
    transactions_total: AtomicU64,
    records_total: AtomicU64,
    fsync_total_micros: AtomicU64,
    fsync_count: AtomicU64,
    write_lock_wait_total_micros: AtomicU64,
    write_lock_wait_count: AtomicU64,
}

impl GroupCommitMetrics {
    /// Record one published batch: `transactions` queued transactions and `records` records drained.
    fn record_batch(&self, transactions: u64, records: u64) {
        self.batches_total.fetch_add(1, Ordering::Relaxed);
        self.transactions_total
            .fetch_add(transactions, Ordering::Relaxed);
        self.records_total.fetch_add(records, Ordering::Relaxed);
    }

    /// Record one durable-commit `fsync` and how long it took.
    pub(crate) fn record_fsync(&self, elapsed: std::time::Duration) {
        self.fsync_total_micros
            .fetch_add(elapsed.as_micros() as u64, Ordering::Relaxed);
        self.fsync_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record one write-lock acquisition and how long it waited.
    fn record_write_lock_wait(&self, elapsed: std::time::Duration) {
        self.write_lock_wait_total_micros
            .fetch_add(elapsed.as_micros() as u64, Ordering::Relaxed);
        self.write_lock_wait_count.fetch_add(1, Ordering::Relaxed);
    }
}

const HOT_MUTABLE_QUEUE_MAX_TRANSACTIONS: usize = 1024;
const HOT_MUTABLE_QUEUE_MAX_RECORDS: usize = 8192;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotMutableCommit {
    pub sequence: u64,
    pub base_generation: u64,
    pub pending_generation: u64,
    pub durability: StoreDurabilityPolicy,
    pub records: Vec<([u8; 32], Vec<u8>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotMutableCommitWindow {
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub base_generation: u64,
    pub pending_generation: u64,
    pub transaction_count: usize,
    pub record_count: usize,
}

#[derive(Debug, Default)]
pub struct HotMutableCommitQueue {
    next_sequence: u64,
    pending: VecDeque<HotMutableCommit>,
    pending_records: usize,
    waiters: VecDeque<Arc<Waiter>>,
    leader_active: bool,
}

impl HotMutableCommitQueue {
    pub fn enqueue(
        &mut self,
        base_generation: u64,
        durability: StoreDurabilityPolicy,
        records: Vec<([u8; 32], Vec<u8>)>,
    ) -> Result<HotMutableCommitWindow> {
        if durability != StoreDurabilityPolicy::Normal {
            return Err(LoomError::new(
                Code::InvalidArgument,
                "hot mutable commit queue only accepts normal durability transactions",
            ));
        }
        if records.is_empty() {
            return Err(LoomError::new(
                Code::InvalidArgument,
                "hot mutable commit queue transaction is empty",
            ));
        }
        if self.pending.len() >= HOT_MUTABLE_QUEUE_MAX_TRANSACTIONS {
            return Err(LoomError::new(
                Code::ResourceExhausted,
                "hot mutable commit queue transaction limit reached",
            ));
        }
        let pending_records = self
            .pending_records
            .checked_add(records.len())
            .ok_or_else(|| corrupt("hot mutable commit queue record count overflow"))?;
        if pending_records > HOT_MUTABLE_QUEUE_MAX_RECORDS {
            return Err(LoomError::new(
                Code::ResourceExhausted,
                "hot mutable commit queue record limit reached",
            ));
        }
        let sequence = self.next_sequence;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or_else(|| corrupt("hot mutable commit queue sequence overflow"))?;
        let pending_generation = match self.pending.back() {
            Some(commit) => commit
                .pending_generation
                .checked_add(1)
                .ok_or_else(|| corrupt("hot mutable commit queue generation overflow"))?,
            None => base_generation
                .checked_add(1)
                .ok_or_else(|| corrupt("hot mutable commit queue generation overflow"))?,
        };
        self.pending.push_back(HotMutableCommit {
            sequence,
            base_generation,
            pending_generation,
            durability,
            records,
        });
        self.pending_records = pending_records;
        self.pending_window()
            .ok_or_else(|| corrupt("hot mutable commit queue window missing after enqueue"))
    }

    pub fn pending_window(&self) -> Option<HotMutableCommitWindow> {
        let first = self.pending.front()?;
        let last = self.pending.back().expect("front exists");
        Some(HotMutableCommitWindow {
            first_sequence: first.sequence,
            last_sequence: last.sequence,
            base_generation: first.base_generation,
            pending_generation: last.pending_generation,
            transaction_count: self.pending.len(),
            record_count: self.pending_records,
        })
    }

    pub fn drain_ready(&mut self, max_records: usize) -> Vec<HotMutableCommit> {
        self.drain_ready_with_waiters(max_records)
            .into_iter()
            .map(|(commit, _)| commit)
            .collect()
    }

    fn enqueue_with_waiter(
        &mut self,
        base_generation: u64,
        records: Vec<([u8; 32], Vec<u8>)>,
        waiter: Arc<Waiter>,
    ) -> Result<bool> {
        self.enqueue(base_generation, StoreDurabilityPolicy::Normal, records)?;
        self.waiters.push_back(waiter);
        let was_idle = !self.leader_active;
        self.leader_active = true;
        Ok(was_idle)
    }

    fn drain_ready_with_waiters(
        &mut self,
        max_records: usize,
    ) -> Vec<(HotMutableCommit, Option<Arc<Waiter>>)> {
        let mut drained = Vec::new();
        let mut drained_records = 0usize;
        while let Some(next) = self.pending.front() {
            if !drained.is_empty() && drained_records + next.records.len() > max_records {
                break;
            }
            let next = self.pending.pop_front().expect("front exists");
            drained_records += next.records.len();
            self.pending_records -= next.records.len();
            drained.push((next, self.waiters.pop_front()));
        }
        drained
    }

    fn finish_leader_if_empty(&mut self) -> bool {
        if self.pending.is_empty() {
            self.leader_active = false;
            true
        } else {
            false
        }
    }
}

impl FileStore {
    /// Open the `.loom` at `path` for writing, creating it if absent, and recover to the last
    /// committed state. Takes an exclusive advisory lock so only one writer holds the loom at a time;
    /// a second writer gets [`Code::Conflict`]. Native-file-only; the wasm32 build opens over a
    /// caller-supplied backing via [`FileStore::with_backing`] instead.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_inner(path.as_ref().to_path_buf(), true, true)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_daemon_authorized(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_inner(path.as_ref().to_path_buf(), true, false)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_daemon_authorized_with_progress(
        path: impl AsRef<Path>,
        progress: impl FnMut(StoreOpenProgress),
    ) -> Result<Self> {
        Self::open_inner_enc_with_progress(
            path.as_ref().to_path_buf(),
            true,
            false,
            None,
            Algo::Blake3,
            true,
            progress,
        )
    }

    /// Open the `.loom` at `path` read-only and lock-free: many readers can open the same loom
    /// concurrently and they do not exclude a writer. The file must already exist; writes through the
    /// returned handle fail at the OS (the descriptor is read-only). Native-file-only.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_read(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_inner(path.as_ref().to_path_buf(), false, false)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_copy_source(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_inner_enc(
            path.as_ref().to_path_buf(),
            false,
            false,
            None,
            Algo::Blake3,
            false,
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open_inner(path: PathBuf, writable: bool, enforce_daemon_guard: bool) -> Result<Self> {
        // Plain open: an existing store reads its own profile from the superblock; a fresh one created
        // here gets the default (blake3) profile. FIPS stores are created via `create_with_profile`.
        Self::open_inner_enc(
            path,
            writable,
            enforce_daemon_guard,
            None,
            Algo::Blake3,
            true,
        )
    }

    /// Create a fresh `.loom` under an explicit identity profile: `Algo::Blake3` is
    /// the default profile, `Algo::Sha256` the FIPS profile. The profile is immutable once written.
    /// Native-file-only.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn create_with_profile(path: impl AsRef<Path>, digest_algo: Algo) -> Result<Self> {
        let store = Self::open_inner_enc(
            path.as_ref().to_path_buf(),
            true,
            true,
            None,
            digest_algo,
            true,
        )?;
        store.validate_runtime_policy()?;
        Ok(store)
    }

    /// Create a fresh **encrypted** `.loom` at `path`, writing `encryption_meta` (the wrapped DEK + KDF
    /// salt + active suite, from [`loom_core::keys::EncryptionMeta::encode`]) into its superblock and
    /// holding the unlocked `session`. Fails with [`Code::AlreadyExists`] if a non-empty file is already
    /// there: the encryption bit is set only at creation. Native-file-only.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn create_encrypted(
        path: impl AsRef<Path>,
        encryption_meta: Vec<u8>,
        session: loom_core::keys::DekSession,
    ) -> Result<Self> {
        Self::create_encrypted_with_profile(path, encryption_meta, session, Algo::Blake3)
    }

    /// Like [`create_encrypted`](Self::create_encrypted) but under an explicit identity profile (the
    /// digest algorithm). The FIPS profile pairs `Algo::Sha256` with the AES-256-GCM
    /// encryption suite carried in `encryption_meta`. Native-file-only.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn create_encrypted_with_profile(
        path: impl AsRef<Path>,
        encryption_meta: Vec<u8>,
        session: loom_core::keys::DekSession,
        digest_algo: Algo,
    ) -> Result<Self> {
        let store = Self::open_inner_enc(
            path.as_ref().to_path_buf(),
            true,
            true,
            Some(encryption_meta),
            digest_algo,
            true,
        )?;
        *store.dek.lock().map_err(|_| poisoned())? = Some(session);
        store.validate_runtime_policy()?;
        Ok(store)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open_inner_enc(
        path: PathBuf,
        writable: bool,
        enforce_daemon_guard: bool,
        encryption: Option<Vec<u8>>,
        create_digest_algo: Algo,
        load_mutable_overlay: bool,
    ) -> Result<Self> {
        Self::open_inner_enc_with_progress(
            path,
            writable,
            enforce_daemon_guard,
            encryption,
            create_digest_algo,
            load_mutable_overlay,
            |_| {},
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open_inner_enc_with_progress(
        path: PathBuf,
        writable: bool,
        enforce_daemon_guard: bool,
        encryption: Option<Vec<u8>>,
        create_digest_algo: Algo,
        load_mutable_overlay: bool,
        mut progress: impl FnMut(StoreOpenProgress),
    ) -> Result<Self> {
        if writable && enforce_daemon_guard {
            reject_daemon_owned_direct_open(&path)?;
        }
        let reclamation_reader_lease = if writable {
            None
        } else {
            Some(acquire_reclamation_reader_lease(&path)?)
        };
        let file = OpenOptions::new()
            .read(true)
            .write(writable)
            .create(writable)
            .truncate(false)
            .open(&path)
            .map_err(io_err)?;
        // One writer per file: an exclusive advisory lock for this handle's lifetime keeps a second
        // process from racing the superblock. Readers take no lock; the lock releases when the handle
        // is dropped.
        // Measure the wait to take the exclusive advisory lock. The store struct does not
        // exist yet, so the sample is recorded into its metrics once it is constructed below.
        let write_lock_wait = if writable {
            let started = std::time::Instant::now();
            acquire_write_lock(&file)?;
            Some(started.elapsed())
        } else {
            None
        };
        let mut store = Self::open_over_backing_with_progress(
            Box::new(file),
            writable,
            path,
            encryption,
            create_digest_algo,
            load_mutable_overlay,
            &mut progress,
        )?;
        store._reclamation_reader_lease = reclamation_reader_lease;
        if let Some(elapsed) = write_lock_wait {
            store.group_commit_metrics.record_write_lock_wait(elapsed);
        }
        Ok(store)
    }

    /// Open a `FileStore` over a caller-supplied [`BackingIo`] - an in-memory buffer, a browser OPFS
    /// sync handle, or any other block device - instead of a native file. The caller is
    /// responsible for whatever exclusive locking the backing requires (acquiring an OPFS sync handle
    /// is itself exclusive; an in-memory backing needs none). Compaction's atomic file replace is
    /// native-only, so a non-file backing must not call [`FileStore::compact`].
    pub fn with_backing(backing: Box<dyn BackingIo>, writable: bool) -> Result<Self> {
        Self::open_over_backing(backing, writable, PathBuf::new(), None, Algo::Blake3, true)
    }

    /// Create a fresh `FileStore` over a caller-supplied backing under an explicit identity profile
    /// (the browser / in-memory counterpart of [`create_with_profile`](Self::create_with_profile)).
    pub fn with_backing_profile(
        backing: Box<dyn BackingIo>,
        writable: bool,
        digest_algo: Algo,
    ) -> Result<Self> {
        Self::open_over_backing(backing, writable, PathBuf::new(), None, digest_algo, true)
    }

    /// Create a fresh **encrypted** `FileStore` over a caller-supplied backing (the browser / in-memory
    /// counterpart of [`create_encrypted`](Self::create_encrypted)). The backing must be empty.
    pub fn with_backing_encrypted(
        backing: Box<dyn BackingIo>,
        encryption_meta: Vec<u8>,
        session: loom_core::keys::DekSession,
        digest_algo: Algo,
    ) -> Result<Self> {
        let store = Self::open_over_backing(
            backing,
            true,
            PathBuf::new(),
            Some(encryption_meta),
            digest_algo,
            true,
        )?;
        *store.dek.lock().map_err(|_| poisoned())? = Some(session);
        Ok(store)
    }

    /// Recover (or, when empty, initialize) a `FileStore` over `backing`, independent of how the
    /// backing is realized. `path` is used only by native compaction's atomic rename. `encryption` is
    /// `Some` only when **creating** a fresh encrypted store; opening an existing store reads its
    /// encryption metadata from the superblock instead.
    fn open_over_backing(
        backing: Box<dyn BackingIo>,
        writable: bool,
        path: PathBuf,
        encryption: Option<Vec<u8>>,
        // The identity-profile digest algorithm to use when *creating* a fresh store.
        // Opening an existing store ignores this and reads the algorithm from the superblock instead.
        create_digest_algo: Algo,
        load_mutable_overlay: bool,
    ) -> Result<Self> {
        Self::open_over_backing_with_progress(
            backing,
            writable,
            path,
            encryption,
            create_digest_algo,
            load_mutable_overlay,
            &mut |_| {},
        )
    }

    fn open_over_backing_with_progress(
        mut backing: Box<dyn BackingIo>,
        writable: bool,
        path: PathBuf,
        encryption: Option<Vec<u8>>,
        create_digest_algo: Algo,
        load_mutable_overlay: bool,
        progress: &mut impl FnMut(StoreOpenProgress),
    ) -> Result<Self> {
        let len = backing.size().map_err(io_err)?;
        progress(StoreOpenProgress {
            stage: StoreOpenStage::Backing,
            completed: len,
            total: Some(len),
        });

        // The encryption bit is set only at creation: a request to create encrypted over a
        // store that already has data is refused rather than silently opening it unencrypted.
        if encryption.is_some() && len != 0 {
            return Err(LoomError::new(
                Code::AlreadyExists,
                "loom-store: cannot enable encryption on an existing store",
            ));
        }

        if len == 0 {
            if !writable {
                return Err(corrupt("loom is empty or uninitialized"));
            }
            // Fresh file: zero the header region (two superblock slots + the journal slot) so the empty
            // journal slot decodes as "no record", then write both superblock slots at generation 0.
            // The page array starts empty: the file is exactly DATA_START bytes and grows a page at a time.
            write_at(&mut *backing, 0, &vec![0u8; DATA_START as usize]).map_err(io_err)?;
            let sb = Superblock {
                generation: 0,
                page_count: 0,
                digest_algo: create_digest_algo,
                region_table: None,
                reference: None,
                control: None,
                encryption: encryption.clone(),
            }
            .encode();
            write_at(&mut *backing, 0, &sb).map_err(io_err)?;
            write_at(&mut *backing, SLOT_SIZE, &sb).map_err(io_err)?;
            backing.fsync().map_err(io_err)?;
            let store = Self {
                file: Mutex::new(backing),
                inner: Mutex::new(Inner {
                    index: BTreeMap::new(),
                    locator_cache_order: VecDeque::new(),
                    index_page_cache: BTreeMap::new(),
                    index_page_cache_order: VecDeque::new(),
                    io_stats: StoreIoStats {
                        open_index_materialized: true,
                        ..StoreIoStats::default()
                    },
                    index_materialized: true,
                    page_count: 0,
                    generation: 0,
                    reference_root: None,
                    control_root: None,
                    index_root: None,
                    overlay_root: None,
                    current_record_root: None,
                    root_catalog_root: None,
                    root_catalog_entries: Vec::new(),
                    mutable_overlay_generation_floor: 0,
                    minimum_recoverable_generation: 0,
                    retained_history_root: None,
                    owner_token_root: None,
                    secondary_index_root: None,
                    metadata_bootstrap_reserve: MetadataBootstrapReserve::default(),
                    mutable_idempotency_root: None,
                    workflow_idempotency_root: None,
                    audit_retention_root: None,
                    mvcc_generation_root: None,
                    retention_index_root: None,
                    checkpoint_index_root: None,
                    reclaim_index_root: None,
                    freemap: None,
                    region_table_root: None,
                    maintenance_root: None,
                    maintenance: MaintenanceState::default(),
                    active_mark_epoch_reclaim_fence: None,
                    open_segment: 0,
                    free: Vec::new(),
                    encryption_meta: encryption,
                }),
                path,
                #[cfg(not(target_arch = "wasm32"))]
                _reclamation_reader_lease: None,
                default_codec: Codec::Deflate,
                group: Mutex::new(GroupCommit::default()),
                group_commit_metrics: GroupCommitMetrics::default(),
                mutable_overlay: Mutex::new(loom_core::MutableOverlay::new()),
                copy_source_read_view: Mutex::new(None),
                mutable_overlay_enumerations: AtomicU64::new(0),
                mutable_overlay_prefix_enumerations: AtomicU64::new(0),
                mutable_overlay_prefix_entries_returned: AtomicU64::new(0),
                overlay_publication: Mutex::new(()),
                pending_mutable_idempotency: Mutex::new(BTreeMap::new()),
                pending_workflow_idempotency: Mutex::new(BTreeMap::new()),
                mvcc_snapshot_registry: Arc::new(Mutex::new(StoreMvccSnapshotRegistry::default())),
                hot_mutable_queue: Mutex::new(HotMutableCommitQueue::default()),
                maintenance_index_scan: Mutex::new(None),
                dek: Mutex::new(None),
                digest_algo: create_digest_algo,
                #[cfg(test)]
                post_commit_pre_adopt_hook: PostCommitPreAdoptHookSlot::default(),
                #[cfg(test)]
                source_layout_activation_pre_finish_hook:
                    SourceLayoutActivationPreFinishHookSlot::default(),
                #[cfg(test)]
                reachability_epoch_pre_finish_hook: ReachabilityEpochPreFinishHookSlot::default(),
                #[cfg(test)]
                source_layout_preflight_after_discovery_hook:
                    SourceLayoutPreflightAfterDiscoveryHookSlot::default(),
                #[cfg(test)]
                audit_retention_test_instrumentation: AuditRetentionTestInstrumentation::default(),
            };
            if load_mutable_overlay {
                store.load_mutable_overlay_from_storage_with_progress(progress)?;
            }
            return Ok(store);
        }
        if len < DATA_START {
            return Err(corrupt("file too short to hold both superblock slots"));
        }

        // Pick the valid (CRC-ok) superblock with the highest generation.
        let mut a = [0u8; SLOT_SIZE as usize];
        read_exact_at(&mut *backing, 0, &mut a).map_err(io_err)?;
        let mut b = [0u8; SLOT_SIZE as usize];
        read_exact_at(&mut *backing, SLOT_SIZE, &mut b).map_err(io_err)?;
        let mut sb = match (Superblock::decode(&a), Superblock::decode(&b)) {
            (None, None) => return Err(corrupt("no valid superblock")),
            (Some(x), None) => x,
            (None, Some(y)) => y,
            (Some(x), Some(y)) => {
                if y.generation > x.generation {
                    y
                } else {
                    x
                }
            }
        };

        // journal ring recovery: the superblock is only a periodic checkpoint, so scan the ring for
        // the newest durably-journaled commit. A torn record (bad CRC) is skipped, so a crash during
        // the latest commit's journal write falls back to the previous one - the ring's advantage over
        // a single slot. A record's referenced data is durable before its fsync, so a valid record
        // newer than the superblock is the real committed state.
        let mut newest: Option<journal::Roots> = None;
        let mut rbuf = [0u8; journal::RECORD_SIZE];
        for i in 0..RING_SLOTS {
            progress(StoreOpenProgress {
                stage: StoreOpenStage::JournalRecovery,
                completed: i,
                total: Some(RING_SLOTS),
            });
            let off = JOURNAL_OFFSET + i * journal::RECORD_SIZE as u64;
            if read_exact_at(&mut *backing, off, &mut rbuf).is_ok()
                && let Some((journal::KIND_COMMIT, jr)) = journal::decode(&rbuf)
                && newest.is_none_or(|n| jr.generation > n.generation)
            {
                newest = Some(jr);
            }
        }
        progress(StoreOpenProgress {
            stage: StoreOpenStage::JournalRecovery,
            completed: RING_SLOTS,
            total: Some(RING_SLOTS),
        });
        if let Some(jr) = newest
            && jr.generation > sb.generation
        {
            sb = Superblock {
                generation: jr.generation,
                page_count: jr.page_count,
                // The journal `Roots` carries neither the digest profile nor the encryption_meta (both
                // immutable); preserve them from the checkpoint superblock slot we just decoded.
                digest_algo: sb.digest_algo,
                region_table: jr.region_table,
                reference: jr.reference,
                control: jr.control,
                encryption: sb.encryption.clone(),
            };
            if writable {
                // Fold the recovered state into a superblock (checkpoint on open) so the next open is
                // cheap and the ring scan stays bounded.
                let cp_slot = ((sb.generation / CHECKPOINT_INTERVAL) & 1) * SLOT_SIZE;
                let enc = sb.encode();
                write_at(&mut *backing, cp_slot, &enc).map_err(io_err)?;
                backing.fsync().map_err(io_err)?;
            }
        }

        // The committed page array must be wholly present; a shorter file means a committed generation
        // was truncated away - a clean CORRUPT error, never a silent fall back to an older generation.
        if len < DATA_START + sb.page_count * PAGE_SIZE {
            return Err(corrupt(
                "committed data truncated: file shorter than the page array",
            ));
        }

        // Read the region table the superblock points at. Object lookups use bounded B-tree reads from
        // the index root; heavyweight maintenance paths materialize the full map explicitly.
        let (
            index_root,
            overlay_root,
            current_record_root,
            root_catalog_root,
            freemap_root,
            maintenance_root,
            open_segment,
            mutable_overlay_generation_floor,
            minimum_recoverable_generation,
            metadata_bootstrap_reserve,
        ) = match sb.region_table {
            Some(rt) => {
                progress(StoreOpenProgress {
                    stage: StoreOpenStage::RegionTable,
                    completed: 0,
                    total: Some(1),
                });
                let region =
                    read_canonical_region_table(&mut *backing, rt, sb.page_count, sb.generation);
                #[cfg(test)]
                let region = match region {
                    Ok(region) => Ok(region),
                    Err(error) if legacy_free_map_promotion_destination(&path) => {
                        let _ = error;
                        read_region_table(&mut *backing, rt, sb.page_count)
                    }
                    Err(error) => Err(error),
                };
                let region = region?;
                progress(StoreOpenProgress {
                    stage: StoreOpenStage::RegionTable,
                    completed: 1,
                    total: Some(1),
                });
                (
                    region.index_root,
                    region.overlay_root,
                    region.current_record_root,
                    region.root_catalog_root,
                    region.freemap_root,
                    region.maintenance_root,
                    region.open_segment,
                    region.mutable_overlay_generation_floor,
                    region.minimum_recoverable_generation,
                    region.metadata_bootstrap_reserve,
                )
            }
            None => (
                None,
                None,
                None,
                None,
                None,
                None,
                0,
                0,
                0,
                MetadataBootstrapReserve::default(),
            ),
        };
        let root_catalog_entries = match root_catalog_root {
            Some(root) => {
                progress(StoreOpenProgress {
                    stage: StoreOpenStage::RootCatalog,
                    completed: 0,
                    total: Some(1),
                });
                let entries = read_root_catalog(&mut *backing, root, sb.page_count)?.entries;
                progress(StoreOpenProgress {
                    stage: StoreOpenStage::RootCatalog,
                    completed: 1,
                    total: Some(1),
                });
                entries
            }
            None => Vec::new(),
        };
        let family_roots = root_catalog_family_roots(&root_catalog_entries);
        let mut index = BTreeMap::new();
        let mut index_materialized = false;

        // Restore the persisted free-page map (consistent with the recovered generation) so reuse of
        // reclaimed pages survives the restart rather than starting empty.
        let (free, freemap) = match freemap_root {
            Some(root) => {
                progress(StoreOpenProgress {
                    stage: StoreOpenStage::FreeMap,
                    completed: 0,
                    total: Some(1),
                });
                let decoded = pagemap::read_map_with_root_span(
                    &mut *backing,
                    DATA_START,
                    root,
                    sb.page_count,
                );
                #[cfg(test)]
                let decoded = match decoded {
                    Ok(decoded) => Ok((decoded.0, Some((root, decoded.1)))),
                    Err(error) if legacy_free_map_promotion_destination(&path) => {
                        let inventory = pagemap::read_legacy_recordloc_map_for_promotion(
                            &mut *backing,
                            DATA_START,
                            root,
                            sb.page_count,
                        )?;
                        pagemap::record_legacy_promotion_inventory(&inventory)?;
                        let retired_generation = sb.generation.saturating_add(1);
                        let mut runs = inventory.runs;
                        runs.extend(
                            inventory
                                .tree_pages
                                .into_iter()
                                .chain(inventory.blob_pages)
                                .map(|page| FreePageRun {
                                    start: page,
                                    len: 1,
                                    freed_gen: retired_generation,
                                }),
                        );
                        let _ = error;
                        Ok((runs, None))
                    }
                    Err(error) => Err(error),
                };
                #[cfg(not(test))]
                let decoded = decoded.map(|(runs, span)| (runs, Some((root, span))));
                let (runs, freemap) = decoded?;
                progress(StoreOpenProgress {
                    stage: StoreOpenStage::FreeMap,
                    completed: 1,
                    total: Some(1),
                });
                (runs, freemap)
            }
            None => (Vec::new(), None),
        };
        for extent in &metadata_bootstrap_reserve.extents {
            let extent_end = extent.start.saturating_add(extent.len);
            if free.iter().any(|run| {
                run.start < extent_end && extent.start < run.start.saturating_add(run.len)
            }) {
                return Err(corrupt(
                    "metadata bootstrap reserve overlaps the canonical free map",
                ));
            }
        }
        let mut maintenance = match maintenance_root {
            Some(root) => read_maintenance(&mut *backing, root, sb.page_count)?,
            None => MaintenanceState::default(),
        };
        if !maintenance.object_count_known {
            if let Some(root) = index_root {
                let mut pages = 0u64;
                for (key, loc) in pagebtree::load_all_with_progress(
                    &mut *backing,
                    DATA_START,
                    root,
                    sb.page_count,
                    |advanced| {
                        pages = pages.saturating_add(advanced);
                        progress(StoreOpenProgress {
                            stage: StoreOpenStage::Index,
                            completed: pages,
                            total: None,
                        });
                    },
                )? {
                    index.insert(key, loc);
                }
                maintenance.object_count = index.len() as u64;
                maintenance.object_count_known = true;
                index_materialized = true;
            } else {
                maintenance.object_count = 0;
                maintenance.object_count_known = true;
            }
        }

        let store = Self {
            file: Mutex::new(backing),
            inner: Mutex::new(Inner {
                index,
                locator_cache_order: VecDeque::new(),
                index_page_cache: BTreeMap::new(),
                index_page_cache_order: VecDeque::new(),
                io_stats: StoreIoStats {
                    locator_cache_entries: if index_materialized {
                        maintenance.object_count
                    } else {
                        0
                    },
                    open_index_materialized: index_materialized,
                    ..StoreIoStats::default()
                },
                index_materialized,
                page_count: sb.page_count,
                generation: sb.generation,
                // The reference root is addressed under the store's own identity profile, not always
                // blake3, so reconstruct its algorithm from the superblock.
                reference_root: sb.reference.map(|b| Digest::of(sb.digest_algo, b)),
                control_root: sb.control.map(|b| Digest::of(sb.digest_algo, b)),
                index_root,
                overlay_root,
                current_record_root,
                root_catalog_root,
                root_catalog_entries,
                mutable_overlay_generation_floor,
                minimum_recoverable_generation,
                retained_history_root: family_roots.retained_history,
                owner_token_root: family_roots.owner_token,
                secondary_index_root: family_roots.secondary_index,
                mutable_idempotency_root: family_roots.mutable_idempotency,
                workflow_idempotency_root: family_roots.workflow_idempotency,
                audit_retention_root: family_roots.audit_retention,
                mvcc_generation_root: family_roots.mvcc_generation,
                retention_index_root: family_roots.retention_index,
                checkpoint_index_root: family_roots.checkpoint_index,
                reclaim_index_root: family_roots.reclaim_index,
                freemap,
                region_table_root: sb.region_table,
                maintenance_root,
                maintenance,
                active_mark_epoch_reclaim_fence: None,
                open_segment,
                free,
                metadata_bootstrap_reserve,
                encryption_meta: sb.encryption,
            }),
            path,
            #[cfg(not(target_arch = "wasm32"))]
            _reclamation_reader_lease: None,
            default_codec: Codec::Deflate,
            group: Mutex::new(GroupCommit::default()),
            group_commit_metrics: GroupCommitMetrics::default(),
            mutable_overlay: Mutex::new(loom_core::MutableOverlay::new()),
            copy_source_read_view: Mutex::new(None),
            mutable_overlay_enumerations: AtomicU64::new(0),
            mutable_overlay_prefix_enumerations: AtomicU64::new(0),
            mutable_overlay_prefix_entries_returned: AtomicU64::new(0),
            overlay_publication: Mutex::new(()),
            pending_mutable_idempotency: Mutex::new(BTreeMap::new()),
            pending_workflow_idempotency: Mutex::new(BTreeMap::new()),
            mvcc_snapshot_registry: Arc::new(Mutex::new(StoreMvccSnapshotRegistry::default())),
            hot_mutable_queue: Mutex::new(HotMutableCommitQueue::default()),
            maintenance_index_scan: Mutex::new(None),
            dek: Mutex::new(None),
            digest_algo: sb.digest_algo,
            #[cfg(test)]
            post_commit_pre_adopt_hook: PostCommitPreAdoptHookSlot::default(),
            #[cfg(test)]
            source_layout_activation_pre_finish_hook:
                SourceLayoutActivationPreFinishHookSlot::default(),
            #[cfg(test)]
            reachability_epoch_pre_finish_hook: ReachabilityEpochPreFinishHookSlot::default(),
            #[cfg(test)]
            source_layout_preflight_after_discovery_hook:
                SourceLayoutPreflightAfterDiscoveryHookSlot::default(),
            #[cfg(test)]
            audit_retention_test_instrumentation: AuditRetentionTestInstrumentation::default(),
        };
        if let Some(epoch) = store.active_reachability_mark_epoch()? {
            store.set_active_reachability_mark_epoch_reclaim_fence(Some(
                epoch.page_high_water_mark,
            ))?;
        }
        if load_mutable_overlay {
            store.load_mutable_overlay_from_storage_with_progress(progress)?;
        }
        Ok(store)
    }

    /// The codec attempted for newly written object records. The size and shrink guardrails still
    /// apply per object, so incompressible or tiny payloads are stored identity regardless. Reads are
    /// self-describing (the frame id is in each record), so changing this is always safe and affects
    /// only subsequent writes.
    pub fn set_default_codec(&mut self, codec: Codec) {
        self.default_codec = codec;
    }

    pub fn prepare_copy_source_read_view(&self) -> Result<()> {
        let preflight = self.source_layout_replacement_preflight()?;
        let plan =
            if preflight.disposition == SourceLayoutReplacementPreflightDisposition::LegacyReady {
                Some(self.source_layout_migration_plan()?)
            } else {
                None
            };
        let historical_index = if plan.is_some() {
            let (root, page_count) = {
                let inner = self.inner.lock().map_err(|_| poisoned())?;
                (inner.index_root, inner.page_count)
            };
            match root {
                Some(root) => {
                    let mut file = self.file.lock().map_err(|_| poisoned())?;
                    #[cfg(test)]
                    COPY_SOURCE_READ_VIEW_MATERIALIZATIONS.fetch_add(1, Ordering::Relaxed);
                    Some(pagebtree::load_all(
                        &mut **file,
                        DATA_START,
                        root,
                        page_count,
                    )?)
                }
                None => Some(Vec::new()),
            }
        } else {
            None
        };
        if let Some(plan) = plan {
            self.validate_source_layout_migration_plan(&plan)?;
            let mut entries = Vec::new();
            for record in &plan.current_records {
                entries.push(decode_mutable_overlay_entry(&record.bytes)?);
            }
            entries.sort_by_key(|entry| entry.generation);
            let mut overlay = loom_core::MutableOverlay::import_entries(&entries)?;
            overlay.set_generation_floor(plan.source_identity.generation);
            *self.mutable_overlay.lock().map_err(|_| poisoned())? = overlay;
        } else {
            self.load_mutable_overlay_from_storage()?;
        }
        *self.copy_source_read_view.lock().map_err(|_| poisoned())? =
            Some(CopySourceReadView { historical_index });
        Ok(())
    }

    /// The engine-state (reference) root digest recorded in the committed superblock, if any.
    pub fn reference_root(&self) -> Option<Digest> {
        self.inner.lock().ok().and_then(|i| i.reference_root)
    }

    pub fn mutable_overlay_health(&self) -> Result<loom_core::MutableOverlayHealth> {
        self.mutable_overlay
            .lock()
            .map_err(|_| poisoned())?
            .health()
    }

    pub fn mutable_overlay_snapshot(&self) -> Result<loom_core::OverlaySnapshot> {
        Ok(self
            .mutable_overlay
            .lock()
            .map_err(|_| poisoned())?
            .snapshot())
    }

    pub fn open_mvcc_snapshot(&self) -> Result<StoreMvccSnapshot> {
        self.open_mvcc_snapshot_with_owner(None)
    }

    pub fn open_mvcc_snapshot_with_owner(&self, owner: Option<&str>) -> Result<StoreMvccSnapshot> {
        let _publication_guard = self.overlay_publication.lock().map_err(|_| poisoned())?;
        let immutable_base_root = self.inner.lock().map_err(|_| poisoned())?.reference_root;
        let snapshot = self
            .mutable_overlay
            .lock()
            .map_err(|_| poisoned())?
            .snapshot();
        self.register_mvcc_snapshot(snapshot, immutable_base_root, owner)
    }

    fn register_mvcc_snapshot(
        &self,
        snapshot: loom_core::OverlaySnapshot,
        immutable_base_root: Option<Digest>,
        owner: Option<&str>,
    ) -> Result<StoreMvccSnapshot> {
        let identity = StoreMvccSnapshotIdentity {
            overlay_generation: snapshot.generation(),
            immutable_base_root,
        };
        let owner = owner.map(str::to_owned);
        let mut registry = self.mvcc_snapshot_registry.lock().map_err(|_| poisoned())?;
        registry.next_pin_id = registry
            .next_pin_id
            .checked_add(1)
            .ok_or_else(|| corrupt("MVCC snapshot pin id overflow"))?;
        let pin_id = registry.next_pin_id;
        registry.pins.insert(
            pin_id,
            StoreMvccSnapshotPin {
                pin_id,
                identity,
                owner,
            },
        );
        Ok(StoreMvccSnapshot {
            pin_id,
            identity,
            snapshot,
            registry: Arc::clone(&self.mvcc_snapshot_registry),
            released: AtomicBool::new(false),
        })
    }

    pub fn mvcc_snapshot_diagnostics(&self) -> Result<StoreMvccSnapshotDiagnostics> {
        let registry = self.mvcc_snapshot_registry.lock().map_err(|_| poisoned())?;
        let pins = registry.pins.values().cloned().collect::<Vec<_>>();
        let oldest_pinned_overlay_generation =
            pins.iter().map(|pin| pin.identity.overlay_generation).min();
        Ok(StoreMvccSnapshotDiagnostics {
            active_snapshot_count: pins.len() as u64,
            oldest_pinned_overlay_generation,
            pins,
        })
    }

    pub fn oldest_pinned_mvcc_snapshot_generation(
        &self,
    ) -> Result<Option<loom_core::OverlayGeneration>> {
        Ok(self
            .mvcc_snapshot_diagnostics()?
            .oldest_pinned_overlay_generation)
    }

    pub fn mutable_overlay_entries(&self) -> Result<Vec<loom_core::MutableOverlayEntrySnapshot>> {
        self.mutable_overlay_entries_with_progress(|_, _| {})
    }

    pub fn mutable_overlay_entries_with_progress(
        &self,
        progress: impl FnMut(u64, u64),
    ) -> Result<Vec<loom_core::MutableOverlayEntrySnapshot>> {
        self.mutable_overlay_enumerations
            .fetch_add(1, Ordering::Relaxed);
        self.mutable_overlay
            .lock()
            .map_err(|_| poisoned())?
            .export_entries_with_progress(progress)
    }

    pub fn mutable_overlay_entries_with_prefix(
        &self,
        key_prefix: &loom_core::OverlayKeyPrefix,
    ) -> Result<Vec<loom_core::MutableOverlayEntrySnapshot>> {
        self.mutable_overlay_prefix_enumerations
            .fetch_add(1, Ordering::Relaxed);
        let entries = self
            .mutable_overlay
            .lock()
            .map_err(|_| poisoned())?
            .export_entries_with_key_prefix(key_prefix)?;
        self.mutable_overlay_prefix_entries_returned
            .fetch_add(entries.len() as u64, Ordering::Relaxed);
        Ok(entries)
    }

    pub fn mutable_overlay_enumeration_count(&self) -> u64 {
        self.mutable_overlay_enumerations.load(Ordering::Relaxed)
    }

    pub fn mutable_overlay_prefix_enumeration_count(&self) -> u64 {
        self.mutable_overlay_prefix_enumerations
            .load(Ordering::Relaxed)
    }

    pub fn mutable_overlay_prefix_entries_returned_count(&self) -> u64 {
        self.mutable_overlay_prefix_entries_returned
            .load(Ordering::Relaxed)
    }

    #[cfg(test)]
    fn reset_copy_source_read_view_test_counters() {
        COPY_SOURCE_READ_VIEW_CLONES.store(0, Ordering::Relaxed);
        COPY_SOURCE_READ_VIEW_MATERIALIZATIONS.store(0, Ordering::Relaxed);
    }

    #[cfg(test)]
    fn copy_source_read_view_test_counters() -> (u64, u64) {
        (
            COPY_SOURCE_READ_VIEW_CLONES.load(Ordering::Relaxed),
            COPY_SOURCE_READ_VIEW_MATERIALIZATIONS.load(Ordering::Relaxed),
        )
    }

    pub fn mutable_overlay_current_entry(
        &self,
        key: &loom_core::OverlayKey,
    ) -> Result<Option<loom_core::MutableOverlayEntrySnapshot>> {
        self.mutable_overlay
            .lock()
            .map_err(|_| poisoned())?
            .current_entry(key)
            .map_or(Ok(None), |entry| Ok(Some(entry)))
    }

    pub fn mutable_overlay_generation(&self) -> Result<loom_core::OverlayGeneration> {
        Ok(self
            .mutable_overlay
            .lock()
            .map_err(|_| poisoned())?
            .generation())
    }

    pub fn mutable_overlay_owner_token(
        &self,
        key: &loom_core::OverlayKey,
    ) -> Result<Option<loom_core::OverlayOwnerToken>> {
        Ok(self
            .mutable_overlay
            .lock()
            .map_err(|_| poisoned())?
            .current_entry(key)
            .map(|entry| entry.owner_token))
    }

    pub fn hot_mutable_commit_window(&self) -> Result<Option<HotMutableCommitWindow>> {
        Ok(self
            .hot_mutable_queue
            .lock()
            .map_err(|_| poisoned())?
            .pending_window())
    }

    pub fn put_mutable_overlay_value(
        &self,
        key: loom_core::OverlayKey,
        payload: Vec<u8>,
    ) -> Result<loom_core::OverlayOwnerToken> {
        let mut tokens = self.put_mutable_overlay_values(vec![(key, payload)])?;
        tokens
            .pop()
            .ok_or_else(|| corrupt("mutable overlay batch returned no owner token"))
    }

    pub fn put_mutable_overlay_value_idempotent(
        &self,
        key: loom_core::OverlayKey,
        payload: Vec<u8>,
        idempotency_key: &str,
    ) -> Result<loom_core::OverlayOwnerToken> {
        let _publication_guard = self.overlay_publication.lock().map_err(|_| poisoned())?;
        validate_mutable_overlay_idempotency_key(idempotency_key)?;
        let request_digest = mutable_overlay_idempotency_request_digest(&key, &payload);
        let durability = self.mutable_overlay_key_durability(&key)?;
        if durability != StoreDurabilityPolicy::Ephemeral
            && let Some(record) = self.mutable_overlay_idempotency_record(idempotency_key)?
        {
            if record.request_digest == request_digest {
                return Ok(record.owner_token);
            }
            return Err(LoomError::new(
                Code::Conflict,
                "mutable overlay idempotency key was already used with a different payload",
            ));
        }
        let pending = {
            self.pending_mutable_idempotency
                .lock()
                .map_err(|_| poisoned())?
                .get(idempotency_key)
                .cloned()
        };
        if let Some(pending) = pending {
            if pending.request_digest != request_digest {
                return Err(LoomError::new(
                    Code::Conflict,
                    "mutable overlay idempotency key was already used with a different payload",
                ));
            }
            drop(_publication_guard);
            self.finish_normal_mutable_publish(pending.waiter, false)?;
            return Ok(pending.owner_token);
        }
        let mut overlay = self.mutable_overlay.lock().map_err(|_| poisoned())?;
        let owner_token = overlay.snapshot().owner_token(&key)?;
        let token = overlay.put_value(key.clone(), owner_token.as_ref(), payload)?;
        let latest = overlay
            .export_entries()?
            .into_iter()
            .rev()
            .find(|entry| entry.key == key)
            .ok_or_else(|| corrupt("mutable overlay idempotent write missing current entry"))?;
        drop(overlay);
        let records = vec![
            (
                mutable_overlay_entry_address(&key),
                encode_mutable_overlay_entry(&latest),
            ),
            (
                mutable_overlay_owner_token_address(&key),
                encode_mutable_overlay_owner_token_record(&token),
            ),
            (
                mutable_overlay_idempotency_address(idempotency_key),
                encode_mutable_overlay_idempotency_record(&request_digest, &token),
            ),
        ];
        if durability == StoreDurabilityPolicy::Normal {
            let (waiter, lead) = self.enqueue_normal_mutable_records(records)?;
            self.pending_mutable_idempotency
                .lock()
                .map_err(|_| poisoned())?
                .insert(
                    idempotency_key.to_string(),
                    PendingMutableIdempotency {
                        request_digest,
                        owner_token: token.clone(),
                        waiter: Arc::clone(&waiter),
                    },
                );
            drop(_publication_guard);
            let outcome = self.finish_normal_mutable_publish(waiter, lead);
            let _publication_guard = self.overlay_publication.lock().map_err(|_| poisoned())?;
            self.pending_mutable_idempotency
                .lock()
                .map_err(|_| poisoned())?
                .remove(idempotency_key);
            outcome?;
        } else {
            self.publish_mutable_overlay_records(durability, records)?;
        }
        Ok(token)
    }

    fn publish_mutable_overlay_records(
        &self,
        durability: StoreDurabilityPolicy,
        records: Vec<([u8; 32], Vec<u8>)>,
    ) -> Result<()> {
        match durability {
            StoreDurabilityPolicy::Normal => self.publish_normal_mutable_records(records),
            StoreDurabilityPolicy::Strict => {
                self.publish_hot_mutable_queue()?;
                self.commit_mutable_overlay_records(&records)
            }
            StoreDurabilityPolicy::Relaxed => self.commit_mutable_overlay_records(&records),
            StoreDurabilityPolicy::Ephemeral => Ok(()),
        }
    }

    fn enqueue_normal_mutable_records(
        &self,
        records: Vec<([u8; 32], Vec<u8>)>,
    ) -> Result<(Arc<Waiter>, bool)> {
        let waiter = Arc::new(Waiter {
            outcome: Mutex::new(None),
            cv: Condvar::new(),
        });
        let generation = self.inner.lock().map_err(|_| poisoned())?.generation;
        let lead = self
            .hot_mutable_queue
            .lock()
            .map_err(|_| poisoned())?
            .enqueue_with_waiter(generation, records, Arc::clone(&waiter))?;
        Ok((waiter, lead))
    }

    fn finish_normal_mutable_publish(&self, waiter: Arc<Waiter>, lead: bool) -> Result<()> {
        if lead {
            loop {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    for _ in 0..4 {
                        let ready = {
                            let queue = self.hot_mutable_queue.lock().map_err(|_| poisoned())?;
                            queue.pending.len() > 1
                        };
                        if ready {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_micros(250));
                    }
                }
                let batch = {
                    let mut queue = self.hot_mutable_queue.lock().map_err(|_| poisoned())?;
                    if queue.finish_leader_if_empty() {
                        break;
                    }
                    queue.drain_ready_with_waiters(HOT_MUTABLE_QUEUE_MAX_RECORDS)
                };
                let waiters = batch
                    .iter()
                    .filter_map(|(_, waiter)| waiter.clone())
                    .collect::<Vec<_>>();
                let batch_transactions = batch.len() as u64;
                let records = batch
                    .into_iter()
                    .flat_map(|(commit, _)| commit.records)
                    .collect::<Vec<_>>();
                // One measurement per drained batch (never per record).
                self.group_commit_metrics
                    .record_batch(batch_transactions, records.len() as u64);
                let outcome = self.commit_mutable_overlay_records(&records);
                for waiter in waiters {
                    let mut slot = waiter.outcome.lock().unwrap_or_else(|p| p.into_inner());
                    *slot = Some(outcome.clone());
                    waiter.cv.notify_all();
                }
            }
        }

        let mut slot = waiter.outcome.lock().map_err(|_| poisoned())?;
        loop {
            if let Some(outcome) = slot.as_ref() {
                return outcome.clone();
            }
            slot = waiter.cv.wait(slot).map_err(|_| poisoned())?;
        }
    }

    fn publish_normal_mutable_records(&self, records: Vec<([u8; 32], Vec<u8>)>) -> Result<()> {
        let (waiter, lead) = self.enqueue_normal_mutable_records(records)?;
        self.finish_normal_mutable_publish(waiter, lead)
    }

    fn publish_hot_mutable_queue(&self) -> Result<()> {
        loop {
            let batch = {
                let mut queue = self.hot_mutable_queue.lock().map_err(|_| poisoned())?;
                if queue.pending.is_empty() {
                    queue.leader_active = false;
                    break;
                }
                queue.drain_ready_with_waiters(HOT_MUTABLE_QUEUE_MAX_RECORDS)
            };
            let waiters = batch
                .iter()
                .filter_map(|(_, waiter)| waiter.clone())
                .collect::<Vec<_>>();
            let batch_transactions = batch.len() as u64;
            let records = batch
                .into_iter()
                .flat_map(|(commit, _)| commit.records)
                .collect::<Vec<_>>();
            // One measurement per drained batch (never per record).
            self.group_commit_metrics
                .record_batch(batch_transactions, records.len() as u64);
            let outcome = self.commit_mutable_overlay_records(&records);
            for waiter in waiters {
                let mut slot = waiter.outcome.lock().unwrap_or_else(|p| p.into_inner());
                *slot = Some(outcome.clone());
                waiter.cv.notify_all();
            }
            outcome?;
        }
        Ok(())
    }

    fn mutable_overlay_records_durability(
        &self,
        durabilities: &[StoreDurabilityPolicy],
    ) -> StoreDurabilityPolicy {
        if durabilities.contains(&StoreDurabilityPolicy::Strict) {
            StoreDurabilityPolicy::Strict
        } else if durabilities.contains(&StoreDurabilityPolicy::Normal) {
            StoreDurabilityPolicy::Normal
        } else if durabilities.contains(&StoreDurabilityPolicy::Relaxed) {
            StoreDurabilityPolicy::Relaxed
        } else {
            StoreDurabilityPolicy::Ephemeral
        }
    }

    pub fn flush_hot_mutable_commits(&self) -> Result<()> {
        let _publication_guard = self.overlay_publication.lock().map_err(|_| poisoned())?;
        self.publish_hot_mutable_queue()
    }

    #[cfg(test)]
    fn enqueue_hot_mutable_commit_for_test(
        &self,
        records: Vec<([u8; 32], Vec<u8>)>,
    ) -> Result<HotMutableCommitWindow> {
        let generation = self.inner.lock().map_err(|_| poisoned())?.generation;
        self.hot_mutable_queue
            .lock()
            .map_err(|_| poisoned())?
            .enqueue(generation, StoreDurabilityPolicy::Normal, records)
    }

    #[cfg(test)]
    fn publish_mutable_overlay_records_for_test(
        &self,
        durability: StoreDurabilityPolicy,
        records: Vec<([u8; 32], Vec<u8>)>,
    ) -> Result<()> {
        let _publication_guard = self.overlay_publication.lock().map_err(|_| poisoned())?;
        self.publish_mutable_overlay_records(durability, records)
    }

    pub fn put_mutable_overlay_values(
        &self,
        entries: Vec<(loom_core::OverlayKey, Vec<u8>)>,
    ) -> Result<Vec<loom_core::OverlayOwnerToken>> {
        if entries.is_empty() {
            return Ok(Vec::new());
        }
        let publication_guard = self.overlay_publication.lock().map_err(|_| poisoned())?;
        let entry_durabilities = entries
            .iter()
            .map(|(key, _)| self.mutable_overlay_key_durability(key))
            .collect::<Result<Vec<_>>>()?;
        let mut overlay = self.mutable_overlay.lock().map_err(|_| poisoned())?;
        let mut keys = Vec::new();
        let mut tokens = Vec::new();
        for ((key, payload), durability) in entries.into_iter().zip(entry_durabilities) {
            let owner_token = overlay.current_entry(&key).map(|entry| entry.owner_token);
            let token = overlay.put_value(key.clone(), owner_token.as_ref(), payload)?;
            keys.push((key, durability));
            tokens.push(token);
        }
        let mut records = Vec::new();
        let mut record_durabilities = Vec::new();
        for (key, durability) in keys {
            if durability == StoreDurabilityPolicy::Ephemeral {
                continue;
            }
            let latest = overlay
                .current_entry(&key)
                .ok_or_else(|| corrupt("mutable overlay write missing current entry"))?;
            records.push((
                mutable_overlay_entry_address(&key),
                encode_mutable_overlay_entry(&latest),
            ));
            records.push((
                mutable_overlay_owner_token_address(&key),
                encode_mutable_overlay_owner_token_record(&latest.owner_token),
            ));
            record_durabilities.push(durability);
        }
        drop(overlay);
        let durability = self.mutable_overlay_records_durability(&record_durabilities);
        if durability == StoreDurabilityPolicy::Normal {
            let (waiter, lead) = self.enqueue_normal_mutable_records(records)?;
            drop(publication_guard);
            self.finish_normal_mutable_publish(waiter, lead)?;
        } else {
            self.publish_mutable_overlay_records(durability, records)?;
        }
        Ok(tokens)
    }

    fn mutable_overlay_key_durability(
        &self,
        key: &loom_core::OverlayKey,
    ) -> Result<StoreDurabilityPolicy> {
        let policy = self.store_policy()?;
        Ok(mutable_overlay_key_facet(key)?
            .map(|facet| policy.effective_durability(facet))
            .unwrap_or(policy.default_durability))
    }

    fn validate_resolved_workflow_durability(
        &self,
        txn: &WorkflowTransaction,
        resolved: StoreDurabilityPolicy,
        write_durabilities: &[StoreDurabilityPolicy],
    ) -> Result<()> {
        if txn.idempotency.is_some() && resolved == StoreDurabilityPolicy::Ephemeral {
            return Err(WorkflowTransactionErrorKind::UnhonoredDurabilityPolicy
                .into_error("ephemeral workflow transaction idempotency cannot be honored"));
        }
        if resolved != StoreDurabilityPolicy::Ephemeral
            && write_durabilities.contains(&StoreDurabilityPolicy::Ephemeral)
        {
            return Err(WorkflowTransactionErrorKind::UnhonoredDurabilityPolicy
                .into_error("ephemeral write cannot join a stronger single transaction"));
        }
        Ok(())
    }

    pub fn commit_workflow_transaction(&self, txn: WorkflowTransaction) -> Result<CommitReceipt> {
        txn.validate()?;
        let publication_guard = self.overlay_publication.lock().map_err(|_| poisoned())?;
        if let Some(generation) = txn.expected_generation {
            let current = self
                .mutable_overlay
                .lock()
                .map_err(|_| poisoned())?
                .generation();
            if current != generation {
                return Err(WorkflowTransactionErrorKind::RetryableStaleGeneration
                    .into_error("workflow transaction overlay generation is stale"));
            }
        }
        let policy = self.store_policy()?;
        let write_durabilities = txn
            .writes
            .iter()
            .map(|write| workflow_write_durability(&txn, &policy, write))
            .collect::<Vec<_>>();
        let resolved_durability = self.mutable_overlay_records_durability(&write_durabilities);
        self.validate_resolved_workflow_durability(&txn, resolved_durability, &write_durabilities)?;
        let request_digest = workflow_transaction_request_digest(&txn, &write_durabilities);
        if let Some(idempotency) = txn.idempotency.as_ref()
            && let Some(receipt) =
                self.workflow_transaction_idempotency_record(idempotency.as_bytes())?
        {
            if receipt.request_digest == request_digest {
                return Ok(receipt.receipt);
            }
            return Err(WorkflowTransactionErrorKind::DuplicateIdempotencyKey
                .into_error("workflow transaction idempotency key was already used"));
        }
        let pending = txn.idempotency.as_ref().map_or(Ok(None), |idempotency| {
            self.pending_workflow_idempotency
                .lock()
                .map_err(|_| poisoned())
                .map(|pending| pending.get(idempotency.as_bytes()).cloned())
        })?;
        if let Some(pending) = pending {
            if pending.request_digest != request_digest {
                return Err(WorkflowTransactionErrorKind::DuplicateIdempotencyKey
                    .into_error("workflow transaction idempotency key was already used"));
            }
            drop(publication_guard);
            self.finish_normal_mutable_publish(pending.waiter, false)?;
            let mut receipt = pending.receipt;
            receipt.replayed = true;
            return Ok(receipt);
        }
        #[cfg(any(test, feature = "test-hooks"))]
        observe_store_publication(&self.path, StorePublicationTestEvent::WorkflowTransaction);
        let mut overlay = self.mutable_overlay.lock().map_err(|_| poisoned())?;
        let before_snapshot = overlay.snapshot();
        let mut outcomes = Vec::with_capacity(txn.writes.len());
        let mut writes = Vec::with_capacity(txn.writes.len());
        let overlay_writes = txn
            .writes
            .iter()
            .map(|write| {
                (
                    write.target.clone(),
                    write.expected.as_ref().map(|token| token.0.clone()),
                    write.op.entry_kind(),
                    match &write.op {
                        loom_core::FacetWriteOp::Put { payload } => payload.clone(),
                        loom_core::FacetWriteOp::Delete => Vec::new(),
                    },
                )
            })
            .collect::<Vec<_>>();
        let tokens = match overlay.put_entries_in_next_generation(overlay_writes) {
            Ok(tokens) => tokens,
            Err(error) => {
                *overlay = loom_core::MutableOverlay::fork_from_snapshot(before_snapshot.clone());
                return Err(error);
            }
        };
        for ((write, durability), token) in txn
            .writes
            .iter()
            .zip(write_durabilities.iter().copied())
            .zip(tokens)
        {
            outcomes.push(WriteOutcome {
                facet: write.facet,
                target: write.target.clone(),
                owner_token: token,
                change: write.op.entry_kind(),
            });
            writes.push((
                write.target.clone(),
                durability,
                write.secondary_indexes.clone(),
            ));
        }
        let overlay_generation = overlay.generation();
        let mut records = Vec::new();
        let mut record_durabilities = Vec::new();
        for (key, durability, secondary_indexes) in &writes {
            if *durability == StoreDurabilityPolicy::Ephemeral {
                continue;
            }
            let latest = overlay
                .current_entry(key)
                .ok_or_else(|| corrupt("workflow transaction write missing current entry"))?;
            records.push((
                mutable_overlay_entry_address(key),
                encode_mutable_overlay_entry(&latest),
            ));
            records.push((
                mutable_overlay_owner_token_address(key),
                encode_mutable_overlay_owner_token_record(&latest.owner_token),
            ));
            for index_write in secondary_indexes {
                records.push((
                    mutable_overlay_secondary_index_address(&index_write.index),
                    encode_mutable_overlay_secondary_index_record(overlay_generation, index_write),
                ));
            }
            record_durabilities.push(*durability);
        }
        drop(overlay);
        let root_after = workflow_transaction_root_digest(self.digest_algo, overlay_generation);
        let mut receipt = CommitReceipt {
            generation: overlay_generation,
            root_after,
            writes: outcomes,
            operation_identities: txn
                .prepared_operations
                .iter()
                .map(|operation| operation.operation_id.clone())
                .collect(),
            revision_identities: txn
                .revision_metadata
                .iter()
                .map(|revision| loom_core::RevisionReceipt {
                    entity_id: revision.entity_id.clone(),
                    revision_id: revision.revision_id.clone(),
                })
                .collect(),
            audit_sequences: Vec::new(),
            retained_sequences: Vec::new(),
            delivery_receipts: txn
                .delivery_intents
                .iter()
                .map(|delivery| loom_core::DeliveryReceipt {
                    stream_id: delivery.stream_id.clone(),
                    sequence: delivery.sequence,
                    envelope_id: delivery.envelope_id.clone(),
                    payload_digest: delivery.payload_digest,
                })
                .collect(),
            post_commit_delta: txn
                .post_commit_delta
                .as_ref()
                .map(loom_core::PostCommitDeltaReceipt::from),
            replayed: false,
        };
        if txn.owner_state.is_empty()
            && let Some(idempotency) = txn.idempotency.as_ref()
            && !records.is_empty()
        {
            records.push((
                mutable_overlay_transaction_idempotency_address(idempotency.as_bytes()),
                encode_workflow_transaction_idempotency_record(&request_digest, &receipt)?,
            ));
            record_durabilities.push(resolved_durability);
        }
        let durability = self.mutable_overlay_records_durability(&record_durabilities);
        let publish = if txn.owner_state.is_empty() {
            if durability == StoreDurabilityPolicy::Normal {
                let (waiter, lead) = self.enqueue_normal_mutable_records(records)?;
                let pending_key = txn.idempotency.as_ref().map(|key| key.as_bytes().to_vec());
                if let Some(key) = pending_key.as_ref() {
                    self.pending_workflow_idempotency
                        .lock()
                        .map_err(|_| poisoned())?
                        .insert(
                            key.clone(),
                            PendingWorkflowIdempotency {
                                request_digest,
                                receipt: receipt.clone(),
                                waiter: Arc::clone(&waiter),
                            },
                        );
                }
                drop(publication_guard);
                let outcome = self.finish_normal_mutable_publish(waiter, lead);
                let _publication_guard = self.overlay_publication.lock().map_err(|_| poisoned())?;
                if let Some(key) = pending_key {
                    self.pending_workflow_idempotency
                        .lock()
                        .map_err(|_| poisoned())?
                        .remove(&key);
                }
                outcome
            } else {
                self.publish_mutable_overlay_records(durability, records)
            }
        } else {
            self.publish_hot_mutable_queue()?;
            let idempotency_record =
                txn.idempotency
                    .as_ref()
                    .map(|idempotency| WorkflowIdempotencyCommitRecord {
                        key: idempotency.as_bytes().to_vec(),
                        request_digest,
                        receipt: receipt.clone(),
                    });
            self.commit_workflow_owner_state_records(&records, &txn.owner_state, idempotency_record)
                .map(|owner_receipt| {
                    receipt.audit_sequences = owner_receipt.audit_sequences;
                    receipt.retained_sequences = owner_receipt.retained_sequences;
                })
        };
        if let Err(error) = publish {
            *self.mutable_overlay.lock().map_err(|_| poisoned())? =
                loom_core::MutableOverlay::fork_from_snapshot(before_snapshot);
            return Err(error);
        }
        Ok(receipt)
    }

    fn commit_workflow_owner_state_records(
        &self,
        records: &[([u8; 32], Vec<u8>)],
        owner_state: &loom_core::WorkflowOwnerState,
        idempotency_record: Option<WorkflowIdempotencyCommitRecord>,
    ) -> Result<SavedStateAndAuditReceipt> {
        let mut retained_heads = BTreeMap::<Vec<u8>, u64>::new();
        let mut records = records.to_vec();
        let mut retained_sequences = Vec::new();
        for write in &owner_state.controls {
            let loom_core::WorkflowControlWrite::AppendRetained {
                key,
                expected_next_sequence,
                records: appended,
            } = write
            else {
                continue;
            };
            let current = match retained_heads.get(key) {
                Some(sequence) => *sequence,
                None => self.retained_history_head(key)?,
            };
            let actual_next = current
                .checked_add(1)
                .ok_or_else(|| LoomError::invalid("retained-history sequence overflow"))?;
            if actual_next != *expected_next_sequence {
                return Err(LoomError::new(
                    Code::Conflict,
                    format!(
                        "retained-history expected sequence {expected_next_sequence}, current next sequence is {actual_next}"
                    ),
                ));
            }
            let mut sequence = actual_next;
            for payload in appended {
                records.push((
                    retained_history_record_address(key, sequence),
                    encode_retained_history_entry(key, sequence, payload),
                ));
                sequence = sequence
                    .checked_add(1)
                    .ok_or_else(|| LoomError::invalid("retained-history sequence overflow"))?;
            }
            let head = sequence - 1;
            retained_sequences.push(loom_core::RetainedSequenceReceipt {
                key: key.clone(),
                first_sequence: *expected_next_sequence,
                last_sequence: head,
            });
            records.push((
                retained_history_head_address(key),
                encode_retained_history_head(key, head),
            ));
            retained_heads.insert(key.clone(), head);
        }
        let oldest_pinned_snapshot_generation = self
            .oldest_pinned_mvcc_snapshot_generation()?
            .map(|generation| generation.as_u64());
        let audit_retention_active = self.audit_config()?.legal_hold;
        let mut control_puts = BTreeMap::<Vec<u8>, Vec<u8>>::new();
        let mut control_deletes = BTreeSet::<Vec<u8>>::new();
        let mut audit_delta = AuditRetentionDelta::default();
        for write in &owner_state.controls {
            match write {
                loom_core::WorkflowControlWrite::Put { key, payload } => {
                    if is_audit_retention_control_key(key) {
                        audit_delta.put(key, payload.clone());
                    } else {
                        control_deletes.remove(key);
                        control_puts.insert(key.clone(), payload.clone());
                    }
                }
                loom_core::WorkflowControlWrite::Delete { key } => {
                    if is_audit_retention_control_key(key) {
                        audit_delta.delete(key.clone());
                    } else {
                        control_puts.remove(key);
                        control_deletes.insert(key.clone());
                    }
                }
                loom_core::WorkflowControlWrite::AppendRetained { .. } => {}
            }
        }
        let mut audit_sequences = Vec::with_capacity(owner_state.audits.len());
        for audit in &owner_state.audits {
            audit_sequences.push(self.append_audit_record_delta(
                &mut audit_delta,
                audit.principal,
                &audit.action,
                audit.target.as_deref(),
            )?);
        }
        if let Some(mut idempotency) = idempotency_record {
            idempotency.receipt.audit_sequences = audit_sequences.clone();
            idempotency.receipt.retained_sequences = retained_sequences.clone();
            records.push((
                mutable_overlay_transaction_idempotency_address(&idempotency.key),
                encode_workflow_transaction_idempotency_record(
                    &idempotency.request_digest,
                    &idempotency.receipt,
                )?,
            ));
        }
        let records = records
            .into_iter()
            .collect::<BTreeMap<_, _>>()
            .into_iter()
            .collect::<Vec<_>>();
        let needs_legacy_audit_migration = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            inner.audit_retention_root.is_none() && !audit_delta.is_empty()
        };
        let mut control_map = self.control_root_map()?;
        let audit_map = if needs_legacy_audit_migration {
            let map = control_map.clone();
            let (control, mut audit) = split_audit_retention_control_map(map);
            control_map = control;
            apply_audit_retention_delta(&mut audit, &audit_delta);
            Some(audit)
        } else {
            None
        };
        control_map.retain(|key, _| !is_audit_retention_control_key(key));
        for key in control_deletes {
            control_map.remove(&key);
        }
        for (key, value) in control_puts {
            control_map.insert(key, value);
        }

        let mut inner = self.inner.lock().map_err(|_| poisoned())?;
        let publication_authority =
            self.begin_foreground_transaction_publication(&inner, control_map)?;
        let owner_objects = owner_state.objects.clone();
        let reference = match owner_state.reference {
            loom_core::WorkflowReferenceUpdate::Keep => {
                inner.reference_root.map(|digest| *digest.bytes())
            }
            loom_core::WorkflowReferenceUpdate::Set(root) => root.map(|digest| *digest.bytes()),
        };
        let mut seen = BTreeSet::new();
        let mut fresh = Vec::new();
        for (digest, canonical) in &owner_objects {
            if Digest::hash(self.digest_algo, canonical) != *digest {
                return Err(LoomError::integrity_failure(
                    "workflow owner-state object digest does not match payload",
                ));
            }
            if seen.insert(*digest.bytes())
                && self
                    .lookup_loc_locked(&mut inner, digest.bytes())?
                    .is_none()
            {
                fresh.push((*digest, canonical.as_slice(), self.default_codec));
            }
        }
        let new_gen = inner.generation + 1;
        let (roots, object_placements) = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            let prepared = self.prepare_foreground_transaction_publication(
                &mut **file,
                &inner,
                ForegroundMutationInput::WorkflowOwnerState,
                &publication_authority,
                |file, alloc| {
                    let mut current_records = BTreeMap::<[u8; 32], Vec<u8>>::new();
                    let mut retained_records = BTreeMap::<[u8; 32], Vec<u8>>::new();
                    let mut owner_token_records = BTreeMap::<[u8; 32], Vec<u8>>::new();
                    let mut secondary_index_records = BTreeMap::<[u8; 32], Vec<u8>>::new();
                    let mut mutable_idempotency_records = BTreeMap::<[u8; 32], Vec<u8>>::new();
                    let mut workflow_idempotency_records = BTreeMap::<[u8; 32], Vec<u8>>::new();
                    let mut legacy_records = BTreeMap::<[u8; 32], Vec<u8>>::new();
                    let legacy_overlay_root_before = legacy_overlay_root_for_publication(
                        &inner,
                        inner.current_record_root,
                        inner.root_catalog_root,
                    );
                    {
                        let mut classify_record = |address: [u8; 32],
                                                   value: Vec<u8>|
                         -> Result<()> {
                            if is_mutable_overlay_current_entry_record(&value) {
                                current_records.insert(address, value);
                            } else if value.starts_with(RETAINED_HISTORY_HEAD_RECORD)
                                || value.starts_with(RETAINED_HISTORY_ENTRY_RECORD)
                            {
                                retained_records.insert(address, value);
                            } else if value.starts_with(MUTABLE_OVERLAY_OWNER_TOKEN_RECORD) {
                                owner_token_records.insert(address, value);
                            } else if value.starts_with(MUTABLE_OVERLAY_SECONDARY_INDEX_RECORD) {
                                secondary_index_records.insert(address, value);
                            } else if value.starts_with(MUTABLE_OVERLAY_IDEMPOTENCY_RECORD) {
                                mutable_idempotency_records.insert(address, value);
                            } else if value
                                .starts_with(MUTABLE_OVERLAY_TRANSACTION_IDEMPOTENCY_RECORD)
                            {
                                workflow_idempotency_records.insert(address, value);
                            } else {
                                legacy_records.insert(address, value);
                            }
                            Ok(())
                        };
                        if let Some(root) = legacy_overlay_root_before {
                            for (address, loc) in
                                pagebtree::load_all(file, DATA_START, root, inner.page_count)?
                            {
                                if address == mutable_overlay_meta_address()
                                    || address == mutable_overlay_current_root_address()
                                {
                                    continue;
                                }
                                classify_record(address, read_blob_from_loc(file, loc)?)?;
                            }
                        }
                        for (address, value) in &records {
                            classify_record(*address, value.clone())?;
                        }
                    }
                    let mutable_overlay_generation_floor =
                        mutable_overlay_generation_floor_from_current_records(
                            inner.mutable_overlay_generation_floor,
                            current_records.values().map(Vec::as_slice),
                        )?;
                    let current_root_before = read_mutable_overlay_current_record_root(
                        file,
                        inner.current_record_root,
                        legacy_overlay_root_before,
                        inner.page_count,
                    )?;
                    let current_record_refs = current_records
                        .iter()
                        .map(|(address, value)| (*address, value.as_slice()))
                        .collect::<Vec<_>>();
                    let mut reclaimed = BTreeSet::new();
                    if let Some(root) = legacy_overlay_root_before {
                        pagebtree::free_all(file, DATA_START, alloc, root, inner.page_count)?;
                    }
                    let legacy_record_refs = legacy_records
                        .iter()
                        .map(|(address, value)| (*address, value.as_slice()))
                        .collect::<Vec<_>>();
                    let (legacy_overlay_root, legacy_reclaimed) =
                        write_mutable_record_refs_to_root(
                            file,
                            alloc,
                            None,
                            inner.page_count,
                            &legacy_record_refs,
                            None,
                            false,
                        )?;
                    reclaimed.extend(legacy_reclaimed);
                    let retained_record_refs = retained_records
                        .iter()
                        .map(|(address, value)| (*address, value.as_slice()))
                        .collect::<Vec<_>>();
                    let owner_token_record_refs = owner_token_records
                        .iter()
                        .map(|(address, value)| (*address, value.as_slice()))
                        .collect::<Vec<_>>();
                    let secondary_index_record_refs = secondary_index_records
                        .iter()
                        .map(|(address, value)| (*address, value.as_slice()))
                        .collect::<Vec<_>>();
                    let mutable_idempotency_record_refs = mutable_idempotency_records
                        .iter()
                        .map(|(address, value)| (*address, value.as_slice()))
                        .collect::<Vec<_>>();
                    let workflow_idempotency_record_refs = workflow_idempotency_records
                        .iter()
                        .map(|(address, value)| (*address, value.as_slice()))
                        .collect::<Vec<_>>();
                    let family_outcome = write_root_family_record_batches(
                        file,
                        alloc,
                        inner.page_count,
                        &[
                            RootFamilyRecordBatch {
                                family_id: CURRENT_RECORDS_FAMILY_ID,
                                root: current_root_before,
                                records: &current_record_refs,
                            },
                            RootFamilyRecordBatch {
                                family_id: RETAINED_HISTORY_FAMILY_ID,
                                root: inner.retained_history_root,
                                records: &retained_record_refs,
                            },
                            RootFamilyRecordBatch {
                                family_id: OWNER_TOKEN_FAMILY_ID,
                                root: inner.owner_token_root,
                                records: &owner_token_record_refs,
                            },
                            RootFamilyRecordBatch {
                                family_id: SECONDARY_INDEX_FAMILY_ID,
                                root: inner.secondary_index_root,
                                records: &secondary_index_record_refs,
                            },
                            RootFamilyRecordBatch {
                                family_id: MUTABLE_IDEMPOTENCY_FAMILY_ID,
                                root: inner.mutable_idempotency_root,
                                records: &mutable_idempotency_record_refs,
                            },
                            RootFamilyRecordBatch {
                                family_id: WORKFLOW_IDEMPOTENCY_FAMILY_ID,
                                root: inner.workflow_idempotency_root,
                                records: &workflow_idempotency_record_refs,
                            },
                        ],
                        root_catalog_family_root(
                            &inner.root_catalog_entries,
                            DELTA_PACK_CANDIDATE_FAMILY_ID,
                        ),
                        new_gen,
                        self.digest_algo,
                        false,
                        oldest_pinned_snapshot_generation,
                        audit_retention_active,
                    )?;
                    reclaimed.extend(&family_outcome.touched_segments);
                    let family_roots = &family_outcome.roots;
                    let current_root = family_roots[&CURRENT_RECORDS_FAMILY_ID];
                    let retained_history_root = family_roots[&RETAINED_HISTORY_FAMILY_ID];
                    let owner_token_root = family_roots[&OWNER_TOKEN_FAMILY_ID];
                    let secondary_index_root = family_roots[&SECONDARY_INDEX_FAMILY_ID];
                    let mutable_idempotency_root = family_roots[&MUTABLE_IDEMPOTENCY_FAMILY_ID];
                    let workflow_idempotency_root = family_roots[&WORKFLOW_IDEMPOTENCY_FAMILY_ID];
                    let audit_retention_root = if let Some(audit_map) = &audit_map {
                        write_audit_retention_map_to_root(
                            file,
                            alloc,
                            inner.audit_retention_root,
                            inner.page_count,
                            audit_map,
                        )?
                    } else if audit_delta.is_empty() {
                        inner.audit_retention_root
                    } else {
                        write_audit_retention_delta_to_root(
                            file,
                            alloc,
                            inner.audit_retention_root,
                            inner.page_count,
                            &audit_delta,
                            #[cfg(test)]
                            Some(&self.audit_retention_test_instrumentation),
                        )?
                    };
                    let root_catalog_entries = root_catalog_entries_with_advisory_family(
                        &root_catalog_entries_with_family(
                            &root_catalog_entries_with_family(
                                &root_catalog_entries_with_family(
                                    &root_catalog_entries_with_family(
                                        &root_catalog_entries_with_family(
                                            &root_catalog_entries_with_family(
                                                &inner.root_catalog_entries,
                                                RETAINED_HISTORY_FAMILY_ID,
                                                retained_history_root,
                                            ),
                                            OWNER_TOKEN_FAMILY_ID,
                                            owner_token_root,
                                        ),
                                        SECONDARY_INDEX_FAMILY_ID,
                                        secondary_index_root,
                                    ),
                                    MUTABLE_IDEMPOTENCY_FAMILY_ID,
                                    mutable_idempotency_root,
                                ),
                                WORKFLOW_IDEMPOTENCY_FAMILY_ID,
                                workflow_idempotency_root,
                            ),
                            AUDIT_RETENTION_FAMILY_ID,
                            audit_retention_root,
                        ),
                        DELTA_PACK_CANDIDATE_FAMILY_ID,
                        family_outcome.delta_pack_candidate_root,
                    );
                    let root_catalog_root = write_root_catalog_page(
                        file,
                        alloc,
                        inner.root_catalog_root,
                        inner.page_count,
                        &root_catalog_entries,
                    )?;
                    let dek = self.dek.lock().map_err(|_| poisoned())?;
                    let mut object_placements =
                        write_record_pages(file, alloc, &fresh, dek.as_ref())?;
                    drop(dek);
                    let index_batch = pagebtree::batch_upsert(
                        file,
                        DATA_START,
                        alloc,
                        inner.index_root,
                        &object_placements,
                        inner.page_count,
                    )?;
                    #[cfg(any(test, feature = "test-hooks"))]
                    observe_object_index_batch(index_batch.stats);
                    let prepared_finalization = self.prepare_foreground_transaction_finalization(
                        file,
                        &inner,
                        &*alloc,
                        &publication_authority,
                        index_batch.root,
                    )?;
                    let finalization = self.apply_foreground_transaction_finalization(
                        file,
                        alloc,
                        index_batch.root,
                        prepared_finalization,
                    )?;
                    let index_root = finalization.index_root;
                    let mut touched_segments = object_placements
                        .iter()
                        .map(|(_, loc)| loc.segment_id)
                        .collect::<BTreeSet<_>>();
                    if let Some(placement) = finalization.fresh_control_placement {
                        touched_segments.insert(placement.1.segment_id);
                        object_placements.push(placement);
                    }
                    touched_segments.extend(reclaimed);
                    let object_count = inner
                        .maintenance
                        .object_count
                        .saturating_add(object_placements.len() as u64);
                    #[cfg(any(test, feature = "test-hooks"))]
                    invoke_store_publication_failure_test_injector(
                        &self.path,
                        StorePublicationFailureTestBoundary::WorkflowOwnerStateCommit,
                    )?;
                    let publication = finish_foreground_txn_on_planning_backing(
                        file,
                        alloc,
                        new_gen,
                        object_count,
                        TxnRootInputs {
                            object_index: index_root,
                            legacy_overlay: legacy_overlay_root,
                            current_records: current_root,
                            root_catalog: TxnRootCatalog {
                                root: root_catalog_root,
                                entries: root_catalog_entries.clone(),
                            },
                            previous_mutable_overlay_generation_floor: inner
                                .mutable_overlay_generation_floor,
                            mutable_overlay_generation_floor,
                            reference,
                            control: finalization.control,
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
                        finalization.free_map_publication,
                    )?;
                    Ok(PreparedForegroundTransactionOutcome {
                        publication,
                        value: object_placements,
                    })
                },
            )?;
            self.finish_foreground_txn(&mut **file, &inner, prepared)?
        };
        self.adopt_committed_roots_locked(&mut inner, roots)?;
        for (key, loc) in object_placements {
            Self::cache_locator_locked(&mut inner, key, loc);
        }
        Ok(SavedStateAndAuditReceipt {
            audit_sequences,
            retained_sequences,
        })
    }

    pub fn put_mutable_overlay_tombstone(
        &self,
        key: loom_core::OverlayKey,
    ) -> Result<loom_core::OverlayOwnerToken> {
        let publication_guard = self.overlay_publication.lock().map_err(|_| poisoned())?;
        let durability = self.mutable_overlay_key_durability(&key)?;
        let mut overlay = self.mutable_overlay.lock().map_err(|_| poisoned())?;
        let owner_token = overlay.current_entry(&key).map(|entry| entry.owner_token);
        let token = overlay.put_tombstone(key.clone(), owner_token.as_ref())?;
        let latest = overlay
            .current_entry(&key)
            .ok_or_else(|| corrupt("mutable overlay tombstone missing current entry"))?;
        drop(overlay);
        let records = vec![
            (
                mutable_overlay_entry_address(&key),
                encode_mutable_overlay_entry(&latest),
            ),
            (
                mutable_overlay_owner_token_address(&key),
                encode_mutable_overlay_owner_token_record(&token),
            ),
        ];
        if durability == StoreDurabilityPolicy::Normal {
            let (waiter, lead) = self.enqueue_normal_mutable_records(records)?;
            drop(publication_guard);
            self.finish_normal_mutable_publish(waiter, lead)?;
        } else {
            self.publish_mutable_overlay_records(durability, records)?;
        }
        Ok(token)
    }

    pub fn mutable_overlay_durable_owner_token(
        &self,
        key: &loom_core::OverlayKey,
    ) -> Result<Option<loom_core::OverlayOwnerToken>> {
        self.mutable_overlay_owner_token_record(&mutable_overlay_owner_token_address(key))
    }

    fn mutable_overlay_owner_token_record(
        &self,
        address: &[u8; 32],
    ) -> Result<Option<loom_core::OverlayOwnerToken>> {
        self.owner_token_record_payload(address)?
            .map(|bytes| decode_mutable_overlay_owner_token_record(&bytes))
            .transpose()
    }

    fn mutable_overlay_idempotency_record(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<MutableOverlayIdempotencyRecord>> {
        self.mutable_idempotency_record_payload(&mutable_overlay_idempotency_address(
            idempotency_key,
        ))?
        .map(|bytes| decode_mutable_overlay_idempotency_record(&bytes))
        .transpose()
    }

    pub fn mutable_overlay_secondary_index_value(
        &self,
        index: &loom_core::OverlayKey,
    ) -> Result<Option<Vec<u8>>> {
        let Some(bytes) =
            self.secondary_index_record_payload(&mutable_overlay_secondary_index_address(index))?
        else {
            return Ok(None);
        };
        let record = decode_mutable_overlay_secondary_index_record(&bytes)?;
        if record.index != *index {
            return Err(corrupt("mutable overlay secondary-index key mismatch"));
        }
        let _generation = record.generation;
        Ok(match record.kind {
            loom_core::OverlayEntryKind::Value => record.payload,
            loom_core::OverlayEntryKind::Tombstone => None,
        })
    }

    pub fn retained_history_head(&self, key: &[u8]) -> Result<u64> {
        let Some(bytes) =
            self.retained_history_record_payload(&retained_history_head_address(key))?
        else {
            return Ok(0);
        };
        let (stored_key, sequence) = decode_retained_history_head(&bytes)?;
        if stored_key != key {
            return Err(corrupt("retained-history head key mismatch"));
        }
        Ok(sequence)
    }

    pub fn retained_history_records(
        &self,
        key: &[u8],
        first_sequence: u64,
        max: usize,
    ) -> Result<Vec<Vec<u8>>> {
        if first_sequence == 0 {
            return Err(LoomError::invalid(
                "retained-history sequence must start at one",
            ));
        }
        if max == 0 {
            return Ok(Vec::new());
        }
        let head = self.retained_history_head(key)?;
        if first_sequence > head {
            return Ok(Vec::new());
        }
        let available = head - first_sequence + 1;
        let count = available.min(max as u64);
        let mut records = Vec::with_capacity(count as usize);
        for sequence in first_sequence..first_sequence.saturating_add(count) {
            let bytes = self
                .retained_history_record_payload(&retained_history_record_address(key, sequence))?
                .ok_or_else(|| corrupt("retained-history record is missing before head"))?;
            let (stored_key, stored_sequence, payload) = decode_retained_history_entry(&bytes)?;
            if stored_key != key || stored_sequence != sequence {
                return Err(corrupt("retained-history record identity mismatch"));
            }
            records.push(payload);
        }
        Ok(records)
    }

    fn workflow_transaction_idempotency_record(
        &self,
        idempotency_key: &[u8],
    ) -> Result<Option<WorkflowTransactionIdempotencyRecord>> {
        self.workflow_idempotency_record_payload(&mutable_overlay_transaction_idempotency_address(
            idempotency_key,
        ))?
        .map(|bytes| decode_workflow_transaction_idempotency_record(&bytes))
        .transpose()
    }

    #[cfg(test)]
    fn mutable_overlay_record_payload(&self, address: &[u8; 32]) -> Result<Option<Vec<u8>>> {
        let (current_record_root, overlay_root, page_count) = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            (
                inner.current_record_root,
                inner.overlay_root,
                inner.page_count,
            )
        };
        let loc = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            let current_loc = root_family_get(
                &mut **file,
                CURRENT_RECORDS_FAMILY_ID,
                current_record_root,
                address,
                page_count,
            )?;
            match current_loc {
                Some(loc) => Some(loc),
                None => pagebtree::get(&mut **file, DATA_START, overlay_root, address, page_count)?,
            }
        };
        let Some(loc) = loc else {
            return Ok(None);
        };
        self.read_blob_at_loc(loc, page_count).map(Some)
    }

    fn retained_history_record_payload(&self, address: &[u8; 32]) -> Result<Option<Vec<u8>>> {
        let (retained_history_root, overlay_root, page_count) = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            (
                inner.retained_history_root,
                inner.overlay_root,
                inner.page_count,
            )
        };
        let loc = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            match retained_history_root {
                Some(root) => root_family_get(
                    &mut **file,
                    RETAINED_HISTORY_FAMILY_ID,
                    Some(root),
                    address,
                    page_count,
                )?,
                None => pagebtree::get(&mut **file, DATA_START, overlay_root, address, page_count)?,
            }
        };
        let Some(loc) = loc else {
            return Ok(None);
        };
        self.read_blob_at_loc(loc, page_count).map(Some)
    }

    fn owner_token_record_payload(&self, address: &[u8; 32]) -> Result<Option<Vec<u8>>> {
        let (owner_token_root, overlay_root, page_count) = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            (inner.owner_token_root, inner.overlay_root, inner.page_count)
        };
        let loc = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            match owner_token_root {
                Some(root) => root_family_get(
                    &mut **file,
                    OWNER_TOKEN_FAMILY_ID,
                    Some(root),
                    address,
                    page_count,
                )?,
                None => pagebtree::get(&mut **file, DATA_START, overlay_root, address, page_count)?,
            }
        };
        let Some(loc) = loc else {
            return Ok(None);
        };
        self.read_blob_at_loc(loc, page_count).map(Some)
    }

    fn secondary_index_record_payload(&self, address: &[u8; 32]) -> Result<Option<Vec<u8>>> {
        let (secondary_index_root, overlay_root, page_count) = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            (
                inner.secondary_index_root,
                inner.overlay_root,
                inner.page_count,
            )
        };
        let loc = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            match secondary_index_root {
                Some(root) => root_family_get(
                    &mut **file,
                    SECONDARY_INDEX_FAMILY_ID,
                    Some(root),
                    address,
                    page_count,
                )?,
                None => pagebtree::get(&mut **file, DATA_START, overlay_root, address, page_count)?,
            }
        };
        let Some(loc) = loc else {
            return Ok(None);
        };
        self.read_blob_at_loc(loc, page_count).map(Some)
    }

    fn mutable_idempotency_record_payload(&self, address: &[u8; 32]) -> Result<Option<Vec<u8>>> {
        let (mutable_idempotency_root, overlay_root, page_count) = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            (
                inner.mutable_idempotency_root,
                inner.overlay_root,
                inner.page_count,
            )
        };
        let loc = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            match mutable_idempotency_root {
                Some(root) => root_family_get(
                    &mut **file,
                    MUTABLE_IDEMPOTENCY_FAMILY_ID,
                    Some(root),
                    address,
                    page_count,
                )?,
                None => pagebtree::get(&mut **file, DATA_START, overlay_root, address, page_count)?,
            }
        };
        let Some(loc) = loc else {
            return Ok(None);
        };
        self.read_blob_at_loc(loc, page_count).map(Some)
    }

    fn workflow_idempotency_record_payload(&self, address: &[u8; 32]) -> Result<Option<Vec<u8>>> {
        let (workflow_idempotency_root, overlay_root, page_count) = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            (
                inner.workflow_idempotency_root,
                inner.overlay_root,
                inner.page_count,
            )
        };
        let loc = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            match workflow_idempotency_root {
                Some(root) => root_family_get(
                    &mut **file,
                    WORKFLOW_IDEMPOTENCY_FAMILY_ID,
                    Some(root),
                    address,
                    page_count,
                )?,
                None => pagebtree::get(&mut **file, DATA_START, overlay_root, address, page_count)?,
            }
        };
        let Some(loc) = loc else {
            return Ok(None);
        };
        self.read_blob_at_loc(loc, page_count).map(Some)
    }

    fn audit_retention_record_payload(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let (audit_retention_root, page_count) = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            (inner.audit_retention_root, inner.page_count)
        };
        let Some(root) = audit_retention_root else {
            return Ok(self.control_root_map()?.get(key).cloned());
        };
        let address = audit_retention_record_address(key);
        let loc = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            root_family_get(
                &mut **file,
                AUDIT_RETENTION_FAMILY_ID,
                Some(root),
                &address,
                page_count,
            )?
        };
        let Some(loc) = loc else {
            return Ok(None);
        };
        let bytes = self.read_blob_at_loc(loc, page_count)?;
        let (stored_key, payload) = decode_audit_retention_record(&bytes)?;
        if stored_key != key {
            return Err(corrupt("audit-retention record key mismatch"));
        }
        Ok(Some(payload))
    }

    fn audit_retention_map(&self) -> Result<BTreeMap<Vec<u8>, Vec<u8>>> {
        let (audit_retention_root, page_count) = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            (inner.audit_retention_root, inner.page_count)
        };
        let Some(root) = audit_retention_root else {
            return Ok(self
                .control_root_map()?
                .into_iter()
                .filter(|(key, _)| is_audit_retention_control_key(key))
                .collect());
        };
        let mut out = BTreeMap::new();
        {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            #[cfg(test)]
            self.audit_retention_test_instrumentation
                .full_family_enumerations
                .fetch_add(1, Ordering::SeqCst);
            for (_, loc) in
                root_family_load_all(&mut **file, AUDIT_RETENTION_FAMILY_ID, root, page_count)?
            {
                let bytes = read_blob_from_loc(&mut **file, loc)?;
                let (key, value) = decode_audit_retention_record(&bytes)?;
                if !is_audit_retention_control_key(&key) {
                    return Err(corrupt("audit-retention record key outside family"));
                }
                if out.insert(key, value).is_some() {
                    return Err(corrupt("duplicate audit-retention record key"));
                }
            }
        }
        Ok(out)
    }

    #[cfg(test)]
    fn mvcc_generation_record(
        &self,
        generation: loom_core::OverlayGeneration,
    ) -> Result<Option<MvccGenerationRecord>> {
        let (mvcc_generation_root, page_count) = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            (inner.mvcc_generation_root, inner.page_count)
        };
        let Some(root) = mvcc_generation_root else {
            return Ok(None);
        };
        let address = mvcc_generation_record_address(generation);
        let loc = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            root_family_get(
                &mut **file,
                MVCC_GENERATION_FAMILY_ID,
                Some(root),
                &address,
                page_count,
            )?
        };
        let Some(loc) = loc else {
            return Ok(None);
        };
        let bytes = self.read_blob_at_loc(loc, page_count)?;
        let record = decode_mvcc_generation_record(&bytes)?;
        if record.generation != generation {
            return Err(corrupt("mvcc-generation record key mismatch"));
        }
        Ok(Some(record))
    }

    #[cfg(test)]
    fn retention_index_record(
        &self,
        target: &loom_core::OverlayKey,
    ) -> Result<Option<RetentionIndexRecord>> {
        let (retention_index_root, page_count) = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            (inner.retention_index_root, inner.page_count)
        };
        let Some(root) = retention_index_root else {
            return Ok(None);
        };
        let address = retention_index_record_address(target);
        let loc = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            root_family_get(
                &mut **file,
                RETENTION_INDEX_FAMILY_ID,
                Some(root),
                &address,
                page_count,
            )?
        };
        let Some(loc) = loc else {
            return Ok(None);
        };
        let bytes = self.read_blob_at_loc(loc, page_count)?;
        let record = decode_retention_index_record(&bytes)?;
        if record.target != *target {
            return Err(corrupt("retention-index record key mismatch"));
        }
        Ok(Some(record))
    }

    #[cfg(test)]
    fn checkpoint_index_record(
        &self,
        checkpoint_id: &[u8],
    ) -> Result<Option<CheckpointIndexRecord>> {
        let (checkpoint_index_root, page_count) = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            (inner.checkpoint_index_root, inner.page_count)
        };
        let Some(root) = checkpoint_index_root else {
            return Ok(None);
        };
        let address = checkpoint_index_record_address(checkpoint_id);
        let loc = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            root_family_get(
                &mut **file,
                CHECKPOINT_INDEX_FAMILY_ID,
                Some(root),
                &address,
                page_count,
            )?
        };
        let Some(loc) = loc else {
            return Ok(None);
        };
        let bytes = self.read_blob_at_loc(loc, page_count)?;
        let record = decode_checkpoint_index_record(&bytes)?;
        if record.checkpoint_id != checkpoint_id {
            return Err(corrupt("checkpoint-index record key mismatch"));
        }
        Ok(Some(record))
    }

    #[cfg(test)]
    fn reclaim_index_record(&self, reclaim_key: &[u8]) -> Result<Option<ReclaimIndexRecord>> {
        let (reclaim_index_root, page_count) = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            (inner.reclaim_index_root, inner.page_count)
        };
        let Some(root) = reclaim_index_root else {
            return Ok(None);
        };
        let address = reclaim_index_record_address(reclaim_key);
        let loc = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            root_family_get(
                &mut **file,
                RECLAIM_INDEX_FAMILY_ID,
                Some(root),
                &address,
                page_count,
            )?
        };
        let Some(loc) = loc else {
            return Ok(None);
        };
        let bytes = self.read_blob_at_loc(loc, page_count)?;
        let record = decode_reclaim_index_record(&bytes)?;
        if record.reclaim_key != reclaim_key {
            return Err(corrupt("reclaim-index record key mismatch"));
        }
        Ok(Some(record))
    }

    #[cfg(test)]
    fn delta_pack_advisory_record(
        &self,
        advisory_key: &[u8],
    ) -> Result<Option<DeltaPackAdvisoryRecord>> {
        let (delta_pack_candidate_root, page_count) = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            (
                inner
                    .root_catalog_entries
                    .iter()
                    .find(|entry| entry.family_id == DELTA_PACK_CANDIDATE_FAMILY_ID)
                    .map(|entry| entry.root),
                inner.page_count,
            )
        };
        let Some(root) = delta_pack_candidate_root else {
            return Ok(None);
        };
        let address = delta_pack_advisory_record_address(advisory_key);
        let loc = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            root_family_get(
                &mut **file,
                DELTA_PACK_CANDIDATE_FAMILY_ID,
                Some(root),
                &address,
                page_count,
            )?
        };
        let Some(loc) = loc else {
            return Ok(None);
        };
        let bytes = self.read_blob_at_loc(loc, page_count)?;
        let record = decode_delta_pack_advisory_record(&bytes)?;
        if record.advisory_key != advisory_key {
            return Err(corrupt("delta-pack advisory record key mismatch"));
        }
        Ok(Some(record))
    }

    pub fn source_layout_discovery_report(&self) -> Result<SourceLayoutDiscoveryReport> {
        let (
            generation,
            page_count,
            overlay_root,
            current_record_root,
            root_catalog_root,
            control_root,
        ) = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            (
                inner.generation,
                inner.page_count,
                inner.overlay_root,
                inner.current_record_root,
                inner.root_catalog_root,
                inner.control_root,
            )
        };
        let mut entries = Vec::new();
        {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            let mut overlay_records = Vec::<([u8; 32], Vec<u8>)>::new();
            if let Some(root) = overlay_root {
                for (address, loc) in
                    pagebtree::load_all(&mut **file, DATA_START, root, page_count)?
                {
                    overlay_records.push((address, read_blob_from_loc(&mut **file, loc)?));
                }
            }

            let mut nested_current_root = current_record_root;
            for (address, bytes) in &overlay_records {
                if *address == mutable_overlay_current_root_address()
                    && let Ok(root) = decode_mutable_overlay_current_root_record(bytes)
                    && nested_current_root.is_none()
                {
                    nested_current_root = root;
                }
            }

            if let Some(root) = nested_current_root {
                for (address, loc) in
                    root_family_load_all(&mut **file, CURRENT_RECORDS_FAMILY_ID, root, page_count)?
                {
                    let bytes = read_blob_from_loc(&mut **file, loc)?;
                    entries.push(source_layout_classify_record(
                        address,
                        &bytes,
                        SourceLayoutOwnership::NestedCurrentRoot,
                    ));
                }
            }

            for (address, bytes) in overlay_records {
                entries.push(source_layout_classify_record(
                    address,
                    &bytes,
                    SourceLayoutOwnership::LegacyOverlay,
                ));
            }
        }

        if let Some(root) = control_root {
            match self.get(&root)? {
                Some(bytes) => match decode_control_map(&bytes) {
                    Ok(map) => {
                        for (key, value) in map {
                            entries.push(source_layout_control_entry(key, value));
                        }
                    }
                    Err(err) => entries.push(source_layout_malformed_entry(
                        format!("control:{}", root.to_hex()),
                        SourceLayoutFamily::Control,
                        SourceLayoutOwnership::ControlRootObject,
                        Some(bytes.as_slice()),
                        err.to_string(),
                    )),
                },
                None => entries.push(source_layout_malformed_entry(
                    format!("control:{}", root.to_hex()),
                    SourceLayoutFamily::Control,
                    SourceLayoutOwnership::ControlRootObject,
                    None,
                    "control-plane root object missing".to_string(),
                )),
            }
        }

        source_layout_append_absent_families(&mut entries);
        source_layout_append_conflicts(&mut entries);
        entries.sort_by(|left, right| {
            (
                &left.source_address,
                left.family,
                &left.key_or_identity,
                left.ownership,
                left.decode_state,
            )
                .cmp(&(
                    &right.source_address,
                    right.family,
                    &right.key_or_identity,
                    right.ownership,
                    right.decode_state,
                ))
        });
        Ok(SourceLayoutDiscoveryReport {
            generation,
            page_count,
            overlay_root: overlay_root.map(|page| page.0),
            current_record_root: current_record_root.map(|page| page.0),
            root_catalog_root: root_catalog_root.map(|page| page.0),
            control_root,
            entries,
        })
    }

    pub(crate) fn source_layout_migration_plan(&self) -> Result<SourceLayoutMigrationPlan> {
        let (
            generation,
            page_count,
            region_table_root,
            overlay_root,
            current_record_root,
            root_catalog_root,
            control_root,
        ) = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            (
                inner.generation,
                inner.page_count,
                inner.region_table_root,
                inner.overlay_root,
                inner.current_record_root,
                inner.root_catalog_root,
                inner.control_root,
            )
        };
        let mut entries = Vec::<SourceLayoutDiscoveryEntry>::new();
        let mut source_records =
            Vec::<(SourceLayoutDiscoveryEntry, Option<PageId>, Vec<u8>)>::new();
        {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            let mut overlay_records = Vec::<([u8; 32], Vec<u8>)>::new();
            if let Some(root) = overlay_root {
                for (address, loc) in
                    pagebtree::load_all(&mut **file, DATA_START, root, page_count)?
                {
                    overlay_records.push((address, read_blob_from_loc(&mut **file, loc)?));
                }
            }
            let mut nested_current_root = current_record_root;
            for (address, bytes) in &overlay_records {
                if *address == mutable_overlay_current_root_address()
                    && let Ok(root) = decode_mutable_overlay_current_root_record(bytes)
                    && nested_current_root.is_none()
                {
                    nested_current_root = root;
                }
            }
            if let Some(root) = nested_current_root {
                for (address, loc) in
                    root_family_load_all(&mut **file, CURRENT_RECORDS_FAMILY_ID, root, page_count)?
                {
                    let bytes = read_blob_from_loc(&mut **file, loc)?;
                    let entry = source_layout_classify_record(
                        address,
                        &bytes,
                        SourceLayoutOwnership::NestedCurrentRoot,
                    );
                    source_records.push((entry.clone(), Some(root), bytes));
                    entries.push(entry);
                }
            }
            for (address, bytes) in overlay_records {
                let entry = source_layout_classify_record(
                    address,
                    &bytes,
                    SourceLayoutOwnership::LegacyOverlay,
                );
                source_records.push((entry.clone(), overlay_root, bytes));
                entries.push(entry);
            }
        }

        let mut audit_records = BTreeMap::<Vec<u8>, Vec<u8>>::new();
        let mut control_records = Vec::<SourceLayoutMigrationRecord>::new();
        if let Some(root) = control_root {
            match self.get(&root)? {
                Some(bytes) => match decode_control_map(&bytes) {
                    Ok(map) => {
                        for (key, value) in map {
                            let entry = source_layout_control_entry(key.clone(), value.clone());
                            entries.push(entry.clone());
                            if is_audit_retention_control_key(&key) {
                                audit_records.insert(key, value);
                            } else {
                                control_records.push(source_layout_migration_record(
                                    &entry,
                                    None,
                                    source_layout_bytes_identity(&key),
                                    value,
                                )?);
                            }
                        }
                    }
                    Err(err) => entries.push(source_layout_malformed_entry(
                        format!("control:{}", root.to_hex()),
                        SourceLayoutFamily::Control,
                        SourceLayoutOwnership::ControlRootObject,
                        Some(bytes.as_slice()),
                        err.to_string(),
                    )),
                },
                None => entries.push(source_layout_malformed_entry(
                    format!("control:{}", root.to_hex()),
                    SourceLayoutFamily::Control,
                    SourceLayoutOwnership::ControlRootObject,
                    None,
                    "control-plane root object missing".to_string(),
                )),
            }
        }

        source_layout_append_absent_families(&mut entries);
        source_layout_append_conflicts(&mut entries);
        source_layout_reject_unplannable_entries(&entries)?;

        let mut current_records = Vec::new();
        let mut source_pointers = Vec::new();
        let mut retained_history_records = Vec::new();
        let mut owner_token_records = Vec::new();
        let mut secondary_index_records = Vec::new();
        let mut mutable_idempotency_records = Vec::new();
        let mut workflow_idempotency_records = Vec::new();
        for (entry, source_root, bytes) in source_records {
            if entry.decode_state != SourceLayoutDecodeState::Decoded {
                continue;
            }
            match entry.family {
                SourceLayoutFamily::CurrentEntry => {
                    current_records.push(source_layout_migration_record(
                        &entry,
                        source_root,
                        entry.source_address.clone(),
                        bytes,
                    )?)
                }
                SourceLayoutFamily::CurrentRootPointer => {
                    source_pointers.push(source_layout_migration_record(
                        &entry,
                        source_root,
                        "current-state-root".to_string(),
                        bytes,
                    )?)
                }
                SourceLayoutFamily::RetainedHistoryHead
                | SourceLayoutFamily::RetainedHistoryRecord => {
                    retained_history_records.push(source_layout_migration_record(
                        &entry,
                        source_root,
                        entry.source_address.clone(),
                        bytes,
                    )?)
                }
                SourceLayoutFamily::OwnerToken => {
                    owner_token_records.push(source_layout_migration_record(
                        &entry,
                        source_root,
                        entry.source_address.clone(),
                        bytes,
                    )?)
                }
                SourceLayoutFamily::SecondaryIndex => {
                    secondary_index_records.push(source_layout_migration_record(
                        &entry,
                        source_root,
                        entry.source_address.clone(),
                        bytes,
                    )?)
                }
                SourceLayoutFamily::MutableIdempotency => {
                    mutable_idempotency_records.push(source_layout_migration_record(
                        &entry,
                        source_root,
                        entry.source_address.clone(),
                        bytes,
                    )?)
                }
                SourceLayoutFamily::WorkflowIdempotency => {
                    workflow_idempotency_records.push(source_layout_migration_record(
                        &entry,
                        source_root,
                        entry.source_address.clone(),
                        bytes,
                    )?)
                }
                SourceLayoutFamily::AuditControl
                | SourceLayoutFamily::Control
                | SourceLayoutFamily::Unknown => {}
            }
        }

        let audit_retention_records = audit_retention_family_records(&audit_records)
            .into_iter()
            .map(|(address, bytes)| {
                let (key, _) = decode_audit_retention_record(&bytes)?;
                let entry = SourceLayoutDiscoveryEntry {
                    source_address: format!("control:{}", source_layout_bytes_identity(&key)),
                    family: SourceLayoutFamily::AuditControl,
                    key_or_identity: Some(source_layout_bytes_identity(&key)),
                    generation: None,
                    sequence: source_layout_audit_sequence(&key),
                    payload_digest: Some(Digest::blake3(&bytes).to_hex()),
                    payload_len: Some(bytes.len()),
                    ownership: SourceLayoutOwnership::ControlRootObject,
                    decode_state: SourceLayoutDecodeState::Decoded,
                    rejection_reason: None,
                };
                source_layout_migration_record(&entry, None, source_layout_address(address), bytes)
            })
            .collect::<Result<Vec<_>>>()?;

        current_records.sort_by(source_layout_migration_record_cmp);
        source_pointers.sort_by(source_layout_migration_record_cmp);
        retained_history_records.sort_by(source_layout_migration_record_cmp);
        owner_token_records.sort_by(source_layout_migration_record_cmp);
        secondary_index_records.sort_by(source_layout_migration_record_cmp);
        mutable_idempotency_records.sort_by(source_layout_migration_record_cmp);
        workflow_idempotency_records.sort_by(source_layout_migration_record_cmp);
        control_records.sort_by(source_layout_migration_record_cmp);

        let mut catalog_families = Vec::new();
        source_layout_push_family_plan(
            &mut catalog_families,
            SourceLayoutFamily::RetainedHistoryRecord,
            RETAINED_HISTORY_FAMILY_ID,
            retained_history_records,
        );
        source_layout_push_family_plan(
            &mut catalog_families,
            SourceLayoutFamily::OwnerToken,
            OWNER_TOKEN_FAMILY_ID,
            owner_token_records,
        );
        source_layout_push_family_plan(
            &mut catalog_families,
            SourceLayoutFamily::SecondaryIndex,
            SECONDARY_INDEX_FAMILY_ID,
            secondary_index_records,
        );
        source_layout_push_family_plan(
            &mut catalog_families,
            SourceLayoutFamily::MutableIdempotency,
            MUTABLE_IDEMPOTENCY_FAMILY_ID,
            mutable_idempotency_records,
        );
        source_layout_push_family_plan(
            &mut catalog_families,
            SourceLayoutFamily::WorkflowIdempotency,
            WORKFLOW_IDEMPOTENCY_FAMILY_ID,
            workflow_idempotency_records,
        );
        source_layout_push_family_plan(
            &mut catalog_families,
            SourceLayoutFamily::AuditControl,
            AUDIT_RETENTION_FAMILY_ID,
            audit_retention_records,
        );
        catalog_families.sort_by_key(|family| family.family_id);

        Ok(SourceLayoutMigrationPlan {
            source_identity: SourceLayoutSourceIdentity {
                generation,
                page_count,
                region_table_root: region_table_root.map(|root| root.0),
                overlay_root: overlay_root.map(|root| root.0),
                current_record_root: current_record_root.map(|root| root.0),
                root_catalog_root: root_catalog_root.map(|root| root.0),
                control_root,
            },
            current_records,
            source_pointers,
            catalog_families,
            control_records,
        })
    }

    pub(crate) fn source_layout_replacement_preflight(
        &self,
    ) -> Result<SourceLayoutReplacementPreflight> {
        let report = self.source_layout_discovery_report()?;
        #[cfg(test)]
        self.run_source_layout_preflight_after_discovery_hook()?;
        let source_identity = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            Self::source_layout_identity_locked(&inner)
        };
        source_layout_reject_discovery_identity_mismatch(&report, &source_identity)?;
        source_layout_reject_unplannable_entries(&report.entries)?;
        let classified_owner_counts = source_layout_classified_owner_counts(&report.entries);
        if source_identity.overlay_root.is_some() && source_identity.current_record_root.is_some() {
            return Err(LoomError::new(
                Code::Conflict,
                "replacement preflight rejected legacy overlay with canonical current-record root",
            ));
        }
        if source_identity.overlay_root.is_some() && source_identity.root_catalog_root.is_some() {
            return Err(LoomError::new(
                Code::Conflict,
                "replacement preflight rejected legacy overlay with canonical root-catalog root",
            ));
        }
        if !source_layout_discovery_has_legacy_overlay_records(&report.entries)
            && !source_layout_discovery_has_audit_control_records(&report.entries)
        {
            return Ok(SourceLayoutReplacementPreflight {
                disposition: SourceLayoutReplacementPreflightDisposition::CanonicalNoop,
                source_identity,
                classified_owner_counts,
                validation: None,
            });
        }
        let plan = self.source_layout_migration_plan()?;
        if plan.source_identity != source_identity {
            return Err(LoomError::new(
                Code::Conflict,
                "replacement preflight source changed before validation",
            ));
        }
        let validation = self.validate_source_layout_migration_plan(&plan)?;
        Ok(SourceLayoutReplacementPreflight {
            disposition: SourceLayoutReplacementPreflightDisposition::LegacyReady,
            source_identity: plan.source_identity,
            classified_owner_counts,
            validation: Some(validation),
        })
    }

    pub(crate) fn validate_source_layout_migration_plan(
        &self,
        plan: &SourceLayoutMigrationPlan,
    ) -> Result<SourceLayoutMigrationValidation> {
        self.source_layout_reject_stale_migration_plan(plan)?;
        let reconstructed = self.source_layout_migration_plan()?;
        if reconstructed.source_identity != plan.source_identity {
            return Err(LoomError::new(
                Code::Conflict,
                "source-layout migration source changed during validation",
            ));
        }
        if reconstructed != *plan {
            return Err(corrupt(
                "source-layout migration plan is not the deterministic source plan",
            ));
        }
        source_layout_validate_plan_records(plan)?;
        self.source_layout_verify_plan_source_membership(plan)?;

        let temp = FileStore::with_backing_profile(
            Box::new(MemoryBacking::new()),
            true,
            self.digest_algo,
        )?;
        let current_records = plan
            .current_records
            .iter()
            .map(source_layout_temp_record)
            .collect::<Result<Vec<_>>>()?;
        let mut temporary_current_root = None;
        if !current_records.is_empty() {
            temporary_current_root =
                source_layout_write_temp_family_root(&temp, &current_records, None)?;
        }

        let mut root_catalog_entries = Vec::new();
        let mut temporary_catalog_roots = Vec::new();
        for family in &plan.catalog_families {
            let records = family
                .records
                .iter()
                .map(source_layout_temp_record)
                .collect::<Result<Vec<_>>>()?;
            if let Some(root) = source_layout_write_temp_family_root(&temp, &records, None)? {
                temporary_catalog_roots.push((family.family_id, root.0));
                root_catalog_entries.push(RootCatalogEntry::authoritative(family.family_id, root));
            }
        }
        root_catalog_entries.sort_by_key(|entry| entry.family_id);
        temporary_catalog_roots.sort_by_key(|(family_id, _)| *family_id);
        let control_map = plan
            .control_records
            .iter()
            .map(|record| {
                source_layout_decode_hex_bytes(&record.canonical_address).map(|key| {
                    let value = record.bytes.clone();
                    (key, value)
                })
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let temporary_control_root =
            source_layout_write_temp_control_root(&temp, self.digest_algo, &control_map)?;
        let (
            temporary_object_index_root,
            temporary_root_catalog_root,
            temporary_region_table_root,
            temporary_page_count,
        ) = source_layout_build_temp_canonical_closure(
            &temp,
            temporary_current_root,
            &root_catalog_entries,
        )?;

        Ok(SourceLayoutMigrationValidation {
            current_record_count: plan.current_records.len(),
            source_pointer_count: plan.source_pointers.len(),
            catalog_families: plan
                .catalog_families
                .iter()
                .map(|family| SourceLayoutMigrationFamilyValidation {
                    family: family.family,
                    family_id: family.family_id,
                    record_count: family.records.len(),
                })
                .collect(),
            control_record_count: plan.control_records.len(),
            temporary_current_root: temporary_current_root.map(|root| root.0),
            temporary_catalog_roots,
            temporary_control_root,
            temporary_object_index_root: temporary_object_index_root.map(|root| root.0),
            temporary_root_catalog_root: temporary_root_catalog_root.map(|root| root.0),
            temporary_region_table_root: temporary_region_table_root.map(|root| root.0),
            temporary_page_count,
        })
    }

    #[cfg(test)]
    pub(crate) fn activate_source_layout_migration_plan(
        &self,
        plan: &SourceLayoutMigrationPlan,
    ) -> Result<SourceLayoutMigrationValidation> {
        let validation = self.validate_source_layout_migration_plan(plan)?;
        self.source_layout_reject_stale_migration_plan(plan)?;
        let reconstructed = self.source_layout_migration_plan()?;
        if reconstructed.source_identity != plan.source_identity {
            return Err(LoomError::new(
                Code::Conflict,
                "source-layout migration source changed before activation",
            ));
        }
        if reconstructed != *plan {
            return Err(corrupt(
                "source-layout migration activation plan is not deterministic",
            ));
        }
        source_layout_validate_plan_records(plan)?;
        self.source_layout_verify_plan_source_membership(plan)?;

        let publication_guard = self.overlay_publication.lock().map_err(|_| poisoned())?;
        let mut inner = self.inner.lock().map_err(|_| poisoned())?;
        if Self::source_layout_identity_locked(&inner) != plan.source_identity {
            return Err(LoomError::new(
                Code::Conflict,
                "source-layout migration plan is stale",
            ));
        }
        let control_map = plan
            .control_records
            .iter()
            .map(|record| {
                source_layout_decode_hex_bytes(&record.canonical_address)
                    .map(|key| (key, record.bytes.clone()))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let mut fresh_payloads = Vec::<(Digest, Vec<u8>, Codec)>::new();
        let control_root = if control_map.is_empty() {
            None
        } else {
            let bytes = encode_control_map(&control_map);
            let digest = Digest::hash(self.digest_algo, &bytes);
            if self
                .lookup_loc_locked(&mut inner, digest.bytes())?
                .is_none()
            {
                fresh_payloads.push((digest, bytes, self.default_codec));
            }
            Some(digest)
        };
        let new_gen = inner.generation + 1;
        let (reusable_free, _reclamation_lease) = self.transaction_reusable_free(
            &inner.free,
            inner.active_mark_epoch_reclaim_fence,
            inner.minimum_recoverable_generation,
        )?;
        let (roots, object_placements) = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            let mut alloc = PageAllocator::new_with_reusable_runs(
                inner.page_count,
                new_gen,
                inner.free.clone(),
                reusable_free,
            );
            alloc.install_metadata_bootstrap_reserve(&inner.metadata_bootstrap_reserve)?;

            let current_records = plan
                .current_records
                .iter()
                .map(source_layout_temp_record)
                .collect::<Result<Vec<_>>>()?;
            let current_record_refs = current_records
                .iter()
                .map(|(address, bytes)| (*address, bytes.as_slice()))
                .collect::<Vec<_>>();
            let mutable_overlay_generation_floor =
                mutable_overlay_generation_floor_from_current_records(
                    inner.mutable_overlay_generation_floor,
                    current_records.iter().map(|(_, bytes)| bytes.as_slice()),
                )?;
            let (current_root, mut touched_segments) = write_mutable_record_refs_to_root(
                &mut **file,
                &mut alloc,
                None,
                inner.page_count,
                &current_record_refs,
                None,
                false,
            )?;

            let mut root_catalog_entries = Vec::new();
            for family in &plan.catalog_families {
                let records = family
                    .records
                    .iter()
                    .map(source_layout_temp_record)
                    .collect::<Result<Vec<_>>>()?;
                let record_refs = records
                    .iter()
                    .map(|(address, bytes)| (*address, bytes.as_slice()))
                    .collect::<Vec<_>>();
                let (root, reclaimed) = write_root_family_record_refs_to_root(
                    &mut **file,
                    &mut alloc,
                    family.family_id,
                    None,
                    inner.page_count,
                    &record_refs,
                    None,
                    false,
                )?;
                touched_segments.extend(reclaimed);
                if let Some(root) = root {
                    root_catalog_entries
                        .push(RootCatalogEntry::authoritative(family.family_id, root));
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

            let fresh = fresh_payloads
                .iter()
                .map(|(digest, bytes, codec)| (*digest, bytes.as_slice(), *codec))
                .collect::<Vec<_>>();
            let dek = self.dek.lock().map_err(|_| poisoned())?;
            let object_placements =
                write_record_pages(&mut **file, &mut alloc, &fresh, dek.as_ref())?;
            drop(dek);
            let index_batch = pagebtree::batch_upsert(
                &mut **file,
                DATA_START,
                &mut alloc,
                inner.index_root,
                &object_placements,
                inner.page_count,
            )?;
            #[cfg(any(test, feature = "test-hooks"))]
            observe_object_index_batch(index_batch.stats);
            let index_root = index_batch.root;
            touched_segments.extend(object_placements.iter().map(|(_, loc)| loc.segment_id));
            #[cfg(test)]
            self.run_source_layout_activation_pre_finish_hook()?;
            let object_count = inner
                .maintenance
                .object_count
                .saturating_add(fresh_payloads.len() as u64);
            let roots = finish_txn(
                &mut **file,
                &mut alloc,
                new_gen,
                object_count,
                TxnRootInputs {
                    object_index: index_root,
                    legacy_overlay: None,
                    current_records: current_root,
                    root_catalog: TxnRootCatalog {
                        root: root_catalog_root,
                        entries: root_catalog_entries.clone(),
                    },
                    previous_mutable_overlay_generation_floor: inner
                        .mutable_overlay_generation_floor,
                    mutable_overlay_generation_floor,
                    reference: inner.reference_root.map(|d| *d.bytes()),
                    control: control_root.map(|d| *d.bytes()),
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
            (roots, object_placements)
        };
        self.adopt_committed_roots_locked(&mut inner, roots)?;
        for (key, loc) in object_placements {
            Self::cache_locator_locked(&mut inner, key, loc);
        }
        drop(publication_guard);
        Ok(validation)
    }

    fn source_layout_reject_stale_migration_plan(
        &self,
        plan: &SourceLayoutMigrationPlan,
    ) -> Result<()> {
        let live = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            Self::source_layout_identity_locked(&inner)
        };
        if plan.source_identity != live {
            return Err(LoomError::new(
                Code::Conflict,
                "source-layout migration plan is stale",
            ));
        }
        Ok(())
    }

    fn source_layout_identity_locked(inner: &Inner) -> SourceLayoutSourceIdentity {
        SourceLayoutSourceIdentity {
            generation: inner.generation,
            page_count: inner.page_count,
            region_table_root: inner.region_table_root.map(|root| root.0),
            overlay_root: inner.overlay_root.map(|root| root.0),
            current_record_root: inner.current_record_root.map(|root| root.0),
            root_catalog_root: inner.root_catalog_root.map(|root| root.0),
            control_root: inner.control_root,
        }
    }

    fn source_layout_verify_plan_source_membership(
        &self,
        plan: &SourceLayoutMigrationPlan,
    ) -> Result<()> {
        let mut file = self.file.lock().map_err(|_| poisoned())?;
        for record in &plan.current_records {
            let source_root = record
                .source_root
                .map(PageId)
                .ok_or_else(|| corrupt("source-layout current record missing source root"))?;
            source_layout_verify_page_member(
                &mut **file,
                plan.source_identity.page_count,
                source_root,
                record,
            )?;
        }
        let overlay_root = plan.source_identity.overlay_root.map(PageId);
        for record in &plan.source_pointers {
            let Some(root) = overlay_root else {
                return Err(corrupt(
                    "source-layout current-root pointer lacks captured overlay root",
                ));
            };
            if record.source_root != Some(root.0) {
                return Err(corrupt(
                    "source-layout current-root pointer source root mismatch",
                ));
            }
            source_layout_verify_page_member(
                &mut **file,
                plan.source_identity.page_count,
                root,
                record,
            )?;
        }
        for family in &plan.catalog_families {
            if family.family == SourceLayoutFamily::AuditControl {
                continue;
            }
            let Some(root) = overlay_root else {
                return Err(corrupt(
                    "source-layout catalog record lacks captured overlay root",
                ));
            };
            for record in &family.records {
                if record.source_root != Some(root.0) {
                    return Err(corrupt("source-layout catalog record source root mismatch"));
                }
                source_layout_verify_page_member(
                    &mut **file,
                    plan.source_identity.page_count,
                    root,
                    record,
                )?;
            }
        }
        drop(file);
        self.source_layout_verify_control_derived_membership(plan)
    }

    fn source_layout_verify_control_derived_membership(
        &self,
        plan: &SourceLayoutMigrationPlan,
    ) -> Result<()> {
        let Some(control_root) = plan.source_identity.control_root else {
            if plan.control_records.is_empty()
                && source_layout_plan_audit_records(plan).next().is_none()
            {
                return Ok(());
            }
            return Err(corrupt(
                "source-layout control-derived records lack captured control root",
            ));
        };
        let control_bytes = self
            .get(&control_root)?
            .ok_or_else(|| corrupt("source-layout captured control root missing"))?;
        let control_map = decode_control_map(&control_bytes)?;
        let mut expected_control = BTreeSet::<(String, Vec<u8>)>::new();
        let mut audit_map = BTreeMap::<Vec<u8>, Vec<u8>>::new();
        for (key, value) in control_map {
            if is_audit_retention_control_key(&key) {
                audit_map.insert(key, value);
            } else {
                expected_control.insert((source_layout_bytes_identity(&key), value));
            }
        }
        let actual_control = plan
            .control_records
            .iter()
            .map(|record| (record.canonical_address.clone(), record.bytes.clone()))
            .collect::<BTreeSet<_>>();
        if actual_control != expected_control {
            return Err(corrupt("source-layout control record set mismatch"));
        }
        let expected_audit = audit_retention_family_records(&audit_map)
            .into_iter()
            .map(|(address, bytes)| (source_layout_address(address), bytes))
            .collect::<BTreeSet<_>>();
        let actual_audit = source_layout_plan_audit_records(plan)
            .map(|record| (record.canonical_address.clone(), record.bytes.clone()))
            .collect::<BTreeSet<_>>();
        if actual_audit != expected_audit {
            return Err(corrupt("source-layout audit-retention record set mismatch"));
        }
        Ok(())
    }

    fn load_mutable_overlay_from_storage(&self) -> Result<()> {
        self.load_mutable_overlay_from_storage_with_progress(&mut |_| {})
    }

    fn load_mutable_overlay_from_storage_with_progress(
        &self,
        progress: &mut impl FnMut(StoreOpenProgress),
    ) -> Result<()> {
        let (overlay_root, current_record_root, page_count) = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            (
                inner.overlay_root,
                inner.current_record_root,
                inner.page_count,
            )
        };
        if overlay_root.is_none() && current_record_root.is_none() {
            return Ok(());
        }
        let mut used_current_root = false;
        let mut control_records_skipped = 0u64;
        let mut generation = 0;
        let mut entries = Vec::new();
        {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            if let Some(root) = overlay_root
                && let Some(loc) = pagebtree::get(
                    &mut **file,
                    DATA_START,
                    Some(root),
                    &mutable_overlay_meta_address(),
                    page_count,
                )?
            {
                generation = decode_mutable_overlay_meta(&read_blob_from_loc(&mut **file, loc)?)?;
            }
            let current_root = match current_record_root {
                Some(root) => Some(root),
                None => read_mutable_overlay_current_root(&mut **file, overlay_root, page_count)?,
            };
            if let Some(current_root) = current_root {
                used_current_root = true;
                let mut pages = 0u64;
                let current_entries = pagebtree::load_all_with_progress_and_codec(
                    &mut **file,
                    DATA_START,
                    current_root,
                    page_count,
                    root_family_value_codec(CURRENT_RECORDS_FAMILY_ID)?,
                    |advanced| {
                        pages = pages.saturating_add(advanced);
                        progress(StoreOpenProgress {
                            stage: StoreOpenStage::MutableOverlayIndex,
                            completed: pages,
                            total: None,
                        });
                    },
                )?;
                let total = current_entries.len() as u64;
                for (index, (_, loc)) in current_entries.into_iter().enumerate() {
                    progress(StoreOpenProgress {
                        stage: StoreOpenStage::MutableOverlayRecords,
                        completed: index as u64,
                        total: Some(total),
                    });
                    entries.push(decode_mutable_overlay_entry(&read_blob_from_loc(
                        &mut **file,
                        loc,
                    )?)?);
                }
                progress(StoreOpenProgress {
                    stage: StoreOpenStage::MutableOverlayRecords,
                    completed: total,
                    total: Some(total),
                });
            } else if let Some(root) = overlay_root {
                let mut pages = 0u64;
                let legacy_entries = pagebtree::load_all_with_progress(
                    &mut **file,
                    DATA_START,
                    root,
                    page_count,
                    |advanced| {
                        pages = pages.saturating_add(advanced);
                        progress(StoreOpenProgress {
                            stage: StoreOpenStage::MutableOverlayIndex,
                            completed: pages,
                            total: None,
                        });
                    },
                )?;
                let total = legacy_entries.len() as u64;
                for (index, (address, loc)) in legacy_entries.into_iter().enumerate() {
                    progress(StoreOpenProgress {
                        stage: StoreOpenStage::MutableOverlayRecords,
                        completed: index as u64,
                        total: Some(total),
                    });
                    let value = read_blob_from_loc(&mut **file, loc)?;
                    if address == mutable_overlay_meta_address() {
                        generation = decode_mutable_overlay_meta(&value)?;
                    } else if address == mutable_overlay_current_root_address()
                        || value.starts_with(MUTABLE_OVERLAY_OWNER_TOKEN_RECORD)
                        || value.starts_with(MUTABLE_OVERLAY_SECONDARY_INDEX_RECORD)
                        || value.starts_with(MUTABLE_OVERLAY_IDEMPOTENCY_RECORD)
                        || value.starts_with(MUTABLE_OVERLAY_TRANSACTION_IDEMPOTENCY_RECORD)
                        || value.starts_with(RETAINED_HISTORY_HEAD_RECORD)
                        || value.starts_with(RETAINED_HISTORY_ENTRY_RECORD)
                    {
                        control_records_skipped += 1;
                    } else {
                        return Err(corrupt(
                            "mutable overlay current root missing; controlled migration required",
                        ));
                    }
                }
                progress(StoreOpenProgress {
                    stage: StoreOpenStage::MutableOverlayRecords,
                    completed: total,
                    total: Some(total),
                });
            }
        }
        let entries_loaded = entries.len() as u64;
        entries.sort_by_key(|entry| entry.generation);
        let mut overlay = loom_core::MutableOverlay::import_entries_with_progress(
            &entries,
            |completed, total| {
                progress(StoreOpenProgress {
                    stage: StoreOpenStage::MutableOverlayImport,
                    completed,
                    total: Some(total),
                });
            },
        )?;
        overlay.set_generation_floor(generation);
        *self.mutable_overlay.lock().map_err(|_| poisoned())? = overlay;
        let mut inner = self.inner.lock().map_err(|_| poisoned())?;
        inner.io_stats.open_mutable_current_records_loaded = entries_loaded;
        inner.io_stats.open_mutable_control_records_skipped = control_records_skipped;
        inner.io_stats.open_mutable_used_current_root = used_current_root;
        Ok(())
    }

    fn read_blob_at_loc(&self, loc: RecordLoc, page_count: u64) -> Result<Vec<u8>> {
        let global = loc.global_page();
        if global >= page_count {
            return Err(corrupt("blob locator past the page array"));
        }
        let mut file = self.file.lock().map_err(|_| poisoned())?;
        let mut first = [0u8; PAGE_SIZE as usize];
        read_exact_at(&mut **file, PageId(global).offset(DATA_START), &mut first)
            .map_err(io_err)?;
        match first[0] {
            record::SLAB_MAGIC => record::read_slab_slot(&first, loc.slot)
                .map(|bytes| bytes.to_vec())
                .ok_or_else(|| corrupt("bad slab blob slot on read")),
            record::LARGE_MAGIC => {
                let blob_len = record::large_blob_len(&first)
                    .ok_or_else(|| corrupt("bad large blob header"))?;
                let pages = record::large_pages(blob_len);
                if global + pages > page_count {
                    return Err(corrupt("large blob run past the page array"));
                }
                let mut buf = vec![0u8; (pages * PAGE_SIZE) as usize];
                read_exact_at(&mut **file, PageId(global).offset(DATA_START), &mut buf)
                    .map_err(io_err)?;
                record::decode_large(&buf)
                    .map(|bytes| bytes.to_vec())
                    .ok_or_else(|| corrupt("large blob parse failure"))
            }
            record::CHUNKED_BLOB_MAGIC => {
                record_io::read_chunked_blob(&mut **file, global, page_count)
            }
            _ => Err(corrupt("bad blob page magic on read")),
        }
    }

    fn read_object_payload_at_loc(
        &self,
        loc: RecordLoc,
        page_count: u64,
        digest: &Digest,
    ) -> Result<Vec<u8>> {
        let global = loc.global_page();
        if global >= page_count {
            return Err(corrupt("record locator past the page array"));
        }
        let mut file = self.file.lock().map_err(|_| poisoned())?;
        let dek = self.dek.lock().map_err(|_| poisoned())?;
        let mut first = [0u8; PAGE_SIZE as usize];
        read_exact_at(&mut **file, PageId(global).offset(DATA_START), &mut first)
            .map_err(io_err)?;
        match first[0] {
            record::SLAB_MAGIC => {
                let rec = record::read_slab_slot(&first, loc.slot)
                    .ok_or_else(|| corrupt("bad slab slot on read"))?;
                decode_record(rec, digest, dek.as_ref(), self.digest_algo)
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
                decode_record(rec, digest, dek.as_ref(), self.digest_algo)
            }
            record::CHUNKED_BLOB_MAGIC => {
                let rec = record_io::read_chunked_blob(&mut **file, global, page_count)?;
                decode_record(&rec, digest, dek.as_ref(), self.digest_algo)
            }
            _ => Err(corrupt("bad record page magic on read")),
        }
    }

    fn commit_mutable_overlay_records(&self, records: &[([u8; 32], Vec<u8>)]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let records = records
            .iter()
            .cloned()
            .collect::<BTreeMap<_, _>>()
            .into_iter()
            .collect::<Vec<_>>();
        let oldest_pinned_snapshot_generation = self
            .oldest_pinned_mvcc_snapshot_generation()?
            .map(|generation| generation.as_u64());
        let audit_retention_active = self.audit_config()?.legal_hold;
        let mut inner = self.inner.lock().map_err(|_| poisoned())?;
        let new_gen = inner.generation + 1;
        let control_map = self.control_map_locked(&mut inner)?;
        let publication_authority =
            self.begin_foreground_transaction_publication(&inner, control_map)?;
        let roots = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            let prepared = self.prepare_foreground_transaction_publication(
                &mut **file,
                &inner,
                ForegroundMutationInput::MutableOverlayRecords,
                &publication_authority,
                |file, alloc| {
                    let mut current_records = BTreeMap::<[u8; 32], Vec<u8>>::new();
                    let mut retained_records = BTreeMap::<[u8; 32], Vec<u8>>::new();
                    let mut owner_token_records = BTreeMap::<[u8; 32], Vec<u8>>::new();
                    let mut secondary_index_records = BTreeMap::<[u8; 32], Vec<u8>>::new();
                    let mut mutable_idempotency_records = BTreeMap::<[u8; 32], Vec<u8>>::new();
                    let mut workflow_idempotency_records = BTreeMap::<[u8; 32], Vec<u8>>::new();
                    let mut legacy_records = BTreeMap::<[u8; 32], Vec<u8>>::new();
                    let legacy_overlay_root_before = legacy_overlay_root_for_publication(
                        &inner,
                        inner.current_record_root,
                        inner.root_catalog_root,
                    );
                    {
                        let mut classify_record = |address: [u8; 32],
                                                   value: Vec<u8>|
                         -> Result<()> {
                            if is_mutable_overlay_current_entry_record(&value) {
                                current_records.insert(address, value);
                            } else if value.starts_with(RETAINED_HISTORY_HEAD_RECORD)
                                || value.starts_with(RETAINED_HISTORY_ENTRY_RECORD)
                            {
                                retained_records.insert(address, value);
                            } else if value.starts_with(MUTABLE_OVERLAY_OWNER_TOKEN_RECORD) {
                                owner_token_records.insert(address, value);
                            } else if value.starts_with(MUTABLE_OVERLAY_SECONDARY_INDEX_RECORD) {
                                secondary_index_records.insert(address, value);
                            } else if value.starts_with(MUTABLE_OVERLAY_IDEMPOTENCY_RECORD) {
                                mutable_idempotency_records.insert(address, value);
                            } else if value
                                .starts_with(MUTABLE_OVERLAY_TRANSACTION_IDEMPOTENCY_RECORD)
                            {
                                workflow_idempotency_records.insert(address, value);
                            } else {
                                legacy_records.insert(address, value);
                            }
                            Ok(())
                        };
                        if let Some(root) = legacy_overlay_root_before {
                            for (address, loc) in
                                pagebtree::load_all(file, DATA_START, root, inner.page_count)?
                            {
                                if address == mutable_overlay_meta_address()
                                    || address == mutable_overlay_current_root_address()
                                {
                                    continue;
                                }
                                classify_record(address, read_blob_from_loc(file, loc)?)?;
                            }
                        }
                        for (address, value) in &records {
                            classify_record(*address, value.clone())?;
                        }
                    }
                    let mutable_overlay_generation_floor =
                        mutable_overlay_generation_floor_from_current_records(
                            inner.mutable_overlay_generation_floor,
                            current_records.values().map(Vec::as_slice),
                        )?;
                    let current_root_before = read_mutable_overlay_current_record_root(
                        file,
                        inner.current_record_root,
                        legacy_overlay_root_before,
                        inner.page_count,
                    )?;
                    let current_record_refs = current_records
                        .iter()
                        .map(|(address, value)| (*address, value.as_slice()))
                        .collect::<Vec<_>>();
                    let mut touched_segments = BTreeSet::new();
                    if let Some(root) = legacy_overlay_root_before {
                        pagebtree::free_all(file, DATA_START, alloc, root, inner.page_count)?;
                    }
                    let legacy_record_refs = legacy_records
                        .iter()
                        .map(|(address, value)| (*address, value.as_slice()))
                        .collect::<Vec<_>>();
                    let (legacy_overlay_root, legacy_reclaimed) =
                        write_mutable_record_refs_to_root(
                            file,
                            alloc,
                            None,
                            inner.page_count,
                            &legacy_record_refs,
                            None,
                            false,
                        )?;
                    touched_segments.extend(legacy_reclaimed);
                    let retained_record_refs = retained_records
                        .iter()
                        .map(|(address, value)| (*address, value.as_slice()))
                        .collect::<Vec<_>>();
                    let owner_token_record_refs = owner_token_records
                        .iter()
                        .map(|(address, value)| (*address, value.as_slice()))
                        .collect::<Vec<_>>();
                    let secondary_index_record_refs = secondary_index_records
                        .iter()
                        .map(|(address, value)| (*address, value.as_slice()))
                        .collect::<Vec<_>>();
                    let mutable_idempotency_record_refs = mutable_idempotency_records
                        .iter()
                        .map(|(address, value)| (*address, value.as_slice()))
                        .collect::<Vec<_>>();
                    let workflow_idempotency_record_refs = workflow_idempotency_records
                        .iter()
                        .map(|(address, value)| (*address, value.as_slice()))
                        .collect::<Vec<_>>();
                    let family_outcome = write_root_family_record_batches(
                        file,
                        alloc,
                        inner.page_count,
                        &[
                            RootFamilyRecordBatch {
                                family_id: CURRENT_RECORDS_FAMILY_ID,
                                root: current_root_before,
                                records: &current_record_refs,
                            },
                            RootFamilyRecordBatch {
                                family_id: RETAINED_HISTORY_FAMILY_ID,
                                root: inner.retained_history_root,
                                records: &retained_record_refs,
                            },
                            RootFamilyRecordBatch {
                                family_id: OWNER_TOKEN_FAMILY_ID,
                                root: inner.owner_token_root,
                                records: &owner_token_record_refs,
                            },
                            RootFamilyRecordBatch {
                                family_id: SECONDARY_INDEX_FAMILY_ID,
                                root: inner.secondary_index_root,
                                records: &secondary_index_record_refs,
                            },
                            RootFamilyRecordBatch {
                                family_id: MUTABLE_IDEMPOTENCY_FAMILY_ID,
                                root: inner.mutable_idempotency_root,
                                records: &mutable_idempotency_record_refs,
                            },
                            RootFamilyRecordBatch {
                                family_id: WORKFLOW_IDEMPOTENCY_FAMILY_ID,
                                root: inner.workflow_idempotency_root,
                                records: &workflow_idempotency_record_refs,
                            },
                        ],
                        root_catalog_family_root(
                            &inner.root_catalog_entries,
                            DELTA_PACK_CANDIDATE_FAMILY_ID,
                        ),
                        new_gen,
                        self.digest_algo,
                        false,
                        oldest_pinned_snapshot_generation,
                        audit_retention_active,
                    )?;
                    touched_segments.extend(&family_outcome.touched_segments);
                    let family_roots = &family_outcome.roots;
                    let current_root = family_roots[&CURRENT_RECORDS_FAMILY_ID];
                    let retained_history_root = family_roots[&RETAINED_HISTORY_FAMILY_ID];
                    let owner_token_root = family_roots[&OWNER_TOKEN_FAMILY_ID];
                    let secondary_index_root = family_roots[&SECONDARY_INDEX_FAMILY_ID];
                    let mutable_idempotency_root = family_roots[&MUTABLE_IDEMPOTENCY_FAMILY_ID];
                    let workflow_idempotency_root = family_roots[&WORKFLOW_IDEMPOTENCY_FAMILY_ID];
                    let audit_retention_root = inner.audit_retention_root;
                    let root_catalog_entries = root_catalog_entries_with_advisory_family(
                        &root_catalog_entries_with_family(
                            &root_catalog_entries_with_family(
                                &root_catalog_entries_with_family(
                                    &root_catalog_entries_with_family(
                                        &root_catalog_entries_with_family(
                                            &root_catalog_entries_with_family(
                                                &inner.root_catalog_entries,
                                                RETAINED_HISTORY_FAMILY_ID,
                                                retained_history_root,
                                            ),
                                            OWNER_TOKEN_FAMILY_ID,
                                            owner_token_root,
                                        ),
                                        SECONDARY_INDEX_FAMILY_ID,
                                        secondary_index_root,
                                    ),
                                    MUTABLE_IDEMPOTENCY_FAMILY_ID,
                                    mutable_idempotency_root,
                                ),
                                WORKFLOW_IDEMPOTENCY_FAMILY_ID,
                                workflow_idempotency_root,
                            ),
                            AUDIT_RETENTION_FAMILY_ID,
                            audit_retention_root,
                        ),
                        DELTA_PACK_CANDIDATE_FAMILY_ID,
                        family_outcome.delta_pack_candidate_root,
                    );
                    let root_catalog_root = write_root_catalog_page(
                        file,
                        alloc,
                        inner.root_catalog_root,
                        inner.page_count,
                        &root_catalog_entries,
                    )?;
                    let prepared_finalization = self.prepare_foreground_transaction_finalization(
                        file,
                        &inner,
                        &*alloc,
                        &publication_authority,
                        inner.index_root,
                    )?;
                    let finalization = self.apply_foreground_transaction_finalization(
                        file,
                        alloc,
                        inner.index_root,
                        prepared_finalization,
                    )?;
                    if let Some((_, loc)) = finalization.fresh_control_placement {
                        touched_segments.insert(loc.segment_id);
                    }
                    let publication = finish_foreground_txn_on_planning_backing(
                        file,
                        alloc,
                        new_gen,
                        inner.maintenance.object_count.saturating_add(u64::from(
                            finalization.fresh_control_placement.is_some(),
                        )),
                        TxnRootInputs {
                            object_index: finalization.index_root,
                            legacy_overlay: legacy_overlay_root,
                            current_records: current_root,
                            root_catalog: TxnRootCatalog {
                                root: root_catalog_root,
                                entries: root_catalog_entries.clone(),
                            },
                            previous_mutable_overlay_generation_floor: inner
                                .mutable_overlay_generation_floor,
                            mutable_overlay_generation_floor,
                            reference: inner.reference_root.map(|d| *d.bytes()),
                            control: finalization.control,
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
                        finalization.free_map_publication,
                    )?;
                    Ok(PreparedForegroundTransactionOutcome {
                        publication,
                        value: (),
                    })
                },
            )?;
            self.finish_foreground_txn(&mut **file, &inner, prepared)?.0
        };
        self.adopt_committed_roots_locked(&mut inner, roots)?;
        Ok(())
    }

    pub fn consolidate_delta_pack_candidates(&self, max_candidates: usize) -> Result<u64> {
        if max_candidates == 0 {
            return Ok(0);
        }
        let _publication_guard = self.overlay_publication.lock().map_err(|_| poisoned())?;
        let mut inner = self.inner.lock().map_err(|_| poisoned())?;
        let Some(candidate_root) =
            root_catalog_family_root(&inner.root_catalog_entries, DELTA_PACK_CANDIDATE_FAMILY_ID)
        else {
            return Ok(0);
        };
        let selected = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            root_family_load_all(
                &mut **file,
                DELTA_PACK_CANDIDATE_FAMILY_ID,
                candidate_root,
                inner.page_count,
            )?
            .into_iter()
            .filter_map(|(address, loc)| {
                let bytes = read_blob_from_loc(&mut **file, loc).ok()?;
                let advisory = delta_pack::PackAdvisory::decode(&bytes).ok()?;
                advisory.has_debt().then_some((address, advisory))
            })
            .take(max_candidates)
            .collect::<Vec<_>>()
        };
        if selected.is_empty() || (selected.len() == 1 && selected[0].1.dead_slots.is_empty()) {
            return Ok(0);
        }

        let mut records_by_family = BTreeMap::<u16, BTreeMap<[u8; 32], Vec<u8>>>::new();
        {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            for (_, advisory) in &selected {
                for member in &advisory.members {
                    if advisory.dead_slots.contains(&member.slot) {
                        continue;
                    }
                    let root = if member.family_id == CURRENT_RECORDS_FAMILY_ID {
                        inner.current_record_root
                    } else {
                        root_catalog_family_root(&inner.root_catalog_entries, member.family_id)
                    };
                    let Some(loc) = root_family_get(
                        &mut **file,
                        member.family_id,
                        root,
                        &member.address,
                        inner.page_count,
                    )?
                    else {
                        continue;
                    };
                    if loc.global_page() != advisory.page || loc.slot != member.slot {
                        continue;
                    }
                    let payload = read_blob_from_loc(&mut **file, loc)?;
                    if Digest::hash(self.digest_algo, &payload).bytes() != &member.digest {
                        continue;
                    }
                    records_by_family
                        .entry(member.family_id)
                        .or_default()
                        .insert(member.address, payload);
                }
            }
        }
        if records_by_family.is_empty() {
            return Ok(0);
        }

        let control_map = self.control_map_locked(&mut inner)?;
        let publication_authority =
            self.begin_foreground_transaction_publication(&inner, control_map)?;
        let new_gen = inner.generation.saturating_add(1);
        let selected_addresses = selected
            .iter()
            .map(|(address, _)| *address)
            .collect::<BTreeSet<_>>();
        let owned_records = records_by_family
            .into_iter()
            .map(|(family_id, records)| {
                (
                    family_id,
                    if family_id == CURRENT_RECORDS_FAMILY_ID {
                        inner.current_record_root
                    } else {
                        root_catalog_family_root(&inner.root_catalog_entries, family_id)
                    },
                    records.into_iter().collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        let record_refs = owned_records
            .iter()
            .map(|(_, _, records)| {
                records
                    .iter()
                    .map(|(address, payload)| (*address, payload.as_slice()))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let batches = owned_records
            .iter()
            .zip(&record_refs)
            .map(|((family_id, root, _), records)| RootFamilyRecordBatch {
                family_id: *family_id,
                root: *root,
                records,
            })
            .collect::<Vec<_>>();

        let roots = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            let prepared = self.prepare_foreground_transaction_publication(
                &mut **file,
                &inner,
                ForegroundMutationInput::DeltaPackConsolidation,
                &publication_authority,
                |file, alloc| {
                    let mut outcome = write_root_family_record_batches(
                        file,
                        alloc,
                        inner.page_count,
                        &batches,
                        Some(candidate_root),
                        new_gen,
                        self.digest_algo,
                        true,
                        None,
                        false,
                    )?;
                    let mut retired_locs = Vec::new();
                    for address in &selected_addresses {
                        if outcome.fresh_delta_pack_advisories.contains(address) {
                            continue;
                        }
                        if let Some(loc) = root_family_get(
                            file,
                            DELTA_PACK_CANDIDATE_FAMILY_ID,
                            outcome.delta_pack_candidate_root,
                            address,
                            alloc.page_count(),
                        )? {
                            retired_locs.push(loc);
                        }
                        outcome.delta_pack_candidate_root = pagebtree::delete_with_codec(
                            file,
                            DATA_START,
                            alloc,
                            outcome.delta_pack_candidate_root,
                            address,
                            alloc.page_count(),
                            pagebtree::ValueCodecKind::RecordLoc,
                        )?;
                    }
                    free_overlay_record_pages_batch(
                        file,
                        alloc,
                        &retired_locs,
                        &mut outcome.touched_segments,
                    )?;
                    let mut root_catalog_entries = inner.root_catalog_entries.clone();
                    for (family_id, root) in &outcome.roots {
                        if *family_id == CURRENT_RECORDS_FAMILY_ID {
                            continue;
                        }
                        root_catalog_entries = root_catalog_entries_with_family(
                            &root_catalog_entries,
                            *family_id,
                            *root,
                        );
                    }
                    root_catalog_entries = root_catalog_entries_with_advisory_family(
                        &root_catalog_entries,
                        DELTA_PACK_CANDIDATE_FAMILY_ID,
                        outcome.delta_pack_candidate_root,
                    );
                    let root_catalog_root = write_root_catalog_page(
                        file,
                        alloc,
                        inner.root_catalog_root,
                        inner.page_count,
                        &root_catalog_entries,
                    )?;
                    let finalization = self.prepare_foreground_transaction_finalization(
                        file,
                        &inner,
                        alloc,
                        &publication_authority,
                        inner.index_root,
                    )?;
                    let finalization = self.apply_foreground_transaction_finalization(
                        file,
                        alloc,
                        inner.index_root,
                        finalization,
                    )?;
                    if let Some((_, loc)) = finalization.fresh_control_placement {
                        outcome.touched_segments.insert(loc.segment_id);
                    }
                    let current_record_root = outcome
                        .roots
                        .get(&CURRENT_RECORDS_FAMILY_ID)
                        .copied()
                        .unwrap_or(inner.current_record_root);
                    let publication = finish_foreground_txn_on_planning_backing(
                        file,
                        alloc,
                        new_gen,
                        inner.maintenance.object_count.saturating_add(u64::from(
                            finalization.fresh_control_placement.is_some(),
                        )),
                        TxnRootInputs {
                            object_index: finalization.index_root,
                            legacy_overlay: legacy_overlay_root_for_publication(
                                &inner,
                                current_record_root,
                                root_catalog_root,
                            ),
                            current_records: current_record_root,
                            root_catalog: TxnRootCatalog {
                                root: root_catalog_root,
                                entries: root_catalog_entries,
                            },
                            previous_mutable_overlay_generation_floor: inner
                                .mutable_overlay_generation_floor,
                            mutable_overlay_generation_floor: inner
                                .mutable_overlay_generation_floor,
                            reference: inner.reference_root.map(|digest| *digest.bytes()),
                            control: finalization.control,
                        },
                        inner.open_segment,
                        &inner.maintenance,
                        &outcome.touched_segments,
                        (
                            inner.freemap,
                            inner.region_table_root,
                            inner.maintenance_root,
                        ),
                        inner.encryption_meta.clone(),
                        self.digest_algo,
                        None,
                        finalization.free_map_publication,
                    )?;
                    Ok(PreparedForegroundTransactionOutcome {
                        publication,
                        value: (),
                    })
                },
            )?;
            self.finish_foreground_txn(&mut **file, &inner, prepared)?.0
        };
        self.adopt_committed_roots_locked(&mut inner, roots)?;
        Ok(selected.len() as u64)
    }

    #[cfg(test)]
    fn commit_raw_overlay_records_for_test(&self, records: &[([u8; 32], Vec<u8>)]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let records = records
            .iter()
            .cloned()
            .collect::<BTreeMap<_, _>>()
            .into_iter()
            .collect::<Vec<_>>();
        let mut inner = self.inner.lock().map_err(|_| poisoned())?;
        if (inner.current_record_root.is_some() || inner.root_catalog_root.is_some())
            && records.iter().any(|(address, value)| {
                is_mutable_overlay_current_entry_record(value)
                    || (*address != mutable_overlay_current_root_address()
                        && value.starts_with(b"loom.store."))
                    || value.starts_with(RETAINED_HISTORY_HEAD_RECORD)
                    || value.starts_with(RETAINED_HISTORY_ENTRY_RECORD)
                    || value.starts_with(MUTABLE_OVERLAY_OWNER_TOKEN_RECORD)
                    || value.starts_with(MUTABLE_OVERLAY_SECONDARY_INDEX_RECORD)
                    || value.starts_with(MUTABLE_OVERLAY_IDEMPOTENCY_RECORD)
                    || value.starts_with(MUTABLE_OVERLAY_TRANSACTION_IDEMPOTENCY_RECORD)
            })
        {
            return Err(corrupt(
                "legacy overlay canonical family cannot publish over canonical roots",
            ));
        }
        let new_gen = inner.generation + 1;
        let (reusable_free, _reclamation_lease) = self.transaction_reusable_free(
            &inner.free,
            inner.active_mark_epoch_reclaim_fence,
            inner.minimum_recoverable_generation,
        )?;
        let roots = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            let mut alloc = PageAllocator::new_with_reusable_runs(
                inner.page_count,
                new_gen,
                inner.free.clone(),
                reusable_free,
            );
            alloc.install_metadata_bootstrap_reserve(&inner.metadata_bootstrap_reserve)?;
            let records = records
                .iter()
                .map(|(address, value)| (*address, value.as_slice()))
                .collect::<Vec<_>>();
            let entries = overlay_current_record_locs(
                &mut **file,
                inner.overlay_root,
                inner.page_count,
                pagebtree::ValueCodecKind::RecordLoc,
                records.iter().map(|(address, _)| *address),
            )?;
            let placements = write_overlay_blob_pages(&mut **file, &mut alloc, &entries, &records)?;
            let overlay_batch = pagebtree::batch_upsert(
                &mut **file,
                DATA_START,
                &mut alloc,
                inner.overlay_root,
                &placements,
                inner.page_count,
            )?;
            #[cfg(any(test, feature = "test-hooks"))]
            observe_btree_batch(overlay_batch.stats);
            let overlay_root = overlay_batch.root;
            let touched_segments = BTreeSet::<u64>::new();
            finish_txn(
                &mut **file,
                &mut alloc,
                new_gen,
                inner.maintenance.object_count,
                TxnRootInputs {
                    object_index: inner.index_root,
                    legacy_overlay: overlay_root,
                    current_records: inner.current_record_root,
                    root_catalog: TxnRootCatalog {
                        root: None,
                        entries: Vec::new(),
                    },
                    previous_mutable_overlay_generation_floor: inner
                        .mutable_overlay_generation_floor,
                    mutable_overlay_generation_floor: inner.mutable_overlay_generation_floor,
                    reference: inner.reference_root.map(|d| *d.bytes()),
                    control: inner.control_root.map(|d| *d.bytes()),
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
            )?
        };
        self.adopt_committed_roots_locked(&mut inner, roots)?;
        Ok(())
    }

    #[cfg(test)]
    fn commit_raw_control_map_for_test(&self, map: BTreeMap<Vec<u8>, Vec<u8>>) -> Result<()> {
        if map.is_empty() {
            return self.commit_txn(&[], None, Some(None), None);
        }
        let bytes = encode_control_map(&map);
        let digest = Digest::hash(self.digest_algo, &bytes);
        self.commit_txn(
            &[(digest, bytes.as_slice(), self.default_codec)],
            None,
            Some(Some(*digest.bytes())),
            None,
        )
    }

    #[cfg(test)]
    fn commit_family_root_records_for_test(
        &self,
        family_id: u16,
        records: &[([u8; 32], Vec<u8>)],
    ) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let records = records
            .iter()
            .cloned()
            .collect::<BTreeMap<_, _>>()
            .into_iter()
            .collect::<Vec<_>>();
        let mut inner = self.inner.lock().map_err(|_| poisoned())?;
        let new_gen = inner.generation + 1;
        let (reusable_free, _reclamation_lease) = self.transaction_reusable_free(
            &inner.free,
            inner.active_mark_epoch_reclaim_fence,
            inner.minimum_recoverable_generation,
        )?;
        let roots = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            let mut alloc = PageAllocator::new_with_reusable_runs(
                inner.page_count,
                new_gen,
                inner.free.clone(),
                reusable_free,
            );
            alloc.install_metadata_bootstrap_reserve(&inner.metadata_bootstrap_reserve)?;
            let records = records
                .iter()
                .map(|(address, value)| (*address, value.as_slice()))
                .collect::<Vec<_>>();
            let mut retained_history_root = inner.retained_history_root;
            let mut owner_token_root = inner.owner_token_root;
            let mut secondary_index_root = inner.secondary_index_root;
            let mut mutable_idempotency_root = inner.mutable_idempotency_root;
            let mut workflow_idempotency_root = inner.workflow_idempotency_root;
            let mut audit_retention_root = inner.audit_retention_root;
            let mut mvcc_generation_root = inner.mvcc_generation_root;
            let mut retention_index_root = inner.retention_index_root;
            let mut checkpoint_index_root = inner.checkpoint_index_root;
            let mut reclaim_index_root = inner.reclaim_index_root;
            let mut delta_pack_candidate_root = inner
                .root_catalog_entries
                .iter()
                .find(|entry| entry.family_id == DELTA_PACK_CANDIDATE_FAMILY_ID)
                .map(|entry| entry.root);
            let family_root = match family_id {
                RETAINED_HISTORY_FAMILY_ID => &mut retained_history_root,
                OWNER_TOKEN_FAMILY_ID => &mut owner_token_root,
                SECONDARY_INDEX_FAMILY_ID => &mut secondary_index_root,
                MUTABLE_IDEMPOTENCY_FAMILY_ID => &mut mutable_idempotency_root,
                WORKFLOW_IDEMPOTENCY_FAMILY_ID => &mut workflow_idempotency_root,
                AUDIT_RETENTION_FAMILY_ID => &mut audit_retention_root,
                MVCC_GENERATION_FAMILY_ID => &mut mvcc_generation_root,
                RETENTION_INDEX_FAMILY_ID => &mut retention_index_root,
                CHECKPOINT_INDEX_FAMILY_ID => &mut checkpoint_index_root,
                RECLAIM_INDEX_FAMILY_ID => &mut reclaim_index_root,
                DELTA_PACK_CANDIDATE_FAMILY_ID => &mut delta_pack_candidate_root,
                _ => {
                    return Err(LoomError::invalid(
                        "unsupported test-only mutable family root",
                    ));
                }
            };
            let codec = root_family_descriptor(family_id)
                .ok_or_else(|| corrupt("unknown test-only mutable family root"))?
                .value_codec;
            let entries = overlay_current_record_locs(
                &mut **file,
                *family_root,
                inner.page_count,
                codec,
                records.iter().map(|(address, _)| *address),
            )?;
            let placements = write_overlay_blob_pages(&mut **file, &mut alloc, &entries, &records)?;
            for (address, loc) in &placements {
                let bound = alloc.page_count();
                *family_root = Some(pagebtree::insert_with_codec(
                    &mut **file,
                    DATA_START,
                    &mut alloc,
                    *family_root,
                    address,
                    *loc,
                    bound,
                    codec,
                )?);
            }
            let root = match family_id {
                RETAINED_HISTORY_FAMILY_ID => retained_history_root,
                OWNER_TOKEN_FAMILY_ID => owner_token_root,
                SECONDARY_INDEX_FAMILY_ID => secondary_index_root,
                MUTABLE_IDEMPOTENCY_FAMILY_ID => mutable_idempotency_root,
                WORKFLOW_IDEMPOTENCY_FAMILY_ID => workflow_idempotency_root,
                AUDIT_RETENTION_FAMILY_ID => audit_retention_root,
                MVCC_GENERATION_FAMILY_ID => mvcc_generation_root,
                RETENTION_INDEX_FAMILY_ID => retention_index_root,
                CHECKPOINT_INDEX_FAMILY_ID => checkpoint_index_root,
                RECLAIM_INDEX_FAMILY_ID => reclaim_index_root,
                DELTA_PACK_CANDIDATE_FAMILY_ID => delta_pack_candidate_root,
                _ => None,
            };
            let root_catalog_entries = if family_id == DELTA_PACK_CANDIDATE_FAMILY_ID {
                root_catalog_entries_with_advisory_family(
                    &inner.root_catalog_entries,
                    family_id,
                    root,
                )
            } else {
                root_catalog_entries_with_family(&inner.root_catalog_entries, family_id, root)
            };
            let root_catalog_root = write_root_catalog_page(
                &mut **file,
                &mut alloc,
                inner.root_catalog_root,
                inner.page_count,
                &root_catalog_entries,
            )?;
            let touched_segments = BTreeSet::<u64>::new();
            let roots = finish_txn(
                &mut **file,
                &mut alloc,
                new_gen,
                inner.maintenance.object_count,
                TxnRootInputs {
                    object_index: inner.index_root,
                    legacy_overlay: legacy_overlay_root_for_publication(
                        &inner,
                        inner.current_record_root,
                        root_catalog_root,
                    ),
                    current_records: inner.current_record_root,
                    root_catalog: TxnRootCatalog {
                        root: root_catalog_root,
                        entries: root_catalog_entries.clone(),
                    },
                    previous_mutable_overlay_generation_floor: inner
                        .mutable_overlay_generation_floor,
                    mutable_overlay_generation_floor: inner.mutable_overlay_generation_floor,
                    reference: inner.reference_root.map(|d| *d.bytes()),
                    control: inner.control_root.map(|d| *d.bytes()),
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
            roots
        };
        self.adopt_committed_roots_locked(&mut inner, roots)?;
        Ok(())
    }

    #[cfg(test)]
    fn commit_current_root_records_for_test(&self, records: &[([u8; 32], Vec<u8>)]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let records = records
            .iter()
            .cloned()
            .collect::<BTreeMap<_, _>>()
            .into_iter()
            .collect::<Vec<_>>();
        let mut inner = self.inner.lock().map_err(|_| poisoned())?;
        let new_gen = inner.generation + 1;
        let (reusable_free, _reclamation_lease) = self.transaction_reusable_free(
            &inner.free,
            inner.active_mark_epoch_reclaim_fence,
            inner.minimum_recoverable_generation,
        )?;
        let roots = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            let mut alloc = PageAllocator::new_with_reusable_runs(
                inner.page_count,
                new_gen,
                inner.free.clone(),
                reusable_free,
            );
            alloc.install_metadata_bootstrap_reserve(&inner.metadata_bootstrap_reserve)?;
            let records = records
                .iter()
                .map(|(address, value)| (*address, value.as_slice()))
                .collect::<Vec<_>>();
            let mutable_overlay_generation_floor =
                mutable_overlay_generation_floor_from_current_records(
                    inner.mutable_overlay_generation_floor,
                    records.iter().map(|(_, bytes)| *bytes),
                )?;
            let entries = overlay_current_record_locs(
                &mut **file,
                inner.current_record_root,
                inner.page_count,
                pagebtree::ValueCodecKind::RecordLoc,
                records.iter().map(|(address, _)| *address),
            )?;
            let placements = write_overlay_blob_pages(&mut **file, &mut alloc, &entries, &records)?;
            let mut current_record_root = inner.current_record_root;
            for (address, loc) in &placements {
                let bound = alloc.page_count();
                current_record_root = Some(pagebtree::insert(
                    &mut **file,
                    DATA_START,
                    &mut alloc,
                    current_record_root,
                    address,
                    *loc,
                    bound,
                )?);
            }
            let touched_segments = BTreeSet::<u64>::new();
            finish_txn(
                &mut **file,
                &mut alloc,
                new_gen,
                inner.maintenance.object_count,
                TxnRootInputs {
                    object_index: inner.index_root,
                    legacy_overlay: legacy_overlay_root_for_publication(
                        &inner,
                        current_record_root,
                        inner.root_catalog_root,
                    ),
                    current_records: current_record_root,
                    root_catalog: TxnRootCatalog {
                        root: inner.root_catalog_root,
                        entries: inner.root_catalog_entries.clone(),
                    },
                    previous_mutable_overlay_generation_floor: inner
                        .mutable_overlay_generation_floor,
                    mutable_overlay_generation_floor,
                    reference: inner.reference_root.map(|d| *d.bytes()),
                    control: inner.control_root.map(|d| *d.bytes()),
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
            )?
        };
        self.adopt_committed_roots_locked(&mut inner, roots)?;
        Ok(())
    }

    /// Group-commit / hot-mutable durability diagnostics. Loads the cumulative counters from
    /// the lock-free accumulator and reads the point-in-time pending-window gauges from the hot-mutable
    /// queue. Takes no lock beyond a brief hold of the hot-mutable queue; does not touch `inner`, so it
    /// can be called before locking `inner` without inverting the queue -> inner lock order used on the
    /// publish path.
    pub fn group_commit_diagnostics(&self) -> Result<GroupCommitDiagnostics> {
        let metrics = &self.group_commit_metrics;
        let (pending_transactions, pending_records) = {
            let queue = self.hot_mutable_queue.lock().map_err(|_| poisoned())?;
            queue.pending_window().map_or((0, 0), |window| {
                (window.transaction_count as u64, window.record_count as u64)
            })
        };
        Ok(GroupCommitDiagnostics {
            group_commit_batches_total: metrics.batches_total.load(Ordering::Relaxed),
            group_commit_transactions_total: metrics.transactions_total.load(Ordering::Relaxed),
            group_commit_records_total: metrics.records_total.load(Ordering::Relaxed),
            fsync_total_micros: metrics.fsync_total_micros.load(Ordering::Relaxed),
            fsync_count: metrics.fsync_count.load(Ordering::Relaxed),
            write_lock_wait_total_micros: metrics
                .write_lock_wait_total_micros
                .load(Ordering::Relaxed),
            write_lock_wait_count: metrics.write_lock_wait_count.load(Ordering::Relaxed),
            pending_durable_window_transactions: pending_transactions,
            pending_durable_window_records: pending_records,
            pinned_reader_blockers: {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    Some(u64::from(!self.try_reclamation_write_lease()?.allowed))
                }
                #[cfg(target_arch = "wasm32")]
                {
                    None
                }
            },
        })
    }

    pub fn maintenance_status(&self) -> Result<MaintenanceStatus> {
        // Computed before locking `inner` to keep the hot-mutable queue -> inner lock order.
        let group_commit = self.group_commit_diagnostics()?;
        let inner = self.inner.lock().map_err(|_| poisoned())?;
        let tail_free_pages = tail_free_pages(&inner.free, inner.maintenance.physical_page_count);
        Ok(MaintenanceStatus {
            generation: inner.maintenance.generation,
            object_count: inner.maintenance.object_count,
            physical_page_count: inner.maintenance.physical_page_count,
            physical_bytes: DATA_START + inner.maintenance.physical_page_count * PAGE_SIZE,
            reusable_free_pages: inner.maintenance.reusable_free_pages,
            candidate_dead_pages: inner.maintenance.candidate_dead_pages,
            tail_free_pages,
            tail_free_bytes: tail_free_pages.saturating_mul(PAGE_SIZE),
            last_validated_mark_epoch: inner.maintenance.last_validated_mark_epoch,
            touched_segments: inner.maintenance.touched_segments.clone(),
            candidate_segments: inner.maintenance.candidate_segments.clone(),
            segment_overflow: inner.maintenance.segment_overflow,
            group_commit,
        })
    }

    pub fn page_class_attribution(&self, max_examples: usize) -> Result<StorePageClassAttribution> {
        let (
            page_count,
            index_root,
            overlay_root,
            current_record_root,
            freemap,
            region_table_root,
            maintenance_root,
            free,
            root_catalog_root,
            root_catalog_entries,
            index_locs,
        ) = {
            let mut inner = self.inner.lock().map_err(|_| poisoned())?;
            self.materialize_index_locked(&mut inner)?;
            (
                inner.maintenance.physical_page_count,
                inner.index_root,
                inner.overlay_root,
                inner.current_record_root,
                inner.freemap,
                inner.region_table_root,
                inner.maintenance_root,
                inner.free.clone(),
                inner.root_catalog_root,
                inner.root_catalog_entries.clone(),
                inner.index.values().copied().collect::<Vec<_>>(),
            )
        };
        let mut pages = BTreeMap::<u64, String>::new();
        let mut file = self.file.lock().map_err(|_| poisoned())?;
        if let Some((root, span)) = freemap {
            classify_page_run(&mut pages, root.0, span, "free_map_page");
        }
        if let Some(root) = region_table_root {
            classify_page_run(&mut pages, root.0, 1, "region_table_page");
        }
        if let Some(root) = maintenance_root {
            classify_page_run(&mut pages, root.0, 1, "maintenance_page");
        }
        if let Some(root) = root_catalog_root {
            classify_page_run(&mut pages, root.0, 1, "root_catalog_page");
        }
        for run in free {
            let class = if run.start.saturating_add(run.len) == page_count {
                "tail_free_page"
            } else {
                "reusable_free_page"
            };
            classify_page_run(&mut pages, run.start, run.len, class);
        }
        if let Some(root) = index_root {
            for page in pagebtree::collect_pages(&mut **file, DATA_START, root, page_count)? {
                classify_page_run(&mut pages, page.0, 1, "object_index_tree_page");
            }
        }
        let mut overlay_locs = if let Some(root) = overlay_root {
            for page in pagebtree::collect_pages(&mut **file, DATA_START, root, page_count)? {
                classify_page_run(&mut pages, page.0, 1, "mutable_overlay_tree_page");
            }
            pagebtree::load_all(&mut **file, DATA_START, root, page_count)?
                .into_iter()
                .map(|(_, loc)| loc)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let legacy_current_root =
            read_mutable_overlay_current_root(&mut **file, overlay_root, page_count)?;
        let current_root = match current_record_root {
            Some(root) => Some(root),
            None => legacy_current_root,
        };
        if let Some(root) = current_root {
            for page in
                root_family_collect_pages(&mut **file, CURRENT_RECORDS_FAMILY_ID, root, page_count)?
            {
                classify_page_run(&mut pages, page.0, 1, "mutable_overlay_current_tree_page");
            }
            overlay_locs.extend(
                root_family_load_all(&mut **file, CURRENT_RECORDS_FAMILY_ID, root, page_count)?
                    .into_iter()
                    .map(|(_, loc)| loc),
            );
        }
        for entry in &root_catalog_entries {
            if entry.family_id == CURRENT_RECORDS_FAMILY_ID {
                continue;
            }
            let name = ROOT_FAMILY_REGISTRY
                .iter()
                .find(|descriptor| descriptor.family_id == entry.family_id)
                .map(|descriptor| descriptor.name)
                .unwrap_or("catalog_family");
            let tree_class = format!("{name}_tree_page");
            for page in
                root_family_collect_pages(&mut **file, entry.family_id, entry.root, page_count)?
            {
                classify_page_run(&mut pages, page.0, 1, &tree_class);
            }
            overlay_locs.extend(
                root_family_load_all(&mut **file, entry.family_id, entry.root, page_count)?
                    .into_iter()
                    .map(|(_, loc)| loc),
            );
        }
        for loc in index_locs {
            classify_record_loc(&mut **file, &mut pages, loc, page_count, "record")?;
        }
        for loc in overlay_locs {
            classify_record_loc(
                &mut **file,
                &mut pages,
                loc,
                page_count,
                "mutable_overlay_record",
            )?;
        }
        let mut classes = BTreeMap::<String, StorePageClass>::new();
        add_page_class(
            &mut classes,
            "file_header_journal_checkpoint",
            DATA_START / PAGE_SIZE,
            "file header",
            max_examples,
        );
        let mut page = 0;
        while page < page_count {
            let class = if let Some(class) = pages.get(&page).cloned() {
                class
            } else {
                classify_unreferenced_page(&mut **file, &mut pages, page, page_count)?
            };
            add_page_class(
                &mut classes,
                &class,
                1,
                &format!("page:{page}"),
                max_examples,
            );
            page += 1;
        }
        let mut classes = classes.into_values().collect::<Vec<_>>();
        classes.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.class.cmp(&b.class)));
        Ok(StorePageClassAttribution {
            physical_bytes: DATA_START + page_count * PAGE_SIZE,
            page_size: PAGE_SIZE,
            data_pages: page_count,
            classes,
        })
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub fn btree_root_depths_for_test(&self) -> Result<Vec<StoreBtreeRootDepth>> {
        let inner = self.inner.lock().map_err(|_| poisoned())?;
        let mut file = self.file.lock().map_err(|_| poisoned())?;
        let mut roots = Vec::new();
        let mut push =
            |name: &str, root: Option<PageId>, codec: pagebtree::ValueCodecKind| -> Result<()> {
                if let Some(root) = root {
                    roots.push(StoreBtreeRootDepth {
                        root: name.to_string(),
                        depth: pagebtree::tree_depth_with_codec(
                            &mut **file,
                            DATA_START,
                            root,
                            inner.page_count,
                            codec,
                        )?,
                    });
                }
                Ok(())
            };
        push(
            "object_index",
            inner.index_root,
            pagebtree::ValueCodecKind::RecordLoc,
        )?;
        push(
            "legacy_overlay",
            inner.overlay_root,
            pagebtree::ValueCodecKind::RecordLoc,
        )?;
        push(
            "current_records",
            inner.current_record_root,
            root_family_value_codec(CURRENT_RECORDS_FAMILY_ID)?,
        )?;
        for entry in &inner.root_catalog_entries {
            let descriptor = root_family_descriptor(entry.family_id)
                .ok_or_else(|| corrupt("unknown catalog family in B-tree depth diagnostics"))?;
            push(descriptor.name, Some(entry.root), descriptor.value_codec)?;
        }
        push(
            "free_map",
            inner.freemap.map(|(root, _)| root),
            pagebtree::ValueCodecKind::FreePageExtent,
        )?;
        roots.sort_by(|left, right| left.root.cmp(&right.root));
        Ok(roots)
    }

    pub fn root_codec_diagnostics(&self) -> Result<StoreRootCodecDiagnostics> {
        let (page_count, mut roots) =
            {
                let inner = self.inner.lock().map_err(|_| poisoned())?;
                let mut roots = vec![
                    StoreRootCodecExpectation {
                        root_name: "object_index",
                        family_id: None,
                        root: inner.index_root,
                        codec: Some(pagebtree::ValueCodecKind::RecordLoc),
                    },
                    StoreRootCodecExpectation {
                        root_name: "current_records",
                        family_id: Some(CURRENT_RECORDS_FAMILY_ID),
                        root: inner.current_record_root,
                        codec: root_family_descriptor(CURRENT_RECORDS_FAMILY_ID)
                            .map(|descriptor| descriptor.value_codec),
                    },
                ];
                roots.extend(inner.root_catalog_entries.iter().map(|entry| {
                    root_catalog_codec_expectation(entry.family_id, Some(entry.root))
                }));
                (inner.maintenance.physical_page_count, roots)
            };
        roots.sort_by_key(|root| (root.family_id.unwrap_or(0), root.root_name));
        let mut details = Vec::new();
        let mut failures = Vec::new();
        let mut file = self.file.lock().map_err(|_| poisoned())?;
        for root in roots {
            let Some(root_page) = root.root else {
                continue;
            };
            let diagnostic = if let Some(codec) = root.codec {
                let (failing_page, inspection) = pagebtree::inspect_tree_codec(
                    &mut **file,
                    DATA_START,
                    root_page,
                    page_count,
                    codec,
                )?;
                StoreRootCodecDiagnostic {
                    root_name: root.root_name,
                    family_id: root.family_id,
                    root_page: failing_page.0,
                    byte_offset: failing_page.offset(DATA_START),
                    expected_codec: codec.name(),
                    expected_discriminator: codec.discriminator(),
                    raw_magic: inspection.raw_magic,
                    raw_flags: inspection.raw_flags,
                    actual_discriminator: inspection.actual_discriminator,
                    in_range: inspection.in_range,
                    checksum_ok: inspection.checksum_ok,
                    magic_ok: inspection.magic_ok,
                    codec_ok: inspection.codec_ok,
                    reachable: true,
                    failure: inspection.failure,
                }
            } else {
                StoreRootCodecDiagnostic {
                    root_name: root.root_name,
                    family_id: root.family_id,
                    root_page: root_page.0,
                    byte_offset: root_page.offset(DATA_START),
                    expected_codec: "unknown_family",
                    expected_discriminator: 0,
                    raw_magic: None,
                    raw_flags: None,
                    actual_discriminator: None,
                    in_range: root_page.0 < page_count,
                    checksum_ok: false,
                    magic_ok: false,
                    codec_ok: false,
                    reachable: true,
                    failure: Some("unknown_root_family"),
                }
            };
            if diagnostic.failure.is_some() {
                failures.push(diagnostic.clone());
            }
            details.push(diagnostic);
        }
        Ok(StoreRootCodecDiagnostics {
            checked_roots: details.len(),
            failures,
            details,
        })
    }

    pub fn root_storage_attribution(
        &self,
        max_examples: usize,
    ) -> Result<StoreRootStorageAttribution> {
        let (
            page_count,
            index_root,
            freemap,
            maintenance_root,
            current_record_root,
            root_catalog_root,
            catalog_roots,
            reference_root,
            control_root,
            index_entries,
        ) = {
            let mut inner = self.inner.lock().map_err(|_| poisoned())?;
            self.materialize_index_locked(&mut inner)?;
            let catalog_roots = inner
                .root_catalog_entries
                .iter()
                .map(|entry| (entry.family_id, entry.root))
                .collect::<BTreeMap<_, _>>();
            (
                inner.maintenance.physical_page_count,
                inner.index_root,
                inner.freemap,
                inner.maintenance_root,
                inner.current_record_root,
                inner.root_catalog_root,
                catalog_roots,
                inner.reference_root,
                inner.control_root,
                inner.index.clone(),
            )
        };

        let mut file = self.file.lock().map_err(|_| poisoned())?;
        let mut roots = Vec::new();
        roots.push(attribution_for_btree_root(
            &mut **file,
            page_count,
            "object_index_records",
            None,
            "object_index",
            index_root,
            max_examples,
        )?);
        if let Some(root) = roots.last_mut() {
            for loc in index_entries.values() {
                let (pages, payload_bytes) = record_loc_storage(&mut **file, *loc, page_count)?;
                root.record_pages = root.record_pages.saturating_add(pages);
                root.payload_bytes = root.payload_bytes.saturating_add(payload_bytes);
                push_example(
                    &mut root.examples,
                    format!("record_page:{}", loc.global_page()),
                    max_examples,
                );
            }
        }
        roots.push(attribution_for_btree_root(
            &mut **file,
            page_count,
            "current_records",
            Some(CURRENT_RECORDS_FAMILY_ID),
            "current",
            current_record_root,
            max_examples,
        )?);
        roots.push(attribution_for_page_run(
            "root_catalog",
            None,
            "root_catalog",
            root_catalog_root.map(|root| (root.0, 1)),
            max_examples,
        ));
        for descriptor in ROOT_FAMILY_REGISTRY {
            if descriptor.family_id == CURRENT_RECORDS_FAMILY_ID {
                continue;
            }
            roots.push(attribution_for_catalog_family(
                &mut **file,
                page_count,
                descriptor,
                catalog_roots.get(&descriptor.family_id).copied(),
                max_examples,
            )?);
        }
        roots.push(attribution_for_page_run(
            "free_map",
            None,
            "physical_metadata",
            freemap.map(|(root, span)| (root.0, span)),
            max_examples,
        ));
        roots.push(attribution_for_page_run(
            "maintenance",
            None,
            "physical_metadata",
            maintenance_root.map(|root| (root.0, 1)),
            max_examples,
        ));
        let dek = self.dek.lock().map_err(|_| poisoned())?;
        roots.push(attribution_for_digest_root(
            &mut **file,
            page_count,
            dek.as_ref(),
            "reference_root",
            "reference",
            reference_root,
            &index_entries,
            max_examples,
        )?);
        roots.push(attribution_for_digest_root(
            &mut **file,
            page_count,
            dek.as_ref(),
            "control_root",
            "control",
            control_root,
            &index_entries,
            max_examples,
        )?);

        let mut object_reverse_ownership = object_index_reverse_ownership(
            &mut **file,
            page_count,
            self.digest_algo,
            &index_entries,
            max_examples,
        )?;
        walk_object_graph_attribution(
            &mut **file,
            page_count,
            dek.as_ref(),
            reference_root,
            "reference_root",
            "reference_object_graph",
            &index_entries,
            &mut object_reverse_ownership,
            max_examples,
        )?;
        walk_object_graph_attribution(
            &mut **file,
            page_count,
            dek.as_ref(),
            control_root,
            "control_root",
            "control_object_graph",
            &index_entries,
            &mut object_reverse_ownership,
            max_examples,
        )?;
        append_record_reverse_ownership(
            &mut **file,
            page_count,
            self.digest_algo,
            Some(CURRENT_RECORDS_FAMILY_ID),
            current_record_root,
            "current_records",
            "current_record",
            false,
            &mut object_reverse_ownership,
            max_examples,
        )?;
        for descriptor in ROOT_FAMILY_REGISTRY {
            if descriptor.family_id == CURRENT_RECORDS_FAMILY_ID {
                continue;
            }
            let rebuildable = descriptor.role == RootFamilyRole::RebuildableAdvisory;
            append_record_reverse_ownership(
                &mut **file,
                page_count,
                self.digest_algo,
                Some(descriptor.family_id),
                catalog_roots.get(&descriptor.family_id).copied(),
                descriptor.name,
                root_family_role(descriptor),
                rebuildable,
                &mut object_reverse_ownership,
                max_examples,
            )?;
        }
        drop(dek);
        drop(file);

        let mut stale_owner_reasons = match self.mutable_overlay_checkpoint_plan(max_examples) {
            Ok(plan) => concrete_stale_owner_reasons(&plan, max_examples),
            Err(err) => vec![StoreStaleOwnerReason {
                reason: "unknown_ownership".to_string(),
                pages: 0,
                bytes: 0,
                current_key: None,
                retained_sequence: None,
                examples: vec![format!("checkpoint_plan_unavailable:{err:?}")],
            }],
        };
        let mvcc = self.mvcc_snapshot_diagnostics()?;
        let audit_retention_active = self
            .audit_config()
            .map(|config| config.legal_hold)
            .unwrap_or(false);
        let durable_reclaim_floor = self.mutable_overlay_health()?.current_generation;
        let mut file = self.file.lock().map_err(|_| poisoned())?;
        stale_owner_reasons.extend(current_record_concrete_stale_owner_reasons(
            &mut **file,
            page_count,
            current_record_root,
            &mvcc,
            audit_retention_active,
            durable_reclaim_floor,
            max_examples,
        )?);
        drop(file);
        for class in self
            .page_class_attribution(max_examples)?
            .classes
            .into_iter()
            .filter(|class| {
                class.class.starts_with("stale_")
                    || class.class.starts_with("unreferenced_")
                    || class.class == "reusable_free_page"
                    || class.class == "tail_free_page"
            })
        {
            let reason = match class.class.as_str() {
                "reusable_free_page" => "pending_free_map_age".to_string(),
                class if class.starts_with("unreferenced_") => "unknown_ownership".to_string(),
                class => class.to_string(),
            };
            stale_owner_reasons.push(StoreStaleOwnerReason {
                reason,
                pages: class.pages,
                bytes: class.bytes,
                current_key: None,
                retained_sequence: None,
                examples: class.examples,
            });
        }

        Ok(StoreRootStorageAttribution {
            physical_bytes: DATA_START + page_count * PAGE_SIZE,
            page_size: PAGE_SIZE,
            data_pages: page_count,
            roots,
            object_reverse_ownership: object_reverse_ownership.into_values().collect(),
            stale_owner_reasons,
        })
    }

    pub fn mutable_overlay_checkpoint_plan(
        &self,
        max_examples: usize,
    ) -> Result<MutableOverlayCheckpointPlan> {
        self.mutable_overlay_checkpoint_plan_with_durable_floor(max_examples, None)
    }

    pub fn checkpoint_mutable_overlay_pages(
        &self,
        max_examples: usize,
    ) -> Result<MutableOverlayCheckpointWriteReport> {
        let _publication_guard = self.overlay_publication.lock().map_err(|_| poisoned())?;
        let plan = self.mutable_overlay_checkpoint_plan(max_examples)?;
        if plan.compactable_current_records == 0 || plan.stale_record_bytes == 0 {
            let status = self.store_maintenance_report(0)?;
            return Ok(MutableOverlayCheckpointWriteReport {
                planned_current_records: plan.current_record_count,
                compacted_current_records: 0,
                blocked_current_records: plan.blocked_current_records,
                rewritten_record_bytes: 0,
                freed_record_pages: 0,
                reusable_free_bytes: status.reusable_free_bytes,
                physical_page_count: status.status.physical_page_count,
            });
        }
        let compactable = plan
            .current_records
            .iter()
            .filter(|record| record.compactable)
            .map(|record| (record.key.clone(), record.generation))
            .collect::<BTreeMap<_, _>>();
        let mut inner = self.inner.lock().map_err(|_| poisoned())?;
        let current_root = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            match inner.current_record_root {
                Some(root) => Some(root),
                None => read_mutable_overlay_current_root(
                    &mut **file,
                    inner.overlay_root,
                    inner.page_count,
                )?,
            }
        };
        let Some(current_root) = current_root else {
            let physical_page_count = inner.maintenance.physical_page_count;
            drop(inner);
            let status = self.store_maintenance_report(0)?;
            return Ok(MutableOverlayCheckpointWriteReport {
                planned_current_records: plan.current_record_count,
                compacted_current_records: 0,
                blocked_current_records: plan.blocked_current_records,
                rewritten_record_bytes: 0,
                freed_record_pages: 0,
                reusable_free_bytes: status.reusable_free_bytes,
                physical_page_count,
            });
        };
        let new_gen = inner.generation + 1;
        let (reusable_free, _reclamation_lease) = self.transaction_reusable_free(
            &inner.free,
            inner.active_mark_epoch_reclaim_fence,
            inner.minimum_recoverable_generation,
        )?;
        let (roots, compacted_current_records, rewritten_record_bytes, freed_record_pages) = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            let mut alloc = PageAllocator::new_with_reusable_runs(
                inner.page_count,
                new_gen,
                inner.free.clone(),
                reusable_free,
            );
            alloc.install_metadata_bootstrap_reserve(&inner.metadata_bootstrap_reserve)?;
            let current_entries = root_family_load_all(
                &mut **file,
                CURRENT_RECORDS_FAMILY_ID,
                current_root,
                inner.page_count,
            )?;
            root_family_free_all(
                &mut **file,
                &mut alloc,
                CURRENT_RECORDS_FAMILY_ID,
                current_root,
                inner.page_count,
            )?;
            let mut compacted_current_records = 0u64;
            let mut rewritten_record_bytes = 0u64;
            let mut freed_record_pages = 0u64;
            let mut rewritten = Vec::<([u8; 32], Vec<u8>)>::new();
            let mut next_current_entries = Vec::<([u8; 32], RecordLoc)>::new();
            let mut freed_segments = BTreeSet::new();
            for (address, loc) in current_entries {
                let value = read_blob_from_loc(&mut **file, loc)?;
                let entry = decode_mutable_overlay_entry(&value)?;
                let rewrite = compactable
                    .get(&entry.key)
                    .is_some_and(|generation| *generation == entry.generation);
                if rewrite {
                    let pages =
                        record_io::blob_pages(&mut **file, loc.global_page(), inner.page_count)?;
                    for page in &pages {
                        alloc.free(PageId(*page), 1)?;
                        freed_segments.insert(page / page::PAGES_PER_SEGMENT);
                    }
                    let page_span = pages.len() as u64;
                    compacted_current_records += 1;
                    rewritten_record_bytes =
                        rewritten_record_bytes.saturating_add(value.len() as u64);
                    freed_record_pages = freed_record_pages.saturating_add(page_span);
                    rewritten.push((address, value));
                } else {
                    next_current_entries.push((address, loc));
                }
            }
            let borrowed = rewritten
                .iter()
                .map(|(address, value)| (*address, value.as_slice()))
                .collect::<Vec<_>>();
            let placements =
                record_io::write_dedicated_blob_pages(&mut **file, &mut alloc, &borrowed)?;
            next_current_entries.extend(placements);
            next_current_entries.sort_by_key(|entry| entry.0);
            let current_batch = pagebtree::batch_upsert(
                &mut **file,
                DATA_START,
                &mut alloc,
                None,
                &next_current_entries,
                inner.page_count,
            )?;
            #[cfg(any(test, feature = "test-hooks"))]
            observe_btree_batch(current_batch.stats);
            let next_current_root = current_batch.root;
            let roots = finish_txn(
                &mut **file,
                &mut alloc,
                new_gen,
                inner.maintenance.object_count,
                TxnRootInputs {
                    object_index: inner.index_root,
                    legacy_overlay: inner.overlay_root,
                    current_records: next_current_root,
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
                &freed_segments,
                (
                    inner.freemap,
                    inner.region_table_root,
                    inner.maintenance_root,
                ),
                inner.encryption_meta.clone(),
                self.digest_algo,
                Some(&self.group_commit_metrics),
            )?;
            (
                roots,
                compacted_current_records,
                rewritten_record_bytes,
                freed_record_pages,
            )
        };
        self.adopt_committed_roots_locked(&mut inner, roots)?;
        drop(inner);
        let status = self.store_maintenance_report(0)?;
        Ok(MutableOverlayCheckpointWriteReport {
            planned_current_records: plan.current_record_count,
            compacted_current_records,
            blocked_current_records: plan.blocked_current_records,
            rewritten_record_bytes,
            freed_record_pages,
            reusable_free_bytes: status.reusable_free_bytes,
            physical_page_count: status.status.physical_page_count,
        })
    }

    fn mutable_overlay_checkpoint_plan_with_durable_floor(
        &self,
        max_examples: usize,
        durable_reclaim_floor_override: Option<u64>,
    ) -> Result<MutableOverlayCheckpointPlan> {
        let overlay_health = self.mutable_overlay_health()?;
        let mvcc = self.mvcc_snapshot_diagnostics()?;
        let audit_retention_active = self.audit_config()?.legal_hold;
        let attribution = self.page_class_attribution(max_examples)?;
        let stale_record_bytes = attribution
            .classes
            .iter()
            .filter(|class| class.class.starts_with("stale_record_"))
            .map(|class| class.bytes)
            .sum();
        let reusable_free_bytes = attribution
            .classes
            .iter()
            .filter(|class| class.class == "reusable_free_page")
            .map(|class| class.bytes)
            .sum();
        let (overlay_root, current_record_root, page_count, durable_reclaim_floor) = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            (
                inner.overlay_root,
                inner.current_record_root,
                inner.page_count,
                durable_reclaim_floor_override.unwrap_or(overlay_health.current_generation),
            )
        };
        let mut current_records = Vec::new();
        let current_root = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            match current_record_root {
                Some(root) => Some(root),
                None => read_mutable_overlay_current_root(&mut **file, overlay_root, page_count)?,
            }
        };
        if let Some(current_root) = current_root {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            for (_address, loc) in root_family_load_all(
                &mut **file,
                CURRENT_RECORDS_FAMILY_ID,
                current_root,
                page_count,
            )? {
                let value = read_blob_from_loc(&mut **file, loc)?;
                let entry = decode_mutable_overlay_entry(&value)?;
                let generation = if entry.generation.as_u64() == 0 {
                    loom_core::OverlayGeneration::new(overlay_health.current_generation)
                } else {
                    entry.generation
                };
                let blockers = mutable_overlay_checkpoint_record_blockers(
                    generation,
                    entry.kind,
                    &mvcc,
                    audit_retention_active,
                    durable_reclaim_floor,
                );
                let page_span =
                    record_io::blob_pages(&mut **file, loc.global_page(), page_count)?.len() as u64;
                let compactable = blockers.is_empty();
                current_records.push(MutableOverlayCheckpointRecordPlan {
                    key: entry.key,
                    generation,
                    kind: entry.kind,
                    page_start: loc.global_page(),
                    page_span,
                    bytes: page_span.saturating_mul(PAGE_SIZE),
                    blockers,
                    compactable,
                });
            }
        }
        current_records.sort_by(|left, right| {
            left.generation
                .cmp(&right.generation)
                .then_with(|| left.key.cmp(&right.key))
        });
        let compactable_current_records = current_records
            .iter()
            .filter(|record| record.compactable)
            .count() as u64;
        let blocked_current_records = current_records.len() as u64 - compactable_current_records;
        let pinned_generations = mvcc
            .pins
            .iter()
            .map(|pin| pin.identity.overlay_generation)
            .collect();
        Ok(MutableOverlayCheckpointPlan {
            overlay_generation: loom_core::OverlayGeneration::new(
                overlay_health.current_generation,
            ),
            active_snapshot_count: mvcc.active_snapshot_count,
            oldest_pinned_generation: mvcc.oldest_pinned_overlay_generation,
            pinned_generations,
            current_record_count: overlay_health.current_record_count,
            tombstone_count: overlay_health.tombstone_count,
            compactable_current_records,
            blocked_current_records,
            stale_record_bytes,
            reusable_free_bytes,
            current_records,
        })
    }

    pub fn io_stats(&self) -> Result<StoreIoStats> {
        let inner = self.inner.lock().map_err(|_| poisoned())?;
        let mut stats = inner.io_stats.clone();
        stats.locator_cache_entries = inner.index.len() as u64;
        stats.index_page_cache_entries = inner.index_page_cache.len() as u64;
        Ok(stats)
    }

    /// Record (or clear) the engine-state (reference) root, committing a new superblock generation. No
    /// object data is appended; only the reference field changes, atomically via the two-slot swap.
    pub fn set_reference_root(&self, root: Option<Digest>) -> Result<()> {
        // A reference-root change carries no objects, so it commits directly rather than through the
        // group-commit queue (that queue coalesces object writes); the inner lock still serializes it.
        self.commit_txn(&[], Some(root.map(|d| *d.bytes())), None, None)
    }

    /// The durable-local control-plane root digest recorded in the committed superblock, if any.
    pub fn control_root(&self) -> Option<Digest> {
        self.inner.lock().ok().and_then(|i| i.control_root)
    }

    /// Record (or clear) the durable-local control-plane root. This root is outside the engine
    /// reference tree: workspace commits, bundles, clone, and sync do not see it.
    pub fn set_control_root(&self, root: Option<Digest>) -> Result<()> {
        self.commit_txn(&[], None, Some(root.map(|d| *d.bytes())), None)
    }

    fn cache_locator_locked(inner: &mut Inner, key: [u8; 32], loc: RecordLoc) {
        let known = inner.index.contains_key(&key);
        inner.index.insert(key, loc);
        if inner.index_materialized {
            return;
        }
        if !known {
            inner.locator_cache_order.push_back(key);
        }
        while inner.index.len() > LOCATOR_CACHE_LIMIT {
            let Some(evict) = inner.locator_cache_order.pop_front() else {
                break;
            };
            if evict != key {
                inner.index.remove(&evict);
            }
        }
    }

    fn cache_index_page_locked(inner: &mut Inner, page: PageId, bytes: [u8; PAGE_SIZE as usize]) {
        let known = inner.index_page_cache.contains_key(&page);
        inner.index_page_cache.insert(page, bytes);
        if !known {
            inner.index_page_cache_order.push_back(page);
        }
        while inner.index_page_cache.len() > INDEX_PAGE_CACHE_LIMIT {
            let Some(evict) = inner.index_page_cache_order.pop_front() else {
                break;
            };
            if evict != page {
                inner.index_page_cache.remove(&evict);
            }
        }
    }

    fn clear_index_page_cache_locked(inner: &mut Inner) {
        inner.index_page_cache.clear();
        inner.index_page_cache_order.clear();
    }

    #[cfg(test)]
    fn set_post_commit_pre_adopt_hook_for_test(&self, hook: PostCommitPreAdoptHook) -> Result<()> {
        let mut slot = self
            .post_commit_pre_adopt_hook
            .0
            .lock()
            .map_err(|_| poisoned())?;
        *slot = Some(hook);
        Ok(())
    }

    #[cfg(test)]
    fn set_source_layout_activation_pre_finish_hook_for_test(
        &self,
        hook: SourceLayoutActivationPreFinishHook,
    ) -> Result<()> {
        let mut slot = self
            .source_layout_activation_pre_finish_hook
            .0
            .lock()
            .map_err(|_| poisoned())?;
        *slot = Some(hook);
        Ok(())
    }

    #[cfg(test)]
    fn set_reachability_epoch_pre_finish_hook_for_test(
        &self,
        hook: ReachabilityEpochPreFinishHook,
    ) -> Result<()> {
        let mut slot = self
            .reachability_epoch_pre_finish_hook
            .0
            .lock()
            .map_err(|_| poisoned())?;
        *slot = Some(hook);
        Ok(())
    }

    #[cfg(test)]
    fn set_source_layout_preflight_after_discovery_hook_for_test(
        &self,
        hook: SourceLayoutPreflightAfterDiscoveryHook,
    ) -> Result<()> {
        let mut slot = self
            .source_layout_preflight_after_discovery_hook
            .0
            .lock()
            .map_err(|_| poisoned())?;
        *slot = Some(hook);
        Ok(())
    }

    #[cfg(test)]
    fn run_post_commit_pre_adopt_hook(&self, roots: &TxnRoots) -> Result<()> {
        let hook = self
            .post_commit_pre_adopt_hook
            .0
            .lock()
            .map_err(|_| poisoned())?
            .take();
        if let Some(hook) = hook {
            hook(roots)?;
        }
        Ok(())
    }

    #[cfg(test)]
    fn run_source_layout_activation_pre_finish_hook(&self) -> Result<()> {
        let hook = self
            .source_layout_activation_pre_finish_hook
            .0
            .lock()
            .map_err(|_| poisoned())?
            .take();
        if let Some(hook) = hook {
            hook()?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn run_reachability_epoch_pre_finish_hook(&self) -> Result<()> {
        let hook = self
            .reachability_epoch_pre_finish_hook
            .0
            .lock()
            .map_err(|_| poisoned())?
            .take();
        if let Some(hook) = hook {
            hook()?;
        }
        Ok(())
    }

    #[cfg(test)]
    fn run_source_layout_preflight_after_discovery_hook(&self) -> Result<()> {
        let hook = self
            .source_layout_preflight_after_discovery_hook
            .0
            .lock()
            .map_err(|_| poisoned())?
            .take();
        if let Some(hook) = hook {
            hook(&self.inner)?;
        }
        Ok(())
    }

    #[cfg(test)]
    fn reset_audit_retention_instrumentation_for_test(&self) {
        self.audit_retention_test_instrumentation.reset();
    }

    #[cfg(test)]
    fn audit_retention_point_write_counts_for_test(&self) -> (u64, u64) {
        self.audit_retention_test_instrumentation
            .point_write_counts()
    }

    #[cfg(test)]
    fn audit_retention_full_family_enumerations_for_test(&self) -> u64 {
        self.audit_retention_test_instrumentation
            .full_family_enumerations()
    }

    #[cfg(not(test))]
    fn run_post_commit_pre_adopt_hook(&self, _roots: &TxnRoots) -> Result<()> {
        Ok(())
    }

    #[cfg(not(test))]
    pub(crate) fn run_reachability_epoch_pre_finish_hook(&self) -> Result<()> {
        Ok(())
    }

    fn adopt_committed_roots_locked(&self, inner: &mut Inner, roots: TxnRoots) -> Result<()> {
        self.run_post_commit_pre_adopt_hook(&roots)?;
        let previous_index_root = inner.index_root;
        let family_roots = root_catalog_family_roots(&roots.root_catalog.entries);
        inner.generation = roots.generation;
        inner.page_count = roots.page_count;
        inner.index_root = roots.object_index;
        inner.overlay_root = roots.legacy_overlay;
        inner.current_record_root = roots.current_record_root;
        inner.root_catalog_root = roots.root_catalog.root;
        inner.root_catalog_entries = roots.root_catalog.entries;
        inner.mutable_overlay_generation_floor = roots.mutable_overlay_generation_floor;
        inner.minimum_recoverable_generation = roots.minimum_recoverable_generation;
        inner.reference_root = roots
            .reference
            .map(|bytes| Digest::of(self.digest_algo, bytes));
        inner.control_root = roots
            .control
            .map(|bytes| Digest::of(self.digest_algo, bytes));
        inner.retained_history_root = family_roots.retained_history;
        inner.owner_token_root = family_roots.owner_token;
        inner.secondary_index_root = family_roots.secondary_index;
        inner.mutable_idempotency_root = family_roots.mutable_idempotency;
        inner.workflow_idempotency_root = family_roots.workflow_idempotency;
        inner.audit_retention_root = family_roots.audit_retention;
        inner.mvcc_generation_root = family_roots.mvcc_generation;
        inner.retention_index_root = family_roots.retention_index;
        inner.checkpoint_index_root = family_roots.checkpoint_index;
        inner.reclaim_index_root = family_roots.reclaim_index;
        inner.free = roots.free;
        inner.metadata_bootstrap_reserve = roots.metadata_bootstrap_reserve;
        inner.freemap = roots.freemap;
        inner.region_table_root = Some(roots.region_table_root);
        inner.maintenance_root = Some(roots.maintenance_root);
        inner.maintenance = roots.maintenance;
        if previous_index_root != inner.index_root {
            Self::clear_index_page_cache_locked(inner);
        }
        Ok(())
    }

    fn lookup_loc_locked(&self, inner: &mut Inner, key: &[u8; 32]) -> Result<Option<RecordLoc>> {
        if let Some(&loc) = inner.index.get(key) {
            if inner.index_materialized {
                inner.io_stats.materialized_index_lookup_count += 1;
            } else {
                inner.io_stats.locator_cache_hits += 1;
                inner.io_stats.sparse_index_lookup_count += 1;
            }
            return Ok(Some(loc));
        }
        inner.io_stats.locator_cache_misses += 1;
        inner.io_stats.sparse_index_lookup_count += 1;
        let Some(root) = inner.index_root else {
            return Ok(None);
        };
        let mut file = self.file.lock().map_err(|_| poisoned())?;
        let page_count = inner.page_count;
        let loc = pagebtree::get_with_page_reader(Some(root), key, page_count, |page| {
            if let Some(bytes) = inner.index_page_cache.get(&page) {
                inner.io_stats.index_page_cache_hits += 1;
                return Ok(*bytes);
            }
            let mut bytes = [0u8; PAGE_SIZE as usize];
            read_exact_at(&mut **file, page.offset(DATA_START), &mut bytes)
                .map_err(|_| corrupt("truncated btree node page"))?;
            inner.io_stats.index_pages_read += 1;
            inner.io_stats.index_page_cache_misses += 1;
            Self::cache_index_page_locked(inner, page, bytes);
            Ok(bytes)
        })?;
        if let Some(loc) = loc {
            Self::cache_locator_locked(inner, *key, loc);
        }
        Ok(loc)
    }

    fn materialize_index_locked(&self, inner: &mut Inner) -> Result<()> {
        if inner.index_materialized {
            return Ok(());
        }
        let mut index = BTreeMap::new();
        if let Some(root) = inner.index_root {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            for (key, loc) in pagebtree::load_all(&mut **file, DATA_START, root, inner.page_count)?
            {
                index.insert(key, loc);
            }
        }
        inner.index = index;
        inner.locator_cache_order.clear();
        Self::clear_index_page_cache_locked(inner);
        inner.index_materialized = true;
        Ok(())
    }

    /// Read one durable-local control-plane value.
    pub fn control_get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        if is_audit_retention_control_key(key) {
            self.audit_retention_record_payload(key)
        } else {
            Ok(self.control_root_map()?.get(key).cloned())
        }
    }

    /// Set one durable-local control-plane value.
    pub fn control_set(&self, key: &[u8], value: Vec<u8>) -> Result<()> {
        if is_audit_retention_control_key(key) {
            let mut audit_delta = AuditRetentionDelta::default();
            audit_delta.put(key, value);
            self.commit_control_delta_and_audit_retention(
                BTreeMap::new(),
                BTreeSet::new(),
                audit_delta,
                None,
            )
        } else {
            let mut control_puts = BTreeMap::new();
            control_puts.insert(key.to_vec(), value);
            self.commit_control_delta_and_audit_retention(
                control_puts,
                BTreeSet::new(),
                AuditRetentionDelta::default(),
                None,
            )
        }
    }

    pub fn control_set_audited(
        &self,
        key: &[u8],
        value: Vec<u8>,
        principal: Option<WorkspaceId>,
        action: &str,
        target: Option<&str>,
    ) -> Result<u64> {
        let mut control_puts = BTreeMap::new();
        control_puts.insert(key.to_vec(), value);
        let mut audit_delta = AuditRetentionDelta::default();
        let seq = self.append_audit_record_delta(&mut audit_delta, principal, action, target)?;
        self.commit_control_delta_and_audit_retention(
            control_puts,
            BTreeSet::new(),
            audit_delta,
            None,
        )?;
        Ok(seq)
    }

    /// Atomically set one control-plane value AND the reference (engine working-tree) root in a
    /// single superblock commit. Callers that must keep an indexed-table root and a control-plane
    /// record consistent (e.g. the ticket profile state versus its indexed tables) use this so a
    /// successful write can never leave a mixed committed state: an interruption exposes either the
    /// old or the new superblock, never one root advanced without the other.
    pub fn control_set_with_reference(
        &self,
        key: &[u8],
        value: Vec<u8>,
        reference_root: Option<Digest>,
    ) -> Result<()> {
        if is_audit_retention_control_key(key) {
            let mut audit_delta = AuditRetentionDelta::default();
            audit_delta.put(key, value);
            self.commit_control_delta_and_audit_retention(
                BTreeMap::new(),
                BTreeSet::new(),
                audit_delta,
                Some(reference_root),
            )
        } else {
            let mut control_puts = BTreeMap::new();
            control_puts.insert(key.to_vec(), value);
            self.commit_control_delta_and_audit_retention(
                control_puts,
                BTreeSet::new(),
                AuditRetentionDelta::default(),
                Some(reference_root),
            )
        }
    }

    /// Audited variant of [`control_set_with_reference`]: records an audit entry for the control-plane
    /// mutation and commits it together with the reference root in one superblock.
    pub fn control_set_audited_with_reference(
        &self,
        key: &[u8],
        value: Vec<u8>,
        principal: Option<WorkspaceId>,
        action: &str,
        target: Option<&str>,
        reference_root: Option<Digest>,
    ) -> Result<u64> {
        let mut control_puts = BTreeMap::new();
        control_puts.insert(key.to_vec(), value);
        let mut audit_delta = AuditRetentionDelta::default();
        let seq = self.append_audit_record_delta(&mut audit_delta, principal, action, target)?;
        self.commit_control_delta_and_audit_retention(
            control_puts,
            BTreeSet::new(),
            audit_delta,
            Some(reference_root),
        )?;
        Ok(seq)
    }

    /// Delete one durable-local control-plane value; returns whether it was present.
    pub fn control_delete(&self, key: &[u8]) -> Result<bool> {
        let present = self.control_get(key)?.is_some();
        if present {
            if is_audit_retention_control_key(key) {
                let mut audit_delta = AuditRetentionDelta::default();
                audit_delta.delete(key.to_vec());
                self.commit_control_delta_and_audit_retention(
                    BTreeMap::new(),
                    BTreeSet::new(),
                    audit_delta,
                    None,
                )?;
            } else {
                self.commit_control_delta_and_audit_retention(
                    BTreeMap::new(),
                    BTreeSet::from([key.to_vec()]),
                    AuditRetentionDelta::default(),
                    None,
                )?;
            }
        }
        Ok(present)
    }

    /// Durable-local control-plane entries matching `prefix`, in key order.
    pub fn control_scan_prefix(&self, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        Ok(self
            .control_map()?
            .into_iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .collect())
    }

    /// Restore the embedded lock coordinator's durable-local fence state.
    pub fn lock_coordinator(&self) -> Result<LockCoordinator> {
        let next = self.decode_lock_fence_records(LOCK_NEXT_FENCE_PREFIX)?;
        let applied = self.decode_lock_fence_records(LOCK_APPLIED_FENCE_PREFIX)?;
        Ok(LockCoordinator::restore_fences(next, applied))
    }

    /// Persist the embedded lock coordinator's durable-local fence state.
    pub fn save_lock_coordinator(&self, coordinator: &LockCoordinator) -> Result<()> {
        let mut map = self.control_root_map()?;
        map.retain(|key, _| {
            !key.starts_with(LOCK_NEXT_FENCE_PREFIX) && !key.starts_with(LOCK_APPLIED_FENCE_PREFIX)
        });
        for (key, fence) in coordinator.fence_counters() {
            map.insert(
                lock_control_key(LOCK_NEXT_FENCE_PREFIX, &key),
                fence.to_be_bytes().to_vec(),
            );
        }
        for (key, fence) in coordinator.applied_fences() {
            map.insert(
                lock_control_key(LOCK_APPLIED_FENCE_PREFIX, &key),
                fence.to_be_bytes().to_vec(),
            );
        }
        self.write_control_map(map)
    }

    /// Restore the persisted principal registry, if one has been initialized.
    pub fn identity_store(&self) -> Result<Option<IdentityStore>> {
        self.control_get(IDENTITY_STORE_KEY)?
            .map(|bytes| IdentityStore::decode(&bytes))
            .transpose()
    }

    /// Persist the principal registry snapshot outside workspace history.
    pub fn save_identity_store(&self, identity: &IdentityStore) -> Result<()> {
        self.control_set(IDENTITY_STORE_KEY, identity.encode())
    }

    pub fn save_identity_store_audited(
        &self,
        identity: &IdentityStore,
        principal: Option<WorkspaceId>,
        action: &str,
        target: Option<&str>,
    ) -> Result<u64> {
        let mut control_puts = BTreeMap::new();
        control_puts.insert(IDENTITY_STORE_KEY.to_vec(), identity.encode());
        let mut audit_delta = AuditRetentionDelta::default();
        let seq = self.append_audit_record_delta(&mut audit_delta, principal, action, target)?;
        self.commit_control_delta_and_audit_retention(
            control_puts,
            BTreeSet::new(),
            audit_delta,
            None,
        )?;
        Ok(seq)
    }

    pub fn save_identity_store_and_authority_replication_policy_audited(
        &self,
        identity: &IdentityStore,
        policy: &AuthorityReplicationPolicy,
        principal: Option<WorkspaceId>,
        action: &str,
        target: Option<&str>,
    ) -> Result<u64> {
        validate_authority_replication_policy(policy)?;
        let mut audit_delta = AuditRetentionDelta::default();
        let seq = self.append_audit_record_delta(&mut audit_delta, principal, action, target)?;
        let mut stored = policy.clone();
        stored.schema_version = AUTHORITY_REPLICATION_SCHEMA_VERSION;
        stored.last_modified_audit_seq = Some(seq);
        let mut control_puts = BTreeMap::new();
        control_puts.insert(IDENTITY_STORE_KEY.to_vec(), identity.encode());
        control_puts.insert(
            authority_replication_key(&stored.id),
            encode_authority_replication_policy(&stored),
        );
        self.commit_control_delta_and_audit_retention(
            control_puts,
            BTreeSet::new(),
            audit_delta,
            None,
        )?;
        Ok(seq)
    }

    /// Restore the persisted ACL grant snapshot, if one has been initialized.
    pub fn acl_store(&self) -> Result<Option<AclStore>> {
        self.control_get(ACL_STORE_KEY)?
            .map(|bytes| AclStore::decode(&bytes))
            .transpose()
    }

    /// Persist the ACL grant snapshot outside workspace history.
    pub fn save_acl_store(&self, acl: &AclStore) -> Result<()> {
        self.control_set(ACL_STORE_KEY, acl.encode())
    }

    pub fn acl_store_control_write(&self, acl: &AclStore) -> loom_core::WorkflowControlWrite {
        loom_core::WorkflowControlWrite::Put {
            key: ACL_STORE_KEY.to_vec(),
            payload: acl.encode(),
        }
    }

    pub fn save_acl_store_audited(
        &self,
        acl: &AclStore,
        principal: Option<WorkspaceId>,
        action: &str,
        target: Option<&str>,
    ) -> Result<u64> {
        let mut control_puts = BTreeMap::new();
        control_puts.insert(ACL_STORE_KEY.to_vec(), acl.encode());
        let mut audit_delta = AuditRetentionDelta::default();
        let seq = self.append_audit_record_delta(&mut audit_delta, principal, action, target)?;
        self.commit_control_delta_and_audit_retention(
            control_puts,
            BTreeSet::new(),
            audit_delta,
            None,
        )?;
        Ok(seq)
    }

    pub fn audit_append(
        &self,
        principal: Option<WorkspaceId>,
        action: &str,
        target: Option<&str>,
    ) -> Result<u64> {
        let mut audit_delta = AuditRetentionDelta::default();
        let seq = self.append_audit_record_delta(&mut audit_delta, principal, action, target)?;
        self.commit_control_delta_and_audit_retention(
            BTreeMap::new(),
            BTreeSet::new(),
            audit_delta,
            None,
        )?;
        Ok(seq)
    }

    fn append_audit_record_delta(
        &self,
        delta: &mut AuditRetentionDelta,
        principal: Option<WorkspaceId>,
        action: &str,
        target: Option<&str>,
    ) -> Result<u64> {
        validate_audit_field("audit action", action.as_bytes(), 128)?;
        if let Some(target) = target {
            validate_audit_field("audit target", target.as_bytes(), 1024)?;
        }
        let next_value = self.audit_delta_payload(delta, AUDIT_NEXT_KEY)?;
        let seq = match next_value {
            Some(value) => decode_audit_next(&value)?,
            None => 0,
        };
        let prev_hash = if seq == 0 {
            None
        } else {
            let prev_key = audit_entry_key(seq - 1);
            let prev_value = self.audit_delta_payload(delta, &prev_key)?;
            match prev_value {
                Some(prev_value) => {
                    Some(decode_audit_value(seq - 1, &prev_value, self.digest_algo)?.hash)
                }
                None => {
                    let checkpoint_value =
                        self.audit_delta_payload(delta, AUDIT_PRUNE_CHECKPOINT_KEY)?;
                    match checkpoint_value
                        .map(|bytes| decode_audit_checkpoint(&bytes, self.digest_algo))
                        .transpose()?
                    {
                        Some(checkpoint) if checkpoint.seq == seq - 1 => Some(checkpoint.hash),
                        _ => return Err(corrupt("audit chain previous entry missing")),
                    }
                }
            }
        };
        let value = encode_audit_value(self.digest_algo, seq, prev_hash, principal, action, target);
        delta.put(&audit_entry_key(seq), value);
        let next = seq
            .checked_add(1)
            .ok_or_else(|| corrupt("audit sequence overflow"))?;
        delta.put(AUDIT_NEXT_KEY, next.to_be_bytes().to_vec());
        Ok(seq)
    }

    fn audit_delta_payload(
        &self,
        delta: &AuditRetentionDelta,
        key: &[u8],
    ) -> Result<Option<Vec<u8>>> {
        if let Some(value) = delta.puts.get(key) {
            return Ok(Some(value.clone()));
        }
        if delta.deletes.contains(key) {
            return Ok(None);
        }
        self.audit_retention_record_payload(key)
    }

    pub fn audit_config(&self) -> Result<AuditConfig> {
        self.audit_retention_record_payload(AUDIT_CONFIG_KEY)?
            .map(|bytes| decode_audit_config(&bytes))
            .transpose()
            .map(|value| value.unwrap_or_default())
    }

    pub fn store_policy(&self) -> Result<StorePolicy> {
        self.control_get(STORE_POLICY_KEY)?
            .map(|bytes| decode_store_policy(&bytes))
            .transpose()
            .map(|value| value.unwrap_or_default())
    }

    pub fn validate_runtime_policy(&self) -> Result<()> {
        let profile = loom_core::runtime_profile();
        if profile.fips_capable && self.digest_algo() != Algo::Sha256 {
            return Err(LoomError::new(
                Code::PermissionDenied,
                "FIPS runtime requires a FIPS-profile store",
            ));
        }
        if self.store_policy()?.fips_required && !profile.fips_capable {
            return Err(LoomError::new(
                Code::PermissionDenied,
                "FIPS-required stores cannot be opened by the current non-FIPS runtime",
            ));
        }
        Ok(())
    }

    pub fn save_store_policy_audited(
        &self,
        policy: StorePolicy,
        principal: Option<WorkspaceId>,
        action: &str,
        target: Option<&str>,
    ) -> Result<u64> {
        let mut control_puts = BTreeMap::new();
        control_puts.insert(STORE_POLICY_KEY.to_vec(), encode_store_policy(policy));
        let mut audit_delta = AuditRetentionDelta::default();
        let seq = self.append_audit_record_delta(&mut audit_delta, principal, action, target)?;
        self.commit_control_delta_and_audit_retention(
            control_puts,
            BTreeSet::new(),
            audit_delta,
            None,
        )?;
        Ok(seq)
    }

    pub fn save_audit_config_audited(
        &self,
        config: AuditConfig,
        principal: Option<WorkspaceId>,
        action: &str,
        target: Option<&str>,
    ) -> Result<u64> {
        let mut audit_delta = AuditRetentionDelta::default();
        audit_delta.put(AUDIT_CONFIG_KEY, encode_audit_config(config));
        let seq = self.append_audit_record_delta(&mut audit_delta, principal, action, target)?;
        self.commit_control_delta_and_audit_retention(
            BTreeMap::new(),
            BTreeSet::new(),
            audit_delta,
            None,
        )?;
        Ok(seq)
    }

    pub fn audit_records(&self) -> Result<Vec<AuditRecord>> {
        let map = self.audit_retention_map()?;
        let checkpoint = map
            .get(AUDIT_PRUNE_CHECKPOINT_KEY)
            .map(|bytes| decode_audit_checkpoint(bytes, self.digest_algo))
            .transpose()?;
        map.into_iter()
            .filter(|(key, _)| key.starts_with(AUDIT_ENTRY_PREFIX))
            .map(|(key, value)| decode_audit_entry(&key, &value, self.digest_algo))
            .collect::<Result<Vec<_>>>()
            .and_then(|records| verify_audit_chain(records, checkpoint))
    }

    pub fn audit_prune_through(
        &self,
        principal: Option<WorkspaceId>,
        through_seq: u64,
    ) -> Result<AuditPruneStats> {
        let config = self
            .audit_retention_record_payload(AUDIT_CONFIG_KEY)?
            .as_deref()
            .map(decode_audit_config)
            .transpose()?
            .unwrap_or_default();
        if config.legal_hold {
            return Err(LoomError::new(
                Code::PermissionDenied,
                "audit legal hold prevents pruning",
            ));
        }
        let prior_checkpoint = self
            .audit_retention_record_payload(AUDIT_PRUNE_CHECKPOINT_KEY)?
            .map(|bytes| decode_audit_checkpoint(&bytes, self.digest_algo))
            .transpose()?;
        let start_seq = prior_checkpoint
            .map(|checkpoint| checkpoint.seq.saturating_add(1))
            .unwrap_or(0);
        let mut records = Vec::new();
        for seq in start_seq..=through_seq {
            let key = audit_entry_key(seq);
            if let Some(value) = self.audit_retention_record_payload(&key)? {
                records.push(decode_audit_entry(&key, &value, self.digest_algo)?);
            }
        }
        let checkpoint = records
            .iter()
            .max_by_key(|record| record.seq)
            .map(|record| AuditCheckpoint {
                seq: record.seq,
                hash: record.hash,
            });
        let Some(checkpoint) = checkpoint else {
            let target = format!("through_seq={through_seq};pruned=0");
            let mut audit_delta = AuditRetentionDelta::default();
            let audit_seq = self.append_audit_record_delta(
                &mut audit_delta,
                principal,
                "audit.prune",
                Some(&target),
            )?;
            self.commit_control_delta_and_audit_retention(
                BTreeMap::new(),
                BTreeSet::new(),
                audit_delta,
                None,
            )?;
            return Ok(AuditPruneStats {
                pruned: 0,
                checkpoint_seq: None,
                checkpoint_hash: None,
                audit_seq,
            });
        };
        let mut pruned = 0u64;
        let mut audit_delta = AuditRetentionDelta::default();
        for record in &records {
            if record.seq <= checkpoint.seq {
                audit_delta.delete(audit_entry_key(record.seq));
                pruned += 1;
            }
        }
        audit_delta.put(
            AUDIT_PRUNE_CHECKPOINT_KEY,
            encode_audit_checkpoint(checkpoint),
        );
        let target = format!("through_seq={};pruned={pruned}", checkpoint.seq);
        let audit_seq = self.append_audit_record_delta(
            &mut audit_delta,
            principal,
            "audit.prune",
            Some(&target),
        )?;
        self.commit_control_delta_and_audit_retention(
            BTreeMap::new(),
            BTreeSet::new(),
            audit_delta,
            None,
        )?;
        Ok(AuditPruneStats {
            pruned,
            checkpoint_seq: Some(checkpoint.seq),
            checkpoint_hash: Some(checkpoint.hash),
            audit_seq,
        })
    }

    pub fn certificate_bundle_record(
        &self,
        name: &str,
        server_cert_chain_pem: Vec<u8>,
        private_key_pem: Vec<u8>,
        trust_bundle_pem: Option<Vec<u8>>,
    ) -> Result<CertificateBundleRecord> {
        let record = CertificateBundleRecord {
            name: name.to_string(),
            schema_version: CERTIFICATE_BUNDLE_SCHEMA_VERSION,
            profile: "tls-server-direct".to_string(),
            server_cert_chain_digest: Digest::hash(self.digest_algo, &server_cert_chain_pem),
            private_key_digest: Digest::hash(self.digest_algo, &private_key_pem),
            trust_bundle_digest: trust_bundle_pem
                .as_ref()
                .map(|bytes| Digest::hash(self.digest_algo, bytes)),
            server_cert_chain_pem,
            private_key_pem,
            trust_bundle_pem,
            created_audit_seq: None,
            updated_audit_seq: None,
            unencrypted_private_key_override: false,
        };
        validate_certificate_bundle_record(&record)?;
        Ok(record)
    }

    pub fn certificate_bundles(&self) -> Result<Vec<CertificateBundleRecord>> {
        let mut out = self
            .control_scan_prefix(CERTIFICATE_BUNDLE_PREFIX)?
            .into_iter()
            .map(|(key, value)| decode_certificate_bundle_entry(&key, &value, self.digest_algo))
            .collect::<Result<Vec<_>>>()?;
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    pub fn certificate_bundle(&self, name: &str) -> Result<Option<CertificateBundleRecord>> {
        validate_certificate_bundle_name(name)?;
        self.control_get(&certificate_bundle_key(name))?
            .map(|value| decode_certificate_bundle(&value, self.digest_algo))
            .transpose()
    }

    pub fn save_certificate_bundle_audited(
        &self,
        record: &CertificateBundleRecord,
        principal: Option<WorkspaceId>,
        action: &str,
        target: Option<&str>,
        force_unencrypted_private_key: bool,
    ) -> Result<u64> {
        validate_certificate_bundle_record(record)?;
        if !self.is_encrypted() && !force_unencrypted_private_key {
            return Err(LoomError::new(
                Code::PermissionDenied,
                "certificate bundle private key import requires an encrypted store or --force",
            ));
        }
        let key = certificate_bundle_key(&record.name);
        let existing = self
            .control_get(&key)?
            .as_deref()
            .map(|value| decode_certificate_bundle(value, self.digest_algo))
            .transpose()?;
        let mut audit_delta = AuditRetentionDelta::default();
        let seq = self.append_audit_record_delta(&mut audit_delta, principal, action, target)?;
        let mut stored = record.clone();
        stored.schema_version = CERTIFICATE_BUNDLE_SCHEMA_VERSION;
        stored.created_audit_seq = existing
            .as_ref()
            .and_then(|value| value.created_audit_seq)
            .or(Some(seq));
        stored.updated_audit_seq = Some(seq);
        stored.unencrypted_private_key_override =
            !self.is_encrypted() && force_unencrypted_private_key;
        let mut control_puts = BTreeMap::new();
        control_puts.insert(key, encode_certificate_bundle(&stored));
        self.commit_control_delta_and_audit_retention(
            control_puts,
            BTreeSet::new(),
            audit_delta,
            None,
        )?;
        Ok(seq)
    }

    pub fn remove_certificate_bundle_audited(
        &self,
        name: &str,
        principal: Option<WorkspaceId>,
        action: &str,
        target: Option<&str>,
    ) -> Result<u64> {
        validate_certificate_bundle_name(name)?;
        let key = certificate_bundle_key(name);
        if self.control_get(&key)?.is_none() {
            return Err(LoomError::not_found("certificate bundle not found"));
        }
        let mut audit_delta = AuditRetentionDelta::default();
        let seq = self.append_audit_record_delta(&mut audit_delta, principal, action, target)?;
        self.commit_control_delta_and_audit_retention(
            BTreeMap::new(),
            BTreeSet::from([key]),
            audit_delta,
            None,
        )?;
        Ok(seq)
    }

    pub fn network_access_policy_record(
        name: &str,
        description: Option<String>,
        default_action: NetworkAccessAction,
        rules: Vec<NetworkAccessRule>,
    ) -> Result<NetworkAccessPolicyRecord> {
        let record = NetworkAccessPolicyRecord {
            name: name.to_string(),
            schema_version: NETWORK_ACCESS_POLICY_SCHEMA_VERSION,
            description,
            default_action,
            rules,
            created_audit_seq: None,
            updated_audit_seq: None,
        };
        validate_network_access_policy_record(&record)?;
        Ok(record)
    }

    pub fn network_access_policies(&self) -> Result<Vec<NetworkAccessPolicyRecord>> {
        let mut out = self
            .control_scan_prefix(NETWORK_ACCESS_POLICY_PREFIX)?
            .into_iter()
            .map(|(key, value)| decode_network_access_policy_entry(&key, &value))
            .collect::<Result<Vec<_>>>()?;
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    pub fn network_access_policy(&self, name: &str) -> Result<Option<NetworkAccessPolicyRecord>> {
        validate_network_access_policy_name(name)?;
        self.control_get(&network_access_policy_key(name))?
            .map(|value| decode_network_access_policy(&value))
            .transpose()
    }

    pub fn network_access_policy_digest(
        &self,
        record: &NetworkAccessPolicyRecord,
    ) -> Result<Digest> {
        validate_network_access_policy_record(record)?;
        Ok(Digest::hash(
            self.digest_algo,
            &encode_network_access_policy(record),
        ))
    }

    pub fn save_network_access_policy_audited(
        &self,
        record: &NetworkAccessPolicyRecord,
        principal: Option<WorkspaceId>,
        action: &str,
        target: Option<&str>,
    ) -> Result<u64> {
        validate_network_access_policy_record(record)?;
        let key = network_access_policy_key(&record.name);
        let existing = self
            .control_get(&key)?
            .as_deref()
            .map(decode_network_access_policy)
            .transpose()?;
        let mut audit_delta = AuditRetentionDelta::default();
        let seq = self.append_audit_record_delta(&mut audit_delta, principal, action, target)?;
        let mut stored = record.clone();
        stored.schema_version = NETWORK_ACCESS_POLICY_SCHEMA_VERSION;
        stored.created_audit_seq = existing
            .as_ref()
            .and_then(|value| value.created_audit_seq)
            .or(Some(seq));
        stored.updated_audit_seq = Some(seq);
        let mut control_puts = BTreeMap::new();
        control_puts.insert(key, encode_network_access_policy(&stored));
        self.commit_control_delta_and_audit_retention(
            control_puts,
            BTreeSet::new(),
            audit_delta,
            None,
        )?;
        Ok(seq)
    }

    pub fn remove_network_access_policy_audited(
        &self,
        name: &str,
        principal: Option<WorkspaceId>,
        action: &str,
        target: Option<&str>,
    ) -> Result<u64> {
        validate_network_access_policy_name(name)?;
        let key = network_access_policy_key(name);
        if self.control_get(&key)?.is_none() {
            return Err(LoomError::not_found("network access policy not found"));
        }
        let mut audit_delta = AuditRetentionDelta::default();
        let seq = self.append_audit_record_delta(&mut audit_delta, principal, action, target)?;
        self.commit_control_delta_and_audit_retention(
            BTreeMap::new(),
            BTreeSet::from([key]),
            audit_delta,
            None,
        )?;
        Ok(seq)
    }

    pub fn served_listener_record(
        surface: &str,
        selectors: Vec<String>,
        transport: &str,
        bind: &str,
        enabled: bool,
    ) -> Result<ServedListenerRecord> {
        Self::served_listener_record_with_profile(
            surface, selectors, transport, None, bind, enabled,
        )
    }

    pub fn served_listener_record_with_profile(
        surface: &str,
        selectors: Vec<String>,
        transport: &str,
        profile: Option<&str>,
        bind: &str,
        enabled: bool,
    ) -> Result<ServedListenerRecord> {
        validate_served_listener_field("served listener surface", surface.as_bytes(), 64)?;
        validate_served_listener_field("served listener transport", transport.as_bytes(), 64)?;
        if let Some(profile) = profile {
            validate_served_listener_field("served listener profile", profile.as_bytes(), 64)?;
        }
        validate_served_listener_field("served listener bind", bind.as_bytes(), 256)?;
        for selector in &selectors {
            validate_served_listener_field("served listener selector", selector.as_bytes(), 256)?;
        }
        let id = served_listener_id_with_profile(surface, &selectors, transport, profile, bind);
        let route_scope = served_listener_route_scope(surface);
        let tls = ServedListenerTls::default();
        let auth = ServedListenerAuth::default();
        let limits = ServedListenerLimits::default();
        let audit = ServedListenerAudit::default();
        validate_served_listener_policy(&tls, &auth, &limits, &audit, route_scope, "read-write")?;
        Ok(ServedListenerRecord {
            id,
            schema_version: SERVED_LISTENER_SCHEMA_VERSION,
            surface: surface.to_string(),
            selectors,
            transport: transport.to_string(),
            profile: profile.map(str::to_string),
            bind: bind.to_string(),
            enabled,
            tls,
            auth,
            limits,
            audit,
            route_scope: route_scope.to_string(),
            exposure: "read-write".to_string(),
            network_access_policy_ref: None,
            last_modified_audit_seq: None,
        })
    }

    pub fn served_listeners(&self) -> Result<Vec<ServedListenerRecord>> {
        let mut out = self
            .control_scan_prefix(SERVED_LISTENER_PREFIX)?
            .into_iter()
            .map(|(key, value)| decode_served_listener_entry(&key, &value))
            .collect::<Result<Vec<_>>>()?;
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }

    pub fn served_listener(&self, id: &str) -> Result<Option<ServedListenerRecord>> {
        self.control_get(&served_listener_key(id))?
            .map(|value| decode_served_listener(&value))
            .transpose()
    }

    pub fn save_served_listener_audited(
        &self,
        record: &ServedListenerRecord,
        principal: Option<WorkspaceId>,
        action: &str,
        target: Option<&str>,
    ) -> Result<u64> {
        validate_served_listener_record(record)?;
        let mut audit_delta = AuditRetentionDelta::default();
        let seq = self.append_audit_record_delta(&mut audit_delta, principal, action, target)?;
        let mut stored = record.clone();
        stored.schema_version = SERVED_LISTENER_SCHEMA_VERSION;
        stored.last_modified_audit_seq = Some(seq);
        let mut control_puts = BTreeMap::new();
        control_puts.insert(
            served_listener_key(&stored.id),
            encode_served_listener(&stored),
        );
        self.commit_control_delta_and_audit_retention(
            control_puts,
            BTreeSet::new(),
            audit_delta,
            None,
        )?;
        Ok(seq)
    }

    pub fn remove_served_listener_audited(
        &self,
        id: &str,
        principal: Option<WorkspaceId>,
        action: &str,
        target: Option<&str>,
    ) -> Result<u64> {
        let key = served_listener_key(id);
        if self.control_get(&key)?.is_none() {
            return Err(LoomError::not_found("served listener not found"));
        }
        let mut audit_delta = AuditRetentionDelta::default();
        let seq = self.append_audit_record_delta(&mut audit_delta, principal, action, target)?;
        self.commit_control_delta_and_audit_retention(
            BTreeMap::new(),
            BTreeSet::from([key]),
            audit_delta,
            None,
        )?;
        Ok(seq)
    }

    pub fn authority_replication_policy(
        id: &str,
        source: &str,
        enabled: bool,
    ) -> Result<AuthorityReplicationPolicy> {
        validate_authority_replication_id(id)?;
        validate_authority_replication_source(source)?;
        Ok(AuthorityReplicationPolicy {
            id: id.to_string(),
            schema_version: AUTHORITY_REPLICATION_SCHEMA_VERSION,
            source: source.to_string(),
            enabled,
            pull_on_start: true,
            interval_ms: None,
            jitter_ms: 0,
            backoff_ms: 60_000,
            publish_witness: true,
            last_success_ms: None,
            last_failure_ms: None,
            last_error: None,
            last_modified_audit_seq: None,
        })
    }

    pub fn authority_replication_policies(&self) -> Result<Vec<AuthorityReplicationPolicy>> {
        let mut out = self
            .control_scan_prefix(AUTHORITY_REPLICATION_PREFIX)?
            .into_iter()
            .map(|(key, value)| decode_authority_replication_entry(&key, &value))
            .collect::<Result<Vec<_>>>()?;
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }

    pub fn authority_replication_policy_by_id(
        &self,
        id: &str,
    ) -> Result<Option<AuthorityReplicationPolicy>> {
        self.control_get(&authority_replication_key(id))?
            .map(|value| decode_authority_replication_policy(&value))
            .transpose()
    }

    pub fn save_authority_replication_policy_audited(
        &self,
        policy: &AuthorityReplicationPolicy,
        principal: Option<WorkspaceId>,
        action: &str,
        target: Option<&str>,
    ) -> Result<u64> {
        validate_authority_replication_policy(policy)?;
        let mut audit_delta = AuditRetentionDelta::default();
        let seq = self.append_audit_record_delta(&mut audit_delta, principal, action, target)?;
        let mut stored = policy.clone();
        stored.schema_version = AUTHORITY_REPLICATION_SCHEMA_VERSION;
        stored.last_modified_audit_seq = Some(seq);
        let mut control_puts = BTreeMap::new();
        control_puts.insert(
            authority_replication_key(&stored.id),
            encode_authority_replication_policy(&stored),
        );
        self.commit_control_delta_and_audit_retention(
            control_puts,
            BTreeSet::new(),
            audit_delta,
            None,
        )?;
        Ok(seq)
    }

    pub fn remove_authority_replication_policy_audited(
        &self,
        id: &str,
        principal: Option<WorkspaceId>,
        action: &str,
        target: Option<&str>,
    ) -> Result<u64> {
        let key = authority_replication_key(id);
        if self.control_get(&key)?.is_none() {
            return Err(LoomError::not_found(
                "authority replication policy not found",
            ));
        }
        let mut audit_delta = AuditRetentionDelta::default();
        let seq = self.append_audit_record_delta(&mut audit_delta, principal, action, target)?;
        self.commit_control_delta_and_audit_retention(
            BTreeMap::new(),
            BTreeSet::from([key]),
            audit_delta,
            None,
        )?;
        Ok(seq)
    }

    fn control_root_map(&self) -> Result<BTreeMap<Vec<u8>, Vec<u8>>> {
        let Some(root) = self.control_root() else {
            return Ok(BTreeMap::new());
        };
        let bytes = self
            .get(&root)?
            .ok_or_else(|| corrupt("control-plane root object missing"))?;
        decode_control_map(&bytes)
    }

    fn control_map(&self) -> Result<BTreeMap<Vec<u8>, Vec<u8>>> {
        let mut map = self.control_root_map()?;
        let audit_map = self.audit_retention_map()?;
        map.retain(|key, _| !is_audit_retention_control_key(key));
        map.extend(audit_map);
        Ok(map)
    }

    fn write_control_map(&self, map: BTreeMap<Vec<u8>, Vec<u8>>) -> Result<()> {
        let (control_map, audit_map) = split_audit_retention_control_map(map);
        if audit_map.is_empty() {
            self.commit_control_map_and_audit_retention_delta(
                Some(control_map),
                AuditRetentionDelta::default(),
                None,
            )
        } else {
            self.commit_control_map_and_audit_retention_map(control_map, audit_map, None)
        }
    }

    fn commit_control_delta_and_audit_retention(
        &self,
        control_puts: BTreeMap<Vec<u8>, Vec<u8>>,
        control_deletes: BTreeSet<Vec<u8>>,
        mut audit_delta: AuditRetentionDelta,
        reference_root: Option<Option<Digest>>,
    ) -> Result<()> {
        let mut normalized_control_puts = BTreeMap::new();
        for (key, value) in control_puts {
            if is_audit_retention_control_key(&key) {
                audit_delta.put(&key, value);
            } else {
                normalized_control_puts.insert(key, value);
            }
        }
        let mut normalized_control_deletes = BTreeSet::new();
        for key in control_deletes {
            if is_audit_retention_control_key(&key) {
                audit_delta.delete(key);
            } else {
                normalized_control_deletes.insert(key);
            }
        }
        let needs_legacy_audit_migration = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            inner.audit_retention_root.is_none() && !audit_delta.is_empty()
        };
        let mut control_map = if normalized_control_puts.is_empty()
            && normalized_control_deletes.is_empty()
            && !needs_legacy_audit_migration
        {
            None
        } else {
            Some(self.control_root_map()?)
        };
        let audit_map = if needs_legacy_audit_migration {
            let map = control_map.get_or_insert_with(BTreeMap::new).clone();
            let (control, mut audit) = split_audit_retention_control_map(map);
            *control_map.as_mut().expect("control map") = control;
            apply_audit_retention_delta(&mut audit, &audit_delta);
            Some(audit)
        } else {
            None
        };
        if let Some(map) = &mut control_map {
            map.retain(|key, _| !is_audit_retention_control_key(key));
            for key in normalized_control_deletes {
                map.remove(&key);
            }
            for (key, value) in normalized_control_puts {
                map.insert(key, value);
            }
        }
        match audit_map {
            Some(audit_map) => self.commit_control_map_and_audit_retention_map(
                control_map.unwrap_or_default(),
                audit_map,
                reference_root,
            ),
            None => self.commit_control_map_and_audit_retention_delta(
                control_map,
                audit_delta,
                reference_root,
            ),
        }
    }

    fn commit_control_map_and_audit_retention_map(
        &self,
        control_map: BTreeMap<Vec<u8>, Vec<u8>>,
        audit_map: BTreeMap<Vec<u8>, Vec<u8>>,
        reference_root: Option<Option<Digest>>,
    ) -> Result<()> {
        let mut inner = self.inner.lock().map_err(|_| poisoned())?;
        let publication_authority =
            self.begin_foreground_transaction_publication(&inner, control_map)?;
        let reference = match reference_root {
            Some(root) => root.map(|digest| *digest.bytes()),
            None => inner.reference_root.map(|digest| *digest.bytes()),
        };
        let new_gen = inner.generation + 1;
        let (roots, control_placement) = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            let prepared = self.prepare_foreground_transaction_publication(
                &mut **file,
                &inner,
                ForegroundMutationInput::AuditRetentionMap,
                &publication_authority,
                |file, alloc| {
                    let audit_retention_root = write_audit_retention_map_to_root(
                        file,
                        alloc,
                        inner.audit_retention_root,
                        inner.page_count,
                        &audit_map,
                    )?;
                    let root_catalog_entries = root_catalog_entries_with_family(
                        &inner.root_catalog_entries,
                        AUDIT_RETENTION_FAMILY_ID,
                        audit_retention_root,
                    );
                    let root_catalog_root = write_root_catalog_page(
                        file,
                        alloc,
                        inner.root_catalog_root,
                        inner.page_count,
                        &root_catalog_entries,
                    )?;
                    let prepared_finalization = self.prepare_foreground_transaction_finalization(
                        file,
                        &inner,
                        alloc,
                        &publication_authority,
                        inner.index_root,
                    )?;
                    let finalization = self.apply_foreground_transaction_finalization(
                        file,
                        alloc,
                        inner.index_root,
                        prepared_finalization,
                    )?;
                    let touched_segments = finalization
                        .fresh_control_placement
                        .iter()
                        .map(|(_, loc)| loc.segment_id)
                        .collect::<BTreeSet<_>>();
                    let object_count = inner
                        .maintenance
                        .object_count
                        .saturating_add(u64::from(finalization.fresh_control_placement.is_some()));
                    let publication = finish_foreground_txn_on_planning_backing(
                        file,
                        alloc,
                        new_gen,
                        object_count,
                        TxnRootInputs {
                            object_index: finalization.index_root,
                            legacy_overlay: legacy_overlay_root_for_publication(
                                &inner,
                                inner.current_record_root,
                                root_catalog_root,
                            ),
                            current_records: inner.current_record_root,
                            root_catalog: TxnRootCatalog {
                                root: root_catalog_root,
                                entries: root_catalog_entries,
                            },
                            previous_mutable_overlay_generation_floor: inner
                                .mutable_overlay_generation_floor,
                            mutable_overlay_generation_floor: inner
                                .mutable_overlay_generation_floor,
                            reference,
                            control: finalization.control,
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
                        finalization.free_map_publication,
                    )?;
                    Ok(PreparedForegroundTransactionOutcome {
                        publication,
                        value: finalization.fresh_control_placement,
                    })
                },
            )?;
            self.finish_foreground_txn(&mut **file, &inner, prepared)?
        };

        self.adopt_committed_roots_locked(&mut inner, roots)?;
        if let Some((key, loc)) = control_placement {
            Self::cache_locator_locked(&mut inner, key, loc);
        }
        Ok(())
    }

    fn commit_control_map_and_audit_retention_delta(
        &self,
        control_map: Option<BTreeMap<Vec<u8>, Vec<u8>>>,
        audit_delta: AuditRetentionDelta,
        reference_root: Option<Option<Digest>>,
    ) -> Result<()> {
        let mut inner = self.inner.lock().map_err(|_| poisoned())?;
        let control_map_was_supplied = control_map.is_some();
        let control_map = match control_map {
            Some(control_map) => control_map,
            None => self.control_map_locked(&mut inner)?,
        };
        let publication_authority =
            self.begin_foreground_transaction_publication(&inner, control_map)?;
        let reference = match reference_root {
            Some(root) => root.map(|digest| *digest.bytes()),
            None => inner.reference_root.map(|digest| *digest.bytes()),
        };
        let new_gen = inner.generation + 1;
        let (roots, control_placement) = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            let prepared = self.prepare_foreground_transaction_publication(
                &mut **file,
                &inner,
                ForegroundMutationInput::AuditRetentionDelta,
                &publication_authority,
                |file, alloc| {
                    let audit_retention_root = write_audit_retention_delta_to_root(
                        file,
                        alloc,
                        inner.audit_retention_root,
                        inner.page_count,
                        &audit_delta,
                        #[cfg(test)]
                        Some(&self.audit_retention_test_instrumentation),
                    )?;
                    let root_catalog_entries = root_catalog_entries_with_family(
                        &inner.root_catalog_entries,
                        AUDIT_RETENTION_FAMILY_ID,
                        audit_retention_root,
                    );
                    let root_catalog_root = write_root_catalog_page(
                        file,
                        alloc,
                        inner.root_catalog_root,
                        inner.page_count,
                        &root_catalog_entries,
                    )?;
                    let prepared_finalization = self.prepare_foreground_transaction_finalization(
                        file,
                        &inner,
                        alloc,
                        &publication_authority,
                        inner.index_root,
                    )?;
                    let finalization = self.apply_foreground_transaction_finalization(
                        file,
                        alloc,
                        inner.index_root,
                        prepared_finalization,
                    )?;
                    let touched_segments = finalization
                        .fresh_control_placement
                        .iter()
                        .map(|(_, loc)| loc.segment_id)
                        .collect::<BTreeSet<_>>();
                    let object_count = inner
                        .maintenance
                        .object_count
                        .saturating_add(u64::from(finalization.fresh_control_placement.is_some()));
                    #[cfg(any(test, feature = "test-hooks"))]
                    invoke_store_publication_failure_test_injector(
                        &self.path,
                        StorePublicationFailureTestBoundary::AuditRetentionBeforeFinishTxn,
                    )?;
                    let publication = finish_foreground_txn_on_planning_backing(
                        file,
                        alloc,
                        new_gen,
                        object_count,
                        TxnRootInputs {
                            object_index: finalization.index_root,
                            legacy_overlay: legacy_overlay_root_for_publication(
                                &inner,
                                inner.current_record_root,
                                root_catalog_root,
                            ),
                            current_records: inner.current_record_root,
                            root_catalog: TxnRootCatalog {
                                root: root_catalog_root,
                                entries: root_catalog_entries,
                            },
                            previous_mutable_overlay_generation_floor: inner
                                .mutable_overlay_generation_floor,
                            mutable_overlay_generation_floor: inner
                                .mutable_overlay_generation_floor,
                            reference,
                            control: match finalization.control {
                                Some(control) => Some(control),
                                None if control_map_was_supplied => None,
                                None => inner.control_root.map(|digest| *digest.bytes()),
                            },
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
                        finalization.free_map_publication,
                    )?;
                    Ok(PreparedForegroundTransactionOutcome {
                        publication,
                        value: finalization.fresh_control_placement,
                    })
                },
            )?;
            self.finish_foreground_txn(&mut **file, &inner, prepared)?
        };

        self.adopt_committed_roots_locked(&mut inner, roots)?;
        if let Some((key, loc)) = control_placement {
            Self::cache_locator_locked(&mut inner, key, loc);
        }
        Ok(())
    }

    fn decode_lock_fence_records(&self, prefix: &[u8]) -> Result<Vec<(Vec<u8>, u64)>> {
        self.control_scan_prefix(prefix)?
            .into_iter()
            .map(|(key, value)| {
                let fence = decode_lock_fence_value(&value)?;
                Ok((key[prefix.len()..].to_vec(), fence))
            })
            .collect()
    }

    /// Whether this Loom was created encrypted (its superblock carries `encryption_meta`).
    pub fn is_encrypted(&self) -> bool {
        self.inner
            .lock()
            .map(|i| i.encryption_meta.is_some())
            .unwrap_or(false)
    }

    /// The store's identity-profile digest algorithm: `Algo::Blake3` for the default
    /// profile, `Algo::Sha256` for the FIPS profile. Set at creation, read from the superblock on open,
    /// immutable. The engine threads this into content addressing so the whole Loom uses one algorithm.
    pub fn digest_algo(&self) -> Algo {
        self.digest_algo
    }

    /// The decoded `encryption_meta`, or `None` for an unencrypted Loom.
    pub fn encryption_meta(&self) -> Result<Option<loom_core::keys::EncryptionMeta>> {
        let raw = self
            .inner
            .lock()
            .map_err(|_| poisoned())?
            .encryption_meta
            .clone();
        match raw {
            Some(bytes) => Ok(Some(loom_core::keys::EncryptionMeta::decode(&bytes)?)),
            None => Ok(None),
        }
    }

    /// Whether an unlocked DEK session is held (the store is encrypted and has been unlocked this open).
    pub fn is_unlocked(&self) -> bool {
        self.dek.lock().map(|d| d.is_some()).unwrap_or(false)
    }

    /// Unlock the data-encryption key from a credential, enabling encrypted object reads/writes on this
    /// handle. [`Code::Unsupported`] if the Loom is not encrypted; [`Code::E2eKeyInvalid`]
    /// if the credential does not unwrap the DEK. Idempotent - a later unlock replaces the session.
    pub fn unlock(&self, spec: &loom_core::keys::KeySpec) -> Result<()> {
        let meta = self.encryption_meta()?.ok_or_else(|| {
            LoomError::new(Code::Unsupported, "loom-store: store is not encrypted")
        })?;
        let session = meta.unlock(spec)?;
        *self.dek.lock().map_err(|_| poisoned())? = Some(session);
        Ok(())
    }

    /// Re-wrap the DEK under a new credential (the cheap `rekey`): requires an unlocked
    /// session, derives a new `encryption_meta` from caller-supplied fresh `salt` + `wrap_nonce`, installs
    /// it, and forces it into the superblock immediately. Objects are not re-sealed (the DEK is
    /// unchanged), so it is O(1).
    pub fn rekey(
        &self,
        new_spec: &loom_core::keys::KeySpec,
        salt: Vec<u8>,
        wrap_nonce: Vec<u8>,
    ) -> Result<()> {
        let encoded = {
            let dek = self.dek.lock().map_err(|_| poisoned())?;
            let session = dek.as_ref().ok_or_else(|| {
                LoomError::new(
                    Code::E2eLocked,
                    "loom-store: rekey requires an unlocked store",
                )
            })?;
            loom_core::keys::EncryptionMeta::rewrap(session, new_spec, salt, wrap_nonce)?.encode()
        };
        self.inner.lock().map_err(|_| poisoned())?.encryption_meta = Some(encoded);
        // encryption_meta is not part of the per-commit journal, so a checkpoint that only happens on
        // an interval would lag the rekey; force the superblock write now so the new meta is durable
        // immediately (and every later journal-recovery fold preserves it from this checkpoint).
        self.write_superblock_checkpoint()
    }

    /// Add a second unlock credential for the same DEK. The store must already be unlocked. External
    /// credentials require a passphrase recovery wrap unless `allow_no_recovery` is set.
    pub fn add_wrap(
        &self,
        new_spec: &loom_core::keys::KeySpec,
        salt: Vec<u8>,
        wrap_nonce: Vec<u8>,
        allow_no_recovery: bool,
    ) -> Result<()> {
        let meta = self.encryption_meta()?.ok_or_else(|| {
            LoomError::new(Code::Unsupported, "loom-store: store is not encrypted")
        })?;
        let encoded = {
            let dek = self.dek.lock().map_err(|_| poisoned())?;
            let session = dek.as_ref().ok_or_else(|| {
                LoomError::new(
                    Code::E2eLocked,
                    "loom-store: add-wrap requires an unlocked store",
                )
            })?;
            meta.add_wrap(session, new_spec, salt, wrap_nonce, allow_no_recovery)?
                .encode()
        };
        self.inner.lock().map_err(|_| poisoned())?.encryption_meta = Some(encoded);
        self.write_superblock_checkpoint()
    }

    /// Remove one unlock credential by zero-based wrap index. The store must already be unlocked.
    pub fn remove_wrap(&self, index: usize, allow_no_recovery: bool) -> Result<()> {
        let meta = self.encryption_meta()?.ok_or_else(|| {
            LoomError::new(Code::Unsupported, "loom-store: store is not encrypted")
        })?;
        {
            let dek = self.dek.lock().map_err(|_| poisoned())?;
            if dek.is_none() {
                return Err(LoomError::new(
                    Code::E2eLocked,
                    "loom-store: remove-wrap requires an unlocked store",
                ));
            }
        }
        let encoded = meta.remove_wrap(index, allow_no_recovery)?.encode();
        self.inner.lock().map_err(|_| poisoned())?.encryption_meta = Some(encoded);
        self.write_superblock_checkpoint()
    }

    /// Force-write the current committed state (including `encryption_meta`) into a superblock checkpoint
    /// slot now, rather than waiting for the commit-interval checkpoint. Used by [`rekey`](Self::rekey)
    /// so an encryption-metadata change is durable immediately.
    fn write_superblock_checkpoint(&self) -> Result<()> {
        let (sb, cp_slot) = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            let sb = Superblock {
                generation: inner.generation,
                page_count: inner.page_count,
                digest_algo: self.digest_algo,
                region_table: inner.region_table_root,
                reference: inner.reference_root.map(|d| *d.bytes()),
                control: inner.control_root.map(|d| *d.bytes()),
                encryption: inner.encryption_meta.clone(),
            }
            .encode();
            let cp_slot = ((inner.generation / CHECKPOINT_INTERVAL) & 1) * SLOT_SIZE;
            (sb, cp_slot)
        };
        let mut file = self.file.lock().map_err(|_| poisoned())?;
        write_at(&mut **file, cp_slot, &sb).map_err(io_err)?;
        file.fsync().map_err(io_err)?;
        Ok(())
    }

    /// Store many objects in **one atomic transaction**: a crash commits them all or none, via a
    /// single superblock swap rather than one swap per object. Objects already stored, or duplicated
    /// within `items`, are deduped but still reported, so the returned digests line up 1:1 with
    /// `items`.
    pub fn put_batch(&self, items: &[&[u8]]) -> Result<Vec<Digest>> {
        if items.is_empty() {
            return Ok(Vec::new());
        }
        let digests: Vec<Digest> = items
            .iter()
            .map(|c| Digest::hash(self.digest_algo, c))
            .collect();
        let to_append: Vec<(Digest, &[u8], Codec)> = digests
            .iter()
            .copied()
            .zip(items.iter().copied())
            .map(|(d, c)| (d, c, self.default_codec))
            .collect();
        self.group_commit(&to_append)?;
        Ok(digests)
    }

    pub fn put_batch_and_set_reference_root(
        &self,
        items: &[(Digest, Vec<u8>)],
        root: Digest,
    ) -> Result<()> {
        #[cfg(any(test, feature = "test-hooks"))]
        observe_store_publication(&self.path, StorePublicationTestEvent::BatchReferenceRoot);
        let to_append = items
            .iter()
            .map(|(digest, canonical)| (*digest, canonical.as_slice(), self.default_codec))
            .collect::<Vec<_>>();
        self.commit_txn(&to_append, Some(Some(*root.bytes())), None, None)
    }

    pub fn put_batch_control_delta_and_set_reference_root(
        &self,
        items: &[(Digest, Vec<u8>)],
        controls: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
        root: Digest,
    ) -> Result<()> {
        #[cfg(any(test, feature = "test-hooks"))]
        observe_store_publication(
            &self.path,
            StorePublicationTestEvent::BatchControlReferenceRoot,
        );
        let _publication_guard = self.overlay_publication.lock().map_err(|_| poisoned())?;
        let controls = controls
            .into_iter()
            .map(|(key, value)| match value {
                Some(payload) => loom_core::WorkflowControlWrite::Put { key, payload },
                None => loom_core::WorkflowControlWrite::Delete { key },
            })
            .collect();
        let owner_state = loom_core::WorkflowOwnerState {
            objects: items.to_vec(),
            reference: loom_core::WorkflowReferenceUpdate::Set(Some(root)),
            controls,
            audits: Vec::new(),
        };
        self.commit_workflow_owner_state_records(&[], &owner_state, None)?;
        Ok(())
    }

    pub fn put_batch_control_set_with_reference(
        &self,
        items: &[(Digest, Vec<u8>)],
        key: &[u8],
        value: Vec<u8>,
        principal: Option<WorkspaceId>,
        action: Option<&str>,
        target: Option<&str>,
        reference_root: Option<Digest>,
    ) -> Result<()> {
        let mut control_puts = BTreeMap::new();
        control_puts.insert(key.to_vec(), value);
        let mut audit_delta = AuditRetentionDelta::default();
        if let Some(action) = action {
            self.append_audit_record_delta(&mut audit_delta, principal, action, target)?;
        }
        self.commit_control_delta_and_audit_retention(
            control_puts,
            BTreeSet::new(),
            audit_delta,
            Some(reference_root),
        )?;
        let to_append = items
            .iter()
            .map(|(digest, canonical)| (*digest, canonical.as_slice(), self.default_codec))
            .collect::<Vec<_>>();
        self.group_commit(&to_append)
    }

    /// Group commit: coalesce concurrent object writes into one fsync'd transaction. The caller
    /// enqueues its objects, then whichever caller finds no leader running becomes the leader and
    /// commits the whole queue (its own objects plus any other threads' that have arrived) via
    /// [`FileStore::commit_txn`], draining repeatedly until the queue empties; every other caller
    /// waits for its batch's outcome. So `N` threads each doing a `put` cost far fewer than `N`
    /// fsyncs under contention, while still serializing through the single commit path. Each caller's
    /// objects must be owned in the queue, because the leader (a different thread) commits them.
    fn group_commit(&self, items: &[(Digest, &[u8], Codec)]) -> Result<()> {
        let me = Arc::new(Waiter {
            outcome: Mutex::new(None),
            cv: Condvar::new(),
        });
        let lead = {
            let mut g = self.group.lock().map_err(|_| poisoned())?;
            for (digest, canonical, codec) in items {
                g.pending.push((*digest, canonical.to_vec(), *codec));
            }
            g.waiters.push(me.clone());
            let was_idle = !g.leader_active;
            g.leader_active = true; // claim leadership (or confirm one is already running)
            was_idle
        };

        if lead {
            loop {
                let (batch, waiters) = {
                    let mut g = self.group.lock().map_err(|_| poisoned())?;
                    if g.pending.is_empty() {
                        g.leader_active = false; // queue drained: a later arrival leads the next batch
                        break;
                    }
                    (
                        std::mem::take(&mut g.pending),
                        std::mem::take(&mut g.waiters),
                    )
                };
                let borrowed: Vec<(Digest, &[u8], Codec)> = batch
                    .iter()
                    .map(|(d, c, codec)| (*d, c.as_slice(), *codec))
                    .collect();
                let outcome = self.commit_txn(&borrowed, None, None, None);
                for w in &waiters {
                    let mut slot = w.outcome.lock().unwrap_or_else(|p| p.into_inner());
                    *slot = Some(outcome.clone());
                    w.cv.notify_one();
                }
            }
        }

        // The leader filled our slot during its loop (our objects were in some batch); wait for it.
        let mut slot = me.outcome.lock().map_err(|_| poisoned())?;
        loop {
            if let Some(outcome) = slot.take() {
                return outcome;
            }
            slot = me.cv.wait(slot).map_err(|_| poisoned())?;
        }
    }

    /// The single durable commit path: write the batch's records onto fresh record pages, CoW-insert
    /// each `(digest -> RecordLoc)` into the index B-tree, write the new free-page map and region-table
    /// page, then fsync and journal a commit record - that journal fsync is the commit point. Every new
    /// page is freshly extended or an aged-out free page, so a crash before the journal fsync discards
    /// the whole batch (all-or-nothing). In-memory state is published only after the commit succeeds.
    /// Object writes reach here batched through [`FileStore::group_commit`]; `set_reference_root` calls
    /// directly.
    fn commit_txn(
        &self,
        to_append: &[(Digest, &[u8], Codec)],
        reference_override: Option<Option<[u8; 32]>>,
        control_override: Option<Option<[u8; 32]>>,
        mark_epoch_completed: Option<u64>,
    ) -> Result<()> {
        let mut inner = self.inner.lock().map_err(|_| poisoned())?;
        let reference =
            reference_override.unwrap_or_else(|| inner.reference_root.map(|d| *d.bytes()));
        let mut control =
            control_override.unwrap_or_else(|| inner.control_root.map(|d| *d.bytes()));

        let mut seen = BTreeSet::new();
        let mut fresh = Vec::new();
        for (digest, canonical, codec) in to_append {
            if !seen.insert(*digest.bytes()) {
                continue;
            }
            if self
                .lookup_loc_locked(&mut inner, digest.bytes())?
                .is_none()
            {
                fresh.push((*digest, *canonical, *codec));
            }
        }
        if fresh.is_empty()
            && reference_override.is_none()
            && control_override.is_none()
            && mark_epoch_completed.is_none()
        {
            return Ok(()); // nothing new and no engine-state change: no commit
        }
        let mut maintenance = inner.maintenance.clone();
        if let Some(epoch) = mark_epoch_completed {
            maintenance.last_validated_mark_epoch =
                maintenance.last_validated_mark_epoch.max(epoch);
        }

        let new_gen = inner.generation + 1;
        let mut foreground_authority = None;
        let mut specialized_lease = None;
        let reusable_free = if mark_epoch_completed.is_none() {
            let control_map = match control_override {
                Some(None) => BTreeMap::new(),
                Some(Some(expected)) => {
                    if inner
                        .control_root
                        .is_some_and(|root| *root.bytes() == expected)
                    {
                        self.control_map_locked(&mut inner)?
                    } else {
                        let bytes = to_append
                            .iter()
                            .find_map(|(digest, bytes, _)| {
                                (*digest.bytes() == expected).then_some(*bytes)
                            })
                            .ok_or_else(|| {
                                corrupt("control override object missing from transaction")
                            })?;
                        decode_control_map(bytes)?
                    }
                }
                None => self.control_map_locked(&mut inner)?,
            };
            let authority = self.begin_foreground_transaction_publication(&inner, control_map)?;
            let reusable = AllocatorVisibleReusableRuns {
                ordinary: authority.ordinary_reusable_runs.clone(),
                publication: authority.publication_eligible_runs.clone(),
            };
            foreground_authority = Some(authority);
            reusable
        } else {
            let (reusable, lease) = self.transaction_reusable_free(
                &inner.free,
                inner.active_mark_epoch_reclaim_fence,
                inner.minimum_recoverable_generation,
            )?;
            specialized_lease = Some(lease);
            AllocatorVisibleReusableRuns {
                ordinary: reusable,
                publication: Vec::new(),
            }
        };
        let (roots, placements) = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            if let Some(authority) = &foreground_authority {
                let prepared = self.prepare_foreground_transaction_publication(
                    &mut **file,
                    &inner,
                    ForegroundMutationInput::ObjectBatch,
                    authority,
                    |file, alloc| {
                        let dek = self.dek.lock().map_err(|_| poisoned())?;
                        let mut placements = write_record_pages(file, alloc, &fresh, dek.as_ref())?;
                        drop(dek);
                        let mut touched_segments: BTreeSet<u64> =
                            placements.iter().map(|(_, loc)| loc.segment_id).collect();
                        let index_batch = pagebtree::batch_upsert(
                            file,
                            DATA_START,
                            alloc,
                            inner.index_root,
                            &placements,
                            inner.page_count,
                        )?;
                        #[cfg(any(test, feature = "test-hooks"))]
                        observe_object_index_batch(index_batch.stats);
                        let prepared_finalization = self
                            .prepare_foreground_transaction_finalization(
                                file,
                                &inner,
                                &*alloc,
                                authority,
                                index_batch.root,
                            )?;
                        let finalization = self.apply_foreground_transaction_finalization(
                            file,
                            alloc,
                            index_batch.root,
                            prepared_finalization,
                        )?;
                        control = finalization.control;
                        if let Some(placement) = finalization.fresh_control_placement {
                            touched_segments.insert(placement.1.segment_id);
                            placements.push(placement);
                        }
                        let publication = finish_foreground_txn_on_planning_backing(
                            file,
                            alloc,
                            new_gen,
                            inner
                                .maintenance
                                .object_count
                                .saturating_add(placements.len() as u64),
                            TxnRootInputs {
                                object_index: finalization.index_root,
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
                                mutable_overlay_generation_floor: inner
                                    .mutable_overlay_generation_floor,
                                reference,
                                control,
                            },
                            inner.open_segment,
                            &maintenance,
                            &touched_segments,
                            (
                                inner.freemap,
                                inner.region_table_root,
                                inner.maintenance_root,
                            ),
                            inner.encryption_meta.clone(),
                            self.digest_algo,
                            None,
                            finalization.free_map_publication,
                        )?;
                        Ok(PreparedForegroundTransactionOutcome {
                            publication,
                            value: placements,
                        })
                    },
                )?;
                self.finish_foreground_txn(&mut **file, &inner, prepared)?
            } else {
                let mut alloc = PageAllocator::new_with_reusable_authorities(
                    inner.page_count,
                    new_gen,
                    inner.free.clone(),
                    reusable_free.ordinary,
                    reusable_free.publication,
                );
                alloc.install_metadata_bootstrap_reserve(&inner.metadata_bootstrap_reserve)?;
                let dek = self.dek.lock().map_err(|_| poisoned())?;
                let placements = write_record_pages(&mut **file, &mut alloc, &fresh, dek.as_ref())?;
                drop(dek);
                let touched_segments: BTreeSet<u64> =
                    placements.iter().map(|(_, loc)| loc.segment_id).collect();
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
                let roots = finish_txn(
                    &mut **file,
                    &mut alloc,
                    new_gen,
                    inner
                        .maintenance
                        .object_count
                        .saturating_add(placements.len() as u64),
                    TxnRootInputs {
                        object_index: index_batch.root,
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
                        reference,
                        control,
                    },
                    inner.open_segment,
                    &maintenance,
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
            }
        };
        drop(foreground_authority);
        drop(specialized_lease);

        self.adopt_committed_roots_locked(&mut inner, roots)?;
        for (key, loc) in placements {
            Self::cache_locator_locked(&mut inner, key, loc);
        }
        Ok(())
    }

    #[cfg(test)]
    fn generation(&self) -> u64 {
        self.inner.lock().map(|i| i.generation).unwrap_or(0)
    }

    #[cfg(test)]
    fn logical_end(&self) -> u64 {
        self.inner
            .lock()
            .map(|i| DATA_START + i.page_count * PAGE_SIZE)
            .unwrap_or(0)
    }

    #[cfg(test)]
    fn free_runs(&self) -> Vec<FreePageRun> {
        self.inner
            .lock()
            .map(|i| i.free.clone())
            .unwrap_or_default()
    }
}

fn tail_free_pages(free: &[FreePageRun], page_count: u64) -> u64 {
    let mut end = page_count;
    let mut total = 0u64;
    while let Some(run) = free
        .iter()
        .find(|run| run.start.saturating_add(run.len) == end)
    {
        total = total.saturating_add(run.len);
        end = run.start;
    }
    total
}

fn classify_page_run(pages: &mut BTreeMap<u64, String>, start: u64, len: u64, class: &str) {
    for page in start..start.saturating_add(len) {
        pages.insert(page, class.to_string());
    }
}

fn classify_record_loc(
    file: &mut dyn BackingIo,
    pages: &mut BTreeMap<u64, String>,
    loc: RecordLoc,
    page_count: u64,
    prefix: &str,
) -> Result<()> {
    let page = loc.global_page();
    let mut first = [0u8; PAGE_SIZE as usize];
    read_exact_at(file, PageId(page).offset(DATA_START), &mut first).map_err(io_err)?;
    if first[0] == record::CHUNKED_BLOB_MAGIC {
        let class = format!("{prefix}_chunked_page");
        for page in record_io::chunked_blob_pages(file, page, page_count)? {
            pages.insert(page, class.clone());
        }
        return Ok(());
    }
    let span = record_io::page_span(file, page)?;
    let class = if span == 1 {
        format!("{prefix}_slab_page")
    } else {
        format!("{prefix}_large_page")
    };
    classify_page_run(pages, page, span, &class);
    Ok(())
}

fn classify_unreferenced_page(
    file: &mut dyn BackingIo,
    pages: &mut BTreeMap<u64, String>,
    page: u64,
    page_count: u64,
) -> Result<String> {
    let mut buf = [0u8; PAGE_SIZE as usize];
    read_exact_at(file, PageId(page).offset(DATA_START), &mut buf).map_err(io_err)?;
    let class = if buf.iter().all(|byte| *byte == 0) {
        "unreferenced_zero_page".to_string()
    } else if record::read_slab_slot(&buf, 0).is_some() {
        "stale_record_slab_page".to_string()
    } else if let Some(blob_len) = record::large_blob_len(&buf) {
        let span = record::large_pages(blob_len);
        if page.saturating_add(span) <= page_count {
            let mut run = vec![0u8; (span * PAGE_SIZE) as usize];
            read_exact_at(file, PageId(page).offset(DATA_START), &mut run).map_err(io_err)?;
            if record::decode_large(&run).is_some() {
                classify_page_run(pages, page, span, "stale_record_large_page");
                "stale_record_large_page".to_string()
            } else {
                "unreferenced_unclassified_page".to_string()
            }
        } else {
            "unreferenced_unclassified_page".to_string()
        }
    } else if pagebtree::looks_like_node_page(&buf) {
        "stale_tree_page".to_string()
    } else if page::RegionTable::decode(&buf).is_some() {
        "stale_region_table_page".to_string()
    } else if maintenance::looks_like_maintenance_page(&buf) {
        "stale_maintenance_page".to_string()
    } else if record::decode_chunked_blob_page(&buf).is_some() {
        "stale_record_chunked_page".to_string()
    } else {
        "unreferenced_unclassified_page".to_string()
    };
    pages.insert(page, class.clone());
    Ok(class)
}

fn add_page_class(
    classes: &mut BTreeMap<String, StorePageClass>,
    class: &str,
    pages: u64,
    example: &str,
    max_examples: usize,
) {
    let entry = classes.entry(class.to_string()).or_insert(StorePageClass {
        class: class.to_string(),
        pages: 0,
        bytes: 0,
        examples: Vec::new(),
    });
    entry.pages = entry.pages.saturating_add(pages);
    entry.bytes = entry.bytes.saturating_add(pages.saturating_mul(PAGE_SIZE));
    if entry.examples.len() < max_examples {
        entry.examples.push(example.to_string());
    }
}

fn push_example(examples: &mut Vec<String>, example: String, max_examples: usize) {
    if examples.len() < max_examples {
        examples.push(example);
    }
}

fn attribution_for_page_run(
    root: &str,
    family_id: Option<u16>,
    role: &str,
    run: Option<(u64, u64)>,
    max_examples: usize,
) -> StoreRootStorageClass {
    let (present, tree_pages, examples) = match run {
        Some((start, pages)) => {
            let mut examples = Vec::new();
            push_example(&mut examples, format!("page:{start}"), max_examples);
            (true, pages, examples)
        }
        None => (false, 0, Vec::new()),
    };
    StoreRootStorageClass {
        root: root.to_string(),
        family_id,
        role: role.to_string(),
        present,
        tree_pages,
        tree_bytes: tree_pages.saturating_mul(PAGE_SIZE),
        record_pages: 0,
        payload_bytes: 0,
        examples,
    }
}

fn root_family_role(descriptor: &RootFamilyDescriptor) -> &'static str {
    match descriptor.role {
        RootFamilyRole::CurrentState => "current",
        RootFamilyRole::RetainedControl => match descriptor.gc_reachability {
            RootFamilyReachability::SemanticRoot => "retained",
            RootFamilyReachability::ControlRoot | RootFamilyReachability::PhysicalSafetyRoot => {
                "control"
            }
            RootFamilyReachability::AdvisoryPreserveOnly => "advisory",
        },
        RootFamilyRole::RebuildableAdvisory => "advisory",
    }
}

fn attribution_for_catalog_family(
    file: &mut dyn BackingIo,
    page_count: u64,
    descriptor: &RootFamilyDescriptor,
    root: Option<PageId>,
    max_examples: usize,
) -> Result<StoreRootStorageClass> {
    let expected = match descriptor.flags {
        ROOT_FLAG_AUTHORITATIVE => "authoritative",
        ROOT_FLAG_ADVISORY => "advisory",
        _ => "catalog",
    };
    let role = format!("{}:{expected}", root_family_role(descriptor));
    attribution_for_btree_root(
        file,
        page_count,
        descriptor.name,
        Some(descriptor.family_id),
        &role,
        root,
        max_examples,
    )
}

fn attribution_for_btree_root(
    file: &mut dyn BackingIo,
    page_count: u64,
    root_name: &str,
    family_id: Option<u16>,
    role: &str,
    root: Option<PageId>,
    max_examples: usize,
) -> Result<StoreRootStorageClass> {
    let Some(root_page) = root else {
        return Ok(StoreRootStorageClass {
            root: root_name.to_string(),
            family_id,
            role: role.to_string(),
            present: false,
            tree_pages: 0,
            tree_bytes: 0,
            record_pages: 0,
            payload_bytes: 0,
            examples: Vec::new(),
        });
    };
    let codec = match family_id {
        Some(family_id) => root_family_value_codec(family_id)?,
        None => pagebtree::ValueCodecKind::RecordLoc,
    };
    let tree_pages =
        pagebtree::collect_pages_with_codec(file, DATA_START, root_page, page_count, codec)?.len()
            as u64;
    let entries = pagebtree::load_all_with_codec(file, DATA_START, root_page, page_count, codec)?;
    let mut record_pages = 0u64;
    let mut payload_bytes = 0u64;
    let mut examples = Vec::new();
    push_example(
        &mut examples,
        format!("tree_root:{}", root_page.0),
        max_examples,
    );
    for (address, loc) in entries {
        let (pages, bytes) = record_loc_storage(file, loc, page_count)?;
        record_pages = record_pages.saturating_add(pages);
        payload_bytes = payload_bytes.saturating_add(bytes);
        push_example(
            &mut examples,
            format!(
                "record:{:02x}{:02x}:page:{}",
                address[0],
                address[1],
                loc.global_page()
            ),
            max_examples,
        );
    }
    Ok(StoreRootStorageClass {
        root: root_name.to_string(),
        family_id,
        role: role.to_string(),
        present: true,
        tree_pages,
        tree_bytes: tree_pages.saturating_mul(PAGE_SIZE),
        record_pages,
        payload_bytes,
        examples,
    })
}

fn attribution_for_digest_root(
    file: &mut dyn BackingIo,
    page_count: u64,
    dek: Option<&DekSession>,
    root_name: &str,
    role: &str,
    digest: Option<Digest>,
    index_entries: &BTreeMap<[u8; 32], RecordLoc>,
    max_examples: usize,
) -> Result<StoreRootStorageClass> {
    let Some(digest) = digest else {
        return Ok(StoreRootStorageClass {
            root: root_name.to_string(),
            family_id: None,
            role: role.to_string(),
            present: false,
            tree_pages: 0,
            tree_bytes: 0,
            record_pages: 0,
            payload_bytes: 0,
            examples: Vec::new(),
        });
    };
    let mut out = StoreRootStorageClass {
        root: root_name.to_string(),
        family_id: None,
        role: role.to_string(),
        present: true,
        tree_pages: 0,
        tree_bytes: 0,
        record_pages: 0,
        payload_bytes: 0,
        examples: Vec::new(),
    };
    let mut queue = VecDeque::from([digest]);
    let mut seen = BTreeSet::new();
    while let Some(digest) = queue.pop_front() {
        if !seen.insert(*digest.bytes()) {
            continue;
        }
        let Some(loc) = index_entries.get(digest.bytes()).copied() else {
            continue;
        };
        let (pages, payload_bytes) = record_loc_storage(file, loc, page_count)?;
        out.record_pages = out.record_pages.saturating_add(pages);
        out.payload_bytes = out.payload_bytes.saturating_add(payload_bytes);
        push_example(
            &mut out.examples,
            format!("record_page:{}", loc.global_page()),
            max_examples,
        );
        let bytes = read_payload_from_loc(file, digest, loc, page_count, dek)?;
        let Ok(object) = Object::decode(&bytes) else {
            continue;
        };
        for child in object_child_classifications(&object, index_entries) {
            if let ObjectChildAttribution::Traversable(digest) = child {
                queue.push_back(digest);
            }
        }
    }
    Ok(out)
}

fn object_index_reverse_ownership(
    file: &mut dyn BackingIo,
    page_count: u64,
    digest_algo: Algo,
    index_entries: &BTreeMap<[u8; 32], RecordLoc>,
    max_examples: usize,
) -> Result<BTreeMap<[u8; 32], StoreObjectReverseOwnership>> {
    let mut owners = BTreeMap::new();
    for (digest_bytes, loc) in index_entries {
        let digest = Digest::of(digest_algo, *digest_bytes);
        let mut owner =
            reverse_owner_for_record(file, page_count, digest, *loc, false, max_examples)?;
        add_physical_owner_root(&mut owner, "object_index_records");
        owners.insert(*digest_bytes, owner);
    }
    Ok(owners)
}

fn walk_object_graph_attribution(
    file: &mut dyn BackingIo,
    page_count: u64,
    dek: Option<&DekSession>,
    root: Option<Digest>,
    retaining_root: &str,
    logical_owner: &str,
    index_entries: &BTreeMap<[u8; 32], RecordLoc>,
    owners: &mut BTreeMap<[u8; 32], StoreObjectReverseOwnership>,
    max_examples: usize,
) -> Result<()> {
    let Some(root) = root else {
        return Ok(());
    };
    let mut queue = VecDeque::from([root]);
    let mut seen = BTreeSet::new();
    while let Some(digest) = queue.pop_front() {
        if !seen.insert(*digest.bytes()) {
            continue;
        }
        let Some(loc) = index_entries.get(digest.bytes()).copied() else {
            add_unresolved_owner(
                owners,
                digest,
                retaining_root,
                logical_owner,
                "missing_object_locator",
            );
            continue;
        };
        let owner = owners.entry(*digest.bytes()).or_insert_with(|| {
            reverse_owner_for_record(file, page_count, digest, loc, false, max_examples)
                .unwrap_or_else(|_| StoreObjectReverseOwnership {
                    digest,
                    record_loc: Some(record_location_attribution(loc)),
                    frame_kind: "unknown".to_string(),
                    byte_span: 0,
                    payload_bytes: 0,
                    physical_roots: Vec::new(),
                    retaining_roots: Vec::new(),
                    logical_owners: Vec::new(),
                    current_key: None,
                    retained_sequence: None,
                    rebuildable: false,
                    unresolved_reason: Some("record_metadata_unavailable".to_string()),
                })
        });
        add_reverse_owner_root(owner, retaining_root, logical_owner);
        let bytes = match read_payload_from_loc(file, digest, loc, page_count, dek) {
            Ok(bytes) => bytes,
            Err(_) => {
                mark_unresolved_owner(
                    owners,
                    digest,
                    retaining_root,
                    logical_owner,
                    "record_payload_unreadable",
                );
                continue;
            }
        };
        let object = match Object::decode(&bytes) {
            Ok(object) => object,
            Err(_) => {
                mark_unresolved_owner(
                    owners,
                    digest,
                    retaining_root,
                    logical_owner,
                    "invalid_canonical_object",
                );
                continue;
            }
        };
        for child in object_child_classifications(&object, index_entries) {
            match child {
                ObjectChildAttribution::Traversable(digest) => queue.push_back(digest),
                ObjectChildAttribution::Unresolved { digest, reason } => {
                    add_unresolved_owner(owners, digest, retaining_root, logical_owner, reason);
                }
            }
        }
    }
    Ok(())
}

enum ObjectChildAttribution {
    Traversable(Digest),
    Unresolved {
        digest: Digest,
        reason: &'static str,
    },
}

fn classify_object_child(
    digest: Digest,
    index_entries: &BTreeMap<[u8; 32], RecordLoc>,
) -> ObjectChildAttribution {
    if index_entries.contains_key(digest.bytes()) {
        ObjectChildAttribution::Traversable(digest)
    } else {
        ObjectChildAttribution::Unresolved {
            digest,
            reason: "missing_object_locator",
        }
    }
}

fn object_child_classifications(
    object: &Object,
    index_entries: &BTreeMap<[u8; 32], RecordLoc>,
) -> Vec<ObjectChildAttribution> {
    match object {
        Object::Blob(_) => Vec::new(),
        Object::ChunkList { entries, .. } => entries
            .iter()
            .map(|entry| classify_object_child(entry.target, index_entries))
            .collect(),
        Object::Tree(entries) => entries
            .iter()
            .map(|entry| classify_object_child(entry.target, index_entries))
            .collect(),
        Object::Commit(commit) => {
            let mut out = Vec::with_capacity(commit.parents.len() + 1);
            out.push(classify_object_child(commit.tree, index_entries));
            out.extend(
                commit
                    .parents
                    .iter()
                    .map(|parent| classify_object_child(*parent, index_entries)),
            );
            out
        }
        Object::Tag(tag) => vec![classify_object_child(tag.target, index_entries)],
        _ => Vec::new(),
    }
}

fn append_record_reverse_ownership(
    file: &mut dyn BackingIo,
    page_count: u64,
    digest_algo: Algo,
    family_id: Option<u16>,
    root: Option<PageId>,
    retaining_root: &str,
    logical_owner: &str,
    rebuildable: bool,
    owners: &mut BTreeMap<[u8; 32], StoreObjectReverseOwnership>,
    max_examples: usize,
) -> Result<()> {
    let Some(root) = root else {
        return Ok(());
    };
    let entries = match family_id {
        Some(family_id) => root_family_load_all(file, family_id, root, page_count)?,
        None => pagebtree::load_all(file, DATA_START, root, page_count)?,
    };
    for (_address, loc) in entries {
        let bytes = read_blob_from_loc(file, loc)?;
        let digest = Digest::hash(digest_algo, &bytes);
        let mut owner =
            reverse_owner_for_record(file, page_count, digest, loc, rebuildable, max_examples)?;
        add_reverse_owner_root(&mut owner, retaining_root, logical_owner);
        if retaining_root == "current_records"
            && let Ok(entry) = decode_mutable_overlay_entry(&bytes)
        {
            owner.current_key = Some(entry.key.as_bytes().to_vec());
        }
        if retaining_root == "retained_history"
            && let Ok((_key, sequence, _payload)) = decode_retained_history_entry(&bytes)
        {
            owner.retained_sequence = Some(sequence);
        }
        merge_reverse_owner(owners, owner);
    }
    Ok(())
}

fn reverse_owner_for_record(
    file: &mut dyn BackingIo,
    page_count: u64,
    digest: Digest,
    loc: RecordLoc,
    rebuildable: bool,
    _max_examples: usize,
) -> Result<StoreObjectReverseOwnership> {
    let (pages, payload_bytes) = record_loc_storage(file, loc, page_count)?;
    Ok(StoreObjectReverseOwnership {
        digest,
        record_loc: Some(record_location_attribution(loc)),
        frame_kind: record_frame_kind(file, loc)?,
        byte_span: pages.saturating_mul(PAGE_SIZE),
        payload_bytes,
        physical_roots: Vec::new(),
        retaining_roots: Vec::new(),
        logical_owners: Vec::new(),
        current_key: None,
        retained_sequence: None,
        rebuildable,
        unresolved_reason: None,
    })
}

fn add_reverse_owner_root(
    owner: &mut StoreObjectReverseOwnership,
    retaining_root: &str,
    logical_owner: &str,
) {
    if !owner
        .retaining_roots
        .iter()
        .any(|known| known == retaining_root)
    {
        owner.retaining_roots.push(retaining_root.to_string());
    }
    if !owner
        .logical_owners
        .iter()
        .any(|known| known == logical_owner)
    {
        owner.logical_owners.push(logical_owner.to_string());
    }
}

fn add_physical_owner_root(owner: &mut StoreObjectReverseOwnership, root: &str) {
    if !owner.physical_roots.iter().any(|known| known == root) {
        owner.physical_roots.push(root.to_string());
    }
}

fn merge_reverse_owner(
    owners: &mut BTreeMap<[u8; 32], StoreObjectReverseOwnership>,
    incoming: StoreObjectReverseOwnership,
) {
    let owner = owners
        .entry(*incoming.digest.bytes())
        .or_insert_with(|| incoming.clone());
    for physical_root in incoming.physical_roots {
        add_physical_owner_root(owner, &physical_root);
    }
    for retaining_root in incoming.retaining_roots {
        if !owner
            .retaining_roots
            .iter()
            .any(|known| known == &retaining_root)
        {
            owner.retaining_roots.push(retaining_root);
        }
    }
    for logical_owner in incoming.logical_owners {
        if !owner
            .logical_owners
            .iter()
            .any(|known| known == &logical_owner)
        {
            owner.logical_owners.push(logical_owner);
        }
    }
    if owner.record_loc.is_none() {
        owner.record_loc = incoming.record_loc;
    }
    if owner.frame_kind == "unresolved" {
        owner.frame_kind = incoming.frame_kind;
    }
    owner.byte_span = owner.byte_span.max(incoming.byte_span);
    owner.payload_bytes = owner.payload_bytes.max(incoming.payload_bytes);
    owner.current_key = owner.current_key.take().or(incoming.current_key);
    owner.retained_sequence = owner.retained_sequence.or(incoming.retained_sequence);
    owner.rebuildable |= incoming.rebuildable;
    if owner.unresolved_reason.is_none() {
        owner.unresolved_reason = incoming.unresolved_reason;
    }
}

fn add_unresolved_owner(
    owners: &mut BTreeMap<[u8; 32], StoreObjectReverseOwnership>,
    digest: Digest,
    retaining_root: &str,
    logical_owner: &str,
    reason: &str,
) {
    let mut owner = StoreObjectReverseOwnership {
        digest,
        record_loc: None,
        frame_kind: "unresolved".to_string(),
        byte_span: 0,
        payload_bytes: 0,
        physical_roots: Vec::new(),
        retaining_roots: Vec::new(),
        logical_owners: Vec::new(),
        current_key: None,
        retained_sequence: None,
        rebuildable: false,
        unresolved_reason: Some(reason.to_string()),
    };
    add_reverse_owner_root(&mut owner, retaining_root, logical_owner);
    merge_reverse_owner(owners, owner);
}

fn mark_unresolved_owner(
    owners: &mut BTreeMap<[u8; 32], StoreObjectReverseOwnership>,
    digest: Digest,
    retaining_root: &str,
    logical_owner: &str,
    reason: &str,
) {
    add_unresolved_owner(owners, digest, retaining_root, logical_owner, reason);
}

fn record_location_attribution(loc: RecordLoc) -> StoreRecordLocationAttribution {
    StoreRecordLocationAttribution {
        segment_id: loc.segment_id,
        page_index: loc.page_index,
        slot: loc.slot,
        global_page: loc.global_page(),
    }
}

fn record_frame_kind(file: &mut dyn BackingIo, loc: RecordLoc) -> Result<String> {
    let mut first = [0u8; PAGE_SIZE as usize];
    read_exact_at(
        file,
        PageId(loc.global_page()).offset(DATA_START),
        &mut first,
    )
    .map_err(io_err)?;
    Ok(match first[0] {
        record::SLAB_MAGIC => "record_slab",
        record::LARGE_MAGIC => "record_large",
        record::CHUNKED_BLOB_MAGIC => "record_chunked",
        _ => "record_unknown",
    }
    .to_string())
}

fn concrete_stale_owner_reasons(
    plan: &MutableOverlayCheckpointPlan,
    max_examples: usize,
) -> Vec<StoreStaleOwnerReason> {
    let mut by_reason = BTreeMap::<String, StoreStaleOwnerReason>::new();
    for record in &plan.current_records {
        for blocker in &record.blockers {
            let reason = reclaim_blocker_label(*blocker).to_string();
            let entry = by_reason
                .entry(reason.clone())
                .or_insert(StoreStaleOwnerReason {
                    reason,
                    pages: 0,
                    bytes: 0,
                    current_key: Some(record.key.as_bytes().to_vec()),
                    retained_sequence: None,
                    examples: Vec::new(),
                });
            entry.pages = entry.pages.saturating_add(record.page_span);
            entry.bytes = entry.bytes.saturating_add(record.bytes);
            if entry.current_key.is_none() {
                entry.current_key = Some(record.key.as_bytes().to_vec());
            }
            push_example(
                &mut entry.examples,
                format!(
                    "current_key_len:{}:generation:{}",
                    record.key.as_bytes().len(),
                    record.generation.as_u64()
                ),
                max_examples,
            );
        }
    }
    by_reason.into_values().collect()
}

fn current_record_concrete_stale_owner_reasons(
    file: &mut dyn BackingIo,
    page_count: u64,
    current_record_root: Option<PageId>,
    mvcc: &StoreMvccSnapshotDiagnostics,
    audit_retention_active: bool,
    durable_reclaim_floor: u64,
    max_examples: usize,
) -> Result<Vec<StoreStaleOwnerReason>> {
    let Some(root) = current_record_root else {
        return Ok(Vec::new());
    };
    let mut by_reason = BTreeMap::<String, StoreStaleOwnerReason>::new();
    for (_address, loc) in root_family_load_all(file, CURRENT_RECORDS_FAMILY_ID, root, page_count)?
    {
        let value = read_blob_from_loc(file, loc)?;
        let entry = decode_mutable_overlay_entry(&value)?;
        let blockers = mutable_overlay_checkpoint_record_blockers(
            entry.generation,
            entry.kind,
            mvcc,
            audit_retention_active,
            durable_reclaim_floor,
        );
        let page_span = record_io::blob_pages(file, loc.global_page(), page_count)?.len() as u64;
        for blocker in blockers {
            let reason = reclaim_blocker_label(blocker).to_string();
            let entry_reason = by_reason
                .entry(reason.clone())
                .or_insert(StoreStaleOwnerReason {
                    reason,
                    pages: 0,
                    bytes: 0,
                    current_key: Some(entry.key.as_bytes().to_vec()),
                    retained_sequence: None,
                    examples: Vec::new(),
                });
            entry_reason.pages = entry_reason.pages.saturating_add(page_span);
            entry_reason.bytes = entry_reason
                .bytes
                .saturating_add(page_span.saturating_mul(PAGE_SIZE));
            if entry_reason.current_key.is_none() {
                entry_reason.current_key = Some(entry.key.as_bytes().to_vec());
            }
            push_example(
                &mut entry_reason.examples,
                format!(
                    "current_key_len:{}:generation:{}",
                    entry.key.as_bytes().len(),
                    entry.generation.as_u64()
                ),
                max_examples,
            );
        }
    }
    Ok(by_reason.into_values().collect())
}

fn reclaim_blocker_label(blocker: MutableOverlayReclaimBlocker) -> &'static str {
    match blocker {
        MutableOverlayReclaimBlocker::CurrentIndexVisible => "current_index_visible",
        MutableOverlayReclaimBlocker::PinnedSnapshot => "pinned_snapshot",
        MutableOverlayReclaimBlocker::RetainedHistory => "retained_history_checkpoint",
        MutableOverlayReclaimBlocker::AuditRetention => "audit_retention",
        MutableOverlayReclaimBlocker::TombstoneRetention => "tombstone_retention",
        MutableOverlayReclaimBlocker::DurableGenerationWindow => "recovery_generation_floor",
        MutableOverlayReclaimBlocker::StrictPromotionBoundary => "strict_promotion_boundary",
    }
}

fn record_loc_storage(
    file: &mut dyn BackingIo,
    loc: RecordLoc,
    page_count: u64,
) -> Result<(u64, u64)> {
    let pages = record_io::blob_pages(file, loc.global_page(), page_count)?.len() as u64;
    let payload_bytes = read_blob_from_loc(file, loc)?.len() as u64;
    Ok((pages, payload_bytes))
}

// ---- compaction / GC FileStore impl lives in compact.rs ----
mod compact;
pub use compact::{
    GcCanonicalCompactionPlan, GcCanonicalRelocationStats, GcCompactionClassification,
    GcCompactionPageCandidate, GcCompactionRootPlan,
};

/// Outcome of [`FileStore::compact`]: the committed file size before and after.
#[derive(Debug, Clone, Copy)]
pub struct CompactStats {
    pub before: u64,
    pub after: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactionCapacity {
    pub required_temp_bytes: u64,
    pub available_temp_bytes: Option<u64>,
}

/// Outcome of [`FileStore::gc_segments`]: what one incremental collection reclaimed.
#[derive(Debug, Clone, Copy, Default)]
pub struct GcStats {
    pub segments_reclaimed: u64,
    pub pages_freed: u64,
    pub pages_trimmed: u64,
    pub objects_relocated: u64,
    pub objects_dropped: u64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TailCompactionStats {
    pub attempted: bool,
    pub relocated_objects: u64,
    pub relocated_pages: u64,
    pub relocated_bytes: u64,
    pub truncated_pages: u64,
    pub conflicts: u64,
    pub skipped: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GcSegmentBudget {
    pub max_segments: u64,
    pub max_pages: u64,
}

impl GcSegmentBudget {
    pub const fn unlimited() -> Self {
        Self {
            max_segments: u64::MAX,
            max_pages: u64::MAX,
        }
    }
}

impl CompactStats {
    /// Bytes reclaimed by compaction (0 if there was no dead space to recover).
    pub fn reclaimed(&self) -> u64 {
        self.before.saturating_sub(self.after)
    }
}

// ---- record / txn / control-map codec lives in record_io.rs ----
mod record_io;
pub(crate) use record_io::*;

#[cfg(test)]
fn append_audit_record(
    map: &mut BTreeMap<Vec<u8>, Vec<u8>>,
    algo: Algo,
    principal: Option<WorkspaceId>,
    action: &str,
    target: Option<&str>,
) -> Result<u64> {
    validate_audit_field("audit action", action.as_bytes(), 128)?;
    if let Some(target) = target {
        validate_audit_field("audit target", target.as_bytes(), 1024)?;
    }
    let seq = match map.get(AUDIT_NEXT_KEY) {
        Some(value) => decode_audit_next(value)?,
        None => 0,
    };
    let prev_hash = if seq == 0 {
        None
    } else {
        let prev_key = audit_entry_key(seq - 1);
        match map.get(&prev_key) {
            Some(prev_value) => Some(decode_audit_value(seq - 1, prev_value, algo)?.hash),
            None => {
                let checkpoint = map
                    .get(AUDIT_PRUNE_CHECKPOINT_KEY)
                    .map(|bytes| decode_audit_checkpoint(bytes, algo))
                    .transpose()?;
                match checkpoint {
                    Some(checkpoint) if checkpoint.seq == seq - 1 => Some(checkpoint.hash),
                    _ => return Err(corrupt("audit chain previous entry missing")),
                }
            }
        }
    };
    let value = encode_audit_value(algo, seq, prev_hash, principal, action, target);
    map.insert(audit_entry_key(seq), value);
    let next = seq
        .checked_add(1)
        .ok_or_else(|| corrupt("audit sequence overflow"))?;
    map.insert(AUDIT_NEXT_KEY.to_vec(), next.to_be_bytes().to_vec());
    Ok(seq)
}

fn validate_audit_field(name: &str, value: &[u8], max: usize) -> Result<()> {
    if value.is_empty() {
        return Err(LoomError::invalid(format!("{name} must not be empty")));
    }
    if value.len() > max {
        return Err(LoomError::invalid(format!("{name} too long")));
    }
    Ok(())
}

fn validate_served_listener_field(name: &str, value: &[u8], max: usize) -> Result<()> {
    if value.is_empty() {
        return Err(LoomError::invalid(format!("{name} must not be empty")));
    }
    if value.len() > max {
        return Err(LoomError::invalid(format!("{name} too long")));
    }
    if value
        .iter()
        .any(|byte| matches!(*byte, b'\t' | b'\n' | b'\r' | 0))
    {
        return Err(LoomError::invalid(format!(
            "{name} cannot contain control separators"
        )));
    }
    Ok(())
}

fn audit_entry_key(seq: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(AUDIT_ENTRY_PREFIX.len() + 8);
    key.extend_from_slice(AUDIT_ENTRY_PREFIX);
    key.extend_from_slice(&seq.to_be_bytes());
    key
}

fn is_audit_retention_control_key(key: &[u8]) -> bool {
    key == AUDIT_CONFIG_KEY
        || key == AUDIT_NEXT_KEY
        || key == AUDIT_PRUNE_CHECKPOINT_KEY
        || key.starts_with(AUDIT_ENTRY_PREFIX)
}

fn served_listener_key(id: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(SERVED_LISTENER_PREFIX.len() + id.len());
    key.extend_from_slice(SERVED_LISTENER_PREFIX);
    key.extend_from_slice(id.as_bytes());
    key
}

fn authority_replication_key(id: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(AUTHORITY_REPLICATION_PREFIX.len() + id.len());
    key.extend_from_slice(AUTHORITY_REPLICATION_PREFIX);
    key.extend_from_slice(id.as_bytes());
    key
}

fn certificate_bundle_key(name: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(CERTIFICATE_BUNDLE_PREFIX.len() + name.len());
    key.extend_from_slice(CERTIFICATE_BUNDLE_PREFIX);
    key.extend_from_slice(name.as_bytes());
    key
}

fn network_access_policy_key(name: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(NETWORK_ACCESS_POLICY_PREFIX.len() + name.len());
    key.extend_from_slice(NETWORK_ACCESS_POLICY_PREFIX);
    key.extend_from_slice(name.as_bytes());
    key
}

fn validate_authority_replication_id(id: &str) -> Result<()> {
    validate_served_listener_field("authority replication id", id.as_bytes(), 128)?;
    if !id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(LoomError::invalid(
            "authority replication id contains unsupported characters",
        ));
    }
    Ok(())
}

fn validate_authority_replication_source(source: &str) -> Result<()> {
    validate_served_listener_field("authority replication source", source.as_bytes(), 1024)
}

fn validate_certificate_bundle_name(name: &str) -> Result<()> {
    validate_served_listener_field("certificate bundle name", name.as_bytes(), 128)?;
    if name.starts_with('.') {
        return Err(LoomError::invalid(
            "certificate bundle name must not start with '.'",
        ));
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(LoomError::invalid(
            "certificate bundle name contains unsupported characters",
        ));
    }
    Ok(())
}

fn validate_network_access_policy_name(name: &str) -> Result<()> {
    validate_served_listener_field("network access policy name", name.as_bytes(), 128)?;
    if name.starts_with('.') {
        return Err(LoomError::invalid(
            "network access policy name must not start with '.'",
        ));
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(LoomError::invalid(
            "network access policy name contains unsupported characters",
        ));
    }
    Ok(())
}

fn served_listener_id_with_profile(
    surface: &str,
    selectors: &[String],
    transport: &str,
    profile: Option<&str>,
    bind: &str,
) -> String {
    let mut bytes = Vec::new();
    put_lp(&mut bytes, surface.as_bytes());
    put_lp(&mut bytes, transport.as_bytes());
    if let Some(profile) = profile {
        put_lp(&mut bytes, b"profile");
        put_lp(&mut bytes, profile.as_bytes());
    }
    put_lp(&mut bytes, bind.as_bytes());
    put_uvarint(&mut bytes, selectors.len() as u64);
    for selector in selectors {
        put_lp(&mut bytes, selector.as_bytes());
    }
    Digest::blake3(&bytes).to_hex()
}

fn served_listener_route_scope(surface: &str) -> &'static str {
    match surface {
        "admin" | "mcp" => "loom",
        "cas" | "files" | "vcs" | "calendar" | "contacts" | "mail" => "workspace",
        "sql" | "kv" | "document" | "queue" | "time-series" | "columnar" | "vector" | "search"
        | "graph" | "ledger" => "workspace-collection",
        _ => "surface",
    }
}

fn validate_served_listener_record(record: &ServedListenerRecord) -> Result<()> {
    validate_served_listener_field("served listener id", record.id.as_bytes(), 128)?;
    validate_served_listener_field("served listener surface", record.surface.as_bytes(), 64)?;
    validate_served_listener_field("served listener transport", record.transport.as_bytes(), 64)?;
    if let Some(profile) = &record.profile {
        validate_served_listener_field("served listener profile", profile.as_bytes(), 64)?;
    }
    validate_optional_served_listener_ref(
        "served listener network access policy ref",
        record.network_access_policy_ref.as_deref(),
    )?;
    validate_served_listener_field("served listener bind", record.bind.as_bytes(), 256)?;
    for selector in &record.selectors {
        validate_served_listener_field("served listener selector", selector.as_bytes(), 256)?;
    }
    if served_listener_id_with_profile(
        &record.surface,
        &record.selectors,
        &record.transport,
        record.profile.as_deref(),
        &record.bind,
    ) != record.id
    {
        return Err(LoomError::invalid("served listener id mismatch"));
    }
    validate_served_listener_policy(
        &record.tls,
        &record.auth,
        &record.limits,
        &record.audit,
        &record.route_scope,
        &record.exposure,
    )
}

fn validate_network_access_policy_record(record: &NetworkAccessPolicyRecord) -> Result<()> {
    validate_network_access_policy_name(&record.name)?;
    if record.schema_version != NETWORK_ACCESS_POLICY_SCHEMA_VERSION {
        return Err(LoomError::invalid(
            "unsupported network access policy schema version",
        ));
    }
    if let Some(description) = &record.description
        && !description.is_empty()
    {
        validate_served_listener_field(
            "network access policy description",
            description.as_bytes(),
            512,
        )?;
    }
    let mut ids = BTreeSet::new();
    for rule in &record.rules {
        validate_served_listener_field("network access rule id", rule.id.as_bytes(), 128)?;
        if !ids.insert(rule.id.clone()) {
            return Err(LoomError::invalid("duplicate network access rule id"));
        }
        if let Some(description) = &rule.description
            && !description.is_empty()
        {
            validate_served_listener_field(
                "network access rule description",
                description.as_bytes(),
                512,
            )?;
        }
        validate_optional_served_listener_ref(
            "network access client certificate subject",
            rule.client_cert_subject.as_deref(),
        )?;
        validate_optional_served_listener_ref(
            "network access client certificate san",
            rule.client_cert_san.as_deref(),
        )?;
        validate_optional_served_listener_ref(
            "network access client certificate issuer",
            rule.client_cert_issuer.as_deref(),
        )?;
    }
    Ok(())
}

fn validate_authority_replication_policy(policy: &AuthorityReplicationPolicy) -> Result<()> {
    validate_authority_replication_id(&policy.id)?;
    validate_authority_replication_source(&policy.source)?;
    if policy.schema_version != AUTHORITY_REPLICATION_SCHEMA_VERSION {
        return Err(LoomError::invalid(
            "unsupported authority replication schema version",
        ));
    }
    if matches!(policy.interval_ms, Some(0)) {
        return Err(LoomError::invalid(
            "authority replication interval must be positive",
        ));
    }
    if policy.backoff_ms == 0 {
        return Err(LoomError::invalid(
            "authority replication backoff must be positive",
        ));
    }
    if let Some(error) = &policy.last_error {
        validate_served_listener_field("authority replication error", error.as_bytes(), 512)?;
    }
    Ok(())
}

fn validate_certificate_bundle_record(record: &CertificateBundleRecord) -> Result<()> {
    validate_certificate_bundle_name(&record.name)?;
    validate_served_listener_token(
        "certificate bundle profile",
        &record.profile,
        &["tls-server-direct"],
    )?;
    validate_certificate_bundle_pem(
        "certificate bundle server certificate chain",
        &record.server_cert_chain_pem,
    )?;
    validate_certificate_bundle_pem("certificate bundle private key", &record.private_key_pem)?;
    if let Some(bytes) = &record.trust_bundle_pem {
        validate_certificate_bundle_pem("certificate bundle trust bundle", bytes)?;
    }
    Ok(())
}

fn validate_certificate_bundle_pem(name: &str, bytes: &[u8]) -> Result<()> {
    if bytes.is_empty() {
        return Err(LoomError::invalid(format!("{name} must not be empty")));
    }
    if bytes.len() > CERTIFICATE_BUNDLE_MAX_PEM_BYTES {
        return Err(LoomError::invalid(format!("{name} too large")));
    }
    Ok(())
}

fn decode_audit_next(value: &[u8]) -> Result<u64> {
    let bytes: [u8; 8] = value
        .try_into()
        .map_err(|_| corrupt("audit next sequence must be 8 bytes"))?;
    Ok(u64::from_be_bytes(bytes))
}

fn encode_audit_config(config: AuditConfig) -> Vec<u8> {
    let mut out = Vec::with_capacity(AUDIT_CONFIG_MAGIC.len() + 5);
    out.extend_from_slice(AUDIT_CONFIG_MAGIC);
    out.extend_from_slice(&config.retention_days.to_be_bytes());
    out.push(u8::from(config.legal_hold));
    out
}

fn decode_audit_config(value: &[u8]) -> Result<AuditConfig> {
    if value.len() != AUDIT_CONFIG_MAGIC.len() + 5 {
        return Err(corrupt("audit config length"));
    }
    if &value[..AUDIT_CONFIG_MAGIC.len()] != AUDIT_CONFIG_MAGIC {
        return Err(corrupt("bad audit config magic"));
    }
    let offset = AUDIT_CONFIG_MAGIC.len();
    let retention_days = u32::from_be_bytes(
        value[offset..offset + 4]
            .try_into()
            .map_err(|_| corrupt("audit config retention"))?,
    );
    let legal_hold = match value[offset + 4] {
        0 => false,
        1 => true,
        _ => return Err(corrupt("audit config legal-hold tag")),
    };
    Ok(AuditConfig {
        retention_days,
        legal_hold,
    })
}

fn encode_audit_retention_record(key: &[u8], value: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(AUDIT_RETENTION_RECORD);
    put_uvarint(&mut out, key.len() as u64);
    out.extend_from_slice(key);
    put_uvarint(&mut out, value.len() as u64);
    out.extend_from_slice(value);
    out
}

fn decode_audit_retention_record(bytes: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    if !bytes.starts_with(AUDIT_RETENTION_RECORD) {
        return Err(corrupt("audit-retention record schema mismatch"));
    }
    let mut pos = AUDIT_RETENTION_RECORD.len();
    let key_len = get_uvarint(bytes, &mut pos)
        .ok_or_else(|| corrupt("audit-retention record key length truncated"))?
        as usize;
    let key_end = pos
        .checked_add(key_len)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| corrupt("audit-retention record key truncated"))?;
    let key = bytes[pos..key_end].to_vec();
    pos = key_end;
    let value_len = get_uvarint(bytes, &mut pos)
        .ok_or_else(|| corrupt("audit-retention record value length truncated"))?
        as usize;
    let value_end = pos
        .checked_add(value_len)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| corrupt("audit-retention record value truncated"))?;
    let value = bytes[pos..value_end].to_vec();
    pos = value_end;
    if pos != bytes.len() {
        return Err(corrupt("audit-retention record trailing bytes"));
    }
    Ok((key, value))
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct MvccGenerationRecord {
    generation: loom_core::OverlayGeneration,
    immutable_base_root: Option<Digest>,
}

#[cfg(test)]
fn encode_mvcc_generation_record(record: &MvccGenerationRecord) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MVCC_GENERATION_RECORD);
    put_uvarint(&mut out, record.generation.as_u64());
    match record.immutable_base_root {
        Some(root) => {
            out.push(1);
            out.push(root.algo().code());
            out.extend_from_slice(root.bytes());
        }
        None => out.push(0),
    }
    out
}

#[cfg(test)]
fn decode_mvcc_generation_record(bytes: &[u8]) -> Result<MvccGenerationRecord> {
    if !bytes.starts_with(MVCC_GENERATION_RECORD) {
        return Err(corrupt("mvcc-generation record schema mismatch"));
    }
    let mut pos = MVCC_GENERATION_RECORD.len();
    let generation = loom_core::OverlayGeneration::new(
        get_uvarint(bytes, &mut pos)
            .ok_or_else(|| corrupt("mvcc-generation record generation truncated"))?,
    );
    let immutable_base_root = match bytes.get(pos).copied() {
        Some(0) => {
            pos += 1;
            None
        }
        Some(1) => {
            pos += 1;
            let algo = bytes
                .get(pos)
                .copied()
                .ok_or_else(|| corrupt("mvcc-generation record digest algorithm truncated"))
                .and_then(Algo::from_code)?;
            pos += 1;
            let digest_end = pos
                .checked_add(32)
                .filter(|end| *end <= bytes.len())
                .ok_or_else(|| corrupt("mvcc-generation record base root truncated"))?;
            let digest = Digest::of(
                algo,
                bytes[pos..digest_end]
                    .try_into()
                    .map_err(|_| corrupt("mvcc-generation record base root invalid"))?,
            );
            pos = digest_end;
            Some(digest)
        }
        _ => return Err(corrupt("mvcc-generation record base root tag invalid")),
    };
    if pos != bytes.len() {
        return Err(corrupt("mvcc-generation record trailing bytes"));
    }
    Ok(MvccGenerationRecord {
        generation,
        immutable_base_root,
    })
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct RetentionIndexRecord {
    target: loom_core::OverlayKey,
    retention_class: Vec<u8>,
    expires_at_ms: Option<u64>,
}

#[cfg(test)]
fn encode_retention_index_record(record: &RetentionIndexRecord) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(RETENTION_INDEX_RECORD);
    put_uvarint(&mut out, record.target.as_bytes().len() as u64);
    out.extend_from_slice(record.target.as_bytes());
    put_uvarint(&mut out, record.retention_class.len() as u64);
    out.extend_from_slice(&record.retention_class);
    match record.expires_at_ms {
        Some(expires_at_ms) => {
            out.push(1);
            put_uvarint(&mut out, expires_at_ms);
        }
        None => out.push(0),
    }
    out
}

#[cfg(test)]
fn decode_retention_index_record(bytes: &[u8]) -> Result<RetentionIndexRecord> {
    if !bytes.starts_with(RETENTION_INDEX_RECORD) {
        return Err(corrupt("retention-index record schema mismatch"));
    }
    let mut pos = RETENTION_INDEX_RECORD.len();
    let target_len = get_uvarint(bytes, &mut pos)
        .ok_or_else(|| corrupt("retention-index record target length truncated"))?
        as usize;
    let target_end = pos
        .checked_add(target_len)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| corrupt("retention-index record target truncated"))?;
    let target = loom_core::OverlayKey::from_encoded_bytes(bytes[pos..target_end].to_vec())?;
    pos = target_end;
    let class_len = get_uvarint(bytes, &mut pos)
        .ok_or_else(|| corrupt("retention-index record class length truncated"))?
        as usize;
    let class_end = pos
        .checked_add(class_len)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| corrupt("retention-index record class truncated"))?;
    let retention_class = bytes[pos..class_end].to_vec();
    pos = class_end;
    let expires_at_ms = match bytes.get(pos).copied() {
        Some(0) => {
            pos += 1;
            None
        }
        Some(1) => {
            pos += 1;
            Some(
                get_uvarint(bytes, &mut pos)
                    .ok_or_else(|| corrupt("retention-index record expiry truncated"))?,
            )
        }
        _ => return Err(corrupt("retention-index record expiry tag invalid")),
    };
    if pos != bytes.len() {
        return Err(corrupt("retention-index record trailing bytes"));
    }
    Ok(RetentionIndexRecord {
        target,
        retention_class,
        expires_at_ms,
    })
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct CheckpointIndexRecord {
    checkpoint_id: Vec<u8>,
    generation: loom_core::OverlayGeneration,
    base_root: Option<Digest>,
    retained_root: Option<PageId>,
}

#[cfg(test)]
fn encode_checkpoint_index_record(record: &CheckpointIndexRecord) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(CHECKPOINT_INDEX_RECORD);
    put_uvarint(&mut out, record.checkpoint_id.len() as u64);
    out.extend_from_slice(&record.checkpoint_id);
    put_uvarint(&mut out, record.generation.as_u64());
    match record.base_root {
        Some(root) => {
            out.push(1);
            out.push(root.algo().code());
            out.extend_from_slice(root.bytes());
        }
        None => out.push(0),
    }
    match record.retained_root {
        Some(root) => {
            out.push(1);
            put_uvarint(&mut out, root.0);
        }
        None => out.push(0),
    }
    out
}

#[cfg(test)]
fn decode_checkpoint_index_record(bytes: &[u8]) -> Result<CheckpointIndexRecord> {
    if !bytes.starts_with(CHECKPOINT_INDEX_RECORD) {
        return Err(corrupt("checkpoint-index record schema mismatch"));
    }
    let mut pos = CHECKPOINT_INDEX_RECORD.len();
    let id_len = get_uvarint(bytes, &mut pos)
        .ok_or_else(|| corrupt("checkpoint-index record id length truncated"))?
        as usize;
    let id_end = pos
        .checked_add(id_len)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| corrupt("checkpoint-index record id truncated"))?;
    let checkpoint_id = bytes[pos..id_end].to_vec();
    pos = id_end;
    let generation = loom_core::OverlayGeneration::new(
        get_uvarint(bytes, &mut pos)
            .ok_or_else(|| corrupt("checkpoint-index record generation truncated"))?,
    );
    let base_root = match bytes.get(pos).copied() {
        Some(0) => {
            pos += 1;
            None
        }
        Some(1) => {
            pos += 1;
            let algo = bytes
                .get(pos)
                .copied()
                .ok_or_else(|| corrupt("checkpoint-index record digest algorithm truncated"))
                .and_then(Algo::from_code)?;
            pos += 1;
            let digest_end = pos
                .checked_add(32)
                .filter(|end| *end <= bytes.len())
                .ok_or_else(|| corrupt("checkpoint-index record base root truncated"))?;
            let digest = Digest::of(
                algo,
                bytes[pos..digest_end]
                    .try_into()
                    .map_err(|_| corrupt("checkpoint-index record base root invalid"))?,
            );
            pos = digest_end;
            Some(digest)
        }
        _ => return Err(corrupt("checkpoint-index record base root tag invalid")),
    };
    let retained_root = match bytes.get(pos).copied() {
        Some(0) => {
            pos += 1;
            None
        }
        Some(1) => {
            pos += 1;
            Some(PageId(get_uvarint(bytes, &mut pos).ok_or_else(|| {
                corrupt("checkpoint-index record retained root truncated")
            })?))
        }
        _ => return Err(corrupt("checkpoint-index record retained root tag invalid")),
    };
    if pos != bytes.len() {
        return Err(corrupt("checkpoint-index record trailing bytes"));
    }
    Ok(CheckpointIndexRecord {
        checkpoint_id,
        generation,
        base_root,
        retained_root,
    })
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct ReclaimIndexRecord {
    reclaim_key: Vec<u8>,
    blocker: Vec<u8>,
    blocked_page: Option<PageId>,
    blocked_object: Option<Digest>,
}

#[cfg(test)]
fn encode_reclaim_index_record(record: &ReclaimIndexRecord) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(RECLAIM_INDEX_RECORD);
    put_uvarint(&mut out, record.reclaim_key.len() as u64);
    out.extend_from_slice(&record.reclaim_key);
    put_uvarint(&mut out, record.blocker.len() as u64);
    out.extend_from_slice(&record.blocker);
    match record.blocked_page {
        Some(page) => {
            out.push(1);
            put_uvarint(&mut out, page.0);
        }
        None => out.push(0),
    }
    match record.blocked_object {
        Some(object) => {
            out.push(1);
            out.push(object.algo().code());
            out.extend_from_slice(object.bytes());
        }
        None => out.push(0),
    }
    out
}

#[cfg(test)]
fn decode_reclaim_index_record(bytes: &[u8]) -> Result<ReclaimIndexRecord> {
    if !bytes.starts_with(RECLAIM_INDEX_RECORD) {
        return Err(corrupt("reclaim-index record schema mismatch"));
    }
    let mut pos = RECLAIM_INDEX_RECORD.len();
    let key_len = get_uvarint(bytes, &mut pos)
        .ok_or_else(|| corrupt("reclaim-index record key length truncated"))?
        as usize;
    let key_end = pos
        .checked_add(key_len)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| corrupt("reclaim-index record key truncated"))?;
    let reclaim_key = bytes[pos..key_end].to_vec();
    pos = key_end;
    let blocker_len = get_uvarint(bytes, &mut pos)
        .ok_or_else(|| corrupt("reclaim-index record blocker length truncated"))?
        as usize;
    let blocker_end = pos
        .checked_add(blocker_len)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| corrupt("reclaim-index record blocker truncated"))?;
    let blocker = bytes[pos..blocker_end].to_vec();
    pos = blocker_end;
    let blocked_page = match bytes.get(pos).copied() {
        Some(0) => {
            pos += 1;
            None
        }
        Some(1) => {
            pos += 1;
            Some(PageId(get_uvarint(bytes, &mut pos).ok_or_else(|| {
                corrupt("reclaim-index record blocked page truncated")
            })?))
        }
        _ => return Err(corrupt("reclaim-index record blocked page tag invalid")),
    };
    let blocked_object = match bytes.get(pos).copied() {
        Some(0) => {
            pos += 1;
            None
        }
        Some(1) => {
            pos += 1;
            let algo = bytes
                .get(pos)
                .copied()
                .ok_or_else(|| corrupt("reclaim-index record digest algorithm truncated"))
                .and_then(Algo::from_code)?;
            pos += 1;
            let digest_end = pos
                .checked_add(32)
                .filter(|end| *end <= bytes.len())
                .ok_or_else(|| corrupt("reclaim-index record blocked object truncated"))?;
            let digest = Digest::of(
                algo,
                bytes[pos..digest_end]
                    .try_into()
                    .map_err(|_| corrupt("reclaim-index record blocked object invalid"))?,
            );
            pos = digest_end;
            Some(digest)
        }
        _ => return Err(corrupt("reclaim-index record blocked object tag invalid")),
    };
    if pos != bytes.len() {
        return Err(corrupt("reclaim-index record trailing bytes"));
    }
    Ok(ReclaimIndexRecord {
        reclaim_key,
        blocker,
        blocked_page,
        blocked_object,
    })
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeltaPackAdvisoryKind {
    Candidate,
    Debt,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct DeltaPackAdvisoryRecord {
    advisory_key: Vec<u8>,
    kind: DeltaPackAdvisoryKind,
    generation: loom_core::OverlayGeneration,
    source_root: Option<Digest>,
    estimated_pages: u64,
    stale: bool,
}

#[cfg(test)]
fn encode_delta_pack_advisory_record(record: &DeltaPackAdvisoryRecord) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(DELTA_PACK_ADVISORY_RECORD);
    out.push(match record.kind {
        DeltaPackAdvisoryKind::Candidate => 1,
        DeltaPackAdvisoryKind::Debt => 2,
    });
    put_uvarint(&mut out, record.advisory_key.len() as u64);
    out.extend_from_slice(&record.advisory_key);
    put_uvarint(&mut out, record.generation.as_u64());
    match record.source_root {
        Some(root) => {
            out.push(1);
            out.push(root.algo().code());
            out.extend_from_slice(root.bytes());
        }
        None => out.push(0),
    }
    put_uvarint(&mut out, record.estimated_pages);
    out.push(u8::from(record.stale));
    out
}

#[cfg(test)]
fn decode_delta_pack_advisory_record(bytes: &[u8]) -> Result<DeltaPackAdvisoryRecord> {
    if !bytes.starts_with(DELTA_PACK_ADVISORY_RECORD) {
        return Err(corrupt("delta-pack advisory record schema mismatch"));
    }
    let mut pos = DELTA_PACK_ADVISORY_RECORD.len();
    let kind = match bytes.get(pos).copied() {
        Some(1) => {
            pos += 1;
            DeltaPackAdvisoryKind::Candidate
        }
        Some(2) => {
            pos += 1;
            DeltaPackAdvisoryKind::Debt
        }
        _ => return Err(corrupt("delta-pack advisory record kind invalid")),
    };
    let key_len = get_uvarint(bytes, &mut pos)
        .ok_or_else(|| corrupt("delta-pack advisory record key length truncated"))?
        as usize;
    let key_end = pos
        .checked_add(key_len)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| corrupt("delta-pack advisory record key truncated"))?;
    let advisory_key = bytes[pos..key_end].to_vec();
    pos = key_end;
    let generation = loom_core::OverlayGeneration::new(
        get_uvarint(bytes, &mut pos)
            .ok_or_else(|| corrupt("delta-pack advisory record generation truncated"))?,
    );
    let source_root = match bytes.get(pos).copied() {
        Some(0) => {
            pos += 1;
            None
        }
        Some(1) => {
            pos += 1;
            let algo = bytes
                .get(pos)
                .copied()
                .ok_or_else(|| corrupt("delta-pack advisory record digest algorithm truncated"))
                .and_then(Algo::from_code)?;
            pos += 1;
            let digest_end = pos
                .checked_add(32)
                .filter(|end| *end <= bytes.len())
                .ok_or_else(|| corrupt("delta-pack advisory record source root truncated"))?;
            let digest = Digest::of(
                algo,
                bytes[pos..digest_end]
                    .try_into()
                    .map_err(|_| corrupt("delta-pack advisory record source root invalid"))?,
            );
            pos = digest_end;
            Some(digest)
        }
        _ => {
            return Err(corrupt(
                "delta-pack advisory record source root tag invalid",
            ));
        }
    };
    let estimated_pages = get_uvarint(bytes, &mut pos)
        .ok_or_else(|| corrupt("delta-pack advisory record page estimate truncated"))?;
    let stale = match bytes.get(pos).copied() {
        Some(0) => {
            pos += 1;
            false
        }
        Some(1) => {
            pos += 1;
            true
        }
        _ => return Err(corrupt("delta-pack advisory record stale flag invalid")),
    };
    if pos != bytes.len() {
        return Err(corrupt("delta-pack advisory record trailing bytes"));
    }
    Ok(DeltaPackAdvisoryRecord {
        advisory_key,
        kind,
        generation,
        source_root,
        estimated_pages,
        stale,
    })
}

fn encode_store_policy(policy: StorePolicy) -> Vec<u8> {
    let override_count = policy
        .facet_durability_overrides
        .iter()
        .filter(|value| value.is_some())
        .count();
    let mut out = Vec::with_capacity(STORE_POLICY_MAGIC.len() + 5 + override_count * 2);
    out.extend_from_slice(STORE_POLICY_MAGIC);
    out.push(2);
    out.push(u8::from(policy.fips_required));
    out.push(encode_durability_policy_tag(policy.default_durability));
    out.extend_from_slice(&(override_count as u16).to_be_bytes());
    for (idx, policy) in policy.facet_durability_overrides.iter().enumerate() {
        if let Some(policy) = policy {
            out.push(idx as u8);
            out.push(encode_durability_policy_tag(*policy));
        }
    }
    out
}

fn decode_store_policy(value: &[u8]) -> Result<StorePolicy> {
    if value.len() < STORE_POLICY_MAGIC.len() {
        return Err(corrupt("store policy truncated"));
    }
    if &value[..STORE_POLICY_MAGIC.len()] != STORE_POLICY_MAGIC {
        return Err(corrupt("bad store policy magic"));
    }
    if value.len() == STORE_POLICY_MAGIC.len() + 1 {
        let fips_required = match value[STORE_POLICY_MAGIC.len()] {
            0 => false,
            1 => true,
            _ => return Err(corrupt("store policy FIPS-required tag")),
        };
        return Ok(StorePolicy {
            fips_required,
            ..StorePolicy::default()
        });
    }
    let mut pos = STORE_POLICY_MAGIC.len();
    let Some(version) = value.get(pos).copied() else {
        return Err(corrupt("store policy version missing"));
    };
    pos += 1;
    if version != 2 {
        return Err(corrupt("unsupported store policy version"));
    }
    let fips_required = match value.get(pos).copied() {
        Some(0) => false,
        Some(1) => true,
        Some(_) => return Err(corrupt("store policy FIPS-required tag")),
        None => return Err(corrupt("store policy FIPS-required tag missing")),
    };
    pos += 1;
    let default_durability = decode_durability_policy_tag(
        value
            .get(pos)
            .copied()
            .ok_or_else(|| corrupt("store policy default durability missing"))?,
    )?;
    pos += 1;
    if pos + 2 > value.len() {
        return Err(corrupt("store policy facet override count missing"));
    }
    let count = u16::from_be_bytes(
        value[pos..pos + 2]
            .try_into()
            .map_err(|_| corrupt("store policy facet override count invalid"))?,
    ) as usize;
    pos += 2;
    let mut policy = StorePolicy {
        fips_required,
        default_durability,
        facet_durability_overrides: [None; FacetKind::ALL.len()],
    };
    for _ in 0..count {
        if pos + 2 > value.len() {
            return Err(corrupt("store policy facet override truncated"));
        }
        let facet = FacetKind::from_stable_tag(value[pos])
            .ok_or_else(|| corrupt("store policy facet override tag"))?;
        let durability = decode_durability_policy_tag(value[pos + 1])?;
        policy.set_facet_durability(facet, Some(durability))?;
        pos += 2;
    }
    if pos != value.len() {
        return Err(corrupt("store policy trailing bytes"));
    }
    Ok(policy)
}

fn encode_durability_policy_tag(policy: StoreDurabilityPolicy) -> u8 {
    match policy {
        StoreDurabilityPolicy::Strict => 0,
        StoreDurabilityPolicy::Normal => 1,
        StoreDurabilityPolicy::Relaxed => 2,
        StoreDurabilityPolicy::Ephemeral => 3,
    }
}

fn decode_durability_policy_tag(value: u8) -> Result<StoreDurabilityPolicy> {
    match value {
        0 => Ok(StoreDurabilityPolicy::Strict),
        1 => Ok(StoreDurabilityPolicy::Normal),
        2 => Ok(StoreDurabilityPolicy::Relaxed),
        3 => Ok(StoreDurabilityPolicy::Ephemeral),
        _ => Err(corrupt("store policy durability tag")),
    }
}

fn mutable_overlay_key_facet(key: &loom_core::OverlayKey) -> Result<Option<FacetKind>> {
    let segments = key.segments()?;
    if segments.len() != 6 {
        return Ok(None);
    }
    if segments[2] == b"loom.document.current.v1" || segments[2] == b"documents" {
        return Ok(Some(FacetKind::Document));
    }
    let Ok(name) = std::str::from_utf8(segments[2]) else {
        return Ok(None);
    };
    match FacetKind::parse(name) {
        Ok(facet) => Ok(Some(facet)),
        Err(_) => Ok(None),
    }
}

fn encode_authority_replication_policy(policy: &AuthorityReplicationPolicy) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(AUTHORITY_REPLICATION_MAGIC);
    put_uvarint(&mut out, u64::from(policy.schema_version));
    put_lp(&mut out, policy.id.as_bytes());
    put_lp(&mut out, policy.source.as_bytes());
    out.push(u8::from(policy.enabled));
    out.push(u8::from(policy.pull_on_start));
    encode_optional_u64(&mut out, policy.interval_ms);
    out.extend_from_slice(&policy.jitter_ms.to_be_bytes());
    out.extend_from_slice(&policy.backoff_ms.to_be_bytes());
    out.push(u8::from(policy.publish_witness));
    encode_optional_u64(&mut out, policy.last_success_ms);
    encode_optional_u64(&mut out, policy.last_failure_ms);
    put_optional_served_listener_string(&mut out, policy.last_error.as_deref());
    encode_optional_u64(&mut out, policy.last_modified_audit_seq);
    out
}

fn decode_authority_replication_entry(
    key: &[u8],
    value: &[u8],
) -> Result<AuthorityReplicationPolicy> {
    let id_from_key = std::str::from_utf8(
        key.strip_prefix(AUTHORITY_REPLICATION_PREFIX)
            .ok_or_else(|| corrupt("authority replication key prefix"))?,
    )
    .map_err(|e| corrupt(&format!("invalid authority replication key utf8: {e}")))?;
    let policy = decode_authority_replication_policy(value)?;
    if policy.id != id_from_key {
        return Err(corrupt("authority replication id does not match key"));
    }
    Ok(policy)
}

fn decode_authority_replication_policy(value: &[u8]) -> Result<AuthorityReplicationPolicy> {
    if value.len() < AUTHORITY_REPLICATION_MAGIC.len() {
        return Err(corrupt("authority replication policy truncated"));
    }
    if &value[..AUTHORITY_REPLICATION_MAGIC.len()] != AUTHORITY_REPLICATION_MAGIC {
        return Err(corrupt("bad authority replication policy magic"));
    }
    let mut pos = AUTHORITY_REPLICATION_MAGIC.len();
    let schema_version = get_uvarint(value, &mut pos)
        .ok_or_else(|| corrupt("authority replication schema version"))?;
    if schema_version != u64::from(AUTHORITY_REPLICATION_SCHEMA_VERSION) {
        return Err(corrupt("unsupported authority replication schema version"));
    }
    let id = decode_authority_replication_id(value, &mut pos)?;
    let source = decode_authority_replication_source(value, &mut pos)?;
    let enabled = match take_u8(value, &mut pos)? {
        0 => false,
        1 => true,
        _ => return Err(corrupt("authority replication enabled tag")),
    };
    let pull_on_start = match take_u8(value, &mut pos)? {
        0 => false,
        1 => true,
        _ => return Err(corrupt("authority replication pull-on-start tag")),
    };
    let interval_ms =
        decode_optional_served_listener_u64(value, &mut pos, "authority replication interval")?;
    let jitter_ms = take_u64(value, &mut pos, "authority replication jitter")?;
    let backoff_ms = take_u64(value, &mut pos, "authority replication backoff")?;
    let publish_witness = match take_u8(value, &mut pos)? {
        0 => false,
        1 => true,
        _ => return Err(corrupt("authority replication publish-witness tag")),
    };
    let last_success_ms =
        decode_optional_served_listener_u64(value, &mut pos, "authority replication last success")?;
    let last_failure_ms =
        decode_optional_served_listener_u64(value, &mut pos, "authority replication last failure")?;
    let last_error =
        decode_optional_served_listener_string(value, &mut pos, "authority replication error")?;
    let last_modified_audit_seq =
        decode_optional_served_listener_u64(value, &mut pos, "authority replication audit seq")?;
    if pos != value.len() {
        return Err(corrupt("authority replication policy trailing bytes"));
    }
    let policy = AuthorityReplicationPolicy {
        id,
        schema_version: AUTHORITY_REPLICATION_SCHEMA_VERSION,
        source,
        enabled,
        pull_on_start,
        interval_ms,
        jitter_ms,
        backoff_ms,
        publish_witness,
        last_success_ms,
        last_failure_ms,
        last_error,
        last_modified_audit_seq,
    };
    validate_authority_replication_policy(&policy)?;
    Ok(policy)
}

fn decode_authority_replication_id(value: &[u8], pos: &mut usize) -> Result<String> {
    let id = decode_audit_string(value, pos, "authority replication id")?;
    validate_authority_replication_id(&id)?;
    Ok(id)
}

fn decode_authority_replication_source(value: &[u8], pos: &mut usize) -> Result<String> {
    let source = decode_audit_string(value, pos, "authority replication source")?;
    validate_authority_replication_source(&source)?;
    Ok(source)
}

fn encode_audit_checkpoint(checkpoint: AuditCheckpoint) -> Vec<u8> {
    let mut out = Vec::with_capacity(AUDIT_CHECKPOINT_MAGIC.len() + 40);
    out.extend_from_slice(AUDIT_CHECKPOINT_MAGIC);
    out.extend_from_slice(&checkpoint.seq.to_be_bytes());
    out.extend_from_slice(checkpoint.hash.bytes());
    out
}

fn decode_audit_checkpoint(value: &[u8], algo: Algo) -> Result<AuditCheckpoint> {
    if value.len() != AUDIT_CHECKPOINT_MAGIC.len() + 40 {
        return Err(corrupt("audit checkpoint length"));
    }
    if &value[..AUDIT_CHECKPOINT_MAGIC.len()] != AUDIT_CHECKPOINT_MAGIC {
        return Err(corrupt("bad audit checkpoint magic"));
    }
    let seq_offset = AUDIT_CHECKPOINT_MAGIC.len();
    let seq = u64::from_be_bytes(
        value[seq_offset..seq_offset + 8]
            .try_into()
            .map_err(|_| corrupt("audit checkpoint sequence"))?,
    );
    let hash_offset = seq_offset + 8;
    let hash = Digest::of(
        algo,
        value[hash_offset..hash_offset + 32]
            .try_into()
            .map_err(|_| corrupt("audit checkpoint hash"))?,
    );
    Ok(AuditCheckpoint { seq, hash })
}

fn encode_certificate_bundle(record: &CertificateBundleRecord) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(CERTIFICATE_BUNDLE_MAGIC);
    put_uvarint(&mut out, u64::from(record.schema_version));
    put_lp(&mut out, record.name.as_bytes());
    put_lp(&mut out, record.profile.as_bytes());
    put_lp(&mut out, &record.server_cert_chain_pem);
    put_lp(&mut out, &record.private_key_pem);
    put_optional_bytes(&mut out, record.trust_bundle_pem.as_deref());
    out.extend_from_slice(record.server_cert_chain_digest.bytes());
    out.extend_from_slice(record.private_key_digest.bytes());
    match record.trust_bundle_digest {
        Some(digest) => {
            out.push(1);
            out.extend_from_slice(digest.bytes());
        }
        None => out.push(0),
    }
    encode_optional_u64(&mut out, record.created_audit_seq);
    encode_optional_u64(&mut out, record.updated_audit_seq);
    out.push(u8::from(record.unencrypted_private_key_override));
    out
}

fn decode_certificate_bundle_entry(
    key: &[u8],
    value: &[u8],
    algo: Algo,
) -> Result<CertificateBundleRecord> {
    let name_from_key = std::str::from_utf8(
        key.strip_prefix(CERTIFICATE_BUNDLE_PREFIX)
            .ok_or_else(|| corrupt("certificate bundle key prefix"))?,
    )
    .map_err(|e| corrupt(&format!("invalid certificate bundle key utf8: {e}")))?;
    let record = decode_certificate_bundle(value, algo)?;
    if record.name != name_from_key {
        return Err(corrupt("certificate bundle name does not match key"));
    }
    Ok(record)
}

fn decode_certificate_bundle(value: &[u8], algo: Algo) -> Result<CertificateBundleRecord> {
    if value.len() < CERTIFICATE_BUNDLE_MAGIC.len() {
        return Err(corrupt("certificate bundle truncated"));
    }
    if &value[..CERTIFICATE_BUNDLE_MAGIC.len()] != CERTIFICATE_BUNDLE_MAGIC {
        return Err(corrupt("bad certificate bundle magic"));
    }
    let mut pos = CERTIFICATE_BUNDLE_MAGIC.len();
    let schema_version =
        get_uvarint(value, &mut pos).ok_or_else(|| corrupt("certificate bundle schema version"))?;
    if schema_version != u64::from(CERTIFICATE_BUNDLE_SCHEMA_VERSION) {
        return Err(corrupt("unsupported certificate bundle schema version"));
    }
    let name = decode_certificate_bundle_string(value, &mut pos, "certificate bundle name")?;
    let profile = decode_certificate_bundle_string(value, &mut pos, "certificate bundle profile")?;
    let server_cert_chain_pem = decode_certificate_bundle_bytes(
        value,
        &mut pos,
        "certificate bundle server certificate chain",
    )?;
    let private_key_pem =
        decode_certificate_bundle_bytes(value, &mut pos, "certificate bundle private key")?;
    let trust_bundle_pem = decode_optional_certificate_bundle_bytes(
        value,
        &mut pos,
        "certificate bundle trust bundle",
    )?;
    let server_cert_chain_digest = Digest::of(algo, take_32(value, &mut pos)?);
    let private_key_digest = Digest::of(algo, take_32(value, &mut pos)?);
    let trust_bundle_digest = match take_u8(value, &mut pos)? {
        0 => None,
        1 => Some(Digest::of(algo, take_32(value, &mut pos)?)),
        _ => return Err(corrupt("certificate bundle optional digest tag")),
    };
    let created_audit_seq =
        decode_optional_served_listener_u64(value, &mut pos, "certificate bundle created seq")?;
    let updated_audit_seq =
        decode_optional_served_listener_u64(value, &mut pos, "certificate bundle updated seq")?;
    let unencrypted_private_key_override = match take_u8(value, &mut pos)? {
        0 => false,
        1 => true,
        _ => return Err(corrupt("certificate bundle unencrypted override tag")),
    };
    if pos != value.len() {
        return Err(corrupt("certificate bundle trailing bytes"));
    }
    let record = CertificateBundleRecord {
        name,
        schema_version: CERTIFICATE_BUNDLE_SCHEMA_VERSION,
        profile,
        server_cert_chain_pem,
        private_key_pem,
        trust_bundle_pem,
        server_cert_chain_digest,
        private_key_digest,
        trust_bundle_digest,
        created_audit_seq,
        updated_audit_seq,
        unencrypted_private_key_override,
    };
    validate_certificate_bundle_record(&record)?;
    validate_certificate_bundle_digests(&record, algo)?;
    Ok(record)
}

fn encode_network_access_policy(record: &NetworkAccessPolicyRecord) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(NETWORK_ACCESS_POLICY_MAGIC);
    put_uvarint(&mut out, u64::from(record.schema_version));
    put_lp(&mut out, record.name.as_bytes());
    put_optional_served_listener_string(&mut out, record.description.as_deref());
    out.push(network_access_action_tag(record.default_action));
    put_uvarint(&mut out, record.rules.len() as u64);
    for rule in &record.rules {
        put_lp(&mut out, rule.id.as_bytes());
        out.push(network_access_action_tag(rule.action));
        put_optional_network_access_cidr(&mut out, rule.source_cidr);
        put_optional_network_access_cidr(&mut out, rule.trusted_proxy_cidr);
        out.push(u8::from(rule.require_mtls));
        put_optional_served_listener_string(&mut out, rule.client_cert_subject.as_deref());
        put_optional_served_listener_string(&mut out, rule.client_cert_san.as_deref());
        put_optional_served_listener_string(&mut out, rule.client_cert_issuer.as_deref());
        put_optional_served_listener_string(&mut out, rule.description.as_deref());
    }
    encode_optional_u64(&mut out, record.created_audit_seq);
    encode_optional_u64(&mut out, record.updated_audit_seq);
    out
}

fn decode_network_access_policy_entry(
    key: &[u8],
    value: &[u8],
) -> Result<NetworkAccessPolicyRecord> {
    let name_from_key = std::str::from_utf8(
        key.strip_prefix(NETWORK_ACCESS_POLICY_PREFIX)
            .ok_or_else(|| corrupt("network access policy key prefix"))?,
    )
    .map_err(|e| corrupt(&format!("invalid network access policy key utf8: {e}")))?;
    let record = decode_network_access_policy(value)?;
    if record.name != name_from_key {
        return Err(corrupt("network access policy name does not match key"));
    }
    Ok(record)
}

fn decode_network_access_policy(value: &[u8]) -> Result<NetworkAccessPolicyRecord> {
    if value.len() < NETWORK_ACCESS_POLICY_MAGIC.len() {
        return Err(corrupt("network access policy truncated"));
    }
    if &value[..NETWORK_ACCESS_POLICY_MAGIC.len()] != NETWORK_ACCESS_POLICY_MAGIC {
        return Err(corrupt("bad network access policy magic"));
    }
    let mut pos = NETWORK_ACCESS_POLICY_MAGIC.len();
    let schema_version = get_uvarint(value, &mut pos)
        .ok_or_else(|| corrupt("network access policy schema version"))?;
    if schema_version != u64::from(NETWORK_ACCESS_POLICY_SCHEMA_VERSION) {
        return Err(corrupt("unsupported network access policy schema version"));
    }
    let name = decode_network_access_string(value, &mut pos, "network access policy name")?;
    let description = decode_optional_network_access_description(
        value,
        &mut pos,
        "network access policy description",
    )?;
    let default_action = decode_network_access_action(value, &mut pos)?;
    let rule_count =
        get_uvarint(value, &mut pos).ok_or_else(|| corrupt("network access policy rule count"))?;
    let rule_count: usize = rule_count
        .try_into()
        .map_err(|_| corrupt("network access policy rule count overflow"))?;
    let mut rules = Vec::with_capacity(rule_count);
    for _ in 0..rule_count {
        rules.push(NetworkAccessRule {
            id: decode_network_access_string(value, &mut pos, "network access rule id")?,
            action: decode_network_access_action(value, &mut pos)?,
            source_cidr: decode_optional_network_access_cidr(value, &mut pos)?,
            trusted_proxy_cidr: decode_optional_network_access_cidr(value, &mut pos)?,
            require_mtls: match take_u8(value, &mut pos)? {
                0 => false,
                1 => true,
                _ => return Err(corrupt("network access rule mTLS tag")),
            },
            client_cert_subject: decode_optional_served_listener_string(
                value,
                &mut pos,
                "network access client certificate subject",
            )?,
            client_cert_san: decode_optional_served_listener_string(
                value,
                &mut pos,
                "network access client certificate san",
            )?,
            client_cert_issuer: decode_optional_served_listener_string(
                value,
                &mut pos,
                "network access client certificate issuer",
            )?,
            description: decode_optional_network_access_description(
                value,
                &mut pos,
                "network access rule description",
            )?,
        });
    }
    let created_audit_seq =
        decode_optional_served_listener_u64(value, &mut pos, "network access policy created seq")?;
    let updated_audit_seq =
        decode_optional_served_listener_u64(value, &mut pos, "network access policy updated seq")?;
    if pos != value.len() {
        return Err(corrupt("network access policy trailing bytes"));
    }
    let record = NetworkAccessPolicyRecord {
        name,
        schema_version: NETWORK_ACCESS_POLICY_SCHEMA_VERSION,
        description,
        default_action,
        rules,
        created_audit_seq,
        updated_audit_seq,
    };
    validate_network_access_policy_record(&record)?;
    Ok(record)
}

fn encode_served_listener(record: &ServedListenerRecord) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(SERVED_LISTENER_MAGIC);
    put_lp(&mut out, record.id.as_bytes());
    put_uvarint(&mut out, u64::from(record.schema_version));
    put_lp(&mut out, record.surface.as_bytes());
    put_uvarint(&mut out, record.selectors.len() as u64);
    for selector in &record.selectors {
        put_lp(&mut out, selector.as_bytes());
    }
    put_lp(&mut out, record.transport.as_bytes());
    put_lp(&mut out, record.bind.as_bytes());
    out.push(u8::from(record.enabled));
    put_lp(&mut out, record.tls.mode.as_bytes());
    put_optional_served_listener_string(&mut out, record.tls.certificate_bundle_ref.as_deref());
    put_lp(&mut out, record.auth.mode.as_bytes());
    out.extend_from_slice(&record.limits.request_size_limit.to_be_bytes());
    out.extend_from_slice(&record.limits.idle_timeout_ms.to_be_bytes());
    out.extend_from_slice(&record.limits.session_timeout_ms.to_be_bytes());
    put_lp(&mut out, record.audit.mode.as_bytes());
    put_lp(&mut out, record.route_scope.as_bytes());
    put_lp(&mut out, record.exposure.as_bytes());
    if record.schema_version >= 2 {
        match record.last_modified_audit_seq {
            Some(seq) => {
                out.push(1);
                out.extend_from_slice(&seq.to_be_bytes());
            }
            None => out.push(0),
        }
        put_optional_served_listener_string(&mut out, record.profile.as_deref());
    }
    if record.schema_version >= 3 {
        put_optional_served_listener_string(&mut out, record.network_access_policy_ref.as_deref());
    }
    out
}

fn decode_served_listener_entry(key: &[u8], value: &[u8]) -> Result<ServedListenerRecord> {
    let id_from_key = std::str::from_utf8(
        key.strip_prefix(SERVED_LISTENER_PREFIX)
            .ok_or_else(|| corrupt("served listener key prefix"))?,
    )
    .map_err(|e| corrupt(&format!("invalid served listener key utf8: {e}")))?;
    let record = decode_served_listener(value)?;
    if record.id != id_from_key {
        return Err(corrupt("served listener id does not match key"));
    }
    Ok(record)
}

fn decode_served_listener(value: &[u8]) -> Result<ServedListenerRecord> {
    if value.len() < SERVED_LISTENER_MAGIC.len() {
        return Err(corrupt("served listener truncated"));
    }
    if &value[..SERVED_LISTENER_MAGIC.len()] != SERVED_LISTENER_MAGIC {
        return Err(corrupt("bad served listener magic"));
    }
    let mut pos = SERVED_LISTENER_MAGIC.len();
    let id = decode_served_listener_string(value, &mut pos, "served listener id")?;
    let schema_version = match get_uvarint(value, &mut pos) {
        Some(2) => 2,
        Some(3) => 3,
        _ => return Err(corrupt("unsupported served listener schema version")),
    };
    let surface = decode_served_listener_string(value, &mut pos, "served listener surface")?;
    let selector_len =
        get_uvarint(value, &mut pos).ok_or_else(|| corrupt("served listener selector count"))?;
    let selector_len: usize = selector_len
        .try_into()
        .map_err(|_| corrupt("served listener selector count overflow"))?;
    let mut selectors = Vec::with_capacity(selector_len);
    for _ in 0..selector_len {
        selectors.push(decode_served_listener_string(
            value,
            &mut pos,
            "served listener selector",
        )?);
    }
    let transport = decode_served_listener_string(value, &mut pos, "served listener transport")?;
    let bind = decode_served_listener_string(value, &mut pos, "served listener bind")?;
    let enabled = match take_u8(value, &mut pos)? {
        0 => false,
        1 => true,
        _ => return Err(corrupt("served listener enabled tag")),
    };
    let mut tls = ServedListenerTls::default();
    let mut auth = ServedListenerAuth::default();
    let mut limits = ServedListenerLimits::default();
    let mut audit = ServedListenerAudit::default();
    let mut route_scope = served_listener_route_scope(&surface).to_string();
    let mut exposure = "read-write".to_string();
    let mut last_modified_audit_seq = None;
    let mut profile = None;
    let mut network_access_policy_ref = None;
    if pos != value.len() {
        tls.mode = decode_served_listener_string(value, &mut pos, "served listener tls mode")?;
        tls.certificate_bundle_ref = decode_optional_served_listener_string(
            value,
            &mut pos,
            "served listener tls certificate bundle ref",
        )?;
        auth.mode = decode_served_listener_string(value, &mut pos, "served listener auth mode")?;
        limits.request_size_limit =
            take_u64(value, &mut pos, "served listener request size limit")?;
        limits.idle_timeout_ms = take_u64(value, &mut pos, "served listener idle timeout")?;
        limits.session_timeout_ms = take_u64(value, &mut pos, "served listener session timeout")?;
        audit.mode = decode_served_listener_string(value, &mut pos, "served listener audit mode")?;
        route_scope =
            decode_served_listener_string(value, &mut pos, "served listener route scope")?;
        exposure = decode_served_listener_string(value, &mut pos, "served listener exposure")?;
        if pos != value.len() {
            last_modified_audit_seq =
                decode_optional_served_listener_u64(value, &mut pos, "served listener audit seq")?;
        }
        if pos != value.len() {
            profile =
                decode_optional_served_listener_string(value, &mut pos, "served listener profile")?;
        }
        if pos != value.len() {
            network_access_policy_ref = decode_optional_served_listener_string(
                value,
                &mut pos,
                "served listener network access policy ref",
            )?;
        }
        validate_served_listener_policy(&tls, &auth, &limits, &audit, &route_scope, &exposure)?;
    }
    if pos != value.len() {
        return Err(corrupt("served listener trailing bytes"));
    }
    if served_listener_id_with_profile(&surface, &selectors, &transport, profile.as_deref(), &bind)
        != id
    {
        return Err(corrupt("served listener id mismatch"));
    }
    Ok(ServedListenerRecord {
        id,
        schema_version,
        surface,
        selectors,
        transport,
        profile,
        bind,
        enabled,
        tls,
        auth,
        limits,
        audit,
        route_scope,
        exposure,
        network_access_policy_ref,
        last_modified_audit_seq,
    })
}

fn decode_network_access_string(value: &[u8], pos: &mut usize, label: &str) -> Result<String> {
    let out = decode_audit_string(value, pos, label)?;
    validate_served_listener_field(label, out.as_bytes(), 512)?;
    Ok(out)
}

fn decode_served_listener_string(value: &[u8], pos: &mut usize, label: &str) -> Result<String> {
    let out = decode_audit_string(value, pos, label)?;
    validate_served_listener_field(label, out.as_bytes(), 256)?;
    Ok(out)
}

fn decode_certificate_bundle_string(value: &[u8], pos: &mut usize, label: &str) -> Result<String> {
    let out = decode_audit_string(value, pos, label)?;
    validate_served_listener_field(label, out.as_bytes(), 256)?;
    Ok(out)
}

fn decode_certificate_bundle_bytes(value: &[u8], pos: &mut usize, label: &str) -> Result<Vec<u8>> {
    let bytes = decode_lp_bytes(value, pos, label)?;
    validate_certificate_bundle_pem(label, &bytes)?;
    Ok(bytes)
}

fn validate_certificate_bundle_digests(record: &CertificateBundleRecord, algo: Algo) -> Result<()> {
    if record.server_cert_chain_digest != Digest::hash(algo, &record.server_cert_chain_pem) {
        return Err(LoomError::integrity_failure(
            "certificate bundle server certificate digest mismatch",
        ));
    }
    if record.private_key_digest != Digest::hash(algo, &record.private_key_pem) {
        return Err(LoomError::integrity_failure(
            "certificate bundle private key digest mismatch",
        ));
    }
    let expected_trust_digest = record
        .trust_bundle_pem
        .as_ref()
        .map(|bytes| Digest::hash(algo, bytes));
    if record.trust_bundle_digest != expected_trust_digest {
        return Err(LoomError::integrity_failure(
            "certificate bundle trust bundle digest mismatch",
        ));
    }
    Ok(())
}

fn put_optional_bytes(out: &mut Vec<u8>, value: Option<&[u8]>) {
    match value {
        Some(value) => {
            out.push(1);
            put_lp(out, value);
        }
        None => out.push(0),
    }
}

fn put_optional_served_listener_string(out: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(value) => {
            out.push(1);
            put_lp(out, value.as_bytes());
        }
        None => out.push(0),
    }
}

fn network_access_action_tag(action: NetworkAccessAction) -> u8 {
    match action {
        NetworkAccessAction::Allow => 1,
        NetworkAccessAction::Deny => 2,
    }
}

fn decode_network_access_action(value: &[u8], pos: &mut usize) -> Result<NetworkAccessAction> {
    match take_u8(value, pos)? {
        1 => Ok(NetworkAccessAction::Allow),
        2 => Ok(NetworkAccessAction::Deny),
        _ => Err(corrupt("network access action tag")),
    }
}

fn put_optional_network_access_cidr(out: &mut Vec<u8>, value: Option<NetworkAccessCidr>) {
    match value {
        Some(value) => {
            out.push(1);
            put_network_access_cidr(out, value);
        }
        None => out.push(0),
    }
}

fn put_network_access_cidr(out: &mut Vec<u8>, value: NetworkAccessCidr) {
    match value.addr {
        IpAddr::V4(addr) => {
            out.push(4);
            out.extend_from_slice(&addr.octets());
        }
        IpAddr::V6(addr) => {
            out.push(6);
            out.extend_from_slice(&addr.octets());
        }
    }
    out.push(value.prefix);
}

fn decode_optional_network_access_cidr(
    value: &[u8],
    pos: &mut usize,
) -> Result<Option<NetworkAccessCidr>> {
    match take_u8(value, pos)? {
        0 => Ok(None),
        1 => Ok(Some(decode_network_access_cidr(value, pos)?)),
        _ => Err(corrupt("network access optional CIDR tag")),
    }
}

fn decode_network_access_cidr(value: &[u8], pos: &mut usize) -> Result<NetworkAccessCidr> {
    let family = take_u8(value, pos)?;
    let addr = match family {
        4 => {
            let end = pos
                .checked_add(4)
                .ok_or_else(|| corrupt("network access CIDR length overflow"))?;
            let bytes: [u8; 4] = value
                .get(*pos..end)
                .ok_or_else(|| corrupt("network access IPv4 CIDR truncated"))?
                .try_into()
                .map_err(|_| corrupt("network access IPv4 CIDR truncated"))?;
            *pos = end;
            IpAddr::V4(Ipv4Addr::from(bytes))
        }
        6 => {
            let end = pos
                .checked_add(16)
                .ok_or_else(|| corrupt("network access CIDR length overflow"))?;
            let bytes: [u8; 16] = value
                .get(*pos..end)
                .ok_or_else(|| corrupt("network access IPv6 CIDR truncated"))?
                .try_into()
                .map_err(|_| corrupt("network access IPv6 CIDR truncated"))?;
            *pos = end;
            IpAddr::V6(Ipv6Addr::from(bytes))
        }
        _ => return Err(corrupt("network access CIDR family")),
    };
    let prefix = take_u8(value, pos)?;
    NetworkAccessCidr::new(addr, prefix)
}

fn decode_optional_served_listener_string(
    value: &[u8],
    pos: &mut usize,
    label: &str,
) -> Result<Option<String>> {
    match take_u8(value, pos)? {
        0 => Ok(None),
        1 => Ok(Some(decode_served_listener_string(value, pos, label)?)),
        _ => Err(corrupt("served listener optional string tag")),
    }
}

fn decode_optional_network_access_description(
    value: &[u8],
    pos: &mut usize,
    label: &str,
) -> Result<Option<String>> {
    match take_u8(value, pos)? {
        0 => Ok(None),
        1 => {
            let out = decode_audit_string(value, pos, label)?;
            if !out.is_empty() {
                validate_served_listener_field(label, out.as_bytes(), 512)?;
            }
            Ok(Some(out))
        }
        _ => Err(corrupt("network access optional description tag")),
    }
}

fn decode_optional_certificate_bundle_bytes(
    value: &[u8],
    pos: &mut usize,
    label: &str,
) -> Result<Option<Vec<u8>>> {
    match take_u8(value, pos)? {
        0 => Ok(None),
        1 => Ok(Some(decode_certificate_bundle_bytes(value, pos, label)?)),
        _ => Err(corrupt("certificate bundle optional bytes tag")),
    }
}

fn encode_optional_u64(out: &mut Vec<u8>, value: Option<u64>) {
    match value {
        Some(value) => {
            out.push(1);
            out.extend_from_slice(&value.to_be_bytes());
        }
        None => out.push(0),
    }
}

fn decode_optional_served_listener_u64(
    value: &[u8],
    pos: &mut usize,
    label: &str,
) -> Result<Option<u64>> {
    match take_u8(value, pos)? {
        0 => Ok(None),
        1 => Ok(Some(take_u64(value, pos, label)?)),
        _ => Err(corrupt("served listener optional u64 tag")),
    }
}

fn take_u64(value: &[u8], pos: &mut usize, label: &str) -> Result<u64> {
    let end = pos
        .checked_add(8)
        .ok_or_else(|| corrupt("served listener length overflow"))?;
    let bytes: [u8; 8] = value
        .get(*pos..end)
        .ok_or_else(|| corrupt(label))?
        .try_into()
        .map_err(|_| corrupt(label))?;
    *pos = end;
    Ok(u64::from_be_bytes(bytes))
}

fn decode_lp_bytes(value: &[u8], pos: &mut usize, label: &str) -> Result<Vec<u8>> {
    let len = get_uvarint(value, pos).ok_or_else(|| corrupt(label))?;
    let len: usize = len
        .try_into()
        .map_err(|_| corrupt("length-prefixed bytes length overflow"))?;
    let end = pos
        .checked_add(len)
        .ok_or_else(|| corrupt("length-prefixed bytes length overflow"))?;
    if end > value.len() {
        return Err(corrupt("length-prefixed bytes truncated"));
    }
    let out = value[*pos..end].to_vec();
    *pos = end;
    Ok(out)
}

fn validate_served_listener_policy(
    tls: &ServedListenerTls,
    auth: &ServedListenerAuth,
    limits: &ServedListenerLimits,
    audit: &ServedListenerAudit,
    route_scope: &str,
    exposure: &str,
) -> Result<()> {
    validate_served_listener_token(
        "served listener tls mode",
        &tls.mode,
        &["off", "direct", "starttls"],
    )?;
    validate_optional_served_listener_ref(
        "served listener tls certificate bundle ref",
        tls.certificate_bundle_ref.as_deref(),
    )?;
    validate_served_listener_token(
        "served listener auth mode",
        &auth.mode,
        &["owner-or-passphrase", "passphrase"],
    )?;
    validate_served_listener_token(
        "served listener audit mode",
        &audit.mode,
        &["management-and-security", "all"],
    )?;
    validate_served_listener_token(
        "served listener route scope",
        route_scope,
        &["loom", "workspace", "workspace-collection", "surface"],
    )?;
    validate_served_listener_token(
        "served listener exposure",
        exposure,
        &["read-only", "read-write"],
    )?;
    if limits.request_size_limit == 0
        || limits.idle_timeout_ms == 0
        || limits.session_timeout_ms == 0
    {
        return Err(LoomError::invalid(
            "served listener limits must be positive",
        ));
    }
    if matches!(tls.mode.as_str(), "direct" | "starttls") && tls.certificate_bundle_ref.is_none() {
        return Err(LoomError::invalid(
            "TLS listeners require a certificate bundle reference",
        ));
    }
    if tls.mode == "off" && tls.certificate_bundle_ref.is_some() {
        return Err(LoomError::invalid(
            "off TLS listeners cannot carry a certificate bundle reference",
        ));
    }
    Ok(())
}

fn validate_served_listener_token(name: &str, value: &str, allowed: &[&str]) -> Result<()> {
    validate_served_listener_field(name, value.as_bytes(), 64)?;
    if !allowed.contains(&value) {
        return Err(LoomError::invalid(format!("{name} is unsupported")));
    }
    Ok(())
}

fn validate_optional_served_listener_ref(name: &str, value: Option<&str>) -> Result<()> {
    if let Some(value) = value {
        validate_served_listener_field(name, value.as_bytes(), 256)?;
    }
    Ok(())
}

fn encode_audit_value(
    algo: Algo,
    seq: u64,
    prev_hash: Option<Digest>,
    principal: Option<WorkspaceId>,
    action: &str,
    target: Option<&str>,
) -> Vec<u8> {
    let mut body = Vec::new();
    encode_audit_body(&mut body, seq, prev_hash, principal, action, target);
    let hash = Digest::hash(algo, &body);
    body.extend_from_slice(hash.bytes());
    body
}

fn encode_audit_body(
    out: &mut Vec<u8>,
    seq: u64,
    prev_hash: Option<Digest>,
    principal: Option<WorkspaceId>,
    action: &str,
    target: Option<&str>,
) {
    out.extend_from_slice(AUDIT_RECORD_MAGIC);
    put_uvarint(out, seq);
    match prev_hash {
        Some(hash) => {
            out.push(1);
            out.extend_from_slice(hash.bytes());
        }
        None => out.push(0),
    }
    match principal {
        Some(principal) => {
            out.push(1);
            out.extend_from_slice(principal.as_bytes());
        }
        None => out.push(0),
    }
    put_lp(out, action.as_bytes());
    match target {
        Some(target) => {
            out.push(1);
            put_lp(out, target.as_bytes());
        }
        None => out.push(0),
    }
}

fn put_lp(out: &mut Vec<u8>, bytes: &[u8]) {
    put_uvarint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

fn decode_audit_entry(key: &[u8], value: &[u8], algo: Algo) -> Result<AuditRecord> {
    let suffix = key
        .strip_prefix(AUDIT_ENTRY_PREFIX)
        .ok_or_else(|| corrupt("audit entry key prefix"))?;
    let seq_bytes: [u8; 8] = suffix
        .try_into()
        .map_err(|_| corrupt("audit entry key sequence"))?;
    let seq = u64::from_be_bytes(seq_bytes);
    decode_audit_value(seq, value, algo)
}

fn decode_audit_value(seq_from_key: u64, value: &[u8], algo: Algo) -> Result<AuditRecord> {
    if value.len() < AUDIT_RECORD_MAGIC.len() + 32 {
        return Err(corrupt("audit record truncated"));
    }
    let hash_start = value.len() - 32;
    let body = &value[..hash_start];
    let stored_hash_bytes: [u8; 32] = value[hash_start..]
        .try_into()
        .map_err(|_| corrupt("audit record hash length"))?;
    let stored_hash = Digest::of(algo, stored_hash_bytes);
    let expected_hash = Digest::hash(algo, body);
    if stored_hash != expected_hash {
        return Err(LoomError::integrity_failure("audit record hash mismatch"));
    }
    if body.len() < AUDIT_RECORD_MAGIC.len()
        || &body[..AUDIT_RECORD_MAGIC.len()] != AUDIT_RECORD_MAGIC
    {
        return Err(corrupt("bad audit record magic"));
    }
    let mut pos = AUDIT_RECORD_MAGIC.len();
    let seq = get_uvarint(body, &mut pos).ok_or_else(|| corrupt("audit record sequence"))?;
    if seq != seq_from_key {
        return Err(corrupt("audit record sequence does not match key"));
    }
    let prev_hash = match take_u8(body, &mut pos)? {
        0 => None,
        1 => Some(Digest::of(algo, take_32(body, &mut pos)?)),
        _ => return Err(corrupt("audit record prev-hash tag")),
    };
    let principal = match take_u8(body, &mut pos)? {
        0 => None,
        1 => Some(WorkspaceId::from_bytes(take_16(body, &mut pos)?)),
        _ => return Err(corrupt("audit record principal tag")),
    };
    let action = decode_audit_string(body, &mut pos, "audit record action")?;
    let target = match take_u8(body, &mut pos)? {
        0 => None,
        1 => Some(decode_audit_string(body, &mut pos, "audit record target")?),
        _ => return Err(corrupt("audit record target tag")),
    };
    if pos != body.len() {
        return Err(corrupt("audit record trailing bytes"));
    }
    Ok(AuditRecord {
        seq,
        principal,
        action,
        target,
        prev_hash,
        hash: stored_hash,
    })
}

fn decode_audit_string(bytes: &[u8], pos: &mut usize, label: &str) -> Result<String> {
    let len = get_uvarint(bytes, pos).ok_or_else(|| corrupt(label))? as usize;
    let end = pos
        .checked_add(len)
        .ok_or_else(|| corrupt("audit record length overflow"))?;
    if end > bytes.len() {
        return Err(corrupt("audit record string truncated"));
    }
    let out = std::str::from_utf8(&bytes[*pos..end])
        .map_err(|e| corrupt(&format!("invalid audit record utf8: {e}")))?
        .to_string();
    *pos = end;
    Ok(out)
}

fn take_u8(bytes: &[u8], pos: &mut usize) -> Result<u8> {
    let value = *bytes
        .get(*pos)
        .ok_or_else(|| corrupt("audit record truncated"))?;
    *pos += 1;
    Ok(value)
}

fn take_16(bytes: &[u8], pos: &mut usize) -> Result<[u8; 16]> {
    let end = pos
        .checked_add(16)
        .ok_or_else(|| corrupt("audit record length overflow"))?;
    if end > bytes.len() {
        return Err(corrupt("audit record truncated"));
    }
    let out = bytes[*pos..end]
        .try_into()
        .map_err(|_| corrupt("audit record truncated"))?;
    *pos = end;
    Ok(out)
}

fn take_32(bytes: &[u8], pos: &mut usize) -> Result<[u8; 32]> {
    let end = pos
        .checked_add(32)
        .ok_or_else(|| corrupt("audit record length overflow"))?;
    if end > bytes.len() {
        return Err(corrupt("audit record truncated"));
    }
    let out = bytes[*pos..end]
        .try_into()
        .map_err(|_| corrupt("audit record truncated"))?;
    *pos = end;
    Ok(out)
}

fn verify_audit_chain(
    mut records: Vec<AuditRecord>,
    checkpoint: Option<AuditCheckpoint>,
) -> Result<Vec<AuditRecord>> {
    records.sort_by_key(|record| record.seq);
    let mut prev_hash = checkpoint.map(|value| value.hash);
    let mut expected_seq = match checkpoint {
        Some(value) => value
            .seq
            .checked_add(1)
            .ok_or_else(|| corrupt("audit sequence overflow"))?,
        None => 0,
    };
    for record in &records {
        if record.seq != expected_seq {
            return Err(corrupt("audit record sequence gap"));
        }
        if record.prev_hash != prev_hash {
            return Err(LoomError::integrity_failure(
                "audit chain previous hash mismatch",
            ));
        }
        prev_hash = Some(record.hash);
        expected_seq = expected_seq
            .checked_add(1)
            .ok_or_else(|| corrupt("audit sequence overflow"))?;
    }
    Ok(records)
}

impl ObjectStore for FileStore {
    fn put(&self, canonical: &[u8]) -> Result<Digest> {
        let digest = Digest::hash(self.digest_algo, canonical);
        #[cfg(any(test, feature = "test-hooks"))]
        observe_store_publication(&self.path, StorePublicationTestEvent::DirectPut);
        // One object joins the group-commit queue: concurrent puts coalesce into one fsync, and a
        // repeat or already-stored object is deduped under the lock (the reference root is preserved).
        self.group_commit(&[(digest, canonical, self.default_codec)])?;
        Ok(digest)
    }

    fn put_hint(&self, canonical: &[u8], hint: CompressionHint) -> Result<Digest> {
        let digest = Digest::hash(self.digest_algo, canonical);
        #[cfg(any(test, feature = "test-hooks"))]
        observe_store_publication(&self.path, StorePublicationTestEvent::DirectPutHint);
        // The workspace's hint picks the codec for this object; guardrails still apply (frame.rs).
        self.group_commit(&[(digest, canonical, frame::codec_for_hint(hint))])?;
        Ok(digest)
    }

    fn get(&self, digest: &Digest) -> Result<Option<Vec<u8>>> {
        let (loc, page_count) = {
            let mut inner = self.inner.lock().map_err(|_| poisoned())?;
            match self.lookup_loc_locked(&mut inner, digest.bytes())? {
                Some(loc) => (loc, inner.page_count),
                None => {
                    let page_count = inner.page_count;
                    drop(inner);
                    let view = self.copy_source_read_view.lock().map_err(|_| poisoned())?;
                    match view
                        .as_ref()
                        .and_then(|view| view.historical_index.as_ref())
                        .and_then(|index| {
                            index
                                .binary_search_by(|(key, _)| key.as_slice().cmp(digest.bytes()))
                                .ok()
                                .map(|slot| index[slot].1)
                        }) {
                        Some(loc) => (loc, page_count),
                        None => return Ok(None),
                    }
                }
            }
        };
        self.read_object_payload_at_loc(loc, page_count, digest)
            .map(Some)
    }

    fn has(&self, digest: &Digest) -> Result<bool> {
        let mut inner = self.inner.lock().map_err(|_| poisoned())?;
        if self
            .lookup_loc_locked(&mut inner, digest.bytes())?
            .is_some()
        {
            return Ok(true);
        }
        drop(inner);
        let view = self.copy_source_read_view.lock().map_err(|_| poisoned())?;
        Ok(view
            .as_ref()
            .and_then(|view| view.historical_index.as_ref())
            .is_some_and(|index| {
                index
                    .binary_search_by(|(key, _)| key.as_slice().cmp(digest.bytes()))
                    .is_ok()
            }))
    }

    fn len(&self) -> usize {
        self.inner
            .lock()
            .map(|i| i.maintenance.object_count as usize)
            .unwrap_or(0)
    }

    fn digest_algo(&self) -> Algo {
        self.digest_algo
    }

    fn put_mutable_overlay_value(
        &self,
        key: loom_core::OverlayKey,
        payload: Vec<u8>,
    ) -> Result<loom_core::OverlayOwnerToken> {
        FileStore::put_mutable_overlay_value(self, key, payload)
    }

    fn put_mutable_overlay_tombstone(
        &self,
        key: loom_core::OverlayKey,
    ) -> Result<loom_core::OverlayOwnerToken> {
        FileStore::put_mutable_overlay_tombstone(self, key)
    }

    fn uses_mutable_overlay_current_records(&self) -> bool {
        true
    }

    fn mutable_overlay_current_entries(
        &self,
    ) -> Result<Vec<loom_core::MutableOverlayEntrySnapshot>> {
        #[cfg(any(test, feature = "test-hooks"))]
        MUTABLE_OVERLAY_CURRENT_ENTRIES_ENUMERATIONS.fetch_add(1, Ordering::SeqCst);
        FileStore::mutable_overlay_entries(self)
    }

    fn mutable_overlay_current_entries_with_prefix(
        &self,
        key_prefix: &loom_core::OverlayKeyPrefix,
    ) -> Result<Vec<loom_core::MutableOverlayEntrySnapshot>> {
        FileStore::mutable_overlay_entries_with_prefix(self, key_prefix)
    }

    fn mutable_overlay_current_entry(
        &self,
        key: &loom_core::OverlayKey,
    ) -> Result<Option<loom_core::MutableOverlayEntrySnapshot>> {
        FileStore::mutable_overlay_current_entry(self, key)
    }

    fn mutable_overlay_generation(&self) -> Result<loom_core::OverlayGeneration> {
        FileStore::mutable_overlay_generation(self)
    }

    fn retained_history_head(&self, key: &[u8]) -> Result<u64> {
        FileStore::retained_history_head(self, key)
    }

    fn retained_history_records(
        &self,
        key: &[u8],
        first_sequence: u64,
        max: usize,
    ) -> Result<Vec<Vec<u8>>> {
        FileStore::retained_history_records(self, key, first_sequence, max)
    }

    fn mutable_overlay_owner_token(
        &self,
        key: &loom_core::OverlayKey,
    ) -> Result<Option<loom_core::OverlayOwnerToken>> {
        FileStore::mutable_overlay_owner_token(self, key)
    }

    fn open_mutable_overlay_read_snapshot(
        &self,
        snapshot: loom_core::OverlaySnapshot,
        owner: Option<&str>,
    ) -> Result<loom_core::OverlayReadSnapshot> {
        let _publication_guard = self.overlay_publication.lock().map_err(|_| poisoned())?;
        let immutable_base_root = self.inner.lock().map_err(|_| poisoned())?.reference_root;
        let store_snapshot =
            self.register_mvcc_snapshot(snapshot.clone(), immutable_base_root, owner)?;
        Ok(loom_core::OverlayReadSnapshot::new(
            snapshot,
            immutable_base_root,
            Some(Box::new(store_snapshot)),
        ))
    }

    fn open_workflow_planning_snapshot(
        &self,
        owner: Option<&str>,
    ) -> Result<loom_core::OverlayReadSnapshot> {
        let store_snapshot = self.open_mvcc_snapshot_with_owner(owner)?;
        Ok(loom_core::OverlayReadSnapshot::new(
            store_snapshot.snapshot.clone(),
            store_snapshot.identity.immutable_base_root,
            Some(Box::new(store_snapshot)),
        ))
    }

    fn commit_workflow_transaction(&self, txn: WorkflowTransaction) -> Result<CommitReceipt> {
        FileStore::commit_workflow_transaction(self, txn)
    }

    fn control_get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        FileStore::control_get(self, key)
    }

    fn control_set(&self, key: &[u8], value: Vec<u8>) -> Result<()> {
        FileStore::control_set(self, key, value)
    }

    fn acl_store_control_write(
        &self,
        acl: &loom_core::AclStore,
    ) -> loom_core::WorkflowControlWrite {
        FileStore::acl_store_control_write(self, acl)
    }
}

// ---- full-engine persistence (reference root) -----------------------------------------------------

/// Finish opening a [`Loom`] over an already-opened `store`: unlock it first if it is encrypted (the
/// engine-state root object is itself a sealed frame, so `load_state` below cannot read it while the
/// store is locked), then load the registry + content map + working trees from the reference root.
/// An encrypted store with no `key` is a clear `E2eLocked` rather than a confusing decode failure.
fn finish_open(
    store: FileStore,
    key: Option<&loom_core::keys::KeySpec>,
) -> Result<Loom<FileStore>> {
    finish_open_with_progress(store, key, |_| {})
}

fn finish_open_with_progress(
    store: FileStore,
    key: Option<&loom_core::keys::KeySpec>,
    mut progress: impl FnMut(LoomOpenProgress),
) -> Result<Loom<FileStore>> {
    if store.is_encrypted() {
        match key {
            Some(k) => {
                progress(LoomOpenProgress::Loom(LoomOpenPhaseProgress {
                    stage: LoomOpenStage::Unlock,
                    completed: 0,
                    total: Some(1),
                }));
                store.unlock(k)?;
                progress(LoomOpenProgress::Loom(LoomOpenPhaseProgress {
                    stage: LoomOpenStage::Unlock,
                    completed: 1,
                    total: Some(1),
                }));
            }
            None if store.is_unlocked() => {}
            None => {
                return Err(LoomError::new(
                    Code::E2eLocked,
                    "loom-store: this loom is encrypted; a passphrase/key is required to open it",
                ));
            }
        }
    }
    progress(LoomOpenProgress::Loom(LoomOpenPhaseProgress {
        stage: LoomOpenStage::RuntimePolicy,
        completed: 0,
        total: Some(1),
    }));
    store.validate_runtime_policy()?;
    progress(LoomOpenProgress::Loom(LoomOpenPhaseProgress {
        stage: LoomOpenStage::RuntimePolicy,
        completed: 1,
        total: Some(1),
    }));
    let root = store.reference_root();
    let mut loom = Loom::new(store);
    if let Some(root) = root {
        loom.load_state_with_progress(root, |event| progress(LoomOpenProgress::Engine(event)))?;
    }
    progress(LoomOpenProgress::Loom(LoomOpenPhaseProgress {
        stage: LoomOpenStage::MutableOverlayExport,
        completed: 0,
        total: None,
    }));
    let entries = loom
        .store()
        .mutable_overlay_entries_with_progress(|completed, total| {
            progress(LoomOpenProgress::Loom(LoomOpenPhaseProgress {
                stage: LoomOpenStage::MutableOverlayExport,
                completed,
                total: Some(total),
            }));
        })?;
    progress(LoomOpenProgress::Loom(LoomOpenPhaseProgress {
        stage: LoomOpenStage::MutableOverlayExport,
        completed: entries.len() as u64,
        total: Some(entries.len() as u64),
    }));
    *loom.mutable_overlay_mut() =
        loom_core::MutableOverlay::import_entries_with_progress(&entries, |completed, total| {
            progress(LoomOpenProgress::Loom(LoomOpenPhaseProgress {
                stage: LoomOpenStage::MutableOverlayImport,
                completed,
                total: Some(total),
            }));
        })?;
    progress(LoomOpenProgress::Ready);
    Ok(loom)
}

fn finish_open_registry(
    store: FileStore,
    key: Option<&loom_core::keys::KeySpec>,
) -> Result<Loom<FileStore>> {
    if store.is_encrypted() {
        match key {
            Some(k) => {
                store.unlock(k)?;
            }
            None if store.is_unlocked() => {}
            None => {
                return Err(LoomError::new(
                    Code::E2eLocked,
                    "loom-store: this loom is encrypted; a passphrase/key is required to open it",
                ));
            }
        }
    }
    store.validate_runtime_policy()?;
    let root = store.reference_root();
    let mut loom = Loom::new(store);
    if let Some(root) = root {
        loom.load_state_lazy(root)?;
    }
    Ok(loom)
}

/// Open a complete [`Loom`] from a `.loom` file: open the [`FileStore`], then if a reference (engine-state)
/// root is recorded in the superblock, load the registry + content map and re-check-out every
/// workspace's HEAD. A fresh file yields an empty engine. Reverse of [`save_loom`]. Errors with
/// `E2eLocked` on an encrypted loom; use [`open_loom_unlocked`] with the passphrase for those.
#[cfg(not(target_arch = "wasm32"))]
pub fn open_loom(path: impl AsRef<Path>) -> Result<Loom<FileStore>> {
    let store = FileStore::open(path)?;
    finish_open(store, None)
}

/// Like [`open_loom`], but unlocks an encrypted loom with `key` before loading engine state (the
/// reference-root object is a sealed frame). `key` is ignored for an unencrypted loom.
#[cfg(not(target_arch = "wasm32"))]
pub fn open_loom_unlocked(
    path: impl AsRef<Path>,
    key: Option<&KeySpec>,
) -> Result<Loom<FileStore>> {
    let store = FileStore::open(path)?;
    finish_open(store, key)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn open_loom_daemon_authorized_unlocked(
    path: impl AsRef<Path>,
    key: Option<&KeySpec>,
) -> Result<Loom<FileStore>> {
    let store = FileStore::open_daemon_authorized(path)?;
    finish_open(store, key)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn open_loom_daemon_authorized_unlocked_with_progress(
    path: impl AsRef<Path>,
    key: Option<&KeySpec>,
    mut progress: impl FnMut(LoomOpenProgress),
) -> Result<Loom<FileStore>> {
    let store = FileStore::open_daemon_authorized_with_progress(path, |event| {
        progress(LoomOpenProgress::Store(event))
    })?;
    finish_open_with_progress(store, key, progress)
}

/// Open a complete [`Loom`] read-only and lock-free (via [`FileStore::open_read`]): for read-only
/// commands that should not exclude a writer or other readers. Mutating the returned engine and
/// persisting it fails, since the underlying store descriptor is read-only. Errors with `E2eLocked` on
/// an encrypted loom; use [`open_loom_read_unlocked`].
#[cfg(not(target_arch = "wasm32"))]
pub fn open_loom_read(path: impl AsRef<Path>) -> Result<Loom<FileStore>> {
    let store = FileStore::open_read(path)?;
    finish_open(store, None)
}

/// Like [`open_loom_read`], but unlocks an encrypted loom with `key` before loading engine state.
#[cfg(not(target_arch = "wasm32"))]
pub fn open_loom_read_unlocked(
    path: impl AsRef<Path>,
    key: Option<&KeySpec>,
) -> Result<Loom<FileStore>> {
    let store = FileStore::open_read(path)?;
    finish_open(store, key)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn open_loom_registry_read_unlocked(
    path: impl AsRef<Path>,
    key: Option<&KeySpec>,
) -> Result<Loom<FileStore>> {
    let store = FileStore::open_read(path)?;
    finish_open_registry(store, key)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn open_store_metadata_checked(path: impl AsRef<Path>) -> Result<FileStore> {
    let store = FileStore::open_read(path)?;
    store.validate_runtime_policy()?;
    Ok(store)
}

#[derive(Clone, Default)]
pub struct LocalOpenAuth {
    pub unlock_key: Option<KeySpec>,
    pub principal: Option<WorkspaceId>,
    pub passphrase: Option<String>,
    pub app_credential: Option<String>,
    pub verified_external: Option<VerifiedExternalCredential>,
    pub preauthenticated_principal: Option<WorkspaceId>,
    pub session_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedExternalCredential {
    pub kind: ExternalCredentialKind,
    pub issuer: String,
    pub subject: String,
    pub material_digest: Option<String>,
    pub challenge_id: Option<WorkspaceId>,
}

pub fn attach_local_auth(
    mut loom: Loom<FileStore>,
    auth: &LocalOpenAuth,
) -> Result<Loom<FileStore>> {
    attach_local_auth_in_place(&mut loom, auth)?;
    Ok(loom)
}

pub fn attach_local_auth_in_place(loom: &mut Loom<FileStore>, auth: &LocalOpenAuth) -> Result<()> {
    let persist_identity = auth
        .verified_external
        .as_ref()
        .and_then(|credential| credential.challenge_id)
        .is_some();
    if let Some(mut identity) = loom.store().identity_store()? {
        if auth.preauthenticated_principal.is_some()
            && (auth.principal.is_some()
                || auth.passphrase.is_some()
                || auth.app_credential.is_some()
                || auth.verified_external.is_some())
        {
            return Err(LoomError::invalid(
                "preauthenticated principal cannot be combined with local credentials",
            ));
        }
        if let Some(principal) = auth.preauthenticated_principal {
            let session = identity.bind_session(
                principal,
                auth.session_id
                    .clone()
                    .unwrap_or_else(default_local_session_id),
            )?;
            loom.set_session(session.id);
        } else if let Some(app_credential) = &auth.app_credential {
            let session = identity.authenticate_app_credential(
                app_credential,
                auth.session_id
                    .clone()
                    .unwrap_or_else(default_local_session_id),
            )?;
            loom.set_session(session.id);
        } else if let Some(credential) = &auth.verified_external {
            let session_id = auth
                .session_id
                .clone()
                .unwrap_or_else(default_local_session_id);
            let session = identity.authenticate_verified_external_credential(
                VerifiedExternalCredentialAuth {
                    kind: credential.kind,
                    issuer: &credential.issuer,
                    subject: &credential.subject,
                    material_digest: credential.material_digest.as_deref(),
                    challenge_id: credential.challenge_id,
                    now_ms: local_now_ms(),
                    session_id: &session_id,
                },
            )?;
            loom.set_session(session.id);
        } else if let Some(principal) = auth.principal {
            let passphrase = auth.passphrase.as_ref().ok_or_else(|| {
                LoomError::new(
                    Code::AuthenticationFailed,
                    "loom-store: principal passphrase is required",
                )
            })?;
            let session = identity.authenticate_passphrase(
                principal,
                passphrase,
                auth.session_id
                    .clone()
                    .unwrap_or_else(default_local_session_id),
            )?;
            loom.set_session(session.id);
        }
        if persist_identity {
            loom.store().save_identity_store(&identity)?;
        }
        loom.set_identity_store(identity);
    }
    if let Some(acl) = loom.store().acl_store()? {
        loom.set_acl_store(acl);
    }
    Ok(())
}

pub fn local_auth_requires_write(auth: &LocalOpenAuth) -> bool {
    auth.verified_external
        .as_ref()
        .and_then(|credential| credential.challenge_id)
        .is_some()
}

fn default_local_session_id() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        "local".to_string()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        format!("local-{}", std::process::id())
    }
}

fn local_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().min(u128::from(u64::MAX)) as u64
        })
}

/// Open a complete [`Loom`] over a caller-supplied [`BackingIo`] instead of a native file - the browser
/// path: the wasm binding acquires an OPFS sync handle, wraps it as a `BackingIo`, and calls
/// this. Same recovery as [`open_loom`] (load the engine-state root if present). Persist with
/// [`save_loom`]; compaction is native-file-only (see [`FileStore::with_backing`]).
pub fn loom_over_backing(backing: Box<dyn BackingIo>, writable: bool) -> Result<Loom<FileStore>> {
    let store = FileStore::with_backing(backing, writable)?;
    finish_open(store, None)
}

/// Like [`loom_over_backing`], but unlocks an encrypted backing with `key` before loading engine state
/// (the browser-side counterpart of [`open_loom_unlocked`]).
pub fn loom_over_backing_unlocked(
    backing: Box<dyn BackingIo>,
    writable: bool,
    key: Option<&loom_core::keys::KeySpec>,
) -> Result<Loom<FileStore>> {
    let store = FileStore::with_backing(backing, writable)?;
    finish_open(store, key)
}

/// Create a fresh [`Loom`] over a caller-supplied backing under an explicit identity profile (the
/// browser/in-memory counterpart of [`open_loom`] with [`FileStore::create_with_profile`]).
/// The backing must be empty.
pub fn loom_over_backing_profile(
    backing: Box<dyn BackingIo>,
    writable: bool,
    digest_algo: Algo,
) -> Result<Loom<FileStore>> {
    let store = FileStore::with_backing_profile(backing, writable, digest_algo)?;
    finish_open(store, None)
}

/// Create a fresh **encrypted** [`Loom`] over a caller-supplied backing (the browser/in-memory
/// counterpart of [`open_loom`] with [`FileStore::create_encrypted_with_profile`]). The
/// caller builds `encryption_meta` + the unlocked `session` (via [`loom_core::keys::EncryptionMeta::create`])
/// and passes them in; the returned Loom is already unlocked, so no key is needed to load engine state.
/// The backing must be empty.
pub fn loom_over_backing_encrypted(
    backing: Box<dyn BackingIo>,
    encryption_meta: Vec<u8>,
    session: loom_core::keys::DekSession,
    digest_algo: Algo,
) -> Result<Loom<FileStore>> {
    let store = FileStore::with_backing_encrypted(backing, encryption_meta, session, digest_algo)?;
    finish_open(store, None)
}

/// Persist a complete [`Loom`]: serialize the engine state and publish its digest as the file's
/// reference root in one store transaction. Reverse of [`open_loom`].
pub fn save_loom(loom: &mut Loom<FileStore>) -> Result<()> {
    let (root, objects) = loom.save_state_objects()?;
    loom.store_mut()
        .put_batch_and_set_reference_root(&objects, root)
}

/// Publish a previously serialized engine state and audit records in one owner-state transaction.
pub fn put_saved_state_and_audit(
    store: &FileStore,
    saved: loom_core::vcs::SavedStateObjects,
    audits: Vec<loom_core::WorkflowAuditWrite>,
) -> Result<SavedStateAndAuditReceipt> {
    #[cfg(any(test, feature = "test-hooks"))]
    observe_store_publication(&store.path, StorePublicationTestEvent::SavedStateAndAudit);
    let _publication_guard = store.overlay_publication.lock().map_err(|_| poisoned())?;
    let (root, objects) = saved;
    let owner_state = loom_core::WorkflowOwnerState {
        objects,
        reference: loom_core::WorkflowReferenceUpdate::Set(Some(root)),
        controls: Vec::new(),
        audits,
    };
    store.commit_workflow_owner_state_records(&[], &owner_state, None)
}

#[allow(clippy::too_many_arguments)]
pub fn put_saved_state_served_listener_controls_audited(
    store: &FileStore,
    saved: loom_core::vcs::SavedStateObjects,
    listener: &ServedListenerRecord,
    controls: Vec<loom_core::WorkflowControlWrite>,
    audits: Vec<loom_core::WorkflowAuditWrite>,
    principal: Option<WorkspaceId>,
    action: &str,
    target: Option<&str>,
) -> Result<SavedStateAndAuditReceipt> {
    validate_served_listener_record(listener)?;
    let _publication_guard = store.overlay_publication.lock().map_err(|_| poisoned())?;
    let next_value = store.audit_delta_payload(&AuditRetentionDelta::default(), AUDIT_NEXT_KEY)?;
    let listener_audit_sequence = match next_value {
        Some(value) => decode_audit_next(&value)?,
        None => 0,
    };
    let listener_key = served_listener_key(&listener.id);
    if controls.iter().any(|control| match control {
        loom_core::WorkflowControlWrite::Put { key, .. }
        | loom_core::WorkflowControlWrite::Delete { key }
        | loom_core::WorkflowControlWrite::AppendRetained { key, .. } => key == &listener_key,
    }) {
        return Err(LoomError::invalid(
            "additional control writes cannot replace the served listener record",
        ));
    }
    let mut stored = listener.clone();
    stored.schema_version = SERVED_LISTENER_SCHEMA_VERSION;
    stored.last_modified_audit_seq = Some(listener_audit_sequence);
    let mut owner_controls = Vec::with_capacity(controls.len() + 1);
    owner_controls.push(loom_core::WorkflowControlWrite::Put {
        key: listener_key,
        payload: encode_served_listener(&stored),
    });
    owner_controls.extend(controls);
    let mut owner_audits = Vec::with_capacity(audits.len() + 1);
    owner_audits.push(loom_core::WorkflowAuditWrite {
        principal,
        action: action.to_string(),
        target: target.map(str::to_string),
    });
    owner_audits.extend(audits);
    let (root, objects) = saved;
    let owner_state = loom_core::WorkflowOwnerState {
        objects,
        reference: loom_core::WorkflowReferenceUpdate::Set(Some(root)),
        controls: owner_controls,
        audits: owner_audits,
    };
    let receipt = store.commit_workflow_owner_state_records(&[], &owner_state, None)?;
    if receipt.audit_sequences.first().copied() != Some(listener_audit_sequence) {
        return Err(LoomError::corrupt(
            "served listener audit sequence changed during atomic publication",
        ));
    }
    Ok(receipt)
}

pub fn ensure_engine_state_base(loom: &mut Loom<FileStore>) -> Result<Digest> {
    if let Some(root) = loom.store().reference_root() {
        return Ok(root);
    }
    save_loom(loom)?;
    loom.store()
        .reference_root()
        .ok_or_else(|| LoomError::corrupt("engine-state base root was not published"))
}

/// Garbage-collect a `.loom`: keep only the objects reachable from the engine's refs + tags + the
/// current reference root, dropping superseded engine-state blobs and commits on deleted branches, then
/// compact. Returns the compaction stats (bytes before/after). Crash-safe via [`FileStore::compact`]'s
/// atomic rename. Call after churn (many commits / `save_loom`s) to reclaim accumulated garbage.
#[cfg(not(target_arch = "wasm32"))]
pub fn gc_loom(loom: &mut Loom<FileStore>) -> Result<CompactStats> {
    let reference = loom.store().reference_root();
    let live = loom.live_object_set(reference)?;
    let retain: BTreeSet<[u8; 32]> = live.iter().map(|d| *d.bytes()).collect();
    loom.store_mut().compact_retaining(&retain)
}

pub fn gc_loom_validated_segments(
    loom: &mut Loom<FileStore>,
    budget: GcSegmentBudget,
) -> Result<GcStats> {
    let reference = loom.store().reference_root();
    let live = loom.live_object_set(reference)?;
    let retain = live
        .into_iter()
        .map(|digest| *digest.bytes())
        .collect::<BTreeSet<_>>();
    loom.store_mut()
        .gc_validated_segments_retaining(budget, &retain)
}

// ---- superblock (struct + impl) lives in superblock.rs ----
mod superblock;
pub(crate) use superblock::*;

// ---- helpers -----------------------------------------------------------------------------------

#[cfg(test)]
fn legacy_free_map_promotion_destination(path: &Path) -> bool {
    let Some(destination) = std::env::var_os("LOOM_PROMOTION_DEST") else {
        return false;
    };
    !path.as_os_str().is_empty() && path == Path::new(&destination)
}

/// Take an exclusive advisory lock on `file`, or report the loom as busy if another handle holds it.
#[cfg(not(target_arch = "wasm32"))]
fn acquire_write_lock(file: &File) -> Result<()> {
    match file.try_lock() {
        Ok(()) => Ok(()),
        Err(std::fs::TryLockError::WouldBlock) => Err(LoomError::new(
            Code::Conflict,
            "loom-store: loom is open for writing by another process",
        )),
        Err(std::fs::TryLockError::Error(e))
            if cfg!(target_os = "android") && e.kind() == std::io::ErrorKind::Unsupported =>
        {
            Ok(())
        }
        Err(std::fs::TryLockError::Error(e)) => Err(io_err(e)),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn reclamation_lease_path(path: &Path) -> Result<PathBuf> {
    Ok(daemon::paths(path)?.lock_file.with_extension("reclamation"))
}

#[cfg(not(target_arch = "wasm32"))]
fn open_reclamation_lease(path: &Path) -> Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(reclamation_lease_path(path)?)
        .map_err(io_err)
}

#[cfg(not(target_arch = "wasm32"))]
fn acquire_reclamation_reader_lease(path: &Path) -> Result<File> {
    let file = open_reclamation_lease(path)?;
    file.lock_shared().map_err(io_err)?;
    Ok(file)
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
pub(crate) struct ReclamationWriteLease {
    _file: Option<File>,
    allowed: bool,
}

#[cfg(target_arch = "wasm32")]
#[derive(Debug)]
pub(crate) struct ReclamationWriteLease {
    allowed: bool,
}

impl FileStore {
    fn try_reclamation_write_lease(&self) -> Result<ReclamationWriteLease> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.path.as_os_str().is_empty() {
                return Ok(ReclamationWriteLease {
                    _file: None,
                    allowed: true,
                });
            }
            let file = open_reclamation_lease(&self.path)?;
            return match file.try_lock() {
                Ok(()) => Ok(ReclamationWriteLease {
                    _file: Some(file),
                    allowed: true,
                }),
                Err(std::fs::TryLockError::WouldBlock) => Ok(ReclamationWriteLease {
                    _file: None,
                    allowed: false,
                }),
                Err(std::fs::TryLockError::Error(error)) => Err(io_err(error)),
            };
        }
        #[cfg(target_arch = "wasm32")]
        {
            Ok(ReclamationWriteLease { allowed: true })
        }
    }

    pub(crate) fn transaction_reusable_free(
        &self,
        free: &[FreePageRun],
        active_mark_epoch_reclaim_fence: Option<u64>,
        minimum_recoverable_generation: u64,
    ) -> Result<(Vec<FreePageRun>, ReclamationWriteLease)> {
        let lease = self.try_reclamation_write_lease()?;
        let reusable = if lease.allowed {
            foreground_recovery_safe_reusable_free(
                free,
                active_mark_epoch_reclaim_fence,
                minimum_recoverable_generation,
            )
        } else {
            Vec::new()
        };
        Ok((reusable, lease))
    }

    fn begin_foreground_transaction_publication(
        &self,
        inner: &Inner,
        control_map: BTreeMap<Vec<u8>, Vec<u8>>,
    ) -> Result<ForegroundTransactionPublicationAuthority> {
        let reclamation_lease = self.try_reclamation_write_lease()?;
        let ordinary_reusable_runs = if reclamation_lease.allowed {
            foreground_recovery_safe_reusable_free(
                &inner.free,
                inner.active_mark_epoch_reclaim_fence,
                inner.minimum_recoverable_generation,
            )
        } else {
            Vec::new()
        };
        let active_epoch = if reclamation_lease.allowed {
            mark_epoch::active_mark_epoch_from_control_map(&control_map, self.digest_algo)?
        } else {
            None
        };
        let captured_free_selection = if let Some(epoch) = &active_epoch {
            if inner.active_mark_epoch_reclaim_fence != Some(epoch.page_high_water_mark) {
                return Err(corrupt("reachability mark foreground fence mismatch"));
            }
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            mark_epoch::captured_free_reuse_runs(
                &mut **file,
                self.digest_algo,
                epoch,
                &inner.free,
                inner.minimum_recoverable_generation,
                usize::MAX,
            )?
        } else {
            mark_epoch::CapturedFreeReuseSelection::default()
        };
        Ok(ForegroundTransactionPublicationAuthority {
            ordinary_reusable_runs,
            publication_eligible_runs: captured_free_selection.runs,
            captured_free_authority: captured_free_selection.allocation_authority,
            control_map,
            active_epoch,
            _reclamation_lease: reclamation_lease,
        })
    }
}

struct ForegroundTransactionPublicationAuthority {
    ordinary_reusable_runs: Vec<FreePageRun>,
    publication_eligible_runs: Vec<FreePageRun>,
    captured_free_authority: pagemap::CapturedFreeAllocationAuthority,
    control_map: BTreeMap<Vec<u8>, Vec<u8>>,
    active_epoch: Option<mark_epoch::ReachabilityMarkEpoch>,
    _reclamation_lease: ReclamationWriteLease,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ForegroundMutationInput {
    WorkflowOwnerState,
    MutableOverlayRecords,
    AuditRetentionMap,
    AuditRetentionDelta,
    ObjectBatch,
    DeltaPackConsolidation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ForegroundPublicationSourceIdentity {
    generation: u64,
    page_count: u64,
    object_index_root: Option<PageId>,
    legacy_overlay_root: Option<PageId>,
    current_record_root: Option<PageId>,
    root_catalog_root: Option<PageId>,
    root_catalog_entries: Vec<RootCatalogEntry>,
    mutable_overlay_generation_floor: u64,
    minimum_recoverable_generation: u64,
    free_map_root: Option<(PageId, u64)>,
    region_table_root: Option<PageId>,
    maintenance_root: Option<PageId>,
    reference_root: Option<[u8; 32]>,
    control_root: Option<[u8; 32]>,
    free: Vec<FreePageRun>,
    maintenance: MaintenanceState,
    active_mark_epoch_reclaim_fence: Option<u64>,
    open_segment: u64,
    metadata_bootstrap_reserve: MetadataBootstrapReserve,
    encryption_meta: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ForegroundAllocationSchedule {
    source_page_count: u64,
    resulting_page_count: u64,
    free_map_metadata_pages: u64,
    captured_free_consumed_through: Option<u64>,
    resulting_bootstrap_reserve: MetadataBootstrapReserve,
}

struct PreparedForegroundTransactionOutcome<T> {
    publication: record_io::PreparedForegroundTxnResult,
    value: T,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PreparedForegroundFamilyRootMutation {
    family_id: u16,
    source: Option<PageId>,
    result: Option<PageId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PreparedForegroundRootMutations {
    object_index: (Option<PageId>, Option<PageId>),
    legacy_overlay: (Option<PageId>, Option<PageId>),
    current_records: (Option<PageId>, Option<PageId>),
    catalog_families: Vec<PreparedForegroundFamilyRootMutation>,
    control: (Option<[u8; 32]>, Option<[u8; 32]>),
    root_catalog: (Option<PageId>, Option<PageId>),
    free_map: (Option<(PageId, u64)>, Option<(PageId, u64)>),
}

struct PreparedForegroundTransactionPublication<T> {
    input: ForegroundMutationInput,
    source_identity: ForegroundPublicationSourceIdentity,
    encoded_frames_and_tree_writes: PreparedBackingTransaction,
    prepared_root_mutations: PreparedForegroundRootMutations,
    allocation_schedule: ForegroundAllocationSchedule,
    resulting_roots: TxnRoots,
    value: T,
    #[cfg(any(test, feature = "test-hooks"))]
    allocator_stats: pagemap::PageAllocatorTransactionStats,
}

struct PreparedForegroundTransactionFinalization {
    publication_reserve_pages: u64,
    selected_publication_runs: Vec<FreePageRun>,
    control: Option<[u8; 32]>,
    control_frame: Option<record_io::PreparedRecordFrame>,
    index_source_page_count: u64,
    index_delta: pagebtree::PreparedPageTreeDelta,
    free_map_publication: pagemap::PreparedFreeMapPublication,
}

struct AppliedForegroundTransactionFinalization {
    index_root: Option<PageId>,
    control: Option<[u8; 32]>,
    fresh_control_placement: Option<([u8; 32], RecordLoc)>,
    free_map_publication: pagemap::PreparedFreeMapPublication,
}

impl FileStore {
    fn foreground_publication_source_identity(
        inner: &Inner,
    ) -> ForegroundPublicationSourceIdentity {
        ForegroundPublicationSourceIdentity {
            generation: inner.generation,
            page_count: inner.page_count,
            object_index_root: inner.index_root,
            legacy_overlay_root: inner.overlay_root,
            current_record_root: inner.current_record_root,
            root_catalog_root: inner.root_catalog_root,
            root_catalog_entries: inner.root_catalog_entries.clone(),
            mutable_overlay_generation_floor: inner.mutable_overlay_generation_floor,
            minimum_recoverable_generation: inner.minimum_recoverable_generation,
            free_map_root: inner.freemap,
            region_table_root: inner.region_table_root,
            maintenance_root: inner.maintenance_root,
            reference_root: inner.reference_root.map(|root| *root.bytes()),
            control_root: inner.control_root.map(|root| *root.bytes()),
            free: inner.free.clone(),
            maintenance: inner.maintenance.clone(),
            active_mark_epoch_reclaim_fence: inner.active_mark_epoch_reclaim_fence,
            open_segment: inner.open_segment,
            metadata_bootstrap_reserve: inner.metadata_bootstrap_reserve.clone(),
            encryption_meta: inner.encryption_meta.clone(),
        }
    }

    fn prepare_foreground_transaction_publication<T>(
        &self,
        file: &mut dyn BackingIo,
        inner: &Inner,
        input: ForegroundMutationInput,
        authority: &ForegroundTransactionPublicationAuthority,
        prepare: impl FnOnce(
            &mut PlanningBacking<'_>,
            &mut PageAllocator,
        ) -> Result<PreparedForegroundTransactionOutcome<T>>,
    ) -> Result<PreparedForegroundTransactionPublication<T>> {
        let source_identity = Self::foreground_publication_source_identity(inner);
        let new_generation = inner.generation.saturating_add(1);
        let mut allocator = PageAllocator::new_with_reusable_authorities(
            inner.page_count,
            new_generation,
            inner.free.clone(),
            authority.ordinary_reusable_runs.clone(),
            authority.publication_eligible_runs.clone(),
        );
        allocator.install_captured_free_authority(authority.captured_free_authority.clone())?;
        allocator.install_metadata_bootstrap_reserve(&inner.metadata_bootstrap_reserve)?;
        let mut planning_backing = PlanningBacking::new(file).map_err(io_err)?;
        let outcome = prepare(&mut planning_backing, &mut allocator)?;
        let prepared_backing = planning_backing.finish();
        let (roots, free_map_publication_demand) = outcome.publication.into_parts();
        let free_map_metadata_pages = free_map_publication_demand.allocation_pages();
        let resulting_reserve_pages = roots.metadata_bootstrap_reserve.page_count();
        if free_map_metadata_pages > pagemap::FOREGROUND_TRANSACTION_METADATA_LIMIT_PAGES
            || resulting_reserve_pages > pagemap::FOREGROUND_TRANSACTION_METADATA_LIMIT_PAGES
        {
            return Err(LoomError::new(
                Code::ResourceExhausted,
                format!(
                    "loom-store: foreground transaction metadata exceeds {} bytes",
                    pagemap::FOREGROUND_TRANSACTION_METADATA_LIMIT_BYTES
                ),
            ));
        }
        if roots.generation != new_generation {
            return Err(corrupt(
                "prepared foreground publication returned the wrong generation",
            ));
        }
        let allocation_schedule = ForegroundAllocationSchedule {
            source_page_count: inner.page_count,
            resulting_page_count: roots.page_count,
            free_map_metadata_pages,
            captured_free_consumed_through: allocator.captured_free_consumed_through(),
            resulting_bootstrap_reserve: roots.metadata_bootstrap_reserve.clone(),
        };
        let source_catalog = source_identity
            .root_catalog_entries
            .iter()
            .map(|entry| (entry.family_id, entry.root))
            .collect::<BTreeMap<_, _>>();
        let result_catalog = roots
            .root_catalog
            .entries
            .iter()
            .map(|entry| (entry.family_id, entry.root))
            .collect::<BTreeMap<_, _>>();
        let family_ids = source_catalog
            .keys()
            .chain(result_catalog.keys())
            .copied()
            .collect::<BTreeSet<_>>();
        let prepared_root_mutations = PreparedForegroundRootMutations {
            object_index: (source_identity.object_index_root, roots.object_index),
            legacy_overlay: (source_identity.legacy_overlay_root, roots.legacy_overlay),
            current_records: (
                source_identity.current_record_root,
                roots.current_record_root,
            ),
            catalog_families: family_ids
                .into_iter()
                .map(|family_id| PreparedForegroundFamilyRootMutation {
                    family_id,
                    source: source_catalog.get(&family_id).copied(),
                    result: result_catalog.get(&family_id).copied(),
                })
                .collect(),
            control: (source_identity.control_root, roots.control),
            root_catalog: (source_identity.root_catalog_root, roots.root_catalog.root),
            free_map: (source_identity.free_map_root, roots.freemap),
        };
        Ok(PreparedForegroundTransactionPublication {
            input,
            source_identity,
            encoded_frames_and_tree_writes: prepared_backing,
            prepared_root_mutations,
            allocation_schedule,
            resulting_roots: roots,
            value: outcome.value,
            #[cfg(any(test, feature = "test-hooks"))]
            allocator_stats: allocator.transaction_stats(),
        })
    }

    fn finish_foreground_txn<T>(
        &self,
        file: &mut dyn BackingIo,
        inner: &Inner,
        prepared: PreparedForegroundTransactionPublication<T>,
    ) -> Result<(TxnRoots, T)> {
        if Self::foreground_publication_source_identity(inner) != prepared.source_identity {
            return Err(LoomError::new(
                Code::Conflict,
                "loom-store: foreground publication source changed after preparation",
            ));
        }
        let family_roots_match = prepared
            .prepared_root_mutations
            .catalog_families
            .iter()
            .all(|mutation| {
                prepared
                    .source_identity
                    .root_catalog_entries
                    .iter()
                    .find(|entry| entry.family_id == mutation.family_id)
                    .map(|entry| entry.root)
                    == mutation.source
                    && prepared
                        .resulting_roots
                        .root_catalog
                        .entries
                        .iter()
                        .find(|entry| entry.family_id == mutation.family_id)
                        .map(|entry| entry.root)
                        == mutation.result
            });
        if prepared.encoded_frames_and_tree_writes.source_len() != file.size().map_err(io_err)?
            || prepared.allocation_schedule.source_page_count != inner.page_count
            || prepared.allocation_schedule.resulting_page_count
                != prepared.resulting_roots.page_count
            || prepared.allocation_schedule.free_map_metadata_pages
                > pagemap::FOREGROUND_TRANSACTION_METADATA_LIMIT_PAGES
            || prepared.allocation_schedule.resulting_bootstrap_reserve
                != prepared.resulting_roots.metadata_bootstrap_reserve
            || prepared.prepared_root_mutations.object_index
                != (
                    prepared.source_identity.object_index_root,
                    prepared.resulting_roots.object_index,
                )
            || prepared.prepared_root_mutations.legacy_overlay
                != (
                    prepared.source_identity.legacy_overlay_root,
                    prepared.resulting_roots.legacy_overlay,
                )
            || prepared.prepared_root_mutations.current_records
                != (
                    prepared.source_identity.current_record_root,
                    prepared.resulting_roots.current_record_root,
                )
            || prepared.prepared_root_mutations.control
                != (
                    prepared.source_identity.control_root,
                    prepared.resulting_roots.control,
                )
            || prepared.prepared_root_mutations.root_catalog
                != (
                    prepared.source_identity.root_catalog_root,
                    prepared.resulting_roots.root_catalog.root,
                )
            || prepared.prepared_root_mutations.free_map
                != (
                    prepared.source_identity.free_map_root,
                    prepared.resulting_roots.freemap,
                )
            || !family_roots_match
        {
            return Err(corrupt(
                "prepared foreground publication is internally inconsistent",
            ));
        }
        let _input = prepared.input;
        let _captured_free_consumed_through =
            prepared.allocation_schedule.captured_free_consumed_through;
        let _prepared_write_count = prepared.encoded_frames_and_tree_writes.write_count();
        let _prepared_final_len = prepared.encoded_frames_and_tree_writes.final_len();
        prepared
            .encoded_frames_and_tree_writes
            .apply(file, |elapsed| {
                self.group_commit_metrics.record_fsync(elapsed)
            })
            .map_err(io_err)?;
        #[cfg(any(test, feature = "test-hooks"))]
        {
            complete_btree_batch_transaction_for_test();
            complete_foreground_allocator_transaction_for_test(prepared.allocator_stats);
        }
        Ok((prepared.resulting_roots, prepared.value))
    }

    fn prepare_foreground_transaction_finalization(
        &self,
        file: &mut dyn BackingIo,
        inner: &Inner,
        allocator: &PageAllocator,
        authority: &ForegroundTransactionPublicationAuthority,
        index_root: Option<PageId>,
    ) -> Result<PreparedForegroundTransactionFinalization> {
        const FIXED_METADATA_PAGES: u64 = 2;
        let control_page_bound = if authority.active_epoch.is_some() {
            let mut maximum_control_map = authority.control_map.clone();
            let epoch = authority.active_epoch.as_ref().unwrap();
            let maximum_cursor = allocator
                .captured_free_page_count()
                .unwrap_or(epoch.captured_free_consumed_through);
            if maximum_cursor > epoch.captured_free_consumed_through {
                mark_epoch::advance_captured_free_consumption_in_control_map(
                    &mut maximum_control_map,
                    epoch,
                    maximum_cursor,
                    self.digest_algo,
                )?;
            }
            let bytes = encode_control_map(&maximum_control_map);
            let digest = Digest::hash(self.digest_algo, &bytes);
            let frame = record_io::prepare_record_frame(
                digest,
                &bytes,
                self.default_codec,
                self.dek.lock().map_err(|_| poisoned())?.as_ref(),
            )?;
            prepared_record_page_allocations(&frame.frame)
        } else {
            0
        };
        let index_depth = match index_root {
            Some(root) => pagebtree::tree_depth(file, DATA_START, root, allocator.page_count())?,
            None => 1,
        };
        let index_page_bound = index_depth.saturating_mul(2).saturating_add(2);
        let publication_reserve_pages = control_page_bound
            .saturating_add(index_page_bound)
            .saturating_add(FIXED_METADATA_PAGES);

        let mut simulated = allocator.clone();
        let selected_publication_runs =
            simulated.select_captured_publication_reserve(publication_reserve_pages)?;
        let combined_cursor = simulated
            .captured_free_consumed_through()
            .unwrap_or_default();
        let mut control_map = authority.control_map.clone();
        if let Some(epoch) = &authority.active_epoch
            && combined_cursor > epoch.captured_free_consumed_through
        {
            mark_epoch::advance_captured_free_consumption_in_control_map(
                &mut control_map,
                epoch,
                combined_cursor,
                self.digest_algo,
            )?;
        }
        let control_bytes = (!control_map.is_empty()).then(|| encode_control_map(&control_map));
        let control = control_bytes
            .as_ref()
            .map(|bytes| *Digest::hash(self.digest_algo, bytes).bytes());
        let control_is_fresh = match control {
            Some(digest) => pagebtree::get(
                file,
                DATA_START,
                index_root,
                &digest,
                allocator.page_count(),
            )?
            .is_none(),
            None => false,
        };
        let control_frame = match (control, control_bytes.as_deref(), control_is_fresh) {
            (Some(digest), Some(bytes), true) => Some(record_io::prepare_record_frame(
                Digest::of(self.digest_algo, digest),
                bytes,
                self.default_codec,
                self.dek.lock().map_err(|_| poisoned())?.as_ref(),
            )?),
            _ => None,
        };
        let control_record_pages = control_frame
            .as_ref()
            .map_or(0, |record| prepared_record_page_allocations(&record.frame));
        let placeholder = control.filter(|_| control_is_fresh).map(|digest| {
            (
                digest,
                RecordLoc {
                    segment_id: u64::MAX,
                    page_index: 0,
                    slot: u32::MAX,
                },
            )
        });
        let index_source_page_count = allocator.page_count();
        let index_delta = pagebtree::prepare_delete_upsert_delta(
            file,
            DATA_START,
            index_root,
            index_source_page_count,
            pagebtree::ValueCodecKind::RecordLoc,
            &[],
            placeholder.as_slice(),
        )?;
        let actual_pre_map_pages = control_record_pages
            .saturating_add(index_delta.allocation_calls())
            .saturating_add(FIXED_METADATA_PAGES);
        if actual_pre_map_pages > publication_reserve_pages {
            return Err(LoomError::new(
                Code::ResourceExhausted,
                "loom-store: foreground publication exceeds its structural reservation",
            ));
        }
        simulated.activate_publication_reserve();
        simulate_one_page_allocations(&mut simulated, control_record_pages);
        simulate_one_page_allocations(&mut simulated, index_delta.allocation_calls());
        for page in index_delta.affected_pages() {
            simulated.free(*page, 1)?;
        }
        let source_updates = simulated.pending_free_map_extent_updates();
        simulate_one_page_allocations(&mut simulated, FIXED_METADATA_PAGES);
        simulated.ensure_metadata_bootstrap_capacity()?;
        #[cfg(any(test, feature = "test-hooks"))]
        let rejected_dirty_range_count = simulated.pending_free_map_extent_update_count() as u64;
        let updates = simulated.take_free_map_extent_updates();
        let free_map_publication = pagemap::prepare_tree_map_publication(
            file,
            DATA_START,
            inner.freemap.map(|(root, _)| root),
            &allocator.initial_free_runs(),
            source_updates,
            updates,
            simulated.page_count(),
        )?;
        let actual_free_map_pages = free_map_publication.demand().allocation_pages();
        if actual_free_map_pages > pagemap::FOREGROUND_TRANSACTION_METADATA_LIMIT_PAGES {
            #[cfg(any(test, feature = "test-hooks"))]
            {
                let demand = free_map_publication.demand();
                let free_map_depth = inner
                    .freemap
                    .and_then(|(root, _)| {
                        pagebtree::free_page_extent_tree_depth(
                            file,
                            DATA_START,
                            root,
                            inner.page_count,
                        )
                        .ok()
                    })
                    .unwrap_or_default();
                observe_rejected_free_map_publication(RejectedFreeMapPublicationDiagnostic {
                    demanded_pages: actual_free_map_pages,
                    reserve_capacity_pages: pagemap::FOREGROUND_TRANSACTION_METADATA_LIMIT_PAGES,
                    reserve_available_pages: simulated
                        .metadata_bootstrap_descriptor(inner.generation)
                        .page_count(),
                    extent_deletes: demand.extent_deletes,
                    extent_upserts: demand.extent_upserts,
                    btree_node_pages: demand.btree_node_pages,
                    affected_existing_btree_pages: demand.affected_existing_btree_pages,
                    split_decisions: demand.split_decisions,
                    dirty_range_count: rejected_dirty_range_count,
                    free_map_depth,
                });
            }
            return Err(LoomError::new(
                Code::ResourceExhausted,
                "loom-store: free-map publication exceeds metadata bootstrap capacity",
            ));
        }
        Ok(PreparedForegroundTransactionFinalization {
            publication_reserve_pages,
            selected_publication_runs,
            control,
            control_frame,
            index_source_page_count,
            index_delta,
            free_map_publication,
        })
    }

    fn apply_foreground_transaction_finalization(
        &self,
        file: &mut dyn BackingIo,
        allocator: &mut PageAllocator,
        index_root: Option<PageId>,
        prepared: PreparedForegroundTransactionFinalization,
    ) -> Result<AppliedForegroundTransactionFinalization> {
        let selected =
            allocator.select_captured_publication_reserve(prepared.publication_reserve_pages)?;
        if selected != prepared.selected_publication_runs {
            return Err(corrupt(
                "foreground publication selection changed after planning",
            ));
        }
        allocator.activate_publication_reserve();
        let placements = match &prepared.control_frame {
            Some(frame) => {
                let reference = frame.as_ref();
                record_io::write_prepared_record_pages(file, allocator, &[reference])?
            }
            None => Vec::new(),
        };
        let mut index_delta = prepared.index_delta;
        index_delta.rebind_upsert_values(&placements)?;
        let applied_index = pagebtree::apply_prepared_delta(
            file,
            DATA_START,
            allocator,
            index_root,
            prepared.index_source_page_count,
            pagebtree::ValueCodecKind::RecordLoc,
            index_delta,
        )?;
        Ok(AppliedForegroundTransactionFinalization {
            index_root: applied_index.root,
            control: prepared.control,
            fresh_control_placement: placements.into_iter().next(),
            free_map_publication: prepared.free_map_publication,
        })
    }
}

fn prepared_record_page_allocations(frame: &[u8]) -> u64 {
    if record::is_large(frame.len() as u64) {
        frame
            .len()
            .max(1)
            .div_ceil(record::chunked_blob_payload_capacity()) as u64
    } else {
        1
    }
}

fn simulate_one_page_allocations(allocator: &mut PageAllocator, count: u64) {
    for _ in 0..count {
        allocator.alloc(1);
    }
}

#[derive(Clone)]
struct AllocatorVisibleReusableRuns {
    ordinary: Vec<FreePageRun>,
    publication: Vec<FreePageRun>,
}

fn reclaim_fence_filter_reusable_free(
    free: &[FreePageRun],
    high_water_mark: Option<u64>,
) -> Vec<FreePageRun> {
    let Some(high_water_mark) = high_water_mark else {
        return free.to_vec();
    };
    free.iter()
        .filter_map(|run| {
            let end = run.start.saturating_add(run.len);
            if end <= high_water_mark {
                None
            } else if run.start < high_water_mark {
                Some(FreePageRun {
                    start: high_water_mark,
                    len: end.saturating_sub(high_water_mark),
                    freed_gen: run.freed_gen,
                })
            } else {
                Some(*run)
            }
        })
        .collect()
}

fn foreground_recovery_safe_reusable_free(
    free: &[FreePageRun],
    high_water_mark: Option<u64>,
    minimum_recoverable_generation: u64,
) -> Vec<FreePageRun> {
    reclaim_fence_filter_reusable_free(free, high_water_mark)
        .into_iter()
        .filter(|run| run.freed_gen <= minimum_recoverable_generation)
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
fn reject_daemon_owned_direct_open(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let Ok(paths) = daemon::paths(path) else {
        return Ok(());
    };
    if daemon::status_response(&paths).is_ok() {
        return Err(LoomError::new(
            Code::Conflict,
            "loom-store: CLI daemon is running for this store; direct writable opens are disabled",
        ));
    }
    Ok(())
}

pub(crate) fn corrupt(msg: &str) -> LoomError {
    LoomError::corrupt(format!("loom-store: {msg}"))
}
pub(crate) fn io_err(e: std::io::Error) -> LoomError {
    LoomError::new(Code::Io, format!("loom-store io: {e}"))
}
fn poisoned() -> LoomError {
    LoomError::new(Code::Internal, "loom-store: file lock poisoned")
}

/// A sibling temp path for compaction (same directory, so `rename` is an atomic same-filesystem move).
#[cfg(not(target_arch = "wasm32"))]
fn compact_tmp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".compact-{}.tmp", std::process::id()));
    path.with_file_name(name)
}

/// Best-effort fsync of the file's parent directory so a `rename` is durable across a crash. A no-op
/// where directories cannot be opened as files (e.g. Windows); compaction correctness does not depend
/// on it (a lost rename simply leaves the prior committed file).
#[cfg(not(target_arch = "wasm32"))]
fn sync_parent_dir(path: &Path) {
    if let Some(parent) = path.parent()
        && let Ok(dir) = File::open(parent)
    {
        let _ = dir.sync_all();
    }
}

// ---- BackingIo block-device abstraction lives in backing.rs ----
mod backing;
pub use backing::*;
/// Read the canonical region table selected by recovery and validate it against that root set.
fn read_canonical_region_table(
    file: &mut dyn BackingIo,
    rt: PageId,
    page_count: u64,
    recovered_generation: u64,
) -> Result<RegionTable> {
    if rt.0 >= page_count {
        return Err(corrupt("region table page out of range"));
    }
    let mut buf = [0u8; PAGE_SIZE as usize];
    read_exact_at(file, rt.offset(DATA_START), &mut buf).map_err(io_err)?;
    let canonical = CanonicalRegionTable::decode(&buf)
        .map_err(|_| corrupt("canonical region table parse failure"))?;
    canonical
        .validate_recovered_generation(page_count, recovered_generation)
        .map_err(|error| match error {
            page::RootCodecError::MetadataBootstrapGenerationMismatch { .. } => {
                corrupt("metadata bootstrap reserve owning generation mismatch")
            }
            _ => corrupt("canonical region table validation failure"),
        })?;
    let region = RegionTable::from_canonical(canonical);
    if region.metadata_bootstrap_reserve.capacity != pagemap::METADATA_BOOTSTRAP_CAPACITY_PAGES {
        return Err(corrupt(
            "metadata bootstrap reserve requires controlled offline migration",
        ));
    }
    Ok(region)
}

#[cfg(test)]
fn read_region_table(file: &mut dyn BackingIo, rt: PageId, page_count: u64) -> Result<RegionTable> {
    if rt.0 >= page_count {
        return Err(corrupt("region table page out of range"));
    }
    let mut buf = [0u8; PAGE_SIZE as usize];
    read_exact_at(file, rt.offset(DATA_START), &mut buf).map_err(io_err)?;
    if let Some(region) = RegionTable::decode(&buf) {
        return Ok(region);
    }
    page::decode_lrt4_for_promotion(&buf, page_count)
        .map(RegionTable::from_canonical)
        .map_err(|_| corrupt("region table parse failure"))
}

fn read_root_catalog(
    file: &mut dyn BackingIo,
    root: PageId,
    page_count: u64,
) -> Result<RootCatalog> {
    if root.0 >= page_count {
        return Err(corrupt("root catalog page out of range"));
    }
    let mut buf = [0u8; PAGE_SIZE as usize];
    read_exact_at(file, root.offset(DATA_START), &mut buf).map_err(io_err)?;
    let catalog = RootCatalog::decode(&buf).map_err(|_| corrupt("root catalog parse failure"))?;
    catalog
        .validate_root_bounds(page_count)
        .map_err(|_| corrupt("root catalog family root out of range"))?;
    Ok(catalog)
}

pub(crate) fn put_uvarint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            out.push(byte | 0x80);
        } else {
            out.push(byte);
            break;
        }
    }
}

/// Read a LEB128 `uvarint` from `buf` at `*pos`, advancing `*pos` past it. `None` on truncation or an
/// overlong (> 64-bit) encoding.
pub(crate) fn get_uvarint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    let mut shift = 0u32;
    loop {
        let byte = *buf.get(*pos)?;
        *pos += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
}

fn mutable_overlay_meta_address() -> [u8; 32] {
    *Digest::blake3(MUTABLE_OVERLAY_META_ADDRESS).bytes()
}

fn mutable_overlay_current_root_address() -> [u8; 32] {
    *Digest::blake3(MUTABLE_OVERLAY_CURRENT_ROOT_ADDRESS).bytes()
}

fn mutable_overlay_entry_address(key: &loom_core::OverlayKey) -> [u8; 32] {
    let mut out = MUTABLE_OVERLAY_ENTRY_ADDRESS_PREFIX.to_vec();
    out.extend_from_slice(Digest::blake3(key.as_bytes()).bytes());
    *Digest::blake3(&out).bytes()
}

fn mutable_overlay_owner_token_address(key: &loom_core::OverlayKey) -> [u8; 32] {
    let mut out = MUTABLE_OVERLAY_OWNER_TOKEN_ADDRESS_PREFIX.to_vec();
    out.extend_from_slice(Digest::blake3(key.as_bytes()).bytes());
    *Digest::blake3(&out).bytes()
}

fn mutable_overlay_secondary_index_address(index: &loom_core::OverlayKey) -> [u8; 32] {
    let mut out = MUTABLE_OVERLAY_SECONDARY_INDEX_ADDRESS_PREFIX.to_vec();
    out.extend_from_slice(Digest::blake3(index.as_bytes()).bytes());
    *Digest::blake3(&out).bytes()
}

fn mutable_overlay_idempotency_address(idempotency_key: &str) -> [u8; 32] {
    let mut out = MUTABLE_OVERLAY_IDEMPOTENCY_ADDRESS_PREFIX.to_vec();
    out.extend_from_slice(idempotency_key.as_bytes());
    *Digest::blake3(&out).bytes()
}

fn mutable_overlay_transaction_idempotency_address(idempotency_key: &[u8]) -> [u8; 32] {
    let mut out = MUTABLE_OVERLAY_TRANSACTION_IDEMPOTENCY_ADDRESS_PREFIX.to_vec();
    out.extend_from_slice(idempotency_key);
    *Digest::blake3(&out).bytes()
}

fn audit_retention_record_address(key: &[u8]) -> [u8; 32] {
    let mut out = AUDIT_RETENTION_RECORD_ADDRESS_PREFIX.to_vec();
    out.extend_from_slice(Digest::blake3(key).bytes());
    *Digest::blake3(&out).bytes()
}

#[cfg(test)]
fn mvcc_generation_record_address(generation: loom_core::OverlayGeneration) -> [u8; 32] {
    let mut out = MVCC_GENERATION_RECORD_ADDRESS_PREFIX.to_vec();
    out.extend_from_slice(&generation.as_u64().to_be_bytes());
    *Digest::blake3(&out).bytes()
}

#[cfg(test)]
fn retention_index_record_address(target: &loom_core::OverlayKey) -> [u8; 32] {
    let mut out = RETENTION_INDEX_RECORD_ADDRESS_PREFIX.to_vec();
    out.extend_from_slice(Digest::blake3(target.as_bytes()).bytes());
    *Digest::blake3(&out).bytes()
}

#[cfg(test)]
fn checkpoint_index_record_address(checkpoint_id: &[u8]) -> [u8; 32] {
    let mut out = CHECKPOINT_INDEX_RECORD_ADDRESS_PREFIX.to_vec();
    out.extend_from_slice(Digest::blake3(checkpoint_id).bytes());
    *Digest::blake3(&out).bytes()
}

#[cfg(test)]
fn reclaim_index_record_address(reclaim_key: &[u8]) -> [u8; 32] {
    let mut out = RECLAIM_INDEX_RECORD_ADDRESS_PREFIX.to_vec();
    out.extend_from_slice(Digest::blake3(reclaim_key).bytes());
    *Digest::blake3(&out).bytes()
}

#[cfg(test)]
fn delta_pack_advisory_record_address(advisory_key: &[u8]) -> [u8; 32] {
    let mut out = DELTA_PACK_ADVISORY_RECORD_ADDRESS_PREFIX.to_vec();
    out.extend_from_slice(Digest::blake3(advisory_key).bytes());
    *Digest::blake3(&out).bytes()
}

fn retained_history_head_address(key: &[u8]) -> [u8; 32] {
    let mut out = RETAINED_HISTORY_HEAD_ADDRESS_PREFIX.to_vec();
    out.extend_from_slice(Digest::blake3(key).bytes());
    *Digest::blake3(&out).bytes()
}

fn retained_history_record_address(key: &[u8], sequence: u64) -> [u8; 32] {
    let mut out = RETAINED_HISTORY_RECORD_ADDRESS_PREFIX.to_vec();
    out.extend_from_slice(Digest::blake3(key).bytes());
    out.extend_from_slice(&sequence.to_be_bytes());
    *Digest::blake3(&out).bytes()
}

fn encode_retained_history_head(key: &[u8], sequence: u64) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(RETAINED_HISTORY_HEAD_RECORD);
    put_uvarint(&mut out, key.len() as u64);
    out.extend_from_slice(key);
    put_uvarint(&mut out, sequence);
    out
}

fn decode_retained_history_head(bytes: &[u8]) -> Result<(Vec<u8>, u64)> {
    if !bytes.starts_with(RETAINED_HISTORY_HEAD_RECORD) {
        return Err(corrupt("retained-history head schema mismatch"));
    }
    let mut pos = RETAINED_HISTORY_HEAD_RECORD.len();
    let key_len = get_uvarint(bytes, &mut pos)
        .ok_or_else(|| corrupt("retained-history head key length truncated"))?
        as usize;
    let key_end = pos
        .checked_add(key_len)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| corrupt("retained-history head key truncated"))?;
    let key = bytes[pos..key_end].to_vec();
    pos = key_end;
    let sequence = get_uvarint(bytes, &mut pos)
        .ok_or_else(|| corrupt("retained-history head sequence truncated"))?;
    if pos != bytes.len() {
        return Err(corrupt("retained-history head trailing bytes"));
    }
    Ok((key, sequence))
}

fn encode_retained_history_entry(key: &[u8], sequence: u64, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(RETAINED_HISTORY_ENTRY_RECORD);
    put_uvarint(&mut out, key.len() as u64);
    out.extend_from_slice(key);
    put_uvarint(&mut out, sequence);
    put_uvarint(&mut out, payload.len() as u64);
    out.extend_from_slice(payload);
    out
}

fn decode_retained_history_entry(bytes: &[u8]) -> Result<(Vec<u8>, u64, Vec<u8>)> {
    if !bytes.starts_with(RETAINED_HISTORY_ENTRY_RECORD) {
        return Err(corrupt("retained-history entry schema mismatch"));
    }
    let mut pos = RETAINED_HISTORY_ENTRY_RECORD.len();
    let key_len = get_uvarint(bytes, &mut pos)
        .ok_or_else(|| corrupt("retained-history entry key length truncated"))?
        as usize;
    let key_end = pos
        .checked_add(key_len)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| corrupt("retained-history entry key truncated"))?;
    let key = bytes[pos..key_end].to_vec();
    pos = key_end;
    let sequence = get_uvarint(bytes, &mut pos)
        .ok_or_else(|| corrupt("retained-history entry sequence truncated"))?;
    let payload_len = get_uvarint(bytes, &mut pos)
        .ok_or_else(|| corrupt("retained-history entry payload length truncated"))?
        as usize;
    let payload_end = pos
        .checked_add(payload_len)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| corrupt("retained-history entry payload truncated"))?;
    let payload = bytes[pos..payload_end].to_vec();
    if payload_end != bytes.len() {
        return Err(corrupt("retained-history entry trailing bytes"));
    }
    Ok((key, sequence, payload))
}

fn validate_mutable_overlay_idempotency_key(idempotency_key: &str) -> Result<()> {
    if idempotency_key.is_empty() {
        return Err(LoomError::invalid(
            "mutable overlay idempotency key must not be empty",
        ));
    }
    if idempotency_key.len() > 512 {
        return Err(LoomError::invalid(
            "mutable overlay idempotency key is too long",
        ));
    }
    if idempotency_key.as_bytes().contains(&0) {
        return Err(LoomError::invalid(
            "mutable overlay idempotency key contains NUL",
        ));
    }
    Ok(())
}

fn mutable_overlay_idempotency_request_digest(
    key: &loom_core::OverlayKey,
    payload: &[u8],
) -> Digest {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"loom.store.mutable-overlay.idempotency-request.v1");
    put_uvarint(&mut bytes, key.as_bytes().len() as u64);
    bytes.extend_from_slice(key.as_bytes());
    put_uvarint(&mut bytes, payload.len() as u64);
    bytes.extend_from_slice(payload);
    Digest::blake3(&bytes)
}

fn workflow_transaction_request_digest(
    txn: &WorkflowTransaction,
    write_durabilities: &[StoreDurabilityPolicy],
) -> Digest {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"loom.store.workflow-transaction.request.v2");
    bytes.extend_from_slice(txn.workspace.as_bytes());
    bytes.extend_from_slice(txn.actor.as_bytes());
    put_uvarint(&mut bytes, txn.writes.len() as u64);
    for (write, durability) in txn.writes.iter().zip(write_durabilities) {
        bytes.push(write.facet.stable_tag());
        bytes.push(workflow_durability_tag(*durability));
        put_uvarint(&mut bytes, write.target.as_bytes().len() as u64);
        bytes.extend_from_slice(write.target.as_bytes());
        match &write.op {
            loom_core::FacetWriteOp::Put { payload } => {
                bytes.push(1);
                put_uvarint(&mut bytes, payload.len() as u64);
                bytes.extend_from_slice(payload);
            }
            loom_core::FacetWriteOp::Delete => bytes.push(2),
        }
        match write.expected.as_ref() {
            Some(token) => {
                bytes.push(1);
                bytes.extend_from_slice(token.0.as_bytes());
            }
            None => bytes.push(0),
        }
        put_uvarint(&mut bytes, write.secondary_indexes.len() as u64);
        for index_write in &write.secondary_indexes {
            put_uvarint(&mut bytes, index_write.index.as_bytes().len() as u64);
            bytes.extend_from_slice(index_write.index.as_bytes());
            match &index_write.op {
                loom_core::SecondaryIndexWriteOp::Put { payload } => {
                    bytes.push(1);
                    put_uvarint(&mut bytes, payload.len() as u64);
                    bytes.extend_from_slice(payload);
                }
                loom_core::SecondaryIndexWriteOp::Delete => bytes.push(2),
            }
        }
        match write.audit.as_ref() {
            Some(audit) => {
                bytes.push(1);
                put_uvarint(&mut bytes, audit.operation.len() as u64);
                bytes.extend_from_slice(audit.operation.as_bytes());
            }
            None => bytes.push(0),
        }
        put_uvarint(&mut bytes, write.side_effects.intents.len() as u64);
        for intent in &write.side_effects.intents {
            match intent {
                loom_core::FacetSideEffect::OperationLog { operation_id } => {
                    bytes.push(1);
                    put_uvarint(&mut bytes, operation_id.len() as u64);
                    bytes.extend_from_slice(operation_id.as_bytes());
                }
                loom_core::FacetSideEffect::AuditRecord { operation } => {
                    bytes.push(2);
                    put_uvarint(&mut bytes, operation.len() as u64);
                    bytes.extend_from_slice(operation.as_bytes());
                }
                loom_core::FacetSideEffect::RevisionIndex { entity_id } => {
                    bytes.push(3);
                    put_uvarint(&mut bytes, entity_id.len() as u64);
                    bytes.extend_from_slice(entity_id.as_bytes());
                }
                loom_core::FacetSideEffect::ReferenceIndex { source_id } => {
                    bytes.push(4);
                    put_uvarint(&mut bytes, source_id.len() as u64);
                    bytes.extend_from_slice(source_id.as_bytes());
                }
            }
        }
    }
    put_uvarint(&mut bytes, txn.owner_state.objects.len() as u64);
    for (digest, payload) in &txn.owner_state.objects {
        bytes.extend_from_slice(digest.bytes());
        put_uvarint(&mut bytes, payload.len() as u64);
        bytes.extend_from_slice(payload);
    }
    match txn.owner_state.reference {
        loom_core::WorkflowReferenceUpdate::Keep => bytes.push(0),
        loom_core::WorkflowReferenceUpdate::Set(None) => bytes.push(1),
        loom_core::WorkflowReferenceUpdate::Set(Some(root)) => {
            bytes.push(2);
            bytes.extend_from_slice(root.bytes());
        }
    }
    put_uvarint(&mut bytes, txn.owner_state.controls.len() as u64);
    for write in &txn.owner_state.controls {
        match write {
            loom_core::WorkflowControlWrite::Put { key, payload } => {
                bytes.push(1);
                put_uvarint(&mut bytes, key.len() as u64);
                bytes.extend_from_slice(key);
                put_uvarint(&mut bytes, payload.len() as u64);
                bytes.extend_from_slice(payload);
            }
            loom_core::WorkflowControlWrite::Delete { key } => {
                bytes.push(2);
                put_uvarint(&mut bytes, key.len() as u64);
                bytes.extend_from_slice(key);
            }
            loom_core::WorkflowControlWrite::AppendRetained {
                key,
                expected_next_sequence,
                records,
            } => {
                bytes.push(3);
                put_uvarint(&mut bytes, key.len() as u64);
                bytes.extend_from_slice(key);
                put_uvarint(&mut bytes, *expected_next_sequence);
                put_uvarint(&mut bytes, records.len() as u64);
                for record in records {
                    put_uvarint(&mut bytes, record.len() as u64);
                    bytes.extend_from_slice(record);
                }
            }
        }
    }
    put_uvarint(&mut bytes, txn.owner_state.audits.len() as u64);
    for audit in &txn.owner_state.audits {
        match audit.principal {
            Some(principal) => {
                bytes.push(1);
                bytes.extend_from_slice(principal.as_bytes());
            }
            None => bytes.push(0),
        }
        put_uvarint(&mut bytes, audit.action.len() as u64);
        bytes.extend_from_slice(audit.action.as_bytes());
        match &audit.target {
            Some(target) => {
                bytes.push(1);
                put_uvarint(&mut bytes, target.len() as u64);
                bytes.extend_from_slice(target.as_bytes());
            }
            None => bytes.push(0),
        }
    }
    put_uvarint(&mut bytes, txn.prepared_operations.len() as u64);
    for operation in &txn.prepared_operations {
        put_uvarint(&mut bytes, operation.operation_id.len() as u64);
        bytes.extend_from_slice(operation.operation_id.as_bytes());
        put_uvarint(&mut bytes, operation.payload.len() as u64);
        bytes.extend_from_slice(&operation.payload);
    }
    put_uvarint(&mut bytes, txn.revision_metadata.len() as u64);
    for revision in &txn.revision_metadata {
        put_uvarint(&mut bytes, revision.entity_id.len() as u64);
        bytes.extend_from_slice(revision.entity_id.as_bytes());
        put_uvarint(&mut bytes, revision.revision_id.len() as u64);
        bytes.extend_from_slice(revision.revision_id.as_bytes());
        put_uvarint(&mut bytes, revision.payload.len() as u64);
        bytes.extend_from_slice(&revision.payload);
    }
    put_uvarint(&mut bytes, txn.delivery_intents.len() as u64);
    for delivery in &txn.delivery_intents {
        put_uvarint(&mut bytes, delivery.stream_id.len() as u64);
        bytes.extend_from_slice(delivery.stream_id.as_bytes());
        put_uvarint(&mut bytes, delivery.sequence);
        put_uvarint(&mut bytes, delivery.envelope_id.len() as u64);
        bytes.extend_from_slice(delivery.envelope_id.as_bytes());
        bytes.extend_from_slice(delivery.payload_digest.bytes());
    }
    match txn.post_commit_delta.as_ref() {
        Some(delta) => {
            bytes.push(1);
            bytes.extend_from_slice(delta.workspace().as_bytes());
            let changed_paths = delta.changed_paths();
            put_uvarint(&mut bytes, changed_paths.len() as u64);
            for path in changed_paths {
                put_uvarint(&mut bytes, path.len() as u64);
                bytes.extend_from_slice(path.as_bytes());
            }
            put_uvarint(&mut bytes, delta.changed_content_count() as u64);
        }
        None => bytes.push(0),
    }
    Digest::blake3(&bytes)
}

fn workflow_write_durability(
    txn: &WorkflowTransaction,
    policy: &StorePolicy,
    write: &loom_core::FacetWrite,
) -> StoreDurabilityPolicy {
    loom_core::strictest_durability([
        txn.durability,
        write
            .durability
            .unwrap_or_else(|| policy.effective_durability(write.facet)),
    ])
}

fn workflow_durability_tag(durability: StoreDurabilityPolicy) -> u8 {
    match durability {
        StoreDurabilityPolicy::Strict => 1,
        StoreDurabilityPolicy::Normal => 2,
        StoreDurabilityPolicy::Relaxed => 3,
        StoreDurabilityPolicy::Ephemeral => 4,
    }
}

fn workflow_transaction_root_digest(
    algo: Algo,
    generation: loom_core::OverlayGeneration,
) -> Digest {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"loom.store.workflow-transaction.root.v1");
    put_uvarint(&mut bytes, generation.as_u64());
    Digest::hash(algo, &bytes)
}

fn mutable_overlay_checkpoint_record_blockers(
    generation: loom_core::OverlayGeneration,
    kind: loom_core::OverlayEntryKind,
    mvcc: &StoreMvccSnapshotDiagnostics,
    audit_retention_active: bool,
    durable_reclaim_floor: u64,
) -> Vec<MutableOverlayReclaimBlocker> {
    let generation = generation.as_u64();
    let mut blockers = Vec::new();
    if mvcc
        .pins
        .iter()
        .any(|pin| pin.identity.overlay_generation.as_u64() >= generation)
    {
        blockers.push(MutableOverlayReclaimBlocker::PinnedSnapshot);
        blockers.push(MutableOverlayReclaimBlocker::RetainedHistory);
        blockers.push(MutableOverlayReclaimBlocker::StrictPromotionBoundary);
    }
    if audit_retention_active {
        blockers.push(MutableOverlayReclaimBlocker::AuditRetention);
    }
    if kind == loom_core::OverlayEntryKind::Tombstone {
        blockers.push(MutableOverlayReclaimBlocker::TombstoneRetention);
    }
    if durable_reclaim_floor < generation {
        blockers.push(MutableOverlayReclaimBlocker::DurableGenerationWindow);
    }
    blockers.sort();
    blockers.dedup();
    blockers
}

fn overlay_current_record_locs(
    file: &mut dyn BackingIo,
    root: Option<PageId>,
    page_count: u64,
    codec: pagebtree::ValueCodecKind,
    addresses: impl Iterator<Item = [u8; 32]>,
) -> Result<Vec<([u8; 32], RecordLoc)>> {
    let mut current = Vec::new();
    for address in addresses {
        if let Some(loc) =
            pagebtree::get_with_codec(file, DATA_START, root, &address, page_count, codec)?
        {
            current.push((address, loc));
        }
    }
    Ok(current)
}

fn write_overlay_blob_pages(
    file: &mut dyn BackingIo,
    alloc: &mut PageAllocator,
    current: &[([u8; 32], RecordLoc)],
    records: &[([u8; 32], &[u8])],
) -> Result<Vec<([u8; 32], RecordLoc)>> {
    let replaced = current
        .iter()
        .map(|(address, _)| *address)
        .collect::<BTreeSet<_>>();
    let mut new_records = Vec::new();
    let mut replacement_records = Vec::new();
    for record in records {
        if replaced.contains(&record.0) {
            replacement_records.push(*record);
        } else {
            new_records.push(*record);
        }
    }
    let mut placements = record_io::write_blob_pages(file, alloc, &new_records)?;
    placements.extend(record_io::write_dedicated_blob_pages(
        file,
        alloc,
        &replacement_records,
    )?);
    Ok(placements)
}

fn reclaim_superseded_overlay_blobs_from_current<'a>(
    file: &mut dyn BackingIo,
    alloc: &mut PageAllocator,
    current: &[([u8; 32], RecordLoc)],
    replacements: impl Iterator<Item = ([u8; 32], &'a [u8])>,
    oldest_pinned_snapshot_generation: Option<u64>,
    audit_retention_active: bool,
) -> Result<BTreeSet<u64>> {
    let eligible_locs = superseded_overlay_blob_locs_from_current(
        file,
        current,
        replacements,
        oldest_pinned_snapshot_generation,
        audit_retention_active,
    )?;
    let mut touched_segments = BTreeSet::new();
    free_overlay_record_pages_batch(file, alloc, &eligible_locs, &mut touched_segments)?;
    Ok(touched_segments)
}

fn superseded_overlay_blob_locs_from_current<'a>(
    file: &mut dyn BackingIo,
    current: &[([u8; 32], RecordLoc)],
    replacements: impl Iterator<Item = ([u8; 32], &'a [u8])>,
    oldest_pinned_snapshot_generation: Option<u64>,
    audit_retention_active: bool,
) -> Result<Vec<RecordLoc>> {
    let replacements = replacements.collect::<BTreeMap<_, _>>();
    if replacements.is_empty() {
        return Ok(Vec::new());
    }
    let mut eligible_locs = Vec::new();
    for (address, loc) in current {
        let Some(replacement) = replacements.get(address) else {
            continue;
        };
        let prior = read_blob_from_loc(file, *loc)?;
        if let (Ok((prior_key, prior_sequence)), Ok((replacement_key, replacement_sequence))) = (
            decode_retained_history_head(&prior),
            decode_retained_history_head(replacement),
        ) {
            if prior_key != replacement_key || replacement_sequence <= prior_sequence {
                return Err(corrupt(
                    "retained-history head replacement is not monotonic",
                ));
            }
            eligible_locs.push(*loc);
            continue;
        }
        if decode_mutable_overlay_owner_token_record(&prior).is_ok()
            && decode_mutable_overlay_owner_token_record(replacement).is_ok()
        {
            eligible_locs.push(*loc);
            continue;
        }
        if decode_mutable_overlay_current_root_record(&prior).is_ok()
            && decode_mutable_overlay_current_root_record(replacement).is_ok()
        {
            eligible_locs.push(*loc);
            continue;
        }
        if let (Ok(prior), Ok(replacement)) = (
            decode_mutable_overlay_secondary_index_record(&prior),
            decode_mutable_overlay_secondary_index_record(replacement),
        ) {
            if superseded_overlay_record_is_eligible(
                prior.generation,
                replacement.generation,
                replacement.kind == loom_core::OverlayEntryKind::Tombstone,
                oldest_pinned_snapshot_generation,
                audit_retention_active,
            )? {
                eligible_locs.push(*loc);
            }
            continue;
        }
        let Ok(prior) = decode_mutable_overlay_entry(&prior) else {
            continue;
        };
        let replacement = decode_mutable_overlay_entry(replacement)?;
        if prior.generation.as_u64() == 0 || replacement.generation <= prior.generation {
            continue;
        }
        // A tombstone must be kept only while it is the current entry required to hide a value still
        // reachable from the immutable base through composite reads. A value superseded by a tombstone
        // (a delete) keeps that requirement, so its superseded page stays retained. A tombstone
        // superseded by a value (a reopen) no longer hides anything because the newer value shadows
        // the base, so the superseded tombstone page is governed only by the pinned-snapshot,
        // retained-history, durable-window, and strict-promotion horizons below.
        let tombstone_masks_base = replacement.kind == loom_core::OverlayEntryKind::Tombstone;
        if !superseded_overlay_record_is_eligible(
            prior.generation,
            replacement.generation,
            tombstone_masks_base,
            oldest_pinned_snapshot_generation,
            audit_retention_active,
        )? {
            continue;
        }
        eligible_locs.push(*loc);
    }
    Ok(eligible_locs)
}

fn superseded_overlay_record_is_eligible(
    prior_generation: loom_core::OverlayGeneration,
    replacement_generation: loom_core::OverlayGeneration,
    tombstone_masks_base: bool,
    oldest_pinned_snapshot_generation: Option<u64>,
    audit_retention_active: bool,
) -> Result<bool> {
    if prior_generation.as_u64() == 0 || replacement_generation <= prior_generation {
        return Ok(false);
    }
    MutableOverlayReclaimState {
        superseded_generation: prior_generation.as_u64(),
        superseding_generation: replacement_generation.as_u64(),
        latest_index_generation: replacement_generation.as_u64(),
        oldest_pinned_snapshot_generation,
        retained_history_generation: oldest_pinned_snapshot_generation,
        audit_retention_active,
        tombstone_masks_base,
        durable_reclaim_floor: replacement_generation.as_u64(),
        strict_promotion_generation: oldest_pinned_snapshot_generation,
    }
    .is_eligible()
}

fn free_overlay_record_pages(
    file: &mut dyn BackingIo,
    alloc: &mut PageAllocator,
    loc: RecordLoc,
    touched_segments: &mut BTreeSet<u64>,
) -> Result<()> {
    let start = loc.global_page();
    let mut first = [0u8; PAGE_SIZE as usize];
    read_exact_at(file, PageId(start).offset(DATA_START), &mut first).map_err(io_err)?;
    if first[0] == record::SLAB_MAGIC {
        if record::read_slab_slot(&first, 0).is_some()
            && record::read_slab_slot(&first, 1).is_none()
        {
            alloc.free(PageId(start), 1)?;
            touched_segments.insert(start / page::PAGES_PER_SEGMENT);
        }
        return Ok(());
    }
    if first[0] == record::CHUNKED_BLOB_MAGIC {
        for page in record_io::chunked_blob_pages(file, start, alloc.page_count())? {
            alloc.free(PageId(page), 1)?;
            touched_segments.insert(page / page::PAGES_PER_SEGMENT);
        }
        return Ok(());
    }
    let span = record_io::page_span(file, start)?;
    alloc.free(PageId(start), span)?;
    for page in start..start.saturating_add(span) {
        touched_segments.insert(page / page::PAGES_PER_SEGMENT);
    }
    Ok(())
}

fn free_overlay_record_pages_batch(
    file: &mut dyn BackingIo,
    alloc: &mut PageAllocator,
    locs: &[RecordLoc],
    touched_segments: &mut BTreeSet<u64>,
) -> Result<()> {
    let mut by_page = BTreeMap::<u64, BTreeSet<u32>>::new();
    for loc in locs {
        by_page
            .entry(loc.global_page())
            .or_default()
            .insert(loc.slot);
    }
    for (page, slots) in by_page {
        let mut first = [0u8; PAGE_SIZE as usize];
        read_exact_at(file, PageId(page).offset(DATA_START), &mut first).map_err(io_err)?;
        if first[0] == record::SLAB_MAGIC {
            let slot_count = u16::from_le_bytes([first[1], first[2]]) as usize;
            let all_slots_are_eligible = slots.len() == slot_count
                && (0..slot_count).all(|slot| slots.contains(&(slot as u32)))
                && (0..slot_count)
                    .all(|slot| record::read_slab_slot(&first, slot as u32).is_some());
            if all_slots_are_eligible {
                alloc.free(PageId(page), 1)?;
                touched_segments.insert(page / page::PAGES_PER_SEGMENT);
            }
            continue;
        }
        free_overlay_record_pages(
            file,
            alloc,
            RecordLoc::from_global(page, 0),
            touched_segments,
        )?;
    }
    Ok(())
}

fn read_blob_from_loc(file: &mut dyn BackingIo, loc: RecordLoc) -> Result<Vec<u8>> {
    let global = loc.global_page();
    let mut first = [0u8; PAGE_SIZE as usize];
    read_exact_at(file, PageId(global).offset(DATA_START), &mut first).map_err(io_err)?;
    match first[0] {
        record::SLAB_MAGIC => record::read_slab_slot(&first, loc.slot)
            .map(|bytes| bytes.to_vec())
            .ok_or_else(|| corrupt("bad slab blob slot on read")),
        record::LARGE_MAGIC => {
            let blob_len =
                record::large_blob_len(&first).ok_or_else(|| corrupt("bad large blob header"))?;
            let pages = record::large_pages(blob_len);
            let mut buf = vec![0u8; (pages * PAGE_SIZE) as usize];
            read_exact_at(file, PageId(global).offset(DATA_START), &mut buf).map_err(io_err)?;
            record::decode_large(&buf)
                .map(|bytes| bytes.to_vec())
                .ok_or_else(|| corrupt("large blob parse failure"))
        }
        record::CHUNKED_BLOB_MAGIC => record_io::read_chunked_blob(file, global, u64::MAX),
        _ => Err(corrupt("bad blob page magic on read")),
    }
}

fn read_payload_from_loc(
    file: &mut dyn BackingIo,
    digest: Digest,
    loc: RecordLoc,
    page_count: u64,
    dek: Option<&DekSession>,
) -> Result<Vec<u8>> {
    let global = loc.global_page();
    if global >= page_count {
        return Err(corrupt("record locator past the page array"));
    }
    let mut first = [0u8; PAGE_SIZE as usize];
    read_exact_at(file, PageId(global).offset(DATA_START), &mut first).map_err(io_err)?;
    match first[0] {
        record::SLAB_MAGIC => {
            let rec = record::read_slab_slot(&first, loc.slot)
                .ok_or_else(|| corrupt("bad slab slot on read"))?;
            decode_record(rec, &digest, dek, digest.algo())
        }
        record::LARGE_MAGIC => {
            let blob_len =
                record::large_blob_len(&first).ok_or_else(|| corrupt("bad large record header"))?;
            let pages = record::large_pages(blob_len);
            if global + pages > page_count {
                return Err(corrupt("large record run past the page array"));
            }
            let mut buf = vec![0u8; (pages * PAGE_SIZE) as usize];
            read_exact_at(file, PageId(global).offset(DATA_START), &mut buf).map_err(io_err)?;
            let rec =
                record::decode_large(&buf).ok_or_else(|| corrupt("large record parse failure"))?;
            decode_record(rec, &digest, dek, digest.algo())
        }
        record::CHUNKED_BLOB_MAGIC => {
            let rec = record_io::read_chunked_blob(file, global, page_count)?;
            decode_record(&rec, &digest, dek, digest.algo())
        }
        _ => Err(corrupt("bad record page magic on read")),
    }
}

fn decode_mutable_overlay_meta(bytes: &[u8]) -> Result<u64> {
    const HEADER: &[u8] = b"loom.store.mutable-overlay.meta.v1";
    if !bytes.starts_with(HEADER) {
        return Err(corrupt("mutable overlay meta schema mismatch"));
    }
    let mut pos = HEADER.len();
    let generation = get_uvarint(bytes, &mut pos)
        .ok_or_else(|| corrupt("mutable overlay generation truncated"))?;
    if pos != bytes.len() {
        return Err(corrupt("mutable overlay meta trailing bytes"));
    }
    Ok(generation)
}

#[cfg(test)]
fn encode_mutable_overlay_current_root_record(current_root: Option<PageId>) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MUTABLE_OVERLAY_CURRENT_ROOT_RECORD);
    match current_root {
        Some(root) => {
            out.push(1);
            out.extend_from_slice(&root.0.to_be_bytes());
        }
        None => out.push(0),
    }
    out
}

fn decode_mutable_overlay_current_root_record(bytes: &[u8]) -> Result<Option<PageId>> {
    if !bytes.starts_with(MUTABLE_OVERLAY_CURRENT_ROOT_RECORD) {
        return Err(corrupt("mutable overlay current-root schema mismatch"));
    }
    let mut pos = MUTABLE_OVERLAY_CURRENT_ROOT_RECORD.len();
    let tag = bytes
        .get(pos)
        .copied()
        .ok_or_else(|| corrupt("mutable overlay current-root tag missing"))?;
    pos += 1;
    let root = match tag {
        0 => None,
        1 => {
            let end = pos
                .checked_add(8)
                .filter(|end| *end <= bytes.len())
                .ok_or_else(|| corrupt("mutable overlay current-root page truncated"))?;
            let page = u64::from_be_bytes(
                bytes[pos..end]
                    .try_into()
                    .map_err(|_| corrupt("mutable overlay current-root page invalid"))?,
            );
            pos = end;
            Some(PageId(page))
        }
        _ => return Err(corrupt("mutable overlay current-root tag invalid")),
    };
    if pos != bytes.len() {
        return Err(corrupt("mutable overlay current-root trailing bytes"));
    }
    Ok(root)
}

fn read_mutable_overlay_current_root(
    file: &mut dyn BackingIo,
    overlay_root: Option<PageId>,
    page_count: u64,
) -> Result<Option<PageId>> {
    let Some(loc) = pagebtree::get(
        file,
        DATA_START,
        overlay_root,
        &mutable_overlay_current_root_address(),
        page_count,
    )?
    else {
        return Ok(None);
    };
    decode_mutable_overlay_current_root_record(&read_blob_from_loc(file, loc)?)
}

fn read_mutable_overlay_current_record_root(
    file: &mut dyn BackingIo,
    current_record_root: Option<PageId>,
    overlay_root: Option<PageId>,
    page_count: u64,
) -> Result<Option<PageId>> {
    match current_record_root {
        Some(root) => Ok(Some(root)),
        None => read_mutable_overlay_current_root(file, overlay_root, page_count),
    }
}

fn source_layout_classify_record(
    address: [u8; 32],
    bytes: &[u8],
    ownership: SourceLayoutOwnership,
) -> SourceLayoutDiscoveryEntry {
    if address == mutable_overlay_current_root_address() {
        return match decode_mutable_overlay_current_root_record(bytes) {
            Ok(root) => source_layout_decoded_entry(
                source_layout_address(address),
                SourceLayoutFamily::CurrentRootPointer,
                root.map(|page| format!("page:{}", page.0)),
                None,
                None,
                bytes,
                ownership,
            ),
            Err(err) => source_layout_malformed_entry(
                source_layout_address(address),
                SourceLayoutFamily::CurrentRootPointer,
                ownership,
                Some(bytes),
                err.to_string(),
            ),
        };
    }
    if is_mutable_overlay_current_entry_record(bytes) {
        return match decode_mutable_overlay_entry(bytes) {
            Ok(entry) => {
                let expected = mutable_overlay_entry_address(&entry.key);
                let rejection = (expected != address).then(|| {
                    format!(
                        "mutable overlay entry address mismatch: expected {}",
                        source_layout_address(expected)
                    )
                });
                SourceLayoutDiscoveryEntry {
                    source_address: source_layout_address(address),
                    family: SourceLayoutFamily::CurrentEntry,
                    key_or_identity: Some(source_layout_bytes_identity(entry.key.as_bytes())),
                    generation: Some(entry.generation.as_u64()),
                    sequence: None,
                    payload_digest: Some(Digest::blake3(bytes).to_hex()),
                    payload_len: Some(bytes.len()),
                    ownership,
                    decode_state: if rejection.is_some() {
                        SourceLayoutDecodeState::Malformed
                    } else {
                        SourceLayoutDecodeState::Decoded
                    },
                    rejection_reason: rejection,
                }
            }
            Err(err) => source_layout_malformed_entry(
                source_layout_address(address),
                SourceLayoutFamily::CurrentEntry,
                ownership,
                Some(bytes),
                err.to_string(),
            ),
        };
    }
    if bytes.starts_with(RETAINED_HISTORY_HEAD_RECORD) {
        return match decode_retained_history_head(bytes) {
            Ok((key, sequence)) => source_layout_decoded_verified_entry(
                address,
                retained_history_head_address(&key),
                SourceLayoutFamily::RetainedHistoryHead,
                Some(source_layout_bytes_identity(&key)),
                None,
                Some(sequence),
                bytes,
                ownership,
                "retained-history head address mismatch",
            ),
            Err(err) => source_layout_malformed_entry(
                source_layout_address(address),
                SourceLayoutFamily::RetainedHistoryHead,
                ownership,
                Some(bytes),
                err.to_string(),
            ),
        };
    }
    if bytes.starts_with(RETAINED_HISTORY_ENTRY_RECORD) {
        return match decode_retained_history_entry(bytes) {
            Ok((key, sequence, _payload)) => source_layout_decoded_verified_entry(
                address,
                retained_history_record_address(&key, sequence),
                SourceLayoutFamily::RetainedHistoryRecord,
                Some(source_layout_bytes_identity(&key)),
                None,
                Some(sequence),
                bytes,
                ownership,
                "retained-history record address mismatch",
            ),
            Err(err) => source_layout_malformed_entry(
                source_layout_address(address),
                SourceLayoutFamily::RetainedHistoryRecord,
                ownership,
                Some(bytes),
                err.to_string(),
            ),
        };
    }
    if bytes.starts_with(MUTABLE_OVERLAY_OWNER_TOKEN_RECORD) {
        return match decode_mutable_overlay_owner_token_record(bytes) {
            Ok(token) => source_layout_decoded_entry(
                source_layout_address(address),
                SourceLayoutFamily::OwnerToken,
                Some(source_layout_address(*token.as_bytes())),
                None,
                None,
                bytes,
                ownership,
            ),
            Err(err) => source_layout_malformed_entry(
                source_layout_address(address),
                SourceLayoutFamily::OwnerToken,
                ownership,
                Some(bytes),
                err.to_string(),
            ),
        };
    }
    if bytes.starts_with(MUTABLE_OVERLAY_SECONDARY_INDEX_RECORD) {
        return match decode_mutable_overlay_secondary_index_record(bytes) {
            Ok(record) => source_layout_decoded_verified_entry(
                address,
                mutable_overlay_secondary_index_address(&record.index),
                SourceLayoutFamily::SecondaryIndex,
                Some(source_layout_bytes_identity(record.index.as_bytes())),
                Some(record.generation.as_u64()),
                None,
                bytes,
                ownership,
                "mutable overlay secondary-index address mismatch",
            ),
            Err(err) => source_layout_malformed_entry(
                source_layout_address(address),
                SourceLayoutFamily::SecondaryIndex,
                ownership,
                Some(bytes),
                err.to_string(),
            ),
        };
    }
    if bytes.starts_with(MUTABLE_OVERLAY_IDEMPOTENCY_RECORD) {
        return match decode_mutable_overlay_idempotency_record(bytes) {
            Ok(record) => source_layout_decoded_entry(
                source_layout_address(address),
                SourceLayoutFamily::MutableIdempotency,
                Some(record.request_digest.to_hex()),
                None,
                None,
                bytes,
                ownership,
            ),
            Err(err) => source_layout_malformed_entry(
                source_layout_address(address),
                SourceLayoutFamily::MutableIdempotency,
                ownership,
                Some(bytes),
                err.to_string(),
            ),
        };
    }
    if bytes.starts_with(MUTABLE_OVERLAY_TRANSACTION_IDEMPOTENCY_RECORD) {
        return match decode_workflow_transaction_idempotency_record(bytes) {
            Ok(record) => source_layout_decoded_entry(
                source_layout_address(address),
                SourceLayoutFamily::WorkflowIdempotency,
                Some(record.request_digest.to_hex()),
                Some(record.receipt.generation.as_u64()),
                None,
                bytes,
                ownership,
            ),
            Err(err) => source_layout_malformed_entry(
                source_layout_address(address),
                SourceLayoutFamily::WorkflowIdempotency,
                ownership,
                Some(bytes),
                err.to_string(),
            ),
        };
    }
    source_layout_malformed_entry(
        source_layout_address(address),
        SourceLayoutFamily::Unknown,
        ownership,
        Some(bytes),
        "unknown source-layout family".to_string(),
    )
}

fn source_layout_control_entry(key: Vec<u8>, value: Vec<u8>) -> SourceLayoutDiscoveryEntry {
    let family = if is_audit_retention_control_key(&key) {
        SourceLayoutFamily::AuditControl
    } else {
        SourceLayoutFamily::Control
    };
    SourceLayoutDiscoveryEntry {
        source_address: format!("control:{}", source_layout_bytes_identity(&key)),
        family,
        key_or_identity: Some(source_layout_bytes_identity(&key)),
        generation: None,
        sequence: source_layout_audit_sequence(&key),
        payload_digest: Some(Digest::blake3(&value).to_hex()),
        payload_len: Some(value.len()),
        ownership: SourceLayoutOwnership::ControlRootObject,
        decode_state: SourceLayoutDecodeState::Decoded,
        rejection_reason: None,
    }
}

fn source_layout_decoded_verified_entry(
    address: [u8; 32],
    expected: [u8; 32],
    family: SourceLayoutFamily,
    key_or_identity: Option<String>,
    generation: Option<u64>,
    sequence: Option<u64>,
    bytes: &[u8],
    ownership: SourceLayoutOwnership,
    mismatch: &str,
) -> SourceLayoutDiscoveryEntry {
    let mut entry = source_layout_decoded_entry(
        source_layout_address(address),
        family,
        key_or_identity,
        generation,
        sequence,
        bytes,
        ownership,
    );
    if address != expected {
        entry.decode_state = SourceLayoutDecodeState::Malformed;
        entry.rejection_reason = Some(format!(
            "{mismatch}: expected {}",
            source_layout_address(expected)
        ));
    }
    entry
}

fn source_layout_decoded_entry(
    source_address: String,
    family: SourceLayoutFamily,
    key_or_identity: Option<String>,
    generation: Option<u64>,
    sequence: Option<u64>,
    bytes: &[u8],
    ownership: SourceLayoutOwnership,
) -> SourceLayoutDiscoveryEntry {
    SourceLayoutDiscoveryEntry {
        source_address,
        family,
        key_or_identity,
        generation,
        sequence,
        payload_digest: Some(Digest::blake3(bytes).to_hex()),
        payload_len: Some(bytes.len()),
        ownership,
        decode_state: SourceLayoutDecodeState::Decoded,
        rejection_reason: None,
    }
}

fn source_layout_malformed_entry(
    source_address: String,
    family: SourceLayoutFamily,
    ownership: SourceLayoutOwnership,
    bytes: Option<&[u8]>,
    reason: String,
) -> SourceLayoutDiscoveryEntry {
    SourceLayoutDiscoveryEntry {
        source_address,
        family,
        key_or_identity: None,
        generation: None,
        sequence: None,
        payload_digest: bytes.map(|bytes| Digest::blake3(bytes).to_hex()),
        payload_len: bytes.map(|bytes| bytes.len()),
        ownership,
        decode_state: if family == SourceLayoutFamily::Unknown {
            SourceLayoutDecodeState::UnknownFamily
        } else {
            SourceLayoutDecodeState::Malformed
        },
        rejection_reason: Some(reason),
    }
}

fn source_layout_append_absent_families(entries: &mut Vec<SourceLayoutDiscoveryEntry>) {
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
        if !entries.iter().any(|entry| entry.family == family) {
            entries.push(SourceLayoutDiscoveryEntry {
                source_address: format!("absent:{family:?}"),
                family,
                key_or_identity: None,
                generation: None,
                sequence: None,
                payload_digest: None,
                payload_len: None,
                ownership: SourceLayoutOwnership::OptionalFamilyAbsent,
                decode_state: SourceLayoutDecodeState::Absent,
                rejection_reason: None,
            });
        }
    }
}

fn source_layout_append_conflicts(entries: &mut Vec<SourceLayoutDiscoveryEntry>) {
    let mut seen = BTreeMap::<(SourceLayoutFamily, String), (usize, BTreeSet<String>)>::new();
    for entry in entries.iter() {
        if entry.decode_state == SourceLayoutDecodeState::Decoded
            && let Some(identity) = source_layout_conflict_identity(entry)
        {
            let (count, digests) = seen.entry((entry.family, identity)).or_default();
            *count += 1;
            digests.insert(entry.payload_digest.clone().unwrap_or_default());
        }
    }
    for ((family, identity), (count, digests)) in seen {
        if count > 1 {
            entries.push(SourceLayoutDiscoveryEntry {
                source_address: format!("conflict:{family:?}:{identity}"),
                family,
                key_or_identity: Some(identity),
                generation: None,
                sequence: None,
                payload_digest: None,
                payload_len: None,
                ownership: SourceLayoutOwnership::LegacyOverlay,
                decode_state: SourceLayoutDecodeState::Conflict,
                rejection_reason: Some(if digests.len() > 1 {
                    "duplicate conflicting source-layout records".to_string()
                } else {
                    "duplicate equivalent source-layout records".to_string()
                }),
            });
        }
    }
}

fn source_layout_classified_owner_counts(
    entries: &[SourceLayoutDiscoveryEntry],
) -> Vec<SourceLayoutClassifiedOwnerCount> {
    let mut counts = BTreeMap::<
        (
            SourceLayoutFamily,
            SourceLayoutOwnership,
            SourceLayoutDecodeState,
        ),
        usize,
    >::new();
    for entry in entries {
        *counts
            .entry((entry.family, entry.ownership, entry.decode_state))
            .or_default() += 1;
    }
    counts
        .into_iter()
        .map(
            |((family, ownership, decode_state), count)| SourceLayoutClassifiedOwnerCount {
                family,
                ownership,
                decode_state,
                count,
            },
        )
        .collect()
}

fn source_layout_reject_discovery_identity_mismatch(
    report: &SourceLayoutDiscoveryReport,
    identity: &SourceLayoutSourceIdentity,
) -> Result<()> {
    if report.generation != identity.generation
        || report.page_count != identity.page_count
        || report.overlay_root != identity.overlay_root
        || report.current_record_root != identity.current_record_root
        || report.root_catalog_root != identity.root_catalog_root
        || report.control_root != identity.control_root
    {
        return Err(LoomError::new(
            Code::Conflict,
            "source-layout discovery report does not match captured source identity",
        ));
    }
    Ok(())
}

fn source_layout_discovery_has_legacy_overlay_records(
    entries: &[SourceLayoutDiscoveryEntry],
) -> bool {
    entries.iter().any(|entry| {
        entry.ownership == SourceLayoutOwnership::LegacyOverlay
            && entry.decode_state == SourceLayoutDecodeState::Decoded
    })
}

fn source_layout_discovery_has_audit_control_records(
    entries: &[SourceLayoutDiscoveryEntry],
) -> bool {
    entries.iter().any(|entry| {
        entry.family == SourceLayoutFamily::AuditControl
            && entry.ownership == SourceLayoutOwnership::ControlRootObject
            && entry.decode_state == SourceLayoutDecodeState::Decoded
    })
}

fn source_layout_conflict_identity(entry: &SourceLayoutDiscoveryEntry) -> Option<String> {
    match entry.family {
        SourceLayoutFamily::RetainedHistoryRecord => Some(format!(
            "{}:{}",
            entry.key_or_identity.as_ref()?,
            entry.sequence?
        )),
        SourceLayoutFamily::CurrentRootPointer => Some("current-root-pointer".to_string()),
        SourceLayoutFamily::OwnerToken => Some(entry.source_address.clone()),
        SourceLayoutFamily::CurrentEntry
        | SourceLayoutFamily::RetainedHistoryHead
        | SourceLayoutFamily::SecondaryIndex
        | SourceLayoutFamily::MutableIdempotency
        | SourceLayoutFamily::WorkflowIdempotency
        | SourceLayoutFamily::AuditControl
        | SourceLayoutFamily::Control => entry.key_or_identity.clone(),
        SourceLayoutFamily::Unknown => None,
    }
}

fn source_layout_reject_unplannable_entries(entries: &[SourceLayoutDiscoveryEntry]) -> Result<()> {
    for entry in entries {
        match entry.decode_state {
            SourceLayoutDecodeState::Decoded | SourceLayoutDecodeState::Absent => {}
            SourceLayoutDecodeState::Malformed => {
                return Err(LoomError::new(
                    Code::CorruptObject,
                    format!(
                        "source-layout migration plan rejected malformed {:?} record at {}: {}",
                        entry.family,
                        entry.source_address,
                        entry.rejection_reason.as_deref().unwrap_or("malformed")
                    ),
                ));
            }
            SourceLayoutDecodeState::UnknownFamily => {
                return Err(LoomError::new(
                    Code::CorruptObject,
                    format!(
                        "source-layout migration plan rejected unknown record at {}: {}",
                        entry.source_address,
                        entry
                            .rejection_reason
                            .as_deref()
                            .unwrap_or("unknown source-layout family")
                    ),
                ));
            }
            SourceLayoutDecodeState::Conflict => {
                return Err(LoomError::new(
                    Code::Conflict,
                    format!(
                        "source-layout migration plan rejected duplicate {:?} record {}: {}",
                        entry.family,
                        entry.key_or_identity.as_deref().unwrap_or("unknown"),
                        entry.rejection_reason.as_deref().unwrap_or("duplicate")
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn source_layout_migration_record(
    entry: &SourceLayoutDiscoveryEntry,
    source_root: Option<PageId>,
    canonical_address: String,
    bytes: Vec<u8>,
) -> Result<SourceLayoutMigrationRecord> {
    if entry.decode_state != SourceLayoutDecodeState::Decoded {
        return Err(corrupt("source-layout migration record is not decoded"));
    }
    Ok(SourceLayoutMigrationRecord {
        source_address: entry.source_address.clone(),
        source_root: source_root.map(|root| root.0),
        canonical_address,
        source_family: entry.family,
        source_ownership: entry.ownership,
        key_or_identity: entry.key_or_identity.clone(),
        generation: entry.generation,
        sequence: entry.sequence,
        payload_digest: Digest::blake3(&bytes).to_hex(),
        payload_len: bytes.len(),
        bytes,
    })
}

fn source_layout_migration_record_cmp(
    left: &SourceLayoutMigrationRecord,
    right: &SourceLayoutMigrationRecord,
) -> std::cmp::Ordering {
    (
        left.source_family,
        &left.key_or_identity,
        left.sequence,
        left.generation,
        &left.canonical_address,
        &left.payload_digest,
        &left.source_address,
    )
        .cmp(&(
            right.source_family,
            &right.key_or_identity,
            right.sequence,
            right.generation,
            &right.canonical_address,
            &right.payload_digest,
            &right.source_address,
        ))
}

fn source_layout_push_family_plan(
    families: &mut Vec<SourceLayoutMigrationFamilyPlan>,
    family: SourceLayoutFamily,
    family_id: u16,
    records: Vec<SourceLayoutMigrationRecord>,
) {
    if !records.is_empty() {
        families.push(SourceLayoutMigrationFamilyPlan {
            family,
            family_id,
            records,
        });
    }
}

fn source_layout_validate_plan_records(plan: &SourceLayoutMigrationPlan) -> Result<()> {
    source_layout_reject_duplicate_addresses("current", &plan.current_records)?;
    source_layout_reject_duplicate_addresses("source-pointers", &plan.source_pointers)?;
    source_layout_reject_duplicate_addresses("control", &plan.control_records)?;
    let current_pointer_root = source_layout_validate_current_pointer_records(plan)?;
    let mut family_ids = BTreeSet::new();
    for family in &plan.catalog_families {
        if !family_ids.insert(family.family_id) {
            return Err(LoomError::new(
                Code::Conflict,
                "source-layout migration duplicate family id",
            ));
        }
        source_layout_reject_duplicate_addresses("catalog-family", &family.records)?;
    }
    for record in &plan.current_records {
        source_layout_validate_record(
            record,
            SourceLayoutFamily::CurrentEntry,
            None,
            Some(CURRENT_RECORDS_FAMILY_ID),
        )?;
        source_layout_validate_current_record_ownership(
            record,
            &plan.source_identity,
            current_pointer_root,
        )?;
    }
    for record in &plan.source_pointers {
        source_layout_validate_record(
            record,
            SourceLayoutFamily::CurrentRootPointer,
            None,
            Some(CURRENT_RECORDS_FAMILY_ID),
        )?;
    }
    for family in &plan.catalog_families {
        let expected_family = source_layout_family_for_catalog_id(family.family_id)?;
        if family.family != expected_family {
            return Err(corrupt(
                "source-layout migration family assignment mismatch",
            ));
        }
        if family.records.is_empty() {
            return Err(corrupt("source-layout migration family has no records"));
        }
        for record in &family.records {
            source_layout_validate_record(
                record,
                record.source_family,
                Some(family.family_id),
                None,
            )?;
            source_layout_validate_catalog_record_ownership(record)?;
        }
    }
    for record in &plan.control_records {
        source_layout_validate_record(record, SourceLayoutFamily::Control, None, None)?;
        source_layout_validate_control_record_ownership(record)?;
    }
    Ok(())
}

fn source_layout_validate_current_pointer_records(
    plan: &SourceLayoutMigrationPlan,
) -> Result<Option<PageId>> {
    match plan.source_pointers.as_slice() {
        [] => Ok(None),
        [record] => {
            if record.source_ownership != SourceLayoutOwnership::LegacyOverlay {
                return Err(corrupt(
                    "source-layout current-root pointer ownership mismatch",
                ));
            }
            if record.source_address
                != source_layout_address(mutable_overlay_current_root_address())
                || record.canonical_address != "current-state-root"
            {
                return Err(corrupt(
                    "source-layout current-root pointer canonical source mismatch",
                ));
            }
            let decoded = decode_mutable_overlay_current_root_record(&record.bytes)?;
            if let Some(root) = decoded {
                let current_roots = plan
                    .current_records
                    .iter()
                    .filter_map(|record| record.source_root)
                    .collect::<BTreeSet<_>>();
                if !current_roots.is_empty()
                    && (current_roots.len() != 1 || !current_roots.contains(&root.0))
                {
                    return Err(corrupt(
                        "source-layout current-root pointer target mismatch",
                    ));
                }
            }
            Ok(decoded)
        }
        _ => Err(LoomError::new(
            Code::Conflict,
            "source-layout migration multiple current-root pointers",
        )),
    }
}

fn source_layout_reject_duplicate_addresses(
    label: &str,
    records: &[SourceLayoutMigrationRecord],
) -> Result<()> {
    let mut seen = BTreeSet::new();
    for record in records {
        if !seen.insert(record.canonical_address.clone()) {
            return Err(LoomError::new(
                Code::Conflict,
                format!("source-layout migration duplicate canonical address in {label}"),
            ));
        }
    }
    Ok(())
}

fn source_layout_verify_page_member(
    file: &mut dyn BackingIo,
    page_count: u64,
    root: PageId,
    record: &SourceLayoutMigrationRecord,
) -> Result<()> {
    let address = source_layout_decode_hex_address(&record.source_address)?;
    let loc = pagebtree::get(file, DATA_START, Some(root), &address, page_count)?
        .ok_or_else(|| corrupt("source-layout source record missing"))?;
    let bytes = read_blob_from_loc(file, loc)?;
    if bytes != record.bytes {
        return Err(corrupt("source-layout source record bytes mismatch"));
    }
    Ok(())
}

fn source_layout_plan_audit_records(
    plan: &SourceLayoutMigrationPlan,
) -> impl Iterator<Item = &SourceLayoutMigrationRecord> {
    plan.catalog_families
        .iter()
        .filter(|family| family.family == SourceLayoutFamily::AuditControl)
        .flat_map(|family| family.records.iter())
}

fn source_layout_validate_current_record_ownership(
    record: &SourceLayoutMigrationRecord,
    identity: &SourceLayoutSourceIdentity,
    current_pointer_root: Option<PageId>,
) -> Result<()> {
    if record.source_ownership != SourceLayoutOwnership::NestedCurrentRoot {
        return Err(corrupt(
            "source-layout current record ownership lacks current-root evidence",
        ));
    }
    if identity.current_record_root.is_none() && current_pointer_root.is_none() {
        return Err(corrupt(
            "source-layout current record ownership lacks current-root evidence",
        ));
    }
    if let Some(root) = current_pointer_root
        && identity.current_record_root.is_none()
        && !source_layout_record_source_matches_root(record, root)
    {
        return Err(corrupt(
            "source-layout current record source is not backed by decoded pointer",
        ));
    }
    if let Some(root) = identity.current_record_root
        && !source_layout_record_source_matches_root(record, PageId(root))
        && current_pointer_root
            .is_none_or(|pointer| !source_layout_record_source_matches_root(record, pointer))
    {
        return Err(corrupt(
            "source-layout current record source is not backed by captured current root",
        ));
    }
    Ok(())
}

fn source_layout_validate_catalog_record_ownership(
    record: &SourceLayoutMigrationRecord,
) -> Result<()> {
    let expected = match record.source_family {
        SourceLayoutFamily::AuditControl => SourceLayoutOwnership::ControlRootObject,
        SourceLayoutFamily::RetainedHistoryHead
        | SourceLayoutFamily::RetainedHistoryRecord
        | SourceLayoutFamily::OwnerToken
        | SourceLayoutFamily::SecondaryIndex
        | SourceLayoutFamily::MutableIdempotency
        | SourceLayoutFamily::WorkflowIdempotency => SourceLayoutOwnership::LegacyOverlay,
        _ => return Err(corrupt("source-layout catalog ownership family mismatch")),
    };
    if record.source_ownership != expected {
        return Err(corrupt("source-layout catalog record ownership mismatch"));
    }
    Ok(())
}

fn source_layout_validate_control_record_ownership(
    record: &SourceLayoutMigrationRecord,
) -> Result<()> {
    if record.source_ownership != SourceLayoutOwnership::ControlRootObject {
        return Err(corrupt("source-layout control record ownership mismatch"));
    }
    Ok(())
}

fn source_layout_record_source_matches_root(
    record: &SourceLayoutMigrationRecord,
    root: PageId,
) -> bool {
    record.source_root == Some(root.0)
}

fn source_layout_validate_record(
    record: &SourceLayoutMigrationRecord,
    expected_family: SourceLayoutFamily,
    expected_catalog_family_id: Option<u16>,
    expected_current_family_id: Option<u16>,
) -> Result<()> {
    if record.source_family != expected_family
        && !(expected_family == SourceLayoutFamily::RetainedHistoryRecord
            && record.source_family == SourceLayoutFamily::RetainedHistoryHead)
    {
        return Err(corrupt("source-layout migration record family mismatch"));
    }
    if record.payload_len != record.bytes.len() {
        return Err(corrupt(
            "source-layout migration record payload length mismatch",
        ));
    }
    if record.payload_digest != Digest::blake3(&record.bytes).to_hex() {
        return Err(corrupt(
            "source-layout migration record payload digest mismatch",
        ));
    }
    match expected_current_family_id {
        Some(CURRENT_RECORDS_FAMILY_ID) => {}
        Some(_) => return Err(corrupt("source-layout migration current family mismatch")),
        None => {}
    }
    match expected_catalog_family_id {
        Some(RETAINED_HISTORY_FAMILY_ID) => match record.source_family {
            SourceLayoutFamily::RetainedHistoryHead => {
                let (key, sequence) = decode_retained_history_head(&record.bytes)?;
                source_layout_validate_address(
                    record,
                    retained_history_head_address(&key),
                    sequence,
                )?;
            }
            SourceLayoutFamily::RetainedHistoryRecord => {
                let (key, sequence, _) = decode_retained_history_entry(&record.bytes)?;
                source_layout_validate_address(
                    record,
                    retained_history_record_address(&key, sequence),
                    sequence,
                )?;
            }
            _ => return Err(corrupt("source-layout retained-history family mismatch")),
        },
        Some(OWNER_TOKEN_FAMILY_ID) => {
            if record.source_family != SourceLayoutFamily::OwnerToken {
                return Err(corrupt("source-layout owner-token family mismatch"));
            }
            decode_mutable_overlay_owner_token_record(&record.bytes)?;
            source_layout_decode_hex_address(&record.canonical_address)?;
        }
        Some(SECONDARY_INDEX_FAMILY_ID) => {
            if record.source_family != SourceLayoutFamily::SecondaryIndex {
                return Err(corrupt("source-layout secondary-index family mismatch"));
            }
            let decoded = decode_mutable_overlay_secondary_index_record(&record.bytes)?;
            source_layout_validate_address(
                record,
                mutable_overlay_secondary_index_address(&decoded.index),
                decoded.generation.as_u64(),
            )?;
        }
        Some(MUTABLE_IDEMPOTENCY_FAMILY_ID) => {
            if record.source_family != SourceLayoutFamily::MutableIdempotency {
                return Err(corrupt("source-layout mutable-idempotency family mismatch"));
            }
            decode_mutable_overlay_idempotency_record(&record.bytes)?;
            source_layout_decode_hex_address(&record.canonical_address)?;
        }
        Some(WORKFLOW_IDEMPOTENCY_FAMILY_ID) => {
            if record.source_family != SourceLayoutFamily::WorkflowIdempotency {
                return Err(corrupt(
                    "source-layout workflow-idempotency family mismatch",
                ));
            }
            decode_workflow_transaction_idempotency_record(&record.bytes)?;
            source_layout_decode_hex_address(&record.canonical_address)?;
        }
        Some(AUDIT_RETENTION_FAMILY_ID) => {
            if record.source_family != SourceLayoutFamily::AuditControl {
                return Err(corrupt("source-layout audit-retention family mismatch"));
            }
            let (key, _) = decode_audit_retention_record(&record.bytes)?;
            let _ = source_layout_audit_sequence(&key);
            source_layout_validate_address(record, audit_retention_record_address(&key), 0)?;
        }
        Some(_) => return Err(corrupt("source-layout unsupported migration family")),
        None => match record.source_family {
            SourceLayoutFamily::CurrentEntry => {
                let entry = decode_mutable_overlay_entry(&record.bytes)?;
                source_layout_validate_address(
                    record,
                    mutable_overlay_entry_address(&entry.key),
                    entry.generation.as_u64(),
                )?;
            }
            SourceLayoutFamily::CurrentRootPointer => {
                decode_mutable_overlay_current_root_record(&record.bytes)?;
                if record.canonical_address != "current-state-root" {
                    return Err(corrupt(
                        "source-layout current-root pointer canonical target mismatch",
                    ));
                }
            }
            SourceLayoutFamily::Control => {}
            _ => return Err(corrupt("source-layout migration record family unassigned")),
        },
    }
    Ok(())
}

fn source_layout_validate_address(
    record: &SourceLayoutMigrationRecord,
    expected: [u8; 32],
    _decoded_ordinal: u64,
) -> Result<()> {
    let actual = source_layout_decode_hex_address(&record.canonical_address)?;
    if actual != expected {
        return Err(corrupt(
            "source-layout migration canonical address mismatch",
        ));
    }
    Ok(())
}

fn source_layout_family_for_catalog_id(family_id: u16) -> Result<SourceLayoutFamily> {
    match family_id {
        RETAINED_HISTORY_FAMILY_ID => Ok(SourceLayoutFamily::RetainedHistoryRecord),
        OWNER_TOKEN_FAMILY_ID => Ok(SourceLayoutFamily::OwnerToken),
        SECONDARY_INDEX_FAMILY_ID => Ok(SourceLayoutFamily::SecondaryIndex),
        MUTABLE_IDEMPOTENCY_FAMILY_ID => Ok(SourceLayoutFamily::MutableIdempotency),
        WORKFLOW_IDEMPOTENCY_FAMILY_ID => Ok(SourceLayoutFamily::WorkflowIdempotency),
        AUDIT_RETENTION_FAMILY_ID => Ok(SourceLayoutFamily::AuditControl),
        _ => Err(corrupt(
            "source-layout migration unsupported catalog family",
        )),
    }
}

fn source_layout_temp_record(record: &SourceLayoutMigrationRecord) -> Result<([u8; 32], Vec<u8>)> {
    Ok((
        source_layout_decode_hex_address(&record.canonical_address)?,
        record.bytes.clone(),
    ))
}

fn decode_source_layout_migration_region_table(page: &[u8]) -> Result<RegionTable> {
    RegionTable::decode(page)
        .ok_or_else(|| corrupt("source-layout temporary RegionTable decode failure"))
}

fn source_layout_write_temp_family_root(
    store: &FileStore,
    records: &[([u8; 32], Vec<u8>)],
    previous_root: Option<PageId>,
) -> Result<Option<PageId>> {
    if records.is_empty() {
        return Ok(previous_root);
    }
    let mut inner = store.inner.lock().map_err(|_| poisoned())?;
    let mut file = store.file.lock().map_err(|_| poisoned())?;
    let (reusable_free, _reclamation_lease) = store.transaction_reusable_free(
        &inner.free,
        inner.active_mark_epoch_reclaim_fence,
        inner.minimum_recoverable_generation,
    )?;
    let mut alloc = PageAllocator::new(inner.page_count, inner.generation + 1, reusable_free);
    alloc.install_metadata_bootstrap_reserve(&inner.metadata_bootstrap_reserve)?;
    let record_refs = records
        .iter()
        .map(|(address, bytes)| (*address, bytes.as_slice()))
        .collect::<Vec<_>>();
    let (root, _) = write_mutable_record_refs_to_root(
        &mut **file,
        &mut alloc,
        previous_root,
        inner.page_count,
        &record_refs,
        None,
        false,
    )?;
    inner.page_count = alloc.page_count();
    inner.free = alloc.snapshot_free();
    Ok(root)
}

fn source_layout_write_temp_control_root(
    store: &FileStore,
    digest_algo: Algo,
    control_map: &BTreeMap<Vec<u8>, Vec<u8>>,
) -> Result<Option<Digest>> {
    if control_map.is_empty() {
        return Ok(None);
    }
    let bytes = encode_control_map(control_map);
    let expected = Digest::hash(digest_algo, &bytes);
    let actual = store.put(&bytes)?;
    if actual != expected {
        return Err(corrupt("source-layout temporary control digest mismatch"));
    }
    let readback = store
        .get(&actual)?
        .ok_or_else(|| corrupt("source-layout temporary control readback missing"))?;
    if readback != bytes {
        return Err(corrupt("source-layout temporary control readback mismatch"));
    }
    Ok(Some(actual))
}

type SourceLayoutTempRootClosure = (Option<PageId>, Option<PageId>, Option<PageId>, u64);

fn source_layout_build_temp_canonical_closure(
    store: &FileStore,
    current_root: Option<PageId>,
    root_catalog_entries: &[RootCatalogEntry],
) -> Result<SourceLayoutTempRootClosure> {
    let mut inner = store.inner.lock().map_err(|_| poisoned())?;
    let mut file = store.file.lock().map_err(|_| poisoned())?;
    let (reusable_free, _reclamation_lease) = store.transaction_reusable_free(
        &inner.free,
        inner.active_mark_epoch_reclaim_fence,
        inner.minimum_recoverable_generation,
    )?;
    let mut alloc = PageAllocator::new(inner.page_count, inner.generation + 1, reusable_free);
    alloc.install_metadata_bootstrap_reserve(&inner.metadata_bootstrap_reserve)?;
    let object_index_root = inner.index_root;
    let catalog_root = if root_catalog_entries.is_empty() {
        None
    } else {
        let root = alloc.alloc(1);
        let catalog = RootCatalog {
            entries: root_catalog_entries.to_vec(),
        };
        let page = catalog
            .encode()
            .map_err(|_| corrupt("source-layout temporary root catalog encode failure"))?;
        let page_count_after_catalog = alloc.page_count();
        let decoded = RootCatalog::decode(&page)
            .map_err(|_| corrupt("source-layout temporary root catalog decode failure"))?;
        decoded
            .validate_root_bounds(page_count_after_catalog)
            .map_err(|_| corrupt("source-layout temporary root catalog bounds failure"))?;
        write_at(&mut **file, root.offset(DATA_START), &page).map_err(io_err)?;
        Some(root)
    };
    let region_root =
        if object_index_root.is_some() || current_root.is_some() || catalog_root.is_some() {
            let root = alloc.alloc(1);
            let canonical = CanonicalRegionTable {
                index_root: object_index_root,
                freemap_root: None,
                maintenance_root: None,
                current_record_root: current_root,
                root_catalog_root: catalog_root,
                open_segment: inner.open_segment,
                mutable_overlay_generation_floor: inner.mutable_overlay_generation_floor,
                minimum_recoverable_generation: inner.minimum_recoverable_generation,
                metadata_bootstrap_reserve: inner.metadata_bootstrap_reserve.clone(),
            };
            let page = canonical
                .encode(alloc.page_count())
                .map_err(|_| corrupt("source-layout temporary RegionTable encode failure"))?;
            let decoded = decode_source_layout_migration_region_table(&page)?;
            if decoded.overlay_root.is_some()
                || decoded.index_root != object_index_root
                || decoded.current_record_root != current_root
                || decoded.root_catalog_root != catalog_root
            {
                return Err(corrupt("source-layout temporary RegionTable root mismatch"));
            }
            let page_count_after_region = alloc.page_count();
            let decoded_canonical = CanonicalRegionTable::decode(&page).map_err(|_| {
                corrupt("source-layout temporary canonical RegionTable decode failure")
            })?;
            for root in [
                decoded_canonical.index_root,
                decoded_canonical.freemap_root,
                decoded_canonical.maintenance_root,
                decoded_canonical.current_record_root,
                decoded_canonical.root_catalog_root,
            ] {
                if let Some(root) = root
                    && root.0 >= page_count_after_region
                {
                    return Err(corrupt(
                        "source-layout temporary RegionTable bounds failure",
                    ));
                }
            }
            write_at(&mut **file, root.offset(DATA_START), &page).map_err(io_err)?;
            Some(root)
        } else {
            None
        };
    inner.page_count = alloc.page_count();
    inner.free = alloc.snapshot_free();
    Ok((
        object_index_root,
        catalog_root,
        region_root,
        inner.page_count,
    ))
}

fn source_layout_decode_hex_address(hex: &str) -> Result<[u8; 32]> {
    if hex.len() != 64 {
        return Err(corrupt("source-layout hex address length"));
    }
    let mut out = [0u8; 32];
    for (idx, slot) in out.iter_mut().enumerate() {
        let high = source_layout_hex_nibble(hex.as_bytes()[idx * 2])?;
        let low = source_layout_hex_nibble(hex.as_bytes()[idx * 2 + 1])?;
        *slot = (high << 4) | low;
    }
    Ok(out)
}

fn source_layout_decode_hex_bytes(hex: &str) -> Result<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return Err(corrupt("source-layout hex bytes length"));
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    for pair in hex.as_bytes().chunks_exact(2) {
        let high = source_layout_hex_nibble(pair[0])?;
        let low = source_layout_hex_nibble(pair[1])?;
        out.push((high << 4) | low);
    }
    Ok(out)
}

fn source_layout_hex_nibble(byte: u8) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(corrupt("source-layout hex address digit")),
    }
}

fn source_layout_audit_sequence(key: &[u8]) -> Option<u64> {
    key.strip_prefix(AUDIT_ENTRY_PREFIX)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u64::from_be_bytes)
}

fn source_layout_address(bytes: [u8; 32]) -> String {
    source_layout_hex(&bytes)
}

fn source_layout_bytes_identity(bytes: &[u8]) -> String {
    source_layout_hex(bytes)
}

fn source_layout_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

type MutableOverlayRecordRef<'a> = ([u8; 32], &'a [u8]);

struct RootFamilyRecordBatch<'a> {
    family_id: u16,
    root: Option<PageId>,
    records: &'a [MutableOverlayRecordRef<'a>],
}

#[derive(Debug)]
struct RootFamilyRecordBatchOutcome {
    roots: BTreeMap<u16, Option<PageId>>,
    delta_pack_candidate_root: Option<PageId>,
    fresh_delta_pack_advisories: BTreeSet<[u8; 32]>,
    touched_segments: BTreeSet<u64>,
}

fn root_family_uses_record_locators(codec: pagebtree::ValueCodecKind) -> bool {
    matches!(
        codec,
        pagebtree::ValueCodecKind::RecordLoc | pagebtree::ValueCodecKind::PackedRecordRef
    )
}

fn write_root_family_record_batches(
    file: &mut dyn BackingIo,
    alloc: &mut PageAllocator,
    page_count: u64,
    batches: &[RootFamilyRecordBatch<'_>],
    delta_pack_candidate_root: Option<PageId>,
    generation: u64,
    digest_algo: Algo,
    pack_replacements: bool,
    oldest_pinned_snapshot_generation: Option<u64>,
    audit_retention_active: bool,
) -> Result<RootFamilyRecordBatchOutcome> {
    let mut roots = BTreeMap::new();
    let mut owners = BTreeMap::<[u8; 32], u16>::new();
    let mut current_by_family = BTreeMap::<u16, Vec<([u8; 32], RecordLoc)>>::new();
    let mut new_records = Vec::new();
    let mut replacement_records = Vec::new();

    for batch in batches {
        let descriptor = root_family_descriptor(batch.family_id)
            .ok_or_else(|| corrupt("unknown root family publication"))?;
        if !root_family_uses_record_locators(descriptor.value_codec) {
            return Err(corrupt(
                "transaction-packed root family does not use record locators",
            ));
        }
        if batch.records.is_empty() {
            roots.insert(batch.family_id, batch.root);
            continue;
        }
        let current = overlay_current_record_locs(
            file,
            batch.root,
            page_count,
            descriptor.value_codec,
            batch.records.iter().map(|(address, _)| *address),
        )?;
        let replaced = current
            .iter()
            .map(|(address, _)| *address)
            .collect::<BTreeSet<_>>();
        for record in batch.records {
            if owners.insert(record.0, batch.family_id).is_some() {
                return Err(corrupt(
                    "transaction-packed record address belongs to multiple root families",
                ));
            }
            if replaced.contains(&record.0) && !pack_replacements {
                replacement_records.push(*record);
            } else {
                new_records.push(*record);
            }
        }
        current_by_family.insert(batch.family_id, current);
    }

    let mut touched_segments = BTreeSet::new();
    let mut eligible_locs = Vec::new();
    for batch in batches {
        let Some(current) = current_by_family.get(&batch.family_id) else {
            continue;
        };
        eligible_locs.extend(superseded_overlay_blob_locs_from_current(
            file,
            current,
            batch.records.iter().copied(),
            oldest_pinned_snapshot_generation,
            audit_retention_active,
        )?);
    }
    free_overlay_record_pages_batch(file, alloc, &eligible_locs, &mut touched_segments)?;

    let fresh_placements = record_io::write_blob_pages(file, alloc, &new_records)?;
    let mut placements = fresh_placements.clone();
    placements.extend(record_io::write_dedicated_blob_pages(
        file,
        alloc,
        &replacement_records,
    )?);
    let mut placements_by_family = BTreeMap::<u16, Vec<([u8; 32], RecordLoc)>>::new();
    for (address, loc) in placements {
        let family_id = owners
            .get(&address)
            .copied()
            .ok_or_else(|| corrupt("transaction-packed placement has no owning root family"))?;
        placements_by_family
            .entry(family_id)
            .or_default()
            .push((address, loc));
    }

    let payloads = new_records.iter().copied().collect::<BTreeMap<_, _>>();
    let mut advisories = BTreeMap::<[u8; 32], delta_pack::PackAdvisory>::new();
    for (address, loc) in &fresh_placements {
        let payload = payloads
            .get(address)
            .copied()
            .ok_or_else(|| corrupt("delta-pack placement payload missing"))?;
        if record::is_large(payload.len() as u64) {
            continue;
        }
        let family_id = owners
            .get(address)
            .copied()
            .ok_or_else(|| corrupt("delta-pack placement owner missing"))?;
        let member = delta_pack::PackMember {
            family_id,
            address: *address,
            digest: *Digest::hash(digest_algo, payload).bytes(),
            slot: loc.slot,
            payload_len: u32::try_from(payload.len())
                .map_err(|_| corrupt("delta-pack payload length out of range"))?,
        };
        let advisory_address = delta_pack::PackAdvisory::address(digest_algo, loc.global_page());
        match advisories.get_mut(&advisory_address) {
            Some(advisory) => advisory.members.push(member),
            None => {
                advisories.insert(
                    advisory_address,
                    delta_pack::PackAdvisory::new(
                        loc.global_page(),
                        generation,
                        vec![member],
                        BTreeSet::new(),
                    )?,
                );
            }
        }
    }
    for advisory in advisories.values_mut() {
        advisory.members.sort();
    }

    let mut superseded_by_page = BTreeMap::<u64, Vec<(u16, [u8; 32], RecordLoc)>>::new();
    for (family_id, current) in &current_by_family {
        for (address, loc) in current {
            superseded_by_page
                .entry(loc.global_page())
                .or_default()
                .push((*family_id, *address, *loc));
        }
    }
    for (page, superseded) in superseded_by_page {
        let advisory_address = delta_pack::PackAdvisory::address(digest_algo, page);
        let Some(mut advisory) = read_delta_pack_advisory(
            file,
            delta_pack_candidate_root,
            page_count,
            advisory_address,
        ) else {
            continue;
        };
        for (family_id, address, loc) in superseded {
            let prior_digest = read_blob_from_loc(file, loc)
                .map(|payload| *Digest::hash(digest_algo, &payload).bytes())?;
            if advisory.members.iter().any(|member| {
                member.family_id == family_id
                    && member.address == address
                    && member.slot == loc.slot
                    && member.digest == prior_digest
            }) {
                advisory.dead_slots.insert(loc.slot);
            }
        }
        advisories.insert(advisory_address, advisory);
    }

    for batch in batches {
        if batch.records.is_empty() {
            continue;
        }
        let descriptor = root_family_descriptor(batch.family_id)
            .ok_or_else(|| corrupt("unknown root family publication"))?;
        let updates = placements_by_family
            .remove(&batch.family_id)
            .unwrap_or_default();
        let root_batch = pagebtree::batch_upsert_with_codec(
            file,
            DATA_START,
            alloc,
            batch.root,
            &updates,
            page_count,
            descriptor.value_codec,
        )?;
        #[cfg(any(test, feature = "test-hooks"))]
        observe_btree_batch(root_batch.stats);
        roots.insert(batch.family_id, root_batch.root);
    }
    let fresh_delta_pack_advisories = fresh_placements
        .iter()
        .filter_map(|(address, loc)| {
            payloads.get(address).and_then(|payload| {
                (!record::is_large(payload.len() as u64))
                    .then(|| delta_pack::PackAdvisory::address(digest_algo, loc.global_page()))
            })
        })
        .collect::<BTreeSet<_>>();
    let delta_pack_candidate_root = write_delta_pack_advisories(
        file,
        alloc,
        delta_pack_candidate_root,
        page_count,
        digest_algo,
        &advisories,
        &mut touched_segments,
    )?;
    Ok(RootFamilyRecordBatchOutcome {
        roots,
        delta_pack_candidate_root,
        fresh_delta_pack_advisories,
        touched_segments,
    })
}

fn read_delta_pack_advisory(
    file: &mut dyn BackingIo,
    root: Option<PageId>,
    page_count: u64,
    address: [u8; 32],
) -> Option<delta_pack::PackAdvisory> {
    let loc = root_family_get(
        file,
        DELTA_PACK_CANDIDATE_FAMILY_ID,
        root,
        &address,
        page_count,
    )
    .ok()??;
    delta_pack::PackAdvisory::decode(&read_blob_from_loc(file, loc).ok()?).ok()
}

fn write_delta_pack_advisories(
    file: &mut dyn BackingIo,
    alloc: &mut PageAllocator,
    root: Option<PageId>,
    page_count: u64,
    digest_algo: Algo,
    advisories: &BTreeMap<[u8; 32], delta_pack::PackAdvisory>,
    touched_segments: &mut BTreeSet<u64>,
) -> Result<Option<PageId>> {
    if advisories.is_empty() {
        return Ok(root);
    }
    let mut complete_advisories = advisories.clone();
    let (root, current) = match overlay_current_record_locs(
        file,
        root,
        page_count,
        pagebtree::ValueCodecKind::RecordLoc,
        advisories.keys().copied(),
    ) {
        Ok(current) => (root, current),
        Err(_) => (None, Vec::new()),
    };
    let mut fully_relocated_slab_pages = BTreeSet::new();
    for page in current
        .iter()
        .map(|(_, loc)| loc.global_page())
        .collect::<BTreeSet<_>>()
    {
        let mut bytes = [0u8; PAGE_SIZE as usize];
        read_exact_at(file, PageId(page).offset(DATA_START), &mut bytes).map_err(io_err)?;
        let Some(slot_count) = record::slab_slot_count(&bytes) else {
            continue;
        };
        let mut page_is_decodable = true;
        for slot in 0..slot_count {
            let Some(payload) = record::read_slab_slot(&bytes, slot) else {
                page_is_decodable = false;
                break;
            };
            let Ok(advisory) = delta_pack::PackAdvisory::decode(payload) else {
                page_is_decodable = false;
                break;
            };
            let address = delta_pack::PackAdvisory::address(digest_algo, advisory.page);
            if root_family_get(
                file,
                DELTA_PACK_CANDIDATE_FAMILY_ID,
                root,
                &address,
                page_count,
            )? == Some(RecordLoc::from_global(page, slot))
            {
                complete_advisories.entry(address).or_insert(advisory);
            }
        }
        if page_is_decodable {
            fully_relocated_slab_pages.insert(page);
        }
    }
    let records = complete_advisories
        .iter()
        .map(|(address, advisory)| Ok((*address, advisory.encode()?)))
        .collect::<Result<Vec<_>>>()?;
    let refs = records
        .iter()
        .map(|(address, bytes)| (*address, bytes.as_slice()))
        .collect::<Vec<_>>();
    let placements = record_io::write_blob_pages(file, alloc, &refs)?;
    let batch = pagebtree::batch_upsert_with_codec(
        file,
        DATA_START,
        alloc,
        root,
        &placements,
        page_count,
        pagebtree::ValueCodecKind::RecordLoc,
    )?;
    let prior_locs = current
        .into_iter()
        .map(|(_, loc)| loc)
        .filter(|loc| !fully_relocated_slab_pages.contains(&loc.global_page()))
        .collect::<Vec<_>>();
    free_overlay_record_pages_batch(file, alloc, &prior_locs, touched_segments)?;
    for page in fully_relocated_slab_pages {
        alloc.free(PageId(page), 1)?;
        touched_segments.insert(page / page::PAGES_PER_SEGMENT);
    }
    Ok(batch.root)
}

fn write_root_catalog_page(
    file: &mut dyn BackingIo,
    alloc: &mut PageAllocator,
    previous_root: Option<PageId>,
    page_count: u64,
    entries: &[RootCatalogEntry],
) -> Result<Option<PageId>> {
    if entries.is_empty() {
        if let Some(root) = previous_root {
            alloc.free(root, 1)?;
        }
        return Ok(None);
    }
    if let Some(root) = previous_root {
        let existing = read_root_catalog(file, root, page_count)?;
        if existing.entries == entries {
            return Ok(Some(root));
        }
    }
    let root = alloc.alloc(1);
    let catalog = RootCatalog {
        entries: entries.to_vec(),
    };
    let page = catalog
        .encode()
        .map_err(|_| corrupt("root catalog encode failure"))?;
    write_at(file, root.offset(DATA_START), &page).map_err(io_err)?;
    if let Some(old) = previous_root {
        alloc.free(old, 1)?;
    }
    Ok(Some(root))
}

fn root_catalog_entries_with_family(
    entries: &[RootCatalogEntry],
    family_id: u16,
    root: Option<PageId>,
) -> Vec<RootCatalogEntry> {
    let mut next = entries
        .iter()
        .copied()
        .filter(|entry| entry.family_id != family_id)
        .collect::<Vec<_>>();
    if let Some(root) = root {
        next.push(RootCatalogEntry::authoritative(family_id, root));
    }
    next.sort_by_key(|entry| entry.family_id);
    next
}

fn root_catalog_entries_with_advisory_family(
    entries: &[RootCatalogEntry],
    family_id: u16,
    root: Option<PageId>,
) -> Vec<RootCatalogEntry> {
    let mut next = entries
        .iter()
        .copied()
        .filter(|entry| entry.family_id != family_id)
        .collect::<Vec<_>>();
    if let Some(root) = root {
        next.push(RootCatalogEntry::advisory(family_id, root));
    }
    next.sort_by_key(|entry| entry.family_id);
    next
}

fn write_mutable_record_refs_to_root(
    file: &mut dyn BackingIo,
    alloc: &mut PageAllocator,
    root: Option<PageId>,
    page_count: u64,
    records: &[MutableOverlayRecordRef<'_>],
    oldest_pinned_snapshot_generation: Option<u64>,
    audit_retention_active: bool,
) -> Result<(Option<PageId>, BTreeSet<u64>)> {
    write_mutable_record_refs_to_root_with_codec(
        file,
        alloc,
        root,
        page_count,
        records,
        oldest_pinned_snapshot_generation,
        audit_retention_active,
        pagebtree::ValueCodecKind::RecordLoc,
    )
}

fn write_mutable_record_refs_to_root_with_codec(
    file: &mut dyn BackingIo,
    alloc: &mut PageAllocator,
    root: Option<PageId>,
    page_count: u64,
    records: &[MutableOverlayRecordRef<'_>],
    oldest_pinned_snapshot_generation: Option<u64>,
    audit_retention_active: bool,
    codec: pagebtree::ValueCodecKind,
) -> Result<(Option<PageId>, BTreeSet<u64>)> {
    if records.is_empty() {
        return Ok((root, BTreeSet::new()));
    }
    let entries = overlay_current_record_locs(
        file,
        root,
        page_count,
        codec,
        records.iter().map(|(address, _)| *address),
    )?;
    let reclaimed = reclaim_superseded_overlay_blobs_from_current(
        file,
        alloc,
        &entries,
        records.iter().copied(),
        oldest_pinned_snapshot_generation,
        audit_retention_active,
    )?;
    let placements = write_overlay_blob_pages(file, alloc, &entries, records)?;
    let root_batch = pagebtree::batch_upsert_with_codec(
        file,
        DATA_START,
        alloc,
        root,
        &placements,
        page_count,
        codec,
    )?;
    #[cfg(any(test, feature = "test-hooks"))]
    observe_btree_batch(root_batch.stats);
    Ok((root_batch.root, reclaimed.into_iter().collect()))
}

fn write_root_family_record_refs_to_root(
    file: &mut dyn BackingIo,
    alloc: &mut PageAllocator,
    family_id: u16,
    root: Option<PageId>,
    page_count: u64,
    records: &[MutableOverlayRecordRef<'_>],
    oldest_pinned_snapshot_generation: Option<u64>,
    audit_retention_active: bool,
) -> Result<(Option<PageId>, BTreeSet<u64>)> {
    let descriptor = root_family_descriptor(family_id)
        .ok_or_else(|| corrupt("unknown root family publication"))?;
    write_mutable_record_refs_to_root_with_codec(
        file,
        alloc,
        root,
        page_count,
        records,
        oldest_pinned_snapshot_generation,
        audit_retention_active,
        descriptor.value_codec,
    )
}

fn write_audit_retention_map_to_root(
    file: &mut dyn BackingIo,
    alloc: &mut PageAllocator,
    previous_root: Option<PageId>,
    page_count: u64,
    map: &BTreeMap<Vec<u8>, Vec<u8>>,
) -> Result<Option<PageId>> {
    let Some(root) = previous_root else {
        let records = audit_retention_family_records(map);
        let record_refs = records
            .iter()
            .map(|(address, value)| (*address, value.as_slice()))
            .collect::<Vec<_>>();
        let (root, _) = write_root_family_record_refs_to_root(
            file,
            alloc,
            AUDIT_RETENTION_FAMILY_ID,
            None,
            page_count,
            &record_refs,
            None,
            false,
        )?;
        return Ok(root);
    };
    let _ = (file, alloc, root, page_count, map);
    Err(LoomError::invalid(
        "complete audit-retention map replacement requires explicit delta after family activation",
    ))
}

fn write_audit_retention_delta_to_root(
    file: &mut dyn BackingIo,
    alloc: &mut PageAllocator,
    previous_root: Option<PageId>,
    page_count: u64,
    delta: &AuditRetentionDelta,
    #[cfg(test)] instrumentation: Option<&AuditRetentionTestInstrumentation>,
) -> Result<Option<PageId>> {
    if delta.is_empty() {
        return Ok(previous_root);
    }
    let mut next_root = previous_root;
    let mut read_bound = page_count;
    let codec = root_family_value_codec(AUDIT_RETENTION_FAMILY_ID)?;
    for key in &delta.deletes {
        let address = audit_retention_record_address(key);
        next_root = pagebtree::delete_with_codec(
            file, DATA_START, alloc, next_root, &address, read_bound, codec,
        )?;
        read_bound = alloc.page_count();
        #[cfg(test)]
        if let Some(instrumentation) = instrumentation {
            instrumentation.point_deletes.fetch_add(1, Ordering::SeqCst);
        }
    }
    if !delta.puts.is_empty() {
        let records = delta
            .puts
            .iter()
            .map(|(key, value)| {
                (
                    audit_retention_record_address(key),
                    encode_audit_retention_record(key, value),
                )
            })
            .collect::<Vec<_>>();
        let current = overlay_current_record_locs(
            file,
            next_root,
            read_bound,
            codec,
            records.iter().map(|(address, _)| *address),
        )?;
        let record_refs = records
            .iter()
            .map(|(address, bytes)| (*address, bytes.as_slice()))
            .collect::<Vec<_>>();
        let placements = write_overlay_blob_pages(file, alloc, &current, &record_refs)?;
        let root_batch = pagebtree::batch_upsert_with_codec(
            file,
            DATA_START,
            alloc,
            next_root,
            &placements,
            read_bound,
            codec,
        )?;
        #[cfg(any(test, feature = "test-hooks"))]
        observe_btree_batch(root_batch.stats);
        next_root = root_batch.root;
        #[cfg(test)]
        if let Some(instrumentation) = instrumentation {
            instrumentation
                .point_puts
                .fetch_add(delta.puts.len() as u64, Ordering::SeqCst);
        }
    }
    Ok(next_root)
}

fn apply_audit_retention_delta(map: &mut BTreeMap<Vec<u8>, Vec<u8>>, delta: &AuditRetentionDelta) {
    for key in &delta.deletes {
        map.remove(key);
    }
    for (key, value) in &delta.puts {
        map.insert(key.clone(), value.clone());
    }
}

type ControlAndAuditRetentionMaps = (BTreeMap<Vec<u8>, Vec<u8>>, BTreeMap<Vec<u8>, Vec<u8>>);

fn split_audit_retention_control_map(
    map: BTreeMap<Vec<u8>, Vec<u8>>,
) -> ControlAndAuditRetentionMaps {
    let mut control = BTreeMap::new();
    let mut audit = BTreeMap::new();
    for (key, value) in map {
        if is_audit_retention_control_key(&key) {
            audit.insert(key, value);
        } else {
            control.insert(key, value);
        }
    }
    (control, audit)
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

fn split_mutable_overlay_records(
    records: &[([u8; 32], Vec<u8>)],
) -> (
    Vec<MutableOverlayRecordRef<'_>>,
    Vec<MutableOverlayRecordRef<'_>>,
) {
    let records = records
        .iter()
        .map(|(address, value)| (*address, value.as_slice()))
        .collect::<Vec<_>>();
    split_mutable_overlay_record_refs(&records)
}

fn split_mutable_overlay_record_refs<'a>(
    records: &[MutableOverlayRecordRef<'a>],
) -> (
    Vec<MutableOverlayRecordRef<'a>>,
    Vec<MutableOverlayRecordRef<'a>>,
) {
    let mut current = Vec::new();
    let mut control = Vec::new();
    for (address, value) in records.iter().copied() {
        if is_mutable_overlay_current_entry_record(value) {
            current.push((address, value));
        } else {
            control.push((address, value));
        }
    }
    (current, control)
}

fn mutable_overlay_generation_floor_from_current_records<'a>(
    previous_floor: u64,
    records: impl IntoIterator<Item = &'a [u8]>,
) -> Result<u64> {
    let mut floor = previous_floor;
    for value in records {
        if is_mutable_overlay_current_entry_record(value) {
            let entry = decode_mutable_overlay_entry(value)?;
            floor = floor.max(entry.generation.as_u64());
        }
    }
    Ok(floor)
}

fn is_mutable_overlay_current_entry_record(value: &[u8]) -> bool {
    value.starts_with(b"loom.store.mutable-overlay.entry.v1")
        || value.starts_with(b"loom.store.mutable-overlay.entry.v2")
        || value.starts_with(b"loom.store.mutable-overlay.entry.v3")
}

fn encode_mutable_overlay_entry(entry: &loom_core::MutableOverlayEntrySnapshot) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"loom.store.mutable-overlay.entry.v3");
    put_uvarint(&mut out, entry.generation.as_u64());
    put_uvarint(&mut out, entry.key.as_bytes().len() as u64);
    out.extend_from_slice(entry.key.as_bytes());
    out.push(match entry.kind {
        loom_core::OverlayEntryKind::Value => 1,
        loom_core::OverlayEntryKind::Tombstone => 2,
    });
    out.extend_from_slice(entry.owner_token.as_bytes());
    put_uvarint(&mut out, entry.payload.len() as u64);
    out.extend_from_slice(&entry.payload);
    out
}

fn decode_mutable_overlay_entry(bytes: &[u8]) -> Result<loom_core::MutableOverlayEntrySnapshot> {
    const HEADER_V1: &[u8] = b"loom.store.mutable-overlay.entry.v1";
    const HEADER_V2: &[u8] = b"loom.store.mutable-overlay.entry.v2";
    const HEADER_V3: &[u8] = b"loom.store.mutable-overlay.entry.v3";
    let version = if bytes.starts_with(HEADER_V2) {
        2
    } else if bytes.starts_with(HEADER_V3) {
        3
    } else if bytes.starts_with(HEADER_V1) {
        1
    } else {
        return Err(corrupt("mutable overlay entry schema mismatch"));
    };
    let mut pos = match version {
        3 => HEADER_V3.len(),
        2 => HEADER_V2.len(),
        _ => HEADER_V1.len(),
    };
    let generation = if version == 3 {
        let generation = get_uvarint(bytes, &mut pos)
            .ok_or_else(|| corrupt("mutable overlay entry generation truncated"))?;
        if generation == 0 {
            return Err(corrupt("mutable overlay entry generation invalid"));
        }
        loom_core::OverlayGeneration::new(generation)
    } else {
        loom_core::OverlayGeneration::new(0)
    };
    let key_len = get_uvarint(bytes, &mut pos)
        .ok_or_else(|| corrupt("mutable overlay entry key length truncated"))?
        as usize;
    let key_end = pos
        .checked_add(key_len)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| corrupt("mutable overlay entry key truncated"))?;
    let key = loom_core::OverlayKey::from_encoded_bytes(bytes[pos..key_end].to_vec())?;
    pos = key_end;
    let kind = match bytes.get(pos).copied() {
        Some(1) => loom_core::OverlayEntryKind::Value,
        Some(2) => loom_core::OverlayEntryKind::Tombstone,
        _ => return Err(corrupt("mutable overlay entry kind invalid")),
    };
    pos += 1;
    let owner_token = if version >= 2 {
        let token_end = pos
            .checked_add(32)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| corrupt("mutable overlay entry owner token truncated"))?;
        let token = loom_core::OverlayOwnerToken::from_bytes(
            bytes[pos..token_end]
                .try_into()
                .map_err(|_| corrupt("mutable overlay entry owner token invalid"))?,
        );
        pos = token_end;
        token
    } else {
        loom_core::OverlayOwnerToken::from_bytes([0; 32])
    };
    let payload_len = get_uvarint(bytes, &mut pos)
        .ok_or_else(|| corrupt("mutable overlay entry payload length truncated"))?
        as usize;
    let payload_end = pos
        .checked_add(payload_len)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| corrupt("mutable overlay entry payload truncated"))?;
    let payload = bytes[pos..payload_end].to_vec();
    pos = payload_end;
    if pos != bytes.len() {
        return Err(corrupt("mutable overlay entry trailing bytes"));
    }
    let owner_token = if version >= 2 {
        owner_token
    } else {
        let prior = None;
        match kind {
            loom_core::OverlayEntryKind::Value => {
                let mut overlay = loom_core::MutableOverlay::new();
                overlay.put_value(key.clone(), prior.as_ref(), payload.clone())?
            }
            loom_core::OverlayEntryKind::Tombstone => {
                let mut overlay = loom_core::MutableOverlay::new();
                overlay.put_tombstone(key.clone(), prior.as_ref())?
            }
        }
    };
    Ok(loom_core::MutableOverlayEntrySnapshot {
        key,
        generation,
        owner_token,
        kind,
        payload,
    })
}

fn encode_mutable_overlay_owner_token_record(token: &loom_core::OverlayOwnerToken) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MUTABLE_OVERLAY_OWNER_TOKEN_RECORD);
    out.extend_from_slice(token.as_bytes());
    out
}

fn decode_mutable_overlay_owner_token_record(bytes: &[u8]) -> Result<loom_core::OverlayOwnerToken> {
    if !bytes.starts_with(MUTABLE_OVERLAY_OWNER_TOKEN_RECORD) {
        return Err(corrupt("mutable overlay owner-token schema mismatch"));
    }
    let pos = MUTABLE_OVERLAY_OWNER_TOKEN_RECORD.len();
    if pos + 32 != bytes.len() {
        return Err(corrupt("mutable overlay owner-token length"));
    }
    Ok(loom_core::OverlayOwnerToken::from_bytes(
        bytes[pos..pos + 32]
            .try_into()
            .map_err(|_| corrupt("mutable overlay owner-token invalid"))?,
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MutableOverlaySecondaryIndexRecord {
    generation: loom_core::OverlayGeneration,
    index: loom_core::OverlayKey,
    kind: loom_core::OverlayEntryKind,
    payload: Option<Vec<u8>>,
}

fn encode_mutable_overlay_secondary_index_record(
    generation: loom_core::OverlayGeneration,
    write: &SecondaryIndexWrite,
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MUTABLE_OVERLAY_SECONDARY_INDEX_RECORD);
    put_uvarint(&mut out, generation.as_u64());
    put_uvarint(&mut out, write.index.as_bytes().len() as u64);
    out.extend_from_slice(write.index.as_bytes());
    match &write.op {
        loom_core::SecondaryIndexWriteOp::Put { payload } => {
            out.push(1);
            put_uvarint(&mut out, payload.len() as u64);
            out.extend_from_slice(payload);
        }
        loom_core::SecondaryIndexWriteOp::Delete => out.push(2),
    }
    out
}

fn decode_mutable_overlay_secondary_index_record(
    bytes: &[u8],
) -> Result<MutableOverlaySecondaryIndexRecord> {
    if !bytes.starts_with(MUTABLE_OVERLAY_SECONDARY_INDEX_RECORD) {
        return Err(corrupt("mutable overlay secondary-index schema mismatch"));
    }
    let mut pos = MUTABLE_OVERLAY_SECONDARY_INDEX_RECORD.len();
    let generation = loom_core::OverlayGeneration::new(
        get_uvarint(bytes, &mut pos)
            .ok_or_else(|| corrupt("mutable overlay secondary-index generation truncated"))?,
    );
    let key_len = get_uvarint(bytes, &mut pos)
        .ok_or_else(|| corrupt("mutable overlay secondary-index key length truncated"))?
        as usize;
    let key_end = pos
        .checked_add(key_len)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| corrupt("mutable overlay secondary-index key truncated"))?;
    let index = loom_core::OverlayKey::from_encoded_bytes(bytes[pos..key_end].to_vec())?;
    pos = key_end;
    let kind = match bytes.get(pos).copied() {
        Some(1) => loom_core::OverlayEntryKind::Value,
        Some(2) => loom_core::OverlayEntryKind::Tombstone,
        _ => return Err(corrupt("mutable overlay secondary-index kind invalid")),
    };
    pos += 1;
    let payload = if kind == loom_core::OverlayEntryKind::Value {
        let payload_len = get_uvarint(bytes, &mut pos)
            .ok_or_else(|| corrupt("mutable overlay secondary-index payload length truncated"))?
            as usize;
        let payload_end = pos
            .checked_add(payload_len)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| corrupt("mutable overlay secondary-index payload truncated"))?;
        let payload = bytes[pos..payload_end].to_vec();
        pos = payload_end;
        Some(payload)
    } else {
        None
    };
    if pos != bytes.len() {
        return Err(corrupt("mutable overlay secondary-index trailing bytes"));
    }
    Ok(MutableOverlaySecondaryIndexRecord {
        generation,
        index,
        kind,
        payload,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MutableOverlayIdempotencyRecord {
    request_digest: Digest,
    owner_token: loom_core::OverlayOwnerToken,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkflowTransactionIdempotencyRecord {
    request_digest: Digest,
    receipt: CommitReceipt,
}

fn put_workflow_receipt_string(out: &mut Vec<u8>, value: &str) {
    put_uvarint(out, value.len() as u64);
    out.extend_from_slice(value.as_bytes());
}

fn get_workflow_receipt_count(
    bytes: &[u8],
    pos: &mut usize,
    aggregate: &mut loom_core::WorkflowAggregateByteBudget,
    max: usize,
    label: &str,
) -> Result<usize> {
    let start = *pos;
    let count = get_uvarint(bytes, pos).ok_or_else(|| corrupt(label))?;
    reserve_workflow_receipt_aggregate(aggregate, pos.saturating_sub(start), label)?;
    if count > max as u64 {
        return Err(corrupt(label));
    }
    usize::try_from(count).map_err(|_| corrupt(label))
}

fn get_workflow_receipt_bytes<'a>(
    bytes: &'a [u8],
    pos: &mut usize,
    aggregate: &mut loom_core::WorkflowAggregateByteBudget,
    max_len: usize,
    label: &str,
) -> Result<&'a [u8]> {
    let start = *pos;
    let len = get_uvarint(bytes, pos).ok_or_else(|| corrupt(label))?;
    reserve_workflow_receipt_aggregate(aggregate, pos.saturating_sub(start), label)?;
    if len > max_len as u64 {
        return Err(corrupt(label));
    }
    let len = usize::try_from(len).map_err(|_| corrupt(label))?;
    reserve_workflow_receipt_aggregate(aggregate, len, label)?;
    let end = pos
        .checked_add(len)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| corrupt(label))?;
    let value = &bytes[*pos..end];
    *pos = end;
    Ok(value)
}

fn get_workflow_receipt_string(
    bytes: &[u8],
    pos: &mut usize,
    aggregate: &mut loom_core::WorkflowAggregateByteBudget,
    label: &str,
) -> Result<String> {
    let value = get_workflow_receipt_bytes(
        bytes,
        pos,
        aggregate,
        loom_core::WORKFLOW_RECEIPT_MAX_STRING_BYTES,
        label,
    )?;
    String::from_utf8(value.to_vec()).map_err(|_| corrupt(label))
}

fn get_workflow_receipt_uvarint(
    bytes: &[u8],
    pos: &mut usize,
    aggregate: &mut loom_core::WorkflowAggregateByteBudget,
    label: &str,
) -> Result<u64> {
    let start = *pos;
    let value = get_uvarint(bytes, pos).ok_or_else(|| corrupt(label))?;
    reserve_workflow_receipt_aggregate(aggregate, pos.saturating_sub(start), label)?;
    Ok(value)
}

fn reserve_workflow_receipt_aggregate(
    aggregate: &mut loom_core::WorkflowAggregateByteBudget,
    bytes: usize,
    label: &str,
) -> Result<()> {
    if !aggregate.reserve(bytes) {
        return Err(corrupt(label));
    }
    Ok(())
}

fn encode_mutable_overlay_idempotency_record(
    request_digest: &Digest,
    token: &loom_core::OverlayOwnerToken,
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MUTABLE_OVERLAY_IDEMPOTENCY_RECORD);
    out.extend_from_slice(request_digest.bytes());
    out.extend_from_slice(token.as_bytes());
    out
}

fn decode_mutable_overlay_idempotency_record(
    bytes: &[u8],
) -> Result<MutableOverlayIdempotencyRecord> {
    if !bytes.starts_with(MUTABLE_OVERLAY_IDEMPOTENCY_RECORD) {
        return Err(corrupt("mutable overlay idempotency schema mismatch"));
    }
    let pos = MUTABLE_OVERLAY_IDEMPOTENCY_RECORD.len();
    if pos + 64 != bytes.len() {
        return Err(corrupt("mutable overlay idempotency length"));
    }
    let request_digest = Digest::of(
        Algo::Blake3,
        bytes[pos..pos + 32]
            .try_into()
            .map_err(|_| corrupt("mutable overlay idempotency digest invalid"))?,
    );
    let owner_token = loom_core::OverlayOwnerToken::from_bytes(
        bytes[pos + 32..pos + 64]
            .try_into()
            .map_err(|_| corrupt("mutable overlay idempotency token invalid"))?,
    );
    Ok(MutableOverlayIdempotencyRecord {
        request_digest,
        owner_token,
    })
}

fn encode_workflow_transaction_idempotency_record(
    request_digest: &Digest,
    receipt: &CommitReceipt,
) -> Result<Vec<u8>> {
    receipt.aggregate_encoded_len()?;
    let mut out = Vec::new();
    out.extend_from_slice(MUTABLE_OVERLAY_TRANSACTION_IDEMPOTENCY_RECORD);
    out.extend_from_slice(request_digest.bytes());
    put_uvarint(&mut out, receipt.generation.as_u64());
    out.extend_from_slice(receipt.root_after.bytes());
    put_uvarint(&mut out, receipt.writes.len() as u64);
    for write in &receipt.writes {
        out.push(write.facet.stable_tag());
        put_uvarint(&mut out, write.target.as_bytes().len() as u64);
        out.extend_from_slice(write.target.as_bytes());
        out.extend_from_slice(write.owner_token.as_bytes());
        out.push(match write.change {
            loom_core::OverlayEntryKind::Value => 1,
            loom_core::OverlayEntryKind::Tombstone => 2,
        });
    }
    put_uvarint(&mut out, receipt.operation_identities.len() as u64);
    for operation_id in &receipt.operation_identities {
        put_workflow_receipt_string(&mut out, operation_id);
    }
    put_uvarint(&mut out, receipt.revision_identities.len() as u64);
    for revision in &receipt.revision_identities {
        put_workflow_receipt_string(&mut out, &revision.entity_id);
        put_workflow_receipt_string(&mut out, &revision.revision_id);
    }
    put_uvarint(&mut out, receipt.audit_sequences.len() as u64);
    for sequence in &receipt.audit_sequences {
        put_uvarint(&mut out, *sequence);
    }
    put_uvarint(&mut out, receipt.retained_sequences.len() as u64);
    for retained in &receipt.retained_sequences {
        put_uvarint(&mut out, retained.key.len() as u64);
        out.extend_from_slice(&retained.key);
        put_uvarint(&mut out, retained.first_sequence);
        put_uvarint(&mut out, retained.last_sequence);
    }
    put_uvarint(&mut out, receipt.delivery_receipts.len() as u64);
    for delivery in &receipt.delivery_receipts {
        put_workflow_receipt_string(&mut out, &delivery.stream_id);
        put_uvarint(&mut out, delivery.sequence);
        put_workflow_receipt_string(&mut out, &delivery.envelope_id);
        out.extend_from_slice(delivery.payload_digest.bytes());
    }
    match receipt.post_commit_delta.as_ref() {
        Some(delta) => {
            out.push(1);
            out.extend_from_slice(delta.workspace.as_bytes());
            put_uvarint(&mut out, delta.changed_paths.len() as u64);
            for path in &delta.changed_paths {
                put_workflow_receipt_string(&mut out, path);
            }
            put_uvarint(&mut out, delta.changed_content_count as u64);
        }
        None => out.push(0),
    }
    Ok(out)
}

fn decode_workflow_transaction_idempotency_record(
    bytes: &[u8],
) -> Result<WorkflowTransactionIdempotencyRecord> {
    if !bytes.starts_with(MUTABLE_OVERLAY_TRANSACTION_IDEMPOTENCY_RECORD) {
        return Err(corrupt("workflow transaction idempotency schema mismatch"));
    }
    let mut pos = MUTABLE_OVERLAY_TRANSACTION_IDEMPOTENCY_RECORD.len();
    let digest_end = pos
        .checked_add(32)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| corrupt("workflow transaction idempotency digest truncated"))?;
    let request_digest = Digest::of(
        Algo::Blake3,
        bytes[pos..digest_end]
            .try_into()
            .map_err(|_| corrupt("workflow transaction idempotency digest invalid"))?,
    );
    pos = digest_end;
    let mut aggregate = loom_core::WorkflowAggregateByteBudget::new();
    let generation = loom_core::OverlayGeneration::new(get_workflow_receipt_uvarint(
        bytes,
        &mut pos,
        &mut aggregate,
        "workflow transaction idempotency generation truncated",
    )?);
    let root_end = pos
        .checked_add(32)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| corrupt("workflow transaction idempotency root truncated"))?;
    reserve_workflow_receipt_aggregate(
        &mut aggregate,
        32,
        "workflow transaction idempotency root truncated",
    )?;
    let root_after = Digest::of(
        Algo::Blake3,
        bytes[pos..root_end]
            .try_into()
            .map_err(|_| corrupt("workflow transaction idempotency root invalid"))?,
    );
    pos = root_end;
    let write_count = get_workflow_receipt_count(
        bytes,
        &mut pos,
        &mut aggregate,
        loom_core::WORKFLOW_RECEIPT_MAX_WRITES,
        "workflow transaction idempotency write count truncated or too large",
    )?;
    let mut writes = Vec::with_capacity(write_count);
    for _ in 0..write_count {
        let facet = bytes
            .get(pos)
            .copied()
            .ok_or_else(|| corrupt("workflow transaction idempotency facet truncated"))
            .and_then(|tag| {
                FacetKind::from_stable_tag(tag)
                    .ok_or_else(|| corrupt("workflow transaction idempotency facet invalid"))
            })?;
        reserve_workflow_receipt_aggregate(
            &mut aggregate,
            1,
            "workflow transaction idempotency facet truncated",
        )?;
        pos += 1;
        let key = get_workflow_receipt_bytes(
            bytes,
            &mut pos,
            &mut aggregate,
            loom_core::WORKFLOW_RECEIPT_MAX_KEY_BYTES,
            "workflow transaction idempotency key truncated or too large",
        )?;
        let target = loom_core::OverlayKey::from_encoded_bytes(key.to_vec())?;
        let token_end = pos
            .checked_add(32)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| corrupt("workflow transaction idempotency token truncated"))?;
        reserve_workflow_receipt_aggregate(
            &mut aggregate,
            32,
            "workflow transaction idempotency token truncated",
        )?;
        let owner_token = loom_core::OverlayOwnerToken::from_bytes(
            bytes[pos..token_end]
                .try_into()
                .map_err(|_| corrupt("workflow transaction idempotency token invalid"))?,
        );
        pos = token_end;
        let change = match bytes.get(pos).copied() {
            Some(1) => loom_core::OverlayEntryKind::Value,
            Some(2) => loom_core::OverlayEntryKind::Tombstone,
            _ => return Err(corrupt("workflow transaction idempotency change invalid")),
        };
        reserve_workflow_receipt_aggregate(
            &mut aggregate,
            1,
            "workflow transaction idempotency change invalid",
        )?;
        pos += 1;
        writes.push(WriteOutcome {
            facet,
            target,
            owner_token,
            change,
        });
    }
    let mut operation_identities = Vec::new();
    let mut revision_identities = Vec::new();
    let mut audit_sequences = Vec::new();
    let mut retained_sequences = Vec::new();
    let mut delivery_receipts = Vec::new();
    let mut post_commit_delta = None;
    if pos != bytes.len() {
        let operation_count = get_workflow_receipt_count(
            bytes,
            &mut pos,
            &mut aggregate,
            loom_core::WORKFLOW_RECEIPT_MAX_OPERATIONS,
            "workflow transaction idempotency operation count truncated or too large",
        )?;
        operation_identities.reserve(operation_count);
        for _ in 0..operation_count {
            operation_identities.push(get_workflow_receipt_string(
                bytes,
                &mut pos,
                &mut aggregate,
                "workflow transaction idempotency operation id truncated",
            )?);
        }
        let revision_count = get_workflow_receipt_count(
            bytes,
            &mut pos,
            &mut aggregate,
            loom_core::WORKFLOW_RECEIPT_MAX_REVISIONS,
            "workflow transaction idempotency revision count truncated or too large",
        )?;
        revision_identities.reserve(revision_count);
        for _ in 0..revision_count {
            let entity_id = get_workflow_receipt_string(
                bytes,
                &mut pos,
                &mut aggregate,
                "workflow transaction idempotency revision entity truncated",
            )?;
            let revision_id = get_workflow_receipt_string(
                bytes,
                &mut pos,
                &mut aggregate,
                "workflow transaction idempotency revision id truncated",
            )?;
            revision_identities.push(loom_core::RevisionReceipt {
                entity_id,
                revision_id,
            });
        }
        let audit_count = get_workflow_receipt_count(
            bytes,
            &mut pos,
            &mut aggregate,
            loom_core::WORKFLOW_RECEIPT_MAX_AUDIT_SEQUENCES,
            "workflow transaction idempotency audit count truncated or too large",
        )?;
        audit_sequences.reserve(audit_count);
        for _ in 0..audit_count {
            audit_sequences.push(get_workflow_receipt_uvarint(
                bytes,
                &mut pos,
                &mut aggregate,
                "workflow transaction idempotency audit sequence truncated",
            )?);
        }
        let retained_count = get_workflow_receipt_count(
            bytes,
            &mut pos,
            &mut aggregate,
            loom_core::WORKFLOW_RECEIPT_MAX_RETAINED_SEQUENCES,
            "workflow transaction idempotency retained count truncated or too large",
        )?;
        retained_sequences.reserve(retained_count);
        for _ in 0..retained_count {
            let key = get_workflow_receipt_bytes(
                bytes,
                &mut pos,
                &mut aggregate,
                loom_core::WORKFLOW_RECEIPT_MAX_KEY_BYTES,
                "workflow transaction idempotency retained key truncated or too large",
            )?
            .to_vec();
            let first_sequence = get_workflow_receipt_uvarint(
                bytes,
                &mut pos,
                &mut aggregate,
                "workflow transaction idempotency retained first sequence truncated",
            )?;
            let last_sequence = get_workflow_receipt_uvarint(
                bytes,
                &mut pos,
                &mut aggregate,
                "workflow transaction idempotency retained last sequence truncated",
            )?;
            retained_sequences.push(loom_core::RetainedSequenceReceipt {
                key,
                first_sequence,
                last_sequence,
            });
        }
        let delivery_count = get_workflow_receipt_count(
            bytes,
            &mut pos,
            &mut aggregate,
            loom_core::WORKFLOW_RECEIPT_MAX_DELIVERY_RECEIPTS,
            "workflow transaction idempotency delivery count truncated or too large",
        )?;
        delivery_receipts.reserve(delivery_count);
        for _ in 0..delivery_count {
            let stream_id = get_workflow_receipt_string(
                bytes,
                &mut pos,
                &mut aggregate,
                "workflow transaction idempotency delivery stream truncated",
            )?;
            let sequence = get_workflow_receipt_uvarint(
                bytes,
                &mut pos,
                &mut aggregate,
                "workflow transaction idempotency delivery sequence truncated",
            )?;
            let envelope_id = get_workflow_receipt_string(
                bytes,
                &mut pos,
                &mut aggregate,
                "workflow transaction idempotency delivery envelope truncated",
            )?;
            let digest_end = pos
                .checked_add(32)
                .filter(|end| *end <= bytes.len())
                .ok_or_else(|| {
                    corrupt("workflow transaction idempotency delivery digest truncated")
                })?;
            reserve_workflow_receipt_aggregate(
                &mut aggregate,
                32,
                "workflow transaction idempotency delivery digest truncated",
            )?;
            let payload_digest = Digest::of(
                Algo::Blake3,
                bytes[pos..digest_end].try_into().map_err(|_| {
                    corrupt("workflow transaction idempotency delivery digest invalid")
                })?,
            );
            pos = digest_end;
            delivery_receipts.push(loom_core::DeliveryReceipt {
                stream_id,
                sequence,
                envelope_id,
                payload_digest,
            });
        }
        post_commit_delta = match bytes.get(pos).copied() {
            Some(0) => {
                reserve_workflow_receipt_aggregate(
                    &mut aggregate,
                    1,
                    "workflow transaction idempotency post-commit tag invalid",
                )?;
                pos += 1;
                None
            }
            Some(1) => {
                reserve_workflow_receipt_aggregate(
                    &mut aggregate,
                    1,
                    "workflow transaction idempotency post-commit tag invalid",
                )?;
                pos += 1;
                let workspace_end = pos
                    .checked_add(16)
                    .filter(|end| *end <= bytes.len())
                    .ok_or_else(|| {
                        corrupt("workflow transaction idempotency post-commit workspace truncated")
                    })?;
                reserve_workflow_receipt_aggregate(
                    &mut aggregate,
                    16,
                    "workflow transaction idempotency post-commit workspace truncated",
                )?;
                let workspace = WorkspaceId::from_bytes(
                    bytes[pos..workspace_end].try_into().map_err(|_| {
                        corrupt("workflow transaction idempotency post-commit workspace invalid")
                    })?,
                );
                pos = workspace_end;
                let path_count = get_workflow_receipt_count(
                    bytes,
                    &mut pos,
                    &mut aggregate,
                    loom_core::WORKFLOW_RECEIPT_MAX_CHANGED_PATHS,
                    "workflow transaction idempotency post-commit path count truncated or too large",
                )?;
                let mut changed_paths = Vec::with_capacity(path_count);
                for _ in 0..path_count {
                    changed_paths.push(get_workflow_receipt_string(
                        bytes,
                        &mut pos,
                        &mut aggregate,
                        "workflow transaction idempotency post-commit path truncated",
                    )?);
                }
                let changed_content_count = get_workflow_receipt_uvarint(
                    bytes,
                    &mut pos,
                    &mut aggregate,
                    "workflow transaction idempotency post-commit content count truncated",
                )?;
                if changed_content_count > loom_core::WORKFLOW_RECEIPT_MAX_CHANGED_CONTENT_COUNT {
                    return Err(corrupt(
                        "workflow transaction idempotency post-commit content count too large",
                    ));
                }
                let changed_content_count =
                    usize::try_from(changed_content_count).map_err(|_| {
                        corrupt(
                            "workflow transaction idempotency post-commit content count invalid",
                        )
                    })?;
                Some(loom_core::PostCommitDeltaReceipt {
                    workspace,
                    changed_paths,
                    changed_content_count,
                })
            }
            _ => {
                return Err(corrupt(
                    "workflow transaction idempotency post-commit tag invalid",
                ));
            }
        };
    }
    if pos != bytes.len() {
        return Err(corrupt("workflow transaction idempotency trailing bytes"));
    }
    Ok(WorkflowTransactionIdempotencyRecord {
        request_digest,
        receipt: CommitReceipt {
            generation,
            root_after,
            writes,
            operation_identities,
            revision_identities,
            audit_sequences,
            retained_sequences,
            delivery_receipts,
            post_commit_delta,
            replayed: true,
        },
    })
}

/// CRC-32C (Castagnoli), software bitwise, reflected polynomial `0x82F63B78`. No dependency.
pub(crate) fn crc32c(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0x82F6_3B78 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod storage_invariant_tests;
#[cfg(test)]
mod tests;
