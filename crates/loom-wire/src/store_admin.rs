//! Canonical wire codecs for the `StoreAdmin` control plane (`specs/0067` §13, task 640).
//!
//! `StoreAdmin` is the server-owned store-administration surface: `store_stat` (read), `store_policy_get`
//! (read), `store_policy_set` (audited), and `store_rekey` (server-side crypto, audited). Each method
//! returns canonical CBOR encoded here; rekey accepts credential material while keeping derived wrapping
//! keys and data-encryption keys server-side. Malformed input is `INVALID_ARGUMENT`.

use loom_codec::{Value as CborValue, decode, encode};
use loom_core::keys::KEY_LEN;
use loom_core::{FacetKind, OverlayDurabilityPolicy};
use loom_types::{Code, LoomError};

fn enc(value: CborValue) -> Vec<u8> {
    encode(&value).expect("canonical cbor encode of store-admin result never fails")
}

fn arr(bytes: &[u8]) -> Result<Vec<CborValue>, LoomError> {
    match decode(bytes)
        .map_err(|err| LoomError::new(Code::InvalidArgument, format!("store-admin cbor: {err}")))?
    {
        CborValue::Array(items) => Ok(items),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            "store-admin result must be a CBOR array",
        )),
    }
}

fn exact(items: &[CborValue], len: usize, label: &str) -> Result<(), LoomError> {
    if items.len() == len {
        Ok(())
    } else {
        Err(LoomError::new(
            Code::InvalidArgument,
            format!("{label} must have {len} fields"),
        ))
    }
}

fn uint(items: &[CborValue], i: usize) -> Result<u64, LoomError> {
    match items.get(i) {
        Some(CborValue::Uint(n)) => Ok(*n),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            "store-admin field must be an unsigned integer",
        )),
    }
}

fn opt_uint(items: &[CborValue], i: usize) -> Result<Option<u64>, LoomError> {
    match items.get(i) {
        Some(CborValue::Uint(n)) => Ok(Some(*n)),
        Some(CborValue::Null) => Ok(None),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            "store-admin optional field must be an unsigned integer or null",
        )),
    }
}

fn opt_bool(items: &[CborValue], i: usize) -> Result<Option<bool>, LoomError> {
    match items.get(i) {
        Some(CborValue::Bool(b)) => Ok(Some(*b)),
        Some(CborValue::Null) => Ok(None),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            "store-admin optional field must be a boolean or null",
        )),
    }
}

fn boolean(items: &[CborValue], i: usize) -> Result<bool, LoomError> {
    match items.get(i) {
        Some(CborValue::Bool(b)) => Ok(*b),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            "store-admin field must be a boolean",
        )),
    }
}

fn text(items: &[CborValue], i: usize) -> Result<String, LoomError> {
    match items.get(i) {
        Some(CborValue::Text(s)) => Ok(s.clone()),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            "store-admin field must be text",
        )),
    }
}

fn text_vec(items: &[CborValue], i: usize) -> Result<Vec<String>, LoomError> {
    match items.get(i) {
        Some(CborValue::Array(values)) => values
            .iter()
            .map(|value| match value {
                CborValue::Text(text) => Ok(text.clone()),
                _ => Err(LoomError::new(
                    Code::InvalidArgument,
                    "store-admin text list item must be text",
                )),
            })
            .collect(),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            "store-admin field must be a text list",
        )),
    }
}

fn value_array<'a>(value: &'a CborValue, label: &str) -> Result<&'a [CborValue], LoomError> {
    match value {
        CborValue::Array(items) => Ok(items),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            format!("{label} must be a CBOR array"),
        )),
    }
}

fn value_uint(value: &CborValue, label: &str) -> Result<u64, LoomError> {
    match value {
        CborValue::Uint(value) => Ok(*value),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            format!("{label} must be an unsigned integer"),
        )),
    }
}

fn value_opt_uint(value: &CborValue, label: &str) -> Result<Option<u64>, LoomError> {
    match value {
        CborValue::Uint(value) => Ok(Some(*value)),
        CborValue::Null => Ok(None),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            format!("{label} must be an unsigned integer or null"),
        )),
    }
}

fn value_bool(value: &CborValue, label: &str) -> Result<bool, LoomError> {
    match value {
        CborValue::Bool(value) => Ok(*value),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            format!("{label} must be a boolean"),
        )),
    }
}

fn value_opt_bool(value: &CborValue, label: &str) -> Result<Option<bool>, LoomError> {
    match value {
        CborValue::Bool(value) => Ok(Some(*value)),
        CborValue::Null => Ok(None),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            format!("{label} must be a boolean or null"),
        )),
    }
}

fn value_text(value: &CborValue, label: &str) -> Result<String, LoomError> {
    match value {
        CborValue::Text(value) => Ok(value.clone()),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            format!("{label} must be text"),
        )),
    }
}

fn value_bytes(value: &CborValue, label: &str) -> Result<Vec<u8>, LoomError> {
    match value {
        CborValue::Bytes(value) => Ok(value.clone()),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            format!("{label} must be bytes"),
        )),
    }
}

fn value_opt_text(value: &CborValue, label: &str) -> Result<Option<String>, LoomError> {
    match value {
        CborValue::Text(value) => Ok(Some(value.clone())),
        CborValue::Null => Ok(None),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            format!("{label} must be text or null"),
        )),
    }
}

fn facet_to_value(facet: FacetKind) -> CborValue {
    CborValue::Uint(u64::from(facet.stable_tag()))
}

fn facet_from_value(value: &CborValue, label: &str) -> Result<FacetKind, LoomError> {
    let tag = match value {
        CborValue::Uint(tag) => u8::try_from(*tag).map_err(|_| {
            LoomError::new(
                Code::InvalidArgument,
                format!("{label} facet tag out of range"),
            )
        })?,
        _ => {
            return Err(LoomError::new(
                Code::InvalidArgument,
                format!("{label} facet must be an unsigned integer"),
            ));
        }
    };
    FacetKind::from_stable_tag(tag).ok_or_else(|| {
        LoomError::new(
            Code::InvalidArgument,
            format!("{label} facet tag is unknown"),
        )
    })
}

fn durability_to_value(policy: OverlayDurabilityPolicy) -> CborValue {
    CborValue::Text(policy.as_str().to_string())
}

fn durability_from_value(
    value: &CborValue,
    label: &str,
) -> Result<OverlayDurabilityPolicy, LoomError> {
    let text = value_text(value, label)?;
    OverlayDurabilityPolicy::parse(&text).map_err(|err| LoomError::new(err.code, err.message))
}

fn facet_assignment_to_value(assignment: &StoreFacetDurabilityAssignment) -> CborValue {
    CborValue::Array(vec![
        facet_to_value(assignment.facet),
        durability_to_value(assignment.durability),
    ])
}

fn facet_assignment_from_value(
    value: &CborValue,
    label: &str,
) -> Result<StoreFacetDurabilityAssignment, LoomError> {
    let items = value_array(value, label)?;
    exact(items, 2, label)?;
    Ok(StoreFacetDurabilityAssignment {
        facet: facet_from_value(&items[0], label)?,
        durability: durability_from_value(&items[1], label)?,
    })
}

fn facet_assignment_vec(
    value: &CborValue,
    label: &str,
) -> Result<Vec<StoreFacetDurabilityAssignment>, LoomError> {
    match value {
        CborValue::Array(items) => items
            .iter()
            .map(|item| facet_assignment_from_value(item, label))
            .collect(),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            format!("{label} must be a facet assignment list"),
        )),
    }
}

fn facet_vec(value: &CborValue, label: &str) -> Result<Vec<FacetKind>, LoomError> {
    match value {
        CborValue::Array(items) => items
            .iter()
            .map(|item| facet_from_value(item, label))
            .collect(),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            format!("{label} must be a facet list"),
        )),
    }
}

fn value_uint_vec(value: &CborValue, label: &str) -> Result<Vec<u64>, LoomError> {
    value_array(value, label)?
        .iter()
        .map(|item| value_uint(item, label))
        .collect()
}

fn value_text_vec(value: &CborValue, label: &str) -> Result<Vec<String>, LoomError> {
    value_array(value, label)?
        .iter()
        .map(|item| value_text(item, label))
        .collect()
}

/// The store maintenance/size snapshot returned by `store_stat` (`loom.store.stat.v1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreStat {
    pub object_count: u64,
    pub generation: u64,
    pub physical_page_count: u64,
    pub physical_bytes: u64,
    pub reusable_free_pages: u64,
    pub candidate_dead_pages: u64,
    pub last_validated_mark_epoch: u64,
    pub touched_segments: u64,
    pub candidate_segments: u64,
    pub segment_overflow: u64,
}

/// Encode a [`StoreStat`] as the canonical CBOR array (10 unsigned integers, field order below).
pub fn store_stat_to_cbor(stat: &StoreStat) -> Vec<u8> {
    enc(CborValue::Array(vec![
        CborValue::Uint(stat.object_count),
        CborValue::Uint(stat.generation),
        CborValue::Uint(stat.physical_page_count),
        CborValue::Uint(stat.physical_bytes),
        CborValue::Uint(stat.reusable_free_pages),
        CborValue::Uint(stat.candidate_dead_pages),
        CborValue::Uint(stat.last_validated_mark_epoch),
        CborValue::Uint(stat.touched_segments),
        CborValue::Uint(stat.candidate_segments),
        CborValue::Uint(stat.segment_overflow),
    ]))
}

/// Decode a [`StoreStat`] CBOR array.
pub fn store_stat_from_cbor(bytes: &[u8]) -> Result<StoreStat, LoomError> {
    let items = arr(bytes)?;
    exact(&items, 10, "store stat")?;
    Ok(StoreStat {
        object_count: uint(&items, 0)?,
        generation: uint(&items, 1)?,
        physical_page_count: uint(&items, 2)?,
        physical_bytes: uint(&items, 3)?,
        reusable_free_pages: uint(&items, 4)?,
        candidate_dead_pages: uint(&items, 5)?,
        last_validated_mark_epoch: uint(&items, 6)?,
        touched_segments: uint(&items, 7)?,
        candidate_segments: uint(&items, 8)?,
        segment_overflow: uint(&items, 9)?,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreFacetDurabilityAssignment {
    pub facet: FacetKind,
    pub durability: OverlayDurabilityPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorePolicyUpdate {
    pub fips_required: Option<bool>,
    pub default_durability: Option<OverlayDurabilityPolicy>,
    pub facet_durability_assignments: Vec<StoreFacetDurabilityAssignment>,
    pub clear_facet_durability: Vec<FacetKind>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreRekeyCredential {
    Passphrase(Vec<u8>),
    RawKek([u8; KEY_LEN]),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreRekeyRequest {
    pub credential: StoreRekeyCredential,
    pub reseal: bool,
    pub suite: Option<String>,
}

/// The result of `store_policy_get`/`store_policy_set` (`loom.store.policy.v1`). `audit_seq` is present
/// after a `set` (the audit sequence assigned to the mutation) and absent for a `get`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorePolicyResult {
    pub fips_required: bool,
    pub default_durability: OverlayDurabilityPolicy,
    pub facet_durability_overrides: Vec<StoreFacetDurabilityAssignment>,
    pub audit_seq: Option<u64>,
}

pub fn store_policy_update_to_cbor(update: &StorePolicyUpdate) -> Vec<u8> {
    enc(CborValue::Array(vec![
        update
            .fips_required
            .map_or(CborValue::Null, CborValue::Bool),
        update
            .default_durability
            .map_or(CborValue::Null, durability_to_value),
        CborValue::Array(
            update
                .facet_durability_assignments
                .iter()
                .map(facet_assignment_to_value)
                .collect(),
        ),
        CborValue::Array(
            update
                .clear_facet_durability
                .iter()
                .copied()
                .map(facet_to_value)
                .collect(),
        ),
    ]))
}

pub fn store_policy_update_from_cbor(bytes: &[u8]) -> Result<StorePolicyUpdate, LoomError> {
    let items = arr(bytes)?;
    exact(&items, 4, "store policy update")?;
    Ok(StorePolicyUpdate {
        fips_required: opt_bool(&items, 0)?,
        default_durability: match &items[1] {
            CborValue::Null => None,
            value => Some(durability_from_value(
                value,
                "store policy default durability",
            )?),
        },
        facet_durability_assignments: facet_assignment_vec(
            &items[2],
            "store policy facet assignments",
        )?,
        clear_facet_durability: facet_vec(&items[3], "store policy facet removals")?,
    })
}

pub fn store_rekey_request_to_cbor(request: &StoreRekeyRequest) -> Vec<u8> {
    let credential = match &request.credential {
        StoreRekeyCredential::Passphrase(secret) => CborValue::Array(vec![
            CborValue::Text("passphrase".to_string()),
            CborValue::Bytes(secret.clone()),
        ]),
        StoreRekeyCredential::RawKek(secret) => CborValue::Array(vec![
            CborValue::Text("raw_kek".to_string()),
            CborValue::Bytes(secret.to_vec()),
        ]),
    };
    enc(CborValue::Array(vec![
        credential,
        CborValue::Bool(request.reseal),
        request
            .suite
            .clone()
            .map_or(CborValue::Null, CborValue::Text),
    ]))
}

pub fn store_rekey_request_from_cbor(bytes: &[u8]) -> Result<StoreRekeyRequest, LoomError> {
    let items = arr(bytes)?;
    exact(&items, 3, "store rekey request")?;
    let credential_items = value_array(&items[0], "store rekey credential")?;
    exact(credential_items, 2, "store rekey credential")?;
    let secret = value_bytes(&credential_items[1], "store rekey credential secret")?;
    let credential = match value_text(&credential_items[0], "store rekey credential kind")?.as_str()
    {
        "passphrase" => StoreRekeyCredential::Passphrase(secret),
        "raw_kek" => {
            let raw = <[u8; KEY_LEN]>::try_from(secret.as_slice()).map_err(|_| {
                LoomError::new(
                    Code::InvalidArgument,
                    "store rekey raw KEK must be exactly 32 bytes",
                )
            })?;
            StoreRekeyCredential::RawKek(raw)
        }
        _ => {
            return Err(LoomError::new(
                Code::InvalidArgument,
                "store rekey credential kind is unknown",
            ));
        }
    };
    Ok(StoreRekeyRequest {
        credential,
        reseal: boolean(&items, 1)?,
        suite: value_opt_text(&items[2], "store rekey suite")?,
    })
}

/// Encode a [`StorePolicyResult`] as canonical CBOR
/// `[fips_required, default_durability, facet_overrides, audit_seq|null]`.
pub fn store_policy_result_to_cbor(result: &StorePolicyResult) -> Vec<u8> {
    enc(CborValue::Array(vec![
        CborValue::Bool(result.fips_required),
        durability_to_value(result.default_durability),
        CborValue::Array(
            result
                .facet_durability_overrides
                .iter()
                .map(facet_assignment_to_value)
                .collect(),
        ),
        result.audit_seq.map_or(CborValue::Null, CborValue::Uint),
    ]))
}

/// Decode a [`StorePolicyResult`] CBOR array.
pub fn store_policy_result_from_cbor(bytes: &[u8]) -> Result<StorePolicyResult, LoomError> {
    let items = arr(bytes)?;
    exact(&items, 4, "store policy result")?;
    Ok(StorePolicyResult {
        fips_required: boolean(&items, 0)?,
        default_durability: durability_from_value(&items[1], "store policy default durability")?,
        facet_durability_overrides: facet_assignment_vec(&items[2], "store policy overrides")?,
        audit_seq: opt_uint(&items, 3)?,
    })
}

/// The result of `store_rekey` (`loom.store.rekey.v1`): the audit sequence, whether every object was
/// re-sealed under a fresh DEK, the active AEAD suite, and the reseal byte deltas (present only for a
/// reseal). No key material is ever included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreRekeyResult {
    pub audit_seq: u64,
    pub resealed: bool,
    pub suite: String,
    pub bytes_before: Option<u64>,
    pub bytes_after: Option<u64>,
}

/// Encode a [`StoreRekeyResult`] as canonical CBOR `[audit_seq, resealed, suite, before|null, after|null]`.
pub fn store_rekey_result_to_cbor(result: &StoreRekeyResult) -> Vec<u8> {
    enc(CborValue::Array(vec![
        CborValue::Uint(result.audit_seq),
        CborValue::Bool(result.resealed),
        CborValue::Text(result.suite.clone()),
        result.bytes_before.map_or(CborValue::Null, CborValue::Uint),
        result.bytes_after.map_or(CborValue::Null, CborValue::Uint),
    ]))
}

/// Decode a [`StoreRekeyResult`] CBOR array.
pub fn store_rekey_result_from_cbor(bytes: &[u8]) -> Result<StoreRekeyResult, LoomError> {
    let items = arr(bytes)?;
    exact(&items, 5, "store rekey result")?;
    Ok(StoreRekeyResult {
        audit_seq: uint(&items, 0)?,
        resealed: boolean(&items, 1)?,
        suite: text(&items, 2)?,
        bytes_before: opt_uint(&items, 3)?,
        bytes_after: opt_uint(&items, 4)?,
    })
}

/// The result of `store_bundle_import` (`loom.store.bundle_import.v1`). `dry_run` reports the
/// validation and transfer plan without creating the workspace or refs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreBundleImportResult {
    pub workspace_id: String,
    pub workspace_name: String,
    pub facets: Vec<String>,
    pub objects_transferred: u64,
    pub objects_skipped: u64,
    pub new_tips: Vec<String>,
    pub dry_run: bool,
}

/// Encode a [`StoreBundleImportResult`] as canonical CBOR
/// `[workspace_id, workspace_name, facets, transferred, skipped, new_tips, dry_run]`.
pub fn store_bundle_import_result_to_cbor(result: &StoreBundleImportResult) -> Vec<u8> {
    enc(CborValue::Array(vec![
        CborValue::Text(result.workspace_id.clone()),
        CborValue::Text(result.workspace_name.clone()),
        CborValue::Array(result.facets.iter().cloned().map(CborValue::Text).collect()),
        CborValue::Uint(result.objects_transferred),
        CborValue::Uint(result.objects_skipped),
        CborValue::Array(
            result
                .new_tips
                .iter()
                .cloned()
                .map(CborValue::Text)
                .collect(),
        ),
        CborValue::Bool(result.dry_run),
    ]))
}

/// Decode a [`StoreBundleImportResult`] CBOR array.
pub fn store_bundle_import_result_from_cbor(
    bytes: &[u8],
) -> Result<StoreBundleImportResult, LoomError> {
    let items = arr(bytes)?;
    exact(&items, 7, "store bundle import result")?;
    Ok(StoreBundleImportResult {
        workspace_id: text(&items, 0)?,
        workspace_name: text(&items, 1)?,
        facets: text_vec(&items, 2)?,
        objects_transferred: uint(&items, 3)?,
        objects_skipped: uint(&items, 4)?,
        new_tips: text_vec(&items, 5)?,
        dry_run: boolean(&items, 6)?,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreMaintenancePolicyRecord {
    pub min_candidate_pages: u64,
    pub min_reusable_pages: u64,
    pub interval_ms: u64,
    pub backoff_ms: u64,
    pub max_segments: u64,
    pub max_pages: u64,
    pub full_compaction_enabled: bool,
    pub tail_trim_enabled: bool,
    pub tail_compaction_enabled: bool,
    pub tail_compaction_max_pages: u64,
    pub tail_compaction_max_objects: u64,
    pub tail_compaction_max_bytes: u64,
    pub tail_compaction_interval_ms: u64,
    pub tail_compaction_backoff_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreMaintenanceRunStateRecord {
    pub last_run_ms: Option<u64>,
    pub next_eligible_ms: u64,
    pub last_skip_reason: Option<String>,
    pub last_error: Option<String>,
    pub last_tail_trim_attempted: bool,
    pub last_tail_trim_pages: u64,
    pub last_tail_trim_bytes: u64,
    pub last_tail_compaction_attempted: bool,
    pub last_tail_compaction_relocated_objects: u64,
    pub last_tail_compaction_relocated_pages: u64,
    pub last_tail_compaction_relocated_bytes: u64,
    pub last_tail_compaction_truncated_pages: u64,
    pub last_tail_compaction_conflicts: u64,
    pub last_shrink_skip_reason: Option<String>,
    pub last_progress_steps: u64,
    pub last_yield_count: u64,
    pub last_overrun_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreGroupCommitDiagnosticsRecord {
    pub group_commit_batches_total: u64,
    pub group_commit_transactions_total: u64,
    pub group_commit_records_total: u64,
    pub fsync_total_micros: u64,
    pub fsync_count: u64,
    pub write_lock_wait_total_micros: u64,
    pub write_lock_wait_count: u64,
    pub pending_durable_window_transactions: u64,
    pub pending_durable_window_records: u64,
    pub pinned_reader_blockers: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreMaintenanceStatusRecord {
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
    pub group_commit: StoreGroupCommitDiagnosticsRecord,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreMutableOverlayHealthRecord {
    pub current_generation: u64,
    pub current_record_count: u64,
    pub tombstone_count: u64,
    pub live_checkpoint_references: u64,
    pub reclaimable_overlay_pages: u64,
    pub blocked_reclamation_reasons: Vec<String>,
    pub hot_write_count: u64,
    pub active_writer_contention_indicators: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreMvccSnapshotIdentityRecord {
    pub overlay_generation: u64,
    pub immutable_base_root: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreMvccSnapshotPinRecord {
    pub pin_id: u64,
    pub identity: StoreMvccSnapshotIdentityRecord,
    pub owner: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreMvccSnapshotDiagnosticsRecord {
    pub active_snapshot_count: u64,
    pub oldest_pinned_overlay_generation: Option<u64>,
    pub pins: Vec<StoreMvccSnapshotPinRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreGrowthDomainRecord {
    pub domain: String,
    pub current_records: u64,
    pub obsolete_records: u64,
    pub payload_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreLiveRootExampleRecord {
    pub id: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreLiveRootClassDiagnosticsRecord {
    pub class: String,
    pub count: u64,
    pub examples: Vec<StoreLiveRootExampleRecord>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreLiveRootDiagnosticsRecord {
    pub sample_limit: u64,
    pub classes: Vec<StoreLiveRootClassDiagnosticsRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreMaintenanceReportRecord {
    pub status: StoreMaintenanceStatusRecord,
    pub overlay_health: StoreMutableOverlayHealthRecord,
    pub mvcc_snapshots: StoreMvccSnapshotDiagnosticsRecord,
    pub policy: StoreMaintenancePolicyRecord,
    pub run_state: StoreMaintenanceRunStateRecord,
    pub mark_epoch: Option<u64>,
    pub mark_completed: bool,
    pub marked_live_objects: u64,
    pub marked_live_bytes: u64,
    pub live_bytes: u64,
    pub candidate_reclaimable_bytes: u64,
    pub reusable_free_bytes: u64,
    pub overlay_obsolete_record_count: u64,
    pub overlay_obsolete_page_count: u64,
    pub tail_free_pages: u64,
    pub tail_free_bytes: u64,
    pub tail_trim_eligible: bool,
    pub tail_blocked_by_live_objects: bool,
    pub tail_compaction_eligible: bool,
    pub full_compaction_required_for_shrink: bool,
    pub tail_trim_attempted: bool,
    pub tail_trim_pages: u64,
    pub tail_trim_bytes: u64,
    pub tail_compaction_attempted: bool,
    pub tail_compaction_relocated_objects: u64,
    pub tail_compaction_relocated_pages: u64,
    pub tail_compaction_relocated_bytes: u64,
    pub tail_compaction_truncated_pages: u64,
    pub tail_compaction_conflicts: u64,
    pub last_shrink_skip_reason: Option<String>,
    pub retained_control_roots: u64,
    pub derived_payload_count: u64,
    pub growth_domains: Vec<StoreGrowthDomainRecord>,
    pub eligible: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreMaintenanceStatusRequest {
    pub include_live_root_diagnostics: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreMaintenanceStatusResult {
    pub report: StoreMaintenanceReportRecord,
    pub live_root_diagnostics: Option<StoreLiveRootDiagnosticsRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreMaintenancePolicyUpdate {
    pub min_candidate_pages: Option<u64>,
    pub min_reusable_pages: Option<u64>,
    pub interval_ms: Option<u64>,
    pub backoff_ms: Option<u64>,
    pub max_segments: Option<u64>,
    pub max_pages: Option<u64>,
    pub full_compaction_enabled: Option<bool>,
    pub tail_trim_enabled: Option<bool>,
    pub tail_compaction_enabled: Option<bool>,
    pub tail_compaction_max_pages: Option<u64>,
    pub tail_compaction_max_objects: Option<u64>,
    pub tail_compaction_max_bytes: Option<u64>,
    pub tail_compaction_interval_ms: Option<u64>,
    pub tail_compaction_backoff_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreMaintenanceRunKind {
    Skipped,
    Marked,
    Compacted,
    Reclaimed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreMaintenanceRunRequest {
    pub max_segments: Option<u64>,
    pub max_pages: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreMaintenanceRunResult {
    pub kind: StoreMaintenanceRunKind,
    pub reason: Option<String>,
    pub visited: Option<u64>,
    pub pending: Option<u64>,
    pub before: Option<u64>,
    pub after: Option<u64>,
    pub reclaimed: Option<u64>,
    pub required_temp_bytes: Option<u64>,
    pub available_temp_bytes: Option<u64>,
    pub segments_reclaimed: Option<u64>,
    pub pages_freed: Option<u64>,
    pub tail_trim_pages: Option<u64>,
    pub tail_trim_bytes: Option<u64>,
    pub tail_compaction_attempted: Option<bool>,
    pub tail_compaction_relocated_objects: Option<u64>,
    pub tail_compaction_relocated_pages: Option<u64>,
    pub tail_compaction_truncated_pages: Option<u64>,
    pub objects_relocated: Option<u64>,
    pub objects_dropped: Option<u64>,
    pub elapsed_ms: Option<u64>,
    pub run_state: StoreMaintenanceRunStateRecord,
    pub report: StoreMaintenanceReportRecord,
}

fn policy_to_value(policy: &StoreMaintenancePolicyRecord) -> CborValue {
    CborValue::Array(vec![
        CborValue::Uint(policy.min_candidate_pages),
        CborValue::Uint(policy.min_reusable_pages),
        CborValue::Uint(policy.interval_ms),
        CborValue::Uint(policy.backoff_ms),
        CborValue::Uint(policy.max_segments),
        CborValue::Uint(policy.max_pages),
        CborValue::Bool(policy.full_compaction_enabled),
        CborValue::Bool(policy.tail_trim_enabled),
        CborValue::Bool(policy.tail_compaction_enabled),
        CborValue::Uint(policy.tail_compaction_max_pages),
        CborValue::Uint(policy.tail_compaction_max_objects),
        CborValue::Uint(policy.tail_compaction_max_bytes),
        CborValue::Uint(policy.tail_compaction_interval_ms),
        CborValue::Uint(policy.tail_compaction_backoff_ms),
    ])
}

fn run_state_to_value(state: &StoreMaintenanceRunStateRecord) -> CborValue {
    CborValue::Array(vec![
        state.last_run_ms.map_or(CborValue::Null, CborValue::Uint),
        CborValue::Uint(state.next_eligible_ms),
        state
            .last_skip_reason
            .as_ref()
            .map_or(CborValue::Null, |s| CborValue::Text(s.clone())),
        state
            .last_error
            .as_ref()
            .map_or(CborValue::Null, |s| CborValue::Text(s.clone())),
        CborValue::Bool(state.last_tail_trim_attempted),
        CborValue::Uint(state.last_tail_trim_pages),
        CborValue::Uint(state.last_tail_trim_bytes),
        CborValue::Bool(state.last_tail_compaction_attempted),
        CborValue::Uint(state.last_tail_compaction_relocated_objects),
        CborValue::Uint(state.last_tail_compaction_relocated_pages),
        CborValue::Uint(state.last_tail_compaction_relocated_bytes),
        CborValue::Uint(state.last_tail_compaction_truncated_pages),
        CborValue::Uint(state.last_tail_compaction_conflicts),
        state
            .last_shrink_skip_reason
            .as_ref()
            .map_or(CborValue::Null, |s| CborValue::Text(s.clone())),
        CborValue::Uint(state.last_progress_steps),
        CborValue::Uint(state.last_yield_count),
        CborValue::Uint(state.last_overrun_count),
    ])
}

fn group_commit_to_value(group: &StoreGroupCommitDiagnosticsRecord) -> CborValue {
    CborValue::Array(vec![
        CborValue::Uint(group.group_commit_batches_total),
        CborValue::Uint(group.group_commit_transactions_total),
        CborValue::Uint(group.group_commit_records_total),
        CborValue::Uint(group.fsync_total_micros),
        CborValue::Uint(group.fsync_count),
        CborValue::Uint(group.write_lock_wait_total_micros),
        CborValue::Uint(group.write_lock_wait_count),
        CborValue::Uint(group.pending_durable_window_transactions),
        CborValue::Uint(group.pending_durable_window_records),
        group
            .pinned_reader_blockers
            .map_or(CborValue::Null, CborValue::Uint),
    ])
}

fn status_to_value(status: &StoreMaintenanceStatusRecord) -> CborValue {
    CborValue::Array(vec![
        CborValue::Uint(status.generation),
        CborValue::Uint(status.object_count),
        CborValue::Uint(status.physical_page_count),
        CborValue::Uint(status.physical_bytes),
        CborValue::Uint(status.reusable_free_pages),
        CborValue::Uint(status.candidate_dead_pages),
        CborValue::Uint(status.tail_free_pages),
        CborValue::Uint(status.tail_free_bytes),
        CborValue::Uint(status.last_validated_mark_epoch),
        CborValue::Array(
            status
                .touched_segments
                .iter()
                .copied()
                .map(CborValue::Uint)
                .collect(),
        ),
        CborValue::Array(
            status
                .candidate_segments
                .iter()
                .copied()
                .map(CborValue::Uint)
                .collect(),
        ),
        CborValue::Bool(status.segment_overflow),
        group_commit_to_value(&status.group_commit),
    ])
}

fn overlay_health_to_value(health: &StoreMutableOverlayHealthRecord) -> CborValue {
    CborValue::Array(vec![
        CborValue::Uint(health.current_generation),
        CborValue::Uint(health.current_record_count),
        CborValue::Uint(health.tombstone_count),
        CborValue::Uint(health.live_checkpoint_references),
        CborValue::Uint(health.reclaimable_overlay_pages),
        CborValue::Array(
            health
                .blocked_reclamation_reasons
                .iter()
                .cloned()
                .map(CborValue::Text)
                .collect(),
        ),
        CborValue::Uint(health.hot_write_count),
        CborValue::Uint(health.active_writer_contention_indicators),
    ])
}

fn mvcc_to_value(mvcc: &StoreMvccSnapshotDiagnosticsRecord) -> CborValue {
    CborValue::Array(vec![
        CborValue::Uint(mvcc.active_snapshot_count),
        mvcc.oldest_pinned_overlay_generation
            .map_or(CborValue::Null, CborValue::Uint),
        CborValue::Array(
            mvcc.pins
                .iter()
                .map(|pin| {
                    CborValue::Array(vec![
                        CborValue::Uint(pin.pin_id),
                        CborValue::Array(vec![
                            CborValue::Uint(pin.identity.overlay_generation),
                            pin.identity
                                .immutable_base_root
                                .as_ref()
                                .map_or(CborValue::Null, |digest| CborValue::Text(digest.clone())),
                        ]),
                        pin.owner
                            .as_ref()
                            .map_or(CborValue::Null, |owner| CborValue::Text(owner.clone())),
                    ])
                })
                .collect(),
        ),
    ])
}

fn growth_to_value(growth: &StoreGrowthDomainRecord) -> CborValue {
    CborValue::Array(vec![
        CborValue::Text(growth.domain.clone()),
        CborValue::Uint(growth.current_records),
        CborValue::Uint(growth.obsolete_records),
        CborValue::Uint(growth.payload_bytes),
    ])
}

fn live_roots_to_value(roots: &StoreLiveRootDiagnosticsRecord) -> CborValue {
    CborValue::Array(vec![
        CborValue::Uint(roots.sample_limit),
        CborValue::Array(
            roots
                .classes
                .iter()
                .map(|class| {
                    CborValue::Array(vec![
                        CborValue::Text(class.class.clone()),
                        CborValue::Uint(class.count),
                        CborValue::Array(
                            class
                                .examples
                                .iter()
                                .map(|example| {
                                    CborValue::Array(vec![
                                        CborValue::Text(example.id.clone()),
                                        CborValue::Text(example.digest.clone()),
                                    ])
                                })
                                .collect(),
                        ),
                        CborValue::Bool(class.truncated),
                    ])
                })
                .collect(),
        ),
    ])
}

fn report_to_value(report: &StoreMaintenanceReportRecord) -> CborValue {
    CborValue::Array(vec![
        status_to_value(&report.status),
        overlay_health_to_value(&report.overlay_health),
        mvcc_to_value(&report.mvcc_snapshots),
        policy_to_value(&report.policy),
        run_state_to_value(&report.run_state),
        report.mark_epoch.map_or(CborValue::Null, CborValue::Uint),
        CborValue::Bool(report.mark_completed),
        CborValue::Uint(report.marked_live_objects),
        CborValue::Uint(report.marked_live_bytes),
        CborValue::Uint(report.live_bytes),
        CborValue::Uint(report.candidate_reclaimable_bytes),
        CborValue::Uint(report.reusable_free_bytes),
        CborValue::Uint(report.overlay_obsolete_record_count),
        CborValue::Uint(report.overlay_obsolete_page_count),
        CborValue::Uint(report.tail_free_pages),
        CborValue::Uint(report.tail_free_bytes),
        CborValue::Bool(report.tail_trim_eligible),
        CborValue::Bool(report.tail_blocked_by_live_objects),
        CborValue::Bool(report.tail_compaction_eligible),
        CborValue::Bool(report.full_compaction_required_for_shrink),
        CborValue::Bool(report.tail_trim_attempted),
        CborValue::Uint(report.tail_trim_pages),
        CborValue::Uint(report.tail_trim_bytes),
        CborValue::Bool(report.tail_compaction_attempted),
        CborValue::Uint(report.tail_compaction_relocated_objects),
        CborValue::Uint(report.tail_compaction_relocated_pages),
        CborValue::Uint(report.tail_compaction_relocated_bytes),
        CborValue::Uint(report.tail_compaction_truncated_pages),
        CborValue::Uint(report.tail_compaction_conflicts),
        report
            .last_shrink_skip_reason
            .as_ref()
            .map_or(CborValue::Null, |s| CborValue::Text(s.clone())),
        CborValue::Uint(report.retained_control_roots),
        CborValue::Uint(report.derived_payload_count),
        CborValue::Array(report.growth_domains.iter().map(growth_to_value).collect()),
        CborValue::Bool(report.eligible),
        CborValue::Text(report.reason.clone()),
    ])
}

fn policy_from_value(value: &CborValue) -> Result<StoreMaintenancePolicyRecord, LoomError> {
    let items = value_array(value, "store maintenance policy")?;
    exact(items, 14, "store maintenance policy")?;
    Ok(StoreMaintenancePolicyRecord {
        min_candidate_pages: value_uint(&items[0], "store maintenance policy min_candidate_pages")?,
        min_reusable_pages: value_uint(&items[1], "store maintenance policy min_reusable_pages")?,
        interval_ms: value_uint(&items[2], "store maintenance policy interval_ms")?,
        backoff_ms: value_uint(&items[3], "store maintenance policy backoff_ms")?,
        max_segments: value_uint(&items[4], "store maintenance policy max_segments")?,
        max_pages: value_uint(&items[5], "store maintenance policy max_pages")?,
        full_compaction_enabled: value_bool(
            &items[6],
            "store maintenance policy full_compaction_enabled",
        )?,
        tail_trim_enabled: value_bool(&items[7], "store maintenance policy tail_trim_enabled")?,
        tail_compaction_enabled: value_bool(
            &items[8],
            "store maintenance policy tail_compaction_enabled",
        )?,
        tail_compaction_max_pages: value_uint(
            &items[9],
            "store maintenance policy tail_compaction_max_pages",
        )?,
        tail_compaction_max_objects: value_uint(
            &items[10],
            "store maintenance policy tail_compaction_max_objects",
        )?,
        tail_compaction_max_bytes: value_uint(
            &items[11],
            "store maintenance policy tail_compaction_max_bytes",
        )?,
        tail_compaction_interval_ms: value_uint(
            &items[12],
            "store maintenance policy tail_compaction_interval_ms",
        )?,
        tail_compaction_backoff_ms: value_uint(
            &items[13],
            "store maintenance policy tail_compaction_backoff_ms",
        )?,
    })
}

fn run_state_from_value(value: &CborValue) -> Result<StoreMaintenanceRunStateRecord, LoomError> {
    let items = value_array(value, "store maintenance run state")?;
    exact(items, 17, "store maintenance run state")?;
    Ok(StoreMaintenanceRunStateRecord {
        last_run_ms: value_opt_uint(&items[0], "store maintenance run state last_run_ms")?,
        next_eligible_ms: value_uint(&items[1], "store maintenance run state next_eligible_ms")?,
        last_skip_reason: value_opt_text(
            &items[2],
            "store maintenance run state last_skip_reason",
        )?,
        last_error: value_opt_text(&items[3], "store maintenance run state last_error")?,
        last_tail_trim_attempted: value_bool(
            &items[4],
            "store maintenance run state last_tail_trim_attempted",
        )?,
        last_tail_trim_pages: value_uint(
            &items[5],
            "store maintenance run state last_tail_trim_pages",
        )?,
        last_tail_trim_bytes: value_uint(
            &items[6],
            "store maintenance run state last_tail_trim_bytes",
        )?,
        last_tail_compaction_attempted: value_bool(
            &items[7],
            "store maintenance run state last_tail_compaction_attempted",
        )?,
        last_tail_compaction_relocated_objects: value_uint(
            &items[8],
            "store maintenance run state last_tail_compaction_relocated_objects",
        )?,
        last_tail_compaction_relocated_pages: value_uint(
            &items[9],
            "store maintenance run state last_tail_compaction_relocated_pages",
        )?,
        last_tail_compaction_relocated_bytes: value_uint(
            &items[10],
            "store maintenance run state last_tail_compaction_relocated_bytes",
        )?,
        last_tail_compaction_truncated_pages: value_uint(
            &items[11],
            "store maintenance run state last_tail_compaction_truncated_pages",
        )?,
        last_tail_compaction_conflicts: value_uint(
            &items[12],
            "store maintenance run state last_tail_compaction_conflicts",
        )?,
        last_shrink_skip_reason: value_opt_text(
            &items[13],
            "store maintenance run state last_shrink_skip_reason",
        )?,
        last_progress_steps: value_uint(
            &items[14],
            "store maintenance run state last_progress_steps",
        )?,
        last_yield_count: value_uint(&items[15], "store maintenance run state last_yield_count")?,
        last_overrun_count: value_uint(
            &items[16],
            "store maintenance run state last_overrun_count",
        )?,
    })
}

fn group_commit_from_value(
    value: &CborValue,
) -> Result<StoreGroupCommitDiagnosticsRecord, LoomError> {
    let items = value_array(value, "store group commit diagnostics")?;
    exact(items, 10, "store group commit diagnostics")?;
    Ok(StoreGroupCommitDiagnosticsRecord {
        group_commit_batches_total: value_uint(&items[0], "group commit batches total")?,
        group_commit_transactions_total: value_uint(&items[1], "group commit transactions total")?,
        group_commit_records_total: value_uint(&items[2], "group commit records total")?,
        fsync_total_micros: value_uint(&items[3], "group commit fsync total micros")?,
        fsync_count: value_uint(&items[4], "group commit fsync count")?,
        write_lock_wait_total_micros: value_uint(&items[5], "group commit write lock wait total")?,
        write_lock_wait_count: value_uint(&items[6], "group commit write lock wait count")?,
        pending_durable_window_transactions: value_uint(
            &items[7],
            "group commit pending transactions",
        )?,
        pending_durable_window_records: value_uint(&items[8], "group commit pending records")?,
        pinned_reader_blockers: value_opt_uint(&items[9], "group commit pinned reader blockers")?,
    })
}

fn status_from_value(value: &CborValue) -> Result<StoreMaintenanceStatusRecord, LoomError> {
    let items = value_array(value, "store maintenance status")?;
    exact(items, 13, "store maintenance status")?;
    Ok(StoreMaintenanceStatusRecord {
        generation: value_uint(&items[0], "store maintenance status generation")?,
        object_count: value_uint(&items[1], "store maintenance status object_count")?,
        physical_page_count: value_uint(&items[2], "store maintenance status physical_page_count")?,
        physical_bytes: value_uint(&items[3], "store maintenance status physical_bytes")?,
        reusable_free_pages: value_uint(&items[4], "store maintenance status reusable_free_pages")?,
        candidate_dead_pages: value_uint(
            &items[5],
            "store maintenance status candidate_dead_pages",
        )?,
        tail_free_pages: value_uint(&items[6], "store maintenance status tail_free_pages")?,
        tail_free_bytes: value_uint(&items[7], "store maintenance status tail_free_bytes")?,
        last_validated_mark_epoch: value_uint(
            &items[8],
            "store maintenance status last_validated_mark_epoch",
        )?,
        touched_segments: value_uint_vec(&items[9], "store maintenance status touched_segments")?,
        candidate_segments: value_uint_vec(
            &items[10],
            "store maintenance status candidate_segments",
        )?,
        segment_overflow: value_bool(&items[11], "store maintenance status segment_overflow")?,
        group_commit: group_commit_from_value(&items[12])?,
    })
}

fn overlay_health_from_value(
    value: &CborValue,
) -> Result<StoreMutableOverlayHealthRecord, LoomError> {
    let items = value_array(value, "store mutable overlay health")?;
    exact(items, 8, "store mutable overlay health")?;
    Ok(StoreMutableOverlayHealthRecord {
        current_generation: value_uint(&items[0], "overlay health current_generation")?,
        current_record_count: value_uint(&items[1], "overlay health current_record_count")?,
        tombstone_count: value_uint(&items[2], "overlay health tombstone_count")?,
        live_checkpoint_references: value_uint(
            &items[3],
            "overlay health live_checkpoint_references",
        )?,
        reclaimable_overlay_pages: value_uint(&items[4], "overlay health reclaimable_pages")?,
        blocked_reclamation_reasons: value_text_vec(
            &items[5],
            "overlay health blocked_reclamation_reasons",
        )?,
        hot_write_count: value_uint(&items[6], "overlay health hot_write_count")?,
        active_writer_contention_indicators: value_uint(
            &items[7],
            "overlay health active_writer_contention_indicators",
        )?,
    })
}

fn mvcc_from_value(value: &CborValue) -> Result<StoreMvccSnapshotDiagnosticsRecord, LoomError> {
    let items = value_array(value, "store mvcc snapshot diagnostics")?;
    exact(items, 3, "store mvcc snapshot diagnostics")?;
    let pins = value_array(&items[2], "store mvcc pins")?
        .iter()
        .map(|pin| {
            let pin_items = value_array(pin, "store mvcc pin")?;
            exact(pin_items, 3, "store mvcc pin")?;
            let identity_items = value_array(&pin_items[1], "store mvcc pin identity")?;
            exact(identity_items, 2, "store mvcc pin identity")?;
            Ok(StoreMvccSnapshotPinRecord {
                pin_id: value_uint(&pin_items[0], "store mvcc pin id")?,
                identity: StoreMvccSnapshotIdentityRecord {
                    overlay_generation: value_uint(
                        &identity_items[0],
                        "store mvcc pin overlay_generation",
                    )?,
                    immutable_base_root: value_opt_text(
                        &identity_items[1],
                        "store mvcc pin immutable_base_root",
                    )?,
                },
                owner: value_opt_text(&pin_items[2], "store mvcc pin owner")?,
            })
        })
        .collect::<Result<Vec<_>, LoomError>>()?;
    Ok(StoreMvccSnapshotDiagnosticsRecord {
        active_snapshot_count: value_uint(&items[0], "store mvcc active_snapshot_count")?,
        oldest_pinned_overlay_generation: value_opt_uint(
            &items[1],
            "store mvcc oldest_pinned_overlay_generation",
        )?,
        pins,
    })
}

fn growth_from_value(value: &CborValue) -> Result<StoreGrowthDomainRecord, LoomError> {
    let items = value_array(value, "store growth domain")?;
    exact(items, 4, "store growth domain")?;
    Ok(StoreGrowthDomainRecord {
        domain: value_text(&items[0], "store growth domain name")?,
        current_records: value_uint(&items[1], "store growth current_records")?,
        obsolete_records: value_uint(&items[2], "store growth obsolete_records")?,
        payload_bytes: value_uint(&items[3], "store growth payload_bytes")?,
    })
}

fn live_roots_from_value(value: &CborValue) -> Result<StoreLiveRootDiagnosticsRecord, LoomError> {
    let items = value_array(value, "store live-root diagnostics")?;
    exact(items, 2, "store live-root diagnostics")?;
    let classes = value_array(&items[1], "store live-root classes")?
        .iter()
        .map(|class| {
            let class_items = value_array(class, "store live-root class")?;
            exact(class_items, 4, "store live-root class")?;
            let examples = value_array(&class_items[2], "store live-root examples")?
                .iter()
                .map(|example| {
                    let example_items = value_array(example, "store live-root example")?;
                    exact(example_items, 2, "store live-root example")?;
                    Ok(StoreLiveRootExampleRecord {
                        id: value_text(&example_items[0], "store live-root example id")?,
                        digest: value_text(&example_items[1], "store live-root example digest")?,
                    })
                })
                .collect::<Result<Vec<_>, LoomError>>()?;
            Ok(StoreLiveRootClassDiagnosticsRecord {
                class: value_text(&class_items[0], "store live-root class")?,
                count: value_uint(&class_items[1], "store live-root class count")?,
                examples,
                truncated: value_bool(&class_items[3], "store live-root class truncated")?,
            })
        })
        .collect::<Result<Vec<_>, LoomError>>()?;
    Ok(StoreLiveRootDiagnosticsRecord {
        sample_limit: value_uint(&items[0], "store live-root sample_limit")?,
        classes,
    })
}

fn report_from_value(value: &CborValue) -> Result<StoreMaintenanceReportRecord, LoomError> {
    let items = value_array(value, "store maintenance report")?;
    exact(items, 35, "store maintenance report")?;
    Ok(StoreMaintenanceReportRecord {
        status: status_from_value(&items[0])?,
        overlay_health: overlay_health_from_value(&items[1])?,
        mvcc_snapshots: mvcc_from_value(&items[2])?,
        policy: policy_from_value(&items[3])?,
        run_state: run_state_from_value(&items[4])?,
        mark_epoch: value_opt_uint(&items[5], "store maintenance report mark_epoch")?,
        mark_completed: value_bool(&items[6], "store maintenance report mark_completed")?,
        marked_live_objects: value_uint(&items[7], "store maintenance report marked_live_objects")?,
        marked_live_bytes: value_uint(&items[8], "store maintenance report marked_live_bytes")?,
        live_bytes: value_uint(&items[9], "store maintenance report live_bytes")?,
        candidate_reclaimable_bytes: value_uint(
            &items[10],
            "store maintenance report candidate_reclaimable_bytes",
        )?,
        reusable_free_bytes: value_uint(
            &items[11],
            "store maintenance report reusable_free_bytes",
        )?,
        overlay_obsolete_record_count: value_uint(
            &items[12],
            "store maintenance report overlay_obsolete_record_count",
        )?,
        overlay_obsolete_page_count: value_uint(
            &items[13],
            "store maintenance report overlay_obsolete_page_count",
        )?,
        tail_free_pages: value_uint(&items[14], "store maintenance report tail_free_pages")?,
        tail_free_bytes: value_uint(&items[15], "store maintenance report tail_free_bytes")?,
        tail_trim_eligible: value_bool(&items[16], "store maintenance report tail_trim_eligible")?,
        tail_blocked_by_live_objects: value_bool(
            &items[17],
            "store maintenance report tail_blocked_by_live_objects",
        )?,
        tail_compaction_eligible: value_bool(
            &items[18],
            "store maintenance report tail_compaction_eligible",
        )?,
        full_compaction_required_for_shrink: value_bool(
            &items[19],
            "store maintenance report full_compaction_required_for_shrink",
        )?,
        tail_trim_attempted: value_bool(
            &items[20],
            "store maintenance report tail_trim_attempted",
        )?,
        tail_trim_pages: value_uint(&items[21], "store maintenance report tail_trim_pages")?,
        tail_trim_bytes: value_uint(&items[22], "store maintenance report tail_trim_bytes")?,
        tail_compaction_attempted: value_bool(
            &items[23],
            "store maintenance report tail_compaction_attempted",
        )?,
        tail_compaction_relocated_objects: value_uint(
            &items[24],
            "store maintenance report tail_compaction_relocated_objects",
        )?,
        tail_compaction_relocated_pages: value_uint(
            &items[25],
            "store maintenance report tail_compaction_relocated_pages",
        )?,
        tail_compaction_relocated_bytes: value_uint(
            &items[26],
            "store maintenance report tail_compaction_relocated_bytes",
        )?,
        tail_compaction_truncated_pages: value_uint(
            &items[27],
            "store maintenance report tail_compaction_truncated_pages",
        )?,
        tail_compaction_conflicts: value_uint(
            &items[28],
            "store maintenance report tail_compaction_conflicts",
        )?,
        last_shrink_skip_reason: value_opt_text(
            &items[29],
            "store maintenance report last_shrink_skip_reason",
        )?,
        retained_control_roots: value_uint(
            &items[30],
            "store maintenance report retained_control_roots",
        )?,
        derived_payload_count: value_uint(
            &items[31],
            "store maintenance report derived_payload_count",
        )?,
        growth_domains: value_array(&items[32], "store maintenance report growth_domains")?
            .iter()
            .map(growth_from_value)
            .collect::<Result<Vec<_>, LoomError>>()?,
        eligible: value_bool(&items[33], "store maintenance report eligible")?,
        reason: value_text(&items[34], "store maintenance report reason")?,
    })
}

pub fn store_maintenance_status_request_from_cbor(
    bytes: &[u8],
) -> Result<StoreMaintenanceStatusRequest, LoomError> {
    let items = arr(bytes)?;
    exact(&items, 1, "store maintenance status request")?;
    Ok(StoreMaintenanceStatusRequest {
        include_live_root_diagnostics: boolean(&items, 0)?,
    })
}

pub fn store_maintenance_status_request_to_cbor(
    request: &StoreMaintenanceStatusRequest,
) -> Vec<u8> {
    enc(CborValue::Array(vec![CborValue::Bool(
        request.include_live_root_diagnostics,
    )]))
}

pub fn store_maintenance_status_result_to_cbor(result: &StoreMaintenanceStatusResult) -> Vec<u8> {
    enc(CborValue::Array(vec![
        report_to_value(&result.report),
        result
            .live_root_diagnostics
            .as_ref()
            .map_or(CborValue::Null, live_roots_to_value),
    ]))
}

pub fn store_maintenance_status_result_from_cbor(
    bytes: &[u8],
) -> Result<StoreMaintenanceStatusResult, LoomError> {
    let items = arr(bytes)?;
    exact(&items, 2, "store maintenance status result")?;
    Ok(StoreMaintenanceStatusResult {
        report: report_from_value(&items[0])?,
        live_root_diagnostics: match &items[1] {
            CborValue::Null => None,
            value => Some(live_roots_from_value(value)?),
        },
    })
}

pub fn store_maintenance_policy_update_from_cbor(
    bytes: &[u8],
) -> Result<StoreMaintenancePolicyUpdate, LoomError> {
    let items = arr(bytes)?;
    exact(&items, 14, "store maintenance policy update")?;
    Ok(StoreMaintenancePolicyUpdate {
        min_candidate_pages: opt_uint(&items, 0)?,
        min_reusable_pages: opt_uint(&items, 1)?,
        interval_ms: opt_uint(&items, 2)?,
        backoff_ms: opt_uint(&items, 3)?,
        max_segments: opt_uint(&items, 4)?,
        max_pages: opt_uint(&items, 5)?,
        full_compaction_enabled: opt_bool(&items, 6)?,
        tail_trim_enabled: opt_bool(&items, 7)?,
        tail_compaction_enabled: opt_bool(&items, 8)?,
        tail_compaction_max_pages: opt_uint(&items, 9)?,
        tail_compaction_max_objects: opt_uint(&items, 10)?,
        tail_compaction_max_bytes: opt_uint(&items, 11)?,
        tail_compaction_interval_ms: opt_uint(&items, 12)?,
        tail_compaction_backoff_ms: opt_uint(&items, 13)?,
    })
}

pub fn store_maintenance_policy_update_to_cbor(update: &StoreMaintenancePolicyUpdate) -> Vec<u8> {
    enc(CborValue::Array(vec![
        update
            .min_candidate_pages
            .map_or(CborValue::Null, CborValue::Uint),
        update
            .min_reusable_pages
            .map_or(CborValue::Null, CborValue::Uint),
        update.interval_ms.map_or(CborValue::Null, CborValue::Uint),
        update.backoff_ms.map_or(CborValue::Null, CborValue::Uint),
        update.max_segments.map_or(CborValue::Null, CborValue::Uint),
        update.max_pages.map_or(CborValue::Null, CborValue::Uint),
        update
            .full_compaction_enabled
            .map_or(CborValue::Null, CborValue::Bool),
        update
            .tail_trim_enabled
            .map_or(CborValue::Null, CborValue::Bool),
        update
            .tail_compaction_enabled
            .map_or(CborValue::Null, CborValue::Bool),
        update
            .tail_compaction_max_pages
            .map_or(CborValue::Null, CborValue::Uint),
        update
            .tail_compaction_max_objects
            .map_or(CborValue::Null, CborValue::Uint),
        update
            .tail_compaction_max_bytes
            .map_or(CborValue::Null, CborValue::Uint),
        update
            .tail_compaction_interval_ms
            .map_or(CborValue::Null, CborValue::Uint),
        update
            .tail_compaction_backoff_ms
            .map_or(CborValue::Null, CborValue::Uint),
    ]))
}

pub fn store_maintenance_run_request_from_cbor(
    bytes: &[u8],
) -> Result<StoreMaintenanceRunRequest, LoomError> {
    let items = arr(bytes)?;
    exact(&items, 2, "store maintenance run request")?;
    Ok(StoreMaintenanceRunRequest {
        max_segments: opt_uint(&items, 0)?,
        max_pages: opt_uint(&items, 1)?,
    })
}

pub fn store_maintenance_run_request_to_cbor(request: &StoreMaintenanceRunRequest) -> Vec<u8> {
    enc(CborValue::Array(vec![
        request
            .max_segments
            .map_or(CborValue::Null, CborValue::Uint),
        request.max_pages.map_or(CborValue::Null, CborValue::Uint),
    ]))
}

pub fn store_maintenance_run_result_to_cbor(result: &StoreMaintenanceRunResult) -> Vec<u8> {
    let kind = match result.kind {
        StoreMaintenanceRunKind::Skipped => 0,
        StoreMaintenanceRunKind::Marked => 1,
        StoreMaintenanceRunKind::Compacted => 2,
        StoreMaintenanceRunKind::Reclaimed => 3,
    };
    enc(CborValue::Array(vec![
        CborValue::Uint(kind),
        result
            .reason
            .as_ref()
            .map_or(CborValue::Null, |s| CborValue::Text(s.clone())),
        result.visited.map_or(CborValue::Null, CborValue::Uint),
        result.pending.map_or(CborValue::Null, CborValue::Uint),
        result.before.map_or(CborValue::Null, CborValue::Uint),
        result.after.map_or(CborValue::Null, CborValue::Uint),
        result.reclaimed.map_or(CborValue::Null, CborValue::Uint),
        result
            .required_temp_bytes
            .map_or(CborValue::Null, CborValue::Uint),
        result
            .available_temp_bytes
            .map_or(CborValue::Null, CborValue::Uint),
        result
            .segments_reclaimed
            .map_or(CborValue::Null, CborValue::Uint),
        result.pages_freed.map_or(CborValue::Null, CborValue::Uint),
        result
            .tail_trim_pages
            .map_or(CborValue::Null, CborValue::Uint),
        result
            .tail_trim_bytes
            .map_or(CborValue::Null, CborValue::Uint),
        result
            .tail_compaction_attempted
            .map_or(CborValue::Null, CborValue::Bool),
        result
            .tail_compaction_relocated_objects
            .map_or(CborValue::Null, CborValue::Uint),
        result
            .tail_compaction_relocated_pages
            .map_or(CborValue::Null, CborValue::Uint),
        result
            .tail_compaction_truncated_pages
            .map_or(CborValue::Null, CborValue::Uint),
        result
            .objects_relocated
            .map_or(CborValue::Null, CborValue::Uint),
        result
            .objects_dropped
            .map_or(CborValue::Null, CborValue::Uint),
        result.elapsed_ms.map_or(CborValue::Null, CborValue::Uint),
        run_state_to_value(&result.run_state),
        report_to_value(&result.report),
    ]))
}

pub fn store_maintenance_run_result_from_cbor(
    bytes: &[u8],
) -> Result<StoreMaintenanceRunResult, LoomError> {
    let items = arr(bytes)?;
    exact(&items, 22, "store maintenance run result")?;
    let kind = match value_uint(&items[0], "store maintenance run kind")? {
        0 => StoreMaintenanceRunKind::Skipped,
        1 => StoreMaintenanceRunKind::Marked,
        2 => StoreMaintenanceRunKind::Compacted,
        3 => StoreMaintenanceRunKind::Reclaimed,
        _ => {
            return Err(LoomError::new(
                Code::InvalidArgument,
                "store maintenance run kind must be a known tag",
            ));
        }
    };
    Ok(StoreMaintenanceRunResult {
        kind,
        reason: value_opt_text(&items[1], "store maintenance run reason")?,
        visited: value_opt_uint(&items[2], "store maintenance run visited")?,
        pending: value_opt_uint(&items[3], "store maintenance run pending")?,
        before: value_opt_uint(&items[4], "store maintenance run before")?,
        after: value_opt_uint(&items[5], "store maintenance run after")?,
        reclaimed: value_opt_uint(&items[6], "store maintenance run reclaimed")?,
        required_temp_bytes: value_opt_uint(
            &items[7],
            "store maintenance run required_temp_bytes",
        )?,
        available_temp_bytes: value_opt_uint(
            &items[8],
            "store maintenance run available_temp_bytes",
        )?,
        segments_reclaimed: value_opt_uint(&items[9], "store maintenance run segments_reclaimed")?,
        pages_freed: value_opt_uint(&items[10], "store maintenance run pages_freed")?,
        tail_trim_pages: value_opt_uint(&items[11], "store maintenance run tail_trim_pages")?,
        tail_trim_bytes: value_opt_uint(&items[12], "store maintenance run tail_trim_bytes")?,
        tail_compaction_attempted: value_opt_bool(
            &items[13],
            "store maintenance run tail_compaction_attempted",
        )?,
        tail_compaction_relocated_objects: value_opt_uint(
            &items[14],
            "store maintenance run tail_compaction_relocated_objects",
        )?,
        tail_compaction_relocated_pages: value_opt_uint(
            &items[15],
            "store maintenance run tail_compaction_relocated_pages",
        )?,
        tail_compaction_truncated_pages: value_opt_uint(
            &items[16],
            "store maintenance run tail_compaction_truncated_pages",
        )?,
        objects_relocated: value_opt_uint(&items[17], "store maintenance run objects_relocated")?,
        objects_dropped: value_opt_uint(&items[18], "store maintenance run objects_dropped")?,
        elapsed_ms: value_opt_uint(&items[19], "store maintenance run elapsed_ms")?,
        run_state: run_state_from_value(&items[20])?,
        report: report_from_value(&items[21])?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn maintenance_policy_record() -> StoreMaintenancePolicyRecord {
        StoreMaintenancePolicyRecord {
            min_candidate_pages: 1,
            min_reusable_pages: 2,
            interval_ms: 3,
            backoff_ms: 4,
            max_segments: 5,
            max_pages: 6,
            full_compaction_enabled: true,
            tail_trim_enabled: false,
            tail_compaction_enabled: true,
            tail_compaction_max_pages: 7,
            tail_compaction_max_objects: 8,
            tail_compaction_max_bytes: 9,
            tail_compaction_interval_ms: 10,
            tail_compaction_backoff_ms: 11,
        }
    }

    fn maintenance_run_state_record() -> StoreMaintenanceRunStateRecord {
        StoreMaintenanceRunStateRecord {
            last_run_ms: Some(12),
            next_eligible_ms: 13,
            last_skip_reason: Some("skip".to_string()),
            last_error: Some("error".to_string()),
            last_tail_trim_attempted: true,
            last_tail_trim_pages: 14,
            last_tail_trim_bytes: 15,
            last_tail_compaction_attempted: true,
            last_tail_compaction_relocated_objects: 16,
            last_tail_compaction_relocated_pages: 17,
            last_tail_compaction_relocated_bytes: 18,
            last_tail_compaction_truncated_pages: 19,
            last_tail_compaction_conflicts: 20,
            last_shrink_skip_reason: Some("blocked".to_string()),
            last_progress_steps: 21,
            last_yield_count: 22,
            last_overrun_count: 23,
        }
    }

    fn maintenance_report_record() -> StoreMaintenanceReportRecord {
        StoreMaintenanceReportRecord {
            status: StoreMaintenanceStatusRecord {
                generation: 24,
                object_count: 25,
                physical_page_count: 26,
                physical_bytes: 27,
                reusable_free_pages: 28,
                candidate_dead_pages: 29,
                tail_free_pages: 30,
                tail_free_bytes: 31,
                last_validated_mark_epoch: 32,
                touched_segments: vec![33, 34],
                candidate_segments: vec![35, 36],
                segment_overflow: true,
                group_commit: StoreGroupCommitDiagnosticsRecord {
                    group_commit_batches_total: 37,
                    group_commit_transactions_total: 38,
                    group_commit_records_total: 39,
                    fsync_total_micros: 40,
                    fsync_count: 41,
                    write_lock_wait_total_micros: 42,
                    write_lock_wait_count: 43,
                    pending_durable_window_transactions: 44,
                    pending_durable_window_records: 45,
                    pinned_reader_blockers: Some(46),
                },
            },
            overlay_health: StoreMutableOverlayHealthRecord {
                current_generation: 47,
                current_record_count: 48,
                tombstone_count: 49,
                live_checkpoint_references: 50,
                reclaimable_overlay_pages: 51,
                blocked_reclamation_reasons: vec!["writer".to_string(), "snapshot".to_string()],
                hot_write_count: 52,
                active_writer_contention_indicators: 53,
            },
            mvcc_snapshots: StoreMvccSnapshotDiagnosticsRecord {
                active_snapshot_count: 54,
                oldest_pinned_overlay_generation: Some(55),
                pins: vec![StoreMvccSnapshotPinRecord {
                    pin_id: 56,
                    identity: StoreMvccSnapshotIdentityRecord {
                        overlay_generation: 57,
                        immutable_base_root: Some("b3:abc".to_string()),
                    },
                    owner: Some("owner".to_string()),
                }],
            },
            policy: maintenance_policy_record(),
            run_state: maintenance_run_state_record(),
            mark_epoch: Some(58),
            mark_completed: true,
            marked_live_objects: 59,
            marked_live_bytes: 60,
            live_bytes: 61,
            candidate_reclaimable_bytes: 62,
            reusable_free_bytes: 63,
            overlay_obsolete_record_count: 64,
            overlay_obsolete_page_count: 65,
            tail_free_pages: 66,
            tail_free_bytes: 67,
            tail_trim_eligible: true,
            tail_blocked_by_live_objects: false,
            tail_compaction_eligible: true,
            full_compaction_required_for_shrink: true,
            tail_trim_attempted: true,
            tail_trim_pages: 68,
            tail_trim_bytes: 69,
            tail_compaction_attempted: true,
            tail_compaction_relocated_objects: 70,
            tail_compaction_relocated_pages: 71,
            tail_compaction_relocated_bytes: 72,
            tail_compaction_truncated_pages: 73,
            tail_compaction_conflicts: 74,
            last_shrink_skip_reason: Some("capacity".to_string()),
            retained_control_roots: 75,
            derived_payload_count: 76,
            growth_domains: vec![StoreGrowthDomainRecord {
                domain: "overlay".to_string(),
                current_records: 77,
                obsolete_records: 78,
                payload_bytes: 79,
            }],
            eligible: true,
            reason: "eligible".to_string(),
        }
    }

    fn live_root_diagnostics_record() -> StoreLiveRootDiagnosticsRecord {
        StoreLiveRootDiagnosticsRecord {
            sample_limit: 2,
            classes: vec![StoreLiveRootClassDiagnosticsRecord {
                class: "current".to_string(),
                count: 1,
                examples: vec![StoreLiveRootExampleRecord {
                    id: "root".to_string(),
                    digest: "b3:def".to_string(),
                }],
                truncated: false,
            }],
        }
    }

    #[test]
    fn store_stat_round_trips() {
        let stat = StoreStat {
            object_count: 42,
            generation: 3,
            physical_page_count: 100,
            physical_bytes: 4096,
            reusable_free_pages: 5,
            candidate_dead_pages: 2,
            last_validated_mark_epoch: 7,
            touched_segments: 1,
            candidate_segments: 0,
            segment_overflow: 0,
        };
        assert_eq!(
            store_stat_from_cbor(&store_stat_to_cbor(&stat)).unwrap(),
            stat
        );
    }

    #[test]
    fn store_policy_result_round_trips_get_and_set() {
        let get = StorePolicyResult {
            fips_required: true,
            default_durability: OverlayDurabilityPolicy::Strict,
            facet_durability_overrides: vec![StoreFacetDurabilityAssignment {
                facet: FacetKind::Document,
                durability: OverlayDurabilityPolicy::Relaxed,
            }],
            audit_seq: None,
        };
        let set = StorePolicyResult {
            fips_required: false,
            default_durability: OverlayDurabilityPolicy::Normal,
            facet_durability_overrides: vec![StoreFacetDurabilityAssignment {
                facet: FacetKind::Search,
                durability: OverlayDurabilityPolicy::Ephemeral,
            }],
            audit_seq: Some(9),
        };
        assert_eq!(
            store_policy_result_from_cbor(&store_policy_result_to_cbor(&get)).unwrap(),
            get
        );
        assert_eq!(
            store_policy_result_from_cbor(&store_policy_result_to_cbor(&set)).unwrap(),
            set
        );
    }

    #[test]
    fn store_policy_update_round_trips_complete_fields() {
        let update = StorePolicyUpdate {
            fips_required: Some(true),
            default_durability: Some(OverlayDurabilityPolicy::Strict),
            facet_durability_assignments: vec![StoreFacetDurabilityAssignment {
                facet: FacetKind::Document,
                durability: OverlayDurabilityPolicy::Relaxed,
            }],
            clear_facet_durability: vec![FacetKind::Search],
        };
        assert_eq!(
            store_policy_update_from_cbor(&store_policy_update_to_cbor(&update)).unwrap(),
            update
        );
    }

    #[test]
    fn store_rekey_request_round_trips_passphrase_and_raw_kek() {
        let passphrase = StoreRekeyRequest {
            credential: StoreRekeyCredential::Passphrase(b"newpw".to_vec()),
            reseal: false,
            suite: None,
        };
        assert_eq!(
            store_rekey_request_from_cbor(&store_rekey_request_to_cbor(&passphrase)).unwrap(),
            passphrase
        );

        let raw = StoreRekeyRequest {
            credential: StoreRekeyCredential::RawKek([0x5a; KEY_LEN]),
            reseal: true,
            suite: Some("aes256gcm".to_string()),
        };
        assert_eq!(
            store_rekey_request_from_cbor(&store_rekey_request_to_cbor(&raw)).unwrap(),
            raw
        );
    }

    #[test]
    fn store_rekey_result_round_trips_fast_and_reseal() {
        let fast = StoreRekeyResult {
            audit_seq: 1,
            resealed: false,
            suite: "xchacha20poly1305".to_string(),
            bytes_before: None,
            bytes_after: None,
        };
        let reseal = StoreRekeyResult {
            audit_seq: 2,
            resealed: true,
            suite: "aes256gcm".to_string(),
            bytes_before: Some(1000),
            bytes_after: Some(1024),
        };
        assert_eq!(
            store_rekey_result_from_cbor(&store_rekey_result_to_cbor(&fast)).unwrap(),
            fast
        );
        assert_eq!(
            store_rekey_result_from_cbor(&store_rekey_result_to_cbor(&reseal)).unwrap(),
            reseal
        );
    }

    #[test]
    fn store_bundle_import_result_round_trips() {
        let result = StoreBundleImportResult {
            workspace_id: "11111111-1111-4111-9111-111111111111".to_string(),
            workspace_name: "main".to_string(),
            facets: vec!["vcs".to_string(), "files".to_string()],
            objects_transferred: 3,
            objects_skipped: 1,
            new_tips: vec!["main:abcd".to_string()],
            dry_run: true,
        };
        assert_eq!(
            store_bundle_import_result_from_cbor(&store_bundle_import_result_to_cbor(&result))
                .unwrap(),
            result
        );
    }

    #[test]
    fn rejects_non_array() {
        let bad = encode(&CborValue::Uint(1)).unwrap();
        assert_eq!(
            store_stat_from_cbor(&bad).unwrap_err().code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn store_maintenance_status_result_round_trips_non_default_records() {
        let result = StoreMaintenanceStatusResult {
            report: maintenance_report_record(),
            live_root_diagnostics: Some(live_root_diagnostics_record()),
        };
        assert_eq!(
            store_maintenance_status_result_from_cbor(&store_maintenance_status_result_to_cbor(
                &result
            ))
            .unwrap(),
            result
        );

        let absent = StoreMaintenanceStatusResult {
            report: maintenance_report_record(),
            live_root_diagnostics: None,
        };
        assert_eq!(
            store_maintenance_status_result_from_cbor(&store_maintenance_status_result_to_cbor(
                &absent
            ))
            .unwrap(),
            absent
        );
    }

    #[test]
    fn store_maintenance_run_result_round_trips_non_default_records() {
        let result = StoreMaintenanceRunResult {
            kind: StoreMaintenanceRunKind::Reclaimed,
            reason: Some("done".to_string()),
            visited: Some(80),
            pending: Some(81),
            before: Some(82),
            after: Some(83),
            reclaimed: Some(84),
            required_temp_bytes: Some(85),
            available_temp_bytes: Some(86),
            segments_reclaimed: Some(87),
            pages_freed: Some(88),
            tail_trim_pages: Some(89),
            tail_trim_bytes: Some(90),
            tail_compaction_attempted: Some(true),
            tail_compaction_relocated_objects: Some(91),
            tail_compaction_relocated_pages: Some(92),
            tail_compaction_truncated_pages: Some(93),
            objects_relocated: Some(94),
            objects_dropped: Some(95),
            elapsed_ms: Some(96),
            run_state: maintenance_run_state_record(),
            report: maintenance_report_record(),
        };
        assert_eq!(
            store_maintenance_run_result_from_cbor(&store_maintenance_run_result_to_cbor(&result))
                .unwrap(),
            result
        );
    }

    #[test]
    fn store_maintenance_result_decoders_reject_malformed_structures() {
        let status = store_maintenance_status_result_to_cbor(&StoreMaintenanceStatusResult {
            report: maintenance_report_record(),
            live_root_diagnostics: Some(live_root_diagnostics_record()),
        });
        let mut status_value = decode(&status).unwrap();
        if let CborValue::Array(items) = &mut status_value {
            items.push(CborValue::Null);
        }
        assert_eq!(
            store_maintenance_status_result_from_cbor(&enc(status_value))
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );

        let mut status_value = decode(&status).unwrap();
        if let CborValue::Array(items) = &mut status_value
            && let CborValue::Array(report) = &mut items[0]
            && let CborValue::Array(policy) = &mut report[3]
        {
            policy.pop();
        }
        assert_eq!(
            store_maintenance_status_result_from_cbor(&enc(status_value))
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );

        let mut status_value = decode(&status).unwrap();
        if let CborValue::Array(items) = &mut status_value
            && let CborValue::Array(live_roots) = &mut items[1]
            && let CborValue::Array(classes) = &mut live_roots[1]
            && let CborValue::Array(class) = &mut classes[0]
            && let CborValue::Array(examples) = &mut class[2]
            && let CborValue::Array(example) = &mut examples[0]
        {
            example[1] = CborValue::Uint(1);
        }
        assert_eq!(
            store_maintenance_status_result_from_cbor(&enc(status_value))
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );

        let run = store_maintenance_run_result_to_cbor(&StoreMaintenanceRunResult {
            kind: StoreMaintenanceRunKind::Reclaimed,
            reason: Some("done".to_string()),
            visited: Some(1),
            pending: None,
            before: None,
            after: None,
            reclaimed: None,
            required_temp_bytes: None,
            available_temp_bytes: None,
            segments_reclaimed: None,
            pages_freed: None,
            tail_trim_pages: None,
            tail_trim_bytes: None,
            tail_compaction_attempted: None,
            tail_compaction_relocated_objects: None,
            tail_compaction_relocated_pages: None,
            tail_compaction_truncated_pages: None,
            objects_relocated: None,
            objects_dropped: None,
            elapsed_ms: None,
            run_state: maintenance_run_state_record(),
            report: maintenance_report_record(),
        });
        let mut run_value = decode(&run).unwrap();
        if let CborValue::Array(items) = &mut run_value {
            items[0] = CborValue::Uint(99);
        }
        assert_eq!(
            store_maintenance_run_result_from_cbor(&enc(run_value))
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );

        let mut run_value = decode(&run).unwrap();
        if let CborValue::Array(items) = &mut run_value {
            items.pop();
        }
        assert_eq!(
            store_maintenance_run_result_from_cbor(&enc(run_value))
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
    }
}
