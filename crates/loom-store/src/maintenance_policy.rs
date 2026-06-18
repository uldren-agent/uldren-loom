use crate::{FileStore, PAGE_SIZE, corrupt};
use loom_core::error::{Code, LoomError, Result};

pub(crate) const MAINTENANCE_POLICY_KEY: &[u8] = b"maintenance/v1/policy";
pub(crate) const MAINTENANCE_RUN_KEY: &[u8] = b"maintenance/v1/run-state";
const POLICY_MAGIC: &[u8; 8] = b"LMAINTP1";
const RUN_MAGIC: &[u8; 8] = b"LMAINTR1";
const POLICY_VERSION_V1: u16 = 1;
const POLICY_VERSION_V2: u16 = 2;
const POLICY_VERSION: u16 = 3;
const RUN_VERSION_V1: u16 = 1;
const RUN_VERSION_V2: u16 = 2;
const RUN_VERSION: u16 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreMaintenancePolicy {
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

impl Default for StoreMaintenancePolicy {
    fn default() -> Self {
        Self {
            min_candidate_pages: 256,
            min_reusable_pages: 256,
            interval_ms: 60_000,
            backoff_ms: 300_000,
            max_segments: 1,
            max_pages: 1024,
            full_compaction_enabled: false,
            tail_trim_enabled: true,
            tail_compaction_enabled: true,
            tail_compaction_max_pages: 128,
            tail_compaction_max_objects: 64,
            tail_compaction_max_bytes: 8 * 1024 * 1024,
            tail_compaction_interval_ms: 300_000,
            tail_compaction_backoff_ms: 900_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreMaintenanceRunState {
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
}

impl Default for StoreMaintenanceRunState {
    fn default() -> Self {
        Self {
            last_run_ms: None,
            next_eligible_ms: 0,
            last_skip_reason: Some("never_run".to_string()),
            last_error: None,
            last_tail_trim_attempted: false,
            last_tail_trim_pages: 0,
            last_tail_trim_bytes: 0,
            last_tail_compaction_attempted: false,
            last_tail_compaction_relocated_objects: 0,
            last_tail_compaction_relocated_pages: 0,
            last_tail_compaction_relocated_bytes: 0,
            last_tail_compaction_truncated_pages: 0,
            last_tail_compaction_conflicts: 0,
            last_shrink_skip_reason: Some("never_run".to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreMaintenanceReport {
    pub status: crate::MaintenanceStatus,
    pub policy: StoreMaintenancePolicy,
    pub run_state: StoreMaintenanceRunState,
    pub mark_epoch: Option<u64>,
    pub mark_completed: bool,
    pub marked_live_objects: u64,
    pub marked_live_bytes: u64,
    pub candidate_reclaimable_bytes: u64,
    pub reusable_free_bytes: u64,
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
    /// Count of control roots retained live by the active reachability-mark epoch (reference root,
    /// control fingerprint, and captured derived-artifact roots). 0 when no mark epoch is active.
    pub retained_control_roots: u64,
    /// Count of durable-local derived-artifact payload records (rebuildable, identity-excluded).
    pub derived_payload_count: u64,
    pub eligible: bool,
    pub reason: String,
}

impl FileStore {
    pub fn store_maintenance_policy(&self) -> Result<StoreMaintenancePolicy> {
        self.control_get(MAINTENANCE_POLICY_KEY)?
            .map(|bytes| decode_policy(&bytes))
            .transpose()
            .map(|policy| policy.unwrap_or_default())
    }

    pub fn set_store_maintenance_policy(&self, policy: StoreMaintenancePolicy) -> Result<()> {
        validate_policy(policy)?;
        self.control_set(MAINTENANCE_POLICY_KEY, encode_policy(policy))
    }

    pub fn store_maintenance_run_state(&self) -> Result<StoreMaintenanceRunState> {
        self.control_get(MAINTENANCE_RUN_KEY)?
            .map(|bytes| decode_run_state(&bytes))
            .transpose()
            .map(|state| state.unwrap_or_default())
    }

    pub fn record_store_maintenance_run_state(
        &self,
        state: StoreMaintenanceRunState,
    ) -> Result<()> {
        self.control_set(MAINTENANCE_RUN_KEY, encode_run_state(&state))
    }

    pub fn store_maintenance_report(&self, now_ms: u64) -> Result<StoreMaintenanceReport> {
        let status = self.maintenance_status()?;
        let policy = self.store_maintenance_policy()?;
        let run_state = self.store_maintenance_run_state()?;
        let active = self.active_reachability_mark_epoch()?;
        let mark_current = active
            .as_ref()
            .map(
                |epoch| match self.validate_reachability_mark_epoch_current(epoch) {
                    Ok(()) => Ok(true),
                    Err(error) if error.code == Code::Conflict => Ok(false),
                    Err(error) => Err(error),
                },
            )
            .transpose()?;
        let mut marked_live_objects = 0u64;
        let marked_live_bytes = 0u64;
        let mut mark_epoch = None;
        let mut mark_completed = false;
        if let Some(epoch) = &active {
            mark_epoch = Some(epoch.epoch);
            mark_completed = epoch.state.completed;
            marked_live_objects = epoch.state.marked.len() as u64;
        }
        let candidate_reclaimable_bytes = status.candidate_dead_pages.saturating_mul(PAGE_SIZE);
        let reusable_free_bytes = status.reusable_free_pages.saturating_mul(PAGE_SIZE);
        let tail_free_pages = status.tail_free_pages;
        let tail_free_bytes = status.tail_free_bytes;
        let tail_trim_eligible = policy.tail_trim_enabled && tail_free_pages > 0;
        let tail_blocked_by_live_objects =
            status.reusable_free_pages > tail_free_pages && status.reusable_free_pages > 0;
        let tail_compaction_eligible =
            policy.tail_compaction_enabled && tail_blocked_by_live_objects;
        let full_compaction_required_for_shrink =
            tail_blocked_by_live_objects && !tail_compaction_eligible;
        let retained_control_roots = active.as_ref().map_or(0, |epoch| {
            epoch.derived_roots.len() as u64
                + u64::from(epoch.reference_root.is_some())
                + u64::from(epoch.control_fingerprint.is_some())
        });
        let derived_payload_count = self
            .control_scan_prefix(crate::derived::DERIVED_PREFIX)?
            .len() as u64;
        let reason = maintenance_eligibility_reason(
            &policy,
            &status,
            &run_state,
            active.as_ref(),
            mark_current,
            now_ms,
        );
        let tail_trim_attempted = run_state.last_tail_trim_attempted;
        let tail_trim_pages = run_state.last_tail_trim_pages;
        let tail_trim_bytes = run_state.last_tail_trim_bytes;
        let tail_compaction_attempted = run_state.last_tail_compaction_attempted;
        let tail_compaction_relocated_objects = run_state.last_tail_compaction_relocated_objects;
        let tail_compaction_relocated_pages = run_state.last_tail_compaction_relocated_pages;
        let tail_compaction_relocated_bytes = run_state.last_tail_compaction_relocated_bytes;
        let tail_compaction_truncated_pages = run_state.last_tail_compaction_truncated_pages;
        let tail_compaction_conflicts = run_state.last_tail_compaction_conflicts;
        let last_shrink_skip_reason = run_state.last_shrink_skip_reason.clone();
        Ok(StoreMaintenanceReport {
            status,
            policy,
            run_state,
            mark_epoch,
            mark_completed,
            marked_live_objects,
            marked_live_bytes,
            candidate_reclaimable_bytes,
            reusable_free_bytes,
            tail_free_pages,
            tail_free_bytes,
            tail_trim_eligible,
            tail_blocked_by_live_objects,
            tail_compaction_eligible,
            full_compaction_required_for_shrink,
            tail_trim_attempted,
            tail_trim_pages,
            tail_trim_bytes,
            tail_compaction_attempted,
            tail_compaction_relocated_objects,
            tail_compaction_relocated_pages,
            tail_compaction_relocated_bytes,
            tail_compaction_truncated_pages,
            tail_compaction_conflicts,
            last_shrink_skip_reason,
            retained_control_roots,
            derived_payload_count,
            eligible: matches!(
                reason.as_str(),
                "eligible" | "mark_epoch_incomplete" | "mark_epoch_missing" | "mark_epoch_stale"
            ),
            reason,
        })
    }
}

fn maintenance_eligibility_reason(
    policy: &StoreMaintenancePolicy,
    status: &crate::MaintenanceStatus,
    run_state: &StoreMaintenanceRunState,
    active: Option<&crate::ReachabilityMarkEpoch>,
    mark_current: Option<bool>,
    now_ms: u64,
) -> String {
    if now_ms < run_state.next_eligible_ms {
        return "backoff".to_string();
    }
    match (active, mark_current) {
        (None, _) => return "mark_epoch_missing".to_string(),
        (Some(_), Some(false)) => return "mark_epoch_stale".to_string(),
        (Some(epoch), _) if !epoch.state.completed => {
            return "mark_epoch_incomplete".to_string();
        }
        _ => {}
    }
    if status.candidate_dead_pages < policy.min_candidate_pages {
        return "candidate_debt_below_threshold".to_string();
    }
    if status.reusable_free_pages < policy.min_reusable_pages {
        return "free_debt_below_threshold".to_string();
    }
    match active {
        Some(epoch) if status.last_validated_mark_epoch >= epoch.epoch => "eligible".to_string(),
        Some(_) => "mark_epoch_incomplete".to_string(),
        None => unreachable!("missing mark epochs return before debt evaluation"),
    }
}

fn encode_policy(policy: StoreMaintenancePolicy) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(POLICY_MAGIC);
    out.extend_from_slice(&POLICY_VERSION.to_le_bytes());
    out.push(u8::from(policy.full_compaction_enabled));
    out.push(u8::from(policy.tail_trim_enabled));
    out.push(u8::from(policy.tail_compaction_enabled));
    for value in [
        policy.min_candidate_pages,
        policy.min_reusable_pages,
        policy.interval_ms,
        policy.backoff_ms,
        policy.max_segments,
        policy.max_pages,
        policy.tail_compaction_max_pages,
        policy.tail_compaction_max_objects,
        policy.tail_compaction_max_bytes,
        policy.tail_compaction_interval_ms,
        policy.tail_compaction_backoff_ms,
    ] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

fn decode_policy(bytes: &[u8]) -> Result<StoreMaintenancePolicy> {
    let mut cur = Cursor { bytes, pos: 0 };
    if cur.take(POLICY_MAGIC.len())? != POLICY_MAGIC {
        return Err(corrupt("store maintenance policy magic"));
    }
    let version = cur.u16()?;
    if version != POLICY_VERSION && version != POLICY_VERSION_V2 && version != POLICY_VERSION_V1 {
        return Err(corrupt("store maintenance policy version"));
    }
    if version == POLICY_VERSION_V1 || version == POLICY_VERSION_V2 {
        let _ignored_enabled = cur.bool()?;
    }
    let full_compaction_enabled = cur.bool()?;
    let mut policy = StoreMaintenancePolicy {
        full_compaction_enabled,
        ..StoreMaintenancePolicy::default()
    };
    if version == POLICY_VERSION || version == POLICY_VERSION_V2 {
        policy.tail_trim_enabled = cur.bool()?;
        policy.tail_compaction_enabled = cur.bool()?;
    }
    policy.min_candidate_pages = cur.u64()?;
    policy.min_reusable_pages = cur.u64()?;
    policy.interval_ms = cur.u64()?;
    policy.backoff_ms = cur.u64()?;
    policy.max_segments = cur.u64()?;
    policy.max_pages = cur.u64()?;
    if version == POLICY_VERSION || version == POLICY_VERSION_V2 {
        policy.tail_compaction_max_pages = cur.u64()?;
        policy.tail_compaction_max_objects = cur.u64()?;
        policy.tail_compaction_max_bytes = cur.u64()?;
        policy.tail_compaction_interval_ms = cur.u64()?;
        policy.tail_compaction_backoff_ms = cur.u64()?;
    }
    validate_policy(policy)?;
    cur.finish()?;
    Ok(policy)
}

fn validate_policy(policy: StoreMaintenancePolicy) -> Result<()> {
    if policy.interval_ms == 0 || policy.backoff_ms == 0 {
        return Err(LoomError::new(
            Code::InvalidArgument,
            "store maintenance intervals must be nonzero",
        ));
    }
    if policy.tail_compaction_interval_ms == 0 || policy.tail_compaction_backoff_ms == 0 {
        return Err(LoomError::new(
            Code::InvalidArgument,
            "store maintenance tail compaction intervals must be nonzero",
        ));
    }
    if policy.max_segments == 0 || policy.max_pages == 0 {
        return Err(LoomError::new(
            Code::InvalidArgument,
            "store maintenance budgets must be nonzero",
        ));
    }
    if policy.tail_compaction_max_pages == 0
        || policy.tail_compaction_max_objects == 0
        || policy.tail_compaction_max_bytes == 0
    {
        return Err(LoomError::new(
            Code::InvalidArgument,
            "store maintenance tail compaction budgets must be nonzero",
        ));
    }
    Ok(())
}

fn encode_run_state(state: &StoreMaintenanceRunState) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(RUN_MAGIC);
    out.extend_from_slice(&RUN_VERSION.to_le_bytes());
    put_optional_u64(&mut out, state.last_run_ms);
    out.extend_from_slice(&state.next_eligible_ms.to_le_bytes());
    put_optional_text(&mut out, state.last_skip_reason.as_deref());
    put_optional_text(&mut out, state.last_error.as_deref());
    out.push(u8::from(state.last_tail_trim_attempted));
    out.extend_from_slice(&state.last_tail_trim_pages.to_le_bytes());
    out.extend_from_slice(&state.last_tail_trim_bytes.to_le_bytes());
    out.push(u8::from(state.last_tail_compaction_attempted));
    out.extend_from_slice(&state.last_tail_compaction_relocated_objects.to_le_bytes());
    out.extend_from_slice(&state.last_tail_compaction_relocated_pages.to_le_bytes());
    out.extend_from_slice(&state.last_tail_compaction_relocated_bytes.to_le_bytes());
    out.extend_from_slice(&state.last_tail_compaction_truncated_pages.to_le_bytes());
    out.extend_from_slice(&state.last_tail_compaction_conflicts.to_le_bytes());
    put_optional_text(&mut out, state.last_shrink_skip_reason.as_deref());
    out
}

fn decode_run_state(bytes: &[u8]) -> Result<StoreMaintenanceRunState> {
    let mut cur = Cursor { bytes, pos: 0 };
    if cur.take(RUN_MAGIC.len())? != RUN_MAGIC {
        return Err(corrupt("store maintenance run-state magic"));
    }
    let version = cur.u16()?;
    if version != RUN_VERSION && version != RUN_VERSION_V2 && version != RUN_VERSION_V1 {
        return Err(corrupt("store maintenance run-state version"));
    }
    let mut state = StoreMaintenanceRunState {
        last_run_ms: cur.optional_u64()?,
        next_eligible_ms: cur.u64()?,
        last_skip_reason: cur.optional_text()?,
        last_error: cur.optional_text()?,
        last_tail_trim_attempted: false,
        last_tail_trim_pages: 0,
        last_tail_trim_bytes: 0,
        last_tail_compaction_attempted: false,
        last_tail_compaction_relocated_objects: 0,
        last_tail_compaction_relocated_pages: 0,
        last_tail_compaction_relocated_bytes: 0,
        last_tail_compaction_truncated_pages: 0,
        last_tail_compaction_conflicts: 0,
        last_shrink_skip_reason: Some("never_run".to_string()),
    };
    if version >= RUN_VERSION_V2 {
        state.last_tail_trim_attempted = cur.bool()?;
        state.last_tail_trim_pages = cur.u64()?;
        state.last_tail_trim_bytes = cur.u64()?;
    }
    if version == RUN_VERSION {
        state.last_tail_compaction_attempted = cur.bool()?;
        state.last_tail_compaction_relocated_objects = cur.u64()?;
        state.last_tail_compaction_relocated_pages = cur.u64()?;
        state.last_tail_compaction_relocated_bytes = cur.u64()?;
        state.last_tail_compaction_truncated_pages = cur.u64()?;
        state.last_tail_compaction_conflicts = cur.u64()?;
        state.last_shrink_skip_reason = cur.optional_text()?;
    }
    cur.finish()?;
    Ok(state)
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

fn put_optional_text(out: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(value) => {
            out.push(1);
            out.extend_from_slice(&(value.len() as u32).to_le_bytes());
            out.extend_from_slice(value.as_bytes());
        }
        None => out.push(0),
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(len)
            .ok_or_else(|| corrupt("store maintenance record offset overflow"))?;
        let out = self
            .bytes
            .get(self.pos..end)
            .ok_or_else(|| corrupt("store maintenance record truncated"))?;
        self.pos = end;
        Ok(out)
    }

    fn bool(&mut self) -> Result<bool> {
        match self.take(1)?[0] {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(corrupt("store maintenance boolean")),
        }
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

    fn optional_u64(&mut self) -> Result<Option<u64>> {
        match self.take(1)?[0] {
            0 => Ok(None),
            1 => Ok(Some(self.u64()?)),
            _ => Err(corrupt("store maintenance optional u64")),
        }
    }

    fn optional_text(&mut self) -> Result<Option<String>> {
        match self.take(1)?[0] {
            0 => Ok(None),
            1 => {
                let len = self.u32()? as usize;
                let bytes = self.take(len)?;
                String::from_utf8(bytes.to_vec())
                    .map(Some)
                    .map_err(|_| corrupt("store maintenance utf-8"))
            }
            _ => Err(corrupt("store maintenance optional text")),
        }
    }

    fn finish(&self) -> Result<()> {
        if self.pos == self.bytes.len() {
            Ok(())
        } else {
            Err(corrupt("store maintenance trailing bytes"))
        }
    }
}
