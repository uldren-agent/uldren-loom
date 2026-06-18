//! rmcp stdio host (feature `server`): the wire registration of the curated tool surface.
//!
//! Every tool in [`crate::tools::TOOL_SURFACE`] is registered here as an rmcp `#[tool]` that calls the
//! read facade ([`crate::reads`]) or write facade ([`crate::writes`]) - so every `tools/call` crosses
//! the engine policy enforcement point. Tool arguments arrive as `Parameters<T>` (deserialized +
//! JSON-schema'd); results return as `Json<serde_json::Value>` (structured content). A drift test
//! asserts the registered tool set equals `TOOL_SURFACE`.

use std::sync::Arc;

#[cfg(feature = "http")]
use axum::response::IntoResponse;
use loom_core::calendar::Occurrence;
use loom_core::error::{Code, LoomError};
use loom_core::mail::MailMessage;
use loom_core::tabular::cell_value;
use loom_core::timeseries::Series;
use loom_core::vcs::{Change, ChangeKind, MergeOutcome, ReplayOutcome, Status};
use loom_core::workspace::{DEFAULT_BRANCH, FacetKind, WorkspaceId, WsSelector};
use loom_core::{AclDomain, AclRight, LiveRootDiagnostics, Loom};
use loom_store::FileStore;
use loom_substrate::admission::WriteAdmissionTarget;
use loom_substrate::lifecycle::{LifecycleOperationLog, lifecycle_operation_log_key};
use loom_substrate::predicate::{CompareOp, Predicate};
use rmcp::ErrorData;
use rmcp::handler::server::prompt::PromptContext;
use rmcp::handler::server::router::prompt::PromptRouter;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::{
    AnnotateAble, ArgumentInfo, CallToolRequestParams, CallToolResult, CompleteRequestParams,
    CompleteResult, CompletionInfo, ErrorCode, ExtensionCapabilities, GetPromptRequestParams,
    GetPromptResult, Implementation, JsonObject, ListPromptsResult, ListResourceTemplatesResult,
    ListResourcesResult, ListToolsResult, Meta, PaginatedRequestParams, ProgressNotificationParam,
    ProgressToken, PromptMessage, PromptMessageRole, RawResource, RawResourceTemplate,
    ReadResourceRequestParams, ReadResourceResult, Reference, Resource, ResourceContents,
    ResourceTemplate, ResourceUpdatedNotificationParam, ServerCapabilities, ServerInfo,
    SubscribeRequestParams, Tool, ToolAnnotations, UnsubscribeRequestParams,
};
use rmcp::service::{NotificationContext, Peer, RequestContext, RoleServer};

use crate::apps::{self, AppMeta};
use crate::pages::{
    PageCreateRequest, StructureBindRequest, StructureCreateRequest, StructureDecomposeItem,
    StructureDecomposeRequest, StructureLinkRequest, StructureMoveRequest, StructureNodeRequest,
};
use crate::resources::{self, ResourceTarget};
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::{ServerHandler, ServiceExt, prompt, prompt_router, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::Notify;

use crate::LoomMcp;
use crate::reads::{StoreSearchReadRequest, TsPoint, VectorSearchPolicyRead};
use crate::tools::{ToolKind, ToolSpec};
use crate::writes::{
    DocumentReplaceTextRequest, GraphEdgeWrite, LaneCreateRequest, LaneDeleteRequest,
    LaneTicketTransferRequest, LaneTicketUpdateRequest, LaneUpdateRequest,
    MeetingsPromoteArtifactToReferenceArtifactRequest, MeetingsPromoteDecisionToDecisionLogRequest,
    MeetingsPromoteQuestionToLifecycleRequest, MeetingsPromoteReferenceToReferenceArtifactRequest,
    MeetingsPromoteTaskToTicketRequest, SubstrateTransactOp, SubstrateViewDefineOwned,
    SubstrateViewDefineRequest, VectorSourceWrite, WriteAdmission, WriteAdmissionPolicyRequest,
};
use loom_tickets::{
    BoardCardMoveRequest, BoardColumn, BoardColumnConfigureRequest, BoardCreateRequest, BoardMode,
    BoardScope, BoardStatus, BoardSwimlane, BoardUpdateRequest, TicketCommentDeleteRequest,
    TicketCommentEvidence, TicketCommentRequest, TicketCommentUpdateRequest, TicketCreateRequest,
    TicketDeleteRequest, TicketFieldDefinitionRetireRequest, TicketFieldDefinitionWriteRequest,
    TicketLifecycleAction, TicketLifecycleAuthorizationPolicy, TicketRelationKind,
    TicketRelationRemoveRequest, TicketRelationRequest, TicketUpdateRequest,
};

pub mod conformance;

type ToolResult = Result<Json<Value>, ErrorData>;
const DEFAULT_RESULT_LIMIT: usize = 500;
const DEFAULT_RESOURCE_READ_MAX_BYTES: usize = 4 * 1024 * 1024;
#[cfg(feature = "http")]
pub type HttpNetworkAccess = Arc<
    dyn for<'a> Fn(std::net::SocketAddr, Option<&'a str>, Option<&'a str>) -> bool
        + Send
        + Sync
        + 'static,
>;
const APP_OPEN_TOOL: &str = "apps.open";

fn ticket_comment_evidence_from_map(
    evidence: &BTreeMap<String, Vec<String>>,
) -> loom_core::Result<TicketCommentEvidence> {
    let value = serde_json::Value::Object(
        evidence
            .iter()
            .map(|(key, values)| {
                (
                    key.clone(),
                    serde_json::Value::Array(
                        values
                            .iter()
                            .cloned()
                            .map(serde_json::Value::String)
                            .collect(),
                    ),
                )
            })
            .collect(),
    );
    TicketCommentEvidence::from_json(&value)
}
const APP_LAUNCH_PREFIX: &str = "apps.launch.";
/// Document-facet collection holding ask state (the pending ask plus one archive doc per ask id).
const ASK_COLLECTION: &str = "loom.ask";
/// Document id of the ask the internal Ask app renders.
const ASK_CURRENT_DOC: &str = "current";
const ASK_POLL_INTERVAL: Duration = Duration::from_millis(400);
const ASK_WAIT_DEFAULT_MS: u64 = 600_000;
const ASK_WAIT_MAX_MS: u64 = 3_600_000;
const MCP_UI_EXTENSION: &str = "io.modelcontextprotocol/ui";
const DAEMON_LIVENESS_POLL: Duration = Duration::from_millis(250);
const MCP_SHUTDOWN_GRACE: Duration = Duration::from_secs(30);

fn err(e: LoomError) -> ErrorData {
    let message = e.to_string();
    let details = if e.details.is_empty() {
        None
    } else {
        serde_json::to_value(json!({ "details": e.details })).ok()
    };
    ErrorData::internal_error(message, details)
}

/// Build a [`loom_lanes::LaneTicketPlacement`] from the wire placement verb and optional anchor.
/// Defaults to append; "before"/"after" require an anchor; unknown verbs are rejected.
fn lane_ticket_placement<'a>(
    placement: Option<&str>,
    anchor: Option<&'a str>,
) -> Result<loom_lanes::LaneTicketPlacement<'a>, LoomError> {
    loom_lanes::LaneTicketPlacement::parse(placement.unwrap_or("append"), anchor)
}

fn board_columns(columns: Vec<PBoardColumn>) -> Result<Vec<BoardColumn>, LoomError> {
    columns
        .into_iter()
        .map(|column| {
            BoardColumn::with_display(
                column.column_id,
                column.name,
                column.mapped_statuses.into_iter().collect(),
                column.wip_limit,
                column.hidden,
                column.rank,
            )
        })
        .collect()
}

fn board_swimlanes(swimlanes: Vec<PBoardSwimlane>) -> Result<Vec<BoardSwimlane>, LoomError> {
    swimlanes
        .into_iter()
        .map(|swimlane| {
            BoardSwimlane::new(
                swimlane.swimlane_id,
                swimlane.name,
                swimlane.predicate,
                swimlane.rank,
            )
        })
        .collect()
}

fn board_scope(kind: &str, project_id: &str) -> Result<BoardScope, LoomError> {
    match kind {
        "project" => Ok(BoardScope::project(project_id.to_string())),
        "manual_set" => Ok(BoardScope::ManualSet),
        _ => Err(LoomError::invalid(
            "board scope must be project or manual_set",
        )),
    }
}

fn drive_conflict_resolution(
    value: &str,
) -> Result<crate::drive::DriveConflictResolutionRequest, LoomError> {
    match value {
        "keep_current" => Ok(crate::drive::DriveConflictResolutionRequest::Current),
        "keep_conflict" => Ok(crate::drive::DriveConflictResolutionRequest::Conflict),
        "keep_both" => Ok(crate::drive::DriveConflictResolutionRequest::Both),
        _ => Err(LoomError::invalid(
            "drive conflict resolution must be keep_current, keep_conflict, or keep_both",
        )),
    }
}

fn fence(value: crate::server::params::PFence) -> loom_core::Fence {
    loom_core::Fence::new(value.authority, value.epoch, value.sequence)
}

fn write_admission(value: Option<PWriteAdmission>) -> Option<WriteAdmission> {
    value.map(|admission| WriteAdmission {
        target_kind: admission.target_kind,
        target_id: admission.target_id,
        fence: fence(admission.fence),
    })
}

fn workspace_profile_id(mcp: &LoomMcp, workspace: &str) -> Result<String, LoomError> {
    mcp.store().read(|loom| {
        let ns = crate::reads::resolve_ns(loom, workspace)?;
        Ok(ns.to_string())
    })
}

fn ser<T: serde::Serialize>(v: T) -> ToolResult {
    serde_json::to_value(v)
        .map(out_value)
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))
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
) -> loom_core::error::Result<LiveRootDiagnostics> {
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

fn live_root_diagnostics_json(diagnostics: &LiveRootDiagnostics) -> Value {
    json!({
        "sample_limit": diagnostics.sample_limit,
        "classes": diagnostics.classes.iter().map(|class| {
            json!({
                "class": class.class,
                "count": class.count,
                "examples": class.examples.iter().map(|example| {
                    json!({
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
    report: &loom_store::StoreMaintenanceReport,
    diagnostics: Option<&LiveRootDiagnostics>,
) -> Value {
    let policy = json!({
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
    let run_state = json!({
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
    let mut value = json!({
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

fn run_mcp_store_maintenance_once(
    loom: &mut Loom<FileStore>,
    now: u64,
    manual: bool,
    max_segments: Option<u64>,
    max_pages: Option<u64>,
) -> Result<Value, LoomError> {
    let mut policy = loom.store().store_maintenance_policy()?;
    if let Some(value) = max_segments {
        if value == 0 {
            return Err(LoomError::invalid("max_segments must be nonzero"));
        }
        policy.max_segments = value;
    }
    if let Some(value) = max_pages {
        if value == 0 {
            return Err(LoomError::invalid("max_pages must be nonzero"));
        }
        policy.max_pages = value;
    }
    let report = loom.store().store_maintenance_report(now)?;
    if !manual && !report.eligible {
        return Ok(json!({"outcome": "skipped", "reason": "not_eligible"}));
    }
    let mut active = loom.store().active_reachability_mark_epoch()?;
    if let Some(epoch) = &active
        && let Err(error) = loom.store().validate_reachability_mark_epoch_current(epoch)
    {
        if error.code != Code::Conflict {
            return Err(error);
        }
        loom.store().clear_reachability_mark_epoch()?;
        active = None;
    }
    let needs_mark = active
        .as_ref()
        .map(|epoch| !epoch.state.completed)
        .unwrap_or(true);
    if needs_mark {
        if active.is_none() {
            loom_store::begin_loom_reachability_mark_epoch(loom)?;
        }
        let step = loom_store::step_loom_reachability_mark_epoch(loom, 1024)?;
        if !step.completed {
            loom.store().record_store_maintenance_run_state(
                loom_store::StoreMaintenanceRunState {
                    last_run_ms: Some(now),
                    next_eligible_ms: now.saturating_add(policy.interval_ms),
                    last_skip_reason: Some("mark_epoch_incomplete".to_string()),
                    last_error: None,
                    ..loom_store::StoreMaintenanceRunState::default()
                },
            )?;
            return Ok(json!({
                "outcome": "marked",
                "visited": step.visited,
                "pending": step.pending,
            }));
        }
    }
    let mut tail_trim_attempted = false;
    let mut tail_trim_pages = 0;
    let mut tail_trim_bytes = 0;
    let mut tail_compaction = loom_store::TailCompactionStats::default();
    let outcome = if policy.full_compaction_enabled
        && maintenance_debt_thresholds_met(&policy, &report.status)
    {
        let capacity = loom.store().ensure_compaction_capacity()?;
        let stats = loom_store::gc_loom(loom)?;
        json!({
            "outcome": "compacted",
            "before": stats.before,
            "after": stats.after,
            "reclaimed": stats.reclaimed(),
            "required_temp_bytes": capacity.required_temp_bytes,
            "available_temp_bytes": capacity.available_temp_bytes,
        })
    } else {
        let budget = loom_store::GcSegmentBudget {
            max_segments: policy.max_segments,
            max_pages: policy.max_pages,
        };
        let stats = if policy.tail_trim_enabled {
            tail_trim_attempted = true;
            loom.store_mut().gc_validated_segments(budget)
        } else {
            loom.store_mut()
                .gc_validated_segments_without_tail_trim(budget)
        }?;
        tail_trim_pages = stats.pages_trimmed;
        tail_trim_bytes = stats
            .pages_trimmed
            .saturating_mul(loom_store::STORE_PAGE_SIZE);
        if policy.tail_compaction_enabled {
            tail_compaction = loom.store_mut().compact_tail_once(
                policy.tail_compaction_max_pages,
                policy.tail_compaction_max_objects,
                policy.tail_compaction_max_bytes,
            )?;
            if tail_compaction.truncated_pages > 0 {
                tail_trim_attempted = true;
                tail_trim_pages = tail_trim_pages.saturating_add(tail_compaction.truncated_pages);
                tail_trim_bytes = tail_trim_pages.saturating_mul(loom_store::STORE_PAGE_SIZE);
            }
        }
        json!({
            "outcome": "reclaimed",
            "segments_reclaimed": stats.segments_reclaimed,
            "pages_freed": stats.pages_freed,
            "tail_trim_pages": tail_trim_pages,
            "tail_trim_bytes": tail_trim_bytes,
            "tail_compaction_attempted": tail_compaction.attempted,
            "tail_compaction_relocated_objects": tail_compaction.relocated_objects,
            "tail_compaction_relocated_pages": tail_compaction.relocated_pages,
            "tail_compaction_relocated_bytes": tail_compaction.relocated_bytes,
            "tail_compaction_truncated_pages": tail_compaction.truncated_pages,
            "tail_compaction_conflicts": tail_compaction.conflicts,
            "objects_relocated": stats.objects_relocated,
            "objects_dropped": stats.objects_dropped,
        })
    };
    loom.store()
        .record_store_maintenance_run_state(loom_store::StoreMaintenanceRunState {
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
        })?;
    Ok(outcome)
}

fn ser_public_lane_envelope(
    envelope: loom_types::MutationEnvelope<loom_lanes::Lane>,
) -> ToolResult {
    ser(loom_types::MutationEnvelope::new(
        loom_lanes::public_lane(&envelope.resource),
        envelope.receipt,
    ))
}

fn promoted_public_lane_envelope_bytes(
    envelope: loom_types::MutationEnvelope<loom_lanes::Lane>,
) -> Result<Vec<u8>, LoomError> {
    promoted_result_bytes(loom_types::MutationEnvelope::new(
        loom_lanes::public_lane(&envelope.resource),
        envelope.receipt,
    ))
}

/// Build the native bounded-list query from the MCP `tickets_list` params, resolving first-class Lane
/// membership (`--lane`) into a ticket-id allowlist via loom-lanes.
fn build_ticket_list_query(
    mcp: &LoomMcp,
    a: &params::PTicketsList,
) -> Result<loom_tickets::TicketListQuery, LoomError> {
    let projection = loom_tickets::parse_ticket_projection(a.projection.as_deref())?;
    let lane_member_ids = match a.lane.as_deref() {
        Some(lane_id) => Some(
            mcp.read_lanes_get(&a.workspace, lane_id)?
                .ok_or_else(|| {
                    LoomError::new(Code::NotFound, format!("lane {lane_id:?} not found"))
                })?
                .lane_tickets
                .iter()
                .map(|ticket| ticket.ticket_id.clone())
                .collect::<Vec<_>>(),
        ),
        None => None,
    };
    Ok(loom_tickets::TicketListQuery {
        projection,
        statuses: a.statuses.clone(),
        assignees: a.assignees.clone(),
        priorities: a.priorities.clone(),
        ticket_types: a.ticket_types.clone(),
        labels: a.labels.clone(),
        policy_labels: a.policy_labels.clone(),
        ready_only: a.ready,
        include_completed: a.include_completed,
        lane_member_ids,
        board_id: a.board.clone(),
        cursor: a.cursor.clone(),
        limit: a.limit,
    })
}

fn out_value(v: Value) -> Json<Value> {
    Json(json!({ "value": v }))
}

fn json_object_value(fields: std::collections::BTreeMap<String, Value>) -> Value {
    Value::Object(fields.into_iter().collect())
}

fn ticket_field_cardinality(
    value: &str,
) -> Result<loom_tickets::TicketFieldCardinality, LoomError> {
    match value {
        "single" => Ok(loom_tickets::TicketFieldCardinality::Single),
        "optional" => Ok(loom_tickets::TicketFieldCardinality::Optional),
        "list" => Ok(loom_tickets::TicketFieldCardinality::List {
            min_items: 0,
            max_items: None,
        }),
        _ => Err(LoomError::invalid(
            "ticket field cardinality must be single, optional, or list",
        )),
    }
}

fn result_limit(page: &PResultPage) -> Result<usize, ErrorData> {
    match page.limit {
        Some(0) => Err(ErrorData::invalid_params(
            "limit must be greater than zero",
            None,
        )),
        Some(limit) => Ok(limit),
        None => Ok(DEFAULT_RESULT_LIMIT),
    }
}

fn result_offset(page: &PResultPage) -> usize {
    page.offset.unwrap_or(0)
}

fn slice_results<T>(items: Vec<T>, page: &PResultPage) -> Result<Vec<T>, ErrorData> {
    let limit = result_limit(page)?;
    let offset = result_offset(page);
    Ok(items.into_iter().skip(offset).take(limit).collect())
}

fn ensure_delivered_budget(
    operation: &str,
    actual_bytes: usize,
    max_bytes: usize,
) -> Result<(), ErrorData> {
    if actual_bytes <= max_bytes {
        return Ok(());
    }
    Err(ErrorData::invalid_params(
        format!(
            "{operation} result exceeds delivered payload budget: {actual_bytes} bytes > {max_bytes} bytes; narrow the request, lower the range, use limit/offset, or use a chunked/app-specific path"
        ),
        None,
    ))
}

fn budgeted_out_value(operation: &str, value: Value, max_bytes: Option<usize>) -> ToolResult {
    let budget = max_bytes.unwrap_or(DEFAULT_RESOURCE_READ_MAX_BYTES);
    let delivered = serde_json::to_vec(&json!({ "value": &value }))
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
    ensure_delivered_budget(operation, delivered.len(), budget)?;
    Ok(out_value(value))
}

fn promoted_args<T: serde::de::DeserializeOwned>(args_json: &[u8]) -> Result<T, LoomError> {
    serde_json::from_slice(args_json)
        .map_err(|e| LoomError::new(Code::InvalidArgument, format!("decode tool arguments: {e}")))
}

fn promoted_result_bytes<T: serde::Serialize>(value: T) -> Result<Vec<u8>, LoomError> {
    let v = serde_json::to_value(value)
        .map_err(|e| LoomError::new(Code::Internal, format!("encode tool result: {e}")))?;
    serde_json::to_vec(&json!({ "value": v }))
        .map_err(|e| LoomError::new(Code::Internal, format!("encode tool result: {e}")))
}

/// Read an ask document by id, or `None` if absent.
fn ask_read_doc(mcp: &LoomMcp, workspace: &str, id: &str) -> Result<Option<Value>, LoomError> {
    let Some(bytes) = mcp
        .read_document_get_binary(workspace, ASK_COLLECTION, id)?
        .map(|document| document.bytes)
    else {
        return Ok(None);
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| LoomError::new(Code::CorruptObject, format!("malformed ask document: {e}")))
}

/// Persist an ask document under `id`.
fn ask_write_doc(mcp: &LoomMcp, workspace: &str, id: &str, doc: &Value) -> Result<(), LoomError> {
    let bytes =
        serde_json::to_vec(doc).map_err(|e| LoomError::new(Code::Internal, e.to_string()))?;
    mcp.write_document_put(workspace, ASK_COLLECTION, id, bytes)
}

fn normalize_ask_questions(questions: &[params::PAskQuestion]) -> Vec<Value> {
    questions
        .iter()
        .map(|q| {
            json!({
                "question": q.question,
                "context": q.context,
                "examples": q.examples,
                "options": q.options.as_deref().unwrap_or(&[]).iter().map(|o| json!({
                    "label": o.label,
                    "description": o.description,
                })).collect::<Vec<_>>(),
                "recommendation": q.recommendation,
                "shape": q.shape,
            })
        })
        .collect()
}

fn normalize_ask_answers(answers: &[params::PAskAnswer]) -> Vec<Value> {
    answers
        .iter()
        .map(|a| {
            json!({
                "index": a.index,
                "status": a.status,
                "selected": a.selected.clone().unwrap_or_default(),
                "text": a.text.clone().unwrap_or_default(),
            })
        })
        .collect()
}

fn ask_begin(mcp: &LoomMcp, workspace: &str, questions: Vec<Value>) -> Result<Value, LoomError> {
    if questions.is_empty() {
        return Err(LoomError::new(
            Code::InvalidArgument,
            "questions must not be empty",
        ));
    }
    for (index, q) in questions.iter().enumerate() {
        let shape = q.get("shape").and_then(Value::as_str).unwrap_or_default();
        if !matches!(shape, "radio" | "checkbox" | "text") {
            return Err(LoomError::new(
                Code::InvalidArgument,
                format!("question {index}: shape must be radio, checkbox, or text"),
            ));
        }
        let has_options = q
            .get("options")
            .and_then(Value::as_array)
            .is_some_and(|o| !o.is_empty());
        if shape != "text" && !has_options {
            return Err(LoomError::new(
                Code::InvalidArgument,
                format!("question {index}: {shape} questions require options"),
            ));
        }
    }
    let id = next_ask_id();
    let doc = json!({
        "id": id,
        "status": "pending",
        "created_ms": crate::now_ms(),
        "questions": questions,
        "answers": [],
    });
    ask_write_doc(mcp, workspace, &id, &doc)?;
    ask_write_doc(mcp, workspace, ASK_CURRENT_DOC, &doc)?;
    Ok(doc)
}

/// A single-shot poll of ask `id`: `{id, status, answers}` or `None` if the ask is unknown.
fn ask_poll_state(mcp: &LoomMcp, workspace: &str, id: &str) -> Result<Option<Value>, LoomError> {
    let Some(doc) = ask_read_doc(mcp, workspace, id)? else {
        return Ok(None);
    };
    let status = doc
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("pending");
    Ok(Some(json!({
        "id": id,
        "status": status,
        "answers": doc.get("answers").cloned().unwrap_or_else(|| json!([])),
    })))
}

/// Record `answers` for pending ask `id` (or mark it aborted), updating both the ask document and the
/// `current` pointer when it still points at this ask. Shared by the local handler and the server route.
fn ask_submit(
    mcp: &LoomMcp,
    workspace: &str,
    id: &str,
    answers: &[Value],
    aborted: bool,
) -> Result<(), LoomError> {
    let Some(mut doc) = ask_read_doc(mcp, workspace, id)? else {
        return Err(LoomError::new(Code::NotFound, format!("unknown ask {id}")));
    };
    if doc.get("status").and_then(Value::as_str) != Some("pending") {
        return Err(LoomError::new(
            Code::InvalidArgument,
            format!("ask {id} is not pending"),
        ));
    }
    let questions = doc
        .get("questions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut out: Vec<Value> = questions
        .iter()
        .map(|q| {
            json!({
                "question": q.get("question").cloned().unwrap_or(Value::Null),
                "status": "skipped",
                "selected": [],
                "text": "",
            })
        })
        .collect();
    for answer in answers {
        let index = answer
            .get("index")
            .and_then(Value::as_u64)
            .unwrap_or(u64::MAX) as usize;
        let Some(entry) = out.get_mut(index) else {
            return Err(LoomError::new(
                Code::InvalidArgument,
                format!("answer index {index} is out of range"),
            ));
        };
        let a_status = answer
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !matches!(a_status, "answered" | "skipped") {
            return Err(LoomError::new(
                Code::InvalidArgument,
                format!("answer {index}: status must be answered or skipped"),
            ));
        }
        let answered = !aborted && a_status == "answered";
        if let Some(map) = entry.as_object_mut() {
            map.insert(
                "status".to_string(),
                Value::String(if answered { "answered" } else { "skipped" }.to_string()),
            );
            if answered {
                map.insert(
                    "selected".to_string(),
                    answer.get("selected").cloned().unwrap_or_else(|| json!([])),
                );
                map.insert(
                    "text".to_string(),
                    Value::String(
                        answer
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    ),
                );
            }
        }
    }
    let status = if aborted { "aborted" } else { "answered" };
    if let Some(map) = doc.as_object_mut() {
        map.insert("status".to_string(), Value::String(status.to_string()));
        map.insert("answers".to_string(), Value::Array(out));
        map.insert("submitted_ms".to_string(), json!(crate::now_ms()));
    }
    ask_write_doc(mcp, workspace, id, &doc)?;
    let current_is_this_ask = ask_read_doc(mcp, workspace, ASK_CURRENT_DOC)?
        .and_then(|current| {
            current
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .is_some_and(|current_id| current_id == id);
    if current_is_this_ask {
        ask_write_doc(mcp, workspace, ASK_CURRENT_DOC, &doc)?;
    }
    Ok(())
}

/// # Errors
/// Returns [`LoomError`] for malformed arguments, an unknown/unpromoted tool, or a domain failure.
pub fn execute_promoted_tool(
    mcp: &LoomMcp,
    name: &str,
    args_json: &[u8],
) -> Result<Vec<u8>, LoomError> {
    match name {
        "apps_list" => {
            let a: crate::server::params::PNs = promoted_args(args_json)?;
            let mut items = mcp.read_mcp_app_inventory(&a.workspace)?;
            // No local UX binding on the server; emit fully-qualified app URIs (unbound host form).
            for item in &mut items {
                if item.uri.is_some() {
                    item.uri = Some(apps::app_uri(&item.workspace, &item.app, false));
                }
            }
            promoted_result_bytes(items)
        }
        "apps_show" => {
            let a: crate::server::params::PApp = promoted_args(args_json)?;
            let resource = mcp.read_mcp_app_show(&a.workspace, &a.app)?.map(|mut r| {
                r.uri = apps::app_uri(&r.workspace, &r.app, false);
                r
            });
            promoted_result_bytes(resource)
        }
        "apps_read_file" => {
            let a: crate::server::params::PAppPath = promoted_args(args_json)?;
            promoted_result_bytes(mcp.read_mcp_app_file(&a.workspace, &a.app, &a.path)?)
        }
        "apps_create" => {
            let a: crate::server::params::PAppCreate = promoted_args(args_json)?;
            promoted_result_bytes(mcp.write_mcp_app_create(
                &a.workspace,
                &a.app,
                &a.index_html,
                &a.meta_md,
            )?)
        }
        "apps_write_file" => {
            let a: crate::server::params::PAppWrite = promoted_args(args_json)?;
            promoted_result_bytes(mcp.write_mcp_app_write_file(
                &a.workspace,
                &a.app,
                &a.path,
                &a.content,
                a.mode,
            )?)
        }
        "apps_remove_file" => {
            let a: crate::server::params::PAppPath = promoted_args(args_json)?;
            promoted_result_bytes(mcp.write_mcp_app_remove_file(&a.workspace, &a.app, &a.path)?)
        }
        "drive_list" => {
            let a: params::PDriveFolder = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_drive_list(&a.workspace, &pid, &a.folder_id)?)
        }
        "drive_stat" => {
            let a: params::PDriveStat = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_drive_stat(&a.workspace, &pid, &a.folder_id, &a.name)?)
        }
        "drive_read" => {
            let a: params::PDriveFile = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_drive_read(&a.workspace, &pid, &a.file_id)?)
        }
        "drive_list_versions" => {
            let a: params::PDriveFile = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_drive_list_versions(&a.workspace, &pid, &a.file_id)?)
        }
        "drive_list_conflicts" => {
            let a: params::PDriveConflicts = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_drive_list_conflicts(&a.workspace, &pid)?)
        }
        "drive_list_shares" => {
            let a: params::PDriveConflicts = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_drive_list_shares(&a.workspace, &pid)?)
        }
        "drive_list_retention" => {
            let a: params::PDriveConflicts = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_drive_list_retention(&a.workspace, &pid)?)
        }
        "drive_grant_share" => {
            let a: params::PDriveShareGrant = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_drive_grant_share(
                &a.workspace,
                crate::drive::DriveGrantShareRequest {
                    workspace_id: &pid,
                    grant_id: &a.grant_id,
                    target_kind: &a.target_kind,
                    target_id: &a.target_id,
                    principal: &a.principal,
                    role: &a.role,
                    granted_at_ms: crate::now_ms(),
                    expires_at_ms: a.expires_at_ms,
                },
            )?)
        }
        "drive_revoke_share" => {
            let a: params::PDriveShareRevoke = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_drive_revoke_share(&a.workspace, &pid, &a.grant_id)?)
        }
        "drive_apply_share_expiry" => {
            let a: params::PDriveShareExpiryApply = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_drive_apply_share_expiry(
                &a.workspace,
                &pid,
                a.now_ms,
            )?)
        }
        "drive_pin_retention" => {
            let a: params::PDriveRetentionPin = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_drive_pin_retention(
                &a.workspace,
                crate::drive::DrivePinRetentionRequest {
                    workspace_id: &pid,
                    pin_id: &a.pin_id,
                    kind: &a.kind,
                    root: &a.root,
                    target_entity_id: a.target_entity_id.as_deref(),
                    added_at_ms: crate::now_ms(),
                    expires_at_ms: a.expires_at_ms,
                },
            )?)
        }
        "drive_unpin_retention" => {
            let a: params::PDriveRetentionUnpin = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_drive_unpin_retention(&a.workspace, &pid, &a.pin_id)?)
        }
        "drive_apply_retention" => {
            let a: params::PDriveRetentionApply = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_drive_apply_retention(&a.workspace, &pid, a.now_ms)?)
        }
        "drive_acquire_lease" => {
            let a: params::PDriveAcquireLease = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_drive_acquire_lease(
                &a.workspace,
                &pid,
                &a.target_kind,
                &a.target_id,
                a.lease_ms,
                a.wait_ms,
            )?)
        }
        "drive_refresh_lease" => {
            let a: params::PDriveRefreshLease = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_drive_refresh_lease(
                &a.workspace,
                &pid,
                &a.target_kind,
                &a.target_id,
                fence(a.fence),
                a.lease_ms,
            )?)
        }
        "drive_release_lease" => {
            let a: params::PDriveReleaseLease = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_drive_release_lease(
                &a.workspace,
                &pid,
                &a.target_kind,
                &a.target_id,
                fence(a.fence),
            )?)
        }
        "drive_break_lease" => {
            let a: params::PDriveBreakLease = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_drive_break_lease(
                &a.workspace,
                &pid,
                &a.target_kind,
                &a.target_id,
            )?)
        }
        "drive_create_folder" => {
            let a: params::PDriveCreateFolder = promoted_args(args_json)?;
            let admission = write_admission(a.write_admission);
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_drive_create_folder(
                &a.workspace,
                &pid,
                &a.parent_folder_id,
                &a.folder_id,
                &a.name,
                &a.expected_root,
                admission.as_ref(),
            )?)
        }
        "drive_create_upload" => {
            let a: params::PDriveCreateUpload = promoted_args(args_json)?;
            let admission = write_admission(a.write_admission);
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_drive_create_upload(
                &a.workspace,
                crate::drive::DriveCreateUploadRequest {
                    workspace_id: &pid,
                    upload_id: &a.upload_id,
                    parent_folder_id: &a.parent_folder_id,
                    name: &a.name,
                    file_id: &a.file_id,
                    expected_root: &a.expected_root,
                    created_at_ms: crate::now_ms(),
                    replace_file: a.replace_file,
                },
                admission.as_ref(),
            )?)
        }
        "drive_upload_chunk" => {
            let a: params::PDriveUploadChunk = promoted_args(args_json)?;
            let admission = write_admission(a.write_admission);
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_drive_upload_chunk(
                &a.workspace,
                &pid,
                &a.upload_id,
                &a.bytes,
                admission.as_ref(),
            )?)
        }
        "drive_commit_upload" => {
            let a: params::PDriveCommitUpload = promoted_args(args_json)?;
            let admission = write_admission(a.write_admission);
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_drive_commit_upload(
                &a.workspace,
                &pid,
                &a.upload_id,
                admission.as_ref(),
            )?)
        }
        "drive_rename" => {
            let a: params::PDriveRename = promoted_args(args_json)?;
            let admission = write_admission(a.write_admission);
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_drive_rename(
                &a.workspace,
                &pid,
                &a.folder_id,
                &a.node_id,
                &a.new_name,
                &a.expected_root,
                admission.as_ref(),
            )?)
        }
        "drive_move" => {
            let a: params::PDriveMove = promoted_args(args_json)?;
            let admission = write_admission(a.write_admission);
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_drive_move(
                &a.workspace,
                &pid,
                &a.source_folder_id,
                &a.target_folder_id,
                &a.node_id,
                &a.expected_root,
                admission.as_ref(),
            )?)
        }
        "drive_delete" => {
            let a: params::PDriveDelete = promoted_args(args_json)?;
            let admission = write_admission(a.write_admission);
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_drive_delete(
                &a.workspace,
                &pid,
                &a.folder_id,
                &a.node_id,
                &a.expected_root,
                admission.as_ref(),
            )?)
        }
        "drive_resolve_conflict" => {
            let a: params::PDriveResolveConflict = promoted_args(args_json)?;
            let resolution = drive_conflict_resolution(&a.resolution)?;
            let admission = write_admission(a.write_admission);
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_drive_resolve_conflict(
                &a.workspace,
                &pid,
                &a.conflict_id,
                resolution,
                admission.as_ref(),
            )?)
        }
        "meetings_projection_outputs" => {
            let a: params::PMeetingsProfile = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_meetings_projection_outputs(&a.workspace, &pid)?)
        }
        "meetings_list" => {
            let a: params::PMeetingsList = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_meetings_list(
                &a.workspace,
                &pid,
                a.limit.unwrap_or(100),
                a.offset.unwrap_or(0),
            )?)
        }
        "meetings_get" => {
            let a: params::PMeetingsGet = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_meetings_get(&a.workspace, &pid, &a.meeting_id)?)
        }
        "meetings_search" => {
            let a: params::PMeetingsSearch = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_meetings_search(
                &a.workspace,
                &pid,
                &a.query,
                a.field.as_deref(),
                a.limit.unwrap_or(20),
                a.offset.unwrap_or(0),
            )?)
        }
        "meetings_extraction_review" => {
            let a: params::PMeetingsProfile = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_meetings_extraction_review(&a.workspace, &pid)?)
        }
        "meetings_accept_annotation" => {
            let a: params::PMeetingsAnnotation = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_meetings_accept_annotation(
                &a.workspace,
                &pid,
                &a.annotation_id,
            )?)
        }
        "meetings_reject_annotation" => {
            let a: params::PMeetingsAnnotation = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_meetings_reject_annotation(
                &a.workspace,
                &pid,
                &a.annotation_id,
            )?)
        }
        "meetings_propose_vocabulary" => {
            let a: params::PMeetingsVocabularyPropose = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_meetings_propose_vocabulary(
                &a.workspace,
                &pid,
                loom_substrate::meetings::VocabularyTermInput {
                    term_id: &a.term_id,
                    kind: &a.kind,
                    label: &a.label,
                    evidence_annotation_ids: a.evidence_annotation_ids,
                    created_at_ms: crate::now_ms(),
                },
                a.aliases.unwrap_or_default(),
            )?)
        }
        "meetings_accept_vocabulary" => {
            let a: params::PMeetingsVocabulary = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_meetings_accept_vocabulary(
                &a.workspace,
                &pid,
                &a.term_id,
            )?)
        }
        "meetings_reject_vocabulary" => {
            let a: params::PMeetingsVocabulary = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_meetings_reject_vocabulary(
                &a.workspace,
                &pid,
                &a.term_id,
            )?)
        }
        "meetings_add_entity_merge" => {
            let a: params::PMeetingsEntityMerge = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_meetings_add_entity_merge(
                &a.workspace,
                &pid,
                &a.merge_id,
                &a.canonical_entity_id,
                a.merged_entity_ids,
                a.evidence_annotation_ids,
            )?)
        }
        "meetings_add_promotion" => {
            let a: params::PMeetingsPromotion = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_meetings_add_promotion(
                &a.workspace,
                &pid,
                &a.promotion_id,
                &a.operation_kind,
                &a.source_annotation_id,
                &a.target_profile,
                &a.target_entity_ref,
            )?)
        }
        "meetings_promote_task_to_ticket" => {
            let a: params::PMeetingsPromoteTaskToTicket = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_meetings_promote_task_to_ticket(
                &a.workspace,
                &pid,
                MeetingsPromoteTaskToTicketRequest {
                    promotion_id: &a.promotion_id,
                    source_annotation_id: &a.source_annotation_id,
                    project_id: &a.project_id,
                    ticket_type: &a.ticket_type,
                    policy_labels: &a.policy_labels,
                    expected_ticket_root: a.expected_ticket_root.as_deref(),
                },
            )?)
        }
        "meetings_promote_decision_to_decision_log" => {
            let a: params::PMeetingsPromoteDecisionToDecisionLog = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_meetings_promote_decision_to_decision_log(
                &a.workspace,
                &pid,
                MeetingsPromoteDecisionToDecisionLogRequest {
                    promotion_id: &a.promotion_id,
                    source_annotation_id: &a.source_annotation_id,
                    decision_id: &a.decision_id,
                    ledger_name: &a.ledger_name,
                },
            )?)
        }
        "meetings_promote_question_to_lifecycle" => {
            let a: params::PMeetingsPromoteQuestionToLifecycle = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_meetings_promote_question_to_lifecycle(
                &a.workspace,
                &pid,
                MeetingsPromoteQuestionToLifecycleRequest {
                    promotion_id: &a.promotion_id,
                    source_annotation_id: &a.source_annotation_id,
                    instance_id: &a.instance_id,
                    definition_id: &a.definition_id,
                },
            )?)
        }
        "meetings_promote_artifact_to_reference_artifact" => {
            let a: params::PMeetingsPromoteArtifactToReferenceArtifact = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_meetings_promote_artifact_to_reference_artifact(
                &a.workspace,
                &pid,
                MeetingsPromoteArtifactToReferenceArtifactRequest {
                    promotion_id: &a.promotion_id,
                    source_annotation_id: &a.source_annotation_id,
                    artifact_id: &a.artifact_id,
                    target_ref: a.target_ref.as_deref(),
                },
            )?)
        }
        "meetings_promote_reference_to_reference_artifact" => {
            let a: params::PMeetingsPromoteReferenceToReferenceArtifact = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_meetings_promote_reference_to_reference_artifact(
                &a.workspace,
                &pid,
                MeetingsPromoteReferenceToReferenceArtifactRequest {
                    promotion_id: &a.promotion_id,
                    source_annotation_id: &a.source_annotation_id,
                    reference_id: &a.reference_id,
                    target_ref: a.target_ref.as_deref(),
                },
            )?)
        }
        "meetings_import_snapshot" => {
            let a: params::PMeetingsImportSnapshot = promoted_args(args_json)?;
            promoted_result_bytes(mcp.write_meetings_import_snapshot(
                &a.workspace,
                &a.input_profile,
                &a.snapshot,
                a.dry_run.unwrap_or(false),
            )?)
        }
        "ask_questions" => {
            let a: params::PAskBegin = promoted_args(args_json)?;
            let questions = normalize_ask_questions(&a.questions);
            let doc = ask_begin(mcp, &a.workspace, questions)?;
            let id = doc
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            // Server-side presentation: no client UX binding, so emit the unbound Decisions-app payload.
            let app = mcp
                .read_mcp_app_show(&a.workspace, apps::INTERNAL_DECISIONS_APP)?
                .ok_or_else(|| LoomError::new(Code::Internal, "internal ask app is unavailable"))?;
            let uri = apps::app_uri_with_instance(&app.workspace, &app.app, Some(&id), false);
            let mut payload = app_launch_payload(&app, false);
            if let Some(map) = payload.as_object_mut() {
                map.insert("ask_id".to_string(), Value::String(id));
                map.insert("uri".to_string(), Value::String(uri));
            }
            promoted_result_bytes(payload)
        }
        // Single-shot poll: the bounded wait is driven client-side (`ask_poll_current`), so the server
        // never blocks the write authority on a long poll.
        "ask_answers" => {
            let a: params::PAskWait = promoted_args(args_json)?;
            match ask_poll_state(mcp, &a.workspace, &a.id)? {
                Some(state) => promoted_result_bytes(state),
                None => Err(LoomError::new(
                    Code::NotFound,
                    format!("unknown ask {}", a.id),
                )),
            }
        }
        "ask_record" => {
            let a: params::PAskSubmit = promoted_args(args_json)?;
            let answers = normalize_ask_answers(&a.answers);
            ask_submit(
                mcp,
                &a.workspace,
                &a.id,
                &answers,
                a.aborted.unwrap_or(false),
            )?;
            promoted_result_bytes(Value::Null)
        }
        // ---- chat (Kv + Document + Queue over the substrate chat profile) ----
        // `chat_presence`/`chat_set_presence` are intentionally absent: they act on in-process ephemeral
        // presence (host-runtime state), not the served store, so they stay local.
        "chat_fetch_events" => {
            let a: params::PChatFetchEvents = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_chat_fetch_events(
                &a.workspace,
                &pid,
                &a.channel_id,
                a.from_sequence,
                a.max as usize,
            )?)
        }
        "chat_channels" => {
            let a: params::PChatWorkspace = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_chat_channels(&a.workspace, &pid)?)
        }
        "chat_create_channel" => {
            let a: params::PChatCreateChannel = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_chat_create_channel(
                &a.workspace,
                &pid,
                &a.handle,
                &a.name,
            )?)
        }
        "chat_rename_channel" => {
            let a: params::PChatRenameChannel = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_chat_rename_channel(
                &a.workspace,
                &pid,
                &a.channel_id,
                &a.handle,
            )?)
        }
        "chat_messages" => {
            let a: params::PChatChannel = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_chat_messages(&a.workspace, &pid, &a.channel_id)?)
        }
        "chat_cursor" => {
            let a: params::PChatChannel = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_chat_cursor(&a.workspace, &pid, &a.channel_id)?)
        }
        "chat_post_message" => {
            let a: params::PChatPostMessage = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_chat_post_message(
                &a.workspace,
                &pid,
                &a.channel_id,
                &a.message_id,
                a.thread_id.as_deref(),
                a.body_text.into_bytes(),
            )?)
        }
        "chat_edit_message" => {
            let a: params::PChatEditMessage = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_chat_edit_message(
                &a.workspace,
                &pid,
                &a.channel_id,
                &a.message_id,
                a.body_text.into_bytes(),
            )?)
        }
        "chat_redact_message" => {
            let a: params::PChatRedactMessage = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_chat_redact_message(
                &a.workspace,
                &pid,
                &a.channel_id,
                &a.message_id,
                a.reason.as_deref(),
            )?)
        }
        "chat_emoji_list" => {
            let a: params::PChatWorkspace = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_chat_emoji_registry(&a.workspace, &pid)?)
        }
        "chat_emoji_register" => {
            let a: params::PChatEmoji = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_chat_emoji_register(&a.workspace, &pid, &a.kind)?)
        }
        "chat_emoji_unregister" => {
            let a: params::PChatEmoji = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_chat_emoji_unregister(&a.workspace, &pid, &a.kind)?)
        }
        "chat_add_reaction" => {
            let a: params::PChatReaction = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_chat_add_reaction(
                &a.workspace,
                &pid,
                &a.channel_id,
                &a.message_id,
                &a.kind,
            )?)
        }
        "chat_remove_reaction" => {
            let a: params::PChatReaction = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_chat_remove_reaction(
                &a.workspace,
                &pid,
                &a.channel_id,
                &a.message_id,
                &a.kind,
            )?)
        }
        "chat_create_thread" => {
            let a: params::PChatCreateThread = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_chat_create_thread(
                &a.workspace,
                &pid,
                &a.channel_id,
                &a.thread_id,
                &a.parent_message_id,
            )?)
        }
        "chat_create_task" => {
            let a: params::PChatCreateTask = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_chat_create_task(
                &a.workspace,
                &pid,
                &a.channel_id,
                &a.task_id,
                a.message_id.as_deref(),
                &a.title,
            )?)
        }
        "chat_claim_task" => {
            let a: params::PChatClaimTask = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_chat_claim_task(
                &a.workspace,
                &pid,
                &a.channel_id,
                &a.task_id,
                &a.claim_id,
                a.lease_token.as_deref(),
            )?)
        }
        "chat_complete_task" => {
            let a: params::PChatCompleteTask = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_chat_complete_task(
                &a.workspace,
                &pid,
                &a.channel_id,
                &a.task_id,
                &a.claim_id,
                a.result_message_id.as_deref(),
            )?)
        }
        "chat_invoke_agent" => {
            let a: params::PChatInvokeAgent = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_chat_invoke_agent(
                &a.workspace,
                &pid,
                &a.channel_id,
                &a.invocation_id,
                &a.agent_principal,
                a.source_message_ids,
                a.prompt_text.into_bytes(),
            )?)
        }
        "chat_agent_reply" => {
            let a: params::PChatAgentReply = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_chat_agent_reply(
                &a.workspace,
                &pid,
                &a.channel_id,
                &a.invocation_id,
                &a.message_id,
            )?)
        }
        "chat_request_handoff" => {
            let a: params::PChatRequestHandoff = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_chat_request_handoff(
                &a.workspace,
                &pid,
                &a.channel_id,
                &a.handoff_id,
                &a.from_agent_principal,
                a.to_principal.as_deref(),
                a.reason.as_deref(),
            )?)
        }
        "chat_update_cursor" => {
            let a: params::PChatUpdateCursor = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_chat_update_cursor(
                &a.workspace,
                &pid,
                &a.channel_id,
                a.next_sequence,
            )?)
        }
        // ---- spaces / pages / structures (Studio profile-root families) ----
        "spaces_create" => {
            let a: params::PSpacesCreate = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_spaces_create(
                &a.workspace,
                &pid,
                &a.space_id,
                &a.title,
                a.expected_root.as_deref(),
            )?)
        }
        "spaces_get" => {
            let a: params::PSpacesGet = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_spaces_get(&a.workspace, &pid, &a.space_id)?)
        }
        "spaces_list" => {
            let a: params::PSpacesList = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_spaces_list(&a.workspace, &pid)?)
        }
        "pages_create" => {
            let a: params::PPagesCreate = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_pages_create(
                &a.workspace,
                PageCreateRequest {
                    workspace_id: &pid,
                    page_id: &a.page_id,
                    space_id: &a.space_id,
                    parent_page_id: a.parent_page_id.as_deref(),
                    title: &a.title,
                    expected_root: a.expected_root.as_deref(),
                },
            )?)
        }
        "pages_update" => {
            let a: params::PPagesUpdate = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_pages_update_text(
                &a.workspace,
                &pid,
                &a.page_id,
                a.body_text.as_str(),
                a.expected_root.as_deref(),
            )?)
        }
        "pages_publish" => {
            let a: params::PPagesPublish = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_pages_publish(
                &a.workspace,
                &pid,
                &a.page_id,
                a.expected_root.as_deref(),
            )?)
        }
        "pages_get" => {
            let a: params::PPagesGet = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_pages_get(&a.workspace, &pid, &a.page_id)?)
        }
        "pages_list" => {
            let a: params::PSpacesList = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_pages_list(&a.workspace, &pid)?)
        }
        "pages_history" => {
            let a: params::PPagesGet = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_pages_history(&a.workspace, &pid, &a.page_id)?)
        }
        "structures_create" => {
            let a: params::PStructuresCreate = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_structures_create(
                &a.workspace,
                StructureCreateRequest {
                    workspace_id: &pid,
                    structure_id: &a.structure_id,
                    space_id: &a.space_id,
                    kind: &a.kind,
                    title: &a.title,
                    expected_root: a.expected_root.as_deref(),
                },
            )?)
        }
        "structures_get" => {
            let a: params::PStructuresGet = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_structures_get(&a.workspace, &pid, &a.structure_id)?)
        }
        "structures_list" => {
            let a: params::PSpacesList = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_structures_list(&a.workspace, &pid)?)
        }
        "structures_add_node" => {
            let a: params::PStructuresAddNode = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_structures_add_node(
                &a.workspace,
                StructureNodeRequest {
                    workspace_id: &pid,
                    structure_id: &a.structure_id,
                    node_id: &a.node_id,
                    kind: &a.kind,
                    label: &a.label,
                    body_digest: a.body_digest.as_deref(),
                    entity_ref: a.entity_ref,
                    expected_root: a.expected_root.as_deref(),
                },
            )?)
        }
        "structures_update_node" => {
            let a: params::PStructuresAddNode = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_structures_update_node(
                &a.workspace,
                StructureNodeRequest {
                    workspace_id: &pid,
                    structure_id: &a.structure_id,
                    node_id: &a.node_id,
                    kind: &a.kind,
                    label: &a.label,
                    body_digest: a.body_digest.as_deref(),
                    entity_ref: a.entity_ref,
                    expected_root: a.expected_root.as_deref(),
                },
            )?)
        }
        "structures_move_node" => {
            let a: params::PStructuresMoveNode = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_structures_move_node(
                &a.workspace,
                StructureMoveRequest {
                    workspace_id: &pid,
                    structure_id: &a.structure_id,
                    node_id: &a.node_id,
                    parent_node_id: a.parent_node_id.as_deref(),
                    label: a.label.as_deref(),
                    expected_root: a.expected_root.as_deref(),
                },
            )?)
        }
        "structures_link_node" => {
            let a: params::PStructuresLinkNode = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_structures_link_node(
                &a.workspace,
                StructureLinkRequest {
                    workspace_id: &pid,
                    structure_id: &a.structure_id,
                    edge_id: &a.edge_id,
                    src_node_id: &a.src_node_id,
                    dst_node_id: &a.dst_node_id,
                    label: &a.label,
                    target_ref: a.target_ref,
                    expected_root: a.expected_root.as_deref(),
                },
            )?)
        }
        "structures_bind" => {
            let a: params::PStructuresBind = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_structures_bind(
                &a.workspace,
                StructureBindRequest {
                    workspace_id: &pid,
                    structure_id: &a.structure_id,
                    node_id: &a.node_id,
                    entity_ref: a.entity_ref,
                    expected_root: a.expected_root.as_deref(),
                },
            )?)
        }
        "structures_decompose_to_tickets" => {
            let a: params::PStructuresDecomposeToTickets = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            let items = a
                .items
                .iter()
                .map(|item| StructureDecomposeItem {
                    node_id: &item.node_id,
                    project_id: &item.project_id,
                    ticket_type: item.ticket_type.as_deref(),
                    fields: item.fields.as_ref(),
                    policy_labels: &item.policy_labels,
                })
                .collect::<Vec<_>>();
            promoted_result_bytes(mcp.write_structures_decompose_to_tickets(
                &a.workspace,
                StructureDecomposeRequest {
                    workspace_id: &pid,
                    structure_id: &a.structure_id,
                    items: &items,
                },
            )?)
        }
        "substrate_changes" => {
            let a: params::PSubstrateChanges = promoted_args(args_json)?;
            promoted_result_bytes(mcp.read_substrate_changes(&a.workspace, &a.cursor, a.max)?)
        }
        "workgraph_metrics" => {
            let a: params::PWorkgraphMetrics = promoted_args(args_json)?;
            promoted_result_bytes(mcp.read_workgraph_metrics(
                &a.workspace,
                a.workspace_id.as_deref(),
                &a.statuses,
                &a.lanes,
                a.limit,
            )?)
        }
        "substrate_refs" => {
            let a: params::PSubstrateRefs = promoted_args(args_json)?;
            promoted_result_bytes(mcp.read_substrate_refs(&a.workspace, &a.target)?)
        }
        "substrate_alias_bind" => {
            let a: params::PSubstrateAliasBind = promoted_args(args_json)?;
            promoted_result_bytes(mcp.write_substrate_alias_bind(
                &a.workspace,
                &a.scope_id,
                &a.alias,
                &a.target,
            )?)
        }
        "substrate_alias_release" => {
            let a: params::PSubstrateAliasKey = promoted_args(args_json)?;
            promoted_result_bytes(mcp.write_substrate_alias_release(
                &a.workspace,
                &a.scope_id,
                &a.alias,
            )?)
        }
        "substrate_alias_resolve" => {
            let a: params::PSubstrateAliasKey = promoted_args(args_json)?;
            promoted_result_bytes(mcp.read_substrate_alias_resolve(
                &a.workspace,
                &a.scope_id,
                &a.alias,
            )?)
        }
        "substrate_alias_list" => {
            let a: params::PSubstrateAliasList = promoted_args(args_json)?;
            promoted_result_bytes(mcp.read_substrate_alias_list(&a.workspace, &a.scope_id)?)
        }
        "substrate_reference_status" => {
            let a: params::PSubstrateReferenceStatus = promoted_args(args_json)?;
            promoted_result_bytes(mcp.read_substrate_reference_reconciliation_status(&a.workspace)?)
        }
        "substrate_reference_reconcile" => {
            let a: params::PSubstrateReferenceReconcile = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_substrate_reference_reconcile(
                &a.workspace,
                &pid,
                a.max,
            )?)
        }
        "substrate_history" => {
            let a: params::PSubstrateHistory = promoted_args(args_json)?;
            promoted_result_bytes(mcp.read_substrate_history(
                &a.workspace,
                &a.scope_id,
                &a.entity_id,
            )?)
        }
        "substrate_revision_latest" => {
            let a: params::PSubstrateHistory = promoted_args(args_json)?;
            promoted_result_bytes(mcp.read_substrate_revision_latest(
                &a.workspace,
                &a.scope_id,
                &a.entity_id,
            )?)
        }
        "substrate_revision_at" => {
            let a: params::PSubstrateRevisionAt = promoted_args(args_json)?;
            promoted_result_bytes(mcp.read_substrate_revision_at(
                &a.workspace,
                &a.scope_id,
                &a.entity_id,
                a.revision,
            )?)
        }
        "substrate_revision_as_of_root" => {
            let a: params::PSubstrateRevisionAsOfRoot = promoted_args(args_json)?;
            promoted_result_bytes(mcp.read_substrate_revision_as_of_root(
                &a.workspace,
                &a.scope_id,
                &a.entity_id,
                &a.root,
            )?)
        }
        "substrate_checkpoint_before" => {
            let a: params::PSubstrateCheckpointBefore = promoted_args(args_json)?;
            promoted_result_bytes(mcp.read_substrate_checkpoint_before(
                &a.workspace,
                &a.scope_id,
                a.revision,
            )?)
        }
        "substrate_view_define" => {
            let a: params::PSubstrateViewDefine = promoted_args(args_json)?;
            let source_scopes = a
                .source_scopes
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            let source_facets = a
                .source_facets
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            promoted_result_bytes(mcp.write_substrate_view_define(SubstrateViewDefineRequest {
                workspace: &a.workspace,
                view_id: &a.view_id,
                source_scopes: &source_scopes,
                source_facets: &source_facets,
                projection_ref: &a.projection_ref,
                output_facet: a.output_facet.as_deref(),
                media_type: &a.media_type,
                freshness_policy: &a.freshness_policy,
            })?)
        }
        "substrate_view_get" => {
            let a: params::PSubstrateViewGet = promoted_args(args_json)?;
            promoted_result_bytes(mcp.read_substrate_view_get(&a.workspace, &a.view_id)?)
        }
        "substrate_view_list" => {
            let a: params::PSubstrateViewList = promoted_args(args_json)?;
            promoted_result_bytes(mcp.read_substrate_view_list(&a.workspace)?)
        }
        "substrate_write_admission_policy_get" => {
            let a: params::PSubstrateWriteAdmissionPolicyKey = promoted_args(args_json)?;
            promoted_result_bytes(mcp.read_substrate_write_admission_policy(
                &a.workspace,
                &a.surface,
                &a.scope_id,
            )?)
        }
        "substrate_write_admission_policy_set" => {
            let a: params::PSubstrateWriteAdmissionPolicySet = promoted_args(args_json)?;
            let mandatory_targets = a
                .mandatory_targets
                .into_iter()
                .map(|target| WriteAdmissionTarget::new(target.target_kind, target.target_id))
                .collect::<Result<Vec<_>, _>>()?;
            promoted_result_bytes(mcp.write_substrate_write_admission_policy_set(
                WriteAdmissionPolicyRequest {
                    workspace: &a.workspace,
                    surface: &a.surface,
                    scope_id: &a.scope_id,
                    default_mode: &a.default_mode,
                    mandatory_targets: &mandatory_targets,
                },
            )?)
        }
        "substrate_transact" => {
            let a: params::PSubstrateTransact = promoted_args(args_json)?;
            let mut ops = Vec::with_capacity(a.ops.len());
            for op in a.ops {
                ops.push(
                    substrate_transact_op(&Binding::default(), op).map_err(|e| {
                        LoomError::new(Code::InvalidArgument, e.message.to_string())
                    })?,
                );
            }
            promoted_result_bytes(mcp.write_substrate_transact(ops)?)
        }
        "tickets_project_create" => {
            let a: params::PTicketsProjectCreate = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_tickets_project_create(
                &a.workspace,
                &pid,
                &a.project_id,
                &a.key_prefix,
                &a.name,
                a.expected_root.as_deref(),
            )?)
        }
        "tickets_project_rekey" => {
            let a: params::PTicketsProjectRekey = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_tickets_project_rekey(
                &a.workspace,
                &pid,
                &a.project_id,
                &a.key_prefix,
                a.expected_root.as_deref(),
            )?)
        }
        "tickets_project_settings_get" => {
            let a: params::PTicketsProjectSettingsGet = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_tickets_project_settings_get(
                &a.workspace,
                &pid,
                &a.project_id,
                a.include_contracts,
            )?)
        }
        "tickets_projects" => {
            let a: params::PTicketsProjects = promoted_args(args_json)?;
            promoted_result_bytes(mcp.read_tickets_projects(&a.workspace)?)
        }
        "tickets_relations" => {
            let a: params::PTicketsRelations = promoted_args(args_json)?;
            promoted_result_bytes(mcp.read_tickets_relations(&a.workspace, &a.ticket_id)?)
        }
        "tickets_fields" => {
            let a: params::PTicketsFields = promoted_args(args_json)?;
            promoted_result_bytes(mcp.read_tickets_fields(
                &a.workspace,
                a.project_id.as_deref(),
                a.projection.as_deref(),
                a.operation.as_deref(),
            )?)
        }
        "tickets_field_put" => {
            let a: params::PTicketsFieldPut = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_tickets_field_put(
                &a.workspace,
                TicketFieldDefinitionWriteRequest {
                    workspace_id: &pid,
                    project_id: &a.project_id,
                    field_id: &a.field_id,
                    key: &a.key,
                    name: &a.name,
                    description: a.description.as_deref(),
                    field_type: &a.field_type,
                    option_set: a.option_set.as_deref(),
                    max_length: a.max_length,
                    required: a.required,
                    searchable: a.searchable,
                    orderable: a.orderable,
                    cardinality: ticket_field_cardinality(&a.cardinality)?,
                    applicable_type_ids: &a.applicable_type_ids,
                    expected_root: a.expected_root.as_deref(),
                },
            )?)
        }
        "tickets_field_retire" => {
            let a: params::PTicketsFieldRetire = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_tickets_field_retire(
                &a.workspace,
                TicketFieldDefinitionRetireRequest {
                    workspace_id: &pid,
                    project_id: &a.project_id,
                    field_id: &a.field_id,
                    expected_root: a.expected_root.as_deref(),
                },
            )?)
        }
        "tickets_project_settings_set" => {
            let a: params::PTicketsProjectSettingsSet = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            let default_projection = a
                .default_projection
                .as_deref()
                .map(loom_tickets::TicketProjectionProfile::parse)
                .transpose()?;
            let actor_enforcement = a
                .actor_enforcement
                .as_deref()
                .map(TicketLifecycleAuthorizationPolicy::parse)
                .transpose()?;
            let acceptance_authorities = a.acceptance_authorities.as_deref();
            let required_acceptance_evidence_keys = a
                .required_acceptance_evidence_keys
                .as_deref()
                .map(|keys| {
                    keys.iter()
                        .map(|key| loom_tickets::TicketAcceptanceEvidenceKey::parse(key))
                        .collect::<loom_core::Result<Vec<_>>>()
                })
                .transpose()?;
            promoted_result_bytes(mcp.write_tickets_project_settings_set(
                &a.workspace,
                loom_tickets::TicketProjectSettingsRequest {
                    workspace_id: &pid,
                    project_id: &a.project_id,
                    default_projection,
                    enable_projections: &[],
                    disable_projections: &[],
                    actor_enforcement,
                    project_owner_principal: a.project_owner_principal.as_deref(),
                    clear_project_owner_principal: a.clear_project_owner_principal,
                    acceptance_authorities,
                    acceptance_evidence_enforcement: a.acceptance_evidence_enforcement,
                    required_acceptance_evidence_keys: required_acceptance_evidence_keys.as_deref(),
                    owner_contract_summary: a.owner_contract_summary.as_deref(),
                    owner_contract_details: a.owner_contract_details.as_deref(),
                    worker_contract_summary: a.worker_contract_summary.as_deref(),
                    worker_contract_details: a.worker_contract_details.as_deref(),
                    expected_root: a.expected_root.as_deref(),
                },
            )?)
        }
        "tickets_create" => {
            let a: params::PTicketsCreate = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            let fields = json_object_value(a.fields);
            let projection = loom_tickets::parse_ticket_projection(a.projection.as_deref())?;
            let fields = loom_tickets::normalize_ticket_fields_for_projection(&fields, projection)?;
            promoted_result_bytes(mcp.write_tickets_create_receipt(
                &a.workspace,
                TicketCreateRequest {
                    workspace_id: &pid,
                    project_id: &a.project_id,
                    ticket_type: &a.ticket_type,
                    external_source: a.external_source.as_deref(),
                    external_id: a.external_id.as_deref(),
                    fields: &fields,
                    policy_labels: &a.policy_labels,
                    expected_root: a.expected_root.as_deref(),
                },
            )?)
        }
        "tickets_update" => {
            let a: params::PTicketsUpdate = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            let projection = loom_tickets::parse_ticket_projection(a.projection.as_deref())?;
            let set_fields = a
                .set_fields
                .map(json_object_value)
                .map(|fields| {
                    loom_tickets::normalize_ticket_fields_for_projection(&fields, projection)
                })
                .transpose()?;
            let delete_fields = loom_tickets::normalize_ticket_delete_fields_for_projection(
                &a.delete_fields,
                projection,
            );
            let action = a
                .action
                .as_deref()
                .map(TicketLifecycleAction::parse)
                .transpose()?;
            let comment = a
                .comment
                .as_ref()
                .map(|comment| {
                    comment
                        .evidence
                        .as_ref()
                        .map(ticket_comment_evidence_from_map)
                        .transpose()
                        .map(|evidence| loom_tickets::TicketUpdateCommentRequest {
                            comment_id: comment.comment_id.as_deref(),
                            comment_type: comment.comment_type.as_deref(),
                            body: &comment.body,
                            evidence,
                        })
                })
                .transpose()?;
            let comments = a
                .comments
                .iter()
                .map(|comment| {
                    comment
                        .evidence
                        .as_ref()
                        .map(ticket_comment_evidence_from_map)
                        .transpose()
                        .map(|evidence| loom_tickets::TicketUpdateCommentRequest {
                            comment_id: comment.comment_id.as_deref(),
                            comment_type: comment.comment_type.as_deref(),
                            body: &comment.body,
                            evidence,
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let relation_sets = a
                .relation_sets
                .iter()
                .map(|relation| {
                    TicketRelationKind::parse(&relation.kind).map(|kind| {
                        loom_tickets::TicketUpdateRelationSetRequest {
                            relation_id: relation.relation_id.as_deref(),
                            kind,
                            target_id: &relation.target_id,
                        }
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let relation_removes = a
                .relation_removes
                .iter()
                .map(|relation| loom_tickets::TicketUpdateRelationRemoveRequest {
                    relation_id: &relation.relation_id,
                })
                .collect::<Vec<_>>();
            promoted_result_bytes(mcp.write_tickets_update_receipt(
                &a.workspace,
                TicketUpdateRequest {
                    workspace_id: &pid,
                    ticket_id: &a.ticket_id,
                    set_fields: set_fields.as_ref(),
                    delete_fields: &delete_fields,
                    action,
                    target_status: a.target_status.as_deref(),
                    observed_source_status: a.observed_source_status.as_deref(),
                    observed_workflow_version: a.observed_workflow_version.as_deref(),
                    assignee: a.assignee.as_deref(),
                    expected_root: a.expected_root.as_deref(),
                    comment,
                    comments: &comments,
                    relation_sets: &relation_sets,
                    relation_removes: &relation_removes,
                },
            )?)
        }
        "tickets_delete" => {
            let a: params::PTicketsDelete = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_tickets_delete_receipt(
                &a.workspace,
                TicketDeleteRequest {
                    workspace_id: &pid,
                    ticket_id: &a.ticket_id,
                    expected_root: a.expected_root.as_deref(),
                },
            )?)
        }
        "tickets_comments" => {
            let a: params::PTicketsComments = promoted_args(args_json)?;
            promoted_result_bytes(mcp.read_tickets_comments(&a.workspace, &a.ticket_id)?)
        }
        "tickets_comment_add" => {
            let a: params::PTicketsCommentAdd = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(
                mcp.write_tickets_comment_add_receipt(
                    &a.workspace,
                    TicketCommentRequest {
                        workspace_id: &pid,
                        ticket_id: &a.ticket_id,
                        comment_id: a.comment_id.as_deref(),
                        comment_type: Some(&a.comment_type),
                        body: &a.body,
                        evidence: a
                            .evidence
                            .as_ref()
                            .map(ticket_comment_evidence_from_map)
                            .transpose()?,
                        expected_root: a.expected_root.as_deref(),
                    },
                )?,
            )
        }
        "tickets_comment_update" => {
            let a: params::PTicketsCommentUpdate = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(
                mcp.write_tickets_comment_update_receipt(
                    &a.workspace,
                    TicketCommentUpdateRequest {
                        workspace_id: &pid,
                        ticket_id: &a.ticket_id,
                        comment_id: &a.comment_id,
                        comment_type: a.comment_type.as_deref(),
                        body: a.body.as_deref(),
                        evidence: a
                            .evidence
                            .as_ref()
                            .map(|evidence| {
                                evidence
                                    .as_ref()
                                    .map(ticket_comment_evidence_from_map)
                                    .transpose()
                            })
                            .transpose()?,
                        expected_root: a.expected_root.as_deref(),
                    },
                )?,
            )
        }
        "tickets_comment_delete" => {
            let a: params::PTicketsCommentDelete = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_tickets_comment_delete_receipt(
                &a.workspace,
                TicketCommentDeleteRequest {
                    workspace_id: &pid,
                    ticket_id: &a.ticket_id,
                    comment_id: &a.comment_id,
                    expected_root: a.expected_root.as_deref(),
                },
            )?)
        }
        "tickets_board_create" => {
            let a: params::PTicketsBoardCreate = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            let columns = board_columns(a.columns)?;
            let swimlanes = board_swimlanes(a.swimlanes)?;
            promoted_result_bytes(mcp.write_tickets_board_create(
                &a.workspace,
                BoardCreateRequest {
                    workspace_id: &pid,
                    board_id: &a.board_id,
                    board_key: &a.board_key,
                    name: &a.name,
                    description: &a.description,
                    project_id: &a.project_id,
                    scope: board_scope(&a.scope, &a.project_id)?,
                    mode: BoardMode::parse(&a.mode)?,
                    columns: &columns,
                    swimlanes: &swimlanes,
                    card_display_fields: &a.card_display_fields,
                    owner_principal: a.owner_principal.as_deref(),
                    coordinator_principal: a.coordinator_principal.as_deref(),
                    updated_by: &a.updated_by,
                    expected_root: a.expected_root.as_deref(),
                },
            )?)
        }
        "tickets_board_update" => {
            let a: params::PTicketsBoardUpdate = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            let status = a
                .board_status
                .as_deref()
                .map(BoardStatus::parse)
                .transpose()?;
            promoted_result_bytes(mcp.write_tickets_board_update(
                &a.workspace,
                BoardUpdateRequest {
                    workspace_id: &pid,
                    board_id: &a.board_id,
                    board_key: a.board_key.as_deref(),
                    name: a.name.as_deref(),
                    description: a.description.as_deref(),
                    scope: None,
                    owner_principal: None,
                    coordinator_principal: None,
                    card_display_fields: a.card_display_fields.as_deref(),
                    board_status: status,
                    updated_by: &a.updated_by,
                    expected_root: a.expected_root.as_deref(),
                },
            )?)
        }
        "tickets_board_delete" => {
            let a: params::PTicketsBoardUpdate = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_tickets_board_update(
                &a.workspace,
                BoardUpdateRequest {
                    workspace_id: &pid,
                    board_id: &a.board_id,
                    board_key: None,
                    name: None,
                    description: None,
                    scope: None,
                    owner_principal: None,
                    coordinator_principal: None,
                    card_display_fields: None,
                    board_status: Some(BoardStatus::Deleted),
                    updated_by: &a.updated_by,
                    expected_root: a.expected_root.as_deref(),
                },
            )?)
        }
        "tickets_board_configure_columns" => {
            let a: params::PTicketsBoardConfigureColumns = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            let columns = board_columns(a.columns)?;
            let swimlanes = board_swimlanes(a.swimlanes)?;
            let mode = a.mode.as_deref().map(BoardMode::parse).transpose()?;
            promoted_result_bytes(mcp.write_tickets_board_configure_columns(
                &a.workspace,
                BoardColumnConfigureRequest {
                    workspace_id: &pid,
                    board_id: &a.board_id,
                    mode,
                    columns: &columns,
                    swimlanes: &swimlanes,
                    updated_by: &a.updated_by,
                    expected_root: a.expected_root.as_deref(),
                },
            )?)
        }
        "tickets_board_move_card" => {
            let a: params::PTicketsBoardMoveCard = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_tickets_board_move_card(
                &a.workspace,
                BoardCardMoveRequest {
                    workspace_id: &pid,
                    board_id: &a.board_id,
                    ticket_id: &a.ticket_id,
                    column_id: &a.column_id,
                    rank_token: &a.rank_token,
                    swimlane_id: a.swimlane_id.as_deref(),
                    updated_by: &a.updated_by,
                    expected_root: a.expected_root.as_deref(),
                },
            )?)
        }
        "lanes_create" => {
            let a: params::PLanesCreate = promoted_args(args_json)?;
            let lane_tickets = loom_lanes::lane_tickets_from_order(&a.ticket_ids)?;
            promoted_public_lane_envelope_bytes(mcp.write_lanes_create_receipt(
                &a.workspace,
                LaneCreateRequest {
                    lane_id: &a.lane_id,
                    lane_key: &a.lane_key,
                    title: &a.title,
                    description: &a.description,
                    lane_kind: &a.lane_kind,
                    owner_principal: a.owner_principal.as_deref(),
                    lane_status: &a.lane_status,
                    lane_tickets: &lane_tickets,
                    active_ticket_id: a.active_ticket_id.as_deref(),
                    status_report: &a.status_report,
                    reviewer_feedback: &a.reviewer_feedback,
                    updated_by: a.updated_by.as_deref(),
                },
            )?)
        }
        "tickets_relation_set" => {
            let a: params::PTicketsRelationSet = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            let kind = TicketRelationKind::parse(&a.kind)?;
            promoted_result_bytes(mcp.write_tickets_relation_set_receipt(
                &a.workspace,
                TicketRelationRequest {
                    workspace_id: &pid,
                    ticket_id: &a.ticket_id,
                    relation_id: a.relation_id.as_deref(),
                    kind,
                    target_id: &a.target_id,
                    expected_root: a.expected_root.as_deref(),
                },
            )?)
        }
        "tickets_relation_remove" => {
            let a: params::PTicketsRelationRemove = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_tickets_relation_remove_receipt(
                &a.workspace,
                TicketRelationRemoveRequest {
                    workspace_id: &pid,
                    ticket_id: &a.ticket_id,
                    relation_id: &a.relation_id,
                    expected_root: a.expected_root.as_deref(),
                },
            )?)
        }
        "tickets_get" => {
            let a: params::PTicketsGet = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            if a.detailed {
                promoted_result_bytes(mcp.read_tickets_get(
                    &a.workspace,
                    &pid,
                    &a.ticket_id,
                    a.projection.as_deref(),
                )?)
            } else {
                promoted_result_bytes(mcp.read_tickets_get_readable(
                    &a.workspace,
                    &pid,
                    &a.ticket_id,
                    a.projection.as_deref(),
                )?)
            }
        }
        "tickets_list" => {
            let a: params::PTicketsList = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            let query = build_ticket_list_query(mcp, &a)?;
            promoted_result_bytes(mcp.read_tickets_page(&a.workspace, &pid, query)?)
        }
        "tickets_board_get" => {
            let a: params::PTicketsBoardGet = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_tickets_boards_get(&a.workspace, &pid, &a.board_id)?)
        }
        "tickets_board_list" => {
            let a: params::PTicketsBoardList = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_tickets_boards_list(
                &a.workspace,
                &pid,
                a.include_deleted,
            )?)
        }
        "tickets_history" => {
            let a: params::PTicketsHistory = promoted_args(args_json)?;
            let pid = workspace_profile_id(mcp, &a.workspace)?;
            if a.detailed {
                promoted_result_bytes(mcp.read_tickets_history(
                    &a.workspace,
                    &pid,
                    a.ticket_id.as_deref(),
                )?)
            } else {
                promoted_result_bytes(mcp.read_tickets_history_readable(
                    &a.workspace,
                    &pid,
                    a.ticket_id.as_deref(),
                )?)
            }
        }
        // ---- 660: deferred store-backed families promoted server-side ----
        "workgraph_changes" => {
            let a: params::PWorkgraphChanges = promoted_args(args_json)?;
            promoted_result_bytes(mcp.read_workgraph_changes(
                &a.workspace,
                &a.workspace_id,
                a.next_sequence,
                a.max,
            )?)
        }
        "workgraph_fact_put" => {
            let a: params::PWorkgraphFactPut = promoted_args(args_json)?;
            promoted_result_bytes(mcp.write_workgraph_fact_put(
                &a.workspace,
                &a.workspace_id,
                a.fact,
            )?)
        }
        "import_submit_batch" => {
            let a: params::PImportSubmitBatch = promoted_args(args_json)?;
            promoted_result_bytes(mcp.write_import_submit_batch(&a.workspace, &a.batch)?)
        }
        "import_execute_batch" => {
            let a: params::PImportExecuteBatch = promoted_args(args_json)?;
            promoted_result_bytes(mcp.write_import_execute_batch(
                &a.workspace,
                &a.batch,
                a.dry_run.unwrap_or(false),
            )?)
        }
        "redmine_import_snapshot" => {
            let a: params::PRedmineImportSnapshot = promoted_args(args_json)?;
            promoted_result_bytes(mcp.write_redmine_import_snapshot(
                &a.workspace,
                &a.profile,
                a.source_path.as_deref(),
                &a.snapshot,
                a.field_policy.as_deref(),
                a.dry_run.unwrap_or(false),
            )?)
        }
        // 660: lifecycles (store-backed) promoted server-side. `lifecycles_active_set`/
        // `lifecycles_active_clear` stay host-local (in-process active-lifecycle selection).
        "lifecycles_define" => {
            let a: params::PLifecyclesDefine = promoted_args(args_json)?;
            let profile_id = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_lifecycles_define(
                &a.workspace,
                &profile_id,
                &a.definition_cbor,
            )?)
        }
        "lifecycles_define_standard" => {
            let a: params::PLifecyclesDefineStandard = promoted_args(args_json)?;
            let profile_id = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_lifecycles_define_standard(
                &a.workspace,
                loom_lifecycle::StandardLifecycleRequest {
                    workspace_id: &profile_id,
                    kind: &a.kind,
                    version: &a.version,
                    completion_predicate_digest: &a.completion_predicate_digest,
                },
            )?)
        }
        "lifecycles_definitions" => {
            let a: params::PLifecyclesWorkspace = promoted_args(args_json)?;
            let profile_id = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_lifecycles_definitions(&a.workspace, &profile_id)?)
        }
        "lifecycles_definition" => {
            let a: params::PLifecyclesDefinition = promoted_args(args_json)?;
            let profile_id = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_lifecycles_definition(
                &a.workspace,
                &profile_id,
                &a.definition_id,
            )?)
        }
        "lifecycles_instantiate" => {
            let a: params::PLifecyclesInstantiate = promoted_args(args_json)?;
            let profile_id = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.write_lifecycles_instantiate(
                &a.workspace,
                &profile_id,
                &a.instance_id,
                &a.definition_id,
                a.subject_refs,
            )?)
        }
        "lifecycles_instances" => {
            let a: params::PLifecyclesWorkspace = promoted_args(args_json)?;
            let profile_id = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_lifecycles_instances(&a.workspace, &profile_id)?)
        }
        "lifecycles_instance" => {
            let a: params::PLifecyclesInstance = promoted_args(args_json)?;
            let profile_id = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_lifecycles_instance(
                &a.workspace,
                &profile_id,
                &a.instance_id,
            )?)
        }
        "lifecycles_snapshot_plan" => {
            let a: params::PLifecyclesSnapshotPlan = promoted_args(args_json)?;
            let profile_id = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_lifecycles_snapshot_plan(
                &a.workspace,
                &profile_id,
                &a.instance_id,
                &a.to_stage_id,
            )?)
        }
        "lifecycles_current_surface" => {
            let a: params::PLifecyclesInstance = promoted_args(args_json)?;
            let profile_id = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_lifecycles_current_surface(
                &a.workspace,
                &profile_id,
                &a.instance_id,
            )?)
        }
        "lifecycles_transition" => {
            let a: params::PLifecyclesTransition = promoted_args(args_json)?;
            let profile_id = workspace_profile_id(mcp, &a.workspace)?;
            let gate_evaluations = a
                .gate_evaluations
                .into_iter()
                .map(|gate| loom_lifecycle::LifecycleGateEvaluationInput {
                    gate_id: gate.gate_id,
                    passed: gate.passed,
                    principal_id: gate.principal_id,
                    evidence_digest: gate.evidence_digest,
                    evaluated_at_ms: gate.evaluated_at_ms,
                })
                .collect();
            promoted_result_bytes(mcp.write_lifecycles_transition(
                &a.workspace,
                loom_lifecycle::LifecycleTransitionRequest {
                    workspace_id: &profile_id,
                    instance_id: &a.instance_id,
                    transition_id: &a.transition_id,
                    to_stage_id: &a.to_stage_id,
                    actor_principal_id: &a.actor_principal_id,
                    gate_evaluations,
                    snapshot_digest: a.snapshot_digest.as_deref(),
                    recorded_at_ms: a.recorded_at_ms,
                },
            )?)
        }
        "lifecycles_snapshots" => {
            let a: params::PLifecyclesWorkspace = promoted_args(args_json)?;
            let profile_id = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_lifecycles_snapshots(&a.workspace, &profile_id)?)
        }
        "lifecycles_snapshot" => {
            let a: params::PLifecyclesSnapshot = promoted_args(args_json)?;
            let profile_id = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_lifecycles_snapshot(
                &a.workspace,
                &profile_id,
                &a.snapshot_id,
            )?)
        }
        "lifecycles_snapshot_content" => {
            let a: params::PLifecyclesSnapshot = promoted_args(args_json)?;
            let profile_id = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_lifecycles_snapshot_content(
                &a.workspace,
                &profile_id,
                &a.snapshot_id,
            )?)
        }
        "lifecycles_operation_log" => {
            let a: params::PLifecyclesWorkspace = promoted_args(args_json)?;
            let profile_id = workspace_profile_id(mcp, &a.workspace)?;
            promoted_result_bytes(mcp.read_lifecycles_operation_log(&a.workspace, &profile_id)?)
        }
        other => Err(LoomError::new(
            Code::Unsupported,
            format!("MCP tool {other} is not server-promoted for remote execution"),
        )),
    }
}

fn columnar_select_filter_arg(
    filter: Option<&[u8]>,
    predicate: Option<&Value>,
) -> Result<Vec<u8>, LoomError> {
    if predicate.is_some() && filter.is_some_and(|bytes| !bytes.is_empty()) {
        return Err(LoomError::invalid(
            "columnar_select accepts either filter or predicate, not both",
        ));
    }
    match predicate {
        Some(Value::Null) | None => Ok(filter.unwrap_or_default().to_vec()),
        Some(predicate) => columnar_predicate_filter_cbor(predicate),
    }
}

fn columnar_predicate_filter_cbor(predicate: &Value) -> Result<Vec<u8>, LoomError> {
    let simple = Predicate::from_json_value(predicate)?
        .as_simple_comparison()
        .ok_or_else(|| {
            LoomError::invalid(
                "current columnar_select predicate must be one single-column comparison",
            )
        })?;
    let [column] = simple.path.as_slice() else {
        return Err(LoomError::invalid(
            "current columnar_select predicate path must name one column",
        ));
    };
    let value = simple.value.to_tabular_value()?;
    let encoded = loom_codec::Value::Array(vec![
        loom_codec::Value::Text(column.clone()),
        loom_codec::Value::Uint(cmp_op_tag(simple.op)),
        cell_value(&value),
    ]);
    loom_codec::encode(&encoded).map_err(|e| LoomError::invalid(format!("cbor: {e}")))
}

fn cmp_op_tag(op: CompareOp) -> u64 {
    match op {
        CompareOp::Eq => 0,
        CompareOp::Ne => 1,
        CompareOp::Lt => 2,
        CompareOp::Lte => 3,
        CompareOp::Gt => 4,
        CompareOp::Gte => 5,
    }
}

/// Normalize a tool name to its host-compatible wire form. MCP hosts (e.g. Claude's remote-MCP
/// surface) validate advertised tool names against `^[a-zA-Z0-9_-]{1,64}$`, which forbids `.`.
///
/// The curated surface (`tools::TOOL_SURFACE`) and prompts are underscore-native, so this is an
/// identity no-op for them. It remains meaningful for the dynamic app-launcher surface, whose
/// names are derived from arbitrary (possibly dotted, hierarchical) app slugs and therefore must
/// still be sanitized at the serving boundary. Applying it is idempotent, so it doubles as the
/// single normalization rule used to reverse wire names on dispatch (see `canonical_tool_name`).
fn sanitize_tool_name(name: &str) -> String {
    name.replace('.', "_")
}

/// True when a tool declares `_meta.ui.visibility` that excludes `"model"` (i.e. app-only), so it
/// must be omitted from the agent's `tools/list`. Tools without a visibility declaration are always
/// model-visible.
fn tool_hidden_from_model(tool: &Tool) -> bool {
    tool.meta.as_ref().is_some_and(|meta| {
        meta.0
            .get("ui")
            .and_then(|ui| ui.get("visibility"))
            .and_then(|v| v.as_array())
            .is_some_and(|arr| !arr.iter().any(|x| x.as_str() == Some("model")))
    })
}

fn transaction_workspace(
    binding: &Binding,
    workspace: Option<String>,
) -> Result<String, ErrorData> {
    workspace
        .or_else(|| binding.workspace.clone())
        .ok_or_else(|| {
            ErrorData::invalid_params(
                "substrate_transact operation is missing workspace in an unscoped server",
                None,
            )
        })
}

fn transaction_collection(
    binding: &Binding,
    collection: Option<String>,
) -> Result<String, ErrorData> {
    collection
        .or_else(|| binding.collection.clone())
        .ok_or_else(|| {
            ErrorData::invalid_params(
                "substrate_transact document operation is missing collection in an unscoped server",
                None,
            )
        })
}

fn substrate_transact_op(
    binding: &Binding,
    op: PSubstrateTransactOp,
) -> Result<SubstrateTransactOp, ErrorData> {
    match op {
        PSubstrateTransactOp::CasPut { workspace, content } => Ok(SubstrateTransactOp::CasPut {
            workspace: transaction_workspace(binding, workspace)?,
            content,
        }),
        PSubstrateTransactOp::CasDelete { workspace, digest } => {
            Ok(SubstrateTransactOp::CasDelete {
                workspace: transaction_workspace(binding, workspace)?,
                digest,
            })
        }
        PSubstrateTransactOp::DocumentPut {
            workspace,
            collection,
            id,
            doc,
        } => Ok(SubstrateTransactOp::DocumentPut {
            workspace: transaction_workspace(binding, workspace)?,
            collection: transaction_collection(binding, collection)?,
            id,
            doc,
        }),
        PSubstrateTransactOp::DocumentDelete {
            workspace,
            collection,
            id,
        } => Ok(SubstrateTransactOp::DocumentDelete {
            workspace: transaction_workspace(binding, workspace)?,
            collection: transaction_collection(binding, collection)?,
            id,
        }),
        PSubstrateTransactOp::DocumentReplaceText {
            workspace,
            collection,
            id,
            base_digest,
            find,
            replace,
            replace_all,
        } => Ok(SubstrateTransactOp::DocumentReplaceText {
            workspace: transaction_workspace(binding, workspace)?,
            collection: transaction_collection(binding, collection)?,
            id,
            base_digest,
            find,
            replace,
            replace_all,
        }),
        PSubstrateTransactOp::GraphUpsertNode {
            workspace,
            collection,
            id,
            props,
        } => Ok(SubstrateTransactOp::GraphUpsertNode {
            workspace: transaction_workspace(binding, workspace)?,
            collection: transaction_collection(binding, collection)?,
            id,
            props,
        }),
        PSubstrateTransactOp::GraphRemoveNode {
            workspace,
            collection,
            id,
            cascade,
        } => Ok(SubstrateTransactOp::GraphRemoveNode {
            workspace: transaction_workspace(binding, workspace)?,
            collection: transaction_collection(binding, collection)?,
            id,
            cascade,
        }),
        PSubstrateTransactOp::GraphUpsertEdge {
            workspace,
            collection,
            id,
            src,
            dst,
            label,
            props,
        } => Ok(SubstrateTransactOp::GraphUpsertEdge {
            workspace: transaction_workspace(binding, workspace)?,
            collection: transaction_collection(binding, collection)?,
            id,
            src,
            dst,
            label,
            props,
        }),
        PSubstrateTransactOp::GraphRemoveEdge {
            workspace,
            collection,
            id,
        } => Ok(SubstrateTransactOp::GraphRemoveEdge {
            workspace: transaction_workspace(binding, workspace)?,
            collection: transaction_collection(binding, collection)?,
            id,
        }),
        PSubstrateTransactOp::SubstrateViewDefine {
            workspace,
            view_id,
            source_scopes,
            source_facets,
            projection_ref,
            output_facet,
            media_type,
            freshness_policy,
        } => Ok(SubstrateTransactOp::SubstrateViewDefine(
            SubstrateViewDefineOwned {
                workspace: transaction_workspace(binding, workspace)?,
                view_id,
                source_scopes,
                source_facets,
                projection_ref,
                output_facet,
                media_type,
                freshness_policy,
            },
        )),
    }
}

fn jbytes(b: &[u8]) -> Value {
    Value::Array(b.iter().map(|x| Value::from(*x)).collect())
}

fn jopt_bytes(value: Option<&[u8]>) -> Value {
    value.map_or(Value::Null, jbytes)
}

fn string_schema() -> Value {
    json!({ "type": "string" })
}

fn bool_schema() -> Value {
    json!({ "type": "boolean" })
}

fn integer_schema() -> Value {
    json!({ "type": "integer" })
}

fn null_schema() -> Value {
    json!({ "type": "null" })
}

fn object_schema() -> Value {
    json!({ "type": "object", "additionalProperties": true })
}

fn open_object_schema() -> Value {
    json!({ "type": "object" })
}

fn string_array_schema() -> Value {
    array_schema(string_schema())
}

fn string_array_map_schema() -> Value {
    json!({ "type": "object", "additionalProperties": string_array_schema() })
}

fn bytes_schema() -> Value {
    json!({ "type": "array", "items": { "type": "integer", "minimum": 0, "maximum": 255 } })
}

fn digest_string_schema() -> Value {
    string_schema()
}

fn watch_subscribe_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "cursor": string_schema()
        },
        "required": ["cursor"],
        "additionalProperties": false
    })
}

fn domain_change_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "domain": string_schema(),
            "schema_version": integer_schema(),
            "kind": string_schema(),
            "key": bytes_schema(),
            "before": nullable(digest_string_schema()),
            "after": nullable(digest_string_schema()),
            "detail": nullable(bytes_schema())
        },
        "required": ["domain", "schema_version", "kind", "key", "before", "after", "detail"],
        "additionalProperties": false
    })
}

fn unsupported_domain_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "domain": string_schema(),
            "capability": string_schema()
        },
        "required": ["domain", "capability"],
        "additionalProperties": false
    })
}

fn change_event_schema(include_lmdiff: bool) -> Value {
    let mut properties = serde_json::Map::from_iter([
        ("workspace".to_string(), string_schema()),
        ("ref".to_string(), string_schema()),
        ("commit".to_string(), digest_string_schema()),
        ("parent".to_string(), nullable(digest_string_schema())),
        ("seq".to_string(), integer_schema()),
        (
            "changes".to_string(),
            json!({ "type": "array", "items": domain_change_schema() }),
        ),
        (
            "unsupported_domains".to_string(),
            json!({ "type": "array", "items": unsupported_domain_schema() }),
        ),
    ]);
    let mut required = vec![
        "workspace",
        "ref",
        "commit",
        "parent",
        "seq",
        "changes",
        "unsupported_domains",
    ];
    if include_lmdiff {
        properties.insert("lmdiff".to_string(), nullable(bytes_schema()));
        required.push("lmdiff");
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

fn substrate_data_change_event_schema() -> Value {
    let Value::Object(mut schema) = change_event_schema(true) else {
        return object_schema();
    };
    let Some(Value::Object(properties)) = schema.get_mut("properties") else {
        return Value::Object(schema);
    };
    properties.insert("kind".to_string(), json!({ "const": "data" }));
    let Some(Value::Array(required)) = schema.get_mut("required") else {
        return Value::Object(schema);
    };
    required.insert(0, json!("kind"));
    Value::Object(schema)
}

fn substrate_operation_change_event_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "kind": { "const": "operation" },
            "workspace_id": string_schema(),
            "app_id": string_schema(),
            "scope_id": string_schema(),
            "operation_id": string_schema(),
            "operation_kind": string_schema(),
            "sequence": integer_schema(),
            "actor_principal": string_schema(),
            "timestamp_ms": integer_schema(),
            "root_after": digest_string_schema(),
            "payload_digest": digest_string_schema(),
            "policy_labels": { "type": "array", "items": string_schema() }
        },
        "required": [
            "kind",
            "workspace_id",
            "app_id",
            "scope_id",
            "operation_id",
            "operation_kind",
            "sequence",
            "actor_principal",
            "timestamp_ms",
            "root_after",
            "payload_digest",
            "policy_labels"
        ],
        "additionalProperties": false
    })
}

fn substrate_change_event_schema() -> Value {
    json!({
        "oneOf": [
            substrate_data_change_event_schema(),
            substrate_operation_change_event_schema()
        ]
    })
}

fn watch_poll_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "events": {
                "type": "array",
                "items": change_event_schema(false)
            },
            "next": string_schema()
        },
        "required": ["events", "next"],
        "additionalProperties": false
    })
}

fn substrate_changes_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "events": {
                "type": "array",
                "items": substrate_change_event_schema()
            },
            "next": string_schema()
        },
        "required": ["events", "next"],
        "additionalProperties": false
    })
}

fn chat_events_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "events": {
                "type": "array",
                "items": substrate_operation_change_event_schema()
            },
            "next": string_schema()
        },
        "required": ["events", "next"],
        "additionalProperties": false
    })
}

fn substrate_refs_schema() -> Value {
    let string_array_schema = json!({ "type": "array", "items": string_schema() });
    json!({
        "type": "object",
        "properties": {
            "target": string_schema(),
            "inbound": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "source_facet": string_schema(),
                        "source_collection": string_schema(),
                        "source_id": string_schema(),
                        "field": string_schema(),
                        "relation": string_schema(),
                        "span_start": integer_schema(),
                        "span_end": integer_schema(),
                        "evidence": string_schema()
                    },
                    "required": ["source_facet", "source_collection", "source_id", "field", "relation", "span_start", "span_end", "evidence"],
                    "additionalProperties": false
                }
            },
            "indexed_facets": string_array_schema,
            "degraded": {
                "type": "object",
                "properties": {
                    "is_degraded": bool_schema(),
                    "reason": string_schema()
                },
                "required": ["is_degraded", "reason"],
                "additionalProperties": false
            }
        },
        "required": ["target", "inbound", "indexed_facets", "degraded"],
        "additionalProperties": false
    })
}

fn substrate_alias_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "alias": string_schema(),
            "target": string_schema(),
            "scope_id": string_schema(),
            "kind": string_schema(),
            "retired": bool_schema(),
            "sequence": nullable(integer_schema())
        },
        "required": ["alias", "target", "scope_id", "kind", "retired", "sequence"],
        "additionalProperties": false
    })
}

fn reference_reconciliation_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "pending": integer_schema(),
            "resolved": integer_schema(),
            "failed": integer_schema(),
            "processed": integer_schema()
        },
        "required": ["pending", "resolved", "failed", "processed"],
        "additionalProperties": false
    })
}

fn substrate_revision_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "entity_id": string_schema(),
            "revision": integer_schema(),
            "operation_id": string_schema(),
            "body_digest": digest_string_schema(),
            "body_len": integer_schema(),
            "body_media_type": string_schema(),
            "root": digest_string_schema(),
            "timestamp_ms": integer_schema()
        },
        "required": ["entity_id", "revision", "operation_id", "body_digest", "body_len", "body_media_type", "root", "timestamp_ms"],
        "additionalProperties": false
    })
}

fn substrate_checkpoint_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "scope_id": string_schema(),
            "checkpoint_id": string_schema(),
            "root": digest_string_schema(),
            "max_revision": integer_schema(),
            "operation_id": string_schema(),
            "created_at_ms": integer_schema()
        },
        "required": ["scope_id", "checkpoint_id", "root", "max_revision", "operation_id", "created_at_ms"],
        "additionalProperties": false
    })
}

fn substrate_history_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "scope_id": string_schema(),
            "entity_id": string_schema(),
            "index_present": bool_schema(),
            "revisions": {
                "type": "array",
                "items": substrate_revision_schema()
            },
            "latest": nullable(substrate_revision_schema()),
            "checkpoints": {
                "type": "array",
                "items": substrate_checkpoint_schema()
            }
        },
        "required": ["scope_id", "entity_id", "index_present", "revisions", "latest", "checkpoints"],
        "additionalProperties": false
    })
}

fn substrate_revision_lookup_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "scope_id": string_schema(),
            "entity_id": string_schema(),
            "index_present": bool_schema(),
            "revision": nullable(substrate_revision_schema())
        },
        "required": ["scope_id", "entity_id", "index_present", "revision"],
        "additionalProperties": false
    })
}

fn substrate_checkpoint_lookup_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "scope_id": string_schema(),
            "index_present": bool_schema(),
            "revision": integer_schema(),
            "checkpoint": nullable(substrate_checkpoint_schema())
        },
        "required": ["scope_id", "index_present", "revision", "checkpoint"],
        "additionalProperties": false
    })
}

fn substrate_transact_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "applied": integer_schema(),
            "results": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "kind": string_schema(),
                        "value": {}
                    },
                    "required": ["kind", "value"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["applied", "results"],
        "additionalProperties": false
    })
}

fn view_definition_schema() -> Value {
    let string_array_schema = json!({ "type": "array", "items": string_schema() });
    json!({
        "type": "object",
        "properties": {
            "view_id": string_schema(),
            "source_scopes": string_array_schema,
            "source_facets": string_array_schema,
            "projection_ref": string_schema(),
            "output_facet": nullable(string_schema()),
            "media_type": string_schema(),
            "freshness_policy": string_schema(),
            "output_digest": nullable(digest_string_schema()),
            "source_digests": string_array_schema,
            "projection": {
                "type": "object"
            }
        },
        "required": ["view_id", "source_scopes", "source_facets", "projection_ref", "output_facet", "media_type", "freshness_policy", "output_digest", "source_digests"],
        "additionalProperties": false
    })
}

fn write_admission_policy_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace": string_schema(),
            "surface": string_schema(),
            "scope_id": string_schema(),
            "default_mode": string_schema(),
            "mandatory_targets": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "target_kind": string_schema(),
                        "target_id": string_schema()
                    },
                    "required": ["target_kind", "target_id"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["workspace", "surface", "scope_id", "default_mode", "mandatory_targets"],
        "additionalProperties": false
    })
}

fn ticket_project_schema() -> Value {
    let contract_schema = json!({
        "type": "object",
        "properties": {
            "summary": string_schema(),
            "details": nullable(string_schema())
        },
        "required": ["summary", "details"],
        "additionalProperties": false
    });
    let contracts_schema = json!({
        "type": "object",
        "properties": {
            "note": string_schema(),
            "owner": contract_schema.clone(),
            "worker": contract_schema
        },
        "required": ["note", "owner", "worker"],
        "additionalProperties": false
    });
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "project_id": string_schema(),
            "key_prefix": string_schema(),
            "name": string_schema(),
            "next_ticket_number": integer_schema(),
            "default_projection": string_schema(),
            "enabled_projections": string_array_schema(),
            "lifecycle_authorization_policy": string_schema(),
            "project_owner_principal": nullable(string_schema()),
            "acceptance_authorities": string_array_schema(),
            "acceptance_evidence_enforcement": bool_schema(),
            "required_acceptance_evidence_keys": string_array_schema(),
            "contracts": contracts_schema,
            "active_workflow_version": nullable(string_schema()),
            "profile_root": digest_string_schema(),
            "operation_id": string_schema(),
            "sequence": integer_schema()
        },
        "required": ["workspace_id", "project_id", "key_prefix", "name", "next_ticket_number", "default_projection", "enabled_projections", "lifecycle_authorization_policy", "project_owner_principal", "acceptance_authorities", "acceptance_evidence_enforcement", "required_acceptance_evidence_keys", "contracts", "active_workflow_version", "profile_root", "operation_id", "sequence"],
        "additionalProperties": false
    })
}

fn ticket_projects_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projects": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "project_id": string_schema(),
                        "key_prefix": string_schema(),
                        "name": string_schema(),
                        "next_ticket_number": integer_schema(),
                        "default_projection": string_schema()
                    },
                    "required": ["project_id", "key_prefix", "name", "next_ticket_number", "default_projection"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["projects"],
        "additionalProperties": false
    })
}

fn ticket_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "ticket_id": string_schema(),
            "project_id": string_schema(),
            "primary_key": string_schema(),
            "ticket_type": string_schema(),
            "projection_profile": string_schema(),
            "projection_kind": string_schema(),
            "projection_source": string_schema(),
            "projection_selection_source": string_schema(),
            "external_source": nullable(string_schema()),
            "external_id": nullable(string_schema()),
            "fields": object_schema(),
            "policy_labels": string_array_schema(),
            "relations": array_schema(ticket_relation_compact_schema()),
            "relation_rollup": ticket_relation_rollup_schema(),
            "depends_on": string_array_schema(),
            "blocks": string_array_schema(),
            "comments": { "type": "array", "items": {
                "type": "object",
                "properties": {
                    "comment_id": string_schema(),
                    "comment_type": string_schema(),
                    "author_principal": string_schema(),
                    "created_at_ms": integer_schema(),
                    "updated_at_ms": nullable(integer_schema()),
                    "redacted": bool_schema()
                },
                "required": ["comment_id", "comment_type", "author_principal", "created_at_ms", "updated_at_ms", "redacted"],
                "additionalProperties": false
            } },
            "profile_root": digest_string_schema(),
            "operation_id": nullable(string_schema()),
            "sequence": nullable(integer_schema())
        },
        "required": [
            "workspace_id",
            "ticket_id",
            "project_id",
            "primary_key",
            "ticket_type",
            "projection_profile",
            "projection_kind",
            "projection_source",
            "projection_selection_source",
            "external_source",
            "external_id",
            "fields",
            "policy_labels",
            "relations",
            "relation_rollup",
            "depends_on",
            "blocks",
            "comments",
            "profile_root",
            "operation_id",
            "sequence"
        ],
        "additionalProperties": true
    })
}

fn mutation_change_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "kind": string_schema()
        },
        "required": ["kind"],
        "additionalProperties": true
    })
}

fn mutation_receipt_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "operation": string_schema(),
            "resource_kind": string_schema(),
            "resource_id": string_schema(),
            "operation_id": nullable(string_schema()),
            "root_before": nullable(digest_string_schema()),
            "root_after": nullable(digest_string_schema()),
            "changes": {
                "type": "array",
                "items": mutation_change_schema()
            }
        },
        "required": [
            "operation",
            "resource_kind",
            "resource_id",
            "operation_id",
            "root_before",
            "root_after",
            "changes"
        ],
        "additionalProperties": false
    })
}

fn mutation_envelope_schema(resource_schema: Value) -> Value {
    json!({
        "type": "object",
        "properties": {
            "resource": resource_schema,
            "receipt": mutation_receipt_schema()
        },
        "required": ["resource", "receipt"],
        "additionalProperties": false
    })
}

fn ticket_field_catalog_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projection": string_schema(),
            "operation": string_schema(),
            "strict_unknown_fields": bool_schema(),
            "custom_fields_source": string_schema(),
            "fields": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "native_field": string_schema(),
                        "write_path": string_schema(),
                        "aliases": { "type": "array", "items": string_schema() },
                        "field_type": string_schema(),
                        "cardinality": string_schema(),
                        "settable": bool_schema(),
                        "required_on_create": bool_schema(),
                        "required_on_update": bool_schema(),
                        "searchable": bool_schema(),
                        "orderable": bool_schema(),
                        "max_length": nullable(integer_schema()),
                        "enum_values": { "type": "array", "items": string_schema() },
                        "write_semantics": string_schema()
                    },
                    "required": ["native_field", "write_path", "aliases", "field_type", "cardinality", "settable", "required_on_create", "required_on_update", "searchable", "orderable", "max_length", "enum_values", "write_semantics"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["projection", "operation", "strict_unknown_fields", "custom_fields_source", "fields"],
        "additionalProperties": false
    })
}

fn ticket_board_schema() -> Value {
    let column_schema = json!({
        "type": "object",
        "properties": {
            "column_id": string_schema(),
            "name": string_schema(),
            "mapped_statuses": { "type": "array", "items": string_schema() },
            "wip_limit": nullable(integer_schema()),
            "hidden": bool_schema(),
            "rank": integer_schema()
        },
        "required": ["column_id", "name", "mapped_statuses", "wip_limit", "hidden", "rank"],
        "additionalProperties": false
    });
    let swimlane_schema = json!({
        "type": "object",
        "properties": {
            "swimlane_id": string_schema(),
            "name": string_schema(),
            "predicate": nullable(string_schema()),
            "rank": integer_schema()
        },
        "required": ["swimlane_id", "name", "predicate", "rank"],
        "additionalProperties": false
    });
    let card_schema = json!({
        "type": "object",
        "properties": {
            "board_id": string_schema(),
            "ticket_id": string_schema(),
            "column_id": string_schema(),
            "rank_token": string_schema(),
            "swimlane_id": nullable(string_schema()),
            "updated_at": integer_schema(),
            "updated_by": string_schema()
        },
        "required": ["board_id", "ticket_id", "column_id", "rank_token", "swimlane_id", "updated_at", "updated_by"],
        "additionalProperties": false
    });
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "board_id": string_schema(),
            "board_key": string_schema(),
            "name": string_schema(),
            "description": string_schema(),
            "project_id": string_schema(),
            "scope": object_schema(),
            "mode": string_schema(),
            "columns": array_schema(column_schema),
            "swimlanes": array_schema(swimlane_schema),
            "card_display_fields": { "type": "array", "items": string_schema() },
            "owner_principal": nullable(string_schema()),
            "coordinator_principal": nullable(string_schema()),
            "board_status": string_schema(),
            "cards": array_schema(card_schema),
            "profile_root": digest_string_schema(),
            "operation_id": nullable(string_schema()),
            "sequence": nullable(integer_schema())
        },
        "required": ["workspace_id", "board_id", "board_key", "name", "description", "project_id", "scope", "mode", "columns", "swimlanes", "card_display_fields", "owner_principal", "coordinator_principal", "board_status", "cards", "profile_root", "operation_id", "sequence"],
        "additionalProperties": false
    })
}

fn lane_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "lane_id": string_schema(),
            "lane_key": string_schema(),
            "title": string_schema(),
            "description": string_schema(),
            "lane_kind": string_schema(),
            "owner_principal": nullable(string_schema()),
            "lane_status": string_schema(),
            "lane_tickets": {
                "type": "array",
                "items": string_schema()
            },
            "active_ticket_id": nullable(string_schema()),
            "status_report": string_schema(),
            "reviewer_feedback": string_schema(),
            "updated_at": integer_schema(),
            "updated_by": string_schema()
        },
        "required": ["lane_id", "lane_key", "title", "description", "lane_kind", "owner_principal", "lane_status", "lane_tickets", "active_ticket_id", "status_report", "reviewer_feedback", "updated_at", "updated_by"],
        "additionalProperties": false
    })
}

fn lane_ticket_view_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ticket_id": string_schema(),
            "status": nullable(string_schema()),
            "priority": nullable(string_schema()),
            "title": nullable(string_schema())
        },
        "required": ["ticket_id", "status", "priority", "title"],
        "additionalProperties": false
    })
}

fn lane_status_counts_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "blocked": integer_schema(),
            "waiting_for_decision": integer_schema(),
            "feedback_available": integer_schema(),
            "waiting_for_review": integer_schema(),
            "in_progress": integer_schema(),
            "backlog": integer_schema(),
            "accepted": integer_schema(),
            "missing": integer_schema(),
            "total": integer_schema(),
            "next_ticket_id": nullable(string_schema())
        },
        "required": [
            "blocked",
            "waiting_for_decision",
            "feedback_available",
            "waiting_for_review",
            "in_progress",
            "backlog",
            "accepted",
            "missing",
            "total",
            "next_ticket_id"
        ],
        "additionalProperties": false
    })
}

fn lane_view_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "lane_id": string_schema(),
            "lane_key": string_schema(),
            "title": string_schema(),
            "description": string_schema(),
            "lane_kind": string_schema(),
            "owner_principal": nullable(string_schema()),
            "owner_display": nullable(string_schema()),
            "stored_lane_status": string_schema(),
            "display_status": string_schema(),
            "status_counts": lane_status_counts_schema(),
            "lane_tickets": array_schema(lane_ticket_view_schema()),
            "active_ticket_id": nullable(string_schema()),
            "status_report": string_schema(),
            "reviewer_feedback": string_schema(),
            "updated_at": integer_schema(),
            "updated_by": string_schema()
        },
        "required": ["lane_id", "lane_key", "title", "description", "lane_kind", "owner_principal", "owner_display", "stored_lane_status", "display_status", "status_counts", "lane_tickets", "active_ticket_id", "status_report", "reviewer_feedback", "updated_at", "updated_by"],
        "additionalProperties": false
    })
}

fn lane_compact_view_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "lane_id": string_schema(),
            "lane_key": string_schema(),
            "title": string_schema(),
            "display_status": string_schema(),
            "status_counts": lane_status_counts_schema(),
            "lane_tickets": array_schema(string_schema())
        },
        "required": ["lane_id", "lane_key", "title", "display_status", "status_counts", "lane_tickets"],
        "additionalProperties": false
    })
}

/// Lanes reads return the compact projection by default and the full [`lane_view_schema`] when
/// `detailed` is set, so the advertised result schema accepts either shape.
fn lane_view_result_schema() -> Value {
    json!({ "anyOf": [lane_compact_view_schema(), lane_view_schema()] })
}

/// One per-record fail-soft decode failure surfaced by `lanes_list`: the offending lane document id
/// and a human-readable reason. Malformed coordination records appear here instead of being dropped.
fn lane_decode_diagnostic_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "lane_id": string_schema(),
            "error": string_schema()
        },
        "required": ["lane_id", "error"],
        "additionalProperties": false
    })
}

/// `lanes_list` returns the healthy lane views plus one diagnostic per record that failed to decode,
/// so a single malformed lane never makes the whole coordination surface unreadable.
fn lanes_list_result_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "lanes": array_schema(lane_view_result_schema()),
            "diagnostics": array_schema(lane_decode_diagnostic_schema())
        },
        "required": ["lanes", "diagnostics"],
        "additionalProperties": false
    })
}

fn ticket_history_schema() -> Value {
    json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "sequence": integer_schema(),
                "operation_id": string_schema(),
                "operation_kind": string_schema(),
                "target_entity_id": nullable(string_schema()),
                "comments": array_schema(ticket_comment_schema()),
                "envelope": object_schema()
            },
            "required": ["sequence", "operation_id", "operation_kind", "target_entity_id", "comments", "envelope"],
            "additionalProperties": false
        }
    })
}

fn ticket_comment_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "comment_id": string_schema(),
            "comment_type": string_schema(),
            "author_principal": string_schema(),
            "body": string_schema(),
            "content_type": string_schema(),
            "evidence": nullable(string_array_map_schema()),
            "created_at_ms": integer_schema(),
            "updated_at_ms": nullable(integer_schema()),
            "redacted": bool_schema()
        },
        "required": [
            "comment_id",
            "comment_type",
            "author_principal",
            "body",
            "content_type",
            "evidence",
            "created_at_ms",
            "updated_at_ms",
            "redacted"
        ],
        "additionalProperties": false
    })
}

fn ticket_relation_target_state_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "primary_key": string_schema(),
            "title": nullable(string_schema()),
            "status": nullable(string_schema()),
            "blocked": bool_schema()
        },
        "required": ["primary_key", "title", "status", "blocked"],
        "additionalProperties": false
    })
}

fn ticket_relation_compact_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "relation_id": string_schema(),
            "kind": string_schema(),
            "target_type": string_schema(),
            "target_id": string_schema(),
            "target": nullable(ticket_relation_target_state_schema())
        },
        "required": ["relation_id", "kind", "target_type", "target_id", "target"],
        "additionalProperties": false
    })
}

fn ticket_relation_rollup_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "total_children": integer_schema(),
            "accepted_children": integer_schema(),
            "blocked_children": integer_schema(),
            "waiting_for_review_children": integer_schema(),
            "feedback_available_children": integer_schema(),
            "in_progress_children": integer_schema()
        },
        "required": [
            "total_children",
            "accepted_children",
            "blocked_children",
            "waiting_for_review_children",
            "feedback_available_children",
            "in_progress_children"
        ],
        "additionalProperties": false
    })
}

fn ticket_relation_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "ticket_id": string_schema(),
            "relation_id": string_schema(),
            "kind": string_schema(),
            "target_type": string_schema(),
            "target_id": string_schema(),
            "profile_root": digest_string_schema(),
            "operation_id": string_schema(),
            "sequence": integer_schema(),
            "graph_edge_id": string_schema()
        },
        "required": ["workspace_id", "ticket_id", "relation_id", "kind", "target_type", "target_id", "profile_root", "operation_id", "sequence", "graph_edge_id"],
        "additionalProperties": false
    })
}

fn ticket_relation_view_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "direction": string_schema(),
            "kind": string_schema(),
            "target_ticket_id": string_schema(),
            "target_title": nullable(string_schema())
        },
        "required": ["direction", "kind", "target_ticket_id", "target_title"],
        "additionalProperties": false
    })
}

fn ticket_relations_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "relations": array_schema(ticket_relation_view_schema())
        },
        "required": ["relations"],
        "additionalProperties": false
    })
}

fn space_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "space_id": string_schema(),
            "title": string_schema(),
            "archived": bool_schema()
        },
        "required": ["workspace_id", "space_id", "title", "archived"],
        "additionalProperties": false
    })
}

fn page_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "page_id": string_schema(),
            "space_id": string_schema(),
            "parent_page_id": nullable(string_schema()),
            "title": string_schema(),
            "current_revision": nullable(integer_schema()),
            "deleted": bool_schema(),
            "status": string_schema(),
            "body": nullable(bytes_schema()),
            "draft_body": nullable(bytes_schema()),
            "body_text": nullable(string_schema()),
            "draft_body_text": nullable(string_schema()),
            "rendered_body": nullable(string_schema()),
            "draft_rendered_body": nullable(string_schema()),
            "render_issues": array_schema(page_render_issue_schema()),
            "draft_render_issues": array_schema(page_render_issue_schema()),
            "profile_root": digest_string_schema()
        },
        "required": [
            "workspace_id",
            "page_id",
            "space_id",
            "parent_page_id",
            "title",
            "current_revision",
            "deleted",
            "status",
            "body",
            "draft_body",
            "body_text",
            "draft_body_text",
            "rendered_body",
            "draft_rendered_body",
            "render_issues",
            "draft_render_issues",
            "profile_root"
        ],
        "additionalProperties": false
    })
}

fn page_render_issue_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "kind": string_schema(),
            "entity_id": string_schema(),
            "block_id": nullable(string_schema())
        },
        "required": ["kind", "entity_id", "block_id"],
        "additionalProperties": false
    })
}

fn lifecycle_gate_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "gate_id": string_schema(),
            "label": string_schema(),
            "kind": string_schema(),
            "predicate_digest": nullable(digest_string_schema()),
            "required_role": nullable(string_schema())
        },
        "required": ["gate_id", "label", "kind", "predicate_digest", "required_role"],
        "additionalProperties": false
    })
}

fn lifecycle_stage_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "stage_id": string_schema(),
            "label": string_schema(),
            "entry_gates": array_schema(lifecycle_gate_schema()),
            "exit_gates": array_schema(lifecycle_gate_schema()),
            "snapshot_policy": string_schema(),
            "surfaced_tools": array_schema(string_schema()),
            "prompt_refs": array_schema(string_schema())
        },
        "required": [
            "stage_id",
            "label",
            "entry_gates",
            "exit_gates",
            "snapshot_policy",
            "surfaced_tools",
            "prompt_refs"
        ],
        "additionalProperties": false
    })
}

fn lifecycle_definition_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "definition_id": string_schema(),
            "version": string_schema(),
            "initial_stage_id": string_schema(),
            "stages": array_schema(lifecycle_stage_schema()),
            "definition_cbor_hex": string_schema()
        },
        "required": [
            "workspace_id",
            "definition_id",
            "version",
            "initial_stage_id",
            "stages",
            "definition_cbor_hex"
        ],
        "additionalProperties": false
    })
}

fn lifecycle_gate_evaluation_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "gate_id": string_schema(),
            "passed": bool_schema(),
            "principal_id": nullable(string_schema()),
            "evidence_digest": nullable(digest_string_schema()),
            "evaluated_at_ms": integer_schema()
        },
        "required": [
            "gate_id",
            "passed",
            "principal_id",
            "evidence_digest",
            "evaluated_at_ms"
        ],
        "additionalProperties": false
    })
}

fn lifecycle_transition_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "transition_id": string_schema(),
            "instance_id": string_schema(),
            "definition_id": string_schema(),
            "definition_version": string_schema(),
            "from_stage_id": string_schema(),
            "to_stage_id": string_schema(),
            "actor_principal_id": string_schema(),
            "gate_evaluations": array_schema(lifecycle_gate_evaluation_schema()),
            "snapshot_digest": nullable(digest_string_schema()),
            "recorded_at_ms": integer_schema()
        },
        "required": [
            "transition_id",
            "instance_id",
            "definition_id",
            "definition_version",
            "from_stage_id",
            "to_stage_id",
            "actor_principal_id",
            "gate_evaluations",
            "snapshot_digest",
            "recorded_at_ms"
        ],
        "additionalProperties": false
    })
}

fn lifecycle_instance_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "instance_id": string_schema(),
            "definition_id": string_schema(),
            "definition_version": string_schema(),
            "subject_refs": array_schema(string_schema()),
            "current_stage_id": string_schema(),
            "stage_history": array_schema(lifecycle_transition_schema()),
            "instance_cbor_hex": string_schema()
        },
        "required": [
            "workspace_id",
            "instance_id",
            "definition_id",
            "definition_version",
            "subject_refs",
            "current_stage_id",
            "stage_history",
            "instance_cbor_hex"
        ],
        "additionalProperties": false
    })
}

fn lifecycle_surface_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "instance_id": string_schema(),
            "stage_id": string_schema(),
            "surfaced_tools": array_schema(string_schema()),
            "prompt_refs": array_schema(string_schema()),
            "read_only": bool_schema(),
            "surface_cbor_hex": string_schema()
        },
        "required": [
            "workspace_id",
            "instance_id",
            "stage_id",
            "surfaced_tools",
            "prompt_refs",
            "read_only",
            "surface_cbor_hex"
        ],
        "additionalProperties": false
    })
}

fn lifecycle_active_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "active": bool_schema(),
            "workspace": nullable(string_schema()),
            "workspace_id": nullable(string_schema()),
            "instance_id": nullable(string_schema()),
            "surface": nullable(lifecycle_surface_schema())
        },
        "required": ["active", "workspace", "workspace_id", "instance_id", "surface"],
        "additionalProperties": false
    })
}

fn lifecycle_snapshot_plan_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "instance_id": string_schema(),
            "from_stage_id": string_schema(),
            "to_stage_id": string_schema(),
            "required": bool_schema(),
            "subject_refs": array_schema(string_schema()),
            "policy": string_schema(),
            "plan_cbor_hex": string_schema()
        },
        "required": [
            "workspace_id",
            "instance_id",
            "from_stage_id",
            "to_stage_id",
            "required",
            "subject_refs",
            "policy",
            "plan_cbor_hex"
        ],
        "additionalProperties": false
    })
}

fn lifecycle_snapshot_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "snapshot_id": string_schema(),
            "instance_id": string_schema(),
            "transition_id": string_schema(),
            "from_stage_id": string_schema(),
            "to_stage_id": string_schema(),
            "subject_refs": array_schema(string_schema()),
            "policy": string_schema(),
            "snapshot_digest": digest_string_schema(),
            "recorded_at_ms": integer_schema(),
            "snapshot_cbor_hex": string_schema()
        },
        "required": [
            "workspace_id",
            "snapshot_id",
            "instance_id",
            "transition_id",
            "from_stage_id",
            "to_stage_id",
            "subject_refs",
            "policy",
            "snapshot_digest",
            "recorded_at_ms",
            "snapshot_cbor_hex"
        ],
        "additionalProperties": false
    })
}

fn lifecycle_operation_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "sequence": integer_schema(),
            "operation_id": string_schema(),
            "operation_kind": string_schema(),
            "instance_id": string_schema(),
            "target_entity_id": nullable(string_schema()),
            "root_after": digest_string_schema(),
            "envelope_cbor_hex": string_schema()
        },
        "required": [
            "sequence",
            "operation_id",
            "operation_kind",
            "instance_id",
            "target_entity_id",
            "root_after",
            "envelope_cbor_hex"
        ],
        "additionalProperties": false
    })
}

fn lifecycle_operation_log_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "records": array_schema(lifecycle_operation_schema())
        },
        "required": ["workspace_id", "records"],
        "additionalProperties": false
    })
}

fn lifecycle_transition_result_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "instance": lifecycle_instance_schema(),
            "transition": lifecycle_transition_schema(),
            "surface": lifecycle_surface_schema(),
            "snapshot": nullable(lifecycle_snapshot_schema()),
            "operation_log": lifecycle_operation_log_schema()
        },
        "required": ["instance", "transition", "surface", "snapshot", "operation_log"],
        "additionalProperties": false
    })
}

fn chat_reaction_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "kind": string_schema(),
            "principal": string_schema()
        },
        "required": ["kind", "principal"],
        "additionalProperties": false
    })
}

fn chat_emoji_registry_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "custom": array_schema(string_schema())
        },
        "required": ["workspace_id", "custom"],
        "additionalProperties": false
    })
}

fn chat_message_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "message_id": string_schema(),
            "thread_id": nullable(string_schema()),
            "body": bytes_schema(),
            "body_text": nullable(string_schema()),
            "author_principal": string_schema(),
            "created_at_ms": integer_schema(),
            "updated_at_ms": integer_schema(),
            "redacted": bool_schema(),
            "reactions": array_schema(chat_reaction_schema())
        },
        "required": [
            "message_id",
            "thread_id",
            "body",
            "body_text",
            "author_principal",
            "created_at_ms",
            "updated_at_ms",
            "redacted",
            "reactions"
        ],
        "additionalProperties": false
    })
}

fn chat_thread_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "thread_id": string_schema(),
            "parent_message_id": string_schema(),
            "created_at_ms": integer_schema()
        },
        "required": ["thread_id", "parent_message_id", "created_at_ms"],
        "additionalProperties": false
    })
}

fn chat_channel_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "channel_id": string_schema(),
            "channel_handle": string_schema(),
            "channel_name": string_schema(),
            "messages": array_schema(chat_message_schema()),
            "threads": array_schema(chat_thread_schema()),
            "tasks": array_schema(chat_task_schema()),
            "agent_invocations": array_schema(chat_agent_invocation_schema()),
            "handoffs": array_schema(chat_handoff_schema())
        },
        "required": [
            "workspace_id",
            "channel_id",
            "channel_handle",
            "channel_name",
            "messages",
            "threads",
            "tasks",
            "agent_invocations",
            "handoffs"
        ],
        "additionalProperties": false
    })
}

fn chat_channel_directory_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "channel_id": string_schema(),
            "channel_handle": string_schema(),
            "channel_name": string_schema()
        },
        "required": ["workspace_id", "channel_id", "channel_handle", "channel_name"],
        "additionalProperties": false
    })
}

fn chat_task_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "task_id": string_schema(),
            "message_id": nullable(string_schema()),
            "title": string_schema(),
            "created_by": string_schema(),
            "created_at_ms": integer_schema(),
            "state": {
                "type": "object",
                "properties": {
                    "kind": string_schema(),
                    "claim_id": string_schema(),
                    "claimant_principal": string_schema(),
                    "claimed_by": string_schema(),
                    "claimed_at_ms": integer_schema(),
                    "lease_token": nullable(string_schema()),
                    "completed_by": string_schema(),
                    "completed_principal": string_schema(),
                    "completed_at_ms": integer_schema(),
                    "result_message_id": nullable(string_schema())
                },
                "required": ["kind"],
                "additionalProperties": false
            }
        },
        "required": ["task_id", "message_id", "title", "created_by", "created_at_ms", "state"],
        "additionalProperties": false
    })
}

fn chat_agent_invocation_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "invocation_id": string_schema(),
            "agent_principal": string_schema(),
            "requested_by": string_schema(),
            "requested_at_ms": integer_schema(),
            "source_message_ids": array_schema(string_schema()),
            "prompt": bytes_schema(),
            "prompt_text": nullable(string_schema()),
            "reply_message_ids": array_schema(string_schema())
        },
        "required": [
            "invocation_id",
            "agent_principal",
            "requested_by",
            "requested_at_ms",
            "source_message_ids",
            "prompt",
            "prompt_text",
            "reply_message_ids"
        ],
        "additionalProperties": false
    })
}

fn chat_handoff_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "handoff_id": string_schema(),
            "from_agent_principal": string_schema(),
            "to_principal": nullable(string_schema()),
            "requested_by": string_schema(),
            "requested_at_ms": integer_schema(),
            "reason": nullable(string_schema())
        },
        "required": [
            "handoff_id",
            "from_agent_principal",
            "to_principal",
            "requested_by",
            "requested_at_ms",
            "reason"
        ],
        "additionalProperties": false
    })
}

fn meetings_projection_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "output_id": string_schema(),
            "projection": string_schema(),
            "action": string_schema(),
            "output_ref": string_schema(),
            "entity_kind": string_schema(),
            "entity_id": string_schema(),
            "source_ids": array_schema(string_schema()),
            "payload_cbor_hex": string_schema(),
            "redaction_state": nullable(string_schema()),
            "recorded_at_ms": integer_schema(),
            "record_cbor_hex": string_schema()
        },
        "required": [
            "output_id",
            "projection",
            "action",
            "output_ref",
            "entity_kind",
            "entity_id",
            "source_ids",
            "payload_cbor_hex",
            "redaction_state",
            "recorded_at_ms",
            "record_cbor_hex"
        ],
        "additionalProperties": false
    })
}

fn meetings_projection_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "profile_root": string_schema(),
            "outputs": array_schema(meetings_projection_output_schema()),
            "output_set_cbor_hex": string_schema()
        },
        "required": ["workspace_id", "profile_root", "outputs", "output_set_cbor_hex"],
        "additionalProperties": false
    })
}

fn meeting_summary_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "meeting_id": string_schema(),
            "title": string_schema(),
            "starts_at_ms": nullable(integer_schema()),
            "ends_at_ms": nullable(integer_schema()),
            "status": string_schema(),
            "source_refs": array_schema(string_schema()),
            "updated_at_ms": integer_schema()
        },
        "required": [
            "meeting_id",
            "title",
            "starts_at_ms",
            "ends_at_ms",
            "status",
            "source_refs",
            "updated_at_ms"
        ],
        "additionalProperties": false
    })
}

fn meetings_list_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "total": integer_schema(),
            "offset": integer_schema(),
            "limit": integer_schema(),
            "meetings": array_schema(meeting_summary_schema())
        },
        "required": ["workspace_id", "total", "offset", "limit", "meetings"],
        "additionalProperties": false
    })
}

fn meeting_detail_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "meeting_id": string_schema(),
            "title": string_schema(),
            "starts_at_ms": nullable(integer_schema()),
            "ends_at_ms": nullable(integer_schema()),
            "calendar_event_ref": nullable(string_schema()),
            "owner_principal": nullable(string_schema()),
            "attendee_refs": array_schema(string_schema()),
            "folder_refs": array_schema(string_schema()),
            "source_refs": array_schema(string_schema()),
            "current_source_digest": string_schema(),
            "summary_ref": nullable(string_schema()),
            "status": string_schema(),
            "created_at_ms": integer_schema(),
            "updated_at_ms": integer_schema(),
            "annotations": array_schema(meeting_annotation_schema())
        },
        "required": [
            "workspace_id",
            "meeting_id",
            "title",
            "starts_at_ms",
            "ends_at_ms",
            "calendar_event_ref",
            "owner_principal",
            "attendee_refs",
            "folder_refs",
            "source_refs",
            "current_source_digest",
            "summary_ref",
            "status",
            "created_at_ms",
            "updated_at_ms",
            "annotations"
        ],
        "additionalProperties": false
    })
}

fn meeting_annotation_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "annotation_id": string_schema(),
            "meeting_id": string_schema(),
            "source_span_ids": array_schema(string_schema()),
            "kind": string_schema(),
            "label": string_schema(),
            "normalized_id": nullable(string_schema()),
            "confidence_ppm": nullable(integer_schema()),
            "evidence_digest": nullable(string_schema()),
            "extractor": nullable(string_schema()),
            "status": string_schema(),
            "created_at_ms": integer_schema(),
            "accepted_by": nullable(string_schema()),
            "accepted_at_ms": nullable(integer_schema())
        },
        "required": [
            "annotation_id",
            "meeting_id",
            "source_span_ids",
            "kind",
            "label",
            "normalized_id",
            "confidence_ppm",
            "evidence_digest",
            "extractor",
            "status",
            "created_at_ms",
            "accepted_by",
            "accepted_at_ms"
        ],
        "additionalProperties": false
    })
}

fn meetings_review_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "suggested_annotation_ids": array_schema(string_schema()),
            "accepted_annotation_ids": array_schema(string_schema()),
            "rejected_annotation_ids": array_schema(string_schema()),
            "vocabulary_terms": integer_schema(),
            "review_cbor_hex": string_schema()
        },
        "required": [
            "workspace_id",
            "suggested_annotation_ids",
            "accepted_annotation_ids",
            "rejected_annotation_ids",
            "vocabulary_terms",
            "review_cbor_hex"
        ],
        "additionalProperties": false
    })
}

fn meetings_annotation_review_write_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "annotation_id": string_schema(),
            "status": string_schema(),
            "accepted_by": nullable(string_schema()),
            "accepted_at_ms": nullable(integer_schema()),
            "record_cbor_hex": string_schema()
        },
        "required": [
            "workspace_id",
            "annotation_id",
            "status",
            "accepted_by",
            "accepted_at_ms",
            "record_cbor_hex"
        ],
        "additionalProperties": false
    })
}

fn meetings_vocabulary_review_write_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "term_id": string_schema(),
            "status": string_schema(),
            "reviewed_by": nullable(string_schema()),
            "reviewed_at_ms": nullable(integer_schema()),
            "record_cbor_hex": string_schema()
        },
        "required": [
            "workspace_id",
            "term_id",
            "status",
            "reviewed_by",
            "reviewed_at_ms",
            "record_cbor_hex"
        ],
        "additionalProperties": false
    })
}

fn meetings_entity_merge_write_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "merge_id": string_schema(),
            "canonical_entity_id": string_schema(),
            "merged_entity_ids": array_schema(string_schema()),
            "record_cbor_hex": string_schema()
        },
        "required": [
            "workspace_id",
            "merge_id",
            "canonical_entity_id",
            "merged_entity_ids",
            "record_cbor_hex"
        ],
        "additionalProperties": false
    })
}

fn studio_reindex_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace": string_schema(),
            "profile": string_schema(),
            "job_path": string_schema(),
            "state": string_schema(),
            "source_digest": digest_string_schema(),
            "model_id": string_schema(),
            "vector_records_indexed": integer_schema(),
            "vector_records_deleted": integer_schema()
        },
        "required": [
            "workspace",
            "profile",
            "job_path",
            "state",
            "source_digest",
            "model_id",
            "vector_records_indexed",
            "vector_records_deleted"
        ],
        "additionalProperties": false
    })
}

fn meetings_promotion_write_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "promotion_id": string_schema(),
            "operation_kind": string_schema(),
            "source_annotation_id": string_schema(),
            "target_profile": string_schema(),
            "target_entity_ref": string_schema(),
            "record_cbor_hex": string_schema()
        },
        "required": [
            "workspace_id",
            "promotion_id",
            "operation_kind",
            "source_annotation_id",
            "target_profile",
            "target_entity_ref",
            "record_cbor_hex"
        ],
        "additionalProperties": false
    })
}

fn meetings_ticket_promotion_write_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "promotion": meetings_promotion_write_schema(),
            "ticket": ticket_schema()
        },
        "required": ["promotion", "ticket"],
        "additionalProperties": false
    })
}

fn meetings_decision_promotion_write_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "promotion": meetings_promotion_write_schema(),
            "decision_ledger": string_schema(),
            "ledger_sequence": integer_schema()
        },
        "required": ["promotion", "decision_ledger", "ledger_sequence"],
        "additionalProperties": false
    })
}

fn meetings_lifecycle_promotion_write_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "promotion": meetings_promotion_write_schema(),
            "lifecycle": lifecycle_instance_schema()
        },
        "required": ["promotion", "lifecycle"],
        "additionalProperties": false
    })
}

fn reference_artifact_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "record_id": string_schema(),
            "kind": string_schema(),
            "entity_ref": string_schema(),
            "label": string_schema(),
            "source_ref": string_schema(),
            "source_operation_id": string_schema(),
            "target_ref": nullable(string_schema()),
            "created_by": string_schema(),
            "created_at_ms": integer_schema(),
            "record_cbor_hex": string_schema()
        },
        "required": [
            "workspace_id",
            "record_id",
            "kind",
            "entity_ref",
            "label",
            "source_ref",
            "source_operation_id",
            "target_ref",
            "created_by",
            "created_at_ms",
            "record_cbor_hex"
        ],
        "additionalProperties": false
    })
}

fn meetings_reference_artifact_promotion_write_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "promotion": meetings_promotion_write_schema(),
            "reference_artifact": reference_artifact_schema()
        },
        "required": ["promotion", "reference_artifact"],
        "additionalProperties": false
    })
}

fn import_batch_submit_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace": string_schema(),
            "workspace_id": string_schema(),
            "profile": string_schema(),
            "source_system": string_schema(),
            "source_scope": string_schema(),
            "coverage": string_schema(),
            "observed_at_ms": integer_schema(),
            "item_count": integer_schema(),
            "batch_digest": digest_string_schema(),
            "control_key": string_schema()
        },
        "required": [
            "workspace",
            "workspace_id",
            "profile",
            "source_system",
            "source_scope",
            "coverage",
            "observed_at_ms",
            "item_count",
            "batch_digest",
            "control_key"
        ],
        "additionalProperties": false
    })
}

fn import_batch_execute_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace": string_schema(),
            "workspace_id": string_schema(),
            "profile": string_schema(),
            "source_system": string_schema(),
            "source_scope": string_schema(),
            "coverage": string_schema(),
            "observed_at_ms": integer_schema(),
            "payload_count": integer_schema(),
            "execution_digest": digest_string_schema(),
            "control_key": string_schema(),
            "changed": bool_schema(),
            "dry_run": bool_schema(),
            "rows_imported": integer_schema(),
            "operations_planned": integer_schema(),
            "operations_applied": integer_schema(),
            "skipped": integer_schema(),
            "bytes_in": integer_schema(),
            "bytes_stored": integer_schema(),
            "warnings": {
                "type": "array",
                "items": string_schema()
            },
            "fidelity_issues": integer_schema()
        },
        "required": [
            "workspace",
            "workspace_id",
            "profile",
            "source_system",
            "source_scope",
            "coverage",
            "observed_at_ms",
            "payload_count",
            "execution_digest",
            "control_key",
            "changed",
            "dry_run",
            "rows_imported",
            "operations_planned",
            "operations_applied",
            "skipped",
            "bytes_in",
            "bytes_stored",
            "warnings",
            "fidelity_issues"
        ],
        "additionalProperties": false
    })
}

fn meetings_import_snapshot_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace": string_schema(),
            "workspace_id": string_schema(),
            "input_profile": string_schema(),
            "source_scope": string_schema(),
            "changed": bool_schema(),
            "dry_run": bool_schema(),
            "rows_imported": integer_schema(),
            "operations_planned": integer_schema(),
            "operations_applied": integer_schema(),
            "bytes_in": integer_schema(),
            "bytes_stored": integer_schema(),
            "payload_bytes": integer_schema(),
            "warnings": array_schema(string_schema()),
            "fidelity_issues": integer_schema()
        },
        "required": [
            "workspace",
            "workspace_id",
            "input_profile",
            "source_scope",
            "changed",
            "dry_run",
            "rows_imported",
            "operations_planned",
            "operations_applied",
            "bytes_in",
            "bytes_stored",
            "payload_bytes",
            "warnings",
            "fidelity_issues"
        ],
        "additionalProperties": false
    })
}

fn redmine_import_snapshot_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace": string_schema(),
            "workspace_id": string_schema(),
            "profile": string_schema(),
            "source_scope": string_schema(),
            "dry_run": bool_schema(),
            "rows_imported": integer_schema(),
            "operations_planned": integer_schema(),
            "operations_applied": integer_schema(),
            "skipped": integer_schema(),
            "bytes_in": integer_schema(),
            "bytes_stored": integer_schema(),
            "warnings": array_schema(string_schema()),
            "fidelity_issues": integer_schema()
        },
        "required": [
            "workspace",
            "workspace_id",
            "profile",
            "source_scope",
            "dry_run",
            "rows_imported",
            "operations_planned",
            "operations_applied",
            "skipped",
            "bytes_in",
            "bytes_stored",
            "warnings",
            "fidelity_issues"
        ],
        "additionalProperties": false
    })
}

fn chat_cursor_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "channel_id": string_schema(),
            "channel_handle": string_schema(),
            "principal": string_schema(),
            "next_sequence": integer_schema(),
            "head_sequence": integer_schema(),
            "unread_count": integer_schema()
        },
        "required": [
            "workspace_id",
            "channel_id",
            "channel_handle",
            "principal",
            "next_sequence",
            "head_sequence",
            "unread_count"
        ],
        "additionalProperties": false
    })
}

fn chat_presence_entry_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "channel_id": string_schema(),
            "principal": string_schema(),
            "status": string_schema(),
            "expires_at_ms": integer_schema()
        },
        "required": ["workspace_id", "channel_id", "principal", "status", "expires_at_ms"],
        "additionalProperties": false
    })
}

fn chat_presence_schema() -> Value {
    array_schema(chat_presence_entry_schema())
}

fn chat_write_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "channel_id": string_schema(),
            "channel_handle": string_schema(),
            "operation_id": string_schema(),
            "operation_kind": string_schema(),
            "sequence": integer_schema(),
            "root_after": digest_string_schema()
        },
        "required": [
            "workspace_id",
            "channel_id",
            "channel_handle",
            "operation_id",
            "operation_kind",
            "sequence",
            "root_after"
        ],
        "additionalProperties": false
    })
}

fn drive_entry_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": string_schema(),
            "fold_key": string_schema(),
            "node_id": string_schema(),
            "kind": string_schema()
        },
        "required": ["name", "fold_key", "node_id", "kind"],
        "additionalProperties": false
    })
}

fn drive_version_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "file_id": string_schema(),
            "version": integer_schema(),
            "operation_id": string_schema(),
            "author_principal": string_schema(),
            "timestamp_ms": integer_schema(),
            "content_digest": digest_string_schema(),
            "manifest_digest": nullable(digest_string_schema()),
            "size": integer_schema()
        },
        "required": [
            "file_id",
            "version",
            "operation_id",
            "author_principal",
            "timestamp_ms",
            "content_digest",
            "manifest_digest",
            "size"
        ],
        "additionalProperties": false
    })
}

fn drive_folder_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "folder_id": string_schema(),
            "profile_root": digest_string_schema(),
            "entries": array_schema(drive_entry_schema())
        },
        "required": ["workspace_id", "folder_id", "profile_root", "entries"],
        "additionalProperties": false
    })
}

fn drive_stat_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "node_id": string_schema(),
            "name": string_schema(),
            "kind": string_schema(),
            "profile_root": digest_string_schema(),
            "latest_version": nullable(drive_version_schema())
        },
        "required": ["workspace_id", "node_id", "name", "kind", "profile_root", "latest_version"],
        "additionalProperties": false
    })
}

fn drive_upload_session_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "upload_id": string_schema(),
            "target_kind": string_schema(),
            "parent_folder_id": string_schema(),
            "name": string_schema(),
            "file_id": string_schema(),
            "expected_root": digest_string_schema(),
            "chunk_count": integer_schema(),
            "total_size": integer_schema()
        },
        "required": [
            "workspace_id",
            "upload_id",
            "target_kind",
            "parent_folder_id",
            "name",
            "file_id",
            "expected_root",
            "chunk_count",
            "total_size"
        ],
        "additionalProperties": false
    })
}

fn drive_write_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "operation_id": string_schema(),
            "operation_kind": string_schema(),
            "sequence": integer_schema(),
            "profile_root": digest_string_schema(),
            "target_entity_id": nullable(string_schema()),
            "conflict_id": nullable(string_schema())
        },
        "required": [
            "workspace_id",
            "operation_id",
            "operation_kind",
            "sequence",
            "profile_root",
            "target_entity_id",
            "conflict_id"
        ],
        "additionalProperties": false
    })
}

fn drive_lease_token_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "key": string_schema(),
            "principal": string_schema(),
            "session": string_schema(),
            "mode": string_schema(),
            "fence": fence_schema(),
            "lease_deadline_ms": integer_schema()
        },
        "required": [
            "key",
            "principal",
            "session",
            "mode",
            "fence",
            "lease_deadline_ms"
        ],
        "additionalProperties": false
    })
}

fn fence_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "authority": integer_schema(),
            "epoch": integer_schema(),
            "sequence": integer_schema()
        },
        "required": ["authority", "epoch", "sequence"],
        "additionalProperties": false
    })
}

fn drive_lease_break_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "key": string_schema(),
            "broken_holders": integer_schema()
        },
        "required": ["key", "broken_holders"],
        "additionalProperties": false
    })
}

fn drive_conflict_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "conflict_id": string_schema(),
            "folder_id": string_schema(),
            "visible_node_id": string_schema(),
            "conflict_node_id": string_schema(),
            "conflict_name": string_schema(),
            "base_root": digest_string_schema(),
            "resolution": string_schema()
        },
        "required": [
            "conflict_id",
            "folder_id",
            "visible_node_id",
            "conflict_node_id",
            "conflict_name",
            "base_root",
            "resolution"
        ],
        "additionalProperties": false
    })
}

fn drive_share_grant_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "grant_id": string_schema(),
            "target_kind": string_schema(),
            "target_id": string_schema(),
            "principal": string_schema(),
            "role": string_schema(),
            "granted_by": string_schema(),
            "granted_at_ms": integer_schema(),
            "expires_at_ms": nullable(integer_schema())
        },
        "required": [
            "grant_id",
            "target_kind",
            "target_id",
            "principal",
            "role",
            "granted_by",
            "granted_at_ms",
            "expires_at_ms"
        ],
        "additionalProperties": false
    })
}

fn drive_retention_pin_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "pin_id": string_schema(),
            "kind": string_schema(),
            "root": digest_string_schema(),
            "target_entity_id": nullable(string_schema()),
            "added_by": string_schema(),
            "added_at_ms": integer_schema(),
            "expires_at_ms": nullable(integer_schema())
        },
        "required": [
            "pin_id",
            "kind",
            "root",
            "target_entity_id",
            "added_by",
            "added_at_ms",
            "expires_at_ms"
        ],
        "additionalProperties": false
    })
}

fn drive_retention_apply_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "now_ms": integer_schema(),
            "expired_pin_ids": array_schema(string_schema()),
            "remaining_pins": integer_schema(),
            "operation": nullable(drive_write_schema())
        },
        "required": [
            "workspace_id",
            "now_ms",
            "expired_pin_ids",
            "remaining_pins",
            "operation"
        ],
        "additionalProperties": false
    })
}

fn drive_share_expiry_apply_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "now_ms": integer_schema(),
            "expired_grant_ids": array_schema(string_schema()),
            "remaining_grants": integer_schema(),
            "operation": nullable(drive_write_schema())
        },
        "required": [
            "workspace_id",
            "now_ms",
            "expired_grant_ids",
            "remaining_grants",
            "operation"
        ],
        "additionalProperties": false
    })
}

fn page_update_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "page_id": string_schema(),
            "status": string_schema(),
            "base_revision": nullable(integer_schema()),
            "updated_at_ms": integer_schema(),
            "profile_root": digest_string_schema()
        },
        "required": ["workspace_id", "page_id", "status", "base_revision", "updated_at_ms", "profile_root"],
        "additionalProperties": false
    })
}

fn page_publish_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "page_id": string_schema(),
            "outcome": string_schema(),
            "revision": nullable(integer_schema()),
            "conflict_id": nullable(string_schema()),
            "current_revision": nullable(integer_schema()),
            "body_digest": nullable(digest_string_schema()),
            "profile_root": digest_string_schema()
        },
        "required": ["workspace_id", "page_id", "outcome", "revision", "conflict_id", "current_revision", "body_digest", "profile_root"],
        "additionalProperties": false
    })
}

fn page_history_schema() -> Value {
    json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "kind": string_schema(),
                "page_id": string_schema(),
                "revision": nullable(integer_schema()),
                "body_digest": nullable(digest_string_schema()),
                "author": nullable(string_schema()),
                "published_at_ms": nullable(integer_schema()),
                "conflict_id": nullable(string_schema()),
                "base_revision": nullable(integer_schema()),
                "current_revision": nullable(integer_schema()),
                "candidate_digest": nullable(digest_string_schema())
            },
            "required": ["kind", "page_id", "revision", "body_digest", "author", "published_at_ms", "conflict_id", "base_revision", "current_revision", "candidate_digest"],
            "additionalProperties": false
        }
    })
}

fn structure_node_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "structure_id": string_schema(),
            "node_id": string_schema(),
            "kind": string_schema(),
            "label": string_schema(),
            "body_digest": nullable(digest_string_schema()),
            "entity_ref": nullable(string_schema())
        },
        "required": ["workspace_id", "structure_id", "node_id", "kind", "label", "body_digest", "entity_ref"],
        "additionalProperties": false
    })
}

fn structure_edge_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "structure_id": string_schema(),
            "edge_id": string_schema(),
            "src_node_id": string_schema(),
            "dst_node_id": string_schema(),
            "label": string_schema(),
            "target_ref": nullable(string_schema())
        },
        "required": ["workspace_id", "structure_id", "edge_id", "src_node_id", "dst_node_id", "label", "target_ref"],
        "additionalProperties": false
    })
}

fn structure_move_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "structure_id": string_schema(),
            "node_id": string_schema(),
            "parent_node_id": nullable(string_schema()),
            "label": string_schema(),
            "edge": nullable(structure_edge_schema()),
            "graph_collection": string_schema()
        },
        "required": ["workspace_id", "structure_id", "node_id", "parent_node_id", "label", "edge", "graph_collection"],
        "additionalProperties": false
    })
}

fn structure_decompose_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "structure_id": string_schema(),
            "tickets": { "type": "array", "items": ticket_schema() },
            "implemented_by_edges": { "type": "array", "items": string_schema() },
            "graph_collection": string_schema()
        },
        "required": ["workspace_id", "structure_id", "tickets", "implemented_by_edges", "graph_collection"],
        "additionalProperties": false
    })
}

fn structure_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workspace_id": string_schema(),
            "structure_id": string_schema(),
            "space_id": string_schema(),
            "kind": string_schema(),
            "title": string_schema(),
            "root_node_id": nullable(string_schema()),
            "field_ids": array_schema(string_schema()),
            "profile_root": digest_string_schema()
        },
        "required": [
            "workspace_id",
            "structure_id",
            "space_id",
            "kind",
            "title",
            "root_node_id",
            "field_ids",
            "profile_root"
        ],
        "additionalProperties": false
    })
}

fn structure_render_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "structure": structure_schema(),
            "nodes": array_schema(structure_node_schema()),
            "edges": array_schema(structure_edge_schema()),
            "graph_collection": string_schema()
        },
        "required": ["structure", "nodes", "edges", "graph_collection"],
        "additionalProperties": false
    })
}

fn document_query_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "items": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": string_schema(),
                        "len": integer_schema(),
                        "digest": digest_string_schema(),
                        "document": nullable(array_schema(integer_schema())),
                        "projections": {
                            "type": "object",
                            "additionalProperties": true
                        }
                    },
                    "required": ["id", "len", "digest", "document", "projections"],
                    "additionalProperties": false
                }
            },
            "next_cursor": nullable(string_schema())
        },
        "required": ["items", "next_cursor"],
        "additionalProperties": false
    })
}

fn document_replace_text_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "replacements": integer_schema(),
            "digest": digest_string_schema(),
            "entity_tag": string_schema()
        },
        "required": ["replacements", "digest", "entity_tag"],
        "additionalProperties": false
    })
}

fn search_degraded_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "is_degraded": bool_schema(),
            "reason": string_schema()
        },
        "required": ["is_degraded", "reason"],
        "additionalProperties": false
    })
}

fn search_schema() -> Value {
    let degraded_schema = search_degraded_schema();
    let string_array_schema = json!({ "type": "array", "items": string_schema() });
    json!({
        "type": "object",
        "properties": {
            "hits": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "facet": string_schema(),
                        "workspace": string_schema(),
                        "collection": string_schema(),
                        "entity_id": string_schema(),
                        "field": string_schema(),
                        "snippet": string_schema(),
                        "offsets": {
                            "type": "array",
                            "items": {
                                "type": "array",
                                "items": integer_schema(),
                                "minItems": 2,
                                "maxItems": 2
                            }
                        },
                        "scope_context": {
                            "type": "object",
                            "properties": {
                                "owning_entity": nullable(string_schema()),
                                "status_fields": string_array_schema,
                                "refs": string_array_schema
                            },
                            "required": ["owning_entity", "status_fields", "refs"],
                            "additionalProperties": false
                        },
                        "root": nullable(digest_string_schema()),
                        "match_via": string_schema(),
                        "contributing_rungs": string_array_schema,
                        "fused_score": { "type": "number" },
                        "raw_score": { "type": "number" },
                        "rung": string_schema()
                    },
                    "required": ["facet", "workspace", "collection", "entity_id", "field", "snippet", "offsets", "scope_context", "root", "match_via", "contributing_rungs", "fused_score", "raw_score", "rung"],
                    "additionalProperties": false
                }
            },
            "engine": {
                "type": "object",
                "properties": {
                    "rungs_available": string_array_schema,
                    "rung_selected_ceiling": string_schema(),
                    "rrf_k": integer_schema(),
                    "rung_depth": integer_schema()
                },
                "required": ["rungs_available", "rung_selected_ceiling", "rrf_k", "rung_depth"],
                "additionalProperties": false
            },
            "index_status": {
                "type": "object",
                "properties": {
                    "lexical": string_schema(),
                    "semantic": string_schema(),
                    "graph": string_schema()
                },
                "required": ["lexical", "semantic", "graph"],
                "additionalProperties": false
            },
            "reduced": bool_schema(),
            "degraded": degraded_schema
        },
        "required": ["hits", "engine", "index_status", "reduced", "degraded"],
        "additionalProperties": false
    })
}

fn array_schema(item: Value) -> Value {
    json!({ "type": "array", "items": item })
}

fn nullable(schema: Value) -> Value {
    json!({ "anyOf": [schema, null_schema()] })
}

fn output_value_schema(name: &str) -> Option<Value> {
    let schema = match name {
        "store_version"
        | "metrics_put_descriptor"
        | "metrics_put_observation"
        | "logs_put_record"
        | "traces_put_span"
        | "store_blob_digest"
        | "cas_put"
        | "fs_read_link"
        | "vcs_commit"
        | "vcs_head_branch"
        | "vcs_merge_continue"
        | "vcs_commit_staged"
        | "vcs_tag_create"
        | "vcs_squash"
        | "sql_commit"
        | "calendar_put_entry"
        | "calendar_put_ics"
        | "contacts_put_entry"
        | "contacts_put_vcard"
        | "mail_ingest_message"
        | "dataframe_plan_digest"
        | "fts_source_digest"
        | "columnar_source_digest" => string_schema(),
        "cas_has"
        | "cas_delete"
        | "graph_remove_edge"
        | "vector_create_metadata_index"
        | "vector_drop_metadata_index"
        | "vector_delete"
        | "kv_delete"
        | "document_delete"
        | "fts_delete"
        | "vcs_merge_in_progress"
        | "calendar_delete_collection"
        | "calendar_delete_entry"
        | "contacts_delete_book"
        | "contacts_delete_entry"
        | "mail_delete_mailbox"
        | "mail_delete_message" => bool_schema(),
        "ledger_append"
        | "ledger_len"
        | "columnar_rows"
        | "queue_append"
        | "queue_len"
        | "queue_consumer_position" => integer_schema(),
        "store_capabilities"
        | "store_capabilities_json"
        | "store_maintenance_status"
        | "store_maintenance_policy_set"
        | "store_maintenance_run"
        | "metrics_query"
        | "logs_query"
        | "traces_trace_spans"
        | "traces_query"
        | "graph_query"
        | "graph_explain_query"
        | "graph_neighbors"
        | "graph_out_edges"
        | "graph_in_edges"
        | "graph_reachable"
        | "vector_ids"
        | "vector_metadata_index_keys"
        | "vector_search"
        | "vector_search_policy"
        | "columnar_scan"
        | "columnar_columns"
        | "columnar_inspect"
        | "columnar_select"
        | "columnar_aggregate"
        | "dataframe_collect"
        | "dataframe_preview"
        | "dataframe_source_digests"
        | "fts_ids"
        | "fts_query"
        | "fts_status"
        | "kv_list"
        | "kv_range"
        | "document_list_binary"
        | "timeseries_range"
        | "vcs_diff"
        | "sql_exec"
        | "sql_query"
        | "sql_read_table"
        | "sql_read_table_at"
        | "sql_index_scan"
        | "sql_index_scan_at"
        | "sql_diff"
        | "sql_table_diff"
        | "sql_blame"
        | "drive_read" => bytes_schema(),
        "fs_stat" | "fs_list_directory" => bytes_schema(),
        "cas_get"
        | "kv_get"
        | "timeseries_get"
        | "ledger_get"
        | "queue_get"
        | "graph_get_node"
        | "graph_get_edge"
        | "graph_shortest_path"
        | "vector_get"
        | "vector_source_text"
        | "vector_embedding_model"
        | "fts_get"
        | "fs_read_file"
        | "fs_read_at"
        | "calendar_get_entry"
        | "contacts_get_entry"
        | "mail_to_eml"
        | "apps_read_file" => nullable(bytes_schema()),
        "metrics_get_descriptor" | "logs_get_record" | "traces_get_span" => {
            nullable(bytes_schema())
        }
        "document_get_text" => nullable(json!({
            "type": "object",
            "properties": {
                "text": string_schema(),
                "digest": digest_string_schema()
            },
            "required": ["text", "digest"]
        })),
        "document_get_binary" => nullable(json!({
            "type": "object",
            "properties": {
                "bytes": bytes_schema(),
                "digest": digest_string_schema()
            },
            "required": ["bytes", "digest"]
        })),
        "cas_list"
        | "vcs_log"
        | "vcs_merge_conflicts"
        | "vcs_tag_list"
        | "kv_list_collections"
        | "document_list_collections"
        | "timeseries_list_collections"
        | "ledger_list_collections"
        | "queue_list_streams"
        | "sql_list_databases"
        | "calendar_list_collections"
        | "contacts_list_books"
        | "mail_list_mailboxes"
        | "mail_get_flags" => array_schema(string_schema()),
        "workspace_list" | "apps_list" => array_schema(object_schema()),
        "apps_call_tool" => json!({
            "type": "object",
            "properties": {
                "tool": string_schema(),
                "result": object_schema()
            },
            "required": ["tool", "result"]
        }),
        "document_put_text" | "document_put_binary" => json!({
            "type": "object",
            "properties": { "digest": digest_string_schema() },
            "required": ["digest"]
        }),
        "document_query" => document_query_schema(),
        "document_replace_text" => document_replace_text_schema(),
        "substrate_changes" => substrate_changes_schema(),
        "workgraph_changes" => substrate_changes_schema(),
        "workgraph_metrics" => object_schema(),
        "substrate_refs" => substrate_refs_schema(),
        "substrate_alias_bind" => substrate_alias_schema(),
        "substrate_alias_release" => bool_schema(),
        "substrate_alias_resolve" => nullable(substrate_alias_schema()),
        "substrate_alias_list" => array_schema(substrate_alias_schema()),
        "substrate_reference_status" | "substrate_reference_reconcile" => {
            reference_reconciliation_schema()
        }
        "substrate_history" => substrate_history_schema(),
        "substrate_revision_latest" | "substrate_revision_at" | "substrate_revision_as_of_root" => {
            substrate_revision_lookup_schema()
        }
        "substrate_checkpoint_before" => substrate_checkpoint_lookup_schema(),
        "substrate_transact" => substrate_transact_schema(),
        "substrate_view_define" => view_definition_schema(),
        "substrate_view_get" => nullable(view_definition_schema()),
        "substrate_view_list" => array_schema(view_definition_schema()),
        "substrate_write_admission_policy_get" => nullable(write_admission_policy_schema()),
        "substrate_write_admission_policy_set" => write_admission_policy_schema(),
        "tickets_project_create" => ticket_project_schema(),
        "tickets_project_rekey" | "tickets_project_settings_set" => ticket_project_schema(),
        "tickets_project_settings_get" => nullable(ticket_project_schema()),
        "tickets_projects" => ticket_projects_schema(),
        "tickets_relations" => ticket_relations_schema(),
        "tickets_fields" => ticket_field_catalog_schema(),
        "tickets_field_put" | "tickets_field_retire" => ticket_field_catalog_schema(),
        "tickets_create" => mutation_envelope_schema(ticket_schema()),
        "tickets_delete" => mutation_envelope_schema(ticket_schema()),
        "tickets_update" => mutation_envelope_schema(ticket_schema()),
        "tickets_comments" => array_schema(ticket_comment_schema()),
        "tickets_comment_add" | "tickets_comment_update" | "tickets_comment_delete" => {
            mutation_envelope_schema(ticket_schema())
        }
        "tickets_board_create"
        | "tickets_board_update"
        | "tickets_board_delete"
        | "tickets_board_configure_columns"
        | "tickets_board_move_card" => ticket_board_schema(),
        "tickets_board_get" => nullable(ticket_board_schema()),
        "tickets_board_list" => array_schema(ticket_board_schema()),
        "tickets_relation_set" => mutation_envelope_schema(ticket_relation_schema()),
        "tickets_relation_remove" => mutation_envelope_schema(ticket_relation_schema()),
        "tickets_get" => nullable(ticket_schema()),
        "tickets_list" => json!({
            "type": "object",
            "properties": {
                "items": array_schema(ticket_schema()),
                "total": integer_schema(),
                "next_cursor": nullable(string_schema())
            },
            "required": ["items", "total"]
        }),
        "tickets_history" => ticket_history_schema(),
        "lanes_create"
        | "lanes_update"
        | "lanes_ticket_add"
        | "lanes_ticket_remove"
        | "lanes_ticket_transfer"
        | "lanes_delete" => mutation_envelope_schema(lane_schema()),
        "lanes_get" => nullable(lane_view_result_schema()),
        "lanes_list" => lanes_list_result_schema(),
        "spaces_create" => space_schema(),
        "spaces_get" => nullable(space_schema()),
        "spaces_list" => array_schema(space_schema()),
        "pages_create" => page_schema(),
        "pages_get" => nullable(page_schema()),
        "pages_list" => array_schema(page_schema()),
        "pages_update" => page_update_schema(),
        "pages_publish" => page_publish_schema(),
        "pages_history" => page_history_schema(),
        "lifecycles_define" | "lifecycles_define_standard" => lifecycle_definition_schema(),
        "lifecycles_definitions" => array_schema(lifecycle_definition_schema()),
        "lifecycles_definition" => nullable(lifecycle_definition_schema()),
        "lifecycles_instantiate" => lifecycle_instance_schema(),
        "lifecycles_instances" => array_schema(lifecycle_instance_schema()),
        "lifecycles_instance" => nullable(lifecycle_instance_schema()),
        "lifecycles_active_set" | "lifecycles_active_clear" => lifecycle_active_schema(),
        "lifecycles_snapshot_plan" => lifecycle_snapshot_plan_schema(),
        "lifecycles_current_surface" => lifecycle_surface_schema(),
        "lifecycles_transition" => lifecycle_transition_result_schema(),
        "lifecycles_snapshots" => array_schema(lifecycle_snapshot_schema()),
        "lifecycles_snapshot" => nullable(lifecycle_snapshot_schema()),
        "lifecycles_snapshot_content" => nullable(bytes_schema()),
        "lifecycles_operation_log" => lifecycle_operation_log_schema(),
        "chat_channels" => array_schema(chat_channel_directory_schema()),
        "chat_emoji_list" | "chat_emoji_register" | "chat_emoji_unregister" => {
            chat_emoji_registry_schema()
        }
        "chat_fetch_events" => chat_events_schema(),
        "chat_messages" => chat_channel_schema(),
        "chat_cursor" => chat_cursor_schema(),
        "chat_presence" => chat_presence_schema(),
        "chat_create_channel" | "chat_rename_channel" => chat_channel_directory_schema(),
        "chat_post_message"
        | "chat_edit_message"
        | "chat_redact_message"
        | "chat_add_reaction"
        | "chat_remove_reaction"
        | "chat_create_thread"
        | "chat_create_task"
        | "chat_claim_task"
        | "chat_complete_task"
        | "chat_invoke_agent"
        | "chat_agent_reply"
        | "chat_request_handoff" => chat_write_schema(),
        "chat_update_cursor" => chat_cursor_schema(),
        "chat_set_presence" => chat_presence_entry_schema(),
        "drive_list" => drive_folder_schema(),
        "drive_stat" => drive_stat_schema(),
        "drive_list_versions" => array_schema(drive_version_schema()),
        "drive_list_conflicts" => array_schema(drive_conflict_schema()),
        "drive_list_shares" => array_schema(drive_share_grant_schema()),
        "drive_list_retention" => array_schema(drive_retention_pin_schema()),
        "drive_apply_retention" => drive_retention_apply_schema(),
        "drive_apply_share_expiry" => drive_share_expiry_apply_schema(),
        "meetings_projection_outputs" => meetings_projection_schema(),
        "meetings_list" => meetings_list_schema(),
        "meetings_get" => meeting_detail_schema(),
        "meetings_search" => search_schema(),
        "meetings_extraction_review" => meetings_review_schema(),
        "meetings_accept_annotation" | "meetings_reject_annotation" => {
            meetings_annotation_review_write_schema()
        }
        "meetings_propose_vocabulary"
        | "meetings_accept_vocabulary"
        | "meetings_reject_vocabulary" => meetings_vocabulary_review_write_schema(),
        "meetings_add_entity_merge" => meetings_entity_merge_write_schema(),
        "meetings_add_promotion" => meetings_promotion_write_schema(),
        "meetings_promote_task_to_ticket" => meetings_ticket_promotion_write_schema(),
        "meetings_promote_decision_to_decision_log" => meetings_decision_promotion_write_schema(),
        "meetings_promote_question_to_lifecycle" => meetings_lifecycle_promotion_write_schema(),
        "meetings_promote_artifact_to_reference_artifact"
        | "meetings_promote_reference_to_reference_artifact" => {
            meetings_reference_artifact_promotion_write_schema()
        }
        "meetings_import_snapshot" => meetings_import_snapshot_schema(),
        "redmine_import_snapshot" => redmine_import_snapshot_schema(),
        "studio_reindex" => studio_reindex_schema(),
        "import_submit_batch" => import_batch_submit_schema(),
        "import_execute_batch" => import_batch_execute_schema(),
        "drive_create_folder"
        | "drive_commit_upload"
        | "drive_rename"
        | "drive_move"
        | "drive_delete"
        | "drive_resolve_conflict"
        | "drive_grant_share"
        | "drive_revoke_share"
        | "drive_pin_retention"
        | "drive_unpin_retention" => drive_write_schema(),
        "drive_acquire_lease" | "drive_refresh_lease" => drive_lease_token_schema(),
        "drive_release_lease" => bool_schema(),
        "drive_break_lease" => drive_lease_break_schema(),
        "drive_create_upload" | "drive_upload_chunk" => drive_upload_session_schema(),
        "structures_create" => structure_render_schema(),
        "structures_get" => nullable(structure_render_schema()),
        "structures_list" => array_schema(structure_schema()),
        "structures_add_node" => structure_node_schema(),
        "structures_update_node" => structure_node_schema(),
        "structures_move_node" => structure_move_schema(),
        "structures_link_node" => structure_edge_schema(),
        "structures_bind" => structure_node_schema(),
        "structures_decompose_to_tickets" => structure_decompose_schema(),
        "search" => search_schema(),
        "watch_subscribe" => watch_subscribe_schema(),
        "watch_poll" => watch_poll_schema(),
        "queue_range"
        | "queue_consumer_read"
        | "calendar_list_entries"
        | "calendar_search"
        | "contacts_list_entries"
        | "contacts_search" => array_schema(bytes_schema()),
        "vcs_blame" => array_schema(array_schema(string_schema())),
        "calendar_range" | "mail_list_messages" | "mail_search" => array_schema(object_schema()),
        "apps_show"
        | "timeseries_latest"
        | "calendar_get_collection"
        | "contacts_get_book"
        | "mail_get_mailbox"
        | "mail_get_message" => nullable(object_schema()),
        "vcs_merge" | "vcs_cherry_pick" | "vcs_revert" | "vcs_rebase" | "vcs_status"
        | "ask_questions" | "ask_answers" => object_schema(),
        "dataframe_materialize"
        | "ledger_head"
        | "vcs_tag_target"
        | "calendar_to_ics"
        | "contacts_to_vcard" => nullable(string_schema()),
        "kv_put"
        | "workgraph_fact_put"
        | "timeseries_put"
        | "graph_upsert_node"
        | "graph_remove_node"
        | "graph_upsert_edge"
        | "vector_create"
        | "vector_upsert"
        | "vector_upsert_source"
        | "columnar_create"
        | "columnar_append"
        | "columnar_compact"
        | "dataframe_create"
        | "fts_create"
        | "fts_index"
        | "fts_remap"
        | "ledger_verify"
        | "queue_consumer_advance"
        | "queue_consumer_reset"
        | "apps_create"
        | "apps_write_file"
        | "apps_remove_file"
        | "fs_write_file"
        | "fs_append_file"
        | "fs_remove_file"
        | "fs_create_directory"
        | "fs_remove_directory"
        | "fs_write_at"
        | "fs_truncate"
        | "fs_symlink"
        | "vcs_branch"
        | "vcs_checkout"
        | "vcs_merge_resolve"
        | "vcs_merge_abort"
        | "vcs_stage"
        | "vcs_stage_all"
        | "vcs_unstage"
        | "vcs_tag_delete"
        | "vcs_tag_rename"
        | "vcs_restore_file"
        | "vcs_restore_path"
        | "calendar_create_collection"
        | "contacts_create_book"
        | "mail_create_mailbox"
        | "mail_set_flags"
        | "ask_record" => null_schema(),
        _ => return None,
    };
    Some(schema)
}

fn tool_output_schema(name: &str) -> Option<Arc<JsonObject>> {
    let mut props = JsonObject::new();
    props.insert("value".to_string(), output_value_schema(name)?);

    let mut schema = JsonObject::new();
    schema.insert("type".to_string(), json!("object"));
    schema.insert("properties".to_string(), Value::Object(props));
    schema.insert("required".to_string(), json!(["value"]));
    schema.insert("additionalProperties".to_string(), json!(false));
    Some(Arc::new(schema))
}

fn json_object_schema(value: Value) -> Arc<JsonObject> {
    match value {
        Value::Object(map) => Arc::new(map),
        _ => Arc::new(JsonObject::new()),
    }
}

fn empty_input_schema() -> Arc<JsonObject> {
    json_object_schema(json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    }))
}

fn app_open_input_schema(workspace_bound: bool) -> Arc<JsonObject> {
    let mut properties = serde_json::Map::new();
    let mut required = vec!["app"];
    if !workspace_bound {
        properties.insert("workspace".to_string(), string_schema());
        required.insert(0, "workspace");
    }
    properties.insert("app".to_string(), string_schema());
    json_object_schema(json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    }))
}

fn app_launcher_output_schema() -> Arc<JsonObject> {
    json_object_schema(json!({
        "type": "object",
        "properties": {
            "value": {
                "type": "object",
                "properties": {
                    "workspace": { "type": "string" },
                    "app": { "type": "string" },
                    "uri": { "type": "string" },
                    "name": { "type": "string" },
                    "description": { "anyOf": [{ "type": "string" }, { "type": "null" }] },
                    "processing": { "type": "string" }
                },
                "required": ["workspace", "app", "uri", "name", "processing"],
                "additionalProperties": false
            }
        },
        "required": ["value"],
        "additionalProperties": false
    }))
}

fn change_kind(k: &ChangeKind) -> &'static str {
    match k {
        ChangeKind::Added => "added",
        ChangeKind::Modified => "modified",
        ChangeKind::Deleted => "deleted",
    }
}

fn change_val(c: &Change) -> Value {
    json!({ "path": c.path, "kind": change_kind(&c.kind) })
}

fn status_val(s: &Status) -> Value {
    json!({
        "staged": s.staged.iter().map(change_val).collect::<Vec<_>>(),
        "unstaged": s.unstaged.iter().map(change_val).collect::<Vec<_>>(),
        "untracked": s.untracked,
        "conflicts": s.conflicts,
    })
}

fn merge_val(o: &MergeOutcome) -> Value {
    match o {
        MergeOutcome::UpToDate => json!({ "outcome": "up_to_date" }),
        MergeOutcome::FastForward(d) => {
            json!({ "outcome": "fast_forward", "commit": d.to_string() })
        }
        MergeOutcome::Merged(d) => json!({ "outcome": "merged", "commit": d.to_string() }),
        MergeOutcome::Conflicts(p) => json!({ "outcome": "conflicts", "paths": p }),
    }
}

fn replay_val(o: &ReplayOutcome) -> Value {
    match o {
        ReplayOutcome::Replayed(d) => json!({ "outcome": "replayed", "commit": d.to_string() }),
        ReplayOutcome::Clean => json!({ "outcome": "clean" }),
        ReplayOutcome::Conflicts(p) => json!({ "outcome": "conflicts", "paths": p }),
        ReplayOutcome::Empty => json!({ "outcome": "empty" }),
    }
}

fn mail_msg_val(m: &MailMessage) -> Value {
    json!({
        "uid": m.uid, "body": m.body, "from": m.from, "to": m.to, "subject": m.subject,
        "date": m.date, "message_id": m.message_id, "size": m.size, "headers": m.headers,
    })
}

fn occ_val(o: &Occurrence) -> Value {
    json!({ "uid": o.uid, "start": o.start.to_string() })
}

/// Display name for an area (acronyms upper-cased, words title-cased).
fn category_display(area: &str) -> &'static str {
    match area {
        "store" => "Store",
        "workspace" => "Workspace",
        "vcs" => "VCS",
        "fs" => "FS",
        "apps" => "Apps",
        "ask" => "Ask",
        "cas" => "CAS",
        "kv" => "KV",
        "document" => "Document",
        "timeseries" => "TimeSeries",
        "ledger" => "Ledger",
        "queue" => "Queue",
        "calendar" => "Calendar",
        "contacts" => "Contacts",
        "mail" => "Mail",
        "sql" => "SQL",
        "vector" => "Vector",
        "fts" => "FTS",
        "search" => "Search",
        "tools" => "Tools",
        "substrate" => "Substrate",
        "tickets" => "Tickets",
        "spaces" => "Spaces",
        "pages" => "Pages",
        "chat" => "Chat",
        "structures" => "Structures",
        _ => "Loom",
    }
}

/// A short descriptive title. Curated titles live in [`crate::tool_titles`]; the mechanical
/// `<Area>: <verb>` derivation remains only as a fallback for dynamic tools (e.g. app launchers)
/// that are not in the static surface. The fallback accepts either the dotted or underscored
/// name form (area is always a single underscore-free token).
fn tool_title(name: &str, area: &str) -> String {
    if let Some(title) = crate::tool_titles::tool_title(name) {
        return title;
    }
    let verb = name
        .split_once('.')
        .or_else(|| name.split_once('_'))
        .map(|(_, v)| v)
        .unwrap_or(name);
    format!(
        "{}: {}",
        category_display(area),
        capitalized_phrase(&verb.replace('_', " "))
    )
}

fn capitalized_phrase(phrase: &str) -> String {
    let mut chars = phrase.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    format!("{}{}", first.to_uppercase(), chars.as_str())
}

/// A write verb that deletes, overwrites, or rewrites history.
fn is_destructive(verb: &str) -> bool {
    matches!(
        verb,
        "delete"
            | "delete_collection"
            | "delete_entry"
            | "delete_book"
            | "delete_mailbox"
            | "delete_message"
            | "remove_file"
            | "truncate"
            | "write_file"
            | "write_at"
            | "restore_file"
            | "restore_path"
            | "merge_abort"
            | "rebase"
            | "revert"
            | "squash"
            | "consumer_reset"
    )
}

/// A write verb where re-running with the same arguments converges to the same state with no
/// additional effect and no error. Excludes creates (error or differ on re-run), renames (the source
/// is gone on the second call), checkout, and appends/commits (each call adds a new entry/commit).
fn is_idempotent(verb: &str) -> bool {
    matches!(
        verb,
        "put"
            | "put_entry"
            | "set_flags"
            | "write_file"
            | "write_at"
            | "truncate"
            | "restore_file"
            | "restore_path"
            | "stage"
            | "stage_all"
            | "unstage"
            | "consumer_advance"
            | "consumer_reset"
    )
}

/// Regular tools invoked by an embedded app over the host bridge, not by the agent. Declared
/// `_meta.ui.visibility: ["app"]` so `tools/list` omits them for the model while `tools/call`
/// still dispatches them (the MCP Apps visibility surfaces, same rule as app-only launchers).
const APP_ONLY_TOOLS: &[&str] = &["ask_record", "apps_call_tool"];

fn substrate_read_resource_meta(name: &str) -> Option<Value> {
    match name {
        "substrate_refs" => Some(json!({
            "resourceTemplate": "loom://{workspace}/substrate/refs/{target}.json"
        })),
        "substrate_view_get" => Some(json!({
            "resourceTemplate": "loom://{workspace}/substrate/views/{view_id}.json"
        })),
        "search" => Some(json!({
            "resourceEquivalent": "tool-only",
            "reason": "query arguments do not identify one browsable resource"
        })),
        "substrate_changes" => Some(json!({
            "resourceEquivalent": "tool-only",
            "reason": "cursor arguments describe a stream position, not one resource"
        })),
        "workgraph_changes" => Some(json!({
            "resourceEquivalent": "tool-only",
            "reason": "workgraph operation cursors describe a stream position, not one resource"
        })),
        _ => None,
    }
}

/// Add the MCP presentation metadata derived from `tools::TOOL_SURFACE`.
fn enrich_metadata(router: &mut ToolRouter<LoomServer>) {
    for route in router.map.values_mut() {
        let name = route.attr.name.to_string();
        let spec = match crate::tools::tool(&name) {
            Some(s) => s,
            None => continue, // drift test fails separately if this ever happens
        };
        let verb = name
            .split_once('.')
            .or_else(|| name.split_once('_'))
            .map(|(_, v)| v)
            .unwrap_or(&name);
        let title = tool_title(&name, spec.area);
        let read_only = spec.kind == ToolKind::Read;
        let annotations = ToolAnnotations::from_raw(
            Some(title.clone()),
            Some(read_only),
            if read_only {
                None
            } else {
                Some(is_destructive(verb))
            },
            if read_only {
                None
            } else {
                Some(is_idempotent(verb))
            },
            Some(false),
        );
        let mut meta = serde_json::Map::new();
        meta.insert(
            "category".to_string(),
            Value::String(category_display(spec.area).to_string()),
        );
        meta.insert(
            "group".to_string(),
            Value::String(category_display(spec.area).to_string()),
        );
        if APP_ONLY_TOOLS.contains(&name.as_str()) {
            meta.insert("ui".to_string(), json!({ "visibility": ["app"] }));
        }
        if let Some(resource) = substrate_read_resource_meta(&name) {
            meta.insert("resource".to_string(), resource);
        }
        route.attr.title = Some(title);
        route.attr.output_schema = Some(
            tool_output_schema(&name).unwrap_or_else(|| panic!("{name} has no MCP output schema")),
        );
        route.attr.annotations = Some(annotations);
        route.attr.meta = Some(Meta(meta));
    }
}

mod params;
use params::*;
pub mod delivery;
use delivery::{DeliveryReplay, DeliveryRetention, DeliveryState};

/// The MCP server handler. Holds the engine facade behind an `Arc` (rmcp requires `Clone`) plus the
/// generated tool router.
#[derive(Clone)]
pub struct LoomServer {
    mcp: Arc<LoomMcp>,
    tool_router: ToolRouter<Self>,
    prompt_router: PromptRouter<Self>,
    binding: Binding,
    shutdown: ShutdownController,
    /// Subscribed resource URIs mapped to their last-seen content-address ETag.
    /// Shared across clones so the connection's poll loop and the request handlers see one registry.
    subscriptions: Arc<Mutex<HashMap<String, Option<String>>>>,
    /// App resource subscriptions mapped to their pull-watch cursors.
    app_watches: Arc<Mutex<HashMap<String, String>>>,
    /// App resource delivery streams and subscriber ack cursors.
    delivery: Arc<Mutex<DeliveryState>>,
    /// Last visible `resources/list` URI set for MCP-level list-changed notifications.
    resource_list: Arc<Mutex<Option<Vec<String>>>>,
    /// Last visible `tools/list` name set for MCP-level list-changed notifications.
    tool_list: Arc<Mutex<Option<Vec<String>>>>,
    /// Cheap durable-state token used to avoid rebuilding list inventories when the store is unchanged.
    list_change_token: Arc<Mutex<Option<String>>>,
    active_lifecycle: Arc<Mutex<Option<ActiveLifecycleContext>>>,
    chat_presence: Arc<Mutex<HashMap<PresenceKey, ChatPresenceSummary>>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ActiveLifecycleContext {
    workspace: String,
    workspace_id: String,
    instance_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct PresenceKey {
    workspace: WorkspaceId,
    workspace_id: String,
    channel_id: String,
    principal: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
struct ChatPresenceSummary {
    #[serde(rename = "workspace_id")]
    workspace_id: String,
    channel_id: String,
    principal: String,
    status: String,
    expires_at_ms: u64,
}

#[derive(Clone)]
pub struct Binding {
    pub workspace: Option<String>,
    pub collection: Option<String>,
    pub principal: String,
    pub allow_writes: bool,
}

impl Default for Binding {
    fn default() -> Self {
        Self {
            workspace: None,
            collection: None,
            principal: "owner".to_string(),
            allow_writes: true,
        }
    }
}

impl Binding {
    pub fn workspace(workspace: impl Into<String>) -> Self {
        Self {
            workspace: Some(workspace.into()),
            ..Self::default()
        }
    }

    pub fn collection(workspace: impl Into<String>, collection: impl Into<String>) -> Self {
        Self {
            workspace: Some(workspace.into()),
            collection: Some(collection.into()),
            ..Self::default()
        }
    }
}

fn omit_write_tools(router: &mut ToolRouter<LoomServer>, binding: &Binding) {
    if binding.allow_writes {
        return;
    }
    router
        .map
        .retain(|name, _| crate::tools::tool(name).is_some_and(|spec| spec.kind == ToolKind::Read));
}

fn tool_area_domain(area: &str) -> Option<AclDomain> {
    match area {
        "store" | "workspace" | "tools" => None,
        "apps" | "fs" | "drive" => Some(AclDomain::Files),
        "ask" => Some(AclDomain::Document),
        "chat" => Some(AclDomain::Chat),
        "spaces" | "pages" | "structures" => Some(AclDomain::Pages),
        "tickets" | "lanes" | "workgraph" | "redmine" => Some(AclDomain::Tickets),
        "lifecycles" => Some(AclDomain::Lifecycle),
        "meetings" => Some(AclDomain::Meetings),
        "fts" | "search" | "studio" => Some(AclDomain::Search),
        "timeseries" => Some(AclDomain::TimeSeries),
        other => AclDomain::parse(other).ok(),
    }
}

fn tool_right(kind: ToolKind) -> AclRight {
    match kind {
        ToolKind::Read => AclRight::Read,
        ToolKind::Write => AclRight::Write,
    }
}

fn workspace_selector(selector: &str) -> WsSelector {
    WorkspaceId::parse(selector)
        .map(WsSelector::Id)
        .unwrap_or_else(|_| WsSelector::Name(selector.to_string()))
}

fn hides_app_launcher_tools(e: &LoomError) -> bool {
    matches!(e.code, Code::AuthenticationFailed | Code::PermissionDenied)
}

fn collection_param(area: &str) -> Option<&'static str> {
    match area {
        "kv" | "document" | "timeseries" | "ledger" | "calendar" => Some("collection"),
        "sql" => Some("db"),
        "queue" => Some("stream"),
        "contacts" => Some("book"),
        "mail" => Some("mailbox"),
        _ => None,
    }
}

fn area_has_principal(area: &str) -> bool {
    matches!(area, "calendar" | "contacts" | "mail")
}

fn workspace_matches(summary: &crate::reads::WorkspaceSummary, selector: &str) -> bool {
    summary.name == selector || summary.id == selector
}

fn area_template_is_per_principal(template: &str) -> bool {
    template.contains("/calendar/{principal}/")
        || template.contains("/contacts/{principal}/")
        || template.contains("/mail/{principal}/")
        || template.contains("/studio/views/status/principal/{principal}")
}

fn lifecycle_surface_tool_name(name: &str) -> String {
    match name {
        "chat.message" => "chat_post_message".to_string(),
        other if crate::tools::tool(other).is_some() => other.to_string(),
        other => {
            let normalized = other.replace('.', "_");
            if crate::tools::tool(&normalized).is_some() {
                normalized
            } else {
                other.to_string()
            }
        }
    }
}

fn lifecycle_control_tool(name: &str) -> bool {
    name.starts_with("lifecycles_")
        || matches!(
            name,
            "store_version" | "store_capabilities" | "workspace_list"
        )
}

fn split_bound_resource<'a>(uri: &'a str, binding: &Binding) -> Option<(String, &'a str)> {
    let rest = uri.strip_prefix("loom://")?;
    if let Some(ns) = &binding.workspace {
        if rest.is_empty() {
            return Some((ns.clone(), ""));
        }
        if let Some(tail) = rest.strip_prefix(&format!("{ns}/")) {
            Some((ns.clone(), tail))
        } else {
            Some((ns.clone(), rest))
        }
    } else {
        let (ns, tail) = rest.split_once('/')?;
        if ns.is_empty() {
            return None;
        }
        Some((ns.to_string(), tail))
    }
}

fn split_principal_container(tail: &str, binding: &Binding, ext: &str) -> Option<(String, String)> {
    let mut rest = tail;
    let principal_prefix = format!("{}/", binding.principal);
    if let Some(after) = rest.strip_prefix(&principal_prefix) {
        rest = after;
    }
    let (container, file) = if let Some(bound) = &binding.collection {
        if let Some((first, after)) = rest.split_once('/') {
            if first != bound {
                return None;
            }
            (bound.clone(), after)
        } else {
            (bound.clone(), rest)
        }
    } else {
        let (container, file) = rest.split_once('/')?;
        (container.to_string(), file)
    };
    if file.contains('/') {
        return None;
    }
    resources::strip_resource_ext(file, ext).map(|id| (container, id.to_string()))
}

fn parse_resource_uri_with_binding(uri: &str, binding: &Binding) -> Option<ResourceTarget> {
    if uri == "loom://capabilities.json" {
        return Some(ResourceTarget::Capabilities);
    }
    if let Some(rest) = uri.strip_prefix("ui://") {
        return parse_app_uri_with_binding(rest, binding);
    }
    let (workspace, tail) = split_bound_resource(uri, binding)?;
    if tail.is_empty() {
        return Some(ResourceTarget::Workspace { workspace });
    }
    let (kind, tail) = tail.split_once('/').unwrap_or((tail, ""));
    match kind {
        "files" => (!tail.is_empty()).then(|| ResourceTarget::File {
            workspace,
            path: tail.to_string(),
        }),
        "cas" => (!tail.is_empty()).then(|| ResourceTarget::Cas {
            workspace,
            digest: tail.to_string(),
        }),
        "calendar" => {
            let (collection, uid) = split_principal_container(tail, binding, ".ics")?;
            Some(ResourceTarget::CalendarIcs {
                workspace,
                principal: binding.principal.clone(),
                collection,
                uid,
            })
        }
        "contacts" => {
            let (book, uid) = split_principal_container(tail, binding, ".vcf")?;
            Some(ResourceTarget::ContactsVcf {
                workspace,
                principal: binding.principal.clone(),
                book,
                uid,
            })
        }
        "mail" => {
            let (mailbox, uid) = split_principal_container(tail, binding, ".eml")?;
            Some(ResourceTarget::MailEml {
                workspace,
                principal: binding.principal.clone(),
                mailbox,
                uid,
            })
        }
        "studio" => {
            if let Some(principal) = resources::parse_studio_status_tail(tail) {
                if principal == binding.principal {
                    Some(ResourceTarget::StudioStatus {
                        workspace,
                        principal: principal.to_string(),
                    })
                } else {
                    None
                }
            } else if tail == "views/status" {
                Some(ResourceTarget::StudioStatus {
                    workspace,
                    principal: binding.principal.clone(),
                })
            } else {
                None
            }
        }
        "substrate" => resources::parse_substrate_tail(workspace, tail),
        _ => None,
    }
}

fn parse_app_uri_with_binding(rest: &str, binding: &Binding) -> Option<ResourceTarget> {
    let (workspace, tail) = if let Some(ns) = &binding.workspace {
        if let Some(tail) = rest.strip_prefix("mcp/apps/") {
            (ns.clone(), tail)
        } else if let Some(tail) = rest.strip_prefix(&format!("{ns}/mcp/apps/")) {
            (ns.clone(), tail)
        } else {
            return None;
        }
    } else {
        let (workspace, tail) = rest.split_once("/mcp/apps/")?;
        if workspace.is_empty() || tail.is_empty() {
            return None;
        }
        (workspace.to_string(), tail)
    };
    if let Some((app, instance)) = apps::split_internal_app_instance(tail) {
        return Some(ResourceTarget::App(apps::AppTarget {
            workspace,
            app: app.to_string(),
            instance,
            internal: true,
        }));
    }
    if tail.contains('/') || apps::validate_app_name(tail).is_err() {
        return None;
    }
    Some(ResourceTarget::App(apps::AppTarget {
        workspace,
        app: tail.to_string(),
        instance: None,
        internal: false,
    }))
}

fn collection_discovery_tool(name: &str) -> bool {
    matches!(
        name,
        "kv_list_collections"
            | "document_list_collections"
            | "timeseries_list_collections"
            | "ledger_list_collections"
            | "queue_list_streams"
            | "sql_list_databases"
            | "calendar_list_collections"
            | "contacts_list_books"
            | "mail_list_mailboxes"
    )
}

/// Collection-discovery tools are outside a collection-scoped server because the collection is already
/// bound and injected server-side. The bound collection need not exist yet.
fn narrow_to_collection(router: &mut ToolRouter<LoomServer>, binding: &Binding) {
    if binding.collection.is_none() {
        return;
    }
    router
        .map
        .retain(|name, _| !collection_discovery_tool(name));
}

/// Workspace discovery is outside a workspace-scoped server because the workspace is already bound and
/// injected server-side.
fn narrow_to_workspace(router: &mut ToolRouter<LoomServer>, binding: &Binding) {
    if binding.workspace.is_none() {
        return;
    }
    router.map.retain(|name, _| *name != "workspace_list");
}

/// Elide the bound parameters from every tool's input schema: `principal` for the per-principal areas,
/// `workspace` when a workspace is bound, and the collection-axis parameter when a collection is bound.
/// The host injects these server-side in `call_tool`.
fn apply_binding(router: &mut ToolRouter<LoomServer>, binding: &Binding) {
    for route in router.map.values_mut() {
        let name = route.attr.name.to_string();
        let Some(spec) = crate::tools::tool(&name) else {
            continue;
        };
        let mut drop: Vec<&str> = Vec::new();
        if area_has_principal(spec.area) {
            drop.push("principal");
        }
        if binding.workspace.is_some() {
            drop.push("workspace");
        }
        if binding.collection.is_some()
            && let Some(p) = collection_param(spec.area)
        {
            drop.push(p);
        }
        if drop.is_empty() {
            continue;
        }
        let schema = Arc::make_mut(&mut route.attr.input_schema);
        if let Some(Value::Object(props)) = schema.get_mut("properties") {
            for d in &drop {
                props.remove(*d);
            }
        }
        if let Some(Value::Array(req)) = schema.get_mut("required") {
            req.retain(|v| v.as_str().is_none_or(|s| !drop.contains(&s)));
        }
    }
}

/// The templated messages for a curated prompt, derived from `PROMPT_SURFACE`.
fn prompt_messages(name: &str) -> Vec<PromptMessage> {
    let summary = crate::prompts::prompt(name)
        .map(|p| p.summary)
        .unwrap_or("");
    let area = name
        .split_once('.')
        .or_else(|| name.split_once('_'))
        .map(|(a, _)| a)
        .unwrap_or("");
    let text = format!(
        "Workflow: {summary}\n\nYou are operating the `{area}` area of an Uldren Loom over MCP. Use the \
         `{area}.*` tools (see tools/list) to accomplish this, reading before writing and confirming \
         any destructive action with the user first."
    );
    vec![PromptMessage::new_text(PromptMessageRole::User, text)]
}

/// Enrich each prompt with its description (from `PROMPT_SURFACE`), a title, and `_meta.category`.
fn enrich_prompts(router: &mut PromptRouter<LoomServer>) {
    for route in router.map.values_mut() {
        let name = route.attr.name.to_string();
        if let Some(spec) = crate::prompts::prompt(&name) {
            let verb = name
                .split_once('.')
                .or_else(|| name.split_once('_'))
                .map(|(_, v)| v)
                .unwrap_or(&name);
            route.attr.title = Some(format!(
                "{}: {}",
                category_display(spec.area),
                verb.replace('_', " ")
            ));
            route.attr.description = Some(spec.summary.to_string());
            let mut meta = serde_json::Map::new();
            meta.insert(
                "category".to_string(),
                Value::String(category_display(spec.area).to_string()),
            );
            route.attr.meta = Some(Meta(meta));
        }
    }
}

#[tool_router]
impl LoomServer {
    /// Build a handler over `mcp` with the default (unscoped) binding: no workspace/collection scope,
    /// owner principal. Per-principal areas still elide `principal` (it is never an agent argument).
    pub fn new(mcp: Arc<LoomMcp>) -> Self {
        Self::with_binding(mcp, Binding::default())
    }

    /// Build a handler over `mcp` scoped by `binding`: the bound values are elided from the tool schemas
    /// and injected server-side at `call_tool`.
    pub fn with_binding(mcp: Arc<LoomMcp>, binding: Binding) -> Self {
        Self::with_binding_and_shutdown(mcp, binding, ShutdownController::new())
    }

    pub fn with_delivery_retention(mcp: Arc<LoomMcp>, retention: DeliveryRetention) -> Self {
        Self::with_binding_shutdown_delivery_retention(
            mcp,
            Binding::default(),
            ShutdownController::new(),
            retention,
        )
    }

    fn with_binding_and_shutdown(
        mcp: Arc<LoomMcp>,
        binding: Binding,
        shutdown: ShutdownController,
    ) -> Self {
        Self::with_binding_shutdown_delivery_retention(
            mcp,
            binding,
            shutdown,
            DeliveryRetention::default(),
        )
    }

    fn with_binding_shutdown_delivery_retention(
        mcp: Arc<LoomMcp>,
        binding: Binding,
        shutdown: ShutdownController,
        retention: DeliveryRetention,
    ) -> Self {
        let mut tool_router = Self::tool_router();
        enrich_metadata(&mut tool_router);
        omit_write_tools(&mut tool_router, &binding);
        apply_binding(&mut tool_router, &binding);
        narrow_to_workspace(&mut tool_router, &binding);
        narrow_to_collection(&mut tool_router, &binding);
        let mut prompt_router = Self::prompt_router();
        enrich_prompts(&mut prompt_router);
        Self {
            mcp,
            tool_router,
            prompt_router,
            binding,
            shutdown,
            subscriptions: Arc::new(Mutex::new(HashMap::new())),
            app_watches: Arc::new(Mutex::new(HashMap::new())),
            delivery: Arc::new(Mutex::new(DeliveryState::new(retention))),
            resource_list: Arc::new(Mutex::new(None)),
            tool_list: Arc::new(Mutex::new(None)),
            list_change_token: Arc::new(Mutex::new(None)),
            active_lifecycle: Arc::new(Mutex::new(None)),
            chat_presence: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn delivery_retention(&self) -> DeliveryRetention {
        self.delivery.lock().expect("delivery lock").policy()
    }

    /// Inject the bound scope into a tool call's arguments before dispatch, so the engine facade receives
    /// the full `principal`/`workspace`/collection it expects even though the agent never supplied them.
    /// Execute a server-promoted MCP tool on the hosted server. The local host forwards the
    /// whole tool operation and renders the same
    /// [`CallToolResult`] the local path would, so a promoted family is byte/shape-compatible with local
    /// without reconstructing behavior client-side. Errors carry the server's precise code/message.
    fn execute_tool_server_side(
        &self,
        name: &str,
        arguments: Option<serde_json::Map<String, Value>>,
    ) -> Result<CallToolResult, ErrorData> {
        let backend = self.mcp.store().remote_backend().ok_or_else(|| {
            ErrorData::internal_error(
                "server-side MCP tool execution requires a remote-backed host".to_string(),
                None,
            )
        })?;
        let args_value = Value::Object(arguments.unwrap_or_default());
        let args_json = serde_json::to_vec(&args_value).map_err(|e| {
            err(LoomError::new(
                Code::Internal,
                format!("encode tool arguments: {e}"),
            ))
        })?;
        let result_bytes = backend.execute_tool(name, &args_json).map_err(err)?;
        let value: Value = serde_json::from_slice(&result_bytes).map_err(|e| {
            err(LoomError::new(
                Code::Internal,
                format!("decode tool result: {e}"),
            ))
        })?;
        Ok(CallToolResult::structured(value))
    }

    /// Fill each `substrate_transact` op's omitted `workspace` (and, for document/graph ops,
    /// `collection`) from the active binding, so the hosted server receives fully explicit ops and runs
    /// the transaction unbound. Ops left without a workspace/collection stay unresolved and are rejected
    /// server-side.
    fn normalize_substrate_transact_arguments(
        &self,
        arguments: Option<serde_json::Map<String, Value>>,
    ) -> Option<serde_json::Map<String, Value>> {
        let mut arguments = arguments?;
        let Some(Value::Array(ops)) = arguments.get_mut("ops") else {
            return Some(arguments);
        };
        for op in ops.iter_mut() {
            let Some(op) = op.as_object_mut() else {
                continue;
            };
            let kind = op
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if op.get("workspace").is_none_or(|v| v.is_null())
                && let Some(workspace) = &self.binding.workspace
            {
                op.insert("workspace".to_string(), Value::String(workspace.clone()));
            }
            let has_collection = kind.starts_with("document.") || kind.starts_with("graph.");
            if has_collection
                && op.get("collection").is_none_or(|v| v.is_null())
                && let Some(collection) = &self.binding.collection
            {
                op.insert("collection".to_string(), Value::String(collection.clone()));
            }
        }
        Some(arguments)
    }

    fn inject_binding(&self, request: &mut CallToolRequestParams) {
        let Some(spec) = crate::tools::tool(&request.name) else {
            return;
        };
        let mut inject: Vec<(&str, String)> = Vec::new();
        if area_has_principal(spec.area) {
            inject.push(("principal", self.binding.principal.clone()));
        }
        if let Some(ns) = &self.binding.workspace {
            inject.push(("workspace", ns.clone()));
        }
        if let Some(col) = &self.binding.collection
            && let Some(p) = collection_param(spec.area)
        {
            inject.push((p, col.clone()));
        }
        if inject.is_empty() {
            return;
        }
        let args = request.arguments.get_or_insert_with(serde_json::Map::new);
        for (k, v) in inject {
            args.insert(k.to_string(), Value::String(v));
        }
    }

    // ===== store =====
    #[tool(
        name = "store_version",
        description = "Engine version",
        annotations(read_only_hint = true)
    )]
    fn store_version(&self) -> ToolResult {
        ser(self.mcp.version())
    }
    #[tool(
        name = "store_capabilities",
        description = "Capability registry (canonical CBOR)",
        annotations(read_only_hint = true)
    )]
    fn store_capabilities(&self) -> ToolResult {
        self.mcp
            .read_capabilities()
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }
    #[tool(
        name = "store_capabilities_json",
        description = "Capability matrix JSON",
        annotations(read_only_hint = true)
    )]
    fn store_capabilities_json(&self, Parameters(a): Parameters<PCapabilities>) -> ToolResult {
        self.mcp
            .read_capabilities_json(a.detailed)
            .map_err(err)
            .and_then(|json| {
                serde_json::from_str::<Value>(&json).map_err(|e| {
                    err(LoomError::new(
                        Code::Internal,
                        format!("decode capability JSON: {e}"),
                    ))
                })
            })
            .map(out_value)
    }
    #[tool(
        name = "store_blob_digest",
        description = "Content address of bytes",
        annotations(read_only_hint = true)
    )]
    fn store_blob_digest(&self, Parameters(a): Parameters<PData>) -> ToolResult {
        ser(self.mcp.read_blob_digest(&a.data))
    }
    #[tool(
        name = "store_maintenance_status",
        description = "Store maintenance policy, status, and last-run counters",
        annotations(read_only_hint = true)
    )]
    fn store_maintenance_status(&self) -> ToolResult {
        self.mcp
            .store()
            .read(|loom| {
                let report = loom.store().store_maintenance_report(crate::now_ms())?;
                let diagnostics = maintenance_live_root_diagnostics(loom)?;
                Ok((report, diagnostics))
            })
            .map_err(err)
            .map(|(report, diagnostics)| {
                out_value(maintenance_report_json(&report, Some(&diagnostics)))
            })
    }
    #[tool(
        name = "store_maintenance_policy_set",
        description = "Update store maintenance policy fields"
    )]
    fn store_maintenance_policy_set(
        &self,
        Parameters(a): Parameters<PStoreMaintenancePolicySet>,
    ) -> ToolResult {
        self.mcp
            .store()
            .write(|loom| {
                let mut policy = loom.store().store_maintenance_policy()?;
                if let Some(value) = a.min_candidate_pages {
                    policy.min_candidate_pages = value;
                }
                if let Some(value) = a.min_reusable_pages {
                    policy.min_reusable_pages = value;
                }
                if let Some(value) = a.interval_ms {
                    policy.interval_ms = value;
                }
                if let Some(value) = a.backoff_ms {
                    policy.backoff_ms = value;
                }
                if let Some(value) = a.max_segments {
                    policy.max_segments = value;
                }
                if let Some(value) = a.max_pages {
                    policy.max_pages = value;
                }
                if let Some(value) = a.full_compaction_enabled {
                    policy.full_compaction_enabled = value;
                }
                if let Some(value) = a.tail_trim_enabled {
                    policy.tail_trim_enabled = value;
                }
                if let Some(value) = a.tail_compaction_enabled {
                    policy.tail_compaction_enabled = value;
                }
                if let Some(value) = a.tail_compaction_max_pages {
                    policy.tail_compaction_max_pages = value;
                }
                if let Some(value) = a.tail_compaction_max_objects {
                    policy.tail_compaction_max_objects = value;
                }
                if let Some(value) = a.tail_compaction_max_bytes {
                    policy.tail_compaction_max_bytes = value;
                }
                if let Some(value) = a.tail_compaction_interval_ms {
                    policy.tail_compaction_interval_ms = value;
                }
                if let Some(value) = a.tail_compaction_backoff_ms {
                    policy.tail_compaction_backoff_ms = value;
                }
                loom.store().set_store_maintenance_policy(policy)?;
                let report = loom.store().store_maintenance_report(crate::now_ms())?;
                let diagnostics = maintenance_live_root_diagnostics(loom)?;
                Ok((report, diagnostics))
            })
            .map_err(err)
            .map(|(report, diagnostics)| {
                out_value(maintenance_report_json(&report, Some(&diagnostics)))
            })
    }
    #[tool(
        name = "store_maintenance_run",
        description = "Run one bounded store maintenance pass"
    )]
    fn store_maintenance_run(&self, Parameters(a): Parameters<PStoreMaintenanceRun>) -> ToolResult {
        self.mcp
            .store()
            .write(|loom| {
                run_mcp_store_maintenance_once(
                    loom,
                    crate::now_ms(),
                    a.manual,
                    a.max_segments,
                    a.max_pages,
                )
            })
            .map_err(err)
            .map(out_value)
    }

    // ===== telemetry =====
    #[tool(
        name = "metrics_put_descriptor",
        description = "Store a metric descriptor"
    )]
    fn metrics_put_descriptor(
        &self,
        Parameters(a): Parameters<PMetricsPutDescriptor>,
    ) -> ToolResult {
        self.mcp
            .write_metrics_put_descriptor(&a.workspace, &a.descriptor)
            .map_err(err)
            .map(|()| out_value(Value::String("ok".into())))
    }
    #[tool(
        name = "metrics_get_descriptor",
        description = "Read a metric descriptor",
        annotations(read_only_hint = true)
    )]
    fn metrics_get_descriptor(
        &self,
        Parameters(a): Parameters<PMetricsGetDescriptor>,
    ) -> ToolResult {
        self.mcp
            .read_metrics_get_descriptor(&a.workspace, &a.name)
            .map_err(err)
            .map(|value| out_value(jopt_bytes(value.as_deref())))
    }
    #[tool(
        name = "metrics_put_observation",
        description = "Store a metric observation"
    )]
    fn metrics_put_observation(
        &self,
        Parameters(a): Parameters<PMetricsPutObservation>,
    ) -> ToolResult {
        self.mcp
            .write_metrics_put_observation(&a.workspace, &a.descriptor_name, &a.observation)
            .map_err(err)
            .map(|()| out_value(Value::String("ok".into())))
    }
    #[tool(
        name = "metrics_query",
        description = "Query metric observations as canonical CBOR",
        annotations(read_only_hint = true)
    )]
    fn metrics_query(&self, Parameters(a): Parameters<PMetricsQuery>) -> ToolResult {
        self.mcp
            .read_metrics_query(
                &a.workspace,
                &a.descriptor_name,
                a.from_timestamp_ms,
                a.to_timestamp_ms,
                a.max_series,
                a.max_groups,
                a.max_samples,
                a.max_output_bytes,
                a.now_timestamp_ms,
            )
            .map_err(err)
            .map(|bytes| out_value(jbytes(&bytes)))
    }
    #[tool(name = "logs_put_record", description = "Store a log record")]
    fn logs_put_record(&self, Parameters(a): Parameters<PLogsPutRecord>) -> ToolResult {
        self.mcp
            .write_logs_put_record(&a.workspace, &a.record)
            .map_err(err)
            .map(|record_id| out_value(Value::String(record_id)))
    }
    #[tool(
        name = "logs_get_record",
        description = "Read a log record",
        annotations(read_only_hint = true)
    )]
    fn logs_get_record(&self, Parameters(a): Parameters<PLogsGetRecord>) -> ToolResult {
        self.mcp
            .read_logs_get_record(&a.workspace, &a.record_id)
            .map_err(err)
            .map(|value| out_value(jopt_bytes(value.as_deref())))
    }
    #[tool(
        name = "logs_query",
        description = "Query log records as canonical CBOR",
        annotations(read_only_hint = true)
    )]
    fn logs_query(&self, Parameters(a): Parameters<PLogsQuery>) -> ToolResult {
        self.mcp
            .read_logs_query(
                &a.workspace,
                a.from_time_unix_nano,
                a.to_time_unix_nano,
                a.max_records,
                a.max_output_bytes,
            )
            .map_err(err)
            .map(|bytes| out_value(jbytes(&bytes)))
    }
    #[tool(name = "traces_put_span", description = "Store a trace span")]
    fn traces_put_span(&self, Parameters(a): Parameters<PTracesPutSpan>) -> ToolResult {
        self.mcp
            .write_traces_put_span(&a.workspace, &a.span)
            .map_err(err)
            .map(|()| out_value(Value::String("ok".into())))
    }
    #[tool(
        name = "traces_get_span",
        description = "Read a trace span",
        annotations(read_only_hint = true)
    )]
    fn traces_get_span(&self, Parameters(a): Parameters<PTracesGetSpan>) -> ToolResult {
        self.mcp
            .read_traces_get_span(&a.workspace, &a.trace_id, &a.span_id)
            .map_err(err)
            .map(|value| out_value(jopt_bytes(value.as_deref())))
    }
    #[tool(
        name = "traces_trace_spans",
        description = "Query spans in one trace as canonical CBOR",
        annotations(read_only_hint = true)
    )]
    fn traces_trace_spans(&self, Parameters(a): Parameters<PTracesTraceSpans>) -> ToolResult {
        self.mcp
            .read_traces_trace_spans(&a.workspace, &a.trace_id, a.max_spans, a.max_output_bytes)
            .map_err(err)
            .map(|bytes| out_value(jbytes(&bytes)))
    }
    #[tool(
        name = "traces_query",
        description = "Query trace spans as canonical CBOR",
        annotations(read_only_hint = true)
    )]
    fn traces_query(&self, Parameters(a): Parameters<PTracesQuery>) -> ToolResult {
        self.mcp
            .read_traces_query(
                &a.workspace,
                a.from_start_time_ns,
                a.to_start_time_ns,
                a.max_spans,
                a.max_output_bytes,
            )
            .map_err(err)
            .map(|bytes| out_value(jbytes(&bytes)))
    }

    // ===== workspace =====
    #[tool(
        name = "workspace_list",
        description = "Workspace registry entries",
        annotations(read_only_hint = true)
    )]
    fn workspace_list(&self) -> ToolResult {
        self.mcp.read_workspace_list().map_err(err).and_then(ser)
    }

    // ===== cas =====
    #[tool(name = "cas_put", description = "Store a blob; returns its address")]
    fn cas_put(&self, Parameters(a): Parameters<PCasPut>) -> ToolResult {
        self.mcp
            .write_cas_put(&a.workspace, &a.content)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "cas_get",
        description = "Fetch a blob",
        annotations(read_only_hint = true)
    )]
    fn cas_get(&self, Parameters(a): Parameters<PCasDigest>) -> ToolResult {
        self.mcp
            .read_cas_get(&a.workspace, &a.digest)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "cas_has",
        description = "Whether a blob is reachable",
        annotations(read_only_hint = true)
    )]
    fn cas_has(&self, Parameters(a): Parameters<PCasDigest>) -> ToolResult {
        self.mcp
            .read_cas_has(&a.workspace, &a.digest)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "cas_delete",
        description = "Unlink a blob; returns whether present"
    )]
    fn cas_delete(&self, Parameters(a): Parameters<PCasDigest>) -> ToolResult {
        self.mcp
            .write_cas_delete(&a.workspace, &a.digest)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "cas_list",
        description = "List reachable blob addresses",
        annotations(read_only_hint = true)
    )]
    fn cas_list(&self, Parameters(a): Parameters<PNs>) -> ToolResult {
        self.mcp
            .read_cas_list(&a.workspace)
            .map_err(err)
            .and_then(ser)
    }

    // ===== graph =====
    #[tool(name = "graph_upsert_node", description = "Upsert a graph node")]
    fn graph_upsert_node(&self, Parameters(a): Parameters<PGraphUpsertNode>) -> ToolResult {
        self.mcp
            .write_graph_upsert_node(&a.workspace, &a.collection, &a.id, &a.props)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "graph_get_node",
        description = "Get graph node properties",
        annotations(read_only_hint = true)
    )]
    fn graph_get_node(&self, Parameters(a): Parameters<PNsNameId>) -> ToolResult {
        self.mcp
            .read_graph_get_node(&a.workspace, &a.collection, &a.id)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "graph_remove_node", description = "Remove a graph node")]
    fn graph_remove_node(&self, Parameters(a): Parameters<PGraphRemoveNode>) -> ToolResult {
        self.mcp
            .write_graph_remove_node(&a.workspace, &a.collection, &a.id, a.cascade)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "graph_upsert_edge", description = "Upsert a graph edge")]
    fn graph_upsert_edge(&self, Parameters(a): Parameters<PGraphUpsertEdge>) -> ToolResult {
        self.mcp
            .write_graph_upsert_edge(
                &a.workspace,
                &a.collection,
                GraphEdgeWrite {
                    id: &a.id,
                    src: &a.src,
                    dst: &a.dst,
                    label: &a.label,
                    props: &a.props,
                },
            )
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "graph_get_edge",
        description = "Get a graph edge",
        annotations(read_only_hint = true)
    )]
    fn graph_get_edge(&self, Parameters(a): Parameters<PNsNameId>) -> ToolResult {
        self.mcp
            .read_graph_get_edge(&a.workspace, &a.collection, &a.id)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "graph_remove_edge", description = "Remove a graph edge")]
    fn graph_remove_edge(&self, Parameters(a): Parameters<PNsNameId>) -> ToolResult {
        self.mcp
            .write_graph_remove_edge(&a.workspace, &a.collection, &a.id)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "graph_neighbors",
        description = "Adjacent node ids",
        annotations(read_only_hint = true)
    )]
    fn graph_neighbors(&self, Parameters(a): Parameters<PNsNameId>) -> ToolResult {
        self.mcp
            .read_graph_neighbors(&a.workspace, &a.collection, &a.id)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }
    #[tool(
        name = "graph_out_edges",
        description = "Outgoing graph edges",
        annotations(read_only_hint = true)
    )]
    fn graph_out_edges(&self, Parameters(a): Parameters<PNsNameId>) -> ToolResult {
        self.mcp
            .read_graph_out_edges(&a.workspace, &a.collection, &a.id)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }
    #[tool(
        name = "graph_in_edges",
        description = "Incoming graph edges",
        annotations(read_only_hint = true)
    )]
    fn graph_in_edges(&self, Parameters(a): Parameters<PNsNameId>) -> ToolResult {
        self.mcp
            .read_graph_in_edges(&a.workspace, &a.collection, &a.id)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }
    #[tool(
        name = "graph_reachable",
        description = "Reachable graph node ids",
        annotations(read_only_hint = true)
    )]
    fn graph_reachable(&self, Parameters(a): Parameters<PGraphReachable>) -> ToolResult {
        self.mcp
            .read_graph_reachable(
                &a.workspace,
                &a.collection,
                &a.start,
                a.max_depth,
                a.via_label.as_deref(),
            )
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }
    #[tool(
        name = "graph_shortest_path",
        description = "Shortest graph path",
        annotations(read_only_hint = true)
    )]
    fn graph_shortest_path(&self, Parameters(a): Parameters<PGraphShortestPath>) -> ToolResult {
        self.mcp
            .read_graph_shortest_path(
                &a.workspace,
                &a.collection,
                &a.from,
                &a.to,
                a.via_label.as_deref(),
            )
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "graph_query",
        description = "Run a bounded graph query",
        annotations(read_only_hint = true)
    )]
    fn graph_query(&self, Parameters(a): Parameters<PGraphQuery>) -> ToolResult {
        self.mcp
            .read_graph_query(&a.workspace, &a.collection, &a.query)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }
    #[tool(
        name = "graph_explain_query",
        description = "Explain a bounded graph query",
        annotations(read_only_hint = true)
    )]
    fn graph_explain_query(&self, Parameters(a): Parameters<PGraphQuery>) -> ToolResult {
        self.mcp
            .read_graph_explain_query(&a.workspace, &a.collection, &a.query)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }

    // ===== vector =====
    #[tool(name = "vector_create", description = "Create a vector set")]
    fn vector_create(&self, Parameters(a): Parameters<PVectorCreate>) -> ToolResult {
        self.mcp
            .write_vector_create(&a.workspace, &a.collection, a.dim, a.metric)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "vector_upsert", description = "Upsert a vector")]
    fn vector_upsert(&self, Parameters(a): Parameters<PVectorUpsert>) -> ToolResult {
        self.mcp
            .write_vector_upsert(&a.workspace, &a.collection, &a.id, &a.vector, &a.metadata)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "vector_upsert_source",
        description = "Upsert a vector with source text"
    )]
    fn vector_upsert_source(&self, Parameters(a): Parameters<PVectorUpsertSource>) -> ToolResult {
        self.mcp
            .write_vector_upsert_source(
                &a.workspace,
                &a.collection,
                VectorSourceWrite {
                    id: &a.id,
                    vector: &a.vector,
                    metadata: &a.metadata,
                    source_text: &a.source_text,
                    model_id: a.model_id.as_deref(),
                    weights_digest: a.weights_digest.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "vector_get",
        description = "Get a vector entry",
        annotations(read_only_hint = true)
    )]
    fn vector_get(&self, Parameters(a): Parameters<PNsNameId>) -> ToolResult {
        self.mcp
            .read_vector_get(&a.workspace, &a.collection, &a.id)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "vector_source_text",
        description = "Get vector source text",
        annotations(read_only_hint = true)
    )]
    fn vector_source_text(&self, Parameters(a): Parameters<PNsNameId>) -> ToolResult {
        self.mcp
            .read_vector_source_text(&a.workspace, &a.collection, &a.id)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "vector_embedding_model",
        description = "Get vector embedding model profile",
        annotations(read_only_hint = true)
    )]
    fn vector_embedding_model(&self, Parameters(a): Parameters<PNsName>) -> ToolResult {
        self.mcp
            .read_vector_embedding_model(&a.workspace, &a.collection)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "vector_ids",
        description = "List vector ids",
        annotations(read_only_hint = true)
    )]
    fn vector_ids(&self, Parameters(a): Parameters<PVectorIds>) -> ToolResult {
        let prefix = a.has_prefix.then_some(a.prefix.as_str());
        self.mcp
            .read_vector_ids(&a.workspace, &a.collection, prefix)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }
    #[tool(
        name = "vector_metadata_index_keys",
        description = "List vector metadata index keys",
        annotations(read_only_hint = true)
    )]
    fn vector_metadata_index_keys(&self, Parameters(a): Parameters<PNsName>) -> ToolResult {
        self.mcp
            .read_vector_metadata_index_keys(&a.workspace, &a.collection)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }
    #[tool(
        name = "vector_create_metadata_index",
        description = "Create a vector metadata equality index"
    )]
    fn vector_create_metadata_index(&self, Parameters(a): Parameters<PVectorKey>) -> ToolResult {
        self.mcp
            .write_vector_create_metadata_index(&a.workspace, &a.collection, &a.key)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "vector_drop_metadata_index",
        description = "Drop a vector metadata equality index"
    )]
    fn vector_drop_metadata_index(&self, Parameters(a): Parameters<PVectorKey>) -> ToolResult {
        self.mcp
            .write_vector_drop_metadata_index(&a.workspace, &a.collection, &a.key)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "vector_delete", description = "Delete a vector")]
    fn vector_delete(&self, Parameters(a): Parameters<PNsNameId>) -> ToolResult {
        self.mcp
            .write_vector_delete(&a.workspace, &a.collection, &a.id)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "vector_search",
        description = "Search vectors exactly",
        annotations(read_only_hint = true)
    )]
    fn vector_search(&self, Parameters(a): Parameters<PVectorSearch>) -> ToolResult {
        self.mcp
            .read_vector_search(&a.workspace, &a.collection, &a.query, a.k, &a.filter)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }
    #[tool(
        name = "vector_search_policy",
        description = "Search vectors with explicit accelerator policy",
        annotations(read_only_hint = true)
    )]
    fn vector_search_policy(&self, Parameters(a): Parameters<PVectorSearchPolicy>) -> ToolResult {
        self.mcp
            .read_vector_search_policy(VectorSearchPolicyRead {
                workspace: &a.workspace,
                name: &a.collection,
                query: &a.query,
                k: a.k,
                filter: &a.filter,
                policy: a.policy,
                threshold: a.threshold,
                ef: a.ef,
                pq_m: a.pq_m,
                pq_k: a.pq_k,
                pq_iters: a.pq_iters,
            })
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }

    // ===== columnar =====
    #[tool(name = "columnar_create", description = "Create a columnar dataset")]
    fn columnar_create(&self, Parameters(a): Parameters<PColumnarCreate>) -> ToolResult {
        self.mcp
            .write_columnar_create(
                &a.workspace,
                &a.collection,
                &a.columns,
                a.target_segment_rows,
            )
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "columnar_append", description = "Append a columnar row")]
    fn columnar_append(&self, Parameters(a): Parameters<PColumnarAppend>) -> ToolResult {
        self.mcp
            .write_columnar_append(&a.workspace, &a.collection, &a.row)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "columnar_compact", description = "Compact columnar segments")]
    fn columnar_compact(&self, Parameters(a): Parameters<PNsName>) -> ToolResult {
        self.mcp
            .write_columnar_compact(&a.workspace, &a.collection)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "columnar_scan",
        description = "Scan all columnar rows",
        annotations(read_only_hint = true)
    )]
    fn columnar_scan(&self, Parameters(a): Parameters<PNsName>) -> ToolResult {
        self.mcp
            .read_columnar_scan(&a.workspace, &a.collection)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }
    #[tool(
        name = "columnar_columns",
        description = "Get columnar schema columns",
        annotations(read_only_hint = true)
    )]
    fn columnar_columns(&self, Parameters(a): Parameters<PNsName>) -> ToolResult {
        self.mcp
            .read_columnar_columns(&a.workspace, &a.collection)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }
    #[tool(
        name = "columnar_rows",
        description = "Columnar row count",
        annotations(read_only_hint = true)
    )]
    fn columnar_rows(&self, Parameters(a): Parameters<PNsName>) -> ToolResult {
        self.mcp
            .read_columnar_rows(&a.workspace, &a.collection)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "columnar_inspect",
        description = "Inspect columnar metadata",
        annotations(read_only_hint = true)
    )]
    fn columnar_inspect(&self, Parameters(a): Parameters<PNsName>) -> ToolResult {
        self.mcp
            .read_columnar_inspect(&a.workspace, &a.collection)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }
    #[tool(
        name = "columnar_source_digest",
        description = "Get columnar source digest",
        annotations(read_only_hint = true)
    )]
    fn columnar_source_digest(&self, Parameters(a): Parameters<PNsName>) -> ToolResult {
        self.mcp
            .read_columnar_source_digest(&a.workspace, &a.collection)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "columnar_select",
        description = "Select projected columnar rows",
        annotations(read_only_hint = true)
    )]
    fn columnar_select(&self, Parameters(a): Parameters<PColumnarSelect>) -> ToolResult {
        let filter =
            columnar_select_filter_arg(a.filter.as_deref(), a.predicate.as_ref()).map_err(err)?;
        self.mcp
            .read_columnar_select(&a.workspace, &a.collection, &a.columns, &filter)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }
    #[tool(
        name = "columnar_aggregate",
        description = "Evaluate columnar aggregate expressions",
        annotations(read_only_hint = true)
    )]
    fn columnar_aggregate(&self, Parameters(a): Parameters<PColumnarAggregate>) -> ToolResult {
        self.mcp
            .read_columnar_aggregate(&a.workspace, &a.collection, &a.aggregates, &a.filter)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }

    // ===== dataframe =====
    #[tool(name = "dataframe_create", description = "Create a dataframe frame")]
    fn dataframe_create(&self, Parameters(a): Parameters<PDataframeCreate>) -> ToolResult {
        self.mcp
            .write_dataframe_create(&a.workspace, &a.collection, &a.plan)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "dataframe_collect",
        description = "Collect dataframe rows",
        annotations(read_only_hint = true)
    )]
    fn dataframe_collect(&self, Parameters(a): Parameters<PNsName>) -> ToolResult {
        self.mcp
            .read_dataframe_collect(&a.workspace, &a.collection)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }
    #[tool(
        name = "dataframe_preview",
        description = "Preview dataframe rows",
        annotations(read_only_hint = true)
    )]
    fn dataframe_preview(&self, Parameters(a): Parameters<PDataframePreview>) -> ToolResult {
        self.mcp
            .read_dataframe_preview(&a.workspace, &a.collection, a.rows)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }
    #[tool(
        name = "dataframe_materialize",
        description = "Materialize a dataframe frame"
    )]
    fn dataframe_materialize(&self, Parameters(a): Parameters<PNsName>) -> ToolResult {
        self.mcp
            .write_dataframe_materialize(&a.workspace, &a.collection)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "dataframe_plan_digest",
        description = "Get dataframe plan digest",
        annotations(read_only_hint = true)
    )]
    fn dataframe_plan_digest(&self, Parameters(a): Parameters<PNsName>) -> ToolResult {
        self.mcp
            .read_dataframe_plan_digest(&a.workspace, &a.collection)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "dataframe_source_digests",
        description = "Get dataframe source digests",
        annotations(read_only_hint = true)
    )]
    fn dataframe_source_digests(&self, Parameters(a): Parameters<PNsName>) -> ToolResult {
        self.mcp
            .read_dataframe_source_digests(&a.workspace, &a.collection)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }

    // ===== fts =====
    #[tool(name = "fts_create", description = "Create a full-text collection")]
    fn fts_create(&self, Parameters(a): Parameters<PFtsCreate>) -> ToolResult {
        self.mcp
            .write_fts_create(&a.workspace, &a.collection, &a.mapping)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "fts_index", description = "Index a full-text document")]
    fn fts_index(&self, Parameters(a): Parameters<PFtsIndex>) -> ToolResult {
        self.mcp
            .write_fts_index(&a.workspace, &a.collection, a.id, &a.doc)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "fts_get",
        description = "Get a full-text document",
        annotations(read_only_hint = true)
    )]
    fn fts_get(&self, Parameters(a): Parameters<PFtsId>) -> ToolResult {
        self.mcp
            .read_fts_get(&a.workspace, &a.collection, &a.id)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "fts_delete", description = "Delete a full-text document")]
    fn fts_delete(&self, Parameters(a): Parameters<PFtsId>) -> ToolResult {
        self.mcp
            .write_fts_delete(&a.workspace, &a.collection, &a.id)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "fts_ids",
        description = "List full-text document ids",
        annotations(read_only_hint = true)
    )]
    fn fts_ids(&self, Parameters(a): Parameters<PFtsIds>) -> ToolResult {
        let prefix = a.has_prefix.then_some(a.prefix.as_slice());
        self.mcp
            .read_fts_ids(&a.workspace, &a.collection, prefix)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }
    #[tool(name = "fts_remap", description = "Replace a full-text mapping")]
    fn fts_remap(&self, Parameters(a): Parameters<PFtsCreate>) -> ToolResult {
        self.mcp
            .write_fts_remap(&a.workspace, &a.collection, &a.mapping)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "fts_query",
        description = "Run a portable full-text collection query",
        annotations(read_only_hint = true)
    )]
    fn fts_query(&self, Parameters(a): Parameters<PFtsQuery>) -> ToolResult {
        self.mcp
            .read_fts_query(&a.workspace, &a.collection, &a.request)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }
    #[tool(
        name = "fts_source_digest",
        description = "Read the full-text source digest",
        annotations(read_only_hint = true)
    )]
    fn fts_source_digest(&self, Parameters(a): Parameters<PNsName>) -> ToolResult {
        self.mcp
            .read_fts_source_digest(&a.workspace, &a.collection)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "fts_status",
        description = "Read full-text derived index status",
        annotations(read_only_hint = true)
    )]
    fn fts_status(&self, Parameters(a): Parameters<PFtsStatus>) -> ToolResult {
        self.mcp
            .read_fts_status(&a.workspace, &a.collection, &a.engine_version)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }

    // ===== tools =====
    #[tool(
        name = "search",
        description = "Search readable full-text collections with a 0064-shaped response",
        annotations(read_only_hint = true)
    )]
    fn search(&self, Parameters(a): Parameters<PStoreSearch>) -> ToolResult {
        self.mcp
            .read_store_search(StoreSearchReadRequest {
                workspace: a.workspace.as_deref(),
                collection: a.collection.as_deref(),
                query: &a.query,
                field: a.field.as_deref(),
                limit: a.limit.unwrap_or(20),
                offset: a.offset.unwrap_or(0),
            })
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "substrate_changes",
        description = "Read durable changes with LMDIFF envelopes",
        annotations(read_only_hint = true)
    )]
    fn substrate_changes(&self, Parameters(a): Parameters<PSubstrateChanges>) -> ToolResult {
        self.mcp
            .read_substrate_changes(&a.workspace, &a.cursor, a.max)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "workgraph_changes",
        description = "Read paged workgraph lifecycle operation changes",
        annotations(read_only_hint = true)
    )]
    fn workgraph_changes(&self, Parameters(a): Parameters<PWorkgraphChanges>) -> ToolResult {
        self.mcp
            .read_workgraph_changes(&a.workspace, &a.workspace_id, a.next_sequence, a.max)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "workgraph_metrics",
        description = "Derive bounded workgraph ticket and Lane metrics",
        annotations(read_only_hint = true)
    )]
    fn workgraph_metrics(&self, Parameters(a): Parameters<PWorkgraphMetrics>) -> ToolResult {
        self.mcp
            .read_workgraph_metrics(
                &a.workspace,
                a.workspace_id.as_deref(),
                &a.statuses,
                &a.lanes,
                a.limit,
            )
            .map_err(err)
            .map(out_value)
    }
    #[tool(
        name = "workgraph_fact_put",
        description = "Append a canonical workgraph lifecycle fact"
    )]
    fn workgraph_fact_put(&self, Parameters(a): Parameters<PWorkgraphFactPut>) -> ToolResult {
        self.mcp
            .write_workgraph_fact_put(&a.workspace, &a.workspace_id, a.fact)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "substrate_refs",
        description = "Read inbound typed references for an entity",
        annotations(read_only_hint = true)
    )]
    fn substrate_refs(&self, Parameters(a): Parameters<PSubstrateRefs>) -> ToolResult {
        self.mcp
            .read_substrate_refs(&a.workspace, &a.target)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "substrate_alias_bind",
        description = "Bind a display alias to a stable substrate entity reference"
    )]
    fn substrate_alias_bind(&self, Parameters(a): Parameters<PSubstrateAliasBind>) -> ToolResult {
        self.mcp
            .write_substrate_alias_bind(&a.workspace, &a.scope_id, &a.alias, &a.target)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "substrate_alias_release",
        description = "Release a display alias binding in a substrate scope"
    )]
    fn substrate_alias_release(&self, Parameters(a): Parameters<PSubstrateAliasKey>) -> ToolResult {
        self.mcp
            .write_substrate_alias_release(&a.workspace, &a.scope_id, &a.alias)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "substrate_alias_resolve",
        description = "Resolve a display alias in a substrate scope",
        annotations(read_only_hint = true)
    )]
    fn substrate_alias_resolve(&self, Parameters(a): Parameters<PSubstrateAliasKey>) -> ToolResult {
        self.mcp
            .read_substrate_alias_resolve(&a.workspace, &a.scope_id, &a.alias)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "substrate_alias_list",
        description = "List display aliases in a substrate scope",
        annotations(read_only_hint = true)
    )]
    fn substrate_alias_list(&self, Parameters(a): Parameters<PSubstrateAliasList>) -> ToolResult {
        self.mcp
            .read_substrate_alias_list(&a.workspace, &a.scope_id)
            .map_err(err)
            .and_then(|items| slice_results(items, &a.page))
            .and_then(ser)
    }
    #[tool(
        name = "substrate_reference_status",
        description = "Read unresolved-reference reconciliation status",
        annotations(read_only_hint = true)
    )]
    fn substrate_reference_status(
        &self,
        Parameters(a): Parameters<PSubstrateReferenceStatus>,
    ) -> ToolResult {
        self.mcp
            .read_substrate_reference_reconciliation_status(&a.workspace)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "substrate_reference_reconcile",
        description = "Run one bounded keyed reference-reconciliation batch"
    )]
    fn substrate_reference_reconcile(
        &self,
        Parameters(a): Parameters<PSubstrateReferenceReconcile>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_substrate_reference_reconcile(&a.workspace, &profile_id, a.max)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "substrate_history",
        description = "Read revision rows and checkpoints for an entity",
        annotations(read_only_hint = true)
    )]
    fn substrate_history(&self, Parameters(a): Parameters<PSubstrateHistory>) -> ToolResult {
        self.mcp
            .read_substrate_history(&a.workspace, &a.scope_id, &a.entity_id)
            .map_err(err)
            .and_then(|mut history| {
                history.revisions = slice_results(history.revisions, &a.page)?;
                ser(history)
            })
    }
    #[tool(
        name = "substrate_revision_latest",
        description = "Read the latest revision-index row for an entity",
        annotations(read_only_hint = true)
    )]
    fn substrate_revision_latest(
        &self,
        Parameters(a): Parameters<PSubstrateHistory>,
    ) -> ToolResult {
        self.mcp
            .read_substrate_revision_latest(&a.workspace, &a.scope_id, &a.entity_id)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "substrate_revision_at",
        description = "Read one revision-index row by entity revision number",
        annotations(read_only_hint = true)
    )]
    fn substrate_revision_at(&self, Parameters(a): Parameters<PSubstrateRevisionAt>) -> ToolResult {
        self.mcp
            .read_substrate_revision_at(&a.workspace, &a.scope_id, &a.entity_id, a.revision)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "substrate_revision_as_of_root",
        description = "Read the revision-index row for an entity at a stored profile root",
        annotations(read_only_hint = true)
    )]
    fn substrate_revision_as_of_root(
        &self,
        Parameters(a): Parameters<PSubstrateRevisionAsOfRoot>,
    ) -> ToolResult {
        self.mcp
            .read_substrate_revision_as_of_root(&a.workspace, &a.scope_id, &a.entity_id, &a.root)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "substrate_checkpoint_before",
        description = "Read the nearest revision-index checkpoint at or before a revision",
        annotations(read_only_hint = true)
    )]
    fn substrate_checkpoint_before(
        &self,
        Parameters(a): Parameters<PSubstrateCheckpointBefore>,
    ) -> ToolResult {
        self.mcp
            .read_substrate_checkpoint_before(&a.workspace, &a.scope_id, a.revision)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "substrate_transact",
        description = "Apply typed substrate operations atomically"
    )]
    fn substrate_transact(&self, Parameters(a): Parameters<PSubstrateTransact>) -> ToolResult {
        let ops = a
            .ops
            .into_iter()
            .map(|op| substrate_transact_op(&self.binding, op))
            .collect::<Result<Vec<_>, _>>()?;
        self.mcp
            .write_substrate_transact(ops)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "substrate_view_define",
        description = "Register a substrate view definition"
    )]
    fn substrate_view_define(&self, Parameters(a): Parameters<PSubstrateViewDefine>) -> ToolResult {
        let source_scopes = a
            .source_scopes
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let source_facets = a
            .source_facets
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        self.mcp
            .write_substrate_view_define(SubstrateViewDefineRequest {
                workspace: &a.workspace,
                view_id: &a.view_id,
                source_scopes: &source_scopes,
                source_facets: &source_facets,
                projection_ref: &a.projection_ref,
                output_facet: a.output_facet.as_deref(),
                media_type: &a.media_type,
                freshness_policy: &a.freshness_policy,
            })
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "substrate_view_get",
        description = "Read a substrate view definition",
        annotations(read_only_hint = true)
    )]
    fn substrate_view_get(&self, Parameters(a): Parameters<PSubstrateViewGet>) -> ToolResult {
        self.mcp
            .read_substrate_view_get(&a.workspace, &a.view_id)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "substrate_view_list",
        description = "List substrate view definitions",
        annotations(read_only_hint = true)
    )]
    fn substrate_view_list(&self, Parameters(a): Parameters<PSubstrateViewList>) -> ToolResult {
        self.mcp
            .read_substrate_view_list(&a.workspace)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "substrate_write_admission_policy_get",
        description = "Read a substrate write-admission policy",
        annotations(read_only_hint = true)
    )]
    fn substrate_write_admission_policy_get(
        &self,
        Parameters(a): Parameters<PSubstrateWriteAdmissionPolicyKey>,
    ) -> ToolResult {
        self.mcp
            .read_substrate_write_admission_policy(&a.workspace, &a.surface, &a.scope_id)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "substrate_write_admission_policy_set",
        description = "Set a substrate write-admission policy"
    )]
    fn substrate_write_admission_policy_set(
        &self,
        Parameters(a): Parameters<PSubstrateWriteAdmissionPolicySet>,
    ) -> ToolResult {
        let mandatory_targets = a
            .mandatory_targets
            .into_iter()
            .map(|target| WriteAdmissionTarget::new(target.target_kind, target.target_id))
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        self.mcp
            .write_substrate_write_admission_policy_set(WriteAdmissionPolicyRequest {
                workspace: &a.workspace,
                surface: &a.surface,
                scope_id: &a.scope_id,
                default_mode: &a.default_mode,
                mandatory_targets: &mandatory_targets,
            })
            .map_err(err)
            .and_then(ser)
    }

    // ===== tickets =====
    #[tool(
        name = "tickets_project_create",
        description = "Create a ticket project in a Studio workspace"
    )]
    fn tickets_project_create(
        &self,
        Parameters(a): Parameters<PTicketsProjectCreate>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_tickets_project_create(
                &a.workspace,
                &profile_id,
                &a.project_id,
                &a.key_prefix,
                &a.name,
                a.expected_root.as_deref(),
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "tickets_project_rekey",
        description = "Change a ticket project's active key prefix"
    )]
    fn tickets_project_rekey(&self, Parameters(a): Parameters<PTicketsProjectRekey>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_tickets_project_rekey(
                &a.workspace,
                &profile_id,
                &a.project_id,
                &a.key_prefix,
                a.expected_root.as_deref(),
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "tickets_project_settings_get",
        description = "Read a ticket project's projection and lifecycle settings",
        annotations(read_only_hint = true)
    )]
    fn tickets_project_settings_get(
        &self,
        Parameters(a): Parameters<PTicketsProjectSettingsGet>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_tickets_project_settings_get(
                &a.workspace,
                &profile_id,
                &a.project_id,
                a.include_contracts,
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "tickets_projects",
        description = "List ticket projects in a workspace with key prefix, name, and default projection (discover projects before creating or updating a ticket)",
        annotations(read_only_hint = true)
    )]
    fn tickets_projects(&self, Parameters(a): Parameters<PTicketsProjects>) -> ToolResult {
        self.mcp
            .read_tickets_projects(&a.workspace)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "tickets_relations",
        description = "List a ticket's dependency and relation edges in both directions (outgoing and incoming) with relation kind, target ticket id, and target title",
        annotations(read_only_hint = true)
    )]
    fn tickets_relations(&self, Parameters(a): Parameters<PTicketsRelations>) -> ToolResult {
        self.mcp
            .read_tickets_relations(&a.workspace, &a.ticket_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "tickets_fields",
        description = "Discover settable ticket fields, projection write paths, types, limits, and enum values",
        annotations(read_only_hint = true)
    )]
    fn tickets_fields(&self, Parameters(a): Parameters<PTicketsFields>) -> ToolResult {
        self.mcp
            .read_tickets_fields(
                &a.workspace,
                a.project_id.as_deref(),
                a.projection.as_deref(),
                a.operation.as_deref(),
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "tickets_field_put",
        description = "Create or update a project custom-field definition"
    )]
    fn tickets_field_put(&self, Parameters(a): Parameters<PTicketsFieldPut>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        let cardinality = ticket_field_cardinality(&a.cardinality).map_err(err)?;
        self.mcp
            .write_tickets_field_put(
                &a.workspace,
                TicketFieldDefinitionWriteRequest {
                    workspace_id: &profile_id,
                    project_id: &a.project_id,
                    field_id: &a.field_id,
                    key: &a.key,
                    name: &a.name,
                    description: a.description.as_deref(),
                    field_type: &a.field_type,
                    option_set: a.option_set.as_deref(),
                    max_length: a.max_length,
                    required: a.required,
                    searchable: a.searchable,
                    orderable: a.orderable,
                    cardinality,
                    applicable_type_ids: &a.applicable_type_ids,
                    expected_root: a.expected_root.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "tickets_field_retire",
        description = "Retire a project custom-field definition"
    )]
    fn tickets_field_retire(&self, Parameters(a): Parameters<PTicketsFieldRetire>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_tickets_field_retire(
                &a.workspace,
                TicketFieldDefinitionRetireRequest {
                    workspace_id: &profile_id,
                    project_id: &a.project_id,
                    field_id: &a.field_id,
                    expected_root: a.expected_root.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "tickets_project_settings_set",
        description = "Update a ticket project's projection and lifecycle settings"
    )]
    fn tickets_project_settings_set(
        &self,
        Parameters(a): Parameters<PTicketsProjectSettingsSet>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        let default_projection = a
            .default_projection
            .as_deref()
            .map(loom_tickets::TicketProjectionProfile::parse)
            .transpose()
            .map_err(err)?;
        let actor_enforcement = a
            .actor_enforcement
            .as_deref()
            .map(TicketLifecycleAuthorizationPolicy::parse)
            .transpose()
            .map_err(err)?;
        let acceptance_authorities = a.acceptance_authorities.as_deref();
        let required_acceptance_evidence_keys = a
            .required_acceptance_evidence_keys
            .as_deref()
            .map(|keys| {
                keys.iter()
                    .map(|key| loom_tickets::TicketAcceptanceEvidenceKey::parse(key))
                    .collect::<loom_core::Result<Vec<_>>>()
            })
            .transpose()
            .map_err(err)?;
        self.mcp
            .write_tickets_project_settings_set(
                &a.workspace,
                loom_tickets::TicketProjectSettingsRequest {
                    workspace_id: &profile_id,
                    project_id: &a.project_id,
                    default_projection,
                    enable_projections: &[],
                    disable_projections: &[],
                    actor_enforcement,
                    project_owner_principal: a.project_owner_principal.as_deref(),
                    clear_project_owner_principal: a.clear_project_owner_principal,
                    acceptance_authorities,
                    acceptance_evidence_enforcement: a.acceptance_evidence_enforcement,
                    required_acceptance_evidence_keys: required_acceptance_evidence_keys.as_deref(),
                    owner_contract_summary: a.owner_contract_summary.as_deref(),
                    owner_contract_details: a.owner_contract_details.as_deref(),
                    worker_contract_summary: a.worker_contract_summary.as_deref(),
                    worker_contract_details: a.worker_contract_details.as_deref(),
                    expected_root: a.expected_root.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(name = "tickets_create", description = "Create a ticket")]
    fn tickets_create(&self, Parameters(a): Parameters<PTicketsCreate>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        let fields = json_object_value(a.fields);
        let projection =
            loom_tickets::parse_ticket_projection(a.projection.as_deref()).map_err(err)?;
        let fields = loom_tickets::normalize_ticket_fields_for_projection(&fields, projection)
            .map_err(err)?;
        self.mcp
            .write_tickets_create_receipt(
                &a.workspace,
                TicketCreateRequest {
                    workspace_id: &profile_id,
                    project_id: &a.project_id,
                    ticket_type: &a.ticket_type,
                    external_source: a.external_source.as_deref(),
                    external_id: a.external_id.as_deref(),
                    fields: &fields,
                    policy_labels: &a.policy_labels,
                    expected_root: a.expected_root.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "tickets_update",
        description = "Apply one atomic ticket update"
    )]
    fn tickets_update(&self, Parameters(a): Parameters<PTicketsUpdate>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        let projection =
            loom_tickets::parse_ticket_projection(a.projection.as_deref()).map_err(err)?;
        let set_fields = a
            .set_fields
            .map(json_object_value)
            .map(|fields| loom_tickets::normalize_ticket_fields_for_projection(&fields, projection))
            .transpose()
            .map_err(err)?;
        let delete_fields = loom_tickets::normalize_ticket_delete_fields_for_projection(
            &a.delete_fields,
            projection,
        );
        let action = a
            .action
            .as_deref()
            .map(TicketLifecycleAction::parse)
            .transpose()
            .map_err(err)?;
        let comment = a
            .comment
            .as_ref()
            .map(|comment| {
                comment
                    .evidence
                    .as_ref()
                    .map(ticket_comment_evidence_from_map)
                    .transpose()
                    .map(|evidence| loom_tickets::TicketUpdateCommentRequest {
                        comment_id: comment.comment_id.as_deref(),
                        comment_type: comment.comment_type.as_deref(),
                        body: &comment.body,
                        evidence,
                    })
            })
            .transpose()
            .map_err(err)?;
        let comments = a
            .comments
            .iter()
            .map(|comment| {
                comment
                    .evidence
                    .as_ref()
                    .map(ticket_comment_evidence_from_map)
                    .transpose()
                    .map(|evidence| loom_tickets::TicketUpdateCommentRequest {
                        comment_id: comment.comment_id.as_deref(),
                        comment_type: comment.comment_type.as_deref(),
                        body: &comment.body,
                        evidence,
                    })
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        let relation_sets = a
            .relation_sets
            .iter()
            .map(|relation| {
                TicketRelationKind::parse(&relation.kind).map(|kind| {
                    loom_tickets::TicketUpdateRelationSetRequest {
                        relation_id: relation.relation_id.as_deref(),
                        kind,
                        target_id: &relation.target_id,
                    }
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        let relation_removes = a
            .relation_removes
            .iter()
            .map(|relation| loom_tickets::TicketUpdateRelationRemoveRequest {
                relation_id: &relation.relation_id,
            })
            .collect::<Vec<_>>();
        self.mcp
            .write_tickets_update_receipt(
                &a.workspace,
                TicketUpdateRequest {
                    workspace_id: &profile_id,
                    ticket_id: &a.ticket_id,
                    set_fields: set_fields.as_ref(),
                    delete_fields: &delete_fields,
                    action,
                    target_status: a.target_status.as_deref(),
                    observed_source_status: a.observed_source_status.as_deref(),
                    observed_workflow_version: a.observed_workflow_version.as_deref(),
                    assignee: a.assignee.as_deref(),
                    expected_root: a.expected_root.as_deref(),
                    comment,
                    comments: &comments,
                    relation_sets: &relation_sets,
                    relation_removes: &relation_removes,
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "tickets_delete",
        description = "Delete a ticket as an audited tombstone operation"
    )]
    fn tickets_delete(&self, Parameters(a): Parameters<PTicketsDelete>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_tickets_delete_receipt(
                &a.workspace,
                TicketDeleteRequest {
                    workspace_id: &profile_id,
                    ticket_id: &a.ticket_id,
                    expected_root: a.expected_root.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "tickets_comments",
        description = "List authenticated comments for a ticket",
        annotations(read_only_hint = true)
    )]
    fn tickets_comments(&self, Parameters(a): Parameters<PTicketsComments>) -> ToolResult {
        self.mcp
            .read_tickets_comments(&a.workspace, &a.ticket_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "tickets_comment_add",
        description = "Add an authenticated typed comment to a ticket"
    )]
    fn tickets_comment_add(&self, Parameters(a): Parameters<PTicketsCommentAdd>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_tickets_comment_add_receipt(
                &a.workspace,
                TicketCommentRequest {
                    workspace_id: &profile_id,
                    ticket_id: &a.ticket_id,
                    comment_id: a.comment_id.as_deref(),
                    comment_type: Some(&a.comment_type),
                    body: &a.body,
                    evidence: a
                        .evidence
                        .as_ref()
                        .map(ticket_comment_evidence_from_map)
                        .transpose()
                        .map_err(err)?,
                    expected_root: a.expected_root.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "tickets_comment_update",
        description = "Update a ticket comment body or comment type"
    )]
    fn tickets_comment_update(
        &self,
        Parameters(a): Parameters<PTicketsCommentUpdate>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_tickets_comment_update_receipt(
                &a.workspace,
                TicketCommentUpdateRequest {
                    workspace_id: &profile_id,
                    ticket_id: &a.ticket_id,
                    comment_id: &a.comment_id,
                    comment_type: a.comment_type.as_deref(),
                    body: a.body.as_deref(),
                    evidence: a
                        .evidence
                        .as_ref()
                        .map(|evidence| {
                            evidence
                                .as_ref()
                                .map(ticket_comment_evidence_from_map)
                                .transpose()
                        })
                        .transpose()
                        .map_err(err)?,
                    expected_root: a.expected_root.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "tickets_comment_delete",
        description = "Redact a ticket comment while preserving audit metadata"
    )]
    fn tickets_comment_delete(
        &self,
        Parameters(a): Parameters<PTicketsCommentDelete>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_tickets_comment_delete_receipt(
                &a.workspace,
                TicketCommentDeleteRequest {
                    workspace_id: &profile_id,
                    ticket_id: &a.ticket_id,
                    comment_id: &a.comment_id,
                    expected_root: a.expected_root.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "tickets_board_create",
        description = "Create a first-class Ticket Board"
    )]
    fn tickets_board_create(&self, Parameters(a): Parameters<PTicketsBoardCreate>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        let columns = board_columns(a.columns).map_err(err)?;
        let swimlanes = board_swimlanes(a.swimlanes).map_err(err)?;
        self.mcp
            .write_tickets_board_create(
                &a.workspace,
                BoardCreateRequest {
                    workspace_id: &profile_id,
                    board_id: &a.board_id,
                    board_key: &a.board_key,
                    name: &a.name,
                    description: &a.description,
                    project_id: &a.project_id,
                    scope: board_scope(&a.scope, &a.project_id).map_err(err)?,
                    mode: BoardMode::parse(&a.mode).map_err(err)?,
                    columns: &columns,
                    swimlanes: &swimlanes,
                    card_display_fields: &a.card_display_fields,
                    owner_principal: a.owner_principal.as_deref(),
                    coordinator_principal: a.coordinator_principal.as_deref(),
                    updated_by: &a.updated_by,
                    expected_root: a.expected_root.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "tickets_board_update",
        description = "Update first-class Ticket Board metadata"
    )]
    fn tickets_board_update(&self, Parameters(a): Parameters<PTicketsBoardUpdate>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        let status = a
            .board_status
            .as_deref()
            .map(BoardStatus::parse)
            .transpose()
            .map_err(err)?;
        self.mcp
            .write_tickets_board_update(
                &a.workspace,
                BoardUpdateRequest {
                    workspace_id: &profile_id,
                    board_id: &a.board_id,
                    board_key: a.board_key.as_deref(),
                    name: a.name.as_deref(),
                    description: a.description.as_deref(),
                    scope: None,
                    owner_principal: None,
                    coordinator_principal: None,
                    card_display_fields: a.card_display_fields.as_deref(),
                    board_status: status,
                    updated_by: &a.updated_by,
                    expected_root: a.expected_root.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "tickets_board_delete",
        description = "Delete a first-class Ticket Board as a tombstone"
    )]
    fn tickets_board_delete(&self, Parameters(a): Parameters<PTicketsBoardUpdate>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_tickets_board_update(
                &a.workspace,
                BoardUpdateRequest {
                    workspace_id: &profile_id,
                    board_id: &a.board_id,
                    board_key: None,
                    name: None,
                    description: None,
                    scope: None,
                    owner_principal: None,
                    coordinator_principal: None,
                    card_display_fields: None,
                    board_status: Some(BoardStatus::Deleted),
                    updated_by: &a.updated_by,
                    expected_root: a.expected_root.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "tickets_board_configure_columns",
        description = "Replace a first-class Ticket Board column and swimlane configuration"
    )]
    fn tickets_board_configure_columns(
        &self,
        Parameters(a): Parameters<PTicketsBoardConfigureColumns>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        let columns = board_columns(a.columns).map_err(err)?;
        let swimlanes = board_swimlanes(a.swimlanes).map_err(err)?;
        let mode = a
            .mode
            .as_deref()
            .map(BoardMode::parse)
            .transpose()
            .map_err(err)?;
        self.mcp
            .write_tickets_board_configure_columns(
                &a.workspace,
                BoardColumnConfigureRequest {
                    workspace_id: &profile_id,
                    board_id: &a.board_id,
                    mode,
                    columns: &columns,
                    swimlanes: &swimlanes,
                    updated_by: &a.updated_by,
                    expected_root: a.expected_root.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "tickets_board_move_card",
        description = "Move or reorder a first-class Ticket Board card"
    )]
    fn tickets_board_move_card(
        &self,
        Parameters(a): Parameters<PTicketsBoardMoveCard>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_tickets_board_move_card(
                &a.workspace,
                BoardCardMoveRequest {
                    workspace_id: &profile_id,
                    board_id: &a.board_id,
                    ticket_id: &a.ticket_id,
                    column_id: &a.column_id,
                    rank_token: &a.rank_token,
                    swimlane_id: a.swimlane_id.as_deref(),
                    updated_by: &a.updated_by,
                    expected_root: a.expected_root.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "tickets_relation_set",
        description = "Add or replace one ticket-owned typed relation"
    )]
    fn tickets_relation_set(&self, Parameters(a): Parameters<PTicketsRelationSet>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        let kind = TicketRelationKind::parse(&a.kind).map_err(err)?;
        self.mcp
            .write_tickets_relation_set_receipt(
                &a.workspace,
                TicketRelationRequest {
                    workspace_id: &profile_id,
                    ticket_id: &a.ticket_id,
                    relation_id: a.relation_id.as_deref(),
                    kind,
                    target_id: &a.target_id,
                    expected_root: a.expected_root.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "tickets_relation_remove",
        description = "Remove one ticket-owned typed relation"
    )]
    fn tickets_relation_remove(
        &self,
        Parameters(a): Parameters<PTicketsRelationRemove>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_tickets_relation_remove_receipt(
                &a.workspace,
                TicketRelationRemoveRequest {
                    workspace_id: &profile_id,
                    ticket_id: &a.ticket_id,
                    relation_id: &a.relation_id,
                    expected_root: a.expected_root.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "tickets_get",
        description = "Read a ticket by id or primary key",
        annotations(read_only_hint = true)
    )]
    fn tickets_get(&self, Parameters(a): Parameters<PTicketsGet>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        if a.detailed {
            self.mcp
                .read_tickets_get(
                    &a.workspace,
                    &profile_id,
                    &a.ticket_id,
                    a.projection.as_deref(),
                )
                .map_err(err)
                .and_then(ser)
        } else {
            self.mcp
                .read_tickets_get_readable(
                    &a.workspace,
                    &profile_id,
                    &a.ticket_id,
                    a.projection.as_deref(),
                )
                .map_err(err)
                .and_then(ser)
        }
    }

    #[tool(
        name = "tickets_list",
        description = "List tickets bounded, filtered, and ordered by most recent update, Lane order, or Board card order. Returns 25 compact summaries by default (hard cap 100) with an opaque continuation cursor. Filters: status, assignee, priority, type, label, policy label, first-class Lane membership, first-class Board membership, and dependency-ready. If Lane and Board are both supplied, membership is intersected and Lane order wins.",
        annotations(read_only_hint = true)
    )]
    fn tickets_list(&self, Parameters(a): Parameters<PTicketsList>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        let query = build_ticket_list_query(&self.mcp, &a).map_err(err)?;
        self.mcp
            .read_tickets_page(&a.workspace, &profile_id, query)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "tickets_board_get",
        description = "Read a first-class Ticket Board",
        annotations(read_only_hint = true)
    )]
    fn tickets_board_get(&self, Parameters(a): Parameters<PTicketsBoardGet>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_tickets_boards_get(&a.workspace, &profile_id, &a.board_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "tickets_board_list",
        description = "List first-class Ticket Boards",
        annotations(read_only_hint = true)
    )]
    fn tickets_board_list(&self, Parameters(a): Parameters<PTicketsBoardList>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_tickets_boards_list(&a.workspace, &profile_id, a.include_deleted)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "tickets_history",
        description = "Read ticket operation history",
        annotations(read_only_hint = true)
    )]
    fn tickets_history(&self, Parameters(a): Parameters<PTicketsHistory>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        if a.detailed {
            self.mcp
                .read_tickets_history(&a.workspace, &profile_id, a.ticket_id.as_deref())
                .map_err(err)
                .and_then(|items| slice_results(items, &a.page))
                .and_then(ser)
        } else {
            self.mcp
                .read_tickets_history_readable(&a.workspace, &profile_id, a.ticket_id.as_deref())
                .map_err(err)
                .and_then(|items| slice_results(items, &a.page))
                .and_then(ser)
        }
    }

    #[tool(
        name = "lanes_create",
        description = "Create a Lane coordination record. Treat tickets as the source of truth and Lane as coordination state."
    )]
    fn lanes_create(&self, Parameters(a): Parameters<PLanesCreate>) -> ToolResult {
        let lane_tickets = loom_lanes::lane_tickets_from_order(&a.ticket_ids).map_err(err)?;
        self.mcp
            .write_lanes_create_receipt(
                &a.workspace,
                LaneCreateRequest {
                    lane_id: &a.lane_id,
                    lane_key: &a.lane_key,
                    title: &a.title,
                    description: &a.description,
                    lane_kind: &a.lane_kind,
                    owner_principal: a.owner_principal.as_deref(),
                    lane_status: &a.lane_status,
                    lane_tickets: &lane_tickets,
                    active_ticket_id: a.active_ticket_id.as_deref(),
                    status_report: &a.status_report,
                    reviewer_feedback: &a.reviewer_feedback,
                    updated_by: a.updated_by.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser_public_lane_envelope)
    }

    #[tool(
        name = "lanes_get",
        description = "Read one Lane coordination record. Treat tickets as the source of truth and Lane as coordination state.",
        annotations(read_only_hint = true)
    )]
    fn lanes_get(&self, Parameters(a): Parameters<PLanesGet>) -> ToolResult {
        let view = self
            .mcp
            .read_lanes_get_view(&a.workspace, &a.lane_id)
            .map_err(err)?;
        if a.detailed {
            ser(view)
        } else {
            ser(view.map(|view| view.compact()))
        }
    }

    #[tool(
        name = "lanes_list",
        description = "List Lane coordination records. Treat tickets as the source of truth and Lane as coordination state.",
        annotations(read_only_hint = true)
    )]
    fn lanes_list(&self, Parameters(a): Parameters<PLanesList>) -> ToolResult {
        let (views, diagnostics) = self
            .mcp
            .read_lanes_list_views_with_diagnostics(&a.workspace)
            .map_err(err)?;
        let views = slice_results(views, &a.page)?;
        let lanes = if a.detailed {
            serde_json::to_value(views)
        } else {
            serde_json::to_value(
                views
                    .iter()
                    .map(loom_lanes::LaneView::compact)
                    .collect::<Vec<_>>(),
            )
        }
        .map_err(|e| err(LoomError::invalid(e.to_string())))?;
        let diagnostics = serde_json::to_value(diagnostics)
            .map_err(|e| err(LoomError::invalid(e.to_string())))?;
        ser(serde_json::json!({ "lanes": lanes, "diagnostics": diagnostics }))
    }

    #[tool(
        name = "lanes_update",
        description = "Atomically update one or more first-class Lane fields. Treat tickets as the source of truth and Lane as coordination state."
    )]
    fn lanes_update(&self, Parameters(a): Parameters<PLanesUpdate>) -> ToolResult {
        self.mcp
            .write_lanes_update_receipt(
                &a.workspace,
                LaneUpdateRequest {
                    lane_id: &a.lane_id,
                    title: a.title.as_deref(),
                    description: a.description.as_deref(),
                    lane_status: a.lane_status.as_deref(),
                    status_report: a.status_report.as_deref(),
                    reviewer_feedback: a.reviewer_feedback.as_deref(),
                    updated_by: a.updated_by.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser_public_lane_envelope)
    }

    #[tool(
        name = "lanes_ticket_add",
        description = "Add a ticket to Lane membership. Treat tickets as the source of truth and Lane as coordination state."
    )]
    fn lanes_ticket_add(&self, Parameters(a): Parameters<PLanesTicketAdd>) -> ToolResult {
        let placement =
            lane_ticket_placement(a.placement.as_deref(), a.anchor.as_deref()).map_err(err)?;
        self.mcp
            .write_lanes_ticket_add_receipt(
                &a.workspace,
                LaneTicketUpdateRequest {
                    lane_id: &a.lane_id,
                    ticket_id: &a.ticket_id,
                    placement,
                    updated_by: a.updated_by.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser_public_lane_envelope)
    }

    #[tool(
        name = "lanes_ticket_remove",
        description = "Remove a ticket from Lane membership. Treat tickets as the source of truth and Lane as coordination state."
    )]
    fn lanes_ticket_remove(&self, Parameters(a): Parameters<PLanesTicketRemove>) -> ToolResult {
        self.mcp
            .write_lanes_ticket_remove_receipt(
                &a.workspace,
                LaneTicketUpdateRequest {
                    lane_id: &a.lane_id,
                    ticket_id: &a.ticket_id,
                    placement: loom_lanes::LaneTicketPlacement::Append,
                    updated_by: a.updated_by.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser_public_lane_envelope)
    }

    #[tool(
        name = "lanes_ticket_transfer",
        description = "Transfer a ticket between assignment Lanes without mutating the ticket. Treat tickets as the source of truth and Lane as coordination state."
    )]
    fn lanes_ticket_transfer(&self, Parameters(a): Parameters<PLanesTicketTransfer>) -> ToolResult {
        self.mcp
            .write_lanes_ticket_transfer_receipt(
                &a.workspace,
                LaneTicketTransferRequest {
                    source_lane_id: &a.source_lane_id,
                    target_lane_id: &a.target_lane_id,
                    ticket_id: &a.ticket_id,
                    updated_by: &a.updated_by,
                },
            )
            .map_err(err)
            .and_then(ser_public_lane_envelope)
    }

    #[tool(
        name = "lanes_delete",
        description = "Delete a closed Lane tombstone without mutating tickets. Treat tickets as the source of truth and Lane as coordination state."
    )]
    fn lanes_delete(&self, Parameters(a): Parameters<PLanesDelete>) -> ToolResult {
        self.mcp
            .write_lanes_delete_receipt(
                &a.workspace,
                LaneDeleteRequest {
                    lane_id: &a.lane_id,
                    updated_by: &a.updated_by,
                },
            )
            .map_err(err)
            .and_then(ser_public_lane_envelope)
    }

    // ===== spaces and pages =====
    #[tool(
        name = "spaces_create",
        description = "Create a Studio space in a workspace"
    )]
    fn spaces_create(&self, Parameters(a): Parameters<PSpacesCreate>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_spaces_create(
                &a.workspace,
                &profile_id,
                &a.space_id,
                &a.title,
                a.expected_root.as_deref(),
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "spaces_get",
        description = "Read a Studio space",
        annotations(read_only_hint = true)
    )]
    fn spaces_get(&self, Parameters(a): Parameters<PSpacesGet>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_spaces_get(&a.workspace, &profile_id, &a.space_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "spaces_list",
        description = "List Studio spaces in a workspace",
        annotations(read_only_hint = true)
    )]
    fn spaces_list(&self, Parameters(a): Parameters<PSpacesList>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_spaces_list(&a.workspace, &profile_id)
            .map_err(err)
            .and_then(|items| slice_results(items, &a.page))
            .and_then(ser)
    }

    #[tool(name = "pages_create", description = "Create a page in a Studio space")]
    fn pages_create(&self, Parameters(a): Parameters<PPagesCreate>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_pages_create(
                &a.workspace,
                PageCreateRequest {
                    workspace_id: &profile_id,
                    page_id: &a.page_id,
                    space_id: &a.space_id,
                    parent_page_id: a.parent_page_id.as_deref(),
                    title: &a.title,
                    expected_root: a.expected_root.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(name = "pages_update", description = "Update a page working state")]
    fn pages_update(&self, Parameters(a): Parameters<PPagesUpdate>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_pages_update_text(
                &a.workspace,
                &profile_id,
                &a.page_id,
                &a.body_text,
                a.expected_root.as_deref(),
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(name = "pages_publish", description = "Publish a page working state")]
    fn pages_publish(&self, Parameters(a): Parameters<PPagesPublish>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_pages_publish(
                &a.workspace,
                &profile_id,
                &a.page_id,
                a.expected_root.as_deref(),
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "pages_get",
        description = "Read a page and caller working state",
        annotations(read_only_hint = true)
    )]
    fn pages_get(&self, Parameters(a): Parameters<PPagesGet>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_pages_get(&a.workspace, &profile_id, &a.page_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "pages_list",
        description = "List pages in a workspace",
        annotations(read_only_hint = true)
    )]
    fn pages_list(&self, Parameters(a): Parameters<PSpacesList>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_pages_list(&a.workspace, &profile_id)
            .map_err(err)
            .and_then(|items| slice_results(items, &a.page))
            .and_then(ser)
    }

    #[tool(
        name = "pages_history",
        description = "Read page revision and conflict history",
        annotations(read_only_hint = true)
    )]
    fn pages_history(&self, Parameters(a): Parameters<PPagesGet>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_pages_history(&a.workspace, &profile_id, &a.page_id)
            .map_err(err)
            .and_then(|items| slice_results(items, &a.page))
            .and_then(ser)
    }

    #[tool(
        name = "lifecycles_define",
        description = "Define a Studio lifecycle from canonical bytes"
    )]
    fn lifecycles_define(&self, Parameters(a): Parameters<PLifecyclesDefine>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_lifecycles_define(&a.workspace, &profile_id, &a.definition_cbor)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "lifecycles_define_standard",
        description = "Define a built-in Studio lifecycle"
    )]
    fn lifecycles_define_standard(
        &self,
        Parameters(a): Parameters<PLifecyclesDefineStandard>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_lifecycles_define_standard(
                &a.workspace,
                loom_lifecycle::StandardLifecycleRequest {
                    workspace_id: &profile_id,
                    kind: &a.kind,
                    version: &a.version,
                    completion_predicate_digest: &a.completion_predicate_digest,
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "lifecycles_definitions",
        description = "List Studio lifecycle definitions",
        annotations(read_only_hint = true)
    )]
    fn lifecycles_definitions(
        &self,
        Parameters(a): Parameters<PLifecyclesWorkspace>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_lifecycles_definitions(&a.workspace, &profile_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "lifecycles_definition",
        description = "Read a Studio lifecycle definition",
        annotations(read_only_hint = true)
    )]
    fn lifecycles_definition(
        &self,
        Parameters(a): Parameters<PLifecyclesDefinition>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_lifecycles_definition(&a.workspace, &profile_id, &a.definition_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "lifecycles_instantiate",
        description = "Create a Studio lifecycle instance"
    )]
    fn lifecycles_instantiate(
        &self,
        Parameters(a): Parameters<PLifecyclesInstantiate>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_lifecycles_instantiate(
                &a.workspace,
                &profile_id,
                &a.instance_id,
                &a.definition_id,
                a.subject_refs,
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "lifecycles_instances",
        description = "List Studio lifecycle instances",
        annotations(read_only_hint = true)
    )]
    fn lifecycles_instances(&self, Parameters(a): Parameters<PLifecyclesWorkspace>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_lifecycles_instances(&a.workspace, &profile_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "lifecycles_instance",
        description = "Read a Studio lifecycle instance",
        annotations(read_only_hint = true)
    )]
    fn lifecycles_instance(&self, Parameters(a): Parameters<PLifecyclesInstance>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_lifecycles_instance(&a.workspace, &profile_id, &a.instance_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "lifecycles_active_set",
        description = "Set this MCP session's active lifecycle instance for stage-scoped tool surfacing"
    )]
    fn lifecycles_active_set(&self, Parameters(a): Parameters<PLifecyclesActiveSet>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        let surface = self
            .mcp
            .read_lifecycles_current_surface(&a.workspace, &profile_id, &a.instance_id)
            .map_err(err)?;
        let context = ActiveLifecycleContext {
            workspace: a.workspace,
            workspace_id: profile_id,
            instance_id: a.instance_id,
        };
        *self.active_lifecycle.lock().expect("active lifecycle lock") = Some(context.clone());
        Ok(out_value(json!({
            "active": true,
            "workspace": context.workspace,
            "workspace_id": context.workspace_id,
            "instance_id": context.instance_id,
            "surface": surface
        })))
    }

    #[tool(
        name = "lifecycles_active_clear",
        description = "Clear this MCP session's active lifecycle tool-surfacing context"
    )]
    fn lifecycles_active_clear(&self) -> ToolResult {
        *self.active_lifecycle.lock().expect("active lifecycle lock") = None;
        Ok(out_value(json!({
            "active": false,
            "workspace": null,
            "workspace_id": null,
            "instance_id": null,
            "surface": null
        })))
    }

    #[tool(
        name = "lifecycles_snapshot_plan",
        description = "Plan the snapshot needed for a lifecycle transition",
        annotations(read_only_hint = true)
    )]
    fn lifecycles_snapshot_plan(
        &self,
        Parameters(a): Parameters<PLifecyclesSnapshotPlan>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_lifecycles_snapshot_plan(
                &a.workspace,
                &profile_id,
                &a.instance_id,
                &a.to_stage_id,
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "lifecycles_current_surface",
        description = "Read the active tool and prompt surface for a lifecycle instance",
        annotations(read_only_hint = true)
    )]
    fn lifecycles_current_surface(
        &self,
        Parameters(a): Parameters<PLifecyclesInstance>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_lifecycles_current_surface(&a.workspace, &profile_id, &a.instance_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "lifecycles_transition",
        description = "Advance a Studio lifecycle instance"
    )]
    fn lifecycles_transition(
        &self,
        Parameters(a): Parameters<PLifecyclesTransition>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        let gate_evaluations = a
            .gate_evaluations
            .into_iter()
            .map(|gate| loom_lifecycle::LifecycleGateEvaluationInput {
                gate_id: gate.gate_id,
                passed: gate.passed,
                principal_id: gate.principal_id,
                evidence_digest: gate.evidence_digest,
                evaluated_at_ms: gate.evaluated_at_ms,
            })
            .collect();
        self.mcp
            .write_lifecycles_transition(
                &a.workspace,
                loom_lifecycle::LifecycleTransitionRequest {
                    workspace_id: &profile_id,
                    instance_id: &a.instance_id,
                    transition_id: &a.transition_id,
                    to_stage_id: &a.to_stage_id,
                    actor_principal_id: &a.actor_principal_id,
                    gate_evaluations,
                    snapshot_digest: a.snapshot_digest.as_deref(),
                    recorded_at_ms: a.recorded_at_ms,
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "lifecycles_snapshots",
        description = "List lifecycle transition snapshots",
        annotations(read_only_hint = true)
    )]
    fn lifecycles_snapshots(&self, Parameters(a): Parameters<PLifecyclesWorkspace>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_lifecycles_snapshots(&a.workspace, &profile_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "lifecycles_snapshot",
        description = "Read a lifecycle transition snapshot",
        annotations(read_only_hint = true)
    )]
    fn lifecycles_snapshot(&self, Parameters(a): Parameters<PLifecyclesSnapshot>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_lifecycles_snapshot(&a.workspace, &profile_id, &a.snapshot_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "lifecycles_snapshot_content",
        description = "Read stored lifecycle snapshot content",
        annotations(read_only_hint = true)
    )]
    fn lifecycles_snapshot_content(
        &self,
        Parameters(a): Parameters<PLifecyclesSnapshot>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_lifecycles_snapshot_content(&a.workspace, &profile_id, &a.snapshot_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "lifecycles_operation_log",
        description = "Read lifecycle operation history",
        annotations(read_only_hint = true)
    )]
    fn lifecycles_operation_log(
        &self,
        Parameters(a): Parameters<PLifecyclesWorkspace>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_lifecycles_operation_log(&a.workspace, &profile_id)
            .map_err(err)
            .and_then(ser)
    }

    fn chat_presence_set(
        &self,
        workspace: &str,
        workspace_id: &str,
        channel_id: &str,
        status: &str,
        ttl_ms: u64,
    ) -> Result<ChatPresenceSummary, LoomError> {
        if status.is_empty() || status.chars().any(char::is_control) {
            return Err(LoomError::invalid("invalid chat presence status"));
        }
        if ttl_ms == 0 || ttl_ms > 300_000 {
            return Err(LoomError::invalid(
                "chat presence ttl must be between 1 and 300000 ms",
            ));
        }
        let now = crate::now_ms();
        self.mcp.store().read(|loom| {
            let ns = crate::reads::resolve_ns(loom, workspace)?;
            loom.authorize_domain(ns, AclDomain::Chat, AclRight::Read)?;
            let channel_id = crate::chat::resolve_channel_id(loom, ns, workspace_id, channel_id)?;
            let principal = loom.effective_principal()?.unwrap_or(ns).to_string();
            let summary = ChatPresenceSummary {
                workspace_id: workspace_id.to_string(),
                channel_id: channel_id.clone(),
                principal: principal.clone(),
                status: status.to_string(),
                expires_at_ms: now.saturating_add(ttl_ms),
            };
            let key = PresenceKey {
                workspace: ns,
                workspace_id: workspace_id.to_string(),
                channel_id,
                principal,
            };
            let mut presence = self.chat_presence.lock().expect("chat presence lock");
            presence.retain(|_, value| value.expires_at_ms > now);
            presence.insert(key, summary.clone());
            Ok(summary)
        })
    }

    fn chat_presence_list(
        &self,
        workspace: &str,
        workspace_id: &str,
        channel_id: &str,
    ) -> Result<Vec<ChatPresenceSummary>, LoomError> {
        let now = crate::now_ms();
        self.mcp.store().read(|loom| {
            let ns = crate::reads::resolve_ns(loom, workspace)?;
            loom.authorize_domain(ns, AclDomain::Chat, AclRight::Read)?;
            let channel_id = crate::chat::resolve_channel_id(loom, ns, workspace_id, channel_id)?;
            let mut presence = self.chat_presence.lock().expect("chat presence lock");
            presence.retain(|_, value| value.expires_at_ms > now);
            Ok(presence
                .iter()
                .filter(|(key, _)| {
                    key.workspace == ns
                        && key.workspace_id == workspace_id
                        && key.channel_id == channel_id
                })
                .map(|(_, value)| value.clone())
                .collect())
        })
    }

    #[tool(
        name = "chat_fetch_events",
        description = "Read sequenced chat operation events",
        annotations(read_only_hint = true)
    )]
    fn chat_fetch_events(&self, Parameters(a): Parameters<PChatFetchEvents>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_chat_fetch_events(
                &a.workspace,
                &profile_id,
                &a.channel_id,
                a.from_sequence,
                a.max as usize,
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "chat_channels",
        description = "List chat channels in a workspace",
        annotations(read_only_hint = true)
    )]
    fn chat_channels(&self, Parameters(a): Parameters<PChatWorkspace>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_chat_channels(&a.workspace, &profile_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(name = "chat_create_channel", description = "Create a chat channel")]
    fn chat_create_channel(&self, Parameters(a): Parameters<PChatCreateChannel>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_chat_create_channel(&a.workspace, &profile_id, &a.handle, &a.name)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "chat_rename_channel",
        description = "Rename a chat channel handle"
    )]
    fn chat_rename_channel(&self, Parameters(a): Parameters<PChatRenameChannel>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_chat_rename_channel(&a.workspace, &profile_id, &a.channel_id, &a.handle)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "chat_messages",
        description = "Read projected chat messages and threads",
        annotations(read_only_hint = true)
    )]
    fn chat_messages(&self, Parameters(a): Parameters<PChatChannel>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_chat_messages(&a.workspace, &profile_id, &a.channel_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "chat_presence",
        description = "Read live ephemeral chat presence",
        annotations(read_only_hint = true)
    )]
    fn chat_presence(&self, Parameters(a): Parameters<PChatChannel>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.chat_presence_list(&a.workspace, &profile_id, &a.channel_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "chat_cursor",
        description = "Read the principal's durable chat cursor",
        annotations(read_only_hint = true)
    )]
    fn chat_cursor(&self, Parameters(a): Parameters<PChatChannel>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_chat_cursor(&a.workspace, &profile_id, &a.channel_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "chat_set_presence",
        description = "Set the principal's ephemeral chat presence"
    )]
    fn chat_set_presence(&self, Parameters(a): Parameters<PChatSetPresence>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.chat_presence_set(
            &a.workspace,
            &profile_id,
            &a.channel_id,
            &a.status,
            a.ttl_ms,
        )
        .map_err(err)
        .and_then(ser)
    }

    #[tool(name = "chat_post_message", description = "Append a chat message")]
    fn chat_post_message(&self, Parameters(a): Parameters<PChatPostMessage>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_chat_post_message(
                &a.workspace,
                &profile_id,
                &a.channel_id,
                &a.message_id,
                a.thread_id.as_deref(),
                a.body_text.into_bytes(),
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(name = "chat_edit_message", description = "Append a chat message edit")]
    fn chat_edit_message(&self, Parameters(a): Parameters<PChatEditMessage>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_chat_edit_message(
                &a.workspace,
                &profile_id,
                &a.channel_id,
                &a.message_id,
                a.body_text.into_bytes(),
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "chat_redact_message",
        description = "Append a chat message redaction"
    )]
    fn chat_redact_message(&self, Parameters(a): Parameters<PChatRedactMessage>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_chat_redact_message(
                &a.workspace,
                &profile_id,
                &a.channel_id,
                &a.message_id,
                a.reason.as_deref(),
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "chat_emoji_list",
        description = "List custom chat emoji registered for a workspace",
        annotations(read_only_hint = true)
    )]
    fn chat_emoji_list(&self, Parameters(a): Parameters<PChatWorkspace>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_chat_emoji_registry(&a.workspace, &profile_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "chat_emoji_register",
        description = "Register a custom chat emoji kind"
    )]
    fn chat_emoji_register(&self, Parameters(a): Parameters<PChatEmoji>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_chat_emoji_register(&a.workspace, &profile_id, &a.kind)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "chat_emoji_unregister",
        description = "Unregister a custom chat emoji kind"
    )]
    fn chat_emoji_unregister(&self, Parameters(a): Parameters<PChatEmoji>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_chat_emoji_unregister(&a.workspace, &profile_id, &a.kind)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(name = "chat_add_reaction", description = "Append a chat reaction")]
    fn chat_add_reaction(&self, Parameters(a): Parameters<PChatReaction>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_chat_add_reaction(
                &a.workspace,
                &profile_id,
                &a.channel_id,
                &a.message_id,
                &a.kind,
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "chat_remove_reaction",
        description = "Append a chat reaction removal"
    )]
    fn chat_remove_reaction(&self, Parameters(a): Parameters<PChatReaction>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_chat_remove_reaction(
                &a.workspace,
                &profile_id,
                &a.channel_id,
                &a.message_id,
                &a.kind,
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(name = "chat_create_thread", description = "Append a chat thread")]
    fn chat_create_thread(&self, Parameters(a): Parameters<PChatCreateThread>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_chat_create_thread(
                &a.workspace,
                &profile_id,
                &a.channel_id,
                &a.thread_id,
                &a.parent_message_id,
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(name = "chat_create_task", description = "Append a chat task")]
    fn chat_create_task(&self, Parameters(a): Parameters<PChatCreateTask>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_chat_create_task(
                &a.workspace,
                &profile_id,
                &a.channel_id,
                &a.task_id,
                a.message_id.as_deref(),
                &a.title,
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(name = "chat_claim_task", description = "Claim a chat task")]
    fn chat_claim_task(&self, Parameters(a): Parameters<PChatClaimTask>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_chat_claim_task(
                &a.workspace,
                &profile_id,
                &a.channel_id,
                &a.task_id,
                &a.claim_id,
                a.lease_token.as_deref(),
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(name = "chat_complete_task", description = "Complete a chat task")]
    fn chat_complete_task(&self, Parameters(a): Parameters<PChatCompleteTask>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_chat_complete_task(
                &a.workspace,
                &profile_id,
                &a.channel_id,
                &a.task_id,
                &a.claim_id,
                a.result_message_id.as_deref(),
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "chat_invoke_agent",
        description = "Invite an agent principal into a chat conversation"
    )]
    fn chat_invoke_agent(&self, Parameters(a): Parameters<PChatInvokeAgent>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_chat_invoke_agent(
                &a.workspace,
                &profile_id,
                &a.channel_id,
                &a.invocation_id,
                &a.agent_principal,
                a.source_message_ids,
                a.prompt_text.into_bytes(),
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "chat_agent_reply",
        description = "Link an agent participant reply"
    )]
    fn chat_agent_reply(&self, Parameters(a): Parameters<PChatAgentReply>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_chat_agent_reply(
                &a.workspace,
                &profile_id,
                &a.channel_id,
                &a.invocation_id,
                &a.message_id,
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "chat_request_handoff",
        description = "Request participant handoff"
    )]
    fn chat_request_handoff(&self, Parameters(a): Parameters<PChatRequestHandoff>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_chat_request_handoff(
                &a.workspace,
                &profile_id,
                &a.channel_id,
                &a.handoff_id,
                &a.from_agent_principal,
                a.to_principal.as_deref(),
                a.reason.as_deref(),
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "chat_update_cursor",
        description = "Advance the principal's durable chat cursor"
    )]
    fn chat_update_cursor(&self, Parameters(a): Parameters<PChatUpdateCursor>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_chat_update_cursor(&a.workspace, &profile_id, &a.channel_id, a.next_sequence)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "drive_list",
        description = "List a Drive folder",
        annotations(read_only_hint = true)
    )]
    fn drive_list(&self, Parameters(a): Parameters<PDriveFolder>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_drive_list(&a.workspace, &profile_id, &a.folder_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "drive_stat",
        description = "Read Drive entry metadata",
        annotations(read_only_hint = true)
    )]
    fn drive_stat(&self, Parameters(a): Parameters<PDriveStat>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_drive_stat(&a.workspace, &profile_id, &a.folder_id, &a.name)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "drive_read",
        description = "Read Drive file bytes",
        annotations(read_only_hint = true)
    )]
    fn drive_read(&self, Parameters(a): Parameters<PDriveFile>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_drive_read(&a.workspace, &profile_id, &a.file_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "drive_list_versions",
        description = "List Drive file versions",
        annotations(read_only_hint = true)
    )]
    fn drive_list_versions(&self, Parameters(a): Parameters<PDriveFile>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_drive_list_versions(&a.workspace, &profile_id, &a.file_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "drive_list_conflicts",
        description = "List unresolved Drive conflict records",
        annotations(read_only_hint = true)
    )]
    fn drive_list_conflicts(&self, Parameters(a): Parameters<PDriveConflicts>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_drive_list_conflicts(&a.workspace, &profile_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "drive_list_shares",
        description = "List Drive share grants",
        annotations(read_only_hint = true)
    )]
    fn drive_list_shares(&self, Parameters(a): Parameters<PDriveConflicts>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_drive_list_shares(&a.workspace, &profile_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "meetings_projection_outputs",
        description = "Derive Meetings projection outputs",
        annotations(read_only_hint = true)
    )]
    fn meetings_projection_outputs(
        &self,
        Parameters(a): Parameters<PMeetingsProfile>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_meetings_projection_outputs(&a.workspace, &profile_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "meetings_list",
        description = "List Meetings records in a workspace",
        annotations(read_only_hint = true)
    )]
    fn meetings_list(&self, Parameters(a): Parameters<PMeetingsList>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_meetings_list(
                &a.workspace,
                &profile_id,
                a.limit.unwrap_or(100),
                a.offset.unwrap_or(0),
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "meetings_get",
        description = "Get one Meetings record",
        annotations(read_only_hint = true)
    )]
    fn meetings_get(&self, Parameters(a): Parameters<PMeetingsGet>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_meetings_get(&a.workspace, &profile_id, &a.meeting_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "meetings_search",
        description = "Search materialized Meetings projection text",
        annotations(read_only_hint = true)
    )]
    fn meetings_search(&self, Parameters(a): Parameters<PMeetingsSearch>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_meetings_search(
                &a.workspace,
                &profile_id,
                &a.query,
                a.field.as_deref(),
                a.limit.unwrap_or(20),
                a.offset.unwrap_or(0),
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "meetings_extraction_review",
        description = "Derive Meetings annotation review buckets",
        annotations(read_only_hint = true)
    )]
    fn meetings_extraction_review(
        &self,
        Parameters(a): Parameters<PMeetingsProfile>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_meetings_extraction_review(&a.workspace, &profile_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "meetings_accept_annotation",
        description = "Accept a Meetings annotation"
    )]
    fn meetings_accept_annotation(
        &self,
        Parameters(a): Parameters<PMeetingsAnnotation>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_meetings_accept_annotation(&a.workspace, &profile_id, &a.annotation_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "meetings_reject_annotation",
        description = "Reject a Meetings annotation"
    )]
    fn meetings_reject_annotation(
        &self,
        Parameters(a): Parameters<PMeetingsAnnotation>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_meetings_reject_annotation(&a.workspace, &profile_id, &a.annotation_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "meetings_propose_vocabulary",
        description = "Propose a Meetings vocabulary term"
    )]
    fn meetings_propose_vocabulary(
        &self,
        Parameters(a): Parameters<PMeetingsVocabularyPropose>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_meetings_propose_vocabulary(
                &a.workspace,
                &profile_id,
                loom_substrate::meetings::VocabularyTermInput {
                    term_id: &a.term_id,
                    kind: &a.kind,
                    label: &a.label,
                    evidence_annotation_ids: a.evidence_annotation_ids,
                    created_at_ms: crate::now_ms(),
                },
                a.aliases.unwrap_or_default(),
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "meetings_accept_vocabulary",
        description = "Accept a Meetings vocabulary term"
    )]
    fn meetings_accept_vocabulary(
        &self,
        Parameters(a): Parameters<PMeetingsVocabulary>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_meetings_accept_vocabulary(&a.workspace, &profile_id, &a.term_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "meetings_reject_vocabulary",
        description = "Reject a Meetings vocabulary term"
    )]
    fn meetings_reject_vocabulary(
        &self,
        Parameters(a): Parameters<PMeetingsVocabulary>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_meetings_reject_vocabulary(&a.workspace, &profile_id, &a.term_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "meetings_add_entity_merge",
        description = "Add a Meetings entity merge decision"
    )]
    fn meetings_add_entity_merge(
        &self,
        Parameters(a): Parameters<PMeetingsEntityMerge>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_meetings_add_entity_merge(
                &a.workspace,
                &profile_id,
                &a.merge_id,
                &a.canonical_entity_id,
                a.merged_entity_ids,
                a.evidence_annotation_ids,
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "meetings_add_promotion",
        description = "Record a Meetings cross-profile promotion"
    )]
    fn meetings_add_promotion(&self, Parameters(a): Parameters<PMeetingsPromotion>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_meetings_add_promotion(
                &a.workspace,
                &profile_id,
                &a.promotion_id,
                &a.operation_kind,
                &a.source_annotation_id,
                &a.target_profile,
                &a.target_entity_ref,
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "meetings_promote_task_to_ticket",
        description = "Promote a Meetings task annotation into a ticket"
    )]
    fn meetings_promote_task_to_ticket(
        &self,
        Parameters(a): Parameters<PMeetingsPromoteTaskToTicket>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_meetings_promote_task_to_ticket(
                &a.workspace,
                &profile_id,
                MeetingsPromoteTaskToTicketRequest {
                    promotion_id: &a.promotion_id,
                    source_annotation_id: &a.source_annotation_id,
                    project_id: &a.project_id,
                    ticket_type: &a.ticket_type,
                    policy_labels: &a.policy_labels,
                    expected_ticket_root: a.expected_ticket_root.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "meetings_promote_decision_to_decision_log",
        description = "Promote a Meetings decision annotation into the decision log"
    )]
    fn meetings_promote_decision_to_decision_log(
        &self,
        Parameters(a): Parameters<PMeetingsPromoteDecisionToDecisionLog>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_meetings_promote_decision_to_decision_log(
                &a.workspace,
                &profile_id,
                MeetingsPromoteDecisionToDecisionLogRequest {
                    promotion_id: &a.promotion_id,
                    source_annotation_id: &a.source_annotation_id,
                    decision_id: &a.decision_id,
                    ledger_name: &a.ledger_name,
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "meetings_promote_question_to_lifecycle",
        description = "Promote a Meetings question annotation into a lifecycle instance"
    )]
    fn meetings_promote_question_to_lifecycle(
        &self,
        Parameters(a): Parameters<PMeetingsPromoteQuestionToLifecycle>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_meetings_promote_question_to_lifecycle(
                &a.workspace,
                &profile_id,
                MeetingsPromoteQuestionToLifecycleRequest {
                    promotion_id: &a.promotion_id,
                    source_annotation_id: &a.source_annotation_id,
                    instance_id: &a.instance_id,
                    definition_id: &a.definition_id,
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "meetings_promote_artifact_to_reference_artifact",
        description = "Promote a Meetings artifact annotation into a reference artifact"
    )]
    fn meetings_promote_artifact_to_reference_artifact(
        &self,
        Parameters(a): Parameters<PMeetingsPromoteArtifactToReferenceArtifact>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_meetings_promote_artifact_to_reference_artifact(
                &a.workspace,
                &profile_id,
                MeetingsPromoteArtifactToReferenceArtifactRequest {
                    promotion_id: &a.promotion_id,
                    source_annotation_id: &a.source_annotation_id,
                    artifact_id: &a.artifact_id,
                    target_ref: a.target_ref.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "meetings_promote_reference_to_reference_artifact",
        description = "Promote a Meetings reference annotation into a reference artifact"
    )]
    fn meetings_promote_reference_to_reference_artifact(
        &self,
        Parameters(a): Parameters<PMeetingsPromoteReferenceToReferenceArtifact>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_meetings_promote_reference_to_reference_artifact(
                &a.workspace,
                &profile_id,
                MeetingsPromoteReferenceToReferenceArtifactRequest {
                    promotion_id: &a.promotion_id,
                    source_annotation_id: &a.source_annotation_id,
                    reference_id: &a.reference_id,
                    target_ref: a.target_ref.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "meetings_import_snapshot",
        description = "Import a normalized Meetings snapshot"
    )]
    fn meetings_import_snapshot(
        &self,
        Parameters(a): Parameters<PMeetingsImportSnapshot>,
    ) -> ToolResult {
        self.mcp
            .write_meetings_import_snapshot(
                &a.workspace,
                &a.input_profile,
                &a.snapshot,
                a.dry_run.unwrap_or(false),
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "redmine_import_snapshot",
        description = "Import a normalized Redmine snapshot"
    )]
    fn redmine_import_snapshot(
        &self,
        Parameters(a): Parameters<PRedmineImportSnapshot>,
    ) -> ToolResult {
        self.mcp
            .write_redmine_import_snapshot(
                &a.workspace,
                &a.profile,
                a.source_path.as_deref(),
                &a.snapshot,
                a.field_policy.as_deref(),
                a.dry_run.unwrap_or(false),
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "studio_reindex",
        description = "Rebuild derived Studio indexes and projections"
    )]
    fn studio_reindex(&self, Parameters(a): Parameters<PStudioReindex>) -> ToolResult {
        self.mcp
            .write_studio_reindex(&a.workspace, a.profile.as_deref())
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "import_submit_batch",
        description = "Persist a normalized import batch"
    )]
    fn import_submit_batch(&self, Parameters(a): Parameters<PImportSubmitBatch>) -> ToolResult {
        self.mcp
            .write_import_submit_batch(&a.workspace, &a.batch)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "import_execute_batch",
        description = "Execute a normalized import batch"
    )]
    fn import_execute_batch(&self, Parameters(a): Parameters<PImportExecuteBatch>) -> ToolResult {
        self.mcp
            .write_import_execute_batch(&a.workspace, &a.batch, a.dry_run.unwrap_or(false))
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "drive_grant_share",
        description = "Grant Drive access metadata"
    )]
    fn drive_grant_share(&self, Parameters(a): Parameters<PDriveShareGrant>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_drive_grant_share(
                &a.workspace,
                crate::drive::DriveGrantShareRequest {
                    workspace_id: &profile_id,
                    grant_id: &a.grant_id,
                    target_kind: &a.target_kind,
                    target_id: &a.target_id,
                    principal: &a.principal,
                    role: &a.role,
                    granted_at_ms: crate::now_ms(),
                    expires_at_ms: a.expires_at_ms,
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "drive_revoke_share",
        description = "Revoke a Drive share grant"
    )]
    fn drive_revoke_share(&self, Parameters(a): Parameters<PDriveShareRevoke>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_drive_revoke_share(&a.workspace, &profile_id, &a.grant_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "drive_apply_share_expiry",
        description = "Apply expired Drive share grants"
    )]
    fn drive_apply_share_expiry(
        &self,
        Parameters(a): Parameters<PDriveShareExpiryApply>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_drive_apply_share_expiry(&a.workspace, &profile_id, a.now_ms)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "drive_list_retention",
        description = "List Drive retention pins",
        annotations(read_only_hint = true)
    )]
    fn drive_list_retention(&self, Parameters(a): Parameters<PDriveConflicts>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_drive_list_retention(&a.workspace, &profile_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "drive_pin_retention",
        description = "Pin a Drive root for retention"
    )]
    fn drive_pin_retention(&self, Parameters(a): Parameters<PDriveRetentionPin>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_drive_pin_retention(
                &a.workspace,
                crate::drive::DrivePinRetentionRequest {
                    workspace_id: &profile_id,
                    pin_id: &a.pin_id,
                    kind: &a.kind,
                    root: &a.root,
                    target_entity_id: a.target_entity_id.as_deref(),
                    added_at_ms: crate::now_ms(),
                    expires_at_ms: a.expires_at_ms,
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "drive_unpin_retention",
        description = "Remove a Drive retention pin"
    )]
    fn drive_unpin_retention(&self, Parameters(a): Parameters<PDriveRetentionUnpin>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_drive_unpin_retention(&a.workspace, &profile_id, &a.pin_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "drive_apply_retention",
        description = "Apply expired Drive retention pins"
    )]
    fn drive_apply_retention(&self, Parameters(a): Parameters<PDriveRetentionApply>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_drive_apply_retention(&a.workspace, &profile_id, a.now_ms)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "drive_acquire_lease",
        description = "Acquire a Drive write-intent lease backed by the shared lock coordinator"
    )]
    fn drive_acquire_lease(&self, Parameters(a): Parameters<PDriveAcquireLease>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_drive_acquire_lease(
                &a.workspace,
                &profile_id,
                &a.target_kind,
                &a.target_id,
                a.lease_ms,
                a.wait_ms,
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "drive_refresh_lease",
        description = "Refresh a Drive write-intent lease"
    )]
    fn drive_refresh_lease(&self, Parameters(a): Parameters<PDriveRefreshLease>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_drive_refresh_lease(
                &a.workspace,
                &profile_id,
                &a.target_kind,
                &a.target_id,
                fence(a.fence),
                a.lease_ms,
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "drive_release_lease",
        description = "Release a Drive write-intent lease"
    )]
    fn drive_release_lease(&self, Parameters(a): Parameters<PDriveReleaseLease>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_drive_release_lease(
                &a.workspace,
                &profile_id,
                &a.target_kind,
                &a.target_id,
                fence(a.fence),
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "drive_break_lease",
        description = "Admin-break a Drive write-intent lease for a target"
    )]
    fn drive_break_lease(&self, Parameters(a): Parameters<PDriveBreakLease>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_drive_break_lease(&a.workspace, &profile_id, &a.target_kind, &a.target_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "drive_create_folder",
        description = "Create a Drive folder with expected-root concurrency control"
    )]
    fn drive_create_folder(&self, Parameters(a): Parameters<PDriveCreateFolder>) -> ToolResult {
        let admission = write_admission(a.write_admission);
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_drive_create_folder(
                &a.workspace,
                &profile_id,
                &a.parent_folder_id,
                &a.folder_id,
                &a.name,
                &a.expected_root,
                admission.as_ref(),
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "drive_create_upload",
        description = "Create a durable Drive upload session"
    )]
    fn drive_create_upload(&self, Parameters(a): Parameters<PDriveCreateUpload>) -> ToolResult {
        let admission = write_admission(a.write_admission);
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_drive_create_upload(
                &a.workspace,
                crate::drive::DriveCreateUploadRequest {
                    workspace_id: &profile_id,
                    upload_id: &a.upload_id,
                    parent_folder_id: &a.parent_folder_id,
                    name: &a.name,
                    file_id: &a.file_id,
                    expected_root: &a.expected_root,
                    created_at_ms: crate::now_ms(),
                    replace_file: a.replace_file,
                },
                admission.as_ref(),
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "drive_upload_chunk",
        description = "Append bytes to a durable Drive upload session"
    )]
    fn drive_upload_chunk(&self, Parameters(a): Parameters<PDriveUploadChunk>) -> ToolResult {
        let admission = write_admission(a.write_admission);
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_drive_upload_chunk(
                &a.workspace,
                &profile_id,
                &a.upload_id,
                &a.bytes,
                admission.as_ref(),
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "drive_commit_upload",
        description = "Commit a Drive upload session"
    )]
    fn drive_commit_upload(&self, Parameters(a): Parameters<PDriveCommitUpload>) -> ToolResult {
        let admission = write_admission(a.write_admission);
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_drive_commit_upload(&a.workspace, &profile_id, &a.upload_id, admission.as_ref())
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "drive_rename",
        description = "Rename a Drive entry with expected-root concurrency control"
    )]
    fn drive_rename(&self, Parameters(a): Parameters<PDriveRename>) -> ToolResult {
        let admission = write_admission(a.write_admission);
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_drive_rename(
                &a.workspace,
                &profile_id,
                &a.folder_id,
                &a.node_id,
                &a.new_name,
                &a.expected_root,
                admission.as_ref(),
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "drive_move",
        description = "Move a Drive entry with expected-root concurrency control"
    )]
    fn drive_move(&self, Parameters(a): Parameters<PDriveMove>) -> ToolResult {
        let admission = write_admission(a.write_admission);
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_drive_move(
                &a.workspace,
                &profile_id,
                &a.source_folder_id,
                &a.target_folder_id,
                &a.node_id,
                &a.expected_root,
                admission.as_ref(),
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "drive_delete",
        description = "Delete a Drive entry with expected-root concurrency control"
    )]
    fn drive_delete(&self, Parameters(a): Parameters<PDriveDelete>) -> ToolResult {
        let admission = write_admission(a.write_admission);
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_drive_delete(
                &a.workspace,
                &profile_id,
                &a.folder_id,
                &a.node_id,
                &a.expected_root,
                admission.as_ref(),
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "drive_resolve_conflict",
        description = "Resolve a Drive conflict record"
    )]
    fn drive_resolve_conflict(
        &self,
        Parameters(a): Parameters<PDriveResolveConflict>,
    ) -> ToolResult {
        let resolution = drive_conflict_resolution(&a.resolution).map_err(err)?;
        let admission = write_admission(a.write_admission);
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_drive_resolve_conflict(
                &a.workspace,
                &profile_id,
                &a.conflict_id,
                resolution,
                admission.as_ref(),
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "structures_create",
        description = "Create a Studio structure in a space"
    )]
    fn structures_create(&self, Parameters(a): Parameters<PStructuresCreate>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_structures_create(
                &a.workspace,
                StructureCreateRequest {
                    workspace_id: &profile_id,
                    structure_id: &a.structure_id,
                    space_id: &a.space_id,
                    kind: &a.kind,
                    title: &a.title,
                    expected_root: a.expected_root.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "structures_get",
        description = "Read a Studio structure render projection",
        annotations(read_only_hint = true)
    )]
    fn structures_get(&self, Parameters(a): Parameters<PStructuresGet>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_structures_get(&a.workspace, &profile_id, &a.structure_id)
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "structures_list",
        description = "List Studio structures in a workspace",
        annotations(read_only_hint = true)
    )]
    fn structures_list(&self, Parameters(a): Parameters<PSpacesList>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .read_structures_list(&a.workspace, &profile_id)
            .map_err(err)
            .and_then(|items| slice_results(items, &a.page))
            .and_then(ser)
    }

    #[tool(
        name = "structures_add_node",
        description = "Add a node to a Studio structure"
    )]
    fn structures_add_node(&self, Parameters(a): Parameters<PStructuresAddNode>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_structures_add_node(
                &a.workspace,
                StructureNodeRequest {
                    workspace_id: &profile_id,
                    structure_id: &a.structure_id,
                    node_id: &a.node_id,
                    kind: &a.kind,
                    label: &a.label,
                    body_digest: a.body_digest.as_deref(),
                    entity_ref: a.entity_ref,
                    expected_root: a.expected_root.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "structures_update_node",
        description = "Update a Studio structure node"
    )]
    fn structures_update_node(&self, Parameters(a): Parameters<PStructuresAddNode>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_structures_update_node(
                &a.workspace,
                StructureNodeRequest {
                    workspace_id: &profile_id,
                    structure_id: &a.structure_id,
                    node_id: &a.node_id,
                    kind: &a.kind,
                    label: &a.label,
                    body_digest: a.body_digest.as_deref(),
                    entity_ref: a.entity_ref,
                    expected_root: a.expected_root.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "structures_move_node",
        description = "Move a Studio structure node under a parent"
    )]
    fn structures_move_node(&self, Parameters(a): Parameters<PStructuresMoveNode>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_structures_move_node(
                &a.workspace,
                StructureMoveRequest {
                    workspace_id: &profile_id,
                    structure_id: &a.structure_id,
                    node_id: &a.node_id,
                    parent_node_id: a.parent_node_id.as_deref(),
                    label: a.label.as_deref(),
                    expected_root: a.expected_root.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "structures_link_node",
        description = "Create a typed edge between structure nodes"
    )]
    fn structures_link_node(&self, Parameters(a): Parameters<PStructuresLinkNode>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_structures_link_node(
                &a.workspace,
                StructureLinkRequest {
                    workspace_id: &profile_id,
                    structure_id: &a.structure_id,
                    edge_id: &a.edge_id,
                    src_node_id: &a.src_node_id,
                    dst_node_id: &a.dst_node_id,
                    label: &a.label,
                    target_ref: a.target_ref,
                    expected_root: a.expected_root.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "structures_bind",
        description = "Bind a Studio structure node to an entity reference"
    )]
    fn structures_bind(&self, Parameters(a): Parameters<PStructuresBind>) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        self.mcp
            .write_structures_bind(
                &a.workspace,
                StructureBindRequest {
                    workspace_id: &profile_id,
                    structure_id: &a.structure_id,
                    node_id: &a.node_id,
                    entity_ref: a.entity_ref,
                    expected_root: a.expected_root.as_deref(),
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    #[tool(
        name = "structures_decompose_to_tickets",
        description = "Create tickets from Studio structure nodes"
    )]
    fn structures_decompose_to_tickets(
        &self,
        Parameters(a): Parameters<PStructuresDecomposeToTickets>,
    ) -> ToolResult {
        let profile_id = workspace_profile_id(&self.mcp, &a.workspace).map_err(err)?;
        let items = a
            .items
            .iter()
            .map(|item| StructureDecomposeItem {
                node_id: &item.node_id,
                project_id: &item.project_id,
                ticket_type: item.ticket_type.as_deref(),
                fields: item.fields.as_ref(),
                policy_labels: &item.policy_labels,
            })
            .collect::<Vec<_>>();
        self.mcp
            .write_structures_decompose_to_tickets(
                &a.workspace,
                StructureDecomposeRequest {
                    workspace_id: &profile_id,
                    structure_id: &a.structure_id,
                    items: &items,
                },
            )
            .map_err(err)
            .and_then(ser)
    }

    // ===== kv =====
    #[tool(name = "kv_put", description = "Set a typed key")]
    fn kv_put(&self, Parameters(a): Parameters<PKvPut>) -> ToolResult {
        self.mcp
            .write_kv_put(&a.workspace, &a.collection, &a.key, a.value)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "kv_get",
        description = "Get a typed key",
        annotations(read_only_hint = true)
    )]
    fn kv_get(&self, Parameters(a): Parameters<PKvKey>) -> ToolResult {
        self.mcp
            .read_kv_get(&a.workspace, &a.collection, &a.key)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "kv_delete",
        description = "Delete a typed key; returns whether present"
    )]
    fn kv_delete(&self, Parameters(a): Parameters<PKvKey>) -> ToolResult {
        self.mcp
            .write_kv_delete(&a.workspace, &a.collection, &a.key)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "kv_list",
        description = "The whole map (canonical CBOR)",
        annotations(read_only_hint = true)
    )]
    fn kv_list(&self, Parameters(a): Parameters<PNsName>) -> ToolResult {
        self.mcp
            .read_kv_list(&a.workspace, &a.collection)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }
    #[tool(
        name = "kv_range",
        description = "Half-open [lo,hi) slice (canonical CBOR)",
        annotations(read_only_hint = true)
    )]
    fn kv_range(&self, Parameters(a): Parameters<PKvRange>) -> ToolResult {
        self.mcp
            .read_kv_range(
                &a.workspace,
                &a.collection,
                &a.lo,
                &a.hi,
                a.predicate.as_ref(),
            )
            .map_err(err)
            .and_then(|b| budgeted_out_value("kv_range", jbytes(&b), a.max_output_bytes))
    }

    // ===== document =====
    #[tool(name = "document_put_text", description = "Upsert a UTF-8 document")]
    fn document_put_text(&self, Parameters(a): Parameters<PDocPutText>) -> ToolResult {
        self.mcp
            .write_document_put_text(
                &a.workspace,
                &a.collection,
                &a.id,
                &a.text,
                a.expected_entity_tag.as_deref(),
            )
            .map(|result| {
                json!({
                    "digest": result.digest.to_string(),
                    "entity_tag": result.entity_tag
                })
            })
            .map_err(err)
            .map(Json)
    }
    #[tool(
        name = "document_get_text",
        description = "Get a UTF-8 document",
        annotations(read_only_hint = true)
    )]
    fn document_get_text(&self, Parameters(a): Parameters<PDocId>) -> ToolResult {
        self.mcp
            .read_document_get_text(&a.workspace, &a.collection, &a.id)
            .map(|document| {
                document.map(|document| {
                    json!({
                        "text": document.text,
                        "digest": document.digest.to_string(),
                        "entity_tag": document.entity_tag
                    })
                })
            })
            .map_err(err)
            .map(|value| Json(json!({ "value": value })))
    }
    #[tool(
        name = "document_put_binary",
        description = "Upsert binary document bytes"
    )]
    fn document_put_binary(&self, Parameters(a): Parameters<PDocPutBinary>) -> ToolResult {
        self.mcp
            .write_document_put_binary(
                &a.workspace,
                &a.collection,
                &a.id,
                a.bytes,
                a.expected_entity_tag.as_deref(),
            )
            .map(|result| {
                json!({
                    "digest": result.digest.to_string(),
                    "entity_tag": result.entity_tag
                })
            })
            .map_err(err)
            .map(Json)
    }
    #[tool(
        name = "document_get_binary",
        description = "Get binary document bytes",
        annotations(read_only_hint = true)
    )]
    fn document_get_binary(&self, Parameters(a): Parameters<PDocId>) -> ToolResult {
        self.mcp
            .read_document_get_binary(&a.workspace, &a.collection, &a.id)
            .map(|document| {
                document.map(|document| {
                    json!({
                        "bytes": document.bytes,
                        "digest": document.digest.to_string(),
                        "entity_tag": document.entity_tag
                    })
                })
            })
            .map_err(err)
            .map(|value| Json(json!({ "value": value })))
    }
    #[tool(
        name = "document_query",
        description = "Query document ids and metadata",
        annotations(read_only_hint = true)
    )]
    fn document_query(&self, Parameters(a): Parameters<PDocQuery>) -> ToolResult {
        let projections = a
            .projections
            .iter()
            .map(|projection| (projection.name.as_str(), projection.path.as_str()))
            .collect::<Vec<_>>();
        self.mcp
            .read_document_query(crate::reads::DocumentQueryRead {
                workspace: &a.workspace,
                name: &a.collection,
                id_prefix: a.id_prefix.as_deref(),
                predicate: a.predicate.as_ref(),
                projections: &projections,
                index: a.index.as_deref(),
                value: a.value.as_ref(),
                cursor: a.cursor.as_deref(),
                limit: a.limit,
                include_document: a.include_document,
            })
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "document_replace_text",
        description = "Guarded UTF-8 text replacement"
    )]
    fn document_replace_text(&self, Parameters(a): Parameters<PDocReplaceText>) -> ToolResult {
        self.mcp
            .write_document_replace_text(DocumentReplaceTextRequest {
                workspace: &a.workspace,
                name: &a.collection,
                id: &a.id,
                base_digest: &a.base_digest,
                find: &a.find,
                replace: &a.replace,
                replace_all: a.replace_all,
            })
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "document_delete",
        description = "Delete a document; returns whether present"
    )]
    fn document_delete(&self, Parameters(a): Parameters<PDocId>) -> ToolResult {
        self.mcp
            .write_document_delete(&a.workspace, &a.collection, &a.id)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "document_list_binary",
        description = "The collection (canonical CBOR)",
        annotations(read_only_hint = true)
    )]
    fn document_list_binary(&self, Parameters(a): Parameters<PNsName>) -> ToolResult {
        self.mcp
            .read_document_list_binary(&a.workspace, &a.collection)
            .map_err(err)
            .map(|bytes| out_value(jbytes(&bytes)))
    }

    // ===== timeseries =====
    #[tool(name = "timeseries_put", description = "Record a point")]
    fn timeseries_put(&self, Parameters(a): Parameters<PTsPut>) -> ToolResult {
        self.mcp
            .write_timeseries_put(&a.workspace, &a.collection, a.ts, a.value)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "timeseries_get",
        description = "Value at a timestamp",
        annotations(read_only_hint = true)
    )]
    fn timeseries_get(&self, Parameters(a): Parameters<PTsGet>) -> ToolResult {
        self.mcp
            .read_timeseries_get(&a.workspace, &a.collection, a.ts)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "timeseries_range",
        description = "Points in [from,to] (canonical CBOR)",
        annotations(read_only_hint = true)
    )]
    fn timeseries_range(&self, Parameters(a): Parameters<PTsRange>) -> ToolResult {
        self.mcp
            .read_timeseries_range(&a.workspace, &a.collection, a.from, a.to)
            .map_err(err)
            .and_then(|s| {
                let mut page = Series::new();
                for (ts, value) in s
                    .iter()
                    .skip(result_offset(&a.page))
                    .take(result_limit(&a.page)?)
                {
                    page.put(ts, value.to_vec());
                }
                budgeted_out_value(
                    "timeseries_range",
                    jbytes(&page.encode()),
                    a.max_output_bytes,
                )
            })
    }
    #[tool(
        name = "timeseries_latest",
        description = "Most recent point",
        annotations(read_only_hint = true)
    )]
    fn timeseries_latest(&self, Parameters(a): Parameters<PNsName>) -> ToolResult {
        self.mcp
            .read_timeseries_latest(&a.workspace, &a.collection)
            .map_err(err)
            .and_then(|o: Option<TsPoint>| ser(o))
    }

    // ===== ledger =====
    #[tool(
        name = "ledger_append",
        description = "Append an entry; returns its sequence"
    )]
    fn ledger_append(&self, Parameters(a): Parameters<PLedgerAppend>) -> ToolResult {
        self.mcp
            .write_ledger_append(&a.workspace, &a.collection, a.payload)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "ledger_get",
        description = "Entry at a sequence",
        annotations(read_only_hint = true)
    )]
    fn ledger_get(&self, Parameters(a): Parameters<PLedgerSeq>) -> ToolResult {
        self.mcp
            .read_ledger_get(&a.workspace, &a.collection, a.seq)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "ledger_head",
        description = "Head hash",
        annotations(read_only_hint = true)
    )]
    fn ledger_head(&self, Parameters(a): Parameters<PNsName>) -> ToolResult {
        self.mcp
            .read_ledger_head(&a.workspace, &a.collection)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "ledger_len",
        description = "Entry count",
        annotations(read_only_hint = true)
    )]
    fn ledger_len(&self, Parameters(a): Parameters<PNsName>) -> ToolResult {
        self.mcp
            .read_ledger_len(&a.workspace, &a.collection)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "ledger_verify",
        description = "Verify hash chain",
        annotations(read_only_hint = true)
    )]
    fn ledger_verify(&self, Parameters(a): Parameters<PNsName>) -> ToolResult {
        self.mcp
            .read_ledger_verify(&a.workspace, &a.collection)
            .map_err(err)
            .and_then(ser)
    }

    // ===== queue =====
    #[tool(
        name = "queue_append",
        description = "Append an entry; returns its sequence"
    )]
    fn queue_append(&self, Parameters(a): Parameters<PQueueAppend>) -> ToolResult {
        self.mcp
            .write_queue_append(&a.workspace, &a.stream, &a.entry)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "queue_get",
        description = "Entry at a sequence",
        annotations(read_only_hint = true)
    )]
    fn queue_get(&self, Parameters(a): Parameters<PQueueGet>) -> ToolResult {
        self.mcp
            .read_queue_get(&a.workspace, &a.stream, a.seq)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "queue_range",
        description = "Entries [lo,hi)",
        annotations(read_only_hint = true)
    )]
    fn queue_range(&self, Parameters(a): Parameters<PQueueRange>) -> ToolResult {
        self.mcp
            .read_queue_range(&a.workspace, &a.stream, a.lo, a.hi)
            .map_err(err)
            .and_then(|items| slice_results(items, &a.page))
            .and_then(ser)
    }
    #[tool(
        name = "queue_len",
        description = "Entry count",
        annotations(read_only_hint = true)
    )]
    fn queue_len(&self, Parameters(a): Parameters<PQueueStream>) -> ToolResult {
        self.mcp
            .read_queue_len(&a.workspace, &a.stream)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "queue_consumer_position",
        description = "Consumer next sequence",
        annotations(read_only_hint = true)
    )]
    fn queue_consumer_position(&self, Parameters(a): Parameters<PConsumer>) -> ToolResult {
        self.mcp
            .read_queue_consumer_position(&a.workspace, &a.stream, &a.consumer_id)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "queue_consumer_read",
        description = "Read up to max, without advancing",
        annotations(read_only_hint = true)
    )]
    fn queue_consumer_read(&self, Parameters(a): Parameters<PConsumerRead>) -> ToolResult {
        self.mcp
            .read_queue_consumer_read(&a.workspace, &a.stream, &a.consumer_id, a.max)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "queue_consumer_advance",
        description = "Advance consumer (monotonic)"
    )]
    fn queue_consumer_advance(&self, Parameters(a): Parameters<PConsumerSeq>) -> ToolResult {
        self.mcp
            .write_queue_consumer_advance(&a.workspace, &a.stream, &a.consumer_id, a.next_seq)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "queue_consumer_reset",
        description = "Set consumer sequence (may rewind)"
    )]
    fn queue_consumer_reset(&self, Parameters(a): Parameters<PConsumerSeq>) -> ToolResult {
        self.mcp
            .write_queue_consumer_reset(&a.workspace, &a.stream, &a.consumer_id, a.next_seq)
            .map_err(err)
            .and_then(ser)
    }

    // ===== watch =====
    #[tool(
        name = "watch_subscribe",
        description = "Create a pull-watch cursor for a workspace branch",
        annotations(read_only_hint = true)
    )]
    fn watch_subscribe(&self, Parameters(a): Parameters<PWatchSubscribe>) -> ToolResult {
        self.mcp
            .read_watch_subscribe(
                &a.workspace,
                &a.branch,
                a.from.as_deref(),
                a.facet.as_deref(),
                a.path_prefix.as_deref(),
                a.change_kinds.as_deref(),
            )
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "watch_poll",
        description = "Read a batch from a pull-watch cursor",
        annotations(read_only_hint = true)
    )]
    fn watch_poll(&self, Parameters(a): Parameters<PWatchPoll>) -> ToolResult {
        self.mcp
            .read_watch_poll(&a.workspace, &a.cursor, a.max)
            .map_err(err)
            .and_then(ser)
    }

    // ===== fs =====
    #[tool(name = "fs_write_file", description = "Write a file")]
    fn fs_write_file(&self, Parameters(a): Parameters<PFsWrite>) -> ToolResult {
        self.mcp
            .write_fs_write_file(&a.workspace, &a.path, &a.content, a.mode)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "fs_read_file",
        description = "Read a file",
        annotations(read_only_hint = true)
    )]
    fn fs_read_file(&self, Parameters(a): Parameters<PFsPath>) -> ToolResult {
        self.mcp
            .read_fs_read_file(&a.workspace, &a.path)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "fs_append_file", description = "Append to a file")]
    fn fs_append_file(&self, Parameters(a): Parameters<PFsAppend>) -> ToolResult {
        self.mcp
            .write_fs_append_file(&a.workspace, &a.path, &a.content)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "fs_remove_file", description = "Remove a file")]
    fn fs_remove_file(&self, Parameters(a): Parameters<PFsPath>) -> ToolResult {
        self.mcp
            .write_fs_remove_file(&a.workspace, &a.path)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "fs_create_directory", description = "Create a directory")]
    fn fs_create_directory(&self, Parameters(a): Parameters<PFsDirectory>) -> ToolResult {
        self.mcp
            .write_fs_create_directory(&a.workspace, &a.path, a.recursive)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "fs_remove_directory", description = "Remove a directory")]
    fn fs_remove_directory(&self, Parameters(a): Parameters<PFsDirectory>) -> ToolResult {
        self.mcp
            .write_fs_remove_directory(&a.workspace, &a.path, a.recursive)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "fs_read_at",
        description = "Read a byte range",
        annotations(read_only_hint = true)
    )]
    fn fs_read_at(&self, Parameters(a): Parameters<PFsReadAt>) -> ToolResult {
        self.mcp
            .read_fs_read_at(&a.workspace, &a.path, a.offset, a.len)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "fs_stat",
        description = "Read file metadata",
        annotations(read_only_hint = true)
    )]
    fn fs_stat(&self, Parameters(a): Parameters<PFsPath>) -> ToolResult {
        self.mcp
            .read_fs_stat(&a.workspace, &a.path)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "fs_list_directory",
        description = "List directory entries",
        annotations(read_only_hint = true)
    )]
    fn fs_list_directory(&self, Parameters(a): Parameters<PFsPath>) -> ToolResult {
        self.mcp
            .read_fs_list_directory(&a.workspace, &a.path)
            .map_err(err)
            .and_then(|bytes| {
                let entries = loom_wire::fs::dir_listing_from_cbor(&bytes).map_err(err)?;
                let page = slice_results(entries, &a.page)?;
                loom_wire::fs::dir_listing_to_cbor(&page).map_err(err)
            })
            .and_then(ser)
    }
    #[tool(name = "fs_write_at", description = "Write at an offset")]
    fn fs_write_at(&self, Parameters(a): Parameters<PFsWriteAt>) -> ToolResult {
        self.mcp
            .write_fs_write_at(&a.workspace, &a.path, a.offset, &a.data)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "fs_truncate", description = "Resize a file")]
    fn fs_truncate(&self, Parameters(a): Parameters<PFsTruncate>) -> ToolResult {
        self.mcp
            .write_fs_truncate(&a.workspace, &a.path, a.size)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "fs_symlink", description = "Create a symlink")]
    fn fs_symlink(&self, Parameters(a): Parameters<PFsSymlink>) -> ToolResult {
        self.mcp
            .write_fs_symlink(&a.workspace, &a.target, &a.link_path)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "fs_read_link",
        description = "Read a symlink target",
        annotations(read_only_hint = true)
    )]
    fn fs_read_link(&self, Parameters(a): Parameters<PFsPath>) -> ToolResult {
        self.mcp
            .read_fs_read_link(&a.workspace, &a.path)
            .map_err(err)
            .and_then(ser)
    }

    // ===== apps =====
    #[tool(
        name = "apps_list",
        description = "List MCP app candidates with validity status",
        annotations(read_only_hint = true)
    )]
    fn apps_list(&self, Parameters(a): Parameters<PNs>) -> ToolResult {
        let workspace_bound = self.binding.workspace.is_some();
        self.mcp
            .read_mcp_app_inventory(&a.workspace)
            .map(|mut items| {
                // Honor the active workspace binding when reporting app URIs, matching the
                // resources/list, resources/read, and launcher surfaces (which elide the bound
                // workspace). The facade builds URIs unbound; rebind here where we know the scope.
                for item in &mut items {
                    if item.uri.is_some() {
                        item.uri = Some(apps::app_uri(&item.workspace, &item.app, workspace_bound));
                    }
                }
                items
            })
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "apps_show",
        description = "Return a valid MCP app resource",
        annotations(read_only_hint = true)
    )]
    fn apps_show(&self, Parameters(a): Parameters<PApp>) -> ToolResult {
        let workspace_bound = self.binding.workspace.is_some();
        self.mcp
            .read_mcp_app_show(&a.workspace, &a.app)
            .map(|resource| {
                // See `apps_list`: report the binding-aware URI so `apps_show` agrees with the
                // resource/launcher surfaces instead of always emitting the fully-qualified form.
                resource.map(|mut r| {
                    r.uri = apps::app_uri(&r.workspace, &r.app, workspace_bound);
                    r
                })
            })
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "apps_read_file",
        description = "Read a file inside an MCP app",
        annotations(read_only_hint = true)
    )]
    fn apps_read_file(&self, Parameters(a): Parameters<PAppPath>) -> ToolResult {
        self.mcp
            .read_mcp_app_file(&a.workspace, &a.app, &a.path)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "apps_create",
        description = "Create or replace root MCP app files"
    )]
    fn apps_create(&self, Parameters(a): Parameters<PAppCreate>) -> ToolResult {
        self.mcp
            .write_mcp_app_create(&a.workspace, &a.app, &a.index_html, &a.meta_md)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "apps_write_file",
        description = "Write a file inside an MCP app"
    )]
    fn apps_write_file(&self, Parameters(a): Parameters<PAppWrite>) -> ToolResult {
        self.mcp
            .write_mcp_app_write_file(&a.workspace, &a.app, &a.path, &a.content, a.mode)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "apps_remove_file",
        description = "Remove a file inside an MCP app"
    )]
    fn apps_remove_file(&self, Parameters(a): Parameters<PAppPath>) -> ToolResult {
        self.mcp
            .write_mcp_app_remove_file(&a.workspace, &a.app, &a.path)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "apps_call_tool",
        description = "Call a visible Loom MCP tool from an MCP app"
    )]
    fn apps_call_tool(&self, Parameters(_a): Parameters<PAppCallTool>) -> ToolResult {
        Err(ErrorData::internal_error(
            "apps_call_tool must dispatch through the app bridge",
            None,
        ))
    }

    // ===== ask =====
    #[tool(
        name = "ask_questions",
        description = "Ask the user structured decision questions (question, context, examples, \
                       options, recommendation) rendered in the Decisions app; returns an ask_id, \
                       then call ask_answers with it to wait for the answers"
    )]
    fn ask_questions(
        &self,
        Parameters(a): Parameters<PAskBegin>,
    ) -> Result<CallToolResult, ErrorData> {
        let workspace_bound = self.binding.workspace.is_some();
        // Domain: validate, mint the ask id, and persist the pending document (shared with the
        // server-side remote-exec route via `ask_begin`).
        let questions = normalize_ask_questions(&a.questions);
        let doc = ask_begin(&self.mcp, &a.workspace, questions).map_err(err)?;
        let id = doc
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        // Presentation: bind the Decisions app launch payload/meta to the active workspace binding.
        let Some(app) = self
            .mcp
            .read_mcp_app_show(&a.workspace, apps::INTERNAL_DECISIONS_APP)
            .map_err(err)?
        else {
            return Err(ErrorData::internal_error(
                "internal ask app is unavailable",
                None,
            ));
        };
        // Instance-addressed URI: each ask renders as its own app instance, so concurrent asks
        // coexist with independent views.
        let uri = apps::app_uri_with_instance(&app.workspace, &app.app, Some(&id), workspace_bound);
        let mut payload = app_launch_payload(&app, workspace_bound);
        if let Some(map) = payload.as_object_mut() {
            map.insert("ask_id".to_string(), Value::String(id));
            map.insert("uri".to_string(), Value::String(uri.clone()));
        }
        Ok(CallToolResult::structured(json!({ "value": payload }))
            .with_meta(Some(app_tool_meta(&app.meta, &uri))))
    }
    #[tool(
        name = "ask_answers",
        description = "Wait for the user's answers to an ask_questions call: blocks on the ask_id \
                       until answered or aborted, then returns the answers",
        annotations(read_only_hint = true)
    )]
    async fn ask_answers(&self, Parameters(a): Parameters<PAskWait>) -> ToolResult {
        let timeout = Duration::from_millis(
            a.timeout_ms
                .unwrap_or(ASK_WAIT_DEFAULT_MS)
                .min(ASK_WAIT_MAX_MS),
        );
        let started = std::time::Instant::now();
        // The bounded wait is a client-side concern (it must not hold the server write authority); each
        // poll runs the ask domain server-side via `ask_poll_current` (remote-backed) or the local store.
        loop {
            let Some(state) = self.ask_poll_current(&a.workspace, &a.id)? else {
                return Err(ErrorData::invalid_params(
                    format!("unknown ask {}", a.id),
                    None,
                ));
            };
            let status = state
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("pending");
            if status != "pending" {
                return Ok(out_value(state));
            }
            if started.elapsed() >= timeout {
                return Ok(out_value(
                    json!({ "id": a.id, "status": "timeout", "answers": [] }),
                ));
            }
            tokio::time::sleep(ASK_POLL_INTERVAL).await;
        }
    }
    #[tool(
        name = "ask_record",
        description = "Record the user's answers for a pending ask. App-only: the Decisions app \
                       calls this over the host bridge; agents receive answers from ask_answers"
    )]
    fn ask_record(&self, Parameters(a): Parameters<PAskSubmit>) -> ToolResult {
        let answers = normalize_ask_answers(&a.answers);
        ask_submit(
            &self.mcp,
            &a.workspace,
            &a.id,
            &answers,
            a.aborted.unwrap_or(false),
        )
        .map_err(err)?;
        Ok(out_value(Value::Null))
    }

    // ===== vcs =====
    #[tool(name = "vcs_commit", description = "Record the working tree")]
    fn vcs_commit(&self, Parameters(a): Parameters<PCommit>) -> ToolResult {
        self.mcp
            .write_vcs_commit(&a.workspace, &a.author, &a.message, a.timestamp_ms)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "vcs_branch", description = "Create a branch")]
    fn vcs_branch(&self, Parameters(a): Parameters<PVcsName>) -> ToolResult {
        self.mcp
            .write_vcs_branch(&a.workspace, &a.name)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "vcs_checkout", description = "Switch branch")]
    fn vcs_checkout(&self, Parameters(a): Parameters<PBranch>) -> ToolResult {
        self.mcp
            .write_vcs_checkout(&a.workspace, &a.branch)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "vcs_log",
        description = "Commit log, newest first",
        annotations(read_only_hint = true)
    )]
    fn vcs_log(&self, Parameters(a): Parameters<PBranch>) -> ToolResult {
        self.mcp
            .read_vcs_log(&a.workspace, &a.branch)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "vcs_head_branch",
        description = "Read the current branch",
        annotations(read_only_hint = true)
    )]
    fn vcs_head_branch(&self, Parameters(a): Parameters<PNs>) -> ToolResult {
        self.mcp
            .read_vcs_head_branch(&a.workspace)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "vcs_merge", description = "Merge a branch in")]
    fn vcs_merge(&self, Parameters(a): Parameters<PMerge>) -> ToolResult {
        self.mcp
            .write_vcs_merge(&a.workspace, &a.from_branch, &a.author, a.timestamp_ms)
            .map_err(err)
            .map(|o| out_value(merge_val(&o)))
    }
    #[tool(
        name = "vcs_merge_in_progress",
        description = "Whether a merge is paused",
        annotations(read_only_hint = true)
    )]
    fn vcs_merge_in_progress(&self, Parameters(a): Parameters<PNs>) -> ToolResult {
        self.mcp
            .read_vcs_merge_in_progress(&a.workspace)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "vcs_merge_conflicts",
        description = "Unresolved merge paths",
        annotations(read_only_hint = true)
    )]
    fn vcs_merge_conflicts(&self, Parameters(a): Parameters<PNs>) -> ToolResult {
        self.mcp
            .read_vcs_merge_conflicts(&a.workspace)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "vcs_merge_resolve", description = "Resolve a conflicted path")]
    fn vcs_merge_resolve(&self, Parameters(a): Parameters<PMergeResolve>) -> ToolResult {
        self.mcp
            .write_vcs_merge_resolve(&a.workspace, &a.path, &a.resolution)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "vcs_merge_abort", description = "Abort an in-progress merge")]
    fn vcs_merge_abort(&self, Parameters(a): Parameters<PNs>) -> ToolResult {
        self.mcp
            .write_vcs_merge_abort(&a.workspace)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "vcs_merge_continue", description = "Record the merge commit")]
    fn vcs_merge_continue(&self, Parameters(a): Parameters<PMerge2>) -> ToolResult {
        self.mcp
            .write_vcs_merge_continue(&a.workspace, &a.author, a.timestamp_ms)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "vcs_status",
        description = "Working state",
        annotations(read_only_hint = true)
    )]
    fn vcs_status(&self, Parameters(a): Parameters<PNs>) -> ToolResult {
        self.mcp
            .read_vcs_status(&a.workspace)
            .map_err(err)
            .map(|s| out_value(status_val(&s)))
    }
    #[tool(name = "vcs_stage", description = "Stage a path")]
    fn vcs_stage(&self, Parameters(a): Parameters<PNsPath>) -> ToolResult {
        self.mcp
            .write_vcs_stage(&a.workspace, &a.path)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "vcs_stage_all", description = "Stage all changes")]
    fn vcs_stage_all(&self, Parameters(a): Parameters<PNs>) -> ToolResult {
        self.mcp
            .write_vcs_stage_all(&a.workspace)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "vcs_unstage", description = "Unstage a path")]
    fn vcs_unstage(&self, Parameters(a): Parameters<PNsPath>) -> ToolResult {
        self.mcp
            .write_vcs_unstage(&a.workspace, &a.path)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "vcs_commit_staged", description = "Commit the staged index")]
    fn vcs_commit_staged(&self, Parameters(a): Parameters<PCommit>) -> ToolResult {
        self.mcp
            .write_vcs_commit_staged(&a.workspace, &a.author, &a.message, a.timestamp_ms)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "vcs_tag_create", description = "Create a tag")]
    fn vcs_tag_create(&self, Parameters(a): Parameters<PTagCreate>) -> ToolResult {
        self.mcp
            .write_vcs_tag_create(
                &a.workspace,
                &a.name,
                &a.rev,
                &a.tagger,
                &a.message,
                a.timestamp_ms,
            )
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "vcs_tag_list",
        description = "Tag names",
        annotations(read_only_hint = true)
    )]
    fn vcs_tag_list(&self, Parameters(a): Parameters<PNs>) -> ToolResult {
        self.mcp
            .read_vcs_tag_list(&a.workspace)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "vcs_tag_target",
        description = "Tag ref target",
        annotations(read_only_hint = true)
    )]
    fn vcs_tag_target(&self, Parameters(a): Parameters<PVcsName>) -> ToolResult {
        self.mcp
            .read_vcs_tag_target(&a.workspace, &a.name)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "vcs_tag_delete", description = "Delete a tag")]
    fn vcs_tag_delete(&self, Parameters(a): Parameters<PVcsName>) -> ToolResult {
        self.mcp
            .write_vcs_tag_delete(&a.workspace, &a.name)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "vcs_tag_rename", description = "Rename a tag")]
    fn vcs_tag_rename(&self, Parameters(a): Parameters<PTagRename>) -> ToolResult {
        self.mcp
            .write_vcs_tag_rename(&a.workspace, &a.old_name, &a.new_name)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "vcs_restore_file", description = "Restore one path to a rev")]
    fn vcs_restore_file(&self, Parameters(a): Parameters<PRestoreFile>) -> ToolResult {
        self.mcp
            .write_vcs_restore_file(&a.workspace, &a.rev, &a.path)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "vcs_restore_path", description = "Restore a subtree to a rev")]
    fn vcs_restore_path(&self, Parameters(a): Parameters<PRestorePath>) -> ToolResult {
        self.mcp
            .write_vcs_restore_path(&a.workspace, &a.rev, &a.prefix)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "vcs_cherry_pick",
        description = "Replay commits onto the branch"
    )]
    fn vcs_cherry_pick(&self, Parameters(a): Parameters<PCherryPick>) -> ToolResult {
        self.mcp
            .write_vcs_cherry_pick(&a.workspace, &a.commits, a.timestamp_ms, a.dry_run)
            .map_err(err)
            .map(|o| out_value(replay_val(&o)))
    }
    #[tool(name = "vcs_revert", description = "Revert commits")]
    fn vcs_revert(&self, Parameters(a): Parameters<PRevert>) -> ToolResult {
        self.mcp
            .write_vcs_revert(
                &a.workspace,
                &a.commits,
                &a.author,
                a.timestamp_ms,
                a.dry_run,
            )
            .map_err(err)
            .map(|o| out_value(replay_val(&o)))
    }
    #[tool(name = "vcs_rebase", description = "Rebase onto a target")]
    fn vcs_rebase(&self, Parameters(a): Parameters<PRebase>) -> ToolResult {
        self.mcp
            .write_vcs_rebase(&a.workspace, &a.onto, a.timestamp_ms, a.dry_run)
            .map_err(err)
            .map(|o| out_value(replay_val(&o)))
    }
    #[tool(name = "vcs_squash", description = "Squash commits after onto")]
    fn vcs_squash(&self, Parameters(a): Parameters<PSquash>) -> ToolResult {
        self.mcp
            .write_vcs_squash(&a.workspace, &a.onto, &a.author, &a.message, a.timestamp_ms)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "vcs_diff",
        description = "Cross-facet structural diff envelope between commits",
        annotations(read_only_hint = true)
    )]
    fn vcs_diff(&self, Parameters(a): Parameters<PVcsDiff>) -> ToolResult {
        self.mcp
            .read_vcs_diff(&a.workspace, &a.from_commit, &a.to_commit)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }
    #[tool(
        name = "vcs_blame",
        description = "Each path with its last commit",
        annotations(read_only_hint = true)
    )]
    fn vcs_blame(&self, Parameters(a): Parameters<PBranch>) -> ToolResult {
        self.mcp
            .read_vcs_blame(&a.workspace, &a.branch)
            .map_err(err)
            .and_then(ser)
    }

    // ===== sql =====
    #[tool(name = "sql_exec", description = "Run SQL statements")]
    fn sql_exec(&self, Parameters(a): Parameters<PSqlExec>) -> ToolResult {
        self.mcp
            .write_sql_exec(&a.workspace, &a.db, &a.sql)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }
    #[tool(name = "sql_commit", description = "Record a SQL-workspace commit")]
    fn sql_commit(&self, Parameters(a): Parameters<PSqlCommit>) -> ToolResult {
        self.mcp
            .write_sql_commit(&a.workspace, &a.author, &a.message, a.timestamp_ms)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "sql_query",
        description = "Run a read-only query (canonical CBOR)",
        annotations(read_only_hint = true)
    )]
    fn sql_query(&self, Parameters(a): Parameters<PSqlExec>) -> ToolResult {
        self.mcp
            .read_sql_query(&a.workspace, &a.db, &a.sql)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }
    #[tool(
        name = "sql_read_table",
        description = "Read a staged table (canonical CBOR)",
        annotations(read_only_hint = true)
    )]
    fn sql_read_table(&self, Parameters(a): Parameters<PSqlTable>) -> ToolResult {
        self.mcp
            .read_sql_read_table(&a.workspace, &a.db, &a.table)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }
    #[tool(
        name = "sql_read_table_at",
        description = "Read a committed table at a commit (canonical CBOR)",
        annotations(read_only_hint = true)
    )]
    fn sql_read_table_at(&self, Parameters(a): Parameters<PSqlTableAt>) -> ToolResult {
        self.mcp
            .read_sql_read_table_at(&a.workspace, &a.db, &a.table, &a.commit)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }
    #[tool(
        name = "sql_index_scan",
        description = "Scan a secondary index (canonical CBOR)",
        annotations(read_only_hint = true)
    )]
    fn sql_index_scan(&self, Parameters(a): Parameters<PSqlIndexScan>) -> ToolResult {
        self.mcp
            .read_sql_index_scan(&a.workspace, &a.db, &a.table, &a.index, &a.prefix)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }
    #[tool(
        name = "sql_index_scan_at",
        description = "Scan a secondary index at a commit (canonical CBOR)",
        annotations(read_only_hint = true)
    )]
    fn sql_index_scan_at(&self, Parameters(a): Parameters<PSqlIndexScanAt>) -> ToolResult {
        self.mcp
            .read_sql_index_scan_at(
                &a.workspace,
                &a.db,
                &a.table,
                &a.index,
                &a.prefix,
                &a.commit,
            )
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }
    #[tool(
        name = "sql_diff",
        description = "Row-level table diff (canonical CBOR)",
        annotations(read_only_hint = true)
    )]
    fn sql_diff(&self, Parameters(a): Parameters<PSqlDiff>) -> ToolResult {
        self.mcp
            .read_sql_diff(&a.workspace, &a.db, &a.table, &a.from_commit, &a.to_commit)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }
    #[tool(
        name = "sql_table_diff",
        description = "Schema-aware table diff (canonical CBOR)",
        annotations(read_only_hint = true)
    )]
    fn sql_table_diff(&self, Parameters(a): Parameters<PSqlDiff>) -> ToolResult {
        self.mcp
            .read_sql_table_diff(&a.workspace, &a.db, &a.table, &a.from_commit, &a.to_commit)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }
    #[tool(
        name = "sql_blame",
        description = "Row-level table blame (canonical CBOR)",
        annotations(read_only_hint = true)
    )]
    fn sql_blame(&self, Parameters(a): Parameters<PSqlBlame>) -> ToolResult {
        self.mcp
            .read_sql_blame(&a.workspace, &a.db, &a.branch, &a.table)
            .map_err(err)
            .map(|b| out_value(jbytes(&b)))
    }

    // ===== list-collections (collection discovery) =====
    #[tool(
        name = "kv_list_collections",
        description = "List the KV map (collection) names in the workspace",
        annotations(read_only_hint = true)
    )]
    fn kv_list_collections(&self, Parameters(a): Parameters<PNs>) -> ToolResult {
        self.mcp
            .read_collections(&a.workspace, FacetKind::Kv)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "document_list_collections",
        description = "List the document collection names in the workspace",
        annotations(read_only_hint = true)
    )]
    fn document_list_collections(&self, Parameters(a): Parameters<PNs>) -> ToolResult {
        self.mcp
            .read_collections(&a.workspace, FacetKind::Document)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "timeseries_list_collections",
        description = "List the time-series set (collection) names in the workspace",
        annotations(read_only_hint = true)
    )]
    fn timeseries_list_collections(&self, Parameters(a): Parameters<PNs>) -> ToolResult {
        self.mcp
            .read_collections(&a.workspace, FacetKind::TimeSeries)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "ledger_list_collections",
        description = "List the ledger log (collection) names in the workspace",
        annotations(read_only_hint = true)
    )]
    fn ledger_list_collections(&self, Parameters(a): Parameters<PNs>) -> ToolResult {
        self.mcp
            .read_collections(&a.workspace, FacetKind::Ledger)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "queue_list_streams",
        description = "List the queue stream (collection) names in the workspace",
        annotations(read_only_hint = true)
    )]
    fn queue_list_streams(&self, Parameters(a): Parameters<PNs>) -> ToolResult {
        self.mcp
            .read_collections(&a.workspace, FacetKind::Queue)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "sql_list_databases",
        description = "List the SQL database (collection) names in the workspace",
        annotations(read_only_hint = true)
    )]
    fn sql_list_databases(&self, Parameters(a): Parameters<PNs>) -> ToolResult {
        self.mcp
            .read_collections(&a.workspace, FacetKind::Sql)
            .map_err(err)
            .and_then(ser)
    }

    // ===== calendar =====
    #[tool(
        name = "calendar_create_collection",
        description = "Create a collection"
    )]
    fn calendar_create_collection(&self, Parameters(a): Parameters<PCalCreateColl>) -> ToolResult {
        self.mcp
            .write_calendar_create_collection(
                &a.workspace,
                &a.principal,
                &a.collection,
                &a.display_name,
                &a.components,
            )
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "calendar_get_collection",
        description = "Collection metadata",
        annotations(read_only_hint = true)
    )]
    fn calendar_get_collection(&self, Parameters(a): Parameters<PCalColl>) -> ToolResult {
        self.mcp
            .read_calendar_get_collection(&a.workspace, &a.principal, &a.collection)
            .map_err(err)
            .map(|o| {
                out_value(match o {
                    Some(m) => json!({
                        "display_name": m.display_name,
                        "components": m
                            .component_set
                            .iter()
                            .map(|c| match c {
                                loom_core::calendar::Component::Event => "event",
                                loom_core::calendar::Component::Todo => "todo",
                            })
                            .collect::<Vec<_>>(),
                    }),
                    None => Value::Null,
                })
            })
    }
    #[tool(
        name = "calendar_list_collections",
        description = "Collection names",
        annotations(read_only_hint = true)
    )]
    fn calendar_list_collections(&self, Parameters(a): Parameters<PCalPrincipal>) -> ToolResult {
        self.mcp
            .read_calendar_list_collections(&a.workspace, &a.principal)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "calendar_delete_collection",
        description = "Delete a collection; returns whether present"
    )]
    fn calendar_delete_collection(&self, Parameters(a): Parameters<PCalColl>) -> ToolResult {
        self.mcp
            .write_calendar_delete_collection(&a.workspace, &a.principal, &a.collection)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "calendar_put_entry",
        description = "Upsert a calendar entry (canonical-CBOR CalendarEntry)"
    )]
    fn calendar_put_entry(&self, Parameters(a): Parameters<PCalPutEntry>) -> ToolResult {
        self.mcp
            .write_calendar_put_entry(&a.workspace, &a.principal, &a.collection, &a.entry)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "calendar_put_ics",
        description = "Import an iCalendar document"
    )]
    fn calendar_put_ics(&self, Parameters(a): Parameters<PCalPutIcs>) -> ToolResult {
        self.mcp
            .write_calendar_put_ics(&a.workspace, &a.principal, &a.collection, &a.ics)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "calendar_get_entry",
        description = "One entry (canonical CBOR)",
        annotations(read_only_hint = true)
    )]
    fn calendar_get_entry(&self, Parameters(a): Parameters<PCalEntry>) -> ToolResult {
        self.mcp
            .read_calendar_get_entry(&a.workspace, &a.principal, &a.collection, &a.uid)
            .map_err(err)
            .map(|o| {
                out_value(match o {
                    Some(e) => jbytes(&e.encode()),
                    None => Value::Null,
                })
            })
    }
    #[tool(
        name = "calendar_delete_entry",
        description = "Delete an entry; returns whether present"
    )]
    fn calendar_delete_entry(&self, Parameters(a): Parameters<PCalEntry>) -> ToolResult {
        self.mcp
            .write_calendar_delete_entry(&a.workspace, &a.principal, &a.collection, &a.uid)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "calendar_list_entries",
        description = "All entries (canonical CBOR each)",
        annotations(read_only_hint = true)
    )]
    fn calendar_list_entries(&self, Parameters(a): Parameters<PCalColl>) -> ToolResult {
        self.mcp
            .read_calendar_list_entries(&a.workspace, &a.principal, &a.collection)
            .map_err(err)
            .and_then(|items| slice_results(items, &a.page))
            .map(|v| {
                out_value(Value::Array(
                    v.iter().map(|e| jbytes(&e.encode())).collect(),
                ))
            })
    }
    #[tool(
        name = "calendar_range",
        description = "Occurrences overlapping [from,to]",
        annotations(read_only_hint = true)
    )]
    fn calendar_range(&self, Parameters(a): Parameters<PCalRange>) -> ToolResult {
        self.mcp
            .read_calendar_range(&a.workspace, &a.principal, &a.collection, &a.from, &a.to)
            .map_err(err)
            .and_then(|items| slice_results(items, &a.page))
            .map(|v| out_value(Value::Array(v.iter().map(occ_val).collect())))
    }
    #[tool(
        name = "calendar_search",
        description = "Search entries (canonical CBOR each)",
        annotations(read_only_hint = true)
    )]
    fn calendar_search(&self, Parameters(a): Parameters<PCalSearch>) -> ToolResult {
        self.mcp
            .read_calendar_search(
                &a.workspace,
                &a.principal,
                &a.collection,
                &a.component,
                &a.text,
            )
            .map_err(err)
            .and_then(|items| slice_results(items, &a.page))
            .map(|v| {
                out_value(Value::Array(
                    v.iter().map(|e| jbytes(&e.encode())).collect(),
                ))
            })
    }
    #[tool(
        name = "calendar_to_ics",
        description = "iCalendar serialization of an entry",
        annotations(read_only_hint = true)
    )]
    fn calendar_to_ics(&self, Parameters(a): Parameters<PCalEntry>) -> ToolResult {
        self.mcp
            .read_calendar_to_ics(&a.workspace, &a.principal, &a.collection, &a.uid)
            .map_err(err)
            .and_then(ser)
    }

    // ===== contacts =====
    #[tool(name = "contacts_create_book", description = "Create an address book")]
    fn contacts_create_book(&self, Parameters(a): Parameters<PCardCreateBook>) -> ToolResult {
        self.mcp
            .write_contacts_create_book(&a.workspace, &a.principal, &a.book, &a.display_name)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "contacts_get_book",
        description = "Book metadata",
        annotations(read_only_hint = true)
    )]
    fn contacts_get_book(&self, Parameters(a): Parameters<PCardBook>) -> ToolResult {
        self.mcp
            .read_contacts_get_book(&a.workspace, &a.principal, &a.book)
            .map_err(err)
            .map(|o| {
                out_value(match o {
                    Some(m) => json!({"display_name": m.display_name}),
                    None => Value::Null,
                })
            })
    }
    #[tool(
        name = "contacts_list_books",
        description = "Book names",
        annotations(read_only_hint = true)
    )]
    fn contacts_list_books(&self, Parameters(a): Parameters<PCardPrincipal>) -> ToolResult {
        self.mcp
            .read_contacts_list_books(&a.workspace, &a.principal)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "contacts_delete_book",
        description = "Delete a book; returns whether present"
    )]
    fn contacts_delete_book(&self, Parameters(a): Parameters<PCardBook>) -> ToolResult {
        self.mcp
            .write_contacts_delete_book(&a.workspace, &a.principal, &a.book)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "contacts_put_entry",
        description = "Upsert a contact (canonical-CBOR ContactEntry)"
    )]
    fn contacts_put_entry(&self, Parameters(a): Parameters<PCardPutEntry>) -> ToolResult {
        self.mcp
            .write_contacts_put_entry(&a.workspace, &a.principal, &a.book, &a.entry)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "contacts_put_vcard", description = "Import a vCard document")]
    fn contacts_put_vcard(&self, Parameters(a): Parameters<PCardPutVcard>) -> ToolResult {
        self.mcp
            .write_contacts_put_vcard(&a.workspace, &a.principal, &a.book, &a.vcard)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "contacts_get_entry",
        description = "One contact (canonical CBOR)",
        annotations(read_only_hint = true)
    )]
    fn contacts_get_entry(&self, Parameters(a): Parameters<PCardEntry>) -> ToolResult {
        self.mcp
            .read_contacts_get_entry(&a.workspace, &a.principal, &a.book, &a.uid)
            .map_err(err)
            .map(|o| {
                out_value(match o {
                    Some(e) => jbytes(&e.encode()),
                    None => Value::Null,
                })
            })
    }
    #[tool(
        name = "contacts_delete_entry",
        description = "Delete a contact; returns whether present"
    )]
    fn contacts_delete_entry(&self, Parameters(a): Parameters<PCardEntry>) -> ToolResult {
        self.mcp
            .write_contacts_delete_entry(&a.workspace, &a.principal, &a.book, &a.uid)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "contacts_list_entries",
        description = "All contacts (canonical CBOR each)",
        annotations(read_only_hint = true)
    )]
    fn contacts_list_entries(&self, Parameters(a): Parameters<PCardBook>) -> ToolResult {
        self.mcp
            .read_contacts_list_entries(&a.workspace, &a.principal, &a.book)
            .map_err(err)
            .and_then(|items| slice_results(items, &a.page))
            .map(|v| {
                out_value(Value::Array(
                    v.iter().map(|e| jbytes(&e.encode())).collect(),
                ))
            })
    }
    #[tool(
        name = "contacts_search",
        description = "Search contacts (canonical CBOR each)",
        annotations(read_only_hint = true)
    )]
    fn contacts_search(&self, Parameters(a): Parameters<PCardSearch>) -> ToolResult {
        self.mcp
            .read_contacts_search(&a.workspace, &a.principal, &a.book, &a.text)
            .map_err(err)
            .and_then(|items| slice_results(items, &a.page))
            .map(|v| {
                out_value(Value::Array(
                    v.iter().map(|e| jbytes(&e.encode())).collect(),
                ))
            })
    }
    #[tool(
        name = "contacts_to_vcard",
        description = "vCard serialization of a contact",
        annotations(read_only_hint = true)
    )]
    fn contacts_to_vcard(&self, Parameters(a): Parameters<PCardEntry>) -> ToolResult {
        self.mcp
            .read_contacts_to_vcard(&a.workspace, &a.principal, &a.book, &a.uid)
            .map_err(err)
            .and_then(ser)
    }

    // ===== mail =====
    #[tool(name = "mail_create_mailbox", description = "Create a mailbox")]
    fn mail_create_mailbox(&self, Parameters(a): Parameters<PMailCreateBox>) -> ToolResult {
        self.mcp
            .write_mail_create_mailbox(&a.workspace, &a.principal, &a.mailbox, &a.display_name)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "mail_get_mailbox",
        description = "Mailbox metadata",
        annotations(read_only_hint = true)
    )]
    fn mail_get_mailbox(&self, Parameters(a): Parameters<PMailBox>) -> ToolResult {
        self.mcp
            .read_mail_get_mailbox(&a.workspace, &a.principal, &a.mailbox)
            .map_err(err)
            .map(|o| {
                out_value(match o {
                    Some(m) => json!({"display_name": m.display_name}),
                    None => Value::Null,
                })
            })
    }
    #[tool(
        name = "mail_list_mailboxes",
        description = "Mailbox names",
        annotations(read_only_hint = true)
    )]
    fn mail_list_mailboxes(&self, Parameters(a): Parameters<PMailPrincipal>) -> ToolResult {
        self.mcp
            .read_mail_list_mailboxes(&a.workspace, &a.principal)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "mail_delete_mailbox",
        description = "Delete a mailbox; returns whether present"
    )]
    fn mail_delete_mailbox(&self, Parameters(a): Parameters<PMailBox>) -> ToolResult {
        self.mcp
            .write_mail_delete_mailbox(&a.workspace, &a.principal, &a.mailbox)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "mail_ingest_message",
        description = "Ingest a raw RFC 5322 message"
    )]
    fn mail_ingest_message(&self, Parameters(a): Parameters<PMailIngest>) -> ToolResult {
        self.mcp
            .write_mail_ingest_message(&a.workspace, &a.principal, &a.mailbox, &a.uid, &a.raw)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "mail_get_message",
        description = "Structured message record",
        annotations(read_only_hint = true)
    )]
    fn mail_get_message(&self, Parameters(a): Parameters<PMailMsg>) -> ToolResult {
        self.mcp
            .read_mail_get_message(&a.workspace, &a.principal, &a.mailbox, &a.uid)
            .map_err(err)
            .map(|o| {
                out_value(match o {
                    Some(m) => mail_msg_val(&m),
                    None => Value::Null,
                })
            })
    }
    #[tool(
        name = "mail_to_eml",
        description = "Raw RFC 5322 (.eml) bytes",
        annotations(read_only_hint = true)
    )]
    fn mail_to_eml(&self, Parameters(a): Parameters<PMailMsg>) -> ToolResult {
        self.mcp
            .read_mail_to_eml(&a.workspace, &a.principal, &a.mailbox, &a.uid)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "mail_delete_message",
        description = "Delete a message; returns whether present"
    )]
    fn mail_delete_message(&self, Parameters(a): Parameters<PMailMsg>) -> ToolResult {
        self.mcp
            .write_mail_delete_message(&a.workspace, &a.principal, &a.mailbox, &a.uid)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "mail_list_messages",
        description = "Structured records in a mailbox",
        annotations(read_only_hint = true)
    )]
    fn mail_list_messages(&self, Parameters(a): Parameters<PMailBox>) -> ToolResult {
        self.mcp
            .read_mail_list_messages(&a.workspace, &a.principal, &a.mailbox)
            .map_err(err)
            .and_then(|items| slice_results(items, &a.page))
            .map(|v| out_value(Value::Array(v.iter().map(mail_msg_val).collect())))
    }
    #[tool(
        name = "mail_get_flags",
        description = "Message flags",
        annotations(read_only_hint = true)
    )]
    fn mail_get_flags(&self, Parameters(a): Parameters<PMailMsg>) -> ToolResult {
        self.mcp
            .read_mail_get_flags(&a.workspace, &a.principal, &a.mailbox, &a.uid)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(name = "mail_set_flags", description = "Set message flags")]
    fn mail_set_flags(&self, Parameters(a): Parameters<PMailSetFlags>) -> ToolResult {
        self.mcp
            .write_mail_set_flags(&a.workspace, &a.principal, &a.mailbox, &a.uid, &a.flags)
            .map_err(err)
            .and_then(ser)
    }
    #[tool(
        name = "mail_search",
        description = "Search messages",
        annotations(read_only_hint = true)
    )]
    fn mail_search(&self, Parameters(a): Parameters<PMailSearch>) -> ToolResult {
        self.mcp
            .read_mail_search(&a.workspace, &a.principal, &a.mailbox, &a.text)
            .map_err(err)
            .and_then(|items| slice_results(items, &a.page))
            .map(|v| out_value(Value::Array(v.iter().map(mail_msg_val).collect())))
    }
}

#[derive(Deserialize, JsonSchema)]
struct PMerge2 {
    workspace: String,
    author: String,
    timestamp_ms: u64,
}

#[prompt_router]
impl LoomServer {
    #[prompt(name = "calendar_summarize_period")]
    fn pr_calendar_summarize_period(&self) -> Vec<PromptMessage> {
        prompt_messages("calendar_summarize_period")
    }
    #[prompt(name = "calendar_find_conflicts")]
    fn pr_calendar_find_conflicts(&self) -> Vec<PromptMessage> {
        prompt_messages("calendar_find_conflicts")
    }
    #[prompt(name = "calendar_schedule_event")]
    fn pr_calendar_schedule_event(&self) -> Vec<PromptMessage> {
        prompt_messages("calendar_schedule_event")
    }
    #[prompt(name = "calendar_agenda")]
    fn pr_calendar_agenda(&self) -> Vec<PromptMessage> {
        prompt_messages("calendar_agenda")
    }
    #[prompt(name = "contacts_find")]
    fn pr_contacts_find(&self) -> Vec<PromptMessage> {
        prompt_messages("contacts_find")
    }
    #[prompt(name = "contacts_deduplicate")]
    fn pr_contacts_deduplicate(&self) -> Vec<PromptMessage> {
        prompt_messages("contacts_deduplicate")
    }
    #[prompt(name = "contacts_enrich")]
    fn pr_contacts_enrich(&self) -> Vec<PromptMessage> {
        prompt_messages("contacts_enrich")
    }
    #[prompt(name = "mail_triage")]
    fn pr_mail_triage(&self) -> Vec<PromptMessage> {
        prompt_messages("mail_triage")
    }
    #[prompt(name = "mail_summarize_thread")]
    fn pr_mail_summarize_thread(&self) -> Vec<PromptMessage> {
        prompt_messages("mail_summarize_thread")
    }
    #[prompt(name = "mail_draft_reply")]
    fn pr_mail_draft_reply(&self) -> Vec<PromptMessage> {
        prompt_messages("mail_draft_reply")
    }
    #[prompt(name = "mail_find")]
    fn pr_mail_find(&self) -> Vec<PromptMessage> {
        prompt_messages("mail_find")
    }
    #[prompt(name = "vcs_summarize_changes")]
    fn pr_vcs_summarize_changes(&self) -> Vec<PromptMessage> {
        prompt_messages("vcs_summarize_changes")
    }
    #[prompt(name = "vcs_explain_conflict")]
    fn pr_vcs_explain_conflict(&self) -> Vec<PromptMessage> {
        prompt_messages("vcs_explain_conflict")
    }
    #[prompt(name = "vcs_blame")]
    fn pr_vcs_blame(&self) -> Vec<PromptMessage> {
        prompt_messages("vcs_blame")
    }
    #[prompt(name = "vcs_release_notes")]
    fn pr_vcs_release_notes(&self) -> Vec<PromptMessage> {
        prompt_messages("vcs_release_notes")
    }
    #[prompt(name = "fs_summarize_tree")]
    fn pr_fs_summarize_tree(&self) -> Vec<PromptMessage> {
        prompt_messages("fs_summarize_tree")
    }
    #[prompt(name = "fs_find")]
    fn pr_fs_find(&self) -> Vec<PromptMessage> {
        prompt_messages("fs_find")
    }
    #[prompt(name = "sql_ask")]
    fn pr_sql_ask(&self) -> Vec<PromptMessage> {
        prompt_messages("sql_ask")
    }
    #[prompt(name = "sql_schema_overview")]
    fn pr_sql_schema_overview(&self) -> Vec<PromptMessage> {
        prompt_messages("sql_schema_overview")
    }
    #[prompt(name = "timeseries_trend")]
    fn pr_timeseries_trend(&self) -> Vec<PromptMessage> {
        prompt_messages("timeseries_trend")
    }
    #[prompt(name = "ledger_audit")]
    fn pr_ledger_audit(&self) -> Vec<PromptMessage> {
        prompt_messages("ledger_audit")
    }
    #[prompt(name = "queue_inspect")]
    fn pr_queue_inspect(&self) -> Vec<PromptMessage> {
        prompt_messages("queue_inspect")
    }
    #[prompt(name = "document_summarize_collection")]
    fn pr_document_summarize_collection(&self) -> Vec<PromptMessage> {
        prompt_messages("document_summarize_collection")
    }
    #[prompt(name = "lifecycle_feature_ideate")]
    fn pr_lifecycle_feature_ideate(&self) -> Vec<PromptMessage> {
        prompt_messages("lifecycle_feature_ideate")
    }
    #[prompt(name = "lifecycle_feature_draft")]
    fn pr_lifecycle_feature_draft(&self) -> Vec<PromptMessage> {
        prompt_messages("lifecycle_feature_draft")
    }
    #[prompt(name = "lifecycle_feature_structure")]
    fn pr_lifecycle_feature_structure(&self) -> Vec<PromptMessage> {
        prompt_messages("lifecycle_feature_structure")
    }
    #[prompt(name = "lifecycle_feature_ready")]
    fn pr_lifecycle_feature_ready(&self) -> Vec<PromptMessage> {
        prompt_messages("lifecycle_feature_ready")
    }
    #[prompt(name = "lifecycle_feature_build")]
    fn pr_lifecycle_feature_build(&self) -> Vec<PromptMessage> {
        prompt_messages("lifecycle_feature_build")
    }
    #[prompt(name = "lifecycle_feature_done")]
    fn pr_lifecycle_feature_done(&self) -> Vec<PromptMessage> {
        prompt_messages("lifecycle_feature_done")
    }
    #[prompt(name = "lifecycle_bug_triage")]
    fn pr_lifecycle_bug_triage(&self) -> Vec<PromptMessage> {
        prompt_messages("lifecycle_bug_triage")
    }
    #[prompt(name = "lifecycle_bug_reproduce")]
    fn pr_lifecycle_bug_reproduce(&self) -> Vec<PromptMessage> {
        prompt_messages("lifecycle_bug_reproduce")
    }
    #[prompt(name = "lifecycle_bug_fix")]
    fn pr_lifecycle_bug_fix(&self) -> Vec<PromptMessage> {
        prompt_messages("lifecycle_bug_fix")
    }
    #[prompt(name = "lifecycle_bug_verify")]
    fn pr_lifecycle_bug_verify(&self) -> Vec<PromptMessage> {
        prompt_messages("lifecycle_bug_verify")
    }
    #[prompt(name = "lifecycle_bug_done")]
    fn pr_lifecycle_bug_done(&self) -> Vec<PromptMessage> {
        prompt_messages("lifecycle_bug_done")
    }
    #[prompt(name = "lifecycle_incident_triage")]
    fn pr_lifecycle_incident_triage(&self) -> Vec<PromptMessage> {
        prompt_messages("lifecycle_incident_triage")
    }
    #[prompt(name = "lifecycle_incident_mitigate")]
    fn pr_lifecycle_incident_mitigate(&self) -> Vec<PromptMessage> {
        prompt_messages("lifecycle_incident_mitigate")
    }
    #[prompt(name = "lifecycle_incident_resolve")]
    fn pr_lifecycle_incident_resolve(&self) -> Vec<PromptMessage> {
        prompt_messages("lifecycle_incident_resolve")
    }
    #[prompt(name = "lifecycle_incident_review")]
    fn pr_lifecycle_incident_review(&self) -> Vec<PromptMessage> {
        prompt_messages("lifecycle_incident_review")
    }
    #[prompt(name = "lifecycle_design_ideate")]
    fn pr_lifecycle_design_ideate(&self) -> Vec<PromptMessage> {
        prompt_messages("lifecycle_design_ideate")
    }
    #[prompt(name = "lifecycle_design_draft")]
    fn pr_lifecycle_design_draft(&self) -> Vec<PromptMessage> {
        prompt_messages("lifecycle_design_draft")
    }
    #[prompt(name = "lifecycle_design_review")]
    fn pr_lifecycle_design_review(&self) -> Vec<PromptMessage> {
        prompt_messages("lifecycle_design_review")
    }
    #[prompt(name = "lifecycle_design_accepted")]
    fn pr_lifecycle_design_accepted(&self) -> Vec<PromptMessage> {
        prompt_messages("lifecycle_design_accepted")
    }
    #[prompt(name = "lifecycle_archive")]
    fn pr_lifecycle_archive(&self) -> Vec<PromptMessage> {
        prompt_messages("lifecycle_archive")
    }
    #[prompt(name = "apps_author")]
    fn pr_apps_author(&self) -> Vec<PromptMessage> {
        prompt_messages("apps_author")
    }
    #[prompt(name = "apps_inspect")]
    fn pr_apps_inspect(&self) -> Vec<PromptMessage> {
        prompt_messages("apps_inspect")
    }
    #[prompt(name = "store_inventory")]
    fn pr_store_inventory(&self) -> Vec<PromptMessage> {
        prompt_messages("store_inventory")
    }
}

fn version_meta(addr: &str) -> Option<Meta> {
    let mut m = serde_json::Map::new();
    m.insert("version".to_string(), Value::String(addr.to_string()));
    Some(Meta(m))
}

fn blob_contents(
    uri: String,
    mime: &str,
    bytes: &[u8],
    version: &str,
) -> Result<ResourceContents, ErrorData> {
    let blob = resources::base64_encode(bytes);
    ensure_delivered_budget(
        &format!("resources/read {uri}"),
        blob.len(),
        DEFAULT_RESOURCE_READ_MAX_BYTES,
    )?;
    Ok(ResourceContents::BlobResourceContents {
        uri,
        mime_type: Some(mime.to_string()),
        blob,
        meta: version_meta(version),
    })
}

fn text_contents(
    uri: String,
    mime: &str,
    text: String,
    version: &str,
) -> Result<ResourceContents, ErrorData> {
    ensure_delivered_budget(
        &format!("resources/read {uri}"),
        text.len(),
        DEFAULT_RESOURCE_READ_MAX_BYTES,
    )?;
    Ok(ResourceContents::TextResourceContents {
        uri,
        mime_type: Some(mime.to_string()),
        text,
        meta: version_meta(version),
    })
}

fn app_resource_meta(meta: &AppMeta, version: Option<&str>) -> Meta {
    let mut root = serde_json::Map::new();
    if let Some(version) = version {
        root.insert("version".to_string(), Value::String(version.to_string()));
    }
    let mut ui = serde_json::Map::new();
    let mut csp = serde_json::Map::new();
    add_string_array(&mut csp, "connectDomains", &meta.csp.connect_domains);
    add_string_array(&mut csp, "resourceDomains", &meta.csp.resource_domains);
    add_string_array(&mut csp, "frameDomains", &meta.csp.frame_domains);
    add_string_array(&mut csp, "baseUriDomains", &meta.csp.base_uri_domains);
    if !csp.is_empty() {
        ui.insert("csp".to_string(), Value::Object(csp));
    }
    let mut permissions = serde_json::Map::new();
    add_permission(&mut permissions, "camera", meta.permissions.camera);
    add_permission(&mut permissions, "microphone", meta.permissions.microphone);
    add_permission(
        &mut permissions,
        "geolocation",
        meta.permissions.geolocation,
    );
    add_permission(
        &mut permissions,
        "clipboardWrite",
        meta.permissions.clipboard_write,
    );
    if !permissions.is_empty() {
        ui.insert("permissions".to_string(), Value::Object(permissions));
    }
    if let Some(domain) = &meta.domain {
        ui.insert("domain".to_string(), Value::String(domain.clone()));
    }
    if let Some(prefers_border) = meta.prefers_border {
        ui.insert("prefersBorder".to_string(), Value::Bool(prefers_border));
    }
    root.insert("ui".to_string(), Value::Object(ui));
    let mut loom = serde_json::Map::new();
    loom.insert(
        "processing".to_string(),
        Value::String(meta.processing.clone()),
    );
    root.insert("loom".to_string(), Value::Object(loom));
    Meta(root)
}

fn schema_bool_as_object(value: bool) -> Value {
    if value {
        json!({})
    } else {
        json!({ "not": {} })
    }
}

fn normalize_schema_child(value: &mut Value) {
    if let Some(bool_value) = value.as_bool() {
        *value = schema_bool_as_object(bool_value);
        return;
    }
    normalize_schema_node(value);
}

fn normalize_schema_node(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    for (key, child) in object {
        match key.as_str() {
            "const" | "default" | "enum" | "examples" => {}
            "$defs" | "definitions" | "dependentSchemas" | "patternProperties" | "properties" => {
                if let Some(children) = child.as_object_mut() {
                    for schema in children.values_mut() {
                        normalize_schema_child(schema);
                    }
                }
            }
            "additionalItems"
            | "additionalProperties"
            | "contains"
            | "else"
            | "if"
            | "items"
            | "not"
            | "propertyNames"
            | "then"
            | "unevaluatedItems"
            | "unevaluatedProperties" => normalize_schema_child(child),
            "allOf" | "anyOf" | "oneOf" | "prefixItems" => {
                if let Some(children) = child.as_array_mut() {
                    for schema in children {
                        normalize_schema_child(schema);
                    }
                }
            }
            _ => normalize_schema_node(child),
        }
    }
}

fn normalize_schema_object(schema: Arc<JsonObject>) -> Arc<JsonObject> {
    let mut value = Value::Object((*schema).clone());
    normalize_schema_node(&mut value);
    match value {
        Value::Object(object) => Arc::new(object),
        _ => schema,
    }
}

fn concrete_nullable_branch(value: &Value) -> Option<Value> {
    let object = value.as_object()?;
    if let Some(types) = object.get("type").and_then(Value::as_array) {
        let non_null: Vec<_> = types
            .iter()
            .filter_map(Value::as_str)
            .filter(|ty| *ty != "null")
            .collect();
        if non_null.len() == 1 {
            let mut concrete = object.clone();
            concrete.insert("type".to_string(), Value::String(non_null[0].to_string()));
            concrete.remove("default");
            return Some(Value::Object(concrete));
        }
    }
    for key in ["anyOf", "oneOf"] {
        let Some(branches) = object.get(key).and_then(Value::as_array) else {
            continue;
        };
        let mut concrete = None;
        for branch in branches {
            if branch.get("type").and_then(Value::as_str) == Some("null") {
                continue;
            }
            if concrete.replace(branch.clone()).is_some() {
                return None;
            }
        }
        if concrete.is_some() {
            return concrete;
        }
    }
    None
}

fn collapse_nullable_schema_properties(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if let Some(Value::Object(properties)) = object.get_mut("properties") {
                for property in properties.values_mut() {
                    if let Some(concrete) = concrete_nullable_branch(property) {
                        *property = concrete;
                    }
                    collapse_nullable_schema_properties(property);
                }
            }
            for (key, child) in object {
                if key == "properties" {
                    continue;
                }
                collapse_nullable_schema_properties(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                collapse_nullable_schema_properties(item);
            }
        }
        _ => {}
    }
}

fn collapse_nullable_input_properties(schema: &mut JsonObject) {
    let mut root = Value::Object(std::mem::take(schema));
    collapse_nullable_schema_properties(&mut root);
    if let Value::Object(object) = root {
        *schema = object;
    }
}

fn force_input_property_schema(tool: &mut Tool, property: &str, schema: Value) {
    let root = Arc::make_mut(&mut tool.input_schema);
    let Some(Value::Object(properties)) = root.get_mut("properties") else {
        return;
    };
    // Only replace a property the derived schema already declares; never invent a new one.
    if let Some(slot) = properties.get_mut(property) {
        *slot = schema;
    }
}

/// Model-facing input properties whose value is a JSON object/map but which `schemars` derives from
/// a `serde_json::Value`, `Map<String, Value>`, or structured-predicate field as an empty `{}`
/// schema. Stricter MCP clients can treat `{}` as "no shape" and stringify the object before
/// dispatch, corrupting the call. Forcing a direct object schema (`type: object`,
/// `additionalProperties: true`) keeps the object intact on the wire. Every entry here is asserted
/// object-typed by the
/// `model_tool_input_properties_are_not_empty_schemas` regression test; any new model-facing `{}`
/// property must be added here (a real object/map) or to that test's free-form allowlist.
const MODEL_TOOL_OBJECT_INPUT_FIELDS: &[(&str, &str)] = &[
    ("tickets_create", "fields"),
    ("tickets_update", "set_fields"),
    ("columnar_select", "predicate"),
    ("kv_range", "predicate"),
    ("document_query", "predicate"),
];

fn normalize_tool_schema_objects(tool: &mut Tool) {
    tool.input_schema = normalize_schema_object(tool.input_schema.clone());
    collapse_nullable_input_properties(Arc::make_mut(&mut tool.input_schema));
    for &(tool_name, property) in MODEL_TOOL_OBJECT_INPUT_FIELDS {
        if tool.name.as_ref() == tool_name {
            let schema = if property == "predicate" {
                open_object_schema()
            } else {
                object_schema()
            };
            force_input_property_schema(tool, property, schema);
        }
    }
    if let Some(output_schema) = tool.output_schema.as_mut() {
        *output_schema = normalize_schema_object(output_schema.clone());
    }
}

fn app_tool_meta(meta: &AppMeta, uri: &str) -> Meta {
    let mut root = app_resource_meta(meta, None).0;
    let ui = root
        .entry("ui".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if let Some(ui) = ui.as_object_mut() {
        ui.insert("resourceUri".to_string(), Value::String(uri.to_string()));
        // `_meta.ui.visibility` (model/app) from the app's declared `ui.visibility` list in
        // `_meta.md` (defaulting to both surfaces when unset).
        ui.insert(
            "visibility".to_string(),
            Value::Array(
                meta.visibility_surfaces()
                    .into_iter()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    root.insert("ui/resourceUri".to_string(), Value::String(uri.to_string()));
    root.insert("category".to_string(), Value::String("Apps".to_string()));
    Meta(root)
}

fn app_launch_segment(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        if b == b'/' {
            out.push('.');
        } else if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.') {
            out.push(char::from(b));
        } else {
            out.push('~');
            out.push_str(&format!("{b:02X}"));
        }
    }
    out
}

fn app_launch_tool_name(workspace: &str, app: &str, workspace_bound: bool) -> String {
    if workspace_bound {
        format!("{APP_LAUNCH_PREFIX}{}", app_launch_segment(app))
    } else {
        format!(
            "{APP_LAUNCH_PREFIX}{}.{}",
            app_launch_segment(workspace),
            app_launch_segment(app)
        )
    }
}

/// A process-unique ask id: creation time plus a monotone sequence.
fn next_ask_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static ASK_SEQ: AtomicU64 = AtomicU64::new(0);
    format!(
        "ask-{}-{}",
        crate::now_ms(),
        ASK_SEQ.fetch_add(1, Ordering::Relaxed)
    )
}

fn app_launch_payload(app: &crate::reads::McpAppResource, workspace_bound: bool) -> Value {
    json!({
        "workspace": app.workspace,
        "app": app.app,
        "uri": apps::app_uri(&app.workspace, &app.app, workspace_bound),
        "name": app.meta.name,
        "description": app.meta.description,
        "processing": app.meta.processing
    })
}

fn app_launcher_tool(app: &crate::reads::McpAppResource, workspace_bound: bool) -> Tool {
    let uri = apps::app_uri(&app.workspace, &app.app, workspace_bound);
    let tool_name = app_launch_tool_name(&app.workspace, &app.app, workspace_bound);
    let display_name = if workspace_bound {
        app.meta.name.clone()
    } else {
        format!("{}/{}", app.workspace, app.meta.name)
    };
    let annotations = ToolAnnotations::from_raw(
        Some(display_name.clone()),
        Some(true),
        None,
        None,
        Some(false),
    );
    let description = app
        .meta
        .description
        .clone()
        .unwrap_or_else(|| format!("Open Loom MCP App {display_name} from {uri}"));
    Tool::new(tool_name, description, empty_input_schema())
        .with_title(display_name)
        .with_raw_output_schema(app_launcher_output_schema())
        .with_annotations(annotations)
        .with_meta(app_tool_meta(&app.meta, &uri))
}

fn required_string_arg(args: &JsonObject, name: &str) -> Result<String, ErrorData> {
    args.get(name)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| ErrorData::invalid_params(format!("{name} is required"), None))
}

fn generic_app_open_tool(workspace_bound: bool) -> Tool {
    let title = "Open Loom App".to_string();
    let annotations =
        ToolAnnotations::from_raw(Some(title.clone()), Some(true), None, None, Some(false));
    Tool::new(
        APP_OPEN_TOOL,
        "Resolve a Loom MCP App by workspace and app id.",
        app_open_input_schema(workspace_bound),
    )
    .with_title(title)
    .with_raw_output_schema(app_launcher_output_schema())
    .with_annotations(annotations)
    .with_meta(Meta(serde_json::Map::from_iter([(
        "category".to_string(),
        Value::String("Apps".to_string()),
    )])))
}

fn app_extension_capabilities() -> ExtensionCapabilities {
    ExtensionCapabilities::from_iter([(
        MCP_UI_EXTENSION.to_string(),
        JsonObject::from_iter([
            ("htmlResources".to_string(), Value::Bool(true)),
            ("toolResourceUri".to_string(), Value::Bool(true)),
            (
                "iframeJsonRpcBridgeRequired".to_string(),
                Value::Bool(false),
            ),
        ]),
    )])
}

fn add_string_array(target: &mut serde_json::Map<String, Value>, key: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }
    target.insert(
        key.to_string(),
        Value::Array(values.iter().cloned().map(Value::String).collect()),
    );
}

fn add_permission(target: &mut serde_json::Map<String, Value>, key: &str, enabled: bool) {
    if enabled {
        target.insert(key.to_string(), Value::Object(serde_json::Map::new()));
    }
}

fn not_found(uri: &str) -> ErrorData {
    ErrorData::invalid_params(format!("resource not found: {uri}"), None)
}

/// The JSON-RPC "request cancelled" error (code -32800) for a tool whose request was cancelled before
/// it ran.
fn cancelled_error(tool: &str) -> ErrorData {
    ErrorData::new(
        ErrorCode(-32800),
        format!("request cancelled: {tool}"),
        None,
    )
}

fn shutting_down_error() -> ErrorData {
    ErrorData::internal_error("server is shutting down", None)
}

#[derive(Clone)]
struct ShutdownController {
    inner: Arc<ShutdownInner>,
}

struct ShutdownInner {
    state: Mutex<ShutdownState>,
    idle: Notify,
}

#[derive(Default)]
struct ShutdownState {
    draining: bool,
    active_requests: usize,
}

struct ActiveRequest {
    shutdown: ShutdownController,
}

impl ShutdownController {
    fn new() -> Self {
        Self {
            inner: Arc::new(ShutdownInner {
                state: Mutex::new(ShutdownState::default()),
                idle: Notify::new(),
            }),
        }
    }

    fn begin_request(&self) -> Result<ActiveRequest, ErrorData> {
        let mut state = self.inner.state.lock().expect("shutdown state lock");
        if state.draining {
            return Err(shutting_down_error());
        }
        state.active_requests += 1;
        Ok(ActiveRequest {
            shutdown: self.clone(),
        })
    }

    fn start_draining(&self) {
        let mut state = self.inner.state.lock().expect("shutdown state lock");
        state.draining = true;
        if state.active_requests == 0 {
            self.inner.idle.notify_waiters();
        }
    }

    async fn wait_idle(&self) {
        loop {
            let idle = {
                let state = self.inner.state.lock().expect("shutdown state lock");
                state.active_requests == 0
            };
            if idle {
                return;
            }
            self.inner.idle.notified().await;
        }
    }
}

impl Drop for ActiveRequest {
    fn drop(&mut self) {
        let mut state = self
            .shutdown
            .inner
            .state
            .lock()
            .expect("shutdown state lock");
        state.active_requests = state.active_requests.saturating_sub(1);
        if state.draining && state.active_requests == 0 {
            self.shutdown.inner.idle.notify_waiters();
        }
    }
}

/// A progress notification for a tool call: `0/1` at the start, `1/1` on completion.
fn progress_param(token: ProgressToken, done: bool, tool: &str) -> ProgressNotificationParam {
    let (progress, phase) = if done {
        (1.0, "completed")
    } else {
        (0.0, "started")
    };
    ProgressNotificationParam {
        progress_token: token,
        progress,
        total: Some(1.0),
        message: Some(format!("{tool}: {phase}")),
    }
}

impl LoomServer {
    /// Maximum items returned in one `*/list` page (MCP pagination).
    const PAGE_SIZE: usize = 100;

    /// Opaque-cursor pagination: the cursor is the next item's index as a decimal string. Returns the
    /// page (at most [`Self::PAGE_SIZE`] items) and the cursor for the following page (or `None` at the
    /// end). An unparsable or out-of-range cursor is an invalid-params error per the MCP spec.
    fn paginate<T>(
        items: Vec<T>,
        cursor: Option<String>,
    ) -> Result<(Vec<T>, Option<String>), ErrorData> {
        let start = match cursor {
            None => 0,
            Some(c) => c
                .parse::<usize>()
                .ok()
                .filter(|&n| n <= items.len())
                .ok_or_else(|| {
                    ErrorData::invalid_params(format!("invalid pagination cursor: {c}"), None)
                })?,
        };
        let end = start.saturating_add(Self::PAGE_SIZE).min(items.len());
        let next = (end < items.len()).then(|| end.to_string());
        let page = items
            .into_iter()
            .skip(start)
            .take(Self::PAGE_SIZE)
            .collect();
        Ok((page, next))
    }

    /// Whether a registered tool is annotated destructive (its `destructive_hint` is `Some(true)`).
    #[cfg(test)]
    fn is_destructive_tool(&self, name: &str) -> bool {
        self.tool_router
            .get(name)
            .and_then(|t| t.annotations.as_ref())
            .and_then(|a| a.destructive_hint)
            .unwrap_or(false)
    }

    /// Argument autocompletion for `completion/complete`. The completable argument shared by the prompt
    /// surface and the `loom://` resource templates is `workspace`; we resolve live workspace names
    /// through the facade (PEP-gated) and prefix-filter them by the partial value, capped at the MCP
    /// per-response maximum. Other arguments have no enumerable domain and return empty.
    fn complete_argument(
        &self,
        _reference: &Reference,
        argument: &ArgumentInfo,
    ) -> Result<Vec<String>, ErrorData> {
        // A bound workspace is not an open argument, so there is nothing to complete.
        if argument.name != "workspace" || self.binding.workspace.is_some() {
            return Ok(Vec::new());
        }
        let names = self.mcp.read_workspace_list().map_err(err)?;
        Ok(names
            .into_iter()
            .map(|n| n.name)
            .filter(|name| name.starts_with(&argument.value))
            .take(CompletionInfo::MAX_VALUES)
            .collect())
    }

    fn resource_templates(&self) -> Vec<ResourceTemplate> {
        resources::TEMPLATES
            .iter()
            .map(|t| {
                let uri_template = self.scoped_resource_template(t.uri_template);
                RawResourceTemplate {
                    uri_template,
                    name: t.name.to_string(),
                    title: Some(t.name.to_string()),
                    description: Some(t.description.to_string()),
                    mime_type: Some(t.mime_type.to_string()),
                    icons: None,
                }
                .no_annotation()
            })
            .collect()
    }

    fn scoped_resource_template(&self, template: &str) -> String {
        let mut out = template.to_string();
        if self.binding.workspace.is_some() {
            out = out.replace("loom://{workspace}/", "loom://");
        }
        if area_template_is_per_principal(&out) {
            out = out.replace("/principal/{principal}", "");
            out = out.replace("/{principal}", "");
        }
        if self.binding.collection.is_some() {
            out = out
                .replace("/{collection}", "")
                .replace("/{book}", "")
                .replace("/{mailbox}", "");
        }
        out
    }

    /// One browsable resource per workspace (`loom://<name>/`).
    fn list_workspace_resources(&self) -> Result<Vec<Resource>, ErrorData> {
        let names = self.mcp.read_workspace_list().map_err(err)?;
        let mut resources = Vec::new();
        for n in names {
            if let Some(ns) = &self.binding.workspace {
                if !workspace_matches(&n, ns) {
                    continue;
                }
                let mut r = RawResource::new("loom://".to_string(), n.name.clone());
                r.title = Some(format!("Workspace: {}", n.name));
                r.description = Some(format!("facets: {}", n.facets.join(", ")));
                resources.push(r.no_annotation());
            } else {
                let mut r = RawResource::new(format!("loom://{}/", n.name), n.name.clone());
                r.title = Some(format!("Workspace: {}", n.name));
                r.description = Some(format!("facets: {}", n.facets.join(", ")));
                resources.push(r.no_annotation());
            }
        }
        Ok(resources)
    }

    fn list_app_resources(&self) -> Result<Vec<Resource>, ErrorData> {
        let apps = self.mcp.read_mcp_app_list().map_err(err)?;
        let mut out = Vec::new();
        for app in apps {
            if let Some(ns) = &self.binding.workspace
                && app.workspace != *ns
            {
                continue;
            }
            let uri = apps::app_uri(&app.workspace, &app.app, self.binding.workspace.is_some());
            let display_name = if self.binding.workspace.is_some() {
                app.meta.name.clone()
            } else {
                format!("{}/{}", app.workspace, app.meta.name)
            };
            let mut r = RawResource::new(uri, display_name.clone());
            r.title = Some(display_name);
            r.description = app.meta.description.clone();
            r.mime_type = Some(apps::APP_MIME.to_string());
            r.meta = Some(app_resource_meta(&app.meta, None));
            out.push(r.no_annotation());
        }
        Ok(out)
    }

    fn visible_app_resources(&self) -> Result<Vec<crate::reads::McpAppResource>, LoomError> {
        let apps = self.mcp.read_mcp_app_list()?;
        Ok(apps
            .into_iter()
            .filter(|app| {
                self.binding
                    .workspace
                    .as_ref()
                    .is_none_or(|ns| app.workspace == *ns)
            })
            .collect())
    }

    /// Reverse of [`sanitize_tool_name`]: map a wire tool name (as a host sends it on
    /// `tools/call`) back to its registered canonical name.
    ///
    /// A single rule covers every case: normalize both the incoming wire name and each known
    /// canonical name through [`sanitize_tool_name`], then match. Because sanitization is
    /// idempotent this resolves underscore-native curated tools, dynamic app-launcher tools
    /// (whose slugs may contain dots), and legacy dotted callers (stdio/Inspector) uniformly,
    /// with no separate passthrough branch. Unknown names are returned unchanged so the router
    /// can surface a normal "unknown tool" error.
    fn canonical_tool_name(&self, wire: &str) -> String {
        let target = sanitize_tool_name(wire);
        for tool in self.tool_router.list_all().iter() {
            if sanitize_tool_name(&tool.name) == target {
                return tool.name.to_string();
            }
        }
        if let Ok(launchers) = self.list_app_launcher_tools() {
            for tool in launchers.iter() {
                if sanitize_tool_name(&tool.name) == target {
                    return tool.name.to_string();
                }
            }
        }
        wire.to_string()
    }

    fn list_app_launcher_tools(&self) -> Result<Vec<Tool>, ErrorData> {
        let workspace_bound = self.binding.workspace.is_some();
        let resources = match self.visible_app_resources() {
            Ok(resources) => resources,
            Err(e) if hides_app_launcher_tools(&e) => return Ok(Vec::new()),
            Err(e) => return Err(err(e)),
        };
        let mut tools = vec![generic_app_open_tool(workspace_bound)];
        tools.extend(
            resources
                .iter()
                .map(|app| app_launcher_tool(app, workspace_bound)),
        );
        Ok(tools)
    }

    fn get_app_launcher_tool(&self, name: &str) -> Option<Tool> {
        if name == APP_OPEN_TOOL {
            return Some(generic_app_open_tool(self.binding.workspace.is_some()));
        }
        if !name.starts_with(APP_LAUNCH_PREFIX) {
            return None;
        }
        let workspace_bound = self.binding.workspace.is_some();
        self.visible_app_resources()
            .ok()?
            .into_iter()
            .find_map(|app| {
                (app_launch_tool_name(&app.workspace, &app.app, workspace_bound) == name)
                    .then(|| app_launcher_tool(&app, workspace_bound))
            })
    }

    fn regular_tool_visible(&self, spec: &ToolSpec) -> Result<bool, ErrorData> {
        if !self.binding.allow_writes && spec.kind == ToolKind::Write {
            return Ok(false);
        }
        self.mcp
            .store()
            .read(|loom| {
                let active_tools = self.active_lifecycle_tools_in_loom(loom);
                Ok(
                    self.lifecycle_surface_allows_tool(spec.name, active_tools.as_ref())
                        && self.regular_tool_spec_visible_in_loom(loom, spec),
                )
            })
            .map_err(err)
    }

    fn active_lifecycle_context(&self) -> Option<ActiveLifecycleContext> {
        self.active_lifecycle
            .lock()
            .expect("active lifecycle lock")
            .clone()
    }

    fn active_lifecycle_tools_in_loom(&self, loom: &Loom<FileStore>) -> Option<BTreeSet<String>> {
        let active = self.active_lifecycle_context()?;
        let workspace = loom
            .registry()
            .open(&workspace_selector(&active.workspace))
            .ok()?;
        let surface = loom_lifecycle::current_surface(
            loom,
            workspace,
            &active.workspace_id,
            &active.instance_id,
        )
        .ok()?;
        Some(
            surface
                .surfaced_tools
                .iter()
                .map(|name| lifecycle_surface_tool_name(name))
                .collect(),
        )
    }

    fn lifecycle_surface_allows_tool(
        &self,
        name: &str,
        active_tools: Option<&BTreeSet<String>>,
    ) -> bool {
        let Some(active_tools) = active_tools else {
            return true;
        };
        lifecycle_control_tool(name) || active_tools.contains(name)
    }

    fn regular_tool_spec_visible_in_loom(&self, loom: &Loom<FileStore>, spec: &ToolSpec) -> bool {
        if !self.binding.allow_writes && spec.kind == ToolKind::Write {
            return false;
        }
        if self.binding.workspace.is_none() {
            return true;
        }
        let Some(domain) = tool_area_domain(spec.area) else {
            return match spec.kind {
                ToolKind::Read => true,
                ToolKind::Write => loom.authorize_global_admin().is_ok(),
            };
        };
        self.regular_tool_visible_in_loom(loom, domain, tool_right(spec.kind))
    }

    fn regular_tool_visible_in_loom(
        &self,
        loom: &Loom<FileStore>,
        domain: AclDomain,
        right: AclRight,
    ) -> bool {
        if let Some(selector) = &self.binding.workspace {
            let Ok(ns) = loom.registry().open(&workspace_selector(selector)) else {
                return false;
            };
            return loom.authorize_domain(ns, domain, right).is_ok();
        }
        loom.registry()
            .list(None)
            .into_iter()
            .any(|ns| loom.authorize_domain(ns.id, domain, right).is_ok())
    }

    fn list_regular_tools(&self) -> Result<Vec<Tool>, ErrorData> {
        self.mcp
            .store()
            .read(|loom| {
                let active_tools = self.active_lifecycle_tools_in_loom(loom);
                Ok(self
                    .tool_router
                    .list_all()
                    .into_iter()
                    .filter(|tool| {
                        crate::tools::tool(&tool.name).is_some_and(|spec| {
                            self.lifecycle_surface_allows_tool(spec.name, active_tools.as_ref())
                                && self.regular_tool_spec_visible_in_loom(loom, spec)
                        })
                    })
                    .collect())
            })
            .map_err(err)
    }

    fn visible_model_tools(&self) -> Result<Vec<Tool>, ErrorData> {
        let mut tools = self.list_app_launcher_tools()?;
        tools.extend(self.list_regular_tools()?);
        let active_tools = self
            .mcp
            .store()
            .read(|loom| Ok(self.active_lifecycle_tools_in_loom(loom)))
            .map_err(err)?;
        tools.retain(|tool| self.lifecycle_surface_allows_tool(&tool.name, active_tools.as_ref()));
        tools.retain(|tool| !tool_hidden_from_model(tool));
        Ok(tools)
    }

    fn listed_model_tools(&self) -> Result<Vec<Tool>, ErrorData> {
        let mut tools = self.visible_model_tools()?;
        for tool in &mut tools {
            normalize_tool_schema_objects(tool);
            tool.name = sanitize_tool_name(&tool.name).into();
        }
        Ok(tools)
    }

    fn listed_model_tools_result(&self) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: self.listed_model_tools()?,
            next_cursor: None,
            ..Default::default()
        })
    }

    fn regular_tool_visible_by_name(&self, name: &str) -> Result<bool, ErrorData> {
        let Some(spec) = crate::tools::tool(name) else {
            return Ok(false);
        };
        self.regular_tool_visible(spec)
    }

    fn prepare_app_tool_call(
        &self,
        args: PAppCallTool,
    ) -> Result<(String, Option<JsonObject>), ErrorData> {
        let target = self.resolve_resource_uri(&args.app_uri).ok_or_else(|| {
            ErrorData::invalid_params(format!("unknown MCP app resource {}", args.app_uri), None)
        })?;
        let ResourceTarget::App(app_target) = target else {
            return Err(ErrorData::invalid_params(
                format!("resource {} is not an MCP app", args.app_uri),
                None,
            ));
        };
        let visible = self
            .visible_app_resources()
            .map_err(err)?
            .into_iter()
            .any(|app| app.workspace == app_target.workspace && app.app == app_target.app);
        if !visible {
            return Err(ErrorData::invalid_params(
                format!("MCP app resource {} is not visible", args.app_uri),
                None,
            ));
        }
        let tool = self.canonical_tool_name(&args.tool);
        if APP_ONLY_TOOLS.contains(&tool.as_str()) || self.get_app_launcher_tool(&tool).is_some() {
            return Err(ErrorData::invalid_params(
                format!("MCP app cannot call app-only tool {tool} through apps_call_tool"),
                None,
            ));
        }
        if crate::tools::tool(&tool).is_none() {
            return Err(ErrorData::invalid_params(
                format!("MCP app requested unknown tool {tool}"),
                None,
            ));
        }
        Ok((tool, args.arguments))
    }

    fn call_app_launcher_tool(
        &self,
        name: &str,
        arguments: Option<JsonObject>,
    ) -> Result<Option<CallToolResult>, ErrorData> {
        if name == APP_OPEN_TOOL {
            return self.call_generic_app_open(arguments).map(Some);
        }
        if !name.starts_with(APP_LAUNCH_PREFIX) {
            return Ok(None);
        }
        let workspace_bound = self.binding.workspace.is_some();
        let Some(app) = self
            .visible_app_resources()
            .map_err(err)?
            .into_iter()
            .find(|app| app_launch_tool_name(&app.workspace, &app.app, workspace_bound) == name)
        else {
            return Err(ErrorData::invalid_params(
                format!("unknown app launcher tool {name}"),
                None,
            ));
        };
        let uri = apps::app_uri(&app.workspace, &app.app, workspace_bound);
        Ok(Some(
            CallToolResult::structured(json!({
                "value": app_launch_payload(&app, workspace_bound)
            }))
            .with_meta(Some(app_tool_meta(&app.meta, &uri))),
        ))
    }

    fn call_generic_app_open(
        &self,
        arguments: Option<JsonObject>,
    ) -> Result<CallToolResult, ErrorData> {
        let arguments = arguments.unwrap_or_default();
        let app = required_string_arg(&arguments, "app")?;
        let workspace = if let Some(workspace) = &self.binding.workspace {
            workspace.clone()
        } else {
            required_string_arg(&arguments, "workspace")?
        };
        let Some(app) = self.mcp.read_mcp_app_show(&workspace, &app).map_err(err)? else {
            return Err(ErrorData::invalid_params(
                format!("unknown MCP app {workspace}/{app}"),
                None,
            ));
        };
        let workspace_bound = self.binding.workspace.is_some();
        let uri = apps::app_uri(&app.workspace, &app.app, workspace_bound);
        Ok(CallToolResult::structured(json!({
            "value": app_launch_payload(&app, workspace_bound)
        }))
        .with_meta(Some(app_tool_meta(&app.meta, &uri))))
    }

    fn app_template_bindings(
        &self,
        target: &apps::AppTarget,
        meta: &apps::AppMeta,
    ) -> Result<loom_templates::TemplateBindings, ErrorData> {
        let mut bindings = loom_templates::TemplateBindings::default();
        // Expose `_meta.md` under the template `meta.*` root (parallel to `loom.*`), mirroring the
        // front-matter key paths so a template reads the same names the author wrote:
        // `meta.name`, `meta.ui.visibility`, `meta.ui.availableDisplayModes`, `meta.ui.prefersBorder`,
        // `meta.ui.csp.connectDomains`, `meta.ui.permissions.camera`, `meta.loom.processing`, ...
        bindings = bindings.with_meta(json!({
            "name": meta.name,
            "description": meta.description,
            "mimeType": meta.mime_type,
            "loom": { "processing": meta.processing },
            "ui": {
                "visibility": meta.visibility_surfaces(),
                "availableDisplayModes": meta.display_modes(),
                "prefersBorder": meta.prefers_border,
                "domain": meta.domain,
                "csp": {
                    "connectDomains": meta.csp.connect_domains,
                    "resourceDomains": meta.csp.resource_domains,
                    "frameDomains": meta.csp.frame_domains,
                    "baseUriDomains": meta.csp.base_uri_domains,
                },
                "permissions": {
                    "camera": meta.permissions.camera,
                    "microphone": meta.permissions.microphone,
                    "geolocation": meta.permissions.geolocation,
                    "clipboardWrite": meta.permissions.clipboard_write,
                },
            },
        }));
        bindings = bindings.with_loom_value(
            "app_shell",
            json!({
                "css": apps::app_shell_css(),
            }),
        );
        if target.internal && target.app == apps::INTERNAL_VCS_APP {
            bindings =
                bindings.with_loom_value("vcs", self.internal_vcs_app_data(&target.workspace)?);
        }
        if target.internal && target.app == apps::INTERNAL_DECISIONS_APP {
            bindings = bindings.with_loom_value(
                "ask",
                self.internal_ask_app_data(&target.workspace, target.instance.as_deref())?,
            );
        }
        if target.internal && target.app == apps::DIRECTED_GRAPH_APP {
            bindings = bindings.with_loom_value(
                "graph",
                self.internal_directed_graph_app_data(&target.workspace)?,
            );
        }
        if target.internal && Self::pages_app_id(&target.app) {
            bindings = bindings.with_loom_value(
                "pages",
                self.internal_pages_app_data(
                    &target.workspace,
                    &target.app,
                    target.instance.as_deref(),
                )?,
            );
        }
        if target.internal && Self::tickets_app_id(&target.app) {
            bindings = bindings.with_loom_value(
                "tickets",
                self.internal_tickets_app_data(
                    &target.workspace,
                    &target.app,
                    target.instance.as_deref(),
                )?,
            );
        }
        if target.internal && Self::chat_app_id(&target.app) {
            bindings = bindings.with_loom_value(
                "chat",
                self.internal_chat_app_data(
                    &target.workspace,
                    &target.app,
                    target.instance.as_deref(),
                )?,
            );
        }
        if target.internal && Self::drive_app_id(&target.app) {
            bindings = bindings.with_loom_value(
                "drive",
                self.internal_drive_app_data(
                    &target.workspace,
                    &target.app,
                    target.instance.as_deref(),
                )?,
            );
        }
        if target.internal && Self::meetings_app_id(&target.app) {
            bindings = bindings.with_loom_value(
                "meetings",
                self.internal_meetings_app_data(
                    &target.workspace,
                    &target.app,
                    target.instance.as_deref(),
                )?,
            );
        }
        Ok(bindings)
    }

    /// The `loom.ask` template binding: the instance's ask document when the URI carries an
    /// instance segment, otherwise the `current` pointer (the most recently begun ask).
    fn internal_ask_app_data(
        &self,
        workspace: &str,
        instance: Option<&str>,
    ) -> Result<Value, ErrorData> {
        let selected = self.mcp.read_workspace_get(workspace).map_err(err)?;
        let current = self.read_ask_doc(workspace, instance.unwrap_or(ASK_CURRENT_DOC))?;
        Ok(json!({
            "workspace": selected,
            "current": current,
        }))
    }

    fn internal_directed_graph_app_data(&self, workspace: &str) -> Result<Value, ErrorData> {
        let selected = self.mcp.read_workspace_get(workspace).map_err(err)?;
        let definition = loom_substrate::surfaces::core_surface_app(
            loom_substrate::surfaces::CoreSurfaceAppKind::DirectedGraph,
            workspace,
        )
        .map_err(err)?;
        let mut catalog = loom_substrate::surfaces::surface_app_catalog(workspace).map_err(err)?;
        catalog.extend(
            loom_substrate::surfaces::meeting_memory_surface_catalog(workspace).map_err(err)?,
        );
        Ok(json!({
            "workspace": selected,
            "definition": {
                "app_id": definition.app_id,
                "display_name": definition.display_name,
                "resource_uri": definition.resource_uri,
                "projection_refs": definition.projection_refs,
                "read_tools": definition.read_tools,
                "write_tools": definition.write_tools,
                "subscription_refs": definition.subscription_refs
            },
            "catalog": {
                "apps": catalog.len()
            },
            "nodes": Self::directed_graph_catalog_nodes(&catalog),
            "edges": Self::directed_graph_catalog_edges(&catalog)
        }))
    }

    fn internal_pages_app_data(
        &self,
        workspace: &str,
        app: &str,
        instance: Option<&str>,
    ) -> Result<Value, ErrorData> {
        let selected = self.mcp.read_workspace_get(workspace).map_err(err)?;
        let profile_id = workspace_profile_id(&self.mcp, workspace).map_err(err)?;
        let app_uri =
            apps::app_uri_with_instance(workspace, app, instance, self.binding.workspace.is_some());
        let definition = self
            .pages_app_definition(app, workspace)
            .ok_or_else(|| ErrorData::invalid_params(format!("unknown Pages app {app}"), None))?
            .map_err(err)?;
        let spaces = match self.mcp.read_spaces_list(workspace, &profile_id) {
            Ok(spaces) => serde_json::to_value(spaces)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
            Err(e) if e.code == Code::NotFound => json!([]),
            Err(e) => return Err(err(e)),
        };
        let pages = match self.mcp.read_pages_list(workspace, &profile_id) {
            Ok(pages) => serde_json::to_value(pages)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
            Err(e) if e.code == Code::NotFound => json!([]),
            Err(e) => return Err(err(e)),
        };
        let structures = match self.mcp.read_structures_list(workspace, &profile_id) {
            Ok(structures) => serde_json::to_value(structures)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
            Err(e) if e.code == Code::NotFound => json!([]),
            Err(e) => return Err(err(e)),
        };
        let mut page = Value::Null;
        let mut history = json!([]);
        let mut backlinks = Value::Null;
        let mut structure = Value::Null;
        if let Some(instance) = instance {
            let (kind, id) = instance.split_once('/').ok_or_else(|| {
                ErrorData::invalid_params(format!("invalid Pages app instance {instance}"), None)
            })?;
            match (app, kind) {
                (apps::DOCUMENT_VIEWER_APP, "page") => {
                    page = serde_json::to_value(
                        self.mcp
                            .read_pages_get(workspace, &profile_id, id)
                            .map_err(err)?,
                    )
                    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
                    history = serde_json::to_value(
                        self.mcp
                            .read_pages_history(workspace, &profile_id, id)
                            .map_err(err)?,
                    )
                    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
                    backlinks = serde_json::to_value(
                        self.mcp
                            .read_substrate_refs(workspace, &format!("page:{id}"))
                            .map_err(err)?,
                    )
                    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
                }
                (apps::MIND_MAP_APP | apps::CANVAS_APP | apps::DIAGRAM_EDITOR_APP, "structure") => {
                    structure = serde_json::to_value(
                        self.mcp
                            .read_structures_get(workspace, &profile_id, id)
                            .map_err(err)?,
                    )
                    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
                }
                _ => {
                    return Err(ErrorData::invalid_params(
                        format!("invalid Pages app instance {app}/{instance}"),
                        None,
                    ));
                }
            }
        }
        Ok(json!({
            "app_uri": app_uri,
            "workspace": selected,
            "profile_id": profile_id,
            "definition": {
                "app_id": definition.app_id,
                "display_name": definition.display_name,
                "resource_uri": definition.resource_uri,
                "projection_refs": definition.projection_refs,
                "read_tools": definition.read_tools,
                "write_tools": definition.write_tools,
                "subscription_refs": definition.subscription_refs
            },
            "spaces": spaces,
            "pages": pages,
            "structures": structures,
            "page": page,
            "history": history,
            "backlinks": backlinks,
            "structure": structure,
        }))
    }

    fn pages_app_id(app: &str) -> bool {
        matches!(
            app,
            apps::DOCUMENT_VIEWER_APP
                | apps::MIND_MAP_APP
                | apps::CANVAS_APP
                | apps::DIAGRAM_EDITOR_APP
        )
    }

    fn pages_app_definition(
        &self,
        app: &str,
        workspace: &str,
    ) -> Option<Result<loom_substrate::surfaces::SurfaceAppDefinition, LoomError>> {
        match app {
            apps::DOCUMENT_VIEWER_APP => Some(loom_substrate::surfaces::core_surface_app(
                loom_substrate::surfaces::CoreSurfaceAppKind::DocumentViewer,
                workspace,
            )),
            apps::MIND_MAP_APP => Some(loom_substrate::surfaces::catalog_surface_app(
                loom_substrate::surfaces::CatalogSurfaceAppKind::MindMap,
                workspace,
            )),
            apps::CANVAS_APP => Some(loom_substrate::surfaces::catalog_surface_app(
                loom_substrate::surfaces::CatalogSurfaceAppKind::Canvas,
                workspace,
            )),
            apps::DIAGRAM_EDITOR_APP => Some(loom_substrate::surfaces::catalog_surface_app(
                loom_substrate::surfaces::CatalogSurfaceAppKind::DiagramEditor,
                workspace,
            )),
            _ => None,
        }
    }

    fn internal_tickets_app_data(
        &self,
        workspace: &str,
        app: &str,
        instance: Option<&str>,
    ) -> Result<Value, ErrorData> {
        let selected = self.mcp.read_workspace_get(workspace).map_err(err)?;
        let profile_id = workspace_profile_id(&self.mcp, workspace).map_err(err)?;
        let app_uri =
            apps::app_uri_with_instance(workspace, app, instance, self.binding.workspace.is_some());
        let definition = self
            .tickets_app_definition(app, workspace)
            .ok_or_else(|| ErrorData::invalid_params(format!("unknown Tickets app {app}"), None))?
            .map_err(err)?;
        let tickets = match self.mcp.read_tickets_list(workspace, &profile_id, None) {
            Ok(tickets) => serde_json::to_value(tickets)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
            Err(e) if e.code == Code::NotFound => json!([]),
            Err(e) => return Err(err(e)),
        };
        let (lanes, lane_diagnostics) = match self.mcp.read_lanes_list_with_diagnostics(workspace) {
            Ok((lanes, diagnostics)) => {
                let lanes = serde_json::to_value(lanes)
                    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
                let diagnostics = serde_json::to_value(diagnostics)
                    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
                (lanes, diagnostics)
            }
            Err(e) if e.code == Code::NotFound => (json!([]), json!([])),
            Err(e) => return Err(err(e)),
        };
        let boards = match self
            .mcp
            .read_tickets_boards_list(workspace, &profile_id, false)
        {
            Ok(boards) => serde_json::to_value(boards)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
            Err(e) if e.code == Code::NotFound => json!([]),
            Err(e) => return Err(err(e)),
        };
        let mut ticket = Value::Null;
        let mut history = json!([]);
        let mut refs = Value::Null;
        if let Some(instance) = instance {
            let (kind, id) = instance.split_once('/').ok_or_else(|| {
                ErrorData::invalid_params(format!("invalid Tickets app instance {instance}"), None)
            })?;
            if app != apps::TICKET_DETAILS_APP || kind != "ticket" {
                return Err(ErrorData::invalid_params(
                    format!("invalid Tickets app instance {app}/{instance}"),
                    None,
                ));
            }
            ticket = serde_json::to_value(
                self.mcp
                    .read_tickets_get(workspace, &profile_id, id, None)
                    .map_err(err)?,
            )
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
            history = serde_json::to_value(
                self.mcp
                    .read_tickets_history(workspace, &profile_id, Some(id))
                    .map_err(err)?,
            )
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
            refs = serde_json::to_value(
                self.mcp
                    .read_substrate_refs(workspace, &format!("ticket:{id}"))
                    .map_err(err)?,
            )
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        }
        Ok(json!({
            "app_uri": app_uri,
            "workspace": selected,
            "profile_id": profile_id,
            "definition": {
                "app_id": definition.app_id,
                "display_name": definition.display_name,
                "resource_uri": definition.resource_uri,
                "projection_refs": definition.projection_refs,
                "read_tools": definition.read_tools,
                "write_tools": definition.write_tools,
                "subscription_refs": definition.subscription_refs
            },
            "tickets": tickets,
            "ticket": ticket,
            "history": history,
            "refs": refs,
            "lanes": lanes,
            "lane_diagnostics": lane_diagnostics,
            "boards": boards,
        }))
    }

    fn tickets_app_id(app: &str) -> bool {
        matches!(
            app,
            apps::TICKET_DETAILS_APP
                | apps::BOARD_APP
                | apps::ROADMAP_APP
                | apps::SPRINT_PLANNER_APP
                | apps::BACKLOG_TRIAGE_APP
                | apps::DASHBOARDS_APP
        )
    }

    fn tickets_app_definition(
        &self,
        app: &str,
        workspace: &str,
    ) -> Option<Result<loom_substrate::surfaces::SurfaceAppDefinition, LoomError>> {
        let definition = match app {
            apps::TICKET_DETAILS_APP => loom_substrate::surfaces::core_surface_app(
                loom_substrate::surfaces::CoreSurfaceAppKind::TicketDetails,
                workspace,
            ),
            apps::BOARD_APP => loom_substrate::surfaces::core_surface_app(
                loom_substrate::surfaces::CoreSurfaceAppKind::Board,
                workspace,
            ),
            apps::ROADMAP_APP => loom_substrate::surfaces::catalog_surface_app(
                loom_substrate::surfaces::CatalogSurfaceAppKind::Roadmap,
                workspace,
            ),
            apps::SPRINT_PLANNER_APP => loom_substrate::surfaces::catalog_surface_app(
                loom_substrate::surfaces::CatalogSurfaceAppKind::SprintPlanner,
                workspace,
            ),
            apps::BACKLOG_TRIAGE_APP => loom_substrate::surfaces::catalog_surface_app(
                loom_substrate::surfaces::CatalogSurfaceAppKind::BacklogTriage,
                workspace,
            ),
            apps::DASHBOARDS_APP => loom_substrate::surfaces::catalog_surface_app(
                loom_substrate::surfaces::CatalogSurfaceAppKind::Dashboards,
                workspace,
            ),
            _ => return None,
        };
        Some(
            definition.map(|definition| Self::source_backed_ticket_app_definition(app, definition)),
        )
    }

    fn source_backed_ticket_app_definition(
        app: &str,
        mut definition: loom_substrate::surfaces::SurfaceAppDefinition,
    ) -> loom_substrate::surfaces::SurfaceAppDefinition {
        let (read_tools, write_tools): (&[&str], &[&str]) = match app {
            apps::TICKET_DETAILS_APP => (
                &[
                    "tickets_get",
                    "tickets_relations",
                    "tickets_comments",
                    "tickets_history",
                    "substrate_refs",
                ],
                &[
                    "tickets_update",
                    "tickets_delete",
                    "tickets_comment_add",
                    "tickets_comment_update",
                    "tickets_comment_delete",
                    "tickets_relation_set",
                    "tickets_relation_remove",
                ],
            ),
            apps::BOARD_APP => (
                &["tickets_get", "tickets_history", "lanes_list"],
                &["tickets_update", "lanes_ticket_add", "lanes_ticket_remove"],
            ),
            apps::ROADMAP_APP => (
                &[
                    "tickets_get",
                    "tickets_relations",
                    "tickets_comments",
                    "tickets_history",
                    "substrate_refs",
                ],
                &[
                    "tickets_update",
                    "tickets_delete",
                    "tickets_comment_add",
                    "tickets_comment_update",
                    "tickets_comment_delete",
                    "tickets_relation_set",
                    "tickets_relation_remove",
                ],
            ),
            apps::SPRINT_PLANNER_APP => (
                &["tickets_get", "tickets_history", "lanes_list"],
                &["tickets_update", "lanes_ticket_add"],
            ),
            apps::BACKLOG_TRIAGE_APP => (
                &["tickets_get", "tickets_history"],
                &["tickets_create", "tickets_update"],
            ),
            apps::DASHBOARDS_APP => (&["tickets_history", "lanes_list"], &[]),
            _ => (&[], &[]),
        };
        definition.read_tools = read_tools.iter().map(|tool| (*tool).to_string()).collect();
        definition.write_tools = write_tools.iter().map(|tool| (*tool).to_string()).collect();
        definition
    }

    fn internal_chat_app_data(
        &self,
        workspace: &str,
        app: &str,
        instance: Option<&str>,
    ) -> Result<Value, ErrorData> {
        let selected = self.mcp.read_workspace_get(workspace).map_err(err)?;
        let profile_id = workspace_profile_id(&self.mcp, workspace).map_err(err)?;
        let app_uri =
            apps::app_uri_with_instance(workspace, app, instance, self.binding.workspace.is_some());
        let (instance_channel, instance_thread) = Self::chat_app_instance(app, instance)?;
        let channels = match self.mcp.read_chat_channels(workspace, &profile_id) {
            Ok(channels) => channels,
            Err(e) if e.code == Code::NotFound => Vec::new(),
            Err(e) => return Err(err(e)),
        };
        let selected_channel =
            instance_channel.or_else(|| channels.first().map(|channel| channel.channel_id.clone()));
        let mut channel = Value::Null;
        let mut cursor = Value::Null;
        let mut presence = json!([]);
        let mut events = json!([]);
        let mut emoji = Value::Null;
        let mut selected_thread = Value::Null;
        if let Some(channel_id) = selected_channel.as_deref() {
            let channel_summary = self
                .mcp
                .read_chat_messages(workspace, &profile_id, channel_id)
                .map_err(err)?;
            if let Some(thread_id) = instance_thread.as_deref()
                && let Some(thread) = channel_summary
                    .threads
                    .iter()
                    .find(|thread| thread.thread_id == thread_id)
            {
                selected_thread = serde_json::to_value(thread)
                    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
            }
            channel = serde_json::to_value(channel_summary)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
            cursor = serde_json::to_value(
                self.mcp
                    .read_chat_cursor(workspace, &profile_id, channel_id)
                    .map_err(err)?,
            )
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
            presence = serde_json::to_value(
                self.chat_presence_list(workspace, &profile_id, channel_id)
                    .map_err(err)?,
            )
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
            events = serde_json::to_value(
                self.mcp
                    .read_chat_fetch_events(workspace, &profile_id, channel_id, 1, 50)
                    .map_err(err)?,
            )
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        }
        if matches!(app, apps::CHAT_CHANNEL_APP | apps::CHAT_PRESENCE_APP) {
            emoji = serde_json::to_value(
                self.mcp
                    .read_chat_emoji_registry(workspace, &profile_id)
                    .map_err(err)?,
            )
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        }
        Ok(json!({
            "app_uri": app_uri,
            "workspace": selected,
            "profile_id": profile_id,
            "definition": Self::chat_app_definition(app, workspace),
            "channels": channels,
            "channel": channel,
            "selected_thread": selected_thread,
            "cursor": cursor,
            "presence": presence,
            "events": events,
            "emoji": emoji,
        }))
    }

    fn chat_app_id(app: &str) -> bool {
        matches!(
            app,
            apps::CHAT_CHANNEL_APP
                | apps::CHAT_THREAD_APP
                | apps::CHAT_TASKS_APP
                | apps::CHAT_PRESENCE_APP
                | apps::CHAT_HANDOFFS_APP
        )
    }

    fn chat_app_instance(
        app: &str,
        instance: Option<&str>,
    ) -> Result<(Option<String>, Option<String>), ErrorData> {
        let Some(instance) = instance else {
            return Ok((None, None));
        };
        let parts = instance.split('/').collect::<Vec<_>>();
        match (app, parts.as_slice()) {
            (
                apps::CHAT_CHANNEL_APP
                | apps::CHAT_TASKS_APP
                | apps::CHAT_PRESENCE_APP
                | apps::CHAT_HANDOFFS_APP,
                ["channel", channel_id],
            ) => Ok((Some((*channel_id).to_string()), None)),
            (apps::CHAT_THREAD_APP, ["channel", channel_id, "thread", thread_id]) => Ok((
                Some((*channel_id).to_string()),
                Some((*thread_id).to_string()),
            )),
            _ => Err(ErrorData::invalid_params(
                format!("invalid Chat app instance {app}/{instance}"),
                None,
            )),
        }
    }

    fn chat_app_definition(app: &str, workspace: &str) -> Value {
        let (display_name, projection_refs, read_tools, write_tools, subscription_refs): (
            &str,
            &[&str],
            &[&str],
            &[&str],
            &[&str],
        ) = match app {
            apps::CHAT_CHANNEL_APP => (
                "Chat Channel",
                &["view:chat.channel", "view:chat.cursor"],
                &[
                    "chat_channels",
                    "chat_messages",
                    "chat_fetch_events",
                    "chat_cursor",
                    "chat_emoji_list",
                ],
                &[
                    "chat_post_message",
                    "chat_edit_message",
                    "chat_redact_message",
                    "chat_add_reaction",
                    "chat_remove_reaction",
                    "chat_create_thread",
                    "chat_update_cursor",
                ],
                &["changes:chat"],
            ),
            apps::CHAT_THREAD_APP => (
                "Chat Thread",
                &["view:chat.thread"],
                &["chat_messages", "chat_fetch_events"],
                &[
                    "chat_post_message",
                    "chat_edit_message",
                    "chat_add_reaction",
                ],
                &["changes:chat"],
            ),
            apps::CHAT_TASKS_APP => (
                "Chat Tasks",
                &["view:chat.tasks"],
                &["chat_messages", "chat_fetch_events"],
                &["chat_create_task", "chat_claim_task", "chat_complete_task"],
                &["changes:chat"],
            ),
            apps::CHAT_PRESENCE_APP => (
                "Chat Presence",
                &["view:chat.presence"],
                &["chat_channels", "chat_presence"],
                &["chat_set_presence"],
                &["changes:chat.presence"],
            ),
            apps::CHAT_HANDOFFS_APP => (
                "Chat Handoffs",
                &["view:chat.handoffs", "view:chat.agent-invocations"],
                &["chat_messages", "chat_fetch_events"],
                &[
                    "chat_invoke_agent",
                    "chat_agent_reply",
                    "chat_request_handoff",
                ],
                &["changes:chat"],
            ),
            _ => ("Chat", &[], &[], &[], &[]),
        };
        json!({
            "app_id": app,
            "display_name": display_name,
            "resource_uri": apps::app_uri(workspace, app, false),
            "projection_refs": projection_refs,
            "read_tools": read_tools,
            "write_tools": write_tools,
            "subscription_refs": subscription_refs
        })
    }

    fn internal_drive_app_data(
        &self,
        workspace: &str,
        app: &str,
        instance: Option<&str>,
    ) -> Result<Value, ErrorData> {
        let selected = self.mcp.read_workspace_get(workspace).map_err(err)?;
        let profile_id = workspace_profile_id(&self.mcp, workspace).map_err(err)?;
        let app_uri =
            apps::app_uri_with_instance(workspace, app, instance, self.binding.workspace.is_some());
        let (folder_id, file_id) = Self::drive_app_instance(app, instance)?;
        let folder_id = folder_id.as_deref().unwrap_or("root");
        let folder = self
            .mcp
            .read_drive_list(workspace, &profile_id, folder_id)
            .map_err(err)?;
        let mut selected_file = Value::Null;
        let mut file_bytes = Value::Null;
        let mut versions = json!([]);
        if let Some(file_id) = file_id.as_deref() {
            selected_file = json!({ "file_id": file_id });
            file_bytes = serde_json::to_value(
                self.mcp
                    .read_drive_read(workspace, &profile_id, file_id)
                    .map_err(err)?,
            )
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
            versions = serde_json::to_value(
                self.mcp
                    .read_drive_list_versions(workspace, &profile_id, file_id)
                    .map_err(err)?,
            )
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        }
        let conflicts = self
            .mcp
            .read_drive_list_conflicts(workspace, &profile_id)
            .map(|items| {
                serde_json::to_value(items)
                    .map_err(|e| ErrorData::internal_error(e.to_string(), None))
            })
            .unwrap_or_else(Self::optional_drive_admin_items)?;
        let shares = self
            .mcp
            .read_drive_list_shares(workspace, &profile_id)
            .map(|items| {
                serde_json::to_value(items)
                    .map_err(|e| ErrorData::internal_error(e.to_string(), None))
            })
            .unwrap_or_else(Self::optional_drive_admin_items)?;
        let retention = self
            .mcp
            .read_drive_list_retention(workspace, &profile_id)
            .map(|items| {
                serde_json::to_value(items)
                    .map_err(|e| ErrorData::internal_error(e.to_string(), None))
            })
            .unwrap_or_else(Self::optional_drive_admin_items)?;
        Ok(json!({
            "app_uri": app_uri,
            "workspace": selected,
            "profile_id": profile_id,
            "definition": Self::drive_app_definition(app, workspace),
            "folder": folder,
            "stat": Value::Null,
            "selected_file": selected_file,
            "file_bytes": file_bytes,
            "versions": versions,
            "conflicts": conflicts,
            "shares": shares,
            "retention": retention,
            "lease_tools": Self::drive_lease_tool_descriptions()
        }))
    }

    fn optional_drive_admin_items(error: LoomError) -> Result<Value, ErrorData> {
        if matches!(error.code, Code::NotFound | Code::PermissionDenied) {
            Ok(json!([]))
        } else {
            Err(err(error))
        }
    }

    fn drive_app_id(app: &str) -> bool {
        matches!(
            app,
            apps::DRIVE_BROWSER_APP
                | apps::DRIVE_PREVIEW_APP
                | apps::DRIVE_SHARING_APP
                | apps::DRIVE_CONFLICTS_APP
                | apps::DRIVE_RETENTION_APP
        )
    }

    fn drive_app_instance(
        app: &str,
        instance: Option<&str>,
    ) -> Result<(Option<String>, Option<String>), ErrorData> {
        let Some(instance) = instance else {
            return Ok((None, None));
        };
        let parts = instance.split('/').collect::<Vec<_>>();
        match (app, parts.as_slice()) {
            (apps::DRIVE_BROWSER_APP, ["folder", folder_id]) => {
                Ok((Some((*folder_id).to_string()), None))
            }
            (apps::DRIVE_PREVIEW_APP, ["file", file_id]) => {
                Ok((None, Some((*file_id).to_string())))
            }
            _ => Err(ErrorData::invalid_params(
                format!("invalid Drive app instance {app}/{instance}"),
                None,
            )),
        }
    }

    fn drive_app_definition(app: &str, workspace: &str) -> Value {
        let (display_name, projection_refs, read_tools, write_tools, subscription_refs): (
            &str,
            &[&str],
            &[&str],
            &[&str],
            &[&str],
        ) = match app {
            apps::DRIVE_BROWSER_APP => (
                "Drive Browser",
                &["view:drive.folder", "view:drive.file"],
                &[
                    "drive_list",
                    "drive_stat",
                    "drive_read",
                    "drive_list_versions",
                ],
                &[
                    "drive_create_folder",
                    "drive_create_upload",
                    "drive_upload_chunk",
                    "drive_commit_upload",
                    "drive_rename",
                    "drive_move",
                    "drive_delete",
                ],
                &["changes:drive"],
            ),
            apps::DRIVE_PREVIEW_APP => (
                "Drive Preview",
                &["view:drive.file", "view:drive.versions"],
                &["drive_read", "drive_list_versions"],
                &[
                    "drive_create_upload",
                    "drive_upload_chunk",
                    "drive_commit_upload",
                ],
                &["changes:drive"],
            ),
            apps::DRIVE_SHARING_APP => (
                "Drive Sharing",
                &["view:drive.shares"],
                &["drive_list_shares"],
                &[
                    "drive_grant_share",
                    "drive_revoke_share",
                    "drive_apply_share_expiry",
                ],
                &["changes:drive.shares"],
            ),
            apps::DRIVE_CONFLICTS_APP => (
                "Drive Conflicts",
                &["view:drive.conflicts"],
                &["drive_list_conflicts"],
                &["drive_resolve_conflict"],
                &["changes:drive.conflicts"],
            ),
            apps::DRIVE_RETENTION_APP => (
                "Drive Retention",
                &["view:drive.retention"],
                &["drive_list_retention"],
                &[
                    "drive_pin_retention",
                    "drive_unpin_retention",
                    "drive_apply_retention",
                ],
                &["changes:drive.retention"],
            ),
            _ => ("Drive", &[], &[], &[], &[]),
        };
        json!({
            "app_id": app,
            "display_name": display_name,
            "resource_uri": apps::app_uri(workspace, app, false),
            "projection_refs": projection_refs,
            "read_tools": read_tools,
            "write_tools": write_tools,
            "subscription_refs": subscription_refs
        })
    }

    fn drive_lease_tool_descriptions() -> Vec<Value> {
        vec![
            json!({
                "tool": "drive_acquire_lease",
                "target": "file or folder",
                "description": "Acquire an attached-daemon write-intent lease"
            }),
            json!({
                "tool": "drive_refresh_lease",
                "target": "file or folder",
                "description": "Refresh a live write-intent lease"
            }),
            json!({
                "tool": "drive_release_lease",
                "target": "file or folder",
                "description": "Release a write-intent lease"
            }),
            json!({
                "tool": "drive_break_lease",
                "target": "file or folder",
                "description": "Admin-break a write-intent lease"
            }),
        ]
    }

    fn internal_meetings_app_data(
        &self,
        workspace: &str,
        app: &str,
        instance: Option<&str>,
    ) -> Result<Value, ErrorData> {
        let selected = self.mcp.read_workspace_get(workspace).map_err(err)?;
        let profile_id = workspace_profile_id(&self.mcp, workspace).map_err(err)?;
        let app_uri =
            apps::app_uri_with_instance(workspace, app, instance, self.binding.workspace.is_some());
        let definition = self
            .meetings_app_definition(app, workspace)
            .ok_or_else(|| ErrorData::invalid_params(format!("unknown Meetings app {app}"), None))?
            .map_err(err)?;
        let list = match self.mcp.read_meetings_list(workspace, &profile_id, 50, 0) {
            Ok(list) => serde_json::to_value(list)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
            Err(e) if e.code == Code::NotFound => json!({
                "workspace_id": profile_id,
                "total": 0,
                "offset": 0,
                "limit": 50,
                "meetings": []
            }),
            Err(e) => return Err(err(e)),
        };
        let selected_meeting_id = Self::meetings_app_instance(app, instance)?.or_else(|| {
            list["meetings"][0]["meeting_id"]
                .as_str()
                .map(str::to_string)
        });
        let meeting = match selected_meeting_id.as_deref() {
            Some(meeting_id) => serde_json::to_value(
                self.mcp
                    .read_meetings_get(workspace, &profile_id, meeting_id)
                    .map_err(err)?,
            )
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
            None => Value::Null,
        };
        let projection = match self
            .mcp
            .read_meetings_projection_outputs(workspace, &profile_id)
        {
            Ok(projection) => serde_json::to_value(projection)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
            Err(e) if e.code == Code::NotFound => json!({
                "workspace_id": profile_id,
                "profile_root": null,
                "outputs": [],
                "output_set_cbor_hex": ""
            }),
            Err(e) => return Err(err(e)),
        };
        let review = match self
            .mcp
            .read_meetings_extraction_review(workspace, &profile_id)
        {
            Ok(review) => serde_json::to_value(review)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
            Err(e) if e.code == Code::NotFound => json!({
                "workspace_id": profile_id,
                "suggested_annotation_ids": [],
                "accepted_annotation_ids": [],
                "rejected_annotation_ids": [],
                "vocabulary_terms": 0,
                "review_cbor_hex": ""
            }),
            Err(e) => return Err(err(e)),
        };
        Ok(json!({
            "app_uri": app_uri,
            "workspace": selected,
            "profile_id": profile_id,
            "definition": definition,
            "list": list,
            "meeting": meeting,
            "projection": projection,
            "review": review,
            "import_coverage": {
                "status": "source-backed list and import tool",
                "import_tool": "meetings_import_snapshot",
                "imported_rows": list["total"].as_u64().unwrap_or(0),
                "gaps": []
            },
            "access_audit": {
                "status": "target",
                "read_policy": "shared Studio ACL",
                "reason": "Meetings-specific audit projection is not promoted yet"
            }
        }))
    }

    fn meetings_app_id(app: &str) -> bool {
        matches!(
            app,
            apps::MEETING_DETAILS_APP
                | apps::MEMORY_GRAPH_APP
                | apps::EXTRACTION_REVIEW_APP
                | apps::MEETING_SEARCH_APP
                | apps::IMPORT_COVERAGE_APP
                | apps::ACCESS_AUDIT_APP
        )
    }

    fn meetings_app_instance(
        app: &str,
        instance: Option<&str>,
    ) -> Result<Option<String>, ErrorData> {
        let Some(instance) = instance else {
            return Ok(None);
        };
        let (kind, id) = instance.split_once('/').ok_or_else(|| {
            ErrorData::invalid_params(format!("invalid Meetings app instance {instance}"), None)
        })?;
        if app != apps::MEETING_DETAILS_APP || kind != "meeting" || id.is_empty() {
            return Err(ErrorData::invalid_params(
                format!("invalid Meetings app instance {app}/{instance}"),
                None,
            ));
        }
        Ok(Some(id.to_string()))
    }

    fn meetings_app_definition(
        &self,
        app: &str,
        workspace: &str,
    ) -> Option<Result<Value, LoomError>> {
        let kind = match app {
            apps::MEETING_DETAILS_APP => {
                loom_substrate::surfaces::MeetingMemorySurfaceAppKind::MeetingDetails
            }
            apps::MEMORY_GRAPH_APP => {
                loom_substrate::surfaces::MeetingMemorySurfaceAppKind::MemoryGraph
            }
            apps::EXTRACTION_REVIEW_APP => {
                loom_substrate::surfaces::MeetingMemorySurfaceAppKind::ExtractionReview
            }
            apps::MEETING_SEARCH_APP => {
                loom_substrate::surfaces::MeetingMemorySurfaceAppKind::MeetingSearch
            }
            apps::IMPORT_COVERAGE_APP => {
                loom_substrate::surfaces::MeetingMemorySurfaceAppKind::ImportCoverage
            }
            apps::ACCESS_AUDIT_APP => {
                loom_substrate::surfaces::MeetingMemorySurfaceAppKind::AccessAudit
            }
            _ => return None,
        };
        Some(
            loom_substrate::surfaces::meeting_memory_surface_app(kind, workspace)
                .map(|definition| Self::source_backed_meetings_app_definition(app, definition)),
        )
    }

    fn source_backed_meetings_app_definition(
        app: &str,
        definition: loom_substrate::surfaces::SurfaceAppDefinition,
    ) -> Value {
        let (read_tools, write_tools): (&[&str], &[&str]) = match app {
            apps::MEETING_DETAILS_APP => (
                &[
                    "meetings_list",
                    "meetings_get",
                    "meetings_projection_outputs",
                    "meetings_extraction_review",
                ],
                &[
                    "meetings_accept_annotation",
                    "meetings_reject_annotation",
                    "meetings_add_promotion",
                    "meetings_promote_task_to_ticket",
                    "meetings_promote_decision_to_decision_log",
                    "meetings_promote_question_to_lifecycle",
                    "meetings_promote_artifact_to_reference_artifact",
                    "meetings_promote_reference_to_reference_artifact",
                ],
            ),
            apps::MEMORY_GRAPH_APP => (
                &[
                    "meetings_list",
                    "meetings_get",
                    "meetings_projection_outputs",
                ],
                &["meetings_add_entity_merge", "meetings_add_promotion"],
            ),
            apps::EXTRACTION_REVIEW_APP => (
                &["meetings_extraction_review", "meetings_get"],
                &[
                    "meetings_accept_annotation",
                    "meetings_reject_annotation",
                    "meetings_propose_vocabulary",
                    "meetings_accept_vocabulary",
                    "meetings_reject_vocabulary",
                    "meetings_add_entity_merge",
                    "meetings_add_promotion",
                ],
            ),
            apps::MEETING_SEARCH_APP => {
                (&["meetings_search", "meetings_list", "meetings_get"], &[])
            }
            apps::IMPORT_COVERAGE_APP => (
                &["meetings_list", "meetings_projection_outputs"],
                &["meetings_import_snapshot"],
            ),
            apps::ACCESS_AUDIT_APP => (&["meetings_list", "meetings_get"], &[]),
            _ => (&[], &[]),
        };
        json!({
            "app_id": definition.app_id,
            "display_name": definition.display_name,
            "resource_uri": definition.resource_uri,
            "projection_refs": definition.projection_refs,
            "read_tools": read_tools,
            "write_tools": write_tools,
            "subscription_refs": definition.subscription_refs
        })
    }

    fn directed_graph_catalog_nodes(
        catalog: &[loom_substrate::surfaces::SurfaceAppDefinition],
    ) -> Vec<Value> {
        let mut nodes = BTreeMap::new();
        for definition in catalog {
            nodes.insert(
                definition.app_id.clone(),
                json!({
                    "id": definition.app_id,
                    "label": definition.display_name,
                    "kind": "app"
                }),
            );
            for id in &definition.projection_refs {
                nodes
                    .entry(id.clone())
                    .or_insert_with(|| json!({ "id": id, "label": id, "kind": "projection" }));
            }
            for id in &definition.read_tools {
                nodes
                    .entry(id.clone())
                    .or_insert_with(|| json!({ "id": id, "label": id, "kind": "read_tool" }));
            }
            for id in &definition.write_tools {
                nodes
                    .entry(id.clone())
                    .or_insert_with(|| json!({ "id": id, "label": id, "kind": "write_tool" }));
            }
            for id in &definition.elicitation_schema_refs {
                nodes.entry(id.clone()).or_insert_with(
                    || json!({ "id": id, "label": id, "kind": "elicitation_schema" }),
                );
            }
            for id in &definition.prompt_handoff_refs {
                nodes
                    .entry(id.clone())
                    .or_insert_with(|| json!({ "id": id, "label": id, "kind": "prompt" }));
            }
            for id in &definition.subscription_refs {
                nodes
                    .entry(id.clone())
                    .or_insert_with(|| json!({ "id": id, "label": id, "kind": "subscription" }));
            }
        }
        nodes.into_values().collect()
    }

    fn directed_graph_catalog_edges(
        catalog: &[loom_substrate::surfaces::SurfaceAppDefinition],
    ) -> Vec<Value> {
        let mut edges = BTreeSet::new();
        for definition in catalog {
            let app = definition.app_id.as_str();
            edges.extend(
                definition
                    .projection_refs
                    .iter()
                    .map(|id| (app.to_string(), id.clone(), "renders")),
            );
            edges.extend(
                definition
                    .read_tools
                    .iter()
                    .map(|id| (app.to_string(), id.clone(), "reads")),
            );
            edges.extend(
                definition
                    .write_tools
                    .iter()
                    .map(|id| (app.to_string(), id.clone(), "writes")),
            );
            edges.extend(
                definition
                    .elicitation_schema_refs
                    .iter()
                    .map(|id| (app.to_string(), id.clone(), "elicits")),
            );
            edges.extend(
                definition
                    .prompt_handoff_refs
                    .iter()
                    .map(|id| (app.to_string(), id.clone(), "prompts")),
            );
            edges.extend(
                definition
                    .subscription_refs
                    .iter()
                    .map(|id| (app.to_string(), id.clone(), "subscribes")),
            );
        }
        edges
            .into_iter()
            .map(|(from, to, kind)| json!({ "from": from, "to": to, "kind": kind }))
            .collect()
    }

    fn status_available(value: Value) -> Value {
        json!({ "status": "source_backed", "value": value })
    }

    fn status_target(reason: &str) -> Value {
        json!({ "status": "target", "reason": reason })
    }

    fn status_error(error: impl ToString) -> Value {
        json!({ "status": "unavailable", "error": error.to_string() })
    }

    fn status_vcs_snapshot(&self, workspace: &str) -> Value {
        match self.mcp.read_vcs_status(workspace) {
            Ok(status) => Self::status_available(status_val(&status)),
            Err(error) => Self::status_error(error),
        }
    }

    fn status_open_conflicts(&self, workspace: &str) -> Value {
        match self.mcp.read_vcs_merge_conflicts(workspace) {
            Ok(items) => Self::status_available(json!({ "items": items })),
            Err(error) => Self::status_error(error),
        }
    }

    fn status_pending_decisions(&self, workspace: &str) -> Value {
        match self.read_ask_doc(workspace, ASK_CURRENT_DOC) {
            Ok(current) => Self::status_available(json!({
                "has_pending": current.is_some(),
                "current": current
            })),
            Err(error) => Self::status_error(error.message),
        }
    }

    fn status_suggested_prompts() -> Value {
        Self::status_available(json!({
        "items": crate::prompts::prompt_surface()
            .iter()
            .map(|prompt| json!({
                "name": prompt.name,
                "area": prompt.area,
                "summary": prompt.summary
            }))
            .collect::<Vec<_>>()
        }))
    }

    fn status_changes_since_cursor(&self, workspace: &str, workspace_id: &str) -> Value {
        const MAX_STATUS_CHANGES: u32 = 10;
        let sources = [
            ("tickets", format!("oplog:1:tickets:{workspace_id}")),
            ("pages", format!("oplog:1:pages:{workspace_id}")),
        ];
        let mut out = Vec::with_capacity(sources.len());
        for (source, cursor) in sources {
            let source_value =
                match self
                    .mcp
                    .read_substrate_changes(workspace, &cursor, MAX_STATUS_CHANGES)
                {
                    Ok(batch) => {
                        let recent = batch
                            .events
                            .iter()
                            .filter_map(|event| match event {
                                crate::reads::SubstrateChangeSummary::Operation {
                                    operation_kind,
                                    sequence,
                                    app_id,
                                    scope_id,
                                    root_after,
                                    ..
                                } => Some(json!({
                                    "app_id": app_id,
                                    "scope_id": scope_id,
                                    "operation_kind": operation_kind,
                                    "sequence": sequence,
                                    "root_after": root_after
                                })),
                                crate::reads::SubstrateChangeSummary::Data { .. } => None,
                            })
                            .collect::<Vec<_>>();
                        json!({
                            "source": source,
                            "status": "source_backed",
                            "cursor": cursor,
                            "next": batch.next,
                            "count": recent.len(),
                            "recent": recent
                        })
                    }
                    Err(error) if matches!(error.code, Code::NotFound) => json!({
                        "source": source,
                        "status": "source_backed",
                        "cursor": cursor,
                        "next": cursor,
                        "count": 0,
                        "recent": []
                    }),
                    Err(error) => json!({
                        "source": source,
                        "status": "unavailable",
                        "cursor": cursor,
                        "error": error.to_string()
                    }),
                };
            out.push(source_value);
        }
        Self::status_available(json!({
            "workspace_id": workspace_id,
            "tool": "substrate_changes",
            "sources": out
        }))
    }

    fn status_active_lifecycle(&self, workspace_id: &str) -> Value {
        match self.mcp.store().read(|loom| {
            let Some(bytes) = loom
                .store()
                .control_get(&lifecycle_operation_log_key(workspace_id)?)?
            else {
                return LifecycleOperationLog::new(workspace_id, Vec::new());
            };
            LifecycleOperationLog::decode(&bytes)
        }) {
            Ok(log) => {
                let mut recent = log.records.iter().rev().take(10).collect::<Vec<_>>();
                recent.reverse();
                Self::status_available(json!({
                    "workspace_id": workspace_id,
                    "source": "lifecycle",
                    "operation_log": {
                        "cursor": format!("oplog:1:lifecycle:{workspace_id}"),
                        "count": log.records.len(),
                        "recent": recent
                            .into_iter()
                            .map(|record| json!({
                                "operation_id": record.operation_id,
                                "operation_kind": record.operation_kind,
                                "sequence": record.sequence,
                                "instance_id": record.instance_id,
                                "target_entity_id": record.target_entity_id,
                                "root_after": record.root_after.to_string()
                            }))
                            .collect::<Vec<_>>()
                    }
                }))
            }
            Err(error) => Self::status_error(error),
        }
    }

    fn status_review_comment_ownership(
        &self,
        workspace: &str,
        workspace_id: &str,
        principal: &str,
    ) -> Value {
        let inline_comments = Self::status_target(
            "generic inline comment ownership requires a promoted annotation projection",
        );
        match self
            .mcp
            .read_meetings_extraction_review(workspace, workspace_id)
        {
            Ok(review) => Self::status_available(json!({
                "workspace_id": workspace_id,
                "principal": principal,
                "meetings_extraction_review": {
                    "status": "source_backed",
                    "configured": true,
                    "suggested_annotation_ids": review.suggested_annotation_ids,
                    "accepted_annotation_ids": review.accepted_annotation_ids,
                    "rejected_annotation_ids": review.rejected_annotation_ids,
                    "vocabulary_terms": review.vocabulary_terms,
                    "review_cbor_hex": review.review_cbor_hex
                },
                "unresolved_inline_comments": inline_comments
            })),
            Err(error) if matches!(error.code, Code::NotFound) => Self::status_available(json!({
                "workspace_id": workspace_id,
                "principal": principal,
                "meetings_extraction_review": {
                    "status": "source_backed",
                    "configured": false,
                    "suggested_annotation_ids": [],
                    "accepted_annotation_ids": [],
                    "rejected_annotation_ids": [],
                    "vocabulary_terms": 0
                },
                "unresolved_inline_comments": inline_comments
            })),
            Err(error) => Self::status_error(error),
        }
    }

    fn status_ticket_views(
        &self,
        workspace: &str,
        workspace_id: &str,
        principal: &str,
    ) -> (Value, Value) {
        match self.mcp.read_tickets_list(workspace, workspace_id, None) {
            Ok(tickets) => {
                let assigned = tickets
                    .iter()
                    .filter(|ticket| Self::ticket_assignee(ticket).as_deref() == Some(principal))
                    .filter(|ticket| Self::ticket_is_open(ticket))
                    .map(Self::ticket_status_item)
                    .collect::<Vec<_>>();
                let open = tickets
                    .iter()
                    .filter(|ticket| Self::ticket_is_open(ticket))
                    .map(Self::ticket_status_item)
                    .collect::<Vec<_>>();
                let markdown =
                    Self::planning_status_markdown(workspace_id, principal, &assigned, &open);
                (
                    Self::status_available(json!({
                        "workspace_id": workspace_id,
                        "principal": principal,
                        "assigned": assigned,
                        "open": open
                    })),
                    Self::status_available(json!({
                        "workspace_id": workspace_id,
                        "media_type": "text/markdown",
                        "body": markdown
                    })),
                )
            }
            Err(error) => {
                let value = Self::status_error(error);
                (value.clone(), value)
            }
        }
    }

    fn ticket_status_item(ticket: &loom_tickets::TicketSummary) -> Value {
        json!({
            "ticket_id": &ticket.ticket_id,
            "primary_key": &ticket.primary_key,
            "project_id": &ticket.project_id,
            "ticket_type": &ticket.ticket_type,
            "external_source": &ticket.external_source,
            "external_id": &ticket.external_id,
            "title": Self::ticket_title(ticket),
            "assignee": Self::ticket_assignee(ticket),
            "assignee_display": Self::ticket_text_field(ticket, "assignee_display"),
            "status_category": Self::ticket_status_category(ticket),
            "profile_root": &ticket.profile_root
        })
    }

    fn ticket_title(ticket: &loom_tickets::TicketSummary) -> String {
        Self::ticket_text_field(ticket, "title")
            .or_else(|| Self::ticket_text_field(ticket, "summary"))
            .unwrap_or_default()
    }

    fn ticket_assignee(ticket: &loom_tickets::TicketSummary) -> Option<String> {
        Self::ticket_text_field(ticket, "assignee")
    }

    fn ticket_status_category(ticket: &loom_tickets::TicketSummary) -> Option<String> {
        Self::ticket_text_field(ticket, "status_category")
            .or_else(|| Self::ticket_text_field(ticket, "status"))
    }

    fn ticket_is_open(ticket: &loom_tickets::TicketSummary) -> bool {
        !Self::ticket_status_category(ticket)
            .as_deref()
            .is_some_and(|status| matches!(status, "done" | "accepted"))
    }

    fn ticket_text_field(ticket: &loom_tickets::TicketSummary, field: &str) -> Option<String> {
        match ticket.fields.get(field)? {
            Value::String(value) => Some(value.clone()),
            Value::Object(map) => map
                .get("String")
                .or_else(|| map.get("Text"))
                .or_else(|| map.get("EnumOption"))
                .or_else(|| map.get("Principal"))
                .and_then(Value::as_str)
                .map(str::to_string),
            _ => None,
        }
    }

    fn planning_status_markdown(
        workspace_id: &str,
        principal: &str,
        assigned: &[Value],
        open: &[Value],
    ) -> String {
        let mut out = String::new();
        out.push_str("# Studio Planning\n\n");
        out.push_str(&format!(
            "Workspace: `{}`\nPrincipal: `{}`\n\n",
            Self::markdown_inline(workspace_id),
            Self::markdown_inline(principal)
        ));
        out.push_str("## Assigned Open Items\n\n");
        Self::push_markdown_items(&mut out, assigned);
        out.push_str("\n## Open Items\n\n");
        Self::push_markdown_items(&mut out, open);
        out
    }

    fn push_markdown_items(out: &mut String, items: &[Value]) {
        if items.is_empty() {
            out.push_str("- none\n");
            return;
        }
        for item in items {
            let key = item["primary_key"].as_str().unwrap_or("");
            let title = item["title"].as_str().unwrap_or("");
            let status = item["status_category"].as_str().unwrap_or("open");
            out.push_str(&format!(
                "- `{}` {} ({})\n",
                Self::markdown_inline(key),
                Self::markdown_text(title),
                Self::markdown_text(status)
            ));
        }
    }

    fn markdown_inline(value: &str) -> String {
        value.replace('`', "'").replace('\n', " ")
    }

    fn markdown_text(value: &str) -> String {
        value.replace('\n', " ")
    }

    fn studio_status_payload(
        &self,
        workspace: &str,
        principal: &str,
        summary: crate::reads::WorkspaceSummary,
    ) -> Value {
        let workspace_id = summary.id.clone();
        let active_lifecycle = self.status_active_lifecycle(&workspace_id);
        let review_comment_ownership =
            self.status_review_comment_ownership(workspace, &workspace_id, principal);
        let changes_since_cursor = self.status_changes_since_cursor(workspace, &workspace_id);
        let open_conflicts = self.status_open_conflicts(workspace);
        let pending_decisions = self.status_pending_decisions(workspace);
        let suggested_prompts = Self::status_suggested_prompts();
        let (assigned_open_items, planning_markdown_mirror) =
            self.status_ticket_views(workspace, &workspace_id, principal);
        let active_lifecycle_status = active_lifecycle["status"].clone();
        let review_comment_ownership_status = review_comment_ownership["status"].clone();
        let assigned_open_items_status = assigned_open_items["status"].clone();
        let planning_markdown_mirror_status = planning_markdown_mirror["status"].clone();
        json!({
            "workspace": {
                "id": summary.id,
                "name": summary.name,
                "facets": summary.facets,
                "head": summary.head
            },
            "principal": principal,
            "view": "studio.status",
            "sections": {
                "vcs_status": self.status_vcs_snapshot(workspace),
                "active_lifecycle": active_lifecycle,
                "assigned_open_items": assigned_open_items,
                "changes_since_cursor": changes_since_cursor,
                "open_conflicts": open_conflicts,
                "pending_decisions": pending_decisions,
                "review_comment_ownership": review_comment_ownership,
                "suggested_prompts": suggested_prompts,
                "planning_markdown_mirror": planning_markdown_mirror
            },
            "projection_status": {
                "vcs_status": "source_backed",
                "active_lifecycle": active_lifecycle_status,
                "assigned_open_items": assigned_open_items_status,
                "changes_since_cursor": changes_since_cursor["status"].clone(),
                "open_conflicts": "source_backed",
                "pending_decisions": "source_backed",
                "review_comment_ownership": review_comment_ownership_status,
                "suggested_prompts": "source_backed",
                "planning_markdown_mirror": planning_markdown_mirror_status
            }
        })
    }

    fn read_ask_doc(&self, workspace: &str, id: &str) -> Result<Option<Value>, ErrorData> {
        ask_read_doc(&self.mcp, workspace, id).map_err(err)
    }

    /// Poll an ask from the served store or remote execution path.
    fn ask_poll_current(&self, workspace: &str, id: &str) -> Result<Option<Value>, ErrorData> {
        if let Some(backend) = self.mcp.store().remote_backend() {
            let args = serde_json::to_vec(&json!({ "workspace": workspace, "id": id }))
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
            return match backend.execute_tool("ask_answers", &args) {
                Ok(bytes) => {
                    let value: Value = serde_json::from_slice(&bytes)
                        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
                    Ok(Some(value.get("value").cloned().unwrap_or(Value::Null)))
                }
                Err(e) if e.code == Code::NotFound => Ok(None),
                Err(e) => Err(err(e)),
            };
        }
        ask_poll_state(&self.mcp, workspace, id).map_err(err)
    }

    fn internal_vcs_app_data(&self, workspace: &str) -> Result<Value, ErrorData> {
        let workspaces = self.mcp.read_workspace_list().map_err(err)?;
        let selected = self.mcp.read_workspace_get(workspace).map_err(err)?;
        let status = match self.mcp.read_vcs_status(workspace) {
            Ok(status) => json!({ "ok": true, "value": status_val(&status) }),
            Err(e) => json!({ "ok": false, "error": e.to_string(), "value": null }),
        };
        let history = match self.mcp.read_vcs_log(workspace, "main") {
            Ok(items) => json!({ "ok": true, "items": items }),
            Err(e) => json!({ "ok": false, "error": e.to_string(), "items": [] }),
        };
        let tags = match self.mcp.read_vcs_tag_list(workspace) {
            Ok(items) => json!({ "ok": true, "items": items }),
            Err(e) => json!({ "ok": false, "error": e.to_string(), "items": [] }),
        };
        let conflicts = match self.mcp.read_vcs_merge_conflicts(workspace) {
            Ok(items) => json!({ "ok": true, "items": items }),
            Err(e) => json!({ "ok": false, "error": e.to_string(), "items": [] }),
        };

        Ok(json!({
            "workspace": selected,
            "workspaces": workspaces,
            "status": status,
            "history": history,
            "tags": tags,
            "conflicts": conflicts
        }))
    }

    fn list_all_resources(&self) -> Result<Vec<Resource>, ErrorData> {
        let mut capabilities = RawResource::new(
            "loom://capabilities.json".to_string(),
            "capabilities".to_string(),
        );
        capabilities.title = Some("Capability matrix".to_string());
        capabilities.description = Some("Source-owned capability matrix as JSON.".to_string());
        capabilities.mime_type = Some("application/json".to_string());
        let mut resources = vec![capabilities.no_annotation()];
        resources.extend(self.list_workspace_resources()?);
        resources.extend(self.list_app_resources()?);
        Ok(resources)
    }

    /// Read one parsed resource target through the facade (PEP-gated), with the content address as the
    /// version in `_meta`.
    fn read_target(&self, target: &ResourceTarget) -> Result<ResourceContents, ErrorData> {
        match target {
            ResourceTarget::Capabilities => {
                let json = self.mcp.read_capabilities_json(false).map_err(err)?;
                let version = self.mcp.read_blob_digest(json.as_bytes());
                text_contents(
                    "loom://capabilities.json".to_string(),
                    "application/json",
                    json,
                    &version,
                )
            }
            ResourceTarget::Workspace { workspace } => {
                let summary = self
                    .mcp
                    .read_workspace_get(workspace)
                    .map_err(err)?
                    .ok_or_else(|| not_found(&format!("loom://{workspace}/")))?;
                let json = serde_json::to_string_pretty(&summary).map_err(|e| {
                    ErrorData::internal_error(
                        format!("workspace resource serialization failed: {e}"),
                        None,
                    )
                })?;
                let version = self.mcp.read_blob_digest(json.as_bytes());
                text_contents(
                    format!("loom://{workspace}/"),
                    "application/json",
                    json,
                    &version,
                )
            }
            ResourceTarget::File { workspace, path } => {
                let bytes = self.mcp.read_fs_read_file(workspace, path).map_err(err)?;
                let version = self.mcp.read_blob_digest(&bytes);
                let uri = format!("loom://{workspace}/files/{path}");
                blob_contents(uri, "application/octet-stream", &bytes, &version)
            }
            ResourceTarget::Cas { workspace, digest } => {
                let bytes = self
                    .mcp
                    .read_cas_get(workspace, digest)
                    .map_err(err)?
                    .ok_or_else(|| not_found(&format!("loom://{workspace}/cas/{digest}")))?;
                let uri = format!("loom://{workspace}/cas/{digest}");
                blob_contents(uri, "application/octet-stream", &bytes, digest)
            }
            ResourceTarget::CalendarIcs {
                workspace,
                principal,
                collection,
                uid,
            } => {
                let ics = self
                    .mcp
                    .read_calendar_to_ics(workspace, principal, collection, uid)
                    .map_err(err)?
                    .ok_or_else(|| {
                        not_found(&format!(
                            "loom://{workspace}/calendar/{principal}/{collection}/{uid}.ics"
                        ))
                    })?;
                let version = self.mcp.read_blob_digest(ics.as_bytes());
                let uri = format!("loom://{workspace}/calendar/{principal}/{collection}/{uid}.ics");
                text_contents(uri, "text/calendar", ics, &version)
            }
            ResourceTarget::ContactsVcf {
                workspace,
                principal,
                book,
                uid,
            } => {
                let vcf = self
                    .mcp
                    .read_contacts_to_vcard(workspace, principal, book, uid)
                    .map_err(err)?
                    .ok_or_else(|| {
                        not_found(&format!(
                            "loom://{workspace}/contacts/{principal}/{book}/{uid}.vcf"
                        ))
                    })?;
                let version = self.mcp.read_blob_digest(vcf.as_bytes());
                let uri = format!("loom://{workspace}/contacts/{principal}/{book}/{uid}.vcf");
                text_contents(uri, "text/vcard", vcf, &version)
            }
            ResourceTarget::MailEml {
                workspace,
                principal,
                mailbox,
                uid,
            } => {
                let eml = self
                    .mcp
                    .read_mail_to_eml(workspace, principal, mailbox, uid)
                    .map_err(err)?
                    .ok_or_else(|| {
                        not_found(&format!(
                            "loom://{workspace}/mail/{principal}/{mailbox}/{uid}.eml"
                        ))
                    })?;
                let version = self.mcp.read_blob_digest(&eml);
                let uri = format!("loom://{workspace}/mail/{principal}/{mailbox}/{uid}.eml");
                blob_contents(uri, "message/rfc822", &eml, &version)
            }
            ResourceTarget::StudioStatus {
                workspace,
                principal,
            } => {
                let summary = self
                    .mcp
                    .read_workspace_get(workspace)
                    .map_err(err)?
                    .ok_or_else(|| {
                        not_found(&format!(
                            "loom://{workspace}/studio/views/status/principal/{principal}"
                        ))
                    })?;
                let status = self.studio_status_payload(workspace, principal, summary);
                let json = serde_json::to_string_pretty(&status).map_err(|e| {
                    ErrorData::internal_error(
                        format!("studio status resource serialization failed: {e}"),
                        None,
                    )
                })?;
                let version = self.mcp.read_blob_digest(json.as_bytes());
                let uri = format!("loom://{workspace}/studio/views/status/principal/{principal}");
                text_contents(uri, "application/json", json, &version)
            }
            ResourceTarget::SubstrateView { workspace, view_id } => {
                let view = self
                    .mcp
                    .read_substrate_view_get(workspace, view_id)
                    .map_err(err)?
                    .ok_or_else(|| {
                        not_found(&format!(
                            "loom://{workspace}/substrate/views/{view_id}.json"
                        ))
                    })?;
                let json = serde_json::to_string_pretty(&view).map_err(|e| {
                    ErrorData::internal_error(
                        format!("substrate view resource serialization failed: {e}"),
                        None,
                    )
                })?;
                let version = self.mcp.read_blob_digest(json.as_bytes());
                let uri = format!("loom://{workspace}/substrate/views/{view_id}.json");
                text_contents(uri, "application/json", json, &version)
            }
            ResourceTarget::SubstrateRefs { workspace, target } => {
                let refs = self
                    .mcp
                    .read_substrate_refs(workspace, target)
                    .map_err(err)?;
                let json = serde_json::to_string_pretty(&refs).map_err(|e| {
                    ErrorData::internal_error(
                        format!("substrate refs resource serialization failed: {e}"),
                        None,
                    )
                })?;
                let version = self.mcp.read_blob_digest(json.as_bytes());
                let uri = format!("loom://{workspace}/substrate/refs/{target}.json");
                text_contents(uri, "application/json", json, &version)
            }
            ResourceTarget::App(target) => {
                let (mut html, meta) = self
                    .mcp
                    .read_mcp_app_html(&target.workspace, &target.app)
                    .map_err(err)?;
                let version = if meta.processing == "templates" {
                    let source_path = if target.internal {
                        apps::internal_app_source_path(&target.app)
                            .unwrap_or(apps::INDEX_FILE)
                            .to_string()
                    } else {
                        apps::index_path(&target.app)
                    };
                    let bindings = self.app_template_bindings(target, &meta)?;
                    let rendered = loom_templates::TemplateProcessor::new()
                        .render(source_path, &html, &bindings)
                        .map_err(|e| {
                            ErrorData::internal_error(
                                format!("template rendering failed for app {}: {e}", target.app),
                                None,
                            )
                        })?;
                    html = rendered.html;
                    self.mcp.read_blob_digest(html.as_bytes())
                } else {
                    self.mcp.read_blob_digest(html.as_bytes())
                };
                let uri = apps::app_uri_with_instance(
                    &target.workspace,
                    &target.app,
                    target.instance.as_deref(),
                    self.binding.workspace.is_some(),
                );
                ensure_delivered_budget(
                    &format!("resources/read {uri}"),
                    html.len(),
                    DEFAULT_RESOURCE_READ_MAX_BYTES,
                )?;
                Ok(ResourceContents::TextResourceContents {
                    uri,
                    mime_type: Some(apps::APP_MIME.to_string()),
                    text: html,
                    meta: Some(app_resource_meta(&meta, Some(&version))),
                })
            }
        }
    }

    /// The current content-address ETag of a resource URI, or `None` if it cannot be read.
    fn resource_etag(&self, uri: &str) -> Option<String> {
        let target = self.resolve_resource_uri(uri)?;
        let contents = self.read_target(&target).ok()?;
        let meta = match &contents {
            ResourceContents::TextResourceContents { meta, .. } => meta,
            ResourceContents::BlobResourceContents { meta, .. } => meta,
        };
        meta.as_ref()?
            .0
            .get("version")
            .and_then(|v| v.as_str())
            .map(str::to_string)
    }

    fn resolve_resource_uri(&self, uri: &str) -> Option<ResourceTarget> {
        parse_resource_uri_with_binding(uri, &self.binding)
    }

    fn app_watch_cursor(&self, target: &ResourceTarget) -> Result<Option<String>, ErrorData> {
        let ResourceTarget::App(app) = target else {
            return Ok(None);
        };
        Ok(Some(
            self.mcp
                .read_watch_subscribe(&app.workspace, DEFAULT_BRANCH, None, None, None, None)
                .map_err(err)?
                .cursor,
        ))
    }

    fn subscribe_resource(&self, uri: String, target: &ResourceTarget) -> Result<(), ErrorData> {
        let etag = self.resource_etag(&uri);
        let app_watch_cursor = self.app_watch_cursor(target)?;
        self.subscriptions
            .lock()
            .expect("subscriptions lock")
            .insert(uri.clone(), etag);
        let mut app_watches = self.app_watches.lock().expect("app watches lock");
        if let Some(cursor) = app_watch_cursor {
            app_watches.insert(uri, cursor);
        } else {
            app_watches.remove(&uri);
        }
        Ok(())
    }

    fn advance_app_watch(&self, uri: &str) -> Option<String> {
        let Some(ResourceTarget::App(app)) = self.resolve_resource_uri(uri) else {
            return None;
        };
        let cursor = {
            self.app_watches
                .lock()
                .expect("app watches lock")
                .get(uri)
                .cloned()
        };
        let cursor = cursor?;
        let Ok(batch) = self.mcp.read_watch_poll(&app.workspace, &cursor, 100) else {
            return None;
        };
        let has_events = !batch.events.is_empty();
        let next = batch.next;
        self.app_watches
            .lock()
            .expect("app watches lock")
            .insert(uri.to_string(), next.clone());
        has_events.then_some(next)
    }

    fn record_app_delivery(
        &self,
        uri: &str,
        version: Option<String>,
        source_cursor: Option<String>,
    ) {
        let _ = self
            .delivery
            .lock()
            .expect("delivery lock")
            .produce_app_update(uri, version, source_cursor, crate::now_ms());
    }

    pub fn delivery_ack(&self, stream_id: &str, subscriber_id: &str, seq: u64) -> u64 {
        self.delivery
            .lock()
            .expect("delivery lock")
            .ack(stream_id, subscriber_id, seq)
    }

    pub fn delivery_replay(
        &self,
        stream_id: &str,
        subscriber_id: &str,
        from_seq: Option<u64>,
        resume_from_ack: bool,
        limit: usize,
    ) -> loom_core::Result<DeliveryReplay> {
        self.delivery.lock().expect("delivery lock").replay(
            stream_id,
            subscriber_id,
            from_seq,
            resume_from_ack,
            limit,
            crate::now_ms(),
        )
    }

    /// Recompute subscribed resource ETags and return the URIs whose content changed.
    fn compute_changed(&self) -> Vec<String> {
        let mut changed = Vec::new();
        let uris = self
            .subscriptions
            .lock()
            .expect("subscriptions lock")
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for uri in uris {
            let source_cursor = self.advance_app_watch(&uri);
            let cur = self.resource_etag(&uri);
            let mut subs = self.subscriptions.lock().expect("subscriptions lock");
            if let Some(last) = subs.get_mut(&uri)
                && cur != *last
            {
                *last = cur.clone();
                self.record_app_delivery(&uri, cur.clone(), source_cursor);
                changed.push(uri);
            }
        }
        changed
    }

    fn resource_list_fingerprint(&self) -> Option<Vec<String>> {
        let mut uris = self
            .list_all_resources()
            .ok()?
            .into_iter()
            .map(|r| r.uri.to_string())
            .collect::<Vec<_>>();
        uris.sort();
        Some(uris)
    }

    fn compute_resource_list_changed(&self) -> bool {
        let cur = self.resource_list_fingerprint();
        let mut last = self.resource_list.lock().expect("resource list lock");
        let changed = last.as_ref().is_some_and(|prev| Some(prev) != cur.as_ref());
        *last = cur;
        changed
    }

    fn record_resource_list_fingerprint(&self, resources: &[Resource]) {
        let mut uris = resources
            .iter()
            .map(|resource| resource.uri.to_string())
            .collect::<Vec<_>>();
        uris.sort();
        *self.resource_list.lock().expect("resource list lock") = Some(uris);
        self.record_list_change_token();
    }

    fn tool_list_fingerprint(&self) -> Option<Vec<String>> {
        let mut names = self
            .list_app_launcher_tools()
            .ok()?
            .into_iter()
            .chain(self.list_regular_tools().ok()?)
            .map(|tool| tool.name.to_string())
            .collect::<Vec<_>>();
        names.sort();
        Some(names)
    }

    fn compute_tool_list_changed(&self) -> bool {
        let cur = self.tool_list_fingerprint();
        let mut last = self.tool_list.lock().expect("tool list lock");
        let changed = last.as_ref().is_some_and(|prev| Some(prev) != cur.as_ref());
        *last = cur;
        changed
    }

    fn record_tool_list_fingerprint(&self) {
        let cur = self.tool_list_fingerprint();
        *self.tool_list.lock().expect("tool list lock") = cur;
        self.record_list_change_token();
    }

    fn record_list_change_token(&self) {
        *self
            .list_change_token
            .lock()
            .expect("list change token lock") = self.mcp.store().change_token();
    }

    fn list_inventories_may_have_changed(&self) -> bool {
        let current = self.mcp.store().change_token();
        let mut previous = self
            .list_change_token
            .lock()
            .expect("list change token lock");
        let has_baseline = self
            .resource_list
            .lock()
            .expect("resource list lock")
            .is_some()
            || self.tool_list.lock().expect("tool list lock").is_some();
        let changed = has_baseline
            && match (&*previous, &current) {
                (Some(previous), Some(current)) => previous != current,
                (None, None) => true,
                _ => false,
            };
        *previous = current;
        changed
    }

    fn has_subscriptions(&self) -> bool {
        !self
            .subscriptions
            .lock()
            .expect("subscriptions lock")
            .is_empty()
    }

    /// Emit notifications for changed subscribed content and visible list membership.
    async fn emit_list_updates(&self, peer: &Peer<RoleServer>) {
        if self.has_subscriptions() {
            for uri in self.compute_changed() {
                let _ = peer
                    .notify_resource_updated(ResourceUpdatedNotificationParam { uri })
                    .await;
            }
        }
        if self.list_inventories_may_have_changed() {
            if self.compute_resource_list_changed() {
                let _ = peer.notify_resource_list_changed().await;
            }
            if self.compute_tool_list_changed() {
                let _ = peer.notify_tool_list_changed().await;
            }
        }
    }

    /// Poll subscribed resources until the session transport closes.
    async fn subscription_poll_loop(self, peer: Peer<RoleServer>) {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(2));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            if peer.is_transport_closed() {
                break;
            }
            self.emit_list_updates(&peer).await;
        }
    }
}

impl ServerHandler for LoomServer {
    /// Start this session's resource-subscription poll loop.
    async fn on_initialized(&self, context: NotificationContext<RoleServer>) {
        tokio::spawn(self.clone().subscription_poll_loop(context.peer.clone()));
    }

    /// Dispatch tools through the single progress and cancellation boundary.
    async fn call_tool(
        &self,
        mut request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let _active = self.shutdown.begin_request()?;
        // Hosts call with the sanitized (`_`) wire name; restore the canonical dotted form
        // before any visibility check or dispatch. See `sanitize_tool_name`.
        let canonical = self.canonical_tool_name(request.name.as_ref());
        if canonical.as_str() != request.name.as_ref() {
            request.name = canonical.into();
        }
        let app_tool_call = if request.name.as_ref() == "apps_call_tool" {
            let args = Value::Object(request.arguments.clone().unwrap_or_default());
            let args = serde_json::from_value::<PAppCallTool>(args).map_err(|e| {
                ErrorData::invalid_params(format!("invalid apps_call_tool arguments: {e}"), None)
            })?;
            let (tool, arguments) = self.prepare_app_tool_call(args)?;
            request.name = tool.clone().into();
            request.arguments = arguments;
            Some(tool)
        } else {
            None
        };
        // `ask_answers` falls through to its own remote-aware async handler (its bounded wait runs
        // client-side). `substrate_transact` fills bound workspace/collection defaults into explicit
        // per-op fields before server-side execution. Every other tool is routed by the catalog.
        if self.mcp.store().remote_backend().is_some() {
            let name = request.name.as_ref();
            if name == "substrate_transact" {
                return self.execute_tool_server_side(
                    name,
                    self.normalize_substrate_transact_arguments(request.arguments.clone()),
                );
            }
            if name != "ask_answers" {
                match crate::tools::remote_tool_route(name) {
                    crate::tools::RemoteToolRoute::Reject(message) => {
                        return Err(ErrorData::invalid_params(message, None));
                    }
                    crate::tools::RemoteToolRoute::ServerExecute => {
                        return self.execute_tool_server_side(name, request.arguments.clone());
                    }
                    crate::tools::RemoteToolRoute::UnaryForward => {}
                }
            }
        }
        let lifecycle_allows =
            self.mcp
                .store()
                .read(|loom| {
                    let active_tools = self.active_lifecycle_tools_in_loom(loom);
                    Ok(self.lifecycle_surface_allows_tool(
                        request.name.as_ref(),
                        active_tools.as_ref(),
                    ))
                })
                .map_err(err)?;
        if !lifecycle_allows {
            return Err(ErrorData::invalid_params(
                format!(
                    "MCP tool {} is not surfaced by the active lifecycle stage",
                    request.name
                ),
                None,
            ));
        }
        if let Some(result) =
            self.call_app_launcher_tool(request.name.as_ref(), request.arguments.clone())?
        {
            return Ok(result);
        }
        if !self.regular_tool_visible_by_name(&request.name)? {
            return Err(ErrorData::invalid_params(
                format!(
                    "MCP tool {} is not visible for the current principal",
                    request.name
                ),
                None,
            ));
        }
        self.inject_binding(&mut request);
        let token = context.meta.get_progress_token();
        let ct = context.ct.clone();
        let peer = context.peer.clone();
        let name = request.name.to_string();
        if let Some(token) = &token {
            let _ = peer
                .notify_progress(progress_param(token.clone(), false, &name))
                .await;
        }
        let tcc = ToolCallContext::new(self, request, context);
        let result = tokio::select! {
            biased;
            _ = ct.cancelled() => Err(cancelled_error(&name)),
            r = self.tool_router.call(tcc) => r,
        };
        let result = match (result, app_tool_call) {
            (Ok(inner), Some(tool)) => {
                let result = serde_json::to_value(inner).map_err(|e| {
                    ErrorData::internal_error(
                        format!("apps_call_tool result serialization failed: {e}"),
                        None,
                    )
                })?;
                Ok(CallToolResult::structured(json!({
                    "value": {
                        "tool": tool,
                        "result": result
                    }
                })))
            }
            (result, _) => result,
        };
        if result.is_ok()
            && let Some(token) = &token
        {
            let _ = peer
                .notify_progress(progress_param(token.clone(), true, &name))
                .await;
        }
        result
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let _active = self.shutdown.begin_request()?;
        let result = self.listed_model_tools_result();
        if result.is_ok() {
            self.record_tool_list_fingerprint();
        }
        result
    }

    async fn list_prompts(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        let _active = self.shutdown.begin_request()?;
        let (prompts, next_cursor) = Self::paginate(
            self.prompt_router.list_all(),
            request.and_then(|r| r.cursor),
        )?;
        Ok(ListPromptsResult {
            prompts,
            next_cursor,
            ..Default::default()
        })
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, ErrorData> {
        let _active = self.shutdown.begin_request()?;
        self.prompt_router
            .get_prompt(PromptContext::new(
                self,
                request.name,
                request.arguments,
                context,
            ))
            .await
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        let canonical = self.canonical_tool_name(name);
        let name = canonical.as_str();
        if let Some(tool) = self.get_app_launcher_tool(name) {
            return Some(tool);
        }
        if self.regular_tool_visible_by_name(name).ok()? {
            self.tool_router.get(name).cloned()
        } else {
            None
        }
    }

    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        let mut server_info = Implementation::new("loom", env!("CARGO_PKG_VERSION"));
        server_info.title = Some("Loom MCP".to_string());
        info.server_info = server_info;
        info.capabilities = ServerCapabilities::builder()
            .enable_extensions_with(app_extension_capabilities())
            .enable_tools()
            .enable_tool_list_changed()
            .enable_prompts()
            .enable_resources()
            .enable_resources_subscribe()
            .enable_resources_list_changed()
            .enable_completions()
            .build();
        info.instructions = Some(
            "Uldren Loom MCP host: area-scoped tools, curated prompts, loom:// resources, and ui:// \
             MCP Apps resources, all through the engine policy enforcement point. Use apps.* tools \
             to author and inspect Loom-backed MCP Apps."
                .to_string(),
        );
        info
    }

    async fn list_resources(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let _active = self.shutdown.begin_request()?;
        let all = self.list_all_resources()?;
        self.record_resource_list_fingerprint(&all);
        let (resources, next_cursor) = Self::paginate(all, request.and_then(|r| r.cursor))?;
        Ok(ListResourcesResult {
            resources,
            next_cursor,
            ..Default::default()
        })
    }

    async fn list_resource_templates(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        let _active = self.shutdown.begin_request()?;
        let (resource_templates, next_cursor) =
            Self::paginate(self.resource_templates(), request.and_then(|r| r.cursor))?;
        Ok(ListResourceTemplatesResult {
            resource_templates,
            next_cursor,
            ..Default::default()
        })
    }

    async fn complete(
        &self,
        request: CompleteRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CompleteResult, ErrorData> {
        let _active = self.shutdown.begin_request()?;
        let values = self.complete_argument(&request.r#ref, &request.argument)?;
        let total = values.len() as u32;
        Ok(CompleteResult::new(CompletionInfo {
            values,
            total: Some(total),
            has_more: Some(false),
        }))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, ErrorData> {
        let _active = self.shutdown.begin_request()?;
        let target = self.resolve_resource_uri(&request.uri).ok_or_else(|| {
            ErrorData::invalid_params(format!("unknown resource uri: {}", request.uri), None)
        })?;
        Ok(ReadResourceResult::new(vec![self.read_target(&target)?]))
    }

    async fn subscribe(
        &self,
        request: SubscribeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        let _active = self.shutdown.begin_request()?;
        let target = self.resolve_resource_uri(&request.uri).ok_or_else(|| {
            ErrorData::invalid_params(format!("unknown resource uri: {}", request.uri), None)
        })?;
        self.subscribe_resource(request.uri, &target)
    }

    async fn unsubscribe(
        &self,
        request: UnsubscribeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        let _active = self.shutdown.begin_request()?;
        self.subscriptions
            .lock()
            .expect("subscriptions lock")
            .remove(&request.uri);
        self.app_watches
            .lock()
            .expect("app watches lock")
            .remove(&request.uri);
        Ok(())
    }
}

/// Serve the MCP host over stdio (owner mode) until the client disconnects. Every call still passes
/// the engine policy enforcement point; a passwordless loom resolves the caller to the owner.
pub async fn serve_stdio(mcp: LoomMcp, binding: Binding) -> Result<(), Box<dyn std::error::Error>> {
    let mcp = Arc::new(mcp);
    let shutdown = ShutdownController::new();
    let server = LoomServer::with_binding_and_shutdown(mcp.clone(), binding, shutdown.clone());
    // The subscription poll loop is started from `on_initialized` (transport-agnostic), so stdio and
    // Streamable HTTP behave identically; no manual spawn/abort here.
    let running = server.serve(stdio_transport()).await?;
    if mcp.has_attached_daemon_session() {
        let token = running.cancellation_token();
        tokio::spawn(async move {
            wait_until_daemon_lost(mcp).await;
            shutdown.start_draining();
            let _ = tokio::time::timeout(MCP_SHUTDOWN_GRACE, shutdown.wait_idle()).await;
            token.cancel();
        });
    }
    running.waiting().await?;
    Ok(())
}

fn stdio_transport() -> (tokio::io::Stdin, tokio::io::Stdout) {
    (tokio::io::stdin(), tokio::io::stdout())
}

/// Build the Streamable HTTP tower service that serves the loom at `POST /mcp` in read-only owner mode.
/// Each session builds a fresh [`LoomServer`] over the shared [`LoomMcp`]; the engine PEP still gates
/// every call. Mount this on an axum router or any hyper service stack. Behind feature `http`.
#[cfg(feature = "http")]
pub fn http_service(
    mcp: std::sync::Arc<LoomMcp>,
    binding: Binding,
    stateful: bool,
) -> rmcp::transport::streamable_http_server::StreamableHttpService<
    LoomServer,
    rmcp::transport::streamable_http_server::session::local::LocalSessionManager,
> {
    let shutdown = ShutdownController::new();
    let token = tokio_util::sync::CancellationToken::new();
    http_service_with_shutdown(mcp, binding, stateful, shutdown, token)
}

#[cfg(feature = "http")]
fn http_service_with_shutdown(
    mcp: std::sync::Arc<LoomMcp>,
    mut binding: Binding,
    stateful: bool,
    shutdown: ShutdownController,
    token: tokio_util::sync::CancellationToken,
) -> rmcp::transport::streamable_http_server::StreamableHttpService<
    LoomServer,
    rmcp::transport::streamable_http_server::session::local::LocalSessionManager,
> {
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    };
    binding.allow_writes = false;
    let config = StreamableHttpServerConfig::default()
        .with_stateful_mode(stateful)
        .with_cancellation_token(token);
    StreamableHttpService::new(
        move || {
            Ok(LoomServer::with_binding_and_shutdown(
                mcp.clone(),
                binding.clone(),
                shutdown.clone(),
            ))
        },
        std::sync::Arc::new(LocalSessionManager::default()),
        config,
    )
}

/// Serve the loom over Streamable HTTP in read-only owner mode at `addr`.
#[cfg(feature = "http")]
pub async fn serve_http(
    mcp: LoomMcp,
    addr: std::net::SocketAddr,
    binding: Binding,
    stateful: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    serve_http_with_network_access(mcp, addr, binding, stateful, None).await
}

#[cfg(feature = "http")]
pub async fn serve_http_with_network_access(
    mcp: LoomMcp,
    addr: std::net::SocketAddr,
    binding: Binding,
    stateful: bool,
    network_access: Option<HttpNetworkAccess>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mcp = std::sync::Arc::new(mcp);
    let shutdown = ShutdownController::new();
    let accept_token = tokio_util::sync::CancellationToken::new();
    let service_token = tokio_util::sync::CancellationToken::new();
    let service = http_service_with_shutdown(
        mcp.clone(),
        binding,
        stateful,
        shutdown.clone(),
        service_token.clone(),
    );
    let mut app = axum::Router::new().route_service("/mcp", service);
    if let Some(network_access) = network_access {
        app = app.route_layer(axum::middleware::from_fn_with_state(
            network_access,
            http_network_access_layer,
        ));
    }
    let listener = tokio::net::TcpListener::bind(addr).await?;
    if mcp.has_attached_daemon_session() {
        let accept_token = accept_token.clone();
        tokio::spawn(async move {
            wait_until_daemon_lost(mcp).await;
            shutdown.start_draining();
            accept_token.cancel();
            let _ = tokio::time::timeout(MCP_SHUTDOWN_GRACE, shutdown.wait_idle()).await;
            service_token.cancel();
        });
    }
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(accept_token.cancelled_owned())
    .await?;
    Ok(())
}

#[cfg(feature = "http")]
async fn http_network_access_layer(
    axum::extract::ConnectInfo(peer_addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    axum::extract::State(network_access): axum::extract::State<HttpNetworkAccess>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let forwarded_for = request
        .headers()
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok());
    let forwarded = request
        .headers()
        .get("forwarded")
        .and_then(|value| value.to_str().ok());
    if !network_access(peer_addr, forwarded_for, forwarded) {
        return (axum::http::StatusCode::FORBIDDEN, "network access denied").into_response();
    }
    next.run(request).await
}

async fn wait_until_daemon_lost(mcp: Arc<LoomMcp>) {
    loop {
        tokio::time::sleep(DAEMON_LIVENESS_POLL).await;
        if mcp.ensure_attached_daemon_live().is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests;
