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
    ]
}
use loom_core::keys::{DekSession, KeySpec};
use loom_core::provider::ObjectStore;
use loom_core::{CompressionHint, FacetKind, Loom, WorkspaceId};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
#[cfg(not(target_arch = "wasm32"))]
use std::fs::{File, OpenOptions};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;
// `Path` is only referenced by the native-file API (open/open_loom/compaction helpers), all of which
// are cfg-gated off for wasm32; `PathBuf` stays unconditional (it is the `FileStore.path` field type).
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

mod frame;
mod journal;
mod maintenance;
mod maintenance_policy;
mod mark_epoch;
mod page;
mod pagebtree;
mod pagemap;
mod record;

use maintenance::{MaintenanceState, read_maintenance};
use page::{PAGE_SIZE, PageId, RegionTable};

pub const STORE_PAGE_SIZE: u64 = PAGE_SIZE;
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
pub use maintenance_policy::{
    StoreMaintenancePolicy, StoreMaintenanceReport, StoreMaintenanceRunState,
};
pub use mark_epoch::{
    ReachabilityMarkEpoch, begin_loom_reachability_mark_epoch, step_loom_reachability_mark_epoch,
    step_loom_reachability_mark_epoch_until,
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
    pub facet_durability_overrides: [Option<StoreDurabilityPolicy>; 21],
}

impl Default for StorePolicy {
    fn default() -> Self {
        Self {
            fips_required: false,
            default_durability: StoreDurabilityPolicy::Normal,
            facet_durability_overrides: [None; 21],
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
    /// Reader pins currently blocking overlay reclamation. `None` when this count is not available
    /// from the store diagnostics surface: the live reader-pin registry is owned by the daemon layer
    /// (`daemon::DaemonPinStatus`) and is not reachable here, so the store reports "unavailable"
    /// rather than a hardcoded zero that would falsely imply no blockers.
    pub pinned_reader_blockers: Option<u64>,
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
    default_codec: Codec, // codec attempted for new object records; a runtime write policy only,
    // reads are self-describing, so it isn't persisted
    group: Mutex<GroupCommit>, // staging queue that coalesces concurrent writers into one fsync
    // Lock-free durability diagnostics counters surfaced through GroupCommitDiagnostics.
    group_commit_metrics: GroupCommitMetrics,
    mutable_overlay: Mutex<loom_core::MutableOverlay>,
    mutable_overlay_enumerations: AtomicU64,
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
    freemap: Option<(PageId, u64)>, // (root, page span) of the persisted free-page map
    region_table_root: Option<PageId>, // page holding the region roots, freed and rewritten each commit
    maintenance_root: Option<PageId>,  // page holding conservative maintenance metadata
    maintenance: MaintenanceState,
    open_segment: u64,      // segment new record pages are attributed to
    free: Vec<FreePageRun>, // reclaimable page runs (superseded pages), persisted each commit
    // Encoded `encryption_meta`, immutable after creation; carried into every
    // superblock write so checkpoints and compaction preserve it. `None` = unencrypted.
    encryption_meta: Option<Vec<u8>>,
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

    /// Open the `.loom` at `path` read-only and lock-free: many readers can open the same loom
    /// concurrently and they do not exclude a writer. The file must already exist; writes through the
    /// returned handle fail at the OS (the descriptor is read-only). Native-file-only.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_read(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_inner(path.as_ref().to_path_buf(), false, false)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open_inner(path: PathBuf, writable: bool, enforce_daemon_guard: bool) -> Result<Self> {
        // Plain open: an existing store reads its own profile from the superblock; a fresh one created
        // here gets the default (blake3) profile. FIPS stores are created via `create_with_profile`.
        Self::open_inner_enc(path, writable, enforce_daemon_guard, None, Algo::Blake3)
    }

    /// Create a fresh `.loom` under an explicit identity profile: `Algo::Blake3` is
    /// the default profile, `Algo::Sha256` the FIPS profile. The profile is immutable once written.
    /// Native-file-only.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn create_with_profile(path: impl AsRef<Path>, digest_algo: Algo) -> Result<Self> {
        let store =
            Self::open_inner_enc(path.as_ref().to_path_buf(), true, true, None, digest_algo)?;
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
    ) -> Result<Self> {
        if writable && enforce_daemon_guard {
            reject_daemon_owned_direct_open(&path)?;
        }
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
        let store = Self::open_over_backing(
            Box::new(file),
            writable,
            path,
            encryption,
            create_digest_algo,
        )?;
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
        Self::open_over_backing(backing, writable, PathBuf::new(), None, Algo::Blake3)
    }

    /// Create a fresh `FileStore` over a caller-supplied backing under an explicit identity profile
    /// (the browser / in-memory counterpart of [`create_with_profile`](Self::create_with_profile)).
    pub fn with_backing_profile(
        backing: Box<dyn BackingIo>,
        writable: bool,
        digest_algo: Algo,
    ) -> Result<Self> {
        Self::open_over_backing(backing, writable, PathBuf::new(), None, digest_algo)
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
        )?;
        *store.dek.lock().map_err(|_| poisoned())? = Some(session);
        Ok(store)
    }

    /// Recover (or, when empty, initialize) a `FileStore` over `backing`, independent of how the
    /// backing is realized. `path` is used only by native compaction's atomic rename. `encryption` is
    /// `Some` only when **creating** a fresh encrypted store; opening an existing store reads its
    /// encryption metadata from the superblock instead.
    fn open_over_backing(
        mut backing: Box<dyn BackingIo>,
        writable: bool,
        path: PathBuf,
        encryption: Option<Vec<u8>>,
        // The identity-profile digest algorithm to use when *creating* a fresh store.
        // Opening an existing store ignores this and reads the algorithm from the superblock instead.
        create_digest_algo: Algo,
    ) -> Result<Self> {
        let len = backing.size().map_err(io_err)?;

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
                    freemap: None,
                    region_table_root: None,
                    maintenance_root: None,
                    maintenance: MaintenanceState::default(),
                    open_segment: 0,
                    free: Vec::new(),
                    encryption_meta: encryption,
                }),
                path,
                default_codec: Codec::Deflate,
                group: Mutex::new(GroupCommit::default()),
                group_commit_metrics: GroupCommitMetrics::default(),
                mutable_overlay: Mutex::new(loom_core::MutableOverlay::new()),
                mutable_overlay_enumerations: AtomicU64::new(0),
                overlay_publication: Mutex::new(()),
                pending_mutable_idempotency: Mutex::new(BTreeMap::new()),
                pending_workflow_idempotency: Mutex::new(BTreeMap::new()),
                mvcc_snapshot_registry: Arc::new(Mutex::new(StoreMvccSnapshotRegistry::default())),
                hot_mutable_queue: Mutex::new(HotMutableCommitQueue::default()),
                maintenance_index_scan: Mutex::new(None),
                dek: Mutex::new(None),
                digest_algo: create_digest_algo,
            };
            store.load_mutable_overlay_from_storage()?;
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
            let off = JOURNAL_OFFSET + i * journal::RECORD_SIZE as u64;
            if read_exact_at(&mut *backing, off, &mut rbuf).is_ok()
                && let Some((journal::KIND_COMMIT, jr)) = journal::decode(&rbuf)
                && newest.is_none_or(|n| jr.generation > n.generation)
            {
                newest = Some(jr);
            }
        }
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
        let (index_root, overlay_root, freemap_root, maintenance_root, open_segment) =
            match sb.region_table {
                Some(rt) => {
                    let region = read_region_table(&mut *backing, rt, sb.page_count)?;
                    (
                        region.index_root,
                        region.overlay_root,
                        region.freemap_root,
                        region.maintenance_root,
                        region.open_segment,
                    )
                }
                None => (None, None, None, None, 0),
            };
        let mut index = BTreeMap::new();
        let mut index_materialized = false;

        // Restore the persisted free-page map (consistent with the recovered generation) so reuse of
        // reclaimed pages survives the restart rather than starting empty.
        let (free, freemap) = match freemap_root {
            Some(root) => {
                let runs = pagemap::read_map(&mut *backing, DATA_START, root, sb.page_count)?;
                let span = pagemap::map_pages(runs.len());
                (runs, Some((root, span)))
            }
            None => (Vec::new(), None),
        };
        let mut maintenance = match maintenance_root {
            Some(root) => read_maintenance(&mut *backing, root, sb.page_count)?,
            None => MaintenanceState::default(),
        };
        if !maintenance.object_count_known {
            if let Some(root) = index_root {
                for (key, loc) in
                    pagebtree::load_all(&mut *backing, DATA_START, root, sb.page_count)?
                {
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
                freemap,
                region_table_root: sb.region_table,
                maintenance_root,
                maintenance,
                open_segment,
                free,
                encryption_meta: sb.encryption,
            }),
            path,
            default_codec: Codec::Deflate,
            group: Mutex::new(GroupCommit::default()),
            group_commit_metrics: GroupCommitMetrics::default(),
            mutable_overlay: Mutex::new(loom_core::MutableOverlay::new()),
            mutable_overlay_enumerations: AtomicU64::new(0),
            overlay_publication: Mutex::new(()),
            pending_mutable_idempotency: Mutex::new(BTreeMap::new()),
            pending_workflow_idempotency: Mutex::new(BTreeMap::new()),
            mvcc_snapshot_registry: Arc::new(Mutex::new(StoreMvccSnapshotRegistry::default())),
            hot_mutable_queue: Mutex::new(HotMutableCommitQueue::default()),
            maintenance_index_scan: Mutex::new(None),
            dek: Mutex::new(None),
            digest_algo: sb.digest_algo,
        };
        store.load_mutable_overlay_from_storage()?;
        Ok(store)
    }

    /// The codec attempted for newly written object records. The size and shrink guardrails still
    /// apply per object, so incompressible or tiny payloads are stored identity regardless. Reads are
    /// self-describing (the frame id is in each record), so changing this is always safe and affects
    /// only subsequent writes.
    pub fn set_default_codec(&mut self, codec: Codec) {
        self.default_codec = codec;
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
        self.mutable_overlay_enumerations
            .fetch_add(1, Ordering::Relaxed);
        self.mutable_overlay
            .lock()
            .map_err(|_| poisoned())?
            .export_entries()
    }

    pub fn mutable_overlay_enumeration_count(&self) -> u64 {
        self.mutable_overlay_enumerations.load(Ordering::Relaxed)
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
        let mut overlay = self.mutable_overlay.lock().map_err(|_| poisoned())?;
        let before_snapshot = overlay.snapshot();
        let mut outcomes = Vec::with_capacity(txn.writes.len());
        let mut writes = Vec::with_capacity(txn.writes.len());
        for (write, durability) in txn.writes.iter().zip(write_durabilities.iter().copied()) {
            let expected = write.expected.as_ref().map(|token| &token.0);
            let token = match match &write.op {
                loom_core::FacetWriteOp::Put { payload } => {
                    overlay.put_value(write.target.clone(), expected, payload.clone())
                }
                loom_core::FacetWriteOp::Delete => {
                    overlay.put_tombstone(write.target.clone(), expected)
                }
            } {
                Ok(token) => token,
                Err(error) => {
                    *overlay =
                        loom_core::MutableOverlay::fork_from_snapshot(before_snapshot.clone());
                    return Err(error);
                }
            };
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
        let receipt = CommitReceipt {
            generation: overlay_generation,
            root_after,
            writes: outcomes,
            replayed: false,
        };
        if let Some(idempotency) = txn.idempotency.as_ref()
            && !records.is_empty()
        {
            records.push((
                mutable_overlay_transaction_idempotency_address(idempotency.as_bytes()),
                encode_workflow_transaction_idempotency_record(&request_digest, &receipt),
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
            self.commit_workflow_owner_state_records(&records, &txn.owner_state)
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
    ) -> Result<()> {
        let mut retained_heads = BTreeMap::<Vec<u8>, u64>::new();
        let mut records = records.to_vec();
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
            records.push((
                retained_history_head_address(key),
                encode_retained_history_head(key, head),
            ));
            retained_heads.insert(key.clone(), head);
        }
        let records = records
            .into_iter()
            .collect::<BTreeMap<_, _>>()
            .into_iter()
            .collect::<Vec<_>>();
        let oldest_pinned_snapshot_generation = self
            .oldest_pinned_mvcc_snapshot_generation()?
            .map(|generation| generation.as_u64());
        let audit_retention_active = self.audit_config()?.legal_hold;
        let mut control_map = self.control_map()?;
        let mut control_changed = false;
        for write in &owner_state.controls {
            match write {
                loom_core::WorkflowControlWrite::Put { key, payload } => {
                    control_map.insert(key.clone(), payload.clone());
                    control_changed = true;
                }
                loom_core::WorkflowControlWrite::Delete { key } => {
                    control_map.remove(key);
                    control_changed = true;
                }
                loom_core::WorkflowControlWrite::AppendRetained { .. } => {}
            }
        }
        for audit in &owner_state.audits {
            append_audit_record(
                &mut control_map,
                self.digest_algo,
                audit.principal,
                &audit.action,
                audit.target.as_deref(),
            )?;
        }
        let control_bytes = if !control_changed && owner_state.audits.is_empty() {
            None
        } else {
            let bytes = encode_control_map(&control_map);
            Some((Digest::hash(self.digest_algo, &bytes), bytes))
        };
        let mut owner_objects = owner_state.objects.clone();
        if let Some((digest, bytes)) = &control_bytes {
            owner_objects.push((*digest, bytes.clone()));
        }

        let mut inner = self.inner.lock().map_err(|_| poisoned())?;
        let reference = match owner_state.reference {
            loom_core::WorkflowReferenceUpdate::Keep => {
                inner.reference_root.map(|digest| *digest.bytes())
            }
            loom_core::WorkflowReferenceUpdate::Set(root) => root.map(|digest| *digest.bytes()),
        };
        let control = control_bytes
            .as_ref()
            .map(|(digest, _)| *digest.bytes())
            .or_else(|| inner.control_root.map(|digest| *digest.bytes()));
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
        let (roots, index_root, object_placements) = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            let mut alloc = PageAllocator::new_with_current_free_reusable(
                inner.page_count,
                new_gen,
                inner.free.clone(),
            );
            let (current_records, control_records) = split_mutable_overlay_records(&records);
            let current_root_before = read_mutable_overlay_current_root(
                &mut **file,
                inner.overlay_root,
                inner.page_count,
            )?;
            let current_entries = overlay_current_record_locs(
                &mut **file,
                current_root_before,
                inner.page_count,
                current_records.iter().map(|(address, _)| *address),
            )?;
            let mut reclaimed = reclaim_superseded_overlay_blobs_from_current(
                &mut **file,
                &mut alloc,
                &current_entries,
                current_records.iter().copied(),
                oldest_pinned_snapshot_generation,
                audit_retention_active,
            )?;
            let current_placements = write_overlay_blob_pages(
                &mut **file,
                &mut alloc,
                &current_entries,
                &current_records,
            )?;
            let mut current_root = current_root_before;
            for (address, loc) in &current_placements {
                let bound = alloc.page_count();
                current_root = Some(pagebtree::insert(
                    &mut **file,
                    DATA_START,
                    &mut alloc,
                    current_root,
                    address,
                    *loc,
                    bound,
                )?);
            }
            let current_root_record = mutable_overlay_current_root_record(current_root);
            let mut control_records = control_records;
            control_records.push((current_root_record.0, current_root_record.1.as_slice()));
            let overlay_entries = overlay_current_record_locs(
                &mut **file,
                inner.overlay_root,
                inner.page_count,
                control_records.iter().map(|(address, _)| *address),
            )?;
            reclaimed.extend(reclaim_superseded_overlay_blobs_from_current(
                &mut **file,
                &mut alloc,
                &overlay_entries,
                control_records.iter().copied(),
                oldest_pinned_snapshot_generation,
                audit_retention_active,
            )?);
            let overlay_placements = write_overlay_blob_pages(
                &mut **file,
                &mut alloc,
                &overlay_entries,
                &control_records,
            )?;
            let mut overlay_root = inner.overlay_root;
            for (address, loc) in &overlay_placements {
                let bound = alloc.page_count();
                overlay_root = Some(pagebtree::insert(
                    &mut **file,
                    DATA_START,
                    &mut alloc,
                    overlay_root,
                    address,
                    *loc,
                    bound,
                )?);
            }
            let dek = self.dek.lock().map_err(|_| poisoned())?;
            let object_placements =
                write_record_pages(&mut **file, &mut alloc, &fresh, dek.as_ref())?;
            drop(dek);
            let mut index_root = inner.index_root;
            for (key, loc) in &object_placements {
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
            let mut touched_segments = object_placements
                .iter()
                .map(|(_, loc)| loc.segment_id)
                .collect::<BTreeSet<_>>();
            touched_segments.extend(reclaimed);
            let object_count = inner
                .maintenance
                .object_count
                .saturating_add(fresh.len() as u64);
            let roots = finish_txn(
                &mut **file,
                &mut alloc,
                new_gen,
                object_count,
                index_root,
                overlay_root,
                inner.open_segment,
                reference,
                control,
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
            (roots, index_root, object_placements)
        };
        inner.generation = new_gen;
        inner.page_count = roots.page_count;
        inner.index_root = index_root;
        inner.overlay_root = roots.overlay_root;
        Self::clear_index_page_cache_locked(&mut inner);
        inner.free = roots.free;
        inner.freemap = roots.freemap;
        inner.region_table_root = Some(roots.region_table_root);
        inner.maintenance_root = Some(roots.maintenance_root);
        inner.maintenance = roots.maintenance;
        for (key, loc) in object_placements {
            Self::cache_locator_locked(&mut inner, key, loc);
        }
        if let loom_core::WorkflowReferenceUpdate::Set(root) = owner_state.reference {
            inner.reference_root = root;
        }
        if control_changed || !owner_state.audits.is_empty() {
            inner.control_root = control_bytes.map(|(digest, _)| digest);
        }
        Ok(())
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
        self.mutable_overlay_record_payload(address)?
            .map(|bytes| decode_mutable_overlay_owner_token_record(&bytes))
            .transpose()
    }

    fn mutable_overlay_idempotency_record(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<MutableOverlayIdempotencyRecord>> {
        self.mutable_overlay_record_payload(&mutable_overlay_idempotency_address(idempotency_key))?
            .map(|bytes| decode_mutable_overlay_idempotency_record(&bytes))
            .transpose()
    }

    pub fn mutable_overlay_secondary_index_value(
        &self,
        index: &loom_core::OverlayKey,
    ) -> Result<Option<Vec<u8>>> {
        let Some(bytes) =
            self.mutable_overlay_record_payload(&mutable_overlay_secondary_index_address(index))?
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
            self.mutable_overlay_record_payload(&retained_history_head_address(key))?
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
                .mutable_overlay_record_payload(&retained_history_record_address(key, sequence))?
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
        self.mutable_overlay_record_payload(&mutable_overlay_transaction_idempotency_address(
            idempotency_key,
        ))?
        .map(|bytes| decode_workflow_transaction_idempotency_record(&bytes))
        .transpose()
    }

    fn mutable_overlay_record_payload(&self, address: &[u8; 32]) -> Result<Option<Vec<u8>>> {
        let (root, page_count) = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            (inner.overlay_root, inner.page_count)
        };
        let Some(root) = root else {
            return Ok(None);
        };
        let loc = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            pagebtree::get(&mut **file, DATA_START, Some(root), address, page_count)?
        };
        let Some(loc) = loc else {
            return Ok(None);
        };
        self.read_blob_at_loc(loc, page_count).map(Some)
    }

    fn load_mutable_overlay_from_storage(&self) -> Result<()> {
        let (root, page_count) = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            (inner.overlay_root, inner.page_count)
        };
        let Some(root) = root else {
            return Ok(());
        };
        let mut used_current_root = false;
        let mut control_records_skipped = 0u64;
        let mut generation = 0;
        let mut entries = Vec::new();
        {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            if let Some(loc) = pagebtree::get(
                &mut **file,
                DATA_START,
                Some(root),
                &mutable_overlay_meta_address(),
                page_count,
            )? {
                generation = decode_mutable_overlay_meta(&read_blob_from_loc(&mut **file, loc)?)?;
            }
            if let Some(current_root) =
                read_mutable_overlay_current_root(&mut **file, Some(root), page_count)?
            {
                used_current_root = true;
                for (_, loc) in
                    pagebtree::load_all(&mut **file, DATA_START, current_root, page_count)?
                {
                    entries.push(decode_mutable_overlay_entry(&read_blob_from_loc(
                        &mut **file,
                        loc,
                    )?)?);
                }
            } else {
                for (address, loc) in
                    pagebtree::load_all(&mut **file, DATA_START, root, page_count)?
                {
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
            }
        }
        let entries_loaded = entries.len() as u64;
        entries.sort_by_key(|entry| entry.generation);
        let mut overlay = loom_core::MutableOverlay::import_entries(&entries)?;
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
        let roots = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            let mut alloc = PageAllocator::new_with_current_free_reusable(
                inner.page_count,
                new_gen,
                inner.free.clone(),
            );
            let (current_records, control_records) = split_mutable_overlay_records(&records);
            let current_root_before = read_mutable_overlay_current_root(
                &mut **file,
                inner.overlay_root,
                inner.page_count,
            )?;
            let current_entries = overlay_current_record_locs(
                &mut **file,
                current_root_before,
                inner.page_count,
                current_records.iter().map(|(address, _)| *address),
            )?;
            let mut reclaimed = reclaim_superseded_overlay_blobs_from_current(
                &mut **file,
                &mut alloc,
                &current_entries,
                current_records.iter().copied(),
                oldest_pinned_snapshot_generation,
                audit_retention_active,
            )?;
            let current_placements = write_overlay_blob_pages(
                &mut **file,
                &mut alloc,
                &current_entries,
                &current_records,
            )?;
            let mut current_root = current_root_before;
            for (address, loc) in &current_placements {
                let bound = alloc.page_count();
                current_root = Some(pagebtree::insert(
                    &mut **file,
                    DATA_START,
                    &mut alloc,
                    current_root,
                    address,
                    *loc,
                    bound,
                )?);
            }
            let current_root_record = mutable_overlay_current_root_record(current_root);
            let mut control_records = control_records;
            control_records.push((current_root_record.0, current_root_record.1.as_slice()));
            let overlay_entries = overlay_current_record_locs(
                &mut **file,
                inner.overlay_root,
                inner.page_count,
                control_records.iter().map(|(address, _)| *address),
            )?;
            reclaimed.extend(reclaim_superseded_overlay_blobs_from_current(
                &mut **file,
                &mut alloc,
                &overlay_entries,
                control_records.iter().copied(),
                oldest_pinned_snapshot_generation,
                audit_retention_active,
            )?);
            let placements = write_overlay_blob_pages(
                &mut **file,
                &mut alloc,
                &overlay_entries,
                &control_records,
            )?;
            let mut overlay_root = inner.overlay_root;
            for (address, loc) in &placements {
                let bound = alloc.page_count();
                overlay_root = Some(pagebtree::insert(
                    &mut **file,
                    DATA_START,
                    &mut alloc,
                    overlay_root,
                    address,
                    *loc,
                    bound,
                )?);
            }
            finish_txn(
                &mut **file,
                &mut alloc,
                new_gen,
                inner.maintenance.object_count,
                inner.index_root,
                overlay_root,
                inner.open_segment,
                inner.reference_root.map(|d| *d.bytes()),
                inner.control_root.map(|d| *d.bytes()),
                &inner.maintenance,
                &reclaimed,
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
        inner.generation = new_gen;
        inner.page_count = roots.page_count;
        inner.overlay_root = roots.overlay_root;
        inner.free = roots.free;
        inner.freemap = roots.freemap;
        inner.region_table_root = Some(roots.region_table_root);
        inner.maintenance_root = Some(roots.maintenance_root);
        inner.maintenance = roots.maintenance;
        Ok(())
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
        let new_gen = inner.generation + 1;
        let roots = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            let mut alloc = PageAllocator::new_with_current_free_reusable(
                inner.page_count,
                new_gen,
                inner.free.clone(),
            );
            let records = records
                .iter()
                .map(|(address, value)| (*address, value.as_slice()))
                .collect::<Vec<_>>();
            let entries = overlay_current_record_locs(
                &mut **file,
                inner.overlay_root,
                inner.page_count,
                records.iter().map(|(address, _)| *address),
            )?;
            let placements = write_overlay_blob_pages(&mut **file, &mut alloc, &entries, &records)?;
            let mut overlay_root = inner.overlay_root;
            for (address, loc) in &placements {
                let bound = alloc.page_count();
                overlay_root = Some(pagebtree::insert(
                    &mut **file,
                    DATA_START,
                    &mut alloc,
                    overlay_root,
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
                inner.index_root,
                overlay_root,
                inner.open_segment,
                inner.reference_root.map(|d| *d.bytes()),
                inner.control_root.map(|d| *d.bytes()),
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
        inner.generation = new_gen;
        inner.page_count = roots.page_count;
        inner.overlay_root = roots.overlay_root;
        inner.free = roots.free;
        inner.freemap = roots.freemap;
        inner.region_table_root = Some(roots.region_table_root);
        inner.maintenance_root = Some(roots.maintenance_root);
        inner.maintenance = roots.maintenance;
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
            // The live reader-pin registry is owned by the daemon layer and is not reachable from the
            // store diagnostics surface, so this count is reported as unavailable rather than a
            // hardcoded zero.
            pinned_reader_blockers: None,
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
            freemap,
            region_table_root,
            maintenance_root,
            free,
            index_locs,
        ) = {
            let mut inner = self.inner.lock().map_err(|_| poisoned())?;
            self.materialize_index_locked(&mut inner)?;
            (
                inner.maintenance.physical_page_count,
                inner.index_root,
                inner.overlay_root,
                inner.freemap,
                inner.region_table_root,
                inner.maintenance_root,
                inner.free.clone(),
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
        if let Some(root) =
            read_mutable_overlay_current_root(&mut **file, overlay_root, page_count)?
        {
            for page in pagebtree::collect_pages(&mut **file, DATA_START, root, page_count)? {
                classify_page_run(&mut pages, page.0, 1, "mutable_overlay_current_tree_page");
            }
            overlay_locs.extend(
                pagebtree::load_all(&mut **file, DATA_START, root, page_count)?
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
        let Some(root) = inner.overlay_root else {
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
        let current_root = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            read_mutable_overlay_current_root(&mut **file, Some(root), inner.page_count)?
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
        let (roots, compacted_current_records, rewritten_record_bytes, freed_record_pages) = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            let mut alloc = PageAllocator::new_with_current_free_reusable(
                inner.page_count,
                new_gen,
                inner.free.clone(),
            );
            let current_entries =
                pagebtree::load_all(&mut **file, DATA_START, current_root, inner.page_count)?;
            pagebtree::free_all(
                &mut **file,
                DATA_START,
                &mut alloc,
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
                        alloc.free(PageId(*page), 1);
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
            let mut next_current_root = None;
            for (address, loc) in &next_current_entries {
                let bound = alloc.page_count();
                next_current_root = Some(pagebtree::insert(
                    &mut **file,
                    DATA_START,
                    &mut alloc,
                    next_current_root,
                    address,
                    *loc,
                    bound,
                )?);
            }
            let current_root_record = mutable_overlay_current_root_record(next_current_root);
            let control_entries = overlay_current_record_locs(
                &mut **file,
                Some(root),
                inner.page_count,
                std::iter::once(current_root_record.0),
            )?;
            let control_records = [(current_root_record.0, current_root_record.1.as_slice())];
            freed_segments.extend(reclaim_superseded_overlay_blobs_from_current(
                &mut **file,
                &mut alloc,
                &control_entries,
                control_records.iter().copied(),
                None,
                false,
            )?);
            let placements = write_overlay_blob_pages(
                &mut **file,
                &mut alloc,
                &control_entries,
                &control_records,
            )?;
            let mut overlay_root = Some(root);
            for (address, loc) in &placements {
                let bound = alloc.page_count();
                overlay_root = Some(pagebtree::insert(
                    &mut **file,
                    DATA_START,
                    &mut alloc,
                    overlay_root,
                    address,
                    *loc,
                    bound,
                )?);
            }
            let roots = finish_txn(
                &mut **file,
                &mut alloc,
                new_gen,
                inner.maintenance.object_count,
                inner.index_root,
                overlay_root,
                inner.open_segment,
                inner.reference_root.map(|d| *d.bytes()),
                inner.control_root.map(|d| *d.bytes()),
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
        inner.generation = new_gen;
        inner.page_count = roots.page_count;
        inner.overlay_root = roots.overlay_root;
        inner.free = roots.free;
        inner.freemap = roots.freemap;
        inner.region_table_root = Some(roots.region_table_root);
        inner.maintenance_root = Some(roots.maintenance_root);
        inner.maintenance = roots.maintenance;
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
        let (overlay_root, page_count, durable_reclaim_floor) = {
            let inner = self.inner.lock().map_err(|_| poisoned())?;
            (
                inner.overlay_root,
                inner.page_count,
                durable_reclaim_floor_override.unwrap_or(overlay_health.current_generation),
            )
        };
        let mut current_records = Vec::new();
        if let Some(root) = overlay_root {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            let Some(current_root) =
                read_mutable_overlay_current_root(&mut **file, Some(root), page_count)?
            else {
                let pinned_generations = mvcc
                    .pins
                    .iter()
                    .map(|pin| pin.identity.overlay_generation)
                    .collect();
                return Ok(MutableOverlayCheckpointPlan {
                    overlay_generation: loom_core::OverlayGeneration::new(
                        overlay_health.current_generation,
                    ),
                    active_snapshot_count: mvcc.active_snapshot_count,
                    oldest_pinned_generation: mvcc.oldest_pinned_overlay_generation,
                    pinned_generations,
                    current_record_count: overlay_health.current_record_count,
                    tombstone_count: overlay_health.tombstone_count,
                    compactable_current_records: 0,
                    blocked_current_records: 0,
                    stale_record_bytes,
                    reusable_free_bytes,
                    current_records,
                });
            };
            for (_address, loc) in
                pagebtree::load_all(&mut **file, DATA_START, current_root, page_count)?
            {
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
        Ok(self.control_map()?.get(key).cloned())
    }

    /// Set one durable-local control-plane value.
    pub fn control_set(&self, key: &[u8], value: Vec<u8>) -> Result<()> {
        let mut map = self.control_map()?;
        map.insert(key.to_vec(), value);
        self.write_control_map(map)
    }

    pub fn control_set_audited(
        &self,
        key: &[u8],
        value: Vec<u8>,
        principal: Option<WorkspaceId>,
        action: &str,
        target: Option<&str>,
    ) -> Result<u64> {
        let mut map = self.control_map()?;
        map.insert(key.to_vec(), value);
        let seq = append_audit_record(&mut map, self.digest_algo, principal, action, target)?;
        self.write_control_map(map)?;
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
        let mut map = self.control_map()?;
        map.insert(key.to_vec(), value);
        self.commit_control_map_with_reference(map, reference_root)
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
        let mut map = self.control_map()?;
        map.insert(key.to_vec(), value);
        let seq = append_audit_record(&mut map, self.digest_algo, principal, action, target)?;
        self.commit_control_map_with_reference(map, reference_root)?;
        Ok(seq)
    }

    fn commit_control_map_with_reference(
        &self,
        map: BTreeMap<Vec<u8>, Vec<u8>>,
        reference_root: Option<Digest>,
    ) -> Result<()> {
        let reference = Some(reference_root.map(|d| *d.bytes()));
        if map.is_empty() {
            return self.commit_txn(&[], reference, Some(None), None);
        }
        let bytes = encode_control_map(&map);
        let digest = Digest::hash(self.digest_algo, &bytes);
        let codec = self.default_codec;
        self.commit_txn(
            &[(digest, bytes.as_slice(), codec)],
            reference,
            Some(Some(*digest.bytes())),
            None,
        )
    }

    /// Delete one durable-local control-plane value; returns whether it was present.
    pub fn control_delete(&self, key: &[u8]) -> Result<bool> {
        let mut map = self.control_map()?;
        let present = map.remove(key).is_some();
        if present {
            self.write_control_map(map)?;
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
        let mut map = self.control_map()?;
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
        let mut map = self.control_map()?;
        map.insert(IDENTITY_STORE_KEY.to_vec(), identity.encode());
        let seq = append_audit_record(&mut map, self.digest_algo, principal, action, target)?;
        self.write_control_map(map)?;
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
        let mut map = self.control_map()?;
        let seq = append_audit_record(&mut map, self.digest_algo, principal, action, target)?;
        let mut stored = policy.clone();
        stored.schema_version = AUTHORITY_REPLICATION_SCHEMA_VERSION;
        stored.last_modified_audit_seq = Some(seq);
        map.insert(IDENTITY_STORE_KEY.to_vec(), identity.encode());
        map.insert(
            authority_replication_key(&stored.id),
            encode_authority_replication_policy(&stored),
        );
        self.write_control_map(map)?;
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
        let mut map = self.control_map()?;
        map.insert(ACL_STORE_KEY.to_vec(), acl.encode());
        let seq = append_audit_record(&mut map, self.digest_algo, principal, action, target)?;
        self.write_control_map(map)?;
        Ok(seq)
    }

    pub fn audit_append(
        &self,
        principal: Option<WorkspaceId>,
        action: &str,
        target: Option<&str>,
    ) -> Result<u64> {
        let mut map = self.control_map()?;
        let seq = append_audit_record(&mut map, self.digest_algo, principal, action, target)?;
        self.write_control_map(map)?;
        Ok(seq)
    }

    pub fn audit_config(&self) -> Result<AuditConfig> {
        self.control_get(AUDIT_CONFIG_KEY)?
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
        let mut map = self.control_map()?;
        map.insert(STORE_POLICY_KEY.to_vec(), encode_store_policy(policy));
        let seq = append_audit_record(&mut map, self.digest_algo, principal, action, target)?;
        self.write_control_map(map)?;
        Ok(seq)
    }

    pub fn save_audit_config_audited(
        &self,
        config: AuditConfig,
        principal: Option<WorkspaceId>,
        action: &str,
        target: Option<&str>,
    ) -> Result<u64> {
        let mut map = self.control_map()?;
        map.insert(AUDIT_CONFIG_KEY.to_vec(), encode_audit_config(config));
        let seq = append_audit_record(&mut map, self.digest_algo, principal, action, target)?;
        self.write_control_map(map)?;
        Ok(seq)
    }

    pub fn audit_records(&self) -> Result<Vec<AuditRecord>> {
        let map = self.control_map()?;
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
        let mut map = self.control_map()?;
        let config = map
            .get(AUDIT_CONFIG_KEY)
            .map(|bytes| decode_audit_config(bytes))
            .transpose()?
            .unwrap_or_default();
        if config.legal_hold {
            return Err(LoomError::new(
                Code::PermissionDenied,
                "audit legal hold prevents pruning",
            ));
        }
        let records = map
            .iter()
            .filter(|(key, _)| key.starts_with(AUDIT_ENTRY_PREFIX))
            .map(|(key, value)| decode_audit_entry(key, value, self.digest_algo))
            .collect::<Result<Vec<_>>>()?;
        let checkpoint = records
            .iter()
            .filter(|record| record.seq <= through_seq)
            .max_by_key(|record| record.seq)
            .map(|record| AuditCheckpoint {
                seq: record.seq,
                hash: record.hash,
            });
        let Some(checkpoint) = checkpoint else {
            let target = format!("through_seq={through_seq};pruned=0");
            let audit_seq = append_audit_record(
                &mut map,
                self.digest_algo,
                principal,
                "audit.prune",
                Some(&target),
            )?;
            self.write_control_map(map)?;
            return Ok(AuditPruneStats {
                pruned: 0,
                checkpoint_seq: None,
                checkpoint_hash: None,
                audit_seq,
            });
        };
        let mut pruned = 0u64;
        for record in &records {
            if record.seq <= checkpoint.seq {
                map.remove(&audit_entry_key(record.seq));
                pruned += 1;
            }
        }
        map.insert(
            AUDIT_PRUNE_CHECKPOINT_KEY.to_vec(),
            encode_audit_checkpoint(checkpoint),
        );
        let target = format!("through_seq={};pruned={pruned}", checkpoint.seq);
        let audit_seq = append_audit_record(
            &mut map,
            self.digest_algo,
            principal,
            "audit.prune",
            Some(&target),
        )?;
        self.write_control_map(map)?;
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
        let mut map = self.control_map()?;
        let key = certificate_bundle_key(&record.name);
        let existing = map
            .get(&key)
            .map(|value| decode_certificate_bundle(value, self.digest_algo))
            .transpose()?;
        let seq = append_audit_record(&mut map, self.digest_algo, principal, action, target)?;
        let mut stored = record.clone();
        stored.schema_version = CERTIFICATE_BUNDLE_SCHEMA_VERSION;
        stored.created_audit_seq = existing
            .as_ref()
            .and_then(|value| value.created_audit_seq)
            .or(Some(seq));
        stored.updated_audit_seq = Some(seq);
        stored.unencrypted_private_key_override =
            !self.is_encrypted() && force_unencrypted_private_key;
        map.insert(key, encode_certificate_bundle(&stored));
        self.write_control_map(map)?;
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
        let mut map = self.control_map()?;
        let key = certificate_bundle_key(name);
        if map.remove(&key).is_none() {
            return Err(LoomError::not_found("certificate bundle not found"));
        }
        let seq = append_audit_record(&mut map, self.digest_algo, principal, action, target)?;
        self.write_control_map(map)?;
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
        let mut map = self.control_map()?;
        let key = network_access_policy_key(&record.name);
        let existing = map
            .get(&key)
            .map(|value| decode_network_access_policy(value))
            .transpose()?;
        let seq = append_audit_record(&mut map, self.digest_algo, principal, action, target)?;
        let mut stored = record.clone();
        stored.schema_version = NETWORK_ACCESS_POLICY_SCHEMA_VERSION;
        stored.created_audit_seq = existing
            .as_ref()
            .and_then(|value| value.created_audit_seq)
            .or(Some(seq));
        stored.updated_audit_seq = Some(seq);
        map.insert(key, encode_network_access_policy(&stored));
        self.write_control_map(map)?;
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
        let mut map = self.control_map()?;
        let key = network_access_policy_key(name);
        if map.remove(&key).is_none() {
            return Err(LoomError::not_found("network access policy not found"));
        }
        let seq = append_audit_record(&mut map, self.digest_algo, principal, action, target)?;
        self.write_control_map(map)?;
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
        let mut map = self.control_map()?;
        let seq = append_audit_record(&mut map, self.digest_algo, principal, action, target)?;
        let mut stored = record.clone();
        stored.schema_version = SERVED_LISTENER_SCHEMA_VERSION;
        stored.last_modified_audit_seq = Some(seq);
        map.insert(
            served_listener_key(&stored.id),
            encode_served_listener(&stored),
        );
        self.write_control_map(map)?;
        Ok(seq)
    }

    pub fn remove_served_listener_audited(
        &self,
        id: &str,
        principal: Option<WorkspaceId>,
        action: &str,
        target: Option<&str>,
    ) -> Result<u64> {
        let mut map = self.control_map()?;
        let key = served_listener_key(id);
        if map.remove(&key).is_none() {
            return Err(LoomError::not_found("served listener not found"));
        }
        let seq = append_audit_record(&mut map, self.digest_algo, principal, action, target)?;
        self.write_control_map(map)?;
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
        let mut map = self.control_map()?;
        let seq = append_audit_record(&mut map, self.digest_algo, principal, action, target)?;
        let mut stored = policy.clone();
        stored.schema_version = AUTHORITY_REPLICATION_SCHEMA_VERSION;
        stored.last_modified_audit_seq = Some(seq);
        map.insert(
            authority_replication_key(&stored.id),
            encode_authority_replication_policy(&stored),
        );
        self.write_control_map(map)?;
        Ok(seq)
    }

    pub fn remove_authority_replication_policy_audited(
        &self,
        id: &str,
        principal: Option<WorkspaceId>,
        action: &str,
        target: Option<&str>,
    ) -> Result<u64> {
        let mut map = self.control_map()?;
        let key = authority_replication_key(id);
        if map.remove(&key).is_none() {
            return Err(LoomError::not_found(
                "authority replication policy not found",
            ));
        }
        let seq = append_audit_record(&mut map, self.digest_algo, principal, action, target)?;
        self.write_control_map(map)?;
        Ok(seq)
    }

    fn control_map(&self) -> Result<BTreeMap<Vec<u8>, Vec<u8>>> {
        let Some(root) = self.control_root() else {
            return Ok(BTreeMap::new());
        };
        let bytes = self
            .get(&root)?
            .ok_or_else(|| corrupt("control-plane root object missing"))?;
        decode_control_map(&bytes)
    }

    fn write_control_map(&self, map: BTreeMap<Vec<u8>, Vec<u8>>) -> Result<()> {
        if map.is_empty() {
            return self.set_control_root(None);
        }
        let bytes = encode_control_map(&map);
        let digest = Digest::hash(self.digest_algo, &bytes);
        let codec = self.default_codec;
        self.commit_txn(
            &[(digest, bytes.as_slice(), codec)],
            None,
            Some(Some(*digest.bytes())),
            None,
        )
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
        let to_append = items
            .iter()
            .map(|(digest, canonical)| (*digest, canonical.as_slice(), self.default_codec))
            .collect::<Vec<_>>();
        self.commit_txn(&to_append, Some(Some(*root.bytes())), None, None)
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
        let mut map = self.control_map()?;
        map.insert(key.to_vec(), value);
        if let Some(action) = action {
            append_audit_record(&mut map, self.digest_algo, principal, action, target)?;
        }

        let mut to_append = items
            .iter()
            .map(|(digest, canonical)| (*digest, canonical.as_slice(), self.default_codec))
            .collect::<Vec<_>>();
        let control_bytes = if map.is_empty() {
            None
        } else {
            let bytes = encode_control_map(&map);
            let digest = Digest::hash(self.digest_algo, &bytes);
            Some((digest, bytes))
        };
        let control = if let Some((digest, bytes)) = &control_bytes {
            to_append.push((*digest, bytes.as_slice(), self.default_codec));
            Some(*digest.bytes())
        } else {
            None
        };
        self.commit_txn(
            &to_append,
            Some(reference_root.map(|d| *d.bytes())),
            Some(control),
            None,
        )
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
        let control = control_override.unwrap_or_else(|| inner.control_root.map(|d| *d.bytes()));

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
        let (roots, index_root, placements) = {
            let mut file = self.file.lock().map_err(|_| poisoned())?;
            // Seed the allocator with a clone of the free list, so a failed commit (an early `?`) leaves
            // `inner.free` untouched; the updated list is published only on success.
            let mut alloc = PageAllocator::new(inner.page_count, new_gen, inner.free.clone());
            // Seal object frames under the unlocked DEK if this store is encrypted. The lock is taken
            // only around the record write (not across any `get`), so compaction's read-then-rewrite
            // path cannot deadlock on it.
            let dek = self.dek.lock().map_err(|_| poisoned())?;
            let placements = write_record_pages(&mut **file, &mut alloc, &fresh, dek.as_ref())?;
            drop(dek);
            let touched_segments: BTreeSet<u64> =
                placements.iter().map(|(_, loc)| loc.segment_id).collect();
            let mut index_root = inner.index_root;
            for (key, loc) in &placements {
                // Read bound = the live cursor: existing committed index nodes are all below it.
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
            let object_count = inner
                .maintenance
                .object_count
                .saturating_add(fresh.len() as u64);
            let roots = finish_txn(
                &mut **file,
                &mut alloc,
                new_gen,
                object_count,
                index_root,
                inner.overlay_root,
                inner.open_segment,
                reference,
                control,
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
            (roots, index_root, placements)
        };

        inner.generation = new_gen;
        inner.page_count = roots.page_count;
        inner.index_root = index_root;
        inner.overlay_root = roots.overlay_root;
        Self::clear_index_page_cache_locked(&mut inner);
        inner.free = roots.free;
        inner.freemap = roots.freemap;
        inner.region_table_root = Some(roots.region_table_root);
        inner.maintenance_root = Some(roots.maintenance_root);
        inner.maintenance = roots.maintenance;
        for (key, loc) in placements {
            Self::cache_locator_locked(&mut inner, key, loc);
        }
        if let Some(h) = reference_override {
            inner.reference_root = h.map(|bytes| Digest::of(self.digest_algo, bytes));
        }
        if let Some(h) = control_override {
            inner.control_root = h.map(|bytes| Digest::of(self.digest_algo, bytes));
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
    } else if record::decode_chunked_blob_page(&buf).is_some() {
        "stale_record_chunked_page".to_string()
    } else if pagebtree::looks_like_node_page(&buf) {
        "stale_tree_page".to_string()
    } else if page::RegionTable::decode(&buf).is_some() {
        "stale_region_table_page".to_string()
    } else if maintenance::looks_like_maintenance_page(&buf) {
        "stale_maintenance_page".to_string()
    } else if pagemap::decode(&buf).is_some() {
        "stale_free_map_page".to_string()
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

// ---- compaction / GC FileStore impl lives in compact.rs ----
mod compact;

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
    if let Some(description) = &record.description {
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
        if let Some(description) = &rule.description {
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
        facet_durability_overrides: [None; 21],
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
    let description = decode_optional_served_listener_string(
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
            description: decode_optional_served_listener_string(
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
        // One object joins the group-commit queue: concurrent puts coalesce into one fsync, and a
        // repeat or already-stored object is deduped under the lock (the reference root is preserved).
        self.group_commit(&[(digest, canonical, self.default_codec)])?;
        Ok(digest)
    }

    fn put_hint(&self, canonical: &[u8], hint: CompressionHint) -> Result<Digest> {
        let digest = Digest::hash(self.digest_algo, canonical);
        // The workspace's hint picks the codec for this object; guardrails still apply (frame.rs).
        self.group_commit(&[(digest, canonical, frame::codec_for_hint(hint))])?;
        Ok(digest)
    }

    fn get(&self, digest: &Digest) -> Result<Option<Vec<u8>>> {
        let (loc, page_count) = {
            let mut inner = self.inner.lock().map_err(|_| poisoned())?;
            match self.lookup_loc_locked(&mut inner, digest.bytes())? {
                Some(loc) => (loc, inner.page_count),
                None => return Ok(None),
            }
        };
        let global = loc.global_page();
        if global >= page_count {
            return Err(corrupt("record locator past the page array"));
        }
        let mut file = self.file.lock().map_err(|_| poisoned())?;
        // Acquired after `file` to keep a single global order (file before dek) shared with the write
        // paths, so a reader decrypting and a writer sealing can never deadlock. An AEAD frame read with
        // the store locked surfaces `E2eLocked` from `decode_record`.
        let dek = self.dek.lock().map_err(|_| poisoned())?;
        let mut first = [0u8; PAGE_SIZE as usize];
        read_exact_at(&mut **file, PageId(global).offset(DATA_START), &mut first)
            .map_err(io_err)?;
        // The record's framed bytes come either from a slot in a shared slab page or from a large
        // page run; either way `decode_record` re-verifies the digest below the content boundary.
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
                let rec = record_io::read_chunked_blob(&mut **file, global, page_count)?;
                decode_record(&rec, digest, dek.as_ref(), self.digest_algo)?
            }
            _ => return Err(corrupt("bad record page magic on read")),
        };
        Ok(Some(payload))
    }

    fn has(&self, digest: &Digest) -> Result<bool> {
        let mut inner = self.inner.lock().map_err(|_| poisoned())?;
        Ok(self
            .lookup_loc_locked(&mut inner, digest.bytes())?
            .is_some())
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
        FileStore::mutable_overlay_entries(self)
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
        loom.load_state(root)?;
    }
    let entries = loom.store().mutable_overlay_entries()?;
    *loom.mutable_overlay_mut() = loom_core::MutableOverlay::import_entries(&entries)?;
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
    Ok(loom)
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

// ---- superblock (struct + impl) lives in superblock.rs ----
mod superblock;
pub(crate) use superblock::*;

// ---- helpers -----------------------------------------------------------------------------------

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
/// Read and decode the region-table page, bounding the read by `page_count`.
fn read_region_table(file: &mut dyn BackingIo, rt: PageId, page_count: u64) -> Result<RegionTable> {
    if rt.0 >= page_count {
        return Err(corrupt("region table page out of range"));
    }
    let mut buf = [0u8; PAGE_SIZE as usize];
    read_exact_at(file, rt.offset(DATA_START), &mut buf).map_err(io_err)?;
    RegionTable::decode(&buf).ok_or_else(|| corrupt("region table parse failure"))
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
    addresses: impl Iterator<Item = [u8; 32]>,
) -> Result<Vec<([u8; 32], RecordLoc)>> {
    let mut current = Vec::new();
    for address in addresses {
        if let Some(loc) = pagebtree::get(file, DATA_START, root, &address, page_count)? {
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
    let replacements = replacements.collect::<BTreeMap<_, _>>();
    if replacements.is_empty() {
        return Ok(BTreeSet::new());
    }
    let mut touched_segments = BTreeSet::new();
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
    free_overlay_record_pages_batch(file, alloc, &eligible_locs, &mut touched_segments)?;
    Ok(touched_segments)
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
            alloc.free(PageId(start), 1);
            touched_segments.insert(start / page::PAGES_PER_SEGMENT);
        }
        return Ok(());
    }
    if first[0] == record::CHUNKED_BLOB_MAGIC {
        for page in record_io::chunked_blob_pages(file, start, alloc.page_count())? {
            alloc.free(PageId(page), 1);
            touched_segments.insert(page / page::PAGES_PER_SEGMENT);
        }
        return Ok(());
    }
    let span = record_io::page_span(file, start)?;
    alloc.free(PageId(start), span);
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
                alloc.free(PageId(page), 1);
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

fn encode_mutable_overlay_meta(generation: u64) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"loom.store.mutable-overlay.meta.v1");
    put_uvarint(&mut out, generation);
    out
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

fn mutable_overlay_current_root_record(current_root: Option<PageId>) -> ([u8; 32], Vec<u8>) {
    (
        mutable_overlay_current_root_address(),
        encode_mutable_overlay_current_root_record(current_root),
    )
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

type MutableOverlayRecordRef<'a> = ([u8; 32], &'a [u8]);

fn split_mutable_overlay_records(
    records: &[([u8; 32], Vec<u8>)],
) -> (
    Vec<MutableOverlayRecordRef<'_>>,
    Vec<MutableOverlayRecordRef<'_>>,
) {
    let mut current = Vec::new();
    let mut control = Vec::new();
    for (address, value) in records {
        if is_mutable_overlay_current_entry_record(value) {
            current.push((*address, value.as_slice()));
        } else {
            control.push((*address, value.as_slice()));
        }
    }
    (current, control)
}

fn is_mutable_overlay_current_entry_record(value: &[u8]) -> bool {
    value.starts_with(b"loom.store.mutable-overlay.entry.v1")
        || value.starts_with(b"loom.store.mutable-overlay.entry.v2")
        || value.starts_with(b"loom.store.mutable-overlay.entry.v3")
}

fn is_mutable_overlay_control_record_family(value: &[u8]) -> bool {
    value.starts_with(MUTABLE_OVERLAY_OWNER_TOKEN_RECORD)
        || value.starts_with(MUTABLE_OVERLAY_SECONDARY_INDEX_RECORD)
        || value.starts_with(MUTABLE_OVERLAY_IDEMPOTENCY_RECORD)
        || value.starts_with(MUTABLE_OVERLAY_TRANSACTION_IDEMPOTENCY_RECORD)
        || value.starts_with(RETAINED_HISTORY_HEAD_RECORD)
        || value.starts_with(RETAINED_HISTORY_ENTRY_RECORD)
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
) -> Vec<u8> {
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
    out
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
    let generation = loom_core::OverlayGeneration::new(
        get_uvarint(bytes, &mut pos)
            .ok_or_else(|| corrupt("workflow transaction idempotency generation truncated"))?,
    );
    let root_end = pos
        .checked_add(32)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| corrupt("workflow transaction idempotency root truncated"))?;
    let root_after = Digest::of(
        Algo::Blake3,
        bytes[pos..root_end]
            .try_into()
            .map_err(|_| corrupt("workflow transaction idempotency root invalid"))?,
    );
    pos = root_end;
    let write_count = get_uvarint(bytes, &mut pos)
        .ok_or_else(|| corrupt("workflow transaction idempotency write count truncated"))?
        as usize;
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
        pos += 1;
        let key_len = get_uvarint(bytes, &mut pos)
            .ok_or_else(|| corrupt("workflow transaction idempotency key length truncated"))?
            as usize;
        let key_end = pos
            .checked_add(key_len)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| corrupt("workflow transaction idempotency key truncated"))?;
        let target = loom_core::OverlayKey::from_encoded_bytes(bytes[pos..key_end].to_vec())?;
        pos = key_end;
        let token_end = pos
            .checked_add(32)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| corrupt("workflow transaction idempotency token truncated"))?;
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
        pos += 1;
        writes.push(WriteOutcome {
            facet,
            target,
            owner_token,
            change,
        });
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
mod tests;
