//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

pub(crate) fn run_daemon(action: DaemonCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        DaemonCmd::Start { store, transport } => daemon_start_with_transport(&store, &transport),
        DaemonCmd::Stop {
            store,
            force,
            wait,
            hard,
        } => daemon_stop(
            &store,
            daemon::StopOptions {
                force,
                hard,
                wait_ms: wait,
            },
            keys,
        ),
        DaemonCmd::Restart { store, transport } => {
            match daemon_stop(&store, daemon::StopOptions::default(), keys) {
                Ok(()) => {}
                Err(e) if e.contains("not running") => {}
                Err(e) => return Err(e),
            }
            daemon_start_with_transport(&store, &transport)
        }
        DaemonCmd::Status { store, json } => daemon_status(&store, json),
        DaemonCmd::Maintenance { action } => run_daemon_maintenance(action, keys),
        DaemonCmd::Session { action } => match action {
            DaemonSessionCmd::Attach { store, session } => {
                daemon_session(&store, "attach", &session, keys)
            }
            DaemonSessionCmd::Detach { store, session } => {
                daemon_session(&store, "detach", &session, keys)
            }
        },
        DaemonCmd::Pin { action } => match action {
            DaemonPinCmd::Add { store, pin } => daemon_pin_with_keys(&store, &pin, keys),
            DaemonPinCmd::Remove { store, pin } => daemon_unpin_with_keys(&store, &pin, keys),
        },
        DaemonCmd::Run {
            store,
            addr_file,
            pid_file,
            lock_file,
            transport,
        } => daemon_run(&store, &addr_file, &pid_file, &lock_file, &transport),
    }
}

#[cfg(feature = "mcp")]
/// How `loom mcp` should launch for a resolved locator. A local target keeps the existing local
/// `StoreAccess` path; a remote target serves the MCP surface backed by a remote Loom.
#[cfg(feature = "mcp")]
#[derive(Debug)]
enum McpLaunchTarget {
    Local,
    #[cfg_attr(not(feature = "remote-client"), allow(dead_code))]
    Remote(loom_locator::RemoteTarget),
}

/// Resolve the `loom mcp` locator to a launch target. `--stateless` is a local-only MCP mode, so a
/// remote locator combined with `--stateless` is refused immediately, before any connection.
#[cfg(feature = "mcp")]
fn resolve_mcp_target(
    cx: &crate::locator_cx::LocatorContext,
    locator: &str,
    stateless: bool,
) -> Result<McpLaunchTarget, String> {
    match cx.resolve_target(locator)? {
        loom_locator::Target::Local(_) => Ok(McpLaunchTarget::Local),
        loom_locator::Target::Remote(target) => {
            if stateless {
                return Err(format!(
                    "--stateless applies only to a local MCP host; the remote endpoint {} manages session statefulness on the server",
                    target.url
                ));
            }
            Ok(McpLaunchTarget::Remote(target))
        }
    }
}

#[cfg(feature = "mcp")]
pub(crate) fn run_mcp(
    store: &str,
    workspace: Option<String>,
    collection: Option<String>,
    http: Option<String>,
    network_access: Option<String>,
    stateless: bool,
    keys: &KeyOpts,
) -> Result<(), String> {
    if network_access.is_some() && http.is_none() {
        return Err("--network-access requires --http".to_string());
    }
    let launch = resolve_mcp_target(crate::locator_cx::current(), store, stateless)?;
    let binding = uldren_loom_mcp::server::Binding {
        workspace,
        collection,
        ..Default::default()
    };
    let access = match launch {
        McpLaunchTarget::Local => {
            let auth = mount_open_auth(store, keys)?;
            match uldren_loom_mcp::StoreAccess::per_request_attached_auth(store, auth.clone()) {
                Ok(access) => access,
                Err(e) if e.code == loom_core::error::Code::NotFound => {
                    uldren_loom_mcp::StoreAccess::per_request_auth(store, auth)
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        McpLaunchTarget::Remote(_target) => {
            if network_access.is_some() {
                return Err(
                    "--network-access is a local store policy and does not apply to a remote MCP store"
                        .to_string(),
                );
            }
            #[cfg(feature = "remote-client")]
            {
                let backend = crate::remote::McpRemoteBackend::connect(&_target)?;
                uldren_loom_mcp::StoreAccess::remote(std::sync::Arc::new(backend))
            }
            #[cfg(not(feature = "remote-client"))]
            {
                return Err(
                    "remote MCP requires a build with the `remote-client` feature".to_string(),
                );
            }
        }
    };
    let mcp = uldren_loom_mcp::LoomMcp::new(access);
    match http {
        // Streamable HTTP: any-node-serves-any-request when `--stateless`, else a live session per
        // client (subscriptions push, progress streams). Needs the IO + multi-thread runtime.
        Some(addr) => {
            let addr: std::net::SocketAddr = addr
                .parse()
                .map_err(|e| format!("invalid --http address '{addr}': {e}"))?;
            let network_access = match network_access {
                Some(name) => Some(mcp_http_network_access(store, &name, addr)?),
                None => None,
            };
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?;
            runtime
                .block_on(uldren_loom_mcp::server::serve_http_with_network_access(
                    mcp,
                    addr,
                    binding,
                    !stateless,
                    network_access,
                ))
                .map_err(|e| e.to_string())
        }
        // stdio (default): a single owner-mode session over the process pipes.
        None => {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .map_err(|e| e.to_string())?;
            runtime
                .block_on(uldren_loom_mcp::server::serve_stdio(mcp, binding))
                .map_err(|e| e.to_string())
        }
    }
}

#[cfg(feature = "mcp")]
fn mcp_http_network_access(
    store: &str,
    policy_name: &str,
    addr: std::net::SocketAddr,
) -> Result<uldren_loom_mcp::server::HttpNetworkAccess, String> {
    let fs = FileStore::open_read(store).map_err(|e| e.to_string())?;
    let policy = fs
        .network_access_policy(policy_name)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("network access policy {policy_name:?} not found"))?;
    if served_listener_network_access_policy_requires_mtls(&policy) {
        return Err(format!(
            "network access policy {policy_name:?} requires mTLS but `loom mcp --http` does not terminate direct TLS"
        ));
    }
    let policy = loom_hosted::HostedNetworkAccessPolicy::from_record_for_listener(
        Some(format!("mcp-http;bind={addr}")),
        policy,
    );
    let denied_audit = network_access_denied_audit_sink(store.to_string());
    Ok(std::sync::Arc::new(
        move |peer_addr, x_forwarded_for, forwarded| {
            loom_hosted::network_access_allows_with_denied_audit(
                Some(&policy),
                peer_addr,
                None,
                x_forwarded_for,
                forwarded,
                Some(&denied_audit),
            )
        },
    ))
}

pub(crate) fn run_lock(action: LockCmd, keys: &KeyOpts) -> Result<(), String> {
    let auth = daemon_auth_from_keys(keys)?;
    match action {
        LockCmd::Acquire {
            store,
            key,
            principal,
            session,
            mode,
            permits,
            capacity,
            lease_ms,
            wait,
            no_wait,
        } => {
            let mode =
                daemon::parse_lock_mode(&mode, permits, capacity).map_err(|e| e.to_string())?;
            if no_wait && wait.is_some() {
                return Err("--no-wait cannot be combined with --wait".to_string());
            }
            let wait_ms = if no_wait {
                0
            } else {
                wait.unwrap_or(daemon::DEFAULT_LOCK_WAIT_MS)
            };
            let response = daemon::lock_acquire_auth(
                &daemon::paths(&store).map_err(|e| e.to_string())?,
                daemon::AcquireRequest {
                    key: &key,
                    principal: &principal,
                    session: &session,
                    mode,
                    lease_ms,
                    wait_ms,
                    now_ms: now_ms(),
                },
                &auth,
            )
            .map_err(|e| e.to_string())?;
            print!("{response}");
            Ok(())
        }
        LockCmd::Refresh {
            store,
            key,
            principal,
            session,
            mode,
            permits,
            capacity,
            fence,
            lease_ms,
        } => {
            let mode =
                daemon::parse_lock_mode(&mode, permits, capacity).map_err(|e| e.to_string())?;
            let response = daemon::lock_refresh_auth(
                &daemon::paths(&store).map_err(|e| e.to_string())?,
                daemon::RefreshRequest {
                    key: &key,
                    principal: &principal,
                    session: &session,
                    mode,
                    fence: loom_core::Fence::embedded(fence),
                    lease_ms,
                    now_ms: now_ms(),
                },
                &auth,
            )
            .map_err(|e| e.to_string())?;
            print!("{response}");
            Ok(())
        }
        LockCmd::Release {
            store,
            key,
            principal,
            session,
            mode,
            permits,
            capacity,
            fence,
        } => {
            let mode =
                daemon::parse_lock_mode(&mode, permits, capacity).map_err(|e| e.to_string())?;
            let response = daemon::lock_release_auth(
                &daemon::paths(&store).map_err(|e| e.to_string())?,
                daemon::ReleaseRequest {
                    key: &key,
                    principal: &principal,
                    session: &session,
                    mode,
                    fence: loom_core::Fence::embedded(fence),
                    now_ms: now_ms(),
                },
                &auth,
            )
            .map_err(|e| e.to_string())?;
            print!("{response}");
            Ok(())
        }
    }
}

fn daemon_auth_from_keys(keys: &KeyOpts) -> Result<daemon::DaemonAuth, String> {
    let Some((principal, passphrase)) = acquire_auth_session(keys)? else {
        return Ok(daemon::DaemonAuth::default());
    };
    Ok(daemon::DaemonAuth {
        principal: Some(principal.to_string()),
        passphrase: Some(passphrase),
        session: Some(session_id()),
    })
}

pub(crate) fn daemon_session(
    store: &str,
    action: &str,
    session: &str,
    keys: &KeyOpts,
) -> Result<(), String> {
    let paths = daemon::paths(store).map_err(|e| e.to_string())?;
    let auth = daemon_auth_from_keys(keys)?;
    let response = match action {
        "attach" => daemon::session_attach_auth(&paths, session, &auth),
        "detach" => daemon::session_detach_auth(&paths, session, &auth),
        _ => Err(loom_core::error::LoomError::invalid(
            "unknown daemon session action",
        )),
    }
    .map_err(|e| e.to_string())?;
    print!("{response}");
    Ok(())
}

pub(crate) fn daemon_pin_with_keys(store: &str, pin: &str, keys: &KeyOpts) -> Result<(), String> {
    let paths = daemon::paths(store).map_err(|e| e.to_string())?;
    let auth = daemon_auth_from_keys(keys)?;
    let response = daemon::pin_add_auth(&paths, pin, &auth).map_err(|e| e.to_string())?;
    print!("{response}");
    Ok(())
}

pub(crate) fn daemon_unpin_with_keys(store: &str, pin: &str, keys: &KeyOpts) -> Result<(), String> {
    let paths = daemon::paths(store).map_err(|e| e.to_string())?;
    let auth = daemon_auth_from_keys(keys)?;
    let response = daemon::pin_remove_auth(&paths, pin, &auth).map_err(|e| e.to_string())?;
    print!("{response}");
    Ok(())
}

#[cfg(any(feature = "fuse", feature = "nfs"))]
pub(crate) fn daemon_unpin(store: &str, pin: &str) -> Result<(), String> {
    let paths = daemon::paths(store).map_err(|e| e.to_string())?;
    let response = daemon::pin_remove(&paths, pin).map_err(|e| e.to_string())?;
    print!("{response}");
    Ok(())
}

fn run_daemon_maintenance(action: DaemonMaintenanceCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        DaemonMaintenanceCmd::Status { store, json } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let report = loom
                .store()
                .store_maintenance_report(now_ms())
                .map_err(|e| e.to_string())?;
            let diagnostics = maintenance_live_root_diagnostics(&loom)?;
            print_maintenance_report(&report, Some(&diagnostics), json)
        }
        DaemonMaintenanceCmd::Policy {
            store,
            min_candidate_pages,
            min_reusable_pages,
            interval_ms,
            backoff_ms,
            max_segments,
            max_pages,
            allow_full_compaction,
            disallow_full_compaction,
            enable_tail_trim,
            disable_tail_trim,
            enable_tail_compaction,
            disable_tail_compaction,
            tail_compaction_max_pages,
            tail_compaction_max_objects,
            tail_compaction_max_bytes,
            tail_compaction_interval_ms,
            tail_compaction_backoff_ms,
        } => {
            let loom = cli_open_loom(&store, keys)?;
            let mut policy = loom
                .store()
                .store_maintenance_policy()
                .map_err(|e| e.to_string())?;
            if let Some(value) = min_candidate_pages {
                policy.min_candidate_pages = value;
            }
            if let Some(value) = min_reusable_pages {
                policy.min_reusable_pages = value;
            }
            if let Some(value) = interval_ms {
                policy.interval_ms = value;
            }
            if let Some(value) = backoff_ms {
                policy.backoff_ms = value;
            }
            if let Some(value) = max_segments {
                policy.max_segments = value;
            }
            if let Some(value) = max_pages {
                policy.max_pages = value;
            }
            if allow_full_compaction {
                policy.full_compaction_enabled = true;
            }
            if disallow_full_compaction {
                policy.full_compaction_enabled = false;
            }
            if enable_tail_trim {
                policy.tail_trim_enabled = true;
            }
            if disable_tail_trim {
                policy.tail_trim_enabled = false;
            }
            if enable_tail_compaction {
                policy.tail_compaction_enabled = true;
            }
            if disable_tail_compaction {
                policy.tail_compaction_enabled = false;
            }
            if let Some(value) = tail_compaction_max_pages {
                policy.tail_compaction_max_pages = value;
            }
            if let Some(value) = tail_compaction_max_objects {
                policy.tail_compaction_max_objects = value;
            }
            if let Some(value) = tail_compaction_max_bytes {
                policy.tail_compaction_max_bytes = value;
            }
            if let Some(value) = tail_compaction_interval_ms {
                policy.tail_compaction_interval_ms = value;
            }
            if let Some(value) = tail_compaction_backoff_ms {
                policy.tail_compaction_backoff_ms = value;
            }
            loom.store()
                .set_store_maintenance_policy(policy)
                .map_err(|e| e.to_string())?;
            let report = loom
                .store()
                .store_maintenance_report(now_ms())
                .map_err(|e| e.to_string())?;
            let diagnostics = maintenance_live_root_diagnostics(&loom)?;
            print_maintenance_report(&report, Some(&diagnostics), false)
        }
        DaemonMaintenanceCmd::Run {
            store,
            max_segments,
            max_pages,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let outcome =
                run_store_maintenance_once(&mut loom, now_ms(), true, max_segments, max_pages)?;
            println!("{outcome}");
            Ok(())
        }
    }
}

fn print_maintenance_report(
    report: &StoreMaintenanceReport,
    diagnostics: Option<&LiveRootDiagnostics>,
    json: bool,
) -> Result<(), String> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&maintenance_report_json(report, diagnostics))
                .map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    print!("{}", maintenance_report_text(report, diagnostics));
    Ok(())
}

fn maintenance_report_text(
    report: &StoreMaintenanceReport,
    diagnostics: Option<&LiveRootDiagnostics>,
) -> String {
    let mut text = format!(
        "maintenance\teligible={}\treason={}\tphysical_bytes={}\tmarked_live_bytes={}\tcandidate_reclaimable_bytes={}\treusable_free_bytes={}\ttail_free_pages={}\ttail_free_bytes={}\ttail_trim_eligible={}\ttail_blocked_by_live_objects={}\ttail_compaction_eligible={}\tfull_compaction_required_for_shrink={}\ttail_trim_attempted={}\ttail_trim_pages={}\ttail_trim_bytes={}\ttail_compaction_attempted={}\ttail_compaction_relocated_objects={}\ttail_compaction_relocated_pages={}\ttail_compaction_relocated_bytes={}\ttail_compaction_truncated_pages={}\ttail_compaction_conflicts={}\tretained_control_roots={}\tderived_payload_count={}\tlast_shrink_skip_reason={}",
        report.eligible,
        report.reason,
        report.status.physical_bytes,
        report.marked_live_bytes,
        report.candidate_reclaimable_bytes,
        report.reusable_free_bytes,
        report.tail_free_pages,
        report.tail_free_bytes,
        report.tail_trim_eligible,
        report.tail_blocked_by_live_objects,
        report.tail_compaction_eligible,
        report.full_compaction_required_for_shrink,
        report.tail_trim_attempted,
        report.tail_trim_pages,
        report.tail_trim_bytes,
        report.tail_compaction_attempted,
        report.tail_compaction_relocated_objects,
        report.tail_compaction_relocated_pages,
        report.tail_compaction_relocated_bytes,
        report.tail_compaction_truncated_pages,
        report.tail_compaction_conflicts,
        report.retained_control_roots,
        report.derived_payload_count,
        report.last_shrink_skip_reason.as_deref().unwrap_or("none")
    ) + "\n"
        + &format!(
            "maintenance_policy\tmin_candidate_pages={}\tmin_reusable_pages={}\tinterval_ms={}\tbackoff_ms={}\tmax_segments={}\tmax_pages={}\tfull_compaction_enabled={}\ttail_trim_enabled={}\ttail_compaction_enabled={}\ttail_compaction_max_pages={}\ttail_compaction_max_objects={}\ttail_compaction_max_bytes={}\ttail_compaction_interval_ms={}\ttail_compaction_backoff_ms={}",
            report.policy.min_candidate_pages,
            report.policy.min_reusable_pages,
            report.policy.interval_ms,
            report.policy.backoff_ms,
            report.policy.max_segments,
            report.policy.max_pages,
            report.policy.full_compaction_enabled,
            report.policy.tail_trim_enabled,
            report.policy.tail_compaction_enabled,
            report.policy.tail_compaction_max_pages,
            report.policy.tail_compaction_max_objects,
            report.policy.tail_compaction_max_bytes,
            report.policy.tail_compaction_interval_ms,
            report.policy.tail_compaction_backoff_ms
        )
        + "\n"
        + &format!(
            "maintenance_epoch\tepoch={}\tcompleted={}\tmarked_live_objects={}\tlast_validated_mark_epoch={}",
            report
                .mark_epoch
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            report.mark_completed,
            report.marked_live_objects,
            report.status.last_validated_mark_epoch
        )
        + "\n"
        + &format!(
            "maintenance_run\tlast_run_ms={}\tnext_eligible_ms={}\tlast_skip_reason={}\tlast_error={}",
            optional_u64_text(report.run_state.last_run_ms),
            report.run_state.next_eligible_ms,
            optional_text(report.run_state.last_skip_reason.as_deref()),
            optional_text(report.run_state.last_error.as_deref())
        )
        + "\n";
    if let Some(diagnostics) = diagnostics {
        for class in &diagnostics.classes {
            let examples = class
                .examples
                .iter()
                .map(|example| format!("{}={}", example.id, example.digest))
                .collect::<Vec<_>>()
                .join(",");
            text.push_str(&format!(
                "maintenance_live_roots\tclass={}\tcount={}\ttruncated={}\texamples={}\n",
                class.class,
                class.count,
                class.truncated,
                if examples.is_empty() {
                    "none".to_string()
                } else {
                    examples
                }
            ));
        }
    }
    text
}

fn print_live_root_diagnostics(prefix: &str, diagnostics: &LiveRootDiagnostics) {
    for class in &diagnostics.classes {
        let examples = class
            .examples
            .iter()
            .map(|example| format!("{}={}", example.id, example.digest))
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{prefix}\tclass={}\tcount={}\ttruncated={}\texamples={}",
            class.class,
            class.count,
            class.truncated,
            if examples.is_empty() {
                "none".to_string()
            } else {
                examples
            }
        );
    }
}

fn open_read_loom_for_diagnostics(
    store: &str,
    fs: &FileStore,
    keys: &KeyOpts,
) -> Result<Loom<FileStore>, String> {
    let key = if fs.is_encrypted() {
        Some(acquire_key_spec(&keys.source, "key", false)?)
    } else {
        None
    };
    open_loom_read_unlocked(store, key.as_ref()).map_err(|e| e.to_string())
}

fn run_store_maintenance_once(
    loom: &mut Loom<FileStore>,
    now: u64,
    manual: bool,
    max_segments: Option<u64>,
    max_pages: Option<u64>,
) -> Result<String, String> {
    let mut policy = loom
        .store()
        .store_maintenance_policy()
        .map_err(|e| e.to_string())?;
    if let Some(value) = max_segments {
        if value == 0 {
            return Err("max-segments must be nonzero".to_string());
        }
        policy.max_segments = value;
    }
    if let Some(value) = max_pages {
        if value == 0 {
            return Err("max-pages must be nonzero".to_string());
        }
        policy.max_pages = value;
    }
    let report = loom
        .store()
        .store_maintenance_report(now)
        .map_err(|e| e.to_string())?;
    let whole_file_allowed =
        policy.full_compaction_enabled && maintenance_debt_thresholds_met(&policy, &report.status);
    if !manual && !report.eligible {
        return Ok("maintenance\tskipped\treason=not_eligible".to_string());
    }
    let mut active = loom
        .store()
        .active_reachability_mark_epoch()
        .map_err(|e| e.to_string())?;
    if let Some(epoch) = &active
        && let Err(error) = loom.store().validate_reachability_mark_epoch_current(epoch)
    {
        if error.code != loom_core::error::Code::Conflict {
            return Err(error.to_string());
        }
        loom.store()
            .clear_reachability_mark_epoch()
            .map_err(|e| e.to_string())?;
        active = None;
    }
    let needs_mark = active
        .as_ref()
        .map(|epoch| !epoch.state.completed)
        .unwrap_or(true);
    if needs_mark {
        if active.is_none() {
            loom_store::begin_loom_reachability_mark_epoch(loom).map_err(|e| e.to_string())?;
        }
        let step =
            loom_store::step_loom_reachability_mark_epoch(loom, 1024).map_err(|e| e.to_string())?;
        if !step.completed {
            let state = StoreMaintenanceRunState {
                last_run_ms: Some(now),
                next_eligible_ms: now.saturating_add(policy.interval_ms),
                last_skip_reason: Some("mark_epoch_incomplete".to_string()),
                last_error: None,
                ..StoreMaintenanceRunState::default()
            };
            loom.store()
                .record_store_maintenance_run_state(state)
                .map_err(|e| e.to_string())?;
            return Ok(format!(
                "maintenance\tmarked\tvisited={}\tpending={}",
                step.visited, step.pending
            ));
        }
    }
    let mut tail_trim_attempted = false;
    let mut tail_trim_pages = 0;
    let mut tail_trim_bytes = 0;
    let mut tail_compaction = loom_store::TailCompactionStats::default();
    let outcome = if whole_file_allowed {
        let capacity = loom
            .store()
            .ensure_compaction_capacity()
            .map_err(|e| e.to_string())?;
        let stats = gc_loom(loom).map_err(|e| e.to_string())?;
        format!(
            "maintenance\tcompacted\tbefore={}\tafter={}\treclaimed={}\trequired_temp_bytes={}\tavailable_temp_bytes={}",
            stats.before,
            stats.after,
            stats.reclaimed(),
            capacity.required_temp_bytes,
            optional_u64_text(capacity.available_temp_bytes)
        )
    } else {
        let budget = GcSegmentBudget {
            max_segments: policy.max_segments,
            max_pages: policy.max_pages,
        };
        let stats = if policy.tail_trim_enabled {
            tail_trim_attempted = true;
            loom.store_mut().gc_validated_segments(budget)
        } else {
            loom.store_mut()
                .gc_validated_segments_without_tail_trim(budget)
        }
        .map_err(|e| e.to_string())?;
        tail_trim_pages = stats.pages_trimmed;
        tail_trim_bytes = stats
            .pages_trimmed
            .saturating_mul(loom_store::STORE_PAGE_SIZE);
        if policy.tail_compaction_enabled {
            tail_compaction = loom
                .store_mut()
                .compact_tail_once(
                    policy.tail_compaction_max_pages,
                    policy.tail_compaction_max_objects,
                    policy.tail_compaction_max_bytes,
                )
                .map_err(|e| e.to_string())?;
            if tail_compaction.truncated_pages > 0 {
                tail_trim_attempted = true;
                tail_trim_pages = tail_trim_pages.saturating_add(tail_compaction.truncated_pages);
                tail_trim_bytes = tail_trim_pages.saturating_mul(loom_store::STORE_PAGE_SIZE);
            }
        }
        format!(
            "maintenance\treclaimed\tsegments_reclaimed={}\tpages_freed={}\ttail_trim_pages={}\ttail_trim_bytes={}\ttail_compaction_attempted={}\ttail_compaction_relocated_objects={}\ttail_compaction_relocated_pages={}\ttail_compaction_truncated_pages={}\tobjects_relocated={}\tobjects_dropped={}",
            stats.segments_reclaimed,
            stats.pages_freed,
            tail_trim_pages,
            tail_trim_bytes,
            tail_compaction.attempted,
            tail_compaction.relocated_objects,
            tail_compaction.relocated_pages,
            tail_compaction.truncated_pages,
            stats.objects_relocated,
            stats.objects_dropped
        )
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
        last_shrink_skip_reason: tail_compaction
            .skipped
            .then(|| "tail_compaction_skipped".to_string()),
    };
    loom.store()
        .record_store_maintenance_run_state(state)
        .map_err(|e| e.to_string())?;
    Ok(outcome)
}

fn maintenance_debt_thresholds_met(
    policy: &loom_store::StoreMaintenancePolicy,
    status: &loom_store::MaintenanceStatus,
) -> bool {
    status.candidate_dead_pages >= policy.min_candidate_pages
        && status.reusable_free_pages >= policy.min_reusable_pages
}

fn maintenance_live_root_diagnostics(
    loom: &Loom<FileStore>,
) -> Result<LiveRootDiagnostics, String> {
    let mut extra_roots = Vec::new();
    let derived_roots = loom
        .store()
        .derived_artifact_roots()
        .map_err(|e| e.to_string())?;
    for (idx, root) in derived_roots.into_iter().enumerate() {
        extra_roots.push(("derived_artifact_roots", format!("derived:{idx}"), root));
    }
    if let Some(epoch) = loom
        .store()
        .active_reachability_mark_epoch()
        .map_err(|e| e.to_string())?
    {
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
        .map_err(|e| e.to_string())
}

fn live_root_diagnostics_json(diagnostics: &LiveRootDiagnostics) -> serde_json::Value {
    serde_json::json!({
        "sample_limit": diagnostics.sample_limit,
        "classes": diagnostics.classes.iter().map(|class| {
            serde_json::json!({
                "class": class.class,
                "count": class.count,
                "examples": class.examples.iter().map(|example| {
                    serde_json::json!({
                        "id": example.id,
                        "digest": example.digest.to_string(),
                    })
                }).collect::<Vec<_>>(),
                "truncated": class.truncated,
            })
        }).collect::<Vec<_>>(),
    })
}

fn maintenance_report_json(
    report: &StoreMaintenanceReport,
    diagnostics: Option<&LiveRootDiagnostics>,
) -> serde_json::Value {
    let policy = serde_json::json!({
        "min_candidate_pages": report.policy.min_candidate_pages,
        "min_reusable_pages": report.policy.min_reusable_pages,
        "interval_ms": report.policy.interval_ms,
        "backoff_ms": report.policy.backoff_ms,
        "max_segments": report.policy.max_segments,
        "max_pages": report.policy.max_pages,
        "full_compaction_enabled": report.policy.full_compaction_enabled,
        "tail_trim_enabled": report.policy.tail_trim_enabled,
        "tail_compaction_enabled": report.policy.tail_compaction_enabled,
        "tail_compaction_max_pages": report.policy.tail_compaction_max_pages,
        "tail_compaction_max_objects": report.policy.tail_compaction_max_objects,
        "tail_compaction_max_bytes": report.policy.tail_compaction_max_bytes,
        "tail_compaction_interval_ms": report.policy.tail_compaction_interval_ms,
        "tail_compaction_backoff_ms": report.policy.tail_compaction_backoff_ms,
    });
    let run_state = serde_json::json!({
        "last_run_ms": report.run_state.last_run_ms,
        "next_eligible_ms": report.run_state.next_eligible_ms,
        "last_skip_reason": report.run_state.last_skip_reason,
        "last_error": report.run_state.last_error,
        "last_tail_trim_attempted": report.run_state.last_tail_trim_attempted,
        "last_tail_trim_pages": report.run_state.last_tail_trim_pages,
        "last_tail_trim_bytes": report.run_state.last_tail_trim_bytes,
        "last_tail_compaction_attempted": report.run_state.last_tail_compaction_attempted,
        "last_tail_compaction_relocated_objects": report.run_state.last_tail_compaction_relocated_objects,
        "last_tail_compaction_relocated_pages": report.run_state.last_tail_compaction_relocated_pages,
        "last_tail_compaction_relocated_bytes": report.run_state.last_tail_compaction_relocated_bytes,
        "last_tail_compaction_truncated_pages": report.run_state.last_tail_compaction_truncated_pages,
        "last_tail_compaction_conflicts": report.run_state.last_tail_compaction_conflicts,
        "last_shrink_skip_reason": report.run_state.last_shrink_skip_reason,
    });
    let mut value = serde_json::json!({
        "eligible": report.eligible,
        "reason": report.reason,
        "physical_bytes": report.status.physical_bytes,
        "reusable_free_pages": report.status.reusable_free_pages,
        "candidate_dead_pages": report.status.candidate_dead_pages,
        "candidate_reclaimable_bytes": report.candidate_reclaimable_bytes,
        "reusable_free_bytes": report.reusable_free_bytes,
        "tail_free_pages": report.tail_free_pages,
        "tail_free_bytes": report.tail_free_bytes,
        "tail_trim_eligible": report.tail_trim_eligible,
        "tail_blocked_by_live_objects": report.tail_blocked_by_live_objects,
        "tail_compaction_eligible": report.tail_compaction_eligible,
        "full_compaction_required_for_shrink": report.full_compaction_required_for_shrink,
        "tail_trim_attempted": report.tail_trim_attempted,
        "tail_trim_pages": report.tail_trim_pages,
        "tail_trim_bytes": report.tail_trim_bytes,
        "tail_compaction_attempted": report.tail_compaction_attempted,
        "tail_compaction_relocated_objects": report.tail_compaction_relocated_objects,
        "tail_compaction_relocated_pages": report.tail_compaction_relocated_pages,
        "tail_compaction_relocated_bytes": report.tail_compaction_relocated_bytes,
        "tail_compaction_truncated_pages": report.tail_compaction_truncated_pages,
        "tail_compaction_conflicts": report.tail_compaction_conflicts,
        "last_shrink_skip_reason": report.last_shrink_skip_reason,
        "mark_epoch": report.mark_epoch,
        "mark_completed": report.mark_completed,
        "marked_live_objects": report.marked_live_objects,
        "marked_live_bytes": report.marked_live_bytes,
        "last_validated_mark_epoch": report.status.last_validated_mark_epoch,
        "retained_control_roots": report.retained_control_roots,
        "derived_payload_count": report.derived_payload_count,
        "policy": policy,
        "run_state": run_state,
    });
    if let Some(diagnostics) = diagnostics {
        value["live_root_diagnostics"] = live_root_diagnostics_json(diagnostics);
    }
    value
}

fn optional_u64_text(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string())
}

fn optional_text(value: Option<&str>) -> &str {
    value.unwrap_or("none")
}

fn parse_optional_u64_field(value: &str) -> Result<Option<u64>, String> {
    if value == "none" {
        return Ok(None);
    }
    value
        .parse::<u64>()
        .map(Some)
        .map_err(|_| format!("invalid optional u64 field {value:?}"))
}

pub(crate) fn daemon_doctor(store: &str, _keys: &KeyOpts) -> Result<(), String> {
    let paths = daemon::paths(store).map_err(|e| e.to_string())?;
    println!("store\t{}", paths.store);
    println!("runtime_dir\t{}", daemon::runtime_dir().display());
    println!("addr_file\t{}", paths.addr_file.display());
    println!("pid_file\t{}", paths.pid_file.display());
    println!("lock_file\t{}", paths.lock_file.display());
    println!("sock_file\t{}", paths.sock_file.display());
    println!("pipe_name\t{}", paths.pipe_name);
    print_daemon_transport_capabilities();
    print_runtime_artifact("addr_file_state", &paths.addr_file);
    print_runtime_artifact("pid_file_state", &paths.pid_file);
    print_runtime_artifact("lock_file_state", &paths.lock_file);
    print_runtime_artifact("sock_file_state", &paths.sock_file);
    match daemon::validate_runtime_artifacts(&paths) {
        Ok(()) => println!("runtime_artifacts_trusted\tok"),
        Err(e) => println!("runtime_artifacts_trusted\terror\t{e}"),
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(
            paths
                .addr_file
                .parent()
                .unwrap_or(paths.addr_file.as_path()),
        ) {
            Ok(meta) if meta.permissions().mode() & 0o077 == 0 => {
                println!("runtime_dir_private\tok")
            }
            Ok(meta) => println!(
                "runtime_dir_private\terror\tmode={:o}",
                meta.permissions().mode() & 0o777
            ),
            Err(e) => println!("runtime_dir_private\terror\t{e}"),
        }
    }
    match std::net::TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => {
            let addr = listener
                .local_addr()
                .map(|addr| addr.to_string())
                .unwrap_or_else(|_| "unknown".to_string());
            println!("loopback_bind\tok\t{addr}");
        }
        Err(e) => println!("loopback_bind\terror\t{e}"),
    }
    let endpoint = match daemon::daemon_transport_endpoint(&paths) {
        Ok(endpoint) => {
            println!(
                "daemon_endpoint\tok\ttransport={}\tsecurity={}\tprofile={}\taddr={addr}",
                endpoint.transport().wire_name(),
                endpoint.security().wire_name(),
                daemon_transport_profile(endpoint.transport()),
                addr = endpoint.label()
            );
            Some(endpoint)
        }
        Err(e) => {
            println!("daemon_endpoint\terror\t{e}");
            None
        }
    };
    match daemon::status_response(&paths) {
        Ok(status) => {
            match &endpoint {
                Some(endpoint) => println!("host_daemon_reachable\tok\taddr={}", endpoint.label()),
                None => println!("host_daemon_reachable\tok"),
            }
            println!(
                "daemon\trunning\tprotocol={}\ttransport={}\tsecurity={}\tprofile={}\tpid={}\tstore={}\tidentity={}",
                daemon::PROTOCOL,
                status.transport.wire_name(),
                status.transport.security().wire_name(),
                daemon_transport_profile(status.transport),
                status.pid,
                status.store,
                status.store_id
            );
            print_daemon_pin_status(&status);
        }
        Err(e) => {
            if endpoint.is_some() {
                println!("host_daemon_reachable\terror\t{e}");
            } else {
                println!("host_daemon_reachable\tnot_checked\taddress_unavailable");
            }
            println!("daemon\tstopped\t{e}");
        }
    }
    Ok(())
}

pub(crate) fn store_doctor(store: &str, keys: &KeyOpts) -> Result<(), String> {
    let paths = daemon::paths(store).map_err(|e| e.to_string())?;
    println!("store\t{}", paths.store);
    print_store_doctor_for_path(&paths.store, keys);
    Ok(())
}

fn print_store_doctor_for_path(store: &str, keys: &KeyOpts) {
    match FileStore::open_read(store) {
        Ok(fs) => {
            println!(
                "store_encrypted\t{}",
                if fs.is_encrypted() { "yes" } else { "no" }
            );
            println!("daemon_control_plane\tlock_fences,identity_acl,audit");
            match fs.store_maintenance_report(now_ms()) {
                Ok(report) => {
                    println!(
                        "store_maintenance\teligible={}\treason={}\tphysical_bytes={}\tmarked_live_bytes={}\tcandidate_reclaimable_bytes={}\treusable_free_bytes={}\ttail_free_pages={}\ttail_free_bytes={}\ttail_trim_eligible={}\ttail_blocked_by_live_objects={}\ttail_compaction_eligible={}\tfull_compaction_required_for_shrink={}\ttail_trim_attempted={}\ttail_trim_pages={}\ttail_trim_bytes={}\tretained_control_roots={}\tderived_payload_count={}\tmark_epoch={}\tmark_completed={}\tlast_validated_mark_epoch={}",
                        report.eligible,
                        report.reason,
                        report.status.physical_bytes,
                        report.marked_live_bytes,
                        report.candidate_reclaimable_bytes,
                        report.reusable_free_bytes,
                        report.tail_free_pages,
                        report.tail_free_bytes,
                        report.tail_trim_eligible,
                        report.tail_blocked_by_live_objects,
                        report.tail_compaction_eligible,
                        report.full_compaction_required_for_shrink,
                        report.tail_trim_attempted,
                        report.tail_trim_pages,
                        report.tail_trim_bytes,
                        report.retained_control_roots,
                        report.derived_payload_count,
                        report
                            .mark_epoch
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "none".to_string()),
                        report.mark_completed,
                        report.status.last_validated_mark_epoch
                    );
                    match open_read_loom_for_diagnostics(store, &fs, keys)
                        .and_then(|loom| maintenance_live_root_diagnostics(&loom))
                    {
                        Ok(diagnostics) => {
                            print_live_root_diagnostics("store_live_roots", &diagnostics)
                        }
                        Err(error) => println!("store_live_roots\terror\t{error}"),
                    }
                }
                Err(e) => println!("store_maintenance\terror\t{e}"),
            }
            match daemon_kv_loom(store) {
                Ok(_) => println!("daemon_runtime_data\tpure_ephemeral_kv=available"),
                Err(e) if e.code == loom_core::Code::E2eLocked => {
                    println!("daemon_runtime_data\tpure_ephemeral_kv=requires_unlock")
                }
                Err(e) => println!("daemon_runtime_data\tpure_ephemeral_kv=error\t{e}"),
            }
            match certificate_bundle_doctor_lines(&fs) {
                Ok(lines) => {
                    if lines.is_empty() {
                        println!("certificate_bundle_health\tok\tcount=0");
                    } else {
                        for line in lines {
                            println!("{line}");
                        }
                    }
                }
                Err(e) => println!("certificate_bundle_health\terror\t{e}"),
            }
            match network_access_policy_doctor_lines(&fs) {
                Ok(lines) => {
                    if lines.is_empty() {
                        println!("network_access_policy_health\tok\tcount=0");
                    } else {
                        for line in lines {
                            println!("{line}");
                        }
                    }
                }
                Err(e) => println!("network_access_policy_health\terror\t{e}"),
            }
        }
        Err(e) => println!("store_encrypted\terror\t{e}"),
    }
    print_reference_reconciliation_doctor(store, keys);
}

fn print_reference_reconciliation_doctor(store: &str, keys: &KeyOpts) {
    let loom = match cli_open_loom_read(store, keys) {
        Ok(loom) => loom,
        Err(error) => {
            println!("reference_reconciliation\tunavailable\t{error}");
            return;
        }
    };
    for workspace in loom.registry().list(None) {
        match loom_reference::status(&loom, workspace.id) {
            Ok(summary) => println!(
                "reference_reconciliation\tok\tworkspace={}\tid={}\tpending={}\tresolved={}\tfailed={}",
                workspace.name, workspace.id, summary.pending, summary.resolved, summary.failed
            ),
            Err(error) if error.code == Code::PermissionDenied => println!(
                "reference_reconciliation\tpermission_denied\tworkspace={}\tid={}",
                workspace.name, workspace.id
            ),
            Err(error) => println!(
                "reference_reconciliation\terror\tworkspace={}\tid={}\treason={}",
                workspace.name, workspace.id, error.message
            ),
        }
    }
}

fn daemon_transport_profile(transport: daemon::DaemonTransport) -> &'static str {
    daemon::transport_capabilities()
        .into_iter()
        .find(|capability| capability.transport == transport)
        .map(|capability| capability.status.wire_name())
        .unwrap_or("unknown")
}

fn print_daemon_transport_capabilities() {
    for capability in daemon::transport_capabilities() {
        println!("{}", daemon_transport_capability_line(&capability));
    }
}

fn daemon_transport_capability_line(capability: &daemon::DaemonTransportCapability) -> String {
    format!(
        "daemon_transport_capability\ttransport={}\tstatus={}\tsecurity={}\treason={}",
        capability.transport.wire_name(),
        capability.status.wire_name(),
        capability.security.wire_name(),
        capability.reason
    )
}

fn print_daemon_pin_status(status: &daemon::DaemonStatus) {
    println!(
        "daemon_pins\ttotal={}\tpermanent={}\tleased={}",
        status.pins, status.permanent_pins, status.leased_pins
    );
    for pin in &status.pin_details {
        match &pin.kind {
            daemon::DaemonPinKind::Permanent => {
                println!("daemon_pin\t{}\tpermanent", pin.id);
            }
            daemon::DaemonPinKind::Leased { deadline_ms } => {
                println!("daemon_pin\t{}\tleased\tdeadline_ms={deadline_ms}", pin.id);
            }
        }
    }
}

fn print_runtime_artifact(label: &str, path: &std::path::Path) {
    println!("{label}\t{}", runtime_artifact_state(path));
}

fn runtime_artifact_state(path: &std::path::Path) -> String {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => format!("present\tbytes={}", meta.len()),
        Ok(_) => "present\tnon-file".to_string(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "absent".to_string(),
        Err(e) => format!("error\t{e}"),
    }
}

fn append_daemon_audit(store: &str, action: &str, target: Option<&str>) -> Result<(), String> {
    append_daemon_audit_actor(store, None, action, target)
}

fn append_daemon_audit_actor(
    store: &str,
    actor: Option<WorkspaceId>,
    action: &str,
    target: Option<&str>,
) -> Result<(), String> {
    let fs = FileStore::open_daemon_authorized(store).map_err(|e| e.to_string())?;
    fs.audit_append(actor, action, target)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(any(feature = "serve", feature = "mcp"))]
fn network_access_denied_audit_sink(store: String) -> loom_hosted::HostedNetworkAccessAuditSink {
    std::sync::Arc::new(move |event| {
        let target = network_access_denied_audit_target(&event);
        let _ = append_daemon_audit(&store, "network_access.connection.deny", Some(&target));
    })
}

#[cfg(any(feature = "serve", feature = "mcp"))]
fn network_access_denied_audit_target(
    event: &loom_hosted::HostedNetworkAccessAuditEvent,
) -> String {
    let rule = event.rule_id.as_deref().unwrap_or("default");
    format!(
        "{};policy={};rule={rule};source_family={}",
        event.listener_id, event.policy_name, event.source_family
    )
}

#[cfg(feature = "serve")]
struct HostedListenerRuntime {
    configuration_fingerprint: String,
    tls_certificate_bundle_fingerprint: Option<String>,
    network_access_policy_fingerprint: Option<String>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    done: Option<std::sync::mpsc::Receiver<()>>,
    join: Option<std::thread::JoinHandle<()>>,
}

#[cfg(feature = "serve")]
struct DesiredHostedRuntime {
    id: String,
    records: Vec<ServedListenerRecord>,
    configuration_fingerprint: String,
    tls_certificate_bundle_fingerprint: Option<String>,
    network_access_policy_fingerprint: Option<String>,
}

#[cfg(feature = "serve")]
#[derive(Debug, Clone, PartialEq, Eq)]
struct HostedTlsBundleFingerprint {
    internal: String,
    audit_suffix: String,
}

#[cfg(feature = "serve")]
#[derive(Debug, Clone, PartialEq, Eq)]
struct HostedNetworkAccessPolicyFingerprint {
    internal: String,
    audit_suffix: String,
}

#[cfg(feature = "serve")]
#[derive(Debug, Clone, Copy)]
struct HostedStopPolicy {
    hard: bool,
    wait_ms: u64,
}

#[cfg(feature = "serve")]
const DEFAULT_DAEMON_STOP_WAIT_MS: u64 = 30_000;
#[cfg(feature = "serve")]
const DEFAULT_DRIVE_POLICY_RECONCILE_MS: u64 = 60_000;
#[cfg(feature = "serve")]
const REFERENCE_RECONCILE_IDLE_MS: u64 = 60_000;
#[cfg(feature = "serve")]
const DAEMON_SERVICE_PRINCIPAL_ID: WorkspaceId = WorkspaceId::from_bytes([
    0xda, 0xe0, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
]);
#[cfg(feature = "serve")]
const REFERENCE_RESOLVER_SERVICE_PRINCIPAL_ID: WorkspaceId = WorkspaceId::from_bytes([
    0xda, 0xe0, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02,
]);

#[cfg(feature = "serve")]
impl Drop for HostedListenerRuntime {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

#[cfg(feature = "serve")]
impl HostedListenerRuntime {
    fn stop(mut self, policy: HostedStopPolicy) -> bool {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if policy.hard {
            self.done.take();
            self.join.take();
            return false;
        }
        let drained = match self.done.as_ref() {
            Some(done) => done
                .recv_timeout(std::time::Duration::from_millis(policy.wait_ms))
                .is_ok(),
            None => true,
        };
        if drained {
            if let Some(join) = self.join.take() {
                let _ = join.join();
            }
            self.done.take();
            false
        } else {
            self.join.take();
            self.done.take();
            true
        }
    }
}

#[cfg(feature = "serve")]
fn spawn_hosted_listener(
    f: impl FnOnce() + Send + 'static,
) -> (std::thread::JoinHandle<()>, std::sync::mpsc::Receiver<()>) {
    let (done_tx, done_rx) = std::sync::mpsc::sync_channel::<()>(1);
    let join = std::thread::spawn(move || {
        f();
        let _ = done_tx.send(());
    });
    (join, done_rx)
}

#[cfg(feature = "serve")]
fn start_hosted_listeners(
    store: &str,
) -> Result<std::collections::BTreeMap<String, HostedListenerRuntime>, String> {
    let mut runtimes = std::collections::BTreeMap::new();
    for runtime in desired_hosted_listener_runtimes(store)? {
        let id = runtime.id.clone();
        match start_served_runtime(store, runtime) {
            Ok(runtime) => {
                runtimes.insert(id, runtime);
            }
            Err(err) => {
                let (record, reason) = *err;
                let target = format!("{};reason={reason}", served_listener_target(&record));
                append_daemon_audit(store, "serve.listener.reject", Some(&target))?;
                return Err(reason);
            }
        }
    }
    Ok(runtimes)
}

#[cfg(feature = "serve")]
fn desired_hosted_listener_runtimes(store: &str) -> Result<Vec<DesiredHostedRuntime>, String> {
    coalesce_dav_listener_records(store, desired_hosted_listener_records(store)?)
}

#[cfg(feature = "serve")]
fn desired_hosted_listener_records(store: &str) -> Result<Vec<ServedListenerRecord>, String> {
    let fs = FileStore::open_read(store).map_err(|e| e.to_string())?;
    let store_algo = fs.digest_algo();
    let fips_required = fs.store_policy().map_err(|e| e.to_string())?.fips_required;
    let records = fs.served_listeners().map_err(|e| e.to_string())?;
    let has_enabled = records.iter().any(|record| record.enabled);
    drop(fs);
    if has_enabled
        && let Err(err) = loom_hosted::validate_hosted_store_profile(store_algo, fips_required)
    {
        let target = format!("reason={}", err.message);
        append_daemon_audit(store, "serve.listener.reject", Some(&target))?;
        return Err(err.to_string());
    }
    let mut enabled = Vec::new();
    for record in records.into_iter().filter(|record| record.enabled) {
        match validate_hosted_listener_runtime(record) {
            Ok(record) => enabled.push(record),
            Err(err) => {
                let (record, reason) = *err;
                let target = format!("{};reason={reason}", served_listener_target(&record));
                append_daemon_audit(store, "serve.listener.reject", Some(&target))?;
                return Err(reason);
            }
        }
    }
    Ok(enabled
        .into_iter()
        .filter(supported_hosted_listener_runtime)
        .collect())
}

#[cfg(feature = "serve")]
fn coalesce_dav_listener_records(
    store: &str,
    records: Vec<ServedListenerRecord>,
) -> Result<Vec<DesiredHostedRuntime>, String> {
    let mut runtimes = Vec::new();
    let mut dav_groups = std::collections::BTreeMap::<String, Vec<ServedListenerRecord>>::new();
    let mut dav_bind_keys = std::collections::BTreeMap::<String, String>::new();
    for record in records {
        if is_dav_listener_record(&record) {
            let group_key = dav_listener_group_key(&record);
            match dav_bind_keys.get(&record.bind) {
                Some(existing) if existing != &group_key => {
                    return Err(format!(
                        "DAV listeners on bind {} must share TLS, auth, limits, audit, exposure, and network policy",
                        record.bind
                    ));
                }
                Some(_) => {}
                None => {
                    dav_bind_keys.insert(record.bind.clone(), group_key.clone());
                }
            }
            dav_groups.entry(group_key).or_default().push(record);
        } else {
            runtimes.push(desired_runtime_from_records(
                store,
                record.id.clone(),
                vec![record],
            )?);
        }
    }
    for (group_key, records) in dav_groups {
        let mut surfaces = std::collections::BTreeSet::new();
        for record in &records {
            if !surfaces.insert(record.surface.as_str()) {
                return Err(format!(
                    "DAV listener group {} cannot serve more than one {} record",
                    group_key, record.surface
                ));
            }
        }
        runtimes.push(desired_runtime_from_records(
            store,
            format!("dav:{group_key}"),
            records,
        )?);
    }
    Ok(runtimes)
}

#[cfg(feature = "serve")]
fn desired_runtime_from_records(
    store: &str,
    id: String,
    records: Vec<ServedListenerRecord>,
) -> Result<DesiredHostedRuntime, String> {
    let tls_certificate_bundle_fingerprint =
        shared_tls_certificate_bundle_fingerprint(store, &records)?;
    let network_access_policy_fingerprint =
        shared_network_access_policy_fingerprint(store, &records)?;
    let configuration_fingerprint = desired_runtime_configuration_fingerprint(
        &records,
        tls_certificate_bundle_fingerprint.as_deref(),
        network_access_policy_fingerprint.as_deref(),
    );
    Ok(DesiredHostedRuntime {
        id,
        records,
        configuration_fingerprint,
        tls_certificate_bundle_fingerprint,
        network_access_policy_fingerprint,
    })
}

#[cfg(feature = "serve")]
fn is_dav_listener_record(record: &ServedListenerRecord) -> bool {
    matches!(
        (record.surface.as_str(), record.transport.as_str()),
        ("calendar", "caldav") | ("contacts", "carddav")
    )
}

#[cfg(feature = "serve")]
fn dav_listener_group_key(record: &ServedListenerRecord) -> String {
    let tls_ref = record.tls.certificate_bundle_ref.as_deref().unwrap_or("");
    let network_ref = record.network_access_policy_ref.as_deref().unwrap_or("");
    format!(
        "bind={};tls_mode={};tls_ref={};auth={};request_limit={};idle={};session={};audit={};exposure={};network={}",
        record.bind,
        record.tls.mode,
        tls_ref,
        record.auth.mode,
        record.limits.request_size_limit,
        record.limits.idle_timeout_ms,
        record.limits.session_timeout_ms,
        record.audit.mode,
        record.exposure,
        network_ref
    )
}

#[cfg(feature = "serve")]
fn shared_tls_certificate_bundle_fingerprint(
    store: &str,
    records: &[ServedListenerRecord],
) -> Result<Option<String>, String> {
    let mut out = None;
    for record in records {
        let fingerprint = served_listener_tls_certificate_bundle_fingerprint(store, record)?
            .map(|fingerprint| fingerprint.internal);
        match (&out, fingerprint) {
            (None, value) => out = value,
            (Some(existing), Some(value)) if existing == &value => {}
            (Some(_), Some(_)) | (Some(_), None) => {
                return Err(format!(
                    "shared DAV listener {} has inconsistent TLS certificate bundles",
                    records[0].bind
                ));
            }
        }
    }
    Ok(out)
}

#[cfg(feature = "serve")]
fn shared_network_access_policy_fingerprint(
    store: &str,
    records: &[ServedListenerRecord],
) -> Result<Option<String>, String> {
    let mut out = None;
    for record in records {
        let fingerprint = served_listener_network_access_policy_fingerprint(store, record)?
            .map(|fingerprint| fingerprint.internal);
        match (&out, fingerprint) {
            (None, value) => out = value,
            (Some(existing), Some(value)) if existing == &value => {}
            (Some(_), Some(_)) | (Some(_), None) => {
                return Err(format!(
                    "shared DAV listener {} has inconsistent network access policies",
                    records[0].bind
                ));
            }
        }
    }
    Ok(out)
}

#[cfg(feature = "serve")]
fn desired_runtime_configuration_fingerprint(
    records: &[ServedListenerRecord],
    tls_certificate_bundle_fingerprint: Option<&str>,
    network_access_policy_fingerprint: Option<&str>,
) -> String {
    let mut out = String::new();
    for record in records {
        out.push_str(&served_listener_target(record));
        out.push_str(";seq=");
        match record.last_modified_audit_seq {
            Some(seq) => out.push_str(&seq.to_string()),
            None => out.push_str("none"),
        }
        out.push('\n');
    }
    out.push_str("tls=");
    out.push_str(tls_certificate_bundle_fingerprint.unwrap_or(""));
    out.push_str("\nnetwork=");
    out.push_str(network_access_policy_fingerprint.unwrap_or(""));
    out
}

#[cfg(feature = "serve")]
fn supported_hosted_listener_runtime(record: &ServedListenerRecord) -> bool {
    (record.surface == "cas" && matches!(record.transport.as_str(), "rest" | "json_rpc" | "grpc"))
        || (record.surface == "oci" && record.transport == "rest")
        || (record.surface == "s3" && record.transport == "rest")
        || (record.surface == "files"
            && matches!(record.transport.as_str(), "rest" | "json_rpc" | "grpc"))
        || (record.surface == "vcs"
            && matches!(record.transport.as_str(), "rest" | "json_rpc" | "grpc"))
        || (record.surface == "web" && record.transport == "rest")
        || (record.surface == "admin" && matches!(record.transport.as_str(), "rest" | "json_rpc"))
        || (record.surface == "exec"
            && matches!(record.transport.as_str(), "rest" | "json_rpc" | "grpc"))
        || (record.surface == "sql"
            && matches!(record.transport.as_str(), "rest" | "json_rpc" | "grpc"))
        || (matches!(record.surface.as_str(), "postgres" | "mysql") && record.transport == "tcp")
        || (record.surface == "mail"
            && matches!(record.transport.as_str(), "imap" | "jmap" | "smtp"))
        || (record.surface == "calendar" && record.transport == "caldav")
        || (record.surface == "contacts" && record.transport == "carddav")
        || (record.surface == "kv"
            && matches!(record.transport.as_str(), "rest" | "json_rpc" | "grpc"))
        || (record.surface == "queue"
            && matches!(record.transport.as_str(), "rest" | "json_rpc" | "grpc"))
        || (record.surface == "time-series"
            && matches!(record.transport.as_str(), "rest" | "json_rpc" | "grpc"))
        || (record.surface == "document"
            && matches!(record.transport.as_str(), "rest" | "json_rpc" | "grpc"))
        || (record.surface == "fts"
            && matches!(record.transport.as_str(), "rest" | "json_rpc" | "grpc"))
        || (record.surface == "columnar"
            && matches!(record.transport.as_str(), "rest" | "json_rpc" | "grpc")
            && record.profile.is_none())
        || (record.surface == "graph"
            && matches!(record.transport.as_str(), "rest" | "json_rpc" | "grpc")
            && record.profile.is_none())
        || (record.surface == "ledger"
            && matches!(record.transport.as_str(), "rest" | "json_rpc" | "grpc")
            && record.profile.is_none())
        || (matches!(record.surface.as_str(), "drive" | "chat" | "vector")
            && matches!(record.transport.as_str(), "rest" | "json_rpc"))
        || (matches!(record.surface.as_str(), "influx" | "prometheus" | "grafana")
            && record.transport == "http")
        || (record.surface == "otlp" && record.transport == "http")
        || (record.surface == "fts" && record.transport == "ndjson")
        || (record.surface == "redis" && record.transport == "resp")
        || (record.surface == "memcached" && record.transport == "text")
        || (record.surface == "etcd" && record.transport == "tcp")
        || (record.surface == "kafka" && record.transport == "tcp")
        || (record.surface == "neo4j" && record.transport == "tcp")
        || (record.surface == "dataframe" && record.transport == "rest")
        || (record.surface == "vector"
            && record.transport == "grpc"
            && record.profile.as_deref() == Some("qdrant"))
}

#[cfg(feature = "serve")]
fn validate_hosted_listener_runtime(
    record: ServedListenerRecord,
) -> Result<ServedListenerRecord, Box<(ServedListenerRecord, String)>> {
    if record.tls.mode == "direct" && record.transport == "grpc" {
        let reason = format!(
            "served listener {} uses unsupported direct TLS for grpc",
            record.id
        );
        return Err(Box::new((record, reason)));
    }
    if !matches!(record.tls.mode.as_str(), "off" | "direct" | "starttls") {
        let reason = format!(
            "served listener {} uses unsupported TLS mode {:?}",
            record.id, record.tls.mode
        );
        return Err(Box::new((record, reason)));
    }
    if record.tls.mode == "starttls" && !(record.surface == "mail" && record.transport == "smtp") {
        let reason = format!(
            "served listener {} uses STARTTLS for unsupported {}/{} transport",
            record.id, record.surface, record.transport
        );
        return Err(Box::new((record, reason)));
    }
    if !matches!(
        record.auth.mode.as_str(),
        "owner-or-passphrase" | "passphrase"
    ) {
        let reason = format!(
            "served listener {} uses unsupported auth mode {:?}",
            record.id, record.auth.mode
        );
        return Err(Box::new((record, reason)));
    }
    if record.exposure != "read-write" {
        let reason = format!(
            "served listener {} uses unsupported exposure {:?}",
            record.id, record.exposure
        );
        return Err(Box::new((record, reason)));
    }
    Ok(record)
}

#[cfg(feature = "serve")]
fn start_served_listener(
    store: &str,
    record: ServedListenerRecord,
) -> Result<HostedListenerRuntime, Box<(ServedListenerRecord, String)>> {
    let tls_certificate_bundle_fingerprint =
        match served_listener_tls_certificate_bundle_fingerprint(store, &record) {
            Ok(fingerprint) => fingerprint.map(|value| value.internal),
            Err(reason) => return Err(Box::new((record, reason))),
        };
    let network_access_policy_fingerprint =
        match served_listener_network_access_policy_fingerprint(store, &record) {
            Ok(fingerprint) => fingerprint.map(|value| value.internal),
            Err(reason) => return Err(Box::new((record, reason))),
        };
    let result = match record.surface.as_str() {
        "cas" => start_cas_listener(store, record.clone()),
        "oci" => start_oci_listener(store, record.clone()),
        "s3" => start_s3_listener(store, record.clone()),
        "files" => start_files_listener(store, record.clone()),
        "web" => start_web_listener(store, record.clone()),
        "vcs" => start_vcs_listener(store, record.clone()),
        "admin" => start_admin_listener(store, record.clone()),
        "exec" => start_exec_listener(store, record.clone()),
        "sql" | "postgres" | "mysql" => start_sql_listener(store, record.clone()),
        "mail" => start_mail_listener(store, record.clone()),
        "calendar" => start_pim_listener(store, record.clone()),
        "contacts" => start_pim_listener(store, record.clone()),
        "redis" => start_redis_listener(store, record.clone()),
        "memcached" => start_memcached_listener(store, record.clone()),
        "etcd" => start_etcd_listener(store, record.clone()),
        "kafka" => start_kafka_listener(store, record.clone()),
        "neo4j" => start_neo4j_listener(store, record.clone()),
        "influx" => start_influx_listener(store, record.clone()),
        "prometheus" => start_prometheus_listener(store, record.clone()),
        "grafana" => start_grafana_listener(store, record.clone()),
        "otlp" => start_otlp_listener(store, record.clone()),
        "kv" | "document" | "drive" | "chat" | "meetings" | "queue" | "time-series" | "graph"
        | "ledger" | "dataframe" | "vector" | "fts" | "columnar" => {
            start_data_listener(store, record.clone())
        }
        _ => Err(format!("unsupported served surface {:?}", record.surface)),
    };
    match result {
        Ok(mut runtime) => {
            runtime.tls_certificate_bundle_fingerprint = tls_certificate_bundle_fingerprint;
            runtime.network_access_policy_fingerprint = network_access_policy_fingerprint;
            Ok(runtime)
        }
        Err(reason) => Err(Box::new((record, reason))),
    }
}

#[cfg(feature = "serve")]
fn start_served_runtime(
    store: &str,
    desired: DesiredHostedRuntime,
) -> Result<HostedListenerRuntime, Box<(ServedListenerRecord, String)>> {
    if desired.records.len() == 1 {
        let record = desired.records[0].clone();
        let mut runtime = start_served_listener(store, record)?;
        runtime.configuration_fingerprint = desired.configuration_fingerprint;
        runtime.tls_certificate_bundle_fingerprint = desired.tls_certificate_bundle_fingerprint;
        runtime.network_access_policy_fingerprint = desired.network_access_policy_fingerprint;
        return Ok(runtime);
    }
    start_dav_listener_group(store, desired)
}

#[cfg(feature = "serve")]
fn start_cas_listener(
    store: &str,
    record: ServedListenerRecord,
) -> Result<HostedListenerRuntime, String> {
    let [selector] = record.selectors.as_slice() else {
        return Err(format!(
            "cas/{} listener {} expects one selector",
            record.transport, record.id
        ));
    };
    let workspace = resolve_served_workspace(store, selector)?;
    let target = served_listener_runtime_target(store, &record)?;
    let bind = record.bind.clone();
    let transport = record.transport.clone();
    let limits = served_listener_http_limits(&record)?;
    let tls = served_listener_tls_config(store, &record)?;
    let auth_policy = served_listener_auth_policy(&record)?;
    let network_access_policy = served_listener_network_access_policy(store, &record)?;
    let kernel = loom_hosted::HostedKernel::new(store)
        .with_write_guard(loom_hosted::HostedWriteGuard::DaemonAuthorized);
    let store = store.to_string();
    let store_for_thread = store.clone();
    let target_for_thread = target.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<String, String>>(1);
    let (join, done) = spawn_hosted_listener(move || {
        let result = serve_cas_thread(CasThreadRequest {
            listener_id: target_for_thread.clone(),
            store: store_for_thread.clone(),
            bind,
            transport,
            limits,
            tls,
            auth_policy,
            network_access_policy,
            kernel,
            workspace,
            shutdown_rx,
            ready_tx,
        });
        if result.is_ok() {
            let _ = append_daemon_audit(
                &store_for_thread,
                "serve.listener.close",
                Some(&target_for_thread),
            );
        }
    });
    match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(addr)) => {
            let open_target = format!("{target};addr={addr}");
            append_daemon_audit(&store, "serve.listener.open", Some(&open_target))?;
            Ok(HostedListenerRuntime {
                configuration_fingerprint: served_listener_target(&record),
                tls_certificate_bundle_fingerprint: None,
                network_access_policy_fingerprint: None,
                shutdown: Some(shutdown_tx),
                done: Some(done),
                join: Some(join),
            })
        }
        Ok(Err(e)) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(e)
        }
        Err(e) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(format!(
                "cas/{} listener {} did not report startup: {e}",
                record.transport, record.id
            ))
        }
    }
}

#[cfg(feature = "serve")]
fn start_oci_listener(
    store: &str,
    record: ServedListenerRecord,
) -> Result<HostedListenerRuntime, String> {
    let [selector] = record.selectors.as_slice() else {
        return Err(format!(
            "oci/{} listener {} expects one selector",
            record.transport, record.id
        ));
    };
    resolve_served_workspace(store, selector)?;
    let workspace = selector.clone();
    let target = served_listener_runtime_target(store, &record)?;
    let bind = record.bind.clone();
    let transport = record.transport.clone();
    let limits = served_listener_http_limits(&record)?;
    let tls = served_listener_tls_config(store, &record)?;
    let auth_policy = served_listener_auth_policy(&record)?;
    let network_access_policy = served_listener_network_access_policy(store, &record)?;
    let kernel = loom_hosted::HostedKernel::new(store)
        .with_write_guard(loom_hosted::HostedWriteGuard::DaemonAuthorized);
    let store = store.to_string();
    let store_for_thread = store.clone();
    let target_for_thread = target.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<String, String>>(1);
    let (join, done) = spawn_hosted_listener(move || {
        let result = serve_oci_thread(OciThreadRequest {
            listener_id: target_for_thread.clone(),
            store: store_for_thread.clone(),
            bind,
            transport,
            limits,
            tls,
            auth_policy,
            network_access_policy,
            kernel,
            workspace,
            shutdown_rx,
            ready_tx,
        });
        if result.is_ok() {
            let _ = append_daemon_audit(
                &store_for_thread,
                "serve.listener.close",
                Some(&target_for_thread),
            );
        }
    });
    match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(addr)) => {
            let open_target = format!("{target};addr={addr}");
            append_daemon_audit(&store, "serve.listener.open", Some(&open_target))?;
            Ok(HostedListenerRuntime {
                configuration_fingerprint: served_listener_target(&record),
                tls_certificate_bundle_fingerprint: None,
                network_access_policy_fingerprint: None,
                shutdown: Some(shutdown_tx),
                done: Some(done),
                join: Some(join),
            })
        }
        Ok(Err(e)) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(e)
        }
        Err(e) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(format!(
                "oci/{} listener {} did not report startup: {e}",
                record.transport, record.id
            ))
        }
    }
}

#[cfg(feature = "serve")]
fn start_s3_listener(
    store: &str,
    record: ServedListenerRecord,
) -> Result<HostedListenerRuntime, String> {
    let (workspace, bucket) = match record.selectors.as_slice() {
        [workspace] => (workspace.clone(), None),
        [workspace, bucket] => (workspace.clone(), Some(bucket.clone())),
        _ => {
            return Err(format!(
                "s3/{} listener {} expects one or two selectors",
                record.transport, record.id
            ));
        }
    };
    resolve_served_workspace(store, &workspace)?;
    let target = served_listener_runtime_target(store, &record)?;
    let bind = record.bind.clone();
    let transport = record.transport.clone();
    let limits = served_listener_http_limits(&record)?;
    let tls = served_listener_tls_config(store, &record)?;
    let auth_policy = served_listener_auth_policy(&record)?;
    let network_access_policy = served_listener_network_access_policy(store, &record)?;
    let kernel = loom_hosted::HostedKernel::new(store)
        .with_write_guard(loom_hosted::HostedWriteGuard::DaemonAuthorized);
    let store = store.to_string();
    let store_for_thread = store.clone();
    let target_for_thread = target.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<String, String>>(1);
    let (join, done) = spawn_hosted_listener(move || {
        let result = serve_s3_thread(S3ThreadRequest {
            listener_id: target_for_thread.clone(),
            store: store_for_thread.clone(),
            bind,
            transport,
            limits,
            tls,
            auth_policy,
            network_access_policy,
            kernel,
            workspace,
            bucket,
            shutdown_rx,
            ready_tx,
        });
        if result.is_ok() {
            let _ = append_daemon_audit(
                &store_for_thread,
                "serve.listener.close",
                Some(&target_for_thread),
            );
        }
    });
    match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(addr)) => {
            let open_target = format!("{target};addr={addr}");
            append_daemon_audit(&store, "serve.listener.open", Some(&open_target))?;
            Ok(HostedListenerRuntime {
                configuration_fingerprint: served_listener_target(&record),
                tls_certificate_bundle_fingerprint: None,
                network_access_policy_fingerprint: None,
                shutdown: Some(shutdown_tx),
                done: Some(done),
                join: Some(join),
            })
        }
        Ok(Err(e)) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(e)
        }
        Err(e) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(format!(
                "s3/{} listener {} did not report startup: {e}",
                record.transport, record.id
            ))
        }
    }
}

#[cfg(feature = "serve")]
fn start_files_listener(
    store: &str,
    record: ServedListenerRecord,
) -> Result<HostedListenerRuntime, String> {
    start_workspace_file_listener(store, record)
}

#[cfg(feature = "serve")]
fn start_web_listener(
    store: &str,
    record: ServedListenerRecord,
) -> Result<HostedListenerRuntime, String> {
    start_workspace_file_listener(store, record)
}

#[cfg(feature = "serve")]
fn start_workspace_file_listener(
    store: &str,
    record: ServedListenerRecord,
) -> Result<HostedListenerRuntime, String> {
    let [selector] = record.selectors.as_slice() else {
        return Err(format!(
            "{}/{} listener {} expects one selector",
            record.surface, record.transport, record.id
        ));
    };
    let workspace = resolve_served_workspace(store, selector)?;
    let web_listener = if record.surface == "web" {
        load_web_listener_config(store, &record, workspace)?
    } else {
        None
    };
    let target = served_listener_runtime_target(store, &record)?;
    let surface = record.surface.clone();
    let bind = record.bind.clone();
    let transport = record.transport.clone();
    let limits = served_listener_http_limits(&record)?;
    let tls = served_listener_tls_config(store, &record)?;
    let auth_policy = served_listener_auth_policy(&record)?;
    let network_access_policy = served_listener_network_access_policy(store, &record)?;
    let kernel = loom_hosted::HostedKernel::new(store)
        .with_write_guard(loom_hosted::HostedWriteGuard::DaemonAuthorized);
    let store = store.to_string();
    let store_for_thread = store.clone();
    let target_for_thread = target.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<String, String>>(1);
    let (join, done) = spawn_hosted_listener(move || {
        let result = serve_files_thread(FilesThreadRequest {
            listener_id: target_for_thread.clone(),
            surface,
            store: store_for_thread.clone(),
            bind,
            transport,
            limits,
            tls,
            auth_policy,
            network_access_policy,
            kernel,
            workspace,
            web_listener,
            shutdown_rx,
            ready_tx,
        });
        if result.is_ok() {
            let _ = append_daemon_audit(
                &store_for_thread,
                "serve.listener.close",
                Some(&target_for_thread),
            );
        }
    });
    match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(addr)) => {
            let open_target = format!("{target};addr={addr}");
            append_daemon_audit(&store, "serve.listener.open", Some(&open_target))?;
            Ok(HostedListenerRuntime {
                configuration_fingerprint: served_listener_target(&record),
                tls_certificate_bundle_fingerprint: None,
                network_access_policy_fingerprint: None,
                shutdown: Some(shutdown_tx),
                done: Some(done),
                join: Some(join),
            })
        }
        Ok(Err(e)) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(e)
        }
        Err(e) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(format!(
                "{}/{} listener {} did not report startup: {e}",
                record.surface, record.transport, record.id
            ))
        }
    }
}

#[cfg(feature = "serve")]
fn start_vcs_listener(
    store: &str,
    record: ServedListenerRecord,
) -> Result<HostedListenerRuntime, String> {
    let [selector] = record.selectors.as_slice() else {
        return Err(format!(
            "vcs/{} listener {} expects one selector",
            record.transport, record.id
        ));
    };
    let workspace = resolve_served_workspace(store, selector)?;
    let target = served_listener_runtime_target(store, &record)?;
    let bind = record.bind.clone();
    let transport = record.transport.clone();
    let limits = served_listener_http_limits(&record)?;
    let tls = served_listener_tls_config(store, &record)?;
    let auth_policy = served_listener_auth_policy(&record)?;
    let network_access_policy = served_listener_network_access_policy(store, &record)?;
    let kernel = loom_hosted::HostedKernel::new(store)
        .with_write_guard(loom_hosted::HostedWriteGuard::DaemonAuthorized);
    let store = store.to_string();
    let store_for_thread = store.clone();
    let target_for_thread = target.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<String, String>>(1);
    let (join, done) = spawn_hosted_listener(move || {
        let result = serve_vcs_thread(VcsThreadRequest {
            listener_id: target_for_thread.clone(),
            store: store_for_thread.clone(),
            bind,
            transport,
            limits,
            tls,
            auth_policy,
            network_access_policy,
            kernel,
            workspace,
            shutdown_rx,
            ready_tx,
        });
        if result.is_ok() {
            let _ = append_daemon_audit(
                &store_for_thread,
                "serve.listener.close",
                Some(&target_for_thread),
            );
        }
    });
    match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(addr)) => {
            let open_target = format!("{target};addr={addr}");
            append_daemon_audit(&store, "serve.listener.open", Some(&open_target))?;
            Ok(HostedListenerRuntime {
                configuration_fingerprint: served_listener_target(&record),
                tls_certificate_bundle_fingerprint: None,
                network_access_policy_fingerprint: None,
                shutdown: Some(shutdown_tx),
                done: Some(done),
                join: Some(join),
            })
        }
        Ok(Err(e)) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(e)
        }
        Err(e) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(format!(
                "vcs/{} listener {} did not report startup: {e}",
                record.transport, record.id
            ))
        }
    }
}

#[cfg(feature = "serve")]
fn start_data_listener(
    store: &str,
    record: ServedListenerRecord,
) -> Result<HostedListenerRuntime, String> {
    let (workspace, collection, selector_profile) =
        match (record.surface.as_str(), record.selectors.as_slice()) {
            ("drive", [workspace]) => {
                let workspace_id = resolve_served_workspace(store, workspace)?;
                (workspace.clone(), workspace_id.to_string(), None)
            }
            ("chat", [workspace, channel]) => {
                let workspace_id = resolve_served_workspace(store, workspace)?;
                (
                    workspace.clone(),
                    workspace_id.to_string(),
                    Some(channel.clone()),
                )
            }
            ("meetings", [workspace]) => {
                let workspace_id = resolve_served_workspace(store, workspace)?;
                (workspace.clone(), workspace_id.to_string(), None)
            }
            ("chat", _) => {
                return Err(format!(
                    "chat/{} listener {} expects workspace and channel selectors",
                    record.transport, record.id
                ));
            }
            ("drive", _) | ("meetings", _) => {
                return Err(format!(
                    "{}/{} listener {} expects one workspace selector",
                    record.surface, record.transport, record.id
                ));
            }
            (_, [workspace, collection]) => (workspace.clone(), collection.clone(), None),
            _ => {
                return Err(format!(
                    "{}/{} listener {} expects two selectors",
                    record.surface, record.transport, record.id
                ));
            }
        };
    let target = served_listener_runtime_target(store, &record)?;
    let bind = record.bind.clone();
    let surface = record.surface.clone();
    let transport = record.transport.clone();
    let profile = selector_profile.or_else(|| record.profile.clone());
    let limits = served_listener_http_limits(&record)?;
    let tls = served_listener_tls_config(store, &record)?;
    let auth_policy = served_listener_auth_policy(&record)?;
    let network_access_policy = served_listener_network_access_policy(store, &record)?;
    let mut kernel = loom_hosted::HostedKernel::new(store)
        .with_write_guard(loom_hosted::HostedWriteGuard::DaemonAuthorized);
    if surface == "meetings" {
        let inference_loom = loom_store::open_loom_daemon_authorized_unlocked(store, None)
            .map_err(|error| error.to_string())?;
        let inference_workspace = resolve_served_workspace(store, &workspace)?;
        if let Some(resolved) =
            resolve_optional_vector_binding(&inference_loom, inference_workspace, None)?
        {
            kernel = kernel.with_meetings_embedding_runtime(
                loom_hosted::meetings::HostedMeetingsEmbeddingRuntime::new(
                    resolved.instance,
                    resolved.handle,
                ),
            );
        }
    }
    let store = store.to_string();
    let store_for_thread = store.clone();
    let target_for_thread = target.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<String, String>>(1);
    let (join, done) = spawn_hosted_listener(move || {
        let result = serve_data_thread(DataThreadRequest {
            listener_id: target_for_thread.clone(),
            store: store_for_thread.clone(),
            bind,
            surface,
            transport,
            profile,
            limits,
            tls,
            auth_policy,
            network_access_policy,
            kernel,
            workspace,
            collection,
            shutdown_rx,
            ready_tx,
        });
        if result.is_ok() {
            let _ = append_daemon_audit(
                &store_for_thread,
                "serve.listener.close",
                Some(&target_for_thread),
            );
        }
    });
    match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(addr)) => {
            let open_target = format!("{target};addr={addr}");
            Ok(HostedListenerRuntime {
                configuration_fingerprint: open_target,
                tls_certificate_bundle_fingerprint: None,
                network_access_policy_fingerprint: None,
                shutdown: Some(shutdown_tx),
                done: Some(done),
                join: Some(join),
            })
        }
        Ok(Err(e)) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(e)
        }
        Err(e) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(format!(
                "{}/{} listener {} did not report startup: {e}",
                record.surface, record.transport, record.id
            ))
        }
    }
}

#[cfg(feature = "serve")]
fn start_influx_listener(
    store: &str,
    record: ServedListenerRecord,
) -> Result<HostedListenerRuntime, String> {
    let [workspace] = record.selectors.as_slice() else {
        return Err(format!(
            "{}/{} listener {} expects one workspace selector",
            record.surface, record.transport, record.id
        ));
    };
    if record.tls.mode != "off" {
        return Err(format!(
            "{}/{} listener {} does not support direct TLS or STARTTLS",
            record.surface, record.transport, record.id
        ));
    }
    let target = served_listener_runtime_target(store, &record)?;
    let bind = record.bind.clone();
    let surface = record.surface.clone();
    let transport = record.transport.clone();
    let limits = served_listener_http_limits(&record)?;
    let auth_policy = served_listener_auth_policy(&record)?;
    let network_access_policy = served_listener_network_access_policy(store, &record)?;
    let kernel = loom_hosted::HostedKernel::new(store)
        .with_write_guard(loom_hosted::HostedWriteGuard::DaemonAuthorized);
    let store = store.to_string();
    let workspace = workspace.clone();
    let store_for_thread = store.clone();
    let target_for_thread = target.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<String, String>>(1);
    let (join, done) = spawn_hosted_listener(move || {
        let result = serve_influx_thread(InfluxThreadRequest {
            listener_id: target_for_thread.clone(),
            store: store_for_thread.clone(),
            bind,
            surface,
            transport,
            limits,
            auth_policy,
            network_access_policy,
            kernel,
            workspace,
            shutdown_rx,
            ready_tx,
        });
        if result.is_ok() {
            let _ = append_daemon_audit(
                &store_for_thread,
                "serve.listener.close",
                Some(&target_for_thread),
            );
        }
    });
    match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(addr)) => {
            let open_target = format!("{target};addr={addr}");
            append_daemon_audit(&store, "serve.listener.open", Some(&open_target))?;
            Ok(HostedListenerRuntime {
                configuration_fingerprint: served_listener_target(&record),
                tls_certificate_bundle_fingerprint: None,
                network_access_policy_fingerprint: None,
                shutdown: Some(shutdown_tx),
                done: Some(done),
                join: Some(join),
            })
        }
        Ok(Err(e)) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(e)
        }
        Err(e) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(format!(
                "{}/{} listener {} did not report startup: {e}",
                record.surface, record.transport, record.id
            ))
        }
    }
}

#[cfg(feature = "serve")]
fn start_prometheus_listener(
    store: &str,
    record: ServedListenerRecord,
) -> Result<HostedListenerRuntime, String> {
    let [workspace] = record.selectors.as_slice() else {
        return Err(format!(
            "{}/{} listener {} expects one workspace selector",
            record.surface, record.transport, record.id
        ));
    };
    if record.tls.mode != "off" {
        return Err(format!(
            "{}/{} listener {} does not support direct TLS or STARTTLS",
            record.surface, record.transport, record.id
        ));
    }
    let target = served_listener_runtime_target(store, &record)?;
    let bind = record.bind.clone();
    let surface = record.surface.clone();
    let transport = record.transport.clone();
    let limits = served_listener_http_limits(&record)?;
    let auth_policy = served_listener_auth_policy(&record)?;
    let network_access_policy = served_listener_network_access_policy(store, &record)?;
    let kernel = loom_hosted::HostedKernel::new(store)
        .with_write_guard(loom_hosted::HostedWriteGuard::DaemonAuthorized);
    let store = store.to_string();
    let workspace = workspace.clone();
    let store_for_thread = store.clone();
    let target_for_thread = target.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<String, String>>(1);
    let (join, done) = spawn_hosted_listener(move || {
        let result = serve_prometheus_thread(PrometheusThreadRequest {
            listener_id: target_for_thread.clone(),
            store: store_for_thread.clone(),
            bind,
            surface,
            transport,
            limits,
            auth_policy,
            network_access_policy,
            kernel,
            workspace,
            shutdown_rx,
            ready_tx,
        });
        if result.is_ok() {
            let _ = append_daemon_audit(
                &store_for_thread,
                "serve.listener.close",
                Some(&target_for_thread),
            );
        }
    });
    match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(addr)) => {
            let open_target = format!("{target};addr={addr}");
            append_daemon_audit(&store, "serve.listener.open", Some(&open_target))?;
            Ok(HostedListenerRuntime {
                configuration_fingerprint: served_listener_target(&record),
                tls_certificate_bundle_fingerprint: None,
                network_access_policy_fingerprint: None,
                shutdown: Some(shutdown_tx),
                done: Some(done),
                join: Some(join),
            })
        }
        Ok(Err(e)) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(e)
        }
        Err(e) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(format!(
                "{}/{} listener {} did not report startup: {e}",
                record.surface, record.transport, record.id
            ))
        }
    }
}

#[cfg(feature = "serve")]
fn start_grafana_listener(
    store: &str,
    record: ServedListenerRecord,
) -> Result<HostedListenerRuntime, String> {
    let (workspace, collection) = match record.selectors.as_slice() {
        [workspace] => (workspace.clone(), "prometheus".to_string()),
        [workspace, collection] => (workspace.clone(), collection.clone()),
        _ => {
            return Err(format!(
                "{}/{} listener {} expects workspace and optional collection selectors",
                record.surface, record.transport, record.id
            ));
        }
    };
    if record.tls.mode != "off" {
        return Err(format!(
            "{}/{} listener {} does not support direct TLS or STARTTLS",
            record.surface, record.transport, record.id
        ));
    }
    let target = served_listener_runtime_target(store, &record)?;
    let bind = record.bind.clone();
    let surface = record.surface.clone();
    let transport = record.transport.clone();
    let limits = served_listener_http_limits(&record)?;
    let auth_policy = served_listener_auth_policy(&record)?;
    let network_access_policy = served_listener_network_access_policy(store, &record)?;
    let kernel = loom_hosted::HostedKernel::new(store)
        .with_write_guard(loom_hosted::HostedWriteGuard::DaemonAuthorized);
    let store = store.to_string();
    let store_for_thread = store.clone();
    let target_for_thread = target.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<String, String>>(1);
    let (join, done) = spawn_hosted_listener(move || {
        let result = serve_grafana_thread(GrafanaThreadRequest {
            listener_id: target_for_thread.clone(),
            store: store_for_thread.clone(),
            bind,
            surface,
            transport,
            limits,
            auth_policy,
            network_access_policy,
            kernel,
            workspace,
            collection,
            shutdown_rx,
            ready_tx,
        });
        if result.is_ok() {
            let _ = append_daemon_audit(
                &store_for_thread,
                "serve.listener.close",
                Some(&target_for_thread),
            );
        }
    });
    match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(addr)) => {
            let open_target = format!("{target};addr={addr}");
            append_daemon_audit(&store, "serve.listener.open", Some(&open_target))?;
            Ok(HostedListenerRuntime {
                configuration_fingerprint: served_listener_target(&record),
                tls_certificate_bundle_fingerprint: None,
                network_access_policy_fingerprint: None,
                shutdown: Some(shutdown_tx),
                done: Some(done),
                join: Some(join),
            })
        }
        Ok(Err(e)) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(e)
        }
        Err(e) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(format!(
                "{}/{} listener {} did not report startup: {e}",
                record.surface, record.transport, record.id
            ))
        }
    }
}

#[cfg(feature = "serve")]
fn start_otlp_listener(
    store: &str,
    record: ServedListenerRecord,
) -> Result<HostedListenerRuntime, String> {
    let [workspace] = record.selectors.as_slice() else {
        return Err(format!(
            "{}/{} listener {} expects one workspace selector",
            record.surface, record.transport, record.id
        ));
    };
    if record.tls.mode != "off" {
        return Err(format!(
            "{}/{} listener {} does not support direct TLS or STARTTLS",
            record.surface, record.transport, record.id
        ));
    }
    let target = served_listener_runtime_target(store, &record)?;
    let bind = record.bind.clone();
    let surface = record.surface.clone();
    let transport = record.transport.clone();
    let limits = served_listener_http_limits(&record)?;
    let auth_policy = served_listener_auth_policy(&record)?;
    let network_access_policy = served_listener_network_access_policy(store, &record)?;
    let kernel = loom_hosted::HostedKernel::new(store)
        .with_write_guard(loom_hosted::HostedWriteGuard::DaemonAuthorized);
    let store = store.to_string();
    let workspace = workspace.clone();
    let store_for_thread = store.clone();
    let target_for_thread = target.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<String, String>>(1);
    let (join, done) = spawn_hosted_listener(move || {
        let result = serve_otlp_thread(OtlpThreadRequest {
            listener_id: target_for_thread.clone(),
            store: store_for_thread.clone(),
            bind,
            surface,
            transport,
            limits,
            auth_policy,
            network_access_policy,
            kernel,
            workspace,
            shutdown_rx,
            ready_tx,
        });
        if result.is_ok() {
            let _ = append_daemon_audit(
                &store_for_thread,
                "serve.listener.close",
                Some(&target_for_thread),
            );
        }
    });
    match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(addr)) => {
            let open_target = format!("{target};addr={addr}");
            append_daemon_audit(&store, "serve.listener.open", Some(&open_target))?;
            Ok(HostedListenerRuntime {
                configuration_fingerprint: served_listener_target(&record),
                tls_certificate_bundle_fingerprint: None,
                network_access_policy_fingerprint: None,
                shutdown: Some(shutdown_tx),
                done: Some(done),
                join: Some(join),
            })
        }
        Ok(Err(e)) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(e)
        }
        Err(e) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(format!(
                "{}/{} listener {} did not report startup: {e}",
                record.surface, record.transport, record.id
            ))
        }
    }
}

#[cfg(feature = "serve")]
fn start_redis_listener(
    store: &str,
    record: ServedListenerRecord,
) -> Result<HostedListenerRuntime, String> {
    let [workspace, keyspace] = record.selectors.as_slice() else {
        return Err(format!(
            "{}/{} listener {} expects two selectors",
            record.surface, record.transport, record.id
        ));
    };
    if record.tls.mode != "off" {
        return Err(format!(
            "{}/{} listener {} does not support direct TLS or STARTTLS",
            record.surface, record.transport, record.id
        ));
    }
    let target = served_listener_runtime_target(store, &record)?;
    let bind = record.bind.clone();
    let surface = record.surface.clone();
    let transport = record.transport.clone();
    let network_access_policy = served_listener_network_access_policy(store, &record)?;
    let kernel = loom_hosted::HostedKernel::new(store)
        .with_write_guard(loom_hosted::HostedWriteGuard::DaemonAuthorized);
    let store = store.to_string();
    let workspace = workspace.clone();
    let keyspace = keyspace.clone();
    let store_for_thread = store.clone();
    let target_for_thread = target.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<String, String>>(1);
    let (join, done) = spawn_hosted_listener(move || {
        let result = serve_redis_thread(RedisThreadRequest {
            listener_id: target_for_thread.clone(),
            store: store_for_thread.clone(),
            bind,
            surface,
            transport,
            network_access_policy,
            kernel,
            workspace,
            keyspace,
            shutdown_rx,
            ready_tx,
        });
        if result.is_ok() {
            let _ = append_daemon_audit(
                &store_for_thread,
                "serve.listener.close",
                Some(&target_for_thread),
            );
        }
    });
    match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(addr)) => {
            let open_target = format!("{target};addr={addr}");
            append_daemon_audit(&store, "serve.listener.open", Some(&open_target))?;
            Ok(HostedListenerRuntime {
                configuration_fingerprint: served_listener_target(&record),
                tls_certificate_bundle_fingerprint: None,
                network_access_policy_fingerprint: None,
                shutdown: Some(shutdown_tx),
                done: Some(done),
                join: Some(join),
            })
        }
        Ok(Err(e)) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(e)
        }
        Err(e) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(format!(
                "{}/{} listener {} did not report startup: {e}",
                record.surface, record.transport, record.id
            ))
        }
    }
}

#[cfg(feature = "serve")]
fn start_memcached_listener(
    store: &str,
    record: ServedListenerRecord,
) -> Result<HostedListenerRuntime, String> {
    let [workspace, cache] = record.selectors.as_slice() else {
        return Err(format!(
            "{}/{} listener {} expects two selectors",
            record.surface, record.transport, record.id
        ));
    };
    if record.tls.mode != "off" {
        return Err(format!(
            "{}/{} listener {} does not support direct TLS or STARTTLS",
            record.surface, record.transport, record.id
        ));
    }
    let target = served_listener_runtime_target(store, &record)?;
    let bind = record.bind.clone();
    let surface = record.surface.clone();
    let transport = record.transport.clone();
    let mode = loom_hosted::MemcachedCacheMode::from_profile(record.profile.as_deref());
    let network_access_policy = served_listener_network_access_policy(store, &record)?;
    let kernel = loom_hosted::HostedKernel::new(store)
        .with_write_guard(loom_hosted::HostedWriteGuard::DaemonAuthorized);
    let store = store.to_string();
    let workspace = workspace.clone();
    let display_cache_name = format!("{workspace}:{cache}");
    let collection = cache.clone();
    let store_for_thread = store.clone();
    let target_for_thread = target.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<String, String>>(1);
    let (join, done) = spawn_hosted_listener(move || {
        let result = serve_memcached_thread(MemcachedThreadRequest {
            listener_id: target_for_thread.clone(),
            store: store_for_thread.clone(),
            bind,
            surface,
            transport,
            network_access_policy,
            kernel,
            workspace,
            display_cache_name,
            collection,
            mode,
            shutdown_rx,
            ready_tx,
        });
        if result.is_ok() {
            let _ = append_daemon_audit(
                &store_for_thread,
                "serve.listener.close",
                Some(&target_for_thread),
            );
        }
    });
    match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(addr)) => {
            let open_target = format!("{target};addr={addr}");
            append_daemon_audit(&store, "serve.listener.open", Some(&open_target))?;
            Ok(HostedListenerRuntime {
                configuration_fingerprint: served_listener_target(&record),
                tls_certificate_bundle_fingerprint: None,
                network_access_policy_fingerprint: None,
                shutdown: Some(shutdown_tx),
                done: Some(done),
                join: Some(join),
            })
        }
        Ok(Err(e)) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(e)
        }
        Err(e) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(format!(
                "{}/{} listener {} did not report startup: {e}",
                record.surface, record.transport, record.id
            ))
        }
    }
}

#[cfg(feature = "serve")]
fn start_etcd_listener(
    store: &str,
    record: ServedListenerRecord,
) -> Result<HostedListenerRuntime, String> {
    let [workspace, collection] = record.selectors.as_slice() else {
        return Err(format!(
            "{}/{} listener {} expects two selectors",
            record.surface, record.transport, record.id
        ));
    };
    if record.tls.mode != "off" {
        return Err(format!(
            "{}/{} listener {} does not support direct TLS or STARTTLS",
            record.surface, record.transport, record.id
        ));
    }
    let target = served_listener_runtime_target(store, &record)?;
    let bind = record.bind.clone();
    let surface = record.surface.clone();
    let transport = record.transport.clone();
    let network_access_policy = served_listener_network_access_policy(store, &record)?;
    let kernel = loom_hosted::HostedKernel::new(store)
        .with_write_guard(loom_hosted::HostedWriteGuard::DaemonAuthorized);
    let store = store.to_string();
    let workspace = workspace.clone();
    let collection = collection.clone();
    let store_for_thread = store.clone();
    let target_for_thread = target.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<String, String>>(1);
    let (join, done) = spawn_hosted_listener(move || {
        let result = serve_etcd_thread(EtcdThreadRequest {
            listener_id: target_for_thread.clone(),
            store: store_for_thread.clone(),
            bind,
            surface,
            transport,
            network_access_policy,
            kernel,
            workspace,
            collection,
            shutdown_rx,
            ready_tx,
        });
        if result.is_ok() {
            let _ = append_daemon_audit(
                &store_for_thread,
                "serve.listener.close",
                Some(&target_for_thread),
            );
        }
    });
    match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(addr)) => {
            let open_target = format!("{target};addr={addr}");
            append_daemon_audit(&store, "serve.listener.open", Some(&open_target))?;
            Ok(HostedListenerRuntime {
                configuration_fingerprint: served_listener_target(&record),
                tls_certificate_bundle_fingerprint: None,
                network_access_policy_fingerprint: None,
                shutdown: Some(shutdown_tx),
                done: Some(done),
                join: Some(join),
            })
        }
        Ok(Err(e)) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(e)
        }
        Err(e) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(format!(
                "{}/{} listener {} did not report startup: {e}",
                record.surface, record.transport, record.id
            ))
        }
    }
}

#[cfg(feature = "serve")]
fn start_kafka_listener(
    store: &str,
    record: ServedListenerRecord,
) -> Result<HostedListenerRuntime, String> {
    let [workspace] = record.selectors.as_slice() else {
        return Err(format!(
            "{}/{} listener {} expects one selector",
            record.surface, record.transport, record.id
        ));
    };
    if record.tls.mode != "off" {
        return Err(format!(
            "{}/{} listener {} does not support direct TLS or STARTTLS",
            record.surface, record.transport, record.id
        ));
    }
    let target = served_listener_runtime_target(store, &record)?;
    let bind = record.bind.clone();
    let surface = record.surface.clone();
    let transport = record.transport.clone();
    let network_access_policy = served_listener_network_access_policy(store, &record)?;
    let kernel = loom_hosted::HostedKernel::new(store)
        .with_write_guard(loom_hosted::HostedWriteGuard::DaemonAuthorized);
    let store = store.to_string();
    let workspace = workspace.clone();
    let store_for_thread = store.clone();
    let target_for_thread = target.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<String, String>>(1);
    let (join, done) = spawn_hosted_listener(move || {
        let result = serve_kafka_thread(KafkaThreadRequest {
            listener_id: target_for_thread.clone(),
            store: store_for_thread.clone(),
            bind,
            surface,
            transport,
            network_access_policy,
            kernel,
            workspace,
            shutdown_rx,
            ready_tx,
        });
        if result.is_ok() {
            let _ = append_daemon_audit(
                &store_for_thread,
                "serve.listener.close",
                Some(&target_for_thread),
            );
        }
    });
    match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(addr)) => {
            let open_target = format!("{target};addr={addr}");
            append_daemon_audit(&store, "serve.listener.open", Some(&open_target))?;
            Ok(HostedListenerRuntime {
                configuration_fingerprint: served_listener_target(&record),
                tls_certificate_bundle_fingerprint: None,
                network_access_policy_fingerprint: None,
                shutdown: Some(shutdown_tx),
                done: Some(done),
                join: Some(join),
            })
        }
        Ok(Err(e)) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(e)
        }
        Err(e) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(format!(
                "{}/{} listener {} did not report startup: {e}",
                record.surface, record.transport, record.id
            ))
        }
    }
}

#[cfg(feature = "serve")]
fn start_neo4j_listener(
    store: &str,
    record: ServedListenerRecord,
) -> Result<HostedListenerRuntime, String> {
    let [workspace, graph] = record.selectors.as_slice() else {
        return Err(format!(
            "{}/{} listener {} expects two selectors",
            record.surface, record.transport, record.id
        ));
    };
    if record.tls.mode != "off" {
        return Err(format!(
            "{}/{} listener {} does not support direct TLS or STARTTLS",
            record.surface, record.transport, record.id
        ));
    }
    let target = served_listener_runtime_target(store, &record)?;
    let bind = record.bind.clone();
    let surface = record.surface.clone();
    let transport = record.transport.clone();
    let network_access_policy = served_listener_network_access_policy(store, &record)?;
    let kernel = loom_hosted::HostedKernel::new(store)
        .with_write_guard(loom_hosted::HostedWriteGuard::DaemonAuthorized);
    let store = store.to_string();
    let workspace = workspace.clone();
    let graph = graph.clone();
    let store_for_thread = store.clone();
    let target_for_thread = target.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<String, String>>(1);
    let (join, done) = spawn_hosted_listener(move || {
        let result = serve_neo4j_thread(Neo4jThreadRequest {
            listener_id: target_for_thread.clone(),
            store: store_for_thread.clone(),
            bind,
            surface,
            transport,
            network_access_policy,
            kernel,
            workspace,
            graph,
            shutdown_rx,
            ready_tx,
        });
        if result.is_ok() {
            let _ = append_daemon_audit(
                &store_for_thread,
                "serve.listener.close",
                Some(&target_for_thread),
            );
        }
    });
    match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(addr)) => {
            let open_target = format!("{target};addr={addr}");
            append_daemon_audit(&store, "serve.listener.open", Some(&open_target))?;
            Ok(HostedListenerRuntime {
                configuration_fingerprint: served_listener_target(&record),
                tls_certificate_bundle_fingerprint: None,
                network_access_policy_fingerprint: None,
                shutdown: Some(shutdown_tx),
                done: Some(done),
                join: Some(join),
            })
        }
        Ok(Err(e)) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(e)
        }
        Err(e) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(format!(
                "{}/{} listener {} did not report startup: {e}",
                record.surface, record.transport, record.id
            ))
        }
    }
}

#[cfg(feature = "serve")]
fn start_admin_listener(
    store: &str,
    record: ServedListenerRecord,
) -> Result<HostedListenerRuntime, String> {
    if !record.selectors.is_empty() {
        return Err(format!(
            "admin/{} listener {} expects no selectors",
            record.transport, record.id
        ));
    }
    let target = served_listener_runtime_target(store, &record)?;
    let bind = record.bind.clone();
    let transport = record.transport.clone();
    let limits = served_listener_http_limits(&record)?;
    let tls = served_listener_tls_config(store, &record)?;
    let auth_policy = served_listener_auth_policy(&record)?;
    let network_access_policy = served_listener_network_access_policy(store, &record)?;
    let kernel = loom_hosted::HostedKernel::new(store)
        .with_write_guard(loom_hosted::HostedWriteGuard::DaemonAuthorized);
    let store = store.to_string();
    let store_for_thread = store.clone();
    let target_for_thread = target.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<String, String>>(1);
    let (join, done) = spawn_hosted_listener(move || {
        let result = serve_admin_thread(ServeAdminThread {
            listener_id: target_for_thread.clone(),
            store: store_for_thread.clone(),
            bind,
            transport,
            limits,
            tls,
            auth_policy,
            network_access_policy,
            kernel,
            shutdown_rx,
            ready_tx,
        });
        if result.is_ok() {
            let _ = append_daemon_audit(
                &store_for_thread,
                "serve.listener.close",
                Some(&target_for_thread),
            );
        }
    });
    match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(addr)) => {
            let open_target = format!("{target};addr={addr}");
            append_daemon_audit(&store, "serve.listener.open", Some(&open_target))?;
            Ok(HostedListenerRuntime {
                configuration_fingerprint: served_listener_target(&record),
                tls_certificate_bundle_fingerprint: None,
                network_access_policy_fingerprint: None,
                shutdown: Some(shutdown_tx),
                done: Some(done),
                join: Some(join),
            })
        }
        Ok(Err(e)) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(e)
        }
        Err(e) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(format!(
                "admin/{} listener {} did not report startup: {e}",
                record.transport, record.id
            ))
        }
    }
}

#[cfg(feature = "serve")]
fn start_exec_listener(
    store: &str,
    record: ServedListenerRecord,
) -> Result<HostedListenerRuntime, String> {
    if !record.selectors.is_empty() {
        return Err(format!(
            "exec/{} listener {} expects no selectors",
            record.transport, record.id
        ));
    }
    let target = served_listener_runtime_target(store, &record)?;
    let bind = record.bind.clone();
    let transport = record.transport.clone();
    let limits = served_listener_http_limits(&record)?;
    let tls = served_listener_tls_config(store, &record)?;
    let auth_policy = served_listener_auth_policy(&record)?;
    let network_access_policy = served_listener_network_access_policy(store, &record)?;
    let kernel = loom_hosted::HostedKernel::new(store)
        .with_write_guard(loom_hosted::HostedWriteGuard::DaemonAuthorized);
    let store = store.to_string();
    let store_for_thread = store.clone();
    let target_for_thread = target.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<String, String>>(1);
    let (join, done) = spawn_hosted_listener(move || {
        let result = serve_exec_thread(ExecThreadRequest {
            listener_id: target_for_thread.clone(),
            store: store_for_thread.clone(),
            bind,
            transport,
            limits,
            tls,
            auth_policy,
            network_access_policy,
            kernel,
            shutdown_rx,
            ready_tx,
        });
        if result.is_ok() {
            let _ = append_daemon_audit(
                &store_for_thread,
                "serve.listener.close",
                Some(&target_for_thread),
            );
        }
    });
    match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(addr)) => {
            let open_target = format!("{target};addr={addr}");
            append_daemon_audit(&store, "serve.listener.open", Some(&open_target))?;
            Ok(HostedListenerRuntime {
                configuration_fingerprint: served_listener_target(&record),
                tls_certificate_bundle_fingerprint: None,
                network_access_policy_fingerprint: None,
                shutdown: Some(shutdown_tx),
                done: Some(done),
                join: Some(join),
            })
        }
        Ok(Err(e)) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(e)
        }
        Err(e) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(format!(
                "exec/{} listener {} did not report startup: {e}",
                record.transport, record.id
            ))
        }
    }
}

#[cfg(feature = "serve")]
fn start_sql_listener(
    store: &str,
    record: ServedListenerRecord,
) -> Result<HostedListenerRuntime, String> {
    let [workspace, database] = record.selectors.as_slice() else {
        return Err(format!(
            "sql/{} listener {} expects two selectors",
            record.transport, record.id
        ));
    };
    let target = served_listener_runtime_target(store, &record)?;
    let bind = record.bind.clone();
    let surface = record.surface.clone();
    let transport = match surface.as_str() {
        "postgres" => "pg_wire".to_string(),
        "mysql" => "mysql_wire".to_string(),
        "sql" => record.transport.clone(),
        _ => return Err(format!("unsupported SQL surface {surface:?}")),
    };
    let limits = served_listener_http_limits(&record)?;
    let tls = served_listener_tls_config(store, &record)?;
    let auth_policy = served_listener_auth_policy(&record)?;
    let network_access_policy = served_listener_network_access_policy(store, &record)?;
    let kernel = loom_hosted::HostedKernel::new(store)
        .with_write_guard(loom_hosted::HostedWriteGuard::DaemonAuthorized);
    let store = store.to_string();
    let workspace = workspace.clone();
    let database = database.clone();
    let store_for_thread = store.clone();
    let target_for_thread = target.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<String, String>>(1);
    let (join, done) = spawn_hosted_listener(move || {
        let result = serve_sql_thread(SqlThreadRequest {
            listener_id: target_for_thread.clone(),
            store: store_for_thread.clone(),
            bind,
            surface,
            transport,
            limits,
            tls,
            auth_policy,
            network_access_policy,
            kernel,
            workspace,
            database,
            shutdown_rx,
            ready_tx,
        });
        if result.is_ok() {
            let _ = append_daemon_audit(
                &store_for_thread,
                "serve.listener.close",
                Some(&target_for_thread),
            );
        }
    });
    match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(addr)) => {
            let open_target = format!("{target};addr={addr}");
            append_daemon_audit(&store, "serve.listener.open", Some(&open_target))?;
            Ok(HostedListenerRuntime {
                configuration_fingerprint: served_listener_target(&record),
                tls_certificate_bundle_fingerprint: None,
                network_access_policy_fingerprint: None,
                shutdown: Some(shutdown_tx),
                done: Some(done),
                join: Some(join),
            })
        }
        Ok(Err(e)) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(e)
        }
        Err(e) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(format!(
                "sql/{} listener {} did not report startup: {e}",
                record.transport, record.id
            ))
        }
    }
}

#[cfg(feature = "serve")]
fn start_mail_listener(
    store: &str,
    record: ServedListenerRecord,
) -> Result<HostedListenerRuntime, String> {
    let [workspace] = record.selectors.as_slice() else {
        return Err(format!(
            "mail/{} listener {} expects one selector",
            record.transport, record.id
        ));
    };
    let target = served_listener_runtime_target(store, &record)?;
    let bind = record.bind.clone();
    let transport = record.transport.clone();
    let tls = served_listener_tls_config(store, &record)?;
    let tls_mode = record.tls.mode.clone();
    let limits = served_listener_http_limits(&record)?;
    let auth_policy = served_listener_auth_policy(&record)?;
    let network_access_policy = served_listener_network_access_policy(store, &record)?;
    let kernel = loom_hosted::HostedKernel::new(store)
        .with_write_guard(loom_hosted::HostedWriteGuard::DaemonAuthorized);
    let store = store.to_string();
    let workspace = workspace.clone();
    let store_for_thread = store.clone();
    let target_for_thread = target.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<String, String>>(1);
    let (join, done) = spawn_hosted_listener(move || {
        let result = serve_mail_thread(MailThreadRequest {
            listener_id: target_for_thread.clone(),
            store: store_for_thread.clone(),
            bind,
            transport,
            tls_mode,
            tls,
            limits,
            auth_policy,
            network_access_policy,
            kernel,
            workspace,
            shutdown_rx,
            ready_tx,
        });
        if result.is_ok() {
            let _ = append_daemon_audit(
                &store_for_thread,
                "serve.listener.close",
                Some(&target_for_thread),
            );
        }
    });
    match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(addr)) => {
            let open_target = format!("{target};addr={addr}");
            append_daemon_audit(&store, "serve.listener.open", Some(&open_target))?;
            Ok(HostedListenerRuntime {
                configuration_fingerprint: served_listener_target(&record),
                tls_certificate_bundle_fingerprint: None,
                network_access_policy_fingerprint: None,
                shutdown: Some(shutdown_tx),
                done: Some(done),
                join: Some(join),
            })
        }
        Ok(Err(e)) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(e)
        }
        Err(e) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(format!(
                "mail/{} listener {} did not report startup: {e}",
                record.transport, record.id
            ))
        }
    }
}

#[cfg(feature = "serve")]
fn start_pim_listener(
    store: &str,
    record: ServedListenerRecord,
) -> Result<HostedListenerRuntime, String> {
    let [workspace] = record.selectors.as_slice() else {
        return Err(format!(
            "{}/{} listener {} expects one selector",
            record.surface, record.transport, record.id
        ));
    };
    let target = served_listener_runtime_target(store, &record)?;
    let bind = record.bind.clone();
    let surface = record.surface.clone();
    let transport = record.transport.clone();
    let limits = served_listener_http_limits(&record)?;
    let tls = served_listener_tls_config(store, &record)?;
    let auth_policy = served_listener_auth_policy(&record)?;
    let network_access_policy = served_listener_network_access_policy(store, &record)?;
    let kernel = loom_hosted::HostedKernel::new(store)
        .with_write_guard(loom_hosted::HostedWriteGuard::DaemonAuthorized);
    let store = store.to_string();
    let workspace = workspace.clone();
    let store_for_thread = store.clone();
    let target_for_thread = target.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<String, String>>(1);
    let (join, done) = spawn_hosted_listener(move || {
        let result = serve_pim_thread(PimThreadRequest {
            listener_id: target_for_thread.clone(),
            store: store_for_thread.clone(),
            bind,
            surface,
            transport,
            limits,
            tls,
            auth_policy,
            network_access_policy,
            kernel,
            workspace,
            shutdown_rx,
            ready_tx,
        });
        if result.is_ok() {
            let _ = append_daemon_audit(
                &store_for_thread,
                "serve.listener.close",
                Some(&target_for_thread),
            );
        }
    });
    match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(addr)) => {
            let open_target = format!("{target};addr={addr}");
            append_daemon_audit(&store, "serve.listener.open", Some(&open_target))?;
            Ok(HostedListenerRuntime {
                configuration_fingerprint: served_listener_target(&record),
                tls_certificate_bundle_fingerprint: None,
                network_access_policy_fingerprint: None,
                shutdown: Some(shutdown_tx),
                done: Some(done),
                join: Some(join),
            })
        }
        Ok(Err(e)) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(e)
        }
        Err(e) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(format!(
                "{}/{} listener {} did not report startup: {e}",
                record.surface, record.transport, record.id
            ))
        }
    }
}

#[cfg(feature = "serve")]
fn start_dav_listener_group(
    store: &str,
    desired: DesiredHostedRuntime,
) -> Result<HostedListenerRuntime, Box<(ServedListenerRecord, String)>> {
    let first = desired.records[0].clone();
    let target = match dav_listener_runtime_target(store, &desired.records) {
        Ok(target) => target,
        Err(reason) => return Err(Box::new((first, reason))),
    };
    let bind = first.bind.clone();
    let limits = match served_listener_http_limits(&first) {
        Ok(limits) => limits,
        Err(reason) => return Err(Box::new((first, reason))),
    };
    let tls = match served_listener_tls_config(store, &first) {
        Ok(tls) => tls,
        Err(reason) => return Err(Box::new((first, reason))),
    };
    let auth_policy = match served_listener_auth_policy(&first) {
        Ok(auth_policy) => auth_policy,
        Err(reason) => return Err(Box::new((first, reason))),
    };
    let network_access_policy = match served_listener_network_access_policy(store, &first) {
        Ok(policy) => policy,
        Err(reason) => return Err(Box::new((first, reason))),
    };
    let mut caldav_workspace = None;
    let mut carddav_workspace = None;
    for record in &desired.records {
        let [workspace] = record.selectors.as_slice() else {
            let reason = format!(
                "{}/{} listener {} expects one selector",
                record.surface, record.transport, record.id
            );
            return Err(Box::new((record.clone(), reason)));
        };
        match (record.surface.as_str(), record.transport.as_str()) {
            ("calendar", "caldav") => caldav_workspace = Some(workspace.clone()),
            ("contacts", "carddav") => carddav_workspace = Some(workspace.clone()),
            _ => {
                let reason = format!(
                    "shared DAV listener does not support {}/{}",
                    record.surface, record.transport
                );
                return Err(Box::new((record.clone(), reason)));
            }
        }
    }
    let kernel = loom_hosted::HostedKernel::new(store)
        .with_write_guard(loom_hosted::HostedWriteGuard::DaemonAuthorized);
    let store = store.to_string();
    let store_for_thread = store.clone();
    let target_for_thread = target.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<String, String>>(1);
    let (join, done) = spawn_hosted_listener(move || {
        let result = serve_dav_thread(DavThreadRequest {
            listener_id: target_for_thread.clone(),
            store: store_for_thread.clone(),
            bind,
            limits,
            tls,
            auth_policy,
            network_access_policy,
            kernel,
            caldav_workspace,
            carddav_workspace,
            shutdown_rx,
            ready_tx,
        });
        if result.is_ok() {
            let _ = append_daemon_audit(
                &store_for_thread,
                "serve.listener.close",
                Some(&target_for_thread),
            );
        }
    });
    match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(addr)) => {
            let open_target = format!("{target};addr={addr}");
            append_daemon_audit(&store, "serve.listener.open", Some(&open_target))
                .map_err(|reason| Box::new((first.clone(), reason)))?;
            Ok(HostedListenerRuntime {
                configuration_fingerprint: desired.configuration_fingerprint,
                tls_certificate_bundle_fingerprint: desired.tls_certificate_bundle_fingerprint,
                network_access_policy_fingerprint: desired.network_access_policy_fingerprint,
                shutdown: Some(shutdown_tx),
                done: Some(done),
                join: Some(join),
            })
        }
        Ok(Err(e)) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(Box::new((first, e)))
        }
        Err(e) => {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            Err(Box::new((
                first,
                format!("shared DAV listener did not report startup: {e}"),
            )))
        }
    }
}

#[cfg(feature = "serve")]
struct MailThreadRequest {
    listener_id: String,
    store: String,
    bind: String,
    transport: String,
    tls_mode: String,
    tls: Option<loom_hosted::HostedTlsConfig>,
    limits: loom_hosted::HostedHttpLimits,
    auth_policy: loom_hosted::HostedAuthPolicy,
    network_access_policy: Option<loom_store::NetworkAccessPolicyRecord>,
    kernel: loom_hosted::HostedKernel,
    workspace: String,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ready_tx: std::sync::mpsc::SyncSender<Result<String, String>>,
}

#[cfg(feature = "serve")]
fn serve_mail_thread(request: MailThreadRequest) -> Result<(), String> {
    let MailThreadRequest {
        listener_id,
        store,
        bind,
        transport,
        tls_mode,
        tls,
        limits,
        auth_policy,
        network_access_policy,
        kernel,
        workspace,
        shutdown_rx,
        ready_tx,
    } = request;
    let denied_audit = Some(network_access_denied_audit_sink(store));
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            let err = format!("build mail/{transport} runtime: {e}");
            let _ = ready_tx.send(Err(err.clone()));
            return Err(err);
        }
    };
    runtime.block_on(
        loom_hosted::with_hosted_network_access_policy_for_listener_and_audit(
            listener_id,
            network_access_policy,
            denied_audit,
            async move {
                let listener = match tokio::net::TcpListener::bind(&bind).await {
                    Ok(listener) => listener,
                    Err(e) => {
                        let err = format!("bind mail/{transport} listener {bind}: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let local_addr = match listener.local_addr() {
                    Ok(addr) => addr,
                    Err(e) => {
                        let err = format!("read mail/{transport} listener address: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let _ = ready_tx.send(Ok(local_addr.to_string()));
                match transport.as_str() {
                    "imap" => match tls {
                        Some(tls) => {
                            loom_hosted::serve_mail_imap_tls(
                                listener,
                                tls,
                                kernel,
                                workspace,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                        None => {
                            loom_hosted::serve_mail_imap(
                                listener,
                                kernel,
                                workspace,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                    },
                    "jmap" => match tls {
                        Some(tls) => {
                            loom_hosted::serve_mail_jmap_tls_with_limits(
                                listener,
                                tls,
                                kernel,
                                workspace,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                        None => {
                            loom_hosted::serve_mail_jmap_with_limits(
                                listener,
                                kernel,
                                workspace,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                    },
                    "smtp" => match (tls_mode.as_str(), tls) {
                        ("direct", Some(tls)) => {
                            loom_hosted::serve_mail_smtp_tls(
                                listener,
                                tls,
                                kernel,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                        ("starttls", Some(tls)) => {
                            loom_hosted::serve_mail_smtp_starttls(
                                listener,
                                tls,
                                kernel,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                        ("off", None) => {
                            loom_hosted::serve_mail_smtp(
                                listener,
                                kernel,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                        (mode, _) => {
                            return Err(format!(
                                "mail/smtp listener {bind} has invalid TLS mode {mode:?}"
                            ));
                        }
                    },
                    _ => return Err(format!("unsupported mail transport {transport}")),
                }
                .map_err(|e| format!("serve mail/{transport} listener {bind}: {e}"))
            },
        ),
    )
}

#[cfg(feature = "serve")]
fn served_listener_http_limits(
    record: &ServedListenerRecord,
) -> Result<loom_hosted::HostedHttpLimits, String> {
    let request_size_limit = record.limits.request_size_limit.try_into().map_err(|_| {
        format!(
            "served listener {} request-size limit is too large",
            record.id
        )
    })?;
    loom_hosted::HostedHttpLimits::new(
        request_size_limit,
        record.limits.idle_timeout_ms,
        record.limits.session_timeout_ms,
    )
    .map_err(|e| format!("served listener {} has invalid HTTP limits: {e}", record.id))
}

#[cfg(feature = "serve")]
fn served_listener_runtime_target(
    store: &str,
    record: &ServedListenerRecord,
) -> Result<String, String> {
    let mut target = served_listener_target(record);
    if let Some(fingerprint) = served_listener_tls_certificate_bundle_fingerprint(store, record)? {
        target.push_str(&fingerprint.audit_suffix);
    }
    if let Some(fingerprint) = served_listener_network_access_policy_fingerprint(store, record)? {
        target.push_str(&fingerprint.audit_suffix);
    }
    Ok(target)
}

#[cfg(feature = "serve")]
fn load_web_listener_config(
    store: &str,
    record: &ServedListenerRecord,
    default_workspace: WorkspaceId,
) -> Result<Option<loom_substrate::web::WebListener>, String> {
    let key = loom_substrate::web::web_profile_listener_key(&record.id).map_err(|e| {
        format!(
            "build Webish listener config key for served listener {}: {e}",
            record.id
        )
    })?;
    let fs = FileStore::open_read(store)
        .map_err(|e| format!("open store to load Webish listener {}: {e}", record.id))?;
    let Some(bytes) = fs
        .control_get(&key)
        .map_err(|e| format!("load Webish listener config {}: {e}", record.id))?
    else {
        return Ok(None);
    };
    let listener = loom_substrate::web::WebListener::decode(&bytes)
        .map_err(|e| format!("decode Webish listener config {}: {e}", record.id))?;
    if listener.default_workspace != default_workspace {
        return Err(format!(
            "Webish listener config {} default workspace does not match served listener selector",
            record.id
        ));
    }
    Ok(Some(listener))
}

#[cfg(feature = "serve")]
fn dav_listener_runtime_target(
    store: &str,
    records: &[ServedListenerRecord],
) -> Result<String, String> {
    let mut target = String::from("dav");
    for record in records {
        target.push_str(";record=(");
        target.push_str(&served_listener_target(record));
        target.push(')');
    }
    if let Some(fingerprint) = shared_tls_certificate_bundle_fingerprint(store, records)? {
        target.push_str(";tls=");
        target.push_str(&fingerprint);
    }
    if let Some(fingerprint) = shared_network_access_policy_fingerprint(store, records)? {
        target.push_str(";network=");
        target.push_str(&fingerprint);
    }
    Ok(target)
}

#[cfg(feature = "serve")]
fn served_listener_tls_certificate_bundle_fingerprint(
    store: &str,
    record: &ServedListenerRecord,
) -> Result<Option<HostedTlsBundleFingerprint>, String> {
    match record.tls.mode.as_str() {
        "off" => Ok(None),
        "direct" | "starttls" => {
            let bundle_name = record
                .tls
                .certificate_bundle_ref
                .as_deref()
                .ok_or_else(|| {
                    format!(
                        "served listener {} is missing TLS certificate bundle ref",
                        record.id
                    )
                })?;
            let bundle = FileStore::open_read(store)
                .map_err(|e| format!("open store to fingerprint TLS bundle {bundle_name:?}: {e}"))?
                .certificate_bundle(bundle_name)
                .map_err(|e| format!("fingerprint TLS bundle {bundle_name:?}: {e}"))?
                .ok_or_else(|| format!("TLS certificate bundle {bundle_name:?} not found"))?;
            let trust_digest = bundle
                .trust_bundle_digest
                .map(|digest| digest.to_string())
                .unwrap_or_else(|| "none".to_string());
            Ok(Some(HostedTlsBundleFingerprint {
                internal: format!(
                    "name={bundle_name};server={};key={};trust={trust_digest}",
                    bundle.server_cert_chain_digest, bundle.private_key_digest
                ),
                audit_suffix: format!(
                    ";tls_certificate_bundle={bundle_name};tls_server_chain_digest={};tls_trust_bundle_digest={trust_digest}",
                    bundle.server_cert_chain_digest
                ),
            }))
        }
        other => Err(format!(
            "served listener {} uses unsupported TLS mode {other:?}",
            record.id
        )),
    }
}

#[cfg(feature = "serve")]
fn served_listener_tls_config(
    store: &str,
    record: &ServedListenerRecord,
) -> Result<Option<loom_hosted::HostedTlsConfig>, String> {
    match record.tls.mode.as_str() {
        "off" => Ok(None),
        "direct" | "starttls" => {
            let bundle_name = record
                .tls
                .certificate_bundle_ref
                .as_deref()
                .ok_or_else(|| {
                    format!(
                        "served listener {} is missing TLS certificate bundle ref",
                        record.id
                    )
                })?;
            let fs = FileStore::open_read(store)
                .map_err(|e| format!("open store to load TLS bundle {bundle_name:?}: {e}"))?;
            let bundle = fs
                .certificate_bundle(bundle_name)
                .map_err(|e| format!("load TLS bundle {bundle_name:?}: {e}"))?
                .ok_or_else(|| format!("TLS certificate bundle {bundle_name:?} not found"))?;
            let cert_ref = format!("certificate bundle {bundle_name} server chain");
            let key_ref = format!("certificate bundle {bundle_name} private key");
            let require_client_auth = match record.network_access_policy_ref.as_deref() {
                Some(policy_name) => {
                    let policy = fs
                        .network_access_policy(policy_name)
                        .map_err(|e| format!("load network access policy {policy_name:?}: {e}"))?
                        .ok_or_else(|| {
                            format!("network access policy {policy_name:?} not found")
                        })?;
                    served_listener_network_access_policy_requires_mtls(&policy)
                }
                None => false,
            };
            let trust_bundle_ref = if require_client_auth {
                Some((
                    format!("certificate bundle {bundle_name} trust bundle"),
                    bundle.trust_bundle_pem.as_deref().ok_or_else(|| {
                        format!(
                            "served listener {} requires mTLS but certificate bundle {bundle_name:?} has no trust bundle",
                            record.id
                        )
                    })?,
                ))
            } else {
                None
            };
            if let Some(trust_bundle_pem) = bundle.trust_bundle_pem.as_deref() {
                validate_tls_trust_bundle_pem(
                    &format!("certificate bundle {bundle_name} trust bundle"),
                    trust_bundle_pem,
                )
                .map_err(|e| format!("load TLS material for served listener {}: {e}", record.id))?;
            }
            loom_hosted::HostedTlsConfig::from_pem_bytes_with_client_trust(
                &cert_ref,
                &bundle.server_cert_chain_pem,
                &key_ref,
                &bundle.private_key_pem,
                trust_bundle_ref
                    .as_ref()
                    .map(|(label, pem)| (label.as_str(), *pem)),
            )
            .map(Some)
            .map_err(|e| format!("load TLS material for served listener {}: {e}", record.id))
        }
        other => Err(format!(
            "served listener {} uses unsupported TLS mode {other:?}",
            record.id
        )),
    }
}

#[cfg(feature = "serve")]
fn validate_tls_trust_bundle_pem(label: &str, pem: &[u8]) -> std::io::Result<()> {
    use rustls::pki_types::pem::PemObject;

    crate::tls_crypto::ensure_rustls_crypto_provider();
    let mut roots = rustls::RootCertStore::empty();
    let mut count = 0usize;
    for cert in rustls::pki_types::CertificateDer::pem_slice_iter(pem) {
        roots
            .add(cert.map_err(|e| invalid_tls_trust_bundle(label, e))?)
            .map_err(|e| invalid_tls_trust_bundle(label, e))?;
        count += 1;
    }
    if count == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("TLS trust bundle {label:?} contains no certificates"),
        ));
    }
    Ok(())
}

#[cfg(feature = "serve")]
fn invalid_tls_trust_bundle(
    label: &str,
    source: impl std::error::Error + Send + Sync + 'static,
) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!("invalid TLS trust bundle {label:?}: {source}"),
    )
}

#[cfg(feature = "serve")]
fn served_listener_network_access_policy(
    store: &str,
    record: &ServedListenerRecord,
) -> Result<Option<loom_store::NetworkAccessPolicyRecord>, String> {
    let Some(policy_name) = record.network_access_policy_ref.as_deref() else {
        return Ok(None);
    };
    let fs = FileStore::open_read(store)
        .map_err(|e| format!("open store to load network access policy {policy_name:?}: {e}"))?;
    let policy = fs
        .network_access_policy(policy_name)
        .map_err(|e| format!("load network access policy {policy_name:?}: {e}"))?
        .ok_or_else(|| format!("network access policy {policy_name:?} not found"))?;
    if served_listener_network_access_policy_requires_mtls(&policy) {
        if record.tls.mode != "direct" {
            return Err(format!(
                "network access policy {policy_name:?} requires mTLS but listener TLS is not direct"
            ));
        }
        let bundle_name = record
            .tls
            .certificate_bundle_ref
            .as_deref()
            .ok_or_else(|| {
                format!("network access policy {policy_name:?} requires a TLS certificate bundle")
            })?;
        let bundle = fs
            .certificate_bundle(bundle_name)
            .map_err(|e| format!("load TLS bundle {bundle_name:?}: {e}"))?
            .ok_or_else(|| format!("TLS certificate bundle {bundle_name:?} not found"))?;
        if bundle.trust_bundle_pem.is_none() {
            return Err(format!(
                "network access policy {policy_name:?} requires mTLS but certificate bundle {bundle_name:?} has no trust bundle"
            ));
        }
    }
    Ok(Some(policy))
}

#[cfg(any(feature = "serve", feature = "mcp"))]
fn served_listener_network_access_policy_requires_mtls(
    policy: &loom_store::NetworkAccessPolicyRecord,
) -> bool {
    policy.rules.iter().any(|rule| {
        rule.require_mtls
            || rule.client_cert_subject.is_some()
            || rule.client_cert_san.is_some()
            || rule.client_cert_issuer.is_some()
    })
}

#[cfg(feature = "serve")]
fn served_listener_network_access_policy_fingerprint(
    store: &str,
    record: &ServedListenerRecord,
) -> Result<Option<HostedNetworkAccessPolicyFingerprint>, String> {
    let Some(policy_name) = record.network_access_policy_ref.as_deref() else {
        return Ok(None);
    };
    let fs = FileStore::open_read(store).map_err(|e| {
        format!("open store to fingerprint network access policy {policy_name:?}: {e}")
    })?;
    let policy = fs
        .network_access_policy(policy_name)
        .map_err(|e| format!("fingerprint network access policy {policy_name:?}: {e}"))?
        .ok_or_else(|| format!("network access policy {policy_name:?} not found"))?;
    let digest = fs
        .network_access_policy_digest(&policy)
        .map_err(|e| format!("fingerprint network access policy {policy_name:?}: {e}"))?;
    Ok(Some(HostedNetworkAccessPolicyFingerprint {
        internal: format!("name={policy_name};digest={digest}"),
        audit_suffix: format!(
            ";network_access_policy={policy_name};network_access_digest={digest}"
        ),
    }))
}

#[cfg(feature = "serve")]
fn served_listener_auth_policy(
    record: &ServedListenerRecord,
) -> Result<loom_hosted::HostedAuthPolicy, String> {
    match record.auth.mode.as_str() {
        "owner-or-passphrase" => Ok(loom_hosted::HostedAuthPolicy::OwnerOrPassphrase),
        "passphrase" => Ok(loom_hosted::HostedAuthPolicy::Passphrase),
        other => Err(format!(
            "served listener {} uses unsupported auth mode {other:?}",
            record.id
        )),
    }
}

#[cfg(feature = "serve")]
struct ServeAdminThread {
    listener_id: String,
    store: String,
    bind: String,
    transport: String,
    limits: loom_hosted::HostedHttpLimits,
    tls: Option<loom_hosted::HostedTlsConfig>,
    auth_policy: loom_hosted::HostedAuthPolicy,
    network_access_policy: Option<loom_store::NetworkAccessPolicyRecord>,
    kernel: loom_hosted::HostedKernel,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ready_tx: std::sync::mpsc::SyncSender<Result<String, String>>,
}

#[cfg(feature = "serve")]
struct ExecThreadRequest {
    listener_id: String,
    store: String,
    bind: String,
    transport: String,
    limits: loom_hosted::HostedHttpLimits,
    tls: Option<loom_hosted::HostedTlsConfig>,
    auth_policy: loom_hosted::HostedAuthPolicy,
    network_access_policy: Option<loom_store::NetworkAccessPolicyRecord>,
    kernel: loom_hosted::HostedKernel,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ready_tx: std::sync::mpsc::SyncSender<Result<String, String>>,
}

#[cfg(feature = "serve")]
fn serve_exec_thread(request: ExecThreadRequest) -> Result<(), String> {
    let ExecThreadRequest {
        listener_id,
        store,
        bind,
        transport,
        limits,
        tls,
        auth_policy,
        network_access_policy,
        kernel,
        shutdown_rx,
        ready_tx,
    } = request;
    let denied_audit = Some(network_access_denied_audit_sink(store));
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            let err = format!("build exec/{transport} runtime: {e}");
            let _ = ready_tx.send(Err(err.clone()));
            return Err(err);
        }
    };
    runtime.block_on(
        loom_hosted::with_hosted_network_access_policy_for_listener_and_audit(
            listener_id,
            network_access_policy,
            denied_audit,
            async move {
                let listener = match tokio::net::TcpListener::bind(&bind).await {
                    Ok(listener) => listener,
                    Err(e) => {
                        let err = format!("bind exec/{transport} listener {bind}: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let local_addr = match listener.local_addr() {
                    Ok(addr) => addr,
                    Err(e) => {
                        let err = format!("read exec/{transport} listener address: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let _ = ready_tx.send(Ok(local_addr.to_string()));
                match transport.as_str() {
                    "rest" => match tls {
                        Some(tls) => {
                            loom_hosted::serve_exec_rest_tls_with_limits(
                                listener,
                                tls,
                                kernel,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                        None => {
                            loom_hosted::serve_exec_rest_with_limits(
                                listener,
                                kernel,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                    },
                    "json_rpc" => match tls {
                        Some(tls) => {
                            loom_hosted::serve_exec_jsonrpc_tls_with_limits(
                                listener,
                                tls,
                                kernel,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                        None => {
                            loom_hosted::serve_exec_jsonrpc_with_limits(
                                listener,
                                kernel,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                    },
                    "grpc" => {
                        if tls.is_some() {
                            return Err("direct TLS for exec/grpc is not supported".to_string());
                        }
                        loom_hosted::serve_exec_grpc(listener, kernel, async move {
                            let _ = shutdown_rx.await;
                        })
                        .await
                    }
                    _ => unreachable!("unsupported exec transport already filtered"),
                }
                .map_err(|e| format!("serve exec/{transport} listener {bind}: {e}"))
            },
        ),
    )
}

#[cfg(feature = "serve")]
fn serve_admin_thread(thread: ServeAdminThread) -> Result<(), String> {
    let ServeAdminThread {
        listener_id,
        store,
        bind,
        transport,
        limits,
        tls,
        auth_policy,
        network_access_policy,
        kernel,
        shutdown_rx,
        ready_tx,
    } = thread;
    let denied_audit = Some(network_access_denied_audit_sink(store));
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            let err = format!("build admin/{transport} runtime: {e}");
            let _ = ready_tx.send(Err(err.clone()));
            return Err(err);
        }
    };
    runtime.block_on(
        loom_hosted::with_hosted_network_access_policy_for_listener_and_audit(
            listener_id,
            network_access_policy,
            denied_audit,
            async move {
                let listener = match tokio::net::TcpListener::bind(&bind).await {
                    Ok(listener) => listener,
                    Err(e) => {
                        let err = format!("bind admin/{transport} listener {bind}: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let local_addr = match listener.local_addr() {
                    Ok(addr) => addr,
                    Err(e) => {
                        let err = format!("read admin/{transport} listener address: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let _ = ready_tx.send(Ok(local_addr.to_string()));
                match transport.as_str() {
                    "rest" => match tls {
                        Some(tls) => {
                            loom_hosted::serve_admin_rest_tls_with_limits(
                                listener,
                                tls,
                                kernel,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                        None => {
                            loom_hosted::serve_admin_rest_with_limits(
                                listener,
                                kernel,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                    },
                    "json_rpc" => match tls {
                        Some(tls) => {
                            loom_hosted::serve_admin_jsonrpc_tls_with_limits(
                                listener,
                                tls,
                                kernel,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                        None => {
                            loom_hosted::serve_admin_jsonrpc_with_limits(
                                listener,
                                kernel,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                    },
                    _ => unreachable!("unsupported admin transport already filtered"),
                }
                .map_err(|e| format!("serve admin/{transport} listener {bind}: {e}"))
            },
        ),
    )
}

#[cfg(feature = "serve")]
struct SqlThreadRequest {
    listener_id: String,
    store: String,
    bind: String,
    surface: String,
    transport: String,
    limits: loom_hosted::HostedHttpLimits,
    tls: Option<loom_hosted::HostedTlsConfig>,
    auth_policy: loom_hosted::HostedAuthPolicy,
    network_access_policy: Option<loom_store::NetworkAccessPolicyRecord>,
    kernel: loom_hosted::HostedKernel,
    workspace: String,
    database: String,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ready_tx: std::sync::mpsc::SyncSender<Result<String, String>>,
}

#[cfg(feature = "serve")]
fn serve_sql_thread(request: SqlThreadRequest) -> Result<(), String> {
    let SqlThreadRequest {
        listener_id,
        store,
        bind,
        surface,
        transport,
        limits,
        tls,
        auth_policy,
        network_access_policy,
        kernel,
        workspace,
        database,
        shutdown_rx,
        ready_tx,
    } = request;
    let display_transport = if matches!(surface.as_str(), "postgres" | "mysql") {
        "tcp".to_string()
    } else {
        transport.clone()
    };
    let denied_audit = Some(network_access_denied_audit_sink(store));
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            let err = format!("build {surface}/{display_transport} runtime: {e}");
            let _ = ready_tx.send(Err(err.clone()));
            return Err(err);
        }
    };
    runtime.block_on(
        loom_hosted::with_hosted_network_access_policy_for_listener_and_audit(
            listener_id,
            network_access_policy,
            denied_audit,
            async move {
                let listener = match tokio::net::TcpListener::bind(&bind).await {
                    Ok(listener) => listener,
                    Err(e) => {
                        let err = format!(
                            "bind {surface}/{display_transport} listener {bind}: {e}"
                        );
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let local_addr = match listener.local_addr() {
                    Ok(addr) => addr,
                    Err(e) => {
                        let err = format!(
                            "read {surface}/{display_transport} listener address: {e}"
                        );
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let _ = ready_tx.send(Ok(local_addr.to_string()));
                match transport.as_str() {
                    "rest" => match tls {
                        Some(tls) => {
                            loom_hosted::serve_sql_rest_tls_with_limits(
                                listener,
                                tls,
                                kernel,
                                loom_hosted::HostedSqlTarget::new(workspace, database),
                                loom_hosted::HostedServePolicy::new(limits, auth_policy),
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                        None => {
                            loom_hosted::serve_sql_rest_with_limits(
                                listener,
                                kernel,
                                workspace,
                                database,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                    },
                    "json_rpc" => match tls {
                        Some(tls) => {
                            loom_hosted::serve_sql_jsonrpc_tls_with_limits(
                                listener,
                                tls,
                                kernel,
                                loom_hosted::HostedSqlTarget::new(workspace, database),
                                loom_hosted::HostedServePolicy::new(limits, auth_policy),
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                        None => {
                            loom_hosted::serve_sql_jsonrpc_with_limits(
                                listener,
                                kernel,
                                workspace,
                                database,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                    },
                    "grpc" => {
                        loom_hosted::serve_sql_grpc(
                            listener,
                            kernel,
                            workspace,
                            database,
                            async move {
                                let _ = shutdown_rx.await;
                            },
                        )
                        .await
                    }
                    "pg_wire" => {
                        if tls.is_some() {
                            return Err(format!(
                                "direct TLS is not supported for {surface}/{display_transport} listeners"
                            ));
                        }
                        if !matches!(
                            auth_policy,
                            loom_hosted::HostedAuthPolicy::OwnerOrPassphrase
                        ) {
                            return Err(format!(
                                "{surface}/{display_transport} requires owner-or-passphrase auth policy"
                            ));
                        }
                        loom_hosted::serve_sql_pg_wire(
                            listener,
                            kernel,
                            workspace,
                            database,
                            async move {
                                let _ = shutdown_rx.await;
                            },
                        )
                        .await
                    }
                    "mysql_wire" => {
                        if tls.is_some() {
                            return Err(format!(
                                "direct TLS is not supported for {surface}/{display_transport} listeners"
                            ));
                        }
                        if !matches!(
                            auth_policy,
                            loom_hosted::HostedAuthPolicy::OwnerOrPassphrase
                        ) {
                            return Err(format!(
                                "{surface}/{display_transport} requires owner-or-passphrase auth policy"
                            ));
                        }
                        loom_hosted::serve_sql_mysql_wire(
                            listener,
                            kernel,
                            workspace,
                            database,
                            async move {
                                let _ = shutdown_rx.await;
                            },
                        )
                        .await
                    }
                    _ => unreachable!("unsupported sql transport already filtered"),
                }
                .map_err(|e| {
                    format!("serve {surface}/{display_transport} listener {bind}: {e}")
                })
            },
        ),
    )
}

#[cfg(feature = "serve")]
struct DataThreadRequest {
    listener_id: String,
    store: String,
    bind: String,
    surface: String,
    transport: String,
    profile: Option<String>,
    limits: loom_hosted::HostedHttpLimits,
    tls: Option<loom_hosted::HostedTlsConfig>,
    auth_policy: loom_hosted::HostedAuthPolicy,
    network_access_policy: Option<loom_store::NetworkAccessPolicyRecord>,
    kernel: loom_hosted::HostedKernel,
    workspace: String,
    collection: String,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ready_tx: std::sync::mpsc::SyncSender<Result<String, String>>,
}

#[cfg(feature = "serve")]
struct InfluxThreadRequest {
    listener_id: String,
    store: String,
    bind: String,
    surface: String,
    transport: String,
    limits: loom_hosted::HostedHttpLimits,
    auth_policy: loom_hosted::HostedAuthPolicy,
    network_access_policy: Option<loom_store::NetworkAccessPolicyRecord>,
    kernel: loom_hosted::HostedKernel,
    workspace: String,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ready_tx: std::sync::mpsc::SyncSender<Result<String, String>>,
}

#[cfg(feature = "serve")]
struct PrometheusThreadRequest {
    listener_id: String,
    store: String,
    bind: String,
    surface: String,
    transport: String,
    limits: loom_hosted::HostedHttpLimits,
    auth_policy: loom_hosted::HostedAuthPolicy,
    network_access_policy: Option<loom_store::NetworkAccessPolicyRecord>,
    kernel: loom_hosted::HostedKernel,
    workspace: String,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ready_tx: std::sync::mpsc::SyncSender<Result<String, String>>,
}

#[cfg(feature = "serve")]
struct GrafanaThreadRequest {
    listener_id: String,
    store: String,
    bind: String,
    surface: String,
    transport: String,
    limits: loom_hosted::HostedHttpLimits,
    auth_policy: loom_hosted::HostedAuthPolicy,
    network_access_policy: Option<loom_store::NetworkAccessPolicyRecord>,
    kernel: loom_hosted::HostedKernel,
    workspace: String,
    collection: String,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ready_tx: std::sync::mpsc::SyncSender<Result<String, String>>,
}

#[cfg(feature = "serve")]
struct OtlpThreadRequest {
    listener_id: String,
    store: String,
    bind: String,
    surface: String,
    transport: String,
    limits: loom_hosted::HostedHttpLimits,
    auth_policy: loom_hosted::HostedAuthPolicy,
    network_access_policy: Option<loom_store::NetworkAccessPolicyRecord>,
    kernel: loom_hosted::HostedKernel,
    workspace: String,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ready_tx: std::sync::mpsc::SyncSender<Result<String, String>>,
}

#[cfg(feature = "serve")]
struct OciThreadRequest {
    listener_id: String,
    store: String,
    bind: String,
    transport: String,
    limits: loom_hosted::HostedHttpLimits,
    tls: Option<loom_hosted::HostedTlsConfig>,
    auth_policy: loom_hosted::HostedAuthPolicy,
    network_access_policy: Option<loom_store::NetworkAccessPolicyRecord>,
    kernel: loom_hosted::HostedKernel,
    workspace: String,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ready_tx: std::sync::mpsc::SyncSender<Result<String, String>>,
}

#[cfg(feature = "serve")]
struct S3ThreadRequest {
    listener_id: String,
    store: String,
    bind: String,
    transport: String,
    limits: loom_hosted::HostedHttpLimits,
    tls: Option<loom_hosted::HostedTlsConfig>,
    auth_policy: loom_hosted::HostedAuthPolicy,
    network_access_policy: Option<loom_store::NetworkAccessPolicyRecord>,
    kernel: loom_hosted::HostedKernel,
    workspace: String,
    bucket: Option<String>,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ready_tx: std::sync::mpsc::SyncSender<Result<String, String>>,
}

#[cfg(feature = "serve")]
struct RedisThreadRequest {
    listener_id: String,
    store: String,
    bind: String,
    surface: String,
    transport: String,
    network_access_policy: Option<loom_store::NetworkAccessPolicyRecord>,
    kernel: loom_hosted::HostedKernel,
    workspace: String,
    keyspace: String,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ready_tx: std::sync::mpsc::SyncSender<Result<String, String>>,
}

#[cfg(feature = "serve")]
struct MemcachedThreadRequest {
    listener_id: String,
    store: String,
    bind: String,
    surface: String,
    transport: String,
    network_access_policy: Option<loom_store::NetworkAccessPolicyRecord>,
    kernel: loom_hosted::HostedKernel,
    workspace: String,
    display_cache_name: String,
    collection: String,
    mode: Option<loom_hosted::MemcachedCacheMode>,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ready_tx: std::sync::mpsc::SyncSender<Result<String, String>>,
}

#[cfg(feature = "serve")]
struct EtcdThreadRequest {
    listener_id: String,
    store: String,
    bind: String,
    surface: String,
    transport: String,
    network_access_policy: Option<loom_store::NetworkAccessPolicyRecord>,
    kernel: loom_hosted::HostedKernel,
    workspace: String,
    collection: String,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ready_tx: std::sync::mpsc::SyncSender<Result<String, String>>,
}

#[cfg(feature = "serve")]
struct KafkaThreadRequest {
    listener_id: String,
    store: String,
    bind: String,
    surface: String,
    transport: String,
    network_access_policy: Option<loom_store::NetworkAccessPolicyRecord>,
    kernel: loom_hosted::HostedKernel,
    workspace: String,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ready_tx: std::sync::mpsc::SyncSender<Result<String, String>>,
}

#[cfg(feature = "serve")]
struct Neo4jThreadRequest {
    listener_id: String,
    store: String,
    bind: String,
    surface: String,
    transport: String,
    network_access_policy: Option<loom_store::NetworkAccessPolicyRecord>,
    kernel: loom_hosted::HostedKernel,
    workspace: String,
    graph: String,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ready_tx: std::sync::mpsc::SyncSender<Result<String, String>>,
}

#[cfg(feature = "serve")]
struct PimThreadRequest {
    listener_id: String,
    store: String,
    bind: String,
    surface: String,
    transport: String,
    limits: loom_hosted::HostedHttpLimits,
    tls: Option<loom_hosted::HostedTlsConfig>,
    auth_policy: loom_hosted::HostedAuthPolicy,
    network_access_policy: Option<loom_store::NetworkAccessPolicyRecord>,
    kernel: loom_hosted::HostedKernel,
    workspace: String,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ready_tx: std::sync::mpsc::SyncSender<Result<String, String>>,
}

#[cfg(feature = "serve")]
struct DavThreadRequest {
    listener_id: String,
    store: String,
    bind: String,
    limits: loom_hosted::HostedHttpLimits,
    tls: Option<loom_hosted::HostedTlsConfig>,
    auth_policy: loom_hosted::HostedAuthPolicy,
    network_access_policy: Option<loom_store::NetworkAccessPolicyRecord>,
    kernel: loom_hosted::HostedKernel,
    caldav_workspace: Option<String>,
    carddav_workspace: Option<String>,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ready_tx: std::sync::mpsc::SyncSender<Result<String, String>>,
}

#[cfg(feature = "serve")]
fn serve_redis_thread(request: RedisThreadRequest) -> Result<(), String> {
    let RedisThreadRequest {
        listener_id,
        store,
        bind,
        surface,
        transport,
        network_access_policy,
        kernel,
        workspace,
        keyspace,
        shutdown_rx,
        ready_tx,
    } = request;
    let denied_audit = Some(network_access_denied_audit_sink(store));
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            let err = format!("build {surface}/{transport} runtime: {e}");
            let _ = ready_tx.send(Err(err.clone()));
            return Err(err);
        }
    };
    runtime.block_on(
        loom_hosted::with_hosted_network_access_policy_for_listener_and_audit(
            listener_id,
            network_access_policy,
            denied_audit,
            async move {
                let listener = match tokio::net::TcpListener::bind(&bind).await {
                    Ok(listener) => listener,
                    Err(e) => {
                        let err = format!("bind {surface}/{transport} listener {bind}: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let local_addr = match listener.local_addr() {
                    Ok(addr) => addr,
                    Err(e) => {
                        let err = format!("read {surface}/{transport} listener address: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let _ = ready_tx.send(Ok(local_addr.to_string()));
                loom_hosted::serve_redis_resp(listener, kernel, workspace, keyspace, async move {
                    let _ = shutdown_rx.await;
                })
                .await
                .map_err(|e| format!("serve {surface}/{transport} listener {bind}: {e}"))
            },
        ),
    )
}

#[cfg(feature = "serve")]
fn serve_memcached_thread(request: MemcachedThreadRequest) -> Result<(), String> {
    let MemcachedThreadRequest {
        listener_id,
        store,
        bind,
        surface,
        transport,
        network_access_policy,
        kernel,
        workspace,
        display_cache_name,
        collection,
        mode,
        shutdown_rx,
        ready_tx,
    } = request;
    let denied_audit = Some(network_access_denied_audit_sink(store));
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            let err = format!("build {surface}/{transport} runtime: {e}");
            let _ = ready_tx.send(Err(err.clone()));
            return Err(err);
        }
    };
    runtime.block_on(
        loom_hosted::with_hosted_network_access_policy_for_listener_and_audit(
            listener_id,
            network_access_policy,
            denied_audit,
            async move {
                let listener = match tokio::net::TcpListener::bind(&bind).await {
                    Ok(listener) => listener,
                    Err(e) => {
                        let err = format!("bind {surface}/{transport} listener {bind}: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let local_addr = match listener.local_addr() {
                    Ok(addr) => addr,
                    Err(e) => {
                        let err = format!("read {surface}/{transport} listener address: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let _ = ready_tx.send(Ok(local_addr.to_string()));
                let shutdown = async move {
                    let _ = shutdown_rx.await;
                };
                let served = if let Some(mode) = mode {
                    loom_hosted::serve_memcached_text_backed(
                        listener,
                        kernel,
                        workspace,
                        display_cache_name,
                        collection,
                        mode,
                        shutdown,
                    )
                    .await
                } else {
                    loom_hosted::serve_memcached_text(listener, display_cache_name, shutdown).await
                };
                served.map_err(|e| format!("serve {surface}/{transport} listener {bind}: {e}"))
            },
        ),
    )
}

#[cfg(feature = "serve")]
fn serve_etcd_thread(request: EtcdThreadRequest) -> Result<(), String> {
    let EtcdThreadRequest {
        listener_id,
        store,
        bind,
        surface,
        transport,
        network_access_policy,
        kernel,
        workspace,
        collection,
        shutdown_rx,
        ready_tx,
    } = request;
    let denied_audit = Some(network_access_denied_audit_sink(store));
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            let err = format!("build {surface}/{transport} runtime: {e}");
            let _ = ready_tx.send(Err(err.clone()));
            return Err(err);
        }
    };
    runtime.block_on(
        loom_hosted::with_hosted_network_access_policy_for_listener_and_audit(
            listener_id,
            network_access_policy,
            denied_audit,
            async move {
                let listener = match tokio::net::TcpListener::bind(&bind).await {
                    Ok(listener) => listener,
                    Err(e) => {
                        let err = format!("bind {surface}/{transport} listener {bind}: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let local_addr = match listener.local_addr() {
                    Ok(addr) => addr,
                    Err(e) => {
                        let err = format!("read {surface}/{transport} listener address: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let _ = ready_tx.send(Ok(local_addr.to_string()));
                loom_hosted::serve_etcd_grpc(listener, kernel, workspace, collection, async move {
                    let _ = shutdown_rx.await;
                })
                .await
                .map_err(|e| format!("serve {surface}/{transport} listener {bind}: {e}"))
            },
        ),
    )
}

#[cfg(feature = "serve")]
fn serve_kafka_thread(request: KafkaThreadRequest) -> Result<(), String> {
    let KafkaThreadRequest {
        listener_id,
        store,
        bind,
        surface,
        transport,
        network_access_policy,
        kernel,
        workspace,
        shutdown_rx,
        ready_tx,
    } = request;
    let denied_audit = Some(network_access_denied_audit_sink(store));
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            let err = format!("build {surface}/{transport} runtime: {e}");
            let _ = ready_tx.send(Err(err.clone()));
            return Err(err);
        }
    };
    runtime.block_on(
        loom_hosted::with_hosted_network_access_policy_for_listener_and_audit(
            listener_id,
            network_access_policy,
            denied_audit,
            async move {
                let listener = match tokio::net::TcpListener::bind(&bind).await {
                    Ok(listener) => listener,
                    Err(e) => {
                        let err = format!("bind {surface}/{transport} listener {bind}: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let local_addr = match listener.local_addr() {
                    Ok(addr) => addr,
                    Err(e) => {
                        let err = format!("read {surface}/{transport} listener address: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let _ = ready_tx.send(Ok(local_addr.to_string()));
                loom_hosted::serve_kafka_tcp(listener, kernel, workspace, async move {
                    let _ = shutdown_rx.await;
                })
                .await
                .map_err(|e| format!("serve {surface}/{transport} listener {bind}: {e}"))
            },
        ),
    )
}

#[cfg(feature = "serve")]
fn serve_neo4j_thread(request: Neo4jThreadRequest) -> Result<(), String> {
    let Neo4jThreadRequest {
        listener_id,
        store,
        bind,
        surface,
        transport,
        network_access_policy,
        kernel,
        workspace,
        graph,
        shutdown_rx,
        ready_tx,
    } = request;
    let denied_audit = Some(network_access_denied_audit_sink(store));
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            let err = format!("build {surface}/{transport} runtime: {e}");
            let _ = ready_tx.send(Err(err.clone()));
            return Err(err);
        }
    };
    runtime.block_on(
        loom_hosted::with_hosted_network_access_policy_for_listener_and_audit(
            listener_id,
            network_access_policy,
            denied_audit,
            async move {
                let listener = match tokio::net::TcpListener::bind(&bind).await {
                    Ok(listener) => listener,
                    Err(e) => {
                        let err = format!("bind {surface}/{transport} listener {bind}: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let local_addr = match listener.local_addr() {
                    Ok(addr) => addr,
                    Err(e) => {
                        let err = format!("read {surface}/{transport} listener address: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let _ = ready_tx.send(Ok(local_addr.to_string()));
                loom_hosted::serve_neo4j_tcp(listener, kernel, workspace, graph, async move {
                    let _ = shutdown_rx.await;
                })
                .await
                .map_err(|e| format!("serve {surface}/{transport} listener {bind}: {e}"))
            },
        ),
    )
}

#[cfg(feature = "serve")]
fn serve_pim_thread(request: PimThreadRequest) -> Result<(), String> {
    let PimThreadRequest {
        listener_id,
        store,
        bind,
        surface,
        transport,
        limits,
        tls,
        auth_policy,
        network_access_policy,
        kernel,
        workspace,
        shutdown_rx,
        ready_tx,
    } = request;
    let denied_audit = Some(network_access_denied_audit_sink(store));
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            let err = format!("build {surface}/{transport} runtime: {e}");
            let _ = ready_tx.send(Err(err.clone()));
            return Err(err);
        }
    };
    runtime.block_on(
        loom_hosted::with_hosted_network_access_policy_for_listener_and_audit(
            listener_id,
            network_access_policy,
            denied_audit,
            async move {
                let listener = match tokio::net::TcpListener::bind(&bind).await {
                    Ok(listener) => listener,
                    Err(e) => {
                        let err = format!("bind {surface}/{transport} listener {bind}: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let local_addr = match listener.local_addr() {
                    Ok(addr) => addr,
                    Err(e) => {
                        let err = format!("read {surface}/{transport} listener address: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let _ = ready_tx.send(Ok(local_addr.to_string()));
                match (surface.as_str(), transport.as_str()) {
                    ("calendar", "caldav") => match tls {
                        Some(tls) => {
                            loom_hosted::serve_caldav_tls_with_limits(
                                listener,
                                tls,
                                kernel,
                                workspace,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                        None => {
                            loom_hosted::serve_caldav_with_limits(
                                listener,
                                kernel,
                                workspace,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                    },
                    ("contacts", "carddav") => match tls {
                        Some(tls) => {
                            loom_hosted::serve_carddav_tls_with_limits(
                                listener,
                                tls,
                                kernel,
                                workspace,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                        None => {
                            loom_hosted::serve_carddav_with_limits(
                                listener,
                                kernel,
                                workspace,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                    },
                    _ => return Err(format!("unsupported PIM transport {surface}/{transport}")),
                }
                .map_err(|e| format!("serve {surface}/{transport} listener {bind}: {e}"))
            },
        ),
    )
}

#[cfg(feature = "serve")]
fn serve_dav_thread(request: DavThreadRequest) -> Result<(), String> {
    let DavThreadRequest {
        listener_id,
        store,
        bind,
        limits,
        tls,
        auth_policy,
        network_access_policy,
        kernel,
        caldav_workspace,
        carddav_workspace,
        shutdown_rx,
        ready_tx,
    } = request;
    let denied_audit = Some(network_access_denied_audit_sink(store));
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            let err = format!("build shared DAV runtime: {e}");
            let _ = ready_tx.send(Err(err.clone()));
            return Err(err);
        }
    };
    runtime.block_on(
        loom_hosted::with_hosted_network_access_policy_for_listener_and_audit(
            listener_id,
            network_access_policy,
            denied_audit,
            async move {
                let listener = match tokio::net::TcpListener::bind(&bind).await {
                    Ok(listener) => listener,
                    Err(e) => {
                        let err = format!("bind shared DAV listener {bind}: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let local_addr = match listener.local_addr() {
                    Ok(addr) => addr,
                    Err(e) => {
                        let err = format!("read shared DAV listener address: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let _ = ready_tx.send(Ok(local_addr.to_string()));
                let workspaces = loom_hosted::HostedDavWorkspaces {
                    caldav: caldav_workspace,
                    carddav: carddav_workspace,
                };
                match tls {
                    Some(tls) => {
                        loom_hosted::serve_dav_tls_with_limits(
                            listener,
                            tls,
                            kernel,
                            workspaces,
                            limits,
                            auth_policy,
                            async move {
                                let _ = shutdown_rx.await;
                            },
                        )
                        .await
                    }
                    None => {
                        loom_hosted::serve_dav_with_limits(
                            listener,
                            kernel,
                            workspaces,
                            limits,
                            auth_policy,
                            async move {
                                let _ = shutdown_rx.await;
                            },
                        )
                        .await
                    }
                }
                .map_err(|e| format!("serve shared DAV listener {bind}: {e}"))
            },
        ),
    )
}

#[cfg(feature = "serve")]
fn serve_data_thread(request: DataThreadRequest) -> Result<(), String> {
    let DataThreadRequest {
        listener_id,
        store,
        bind,
        surface,
        transport,
        profile,
        limits,
        tls,
        auth_policy,
        network_access_policy,
        kernel,
        workspace,
        collection,
        shutdown_rx,
        ready_tx,
    } = request;
    let denied_audit = Some(network_access_denied_audit_sink(store.clone()));
    let listener_target = listener_id.clone();
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            let err = format!("build {surface}/{transport} runtime: {e}");
            let _ = ready_tx.send(Err(err.clone()));
            return Err(err);
        }
    };
    runtime.block_on(
        loom_hosted::with_hosted_network_access_policy_for_listener_and_audit(
            listener_id,
            network_access_policy,
            denied_audit,
            async move {
                let listener = match tokio::net::TcpListener::bind(&bind).await {
                    Ok(listener) => listener,
                    Err(e) => {
                        let err = format!("bind {surface}/{transport} listener {bind}: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let local_addr = match listener.local_addr() {
                    Ok(addr) => addr,
                    Err(e) => {
                        let err = format!("read {surface}/{transport} listener address: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let open_target = format!("{listener_target};addr={local_addr}");
                if let Err(error) =
                    append_daemon_audit(&store, "serve.listener.open", Some(&open_target))
                {
                    let err = format!("audit {surface}/{transport} listener open: {error}");
                    let _ = ready_tx.send(Err(err.clone()));
                    return Err(err);
                }
                let _ = ready_tx.send(Ok(local_addr.to_string()));
                match transport.as_str() {
                    "rest" | "ndjson" => match tls {
                        Some(tls) => {
                            loom_hosted::serve_data_rest_tls_with_profile(
                                listener,
                                tls,
                                kernel,
                                loom_hosted::HostedDataTarget::with_profile(
                                    surface.as_str(),
                                    workspace,
                                    collection,
                                    profile.clone(),
                                ),
                                loom_hosted::HostedServePolicy::new(limits, auth_policy),
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                        None => {
                            loom_hosted::serve_data_rest_with_profile(
                                listener,
                                kernel,
                                loom_hosted::HostedDataTarget::with_profile(
                                    surface.as_str(),
                                    workspace,
                                    collection,
                                    profile.clone(),
                                ),
                                loom_hosted::HostedServePolicy::new(limits, auth_policy),
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                    },
                    "json_rpc" => match tls {
                        Some(tls) => {
                            loom_hosted::serve_data_jsonrpc_tls_with_profile(
                                listener,
                                tls,
                                kernel,
                                loom_hosted::HostedDataTarget::with_profile(
                                    surface.clone(),
                                    workspace,
                                    collection,
                                    profile.clone(),
                                ),
                                loom_hosted::HostedServePolicy::new(limits, auth_policy),
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                        None => {
                            loom_hosted::serve_data_jsonrpc_with_profile(
                                listener,
                                kernel,
                                loom_hosted::HostedDataTarget::with_profile(
                                    surface.clone(),
                                    workspace,
                                    collection,
                                    profile.clone(),
                                ),
                                loom_hosted::HostedServePolicy::new(limits, auth_policy),
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                    },
                    "grpc" if surface == "vector" && profile.as_deref() == Some("qdrant") => {
                        if tls.is_some() {
                            return Err("direct TLS for vector/grpc is not supported".to_string());
                        }
                        loom_hosted::serve_qdrant_grpc(
                            listener,
                            kernel,
                            workspace,
                            collection,
                            async move {
                                let _ = shutdown_rx.await;
                            },
                        )
                        .await
                    }
                    "grpc" if surface == "kv" && profile.is_none() => {
                        if tls.is_some() {
                            return Err("direct TLS for kv/grpc is not supported".to_string());
                        }
                        loom_hosted::serve_kv_grpc(
                            listener,
                            kernel,
                            workspace,
                            collection,
                            async move {
                                let _ = shutdown_rx.await;
                            },
                        )
                        .await
                    }
                    "grpc" if surface == "queue" && profile.is_none() => {
                        if tls.is_some() {
                            return Err("direct TLS for queue/grpc is not supported".to_string());
                        }
                        loom_hosted::serve_queue_grpc(
                            listener,
                            kernel,
                            workspace,
                            collection,
                            async move {
                                let _ = shutdown_rx.await;
                            },
                        )
                        .await
                    }
                    "grpc" if surface == "graph" && profile.is_none() => {
                        if tls.is_some() {
                            return Err("direct TLS for graph/grpc is not supported".to_string());
                        }
                        loom_hosted::serve_graph_grpc(
                            listener,
                            kernel,
                            workspace,
                            collection,
                            async move {
                                let _ = shutdown_rx.await;
                            },
                        )
                        .await
                    }
                    "grpc" if surface == "ledger" && profile.is_none() => {
                        if tls.is_some() {
                            return Err("direct TLS for ledger/grpc is not supported".to_string());
                        }
                        loom_hosted::serve_ledger_grpc(
                            listener,
                            kernel,
                            workspace,
                            collection,
                            async move {
                                let _ = shutdown_rx.await;
                            },
                        )
                        .await
                    }
                    "grpc" if surface == "document" && profile.is_none() => {
                        if tls.is_some() {
                            return Err("direct TLS for document/grpc is not supported".to_string());
                        }
                        loom_hosted::serve_document_grpc(
                            listener,
                            kernel,
                            workspace,
                            collection,
                            async move {
                                let _ = shutdown_rx.await;
                            },
                        )
                        .await
                    }
                    "grpc" if surface == "fts" && profile.is_none() => {
                        if tls.is_some() {
                            return Err("direct TLS for fts/grpc is not supported".to_string());
                        }
                        loom_hosted::serve_fts_grpc(
                            listener,
                            kernel,
                            workspace,
                            collection,
                            async move {
                                let _ = shutdown_rx.await;
                            },
                        )
                        .await
                    }
                    "grpc" if surface == "time-series" && profile.is_none() => {
                        if tls.is_some() {
                            return Err(
                                "direct TLS for time-series/grpc is not supported".to_string()
                            );
                        }
                        loom_hosted::serve_time_series_grpc(
                            listener,
                            kernel,
                            workspace,
                            collection,
                            async move {
                                let _ = shutdown_rx.await;
                            },
                        )
                        .await
                    }
                    "grpc" if surface == "columnar" && profile.is_none() => {
                        if tls.is_some() {
                            return Err("direct TLS for columnar/grpc is not supported".to_string());
                        }
                        loom_hosted::serve_columnar_grpc(
                            listener,
                            kernel,
                            workspace,
                            collection,
                            async move {
                                let _ = shutdown_rx.await;
                            },
                        )
                        .await
                    }
                    _ => return Err(format!("unsupported data transport {transport}")),
                }
                .map_err(|e| format!("serve {surface}/{transport} listener {bind}: {e}"))
            },
        ),
    )
}

#[cfg(feature = "serve")]
fn serve_influx_thread(request: InfluxThreadRequest) -> Result<(), String> {
    let InfluxThreadRequest {
        listener_id,
        store,
        bind,
        surface,
        transport,
        limits,
        auth_policy,
        network_access_policy,
        kernel,
        workspace,
        shutdown_rx,
        ready_tx,
    } = request;
    let denied_audit = Some(network_access_denied_audit_sink(store));
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            let err = format!("build {surface}/{transport} runtime: {e}");
            let _ = ready_tx.send(Err(err.clone()));
            return Err(err);
        }
    };
    runtime.block_on(
        loom_hosted::with_hosted_network_access_policy_for_listener_and_audit(
            listener_id,
            network_access_policy,
            denied_audit,
            async move {
                let listener = match tokio::net::TcpListener::bind(&bind).await {
                    Ok(listener) => listener,
                    Err(e) => {
                        let err = format!("bind {surface}/{transport} listener {bind}: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let local_addr = match listener.local_addr() {
                    Ok(addr) => addr,
                    Err(e) => {
                        let err = format!("read {surface}/{transport} listener address: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let _ = ready_tx.send(Ok(local_addr.to_string()));
                loom_hosted::serve_influx_http_with_limits(
                    listener,
                    kernel,
                    workspace,
                    loom_hosted::HostedServePolicy::new(limits, auth_policy),
                    async move {
                        let _ = shutdown_rx.await;
                    },
                )
                .await
                .map_err(|e| format!("serve {surface}/{transport} listener {bind}: {e}"))
            },
        ),
    )
}

#[cfg(feature = "serve")]
fn serve_prometheus_thread(request: PrometheusThreadRequest) -> Result<(), String> {
    let PrometheusThreadRequest {
        listener_id,
        store,
        bind,
        surface,
        transport,
        limits,
        auth_policy,
        network_access_policy,
        kernel,
        workspace,
        shutdown_rx,
        ready_tx,
    } = request;
    let denied_audit = Some(network_access_denied_audit_sink(store));
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            let err = format!("build {surface}/{transport} runtime: {e}");
            let _ = ready_tx.send(Err(err.clone()));
            return Err(err);
        }
    };
    runtime.block_on(
        loom_hosted::with_hosted_network_access_policy_for_listener_and_audit(
            listener_id,
            network_access_policy,
            denied_audit,
            async move {
                let listener = match tokio::net::TcpListener::bind(&bind).await {
                    Ok(listener) => listener,
                    Err(e) => {
                        let err = format!("bind {surface}/{transport} listener {bind}: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let local_addr = match listener.local_addr() {
                    Ok(addr) => addr,
                    Err(e) => {
                        let err = format!("read {surface}/{transport} listener address: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let _ = ready_tx.send(Ok(local_addr.to_string()));
                loom_hosted::serve_prometheus_http_with_limits(
                    listener,
                    kernel,
                    workspace,
                    loom_hosted::HostedServePolicy::new(limits, auth_policy),
                    async move {
                        let _ = shutdown_rx.await;
                    },
                )
                .await
                .map_err(|e| format!("serve {surface}/{transport} listener {bind}: {e}"))
            },
        ),
    )
}

#[cfg(feature = "serve")]
fn serve_grafana_thread(request: GrafanaThreadRequest) -> Result<(), String> {
    let GrafanaThreadRequest {
        listener_id,
        store,
        bind,
        surface,
        transport,
        limits,
        auth_policy,
        network_access_policy,
        kernel,
        workspace,
        collection,
        shutdown_rx,
        ready_tx,
    } = request;
    let denied_audit = Some(network_access_denied_audit_sink(store));
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            let err = format!("build {surface}/{transport} runtime: {e}");
            let _ = ready_tx.send(Err(err.clone()));
            return Err(err);
        }
    };
    runtime.block_on(
        loom_hosted::with_hosted_network_access_policy_for_listener_and_audit(
            listener_id,
            network_access_policy,
            denied_audit,
            async move {
                let listener = match tokio::net::TcpListener::bind(&bind).await {
                    Ok(listener) => listener,
                    Err(e) => {
                        let err = format!("bind {surface}/{transport} listener {bind}: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let local_addr = match listener.local_addr() {
                    Ok(addr) => addr,
                    Err(e) => {
                        let err = format!("read {surface}/{transport} listener address: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let _ = ready_tx.send(Ok(local_addr.to_string()));
                loom_hosted::serve_grafana_http_with_limits(
                    listener,
                    kernel,
                    loom_hosted::HostedDataTarget::new("grafana", workspace, collection),
                    loom_hosted::HostedServePolicy::new(limits, auth_policy),
                    async move {
                        let _ = shutdown_rx.await;
                    },
                )
                .await
                .map_err(|e| format!("serve {surface}/{transport} listener {bind}: {e}"))
            },
        ),
    )
}

#[cfg(feature = "serve")]
fn serve_otlp_thread(request: OtlpThreadRequest) -> Result<(), String> {
    let OtlpThreadRequest {
        listener_id,
        store,
        bind,
        surface,
        transport,
        limits,
        auth_policy,
        network_access_policy,
        kernel,
        workspace,
        shutdown_rx,
        ready_tx,
    } = request;
    let denied_audit = Some(network_access_denied_audit_sink(store));
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            let err = format!("build {surface}/{transport} runtime: {e}");
            let _ = ready_tx.send(Err(err.clone()));
            return Err(err);
        }
    };
    runtime.block_on(
        loom_hosted::with_hosted_network_access_policy_for_listener_and_audit(
            listener_id,
            network_access_policy,
            denied_audit,
            async move {
                let listener = match tokio::net::TcpListener::bind(&bind).await {
                    Ok(listener) => listener,
                    Err(e) => {
                        let err = format!("bind {surface}/{transport} listener {bind}: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let local_addr = match listener.local_addr() {
                    Ok(addr) => addr,
                    Err(e) => {
                        let err = format!("read {surface}/{transport} listener address: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let _ = ready_tx.send(Ok(local_addr.to_string()));
                loom_hosted::serve_otlp_http_with_limits(
                    listener,
                    kernel,
                    workspace,
                    loom_hosted::HostedServePolicy::new(limits, auth_policy),
                    async move {
                        let _ = shutdown_rx.await;
                    },
                )
                .await
                .map_err(|e| format!("serve {surface}/{transport} listener {bind}: {e}"))
            },
        ),
    )
}

#[cfg(feature = "serve")]
struct FilesThreadRequest {
    listener_id: String,
    surface: String,
    store: String,
    bind: String,
    transport: String,
    limits: loom_hosted::HostedHttpLimits,
    tls: Option<loom_hosted::HostedTlsConfig>,
    auth_policy: loom_hosted::HostedAuthPolicy,
    network_access_policy: Option<loom_store::NetworkAccessPolicyRecord>,
    kernel: loom_hosted::HostedKernel,
    workspace: WorkspaceId,
    web_listener: Option<loom_substrate::web::WebListener>,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ready_tx: std::sync::mpsc::SyncSender<Result<String, String>>,
}

#[cfg(feature = "serve")]
struct VcsThreadRequest {
    listener_id: String,
    store: String,
    bind: String,
    transport: String,
    limits: loom_hosted::HostedHttpLimits,
    tls: Option<loom_hosted::HostedTlsConfig>,
    auth_policy: loom_hosted::HostedAuthPolicy,
    network_access_policy: Option<loom_store::NetworkAccessPolicyRecord>,
    kernel: loom_hosted::HostedKernel,
    workspace: WorkspaceId,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ready_tx: std::sync::mpsc::SyncSender<Result<String, String>>,
}

#[cfg(feature = "serve")]
fn serve_files_thread(request: FilesThreadRequest) -> Result<(), String> {
    let FilesThreadRequest {
        listener_id,
        surface,
        store,
        bind,
        transport,
        limits,
        tls,
        auth_policy,
        network_access_policy,
        kernel,
        workspace,
        web_listener,
        shutdown_rx,
        ready_tx,
    } = request;
    let denied_audit = Some(network_access_denied_audit_sink(store));
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            let err = format!("build {surface}/{transport} runtime: {e}");
            let _ = ready_tx.send(Err(err.clone()));
            return Err(err);
        }
    };
    runtime.block_on(
        loom_hosted::with_hosted_network_access_policy_for_listener_and_audit(
            listener_id,
            network_access_policy,
            denied_audit,
            async move {
                let listener = match tokio::net::TcpListener::bind(&bind).await {
                    Ok(listener) => listener,
                    Err(e) => {
                        let err = format!("bind {surface}/{transport} listener {bind}: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let local_addr = match listener.local_addr() {
                    Ok(addr) => addr,
                    Err(e) => {
                        let err = format!("read {surface}/{transport} listener address: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let _ = ready_tx.send(Ok(local_addr.to_string()));
                match (surface.as_str(), transport.as_str()) {
                    ("files", "rest") => match tls {
                        Some(tls) => {
                            loom_hosted::serve_files_rest_tls_with_limits(
                                listener,
                                tls,
                                kernel,
                                workspace,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                        None => {
                            loom_hosted::serve_files_rest_with_limits(
                                listener,
                                kernel,
                                workspace,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                    },
                    ("files", "json_rpc") => match tls {
                        Some(tls) => {
                            loom_hosted::serve_files_jsonrpc_tls_with_limits(
                                listener,
                                tls,
                                kernel,
                                workspace,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                        None => {
                            loom_hosted::serve_files_jsonrpc_with_limits(
                                listener,
                                kernel,
                                workspace,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                    },
                    ("files", "grpc") => {
                        if tls.is_some() {
                            return Err("direct TLS for files/grpc is not supported".to_string());
                        }
                        loom_hosted::serve_files_grpc(listener, kernel, workspace, async move {
                            let _ = shutdown_rx.await;
                        })
                        .await
                    }
                    ("web", "rest") => match tls {
                        Some(tls) => match web_listener {
                            Some(web_listener) => {
                                loom_hosted::serve_web_rest_for_listener_tls_with_limits(
                                    listener,
                                    tls,
                                    kernel,
                                    web_listener,
                                    limits,
                                    auth_policy,
                                    async move {
                                        let _ = shutdown_rx.await;
                                    },
                                )
                                .await
                            }
                            None => {
                                loom_hosted::serve_web_rest_tls_with_limits(
                                    listener,
                                    tls,
                                    kernel,
                                    workspace,
                                    limits,
                                    auth_policy,
                                    async move {
                                        let _ = shutdown_rx.await;
                                    },
                                )
                                .await
                            }
                        },
                        None => match web_listener {
                            Some(web_listener) => {
                                loom_hosted::serve_web_rest_for_listener_with_limits(
                                    listener,
                                    kernel,
                                    web_listener,
                                    limits,
                                    auth_policy,
                                    async move {
                                        let _ = shutdown_rx.await;
                                    },
                                )
                                .await
                            }
                            None => {
                                loom_hosted::serve_web_rest_with_limits(
                                    listener,
                                    kernel,
                                    workspace,
                                    limits,
                                    auth_policy,
                                    async move {
                                        let _ = shutdown_rx.await;
                                    },
                                )
                                .await
                            }
                        },
                    },
                    _ => return Err(format!("unsupported {surface} transport {transport}")),
                }
                .map_err(|e| format!("serve {surface}/{transport} listener {bind}: {e}"))
            },
        ),
    )
}

#[cfg(feature = "serve")]
fn serve_vcs_thread(request: VcsThreadRequest) -> Result<(), String> {
    let VcsThreadRequest {
        listener_id,
        store,
        bind,
        transport,
        limits,
        tls,
        auth_policy,
        network_access_policy,
        kernel,
        workspace,
        shutdown_rx,
        ready_tx,
    } = request;
    let denied_audit = Some(network_access_denied_audit_sink(store));
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            let err = format!("build vcs/{transport} runtime: {e}");
            let _ = ready_tx.send(Err(err.clone()));
            return Err(err);
        }
    };
    runtime.block_on(
        loom_hosted::with_hosted_network_access_policy_for_listener_and_audit(
            listener_id,
            network_access_policy,
            denied_audit,
            async move {
                let listener = match tokio::net::TcpListener::bind(&bind).await {
                    Ok(listener) => listener,
                    Err(e) => {
                        let err = format!("bind vcs/{transport} listener {bind}: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let local_addr = match listener.local_addr() {
                    Ok(addr) => addr,
                    Err(e) => {
                        let err = format!("read vcs/{transport} listener address: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let _ = ready_tx.send(Ok(local_addr.to_string()));
                match transport.as_str() {
                    "rest" => match tls {
                        Some(tls) => {
                            loom_hosted::serve_vcs_rest_tls_with_limits(
                                listener,
                                tls,
                                kernel,
                                workspace,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                        None => {
                            loom_hosted::serve_vcs_rest_with_limits(
                                listener,
                                kernel,
                                workspace,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                    },
                    "json_rpc" => match tls {
                        Some(tls) => {
                            loom_hosted::serve_vcs_jsonrpc_tls_with_limits(
                                listener,
                                tls,
                                kernel,
                                workspace,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                        None => {
                            loom_hosted::serve_vcs_jsonrpc_with_limits(
                                listener,
                                kernel,
                                workspace,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                    },
                    "grpc" => {
                        if tls.is_some() {
                            return Err("direct TLS for vcs/grpc is not supported".to_string());
                        }
                        loom_hosted::serve_vcs_grpc(listener, kernel, workspace, async move {
                            let _ = shutdown_rx.await;
                        })
                        .await
                    }
                    _ => return Err(format!("unsupported vcs transport {transport}")),
                }
                .map_err(|e| format!("serve vcs/{transport} listener {bind}: {e}"))
            },
        ),
    )
}

#[cfg(feature = "serve")]
struct CasThreadRequest {
    listener_id: String,
    store: String,
    bind: String,
    transport: String,
    limits: loom_hosted::HostedHttpLimits,
    tls: Option<loom_hosted::HostedTlsConfig>,
    auth_policy: loom_hosted::HostedAuthPolicy,
    network_access_policy: Option<loom_store::NetworkAccessPolicyRecord>,
    kernel: loom_hosted::HostedKernel,
    workspace: WorkspaceId,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ready_tx: std::sync::mpsc::SyncSender<Result<String, String>>,
}

#[cfg(feature = "serve")]
fn serve_cas_thread(request: CasThreadRequest) -> Result<(), String> {
    let CasThreadRequest {
        listener_id,
        store,
        bind,
        transport,
        limits,
        tls,
        auth_policy,
        network_access_policy,
        kernel,
        workspace,
        shutdown_rx,
        ready_tx,
    } = request;
    let denied_audit = Some(network_access_denied_audit_sink(store));
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            let err = format!("build cas/{transport} runtime: {e}");
            let _ = ready_tx.send(Err(err.clone()));
            return Err(err);
        }
    };
    runtime.block_on(
        loom_hosted::with_hosted_network_access_policy_for_listener_and_audit(
            listener_id,
            network_access_policy,
            denied_audit,
            async move {
                let listener = match tokio::net::TcpListener::bind(&bind).await {
                    Ok(listener) => listener,
                    Err(e) => {
                        let err = format!("bind cas/{transport} listener {bind}: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let local_addr = match listener.local_addr() {
                    Ok(addr) => addr,
                    Err(e) => {
                        let err = format!("read cas/{transport} listener address: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let _ = ready_tx.send(Ok(local_addr.to_string()));
                match transport.as_str() {
                    "rest" => match tls {
                        Some(tls) => {
                            loom_hosted::serve_cas_rest_tls_with_limits(
                                listener,
                                tls,
                                kernel,
                                workspace,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                        None => {
                            loom_hosted::serve_cas_rest_with_limits(
                                listener,
                                kernel,
                                workspace,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                    },
                    "json_rpc" => match tls {
                        Some(tls) => {
                            loom_hosted::serve_cas_jsonrpc_tls_with_limits(
                                listener,
                                tls,
                                kernel,
                                workspace,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                        None => {
                            loom_hosted::serve_cas_jsonrpc_with_limits(
                                listener,
                                kernel,
                                workspace,
                                limits,
                                auth_policy,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await
                        }
                    },
                    "grpc" => {
                        if tls.is_some() {
                            return Err("direct TLS for cas/grpc is not supported".to_string());
                        }
                        loom_hosted::serve_cas_grpc(listener, kernel, workspace, async move {
                            let _ = shutdown_rx.await;
                        })
                        .await
                    }
                    _ => return Err(format!("unsupported cas transport {transport}")),
                }
                .map_err(|e| format!("serve cas/{transport} listener {bind}: {e}"))
            },
        ),
    )
}

#[cfg(feature = "serve")]
fn serve_oci_thread(request: OciThreadRequest) -> Result<(), String> {
    let OciThreadRequest {
        listener_id,
        store,
        bind,
        transport,
        limits,
        tls,
        auth_policy,
        network_access_policy,
        kernel,
        workspace,
        shutdown_rx,
        ready_tx,
    } = request;
    let denied_audit = Some(network_access_denied_audit_sink(store));
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            let err = format!("build oci/{transport} runtime: {e}");
            let _ = ready_tx.send(Err(err.clone()));
            return Err(err);
        }
    };
    runtime.block_on(
        loom_hosted::with_hosted_network_access_policy_for_listener_and_audit(
            listener_id,
            network_access_policy,
            denied_audit,
            async move {
                let listener = match tokio::net::TcpListener::bind(&bind).await {
                    Ok(listener) => listener,
                    Err(e) => {
                        let err = format!("bind oci/{transport} listener {bind}: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let local_addr = match listener.local_addr() {
                    Ok(addr) => addr,
                    Err(e) => {
                        let err = format!("read oci/{transport} listener address: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let _ = ready_tx.send(Ok(local_addr.to_string()));
                match transport.as_str() {
                    "rest" => {
                        if tls.is_some() {
                            return Err("direct TLS for oci/rest is not supported".to_string());
                        }
                        loom_hosted::serve_oci_rest_with_limits(
                            listener,
                            kernel,
                            workspace,
                            limits,
                            auth_policy,
                            async move {
                                let _ = shutdown_rx.await;
                            },
                        )
                        .await
                    }
                    _ => return Err(format!("unsupported oci transport {transport}")),
                }
                .map_err(|e| format!("serve oci/{transport} listener {bind}: {e}"))
            },
        ),
    )
}

#[cfg(feature = "serve")]
fn serve_s3_thread(request: S3ThreadRequest) -> Result<(), String> {
    let S3ThreadRequest {
        listener_id,
        store,
        bind,
        transport,
        limits,
        tls,
        auth_policy,
        network_access_policy,
        kernel,
        workspace,
        bucket,
        shutdown_rx,
        ready_tx,
    } = request;
    let denied_audit = Some(network_access_denied_audit_sink(store));
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            let err = format!("build s3/{transport} runtime: {e}");
            let _ = ready_tx.send(Err(err.clone()));
            return Err(err);
        }
    };
    runtime.block_on(
        loom_hosted::with_hosted_network_access_policy_for_listener_and_audit(
            listener_id,
            network_access_policy,
            denied_audit,
            async move {
                let listener = match tokio::net::TcpListener::bind(&bind).await {
                    Ok(listener) => listener,
                    Err(e) => {
                        let err = format!("bind s3/{transport} listener {bind}: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let local_addr = match listener.local_addr() {
                    Ok(addr) => addr,
                    Err(e) => {
                        let err = format!("read s3/{transport} listener address: {e}");
                        let _ = ready_tx.send(Err(err.clone()));
                        return Err(err);
                    }
                };
                let _ = ready_tx.send(Ok(local_addr.to_string()));
                match transport.as_str() {
                    "rest" => {
                        if tls.is_some() {
                            return Err("direct TLS for s3/rest is not supported".to_string());
                        }
                        loom_hosted::serve_s3_rest_with_limits(
                            listener,
                            kernel,
                            workspace,
                            bucket,
                            limits,
                            auth_policy,
                            async move {
                                let _ = shutdown_rx.await;
                            },
                        )
                        .await
                    }
                    _ => return Err(format!("unsupported s3 transport {transport}")),
                }
                .map_err(|e| format!("serve s3/{transport} listener {bind}: {e}"))
            },
        ),
    )
}

#[cfg(feature = "serve")]
fn resolve_served_workspace(store: &str, selector: &str) -> Result<WorkspaceId, String> {
    if let Ok(id) = WorkspaceId::parse(selector) {
        return Ok(id);
    }
    let loom =
        loom_store::open_loom_daemon_authorized_unlocked(store, None).map_err(|e| e.to_string())?;
    loom.registry()
        .open(&WsSelector::Name(selector.to_string()))
        .map_err(|e| e.to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DaemonTransportSelection {
    Native,
    TcpLoopback,
}

impl DaemonTransportSelection {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "native" => Ok(Self::Native),
            "tcp" | "tcp-loopback" => Ok(Self::TcpLoopback),
            _ => Err(format!(
                "unsupported daemon transport {value:?}; expected `native` or `tcp`"
            )),
        }
    }

    fn cli_value(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::TcpLoopback => "tcp",
        }
    }
}

pub(crate) fn daemon_start_with_transport(store: &str, transport: &str) -> Result<(), String> {
    daemon_start_selected(store, DaemonTransportSelection::parse(transport)?)
}

fn daemon_start_selected(store: &str, transport: DaemonTransportSelection) -> Result<(), String> {
    let paths = daemon::paths(store).map_err(|e| e.to_string())?;
    let startup_guard =
        FileStore::open_daemon_authorized(&paths.store).map_err(|e| e.to_string())?;
    if let Ok(status) = daemon::status_response(&paths) {
        let target = format!("pid={};state=running", status.pid);
        append_daemon_audit(&paths.store, "daemon.start", Some(&target))?;
        println!("running\t{}\t{}", status.pid, status.store);
        return Ok(());
    }
    let startup_lock = match daemon_try_runtime_lock(&paths, true) {
        Ok(Some(lock)) => lock,
        Ok(None) => {
            for _ in 0..50 {
                if let Ok(status) = daemon::status_response(&paths) {
                    println!("running\t{}\t{}", status.pid, status.store);
                    return Ok(());
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            return Err(format!(
                "daemon startup is already in progress for {}",
                paths.store
            ));
        }
        Err(e) => return Err(format!("create daemon lock: {e}")),
    };
    daemon_write_runtime_lock(&startup_lock, &paths)
        .map_err(|e| format!("write daemon lock: {e}"))?;
    let _ = std::fs::remove_file(&paths.addr_file);
    let _ = std::fs::remove_file(&paths.pid_file);
    let _ = std::fs::remove_file(&paths.sock_file);
    drop(startup_guard);
    let exe = std::env::current_exe().map_err(|e| format!("resolve current executable: {e}"))?;
    let stdout_log = daemon_start_log_file(&paths, "stdout");
    let stderr_log = daemon_start_log_file(&paths, "stderr");
    let stdout = daemon_create_start_log(&stdout_log)?;
    let stderr = daemon_create_start_log(&stderr_log)?;
    let mut command = std::process::Command::new(exe);
    command
        .arg("daemon")
        .arg("run")
        .arg(&paths.store)
        .arg("--addr-file")
        .arg(&paths.addr_file)
        .arg("--pid-file")
        .arg(&paths.pid_file)
        .arg("--lock-file")
        .arg(&paths.lock_file)
        .arg("--transport")
        .arg(transport.cli_value())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(stdout))
        .stderr(std::process::Stdio::from(stderr));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            daemon_cleanup_files(&paths);
            return Err(format!("start daemon process: {e}"));
        }
    };
    drop(startup_lock);
    for _ in 0..50 {
        if let Ok(status) = daemon::status_response(&paths) {
            let target = format!("pid={};state=started", status.pid);
            append_daemon_audit(&paths.store, "daemon.start", Some(&target))?;
            println!("started\t{}\t{}", status.pid, status.store);
            return Ok(());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                let logs = daemon_start_log_summary(&stdout_log, &stderr_log);
                cleanup_unlocked_runtime_files(&paths);
                return Err(format!(
                    "daemon exited during startup for {} with {status}\n{logs}",
                    paths.store
                ));
            }
            Ok(None) => {}
            Err(e) => {
                cleanup_unlocked_runtime_files(&paths);
                return Err(format!("check daemon startup process: {e}"));
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let logs = daemon_start_log_summary(&stdout_log, &stderr_log);
    let _ = child.kill();
    let _ = child.wait();
    cleanup_unlocked_runtime_files(&paths);
    Err(format!("daemon did not start for {}\n{logs}", paths.store))
}

fn daemon_start_log_file(paths: &daemon::DaemonPaths, stream: &str) -> std::path::PathBuf {
    paths.lock_file.with_extension(format!("{stream}.log"))
}

fn daemon_create_start_log(path: &std::path::Path) -> Result<std::fs::File, String> {
    std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|e| format!("open daemon startup log {}: {e}", path.display()))
}

fn daemon_start_log_summary(stdout_log: &std::path::Path, stderr_log: &std::path::Path) -> String {
    let mut out = String::new();
    for (label, path) in [("stdout", stdout_log), ("stderr", stderr_log)] {
        let content = std::fs::read_to_string(path).unwrap_or_default();
        let content = content.trim();
        if content.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str("daemon ");
        out.push_str(label);
        out.push_str(":\n");
        out.push_str(&daemon_log_tail(content, 4096));
    }
    if out.is_empty() {
        format!(
            "daemon startup logs were empty: stdout={}, stderr={}",
            stdout_log.display(),
            stderr_log.display()
        )
    } else {
        out
    }
}

fn daemon_log_tail(value: &str, max_chars: usize) -> String {
    let len = value.chars().count();
    if len <= max_chars {
        return value.to_string();
    }
    value.chars().skip(len.saturating_sub(max_chars)).collect()
}

pub(crate) fn daemon_stop(
    store: &str,
    options: daemon::StopOptions,
    keys: &KeyOpts,
) -> Result<(), String> {
    let paths = daemon::paths(store).map_err(|e| e.to_string())?;
    if !daemon::is_running(&paths) {
        match daemon_existing_runtime_lock_state(&paths) {
            Ok(RuntimeLockState::Busy) => {
                return Err(format!("daemon startup is in progress for {}", paths.store));
            }
            Ok(RuntimeLockState::Acquired(lock)) => {
                daemon_cleanup_files(&paths);
                drop(lock);
            }
            Ok(RuntimeLockState::Missing) => daemon_cleanup_files(&paths),
            Err(_) => {}
        }
        return Err(format!("daemon is not running for {}", paths.store));
    }
    let auth = daemon_auth_from_keys(keys)?;
    let response =
        daemon::stop_auth_with_options(&paths, options, &auth).map_err(|e| e.to_string())?;
    print!("{response}");
    Ok(())
}

pub(crate) fn daemon_status(store: &str, json: bool) -> Result<(), String> {
    let paths = daemon::paths(store).map_err(|e| e.to_string())?;
    if json {
        println!("{}", daemon::status_json(&paths));
        return Ok(());
    }
    match daemon::status_response(&paths) {
        Ok(status) => {
            println!("{}", daemon_status_line(&status));
            print_daemon_pin_status(&status);
            Ok(())
        }
        Err(_) => {
            match daemon_existing_runtime_lock_state(&paths) {
                Ok(RuntimeLockState::Busy) => println!("starting\t-\t{}", paths.store),
                Ok(RuntimeLockState::Acquired(lock)) => {
                    daemon_cleanup_files(&paths);
                    drop(lock);
                    println!("stopped\t-\t{}", paths.store);
                }
                Ok(RuntimeLockState::Missing) => {
                    daemon_cleanup_files(&paths);
                    println!("stopped\t-\t{}", paths.store);
                }
                Err(_) => println!("stopped\t-\t{}", paths.store),
            }
            Ok(())
        }
    }
}

fn daemon_status_line(status: &daemon::DaemonStatus) -> String {
    format!(
        "running\tprotocol={}\ttransport={}\tsecurity={}\tprofile={}\t{}\t{}",
        daemon::PROTOCOL,
        status.transport.wire_name(),
        status.transport.security().wire_name(),
        daemon_transport_profile(status.transport),
        status.pid,
        status.store
    )
}

enum RuntimeLockState {
    Missing,
    Busy,
    Acquired(std::fs::File),
}

fn daemon_try_runtime_lock(
    paths: &daemon::DaemonPaths,
    create: bool,
) -> std::io::Result<Option<std::fs::File>> {
    let result = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(create)
        .truncate(false)
        .open(&paths.lock_file);
    let file = match result {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !create => return Ok(None),
        Err(e) => return Err(e),
    };
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(e),
    }
}

fn daemon_existing_runtime_lock_state(
    paths: &daemon::DaemonPaths,
) -> std::io::Result<RuntimeLockState> {
    match daemon_try_runtime_lock(paths, false)? {
        Some(file) => Ok(RuntimeLockState::Acquired(file)),
        None if paths.lock_file.exists() => Ok(RuntimeLockState::Busy),
        None => Ok(RuntimeLockState::Missing),
    }
}

fn daemon_write_runtime_lock(
    file: &std::fs::File,
    paths: &daemon::DaemonPaths,
) -> std::io::Result<()> {
    file.set_len(0)?;
    let mut file = file;
    writeln!(file, "store={}", paths.store)?;
    writeln!(file, "identity={}", paths.store_id)?;
    writeln!(file, "pid={}", std::process::id())?;
    daemon::align_runtime_artifact_owner(&paths.lock_file, "lock", paths)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(())
}

pub(crate) fn daemon_cleanup_files(paths: &daemon::DaemonPaths) {
    let _ = std::fs::remove_file(&paths.addr_file);
    let _ = std::fs::remove_file(&paths.pid_file);
    let _ = std::fs::remove_file(&paths.lock_file);
    let _ = std::fs::remove_file(&paths.sock_file);
}

fn cleanup_unlocked_runtime_files(paths: &daemon::DaemonPaths) {
    match daemon_existing_runtime_lock_state(paths) {
        Ok(RuntimeLockState::Acquired(lock)) => {
            daemon_cleanup_files(paths);
            drop(lock);
        }
        Ok(RuntimeLockState::Missing) => daemon_cleanup_files(paths),
        Ok(RuntimeLockState::Busy) | Err(_) => {}
    }
}

pub(crate) struct DaemonRuntime {
    store: String,
    store_id: String,
    transport: daemon::DaemonTransport,
    coordinator: LockCoordinator,
    kv_loom: Option<Loom<FileStore>>,
    kv_unavailable: Option<loom_core::error::LoomError>,
    sessions: std::collections::BTreeSet<String>,
    pins: std::collections::BTreeMap<String, PinLease>,
    authority_replication_next: std::collections::BTreeMap<String, u64>,
    maintenance_next_ms: u64,
    #[cfg(feature = "serve")]
    hosted_listeners: std::collections::BTreeMap<String, HostedListenerRuntime>,
    #[cfg(feature = "serve")]
    drive_policy_next_ms: u64,
    #[cfg(feature = "serve")]
    reference_reconcile_next_ms: u64,
}

#[derive(Debug, Clone, Copy)]
struct PinLease {
    deadline_ms: Option<u64>,
}

#[derive(Debug, Default)]
struct DaemonRequestAuth {
    principal: Option<WorkspaceId>,
    passphrase: Option<String>,
    session: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct DaemonStopRequest {
    hard: bool,
    wait_ms: u64,
}

impl Default for DaemonStopRequest {
    fn default() -> Self {
        Self {
            hard: false,
            wait_ms: default_daemon_stop_wait_ms(),
        }
    }
}

fn default_daemon_stop_wait_ms() -> u64 {
    #[cfg(feature = "serve")]
    {
        DEFAULT_DAEMON_STOP_WAIT_MS
    }
    #[cfg(not(feature = "serve"))]
    {
        30_000
    }
}

#[cfg(feature = "serve")]
#[derive(Debug, Clone, Copy)]
struct DaemonWorkerActor {
    principal: Option<WorkspaceId>,
    preauthenticate: bool,
    session_id: &'static str,
}

#[cfg(feature = "serve")]
fn apply_drive_policy_workers_once(store: &str, now: u64) -> Result<usize, String> {
    let targets = drive_policy_targets(store)?;
    if targets.is_empty() {
        return Ok(0);
    }
    let actor = ensure_daemon_worker_actor(store)?;
    let mut loom = open_daemon_worker_loom(store, actor)?;
    let mut applied = 0usize;
    for (workspace, workspace_id) in targets {
        match loom_hosted::drive::apply_share_expiry(&mut loom, workspace, &workspace_id, now) {
            Ok(summary) => {
                if summary.operation.is_some() {
                    applied += 1;
                }
            }
            Err(err) => {
                append_daemon_audit_actor(
                    store,
                    actor.principal,
                    "drive.share_expiry_worker.error",
                    Some(&format!(
                        "workspace={workspace};drive={workspace_id};reason={}",
                        err.message
                    )),
                )?;
            }
        }
        match loom_hosted::drive::apply_retention(&mut loom, workspace, &workspace_id, now) {
            Ok(summary) => {
                if summary.operation.is_some() {
                    applied += 1;
                }
            }
            Err(err) => {
                append_daemon_audit_actor(
                    store,
                    actor.principal,
                    "drive.retention_worker.error",
                    Some(&format!(
                        "workspace={workspace};drive={workspace_id};reason={}",
                        err.message
                    )),
                )?;
            }
        }
    }
    Ok(applied)
}

#[cfg(feature = "serve")]
fn drive_policy_targets(
    store: &str,
) -> Result<std::collections::BTreeSet<(WorkspaceId, String)>, String> {
    let mut targets = std::collections::BTreeSet::new();
    let fs = FileStore::open_read(store).map_err(|e| e.to_string())?;
    for target in load_drive_policy_registry(&fs)?.enabled_targets() {
        targets.insert((target.workspace, target.workspace_id.clone()));
    }
    for record in desired_hosted_listener_records(store)? {
        if record.surface != "drive" {
            continue;
        }
        let [workspace] = record.selectors.as_slice() else {
            continue;
        };
        let workspace = resolve_served_workspace(store, workspace)?;
        targets.insert((workspace, workspace.to_string()));
    }
    Ok(targets)
}

#[cfg(feature = "serve")]
fn open_daemon_worker_loom(
    store: &str,
    actor: DaemonWorkerActor,
) -> Result<Loom<FileStore>, String> {
    let loom =
        loom_store::open_loom_daemon_authorized_unlocked(store, None).map_err(|e| e.to_string())?;
    if actor.preauthenticate {
        loom_store::attach_local_auth(
            loom,
            &LocalOpenAuth {
                unlock_key: None,
                principal: None,
                passphrase: None,
                app_credential: None,
                verified_external: None,
                preauthenticated_principal: actor.principal,
                session_id: Some(actor.session_id.to_string()),
            },
        )
        .map_err(|e| e.to_string())
    } else {
        Ok(loom)
    }
}

#[cfg(feature = "serve")]
fn ensure_daemon_worker_actor(store: &str) -> Result<DaemonWorkerActor, String> {
    let fs = FileStore::open_daemon_authorized(store).map_err(|e| e.to_string())?;
    let Some(mut identity) = fs.identity_store().map_err(|e| e.to_string())? else {
        return Ok(DaemonWorkerActor {
            principal: None,
            preauthenticate: false,
            session_id: "daemon-drive-policy-worker",
        });
    };
    if !identity.authenticated_mode() {
        return Ok(DaemonWorkerActor {
            principal: identity.root_principal(),
            preauthenticate: false,
            session_id: "daemon-drive-policy-worker",
        });
    }
    let principal = DAEMON_SERVICE_PRINCIPAL_ID;
    let mut identity_changed = false;
    match identity.principal(principal) {
        Ok(existing) if existing.kind != PrincipalKind::Service => {
            return Err(
                "daemon service principal id belongs to a non-service principal".to_string(),
            );
        }
        Ok(_) => {}
        Err(err) if err.code == Code::NotFound => {
            identity
                .add_principal(principal, "loom-daemon", PrincipalKind::Service)
                .map_err(|e| e.to_string())?;
            identity_changed = true;
        }
        Err(err) => return Err(err.to_string()),
    }
    for role in [loom_core::ROLE_SERVICE_ID, loom_core::ROLE_ADMIN_ID] {
        if !identity
            .principal(principal)
            .map_err(|e| e.to_string())?
            .roles
            .contains(&role)
        {
            identity
                .assign_role(principal, role)
                .map_err(|e| e.to_string())?;
            identity_changed = true;
        }
    }
    if identity_changed {
        fs.save_identity_store_audited(
            &identity,
            Some(principal),
            "daemon.service_principal.ensure",
            Some(&format!("principal={principal}")),
        )
        .map_err(|e| e.to_string())?;
    }
    ensure_daemon_worker_acl(&fs, principal)?;
    Ok(DaemonWorkerActor {
        principal: Some(principal),
        preauthenticate: true,
        session_id: "daemon-drive-policy-worker",
    })
}

#[cfg(feature = "serve")]
fn ensure_daemon_worker_acl(fs: &FileStore, principal: WorkspaceId) -> Result<(), String> {
    let mut acl = fs
        .acl_store()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "daemon service principal requires acl state".to_string())?;
    let grant = AclGrant {
        subject: AclSubject::Principal(principal),
        workspace: None,
        domain: Some(FacetKind::Vcs.into()),
        ref_glob: None,
        scopes: vec![AclScope::All],
        rights: [AclRight::Admin, AclRight::Read, AclRight::Write]
            .into_iter()
            .collect(),
        effect: AclEffect::Allow,
        predicate: None,
    };
    if acl.grants().contains(&grant) {
        return Ok(());
    }
    acl.grant(grant).map_err(|e| e.to_string())?;
    fs.save_acl_store_audited(
        &acl,
        Some(principal),
        "daemon.service_principal.acl",
        Some(&format!("principal={principal}")),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(feature = "serve")]
#[derive(Debug, Clone, Copy, Default)]
struct ReferenceWorkerRun {
    pending: u64,
    resolved: u64,
    failed: u64,
    processed: u64,
    next_attempt_ms: Option<u64>,
    unsupported_targets: usize,
}

#[cfg(feature = "serve")]
fn reconcile_references_once(
    store: &str,
    now: u64,
    max: usize,
) -> Result<ReferenceWorkerRun, String> {
    let workspaces = reference_workspace_ids(store)?;
    if workspaces.is_empty() {
        return Ok(ReferenceWorkerRun::default());
    }
    let actor = ensure_reference_resolver_actor(store, &workspaces)?;
    let loom = open_daemon_worker_loom(store, actor)?;
    let mut targets = Vec::new();
    for workspace in &workspaces {
        targets.extend(
            loom_reference::targets(&loom, *workspace)
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|target| (*workspace, target)),
        );
    }
    drop(loom);
    if let Some(principal) = actor.principal {
        let fs = FileStore::open_daemon_authorized(store).map_err(|error| error.to_string())?;
        for (workspace, target) in &targets {
            match target.source_profile.as_str() {
                "tickets" => ensure_reference_resolver_ticket_acl(
                    &fs,
                    principal,
                    *workspace,
                    &target.source_scope,
                )?,
                "chat" => ensure_reference_resolver_chat_acl(
                    &fs,
                    principal,
                    *workspace,
                    &target.source_scope,
                )?,
                _ => {}
            }
        }
    }
    let mut loom = open_daemon_worker_loom(store, actor)?;
    let mut run = ReferenceWorkerRun::default();
    for (workspace, target) in targets {
        let remaining = max.saturating_sub(run.processed as usize);
        if remaining == 0 {
            break;
        }
        let mut index = loom_reference::load_index(&loom, workspace)
            .map_err(|error| error.to_string())?
            .unwrap_or_default();
        let records = match target.source_profile.as_str() {
            "tickets" => {
                let due = loom_reference::due(&loom, workspace, &target, now, remaining)
                    .map_err(|error| error.to_string())?;
                if due.is_empty() {
                    continue;
                }
                let records = loom_tickets::reconcile_reference_candidates(
                    &mut loom,
                    workspace,
                    &target.source_scope,
                    now,
                    due.len(),
                    &actor.principal.unwrap_or(workspace).to_string(),
                )
                .map_err(|error| error.to_string())?;
                run.processed = run.processed.saturating_add(due.len() as u64);
                records
            }
            "chat" => {
                let records = loom_reference::reconcile(
                    &mut loom,
                    workspace,
                    &target,
                    now,
                    remaining,
                    &actor.principal.unwrap_or(workspace).to_string(),
                    |loom, candidate| {
                        resolve_chat_reference_candidate(
                            loom,
                            workspace,
                            &target.source_scope,
                            candidate,
                        )
                    },
                )
                .map_err(|error| error.to_string())?;
                run.processed = run.processed.saturating_add(records.len() as u64);
                records
            }
            _ => {
                run.unsupported_targets = run.unsupported_targets.saturating_add(1);
                continue;
            }
        };
        if !records.is_empty() {
            loom_reference::apply_resolved_edges(&mut index, &records)
                .map_err(|error| error.to_string())?;
            loom_reference::save_index(&mut loom, workspace, &index)
                .map_err(|error| error.to_string())?;
        }
    }
    for workspace in workspaces {
        let summary =
            loom_reference::status(&loom, workspace).map_err(|error| error.to_string())?;
        run.pending = run.pending.saturating_add(summary.pending);
        run.resolved = run.resolved.saturating_add(summary.resolved);
        run.failed = run.failed.saturating_add(summary.failed);
        for target in
            loom_reference::targets(&loom, workspace).map_err(|error| error.to_string())?
        {
            run.next_attempt_ms = Some(
                run.next_attempt_ms
                    .map_or(target.next_attempt_ms, |current| {
                        current.min(target.next_attempt_ms)
                    }),
            );
        }
    }
    if run.processed > 0 {
        save_loom(&mut loom).map_err(|error| error.to_string())?;
    }
    Ok(run)
}

#[cfg(feature = "serve")]
fn reference_workspace_ids(store: &str) -> Result<Vec<WorkspaceId>, String> {
    let loom = loom_store::open_loom_daemon_authorized_unlocked(store, None)
        .map_err(|error| error.to_string())?;
    Ok(loom
        .registry()
        .list(None)
        .into_iter()
        .map(|workspace| workspace.id)
        .collect())
}

#[cfg(feature = "serve")]
fn ensure_reference_resolver_actor(
    store: &str,
    workspaces: &[WorkspaceId],
) -> Result<DaemonWorkerActor, String> {
    let fs = FileStore::open_daemon_authorized(store).map_err(|error| error.to_string())?;
    let Some(mut identity) = fs.identity_store().map_err(|error| error.to_string())? else {
        return Ok(DaemonWorkerActor {
            principal: None,
            preauthenticate: false,
            session_id: "daemon-reference-resolver",
        });
    };
    if !identity.authenticated_mode() {
        return Ok(DaemonWorkerActor {
            principal: identity.root_principal(),
            preauthenticate: false,
            session_id: "daemon-reference-resolver",
        });
    }
    let principal = REFERENCE_RESOLVER_SERVICE_PRINCIPAL_ID;
    let mut changed = false;
    match identity.principal(principal) {
        Ok(existing) if existing.kind != PrincipalKind::Service => {
            return Err(
                "reference resolver principal id belongs to a non-service principal".to_string(),
            );
        }
        Ok(_) => {}
        Err(error) if error.code == Code::NotFound => {
            identity
                .add_principal(principal, "reference-resolver", PrincipalKind::Service)
                .map_err(|error| error.to_string())?;
            changed = true;
        }
        Err(error) => return Err(error.to_string()),
    }
    if !identity
        .principal(principal)
        .map_err(|error| error.to_string())?
        .roles
        .contains(&loom_core::ROLE_SERVICE_ID)
    {
        identity
            .assign_role(principal, loom_core::ROLE_SERVICE_ID)
            .map_err(|error| error.to_string())?;
        changed = true;
    }
    if changed {
        fs.save_identity_store_audited(
            &identity,
            Some(principal),
            "reference_resolver.principal.ensure",
            Some(&format!("principal={principal}")),
        )
        .map_err(|error| error.to_string())?;
    }
    for workspace in workspaces {
        ensure_reference_resolver_base_acl(&fs, principal, *workspace)?;
    }
    Ok(DaemonWorkerActor {
        principal: Some(principal),
        preauthenticate: true,
        session_id: "daemon-reference-resolver",
    })
}

#[cfg(feature = "serve")]
fn ensure_reference_resolver_base_acl(
    fs: &FileStore,
    principal: WorkspaceId,
    workspace: WorkspaceId,
) -> Result<(), String> {
    ensure_service_grant(
        fs,
        principal,
        workspace,
        FacetKind::Vcs,
        AclScope::Prefix {
            kind: AclScopeKind::Table,
            prefix: loom_reference::RECONCILIATION_DIR.as_bytes().to_vec(),
        },
    )?;
    ensure_service_grant(
        fs,
        principal,
        workspace,
        FacetKind::Files,
        AclScope::Prefix {
            kind: AclScopeKind::Path,
            prefix: loom_reference::INDEX_DIR.as_bytes().to_vec(),
        },
    )
}

#[cfg(feature = "serve")]
fn ensure_reference_resolver_ticket_acl(
    fs: &FileStore,
    principal: WorkspaceId,
    workspace: WorkspaceId,
    workspace_id: &str,
) -> Result<(), String> {
    let prefix =
        loom_tickets::profile_table_prefix(workspace_id).map_err(|error| error.to_string())?;
    ensure_service_grant(
        fs,
        principal,
        workspace,
        FacetKind::Vcs,
        AclScope::Prefix {
            kind: AclScopeKind::Table,
            prefix: prefix.into_bytes(),
        },
    )
}

#[cfg(feature = "serve")]
fn ensure_reference_resolver_chat_acl(
    fs: &FileStore,
    principal: WorkspaceId,
    workspace: WorkspaceId,
    source_scope: &str,
) -> Result<(), String> {
    let (workspace_id, _) = source_scope
        .rsplit_once(':')
        .ok_or_else(|| "chat reference scope is invalid".to_string())?;
    let path = String::from_utf8(
        loom_substrate::chat::chat_channel_directory_key(workspace_id)
            .map_err(|error| error.to_string())?,
    )
    .map_err(|_| "chat channel directory path is not utf-8".to_string())?;
    ensure_service_grant(
        fs,
        principal,
        workspace,
        FacetKind::Vcs,
        AclScope::Prefix {
            kind: AclScopeKind::Path,
            prefix: path.into_bytes(),
        },
    )
}

#[cfg(feature = "serve")]
fn resolve_chat_reference_candidate(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    source_scope: &str,
    candidate: &loom_substrate::refs::UnresolvedReference,
) -> loom_core::Result<Option<loom_substrate::refs::EntityRef>> {
    let (workspace_id, _) = source_scope
        .rsplit_once(':')
        .ok_or_else(|| loom_core::LoomError::corrupt("chat reference scope is invalid"))?;
    if let Some(handle) = candidate.alias_text.strip_prefix('@') {
        return loom
            .identity_store()
            .map(|identity| identity.resolve_handle(handle))
            .transpose()?
            .flatten()
            .map(|principal| {
                loom_substrate::refs::EntityRef::parse(&format!("principal:{principal}"))
            })
            .transpose();
    }
    if let Some(handle) = candidate.alias_text.strip_prefix('#') {
        let path = String::from_utf8(loom_substrate::chat::chat_channel_directory_key(
            workspace_id,
        )?)
        .map_err(|_| loom_core::LoomError::corrupt("chat channel directory path is not utf-8"))?;
        loom.authorize_file_path(workspace, &path, AclRight::Read)?;
        let directory = loom_substrate::chat::ChatChannelDirectory::decode(
            &loom.read_file_reserved(workspace, &path)?,
        )?;
        return directory
            .resolve(handle)?
            .map(|channel| {
                loom_substrate::refs::EntityRef::parse(&format!("channel:{}", channel.id))
            })
            .transpose();
    }
    if let Some(key) = candidate.alias_text.strip_prefix("!ticket:") {
        return resolve_ticket_reference_candidate(loom, workspace, workspace_id, key);
    }
    if let Some(target) = candidate.alias_text.strip_prefix('!') {
        return loom_substrate::refs::EntityRef::parse(target).map(Some);
    }
    Ok(None)
}

#[cfg(feature = "serve")]
fn resolve_ticket_reference_candidate(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    key: &str,
) -> loom_core::Result<Option<loom_substrate::refs::EntityRef>> {
    let Some(profile) = loom_tickets::TicketProfileReader::open(loom, workspace, workspace_id)?
    else {
        return Ok(None);
    };
    profile
        .resolve_ticket_key(key)?
        .map(|resolution| {
            loom_substrate::refs::EntityRef::parse(&format!("ticket:{}", resolution.ticket_id))
        })
        .transpose()
}

#[cfg(feature = "serve")]
fn ensure_service_grant(
    fs: &FileStore,
    principal: WorkspaceId,
    workspace: WorkspaceId,
    facet: FacetKind,
    scope: AclScope,
) -> Result<(), String> {
    let mut acl = fs
        .acl_store()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "reference resolver requires acl state".to_string())?;
    let grant = AclGrant {
        subject: AclSubject::Principal(principal),
        workspace: Some(workspace),
        domain: Some(facet.into()),
        ref_glob: None,
        scopes: vec![scope],
        rights: [AclRight::Read, AclRight::Write].into_iter().collect(),
        effect: AclEffect::Allow,
        predicate: None,
    };
    if acl.grants().contains(&grant) {
        return Ok(());
    }
    acl.grant(grant).map_err(|error| error.to_string())?;
    fs.save_acl_store_audited(
        &acl,
        Some(principal),
        "reference_resolver.acl.ensure",
        Some(&format!(
            "principal={principal};workspace={workspace};facet={facet:?}"
        )),
    )
    .map(|_| ())
    .map_err(|error| error.to_string())
}

fn parse_daemon_stop_request(
    fields: Vec<&str>,
) -> Result<(DaemonStopRequest, DaemonRequestAuth), String> {
    let (fields, auth) = split_daemon_auth_fields(fields)?;
    let mut request = DaemonStopRequest::default();
    for field in fields {
        if let Some(value) = field.strip_prefix("hard=") {
            request.hard = match value {
                "true" => true,
                "false" => false,
                _ => return Err("hard must be true or false".to_string()),
            };
        } else if let Some(value) = field.strip_prefix("wait-ms=") {
            request.wait_ms = value
                .parse::<u64>()
                .map_err(|_| "wait-ms must be a non-negative integer".to_string())?;
        } else {
            return Err(format!("unknown stop field {field:?}"));
        }
    }
    if request.hard {
        request.wait_ms = 0;
    }
    Ok((request, auth))
}

fn split_daemon_auth_fields(fields: Vec<&str>) -> Result<(Vec<&str>, DaemonRequestAuth), String> {
    let mut payload = Vec::new();
    let mut auth = DaemonRequestAuth::default();
    let mut seen_auth = false;
    for field in fields {
        if let Some(principal) = field.strip_prefix("auth-principal=") {
            seen_auth = true;
            if auth.principal.is_some() {
                return Err("duplicate auth principal".to_string());
            }
            auth.principal = Some(WorkspaceId::parse(principal).map_err(|e| e.to_string())?);
        } else if let Some(passphrase) = field.strip_prefix("auth-passphrase-hex=") {
            seen_auth = true;
            if auth.passphrase.is_some() {
                return Err("duplicate auth passphrase".to_string());
            }
            let bytes = daemon::hex_decode(passphrase).map_err(|e| e.to_string())?;
            auth.passphrase = Some(
                String::from_utf8(bytes)
                    .map_err(|_| "auth passphrase is not valid utf-8".to_string())?,
            );
        } else if let Some(session) = field.strip_prefix("auth-session=") {
            seen_auth = true;
            if auth.session.is_some() {
                return Err("duplicate auth session".to_string());
            }
            auth.session = Some(session.to_string());
        } else if seen_auth {
            return Err("auth fields must follow request fields".to_string());
        } else {
            payload.push(field);
        }
    }
    Ok((payload, auth))
}

impl DaemonRuntime {
    fn reconcile_store_maintenance(&mut self) -> Result<(), String> {
        let now = now_ms();
        if now < self.maintenance_next_ms {
            return Ok(());
        }
        let fs = FileStore::open_read(&self.store).map_err(|e| e.to_string())?;
        let report = fs
            .store_maintenance_report(now)
            .map_err(|e| e.to_string())?;
        self.maintenance_next_ms = now.saturating_add(report.policy.interval_ms);
        if !report.eligible {
            return Ok(());
        }
        let mut loom = loom_store::open_loom_daemon_authorized_unlocked(&self.store, None)
            .map_err(|e| e.to_string())?;
        match run_store_maintenance_once(&mut loom, now, false, None, None) {
            Ok(_) => Ok(()),
            Err(error) => {
                let fs = FileStore::open_read(&self.store).map_err(|e| e.to_string())?;
                let policy = fs.store_maintenance_policy().map_err(|e| e.to_string())?;
                drop(fs);
                self.maintenance_next_ms = now.saturating_add(policy.backoff_ms);
                let writable =
                    FileStore::open_daemon_authorized(&self.store).map_err(|e| e.to_string())?;
                writable
                    .record_store_maintenance_run_state(StoreMaintenanceRunState {
                        last_run_ms: Some(now),
                        next_eligible_ms: self.maintenance_next_ms,
                        last_skip_reason: None,
                        last_error: Some(error),
                        ..StoreMaintenanceRunState::default()
                    })
                    .map_err(|e| e.to_string())?;
                Ok(())
            }
        }
    }

    fn status_response(&mut self) -> String {
        self.prune_expired_pins(now_ms());
        let permanent_pins = self
            .pins
            .values()
            .filter(|pin| pin.deadline_ms.is_none())
            .count();
        let leased_pins = self.pins.len() - permanent_pins;
        let mut response = format!(
            "{}\tsessions={}\tpins={}\tpermanent_pins={}\tleased_pins={}",
            daemon_running_response(&self.store, &self.store_id, self.transport).trim_end(),
            self.sessions.len(),
            self.pins.len(),
            permanent_pins,
            leased_pins
        );
        for (id, pin) in &self.pins {
            match pin.deadline_ms {
                Some(deadline_ms) => response.push_str(&format!(
                    "\tpin=leased:{}:{}",
                    deadline_ms,
                    daemon::hex_encode(id.as_bytes())
                )),
                None => response.push_str(&format!(
                    "\tpin=permanent:{}",
                    daemon::hex_encode(id.as_bytes())
                )),
            }
        }
        response.push('\n');
        response
    }

    fn stop_daemon(
        &mut self,
        force: bool,
        request: DaemonStopRequest,
        actor: Option<WorkspaceId>,
    ) -> String {
        self.prune_expired_pins(now_ms());
        if !force && !self.pins.is_empty() {
            return format!("error\tdaemon has {} live pin(s)\n", self.pins.len());
        }
        let pin_count = self.pins.len();
        let (listeners, timed_out) = self.stop_hosted_listeners(request);
        let target = format!(
            "force={force};hard={};wait_ms={};pins={pin_count};listeners={listeners};timed_out={timed_out}",
            request.hard, request.wait_ms
        );
        if let Err(e) = append_daemon_audit_actor(&self.store, actor, "daemon.stop", Some(&target))
        {
            return format!("error\t{e}\n");
        }
        format!(
            "stopped\t{}\t{}\tforce={force}\thard={}\twait_ms={}\tpins={pin_count}\tlisteners={listeners}\ttimed_out={timed_out}\n",
            std::process::id(),
            self.store,
            request.hard,
            request.wait_ms
        )
    }

    fn handle(&mut self, request: &str) -> String {
        let mut parts = request.trim_end().split('\t');
        let Some(command) = parts.next() else {
            return "error\tempty daemon request\n".to_string();
        };
        match command {
            "ping" | "status" => self.status_response(),
            "session-attach" => self.session_attach(parts.collect()),
            "session-check" => self.session_check(parts.collect()),
            "session-detach" => self.session_detach(parts.collect()),
            "pin-add" => self.pin_add(parts.collect()),
            "pin-remove" => self.pin_remove(parts.collect()),
            "lock-acquire" => self.lock_acquire(parts.collect()),
            "lock-refresh" => self.lock_refresh(parts.collect()),
            "lock-release" => self.lock_release(parts.collect()),
            "lock-break" => self.lock_break(parts.collect()),
            "lock-apply-fence" => self.lock_apply_fence(parts.collect()),
            "kv-put" => self.kv_put(parts.collect()),
            "kv-get" => self.kv_get(parts.collect()),
            "kv-delete" => self.kv_delete(parts.collect()),
            "kv-list" => self.kv_list(parts.collect()),
            "kv-range" => self.kv_range(parts.collect()),
            "fts-status" => self.fts_status(parts.collect()),
            "fts-rebuild" => self.fts_rebuild(parts.collect()),
            "maintenance-status" => self.maintenance_status(parts.collect()),
            "maintenance-run" => self.maintenance_run(parts.collect()),
            #[cfg(feature = "serve")]
            "reference-reconcile" => self.reference_reconcile(parts.collect()),
            "stop" => {
                let (request, auth) = match parse_daemon_stop_request(parts.collect()) {
                    Ok(split) => split,
                    Err(e) => return format!("error\t{e}\n"),
                };
                let actor = match self.authorize_daemon_operator(&auth) {
                    Ok(actor) => actor,
                    Err(e) => return format!("error\t{e}\n"),
                };
                self.stop_daemon(false, request, actor)
            }
            "stop-force" => {
                let (request, auth) = match parse_daemon_stop_request(parts.collect()) {
                    Ok(split) => split,
                    Err(e) => return format!("error\t{e}\n"),
                };
                let actor = match self.authorize_daemon_operator(&auth) {
                    Ok(actor) => actor,
                    Err(e) => return format!("error\t{e}\n"),
                };
                self.stop_daemon(true, request, actor)
            }
            other => format!("error\tunknown request {other:?}\n"),
        }
    }

    fn open_operator_loom(&self, actor: Option<WorkspaceId>) -> Result<Loom<FileStore>, String> {
        let loom = loom_store::open_loom_daemon_authorized_unlocked(&self.store, None)
            .map_err(|e| e.to_string())?;
        let Some(principal) = actor else {
            return Ok(loom);
        };
        loom_store::attach_local_auth(
            loom,
            &LocalOpenAuth {
                unlock_key: None,
                principal: None,
                passphrase: None,
                app_credential: None,
                verified_external: None,
                preauthenticated_principal: Some(principal),
                session_id: Some("daemon-fts".to_string()),
            },
        )
        .map_err(|e| e.to_string())
    }

    fn maintenance_status(&mut self, fields: Vec<&str>) -> String {
        let (fields, auth) = match split_daemon_auth_fields(fields) {
            Ok(split) => split,
            Err(e) => return format!("error\t{e}\n"),
        };
        if !fields.is_empty() {
            return "error\tmaintenance status expects no fields\n".to_string();
        }
        let actor = match self.authorize_daemon_operator(&auth) {
            Ok(actor) => actor,
            Err(e) => return format!("error\t{e}\n"),
        };
        let loom = match self.open_operator_loom(actor) {
            Ok(loom) => loom,
            Err(e) => return format!("error\t{e}\n"),
        };
        match loom.store().store_maintenance_report(now_ms()) {
            Ok(report) => match maintenance_live_root_diagnostics(&loom) {
                Ok(diagnostics) => maintenance_report_text(&report, Some(&diagnostics)),
                Err(error) => format!("error\t{error}\n"),
            },
            Err(e) => format!("error\t{e}\n"),
        }
    }

    fn maintenance_run(&mut self, fields: Vec<&str>) -> String {
        let (fields, auth) = match split_daemon_auth_fields(fields) {
            Ok(split) => split,
            Err(e) => return format!("error\t{e}\n"),
        };
        let [max_segments, max_pages] = fields.as_slice() else {
            return "error\tmaintenance run expects two fields\n".to_string();
        };
        let max_segments = match parse_optional_u64_field(max_segments) {
            Ok(value) => value,
            Err(e) => return format!("error\t{e}\n"),
        };
        let max_pages = match parse_optional_u64_field(max_pages) {
            Ok(value) => value,
            Err(e) => return format!("error\t{e}\n"),
        };
        let actor = match self.authorize_daemon_operator(&auth) {
            Ok(actor) => actor,
            Err(e) => return format!("error\t{e}\n"),
        };
        let mut loom = match self.open_operator_loom(actor) {
            Ok(loom) => loom,
            Err(e) => return format!("error\t{e}\n"),
        };
        match run_store_maintenance_once(&mut loom, now_ms(), true, max_segments, max_pages) {
            Ok(outcome) => format!("{outcome}\n"),
            Err(e) => format!("error\t{e}\n"),
        }
    }

    fn fts_status(&mut self, fields: Vec<&str>) -> String {
        let (fields, auth) = match split_daemon_auth_fields(fields) {
            Ok(split) => split,
            Err(e) => return format!("error\t{e}\n"),
        };
        let actor = match self.authorize_daemon_operator(&auth) {
            Ok(actor) => actor,
            Err(e) => return format!("error\t{e}\n"),
        };
        let [workspace, collection, engine_version] = fields.as_slice() else {
            return "error\tfts status expects three fields\n".to_string();
        };
        let loom = match self.open_operator_loom(actor) {
            Ok(loom) => loom,
            Err(e) => return format!("error\t{e}\n"),
        };
        let ns = match resolve_ns(&loom, workspace) {
            Ok(ns) => ns,
            Err(e) => return format!("error\t{e}\n"),
        };
        let source_digest = match loom_core::search_source_digest(&loom, ns, collection) {
            Ok(digest) => digest,
            Err(e) => return format!("error\t{e}\n"),
        };
        match loom
            .store()
            .search_tantivy_status(ns, collection, source_digest, engine_version)
        {
            Ok(status) => {
                fts_status_response(ns, collection, source_digest, engine_version, &status)
            }
            Err(e) => format!("error\t{e}\n"),
        }
    }

    fn fts_rebuild(&mut self, fields: Vec<&str>) -> String {
        let (fields, auth) = match split_daemon_auth_fields(fields) {
            Ok(split) => split,
            Err(e) => return format!("error\t{e}\n"),
        };
        let actor = match self.authorize_daemon_operator(&auth) {
            Ok(actor) => actor,
            Err(e) => return format!("error\t{e}\n"),
        };
        let [workspace, collection, engine_version] = fields.as_slice() else {
            return "error\tfts rebuild expects three fields\n".to_string();
        };
        let loom = match self.open_operator_loom(actor) {
            Ok(loom) => loom,
            Err(e) => return format!("error\t{e}\n"),
        };
        let ns = match resolve_ns(&loom, workspace) {
            Ok(ns) => ns,
            Err(e) => return format!("error\t{e}\n"),
        };
        let search = match loom_core::get_search(&loom, ns, collection) {
            Ok(search) => search,
            Err(e) => return format!("error\t{e}\n"),
        };
        let source_digest = match loom_core::search_source_digest(&loom, ns, collection) {
            Ok(digest) => digest,
            Err(e) => return format!("error\t{e}\n"),
        };
        let rebuild = match loom.store().begin_search_tantivy_rebuild(
            ns,
            collection,
            source_digest,
            engine_version,
        ) {
            Ok(rebuild) => rebuild,
            Err(e) => return format!("error\t{e}\n"),
        };
        let status = match rebuild {
            DerivedArtifactRebuild::AlreadyReady { record } => {
                DerivedArtifactStatus::Ready { record }
            }
            DerivedArtifactRebuild::Coalesced { .. } => match loom.store().search_tantivy_status(
                ns,
                collection,
                source_digest,
                engine_version,
            ) {
                Ok(status) => status,
                Err(e) => return format!("error\t{e}\n"),
            },
            DerivedArtifactRebuild::Started { run_id } => {
                drop(loom);
                let target = format!(
                    "workspace={ns};collection={collection};source_digest={source_digest};engine_version={engine_version};run_id={run_id}"
                );
                if let Err(e) = append_daemon_audit_actor(
                    &self.store,
                    actor,
                    "fts.tantivy.rebuild.schedule",
                    Some(&target),
                ) {
                    return format!("error\t{e}\n");
                }
                match schedule_daemon_fts_rebuild(
                    self.store.clone(),
                    ns,
                    collection.to_string(),
                    source_digest,
                    engine_version.to_string(),
                    run_id,
                    search,
                ) {
                    Ok(status) => status,
                    Err(e) => return format!("error\t{e}\n"),
                }
            }
        };
        fts_status_response(ns, collection, source_digest, engine_version, &status)
    }

    fn reconcile_authority_replication(&mut self) -> Result<(), String> {
        let now = now_ms();
        let fs = FileStore::open_read(&self.store).map_err(|e| e.to_string())?;
        let policies = fs
            .authority_replication_policies()
            .map_err(|e| e.to_string())?;
        let active = policies
            .iter()
            .map(|policy| policy.id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        self.authority_replication_next
            .retain(|id, _| active.contains(id));
        for policy in policies {
            if !policy.enabled {
                self.authority_replication_next.remove(&policy.id);
                continue;
            }
            let first_seen = !self.authority_replication_next.contains_key(&policy.id);
            let due = if first_seen {
                policy.pull_on_start
            } else {
                self.authority_replication_next
                    .get(&policy.id)
                    .is_some_and(|next| now >= *next)
            };
            if !due {
                if first_seen && let Some(interval) = policy.interval_ms {
                    self.authority_replication_next.insert(
                        policy.id.clone(),
                        now.saturating_add(interval)
                            .saturating_add(authority_replication_jitter(&policy)),
                    );
                }
                continue;
            }
            let next = match apply_authority_replication_policy(&self.store, policy.clone(), now) {
                Ok(()) => policy.interval_ms.map(|interval| {
                    now.saturating_add(interval)
                        .saturating_add(authority_replication_jitter(&policy))
                }),
                Err(reason) => {
                    mark_authority_replication_failure(&self.store, policy.clone(), now, &reason)?;
                    Some(
                        now.saturating_add(policy.backoff_ms)
                            .saturating_add(authority_replication_jitter(&policy)),
                    )
                }
            };
            match next {
                Some(next) => {
                    self.authority_replication_next
                        .insert(policy.id.clone(), next);
                }
                None => {
                    self.authority_replication_next.remove(&policy.id);
                }
            }
        }
        Ok(())
    }

    #[cfg(feature = "serve")]
    fn reconcile_hosted_listeners(&mut self) -> Result<(), String> {
        let desired = desired_hosted_listener_runtimes(&self.store)?;
        let desired_ids = desired
            .iter()
            .map(|runtime| runtime.id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let current_ids = self
            .hosted_listeners
            .keys()
            .cloned()
            .collect::<Vec<String>>();
        for id in current_ids {
            if !desired_ids.contains(&id) {
                self.hosted_listeners.remove(&id);
            }
        }
        for desired_runtime in desired {
            let restart = self
                .hosted_listeners
                .get(&desired_runtime.id)
                .is_some_and(|runtime| {
                    runtime.configuration_fingerprint != desired_runtime.configuration_fingerprint
                        || runtime.tls_certificate_bundle_fingerprint.as_deref()
                            != desired_runtime
                                .tls_certificate_bundle_fingerprint
                                .as_deref()
                        || runtime.network_access_policy_fingerprint.as_deref()
                            != desired_runtime.network_access_policy_fingerprint.as_deref()
                });
            if restart {
                self.hosted_listeners.remove(&desired_runtime.id);
            }
            if !self.hosted_listeners.contains_key(&desired_runtime.id) {
                let id = desired_runtime.id.clone();
                let runtime = match start_served_runtime(&self.store, desired_runtime) {
                    Ok(runtime) => runtime,
                    Err(err) => {
                        let (record, reason) = *err;
                        let target = format!("{};reason={reason}", served_listener_target(&record));
                        append_daemon_audit(&self.store, "serve.listener.reject", Some(&target))?;
                        return Err(reason);
                    }
                };
                self.hosted_listeners.insert(id, runtime);
            }
        }
        Ok(())
    }

    #[cfg(feature = "serve")]
    fn reconcile_drive_policy_workers(&mut self) -> Result<(), String> {
        let now = now_ms();
        if now < self.drive_policy_next_ms {
            return Ok(());
        }
        self.drive_policy_next_ms = now.saturating_add(DEFAULT_DRIVE_POLICY_RECONCILE_MS);
        match apply_drive_policy_workers_once(&self.store, now) {
            Ok(_) => Ok(()),
            Err(reason) => {
                append_daemon_audit(
                    &self.store,
                    "drive.policy_worker.error",
                    Some(&format!("reason={reason}")),
                )?;
                Ok(())
            }
        }
    }

    #[cfg(feature = "serve")]
    fn reconcile_reference_workers(&mut self) -> Result<(), String> {
        let now = now_ms();
        if now < self.reference_reconcile_next_ms {
            return Ok(());
        }
        match reconcile_references_once(&self.store, now, 100) {
            Ok(run) => {
                self.reference_reconcile_next_ms = run.next_attempt_ms.unwrap_or(u64::MAX);
                Ok(())
            }
            Err(reason) => {
                self.reference_reconcile_next_ms = now.saturating_add(REFERENCE_RECONCILE_IDLE_MS);
                append_daemon_audit(
                    &self.store,
                    "reference_resolver.error",
                    Some(&format!("reason={reason}")),
                )?;
                Ok(())
            }
        }
    }

    #[cfg(feature = "serve")]
    fn reference_reconcile(&mut self, fields: Vec<&str>) -> String {
        let [session] = fields.as_slice() else {
            return "error\treference-reconcile requires one attached session\n".to_string();
        };
        if !self.sessions.contains(*session) {
            return "error\treference-reconcile requires an attached session\n".to_string();
        }
        self.reference_reconcile_next_ms = 0;
        "ok\n".to_string()
    }

    #[cfg(feature = "serve")]
    fn stop_hosted_listeners(&mut self, request: DaemonStopRequest) -> (usize, usize) {
        let policy = HostedStopPolicy {
            hard: request.hard,
            wait_ms: request.wait_ms,
        };
        let listeners = std::mem::take(&mut self.hosted_listeners);
        let count = listeners.len();
        let mut timed_out = 0usize;
        for (_, runtime) in listeners {
            if runtime.stop(policy) {
                timed_out += 1;
            }
        }
        (count, timed_out)
    }

    #[cfg(not(feature = "serve"))]
    fn stop_hosted_listeners(&mut self, _request: DaemonStopRequest) -> (usize, usize) {
        (0, 0)
    }

    #[cfg(not(feature = "serve"))]
    fn reconcile_drive_policy_workers(&mut self) -> Result<(), String> {
        Ok(())
    }

    #[cfg(not(feature = "serve"))]
    fn reconcile_reference_workers(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn authorize_daemon_operator(
        &self,
        auth: &DaemonRequestAuth,
    ) -> Result<Option<WorkspaceId>, String> {
        let fs = FileStore::open_read(&self.store).map_err(|e| e.to_string())?;
        let Some(mut identity) = fs.identity_store().map_err(|e| e.to_string())? else {
            return Ok(None);
        };
        if !identity.authenticated_mode() {
            return Ok(None);
        }
        let principal = auth.principal.ok_or_else(|| {
            loom_core::error::LoomError::new(
                loom_core::Code::AuthenticationFailed,
                "daemon operation requires authentication",
            )
            .to_string()
        })?;
        let passphrase = auth.passphrase.as_deref().ok_or_else(|| {
            loom_core::error::LoomError::new(
                loom_core::Code::AuthenticationFailed,
                "daemon operation requires authentication",
            )
            .to_string()
        })?;
        let session_id = auth
            .session
            .clone()
            .unwrap_or_else(|| format!("daemon-operator-{}", std::process::id()));
        let session = identity
            .authenticate_passphrase(principal, passphrase, session_id)
            .map_err(|e| e.to_string())?;
        let acl = fs.acl_store().map_err(|e| e.to_string())?.ok_or_else(|| {
            loom_core::error::LoomError::new(
                loom_core::Code::PermissionDenied,
                "daemon operation requires acl state",
            )
            .to_string()
        })?;
        let roles = identity
            .effective_roles(session.principal)
            .map_err(|e| e.to_string())?;
        acl.authorize_global_admin_with_roles(
            identity.authenticated_mode(),
            session.principal,
            roles,
        )
        .map_err(|e| e.to_string())?;
        Ok(Some(session.principal))
    }

    fn session_attach(&mut self, fields: Vec<&str>) -> String {
        let (fields, auth) = match split_daemon_auth_fields(fields) {
            Ok(split) => split,
            Err(e) => return format!("error\t{e}\n"),
        };
        let actor = match self.authorize_daemon_operator(&auth) {
            Ok(actor) => actor,
            Err(e) => return format!("error\t{e}\n"),
        };
        let [session] = fields.as_slice() else {
            return "error\tsession attach expects one field\n".to_string();
        };
        let inserted = self.sessions.insert((*session).to_string());
        let target = format!("session={session};sessions={}", self.sessions.len());
        if let Err(e) =
            append_daemon_audit_actor(&self.store, actor, "daemon.session.attach", Some(&target))
        {
            if inserted {
                self.sessions.remove(*session);
            }
            return format!("error\t{e}\n");
        }
        format!("attached\t{session}\tsessions={}\n", self.sessions.len())
    }

    fn session_detach(&mut self, fields: Vec<&str>) -> String {
        let (fields, auth) = match split_daemon_auth_fields(fields) {
            Ok(split) => split,
            Err(e) => return format!("error\t{e}\n"),
        };
        let actor = match self.authorize_daemon_operator(&auth) {
            Ok(actor) => actor,
            Err(e) => return format!("error\t{e}\n"),
        };
        let [session] = fields.as_slice() else {
            return "error\tsession detach expects one field\n".to_string();
        };
        let removed = self.sessions.remove(*session);
        let target = format!("session={session};sessions={}", self.sessions.len());
        if let Err(e) =
            append_daemon_audit_actor(&self.store, actor, "daemon.session.detach", Some(&target))
        {
            if removed {
                self.sessions.insert((*session).to_string());
            }
            return format!("error\t{e}\n");
        }
        format!("detached\t{session}\tsessions={}\n", self.sessions.len())
    }

    fn session_check(&mut self, fields: Vec<&str>) -> String {
        let (fields, auth) = match split_daemon_auth_fields(fields) {
            Ok(split) => split,
            Err(e) => return format!("error\t{e}\n"),
        };
        let actor = match self.authorize_daemon_operator(&auth) {
            Ok(actor) => actor,
            Err(e) => return format!("error\t{e}\n"),
        };
        let [session] = fields.as_slice() else {
            return "error\tsession check expects one field\n".to_string();
        };
        if !self.sessions.contains(*session) {
            return "error\tNOT_FOUND: daemon session is not attached\n".to_string();
        }
        let target = format!("session={session};sessions={}", self.sessions.len());
        if let Err(e) =
            append_daemon_audit_actor(&self.store, actor, "daemon.session.check", Some(&target))
        {
            return format!("error\t{e}\n");
        }
        format!(
            "session\t{session}\tlive\tsessions={}\n",
            self.sessions.len()
        )
    }

    fn pin_add(&mut self, fields: Vec<&str>) -> String {
        let (fields, auth) = match split_daemon_auth_fields(fields) {
            Ok(split) => split,
            Err(e) => return format!("error\t{e}\n"),
        };
        let actor = match self.authorize_daemon_operator(&auth) {
            Ok(actor) => actor,
            Err(e) => return format!("error\t{e}\n"),
        };
        let (pin, lease) = match fields.as_slice() {
            [pin] => (*pin, PinLease { deadline_ms: None }),
            [pin, lease_ms, now_ms] => {
                let lease_ms = match lease_ms.parse::<u64>() {
                    Ok(value) if value > 0 => value,
                    _ => return "error\tinvalid pin lease\n".to_string(),
                };
                let now_ms = match now_ms.parse::<u64>() {
                    Ok(value) => value,
                    Err(_) => return "error\tinvalid pin clock\n".to_string(),
                };
                let Some(deadline_ms) = now_ms.checked_add(lease_ms) else {
                    return "error\tinvalid pin lease deadline\n".to_string();
                };
                (
                    *pin,
                    PinLease {
                        deadline_ms: Some(deadline_ms),
                    },
                )
            }
            _ => return "error\tpin add expects one or three fields\n".to_string(),
        };
        let previous = self.pins.insert(pin.to_string(), lease);
        let target = format!("pin={pin};pins={}", self.pins.len());
        if let Err(e) =
            append_daemon_audit_actor(&self.store, actor, "daemon.pin.add", Some(&target))
        {
            match previous {
                Some(previous) => {
                    self.pins.insert(pin.to_string(), previous);
                }
                None => {
                    self.pins.remove(pin);
                }
            }
            return format!("error\t{e}\n");
        }
        format!("pinned\t{pin}\tpins={}\n", self.pins.len())
    }

    fn pin_remove(&mut self, fields: Vec<&str>) -> String {
        let (fields, auth) = match split_daemon_auth_fields(fields) {
            Ok(split) => split,
            Err(e) => return format!("error\t{e}\n"),
        };
        let actor = match self.authorize_daemon_operator(&auth) {
            Ok(actor) => actor,
            Err(e) => return format!("error\t{e}\n"),
        };
        let [pin] = fields.as_slice() else {
            return "error\tpin remove expects one field\n".to_string();
        };
        let previous = self.pins.remove(*pin);
        let target = format!("pin={pin};pins={}", self.pins.len());
        if let Err(e) =
            append_daemon_audit_actor(&self.store, actor, "daemon.pin.remove", Some(&target))
        {
            if let Some(previous) = previous {
                self.pins.insert((*pin).to_string(), previous);
            }
            return format!("error\t{e}\n");
        }
        format!("unpinned\t{pin}\tpins={}\n", self.pins.len())
    }

    fn prune_expired_pins(&mut self, now_ms: u64) {
        self.pins
            .retain(|_, pin| pin.deadline_ms.is_none_or(|deadline| deadline > now_ms));
    }

    fn lock_acquire(&mut self, fields: Vec<&str>) -> String {
        let (fields, auth) = match split_daemon_auth_fields(fields) {
            Ok(split) => split,
            Err(e) => return format!("error\t{e}\n"),
        };
        if let Err(e) = self.authorize_daemon_operator(&auth) {
            return format!("error\t{e}\n");
        }
        let [key, principal, session, mode, lease_ms, now_ms] = fields.as_slice() else {
            return "error\tlock acquire expects six fields\n".to_string();
        };
        let mode = match daemon::parse_lock_mode_wire(mode) {
            Ok(mode) => mode,
            Err(e) => return format!("error\t{e}\n"),
        };
        let lease_ms = match lease_ms.parse::<u64>() {
            Ok(value) => value,
            Err(_) => return "error\tinvalid lock lease\n".to_string(),
        };
        let now_ms = match now_ms.parse::<u64>() {
            Ok(value) => value,
            Err(_) => return "error\tinvalid lock clock\n".to_string(),
        };
        let owner = LockOwner {
            principal: (*principal).to_string(),
            session: (*session).to_string(),
        };
        match self
            .coordinator
            .try_acquire(key.as_bytes().to_vec(), owner, mode, lease_ms, now_ms)
        {
            Ok(token) => match save_daemon_lock_coordinator(&self.store, &self.coordinator) {
                Ok(()) => daemon::lock_token_response(&token),
                Err(e) => {
                    let _ = self.coordinator.release(&token, now_ms);
                    format!("error\t{e}\n")
                }
            },
            Err(e) => format!("error\t{e}\n"),
        }
    }

    fn lock_refresh(&mut self, fields: Vec<&str>) -> String {
        let (fields, auth) = match split_daemon_auth_fields(fields) {
            Ok(split) => split,
            Err(e) => return format!("error\t{e}\n"),
        };
        if let Err(e) = self.authorize_daemon_operator(&auth) {
            return format!("error\t{e}\n");
        }
        let [key, principal, session, mode, fence, lease_ms, now_ms] = fields.as_slice() else {
            return "error\tlock refresh expects seven fields\n".to_string();
        };
        let mode = match daemon::parse_lock_mode_wire(mode) {
            Ok(mode) => mode,
            Err(e) => return format!("error\t{e}\n"),
        };
        let fence = match fence.parse::<u64>() {
            Ok(value) => value,
            Err(_) => return "error\tinvalid lock fence\n".to_string(),
        };
        let lease_ms = match lease_ms.parse::<u64>() {
            Ok(value) => value,
            Err(_) => return "error\tinvalid lock lease\n".to_string(),
        };
        let now_ms = match now_ms.parse::<u64>() {
            Ok(value) => value,
            Err(_) => return "error\tinvalid lock clock\n".to_string(),
        };
        let token = daemon::lock_token(key, principal, session, mode, fence);
        match self.coordinator.refresh(&token, lease_ms, now_ms) {
            Ok(token) => daemon::lock_token_response(&token),
            Err(e) => format!("error\t{e}\n"),
        }
    }

    fn lock_release(&mut self, fields: Vec<&str>) -> String {
        let (fields, auth) = match split_daemon_auth_fields(fields) {
            Ok(split) => split,
            Err(e) => return format!("error\t{e}\n"),
        };
        if let Err(e) = self.authorize_daemon_operator(&auth) {
            return format!("error\t{e}\n");
        }
        let [key, principal, session, mode, fence, now_ms] = fields.as_slice() else {
            return "error\tlock release expects six fields\n".to_string();
        };
        let mode = match daemon::parse_lock_mode_wire(mode) {
            Ok(mode) => mode,
            Err(e) => return format!("error\t{e}\n"),
        };
        let fence = match fence.parse::<u64>() {
            Ok(value) => value,
            Err(_) => return "error\tinvalid lock fence\n".to_string(),
        };
        let now_ms = match now_ms.parse::<u64>() {
            Ok(value) => value,
            Err(_) => return "error\tinvalid lock clock\n".to_string(),
        };
        let token = daemon::lock_token(key, principal, session, mode, fence);
        match self.coordinator.release(&token, now_ms) {
            Ok(()) => "released\n".to_string(),
            Err(e) => format!("error\t{e}\n"),
        }
    }

    fn lock_break(&mut self, fields: Vec<&str>) -> String {
        let (fields, auth) = match split_daemon_auth_fields(fields) {
            Ok(split) => split,
            Err(e) => return format!("error\t{e}\n"),
        };
        if let Err(e) = self.authorize_daemon_operator(&auth) {
            return format!("error\t{e}\n");
        }
        let [key, now_ms] = fields.as_slice() else {
            return "error\tlock break expects two fields\n".to_string();
        };
        let now_ms = match now_ms.parse::<u64>() {
            Ok(value) => value,
            Err(_) => return "error\tinvalid lock clock\n".to_string(),
        };
        let broken = self.coordinator.break_key(key.as_bytes(), now_ms);
        format!("broken\t{broken}\n")
    }

    fn lock_apply_fence(&mut self, fields: Vec<&str>) -> String {
        let (fields, auth) = match split_daemon_auth_fields(fields) {
            Ok(split) => split,
            Err(e) => return format!("error\t{e}\n"),
        };
        if let Err(e) = self.authorize_daemon_operator(&auth) {
            return format!("error\t{e}\n");
        }
        let [key, principal, session, mode, fence, now_ms] = fields.as_slice() else {
            return "error\tlock apply fence expects six fields\n".to_string();
        };
        let mode = match daemon::parse_lock_mode_wire(mode) {
            Ok(mode) => mode,
            Err(e) => return format!("error\t{e}\n"),
        };
        let fence = match fence.parse::<u64>() {
            Ok(value) => value,
            Err(_) => return "error\tinvalid lock fence\n".to_string(),
        };
        let now_ms = match now_ms.parse::<u64>() {
            Ok(value) => value,
            Err(_) => return "error\tinvalid lock clock\n".to_string(),
        };
        let token = daemon::lock_token(key, principal, session, mode, fence);
        match self.coordinator.apply_fenced_write(&token, now_ms) {
            Ok(()) => match save_daemon_lock_coordinator(&self.store, &self.coordinator) {
                Ok(()) => "applied\n".to_string(),
                Err(e) => format!("error\t{e}\n"),
            },
            Err(e) => format!("error\t{e}\n"),
        }
    }

    fn kv_put(&mut self, fields: Vec<&str>) -> String {
        let (fields, auth) = match split_daemon_auth_fields(fields) {
            Ok(split) => split,
            Err(e) => return format!("error\t{e}\n"),
        };
        let [session, workspace, name, key_hex, value_hex, now_ms] = fields.as_slice() else {
            return "error\tkv put expects six fields\n".to_string();
        };
        if let Err(e) = self.bind_daemon_runtime_session(session, &auth) {
            return format!("error\t{e}\n");
        }
        let key_cbor = match daemon::hex_decode(key_hex) {
            Ok(bytes) => bytes,
            Err(e) => return format!("error\t{e}\n"),
        };
        let value = match daemon::hex_decode(value_hex) {
            Ok(bytes) => bytes,
            Err(e) => return format!("error\t{e}\n"),
        };
        let key = match loom_core::key_from_cbor(&key_cbor) {
            Ok(key) => key,
            Err(e) => return format!("error\t{e}\n"),
        };
        let now_ms = match now_ms.parse::<u64>() {
            Ok(value) => value,
            Err(_) => return "error\tinvalid kv clock\n".to_string(),
        };
        match self.with_pure_ephemeral_kv(session, workspace, name, |loom, ns| {
            loom.kv_put_configured(ns, name, key, value, None, now_ms)
        }) {
            Ok(()) => "ok\n".to_string(),
            Err(e) => format!("error\t{e}\n"),
        }
    }

    fn kv_get(&mut self, fields: Vec<&str>) -> String {
        let (fields, auth) = match split_daemon_auth_fields(fields) {
            Ok(split) => split,
            Err(e) => return format!("error\t{e}\n"),
        };
        let [session, workspace, name, key_hex, now_ms] = fields.as_slice() else {
            return "error\tkv get expects five fields\n".to_string();
        };
        if let Err(e) = self.bind_daemon_runtime_session(session, &auth) {
            return format!("error\t{e}\n");
        }
        let key_cbor = match daemon::hex_decode(key_hex) {
            Ok(bytes) => bytes,
            Err(e) => return format!("error\t{e}\n"),
        };
        let key = match loom_core::key_from_cbor(&key_cbor) {
            Ok(key) => key,
            Err(e) => return format!("error\t{e}\n"),
        };
        let now_ms = match now_ms.parse::<u64>() {
            Ok(value) => value,
            Err(_) => return "error\tinvalid kv clock\n".to_string(),
        };
        match self.with_pure_ephemeral_kv(session, workspace, name, |loom, ns| {
            loom.kv_get_configured(ns, name, &key, now_ms)
        }) {
            Ok(Some(value)) => format!("kv\t1\t{}\n", daemon::hex_encode(&value)),
            Ok(None) => "kv\t0\n".to_string(),
            Err(e) => format!("error\t{e}\n"),
        }
    }

    fn kv_delete(&mut self, fields: Vec<&str>) -> String {
        let (fields, auth) = match split_daemon_auth_fields(fields) {
            Ok(split) => split,
            Err(e) => return format!("error\t{e}\n"),
        };
        let [session, workspace, name, key_hex] = fields.as_slice() else {
            return "error\tkv delete expects four fields\n".to_string();
        };
        if let Err(e) = self.bind_daemon_runtime_session(session, &auth) {
            return format!("error\t{e}\n");
        }
        let key_cbor = match daemon::hex_decode(key_hex) {
            Ok(bytes) => bytes,
            Err(e) => return format!("error\t{e}\n"),
        };
        let key = match loom_core::key_from_cbor(&key_cbor) {
            Ok(key) => key,
            Err(e) => return format!("error\t{e}\n"),
        };
        match self.with_pure_ephemeral_kv(session, workspace, name, |loom, ns| {
            loom.kv_delete_configured(ns, name, &key)
        }) {
            Ok(removed) => format!("deleted\t{}\n", i32::from(removed)),
            Err(e) => format!("error\t{e}\n"),
        }
    }

    fn kv_list(&mut self, fields: Vec<&str>) -> String {
        let (fields, auth) = match split_daemon_auth_fields(fields) {
            Ok(split) => split,
            Err(e) => return format!("error\t{e}\n"),
        };
        let [session, workspace, name, now_ms] = fields.as_slice() else {
            return "error\tkv list expects four fields\n".to_string();
        };
        if let Err(e) = self.bind_daemon_runtime_session(session, &auth) {
            return format!("error\t{e}\n");
        }
        let now_ms = match now_ms.parse::<u64>() {
            Ok(value) => value,
            Err(_) => return "error\tinvalid kv clock\n".to_string(),
        };
        match self.with_pure_ephemeral_kv(session, workspace, name, |loom, ns| {
            Ok(loom.kv_list_configured(ns, name, now_ms)?.encode())
        }) {
            Ok(bytes) => format!("kv-map\t{}\n", daemon::hex_encode(&bytes)),
            Err(e) => format!("error\t{e}\n"),
        }
    }

    fn kv_range(&mut self, fields: Vec<&str>) -> String {
        let (fields, auth) = match split_daemon_auth_fields(fields) {
            Ok(split) => split,
            Err(e) => return format!("error\t{e}\n"),
        };
        let [session, workspace, name, lo_hex, hi_hex, now_ms] = fields.as_slice() else {
            return "error\tkv range expects six fields\n".to_string();
        };
        if let Err(e) = self.bind_daemon_runtime_session(session, &auth) {
            return format!("error\t{e}\n");
        }
        let lo_cbor = match daemon::hex_decode(lo_hex) {
            Ok(bytes) => bytes,
            Err(e) => return format!("error\t{e}\n"),
        };
        let hi_cbor = match daemon::hex_decode(hi_hex) {
            Ok(bytes) => bytes,
            Err(e) => return format!("error\t{e}\n"),
        };
        let lo = match loom_core::key_from_cbor(&lo_cbor) {
            Ok(key) => key,
            Err(e) => return format!("error\t{e}\n"),
        };
        let hi = match loom_core::key_from_cbor(&hi_cbor) {
            Ok(key) => key,
            Err(e) => return format!("error\t{e}\n"),
        };
        let now_ms = match now_ms.parse::<u64>() {
            Ok(value) => value,
            Err(_) => return "error\tinvalid kv clock\n".to_string(),
        };
        match self.with_pure_ephemeral_kv(session, workspace, name, |loom, ns| {
            Ok(loom
                .kv_range_configured(ns, name, &lo, &hi, now_ms)?
                .encode())
        }) {
            Ok(bytes) => format!("kv-map\t{}\n", daemon::hex_encode(&bytes)),
            Err(e) => format!("error\t{e}\n"),
        }
    }

    fn with_pure_ephemeral_kv<T>(
        &mut self,
        session: &str,
        workspace: &str,
        name: &str,
        f: impl FnOnce(&mut Loom<FileStore>, WorkspaceId) -> loom_core::error::Result<T>,
    ) -> loom_core::error::Result<T> {
        let Some(loom) = self.kv_loom.as_mut() else {
            return Err(self.kv_unavailable.clone().unwrap_or_else(|| {
                loom_core::error::LoomError::unsupported(
                    "daemon cannot host ephemeral KV for this store",
                )
            }));
        };
        loom.set_session(session.to_string());
        let selector = match WorkspaceId::parse(workspace) {
            Ok(id) => WsSelector::Id(id),
            Err(_) => WsSelector::Name(workspace.to_string()),
        };
        let ns = loom.registry().open(&selector)?;
        // Pure ephemeral caches are process-local; sweeping on access bounds daemon memory.
        let _ = loom.sweep_all_expired(ns, now_ms());
        let config = loom.kv_map_config(ns, name);
        if config.tier != KvTier::Ephemeral || config.read_through || config.write_through {
            return Err(loom_core::error::LoomError::unsupported(
                "daemon hosts pure ephemeral KV maps only",
            ));
        };
        f(loom, ns)
    }

    fn bind_daemon_runtime_session(
        &mut self,
        session: &str,
        auth: &DaemonRequestAuth,
    ) -> loom_core::error::Result<()> {
        let Some(loom) = self.kv_loom.as_mut() else {
            return Err(self.kv_unavailable.clone().unwrap_or_else(|| {
                loom_core::error::LoomError::unsupported(
                    "daemon cannot host ephemeral KV for this store",
                )
            }));
        };
        let Some(mut identity) = loom.store().identity_store()? else {
            return Ok(());
        };
        if !identity.authenticated_mode() {
            loom.set_identity_store(identity);
            return Ok(());
        }
        if identity.session_principal(session).is_ok() {
            loom.set_identity_store(identity);
            return Ok(());
        }
        let principal = auth.principal.ok_or_else(|| {
            loom_core::error::LoomError::new(
                loom_core::Code::AuthenticationFailed,
                "daemon runtime state requires authentication",
            )
        })?;
        let passphrase = auth.passphrase.as_deref().ok_or_else(|| {
            loom_core::error::LoomError::new(
                loom_core::Code::AuthenticationFailed,
                "daemon runtime state requires authentication",
            )
        })?;
        identity.authenticate_passphrase(principal, passphrase, session.to_string())?;
        loom.set_identity_store(identity);
        Ok(())
    }
}

fn fts_status_response(
    workspace: WorkspaceId,
    collection: &str,
    source_digest: Digest,
    engine_version: &str,
    status: &DerivedArtifactStatus,
) -> String {
    let mut response = format!(
        "fts\t{}\t{}\t{}\t{}\t{}",
        workspace,
        collection,
        source_digest,
        engine_version,
        status.name()
    );
    match status {
        DerivedArtifactStatus::Ready { record } | DerivedArtifactStatus::Stale { record } => {
            response.push_str(&format!(
                "\tpayload_digest={}\tpayload_len={}\tformat_version={}",
                record.payload_digest, record.payload_len, record.stamp.format_version
            ));
        }
        DerivedArtifactStatus::Rebuilding { run_id, stamp } => {
            response.push_str(&format!(
                "\trun_id={run_id}\tformat_version={}",
                stamp.format_version
            ));
        }
        DerivedArtifactStatus::Failed { stamp, message }
        | DerivedArtifactStatus::Unsupported { stamp, message } => {
            response.push_str(&format!(
                "\tmessage_hex={}\tformat_version={}",
                daemon::hex_encode(message.as_bytes()),
                stamp.format_version
            ));
        }
        DerivedArtifactStatus::Missing => {}
    }
    response.push('\n');
    response
}

#[cfg(feature = "native-fts")]
fn schedule_daemon_fts_rebuild(
    store: String,
    workspace: WorkspaceId,
    collection: String,
    source_digest: Digest,
    engine_version: String,
    run_id: String,
    search: loom_core::search::SearchCollection,
) -> Result<DerivedArtifactStatus, String> {
    let stamp = loom_store::search_tantivy_artifact_stamp(source_digest, &engine_version)
        .map_err(|e| e.to_string())?;
    let status = DerivedArtifactStatus::Rebuilding {
        run_id: run_id.clone(),
        stamp,
    };
    let worker_store = store.clone();
    let worker_collection = collection.clone();
    let worker_engine_version = engine_version.clone();
    let worker_run_id = run_id.clone();
    std::thread::spawn(move || {
        let result = finish_daemon_fts_rebuild(
            &worker_store,
            workspace,
            &worker_collection,
            source_digest,
            &worker_engine_version,
            &worker_run_id,
            &search,
        );
        if let Err(reason) = result {
            let _ = append_daemon_audit(
                &worker_store,
                "fts.tantivy.rebuild.error",
                Some(&format!(
                    "workspace={workspace};collection={worker_collection};source_digest={source_digest};engine_version={worker_engine_version};run_id={worker_run_id};reason={reason}"
                )),
            );
        }
    });
    Ok(status)
}

#[cfg(feature = "native-fts")]
fn finish_daemon_fts_rebuild(
    store: &str,
    workspace: WorkspaceId,
    collection: &str,
    source_digest: Digest,
    engine_version: &str,
    run_id: &str,
    search: &loom_core::search::SearchCollection,
) -> Result<(), String> {
    let payload = match loom_tantivy::build_tantivy_index_payload(search) {
        Ok(payload) => payload,
        Err(error) => {
            let fs = open_daemon_authorized_after_request(store)?;
            fs.fail_search_tantivy_rebuild(
                workspace,
                collection,
                run_id,
                source_digest,
                engine_version,
                error.to_string(),
            )
            .map_err(|e| e.to_string())?;
            return Err(error.to_string());
        }
    };
    let fs = open_daemon_authorized_after_request(store)?;
    fs.finish_search_tantivy_rebuild(
        workspace,
        collection,
        run_id,
        source_digest,
        engine_version,
        &payload,
    )
    .map_err(|e| e.to_string())?;
    append_daemon_audit(
        store,
        "fts.tantivy.rebuild.ready",
        Some(&format!(
            "workspace={workspace};collection={collection};source_digest={source_digest};engine_version={engine_version};run_id={run_id}"
        )),
    )
}

#[cfg(feature = "native-fts")]
fn open_daemon_authorized_after_request(store: &str) -> Result<FileStore, String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match FileStore::open_daemon_authorized(store) {
            Ok(fs) => return Ok(fs),
            Err(error)
                if error.code == loom_core::Code::Conflict
                    && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

#[cfg(not(feature = "native-fts"))]
fn schedule_daemon_fts_rebuild(
    store: String,
    workspace: WorkspaceId,
    collection: String,
    source_digest: Digest,
    engine_version: String,
    run_id: String,
    _search: loom_core::search::SearchCollection,
) -> Result<DerivedArtifactStatus, String> {
    let message = "native FTS is not enabled in this loom binary";
    let fs = FileStore::open_daemon_authorized(&store).map_err(|e| e.to_string())?;
    fs.fail_search_tantivy_rebuild(
        workspace,
        &collection,
        &run_id,
        source_digest,
        &engine_version,
        message,
    )
    .map_err(|e| e.to_string())?;
    fs.mark_search_tantivy_unsupported(
        workspace,
        &collection,
        source_digest,
        &engine_version,
        message,
    )
    .map_err(|e| e.to_string())?;
    fs.search_tantivy_status(workspace, &collection, source_digest, &engine_version)
        .map_err(|e| e.to_string())
}

fn apply_authority_replication_policy(
    store: &str,
    mut policy: loom_store::AuthorityReplicationPolicy,
    now: u64,
) -> Result<(), String> {
    let source = FileStore::open_read(&policy.source).map_err(|e| e.to_string())?;
    let source_identity = source
        .identity_store()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "source identity store not initialized".to_string())?;
    let destination = FileStore::open_daemon_authorized(store).map_err(|e| e.to_string())?;
    let mut destination_identity = destination
        .identity_store()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "destination identity store not initialized".to_string())?;
    let report = destination_identity
        .replicate_authority_from(&source_identity, destination.digest_algo(), false)
        .map_err(|e| e.to_string())?;
    policy.last_success_ms = Some(now);
    policy.last_failure_ms = None;
    policy.last_error = None;
    let target = format!(
        "id={};source={};from_generation={};to_generation={};applied={};publish_witness={}",
        policy.id,
        policy.source,
        report.from_generation,
        report.to_generation,
        report.applied,
        policy.publish_witness
    );
    destination
        .save_identity_store_and_authority_replication_policy_audited(
            &destination_identity,
            &policy,
            None,
            "authority.replication.pull",
            Some(&target),
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn mark_authority_replication_failure(
    store: &str,
    mut policy: loom_store::AuthorityReplicationPolicy,
    now: u64,
    reason: &str,
) -> Result<(), String> {
    let destination = FileStore::open_daemon_authorized(store).map_err(|e| e.to_string())?;
    let reason = reason.chars().take(512).collect::<String>();
    policy.last_failure_ms = Some(now);
    policy.last_error = Some(reason.clone());
    let target = format!("id={};source={};reason={reason}", policy.id, policy.source);
    destination
        .save_authority_replication_policy_audited(
            &policy,
            None,
            "authority.replication.failure",
            Some(&target),
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn authority_replication_jitter(policy: &loom_store::AuthorityReplicationPolicy) -> u64 {
    if policy.jitter_ms == 0 {
        return 0;
    }
    policy
        .id
        .bytes()
        .chain(policy.source.bytes())
        .fold(0u64, |acc, byte| {
            acc.wrapping_mul(131).wrapping_add(u64::from(byte))
        })
        % policy.jitter_ms.saturating_add(1)
}

pub(crate) fn daemon_kv_loom(store: &str) -> Result<Loom<FileStore>, loom_core::error::LoomError> {
    loom_store::open_loom_read_unlocked(store, None).and_then(|loom| {
        loom_store::attach_local_auth(
            loom,
            &LocalOpenAuth {
                unlock_key: None,
                principal: None,
                passphrase: None,
                app_credential: None,
                verified_external: None,
                preauthenticated_principal: None,
                session_id: None,
            },
        )
    })
}

fn daemon_kv_unavailable_error(err: loom_core::error::LoomError) -> loom_core::error::LoomError {
    if err.code == loom_core::Code::E2eLocked {
        loom_core::error::LoomError::new(
            loom_core::Code::E2eLocked,
            "daemon pure-ephemeral KV requires delegated encrypted-store credentials",
        )
    } else {
        err
    }
}

pub(crate) fn save_daemon_lock_coordinator(
    store: &str,
    coordinator: &LockCoordinator,
) -> Result<(), String> {
    let fs = FileStore::open_daemon_authorized(store).map_err(|e| e.to_string())?;
    fs.save_lock_coordinator(coordinator)
        .map_err(|e| e.to_string())
}

enum LocalDaemonListener {
    Tcp(std::net::TcpListener),
    #[cfg(unix)]
    Unix(std::os::unix::net::UnixListener),
    #[cfg(windows)]
    WindowsNamedPipe(interprocess::local_socket::Listener),
}

enum LocalDaemonStream {
    Tcp(std::net::TcpStream),
    #[cfg(unix)]
    Unix(std::os::unix::net::UnixStream),
    #[cfg(windows)]
    WindowsNamedPipe(interprocess::local_socket::Stream),
}

impl LocalDaemonListener {
    fn bind(
        paths: &daemon::DaemonPaths,
        selection: DaemonTransportSelection,
    ) -> Result<Self, String> {
        if selection == DaemonTransportSelection::TcpLoopback {
            return bind_tcp_loopback_listener();
        }

        #[cfg(unix)]
        {
            if daemon::unix_peer_credentials_supported() {
                daemon::prepare_unix_socket_artifact(&paths.sock_file)
                    .map_err(|e| e.to_string())?;
                let listener = std::os::unix::net::UnixListener::bind(&paths.sock_file)
                    .map_err(|e| format!("bind daemon Unix socket: {e}"))?;
                daemon::align_runtime_artifact_owner(&paths.sock_file, "socket", paths)
                    .map_err(|e| e.to_string())?;
                daemon::validate_runtime_socket_artifact(&paths.sock_file)
                    .map_err(|e| e.to_string())?;
                listener
                    .set_nonblocking(true)
                    .map_err(|e| format!("set daemon nonblocking mode: {e}"))?;
                return Ok(Self::Unix(listener));
            }
        }
        #[cfg(windows)]
        {
            return bind_windows_named_pipe_listener(paths);
        }
        #[cfg(not(windows))]
        bind_tcp_loopback_listener()
    }

    fn transport(&self) -> daemon::DaemonTransport {
        match self {
            Self::Tcp(_) => daemon::DaemonTransport::TcpLoopback,
            #[cfg(unix)]
            Self::Unix(_) => daemon::DaemonTransport::UnixSocket,
            #[cfg(windows)]
            Self::WindowsNamedPipe(_) => daemon::DaemonTransport::WindowsNamedPipe,
        }
    }

    fn addr_file_contents(&self, paths: &daemon::DaemonPaths) -> Result<String, String> {
        match self {
            Self::Tcp(listener) => {
                let addr = listener
                    .local_addr()
                    .map_err(|e| format!("read daemon address: {e}"))?;
                Ok(daemon::addr_file_contents(paths, addr))
            }
            #[cfg(unix)]
            Self::Unix(_) => {
                Ok(daemon::DaemonEndpointEnvelope::unix_socket(paths).to_addr_file_contents())
            }
            #[cfg(windows)]
            Self::WindowsNamedPipe(_) => {
                Ok(daemon::DaemonEndpointEnvelope::windows_named_pipe(paths)
                    .to_addr_file_contents())
            }
        }
    }

    fn accept(&self, paths: &daemon::DaemonPaths) -> Result<Option<LocalDaemonStream>, String> {
        #[cfg(windows)]
        let _ = paths;

        match self {
            Self::Tcp(listener) => match listener.accept() {
                Ok((stream, _)) => Ok(Some(LocalDaemonStream::Tcp(stream))),
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock) => Ok(None),
                Err(e) => Err(format!("accept daemon connection: {e}")),
            },
            #[cfg(unix)]
            Self::Unix(listener) => match listener.accept() {
                Ok((mut stream, _)) => match daemon::validate_unix_peer_owner(&stream, paths) {
                    Ok(()) => Ok(Some(LocalDaemonStream::Unix(stream))),
                    Err(e) => {
                        let _ = stream.write_all(format!("error\t{e}\n").as_bytes());
                        Ok(None)
                    }
                },
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock) => Ok(None),
                Err(e) => Err(format!("accept daemon connection: {e}")),
            },
            #[cfg(windows)]
            Self::WindowsNamedPipe(listener) => {
                use interprocess::local_socket::prelude::*;

                match listener.accept() {
                    Ok(stream) => Ok(Some(LocalDaemonStream::WindowsNamedPipe(stream))),
                    Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock) => Ok(None),
                    Err(e) => Err(format!("accept daemon connection: {e}")),
                }
            }
        }
    }
}

fn bind_tcp_loopback_listener() -> Result<LocalDaemonListener, String> {
    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| format!("bind daemon: {e}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("set daemon nonblocking mode: {e}"))?;
    Ok(LocalDaemonListener::Tcp(listener))
}

#[cfg(windows)]
fn bind_windows_named_pipe_listener(
    paths: &daemon::DaemonPaths,
) -> Result<LocalDaemonListener, String> {
    use interprocess::local_socket::{
        GenericWorkspaced, ListenerNonblockingMode, ListenerOptions, prelude::*,
    };
    use interprocess::os::windows::local_socket::ListenerOptionsExt;
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;
    use widestring::U16CString;

    const OWNER_ONLY_PIPE_SDDL: &str = "D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FA;;;OW)";

    daemon::validate_windows_pipe_name(&paths.pipe_name).map_err(|e| e.to_string())?;
    let name = paths
        .pipe_name
        .as_str()
        .to_ns_name::<GenericWorkspaced>()
        .map_err(|e| format!("build daemon named pipe name: {e}"))?;
    let sddl = U16CString::from_str(OWNER_ONLY_PIPE_SDDL)
        .map_err(|e| format!("build daemon named pipe security descriptor string: {e}"))?;
    let security_descriptor = SecurityDescriptor::deserialize(&sddl)
        .map_err(|e| format!("build daemon named pipe security descriptor: {e}"))?;
    let listener = ListenerOptions::new()
        .name(name)
        .nonblocking(ListenerNonblockingMode::Accept)
        .security_descriptor(security_descriptor)
        .create_sync()
        .map_err(|e| format!("bind daemon Windows named pipe: {e}"))?;
    Ok(LocalDaemonListener::WindowsNamedPipe(listener))
}

impl Read for LocalDaemonStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.read(buf),
            #[cfg(unix)]
            Self::Unix(stream) => stream.read(buf),
            #[cfg(windows)]
            Self::WindowsNamedPipe(stream) => stream.read(buf),
        }
    }
}

impl Write for LocalDaemonStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.write(buf),
            #[cfg(unix)]
            Self::Unix(stream) => stream.write(buf),
            #[cfg(windows)]
            Self::WindowsNamedPipe(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.flush(),
            #[cfg(unix)]
            Self::Unix(stream) => stream.flush(),
            #[cfg(windows)]
            Self::WindowsNamedPipe(stream) => stream.flush(),
        }
    }
}

pub(crate) fn daemon_run(
    store: &str,
    addr_file: &str,
    pid_file: &str,
    lock_file: &str,
    transport: &str,
) -> Result<(), String> {
    let transport = DaemonTransportSelection::parse(transport)?;
    let paths = daemon::paths(store).map_err(|e| e.to_string())?;
    let runtime_lock =
        daemon_lock_runtime_file(std::path::Path::new(lock_file)).map_err(|e| e.to_string())?;
    daemon_write_runtime_lock(&runtime_lock, &paths).map_err(|e| e.to_string())?;
    let fs = FileStore::open_read(store).map_err(|e| e.to_string())?;
    let coordinator = fs.lock_coordinator().map_err(|e| e.to_string())?;
    let (kv_loom, kv_unavailable) = match daemon_kv_loom(store) {
        Ok(loom) => (Some(loom), None),
        Err(e) => (None, Some(daemon_kv_unavailable_error(e))),
    };
    let listener = LocalDaemonListener::bind(&paths, transport)?;
    let transport = listener.transport();
    #[cfg(feature = "serve")]
    let hosted_listeners = start_hosted_listeners(store)?;
    let store_id = paths.store_id.clone();
    let mut runtime = DaemonRuntime {
        store: store.to_string(),
        store_id,
        transport,
        coordinator,
        kv_loom,
        kv_unavailable,
        sessions: std::collections::BTreeSet::new(),
        pins: std::collections::BTreeMap::new(),
        authority_replication_next: std::collections::BTreeMap::new(),
        maintenance_next_ms: 0,
        #[cfg(feature = "serve")]
        hosted_listeners,
        #[cfg(feature = "serve")]
        drive_policy_next_ms: 0,
        #[cfg(feature = "serve")]
        reference_reconcile_next_ms: 0,
    };
    std::fs::write(addr_file, listener.addr_file_contents(&paths)?)
        .map_err(|e| format!("write daemon address file: {e}"))?;
    daemon::align_runtime_artifact_owner(std::path::Path::new(addr_file), "address", &paths)
        .map_err(|e| e.to_string())?;
    std::fs::write(pid_file, std::process::id().to_string())
        .map_err(|e| format!("write daemon pid file: {e}"))?;
    daemon::align_runtime_artifact_owner(std::path::Path::new(pid_file), "pid", &paths)
        .map_err(|e| e.to_string())?;
    loop {
        runtime.reconcile_authority_replication()?;
        #[cfg(feature = "serve")]
        runtime.reconcile_hosted_listeners()?;
        runtime.reconcile_drive_policy_workers()?;
        runtime.reconcile_reference_workers()?;
        runtime.reconcile_store_maintenance()?;
        let mut stream = match listener.accept(&paths)? {
            Some(stream) => stream,
            None => {
                std::thread::sleep(std::time::Duration::from_millis(250));
                continue;
            }
        };
        let mut request = String::new();
        stream
            .read_to_string(&mut request)
            .map_err(|e| format!("read daemon request: {e}"))?;
        let response = runtime.handle(&request);
        let request_head = request.trim_end().split('\t').next();
        let should_stop = matches!(request_head, Some("stop" | "stop-force"))
            && response.starts_with("stopped\t");
        stream
            .write_all(response.as_bytes())
            .map_err(|e| format!("write daemon response: {e}"))?;
        if should_stop {
            let _ = std::fs::remove_file(addr_file);
            let _ = std::fs::remove_file(pid_file);
            let _ = std::fs::remove_file(lock_file);
            let _ = std::fs::remove_file(&paths.sock_file);
            return Ok(());
        }
    }
}

fn daemon_lock_runtime_file(lock_file: &std::path::Path) -> std::io::Result<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_file)?;
    file.lock()?;
    Ok(file)
}

pub(crate) fn daemon_running_response(
    store: &str,
    store_id: &str,
    transport: daemon::DaemonTransport,
) -> String {
    format!(
        "running\tprotocol={}\ttransport={}\t{}\t{store}\tidentity={store_id}\n",
        daemon::PROTOCOL,
        transport.wire_name(),
        std::process::id()
    )
}

#[cfg(test)]
mod maintenance_tail_trim_tests {
    use super::*;

    fn temp_store(tag: &str) -> String {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("loomcli-{tag}-{}-{seq}.loom", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path.to_string_lossy().into_owned()
    }

    fn test_runtime(tag: &str) -> (String, DaemonRuntime) {
        let store = temp_store(tag);
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        let coordinator = fs.lock_coordinator().unwrap();
        drop(fs);
        let paths = daemon::paths(&store).unwrap();
        (
            store,
            DaemonRuntime {
                store: paths.store,
                store_id: paths.store_id,
                transport: daemon::DaemonTransport::TcpLoopback,
                coordinator,
                kv_loom: None,
                kv_unavailable: None,
                sessions: std::collections::BTreeSet::new(),
                pins: std::collections::BTreeMap::new(),
                authority_replication_next: std::collections::BTreeMap::new(),
                maintenance_next_ms: 0,
                #[cfg(feature = "serve")]
                hosted_listeners: std::collections::BTreeMap::new(),
                #[cfg(feature = "serve")]
                drive_policy_next_ms: 0,
                #[cfg(feature = "serve")]
                reference_reconcile_next_ms: 0,
            },
        )
    }

    #[test]
    fn daemon_runtime_incremental_maintenance_reports_tail_trim() {
        let (store, mut runtime) = test_runtime("maintenance-tail-trim-unit");
        let fs = FileStore::open_daemon_authorized(&store).unwrap();
        let live_payload = vec![0xA5; 800 * 1024];
        let live = fs.put(&live_payload).unwrap();
        let state = loom_core::ReachabilityMarkState {
            pinned: std::collections::BTreeSet::new(),
            marked: std::collections::BTreeSet::from([live]),
            queue: std::collections::VecDeque::new(),
            stream_roots: std::collections::VecDeque::new(),
            completed: true,
        };
        let epoch = fs
            .begin_reachability_mark_epoch(None, std::collections::BTreeSet::new(), state)
            .unwrap();
        fs.complete_reachability_mark_epoch(&epoch).unwrap();
        for i in 0..640usize {
            fs.put(format!("tail-dead-{i:04}").as_bytes()).unwrap();
        }
        fs.set_store_maintenance_policy(loom_store::StoreMaintenancePolicy {
            min_candidate_pages: 0,
            min_reusable_pages: 0,
            interval_ms: 1_000,
            backoff_ms: 2_000,
            max_segments: u64::MAX,
            max_pages: u64::MAX,
            full_compaction_enabled: false,
            ..loom_store::StoreMaintenancePolicy::default()
        })
        .unwrap();
        drop(fs);

        let response = runtime.handle("maintenance-run\tnone\tnone\n");
        assert!(response.starts_with("maintenance\treclaimed"));
        assert!(response.contains("tail_trim_pages="));
        let after = runtime.handle("maintenance-status\n");
        assert!(after.contains("tail_trim_attempted=true"));
    }

    #[test]
    fn daemon_runtime_maintenance_status_reports_shrink_policy() {
        let (_store, mut runtime) = test_runtime("maintenance-shrink-status-unit");
        let status = runtime.handle("maintenance-status\n");
        assert!(status.contains("tail_free_pages="));
        assert!(status.contains("tail_free_bytes="));
        assert!(status.contains("tail_trim_eligible=false"));
        assert!(status.contains("tail_compaction_eligible=false"));
        assert!(status.contains("full_compaction_required_for_shrink="));
        assert!(status.contains("tail_trim_enabled=true"));
        assert!(status.contains("tail_compaction_enabled=true"));
        assert!(status.contains("tail_compaction_max_pages="));
        assert!(status.contains("tail_compaction_max_objects="));
        assert!(status.contains("tail_compaction_max_bytes="));
    }

    #[test]
    fn daemon_runtime_reconciles_policy_gated_segment_gc() {
        let (store, mut runtime) = test_runtime("maintenance-auto-gc-unit");
        let fs = FileStore::open_daemon_authorized(&store).unwrap();
        let live_payload = vec![0xA5; 800 * 1024];
        let live = fs.put(&live_payload).unwrap();
        let state = loom_core::ReachabilityMarkState {
            pinned: std::collections::BTreeSet::new(),
            marked: std::collections::BTreeSet::from([live]),
            queue: std::collections::VecDeque::new(),
            stream_roots: std::collections::VecDeque::new(),
            completed: true,
        };
        let epoch = fs
            .begin_reachability_mark_epoch(None, std::collections::BTreeSet::new(), state)
            .unwrap();
        fs.complete_reachability_mark_epoch(&epoch).unwrap();
        for i in 0..640usize {
            fs.put(format!("auto-dead-{i:04}").as_bytes()).unwrap();
        }
        fs.set_store_maintenance_policy(loom_store::StoreMaintenancePolicy {
            min_candidate_pages: 0,
            min_reusable_pages: 0,
            interval_ms: 1_000,
            backoff_ms: 2_000,
            max_segments: u64::MAX,
            max_pages: u64::MAX,
            full_compaction_enabled: false,
            ..loom_store::StoreMaintenancePolicy::default()
        })
        .unwrap();
        drop(fs);

        runtime.reconcile_store_maintenance().unwrap();
        let fs = FileStore::open_read(&store).unwrap();
        let run_state = fs.store_maintenance_run_state().unwrap();
        assert!(run_state.last_run_ms.is_some());
        assert!(run_state.next_eligible_ms > 0);
        assert_eq!(run_state.last_skip_reason, None);
        assert_eq!(run_state.last_error, None);
        assert!(run_state.last_tail_trim_attempted);
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod tests {
    use super::*;
    use loom_core::IdentityPublicKeySpec;

    #[cfg(feature = "serve")]
    const TLS_TEST_CERT: &str = "-----BEGIN CERTIFICATE-----\nMIIDQzCCAiugAwIBAgIURUxzBqftfDWGZEu7oSwSouoBy2YwDQYJKoZIhvcNAQEL\nBQAwFDESMBAGA1UEAwwJbG9jYWxob3N0MB4XDTI2MDcwMTIzNDQzM1oXDTM2MDYy\nODIzNDQzM1owFDESMBAGA1UEAwwJbG9jYWxob3N0MIIBIjANBgkqhkiG9w0BAQEF\nAAOCAQ8AMIIBCgKCAQEAtdG4yL83GeKMIkDao6rLD0QaiVNUY4AhadsWOyewD0kv\nJtznbtTj8v7Jhq0wSNinPH96Fzh0zd8hNUMZm4kmQq0TWKcIwcmXIX//VCPPhIqQ\npM/K5AKvdXl86ERxCsfR+SGnCNujFENWq2Dr2TfeCwqhLR/s7YImAo8if3H/mKy1\nlMQ54/VsbbcdSDiZlaEaJ+MgzzXnI3NPpTS5R2jOF8VXx8OQ2+zExcsshPr06r0o\n3vVWQW6qOu4AanMusF+MReQrmhmG7NqTrmvJkb9h6jx7SvD/rg6MuSCCzccfbX3J\n3wFn2yBxSYTgZoc4PxRr/kU4iS8v9LzDxJ900irNjwIDAQABo4GMMIGJMB0GA1Ud\nDgQWBBTaBWj6D85uTDycHi7x08dKMkJYIDAfBgNVHSMEGDAWgBTaBWj6D85uTDyc\nHi7x08dKMkJYIDAUBgNVHREEDTALgglsb2NhbGhvc3QwDAYDVR0TAQH/BAIwADAO\nBgNVHQ8BAf8EBAMCBaAwEwYDVR0lBAwwCgYIKwYBBQUHAwEwDQYJKoZIhvcNAQEL\nBQADggEBAGbLvDSnoWG1LZ9iWnq2euLwYkI2cP3i4sMcygbkzdlWUdSZznQkldhS\nP1J0e5af6jOy0fvcTtchd9uoN55E68LmnCmEUN6+ObEDFXNq+9fRSOeQbmhQuCpu\ngLVFn/cRFnJiC5S9XiNGM3EX4b2uZDs3staWV65yHIIVhMK0RSjQzhc0Pkkz6OrM\nXh7VxtAOTuCuKnaEkfvWtu9Cj0F7SSA4vTcZ4XXr1fHu9cYn1a/OpiX4ks3o+cv2\njlKK/0C1JSJLvROjkaTcuD8AAUjsa95X9xb86UXBCYoVYqk4YrQYxNwG73Pw3ISk\nuP98zn2qjNjq48siIZlZ2WrxziSXlGg=\n-----END CERTIFICATE-----\n";

    #[cfg(feature = "serve")]
    const TLS_TEST_KEY: &str = "-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC10bjIvzcZ4owi\nQNqjqssPRBqJU1RjgCFp2xY7J7APSS8m3Odu1OPy/smGrTBI2Kc8f3oXOHTN3yE1\nQxmbiSZCrRNYpwjByZchf/9UI8+EipCkz8rkAq91eXzoRHEKx9H5IacI26MUQ1ar\nYOvZN94LCqEtH+ztgiYCjyJ/cf+YrLWUxDnj9Wxttx1IOJmVoRon4yDPNecjc0+l\nNLlHaM4XxVfHw5Db7MTFyyyE+vTqvSje9VZBbqo67gBqcy6wX4xF5CuaGYbs2pOu\na8mRv2HqPHtK8P+uDoy5IILNxx9tfcnfAWfbIHFJhOBmhzg/FGv+RTiJLy/0vMPE\nn3TSKs2PAgMBAAECggEAAraYfVZtKhO5Z6F0IdEgqp+njbkSn1GALiY41LUo6XF8\nJKZTRdIjfLLUqw/Wkp+5DywP1FzhUVktE7Bmp22KhqAyk7YaaVnqyXMxij3mkOHI\nes0nB/QKTkR72rN6xGBq/77C0V0Ft/2xQ2l0247730pPUW8SiBDTJGKibyhyWgLj\nWLAoj8AcUc2L6EIQhCbNsZS6mYWBmdMNv87oTGVxdNNxCM9wYPej+ZCY9ZBm0CeN\n3bG/7OKugDXrwsDq6kX5KjHvQKvsX8Yo7cQDlLjNoeQY0iQGbhugeoPE7eAylNIt\ntUKwGguKMU+5Xm3DJUMtgfj3Fz7C07V9i6IN5G85wQKBgQDhwkGgO3QK1sVxzFLW\nEnEG7JDFWx7Q7bmlwcCUhlrL2SKcQzSxHqAsAMiuGh4eU9fWxDA93vh+gLW6HGOS\nI3FCOmESU+YjlSHvJCd0+UHGaNxCXJs+wtXvhbfXdmwmj+TL8HKxP0ztenFdKf77\n3xkMf7r0Xok/xOwnihCl2FFLnwKBgQDOLK9LrZCLfl+vMyLQBTK7tCTDkWEh5wa7\ndmPi+SOC9aG41ZRSSIy+JbXsUFSf/4DMzenXTVyt2YU9OoNlRPn0JOBt7OUcOTes\naY/jUsMkfDYnl7uwp6woa1xAHPMyL5dIaw0MRhAuQLOH6HkqTp8IShCIKzXi+c88\nIYimiYY4EQKBgDwX/moNiV0dQF+DWQV80TNbo0m1cKWCsikqQv4GKYMboHfh99Ox\n6EbuSnz1nNDL1qdnf8PoZ1MdJcKNrf+Hia1sZsx/IsKT/v1uLUaY1uZeoUrU5co2\nCMaCXKZw8mbtZKTYs171D6AjOKvo8uPOxhcqpPRJedVMsOPxf271/uXXAoGAWXRT\n3n8BDzUWqPqD6UPIHl7r8JqcTUxixGV6s1krij+vGnY4s3bc8geEpnK4NO9z3+ib\nxBnB04BkaguARSknVkHFyowVYCiHOlxW3Ofk2Wi3SnhwLBakAKmMThkBf83cUsR3\n1dJ0ZM0X2CkKoUuZfsw73gj5iXCf9NQL6U4UGTECgYEAjF59zSzSQ9t3iBuRZ22n\nVXWPTvA878AZ+bdPNUuboQS99qqY1hROyAPgSds7GJw+TIq9qxJuv3H/XyKTuQ9a\n/3OFaM4+HjcaOxy2ZUB1K5PkC/7xyDB/ql//G7S062hiIcXMP+2DK+Cnef6Uh0Xu\nWX4dtn5wzPizsbGoDilhMMg=\n-----END PRIVATE KEY-----\n";

    #[cfg(feature = "serve")]
    fn save_test_certificate_bundle(store: &str, name: &str, trust_bundle_pem: Option<Vec<u8>>) {
        let fs = FileStore::open_daemon_authorized(store).unwrap();
        let record = fs
            .certificate_bundle_record(
                name,
                TLS_TEST_CERT.as_bytes().to_vec(),
                TLS_TEST_KEY.as_bytes().to_vec(),
                trust_bundle_pem,
            )
            .unwrap();
        fs.save_certificate_bundle_audited(
            &record,
            None,
            "certificate.bundle.import.force",
            Some(&format!("name={name}")),
            true,
        )
        .unwrap();
    }

    fn temp_store(tag: &str) -> String {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("loomcli-{tag}-{}-{seq}.loom", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path.to_string_lossy().into_owned()
    }

    #[cfg(feature = "mcp")]
    fn mcp_locator_context(
        tag: &str,
        contexts_toml: &str,
    ) -> (std::path::PathBuf, crate::locator_cx::LocatorContext) {
        let dir =
            std::env::temp_dir().join(format!("loomcli-mcp-locator-{}-{tag}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join(".loom")).expect("create project .loom dir");
        std::fs::write(dir.join(".loom").join("contexts.toml"), contexts_toml)
            .expect("write contexts.toml");
        let cx =
            crate::locator_cx::LocatorContext::from_globals(Some(dir.clone()), Vec::new(), None)
                .expect("build locator context");
        (dir, cx)
    }

    // A remote URL launches remote MCP (not `--stateless`); with `--stateless` it is refused immediately.
    #[cfg(feature = "mcp")]
    #[test]
    fn mcp_remote_url_launches_and_rejects_stateless() {
        let (dir, cx) = mcp_locator_context("url", "");
        assert!(
            matches!(
                resolve_mcp_target(&cx, "https://loom.example.com/prod", false)
                    .expect("a remote URL resolves to a remote launch"),
                McpLaunchTarget::Remote(_)
            ),
            "a remote URL without --stateless should launch remote MCP"
        );
        let err = resolve_mcp_target(&cx, "https://loom.example.com/prod", true)
            .expect_err("remote + --stateless must be rejected");
        assert!(err.contains("--stateless"), "unexpected error: {err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    // A remote context launches remote MCP; with `--stateless` it is refused immediately.
    #[cfg(feature = "mcp")]
    #[test]
    fn mcp_remote_context_launches_and_rejects_stateless() {
        let (dir, cx) = mcp_locator_context(
            "context",
            "[cli]\ncurrent_context = \"prod\"\n\n[contexts.prod]\ntarget = \"https://loom.example.com/prod\"\n",
        );
        assert!(
            matches!(
                resolve_mcp_target(&cx, "context", false)
                    .expect("a remote context resolves to remote"),
                McpLaunchTarget::Remote(_)
            ),
            "a remote context without --stateless should launch remote MCP"
        );
        let err = resolve_mcp_target(&cx, "context", true)
            .expect_err("remote context + --stateless must be rejected");
        assert!(err.contains("--stateless"), "unexpected error: {err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(feature = "mcp")]
    #[test]
    fn mcp_accepts_local_path_locator() {
        let (dir, cx) = mcp_locator_context("local", "");
        assert!(
            matches!(
                resolve_mcp_target(&cx, "./local.loom", false).expect("a local path is accepted"),
                McpLaunchTarget::Local
            ),
            "a local path should be a local launch"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    fn test_runtime(tag: &str) -> (String, DaemonRuntime) {
        let store = temp_store(tag);
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        let coordinator = fs.lock_coordinator().unwrap();
        drop(fs);
        let paths = daemon::paths(&store).unwrap();
        (
            store,
            DaemonRuntime {
                store: paths.store,
                store_id: paths.store_id,
                transport: daemon::DaemonTransport::TcpLoopback,
                coordinator,
                kv_loom: None,
                kv_unavailable: None,
                sessions: std::collections::BTreeSet::new(),
                pins: std::collections::BTreeMap::new(),
                authority_replication_next: std::collections::BTreeMap::new(),
                maintenance_next_ms: 0,
                #[cfg(feature = "serve")]
                hosted_listeners: std::collections::BTreeMap::new(),
                #[cfg(feature = "serve")]
                drive_policy_next_ms: 0,
                #[cfg(feature = "serve")]
                reference_reconcile_next_ms: 0,
            },
        )
    }

    #[test]
    fn daemon_runtime_reports_and_runs_store_maintenance() {
        let (store, mut runtime) = test_runtime("maintenance-runtime");
        let status = runtime.handle("maintenance-status\n");
        assert!(status.contains("maintenance\teligible=true"));
        assert!(status.contains("reason=mark_epoch_missing"));
        assert!(status.contains("tail_free_pages="));
        assert!(status.contains("tail_trim_attempted=false"));
        assert!(status.contains("tail_compaction_eligible=false"));
        assert!(status.contains("tail_trim_enabled=true"));
        assert!(status.contains("tail_compaction_enabled=true"));

        let loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        loom.store()
            .set_store_maintenance_policy(loom_store::StoreMaintenancePolicy {
                min_candidate_pages: 0,
                min_reusable_pages: 0,
                interval_ms: 1_000,
                backoff_ms: 2_000,
                max_segments: 1,
                max_pages: 64,
                full_compaction_enabled: true,
                ..loom_store::StoreMaintenancePolicy::default()
            })
            .unwrap();
        drop(loom);

        let response = runtime.handle("maintenance-run\tnone\tnone\n");
        assert!(response.starts_with("maintenance\tcompacted"));
        let after = runtime.handle("maintenance-status\n");
        assert!(after.contains("maintenance_policy\tmin_candidate_pages=0"));
    }

    #[test]
    fn daemon_runtime_incremental_maintenance_reports_tail_trim() {
        let (store, mut runtime) = test_runtime("maintenance-tail-trim");
        let fs = FileStore::open_daemon_authorized(&store).unwrap();
        let live_payload = vec![0xA5; 800 * 1024];
        let live = fs.put(&live_payload).unwrap();
        let state = loom_core::ReachabilityMarkState {
            pinned: std::collections::BTreeSet::new(),
            marked: std::collections::BTreeSet::from([live]),
            queue: std::collections::VecDeque::new(),
            stream_roots: std::collections::VecDeque::new(),
            completed: true,
        };
        let epoch = fs
            .begin_reachability_mark_epoch(None, std::collections::BTreeSet::new(), state)
            .unwrap();
        fs.complete_reachability_mark_epoch(&epoch).unwrap();
        for i in 0..640usize {
            fs.put(format!("tail-dead-{i:04}").as_bytes()).unwrap();
        }
        fs.set_store_maintenance_policy(loom_store::StoreMaintenancePolicy {
            min_candidate_pages: 0,
            min_reusable_pages: 0,
            interval_ms: 1_000,
            backoff_ms: 2_000,
            max_segments: u64::MAX,
            max_pages: u64::MAX,
            full_compaction_enabled: false,
            ..loom_store::StoreMaintenancePolicy::default()
        })
        .unwrap();
        drop(fs);

        let response = runtime.handle("maintenance-run\tnone\tnone\n");
        assert!(response.starts_with("maintenance\treclaimed"));
        assert!(response.contains("tail_trim_pages="));
        let after = runtime.handle("maintenance-status\n");
        assert!(after.contains("tail_trim_attempted=true"));
    }

    #[cfg(feature = "serve")]
    #[test]
    fn attached_session_coalesces_reference_reconciliation() {
        let (store, mut runtime) = test_runtime("reference-reconcile-signal");
        runtime.reference_reconcile_next_ms = 42;
        runtime.sessions.insert("mcp-session".to_string());
        assert_eq!(runtime.handle("reference-reconcile\tmcp-session\n"), "ok\n");
        assert_eq!(runtime.reference_reconcile_next_ms, 0);
        assert!(
            runtime
                .handle("reference-reconcile\tunknown\n")
                .starts_with("error\t")
        );
        let _ = std::fs::remove_file(store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_reference_resolver_reconciles_a_ticket_target() {
        let store = temp_store("reference-resolver");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        let mut loom = Loom::new(fs);
        let workspace = WorkspaceId::v4_from_bytes([37; 16]);
        loom.registry_mut()
            .create(FacetKind::Vcs, Some("studio"), workspace)
            .unwrap();
        loom_tickets::create_project(&mut loom, workspace, "studio", "core", "CORE", "Core", None)
            .unwrap();
        let fields = serde_json::json!({});
        loom_tickets::create_ticket(
            &mut loom,
            workspace,
            loom_tickets::TicketCreateRequest {
                workspace_id: "studio",
                project_id: "core",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &fields,
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        let candidate = loom_substrate::refs::UnresolvedReference::new(
            loom_substrate::refs::UnresolvedReferenceInput {
                candidate_id: "candidate-1".to_string(),
                source: loom_substrate::refs::ReferenceSource::new(
                    "tickets",
                    "studio",
                    "eng-1",
                    "description",
                )
                .unwrap(),
                source_operation_id: "ticket.updated:eng-1".to_string(),
                source_root: Digest::hash(Algo::Blake3, b"source"),
                alias_text: "CORE-1".to_string(),
                relation: "refers_to".to_string(),
                span_start: 0,
                span_end: 6,
                evidence: "CORE-1".to_string(),
                next_attempt_ms: 1,
            },
        )
        .unwrap();
        loom_reference::enqueue(&mut loom, workspace, &candidate).unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let run = reconcile_references_once(&store, 1, 100).unwrap();
        assert_eq!(run.processed, 1);
        assert_eq!(run.pending, 0);
        assert_eq!(run.resolved, 1);
        let _ = std::fs::remove_file(store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_reference_resolver_reconciles_a_chat_channel_handle() {
        let store = temp_store("reference-resolver-chat");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        let mut loom = Loom::new(fs);
        let workspace = WorkspaceId::v4_from_bytes([38; 16]);
        loom.registry_mut()
            .create(FacetKind::Vcs, Some("studio"), workspace)
            .unwrap();
        let mut directory = loom_substrate::chat::ChatChannelDirectory::new("studio").unwrap();
        let channel = directory
            .create_channel(WorkspaceId::v4_from_bytes([39; 16]), "general", "General")
            .unwrap();
        let directory_path =
            String::from_utf8(loom_substrate::chat::chat_channel_directory_key("studio").unwrap())
                .unwrap();
        let parent = directory_path.rsplit_once('/').unwrap().0;
        loom.create_directory_reserved(workspace, parent, true)
            .unwrap();
        loom.write_file_reserved(
            workspace,
            &directory_path,
            &directory.encode().unwrap(),
            0o100644,
        )
        .unwrap();
        let candidate = loom_substrate::refs::UnresolvedReference::new(
            loom_substrate::refs::UnresolvedReferenceInput {
                candidate_id: "candidate-chat-1".to_string(),
                source: loom_substrate::refs::ReferenceSource::new(
                    "chat",
                    format!("studio:{}", channel.id),
                    "message-1",
                    "body",
                )
                .unwrap(),
                source_operation_id: "studio:message-1:1".to_string(),
                source_root: Digest::hash(Algo::Blake3, b"source"),
                alias_text: "#general".to_string(),
                relation: "refers_to".to_string(),
                span_start: 0,
                span_end: 8,
                evidence: "#general".to_string(),
                next_attempt_ms: 1,
            },
        )
        .unwrap();
        loom_reference::enqueue(&mut loom, workspace, &candidate).unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let run = reconcile_references_once(&store, 1, 100).unwrap();
        assert_eq!(run.processed, 1);
        assert_eq!(run.pending, 0);
        assert_eq!(run.resolved, 1);
        let loom = loom_store::open_loom_daemon_authorized_unlocked(&store, None).unwrap();
        let index = loom_reference::load_index(&loom, workspace)
            .unwrap()
            .unwrap();
        assert_eq!(
            index
                .inbound(
                    &loom_substrate::refs::EntityRef::parse(&format!("channel:{}", channel.id))
                        .unwrap()
                )
                .len(),
            1
        );
        let _ = std::fs::remove_file(store);
    }

    fn test_authenticated_runtime(tag: &str) -> (String, DaemonRuntime, WorkspaceId, WorkspaceId) {
        let store = temp_store(tag);
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        let root = WorkspaceId::v4_from_bytes([9; 16]);
        let user = WorkspaceId::v4_from_bytes([10; 16]);
        let mut identity = IdentityStore::new(root);
        identity
            .set_passphrase(root, "root-pass", b"12345678")
            .unwrap();
        identity
            .add_principal(user, "user", PrincipalKind::User)
            .unwrap();
        identity
            .set_passphrase(user, "user-pass", b"abcdefgh")
            .unwrap();
        let mut acl = AclStore::new();
        acl.allow(AclSubject::Principal(root), None, None, [AclRight::Admin])
            .unwrap();
        fs.save_identity_store(&identity).unwrap();
        fs.save_acl_store(&acl).unwrap();
        let coordinator = fs.lock_coordinator().unwrap();
        drop(fs);
        let paths = daemon::paths(&store).unwrap();
        (
            store,
            DaemonRuntime {
                store: paths.store,
                store_id: paths.store_id,
                transport: daemon::DaemonTransport::TcpLoopback,
                coordinator,
                kv_loom: None,
                kv_unavailable: None,
                sessions: std::collections::BTreeSet::new(),
                pins: std::collections::BTreeMap::new(),
                authority_replication_next: std::collections::BTreeMap::new(),
                maintenance_next_ms: 0,
                #[cfg(feature = "serve")]
                hosted_listeners: std::collections::BTreeMap::new(),
                #[cfg(feature = "serve")]
                drive_policy_next_ms: 0,
                #[cfg(feature = "serve")]
                reference_reconcile_next_ms: 0,
            },
            root,
            user,
        )
    }

    fn request_auth_fields(principal: WorkspaceId, passphrase: &str, session: &str) -> String {
        format!(
            "auth-principal={principal}\tauth-passphrase-hex={}\tauth-session={session}",
            daemon::hex_encode(passphrase.as_bytes())
        )
    }

    fn signed_authority_source(source: &str, destination: &str) -> (WorkspaceId, WorkspaceId) {
        let source_store = FileStore::create_with_profile(source, Algo::Blake3).unwrap();
        let destination_store = FileStore::create_with_profile(destination, Algo::Blake3).unwrap();
        let root = WorkspaceId::v4_from_bytes([33; 16]);
        let next_authority = WorkspaceId::v4_from_bytes([34; 16]);
        let key_id = WorkspaceId::v4_from_bytes([35; 16]);
        let signing_key = p256::ecdsa::SigningKey::from_slice(&[7; 32]).unwrap();
        let verifying_key = signing_key.verifying_key();
        let public_key = verifying_key.to_encoded_point(false).as_bytes().to_vec();
        let mut source_identity = IdentityStore::new(root);
        source_identity
            .add_principal(next_authority, "replica-authority", PrincipalKind::Service)
            .unwrap();
        source_identity
            .add_public_key(
                root,
                IdentityPublicKeySpec {
                    id: key_id,
                    label: "authority-key".to_string(),
                    algorithm: loom_core::IDENTITY_AUTHORITY_HANDOFF_ALG_ES256.to_string(),
                    public_key,
                },
            )
            .unwrap();
        let payload = loom_core::identity_authority_handoff_payload(root, next_authority, 1, None);
        let signature: p256::ecdsa::Signature =
            p256::ecdsa::signature::Signer::sign(&signing_key, &payload);
        let signed_record = loom_core::identity_authority_handoff_record(
            root,
            next_authority,
            1,
            None,
            loom_core::IDENTITY_AUTHORITY_HANDOFF_ALG_ES256,
            key_id.as_bytes(),
            signature.to_bytes().as_slice(),
        )
        .unwrap();
        source_identity
            .apply_verified_authority_handoff(
                loom_core::IdentityAuthorityHandoff {
                    from: root,
                    to: next_authority,
                    generation: 1,
                    head: None,
                    signed_record,
                },
                true,
            )
            .unwrap();
        source_store.save_identity_store(&source_identity).unwrap();
        destination_store
            .save_identity_store(&IdentityStore::new(root))
            .unwrap();
        (root, next_authority)
    }

    #[test]
    fn daemon_reconciles_authority_replication_policy() {
        let source = temp_store("authority-replication-source");
        let mirror = temp_store("authority-replication-mirror");
        let (_root, next_authority) = signed_authority_source(&source, &mirror);
        let fs = FileStore::open_daemon_authorized(&mirror).unwrap();
        let coordinator = fs.lock_coordinator().unwrap();
        let mut policy = FileStore::authority_replication_policy("office", &source, true).unwrap();
        policy.interval_ms = Some(60_000);
        fs.save_authority_replication_policy_audited(
            &policy,
            None,
            "authority.replication.configure",
            Some("id=office"),
        )
        .unwrap();
        drop(fs);
        let paths = daemon::paths(&mirror).unwrap();
        let mut runtime = DaemonRuntime {
            store: paths.store.clone(),
            store_id: paths.store_id,
            transport: daemon::DaemonTransport::TcpLoopback,
            coordinator,
            kv_loom: None,
            kv_unavailable: None,
            sessions: std::collections::BTreeSet::new(),
            pins: std::collections::BTreeMap::new(),
            authority_replication_next: std::collections::BTreeMap::new(),
            maintenance_next_ms: 0,
            #[cfg(feature = "serve")]
            hosted_listeners: std::collections::BTreeMap::new(),
            #[cfg(feature = "serve")]
            drive_policy_next_ms: 0,
            #[cfg(feature = "serve")]
            reference_reconcile_next_ms: 0,
        };

        runtime.reconcile_authority_replication().unwrap();
        let mirror_store = FileStore::open_read(&mirror).unwrap();
        let mirror_identity = mirror_store.identity_store().unwrap().unwrap();
        assert_eq!(mirror_identity.authority_state().authority, next_authority);
        assert_eq!(mirror_identity.authority_state().generation, 1);
        let stored = mirror_store
            .authority_replication_policy_by_id("office")
            .unwrap()
            .unwrap();
        assert!(stored.last_success_ms.is_some());
        assert_eq!(stored.last_failure_ms, None);
        assert_eq!(stored.last_error, None);
        assert!(runtime.authority_replication_next.contains_key("office"));
        assert!(mirror_store.audit_records().unwrap().iter().any(|record| {
            record.action == "authority.replication.pull"
                && record
                    .target
                    .as_deref()
                    .is_some_and(|target| target.contains("applied=true"))
        }));
        let _ = std::fs::remove_file(&source);
        let _ = std::fs::remove_file(&mirror);
    }

    #[cfg(unix)]
    #[test]
    fn daemon_listener_uses_unix_socket_when_peer_credentials_are_supported() {
        let store = temp_store("daemon-unix-listener");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let paths = daemon::paths(&store).unwrap();
        let listener = LocalDaemonListener::bind(&paths, DaemonTransportSelection::Native).unwrap();
        if daemon::unix_peer_credentials_supported() {
            assert_eq!(listener.transport(), daemon::DaemonTransport::UnixSocket);
            assert!(paths.sock_file.exists());
            let envelope = listener.addr_file_contents(&paths).unwrap();
            assert!(envelope.contains("transport=unix_socket\n"));
            assert!(envelope.contains("security=peer_credentials\n"));
            assert!(envelope.contains(&format!("addr={}\n", paths.sock_file.display())));
        } else {
            assert_eq!(listener.transport(), daemon::DaemonTransport::TcpLoopback);
        }
        drop(listener);
        let _ = std::fs::remove_file(paths.sock_file);
        let _ = std::fs::remove_file(store);
    }

    #[test]
    fn daemon_transport_selection_accepts_native_and_explicit_tcp() {
        assert_eq!(
            DaemonTransportSelection::parse("native").unwrap(),
            DaemonTransportSelection::Native
        );
        assert_eq!(
            DaemonTransportSelection::parse("tcp").unwrap(),
            DaemonTransportSelection::TcpLoopback
        );
        assert_eq!(
            DaemonTransportSelection::parse("tcp-loopback").unwrap(),
            DaemonTransportSelection::TcpLoopback
        );
        assert!(DaemonTransportSelection::parse("unix_socket").is_err());
    }

    #[test]
    fn daemon_transport_evidence_lines_include_security_profile() {
        let tcp = daemon::transport_capabilities()
            .into_iter()
            .find(|capability| capability.transport == daemon::DaemonTransport::TcpLoopback)
            .unwrap();
        assert_eq!(daemon_transport_profile(tcp.transport), "degraded");
        assert_eq!(
            daemon_transport_capability_line(&tcp),
            "daemon_transport_capability\ttransport=tcp\tstatus=degraded\tsecurity=degraded_loopback\treason=portable fallback; does not authenticate hostile same-user peers"
        );
        let status = daemon::DaemonStatus {
            transport: daemon::DaemonTransport::TcpLoopback,
            pid: "123".to_string(),
            store: "/tmp/example.loom".to_string(),
            store_id: "identity".to_string(),
            sessions: 0,
            pins: 0,
            permanent_pins: 0,
            leased_pins: 0,
            pin_details: Vec::new(),
        };
        assert_eq!(
            daemon_status_line(&status),
            "running\tprotocol=1\ttransport=tcp\tsecurity=degraded_loopback\tprofile=degraded\t123\t/tmp/example.loom"
        );
    }

    #[test]
    fn daemon_listener_uses_tcp_loopback_when_explicitly_selected() {
        let store = temp_store("daemon-tcp-listener");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let paths = daemon::paths(&store).unwrap();
        let listener =
            LocalDaemonListener::bind(&paths, DaemonTransportSelection::TcpLoopback).unwrap();
        assert_eq!(listener.transport(), daemon::DaemonTransport::TcpLoopback);
        let envelope = listener.addr_file_contents(&paths).unwrap();
        assert!(envelope.contains("transport=tcp\n"));
        assert!(envelope.contains("security=degraded_loopback\n"));
        drop(listener);
        let _ = std::fs::remove_file(store);
    }

    #[test]
    fn daemon_doctor_reports_runtime_artifact_state() {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "loomcli-daemon-artifact-{}-{}",
            std::process::id(),
            seq
        ));
        let _ = std::fs::remove_file(&path);
        assert_eq!(runtime_artifact_state(&path), "absent");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(runtime_artifact_state(&path), "present\tbytes=3");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn daemon_runtime_lock_distinguishes_busy_and_stale_startup() {
        let store = temp_store("daemon-runtime-lock");
        FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        let paths = daemon::paths(&store).unwrap();
        let held = daemon_try_runtime_lock(&paths, true).unwrap().unwrap();
        daemon_write_runtime_lock(&held, &paths).unwrap();
        assert!(matches!(
            daemon_existing_runtime_lock_state(&paths).unwrap(),
            RuntimeLockState::Busy
        ));
        drop(held);
        match daemon_existing_runtime_lock_state(&paths).unwrap() {
            RuntimeLockState::Acquired(lock) => drop(lock),
            RuntimeLockState::Busy => panic!("released daemon runtime lock is still busy"),
            RuntimeLockState::Missing => panic!("daemon runtime lock file is missing"),
        }
        daemon_cleanup_files(&paths);
        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn leased_pins_expire_but_permanent_pins_block_stop() {
        let (store, mut runtime) = test_runtime("daemon-pin-lease");
        assert_eq!(
            runtime.handle("pin-add\tmanual\n"),
            "pinned\tmanual\tpins=1\n"
        );
        assert_eq!(
            runtime.handle("stop\n"),
            "error\tdaemon has 1 live pin(s)\n"
        );
        assert_eq!(
            runtime.handle("pin-remove\tmanual\n"),
            "unpinned\tmanual\tpins=0\n"
        );
        assert_eq!(
            runtime.handle("pin-add\tmount\t100000\t0\n"),
            "pinned\tmount\tpins=1\n"
        );
        assert_eq!(
            runtime.handle("stop\n"),
            format!(
                "stopped\t{}\t{}\tforce=false\thard=false\twait_ms=30000\tpins=0\tlisteners=0\ttimed_out=0\n",
                std::process::id(),
                runtime.store
            )
        );
        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn live_leased_pin_blocks_stop_until_it_expires() {
        let (store, mut runtime) = test_runtime("daemon-pin-live");
        assert!(
            runtime
                .handle("pin-add\tmount\t100000\t")
                .starts_with("error\t")
        );
        assert_eq!(
            runtime.handle(&format!("pin-add\tmount\t100000\t{}\n", now_ms())),
            "pinned\tmount\tpins=1\n"
        );
        assert_eq!(
            runtime.handle("stop\n"),
            "error\tdaemon has 1 live pin(s)\n"
        );
        runtime.pins.insert(
            "mount".to_string(),
            PinLease {
                deadline_ms: Some(0),
            },
        );
        assert_eq!(
            runtime.handle("status\n"),
            daemon_running_response(&runtime.store, &runtime.store_id, runtime.transport)
                .trim_end()
                .to_string()
                + "\tsessions=0\tpins=0\tpermanent_pins=0\tleased_pins=0\n"
        );
        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn status_reports_permanent_and_leased_pins() {
        let (store, mut runtime) = test_runtime("daemon-pin-status");
        assert_eq!(
            runtime.handle("pin-add\tmanual\n"),
            "pinned\tmanual\tpins=1\n"
        );
        let clock = now_ms();
        let lease_ms = 60_000;
        let deadline = clock + lease_ms;
        assert_eq!(
            runtime.handle(&format!("pin-add\tmount\t{lease_ms}\t{clock}\n")),
            "pinned\tmount\tpins=2\n"
        );
        let expected_status = daemon_running_response(
            &runtime.store,
            &runtime.store_id,
            runtime.transport,
        )
        .trim_end()
        .to_string()
            + &format!(
                "\tsessions=0\tpins=2\tpermanent_pins=1\tleased_pins=1\tpin=permanent:6d616e75616c\tpin=leased:{deadline}:6d6f756e74\n"
            );
        assert_eq!(runtime.handle("status\n"), expected_status);
        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn force_stop_overrides_live_pins() {
        let (store, mut runtime) = test_runtime("daemon-pin-force");
        assert_eq!(
            runtime.handle("pin-add\tmanual\n"),
            "pinned\tmanual\tpins=1\n"
        );
        assert_eq!(
            runtime.handle("stop\n"),
            "error\tdaemon has 1 live pin(s)\n"
        );
        assert_eq!(
            runtime.handle("stop-force\n"),
            format!(
                "stopped\t{}\t{}\tforce=true\thard=false\twait_ms=30000\tpins=1\tlisteners=0\ttimed_out=0\n",
                std::process::id(),
                runtime.store
            )
        );
        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn stop_accepts_wait_and_hard_policy_fields() {
        let (store, mut runtime) = test_runtime("daemon-stop-policy");
        assert_eq!(
            runtime.handle("stop\twait-ms=250\n"),
            format!(
                "stopped\t{}\t{}\tforce=false\thard=false\twait_ms=250\tpins=0\tlisteners=0\ttimed_out=0\n",
                std::process::id(),
                runtime.store
            )
        );
        let _ = std::fs::remove_file(&store);

        let (store, mut runtime) = test_runtime("daemon-stop-hard-policy");
        assert_eq!(
            runtime.handle("stop\thard=true\twait-ms=250\n"),
            format!(
                "stopped\t{}\t{}\tforce=false\thard=true\twait_ms=0\tpins=0\tlisteners=0\ttimed_out=0\n",
                std::process::id(),
                runtime.store
            )
        );
        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn daemon_runtime_writes_audit_records_for_lifecycle_events() {
        let (store, mut runtime) = test_runtime("daemon-audit");
        assert_eq!(
            runtime.handle("session-attach\tcli-session\n"),
            "attached\tcli-session\tsessions=1\n"
        );
        assert_eq!(
            runtime.handle("session-check\tcli-session\n"),
            "session\tcli-session\tlive\tsessions=1\n"
        );
        assert_eq!(
            runtime.handle("pin-add\tmount\n"),
            "pinned\tmount\tpins=1\n"
        );
        assert_eq!(
            runtime.handle("pin-remove\tmount\n"),
            "unpinned\tmount\tpins=0\n"
        );
        assert_eq!(
            runtime.handle("session-detach\tcli-session\n"),
            "detached\tcli-session\tsessions=0\n"
        );
        assert_eq!(
            runtime.handle("session-check\tcli-session\n"),
            "error\tNOT_FOUND: daemon session is not attached\n"
        );
        assert_eq!(
            runtime.handle("stop\n"),
            format!(
                "stopped\t{}\t{}\tforce=false\thard=false\twait_ms=30000\tpins=0\tlisteners=0\ttimed_out=0\n",
                std::process::id(),
                runtime.store
            )
        );

        let records = FileStore::open_read(&store)
            .unwrap()
            .audit_records()
            .unwrap();
        assert_eq!(
            records
                .iter()
                .map(|record| record.action.as_str())
                .collect::<Vec<_>>(),
            vec![
                "daemon.session.attach",
                "daemon.session.check",
                "daemon.pin.add",
                "daemon.pin.remove",
                "daemon.session.detach",
                "daemon.stop",
            ]
        );
        assert!(
            records
                .iter()
                .all(|record| record.principal.is_none() && record.target.is_some())
        );
        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn daemon_control_requires_admin_auth_when_authenticated() {
        let (store, mut runtime, root, user) = test_authenticated_runtime("daemon-control-auth");
        let no_auth = runtime.handle("session-attach\tcli-session\n");
        assert!(no_auth.contains("AUTHENTICATION_FAILED"), "{no_auth}");
        assert!(runtime.sessions.is_empty());

        let user_auth = request_auth_fields(user, "user-pass", "user-control");
        let denied = runtime.handle(&format!("pin-add\tmanual\t{user_auth}\n"));
        assert!(denied.contains("PERMISSION_DENIED"), "{denied}");
        assert!(runtime.pins.is_empty());

        let root_auth = request_auth_fields(root, "root-pass", "root-control");
        assert_eq!(
            runtime.handle(&format!("session-attach\tcli-session\t{root_auth}\n")),
            "attached\tcli-session\tsessions=1\n"
        );
        assert_eq!(
            runtime.handle(&format!("pin-add\tmanual\t{root_auth}\n")),
            "pinned\tmanual\tpins=1\n"
        );

        let records = FileStore::open_read(&store)
            .unwrap()
            .audit_records()
            .unwrap();
        assert!(
            records
                .iter()
                .filter(|record| record.action.starts_with("daemon."))
                .all(|record| record.principal == Some(root))
        );
        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn daemon_locks_require_admin_auth_when_authenticated() {
        let (store, mut runtime, root, user) = test_authenticated_runtime("daemon-lock-auth");
        let no_auth = runtime.handle("lock-acquire\tkey\tp\ts\texclusive\t1000\t1\n");
        assert!(no_auth.contains("AUTHENTICATION_FAILED"), "{no_auth}");

        let user_auth = request_auth_fields(user, "user-pass", "user-control");
        let denied = runtime.handle(&format!(
            "lock-acquire\tkey\tp\ts\texclusive\t1000\t1\t{user_auth}\n"
        ));
        assert!(denied.contains("PERMISSION_DENIED"), "{denied}");

        let root_auth = request_auth_fields(root, "root-pass", "root-control");
        let acquired = runtime.handle(&format!(
            "lock-acquire\tkey\tp\ts\texclusive\t1000\t1\t{root_auth}\n"
        ));
        assert!(acquired.starts_with("lock\t"), "{acquired}");
        let denied_break = runtime.handle(&format!("lock-break\tkey\t2\t{user_auth}\n"));
        assert!(denied_break.contains("PERMISSION_DENIED"), "{denied_break}");
        let broken = runtime.handle(&format!("lock-break\tkey\t2\t{root_auth}\n"));
        assert_eq!(broken, "broken\t1\n");
        let stale_apply = runtime.handle(&format!(
            "lock-apply-fence\tkey\tp\ts\texclusive\t1\t3\t{root_auth}\n"
        ));
        assert!(stale_apply.contains("LOCK_NOT_HELD"), "{stale_apply}");
        let reacquired = runtime.handle(&format!(
            "lock-acquire\tkey\tp\ts\texclusive\t1000\t4\t{root_auth}\n"
        ));
        assert!(
            reacquired.starts_with("lock\tkey\tp\ts\texclusive\t2\t"),
            "{reacquired}"
        );
        let applied = runtime.handle(&format!(
            "lock-apply-fence\tkey\tp\ts\texclusive\t2\t5\t{root_auth}\n"
        ));
        assert_eq!(applied, "applied\n");
        assert_eq!(
            FileStore::open_read(&store)
                .unwrap()
                .lock_coordinator()
                .unwrap()
                .applied_fence(b"key"),
            Some(loom_core::Fence::embedded(2))
        );

        let second = runtime.handle(&format!(
            "lock-acquire\tother\tp\ts\texclusive\t1000\t3\t{root_auth}\n"
        ));
        assert!(
            second.starts_with("lock\tother\tp\ts\texclusive\t1\t"),
            "{second}"
        );
        let expired = runtime.handle(&format!(
            "lock-apply-fence\tother\tp\ts\texclusive\t1\t1004\t{root_auth}\n"
        ));
        assert!(expired.contains("LOCK_LEASE_EXPIRED"), "{expired}");
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_cas_rest_listener() {
        let store = temp_store("daemon-cas-rest");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Cas,
                Some("main"),
                WorkspaceId::v4_from_bytes([6; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record(
            "cas",
            vec!["main".to_string()],
            "rest",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        let put = http_request(
            addr,
            "PUT /cas HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: 4\r\n\r\nbeta",
        );
        assert!(put.starts_with("HTTP/1.1 201 Created"), "{put}");
        let digest = put
            .split("\"digest\":\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap();
        let get = http_request(
            addr,
            &format!("GET /cas/{digest} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
        );
        assert!(get.ends_with("beta"), "{get}");
        assert!(
            loom_core::cas_get(
                &loom_store::open_loom_unlocked(&store, None).unwrap(),
                ns,
                &Digest::parse(digest).unwrap()
            )
            .unwrap()
            .is_some()
        );
        let records = FileStore::open_read(&store)
            .unwrap()
            .audit_records()
            .unwrap();
        assert!(
            records
                .iter()
                .any(|record| record.action == "serve.listener.open")
        );
        drop(runtimes);
        let records = FileStore::open_read(&store)
            .unwrap()
            .audit_records()
            .unwrap();
        assert!(
            records
                .iter()
                .any(|record| record.action == "serve.listener.close")
        );
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_oci_rest_listener() {
        let store = temp_store("daemon-oci-rest");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        loom.registry_mut()
            .create(
                FacetKind::Cas,
                Some("main"),
                WorkspaceId::v4_from_bytes([46; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record(
            "oci",
            vec!["main".to_string()],
            "rest",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        let digest = "sha256:f49ad260fbda6e2e6184bd6bc612a4689a92a9eeaa81671fb9b99a225830dc4e";
        let put = http_request(
            addr,
            &format!(
                "POST /v2/org/service/blobs/uploads/?digest={digest} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/octet-stream\r\nContent-Length: 11\r\n\r\ndaemon blob"
            ),
        );
        assert!(put.starts_with("HTTP/1.1 201 Created"), "{put}");
        assert!(
            put.contains("docker-content-digest: sha256:f49ad260"),
            "{put}"
        );
        let get = http_request(
            addr,
            &format!(
                "GET /v2/org/service/blobs/{digest} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
            ),
        );
        assert!(get.ends_with("daemon blob"), "{get}");
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_s3_rest_listener() {
        let store = temp_store("daemon-s3-rest");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        loom.registry_mut()
            .create(
                FacetKind::Cas,
                Some("main"),
                WorkspaceId::v4_from_bytes([54; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record(
            "s3",
            vec!["main".to_string()],
            "rest",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        let create = http_request(
            addr,
            "PUT /photos HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
        );
        assert!(create.starts_with("HTTP/1.1 200 OK"), "{create}");
        let put = http_request(
            addr,
            "PUT /photos/daemon.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: text/plain\r\nx-amz-meta-origin: daemon\r\nContent-Length: 11\r\n\r\ndaemon data",
        );
        assert!(put.starts_with("HTTP/1.1 200 OK"), "{put}");
        assert!(put.contains("etag:"), "{put}");
        assert!(put.contains("x-amz-version-id:"), "{put}");
        let get = http_request(
            addr,
            "GET /photos/daemon.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nRange: bytes=7-10\r\n\r\n",
        );
        assert!(get.starts_with("HTTP/1.1 206 Partial Content"), "{get}");
        assert!(get.ends_with("data"), "{get}");
        assert!(get.contains("x-amz-meta-origin: daemon"), "{get}");
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_redis_resp_listener_for_strings_and_ttl() {
        let store = temp_store("daemon-redis-resp");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        let root = WorkspaceId::v4_from_bytes([51; 16]);
        let mut identity = IdentityStore::new(root);
        identity
            .set_passphrase(root, "root-pass", b"12345678")
            .unwrap();
        let mut acl = AclStore::new();
        acl.allow(
            AclSubject::Principal(root),
            None,
            None,
            [AclRight::Admin, AclRight::Read, AclRight::Write],
        )
        .unwrap();
        fs.save_identity_store(&identity).unwrap();
        fs.save_acl_store(&acl).unwrap();
        drop(fs);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record(
            "redis",
            vec!["main".to_string(), "default".to_string()],
            "resp",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        let response = redis_resp_request(
            addr,
            &[
                resp_command(&["PING"]),
                resp_command(&["GET", "k"]),
                resp_command(&["AUTH", &root.to_string(), "root-pass"]),
                resp_command(&["SET", "k", "v", "PX", "60000"]),
                resp_command(&["SET", "durable", "kept"]),
                resp_command(&["HSET", "user", "name", "ada", "role", "owner"]),
                resp_command(&["HGET", "user", "name"]),
                resp_command(&["HLEN", "user"]),
                resp_command(&["SADD", "tags", "red", "blue", "blue"]),
                resp_command(&["SISMEMBER", "tags", "red"]),
                resp_command(&["SCARD", "tags"]),
                resp_command(&["LPUSH", "jobs", "b", "a"]),
                resp_command(&["RPUSH", "jobs", "c"]),
                resp_command(&["LLEN", "jobs"]),
                resp_command(&["LPOP", "jobs"]),
                resp_command(&["RPOP", "jobs"]),
                resp_command(&["ZADD", "ranks", "2", "bob", "1", "ada"]),
                resp_command(&["ZSCORE", "ranks", "ada"]),
                resp_command(&["ZCARD", "ranks"]),
                resp_command(&["XADD", "events", "*", "kind", "created"]),
                resp_command(&["PUBLISH", "events", "created"]),
                resp_command(&["TYPE", "user"]),
                resp_command(&["TYPE", "tags"]),
                resp_command(&["TYPE", "jobs"]),
                resp_command(&["TYPE", "ranks"]),
                resp_command(&["GET", "k"]),
                resp_command(&["TYPE", "k"]),
                resp_command(&["PTTL", "k"]),
                resp_command(&["DBSIZE"]),
                resp_command(&["PERSIST", "k"]),
                resp_command(&["TTL", "k"]),
                resp_command(&["DEL", "k"]),
                resp_command(&["GET", "k"]),
            ]
            .join(""),
        );
        assert!(response.contains("+PONG\r\n"), "{response}");
        assert!(
            response.contains("-NOAUTH Authentication required\r\n"),
            "{response}"
        );
        assert!(response.contains("+OK\r\n+OK\r\n+OK\r\n"), "{response}");
        assert!(response.contains("$1\r\nv\r\n"), "{response}");
        assert!(response.contains(":2\r\n$3\r\nada\r\n:2\r\n"), "{response}");
        assert!(response.contains(":2\r\n:1\r\n:2\r\n"), "{response}");
        assert!(
            response.contains(":2\r\n:3\r\n:3\r\n$1\r\na\r\n$1\r\nc\r\n"),
            "{response}"
        );
        assert!(response.contains(":2\r\n$1\r\n1\r\n:2\r\n"), "{response}");
        assert!(
            response.contains(
                "-ERR unsupported Redis stream command\r\n-ERR unsupported Redis pubsub command\r\n"
            ),
            "{response}"
        );
        assert!(
            response.contains("+hash\r\n+set\r\n+list\r\n+zset\r\n"),
            "{response}"
        );
        assert!(response.contains("+string\r\n"), "{response}");
        assert!(
            response.contains(":6\r\n:1\r\n:-1\r\n:1\r\n$-1\r\n"),
            "{response}"
        );
        drop(runtimes);

        let restarted = start_hosted_listeners(&store).unwrap();
        assert_eq!(restarted.len(), 1);
        let response = redis_resp_request(
            addr,
            &[
                resp_command(&["AUTH", &root.to_string(), "root-pass"]),
                resp_command(&["GET", "durable"]),
                resp_command(&["HGET", "user", "role"]),
                resp_command(&["SCARD", "tags"]),
                resp_command(&["LLEN", "jobs"]),
                resp_command(&["LPOP", "jobs"]),
                resp_command(&["ZSCORE", "ranks", "bob"]),
                resp_command(&["ZCARD", "ranks"]),
            ]
            .join(""),
        );
        assert!(response.contains("+OK\r\n$4\r\nkept\r\n"), "{response}");
        assert!(
            response.contains("$5\r\nowner\r\n:2\r\n:1\r\n$1\r\nb\r\n"),
            "{response}"
        );
        assert!(response.contains("$1\r\n2\r\n:2\r\n"), "{response}");
        drop(restarted);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_memcached_text_listener_for_cache_commands() {
        let store = temp_store("daemon-memcached-text");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record(
            "memcached",
            vec!["main".to_string(), "sessions".to_string()],
            "text",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        let response = memcached_text_request(
            addr,
            concat!(
                "version\r\n",
                "set text 1 0 5\r\nfirst\r\n",
                "append text 0 0 3\r\n+++\r\n",
                "prepend text 0 0 3\r\npre\r\n",
                "get text\r\n",
                "set count 0 0 2\r\n10\r\n",
                "incr count 5\r\n",
                "decr count 20\r\n",
                "get count\r\n",
                "gat 1 text\r\n",
                "gats 1 text\r\n",
                "flush_all\r\n",
                "get text\r\n",
                "verbosity 1\r\n",
                "set alpha 7 0 5\r\nfirst\r\n",
                "get alpha\r\n",
                "gets alpha\r\n",
                "add alpha 7 0 6\r\nsecond\r\n",
                "replace beta 7 0 5\r\nthird\r\n",
                "replace alpha 9 0 6\r\nsecond\r\n",
                "gets alpha\r\n",
                "cas alpha 9 0 5 2\r\nthird\r\n",
                "get alpha\r\n",
                "touch alpha 1\r\n",
                "delete alpha\r\n",
                "get alpha\r\n",
                "stats\r\n",
            ),
        );
        assert!(
            response.contains("VERSION loom-memcached\r\n"),
            "{response}"
        );
        assert!(
            response.contains("VALUE text 1 11\r\nprefirst+++\r\nEND\r\n"),
            "{response}"
        );
        assert!(
            response.contains("15\r\n0\r\nVALUE count 0 1\r\n0\r\nEND\r\n"),
            "{response}"
        );
        assert!(
            response.contains("VALUE text 1 11\r\nprefirst+++\r\nEND\r\nVALUE text 1 11 3\r\nprefirst+++\r\nEND\r\nOK\r\nEND\r\nOK\r\n"),
            "{response}"
        );
        assert!(
            response.contains("STORED\r\nVALUE alpha 7 5\r\nfirst\r\nEND\r\n"),
            "{response}"
        );
        assert!(
            response.contains("VALUE alpha 7 5 1\r\nfirst\r\nEND\r\n"),
            "{response}"
        );
        assert!(
            response.contains("NOT_STORED\r\nNOT_STORED\r\n"),
            "{response}"
        );
        assert!(
            response.contains("STORED\r\nVALUE alpha 9 6 2\r\nsecond\r\nEND\r\n"),
            "{response}"
        );
        assert!(
            response.contains("STORED\r\nVALUE alpha 9 5\r\nthird\r\nEND\r\n"),
            "{response}"
        );
        assert!(
            response.contains("TOUCHED\r\nDELETED\r\nEND\r\n"),
            "{response}"
        );
        assert!(
            response.contains("STAT version loom-memcached\r\n"),
            "{response}"
        );
        assert!(
            response.contains("STAT cache main:sessions\r\n"),
            "{response}"
        );
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_etcd_tcp_listener() {
        let store = temp_store("daemon-etcd-tcp");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record(
            "etcd",
            vec!["main".to_string(), "config".to_string()],
            "tcp",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_kafka_tcp_listener_for_api_versions() {
        let store = temp_store("daemon-kafka-tcp");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record(
            "kafka",
            vec!["main".to_string()],
            "tcp",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        let response = kafka_request(
            addr,
            kafka_frame(
                kafka_protocol::messages::ApiKey::ApiVersions,
                4,
                kafka_protocol::messages::ApiVersionsRequest::default(),
            ),
        );
        let decoded = decode_kafka_api_versions_response(response, 4);
        let api_keys: Vec<i16> = decoded.api_keys.iter().map(|key| key.api_key).collect();
        assert_eq!(
            api_keys,
            vec![
                kafka_protocol::messages::ApiKey::SaslHandshake as i16,
                kafka_protocol::messages::ApiKey::SaslAuthenticate as i16,
                kafka_protocol::messages::ApiKey::Metadata as i16,
                kafka_protocol::messages::ApiKey::CreateTopics as i16,
                kafka_protocol::messages::ApiKey::DeleteTopics as i16,
                kafka_protocol::messages::ApiKey::Produce as i16,
                kafka_protocol::messages::ApiKey::Fetch as i16,
                kafka_protocol::messages::ApiKey::OffsetCommit as i16,
                kafka_protocol::messages::ApiKey::InitProducerId as i16,
                kafka_protocol::messages::ApiKey::AddPartitionsToTxn as i16,
                kafka_protocol::messages::ApiKey::AddOffsetsToTxn as i16,
                kafka_protocol::messages::ApiKey::EndTxn as i16,
                kafka_protocol::messages::ApiKey::TxnOffsetCommit as i16,
                kafka_protocol::messages::ApiKey::ApiVersions as i16,
            ]
        );
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_cas_jsonrpc_listener() {
        let store = temp_store("daemon-cas-jsonrpc");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Cas,
                Some("main"),
                WorkspaceId::v4_from_bytes([7; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record(
            "cas",
            vec!["main".to_string()],
            "json_rpc",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        let put_body = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"cas.put\",\"params\":{\"bytes_hex\":\"62657461\"}}";
        let put = http_request(addr, &jsonrpc_http_request(put_body));
        assert!(put.starts_with("HTTP/1.1 200 OK"), "{put}");
        let digest = put
            .split("\"digest\":\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap();
        let get_body = format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"cas.get\",\"params\":{{\"digest\":\"{digest}\"}}}}"
        );
        let get = http_request(addr, &jsonrpc_http_request(&get_body));
        assert!(get.contains("\"bytes_hex\":\"62657461\""), "{get}");
        assert!(
            loom_core::cas_get(
                &loom_store::open_loom_unlocked(&store, None).unwrap(),
                ns,
                &Digest::parse(digest).unwrap()
            )
            .unwrap()
            .is_some()
        );
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_files_listeners() {
        let store = temp_store("daemon-files");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        loom.registry_mut()
            .create(
                FacetKind::Files,
                Some("main"),
                WorkspaceId::v4_from_bytes([16; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let records = ["rest", "json_rpc", "grpc"]
            .into_iter()
            .map(|transport| {
                let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
                let addr = probe.local_addr().unwrap();
                drop(probe);
                let record = FileStore::served_listener_record(
                    "files",
                    vec!["main".to_string()],
                    transport,
                    &addr.to_string(),
                    true,
                )
                .unwrap();
                (record, addr)
            })
            .collect::<Vec<_>>();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            for (record, _) in &records {
                fs.save_served_listener_audited(
                    record,
                    None,
                    "serve.listener.configure",
                    Some(&served_listener_target(record)),
                )
                .unwrap();
            }
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 3);
        let mkdir = http_request(
            records[0].1,
            &admin_json_request(
                "POST",
                "/tree:mkdir",
                "{\"path\":\"docs\",\"recursive\":true}",
            ),
        );
        assert!(mkdir.starts_with("HTTP/1.1 204 No Content"), "{mkdir}");
        let put = http_request(
            records[0].1,
            &admin_json_request("PUT", "/tree/docs/a.txt", "alpha"),
        );
        assert!(put.starts_with("HTTP/1.1 204 No Content"), "{put}");
        let get = http_request(
            records[0].1,
            "GET /tree/docs/a.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        assert!(get.ends_with("alpha"), "{get}");
        let write = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"fs.write_file\",\"params\":{\"path\":\"docs/b.txt\",\"bytes_hex\":\"627261766f\"}}";
        let write = http_request(records[1].1, &jsonrpc_http_request(write));
        assert!(write.starts_with("HTTP/1.1 200 OK"), "{write}");
        let read = "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"fs.read_file\",\"params\":{\"path\":\"docs/b.txt\"}}";
        let read = http_request(records[1].1, &jsonrpc_http_request(read));
        assert!(read.contains("\"bytes_hex\":\"627261766f\""), "{read}");
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_web_listener_for_static_files() {
        let store = temp_store("daemon-web");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("site"),
                WorkspaceId::v4_from_bytes([18; 16]),
            )
            .unwrap();
        loom.write_file(ns, "index.html", b"<h1>Home</h1>", 0o100644)
            .unwrap();
        loom.write_file(ns, "about.html", b"<h1>About</h1>", 0o100644)
            .unwrap();
        loom.create_directory(ns, "docs", true).unwrap();
        loom.write_file(ns, "docs/index.html", b"<h1>Docs</h1>", 0o100644)
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record(
            "web",
            vec!["site".to_string()],
            "rest",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
            let mut web_listener = loom_substrate::web::WebListener::new(
                &record.id,
                "127.0.0.1",
                addr.port(),
                loom_substrate::web::WebProtocol::Http,
                ns,
                "/",
            )
            .unwrap();
            web_listener.routes = loom_substrate::web::WebRouteTable::new(vec![
                loom_substrate::web::WebRoute::new(
                    "manual",
                    vec![
                        loom_substrate::web::WebMethod::Get,
                        loom_substrate::web::WebMethod::Head,
                    ],
                    None,
                    "/manual",
                    "/docs",
                    loom_substrate::web::WebRouteMode::StaticFile,
                )
                .unwrap(),
            ])
            .unwrap();
            let key = loom_substrate::web::web_profile_listener_key(&record.id).unwrap();
            fs.control_set(&key, web_listener.encode().unwrap())
                .unwrap();
            let stored = fs.control_get(&key).unwrap().unwrap();
            let stored = loom_substrate::web::WebListener::decode(&stored).unwrap();
            assert_eq!(stored.routes.routes.len(), 1);
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        let home = http_request(
            addr,
            "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        assert!(home.contains("content-type: text/html"), "{home}");
        assert!(home.ends_with("<h1>Home</h1>"), "{home}");
        let about = http_request(
            addr,
            "GET /about HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        assert!(about.ends_with("<h1>About</h1>"), "{about}");
        let docs = http_request(
            addr,
            "GET /docs/ HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        assert!(docs.ends_with("<h1>Docs</h1>"), "{docs}");
        let manual = http_request(
            addr,
            "GET /manual/ HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        assert!(manual.ends_with("<h1>Docs</h1>"), "{manual}");
        let hidden = http_request(
            addr,
            "GET /.loom/config HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        assert!(hidden.starts_with("HTTP/1.1 404 Not Found"), "{hidden}");
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_vcs_listeners() {
        let store = temp_store("daemon-vcs");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("main"),
                WorkspaceId::v4_from_bytes([22; 16]),
            )
            .unwrap();
        loom.create_directory(ns, "docs", true).unwrap();
        loom.write_file(ns, "docs/a.txt", b"alpha", 0o100644)
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let records = ["rest", "json_rpc", "grpc"]
            .into_iter()
            .map(|transport| {
                let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
                let addr = probe.local_addr().unwrap();
                drop(probe);
                let record = FileStore::served_listener_record(
                    "vcs",
                    vec!["main".to_string()],
                    transport,
                    &addr.to_string(),
                    true,
                )
                .unwrap();
                (record, addr)
            })
            .collect::<Vec<_>>();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            for (record, _) in &records {
                fs.save_served_listener_audited(
                    record,
                    None,
                    "serve.listener.configure",
                    Some(&served_listener_target(record)),
                )
                .unwrap();
            }
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 3);
        let commit_one = http_request(
            records[0].1,
            &admin_json_request(
                "POST",
                "/commits",
                "{\"message\":\"one\",\"author\":\"root\"}",
            ),
        );
        assert!(
            commit_one.starts_with("HTTP/1.1 201 Created"),
            "{commit_one}"
        );
        let first = commit_one
            .split("\"commit\":\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap()
            .to_string();
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        loom.write_file(ns, "docs/a.txt", b"bravo", 0o100644)
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);
        let commit_two = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"vcs.commit\",\"params\":{\"message\":\"two\",\"author\":\"root\"}}";
        let commit_two = http_request(records[1].1, &jsonrpc_http_request(commit_two));
        assert!(commit_two.starts_with("HTTP/1.1 200 OK"), "{commit_two}");
        let second = commit_two
            .split("\"commit\":\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap()
            .to_string();
        let log = http_request(records[0].1, &admin_json_request("GET", "/commits", ""));
        assert!(log.contains(&first), "{log}");
        assert!(log.contains(&second), "{log}");
        let diff_body = format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"vcs.diff\",\"params\":{{\"from\":\"{first}\",\"to\":\"{second}\"}}}}"
        );
        let diff = http_request(records[1].1, &jsonrpc_http_request(&diff_body));
        assert!(diff.contains("\"diff_cbor_hex\":\""), "{diff}");
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_cas_grpc_listener() {
        let store = temp_store("daemon-cas-grpc");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        loom.registry_mut()
            .create(
                FacetKind::Cas,
                Some("main"),
                WorkspaceId::v4_from_bytes([17; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record(
            "cas",
            vec!["main".to_string()],
            "grpc",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        let records = FileStore::open_read(&store)
            .unwrap()
            .audit_records()
            .unwrap();
        assert!(
            records
                .iter()
                .any(|record| record.action == "serve.listener.open")
        );
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_reconciles_disabled_cas_listener() {
        let store = temp_store("daemon-cas-reconcile");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        let coordinator = fs.lock_coordinator().unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        loom.registry_mut()
            .create(
                FacetKind::Cas,
                Some("main"),
                WorkspaceId::v4_from_bytes([18; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let mut record = FileStore::served_listener_record(
            "cas",
            vec!["main".to_string()],
            "rest",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }
        let paths = daemon::paths(&store).unwrap();
        let hosted_listeners = start_hosted_listeners(&store).unwrap();
        assert_eq!(hosted_listeners.len(), 1);
        record = FileStore::open_read(&store)
            .unwrap()
            .served_listener(&record.id)
            .unwrap()
            .unwrap();
        record.enabled = false;
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.disable",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }
        let mut runtime = DaemonRuntime {
            store: paths.store,
            store_id: paths.store_id,
            transport: daemon::DaemonTransport::TcpLoopback,
            coordinator,
            kv_loom: None,
            kv_unavailable: None,
            sessions: std::collections::BTreeSet::new(),
            pins: std::collections::BTreeMap::new(),
            authority_replication_next: std::collections::BTreeMap::new(),
            maintenance_next_ms: 0,
            hosted_listeners,
            drive_policy_next_ms: 0,
            reference_reconcile_next_ms: 0,
        };

        runtime.reconcile_hosted_listeners().unwrap();
        assert!(runtime.hosted_listeners.is_empty());
        let records = FileStore::open_read(&store)
            .unwrap()
            .audit_records()
            .unwrap();
        assert!(
            records
                .iter()
                .any(|record| record.action == "serve.listener.close")
        );
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_restarts_listener_when_certificate_bundle_changes() {
        let store = temp_store("daemon-cas-tls-bundle-reconcile");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        let coordinator = fs.lock_coordinator().unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        loom.registry_mut()
            .create(
                FacetKind::Cas,
                Some("main"),
                WorkspaceId::v4_from_bytes([25; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        save_test_certificate_bundle(&store, "cas-main", None);
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let mut record = FileStore::served_listener_record(
            "cas",
            vec!["main".to_string()],
            "rest",
            &addr.to_string(),
            true,
        )
        .unwrap();
        record.tls.mode = "direct".to_string();
        record.tls.certificate_bundle_ref = Some("cas-main".to_string());
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let paths = daemon::paths(&store).unwrap();
        let hosted_listeners = start_hosted_listeners(&store).unwrap();
        let before_fingerprint = hosted_listeners
            .get(&record.id)
            .unwrap()
            .tls_certificate_bundle_fingerprint
            .clone();
        let mut runtime = DaemonRuntime {
            store: paths.store,
            store_id: paths.store_id,
            transport: daemon::DaemonTransport::TcpLoopback,
            coordinator,
            kv_loom: None,
            kv_unavailable: None,
            sessions: std::collections::BTreeSet::new(),
            pins: std::collections::BTreeMap::new(),
            authority_replication_next: std::collections::BTreeMap::new(),
            maintenance_next_ms: 0,
            hosted_listeners,
            drive_policy_next_ms: 0,
            reference_reconcile_next_ms: 0,
        };

        save_test_certificate_bundle(&store, "cas-main", Some(TLS_TEST_CERT.as_bytes().to_vec()));
        runtime.reconcile_hosted_listeners().unwrap();
        let after_fingerprint = runtime
            .hosted_listeners
            .get(&record.id)
            .unwrap()
            .tls_certificate_bundle_fingerprint
            .clone();
        assert_ne!(before_fingerprint, after_fingerprint);
        let records = FileStore::open_read(&store)
            .unwrap()
            .audit_records()
            .unwrap();
        assert!(
            records
                .iter()
                .filter(|record| record.action == "serve.listener.open")
                .count()
                >= 2
        );
        assert!(
            records
                .iter()
                .any(|record| record.action == "serve.listener.close")
        );
        let open_targets = records
            .iter()
            .filter(|record| record.action == "serve.listener.open")
            .filter_map(|record| record.target.as_deref())
            .collect::<Vec<_>>();
        assert!(
            open_targets
                .iter()
                .any(|target| target.contains("tls_server_chain_digest="))
        );
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_restarts_listener_when_network_access_policy_changes() {
        let store = temp_store("daemon-cas-network-access-reconcile");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        let coordinator = fs.lock_coordinator().unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        loom.registry_mut()
            .create(
                FacetKind::Cas,
                Some("main"),
                WorkspaceId::v4_from_bytes([26; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let first_rule = loom_store::NetworkAccessRule {
            id: "allow-localhost".to_string(),
            action: loom_store::NetworkAccessAction::Allow,
            source_cidr: Some(loom_store::NetworkAccessCidr::parse("127.0.0.1").unwrap()),
            trusted_proxy_cidr: None,
            require_mtls: false,
            client_cert_subject: None,
            client_cert_san: None,
            client_cert_issuer: None,
            description: None,
        };
        let policy = FileStore::network_access_policy_record(
            "local",
            None,
            loom_store::NetworkAccessAction::Deny,
            vec![first_rule],
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_network_access_policy_audited(
                &policy,
                None,
                "network-access.policy.set",
                Some("name=local"),
            )
            .unwrap();
        }

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let mut record = FileStore::served_listener_record(
            "cas",
            vec!["main".to_string()],
            "rest",
            &addr.to_string(),
            true,
        )
        .unwrap();
        record.network_access_policy_ref = Some("local".to_string());
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let paths = daemon::paths(&store).unwrap();
        let hosted_listeners = start_hosted_listeners(&store).unwrap();
        let before_fingerprint = hosted_listeners
            .get(&record.id)
            .unwrap()
            .network_access_policy_fingerprint
            .clone();
        let mut runtime = DaemonRuntime {
            store: paths.store,
            store_id: paths.store_id,
            transport: daemon::DaemonTransport::TcpLoopback,
            coordinator,
            kv_loom: None,
            kv_unavailable: None,
            sessions: std::collections::BTreeSet::new(),
            pins: std::collections::BTreeMap::new(),
            authority_replication_next: std::collections::BTreeMap::new(),
            maintenance_next_ms: 0,
            hosted_listeners,
            drive_policy_next_ms: 0,
            reference_reconcile_next_ms: 0,
        };

        let second_rule = loom_store::NetworkAccessRule {
            id: "allow-loopback-subnet".to_string(),
            action: loom_store::NetworkAccessAction::Allow,
            source_cidr: Some(loom_store::NetworkAccessCidr::parse("127.0.0.0/8").unwrap()),
            trusted_proxy_cidr: None,
            require_mtls: false,
            client_cert_subject: None,
            client_cert_san: None,
            client_cert_issuer: None,
            description: None,
        };
        let changed = FileStore::network_access_policy_record(
            "local",
            Some("expanded loopback".to_string()),
            loom_store::NetworkAccessAction::Deny,
            vec![second_rule],
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_network_access_policy_audited(
                &changed,
                None,
                "network-access.policy.set",
                Some("name=local"),
            )
            .unwrap();
        }
        runtime.reconcile_hosted_listeners().unwrap();
        let after_fingerprint = runtime
            .hosted_listeners
            .get(&record.id)
            .unwrap()
            .network_access_policy_fingerprint
            .clone();
        assert_ne!(before_fingerprint, after_fingerprint);
        let records = FileStore::open_read(&store)
            .unwrap()
            .audit_records()
            .unwrap();
        assert!(
            records
                .iter()
                .filter(|record| record.action == "serve.listener.open")
                .count()
                >= 2
        );
        let open_targets = records
            .iter()
            .filter(|record| record.action == "serve.listener.open")
            .filter_map(|record| record.target.as_deref())
            .collect::<Vec<_>>();
        assert!(
            open_targets
                .iter()
                .any(|target| target.contains("network_access_policy=local"))
        );
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_drive_policy_worker_applies_registered_share_expiry_and_retention() {
        let store = temp_store("daemon-drive-policy-worker");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        let workspace = WorkspaceId::v4_from_bytes([27; 16]);
        let root = WorkspaceId::v4_from_bytes([28; 16]);
        let recipient = WorkspaceId::v4_from_bytes([29; 16]);
        let mut loom = Loom::new(fs);
        loom.registry_mut()
            .create(FacetKind::Files, Some("repo"), workspace)
            .unwrap();
        loom.registry_mut()
            .add_facet(workspace, FacetKind::Vcs)
            .unwrap();
        let mut identity = IdentityStore::new(root);
        identity
            .set_passphrase(root, "root-pass", b"12345678")
            .unwrap();
        let mut acl = AclStore::new();
        acl.allow(
            AclSubject::Principal(root),
            None,
            None,
            [AclRight::Admin, AclRight::Read, AclRight::Write],
        )
        .unwrap();
        loom.store().save_identity_store(&identity).unwrap();
        loom.store().save_acl_store(&acl).unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let mut loom = loom_store::open_loom_daemon_authorized_unlocked(&store, None).unwrap();
        loom = loom_store::attach_local_auth(
            loom,
            &LocalOpenAuth {
                preauthenticated_principal: Some(root),
                session_id: Some("root-setup".to_string()),
                ..LocalOpenAuth::default()
            },
        )
        .unwrap();
        let root_folder =
            loom_hosted::drive::list_folder(&loom, workspace, "main", "root").unwrap();
        loom_hosted::drive::grant_share(
            &mut loom,
            workspace,
            loom_hosted::drive::HostedDriveGrantShare {
                workspace_id: "main",
                grant_id: "grant-expiring",
                target_kind: "folder",
                target_id: "root",
                principal: &recipient.to_string(),
                role: "viewer",
                granted_at_ms: 100,
                expires_at_ms: Some(200),
            },
        )
        .unwrap();
        loom_hosted::drive::pin_retention(
            &mut loom,
            workspace,
            loom_hosted::drive::HostedDrivePinRetention {
                workspace_id: "main",
                pin_id: "trash-expiring",
                kind: "trash_subtree",
                root: &root_folder.profile_root,
                target_entity_id: Some("folder:trash"),
                added_at_ms: 100,
                expires_at_ms: Some(200),
            },
        )
        .unwrap();
        drop(loom);

        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            let mut registry = DrivePolicyRegistry::empty();
            registry
                .upsert_enabled(DrivePolicyTarget::new(workspace, "main", true).unwrap())
                .unwrap();
            save_drive_policy_registry_audited(
                &fs,
                &registry,
                Some(root),
                "workspace=repo;drive=main;enabled=true",
            )
            .unwrap();
        }

        assert_eq!(apply_drive_policy_workers_once(&store, 500).unwrap(), 2);

        let fs = FileStore::open_read(&store).unwrap();
        let identity = fs.identity_store().unwrap().unwrap();
        let service = identity.principal(DAEMON_SERVICE_PRINCIPAL_ID).unwrap();
        assert_eq!(service.kind, PrincipalKind::Service);
        assert!(service.roles.contains(&loom_core::ROLE_SERVICE_ID));
        assert!(service.roles.contains(&loom_core::ROLE_ADMIN_ID));
        let acl = fs.acl_store().unwrap().unwrap();
        assert!(acl.grants().iter().any(|grant| {
            grant.subject == AclSubject::Principal(DAEMON_SERVICE_PRINCIPAL_ID)
                && grant.workspace.is_none()
                && grant.facet == Some(FacetKind::Vcs)
                && grant.scopes == [AclScope::All]
                && grant.rights.contains(&AclRight::Admin)
        }));
        let records = fs.audit_records().unwrap();
        assert!(
            records
                .iter()
                .any(|record| record.action == "daemon.service_principal.ensure")
        );
        assert!(
            records
                .iter()
                .any(|record| record.action == "daemon.service_principal.acl")
        );
        assert!(
            records
                .iter()
                .any(|record| record.action == "drive.share_acl.expire")
        );
        drop(fs);

        let mut loom = loom_store::open_loom_daemon_authorized_unlocked(&store, None).unwrap();
        loom = loom_store::attach_local_auth(
            loom,
            &LocalOpenAuth {
                preauthenticated_principal: Some(root),
                session_id: Some("root-verify".to_string()),
                ..LocalOpenAuth::default()
            },
        )
        .unwrap();
        assert!(
            loom_hosted::drive::list_shares(&loom, workspace, "main")
                .unwrap()
                .is_empty()
        );
        assert!(
            loom_hosted::drive::list_retention(&loom, workspace, "main")
                .unwrap()
                .is_empty()
        );
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_records_sanitized_network_access_denied_audit() {
        let store = temp_store("daemon-network-access-denied-audit");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let sink = network_access_denied_audit_sink(store.clone());
        let policy = FileStore::network_access_policy_record(
            "audit-policy",
            None,
            loom_store::NetworkAccessAction::Deny,
            Vec::new(),
        )
        .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(
            loom_hosted::with_hosted_network_access_policy_for_listener_and_audit(
                "id=audit;surface=admin;transport=rest;profile=;bind=127.0.0.1:0;enabled=true;network_access_policy=audit-policy;network_access_digest=blake3:test"
                    .to_string(),
                Some(policy),
                Some(sink),
                async {
                    let allowed = loom_hosted::network_access_allows(
                        loom_hosted::current_hosted_network_access_policy().as_ref(),
                        "198.51.100.1:443".parse().unwrap(),
                        None,
                        Some("203.0.113.9, 198.51.100.1"),
                        None,
                    );
                    assert!(!allowed);
                },
            ),
        );
        let records = FileStore::open_read(&store)
            .unwrap()
            .audit_records()
            .unwrap();
        let target = records
            .iter()
            .find(|record| record.action == "network_access.connection.deny")
            .and_then(|record| record.target.as_deref())
            .unwrap();
        assert!(target.contains("policy=audit-policy"));
        assert!(target.contains("rule=default"));
        assert!(target.contains("source_family=ipv4"));
        assert!(!target.contains("203.0.113.9"));
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "mcp")]
    #[test]
    fn mcp_http_network_access_uses_stored_source_policy_and_audits_denies() {
        let store = temp_store("mcp-network-access-source-policy");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        let policy = FileStore::network_access_policy_record(
            "loopback",
            None,
            loom_store::NetworkAccessAction::Deny,
            vec![loom_store::NetworkAccessRule {
                id: "allow-loopback".to_string(),
                action: loom_store::NetworkAccessAction::Allow,
                source_cidr: Some(loom_store::NetworkAccessCidr::parse("127.0.0.0/8").unwrap()),
                trusted_proxy_cidr: None,
                require_mtls: false,
                client_cert_subject: None,
                client_cert_san: None,
                client_cert_issuer: None,
                description: None,
            }],
        )
        .unwrap();
        fs.save_network_access_policy_audited(
            &policy,
            None,
            "network-access.policy.set",
            Some("name=loopback"),
        )
        .unwrap();
        drop(fs);

        let gate =
            mcp_http_network_access(&store, "loopback", "127.0.0.1:0".parse().unwrap()).unwrap();
        assert!(gate("127.0.0.1:49152".parse().unwrap(), None, None));
        assert!(!gate("198.51.100.10:49152".parse().unwrap(), None, None));

        let records = FileStore::open_read(&store)
            .unwrap()
            .audit_records()
            .unwrap();
        let target = records
            .iter()
            .find(|record| record.action == "network_access.connection.deny")
            .and_then(|record| record.target.as_deref())
            .unwrap();
        assert!(target.contains("mcp-http;bind=127.0.0.1:0"));
        assert!(target.contains("policy=loopback"));
        assert!(target.contains("rule=default"));
        assert!(target.contains("source_family=ipv4"));
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "mcp")]
    #[test]
    fn mcp_http_network_access_rejects_missing_and_mtls_policies() {
        let store = temp_store("mcp-network-access-policy-rejects");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        let mtls_policy = FileStore::network_access_policy_record(
            "mtls-only",
            None,
            loom_store::NetworkAccessAction::Deny,
            vec![loom_store::NetworkAccessRule {
                id: "allow-mtls".to_string(),
                action: loom_store::NetworkAccessAction::Allow,
                source_cidr: None,
                trusted_proxy_cidr: None,
                require_mtls: true,
                client_cert_subject: None,
                client_cert_san: None,
                client_cert_issuer: None,
                description: None,
            }],
        )
        .unwrap();
        fs.save_network_access_policy_audited(
            &mtls_policy,
            None,
            "network-access.policy.set",
            Some("name=mtls-only"),
        )
        .unwrap();
        drop(fs);

        let missing =
            match mcp_http_network_access(&store, "missing", "127.0.0.1:0".parse().unwrap()) {
                Ok(_) => panic!("missing network access policy unexpectedly loaded"),
                Err(err) => err,
            };
        assert!(missing.contains("not found"), "{missing}");
        let mtls =
            match mcp_http_network_access(&store, "mtls-only", "127.0.0.1:0".parse().unwrap()) {
                Ok(_) => panic!("mTLS network access policy unexpectedly loaded"),
                Err(err) => err,
            };
        assert!(mtls.contains("requires mTLS"), "{mtls}");
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_enforces_served_listener_request_size_limit() {
        let store = temp_store("daemon-cas-limit");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        loom.registry_mut()
            .create(
                FacetKind::Cas,
                Some("main"),
                WorkspaceId::v4_from_bytes([20; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let mut record = FileStore::served_listener_record(
            "cas",
            vec!["main".to_string()],
            "rest",
            &addr.to_string(),
            true,
        )
        .unwrap();
        record.limits.request_size_limit = 3;
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        let put = http_request(
            addr,
            "PUT /cas HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: 4\r\n\r\nbeta",
        );
        assert!(put.starts_with("HTTP/1.1 413 Payload Too Large"), "{put}");
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_admin_rest_listener() {
        let store = temp_store("daemon-admin-rest");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        let root = WorkspaceId::v4_from_bytes([28; 16]);
        let identity = IdentityStore::new(root);
        let mut acl = AclStore::new();
        acl.grant(AclGrant {
            subject: AclSubject::Role(loom_core::ROLE_ADMIN_ID),
            workspace: None,
            domain: None,
            ref_glob: None,
            scopes: vec![AclScope::All],
            rights: std::collections::BTreeSet::from([AclRight::Admin]),
            effect: AclEffect::Allow,
            predicate: None,
        })
        .unwrap();
        loom.set_identity_store(identity.clone());
        loom.set_acl_store(acl.clone());
        loom.registry_mut()
            .create(
                FacetKind::Files,
                Some("main"),
                WorkspaceId::v4_from_bytes([19; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
        loom.store().save_identity_store(&identity).unwrap();
        loom.store().save_acl_store(&acl).unwrap();
        drop(loom);
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let admin =
            FileStore::served_listener_record("admin", Vec::new(), "rest", &addr.to_string(), true)
                .unwrap();
        let cas = FileStore::served_listener_record(
            "cas",
            vec!["main".to_string()],
            "rest",
            "127.0.0.1:6550",
            false,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &admin,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&admin)),
            )
            .unwrap();
            fs.save_served_listener_audited(
                &cas,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&cas)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        let list = http_request(
            addr,
            "GET /admin/listeners HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        assert!(list.contains("\"surface\":\"admin\""), "{list}");
        assert!(list.contains("\"surface\":\"cas\""), "{list}");
        let audit = http_request(
            addr,
            "GET /admin/audit HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        assert!(
            audit.contains("\"action\":\"serve.listener.configure\""),
            "{audit}"
        );
        let disable = http_request(
            addr,
            &format!(
                "PUT /admin/listeners/{}/disable HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
                cas.id
            ),
        );
        assert!(disable.starts_with("HTTP/1.1 200 OK"), "{disable}");
        assert!(
            !FileStore::open_read(&store)
                .unwrap()
                .served_listener(&cas.id)
                .unwrap()
                .unwrap()
                .enabled
        );
        let acl_body = "{\"effect\":\"allow\",\"subject\":\"*\",\"rights\":[\"read\"],\"facet\":\"files\",\"workspace\":\"main\",\"scopes\":[\"path:public/\"],\"predicate\":{\"language\":\"cel\",\"expression\":\"principal == 'guest'\"}}";
        let acl = http_request(
            addr,
            &admin_json_request("POST", "/admin/acl/grant", acl_body),
        );
        assert!(acl.starts_with("HTTP/1.1 200 OK"), "{acl}");
        let acl = http_request(
            addr,
            "GET /admin/acl HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        assert!(acl.contains("\"subject\":\"*\""), "{acl}");
        assert!(acl.contains("\"kind\":\"path\""), "{acl}");
        assert!(acl.contains("\"language\":\"cel\""), "{acl}");
        assert!(acl.contains("\"principal == 'guest'\""), "{acl}");
        let protected_body = "{\"workspace\":\"main\",\"ref_name\":\"branch/main\",\"fast_forward_only\":true,\"signed_commits_required\":false,\"signed_ref_advance_required\":false,\"required_review_count\":0,\"retention_lock\":false,\"governance_lock\":false}";
        let set = http_request(
            addr,
            &admin_json_request("POST", "/admin/protected-refs/set", protected_body),
        );
        assert!(set.starts_with("HTTP/1.1 200 OK"), "{set}");
        let policies = http_request(
            addr,
            "GET /admin/protected-refs/main HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        assert!(policies.contains("\"ref\":\"branch/main\""), "{policies}");
        let config = http_request(
            addr,
            "GET /admin/audit/config HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        assert!(config.contains("\"retention_days\":365"), "{config}");
        let config = http_request(
            addr,
            &admin_json_request(
                "PUT",
                "/admin/audit/config",
                "{\"retention_days\":30,\"legal_hold\":false}",
            ),
        );
        assert!(config.contains("\"retention_days\":30"), "{config}");
        let export = http_request(
            addr,
            "GET /admin/audit/export HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        assert!(export.contains("\"hash\":\""), "{export}");
        let prune = http_request(
            addr,
            &admin_json_request("POST", "/admin/audit/prune", "{\"through_seq\":0}"),
        );
        assert!(prune.contains("\"pruned\":"), "{prune}");
        let identity = http_request(
            addr,
            "GET /admin/identity HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        assert!(identity.contains("\"roles\""), "{identity}");
        assert!(identity.contains("\"name\":\"root\""), "{identity}");
        let root_pass = "root-pass";
        let set_pass = http_request(
            addr,
            &admin_json_request(
                "POST",
                "/admin/identity/passphrase",
                &format!("{{\"principal\":\"{root}\",\"passphrase\":\"{root_pass}\"}}"),
            ),
        );
        assert!(set_pass.starts_with("HTTP/1.1 200 OK"), "{set_pass}");
        let user = WorkspaceId::v4_from_bytes([23; 16]);
        let add = http_request(
            addr,
            &admin_json_auth_request(
                "POST",
                "/admin/identity/add",
                &format!("{{\"id\":\"{user}\",\"name\":\"alice\",\"kind\":\"user\"}}"),
                root,
                root_pass,
            ),
        );
        assert!(add.contains("\"name\":\"alice\""), "{add}");
        let assign = http_request(
            addr,
            &admin_json_auth_request(
                "POST",
                "/admin/identity/roles/assign",
                &format!(
                    "{{\"principal\":\"{user}\",\"role\":\"{}\"}}",
                    loom_core::ROLE_READER_ID
                ),
                root,
                root_pass,
            ),
        );
        assert!(assign.starts_with("HTTP/1.1 200 OK"), "{assign}");
        let revoke = http_request(
            addr,
            &admin_json_auth_request(
                "POST",
                "/admin/identity/roles/revoke",
                &format!(
                    "{{\"principal\":\"{user}\",\"role\":\"{}\"}}",
                    loom_core::ROLE_READER_ID
                ),
                root,
                root_pass,
            ),
        );
        assert!(revoke.contains("\"removed\":true"), "{revoke}");
        let remove = http_request(
            addr,
            &admin_json_auth_request(
                "POST",
                "/admin/identity/remove",
                &format!("{{\"principal\":\"{user}\"}}"),
                root,
                root_pass,
            ),
        );
        assert!(remove.starts_with("HTTP/1.1 200 OK"), "{remove}");
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_admin_jsonrpc_listener() {
        let store = temp_store("daemon-admin-jsonrpc");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        let root = WorkspaceId::v4_from_bytes([32; 16]);
        let identity = IdentityStore::new(root);
        let mut acl = AclStore::new();
        acl.grant(AclGrant {
            subject: AclSubject::Role(loom_core::ROLE_ADMIN_ID),
            workspace: None,
            domain: None,
            ref_glob: None,
            scopes: vec![AclScope::All],
            rights: std::collections::BTreeSet::from([AclRight::Admin]),
            effect: AclEffect::Allow,
            predicate: None,
        })
        .unwrap();
        loom.set_identity_store(identity.clone());
        loom.set_acl_store(acl.clone());
        save_loom(&mut loom).unwrap();
        loom.store().save_identity_store(&identity).unwrap();
        loom.store().save_acl_store(&acl).unwrap();
        drop(loom);
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let admin = FileStore::served_listener_record(
            "admin",
            Vec::new(),
            "json_rpc",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &admin,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&admin)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        let body =
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"admin.listeners.list\",\"params\":{}}";
        let list = http_request(addr, &jsonrpc_http_request(body));
        assert!(list.starts_with("HTTP/1.1 200 OK"), "{list}");
        assert!(list.contains("\"surface\":\"admin\""), "{list}");
        assert!(list.contains("\"transport\":\"json_rpc\""), "{list}");
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_sql_rest_listener() {
        let store = temp_store("daemon-sql-rest");
        let _ = std::fs::remove_file(&store);
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        loom.registry_mut()
            .create(
                FacetKind::Sql,
                Some("main"),
                WorkspaceId::v4_from_bytes([33; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record(
            "sql",
            vec!["main".to_string(), "db".to_string()],
            "rest",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        let create_body = "{\"sql\":\"CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t VALUES (1, 'a')\"}";
        let create_bytes =
            http_request_bytes(addr, &admin_json_request("POST", "/sql:exec", create_body));
        let create = String::from_utf8_lossy(&create_bytes);
        assert!(create.starts_with("HTTP/1.1 200 OK"), "{create}");
        let query_body = "{\"sql\":\"SELECT id, v FROM t\"}";
        let query_bytes =
            http_request_bytes(addr, &admin_json_request("POST", "/sql:query", query_body));
        let query = String::from_utf8_lossy(&query_bytes);
        assert!(query.starts_with("HTTP/1.1 200 OK"), "{query}");
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_sql_jsonrpc_listener() {
        let store = temp_store("daemon-sql-jsonrpc");
        let _ = std::fs::remove_file(&store);
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        loom.registry_mut()
            .create(
                FacetKind::Sql,
                Some("main"),
                WorkspaceId::v4_from_bytes([34; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record(
            "sql",
            vec!["main".to_string(), "db".to_string()],
            "json_rpc",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        let body = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"sql.exec\",\"params\":{\"sql\":\"CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t VALUES (1, 'a')\"}}";
        let create = http_request(addr, &jsonrpc_http_request(body));
        assert!(create.starts_with("HTTP/1.1 200 OK"), "{create}");
        assert!(create.contains("\"cbor_hex\":\""), "{create}");
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_sql_grpc_listener() {
        let store = temp_store("daemon-sql-grpc");
        let _ = std::fs::remove_file(&store);
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        loom.registry_mut()
            .create(
                FacetKind::Sql,
                Some("main"),
                WorkspaceId::v4_from_bytes([35; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record(
            "sql",
            vec!["main".to_string(), "db".to_string()],
            "grpc",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_kv_grpc_listener() {
        let store = temp_store("daemon-kv-grpc");
        let _ = std::fs::remove_file(&store);
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        loom.registry_mut()
            .create(
                FacetKind::Kv,
                Some("main"),
                WorkspaceId::v4_from_bytes([36; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record(
            "kv",
            vec!["main".to_string(), "cache".to_string()],
            "grpc",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_document_grpc_listener() {
        let store = temp_store("daemon-document-grpc");
        let _ = std::fs::remove_file(&store);
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        loom.registry_mut()
            .create(
                FacetKind::Document,
                Some("main"),
                WorkspaceId::v4_from_bytes([39; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record(
            "document",
            vec!["main".to_string(), "docs".to_string()],
            "grpc",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_fts_grpc_listener() {
        let store = temp_store("daemon-fts-grpc");
        let _ = std::fs::remove_file(&store);
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        loom.registry_mut()
            .create(
                FacetKind::Search,
                Some("main"),
                WorkspaceId::v4_from_bytes([41; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record(
            "fts",
            vec!["main".to_string(), "docs-search".to_string()],
            "grpc",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_queue_grpc_listener() {
        let store = temp_store("daemon-queue-grpc");
        let _ = std::fs::remove_file(&store);
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        loom.registry_mut()
            .create(
                FacetKind::Queue,
                Some("main"),
                WorkspaceId::v4_from_bytes([37; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record(
            "queue",
            vec!["main".to_string(), "events".to_string()],
            "grpc",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_time_series_grpc_listener() {
        let store = temp_store("daemon-time-series-grpc");
        let _ = std::fs::remove_file(&store);
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        loom.registry_mut()
            .create(
                FacetKind::TimeSeries,
                Some("main"),
                WorkspaceId::v4_from_bytes([38; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record(
            "time-series",
            vec!["main".to_string(), "metrics".to_string()],
            "grpc",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_columnar_grpc_listener() {
        let store = temp_store("daemon-columnar-grpc");
        let _ = std::fs::remove_file(&store);
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        loom.registry_mut()
            .create(
                FacetKind::Columnar,
                Some("main"),
                WorkspaceId::v4_from_bytes([42; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record(
            "columnar",
            vec!["main".to_string(), "events".to_string()],
            "grpc",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_graph_grpc_listener() {
        let store = temp_store("daemon-graph-grpc");
        let _ = std::fs::remove_file(&store);
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        loom.registry_mut()
            .create(
                FacetKind::Graph,
                Some("main"),
                WorkspaceId::v4_from_bytes([44; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record(
            "graph",
            vec!["main".to_string(), "work".to_string()],
            "grpc",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_ledger_grpc_listener() {
        let store = temp_store("daemon-ledger-grpc");
        let _ = std::fs::remove_file(&store);
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        loom.registry_mut()
            .create(
                FacetKind::Ledger,
                Some("main"),
                WorkspaceId::v4_from_bytes([43; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record(
            "ledger",
            vec!["main".to_string(), "audit".to_string()],
            "grpc",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_data_facet_listeners() {
        let store = temp_store("daemon-data-facets");
        let _ = std::fs::remove_file(&store);
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("main"),
                WorkspaceId::v4_from_bytes([40; 16]),
            )
            .unwrap();
        for facet in [
            FacetKind::Kv,
            FacetKind::Document,
            FacetKind::Queue,
            FacetKind::TimeSeries,
            FacetKind::Graph,
            FacetKind::Ledger,
            FacetKind::Vector,
            FacetKind::Search,
            FacetKind::Columnar,
            FacetKind::Vcs,
        ] {
            loom.registry_mut().add_facet(ns, facet).unwrap();
        }
        let chat_workspace_id = ns.to_string();
        let mut chat_directory =
            loom_substrate::chat::ChatChannelDirectory::new(&chat_workspace_id).unwrap();
        chat_directory
            .create_channel(WorkspaceId::v4_from_bytes([44; 16]), "general", "General")
            .unwrap();
        chat_directory
            .create_channel(WorkspaceId::v4_from_bytes([45; 16]), "random", "Random")
            .unwrap();
        let chat_directory_path = String::from_utf8(
            loom_substrate::chat::chat_channel_directory_key(&chat_workspace_id).unwrap(),
        )
        .unwrap();
        let chat_directory_parent = chat_directory_path.rsplit_once('/').unwrap().0;
        loom.create_directory_reserved(ns, chat_directory_parent, true)
            .unwrap();
        loom.write_file_reserved(
            ns,
            &chat_directory_path,
            &chat_directory.encode().unwrap(),
            0o100644,
        )
        .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let records = [
            ("kv", "rest", vec!["main".to_string(), "cache".to_string()]),
            (
                "document",
                "json_rpc",
                vec!["main".to_string(), "docs".to_string()],
            ),
            (
                "queue",
                "rest",
                vec!["main".to_string(), "events".to_string()],
            ),
            (
                "time-series",
                "json_rpc",
                vec!["main".to_string(), "metrics".to_string()],
            ),
            ("graph", "rest", vec!["main".to_string(), "g".to_string()]),
            (
                "ledger",
                "json_rpc",
                vec!["main".to_string(), "audit".to_string()],
            ),
            (
                "vector",
                "rest",
                vec!["main".to_string(), "embeddings".to_string()],
            ),
            (
                "fts",
                "json_rpc",
                vec!["main".to_string(), "docs-search".to_string()],
            ),
            (
                "fts",
                "ndjson",
                vec!["main".to_string(), "docs-os".to_string()],
            ),
            (
                "columnar",
                "json_rpc",
                vec!["main".to_string(), "events-col".to_string()],
            ),
            (
                "chat",
                "rest",
                vec!["main".to_string(), "general".to_string()],
            ),
            (
                "chat",
                "json_rpc",
                vec!["main".to_string(), "random".to_string()],
            ),
        ]
        .into_iter()
        .map(|(surface, transport, selectors)| {
            let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = probe.local_addr().unwrap();
            drop(probe);
            let record = FileStore::served_listener_record(
                surface,
                selectors,
                transport,
                &addr.to_string(),
                true,
            )
            .unwrap();
            (record, addr)
        })
        .collect::<Vec<_>>();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            for (record, _) in &records {
                fs.save_served_listener_audited(
                    record,
                    None,
                    "serve.listener.configure",
                    Some(&served_listener_target(record)),
                )
                .unwrap();
            }
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 12);
        let key_hex = hex_bytes(&loom_core::key_to_cbor(&loom_core::Value::Text("a".into())));
        let kv = http_request(
            records[0].1,
            &admin_json_request(
                "POST",
                "/kv:put",
                &format!("{{\"key_hex\":\"{key_hex}\",\"value_hex\":\"6f6e65\"}}"),
            ),
        );
        assert!(kv.starts_with("HTTP/1.1 200 OK"), "{kv}");
        let doc_body = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"document.put_binary\",\"params\":{\"id\":\"doc-1\",\"bytes_hex\":\"7b7d\"}}";
        let doc = http_request(records[1].1, &jsonrpc_http_request(doc_body));
        assert!(doc.starts_with("HTTP/1.1 200 OK"), "{doc}");
        let queue = http_request(
            records[2].1,
            &admin_json_request("POST", "/queue:append", "{\"payload_hex\":\"657631\"}"),
        );
        assert!(queue.contains("\"seq\":0"), "{queue}");
        let ts_body = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"timeseries.put\",\"params\":{\"timestamp\":100,\"value_hex\":\"70313030\"}}";
        let ts = http_request(records[3].1, &jsonrpc_http_request(ts_body));
        assert!(ts.starts_with("HTTP/1.1 200 OK"), "{ts}");
        let graph_a = http_request(
            records[4].1,
            &admin_json_request("POST", "/graph:upsert-node", "{\"id\":\"a\"}"),
        );
        assert!(graph_a.starts_with("HTTP/1.1 200 OK"), "{graph_a}");
        let graph_b = http_request(
            records[4].1,
            &admin_json_request("POST", "/graph:upsert-node", "{\"id\":\"b\"}"),
        );
        assert!(graph_b.starts_with("HTTP/1.1 200 OK"), "{graph_b}");
        let edge = http_request(
            records[4].1,
            &admin_json_request(
                "POST",
                "/graph:upsert-edge",
                "{\"id\":\"e1\",\"src\":\"a\",\"dst\":\"b\",\"label\":\"knows\"}",
            ),
        );
        assert!(edge.starts_with("HTTP/1.1 200 OK"), "{edge}");
        let ledger_body = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ledger.append\",\"params\":{\"payload_hex\":\"6531\"}}";
        let ledger = http_request(records[5].1, &jsonrpc_http_request(ledger_body));
        assert!(ledger.contains("\"seq\":0"), "{ledger}");
        let vector_create = http_request(
            records[6].1,
            &admin_json_request("POST", "/vector:create", "{\"dim\":2,\"metric\":\"dot\"}"),
        );
        assert!(
            vector_create.starts_with("HTTP/1.1 200 OK"),
            "{vector_create}"
        );
        let search_create = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"fts.create\",\"params\":{\"mapping\":{\"title\":\"text\"}}}";
        let search = http_request(records[7].1, &jsonrpc_http_request(search_create));
        assert!(search.starts_with("HTTP/1.1 200 OK"), "{search}");
        let search_compat = http_request(
            records[8].1,
            &admin_json_request(
                "PUT",
                "/docs-os",
                "{\"mappings\":{\"properties\":{\"title\":{\"type\":\"text\"}}}}",
            ),
        );
        assert!(
            search_compat.starts_with("HTTP/1.1 200 OK"),
            "{search_compat}"
        );

        let create = http_request(
            records[9].1,
            &jsonrpc_http_request(
                "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"columnar.create\",\"params\":{\"columns\":[{\"name\":\"id\",\"type\":\"int\"},{\"name\":\"value\",\"type\":\"text\"}],\"target_segment_rows\":0}}",
            ),
        );
        assert!(create.starts_with("HTTP/1.1 200 OK"), "{create}");
        let append = http_request(
            records[9].1,
            &jsonrpc_http_request(
                "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"columnar.append\",\"params\":{\"dataset\":\"events-col\",\"row\":[1,\"alpha\"]}}",
            ),
        );
        assert!(append.starts_with("HTTP/1.1 200 OK"), "{append}");
        let select = http_request(
            records[9].1,
            &jsonrpc_http_request(
                "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"columnar.select\",\"params\":{\"columns\":[\"value\"],\"dataset\":\"events-col\"}}",
            ),
        );
        assert!(select.contains("[[\"alpha\"]]"), "{select}");
        let chat = http_request(
            records[10].1,
            &admin_json_request(
                "POST",
                "/chat:post-message",
                "{\"message_id\":\"m1\",\"body_hex\":\"68656c6c6f\"}",
            ),
        );
        assert!(chat.starts_with("HTTP/1.1 201 Created"), "{chat}");
        let chat_jsonrpc = http_request(
            records[11].1,
            &jsonrpc_http_request(
                "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"chat.post_message\",\"params\":{\"message_id\":\"m2\",\"body_hex\":\"68656c6c6f\"}}",
            ),
        );
        assert!(
            chat_jsonrpc.starts_with("HTTP/1.1 200 OK"),
            "{chat_jsonrpc}"
        );
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_runtime_supports_neo4j_tcp_after_bolt_skeleton_lands() {
        let record = FileStore::served_listener_record(
            "neo4j",
            vec!["main".to_string(), "people".to_string()],
            "tcp",
            "127.0.0.1:17687",
            true,
        )
        .unwrap();
        assert!(supported_hosted_listener_runtime(&record));
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_neo4j_tcp_listener_for_bolt_handshake() {
        let store = temp_store("daemon-neo4j-tcp");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("main"),
                WorkspaceId::v4_from_bytes([69; 16]),
            )
            .unwrap();
        loom.registry_mut().add_facet(ns, FacetKind::Graph).unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record(
            "neo4j",
            vec!["main".to_string(), "people".to_string()],
            "tcp",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        assert_eq!(neo4j_selected_version(addr), 0x0000_0105);
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_pinecone_rest_listener() {
        let store = temp_store("daemon-pinecone-rest");
        let _ = std::fs::remove_file(&store);
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("main"),
                WorkspaceId::v4_from_bytes([60; 16]),
            )
            .unwrap();
        loom.registry_mut()
            .add_facet(ns, FacetKind::Vector)
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record_with_profile(
            "vector",
            vec!["main".to_string(), "docs".to_string()],
            "rest",
            Some("pinecone"),
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        let capabilities = http_request(addr, &admin_json_request("GET", "/capabilities", ""));
        assert!(
            capabilities.contains("\"profile\":\"pinecone\""),
            "{capabilities}"
        );
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_qdrant_grpc_listener() {
        let store = temp_store("daemon-qdrant-grpc");
        let _ = std::fs::remove_file(&store);
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("main"),
                WorkspaceId::v4_from_bytes([59; 16]),
            )
            .unwrap();
        loom.registry_mut()
            .add_facet(ns, FacetKind::Vector)
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record_with_profile(
            "vector",
            vec!["main".to_string(), "docs".to_string()],
            "grpc",
            Some("qdrant"),
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_calendar_caldav_listener() {
        let store = temp_store("daemon-calendar-caldav");
        let _ = std::fs::remove_file(&store);
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("main"),
                WorkspaceId::v4_from_bytes([41; 16]),
            )
            .unwrap();
        loom.registry_mut()
            .add_facet(ns, FacetKind::Calendar)
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record(
            "calendar",
            vec!["main".to_string()],
            "caldav",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        let mkcalendar = http_request(
            addr,
            "MKCALENDAR /caldav/work HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
        );
        assert!(
            mkcalendar.starts_with("HTTP/1.1 201 Created"),
            "{mkcalendar}"
        );
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:event-1\r\nSUMMARY:Planning\r\nDTSTART:20260101T120000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let put = http_request(
            addr,
            &format!(
                "PUT /caldav/work/event-1.ics HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: text/calendar\r\nContent-Length: {}\r\n\r\n{}",
                ics.len(),
                ics
            ),
        );
        assert!(put.starts_with("HTTP/1.1 201 Created"), "{put}");
        let get = http_request(
            addr,
            "GET /caldav/work/event-1.ics HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        assert!(get.starts_with("HTTP/1.1 200 OK"), "{get}");
        assert!(get.contains("SUMMARY:Planning"), "{get}");
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_contacts_carddav_listener() {
        let store = temp_store("daemon-contacts-carddav");
        let _ = std::fs::remove_file(&store);
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("main"),
                WorkspaceId::v4_from_bytes([42; 16]),
            )
            .unwrap();
        loom.registry_mut()
            .add_facet(ns, FacetKind::Contacts)
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record(
            "contacts",
            vec!["main".to_string()],
            "carddav",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        let mkcol = http_request(
            addr,
            "MKCOL /carddav/people HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
        );
        assert!(mkcol.starts_with("HTTP/1.1 201 Created"), "{mkcol}");
        let vcf =
            "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:contact-1\r\nFN:Alice Example\r\nEND:VCARD\r\n";
        let put = http_request(
            addr,
            &format!(
                "PUT /carddav/people/contact-1.vcf HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: text/vcard\r\nContent-Length: {}\r\n\r\n{}",
                vcf.len(),
                vcf
            ),
        );
        assert!(put.starts_with("HTTP/1.1 201 Created"), "{put}");
        let get = http_request(
            addr,
            "GET /carddav/people/contact-1.vcf HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        assert!(get.starts_with("HTTP/1.1 200 OK"), "{get}");
        assert!(get.contains("FN:Alice Example"), "{get}");
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_direct_tls_calendar_caldav_listener() {
        let store = temp_store("daemon-tls-calendar-caldav");
        let _ = std::fs::remove_file(&store);
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("main"),
                WorkspaceId::v4_from_bytes([51; 16]),
            )
            .unwrap();
        loom.registry_mut()
            .add_facet(ns, FacetKind::Calendar)
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        save_test_certificate_bundle(&store, "calendar-main", None);
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let mut record = FileStore::served_listener_record(
            "calendar",
            vec!["main".to_string()],
            "caldav",
            &addr.to_string(),
            true,
        )
        .unwrap();
        record.tls.mode = "direct".to_string();
        record.tls.certificate_bundle_ref = Some("calendar-main".to_string());
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        let mkcalendar = https_request(
            addr,
            "MKCALENDAR /caldav/work HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
        );
        assert!(
            mkcalendar.starts_with("HTTP/1.1 201 Created"),
            "{mkcalendar}"
        );
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:event-1\r\nSUMMARY:TLS Planning\r\nDTSTART:20260101T120000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let put = https_request(
            addr,
            &format!(
                "PUT /caldav/work/event-1.ics HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: text/calendar\r\nContent-Length: {}\r\n\r\n{}",
                ics.len(),
                ics
            ),
        );
        assert!(put.starts_with("HTTP/1.1 201 Created"), "{put}");
        let get = https_request(
            addr,
            "GET /caldav/work/event-1.ics HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        assert!(get.starts_with("HTTP/1.1 200 OK"), "{get}");
        assert!(get.contains("SUMMARY:TLS Planning"), "{get}");
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_direct_tls_contacts_carddav_listener() {
        let store = temp_store("daemon-tls-contacts-carddav");
        let _ = std::fs::remove_file(&store);
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("main"),
                WorkspaceId::v4_from_bytes([52; 16]),
            )
            .unwrap();
        loom.registry_mut()
            .add_facet(ns, FacetKind::Contacts)
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        save_test_certificate_bundle(&store, "contacts-main", None);
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let mut record = FileStore::served_listener_record(
            "contacts",
            vec!["main".to_string()],
            "carddav",
            &addr.to_string(),
            true,
        )
        .unwrap();
        record.tls.mode = "direct".to_string();
        record.tls.certificate_bundle_ref = Some("contacts-main".to_string());
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        let mkcol = https_request(
            addr,
            "MKCOL /carddav/people HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
        );
        assert!(mkcol.starts_with("HTTP/1.1 201 Created"), "{mkcol}");
        let vcf = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:contact-1\r\nFN:TLS Alice\r\nEND:VCARD\r\n";
        let put = https_request(
            addr,
            &format!(
                "PUT /carddav/people/contact-1.vcf HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: text/vcard\r\nContent-Length: {}\r\n\r\n{}",
                vcf.len(),
                vcf
            ),
        );
        assert!(put.starts_with("HTTP/1.1 201 Created"), "{put}");
        let get = https_request(
            addr,
            "GET /carddav/people/contact-1.vcf HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        assert!(get.starts_with("HTTP/1.1 200 OK"), "{get}");
        assert!(get.contains("FN:TLS Alice"), "{get}");
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_mail_imap_listener() {
        let store = temp_store("daemon-mail-imap");
        let _ = std::fs::remove_file(&store);
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("main"),
                WorkspaceId::v4_from_bytes([48; 16]),
            )
            .unwrap();
        loom.registry_mut().add_facet(ns, FacetKind::Cas).unwrap();
        loom.registry_mut().add_facet(ns, FacetKind::Mail).unwrap();
        loom_core::mail::create_mailbox(
            &mut loom,
            ns,
            "root",
            "inbox",
            &loom_core::mail::MailboxMeta {
                display_name: "Inbox".to_string(),
            },
        )
        .unwrap();
        loom_core::mail::ingest_message(
            &mut loom,
            ns,
            "root",
            "inbox",
            "1",
            b"From: a@example.com\r\nTo: root@example.com\r\nSubject: Daemon IMAP\r\n\r\nBody",
        )
        .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record(
            "mail",
            vec!["main".to_string()],
            "imap",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        let response = imap_request(
            addr,
            "a1 LOGIN root anything\r\na2 SELECT INBOX\r\na3 FETCH 1:* (FLAGS UID RFC822.SIZE BODY[])\r\na4 LOGOUT\r\n",
        );
        assert!(response.contains("a1 OK LOGIN completed"), "{response}");
        assert!(
            response.contains("a2 OK [READ-WRITE] SELECT completed"),
            "{response}"
        );
        assert!(response.contains("Subject: Daemon IMAP"), "{response}");
        assert!(response.contains("a4 OK LOGOUT completed"), "{response}");
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_durable_mail_jmap_listener() {
        let store = temp_store("daemon-mail-jmap");
        let _ = std::fs::remove_file(&store);
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("main"),
                WorkspaceId::v4_from_bytes([50; 16]),
            )
            .unwrap();
        loom.registry_mut().add_facet(ns, FacetKind::Cas).unwrap();
        loom.registry_mut().add_facet(ns, FacetKind::Mail).unwrap();
        loom_core::mail::create_mailbox(
            &mut loom,
            ns,
            "root",
            "inbox",
            &loom_core::mail::MailboxMeta {
                display_name: "Inbox".to_string(),
            },
        )
        .unwrap();
        loom_core::mail::ingest_message(
            &mut loom,
            ns,
            "root",
            "inbox",
            "1",
            b"From: a@example.com\r\nTo: root@example.com\r\nSubject: Daemon JMAP\r\n\r\nBody",
        )
        .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let record = FileStore::served_listener_record(
            "mail",
            vec!["main".to_string()],
            "jmap",
            &addr.to_string(),
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        let body = "{\"using\":[\"urn:ietf:params:jmap:core\",\"urn:ietf:params:jmap:mail\"],\"methodCalls\":[[\"Email/query\",{\"filter\":{\"inMailbox\":\"inbox\",\"text\":\"Daemon\"}},\"a\"],[\"Email/get\",{\"ids\":[\"inbox/1\"]},\"b\"]]}";
        let response = http_request(
            addr,
            &format!(
                "POST /jmap/api HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            ),
        );
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
        assert!(response.contains("\"Email/query\""), "{response}");
        assert!(response.contains("\"inbox/1\""), "{response}");
        assert!(response.contains("Daemon JMAP"), "{response}");
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_direct_tls_mail_jmap_listener() {
        let store = temp_store("daemon-tls-mail-jmap");
        let _ = std::fs::remove_file(&store);
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("main"),
                WorkspaceId::v4_from_bytes([53; 16]),
            )
            .unwrap();
        loom.registry_mut().add_facet(ns, FacetKind::Cas).unwrap();
        loom.registry_mut().add_facet(ns, FacetKind::Mail).unwrap();
        loom_core::mail::create_mailbox(
            &mut loom,
            ns,
            "root",
            "inbox",
            &loom_core::mail::MailboxMeta {
                display_name: "Inbox".to_string(),
            },
        )
        .unwrap();
        loom_core::mail::ingest_message(
            &mut loom,
            ns,
            "root",
            "inbox",
            "1",
            b"From: tls@example.com\r\nTo: root@example.com\r\nSubject: Daemon JMAP TLS\r\n\r\nBody",
        )
        .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        save_test_certificate_bundle(&store, "jmap-main", None);
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let mut record = FileStore::served_listener_record(
            "mail",
            vec!["main".to_string()],
            "jmap",
            &addr.to_string(),
            true,
        )
        .unwrap();
        record.tls.mode = "direct".to_string();
        record.tls.certificate_bundle_ref = Some("jmap-main".to_string());
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        let body = "{\"using\":[\"urn:ietf:params:jmap:core\",\"urn:ietf:params:jmap:mail\"],\"methodCalls\":[[\"Email/query\",{\"filter\":{\"inMailbox\":\"inbox\",\"text\":\"TLS\"}},\"a\"],[\"Email/get\",{\"ids\":[\"inbox/1\"]},\"b\"]]}";
        let response = https_request(
            addr,
            &format!(
                "POST /jmap/api HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            ),
        );
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
        assert!(response.contains("\"Email/query\""), "{response}");
        assert!(response.contains("\"inbox/1\""), "{response}");
        assert!(response.contains("Daemon JMAP TLS"), "{response}");
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_direct_tls_mail_imap_listener() {
        let store = temp_store("daemon-mail-imaps");
        let _ = std::fs::remove_file(&store);
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("main"),
                WorkspaceId::v4_from_bytes([49; 16]),
            )
            .unwrap();
        loom.registry_mut().add_facet(ns, FacetKind::Cas).unwrap();
        loom.registry_mut().add_facet(ns, FacetKind::Mail).unwrap();
        loom_core::mail::create_mailbox(
            &mut loom,
            ns,
            "root",
            "inbox",
            &loom_core::mail::MailboxMeta {
                display_name: "Inbox".to_string(),
            },
        )
        .unwrap();
        loom_core::mail::ingest_message(
            &mut loom,
            ns,
            "root",
            "inbox",
            "1",
            b"From: tls@example.com\r\nTo: root@example.com\r\nSubject: Daemon IMAPS\r\n\r\nBody",
        )
        .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        save_test_certificate_bundle(&store, "mail-main", None);
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let mut record = FileStore::served_listener_record(
            "mail",
            vec!["main".to_string()],
            "imap",
            &addr.to_string(),
            true,
        )
        .unwrap();
        record.tls.mode = "direct".to_string();
        record.tls.certificate_bundle_ref = Some("mail-main".to_string());
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        let response = imaps_request(
            addr,
            "a1 LOGIN root anything\r\na2 SELECT INBOX\r\na3 FETCH 1:* (FLAGS UID RFC822.SIZE BODY[])\r\na4 LOGOUT\r\n",
        );
        assert!(response.contains("a1 OK LOGIN completed"), "{response}");
        assert!(
            response.contains("a2 OK [READ-WRITE] SELECT completed"),
            "{response}"
        );
        assert!(response.contains("Subject: Daemon IMAPS"), "{response}");
        assert!(response.contains("a4 OK LOGOUT completed"), "{response}");
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_direct_tls_cas_rest_listener() {
        let store = temp_store("daemon-tls-cas-rest");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        loom.registry_mut()
            .create(
                FacetKind::Cas,
                Some("main"),
                WorkspaceId::v4_from_bytes([21; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        save_test_certificate_bundle(&store, "cas-main", None);
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let mut record = FileStore::served_listener_record(
            "cas",
            vec!["main".to_string()],
            "rest",
            &addr.to_string(),
            true,
        )
        .unwrap();
        record.tls.mode = "direct".to_string();
        record.tls.certificate_bundle_ref = Some("cas-main".to_string());
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        let put = https_request(
            addr,
            "PUT /cas HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: 4\r\n\r\nbeta",
        );
        assert!(put.starts_with("HTTP/1.1 201 Created"), "{put}");
        let digest = put
            .split("\"digest\":\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap();
        let get = https_request(
            addr,
            &format!("GET /cas/{digest} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
        );
        assert!(get.ends_with("beta"), "{get}");
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_rejects_direct_tls_cas_grpc_listener_until_grpc_tls_is_promoted() {
        let store = temp_store("daemon-grpc-tls-reject");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut record = FileStore::served_listener_record(
            "cas",
            vec![WorkspaceId::v4_from_bytes([22; 16]).to_string()],
            "grpc",
            "127.0.0.1:6551",
            true,
        )
        .unwrap();
        record.tls.mode = "direct".to_string();
        record.tls.certificate_bundle_ref = Some("main".to_string());
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let err = match start_hosted_listeners(&store) {
            Ok(_) => panic!("direct TLS cas/grpc listener unexpectedly started"),
            Err(err) => err,
        };
        assert!(err.contains("unsupported direct TLS for grpc"), "{err}");
        let records = FileStore::open_read(&store)
            .unwrap()
            .audit_records()
            .unwrap();
        assert!(
            records
                .iter()
                .any(|record| record.action == "serve.listener.reject")
        );
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_runtime_filter_accepts_time_series_compat_http_listeners() {
        for surface in ["influx", "prometheus", "grafana"] {
            let record = FileStore::served_listener_record(
                surface,
                vec!["main".to_string()],
                "http",
                "127.0.0.1:0",
                true,
            )
            .unwrap();
            assert!(supported_hosted_listener_runtime(&record), "{surface}");
        }
        let otlp = FileStore::served_listener_record(
            "otlp",
            vec!["main".to_string()],
            "http",
            "127.0.0.1:0",
            true,
        )
        .unwrap();
        assert!(supported_hosted_listener_runtime(&otlp));
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_rejects_invalid_trust_bundle_listener() {
        let store = temp_store("daemon-trust-bundle-invalid");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        save_test_certificate_bundle(
            &store,
            "cas-invalid-trust",
            Some(b"not a certificate".to_vec()),
        );
        let mut record = FileStore::served_listener_record(
            "cas",
            vec![WorkspaceId::v4_from_bytes([24; 16]).to_string()],
            "rest",
            "127.0.0.1:6554",
            true,
        )
        .unwrap();
        record.tls.mode = "direct".to_string();
        record.tls.certificate_bundle_ref = Some("cas-invalid-trust".to_string());
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let err = match start_hosted_listeners(&store) {
            Ok(_) => panic!("invalid trust-bundle listener unexpectedly started"),
            Err(err) => err,
        };
        assert!(err.contains("contains no certificates"), "{err}");
        let records = FileStore::open_read(&store)
            .unwrap()
            .audit_records()
            .unwrap();
        assert!(
            records
                .iter()
                .any(|record| record.action == "serve.listener.reject")
        );
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_opens_custom_listener_timeouts() {
        let store = temp_store("daemon-timeout-open");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        drop(fs);
        let mut loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        loom.registry_mut()
            .create(
                FacetKind::Cas,
                Some("main"),
                WorkspaceId::v4_from_bytes([25; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);
        let mut record = FileStore::served_listener_record(
            "cas",
            vec!["main".to_string()],
            "rest",
            "127.0.0.1:0",
            true,
        )
        .unwrap();
        record.limits.idle_timeout_ms = 250;
        record.limits.session_timeout_ms = 500;
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let runtimes = start_hosted_listeners(&store).unwrap();
        assert_eq!(runtimes.len(), 1);
        let records = FileStore::open_read(&store)
            .unwrap()
            .audit_records()
            .unwrap();
        assert!(
            records
                .iter()
                .any(|record| record.action == "serve.listener.open")
        );
        drop(runtimes);
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn daemon_rejects_fips_profile_served_listener_without_fips_runtime() {
        let store = temp_store("daemon-fips-serve");
        let fs = FileStore::create_with_profile(&store, Algo::Sha256).unwrap();
        drop(fs);
        let record = FileStore::served_listener_record(
            "cas",
            vec![WorkspaceId::v4_from_bytes([9; 16]).to_string()],
            "rest",
            "127.0.0.1:6553",
            true,
        )
        .unwrap();
        {
            let fs = FileStore::open_daemon_authorized(&store).unwrap();
            fs.save_served_listener_audited(
                &record,
                None,
                "serve.listener.configure",
                Some(&served_listener_target(&record)),
            )
            .unwrap();
        }

        let err = match start_hosted_listeners(&store) {
            Ok(_) => panic!("FIPS-profile hosted listener unexpectedly started"),
            Err(err) => err,
        };
        assert!(err.contains("FIPS-profile stores cannot be served"));
        let records = FileStore::open_read(&store)
            .unwrap()
            .audit_records()
            .unwrap();
        assert!(
            records
                .iter()
                .any(|record| record.action == "serve.listener.reject")
        );
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "serve")]
    fn jsonrpc_http_request(body: &str) -> String {
        format!(
            "POST /jsonrpc HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )
    }

    #[cfg(feature = "serve")]
    fn hex_bytes(bytes: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0f) as usize] as char);
        }
        out
    }

    #[cfg(feature = "serve")]
    fn admin_json_request(method: &str, path: &str, body: &str) -> String {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )
    }

    #[cfg(feature = "serve")]
    fn admin_json_auth_request(
        method: &str,
        path: &str,
        body: &str,
        principal: WorkspaceId,
        passphrase: &str,
    ) -> String {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nx-loom-principal: {principal}\r\nx-loom-passphrase: {passphrase}\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )
    }

    #[cfg(feature = "serve")]
    fn http_request(addr: std::net::SocketAddr, request: &str) -> String {
        let mut stream = std::net::TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(30)))
            .unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        stream.flush().unwrap();
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
        response
    }

    #[cfg(feature = "serve")]
    fn http_request_bytes(addr: std::net::SocketAddr, request: &str) -> Vec<u8> {
        let mut stream = std::net::TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(30)))
            .unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        stream.flush().unwrap();
        let mut response = Vec::new();
        let _ = stream.read_to_end(&mut response);
        response
    }

    #[cfg(feature = "serve")]
    fn resp_command(parts: &[&str]) -> String {
        let mut out = format!("*{}\r\n", parts.len());
        for part in parts {
            out.push_str(&format!("${}\r\n{}\r\n", part.len(), part));
        }
        out
    }

    #[cfg(feature = "serve")]
    fn redis_resp_request(addr: std::net::SocketAddr, request: &str) -> String {
        let mut stream = std::net::TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(30)))
            .unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        stream.flush().unwrap();
        stream
            .shutdown(std::net::Shutdown::Write)
            .expect("shutdown RESP write side");
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
        response
    }

    #[cfg(feature = "serve")]
    fn memcached_text_request(addr: std::net::SocketAddr, request: &str) -> String {
        let mut stream = std::net::TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(30)))
            .unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        stream.flush().unwrap();
        stream
            .shutdown(std::net::Shutdown::Write)
            .expect("shutdown Memcached write side");
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
        response
    }

    #[cfg(feature = "serve")]
    fn kafka_frame<T>(
        api_key: kafka_protocol::messages::ApiKey,
        version: i16,
        request: T,
    ) -> Vec<u8>
    where
        T: kafka_protocol::protocol::Encodable,
    {
        let mut body = bytes::BytesMut::new();
        let header = kafka_protocol::messages::RequestHeader::default()
            .with_request_api_key(api_key as i16)
            .with_request_api_version(version)
            .with_correlation_id(9)
            .with_client_id(Some(kafka_protocol::protocol::StrBytes::from_static_str(
                "loom-test",
            )));
        kafka_protocol::protocol::encode_request_header_into_buffer(&mut body, &header).unwrap();
        request.encode(&mut body, version).unwrap();
        let mut frame = Vec::with_capacity(4 + body.len());
        frame.extend_from_slice(&(body.len() as i32).to_be_bytes());
        frame.extend_from_slice(&body);
        frame
    }

    #[cfg(feature = "serve")]
    fn kafka_request(addr: std::net::SocketAddr, request: Vec<u8>) -> Vec<u8> {
        let mut stream = std::net::TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(30)))
            .unwrap();
        stream.write_all(&request).unwrap();
        stream.flush().unwrap();
        let mut len = [0_u8; 4];
        stream.read_exact(&mut len).unwrap();
        let len = i32::from_be_bytes(len);
        assert!(len >= 0, "{len}");
        let mut response = vec![0_u8; len as usize];
        stream.read_exact(&mut response).unwrap();
        response
    }

    #[cfg(feature = "serve")]
    fn neo4j_selected_version(addr: std::net::SocketAddr) -> u32 {
        let mut stream = std::net::TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(30)))
            .unwrap();
        stream.write_all(&[0x60, 0x60, 0xb0, 0x17]).unwrap();
        stream.write_all(&0x0000_0105_u32.to_be_bytes()).unwrap();
        stream.write_all(&0_u32.to_be_bytes()).unwrap();
        stream.write_all(&0_u32.to_be_bytes()).unwrap();
        stream.write_all(&0_u32.to_be_bytes()).unwrap();
        stream.flush().unwrap();
        let mut selected = [0_u8; 4];
        stream.read_exact(&mut selected).unwrap();
        u32::from_be_bytes(selected)
    }

    #[cfg(feature = "serve")]
    fn decode_kafka_api_versions_response(
        response: Vec<u8>,
        version: i16,
    ) -> kafka_protocol::messages::ApiVersionsResponse {
        use kafka_protocol::protocol::Decodable;

        let mut response = bytes::Bytes::from(response);
        let header = kafka_protocol::messages::ResponseHeader::decode(
            &mut response,
            kafka_protocol::messages::ApiKey::ApiVersions.response_header_version(version),
        )
        .unwrap();
        assert_eq!(header.correlation_id, 9);
        kafka_protocol::messages::ApiVersionsResponse::decode(&mut response, version).unwrap()
    }

    #[cfg(feature = "serve")]
    fn imap_request(addr: std::net::SocketAddr, commands: &str) -> String {
        let mut stream = std::net::TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        stream.write_all(commands.as_bytes()).unwrap();
        stream.flush().unwrap();
        stream
            .shutdown(std::net::Shutdown::Write)
            .expect("shutdown IMAP write side");
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
        response
    }

    #[cfg(feature = "serve")]
    fn imaps_request(addr: std::net::SocketAddr, commands: &str) -> String {
        use rustls::pki_types::pem::PemObject;

        crate::tls_crypto::ensure_rustls_crypto_provider();
        let cert =
            rustls::pki_types::CertificateDer::from_pem_slice(TLS_TEST_CERT.as_bytes()).unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(cert).unwrap();
        let config = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let server_name = rustls::pki_types::ServerName::try_from("localhost").unwrap();
        let tcp = std::net::TcpStream::connect(addr).unwrap();
        tcp.set_read_timeout(Some(std::time::Duration::from_secs(30)))
            .unwrap();
        let conn = rustls::ClientConnection::new(std::sync::Arc::new(config), server_name).unwrap();
        let mut stream = rustls::StreamOwned::new(conn, tcp);
        stream.write_all(commands.as_bytes()).unwrap();
        stream.flush().unwrap();
        stream
            .sock
            .shutdown(std::net::Shutdown::Write)
            .expect("shutdown IMAPS write side");
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
        response
    }

    #[cfg(feature = "serve")]
    fn https_request(addr: std::net::SocketAddr, request: &str) -> String {
        use rustls::pki_types::pem::PemObject;

        crate::tls_crypto::ensure_rustls_crypto_provider();
        let cert =
            rustls::pki_types::CertificateDer::from_pem_slice(TLS_TEST_CERT.as_bytes()).unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(cert).unwrap();
        let config = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let server_name = rustls::pki_types::ServerName::try_from("localhost").unwrap();
        let tcp = std::net::TcpStream::connect(addr).unwrap();
        tcp.set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        let conn = rustls::ClientConnection::new(std::sync::Arc::new(config), server_name).unwrap();
        let mut stream = rustls::StreamOwned::new(conn, tcp);
        stream.write_all(request.as_bytes()).unwrap();
        stream.flush().unwrap();
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
        response
    }

    #[cfg(feature = "mcp")]
    #[test]
    fn daemon_attached_mcp_clients_share_ephemeral_kv() {
        let store = temp_store("daemon-kv");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        let mut loom = Loom::new(fs);
        let ns = WorkspaceId::v4_from_bytes([4; 16]);
        loom.registry_mut()
            .create(FacetKind::Kv, Some("cache-ns"), ns)
            .unwrap();
        loom.configure_kv_map(ns, "sessions", KvMapConfig::EPHEMERAL)
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let paths = daemon::paths(&store).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        std::fs::write(&paths.addr_file, listener.local_addr().unwrap().to_string()).unwrap();
        let coordinator = FileStore::open_read(&store)
            .unwrap()
            .lock_coordinator()
            .unwrap();
        let kv_loom = loom_store::open_loom_read_unlocked(&store, None)
            .and_then(|loom| loom_store::attach_local_auth(loom, &LocalOpenAuth::default()))
            .ok();
        let mut runtime = DaemonRuntime {
            store: paths.store.clone(),
            store_id: paths.store_id.clone(),
            transport: daemon::DaemonTransport::TcpLoopback,
            coordinator,
            kv_loom,
            kv_unavailable: None,
            sessions: std::collections::BTreeSet::new(),
            pins: std::collections::BTreeMap::new(),
            authority_replication_next: std::collections::BTreeMap::new(),
            maintenance_next_ms: 0,
            #[cfg(feature = "serve")]
            hosted_listeners: std::collections::BTreeMap::new(),
            #[cfg(feature = "serve")]
            drive_policy_next_ms: 0,
            #[cfg(feature = "serve")]
            reference_reconcile_next_ms: 0,
        };
        let join = std::thread::spawn(move || {
            for _ in 0..19 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = String::new();
                stream.read_to_string(&mut request).unwrap();
                let response = runtime.handle(&request);
                stream.write_all(response.as_bytes()).unwrap();
            }
        });

        {
            let first = uldren_loom_mcp::LoomMcp::new(
                uldren_loom_mcp::StoreAccess::per_request_attached(&store, None).unwrap(),
            );
            let second = uldren_loom_mcp::LoomMcp::new(
                uldren_loom_mcp::StoreAccess::per_request_attached(&store, None).unwrap(),
            );
            let key = loom_core::key_to_cbor(&loom_core::Value::Text("k".into()));
            first
                .write_kv_put("cache-ns", "sessions", &key, b"v".to_vec())
                .unwrap();
            assert_eq!(
                second.read_kv_get("cache-ns", "sessions", &key).unwrap(),
                Some(b"v".to_vec())
            );
            assert!(
                !second
                    .read_kv_list("cache-ns", "sessions")
                    .unwrap()
                    .is_empty()
            );
            assert!(
                second
                    .write_kv_delete("cache-ns", "sessions", &key)
                    .unwrap()
            );
            assert_eq!(
                first.read_kv_get("cache-ns", "sessions", &key).unwrap(),
                None
            );
        }

        join.join().unwrap();
        let _ = std::fs::remove_file(&paths.addr_file);
        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn daemon_ephemeral_kv_uses_engine_authorization() {
        let store = temp_store("daemon-kv-auth");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        let mut loom = Loom::new(fs);
        let ns = WorkspaceId::v4_from_bytes([4; 16]);
        loom.registry_mut()
            .create(FacetKind::Kv, Some("cache-ns"), ns)
            .unwrap();
        loom.configure_kv_map(ns, "sessions", KvMapConfig::EPHEMERAL)
            .unwrap();
        let root = WorkspaceId::v4_from_bytes([9; 16]);
        let mut identity = IdentityStore::new(root);
        identity
            .set_passphrase(root, "root-pass", b"12345678")
            .unwrap();
        let mut acl = AclStore::new();
        acl.allow(
            AclSubject::Principal(root),
            None,
            None,
            [AclRight::Admin, AclRight::Read, AclRight::Write],
        )
        .unwrap();
        loom.store().save_identity_store(&identity).unwrap();
        loom.store().save_acl_store(&acl).unwrap();
        save_loom(&mut loom).unwrap();

        let paths = daemon::paths(&store).unwrap();
        let coordinator = FileStore::open_read(&store)
            .unwrap()
            .lock_coordinator()
            .unwrap();
        let kv_loom = loom_store::open_loom_read_unlocked(&store, None)
            .and_then(|loom| loom_store::attach_local_auth(loom, &LocalOpenAuth::default()))
            .ok();
        let mut runtime = DaemonRuntime {
            store: paths.store.clone(),
            store_id: paths.store_id.clone(),
            transport: daemon::DaemonTransport::TcpLoopback,
            coordinator,
            kv_loom,
            kv_unavailable: None,
            sessions: std::collections::BTreeSet::new(),
            pins: std::collections::BTreeMap::new(),
            authority_replication_next: std::collections::BTreeMap::new(),
            maintenance_next_ms: 0,
            #[cfg(feature = "serve")]
            hosted_listeners: std::collections::BTreeMap::new(),
            #[cfg(feature = "serve")]
            drive_policy_next_ms: 0,
            #[cfg(feature = "serve")]
            reference_reconcile_next_ms: 0,
        };
        let key = loom_core::key_to_cbor(&loom_core::Value::Text("k".into()));
        let response = runtime.handle(&format!(
            "kv-put\tfake-session\t{ns}\tsessions\t{}\t76\t1\n",
            daemon::hex_encode(&key)
        ));
        assert!(response.starts_with("error\t"));
        assert!(
            response.contains("AUTHENTICATION_FAILED") || response.contains("PERMISSION_DENIED")
        );
        let auth = request_auth_fields(root, "root-pass", "root-session");
        let response = runtime.handle(&format!(
            "kv-put\troot-session\t{ns}\tsessions\t{}\t76\t1\t{auth}\n",
            daemon::hex_encode(&key)
        ));
        assert_eq!(response, "ok\n");
        let _ = std::fs::remove_file(&store);
    }

    fn runtime_for_test_store(store: &str) -> DaemonRuntime {
        let paths = daemon::paths(store).unwrap();
        let coordinator = FileStore::open_read(store)
            .unwrap()
            .lock_coordinator()
            .unwrap();
        let (kv_loom, kv_unavailable) = match daemon_kv_loom(store) {
            Ok(loom) => (Some(loom), None),
            Err(error) => (None, Some(daemon_kv_unavailable_error(error))),
        };
        DaemonRuntime {
            store: paths.store.clone(),
            store_id: paths.store_id.clone(),
            transport: daemon::DaemonTransport::TcpLoopback,
            coordinator,
            kv_loom,
            kv_unavailable,
            sessions: std::collections::BTreeSet::new(),
            pins: std::collections::BTreeMap::new(),
            authority_replication_next: std::collections::BTreeMap::new(),
            maintenance_next_ms: 0,
            #[cfg(feature = "serve")]
            hosted_listeners: std::collections::BTreeMap::new(),
            #[cfg(feature = "serve")]
            drive_policy_next_ms: 0,
            #[cfg(feature = "serve")]
            reference_reconcile_next_ms: 0,
        }
    }

    fn create_search_fixture(store: &str) -> WorkspaceId {
        let fs = FileStore::create_with_profile(store, Algo::Blake3).unwrap();
        let mut loom = Loom::new(fs);
        let ns = WorkspaceId::v4_from_bytes([6; 16]);
        loom.registry_mut()
            .create(FacetKind::Search, Some("main"), ns)
            .unwrap();
        let mut mapping = Mapping::new();
        mapping.insert("title".to_string(), FieldMapping::text());
        let mut search = loom_core::search::SearchCollection::new(mapping);
        let mut document = Document::new();
        document.insert(
            "title".to_string(),
            FieldValue::Text("daemon search".into()),
        );
        search.index(b"doc-1".to_vec(), document);
        loom_core::put_search(&mut loom, ns, "docs", &search).unwrap();
        save_loom(&mut loom).unwrap();
        ns
    }

    #[test]
    fn daemon_fts_status_projects_derived_artifact_readiness() {
        let store = temp_store("daemon-fts-status");
        let ns = create_search_fixture(&store);
        let mut runtime = runtime_for_test_store(&store);
        let response = runtime.handle("fts-status\tmain\tdocs\ttantivy-test\n");
        assert!(
            response.starts_with(&format!("fts\t{ns}\tdocs\t")),
            "{response}"
        );
        assert!(response.contains("\ttantivy-test\tmissing"), "{response}");
        let _ = std::fs::remove_file(&store);
    }

    #[cfg(feature = "native-fts")]
    #[test]
    fn daemon_fts_rebuild_schedules_and_finishes_native_payload() {
        let store = temp_store("daemon-fts-rebuild");
        let ns = create_search_fixture(&store);
        let mut runtime = runtime_for_test_store(&store);
        let response = runtime.handle("fts-rebuild\tmain\tdocs\ttantivy-test\n");
        assert!(
            response.contains("\ttantivy-test\trebuilding")
                || response.contains("\ttantivy-test\tready"),
            "{response}"
        );
        if response.contains("\ttantivy-test\trebuilding") {
            let run_id = response
                .trim_end()
                .split('\t')
                .find_map(|field| field.strip_prefix("run_id="))
                .unwrap();
            assert!(!run_id.is_empty(), "{response}");
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut ready = String::new();
        while std::time::Instant::now() < deadline {
            ready = runtime.handle("fts-status\tmain\tdocs\ttantivy-test\n");
            if ready.contains("\ttantivy-test\tready") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(ready.contains("\ttantivy-test\tready"), "{ready}");
        assert!(ready.contains("payload_digest="), "{ready}");
        assert!(ready.starts_with(&format!("fts\t{ns}\tdocs\t")), "{ready}");
        let second = runtime.handle("fts-rebuild\tmain\tdocs\ttantivy-test\n");
        assert!(second.contains("\ttantivy-test\tready"), "{second}");
        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn encrypted_store_daemon_rejects_runtime_data_without_unlock() {
        let store = temp_store("daemon-encrypted-kv");
        let (meta, session) = EncryptionMeta::create(
            &KeySpec::passphrase("store-pass"),
            Suite::Aes256Gcm,
            [7u8; 16].to_vec(),
            [0x42; 32],
            [9u8; 24].to_vec(),
        )
        .unwrap();
        let fs = FileStore::create_encrypted(&store, meta.encode(), session).unwrap();
        let mut loom = Loom::new(fs);
        let ns = WorkspaceId::v4_from_bytes([4; 16]);
        loom.registry_mut()
            .create(FacetKind::Kv, Some("cache-ns"), ns)
            .unwrap();
        loom.configure_kv_map(ns, "sessions", KvMapConfig::EPHEMERAL)
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let paths = daemon::paths(&store).unwrap();
        let fs = FileStore::open_read(&store).unwrap();
        assert!(fs.is_encrypted());
        let coordinator = fs.lock_coordinator().unwrap();
        let kv_err = daemon_kv_loom(&store).unwrap_err();
        assert_eq!(kv_err.code, loom_core::Code::E2eLocked);
        let mut runtime = DaemonRuntime {
            store: paths.store.clone(),
            store_id: paths.store_id.clone(),
            transport: daemon::DaemonTransport::TcpLoopback,
            coordinator,
            kv_loom: None,
            kv_unavailable: Some(daemon_kv_unavailable_error(kv_err)),
            sessions: std::collections::BTreeSet::new(),
            pins: std::collections::BTreeMap::new(),
            authority_replication_next: std::collections::BTreeMap::new(),
            maintenance_next_ms: 0,
            #[cfg(feature = "serve")]
            hosted_listeners: std::collections::BTreeMap::new(),
            #[cfg(feature = "serve")]
            drive_policy_next_ms: 0,
            #[cfg(feature = "serve")]
            reference_reconcile_next_ms: 0,
        };

        let acquired = runtime.handle("lock-acquire\tkey\tp\ts\texclusive\t1000\t1\n");
        assert!(acquired.starts_with("lock\t"), "{acquired}");
        let key = loom_core::key_to_cbor(&loom_core::Value::Text("k".into()));
        let response = runtime.handle(&format!(
            "kv-put\ts\t{ns}\tsessions\t{}\t76\t1\n",
            daemon::hex_encode(&key)
        ));
        assert!(response.contains("E2E_LOCKED"), "{response}");
        assert!(
            response.contains("delegated encrypted-store credentials"),
            "{response}"
        );
        let _ = std::fs::remove_file(&store);
    }
}
