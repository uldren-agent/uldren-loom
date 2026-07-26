use loom_core::document::{document_get_text, document_put_text};
use loom_core::{Algo, Code, FacetKind, ObjectStore, OverlayKey};
use loom_store::{
    FileStore, GcSegmentBudget, StoreDurabilityPolicy, StoreMaintenanceReport,
    StoreMaintenanceRunState, open_loom, save_loom,
};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_ITERATIONS: u64 = 40;

fn main() {
    if let Err(err) = run() {
        eprintln!("loom performance harness failed: {err}");
        std::process::exit(1);
    }
}

fn run() -> loom_core::Result<()> {
    let iterations = read_u64_env("LOOM_PERFORMANCE_ITERATIONS", DEFAULT_ITERATIONS);
    let out_dir = env::var_os("LOOM_PERFORMANCE_ARTIFACT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/loom-performance"));
    fs::create_dir_all(&out_dir).map_err(io_err)?;
    let run_dir = out_dir.join(format!("run-{}", now_ms()));
    fs::create_dir_all(&run_dir).map_err(io_err)?;

    let mut report = PerformanceReport {
        command: "just test-performance".to_string(),
        iterations,
        artifacts: ArtifactPaths {
            json: run_dir.join("performance-report.json"),
            summary: run_dir.join("performance-summary.txt"),
        },
        scenarios: Vec::new(),
    };

    report
        .scenarios
        .push(hot_mutable_overwrite(&run_dir, iterations)?);
    report
        .scenarios
        .push(random_new_item_bundles(&run_dir, iterations)?);
    report
        .scenarios
        .push(concurrent_readers_and_writer(&run_dir, iterations)?);
    report
        .scenarios
        .push(vcs_promotion(&run_dir, iterations.min(12).max(4))?);
    report.scenarios.extend(durability_mode_scenarios(
        &run_dir,
        iterations.min(12).max(4),
    )?);
    report.scenarios.push(maintenance_latency_breakdown(
        &run_dir,
        iterations.min(12).max(4),
    )?);
    require_completed_durability_scenarios(&report.scenarios)?;

    let summary = report.summary();
    fs::write(&report.artifacts.summary, &summary).map_err(io_err)?;
    fs::write(&report.artifacts.json, report.to_json()).map_err(io_err)?;
    print!("{summary}");
    println!();
    println!("JSON artifact: {}", report.artifacts.json.display());
    println!("Summary artifact: {}", report.artifacts.summary.display());
    Ok(())
}

fn hot_mutable_overwrite(run_dir: &Path, iterations: u64) -> loom_core::Result<ScenarioReport> {
    let path = run_dir.join("hot-overwrite.loom");
    let store = FileStore::create_with_profile(&path, Algo::Blake3)?;
    let key = overlay_key("tickets", "hot-ticket", 0)?;
    let mut latencies = Vec::new();
    let start = Instant::now();
    for update in 0..iterations {
        let op_start = Instant::now();
        store.put_mutable_overlay_value(
            key.clone(),
            format!("{{\"ticket\":\"MX-HOT\",\"revision\":{update}}}").into_bytes(),
        )?;
        latencies.push(op_start.elapsed());
    }
    let elapsed = start.elapsed();
    scenario_report(
        "hot_mutable_overwrite",
        "completed",
        iterations,
        elapsed,
        &latencies,
        &store,
        &path,
        Some("single ticket current-record key overwritten repeatedly"),
    )
}

fn random_new_item_bundles(run_dir: &Path, iterations: u64) -> loom_core::Result<ScenarioReport> {
    let path = run_dir.join("random-bundles.loom");
    let store = FileStore::create_with_profile(&path, Algo::Blake3)?;
    let domains = ["tickets", "lanes", "pages", "documents"];
    let mut latencies = Vec::new();
    let start = Instant::now();
    for update in 0..iterations {
        let op_start = Instant::now();
        let mut entries = Vec::new();
        for domain in domains {
            entries.push((
                overlay_key(domain, "bundle", update)?,
                format!("{{\"domain\":\"{domain}\",\"item\":{update}}}").into_bytes(),
            ));
        }
        store.put_mutable_overlay_values(entries)?;
        latencies.push(op_start.elapsed());
    }
    let elapsed = start.elapsed();
    scenario_report(
        "random_new_ticket_lane_page_document_bundles",
        "completed",
        iterations.saturating_mul(domains.len() as u64),
        elapsed,
        &latencies,
        &store,
        &path,
        Some("one ticket, lane, page, and document current-record key per bundle"),
    )
}

fn concurrent_readers_and_writer(
    run_dir: &Path,
    iterations: u64,
) -> loom_core::Result<ScenarioReport> {
    let path = run_dir.join("concurrent.loom");
    let store = Arc::new(FileStore::create_with_profile(&path, Algo::Blake3)?);
    let key = overlay_key("documents", "concurrent", 0)?;
    let started = Arc::new(Barrier::new(3));
    let done = Arc::new(AtomicBool::new(false));
    let reader_successes = Arc::new(AtomicU64::new(0));
    let reader_failures = Arc::new(AtomicU64::new(0));
    let snapshot_mismatches = Arc::new(AtomicU64::new(0));

    let writer_started = Arc::clone(&started);
    let writer_done = Arc::clone(&done);
    let writer_store = Arc::clone(&store);
    let writer_key = key.clone();
    let writer = thread::spawn(move || -> loom_core::Result<Vec<Duration>> {
        writer_started.wait();
        let mut latencies = Vec::new();
        for update in 1..=iterations {
            let op_start = Instant::now();
            writer_store.put_mutable_overlay_value(
                writer_key.clone(),
                format!("{{\"doc\":\"concurrent-{update}\"}}").into_bytes(),
            )?;
            latencies.push(op_start.elapsed());
            thread::yield_now();
        }
        writer_done.store(true, Ordering::Release);
        Ok(latencies)
    });

    let mut readers = Vec::new();
    for _ in 0..2 {
        let reader_started = Arc::clone(&started);
        let reader_done = Arc::clone(&done);
        let reader_successes = Arc::clone(&reader_successes);
        let reader_failures = Arc::clone(&reader_failures);
        let snapshot_mismatches = Arc::clone(&snapshot_mismatches);
        let reader_store = Arc::clone(&store);
        let reader_key = key.clone();
        readers.push(thread::spawn(move || {
            reader_started.wait();
            while !reader_done.load(Ordering::Acquire) {
                let snapshot = match reader_store
                    .open_mvcc_snapshot_with_owner(Some("performance.concurrent_reader"))
                {
                    Ok(snapshot) => snapshot,
                    Err(_) => {
                        reader_failures.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                };
                let generation = snapshot.overlay_generation().as_u64();
                let expected = if generation == 0 {
                    None
                } else {
                    Some(format!("{{\"doc\":\"concurrent-{generation}\"}}").into_bytes())
                };
                match snapshot.read_composite(&reader_key, |_, _| Ok(None)) {
                    Ok(actual) if actual == expected => {
                        reader_successes.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok(_) => {
                        snapshot_mismatches.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(_) => {
                        reader_failures.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }));
    }

    let start = Instant::now();
    let latencies = writer.join().map_err(|_| {
        loom_core::LoomError::new(Code::Internal, "performance writer thread panicked")
    })??;
    for reader in readers {
        reader.join().map_err(|_| {
            loom_core::LoomError::new(Code::Internal, "performance reader thread panicked")
        })?;
    }
    let reader_failed_opens = reader_failures.load(Ordering::Relaxed);
    let stable_snapshot_mismatches = snapshot_mismatches.load(Ordering::Relaxed);
    if reader_failed_opens != 0 || stable_snapshot_mismatches != 0 {
        return Err(loom_core::LoomError::new(
            Code::Internal,
            format!(
                "concurrent reader scenario saw {reader_failed_opens} failed opens and {stable_snapshot_mismatches} torn snapshots"
            ),
        ));
    }
    let reader_successful_opens = reader_successes.load(Ordering::Relaxed);
    if reader_successful_opens == 0 {
        return Err(loom_core::LoomError::new(
            Code::Internal,
            "concurrent reader scenario did not open any snapshots",
        ));
    }
    let elapsed = start.elapsed();
    let mut report = scenario_report(
        "concurrent_readers_and_writer",
        "completed",
        iterations,
        elapsed,
        &latencies,
        store.as_ref(),
        &path,
        Some("two MVCC snapshot readers while one writer mutates a current-record key"),
    )?;
    report.extra.insert(
        "reader_successful_opens".to_string(),
        reader_successful_opens.to_string(),
    );
    report.extra.insert(
        "reader_failed_opens".to_string(),
        reader_failed_opens.to_string(),
    );
    report.extra.insert(
        "stable_snapshot_mismatches".to_string(),
        stable_snapshot_mismatches.to_string(),
    );
    report.extra.insert(
        "concurrency_behavior".to_string(),
        "MVCC readers pin a generation and read stable snapshots while the writer publishes newer generations"
            .to_string(),
    );
    Ok(report)
}

fn vcs_promotion(run_dir: &Path, iterations: u64) -> loom_core::Result<ScenarioReport> {
    let path = run_dir.join("vcs-promotion.loom");
    let mut loom = open_loom(&path)?;
    let workspace = loom.registry_mut().create(
        FacetKind::Document,
        Some("performance-docs"),
        loom_core::WorkspaceId::from_bytes([42; 16]),
    )?;
    let mut latencies = Vec::new();
    let start = Instant::now();
    for update in 0..iterations {
        let op_start = Instant::now();
        document_put_text(
            &mut loom,
            workspace,
            "performance",
            "current",
            &format!("vcs-promotion-{update}"),
            None,
        )?;
        latencies.push(op_start.elapsed());
    }
    let promote_start = Instant::now();
    let commit = loom.commit(workspace, "performance", "promote current documents", 1)?;
    let promotion_latency = promote_start.elapsed();
    let read = document_get_text(&loom, workspace, "performance", "current")?
        .ok_or_else(|| loom_core::LoomError::not_found("promoted document"))?;
    save_loom(&mut loom)?;
    drop(loom);
    let reopened = open_loom(&path)?;
    let store = reopened.store();
    let elapsed = start.elapsed();
    let mut report = scenario_report(
        "vcs_promotion",
        "completed",
        iterations,
        elapsed,
        &latencies,
        store,
        &path,
        Some("document current records are committed to immutable VCS history"),
    )?;
    report
        .extra
        .insert("commit".to_string(), commit.to_string());
    report.extra.insert(
        "promotion_latency_ms".to_string(),
        millis_f64(promotion_latency).to_string(),
    );
    report.extra.insert(
        "promoted_text_bytes".to_string(),
        read.text.len().to_string(),
    );
    Ok(report)
}

fn maintenance_latency_breakdown(
    run_dir: &Path,
    iterations: u64,
) -> loom_core::Result<ScenarioReport> {
    let path = run_dir.join("maintenance-breakdown.loom");
    let mut loom = open_loom(&path)?;
    let _workspace = loom.registry_mut().create(
        FacetKind::Document,
        Some("maintenance-breakdown-docs"),
        loom_core::WorkspaceId::from_bytes([43; 16]),
    )?;
    save_loom(&mut loom)?;
    drop(loom);
    let maintenance_store = FileStore::open(&path)?;
    let live = maintenance_store.put(b"maintenance-live")?;
    drop(maintenance_store);
    let loom = open_loom(&path)?;
    let mut marked = loom.live_object_set(loom.store().reference_root())?;
    drop(loom);
    let maintenance_store = FileStore::open(&path)?;
    marked.insert(live);
    let state = loom_core::ReachabilityMarkState {
        pinned: std::collections::BTreeSet::new(),
        marked,
        queue: std::collections::VecDeque::new(),
        stream_roots: std::collections::VecDeque::new(),
        content_roots: std::collections::VecDeque::new(),
        prolly_cursors: std::collections::VecDeque::new(),
        completed: true,
    };
    let epoch = maintenance_store.begin_reachability_mark_epoch(
        maintenance_store.reference_root(),
        std::collections::BTreeSet::new(),
        state,
    )?;
    maintenance_store.complete_reachability_mark_epoch(&epoch)?;
    for update in 0..iterations.saturating_mul(32) {
        maintenance_store.put(format!("maintenance-dead-{update}").as_bytes())?;
    }
    let initial_report = maintenance_store.store_maintenance_report(now_ms())?;
    drop(maintenance_store);

    let mut open_latencies = Vec::new();
    let mut operation_latencies = Vec::new();
    let mut save_latencies = Vec::new();
    let maintenance_start = Arc::new(Barrier::new(2));
    let foreground_done = Arc::new(AtomicBool::new(false));
    let maintenance_path = path.clone();
    let maintenance_gate = Arc::clone(&maintenance_start);
    let maintenance_done = Arc::clone(&foreground_done);
    let min_maintenance_slices = iterations.max(4);
    let max_maintenance_slices = min_maintenance_slices.saturating_mul(32);
    let maintenance_worker =
        thread::spawn(move || -> loom_core::Result<MaintenanceDiagnosticResult> {
            maintenance_gate.wait();
            let mut result = MaintenanceDiagnosticResult::default();
            let mut slices = 0u64;
            while slices < max_maintenance_slices {
                let started = Instant::now();
                let Ok(mut store) = FileStore::open(&maintenance_path) else {
                    if maintenance_done.load(Ordering::Acquire) {
                        result.latencies.push(started.elapsed());
                        result.yield_count = result.yield_count.saturating_add(1);
                        result.last_outcome = "yielded:store-open-conflict".to_string();
                        slices = slices.saturating_add(1);
                    }
                    thread::sleep(Duration::from_millis(1));
                    thread::yield_now();
                    continue;
                };
                let deadline = started
                    .checked_add(Duration::from_millis(50))
                    .unwrap_or(started);
                let gc = store.gc_validated_segments_until(
                    GcSegmentBudget {
                        max_segments: 1,
                        max_pages: 256,
                    },
                    true,
                    deadline,
                );
                let elapsed = started.elapsed();
                result.latencies.push(elapsed);
                if elapsed >= Duration::from_millis(50) {
                    result.overrun_count = result.overrun_count.saturating_add(1);
                }
                match gc {
                    Ok(stats) => {
                        let progress = stats
                            .segments_reclaimed
                            .saturating_add(stats.pages_freed)
                            .saturating_add(stats.objects_relocated)
                            .saturating_add(stats.objects_dropped);
                        result.completed_work = result.completed_work.saturating_add(progress);
                        result.last_outcome = format!(
                            "completed:segments={}:pages={}:relocated={}:dropped={}",
                            stats.segments_reclaimed,
                            stats.pages_freed,
                            stats.objects_relocated,
                            stats.objects_dropped
                        );
                    }
                    Err(error) if error.code == Code::ResourceExhausted => {
                        result.yield_count = result.yield_count.saturating_add(1);
                        result.last_outcome = format!("yielded:{:?}:{}", error.code, error.message);
                    }
                    Err(error) => {
                        result.last_outcome = format!("error:{:?}:{}", error.code, error.message);
                        result.error_count = result.error_count.saturating_add(1);
                    }
                }
                let run_state = StoreMaintenanceRunState {
                    last_run_ms: Some(now_ms()),
                    next_eligible_ms: now_ms(),
                    last_skip_reason: (result.yield_count > 0)
                        .then(|| "budget_exhausted".to_string()),
                    last_error: (result.error_count > 0).then(|| result.last_outcome.clone()),
                    last_progress_steps: result.completed_work,
                    last_yield_count: result.yield_count,
                    last_overrun_count: result.overrun_count,
                    ..StoreMaintenanceRunState::default()
                };
                store.record_store_maintenance_run_state(run_state)?;
                let report = store.store_maintenance_report(now_ms())?;
                result.final_candidate_pages = report.status.candidate_dead_pages;
                result.final_candidate_bytes = report.candidate_reclaimable_bytes;
                result.final_reusable_pages = report.status.reusable_free_pages;
                result.final_mark_completed = report.mark_completed;
                result.final_eligible = report.eligible;
                result.final_reason = report.reason;
                result.debt_samples.push(report.status.candidate_dead_pages);
                if maintenance_done.load(Ordering::Acquire)
                    && result.latencies.len() as u64 >= min_maintenance_slices
                {
                    result.no_new_debt_slices = result.no_new_debt_slices.saturating_add(1);
                    if !result.final_eligible {
                        break;
                    }
                }
                slices = slices.saturating_add(1);
                if slices < max_maintenance_slices {
                    thread::yield_now();
                }
            }
            Ok(result)
        });
    let start = Instant::now();
    maintenance_start.wait();
    for update in 0..iterations {
        let open_start = Instant::now();
        let store = open_filestore_with_retry(&path, Duration::from_secs(2))?;
        open_latencies.push(open_start.elapsed());

        let operation_start = Instant::now();
        store.put_mutable_overlay_value(
            overlay_key("documents", "maintenance-breakdown", update)?,
            format!("maintenance-breakdown-{update}").into_bytes(),
        )?;
        operation_latencies.push(operation_start.elapsed());

        let save_start = Instant::now();
        store.flush_hot_mutable_commits()?;
        save_latencies.push(save_start.elapsed());
        drop(store);
    }
    foreground_done.store(true, Ordering::Release);
    let maintenance_result = maintenance_worker
        .join()
        .map_err(|_| loom_core::LoomError::new(Code::Internal, "maintenance worker panicked"))??;
    let elapsed = start.elapsed();
    let reopened = open_loom(&path)?;
    let final_report = reopened.store().store_maintenance_report(now_ms())?;
    let diagnostic_status = classify_maintenance_diagnostic(
        initial_report.status.candidate_dead_pages,
        final_report.status.candidate_dead_pages,
        maintenance_result.error_count,
        final_report.eligible,
        &final_report.reason,
    );
    let mut report = scenario_report(
        "maintenance_latency_breakdown",
        diagnostic_status.status,
        iterations,
        elapsed,
        &operation_latencies,
        reopened.store(),
        &path,
        Some("separates open, foreground operation, save, and maintenance diagnostic latency"),
    )?;
    insert_latency_breakdown(&mut report, "open", &open_latencies);
    insert_latency_breakdown(&mut report, "operation", &operation_latencies);
    insert_latency_breakdown(&mut report, "save", &save_latencies);
    insert_latency_breakdown(&mut report, "maintenance", &maintenance_result.latencies);
    report.extra.insert(
        "maintenance_outcome".to_string(),
        maintenance_result.last_outcome,
    );
    report.extra.insert(
        "maintenance_slice_samples".to_string(),
        maintenance_result.latencies.len().to_string(),
    );
    report.extra.insert(
        "maintenance_initial_candidate_pages".to_string(),
        initial_report.status.candidate_dead_pages.to_string(),
    );
    report.extra.insert(
        "maintenance_final_candidate_pages".to_string(),
        final_report.status.candidate_dead_pages.to_string(),
    );
    report.extra.insert(
        "maintenance_initial_candidate_bytes".to_string(),
        initial_report.candidate_reclaimable_bytes.to_string(),
    );
    report.extra.insert(
        "maintenance_final_candidate_bytes".to_string(),
        final_report.candidate_reclaimable_bytes.to_string(),
    );
    report.extra.insert(
        "maintenance_policy_min_candidate_pages".to_string(),
        final_report.policy.min_candidate_pages.to_string(),
    );
    report.extra.insert(
        "maintenance_policy_min_reusable_pages".to_string(),
        final_report.policy.min_reusable_pages.to_string(),
    );
    report.extra.insert(
        "maintenance_final_reusable_pages".to_string(),
        final_report.status.reusable_free_pages.to_string(),
    );
    report.extra.insert(
        "maintenance_final_mark_completed".to_string(),
        final_report.mark_completed.to_string(),
    );
    report.extra.insert(
        "maintenance_completed_work".to_string(),
        maintenance_result.completed_work.to_string(),
    );
    report.extra.insert(
        "maintenance_no_new_debt_slices".to_string(),
        maintenance_result.no_new_debt_slices.to_string(),
    );
    report.extra.insert(
        "maintenance_yield_count".to_string(),
        maintenance_result.yield_count.to_string(),
    );
    report.extra.insert(
        "maintenance_overrun_count".to_string(),
        maintenance_result.overrun_count.to_string(),
    );
    report.extra.insert(
        "maintenance_error_count".to_string(),
        maintenance_result.error_count.to_string(),
    );
    report.extra.insert(
        "maintenance_converged".to_string(),
        diagnostic_status.converged.to_string(),
    );
    report.extra.insert(
        "maintenance_debt_decreased".to_string(),
        diagnostic_status.debt_decreased.to_string(),
    );
    report.extra.insert(
        "maintenance_final_eligible".to_string(),
        final_report.eligible.to_string(),
    );
    report.extra.insert(
        "maintenance_final_reason".to_string(),
        final_report.reason.clone(),
    );
    report.extra.insert(
        "maintenance_run_state_progress_steps".to_string(),
        final_report.run_state.last_progress_steps.to_string(),
    );
    report.extra.insert(
        "maintenance_run_state_yield_count".to_string(),
        final_report.run_state.last_yield_count.to_string(),
    );
    report.extra.insert(
        "maintenance_run_state_overrun_count".to_string(),
        final_report.run_state.last_overrun_count.to_string(),
    );
    Ok(report)
}

fn open_filestore_with_retry(path: &Path, timeout: Duration) -> loom_core::Result<FileStore> {
    let started = Instant::now();
    loop {
        match FileStore::open(path) {
            Ok(store) => return Ok(store),
            Err(error) if error.code == Code::Conflict && started.elapsed() < timeout => {
                thread::yield_now();
            }
            Err(error) => return Err(error),
        }
    }
}

struct MaintenanceDiagnosticStatus {
    status: &'static str,
    debt_decreased: bool,
    converged: bool,
}

fn classify_maintenance_diagnostic(
    initial_candidate_pages: u64,
    final_candidate_pages: u64,
    error_count: u64,
    final_eligible: bool,
    final_reason: &str,
) -> MaintenanceDiagnosticStatus {
    let debt_decreased = final_candidate_pages < initial_candidate_pages;
    let structurally_ineligible = matches!(
        final_reason,
        "candidate_debt_below_threshold" | "free_debt_below_threshold"
    );
    let converged = final_candidate_pages == 0 || (!final_eligible && structurally_ineligible);
    let status = if error_count > 0 {
        "maintenance_errors"
    } else if !debt_decreased {
        "debt_not_decreased"
    } else if final_eligible {
        "residual_reclaimable_debt"
    } else if !converged {
        "residual_unclassified_debt"
    } else {
        "completed"
    };
    MaintenanceDiagnosticStatus {
        status,
        debt_decreased,
        converged,
    }
}

#[cfg(test)]
mod maintenance_diagnostic_status_tests {
    use super::classify_maintenance_diagnostic;

    #[test]
    fn completed_work_with_flat_debt_fails() {
        let status =
            classify_maintenance_diagnostic(10, 10, 0, false, "candidate_debt_below_threshold");

        assert_eq!(status.status, "debt_not_decreased");
        assert!(!status.debt_decreased);
    }

    #[test]
    fn maintenance_errors_fail() {
        let status =
            classify_maintenance_diagnostic(10, 0, 1, false, "candidate_debt_below_threshold");

        assert_eq!(status.status, "maintenance_errors");
        assert!(status.debt_decreased);
        assert!(status.converged);
    }

    #[test]
    fn converged_run_passes() {
        let status =
            classify_maintenance_diagnostic(10, 0, 0, false, "candidate_debt_below_threshold");

        assert_eq!(status.status, "completed");
        assert!(status.debt_decreased);
        assert!(status.converged);
    }

    #[test]
    fn reclaimable_residual_debt_fails() {
        let status = classify_maintenance_diagnostic(10, 3, 0, true, "eligible");

        assert_eq!(status.status, "residual_reclaimable_debt");
        assert!(status.debt_decreased);
        assert!(!status.converged);
    }

    #[test]
    fn structurally_ineligible_residual_pages_pass() {
        let status =
            classify_maintenance_diagnostic(10, 3, 0, false, "candidate_debt_below_threshold");

        assert_eq!(status.status, "completed");
        assert!(status.debt_decreased);
        assert!(status.converged);
    }
}

#[derive(Default)]
struct MaintenanceDiagnosticResult {
    latencies: Vec<Duration>,
    debt_samples: Vec<u64>,
    completed_work: u64,
    yield_count: u64,
    overrun_count: u64,
    error_count: u64,
    no_new_debt_slices: u64,
    final_candidate_pages: u64,
    final_candidate_bytes: u64,
    final_reusable_pages: u64,
    final_mark_completed: bool,
    final_eligible: bool,
    final_reason: String,
    last_outcome: String,
}

fn insert_latency_breakdown(report: &mut ScenarioReport, prefix: &str, latencies: &[Duration]) {
    let latency = LatencyReport::from_durations(latencies);
    report.extra.insert(
        format!("{prefix}_p50_latency_ms"),
        latency.p50_ms.unwrap_or(0.0).to_string(),
    );
    report.extra.insert(
        format!("{prefix}_p95_latency_ms"),
        latency.p95_ms.unwrap_or(0.0).to_string(),
    );
    report.extra.insert(
        format!("{prefix}_p99_latency_ms"),
        latency.p99_ms.unwrap_or(0.0).to_string(),
    );
}

fn durability_mode_scenarios(
    run_dir: &Path,
    iterations: u64,
) -> loom_core::Result<Vec<ScenarioReport>> {
    let mut reports = Vec::new();
    for durability in StoreDurabilityPolicy::ALL {
        reports.push(durability_mode_scenario(run_dir, iterations, durability)?);
    }
    Ok(reports)
}

fn durability_mode_scenario(
    run_dir: &Path,
    iterations: u64,
    durability: StoreDurabilityPolicy,
) -> loom_core::Result<ScenarioReport> {
    let mode = durability.as_str();
    let path = run_dir.join(format!("durability-{mode}.loom"));
    let mut latencies = Vec::new();
    let start = Instant::now();
    let key = overlay_key("documents", &format!("durability-{mode}"), 0)?;
    {
        let store = FileStore::create_with_profile(&path, Algo::Blake3)?;
        let mut policy = store.store_policy()?;
        policy.set_default_durability(durability)?;
        store.save_store_policy_audited(policy, None, "store.policy.set", None)?;
        for update in 0..iterations {
            let op_start = Instant::now();
            store.put_mutable_overlay_value(
                key.clone(),
                format!("{{\"mode\":\"{mode}\",\"revision\":{update}}}").into_bytes(),
            )?;
            latencies.push(op_start.elapsed());
        }
    }

    let reopened = FileStore::open(&path)?;
    let observed = reopened
        .mutable_overlay_snapshot()?
        .read_composite(&key, |_| Ok(None))?;
    let expected = if durability == StoreDurabilityPolicy::Ephemeral {
        None
    } else {
        Some(format!(
            "{{\"mode\":\"{mode}\",\"revision\":{}}}",
            iterations - 1
        ))
    };
    if observed.as_deref() != expected.as_deref().map(str::as_bytes) {
        return Err(loom_core::LoomError::new(
            Code::Internal,
            format!("durability mode {mode} reopened with unexpected current record"),
        ));
    }

    let elapsed = start.elapsed();
    let mut report = scenario_report(
        &format!("durability_mode_{mode}"),
        "completed",
        iterations,
        elapsed,
        &latencies,
        &reopened,
        &path,
        Some("current-record durability mode reopen check"),
    )?;
    report
        .extra
        .insert("configured_durability".to_string(), mode.to_string());
    report.extra.insert(
        "reopen_observed".to_string(),
        if observed.is_some() {
            "survived".to_string()
        } else {
            "lost".to_string()
        },
    );
    let behavior_scope = match durability {
        StoreDurabilityPolicy::Strict => {
            "strict current-record writes use the durable commit path and survive reopen"
        }
        StoreDurabilityPolicy::Normal => {
            "normal current-record writes use the grouped durable commit path and survive reopen"
        }
        StoreDurabilityPolicy::Relaxed => {
            "relaxed current-record writes use a durable publish path in this store profile and survive reopen"
        }
        StoreDurabilityPolicy::Ephemeral => {
            "ephemeral current-record writes remain in memory and are intentionally absent after reopen"
        }
    };
    report
        .extra
        .insert("behavior_scope".to_string(), behavior_scope.to_string());
    Ok(report)
}

fn require_completed_durability_scenarios(scenarios: &[ScenarioReport]) -> loom_core::Result<()> {
    for durability in StoreDurabilityPolicy::ALL {
        let name = format!("durability_mode_{}", durability.as_str());
        match scenarios.iter().find(|scenario| scenario.name == name) {
            Some(scenario) if scenario.status == "completed" => {}
            Some(scenario) => {
                return Err(loom_core::LoomError::new(
                    Code::Internal,
                    format!(
                        "{name} must complete after durability modes are source-backed, got {}",
                        scenario.status
                    ),
                ));
            }
            None => {
                return Err(loom_core::LoomError::new(
                    Code::Internal,
                    format!("{name} is missing from the performance report"),
                ));
            }
        }
    }
    Ok(())
}

fn scenario_report(
    name: &str,
    status: &str,
    operations: u64,
    elapsed: Duration,
    latencies: &[Duration],
    store: &FileStore,
    store_path: &Path,
    note: Option<&str>,
) -> loom_core::Result<ScenarioReport> {
    let maintenance = store.store_maintenance_report(now_ms())?;
    let attribution = store.page_class_attribution(6)?;
    let latency = LatencyReport::from_durations(latencies);
    let mut report = ScenarioReport::new(name, status);
    let compacted_bytes = match compacted_copy_physical_bytes(store_path, name) {
        Ok(bytes) => bytes,
        Err(err) => {
            report
                .extra
                .insert("compacted_bytes_unavailable".to_string(), err.to_string());
            0
        }
    };
    report.operations = operations;
    report.elapsed_ms = millis_f64(elapsed);
    report.operations_per_second = if elapsed.is_zero() {
        0.0
    } else {
        operations as f64 / elapsed.as_secs_f64()
    };
    report.p50_latency_ms = latency.p50_ms;
    report.p95_latency_ms = latency.p95_ms;
    report.p99_latency_ms = latency.p99_ms;
    report.transaction_count = maintenance.status.generation;
    report.write_lock_wait_ms = None;
    report.storage = Some(StorageReport::from_maintenance(
        &maintenance,
        compacted_bytes,
    ));
    report.stale_page_classes = attribution
        .classes
        .into_iter()
        .filter(|class| class.class.contains("free") || class.class.contains("obsolete"))
        .map(|class| PageClassReport {
            class_name: class.class,
            pages: class.pages,
            bytes: class.bytes,
        })
        .collect();
    report.growth_domains = maintenance
        .growth_domains
        .iter()
        .map(|domain| GrowthDomainReport {
            domain: domain.domain.clone(),
            current_records: domain.current_records,
            obsolete_records: domain.obsolete_records,
            payload_bytes: domain.payload_bytes,
        })
        .collect();
    if let Some(note) = note {
        report.extra.insert("note".to_string(), note.to_string());
    }
    Ok(report)
}

fn compacted_copy_physical_bytes(store_path: &Path, scenario_name: &str) -> loom_core::Result<u64> {
    let copy_path = store_path.with_file_name(format!("{scenario_name}-compacted.loom"));
    fs::copy(store_path, &copy_path).map_err(io_err)?;
    let mut compacted = FileStore::open(&copy_path)?;
    compacted.compact()?;
    let bytes = compacted.maintenance_status()?.physical_bytes;
    if bytes == 0 {
        return Err(loom_core::LoomError::new(
            Code::Internal,
            "compacted copy reported zero physical bytes",
        ));
    }
    Ok(bytes)
}

fn overlay_key(domain: &str, bucket: &str, id: u64) -> loom_core::Result<OverlayKey> {
    let id = format!("{id:016x}");
    OverlayKey::from_segments([
        b"workspace",
        &[7; 16],
        domain.as_bytes(),
        bucket.as_bytes(),
        b"current",
        id.as_bytes(),
    ])
}

fn read_u64_env(name: &str, default: u64) -> u64 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn millis_f64(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn percentile(sorted: &[Duration], percent: usize) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() - 1) * percent) / 100;
    millis_f64(sorted[index])
}

fn io_err(err: std::io::Error) -> loom_core::LoomError {
    loom_core::LoomError::new(Code::Io, err.to_string())
}

struct LatencyReport {
    p50_ms: Option<f64>,
    p95_ms: Option<f64>,
    p99_ms: Option<f64>,
}

impl LatencyReport {
    fn from_durations(durations: &[Duration]) -> Self {
        if durations.is_empty() {
            return Self {
                p50_ms: None,
                p95_ms: None,
                p99_ms: None,
            };
        }
        let mut sorted = durations.to_vec();
        sorted.sort();
        Self {
            p50_ms: Some(percentile(&sorted, 50)),
            p95_ms: Some(percentile(&sorted, 95)),
            p99_ms: Some(percentile(&sorted, 99)),
        }
    }
}

struct ArtifactPaths {
    json: PathBuf,
    summary: PathBuf,
}

struct PerformanceReport {
    command: String,
    iterations: u64,
    artifacts: ArtifactPaths,
    scenarios: Vec<ScenarioReport>,
}

impl PerformanceReport {
    fn summary(&self) -> String {
        let mut out = String::new();
        out.push_str("Loom performance harness\n");
        out.push_str(&format!("iterations: {}\n\n", self.iterations));
        for scenario in &self.scenarios {
            out.push_str(&format!(
                "{} [{}]: ops={} elapsed_ms={:.3} ops_per_sec={:.2}",
                scenario.name,
                scenario.status,
                scenario.operations,
                scenario.elapsed_ms,
                scenario.operations_per_second
            ));
            if let Some(storage) = &scenario.storage {
                out.push_str(&format!(
                    " physical_bytes={} live_bytes={} reusable_free_bytes={} compacted_bytes={}",
                    storage.physical_bytes,
                    storage.live_bytes,
                    storage.reusable_free_bytes,
                    storage.compacted_bytes
                ));
            }
            if let Some(reason) = &scenario.skip_reason {
                out.push_str(&format!(" reason={reason}"));
            }
            out.push('\n');
        }
        out
    }

    fn to_json(&self) -> String {
        let scenarios = self
            .scenarios
            .iter()
            .map(ScenarioReport::to_json)
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"command\":\"{}\",\"iterations\":{},\"artifacts\":{{\"json\":\"{}\",\"summary\":\"{}\"}},\"scenarios\":[{}]}}\n",
            json_escape(&self.command),
            self.iterations,
            json_escape(&self.artifacts.json.display().to_string()),
            json_escape(&self.artifacts.summary.display().to_string()),
            scenarios
        )
    }
}

struct ScenarioReport {
    name: String,
    status: String,
    skip_reason: Option<String>,
    operations: u64,
    elapsed_ms: f64,
    operations_per_second: f64,
    p50_latency_ms: Option<f64>,
    p95_latency_ms: Option<f64>,
    p99_latency_ms: Option<f64>,
    transaction_count: u64,
    write_lock_wait_ms: Option<f64>,
    storage: Option<StorageReport>,
    stale_page_classes: Vec<PageClassReport>,
    growth_domains: Vec<GrowthDomainReport>,
    extra: BTreeMap<String, String>,
}

impl ScenarioReport {
    fn new(name: &str, status: &str) -> Self {
        Self {
            name: name.to_string(),
            status: status.to_string(),
            skip_reason: None,
            operations: 0,
            elapsed_ms: 0.0,
            operations_per_second: 0.0,
            p50_latency_ms: None,
            p95_latency_ms: None,
            p99_latency_ms: None,
            transaction_count: 0,
            write_lock_wait_ms: None,
            storage: None,
            stale_page_classes: Vec::new(),
            growth_domains: Vec::new(),
            extra: BTreeMap::new(),
        }
    }

    fn to_json(&self) -> String {
        let storage = self
            .storage
            .as_ref()
            .map(StorageReport::to_json)
            .unwrap_or_else(|| "null".to_string());
        let stale_page_classes = self
            .stale_page_classes
            .iter()
            .map(PageClassReport::to_json)
            .collect::<Vec<_>>()
            .join(",");
        let growth_domains = self
            .growth_domains
            .iter()
            .map(GrowthDomainReport::to_json)
            .collect::<Vec<_>>()
            .join(",");
        let extra = self
            .extra
            .iter()
            .map(|(key, value)| format!("\"{}\":\"{}\"", json_escape(key), json_escape(value)))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"name\":\"{}\",\"status\":\"{}\",\"skip_reason\":{},\"operations\":{},\"elapsed_ms\":{},\"operations_per_second\":{},\"p50_latency_ms\":{},\"p95_latency_ms\":{},\"p99_latency_ms\":{},\"transaction_count\":{},\"write_lock_wait_ms\":{},\"storage\":{},\"stale_page_classes\":[{}],\"growth_domains\":[{}],\"extra\":{{{}}}}}",
            json_escape(&self.name),
            json_escape(&self.status),
            json_option_string(self.skip_reason.as_deref()),
            self.operations,
            self.elapsed_ms,
            self.operations_per_second,
            json_option_f64(self.p50_latency_ms),
            json_option_f64(self.p95_latency_ms),
            json_option_f64(self.p99_latency_ms),
            self.transaction_count,
            json_option_f64(self.write_lock_wait_ms),
            storage,
            stale_page_classes,
            growth_domains,
            extra
        )
    }
}

struct StorageReport {
    physical_bytes: u64,
    useful_live_bytes: u64,
    live_bytes: u64,
    reusable_free_bytes: u64,
    reclaimable_bytes: u64,
    metadata_bytes: u64,
    compacted_bytes: u64,
    overlay_current_records: u64,
    overlay_obsolete_records: u64,
    overlay_obsolete_pages: u64,
    retained_checkpoint_blockers: u64,
}

impl StorageReport {
    fn from_maintenance(report: &StoreMaintenanceReport, compacted_bytes: u64) -> Self {
        Self {
            physical_bytes: report.status.physical_bytes,
            useful_live_bytes: report.marked_live_bytes,
            live_bytes: report.live_bytes,
            reusable_free_bytes: report.reusable_free_bytes,
            reclaimable_bytes: report.candidate_reclaimable_bytes,
            metadata_bytes: report
                .status
                .physical_bytes
                .saturating_sub(report.live_bytes),
            compacted_bytes,
            overlay_current_records: report.overlay_health.current_record_count,
            overlay_obsolete_records: report.overlay_obsolete_record_count,
            overlay_obsolete_pages: report.overlay_obsolete_page_count,
            retained_checkpoint_blockers: report.retained_control_roots,
        }
    }

    fn to_json(&self) -> String {
        format!(
            "{{\"physical_bytes\":{},\"useful_live_bytes\":{},\"live_bytes\":{},\"reusable_free_bytes\":{},\"reclaimable_bytes\":{},\"metadata_bytes\":{},\"compacted_bytes\":{},\"overlay_current_records\":{},\"overlay_obsolete_records\":{},\"overlay_obsolete_pages\":{},\"retained_checkpoint_blockers\":{}}}",
            self.physical_bytes,
            self.useful_live_bytes,
            self.live_bytes,
            self.reusable_free_bytes,
            self.reclaimable_bytes,
            self.metadata_bytes,
            self.compacted_bytes,
            self.overlay_current_records,
            self.overlay_obsolete_records,
            self.overlay_obsolete_pages,
            self.retained_checkpoint_blockers
        )
    }
}

struct PageClassReport {
    class_name: String,
    pages: u64,
    bytes: u64,
}

impl PageClassReport {
    fn to_json(&self) -> String {
        format!(
            "{{\"class\":\"{}\",\"pages\":{},\"bytes\":{}}}",
            json_escape(&self.class_name),
            self.pages,
            self.bytes
        )
    }
}

struct GrowthDomainReport {
    domain: String,
    current_records: u64,
    obsolete_records: u64,
    payload_bytes: u64,
}

impl GrowthDomainReport {
    fn to_json(&self) -> String {
        format!(
            "{{\"domain\":\"{}\",\"current_records\":{},\"obsolete_records\":{},\"payload_bytes\":{}}}",
            json_escape(&self.domain),
            self.current_records,
            self.obsolete_records,
            self.payload_bytes
        )
    }
}

fn json_option_string(value: Option<&str>) -> String {
    value
        .map(|value| format!("\"{}\"", json_escape(value)))
        .unwrap_or_else(|| "null".to_string())
}

fn json_option_f64(value: Option<f64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string())
}

fn json_escape(value: &str) -> String {
    value
        .chars()
        .flat_map(|ch| match ch {
            '"' => "\\\"".chars().collect::<Vec<_>>(),
            '\\' => "\\\\".chars().collect::<Vec<_>>(),
            '\n' => "\\n".chars().collect::<Vec<_>>(),
            '\r' => "\\r".chars().collect::<Vec<_>>(),
            '\t' => "\\t".chars().collect::<Vec<_>>(),
            ch => vec![ch],
        })
        .collect()
}
