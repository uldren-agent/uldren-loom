use crate::{
    FileStore, GcSegmentBudget, StoreMaintenanceReport, StoreMaintenanceRunState, gc_loom,
};
use loom_core::Loom;
use loom_core::error::{Code, LoomError, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub const DAEMON_MAINTENANCE_MARK_OBJECTS: u64 = 256;
pub const DAEMON_MAINTENANCE_MAX_SEGMENTS: u64 = 1;
pub const DAEMON_MAINTENANCE_MAX_PAGES: u64 = 1024;
pub const DAEMON_MAINTENANCE_TAIL_COMPACTION_MAX_PAGES: u64 = 64;
pub const DAEMON_MAINTENANCE_TAIL_COMPACTION_MAX_OBJECTS: u64 = 32;
pub const DAEMON_MAINTENANCE_TAIL_COMPACTION_MAX_BYTES: u64 = 4 * 1024 * 1024;
pub const DAEMON_MAINTENANCE_SLICE_MS: u64 = 250;

#[derive(Debug, Clone, Copy)]
pub struct StoreMaintenanceRunBudget {
    pub mark_objects: u64,
    pub max_segments: u64,
    pub max_pages: u64,
    pub tail_compaction_max_pages: u64,
    pub tail_compaction_max_objects: u64,
    pub tail_compaction_max_bytes: u64,
    pub slice_ms: u64,
}

impl StoreMaintenanceRunBudget {
    pub fn daemon_automatic() -> Self {
        Self {
            mark_objects: DAEMON_MAINTENANCE_MARK_OBJECTS,
            max_segments: DAEMON_MAINTENANCE_MAX_SEGMENTS,
            max_pages: DAEMON_MAINTENANCE_MAX_PAGES,
            tail_compaction_max_pages: DAEMON_MAINTENANCE_TAIL_COMPACTION_MAX_PAGES,
            tail_compaction_max_objects: DAEMON_MAINTENANCE_TAIL_COMPACTION_MAX_OBJECTS,
            tail_compaction_max_bytes: DAEMON_MAINTENANCE_TAIL_COMPACTION_MAX_BYTES,
            slice_ms: DAEMON_MAINTENANCE_SLICE_MS,
        }
    }

    fn apply(self, policy: &mut crate::StoreMaintenancePolicy) {
        policy.max_segments = policy.max_segments.min(self.max_segments);
        policy.max_pages = policy.max_pages.min(self.max_pages);
        policy.tail_compaction_max_pages = policy
            .tail_compaction_max_pages
            .min(self.tail_compaction_max_pages);
        policy.tail_compaction_max_objects = policy
            .tail_compaction_max_objects
            .min(self.tail_compaction_max_objects);
        policy.tail_compaction_max_bytes = policy
            .tail_compaction_max_bytes
            .min(self.tail_compaction_max_bytes);
    }
}

pub trait StoreMaintenanceClock {
    fn now_ms(&self) -> u64;
    fn monotonic_now(&self) -> Instant;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemStoreMaintenanceClock;

impl StoreMaintenanceClock for SystemStoreMaintenanceClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX)
    }

    fn monotonic_now(&self) -> Instant {
        Instant::now()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreMaintenanceRunKind {
    Skipped,
    Marked,
    Compacted,
    Reclaimed,
}

#[derive(Debug, Clone)]
pub struct StoreMaintenanceRunOutcome {
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
    pub run_state: StoreMaintenanceRunState,
    pub report: StoreMaintenanceReport,
}

impl StoreMaintenanceRunOutcome {
    fn skipped(reason: &str, report: StoreMaintenanceReport) -> Self {
        Self {
            kind: StoreMaintenanceRunKind::Skipped,
            reason: Some(reason.to_string()),
            visited: None,
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
            run_state: report.run_state.clone(),
            report,
        }
    }

    fn from_report(kind: StoreMaintenanceRunKind, report: StoreMaintenanceReport) -> Self {
        Self {
            kind,
            reason: None,
            visited: None,
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
            run_state: report.run_state.clone(),
            report,
        }
    }
}

pub fn run_store_maintenance_once(
    loom: &mut Loom<FileStore>,
    manual: bool,
    max_segments: Option<u64>,
    max_pages: Option<u64>,
) -> Result<StoreMaintenanceRunOutcome> {
    run_store_maintenance_once_with_clock(
        loom,
        manual,
        max_segments,
        max_pages,
        None,
        None,
        &SystemStoreMaintenanceClock,
    )
}

pub fn run_store_maintenance_once_with_clock(
    loom: &mut Loom<FileStore>,
    manual: bool,
    max_segments: Option<u64>,
    max_pages: Option<u64>,
    budget: Option<StoreMaintenanceRunBudget>,
    cancel: Option<&AtomicBool>,
    clock: &dyn StoreMaintenanceClock,
) -> Result<StoreMaintenanceRunOutcome> {
    let now = clock.now_ms();
    if cancel.is_some_and(|cancel| cancel.load(Ordering::Acquire)) {
        let report = loom.store().store_maintenance_report(now)?;
        return Ok(StoreMaintenanceRunOutcome::skipped(
            "shutdown_cancelled",
            report,
        ));
    }
    let started = clock.monotonic_now();
    let mut progress_steps = 0u64;
    let mut yield_count = 0u64;
    let mut overrun_count = 0u64;
    let mut policy = loom.store().store_maintenance_policy()?;
    if let Some(budget) = budget {
        budget.apply(&mut policy);
    }
    if let Some(value) = max_segments {
        if value == 0 {
            return Err(LoomError::invalid("max-segments must be nonzero"));
        }
        policy.max_segments = value;
    }
    if let Some(value) = max_pages {
        if value == 0 {
            return Err(LoomError::invalid("max-pages must be nonzero"));
        }
        policy.max_pages = value;
    }
    let report = loom.store().store_maintenance_report(now)?;
    if !manual && !report.eligible {
        return Ok(StoreMaintenanceRunOutcome::skipped("not_eligible", report));
    }
    let active = loom.store().active_reachability_mark_epoch()?;
    let needs_mark = active
        .as_ref()
        .map(|epoch| !epoch.state.completed)
        .unwrap_or(true);
    if needs_mark {
        if active.is_none() {
            crate::begin_loom_reachability_mark_epoch(loom)?;
        }
        if budget.is_some_and(|budget| elapsed_ms(clock, started) >= budget.slice_ms) {
            let state = StoreMaintenanceRunState {
                last_run_ms: Some(now),
                next_eligible_ms: now,
                last_skip_reason: Some("mark_epoch_incomplete".to_string()),
                last_error: None,
                last_yield_count: 1,
                last_overrun_count: 1,
                ..StoreMaintenanceRunState::default()
            };
            loom.store().record_store_maintenance_run_state(state)?;
            let report = loom.store().store_maintenance_report(now)?;
            let mut outcome =
                StoreMaintenanceRunOutcome::from_report(StoreMaintenanceRunKind::Marked, report);
            outcome.visited = Some(0);
            outcome.pending = Some(0);
            return Ok(outcome);
        }
        if cancel.is_some_and(|cancel| cancel.load(Ordering::Acquire)) {
            let report = loom.store().store_maintenance_report(now)?;
            return Ok(StoreMaintenanceRunOutcome::skipped(
                "shutdown_cancelled",
                report,
            ));
        }
        let step = if let Some(budget) = budget {
            let expired = || elapsed_ms(clock, started) >= budget.slice_ms;
            crate::step_loom_reachability_mark_epoch_while(
                loom,
                usize::try_from(budget.mark_objects).unwrap_or(usize::MAX),
                Some(&expired),
            )?
        } else {
            crate::step_loom_reachability_mark_epoch_while(loom, 1024, None)?
        };
        if !step.completed {
            progress_steps = u64::try_from(step.visited).unwrap_or(u64::MAX);
            yield_count = 1;
            if budget.is_some_and(|budget| elapsed_ms(clock, started) >= budget.slice_ms) {
                overrun_count = 1;
            }
            let state = StoreMaintenanceRunState {
                last_run_ms: Some(now),
                next_eligible_ms: now,
                last_skip_reason: Some("mark_epoch_incomplete".to_string()),
                last_error: None,
                last_progress_steps: progress_steps,
                last_yield_count: yield_count,
                last_overrun_count: overrun_count,
                ..StoreMaintenanceRunState::default()
            };
            loom.store().record_store_maintenance_run_state(state)?;
            let report = loom.store().store_maintenance_report(now)?;
            let mut outcome =
                StoreMaintenanceRunOutcome::from_report(StoreMaintenanceRunKind::Marked, report);
            outcome.visited = Some(u64::try_from(step.visited).unwrap_or(u64::MAX));
            outcome.pending = Some(u64::try_from(step.pending).unwrap_or(u64::MAX));
            return Ok(outcome);
        }
    }
    let refreshed_report = loom.store().store_maintenance_report(now)?;
    let whole_file_due = maintenance_debt_thresholds_met(&policy, &refreshed_report.status);
    let whole_file_allowed = manual && policy.full_compaction_enabled && whole_file_due;
    let mut shrink_skip_reason = (!manual && policy.full_compaction_enabled && whole_file_due)
        .then(|| "full_compaction_manual_only".to_string());
    let mut tail_trim_attempted = false;
    let mut tail_trim_pages = 0;
    let mut tail_trim_bytes = 0;
    let mut tail_compaction = crate::TailCompactionStats::default();
    let mut outcome = if whole_file_allowed {
        let capacity = loom.store().ensure_compaction_capacity()?;
        let stats = gc_loom(loom)?;
        let report = loom.store().store_maintenance_report(now)?;
        let mut outcome =
            StoreMaintenanceRunOutcome::from_report(StoreMaintenanceRunKind::Compacted, report);
        outcome.before = Some(stats.before);
        outcome.after = Some(stats.after);
        outcome.reclaimed = Some(stats.reclaimed());
        outcome.required_temp_bytes = Some(capacity.required_temp_bytes);
        outcome.available_temp_bytes = capacity.available_temp_bytes;
        outcome
    } else {
        if cancel.is_some_and(|cancel| cancel.load(Ordering::Acquire)) {
            let report = loom.store().store_maintenance_report(now)?;
            return Ok(StoreMaintenanceRunOutcome::skipped(
                "shutdown_cancelled",
                report,
            ));
        }
        let reclaim_budget = GcSegmentBudget {
            max_segments: policy.max_segments,
            max_pages: policy.max_pages,
        };
        let reclaim = if let Some(budget) = budget {
            let expired = || elapsed_ms(clock, started) >= budget.slice_ms;
            tail_trim_attempted = policy.tail_trim_enabled;
            loom.store_mut().gc_validated_segments_while(
                reclaim_budget,
                policy.tail_trim_enabled,
                &expired,
            )
        } else if policy.tail_trim_enabled {
            tail_trim_attempted = true;
            loom.store_mut().gc_validated_segments(reclaim_budget)
        } else {
            loom.store_mut()
                .gc_validated_segments_without_tail_trim(reclaim_budget)
        };
        let stats = match reclaim {
            Ok(stats) => stats,
            Err(error) if error.code == Code::ResourceExhausted => {
                if shrink_skip_reason.is_none() {
                    shrink_skip_reason = Some("budget_exhausted".to_string());
                }
                yield_count = yield_count.saturating_add(1);
                crate::GcStats::default()
            }
            Err(error) => return Err(error),
        };
        let consolidated = loom.store().consolidate_delta_pack_candidates(
            usize::try_from(policy.max_pages.min(256)).unwrap_or(256),
        )?;
        progress_steps = progress_steps
            .saturating_add(stats.segments_reclaimed)
            .saturating_add(stats.pages_freed)
            .saturating_add(stats.objects_relocated)
            .saturating_add(stats.objects_dropped)
            .saturating_add(consolidated);
        tail_trim_pages = stats.pages_trimmed;
        tail_trim_bytes = stats.pages_trimmed.saturating_mul(crate::STORE_PAGE_SIZE);
        let slice_exhausted =
            budget.is_some_and(|budget| elapsed_ms(clock, started) >= budget.slice_ms);
        if slice_exhausted {
            overrun_count = overrun_count.saturating_add(1);
        }
        if policy.tail_compaction_enabled && !slice_exhausted {
            if cancel.is_some_and(|cancel| cancel.load(Ordering::Acquire)) {
                let report = loom.store().store_maintenance_report(now)?;
                return Ok(StoreMaintenanceRunOutcome::skipped(
                    "shutdown_cancelled",
                    report,
                ));
            }
            let tail_result = if let Some(budget) = budget {
                let expired = || elapsed_ms(clock, started) >= budget.slice_ms;
                loom.store_mut().compact_tail_once_while(
                    policy.tail_compaction_max_pages,
                    policy.tail_compaction_max_objects,
                    policy.tail_compaction_max_bytes,
                    &expired,
                )
            } else {
                loom.store_mut().compact_tail_once(
                    policy.tail_compaction_max_pages,
                    policy.tail_compaction_max_objects,
                    policy.tail_compaction_max_bytes,
                )
            };
            tail_compaction = match tail_result {
                Ok(stats) => stats,
                Err(error) if error.code == Code::ResourceExhausted => {
                    if shrink_skip_reason.is_none() {
                        shrink_skip_reason = Some("budget_exhausted".to_string());
                    }
                    yield_count = yield_count.saturating_add(1);
                    crate::TailCompactionStats {
                        attempted: true,
                        skipped: true,
                        ..crate::TailCompactionStats::default()
                    }
                }
                Err(error) => return Err(error),
            };
            progress_steps = progress_steps
                .saturating_add(tail_compaction.relocated_objects)
                .saturating_add(tail_compaction.relocated_pages)
                .saturating_add(tail_compaction.truncated_pages);
            if tail_compaction.truncated_pages > 0 {
                tail_trim_attempted = true;
                tail_trim_pages = tail_trim_pages.saturating_add(tail_compaction.truncated_pages);
                tail_trim_bytes = tail_trim_pages.saturating_mul(crate::STORE_PAGE_SIZE);
            }
        } else if policy.tail_compaction_enabled && slice_exhausted && shrink_skip_reason.is_none()
        {
            shrink_skip_reason = Some("budget_exhausted".to_string());
            yield_count = yield_count.saturating_add(1);
        }
        let report = loom.store().store_maintenance_report(now)?;
        let mut outcome =
            StoreMaintenanceRunOutcome::from_report(StoreMaintenanceRunKind::Reclaimed, report);
        outcome.segments_reclaimed = Some(stats.segments_reclaimed);
        outcome.pages_freed = Some(stats.pages_freed);
        outcome.tail_trim_pages = Some(tail_trim_pages);
        outcome.tail_trim_bytes = Some(tail_trim_bytes);
        outcome.tail_compaction_attempted = Some(tail_compaction.attempted);
        outcome.tail_compaction_relocated_objects = Some(tail_compaction.relocated_objects);
        outcome.tail_compaction_relocated_pages = Some(tail_compaction.relocated_pages);
        outcome.tail_compaction_truncated_pages = Some(tail_compaction.truncated_pages);
        outcome.objects_relocated = Some(stats.objects_relocated);
        outcome.objects_dropped = Some(stats.objects_dropped);
        outcome.elapsed_ms = Some(elapsed_ms(clock, started));
        outcome
    };
    let state = StoreMaintenanceRunState {
        last_run_ms: Some(now),
        next_eligible_ms: now.saturating_add(policy.interval_ms),
        last_skip_reason: None,
        last_error: None,
        last_tail_trim_attempted: tail_trim_attempted,
        last_tail_trim_pages: tail_trim_pages,
        last_tail_trim_bytes: tail_trim_bytes,
        last_tail_compaction_attempted: tail_compaction.attempted,
        last_tail_compaction_relocated_objects: tail_compaction.relocated_objects,
        last_tail_compaction_relocated_pages: tail_compaction.relocated_pages,
        last_tail_compaction_relocated_bytes: tail_compaction.relocated_bytes,
        last_tail_compaction_truncated_pages: tail_compaction.truncated_pages,
        last_tail_compaction_conflicts: tail_compaction.conflicts,
        last_shrink_skip_reason: shrink_skip_reason.take().or_else(|| {
            tail_compaction
                .skipped
                .then(|| "tail_compaction_skipped".to_string())
        }),
        last_progress_steps: progress_steps,
        last_yield_count: yield_count,
        last_overrun_count: overrun_count,
    };
    loom.store().record_store_maintenance_run_state(state)?;
    let report = loom.store().store_maintenance_report(now)?;
    outcome.run_state = report.run_state.clone();
    outcome.report = report;
    Ok(outcome)
}

pub fn maintenance_debt_thresholds_met(
    policy: &crate::StoreMaintenancePolicy,
    status: &crate::MaintenanceStatus,
) -> bool {
    status.candidate_dead_pages >= policy.min_candidate_pages
        && status.reusable_free_pages >= policy.min_reusable_pages
}

fn elapsed_ms(clock: &dyn StoreMaintenanceClock, started: Instant) -> u64 {
    clock
        .monotonic_now()
        .saturating_duration_since(started)
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
